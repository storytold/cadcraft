//! Residual equations for each constraint type.

use cadcraft_doc::{Constraint, ConstraintKind, DistAxis, GeomRef};
use cadcraft_geom::Vec2;

use crate::curves::{Ell, circle_at};
use crate::model::Model;

/// Contact parameter samples per curve when guessing where two curves touch.
const CONTACT_SAMPLES: usize = 72;

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
    /// Two lines measured along the rays from their intersection to each line's far end, the
    /// angle the dimension draws; `rev` marks a line whose far end is its start point.
    Rays {
        rev: [bool; 2],
    },
    /// Fix of a derived point (residual to an anchor).
    Anchor(Vec2),
    /// Fix handled by locking parameters (no rows).
    Lock,
    /// Point on an ellipse (refs: point, ellipse).
    PE {
        swap: bool,
    },
    /// Line tangent to an ellipse.
    LE {
        swap: bool,
    },
    /// Two curves (circles, arcs, ellipses; at least one ellipse) touching at a contact point
    /// given by one auxiliary parameter per curve.
    Contact,
    /// A spline end on a line, circle, arc or ellipse and tangent to it (`spline`: which
    /// reference is the spline).
    EndOn {
        spline: usize,
        at_end: bool,
    },
    /// Two curve ends joined with a common tangent (G1), and common curvature when `g2`.
    Join {
        a_end: bool,
        b_end: bool,
        g2: bool,
    },
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
    /// Starting values of auxiliary unknowns (contact parameters), appended to the parameter
    /// vector at `aux` by [`crate::interpret`].
    pub aux_init: Vec<f64>,
    pub aux: usize,
}

fn dir_n(a: Vec2, b: Vec2) -> Vec2 {
    let d = b - a;
    let l = d.len();
    if l > 1e-12 { d * (1.0 / l) } else { d }
}

