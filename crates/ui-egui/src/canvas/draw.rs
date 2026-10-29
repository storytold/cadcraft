use super::list::preview_options;
use super::{Bounds2, CanvasState, Color32, DisplayList, Handle, Kind, ListKey, Pos2, Rect, Rgb, Shape, Stroke, Vec2, Xf, pos2, vec2};

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

fn load_texture_from_bytes(ctx: &egui::Context, bytes: &[u8], name: &str) -> Option<egui::TextureHandle> {
    let dyn_img = image::load_from_memory(bytes).ok()?;
    let (w, h) = (dyn_img.width(), dyn_img.height());
    if w == 0 || h == 0 || w > 16384 || h > 16384 {
        return None;
    }
    let rgba = dyn_img.to_rgba8();
    let size = [w as usize, h as usize];
    let pixels = rgba.into_raw();
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
    Some(ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR))
}

#[allow(clippy::too_many_arguments)]
fn paint_image_entity(
    canvas: &mut CanvasState,
    doc: &cadcraft_doc::Drawing,
    img: &cadcraft_doc::Image,
    doc_path: Option<&str>,
    painter: &egui::Painter,
    xf: &Xf,
    ctx: &egui::Context,
    opacity: f32,
) {
    let texture = match canvas.image_textures.get(&img.path) {
        Some(tex) => tex.clone(),
        None => {
            let bytes = cadcraft_engine::cmd::image::resolve_image_bytes(doc, img, doc_path);
            let Some(bytes) = bytes else { return };
            let Some(tex) = load_texture_from_bytes(ctx, &bytes, &img.path) else { return };
            canvas.image_textures.insert(img.path.clone(), tex.clone());
            tex
        }
    };

    let corners = img.corners();
    let p_bl = xf.to_screen(corners[0]);
    let p_br = xf.to_screen(corners[1]);
    let p_tr = xf.to_screen(corners[2]);
    let p_tl = xf.to_screen(corners[3]);

    if !p_bl.is_finite() || !p_br.is_finite() || !p_tr.is_finite() || !p_tl.is_finite() {
        return;
    }

    let fade_alpha = (100u32.saturating_sub(img.fade as u32) * 255 / 100).clamp(0, 255) as f32;
    let alpha = (fade_alpha * opacity).clamp(0.0, 255.0) as u8;
    let tint = Color32::from_rgba_unmultiplied(255, 255, 255, alpha);

    let mut mesh = egui::Mesh::with_texture(texture.id());
    let i0 = mesh.vertices.len() as u32;
    mesh.vertices.push(egui::epaint::Vertex { pos: p_bl, uv: pos2(0.0, 1.0), color: tint });
    mesh.vertices.push(egui::epaint::Vertex { pos: p_br, uv: pos2(1.0, 1.0), color: tint });
    mesh.vertices.push(egui::epaint::Vertex { pos: p_tr, uv: pos2(1.0, 0.0), color: tint });
    mesh.vertices.push(egui::epaint::Vertex { pos: p_tl, uv: pos2(0.0, 0.0), color: tint });
    mesh.indices.extend([i0, i0 + 1, i0 + 2, i0, i0 + 2, i0 + 3]);
    painter.add(egui::Shape::mesh(mesh));
}

pub(super) fn paint_images(
    canvas: &mut CanvasState,
    session: &cadcraft_engine::Session,
    painter: &egui::Painter,
    xf_model: &Xf,
    xf_paper: &Xf,
    ctx: &egui::Context,
) {
    let Ok(st) = session.state() else { return };
    let doc_path = st.path.as_deref();
    let space = &st.space;
    let xf = match space {
        cadcraft_doc::Space::Model => xf_model,
        cadcraft_doc::Space::Paper(_) => xf_paper,
    };
    let Some(store) = st.doc.space(space) else { return };
    for e in store.iter() {
        if let cadcraft_doc::EntityKind::Image(img) = &e.kind {
            if let Some(layer) = st.doc.layer(&e.common.layer)
                && (layer.frozen || !layer.on)
            {
                continue;
            }
            if img.display & 1 == 0 {
                continue;
            }
            paint_image_entity(canvas, &st.doc, img, doc_path, painter, xf, ctx, 1.0);
        } else if let cadcraft_doc::EntityKind::Insert(ins) = &e.kind
            && let Some(b) = st.doc.block(&ins.block)
        {
            let m = ins.transform(b.base.xy());
            for be in b.entities.iter() {
                if let cadcraft_doc::EntityKind::Image(bimg) = &be.kind {
                    let mut ximg = bimg.clone();
                    ximg.transform(&m);
                    paint_image_entity(canvas, &st.doc, &ximg, doc_path, painter, xf, ctx, 1.0);
                }
            }
        }
    }
}

