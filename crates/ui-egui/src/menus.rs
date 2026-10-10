//! Menus (in-window menu bar; the native macOS menu is built from the same tree), UI-only
//! commands and keyboard shortcuts.

use serde_json::{Value, json};

use crate::CadApp;

/// The top-level menus, in order.
pub const MENUS: &[&str] = &["File", "Edit", "View", "Insert", "Format", "Tools", "Draw", "Dimension", "Modify", "Window", "Help"];

/// UI-only commands: (id, label, menu path, shortcut).
pub const UI_COMMANDS: &[(&str, &str, &[&str], Option<&str>)] = &[
    ("ui.open", "Open...", &[], Some("Cmd+O")),
    ("ui.saveas", "Save As...", &[], None),
    ("ui.sample", "Open Sample Drawing", &["Help", "Open Sample Drawing"], None),
    ("ui.toggle.toolsets", "Tool Sets", &["Window", "Tool Sets"], Some("Cmd+3")),
    ("ui.toggle.palettes", "Properties Inspector", &["Window", "Properties Inspector"], Some("Cmd+1")),
    ("ui.toggle.toolbar", "Tool Bar", &["Window", "Tool Bar"], None),
    ("ui.toggle.filetabs", "File Tab", &["Window", "File Tab"], None),
    ("ui.toggle.statusbar", "Status Bar", &["Window", "Status Bar"], None),
    ("ui.toggle.cmdline", "Command Line", &["Window", "Command Line"], Some("Cmd+9")),
    ("ui.toggle.viewcube", "ViewCube", &["View", "ViewCube", "On"], None),
    ("ui.toggle.ucsicon", "UCS Icon", &["View", "UCS Icon", "On"], None),
    ("ui.theme.system", "Use System Setting", &["View", "Interface Theme", "Use System Setting"], None),
    ("ui.theme.light", "Light", &["View", "Interface Theme", "Light"], None),
    ("ui.theme.dark", "Dark", &["View", "Interface Theme", "Dark"], None),
    ("ui.theme", "Interface Theme", &[], None),
    ("ui.toggle.menubar", "In-window Menu Bar", &["Window", "In-window Menu Bar"], None),
    ("ui.hidepalettes", "Hide Palettes", &["Window", "Hide Palettes"], None),
    ("ui.resetpalettes", "Reset Palettes", &["Window", "Reset Palettes"], None),
    ("ui.start", "Start", &["Window", "Start"], None),
    ("ui.dialog.layers", "Layer Properties Manager", &["Window", "Layers"], None),
    ("ui.dialog.blocks", "Blocks", &["Window", "Blocks"], None),
    ("ui.dialog.qselect", "Quick Select...", &[], None),
    ("ui.dialog.parameters", "Parameters Manager", &["Window", "Parameters Manager"], None),
    ("ui.dialog.dsettings", "Drafting Settings...", &[], None),
    ("ui.dialog.about", "About CADCraft", &["Help", "About CADCraft"], None),
    ("ui.dialog.commands", "Command Reference", &["Help", "CADCraft Help"], Some("F1")),
    ("ui.toggle.history", "Command History", &["Window", "Command History"], Some("F2")),
    ("ui.cmdline.lines", "Command Line History Lines", &[], None),
    ("ui.noop", "", &[], None),
    ("ui.quit", "Quit CADCraft", &[], Some("Cmd+Q")),
];

pub fn is_ui_command(id: &str) -> bool {
    UI_COMMANDS.iter().any(|c| c.0 == id)
}

