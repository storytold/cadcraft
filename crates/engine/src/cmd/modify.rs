//! Modify commands: ERASE, MOVE, COPY, ROTATE, SCALE, MIRROR, OFFSET, TRIM, EXTEND, FILLET,
//! CHAMFER, EXPLODE, STRETCH, BREAK, JOIN (arrays: `array`; draw order: `draworder`).

use cadcraft_doc::{Entity, EntityKind, Handle, Prim};
use cadcraft_geom::{
    Arc, Circle, Line, Mat3, PolyVertex, Polyline, Segment, TAU, Vec2, angle_in_sweep, ccw_sweep, intersect_ext, line_line_infinite, norm_angle,
};
use serde_json::{Value, json};

use super::helpers::*;
use super::machines::{SelOutcome, SelectPhase, number};
use super::trimextend::{self, TrimM};
use super::*;
use crate::select::{SelectKind, SelectMode};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("erase", "Erase", run_erase)
            .menu(&["Modify", "Erase"])
            .alias(&["e", "delete"])
            .params("{handles?: [hex]} (default: selection)")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Erase)))),
        CommandSpec::new("move", "Move", run_move)
            .menu(&["Modify", "Move"])
            .alias(&["m"])
            .params("{handles?, from?: [x,y], to?: [x,y] | delta: [dx,dy]}")
            .interactive(|_| Ok(Box::new(DisplaceM::new(Op::Move)))),
        CommandSpec::new("copy", "Copy", run_copy)
            .menu(&["Modify", "Copy"])
            .alias(&["co", "cp"])
            .params("{handles?, delta: [dx,dy] | from,to, count?: n}")
            .interactive(|_| Ok(Box::new(DisplaceM::new(Op::Copy)))),
        CommandSpec::new("rotate", "Rotate", run_rotate)
            .menu(&["Modify", "Rotate"])
            .alias(&["ro"])
            .params("{handles?, base: [x,y], angle (degrees), copy?: bool}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Rotate)))),
        CommandSpec::new("scale", "Scale", run_scale)
            .menu(&["Modify", "Scale"])
            .alias(&["sc"])
            .params("{handles?, base: [x,y], factor, copy?: bool}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Scale)))),
        CommandSpec::new("mirror", "Mirror", run_mirror)
            .menu(&["Modify", "Mirror"])
            .alias(&["mi"])
            .params("{handles?, p1, p2, erase?: bool}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Mirror)))),
        CommandSpec::new("stretch", "Stretch", run_stretch)
            .menu(&["Modify", "Stretch"])
            .alias(&["s"])
            .params("{window: [[x,y],[x,y]] | polygon: [[x,y],...] | fence: [[x,y],...], delta: [dx,dy]}")
            .interactive(|_| Ok(Box::new(StretchM::default()))),
        CommandSpec::new("offset", "Offset", run_offset)
            .menu(&["Modify", "Offset"])
            .alias(&["o"])
            .params("{handle, distance, side: [x,y], erase?: bool, layer?: \"source\"|\"current\"}")
            .interactive(|s| Ok(Box::new(OffsetM::new(s)))),
        CommandSpec::new("trim", "Trim", trimextend::run_trim)
            .menu(&["Modify", "Trim"])
            .alias(&["tr"])
            .params(
                "{handle, pick: [x,y] | fence: [[x,y],...] | crossing: [[x,y],[x,y]], edges?: [hex], edgeMode?: \"extend\"|\"none\", mode?: \"quick\"|\"standard\"}",
            )
            .interactive(|_| Ok(Box::new(TrimM::new(false)))),
        CommandSpec::new("extend", "Extend", trimextend::run_extend)
            .menu(&["Modify", "Extend"])
            .alias(&["ex"])
            .params("{handle, pick: [x,y] | fence: [[x,y],...] | crossing: [[x,y],[x,y]], edges?: [hex], edgeMode?: \"extend\"|\"none\"}")
            .interactive(|_| Ok(Box::new(TrimM::new(true)))),
        CommandSpec::new("fillet", "Fillet", run_fillet)
            .menu(&["Modify", "Fillet"])
            .alias(&["f"])
            .params("{h1, p1, h2, p2, radius?, trim?: bool} (lines, arcs, circles) | {handle, polyline: true, radius?}")
            .interactive(|s| Ok(Box::new(FilletM::new(s, false)))),
        CommandSpec::new("chamfer", "Chamfer", run_chamfer)
            .menu(&["Modify", "Chamfer"])
            .alias(&["cha"])
            .params("{h1, p1, h2, p2, d1?, d2? | length, angle (degrees), trim?: bool} | {handle, polyline: true, d1?, d2? | length, angle}")
            .interactive(|s| Ok(Box::new(FilletM::new(s, true)))),
        CommandSpec::new("explode", "Explode", run_explode)
            .menu(&["Modify", "Explode"])
            .alias(&["x"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Explode)))),
        CommandSpec::new("break", "Break", run_break)
            .menu(&["Modify", "Break"])
            .alias(&["br"])
            .params("{handle, p1, p2}")
            .interactive(|_| Ok(Box::new(BreakM::default()))),
        CommandSpec::new("breakatpoint", "Break At Point", run_breakat).menu(&["Modify", "Break At Point"]).params("{handle, at}"),
        CommandSpec::new("join", "Join", run_join)
            .menu(&["Modify", "Join"])
            .alias(&["j"])
            .params("{handles?, close?: bool (a single arc becomes its circle, an elliptical arc its full ellipse)}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Join)))),
    ]
}

// ---------------- core operations ----------------

pub(crate) fn transform_entities(s: &mut Session, hs: &[Handle], m: &Mat3, copy: bool) -> Result<Vec<Handle>> {
    let d = s.doc_mut()?;
    let mut out = Vec::new();
    for h in hs {
        let Some(sp) = d.space_of(*h) else { continue };
        if copy {
            let Some(e) = d.entity(*h).map(|e| (**e).clone()) else { continue };
            let nh = d.new_handle();
            let mut ne = Entity { handle: nh, ..e };
            ne.kind.transform(m);
            if let Some(st) = d.space_mut(&sp) {
                st.push(ne);
                out.push(nh);
            }
        } else {
            let locked = d.entity(*h).and_then(|e| d.layer(&e.common.layer)).is_some_and(|l| l.locked);
            if locked {
                continue;
            }
            d.modify_entity(*h, |e| e.kind.transform(m))?;
            out.push(*h);
        }
    }
    Ok(out)
}

fn erase(s: &mut Session, hs: &[Handle]) -> Result<usize> {
    let d = s.doc_mut()?;
    let mut n = 0;
    for h in hs {
        let locked = d.entity(*h).and_then(|e| d.layer(&e.common.layer)).is_some_and(|l| l.locked);
        if !locked && d.remove_entity(*h).is_some() {
            n += 1;
        }
    }
    s.set_selection(Vec::new());
    Ok(n)
}

fn run_erase(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let n = erase(s, &hs)?;
    Ok(json!({ "erased": n }))
}

fn delta_of(cmd: &str, p: &Value) -> Result<Vec2> {
    if let Some(d) = point_param(p, "delta") {
        return Ok(d);
    }
    let a = point_req(cmd, p, "from")?;
    let b = point_req(cmd, p, "to")?;
    Ok(b - a)
}

fn run_move(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let d = delta_of("move", p)?;
    let moved = transform_entities(s, &hs, &Mat3::translate(d), false)?;
    Ok(json!({ "moved": moved.len() }))
}

fn run_copy(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let d = delta_of("copy", p)?;
    let n = p.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 10_000);
    let mut out = Vec::new();
    for k in 1..=n {
        out.extend(transform_entities(s, &hs, &Mat3::translate(d * k as f64), true)?.into_iter().map(|h| h.hex()));
    }
    Ok(json!({ "handles": out }))
}

