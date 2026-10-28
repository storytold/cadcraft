//! CADCraft parametric solver.
//!
//! Maps constrained lines, arcs, circles, points, polyline vertices, ellipses, splines and block
//! references to a parameter vector
//! ([`model`]), turns each [`Constraint`] into residual equations ([`residual`]), and drives the
//! residuals to zero with a weighted minimum-norm damped Gauss–Newton solver ([`lm`]), so
//! geometry moves as little as possible and geometry the caller wants kept (just edited, or picked
//! first) moves least. Fix locks parameters. Constraints are solved per connected component.
//!
//! Inconsistent systems are not "solved" by moving things wildly: the solve fails, nothing is
//! written back, and the error names the conflicting constraints (each one whose removal makes
//! the rest solvable). New constraints that add no independent equation are reported as
//! redundant (over-constraining), as AutoCAD does, naming the constraints they duplicate.
//!
//! [`dof()`] reports the remaining degrees of freedom: the rank of the constraint Jacobian per
//! connected component (Householder QR with column pivoting), and per object the dimension of
//! the null-space motion its own parameters still have.
//!
//! Own implementation (clean-room): the KittyCAD `ezpz` crate (MIT) was evaluated; it pulls in
//! the `faer` linear-algebra stack and models KCL's geometry, so a small solver tailored to the
//! drawing database (minimal movement, conflict reporting) was written instead.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod auto;
mod curves;
mod dof;
pub mod expr;
mod lm;
mod model;
mod residual;

use std::collections::{BTreeMap, HashMap};

use cadcraft_doc::{Constraint, ConstraintKind, Drawing, GeomRef, Handle};
use cadcraft_geom::Vec2;
use serde::Serialize;

pub use auto::{INFER_TYPES, InferOptions, autoconstrain, infer, infer_placed};
pub use dof::{DofReport, DofStatus, ObjectDof, dof};
pub use expr::{Env, ExprError};
pub use model::{ref_points, rigid_points, supported};

use model::Model;
use residual::{Built, Form};

/// Most constraints one drawing may hold.
pub const MAX_CONSTRAINTS: usize = 5000;
/// Constraints tried one by one when locating a conflict.
const MAX_CONFLICT_PROBES: usize = 48;
/// Solver iterations.
const MAX_ITER: usize = 200;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SolveError {
    #[error("parameter `{name}`: {err}")]
    Expr { name: String, err: ExprError },
    #[error("constraint {id}: {msg}")]
    Invalid { id: u32, msg: String },
    #[error("constraints conflict: {}", ids_text(ids))]
    Conflict { ids: Vec<u32> },
    #[error("constraint would over-constrain the geometry (redundant with {})", ids_text(ids))]
    Redundant { ids: Vec<u32> },
    #[error("constraint system too large to solve")]
    TooLarge,
    #[error("{0}")]
    Other(String),
}

fn ids_text(ids: &[u32]) -> String {
    if ids.is_empty() {
        return "existing constraints".into();
    }
    ids.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(", ")
}

/// Which geometry should prefer to stay or move.
#[derive(Clone, Debug, Default)]
pub struct SolveOptions {
    /// Entities that should move as little as possible (just edited, or picked first).
    pub keep: Vec<Handle>,
    /// Entities that should take up the change.
    pub prefer_move: Vec<Handle>,
    /// Points held where they are for this solve, as a Fix would hold them (the grip an edit
    /// just put down): see [`edited_points`].
    pub hold: Vec<GeomRef>,
    /// Where a Fix on a derived point (by constraint id) holds it, instead of where the point is
    /// now: see [`moved_anchors`].
    pub anchors: Vec<(u32, Vec2)>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SolveReport {
    /// Entities whose geometry changed.
    pub moved: Vec<Handle>,
    /// Constraints that could not be interpreted (and were ignored), with the reason.
    pub skipped: Vec<(u32, String)>,
}

// ---------------- parameters ----------------

/// The expression environment: user parameters and dimensional constraint names.
pub fn env(d: &Drawing) -> Env {
    let mut e = Env::new();
    for p in &d.parametric.parameters {
        e.insert(&p.name, &p.expr);
    }
    for c in &d.constraints {
        if c.kind.is_dimensional() && !c.name.is_empty() {
            e.insert(&c.name, &c.expr);
        }
    }
    e
}

/// The value of a dimensional constraint (degrees for angular).
pub fn constraint_value(env: &mut Env, c: &Constraint) -> Result<f64, SolveError> {
    let label = if c.name.is_empty() { format!("#{}", c.id) } else { c.name.clone() };
    let r = if c.name.is_empty() || !env.contains(&c.name) { env.eval(&c.expr) } else { env.value(&c.name) };
    r.map_err(|err| SolveError::Expr { name: label, err })
}

/// A row of the Parameters Manager.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamRow {
    pub name: String,
    pub expr: String,
    pub value: Option<f64>,
    pub error: Option<String>,
    /// `dimensional` or `user`.
    pub kind: &'static str,
    pub constraint: Option<u32>,
    pub description: String,
}