fn ray(a: Vec2, b: Vec2, rev: bool) -> Vec2 {
    if rev { dir_n(b, a) } else { dir_n(a, b) }
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
    /// Estimate spline reconstruction work for endpoint and join residuals; other residual forms add no such cost.
    pub(crate) fn spline_reconstruct_cost(&self, m: &Model) -> f64 {
        let cost = |i: usize| self.refs.get(i).map_or(0.0, |r| m.spline_reconstruct_cost(r.handle));
        match self.form {
            Form::EndOn { spline, .. } => cost(spline),
            Form::Join { .. } => cost(0) + cost(1),
            _ => 0.0,
        }
    }

    /// Interpret a constraint against the model at `x`. `target` is the evaluated value for
    /// dimensional constraints (degrees for angular). Errors explain what references are needed.
    pub fn new(c: &Constraint, m: &Model, x: &[f64], target: f64) -> Result<Built, String> {
        use ConstraintKind as K;
        let refs = c.refs.clone();
        let pt = |i: usize| r_at(c, i).and_then(|r| m.point(x, r));
        let ln = |i: usize| r_at(c, i).and_then(|r| m.line(x, r));
        let ci = |i: usize| r_at(c, i).and_then(|r| m.circle(x, r));
        let n = c.refs.len();
        let el = |i: usize| r_at(c, i).and_then(|r| m.ellipse(x, r));
        let lnx = |i: usize| ln(i).or_else(|| el(i).map(|e| (e.c - e.mj, e.c + e.mj)));
        let mut b = Built { id: c.id, kind: c.kind, refs, form: Form::PP, target, sign: 1.0, rows: 0, aux_init: Vec::new(), aux: 0 };
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
                } else if pt(0).is_some() && el(1).is_some() {
                    Form::PE { swap: false }
                } else if pt(1).is_some() && el(0).is_some() {
                    Form::PE { swap: true }
                } else {
                    return need("two points, or a point and a line/circle/arc/ellipse");
                };
                if b.rows == 0 {
                    b.rows = 1;
                }
            }
            K::Horizontal | K::Vertical => {
                if n == 1 && lnx(0).is_some() {
                    b.form = Form::L1;
                } else if n == 2 && pt(0).is_some() && pt(1).is_some() {
                    b.form = Form::P2;
                } else {
                    return need("a line, an ellipse or two points");
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
                let center = |i: usize| ci(i).map(|(c, _)| c).or_else(|| el(i).map(|e| e.c)).or_else(|| pt(i));
                let curve = |i: usize| ci(i).is_some() || el(i).is_some();
                if !(n == 2 && center(0).is_some() && center(1).is_some() && (curve(0) || curve(1))) {
                    return need("two circles, arcs or ellipses");
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
            K::Tangent | K::Smooth => return Self::tangent_form(b, c, m, x),
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
                if let (2, Some((a1, b1)), Some((a2, b2))) = (n, ln(0), ln(1)) {
                    // Start-to-end directions would measure the supplement when a line is drawn
                    // towards the vertex, so use the rays the dimension draws.
                    let rev = match cadcraft_geom::line_line_infinite(a1, b1, a2, b2) {
                        Some((_, t, u)) => [t > 0.5, u > 0.5],
                        None => [false, false],
                    };
                    b.form = Form::Rays { rev };
                    let (d1, d2) = (ray(a1, b1, rev[0]), ray(a2, b2, rev[1]));
                    b.sign = sgn(d1.cross(d2).atan2(d1.dot(d2)));
                } else if n == 1 && m.arc_sweep(x, r_at(c, 0).ok_or_else(|| "Angular needs references".to_string())?).is_some() {
                    b.form = Form::ArcSweep;
                } else {
                    return need("two lines or an arc");
                }
                b.target = target.to_radians();
                // Two lines measure a signed angle in [-180, 180] and the sign is re-read from the geometry, so a larger
                // target would flip the sign once solved and never be satisfied; only an arc sweep can reach a full turn.
                let max = if matches!(b.form, Form::Rays { .. }) { std::f64::consts::PI } else { std::f64::consts::TAU };
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
                if !target.is_finite() || crate::curves::not_above(target, 0.0) {
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

    fn eval_inner(&self, model: &Model, values: &[f64], out: &mut Vec<f64>) -> Option<()> {
        ResidualEval { built: self, model, values, out }.evaluate()
    }

    /// Point and derivative of curve reference `i` (circle, arc or ellipse) at parameter `t`.
    fn curve_at(&self, m: &Model, x: &[f64], i: usize, t: f64) -> Option<(Vec2, Vec2)> {
        let r = self.refs.get(i)?;
        if let Some((c, rad)) = m.circle(x, r) {
            return Some(circle_at(c, rad, t));
        }
        let e = m.ellipse(x, r)?;
        Some((e.at(t), e.d1(t)))
    }

    /// Contact parameters where two curves come closest (sampled), for [`Form::Contact`].
    fn contact_guess(&self, m: &Model, x: &[f64]) -> Option<Vec<f64>> {
        let step = std::f64::consts::TAU / CONTACT_SAMPLES as f64;
        let s0: Vec<(f64, Vec2)> = (0..CONTACT_SAMPLES).filter_map(|k| Some((k as f64 * step, self.curve_at(m, x, 0, k as f64 * step)?.0))).collect();
        let s1: Vec<(f64, Vec2)> = (0..CONTACT_SAMPLES).filter_map(|k| Some((k as f64 * step, self.curve_at(m, x, 1, k as f64 * step)?.0))).collect();
        let mut best: Option<(f64, f64, f64)> = None;
        for (t0, p0) in &s0 {
            for (t1, p1) in &s1 {
                let d = p0.dist(*p1);
                if d.is_finite() && best.is_none_or(|b| d < b.0) {
                    best = Some((d, *t0, *t1));
                }
            }
        }
        best.map(|(_, a, b)| vec![a, b])
    }

    /// Tangent or Smooth with a spline: the spline end and the other curve's nearest end are
    /// joined (Smooth: G2; Tangent between splines: G1), or the spline's end touches a line,
    /// circle, arc or ellipse tangentially. A `start`/`end` reference picks the end.
    fn tangent_form(mut built: Built, constraint: &Constraint, model: &Model, values: &[f64]) -> Result<Built, String> {
        use ConstraintKind as K;

        let need = |what: &str| Err(format!("{} needs {what}", constraint.kind.name()));
        if built.refs.len() != 2 {
            return need("two objects");
        }
        if constraint.refs.iter().any(|reference| model.is_spline(reference)) {
            return Self::spline_form(built, model, values);
        }

        let line = |index| r_at(constraint, index).and_then(|reference| model.line(values, reference));
        let circle = |index| r_at(constraint, index).and_then(|reference| model.circle(values, reference));
        let ellipse = |index| r_at(constraint, index).and_then(|reference| model.ellipse(values, reference));
        built.rows = 1;

        if let (Some((start, end)), Some((center, _))) = (line(0), circle(1)) {
            built.form = Form::LC { swap: false };
            built.sign = sgn(dir_n(start, end).cross(center - start));
        } else if let (Some((start, end)), Some((center, _))) = (line(1), circle(0)) {
            built.form = Form::LC { swap: true };
            built.sign = sgn(dir_n(start, end).cross(center - start));
        } else if let (Some((first_center, first_radius)), Some((second_center, second_radius))) = (circle(0), circle(1)) {
            built.form = Form::CC;
            // sign > 0: external tangency; < 0: internal.
            let distance = first_center.dist(second_center);
            built.sign = if distance >= first_radius.abs().max(second_radius.abs()) { 1.0 } else { -1.0 };
            built.target = sgn(first_radius - second_radius);
        } else if let (Some((start, end)), Some(ellipse)) = (line(0), ellipse(1)) {
            built.form = Form::LE { swap: false };
            built.sign = sgn(dir_n(start, end).cross(ellipse.c - start));
        } else if let (Some((start, end)), Some(ellipse)) = (line(1), ellipse(0)) {
            built.form = Form::LE { swap: true };
            built.sign = sgn(dir_n(start, end).cross(ellipse.c - start));
        } else if (circle(0).is_some() || ellipse(0).is_some()) && (circle(1).is_some() || ellipse(1).is_some()) {
            built.form = Form::Contact;
            built.rows = 3;
            built.aux_init =
                built.contact_guess(model, values).ok_or_else(|| format!("{} could not find where the curves touch", constraint.kind.name()))?;
        } else if line(0).is_some() && line(1).is_some() && constraint.kind == K::Smooth {
            built.form = Form::LL;
        } else {
            return need("a line and a circle/arc/ellipse, two curves, or a spline end and a curve");
        }

        Ok(built)
    }

    fn spline_form(mut b: Built, m: &Model, x: &[f64]) -> Result<Built, String> {
        let (Some(r0), Some(r1)) = (b.refs.first().copied(), b.refs.get(1).copied()) else {
            return Err(format!("{} needs two objects", b.kind.name()));
        };
        let ends = |r: &GeomRef| -> &'static [bool] {
            match r.sub {
                cadcraft_doc::Sub::Start => &[false],
                cadcraft_doc::Sub::End => &[true],
                _ => &[false, true],
            }
        };
        let join = b.kind == ConstraintKind::Smooth || (m.is_spline(&r0) && m.is_spline(&r1));
        if join {
            let mut best: Option<(f64, bool, bool)> = None;
            for &ea in ends(&r0) {
                for &eb in ends(&r1) {
                    if let (Some(fa), Some(fb)) = (m.end_frame(x, &r0, ea), m.end_frame(x, &r1, eb)) {
                        let d = fa.p.dist(fb.p);
                        if d.is_finite() && best.is_none_or(|bb| d < bb.0) {
                            best = Some((d, ea, eb));
                        }
                    }
                }
            }
            let Some((_, a_end, b_end)) = best else {
                return Err(format!("{} needs an open spline and a line, arc, elliptical arc, polyline segment or open spline", b.kind.name()));
            };
            let g2 = b.kind == ConstraintKind::Smooth;
            b.form = Form::Join { a_end, b_end, g2 };
            b.rows = if g2 { 4 } else { 3 };
            return Ok(b);
        }
        let (si, rs, ro) = if m.is_spline(&r0) { (0, r0, r1) } else { (1, r1, r0) };
        let dist = |p: Vec2| -> Option<f64> {
            if let Some((a, bb)) = m.line(x, &ro) {
                Some(dir_n(a, bb).cross(p - a).abs())
            } else if let Some((c, rad)) = m.circle(x, &ro) {
                Some((p.dist(c) - rad).abs())
            } else {
                m.ellipse(x, &ro).map(|e| e.on(p).abs())
            }
        };
        let mut best: Option<(f64, bool)> = None;
        for &e in ends(&rs) {
            if let Some(d) = m.end_frame(x, &rs, e).and_then(|f| dist(f.p))
                && d.is_finite()
                && best.is_none_or(|bb| d < bb.0)
            {
                best = Some((d, e));
            }
        }
        let Some((_, at_end)) = best else {
            return Err("Tangent needs an open spline and a line, circle, arc, ellipse or spline".into());
        };
        b.form = Form::EndOn { spline: si, at_end };
        b.rows = 2;
        Ok(b)
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

/// Shared access to a constraint's geometry, values, and output while evaluating one residual.
struct ResidualEval<'a> {
    built: &'a Built,
    model: &'a Model,
    values: &'a [f64],
    out: &'a mut Vec<f64>,
}

impl ResidualEval<'_> {
    fn evaluate(&mut self) -> Option<()> {
        use ConstraintKind as K;

        match (self.built.kind, self.built.form) {
            (_, Form::Lock) => Some(()),
            (_, Form::PE { swap }) => self.eval_point_ellipse(swap),
            (_, Form::LE { swap }) => self.eval_line_ellipse(swap),
            (_, Form::Contact) => self.eval_contact(),
            (_, Form::EndOn { spline, at_end }) => self.eval_end_on(spline, at_end),
            (_, Form::Join { a_end, b_end, g2 }) => self.eval_join(a_end, b_end, g2),
            (_, Form::Anchor(anchor)) => self.eval_anchor(anchor),
            (K::Coincident, Form::PP) => self.eval_coincident_points(),
            (K::Coincident, Form::PL { swap }) => self.eval_point_line(swap),
            (K::Coincident, Form::PC { swap }) => self.eval_point_circle(swap),
            (K::Horizontal | K::Vertical, form) => self.eval_horizontal_vertical(form),
            (K::Parallel | K::Smooth, Form::LL) => self.eval_parallel(),
            (K::Perpendicular, _) => self.eval_perpendicular(),
            (K::Collinear, _) => self.eval_collinear(),
            (K::Concentric, _) => self.eval_concentric(),
            (K::Equal, Form::LL) => self.eval_equal_lines(),
            (K::Equal, _) => self.eval_equal_circles(),
            (K::Tangent | K::Smooth, Form::LC { swap }) => self.eval_tangent_line_circle(swap),
            (K::Tangent | K::Smooth, _) => self.eval_tangent_circles(),
            (K::Symmetric, Form::PP) => self.eval_symmetric_points(),
            (K::Symmetric, Form::SymLines { crossed }) => self.eval_symmetric_lines(crossed),
            (K::Symmetric, _) => self.eval_symmetric_circles(),
            (K::Distance(axis), form) => self.eval_distance(axis, form),
            (K::Angular, Form::ArcSweep) => self.eval_angular_arc(),
            (K::Angular, Form::Rays { rev }) => self.eval_angular_rays(rev),
            (K::Radius, _) => self.eval_radius(),
            (K::Diameter, _) => self.eval_diameter(),
            _ => Some(()),
        }
    }

    fn reference(&self, index: usize) -> Option<&GeomRef> {
        self.built.refs.get(index)
    }

    fn point(&self, index: usize) -> Option<Vec2> {
        self.model.point(self.values, self.reference(index)?)
    }

    fn line(&self, index: usize) -> Option<(Vec2, Vec2)> {
        self.model.line(self.values, self.reference(index)?)
    }

    fn circle(&self, index: usize) -> Option<(Vec2, f64)> {
        self.model.circle(self.values, self.reference(index)?)
    }

    fn ellipse(&self, index: usize) -> Option<Ell> {
        self.model.ellipse(self.values, self.reference(index)?)
    }

    fn eval_point_ellipse(&mut self, swap: bool) -> Option<()> {
        let (point, ellipse) = if swap { (self.point(1)?, self.ellipse(0)?) } else { (self.point(0)?, self.ellipse(1)?) };
        self.out.push(ellipse.on(point));
        Some(())
    }

    fn eval_line_ellipse(&mut self, swap: bool) -> Option<()> {
        let (line, ellipse) = if swap { (self.line(1)?, self.ellipse(0)?) } else { (self.line(0)?, self.ellipse(1)?) };
        let (start, end) = line;
        let direction = dir_n(start, end);
        self.out.push(self.built.sign * direction.cross(ellipse.c - start) - ellipse.support(direction.perp()));
        Some(())
    }

    fn eval_contact(&mut self) -> Option<()> {
        let first_parameter = *self.values.get(self.built.aux)?;
        let second_parameter = *self.values.get(self.built.aux + 1)?;
        let (first_point, first_tangent) = self.built.curve_at(self.model, self.values, 0, first_parameter)?;
        let (second_point, second_tangent) = self.built.curve_at(self.model, self.values, 1, second_parameter)?;
        self.out.extend([
            first_point.x - second_point.x,
            first_point.y - second_point.y,
            first_tangent.normalized().cross(second_tangent.normalized()),
        ]);
        Some(())
    }

    fn eval_end_on(&mut self, spline: usize, at_end: bool) -> Option<()> {
        let frame = self.model.end_frame(self.values, self.reference(spline)?, at_end)?;
        let other = 1 - spline.min(1);
        if let Some((start, end)) = self.line(other) {
            let direction = dir_n(start, end);
            self.out.extend([direction.cross(frame.p - start), frame.t.cross(direction)]);
        } else if let Some((center, radius)) = self.circle(other) {
            self.out.extend([frame.p.dist(center) - radius, frame.t.dot(dir_n(center, frame.p))]);
        } else {
            let ellipse = self.ellipse(other)?;
            self.out.extend([ellipse.on(frame.p), frame.t.dot(ellipse.normal(frame.p))]);
        }
        Some(())
    }

    fn eval_join(&mut self, a_end: bool, b_end: bool, g2: bool) -> Option<()> {
        let first = self.model.end_frame(self.values, self.reference(0)?, a_end)?;
        let second = self.model.end_frame(self.values, self.reference(1)?, b_end)?;
        let first = if a_end { first } else { first.reversed() };
        let second = if b_end { second.reversed() } else { second };
        self.out.extend([first.p.x - second.p.x, first.p.y - second.p.y, first.t.cross(second.t)]);
        if g2 {
            self.out.push(first.k - second.k);
        }
        Some(())
    }

    fn eval_anchor(&mut self, anchor: Vec2) -> Option<()> {
        let point = self.point(0)?;
        self.out.extend([point.x - anchor.x, point.y - anchor.y]);
        Some(())
    }

    fn eval_coincident_points(&mut self) -> Option<()> {
        let (first, second) = (self.point(0)?, self.point(1)?);
        self.out.extend([first.x - second.x, first.y - second.y]);
        Some(())
    }

    fn eval_point_line(&mut self, swap: bool) -> Option<()> {
        let (point, (start, end)) = if swap { (self.point(1)?, self.line(0)?) } else { (self.point(0)?, self.line(1)?) };
        self.out.push(dir_n(start, end).cross(point - start));
        Some(())
    }

    fn eval_point_circle(&mut self, swap: bool) -> Option<()> {
        let (point, (center, radius)) = if swap { (self.point(1)?, self.circle(0)?) } else { (self.point(0)?, self.circle(1)?) };
        self.out.push(point.dist(center) - radius);
        Some(())
    }

    fn eval_horizontal_vertical(&mut self, form: Form) -> Option<()> {
        use ConstraintKind as K;

        let (first, second) = if form == Form::L1 {
            self.line(0).or_else(|| self.ellipse(0).map(|ellipse| (ellipse.c - ellipse.mj, ellipse.c + ellipse.mj)))?
        } else {
            (self.point(0)?, self.point(1)?)
        };
        let value = if self.built.kind == K::Horizontal { first.y - second.y } else { first.x - second.x };
        self.out.push(value);
        Some(())
    }

    fn eval_parallel(&mut self) -> Option<()> {
        let ((first_start, first_end), (second_start, second_end)) = (self.line(0)?, self.line(1)?);
        self.out.push(dir_n(first_start, first_end).cross(dir_n(second_start, second_end)));
        Some(())
    }

    fn eval_perpendicular(&mut self) -> Option<()> {
        let ((first_start, first_end), (second_start, second_end)) = (self.line(0)?, self.line(1)?);
        self.out.push(dir_n(first_start, first_end).dot(dir_n(second_start, second_end)));
        Some(())
    }

    fn eval_collinear(&mut self) -> Option<()> {
        let ((first_start, first_end), (second_start, second_end)) = (self.line(0)?, self.line(1)?);
        let direction = dir_n(first_start, first_end);
        self.out.extend([direction.cross(second_start - first_start), direction.cross(second_end - first_start)]);
        Some(())
    }

    fn eval_concentric(&mut self) -> Option<()> {
        let center = |index| {
            self.circle(index).map(|(center, _)| center).or_else(|| self.ellipse(index).map(|ellipse| ellipse.c)).or_else(|| self.point(index))
        };
        let (first, second) = (center(0)?, center(1)?);
        self.out.extend([first.x - second.x, first.y - second.y]);
        Some(())
    }

    fn eval_equal_lines(&mut self) -> Option<()> {
        let ((first_start, first_end), (second_start, second_end)) = (self.line(0)?, self.line(1)?);
        self.out.push(first_start.dist(first_end) - second_start.dist(second_end));
        Some(())
    }

    fn eval_equal_circles(&mut self) -> Option<()> {
        let ((_, first_radius), (_, second_radius)) = (self.circle(0)?, self.circle(1)?);
        self.out.push(first_radius - second_radius);
        Some(())
    }

    fn eval_tangent_line_circle(&mut self, swap: bool) -> Option<()> {
        let ((start, end), (center, radius)) = if swap { (self.line(1)?, self.circle(0)?) } else { (self.line(0)?, self.circle(1)?) };
        self.out.push(self.built.sign * dir_n(start, end).cross(center - start) - radius);
        Some(())
    }

    fn eval_tangent_circles(&mut self) -> Option<()> {
        let ((first_center, first_radius), (second_center, second_radius)) = (self.circle(0)?, self.circle(1)?);
        let distance = first_center.dist(second_center);
        let residual = if self.built.sign > 0.0 {
            distance - (first_radius + second_radius)
        } else {
            distance - self.built.target * (first_radius - second_radius)
        };
        self.out.push(residual);
        Some(())
    }

    fn eval_symmetric_points(&mut self) -> Option<()> {
        let (first, second, (line_start, line_end)) = (self.point(0)?, self.point(1)?, self.line(2)?);
        let direction = dir_n(line_start, line_end);
        self.out.extend([direction.cross(first.mid(second) - line_start), direction.dot(second - first)]);
        Some(())
    }

    fn eval_symmetric_lines(&mut self, crossed: bool) -> Option<()> {
        let ((first_start, first_end), (second_start, second_end), (line_start, line_end)) = (self.line(0)?, self.line(1)?, self.line(2)?);
        let (second_start, second_end) = if crossed { (second_end, second_start) } else { (second_start, second_end) };
        let direction = dir_n(line_start, line_end);
        self.out.extend([
            direction.cross(first_start.mid(second_start) - line_start),
            direction.dot(second_start - first_start),
            direction.cross(first_end.mid(second_end) - line_start),
            direction.dot(second_end - first_end),
        ]);
        Some(())
    }

    fn eval_symmetric_circles(&mut self) -> Option<()> {
        let ((first_center, first_radius), (second_center, second_radius), (line_start, line_end)) =
            (self.circle(0)?, self.circle(1)?, self.line(2)?);
        let direction = dir_n(line_start, line_end);
        self.out.extend([
            direction.cross(first_center.mid(second_center) - line_start),
            direction.dot(second_center - first_center),
            first_radius - second_radius,
        ]);
        Some(())
    }

    fn eval_distance(&mut self, axis: DistAxis, form: Form) -> Option<()> {
        let value = match (axis, form) {
            (_, Form::PL { swap }) => {
                let (point, (start, end)) = if swap { (self.point(1)?, self.line(0)?) } else { (self.point(0)?, self.line(1)?) };
                dir_n(start, end).cross(point - start)
            }
            _ => {
                let (first, second) = if form == Form::L1 { self.line(0)? } else { (self.point(0)?, self.point(1)?) };
                match axis {
                    DistAxis::Aligned => first.dist(second),
                    DistAxis::Horizontal => second.x - first.x,
                    DistAxis::Vertical => second.y - first.y,
                }
            }
        };
        self.out.push(self.built.sign * value - self.built.target);
        Some(())
    }

    fn eval_angular_arc(&mut self) -> Option<()> {
        let reference = self.reference(0)?;
        self.out.push(self.model.arc_sweep(self.values, reference)? - self.built.target);
        Some(())
    }

    fn eval_angular_rays(&mut self, reversed: [bool; 2]) -> Option<()> {
        let ((first_start, first_end), (second_start, second_end)) = (self.line(0)?, self.line(1)?);
        let (first, second) = (ray(first_start, first_end, reversed[0]), ray(second_start, second_end, reversed[1]));
        let residual = wrap(first.cross(second).atan2(first.dot(second)) - self.built.sign * self.built.target);
        self.out.push(residual);
        Some(())
    }

    fn eval_radius(&mut self) -> Option<()> {
        self.out.push(self.circle(0)?.1 - self.built.target);
        Some(())
    }

    fn eval_diameter(&mut self) -> Option<()> {
        self.out.push(2.0 * self.circle(0)?.1 - self.built.target);
        Some(())
    }
}
