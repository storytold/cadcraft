use super::transform::{delta_of, transform_entities};
use super::{
    Accept, Entity, EntityKind, Handle, Input, Interactive, Mat3, Prompt, Result, SelectKind, SelectMode, Session, Step, Value, Vec2, bad, json,
    line, points_param,
};

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

pub(super) fn run_stretch(s: &mut Session, p: &Value) -> Result<Value> {
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
    hs.retain(|h| !crate::cmd::curves::is_locked(s, *h));
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
pub(super) struct StretchM {
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
