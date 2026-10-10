//! Parametric drawing: GEOMCONSTRAINT and the GC* commands, DIMCONSTRAINT and the DC* commands,
//! AUTOCONSTRAIN, DELCONSTRAINT, PARAMETERS, CONSTRAINTBAR, DCDISPLAY, CONSTRAINTSETTINGS and
//! `constraints.inspect`, plus the post-command re-solve hook ([`after_command`]).
//!
//! The solver lives in `cadcraft-constraints`; this module turns picks and JSON parameters into
//! constraint references and keeps the drawing's constraints satisfied after edits.

use std::sync::Arc;

use cadcraft_constraints as cs;
use cadcraft_doc::{Constraint, ConstraintKind as K, DistAxis, Drawing, EntityKind, GeomRef, Handle, Sub};
use cadcraft_geom::{Line, Vec2};
use serde_json::{Value, json};

use super::machines::{SelOutcome, SelectPhase};
use super::*;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

macro_rules! gc {
    ($id:literal, $label:literal, $kind:expr, $menu:literal) => {
        CommandSpec::new($id, $label, |s, p| run_gc(s, p, $kind))
            .menu(&["Tools", "Parametric", "Geometric Constraints", $menu])
            .params("{h1, p1?, h2?, p2?, h3?, p3?} (h = handle; p = \"start\"/\"end\"/\"mid\"/\"center\"/\"v<i>\"/\"seg<i>\"/\"object\" or a pick point [x, y])")
            .interactive(|_| Ok(Box::new(GcM::new($kind))))
    };
}

macro_rules! dc {
    ($id:literal, $label:literal, $dc:expr, $menu:literal) => {
        CommandSpec::new($id, $label, |s, p| run_dc(s, p, $dc))
            .menu(&["Tools", "Parametric", "Dimensional Constraints", $menu])
            .params("{h1, p1?, h2?, p2?, expr? | value?, name?}")
            .interactive(|_| Ok(Box::new(DcM::new(Some($dc)))))
    };
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("geomconstraint", "Geometric Constraint", run_geomconstraint)
            .alias(&["gcon"])
            .params("{type: coincident|collinear|concentric|fix|parallel|perpendicular|horizontal|vertical|tangent|smooth|symmetric|equal, h1, p1?, h2?, p2?, h3?, p3?}")
            .interactive(|_| Ok(Box::new(GeomM::default()))),
        gc!("gccoincident", "Coincident", K::Coincident, "Coincident"),
        gc!("gcperpendicular", "Perpendicular", K::Perpendicular, "Perpendicular"),
        gc!("gcparallel", "Parallel", K::Parallel, "Parallel"),
        gc!("gctangent", "Tangent", K::Tangent, "Tangent"),
        gc!("gchorizontal", "Horizontal", K::Horizontal, "Horizontal"),
        gc!("gcvertical", "Vertical", K::Vertical, "Vertical"),
        gc!("gccollinear", "Collinear", K::Collinear, "Collinear"),
        gc!("gcconcentric", "Concentric", K::Concentric, "Concentric"),
        gc!("gcsmooth", "Smooth", K::Smooth, "Smooth"),
        gc!("gcsymmetric", "Symmetric", K::Symmetric, "Symmetric"),
        gc!("gcequal", "Equal", K::Equal, "Equal"),
        gc!("gcfix", "Fix", K::Fix, "Fix"),
        CommandSpec::new("autoconstrain", "AutoConstrain", run_autoconstrain)
            .menu(&["Tools", "Parametric", "AutoConstrain"])
            .params("{handles?, tolerance?, angleTolerance? (degrees), types?: [names]}")
            .interactive(|_| Ok(Box::new(SelM::new(SelOp::Auto)))),
        CommandSpec::new("constraintbar", "Constraint Bars", run_constraintbar)
            .menu(&["Tools", "Parametric", "Constraint Bars", "Select Objects"])
            .params("{mode?: show|hide|showall|hideall, handles?}")
            .interactive(|_| Ok(Box::new(SelM::new(SelOp::Bars)))),
        CommandSpec::new("constraintbar.showall", "Show All Constraint Bars", |s, _| set_bars(s, false, "showall", &[]))
            .menu(&["Tools", "Parametric", "Constraint Bars", "Show All"]),
        CommandSpec::new("constraintbar.hideall", "Hide All Constraint Bars", |s, _| set_bars(s, false, "hideall", &[]))
            .menu(&["Tools", "Parametric", "Constraint Bars", "Hide All"]),
        CommandSpec::new("dimconstraint", "Dimensional Constraint", run_dimconstraint)
            .alias(&["dcon"])
            .params("{type: linear|aligned|horizontal|vertical|angular|radius|diameter, h1, p1?, h2?, p2?, expr? | value?, name?}")
            .interactive(|_| Ok(Box::new(DcM::new(None)))),
        dc!("dcaligned", "Aligned", Dc::Aligned, "Aligned"),
        dc!("dchorizontal", "Horizontal", Dc::Horizontal, "Horizontal"),
        dc!("dcvertical", "Vertical", Dc::Vertical, "Vertical"),
        dc!("dcangular", "Angular", Dc::Angular, "Angular"),
        dc!("dcradius", "Radius", Dc::Radius, "Radius"),
        dc!("dcdiameter", "Diameter", Dc::Diameter, "Diameter"),
        CommandSpec::new("dclinear", "Linear", |s, p| run_dc(s, p, Dc::Linear))
            .params("{h1, p1?, h2?, p2?, expr? | value?, name?}")
            .interactive(|_| Ok(Box::new(DcM::new(Some(Dc::Linear))))),
        CommandSpec::new("dcdisplay", "Dynamic Dimensions", run_dcdisplay)
            .menu(&["Tools", "Parametric", "Dynamic Dimensions", "Select Objects"])
            .params("{mode?: show|hide|showall|hideall, handles?}")
            .interactive(|_| Ok(Box::new(SelM::new(SelOp::Dims)))),
        CommandSpec::new("dcdisplay.showall", "Show All Dynamic Dimensions", |s, _| set_bars(s, true, "showall", &[]))
            .menu(&["Tools", "Parametric", "Dynamic Dimensions", "Show All"]),
        CommandSpec::new("dcdisplay.hideall", "Hide All Dynamic Dimensions", |s, _| set_bars(s, true, "hideall", &[]))
            .menu(&["Tools", "Parametric", "Dynamic Dimensions", "Hide All"]),
        CommandSpec::new("delconstraint", "Delete Constraints", run_delconstraint)
            .menu(&["Tools", "Parametric", "Delete Constraints"])
            .params("{handles? | ids?: [n]}")
            .interactive(|_| Ok(Box::new(SelM::new(SelOp::Delete)))),
        CommandSpec::new("parameters", "Parameters Manager", run_parameters)
            .menu(&["Tools", "Parametric", "Parameters Manager"])
            .alias(&["par", "parametersclose"])
            .params("{} lists; {name, expr | value, description?} sets (new name = user parameter) and re-solves; {delete: name}"),
        CommandSpec::new("constraintsettings", "Constraint Settings", run_constraintsettings)
            .menu(&["Tools", "Parametric", "Constraint Settings"])
            .alias(&["csettings"])
            .params("{infer?, distanceTolerance?, angleTolerance?, autoTypes?: [names], barsVisible?, barTransparency?, dimsVisible?}"),
        CommandSpec::new("constraints.inspect", "Inspect Constraints", run_inspect).noundo().params("{} → constraints, glyph anchors, parameters, settings"),
    ]
}

