//! PDFCLIP: clip a PDF underlay to a rectangle or a polygon (or, inverted, hide what is inside
//! it), show the whole page again (OFF) or the clipped part (ON), or delete the boundary. The boundary is kept in the page's own units
//! ([`PdfUnderlay::clip`]), so it follows the underlay when it is moved, turned or scaled.
//!
//! A rectangular boundary is aligned with the page (on a turned page, the two
//! corners given are taken in the page's own axes) and cut to the page's edges.

use cadcraft_doc::{EntityKind, Handle, PdfUnderlay};
use cadcraft_geom::Vec2;
use cadcraft_render::clip::PolyClip;
use serde_json::{Value, json};

use super::super::machines::{SelOutcome, SelectPhase};
use super::super::{CommandSpec, bad, points_param, targets};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

const ID: &str = "pdfclip";
/// Most vertices of a polygonal clip boundary.
const MAX_VERTICES: usize = 10_000;

pub(super) fn spec() -> CommandSpec {
    CommandSpec::new(ID, "Clip PDF Underlay", run)
        .menu(&["Modify", "Clip", "PDF Underlay"])
        .params("{handle?|handles?: PDF underlays (default the selection), and one of: rect: [[x,y],[x,y]] opposite corners | polygon: [[x,y],...] at least 3 vertices | polyline: handle of a polyline whose outline is the boundary | on: bool (apply or ignore the boundary, keeping it) | delete: true; invert?: bool (with rect/polygon, or alone: hide the inside instead)} → {clipped}")
        .interactive(|_| Ok(Box::new(ClipM::default())))
}

/// What PDFCLIP does to the underlays.
#[derive(Clone)]
enum Change {
    /// A new polygonal boundary, in drawing units, and whether it is inverted.
    Boundary(Vec<Vec2>, bool),
    /// A new rectangular boundary from two opposite corners in drawing units (see
    /// [`page_rectangle`]), and whether it is inverted.
    Rect(Vec2, Vec2, bool),
    On(bool),
    Invert(bool),
    Delete,
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let invert = match p.get("invert") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool().ok_or_else(|| bad(ID, "`invert` must be true or false"))?),
    };
    let polyline = match p.get("polyline") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let h = v.as_str().and_then(Handle::parse_hex).ok_or_else(|| bad(ID, "`polyline` must be an object handle"))?;
            Some(polyline_outline(s, h).ok_or_else(|| bad(ID, "`polyline` must be a polyline enclosing an area"))?)
        }
    };
    let change =
        match (p.get("rect"), p.get("polygon").map(Some).unwrap_or(polyline.as_ref().map(|_| &Value::Null)), p.get("on"), p.get("delete"), invert) {
            (Some(_), None, None, None, inv) => match points_param(p, "rect").as_deref() {
                Some([a, b]) => Change::Rect(*a, *b, inv.unwrap_or(false)),
                _ => return Err(bad(ID, "`rect` must be two opposite corners [[x,y],[x,y]]")),
            },
            (None, Some(_), None, None, inv) if polyline.is_some() && p.get("polygon").is_none() => {
                Change::Boundary(polyline.clone().unwrap_or_default(), inv.unwrap_or(false))
            }
            (None, Some(_), None, None, inv) if polyline.is_none() => match points_param(p, "polygon") {
                Some(pts) if (3..=MAX_VERTICES).contains(&pts.len()) => Change::Boundary(pts, inv.unwrap_or(false)),
                _ => return Err(bad(ID, format!("`polygon` must be 3 to {MAX_VERTICES} vertices [[x,y],...]"))),
            },
            (None, None, Some(v), None, None) => Change::On(v.as_bool().ok_or_else(|| bad(ID, "`on` must be true or false"))?),
            (None, None, None, Some(Value::Bool(true)), None) => Change::Delete,
            (None, None, None, None, Some(inv)) => Change::Invert(inv),
            _ => return Err(bad(ID, "give one of `rect`, `polygon`, `on`, `delete: true` or `invert`")),
        };
    if let Change::Boundary(pts, _) = &change
        && PolyClip::new(pts).is_none()
    {
        return Err(bad(ID, "the boundary encloses no area"));
    }
    let hs = targets(s, p)?;
    if let Change::Rect(a, b, _) = &change {
        let d = s.doc()?;
        let misses = |h: &Handle| match d.entity(*h).map(|e| &e.kind) {
            Some(EntityKind::PdfUnderlay(u)) => page_rectangle(u, *a, *b).is_none(),
            _ => false,
        };
        if !hs.is_empty() && hs.iter().all(misses) {
            return Err(bad(ID, "the rectangle covers none of the page"));
        }
    }
    let n = apply(s, &hs, &change)?;
    if n == 0 {
        return Err(bad(ID, "no PDF underlay to clip (select one, or pass `handle`)"));
    }
    Ok(json!({ "clipped": n }))
}

