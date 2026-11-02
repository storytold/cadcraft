//! TRIM and EXTEND: the prompt sequence and the JSON forms. Quick and Standard modes
//! (TRIMEXTENDMODE), cutting/boundary edges, Fence, Crossing, Project (PROJMODE), Edge (EDGEMODE),
//! eRase and Undo. The geometry of one trim or extend is in modify.rs.

use std::sync::Arc;

use cadcraft_doc::{Drawing, EntityKind, Handle};
use cadcraft_geom::{Bounds2, Line, Vec2, line_line};
use serde_json::{Value, json};

use super::helpers::{line, lwpoly, rect_vertices};
use super::modify::{Edges, extend, trim};
use super::{bad, point_param, points_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// Most fence points the JSON form takes.
const MAX_FENCE: usize = 10_000;

/// TRIMEXTENDMODE: 1 (default) Quick, 0 Standard.
fn quick_mode(s: &Session) -> bool {
    s.doc().map(|d| d.header.i64("TRIMEXTENDMODE", 1) != 0).unwrap_or(true)
}
/// EDGEMODE: 1 extends cutting and boundary edges virtually (Standard mode).
fn edge_mode(s: &Session) -> bool {
    s.doc().map(|d| d.header.i64("EDGEMODE", 0) != 0).unwrap_or(false)
}
/// PROJMODE: 0 None, 1 UCS (default), 2 View. Drawings are 2D (every object lies in the XY
/// plane), so the three give the same result; the setting is kept for scripts and the DWG.
fn proj_mode(s: &Session) -> i64 {
    s.doc().map(|d| d.header.i64("PROJMODE", 1)).unwrap_or(1)
}

/// Whether a trim error says the object meets no cutting edge (a Quick-mode trim then erases it).
fn misses_edges(e: &EngineError) -> bool {
    e.to_string().contains("intersect")
}

/// Trim or extend `h` at `at`. A Quick-mode (`quick`) trim of an object no cutting edge crosses
/// erases it. Returns the handles that replace `h` (none when it was erased).
fn apply(s: &mut Session, h: Handle, at: Vec2, extending: bool, edges: Edges, quick: bool) -> Result<Vec<Handle>> {
    if extending {
        extend(s, h, at, edges)?;
        return Ok(vec![h]);
    }
    match trim(s, h, at, edges) {
        Err(e) if quick && misses_edges(&e) => {
            s.doc_mut()?.remove_entity(h);
            Ok(Vec::new())
        }
        r => r,
    }
}

/// Trim (or extend) every object the fence `pts` crosses, at each crossing (extend: at the first).
/// Returns how many trims or extends were made.
fn fence(s: &mut Session, pts: &[Vec2], extending: bool, edges: Edges, quick: bool) -> Result<usize> {
    let space = s.space();
    let d = s.doc()?;
    let fb = Bounds2::from_points(pts.iter().copied());
    let tol = crate::select::window_tol(&fb);
    let mut work: Vec<(Handle, Vec<Vec2>)> = Vec::new();
    for h in crate::select::select_fence(d, &space, pts) {
        let Some(e) = d.entity(h) else { continue };
        let polys = crate::select::hit_polylines(d, e, tol);
        let mut xs: Vec<Vec2> = Vec::new();
        for f in pts.windows(2) {
            let (Some(a), Some(b)) = (f.first(), f.get(1)) else { continue };
            let fl = Line::new(*a, *b);
            // Crossings along this fence segment, in fence order.
            let mut here: Vec<Vec2> = polys
                .iter()
                .flat_map(|pl| pl.windows(2).filter_map(|w| line_line(&fl, &Line::new(*w.first()?, *w.get(1)?))))
                .filter(|x| !xs.iter().any(|y| y.near(*x, tol)))
                .collect();
            here.sort_by(|x, y| x.dist(*a).total_cmp(&y.dist(*a)));
            here.dedup_by(|x, y| x.near(*y, tol));
            xs.extend(here);
        }
        work.push((h, xs));
    }
    run_picks(s, work, extending, edges, quick, tol)
}

/// Trim (or extend) every object a crossing window from `a` to `b` touches, picked at its point
/// in the window nearest the first corner `a`.
fn crossing(s: &mut Session, a: Vec2, b: Vec2, extending: bool, edges: Edges, quick: bool) -> Result<usize> {
    let space = s.space();
    let d = s.doc()?;
    let bx = Bounds2::new(a, b);
    let tol = crate::select::window_tol(&bx);
    let sides = [
        Line::new(bx.min, Vec2::new(bx.max.x, bx.min.y)),
        Line::new(Vec2::new(bx.max.x, bx.min.y), bx.max),
        Line::new(bx.max, Vec2::new(bx.min.x, bx.max.y)),
        Line::new(Vec2::new(bx.min.x, bx.max.y), bx.min),
    ];
    let inside = |p: Vec2| bx.expand(tol).contains(p);
    let mut work: Vec<(Handle, Vec<Vec2>)> = Vec::new();
    for h in crate::select::select_window(d, &space, bx, true) {
        let Some(e) = d.entity(h) else { continue };
        let mut best: Option<Vec2> = None;
        for pl in crate::select::hit_polylines(d, e, tol) {
            for w in pl.windows(2) {
                let (Some(p), Some(q)) = (w.first(), w.get(1)) else { continue };
                let seg = Line::new(*p, *q);
                let cands = [*p, *q, seg.closest(a)].into_iter().chain(sides.iter().filter_map(|sd| line_line(sd, &seg)));
                for c in cands.filter(|c| inside(*c)) {
                    if best.is_none_or(|b| c.dist(a) < b.dist(a)) {
                        best = Some(c);
                    }
                }
            }
        }
        if let Some(p) = best {
            work.push((h, vec![p]));
        }
    }
    run_picks(s, work, extending, edges, quick, tol)
}

/// Trim (or extend) each object at its pick points. A trim replaces the object with pieces, so
/// later picks go to the piece within `tol` of the point (none: that part is already gone).
fn run_picks(s: &mut Session, work: Vec<(Handle, Vec<Vec2>)>, extending: bool, edges: Edges, quick: bool, tol: f64) -> Result<usize> {
    let mut n = 0;
    let mut last_err = None;
    for (h, picks) in work {
        let mut live = vec![h];
        for x in picks {
            let target = {
                let d = s.doc()?;
                live.iter()
                    .filter_map(|l| d.entity(*l).map(|e| (*l, crate::select::entity_distance(d, e, x, tol))))
                    .filter(|(_, dist)| *dist <= 3.0 * tol)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(l, _)| l)
            };
            let Some(t) = target else { continue };
            match apply(s, t, x, extending, edges, quick) {
                Ok(new) => {
                    live.retain(|l| *l != t);
                    live.extend(new);
                    n += 1;
                }
                Err(e) => last_err = Some(e),
            }
            if extending {
                break;
            }
        }
    }
    match (n, last_err) {
        (0, Some(e)) => Err(e),
        (0, None) => Err(EngineError::Other("No objects found.".into())),
        _ => Ok(n),
    }
}

