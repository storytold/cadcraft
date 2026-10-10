//! CADCraft parametric solver.
//!
//! Maps constrained lines, arcs, circles, points and polyline vertices to a parameter vector
//! ([`model`]), turns each [`Constraint`] into residual equations ([`residual`]), and drives the
//! residuals to zero with a weighted minimum-norm damped Gauss–Newton solver ([`lm`]), so
//! geometry moves as little as possible and geometry the caller wants kept (just edited, or picked
//! first) moves least. Fix locks parameters. Constraints are solved per connected component.
//!
//! Inconsistent systems are not "solved" by moving things wildly: the solve fails, nothing is
//! written back, and the error names the conflicting constraints (each one whose removal makes
//! the rest solvable). New constraints that add no independent equation are reported as
//! redundant (over-constraining), as AutoCAD does.
//!
//! Own implementation (clean-room): the KittyCAD `ezpz` crate (MIT) was evaluated; it pulls in
//! the `faer` linear-algebra stack and models KCL's geometry, so a small solver tailored to the
//! drawing database (minimal movement, conflict reporting) was written instead.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod auto;
pub mod expr;
mod lm;
mod model;
mod residual;

use std::collections::{BTreeMap, HashMap};

use cadcraft_doc::{Constraint, ConstraintKind, Drawing, GeomRef, Handle};
use cadcraft_geom::Vec2;
use serde::Serialize;

pub use auto::{InferOptions, autoconstrain, infer};
pub use expr::{Env, ExprError};
pub use model::supported;

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

/// One connected component of the constraint graph.
struct Comp {
    model: Model,
    built: Vec<Built>,
    weights: Vec<f64>,
    tol: f64,
}

impl Comp {
    fn locks(&self, skip: Option<usize>) -> Vec<bool> {
        let mut locked = vec![false; self.model.x0.len()];
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
    fn free(&self, skip: Option<usize>) -> Vec<usize> {
        self.locks(skip).iter().enumerate().filter(|(_, l)| !**l).map(|(i, _)| i).collect()
    }
    fn eval(&self, x: &[f64], out: &mut Vec<f64>, skip: Option<usize>) {
        for (i, b) in self.built.iter().enumerate() {
            if Some(i) != skip {
                b.eval(&self.model, x, out);
            }
        }
    }
    fn rows(&self, skip: Option<usize>) -> usize {
        self.built.iter().enumerate().filter(|(i, _)| Some(*i) != skip).map(|(_, b)| b.rows).sum()
    }
    fn run(&self, skip: Option<usize>) -> Result<lm::Outcome, SolveError> {
        let free = self.free(skip);
        let rows = self.rows(skip);
        if rows > lm::MAX_ROWS || (rows as f64) * (rows as f64) * (free.len() as f64) > 4e8 {
            return Err(SolveError::TooLarge);
        }
        let f = |x: &[f64], out: &mut Vec<f64>| self.eval(x, out, skip);
        Ok(lm::solve(&f, &self.model.x0, &free, &self.weights, self.tol, MAX_ITER))
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

/// Build the component systems. Constraints that cannot be interpreted are returned as skipped.
fn build(d: &Drawing, cons: &[&Constraint], opts: &SolveOptions) -> Result<(Vec<Comp>, Vec<(u32, String)>), SolveError> {
    let mut e = env(d);
    let mut skipped = Vec::new();
    let mut comps = Vec::new();
    for group in components(cons) {
        let handles: Vec<Handle> = group.iter().flat_map(|c| c.refs.iter().map(|r| r.handle)).collect();
        let model = Model::build(d, &handles);
        let mut built = Vec::new();
        for c in group {
            let target = if c.kind.is_dimensional() { constraint_value(&mut e, c)? } else { 0.0 };
            match Built::new(c, &model, &model.x0, target) {
                Ok(b) => built.push(b),
                Err(msg) => skipped.push((c.id, msg)),
            }
        }
        if built.is_empty() {
            continue;
        }
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
            if s.shape == model::Shape::Arc {
                // Angles: weight by r² so the endpoints' movement is what is minimised.
                let r2 = model.x0.get(s.off + 2).map_or(1.0, |r| (r * r).max(1e-6));
                for j in [s.off + 3, s.off + 4] {
                    if let Some(slot) = weights.get_mut(j) {
                        *slot *= r2;
                    }
                }
            }
        }
        let tol = scale_tol(&model.x0);
        comps.push(Comp { model, built, weights, tol });
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
    let model = Model::build(d, &handles);
    d.constraints
        .iter()
        .take(MAX_CONSTRAINTS)
        .map(|c| {
            let target = if c.kind.is_dimensional() { constraint_value(&mut e, c).ok() } else { Some(0.0) };
            let r = target.and_then(|t| Built::new(c, &model, &model.x0, t).ok()).map(|b| {
                let mut out = Vec::new();
                b.eval(&model, &model.x0, &mut out);
                lm::max_abs(&out)
            });
            (c.id, r)
        })
        .collect()
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
    let model = Model::build(d, &handles);
    Built::new(c, &model, &model.x0, 1.0).map(|_| ())
}

/// The current measured value of a dimensional constraint over `refs` (degrees for angular).
pub fn measure(d: &Drawing, kind: ConstraintKind, refs: &[GeomRef]) -> Result<f64, String> {
    let c = Constraint { id: 0, kind, refs: refs.to_vec(), name: String::new(), expr: String::new() };
    let handles: Vec<Handle> = refs.iter().map(|r| r.handle).collect();
    let model = Model::build(d, &handles);
    let b = Built::new(&c, &model, &model.x0, 1.0)?;
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
    if let Some(pos) = comp.built.iter().position(|b| b.id == id)
        && let Some(nb) = comp.built.get(pos)
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
        let jac = lm::jacobian(&f, &o.x, &free, rows_all);
        let r_all = lm::rank(&jac, rows_all, free.len());
        let r_old = lm::rank(jac.get(..rows_old * free.len()).unwrap_or_default(), rows_old, free.len());
        if r_all < r_old + nb.rows {
            let mine: Vec<Handle> = nb.refs.iter().map(|r| r.handle).collect();
            let ids = comp.built.iter().filter(|b| b.id != id && b.refs.iter().any(|r| mine.contains(&r.handle))).map(|b| b.id).collect();
            return Err(SolveError::Redundant { ids });
        }
    }
    comp.model.write_back(&mut trial, &o.x);
    *d = trial;
    Ok(id)
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