fn err(e: cs::SolveError) -> EngineError {
    EngineError::Other(e.to_string())
}

fn h_param(p: &Value, k: &str) -> Option<Handle> {
    p.get(k).and_then(|v| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle)))
}

// ---------------- references ----------------

/// What a constraint slot needs from the picked object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Want {
    Point,
    Line,
    Curve,
    /// A line, polyline segment, circle, arc or point object.
    Object,
    /// A characteristic point near the pick, else the object.
    PointOrObject,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Hint {
    None,
    Sub(Sub),
    Pick(Vec2),
}

fn hint_param(cmd: &str, p: &Value, key: &str) -> Result<Hint> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(Hint::None),
        Some(Value::String(t)) => match Sub::parse(t) {
            Some(sub) => Ok(Hint::Sub(sub)),
            None => point_value(&Value::String(t.clone())).map(Hint::Pick).ok_or_else(|| bad(cmd, format!("`{key}`: unknown point `{t}`"))),
        },
        Some(v) => point_value(v).map(Hint::Pick).ok_or_else(|| bad(cmd, format!("`{key}` must be a point name or [x, y]"))),
    }
}

/// Characteristic points of an object (end points, mid points, centres, vertices).
fn char_points(kind: &EntityKind) -> Vec<(Sub, Vec2)> {
    match kind {
        EntityKind::Line(l) => vec![(Sub::Start, l.a.xy()), (Sub::End, l.b.xy()), (Sub::Mid, l.a.xy().mid(l.b.xy()))],
        EntityKind::Arc(a) => {
            let g = cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end);
            vec![(Sub::Start, g.start_point()), (Sub::End, g.end_point()), (Sub::Mid, g.mid_point()), (Sub::Center, g.center)]
        }
        EntityKind::Circle(c) => vec![(Sub::Center, c.center.xy())],
        EntityKind::Point(p) => vec![(Sub::Whole, p.p.xy())],
        EntityKind::LwPolyline(p) => p.vertices.iter().take(100_000).enumerate().map(|(i, v)| (Sub::Vertex(i as u32), v.p)).collect(),
        _ => Vec::new(),
    }
}

/// Line-like parts of an object (a line, or each polyline segment).
fn line_parts(kind: &EntityKind) -> Vec<(Sub, Vec2, Vec2)> {
    match kind {
        EntityKind::Line(l) => vec![(Sub::Whole, l.a.xy(), l.b.xy())],
        EntityKind::LwPolyline(p) => {
            let n = p.vertices.len().min(100_000);
            let segs = if p.closed && n > 2 { n } else { n.saturating_sub(1) };
            (0..segs).filter_map(|i| Some((Sub::Segment(i as u32), p.vertices.get(i)?.p, p.vertices.get((i + 1) % n)?.p))).collect()
        }
        _ => Vec::new(),
    }
}

fn object_size(kind: &EntityKind) -> f64 {
    match kind {
        EntityKind::Line(l) => l.a.xy().dist(l.b.xy()),
        EntityKind::Arc(a) => a.radius,
        EntityKind::Circle(c) => c.radius,
        EntityKind::LwPolyline(p) => p.vertices.windows(2).filter_map(|w| Some(w.first()?.p.dist(w.get(1)?.p))).fold(0.0, f64::max),
        _ => 0.0,
    }
    .abs()
}