/// The JSON forms' `edgeMode`: "extend" / "none" (or true / false).
fn edge_mode_param(cmd: &str, p: &Value) -> Result<bool> {
    match p.get("edgeMode") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(Value::String(t)) => match t.trim().to_ascii_lowercase().replace([' ', '_', '-'], "").as_str() {
            "extend" => Ok(true),
            "none" | "noextend" => Ok(false),
            _ => Err(bad(cmd, "`edgeMode` must be \"extend\" or \"none\"")),
        },
        Some(_) => Err(bad(cmd, "`edgeMode` must be \"extend\" or \"none\"")),
    }
}

fn run(s: &mut Session, p: &Value, extending: bool) -> Result<Value> {
    let cmd = if extending { "extend" } else { "trim" };
    let only: Option<Vec<Handle>> =
        p.get("edges").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().and_then(Handle::parse_hex)).collect());
    let edges = Edges { only: only.as_deref(), implied: edge_mode_param(cmd, p)? };
    let quick = match p.get("mode") {
        None | Some(Value::Null) => false,
        Some(v) => match v.as_str().map(|m| m.trim().to_ascii_lowercase()).as_deref() {
            Some("quick") => true,
            Some("standard") => false,
            _ => return Err(bad(cmd, "`mode` must be \"quick\" or \"standard\"")),
        },
    };
    let key = if extending { "extended" } else { "trimmed" };
    if p.get("fence").is_some_and(|v| !v.is_null()) {
        let pts = points_param(p, "fence")
            .filter(|v| (2..=MAX_FENCE).contains(&v.len()))
            .ok_or_else(|| bad(cmd, "`fence` needs two or more points [[x,y], ...]"))?;
        let n = fence(s, &pts, extending, edges, quick)?;
        return Ok(json!({ key: n }));
    }
    if p.get("crossing").is_some_and(|v| !v.is_null()) {
        let c = points_param(p, "crossing").filter(|v| v.len() == 2).ok_or_else(|| bad(cmd, "`crossing` needs two corners [[x,y], [x,y]]"))?;
        let (Some(a), Some(b)) = (c.first(), c.get(1)) else { return Err(bad(cmd, "`crossing` needs two corners")) };
        let n = crossing(s, *a, *b, extending, edges, quick)?;
        return Ok(json!({ key: n }));
    }
    let h = super::targets(s, p)?.first().copied().ok_or_else(|| bad(cmd, "`handle` is required"))?;
    let pick = point_param(p, "pick").ok_or_else(|| bad(cmd, "`pick` ([x, y]) is required"))?;
    let r = apply(s, h, pick, extending, edges, quick)?;
    if extending { Ok(Value::Null) } else { Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() })) }
}

