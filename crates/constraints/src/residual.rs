//! Residual equations for each constraint type.

use cadcraft_doc::{Constraint, ConstraintKind, DistAxis, GeomRef};
use cadcraft_geom::Vec2;

use crate::model::Model;

/// How a constraint's references were interpreted (decided once from the starting geometry).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Form {
    /// Point–point.
    PP,
    /// Point on/at a line (refs: point, line).
    PL {
        swap: bool,
    },
    /// Point on/at a circle (refs: point, circle).
    PC {
        swap: bool,
    },
    /// One line (or two points for horizontal/vertical).
    L1,
    /// Two points (horizontal/vertical/distance).
    P2,
    LL,
    CC,
    /// Line–circle (tangent).
    LC {
        swap: bool,
    },
    /// Symmetric line endpoints, with endpoint pairing crossed or not.
    SymLines {
        crossed: bool,
    },
    /// Circle or arc alone.
    C1,
    /// Arc sweep (angular on an arc).
    ArcSweep,
    /// Fix of a derived point (residual to an anchor).
    Anchor(Vec2),
    /// Fix handled by locking parameters (no rows).
    Lock,
}

#[derive(Clone, Debug)]
pub(crate) struct Built {
    pub id: u32,
    pub kind: ConstraintKind,
    pub refs: Vec<GeomRef>,
    pub form: Form,
    /// Target value (lengths in drawing units, angles in radians).
    pub target: f64,
    /// Orientation chosen from the starting geometry (±1).
    pub sign: f64,
    pub rows: usize,
}

fn dir_n(a: Vec2, b: Vec2) -> Vec2 {
    let d = b - a;
    let l = d.len();
    if l > 1e-12 { d * (1.0 / l) } else { d }
}

fn sgn(v: f64) -> f64 {
    if v < 0.0 { -1.0 } else { 1.0 }
}

fn wrap(a: f64) -> f64 {
    let t = std::f64::consts::TAU;
    let r = (a + std::f64::consts::PI).rem_euclid(t) - std::f64::consts::PI;
    if r.is_finite() { r } else { 0.0 }
}

fn r_at(c: &Constraint, i: usize) -> Option<&GeomRef> {
    c.refs.get(i)
}

