//! The drawing area: grid, entities, selection highlighting, grips, rubber bands, object-snap
//! markers, crosshair cursor, ViewCube, UCS icon and mouse handling.

use cadcraft_color::Rgb;
use cadcraft_doc::Handle;
use cadcraft_engine::Input;
use cadcraft_engine::SelectionOverlayUv;
use cadcraft_engine::snap::{self, SnapHit};
use cadcraft_geom::{Bounds2, Mat3, Vec2};
use cadcraft_render::{DisplayList, Kind};
use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};

use crate::CadApp;
use crate::theme::Tokens;

mod cursor;
mod cycle;
mod draw;
mod dyn_boxes;
mod grid;
mod grips;
mod list;
mod pick;
mod viewport;
mod widgets;

use cursor::{draw_crosshair, draw_snap_marker, draw_tracking, effective_point, override_cursor};
use cycle::{CycleList, cycle_badge, cycle_click, cycle_list, over_canvas_menu, shift_space_cycle};
pub(crate) use draw::{Look, draw_highlight, draw_list};
use draw::{
    HlCache, dashed_polygon, dashed_polyline, dashed_rect, draw_drag_gpu, draw_highlight_layer, draw_list_gpu, list_mesh_key,
    selection_area_fill,
};
use dyn_boxes::dynamic_input;
pub(crate) use dyn_boxes::length_units;
use grid::draw_grid;
use grips::{click_grip, grip_at, grip_menu, grip_options, grip_rect};
use list::{OnlyList, PrimIndex, ensure_list, rebuilt_preview};
#[cfg(test)]
use list::{preview_options, prims_of};
use pick::{edit_text_at, hatch_at, wheel_zoom_points};
#[cfg(test)]
use viewport::viewport_highlight_tolerance;
use viewport::{VpContent, VpHighlight, draw_vp_content, ensure_vp_content};
use widgets::{draw_tiles, draw_ucs_icon, draw_viewcube, tile_rects, tile_xf, viewport_label};

/// World ↔ screen transform for the canvas rect.
#[derive(Clone, Copy, Debug)]
pub struct Xf {
    pub rect: Rect,
    pub center: Vec2,
    /// Pixels (points) per drawing unit.
    pub scale: f64,
    /// The view twist (radians counterclockwise, `cadcraft_engine::View::twist`): the drawing
    /// appears turned by it about `center`. 0 for paper space.
    pub twist: f64,
    /// Elevation Z of the camera target (`cadcraft_engine::View::target_z`).
    pub target_z: f64,
    /// Active 3D camera orientation for model space (ViewCube, 3D Orbit, CADCross).
    pub cam: cadcraft_geom::Camera3D,
}

impl Xf {
    /// Full 3D camera target `(center.x, center.y, target_z)`.
    pub fn target(&self) -> cadcraft_geom::Vec3 {
        cadcraft_geom::Vec3::new(self.center.x, self.center.y, self.target_z)
    }
    /// Screen-frame XY shift caused by non-zero `target_z` when projecting `Z = 0` geometry.
    fn z_shift(&self) -> Vec2 {
        if !self.cam.is_top_plan() && self.target_z != 0.0 {
            Vec2::new(-self.cam.x.z * self.target_z, -self.cam.y.z * self.target_z)
        } else {
            Vec2::ZERO
        }
    }
    /// Convert screen coordinates `p` to camera-plane UV offset (world units relative to `target()`).
    pub fn screen_uv_at(&self, p: Pos2) -> Vec2 {
        let c = self.rect.center();
        Vec2::new(f64::from(p.x - c.x) / self.scale, f64::from(c.y - p.y) / self.scale)
    }
    /// Convert a camera-plane UV offset (world units relative to `target()`) to screen coordinates.
    pub fn screen_uv_to_screen(&self, uv: Vec2) -> Pos2 {
        let c = self.rect.center();
        pos2(c.x + (uv.x * self.scale) as f32, c.y - (uv.y * self.scale) as f32)
    }
    /// Camera/view projection matching `self` for engine picking and selection.
    pub fn pick_projection(&self) -> cadcraft_engine::select::PickProjection {
        cadcraft_engine::select::PickProjection::from_view(self.cam, self.center, self.target_z, self.twist)
    }
    /// A world offset from `center` in the screen's frame (y up): turned by the 3D camera or view twist.
    fn turn(&self, v: Vec2) -> Vec2 {
        if !self.cam.is_top_plan() {
            return self.cam.project_xy(v, Vec2::ZERO);
        }
        if self.twist == 0.0 { v } else { v.rotate(self.twist) }
    }
    /// Map a 3D world point `p` to screen coordinates using `target()`.
    pub fn to_screen_3d(&self, p: cadcraft_geom::Vec3) -> Pos2 {
        let c = self.rect.center();
        let q = if !self.cam.is_top_plan() {
            self.cam.project_3d(p, self.target())
        } else {
            self.turn(p.xy() - self.center)
        };
        pos2(c.x + (q.x * self.scale) as f32, c.y - (q.y * self.scale) as f32)
    }
    /// Map a 2D world point `p` (on `Z = 0`) to screen coordinates.
    pub fn to_screen(&self, p: Vec2) -> Pos2 {
        let c = self.rect.center();
        let q = self.turn(p - self.center) + self.z_shift();
        pos2(c.x + (q.x * self.scale) as f32, c.y - (q.y * self.scale) as f32)
    }
    /// Intersect screen position `p` with the world XY plane (`Z = 0`), returning `None` in
    /// edge-on 3D views (`|cam.z.z| <= 1e-2`) where rays are parallel to the XY plane.
    pub fn try_to_world(&self, p: Pos2) -> Option<Vec2> {
        let c = self.rect.center();
        if !self.cam.is_top_plan() {
            let uv = self.screen_uv_at(p);
            let (ray_o, ray_d) = self.cam.ray_at_target(uv, self.target());
            return cadcraft_geom::intersect_ray_plane(
                ray_o,
                ray_d,
                cadcraft_geom::Vec3::ZERO,
                cadcraft_geom::Vec3::Z,
            )
            .map(cadcraft_geom::Vec3::xy);
        }
        Some(self.center + self.screen_delta_to_world(p - c))
    }
    /// Map a screen position `p` back to world XY (`Z = 0`), falling back in edge-on views to
    /// isometric horizontal/depth mapping without discarding the vertical axis.
    pub fn to_world(&self, p: Pos2) -> Vec2 {
        if !self.cam.is_top_plan() {
            if let Some(hit) = self.try_to_world(p) {
                return hit;
            }
            return self.cam.unproject_to_xy(self.screen_uv_at(p), self.center);
        }
        self.center + self.screen_delta_to_world(p - self.rect.center())
    }
    /// 3D anchor point in the orthographic target plane `(target(), cam.z)` under screen position
    /// `p`, keeping interactive scroll/pinch zooms pixel-locked even when `target_z != 0` or in
    /// edge-on orthogonal views (`Front`, `Left`, etc.).
    pub fn zoom_anchor_3d(&self, p: Pos2) -> cadcraft_geom::Vec3 {
        if !self.cam.is_top_plan() {
            return self.cam.ray_at_target(self.screen_uv_at(p), self.target()).0;
        }
        self.to_world(p).to3(self.target_z)
    }
    /// The world XY offset (on `Z = 0`) covered by screen offset `d` (points, y down).
    pub fn screen_delta_to_world(&self, d: egui::Vec2) -> Vec2 {
        if !self.cam.is_top_plan() {
            let c = self.rect.center();
            if let (Some(world), Some(center_world)) = (self.try_to_world(c + d), self.try_to_world(c)) {
                return world - center_world;
            }
            let du = f64::from(d.x) / self.scale;
            let dv = -f64::from(d.y) / self.scale;
            return (self.cam.x * du + self.cam.y * dv).xy();
        }
        let q = Vec2::new(f64::from(d.x) / self.scale, -f64::from(d.y) / self.scale);
        if self.twist == 0.0 { q } else { q.rotate(-self.twist) }
    }
    /// The 3D camera-plane offset covered by screen offset `d` (points, y down): a pan shifts
    /// `View::target()` by `-offset`.
    pub fn screen_delta_to_world_3d(&self, d: egui::Vec2) -> cadcraft_geom::Vec3 {
        let eff = self.cam.with_twist(self.twist);
        let du = f64::from(d.x) / self.scale;
        let dv = -f64::from(d.y) / self.scale;
        eff.x * du + eff.y * dv
    }
    /// The world box holding everything on screen (in a twisted or 3D view, the box of the
    /// rect's four unprojected corners, capped around `center` for near-edge-on 3D views).
    pub fn world_bounds(&self) -> Bounds2 {
        let r = self.rect;
        if !self.cam.is_top_plan() {
            let diag = f64::from(r.width().hypot(r.height())) / self.scale.max(1e-12);
            // Cap for near-edge-on views, where the visible ground strip is unbounded, kept tight
            // enough that `grid_index_ranges` stays under its 2000-line limit.
            let cap = diag * 3.0;
            let capped = Bounds2::new(self.center - Vec2::new(cap, cap), self.center + Vec2::new(cap, cap));
            if self.cam.z.z.abs() <= 0.15 {
                return capped;
            }
            let b = Bounds2::from_points([r.left_bottom(), r.right_top(), r.left_top(), r.right_bottom()].map(|p| self.to_world(p)));
            let lo = Vec2::new(b.min.x.clamp(capped.min.x, capped.max.x), b.min.y.clamp(capped.min.y, capped.max.y));
            let hi = Vec2::new(b.max.x.clamp(capped.min.x, capped.max.x), b.max.y.clamp(capped.min.y, capped.max.y));
            return Bounds2::new(lo.min(self.center), hi.max(self.center));
        }
        Bounds2::from_points([r.left_bottom(), r.right_top(), r.left_top(), r.right_bottom()].map(|p| self.to_world(p)))
    }
    /// The GPU transform ([`crate::gpu::CanvasCallback::xform`]) that draws vertex data built at
    /// `origin`, moved by `m` (a drag; [`Mat3::IDENTITY`] for the drawing), in this view: the
    /// shader takes `L·p + e + (origin − center)` to the screen frame, which must be
    /// `R(m(origin + p) − center)` with `R` the twist/camera projection, so `L = R·m` and
    /// `e = R(m(origin) − center) − (origin − center)`.
    pub(crate) fn gpu_xform(&self, origin: Vec2, m: Mat3) -> [f32; 6] {
        let (a, b) = (self.turn(Vec2::new(m.a, m.b)), self.turn(Vec2::new(m.c, m.d)));
        let e = self.turn(m.apply(origin) - self.center) + self.z_shift() - (origin - self.center);
        [a.x as f32, a.y as f32, b.x as f32, b.y as f32, e.x as f32, e.y as f32]
    }
}

