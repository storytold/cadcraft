//! Hit testing and window/crossing selection.

use cadcraft_doc::{Drawing, Entity, EntityKind, Handle, Prim, Space, entity_bounds};
use cadcraft_geom::{Bounds2, Camera3D, Line, Vec2, Vec3, point_in_polygon};

use crate::spatial::{self, SpatialIndex};
use crate::{Accept, Prompt};

/// Camera/view projection parameters used so 2D and 3D picking and selection match what the canvas draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickProjection {
    pub camera: Camera3D,
    pub target: Vec3,
    pub twist: f64,
}

impl PickProjection {
    /// Unrotated 2D Top plan projection centered at the world origin.
    pub const fn top() -> Self {
        Self { camera: Camera3D::TOP, target: Vec3::ZERO, twist: 0.0 }
    }

    /// Build a projection matching the active view parameters.
    pub fn from_view(camera: Camera3D, center: Vec2, target_z: f64, twist: f64) -> Self {
        Self { camera, target: center.to3(target_z), twist }
    }

    /// True when the projection is an untwisted 2D Top plan view aligned with world XY.
    pub fn is_plan_axis_aligned(&self) -> bool {
        self.camera.is_top_plan() && self.twist.abs() <= 1e-12
    }

    /// Project a 2D world point `p` (at `Z = 0`) into camera UV coordinates relative to `target`.
    pub fn project(&self, p: Vec2) -> Vec2 {
        if !self.camera.is_top_plan() {
            self.camera.project_3d(p.to3(0.0), self.target)
        } else {
            let d = p - self.target.xy();
            if self.twist == 0.0 { d } else { d.rotate(self.twist) }
        }
    }

    /// Project a 3D world point `p` into camera UV coordinates relative to `target`.
    pub fn project_3d_point(&self, p: Vec3) -> Vec2 {
        if !self.camera.is_top_plan() {
            self.camera.project_3d(p, self.target)
        } else {
            let d = p.xy() - self.target.xy();
            if self.twist == 0.0 { d } else { d.rotate(self.twist) }
        }
    }

    /// Unproject a camera UV coordinate onto the target plane `(target, camera.z)`.
    pub fn unproject_target_plane(&self, uv: Vec2) -> Vec3 {
        if !self.camera.is_top_plan() {
            self.target + self.camera.x * uv.x + self.camera.y * uv.y
        } else {
            let d = if self.twist == 0.0 { uv } else { uv.rotate(-self.twist) };
            (self.target.xy() + d).to3(self.target.z)
        }
    }

    fn shift_bounds(&self, bx: &Bounds2) -> Bounds2 {
        Bounds2::new(bx.min + self.target.xy(), bx.max + self.target.xy())
    }

    fn project_bounds(&self, b: &Bounds2) -> Bounds2 {
        if b.is_empty() {
            return *b;
        }
        Bounds2::from_points(b.corners().map(|c| self.project(c)))
    }
}

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
                    &cadcraft_render::Options { tolerance: tol, text: false, linetypes: false, ..Default::default() },
                );
                v = list.prims.iter().map(|p| list.points(p).to_vec()).collect();
                v.push(vec![c[0], c[1], c[2], c[3], c[0]]);
            }
            v
        }
        // An OLE object's frame is drawn, so it is picked on its outline (the corners the extents and
        // the renderer use); a frame with no width or height is drawn as a line, so it is picked as
        // one. It stays out of prims(), which TRIM, object snap and the dimension commands read.
        EntityKind::Unknown(u) if u.is_ole_frame() => match cadcraft_doc::ole_frame(&u.tags) {
            Some(q) => vec![q.to_vec()],
            None => Vec::new(),
        },
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

fn projected_hit_polylines(d: &Drawing, e: &Entity, tol: f64, proj: Option<&PickProjection>) -> Vec<Vec<Vec2>> {
    let polys = hit_polylines(d, e, tol);
    match proj {
        Some(pr) => polys.into_iter().map(|pl| pl.into_iter().map(|q| pr.project(q)).collect()).collect(),
        None => polys,
    }
}

fn in_quad_with(eb: &Bounds2, offset: Vec2, p: Vec2, proj: Option<&PickProjection>) -> bool {
    if eb.is_empty() {
        return false;
    }
    match proj {
        Some(pr) => point_in_polygon(&eb.corners().map(|c| pr.project(c + offset)), p),
        None => eb.contains(p - offset),
    }
}

fn in_polygon_with(poly: &[Vec2], p: Vec2, proj: Option<&PickProjection>) -> bool {
    match proj {
        Some(pr) => {
            let mapped: Vec<Vec2> = poly.iter().map(|&q| pr.project(q)).collect();
            point_in_polygon(&mapped, p)
        }
        None => point_in_polygon(poly, p),
    }
}

