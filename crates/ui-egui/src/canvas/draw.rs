use super::list::preview_options;
use super::{Bounds2, CanvasState, Color32, DisplayList, Handle, Kind, ListKey, Pos2, Rect, Rgb, Shape, Stroke, Vec2, Xf, vec2};

fn color32(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

/// A primitive's colour on `bg`, with its transparency.
fn prim_color32(prim: &cadcraft_render::DPrim, bg: Rgb) -> Color32 {
    let c = prim.display_rgb(bg);
    Color32::from_rgba_unmultiplied(c.0, c.1, c.2, prim.alpha())
}

fn clip_infinite(xf: &Xf, base: Vec2, dir: Vec2, ray: bool) -> Option<[Pos2; 2]> {
    let wb = xf.world_bounds();
    let far = (wb.width() + wb.height()) * 2.0 + base.dist(wb.center());
    let a = if ray { base } else { base - dir * far };
    let b = base + dir * far;
    Some([xf.to_screen(a), xf.to_screen(b)])
}

/// Points past the view's edge that a dashed outline is still drawn to: more than one dash
/// period, so a dash cut at the clip edge never shows.
const CLIP_MARGIN: f32 = 16.0;

/// The parts of a world-space polyline within `xf`'s view, as screen points, each with how far
/// along the whole polyline it starts (in points). Clipped in world coordinates, so an outline far
/// larger than the screen costs only what can show, and its points stay precise when zoomed far
/// in: dashing a whole line made billions of dashes (#2299).
fn visible_runs(xf: &Xf, pts: &[Vec2]) -> Vec<(Vec<Pos2>, f64)> {
    let vis = xf.world_bounds().expand(f64::from(CLIP_MARGIN) / xf.scale);
    let mut runs: Vec<(Vec<Pos2>, f64)> = Vec::new();
    // World length before the current segment; whether the last run reaches its start.
    let (mut along, mut open) = (0.0, false);
    for w in pts.windows(2) {
        let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
        match cadcraft_render::clip::clip_segment(a, b, &vis) {
            Some((p, q)) => {
                match runs.last_mut() {
                    Some((run, _)) if open && p == a => run.push(xf.to_screen(q)),
                    _ => runs.push((vec![xf.to_screen(p), xf.to_screen(q)], (along + a.dist(p)) * xf.scale)),
                }
                open = q == b;
            }
            None => open = false,
        }
        along += a.dist(b);
    }
    runs
}

/// Dashes along a screen polyline that starts `along` points into the outline it is part of, so
/// they sit where they would on the whole outline.
fn dashes(run: &[Pos2], along: f64, stroke: Stroke, dash: f32, gap: f32, out: &mut Vec<Shape>) {
    let period = f64::from(dash + gap);
    let offset = (period - along.rem_euclid(period)) % period;
    out.extend(Shape::dashed_line_with_offset(run, stroke, &[dash], &[gap], offset as f32));
}

/// A dashed rectangle outline, each edge clipped to `clip` first: a rectangle zoomed far larger
/// than the screen (a layout's printable area, a selection window) gets only the dashes that can
/// show (#2299). Each edge keeps its direction and its dashes start where they would on the whole
/// edge, so they don't crawl as the rectangle or the view moves.
pub(super) fn dashed_rect(clip: Rect, r: Rect, stroke: Stroke, dash: f32, gap: f32) -> Vec<Shape> {
    let v = |p: Pos2| Vec2::new(f64::from(p.x), f64::from(p.y));
    let c = clip.expand(CLIP_MARGIN);
    let c = Bounds2::new(v(c.min), v(c.max));
    let mut out = Vec::new();
    let edges =
        [(r.left_top(), r.right_top()), (r.right_top(), r.right_bottom()), (r.right_bottom(), r.left_bottom()), (r.left_bottom(), r.left_top())];
    for (a, b) in edges {
        if let Some((p, q)) = cadcraft_render::clip::clip_segment(v(a), v(b), &c) {
            let pos = |w: Vec2| Pos2::new(w.x as f32, w.y as f32);
            dashes(&[pos(p), pos(q)], v(a).dist(p), stroke, dash, gap, &mut out);
        }
    }
    out
}

/// Draw a display list.
pub(crate) fn draw_list(p: &egui::Painter, xf: &Xf, list: &DisplayList, bg: Rgb, lwdisplay: bool) {
    let vis = xf.world_bounds();
    let mut shapes: Vec<Shape> = Vec::with_capacity(list.prims.len());
    let px = 1.0 / xf.scale;
    for prim in &list.prims {
        let col = prim_color32(prim, bg);
        let pts = list.points(prim);
        match prim.kind {
            Kind::Polyline => {
                let bb = Bounds2::from_points(pts.iter().copied());
                if !bb.intersects(&vis) {
                    continue;
                }
                let w = if lwdisplay && prim.lw > 0.0 { (prim.lw * 3.78).clamp(1.0, 12.0) } else { 1.0 };
                // Skip sub-pixel objects except as a dot.
                if bb.width() < px && bb.height() < px {
                    shapes.push(Shape::rect_filled(Rect::from_center_size(xf.to_screen(bb.center()), vec2(1.0, 1.0)), 0.0, col));
                    continue;
                }
                let screen: Vec<Pos2> = pts.iter().map(|q| xf.to_screen(*q)).collect();
                if screen.len() == 2 {
                    if let [a, b] = screen.as_slice() {
                        shapes.push(Shape::line_segment([*a, *b], Stroke::new(w, col)));
                    }
                } else {
                    shapes.push(Shape::line(screen, Stroke::new(w, col)));
                }
            }
            Kind::Tris | Kind::Mask => {
                // A wipeout's mask hides what was drawn before it with the background colour.
                let col = if prim.kind == Kind::Mask { color32(bg) } else { col };
                let mut mesh = egui::Mesh::default();
                for tri in pts.chunks(3) {
                    if let [a, b, c] = tri {
                        let i = mesh.vertices.len() as u32;
                        for q in [a, b, c] {
                            mesh.colored_vertex(xf.to_screen(*q), col);
                        }
                        mesh.add_triangle(i, i + 1, i + 2);
                    }
                }
                if !mesh.vertices.is_empty() {
                    shapes.push(Shape::mesh(mesh));
                }
            }
            Kind::Point => {
                if let Some(q) = pts.first()
                    && vis.contains(*q)
                {
                    shapes.push(Shape::rect_filled(Rect::from_center_size(xf.to_screen(*q), vec2(1.5, 1.5)), 0.0, col));
                }
            }
            Kind::Infinite { ray } => {
                if let (Some(b), Some(d)) = (pts.first(), pts.get(1))
                    && let Some(seg) = clip_infinite(xf, *b, *d, ray)
                {
                    shapes.push(Shape::line_segment(seg, Stroke::new(1.0, col)));
                }
            }
        }
    }
    p.extend(shapes);
}

/// Draw the selection highlight (as `look` says) through the GPU: its vertex data is
/// rebuilt only when the display list, the selection or the vertex origin changes. Dashing every
/// selected line and tessellating it as egui shapes each frame made large selections sluggish.
/// `prims`: the selection's primitives in `list`, asked for only when the vertex data is rebuilt.
/// `stencil`: [`crate::gpu::Stencil::Inside`] keeps it inside a clipped viewport's outline.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_highlight_gpu(
    p: &egui::Painter,
    xf: &Xf,
    list: &DisplayList,
    list_gen: u64,
    prims: impl FnOnce() -> Vec<usize>,
    sel: &[Handle],
    color: Color32,
    look: Look,
    hl: (&mut Option<u64>, &crate::gpu::MeshSlot, &mut Vec2, &mut Vec<usize>),
    stencil: crate::gpu::Stencil,
) {
    let (hl_key, hl_slot, hl_origin, hl_infinite) = hl;
    let origin = crate::gpu::choose_origin(&list.bounds, xf.center, f64::from(xf.rect.height()) / xf.scale.max(1e-300));
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (list_gen, sel, origin.x.to_bits(), origin.y.to_bits(), color.to_array(), look.width.to_bits(), look.dashed).hash(&mut h);
        h.finish()
    };
    if *hl_key != Some(key) {
        let prims = prims();
        let mesh = crate::gpu::build_highlight_mesh(list, &prims, origin, color.to_array(), look.width, look.dashed);
        *hl_infinite = prims.into_iter().filter(|&i| list.prims.get(i).is_some_and(|p| matches!(p.kind, Kind::Infinite { .. }))).collect();
        if let Ok(mut slot) = hl_slot.lock() {
            *slot = Some(mesh);
        }
        *hl_key = Some(key);
        *hl_origin = origin;
    }
    let center = xf.rect.center();
    p.add(egui_wgpu::Callback::new_paint_callback(
        xf.rect,
        crate::gpu::CanvasCallback {
            key,
            slot: hl_slot.clone(),
            offset: [(hl_origin.x - xf.center.x) as f32, (hl_origin.y - xf.center.y) as f32],
            scale: xf.scale as f32,
            center: [center.x, center.y],
            layer: crate::gpu::LAYER_HIGHLIGHT,
            xform: crate::gpu::IDENTITY_XFORM,
            dash: if look.dashed { 10.0 } else { 0.0 },
            stencil,
        },
    ));
}

