//! Window chrome: title and tool bar, file tabs, status bar and the Start page.

use egui::{Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

pub mod annoscale;

const TOOLBAR: &[&[(Icon, &str, &str, Option<&str>)]] = &[
    &[
        (Icon::New, "new", "New Drawing", Some("Cmd+N")),
        (Icon::Open, "open", "Open", Some("Cmd+O")),
        (Icon::Save, "qsave", "Save", Some("Cmd+S")),
        (Icon::SaveAs, "saveas", "Save As", None),
    ],
    &[(Icon::Undo, "u", "Undo", Some("Cmd+Z")), (Icon::Redo, "redo", "Redo", Some("Cmd+Shift+Z"))],
    &[
        (Icon::Plot, "plot", "Print", None),
        (Icon::Publish, "publish", "Batch Publish", None),
        (Icon::PageSetup, "pagesetup", "Page Setup Manager", None),
        (Icon::Preview, "preview", "Plot Preview", None),
    ],
    &[
        (Icon::Import, "import", "Import", None),
        (Icon::Export, "export", "Export", None),
        (Icon::Attach, "attach", "Attach", None),
        (Icon::Share, "share", "Share", None),
    ],
    &[(Icon::ZoomWindow, "zoom.window", "Zoom Window", None), (Icon::Pan, "pan", "Pan", None), (Icon::Orbit, "3dorbit", "Orbit", None)],
    &[
        (Icon::ZoomExtents, "zoom.extents", "Zoom Extents", None),
        (Icon::Properties, "ui.toggle.palettes", "Properties", None),
        (Icon::Layers, "ui.dialog.layers", "Layer Properties Manager", None),
        (Icon::Blocks, "ui.dialog.blocks", "Blocks", None),
    ],
    &[(Icon::Measure, "dist", "Measure Distance", None), (Icon::List, "list", "List", None), (Icon::Area, "area", "Area", None)],
    &[(Icon::Help, "ui.dialog.about", "Help", None)],
];

/// Height of the macOS integrated title row (#161). AppKit draws the traffic lights itself,
/// centred in the standard 28 pt title-bar band at the top of the window; moving them would take
/// AppKit calls (`unsafe`, which the workspace forbids). So the title row is that band, and what
/// it holds is centred on the same line as the lights.
const MAC_TITLE_BAR: f32 = 28.0;
/// The traffic lights as AppKit lays them out: button diameter, centre-to-centre pitch and the
/// first button's left edge (8–10 pt depending on the macOS version; the larger value keeps the
/// gap after them from shrinking).
const MAC_LIGHT: f32 = 12.0;
const MAC_LIGHT_PITCH: f32 = 20.0;
const MAC_LIGHTS_LEFT: f32 = 10.0;
/// Where title-row content may start to the right of the traffic lights: the end of the three
/// lights plus the same gap the lights keep from the top of the window.
const MAC_CONTENT_LEFT: f32 = MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT + (MAC_TITLE_BAR - MAC_LIGHT) / 2.0;

/// Left edge of the window title in a title row starting at `left`, `width` wide: centred, but
/// never under the traffic lights (a long drawing name in a narrow window slides right instead).
fn title_left(left: f32, width: f32, text_w: f32) -> f32 {
    (left + (width - text_w) / 2.0).max(left + MAC_CONTENT_LEFT)
}

/// Title row (with the integrated macOS title bar) and the Tool Bar.
pub fn title_and_toolbar(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let title_h = if app.integrated_titlebar { MAC_TITLE_BAR } else { 0.0 };
    let tb_h = if app.ui.show_toolbar { 34.0 } else { 0.0 };
    egui::Panel::top("cc_title_toolbar").exact_size(title_h + tb_h).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        let r = ui.max_rect();
        if title_h > 0.0 {
            // The empty row moves the window; it goes first so later widgets take their own clicks.
            craft_window::title_bar::empty_area(ui, Rect::from_min_size(r.min, vec2(r.width(), title_h)));
            let title = match app.session.state() {
                Ok(st) if !app.ui.start_tab => format!("CADCraft      {}{}", st.title, if st.title.contains('.') { "" } else { ".dwg" }),
                _ => format!("CADCraft      {}", crate::tl!("Start")),
            };
            // Centred on the traffic lights' centre line (the row's middle): with the system font
            // the capitals' middle is the text box's middle, so no optical nudge.
            let galley = ui.painter().layout_no_wrap(title, egui::FontId::proportional(13.0), t.text_dim);
            let x = title_left(r.left(), r.width(), galley.size().x);
            ui.painter().galley(pos2(x, r.top() + (title_h - galley.size().y) / 2.0), galley, t.text_dim);
        }
        if tb_h > 0.0 {
            let row = Rect::from_min_size(pos2(r.left(), r.top() + title_h), vec2(r.width(), tb_h));
            let mut x = r.left() + if app.integrated_titlebar { 120.0 } else { 12.0 };
            let size = 26.0;
            let mut clicked = None;
            let mac = ui.ctx().os().is_mac();
            let widths: Vec<_> = TOOLBAR.iter().map(|group| group.len() as f32 * (size + 6.0) - 6.0).collect();
            let available = (row.right() - 12.0 - x).max(0.0);
            let Some(fit) = craft_ui::layout::fit_toolbar(&widths, available, 24.0, 30.0) else {
                // Very small windows still expose the complete command row by scrolling.
                ui.scope_builder(egui::UiBuilder::new().max_rect(row.shrink2(vec2(4.0, 0.0))), |ui| {
                    egui::ScrollArea::horizontal().id_salt("toolbar_small").show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for group in TOOLBAR {
                                for (icon, cmd, tip, _) in *group {
                                    if icons::button(ui, *icon, size, tip, false).clicked() {
                                        clicked = Some(*cmd);
                                    }
                                }
                                ui.add_space(18.0);
                            }
                        });
                    });
                });
                if let Some(command) = clicked {
                    app.start(command);
                }
                return;
            };
            for (index, _) in &fit.shown {
                let Some(group) = TOOLBAR.get(*index) else { continue };
                for (icon, cmd, label, shortcut) in group.iter() {
                    let br = Rect::from_min_size(pos2(x, row.center().y - size / 2.0), vec2(size, size));
                    let resp = ui.interact(br, ui.scope_id().with(("tb", *cmd)), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(br, 3.0, t.control_hover.gamma_multiply(0.6));
                    }
                    icons::paint(ui.painter(), br.shrink(4.0), *icon, false);
                    let tip = match shortcut {
                        Some(sc) => format!("{} ({})", crate::i18n::t(label), crate::menus::shortcut_label(sc, mac)),
                        None => crate::i18n::t(label).to_string(),
                    };
                    icons::describe_control(ui, &resp, &tip, None, false);
                    if resp.on_hover_text(tip).clicked() {
                        clicked = Some(*cmd);
                    }
                    x += size + 6.0;
                }
                x += 18.0;
            }
            if !fit.overflow.is_empty() {
                let br = Rect::from_min_size(pos2(x, row.center().y - size / 2.0), vec2(fit.overflow_width, size));
                let response = ui.interact(br, ui.scope_id().with("toolbar_more"), Sense::click());
                if response.hovered() {
                    ui.painter().rect_filled(br, 3.0, t.control_hover.gamma_multiply(0.6));
                }
                for offset in [-5.0, 0.0, 5.0] {
                    ui.painter().circle_filled(br.center() + vec2(offset, 0.0), 1.5, t.text);
                }
                icons::describe_control(ui, &response, "More toolbar commands", None, false);
                egui::Popup::menu(&response).show(|ui| {
                    for (position, index) in fit.overflow.iter().enumerate() {
                        let Some(group) = TOOLBAR.get(*index) else { continue };
                        if position > 0 {
                            ui.separator();
                        }
                        for (_, command, label, _) in *group {
                            let response = ui.button(crate::i18n::t(label));
                            crate::control::record_widget(ui, &response, label, "button", None);
                            if response.clicked() {
                                clicked = Some(*command);
                                ui.close();
                            }
                        }
                    }
                });
                response.on_hover_text(crate::tl!("More toolbar commands"));
            }
            if let Some(c) = clicked {
                app.start(c);
            }
        }
    });
}