/// Distance from `p` to the entity (0 inside text boxes and solid fills).
pub fn entity_distance(d: &Drawing, e: &Entity, p: Vec2, tol: f64) -> f64 {
    entity_distance_in(d, e, p, tol, None)
}

fn entity_distance_in(d: &Drawing, e: &Entity, p: Vec2, tol: f64, proj: Option<&PickProjection>) -> f64 {
    let polys = projected_hit_polylines(d, e, tol, proj);
    let mut best = polys.iter().map(|pl| dist_to_polyline(pl, p)).fold(f64::INFINITY, f64::min);
    let filled = matches!(e.kind, EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::AttDef(_) | EntityKind::Table(_))
        || matches!(e.kind, EntityKind::Tolerance(_));
    if (filled && in_quad_with(&entity_bounds(d, e, 0), Vec2::ZERO, p, proj)) || in_text_in(d, e, p, proj) {
        best = 0.0;
    }
    // A solid hatch fills its loops with odd parity under its island style, as it is drawn, not
    // its bounding rectangle.
    if let EntityKind::Hatch(h) = &e.kind
        && h.solid
        && cadcraft_render::hatch_fill_loops(h, tol).iter().filter(|l| in_polygon_with(l, p, proj)).count() % 2 == 1
    {
        best = 0.0;
    }
    // A SOLID fills its polygon, not its bounding rectangle (half of which is empty for a triangle).
    if matches!(e.kind, EntityKind::Solid(_))
        && e.kind.prims().iter().any(|prim| matches!(prim, Prim::Fill(poly) if in_polygon_with(poly, p, proj)))
    {
        best = 0.0;
    }
    best
}

/// Whether `p` is on the text of a dimension or multileader: inside the box around its words,
/// turned with the text, as plain text is hit anywhere inside its box. Their other geometry is
/// hit along its lines, so without this the cursor found the object only between the letters'
/// edges and the lines.
fn in_text_in(d: &Drawing, e: &Entity, p: Vec2, proj: Option<&PickProjection>) -> bool {
    match &e.kind {
        // A dimension with a block (as AutoCAD saves them) is drawn from that block: its text is
        // the text there, not the text the dimension's own geometry would place.
        EntityKind::Dimension(dm) => match dm.block.as_ref().and_then(|n| d.block(n)) {
            // The block draws translated by the insertion point (group 12): test there (#2634).
            Some(blk) => {
                let offset = dm.insertion_point.xy();
                blk.entities.iter().any(|be| match &be.kind {
                    EntityKind::MText(t) => {
                        in_box_in(cadcraft_render::layout_mtext_entity(d, t).strokes.into_iter().flatten(), t.rotation, offset, p, proj)
                    }
                    EntityKind::Text(_) => in_quad_with(&entity_bounds(d, be, 0), offset, p, proj),
                    _ => false,
                })
            }
            None => {
                let g = cadcraft_render::dimension_in(d, dm);
                in_box_in(g.text.iter().flatten().chain(g.text_glyphs.iter().flatten().flatten()).copied(), g.text_angle, Vec2::ZERO, p, proj)
            }
        },
        EntityKind::MLeader(m) => m
            .text
            .as_ref()
            .is_some_and(|t| in_box_in(cadcraft_render::layout_mtext_entity(d, t).strokes.into_iter().flatten(), t.rotation, Vec2::ZERO, p, proj)),
        _ => false,
    }
}

/// Whether `p` is inside the box around `pts` turned by `angle` (the box of text set at `angle`).
fn in_box_in(pts: impl Iterator<Item = Vec2>, angle: f64, offset: Vec2, p: Vec2, proj: Option<&PickProjection>) -> bool {
    let turn = |q: Vec2| q.rotate(-angle);
    let b = Bounds2::from_points(pts.filter(|q| q.is_finite()).map(turn));
    if b.is_empty() {
        return false;
    }
    match proj {
        Some(pr) => point_in_polygon(&b.corners().map(|c| pr.project(c.rotate(angle) + offset)), p),
        None => b.contains(turn(p - offset)),
    }
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

fn projected_candidates<'a>(
    d: &'a Drawing,
    space: &Space,
    ix: &'a Option<std::sync::Arc<SpatialIndex>>,
    uv_query: &Bounds2,
    proj: &PickProjection,
    infinite: bool,
    rev: bool,
) -> Option<Cands<'a>> {
    if proj.camera.is_top_plan() {
        let world_query = Bounds2::from_points(uv_query.corners().map(|c| proj.unproject_target_plane(c).xy()));
        return candidates(d, space, ix, &world_query, infinite, rev);
    }
    candidates(d, space, &None, uv_query, infinite, rev)
}

