//! Hit testing and window/crossing selection.

use cadcraft_doc::{Drawing, Entity, EntityKind, Handle, Prim, Space, entity_bounds};
use cadcraft_geom::{Bounds2, Line, Vec2, point_in_polygon};

use crate::spatial::{self, SpatialIndex};
use crate::{Accept, Prompt};

/// Polylines approximating an entity for hit testing, tessellated at `tol`.
pub fn hit_polylines(d: &Drawing, e: &Entity, tol: f64) -> Vec<Vec<Vec2>> {
    match &e.kind {
        EntityKind::Text(_)
        | EntityKind::MText(_)
        | EntityKind::Insert(_)
        | EntityKind::Dimension(_)
        | EntityKind::AttDef(_)
        | EntityKind::Table(_)
        | EntityKind::Tolerance(_)
        | EntityKind::MLeader(_)
        | EntityKind::Image(_) => {
            // Text-like objects are hit inside their box.
            let b = entity_bounds(d, e, 0);
            if b.is_empty() {
                return Vec::new();
            }
            let c = b.corners();
            let mut v = vec![vec![c[0], c[1], c[2], c[3], c[0]]];
            if matches!(e.kind, EntityKind::Dimension(_) | EntityKind::Insert(_) | EntityKind::MLeader(_)) {
                let list = cadcraft_render::build_entities(
                    d,
                    std::iter::once(e),
                    &cadcraft_render::Options { tolerance: tol, text: false, ..Default::default() },
                );
                v = list.prims.iter().map(|p| list.points(p).to_vec()).collect();
                v.push(vec![c[0], c[1], c[2], c[3], c[0]]);
            }
            v
        }
        kind => {
            let mut out = Vec::new();
            for p in kind.prims() {
                let mut pts = Vec::new();
                match p {
                    Prim::Seg(s) => s.tessellate(tol, &mut pts),
                    Prim::Circle(c) => c.tessellate(tol, &mut pts),
                    Prim::Ellipse(el) => el.tessellate(tol, &mut pts),
                    Prim::Spline(s) => pts = s.tessellate(tol),
                    Prim::Point(p) => pts = vec![p, p],
                    Prim::Infinite { base, dir, ray } => {
                        let far = 1e9;
                        pts = vec![if ray { base } else { base - dir * far }, base + dir * far];
                    }
                    Prim::Fill(f) => {
                        pts = f.clone();
                        if let Some(first) = f.first() {
                            pts.push(*first);
                        }
                    }
                }
                out.push(pts);
            }
            // A pattern hatch is also picked on its drawn pattern strokes, not only its boundary.
            if let EntityKind::Hatch(h) = kind
                && !h.solid
                && h.gradient.is_none()
                && !h.pattern.eq_ignore_ascii_case("SOLID")
            {
                let list = cadcraft_render::build_entities(
                    d,
                    std::iter::once(e),
                    &cadcraft_render::Options { tolerance: tol, text: false, fill: false, ..Default::default() },
                );
                out.extend(list.prims.iter().map(|p| list.points(p).to_vec()));
            }
            out
        }
    }
}

fn dist_to_polyline(pts: &[Vec2], p: Vec2) -> f64 {
    if pts.len() == 1 {
        return pts.first().map(|q| q.dist(p)).unwrap_or(f64::INFINITY);
    }
    pts.windows(2).filter_map(|w| Some(Line::new(*w.first()?, *w.get(1)?).dist(p))).fold(f64::INFINITY, f64::min)
}