/// The four corners of the rectangle with opposite corners `a` and `b`.
fn rectangle(a: Vec2, b: Vec2) -> Vec<Vec2> {
    vec![a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)]
}

/// Most vertices taken from a polyline's outline (arcs are cut into short segments).
const MAX_OUTLINE: usize = MAX_VERTICES;

/// The outline of polyline `h` (its arcs as short segments): the boundary PDFCLIP's "Select
/// polyline" takes. `None` when it isn't a polyline or encloses no area.
fn polyline_outline(s: &Session, h: Handle) -> Option<Vec<Vec2>> {
    let d = s.doc().ok()?;
    let e = d.entity(h)?;
    if !matches!(e.kind, EntityKind::LwPolyline(_) | EntityKind::Polyline3d(_)) {
        return None;
    }
    let b = cadcraft_doc::entity_bounds(d, e, 0);
    let tol = 1e-3 * b.width().max(b.height()).max(1e-9);
    let mut pts: Vec<Vec2> = Vec::new();
    for prim in e.kind.prims() {
        if let cadcraft_doc::Prim::Seg(seg) = prim {
            let mut piece = Vec::new();
            seg.tessellate(tol, &mut piece);
            for q in piece {
                if pts.last().is_none_or(|l| !l.near(q, 1e-12)) && pts.len() < MAX_OUTLINE {
                    pts.push(q);
                }
            }
        }
    }
    if pts.len() > 1 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.near(*b, 1e-12)) {
        pts.pop();
    }
    (pts.len() >= 3 && PolyClip::new(&pts).is_some()).then_some(pts)
}

/// Underlay `u`'s rectangular clip boundary from opposite corners `a` and `b` (drawing units),
/// in the page's own units: aligned with the page and cut to its edges.
/// `None` when it covers none of the page.
fn page_rectangle(u: &PdfUnderlay, a: Vec2, b: Vec2) -> Option<Vec<Vec2>> {
    let (pa, pb) = (u.to_page(a)?, u.to_page(b)?);
    let lo = Vec2::new(pa.x.min(pb.x).max(0.0), pa.y.min(pb.y).max(0.0));
    let hi = Vec2::new(pa.x.max(pb.x).min(u.size.x), pa.y.max(pb.y).min(u.size.y));
    let tiny = 1e-9 * u.size.x.abs().max(u.size.y.abs()).max(1e-9);
    (hi.x - lo.x > tiny && hi.y - lo.y > tiny).then(|| rectangle(lo, hi))
}

/// Make `change` to the PDF underlays among `hs` (those on locked layers are left alone).
/// Returns how many were changed.
fn apply(s: &mut Session, hs: &[Handle], change: &Change) -> Result<usize> {
    let d = s.doc()?;
    let ids: Vec<Handle> = hs
        .iter()
        .copied()
        .filter(|h| d.entity(*h).is_some_and(|e| matches!(e.kind, EntityKind::PdfUnderlay(_)) && !d.layer(&e.common.layer).is_some_and(|l| l.locked)))
        .collect();
    if ids.is_empty() {
        return Ok(0);
    }
    let d = s.doc_mut()?;
    let mut n = 0;
    for h in &ids {
        d.modify_entity(*h, |e| {
            if let EntityKind::PdfUnderlay(u) = &mut e.kind {
                n += 1;
                match change {
                    Change::Rect(a, b, inverted) => match page_rectangle(u, *a, *b) {
                        Some(r) => {
                            u.clip = r;
                            u.clipping = true;
                            u.clip_inverted = *inverted;
                        }
                        None => n -= 1,
                    },
                    Change::Boundary(pts, inverted) => {
                        let page: Option<Vec<Vec2>> = pts.iter().map(|p| u.to_page(*p)).collect();
                        if let Some(page) = page {
                            u.clip = page;
                            u.clipping = true;
                            u.clip_inverted = *inverted;
                        }
                    }
                    Change::On(on) => u.clipping = *on && u.clip.len() >= 3,
                    Change::Invert(inv) => u.clip_inverted = *inv && u.clip.len() >= 3,
                    Change::Delete => {
                        u.clip.clear();
                        u.clipping = false;
                        u.clip_inverted = false;
                    }
                }
            }
        })?;
    }
    Ok(n)
}

