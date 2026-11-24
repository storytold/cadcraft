//! HP-GL and HP-GL/2 plot files (`.plt`) → Drawing, through `craft_plotter::hpgl::read`.
//!
//! Every subpath becomes model-space geometry in millimetres (40 plotter units per mm, y up, the
//! plot turned back upright when RO turned it for the media):
//! straight runs an `LWPOLYLINE`, Bézier runs a cubic `SPLINE`, fills a `SOLID` hatch. Each pen
//! draws on its own layer (`PEN1`, `PEN2`, …) coloured like the pen; the pen's width becomes the
//! nearest standard lineweight, and its line type a linetype (`PLT1`, `PLT2`, …). What the HP-GL
//! reader itself left out is named in a read note.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Common, DashElement, Drawing, EntityKind, Hatch, HatchLoop, Lineweight, Linetype, LwPolyline, Space};
use cadcraft_geom::{PolyVertex, Spline, Vec2};
use craft_plotter::hpgl::{
    UNITS_PER_MM,
    plot::{LinePattern, Shape},
};
use kurbo::PathEl;

use crate::{IoError, Result};

/// How far a fill's flattened boundary may stray from its curves, in plotter units (0.01 mm).
const FILL_TOLERANCE: f64 = 0.4;

/// Read an HP-GL or HP-GL/2 plot file.
pub fn read(bytes: &[u8]) -> Result<Drawing> {
    let plot = craft_plotter::hpgl::read::read(bytes).map_err(|e| IoError::Format(format!("PLT: {e}")))?;
    let mut d = Drawing::new_metric();
    let mut patterns: Vec<Vec<f64>> = Vec::new();
    let mut too_large = 0usize;
    // A plotter driver turns the art with RO to fit the media; the drawing is read upright, in the
    // orientation it was sent in. One turn for the whole plot keeps parts drawn under another RO
    // where they lie relative to the rest.
    let upright = kurbo::Affine::rotate(-f64::from(plot.shapes.first().map_or(0, |s| s.ro)).to_radians());
    for shape in &plot.shapes {
        let common = common(&mut d, &mut patterns, shape);
        let path = upright * shape.path.clone();
        let kinds = match shape.fill {
            Some(_) => hatch(&path).into_iter().collect(),
            None => entities(&path),
        };
        for kind in kinds {
            if !finite(&kind) {
                too_large += 1;
                continue;
            }
            d.add(&Space::Model, common.clone(), kind).map_err(|e| IoError::Format(format!("PLT: {e}")))?;
        }
    }
    if too_large > 0 {
        d.read_notes.push(format!("PLT: {too_large} shapes were left out as invalid: their coordinates are too large to represent"));
    }
    d.read_notes.extend(plot.warnings.into_iter().map(|w| format!("PLT: {w}")));
    Ok(d)
}

/// The shape's pen layer, colour, lineweight and linetype, adding the layer and linetype to `d`
/// the first time they are used. The colour is ByLayer while the pen keeps its first colour.
fn common(d: &mut Drawing, patterns: &mut Vec<Vec<f64>>, shape: &Shape) -> Common {
    let [r, g, b] = shape.stroke.as_ref().map(|s| s.color).or(shape.fill.map(|(c, _)| c)).unwrap_or_default();
    // Black is colour 7, which draws black on light and white on dark backgrounds.
    let color = if [r, g, b] == [0, 0, 0] { Color::WHITE } else { Color::True(Rgb(r, g, b)) };
    let layer = format!("PEN{}", shape.pen);
    if d.ensure_layer(&layer)
        && let Some(l) = d.layer_mut(&layer)
    {
        l.color = color;
    }
    let by_layer = d.layer(&layer).is_some_and(|l| l.color == color);
    let lineweight = shape.stroke.as_ref().map_or(Lineweight::ByLayer, |s| {
        let mm100 = s.width / UNITS_PER_MM * 100.0;
        let nearest = Lineweight::STANDARD.iter().min_by(|a, b| (f64::from(**a) - mm100).abs().total_cmp(&(f64::from(**b) - mm100).abs()));
        nearest.map_or(Lineweight::ByLayer, |v| Lineweight::Mm100(*v))
    });
    let linetype = shape.stroke.as_ref().and_then(|s| s.dash.as_ref()).map_or_else(|| "ByLayer".to_string(), |p| linetype(d, patterns, p));
    Common { layer, color: if by_layer { Color::ByLayer } else { color }, lineweight, linetype, ..Common::default() }
}