/// Run a UI-only command. `None` if `id` isn't one.
pub fn run_ui_command(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if let Some(r) = crate::managers::route(app, id, params) {
        return Some(r);
    }
    let toggle = |b: &mut bool, p: &Value| {
        *b = p.get("on").and_then(Value::as_bool).unwrap_or(!*b);
    };
    // When a file command should ask for a file. The `ui.*` commands are the pickers; the engine's
    // OPEN/SAVEAS/QSAVE ask only when typed or chosen from a menu (no parameters), so JSON calls
    // never open a picker.
    let no_path =
        if id.starts_with("ui.") { params.is_null() || (params.get("path").is_none() && params.get("data").is_none()) } else { params.is_null() };
    let r = match id {
        "ui.open" | "open" if no_path => {
            let picked = app.services.pick_open.as_ref().and_then(|f| f());
            if let Some(p) = picked {
                app.open_path(&p);
            }
            Ok(Value::Null)
        }
        "ui.saveas" | "saveas" if no_path => {
            let name = app.session.state().map(|s| s.title.clone()).unwrap_or_else(|_| "Drawing.dxf".into());
            let name = if name.contains('.') { name } else { format!("{name}.dxf") };
            if let Some(p) = app.services.pick_save.as_ref().and_then(|f| f(&name)) {
                return Some(app.session.execute("saveas", &json!({ "path": p })).map_err(|e| e.to_string()));
            }
            Ok(Value::Null)
        }
        "qsave" if no_path && app.session.state().is_ok_and(|s| s.path.is_none()) => return run_ui_command(app, "ui.saveas", &Value::Null),
        // Menu, toolbar, Cmd+P or typed: ask where to save the PDF. JSON calls (scripts, control
        // channel, MCP) always carry params, never get here and never open a dialog.
        "plot" | "print" | "exportpdf" if params.is_null() && app.session.state().is_ok() => {
            let pick = app.services.pick_save.as_ref()?;
            let title = app.session.state().map(|s| s.title.clone()).unwrap_or_default();
            let stem = std::path::Path::new(&title).file_stem().map(|s| s.to_string_lossy().to_string()).filter(|s| !s.is_empty());
            let Some(path) = pick(&format!("{}.pdf", stem.unwrap_or_else(|| "Drawing".into()))) else { return Some(Ok(Value::Null)) };
            let cmd = if id == "exportpdf" { "exportpdf" } else { "plot" };
            let r = app.session.execute(cmd, &json!({ "path": path })).map_err(|e| e.to_string());
            app.session.echo(match &r {
                Ok(_) => format!("Plotted to {path}"),
                Err(e) => e.clone(),
            });
            r
        }
        "ui.sample" => {
            let d = cadcraft_engine::sample::default_sample();
            app.session.open_drawing(d, "Bracket", None);
            app.canvas.zoom_pending = true;
            app.ui.start_tab = false;
            Ok(Value::Null)
        }
        "ui.toggle.toolsets" => {
            toggle(&mut app.ui.show_toolsets, params);
            Ok(Value::Null)
        }
        "ui.toggle.palettes" => {
            toggle(&mut app.ui.show_palettes, params);
            Ok(Value::Null)
        }
        "ui.toggle.toolbar" => {
            toggle(&mut app.ui.show_toolbar, params);
            Ok(Value::Null)
        }
        "ui.toggle.filetabs" => {
            toggle(&mut app.ui.show_file_tabs, params);
            Ok(Value::Null)
        }
        "ui.toggle.statusbar" => {
            toggle(&mut app.ui.show_status_bar, params);
            Ok(Value::Null)
        }
        "ui.toggle.cmdline" => {
            toggle(&mut app.ui.show_command_line, params);
            Ok(Value::Null)
        }
        "ui.toggle.viewcube" => {
            toggle(&mut app.ui.show_viewcube, params);
            Ok(Value::Null)
        }
        "ui.toggle.ucsicon" => {
            toggle(&mut app.ui.show_ucs_icon, params);
            Ok(Value::Null)
        }
        "ui.toggle.menubar" => {
            toggle(&mut app.ui.in_window_menu, params);
            Ok(Value::Null)
        }
        "ui.theme.system" | "ui.theme.light" | "ui.theme.dark" => {
            let pref = crate::theme::ThemePref::parse(id.trim_start_matches("ui.theme.")).unwrap_or_default();
            set_theme(app, pref)
        }
        // `{"theme": "system"|"light"|"dark"}` sets the choice; without it, reports it.
        "ui.theme" => match params.get("theme").and_then(Value::as_str) {
            Some(name) => match crate::theme::ThemePref::parse(name) {
                Some(pref) => set_theme(app, pref),
                None => Err(format!("unknown theme \"{name}\" (system, light or dark)")),
            },
            None => Ok(theme_json(app)),
        },
        "ui.hidepalettes" => {
            app.ui.show_palettes = false;
            app.ui.show_toolsets = false;
            Ok(Value::Null)
        }
        "ui.resetpalettes" => {
            let (menu, theme) = (app.ui.in_window_menu, app.ui.theme);
            app.ui = crate::UiState { in_window_menu: menu, theme, ..Default::default() };
            Ok(Value::Null)
        }
        "ui.start" => {
            app.ui.start_tab = true;
            Ok(Value::Null)
        }
        // The expanded command history (AutoCAD's text window; TEXTSCR opens it).
        "ui.toggle.history" | "textscr" => {
            let open = json!({"on": true});
            toggle(&mut app.cmd.expanded, if id == "textscr" { &open } else { params });
            Ok(json!({ "on": app.cmd.expanded }))
        }
        // `{"lines": n}`: how many history lines show above the command line (0–12); without it, reports it.
        "ui.cmdline.lines" => {
            if let Some(n) = params.get("lines").and_then(Value::as_u64) {
                app.ui.history_lines = n.min(crate::cmdline::MAX_LINES as u64) as usize;
            }
            Ok(json!({ "lines": app.ui.history_lines }))
        }
        "ui.dialog.layers"
        | "ui.dialog.blocks"
        | "ui.dialog.dsettings"
        | "ui.dialog.about"
        | "ui.dialog.commands"
        | "ui.dialog.qselect"
        | "ui.dialog.parameters" => {
            app.ui.dialog = Some(id.trim_start_matches("ui.dialog.").to_string());
            Ok(Value::Null)
        }
        "ui.dialog.close" => {
            app.ui.dialog = None;
            Ok(Value::Null)
        }
        "ui.quit" => {
            app.quit_requested = true;
            Ok(Value::Null)
        }
        "ui.noop" => Ok(Value::Null),
        // From a menu or typed, closing asks about unsaved changes; JSON calls close without asking.
        "close" if params.is_null() => {
            app.request_close(None);
            Ok(Value::Null)
        }
        "closeall" if params.is_null() => {
            app.request_close_all();
            Ok(Value::Null)
        }
        "layer" | "la" | "layers" if params.is_null() => {
            app.ui.dialog = Some("layers".into());
            Ok(Value::Null)
        }
        // Typed or menu-invoked (no parameters) these open their dialogs; JSON calls run the command.
        "qselect" | "qs" if params.is_null() => {
            app.ui.dialog = Some("qselect".into());
            Ok(Value::Null)
        }
        "parameters" | "par" if params.is_null() => {
            app.ui.dialog = Some("parameters".into());
            Ok(Value::Null)
        }
        "parametersclose" if params.is_null() => {
            if app.ui.dialog.as_deref() == Some("parameters") {
                app.ui.dialog = None;
            }
            Ok(Value::Null)
        }
        "dsettings" | "ds" | "se" if params.is_null() => {
            app.ui.dialog = Some("dsettings".into());
            Ok(Value::Null)
        }
        "properties" | "pr" | "props" | "ch" if params.is_null() => {
            app.ui.show_palettes = true;
            Ok(Value::Null)
        }
        _ => return None,
    };
    Some(r)
}