/// What a display list was built from: drawing uid, revision, tessellation band, hatch level of
/// detail band, lineweight display, document and active viewport view (see [`ensure_list`]).
type ListKey = (u64, u64, i32, i32, bool, bool, usize, u64);

/// Whether two list keys are for the same drawing content (they may differ only in the view).
fn same_content(a: &ListKey, b: &ListKey) -> bool {
    (a.0, a.1, a.4, a.5, a.6, a.7) == (b.0, b.1, b.4, b.5, b.6, b.7)
}

/// Snapshot captured at the start of an interactive 3D orbit drag (`3DORBIT`, `Shift+MMB`, ViewCube).
#[derive(Clone, Copy, Debug)]
pub(crate) struct OrbitDrag {
    pub(crate) start: Pos2,
    pub(crate) cam0: cadcraft_geom::Camera3D,
    pub(crate) target0: cadcraft_geom::Vec3,
    pub(crate) pivot: cadcraft_geom::Vec3,
}

#[derive(Clone, Copy)]
struct PanDrag {
    start: Pos2,
    target0: cadcraft_geom::Vec3,
}

#[derive(Default)]
pub struct CanvasState {
    pub list: Option<DisplayList>,
    /// The font set the list was drawn with ([`cadcraft_fonts::ttf::generation`] and
    /// [`cadcraft_fonts::shx::generation`]): a new font folder redraws the text.
    font_generation: u64,
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
    hl: HlCache,
    /// Live selection preview candidates (rollover, window/crossing, polygon, fence, stretch).
    pub preview_handles: Vec<Handle>,
    pub preview_action: cadcraft_engine::PreviewAction,
    preview_hl: HlCache,
    preview_key: Option<u64>,
    /// The selection and hover highlights inside the active MSPACE viewport (see [`VpHighlight`]).
    vp_sel: VpHighlight,
    vp_hover: VpHighlight,
    /// Model space seen through the active MSPACE viewport (see [`VpContent`]).
    vp: VpContent,
    /// What a running command shows in place of the drawing (see [`OnlyList`]).
    only: OnlyList,
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
    pan_drag: Option<PanDrag>,
    /// Snapshot for Shift+MMB 3D orbit (MATCAD `camera.data.startPt` / `cam0` + resolved 3D pivot).
    orbit_start: Option<OrbitDrag>,
    /// Snapshot for interactive `3DORBIT` primary drag.
    orbit_drag: Option<OrbitDrag>,
    /// Snapshot and ring flag for ViewCube drag (`move_viewcube.m`).
    pub(crate) viewcube_drag: Option<(OrbitDrag, bool)>,
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
    /// The layers frozen in the active viewport when the object and the cycling badge under the
    /// cursor were last worked out ([`cadcraft_engine::DocState::viewport_frozen`]).
    hover_frozen: Vec<String>,
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
    /// Active 3D orbit pivot during `3DORBIT`, `Shift+MMB`, or `ViewCube` drag, if any.
    pub(crate) fn active_orbit_pivot(&self) -> Option<cadcraft_geom::Vec3> {
        self.orbit_drag
            .or(self.orbit_start)
            .or_else(|| self.viewcube_drag.map(|(d, _)| d))
            .map(|d| d.pivot)
    }

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

#[derive(Clone, Copy)]
struct PointerInput {
    hover_pos: Option<Pos2>,
    scroll: egui::Vec2,
    mods: egui::Modifiers,
    middle_down: bool,
    pressed_primary: bool,
    pressed_secondary: bool,
    dbl_middle: bool,
    dbl_primary: bool,
}

struct Navigation<'a> {
    rect: Rect,
    tiles: &'a [(Rect, cadcraft_engine::ModelTile)],
    response: &'a egui::Response,
    xf: Xf,
    pointer: PointerInput,
}

fn navigate_canvas(app: &mut CadApp, ui: &mut egui::Ui, nav: &Navigation<'_>) -> Option<(bool, bool)> {
    let p = nav.pointer;
    let inside = p.hover_pos.is_some_and(|at| nav.rect.contains(at) && ui.ctx().layer_id_at(at) == Some(ui.layer_id()) && nav.response.hovered());
    let on_menu = p.hover_pos.is_some_and(|at| over_canvas_menu(app, at));
    app.session.ctrl_down = p.mods.ctrl;
    ui.input(|i| {
        for event in &i.raw.events {
            if let egui::Event::MouseWheel { unit, .. } = event {
                app.canvas.scroll_pans = cfg!(target_os = "macos") && *unit == egui::MouseWheelUnit::Point;
            }
        }
    });
    handle_scroll_zoom(app, ui, nav, inside);
    handle_button_navigation(app, nav, inside);
    handle_orbit_drag(app, ui, nav);
    if nav.response.dragged_by(egui::PointerButton::Primary) && app.session.realtime_zoom_active() {
        let _ = app.session.realtime_zoom(f64::from(nav.response.drag_delta().y));
    }
    if p.pressed_primary
        && let Some(at) = p.hover_pos
        && !nav.rect.contains(at)
        && let Some(i) = nav.tiles.iter().position(|(r, _)| r.contains(at))
    {
        if let Err(e) = app.session.click_tile(i) {
            app.session.echo(e.to_string());
        }
        ui.ctx().request_repaint();
        return None;
    }
    Some((inside, on_menu))
}

fn handle_scroll_zoom(app: &mut CadApp, ui: &mut egui::Ui, nav: &Navigation<'_>, inside: bool) {
    let p = nav.pointer;
    if inside
        && app.canvas.scroll_pans
        && p.scroll != egui::Vec2::ZERO
        && let Ok(st) = app.session.state_mut()
    {
        let view = st.view();
        let next_target = view.target() - nav.xf.screen_delta_to_world_3d(p.scroll);
        if next_target.is_finite() {
            st.set_view_quiet(view.with_target(next_target));
        }
        app.canvas.pan_drag = None;
    }
    let opts = ui.ctx().options(|o| o.input_options);
    let wheel = ui.input(|i| wheel_zoom_points(&i.raw.events, &opts, nav.rect.height()));
    if inside
        && !app.canvas.scroll_pans
        && wheel != 0.0
        && let Some(at) = p.hover_pos
    {
        let factor = app.session.wheel_zoom_factor(f64::from(wheel));
        let about = nav.xf.zoom_anchor_3d(at);
        let _ = app.session.zoom_about_3d(factor, about);
        app.canvas.pan_drag = None;
    }
    let pinch = ui.input(|i| crate::animation::zoom_delta(i, &opts, app.shows_animations()));
    if inside
        && (pinch - 1.0).abs() > 1e-4
        && let Some(at) = p.hover_pos
    {
        let _ = app.session.zoom_about_3d(f64::from(pinch), nav.xf.zoom_anchor_3d(at));
        app.canvas.pan_drag = None;
    }
}

