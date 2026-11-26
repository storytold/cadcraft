//! TRIM and EXTEND: the prompt sequence and the JSON forms. Quick and Standard modes
//! (TRIMEXTENDMODE), cutting/boundary edges, Fence, Crossing, Project (PROJMODE), Edge (EDGEMODE),
//! eRase and Undo. The geometry of one trim or extend is in modify.rs.

use std::cell::RefCell;
use std::sync::Arc;

use cadcraft_doc::{Drawing, Entity, EntityKind, Handle};
use cadcraft_geom::{Bounds2, Line, Segment, Vec2, line_line};
use serde_json::{Value, json};

use super::curves::KW;
use super::helpers::{line, lwpoly, rect_vertices};
use super::modify::{
    CUT_ONCE, Edges, Overlay, edge_segments, edge_segments_over, extend, extend_kind, extended, json_pick_error, trim, trim_kind, trimmed,
};
use super::{bad, point_param, points_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// Most fence points the JSON form takes.
const MAX_FENCE: usize = 10_000;
/// Most picks a Fence or Crossing preview works out ([`TrimM`]'s `highlight_all`). Each pick
/// gathers the cutting edges over the whole drawing again, on every cursor move, so a fence across
/// a crowded drawing shows no part highlight at all rather than slow the drawing area down; the
/// fence itself still trims everything it crosses.
pub const PREVIEW_MAX_PICKS: usize = 64;

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
/// A closed polyline cut at one point meets one, and is left as it is.
fn misses_edges(e: &EngineError) -> bool {
    let m = e.to_string();
    m.contains("intersect") && m != CUT_ONCE
}

/// Trim or extend `h` at `at`. A Quick-mode (`quick`) trim of an object no cutting edge crosses
/// erases it. Returns what `h` became: its pieces, `h` itself first (none when it was erased).
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
    let (work, tol) = fence_work(s, pts)?;
    run_picks(s, work, extending, edges, quick, tol)
}