/// Distance from `p` to the entity (0 inside text boxes and solid fills).
pub fn entity_distance(d: &Drawing, e: &Entity, p: Vec2, tol: f64) -> f64 {
    let polys = hit_polylines(d, e, tol);
    let mut best = polys.iter().map(|pl| dist_to_polyline(pl, p)).fold(f64::INFINITY, f64::min);
    let filled = matches!(e.kind, EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::AttDef(_) | EntityKind::Table(_))
        || matches!(e.kind, EntityKind::Tolerance(_));
    if (filled && entity_bounds(d, e, 0).contains(p)) || in_text(d, e, p) {
        best = 0.0;
    }
    // A solid hatch fills its loops with odd parity under its island style, as it is drawn, not
    // its bounding rectangle.
    if let EntityKind::Hatch(h) = &e.kind
        && h.solid
        && cadcraft_render::hatch_fill_loops(h, tol).iter().filter(|l| point_in_polygon(l, p)).count() % 2 == 1
    {
        best = 0.0;
    }
    // A SOLID fills its polygon, not its bounding rectangle (half of which is empty for a triangle).
    if matches!(e.kind, EntityKind::Solid(_)) && e.kind.prims().iter().any(|prim| matches!(prim, Prim::Fill(poly) if point_in_polygon(poly, p))) {
        best = 0.0;
    }
    best
}

/// Whether `p` is on the text of a dimension or multileader: inside the box around its words,
/// turned with the text, as plain text is hit anywhere inside its box. Their other geometry is
/// hit along its lines, so without this the cursor found the object only between the letters'
/// edges and the lines.
fn in_text(d: &Drawing, e: &Entity, p: Vec2) -> bool {
    match &e.kind {
        // A dimension with a block (as AutoCAD saves them) is drawn from that block: its text is
        // the text there, not the text the dimension's own geometry would place.
        EntityKind::Dimension(dm) => match dm.block.as_ref().and_then(|n| d.block(n)) {
            Some(blk) => blk.entities.iter().any(|be| match &be.kind {
                EntityKind::MText(t) => in_box(cadcraft_render::layout_mtext_entity(d, t).strokes.into_iter().flatten(), t.rotation, p),
                EntityKind::Text(_) => entity_bounds(d, be, 0).contains(p),
                _ => false,
            }),
            None => {
                let g = cadcraft_render::dimension_in(d, dm);
                in_box(g.text.iter().flatten().chain(g.text_glyphs.iter().flatten().flatten()).copied(), g.text_angle, p)
            }
        },
        EntityKind::MLeader(m) => {
            m.text.as_ref().is_some_and(|t| in_box(cadcraft_render::layout_mtext_entity(d, t).strokes.into_iter().flatten(), t.rotation, p))
        }
        _ => false,
    }
}

/// Whether `p` is inside the box around `pts` turned by `angle` (the box of text set at `angle`).
fn in_box(pts: impl Iterator<Item = Vec2>, angle: f64, p: Vec2) -> bool {
    let turn = |q: Vec2| q.rotate(-angle);
    let b = Bounds2::from_points(pts.filter(|q| q.is_finite()).map(turn));
    !b.is_empty() && b.contains(turn(p))
}

fn selectable(d: &Drawing, e: &Entity) -> bool {
    d.is_visible(e)
}

/// Entities to test, in the order to test them, each with its bounds when already known.
pub(crate) type Cands<'a> = Box<dyn Iterator<Item = (&'a Entity, Option<Bounds2>)> + 'a>;

pub(crate) fn bounds_of(d: &Drawing, e: &Entity, known: Option<Bounds2>) -> Bounds2 {
    known.unwrap_or_else(|| entity_bounds(d, e, 0))
}

pub(crate) fn is_infinite(e: &Entity) -> bool {
    matches!(e.kind, EntityKind::XLine(_) | EntityKind::Ray(_))
}