/// Effective 3D camera for the active drawing space: Paper space (and layout viewports
/// until per-viewport 3D cameras are introduced) always remain in 2D plan view (`Camera3D::TOP`).
fn can_orbit_3d(st: &cadcraft_engine::DocState) -> bool {
    matches!(st.space, cadcraft_doc::Space::Model) || st.active_viewport().is_some_and(|(_, vp)| !vp.locked)
}

pub(crate) fn model_camera(st: &cadcraft_engine::DocState) -> cadcraft_geom::Camera3D {
    st.zoom_camera()
}

/// Store a new Model-space or active-viewport `camera` (and optional new 3D `target`) after an
/// interactive 3D drag, clearing any consumed 2D `twist` so downstream `with_twist` calls do not
/// apply it a second time.
pub(crate) fn update_model_camera(
    st: &mut cadcraft_engine::DocState,
    new_cam: cadcraft_geom::Camera3D,
    new_target: Option<cadcraft_geom::Vec3>,
) {
    let old_v = st.view();
    let in_viewport = st.active_viewport().is_some();
    if !st.set_active_camera(new_cam) {
        return;
    }
    let clear_twist = !in_viewport && old_v.twist.abs() > 1e-12;
    let target_changed = new_target.is_some_and(|t| (t - old_v.target()).len() > 1e-12);
    if clear_twist || target_changed {
        let mut next = match new_target {
            Some(t) => old_v.with_target(t),
            None => old_v,
        };
        if clear_twist || in_viewport {
            next.twist = 0.0;
        }
        st.set_view_quiet(next);
    }
}

/// Capture an [`OrbitDrag`] snapshot in Model space or an unlocked MSPACE viewport using the
/// hybrid MATCAD 3D ray-hit + AutoCAD `ORBITAUTOTARGET` pivot resolver (`click_pos = Some(...)`
/// tests a 3D hit under the pointer).
pub(crate) fn begin_canvas_orbit(app: &CadApp, start: Pos2, click_pos: Option<Pos2>) -> Option<OrbitDrag> {
    let st = app.session.state().ok()?;
    if !can_orbit_3d(st) {
        return None;
    }
    let click_uv = click_pos.and_then(|pos| app.canvas.xf.map(|xf| xf.screen_uv_at(pos)));
    let hit_tol = app.canvas.xf.map_or(0.0, |xf| 25.0 / xf.scale.max(1e-12));
    let pivot = app.session.resolve_orbit_pivot(click_uv, hit_tol);
    let v = st.view();
    Some(OrbitDrag {
        start,
        cam0: st.zoom_camera().with_twist(v.twist),
        target0: v.target(),
        pivot,
    })
}

/// Apply a constrained orbit (`daz_deg`, `del_deg`) relative to `drag` and rigidly carry
/// `drag.target0` around `drag.pivot` (MATCAD `rotating3D.m`).
pub(crate) fn apply_canvas_orbit(app: &mut CadApp, drag: &OrbitDrag, daz_deg: f64, del_deg: f64) {
    if let Ok(st) = app.session.state_mut()
        && can_orbit_3d(st)
    {
        let (cam1, target1) = drag.cam0.orbit_about_target(drag.target0, drag.pivot, daz_deg, del_deg, true);
        update_model_camera(st, cam1, Some(target1));
    }
}

pub(super) fn axis_colors() -> [(&'static str, cadcraft_geom::Vec3, Color32); 3] {
    [
        ("X", cadcraft_geom::Vec3::new(1.0, 0.0, 0.0), Color32::from_rgb(0xe0, 0x45, 0x45)),
        ("Y", cadcraft_geom::Vec3::new(0.0, 1.0, 0.0), Color32::from_rgb(0x3b, 0xb5, 0x4a)),
        ("Z", cadcraft_geom::Vec3::new(0.0, 0.0, 1.0), Color32::from_rgb(0x3a, 0x82, 0xf6)),
    ]
}

/// Paint the small green AutoCAD-style `3DORBIT` pivot sphere at `center`.
fn draw_orbit_pivot(p: &egui::Painter, center: Pos2) {
    p.circle(
        center,
        5.5,
        Color32::from_rgb(0x3a, 0xb5, 0x4a),
        Stroke::new(1.0, Color32::from_rgb(0x1f, 0x6e, 0x2b)),
    );
    p.circle_filled(center + vec2(-1.6, -1.6), 1.8, Color32::from_rgb(0xb8, 0xf2, 0xbf));
}

/// Resolve `(full_screen_rect, visible_clip_rect)` of `vp` (and optional polygonal `clip`)
/// under `xf_paper`, clipped to the outer canvas `rect`.
fn viewport_screen_bounds(
    rect: Rect,
    xf_paper: &Xf,
    vp: &cadcraft_doc::Viewport,
    clip: Option<&cadcraft_render::clip::PolyClip>,
) -> (Rect, Rect) {
    let vp_full = match clip {
        Some(c) => {
            let b = c.bounds();
            Rect::from_two_pos(
                xf_paper.to_screen(Vec2::new(b.min.x, b.max.y)),
                xf_paper.to_screen(Vec2::new(b.max.x, b.min.y)),
            )
        }
        None => Rect::from_two_pos(
            xf_paper.to_screen(Vec2::new(vp.center.x - vp.width / 2.0, vp.center.y + vp.height / 2.0)),
            xf_paper.to_screen(Vec2::new(vp.center.x + vp.width / 2.0, vp.center.y - vp.height / 2.0)),
        ),
    };
    (vp_full, vp_full.intersect(rect))
}

/// Paper-space coordinate transform for `rect`.
fn paper_xform(st: &cadcraft_engine::DocState, rect: Rect) -> Xf {
    let pv = st.paper_view();
    Xf {
        rect,
        center: pv.center,
        scale: f64::from(rect.height()) / pv.height.max(1e-12),
        twist: 0.0,
        target_z: 0.0,
        cam: cadcraft_geom::Camera3D::TOP,
    }
}

/// Screen-to-model coordinate transform for any layout `vp` under `xf_paper`.
fn viewport_model_xf(rect: Rect, xf_paper: &Xf, vp: &cadcraft_doc::Viewport) -> Xf {
    let k = vp.view_height / vp.height.max(1e-12);
    let eff = vp.effective_camera();
    let d_uv = (xf_paper.center - vp.center.xy()) * k;
    let target = vp.target() + eff.x * d_uv.x + eff.y * d_uv.y;
    let twist = if vp.camera.is_top_plan() { vp.view_twist() } else { 0.0 };
    Xf {
        rect,
        center: target.xy(),
        scale: xf_paper.scale / k.max(1e-12),
        twist,
        target_z: target.z,
        cam: eff,
    }
}

/// Draw the coordinate grid for every visible layout viewport whose own `grid` flag is enabled,
/// both when active (`MSPACE`) and inactive (`PSPACE` or another viewport active), clipped cleanly
/// to the viewport's visible screen/polygon boundary.
fn paint_layout_viewport_grids(app: &CadApp, painter: &egui::Painter, rect: Rect, xf_paper: &Xf, bg: Rgb, t: &Tokens) {
    let Ok(st) = app.session.state() else { return };
    if matches!(st.space, cadcraft_doc::Space::Model) {
        return;
    }
    let Some(store) = st.doc.space(&st.space) else { return };
    let vp_tokens = t.clone().with_model_background(Color32::from_rgb(bg.0, bg.1, bg.2));
    for e in store.iter() {
        let cadcraft_doc::EntityKind::Viewport(vp) = &e.kind else { continue };
        if !vp.on || !vp.grid || vp.width <= 1e-12 || vp.height <= 1e-12 || vp.view_height <= 1e-12 {
            continue;
        }
        let clip = cadcraft_render::viewport_clip(&st.doc, e.handle, vp);
        let (_, vp_visible) = viewport_screen_bounds(rect, xf_paper, vp, clip.as_ref());
        if vp_visible.width() < 1.0 || vp_visible.height() < 1.0 {
            continue;
        }
        let vp_xf = viewport_model_xf(rect, xf_paper, vp);
        let clipped = painter.with_clip_rect(vp_visible);
        grid::draw_grid_surface(
            app,
            &grid::GridSurface {
                painter: &clipped,
                xf: &vp_xf,
                clip_rect: vp_visible,
                tokens: &vp_tokens,
                grid_on: true,
                show_axes_always: false,
                poly_clip: clip.as_ref().map(|c| (xf_paper, c)),
            },
        );
    }
}

/// Active ViewCube `Layout` for the current space (Model space uses `rect`; Layout `MSPACE`
/// scales and anchors inside the active viewport's on-screen pixel bounds). Returns `None` in
/// Paper space or when the active viewport is off-screen.
pub fn active_viewcube_layout(app: &CadApp, rect: Rect) -> Option<crate::viewcube::Layout> {
    let st = app.session.state().ok()?;
    if matches!(st.space, cadcraft_doc::Space::Model) {
        return Some(crate::viewcube::Layout::for_viewport(rect, rect));
    }
    let (h, vp) = st.active_viewport()?;
    let xf_paper = paper_xform(st, rect);
    let clip = cadcraft_render::viewport_clip(&st.doc, h, &vp);
    let (vp_full, vp_visible) = viewport_screen_bounds(rect, &xf_paper, &vp, clip.as_ref());
    (vp_visible.width() >= 1.0 && vp_visible.height() >= 1.0).then(|| crate::viewcube::Layout::for_viewport(vp_full, vp_visible))
}

fn handle_orbit_drag(app: &mut CadApp, ui: &egui::Ui, nav: &Navigation<'_>) {
    if !app.session.orbit_active() || !nav.response.dragged_by(egui::PointerButton::Primary) {
        app.canvas.orbit_drag = None;
        return;
    }
    let Some(curr) = nav.pointer.hover_pos else { return };
    let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(curr);
    if app.canvas.orbit_drag.is_none() {
        let over_cube = app.ui.show_viewcube
            && active_viewcube_layout(app, nav.rect)
                .is_some_and(|l| l.clip.contains(origin) && (l.bounds.contains(origin) || l.pill.contains(origin)));
        if !nav.rect.contains(origin) || over_canvas_menu(app, origin) || over_cube {
            return;
        }
        app.canvas.orbit_drag = begin_canvas_orbit(app, origin, Some(origin));
    }
    if let Some(drag) = app.canvas.orbit_drag {
        let total = curr - drag.start;
        apply_canvas_orbit(app, &drag, f64::from(total.x) * 0.45, f64::from(-total.y) * 0.45);
    }
}

fn handle_button_navigation(app: &mut CadApp, nav: &Navigation<'_>, inside: bool) {
    let p = nav.pointer;
    let middle_pressed = nav.response.ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Middle));
    let focused = nav.response.ctx.input(|i| i.focused);
    if p.middle_down
        && focused
        && (app.canvas.pan_last.is_some() || (middle_pressed && inside))
        && let Some(at) = p.hover_pos
    {
        if p.mods.shift {
            app.canvas.pan_drag = None;
            if app.canvas.orbit_start.is_none() {
                app.canvas.orbit_start = begin_canvas_orbit(app, at, Some(at));
            }
            if let Some(drag) = app.canvas.orbit_start {
                let total = at - drag.start;
                // In egui screen coordinates, Y increases downwards; in MATCAD figure pixels, Y increases upwards.
                apply_canvas_orbit(app, &drag, f64::from(total.x) * 0.45, f64::from(-total.y) * 0.45);
            }
        } else {
            app.canvas.orbit_start = None;
            if app.canvas.pan_drag.is_none()
                && let Ok(st) = app.session.state()
            {
                app.canvas.pan_drag = Some(PanDrag { start: at, target0: st.view().target() });
            }
            if let Some(drag) = app.canvas.pan_drag
                && let Ok(st) = app.session.state_mut()
            {
                let view = st.view();
                let next_target = drag.target0 - nav.xf.screen_delta_to_world_3d(at - drag.start);
                if next_target.is_finite() {
                    st.set_view_quiet(view.with_target(next_target));
                }
            }
        }
        app.canvas.pan_last = Some(at);
    } else {
        app.canvas.pan_last = None;
        app.canvas.pan_drag = None;
        app.canvas.orbit_start = None;
    }
    if p.dbl_middle && inside {
        let _ = app.session.zoom_extents();
    }
}