/// The topmost entity within `aperture` of `p`: the one drawn last (on top in the draw order), not
/// the nearest, as AutoCAD 2024 picks (two objects under the pickbox: the one drawn later is
/// picked, and DRAWORDER changes which).
pub fn pick(d: &Drawing, space: &Space, p: Vec2, aperture: f64) -> Option<Handle> {
    let ix = spatial::index(d, space);
    pick_with(d, space, &ix, p, aperture)
}

pub(crate) fn pick_with(d: &Drawing, space: &Space, ix: &Option<std::sync::Arc<SpatialIndex>>, p: Vec2, aperture: f64) -> Option<Handle> {
    let probe = Bounds2::new(p, p).expand(aperture);
    // Candidates come topmost first: the first within the aperture is the one.
    for (e, known) in candidates(d, space, ix, &probe.expand(aperture), true, true)? {
        if !selectable(d, e) {
            continue;
        }
        let infinite = is_infinite(e);
        if !infinite && !bounds_of(d, e, known).expand(aperture).intersects(&probe) {
            continue;
        }
        if entity_distance(d, e, p, aperture / 4.0) <= aperture {
            return Some(e.handle);
        }
    }
    None
}

/// Every entity within `aperture` of `p`, topmost first (so the first is what [`pick`] chooses):
/// the objects selection cycling lists.
pub fn pick_all(d: &Drawing, space: &Space, p: Vec2, aperture: f64) -> Vec<Handle> {
    let ix = spatial::index(d, space);
    let probe = Bounds2::new(p, p).expand(aperture);
    let Some(cands) = candidates(d, space, &ix, &probe.expand(aperture), true, true) else { return Vec::new() };
    cands
        .filter(|(e, known)| selectable(d, e) && (is_infinite(e) || bounds_of(d, e, *known).expand(aperture).intersects(&probe)))
        .filter(|(e, _)| entity_distance(d, e, p, aperture / 4.0) <= aperture)
        .map(|(e, _)| e.handle)
        .collect()
}

/// Collect entities under camera-plane coordinate `uv` within `aperture`, topmost first.
pub fn pick_all_projected(d: &Drawing, space: &Space, uv: Vec2, aperture: f64, proj: &PickProjection) -> Vec<Handle> {
    if proj.is_plan_axis_aligned() {
        return pick_all(d, space, uv + proj.target.xy(), aperture);
    }
    let ix = spatial::index(d, space);
    let probe = Bounds2::new(uv, uv).expand(aperture);
    let Some(cands) = projected_candidates(d, space, &ix, &probe.expand(aperture), proj, true, true) else {
        return Vec::new();
    };
    cands
        .filter(|(e, known)| {
            selectable(d, e) && (is_infinite(e) || proj.project_bounds(&bounds_of(d, e, *known)).expand(aperture).intersects(&probe))
        })
        .filter(|(e, _)| entity_distance_in(d, e, uv, aperture / 4.0, Some(proj)) <= aperture)
        .map(|(e, _)| e.handle)
        .collect()
}

