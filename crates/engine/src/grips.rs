//! Grip editing: dragging an object's grips (the points from [`EntityKind::grips`]) the way
//! AutoCAD does, plus the multi-functional grip modes (move, rotate, scale, mirror about a grip).
//!
//! The UI calls [`Session::grip_edit`]; every edit runs through a `grip.*` command so it is
//! undoable and reachable from scripts, the control channel and MCP.

use cadcraft_doc::{DimKind, EntityKind, Handle, LwPolyline};
use cadcraft_geom::{Arc, EPS, Mat3, Polyline, Spline, Vec2, Vec3, norm_angle};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{EngineError, Result, Session};

/// What dragging a hot grip does (AutoCAD's grip modes, cycled with Space/Enter).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GripMode {
    /// Move the grip itself (the object's shape changes).
    #[default]
    Stretch,
    /// Move the whole object by the grip displacement.
    Move,
    /// Rotate the object about the grip; the angle is from the grip to the new point.
    Rotate,
    /// Scale the object about the grip; the factor is the distance from the grip to the new point.
    Scale,
    /// Mirror the object about the line from the grip to the new point.
    Mirror,
}

fn set_xy(p: &mut Vec3, q: Vec2) {
    p.x = q.x;
    p.y = q.y;
}

fn translated(k: &EntityKind, d: Vec2) -> EntityKind {
    let mut k = k.clone();
    k.transform(&Mat3::translate(d));
    k
}

/// What a polyline grip index refers to (matches the order of `EntityKind::grips`).
enum PolyGrip {
    Vertex(usize),
    /// Midpoint of the segment starting at this vertex.
    Mid(usize),
}

fn poly_grip(p: &LwPolyline, index: usize) -> Option<PolyGrip> {
    let n = p.vertices.len();
    // Start vertices of the non-degenerate segments, in the same order as Polyline::segments().
    let count = if p.closed { n } else { n.saturating_sub(1) };
    let mut seg_starts = Vec::with_capacity(count);
    for i in 0..count {
        let (Some(v), Some(w)) = (p.vertices.get(i), p.vertices.get((i + 1) % n.max(1))) else { continue };
        if v.p.near(w.p, EPS) && v.bulge.abs() < 1e-12 {
            continue;
        }
        seg_starts.push(i);
    }
    let mut k = 0usize;
    for i in 0..n {
        if k == index {
            return Some(PolyGrip::Vertex(i));
        }
        k += 1;
        if let Some(start) = seg_starts.get(i) {
            if k == index {
                return Some(PolyGrip::Mid(*start));
            }
            k += 1;
        }
    }
    None
}