/// Draw a MOVE/COPY/ROTATE/SCALE preview the way the drawing itself is drawn: the dragged
/// objects are tessellated and uploaded once (again only when the drawing, the selection or the
/// zoom band changes), and each frame only the transform in the view uniform moves. Rebuilding
/// the transformed copies as egui shapes every frame was slow with heavy selections and could
/// overflow the GPU buffer limit.
/// `clipboard` is the session's clipboard and its serial (dragged when `drag.clipboard`, by PASTE).
/// `transparency`: TRANSPARENCYDISPLAY (dragged objects keep their transparency).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_drag_gpu(
    c: &mut CanvasState,
    p: &egui::Painter,
    xf: &Xf,
    doc: &cadcraft_doc::Drawing,
    drag: &cadcraft_engine::Drag,
    clipboard: (&[cadcraft_doc::Entity], u64),
    bg: Rgb,
    transparency: bool,
) {
    // Tessellate for a power-of-two zoom band so zooming mid-drag rebuilds only every 2x.
    let band = xf.scale.max(1e-300).log2().round() as i32;
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (c.key, band, bg.0, bg.1, bg.2, &drag.handles, transparency).hash(&mut h);
        if drag.clipboard {
            clipboard.1.hash(&mut h);
        }
        h.finish()
    };
    if c.drag_key != Some(key) {
        let band_scale = 2f64.powi(band);
        let opts = cadcraft_render::Options { transparency, ..preview_options(band_scale) };
        let list = if drag.clipboard {
            cadcraft_render::build_entities(doc, clipboard.0.iter(), &opts)
        } else {
            cadcraft_render::build_entities(doc, drag.handles.iter().filter_map(|h| doc.entity(*h)).map(|e| e.as_ref()), &opts)
        };
        let origin = if list.bounds.is_empty() { Vec2::ZERO } else { list.bounds.center() };
        let mesh = crate::gpu::build_mesh(&list, origin, bg, false);
        if let Ok(mut slot) = c.drag_slot.lock() {
            *slot = Some(mesh);
        }
        c.drag_key = Some(key);
        c.drag_origin = origin;
    }
    // Positions are relative to the origin o: M(o + p) = L·p + (M(o) − o).
    let m = drag.matrix;
    let o = c.drag_origin;
    let t = m.apply(o) - o;
    let center = xf.rect.center();
    p.add(egui_wgpu::Callback::new_paint_callback(
        xf.rect,
        crate::gpu::CanvasCallback {
            key,
            slot: c.drag_slot.clone(),
            offset: [(o.x - xf.center.x) as f32, (o.y - xf.center.y) as f32],
            scale: xf.scale as f32,
            center: [center.x, center.y],
            layer: crate::gpu::LAYER_DRAG,
            xform: [m.a as f32, m.b as f32, m.c as f32, m.d as f32, t.x as f32, t.y as f32],
            dash: 0.0,
            stencil: crate::gpu::Stencil::Off,
        },
    ));
}

