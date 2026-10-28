//! The drawing area: grid, entities, selection highlighting, grips, rubber bands, object-snap
//! markers, crosshair cursor, ViewCube, UCS icon and mouse handling.

use cadcraft_color::Rgb;
use cadcraft_doc::Handle;
use cadcraft_engine::Input;
use cadcraft_engine::snap::{self, SnapHit};
use cadcraft_geom::{Bounds2, Vec2};
use cadcraft_render::{DisplayList, Kind};
use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};

use crate::CadApp;
use crate::theme::Tokens;

/// World ↔ screen transform for the canvas rect.
#[derive(Clone, Copy, Debug)]
pub struct Xf {
    pub rect: Rect,
    pub center: Vec2,
    /// Pixels (points) per drawing unit.
    pub scale: f64,
}

impl Xf {
    pub fn to_screen(&self, p: Vec2) -> Pos2 {
        let c = self.rect.center();
        pos2(c.x + ((p.x - self.center.x) * self.scale) as f32, c.y - ((p.y - self.center.y) * self.scale) as f32)
    }
    pub fn to_world(&self, p: Pos2) -> Vec2 {
        let c = self.rect.center();
        Vec2::new(self.center.x + f64::from(p.x - c.x) / self.scale, self.center.y - f64::from(p.y - c.y) / self.scale)
    }
    pub fn world_bounds(&self) -> Bounds2 {
        Bounds2::new(self.to_world(self.rect.left_bottom()), self.to_world(self.rect.right_top()))
    }
}

/// What a display list was built from: drawing uid, revision, tessellation band, hatch level of
/// detail band, lineweight display, document and active viewport view (see [`ensure_list`]).
type ListKey = (u64, u64, i32, i32, bool, bool, usize, u64);

/// Whether two list keys are for the same drawing content (they may differ only in the view).
fn same_content(a: &ListKey, b: &ListKey) -> bool {
    (a.0, a.1, a.4, a.5, a.6, a.7) == (b.0, b.1, b.4, b.5, b.6, b.7)
}

#[derive(Default)]
pub struct CanvasState {
    pub list: Option<DisplayList>,
    key: ListKey,
    /// The area `list` was built for (hatch patterns exist only inside it); a view leaving it
    /// rebuilds the list.
    window: Option<Bounds2>,
    /// A rebuild for a new view running on a worker thread (see [`rebuild`]); `list` is drawn
    /// meanwhile.
    job: Option<rebuild::Job>,
    /// Bumped whenever `list` is rebuilt (identifies it for [`PrimIndex`]).
    list_gen: u64,
    prim_index: PrimIndex,
    /// Set when the app runs on wgpu: entities are drawn by [`crate::gpu`]; otherwise on the CPU.
    pub gpu: Option<crate::gpu::GpuTarget>,
    /// The wgpu device, for background rebuilds to create their buffers (see [`rebuild`]).
    pub gpu_device: Option<egui_wgpu::wgpu::Device>,
    /// The mesh last handed to the GPU (see [`crate::gpu::CanvasCallback::key`]).
    mesh_key: Option<u64>,
    mesh_slot: crate::gpu::MeshSlot,
    mesh_origin: Vec2,
    /// The dragged objects of a MOVE/COPY/ROTATE/SCALE preview, uploaded once per drag (see
    /// [`draw_drag_gpu`]).
    drag_key: Option<u64>,
    drag_slot: crate::gpu::MeshSlot,
    drag_origin: Vec2,
    /// The selection highlight on the GPU (see [`draw_highlight_gpu`]).
    hl_key: Option<u64>,
    hl_slot: crate::gpu::MeshSlot,
    hl_origin: Vec2,
    /// Selected infinite lines (clipped and drawn on the CPU each frame).
    hl_infinite: Vec<usize>,
    /// The selection and hover highlights inside the active MSPACE viewport (see [`VpHighlight`]).
    vp_sel: VpHighlight,
    vp_hover: VpHighlight,
    /// Model space seen through the active MSPACE viewport (see [`VpContent`]).
    vp: VpContent,
    /// Indices of infinite-line primitives in `list` (drawn on the CPU in GPU mode).
    infinite: Vec<usize>,
    /// Milliseconds spent converting the display list to vertex data (last rebuild).
    pub mesh_ms: f64,
    pub rect: Option<Rect>,
    pub xf: Option<Xf>,
    pub hover: Option<Handle>,
    pub snap: Option<SnapHit>,
    /// Effective cursor point after snaps/ortho/polar.
    pub cursor: Option<Vec2>,
    /// The polar or object snap tracking path(s) the cursor is on.
    pub track: Option<snap::tracking::Track>,
    pan_last: Option<Pos2>,
    /// True when the latest scroll came from a trackpad (or other precise-scrolling device):
    /// scrolling then pans and pinch zooms. A notched mouse wheel clears it so the wheel zooms.
    scroll_pans: bool,
    /// Hot grips (object, grip index): the ones Shift+clicked while idle, then the ones the
    /// running GRIP edit works on (the engine's grip modes take them from there).
    pub hot_grips: Vec<(Handle, usize)>,
    /// Grip dimensional input: the value fields of the grip being stretched.
    pub grip_dyn: crate::grip_dyn::GripDyn,
    /// A GRIP edit was running last frame (its grips go cold when it ends).
    grip_editing: bool,
    /// The grip whose menu (Stretch, Add Vertex, Lengthen…) a right-click opened.
    grip_menu: Option<(Handle, usize)>,
    /// Dynamic block grips of the selection (cached), the one being dragged, and the list grip
    /// whose choices are open (and whether it opened this frame).
    pub(crate) dyn_grips: crate::dyngrips::Cache,
    pub(crate) dyn_hot: Option<(Handle, i32, cadcraft_engine::doc::dynblock::GripPart)>,
    pub(crate) dyn_menu: Option<(Handle, cadcraft_engine::doc::dynblock::DynGrip)>,
    pub(crate) dyn_menu_fresh: bool,
    /// Zoom to extents once the canvas size is known (after opening a drawing).
    pub zoom_pending: bool,
    pub build_ms: f64,
    pub draw_ms: f64,
    hover_at: Option<Pos2>,
    /// Constraint glyphs for the parametric overlay (cached per drawing revision).
    pub param: crate::parametric::Cache,
    /// A selection window opened by a press-and-drag, to close where the button is let go.
    drag_window: bool,
    /// The right-click that opened the shortcut menu held Shift or Ctrl: it is the object snap
    /// menu ([`crate::context_menu`]).
    pub snap_menu: bool,
    /// Tab was pressed over a snap marker: show the next candidate ([`SnapCycle`]).
    pub snap_tab: bool,
    pub snap_cycle: Option<SnapCycle>,
    /// The selection cycling list a click over overlapping objects opened (SELECTIONCYCLING 2).
    cycle: Option<CycleList>,
    /// Selection cycling's badge shows beside the cursor, and where it was last worked out.
    cycle_badge: bool,
    cycle_badge_at: Option<Pos2>,
    /// Shift+Space stepping the highlight through the objects under the cursor (selection
    /// cycling's temporary override): where it started, the objects, which one is highlighted.
    hover_cycle: Option<(Pos2, Vec<Handle>, usize)>,
    /// What the cycling list and the Shift+Space highlight were made for (see [`CycleKey`]).
    cycle_key: Option<CycleKey>,
}

/// The command, prompt, drawing (and its revision) and view that the objects under the cursor
/// were listed for: when any of them changes, the list and the Shift+Space highlight are stale.
type CycleKey = (Option<String>, Option<String>, u64, u64, Vec2, f64);

impl CanvasState {
    /// The active MSPACE viewport's contents when they are drawn apart from the layout's list
    /// (see [`VpContent`]): where they are on paper at the viewport's current view, and how many
    /// times they have been built.
    pub fn viewport_content(&self) -> Option<(Bounds2, u64)> {
        self.vp.key.and(self.vp.paper_bounds()).map(|b| (b, self.vp.builds))
    }

    /// The rows of the open selection cycling list (screen rectangle, object), top to bottom.
    pub fn cycle_rows(&self) -> Vec<(Rect, Handle)> {
        self.cycle.as_ref().map(|c| c.rows.iter().copied().zip(c.items.iter().map(|(h, _, _)| *h)).collect()).unwrap_or_default()
    }

    /// Whether a rebuild for the current view is still running in the background (the canvas
    /// shows the previous list until it lands).
    pub fn building(&self) -> bool {
        self.job.is_some()
    }

    /// Put a finished background build in place, with its GPU vertex data when it has some.
    fn install(&mut self, key: ListKey, window: Option<Bounds2>, built: rebuild::Built) {
        self.list_gen = self.list_gen.wrapping_add(1);
        if let Some(m) = built.mesh {
            if let Ok(mut slot) = self.mesh_slot.lock() {
                *slot = Some(m.data);
            }
            self.mesh_key = Some(list_mesh_key(&key, self.list_gen, m.origin, m.bg, m.lwdisplay));
            self.mesh_origin = m.origin;
            self.mesh_ms = m.ms;
            self.infinite = m.infinite;
        }
        self.list = Some(built.list);
        self.key = key;
        self.window = window;
        self.build_ms = built.build_ms;
    }
}

/// Tab cycling through the candidate snaps under the cursor: where it started and which
/// candidate shows. Moving the cursor out of the aperture ends it.
#[derive(Clone, Copy, Debug)]
pub struct SnapCycle {
    anchor: Vec2,
    index: usize,
}

/// The objects under a click that selection cycling lets the user choose between: where the click
/// was, each object with its row's label (type and layer), Shift held, and the rows as drawn.
#[derive(Clone, Debug)]
struct CycleList {
    at: Pos2,
    items: Vec<(Handle, String, String)>,
    shift: bool,
    rows: Vec<Rect>,
}

impl CycleList {
    fn row_at(&self, p: Pos2) -> Option<Handle> {
        self.rows.iter().zip(&self.items).find(|(r, _)| r.contains(p)).map(|(_, (h, _, _))| *h)
    }
}

/// Primitives of `list` sorted by entity handle, built on the first highlight after each list
/// rebuild. Highlighting the hovered entity used to scan every primitive on every frame: ~2.5 ms
/// per frame on a 270k-entity drawing, whether or not anything changed.
#[derive(Default)]
struct PrimIndex {
    list_gen: Option<u64>,
    by_handle: Vec<(Handle, usize)>,
}