/// File tabs: switcher, "+", Start, open drawings.
pub fn file_tabs(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    egui::Panel::top("cc_file_tabs").exact_size(26.0).frame(egui::Frame::NONE.fill(t.chrome_dark)).show(ui, |ui| {
        let r = ui.max_rect();
        let mut x = r.left() + 4.0;
        let ib = Rect::from_min_size(pos2(x, r.top() + 4.0), vec2(18.0, 18.0));
        icons::paint(ui.painter(), ib.shrink(1.0), Icon::Switcher, false);
        x += 24.0;
        let plus = Rect::from_min_size(pos2(x, r.top() + 4.0), vec2(18.0, 18.0));
        let presp = ui.interact(plus, ui.scope_id().with("tab+"), Sense::click());
        icons::paint(ui.painter(), plus.shrink(2.0), Icon::Plus, false);
        icons::describe_control(ui, &presp, "New Drawing", None, false);
        if presp.on_hover_text(crate::tl!("New Drawing")).clicked() {
            app.start("new");
            app.ui.start_tab = false;
        }
        x += 24.0;
        // `label` is the drawing's name; `dirty` adds the unsaved-changes mark, kept visible when a
        // long name is cut short.
        let mut tab =
            |ui: &mut egui::Ui, label: &str, dirty: bool, active: bool, closable: bool, w: f32, id: egui::Id| -> (bool, bool, egui::Response) {
                let tr = Rect::from_min_size(pos2(x, r.top()), vec2(w, r.height()));
                let resp = ui.interact(tr, id, Sense::click());
                // The close button takes the tab's hover, so `resp.hovered()` would make the tab and
                // its X flicker while the pointer is on the X.
                let hovered = ui.rect_contains_pointer(tr);
                ui.painter().rect_filled(
                    tr,
                    0.0,
                    if active {
                        t.tab_active
                    } else if hovered {
                        t.chrome
                    } else {
                        t.chrome_dark
                    },
                );
                // The name ends in "…" when it doesn't fit before the close button (#513).
                let color = if active { t.text } else { t.text_dim };
                let mark = if dirty { "*" } else { "" };
                let text_w = (w - 8.0 - if closable { 24.0 } else { 8.0 }).max(0.0);
                let mark_w = ui.painter().layout_no_wrap(mark.to_string(), crate::theme::body(), color).size().x;
                let (galley, cut) = elided(ui.painter(), label, crate::theme::body(), color, text_w - mark_w);
                let pos = pos2(tr.left() + 8.0, tr.center().y - galley.size().y / 2.0);
                let name_w = galley.size().x;
                ui.painter().galley(pos, galley, color);
                if dirty {
                    ui.painter().text(pos2(pos.x + name_w, tr.center().y), egui::Align2::LEFT_CENTER, mark, crate::theme::body(), color);
                }
                let resp = if cut { resp.on_hover_text(format!("{label}{mark}")) } else { resp };
                let mut close = false;
                if closable && (active || hovered) {
                    let cr = Rect::from_center_size(pos2(tr.right() - 11.0, tr.center().y), vec2(12.0, 12.0));
                    let cresp = ui.interact(cr, id.with("x"), Sense::click());
                    icons::paint(ui.painter(), cr, Icon::Close, false);
                    icons::describe_control(ui, &cresp, &format!("Close {label}"), None, false);
                    close = cresp.clicked();
                }
                icons::describe_control(ui, &resp, label, Some(active), true);
                ui.painter().vline(tr.right(), tr.y_range(), Stroke::new(1.0, t.border));
                x += w;
                (resp.clicked(), close, resp)
            };
        let (c, _, _) =
            tab(ui, crate::tl!("Start"), false, app.ui.start_tab || app.session.docs.is_empty(), false, 110.0, ui.scope_id().with("tab_start"));
        if c {
            app.ui.start_tab = true;
        }
        let mut switch_to = None;
        let mut close = None;
        let titles: Vec<(usize, String, bool, bool)> =
            app.session.docs.iter().enumerate().map(|(i, d)| (i, d.title.clone(), d.is_dirty(), i == app.session.active)).collect();
        for (i, title, dirty, active) in titles {
            let (c, x, resp) = tab(ui, &title, dirty, active && !app.ui.start_tab, true, 170.0, ui.scope_id().with(("tab", i)));
            crate::shortcut_menus::attach(app, &resp, |app| crate::shortcut_menus::drawing_tab_entries(app, i));
            if c {
                switch_to = Some(i);
            }
            if x {
                close = Some(i);
            }
        }
        if let Some(i) = switch_to {
            let _ = app.run("document.switch", json!({ "index": i }));
            app.ui.start_tab = false;
        }
        if let Some(i) = close {
            app.request_close(Some(i));
        }
    });
}