fn resolve(d: &Drawing, h: Handle, hint: Hint, want: Want, aperture: f64) -> std::result::Result<GeomRef, String> {
    let e = d.entity(h).ok_or_else(|| format!("no such object {}", h.hex()))?;
    if !cs::supported(&e.kind) {
        return Err(format!("{} objects cannot be constrained", e.kind.type_name()));
    }
    let k = &e.kind;
    let object = |at: Option<Vec2>| -> Option<Sub> {
        let parts = line_parts(k);
        if !parts.is_empty() {
            let best = match at {
                Some(p) => parts.iter().min_by(|a, b| Line::new(a.1, a.2).dist(p).total_cmp(&Line::new(b.1, b.2).dist(p))),
                None => parts.first(),
            };
            return best.map(|b| b.0);
        }
        matches!(k, EntityKind::Circle(_) | EntityKind::Arc(_) | EntityKind::Point(_)).then_some(Sub::Whole)
    };
    let sub = match (hint, want) {
        (Hint::Sub(s), _) => Some(s),
        (Hint::None, Want::Point) => char_points(k).first().map(|c| c.0),
        (Hint::None, _) => object(None),
        (Hint::Pick(p), Want::Point) => {
            char_points(k).into_iter().filter(|c| c.1.is_finite()).min_by(|a, b| a.1.dist(p).total_cmp(&b.1.dist(p))).map(|c| c.0)
        }
        (Hint::Pick(p), Want::PointOrObject) => {
            let near = char_points(k).into_iter().filter(|c| c.1.is_finite()).min_by(|a, b| a.1.dist(p).total_cmp(&b.1.dist(p)));
            let limit = (aperture * 2.0).max(object_size(k) * 0.15);
            match near {
                Some((s, q)) if q.dist(p) <= limit => Some(s),
                _ => object(Some(p)),
            }
        }
        (Hint::Pick(p), _) => object(Some(p)),
    };
    let sub = sub.ok_or_else(|| format!("cannot use this {} here", k.type_name()))?;
    // Shape checks for clearer messages.
    let ok = match want {
        Want::Line => !line_parts(k).is_empty() && matches!(sub, Sub::Whole | Sub::Segment(_)),
        Want::Curve => matches!(k, EntityKind::Circle(_) | EntityKind::Arc(_)),
        _ => true,
    };
    if !ok {
        return Err(match want {
            Want::Line => "Select a line or polyline segment".into(),
            _ => "Select an arc or circle".into(),
        });
    }
    Ok(GeomRef::new(h, sub))
}

/// The closest pair of characteristic points of two objects (COINCIDENT with no hints).
fn closest_points(d: &Drawing, a: Handle, b: Handle) -> Option<(GeomRef, GeomRef)> {
    let (ea, eb) = (d.entity(a)?, d.entity(b)?);
    let pa: Vec<_> = char_points(&ea.kind).into_iter().filter(|c| c.0 != Sub::Mid).collect();
    let pb: Vec<_> = char_points(&eb.kind).into_iter().filter(|c| c.0 != Sub::Mid).collect();
    let mut best: Option<(f64, Sub, Sub)> = None;
    for (sa, qa) in pa.iter().take(1000) {
        for (sb, qb) in pb.iter().take(1000) {
            let dd = qa.dist(*qb);
            if dd.is_finite() && best.is_none_or(|b| dd < b.0) {
                best = Some((dd, *sa, *sb));
            }
        }
    }
    best.map(|(_, sa, sb)| (GeomRef::new(a, sa), GeomRef::new(b, sb)))
}

fn wants(kind: K, n: usize) -> Vec<Want> {
    match kind {
        K::Coincident => vec![Want::PointOrObject; 2],
        K::Horizontal | K::Vertical => {
            if n >= 2 {
                vec![Want::Point; 2]
            } else {
                vec![Want::Line]
            }
        }
        K::Parallel | K::Perpendicular | K::Collinear => vec![Want::Line; 2],
        K::Concentric => vec![Want::Curve; 2],
        K::Symmetric => vec![Want::Object, Want::Object, Want::Line],
        K::Fix => vec![Want::PointOrObject],
        _ => vec![Want::Object; 2],
    }
}

fn aperture(s: &Session) -> f64 {
    s.pixel_size() * s.settings.pickbox.max(1.0) * 1.5
}

fn build_refs(s: &Session, kind: K, picks: &[(Handle, Hint)]) -> std::result::Result<Vec<GeomRef>, String> {
    let d = s.doc().map_err(|e| e.to_string())?;
    let want = wants(kind, picks.len());
    if picks.len() < want.len() {
        return Err(format!("{} needs {} object(s)", kind.name(), want.len()));
    }
    if kind == K::Coincident
        && let [(a, Hint::None), (b, Hint::None)] = picks
    {
        return closest_points(d, *a, *b).map(|(x, y)| vec![x, y]).ok_or_else(|| "Select two points".into());
    }
    let ap = aperture(s);
    let mut refs: Vec<GeomRef> = Vec::new();
    for ((h, hint), w) in picks.iter().zip(want) {
        refs.push(resolve(d, *h, *hint, w, ap)?);
    }
    Ok(refs)
}

/// Add a constraint to the active drawing; the drawing changes only on success.
fn add_constraint(s: &mut Session, c: Constraint) -> Result<u32> {
    let keep: Vec<Handle> = c.refs.first().map(|r| r.handle).into_iter().collect();
    let prefer_move: Vec<Handle> = c.refs.iter().skip(1).map(|r| r.handle).filter(|h| !keep.contains(h)).collect();
    let mut d = s.doc()?.clone();
    let id = cs::add(&mut d, c, &cs::SolveOptions { keep, prefer_move }).map_err(err)?;
    *s.doc_mut()? = d;
    Ok(id)
}

