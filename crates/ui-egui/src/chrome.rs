//! Window chrome: title and tool bar, file tabs, status bar and the Start page.

use egui::{Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

const TOOLBAR: &[&[(Icon, &str, &str)]] = &[
    &[
        (Icon::New, "new", "New Drawing (⌘N)"),
        (Icon::Open, "open", "Open (⌘O)"),
        (Icon::Save, "qsave", "Save (⌘S)"),
        (Icon::SaveAs, "saveas", "Save As"),
    ],
    &[(Icon::Undo, "undo", "Undo (⌘Z)"), (Icon::Redo, "redo", "Redo (⇧⌘Z)")],
    &[
        (Icon::Plot, "plot", "Print"),
        (Icon::Publish, "publish", "Batch Publish"),
        (Icon::PageSetup, "pagesetup", "Page Setup Manager"),
        (Icon::Preview, "preview", "Plot Preview"),
    ],
    &[(Icon::Import, "import", "Import"), (Icon::Export, "export", "Export"), (Icon::Attach, "attach", "Attach"), (Icon::Share, "share", "Share")],
    &[(Icon::ZoomWindow, "zoom.window", "Zoom Window"), (Icon::Pan, "pan", "Pan"), (Icon::Orbit, "3dorbit", "Orbit")],
    &[
        (Icon::ZoomExtents, "zoom.extents", "Zoom Extents"),
        (Icon::Properties, "ui.toggle.palettes", "Properties"),
        (Icon::Layers, "ui.dialog.layers", "Layer Properties Manager"),
        (Icon::Blocks, "ui.dialog.blocks", "Blocks"),
    ],
    &[(Icon::Measure, "dist", "Measure Distance"), (Icon::List, "list", "List"), (Icon::Area, "area", "Area")],
    &[(Icon::Help, "ui.dialog.about", "Help")],
];

/// Title row (with the integrated macOS title bar) and the Tool Bar.
pub fn title_and_toolbar(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let title_h = if app.integrated_titlebar { 28.0 } else { 0.0 };
    let tb_h = if app.ui.show_toolbar { 34.0 } else { 0.0 };
    egui::Panel::top("cc_title_toolbar").exact_size(title_h + tb_h).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        let r = ui.max_rect();
        if title_h > 0.0 {
            let title = match app.session.state() {
                Ok(st) if !app.ui.start_tab => format!("CADCraft      {}{}", st.title, if st.title.contains('.') { "" } else { ".dwg" }),
                _ => "CADCraft      Start".into(),
            };
            ui.painter().text(
                pos2(r.center().x, r.top() + title_h / 2.0 + 1.0),
                egui::Align2::CENTER_CENTER,
                title,
                egui::FontId::proportional(13.0),
                t.text_dim,
            );
            // Allow dragging the window by the title row.
            let tr = Rect::from_min_size(r.min, vec2(r.width(), title_h));
            let resp = ui.interact(tr, ui.id().with("titledrag"), Sense::click_and_drag());
            if resp.drag_started() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if resp.double_clicked() {
                let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
            }
        }
        if tb_h > 0.0 {
            let row = Rect::from_min_size(pos2(r.left(), r.top() + title_h), vec2(r.width(), tb_h));
            let mut x = r.left() + if app.integrated_titlebar { 120.0 } else { 12.0 };
            let size = 26.0;
            let mut clicked = None;
            for group in TOOLBAR {
                for (icon, cmd, tip) in group.iter() {
                    let br = Rect::from_min_size(pos2(x, row.center().y - size / 2.0), vec2(size, size));
                    let resp = ui.interact(br, ui.id().with(("tb", *cmd)), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(br, 3.0, t.control_hover.gamma_multiply(0.6));
                    }
                    icons::paint(ui.painter(), br.shrink(4.0), *icon, false);
                    if resp.on_hover_text(*tip).clicked() {
                        clicked = Some(*cmd);
                    }
                    x += size + 6.0;
                }
                x += 18.0;
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
        let presp = ui.interact(plus, ui.id().with("tab+"), Sense::click());
        icons::paint(ui.painter(), plus.shrink(2.0), Icon::Plus, false);
        if presp.on_hover_text("New Drawing").clicked() {
            app.start("new");
            app.ui.start_tab = false;
        }
        x += 24.0;
        let mut tab = |ui: &mut egui::Ui, label: &str, active: bool, w: f32, id: egui::Id| -> (bool, bool) {
            let tr = Rect::from_min_size(pos2(x, r.top()), vec2(w, r.height()));
            let resp = ui.interact(tr, id, Sense::click());
            ui.painter().rect_filled(
                tr,
                0.0,
                if active {
                    t.tab_active
                } else if resp.hovered() {
                    t.chrome
                } else {
                    t.chrome_dark
                },
            );
            ui.painter().text(
                pos2(tr.left() + 8.0, tr.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                crate::theme::body(),
                if active { t.text } else { t.text_dim },
            );
            let mut close = false;
            if active || resp.hovered() {
                let cr = Rect::from_center_size(pos2(tr.right() - 11.0, tr.center().y), vec2(12.0, 12.0));
                let cresp = ui.interact(cr, id.with("x"), Sense::click());
                icons::paint(ui.painter(), cr, Icon::Close, false);
                close = cresp.clicked();
            }
            ui.painter().vline(tr.right(), tr.y_range(), Stroke::new(1.0, t.border));
            x += w;
            (resp.clicked(), close)
        };
        let (c, _) = tab(ui, "Start", app.ui.start_tab || app.session.docs.is_empty(), 110.0, ui.id().with("tab_start"));
        if c {
            app.ui.start_tab = true;
        }
        let mut switch_to = None;
        let mut close = None;
        let titles: Vec<(usize, String, bool)> = app
            .session
            .docs
            .iter()
            .enumerate()
            .map(|(i, d)| (i, format!("{}{}", d.title, if d.is_dirty() { "*" } else { "" }), i == app.session.active))
            .collect();
        for (i, title, active) in titles {
            let (c, x) = tab(ui, &title, active && !app.ui.start_tab, 170.0, ui.id().with(("tab", i)));
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
            let _ = app.run("close", json!({ "index": i }));
        }
    });
}

/// Status bar: layout tabs on the left, coordinates and drafting toggles on the right.
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
        for (icon, tip) in [(Icon::Plus, "New layout"), (Icon::Menu, "Layout list")] {
            let br = Rect::from_min_size(pos2(x, r.top() + 4.0), vec2(18.0, 18.0));
            let resp = ui.interact(br, ui.id().with(("sb", tip)), Sense::click());
            icons::paint(&p, br.shrink(2.0), icon, false);
            if resp.on_hover_text(tip).clicked() && icon == Icon::Plus {
                let _ = app.run("layout.new", json!({}));
            }
            x += 22.0;
        }
        x = x.max(r.left() + if app.ui.show_toolsets { 232.0 } else { 60.0 });
        // Layout tabs.
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
        let mut switch = None;
        for (i, name) in tabs.iter().enumerate() {
            let g = p.layout_no_wrap(name.clone(), crate::theme::body(), t.text);
            let w = g.size().x + 26.0;
            let tr = Rect::from_min_size(pos2(x, r.top() + 1.0), vec2(w, r.height() - 2.0));
            let active = cur.as_deref().is_ok_and(|c| c == name);
            let resp = ui.interact(tr, ui.id().with(("lt", i)), Sense::click());
            if active {
                p.rect_filled(tr, 2.0, t.tab_active);
            } else if resp.hovered() {
                p.rect_filled(tr, 2.0, t.control.gamma_multiply(0.5));
            }
            p.galley(pos2(tr.left() + 13.0, tr.center().y - g.size().y / 2.0), g, if active { t.text } else { t.text_dim });
            if resp.clicked() {
                switch = Some(name.clone());
            }
            x += w + 2.0;
            if i == 0 {
                let pr = Rect::from_min_size(pos2(x, r.top() + 5.0), vec2(16.0, 16.0));
                let presp = ui.interact(pr, ui.id().with("layout_plus"), Sense::click());
                icons::paint(&p, pr.shrink(2.0), Icon::Plus, false);
                if presp.on_hover_text("New layout").clicked()
                    && let Ok(v) = app.run("layout.new", json!({}))
                    && let Some(n) = v.get("name").and_then(serde_json::Value::as_str)
                {
                    switch = Some(n.to_string());
                }
                x += 22.0;
            }
        }
        if let Some(n) = switch {
            let _ = app.run("layout.set", json!({ "name": n }));
        }
        // Right side: toggles, right-aligned.
        let s = app.session.settings.clone();
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
            (Icon::Annotation, s.annoallvisible, "ui.noop", "Annotation Visibility"),
            (Icon::Workspace, false, "ui.noop", "Workspace Switching"),
            (Icon::Gear, false, "ui.dialog.dsettings", "Drafting Settings"),
        ];
        let size = 20.0;
        let mut rx = r.right() - 8.0 - toggles.len() as f32 * (size + 4.0);
        let coords_w = 210.0;
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
        if matches!(app.session.layout_space(), cadcraft_doc::Space::Paper(_)) {
            let model = app.session.state().is_ok_and(|st| st.mspace.is_some());
            let label = if model { "MODEL" } else { "PAPER" };
            let br = Rect::from_min_max(pos2(rx - 60.0, r.center().y - 9.0), pos2(rx - 6.0, r.center().y + 9.0));
            let resp = ui.interact(br, ui.id().with("mspace_toggle"), Sense::click());
            p.rect_filled(br, 3.0, if resp.hovered() { t.control_hover } else { t.toggle_on.gamma_multiply(0.25) });
            p.text(br.center(), egui::Align2::CENTER_CENTER, label, crate::theme::small(), t.text);
            if resp.on_hover_text("Switch between model space in a viewport and paper space").clicked() {
                let _ = app.run(if model { "pspace" } else { "mspace" }, json!({}));
                app.canvas.list = None;
            }
            coord_right = br.left() - 10.0;
        }
        if let Some(c) = coord {
            p.text(pos2(coord_right, r.center().y), egui::Align2::RIGHT_CENTER, c, crate::theme::body(), t.text);
        }
        let _ = coords_w;
        let mut toggle_cmd = None;
        for (icon, on, cmd, tip) in toggles {
            let br = Rect::from_min_size(pos2(rx, r.center().y - size / 2.0), vec2(size, size));
            let resp = ui.interact(br, ui.id().with(("tg", tip)), Sense::click());
            if on {
                p.rect_filled(br, 3.0, t.toggle_on.gamma_multiply(0.35));
            } else if resp.hovered() {
                p.rect_filled(br, 3.0, t.control_hover.gamma_multiply(0.5));
            }
            icons::paint(&p, br.shrink(2.5), icon, false);
            if on {
                // Re-tint the icon blue-ish by an underline.
                p.hline(br.x_range().shrink(4.0), br.bottom() - 1.0, Stroke::new(1.5, t.toggle_on));
            }
            if resp.on_hover_text(tip).clicked() {
                toggle_cmd = Some(cmd);
            }
            rx += size + 4.0;
        }
        if let Some(c) = toggle_cmd {
            app.start(c);
        }
        // Transient status message.
        if let Some((msg, at)) = &app.status
            && crate::now_ms() - at < 5000.0
        {
            p.text(pos2(x + 20.0, r.center().y), egui::Align2::LEFT_CENTER, msg, crate::theme::small(), t.warn);
        }
    });
}

