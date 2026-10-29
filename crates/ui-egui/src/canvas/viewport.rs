use super::draw::draw_list;
use super::{Bounds2, CadApp, DisplayList, Handle, Kind, Rect, Rgb, Vec2, Xf};

/// The active MSPACE viewport when its contents are drawn apart from the layout's list (see
/// [`VpContent`]), with its clip outline when it is clipped. A clipped viewport is clipped with
/// the GPU's stencil buffer (`stencil`); without one its contents stay in the list.
pub(super) fn split_viewport(
    st: &cadcraft_engine::DocState,
    stencil: bool,
) -> Option<(Handle, cadcraft_doc::Viewport, Option<cadcraft_render::clip::PolyClip>)> {
    let (h, vp) = st.active_viewport()?;
    let clip = cadcraft_render::viewport_clip(&st.doc, h, &vp);
    (clip.is_none() || stencil).then_some((h, vp, clip))
}

/// Whether the canvas can clip a viewport to any outline (a GPU with a stencil buffer).
pub(super) fn stencil_clip(app: &CadApp) -> bool {
    app.canvas.gpu.is_some_and(|g| g.stencil.is_some())
}

/// How long the view inside the active viewport stays still before a zoomed [`VpContent`] is
/// rebuilt at the exact view (dash lengths and curve detail follow the viewport's scale).
const VP_SETTLE_MS: f64 = 150.0;

/// Model space seen through the active MSPACE viewport, drawn apart from the layout's list. It is
/// built for an area around the viewport at one view and drawn clipped to the viewport, moved
/// and scaled to the current view, so panning and zooming inside the viewport redraw without
/// rebuilding anything. It is rebuilt when the drawing changes, when the zoom has moved 2x from
/// the view it was built for, when the view leaves the area, and once a zoom comes to rest.
#[derive(Default)]
pub(super) struct VpContent {
    list: DisplayList,
    /// What the list was built from: drawing, revision, document, viewport, tessellation band
    /// and lineweight display.
    pub(super) key: Option<(u64, u64, usize, Handle, i32, bool, bool)>,
    /// The viewport's view (centre, height) the list was built for, and the paper area it covers.
    view: (Vec2, f64),
    area: Bounds2,
    /// The view on the previous frame and when it last changed (ms).
    last_view: (Vec2, f64),
    moved_at: f64,
    /// Scale and offset drawing the list at the viewport's current view (see [`vp_motion`]).
    motion: Option<(f64, Vec2)>,
    /// Bumped on each rebuild.
    pub(super) builds: u64,
    mesh_key: Option<u64>,
    mesh_slot: crate::gpu::MeshSlot,
    mesh_origin: Vec2,
    /// A clipped viewport's outline on the GPU (see [`draw_vp_mask`]).
    mask_key: Option<u64>,
    mask_slot: crate::gpu::MeshSlot,
    mask_origin: Vec2,
}

impl VpContent {
    /// Where the contents are on paper at the viewport's current view.
    pub(super) fn paper_bounds(&self) -> Option<Bounds2> {
        let (k, b) = self.motion?;
        (!self.list.bounds.is_empty()).then(|| Bounds2::new(self.list.bounds.min * k + b, self.list.bounds.max * k + b))
    }
}

/// How a list built for view `built` (centre, height) of viewport `vp` maps to its current
/// view, as `p = k·p0 + b` on paper. A model point m is at p0 = c + s0 (m − c0) in the list and
/// belongs at p = c + s (m − vc), with c the viewport's centre and s, s0 paper units per model
/// unit, so k = s / s0 and b = c (1 − k) + s (c0 − vc).
fn vp_motion(vp: &cadcraft_doc::Viewport, built: (Vec2, f64)) -> Option<(f64, Vec2)> {
    let (s, k) = (vp.height / vp.view_height, built.1 / vp.view_height);
    let b = vp.center.xy() * (1.0 - k) + (built.0 - vp.view_center) * s;
    (k.is_finite() && k > 0.0 && b.is_finite()).then_some((k, b))
}

/// How to draw the active viewport's [`VpContent`] this frame.
pub(super) struct VpFrame {
    /// Draws the list at the viewport's current view.
    xf: Xf,
    /// The viewport's rectangle (a clipped one's outline bounds) on screen.
    pub(super) on_screen: Rect,
    /// A clipped viewport's outline on paper, masked with the stencil buffer.
    pub(super) outline: Option<Vec<Vec2>>,
}