impl PrimIndex {
    /// Indices of the primitives drawn for `handles` (in list order).
    fn prims(&mut self, list_gen: u64, list: &DisplayList, handles: &[Handle]) -> Vec<usize> {
        if handles.is_empty() {
            return Vec::new();
        }
        if self.list_gen != Some(list_gen) {
            self.by_handle = list.prims.iter().enumerate().map(|(i, p)| (p.handle, i)).collect();
            self.by_handle.sort_unstable();
            self.list_gen = Some(list_gen);
        }
        let mut out = Vec::new();
        for h in handles {
            let from = self.by_handle.partition_point(|(k, _)| k < h);
            out.extend(self.by_handle.get(from..).unwrap_or(&[]).iter().take_while(|(k, _)| k == h).map(|(_, i)| *i));
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// Indices of the primitives in `list` drawn for `handles` (a scan: the reference [`PrimIndex`]
/// is tested against).
#[cfg(test)]
fn prims_of(list: &DisplayList, handles: &[Handle]) -> Vec<usize> {
    if handles.is_empty() {
        return Vec::new();
    }
    let set: std::collections::HashSet<Handle> = handles.iter().copied().collect();
    list.prims.iter().enumerate().filter(|(_, pr)| set.contains(&pr.handle)).map(|(i, _)| i).collect()
}

/// Build options for previews at `scale` pixels per drawing unit (opaque).
fn preview_options(scale: f64) -> cadcraft_render::Options {
    cadcraft_render::Options { tolerance: 0.5 / scale, min_dash: 2.0 / scale, ..Default::default() }
}

/// The display list of preview entities the session builds with the current properties
/// ([`cadcraft_engine::Session::preview`], the extras of [`cadcraft_engine::Session::drag`]): the
/// command's rubber band and rebuilt copies of existing objects. They carry CLAYER's properties,
/// not the objects', so they are drawn opaque (the GPU drag of the objects themselves keeps their
/// transparency, see [`draw_drag_gpu`]).
fn rebuilt_preview<'a>(doc: &cadcraft_doc::Drawing, ents: impl IntoIterator<Item = &'a cadcraft_doc::Entity>, scale: f64) -> DisplayList {
    cadcraft_render::build_entities(doc, ents, &preview_options(scale))
}

fn color32(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

/// A primitive's colour on `bg`, with its transparency.
fn prim_color32(prim: &cadcraft_render::DPrim, bg: Rgb) -> Color32 {
    let c = prim.display_rgb(bg);
    Color32::from_rgba_unmultiplied(c.0, c.1, c.2, prim.alpha())
}

/// Rebuild the display list when needed. `xf`: the current view; `views`: every area on screen
/// that shows the list (the current view, other model tiles) with its drawing units per pixel.
///
/// A rebuild only the view asks for (a new zoom band, a pan out of the built area) runs in the
/// background while the current list is drawn (see [`rebuild`]); a changed drawing, or no list
/// yet, is built at once.
fn ensure_list(app: &mut CadApp, xf: &Xf, views: &[(Bounds2, f64)], ctx: &egui::Context) {
    let px = 1.0 / xf.scale;
    let Ok(st) = app.session.state() else { return };
    // Rebuild when the drawing changes, the zoom moves by more than 2x (tessellation band) or a
    // view leaves the area the list was built for.
    let band = |px: f64| px.max(1e-300).log2().floor() as i32;
    // Hatch level of detail follows the finest view showing the list.
    let lod_band = views.iter().map(|v| band(v.1)).fold(band(px), i32::min);
    // The active MSPACE viewport's contents are drawn apart (see `VpContent`), so panning and
    // zooming inside it leave this list alone. Without a stencil buffer a clipped viewport's
    // contents stay in the list, which then follows its view: realtime navigation changes it
    // without a new revision.
    let split = split_viewport(st, stencil_clip(app)).map(|(h, ..)| h);
    let vp_view = {
        use std::hash::{Hash, Hasher};
        let mut s = std::collections::hash_map::DefaultHasher::new();
        split.hash(&mut s);
        if split.is_none()
            && let Some((h, vp)) = st.active_viewport()
        {
            (h, vp.view_center.x.to_bits(), vp.view_center.y.to_bits(), vp.view_height.to_bits()).hash(&mut s);
        }
        s.finish()
    };
    let (lwdisplay, transparency_display) = (app.session.settings.lwdisplay, app.session.settings.transparency_display);
    let key: ListKey = (st.uid, st.revision, band(px), lod_band, lwdisplay, transparency_display, std::sync::Arc::as_ptr(&st.doc) as usize, vp_view);
    // A background build that finished: use it unless the drawing has changed since it started
    // (or the list was dropped to force a rebuild).
    if let Some(job) = &app.canvas.job {
        match job.poll() {
            rebuild::Poll::Pending => {}
            rebuild::Poll::Done(built) => {
                let (job_key, window) = (job.key, job.window);
                app.canvas.job = None;
                if app.canvas.list.is_some() && same_content(&job_key, &key) {
                    app.canvas.install(job_key, window, *built);
                }
            }
            rebuild::Poll::Lost => {
                log::warn!("canvas: a background rebuild failed; rebuilding on the UI thread");
                app.canvas.job = None;
                app.canvas.list = None;
            }
        }
    }
    let visible = views.iter().fold(Bounds2::EMPTY, |acc, v| acc.union(&v.0));
    let inside = app.canvas.window.is_none_or(|w| visible.is_empty() || w.contains_box(&visible));
    if app.canvas.list.is_some() && app.canvas.key == key && inside {
        return;
    }
    // Only the view moved: keep drawing the current list and build the new one in the background.
    let view_only = app.canvas.list.is_some() && same_content(&app.canvas.key, &key);
    if view_only && app.canvas.job.as_ref().is_some_and(|j| j.serves(&key, &visible)) {
        return;
    }
    // A build for another view, or for a drawing that has since changed, is no longer wanted:
    // dropping it cancels it.
    app.canvas.job = None;
    let t0 = crate::now_ms();
    let opts = cadcraft_render::Options {
        tolerance: 2f64.powi(band(px)) * 0.5,
        // Two pixels at the band's finest zoom: the list serves every zoom in the band, so its
        // level of detail must hold at the finest one.
        min_dash: 2f64.powi(lod_band) * 2.0,
        text: true,
        fill: true,
        lineweights: lwdisplay,
        view_height: st.paper_view().height,
        plot_style_table: None,
        view: build_window(views),
        transparency: app.session.settings.transparency_display,
        skip_viewport: split,
        cancel: None,
    };
    if view_only {
        let t = Tokens::get();
        let mesh = app.canvas.gpu.is_some().then(|| rebuild::MeshRequest {
            view_center: xf.center,
            view_height: f64::from(xf.rect.height()) / xf.scale.max(1e-300),
            model_bg: Rgb(t.canvas.r(), t.canvas.g(), t.canvas.b()),
            paper_bg: Rgb(t.paper.r(), t.paper.g(), t.paper.b()),
            lwdisplay,
            device: app.canvas.gpu_device.clone(),
        });
        let window = opts.view;
        let req = rebuild::Request { doc: st.doc.clone(), space: st.space.clone(), opts: opts.clone(), mesh };
        if let Some(job) = rebuild::spawn(req, key, window, ctx) {
            app.canvas.job = Some(job);
            return;
        }
    }
    let space = st.space.clone();
    app.canvas.list = Some(cadcraft_render::build(&st.doc, &space, &opts));
    app.canvas.key = key;
    app.canvas.window = opts.view;
    app.canvas.list_gen = app.canvas.list_gen.wrapping_add(1);
    app.canvas.build_ms = crate::now_ms() - t0;
}

/// The active MSPACE viewport when its contents are drawn apart from the layout's list (see
/// [`VpContent`]), with its clip outline when it is clipped. A clipped viewport is clipped with
/// the GPU's stencil buffer (`stencil`); without one its contents stay in the list.
fn split_viewport(
    st: &cadcraft_engine::DocState,
    stencil: bool,
) -> Option<(Handle, cadcraft_doc::Viewport, Option<cadcraft_render::clip::PolyClip>)> {
    let (h, vp) = st.active_viewport()?;
    let clip = cadcraft_render::viewport_clip(&st.doc, h, &vp);
    (clip.is_none() || stencil).then_some((h, vp, clip))
}

/// Whether the canvas can clip a viewport to any outline (a GPU with a stencil buffer).
fn stencil_clip(app: &CadApp) -> bool {
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
struct VpContent {
    list: DisplayList,
    /// What the list was built from: drawing, revision, document, viewport, tessellation band
    /// and lineweight display.
    key: Option<(u64, u64, usize, Handle, i32, bool, bool)>,
    /// The viewport's view (centre, height) the list was built for, and the paper area it covers.
    view: (Vec2, f64),
    area: Bounds2,
    /// The view on the previous frame and when it last changed (ms).
    last_view: (Vec2, f64),
    moved_at: f64,
    /// Scale and offset drawing the list at the viewport's current view (see [`vp_motion`]).
    motion: Option<(f64, Vec2)>,
    /// Bumped on each rebuild.
    builds: u64,
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
    fn paper_bounds(&self) -> Option<Bounds2> {
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
struct VpFrame {
    /// Draws the list at the viewport's current view.
    xf: Xf,
    /// The viewport's rectangle (a clipped one's outline bounds) on screen.
    on_screen: Rect,
    /// A clipped viewport's outline on paper, masked with the stencil buffer.
    outline: Option<Vec<Vec2>>,
}

/// Build or reuse the active viewport's [`VpContent`] for this frame.
fn ensure_vp_content(app: &mut CadApp, xf_paper: &Xf, ctx: &egui::Context) -> Option<VpFrame> {
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
fn draw_vp_content(c: &mut VpContent, gpu: bool, p: &egui::Painter, f: &VpFrame, xf_paper: &Xf, bg: Rgb, lwdisplay: bool) {
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
fn viewport_highlight_tolerance(scale: f64) -> f64 {
    2f64.powi((1.0 / scale.max(1e-300)).log2().floor() as i32) * 0.5
}

/// The selected or the hovered objects tessellated in model units for the active MSPACE
/// viewport, whose contents can't be looked up by handle in the layout's lists. The selection and
/// the hover are kept apart, so moving the cursor over objects doesn't tessellate the selection
/// again; each is rebuilt only when the drawing, the objects or the zoom band change.
#[derive(Default)]
struct VpHighlight {
    key: Option<u64>,
    list: DisplayList,
    /// Bumped on each rebuild (identifies `list` for [`draw_highlight_gpu`]).
    list_gen: u64,
}

impl VpHighlight {
    fn update(&mut self, st: &cadcraft_engine::DocState, handles: &[Handle], scale: f64) {
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

/// The area a display list is built for: each view with half its larger side of margin all
/// round (panning by up to that much doesn't rebuild), joined. `None` (everything) when a view
/// isn't usable.
fn build_window(views: &[(Bounds2, f64)]) -> Option<Bounds2> {
    let mut w = Bounds2::EMPTY;
    for (v, _) in views {
        let side = v.width().max(v.height());
        if v.is_empty() || !side.is_finite() {
            return None;
        }
        w = w.union(&v.expand(side * 0.5));
    }
    (!w.is_empty()).then_some(w)
}

/// Grid spacing adapted to the zoom: multiply the unit until lines are at least `min_px` apart.
fn adaptive(unit: f64, scale: f64, min_px: f64) -> f64 {
    let mut u = unit.max(1e-12);
    let mut guard = 0;
    while u * scale < min_px && guard < 40 {
        u *= 5.0;
        guard += 1;
    }
    u
}

/// Index ranges of the grid lines inside `wb` for line spacing `minor`, or `None` when there would be
/// 2000 lines or more along an axis. The span is computed with checked arithmetic: a huge view
/// saturates the float-to-integer casts at the i64 limits, and subtracting those would overflow.
fn grid_index_ranges(wb: &Bounds2, minor: f64) -> Option<((i64, i64), (i64, i64))> {
    let range = |lo: f64, hi: f64| {
        let a = (lo / minor).floor() as i64;
        let b = (hi / minor).ceil() as i64;
        b.checked_sub(a).filter(|n| *n < 2000).map(|_| (a, b))
    };
    Some((range(wb.min.x, wb.max.x)?, range(wb.min.y, wb.max.y)?))
}

fn draw_grid(app: &CadApp, p: &egui::Painter, xf: &Xf) {
    let t = Tokens::get();
    let s = app.session.drafting_settings();
    let wb = xf.world_bounds();
    if s.gridmode && s.isodraft {
        draw_iso_grid(app, p, xf);
    } else if s.gridmode {
        let major_n = f64::from(s.gridmajor.max(1));
        let unit = if s.gridunit.x == 0.0 { s.snapunit.x } else { s.gridunit.x };
        let minor = adaptive(unit, xf.scale, 8.0);
        let major = minor * major_n;
        let mut lines = Vec::new();
        if let Some(((x0, x1), (y0, y1))) = grid_index_ranges(&wb, minor) {
            for i in x0..=x1 {
                let x = i as f64 * minor;
                let is_major = (x / major).round() * major - x == 0.0 || ((x / major) - (x / major).round()).abs() < 1e-6;
                let sx = xf.to_screen(Vec2::new(x, 0.0)).x;
                lines.push(Shape::line_segment(
                    [pos2(sx, xf.rect.top()), pos2(sx, xf.rect.bottom())],
                    Stroke::new(1.0, if is_major { t.grid_major } else { t.grid_minor }),
                ));
            }
            for j in y0..=y1 {
                let y = j as f64 * minor;
                let is_major = ((y / major) - (y / major).round()).abs() < 1e-6;
                let sy = xf.to_screen(Vec2::new(0.0, y)).y;
                lines.push(Shape::line_segment(
                    [pos2(xf.rect.left(), sy), pos2(xf.rect.right(), sy)],
                    Stroke::new(1.0, if is_major { t.grid_major } else { t.grid_minor }),
                ));
            }
        }
        p.extend(lines);
    }
    // Axes through the origin.
    let o = xf.to_screen(Vec2::ZERO);
    if xf.rect.y_range().contains(o.y) {
        p.line_segment([pos2(xf.rect.left(), o.y), pos2(xf.rect.right(), o.y)], Stroke::new(1.0, t.axis_x));
    }
    if xf.rect.x_range().contains(o.x) {
        p.line_segment([pos2(o.x, xf.rect.top()), pos2(o.x, xf.rect.bottom())], Stroke::new(1.0, t.axis_y));
    }
}

/// The isometric grid (SNAPSTYL 1): lines at 30°, 90° and 150° through the snap lattice, GRIDUNIT
/// Y apart along the axes, coarsened while they would crowd closer than 8 pixels.
fn draw_iso_grid(app: &CadApp, p: &egui::Painter, xf: &Xf) {
    use cadcraft_engine::snap::iso;
    let t = Tokens::get();
    let s = app.session.drafting_settings();
    let mut d = s.gridunit.y.max(1e-12);
    for _ in 0..40 {
        if d * 0.866 * xf.scale >= 8.0 {
            break;
        }
        d *= 5.0;
    }
    let (origin, angle) = app.session.doc().map(cadcraft_engine::snap::grid_frame).unwrap_or((Vec2::ZERO, 0.0));
    let lines = iso::grid_lines(xf.world_bounds(), d, origin, angle, s.gridmajor);
    p.extend(
        lines.into_iter().map(|l| {
            Shape::line_segment([xf.to_screen(l.a), xf.to_screen(l.b)], Stroke::new(1.0, if l.major { t.grid_major } else { t.grid_minor }))
        }),
    );
}

fn clip_infinite(xf: &Xf, base: Vec2, dir: Vec2, ray: bool) -> Option<[Pos2; 2]> {
    let wb = xf.world_bounds();
    let far = (wb.width() + wb.height()) * 2.0 + base.dist(wb.center());
    let a = if ray { base } else { base - dir * far };
    let b = base + dir * far;
    Some([xf.to_screen(a), xf.to_screen(b)])
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
fn draw_highlight_gpu(
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
fn draw_drag_gpu(
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
fn list_mesh_key(key: &ListKey, list_gen: u64, origin: Vec2, bg: Rgb, lwdisplay: bool) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (key, list_gen, origin.x.to_bits(), origin.y.to_bits(), bg.0, bg.1, bg.2, lwdisplay).hash(&mut h);
    h.finish()
}

/// Draw the display list through the GPU paint callback (uploading it when it changed), plus
/// the infinite lines, which are clipped to the view on the CPU every frame.
fn draw_list_gpu(c: &mut CanvasState, p: &egui::Painter, xf: &Xf, bg: Rgb, lwdisplay: bool) {
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

#[cfg(test)]
mod highlight_stability_tests {
    use super::*;

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

/// The fill of a selection window or crossing area (SELECTIONAREA, WINDOWAREACOLOR,
/// CROSSINGAREACOLOR, SELECTIONAREAOPACITY); `None` when areas aren't filled.
fn selection_area_fill(s: &cadcraft_engine::Settings, crossing: bool) -> Option<Color32> {
    let aci = if crossing { s.crossingareacolor } else { s.windowareacolor };
    let c = cadcraft_color::aci_rgb(aci);
    s.selectionarea.then(|| Color32::from_rgb(c.0, c.1, c.2).gamma_multiply(f32::from(s.selectionareaopacity.min(100)) / 100.0))
}

fn draw_snap_marker(p: &egui::Painter, at: Pos2, hit: &SnapHit) {
    let t = Tokens::get();
    let s = crate::theme::snap_marker_half();
    let st = Stroke::new(2.0, t.snap);
    use snap::mode;
    match hit.mode {
        mode::END => {
            p.rect_stroke(Rect::from_center_size(at, vec2(2.0 * s, 2.0 * s)), 0.0, st, egui::StrokeKind::Middle);
        }
        mode::MID => {
            p.add(Shape::closed_line(vec![at + vec2(0.0, -s), at + vec2(s, s * 0.8), at + vec2(-s, s * 0.8)], st));
        }
        mode::CEN | mode::GCEN => {
            p.circle_stroke(at, s, st);
        }
        mode::QUA => {
            p.add(Shape::closed_line(vec![at + vec2(0.0, -s), at + vec2(s, 0.0), at + vec2(0.0, s), at + vec2(-s, 0.0)], st));
        }
        mode::INT | mode::APP => {
            p.line_segment([at + vec2(-s, -s), at + vec2(s, s)], st);
            p.line_segment([at + vec2(s, -s), at + vec2(-s, s)], st);
            if hit.mode == mode::APP {
                p.rect_stroke(Rect::from_center_size(at, vec2(2.0 * s, 2.0 * s)), 0.0, st, egui::StrokeKind::Middle);
            }
        }
        mode::PER => {
            p.line(vec![at + vec2(-s, -s), at + vec2(-s, s), at + vec2(s, s)], st);
            p.line(vec![at + vec2(-s, 0.0), at + vec2(0.0, 0.0), at + vec2(0.0, s)], st);
        }
        mode::TAN => {
            p.circle_stroke(at, s * 0.8, st);
            p.line_segment([at + vec2(-s, -s), at + vec2(s, -s)], st);
        }
        mode::NOD => {
            p.circle_stroke(at, s, st);
            p.line_segment([at + vec2(-s, -s), at + vec2(s, s)], st);
            p.line_segment([at + vec2(s, -s), at + vec2(-s, s)], st);
        }
        mode::INS => {
            p.line(
                vec![
                    at + vec2(-s, -s),
                    at + vec2(0.0, -s),
                    at + vec2(0.0, 0.0),
                    at + vec2(s, 0.0),
                    at + vec2(s, s),
                    at + vec2(-s, s),
                    at + vec2(-s, -s),
                ],
                st,
            );
        }
        _ => {
            p.line(vec![at + vec2(-s, -s), at + vec2(s, -s), at + vec2(-s, s), at + vec2(s, s), at + vec2(-s, -s)], st);
        }
    }
    p.text(at + vec2(12.0, 12.0), egui::Align2::LEFT_TOP, crate::i18n::t(hit.name), crate::theme::small(), Color32::BLACK);
    let galley = p.layout_no_wrap(crate::i18n::t(hit.name).to_string(), crate::theme::small(), Color32::BLACK);
    let r = Rect::from_min_size(at + vec2(10.0, 10.0), galley.size() + vec2(6.0, 2.0));
    p.rect_filled(r, 2.0, Color32::from_rgb(0xff, 0xff, 0xe1));
    p.galley(r.min + vec2(3.0, 1.0), galley, Color32::BLACK);
}

/// Acquired tracking points (small `+`, `//` for a Parallel line), the dotted paths the cursor
/// is on and the tooltip naming them ("Endpoint: 5.0000 < 90°").
fn draw_tracking(p: &egui::Painter, xf: &Xf, acquired: &[snap::tracking::Acquired], track: Option<&snap::tracking::Track>, far: f64) {
    use snap::tracking::{Acquired, PathShape};
    let t = Tokens::get();
    let st = Stroke::new(1.0, t.snap);
    for a in acquired {
        let c = xf.to_screen(a.at());
        if let Acquired::Parallel { .. } = a {
            p.line_segment([c + vec2(-5.0, 3.0), c + vec2(-1.0, -5.0)], st);
            p.line_segment([c + vec2(1.0, 3.0), c + vec2(5.0, -5.0)], st);
        } else {
            p.line_segment([c + vec2(-4.0, 0.0), c + vec2(4.0, 0.0)], st);
            p.line_segment([c + vec2(0.0, -4.0), c + vec2(0.0, 4.0)], st);
        }
    }
    let Some(tr) = track else { return };
    let mut tips = Vec::new();
    for path in &tr.paths {
        // Around the cursor only (a path's origin may be far off screen).
        let pts: Vec<Pos2> = match path.shape {
            PathShape::Ray { origin, dir } => {
                let back = (tr.point - origin).dot(dir).clamp(0.0, far);
                vec![xf.to_screen(tr.point - dir * back), xf.to_screen(tr.point + dir * far)]
            }
            PathShape::Line { dir, .. } => vec![xf.to_screen(tr.point - dir * far), xf.to_screen(tr.point + dir * far)],
            PathShape::Circle { center, radius, .. } => {
                let (a0, span) = (center.angle_to(tr.point), (far / radius).min(std::f64::consts::PI));
                (0..=96).map(|i| xf.to_screen(Vec2::polar(center, radius, a0 - span + span * f64::from(i) / 48.0))).collect()
            }
            PathShape::Ellipse { center, major, ratio, .. } => {
                let e = cadcraft_geom::Ellipse::full(center, major, ratio);
                let (t0, span) = (e.param_of(tr.point), (far / (major.len() * ratio.min(1.0))).min(std::f64::consts::PI));
                (0..=96).map(|i| xf.to_screen(e.at_param(t0 - span + span * f64::from(i) / 48.0))).collect()
            }
        };
        p.extend(Shape::dotted_line(&pts, t.snap, 6.0, 1.0));
        let (distance, angle) = (
            cadcraft_engine::units::format_distance(path.from.dist(tr.point), 2, 4),
            (path.from.angle_to(tr.point).to_degrees().round() as i64).rem_euclid(360),
        );
        tips.push(if path.name == "Polar" {
            crate::tf!("Polar: {distance} < {angle}°", distance = distance, angle = angle)
        } else {
            crate::tf!("{name}: {distance} < {angle}°", name = crate::i18n::t(path.name), distance = distance, angle = angle)
        });
    }
    if let Some(o) = tr.object {
        tips.push(crate::i18n::t(o).to_string());
    }
    let at = xf.to_screen(tr.point);
    if tr.paths.len() > 1 || tr.object.is_some() {
        p.line_segment([at + vec2(-4.0, -4.0), at + vec2(4.0, 4.0)], st);
        p.line_segment([at + vec2(4.0, -4.0), at + vec2(-4.0, 4.0)], st);
    }
    tooltip(p, at + vec2(16.0, 18.0), &tips.join(", "));
}

/// The rubber-band and snap-adjusted cursor point for the current prompt (the engine's
/// `Session::snap_cursor`: object snaps, acquiring tracking points, grid, tracking paths, ortho).
fn effective_point(app: &mut CadApp, raw: Vec2, xf: &Xf, ctx: &egui::Context) -> Vec2 {
    let prompt = app.session.current_prompt();
    let wants_point = prompt.as_ref().is_some_and(cadcraft_engine::pointmod::accepts);
    let base = prompt.as_ref().and_then(|p| p.base);
    let deferred = prompt.as_ref().is_some_and(|p| p.deferred);
    let corner = prompt.as_ref().is_some_and(|p| p.corner);
    app.canvas.snap = None;
    app.canvas.track = None;
    if !wants_point || app.session.pending_window.is_some() {
        if !wants_point {
            app.session.tracking.clear();
        }
        return raw;
    }
    let (now, shift) = ctx.input(|i| (i.time, i.modifiers.shift));
    let q =
        snap::tracking::CursorQuery { base, deferred, corner, aperture: app.session.settings.aperture / xf.scale, tol: 6.0 / xf.scale, now, shift };
    let r = app.session.snap_cursor(raw, &q);
    if r.wait {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(snap::tracking::DWELL / 4.0));
    }
    app.canvas.snap = r.snap;
    app.canvas.track = r.track;
    r.point
}

/// The vertical wheel scroll this frame that zooms, in points, read from the raw wheel events so
/// each notch zooms by its whole step at once, as in standard CAD (#660). egui's smoothed delta
/// spreads a notch over several frames, so quick notches lag behind the wheel. Units convert to
/// points as egui does (a line is `line_scroll_speed` points, a page the view's height), and
/// events egui turns into a pinch zoom (⌘/Ctrl) or a sideways scroll (⇧) are left out, as it does.
fn wheel_zoom_points(events: &[egui::Event], opts: &egui::InputOptions, page: f32) -> f32 {
    let points: f32 = events
        .iter()
        .filter_map(|e| match e {
            egui::Event::MouseWheel { unit, delta, modifiers, .. }
                if !modifiers.matches_any(opts.zoom_modifier) && !modifiers.matches_any(opts.horizontal_scroll_modifier) =>
            {
                let per = match unit {
                    egui::MouseWheelUnit::Point => 1.0,
                    egui::MouseWheelUnit::Line => opts.line_scroll_speed,
                    egui::MouseWheelUnit::Page => page,
                };
                Some(delta.y * per)
            }
            _ => None,
        })
        .sum();
    if points.is_finite() { points } else { 0.0 }
}

/// Draw the drawing area and handle its input (zoom and pan, picking, the crosshair) for this
/// frame.
///
/// It also owns the cursor-visibility contract: the OS cursor is hidden only while the pointer is
/// over the drawing area *and* the canvas is the topmost layer under it, so the drawn crosshair
/// replaces the pointer over the drawing alone. A menu bar, an open menu (including its frame, not
/// just its items), a tooltip or a floating palette above the canvas keeps the normal OS cursor
/// visible (#1701), and so do the menus painted on the canvas itself (see [`over_canvas_menu`]).
pub fn show(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let full = ui.available_rect_before_wrap();
    // Tiled model viewports (VPORTS): the current tile is the drawing area below; `draw_tiles`
    // draws the others.
    let tiles = tile_rects(app, full);
    let rect = tiles.iter().find(|(_, tile)| tile.active).map_or(full, |(r, _)| *r);
    let resp = ui.interact(full, ui.scope_id().with("canvas"), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    app.canvas.rect = Some(rect);
    app.session.viewport_px = (f64::from(rect.width()).max(1.0), f64::from(rect.height()).max(1.0));
    if app.canvas.zoom_pending {
        app.canvas.zoom_pending = false;
        let _ = app.session.zoom_extents();
    }
    let Ok(st) = app.session.state() else { return };
    let view = st.view();
    let scale = f64::from(rect.height()) / view.height.max(1e-12);
    let xf = Xf { rect, center: view.center, scale };
    app.canvas.xf = Some(xf);

    // ---------- input ----------
    let (hover_pos, scroll, mods, middle_down, pressed_primary, pressed_secondary, dbl_middle, dbl_primary) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.smooth_scroll_delta,
            i.modifiers,
            i.pointer.middle_down(),
            i.pointer.primary_clicked(),
            i.pointer.secondary_clicked(),
            i.pointer.button_double_clicked(egui::PointerButton::Middle),
            i.pointer.button_double_clicked(egui::PointerButton::Primary),
        )
    });
    // The drawing area alone: a menu popup's frame (between its items and its border), a tooltip
    // or a floating palette lives on a layer above the canvas, where `hovered()` alone still
    // counts the canvas. Over those the OS cursor stays visible and the crosshair stays put, as
    // over the menu bar (#1701).
    let inside = hover_pos.is_some_and(|p| rect.contains(p) && ui.ctx().layer_id_at(p) == Some(ui.layer_id()) && resp.hovered());
    // The menus the canvas paints itself (selection cycling's rows, Dynamic Input's options) are
    // menus too, but they share the canvas's layer: `layer_id_at` reports the drawing under them,
    // so only their rectangles tell them apart from it ([`over_canvas_menu`]). They are excluded
    // from the cursor contract below, though not from the canvas's input: a click on a row is
    // taken by the row.
    let on_menu = hover_pos.is_some_and(|p| over_canvas_menu(app, p));
    // Whether Ctrl is held right now. Written every frame (not just while a command prompts), so
    // previews that read `Session::ctrl_down` always see the current state. The engine
    // deliberately does not carry modifiers through `Input`, which is UI-agnostic (see PR
    // #407's Limits note: modifiers used to be dropped on the floor at this point).
    app.session.ctrl_down = mods.ctrl;
    // Which device is scrolling? On macOS, trackpads (and Magic Mouse) report precise point
    // deltas; notched mouse wheels report lines. The answer is sticky because egui spreads a wheel
    // notch over several frames. Elsewhere (browsers, Wayland smooth-scroll mice) ordinary wheels
    // also report points, so the wheel keeps zooming there.
    ui.input(|i| {
        for e in &i.raw.events {
            if let egui::Event::MouseWheel { unit, .. } = e {
                app.canvas.scroll_pans = cfg!(target_os = "macos") && *unit == egui::MouseWheelUnit::Point;
            }
        }
    });
    if inside && app.canvas.scroll_pans && scroll != egui::Vec2::ZERO {
        // Two-finger swipe pans: the drawing follows the fingers, like a middle-drag.
        // Pinch (and Cmd+scroll) still zoom via `zoom_delta` below.
        if let Ok(st) = app.session.state_mut() {
            let v = st.view();
            st.set_view_quiet(cadcraft_engine::View {
                center: v.center - Vec2::new(f64::from(scroll.x) / scale, -f64::from(scroll.y) / scale),
                height: v.height,
            });
        }
    }
    let opts = ui.ctx().options(|o| o.input_options);
    let wheel = ui.input(|i| wheel_zoom_points(&i.raw.events, &opts, rect.height()));
    if inside
        && !app.canvas.scroll_pans
        && wheel != 0.0
        && let Some(hp) = hover_pos
    {
        // Zoom with the mouse wheel about the cursor, each notch at once.
        let f = app.session.wheel_zoom_factor(f64::from(wheel));
        let about = xf.to_world(hp);
        let _ = app.session.zoom_about(f, about);
    }
    let zoom_pinch = ui.input(|i| crate::animation::zoom_delta(i, &opts, app.shows_animations()));
    if inside
        && (zoom_pinch - 1.0).abs() > 1e-4
        && let Some(hp) = hover_pos
    {
        let _ = app.session.zoom_about(f64::from(zoom_pinch), xf.to_world(hp));
    }
    // Pan with the middle button (or Shift+right-drag / two-finger drag on trackpads is scroll).
    let middle_pressed = ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Middle));
    let focused = ui.input(|i| i.focused);
    if middle_down
        && focused
        && (app.canvas.pan_last.is_some() || (middle_pressed && inside))
        && let Some(hp) = hover_pos
    {
        if let Some(last) = app.canvas.pan_last {
            let d = hp - last;
            if let Ok(st) = app.session.state_mut() {
                let v = st.view();
                st.set_view_quiet(cadcraft_engine::View {
                    center: v.center - Vec2::new(f64::from(d.x) / scale, -f64::from(d.y) / scale),
                    height: v.height,
                });
            }
        }
        app.canvas.pan_last = Some(hp);
    } else {
        app.canvas.pan_last = None;
    }
    if dbl_middle && inside {
        let _ = app.session.zoom_extents();
    }
    // ZOOM Realtime: dragging up zooms in, down zooms out.
    if resp.dragged_by(egui::PointerButton::Primary) && app.session.realtime_zoom_active() {
        let _ = app.session.realtime_zoom(f64::from(resp.drag_delta().y));
    }
    // A click in another tile makes it current (or answers -VPORTS Join's prompts).
    if pressed_primary
        && let Some(hp) = hover_pos
        && !rect.contains(hp)
        && let Some(i) = tiles.iter().position(|(r, _)| r.contains(hp))
    {
        if let Err(e) = app.session.click_tile(i) {
            app.session.echo(e.to_string());
        }
        ui.ctx().request_repaint();
        return;
    }
    // Re-read the view after navigation.
    let view = app.session.state().map(|s| s.view()).unwrap_or(view);
    let scale = f64::from(rect.height()) / view.height.max(1e-12);
    let xf = Xf { rect, center: view.center, scale };
    app.canvas.xf = Some(xf);
    // The paper (display) transform; equal to `xf` except inside an MSPACE viewport.
    let (xf_paper, active_vp) = match app.session.state() {
        Ok(st) => {
            let pv = st.paper_view();
            let xp = Xf { rect, center: pv.center, scale: f64::from(rect.height()) / pv.height.max(1e-12) };
            (if st.mspace.is_some() { xp } else { xf }, st.active_viewport().map(|(_, v)| v))
        }
        Err(_) => (xf, None),
    };
    // A clipped active viewport's outline (picks and its heavy border follow it).
    let active_clip = app.session.state().ok().and_then(|st| st.active_viewport().and_then(|(h, v)| cadcraft_render::viewport_clip(&st.doc, h, &v)));
    // Double-click a viewport to work inside it; double-click the sheet outside it to return.
    if inside
        && dbl_primary
        && !app.session.prompting()
        && matches!(app.session.layout_space(), cadcraft_doc::Space::Paper(_))
        && let Some(hp) = hover_pos
    {
        let pp = xf_paper.to_world(hp);
        let in_rect = |v: &cadcraft_doc::Viewport| (pp.x - v.center.x).abs() <= v.width / 2.0 && (pp.y - v.center.y).abs() <= v.height / 2.0;
        let in_active = active_vp.as_ref().is_some_and(|v| active_clip.as_ref().map_or(in_rect(v), |c| c.contains(pp)));
        if !in_active {
            app.canvas.hot_grips.clear();
            app.session.pending_window = None;
            app.canvas.drag_window = false;
            if app.run("mspace", serde_json::json!({"at": [pp.x, pp.y]})).is_err() && active_vp.is_some() {
                let _ = app.run("pspace", serde_json::json!({}));
            }
            app.session.pending_window = None;
            app.canvas.drag_window = false;
            app.canvas.list = None;
            return;
        }
    }
    // Double-click text to edit it, as in AutoCAD: TEXTEDIT with that object already picked.
    if inside
        && dbl_primary
        && app.session.running.is_none()
        && let Some(hp) = hover_pos
        && edit_text_at(app, xf.to_world(hp))
    {
        return;
    }

    // Double-click a hatch (no command running) to edit it in the Hatch Edit dialog. The click is
    // used up: it must not also start a grip edit on the hatch's centre grip below.
    let mut hatch_dialog = false;
    if inside
        && dbl_primary
        && app.session.running.is_none()
        && let Some(hp) = hover_pos
        && let Some(h) = hatch_at(app, xf.to_world(hp), scale)
    {
        app.session.set_selection(vec![h]);
        crate::hatchdlg::open_edit(app);
        hatch_dialog = true;
    }

    // While a command asks for a point, every click places one, however fast the drafting (#815).
    // egui drops a click whose button comes up a few pixels from where it went down (a drag to it),
    // and eframe hands a slow frame's clicks over together, which egui reports as one.
    let point_prompt = app.session.current_prompt().is_some_and(|p| cadcraft_engine::pointmod::accepts(&p))
        && app.session.pending_window.is_none()
        && !app.session.realtime_zoom_active();
    let pressed_primary = pressed_primary || (point_prompt && resp.drag_stopped_by(egui::PointerButton::Primary));
    if point_prompt && inside && pressed_primary && !middle_down && app.cmd.options.is_none() && app.cmd.buffer.is_empty() {
        let releases: Vec<egui::Pos2> = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, .. } => Some(*pos),
                    _ => None,
                })
                .collect()
        });
        // The last one is placed below, at the cursor, like any click.
        if let Some((_, earlier)) = releases.split_last() {
            for pos in earlier.iter().filter(|p| rect.contains(**p)) {
                let w = effective_point(app, xf.to_world(*pos), &xf, ui.ctx());
                let result = match app.canvas.snap {
                    Some(hit) => app.session.input_snapped(hit),
                    None => app.session.input(Input::Point(w)),
                };
                if let Err(e) = result {
                    app.session.echo(e.to_string());
                }
            }
        }
    }

    let raw_world = hover_pos.filter(|_| inside).map(|p| xf.to_world(p));
    let dyn_frame = crate::cmdline::dyn_frame(app);
    if let Some(w) = raw_world {
        let mut eff = effective_point(app, w, &xf, ui.ctx());
        // A locked Dynamic Input value (`40,` or `40<` typed) holds the cursor and overrides snaps.
        if let Some(f) = dyn_frame
            && let Some(e) = crate::dyninput::parse(&app.cmd.buffer, &f)
            && e.first_locked().is_some()
        {
            eff = crate::dyninput::constrain(&e, &f, eff, app.cmd.second_locked());
            app.canvas.snap = None;
            app.canvas.track = None;
        }
        // Grip dimensional input: typed lengths and angles hold the grip and override snaps.
        let grip_held = crate::grip_dyn::constrain(app, eff);
        if let Some(q) = grip_held {
            eff = q;
            app.canvas.snap = None;
            app.canvas.track = None;
        }
        app.canvas.cursor = Some(eff);
        app.session.cursor = eff;
        app.session.cursor_deferred = app.canvas.snap.and_then(|h| h.deferred);
        // Typed or menu snap overrides (END, NON…) and Tab cycling; a locked Dynamic Input value wins.
        if dyn_frame.and_then(|f| crate::dyninput::parse(&app.cmd.buffer, &f)).is_none_or(|e| e.first_locked().is_none()) {
            override_cursor(app, w, &xf);
        }
        // Typed grip dimensions win over snap overrides too.
        if let Some(q) = grip_held {
            app.canvas.cursor = Some(q);
            app.session.cursor = q;
            app.canvas.snap = None;
            app.session.cursor_deferred = None;
        }
    } else {
        app.canvas.cursor = None;
        app.canvas.snap = None;
        app.session.cursor_deferred = None;
    }

    // A click on the Dynamic Input options menu picks the keyword; elsewhere it closes the menu.
    let mut picked = false;
    if pressed_primary && app.cmd.options.is_some() {
        match hover_pos.and_then(|h| app.cmd.option_at(h)) {
            Some(k) => {
                crate::cmdline::pick_option(app, k);
                picked = true;
            }
            None => app.cmd.options = None,
        }
    }
    // A double-click with no command running (on a layout viewport, say) leaves no selection
    // window behind; while a command asks for points, every click still counts (#815).
    let idle_dbl = dbl_primary && app.session.running.is_none();
    if idle_dbl {
        app.session.pending_window = None;
        app.canvas.drag_window = false;
    }
    let pressed_primary = pressed_primary && !picked && !hatch_dialog && !idle_dbl;
    let key: CycleKey = (
        app.session.running.as_ref().map(|r| r.id.clone()),
        app.session.current_prompt().map(|p| p.message),
        app.session.state().map_or(0, |st| st.uid),
        app.session.state().map_or(0, |st| st.revision),
        xf.center,
        xf.scale,
    );
    if app.canvas.cycle_key.as_ref() != Some(&key) {
        app.canvas.cycle = None;
        app.canvas.hover_cycle = None;
        app.canvas.cycle_key = Some(key);
    }
    let cycled = pressed_primary && !middle_down && cycle_click(app, &xf, hover_pos, raw_world.filter(|_| inside), mods.shift);
    // Clicks.
    if inside
        && pressed_primary
        && !middle_down
        && !cycled
        && let Some(p) = app.canvas.cursor
    {
        if app.session.prompting() {
            // Ctrl held while picking the point of a grip edit makes copies (the Copy option).
            if mods.ctrl && app.session.running.as_ref().is_some_and(|r| r.id == "grip") {
                let _ = app.session.input(Input::Keyword("Copy".into()));
            }
            // Clicking places the point; a value typed into the Dynamic Input boxes is used up.
            if let Some(f) = dyn_frame
                && crate::dyninput::parse(&app.cmd.buffer, &f).is_some()
            {
                app.cmd.buffer.clear();
            }
            // Preserve a snap's viewport origin for DIST, and its deferred curve where needed.
            let result = match app.canvas.snap {
                Some(hit) => app.session.input_snapped(hit),
                None => app.session.input(Input::Point(p)),
            };
            if let Err(e) = result {
                app.session.echo(e.to_string());
            }
        } else if let Some(g) = crate::dyngrips::at(app, &xf, hover_pos) {
            crate::dyngrips::click(app, g);
        } else if let Some(g) = grip_at(app, &xf, hover_pos) {
            click_grip(app, g, mods.shift);
        } else if let Err(e) = app.session.idle_click(raw_world.unwrap_or(p), mods.shift) {
            app.session.echo(e.to_string());
        }
    }
    // Press-and-drag selection window (PICKDRAG = 2; click-click still works): a drag
    // that starts while objects are being selected opens a window where the button went down,
    // and letting go closes it. Drags on grips and drags while drawing are left alone.
    if !app.session.settings.pickdrag {
        app.canvas.drag_window = false;
    }
    if app.session.settings.pickdrag
        && resp.drag_started_by(egui::PointerButton::Primary)
        && !middle_down
        && app.canvas.cycle.is_none()
        && app.session.pending_window.is_none()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        && rect.contains(origin)
        && grip_at(app, &xf, Some(origin)).is_none()
        && crate::dyngrips::at(app, &xf, Some(origin)).is_none()
    {
        app.canvas.drag_window = app.session.begin_window(xf.to_world(origin));
    }
    if app.canvas.drag_window && resp.drag_stopped_by(egui::PointerButton::Primary) {
        app.canvas.drag_window = false;
        if app.session.pending_window.is_some()
            && let Some(end) = ui.input(|i| i.pointer.interact_pos())
        {
            let end = xf.to_world(end);
            let done = if app.session.prompting() { app.session.input(Input::Point(end)) } else { app.session.idle_click(end, mods.shift) };
            if let Err(e) = done {
                app.session.echo(e.to_string());
            }
        }
    }
    // Right-click acts as Enter while a command runs (the classic CAD default; held, it opens the
    // command menu); otherwise it opens the shortcut menu (Repeat, Clipboard, …).
    // Shift/Ctrl+right-click opens the object snap menu.
    let idle = !app.session.prompting();
    if inside && pressed_secondary {
        app.canvas.snap_menu = mods.shift || mods.ctrl;
    }
    if crate::context_menu::in_command(app, ui, inside, pressed_secondary) {
        let _ = app.session.input(Input::Enter);
    }
    // Right-click on a grip with options (polyline vertex, line end…) opens the grip menu.
    if inside && pressed_secondary && idle && !app.canvas.snap_menu {
        app.canvas.grip_menu = grip_at(app, &xf, hover_pos).filter(|g| !grip_options(app, *g).is_empty());
    }
    crate::dyngrips::menu(app, ui.ctx(), &xf);
    if idle && !app.canvas.snap_menu && app.canvas.grip_menu.is_some() {
        grip_menu(app, &resp);
    } else if idle || app.canvas.snap_menu {
        crate::context_menu::show(app, &resp);
    }

    // Hover highlight (throttled to cursor movement).
    // The cursor picks objects (pick box, rollover highlight): "Select objects" and single-object
    // prompts, or no command with PICKFIRST on; SELECTIONPREVIEW says in which of those it shows.
    let selecting = app.session.previews_selection();
    if inside && selecting && app.session.pending_window.is_none() {
        if hover_pos != app.canvas.hover_at
            && let (Some(w), Ok(st)) = (raw_world, app.session.state())
        {
            let filter = app.session.settings.previewfilter;
            app.canvas.hover = cadcraft_engine::select::pick(&st.doc, &st.edit_space(), w, app.session.settings.pickbox.max(1.0) * 1.5 / scale)
                .filter(|h| st.doc.entity(*h).is_none_or(|e| !cadcraft_engine::select::preview_filtered(&st.doc, e, filter)));
            app.canvas.hover_at = hover_pos;
        }
    } else {
        app.canvas.hover = None;
        app.canvas.hover_at = None;
    }
    // The open selection cycling list highlights the object of the row under the pointer.
    if app.canvas.cycle.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.canvas.cycle = None;
    }
    if let Some(c) = &app.canvas.cycle {
        app.canvas.hover = hover_pos.and_then(|hp| c.row_at(hp));
        app.canvas.hover_at = None;
    }
    shift_space_cycle(app, ui, hover_pos.filter(|_| inside), raw_world);
    if inside && hover_pos != app.canvas.cycle_badge_at {
        app.canvas.cycle_badge = raw_world.is_some_and(|w| app.session.cycling_badge(w));
        app.canvas.cycle_badge_at = hover_pos;
    } else if !inside {
        app.canvas.cycle_badge = false;
        app.canvas.cycle_badge_at = None;
    }

    // ---------- paint ----------
    painter.rect_filled(rect, 0.0, t.canvas);
    let t0 = crate::now_ms();
    let mut views = vec![(xf_paper.world_bounds(), 1.0 / xf_paper.scale)];
    views.extend(tiles.iter().filter(|(r, tile)| !tile.active && r.width() >= 1.0 && r.height() >= 1.0).map(|(r, tile)| {
        let xf = tile_xf(*r, tile);
        (xf.world_bounds(), 1.0 / xf.scale)
    }));
    ensure_list(app, &xf_paper, &views, ui.ctx());
    let sheet = app.canvas.list.as_ref().and_then(|l| l.sheet);
    let surround = Color32::from_rgb(0x50, 0x57, 0x63);
    let (bg, paper_rect) = match sheet {
        Some(sh) => {
            // Paper space: grey surround, the sheet with a shadow, the printable area dashed.
            painter.rect_filled(rect, 0.0, surround);
            let a = xf_paper.to_screen(Vec2::new(0.0, sh.size.y));
            let b = xf_paper.to_screen(Vec2::new(sh.size.x, 0.0));
            let paper = Rect::from_two_pos(a, b);
            painter.rect_filled(paper.translate(vec2(5.0, 5.0)), 0.0, Color32::from_black_alpha(110));
            painter.rect_filled(paper, 0.0, t.paper);
            let pa = Rect::from_two_pos(
                xf_paper.to_screen(Vec2::new(sh.printable.min.x, sh.printable.max.y)),
                xf_paper.to_screen(Vec2::new(sh.printable.max.x, sh.printable.min.y)),
            );
            let pts = [pa.left_top(), pa.right_top(), pa.right_bottom(), pa.left_bottom(), pa.left_top()];
            painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, Color32::from_gray(150)), 4.0, 4.0));
            (Rgb(t.paper.r(), t.paper.g(), t.paper.b()), Some(paper))
        }
        None => {
            blockedit::wash(app, &painter, rect);
            draw_grid(app, &painter, &xf);
            (Rgb(t.canvas.r(), t.canvas.g(), t.canvas.b()), None)
        }
    };
    let sel = app.session.selection();
    // How the object under the cursor and the selected ones are highlighted (PREVIEWEFFECT,
    // HIGHLIGHT, SELECTIONEFFECT); with HIGHLIGHT off only their grips show.
    let (preview_look, selection_look) = (Look::preview(&app.session.settings), Look::selection(&app.session.settings));
    // The active viewport's contents, drawn apart from the layout's list, under its own objects.
    // `masked`: its outline is in the stencil buffer this frame.
    let mut masked = false;
    if let Some(f) = ensure_vp_content(app, &xf_paper, ui.ctx()) {
        let gpu = app.canvas.gpu.is_some();
        let clipped = painter.with_clip_rect(f.on_screen.intersect(rect));
        draw_vp_content(&mut app.canvas.vp, gpu, &clipped, &f, &xf_paper, bg, app.session.settings.lwdisplay);
        masked = gpu && f.outline.is_some();
    }
    if app.canvas.gpu.is_some() {
        draw_list_gpu(&mut app.canvas, &painter, &xf_paper, bg, app.session.settings.lwdisplay);
    } else if let Some(list) = &app.canvas.list {
        draw_list(&painter, &xf_paper, list, bg, app.session.settings.lwdisplay);
    }
    if let Some(vp) = &active_vp {
        // The active viewport: a heavy border, highlights in model units clipped to it.
        let r = Rect::from_two_pos(
            xf_paper.to_screen(Vec2::new(vp.center.x - vp.width / 2.0, vp.center.y + vp.height / 2.0)),
            xf_paper.to_screen(Vec2::new(vp.center.x + vp.width / 2.0, vp.center.y - vp.height / 2.0)),
        );
        let heavy = Stroke::new(3.0, Color32::from_rgb(0x2a, 0x2a, 0x2a));
        match &active_clip {
            Some(c) => {
                let mut pts: Vec<egui::Pos2> = c.polygon().iter().map(|q| xf_paper.to_screen(*q)).collect();
                pts.extend(pts.first().copied());
                painter.add(egui::Shape::line(pts, heavy));
            }
            None => {
                painter.rect_stroke(r, 0.0, heavy, egui::StrokeKind::Middle);
            }
        }
        if let Ok(st) = app.session.state() {
            // A clipped viewport's outline bounds (its outline is masked when `masked`).
            let bounds = active_clip.as_ref().map_or(r, |c| {
                let b = c.bounds();
                Rect::from_two_pos(xf_paper.to_screen(Vec2::new(b.min.x, b.max.y)), xf_paper.to_screen(Vec2::new(b.max.x, b.min.y)))
            });
            let clipped = painter.with_clip_rect(bounds.intersect(rect));
            let c = &mut app.canvas;
            if let Some(h) = c.hover
                && !sel.contains(&h)
            {
                c.vp_hover.update(st, &[h], scale);
                let list = &c.vp_hover.list;
                draw_highlight(&clipped, &xf, list, &(0..list.prims.len()).collect::<Vec<_>>(), t.hover, preview_look);
            }
            if let Some(look) = selection_look
                && !sel.is_empty()
            {
                c.vp_sel.update(st, &sel, scale);
                let list = &c.vp_sel.list;
                let all = || (0..list.prims.len()).collect::<Vec<_>>();
                if c.gpu.is_some() {
                    // Shares the GPU highlight layer with the layout's own selection; the high bit
                    // keeps this list's generations apart from the layout list's.
                    let list_id = c.vp_sel.list_gen | 1 << 63;
                    let stencil = if masked { crate::gpu::Stencil::Inside } else { crate::gpu::Stencil::Off };
                    let hl = (&mut c.hl_key, &c.hl_slot, &mut c.hl_origin, &mut c.hl_infinite);
                    draw_highlight_gpu(&clipped, &xf, list, list_id, all, &sel, t.selection, look, hl, stencil);
                    draw_highlight(&clipped, &xf, list, &c.hl_infinite, t.selection, look);
                } else {
                    draw_highlight(&clipped, &xf, list, &all(), t.selection, look);
                }
            }
        }
    } else if let CanvasState { list: Some(list), list_gen, prim_index, hover, gpu, hl_key, hl_slot, hl_origin, hl_infinite, .. } = &mut app.canvas {
        if let Some(h) = *hover
            && !sel.contains(&h)
        {
            draw_highlight(&painter, &xf, list, &prim_index.prims(*list_gen, list, &[h]), t.hover, preview_look);
        }
        if let Some(look) = selection_look {
            if gpu.is_some() && !sel.is_empty() {
                let list_id = *list_gen;
                let hl = (hl_key, &*hl_slot, hl_origin, &mut *hl_infinite);
                draw_highlight_gpu(
                    &painter,
                    &xf,
                    list,
                    list_id,
                    || prim_index.prims(list_id, list, &sel),
                    &sel,
                    t.selection,
                    look,
                    hl,
                    crate::gpu::Stencil::Off,
                );
                draw_highlight(&painter, &xf, list, hl_infinite, t.selection, look);
            } else {
                draw_highlight(&painter, &xf, list, &prim_index.prims(*list_gen, list, &sel), t.selection, look);
            }
        }
    }
    app.canvas.draw_ms = crate::now_ms() - t0;
    compare::paint(app, ui.ctx(), &painter, &xf_paper, rect);
    blockedit::paint(app, ui.ctx(), &painter, &xf_paper, rect);
    // Constraint bars and dynamic dimensional constraints (model space, or inside a viewport).
    if sheet.is_none() || active_vp.is_some() {
        let clip = match &active_vp {
            Some(vp) => Rect::from_two_pos(
                xf_paper.to_screen(Vec2::new(vp.center.x - vp.width / 2.0, vp.center.y + vp.height / 2.0)),
                xf_paper.to_screen(Vec2::new(vp.center.x + vp.width / 2.0, vp.center.y - vp.height / 2.0)),
            )
            .intersect(rect),
            None => rect,
        };
        let p = painter.with_clip_rect(clip);
        crate::parametric::draw_overlay(app, ui, &p, &xf);
    }
    // Grips on selected objects (when idle or editing grips).
    let grip_edit = app.session.running.as_ref().is_some_and(|r| r.id == "grip");
    if (!grip_edit && (app.canvas.grip_editing || app.session.prompting())) || sel.is_empty() {
        app.canvas.hot_grips.clear();
    }
    if !grip_edit {
        app.canvas.dyn_hot = None;
    }
    app.canvas.grip_editing = grip_edit;
    if (!app.session.prompting() || grip_edit)
        && app.session.shows_grips(sel.len())
        && let Ok(st) = app.session.state()
    {
        let gs = app.session.settings.gripsize as f32;
        for h in sel.iter().take(200) {
            if let Some(e) = st.doc.entity(*h) {
                let grips = e.kind.grips();
                let count = grips.len();
                for (gi, g) in grips.into_iter().enumerate() {
                    let sp = xf.to_screen(g);
                    if rect.contains(sp) {
                        let r = grip_rect(&e.kind, count, gi, sp, gs);
                        let hot = app.canvas.hot_grips.contains(&(*h, gi));
                        let warm = !hot && hover_pos.is_some_and(|hp| r.expand(2.0).contains(hp));
                        painter.rect_filled(
                            r,
                            0.0,
                            if hot {
                                t.grip_hot
                            } else if warm {
                                Color32::from_rgb(0xff, 0x7f, 0x9f)
                            } else {
                                t.grip
                            },
                        );
                        painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(0x10, 0x30, 0x80)), egui::StrokeKind::Middle);
                    }
                }
            }
        }
        crate::dyngrips::draw(app, &painter, &xf, rect, hover_pos);
    }
    // Rubber band preview of the active command.
    // `xf` is the space being edited: paper space, or model space inside the active viewport.
    let drag = match (app.canvas.cursor, app.canvas.gpu.is_some()) {
        (Some(c), true) => app.session.drag(c),
        _ => None,
    };
    if let Some((drag, extra)) = drag {
        if let Ok(st) = app.session.state() {
            let serial = app.session.clipboard_source.as_ref().map_or(0, |c| c.serial);
            let transparency = app.session.settings.transparency_display;
            draw_drag_gpu(&mut app.canvas, &painter, &xf, &st.doc, &drag, (&app.session.clipboard, serial), bg, transparency);
            let list = rebuilt_preview(&st.doc, extra.iter(), scale);
            draw_list(&painter, &xf, &list, bg, false);
        }
    } else if let (Some(c), true) = (app.canvas.cursor, app.session.prompting()) {
        let ents = app.session.preview(c);
        if !ents.is_empty()
            && let Ok(st) = app.session.state()
        {
            let list = rebuilt_preview(&st.doc, ents.iter(), scale);
            draw_list(&painter, &xf, &list, bg, false);
        }
    }
    // Pending selection window.
    if let (Some(pw), Some(c)) = (app.session.pending_window, raw_world) {
        let a = xf.to_screen(pw.corner);
        let b = xf.to_screen(c);
        let r = Rect::from_two_pos(a, b);
        let crossing = c.x < pw.corner.x;
        if let Some(fill) = selection_area_fill(&app.session.settings, crossing) {
            painter.rect_filled(r, 0.0, fill);
        }
        let st = Stroke::new(1.0, if crossing { t.crossing_stroke } else { t.window_stroke });
        if crossing {
            let pts = vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.extend(Shape::dashed_line(&pts, st, 5.0, 3.0));
        } else {
            painter.rect_stroke(r, 0.0, st, egui::StrokeKind::Middle);
        }
    }
    // Acquired tracking points and the polar / tracking paths under the cursor.
    if app.canvas.cursor.is_some() {
        draw_tracking(&painter, &xf, &app.session.tracking.acquired, app.canvas.track.as_ref(), view.height * 4.0);
    }
    if let (Some(hit), Some(_)) = (app.canvas.snap, app.canvas.cursor) {
        draw_snap_marker(&painter, xf.to_screen(hit.point), &hit);
    }
    draw_tiles(app, ui, &tiles);
    // UCS icon, ViewCube and viewport label.
    if app.ui.show_ucs_icon {
        draw_ucs_icon(&painter, rect);
        crate::viewcube::ucs_icon(app, ui, rect);
    }
    if app.ui.show_viewcube && sheet.is_none() {
        draw_viewcube(app, ui, rect);
    }
    viewport_label(&painter, rect);
    // Typing and Escape reach the command line even when its overlay is hidden.
    crate::cmdline::keyboard(app, ui.ctx());
    // Command line overlay.
    if app.ui.show_command_line {
        crate::cmdline::show(app, ui, full);
    }
    // Crosshair cursor.
    if inside && !on_menu {
        // Hide the OS cursor under the drawn crosshair, unless assistive tech needs it (#39).
        ui.ctx().set_cursor_icon(if app.ui.system_cursor { egui::CursorIcon::Crosshair } else { egui::CursorIcon::None });
        if let Some(hp) = hover_pos {
            // Use the painted sheet even in MSPACE, where the editing transform follows a
            // model viewport. The whole cursor takes one colour from its centre.
            let ink = paper_rect.map_or(t.crosshair, |paper| crate::theme::crosshair_color(if paper.contains(hp) { t.paper } else { surround }));
            draw_crosshair(app, &painter, rect, hp, selecting, ink);
            if app.canvas.cycle_badge && app.canvas.cycle.is_none() {
                cycle_badge(&painter, hp);
            }
        }
    }
    if inside
        && app.session.settings.dynmode
        && let Some(hp) = hover_pos
    {
        // A stretched grip shows its dimensions instead of the pointer boxes; a prompt asking for
        // a distance shows its one field and keeps the prompt and its options.
        let fields = crate::grip_dyn::draw(app, &painter);
        if !fields || app.session.current_prompt().is_some_and(|p| p.distance) {
            dynamic_input(app, &painter, hp, fields);
        }
    }
    // Over the crosshair, so the row under the pointer reads clearly.
    cycle_list(app, &painter);
    // No continuous repaint while hovering: input events repaint by themselves. Repainting every
    // frame kept a vsync swapchain's queue full (every frame waited behind queued ones) and would
    // spin the CPU with the low-latency present mode (`gpu::surface_config`).
}