pub(super) fn paint_preview_images(
    canvas: &mut CanvasState,
    session: &cadcraft_engine::Session,
    ents: &[cadcraft_doc::Entity],
    painter: &egui::Painter,
    xf: &Xf,
    ctx: &egui::Context,
) {
    for ent in ents {
        if let cadcraft_doc::EntityKind::Image(img) = &ent.kind {
            let tex = if img.path == "preview" {
                let Some(clip_img) = &session.clipboard_image else { continue };
                let hash = {
                    use std::hash::{DefaultHasher, Hasher};
                    let mut h = DefaultHasher::new();
                    h.write(&clip_img.bytes);
                    h.finish()
                };
                if canvas.preview_hash != Some(hash)
                    && let Some(t) = load_texture_from_bytes(ctx, &clip_img.bytes, "__preview__")
                {
                    canvas.image_textures.insert("__preview__".to_string(), t);
                    canvas.preview_hash = Some(hash);
                }
                canvas.image_textures.get("__preview__").cloned()
            } else {
                canvas.image_textures.get(&img.path).cloned().or_else(|| {
                    let doc = session.doc().ok()?;
                    let doc_path = session.drawing_path();
                    let bytes = cadcraft_engine::cmd::image::resolve_image_bytes(doc, img, doc_path.as_deref())?;
                    let t = load_texture_from_bytes(ctx, &bytes, &img.path)?;
                    canvas.image_textures.insert(img.path.clone(), t.clone());
                    Some(t)
                })
            };

            let Some(texture) = tex else { continue };
            let corners = img.corners();
            let p_bl = xf.to_screen(corners[0]);
            let p_br = xf.to_screen(corners[1]);
            let p_tr = xf.to_screen(corners[2]);
            let p_tl = xf.to_screen(corners[3]);

            if !p_bl.is_finite() || !p_br.is_finite() || !p_tr.is_finite() || !p_tl.is_finite() {
                continue;
            }

            let tint = Color32::from_rgba_unmultiplied(255, 255, 255, 160);
            let mut mesh = egui::Mesh::with_texture(texture.id());
            let i0 = mesh.vertices.len() as u32;
            mesh.vertices.push(egui::epaint::Vertex { pos: p_bl, uv: pos2(0.0, 1.0), color: tint });
            mesh.vertices.push(egui::epaint::Vertex { pos: p_br, uv: pos2(1.0, 1.0), color: tint });
            mesh.vertices.push(egui::epaint::Vertex { pos: p_tr, uv: pos2(1.0, 0.0), color: tint });
            mesh.vertices.push(egui::epaint::Vertex { pos: p_tl, uv: pos2(0.0, 0.0), color: tint });
            mesh.indices.extend([i0, i0 + 1, i0 + 2, i0, i0 + 2, i0 + 3]);
            painter.add(egui::Shape::mesh(mesh));
        }
    }
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
    orbit: cadcraft_render::Orbit3d,
) {
    // Tessellate for a power-of-two zoom band so zooming mid-drag rebuilds only every 2x.
    let band = xf.scale.max(1e-300).log2().round() as i32;
    let raw_m = drag.matrix;
    let is_translation = (raw_m.a - 1.0).abs() <= 1e-12
        && raw_m.b.abs() <= 1e-12
        && raw_m.c.abs() <= 1e-12
        && (raw_m.d - 1.0).abs() <= 1e-12;
    let pre_transform = !orbit.is_top() && !is_translation;
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (c.key, band, bg.0, bg.1, bg.2, &drag.handles, transparency, orbit.hash_key()).hash(&mut h);
        if pre_transform {
            [raw_m.a, raw_m.b, raw_m.c, raw_m.d, raw_m.e, raw_m.f].map(f64::to_bits).hash(&mut h);
        }
        if drag.clipboard {
            clipboard.1.hash(&mut h);
        }
        h.finish()
    };
    if c.drag_key != Some(key) {
        let band_scale = 2f64.powi(band);
        let opts = cadcraft_render::Options { transparency, orbit, ..preview_options(band_scale) };
        let list = if pre_transform {
            let transformed: Vec<cadcraft_doc::Entity> = if drag.clipboard {
                clipboard
                    .0
                    .iter()
                    .map(|e| {
                        let mut e = e.clone();
                        e.kind.transform(&raw_m);
                        e
                    })
                    .collect()
            } else {
                drag.handles
                    .iter()
                    .filter_map(|h| doc.entity(*h))
                    .map(|e| {
                        let mut e = (**e).clone();
                        e.kind.transform(&raw_m);
                        e
                    })
                    .collect()
            };
            cadcraft_render::build_entities(doc, transformed.iter(), &opts)
        } else if drag.clipboard {
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
    let o = c.drag_origin;
    let xform = if pre_transform {
        crate::gpu::IDENTITY_XFORM
    } else {
        let m = orbit.project_mat3(raw_m);
        let t = m.apply(o) - o;
        [m.a as f32, m.b as f32, m.c as f32, m.d as f32, t.x as f32, t.y as f32]
    };
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
            xform,
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
    let line = |pts: Vec<Pos2>, out: &mut Vec<Shape>| {
        if look.dashed {
            out.extend(Shape::dashed_line(&pts, stroke, 6.0, 4.0));
        } else {
            out.push(Shape::line(pts, stroke));
        }
    };
    let mut shapes = Vec::new();
    let mut fill = egui::Mesh::default();
    let fill_color = color.gamma_multiply(crate::gpu::HIGHLIGHT_FILL_ALPHA);
    for prim in prims.iter().filter_map(|&i| list.prims.get(i)) {
        let pts = list.points(prim);
        match prim.kind {
            Kind::Polyline => line(pts.iter().map(|q| xf.to_screen(*q)).collect(), &mut shapes),
            Kind::Infinite { ray } => {
                if let (Some(b), Some(d)) = (pts.first(), pts.get(1))
                    && let Some(seg) = clip_infinite(xf, *b, *d, ray)
                {
                    line(seg.to_vec(), &mut shapes);
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
                    line(outline.iter().map(|q| xf.to_screen(*q)).collect(), &mut shapes);
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