/// Build or reuse the active viewport's [`VpContent`] for this frame.
pub(super) fn ensure_vp_content(app: &mut CadApp, xf_paper: &Xf, ctx: &egui::Context) -> Option<VpFrame> {
    let (lwdisplay, transparency) = (app.session.settings.lwdisplay, app.session.settings.transparency_display);
    let stencil = stencil_clip(app);
    let c = &mut app.canvas.vp;
    let Some((st, (h, vp, clip))) = app.session.state().ok().and_then(|st| Some((st, split_viewport(st, stencil)?))) else {
        // Not working in a viewport drawn apart: let the geometry go.
        if c.key.is_some() {
            *c = VpContent::default();
        }
        return None;
    };
    let half = Vec2::new(vp.width / 2.0, vp.height / 2.0);
    let rect = match &clip {
        Some(p) => p.bounds(),
        None => Bounds2::new(vp.center.xy() - half, vp.center.xy() + half),
    };
    let px = 1.0 / xf_paper.scale.max(1e-300);
    let band = px.max(1e-300).log2().floor() as i32;
    let key = (st.uid, st.revision, std::sync::Arc::as_ptr(&st.doc) as usize, h, band, lwdisplay, transparency);
    let now = crate::now_ms();
    let view = (vp.view_center, vp.view_height);
    if view != c.last_view {
        c.last_view = view;
        c.moved_at = now;
    }
    let rebuild = match vp_motion(&vp, c.view).filter(|_| c.key == Some(key)) {
        Some((k, b)) => {
            // The viewport in the list's own coordinates.
            let visible = Bounds2::new((rect.min - b) / k, (rect.max - b) / k);
            let zoomed = (k - 1.0).abs() > 1e-9;
            let still = now - c.moved_at;
            if zoomed && still < VP_SETTLE_MS {
                ctx.request_repaint_after(std::time::Duration::from_millis((VP_SETTLE_MS - still).max(1.0) as u64));
            }
            !(0.5..2.0).contains(&k) || !c.area.contains_box(&visible) || (zoomed && still >= VP_SETTLE_MS)
        }
        None => true,
    };
    if rebuild {
        let t0 = crate::now_ms();
        // The viewport with half its larger side of margin all round: panning by up to that much
        // only moves the geometry.
        let area = rect.expand(rect.width().max(rect.height()) * 0.5);
        let opts = cadcraft_render::Options {
            tolerance: 2f64.powi(band) * 0.5,
            min_dash: 2f64.powi(band) * 2.0,
            text: true,
            fill: true,
            lineweights: lwdisplay,
            view_height: st.paper_view().height,
            plot_style_table: None,
            view: Some(area),
            skip_viewport: None,
            transparency,
            cancel: None,
        };
        c.list = cadcraft_render::viewport_content(&st.doc, &st.space, h, area, &opts);
        c.key = Some(key);
        c.view = view;
        c.area = area;
        c.builds = c.builds.wrapping_add(1);
        app.canvas.build_ms = crate::now_ms() - t0;
    }
    let c = &mut app.canvas.vp;
    let (k, b) = vp_motion(&vp, c.view)?;
    c.motion = Some((k, b));
    // Paper p = k·p0 + b, so the list is drawn by the paper transform scaled by k about b.
    let xf = Xf { rect: xf_paper.rect, center: (xf_paper.center - b) / k, scale: xf_paper.scale * k };
    let on_screen = Rect::from_two_pos(xf_paper.to_screen(Vec2::new(rect.min.x, rect.max.y)), xf_paper.to_screen(Vec2::new(rect.max.x, rect.min.y)));
    Some(VpFrame { xf, on_screen, outline: clip.map(|p| p.polygon().to_vec()) })
}

/// Draw the active viewport's contents (see [`VpContent`]) as [`ensure_vp_content`] set out;
/// `p` clips them to the viewport's rectangle, and a clipped viewport's outline is masked with
/// the stencil buffer first.
pub(super) fn draw_vp_content(c: &mut VpContent, gpu: bool, p: &egui::Painter, f: &VpFrame, xf_paper: &Xf, bg: Rgb, lwdisplay: bool) {
    let xf = &f.xf;
    if !gpu {
        draw_list(p, xf, &c.list, bg, lwdisplay);
        return;
    }
    let mut stencil = crate::gpu::Stencil::Off;
    if let Some(outline) = &f.outline {
        draw_vp_mask(c, p, outline, xf_paper);
        stencil = crate::gpu::Stencil::Inside;
    }
    let origin = crate::gpu::choose_origin(&c.list.bounds, xf.center, f64::from(xf.rect.height()) / xf.scale.max(1e-300));
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (c.builds, origin.x.to_bits(), origin.y.to_bits(), bg.0, bg.1, bg.2, lwdisplay).hash(&mut h);
        h.finish()
    };
    if c.mesh_key != Some(key) {
        let mesh = crate::gpu::build_mesh(&c.list, origin, bg, lwdisplay);
        if let Ok(mut slot) = c.mesh_slot.lock() {
            *slot = Some(mesh);
        }
        c.mesh_key = Some(key);
        c.mesh_origin = origin;
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
            layer: crate::gpu::LAYER_VIEWPORT,
            xform: crate::gpu::IDENTITY_XFORM,
            dash: 0.0,
            stencil,
        },
    ));
}