/// Identifies the GPU vertex data of list `key` / `list_gen` converted at `origin` against `bg`.
pub(super) fn list_mesh_key(key: &ListKey, list_gen: u64, origin: Vec2, bg: Rgb, lwdisplay: bool) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (key, list_gen, origin.x.to_bits(), origin.y.to_bits(), bg.0, bg.1, bg.2, lwdisplay).hash(&mut h);
    h.finish()
}

/// Draw the display list through the GPU paint callback (uploading it when it changed), plus
/// the infinite lines, which are clipped to the view on the CPU every frame.
pub(super) fn draw_list_gpu(c: &mut CanvasState, p: &egui::Painter, xf: &Xf, bg: Rgb, lwdisplay: bool) {
    let Some(list) = &c.list else { return };
    let origin = crate::gpu::choose_origin(&list.bounds, xf.center, f64::from(xf.rect.height()) / xf.scale.max(1e-300));
    let key = list_mesh_key(&c.key, c.list_gen, origin, bg, lwdisplay);
    if c.mesh_key != Some(key) {
        let t0 = crate::now_ms();
        let mesh = crate::gpu::build_mesh(list, origin, bg, lwdisplay);
        if let Ok(mut slot) = c.mesh_slot.lock() {
            *slot = Some(mesh);
        }
        c.infinite = list.prims.iter().enumerate().filter(|(_, pr)| matches!(pr.kind, Kind::Infinite { .. })).map(|(i, _)| i).collect();
        c.mesh_key = Some(key);
        c.mesh_origin = origin;
        c.mesh_ms = crate::now_ms() - t0;
    }
    let center = xf.rect.center();
    p.add(egui_wgpu::Callback::new_paint_callback(
        xf.rect,
        crate::gpu::CanvasCallback {
            key,
            slot: c.mesh_slot.clone(),
            offset: [(c.mesh_origin.x - xf.center.x) as f32, (c.mesh_origin.y - xf.center.y) as f32],
            scale: xf.scale as f32,
            center: [center.x, center.y],
            layer: crate::gpu::LAYER_DRAWING,
            xform: crate::gpu::IDENTITY_XFORM,
            dash: 0.0,
            stencil: crate::gpu::Stencil::Off,
        },
    ));
    let mut shapes = Vec::new();
    for prim in c.infinite.iter().filter_map(|i| list.prims.get(*i)) {
        let pts = list.points(prim);
        if let (Kind::Infinite { ray }, Some(b), Some(d)) = (prim.kind, pts.first(), pts.get(1))
            && let Some(seg) = clip_infinite(xf, *b, *d, ray)
        {
            shapes.push(Shape::line_segment(seg, Stroke::new(1.0, prim_color32(prim, bg))));
        }
    }
    p.extend(shapes);
}

