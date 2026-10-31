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
use draw::{dashed_rect, draw_drag_gpu, draw_highlight_gpu, draw_list_gpu, list_mesh_key, selection_area_fill};
use dyn_boxes::dynamic_input;
pub(crate) use dyn_boxes::length_units;
use grid::draw_grid;
use grips::{click_grip, grip_at, grip_menu, grip_options, grip_rect};
use list::{PrimIndex, ensure_list, rebuilt_preview};
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
        st.set_view_quiet(cadcraft_engine::View {
            center: view.center - Vec2::new(f64::from(p.scroll.x) / nav.xf.scale, -f64::from(p.scroll.y) / nav.xf.scale),
            height: view.height,
        });
    }
    let opts = ui.ctx().options(|o| o.input_options);
    let wheel = ui.input(|i| wheel_zoom_points(&i.raw.events, &opts, nav.rect.height()));
    if inside
        && !app.canvas.scroll_pans
        && wheel != 0.0
        && let Some(at) = p.hover_pos
    {
        let factor = app.session.wheel_zoom_factor(f64::from(wheel));
        let about = nav.xf.to_world(at);
        let _ = app.session.zoom_about(factor, about);
    }
    let pinch = ui.input(|i| crate::animation::zoom_delta(i, &opts, app.shows_animations()));
    if inside
        && (pinch - 1.0).abs() > 1e-4
        && let Some(at) = p.hover_pos
    {
        let _ = app.session.zoom_about(f64::from(pinch), nav.xf.to_world(at));
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
        if let Some(last) = app.canvas.pan_last {
            let delta = at - last;
            if let Ok(st) = app.session.state_mut() {
                let view = st.view();
                st.set_view_quiet(cadcraft_engine::View {
                    center: view.center - Vec2::new(f64::from(delta.x) / nav.xf.scale, -f64::from(delta.y) / nav.xf.scale),
                    height: view.height,
                });
            }
        }
        app.canvas.pan_last = Some(at);
    } else {
        app.canvas.pan_last = None;
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

fn paint_content_highlights(frame: &mut CanvasPaint<'_>, bg: Rgb, sel: &[Handle], preview_look: Look, selection_look: Option<Look>) {
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
        draw_list_gpu(&mut app.canvas, painter, &xf_paper, bg, app.session.settings.lwdisplay);
    } else if let Some(list) = &app.canvas.list {
        draw_list(painter, &xf_paper, list, bg, app.session.settings.lwdisplay);
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
                c.vp_sel.update(st, sel, scale);
                let list = &c.vp_sel.list;
                let all = || (0..list.prims.len()).collect::<Vec<_>>();
                if c.gpu.is_some() {
                    // Shares the GPU highlight layer with the layout's own selection; the high bit
                    // keeps this list's generations apart from the layout list's.
                    let list_id = c.vp_sel.list_gen | 1 << 63;
                    let stencil = if masked { crate::gpu::Stencil::Inside } else { crate::gpu::Stencil::Off };
                    let hl = (&mut c.hl_key, &c.hl_slot, &mut c.hl_origin, &mut c.hl_infinite);
                    draw_highlight_gpu(&clipped, &xf, list, list_id, all, sel, t.selection, look, hl, stencil);
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
            draw_highlight(painter, &xf, list, &prim_index.prims(*list_gen, list, &[h]), t.hover, preview_look);
        }
        if let Some(look) = selection_look {
            if gpu.is_some() && !sel.is_empty() {
                let list_id = *list_gen;
                let hl = (hl_key, &*hl_slot, hl_origin, &mut *hl_infinite);
                draw_highlight_gpu(
                    painter,
                    &xf,
                    list,
                    list_id,
                    || prim_index.prims(list_id, list, sel),
                    sel,
                    t.selection,
                    look,
                    hl,
                    crate::gpu::Stencil::Off,
                );
                draw_highlight(painter, &xf, list, hl_infinite, t.selection, look);
            } else {
                draw_highlight(painter, &xf, list, &prim_index.prims(*list_gen, list, sel), t.selection, look);
            }
        }
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
    // UCS icon, ViewCube and viewport label.
    if app.ui.show_ucs_icon {
        draw_ucs_icon(painter, rect);
        crate::viewcube::ucs_icon(app, ui, rect);
    }
    if app.ui.show_viewcube && sheet.is_none() {
        draw_viewcube(app, ui, rect);
    }
    viewport_label(painter, rect);
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

fn handle_double_click(app: &mut CadApp, ctx: DoubleClickContext<'_>) -> Option<bool> {
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
            app.canvas.hot_grips.clear();
            app.session.pending_window = None;
            app.canvas.drag_window = false;
            if app.run("mspace", serde_json::json!({"at": [pp.x, pp.y]})).is_err() && active_viewport.is_some() {
                let _ = app.run("pspace", serde_json::json!({}));
            }
            app.session.pending_window = None;
            app.canvas.drag_window = false;
            app.canvas.list = None;
            return None;
        }
    }
    // Double-click text to edit it, as in AutoCAD: TEXTEDIT with that object already picked.
    if inside
        && double
        && app.session.running.is_none()
        && let Some(hp) = hover
        && edit_text_at(app, xf.to_world(hp))
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
        && let Some(h) = hatch_at(app, xf.to_world(hp), scale)
    {
        app.session.set_selection(vec![h]);
        crate::hatchdlg::open_edit(app);
        hatch_dialog = true;
    }

    Some(hatch_dialog)
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

fn update_hover(app: &mut CadApp, ui: &egui::Ui, inside: bool, hover_pos: Option<Pos2>, raw_world: Option<Vec2>, scale: f64) {
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
    let xf = Xf { rect, center: view.center, scale };
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
    let Some(hatch_dialog) = handle_double_click(
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

    update_hover(app, ui, inside, hover_pos, raw_world, scale);

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
    let selecting = app.session.previews_selection();
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
    let sel = app.session.selection();
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
            painter.extend(dashed_rect(painter.clip_rect(), r, st, 5.0, 3.0));
        } else {
            painter.rect_stroke(r, 0.0, st, egui::StrokeKind::Middle);
        }
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
mod rebuild;