/// The model space tiles on screen (none without tiling): each tile's rectangle in `full`.
/// Start TEXTEDIT on the text, mtext or attribute definition at `p` (the objects its shortcut menu
/// offers Edit Text for); false when there is none.
fn edit_text_at(app: &mut CadApp, p: Vec2) -> bool {
    use cadcraft_engine::doc::EntityKind;
    let is_text = cadcraft_engine::cmd::curves::pick_at(&app.session, p).and_then(|h| {
        app.session.doc().ok()?.entity(h).map(|e| matches!(e.kind, EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::AttDef(_)))
    });
    if is_text != Some(true) {
        return false;
    }
    app.start("textedit");
    if let Err(e) = app.session.input(Input::Point(p)) {
        app.session.echo(e.to_string());
    }
    true
}

fn tile_rects(app: &CadApp, full: Rect) -> Vec<(Rect, cadcraft_engine::ModelTile)> {
    let x = |f: f64| full.left() + full.width() * f.clamp(0.0, 1.0) as f32;
    let y = |f: f64| full.bottom() - full.height() * f.clamp(0.0, 1.0) as f32;
    app.session
        .model_tiles()
        .into_iter()
        .map(|t| (Rect::from_min_max(pos2(x(t.min.x), y(t.max.y)), pos2(x(t.max.x), y(t.min.y))).shrink(1.0), t))
        .collect()
}