/// Candidates from the spatial index touching `query` (draw order, reversed when `rev`), or every
/// entity of the space when it is too small to index (or the query box isn't finite).
pub(crate) fn candidates<'a>(
    d: &'a Drawing,
    space: &Space,
    ix: &'a Option<std::sync::Arc<SpatialIndex>>,
    query: &Bounds2,
    infinite: bool,
    rev: bool,
) -> Option<Cands<'a>> {
    if let Some(ix) = ix
        && let Some(c) = ix.query(query, infinite)
    {
        let it = move |i: u32| ix.entity(i).map(|e| (e.as_ref(), Some(ix.bounds(i))));
        return Some(if rev { Box::new(c.into_iter().rev().filter_map(it)) } else { Box::new(c.into_iter().filter_map(it)) });
    }
    let store = d.space(space)?;
    if rev {
        let ents: Vec<&Entity> = store.iter().map(|e| e.as_ref()).collect();
        Some(Box::new(ents.into_iter().rev().map(|e| (e, None))))
    } else {
        Some(Box::new(store.iter().map(|e| (e.as_ref(), None))))
    }
}

/// The topmost entity within `aperture` of `p`.
pub fn pick(d: &Drawing, space: &Space, p: Vec2, aperture: f64) -> Option<Handle> {
    let ix = spatial::index(d, space);
    pick_with(d, space, &ix, p, aperture)
}

pub(crate) fn pick_with(d: &Drawing, space: &Space, ix: &Option<std::sync::Arc<SpatialIndex>>, p: Vec2, aperture: f64) -> Option<Handle> {
    let probe = Bounds2::new(p, p).expand(aperture);
    let mut best: Option<(f64, Handle)> = None;
    for (e, known) in candidates(d, space, ix, &probe.expand(aperture), true, true)? {
        if !selectable(d, e) {
            continue;
        }
        let infinite = is_infinite(e);
        if !infinite && !bounds_of(d, e, known).expand(aperture).intersects(&probe) {
            continue;
        }
        let dist = entity_distance(d, e, p, aperture / 4.0);
        if dist <= aperture && best.is_none_or(|(bd, _)| dist < bd - 1e-12) {
            best = Some((dist, e.handle));
            if dist == 0.0 {
                break;
            }
        }
    }
    best.map(|(_, h)| h)
}

/// Every entity within `aperture` of `p`, nearest first and topmost among equals (so the first is
/// what [`pick`] chooses): the objects selection cycling lists.
pub fn pick_all(d: &Drawing, space: &Space, p: Vec2, aperture: f64) -> Vec<Handle> {
    let ix = spatial::index(d, space);
    let probe = Bounds2::new(p, p).expand(aperture);
    let Some(cands) = candidates(d, space, &ix, &probe.expand(aperture), true, true) else { return Vec::new() };
    let mut hits: Vec<(f64, Handle)> = cands
        .filter(|(e, known)| selectable(d, e) && (is_infinite(e) || bounds_of(d, e, *known).expand(aperture).intersects(&probe)))
        .map(|(e, _)| (entity_distance(d, e, p, aperture / 4.0), e.handle))
        .filter(|(dist, _)| *dist <= aperture)
        .collect();
    hits.sort_by(|a, b| a.0.total_cmp(&b.0));
    hits.into_iter().map(|(_, h)| h).collect()
}

/// Whether PREVIEWFILTER keeps `e` from being highlighted under the cursor: bits 1 objects on
/// locked layers, 2 xrefs, 4 tables, 8 multiline text, 16 hatches, 32 objects in groups.
pub fn preview_filtered(d: &Drawing, e: &Entity, filter: u8) -> bool {
    let on = |bit: u8| filter & bit != 0;
    (on(1) && d.layer(&e.common.layer).is_some_and(|l| l.locked))
        || (on(2) && matches!(&e.kind, EntityKind::Insert(i) if d.block(&i.block).is_some_and(|b| b.xref_path.is_some())))
        || (on(4) && matches!(e.kind, EntityKind::Table(_)))
        || (on(8) && matches!(e.kind, EntityKind::MText(_)))
        || (on(16) && matches!(e.kind, EntityKind::Hatch(_)))
        || (on(32) && d.groups.iter().any(|g| g.members.contains(&e.handle)))
}

