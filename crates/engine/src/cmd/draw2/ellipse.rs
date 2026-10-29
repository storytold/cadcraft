use super::{Ellipse, EntityKind, Line, Result, Session, TAU, Value, Vec2, added, bad, f64_req, point_param, point_req, v3};

// ---------------- ellipse entries ----------------

fn ellipse_from_axis(p1: Vec2, p2: Vec2, half: f64) -> Option<Ellipse> {
    let c = p1.mid(p2);
    let m = p2 - c;
    let ml = m.len();
    let half = half.abs();
    if ml < 1e-12 || half < 1e-12 || !half.is_finite() {
        return None;
    }
    Some(if half > ml { Ellipse::full(c, m.normalized().perp() * half, ml / half) } else { Ellipse::full(c, m, half / ml) })
}

fn axis_ellipse_params(cmd: &str, p: &Value) -> Result<Ellipse> {
    let p1 = point_req(cmd, p, "p1")?;
    let p2 = point_req(cmd, p, "p2")?;
    let half = match point_param(p, "p3") {
        Some(p3) => {
            let ax = Line::new(p1, p2);
            ax.project(p3).dist(p3)
        }
        None => f64_req(cmd, p, "distance")?,
    };
    ellipse_from_axis(p1, p2, half).ok_or_else(|| bad(cmd, "degenerate ellipse"))
}

fn ellipse_kind(e: &Ellipse) -> EntityKind {
    EntityKind::Ellipse(cadcraft_doc::Ellipse { center: v3(e.center), major: v3(e.major), ratio: e.ratio, start: e.start, end: e.end })
}

pub(super) fn run_ellipse_axis(s: &mut Session, p: &Value) -> Result<Value> {
    let e = axis_ellipse_params("ellipse.axis", p)?;
    added(s.add_entity(ellipse_kind(&e))?)
}

pub(super) fn run_ellipse_arc(s: &mut Session, p: &Value) -> Result<Value> {
    let mut e = if let Some(c) = point_param(p, "center") {
        let m = point_req("ellipse.arc", p, "major")?;
        let ratio = f64_req("ellipse.arc", p, "ratio")?;
        if !(ratio > 0.0 && ratio <= 1.0) || m.len() < 1e-12 {
            return Err(bad("ellipse.arc", "ratio must be in (0, 1] and major non-zero"));
        }
        Ellipse::full(c, m, ratio)
    } else {
        axis_ellipse_params("ellipse.arc", p)?
    };
    // Angles are measured from the major axis (like the prompts); convert to parameters.
    let to_param = |deg: f64| {
        let a = deg.to_radians();
        let ge = Ellipse { start: 0.0, end: TAU, ..e };
        ge.param_of(e.center + Vec2::from_angle(a + e.major.angle()))
    };
    let st = to_param(f64_req("ellipse.arc", p, "start")?);
    let en = to_param(f64_req("ellipse.arc", p, "end")?);
    e.start = st;
    e.end = en;
    added(s.add_entity(ellipse_kind(&e))?)
}