pub(crate) fn run_trim(s: &mut Session, p: &Value) -> Result<Value> {
    run(s, p, false)
}

pub(crate) fn run_extend(s: &mut Session, p: &Value) -> Result<Value> {
    run(s, p, true)
}

// ---------------- the prompt sequence ----------------

#[derive(Clone, PartialEq)]
enum Stage {
    /// "Select object to trim or shift-select to extend".
    Pick,
    /// "Select objects or <select all>": the cutting/boundary edges picked so far.
    Edges(Vec<Handle>),
    /// Fence points so far.
    Fence(Vec<Vec2>),
    /// A crossing window: its first corner once picked.
    Crossing(Option<Vec2>),
    Mode,
    Project,
    Edge,
    /// "Select objects to erase or <exit>": the objects picked so far.
    Erase(Vec<Handle>),
}

pub(crate) struct TrimM {
    extend: bool,
    /// Cutting/boundary edges; `None`: all objects.
    edges: Option<Vec<Handle>>,
    stage: Stage,
    /// The drawing before each trim, extend or erase, for Undo.
    undo: Vec<Arc<Drawing>>,
}

impl TrimM {
    pub(crate) fn new(extend: bool) -> Self {
        TrimM { extend, edges: None, stage: Stage::Pick, undo: Vec::new() }
    }
    fn edges(&self, quick: bool, implied: bool) -> Edges<'_> {
        Edges { only: self.edges.as_deref(), implied: !quick && implied }
    }
    fn select_edges(&mut self, s: &mut Session) {
        s.echo(if self.extend { "Select boundary edges ..." } else { "Select cutting edges ..." });
        s.set_selection(Vec::new());
        self.stage = Stage::Edges(Vec::new());
    }
    /// Run `f` as one undoable step of the command; a step that changed nothing records none.
    fn step(&mut self, s: &mut Session, f: impl FnOnce(&mut Self, &mut Session) -> Result<()>) -> Result<()> {
        let before = s.state()?.doc.clone();
        let r = f(self, s);
        if !Arc::ptr_eq(&before, &s.state()?.doc) {
            self.undo.push(before);
        }
        r
    }
    fn option(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let word = match &i {
            Input::Keyword(k) | Input::Text(k) => k.trim().to_ascii_lowercase(),
            Input::Enter => String::new(),
            _ => return Ok(Step::Continue),
        };
        let stage = self.stage.clone();
        let invalid = || EngineError::Other("Invalid option keyword.".into());
        match stage {
            Stage::Mode => {
                let quick = match word.as_str() {
                    "" => quick_mode(s),
                    w if w.starts_with('q') => true,
                    w if w.starts_with('s') => false,
                    _ => return Err(invalid()),
                };
                s.doc_mut()?.header.set_i64("TRIMEXTENDMODE", i64::from(quick));
                if quick {
                    self.edges = None;
                    self.stage = Stage::Pick;
                } else {
                    self.select_edges(s);
                }
            }
            Stage::Project => {
                let v = match word.as_str() {
                    "" => proj_mode(s),
                    w if w.starts_with('n') => 0,
                    w if w.starts_with('u') => 1,
                    w if w.starts_with('v') => 2,
                    _ => return Err(invalid()),
                };
                s.doc_mut()?.header.set_i64("PROJMODE", v);
                self.stage = Stage::Pick;
            }
            Stage::Edge => {
                let v = match word.as_str() {
                    "" => edge_mode(s),
                    w if w.starts_with('e') => true,
                    w if w.starts_with('n') => false,
                    _ => return Err(invalid()),
                };
                s.doc_mut()?.header.set_i64("EDGEMODE", i64::from(v));
                self.stage = Stage::Pick;
            }
            _ => {}
        }
        Ok(Step::Continue)
    }
}