/// `text` laid out on one line no wider than `max_width`, ending in "…" when it had to be cut;
/// and whether it was.
fn elided(p: &egui::Painter, text: &str, font: egui::FontId, color: egui::Color32, max_width: f32) -> (std::sync::Arc<egui::Galley>, bool) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = egui::text::TextWrapping { max_width: max_width.max(0.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = p.layout_job(job);
    let cut = galley.elided;
    (galley, cut)
}

/// Widest layout tab name before it is cut with "…".
const LAYOUT_TAB_TEXT: f32 = 150.0;
/// Room the layout tabs leave for the coordinates and messages.
const STATUS_READOUT_MIN: f32 = 150.0;

/// Which of the tabs with these `widths` to show in `room`: all of them when they fit, else a run
/// that holds the `active` one (shown even when it alone is too wide), starting as early as it can.
fn visible_tabs(widths: &[f32], active: Option<usize>, room: f32) -> std::ops::Range<usize> {
    let fits = |range: std::ops::Range<usize>| widths.get(range).is_some_and(|w| w.iter().sum::<f32>() <= room);
    let mut end = 0;
    while end < widths.len() && fits(0..end + 1) {
        end += 1;
    }
    let Some(active) = active.filter(|a| *a >= end && *a < widths.len()) else { return 0..end };
    let mut start = active;
    while start > 0 && fits(start - 1..active + 1) {
        start -= 1;
    }
    start..active + 1
}

fn draw_status_toggles(
    app: &mut CadApp,
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    t: Tokens,
    toggles: &[(Icon, bool, &str, &str)],
    layout: (Rect, f32, f32),
) {
    let (rect, mut rx, size) = layout;
    let mut toggle_cmd = None;
    for &(icon, on, cmd, tip) in toggles {
        let br = Rect::from_min_size(pos2(rx, rect.center().y - size / 2.0), vec2(size, size));
        let resp = ui.interact(br, ui.scope_id().with(("tg", tip)), Sense::click());
        if on {
            painter.rect_filled(br, 3.0, t.toggle_on.gamma_multiply(0.35));
        } else if resp.hovered() {
            painter.rect_filled(br, 3.0, t.control_hover.gamma_multiply(0.5));
        }
        icons::paint(painter, br.shrink(2.5), icon, false);
        if on {
            // Re-tint the icon blue-ish by an underline.
            painter.hline(br.x_range().shrink(4.0), br.bottom() - 1.0, Stroke::new(1.5, t.toggle_on));
        }
        icons::describe_control(ui, &resp, tip, Some(on), false);
        if resp.on_hover_text(crate::i18n::t(tip)).clicked() {
            toggle_cmd = Some(cmd);
        }
        rx += size + 4.0;
    }
    // The toggles don't cancel the active command (#1108); the dialog buttons start theirs.
    match toggle_cmd {
        Some(c) if c.starts_with("ui.") => app.start(c),
        Some(c) => crate::menus::run_transparent(app, c),
        None => {}
    }
}

/// Status bar: layout tabs follow the left-hand layout actions, independent of palette docking.
/// Coordinates and drafting toggles remain on the right.
pub fn status_bar(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    egui::Panel::bottom("cc_status").exact_size(26.0).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        let r = ui.max_rect();
        let p = ui.painter().clone();
        let mut x = r.left() + 8.0;
        // Clear the status message on time even when no input arrives (the app only repaints on input).
        if let Some((_, at)) = &app.status
            && crate::now_ms() - at < 5000.0
        {
            let left = std::time::Duration::try_from_secs_f64((5000.0 - (crate::now_ms() - at)) / 1000.0).unwrap_or_default();
            ui.ctx().request_repaint_after(left);
        }
        let mut list_button = None;
        for (icon, tip) in [(Icon::Plus, "New layout"), (Icon::Menu, "Layout list")] {
            let br = Rect::from_min_size(pos2(x, r.top() + 4.0), vec2(18.0, 18.0));
            let resp = ui.interact(br, ui.scope_id().with(("sb", tip)), Sense::click());
            icons::paint(&p, br.shrink(2.0), icon, false);
            icons::describe_control(ui, &resp, tip, None, false);
            let resp = resp.on_hover_text(crate::i18n::t(tip));
            if icon == Icon::Plus {
                if resp.clicked() {
                    crate::shortcut_menus::new_layout(app);
                }
            } else {
                list_button = Some(resp);
            }
            x += 22.0;
        }
        x += 8.0;
        // Right side: toggles, right-aligned.
        let s = app.session.settings.clone();
        let anno_all = app.session.doc().is_ok_and(|d| d.anno_all_visible());
        use cadcraft_engine::snap::mode;
        let toggles: Vec<(Icon, bool, &str, &str)> = vec![
            (Icon::Grid, s.gridmode, "grid", "Grid (F7)"),
            (Icon::Snap, s.snapmode, "snap", "Snap Mode (F9)"),
            (Icon::Ortho, s.orthomode, "ortho", "Ortho Mode (F8)"),
            (Icon::Polar, s.polarmode, "polar", "Polar Tracking (F10)"),
            (Icon::Isodraft, s.isodraft, "isodraft", "Isometric Drafting"),
            (Icon::OTrack, s.otrack, "otrack", "Object Snap Tracking (F11)"),
            (Icon::Osnap, s.osmode & mode::OFF == 0 && s.osmode != 0, "osnap", "Object Snap (F3)"),
            (Icon::Lineweight, s.lwdisplay, "lwdisplay", "Show/Hide Lineweight"),
            (Icon::Transparency, s.transparency_display, "transparencydisplay", "Show/Hide Transparency"),
            (Icon::DynInput, s.dynmode, "dynmode", "Dynamic Input (F12)"),
            (Icon::QuickProps, s.qpmode, "qpmode", "Quick Properties"),
            (Icon::Annotation, anno_all, "annovisible.toggle", "Annotation Visibility"),
            (Icon::Workspace, false, "ui.noop", "Workspace Switching"),
            (Icon::Gear, false, "ui.dialog.dsettings", "Drafting Settings"),
        ];
        let size = 20.0;
        let rx = r.right() - 8.0 - toggles.len() as f32 * (size + 4.0);
        // Coordinates.
        let coord = app.canvas.cursor.map(|c| {
            let (lu, lp) = app.session.doc().map(|d| (d.header.i64("LUNITS", 2), d.header.i64("LUPREC", 4))).unwrap_or((2, 4));
            format!(
                "{}, {}, {}",
                cadcraft_engine::units::format_distance(c.x, lu, lp),
                cadcraft_engine::units::format_distance(c.y, lu, lp),
                cadcraft_engine::units::format_distance(0.0, lu, lp)
            )
        });
        // MODEL / PAPER (layouts only): toggles MSPACE and PSPACE like AutoCAD's status button.
        let mut coord_right = rx - 8.0;
        // The annotation scale list (model space).
        coord_right = annoscale::status(app, ui, coord_right, r.center().y);
        if matches!(app.session.layout_space(), cadcraft_doc::Space::Paper(_)) {
            let model = app.session.state().is_ok_and(|st| st.mspace.is_some());
            let label = if model { "MODEL" } else { "PAPER" };
            let br = Rect::from_min_max(pos2(rx - 60.0, r.center().y - 9.0), pos2(rx - 6.0, r.center().y + 9.0));
            let resp = ui.interact(br, ui.scope_id().with("mspace_toggle"), Sense::click());
            p.rect_filled(br, 3.0, if resp.hovered() { t.control_hover } else { t.toggle_on.gamma_multiply(0.25) });
            p.text(br.center(), egui::Align2::CENTER_CENTER, crate::i18n::t(label), crate::theme::small(), t.text);
            icons::describe_control(ui, &resp, label, None, false);
            if resp.on_hover_text(crate::tl!("Switch between model space in a viewport and paper space")).clicked() {
                let _ = app.run(if model { "pspace" } else { "mspace" }, json!({}));
                app.canvas.list = None;
            }
            coord_right = br.left() - 10.0;
            // Scale and lock of the active or selected viewport.
            coord_right = crate::vpscale::status(app, ui, coord_right + 4.0, r.center().y) - 10.0;
        }
        // Layout tabs: Model, "+", then the layouts that fit before the right-hand controls (the
        // current one always); the layout list reaches the rest.
        let mut tabs: Vec<String> = vec!["Model".into()];
        let cur = app.session.state().map(|s| match &s.space {
            cadcraft_doc::Space::Model => "Model".to_string(),
            cadcraft_doc::Space::Paper(n) => n.clone(),
        });
        if let Ok(st) = app.session.state() {
            let mut ls: Vec<_> = st.doc.layouts.iter().collect();
            ls.sort_by_key(|l| l.tab_order);
            tabs.extend(ls.iter().map(|l| l.name.clone()));
        }
        let shown_name = |i: usize, name: &str| if i == 0 { crate::tl!("Model").to_string() } else { name.to_string() };
        let limit = (coord_right - STATUS_READOUT_MIN).max(x);
        let labels: Vec<_> =
            tabs.iter().enumerate().map(|(i, n)| elided(&p, &shown_name(i, n), crate::theme::body(), t.text, LAYOUT_TAB_TEXT)).collect();
        let widths: Vec<f32> = labels.iter().map(|(g, _)| g.size().x + 26.0 + 2.0).collect();
        let active_tab = tabs.iter().position(|n| cur.as_deref().is_ok_and(|c| c == n));
        // Model and "+" stay; the layouts share what is left.
        let layouts = widths.get(1..).unwrap_or_default();
        let room = limit - x - widths.first().copied().unwrap_or(0.0) - 22.0;
        let active_layout = active_tab.and_then(|a| a.checked_sub(1));
        let mut shown = visible_tabs(layouts, active_layout, room);
        let hidden = shown.len() < layouts.len();
        if hidden {
            shown = visible_tabs(layouts, active_layout, room - 22.0);
        }
        let clip = Rect::from_min_max(r.min, pos2(limit.max(r.left()), r.bottom()));
        let tp = p.with_clip_rect(clip);
        let mut switch = None;
        for (i, (name, (g, cut))) in tabs.iter().zip(labels).enumerate() {
            if i > 0 && !shown.contains(&(i - 1)) {
                continue;
            }
            let w = g.size().x + 26.0;
            let tr = Rect::from_min_size(pos2(x, r.top() + 1.0), vec2(w, r.height() - 2.0)).intersect(clip);
            let active = active_tab == Some(i);
            let resp = ui.interact(tr, ui.scope_id().with(("lt", i)), Sense::click());
            if active {
                tp.rect_filled(tr, 2.0, t.tab_active);
            } else if resp.hovered() {
                tp.rect_filled(tr, 2.0, t.control.gamma_multiply(0.5));
            }
            tp.galley(pos2(tr.left() + 13.0, tr.center().y - g.size().y / 2.0), g, if active { t.text } else { t.text_dim });
            icons::describe_control(ui, &resp, name, Some(active), true);
            let resp = if cut { resp.on_hover_text(shown_name(i, name)) } else { resp };
            if resp.clicked() {
                switch = Some(name.clone());
            }
            crate::shortcut_menus::attach(app, &resp, |app| crate::shortcut_menus::layout_tab_entries(app, name));
            x += w + 2.0;
            if i == 0 {
                let pr = Rect::from_min_size(pos2(x, r.top() + 5.0), vec2(16.0, 16.0));
                let presp = ui.interact(pr, ui.scope_id().with("layout_plus"), Sense::click());
                icons::paint(&p, pr.shrink(2.0), Icon::Plus, false);
                icons::describe_control(ui, &presp, "New layout", None, false);
                if presp.on_hover_text(crate::tl!("New layout")).clicked() {
                    crate::shortcut_menus::new_layout(app);
                }
                x += 22.0;
            }
        }
        // The layouts that didn't fit, behind a button after the last tab shown.
        let more = hidden.then(|| {
            let mr = Rect::from_min_size(pos2(x + 2.0, r.top() + 5.0), vec2(16.0, 16.0));
            let resp = ui.interact(mr, ui.scope_id().with("layout_more"), Sense::click());
            if resp.hovered() {
                p.rect_filled(mr, 3.0, t.control_hover.gamma_multiply(0.5));
            }
            icons::paint(&p, mr.shrink(2.0), Icon::ChevronDown, false);
            icons::describe_control(ui, &resp, "Layout list", None, false);
            x += 22.0;
            resp.on_hover_text(crate::tl!("Layout list"))
        });
        for resp in list_button.iter().chain(&more) {
            egui::Popup::from_toggle_button_response(resp).width(200.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for (i, name) in tabs.iter().enumerate() {
                        if ui.add(egui::Button::selectable(active_tab == Some(i), shown_name(i, name)).truncate()).clicked() {
                            switch = Some(name.clone());
                            ui.close();
                        }
                    }
                });
            });
        }
        if let Some(n) = switch {
            let _ = app.run("layout.set", json!({ "name": n }));
        }
        // A transient message occupies the same bounded readout slot as coordinates.
        // Ellipsis and a tooltip keep either value readable without covering layout tabs.
        let message = app.status.as_ref().filter(|(_, at)| crate::now_ms() - at < 5000.0).map(|(message, _)| message);
        let readout = message.cloned().or(coord);
        if let Some(text) = readout {
            let left = x + 8.0;
            let width = (coord_right - left).max(0.0);
            if width > 4.0 {
                let color = if message.is_some() { t.warn } else { t.text };
                let mut job = egui::text::LayoutJob::simple_singleline(text.clone(), crate::theme::body(), color);
                job.wrap.max_width = width;
                job.wrap.max_rows = 1;
                job.wrap.break_anywhere = true;
                let galley = p.layout_job(job);
                let rect = Rect::from_min_max(pos2(left, r.top()), pos2(coord_right, r.bottom()));
                let text_x = if message.is_some() { left } else { coord_right - galley.size().x };
                p.with_clip_rect(rect).galley(pos2(text_x, r.center().y - galley.size().y / 2.0), galley, color);
                ui.interact(rect, ui.scope_id().with("status_readout"), Sense::hover()).on_hover_text(text);
            }
        }
        draw_status_toggles(app, ui, &p, t, &toggles, (r, rx, size));
    });
}