pub fn parameter_table(d: &Drawing) -> Vec<ParamRow> {
    let mut e = env(d);
    let mut rows = Vec::new();
    for c in d.constraints.iter().filter(|c| c.kind.is_dimensional()) {
        let r = constraint_value(&mut e, c);
        rows.push(ParamRow {
            name: c.name.clone(),
            expr: c.expr.clone(),
            value: r.as_ref().ok().copied(),
            error: r.err().map(|e| e.to_string()),
            kind: "dimensional",
            constraint: Some(c.id),
            description: c.kind.name().to_string(),
        });
    }
    for p in &d.parametric.parameters {
        let r = e.value(&p.name);
        rows.push(ParamRow {
            name: p.name.clone(),
            expr: p.expr.clone(),
            value: r.as_ref().ok().copied(),
            error: r.err().map(|e| e.to_string()),
            kind: "user",
            constraint: None,
            description: p.description.clone(),
        });
    }
    rows
}

/// Is `name` used by a parameter or dimensional constraint?
pub fn name_in_use(d: &Drawing, name: &str) -> bool {
    d.parametric.parameters.iter().any(|p| p.name == name) || d.constraints.iter().any(|c| c.name == name)
}

/// Next free parameter name for a dimensional constraint kind (`d1`, `rad2`, `ang1`, …).
pub fn next_name(d: &Drawing, kind: ConstraintKind) -> String {
    let prefix = kind.name_prefix();
    (1..=100_000).map(|i| format!("{prefix}{i}")).find(|n| !name_in_use(d, n)).unwrap_or_else(|| format!("{prefix}_x"))
}

/// Next free constraint id.
pub fn next_id(d: &Drawing) -> u32 {
    d.constraints.iter().map(|c| c.id).max().map_or(1, |m| m.saturating_add(1))
}

// ---------------- systems ----------------

/// Interpret a constraint against the model's current parameters, appending its auxiliary
/// unknowns (contact parameters) to the model, settled with the geometry held still.
pub(crate) fn interpret(c: &Constraint, model: &mut Model, target: f64) -> Result<Built, String> {
    let x = model.x0.clone();
    let mut b = Built::new(c, model, &x, target)?;
    if b.aux_init.is_empty() {
        return Ok(b);
    }
    if model.x0.len() + b.aux_init.len() > model::MAX_PARAMS + 1000 {
        return Err("constraint system too large".into());
    }
    b.aux = model.x0.len();
    model.x0.extend(b.aux_init.iter().copied());
    let free: Vec<usize> = (b.aux..model.x0.len()).collect();
    let weights = vec![1.0; model.x0.len()];
    let o = {
        let m: &Model = model;
        let f = |x: &[f64], out: &mut Vec<f64>| b.eval(m, x, out);
        lm::solve(&f, &m.x0, &free, &weights, 1e-14, 60)
    };
    model.x0 = o.x;
    Ok(b)
}

/// One connected component of the constraint graph.
pub(crate) struct Comp {
    pub(crate) model: Model,
    pub(crate) built: Vec<Built>,
    weights: Vec<f64>,
    /// Parameters of the points held by [`SolveOptions::hold`].
    held: Vec<usize>,
    tol: f64,
}