struct CanvasPaint<'a> {
    app: &'a mut CadApp,
    ui: &'a mut egui::Ui,
    painter: &'a egui::Painter,
    tokens: Tokens,
    full: Rect,
    tiles: &'a [(Rect, cadcraft_engine::ModelTile)],
    rect: Rect,
    xf: Xf,
    xf_paper: Xf,
    view: cadcraft_engine::View,
    active_vp: Option<cadcraft_doc::Viewport>,
    active_clip: Option<cadcraft_render::clip::PolyClip>,
    inside: bool,
    on_menu: bool,
    hover_pos: Option<Pos2>,
    raw_world: Option<Vec2>,
}

fn paint_content_highlights(
    frame: &mut CanvasPaint<'_>,
    bg: Rgb,
    sel: &[Handle],
    preview_look: Look,
    selection_look: Option<Look>,
    selection_overlay: Option<&SelectionOverlayUv>,
) {
    let app = &mut *frame.app;
    let painter = frame.painter;
    let xf_paper = frame.xf_paper;
    let rect = frame.rect;
    let xf = frame.xf;
    let t = frame.tokens;
    let ui = &mut *frame.ui;
    let active_vp = &frame.active_vp;
    let active_clip = &frame.active_clip;
    let scale = frame.xf.scale;
    paint_layout_viewport_grids(app, painter, rect, &xf_paper, bg, &t);
    // The active viewport's contents, drawn apart from the layout's list, under its own objects.
    // `masked`: its outline is in the stencil buffer this frame.
    let mut masked = false;
    if let Some(only) = app.session.display_only() {
        // A command that shows only some objects while it runs (DVIEW's preview set): those,
        // in the view being changed (inside the active viewport, clipped to it).
        let clip = active_vp.as_ref().map_or(rect, |vp| viewport_screen_bounds(rect, &xf_paper, vp, None).1);
        if let Ok(st) = app.session.state() {
            // A clipped viewport's outline, in the model units the preview is built in.
            let outline =
                active_vp.as_ref().zip(active_clip.as_ref()).map(|(vp, c)| c.polygon().iter().map(|q| vp.paper_to_model(*q)).collect::<Vec<_>>());
            app.canvas.only.update(st, &only, scale, outline.as_deref());
        }
        draw_list(&painter.with_clip_rect(clip), &xf, &app.canvas.only.list, bg, app.session.settings.lwdisplay);
    } else {
        if let Some(f) = ensure_vp_content(app, &xf_paper, ui.ctx()) {
            let gpu = app.canvas.gpu.is_some();
            let clipped = painter.with_clip_rect(f.on_screen.intersect(rect));
            draw_vp_content(&mut app.canvas.vp, gpu, &clipped, &f, &xf_paper, bg, app.session.settings.lwdisplay);
            masked = gpu && f.outline.is_some();
        }
        if app.canvas.gpu.is_some() {
            draw_list_gpu(&mut app.canvas, painter, &xf_paper, bg, app.session.settings.lwdisplay);
        } else if let Some(list) = &app.canvas.list {
            draw_list(painter, &xf_paper, list, bg, app.session.settings.lwdisplay);
        }
    }
    // Paint the translucent selection window/polygon fill beneath highlights so glowing entities keep full contrast.
    if let Some(overlay) = selection_overlay {
        draw_selection_area_fill(painter, &xf, &app.session.settings, overlay);
    }
    let (effective_sel, preview_handles, preview_color, preview_look): (std::borrow::Cow<'_, [Handle]>, Vec<Handle>, Color32, Look) = {
        let c = &app.canvas;
        let sel_set: std::collections::HashSet<Handle> = sel.iter().copied().collect();
        if c.cycle.is_some() || c.hover_cycle.is_some() {
            (std::borrow::Cow::Borrowed(sel), c.hover.filter(|h| !sel_set.contains(h)).into_iter().collect(), t.selection, preview_look)
        } else if c.preview_action == cadcraft_engine::PreviewAction::Remove && !c.preview_handles.is_empty() {
            let rm: Vec<Handle> = c.preview_handles.iter().copied().filter(|h| sel_set.contains(h)).collect();
            let rm_set: std::collections::HashSet<Handle> = rm.iter().copied().collect();
            let kept: Vec<Handle> = sel.iter().copied().filter(|h| !rm_set.contains(h)).collect();
            let dim_remove = Color32::from_rgba_unmultiplied(t.selection.r(), t.selection.g(), t.selection.b(), 95);
            let remove_look = Look { width: 1.5, glow: false, bright_core: false, ..preview_look };
            (std::borrow::Cow::Owned(kept), rm, dim_remove, remove_look)
        } else {
            let add: Vec<Handle> = c.preview_handles.iter().copied().filter(|h| !sel_set.contains(h)).collect();
            (std::borrow::Cow::Borrowed(sel), add, t.selection, preview_look)
        }
    };
    if let Some(vp) = &active_vp {
        // The active viewport: a heavy border, highlights in model units clipped to it.
        let r = viewport_screen_bounds(rect, &xf_paper, vp, None).0;
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
            let visible = viewport_screen_bounds(rect, &xf_paper, vp, active_clip.as_ref()).1;
            let clipped = painter.with_clip_rect(visible);
            let c = &mut app.canvas;
            let has_gpu = c.gpu.is_some();
            let stencil = if masked { crate::gpu::Stencil::Inside } else { crate::gpu::Stencil::Off };
            if let Some(look) = selection_look
                && !effective_sel.is_empty()
            {
                c.vp_sel.update(st, &effective_sel, scale);
                let list = &c.vp_sel.list;
                let list_id = c.vp_sel.list_gen | 1 << 63;
                draw_highlight_layer(
                    &clipped,
                    &xf,
                    list,
                    list_id,
                    || (0..list.prims.len()).collect(),
                    &effective_sel,
                    t.selection,
                    look,
                    has_gpu,
                    &mut c.hl,
                    crate::gpu::LAYER_HIGHLIGHT,
                    stencil,
                );
            }
            if !preview_handles.is_empty() {
                c.vp_hover.update(st, &preview_handles, scale);
                let list = &c.vp_hover.list;
                let list_id = c.vp_hover.list_gen | 1 << 62;
                draw_highlight_layer(
                    &clipped,
                    &xf,
                    list,
                    list_id,
                    || (0..list.prims.len()).collect(),
                    &preview_handles,
                    preview_color,
                    preview_look,
                    has_gpu,
                    &mut c.preview_hl,
                    crate::gpu::LAYER_PREVIEW,
                    stencil,
                );
            }
        }
    } else if let CanvasState { list: Some(list), list_gen, prim_index, gpu, hl, preview_hl, .. } = &mut app.canvas {
        let has_gpu = gpu.is_some();
        if let Some(look) = selection_look {
            draw_highlight_layer(
                painter,
                &xf,
                list,
                *list_gen,
                || prim_index.prims(*list_gen, list, &effective_sel),
                &effective_sel,
                t.selection,
                look,
                has_gpu,
                hl,
                crate::gpu::LAYER_HIGHLIGHT,
                crate::gpu::Stencil::Off,
            );
        }
        draw_highlight_layer(
            painter,
            &xf,
            list,
            *list_gen,
            || prim_index.prims(*list_gen, list, &preview_handles),
            &preview_handles,
            preview_color,
            preview_look,
            has_gpu,
            preview_hl,
            crate::gpu::LAYER_PREVIEW,
            crate::gpu::Stencil::Off,
        );
    }
}