#[derive(Default, PartialEq)]
enum Stage {
    #[default]
    Select,
    Option,
    Kind,
    Corner,
    Opposite,
    Polygon,
    Polyline,
}

/// PDFCLIP typed or from a menu: select an underlay, then ON / OFF / Delete / New boundary,
/// Polygonal or Rectangular, and the boundary's points.
#[derive(Default)]
struct ClipM {
    stage: Stage,
    sel: SelectPhase,
    target: Option<Handle>,
    pts: Vec<Vec2>,
    /// The new boundary will be inverted ("Invert clip" at the clipping type prompt).
    invert: bool,
    /// Picking the polyline whose outline is the boundary ("Select polyline").
    poly_sel: SelectPhase,
}

impl ClipM {
    fn underlay(&self, s: &Session) -> Option<PdfUnderlay> {
        match &s.doc().ok()?.entity(self.target?)?.kind {
            EntityKind::PdfUnderlay(u) => Some(u.clone()),
            _ => None,
        }
    }
    fn selected(&mut self, s: &mut Session, hs: Vec<Handle>) -> Result<Step> {
        s.set_selection(Vec::new());
        let d = s.doc()?;
        self.target = hs.into_iter().find(|h| d.entity(*h).is_some_and(|e| matches!(e.kind, EntityKind::PdfUnderlay(_))));
        if self.target.is_none() {
            s.echo("No PDF underlay selected.");
            return Ok(Step::Done);
        }
        self.stage = Stage::Option;
        Ok(Step::Continue)
    }
    fn finish(&mut self, s: &mut Session, change: Change) -> Result<Step> {
        let hs: Vec<Handle> = self.target.into_iter().collect();
        if apply(s, &hs, &change)? == 0 {
            s.echo("The PDF underlay is on a locked layer.");
        }
        Ok(Step::Done)
    }
}