fn seg_hits_box(a: Vec2, b: Vec2, bx: &Bounds2) -> bool {
    if bx.contains(a) || bx.contains(b) {
        return true;
    }
    let c = bx.corners();
    (0..4).any(|i| cadcraft_geom::line_line(&Line::new(a, b), &Line::new(c[i], c[(i + 1) % 4])).is_some())
}

/// Window selection (entirely inside) or crossing selection (inside or touching).
pub fn select_window(d: &Drawing, space: &Space, bx: Bounds2, crossing: bool) -> Vec<Handle> {
    let ix = spatial::index(d, space);
    select_window_with(d, space, &ix, bx, crossing)
}

pub(crate) fn select_window_with(d: &Drawing, space: &Space, ix: &Option<std::sync::Arc<SpatialIndex>>, bx: Bounds2, crossing: bool) -> Vec<Handle> {
    let Some(cands) = candidates(d, space, ix, &bx, crossing, false) else { return Vec::new() };
    let tol = window_tol(&bx);
    cands.filter(|(e, known)| selectable(d, e) && in_window(d, e, *known, &bx, crossing, tol)).map(|(e, _)| e.handle).collect()
}

/// The tessellation tolerance window and crossing tests use for a box.
pub(crate) fn window_tol(bx: &Bounds2) -> f64 {
    (bx.width() + bx.height()).max(1e-9) / 2000.0
}

/// Whether a window box (`crossing` false: entirely inside) or a crossing box (inside or
/// touching its geometry, not just its bounding box) selects `e`.
pub(crate) fn in_window(d: &Drawing, e: &Entity, known: Option<Bounds2>, bx: &Bounds2, crossing: bool, tol: f64) -> bool {
    let infinite = is_infinite(e);
    let eb = bounds_of(d, e, known);
    if !infinite && bx.contains_box(&eb) {
        return true;
    }
    if !crossing || (!infinite && !eb.intersects(bx)) {
        return false;
    }
    let polys = hit_polylines(d, e, tol);
    let hit = polys.iter().any(|pl| {
        if pl.len() == 1 {
            pl.first().is_some_and(|p| bx.contains(*p))
        } else {
            pl.windows(2).any(|w| w.first().zip(w.get(1)).is_some_and(|(a, b)| seg_hits_box(*a, *b, bx)))
        }
    });
    // A crossing box entirely inside a text/block bounding rectangle also selects it.
    if hit || (eb.contains_box(bx) && matches!(e.kind, EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::Insert(_))) {
        return true;
    }
    // SOLID/TRACE are filled polygons. A crossing window wholly within the
    // fill must select them even when it does not touch their outline. Check
    // the polygon, not just its bounding rectangle (which can cover empty
    // space around triangular solids).
    if matches!(e.kind, EntityKind::Solid(_) | EntityKind::Trace(_)) && eb.contains_box(bx) {
        return e.kind.prims().iter().any(|prim| match prim {
            Prim::Fill(poly) => bx.corners().iter().all(|p| cadcraft_geom::point_in_polygon(poly, *p)),
            _ => false,
        });
    }
    false
}

/// Fence selection: entities crossed by the fence polyline.
pub fn select_fence(d: &Drawing, space: &Space, fence: &[Vec2]) -> Vec<Handle> {
    let ix = spatial::index(d, space);
    select_fence_with(d, space, &ix, fence)
}