fn paint_pointer_overlay(
    frame: &mut CanvasPaint<'_>,
    paper_rect: Option<Rect>,
    surround: Color32,
    selecting: bool,
    sheet: Option<cadcraft_render::Sheet>,
) {
    let app = &mut *frame.app;
    let ui = &mut *frame.ui;
    let painter = frame.painter;
    let t = frame.tokens;
    let rect = frame.rect;
    let full = frame.full;
    let tiles = frame.tiles;
    let xf = frame.xf;
    let inside = frame.inside;
    let on_menu = frame.on_menu;
    let hover_pos = frame.hover_pos;
    let view = frame.view;
    // Acquired tracking points and the polar / tracking paths under the cursor.
    if app.canvas.cursor.is_some() {
        draw_tracking(painter, &xf, &app.session.tracking.acquired, app.canvas.track.as_ref(), view.height * 4.0);
    }
    if let (Some(hit), Some(_)) = (app.canvas.snap, app.canvas.cursor) {
        draw_snap_marker(painter, xf.to_screen(hit.point), &hit);
    }
    draw_tiles(app, ui, tiles);
    if !app.session.settings.orbit_free_camera
        && (app.session.orbit_active() || app.canvas.orbit_start.is_some() || app.canvas.viewcube_drag.is_some())
    {
        let pivot = app.canvas.active_orbit_pivot().unwrap_or_else(|| app.session.resolve_orbit_pivot(None, 0.0));
        draw_orbit_pivot(painter, xf.to_screen_3d(pivot));
    }
    // UCS icon, ViewCube and viewport label.
    let xf_paper = frame.xf_paper;
    let overlay_bounds = frame.active_vp.as_ref().map(|vp| viewport_screen_bounds(rect, &xf_paper, vp, frame.active_clip.as_ref()));
    let (ov_full, ov_clip) = overlay_bounds.unwrap_or((rect, rect));
    let in_mspace = frame.active_vp.is_some();
    let show_viewport_overlays = !in_mspace || (ov_clip.width() >= 1.0 && ov_clip.height() >= 1.0);
    let bg_at = |pt: Pos2| {
        paper_rect.map_or(t.canvas, |pr| if pr.contains(pt) { t.paper } else { surround })
    };
    if app.ui.show_ucs_icon && show_viewport_overlays {
        draw_ucs_icon(painter, ov_full, ov_clip, &xf.cam);
        crate::viewcube::ucs_icon_with_camera(app, ui, ov_full, ov_clip, &xf.cam);
    }
    if app.ui.show_viewcube && (sheet.is_none() || (in_mspace && show_viewport_overlays)) {
        let vc_bg = bg_at(crate::viewcube::Layout::for_viewport(ov_full, ov_clip).center);
        draw_viewcube(app, ui, ov_full, ov_clip, vc_bg);
    }
    if show_viewport_overlays {
        let label_bg = bg_at(ov_clip.left_top() + vec2(12.0, 10.0));
        viewport_label(painter, ov_clip, &xf.cam, app.session.orbit_active(), label_bg);
    }
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
            draw_crosshair(app, painter, rect, hp, selecting, ink);
            if app.canvas.cycle_badge && app.canvas.cycle.is_none() {
                cycle_badge(painter, hp);
            }
        }
    }
    if inside
        && app.session.settings.dynmode
        && let Some(hp) = hover_pos
    {
        // A stretched grip shows its dimensions instead of the pointer boxes; a prompt asking for
        // a distance shows its one field and keeps the prompt and its options.
        let fields = crate::grip_dyn::draw(app, painter);
        if !fields || app.session.current_prompt().is_some_and(|p| p.distance) {
            dynamic_input(app, painter, hp, fields);
        }
    }
    // Over the crosshair, so the row under the pointer reads clearly.
    cycle_list(app, painter);
    // No continuous repaint while hovering: input events repaint by themselves. Repainting every
    // frame kept a vsync swapchain's queue full (every frame waited behind queued ones) and would
    // spin the CPU with the low-latency present mode (`gpu::surface_config`).
}
struct DoubleClickContext<'a> {
    inside: bool,
    double: bool,
    hover: Option<Pos2>,
    xf: Xf,
    xf_paper: Xf,
    scale: f64,
    active_viewport: &'a Option<cadcraft_doc::Viewport>,
    active_clip: &'a Option<cadcraft_render::clip::PolyClip>,
}

struct DoubleClickOutcome {
    hatch_dialog: bool,
    switched_viewport: bool,
}

/// Compute the canvas & paper transforms and active viewport metadata from the current session state.
fn compute_canvas_xforms(
    app: &mut CadApp,
    rect: Rect,
) -> (
    cadcraft_engine::View,
    f64,
    Xf,
    Xf,
    Option<cadcraft_doc::Viewport>,
    Option<cadcraft_render::clip::PolyClip>,
) {
    let view = app.session.state().map(|s| s.view()).unwrap_or_default();
    let scale = f64::from(rect.height()) / view.height.max(1e-12);
    let cam = app.session.state().map(model_camera).unwrap_or_default();
    let xf = Xf { rect, center: view.center, scale, twist: view.twist, target_z: view.target_z, cam };
    app.canvas.xf = Some(xf);
    let (xf_paper, active_vp) = match app.session.state() {
        Ok(st) => (if st.mspace.is_some() { paper_xform(st, rect) } else { xf }, st.active_viewport().map(|(_, v)| v)),
        Err(_) => (xf, None),
    };
    let active_clip = app.session.state().ok().and_then(|st| st.active_viewport().and_then(|(h, v)| cadcraft_render::viewport_clip(&st.doc, h, &v)));
    (view, scale, xf, xf_paper, active_vp, active_clip)
}