/// The linetype drawing `pattern` (dashes positive, gaps negative, in mm), added as `PLT<n>` the
/// first time it is used.
// shortcut: the pattern's start offset and adaptive stretching are not kept; add them when a
// linetype can carry a phase or per-line fitting.
fn linetype(d: &mut Drawing, patterns: &mut Vec<Vec<f64>>, pattern: &LinePattern) -> String {
    let dashes: Vec<f64> =
        pattern.pattern.iter().enumerate().map(|(i, len)| if i % 2 == 0 { len / UNITS_PER_MM } else { -len / UNITS_PER_MM }).collect();
    let n = match patterns.iter().position(|p| *p == dashes) {
        Some(i) => i + 1,
        None => {
            patterns.push(dashes.clone());
            let name = format!("PLT{}", patterns.len());
            let elements = dashes.iter().map(|len| DashElement::dash(*len)).collect();
            d.linetypes.push(Linetype { name, description: "HP-GL line type".into(), pattern: elements });
            patterns.len()
        }
    };
    format!("PLT{n}")
}

/// A filled outline as a `SOLID` hatch: every subpath one loop, curves flattened.
// shortcut: a nonzero fill rule is filled by odd parity, which differs only where loops of one
// direction overlap; map it when hatches carry a winding rule.
fn hatch(path: &kurbo::BezPath) -> Option<EntityKind> {
    let mut loops: Vec<HatchLoop> = Vec::new();
    let mut points: Vec<PolyVertex> = Vec::new();
    let close = |points: &mut Vec<PolyVertex>, loops: &mut Vec<HatchLoop>| {
        if points.len() > 1 && points.first().map(|v| v.p) == points.last().map(|v| v.p) {
            points.pop();
        }
        let vertices = std::mem::take(points);
        if vertices.len() >= 3 {
            loops.push(HatchLoop { vertices, outer: loops.is_empty(), boundary_flags: None, sources: Vec::new() });
        }
    };
    kurbo::flatten(path, FILL_TOLERANCE, |el| match el {
        PathEl::MoveTo(p) => {
            close(&mut points, &mut loops);
            points.push(PolyVertex::new(Vec2::new(p.x / UNITS_PER_MM, p.y / UNITS_PER_MM)));
        }
        PathEl::LineTo(p) => points.push(PolyVertex::new(Vec2::new(p.x / UNITS_PER_MM, p.y / UNITS_PER_MM))),
        PathEl::ClosePath => close(&mut points, &mut loops),
        // `flatten` emits only moves, lines and closes.
        PathEl::QuadTo(..) | PathEl::CurveTo(..) => {}
    });
    close(&mut points, &mut loops);
    (!loops.is_empty()).then(|| {
        EntityKind::Hatch(Hatch {
            pattern: "SOLID".into(),
            solid: true,
            loops,
            scale: 1.0,
            angle: 0.0,
            associative: false,
            style: 0,
            elevation: 0.0,
            gradient: None,
            origin: Vec2::ZERO,
            background: None,
            pattern_lines: Vec::new(),
        })
    })
}