/// Mask a clipped viewport's `outline` (paper units) in the stencil buffer: a triangle fan from
/// its first point, each triangle flipping the stencil bit, leaves the bit set inside the outline
/// (even-odd, concave or not), with no triangulation.
fn draw_vp_mask(c: &mut VpContent, p: &egui::Painter, outline: &[Vec2], xf_paper: &Xf) {
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        outline.iter().for_each(|q| (q.x.to_bits(), q.y.to_bits()).hash(&mut h));
        h.finish()
    };
    if c.mask_key != Some(key) {
        let mut fan = DisplayList::default();
        if let Some((&first, rest)) = outline.split_first() {
            for w in rest.windows(2) {
                if let [a, b] = w {
                    fan.tris.extend_from_slice(&[first, *a, *b]);
                }
            }
        }
        fan.bounds = Bounds2::from_points(outline.iter().copied());
        let len = u32::try_from(fan.tris.len()).unwrap_or(0);
        fan.prims.push(cadcraft_render::DPrim {
            handle: Handle(0),
            color: Rgb(0, 0, 0),
            aci7: false,
            lw: 0.0,
            transparency: 0,
            kind: Kind::Tris,
            start: 0,
            len,
        });
        let origin = if fan.bounds.is_empty() { Vec2::ZERO } else { fan.bounds.center() };
        if let Ok(mut slot) = c.mask_slot.lock() {
            *slot = Some(crate::gpu::build_mesh(&fan, origin, Rgb(0, 0, 0), false));
        }
        c.mask_key = Some(key);
        c.mask_origin = origin;
    }
    let center = xf_paper.rect.center();
    p.add(egui_wgpu::Callback::new_paint_callback(
        xf_paper.rect,
        crate::gpu::CanvasCallback {
            key,
            slot: c.mask_slot.clone(),
            offset: [(c.mask_origin.x - xf_paper.center.x) as f32, (c.mask_origin.y - xf_paper.center.y) as f32],
            scale: xf_paper.scale as f32,
            center: [center.x, center.y],
            layer: crate::gpu::LAYER_MASK,
            xform: crate::gpu::IDENTITY_XFORM,
            dash: 0.0,
            stencil: crate::gpu::Stencil::Mask,
        },
    ));
}

/// Tessellation tolerance for the highlights inside the active viewport: `scale` (pixels per
/// drawing unit) snapped to its 2x zoom band, as in [`ensure_list`], so zooming and panning
/// inside the band doesn't rebuild the geometry.
pub(super) fn viewport_highlight_tolerance(scale: f64) -> f64 {
    2f64.powi((1.0 / scale.max(1e-300)).log2().floor() as i32) * 0.5
}

/// The selected or the hovered objects tessellated in model units for the active MSPACE
/// viewport, whose contents can't be looked up by handle in the layout's lists. The selection and
/// the hover are kept apart, so moving the cursor over objects doesn't tessellate the selection
/// again; each is rebuilt only when the drawing, the objects or the zoom band change.
#[derive(Default)]
pub(super) struct VpHighlight {
    key: Option<u64>,
    pub(super) list: DisplayList,
    /// Bumped on each rebuild (identifies `list` for [`draw_highlight_gpu`]).
    pub(super) list_gen: u64,
}

impl VpHighlight {
    pub(super) fn update(&mut self, st: &cadcraft_engine::DocState, handles: &[Handle], scale: f64) {
        let tolerance = viewport_highlight_tolerance(scale);
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (st.uid, st.revision, std::sync::Arc::as_ptr(&st.doc) as usize, tolerance.to_bits(), handles).hash(&mut h);
            h.finish()
        };
        if self.key != Some(key) {
            let ents = handles.iter().filter_map(|h| st.doc.model.get(*h)).map(|e| e.as_ref());
            self.list = cadcraft_render::build_entities(&st.doc, ents, &cadcraft_render::Options { tolerance, ..Default::default() });
            self.key = Some(key);
            self.list_gen = self.list_gen.wrapping_add(1);
        }
    }
}