/// The world ↔ screen transform of a model tile drawn in `r`.
fn tile_xf(r: Rect, tile: &cadcraft_engine::ModelTile) -> Xf {
    Xf { rect: r, center: tile.view.center, scale: f64::from(r.height()) / tile.view.height.max(1e-12) }
}

/// The tiles other than the current one (each with its own view, clipped to it), then the
/// tile borders: thin, and heavy around the current tile.
fn draw_tiles(app: &CadApp, ui: &egui::Ui, tiles: &[(Rect, cadcraft_engine::ModelTile)]) {
    let t = Tokens::get();
    let bg = Rgb(t.canvas.r(), t.canvas.g(), t.canvas.b());
    for (r, tile) in tiles.iter().filter(|(r, tile)| !tile.active && r.width() >= 1.0 && r.height() >= 1.0) {
        let p = ui.painter_at(*r);
        let xf = tile_xf(*r, tile);
        p.rect_filled(*r, 0.0, t.canvas);
        draw_grid(app, &p, &xf);
        if let Some(list) = &app.canvas.list {
            draw_list(&p, &xf, list, bg, app.session.settings.lwdisplay);
        }
        viewport_label(&p, *r);
    }
    let p = ui.painter();
    for (r, tile) in tiles {
        let (w, c) = if tile.active { (2.0, t.accent) } else { (1.0, t.border) };
        p.rect_stroke(*r, 0.0, Stroke::new(w, c), egui::StrokeKind::Outside);
    }
}