fn set_theme(app: &mut CadApp, pref: crate::theme::ThemePref) -> Result<Value, String> {
    app.ui.theme = pref;
    Ok(theme_json(app))
}

/// The theme choice and, for an explicit choice, the theme it shows (System resolves on the next
/// frame against the OS appearance; see `ui.inspect` → `theme`).
fn theme_json(app: &CadApp) -> Value {
    let shown = match app.ui.theme {
        crate::theme::ThemePref::System => app.shown_theme(),
        p => p.resolve(None),
    };
    json!({ "theme": app.ui.theme.as_str(), "shown": if shown == egui::Theme::Light { "light" } else { "dark" } })
}

/// A menu entry.
#[derive(Clone, Debug)]
pub enum Entry {
    Item { label: String, id: String, shortcut: Option<String>, enabled: bool },
    Sub { label: String, children: Vec<Entry> },
}

/// The full menu tree from the command registry plus UI commands, in registration order.
pub fn tree(app: &CadApp) -> Vec<(String, Vec<Entry>)> {
    let mut out: Vec<(String, Vec<Entry>)> = MENUS.iter().map(|m| (m.to_string(), Vec::new())).collect();
    let mut add = |path: &[&str], label: &str, id: &str, shortcut: Option<&str>, enabled: bool| {
        let Some((top, rest)) = path.split_first() else { return };
        let Some((_, entries)) = out.iter_mut().find(|(m, _)| m == top) else { return };
        let mut cur = entries;
        let n = rest.len();
        for (i, seg) in rest.iter().enumerate() {
            if i + 1 == n {
                cur.push(Entry::Item {
                    label: if seg.is_empty() { label.to_string() } else { seg.to_string() },
                    id: id.to_string(),
                    shortcut: shortcut.map(str::to_string),
                    enabled,
                });
            } else {
                let pos = cur.iter().position(|e| matches!(e, Entry::Sub { label, .. } if label == seg));
                let idx = match pos {
                    Some(p) => p,
                    None => {
                        cur.push(Entry::Sub { label: seg.to_string(), children: Vec::new() });
                        cur.len() - 1
                    }
                };
                let Some(Entry::Sub { children, .. }) = cur.get_mut(idx) else { return };
                cur = children;
            }
        }
    };
    for c in cadcraft_engine::command_specs() {
        if !c.menu.is_empty() {
            add(c.menu, c.label, c.id, c.shortcut, (c.enabled)(&app.session).is_ok());
        }
    }
    for (id, label, menu, sc) in UI_COMMANDS {
        if !menu.is_empty() {
            add(menu, label, id, *sc, true);
        }
    }
    out
}