fn run_rotate(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let base = point_req("rotate", p, "base")?;
    let a = f64_req("rotate", p, "angle")?.to_radians();
    let r = transform_entities(s, &hs, &Mat3::rotate_about(base, a), bool_or(p, "copy", false))?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

fn run_scale(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let base = point_req("scale", p, "base")?;
    let f = f64_req("scale", p, "factor")?;
    if f <= 0.0 {
        return Err(bad("scale", "factor must be positive"));
    }
    let r = transform_entities(s, &hs, &Mat3::scale_about(base, f), bool_or(p, "copy", false))?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

fn run_mirror(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let a = point_req("mirror", p, "p1")?;
    let b = point_req("mirror", p, "p2")?;
    if a.near(b, 1e-12) {
        return Err(bad("mirror", "mirror line points coincide"));
    }
    let erase_src = bool_or(p, "erase", false);
    let r = transform_entities(s, &hs, &Mat3::mirror(a, b), !erase_src)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

/// An area whose vertices STRETCH moves: a window's box, or a polygon closed back to its first
/// point.
#[derive(Clone)]
enum StretchArea {
    Box(cadcraft_geom::Bounds2),
    Polygon(Vec<Vec2>),
}

impl StretchArea {
    fn contains(&self, p: Vec2) -> bool {
        match self {
            StretchArea::Box(bx) => bx.contains(p),
            StretchArea::Polygon(poly) => cadcraft_geom::point_in_polygon(poly, p),
        }
    }
}

/// Add to `hs` the visible multilines in `space` with a control vertex inside `area`. A crossing
/// selection tests the drawn element lines, which sit away from the control vertices by the
/// multiline's scale, so a small area around a vertex alone would miss them.
fn add_mline_vertex_hits(d: &cadcraft_doc::Drawing, space: &cadcraft_doc::Space, area: &StretchArea, hs: &mut Vec<Handle>) {
    let Some(store) = d.space(space) else { return };
    for e in store.iter() {
        if let EntityKind::MLine(m) = &e.kind
            && d.is_visible(e)
            && !hs.contains(&e.handle)
            && m.vertices.iter().any(|v| area.contains(v.p.xy()))
        {
            hs.push(e.handle);
        }
    }
}

/// Move the vertices of `e` that lie inside any of the areas `wins` by `d` (STRETCH).
fn stretch_entity(e: &mut Entity, wins: &[StretchArea], d: Vec2) {
    let inside = |p: Vec2| wins.iter().any(|a| a.contains(p));
    let mv = |p: &mut cadcraft_geom::Vec3| {
        if inside(p.xy()) {
            p.x += d.x;
            p.y += d.y;
        }
    };
    match &mut e.kind {
        EntityKind::Line(l) => {
            mv(&mut l.a);
            mv(&mut l.b);
        }
        EntityKind::LwPolyline(pl) => {
            for v in &mut pl.vertices {
                if inside(v.p) {
                    v.p += d;
                }
            }
        }
        EntityKind::MLine(m) => m.vertices.iter_mut().for_each(|v| mv(&mut v.p)),
        EntityKind::Arc(a) => {
            // Arcs keep their shape: move when the centre is inside.
            if inside(a.center.xy()) {
                a.center.x += d.x;
                a.center.y += d.y;
            }
        }
        EntityKind::Solid(so) | EntityKind::Trace(so) => so.corners.iter_mut().for_each(mv),
        EntityKind::Dimension(dm) => {
            for p in [&mut dm.p13, &mut dm.p14, &mut dm.defpt, &mut dm.text_mid] {
                mv(p);
            }
            dm.block = None;
        }
        k => {
            let g = k.grips();
            if g.first().is_some_and(|p| inside(*p)) {
                k.transform(&Mat3::translate(d));
            }
        }
    }
}

fn run_stretch(s: &mut Session, p: &Value) -> Result<Value> {
    let d = delta_of("stretch", p)?;
    let space = s.space();
    let listed = |key: &str, least: usize| -> Result<Option<Vec<Vec2>>> {
        let Some(pts) = points_param(p, key) else { return Ok(None) };
        if pts.len() < least || pts.len() > crate::select::MAX_MODE_POINTS || pts.iter().any(|q| !q.is_finite()) {
            return Err(bad("stretch", format!("`{key}` needs {least} to {} finite points", crate::select::MAX_MODE_POINTS)));
        }
        Ok(Some(pts))
    };
    // A fence marks no area: the objects it crosses move whole.
    if let Some(fence) = listed("fence", 2)? {
        let hs = crate::select::select_fence(s.doc()?, &space, &fence);
        let moved = transform_entities(s, &hs, &Mat3::translate(d), false)?;
        return Ok(json!({ "stretched": moved.len() }));
    }
    let (mut hs, area) = if let Some(poly) = listed("polygon", 3)? {
        (crate::select::select_polygon(s.doc()?, &space, &poly, true), StretchArea::Polygon(poly))
    } else {
        let w = points_param(p, "window").ok_or_else(|| bad("stretch", "`window` [[x,y],[x,y]], `polygon` or `fence` is required"))?;
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { return Err(bad("stretch", "window needs 2 corners")) };
        let bx = cadcraft_geom::Bounds2::new(*a, *b);
        (crate::select::select_window(s.doc()?, &space, bx, true), StretchArea::Box(bx))
    };
    add_mline_vertex_hits(s.doc()?, &space, &area, &mut hs);
    hs.retain(|h| !super::curves::is_locked(s, *h));
    let doc = s.doc_mut()?;
    let areas = [area];
    for h in &hs {
        doc.modify_entity(*h, |e| stretch_entity(e, &areas, d))?;
    }
    Ok(json!({ "stretched": hs.len() }))
}

#[derive(Clone, Copy, Default, PartialEq)]
enum StretchPhase {
    #[default]
    Select,
    Base,
    Displacement,
    Second,
}

/// STRETCH: "Select objects" repeats until Enter. Crossing windows (implied right to left, or after
/// `C`), windows (left to right, or after `W`) and polygons (`CP`, `WP`) select objects and mark
/// the area whose vertices move; objects picked one at a time (or by a fence, `F`, or
/// All/Last/Previous) move whole. Then the base point or Displacement, and the second point.
#[derive(Default)]
struct StretchM {
    phase: StretchPhase,
    objs: Vec<Handle>,
    /// Objects selected without a window: they move whole.
    whole: Vec<Handle>,
    /// The selection windows and polygons: vertices inside any of them move.
    windows: Vec<StretchArea>,
    removing: bool,
    /// `C`, `W`, `BOX`, `CP`, `WP` or `F` was typed: the next points are that kind of selection.
    mode: Option<SelectMode>,
    /// First corner of the window being drawn.
    corner: Option<Vec2>,
    base: Option<Vec2>,
}

impl StretchM {
    fn add(&mut self, s: &mut Session, hs: Vec<Handle>, whole: bool) {
        let n = hs.len();
        for h in hs {
            if self.removing {
                self.objs.retain(|o| *o != h);
                self.whole.retain(|o| *o != h);
            } else {
                if !self.objs.contains(&h) {
                    self.objs.push(h);
                }
                if whole && !self.whole.contains(&h) {
                    self.whole.push(h);
                }
            }
        }
        if n > 0 {
            s.echo(format!("{n} found, {} total", self.objs.len()));
        } else {
            s.echo("0 found");
        }
        s.set_selection(self.objs.clone());
    }

    /// Select with a typed mode that has its points. With too few (a fence of one point, a polygon
    /// of two) the mode is dropped.
    fn finish_mode(&mut self, s: &mut Session, m: SelectMode) -> Result<()> {
        let space = s.space();
        let Some(mut hs) = m.select(s.doc()?, &space) else { return Ok(()) };
        let crossing = match (m.kind, m.pts.first().zip(m.pts.get(1))) {
            (SelectKind::Crossing | SelectKind::CPolygon, _) => true,
            (SelectKind::Box, Some((a, b))) => b.x < a.x,
            _ => false,
        };
        let area = match (m.kind, m.pts.first().zip(m.pts.get(1))) {
            (SelectKind::WPolygon | SelectKind::CPolygon, _) => Some(StretchArea::Polygon(m.pts)),
            (SelectKind::Window | SelectKind::Crossing | SelectKind::Box, Some((a, b))) => {
                Some(StretchArea::Box(cadcraft_geom::Bounds2::new(*a, *b)))
            }
            // A fence marks no area: what it crosses moves whole.
            _ => None,
        };
        let whole = area.is_none();
        if let (Some(a), true, false) = (&area, crossing, self.removing) {
            add_mline_vertex_hits(s.doc()?, &space, a, &mut hs);
        }
        if let (Some(a), false) = (area, self.removing) {
            self.windows.push(a);
        }
        self.add(s, hs, whole);
        Ok(())
    }

    fn select(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(mut m) = self.mode.take() {
            match &i {
                Input::Point(p) => {
                    if m.push(*p) {
                        self.finish_mode(s, m)?;
                    } else {
                        self.mode = Some(m);
                    }
                    return Ok(Step::Continue);
                }
                Input::Text(t) | Input::Keyword(t) if matches!(t.trim().to_ascii_lowercase().as_str(), "u" | "undo") => {
                    m.pts.pop();
                    self.mode = Some(m);
                    return Ok(Step::Continue);
                }
                // Enter ends a fence or closes a polygon.
                Input::Enter => {
                    self.finish_mode(s, m)?;
                    return Ok(Step::Continue);
                }
                // Anything else leaves the mode and is read as usual.
                _ => {}
            }
        }
        match i {
            Input::Point(p) => match self.corner.take() {
                Some(c) => {
                    let bx = cadcraft_geom::Bounds2::new(c, p);
                    let space = s.space();
                    let mut hs = crate::select::select_window(s.doc()?, &space, bx, p.x < c.x);
                    if !self.removing {
                        if p.x < c.x {
                            add_mline_vertex_hits(s.doc()?, &space, &StretchArea::Box(bx), &mut hs);
                        }
                        self.windows.push(StretchArea::Box(bx));
                    }
                    self.add(s, hs, false);
                }
                None => {
                    let aperture = s.pixel_size() * s.settings.pickbox.max(1.0) * 1.5;
                    let space = s.space();
                    match crate::select::pick(s.doc()?, &space, p, aperture) {
                        Some(h) => self.add(s, vec![h], true),
                        None => self.corner = Some(p),
                    }
                }
            },
            Input::Pick(hs) => self.add(s, hs, true),
            Input::Text(t) | Input::Keyword(t) => {
                self.corner = None;
                let d = s.doc()?;
                let store = d.space(&s.space());
                let word = t.trim().to_ascii_lowercase();
                // `_C`, `_L`: the English name, as scripts write it.
                let word = word.strip_prefix('_').unwrap_or(&word).to_string();
                if let Some(m) = SelectMode::from_keyword(&word).filter(|m| m.kind != SelectKind::Group) {
                    self.mode = Some(m);
                    return Ok(Step::Continue);
                }
                let picked = match word.as_str() {
                    "r" | "remove" => {
                        self.removing = true;
                        None
                    }
                    "a" | "add" => {
                        self.removing = false;
                        None
                    }
                    "all" => Some(store.map(|st| st.iter().filter(|e| d.is_visible(e)).map(|e| e.handle).collect()).unwrap_or_default()),
                    "l" | "last" => Some(store.and_then(|st| st.last()).map(|e| vec![e.handle]).unwrap_or_default()),
                    "p" | "previous" => Some(s.state()?.previous_selection.clone()),
                    _ => {
                        s.echo("*Invalid selection*");
                        None
                    }
                };
                if let Some(hs) = picked {
                    self.add(s, hs, true);
                }
            }
            Input::Enter if self.corner.is_some() => self.corner = None,
            Input::Enter if self.objs.is_empty() => return Ok(Step::Done),
            Input::Enter => {
                s.remember_selection(&self.objs);
                self.phase = StretchPhase::Base;
            }
            _ => {}
        }
        Ok(Step::Continue)
    }

    fn apply(&self, s: &mut Session, d: Vec2) -> Result<Step> {
        let whole: Vec<Handle> = self.objs.iter().filter(|h| self.whole.contains(h)).copied().collect();
        transform_entities(s, &whole, &Mat3::translate(d), false)?;
        let doc = s.doc_mut()?;
        for h in self.objs.iter().filter(|h| !self.whole.contains(h)) {
            let locked = doc.entity(*h).and_then(|e| doc.layer(&e.common.layer)).is_some_and(|l| l.locked);
            if !locked {
                doc.modify_entity(*h, |e| stretch_entity(e, &self.windows, d))?;
            }
        }
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
}

impl Interactive for StretchM {
    fn name(&self) -> &'static str {
        "STRETCH"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        s.echo("Select objects to stretch by crossing-window or crossing-polygon...");
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let (StretchPhase::Select, Some(m)) = (self.phase, &self.mode) {
            return m.prompt();
        }
        match (self.phase, self.corner) {
            (StretchPhase::Select, Some(c)) => Prompt::new("Specify opposite corner", Accept::POINT).corner(c),
            (StretchPhase::Select, None) => Prompt::new(if self.removing { "Remove objects" } else { "Select objects" }, Accept::POINT),
            (StretchPhase::Base, _) => Prompt::new("Specify base point", Accept::POINT).kw(&["Displacement"]).default("Displacement"),
            (StretchPhase::Displacement, _) => Prompt::new("Specify displacement", Accept::POINT).default("0,0"),
            (StretchPhase::Second, _) => Prompt::new("Specify second point or <use first point as displacement>", Accept::POINT).base_opt(self.base),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.phase, i) {
            (StretchPhase::Select, i) => self.select(s, i),
            (StretchPhase::Base, Input::Point(p)) => {
                self.base = Some(p);
                self.phase = StretchPhase::Second;
                Ok(Step::Continue)
            }
            (StretchPhase::Base, Input::Enter) => {
                self.phase = StretchPhase::Displacement;
                Ok(Step::Continue)
            }
            (StretchPhase::Base, Input::Keyword(k)) if k == "Displacement" => {
                self.phase = StretchPhase::Displacement;
                Ok(Step::Continue)
            }
            (StretchPhase::Displacement, Input::Point(p)) => self.apply(s, p),
            (StretchPhase::Displacement, Input::Enter) => Ok(Step::Done),
            (StretchPhase::Second, Input::Point(p)) => self.apply(s, p - self.base.unwrap_or(p)),
            (StretchPhase::Second, Input::Enter) => self.apply(s, self.base.unwrap_or(Vec2::ZERO)),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        let rect = |a: Vec2| {
            let (b, d) = (Vec2::new(c.x, a.y), Vec2::new(a.x, c.y));
            vec![line(a, b), line(b, c), line(c, d), line(d, a)]
        };
        if let (StretchPhase::Select, Some(m)) = (self.phase, &self.mode) {
            let Some(first) = m.pts.first().copied() else { return Vec::new() };
            if !matches!(m.kind, SelectKind::Fence | SelectKind::WPolygon | SelectKind::CPolygon) {
                return rect(first);
            }
            // The fence or polygon so far, on to the cursor; a polygon closes back to its start.
            let mut out: Vec<EntityKind> = m.pts.windows(2).filter_map(|w| Some(line(*w.first()?, *w.get(1)?))).collect();
            out.extend(m.pts.last().map(|l| line(*l, c)));
            if m.kind != SelectKind::Fence && m.pts.len() >= 2 {
                out.push(line(c, first));
            }
            return out;
        }
        match (self.phase, self.corner, self.base) {
            (StretchPhase::Select, Some(a), _) => rect(a),
            (StretchPhase::Second, _, Some(base)) => {
                let Ok(doc) = s.doc() else { return Vec::new() };
                let m = Mat3::translate(c - base);
                let mut out: Vec<EntityKind> = self
                    .objs
                    .iter()
                    .take(500)
                    .filter_map(|h| doc.entity(*h))
                    .map(|e| {
                        let mut e = (**e).clone();
                        if self.whole.contains(&e.handle) {
                            e.kind.transform(&m);
                        } else {
                            stretch_entity(&mut e, &self.windows, c - base);
                        }
                        e.kind
                    })
                    .collect();
                out.push(line(base, c));
                out
            }
            _ => Vec::new(),
        }
    }
}

/// Whether OFFSET can work on this kind of object at all.
fn offsettable(k: &EntityKind) -> bool {
    matches!(
        k,
        EntityKind::Line(_)
            | EntityKind::Circle(_)
            | EntityKind::Arc(_)
            | EntityKind::Ellipse(_)
            | EntityKind::LwPolyline(_)
            | EntityKind::Spline(_)
            | EntityKind::XLine(_)
            | EntityKind::Ray(_)
    )
}

/// Offset an entity by `dist` towards `side`. Returns the new geometry.
pub(crate) fn offset_kind(k: &EntityKind, dist: f64, side: Vec2) -> Option<EntityKind> {
    match k {
        EntityKind::Line(l) => {
            let ln = Line::new(l.a.xy(), l.b.xy());
            let sgn = if ln.side(side) >= 0.0 { 1.0 } else { -1.0 };
            let o = ln.offset(dist * sgn);
            Some(line(o.a, o.b))
        }
        EntityKind::Circle(c) => {
            let inside = side.dist(c.center.xy()) < c.radius;
            let r = if inside { c.radius - dist } else { c.radius + dist };
            (r > 1e-12).then(|| circle(c.center.xy(), r))
        }
        EntityKind::Arc(a) => {
            let inside = side.dist(a.center.xy()) < a.radius;
            let r = if inside { a.radius - dist } else { a.radius + dist };
            (r > 1e-12).then(|| arc(&Arc::new(a.center.xy(), r, a.start, a.end)))
        }
        EntityKind::Ellipse(e) => {
            // Approximate with a polyline offset of the tessellation.
            let ge = cadcraft_geom::Ellipse { center: e.center.xy(), major: e.major.xy(), ratio: e.ratio, start: e.start, end: e.end };
            // Inside = on the centre side of the full ellipse, even for an open arc.
            let inside = {
                let a = e.major.xy().len();
                let b = a * e.ratio.abs();
                let u = e.major.xy().normalized();
                let d = side - e.center.xy();
                let (x, y) = (d.dot(u) * b, d.dot(u.perp()) * a);
                x * x + y * y < (a * b) * (a * b)
            };
            // offset_points shifts along the left normal, which points into a counter-clockwise ellipse.
            let mut pts = Vec::new();
            ge.tessellate(e.major.xy().len() * 1e-3, &mut pts);
            let off = offset_points(&pts, if inside { dist } else { -dist }, ge.is_full());
            Some(lwpoly(off.into_iter().map(PolyVertex::new).collect(), ge.is_full()))
        }
        EntityKind::LwPolyline(p) => {
            let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
            // Side: compare with the nearest segment.
            let segs = pl.segments();
            let nearest = segs.iter().min_by(|a, b| a.closest(side).dist(side).total_cmp(&b.closest(side).dist(side)))?;
            let c = nearest.closest(side);
            let t = nearest.tangent(0.5);
            let left = t.cross(side - c) > 0.0;
            let d = if left { dist } else { -dist };
            offset_polyline(&pl, d).map(|vs| {
                let mut k = lwpoly(vs, p.closed);
                if let EntityKind::LwPolyline(n) = &mut k {
                    n.const_width = p.const_width;
                }
                k
            })
        }
        EntityKind::Spline(sp) => {
            let pts = sp.tessellate(1e-3);
            let n = pts.len();
            let mid = pts.get(n / 2).copied()?;
            let next = pts.get((n / 2 + 1).min(n.saturating_sub(1))).copied()?;
            let left = (next - mid).cross(side - mid) > 0.0;
            let off = offset_points(&pts, if left { dist } else { -dist }, sp.closed);
            // A closed spline stays closed (periodic), smooth across its start point.
            Some(EntityKind::Spline(cadcraft_geom::Spline::from_fit(&decimate(&off, 40), sp.closed)))
        }
        EntityKind::XLine(r) | EntityKind::Ray(r) => {
            let ln = Line::new(r.base.xy(), r.base.xy() + r.dir.xy());
            let sgn = if ln.side(side) >= 0.0 { 1.0 } else { -1.0 };
            let o = ln.offset(dist * sgn);
            let rl = cadcraft_doc::RayLine { base: v3(o.a), dir: r.dir };
            Some(if matches!(k, EntityKind::XLine(_)) { EntityKind::XLine(rl) } else { EntityKind::Ray(rl) })
        }
        _ => None,
    }
}

fn decimate(pts: &[Vec2], max: usize) -> Vec<Vec2> {
    if pts.len() <= max {
        return pts.to_vec();
    }
    let step = pts.len() as f64 / max as f64;
    let mut v: Vec<Vec2> = (0..max).filter_map(|i| pts.get((i as f64 * step) as usize).copied()).collect();
    if let Some(l) = pts.last() {
        v.push(*l);
    }
    v
}

fn offset_points(pts: &[Vec2], d: f64, closed: bool) -> Vec<Vec2> {
    let n = pts.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let p = pts.get(i).copied().unwrap_or_default();
        let prev = if i > 0 {
            pts.get(i - 1)
        } else if closed {
            pts.get(n.saturating_sub(2))
        } else {
            None
        };
        let next = if i + 1 < n {
            pts.get(i + 1)
        } else if closed {
            pts.get(1)
        } else {
            None
        };
        let t = match (prev, next) {
            (Some(a), Some(b)) => (*b - *a).normalized(),
            (None, Some(b)) => (*b - p).normalized(),
            (Some(a), None) => (p - *a).normalized(),
            _ => Vec2::X,
        };
        out.push(p + t.perp() * d);
    }
    out
}

/// Offset a polyline: offset each segment, then join neighbours at their intersections.
pub(crate) fn offset_polyline(pl: &Polyline, d: f64) -> Option<Vec<PolyVertex>> {
    let segs: Vec<Segment> = pl.segments().iter().filter_map(|s| s.offset(d)).collect();
    if segs.is_empty() {
        return None;
    }
    let n = segs.len();
    // Where each offset segment starts after joining it to the previous one.
    let mut starts: Vec<Vec2> = Vec::with_capacity(n);
    for i in 0..n {
        let s = segs.get(i)?;
        starts.push(if i == 0 && !pl.closed {
            s.start()
        } else {
            let prev = segs.get((i + n - 1) % n)?;
            join_point(prev, s).unwrap_or(s.start())
        });
    }
    let mut verts: Vec<PolyVertex> = Vec::new();
    for i in 0..n {
        let s = segs.get(i)?;
        let start = *starts.get(i)?;
        let end = if i + 1 < n {
            *starts.get(i + 1)?
        } else if pl.closed {
            *starts.first()?
        } else {
            s.end()
        };
        let bulge = match s {
            Segment::Line(_) => 0.0,
            Segment::Arc { arc, ccw } => {
                // The joined endpoints may have moved along the circle: take the sweep between them.
                let sweep = if start.near(end, 1e-9) {
                    arc.sweep()
                } else if *ccw {
                    ccw_sweep(arc.center.angle_to(start), arc.center.angle_to(end))
                } else {
                    ccw_sweep(arc.center.angle_to(end), arc.center.angle_to(start))
                };
                cadcraft_geom::arc_to_bulge(if *ccw { sweep } else { -sweep })
            }
        };
        verts.push(PolyVertex { p: start, bulge, start_width: 0.0, end_width: 0.0 });
    }
    if !pl.closed {
        verts.push(PolyVertex::new(segs.last()?.end()));
    }
    Some(verts)
}

fn join_point(a: &Segment, b: &Segment) -> Option<Vec2> {
    if a.end().near(b.start(), 1e-9) {
        return Some(b.start());
    }
    let hits = intersect_ext(a, b, true);
    hits.into_iter().min_by(|x, y| x.dist(a.end()).total_cmp(&y.dist(a.end())))
}

fn run_offset(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("offset", "`handle` is required"))?;
    let dist = f64_req("offset", p, "distance")?;
    let side = point_req("offset", p, "side")?;
    let e = s.doc()?.entity(h).map(|e| (**e).clone()).ok_or_else(|| bad("offset", "no such object"))?;
    let k = offset_kind(&e.kind, dist.abs(), side).ok_or_else(|| bad("offset", "cannot offset that object"))?;
    let current_layer = match str_param(p, "layer").map(str::to_ascii_lowercase).as_deref() {
        None | Some("source") => false,
        Some("current") => true,
        Some(_) => return Err(bad("offset", "`layer` is \"source\" or \"current\"")),
    };
    let nh = add_offset(s, &e, k, current_layer, bool_or(p, "erase", false))?;
    Ok(json!({ "handle": nh.hex() }))
}

/// Add the offset copy `k` of `src` (on the current layer instead of the source's when
/// `current_layer`), then erase the source when `erase` and its layer is not locked.
fn add_offset(s: &mut Session, src: &Entity, k: EntityKind, current_layer: bool, erase: bool) -> Result<Handle> {
    let space = s.space();
    let d = s.doc_mut()?;
    let mut common = src.common.clone();
    if current_layer {
        common.layer = d.header.str("CLAYER", "0");
    }
    let nh = d.new_handle();
    if let Some(st) = d.space_mut(&space) {
        st.push(Entity { handle: nh, common, kind: k });
    }
    let locked = d.layer(&src.common.layer).is_some_and(|l| l.locked);
    if erase && !locked {
        d.remove_entity(src.handle);
    }
    Ok(nh)
}

// ---------------- trim / extend ----------------

/// The cutting (TRIM) or boundary (EXTEND) edges.
#[derive(Clone, Copy, Default)]
pub(crate) struct Edges<'a> {
    /// Only these objects; `None`: every visible object.
    pub only: Option<&'a [Handle]>,
    /// Implied edges (EDGEMODE 1): lines and the end segments of open polylines count as running
    /// on without end, arcs as whole circles.
    pub implied: bool,
}

/// All primitives of the cutting edges (excluding `exclude`).
fn edge_segments(s: &Session, edges: Edges, exclude: Handle) -> Result<Vec<Segment>> {
    let d = s.doc()?;
    let space = s.space();
    let store = d.space(&space).ok_or(EngineError::NoDocument)?;
    let mut out = Vec::new();
    for e in store.iter() {
        if e.handle == exclude || !d.is_visible(e) {
            continue;
        }
        if let Some(es) = edges.only
            && !es.contains(&e.handle)
        {
            continue;
        }
        if edges.implied
            && let Some(segs) = implied_edge(&e.kind)
        {
            out.extend(segs);
            continue;
        }
        for p in e.kind.prims() {
            out.extend(prim_to_segments(&p));
        }
    }
    Ok(out)
}

/// `k` as an implied edge (EDGEMODE 1), for lines, arcs and open polylines.
fn implied_edge(k: &EntityKind) -> Option<Vec<Segment>> {
    const FAR: f64 = 1e8;
    let whole = |a: &Arc| Segment::Arc { arc: Arc { start: 0.0, end: TAU - 1e-12, ..*a }, ccw: true };
    // Run a segment on past its start and/or its end.
    let run_on = |sg: Segment, back: bool, fwd: bool| match sg {
        Segment::Line(l) => {
            let d = (l.b - l.a).normalized();
            Segment::Line(Line::new(if back { l.a - d * FAR } else { l.a }, if fwd { l.b + d * FAR } else { l.b }))
        }
        Segment::Arc { arc, .. } => whole(&arc),
    };
    match k {
        EntityKind::Line(l) => Some(vec![run_on(Segment::Line(Line::new(l.a.xy(), l.b.xy())), true, true)]),
        EntityKind::Arc(a) => Some(vec![whole(&Arc::new(a.center.xy(), a.radius, a.start, a.end))]),
        EntityKind::LwPolyline(p) if !p.closed => {
            let mut segs = Polyline { vertices: p.vertices.clone(), closed: false }.segments();
            let last = segs.len().checked_sub(1)?;
            for (i, sg) in segs.iter_mut().enumerate() {
                if i == 0 || i == last {
                    *sg = run_on(*sg, i == 0, i == last);
                }
            }
            Some(segs)
        }
        _ => None,
    }
}

fn prim_to_segments(p: &Prim) -> Vec<Segment> {
    match p {
        Prim::Seg(s) => vec![*s],
        Prim::Circle(c) => vec![Segment::Arc { arc: Arc { center: c.center, radius: c.radius, start: 0.0, end: TAU - 1e-12 }, ccw: true }],
        Prim::Infinite { base, dir, .. } => vec![Segment::Line(Line::new(*base - *dir * 1e8, *base + *dir * 1e8))],
        Prim::Ellipse(e) => {
            let mut pts = Vec::new();
            e.tessellate(e.major.len() * 1e-4, &mut pts);
            pts.windows(2).filter_map(|w| Some(Segment::Line(Line::new(*w.first()?, *w.get(1)?)))).collect()
        }
        Prim::Spline(sp) => sp.tessellate(1e-4).windows(2).filter_map(|w| Some(Segment::Line(Line::new(*w.first()?, *w.get(1)?)))).collect(),
        Prim::Fill(f) => {
            let n = f.len();
            (0..n).filter_map(|i| Some(Segment::Line(Line::new(*f.get(i)?, *f.get((i + 1) % n)?)))).collect()
        }
        Prim::Point(_) => Vec::new(),
    }
}

/// Trim `h` at the piece containing `pick`. Returns the handles that replace it.
pub(crate) fn trim(s: &mut Session, h: Handle, pick: Vec2, edges: Edges) -> Result<Vec<Handle>> {
    if super::curves::is_locked(s, h) {
        return Err(EngineError::Other("The object is on a locked layer.".into()));
    }
    let e = s.doc()?.entity(h).map(|e| (**e).clone()).ok_or_else(|| EngineError::Other("no such object".into()))?;
    let cut = edge_segments(s, edges, h)?;
    let pieces: Vec<EntityKind> = match &e.kind {
        EntityKind::Line(l) => {
            let ln = Line::new(l.a.xy(), l.b.xy());
            let mut ts: Vec<f64> = cut
                .iter()
                .flat_map(|c| intersect_ext(&Segment::Line(ln), c, false))
                .map(|x| ln.param_of(x))
                .filter(|t| *t > 1e-9 && *t < 1.0 - 1e-9)
                .collect();
            ts.sort_by(f64::total_cmp);
            let tp = ln.param_of(pick).clamp(0.0, 1.0);
            let lo = ts.iter().copied().filter(|t| *t < tp).fold(0.0, f64::max);
            let hi = ts.iter().copied().filter(|t| *t > tp).fold(1.0, f64::min);
            if lo == 0.0 && hi == 1.0 {
                return Err(EngineError::Other("Object does not intersect a cutting edge.".into()));
            }
            let mut v = Vec::new();
            if lo > 0.0 {
                v.push(line(ln.a, ln.at(lo)));
            }
            if hi < 1.0 {
                v.push(line(ln.at(hi), ln.b));
            }
            v
        }
        EntityKind::Arc(a) => {
            let ga = Arc::new(a.center.xy(), a.radius, a.start, a.end);
            let seg = Segment::Arc { arc: ga, ccw: true };
            let sweep = ga.sweep();
            let param = |p: Vec2| norm_angle(ga.center.angle_to(p) - ga.start);
            let mut ts: Vec<f64> =
                cut.iter().flat_map(|c| intersect_ext(&seg, c, false)).map(param).filter(|t| *t > 1e-9 && *t < sweep - 1e-9).collect();
            ts.sort_by(f64::total_cmp);
            let tp = param(pick).min(sweep);
            let lo = ts.iter().copied().filter(|t| *t < tp).fold(0.0, f64::max);
            let hi = ts.iter().copied().filter(|t| *t > tp).fold(sweep, f64::min);
            if lo == 0.0 && hi == sweep {
                return Err(EngineError::Other("Object does not intersect a cutting edge.".into()));
            }
            let mut v = Vec::new();
            if lo > 0.0 {
                v.push(arc(&Arc::new(ga.center, ga.radius, ga.start, ga.start + lo)));
            }
            if hi < sweep {
                v.push(arc(&Arc::new(ga.center, ga.radius, ga.start + hi, ga.end)));
            }
            v
        }
        EntityKind::Circle(c) => {
            let full = Segment::Arc { arc: Arc { center: c.center.xy(), radius: c.radius, start: 0.0, end: TAU - 1e-12 }, ccw: true };
            let mut angs: Vec<f64> = cut.iter().flat_map(|x| intersect_ext(&full, x, false)).map(|p| c.center.xy().angle_to(p)).collect();
            angs.sort_by(f64::total_cmp);
            angs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            if angs.len() < 2 {
                return Err(EngineError::Other("Circle must intersect cutting edges at two or more points.".into()));
            }
            let ap = c.center.xy().angle_to(pick);
            // The removed arc runs between the two intersections around the pick.
            let n = angs.len();
            let mut keep = None;
            for i in 0..n {
                let a0 = angs[i];
                let a1 = angs[(i + 1) % n];
                if angle_in_sweep(ap, a0, a1) {
                    keep = Some(arc(&Arc::new(c.center.xy(), c.radius, a1, a0)));
                }
            }
            keep.into_iter().collect()
        }
        EntityKind::LwPolyline(p) => trim_polyline(p, &cut, pick)?,
        k => super::modify2::trim_other(k, &cut, pick)?,
    };
    let common = e.common.clone();
    let space = s.space();
    let d = s.doc_mut()?;
    d.remove_entity(h);
    let mut out = Vec::new();
    for k in pieces {
        let nh = d.new_handle();
        if let Some(st) = d.space_mut(&space) {
            st.push(Entity { handle: nh, common: common.clone(), kind: k });
            out.push(nh);
        }
    }
    Ok(out)
}

/// Sub-polyline between length parameters `a` and `b` (open polylines).
fn sub_polyline(segs: &[Segment], a: f64, b: f64) -> Vec<PolyVertex> {
    let mut out: Vec<PolyVertex> = Vec::new();
    let mut acc = 0.0;
    for s in segs {
        let len = s.len();
        let s0 = acc;
        let s1 = acc + len;
        acc = s1;
        if s1 <= a + 1e-12 || s0 >= b - 1e-12 || len <= 1e-15 {
            continue;
        }
        let t0 = ((a - s0) / len).clamp(0.0, 1.0);
        let t1 = ((b - s0) / len).clamp(0.0, 1.0);
        let p0 = s.at(t0);
        let bulge = match s {
            Segment::Line(_) => 0.0,
            Segment::Arc { arc, ccw } => {
                let sw = arc.sweep() * (t1 - t0);
                cadcraft_geom::arc_to_bulge(if *ccw { sw } else { -sw })
            }
        };
        if out.last().is_none_or(|l| !l.p.near(p0, 1e-9)) {
            out.push(PolyVertex { p: p0, bulge, ..Default::default() });
        } else if let Some(l) = out.last_mut() {
            l.bulge = bulge;
        }
        out.push(PolyVertex::new(s.at(t1)));
    }
    out
}

fn trim_polyline(p: &cadcraft_doc::LwPolyline, cut: &[Segment], pick: Vec2) -> Result<Vec<EntityKind>> {
    let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
    let segs = pl.segments();
    let total: f64 = segs.iter().map(Segment::len).sum();
    let mut ts: Vec<f64> = Vec::new();
    let mut acc = 0.0;
    let mut tp = 0.0;
    let mut best = f64::INFINITY;
    for s in &segs {
        let len = s.len();
        for x in cut.iter().flat_map(|c| intersect_ext(s, c, false)) {
            ts.push(acc + param_on(s, x) * len);
        }
        let c = s.closest(pick);
        if c.dist(pick) < best {
            best = c.dist(pick);
            tp = acc + param_on(s, c) * len;
        }
        acc += len;
    }
    ts.retain(|t| *t > 1e-9 && *t < total - 1e-9);
    ts.sort_by(f64::total_cmp);
    if ts.is_empty() {
        return Err(EngineError::Other("Object does not intersect a cutting edge.".into()));
    }
    let lo = ts.iter().copied().filter(|t| *t < tp).fold(f64::NEG_INFINITY, f64::max);
    let hi = ts.iter().copied().filter(|t| *t > tp).fold(f64::INFINITY, f64::min);
    let mk = |vs: Vec<PolyVertex>| lwpoly(vs, false);
    let mut out = Vec::new();
    if p.closed {
        // Keep the complementary run from hi around to lo.
        // The pick lies before the first cut or after the last: the piece wraps past the start
        // vertex, so the run to keep is the stretch between the first and the last cut.
        let (lo, hi) = (
            if lo.is_finite() { lo } else { ts.last().copied().unwrap_or(0.0) - total },
            if hi.is_finite() { hi } else { ts.first().copied().unwrap_or(total) + total },
        );
        let doubled: Vec<Segment> = segs.iter().chain(segs.iter()).copied().collect();
        let (a, mut b) = if hi > total { (hi - total, lo) } else { (hi, lo + total) };
        // A single cut leaves the whole outline, opened there.
        if b <= a + 1e-9 {
            b += total;
        }
        out.push(mk(sub_polyline(&doubled, a, b)));
    } else {
        if lo.is_finite() {
            out.push(mk(sub_polyline(&segs, 0.0, lo)));
        }
        if hi.is_finite() {
            out.push(mk(sub_polyline(&segs, hi, total)));
        }
    }
    Ok(out.into_iter().filter(|k| matches!(k, EntityKind::LwPolyline(l) if l.vertices.len() >= 2)).collect())
}

fn param_on(s: &Segment, p: Vec2) -> f64 {
    match s {
        Segment::Line(l) => l.param_of(p).clamp(0.0, 1.0),
        Segment::Arc { arc, ccw } => {
            let t = norm_angle(arc.center.angle_to(p) - arc.start) / arc.sweep().max(1e-15);
            let t = t.clamp(0.0, 1.0);
            if *ccw { t } else { 1.0 - t }
        }
    }
}

/// Extend the end of `h` nearest `pick` to the nearest boundary edge.
pub(crate) fn extend(s: &mut Session, h: Handle, pick: Vec2, edges: Edges) -> Result<()> {
    if super::curves::is_locked(s, h) {
        return Err(EngineError::Other("The object is on a locked layer.".into()));
    }
    let e = s.doc()?.entity(h).map(|e| (**e).clone()).ok_or_else(|| EngineError::Other("no such object".into()))?;
    let cut = edge_segments(s, edges, h)?;
    let new_kind = match &e.kind {
        EntityKind::Line(l) => {
            let (a, b) = (l.a.xy(), l.b.xy());
            let from_b = pick.dist(b) < pick.dist(a);
            let (fixed, end) = if from_b { (a, b) } else { (b, a) };
            let dir = (end - fixed).normalized();
            let ray = Segment::Line(Line::new(end, end + dir * 1e8));
            let best = cut
                .iter()
                .flat_map(|c| intersect_ext(&ray, c, false))
                .filter(|x| (*x - end).dot(dir) > 1e-9)
                .min_by(|x, y| x.dist(end).total_cmp(&y.dist(end)));
            let np = best.ok_or_else(|| EngineError::Other("Object does not intersect an edge.".into()))?;
            if from_b { line(a, np) } else { line(np, b) }
        }
        EntityKind::Arc(a) => {
            let ga = Arc::new(a.center.xy(), a.radius, a.start, a.end);
            let from_end = pick.dist(ga.end_point()) < pick.dist(ga.start_point());
            let full = Segment::Arc { arc: Arc { start: 0.0, end: TAU - 1e-12, ..ga }, ccw: true };
            let hits: Vec<f64> =
                cut.iter().flat_map(|c| intersect_ext(&full, c, false)).map(|x| ga.center.angle_to(x)).filter(|t| !ga.contains_angle(*t)).collect();
            let pick_ang = if from_end {
                hits.iter().copied().min_by(|x, y| ccw_sweep(ga.end, *x).total_cmp(&ccw_sweep(ga.end, *y)))
            } else {
                hits.iter().copied().min_by(|x, y| ccw_sweep(*x, ga.start).total_cmp(&ccw_sweep(*y, ga.start)))
            };
            let t = pick_ang.ok_or_else(|| EngineError::Other("Object does not intersect an edge.".into()))?;
            if from_end { arc(&Arc::new(ga.center, ga.radius, ga.start, t)) } else { arc(&Arc::new(ga.center, ga.radius, t, ga.end)) }
        }
        EntityKind::LwPolyline(p) if !p.closed && p.vertices.len() >= 2 => {
            let mut vs = p.vertices.clone();
            let n = vs.len();
            let from_end = vs.last().is_some_and(|l| pick.dist(l.p) < vs.first().map(|f| pick.dist(f.p)).unwrap_or(f64::INFINITY));
            let (i_end, i_prev) = if from_end { (n - 1, n - 2) } else { (0, 1) };
            let end = vs.get(i_end).map(|v| v.p).unwrap_or_default();
            let prev = vs.get(i_prev).map(|v| v.p).unwrap_or_default();
            let dir = (end - prev).normalized();
            let ray = Segment::Line(Line::new(end, end + dir * 1e8));
            let np = cut
                .iter()
                .flat_map(|c| intersect_ext(&ray, c, false))
                .filter(|x| (*x - end).dot(dir) > 1e-9)
                .min_by(|x, y| x.dist(end).total_cmp(&y.dist(end)))
                .ok_or_else(|| EngineError::Other("Object does not intersect an edge.".into()))?;
            if let Some(v) = vs.get_mut(i_end) {
                v.p = np;
            }
            lwpoly(vs, false)
        }
        k => super::modify2::extend_other(k, &cut, pick)?,
    };
    s.doc_mut()?.modify_entity(h, |e| e.kind = new_kind)?;
    Ok(())
}

// ---------------- fillet / chamfer ----------------

/// What CHAMFER takes (as in AutoCAD, not arcs, circles, ellipses or splines).
const CHAMFER_LINES_ONLY: &str =
    "CHAMFER works on lines (and polylines with the Polyline option); it cannot chamfer arcs, circles, ellipses or splines.";

fn as_line(e: &Entity) -> Option<Line> {
    match &e.kind {
        EntityKind::Line(l) => Some(Line::new(l.a.xy(), l.b.xy())),
        _ => None,
    }
}

/// Fillet (or chamfer) two lines. `p1`/`p2` mark the kept side of each line.
pub(crate) fn fillet_lines(
    s: &mut Session,
    h1: Handle,
    p1: Vec2,
    h2: Handle,
    p2: Vec2,
    radius: f64,
    chamfer: Option<(f64, f64)>,
) -> Result<Option<Handle>> {
    if super::curves::is_locked(s, h1) || super::curves::is_locked(s, h2) {
        return Err(EngineError::Other("The object is on a locked layer.".into()));
    }
    let d = s.doc()?;
    let e1 = d.entity(h1).map(|e| (**e).clone()).ok_or_else(|| EngineError::Other("no such object".into()))?;
    let e2 = d.entity(h2).map(|e| (**e).clone()).ok_or_else(|| EngineError::Other("no such object".into()))?;
    let (Some(l1), Some(l2)) = (as_line(&e1), as_line(&e2)) else {
        if chamfer.is_none() {
            return super::modify2::fillet_curves(s, h1, p1, h2, p2, radius);
        }
        return Err(EngineError::Other(CHAMFER_LINES_ONLY.into()));
    };
    let (x, _, _) = line_line_infinite(l1.a, l1.b, l2.a, l2.b).ok_or_else(|| EngineError::Other("Lines are parallel.".into()))?;
    // Keep the far end on the picked side of the corner.
    let keep = |l: &Line, p: Vec2| -> Vec2 {
        let ta = (l.a - x).dot(p - x);
        let tb = (l.b - x).dot(p - x);
        if ta >= tb { l.a } else { l.b }
    };
    let k1 = keep(&l1, p1);
    let k2 = keep(&l2, p2);
    let u1 = (k1 - x).normalized();
    let u2 = (k2 - x).normalized();
    let mut new_arc = None;
    let (t1, t2) = if let Some((d1, d2)) = chamfer {
        (x + u1 * d1, x + u2 * d2)
    } else if radius <= 1e-12 {
        (x, x)
    } else {
        let half = u1.dot(u2).clamp(-1.0, 1.0).acos() / 2.0;
        if half.sin().abs() < 1e-12 {
            return Err(EngineError::Other("Lines are parallel.".into()));
        }
        let dist = radius / half.tan();
        let t1 = x + u1 * dist;
        let t2 = x + u2 * dist;
        let bis = (u1 + u2).normalized();
        let c = x + bis * (radius / half.sin());
        let a1 = c.angle_to(t1);
        let a2 = c.angle_to(t2);
        let ar = if ccw_sweep(a1, a2) <= std::f64::consts::PI { Arc::new(c, radius, a1, a2) } else { Arc::new(c, radius, a2, a1) };
        new_arc = Some(ar);
        (t1, t2)
    };
    let common = e1.common.clone();
    let doc = s.doc_mut()?;
    doc.modify_entity(h1, |e| e.kind = line(k1, t1))?;
    doc.modify_entity(h2, |e| e.kind = line(k2, t2))?;
    let space = s.space();
    let doc = s.doc_mut()?;
    let extra = match (chamfer, new_arc) {
        (Some(_), _) if !t1.near(t2, 1e-12) => Some(line(t1, t2)),
        (None, Some(a)) => Some(arc(&a)),
        _ => None,
    };
    if let Some(k) = extra {
        let nh = doc.new_handle();
        if let Some(st) = doc.space_mut(&space) {
            st.push(Entity { handle: nh, common, kind: k });
        }
        return Ok(Some(nh));
    }
    Ok(None)
}

fn h_param(p: &Value, k: &str) -> Option<Handle> {
    p.get(k).and_then(|v| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle)))
}