/// The shape after dragging grip `index` to `to` (STRETCH mode). `None` when the grip does not
/// exist or the result would be degenerate.
pub fn stretch_grip(kind: &EntityKind, index: usize, to: Vec2) -> Option<EntityKind> {
    if !to.is_finite() {
        return None;
    }
    let grips = kind.grips();
    let from = *grips.get(index)?;
    let delta = to - from;
    let mut k = kind.clone();
    match &mut k {
        EntityKind::Line(l) => match index {
            0 => set_xy(&mut l.a, to),
            1 => return Some(translated(kind, delta)),
            _ => set_xy(&mut l.b, to),
        },
        EntityKind::Point(_) | EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::AttDef(_) | EntityKind::Image(_) | EntityKind::Table(_) => {
            return Some(translated(kind, delta));
        }
        EntityKind::Hatch(_) => return Some(translated(kind, delta)),
        EntityKind::Circle(c) => {
            if index == 0 {
                return Some(translated(kind, delta));
            }
            let r = c.center.xy().dist(to);
            if r <= EPS {
                return None;
            }
            c.radius = r;
        }
        EntityKind::Arc(a) => {
            let g = Arc::new(a.center.xy(), a.radius, a.start, a.end);
            let (mut s, mut m, mut e) = (g.start_point(), g.mid_point(), g.end_point());
            match index {
                0 => s = to,
                1 => m = to,
                2 => e = to,
                _ => return Some(translated(kind, delta)),
            }
            let na = Arc::from_3_points(s, m, e)?;
            return Some(crate::cmd::helpers::arc(&na));
        }
        EntityKind::Ellipse(e) => {
            let c = e.center.xy();
            let mj = e.major.xy();
            let a = mj.len();
            if a <= EPS {
                return None;
            }
            let b = a * e.ratio;
            match index {
                0 => return Some(translated(kind, delta)),
                1 | 3 => {
                    let v = if index == 1 { to - c } else { c - to };
                    let l = v.len();
                    if l <= EPS {
                        return None;
                    }
                    if l >= b {
                        e.major = v.to3(0.0);
                        e.ratio = (b / l).clamp(1e-9, 1.0);
                    } else {
                        e.major = (v.normalized().perp() * b).to3(0.0);
                        e.ratio = (l / b).clamp(1e-9, 1.0);
                        shift_params(e, -std::f64::consts::FRAC_PI_2);
                    }
                }
                _ => {
                    let u = mj / a;
                    let l = (to - c).dot(u.perp()).abs();
                    if l <= EPS {
                        return None;
                    }
                    if l <= a {
                        e.ratio = (l / a).clamp(1e-9, 1.0);
                    } else {
                        e.major = (u.perp() * l).to3(0.0);
                        e.ratio = (a / l).clamp(1e-9, 1.0);
                        shift_params(e, -std::f64::consts::FRAC_PI_2);
                    }
                }
            }
        }
        EntityKind::LwPolyline(p) => {
            let n = p.vertices.len();
            match poly_grip(p, index)? {
                PolyGrip::Vertex(i) => p.vertices.get_mut(i)?.p = to,
                PolyGrip::Mid(i) => {
                    let j = (i + 1) % n.max(1);
                    let (a, b) = (p.vertices.get(i)?.p, p.vertices.get(j)?.p);
                    if p.vertices.get(i)?.bulge.abs() > 1e-12 {
                        // Arc segment: reshape through the dragged midpoint.
                        p.vertices.get_mut(i)?.bulge = crate::cmd::curves::bulge_through(a, to, b);
                    } else {
                        p.vertices.get_mut(i)?.p = a + delta;
                        p.vertices.get_mut(j)?.p = b + delta;
                    }
                }
            }
        }
        EntityKind::Polyline3d(p) => set_xy(p.points.get_mut(index)?, to),
        EntityKind::Spline(sp) => {
            if sp.fit.is_empty() {
                *sp.control.get_mut(index)? = to;
            } else {
                let mut fit = sp.fit.clone();
                *fit.get_mut(index)? = to;
                *sp = Spline::from_fit(&fit, sp.closed);
            }
        }
        EntityKind::Ray(r) | EntityKind::XLine(r) => {
            if index == 0 {
                return Some(translated(kind, delta));
            }
            let d = (to - r.base.xy()).normalized();
            if d == Vec2::ZERO {
                return None;
            }
            r.dir = d.to3(0.0);
        }
        EntityKind::Insert(i) => {
            if index == 0 {
                return Some(translated(kind, delta));
            }
            let a = i.attribs.get_mut(index - 1)?;
            let ip = a.text.insert.xy() + delta;
            set_xy(&mut a.text.insert, ip);
            if let Some(al) = &mut a.text.align_pt {
                let q = al.xy() + delta;
                set_xy(al, q);
            }
        }
        EntityKind::Dimension(d) => {
            match index {
                0 => set_xy(&mut d.p13, to),
                1 => set_xy(&mut d.p14, to),
                2 => {
                    set_xy(&mut d.defpt, to);
                    if matches!(d.kind, DimKind::Radius | DimKind::Diameter) {
                        d.user_text_pos = false;
                    }
                }
                _ => {
                    set_xy(&mut d.text_mid, to);
                    d.user_text_pos = true;
                }
            }
            d.block = None;
        }
        EntityKind::Leader(l) => set_xy(l.vertices.get_mut(index)?, to),
        EntityKind::MLeader(m) => {
            let mut idx = 0usize;
            let mut done = false;
            'outer: for l in &mut m.leaders {
                for v in l.iter_mut() {
                    if idx == index {
                        set_xy(v, to);
                        done = true;
                        break 'outer;
                    }
                    idx += 1;
                }
            }
            if !done {
                // The landing grip moves the landing and its text.
                let ld = m.landing.xy() + delta;
                set_xy(&mut m.landing, ld);
                if let Some(t) = &mut m.text {
                    let q = t.insert.xy() + delta;
                    set_xy(&mut t.insert, q);
                }
            }
        }
        EntityKind::Solid(s) | EntityKind::Trace(s) => set_xy(s.corners.get_mut(index)?, to),
        EntityKind::Face3d(f) => set_xy(f.corners.get_mut(index)?, to),
        EntityKind::Viewport(v) => {
            let opposite = *grips.get((index + 2) % 4)?;
            let w = (to.x - opposite.x).abs();
            let h = (to.y - opposite.y).abs();
            if w <= EPS || h <= EPS {
                return None;
            }
            let c = to.mid(opposite);
            set_xy(&mut v.center, c);
            v.width = w;
            v.height = h;
        }
        EntityKind::Wipeout(w) => *w.boundary.get_mut(index)? = to,
        EntityKind::Unknown(_) => return None,
    }
    Some(k)
}