/// A click while selection cycling's list is open picks the row under it (anywhere else just
/// closes the list); otherwise a click over overlapping objects that a pick would choose between
/// opens the list (SELECTIONCYCLING 2). True when the click was used up here.
fn cycle_click(app: &mut CadApp, xf: &Xf, hover: Option<Pos2>, world: Option<Vec2>, shift: bool) -> bool {
    // Only where a click picks objects; a click that places a point goes through.
    let picks = app.session.click_picks();
    if let Some((_, hs, i)) = app.canvas.hover_cycle.take()
        && picks
        && let Some(h) = hs.get(i % hs.len().max(1))
    {
        if let Err(e) = app.session.pick_cycled(*h, shift) {
            app.session.echo(e.to_string());
        }
        return true;
    }
    if let Some(list) = app.canvas.cycle.take() {
        let Some(h) = hover.and_then(|hp| list.row_at(hp)).filter(|_| picks) else { return picks };
        if let Err(e) = app.session.pick_cycled(h, list.shift) {
            app.session.echo(e.to_string());
        }
        return true;
    }
    let (Some(hp), Some(w)) = (hover, world) else { return false };
    if crate::dyngrips::at(app, xf, hover).is_some() || grip_at(app, xf, hover).is_some() {
        return false;
    }
    let hs = app.session.cycling_candidates(w);
    let Ok(d) = app.session.doc() else { return false };
    let items: Vec<_> =
        hs.into_iter().filter_map(|h| d.entity(h).map(|e| (h, crate::i18n::t(e.kind.type_name()).to_string(), e.common.layer.clone()))).collect();
    if items.len() < 2 {
        return false;
    }
    app.canvas.cycle = Some(CycleList { at: hp, items, shift, rows: Vec::new() });
    true
}