fn run_fillet(s: &mut Session, p: &Value) -> Result<Value> {
    if bool_or(p, "polyline", false) {
        let h = h_param(p, "handle").or_else(|| h_param(p, "h1")).ok_or_else(|| bad("fillet", "`handle` (polyline) is required"))?;
        let r = f64_or(p, "radius", s.doc()?.header.f64("FILLETRAD", 0.0));
        let n = super::modify2::fillet_polyline(s, h, r)?;
        return Ok(json!({ "filleted": n }));
    }
    let h1 = h_param(p, "h1").ok_or_else(|| bad("fillet", "`h1` is required"))?;
    let h2 = h_param(p, "h2").ok_or_else(|| bad("fillet", "`h2` is required"))?;
    let r = f64_or(p, "radius", s.doc()?.header.f64("FILLETRAD", 0.0));
    let p1 = point_param(p, "p1").unwrap_or_default();
    let p2 = point_param(p, "p2").unwrap_or_default();
    let a = fillet_corner(s, h1, p1, h2, p2, r, None, bool_or(p, "trim", true))?;
    Ok(json!({ "arc": a.map(|h| h.hex()) }))
}

fn run_chamfer(s: &mut Session, p: &Value) -> Result<Value> {
    let cut = match (p.get("length").and_then(Value::as_f64), p.get("angle").and_then(Value::as_f64)) {
        (Some(len), Some(deg)) if len.is_finite() && len >= 0.0 && deg.is_finite() && (0.0..180.0).contains(&deg) => {
            ChamferCut::Angle(len, deg.to_radians())
        }
        (None, None) => {
            let d1 = f64_or(p, "d1", s.doc()?.header.f64("CHAMFERA", 0.0));
            ChamferCut::Dist(d1, f64_or(p, "d2", d1))
        }
        _ => return Err(bad("chamfer", "`length` (>= 0) and `angle` (degrees, 0 to 180) go together")),
    };
    if bool_or(p, "polyline", false) {
        let h = h_param(p, "handle").or_else(|| h_param(p, "h1")).ok_or_else(|| bad("chamfer", "`handle` (polyline) is required"))?;
        return Ok(json!({ "chamfered": chamfer_polyline(s, h, cut)? }));
    }
    let h1 = h_param(p, "h1").ok_or_else(|| bad("chamfer", "`h1` is required"))?;
    let h2 = h_param(p, "h2").ok_or_else(|| bad("chamfer", "`h2` is required"))?;
    let p1 = point_param(p, "p1").unwrap_or_default();
    let p2 = point_param(p, "p2").unwrap_or_default();
    let a = fillet_corner(s, h1, p1, h2, p2, 0.0, Some(cut), bool_or(p, "trim", true))?;
    Ok(json!({ "line": a.map(|h| h.hex()) }))
}