/// The Start page (no drawing open, or the Start tab).
pub fn start_page(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let r = ui.max_rect();
    let p = ui.painter().clone();
    let left = Rect::from_min_size(r.min, vec2(260.0, r.height()));
    p.rect_filled(left, 0.0, t.chrome_dark);
    p.rect_filled(Rect::from_min_max(pos2(left.right(), r.top()), r.max), 0.0, t.chrome);
    p.text(pos2(left.left() + 34.0, left.top() + 70.0), egui::Align2::LEFT_CENTER, "CADCraft", egui::FontId::proportional(28.0), t.text);
    p.text(
        pos2(left.left() + 34.0, left.top() + 98.0),
        egui::Align2::LEFT_CENTER,
        format!("Version {}", env!("CARGO_PKG_VERSION")),
        crate::theme::small(),
        t.text_faint,
    );
    let mut y = left.top() + 140.0;
    let mut action = None;
    for (label, cmd) in [("Open...", "open"), ("New", "new"), ("New (metric)", "new.metric"), ("Open sample drawing", "sample")] {
        let br = Rect::from_min_size(pos2(left.left() + 34.0, y), vec2(190.0, 34.0));
        let resp = ui.interact(br, ui.id().with(("start", cmd)), Sense::click());
        p.rect_stroke(br, 2.0, Stroke::new(1.0, if resp.hovered() { t.text } else { t.text_dim }), egui::StrokeKind::Inside);
        p.text(pos2(br.left() + 16.0, br.center().y), egui::Align2::LEFT_CENTER, label, crate::theme::body(), t.text);
        if resp.clicked() {
            action = Some(cmd);
        }
        y += 44.0;
    }
    let main = Rect::from_min_max(pos2(left.right() + 36.0, r.top() + 40.0), r.max);
    p.text(main.min, egui::Align2::LEFT_TOP, "Recent", egui::FontId::proportional(22.0), t.text);
    p.text(
        pos2(main.center().x, main.center().y),
        egui::Align2::CENTER_CENTER,
        "There is nothing here yet.",
        egui::FontId::proportional(18.0),
        t.text,
    );
    p.text(
        pos2(main.center().x, main.center().y + 26.0),
        egui::Align2::CENTER_CENTER,
        "To get started, create or open a drawing.",
        crate::theme::body(),
        t.text_dim,
    );
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