impl Built {
    /// Interpret a constraint against the model at `x`. `target` is the evaluated value for
    /// dimensional constraints (degrees for angular). Errors explain what references are needed.
    pub fn new(c: &Constraint, m: &Model, x: &[f64], target: f64) -> Result<Built, String> {
        use ConstraintKind as K;
        let refs = c.refs.clone();
        let pt = |i: usize| r_at(c, i).and_then(|r| m.point(x, r));
        let ln = |i: usize| r_at(c, i).and_then(|r| m.line(x, r));
        let ci = |i: usize| r_at(c, i).and_then(|r| m.circle(x, r));
        let n = c.refs.len();
        let mut b = Built { id: c.id, kind: c.kind, refs, form: Form::PP, target, sign: 1.0, rows: 0 };
        let need = |what: &str| Err(format!("{} needs {what}", c.kind.name()));
        match c.kind {
            K::Coincident => {
                if n != 2 {
                    return need("two references");
                }
                b.form = if pt(0).is_some() && pt(1).is_some() {
                    b.rows = 2;
                    Form::PP
                } else if pt(0).is_some() && ln(1).is_some() {
                    Form::PL { swap: false }
                } else if pt(1).is_some() && ln(0).is_some() {
                    Form::PL { swap: true }
                } else if pt(0).is_some() && ci(1).is_some() {
                    Form::PC { swap: false }
                } else if pt(1).is_some() && ci(0).is_some() {
                    Form::PC { swap: true }
                } else {
                    return need("two points, or a point and a line/circle/arc");
                };
                if b.rows == 0 {
                    b.rows = 1;
                }
            }
            K::Horizontal | K::Vertical => {
                if n == 1 && ln(0).is_some() {
                    b.form = Form::L1;
                } else if n == 2 && pt(0).is_some() && pt(1).is_some() {
                    b.form = Form::P2;
                } else {
                    return need("a line or two points");
                }
                b.rows = 1;
            }
            K::Parallel | K::Perpendicular | K::Collinear => {
                if !(n == 2 && ln(0).is_some() && ln(1).is_some()) {
                    return need("two lines");
                }
                b.form = Form::LL;
                b.rows = if c.kind == K::Collinear { 2 } else { 1 };
            }
            K::Concentric => {
                let center = |i: usize| ci(i).map(|(c, _)| c).or_else(|| pt(i));
                if !(n == 2 && center(0).is_some() && center(1).is_some() && (ci(0).is_some() || ci(1).is_some())) {
                    return need("two circles or arcs");
                }
                b.form = Form::CC;
                b.rows = 2;
            }
            K::Equal => {
                if n != 2 {
                    return need("two objects");
                }
                b.form = if ln(0).is_some() && ln(1).is_some() {
                    Form::LL
                } else if ci(0).is_some() && ci(1).is_some() {
                    Form::CC
                } else {
                    return need("two lines or two circles/arcs");
                };
                b.rows = 1;
            }
            K::Tangent | K::Smooth => {
                if n != 2 {
                    return need("two objects");
                }
                b.rows = 1;
                if let (Some((a, bb)), Some((cc, _))) = (ln(0), ci(1)) {
                    b.form = Form::LC { swap: false };
                    b.sign = sgn(dir_n(a, bb).cross(cc - a));
                } else if let (Some((a, bb)), Some((cc, _))) = (ln(1), ci(0)) {
                    b.form = Form::LC { swap: true };
                    b.sign = sgn(dir_n(a, bb).cross(cc - a));
                } else if let (Some((c1, r1)), Some((c2, r2))) = (ci(0), ci(1)) {
                    b.form = Form::CC;
                    // sign > 0: external tangency; < 0: internal.
                    let d = c1.dist(c2);
                    b.sign = if d >= r1.abs().max(r2.abs()) { 1.0 } else { -1.0 };
                    b.target = sgn(r1 - r2);
                } else if ln(0).is_some() && ln(1).is_some() && c.kind == K::Smooth {
                    b.form = Form::LL;
                } else {
                    return need("a line and a circle/arc, or two circles/arcs");
                }
            }
            K::Symmetric => {
                if n != 3 || ln(2).is_none() {
                    return need("two objects and a symmetry line");
                }
                if pt(0).is_some() && pt(1).is_some() {
                    b.form = Form::PP;
                    b.rows = 2;
                } else if let (Some((a1, b1)), Some((a2, b2)), Some((la, lb))) = (ln(0), ln(1), ln(2)) {
                    let straight = a1.mirror(la, lb).dist(a2) + b1.mirror(la, lb).dist(b2);
                    let crossed = a1.mirror(la, lb).dist(b2) + b1.mirror(la, lb).dist(a2);
                    b.form = Form::SymLines { crossed: crossed < straight };
                    b.rows = 4;
                } else if ci(0).is_some() && ci(1).is_some() {
                    b.form = Form::CC;
                    b.rows = 3;
                } else {
                    return need("two points, lines or circles and a symmetry line");
                }
            }
            K::Fix => {
                if n != 1 {
                    return need("one reference");
                }
                let r = r_at(c, 0).ok_or_else(|| "Fix needs one reference".to_string())?;
                if m.lock_indices(r).is_some() {
                    b.form = Form::Lock;
                    b.rows = 0;
                } else if let Some(p) = pt(0) {
                    b.form = Form::Anchor(p);
                    b.rows = 2;
                } else {
                    return need("a point or an object");
                }
            }
            K::Distance(axis) => {
                b.rows = 1;
                if n == 1 && ln(0).is_some() {
                    b.form = Form::L1;
                } else if n == 2 && pt(0).is_some() && pt(1).is_some() {
                    b.form = Form::P2;
                } else if n == 2 && axis == DistAxis::Aligned && pt(0).is_some() && ln(1).is_some() {
                    b.form = Form::PL { swap: false };
                } else if n == 2 && axis == DistAxis::Aligned && pt(1).is_some() && ln(0).is_some() {
                    b.form = Form::PL { swap: true };
                } else {
                    return need("a line or two points");
                }
                if axis != DistAxis::Aligned || matches!(b.form, Form::PL { .. }) {
                    // Keep the current orientation: measure the signed value.
                    b.sign = 1.0;
                    let mut out = Vec::new();
                    b.target = 0.0;
                    b.eval(m, x, &mut out);
                    b.sign = sgn(out.first().copied().unwrap_or(0.0));
                    b.target = target;
                }
                if !(target.is_finite() && target >= 0.0) {
                    return Err(format!("{} value must be non-negative", c.kind.name()));
                }
            }
            K::Angular => {
                b.rows = 1;
                if n == 2 && ln(0).is_some() && ln(1).is_some() {
                    b.form = Form::LL;
                    let (Some((a1, b1)), Some((a2, b2))) = (ln(0), ln(1)) else { return need("two lines") };
                    let (d1, d2) = (dir_n(a1, b1), dir_n(a2, b2));
                    b.sign = sgn(d1.cross(d2).atan2(d1.dot(d2)));
                } else if n == 1 && m.arc_sweep(x, r_at(c, 0).ok_or_else(|| "Angular needs references".to_string())?).is_some() {
                    b.form = Form::ArcSweep;
                } else {
                    return need("two lines or an arc");
                }
                b.target = target.to_radians();
                // Two lines measure a signed angle in [-180, 180] and the sign is re-read from the geometry, so a larger
                // target would flip the sign once solved and never be satisfied; only an arc sweep can reach a full turn.
                let max = if b.form == Form::LL { std::f64::consts::PI } else { std::f64::consts::TAU };
                if !b.target.is_finite() || b.target.abs() > max {
                    return Err("angle out of range".into());
                }
            }
            K::Radius | K::Diameter => {
                if !(n == 1 && ci(0).is_some()) {
                    return need("a circle or arc");
                }
                b.form = Form::C1;
                b.rows = 1;
                if !(target.is_finite() && target > 0.0) {
                    return Err(format!("{} value must be positive", c.kind.name()));
                }
            }
        }
        Ok(b)
    }