impl Interactive for ClipM {
    fn name(&self) -> &'static str {
        "PDFCLIP"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return self.selected(s, hs);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            Stage::Select => self.sel.prompt(),
            Stage::Option => Prompt::new("Enter clipping option", Accept::TEXT).kw(&["ON", "OFF", "Delete", "New boundary"]).default("New boundary"),
            Stage::Kind => {
                let msg = if self.invert { "Inverted: the page inside the boundary is hidden. Enter clipping type" } else { "Enter clipping type" };
                Prompt::new(msg, Accept::TEXT).kw(&["Select polyline", "Polygonal", "Rectangular", "Invert clip"]).default("Rectangular")
            }
            Stage::Polyline => Prompt::new("Select polyline", Accept::SELECT),
            Stage::Corner => Prompt::new("Specify first corner point", Accept::POINT),
            Stage::Opposite => match self.pts.first() {
                Some(a) => Prompt::new("Specify opposite corner point", Accept::POINT).corner(*a),
                None => Prompt::new("Specify opposite corner point", Accept::POINT),
            },
            Stage::Polygon => {
                let msg = if self.pts.is_empty() { "Specify first point" } else { "Specify next point" };
                let p = Prompt::new(msg, Accept::POINT).base_opt(self.pts.last().copied());
                if self.pts.is_empty() { p } else { p.kw(&["Undo"]) }
            }
        }
    }
    fn preview(&self, s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        let outline = match (&self.stage, self.pts.first()) {
            // The rectangle that will be kept: aligned with the page and cut to it.
            (Stage::Opposite, Some(a)) => match self.underlay(s) {
                Some(u) => match page_rectangle(&u, *a, cursor) {
                    Some(r) => r.into_iter().map(|p| u.to_drawing(p)).collect(),
                    None => return Vec::new(),
                },
                None => rectangle(*a, cursor),
            },
            (Stage::Polygon, Some(_)) => self.pts.iter().copied().chain([cursor]).collect(),
            _ => return Vec::new(),
        };
        let n = outline.len();
        (0..n)
            .filter_map(|i| {
                let (a, b) = (outline.get(i)?, outline.get((i + 1) % n)?);
                (a != b).then(|| EntityKind::Line(cadcraft_doc::Line { a: a.to3(0.0), b: b.to3(0.0) }))
            })
            .collect()
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let text = match &i {
            Input::Keyword(k) | Input::Text(k) => Some(k.trim().to_ascii_lowercase()),
            _ => None,
        };
        match self.stage {
            Stage::Select => match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => self.selected(s, hs),
            },
            Stage::Option => {
                let has = self.underlay(s).is_some_and(|u| u.clip.len() >= 3);
                match (&i, text.as_deref()) {
                    (Input::Enter, _) => self.stage = Stage::Kind,
                    (_, Some(t)) if t.starts_with('n') => self.stage = Stage::Kind,
                    (_, Some("on" | "off" | "d" | "delete")) if !has => {
                        s.echo("The PDF underlay has no clip boundary.");
                        return Ok(Step::Done);
                    }
                    (_, Some("on")) => return self.finish(s, Change::On(true)),
                    (_, Some("off")) => return self.finish(s, Change::On(false)),
                    (_, Some("d" | "delete")) => return self.finish(s, Change::Delete),
                    _ => s.echo("Invalid option keyword."),
                }
                Ok(Step::Continue)
            }
            Stage::Kind => {
                match (&i, text.as_deref()) {
                    (Input::Enter, _) | (_, Some("r" | "rectangular")) => self.stage = Stage::Corner,
                    (_, Some("p" | "polygonal")) => self.stage = Stage::Polygon,
                    (_, Some(t)) if t.starts_with('s') => {
                        self.poly_sel = SelectPhase::single();
                        self.stage = Stage::Polyline;
                    }
                    (_, Some(t)) if t.starts_with('i') => self.invert = !self.invert,
                    _ => s.echo("Invalid option keyword."),
                }
                Ok(Step::Continue)
            }
            Stage::Polyline => match self.poly_sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    s.set_selection(Vec::new());
                    match hs.iter().find_map(|h| polyline_outline(s, *h)) {
                        Some(pts) => self.finish(s, Change::Boundary(pts, self.invert)),
                        None => {
                            s.echo("Select a polyline enclosing an area.");
                            self.poly_sel = SelectPhase::single();
                            Ok(Step::Continue)
                        }
                    }
                }
            },
            Stage::Corner => {
                if let Input::Point(p) = i {
                    self.pts = vec![p];
                    self.stage = Stage::Opposite;
                }
                Ok(Step::Continue)
            }
            Stage::Opposite => match (&i, self.pts.first()) {
                (Input::Point(b), Some(a)) => {
                    if self.underlay(s).is_none_or(|u| page_rectangle(&u, *a, *b).is_none()) {
                        s.echo("The rectangle covers none of the page.");
                        return Ok(Step::Continue);
                    }
                    self.finish(s, Change::Rect(*a, *b, self.invert))
                }
                _ => Ok(Step::Continue),
            },
            Stage::Polygon => match (&i, text.as_deref()) {
                (Input::Point(p), _) => {
                    if self.pts.len() < MAX_VERTICES {
                        self.pts.push(*p);
                    }
                    Ok(Step::Continue)
                }
                (_, Some("u" | "undo")) => {
                    self.pts.pop();
                    Ok(Step::Continue)
                }
                (Input::Enter, _) => {
                    if self.pts.len() < 3 || PolyClip::new(&self.pts).is_none() {
                        s.echo("A polygonal boundary needs at least 3 points enclosing an area.");
                        return Ok(if self.pts.is_empty() { Step::Done } else { Step::Continue });
                    }
                    let pts = std::mem::take(&mut self.pts);
                    self.finish(s, Change::Boundary(pts, self.invert))
                }
                _ => Ok(Step::Continue),
            },
        }
    }
}