// ---------------- explode / array / order / break / join ----------------

pub(crate) fn explode_kind(d: &cadcraft_doc::Drawing, e: &Entity) -> Option<Vec<Entity>> {
    // An annotative object explodes into plain objects as it draws at the current annotation scale.
    if cadcraft_doc::is_annotative(e) {
        let mut se = match d.anno_draw(e) {
            cadcraft_doc::AnnoDraw::Scaled(f) => cadcraft_doc::anno_scaled(e, f),
            _ => e.clone(),
        };
        se.common.anno_scales.clear();
        return explode_kind(d, &se);
    }
    match &e.kind {
        EntityKind::LwPolyline(p) => {
            let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
            Some(
                pl.segments()
                    .into_iter()
                    .map(|s| {
                        let k = match s {
                            Segment::Line(l) => line(l.a, l.b),
                            Segment::Arc { arc: a, .. } => arc(&a),
                        };
                        Entity { handle: Handle(0), common: e.common.clone(), kind: k }
                    })
                    .collect(),
            )
        }
        EntityKind::Insert(ins) => {
            if let Some(items) = super::array::explode_array(d, e) {
                return Some(items);
            }
            let blk = d.block(&ins.block)?;
            let m = ins.transform(blk.base.xy());
            Some(
                blk.entities
                    .iter()
                    .map(|be| {
                        let mut ne = (**be).clone();
                        ne.kind.transform(&m);
                        if ne.common.layer == "0" {
                            ne.common.layer = e.common.layer.clone();
                        }
                        if ne.common.color == cadcraft_color::Color::ByBlock {
                            ne.common.color = e.common.color;
                        }
                        ne
                    })
                    .collect(),
            )
        }
        // A helix explodes into its spline (seen from above).
        EntityKind::Helix(h) => Some(vec![Entity { handle: Handle(0), common: e.common.clone(), kind: EntityKind::Spline(h.spline()) }]),
        EntityKind::Polyline3d(p) => Some(
            p.points
                .windows(2)
                .filter_map(|w| {
                    let kind = EntityKind::Line(cadcraft_doc::Line { a: *w.first()?, b: *w.get(1)? });
                    Some(Entity { handle: Handle(0), common: e.common.clone(), kind })
                })
                .collect(),
        ),
        EntityKind::MLine(m) => Some(super::draw2::mline::explode(e, m)),
        EntityKind::MText(m) => Some(
            cadcraft_fonts::plain_mtext(&m.contents)
                .split('\n')
                .enumerate()
                .map(|(i, l)| {
                    let at = m.insert.xy() + Vec2::from_angle(m.rotation - std::f64::consts::FRAC_PI_2) * (m.height * (1.0 + 5.0 / 3.0 * i as f64));
                    Entity {
                        handle: Handle(0),
                        common: e.common.clone(),
                        kind: EntityKind::Text(cadcraft_doc::Text {
                            insert: v3(at),
                            align_pt: None,
                            height: m.height,
                            value: l.into(),
                            rotation: m.rotation,
                            width_factor: 1.0,
                            oblique: 0.0,
                            style: m.style.clone(),
                            halign: Default::default(),
                            valign: Default::default(),
                        }),
                    }
                })
                .collect(),
        ),
        // A region explodes into its boundary's lines and arcs.
        EntityKind::Region(r) => Some(
            r.segments()
                .into_iter()
                .map(|s| {
                    let k = match s {
                        Segment::Line(l) => line(l.a, l.b),
                        Segment::Arc { arc: a, .. } => arc(&a),
                    };
                    Entity { handle: Handle(0), common: e.common.clone(), kind: k }
                })
                .collect(),
        ),
        EntityKind::Dimension(dm) => {
            // The dimension as drawn: its overrides, DIMSCALE (0 = the drawing's) and text font.
            let style = d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides);
            let g = cadcraft_render::dimension_in(d, dm);
            let mut v: Vec<Entity> = g
                .lines
                .iter()
                .filter_map(|l| {
                    if l.len() == 2 {
                        Some(Entity { handle: Handle(0), common: e.common.clone(), kind: line(*l.first()?, *l.get(1)?) })
                    } else {
                        Some(Entity {
                            handle: Handle(0),
                            common: e.common.clone(),
                            kind: lwpoly(l.iter().map(|p| PolyVertex::new(*p)).collect(), false),
                        })
                    }
                })
                .collect();
            for t in &g.fills {
                if let [a, b, c] = t.as_slice() {
                    v.push(Entity {
                        handle: Handle(0),
                        common: e.common.clone(),
                        kind: EntityKind::Solid(cadcraft_doc::Solid { corners: [v3(*a), v3(*b), v3(*c), v3(*c)] }),
                    });
                }
            }
            if !g.value.is_empty() {
                v.push(Entity {
                    handle: Handle(0),
                    common: e.common.clone(),
                    kind: EntityKind::MText(cadcraft_doc::MText {
                        insert: v3(g.text_pos),
                        height: g.text_height,
                        width: 0.0,
                        attach: 5,
                        rotation: g.text_angle,
                        style: style.text_style.clone(),
                        contents: g.mtext_contents(),
                        line_spacing: 1.0,
                        line_spacing_exact: false,
                        columns: None,
                    }),
                });
            }
            Some(v)
        }
        EntityKind::Tolerance(t) => Some(super::tolerance::explode(d, e, t)),
        _ => None,
    }
}