fn handle_double_click(app: &mut CadApp, ctx: DoubleClickContext<'_>) -> Option<DoubleClickOutcome> {
    let DoubleClickContext { inside, double, hover, xf, xf_paper, scale, active_viewport, active_clip } = ctx;
    if inside
        && double
        && !app.session.prompting()
        && matches!(app.session.layout_space(), cadcraft_doc::Space::Paper(_))
        && let Some(hp) = hover
    {
        let pp = xf_paper.to_world(hp);
        let in_rect = |v: &cadcraft_doc::Viewport| (pp.x - v.center.x).abs() <= v.width / 2.0 && (pp.y - v.center.y).abs() <= v.height / 2.0;
        let in_active = active_viewport.as_ref().is_some_and(|v| active_clip.as_ref().map_or(in_rect(v), |c| c.contains(pp)));
        if !in_active {
            let before_mspace = app.session.state().ok().and_then(|st| st.mspace);
            app.canvas.hot_grips.clear();
            app.session.pending_window = None;
            app.canvas.drag_window = false;
            if app.run("mspace", serde_json::json!({"at": [pp.x, pp.y]})).is_err() && active_viewport.is_some() {
                let _ = app.run("pspace", serde_json::json!({}));
            }
            app.session.pending_window = None;
            app.canvas.drag_window = false;
            let after_mspace = app.session.state().ok().and_then(|st| st.mspace);
            if after_mspace != before_mspace {
                app.canvas.list = None;
                app.canvas.vp.key = None;
                return Some(DoubleClickOutcome { hatch_dialog: false, switched_viewport: true });
            }
        }
    }
    // Double-click text to edit it, as in AutoCAD: TEXTEDIT with that object already picked.
    if inside
        && double
        && app.session.running.is_none()
        && let Some(hp) = hover
        && edit_text_at(app, xf.to_world(hp), xf.screen_uv_at(hp), &xf.pick_projection())
    {
        return None;
    }

    let mut hatch_dialog = false;
    // Double-click a hatch (no command running) to edit it in the Hatch Edit dialog. The click is
    // used up: it must not also start a grip edit on the hatch's centre grip below.
    if inside
        && double
        && app.session.running.is_none()
        && let Some(hp) = hover
        && let Some(h) = hatch_at(app, xf.screen_uv_at(hp), &xf.pick_projection(), scale)
    {
        app.session.set_selection(vec![h]);
        crate::hatchdlg::open_edit(app);
        hatch_dialog = true;
    }

    Some(DoubleClickOutcome { hatch_dialog, switched_viewport: false })
}

fn update_canvas_cursor(
    app: &mut CadApp,
    ui: &egui::Ui,
    xf: &Xf,
    inside: bool,
    hover: Option<Pos2>,
) -> (Option<Vec2>, Option<crate::dyninput::Frame>) {
    let raw_world = hover.filter(|_| inside).map(|p| xf.to_world(p));
    let dyn_frame = crate::cmdline::dyn_frame(app);
    if let Some(w) = raw_world {
        let mut eff = effective_point(app, w, xf, ui.ctx());
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
            override_cursor(app, w, xf);
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

    (raw_world, dyn_frame)
}

fn draw_selection_area_fill(
    painter: &egui::Painter,
    xf: &Xf,
    settings: &cadcraft_engine::Settings,
    overlay: &SelectionOverlayUv,
) {
    match overlay {
        SelectionOverlayUv::Box { a_uv, b_uv, crossing } => {
            let corners = [*a_uv, Vec2::new(b_uv.x, a_uv.y), *b_uv, Vec2::new(a_uv.x, b_uv.y)].map(|q| xf.screen_uv_to_screen(q));
            if let Some(fill) = selection_area_fill(settings, *crossing) {
                painter.add(Shape::convex_polygon(corners.to_vec(), fill, Stroke::NONE));
            }
        }
        SelectionOverlayUv::Polygon { pts_uv, crossing } => {
            if pts_uv.len() >= 3
                && let Some(fill) = selection_area_fill(settings, *crossing)
            {
                let tris = cadcraft_render::triangulate_evenodd(&[pts_uv.clone()]);
                let mut mesh = egui::Mesh::default();
                for tri in tris.chunks_exact(3) {
                    let i = mesh.vertices.len() as u32;
                    for q in tri {
                        mesh.colored_vertex(xf.screen_uv_to_screen(*q), fill);
                    }
                    mesh.add_triangle(i, i + 1, i + 2);
                }
                if !mesh.vertices.is_empty() {
                    painter.add(Shape::mesh(mesh));
                }
            }
        }
        SelectionOverlayUv::Fence { .. } => {}
    }
}

fn draw_selection_area_border(painter: &egui::Painter, xf: &Xf, t: &Tokens, overlay: &SelectionOverlayUv) {
    let stroke_for = |crossing| Stroke::new(1.0, if crossing { t.crossing_stroke } else { t.window_stroke });
    match overlay {
        SelectionOverlayUv::Box { a_uv, b_uv, crossing } => {
            let corners = [*a_uv, Vec2::new(b_uv.x, a_uv.y), *b_uv, Vec2::new(a_uv.x, b_uv.y)].map(|q| xf.screen_uv_to_screen(q));
            let st = stroke_for(*crossing);
            if *crossing {
                painter.extend(dashed_polygon(painter.clip_rect(), &corners, st, 5.0, 3.0));
            } else {
                let mut pts = corners.to_vec();
                pts.push(corners[0]);
                painter.add(Shape::line(pts, st));
            }
        }
        SelectionOverlayUv::Polygon { pts_uv, crossing } => {
            let screen: Vec<Pos2> = pts_uv.iter().map(|q| xf.screen_uv_to_screen(*q)).collect();
            let st = stroke_for(*crossing);
            if *crossing {
                painter.extend(dashed_polygon(painter.clip_rect(), &screen, st, 5.0, 3.0));
            } else if screen.len() >= 2 {
                let mut closed = screen;
                closed.push(closed[0]);
                painter.add(Shape::line(closed, st));
            }
        }
        SelectionOverlayUv::Fence { pts_uv } => {
            let screen: Vec<Pos2> = pts_uv.iter().map(|q| xf.screen_uv_to_screen(*q)).collect();
            let st = stroke_for(true);
            painter.extend(dashed_polyline(painter.clip_rect(), &screen, st, 5.0, 3.0));
        }
    }
}

fn requires_xy_plane_snap(app: &CadApp, xf: &Xf, point_prompt: bool, dyn_fully_locked: bool) -> bool {
    point_prompt && app.canvas.snap.is_none() && !dyn_fully_locked && xf.cam.is_edge_on_xy() && app.session.select_mode.is_none()
}

fn overlay_cache_signature(overlay: Option<&cadcraft_engine::SelectionOverlayUv>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    match overlay {
        None => 0u8.hash(&mut h),
        Some(cadcraft_engine::SelectionOverlayUv::Box { a_uv, b_uv, crossing }) => {
            (1u8, a_uv.x.to_bits(), a_uv.y.to_bits(), b_uv.x.to_bits(), b_uv.y.to_bits(), crossing).hash(&mut h);
        }
        Some(cadcraft_engine::SelectionOverlayUv::Polygon { pts_uv, crossing }) => {
            (2u8, pts_uv.len(), crossing).hash(&mut h);
            for p in pts_uv {
                (p.x.to_bits(), p.y.to_bits()).hash(&mut h);
            }
        }
        Some(cadcraft_engine::SelectionOverlayUv::Fence { pts_uv }) => {
            (3u8, pts_uv.len()).hash(&mut h);
            for p in pts_uv {
                (p.x.to_bits(), p.y.to_bits()).hash(&mut h);
            }
        }
    }
    h.finish()
}

fn update_hover(app: &mut CadApp, ui: &egui::Ui, xf: &Xf, inside: bool, hover_pos: Option<Pos2>) {
    // The cursor picks objects (pick box, rollover highlight) or previews active selection areas
    // (Window, Crossing, WP, CP, Fence, Stretch); SELECTIONPREVIEW controls when it shows.
    let selecting = app.session.previews_selection();
    // What the cursor is over also depends on the layers frozen in the active viewport, and those
    // can change while the pointer rests (VPLAYER, undo, another viewport made the active one).
    let refrozen = match app.session.state() {
        Ok(st) if st.viewport_frozen() != app.canvas.hover_frozen.as_slice() => {
            app.canvas.hover_frozen = st.viewport_frozen().to_vec();
            true
        }
        _ => false,
    };
    let shift = ui.input(|i| i.modifiers.shift);
    let proj = xf.pick_projection();
    if inside
        && selecting
        && let (Some(hp), Ok(st)) = (hover_pos, app.session.state())
    {
        let uv = xf.screen_uv_at(hp);
        let overlay = app.session.selection_overlay_uv(uv, &proj);
        let drag_below_threshold = app.canvas.drag_window
            && app.session.pending_window.is_some_and(|pw| hp.distance(xf.to_screen_3d(pw.corner_3d)) < 3.0);
        let cache_key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            let v = st.view();
            let s = &app.session.settings;
            let cmd_id = app.session.running.as_ref().map(|r| r.id.as_str());
            let prompt_msg = app.session.current_prompt().map(|p| p.message.clone());
            let mode_sig = app.session.select_mode.as_ref().map(|m| (m.kind as u8, m.pts.len()));
            (
                (hp.x.to_bits(), hp.y.to_bits(), uv.x.to_bits(), uv.y.to_bits(), st.uid, st.revision),
                (
                    xf.rect.min.x.to_bits(),
                    xf.rect.min.y.to_bits(),
                    xf.rect.max.x.to_bits(),
                    xf.rect.max.y.to_bits(),
                    xf.center.x.to_bits(),
                    xf.center.y.to_bits(),
                    xf.scale.to_bits(),
                    v.target_z.to_bits(),
                    v.twist.to_bits(),
                ),
                (
                    (st.camera.x.x.to_bits(), st.camera.x.y.to_bits(), st.camera.x.z.to_bits()),
                    (st.camera.y.x.to_bits(), st.camera.y.y.to_bits(), st.camera.y.z.to_bits()),
                    (st.camera.z.x.to_bits(), st.camera.z.y.to_bits(), st.camera.z.z.to_bits()),
                    (st.camera.up.x.to_bits(), st.camera.up.y.to_bits(), st.camera.up.z.to_bits()),
                ),
                (cmd_id, prompt_msg, mode_sig, app.session.pending_window.is_some(), drag_below_threshold),
                overlay_cache_signature(overlay.as_ref()),
                &st.selection,
                st.viewport_frozen(),
                (shift, s.selectionpreview, s.previewfilter, s.selectionpreviewlimit, s.pickbox.to_bits(), s.pickadd, s.pickstyle),
            )
                .hash(&mut h);
            h.finish()
        };
        if app.canvas.preview_key != Some(cache_key) || refrozen {
            if drag_below_threshold {
                app.canvas.hover = None;
                app.canvas.preview_handles.clear();
                app.canvas.preview_action = cadcraft_engine::PreviewAction::Add;
            } else {
                let ap = app.session.settings.pickbox.max(1.0) * 1.5 / xf.scale.max(1e-12);
                let preview = app.session.preview_selection_uv(uv, ap, shift, &proj);
                app.canvas.preview_handles = preview.handles;
                app.canvas.preview_action = preview.action;
                app.canvas.hover = if overlay.is_none()
                    && app.session.select_mode.is_none()
                    && app.session.pending_window.is_none()
                {
                    app.canvas.preview_handles.first().copied()
                } else {
                    None
                };
            }
            app.canvas.hover_at = hover_pos;
            app.canvas.preview_key = Some(cache_key);
        }
    } else {
        app.canvas.hover = None;
        app.canvas.hover_at = None;
        app.canvas.preview_handles.clear();
        app.canvas.preview_action = cadcraft_engine::PreviewAction::Add;
        app.canvas.preview_key = None;
    }
    // The open selection cycling list highlights the object of the row under the pointer.
    if app.canvas.cycle.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.canvas.cycle = None;
    }
    if let Some(c) = &app.canvas.cycle {
        app.canvas.hover = hover_pos.and_then(|hp| c.row_at(hp));
        app.canvas.hover_at = None;
    }
    shift_space_cycle(app, ui, xf, hover_pos.filter(|_| inside));
    if inside && (hover_pos != app.canvas.cycle_badge_at || refrozen) {
        app.canvas.cycle_badge = hover_pos.is_some_and(|hp| app.session.cycling_badge_at(xf.screen_uv_at(hp), &proj));
        app.canvas.cycle_badge_at = hover_pos;
    } else if !inside {
        app.canvas.cycle_badge = false;
        app.canvas.cycle_badge_at = None;
    }
}

