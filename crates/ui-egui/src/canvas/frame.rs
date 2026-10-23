//! One frame of the canvas, input side: what [`show`] sets up and every phase reads ([`Frame`]),
//! navigation (pan, zoom, tiles) and clicks (prompts, picks, grips, menus, hover).

use std::ops::ControlFlow;

use super::*;

/// What crosses the phases of [`show`]: the drawing area, its transforms and this frame's input.
pub(super) struct Frame {
    pub(super) t: Tokens,
    /// The whole canvas, tiles included.
    pub(super) full: Rect,
    pub(super) tiles: Vec<(Rect, cadcraft_engine::ModelTile)>,
    /// The drawing area (the current tile).
    pub(super) rect: Rect,
    pub(super) resp: egui::Response,
    pub(super) painter: egui::Painter,
    pub(super) view: cadcraft_engine::View,
    pub(super) scale: f64,
    pub(super) xf: Xf,
    /// The paper (display) transform; equal to `xf` except inside an MSPACE viewport.
    pub(super) xf_paper: Xf,
    pub(super) active_vp: Option<cadcraft_doc::Viewport>,
    /// A clipped active viewport's outline (picks and its heavy border follow it).
    pub(super) active_clip: Option<cadcraft_render::clip::PolyClip>,
    pub(super) hover_pos: Option<Pos2>,
    pub(super) scroll: egui::Vec2,
    pub(super) mods: egui::Modifiers,
    pub(super) middle_down: bool,
    pub(super) pressed_primary: bool,
    pub(super) pressed_secondary: bool,
    pub(super) dbl_middle: bool,
    pub(super) dbl_primary: bool,
    pub(super) inside: bool,
    pub(super) raw_world: Option<Vec2>,
    pub(super) selecting: bool,
}

impl Frame {
    /// The drawing area, its view and this frame's input; `None` without a drawing.
    pub(super) fn setup(app: &mut CadApp, ui: &mut egui::Ui) -> Option<Frame> {
        let t = Tokens::get();
        let full = ui.available_rect_before_wrap();
        // Tiled model viewports (VPORTS): the current tile is the drawing area below; `draw_tiles`
        // draws the others.
        let tiles = tile_rects(app, full);
        let rect = tiles.iter().find(|(_, tile)| tile.active).map_or(full, |(r, _)| *r);
        let resp = ui.interact(full, ui.id().with("canvas"), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        app.canvas.rect = Some(rect);
        app.session.viewport_px = (f64::from(rect.width()).max(1.0), f64::from(rect.height()).max(1.0));
        if app.canvas.zoom_pending {
            app.canvas.zoom_pending = false;
            let _ = app.session.zoom_extents();
        }
        let Ok(st) = app.session.state() else { return None };
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
        let inside = hover_pos.is_some_and(|p| rect.contains(p)) && resp.hovered();
        // Whether Ctrl is held right now. Written every frame (not just while a command prompts), so
        // previews that read `Session::ctrl_down` always see the current state. The engine
        // deliberately does not carry modifiers through `Input`, which is UI-agnostic (see PR
        // #407's Limits note: modifiers used to be dropped on the floor at this point).
        app.session.ctrl_down = mods.ctrl;
        Some(Frame {
            t,
            full,
            tiles,
            rect,
            resp,
            painter,
            view,
            scale,
            xf,
            xf_paper: xf,
            active_vp: None,
            active_clip: None,
            hover_pos,
            scroll,
            mods,
            middle_down,
            pressed_primary,
            pressed_secondary,
            dbl_middle,
            dbl_primary,
            inside,
            raw_world: None,
            selecting: false,
        })
    }

    /// Re-read the view after navigation.
    pub(super) fn reread_view(&mut self, app: &mut CadApp) {
        let rect = self.rect;
        self.view = app.session.state().map(|s| s.view()).unwrap_or(self.view);
        self.scale = f64::from(rect.height()) / self.view.height.max(1e-12);
        self.xf = Xf { rect, center: self.view.center, scale: self.scale };
        let xf = self.xf;
        app.canvas.xf = Some(xf);
        // The paper (display) transform; equal to `xf` except inside an MSPACE viewport.
        (self.xf_paper, self.active_vp) = match app.session.state() {
            Ok(st) => {
                let pv = st.paper_view();
                let xp = Xf { rect, center: pv.center, scale: f64::from(rect.height()) / pv.height.max(1e-12) };
                (if st.mspace.is_some() { xp } else { xf }, st.active_viewport().map(|(_, v)| v))
            }
            Err(_) => (xf, None),
        };
        // A clipped active viewport's outline (picks and its heavy border follow it).
        self.active_clip =
            app.session.state().ok().and_then(|st| st.active_viewport().and_then(|(h, v)| cadcraft_render::viewport_clip(&st.doc, h, &v)));
    }
}

/// Scroll, wheel and pinch zoom, middle-button pan, ZOOM Realtime and clicks on other tiles.
/// `Break`: the frame ends here (a click made another tile current).
pub(super) fn navigate(app: &mut CadApp, ui: &mut egui::Ui, frame: &mut Frame) -> ControlFlow<()> {
    let Frame { rect, scale, xf, hover_pos, scroll, middle_down, pressed_primary, dbl_middle, inside, .. } = *frame;
    let resp = frame.resp.clone();
    let tiles = &frame.tiles;
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
        return ControlFlow::Break(());
    }
    ControlFlow::Continue(())
}

/// Double-clicks (viewports, text, hatches), point prompts, the options menu, clicks, the
/// press-and-drag selection window, right-click menus and the hover highlight.
/// `Break`: the frame ends here (a double-click switched spaces or started TEXTEDIT).
pub(super) fn clicks(app: &mut CadApp, ui: &mut egui::Ui, frame: &mut Frame) -> ControlFlow<()> {
    let Frame { rect, scale, xf, xf_paper, hover_pos, mods, middle_down, pressed_primary, pressed_secondary, dbl_primary, inside, .. } = *frame;
    let resp = frame.resp.clone();
    let (active_vp, active_clip) = (&frame.active_vp, &frame.active_clip);
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
            return ControlFlow::Break(());
        }
    }
    // Double-click text to edit it, as in AutoCAD: TEXTEDIT with that object already picked.
    if inside
        && dbl_primary
        && app.session.running.is_none()
        && let Some(hp) = hover_pos
        && edit_text_at(app, xf.to_world(hp))
    {
        return ControlFlow::Break(());
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

    frame.raw_world = hover_pos.filter(|_| inside).map(|p| xf.to_world(p));
    let raw_world = frame.raw_world;
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
    frame.selecting = app.session.previews_selection();
    if inside && frame.selecting && app.session.pending_window.is_none() {
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
    ControlFlow::Continue(())
}