/// The Start page (no drawing open, or the Start tab).
pub fn start_page(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let r = ui.max_rect();
    let p = ui.painter().clone();
    let left = Rect::from_min_size(r.min, vec2(300.0, r.height()));
    p.rect_filled(left, 0.0, t.chrome_dark);
    p.rect_filled(Rect::from_min_max(pos2(left.right(), r.top()), r.max), 0.0, t.chrome);
    p.text(pos2(left.left() + 34.0, left.top() + 70.0), egui::Align2::LEFT_CENTER, crate::tl!("CADCraft"), egui::FontId::proportional(28.0), t.text);
    p.text(
        pos2(left.left() + 34.0, left.top() + 98.0),
        egui::Align2::LEFT_CENTER,
        crate::tf!("Version {version}", version = env!("CARGO_PKG_VERSION")),
        crate::theme::small(),
        t.text_faint,
    );
    let mut y = left.top() + 140.0;
    let mut action = None;
    for (label, cmd) in [("Open...", "open"), ("New", "new"), ("New (metric)", "new.metric"), ("Open sample drawing", "sample")] {
        let br = Rect::from_min_size(pos2(left.left() + 34.0, y), vec2(220.0, 34.0));
        let resp = ui.interact(br, ui.scope_id().with(("start", cmd)), Sense::click());
        p.rect_stroke(br, 2.0, Stroke::new(1.0, if resp.hovered() { t.text } else { t.text_dim }), egui::StrokeKind::Inside);
        p.text(pos2(br.left() + 16.0, br.center().y), egui::Align2::LEFT_CENTER, crate::i18n::t(label), crate::theme::body(), t.text);
        if resp.clicked() {
            action = Some(cmd);
        }
        y += 44.0;
    }
    let main = Rect::from_min_max(pos2(left.right() + 36.0, r.top() + 40.0), r.max);
    p.text(main.min, egui::Align2::LEFT_TOP, crate::tl!("Recent"), egui::FontId::proportional(22.0), t.text);
    let mut recent = None;
    let mut y = main.top() + 44.0;
    for (i, path) in app.ui.recent_files.iter().enumerate() {
        let row = Rect::from_min_size(pos2(main.left(), y), vec2((r.right() - main.left() - 36.0).clamp(0.0, 640.0), 42.0));
        let resp = ui.interact(row, ui.scope_id().with(("recent", i)), Sense::click());
        if resp.hovered() {
            p.rect_filled(row, 2.0, t.control_hover.gamma_multiply(0.5));
        }
        let dir = std::path::Path::new(path).parent().map(|d| d.display().to_string()).unwrap_or_default();
        let clip = p.with_clip_rect(row.shrink(2.0));
        clip.text(pos2(row.left() + 10.0, row.top() + 13.0), egui::Align2::LEFT_CENTER, crate::menus::file_name(path), crate::theme::body(), t.text);
        clip.text(pos2(row.left() + 10.0, row.top() + 30.0), egui::Align2::LEFT_CENTER, dir, crate::theme::small(), t.text_faint);
        if resp.on_hover_text(path).clicked() {
            recent = Some(path.clone());
        }
        y += 46.0;
    }
    if let Some(path) = recent {
        app.open_path(std::path::Path::new(&path));
    } else if app.ui.recent_files.is_empty() {
        p.text(
            pos2(main.center().x, main.center().y),
            egui::Align2::CENTER_CENTER,
            crate::tl!("There is nothing here yet."),
            egui::FontId::proportional(18.0),
            t.text,
        );
        p.text(
            pos2(main.center().x, main.center().y + 26.0),
            egui::Align2::CENTER_CENTER,
            crate::tl!("To get started, create or open a drawing."),
            crate::theme::body(),
            t.text_dim,
        );
    }
    match action {
        Some("open") => app.start("ui.open"),
        Some("new") => {
            app.start("new");
            app.ui.start_tab = false;
        }
        Some("new.metric") => {
            let _ = app.run("new", json!({ "metric": true }));
            app.ui.start_tab = false;
        }
        Some("sample") => {
            app.start("ui.sample");
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_title_row_centres_and_clears_the_traffic_lights() {
        // The lights are centred in the row, and content keeps the same gap after them.
        let margin = (MAC_TITLE_BAR - MAC_LIGHT) / 2.0;
        let lights_end = MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT;
        assert_eq!(MAC_CONTENT_LEFT - lights_end, margin);
        // A short title is centred; one too wide for that starts after the lights.
        assert_eq!(title_left(100.0, 1000.0, 200.0), 500.0);
        assert_eq!(title_left(100.0, 900.0, 800.0), 100.0 + MAC_CONTENT_LEFT);
    }

    #[test]
    fn layout_tabs_that_do_not_fit_keep_the_current_one() {
        let w = [50.0; 6];
        // All fit, with or without a current tab.
        assert_eq!(visible_tabs(&w, None, 300.0), 0..6);
        assert_eq!(visible_tabs(&w, Some(5), 300.0), 0..6);
        // Too many: the first ones, unless the current one is further on.
        assert_eq!(visible_tabs(&w, None, 170.0), 0..3);
        assert_eq!(visible_tabs(&w, Some(1), 170.0), 0..3);
        assert_eq!(visible_tabs(&w, Some(4), 170.0), 2..5);
        // No room at all: nothing but the current tab; hostile numbers don't panic.
        assert_eq!(visible_tabs(&w, None, -10.0), 0..0);
        assert_eq!(visible_tabs(&w, Some(3), 0.0), 3..4);
        assert_eq!(visible_tabs(&w, Some(9), f32::NAN), 0..0);
        assert_eq!(visible_tabs(&[], Some(0), 100.0), 0..0);
    }
}