/// What a fence through `pts` trims or extends: each object it crosses with its crossings, in
/// fence order, and the tolerance they were found to.
fn fence_work(s: &Session, pts: &[Vec2]) -> Result<(Vec<(Handle, Vec<Vec2>)>, f64)> {
    let space = s.space();
    let d = s.doc()?;
    let fb = Bounds2::from_points(pts.iter().copied());
    let tol = crate::select::window_tol(&fb);
    let mut work: Vec<(Handle, Vec<Vec2>)> = Vec::new();
    for h in crate::select::select_fence_in(d, &space, s.viewport_frozen(), pts) {
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
    Ok((work, tol))
}

/// How far along a segment from its end a crossing window's pick is kept: past the 5e-7 within
/// which a pick is on a polyline's own cut at that vertex. A cut this near the end is taken as at
/// the end, the pick landing just past it.
const OFF_VERTEX: f64 = 1e-6;

/// Trim (or extend) every object a crossing window from `a` to `b` touches, picked at its point
/// in the window nearest the first corner `a`.
fn crossing(s: &mut Session, a: Vec2, b: Vec2, extending: bool, edges: Edges, quick: bool) -> Result<usize> {
    let (work, tol) = crossing_work(s, a, b)?;
    run_picks(s, work, extending, edges, quick, tol)
}

/// What a crossing window from `a` to `b` trims or extends: each object it touches with its pick
/// point, and the tolerance they were found to.
fn crossing_work(s: &Session, a: Vec2, b: Vec2) -> Result<(Vec<(Handle, Vec<Vec2>)>, f64)> {
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
    for h in crate::select::select_window_in(d, &space, s.viewport_frozen(), bx, true) {
        let Some(e) = d.entity(h) else { continue };
        let mut best: Option<Vec2> = None;
        for pl in crate::select::hit_polylines(d, e, tol) {
            for w in pl.windows(2) {
                let (Some(p), Some(q)) = (w.first(), w.get(1)) else { continue };
                let seg = Line::new(*p, *q);
                // Kept off the segment's ends: a vertex, where two segments meet, is a cut of a
                // polyline that is one of its own edges, and a pick on a cut trims nothing. No
                // further than [`OFF_VERTEX`] (or `tol`, in a smaller window), so the pick stays
                // before a cut near the end, whatever the window's size.
                let e = (tol.min(OFF_VERTEX) / seg.len()).min(0.5);
                let cands = [seg.at(e), seg.at(1.0 - e), seg.at(seg.param_of(a).clamp(e, 1.0 - e))]
                    .into_iter()
                    .chain(sides.iter().filter_map(|sd| line_line(sd, &seg)));
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
    Ok((work, tol))
}

/// What [`run_picks`] would change, worked out without changing the drawing: for each trim, the
/// piece it removes (a whole object Quick mode erases), and for each extend the stretch it adds,
/// with the handle of the object it belongs to. Each object is trimmed at its picks in turn, as
/// `run_picks` does, against edges that include what the run has already made of other objects
/// ([`Overlay`]), so the preview of a fence or a crossing window is what it will do. `px`: the
/// tolerance the changes are drawn to.
fn picks_preview(
    s: &Session,
    work: &[(Handle, Vec<Vec2>)],
    extending: bool,
    edges: Edges,
    quick: bool,
    tol: f64,
    px: f64,
) -> Vec<(Handle, EntityKind)> {
    let Ok(d) = s.doc() else { return Vec::new() };
    let mut overlay = Overlay::new();
    let mut out = Vec::new();
    // Handles for the pieces a trim makes, apart from any in the drawing.
    let mut next = u64::MAX;
    let mut fresh = || {
        next -= 1;
        Handle(next)
    };
    for (h, picks) in work {
        let Some(orig) = d.entity(*h) else { continue };
        if crate::cmd::curves::is_locked(s, *h) {
            continue;
        }
        let entity = |handle: Handle, kind: &EntityKind| Entity { handle, common: orig.common.clone(), kind: kind.clone() };
        let mut live: Vec<(Handle, EntityKind)> = vec![(*h, orig.kind.clone())];
        for x in picks {
            let target = live
                .iter()
                .enumerate()
                .map(|(i, (ph, k))| (i, crate::select::entity_distance(d, &entity(*ph, k), *x, tol)))
                .filter(|(_, dist)| *dist <= 3.0 * tol)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            let Some(i) = target else { continue };
            let Some((ph, piece)) = live.get(i).cloned() else { continue };
            overlay.insert(*h, live.clone());
            let Ok(cut) = edge_segments_over(s, edges, ph, &overlay) else { continue };
            if extending {
                if let Ok(grown) = extend_kind(&piece, *x, &cut) {
                    out.extend(uncovered(d, &entity(ph, &grown), std::slice::from_ref(&piece), px).into_iter().map(|k| (*h, k)));
                    if let Some(slot) = live.get_mut(i) {
                        slot.1 = grown;
                    }
                }
                break;
            }
            let itself = edges.only.is_none_or(|o| o.contains(&ph));
            match trim_kind(&piece, *x, itself, &cut) {
                Ok(Some(kept)) => {
                    out.extend(uncovered(d, &entity(ph, &piece), &kept, px).into_iter().map(|k| (*h, k)));
                    live.remove(i);
                    let mut kept = kept.into_iter();
                    if let Some(first) = kept.next() {
                        live.insert(i, (ph, first));
                    }
                    live.extend(kept.map(|k| (fresh(), k)));
                }
                Err(err) if quick && misses_edges(&err) => {
                    out.push((*h, piece));
                    live.remove(i);
                }
                Ok(None) | Err(_) => {}
            }
        }
        overlay.insert(*h, live);
    }
    out
}

/// Trim (or extend) each object at its pick points. A trim leaves the object in pieces, so later
/// picks go to the piece within `tol` of the point (none: that part is already gone).
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
    let quick = match p.get("mode") {
        None | Some(Value::Null) => false,
        Some(v) => match v.as_str().map(|m| m.trim().to_ascii_lowercase()).as_deref() {
            Some("quick") => true,
            Some("standard") => false,
            _ => return Err(bad(cmd, "`mode` must be \"quick\" or \"standard\"")),
        },
    };
    let edges = Edges { only: only.as_deref(), implied: edge_mode_param(cmd, p)? };
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
    let pick = point_param(p, "pick").ok_or_else(|| {
        bad(
            cmd,
            if extending {
                "`pick` ([x, y]) is required: point toward the boundary to extend to, selecting the nearer end"
            } else {
                "`pick` ([x, y]) is required: select the portion of the object to trim away"
            },
        )
    })?;
    // Diagnose only an existing single-pick JSON failure. Successful calls, Quick-mode
    // erasure, fence/crossing and the interactive command keep their existing behavior.
    let r = apply(s, h, pick, extending, edges, quick).map_err(|e| json_pick_error(s, h, pick, edges, extending, e))?;
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
    /// Quick mode's two-point fence, started by a pick in empty space: its first point.
    QuickFence(Vec2),
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
    /// The edges of the object under the cursor, for the rollover highlight ([`CutCache`]).
    cut: RefCell<Option<CutCache>>,
}

/// The cutting or boundary edges of the object under the cursor, kept while the drawing, the
/// object and the modes stay the same. The rollover highlight works a trim out at every cursor
/// move, and in Quick mode the edges are every object in the drawing: gathering them once keeps
/// moving the cursor along one object cheap.
struct CutCache {
    doc: usize,
    h: Handle,
    quick: bool,
    implied: bool,
    cut: Vec<Segment>,
}

impl TrimM {
    pub(crate) fn new(extend: bool) -> Self {
        TrimM { extend, edges: None, stage: Stage::Pick, undo: Vec::new(), cut: RefCell::new(None) }
    }
    /// What a click on `h` at `cursor` changes: the piece a trim removes (the whole object when a
    /// Quick-mode trim erases it), or the stretch an extend adds. `None` when the click would
    /// change nothing or can't be worked out; the rollover highlight then shows the whole object.
    fn change(&self, s: &Session, h: Handle, cursor: Vec2) -> Option<Vec<EntityKind>> {
        let (quick, implied) = (quick_mode(s), edge_mode(s));
        let doc = Arc::as_ptr(&s.state().ok()?.doc) as usize;
        let mut cache = self.cut.borrow_mut();
        if !cache.as_ref().is_some_and(|c| c.doc == doc && c.h == h && c.quick == quick && c.implied == implied) {
            let cut = edge_segments(s, self.edges(quick, implied), h).ok()?;
            *cache = Some(CutCache { doc, h, quick, implied, cut });
        }
        let cut = &cache.as_ref()?.cut;
        let d = s.doc().ok()?;
        let e = d.entity(h)?;
        // Half a pixel: finer than the eye can tell apart, coarse enough to stay quick.
        let tol = (s.pixel_size() / 2.0).max(1e-9);
        let part = if self.extend {
            let grown = Entity { kind: extended(s, h, cursor, cut).ok()?, ..(**e).clone() };
            uncovered(d, &grown, std::slice::from_ref(&e.kind), tol)
        } else {
            match trimmed(s, h, cursor, self.edges(quick, implied), cut) {
                Ok(Some(kept)) => uncovered(d, e, &kept, tol),
                Ok(None) => return None,
                Err(err) if quick && misses_edges(&err) => vec![e.kind.clone()],
                Err(_) => return None,
            }
        };
        (!part.is_empty()).then_some(part)
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
            Stage::QuickFence(a) => Prompt::new("Specify next fence point", Accept::POINT).base(*a),
            Stage::Crossing(None) => Prompt::new("Specify first corner", Accept::POINT),
            Stage::Crossing(Some(_)) => Prompt::new("Specify opposite corner", Accept::POINT),
            Stage::Mode => {
                let msg = if self.extend { "Enter an extend mode option" } else { "Enter a trim mode option" };
                Prompt::new(msg, KW).kw(&["Quick", "Standard"]).default(if quick_mode(s) { "Quick" } else { "Standard" })
            }
            Stage::Project => {
                let cur = match proj_mode(s) {
                    0 => "None",
                    2 => "View",
                    _ => "Ucs",
                };
                Prompt::new("Enter a projection option", KW).kw(&["None", "Ucs", "View"]).default(cur)
            }
            Stage::Edge => Prompt::new("Enter an implied edge extension mode", KW).kw(&["Extend", "No extend"]).default(if edge_mode(s) {
                "Extend"
            } else {
                "No extend"
            }),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        // The edges may change (picked again, Undo): gather them afresh for the highlight.
        self.cut.take();
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
            Stage::QuickFence(a) => {
                self.stage = Stage::Pick;
                if let Input::Point(b) = i {
                    let r = self.step(s, |m, s| fence(s, &[a, b], extending, m.edges(quick, implied), quick).map(|_| ()));
                    if let Err(e) = r {
                        s.echo(e.to_string());
                    }
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
                        // A pick in empty space starts a two-point fence in Quick mode (as in
                        // AutoCAD, #1594) and a crossing window in Standard mode.
                        self.stage = if quick { Stage::QuickFence(p) } else { Stage::Crossing(Some(p)) };
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
    fn highlight(&self, s: &Session, h: Handle, cursor: Vec2) -> Option<Vec<EntityKind>> {
        if self.stage != Stage::Pick {
            return None;
        }
        self.change(s, h, cursor)
    }
    fn highlight_all(&self, s: &Session, cursor: Vec2) -> Option<Vec<(Handle, EntityKind)>> {
        let (quick, implied) = (quick_mode(s), edge_mode(s));
        let found = match &self.stage {
            Stage::Fence(pts) if !pts.is_empty() => {
                let pts: Vec<Vec2> = pts.iter().copied().chain(std::iter::once(cursor)).collect();
                fence_work(s, &pts)
            }
            Stage::QuickFence(a) => fence_work(s, &[*a, cursor]),
            Stage::Crossing(Some(a)) => crossing_work(s, *a, cursor),
            _ => return None,
        };
        let (work, tol) = found.ok()?;
        if work.iter().map(|(_, picks)| picks.len()).sum::<usize>() > PREVIEW_MAX_PICKS {
            return None;
        }
        let px = (s.pixel_size() / 2.0).max(1e-9);
        Some(picks_preview(s, &work, self.extend, self.edges(quick, implied), quick, tol, px))
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
            Stage::QuickFence(a) => vec![line(*a, c)],
            Stage::Crossing(Some(a)) => vec![lwpoly(rect_vertices(*a, c), true)],
            _ => Vec::new(),
        }
    }
}

/// The stretches of `e`'s outline that none of `others` runs along, as polylines flattened to
/// `tol`: what a trim takes away (the object less the pieces it keeps), or what an extend adds
/// (the object as extended less the object as it was). Each segment of the outline is first split
/// where one of `others` ends on it, so a stretch is either all on them or all off them.
fn uncovered(d: &Drawing, e: &Entity, others: &[EntityKind], tol: f64) -> Vec<EntityKind> {
    // Two flattenings of one curve, each within `tol` of it, lie within twice that of each other.
    let near = 3.0 * tol;
    let theirs: Vec<Vec<Vec2>> =
        chains(others.iter().flat_map(|k| crate::select::hit_polylines(d, &Entity { kind: k.clone(), ..e.clone() }, tol)).collect());
    let ends: Vec<Vec2> = theirs.iter().flat_map(|pl| [pl.first().copied(), pl.last().copied()]).flatten().collect();
    // Their segments, each with its box and the ends of the piece it belongs to, worked out once:
    // the test below runs for every stretch of the outline.
    let spans: Vec<(Line, Bounds2, Vec2, Vec2)> = theirs
        .iter()
        .flat_map(|pl| {
            let ends = (pl.first().copied(), pl.last().copied());
            pl.windows(2).filter_map(move |w| {
                let (a, b) = (*w.first()?, *w.get(1)?);
                Some((Line::new(a, b), Bounds2::from_points([a, b]), ends.0?, ends.1?))
            })
        })
        .collect();
    // On them: near one of them, and not only near one of their ends (a point just past where a
    // piece ends is near that end, yet off the piece). A segment whose box is farther than `near`
    // from the point can't be that near it, so it is passed over without working out its distance.
    let on_theirs = |p: Vec2| {
        spans.iter().any(|(l, b, first, last)| {
            if p.x < b.min.x - near || p.x > b.max.x + near || p.y < b.min.y - near || p.y > b.max.y + near {
                return false;
            }
            let c = l.closest(p);
            c.dist(p) <= near && !(c.near(*first, 1e-12 * (1.0 + c.len())) || c.near(*last, 1e-12 * (1.0 + c.len())))
        })
    };
    let mut out = Vec::new();
    let mut run: Vec<Vec2> = Vec::new();
    let mut flush = |run: &mut Vec<Vec2>| {
        if run.len() >= 2 {
            out.push(lwpoly(std::mem::take(run).into_iter().map(cadcraft_geom::PolyVertex::new).collect(), false));
        }
        run.clear();
    };
    for pl in chains(crate::select::hit_polylines(d, e, tol)) {
        for w in pl.windows(2) {
            let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
            let ab = Line::new(a, b);
            let len = ab.len();
            if len <= 0.0 {
                continue;
            }
            let mut ts: Vec<f64> = ends.iter().filter(|p| ab.closest(**p).dist(**p) <= near).map(|p| ab.param_of(*p).clamp(0.0, 1.0)).collect();
            ts.extend([0.0, 1.0]);
            ts.sort_by(f64::total_cmp);
            ts.dedup_by(|x, y| (*x - *y).abs() * len <= 1e-9 * (1.0 + a.len()));
            for t in ts.windows(2) {
                let (Some(&t0), Some(&t1)) = (t.first(), t.get(1)) else { continue };
                let (p, q) = (ab.at(t0), ab.at(t1));
                if on_theirs(ab.at((t0 + t1) / 2.0)) {
                    flush(&mut run);
                } else {
                    if run.last().is_none_or(|l| !l.near(p, 1e-12 * (1.0 + p.len()))) {
                        flush(&mut run);
                        run.push(p);
                    }
                    run.push(q);
                }
            }
        }
        flush(&mut run);
    }
    out
}

/// `pls` with each one that starts where the one before it ends joined onto it: a curve's
/// flattening may come as many short pieces, one after the other.
fn chains(pls: Vec<Vec<Vec2>>) -> Vec<Vec<Vec2>> {
    let mut out: Vec<Vec<Vec2>> = Vec::new();
    for pl in pls {
        match (out.last_mut(), pl.first()) {
            (Some(last), Some(&first)) if last.last().is_some_and(|l| l.near(first, 1e-12 * (1.0 + first.len()))) => {
                last.extend(pl.into_iter().skip(1));
            }
            _ => out.push(pl),
        }
    }
    out
}