fn explode(s: &mut Session, hs: &[Handle]) -> Result<Vec<Handle>> {
    explode_with(s, hs, explode_kind)
}

/// Replace each of `hs` (off locked layers) by the pieces `pieces` gives for it; objects it gives
/// none for are left alone. EXPLODE, and BURST with its own pieces for block references.
pub(crate) fn explode_with(
    s: &mut Session,
    hs: &[Handle],
    pieces: impl Fn(&cadcraft_doc::Drawing, &Entity) -> Option<Vec<Entity>>,
) -> Result<Vec<Handle>> {
    let space = s.space();
    let mut out = Vec::new();
    for h in hs {
        if super::curves::is_locked(s, *h) {
            continue;
        }
        let Some(e) = s.doc()?.entity(*h).map(|e| (**e).clone()) else { continue };
        let Some(parts) = pieces(s.doc()?, &e) else { continue };
        // The pieces of an object are new objects: its xdata goes with it. A block reference's
        // pieces are copies of the block's objects and keep theirs.
        let own_pieces = !matches!(e.kind, EntityKind::Insert(_));
        let d = s.doc_mut()?;
        d.remove_entity(*h);
        for mut p in parts {
            if own_pieces {
                p.common.xdata.clear();
            }
            p.handle = d.new_handle();
            out.push(p.handle);
            if let Some(st) = d.space_mut(&space) {
                st.push(p);
            }
        }
    }
    s.set_selection(Vec::new());
    Ok(out)
}

fn run_explode(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let r = explode(s, &hs)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

/// Gives a rebuilt arc piece its source centre Z, or a rebuilt polyline piece its source elevation.
fn with_z(mut k: EntityKind, z: f64) -> EntityKind {
    match &mut k {
        EntityKind::Arc(a) => a.center.z = z,
        EntityKind::LwPolyline(p) => p.elevation = z,
        _ => {}
    }
    k
}

/// Break `h` between two points (or at one point when p1 == p2).
pub(crate) fn break_entity(s: &mut Session, h: Handle, p1: Vec2, p2: Vec2) -> Result<Vec<Handle>> {
    if super::curves::is_locked(s, h) {
        return Err(EngineError::Other("The object is on a locked layer.".into()));
    }
    let e = s.doc()?.entity(h).map(|e| (**e).clone()).ok_or_else(|| EngineError::Other("no such object".into()))?;
    let pieces: Vec<EntityKind> = match &e.kind {
        EntityKind::Line(l) => {
            let ln = Line::new(l.a.xy(), l.b.xy());
            let (mut t1, mut t2) = (ln.param_of(p1).clamp(0.0, 1.0), ln.param_of(p2).clamp(0.0, 1.0));
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            let at3 = |t: f64| ln.at(t).to3(l.a.z + (l.b.z - l.a.z) * t);
            let mut v = Vec::new();
            if t1 > 1e-12 {
                v.push(EntityKind::Line(cadcraft_doc::Line { a: l.a, b: at3(t1) }));
            }
            if t2 < 1.0 - 1e-12 {
                v.push(EntityKind::Line(cadcraft_doc::Line { a: at3(t2), b: l.b }));
            }
            v
        }
        EntityKind::Circle(c) => {
            if p1.near(p2, 1e-12) {
                return Err(EngineError::Other("Arc cannot be full 360 degrees.".into()));
            }
            let a1 = c.center.xy().angle_to(p1);
            let a2 = c.center.xy().angle_to(p2);
            // AutoCAD removes the CCW portion from the first to the second point.
            vec![with_z(arc(&Arc::new(c.center.xy(), c.radius, a2, a1)), c.center.z)]
        }
        EntityKind::Arc(a) => {
            let ga = Arc::new(a.center.xy(), a.radius, a.start, a.end);
            let par = |p: Vec2| norm_angle(ga.center.angle_to(p) - ga.start).min(ga.sweep());
            let (mut t1, mut t2) = (par(p1), par(p2));
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            let mut v = Vec::new();
            if t1 > 1e-12 {
                v.push(with_z(arc(&Arc::new(ga.center, ga.radius, ga.start, ga.start + t1)), a.center.z));
            }
            if t2 < ga.sweep() - 1e-12 {
                v.push(with_z(arc(&Arc::new(ga.center, ga.radius, ga.start + t2, ga.end)), a.center.z));
            }
            v
        }
        EntityKind::LwPolyline(p) => {
            let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
            let segs = pl.segments();
            let total: f64 = segs.iter().map(Segment::len).sum();
            let lp = |q: Vec2| {
                let mut acc = 0.0;
                let mut best = (f64::INFINITY, 0.0);
                for s in &segs {
                    let c = s.closest(q);
                    let dd = c.dist(q);
                    if dd < best.0 {
                        best = (dd, acc + param_on(s, c) * s.len());
                    }
                    acc += s.len();
                }
                best.1
            };
            let (mut t1, mut t2) = (lp(p1), lp(p2));
            if p.closed {
                if (t1 - t2).abs() < 1e-9 {
                    return Err(EngineError::Other("Cannot break a closed polyline at a single point.".into()));
                }
                // Like a circle: the stretch from the first point to the second, along the
                // vertices, goes; what is left runs from the second point round to the first.
                let doubled: Vec<Segment> = segs.iter().chain(segs.iter()).copied().collect();
                let end = if t1 > t2 { t1 } else { t1 + total };
                vec![with_z(lwpoly(sub_polyline(&doubled, t2, end), false), p.elevation)]
            } else {
                if t1 > t2 {
                    std::mem::swap(&mut t1, &mut t2);
                }
                let mut v = Vec::new();
                if t1 > 1e-12 {
                    v.push(with_z(lwpoly(sub_polyline(&segs, 0.0, t1), false), p.elevation));
                }
                if t2 < total - 1e-12 {
                    v.push(with_z(lwpoly(sub_polyline(&segs, t2, total), false), p.elevation));
                }
                v
            }
        }
        _ => return Err(EngineError::Other("Cannot break that object.".into())),
    };
    let common = e.common.clone();
    let space = s.space();
    let d = s.doc_mut()?;
    d.remove_entity(h);
    let mut out = Vec::new();
    for k in pieces {
        let nh = d.new_handle();
        if let Some(st) = d.space_mut(&space) {
            st.push(Entity { handle: nh, common: common.clone(), kind: k });
            out.push(nh);
        }
    }
    Ok(out)
}

fn run_break(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("break", "`handle` is required"))?;
    let p1 = point_req("break", p, "p1")?;
    let p2 = point_param(p, "p2").unwrap_or(p1);
    let r = break_entity(s, h, p1, p2)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

fn run_breakat(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("breakatpoint", "`handle` is required"))?;
    let at = point_req("breakatpoint", p, "at")?;
    let r = break_entity(s, h, at, at)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

/// Join lines/arcs/polylines that share endpoints into one polyline (or merge collinear lines).
pub(crate) fn join(s: &mut Session, hs: &[Handle]) -> Result<Option<Handle>> {
    if let Some(h) = super::modify2::join_curves(s, hs)? {
        return Ok(Some(h));
    }
    if hs.iter().any(|h| super::curves::is_locked(s, *h)) {
        return Err(EngineError::Other("An object is on a locked layer.".into()));
    }
    let d = s.doc()?;
    let mut segs: Vec<Segment> = Vec::new();
    // Source entity of each segment in `segs`, so only entities that end up in the chain are removed.
    let mut owners: Vec<Handle> = Vec::new();
    let mut common = None;
    for h in hs {
        let Some(e) = d.entity(*h) else { continue };
        common.get_or_insert_with(|| e.common.clone());
        match &e.kind {
            EntityKind::Line(l) => segs.push(Segment::Line(Line::new(l.a.xy(), l.b.xy()))),
            EntityKind::Arc(a) => segs.push(Segment::Arc { arc: Arc::new(a.center.xy(), a.radius, a.start, a.end), ccw: true }),
            EntityKind::LwPolyline(p) if !p.closed => segs.extend(Polyline { vertices: p.vertices.clone(), closed: false }.segments()),
            _ => {}
        }
        owners.resize(segs.len(), *h);
    }
    if segs.len() < 2 {
        return Err(EngineError::Other("Select at least two objects to join.".into()));
    }
    // Chain greedily from the first segment.
    let mut chain = vec![segs.remove(0)];
    let mut used = vec![owners.remove(0)];
    let tol = 1e-6;
    loop {
        let start = chain.first().map(Segment::start).unwrap_or_default();
        let end = chain.last().map(Segment::end).unwrap_or_default();
        let Some(i) =
            segs.iter().position(|s| s.start().near(end, tol) || s.end().near(end, tol) || s.start().near(start, tol) || s.end().near(start, tol))
        else {
            break;
        };
        let s0 = segs.remove(i);
        used.push(owners.remove(i));
        if s0.start().near(end, tol) {
            chain.push(s0);
        } else if s0.end().near(end, tol) {
            chain.push(s0.reversed());
        } else if s0.end().near(start, tol) {
            chain.insert(0, s0);
        } else {
            chain.insert(0, s0.reversed());
        }
    }
    if chain.len() < 2 {
        return Err(EngineError::Other("Objects are not connected.".into()));
    }
    let closed = chain.first().zip(chain.last()).is_some_and(|(a, b)| a.start().near(b.end(), tol));
    let mut vs: Vec<PolyVertex> = chain
        .iter()
        .map(|s| PolyVertex {
            p: s.start(),
            bulge: match s {
                Segment::Line(_) => 0.0,
                Segment::Arc { arc, ccw } => cadcraft_geom::arc_to_bulge(if *ccw { arc.sweep() } else { -arc.sweep() }),
            },
            ..Default::default()
        })
        .collect();
    if !closed && let Some(l) = chain.last() {
        vs.push(PolyVertex::new(l.end()));
    }
    let space = s.space();
    let doc = s.doc_mut()?;
    for h in hs.iter().filter(|h| used.contains(*h)) {
        doc.remove_entity(*h);
    }
    let nh = doc.new_handle();
    if let Some(st) = doc.space_mut(&space) {
        st.push(Entity { handle: nh, common: common.unwrap_or_default(), kind: lwpoly(vs, closed) });
    }
    s.set_selection(Vec::new());
    Ok(Some(nh))
}

fn run_join(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    if p.get("close").and_then(Value::as_bool) == Some(true) {
        let h = super::modify2::join_close(s, &hs)?;
        return Ok(json!({ "handle": h.hex() }));
    }
    let r = join(s, &hs)?;
    Ok(json!({ "handle": r.map(|h| h.hex()) }))
}

// ---------------- interactive ----------------

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Erase,
    Move,
    Copy,
    Rotate,
    Scale,
    Mirror,
    Explode,
    Join,
}

/// Select objects, then collect the points/values the operation needs.
struct SelectThen {
    op: Op,
    sel: SelectPhase,
    objs: Vec<Handle>,
    pts: Vec<Vec2>,
    reference: bool,
    /// ROTATE Reference: the reference angle (radians), once known. Its first point, and the
    /// first of the two new-angle points, are kept in `ref_from`.
    ref_angle: Option<f64>,
    /// ROTATE Reference: the new angle is given by two points (the `Points` option).
    ref_points: bool,
    /// SCALE Reference: first point of a reference length given by two points.
    ref_from: Option<Vec2>,
    /// SCALE Reference: the reference length, once known.
    ref_len: Option<f64>,
    copy_mode: bool,
    ask_erase: bool,
    /// JOIN of a single arc or elliptical arc: the prompt asks for arcs to join to it, or cLose.
    join_source: Option<Handle>,
}

impl SelectThen {
    fn new(op: Op) -> Self {
        SelectThen {
            op,
            sel: SelectPhase::default(),
            objs: Vec::new(),
            pts: Vec::new(),
            reference: false,
            ref_angle: None,
            ref_points: false,
            ref_from: None,
            ref_len: None,
            copy_mode: false,
            ask_erase: false,
            join_source: None,
        }
    }
    fn apply(&mut self, s: &mut Session) -> Result<Step> {
        // Immediate operations once selection is done.
        match self.op {
            Op::Erase => {
                let n = erase(s, &self.objs)?;
                let _ = n;
                Ok(Step::Done)
            }
            Op::Explode => {
                explode(s, &self.objs)?;
                Ok(Step::Done)
            }
            Op::Join => {
                if let [h] = self.objs[..]
                    && s.doc()?.entity(h).is_some_and(|e| super::modify2::closed_kind(&e.kind).is_some())
                {
                    self.join_source = Some(h);
                    return Ok(Step::Continue);
                }
                self.join_now(s)
            }
            _ => Ok(Step::Continue),
        }
    }

    /// JOIN the selected objects and report what they became.
    fn join_now(&mut self, s: &mut Session) -> Result<Step> {
        match join(s, &self.objs) {
            Ok(h) => {
                // Objects outside the joined chain are kept, so count only the ones JOIN consumed
                // (and the source object an ellipse or spline join keeps).
                let d = s.doc()?;
                let n = self.objs.iter().filter(|o| d.entity(**o).is_none() || h == Some(**o)).count();
                let what = match h.and_then(|h| d.entity(h)).map(|e| &e.kind) {
                    Some(EntityKind::Spline(_)) => "spline",
                    Some(EntityKind::Ellipse(_)) => "ellipse",
                    _ => "polyline",
                };
                s.echo(format!("{n} objects joined into 1 {what}"))
            }
            Err(e) => s.echo(e.to_string()),
        }
        Ok(Step::Done)
    }

    /// JOIN of a single arc or elliptical arc: more arcs picked join to it on Enter, cLose makes
    /// it its circle or full ellipse.
    fn join_source_input(&mut self, s: &mut Session, src: Handle, i: Input) -> Result<Step> {
        match i {
            Input::Keyword(k) if k == "cLose" => {
                let ellipse = matches!(s.doc()?.entity(src).map(|e| &e.kind), Some(EntityKind::Ellipse(_)));
                super::modify2::join_close(s, &[src])?;
                s.echo(if ellipse { "The elliptical arc is now an ellipse." } else { "The arc is now a circle." });
                Ok(Step::Done)
            }
            Input::Point(p) => {
                if let Some(h) = super::curves::pick_at(s, p)
                    && !self.objs.contains(&h)
                {
                    self.objs.push(h);
                }
                Ok(Step::Continue)
            }
            Input::Enter => self.join_now(s),
            _ => Ok(Step::Continue),
        }
    }