    /// Append this constraint's residuals (exactly `rows` values).
    pub fn eval(&self, m: &Model, x: &[f64], out: &mut Vec<f64>) {
        let start = out.len();
        self.eval_inner(m, x, out);
        out.resize(start + self.rows, 0.0);
    }

    fn eval_inner(&self, m: &Model, x: &[f64], out: &mut Vec<f64>) -> Option<()> {
        use ConstraintKind as K;
        let r = |i: usize| self.refs.get(i);
        let pt = |i: usize| m.point(x, r(i)?);
        let ln = |i: usize| m.line(x, r(i)?);
        let ci = |i: usize| m.circle(x, r(i)?);
        match (self.kind, self.form) {
            (_, Form::Lock) => {}
            (_, Form::Anchor(a)) => {
                let p = pt(0)?;
                out.extend([p.x - a.x, p.y - a.y]);
            }
            (K::Coincident, Form::PP) => {
                let (p, q) = (pt(0)?, pt(1)?);
                out.extend([p.x - q.x, p.y - q.y]);
            }
            (K::Coincident, Form::PL { swap }) => {
                let (p, (a, b)) = if swap { (pt(1)?, ln(0)?) } else { (pt(0)?, ln(1)?) };
                out.push(dir_n(a, b).cross(p - a));
            }
            (K::Coincident, Form::PC { swap }) => {
                let (p, (c, rad)) = if swap { (pt(1)?, ci(0)?) } else { (pt(0)?, ci(1)?) };
                out.push(p.dist(c) - rad);
            }
            (K::Horizontal | K::Vertical, f) => {
                let (a, b) = if f == Form::L1 { ln(0)? } else { (pt(0)?, pt(1)?) };
                out.push(if self.kind == K::Horizontal { a.y - b.y } else { a.x - b.x });
            }
            (K::Parallel | K::Smooth, Form::LL) => {
                let ((a1, b1), (a2, b2)) = (ln(0)?, ln(1)?);
                out.push(dir_n(a1, b1).cross(dir_n(a2, b2)));
            }
            (K::Perpendicular, _) => {
                let ((a1, b1), (a2, b2)) = (ln(0)?, ln(1)?);
                out.push(dir_n(a1, b1).dot(dir_n(a2, b2)));
            }
            (K::Collinear, _) => {
                let ((a1, b1), (a2, b2)) = (ln(0)?, ln(1)?);
                let d = dir_n(a1, b1);
                out.extend([d.cross(a2 - a1), d.cross(b2 - a1)]);
            }
            (K::Concentric, _) => {
                let center = |i: usize| ci(i).map(|(c, _)| c).or_else(|| pt(i));
                let (p, q) = (center(0)?, center(1)?);
                out.extend([p.x - q.x, p.y - q.y]);
            }
            (K::Equal, Form::LL) => {
                let ((a1, b1), (a2, b2)) = (ln(0)?, ln(1)?);
                out.push(a1.dist(b1) - a2.dist(b2));
            }
            (K::Equal, _) => {
                let ((_, r1), (_, r2)) = (ci(0)?, ci(1)?);
                out.push(r1 - r2);
            }
            (K::Tangent | K::Smooth, Form::LC { swap }) => {
                let ((a, b), (c, rad)) = if swap { (ln(1)?, ci(0)?) } else { (ln(0)?, ci(1)?) };
                out.push(self.sign * dir_n(a, b).cross(c - a) - rad);
            }
            (K::Tangent | K::Smooth, _) => {
                let ((c1, r1), (c2, r2)) = (ci(0)?, ci(1)?);
                let d = c1.dist(c2);
                out.push(if self.sign > 0.0 { d - (r1 + r2) } else { d - self.target * (r1 - r2) });
            }
            (K::Symmetric, Form::PP) => {
                let (p, q, (a, b)) = (pt(0)?, pt(1)?, ln(2)?);
                let d = dir_n(a, b);
                out.extend([d.cross(p.mid(q) - a), d.dot(q - p)]);
            }
            (K::Symmetric, Form::SymLines { crossed }) => {
                let ((a1, b1), (a2, b2), (a, b)) = (ln(0)?, ln(1)?, ln(2)?);
                let (a2, b2) = if crossed { (b2, a2) } else { (a2, b2) };
                let d = dir_n(a, b);
                out.extend([d.cross(a1.mid(a2) - a), d.dot(a2 - a1), d.cross(b1.mid(b2) - a), d.dot(b2 - b1)]);
            }
            (K::Symmetric, _) => {
                let ((p, r1), (q, r2), (a, b)) = (ci(0)?, ci(1)?, ln(2)?);
                let d = dir_n(a, b);
                out.extend([d.cross(p.mid(q) - a), d.dot(q - p), r1 - r2]);
            }
            (K::Distance(axis), f) => {
                let v = match (axis, f) {
                    (_, Form::PL { swap }) => {
                        let (p, (a, b)) = if swap { (pt(1)?, ln(0)?) } else { (pt(0)?, ln(1)?) };
                        dir_n(a, b).cross(p - a)
                    }
                    _ => {
                        let (a, b) = if f == Form::L1 { ln(0)? } else { (pt(0)?, pt(1)?) };
                        match axis {
                            DistAxis::Aligned => a.dist(b),
                            DistAxis::Horizontal => b.x - a.x,
                            DistAxis::Vertical => b.y - a.y,
                        }
                    }
                };
                out.push(self.sign * v - self.target);
            }
            (K::Angular, Form::ArcSweep) => {
                out.push(m.arc_sweep(x, r(0)?)? - self.target);
            }
            (K::Angular, _) => {
                let ((a1, b1), (a2, b2)) = (ln(0)?, ln(1)?);
                let (d1, d2) = (dir_n(a1, b1), dir_n(a2, b2));
                out.push(wrap(d1.cross(d2).atan2(d1.dot(d2)) - self.sign * self.target));
            }
            (K::Radius, _) => out.push(ci(0)?.1 - self.target),
            (K::Diameter, _) => out.push(2.0 * ci(0)?.1 - self.target),
            _ => {}
        }
        Some(())
    }

    /// The current measured value (degrees for angular), for dimensional constraints.
    pub fn measure(&self, m: &Model, x: &[f64]) -> f64 {
        let mut b = self.clone();
        b.target = 0.0;
        let mut out = Vec::new();
        b.eval(m, x, &mut out);
        let v = out.first().copied().unwrap_or(0.0).abs();
        if self.kind == ConstraintKind::Angular { v.to_degrees() } else { v }
    }
}