/// Turn a registry shortcut ("Cmd+Shift+Z") into the form shown on this platform: macOS symbols ("⇧⌘Z") or
/// Windows/Linux text ("Ctrl+Shift+Z"). `mac` comes from the runtime OS (`ctx.os()`), so the web build is right too.
pub(crate) fn shortcut_label(s: &str, mac: bool) -> String {
    if !mac {
        return s.replace("Cmd+", "Ctrl+");
    }
    let shift = if s.contains("Shift+") { "⇧" } else { "" };
    let cmd = if s.contains("Cmd+") { "⌘" } else { "" };
    format!("{shift}{cmd}{}", s.replace("Cmd+", "").replace("Shift+", ""))
}

fn entry_ui(ui: &mut egui::Ui, e: &Entry, clicked: &mut Option<String>) {
    match e {
        Entry::Item { label, id, shortcut, enabled } => {
            let mut b = egui::Button::new(label);
            if let Some(s) = shortcut {
                b = b.shortcut_text(shortcut_label(s, ui.ctx().os().is_mac()));
            }
            if ui.add_enabled(*enabled, b).clicked() {
                *clicked = Some(id.clone());
                ui.close();
            }
        }
        Entry::Sub { label, children } => {
            ui.menu_button(label, |ui| {
                for c in children {
                    entry_ui(ui, c, clicked);
                }
            });
        }
    }
}

pub fn menu_bar(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get();
    let tree = tree(app);
    let mut clicked = None;
    egui::Panel::top("cc_menubar").exact_size(22.0).frame(egui::Frame::NONE.fill(t.chrome_dark).inner_margin(egui::Margin::symmetric(6, 0))).show(
        ui,
        |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                for (name, entries) in &tree {
                    ui.menu_button(name, |ui| {
                        for e in entries {
                            entry_ui(ui, e, &mut clicked);
                        }
                    });
                }
            });
        },
    );
    if let Some(id) = clicked {
        activate(app, &id);
    }
}

/// Like choosing a menu item: starts the command interactively.
pub fn activate(app: &mut CadApp, id: &str) {
    app.cmd.buffer.clear();
    app.start(id);
}