/// Shift+Space over overlapping objects highlights the next one under the cursor, and a click
/// picks the highlighted one, whether selection cycling is on or not; moving the cursor off the
/// pick box where it started ends it.
fn shift_space_cycle(app: &mut CadApp, ui: &egui::Ui, hover: Option<Pos2>, world: Option<Vec2>) {
    let pick_px = (app.session.settings.pickbox.max(1.0) * 1.5) as f32;
    let (Some(hp), Some(w)) = (hover, world) else {
        app.canvas.hover_cycle = None;
        return;
    };
    if app.canvas.hover_cycle.as_ref().is_some_and(|(a, _, _)| a.distance(hp) > pick_px) {
        app.canvas.hover_cycle = None;
    }
    let pressed = ui
        .input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::Space, pressed: true, modifiers, .. } if modifiers.shift)));
    if pressed {
        // With the list open Shift+Space does nothing, and isn't Enter either.
        let next = match app.canvas.hover_cycle.take() {
            _ if app.canvas.cycle.is_some() => None,
            Some((a, hs, i)) => Some((a, hs, i + 1)),
            None => Some((hp, app.session.pick_candidates(w), 1)).filter(|(_, hs, _)| hs.len() > 1),
        };
        if next.is_some() || app.canvas.cycle.is_some() {
            // Space is Enter at the command line; this one only steps the highlight.
            ui.ctx().input_mut(|i| {
                i.events.retain(|e| !matches!(e, egui::Event::Key { key: egui::Key::Space, .. }) && !matches!(e, egui::Event::Text(t) if t == " "))
            });
        }
        app.canvas.hover_cycle = next;
    }
    if let Some((_, hs, i)) = &app.canvas.hover_cycle {
        app.canvas.hover = hs.get(i % hs.len().max(1)).copied();
        app.canvas.hover_at = None;
    }
}

/// Selection cycling's badge beside the cursor: two overlapping squares, objects overlap here.
fn cycle_badge(p: &egui::Painter, at: Pos2) {
    let t = Tokens::get();
    for off in [vec2(14.0, -22.0), vec2(19.0, -17.0)] {
        let r = Rect::from_min_size(at + off, vec2(9.0, 9.0));
        p.rect_filled(r, 1.0, t.cmd_bg);
        p.rect_stroke(r, 1.0, Stroke::new(1.0, t.text), egui::StrokeKind::Inside);
    }
}

/// Whether `p` lies on a menu the canvas paints itself — the selection cycling list's rows or the
/// Dynamic Input options menu's rows. They are menus, so the OS cursor stays visible over them, but
/// they share the canvas's layer: `layer_id_at` reports the drawing, and only their rectangles can
/// tell them apart from it (#1701).
fn over_canvas_menu(app: &CadApp, p: Pos2) -> bool {
    app.canvas.cycle.as_ref().is_some_and(|c| c.rows.iter().any(|r| r.expand(2.0).contains(p)))
        || app.cmd.option_rows.iter().any(|(r, _)| r.expand(2.0).contains(p))
}

/// The selection cycling list beside the click that opened it: a row per object, its type and
/// layer, the row under the pointer highlighted; the rows are kept for clicks.
fn cycle_list(app: &mut CadApp, p: &egui::Painter) {
    let Some(list) = app.canvas.cycle.as_mut() else { return };
    let t = Tokens::get();
    let font = crate::theme::small();
    let galleys: Vec<_> = list
        .items
        .iter()
        .map(|(_, ty, layer)| (p.layout_no_wrap(ty.clone(), font.clone(), t.text), p.layout_no_wrap(layer.clone(), font.clone(), t.text_dim)))
        .collect();
    let row_h = galleys.iter().map(|(a, _)| a.size().y).fold(12.0, f32::max) + 6.0;
    let ty_w = galleys.iter().map(|(a, _)| a.size().x).fold(40.0, f32::max);
    let layer_w = galleys.iter().map(|(_, b)| b.size().x).fold(20.0, f32::max);
    let w = ty_w + layer_w + 36.0;
    let m = Rect::from_min_size(list.at + vec2(14.0, 14.0), vec2(w, row_h * galleys.len() as f32 + 4.0));
    p.rect_filled(m, 3.0, t.cmd_bg);
    p.rect_stroke(m, 3.0, Stroke::new(1.0, t.cmd_border), egui::StrokeKind::Inside);
    let hover = app.canvas.hover;
    list.rows.clear();
    for (i, ((ty, layer), (h, _, _))) in galleys.into_iter().zip(&list.items).enumerate() {
        let row = Rect::from_min_size(pos2(m.left() + 2.0, m.top() + 2.0 + row_h * i as f32), vec2(w - 4.0, row_h));
        if hover == Some(*h) {
            p.rect_filled(row, 2.0, t.cmd_keyword_hover);
        }
        p.galley(pos2(row.left() + 8.0, row.center().y - ty.size().y / 2.0), ty, t.text);
        p.galley(pos2(row.left() + 20.0 + ty_w, row.center().y - layer.size().y / 2.0), layer, t.text_dim);
        list.rows.push(row);
    }
}

fn tooltip(p: &egui::Painter, at: Pos2, text: &str) {
    let galley = p.layout_no_wrap(text.to_string(), crate::theme::small(), Color32::from_rgb(0x20, 0x20, 0x20));
    let r = Rect::from_min_size(at, galley.size() + vec2(8.0, 4.0));
    p.rect_filled(r, 2.0, Color32::from_rgb(0xf4, 0xf4, 0xf4));
    p.galley(r.min + vec2(4.0, 2.0), galley, Color32::BLACK);
}

/// The hatch under world point `w` within the pick aperture (PICKBOX × 1.5 pixels at `scale`
/// pixels per unit), if any.
fn hatch_at(app: &CadApp, w: Vec2, scale: f64) -> Option<Handle> {
    let st = app.session.state().ok()?;
    let aperture = app.session.settings.pickbox.max(1.0) * 1.5 / scale.max(1e-12);
    let h = cadcraft_engine::select::pick(&st.doc, &st.edit_space(), w, aperture)?;
    matches!(st.doc.entity(h)?.kind, cadcraft_doc::EntityKind::Hatch(_)).then_some(h)
}

/// Half the side of the pick box: the pick aperture (PICKBOX × 1.5 pixels, as `select::pick` is
/// called with), so the square shows what a click hits. `None` when PICKBOX is 0 (no box).
/// The candidate Tab has cycled to, if cycling: a Tab press starts or advances it, and the
/// cursor leaving the aperture where it started ends it.
fn snap_cycle(app: &mut CadApp, raw: Vec2, ap: f64) -> Option<usize> {
    let tab = std::mem::take(&mut app.canvas.snap_tab);
    let c = &mut app.canvas.snap_cycle;
    if c.is_some_and(|c| c.anchor.dist(raw) > ap) {
        *c = None;
    }
    if tab {
        *c = Some(c.map_or(SnapCycle { anchor: raw, index: 1 }, |c| SnapCycle { index: c.index + 1, ..c }));
    }
    c.map(|c| c.index)
}

/// The cursor under a one-pick object snap override (`Session::snap_override`: END, NON…) or
/// while Tab cycles the candidates: the snap of that mode (or the cycled candidate) replaces
/// what the running snaps found; with none, a running snap no longer holds the cursor.
fn override_cursor(app: &mut CadApp, raw: Vec2, xf: &Xf) {
    let over = app.session.snap_override();
    let ap = app.session.settings.aperture / xf.scale;
    let cycle = snap_cycle(app, raw, ap);
    let prompt = app.session.current_prompt();
    let wants_point = prompt.as_ref().is_some_and(cadcraft_engine::pointmod::accepts);
    if (over.is_none() && cycle.is_none()) || !wants_point || app.session.pending_window.is_some() {
        return;
    }
    let base = prompt.as_ref().and_then(|p| p.base);
    let deferred = prompt.as_ref().is_some_and(|p| p.deferred);
    let s = &app.session.settings;
    let osmode = over.unwrap_or(s.osmode) | if s.osnaphatch { snap::mode::HATCH } else { 0 };
    let Ok(st) = app.session.state() else { return };
    let hit = match cycle {
        Some(i) => {
            let cands = cadcraft_engine::pointmod::snap_candidates(&st.doc, &st.edit_space(), raw, ap, osmode, base, deferred);
            cands.get(i % cands.len().max(1)).copied()
        }
        None => snap::osnap(&st.doc, &st.edit_space(), raw, ap, osmode, base, deferred),
    };
    let p = match (hit, app.canvas.snap) {
        (Some(h), _) => {
            app.canvas.snap = Some(h);
            h.point
        }
        (None, Some(_)) => {
            app.canvas.snap = None;
            raw
        }
        (None, None) => return,
    };
    app.canvas.cursor = Some(p);
    app.session.cursor = p;
    app.session.cursor_deferred = app.canvas.snap.and_then(|h| h.deferred);
}

fn pickbox_half(pickbox: f64) -> Option<f32> {
    let half = (pickbox * 1.5) as f32;
    (half.is_finite() && half >= 1.0).then_some(half)
}

fn draw_crosshair(app: &CadApp, p: &egui::Painter, rect: Rect, at: Pos2, pickbox: bool, ink: Color32) {
    let st = Stroke::new(1.0, ink);
    let len = (rect.height() * app.session.settings.cursorsize as f32 / 100.0).max(12.0);
    let pb = if pickbox { pickbox_half(app.session.settings.pickbox) } else { None };
    let gap = pb.unwrap_or(0.0);
    // Along the isometric plane's axes in isometric drafting.
    if app.session.settings.isodraft {
        for a in cadcraft_engine::snap::iso::cursor_axes(&app.session.settings) {
            let dir = vec2(a.cos() as f32, -(a.sin() as f32));
            p.line_segment([at - dir * len, at - dir * gap], st);
            p.line_segment([at + dir * gap, at + dir * len], st);
        }
    } else {
        p.line_segment([pos2(at.x - len, at.y), pos2(at.x - gap, at.y)], st);
        p.line_segment([pos2(at.x + gap, at.y), pos2(at.x + len, at.y)], st);
        p.line_segment([pos2(at.x, at.y - len), pos2(at.x, at.y - gap)], st);
        p.line_segment([pos2(at.x, at.y + gap), pos2(at.x, at.y + len)], st);
    }
    if let Some(pb) = pb {
        p.rect_stroke(Rect::from_center_size(at, vec2(pb * 2.0, pb * 2.0)), 0.0, st, egui::StrokeKind::Middle);
    }
}

/// The Dynamic Input tooltip beside the cursor, styled like the command line. At a point prompt
/// it shows the pair of value boxes, a view of the command-line text (see [`crate::dyninput`]);
/// otherwise the prompt with what is being typed, or the live distance and angle. With `fields`
/// (a distance prompt's field is on show, [`crate::grip_dyn`]) only the prompt and its options.
fn dynamic_input(app: &mut CadApp, p: &egui::Painter, hover: Pos2, fields: bool) {
    app.cmd.option_rows.clear();
    // While the options menu is open the tooltip stays where it opened, so the menu can be reached.
    let at = match app.cmd.options {
        Some(_) => *app.cmd.options_anchor.get_or_insert(hover),
        None => {
            app.cmd.options_anchor = None;
            hover
        }
    };
    let Some(prompt) = app.session.current_prompt() else { return };
    // DYNPROMPT = 0: no prompt beside the cursor (the value boxes stay).
    let show_prompt = app.session.settings.dynprompt;
    let options = show_prompt && !prompt.keywords.is_empty();
    if !fields
        && let Some(f) = crate::cmdline::dyn_frame(app)
        && let Some(e) = crate::dyninput::parse(&app.cmd.buffer, &f)
        && let Some(c) = app.canvas.cursor
    {
        let message = if show_prompt { crate::i18n::t(&prompt.message) } else { "" };
        let r = dynamic_input_boxes(p, at, message, &f, &e, c, app.cmd.second_locked(), options, length_units(&app.session));
        options_menu(app, p, r, hover);
        return;
    }
    if !show_prompt {
        return;
    }
    let mut text = crate::i18n::t(&prompt.message).to_string();
    if fields {
        // The field shows the distance and what is typed.
    } else if !app.cmd.buffer.is_empty() {
        text = format!("{}: {}", crate::i18n::t(&prompt.message), app.cmd.buffer);
    } else if let (Some(base), Some(c)) = (prompt.base, app.canvas.cursor) {
        text = format!(
            "{}   {}  <  {}°",
            crate::i18n::t(&prompt.message),
            distance_text(&app.session, base.dist(c)),
            (base.angle_to(c).to_degrees() * 10.0).round() / 10.0
        );
    } else if let Some(c) = app.canvas.cursor {
        text = format!("{}   {}, {}", crate::i18n::t(&prompt.message), distance_text(&app.session, c.x), distance_text(&app.session, c.y));
    }
    let t = Tokens::get();
    let galley = p.layout_no_wrap(text, crate::theme::small(), t.text);
    let hint = if options { OPTIONS_HINT_W } else { 0.0 };
    let r = tooltip_frame(p, at, galley.size() + vec2(12.0 + hint, 8.0));
    p.galley(r.min + vec2(6.0, 4.0), galley, t.text);
    if options {
        options_hint(p, r);
    }
    options_menu(app, p, r, hover);
}

/// The drawing's length units: (LUNITS, LUPREC).
pub(crate) fn length_units(session: &cadcraft_engine::Session) -> (i64, i64) {
    session.doc().map(|d| (d.header.i64("LUNITS", 2), d.header.i64("LUPREC", 4))).unwrap_or((2, 4))
}

/// A distance in the drawing's length units.
fn distance_text(session: &cadcraft_engine::Session, v: f64) -> String {
    let (lu, lp) = length_units(session);
    cadcraft_engine::units::format_distance(v, lu, lp)
}

/// Room for the down-arrow sign that a prompt has options.
const OPTIONS_HINT_W: f32 = 14.0;

/// The down-arrow sign at the right end of the tooltip `r`: the prompt has options, and Down
/// Arrow lists them.
fn options_hint(p: &egui::Painter, r: Rect) {
    let t = Tokens::get();
    let c = pos2(r.right() - OPTIONS_HINT_W / 2.0 - 2.0, r.center().y + 1.0);
    p.add(Shape::convex_polygon(vec![pos2(c.x - 4.0, c.y - 2.5), pos2(c.x + 4.0, c.y - 2.5), pos2(c.x, c.y + 2.5)], t.text_dim, Stroke::NONE));
}