struct PromptClickContext<'a> {
    rect: Rect,
    xf: Xf,
    inside: bool,
    middle_down: bool,
    primary_clicked: bool,
    response: &'a egui::Response,
}

fn collect_prompt_clicks(app: &mut CadApp, ui: &egui::Ui, context: PromptClickContext<'_>) -> bool {
    // egui drops a click whose button comes up a few pixels from where it went down (a drag to it),
    // and eframe hands a slow frame's clicks over together, which egui reports as one.
    let point_prompt = app.session.current_prompt().is_some_and(|p| cadcraft_engine::pointmod::accepts(&p))
        && app.session.pending_window.is_none()
        && !app.session.realtime_zoom_active();
    let pressed_primary = context.primary_clicked || (point_prompt && context.response.drag_stopped_by(egui::PointerButton::Primary));
    if point_prompt && context.inside && pressed_primary && !context.middle_down && app.cmd.options.is_none() && app.cmd.buffer.is_empty() {
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
            for pos in earlier.iter().filter(|p| context.rect.contains(**p)) {
                let w = effective_point(app, context.xf.to_world(*pos), &context.xf, ui.ctx());
                if requires_xy_plane_snap(app, &context.xf, true, false) {
                    app.session.echo("Cannot place an unconstrained point on the XY plane in an edge-on view; snap to an object, enter coordinates, or orbit the view.");
                    continue;
                }
                app.session.set_pending_pick(context.xf.screen_uv_at(*pos), context.xf.pick_projection());
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

    pressed_primary
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
    let cam = model_camera(&st);
    let xf = Xf { rect, center: view.center, scale, twist: view.twist, target_z: view.target_z, cam };
    app.canvas.xf = Some(xf);

    // ---------- input ----------
    let pointer = ui.input(|i| PointerInput {
        hover_pos: i.pointer.hover_pos(),
        scroll: i.smooth_scroll_delta,
        mods: i.modifiers,
        middle_down: i.pointer.middle_down(),
        pressed_primary: i.pointer.primary_clicked(),
        pressed_secondary: i.pointer.secondary_clicked(),
        dbl_middle: i.pointer.button_double_clicked(egui::PointerButton::Middle),
        dbl_primary: i.pointer.button_double_clicked(egui::PointerButton::Primary),
    });
    let navigation = Navigation { rect, tiles: &tiles, response: &resp, xf, pointer };
    let Some((inside, on_menu)) = navigate_canvas(app, ui, &navigation) else { return };
    let PointerInput { hover_pos, mods, middle_down, pressed_primary, pressed_secondary, dbl_primary, .. } = pointer;
    // Re-read the view after navigation, handle double-click transitions, and refresh transforms
    // in the same frame so layout <-> MSPACE switches paint immediately without a blank frame.
    let (mut view, scale, mut xf, mut xf_paper, mut active_vp, mut active_clip) = compute_canvas_xforms(app, rect);
    let Some(dc) = handle_double_click(
        app,
        DoubleClickContext {
            inside,
            double: dbl_primary,
            hover: hover_pos,
            xf,
            xf_paper,
            scale,
            active_viewport: &active_vp,
            active_clip: &active_clip,
        },
    ) else {
        return;
    };
    if dc.switched_viewport {
        (view, _, xf, xf_paper, active_vp, active_clip) = compute_canvas_xforms(app, rect);
    }
    let hatch_dialog = dc.hatch_dialog || dc.switched_viewport;

    let pressed_primary =
        collect_prompt_clicks(app, ui, PromptClickContext { rect, xf, inside, middle_down, primary_clicked: pressed_primary, response: &resp });

    let (raw_world, dyn_frame) = update_canvas_cursor(app, ui, &xf, inside, hover_pos);

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
    let cycled = pressed_primary && !middle_down && cycle_click(app, &xf, hover_pos, inside, mods.shift);
    // Clicks.
    if inside
        && pressed_primary
        && !middle_down
        && !cycled
        && !app.session.orbit_active()
        && let Some(p) = app.canvas.cursor
    {
        let click_uv = hover_pos.map(|hp| xf.screen_uv_at(hp));
        let proj = xf.pick_projection();
        if app.session.prompting() {
            // Ctrl held while picking the point of a grip edit makes copies (the Copy option).
            if mods.ctrl && app.session.running.as_ref().is_some_and(|r| r.id == "grip") {
                let _ = app.session.input(Input::Keyword("Copy".into()));
            }
            // Preserve a snap's viewport origin for DIST, and its deferred curve where needed.
            let point_prompt = app.session.current_prompt().is_some_and(|pr| cadcraft_engine::pointmod::accepts(&pr))
                && app.session.pending_window.is_none()
                && !app.session.realtime_zoom_active();
            let parsed_dyn = dyn_frame.and_then(|f| crate::dyninput::parse(&app.cmd.buffer, &f));
            let dyn_fully_locked =
                parsed_dyn.as_ref().is_some_and(|e| app.cmd.second_locked() && e.first_locked().is_some() && e.second_value().is_some());
            if requires_xy_plane_snap(app, &xf, point_prompt, dyn_fully_locked) {
                app.session.echo("Cannot place an unconstrained point on the XY plane in an edge-on view; snap to an object, enter coordinates, or orbit the view.");
            } else {
                // Clicking places the point; a value typed into the Dynamic Input boxes is used up.
                if parsed_dyn.is_some() {
                    app.cmd.buffer.clear();
                }
                if let Some(uv) = click_uv {
                    app.session.set_pending_pick(uv, proj);
                }
                let result = match app.canvas.snap {
                    Some(hit) => app.session.input_snapped(hit),
                    None => app.session.input(Input::Point(p)),
                };
                if let Err(e) = result {
                    app.session.echo(e.to_string());
                }
            }
        } else if let Some(g) = crate::dyngrips::at(app, &xf, hover_pos) {
            crate::dyngrips::click(app, g);
        } else if let Some(g) = grip_at(app, &xf, hover_pos) {
            click_grip(app, g, mods.shift);
        } else {
            let uv = click_uv.unwrap_or_else(|| proj.project(p));
            if let Err(e) = app.session.idle_click_at(raw_world.unwrap_or(p), uv, &proj, mods.shift) {
                app.session.echo(e.to_string());
            }
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
        && !app.session.orbit_active()
        && !app.session.realtime_zoom_active()
        && app.canvas.cycle.is_none()
        && app.session.pending_window.is_none()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        && rect.contains(origin)
        && grip_at(app, &xf, Some(origin)).is_none()
        && crate::dyngrips::at(app, &xf, Some(origin)).is_none()
    {
        app.canvas.drag_window = app.session.begin_window_at(xf.to_world(origin), xf.screen_uv_at(origin), &xf.pick_projection());
    }
    if app.canvas.drag_window && resp.drag_stopped_by(egui::PointerButton::Primary) {
        app.canvas.drag_window = false;
        if app.session.pending_window.is_some()
            && let Some(end) = ui.input(|i| i.pointer.interact_pos())
        {
            let end_uv = xf.screen_uv_at(end);
            let proj = xf.pick_projection();
            let end_w = xf.to_world(end);
            app.session.set_pending_pick(end_uv, proj);
            let done = if app.session.prompting() {
                app.session.input(Input::Point(end_w))
            } else {
                app.session.idle_click_at(end_w, end_uv, &proj, mods.shift)
            };
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

    update_hover(app, ui, &xf, inside, hover_pos);

    paint_canvas(CanvasPaint {
        app,
        ui,
        painter: &painter,
        tokens: t,
        full,
        tiles: &tiles,
        rect,
        xf,
        xf_paper,
        view,
        active_vp,
        active_clip,
        inside,
        on_menu,
        hover_pos,
        raw_world,
    });
}

fn paint_canvas(frame: CanvasPaint<'_>) {
    let CanvasPaint {
        app,
        ui,
        painter,
        tokens: t,
        full,
        tiles,
        rect,
        xf,
        xf_paper,
        view,
        active_vp,
        active_clip,
        inside,
        on_menu,
        hover_pos,
        raw_world,
    } = frame;
    let scale = xf.scale;
    let selecting = app.session.previews_selection()
        && app.session.pending_window.is_none()
        && app.session.select_mode.is_none()
        && app.session.current_prompt().is_none_or(|p| !p.corner);
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
            painter.extend(dashed_rect(painter.clip_rect(), pa, Stroke::new(1.0, Color32::from_gray(150)), 4.0, 4.0));
            (Rgb(t.paper.r(), t.paper.g(), t.paper.b()), Some(paper))
        }
        None => {
            blockedit::wash(app, painter, rect);
            draw_grid(app, painter, &xf);
            (Rgb(t.canvas.r(), t.canvas.g(), t.canvas.b()), None)
        }
    };
    if let Some(paper) = paper_rect
        && app.session.settings.paper_gridmode
    {
        let paper_clip = paper.intersect(rect);
        if paper_clip.width() >= 1.0 && paper_clip.height() >= 1.0 {
            let paper_tokens = t.clone().with_model_background(t.paper);
            grid::draw_grid_surface(
                app,
                &grid::GridSurface {
                    painter: &painter.with_clip_rect(paper_clip),
                    xf: &xf_paper,
                    clip_rect: paper_clip,
                    tokens: &paper_tokens,
                    grid_on: true,
                    show_axes_always: false,
                    poly_clip: None,
                },
            );
        }
    }
    let sel = app.session.selection();
    let cursor_uv = hover_pos.map(|hp| xf.screen_uv_at(hp)).or_else(|| raw_world.map(|w| xf.pick_projection().project(w)));
    let selection_overlay = cursor_uv.and_then(|uv| app.session.selection_overlay_uv(uv, &xf.pick_projection()));
    // How the object under the cursor and the selected ones are highlighted (PREVIEWEFFECT,
    // HIGHLIGHT, SELECTIONEFFECT); with HIGHLIGHT off only their grips show.
    let (preview_look, selection_look) = (Look::preview(&app.session.settings), Look::selection(&app.session.settings));
    paint_content_highlights(
        &mut CanvasPaint {
            app,
            ui,
            painter,
            tokens: t,
            full,
            tiles,
            rect,
            xf,
            xf_paper,
            view,
            active_vp: active_vp.clone(),
            active_clip: active_clip.clone(),
            inside,
            on_menu,
            hover_pos,
            raw_world,
        },
        bg,
        &sel,
        preview_look,
        selection_look,
        selection_overlay.as_ref(),
    );
    app.canvas.draw_ms = crate::now_ms() - t0;
    compare::paint(app, ui.ctx(), painter, &xf_paper, rect);
    blockedit::paint(app, ui.ctx(), painter, &xf_paper, rect);
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
            if let Some(e) = st.doc.entity(*h)
                && st.shows(e)
            {
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
        crate::dyngrips::draw(app, painter, &xf, rect, hover_pos);
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
            draw_drag_gpu(&mut app.canvas, painter, &xf, &st.doc, &drag, (&app.session.clipboard, serial), bg, transparency);
            let list = rebuilt_preview(&st.doc, extra.iter(), scale);
            draw_list(painter, &xf, &list, bg, false);
        }
    } else if let (Some(c), true) = (app.canvas.cursor, app.session.prompting()) {
        let ents = app.session.preview(c);
        if !ents.is_empty()
            && let Ok(st) = app.session.state()
        {
            let list = rebuilt_preview(&st.doc, ents.iter(), scale);
            draw_list(painter, &xf, &list, bg, false);
        }
    }
    // Pending window or typed selection mode (W/C/BOX/WP/CP/F) rubberband border in screen/camera-UV space.
    if let Some(overlay) = selection_overlay.as_ref() {
        draw_selection_area_border(painter, &xf, &t, overlay);
    }
    paint_pointer_overlay(
        &mut CanvasPaint {
            app,
            ui,
            painter,
            tokens: t,
            full,
            tiles,
            rect,
            xf,
            xf_paper,
            view,
            active_vp: active_vp.clone(),
            active_clip: active_clip.clone(),
            inside,
            on_menu,
            hover_pos,
            raw_world,
        },
        paper_rect,
        surround,
        selecting,
        sheet,
    );
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod tests;

/// The Block Editor's tint, base point and bar.
mod blockedit;
/// The COMPARE overlay.
mod compare;

/// Background rebuilds of the display list for a new view.
mod images;
mod rebuild;