/// The topmost entity under camera-plane coordinate `uv` within `aperture`.
pub fn pick_projected(d: &Drawing, space: &Space, uv: Vec2, aperture: f64, proj: &PickProjection) -> Option<Handle> {
    if proj.is_plan_axis_aligned() {
        return pick(d, space, uv + proj.target.xy(), aperture);
    }
    let ix = spatial::index(d, space);
    let probe = Bounds2::new(uv, uv).expand(aperture);
    for (e, known) in projected_candidates(d, space, &ix, &probe.expand(aperture), proj, true, true)? {
        if !selectable(d, e) {
            continue;
        }
        let infinite = is_infinite(e);
        if !infinite && !proj.project_bounds(&bounds_of(d, e, known)).expand(aperture).intersects(&probe) {
            continue;
        }
        if entity_distance_in(d, e, uv, aperture / 4.0, Some(proj)) <= aperture {
            return Some(e.handle);
        }
    }
    None
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

/// Window or crossing selection in camera-plane coordinates `uv_bx`.
pub fn select_window_projected(d: &Drawing, space: &Space, uv_bx: Bounds2, crossing: bool, proj: &PickProjection) -> Vec<Handle> {
    if proj.is_plan_axis_aligned() {
        return select_window(d, space, proj.shift_bounds(&uv_bx), crossing);
    }
    let ix = spatial::index(d, space);
    let Some(cands) = projected_candidates(d, space, &ix, &uv_bx, proj, crossing, false) else { return Vec::new() };
    let tol = window_tol(&uv_bx);
    cands
        .filter(|(e, known)| selectable(d, e) && in_window_in(d, e, *known, &uv_bx, crossing, tol, Some(proj)))
        .map(|(e, _)| e.handle)
        .collect()
}

/// The tessellation tolerance window and crossing tests use for a box.
pub(crate) fn window_tol(bx: &Bounds2) -> f64 {
    (bx.width() + bx.height()).max(1e-9) / 2000.0
}

/// Whether a window box (`crossing` false: entirely inside) or a crossing box (inside or
/// touching its geometry, not just its bounding box) selects `e`.
pub(crate) fn in_window(d: &Drawing, e: &Entity, known: Option<Bounds2>, bx: &Bounds2, crossing: bool, tol: f64) -> bool {
    in_window_in(d, e, known, bx, crossing, tol, None)
}

fn in_window_in(
    d: &Drawing,
    e: &Entity,
    known: Option<Bounds2>,
    bx: &Bounds2,
    crossing: bool,
    tol: f64,
    proj: Option<&PickProjection>,
) -> bool {
    let infinite = is_infinite(e);
    let raw_eb = bounds_of(d, e, known);
    let eb = match proj {
        Some(pr) => pr.project_bounds(&raw_eb),
        None => raw_eb,
    };
    if !infinite && bx.contains_box(&eb) {
        return true;
    }
    if !infinite && !eb.intersects(bx) {
        return false;
    }
    if !crossing && proj.is_none() {
        return false;
    }
    let polys = projected_hit_polylines(d, e, tol, proj);
    if !crossing {
        return !infinite && !polys.is_empty() && polys.iter().flatten().all(|p| bx.contains(*p));
    }
    let hit = polys.iter().any(|pl| {
        if pl.len() == 1 {
            pl.first().is_some_and(|p| bx.contains(*p))
        } else {
            pl.windows(2).any(|w| w.first().zip(w.get(1)).is_some_and(|(a, b)| seg_hits_box(*a, *b, bx)))
        }
    });
    // A crossing box entirely inside a text/block bounding rectangle also selects it.
    if hit
        || (eb.contains_box(bx)
            && matches!(e.kind, EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::Insert(_))
            && bx.corners().iter().all(|c| in_quad_with(&raw_eb, Vec2::ZERO, *c, proj)))
    {
        return true;
    }
    // SOLID/TRACE are filled polygons. A crossing window wholly within the
    // fill must select them even when it does not touch their outline. Check
    // the polygon, not just its bounding rectangle (which can cover empty
    // space around triangular solids).
    if matches!(e.kind, EntityKind::Solid(_) | EntityKind::Trace(_)) && eb.contains_box(bx) {
        return e.kind.prims().iter().any(|prim| match prim {
            Prim::Fill(poly) => bx.corners().iter().all(|p| in_polygon_with(poly, *p, proj)),
            _ => false,
        });
    }
    false
}

fn polys_hit_fence(polys: &[Vec<Vec2>], fence: &[Vec2]) -> bool {
    fence.windows(2).any(|f| {
        let (Some(a), Some(b)) = (f.first(), f.get(1)) else { return false };
        let fl = Line::new(*a, *b);
        polys.iter().any(|pl| {
            pl.windows(2).any(|w| w.first().zip(w.get(1)).is_some_and(|(p, q)| cadcraft_geom::line_line(&fl, &Line::new(*p, *q)).is_some()))
        })
    })
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
    cands
        .filter(|(e, known)| selectable(d, e) && (is_infinite(e) || bounds_of(d, e, *known).intersects(&probe)))
        .filter(|(e, _)| polys_hit_fence(&hit_polylines(d, e, tol), fence))
        .map(|(e, _)| e.handle)
        .collect()
}

/// Fence selection in camera-plane coordinates `uv_fence`.
pub fn select_fence_projected(d: &Drawing, space: &Space, uv_fence: &[Vec2], proj: &PickProjection) -> Vec<Handle> {
    if proj.is_plan_axis_aligned() {
        let shifted: Vec<Vec2> = uv_fence.iter().map(|&p| p + proj.target.xy()).collect();
        return select_fence(d, space, &shifted);
    }
    let fb = Bounds2::from_points(uv_fence.iter().copied());
    let tol = (fb.width() + fb.height()).max(1e-9) / 2000.0;
    let probe = fb.expand(tol);
    let ix = spatial::index(d, space);
    let Some(cands) = projected_candidates(d, space, &ix, &probe, proj, true, false) else { return Vec::new() };
    cands
        .filter(|(e, known)| selectable(d, e) && (is_infinite(e) || proj.project_bounds(&bounds_of(d, e, *known)).intersects(&probe)))
        .filter(|(e, _)| polys_hit_fence(&projected_hit_polylines(d, e, tol, Some(proj)), uv_fence))
        .map(|(e, _)| e.handle)
        .collect()
}

fn polys_hit_polygon(polys: &[Vec<Vec2>], poly: &[Vec2], edges: &[Line], crossing: bool) -> bool {
    let crosses = polys.iter().any(|pl| {
        pl.windows(2)
            .any(|w| w.first().zip(w.get(1)).is_some_and(|(p, q)| edges.iter().any(|f| cadcraft_geom::line_line(f, &Line::new(*p, *q)).is_some())))
    });
    let mut pts = polys.iter().flatten();
    if crossing {
        crosses || pts.any(|p| point_in_polygon(poly, *p))
    } else {
        !crosses && !polys.is_empty() && pts.all(|p| point_in_polygon(poly, *p))
    }
}

/// Window polygon selection (entities entirely inside `poly`) or crossing polygon selection
/// (inside or crossing its edges). The polygon closes back to its first point.
pub fn select_polygon(d: &Drawing, space: &Space, poly: &[Vec2], crossing: bool) -> Vec<Handle> {
    select_polygon_in(d, space, poly, crossing, None)
}

/// Window polygon or crossing polygon selection in camera-plane coordinates `uv_poly`.
pub fn select_polygon_projected(d: &Drawing, space: &Space, uv_poly: &[Vec2], crossing: bool, proj: &PickProjection) -> Vec<Handle> {
    if proj.is_plan_axis_aligned() {
        let shifted: Vec<Vec2> = uv_poly.iter().map(|&p| p + proj.target.xy()).collect();
        return select_polygon(d, space, &shifted, crossing);
    }
    select_polygon_in(d, space, uv_poly, crossing, Some(proj))
}

fn select_polygon_in(d: &Drawing, space: &Space, poly: &[Vec2], crossing: bool, proj: Option<&PickProjection>) -> Vec<Handle> {
    let (Some(first), true) = (poly.first(), poly.len() >= 3) else { return Vec::new() };
    let ix = spatial::index(d, space);
    let pb = Bounds2::from_points(poly.iter().copied());
    let tol = (pb.width() + pb.height()).max(1e-9) / 2000.0;
    let cands = match proj {
        Some(pr) => projected_candidates(d, space, &ix, &pb, pr, crossing, false),
        None => candidates(d, space, &ix, &pb, crossing, false),
    };
    let Some(cands) = cands else { return Vec::new() };
    let edges: Vec<Line> =
        poly.windows(2).filter_map(|w| Some(Line::new(*w.first()?, *w.get(1)?))).chain(poly.last().map(|l| Line::new(*l, *first))).collect();
    let mut out = Vec::new();
    for (e, known) in cands {
        let infinite = is_infinite(e);
        if !selectable(d, e) || (infinite && !crossing) {
            continue;
        }
        let eb = match proj {
            Some(pr) => pr.project_bounds(&bounds_of(d, e, known)),
            None => bounds_of(d, e, known),
        };
        let reject = if !infinite {
            if crossing || proj.is_some() { !eb.intersects(&pb) } else { !pb.contains_box(&eb) }
        } else {
            false
        };
        if reject {
            continue;
        }
        let polys = projected_hit_polylines(d, e, tol, proj);
        if polys_hit_polygon(&polys, poly, &edges, crossing) {
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
    /// Target-plane 3D anchors corresponding 1:1 to `pts`, preserving full screen UV in 3D/edge-on views.
    pub pts_3d: Vec<Vec3>,
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
        Some(SelectMode { kind, pts: Vec::new(), pts_3d: Vec::new() })
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
    /// Add a point together with its target-plane 3D anchor. Returns whether the mode is complete.
    pub fn push_with_anchor(&mut self, p: Vec2, anchor_3d: Vec3) -> bool {
        if p.is_finite() && self.pts.len() < MAX_MODE_POINTS {
            self.pts.push(p);
            self.pts_3d.push(if anchor_3d.is_finite() { anchor_3d } else { p.to3(0.0) });
        }
        self.is_box() && self.pts.len() >= 2
    }
    /// Add a point. Returns whether the mode is complete (the second corner of W/C).
    pub fn push(&mut self, p: Vec2) -> bool {
        self.push_with_anchor(p, p.to3(0.0))
    }
    /// Remove the last entered point (Undo).
    pub fn pop(&mut self) {
        self.pts.pop();
        self.pts_3d.pop();
    }
    /// Current vertices projected into camera UV under `proj`.
    pub fn uv_points(&self, proj: &PickProjection) -> Vec<Vec2> {
        if self.pts_3d.len() == self.pts.len() {
            self.pts_3d.iter().map(|&p| proj.project_3d_point(p)).collect()
        } else {
            self.pts.iter().map(|&p| proj.project(p)).collect()
        }
    }
    /// Whether `Box` (or `Crossing`/`CPolygon`) evaluates as crossing in camera UV under `proj`.
    pub fn is_crossing_projected(&self, proj: &PickProjection) -> bool {
        match self.kind {
            SelectKind::Crossing | SelectKind::CPolygon => true,
            SelectKind::Box => {
                let uvs = self.uv_points(proj);
                uvs.first().zip(uvs.get(1)).is_some_and(|(a, b)| b.x < a.x)
            }
            _ => false,
        }
    }
    /// The objects the mode selects in unrotated 2D plan view, or `None` while it has too few points.
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
    /// The objects the mode selects under `proj`, or `None` while it has too few points.
    pub fn select_projected(&self, d: &Drawing, space: &Space, proj: &PickProjection) -> Option<Vec<Handle>> {
        if proj.is_plan_axis_aligned() {
            return self.select(d, space);
        }
        let uvs = self.uv_points(proj);
        match (self.kind, uvs.as_slice()) {
            (SelectKind::Window | SelectKind::Crossing, [a, b, ..]) => {
                Some(select_window_projected(d, space, Bounds2::new(*a, *b), self.kind == SelectKind::Crossing, proj))
            }
            (SelectKind::Box, [a, b, ..]) => Some(select_window_projected(d, space, Bounds2::new(*a, *b), b.x < a.x, proj)),
            (SelectKind::Fence, pts) if pts.len() >= 2 => Some(select_fence_projected(d, space, pts, proj)),
            (SelectKind::WPolygon | SelectKind::CPolygon, pts) if pts.len() >= 3 => {
                Some(select_polygon_projected(d, space, pts, self.kind == SelectKind::CPolygon, proj))
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

    /// Two objects under the pickbox: AutoCAD 2024 picks the one drawn last, not the nearer one,
    /// and DRAWORDER changes which (`(ssget pt)` in accoreconsole).
    #[test]
    fn a_pick_takes_the_object_drawn_last() {
        let mut d = Drawing::new_metric();
        let circle = d
            .add(&Space::Model, Common::default(), EntityKind::Circle(cadcraft_doc::Circle { center: Vec3::new(50.0, 50.0, 0.0), radius: 10.0 }))
            .unwrap();
        let line = EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(40.5, 45.0, 0.0), b: Vec3::new(40.5, 55.0, 0.0) });
        let line = d.add(&Space::Model, Common::default(), line).unwrap();
        // 0.05 from the circle and 0.45 from the line, both inside an aperture of 1.
        let p = Vec2::new(40.05, 50.0);
        assert_eq!(pick(&d, &Space::Model, p, 1.0), Some(line));
        assert_eq!(pick_all(&d, &Space::Model, p, 1.0), vec![line, circle]);
        d.model.send_to_back(line);
        assert_eq!(pick(&d, &Space::Model, p, 1.0), Some(circle));
        // Only one within reach: that one, whatever the order.
        assert_eq!(pick(&d, &Space::Model, Vec2::new(41.3, 50.0), 1.0), Some(line));
    }

    /// An OLE2FRAME draws as its frame, so a click on an edge picks it (review on #2587); a click in
    /// the middle of the frame, or on a frame without both corners, does not.
    #[test]
    fn clicking_an_ole_frame_edge_selects_it() {
        let tag = |code: i32, value: &str| cadcraft_doc::RawTag { code, value: value.into() };
        let frame = |tags: Vec<cadcraft_doc::RawTag>| EntityKind::Unknown(cadcraft_doc::Unknown { dxf_type: "OLE2FRAME".into(), tags });
        let mut d = Drawing::new_metric();
        let h = d.add(&Space::Model, Common::default(), frame(vec![tag(10, "30"), tag(20, "-19"), tag(11, "35"), tag(21, "-22")])).unwrap();
        d.add(&Space::Model, Common::default(), frame(vec![tag(10, "80"), tag(20, "80")])).unwrap();
        let line = d.add(&Space::Model, Common::default(), frame(vec![tag(10, "60"), tag(20, "-19"), tag(11, "60"), tag(21, "-22")])).unwrap();
        assert_eq!(pick(&d, &Space::Model, Vec2::new(32.5, -19.0), 0.2), Some(h), "top edge");
        assert_eq!(pick(&d, &Space::Model, Vec2::new(35.0, -20.5), 0.2), Some(h), "right edge");
        assert_eq!(pick(&d, &Space::Model, Vec2::new(32.5, -20.5), 0.2), None, "inside the frame");
        assert_eq!(pick(&d, &Space::Model, Vec2::new(80.0, 80.0), 0.2), None, "a frame without a second corner");
        assert_eq!(pick(&d, &Space::Model, Vec2::new(60.0, -20.0), 0.2), Some(line), "a frame with no width is drawn as a line and picked as one");
    }

    /// A block holding a diagonal dashed line, inserted at `scale`.
    fn dashed_block(length: f64, scale: f64) -> (Drawing, Handle) {
        let mut d = Drawing::new_imperial();
        d.linetypes.extend(cadcraft_doc::library::standard_linetypes().into_iter().filter(|l| l.name == "DASHED"));
        let mut line =
            Entity::new(Handle(1), EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(length, length, 0.0) }));
        line.common.linetype = "DASHED".into();
        let mut blk = cadcraft_doc::Block::new("B");
        blk.entities.push(line);
        d.blocks.insert("B".into(), std::sync::Arc::new(blk));
        let insert = cadcraft_doc::Insert {
            block: "B".into(),
            insert: Vec3::new(0.0, 0.0, 0.0),
            scale: Vec3::new(scale, scale, 1.0),
            rotation: 0.0,
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
            clip: None,
        };
        let h = d.add(&Space::Model, Common::default(), EntityKind::Insert(insert)).unwrap();
        (d, h)
    }

    /// Every mouse move over a block hit-tests what it draws. Two blocks with a fine pattern on
    /// long lines (inserted at 0.001) made 5.8 million dashes, and half a second, per move (#1285).
    #[test]
    fn hit_testing_a_block_does_not_lay_out_its_dashes() {
        let (d, h) = dashed_block(20_000.0, 0.001);
        let e = d.entity(h).unwrap();
        let drawn = cadcraft_render::build_entities(&d, std::iter::once(e.as_ref()), &cadcraft_render::Options { text: false, ..Default::default() });
        assert!(drawn.prims.len() > 10_000, "the pattern draws {} dashes", drawn.prims.len());
        // The line, and the block's bounding rectangle.
        assert_eq!(hit_polylines(&d, e, 0.01).len(), 2);
    }

    /// A dashed line in a block is hit across its gaps, as one outside a block is.
    #[test]
    fn a_dashed_line_in_a_block_is_picked_in_a_gap() {
        let (d, h) = dashed_block(100.0, 1.0);
        let e = d.entity(h).unwrap();
        let drawn = cadcraft_render::build_entities(&d, std::iter::once(e.as_ref()), &cadcraft_render::Options { text: false, ..Default::default() });
        let (a, b) = (drawn.prims.first().unwrap(), drawn.prims.get(1).unwrap());
        let (end, start) = (*drawn.points(a).last().unwrap(), *drawn.points(b).first().unwrap());
        assert!(end.dist(start) > 0.01, "a gap between the first two dashes");
        let gap = end + (start - end) * 0.5;
        assert_eq!(pick(&d, &Space::Model, gap, end.dist(start) / 8.0), Some(h));
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

    #[test]
    fn projected_pick_works_in_edge_on_front_and_left_views_across_depth() {
        let mut d = Drawing::new_metric();
        // Two vertical-depth lines in XY (one at Y=-500, one at Y=+500) plus a circle at (15, 200) r=4.
        let h_back = d
            .add(&Space::Model, Common::default(), EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(-5.0, -500.0, 0.0), b: Vec3::new(5.0, -500.0, 0.0) }))
            .unwrap();
        let h_front = d
            .add(&Space::Model, Common::default(), EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(2.0, 500.0, 0.0), b: Vec3::new(8.0, 500.0, 0.0) }))
            .unwrap();
        let h_circle = d
            .add(&Space::Model, Common::default(), EntityKind::Circle(cadcraft_doc::Circle { center: Vec3::new(15.0, 200.0, 0.0), radius: 4.0 }))
            .unwrap();

        let front = PickProjection::from_view(Camera3D::preset("front").unwrap(), Vec2::ZERO, 0.0, 0.0);
        // At UV = (3.0, 0.0), both h_back and h_front project to Z=0; topmost (h_front) is picked first.
        assert_eq!(pick_projected(&d, &Space::Model, Vec2::new(3.0, 0.0), 0.5, &front), Some(h_front));
        assert_eq!(pick_all_projected(&d, &Space::Model, Vec2::new(3.0, 0.0), 0.5, &front), vec![h_front, h_back]);
        // Outside h_front at UV = (-3.0, 0.0) only h_back hits.
        assert_eq!(pick_projected(&d, &Space::Model, Vec2::new(-3.0, 0.0), 0.5, &front), Some(h_back));
        // Circle at X=15, R=4 projects to horizontal span U=[11..19], V=0 in Front view.
        assert_eq!(pick_projected(&d, &Space::Model, Vec2::new(12.0, 0.0), 0.5, &front), Some(h_circle));
        // Above V=0 outside aperture misses in Front view.
        assert_eq!(pick_projected(&d, &Space::Model, Vec2::new(3.0, 2.0), 0.5, &front), None);

        // In Left view (looking along +X, so camera +U is world -Y): h_back (Y=-500) projects to U=+500,
        // h_front (Y=+500) to U=-500, and h_circle (Y=196..204) to U=-204..-196.
        let left = PickProjection::from_view(Camera3D::preset("left").unwrap(), Vec2::ZERO, 0.0, 0.0);
        assert_eq!(pick_projected(&d, &Space::Model, left.project(Vec2::new(5.0, 500.0)), 0.5, &left), Some(h_front));
        assert_eq!(pick_projected(&d, &Space::Model, left.project(Vec2::new(-2.0, -500.0)), 0.5, &left), Some(h_back));
        assert_eq!(pick_projected(&d, &Space::Model, left.project(Vec2::new(15.0, 200.0)), 0.5, &left), Some(h_circle));
    }

    #[test]
    fn projected_selection_modes_and_windows_match_view_in_edge_on_iso_and_twist() {
        let mut d = Drawing::new_metric();
        // Add >260 decoy entities to trigger the R-tree spatial index in rotated/3D views too.
        for i in 0..270 {
            let x = 1000.0 + f64::from(i) * 5.0;
            d.add(&Space::Model, Common::default(), EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(x, -10.0, 0.0), b: Vec3::new(x, 10.0, 0.0) }))
                .unwrap();
        }
        // Diagonal line (0,0)-(10,10) becomes vertical at U=0, V=[0, sqrt(200)] under +45deg twist.
        let diag = d
            .add(&Space::Model, Common::default(), EntityKind::Line(cadcraft_doc::Line { a: Vec3::ZERO, b: Vec3::new(10.0, 10.0, 0.0) }))
            .unwrap();
        let far_y = d
            .add(&Space::Model, Common::default(), EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(2.0, 400.0, 0.0), b: Vec3::new(8.0, 400.0, 0.0) }))
            .unwrap();

        // 1) DVIEW Twist = +45deg: (10, 10) rotates to (0, sqrt(200)) ≈ (0, 14.142).
        let twist_proj = PickProjection::from_view(Camera3D::TOP, Vec2::ZERO, 0.0, std::f64::consts::FRAC_PI_4);
        let narrow_uv_win = Bounds2::new(Vec2::new(-1.0, -1.0), Vec2::new(1.0, 15.0));
        assert_eq!(select_window_projected(&d, &Space::Model, narrow_uv_win, false, &twist_proj), vec![diag]);

        // 2) Front edge-on view: window across U=[1..9], V=[-1..1] captures far_y (even with R-tree present on >260 ents),
        //    and misses diag's U=[0..10] for strict Window while Crossing catches both diag and far_y.
        let front = PickProjection::from_view(Camera3D::preset("front").unwrap(), Vec2::ZERO, 0.0, 0.0);
        let uv_box = Bounds2::new(Vec2::new(1.0, -1.0), Vec2::new(9.0, 1.0));
        assert_eq!(select_window_projected(&d, &Space::Model, uv_box, false, &front), vec![far_y]);
        assert_eq!(select_window_projected(&d, &Space::Model, uv_box, true, &front), vec![diag, far_y]);

        // 3) WPolygon, CPolygon, and Fence in Front view: a diamond around (U=5, V=0) selects far_y (WP) or both (CP/Fence).
        let poly_uv = vec![Vec2::new(1.0, -2.0), Vec2::new(9.0, -2.0), Vec2::new(9.0, 2.0), Vec2::new(1.0, 2.0)];
        assert_eq!(select_polygon_projected(&d, &Space::Model, &poly_uv, false, &front), vec![far_y]);
        assert_eq!(select_polygon_projected(&d, &Space::Model, &poly_uv, true, &front), vec![diag, far_y]);
        let fence_uv = vec![Vec2::new(5.0, -2.0), Vec2::new(5.0, 2.0)];
        assert_eq!(select_fence_projected(&d, &Space::Model, &fence_uv, &front), vec![diag, far_y]);

        // 4) SelectMode with target-plane anchors works in Iso view too.
        let iso = PickProjection::from_view(Camera3D::preset("sw_isometric").unwrap(), Vec2::ZERO, 0.0, 0.0);
        let mut mode = SelectMode::from_keyword("fence").unwrap();
        let a = iso.project(Vec2::new(4.0, 6.0));
        let b = iso.project(Vec2::new(6.0, 4.0));
        mode.push_with_anchor(Vec2::ZERO, iso.unproject_target_plane(a));
        mode.push_with_anchor(Vec2::ZERO, iso.unproject_target_plane(b));
        assert!(mode.select_projected(&d, &Space::Model, &iso).unwrap().contains(&diag));
    }
}