/// The options menu under the tooltip `r` (DYNPROMPT): one row per keyword, the highlighted one
/// marked; the rows are kept for clicks.
fn options_menu(app: &mut CadApp, p: &egui::Painter, r: Rect, hover: Pos2) {
    let Some(mut menu) = app.cmd.options.clone() else { return };
    let t = Tokens::get();
    let font = crate::theme::small();
    let rows: Vec<_> = menu.keywords.iter().map(|k| p.layout_no_wrap(k.clone(), font.clone(), t.text)).collect();
    let row_h = rows.iter().map(|g| g.size().y).fold(12.0, f32::max) + 6.0;
    let w = rows.iter().map(|g| g.size().x).fold(60.0, f32::max) + 28.0;
    let h = row_h * rows.len() as f32 + 4.0;
    let m = Rect::from_min_size(pos2(r.left(), r.bottom() + 2.0), vec2(w, h));
    p.rect_filled(m, 3.0, t.cmd_bg);
    p.rect_stroke(m, 3.0, Stroke::new(1.0, t.cmd_border), egui::StrokeKind::Inside);
    let row_at = |i: usize| Rect::from_min_size(pos2(m.left() + 2.0, m.top() + 2.0 + row_h * i as f32), vec2(w - 4.0, row_h));
    // The row under the pointer is the highlighted one.
    if let Some(i) = (0..menu.keywords.len()).find(|i| row_at(*i).contains(hover)) {
        menu.index = i;
        app.cmd.options = Some(menu.clone());
    }
    for (i, (g, k)) in rows.into_iter().zip(&menu.keywords).enumerate() {
        let row = row_at(i);
        if i == menu.index {
            p.rect_filled(row, 2.0, t.cmd_keyword_hover);
            p.circle_filled(pos2(row.left() + 9.0, row.center().y), 2.5, t.accent);
        }
        p.galley(pos2(row.left() + 20.0, row.center().y - g.size().y / 2.0), g, t.text);
        app.cmd.option_rows.push((row, k.clone()));
    }
}

/// The tooltip background, matching the command line bar; returns its rectangle.
fn tooltip_frame(p: &egui::Painter, at: Pos2, size: egui::Vec2) -> Rect {
    let t = Tokens::get();
    let r = Rect::from_min_size(at + vec2(18.0, -size.y - 14.0), size);
    p.rect_filled(r, 3.0, t.cmd_bg);
    p.rect_stroke(r, 3.0, Stroke::new(1.0, t.cmd_border), egui::StrokeKind::Inside);
    r
}

/// The Dynamic Input value boxes: X and Y, or distance and angle. Typed values show in full
/// text colour, a locked value (a separator typed after it) gets a lock, the active box gets the
/// accent outline, and open boxes show the cursor's live value, dimmed. Values measured from the
/// last point are marked with `@`.
#[allow(clippy::too_many_arguments)]
fn dynamic_input_boxes(
    p: &egui::Painter,
    at: Pos2,
    message: &str,
    f: &crate::dyninput::Frame,
    e: &crate::dyninput::Entry<'_>,
    cursor: Vec2,
    second_locked: bool,
    options: bool,
    (lu, lp): (i64, i64),
) -> Rect {
    use crate::dyninput::Format;
    let t = Tokens::get();
    let font = crate::theme::small();
    let dist = |v: f64| cadcraft_engine::units::format_distance(v, lu, lp);
    let (live_first, live_second) = f.live(e.format, cursor);
    let live_second = match e.format {
        Format::Cartesian => dist(live_second),
        Format::Polar => format!("{}°", (live_second.to_degrees() * 10.0).round() / 10.0),
    };
    let (label_first, label_second) = match e.format {
        Format::Cartesian => ("X", "Y"),
        Format::Polar => ("", "<"),
    };
    // (label, text, typed, locked, active)
    let cells = [
        (
            label_first,
            if e.first.is_empty() { dist(live_first) } else { e.first.to_owned() },
            !e.first.is_empty(),
            e.first_locked().is_some(),
            !e.split,
        ),
        (
            label_second,
            if e.second.is_empty() { live_second } else { e.second.to_owned() },
            !e.second.is_empty(),
            second_locked,
            e.split && !second_locked,
        ),
    ];

    let msg = p.layout_no_wrap(message.to_owned(), font.clone(), t.text_dim);
    let marker = p.layout_no_wrap(if f.relative() { "@".to_owned() } else { String::new() }, font.clone(), t.text_faint);
    let lock_w = 10.0;
    let laid: Vec<_> = cells
        .into_iter()
        .map(|(label, text, typed, locked, active)| {
            let label = p.layout_no_wrap(label.to_owned(), font.clone(), t.text_faint);
            let value = p.layout_no_wrap(text, font.clone(), if typed { t.text } else { t.text_faint });
            let w = (value.size().x + 10.0 + if locked { lock_w } else { 0.0 }).max(48.0);
            (label, value, w, locked, active)
        })
        .collect();

    // Measure, then draw.
    let pad = 6.0;
    let gap = 6.0;
    let box_h = msg.size().y + 6.0;
    let mut w = pad + msg.size().x + gap + marker.size().x;
    for (label, _, bw, _, _) in &laid {
        w += gap + label.size().x + 3.0 + bw;
    }
    let hint = if options { OPTIONS_HINT_W } else { 0.0 };
    let r = tooltip_frame(p, at, vec2(w + pad + hint, box_h + 2.0 * pad - 2.0));
    if options {
        options_hint(p, r);
    }
    let cy = r.center().y;
    let mut x = r.left() + pad;
    let (msg_w, msg_h) = (msg.size().x, msg.size().y);
    p.galley(pos2(x, cy - msg_h / 2.0), msg, t.text_dim);
    x += msg_w + gap;
    let mw = marker.size().x;
    p.galley(pos2(x, cy - marker.size().y / 2.0), marker, t.text_faint);
    x += mw;
    for (label, value, bw, locked, active) in laid {
        x += gap;
        let lw = label.size().x;
        p.galley(pos2(x, cy - label.size().y / 2.0), label, t.text_faint);
        x += lw + 3.0;
        let b = Rect::from_min_size(pos2(x, cy - box_h / 2.0), vec2(bw, box_h));
        p.rect_filled(b, 2.0, t.chrome_dark);
        let stroke = if active { Stroke::new(1.5, t.accent) } else { Stroke::new(1.0, t.border) };
        p.rect_stroke(b, 2.0, stroke, egui::StrokeKind::Inside);
        let vw = value.size().x;
        p.galley(pos2(b.left() + 5.0, cy - value.size().y / 2.0), value, t.text);
        if active {
            let cx = b.left() + 6.0 + vw;
            p.line_segment([pos2(cx, b.top() + 3.0), pos2(cx, b.bottom() - 3.0)], Stroke::new(1.0, t.text));
        }
        if locked {
            draw_lock(p, pos2(b.right() - lock_w / 2.0 - 3.0, cy), t.accent);
        }
        x += bw;
    }
    r
}

/// A small padlock, drawn in code: body plus shackle.
fn draw_lock(p: &egui::Painter, c: Pos2, color: Color32) {
    p.rect_filled(Rect::from_center_size(c + vec2(0.0, 1.5), vec2(7.0, 5.0)), 1.0, color);
    p.add(Shape::line(
        vec![pos2(c.x - 2.0, c.y - 1.0), pos2(c.x - 2.0, c.y - 4.0), pos2(c.x + 2.0, c.y - 4.0), pos2(c.x + 2.0, c.y - 1.0)],
        Stroke::new(1.2, color),
    ));
}

fn draw_ucs_icon(p: &egui::Painter, rect: Rect) {
    let t = Tokens::get();
    let o = pos2(rect.left() + 36.0, rect.bottom() - 34.0);
    let st = Stroke::new(1.0, t.canvas_ink);
    p.line_segment([o, o + vec2(60.0, 0.0)], st);
    p.line_segment([o, o + vec2(0.0, -60.0)], st);
    p.rect_stroke(Rect::from_center_size(o, vec2(9.0, 9.0)), 0.0, st, egui::StrokeKind::Middle);
    let f = egui::FontId::proportional(13.0);
    p.text(o + vec2(70.0, 0.0), egui::Align2::LEFT_CENTER, crate::tl!("X"), f.clone(), t.canvas_ink);
    p.text(o + vec2(0.0, -70.0), egui::Align2::CENTER_BOTTOM, crate::tl!("Y"), f, t.canvas_ink);
}

fn viewport_label(p: &egui::Painter, rect: Rect) {
    let t = Tokens::get();
    let at = pos2(rect.left() + 10.0, rect.top() + 8.0);
    p.text(at, egui::Align2::LEFT_TOP, crate::tl!("+  |  Top  |  2D Wireframe"), crate::theme::small(), t.canvas_ink);
}

fn draw_viewcube(app: &mut CadApp, ui: &mut egui::Ui, rect: Rect) {
    let l = crate::viewcube::Layout::new(rect);
    let (c, ring) = (l.center, l.ring);
    let p = ui.painter_at(rect);
    crate::viewcube::show(app, ui, &l);
    // WCS pill: the label and a down chevron, drawn as lines rather than a "⌄" glyph that a UI
    // font may lack (it showed as a box, #1541).
    let pill = Rect::from_center_size(c + vec2(0.0, ring + 26.0), vec2(56.0, 16.0));
    p.rect_filled(pill, 8.0, Color32::from_rgb(0x48, 0x50, 0x5c));
    let ink = Tokens::DARK.canvas_ink;
    let label = p.layout_no_wrap(crate::tl!("WCS").to_string(), crate::theme::small(), ink);
    let (gap, chevron) = (4.0, 6.0);
    let left = pill.center().x - (label.size().x + gap + chevron) / 2.0;
    let mid = pill.center().y;
    p.galley(pos2(left, mid - label.size().y / 2.0), label.clone(), ink);
    let x = left + label.size().x + gap;
    p.line(vec![pos2(x, mid - 1.5), pos2(x + chevron / 2.0, mid + 1.5), pos2(x + chevron, mid - 1.5)], Stroke::new(1.2, ink));
}

/// A click on grip `g` (idle): Shift+click makes it hot or cold again; a plain click starts
/// editing it, with the other hot grips when it is one of them.
fn click_grip(app: &mut CadApp, g: (Handle, usize), shift: bool) {
    let hot = &mut app.canvas.hot_grips;
    if shift {
        match hot.iter().position(|x| *x == g) {
            Some(i) => {
                hot.remove(i);
            }
            None => hot.push(g),
        }
        return;
    }
    if !hot.contains(&g) {
        hot.clear();
    }
    hot.retain(|x| *x != g);
    hot.insert(0, g);
    let hs = hot.clone();
    if let Err(e) = app.session.start_grip_edit(&hs) {
        app.canvas.hot_grips.clear();
        app.session.echo(e.to_string());
    }
}

/// The multifunctional options of grip `g` (Add Vertex, Lengthen…; empty: Stretch only).
fn grip_options(app: &CadApp, (h, i): (Handle, usize)) -> Vec<cadcraft_engine::grips::GripAction> {
    let Ok(st) = app.session.state() else { return Vec::new() };
    st.doc.entity(h).map(|e| cadcraft_engine::grips::grip_actions(&e.kind, i)).unwrap_or_default()
}

/// The menu of a multifunctional grip: Stretch and its options. A choice makes the grip hot and
/// runs the option at the grip prompt, as typing its keyword would.
fn grip_menu(app: &mut CadApp, resp: &egui::Response) {
    let Some(g) = app.canvas.grip_menu else { return };
    let options = grip_options(app, g);
    let mut chosen: Option<Option<cadcraft_engine::grips::GripAction>> = None;
    let open = resp
        .context_menu(|ui| {
            let vertex = options.contains(&cadcraft_engine::grips::GripAction::RemoveVertex);
            if ui.button(crate::i18n::t(if vertex { "Stretch Vertex" } else { "Stretch" })).clicked() {
                chosen = Some(None);
                ui.close();
            }
            for a in &options {
                if ui.button(crate::i18n::t(a.label())).clicked() {
                    chosen = Some(Some(*a));
                    ui.close();
                }
            }
        })
        .is_some();
    if !open || chosen.is_some() {
        app.canvas.grip_menu = None;
    }
    let Some(choice) = chosen else { return };
    app.canvas.hot_grips = vec![g];
    let r = app.session.start_grip_edit(&[g]).and_then(|()| match choice {
        Some(a) => app.session.input(Input::Keyword(a.keyword().to_string())),
        None => Ok(()),
    });
    if let Err(e) = r {
        app.canvas.hot_grips.clear();
        app.session.echo(e.to_string());
    }
}

/// Polyline segment midpoint grips are rectangular; vertex grips remain square.
fn grip_rect(kind: &cadcraft_doc::EntityKind, count: usize, index: usize, center: Pos2, size: f32) -> Rect {
    // The grip list interleaves vertices and available segment midpoints, then any remaining vertices.
    let midpoint = matches!(kind, cadcraft_doc::EntityKind::LwPolyline(p) if index % 2 == 1 && index / 2 < count.saturating_sub(p.vertices.len()));
    Rect::from_center_size(center, vec2(size * 2.0, if midpoint { size } else { size * 2.0 }))
}