/// Keyboard shortcuts and function keys.
pub fn shortcuts(app: &mut CadApp, ctx: &egui::Context) {
    use egui::{Key, KeyboardShortcut, Modifiers};
    let sc = |m: Modifiers, k: Key| KeyboardShortcut::new(m, k);
    let cmd = Modifiers::COMMAND;
    let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
    let pairs: &[(KeyboardShortcut, &str)] = &[
        (sc(cmd_shift, Key::Z), "redo"),
        (sc(cmd, Key::Z), "undo"),
        (sc(cmd, Key::Y), "redo"),
        (sc(cmd, Key::N), "new"),
        (sc(cmd, Key::O), "ui.open"),
        (sc(cmd_shift, Key::S), "ui.saveas"),
        (sc(cmd, Key::S), "qsave"),
        (sc(cmd, Key::P), "plot"),
        (sc(cmd, Key::A), "selectall"),
        (sc(cmd_shift, Key::C), "copybase"),
        (sc(cmd, Key::C), "copyclip"),
        (sc(cmd, Key::X), "cutclip"),
        (sc(cmd, Key::V), "pasteclip"),
        (sc(cmd, Key::Num1), "ui.toggle.palettes"),
        (sc(cmd, Key::Num3), "ui.toggle.toolsets"),
        (sc(cmd, Key::Num9), "ui.toggle.cmdline"),
        (sc(Modifiers::NONE, Key::F1), "ui.dialog.commands"),
        (sc(Modifiers::NONE, Key::F2), "ui.toggle.history"),
        (sc(Modifiers::NONE, Key::F3), "osnap"),
        (sc(Modifiers::NONE, Key::F7), "grid"),
        (sc(Modifiers::NONE, Key::F8), "ortho"),
        (sc(Modifiers::NONE, Key::F9), "snap"),
        (sc(Modifiers::NONE, Key::F10), "polar"),
        (sc(Modifiers::NONE, Key::F11), "otrack"),
        (sc(Modifiers::NONE, Key::F12), "dynmode"),
    ];
    if ctx.egui_wants_keyboard_input() || app.closing.is_some() {
        return;
    }
    let mut fire = Vec::new();
    ctx.input_mut(|i| {
        for (s, id) in pairs {
            if i.consume_shortcut(s) {
                fire.push(*id);
            }
        }
    });
    for id in fire {
        // Toggles run transparently (they don't cancel the active command).
        let transparent = ["osnap", "grid", "ortho", "snap", "polar", "otrack", "dynmode"].contains(&id);
        if transparent || id.starts_with("ui.") {
            match app.session.execute(id, &json!({})) {
                Ok(v) => {
                    if let Some(m) = v.get("message").and_then(Value::as_str) {
                        app.session.echo(m.to_string());
                    }
                }
                Err(_) => {
                    let _ = app.run(id, json!({}));
                }
            }
        } else {
            activate(app, id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use cadcraft_engine::Session;
    use cadcraft_engine::cmd::file::{IoHooks, io, set_io};
    use serde_json::{Value, json};

    use crate::{CadApp, Services};

    fn fake_plot(_: &cadcraft_doc::Drawing, _: &cadcraft_doc::Space, _: &Value) -> Result<Vec<u8>, String> {
        Ok(b"%PDF-1.4 test".to_vec())
    }

    /// An app whose save dialog records the suggested name and answers `answer`.
    fn app_with_picker(answer: Option<String>) -> (CadApp, Rc<RefCell<Vec<String>>>) {
        let asked = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&asked);
        let services = Services {
            pick_open: None,
            pick_save: Some(Box::new(move |name: &str| {
                log.borrow_mut().push(name.to_string());
                answer.clone()
            })),
        };
        (CadApp::new(Session::new(), services), asked)
    }

    #[test]
    fn print_asks_where_to_save_the_pdf() {
        set_io(IoHooks { read: |_, _| Err("no reader in tests".into()), write: |_, _| Err("no writer in tests".into()), plot: Some(fake_plot) });
        if io().and_then(|h| h.plot).is_none() {
            return; // another test installed hooks without a plotter first
        }
        let path = std::env::temp_dir().join(format!("cadcraft-print-test-{}.pdf", std::process::id()));
        let ps = path.to_string_lossy().to_string();
        let _ = std::fs::remove_file(&path);
        let (mut app, asked) = app_with_picker(Some(ps.clone()));

        // Toolbar, menu and Cmd+P all start the command by name.
        app.start("plot");
        assert_eq!(*asked.borrow(), vec!["Drawing1.pdf".to_string()]);
        assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF"));
        assert!(app.session.log.iter().any(|l| l.contains(&ps)));
        let _ = std::fs::remove_file(&path);

        app.cmdline("print");
        app.start("exportpdf");
        assert_eq!(asked.borrow().len(), 3);
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);

        // Programmatic calls never open a dialog.
        let r = app.run("plot", json!({})).unwrap();
        assert!(r["data"].is_string());
        assert_eq!(asked.borrow().len(), 3);
    }

    #[test]
    fn cancelled_print_writes_nothing() {
        let (mut app, asked) = app_with_picker(None);
        app.start("plot");
        assert_eq!(asked.borrow().len(), 1);
        assert!(app.session.running.is_none());
    }
}

#[cfg(test)]
mod shortcut_label_tests {
    use super::shortcut_label;

    #[test]
    fn shortcut_label_per_platform() {
        assert_eq!(shortcut_label("Cmd+Shift+Z", true), "⇧⌘Z");
        assert_eq!(shortcut_label("Cmd+Z", true), "⌘Z");
        assert_eq!(shortcut_label("Cmd+Shift+Z", false), "Ctrl+Shift+Z");
        assert_eq!(shortcut_label("Cmd+Z", false), "Ctrl+Z");
        assert_eq!(shortcut_label("Shift+F3", false), "Shift+F3");
        assert_eq!(shortcut_label("Shift+F3", true), "⇧F3");
        assert_eq!(shortcut_label("F1", false), "F1");
        assert_eq!(shortcut_label("F1", true), "F1");
    }
}