pub(crate) fn select_fence_with(d: &Drawing, space: &Space, ix: &Option<std::sync::Arc<SpatialIndex>>, fence: &[Vec2]) -> Vec<Handle> {
    let fb = Bounds2::from_points(fence.iter().copied());
    let tol = (fb.width() + fb.height()).max(1e-9) / 2000.0;
    let probe = fb.expand(tol);
    let Some(cands) = candidates(d, space, ix, &probe, true, false) else { return Vec::new() };
    let mut out = Vec::new();
    for (e, known) in cands {
        // Infinite objects have no useful bounds; like window selection, test their geometry directly.
        if !selectable(d, e) || (!is_infinite(e) && !bounds_of(d, e, known).intersects(&probe)) {
            continue;
        }
        let polys = hit_polylines(d, e, tol);
        let hit = fence.windows(2).any(|f| {
            let (Some(a), Some(b)) = (f.first(), f.get(1)) else { return false };
            let fl = Line::new(*a, *b);
            polys.iter().any(|pl| {
                pl.windows(2).any(|w| w.first().zip(w.get(1)).is_some_and(|(p, q)| cadcraft_geom::line_line(&fl, &Line::new(*p, *q)).is_some()))
            })
        });
        if hit {
            out.push(e.handle);
        }
    }
    out
}

/// Window polygon selection (entities entirely inside `poly`) or crossing polygon selection
/// (inside or crossing its edges). The polygon closes back to its first point.
pub fn select_polygon(d: &Drawing, space: &Space, poly: &[Vec2], crossing: bool) -> Vec<Handle> {
    let (Some(first), true) = (poly.first(), poly.len() >= 3) else { return Vec::new() };
    let ix = spatial::index(d, space);
    let pb = Bounds2::from_points(poly.iter().copied());
    let tol = (pb.width() + pb.height()).max(1e-9) / 2000.0;
    let Some(cands) = candidates(d, space, &ix, &pb, crossing, false) else { return Vec::new() };
    let edges: Vec<Line> =
        poly.windows(2).filter_map(|w| Some(Line::new(*w.first()?, *w.get(1)?))).chain(poly.last().map(|l| Line::new(*l, *first))).collect();
    let mut out = Vec::new();
    for (e, known) in cands {
        let infinite = is_infinite(e);
        if !selectable(d, e) || (infinite && !crossing) {
            continue;
        }
        let eb = bounds_of(d, e, known);
        if !infinite && if crossing { !eb.intersects(&pb) } else { !pb.contains_box(&eb) } {
            continue;
        }
        let polys = hit_polylines(d, e, tol);
        let crosses = polys.iter().any(|pl| {
            pl.windows(2).any(|w| {
                w.first().zip(w.get(1)).is_some_and(|(p, q)| edges.iter().any(|f| cadcraft_geom::line_line(f, &Line::new(*p, *q)).is_some()))
            })
        });
        let mut pts = polys.iter().flatten();
        let hit = if crossing {
            crosses || pts.any(|p| point_in_polygon(poly, *p))
        } else {
            !crosses && !polys.is_empty() && pts.all(|p| point_in_polygon(poly, *p))
        };
        if hit {
            out.push(e.handle);
        }
    }
    out
}

/// The visible objects in `space` of the group called `name` (any case), or `None` when the
/// drawing has no such group.
pub fn select_group(d: &Drawing, space: &Space, name: &str) -> Option<Vec<Handle>> {
    let g = d.groups.iter().find(|g| g.name.eq_ignore_ascii_case(name.trim()))?;
    let store = d.space(space)?;
    Some(g.members.iter().copied().filter(|h| store.get(*h).is_some_and(|e| d.is_visible(e))).collect())
}

/// A selection mode typed at "Select objects", collecting its points before it selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectKind {
    /// `W`: objects entirely inside two corners, whichever order they come in.
    Window,
    /// `C`: objects inside or crossing two corners, whichever order they come in.
    Crossing,
    /// `F`: objects crossed by a series of points; Enter ends it.
    Fence,
    /// `WP`: objects entirely inside a polygon; Enter closes it.
    WPolygon,
    /// `CP`: objects inside or crossing a polygon; Enter closes it.
    CPolygon,
    /// `BOX`: two corners; left to right is a window, right to left a crossing.
    Box,
    /// `G`: the objects of a group, by name.
    Group,
}

/// Most points a fence or polygon collects (typed input is hostile).
pub(crate) const MAX_MODE_POINTS: usize = 10_000;