/// A path's subpaths as polylines (straight runs) and cubic splines (curve runs), in mm.
fn entities(path: &kurbo::BezPath) -> Vec<EntityKind> {
    let mm = |p: kurbo::Point| Vec2::new(p.x / UNITS_PER_MM, p.y / UNITS_PER_MM);
    let mut out = Vec::new();
    let mut run: Vec<PolyVertex> = Vec::new();
    let mut ctrl: Vec<Vec2> = Vec::new();
    let (mut start, mut cur, mut curved) = (Vec2::new(0.0, 0.0), Vec2::new(0.0, 0.0), false);
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                flush_run(&mut run, false, &mut out);
                flush_ctrl(&mut ctrl, &mut out);
                (start, cur, curved) = (mm(p), mm(p), false);
            }
            PathEl::LineTo(p) => line_to(&mut run, &mut ctrl, &mut out, &mut cur, mm(p)),
            PathEl::QuadTo(c, p) => {
                // A quadratic is the cubic with its control points two thirds of the way to `c`.
                let (c, p) = (mm(c), mm(p));
                let pts = [cur + (c - cur) * (2.0 / 3.0), p + (c - p) * (2.0 / 3.0), p];
                curve_to(&mut run, &mut ctrl, &mut out, &mut cur, pts);
                curved = true;
            }
            PathEl::CurveTo(a, b, p) => {
                curve_to(&mut run, &mut ctrl, &mut out, &mut cur, [mm(a), mm(b), mm(p)]);
                curved = true;
            }
            PathEl::ClosePath => {
                if !curved && run.first().is_some_and(|v| v.p == start) {
                    // An all-straight subpath closes as a closed polyline, without a repeated vertex.
                    if run.len() > 2 && run.last().is_some_and(|v| v.p == start) {
                        run.pop();
                    }
                    flush_run(&mut run, true, &mut out);
                } else if cur != start {
                    line_to(&mut run, &mut ctrl, &mut out, &mut cur, start);
                }
                flush_run(&mut run, false, &mut out);
                flush_ctrl(&mut ctrl, &mut out);
                cur = start;
            }
        }
    }
    flush_run(&mut run, false, &mut out);
    flush_ctrl(&mut ctrl, &mut out);
    out
}

fn line_to(run: &mut Vec<PolyVertex>, ctrl: &mut Vec<Vec2>, out: &mut Vec<EntityKind>, cur: &mut Vec2, p: Vec2) {
    flush_ctrl(ctrl, out);
    if run.is_empty() {
        run.push(PolyVertex::new(*cur));
    }
    run.push(PolyVertex::new(p));
    *cur = p;
}

fn curve_to(run: &mut Vec<PolyVertex>, ctrl: &mut Vec<Vec2>, out: &mut Vec<EntityKind>, cur: &mut Vec2, pts: [Vec2; 3]) {
    flush_run(run, false, out);
    if ctrl.is_empty() {
        ctrl.push(*cur);
    }
    ctrl.extend(pts);
    *cur = pts[2];
}

fn flush_run(run: &mut Vec<PolyVertex>, closed: bool, out: &mut Vec<EntityKind>) {
    if run.len() >= 2 {
        let vertices = std::mem::take(run);
        out.push(EntityKind::LwPolyline(LwPolyline { vertices, closed, const_width: 0.0, elevation: 0.0, plinegen: false }));
    }
    run.clear();
}

fn flush_ctrl(ctrl: &mut Vec<Vec2>, out: &mut Vec<EntityKind>) {
    if ctrl.len() >= 4 {
        out.push(EntityKind::Spline(Spline::from_control(std::mem::take(ctrl), 3)));
    }
    ctrl.clear();
}

fn finite(kind: &EntityKind) -> bool {
    match kind {
        EntityKind::LwPolyline(p) => p.vertices.iter().all(|v| v.p.is_finite()),
        EntityKind::Spline(s) => s.control.iter().all(|p| p.is_finite()),
        EntityKind::Hatch(h) => h.loops.iter().flat_map(|l| &l.vertices).all(|v| v.p.is_finite()),
        _ => true,
    }
}