impl Comp {
    fn locks(&self, skip: Option<usize>) -> Vec<bool> {
        let mut locked = vec![false; self.model.x0.len()];
        for &j in &self.held {
            if let Some(l) = locked.get_mut(j) {
                *l = true;
            }
        }
        for (i, b) in self.built.iter().enumerate() {
            if Some(i) == skip || b.form != Form::Lock {
                continue;
            }
            for r in &b.refs {
                for j in self.model.lock_indices(r).unwrap_or_default() {
                    if let Some(l) = locked.get_mut(j) {
                        *l = true;
                    }
                }
            }
        }
        locked
    }
    pub(crate) fn free(&self, skip: Option<usize>) -> Vec<usize> {
        self.locks(skip).iter().enumerate().filter(|(_, l)| !**l).map(|(i, _)| i).collect()
    }
    pub(crate) fn eval(&self, x: &[f64], out: &mut Vec<f64>, skip: Option<usize>) {
        for (i, b) in self.built.iter().enumerate() {
            if Some(i) != skip {
                b.eval(&self.model, x, out);
            }
        }
    }
    pub(crate) fn rows(&self, skip: Option<usize>) -> usize {
        self.built.iter().enumerate().filter(|(i, _)| Some(*i) != skip).map(|(_, b)| b.rows).sum()
    }
    fn run(&self, skip: Option<usize>) -> Result<lm::Outcome, SolveError> {
        let free = self.free(skip);
        let rows = self.rows(skip);
        if rows > lm::MAX_ROWS || (rows as f64) * (rows as f64) * (free.len() as f64) > 4e8 {
            return Err(SolveError::TooLarge);
        }
        let f = |x: &[f64], out: &mut Vec<f64>| self.eval(x, out, skip);
        let mut o = lm::solve(&f, &self.model.x0, &free, &self.weights, self.tol, MAX_ITER);
        // A solution the ellipses would be stored differently from (axes swapped, minor axis
        // mirrored) holds only if the constraints still hold on the stored form: a constraint on
        // an axis end by index does not.
        if o.converged && !self.model.storable(&o.x) {
            let mut out = Vec::new();
            match self.model.stored_form(&o.x) {
                Some(mut xc) => {
                    // Contact parameters are positions along the curves: they follow the stored form.
                    for b in self.built.iter().filter(|b| b.form == Form::Contact) {
                        for (k, r) in b.refs.iter().enumerate() {
                            if let (Some(&t), Some(slot)) = (o.x.get(b.aux + k), xc.get_mut(b.aux + k)) {
                                *slot = self.model.stored_t(&o.x, r.handle, t);
                            }
                        }
                    }
                    self.eval(&xc, &mut out, skip);
                    o.converged = lm::max_abs(&out) <= self.tol;
                    o.x = xc;
                }
                None => o.converged = false,
            }
        }
        Ok(o)
    }
    /// Constraint ids in conflict (each one whose removal makes the rest solvable).
    fn conflicts(&self, x_failed: &[f64]) -> Vec<u32> {
        let mut ids = Vec::new();
        if self.built.len() <= MAX_CONFLICT_PROBES {
            for i in 0..self.built.len() {
                if let Ok(o) = self.run(Some(i))
                    && o.converged
                    && let Some(b) = self.built.get(i)
                {
                    ids.push(b.id);
                }
            }
        }
        if ids.is_empty() {
            // Fall back to the constraints left unsatisfied.
            for b in &self.built {
                let mut out = Vec::new();
                b.eval(&self.model, x_failed, &mut out);
                if lm::max_abs(&out) > self.tol * 10.0 {
                    ids.push(b.id);
                }
            }
        }
        ids
    }
}

fn scale_tol(x: &[f64]) -> f64 {
    1e-9 * x.iter().fold(1.0f64, |m, v| m.max(v.abs()))
}