impl Interactive for TrimM {
    fn name(&self) -> &'static str {
        if self.extend { "EXTEND" } else { "TRIM" }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        let quick = quick_mode(s);
        let proj = match proj_mode(s) {
            0 => "None",
            2 => "View",
            _ => "UCS",
        };
        s.echo(format!(
            "Current settings: Projection={proj}, Edge={}, Mode={}",
            if edge_mode(s) { "Extend" } else { "None" },
            if quick { "Quick" } else { "Standard" }
        ));
        if !quick {
            self.select_edges(s);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match &self.stage {
            Stage::Pick => {
                let (msg, first) = if self.extend {
                    ("Select object to extend or shift-select to trim", "Boundary edges")
                } else {
                    ("Select object to trim or shift-select to extend", "cuTting edges")
                };
                let mut kw = vec![first, "Fence", "Crossing", "mOde", "Project"];
                if !quick_mode(s) {
                    kw.push("Edge");
                }
                if !self.extend {
                    kw.push("eRase");
                }
                kw.push("Undo");
                Prompt::new(msg, Accept::POINT).kw(&kw)
            }
            Stage::Edges(_) => Prompt::new("Select objects or", Accept::SELECT).default("select all"),
            Stage::Erase(_) => Prompt::new("Select objects to erase or", Accept::SELECT).default("exit"),
            Stage::Fence(pts) => match pts.last() {
                None => Prompt::new("Specify first fence point", Accept::POINT),
                Some(l) => Prompt::new("Specify next fence point", Accept::POINT).kw(&["Undo"]).base(*l),
            },
            Stage::Crossing(None) => Prompt::new("Specify first corner", Accept::POINT),
            Stage::Crossing(Some(_)) => Prompt::new("Specify opposite corner", Accept::POINT),
            Stage::Mode => {
                let msg = if self.extend { "Enter an extend mode option" } else { "Enter a trim mode option" };
                Prompt::new(msg, Accept::TEXT).kw(&["Quick", "Standard"]).default(if quick_mode(s) { "Quick" } else { "Standard" })
            }
            Stage::Project => {
                let cur = match proj_mode(s) {
                    0 => "None",
                    2 => "View",
                    _ => "Ucs",
                };
                Prompt::new("Enter a projection option", Accept::TEXT).kw(&["None", "Ucs", "View"]).default(cur)
            }
            Stage::Edge => Prompt::new("Enter an implied edge extension mode", Accept::TEXT).kw(&["Extend", "No extend"]).default(if edge_mode(s) {
                "Extend"
            } else {
                "No extend"
            }),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let quick = quick_mode(s);
        let implied = edge_mode(s);
        let extending = self.extend;
        match self.stage.clone() {
            Stage::Mode | Stage::Project | Stage::Edge => self.option(s, i),
            Stage::Edges(mut picked) => {
                match i {
                    Input::Pick(hs) => {
                        for h in hs {
                            if !picked.contains(&h) {
                                picked.push(h);
                            }
                        }
                        s.set_selection(picked.clone());
                        self.stage = Stage::Edges(picked);
                    }
                    Input::Enter => {
                        s.set_selection(Vec::new());
                        self.edges = if picked.is_empty() { None } else { Some(picked) };
                        self.stage = Stage::Pick;
                    }
                    _ => {}
                }
                Ok(Step::Continue)
            }
            Stage::Erase(mut picked) => {
                match i {
                    Input::Pick(hs) => {
                        for h in hs {
                            if !picked.contains(&h) {
                                picked.push(h);
                            }
                        }
                        s.set_selection(picked.clone());
                        self.stage = Stage::Erase(picked);
                    }
                    Input::Enter => {
                        s.set_selection(Vec::new());
                        self.step(s, |_, s| {
                            let d = s.doc_mut()?;
                            for h in &picked {
                                let locked = d.entity(*h).and_then(|e| d.layer(&e.common.layer)).is_some_and(|l| l.locked);
                                if !locked {
                                    d.remove_entity(*h);
                                }
                            }
                            Ok(())
                        })?;
                        self.stage = Stage::Pick;
                    }
                    _ => {}
                }
                Ok(Step::Continue)
            }
            Stage::Fence(mut pts) => {
                match i {
                    Input::Point(p) => {
                        pts.push(p);
                        self.stage = Stage::Fence(pts);
                    }
                    Input::Keyword(k) if k == "Undo" => {
                        pts.pop();
                        self.stage = Stage::Fence(pts);
                    }
                    Input::Enter => {
                        self.stage = Stage::Pick;
                        if pts.len() >= 2 {
                            let r = self.step(s, |m, s| fence(s, &pts, extending, m.edges(quick, implied), quick).map(|_| ()));
                            if let Err(e) = r {
                                s.echo(e.to_string());
                            }
                        }
                    }
                    _ => {}
                }
                Ok(Step::Continue)
            }
            Stage::Crossing(first) => {
                match (first, i) {
                    (None, Input::Point(p)) => self.stage = Stage::Crossing(Some(p)),
                    (Some(a), Input::Point(b)) => {
                        self.stage = Stage::Pick;
                        let r = self.step(s, |m, s| crossing(s, a, b, extending, m.edges(quick, implied), quick).map(|_| ()));
                        if let Err(e) = r {
                            s.echo(e.to_string());
                        }
                    }
                    (_, Input::Enter) => self.stage = Stage::Pick,
                    _ => {}
                }
                Ok(Step::Continue)
            }
            Stage::Pick => match i {
                Input::Point(p) => {
                    let ap = s.pixel_size() * s.settings.pickbox.max(1.0) * 1.5;
                    let space = s.space();
                    let Some(h) = s.pick_object(&space, p, ap)? else {
                        // A pick in empty space opens a crossing window there.
                        self.stage = Stage::Crossing(Some(p));
                        return Ok(Step::Continue);
                    };
                    let r = self.step(s, |m, s| apply(s, h, p, extending, m.edges(quick, implied), quick).map(|_| ()));
                    if let Err(e) = r {
                        s.echo(e.to_string());
                    }
                    Ok(Step::Continue)
                }
                Input::Enter => Ok(Step::Done),
                Input::Keyword(k) => {
                    match k.as_str() {
                        "cuTting edges" | "Boundary edges" => self.select_edges(s),
                        "Fence" => self.stage = Stage::Fence(Vec::new()),
                        "Crossing" => self.stage = Stage::Crossing(None),
                        "mOde" => self.stage = Stage::Mode,
                        "Project" => self.stage = Stage::Project,
                        "Edge" => self.stage = Stage::Edge,
                        "eRase" => {
                            s.set_selection(Vec::new());
                            self.stage = Stage::Erase(Vec::new());
                        }
                        "Undo" => match self.undo.pop() {
                            Some(doc) => {
                                // Settings changed meanwhile stay.
                                let header = s.doc()?.header.clone();
                                s.state_mut()?.doc = doc;
                                s.doc_mut()?.header = header;
                            }
                            None => s.echo("Nothing to undo."),
                        },
                        _ => {}
                    }
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        match &self.stage {
            Stage::Fence(pts) if !pts.is_empty() => pts
                .iter()
                .copied()
                .chain(std::iter::once(c))
                .collect::<Vec<_>>()
                .windows(2)
                .filter_map(|w| Some(line(*w.first()?, *w.get(1)?)))
                .collect(),
            Stage::Crossing(Some(a)) => vec![lwpoly(rect_vertices(*a, c), true)],
            _ => Vec::new(),
        }
    }
}