fn shift_params(e: &mut cadcraft_doc::Ellipse, by: f64) {
    let full = (cadcraft_geom::ccw_sweep(e.start, e.end) - cadcraft_geom::TAU).abs() < 1e-9;
    if !full {
        e.start = norm_angle(e.start + by);
        e.end = norm_angle(e.end + by);
    }
}

/// The transform for a multi-functional grip mode about `base` with the dragged point `to`.
pub fn mode_matrix(mode: GripMode, base: Vec2, to: Vec2) -> Option<Mat3> {
    match mode {
        GripMode::Stretch => None,
        GripMode::Move => Some(Mat3::translate(to - base)),
        GripMode::Rotate => Some(Mat3::rotate_about(base, base.angle_to(to))),
        GripMode::Scale => {
            let f = base.dist(to);
            (f > EPS && f.is_finite()).then(|| Mat3::scale_about(base, f))
        }
        GripMode::Mirror => (!base.near(to, EPS)).then(|| Mat3::mirror(base, to)),
    }
}

impl Session {
    /// Drag grip `grip_index` of `handle` to `new_point` in `mode` (one undo step).
    /// The base point of the rotate/scale/mirror modes is the grip itself.
    pub fn grip_edit(&mut self, handle: Handle, grip_index: usize, new_point: Vec2, mode: GripMode) -> Result<()> {
        let to = [new_point.x, new_point.y];
        let r = match mode {
            GripMode::Stretch => self.execute("grip.move", &json!({ "handle": handle.hex(), "index": grip_index, "to": to })),
            GripMode::Move => self.execute("grip.move", &json!({ "handles": [handle.hex()], "index": grip_index, "to": to, "mode": "move" })),
            GripMode::Rotate => {
                self.execute("grip.rotate", &json!({ "handles": [handle.hex()], "baseHandle": handle.hex(), "index": grip_index, "to": to }))
            }
            GripMode::Scale => {
                self.execute("grip.scale", &json!({ "handles": [handle.hex()], "baseHandle": handle.hex(), "index": grip_index, "to": to }))
            }
            GripMode::Mirror => {
                self.execute("grip.mirror", &json!({ "handles": [handle.hex()], "baseHandle": handle.hex(), "index": grip_index, "to": to }))
            }
        };
        r.map(|_| ())
    }

    /// Grip points of an object (the indices `grip_edit` takes).
    pub fn grips_of(&self, handle: Handle) -> Result<Vec<Vec2>> {
        self.doc()?.entity(handle).map(|e| e.kind.grips()).ok_or_else(|| EngineError::Other("no such object".into()))
    }
}

/// Polyline helper kept public for the UI: segment midpoints are grips too.
pub fn polyline_grip_count(p: &LwPolyline) -> usize {
    p.vertices.len() + Polyline { vertices: p.vertices.clone(), closed: p.closed }.segments().len().min(p.vertices.len())
}

#[cfg(test)]
#[path = "grips_tests.rs"]
mod tests;