/// How a highlight is drawn: lines `width` points wide, and either solid with fills filled or
/// dashed with fills outlined.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Look {
    pub width: f32,
    pub dashed: bool,
}

impl Look {
    /// Selected objects (HIGHLIGHT, SELECTIONEFFECT); `None`: not highlighted.
    pub(crate) fn selection(s: &cadcraft_engine::Settings) -> Option<Look> {
        s.highlight.then_some(Look { width: 1.5, dashed: s.selectioneffect == 0 })
    }
    /// The object under the cursor (PREVIEWEFFECT: 0 dashed, 1 thicker, 2 both).
    pub(crate) fn preview(s: &cadcraft_engine::Settings) -> Look {
        Look { width: if s.previeweffect == 0 { 1.0 } else { 2.0 }, dashed: s.previeweffect != 1 }
    }
}

/// Draw selected/hovered entities as highlight overlays: `prims` are indices into `list` (from
/// [`PrimIndex::prims`], [`prims_of`], or all of a [`VpHighlight`] list), redrawn in `color` as `look` says. A wipeout is always
/// outlined, since filling it would hide the drawing under it.
pub(crate) fn draw_highlight(p: &egui::Painter, xf: &Xf, list: &DisplayList, prims: &[usize], color: Color32, look: Look) {
    let stroke = Stroke::new(look.width, color);
    let screen_line = |pts: Vec<Pos2>, along: f64, out: &mut Vec<Shape>| {
        if look.dashed {
            dashes(&pts, along, stroke, 6.0, 4.0, out);
        } else {
            out.push(Shape::line(pts, stroke));
        }
    };
    // Only the part of an outline in view (#2299).
    let line = |pts: &[Vec2], out: &mut Vec<Shape>| {
        for (run, along) in visible_runs(xf, pts) {
            screen_line(run, along, out);
        }
    };
    let mut shapes = Vec::new();
    let mut fill = egui::Mesh::default();
    let fill_color = color.gamma_multiply(crate::gpu::HIGHLIGHT_FILL_ALPHA);
    for prim in prims.iter().filter_map(|&i| list.prims.get(i)) {
        let pts = list.points(prim);
        match prim.kind {
            Kind::Polyline => line(pts, &mut shapes),
            Kind::Infinite { ray } => {
                if let (Some(b), Some(d)) = (pts.first(), pts.get(1))
                    && let Some(seg) = clip_infinite(xf, *b, *d, ray)
                {
                    screen_line(seg.to_vec(), 0.0, &mut shapes);
                }
            }
            Kind::Point => {
                if let Some(q) = pts.first() {
                    shapes.push(Shape::circle_stroke(xf.to_screen(*q), 3.0, Stroke::new(1.0, color)));
                }
            }
            Kind::Tris if !look.dashed => {
                for q in pts.as_chunks::<3>().0.iter().flatten() {
                    let i = fill.vertices.len() as u32;
                    fill.colored_vertex(xf.to_screen(*q), fill_color);
                    fill.indices.push(i);
                }
            }
            Kind::Tris | Kind::Mask => {
                for outline in list.fill_outlines(prim) {
                    line(&outline, &mut shapes);
                }
            }
        }
    }
    if !fill.is_empty() {
        shapes.push(Shape::mesh(fill));
    }
    p.extend(shapes);
}