fn constraint_json(d: &Drawing, id: u32) -> Value {
    match d.constraints.iter().find(|c| c.id == id) {
        Some(c) => {
            let mut v = json!({ "id": id, "type": c.kind.name(), "refs": c.refs.iter().map(|r| json!({"handle": r.handle.hex(), "sub": r.sub.label()})).collect::<Vec<_>>() });
            if c.kind.is_dimensional() {
                v["name"] = json!(c.name);
                v["expr"] = json!(c.expr);
                v["value"] = json!(cs::constraint_value(&mut cs::env(d), c).ok());
            }
            v
        }
        None => json!({ "id": id }),
    }
}

// ---------------- geometric constraints ----------------

fn kind_from_name(t: &str) -> Option<K> {
    Some(match t.trim().trim_start_matches("gc").to_ascii_lowercase().as_str() {
        "coincident" => K::Coincident,
        "collinear" => K::Collinear,
        "concentric" => K::Concentric,
        "fix" => K::Fix,
        "parallel" => K::Parallel,
        "perpendicular" => K::Perpendicular,
        "horizontal" => K::Horizontal,
        "vertical" => K::Vertical,
        "tangent" => K::Tangent,
        "smooth" => K::Smooth,
        "symmetric" => K::Symmetric,
        "equal" => K::Equal,
        _ => return None,
    })
}

fn run_geomconstraint(s: &mut Session, p: &Value) -> Result<Value> {
    let t = str_param(p, "type").ok_or_else(|| bad("geomconstraint", "`type` is required"))?;
    let kind = kind_from_name(t).ok_or_else(|| bad("geomconstraint", format!("unknown constraint type `{t}`")))?;
    run_gc(s, p, kind)
}

fn run_gc(s: &mut Session, p: &Value, kind: K) -> Result<Value> {
    let cmd = "geomconstraint";
    let mut picks = Vec::new();
    for (hk, pk) in [("h1", "p1"), ("h2", "p2"), ("h3", "p3")] {
        let hint = hint_param(cmd, p, pk)?;
        match h_param(p, hk) {
            Some(h) => picks.push((h, hint)),
            // Two points on the same object: {h1, p1, p2}.
            None if hint != Hint::None => {
                if let Some(first) = picks.first().map(|x: &(Handle, Hint)| x.0) {
                    picks.push((first, hint));
                }
            }
            None => {}
        }
    }
    if picks.is_empty() {
        return Err(bad(cmd, "`h1` is required"));
    }
    let refs = build_refs(s, kind, &picks).map_err(|m| bad(cmd, m))?;
    let id = add_constraint(s, Constraint { id: 0, kind, refs, name: String::new(), expr: String::new() })?;
    Ok(constraint_json(s.doc()?, id))
}

struct GcM {
    kind: K,
    picks: Vec<(Handle, Vec2)>,
    two_points: bool,
}

impl GcM {
    fn new(kind: K) -> Self {
        GcM { kind, picks: Vec::new(), two_points: false }
    }
    fn needed(&self) -> usize {
        match self.kind {
            K::Horizontal | K::Vertical => {
                if self.two_points {
                    2
                } else {
                    1
                }
            }
            K::Fix => 1,
            K::Symmetric => 3,
            _ => 2,
        }
    }
    fn finish(&mut self, s: &mut Session) -> Result<Step> {
        let picks: Vec<(Handle, Hint)> = self.picks.iter().map(|(h, p)| (*h, Hint::Pick(*p))).collect();
        let r = build_refs(s, self.kind, &picks)
            .map_err(EngineError::Other)
            .and_then(|refs| add_constraint(s, Constraint { id: 0, kind: self.kind, refs, name: String::new(), expr: String::new() }));
        s.set_selection(Vec::new());
        match r {
            Ok(id) => s.echo(format!("{} constraint #{id} applied.", self.kind.name())),
            Err(e) => s.echo(e.to_string()),
        }
        Ok(Step::Done)
    }
}