/// The grip of a selected object under the cursor, if any: (object, grip index).
fn grip_at(app: &CadApp, xf: &Xf, hover: Option<Pos2>) -> Option<(Handle, usize)> {
    let hp = hover?;
    let st = app.session.state().ok()?;
    let gs = app.session.settings.gripsize as f32;
    let sel = app.session.selection();
    if !app.session.shows_grips(sel.len()) {
        return None;
    }
    for h in sel.iter().take(200) {
        let e = st.doc.entity(*h)?;
        if st.doc.layer(&e.common.layer).is_some_and(|l| l.locked) {
            continue;
        }
        let grips = e.kind.grips();
        let count = grips.len();
        for (i, g) in grips.into_iter().enumerate() {
            let sp = xf.to_screen(g);
            if grip_rect(&e.kind, count, i, sp, gs).expand(3.0).contains(hp) {
                return Some((*h, i));
            }
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod tests {
    use super::*;
    use cadcraft_render::DPrim;

    fn list(handles: &[u64]) -> DisplayList {
        let prim =
            |h| DPrim { handle: Handle(h), color: Rgb(255, 255, 255), aci7: true, lw: 0.0, transparency: 0, kind: Kind::Polyline, start: 0, len: 0 };
        DisplayList { prims: handles.iter().map(|&h| prim(h)).collect(), ..Default::default() }
    }

    fn hs(v: &[u64]) -> Vec<Handle> {
        v.iter().map(|&h| Handle(h)).collect()
    }

    /// A dragged object keeps its transparency in the GPU preview when TRANSPARENCYDISPLAY is
    /// on, and is opaque when it is off.
    #[test]
    fn dragged_objects_keep_their_transparency() {
        let mut d = cadcraft_doc::Drawing::new_metric();
        let common = cadcraft_doc::Common { transparency: cadcraft_doc::Transparency::Percent(50), ..Default::default() };
        let line = cadcraft_doc::Line { a: cadcraft_geom::Vec3::ZERO, b: cadcraft_geom::Vec3::new(10.0, 0.0, 0.0) };
        let h = d.add(&cadcraft_doc::Space::Model, common, cadcraft_doc::EntityKind::Line(line)).unwrap();
        let drag = cadcraft_engine::Drag { handles: vec![h], matrix: cadcraft_geom::Mat3::IDENTITY, extra: Vec::new(), clipboard: false };
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(200.0, 100.0));
        let xf = Xf { rect, center: Vec2::new(5.0, 0.0), scale: 10.0 };
        let painter = egui::Painter::new(egui::Context::default(), egui::LayerId::background(), rect);
        // One canvas across the toggle: the cached drag mesh must be rebuilt.
        let mut c = CanvasState::default();
        let mut alpha = |transparency: bool| {
            draw_drag_gpu(&mut c, &painter, &xf, &d, &drag, (&[], 0), Rgb(0, 0, 0), transparency);
            let mesh = c.drag_slot.lock().unwrap().take().unwrap();
            // The first line vertex's colour alpha (after its 2 × f32 position).
            mesh.lines[11]
        };
        assert_eq!(alpha(true), 128);
        assert_eq!(alpha(false), 255);
        assert_eq!(alpha(true), 128);
    }

    /// Previews the session rebuilds with the current properties (the rubber band, copies of
    /// objects) stay opaque even when CLAYER is transparent: they were opaque before transparency
    /// was drawn, and CLAYER's transparency isn't the objects'.
    #[test]
    fn rebuilt_previews_stay_opaque_on_a_transparent_current_layer() {
        let mut s = cadcraft_engine::Session::new();
        let d = s.doc_mut().unwrap();
        d.layers.push(cadcraft_doc::Layer { name: "Glass".into(), transparency: 40, ..Default::default() });
        d.header.set_str("CLAYER", "Glass");
        assert!(s.settings.transparency_display);
        // LINE: the segment from the first point to the cursor.
        s.cmdline("line").unwrap();
        s.cmdline("0,0").unwrap();
        let ents = s.preview(Vec2::new(10.0, 5.0));
        assert!(!ents.is_empty());
        assert_eq!(ents[0].common.layer, "Glass");
        let doc = s.doc().unwrap();
        // Built with transparency, the preview would take CLAYER's 40 %.
        let with = cadcraft_render::build_entities(doc, ents.iter(), &cadcraft_render::Options { transparency: true, ..preview_options(1.0) });
        assert!(with.prims.iter().all(|p| p.transparency == 40));
        let list = rebuilt_preview(doc, ents.iter(), 1.0);
        assert!(!list.prims.is_empty() && list.prims.iter().all(|p| p.transparency == 0));
        s.cancel();
        // MOVE's base-to-cursor rubber band (a drag extra).
        s.execute("line", &serde_json::json!({"points": [[0, 0], [10, 0]]})).unwrap();
        s.cmdline("move").unwrap();
        s.cmdline("all").unwrap();
        s.cmdline("").unwrap();
        s.cmdline("0,0").unwrap();
        let (_, extra) = s.drag(Vec2::new(4.0, 4.0)).unwrap();
        assert!(!extra.is_empty());
        let list = rebuilt_preview(s.doc().unwrap(), extra.iter(), 1.0);
        assert!(!list.prims.is_empty() && list.prims.iter().all(|p| p.transparency == 0));
    }

    #[test]
    fn viewport_highlight_tolerance_is_constant_within_a_zoom_band() {
        assert_eq!(viewport_highlight_tolerance(4.0), viewport_highlight_tolerance(3.0));
        assert_ne!(viewport_highlight_tolerance(4.0), viewport_highlight_tolerance(1.5));
        for s in [0.3, 1.0, 3.0, 4.0, 250.0] {
            assert!(viewport_highlight_tolerance(s) <= 0.5 / s);
        }
    }

    #[test]
    fn selection_area_fill_follows_the_area_effect_settings() {
        let mut s = cadcraft_engine::Settings::default();
        // Window: ACI 150 at 25 %; crossing: ACI 100.
        let rgb = |c: Rgb| Color32::from_rgb(c.0, c.1, c.2);
        assert_eq!(selection_area_fill(&s, false), Some(rgb(cadcraft_color::aci_rgb(150)).gamma_multiply(0.25)));
        assert_eq!(selection_area_fill(&s, true), Some(rgb(cadcraft_color::aci_rgb(100)).gamma_multiply(0.25)));
        s.selectionareaopacity = 100;
        assert_eq!(selection_area_fill(&s, false), Some(rgb(cadcraft_color::aci_rgb(150))));
        s.selectionarea = false;
        assert_eq!(selection_area_fill(&s, false), None, "Indicate selection area off: no fill");
    }

    #[test]
    fn viewport_highlight_is_tessellated_once_per_selection_and_zoom_band() {
        use cadcraft_doc::{Circle, Common, Drawing, EntityKind, Space};
        let mut d = Drawing::new_imperial();
        let circle = |r| EntityKind::Circle(Circle { center: cadcraft_geom::Vec3::new(0.0, 0.0, 0.0), radius: r });
        let a = d.add(&Space::Model, Common::default(), circle(10.0)).unwrap();
        let b = d.add(&Space::Model, Common::default(), circle(5.0)).unwrap();
        let st = cadcraft_engine::DocState::new(d, "highlight", None);
        let points = |l: &DisplayList| l.prims.iter().map(|p| l.points(p).len()).sum::<usize>();
        let (mut sel, mut hover) = (VpHighlight::default(), VpHighlight::default());
        sel.update(&st, &[a], 10.0);
        let (first, segments) = (sel.list_gen, points(&sel.list));
        assert!(!sel.list.prims.is_empty());
        // Every frame of a pan, zooming within the 2x band, and the hover moving: the same list.
        sel.update(&st, &[a], 10.0);
        sel.update(&st, &[a], 12.0);
        hover.update(&st, &[b], 12.0);
        hover.update(&st, &[a], 12.0);
        assert_eq!(sel.list_gen, first);
        // Zooming in past the band tessellates finer.
        sel.update(&st, &[a], 40.0);
        assert_ne!(sel.list_gen, first);
        assert!(points(&sel.list) > segments);
        // A different selection is rebuilt.
        let zoomed = sel.list_gen;
        sel.update(&st, &[a, b], 40.0);
        assert_ne!(sel.list_gen, zoomed);
        assert_eq!(sel.list.prims.iter().map(|p| p.handle).collect::<std::collections::BTreeSet<_>>().len(), 2);
    }

    #[test]
    fn prim_index_matches_a_scan_and_follows_list_rebuilds() {
        let a = list(&[3, 1, 3, 2, 7, 3]);
        let mut ix = PrimIndex::default();
        assert_eq!(ix.prims(1, &a, &hs(&[3])), vec![0, 2, 5]);
        assert_eq!(ix.prims(1, &a, &hs(&[3])), prims_of(&a, &hs(&[3])));
        assert_eq!(ix.prims(1, &a, &hs(&[2, 1, 2])), vec![1, 3]);
        assert!(ix.prims(1, &a, &hs(&[9])).is_empty());
        assert!(ix.prims(1, &a, &[]).is_empty());
        // A rebuilt list (new generation) is re-indexed.
        let b = list(&[3, 9]);
        assert_eq!(ix.prims(2, &b, &hs(&[9, 3])), vec![0, 1]);
    }

    fn wheel(unit: egui::MouseWheelUnit, dy: f32, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::MouseWheel { unit, delta: vec2(0.0, dy), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Unknown, modifiers }
    }

    /// Wheel events convert to points as egui does; ⌘/Ctrl (egui's pinch zoom) and ⇧ (a sideways
    /// scroll) don't zoom here, and every notch of the frame counts (#660).
    #[test]
    fn wheel_zoom_reads_every_notch_of_the_frame() {
        use egui::{Modifiers, MouseWheelUnit as U};
        let o = egui::InputOptions::default();
        let none = Modifiers::NONE;
        assert_eq!(wheel_zoom_points(&[wheel(U::Line, 1.0, none)], &o, 500.0), o.line_scroll_speed);
        assert_eq!(wheel_zoom_points(&[wheel(U::Line, -2.0, none)], &o, 500.0), -2.0 * o.line_scroll_speed);
        assert_eq!(wheel_zoom_points(&[wheel(U::Point, 7.5, none)], &o, 500.0), 7.5);
        assert_eq!(wheel_zoom_points(&[wheel(U::Page, 1.0, none)], &o, 500.0), 500.0);
        // Three quick notches in one frame zoom three steps at once.
        let three = [wheel(U::Line, 1.0, none), wheel(U::Line, 1.0, none), wheel(U::Line, 1.0, none)];
        assert_eq!(wheel_zoom_points(&three, &o, 500.0), 3.0 * o.line_scroll_speed);
        assert_eq!(wheel_zoom_points(&[wheel(U::Line, 1.0, Modifiers::COMMAND)], &o, 500.0), 0.0);
        assert_eq!(wheel_zoom_points(&[wheel(U::Line, 1.0, Modifiers::SHIFT)], &o, 500.0), 0.0);
        assert_eq!(wheel_zoom_points(&[wheel(U::Line, f32::NAN, none)], &o, 500.0), 0.0);
        assert_eq!(wheel_zoom_points(&[egui::Event::PointerGone], &o, 500.0), 0.0);
    }

    /// The frame a notch arrives, egui's smoothed scroll holds only part of it (the rest trickles
    /// in over later frames, which made quick notches lag), while the raw reading zooms the
    /// whole notch at once (#660).
    #[test]
    fn a_wheel_notch_zooms_whole_in_its_own_frame() {
        let ctx = egui::Context::default();
        let o = ctx.options(|o| o.input_options);
        let input = egui::RawInput { events: vec![wheel(egui::MouseWheelUnit::Line, 1.0, egui::Modifiers::NONE)], ..Default::default() };
        let mut seen = (0.0, 0.0);
        ctx.run_ui(input, |ui| {
            seen = ui.input(|i| (i.smooth_scroll_delta.y, wheel_zoom_points(&i.raw.events, &o, 500.0)));
        })
        .textures_delta
        .clear();
        let (smoothed, raw) = seen;
        assert_eq!(raw, o.line_scroll_speed, "the whole notch");
        assert!(smoothed < raw, "egui's smoothed delta lags behind: {smoothed} of {raw}");
    }

    /// The text painted by one frame of the app at `pointer`.
    pub(crate) fn painted_text(app: &mut CadApp, ctx: &egui::Context, pointer: Pos2) -> String {
        fn collect(shape: &Shape, text: &mut String) {
            match shape {
                Shape::Text(s) => {
                    text.push_str(s.galley.text());
                    text.push('\n');
                }
                Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, text)),
                _ => {}
            }
        }
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
            events: vec![egui::Event::PointerMoved(pointer)],
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
        let mut text = String::new();
        out.shapes.iter().for_each(|s| collect(&s.shape, &mut text));
        text
    }

    /// The Dynamic Input distance box shows the cursor's live distance in the drawing's length
    /// units (LUNITS/LUPREC), as DIST and the status bar do (#841).
    #[test]
    fn dynamic_input_distance_follows_the_drawing_units() {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("units", &serde_json::json!({"lunits": 4, "luprec": 4})).unwrap();
        let ctx = egui::Context::default();
        painted_text(&mut app, &ctx, pos2(700.0, 450.0));
        app.session.cmdline("line").unwrap();
        app.session.input(Input::Point(Vec2::ZERO)).unwrap();
        let at = app.canvas.xf.unwrap().to_screen(Vec2::ZERO) + vec2(130.0, -70.0);
        painted_text(&mut app, &ctx, at);
        let text = painted_text(&mut app, &ctx, at);
        let c = app.canvas.cursor.unwrap();
        let arch = |v: f64| cadcraft_engine::units::format_distance(v, 4, 4);
        // The status bar shows X and Y in the same units: the distance must be neither.
        assert!(arch(c.len()) != arch(c.x) && arch(c.len()) != arch(c.y), "{c:?}");
        assert!(text.contains(&arch(c.len())), "no {} in:\n{text}", arch(c.len()));
        // Without the value boxes (a window corner pending) the tooltip shows the distance too.
        app.session.pending_window = Some(cadcraft_engine::PendingWindow { corner: Vec2::ZERO, during_command: true });
        let text = painted_text(&mut app, &ctx, at);
        assert!(text.contains(&format!("   {}  <", arch(c.len()))), "no {} in:\n{text}", arch(c.len()));
    }
}

/// The Block Editor's tint, base point and bar.
mod blockedit;
/// The COMPARE overlay.
mod compare;

/// Background rebuilds of the display list for a new view.
mod rebuild;

#[cfg(test)]
mod hatch_pick_tests {
    use cadcraft_engine::Session;
    use cadcraft_geom::Vec2;
    use serde_json::json;

    use crate::{CadApp, Services};

    #[test]
    fn hatch_at_finds_a_hatch_under_the_cursor_only() {
        let mut app = CadApp::new(Session::new(), Services::default());
        app.session.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        let hx = app.session.execute("hatch", &json!({ "points": [[5, 5]], "pattern": "SOLID" })).unwrap()["handle"].as_str().unwrap().to_string();
        app.session.execute("line", &json!({ "points": [[20, 0], [30, 0]] })).unwrap();
        assert_eq!(super::hatch_at(&app, Vec2::new(5.0, 5.0), 10.0).map(|h| h.hex()), Some(hx));
        assert_eq!(super::hatch_at(&app, Vec2::new(25.0, 0.0), 10.0), None, "a line is not a hatch");
        assert_eq!(super::hatch_at(&app, Vec2::new(60.0, 60.0), 10.0), None);
    }

    #[test]
    fn double_clicking_a_hatch_opens_the_hatch_edit_dialog() {
        let mut app = CadApp::new(Session::new(), Services::default());
        app.session.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        let hx = app.session.execute("hatch", &json!({ "points": [[5, 5]], "pattern": "SOLID" })).unwrap()["handle"].as_str().unwrap().to_string();
        let _ = app.session.zoom_extents();
        let ctx = egui::Context::default();
        let screen = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0)));
        let frame = |app: &mut CadApp, events: Vec<egui::Event>, time: f64| {
            app.logic(&ctx);
            let input = egui::RawInput { screen_rect: screen, events, time: Some(time), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| app.ui(ui));
            out.textures_delta.clear();
        };
        frame(&mut app, Vec::new(), 0.0);
        let at = app.canvas.xf.unwrap().to_screen(Vec2::new(5.0, 5.0));
        let button =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() };
        frame(&mut app, vec![egui::Event::PointerMoved(at)], 0.1);
        frame(&mut app, vec![button(true), button(false)], 0.2);
        frame(&mut app, vec![button(true), button(false)], 0.3);
        frame(&mut app, Vec::new(), 0.4);
        assert_eq!(app.ui.dialog.as_deref(), Some("hatch"));
        assert_eq!(app.hatch_dlg.as_ref().map(|s| s.edit.iter().map(|h| h.hex()).collect::<Vec<_>>()), Some(vec![hx]));
        // The double-click (at the hatch's centre grip) only opens the dialog: no grip edit.
        assert!(app.session.running.is_none(), "{:?}", app.session.running.as_ref().map(|r| r.machine.name()));
    }
}

#[cfg(test)]
mod grid_range_tests {
    use cadcraft_geom::{Bounds2, Vec2};

    #[test]
    fn grid_index_ranges_survive_extreme_views() {
        let huge = Bounds2::new(Vec2::new(-1e300, -1e300), Vec2::new(1e300, 1e300));
        assert_eq!(super::grid_index_ranges(&huge, 1.0), None);
        let normal = Bounds2::new(Vec2::new(-5.0, 0.0), Vec2::new(5.0, 10.0));
        assert_eq!(super::grid_index_ranges(&normal, 1.0), Some(((-5, 5), (0, 10))));
    }
}