/// A typed selection mode in progress and the points given so far.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectMode {
    pub kind: SelectKind,
    pub pts: Vec<Vec2>,
}

impl SelectMode {
    /// The mode named by text typed at "Select objects" (`W`, `Window`, `C`, `F`, `WP`, `CP`…).
    pub fn from_keyword(text: &str) -> Option<Self> {
        let kind = match text.trim().to_ascii_lowercase().as_str() {
            "w" | "window" => SelectKind::Window,
            "c" | "crossing" => SelectKind::Crossing,
            "f" | "fence" => SelectKind::Fence,
            "wp" | "wpolygon" => SelectKind::WPolygon,
            "cp" | "cpolygon" => SelectKind::CPolygon,
            "box" => SelectKind::Box,
            "g" | "group" => SelectKind::Group,
            _ => return None,
        };
        Some(SelectMode { kind, pts: Vec::new() })
    }
    fn is_box(&self) -> bool {
        matches!(self.kind, SelectKind::Window | SelectKind::Crossing | SelectKind::Box)
    }
    /// The prompt shown while the mode collects points.
    pub fn prompt(&self) -> Prompt {
        let last = self.pts.last().copied();
        let (first, next) = match self.kind {
            SelectKind::Group => return Prompt::new("Enter group name", Accept::TEXT),
            SelectKind::Window | SelectKind::Crossing | SelectKind::Box => ("Specify first corner", "Specify opposite corner"),
            SelectKind::Fence => ("Specify first fence point", "Specify next fence point"),
            SelectKind::WPolygon | SelectKind::CPolygon => ("Specify first polygon point", "Specify endpoint of line"),
        };
        match last {
            None => Prompt::new(first, Accept::POINT),
            Some(p) if self.is_box() => Prompt::new(next, Accept::POINT).corner(p),
            Some(p) => Prompt::new(next, Accept::POINT).kw(&["Undo"]).base(p),
        }
    }
    /// Add a point. Returns whether the mode is complete (the second corner of W/C).
    pub fn push(&mut self, p: Vec2) -> bool {
        if p.is_finite() && self.pts.len() < MAX_MODE_POINTS {
            self.pts.push(p);
        }
        self.is_box() && self.pts.len() >= 2
    }
    /// The objects the mode selects, or `None` while it has too few points.
    pub fn select(&self, d: &Drawing, space: &Space) -> Option<Vec<Handle>> {
        match (self.kind, self.pts.as_slice()) {
            (SelectKind::Window | SelectKind::Crossing, [a, b, ..]) => {
                Some(select_window(d, space, Bounds2::new(*a, *b), self.kind == SelectKind::Crossing))
            }
            (SelectKind::Box, [a, b, ..]) => Some(select_window(d, space, Bounds2::new(*a, *b), b.x < a.x)),
            (SelectKind::Fence, pts) if pts.len() >= 2 => Some(select_fence(d, space, pts)),
            (SelectKind::WPolygon | SelectKind::CPolygon, pts) if pts.len() >= 3 => {
                Some(select_polygon(d, space, pts, self.kind == SelectKind::CPolygon))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{Common, RayLine};
    use cadcraft_geom::Vec3;

    fn fence_hits(kind: EntityKind, fence: [(f64, f64); 2]) -> usize {
        let mut d = Drawing::new_metric();
        d.add(&Space::Model, Common::default(), kind).unwrap();
        let f = [Vec2::new(fence[0].0, fence[0].1), Vec2::new(fence[1].0, fence[1].1)];
        select_fence(&d, &Space::Model, &f).len()
    }

    #[test]
    fn fence_selects_crossed_xline_and_ray() {
        let line = RayLine { base: Vec3::new(0.0, 0.0, 0.0), dir: Vec3::new(1.0, 0.0, 0.0) };
        for x in [-5.0, 0.0, 5.0] {
            assert_eq!(fence_hits(EntityKind::XLine(line.clone()), [(x, -5.0), (x, 5.0)]), 1, "xline at x={x}");
        }
        assert_eq!(fence_hits(EntityKind::Ray(line.clone()), [(5.0, -5.0), (5.0, 5.0)]), 1);
        assert_eq!(fence_hits(EntityKind::Ray(line.clone()), [(0.0, -5.0), (0.0, 5.0)]), 1);
        assert_eq!(fence_hits(EntityKind::Ray(line.clone()), [(-5.0, -5.0), (-5.0, 5.0)]), 0);
        assert_eq!(fence_hits(EntityKind::XLine(line.clone()), [(0.0, 1.0), (0.0, 5.0)]), 0);
        assert_eq!(fence_hits(EntityKind::Ray(line), [(5.0, 1.0), (5.0, 5.0)]), 0);
    }
}

#[cfg(test)]
mod solid_interior_tests {
    use super::*;

    fn face(kind: bool, triangular: bool) -> Entity {
        let a = Vec2::new(0.0, 0.0).to3(0.0);
        let b = Vec2::new(10.0, 0.0).to3(0.0);
        let c = Vec2::new(0.0, 10.0).to3(0.0);
        let d = if triangular { c } else { Vec2::new(10.0, 10.0).to3(0.0) };
        let solid = cadcraft_doc::Solid { corners: [a, b, c, d] };
        Entity::new(Handle(256), if kind { EntityKind::Solid(solid) } else { EntityKind::Trace(solid) })
    }

    #[test]
    fn crossing_window_inside_solid_and_trace_selects_fill() {
        let inside = Bounds2::new(Vec2::new(4.0, 4.0), Vec2::new(6.0, 6.0));
        for solid in [true, false] {
            let mut d = Drawing::default();
            let e = face(solid, false);
            assert!(in_window(&d, &e, None, &inside, true, 0.01));
            assert!(!in_window(&d, &e, None, &inside, false, 0.01));
            d.model.push(e);
            assert_eq!(select_window(&d, &Space::Model, inside, true), vec![Handle(256)]);
            assert!(select_window(&d, &Space::Model, inside, false).is_empty());
        }
    }

    #[test]
    fn crossing_window_in_triangular_bbox_but_outside_fill_does_not_select() {
        let d = Drawing::default();
        let e = face(true, true);
        let empty = Bounds2::new(Vec2::new(7.0, 7.0), Vec2::new(8.0, 8.0));
        assert!(!in_window(&d, &e, None, &empty, true, 0.01));
        let inside = Bounds2::new(Vec2::new(3.0, 3.0), Vec2::new(4.0, 4.0));
        assert!(in_window(&d, &e, None, &inside, true, 0.01));
    }

    #[test]
    fn pick_in_triangular_bbox_but_outside_fill_does_not_select() {
        let mut d = Drawing::default();
        d.model.push(face(true, true));
        let at = |x: f64, y: f64| pick(&d, &Space::Model, Vec2::new(x, y), 0.25);
        // The empty corner of the bounding rectangle, 5.66 from the hypotenuse.
        assert_eq!(at(9.0, 9.0), None);
        // Inside the fill, farther than the aperture from every edge.
        assert_eq!(at(2.0, 2.0), Some(Handle(256)));
        // Outside the fill but within the aperture of the hypotenuse.
        assert_eq!(at(5.1, 5.1), Some(Handle(256)));
    }
}

#[cfg(test)]
mod hatch_pattern_pick_tests {
    use super::*;
    use cadcraft_doc::{Common, Hatch, HatchLoop};
    use cadcraft_geom::PolyVertex;

    fn square(lo: f64, hi: f64, outer: bool) -> HatchLoop {
        let vertices = [(lo, lo), (hi, lo), (hi, hi), (lo, hi)].iter().map(|&(x, y)| PolyVertex::new(Vec2::new(x, y))).collect();
        HatchLoop { vertices, outer, sources: Vec::new() }
    }

    fn ansi31_hatch(loops: Vec<HatchLoop>) -> Drawing {
        let mut d = Drawing::new_metric();
        let hatch = Hatch {
            pattern: "ANSI31".into(),
            solid: false,
            loops,
            scale: 1.0,
            angle: 0.0,
            associative: false,
            style: 0,
            elevation: 0.0,
            gradient: None,
            origin: Vec2::ZERO,
            background: None,
            pattern_lines: Vec::new(),
        };
        d.add(&Space::Model, Common::default(), EntityKind::Hatch(hatch)).unwrap();
        d
    }

    #[test]
    fn pattern_hatch_is_picked_on_a_visible_stroke() {
        let d = ansi31_hatch(vec![square(0.0, 10.0, true)]);
        // The ANSI31 diagonal passes through the middle of the square.
        assert!(pick(&d, &Space::Model, Vec2::new(5.0, 5.0), 0.05).is_some());
        // The boundary is still pickable and a point outside is not.
        assert!(pick(&d, &Space::Model, Vec2::new(5.0, 0.0), 0.05).is_some());
        assert!(pick(&d, &Space::Model, Vec2::new(15.0, 5.0), 0.05).is_none());
    }

    #[test]
    fn pattern_hatch_island_stays_empty() {
        // The diagonal is clipped to [0,0]-[4,4] and [6,6]-[10,10] around the island.
        let d = ansi31_hatch(vec![square(0.0, 10.0, true), square(4.0, 6.0, false)]);
        assert!(pick(&d, &Space::Model, Vec2::new(1.0, 1.0), 0.05).is_some());
        assert!(pick(&d, &Space::Model, Vec2::new(5.0, 5.0), 0.05).is_none());
    }

    fn solid_hatch(loops: Vec<HatchLoop>, style: u8) -> Drawing {
        let mut d = Drawing::new_metric();
        let hatch = Hatch {
            pattern: "SOLID".into(),
            solid: true,
            loops,
            scale: 1.0,
            angle: 0.0,
            associative: false,
            style,
            elevation: 0.0,
            gradient: None,
            origin: Vec2::ZERO,
            background: None,
            pattern_lines: Vec::new(),
        };
        d.add(&Space::Model, Common::default(), EntityKind::Hatch(hatch)).unwrap();
        d
    }

    #[test]
    fn solid_hatch_is_picked_inside_its_fill_not_its_bounding_rectangle() {
        let tri = HatchLoop {
            vertices: [(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)].iter().map(|&(x, y)| PolyVertex::new(Vec2::new(x, y))).collect(),
            outer: true,
            sources: Vec::new(),
        };
        let d = solid_hatch(vec![tri], 0);
        // The empty corner of the bounding rectangle, 5.66 from the hypotenuse.
        assert!(pick(&d, &Space::Model, Vec2::new(9.0, 9.0), 0.25).is_none());
        assert!(pick(&d, &Space::Model, Vec2::new(2.0, 2.0), 0.25).is_some());
        // Outside the fill but within the aperture of the hypotenuse.
        assert!(pick(&d, &Space::Model, Vec2::new(5.1, 5.1), 0.25).is_some());
    }

    #[test]
    fn solid_hatch_island_follows_the_island_style() {
        let loops = || vec![square(0.0, 10.0, true), square(4.0, 6.0, false)];
        // Normal style leaves the island empty, as it is drawn.
        let d = solid_hatch(loops(), 0);
        assert!(pick(&d, &Space::Model, Vec2::new(5.0, 5.0), 0.05).is_none());
        assert!(pick(&d, &Space::Model, Vec2::new(2.0, 2.0), 0.05).is_some());
        // Ignore style fills over the island.
        let d = solid_hatch(loops(), 2);
        assert!(pick(&d, &Space::Model, Vec2::new(5.0, 5.0), 0.05).is_some());
    }
}