impl Interactive for GcM {
    fn name(&self) -> &'static str {
        "GEOMCONSTRAINT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        let i = self.picks.len();
        let point_wise = self.kind == K::Coincident || self.two_points;
        match (self.kind, i) {
            (K::Horizontal | K::Vertical, 0) if !self.two_points => Prompt::new("Select an object", Accept::POINT).kw(&["2Points"]),
            (K::Fix, _) => Prompt::new("Select point or object", Accept::POINT),
            (K::Symmetric, 2) => Prompt::new("Select symmetry line", Accept::POINT),
            (_, 0) if point_wise => Prompt::new("Select first point", Accept::POINT),
            (_, _) if point_wise => Prompt::new("Select second point", Accept::POINT),
            (_, 0) => Prompt::new("Select first object", Accept::POINT),
            _ => Prompt::new("Select second object", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Keyword(k) if k == "2Points" => {
                self.two_points = true;
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                let space = s.space();
                let Some(h) = crate::select::pick(s.doc()?, &space, p, aperture(s)) else {
                    s.echo("*Invalid selection*");
                    return Ok(Step::Continue);
                };
                self.picks.push((h, p));
                s.set_selection(self.picks.iter().map(|x| x.0).collect());
                if self.picks.len() >= self.needed() { self.finish(s) } else { Ok(Step::Continue) }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

/// GEOMCONSTRAINT: ask for the type, then run that constraint's prompts.
#[derive(Default)]
struct GeomM {
    inner: Option<GcM>,
}

const GEOM_KW: &[&str] = &[
    "Horizontal",
    "Vertical",
    "Perpendicular",
    "PArallel",
    "Tangent",
    "SMooth",
    "Coincident",
    "CONcentric",
    "COLlinear",
    "Symmetric",
    "Equal",
    "Fix",
];

impl Interactive for GeomM {
    fn name(&self) -> &'static str {
        "GEOMCONSTRAINT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match &self.inner {
            Some(m) => m.prompt(s),
            None => Prompt::new("Enter constraint type", Accept::TEXT).kw(GEOM_KW).default("Horizontal"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(m) = &mut self.inner {
            return m.input(s, i);
        }
        let name = match i {
            Input::Keyword(k) | Input::Text(k) => k,
            Input::Enter => "Horizontal".into(),
            _ => return Ok(Step::Continue),
        };
        let kind = kind_from_name(&name).ok_or_else(|| EngineError::Other("Invalid option keyword.".into()))?;
        self.inner = Some(GcM::new(kind));
        Ok(Step::Continue)
    }
}

// ---------------- dimensional constraints ----------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dc {
    Linear,
    Aligned,
    Horizontal,
    Vertical,
    Angular,
    Radius,
    Diameter,
}

fn dc_from_name(t: &str) -> Option<Dc> {
    Some(match t.trim().trim_start_matches("dc").to_ascii_lowercase().as_str() {
        "linear" => Dc::Linear,
        "aligned" | "distance" => Dc::Aligned,
        "horizontal" => Dc::Horizontal,
        "vertical" => Dc::Vertical,
        "angular" | "angle" => Dc::Angular,
        "radius" | "radial" => Dc::Radius,
        "diameter" => Dc::Diameter,
        _ => return None,
    })
}

/// The constraint kind for a DC type; LINEAR picks horizontal or vertical from the geometry.
fn dc_kind(d: &Drawing, dc: Dc, refs: &[GeomRef]) -> K {
    match dc {
        Dc::Aligned => K::Distance(DistAxis::Aligned),
        Dc::Horizontal => K::Distance(DistAxis::Horizontal),
        Dc::Vertical => K::Distance(DistAxis::Vertical),
        Dc::Angular => K::Angular,
        Dc::Radius => K::Radius,
        Dc::Diameter => K::Diameter,
        Dc::Linear => {
            let h = cs::measure(d, K::Distance(DistAxis::Horizontal), refs).unwrap_or(0.0);
            let v = cs::measure(d, K::Distance(DistAxis::Vertical), refs).unwrap_or(0.0);
            K::Distance(if h >= v { DistAxis::Horizontal } else { DistAxis::Vertical })
        }
    }
}

fn run_dimconstraint(s: &mut Session, p: &Value) -> Result<Value> {
    let t = str_param(p, "type").unwrap_or("aligned");
    let dc = dc_from_name(t).ok_or_else(|| bad("dimconstraint", format!("unknown type `{t}`")))?;
    run_dc(s, p, dc)
}

fn run_dc(s: &mut Session, p: &Value, dc: Dc) -> Result<Value> {
    let cmd = "dimconstraint";
    let h1 = h_param(p, "h1").ok_or_else(|| bad(cmd, "`h1` is required"))?;
    let (p1, p2) = (hint_param(cmd, p, "p1")?, hint_param(cmd, p, "p2")?);
    let h2 = h_param(p, "h2");
    let d = s.doc()?;
    let ap = aperture(s);
    let r = |h, hint, w| resolve(d, h, hint, w, ap).map_err(|m| bad(cmd, m));
    let refs = match dc {
        Dc::Linear | Dc::Aligned | Dc::Horizontal | Dc::Vertical => {
            if h2.is_some() || p2 != Hint::None {
                // `p2: "object"` on a line: point-to-line distance (aligned only).
                let second_whole = matches!(p2, Hint::Sub(Sub::Whole | Sub::Segment(_)));
                let w2 = if second_whole && dc == Dc::Aligned { Want::Line } else { Want::Point };
                // Two points of one object default to its start and end.
                let p2 = if p2 == Hint::None && h2.is_none_or(|h| h == h1) { Hint::Sub(Sub::End) } else { p2 };
                vec![r(h1, p1, Want::Point)?, r(h2.unwrap_or(h1), p2, w2)?]
            } else {
                vec![r(h1, p1, Want::Line)?]
            }
        }
        Dc::Angular => match h2 {
            Some(h2) => vec![r(h1, p1, Want::Line)?, r(h2, p2, Want::Line)?],
            None => vec![r(h1, p1, Want::Curve)?],
        },
        Dc::Radius | Dc::Diameter => vec![r(h1, p1, Want::Curve)?],
    };
    let kind = dc_kind(d, dc, &refs);
    let expr = match (str_param(p, "expr"), p.get("value").and_then(Value::as_f64)) {
        (Some(e), _) => e.trim().to_string(),
        (None, Some(v)) if v.is_finite() => cs::format_value(v),
        _ => String::new(),
    };
    let name = str_param(p, "name").unwrap_or("").trim().to_string();
    let id = add_constraint(s, Constraint { id: 0, kind, refs, name, expr })?;
    Ok(constraint_json(s.doc()?, id))
}

struct DcM {
    dc: Option<Dc>,
    refs: Vec<GeomRef>,
    points: bool,
    /// Picking done: waiting for the value.
    value: Option<(K, f64)>,
}

impl DcM {
    fn new(dc: Option<Dc>) -> Self {
        DcM { dc, refs: Vec::new(), points: false, value: None }
    }
    fn picked(&mut self, s: &mut Session) -> Result<Step> {
        let d = s.doc()?;
        let Some(dc) = self.dc else { return Ok(Step::Continue) };
        let kind = dc_kind(d, dc, &self.refs);
        match cs::measure(d, kind, &self.refs) {
            Ok(v) => {
                self.value = Some((kind, v));
                Ok(Step::Continue)
            }
            Err(m) => {
                self.refs.clear();
                Err(EngineError::Other(m))
            }
        }
    }
    fn commit(&mut self, s: &mut Session, text: Option<&str>) -> Result<Step> {
        let Some((kind, v)) = self.value else { return Ok(Step::Continue) };
        let (name, expr) = match text.map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) => match t.split_once('=') {
                Some((n, e)) => (n.trim().to_string(), e.trim().to_string()),
                None => (String::new(), t.to_string()),
            },
            None => (String::new(), cs::format_value(v)),
        };
        let c = Constraint { id: 0, kind, refs: std::mem::take(&mut self.refs), name, expr };
        match add_constraint(s, c) {
            Ok(id) => {
                let d = s.doc()?;
                if let Some(c) = d.constraints.iter().find(|c| c.id == id) {
                    let msg = format!("{} = {}", c.name, c.expr);
                    s.echo(msg);
                }
            }
            Err(e) => s.echo(e.to_string()),
        }
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
}

const DC_KW: &[&str] = &["LInear", "Horizontal", "Vertical", "ALigned", "ANgular", "Radius", "Diameter"];

impl Interactive for DcM {
    fn name(&self) -> &'static str {
        "DIMCONSTRAINT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        if let Some((_, v)) = self.value {
            return Prompt::new("Enter dimension value or [name=expression]", Accept::TEXT).default(cs::format_value(v));
        }
        let _ = s;
        match self.dc {
            None => Prompt::new("Select a dimension constraint type", Accept::TEXT).kw(DC_KW).default("ALigned"),
            Some(Dc::Radius | Dc::Diameter) => Prompt::new("Select arc or circle", Accept::POINT),
            Some(Dc::Angular) if self.refs.is_empty() => Prompt::new("Select first line or arc", Accept::POINT),
            Some(Dc::Angular) => Prompt::new("Select second line", Accept::POINT),
            Some(_) if self.points && self.refs.is_empty() => Prompt::new("Specify first constraint point", Accept::POINT),
            Some(_) if self.points => Prompt::new("Specify second constraint point", Accept::POINT),
            Some(_) => Prompt::new("Select object", Accept::POINT).kw(&["Points"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.value.is_some() {
            return match i {
                Input::Text(t) | Input::Keyword(t) => self.commit(s, Some(&t)),
                Input::Enter => self.commit(s, None),
                _ => Ok(Step::Continue),
            };
        }
        let Some(dc) = self.dc else {
            let name = match i {
                Input::Keyword(k) | Input::Text(k) => k,
                Input::Enter => "aligned".into(),
                _ => return Ok(Step::Continue),
            };
            self.dc = Some(dc_from_name(&name).ok_or_else(|| EngineError::Other("Invalid option keyword.".into()))?);
            return Ok(Step::Continue);
        };
        match i {
            Input::Keyword(k) if k == "Points" => {
                self.points = true;
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                let space = s.space();
                let ap = aperture(s);
                let d = s.doc()?;
                let Some(h) = crate::select::pick(d, &space, p, ap) else {
                    s.echo("*Invalid selection*");
                    return Ok(Step::Continue);
                };
                let want = match dc {
                    Dc::Radius | Dc::Diameter => Want::Curve,
                    Dc::Angular if self.refs.is_empty() => {
                        if matches!(d.entity(h).map(|e| &e.kind), Some(EntityKind::Arc(_))) {
                            Want::Curve
                        } else {
                            Want::Line
                        }
                    }
                    Dc::Angular => Want::Line,
                    _ if self.points => Want::Point,
                    _ => Want::Line,
                };
                let r = resolve(d, h, Hint::Pick(p), want, ap).map_err(EngineError::Other)?;
                self.refs.push(r);
                let done = match dc {
                    Dc::Radius | Dc::Diameter => true,
                    Dc::Angular => want == Want::Curve || self.refs.len() >= 2,
                    _ => !self.points || self.refs.len() >= 2,
                };
                if done { self.picked(s) } else { Ok(Step::Continue) }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

// ---------------- selection-based commands ----------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelOp {
    Auto,
    Delete,
    Bars,
    Dims,
}

struct SelM {
    op: SelOp,
    phase: SelectPhase,
}

impl SelM {
    fn new(op: SelOp) -> Self {
        SelM { op, phase: SelectPhase::default() }
    }
    fn apply(&self, s: &mut Session, hs: Vec<Handle>) -> Result<Step> {
        let handles = json!(hs.iter().map(|h| h.hex()).collect::<Vec<_>>());
        let r = match self.op {
            SelOp::Auto => run_autoconstrain(s, &json!({ "handles": handles })),
            SelOp::Delete => run_delconstraint(s, &json!({ "handles": handles })),
            SelOp::Bars => set_bars(s, false, "show", &hs),
            SelOp::Dims => set_bars(s, true, "show", &hs),
        };
        match r {
            Ok(v) => {
                if let Some(m) = v.get("message").and_then(Value::as_str) {
                    s.echo(m.to_string());
                }
            }
            Err(e) => s.echo(e.to_string()),
        }
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
}

impl Interactive for SelM {
    fn name(&self) -> &'static str {
        match self.op {
            SelOp::Auto => "AUTOCONSTRAIN",
            SelOp::Delete => "DELCONSTRAINT",
            SelOp::Bars => "CONSTRAINTBAR",
            SelOp::Dims => "DCDISPLAY",
        }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.phase = SelectPhase::begin(s);
        if self.phase.done {
            let hs = self.phase.picked.clone();
            return self.apply(s, hs);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        self.phase.prompt()
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.phase.feed(s, &i)? {
            SelOutcome::More => Ok(Step::Continue),
            SelOutcome::Empty => Ok(Step::Done),
            SelOutcome::Done(hs) => self.apply(s, hs),
        }
    }
}

fn run_autoconstrain(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    if hs.is_empty() {
        return Err(bad("autoconstrain", "select objects (or pass `handles`)"));
    }
    let set = &s.doc()?.parametric.settings;
    let mut o = cs::InferOptions { distance_tolerance: set.distance_tolerance, angle_tolerance: set.angle_tolerance, types: set.auto_types.clone() };
    o.distance_tolerance = f64_or(p, "tolerance", o.distance_tolerance);
    o.angle_tolerance = f64_or(p, "angleTolerance", o.angle_tolerance);
    if let Some(t) = p.get("types").and_then(Value::as_array) {
        o.types = t.iter().filter_map(Value::as_str).map(str::to_string).collect();
    }
    let mut d = s.doc()?.clone();
    let ids = cs::autoconstrain(&mut d, &hs, &o);
    if !ids.is_empty() {
        *s.doc_mut()? = d;
    }
    Ok(json!({ "added": ids.len(), "ids": ids, "message": format!("{} constraint(s) applied to {} object(s)", ids.len(), hs.len()) }))
}

fn run_delconstraint(s: &mut Session, p: &Value) -> Result<Value> {
    let ids: Option<Vec<u32>> =
        p.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).filter_map(|v| u32::try_from(v).ok()).collect());
    let hs = targets(s, p)?;
    if ids.is_none() && hs.is_empty() {
        return Err(bad("delconstraint", "select objects (or pass `handles` / `ids`)"));
    }
    let doomed = |c: &Constraint| match &ids {
        Some(ids) => ids.contains(&c.id),
        None => c.refs.iter().any(|r| hs.contains(&r.handle)),
    };
    let n = s.doc()?.constraints.iter().filter(|c| doomed(c)).count();
    if n > 0 {
        let mut d = s.doc()?.clone();
        let broken_before: Vec<String> = cs::parameter_table(&d).into_iter().filter(|r| r.error.is_some()).map(|r| r.name).collect();
        let removed: Vec<String> = d.constraints.iter().filter(|c| doomed(c)).map(|c| c.name.clone()).collect();
        d.constraints.retain(|c| !doomed(c));
        // Reject when a surviving expression would lose a dimension it refers to (same guard as `parameters delete`).
        if let Some(row) = cs::parameter_table(&d).into_iter().find(|r| r.error.is_some() && !broken_before.contains(&r.name)) {
            let mut refs = Vec::new();
            if let Ok(e) = cs::expr::parse(&row.expr) {
                cs::expr::names(&e, &mut refs);
            }
            let used = removed.iter().find(|n| refs.contains(*n)).or(removed.first()).cloned().unwrap_or_default();
            return Err(EngineError::Other(format!("`{used}` is used by `{}`", row.name)));
        }
        *s.doc_mut()? = d;
    }
    Ok(json!({ "removed": n, "message": format!("{n} constraint(s) removed") }))
}

fn run_parameters(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(name) = str_param(p, "delete") {
        let d = s.doc()?;
        if d.constraints.iter().any(|c| c.name == name) {
            return Err(EngineError::Other(format!("`{name}` belongs to a dimensional constraint; delete the constraint instead")));
        }
        if !d.parametric.parameters.iter().any(|q| q.name == name) {
            return Err(EngineError::Other(format!("no parameter `{name}`")));
        }
        let mut d = d.clone();
        d.parametric.parameters.retain(|q| q.name != name);
        if let Some(row) = cs::parameter_table(&d).into_iter().find(|r| r.error.is_some()) {
            return Err(EngineError::Other(format!("`{name}` is used by `{}`", row.name)));
        }
        *s.doc_mut()? = d;
    } else if let Some(name) = str_param(p, "name") {
        let expr = match (str_param(p, "expr"), p.get("value").and_then(Value::as_f64)) {
            (Some(e), _) => e.to_string(),
            (None, Some(v)) if v.is_finite() => cs::format_value(v),
            _ => return Err(bad("parameters", "`expr` or `value` is required")),
        };
        let mut d = s.doc()?.clone();
        cs::set_parameter(&mut d, name.trim(), &expr, &cs::SolveOptions::default()).map_err(err)?;
        if let Some(desc) = str_param(p, "description")
            && let Some(q) = d.parametric.parameters.iter_mut().find(|q| q.name == name.trim())
        {
            q.description = desc.to_string();
        }
        *s.doc_mut()? = d;
    }
    let rows = cs::parameter_table(s.doc()?);
    let text: Vec<String> =
        rows.iter().map(|r| format!("{} = {} ({})", r.name, r.expr, r.value.map(cs::format_value).unwrap_or_else(|| "error".into()))).collect();
    let message = if text.is_empty() { "No parameters.".to_string() } else { text.join("; ") };
    Ok(json!({ "parameters": rows, "message": message }))
}

/// Show/hide constraint bars (`dims` = false) or dynamic dimensional constraints.
fn set_bars(s: &mut Session, dims: bool, mode: &str, hs: &[Handle]) -> Result<Value> {
    let d = s.doc_mut()?;
    let set = &mut d.parametric.settings;
    let (visible, exceptions) =
        if dims { (&mut set.dims_visible, &mut set.dim_exceptions) } else { (&mut set.bars_visible, &mut set.bar_exceptions) };
    match mode {
        "showall" => {
            *visible = true;
            exceptions.clear();
        }
        "hideall" => {
            *visible = false;
            exceptions.clear();
        }
        "show" | "hide" => {
            // Exceptions are hidden objects when visible, shown objects when hidden.
            let add = (mode == "hide") == *visible;
            for h in hs {
                exceptions.retain(|x| x != h);
                if add {
                    exceptions.push(*h);
                }
            }
            exceptions.truncate(100_000);
        }
        other => return Err(bad(if dims { "dcdisplay" } else { "constraintbar" }, format!("unknown mode `{other}`"))),
    }
    let what = if dims { "Dynamic constraints" } else { "Constraint bars" };
    Ok(json!({ "visible": *visible, "exceptions": exceptions.iter().map(|h| h.hex()).collect::<Vec<_>>(), "message": format!("{what}: {mode}") }))
}

fn bar_cmd(s: &mut Session, p: &Value, dims: bool) -> Result<Value> {
    let mode = str_param(p, "mode").unwrap_or("show").to_ascii_lowercase();
    let hs = targets(s, p)?;
    if matches!(mode.as_str(), "show" | "hide") && hs.is_empty() {
        return Err(bad(if dims { "dcdisplay" } else { "constraintbar" }, "select objects (or pass `handles`)"));
    }
    set_bars(s, dims, &mode, &hs)
}

fn run_constraintbar(s: &mut Session, p: &Value) -> Result<Value> {
    bar_cmd(s, p, false)
}

fn run_dcdisplay(s: &mut Session, p: &Value) -> Result<Value> {
    bar_cmd(s, p, true)
}

fn run_constraintsettings(s: &mut Session, p: &Value) -> Result<Value> {
    let cur = s.doc()?.parametric.settings.clone();
    let mut set = cur.clone();
    set.infer = bool_or(p, "infer", set.infer);
    set.distance_tolerance = f64_or(p, "distanceTolerance", set.distance_tolerance).clamp(0.0, 1e6);
    set.angle_tolerance = f64_or(p, "angleTolerance", set.angle_tolerance).clamp(0.0, 45.0);
    set.bars_visible = bool_or(p, "barsVisible", set.bars_visible);
    set.dims_visible = bool_or(p, "dimsVisible", set.dims_visible);
    if let Some(t) = p.get("barTransparency").and_then(Value::as_f64).filter(|v| v.is_finite()) {
        set.bar_transparency = t.clamp(0.0, 90.0) as u8;
    }
    if let Some(t) = p.get("autoTypes").and_then(Value::as_array) {
        set.auto_types = t.iter().filter_map(Value::as_str).filter(|n| kind_from_name(n).is_some()).map(str::to_string).take(32).collect();
    }
    if set != cur {
        s.doc_mut()?.parametric.settings = set.clone();
    }
    serde_json::to_value(&set).map_err(|e| EngineError::Other(e.to_string()))
}

fn run_inspect(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    Ok(json!({
        "constraints": d.constraints,
        "glyphs": cs::glyphs(d),
        "parameters": cs::parameter_table(d),
        "settings": d.parametric.settings,
        "satisfied": cs::all_satisfied(d),
    }))
}

// ---------------- re-solve after edits ----------------

/// Called after every command (programmatic or interactive). When the command changed the
/// drawing and its constraints no longer hold, re-solve them, keeping the geometry the command
/// edited where it was put; constraints whose objects are gone are removed.
pub fn after_command(s: &mut Session, before: Option<&Arc<Drawing>>, ok: bool) {
    if !ok {
        return;
    }
    let (Some(before), Ok(st)) = (before, s.state()) else { return };
    if st.doc.constraints.is_empty() || Arc::ptr_eq(before, &st.doc) {
        return;
    }
    let mut d = (*st.doc).clone();
    let purged = cs::purge_invalid(&mut d);
    let satisfied = cs::all_satisfied(&d);
    if purged.is_empty() && satisfied {
        return;
    }
    let mut msg = Vec::new();
    if !purged.is_empty() {
        msg.push(format!("{} constraint(s) removed with their objects.", purged.len()));
    }
    if !satisfied {
        let mut keep: Vec<Handle> = Vec::new();
        for h in d.constraints.iter().flat_map(|c| c.refs.iter().map(|r| r.handle)) {
            let changed = match (before.entity(h), d.entity(h)) {
                (Some(a), Some(b)) => !Arc::ptr_eq(a, b) && a != b,
                _ => false,
            };
            if changed && !keep.contains(&h) {
                keep.push(h);
            }
        }
        let mut trial = d.clone();
        match cs::solve(&mut trial, &cs::SolveOptions { keep, prefer_move: Vec::new() }) {
            Ok(_) => d = trial,
            Err(e) => msg.push(format!("Constraints could not be satisfied: {e}")),
        }
    }
    if let Ok(st) = s.state_mut() {
        st.doc = Arc::new(d);
    }
    for m in msg {
        s.echo(m);
    }
}

#[cfg(test)]
#[path = "constraints_tests.rs"]
mod tests;