/// The fill of a selection window or crossing area (SELECTIONAREA, WINDOWAREACOLOR,
/// CROSSINGAREACOLOR, SELECTIONAREAOPACITY); `None` when areas aren't filled.
pub(super) fn selection_area_fill(s: &cadcraft_engine::Settings, crossing: bool) -> Option<Color32> {
    let aci = if crossing { s.crossingareacolor } else { s.windowareacolor };
    let c = cadcraft_color::aci_rgb(aci);
    s.selectionarea.then(|| Color32::from_rgb(c.0, c.1, c.2).gamma_multiply(f32::from(s.selectionareaopacity.min(100)) / 100.0))
}

#[cfg(test)]
mod highlight_stability_tests {
    use super::*;
    use egui::pos2;

    fn frame(list: &DisplayList, look: Look, pointer: Pos2) -> Vec<egui::epaint::ClippedShape> {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0));
        let input = egui::RawInput { screen_rect: Some(rect), events: vec![egui::Event::PointerMoved(pointer)], ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| {
            draw_highlight(ui.painter(), &Xf { rect, center: Vec2::ZERO, scale: 11.0 }, list, &[0], Color32::LIGHT_BLUE, look);
        });
        let shapes = std::mem::take(&mut output.shapes);
        output.textures_delta.clear();
        shapes
    }

    // Compare visible dash segments, not their submission order or endpoint direction.
    fn dash_positions(shapes: &[egui::epaint::ClippedShape]) -> Vec<((u32, u32), (u32, u32))> {
        let mut segments: Vec<_> = shapes
            .iter()
            .filter_map(|s| match &s.shape {
                Shape::LineSegment { points: [a, b], stroke } if stroke.color == Color32::LIGHT_BLUE => {
                    let (a, b) = ((a.x.to_bits(), a.y.to_bits()), (b.x.to_bits(), b.y.to_bits()));
                    Some(if a < b { (a, b) } else { (b, a) })
                }
                _ => None,
            })
            .collect();
        segments.sort_unstable();
        segments
    }

    #[test]
    fn moving_the_pointer_does_not_move_cpu_fill_highlight_dashes() {
        let p = |x, y| Vec2::new(x, y);
        let mut list = DisplayList { tris: vec![p(0.0, 0.0), p(7.0, 0.0), p(7.0, 3.0), p(0.0, 0.0), p(7.0, 3.0), p(0.0, 3.0)], ..Default::default() };
        list.prims.push(cadcraft_render::DPrim {
            handle: Handle(256),
            color: Rgb(255, 255, 255),
            aci7: true,
            lw: 0.0,
            transparency: 0,
            kind: Kind::Tris,
            start: 0,
            len: 6,
        });
        // Filled glyphs, hatches, solids and arrowheads share the Tris path; wipeouts use Mask.
        for kind in [Kind::Tris, Kind::Mask] {
            list.prims.first_mut().unwrap().kind = kind;
            for width in [1.0, 1.5, 2.0] {
                let look = Look { width, dashed: true };
                let expected = dash_positions(&frame(&list, look, pos2(320.0, 190.0)));
                assert!(!expected.is_empty());
                for i in 0..32 {
                    assert_eq!(
                        dash_positions(&frame(&list, look, pos2(320.0 + i as f32, 190.0))),
                        expected,
                        "pointer movement changes only the cursor, not the dashed contour"
                    );
                }
            }
        }
        if let Some(dir) = std::env::var_os("CADCRAFT_HIGHLIGHT_SHOTS") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            let mut h = egui_kittest::Harness::builder().with_size(vec2(600.0, 400.0)).build_ui(|ui| {
                let xf = Xf { rect: ui.max_rect(), center: Vec2::new(3.5, 1.5), scale: 31.0 };
                draw_highlight(ui.painter(), &xf, &list, &[0], Color32::LIGHT_BLUE, Look { width: 2.0, dashed: true });
            });
            h.run_steps(3);
            h.event(egui::Event::PointerMoved(pos2(250.0, 190.0)));
            h.run_steps(2);
            // Harness::render adds a synthetic cursor; omit it from the pixel comparison.
            h.remove_cursor();
            h.run_steps(1);
            let first = h.render().unwrap();
            first.save(dir.join("highlight-pointer-a.png")).unwrap();
            h.event(egui::Event::PointerMoved(pos2(320.0, 190.0)));
            h.run_steps(2);
            h.remove_cursor();
            h.run_steps(1);
            let second = h.render().unwrap();
            second.save(dir.join("highlight-pointer-b.png")).unwrap();
            let changed = first.pixels().zip(second.pixels()).filter(|(a, b)| a != b).count();
            assert_eq!(changed, 0, "moving the pointer cannot change any highlight pixels");
        }
    }
}