    /// SCALE Reference: reference length (typed, or two points), then new length (typed, or a
    /// point measured from the base point). The scale factor is new length / reference length.
    fn scale_reference(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Some(base) = self.pts.first().copied() else { return Ok(Step::Done) };
        match (self.ref_len, self.ref_from, i) {
            (None, None, Input::Point(p)) => {
                self.ref_from = Some(p);
                Ok(Step::Continue)
            }
            (None, None, Input::Text(t)) => {
                self.ref_len = Some(require_length(number(&t))?);
                Ok(Step::Continue)
            }
            (None, None, Input::Enter) => {
                self.ref_len = Some(1.0);
                Ok(Step::Continue)
            }
            (None, Some(a), Input::Point(p)) => {
                self.ref_len = Some(require_length(Some(a.dist(p)))?);
                Ok(Step::Continue)
            }
            (Some(r), _, Input::Point(p)) => self.scale_by(s, base, base.dist(p) / r),
            (Some(r), _, Input::Text(t)) => {
                let n = require_length(number(&t))?;
                self.scale_by(s, base, n / r)
            }
            (_, _, Input::Enter) => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }

    /// ROTATE Reference: reference angle (typed, or two points), then the new angle (typed, a
    /// point measured from the base point, or two points after `Points`). The objects turn by the
    /// new angle minus the reference angle.
    fn rotate_reference(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Some(base) = self.pts.first().copied() else { return Ok(Step::Done) };
        let au = s.angle_settings();
        let angle = |t: &str| au.direction(t).filter(|a| a.is_finite()).ok_or_else(|| EngineError::Other("Requires an angle or point.".into()));
        let two_points = |a: Vec2, p: Vec2| {
            if a.near(p, 1e-12) { Err(EngineError::Other("The two points must differ.".into())) } else { Ok(a.angle_to(p)) }
        };
        let Some(r) = self.ref_angle else {
            match (self.ref_from, i) {
                (None, Input::Point(p)) => self.ref_from = Some(p),
                (None, Input::Text(t)) => self.ref_angle = Some(angle(&t)?),
                (None, Input::Enter) => self.ref_angle = Some(0.0),
                (Some(a), Input::Point(p)) => {
                    self.ref_angle = Some(two_points(a, p)?);
                    self.ref_from = None;
                }
                _ => {}
            }
            return Ok(Step::Continue);
        };
        let new = match (self.ref_points, self.ref_from, i) {
            (false, _, Input::Keyword(k)) if k == "Points" => {
                self.ref_points = true;
                return Ok(Step::Continue);
            }
            (false, _, Input::Point(p)) => base.angle_to(p),
            (false, _, Input::Text(t)) => angle(&t)?,
            (false, _, Input::Enter) => 0.0,
            (true, None, Input::Point(p)) => {
                self.ref_from = Some(p);
                return Ok(Step::Continue);
            }
            (true, Some(a), Input::Point(p)) => two_points(a, p)?,
            _ => return Ok(Step::Continue),
        };
        transform_entities(s, &self.objs, &Mat3::rotate_about(base, new - r), self.copy_mode)?;
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }

    /// The prompt while ROTATE Reference collects its angles.
    fn rotate_reference_prompt(&self) -> Prompt {
        match (self.ref_angle, self.ref_points, self.ref_from) {
            (None, _, None) => Prompt::new("Specify the reference angle", Accept::POINT_OR_NUMBER).default("0"),
            (None, _, Some(p)) | (Some(_), true, Some(p)) => Prompt::new("Specify second point", Accept::POINT).base(p),
            (Some(_), false, _) => {
                Prompt::new("Specify the new angle", Accept::POINT_OR_NUMBER).kw(&["Points"]).default("0").base_opt(self.pts.first().copied())
            }
            (Some(_), true, None) => Prompt::new("Specify first point", Accept::POINT),
        }
    }

    /// Rubber band while ROTATE Reference collects its angles: the objects turned to the cursor
    /// once the reference angle is known, else the line from the first point.
    fn rotate_reference_preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        let (Some(base), Ok(d)) = (self.pts.first().copied(), s.doc()) else { return Vec::new() };
        match (self.ref_angle, self.ref_points, self.ref_from) {
            (Some(r), false, _) => {
                let m = Mat3::rotate_about(base, base.angle_to(c) - r);
                let mut out: Vec<EntityKind> = self
                    .objs
                    .iter()
                    .take(500)
                    .filter_map(|h| d.entity(*h))
                    .map(|e| {
                        let mut k = e.kind.clone();
                        k.transform(&m);
                        k
                    })
                    .collect();
                out.push(line(base, c));
                out
            }
            (_, _, Some(a)) => vec![line(a, c)],
            _ => Vec::new(),
        }
    }