/// Group constraints into connected components by shared entities.
fn components<'a>(cons: &[&'a Constraint]) -> Vec<Vec<&'a Constraint>> {
    let mut parent: Vec<usize> = (0..cons.len()).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        let mut guard = 0;
        while let Some(&q) = p.get(i) {
            if q == i || guard > 100_000 {
                break;
            }
            let g = p.get(q).copied().unwrap_or(q);
            if let Some(slot) = p.get_mut(i) {
                *slot = g;
            }
            i = q;
            guard += 1;
        }
        i
    }
    let mut owner: HashMap<Handle, usize> = HashMap::new();
    for (i, c) in cons.iter().enumerate() {
        for r in &c.refs {
            match owner.get(&r.handle) {
                Some(&j) => {
                    let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                    if a != b
                        && let Some(slot) = parent.get_mut(a)
                    {
                        *slot = b;
                    }
                }
                None => {
                    owner.insert(r.handle, i);
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<&Constraint>> = BTreeMap::new();
    for (i, c) in cons.iter().enumerate() {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(c);
    }
    groups.into_values().collect()
}

/// The solver weight of each unknown of `model`: kept handles weigh more, preferred-to-move ones
/// less, angles and ratios by size², and contact parameters move freely.
fn solver_weights(model: &Model, opts: &SolveOptions) -> Vec<f64> {
    let mut weights = vec![1.0; model.x0.len()];
    for s in &model.slots {
        let w = if opts.keep.contains(&s.handle) {
            1e4
        } else if opts.prefer_move.contains(&s.handle) {
            1e-2
        } else {
            1.0
        };
        for j in s.off..s.off + s.len {
            if let Some(slot) = weights.get_mut(j) {
                *slot = w;
            }
        }
        for (j, f) in slot_scales(model, s) {
            if let Some(slot) = weights.get_mut(j) {
                *slot *= f;
            }
        }
    }
    // Contact parameters move freely.
    let geo = model.slots.iter().map(|s| s.off + s.len).max().unwrap_or(0);
    for w in weights.iter_mut().skip(geo) {
        *w = 1e-3;
    }
    weights
}

/// Per-unknown factors for the angles and ratios of one slot: size², so that the movement of
/// points is what is minimised.
fn slot_scales(model: &Model, s: &model::Slot) -> Vec<(usize, f64)> {
    let sq = |j: usize| model.x0.get(j).map_or(1.0, |r| (r * r).max(1e-6));
    match s.shape {
        model::Shape::Arc => vec![(s.off + 3, sq(s.off + 2)), (s.off + 4, sq(s.off + 2))],
        model::Shape::Ellipse { arc } => {
            let a2 = (sq(s.off + 2) + sq(s.off + 3)).max(1e-6);
            let mut v = vec![(s.off + 4, a2)];
            if arc {
                v.extend([(s.off + 5, a2), (s.off + 6, a2)]);
            }
            v
        }
        model::Shape::Insert { .. } => vec![(s.off + 2, model.rigid_extent2(s.handle).max(1.0))],
        _ => Vec::new(),
    }
}

/// Build the component systems. Constraints that cannot be interpreted are returned as skipped.
pub(crate) fn build(d: &Drawing, cons: &[&Constraint], opts: &SolveOptions) -> Result<(Vec<Comp>, Vec<(u32, String)>), SolveError> {
    let mut members_left = model::MAX_RIGID_WORK;
    build_budgeted(d, cons, opts, &mut members_left)
}

/// Build constraint components with one caller-owned rigid-member sampling budget shared by every model.
/// Return constraints that cannot be interpreted as skipped entries.
pub(crate) fn build_budgeted(
    d: &Drawing,
    cons: &[&Constraint],
    opts: &SolveOptions,
    members_left: &mut usize,
) -> Result<(Vec<Comp>, Vec<(u32, String)>), SolveError> {
    let mut e = env(d);
    let mut skipped = Vec::new();
    let mut comps = Vec::new();
    for group in components(cons) {
        let handles: Vec<Handle> = group.iter().flat_map(|c| c.refs.iter().map(|r| r.handle)).collect();
        let mut model = Model::build_budgeted(d, &handles, members_left);
        let mut built = Vec::new();
        for c in group {
            let target = if c.kind.is_dimensional() { constraint_value(&mut e, c)? } else { 0.0 };
            match interpret(c, &mut model, target) {
                Ok(mut b) => {
                    if let Form::Anchor(_) = b.form
                        && let Some(&(_, a)) = opts.anchors.iter().find(|(id, _)| *id == c.id)
                    {
                        b.form = Form::Anchor(a);
                    }
                    built.push(b);
                }
                Err(msg) => skipped.push((c.id, msg)),
            }
        }
        if built.is_empty() {
            continue;
        }
        let weights = solver_weights(&model, opts);
        let held = opts.hold.iter().filter_map(|r| model.lock_indices(r)).flatten().collect();
        let tol = scale_tol(&model.x0);
        comps.push(Comp { model, built, weights, held, tol });
    }
    Ok((comps, skipped))
}

/// Solve all of the drawing's constraints and write the geometry back. On failure nothing is
/// changed.
pub fn solve(d: &mut Drawing, opts: &SolveOptions) -> Result<SolveReport, SolveError> {
    let cons: Vec<&Constraint> = d.constraints.iter().take(MAX_CONSTRAINTS).collect();
    let (comps, skipped) = build(d, &cons, opts)?;
    let mut solutions = Vec::new();
    for c in comps {
        let o = c.run(None)?;
        if !o.converged {
            return Err(SolveError::Conflict { ids: c.conflicts(&o.x) });
        }
        solutions.push((c, o.x));
    }
    let mut moved = Vec::new();
    for (c, x) in solutions {
        moved.extend(c.model.write_back(d, &x));
    }
    Ok(SolveReport { moved, skipped })
}

/// Largest residual of each constraint at the current geometry (`None` when it cannot be
/// interpreted).
pub fn residuals(d: &Drawing) -> Vec<(u32, Option<f64>)> {
    let mut e = env(d);
    let handles: Vec<Handle> = d.constraints.iter().flat_map(|c| c.refs.iter().map(|r| r.handle)).collect();
    let mut model = Model::build(d, &handles);
    d.constraints
        .iter()
        .take(MAX_CONSTRAINTS)
        .map(|c| {
            let target = if c.kind.is_dimensional() { constraint_value(&mut e, c).ok() } else { Some(0.0) };
            let r = target.and_then(|t| interpret(c, &mut model, t).ok()).map(|b| {
                let mut out = Vec::new();
                b.eval(&model, &model.x0, &mut out);
                lm::max_abs(&out)
            });
            (c.id, r)
        })
        .collect()
}

/// Put back what each Fix locks where `before` had it, and report whether anything moved.
///
/// A Fix keeps no position of its own: the solver locks the parameters at their current values.
/// Without this, an edit that moves a fixed point (a grip drag) would carry the lock along with
/// it. Only Fixes that already existed in `before`, on objects of the same shape, are restored.
/// An object held by a Fix on a derived point (a line's midpoint, an arc's end) has nothing to
/// lock: when the edit only moved it (MOVE), it is put back whole, so it is not reshaped to bring
/// the point back.
pub fn restore_fixed(d: &mut Drawing, before: &Drawing) -> bool {
    let refs: Vec<GeomRef> = d
        .constraints
        .iter()
        .filter(|c| c.kind == ConstraintKind::Fix && before.constraints.contains(c))
        .flat_map(|c| c.refs.iter().copied())
        .collect();
    if refs.is_empty() {
        return false;
    }
    let handles: Vec<Handle> = refs.iter().map(|r| r.handle).collect();
    let (now, old) = (Model::build(d, &handles), Model::build(before, &handles));
    let mut x = now.x0.clone();
    let mut changed = false;
    for r in &refs {
        let same_shape = matches!((now.slot(r.handle), old.slot(r.handle)), (Some(a), Some(b)) if a.shape == b.shape);
        if !same_shape {
            continue;
        }
        let (Some(i), Some(j)) = (now.lock_indices(r), old.lock_indices(r)) else {
            if let (Some(a), Some(b)) = (now.slot(r.handle), old.slot(r.handle))
                && translated(&now, &old, r.handle)
                && let (Some(dst), Some(src)) = (x.get_mut(a.off..a.off + a.len), old.x0.get(b.off..b.off + b.len))
            {
                for (v, &w) in dst.iter_mut().zip(src) {
                    *v = w;
                }
                changed = true;
            }
            continue;
        };
        for (i, j) in i.into_iter().zip(j) {
            if let (Some(slot), Some(&v)) = (x.get_mut(i), old.x0.get(j))
                && *slot != v
            {
                *slot = v;
                changed = true;
            }
        }
    }
    if changed {
        now.write_back(d, &x);
    }
    changed
}

/// The parameters of `h`'s points (see [`Model::point_refs`]) in `m`, as (x, y) pairs.
fn point_params(m: &Model, h: Handle) -> Vec<(usize, usize)> {
    m.point_refs(h)
        .iter()
        .filter_map(|r| m.lock_indices(r))
        .flat_map(|ix| ix.as_chunks::<2>().0.iter().map(|&[x, y]| (x, y)).collect::<Vec<_>>())
        .collect()
}

/// Is `h` in `now` its shape in `old` moved by one vector, and nothing else?
fn translated(now: &Model, old: &Model, h: Handle) -> bool {
    let (Some(a), Some(b)) = (now.slot(h), old.slot(h)) else { return false };
    let (Some(xa), Some(xb)) = (now.x0.get(a.off..a.off + a.len), old.x0.get(b.off..b.off + b.len)) else { return false };
    let tol = scale_tol(xb) * 10.0;
    let pts: Vec<(usize, usize)> =
        point_params(old, h).into_iter().filter_map(|(i, j)| Some((i.checked_sub(b.off)?, j.checked_sub(b.off)?))).collect();
    let delta = |(i, j): (usize, usize)| Some(Vec2::new(xa.get(i)? - xb.get(i)?, xa.get(j)? - xb.get(j)?));
    let Some(v) = pts.first().and_then(|&p| delta(p)) else { return false };
    if v.len() <= tol || !pts.iter().all(|&p| delta(p).is_some_and(|d| d.near(v, tol))) {
        return false;
    }
    (0..xb.len())
        .filter(|k| !pts.iter().any(|&(i, j)| *k == i || *k == j))
        .all(|k| matches!((xa.get(k), xb.get(k)), (Some(p), Some(q)) if (p - q).abs() <= tol))
}

/// The points of `h` (its ends, centre or vertices) the edit from `before` to `d` moved, when it
/// moved some of them but not all: the grip that was dragged, to pass as [`SolveOptions::hold`] so
/// it stays where it was put and the rest of the object takes up the constraints. Empty when the
/// object kept its points or moved them all.
pub fn edited_points(d: &Drawing, before: &Drawing, h: Handle) -> Vec<GeomRef> {
    let (now, old) = (Model::build(d, &[h]), Model::build(before, &[h]));
    if !matches!((now.slot(h), old.slot(h)), (Some(a), Some(b)) if a.shape == b.shape) {
        return Vec::new();
    }
    let tol = scale_tol(&old.x0) * 10.0;
    let refs = now.point_refs(h);
    let moved: Vec<GeomRef> = refs
        .iter()
        .filter(|r| matches!((now.lock_indices(r), old.lock_indices(r)), (Some(i), Some(j)) if i.iter().zip(&j).any(|(&i, &j)| matches!((now.x0.get(i), old.x0.get(j)), (Some(p), Some(q)) if (p - q).abs() > tol))))
        .copied()
        .collect();
    if moved.len() == refs.len() { Vec::new() } else { moved }
}

/// Where `before` had each derived point held by a Fix (a line's midpoint, an arc's end), for the
/// Fixes whose point has moved since, to pass as [`SolveOptions::anchors`].
///
/// A Fix on a derived point anchors it where it is when the system is built, and
/// [`restore_fixed`] has no parameters to put back: without this, an edit that moves the point
/// would leave the Fix satisfied at the new place.
pub fn moved_anchors(d: &Drawing, before: &Drawing) -> Vec<(u32, Vec2)> {
    let fixes: Vec<(u32, GeomRef)> = d
        .constraints
        .iter()
        .filter(|c| c.kind == ConstraintKind::Fix && before.constraints.contains(c))
        .filter_map(|c| match c.refs.as_slice() {
            [r] => Some((c.id, *r)),
            _ => None,
        })
        .collect();
    if fixes.is_empty() {
        return Vec::new();
    }
    let handles: Vec<Handle> = fixes.iter().map(|(_, r)| r.handle).collect();
    let (now, old) = (Model::build(d, &handles), Model::build(before, &handles));
    let tol = scale_tol(&old.x0);
    let mut out = Vec::new();
    for (id, r) in fixes {
        let same_shape = matches!((now.slot(r.handle), old.slot(r.handle)), (Some(a), Some(b)) if a.shape == b.shape);
        if !same_shape || now.lock_indices(&r).is_some() {
            continue;
        }
        if let (Some(p), Some(a)) = (now.point(&now.x0, &r), old.point(&old.x0, &r))
            && !p.near(a, tol)
        {
            out.push((id, a));
        }
    }
    out
}

/// True when every interpretable constraint holds at the current geometry.
pub fn all_satisfied(d: &Drawing) -> bool {
    let handles: Vec<Handle> = d.constraints.iter().flat_map(|c| c.refs.iter().map(|r| r.handle)).collect();
    let tol = scale_tol(&Model::build(d, &handles).x0) * 10.0;
    residuals(d).iter().all(|(_, r)| r.is_none_or(|v| v <= tol))
}

/// Check that a constraint's references make sense for its type.
pub fn validate(d: &Drawing, c: &Constraint) -> Result<(), String> {
    let handles: Vec<Handle> = c.refs.iter().map(|r| r.handle).collect();
    for h in &handles {
        match d.entity(*h) {
            None => return Err(format!("no such object {}", h.hex())),
            Some(e) if !supported(&e.kind) => return Err(format!("{} objects cannot be constrained", e.kind.type_name())),
            _ => {}
        }
    }
    let mut model = Model::build(d, &handles);
    interpret(c, &mut model, 1.0).map(|_| ())
}

/// The current measured value of a dimensional constraint over `refs` (degrees for angular).
pub fn measure(d: &Drawing, kind: ConstraintKind, refs: &[GeomRef]) -> Result<f64, String> {
    let c = Constraint { id: 0, kind, refs: refs.to_vec(), name: String::new(), expr: String::new() };
    let handles: Vec<Handle> = refs.iter().map(|r| r.handle).collect();
    let mut model = Model::build(d, &handles);
    let b = interpret(&c, &mut model, 1.0)?;
    Ok(b.measure(&model, &model.x0))
}

/// Remove constraints whose objects are gone or no longer fit (e.g. a polyline lost vertices).
/// Returns the removed ids.
pub fn purge_invalid(d: &mut Drawing) -> Vec<u32> {
    let mut gone = Vec::new();
    let keep: Vec<bool> = d.constraints.iter().map(|c| validate(d, c).is_ok()).collect();
    let mut i = 0;
    d.constraints.retain(|c| {
        let k = keep.get(i).copied().unwrap_or(true);
        i += 1;
        if !k {
            gone.push(c.id);
        }
        k
    });
    gone
}

/// Add a constraint: assigns its id (and name/expression for dimensional constraints when
/// empty), refuses redundant or conflicting constraints, solves and writes the geometry back.
/// On error the drawing is unchanged.
pub fn add(d: &mut Drawing, mut c: Constraint, opts: &SolveOptions) -> Result<u32, SolveError> {
    if d.constraints.len() >= MAX_CONSTRAINTS {
        return Err(SolveError::Other("too many constraints".into()));
    }
    c.id = next_id(d);
    validate(d, &c).map_err(|msg| SolveError::Invalid { id: c.id, msg })?;
    if c.kind.is_dimensional() {
        if c.name.is_empty() {
            c.name = next_name(d, c.kind);
        }
        if !expr::valid_name(&c.name) {
            return Err(SolveError::Other(format!("invalid parameter name `{}`", c.name)));
        }
        if name_in_use(d, &c.name) {
            return Err(SolveError::Other(format!("parameter name `{}` is already used", c.name)));
        }
        if c.expr.trim().is_empty() {
            let v = measure(d, c.kind, &c.refs).map_err(|msg| SolveError::Invalid { id: c.id, msg })?;
            c.expr = format_value(v);
        }
    } else {
        c.name.clear();
        c.expr.clear();
    }
    let id = c.id;
    let mut trial = d.clone();
    trial.constraints.push(c);
    // Solve only the component holding the new constraint (others are untouched).
    let cons: Vec<&Constraint> = trial.constraints.iter().collect();
    let (comps, skipped) = build(&trial, &cons, opts)?;
    if let Some((_, msg)) = skipped.iter().find(|(i, _)| *i == id) {
        return Err(SolveError::Invalid { id, msg: msg.clone() });
    }
    let Some(comp) = comps.into_iter().find(|k| k.built.iter().any(|b| b.id == id)) else {
        return Err(SolveError::Invalid { id, msg: "could not be interpreted".into() });
    };
    let o = comp.run(None)?;
    if !o.converged {
        // The new constraint first, then the existing ones it fights with.
        let mut ids = comp.conflicts(&o.x);
        ids.retain(|i| *i != id);
        ids.insert(0, id);
        return Err(SolveError::Conflict { ids });
    }
    // Redundancy: the new rows must raise the rank.
    if let Some(nb) = comp.built.iter().find(|b| b.id == id)
        && nb.rows > 0
    {
        let free = comp.free(None);
        let mut ordered: Vec<&Built> = comp.built.iter().filter(|b| b.id != id).collect();
        ordered.push(nb);
        let f = |x: &[f64], out: &mut Vec<f64>| {
            for b in &ordered {
                b.eval(&comp.model, x, out);
            }
        };
        let rows_all: usize = ordered.iter().map(|b| b.rows).sum();
        let rows_old = rows_all - nb.rows;
        let nf = free.len();
        let jac = lm::jacobian(&f, &o.x, &free, rows_all);
        let r_all = lm::rank_qr(&jac, rows_all, nf);
        let r_old = lm::rank_qr(jac.get(..rows_old * nf).unwrap_or_default(), rows_old, nf);
        if r_all < r_old + nb.rows {
            return Err(SolveError::Redundant { ids: redundant_with(&comp, &ordered, &jac, nf, id) });
        }
    }
    // A Fix that locks parameters adds one equation per parameter it locks.
    if let Some(pos) = comp.built.iter().position(|b| b.id == id)
        && comp.built.get(pos).is_some_and(|b| b.form == Form::Lock)
    {
        let before = comp.free(Some(pos));
        let after = comp.free(None);
        let locked: Vec<usize> = before.iter().enumerate().filter(|(_, j)| !after.contains(j)).map(|(k, _)| k).collect();
        let rows_old = comp.rows(Some(pos));
        let nf = before.len();
        let f = |x: &[f64], out: &mut Vec<f64>| comp.eval(x, out, Some(pos));
        let mut jac = lm::jacobian(&f, &o.x, &before, rows_old);
        let r_old = lm::rank_qr(&jac, rows_old, nf);
        for k in &locked {
            jac.extend((0..nf).map(|c| if c == *k { 1.0 } else { 0.0 }));
        }
        if locked.is_empty() || lm::rank_qr(&jac, rows_old + locked.len(), nf) < r_old + locked.len() {
            let mine: Vec<Handle> = comp.built.get(pos).map(|b| b.refs.iter().map(|r| r.handle).collect()).unwrap_or_default();
            let ids = comp.built.iter().filter(|b| b.id != id && b.refs.iter().any(|r| mine.contains(&r.handle))).map(|b| b.id).collect();
            return Err(SolveError::Redundant { ids });
        }
    }
    comp.model.write_back(&mut trial, &o.x);
    *d = trial;
    Ok(id)
}

/// The existing constraints a redundant new one (last in `ordered`) duplicates: each one whose
/// removal would let the new one add independent equations. Falls back to the constraints on
/// the same objects.
fn redundant_with(comp: &Comp, ordered: &[&Built], jac: &[f64], nf: usize, id: u32) -> Vec<u32> {
    let mut starts = Vec::with_capacity(ordered.len());
    let mut at = 0;
    for b in ordered {
        starts.push(at);
        at += b.rows;
    }
    let pick = |skip: usize, with_new: bool| -> (Vec<f64>, usize) {
        let mut m = Vec::new();
        let mut rows = 0;
        let last = ordered.len().saturating_sub(1);
        for (i, b) in ordered.iter().enumerate() {
            if i == skip || (i == last && !with_new) {
                continue;
            }
            let s = starts.get(i).copied().unwrap_or(0);
            if let Some(part) = jac.get(s * nf..(s + b.rows) * nf) {
                m.extend_from_slice(part);
                rows += b.rows;
            }
        }
        (m, rows)
    };
    let new_rows = ordered.last().map_or(0, |b| b.rows);
    let mut ids = Vec::new();
    if ordered.len() <= MAX_CONFLICT_PROBES + 1 {
        for (i, b) in ordered.iter().enumerate().take(ordered.len().saturating_sub(1)) {
            if b.rows == 0 {
                continue;
            }
            let (old, ro) = pick(i, false);
            let (all, ra) = pick(i, true);
            if lm::rank_qr(&all, ra, nf) == lm::rank_qr(&old, ro, nf) + new_rows {
                ids.push(b.id);
            }
        }
    }
    if ids.is_empty() {
        let mine: Vec<Handle> = ordered.last().map(|b| b.refs.iter().map(|r| r.handle).collect()).unwrap_or_default();
        ids = comp.built.iter().filter(|b| b.id != id && b.refs.iter().any(|r| mine.contains(&r.handle))).map(|b| b.id).collect();
    }
    ids
}

/// Format a value for an expression (up to 6 decimals, trailing zeros trimmed).
pub fn format_value(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".into() } else { s.to_string() }
}

/// Set a parameter's expression (dimensional constraint name or user parameter; a new name
/// creates a user parameter), then re-solve. On error the drawing is unchanged.
pub fn set_parameter(d: &mut Drawing, name: &str, expr_text: &str, opts: &SolveOptions) -> Result<SolveReport, SolveError> {
    if !expr::valid_name(name) {
        return Err(SolveError::Other(format!("invalid parameter name `{name}`")));
    }
    expr::parse(expr_text).map_err(|err| SolveError::Expr { name: name.into(), err })?;
    let mut trial = d.clone();
    if let Some(c) = trial.constraints.iter_mut().find(|c| c.name == name) {
        c.expr = expr_text.trim().to_string();
    } else if let Some(p) = trial.parametric.parameters.iter_mut().find(|p| p.name == name) {
        p.expr = expr_text.trim().to_string();
    } else {
        trial.parametric.parameters.push(cadcraft_doc::Parameter { name: name.into(), expr: expr_text.trim().into(), description: String::new() });
    }
    // Every expression must still evaluate.
    for row in parameter_table(&trial) {
        if let Some(err) = row.error {
            return Err(SolveError::Other(format!("parameter `{}`: {err}", row.name)));
        }
    }
    let report = solve(&mut trial, opts)?;
    // A new value that a constraint rejects (negative distance, non-positive radius) would silently
    // stop enforcing it, so refuse it like `add` does. Constraints already skipped before stay allowed.
    if !report.skipped.is_empty() {
        let cons: Vec<&Constraint> = d.constraints.iter().take(MAX_CONSTRAINTS).collect();
        let before = build(d, &cons, opts).map(|(_, skipped)| skipped).unwrap_or_default();
        if let Some((id, msg)) = report.skipped.iter().find(|(id, _)| !before.iter().any(|(b, _)| b == id)) {
            return Err(SolveError::Invalid { id: *id, msg: msg.clone() });
        }
    }
    *d = trial;
    Ok(report)
}

/// Rename a parameter (dimensional constraint name or user parameter) and every expression that
/// uses it. Geometry doesn't change. On error the drawing is unchanged.
pub fn rename_parameter(d: &mut Drawing, old: &str, new: &str) -> Result<(), SolveError> {
    if !name_in_use(d, old) {
        return Err(SolveError::Other(format!("no parameter `{old}`")));
    }
    if !expr::valid_name(new) {
        return Err(SolveError::Other(format!("invalid parameter name `{new}`")));
    }
    if new != old && name_in_use(d, new) {
        return Err(SolveError::Other(format!("`{new}` is already in use")));
    }
    for c in d.constraints.iter_mut().filter(|c| c.kind.is_dimensional()) {
        if c.name == old {
            c.name = new.to_string();
        }
        c.expr = expr::rename(&c.expr, old, new);
    }
    for p in &mut d.parametric.parameters {
        if p.name == old {
            p.name = new.to_string();
        }
        p.expr = expr::rename(&p.expr, old, new);
    }
    Ok(())
}

/// Where to draw a constraint's glyphs (one anchor per reference).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Glyph {
    pub id: u32,
    pub kind: &'static str,
    pub dimensional: bool,
    pub handles: Vec<String>,
    pub anchors: Vec<Vec2>,
    pub name: String,
    pub expr: String,
    pub value: Option<f64>,
    pub satisfied: bool,
}

/// Glyph anchors for every constraint (for the constraint bars / dynamic constraints UI).
pub fn glyphs(d: &Drawing) -> Vec<Glyph> {
    let handles: Vec<Handle> = d.constraints.iter().flat_map(|c| c.refs.iter().map(|r| r.handle)).collect();
    let model = Model::build(d, &handles);
    let x = &model.x0;
    let tol = scale_tol(x) * 10.0;
    let res: HashMap<u32, Option<f64>> = residuals(d).into_iter().collect();
    let mut e = env(d);
    d.constraints
        .iter()
        .take(MAX_CONSTRAINTS)
        .map(|c| {
            let anchors = c
                .refs
                .iter()
                .filter_map(|r| {
                    model
                        .point(x, r)
                        .or_else(|| model.line(x, r).map(|(a, b)| a.mid(b)))
                        .or_else(|| model.circle(x, r).map(|(c, rad)| c + Vec2::new(0.0, rad)))
                        .or_else(|| model.anchor(x, r))
                })
                .collect();
            Glyph {
                id: c.id,
                kind: c.kind.name(),
                dimensional: c.kind.is_dimensional(),
                handles: c.refs.iter().map(|r| r.handle.hex()).collect(),
                anchors,
                name: c.name.clone(),
                expr: c.expr.clone(),
                value: if c.kind.is_dimensional() { constraint_value(&mut e, c).ok() } else { None },
                satisfied: res.get(&c.id).copied().flatten().is_some_and(|v| v <= tol),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