#[cfg(test)]
mod zoomed_highlight_tests {
    use super::*;
    use egui::pos2;

    /// A one-segment polyline from `a` to `b`.
    fn line(a: Vec2, b: Vec2) -> DisplayList {
        let mut list = DisplayList { verts: vec![a, b], ..Default::default() };
        list.prims.push(cadcraft_render::DPrim {
            handle: Handle(256),
            color: Rgb(255, 255, 255),
            aci7: true,
            lw: 0.0,
            transparency: 0,
            kind: Kind::Polyline,
            start: 0,
            len: 2,
        });
        list
    }

    const SCREEN: Rect = Rect { min: Pos2::ZERO, max: Pos2 { x: 600.0, y: 400.0 } };

    /// The dash segments `draw_highlight` makes for `list` in `xf`.
    fn dashes_of(list: &DisplayList, xf: &Xf) -> Vec<[Pos2; 2]> {
        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(SCREEN), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| draw_highlight(ui.painter(), xf, list, &[0], Color32::LIGHT_BLUE, Look { width: 1.0, dashed: true }));
        output.textures_delta.clear();
        output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                Shape::LineSegment { points, .. } => Some(*points),
                _ => None,
            })
            .collect()
    }

    /// Zoomed far in on a line, the highlight dashes only what shows. Dashing the whole line made
    /// one shape per 10 points of it: billions at the zoom limit, so the app froze and then ran out
    /// of memory (#2299).
    #[test]
    fn a_zoomed_in_highlight_dashes_only_what_shows() {
        let list = line(Vec2::new(-1000.0, 0.0), Vec2::new(1000.0, 0.0));
        // The smaller zoom first, so a regression fails here instead of filling memory at 1e9.
        for scale in [1e3, 1e9] {
            let n = dashes_of(&list, &Xf { rect: SCREEN, center: Vec2::new(0.3, 0.0), scale }).len();
            assert!((50..=70).contains(&n), "scale {scale}: {n} dashes for 600 points of line");
        }
    }

    /// A line running off screen keeps its dashes where they are on the whole line, so they don't
    /// crawl along it when the view pans.
    #[test]
    fn clipped_dashes_stay_where_the_whole_line_puts_them() {
        let list = line(Vec2::new(0.0, 0.0), Vec2::new(1000.0, 0.0));
        for x in [500.3, 512.9, 731.0] {
            let xf = Xf { rect: SCREEN, center: Vec2::new(x, 0.0), scale: 1.0 };
            let whole = Shape::dashed_line(
                &[xf.to_screen(Vec2::ZERO), xf.to_screen(Vec2::new(1000.0, 0.0))],
                Stroke::new(1.0, Color32::LIGHT_BLUE),
                6.0,
                4.0,
            );
            let on_screen = |p: &[Pos2; 2]| p.iter().all(|q| SCREEN.contains(*q));
            let expected: Vec<[Pos2; 2]> = whole
                .iter()
                .filter_map(|s| match s {
                    Shape::LineSegment { points, .. } => Some(*points).filter(on_screen),
                    _ => None,
                })
                .collect();
            let got: Vec<[Pos2; 2]> = dashes_of(&list, &xf).into_iter().filter(on_screen).collect();
            assert!(!expected.is_empty());
            assert_eq!(got.len(), expected.len(), "centre {x}");
            for (g, e) in got.iter().zip(&expected) {
                assert!(g.iter().zip(e).all(|(a, b)| a.distance(*b) < 1e-3), "centre {x}: {g:?} vs {e:?}");
            }
        }
    }

    /// A rectangle edge running off screen keeps its dashes where they are on the whole edge.
    #[test]
    fn clipped_rectangle_edges_keep_their_dashes() {
        let stroke = Stroke::new(1.0, Color32::GRAY);
        let segments = |shapes: Vec<Shape>| -> Vec<[Pos2; 2]> {
            shapes
                .into_iter()
                .filter_map(|s| match s {
                    // The top edge's dashes that are wholly on screen.
                    Shape::LineSegment { points, .. } if points.iter().all(|q| SCREEN.contains(*q) && q.y == 100.0) => Some(points),
                    _ => None,
                })
                .collect()
        };
        for left in [-500.3, -517.9, -1200.0] {
            let r = Rect::from_min_max(pos2(left, 100.0), pos2(400.0, 300.0));
            let expected = segments(Shape::dashed_line(&[r.left_top(), r.right_top()], stroke, 4.0, 4.0));
            let got = segments(dashed_rect(SCREEN, r, stroke, 4.0, 4.0));
            assert!(!expected.is_empty());
            assert_eq!(got.len(), expected.len(), "left edge at {left}");
            for (g, e) in got.iter().zip(&expected) {
                assert!(g.iter().zip(e).all(|(a, b)| a.distance(*b) < 1e-3), "left edge at {left}: {g:?} vs {e:?}");
            }
        }
    }

    /// A dashed rectangle far larger than the screen gets dashes only along the edges that show.
    #[test]
    fn a_huge_dashed_rectangle_dashes_only_what_shows() {
        let stroke = Stroke::new(1.0, Color32::GRAY);
        // Its left edge runs through the screen; the other three are far off it. The smaller one
        // first, so a regression fails here instead of filling memory with the larger one.
        for far in [1e6, 1e9] {
            let huge = Rect::from_min_max(pos2(300.0, -far), pos2(far, far));
            let n = dashed_rect(SCREEN, huge, stroke, 4.0, 4.0).len();
            assert!((50..=60).contains(&n), "{far}: {n} dashes along 432 points of edge");
        }
        // A rectangle inside the screen keeps all four edges.
        let small = Rect::from_min_max(pos2(100.0, 100.0), pos2(180.0, 140.0));
        assert_eq!(dashed_rect(SCREEN, small, stroke, 4.0, 4.0).len(), 10 + 5 + 10 + 5);
        // Off screen, nothing.
        assert!(dashed_rect(SCREEN, small.translate(egui::vec2(5000.0, 0.0)), stroke, 4.0, 4.0).is_empty());
    }
}