    fn scale_by(&mut self, s: &mut Session, base: Vec2, f: f64) -> Result<Step> {
        let f = require_length(Some(f))?;
        // The next SCALE offers this factor.
        s.last_used.scale_factor = f;
        transform_entities(s, &self.objs, &Mat3::scale_about(base, f), self.copy_mode)?;
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
}

/// Objects whose text MIRROR keeps readable instead of mirroring it with the geometry.
fn keeps_reading_when_mirrored(k: &EntityKind) -> bool {
    match k {
        EntityKind::Insert(i) => !i.attribs.is_empty(),
        k => matches!(
            k,
            EntityKind::Text(_)
                | EntityKind::AttDef(_)
                | EntityKind::MText(_)
                | EntityKind::Dimension(_)
                | EntityKind::MLeader(_)
                | EntityKind::Table(_)
                | EntityKind::Tolerance(_)
        ),
    }
}

/// Text objects transformed for each frame of a MIRROR preview (on the CPU); more are not drawn.
const MAX_MIRROR_TEXT_PREVIEW: usize = 1000;

/// A finite length greater than zero (rejects 0, negatives, NaN and infinity).
fn positive_length(v: Option<f64>) -> Option<f64> {
    v.filter(|f| f.is_finite() && *f > 1e-12)
}

/// [`positive_length`], or the error that makes the prompt ask again.
fn require_length(v: Option<f64>) -> Result<f64> {
    positive_length(v).ok_or_else(|| EngineError::Other("Requires a positive number.".into()))
}

impl Interactive for SelectThen {
    fn name(&self) -> &'static str {
        match self.op {
            Op::Erase => "ERASE",
            Op::Move => "MOVE",
            Op::Copy => "COPY",
            Op::Rotate => "ROTATE",
            Op::Scale => "SCALE",
            Op::Mirror => "MIRROR",
            Op::Explode => "EXPLODE",
            Op::Join => "JOIN",
        }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            self.objs = self.sel.picked.clone();
            return self.apply(s);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        if !self.sel.done {
            return self.sel.prompt();
        }
        if self.ask_erase {
            return Prompt::new("Erase source objects?", Accept::TEXT).kw(&["Yes", "No"]).default("No");
        }
        if let Some(src) = self.join_source {
            let ellipse = s.doc().is_ok_and(|d| matches!(d.entity(src).map(|e| &e.kind), Some(EntityKind::Ellipse(_))));
            let msg = if ellipse { "Select elliptical arcs to join to source" } else { "Select arcs to join to source" };
            return Prompt::new(msg, Accept::POINT).kw(&["cLose"]);
        }
        let k = self.pts.len();
        let base = self.pts.first().copied();
        if self.op == Op::Scale && self.reference {
            return match (self.ref_len, self.ref_from) {
                (Some(_), _) => Prompt::new("Specify new length", Accept::POINT_OR_NUMBER).base_opt(base),
                (None, Some(p)) => Prompt::new("Specify second point", Accept::POINT).base(p),
                (None, None) => Prompt::new("Specify reference length", Accept::POINT_OR_NUMBER).default("1"),
            };
        }
        if self.op == Op::Rotate && self.reference {
            return self.rotate_reference_prompt();
        }
        match (self.op, k) {
            (Op::Move | Op::Copy, 0) => Prompt::new("Specify base point", Accept::POINT).kw(&["Displacement"]),
            (Op::Move, _) => Prompt::new("Specify second point or <use first point as displacement>", Accept::POINT).base_opt(base),
            (Op::Copy, _) => Prompt::new("Specify second point", Accept::POINT).kw(&["Array", "Exit", "Undo"]).base_opt(base),
            (Op::Rotate | Op::Scale, 0) => Prompt::new("Specify base point", Accept::POINT),
            // ROTATE and SCALE offer the angle/factor used last time.
            (Op::Rotate, _) => {
                let (au, ap) = s.doc().map(|d| (d.header.i64("AUNITS", 0), d.header.i64("AUPREC", 0))).unwrap_or((0, 0));
                // Offered as it would be typed: clockwise is positive when ANGDIR is 1.
                let a = if s.angle_settings().clockwise { -s.last_used.rotate_angle } else { s.last_used.rotate_angle };
                Prompt::new("Specify rotation angle", Accept::POINT_OR_NUMBER)
                    .kw(&["Copy", "Reference"])
                    .default(crate::units::format_angle(a, au, ap))
                    .base_opt(base)
            }
            (Op::Scale, _) => Prompt::new("Specify scale factor", Accept::POINT_OR_NUMBER)
                .kw(&["Copy", "Reference"])
                .default(format!("{:.4}", s.last_used.scale_factor))
                .base_opt(base),
            (Op::Mirror, 0) => Prompt::new("Specify first point of mirror line", Accept::POINT),
            (Op::Mirror, _) => Prompt::new("Specify second point of mirror line", Accept::POINT).base_opt(base),
            _ => Prompt::new("", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            match self.sel.feed(s, &i)? {
                SelOutcome::More => return Ok(Step::Continue),
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.objs = hs;
                    return self.apply(s);
                }
            }
        }
        if let Some(src) = self.join_source {
            return self.join_source_input(s, src, i);
        }
        if self.ask_erase {
            let yes = matches!(&i, Input::Keyword(k) if k == "Yes") || matches!(&i, Input::Text(t) if t.trim().to_ascii_lowercase().starts_with('y'));
            let (a, b) = (self.pts[0], self.pts[1]);
            transform_entities(s, &self.objs, &Mat3::mirror(a, b), !yes)?;
            s.set_selection(Vec::new());
            return Ok(Step::Done);
        }
        let k = self.pts.len();
        if self.op == Op::Scale && self.reference {
            return self.scale_reference(s, i);
        }
        if self.op == Op::Rotate && self.reference {
            return self.rotate_reference(s, i);
        }
        match (self.op, k, i) {
            (_, _, Input::Keyword(kw)) if kw == "Copy" => {
                self.copy_mode = true;
                s.echo("Rotating/scaling a copy of the selected objects.");
                Ok(Step::Continue)
            }
            (Op::Scale | Op::Rotate, 1, Input::Keyword(kw)) if kw == "Reference" => {
                self.reference = true;
                Ok(Step::Continue)
            }
            (Op::Copy, _, Input::Keyword(kw)) if kw == "Exit" => {
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (_, 0, Input::Point(p)) => {
                self.pts.push(p);
                Ok(Step::Continue)
            }
            (Op::Move, 1, Input::Point(p)) => {
                let d = p - self.pts[0];
                transform_entities(s, &self.objs, &Mat3::translate(d), false)?;
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Move, 1, Input::Enter) => {
                let d = self.pts[0];
                transform_entities(s, &self.objs, &Mat3::translate(d), false)?;
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Copy, _, Input::Point(p)) => {
                let d = p - self.pts[0];
                transform_entities(s, &self.objs, &Mat3::translate(d), true)?;
                Ok(Step::Continue)
            }
            (Op::Copy, _, Input::Enter) => {
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Rotate, 1, inp @ (Input::Point(_) | Input::Text(_) | Input::Enter)) => {
                let a = match inp {
                    Input::Point(p) => self.pts[0].angle_to(p),
                    Input::Text(t) => s
                        .angle_settings()
                        .rotation(&t)
                        .filter(|a| a.is_finite())
                        .ok_or_else(|| EngineError::Other("Requires an angle or point.".into()))?,
                    _ => s.last_used.rotate_angle,
                };
                s.last_used.rotate_angle = a;
                transform_entities(s, &self.objs, &Mat3::rotate_about(self.pts[0], a), self.copy_mode)?;
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Scale, 1, inp @ (Input::Point(_) | Input::Text(_) | Input::Enter)) => {
                let f = match inp {
                    Input::Point(p) => self.pts[0].dist(p),
                    Input::Text(t) => {
                        number(&t).filter(|f| f.is_finite() && *f > 0.0).ok_or_else(|| EngineError::Other("Requires a positive number.".into()))?
                    }
                    _ => s.last_used.scale_factor,
                };
                if f > 1e-12 && f.is_finite() {
                    s.last_used.scale_factor = f;
                    transform_entities(s, &self.objs, &Mat3::scale_about(self.pts[0], f), self.copy_mode)?;
                }
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (Op::Mirror, 1, Input::Point(p)) => {
                if p.near(self.pts[0], 1e-12) {
                    return Err(EngineError::Other("Mirror line points must differ.".into()));
                }
                self.pts.push(p);
                self.ask_erase = true;
                Ok(Step::Continue)
            }
            (_, _, Input::Enter) => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.op == Op::Rotate && self.reference {
            return self.rotate_reference_preview(s, c);
        }
        if !self.sel.done {
            return Vec::new();
        }
        let Some(base) = self.pts.first().copied() else { return Vec::new() };
        let m = match self.op {
            Op::Move | Op::Copy => Mat3::translate(c - base),
            Op::Rotate => Mat3::rotate_about(base, base.angle_to(c)),
            Op::Scale if !self.reference => Mat3::scale_about(base, base.dist(c).max(1e-9)),
            Op::Scale => match self.ref_len.and_then(|r| positive_length(Some(base.dist(c) / r))) {
                Some(f) => Mat3::scale_about(base, f),
                None if self.ref_len.is_some() => return Vec::new(),
                None => return self.ref_from.map(|a| vec![line(a, c)]).unwrap_or_default(),
            },
            Op::Mirror if !self.ask_erase => Mat3::mirror(base, c),
            _ => return Vec::new(),
        };
        let Ok(d) = s.doc() else { return Vec::new() };
        let mut out: Vec<EntityKind> = self
            .objs
            .iter()
            .take(500)
            .filter_map(|h| d.entity(*h))
            .map(|e| {
                let mut k = e.kind.clone();
                k.transform(&m);
                k
            })
            .collect();
        out.push(line(base, c));
        out
    }
    fn drag(&self, s: &Session, c: Vec2) -> Option<crate::prompt::Drag> {
        if (self.op == Op::Rotate && self.reference) || !self.sel.done {
            return None;
        }
        let base = self.pts.first().copied()?;
        let matrix = match self.op {
            Op::Move | Op::Copy => Mat3::translate(c - base),
            Op::Rotate => Mat3::rotate_about(base, base.angle_to(c)),
            Op::Scale if !self.reference => Mat3::scale_about(base, base.dist(c).max(1e-9)),
            Op::Scale => Mat3::scale_about(base, self.ref_len.and_then(|r| positive_length(Some(base.dist(c) / r)))?),
            Op::Mirror if !self.ask_erase => Mat3::mirror(base, c),
            _ => return None,
        };
        let mut extra = vec![line(base, c)];
        if self.op != Op::Mirror {
            return Some(crate::prompt::Drag { handles: self.objs.clone(), matrix, extra, clipboard: false });
        }
        // Mirrored text keeps reading left to right (MIRRTEXT 0), which the matrix alone can't
        // draw: those objects are transformed one by one, the rest is mirrored on the GPU.
        let d = s.doc().ok()?;
        let mut handles = Vec::with_capacity(self.objs.len());
        let mut texts = 0;
        for h in &self.objs {
            let Some(e) = d.entity(*h) else { continue };
            if !keeps_reading_when_mirrored(&e.kind) {
                handles.push(*h);
            } else if texts < MAX_MIRROR_TEXT_PREVIEW {
                texts += 1;
                let mut k = e.kind.clone();
                k.transform(&matrix);
                extra.push(k);
            }
        }
        Some(crate::prompt::Drag { handles, matrix, extra, clipboard: false })
    }
}

/// An OFFSET option prompt opened from "Specify offset distance".
#[derive(Clone, Copy, PartialEq)]
enum OffsetAsk {
    Erase,
    Layer,
}

struct OffsetM {
    dist: Option<f64>,
    distance_first: Option<Vec2>,
    through: bool,
    picked: Option<Handle>,
    ask: Option<OffsetAsk>,
    /// Multiple mode: each side point offsets the newest copy again, until Enter.
    multiple: bool,
    /// Undo: the drawing before each offset, with the object picked then.
    undo: Vec<(std::sync::Arc<cadcraft_doc::Drawing>, Option<Handle>)>,
}

impl OffsetM {
    fn new(s: &Session) -> Self {
        let _ = s;
        OffsetM { dist: None, distance_first: None, through: false, picked: None, ask: None, multiple: false, undo: Vec::new() }
    }

    fn set_distance(&mut self, s: &mut Session, distance: f64) -> Result<()> {
        if !(distance.is_finite() && distance > 0.0) {
            return Err(EngineError::Other("Requires a positive distance.".into()));
        }
        s.doc_mut()?.header.set_f64("OFFSETDIST", distance);
        self.dist = Some(distance);
        self.distance_first = None;
        Ok(())
    }
}

/// OFFSET's remembered options: erase the source (OFFSETERASE) and put copies on the current
/// layer instead of the source's (kept in the drawing header beside it).
fn offset_options(s: &Session) -> (bool, bool) {
    s.doc().map(|d| (d.header.i64("OFFSETERASE", 0) != 0, d.header.i64("OFFSETLAYER", 0) != 0)).unwrap_or((false, false))
}

impl Interactive for OffsetM {
    fn name(&self) -> &'static str {
        "OFFSET"
    }
    /// Starts OFFSET, keeping a single preselected offsettable object (issue #1610) so the prompt
    /// sequence goes distance then side instead of asking for the object again; any other
    /// selection is cleared and the object loop asks for it. With PICKFIRST off the selection is
    /// likewise ignored and cleared.
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        // Exactly one offsettable object selected before the command is taken at once, so only the
        // side is asked for; the object loop stays for the ones after it (issue #1610). A multiple
        // selection is left for the select prompt, as AutoCAD does.
        let pre = s.selection();
        if s.settings.pickfirst
            && let [h] = pre[..]
            && s.doc()?.entity(h).is_some_and(|e| offsettable(&e.kind))
        {
            self.picked = Some(h);
            s.set_selection(vec![h]);
        } else {
            s.set_selection(Vec::new());
        }
        let (erase, current) = offset_options(s);
        s.echo(format!(
            "Current settings: Erase source={}  Layer={}  OFFSETGAPTYPE=0",
            if erase { "Yes" } else { "No" },
            if current { "Current" } else { "Source" }
        ));
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let (erase, current) = offset_options(s);
        match self.ask {
            Some(OffsetAsk::Erase) => {
                return Prompt::new("Erase source object after offsetting?", Accept::TEXT).kw(&["Yes", "No"]).default(if erase {
                    "Yes"
                } else {
                    "No"
                });
            }
            Some(OffsetAsk::Layer) => {
                return Prompt::new("Enter layer option for offset objects", Accept::TEXT).kw(&["Current", "Source"]).default(if current {
                    "Current"
                } else {
                    "Source"
                });
            }
            None => {}
        }
        let last = s.doc().map(|d| d.header.f64("OFFSETDIST", -1.0)).unwrap_or(-1.0);
        if self.dist.is_none()
            && !self.through
            && let Some(base) = self.distance_first
        {
            return Prompt::new("Specify second point", Accept::POINT).base(base);
        }
        match (self.dist.is_some() || self.through, self.picked) {
            (false, _) => Prompt::new("Specify offset distance", Accept::POINT_OR_NUMBER).kw(&["Through", "Erase", "Layer"]).default(if last < 0.0 {
                "Through".to_string()
            } else {
                format!("{last:.4}")
            }),
            (true, None) => Prompt::new("Select object to offset", Accept::POINT).kw(&["Exit", "Undo"]).default("Exit"),
            (true, Some(_)) if self.multiple => {
                let msg = if self.through { "Specify through point" } else { "Specify point on side to offset" };
                Prompt::new(msg, Accept::POINT).kw(&["Exit", "Undo"]).default("next object")
            }
            (true, Some(_)) if self.through => Prompt::new("Specify through point", Accept::POINT).kw(&["Exit", "Multiple", "Undo"]).default("Exit"),
            (true, Some(_)) => Prompt::new("Specify point on side to offset", Accept::POINT).kw(&["Exit", "Multiple", "Undo"]).default("Exit"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(ask) = self.ask {
            let word = match &i {
                Input::Keyword(k) | Input::Text(k) => k.trim().to_ascii_lowercase(),
                Input::Enter => String::new(),
                _ => return Ok(Step::Continue),
            };
            let (key, yes, no) = match ask {
                OffsetAsk::Erase => ("OFFSETERASE", "y", "n"),
                OffsetAsk::Layer => ("OFFSETLAYER", "c", "s"),
            };
            let v = if word.is_empty() {
                None
            } else if word.starts_with(yes) {
                Some(1)
            } else if word.starts_with(no) {
                Some(0)
            } else {
                return Err(EngineError::Other("Invalid option keyword.".into()));
            };
            if let Some(v) = v {
                s.doc_mut()?.header.set_i64(key, v);
            }
            self.ask = None;
            return Ok(Step::Continue);
        }
        if self.dist.is_none() && !self.through {
            match i {
                Input::Text(t) => {
                    let d = number(&t).ok_or_else(|| EngineError::Other("Requires a positive distance.".into()))?;
                    self.set_distance(s, d)?;
                }
                Input::Point(p) => {
                    if !p.is_finite() {
                        return Err(EngineError::Other("Invalid point.".into()));
                    }
                    match self.distance_first {
                        Some(base) => self.set_distance(s, base.dist(p))?,
                        None => self.distance_first = Some(p),
                    }
                }
                Input::Keyword(k) if k == "Through" => {
                    self.through = true;
                    self.distance_first = None;
                }
                Input::Keyword(k) if k == "Erase" => self.ask = Some(OffsetAsk::Erase),
                Input::Keyword(k) if k == "Layer" => self.ask = Some(OffsetAsk::Layer),
                Input::Enter => {
                    self.distance_first = None;
                    let last = s.doc()?.header.f64("OFFSETDIST", -1.0);
                    if last > 0.0 {
                        self.dist = Some(last);
                    } else {
                        self.through = true;
                    }
                }
                _ => {}
            }
            return Ok(Step::Continue);
        }
        match (self.picked, i) {
            (_, Input::Keyword(k)) if k == "Exit" => {
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (_, Input::Keyword(k)) if k == "Undo" => {
                match self.undo.pop() {
                    Some((doc, picked)) => {
                        // Settings changed meanwhile stay.
                        let header = s.doc()?.header.clone();
                        s.state_mut()?.doc = doc;
                        s.doc_mut()?.header = header;
                        self.picked = if self.multiple { picked } else { None };
                        if self.picked.is_none() {
                            self.multiple = false;
                        }
                        s.set_selection(self.picked.into_iter().collect());
                    }
                    None => s.echo("Nothing to undo."),
                }
                Ok(Step::Continue)
            }
            (Some(_), Input::Keyword(k)) if k == "Multiple" => {
                self.multiple = true;
                Ok(Step::Continue)
            }
            (Some(_), Input::Enter) if self.multiple => {
                // <next object>: back to selecting.
                self.multiple = false;
                self.picked = None;
                s.set_selection(Vec::new());
                Ok(Step::Continue)
            }
            (_, Input::Enter) => {
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (None, Input::Point(p)) => {
                let ap = s.pixel_size() * s.settings.pickbox.max(1.0) * 1.5;
                let space = s.space();
                match crate::select::pick(s.doc()?, &space, p, ap) {
                    Some(h) => {
                        self.picked = Some(h);
                        s.set_selection(vec![h]);
                    }
                    None => s.echo("*Invalid selection*"),
                }
                Ok(Step::Continue)
            }
            (Some(h), Input::Point(p)) => {
                let e = s.doc()?.entity(h).map(|e| (**e).clone());
                let mut made = None;
                if let Some(e) = e {
                    let dist = match self.dist {
                        Some(d) => d,
                        None => {
                            let polys = crate::select::hit_polylines(s.doc()?, &e, 1e-4);
                            polys
                                .iter()
                                .flat_map(|pl| pl.windows(2).filter_map(|w| Some(Line::new(*w.first()?, *w.get(1)?).dist(p))))
                                .fold(f64::INFINITY, f64::min)
                        }
                    };
                    match offset_kind(&e.kind, dist, p) {
                        Some(k) => {
                            let before = s.state()?.doc.clone();
                            let (erase, current) = offset_options(s);
                            made = Some(add_offset(s, &e, k, current, erase)?);
                            self.undo.push((before, Some(h)));
                        }
                        None => s.echo("Cannot offset that object."),
                    }
                }
                // Multiple mode carries on from the new copy.
                self.picked = if self.multiple { made.or(self.picked) } else { None };
                s.set_selection(self.picked.into_iter().collect());
                Ok(Step::Continue)
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if let Some(base) = self.distance_first
            && c.is_finite()
        {
            return vec![line(base, c)];
        }
        let (Some(h), Ok(d)) = (self.picked, s.doc()) else { return Vec::new() };
        let Some(e) = d.entity(h) else { return Vec::new() };
        let dist = match self.dist {
            Some(x) => x,
            None => crate::select::entity_distance(d, e, c, 1e-3),
        };
        offset_kind(&e.kind, dist, c).into_iter().collect()
    }
}

/// A FILLET/CHAMFER option prompt opened from the select prompts.
#[derive(Clone, Copy, PartialEq)]
enum FilletAsk {
    /// "Enter Trim mode option [Trim/No trim]" (TRIMMODE).
    Trim,
    /// CHAMFER "Enter trim method [Distance/Angle]" (CHAMMODE).
    Method,
    /// CHAMFER Angle: the length on the first line (CHAMFERC), then the angle (CHAMFERD).
    Length,
    Angle,
}

struct FilletM {
    chamfer: bool,
    first: Option<(Handle, Vec2)>,
    /// Asking for the radius (FILLET) or a chamfer distance: 1 = first, 2 = second.
    asking: u8,
    polyline: bool,
    ask: Option<FilletAsk>,
    /// Multiple: keep asking for pairs until Enter.
    multiple: bool,
    /// Undo: the drawing before each fillet/chamfer made in this command.
    undo: Vec<std::sync::Arc<cadcraft_doc::Drawing>>,
}

impl FilletM {
    fn new(_s: &Session, chamfer: bool) -> Self {
        FilletM { chamfer, first: None, asking: 0, polyline: false, ask: None, multiple: false, undo: Vec::new() }
    }

    /// One fillet/chamfer done: carry on in Multiple mode, else finish.
    fn next(&mut self, s: &mut Session, before: std::sync::Arc<cadcraft_doc::Drawing>) -> Step {
        self.undo.push(before);
        self.first = None;
        self.polyline = false;
        s.set_selection(Vec::new());
        if self.multiple { Step::Continue } else { Step::Done }
    }
}

/// TRIMMODE: FILLET and CHAMFER trim or extend the selected objects to the new corner (default).
fn trim_mode(s: &Session) -> bool {
    s.doc().map(|d| d.header.i64("TRIMMODE", 1) != 0).unwrap_or(true)
}

/// The chamfer CHAMFER uses now: two distances, or length and angle when CHAMMODE = 1.
fn chamfer_cut(s: &Session) -> Result<ChamferCut> {
    let h = &s.doc()?.header;
    Ok(if h.i64("CHAMMODE", 0) == 1 {
        ChamferCut::Angle(h.f64("CHAMFERC", 0.0), h.f64("CHAMFERD", 0.0))
    } else {
        ChamferCut::Dist(h.f64("CHAMFERA", 0.0), h.f64("CHAMFERB", 0.0))
    })
}

impl Interactive for FilletM {
    fn name(&self) -> &'static str {
        if self.chamfer { "CHAMFER" } else { "FILLET" }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        let trim = trim_mode(s);
        let d = s.doc()?;
        let msg = if self.chamfer {
            let mode = if trim { "TRIM" } else { "NOTRIM" };
            if d.header.i64("CHAMMODE", 0) == 1 {
                let (au, ap) = (d.header.i64("AUNITS", 0), d.header.i64("AUPREC", 0));
                let a = crate::units::format_angle(d.header.f64("CHAMFERD", 0.0), au, ap);
                format!("({mode} mode) Current chamfer Length = {:.4}, Angle = {a}", d.header.f64("CHAMFERC", 0.0))
            } else {
                format!("({mode} mode) Current chamfer Dist1 = {:.4}, Dist2 = {:.4}", d.header.f64("CHAMFERA", 0.0), d.header.f64("CHAMFERB", 0.0))
            }
        } else {
            format!("Current settings: Mode = {}, Radius = {:.4}", if trim { "TRIM" } else { "NOTRIM" }, d.header.f64("FILLETRAD", 0.0))
        };
        s.echo(msg);
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        // The radius and distances are remembered in the drawing (FILLETRAD, CHAMFERA/B) and
        // offered as defaults; the second chamfer distance defaults to the first.
        if self.asking > 0 {
            let hdr = |k: &str| s.doc().map(|d| d.header.f64(k, 0.0)).unwrap_or(0.0);
            let (msg, v) = match (self.chamfer, self.asking) {
                (false, _) => ("Specify fillet radius", hdr("FILLETRAD")),
                (true, 1) => ("Specify first chamfer distance", hdr("CHAMFERA")),
                (true, _) => ("Specify second chamfer distance", hdr("CHAMFERA")),
            };
            return Prompt::new(msg, Accept::NUMBER).default(format!("{v:.4}"));
        }
        let hdr_i = |k: &str, dflt: i64| s.doc().map(|d| d.header.i64(k, dflt)).unwrap_or(dflt);
        match self.ask {
            Some(FilletAsk::Trim) => {
                let cur = if trim_mode(s) { "Trim" } else { "No trim" };
                return Prompt::new("Enter Trim mode option", Accept::TEXT).kw(&["Trim", "No trim"]).default(cur);
            }
            Some(FilletAsk::Method) => {
                let cur = if hdr_i("CHAMMODE", 0) == 1 { "Angle" } else { "Distance" };
                return Prompt::new("Enter trim method", Accept::TEXT).kw(&["Distance", "Angle"]).default(cur);
            }
            Some(FilletAsk::Length) => {
                let v = s.doc().map(|d| d.header.f64("CHAMFERC", 0.0)).unwrap_or(0.0);
                return Prompt::new("Specify chamfer length on the first line", Accept::NUMBER).default(format!("{v:.4}"));
            }
            Some(FilletAsk::Angle) => {
                let (a, au, ap) =
                    s.doc().map(|d| (d.header.f64("CHAMFERD", 0.0), d.header.i64("AUNITS", 0), d.header.i64("AUPREC", 0))).unwrap_or((0.0, 0, 0));
                return Prompt::new("Specify chamfer angle from the first line", Accept::NUMBER).default(crate::units::format_angle(a, au, ap));
            }
            None => {}
        }
        if self.polyline {
            return Prompt::new("Select 2D polyline", Accept::POINT);
        }
        match (self.first, self.chamfer) {
            (None, false) => Prompt::new("Select first object", Accept::POINT).kw(&["Undo", "Polyline", "Radius", "Trim", "Multiple"]),
            (None, true) => {
                Prompt::new("Select first line", Accept::POINT).kw(&["Undo", "Polyline", "Distance", "Angle", "Trim", "mEthod", "Multiple"])
            }
            (Some(_), false) => Prompt::new("Select second object or shift-select to apply corner", Accept::POINT).kw(&["Radius"]),
            (Some(_), true) => Prompt::new("Select second line or shift-select to apply corner", Accept::POINT).kw(&["Distance", "Angle", "Method"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.asking > 0 {
            // Enter keeps the default.
            let v = match &i {
                Input::Text(t) => Some(
                    number(t).filter(|v| v.is_finite() && *v >= 0.0).ok_or_else(|| EngineError::Other("Requires a non-negative distance.".into()))?,
                ),
                Input::Enter => None,
                _ => return Ok(Step::Continue),
            };
            let d = s.doc_mut()?;
            match (self.chamfer, self.asking) {
                (false, _) => {
                    if let Some(v) = v {
                        d.header.set_f64("FILLETRAD", v);
                    }
                    self.asking = 0;
                }
                (true, 1) => {
                    if let Some(v) = v {
                        d.header.set_f64("CHAMFERA", v);
                    }
                    self.asking = 2;
                }
                (true, _) => {
                    let v = v.unwrap_or_else(|| d.header.f64("CHAMFERA", 0.0));
                    d.header.set_f64("CHAMFERB", v);
                    self.asking = 0;
                }
            }
            return Ok(Step::Continue);
        }
        if let Some(ask) = self.ask {
            return self.option(s, ask, i);
        }
        match i {
            Input::Keyword(k) if k == "Radius" || k == "Distance" => {
                if self.chamfer {
                    s.doc_mut()?.header.set_i64("CHAMMODE", 0);
                }
                self.asking = 1;
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Angle" && self.chamfer => {
                s.doc_mut()?.header.set_i64("CHAMMODE", 1);
                self.ask = Some(FilletAsk::Length);
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "mEthod" || k == "Method" => {
                self.ask = Some(FilletAsk::Method);
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Trim" => {
                self.ask = Some(FilletAsk::Trim);
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Multiple" => {
                self.multiple = true;
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Polyline" => {
                self.polyline = true;
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Undo" => {
                match self.undo.pop() {
                    Some(doc) => {
                        // Settings changed meanwhile stay.
                        let header = s.doc()?.header.clone();
                        s.state_mut()?.doc = doc;
                        s.doc_mut()?.header = header;
                    }
                    None => s.echo("Nothing to undo."),
                }
                self.first = None;
                s.set_selection(Vec::new());
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                let ap = s.pixel_size() * s.settings.pickbox.max(1.0) * 1.5;
                let space = s.space();
                let Some(h) = crate::select::pick(s.doc()?, &space, p, ap) else {
                    s.echo("*Invalid selection*");
                    return Ok(Step::Continue);
                };
                if self.polyline {
                    let before = s.state()?.doc.clone();
                    if self.chamfer {
                        let n = chamfer_polyline(s, h, chamfer_cut(s)?)?;
                        s.echo(format!("{n} lines were chamfered"));
                    } else {
                        let r = s.doc()?.header.f64("FILLETRAD", 0.0);
                        let n = super::modify2::fillet_polyline(s, h, r)?;
                        s.echo(format!("{n} lines were filleted"));
                    }
                    return Ok(self.next(s, before));
                }
                match self.first {
                    None => {
                        self.first = Some((h, p));
                        s.set_selection(vec![h]);
                        Ok(Step::Continue)
                    }
                    Some((h1, p1)) => {
                        let r = s.doc()?.header.f64("FILLETRAD", 0.0);
                        let cut = if self.chamfer { Some(chamfer_cut(s)?) } else { None };
                        let before = s.state()?.doc.clone();
                        let trim = trim_mode(s);
                        match fillet_corner(s, h1, p1, h, p, r, cut, trim) {
                            Ok(_) => Ok(self.next(s, before)),
                            Err(e) => {
                                s.echo(e.to_string());
                                self.first = None;
                                s.set_selection(Vec::new());
                                Ok(Step::Continue)
                            }
                        }
                    }
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

impl FilletM {
    /// Input at a Trim / Method / Length / Angle option prompt; Enter keeps the current value.
    fn option(&mut self, s: &mut Session, ask: FilletAsk, i: Input) -> Result<Step> {
        let text = match &i {
            Input::Keyword(k) | Input::Text(k) => k.trim().to_ascii_lowercase(),
            Input::Enter => String::new(),
            _ => return Ok(Step::Continue),
        };
        let invalid = || EngineError::Other("Invalid option keyword.".into());
        let flag = |yes: &str, no: &str| -> Result<Option<i64>> {
            if text.is_empty() {
                Ok(None)
            } else if text.starts_with(yes) {
                Ok(Some(1))
            } else if text.starts_with(no) {
                Ok(Some(0))
            } else {
                Err(invalid())
            }
        };
        let au = s.angle_settings();
        let d = s.doc_mut()?;
        self.ask = None;
        match ask {
            FilletAsk::Trim => {
                if let Some(v) = flag("t", "n")? {
                    d.header.set_i64("TRIMMODE", v);
                }
            }
            FilletAsk::Method => {
                if let Some(v) = flag("a", "d")? {
                    d.header.set_i64("CHAMMODE", v);
                }
            }
            FilletAsk::Length => {
                if !text.is_empty() {
                    let v = number(&text)
                        .filter(|v| v.is_finite() && *v >= 0.0)
                        .ok_or_else(|| EngineError::Other("Requires a non-negative distance.".into()))?;
                    d.header.set_f64("CHAMFERC", v);
                }
                self.ask = Some(FilletAsk::Angle);
            }
            FilletAsk::Angle => {
                if !text.is_empty() {
                    let a = au
                        .amount(&text)
                        .filter(|a| a.is_finite() && *a >= 0.0 && *a < std::f64::consts::PI)
                        .ok_or_else(|| EngineError::Other("Requires an angle from 0 to 180 degrees.".into()))?;
                    d.header.set_f64("CHAMFERD", a);
                }
            }
        }
        Ok(Step::Continue)
    }
}

/// How CHAMFER cuts a corner: two distances, or a length on the first line and an angle
/// (radians) from it.
#[derive(Clone, Copy, Debug)]
enum ChamferCut {
    Dist(f64, f64),
    Angle(f64, f64),
}

impl ChamferCut {
    /// The distances back from the corner along the first and second lines, which leave it
    /// along the unit directions `u1` and `u2`.
    fn distances(self, u1: Vec2, u2: Vec2) -> Result<(f64, f64)> {
        match self {
            ChamferCut::Dist(a, b) => Ok((a, b)),
            ChamferCut::Angle(len, ang) => {
                // Triangle corner–t1–t2: the corner angle and the chamfer angle at t1 give t2 by
                // the law of sines.
                let corner = u1.dot(u2).clamp(-1.0, 1.0).acos();
                let s = (corner + ang).sin();
                let d2 = len * ang.sin() / s;
                if corner + ang >= std::f64::consts::PI - 1e-9 || s.abs() < 1e-12 || !d2.is_finite() || d2 < 0.0 {
                    return Err(EngineError::Other("The chamfer angle does not fit this corner.".into()));
                }
                Ok((len, d2))
            }
        }
    }
}

/// FILLET (`cut` None) or CHAMFER two objects, honouring TRIMMODE: with `trim` false the new
/// arc or line is added and both objects are left as they were.
#[allow(clippy::too_many_arguments)]
fn fillet_corner(s: &mut Session, h1: Handle, p1: Vec2, h2: Handle, p2: Vec2, r: f64, cut: Option<ChamferCut>, trim: bool) -> Result<Option<Handle>> {
    let dist = cut.map(|c| move |u1: Vec2, u2: Vec2| c.distances(u1, u2));
    if let Some(made) = super::modify2::fillet_pline(s, h1, p1, h2, p2, r, dist.as_ref().map(|f| f as super::modify2::ChamferDistances), trim)? {
        return Ok(made);
    }
    let d = s.doc()?;
    let k1 = d.entity(h1).map(|e| e.kind.clone());
    let k2 = d.entity(h2).map(|e| e.kind.clone());
    let chamfer = match cut {
        None => None,
        Some(c) => {
            let l = |h| d.entity(h).and_then(|e| as_line(e)).ok_or_else(|| EngineError::Other(CHAMFER_LINES_ONLY.into()));
            let (l1, l2) = (l(h1)?, l(h2)?);
            let (x, _, _) = line_line_infinite(l1.a, l1.b, l2.a, l2.b).ok_or_else(|| EngineError::Other("Lines are parallel.".into()))?;
            // The direction along each line towards its picked side.
            let dir = |l: &Line, p: Vec2| if (l.a - x).dot(p - x) >= (l.b - x).dot(p - x) { (l.a - x).normalized() } else { (l.b - x).normalized() };
            Some(c.distances(dir(&l1, p1), dir(&l2, p2))?)
        }
    };
    let made = fillet_lines(s, h1, p1, h2, p2, r, chamfer)?;
    if !trim {
        let doc = s.doc_mut()?;
        for (h, k) in [(h1, k1), (h2, k2)] {
            if let Some(k) = k {
                doc.modify_entity(h, |e| e.kind = k)?;
            }
        }
    }
    Ok(made)
}

/// CHAMFER Polyline: cut every corner between two straight segments of a 2D polyline. Returns
/// how many corners were chamfered.
fn chamfer_polyline(s: &mut Session, h: Handle, cut: ChamferCut) -> Result<usize> {
    let d = s.doc()?;
    let e = d.entity(h).ok_or_else(|| EngineError::Other("no such object".into()))?;
    if d.layer(&e.common.layer).is_some_and(|l| l.locked) {
        return Err(EngineError::Other("The object is on a locked layer.".into()));
    }
    let EntityKind::LwPolyline(pl) = &e.kind else { return Err(EngineError::Other("Select a 2D polyline.".into())) };
    let vs = &pl.vertices;
    let n = vs.len();
    if n < 3 {
        return Ok(0);
    }
    let at = |i: usize| vs.get(i % n).copied().unwrap_or_default();
    // Distances cut back from each corner along the incoming and outgoing segments.
    let mut cuts = vec![(0.0f64, 0.0f64); n];
    for (i, c) in cuts.iter_mut().enumerate() {
        if !pl.closed && (i == 0 || i + 1 == n) {
            continue;
        }
        let (prev, cur, next) = (at(i + n - 1), at(i), at(i + 1));
        if prev.bulge.abs() > 1e-12 || cur.bulge.abs() > 1e-12 {
            continue;
        }
        let u1 = (prev.p - cur.p).normalized();
        let u2 = (next.p - cur.p).normalized();
        if u1 == Vec2::ZERO || u2 == Vec2::ZERO || u1.dot(u2) < -1.0 + 1e-9 {
            continue;
        }
        if let Ok(ab) = cut.distances(u1, u2) {
            *c = ab;
        }
    }
    // Drop chamfers that do not fit their segments.
    for i in 0..n {
        let j = (i + 1) % n;
        let seg = at(i).p.dist(at(j).p);
        let (out_i, in_j) = (cuts.get(i).map_or(0.0, |c| c.1), cuts.get(j).map_or(0.0, |c| c.0));
        if out_i + in_j > seg + 1e-9 {
            for k in [i, j] {
                if let Some(c) = cuts.get_mut(k) {
                    *c = (0.0, 0.0);
                }
            }
        }
    }
    let mut out: Vec<PolyVertex> = Vec::with_capacity(n * 2);
    let mut count = 0;
    for i in 0..n {
        let v = at(i);
        let (a, b) = cuts.get(i).copied().unwrap_or_default();
        if a <= 1e-12 && b <= 1e-12 {
            out.push(v);
            continue;
        }
        let t1 = v.p + (at(i + n - 1).p - v.p).normalized() * a;
        let t2 = v.p + (at(i + 1).p - v.p).normalized() * b;
        out.push(PolyVertex { p: t1, bulge: 0.0, ..v });
        out.push(PolyVertex { p: t2, bulge: 0.0, ..v });
        count += 1;
    }
    let mut npl = pl.clone();
    npl.vertices = out;
    s.doc_mut()?.modify_entity(h, |e| e.kind = EntityKind::LwPolyline(npl))?;
    Ok(count)
}

#[derive(Default)]
struct BreakM {
    obj: Option<(Handle, Vec2)>,
    /// The `First point` option: the next point replaces the selection pick as the first break point.
    first_point: bool,
}

impl Interactive for BreakM {
    fn name(&self) -> &'static str {
        "BREAK"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.obj {
            None => Prompt::new("Select object", Accept::POINT),
            Some(_) if self.first_point => Prompt::new("Specify first break point", Accept::POINT),
            Some(_) => Prompt::new("Specify second break point", Accept::POINT).kw(&["First point"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.obj, i) {
            (None, Input::Point(p)) => {
                let ap = s.pixel_size() * s.settings.pickbox.max(1.0) * 1.5;
                let space = s.space();
                match crate::select::pick(s.doc()?, &space, p, ap) {
                    Some(h) => {
                        self.obj = Some((h, p));
                        s.set_selection(vec![h]);
                    }
                    None => s.echo("*Invalid selection*"),
                }
                Ok(Step::Continue)
            }
            (Some(_), Input::Keyword(k)) if k == "First point" => {
                self.first_point = true;
                Ok(Step::Continue)
            }
            (Some((h, _)), Input::Point(p1)) if self.first_point => {
                self.obj = Some((h, p1));
                self.first_point = false;
                Ok(Step::Continue)
            }
            (Some((h, p1)), Input::Point(p2)) => {
                break_entity(s, h, p1, p2)?;
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

/// MOVE and COPY: [`SelectThen`], plus the Displacement option at the base point prompt
/// (the keyword, or Enter): the typed point is the displacement itself.
struct DisplaceM {
    inner: SelectThen,
    displacement: bool,
}

impl DisplaceM {
    fn new(op: Op) -> Self {
        DisplaceM { inner: SelectThen::new(op), displacement: false }
    }
    /// At "Specify base point": objects selected, no point yet.
    fn at_base(&self) -> bool {
        self.inner.sel.done && self.inner.pts.is_empty() && !self.displacement
    }
}

impl Interactive for DisplaceM {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.inner.begin(s)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        if self.displacement {
            return Prompt::new("Specify displacement", Accept::POINT).default("0,0");
        }
        if self.at_base() {
            return Prompt::new("Specify base point", Accept::POINT).kw(&["Displacement"]).default("Displacement");
        }
        self.inner.prompt(s)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.displacement {
            let d = match i {
                Input::Point(p) => p,
                Input::Enter => Vec2::ZERO,
                _ => return Ok(Step::Continue),
            };
            if d != Vec2::ZERO {
                transform_entities(s, &self.inner.objs, &Mat3::translate(d), self.inner.op == Op::Copy)?;
            }
            s.set_selection(Vec::new());
            return Ok(Step::Done);
        }
        if self.at_base() && (matches!(&i, Input::Enter) || matches!(&i, Input::Keyword(k) if k == "Displacement")) {
            self.displacement = true;
            return Ok(Step::Continue);
        }
        self.inner.input(s, i)
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.displacement {
            return Vec::new();
        }
        self.inner.preview(s, c)
    }
    fn drag(&self, s: &Session, c: Vec2) -> Option<crate::prompt::Drag> {
        if self.displacement {
            return None;
        }
        self.inner.drag(s, c)
    }
}

pub(crate) fn _unused(_: Circle) {}
