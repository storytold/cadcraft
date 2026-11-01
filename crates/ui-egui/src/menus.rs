//! Menus (in-window menu bar; the native macOS menu is built from the same tree), UI-only
//! commands and keyboard shortcuts.

use serde_json::{Value, json};

use crate::CadApp;

/// The top-level menus, in order.
pub const MENUS: &[&str] = &["File", "Edit", "View", "Insert", "Format", "Tools", "Draw", "Dimension", "Modify", "Window", "Help"];

/// File ▸ Open Recent: one `ui.recent.<n>` item per recent file, then Clear Recent Files.
pub const OPEN_RECENT: &str = "Open Recent";
/// The last item of File ▸ Open Recent.
pub const CLEAR_RECENT: &str = "Clear Recent Files";
const RECENT_ITEM: &str = "ui.recent.";
const RECENT_CLEAR: &str = "ui.recent.clear";

/// UI-only commands: (id, label, menu path, shortcut).
pub const UI_COMMANDS: &[(&str, &str, &[&str], Option<&str>)] = &[
    ("ui.spacemouse", "3D Mouse...", &["Tools", "3D Mouse..."], None),
    ("ui.spacemouse.calibrate", "Calibrate 3D Mouse", &[], None),
    ("ui.spacemouse.navigate", "3D Mouse Navigation", &[], None),
    ("ui.dock", "Arrange Panel", &[], None),
    ("ui.dock.reset", "Reset Panel Layout", &["Window", "Reset Panel Layout"], None),
    ("ui.open", "Open...", &[], Some("Cmd+O")),
    ("ui.newtemplate", "New from Template...", &["File", "New from Template..."], None),
    ("ui.saveas", "Save As...", &[], None),
    ("ui.sample", "Open Sample Drawing", &["Help", "Open Sample Drawing"], None),
    ("ui.toggle.toolsets", "Tool Sets", &["Window", "Tool Sets"], Some("Cmd+3")),
    ("ui.panel.properties", "Properties Inspector", &["Window", "Properties Inspector"], Some("Cmd+1")),
    ("ui.panel.layers", "Layers Palette", &["Window", "Layers Palette"], None),
    ("ui.toggle.quickcalc", "QuickCalc", &["Window", "QuickCalc"], Some("Cmd+8")),
    ("ui.panel.quickcalc", "QuickCalc", &[], None),
    ("ui.toggle.systemcursor", "Show System Cursor", &["View", "Accessibility", "Show System Cursor"], None),
    ("ui.animations.system", "Use System Setting", &["View", "Accessibility", "Animation Effects", "Use System Setting"], None),
    ("ui.animations.on", "On", &["View", "Accessibility", "Animation Effects", "On"], None),
    ("ui.animations.off", "Off", &["View", "Accessibility", "Animation Effects", "Off"], None),
    ("ui.animations", "Animation Effects", &[], None),
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
    ("ui.saveformat.dxf", "DXF", &["File", "Default Save Format", "DXF"], None),
    ("ui.saveformat.dwg", "DWG", &["File", "Default Save Format", "DWG"], None),
    ("ui.saveformat", "Default Save Format", &[], None),
    ("ui.saveversion.2018", "Version 2018", &["File", "Default Save Format", "Version 2018"], None),
    ("ui.saveversion.2013", "Version 2013", &["File", "Default Save Format", "Version 2013"], None),
    ("ui.saveversion.2010", "Version 2010", &["File", "Default Save Format", "Version 2010"], None),
    ("ui.saveversion.2007", "Version 2007", &["File", "Default Save Format", "Version 2007"], None),
    ("ui.saveversion.2004", "Version 2004", &["File", "Default Save Format", "Version 2004"], None),
    ("ui.saveversion.2000", "Version 2000", &["File", "Default Save Format", "Version 2000"], None),
    ("ui.saveversion", "Default Save Version", &[], None),
    ("ui.toggle.menubar", "In-window Menu Bar", &["Window", "In-window Menu Bar"], None),
    ("ui.hidepalettes", "Hide Palettes", &["Window", "Hide Palettes"], None),
    ("ui.resetpalettes", "Reset Palettes", &["Window", "Reset Palettes"], None),
    ("ui.start", "Start", &["Window", "Start"], None),
    ("ui.dialog.layers", "Layer Properties Manager", &["Window", "Layers"], None),
    ("ui.dialog.blocks", "Blocks", &["Window", "Blocks"], None),
    ("ui.dialog.qselect", "Quick Select...", &[], None),
    ("ui.dialog.parameters", "Parameters Manager", &["Window", "Parameters Manager"], None),
    ("ui.dialog.dsettings", "Drafting Settings...", &[], None),
    #[cfg(not(target_arch = "wasm32"))]
    ("ui.updates.check", "Check for Updates…", &["Help", "Check for Updates…"], None),
    ("ui.dialog.about", "About CADCraft", &["Help", "About CADCraft"], None),
    ("ui.dialog.commands", "Command Reference", &["Help", "CADCraft Help"], Some("F1")),
    ("ui.toggle.history", "Command History", &["Window", "Command History"], Some("F2")),
    ("ui.cmdline.lines", "Command Line History Lines", &[], None),
    ("ui.cmdline.clear", "Clear Command History", &[], None),
    ("ui.dialog.language", "Interface Language", &["Edit", "Interface Language"], None),
    ("ui.language", "Language", &[], None),
    ("ui.noop", "", &[], None),
    ("ui.quit", "Quit CADCraft", &[], Some("Cmd+Q")),
];

pub fn is_ui_command(id: &str) -> bool {
    UI_COMMANDS.iter().any(|c| c.0 == id)
}

/// Run a UI-only command. `None` if `id` isn't one.
pub fn run_ui_command(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    crate::i18n::with_language(app.language(), || run_ui_command_inner(app, id, params))
}

fn route_ui_subsystems(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if let Some(result) = crate::spacemouse::route(app, id, params) {
        return Some(result);
    }
    if let Some(r) = crate::xrefs::route(app, id, params) {
        return Some(r);
    }
    if let Some(r) = crate::bedit::route(app, id, params) {
        return Some(r);
    }
    if let Some(r) = crate::managers::route(app, id, params) {
        return Some(r);
    }
    if matches!(id, "ui.dock" | "ui.dock.reset") {
        return Some(crate::docking::command(app, id == "ui.dock.reset", params));
    }
    if matches!(id, "ui.panel.properties" | "ui.panel.layers") {
        let panel = if id == "ui.panel.properties" { "properties" } else { "layers" };
        return Some(crate::docking::command(app, false, &json!({"operation":"open", "panel":panel})));
    }
    if id == "ui.toggle.quickcalc" {
        return Some(crate::palettes::quickcalc::toggle(app));
    }
    // Explicit opens remain idempotent; typed QUICKCALC with JSON parameters uses the engine's CAL alias.
    if id == "ui.panel.quickcalc" || (matches!(id, "quickcalc" | "qc") && params.is_null()) {
        return Some(crate::palettes::quickcalc::open(app));
    }
    if let Some(r) = crate::plotstyles::route(app, id, params) {
        return Some(r);
    }
    if let Some(r) = crate::options::route(app, id, params) {
        return Some(r);
    }
    None
}

fn run_recent_file_command(app: &mut CadApp, id: &str) -> Option<Result<Value, String>> {
    if id == RECENT_CLEAR {
        app.ui.recent_files.clear();
        return Some(Ok(Value::Null));
    }
    if let Some(i) = id.strip_prefix(RECENT_ITEM).and_then(|n| n.parse::<usize>().ok()) {
        let Some(path) = app.ui.recent_files.get(i).cloned() else { return Some(Err(format!("no recent file {i}"))) };
        app.open_path(std::path::Path::new(&path));
        return Some(Ok(Value::Null));
    }
    None
}

fn run_preference_command(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    let r = match id {
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
        "ui.animations.system" | "ui.animations.on" | "ui.animations.off" => {
            let pref = crate::animation::AnimationPref::parse(id.trim_start_matches("ui.animations.")).unwrap_or_default();
            Ok(set_animations(app, pref))
        }
        // `{"animations": "system"|"on"|"off"}` sets the choice; without it, reports it.
        "ui.animations" => match params.get("animations").and_then(Value::as_str) {
            Some(name) => match crate::animation::AnimationPref::parse(name) {
                Some(pref) => Ok(set_animations(app, pref)),
                None => Err(format!("unknown animations choice \"{}\" (system, on or off)", name.chars().take(40).collect::<String>())),
            },
            None => Ok(animations_json(app)),
        },
        "ui.saveformat.dxf" | "ui.saveformat.dwg" => {
            let format = crate::SaveFormat::parse(id.trim_start_matches("ui.saveformat.")).unwrap_or_default();
            Ok(set_save_format(app, format))
        }
        // `{"format": "dxf"|"dwg"}` sets the default save format for new drawings; without it,
        // reports it.
        "ui.saveformat" => match params.get("format").and_then(Value::as_str) {
            Some(name) => match crate::SaveFormat::parse(name) {
                Some(format) => Ok(set_save_format(app, format)),
                None => Err(format!("unknown save format \"{name}\" (dxf or dwg)")),
            },
            None => Ok(json!({ "saveFormat": app.ui.save_format.as_str() })),
        },
        // `{"version": "2000"…"2018"}` sets the version new drawings are saved in; without it,
        // reports it.
        id if id.starts_with("ui.saveversion") => {
            let name = id
                .strip_prefix("ui.saveversion.")
                .map(str::to_string)
                .or_else(|| params.get("version").map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string)));
            match name {
                Some(name) => match crate::SaveVersion::parse(&name) {
                    Some(v) => {
                        app.ui.save_version = v.year().to_string();
                        app.session.echo(format!("New drawings will be saved as version {v}"));
                        Ok(json!({ "saveVersion": app.ui.save_version }))
                    }
                    None => Err(format!("unknown version \"{}\" (2000, 2004, 2007, 2010, 2013 or 2018)", name.chars().take(40).collect::<String>())),
                },
                None => Ok(json!({ "saveVersion": app.ui.save_version })),
            }
        }
        _ => return None,
    };
    Some(r)
}

fn run_ui_toggle_command(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    let toggle = |b: &mut bool, p: &Value| {
        *b = p.get("on").and_then(Value::as_bool).unwrap_or(!*b);
    };
    let result = match id {
        "ui.toggle.toolsets" => {
            if params.get("on").and_then(Value::as_bool) == Some(true) {
                return Some(crate::docking::command(app, false, &json!({"operation":"open", "panel":"toolSets"})));
            }
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
        "ui.toggle.systemcursor" => {
            toggle(&mut app.ui.system_cursor, params);
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
        "ui.toggle.history" | "textscr" => {
            let open = json!({"on": true});
            toggle(&mut app.cmd.expanded, if id == "textscr" { &open } else { params });
            Ok(json!({ "on": app.cmd.expanded }))
        }
        _ => return None,
    };
    Some(result)
}

fn run_language_command(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if id != "ui.language" {
        return None;
    }
    let Some(preference) = params.get("lang").and_then(Value::as_str).and_then(crate::i18n::Preference::parse) else {
        return Some(Err(format!("language must be {}", crate::i18n::Preference::codes())));
    };
    app.ui.interface_language = preference;
    let shown = app.language();
    if shown != preference.resolve(&app.system_languages) {
        app.set_status(match preference.resolve(&app.system_languages) {
            "ko" => {
                "No font covers the Korean interface, so it stays in English. Install a Hangul font (Malgun Gothic or Nanum Gothic), or build with a Hang/Kore craft-fonts face."
            }
            "vi" => {
                "No installed interface font has the Vietnamese letters, so the interface stays in English. Install one that does (Segoe UI, Arial, DejaVu Sans or Noto Sans) to use Vietnamese."
            }
            "ar" => "No Arabic font is installed, so the interface stays in English. Install one (Noto Sans Arabic, for example) to use Arabic.",
            _ => "No Chinese font is installed, so the interface stays in English. Install one (Noto Sans CJK, for example) to use Chinese.",
        });
    }
    Some(Ok(json!({"interfaceLanguage": preference.code(), "shown": shown})))
}

fn run_close_command(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    match id {
        "close" if params.is_null() => {
            app.request_close(None);
            Some(Ok(Value::Null))
        }
        "closeall" if params.is_null() => {
            app.request_close_all();
            Some(Ok(Value::Null))
        }
        _ => None,
    }
}

fn should_pick_file_path(id: &str, params: &Value) -> bool {
    if id.starts_with("ui.") { params.is_null() || (params.get("path").is_none() && params.get("data").is_none()) } else { params.is_null() }
}

/// A user's QSAVE: a drawing with no file yet picks one, and saving over a file that would lose
/// content asks first. `None` lets the engine save. JSON calls carry params and never get here;
/// they report the loss in their result instead.
fn user_qsave(app: &mut CadApp) -> Option<Result<Value, String>> {
    if app.session.state().is_ok_and(|s| s.path.is_none()) {
        return run_ui_command(app, "ui.saveas", &Value::Null);
    }
    app.hold_lossy_save().then_some(Ok(Value::Null))
}

fn run_ui_command_inner(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if let Some(result) = route_ui_subsystems(app, id, params) {
        return Some(result);
    }
    // When a file command should ask for a file. The `ui.*` commands are the pickers; the engine's
    // OPEN/SAVEAS/QSAVE ask only when typed or chosen from a menu (no parameters), so JSON calls
    // never open a picker.
    let no_path = should_pick_file_path(id, params);
    if let Some(result) = run_recent_file_command(app, id) {
        return Some(result);
    }
    if let Some(result) = run_preference_command(app, id, params) {
        return Some(result);
    }
    if let Some(result) = run_ui_toggle_command(app, id, params) {
        return Some(result);
    }
    if let Some(result) = run_language_command(app, id, params) {
        return Some(result);
    }
    if let Some(result) = run_close_command(app, id, params) {
        return Some(result);
    }
    let r = match id {
        "ui.open" | "open" if no_path => {
            let picked = app.services.pick_open.as_ref().and_then(|f| f());
            if let Some(p) = picked {
                app.open_path(std::path::Path::new(&p));
            }
            Ok(Value::Null)
        }
        // APPLOAD typed or chosen from a menu: the `.lsp` file picker, starting where the last
        // one came from. JSON calls carry `path` and never open one. A picked file is loaded as a
        // typed name is, through the interactive command, so a routine it starts that asks for a
        // point waits for it; on the web (no file system) it says to drop the file instead.
        "appload" | "ap" if params.is_null() => {
            if app.services.no_file_system {
                let hint = "APPLOAD: there is no file system here; drop a .lsp file on the window to load it";
                app.session.echo(hint.to_string());
                app.set_status(hint);
                return Some(Ok(Value::Null));
            }
            let pick = app.services.pick_lisp.as_ref()?;
            let dir = app.ui.last_lisp_dir.clone().unwrap_or_default();
            let Some(path) = pick(&dir) else { return Some(Ok(Value::Null)) };
            if let Some(dir) = std::path::Path::new(&path).parent().and_then(std::path::Path::to_str).filter(|d| !d.is_empty()) {
                app.ui.last_lisp_dir = Some(dir.to_string());
            }
            if let Err(e) = app.session.start("appload") {
                return Some(Err(e.to_string()));
            }
            app.cmdline(&path);
            Ok(Value::Null)
        }
        // Pick a template (.dwt or .dxf) and start a new drawing from it.
        "ui.newtemplate" => {
            if let Some(p) = app.services.pick_open.as_ref().and_then(|f| f()) {
                return Some(app.session.execute("new", &json!({ "template": p })).map_err(|e| e.to_string()));
            }
            Ok(Value::Null)
        }
        // RECOVER from the menu or typed: pick the file. Without a picker the engine asks for the
        // file name on the command line.
        "recover" if no_path && app.services.pick_open.is_some() => {
            if let Some(p) = app.services.pick_open.as_ref().and_then(|f| f()) {
                let r = app.run("recover", json!({ "path": p }));
                app.opened(r);
            }
            Ok(Value::Null)
        }
        "ui.saveas" | "saveas" if no_path => {
            let title = app.session.state().map(|s| s.title.clone()).ok();
            let name = app.ui.save_format.suggested_name(title.as_deref());
            if let Some(p) = app.services.pick_save.as_ref().and_then(|f| f(&name)) {
                return Some(app.session.execute("saveas", &saveas_params(&app.ui, &app.session, &p)).map_err(|e| e.to_string()));
            }
            Ok(Value::Null)
        }
        "qsave" if no_path => return user_qsave(app),
        // EXPORTPDF from the menu or typed: ask where to save the PDF (PLOT and PRINT open the Plot
        // dialog, `managers::route`). JSON calls (scripts, control channel, MCP) always carry
        // params, never get here and never open a dialog.
        "exportpdf" if params.is_null() && app.session.state().is_ok() => {
            let pick = app.services.pick_save.as_ref()?;
            let title = app.session.state().map(|s| s.title.clone()).unwrap_or_default();
            let stem = std::path::Path::new(&title).file_stem().map(|s| s.to_string_lossy().to_string()).filter(|s| !s.is_empty());
            let Some(path) = pick(&format!("{}.pdf", stem.unwrap_or_else(|| "Drawing".into()))) else { return Some(Ok(Value::Null)) };
            let r = app.session.execute("exportpdf", &json!({ "path": path })).map_err(|e| e.to_string());
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
        "export" | "exp" | "import" | "imp" | "publish" | "preview" | "pre" if params.is_null() && app.session.state().is_ok() => {
            return Some(crate::exchange::route(app, id));
        }
        "ui.hidepalettes" => {
            app.ui.show_palettes = false;
            app.ui.show_toolsets = false;
            Ok(Value::Null)
        }
        "ui.resetpalettes" => {
            let (menu, theme, interface_language, save_format) = (app.ui.in_window_menu, app.ui.theme, app.ui.interface_language, app.ui.save_format);
            let (animations, save_version) = (app.ui.animations, std::mem::take(&mut app.ui.save_version));
            let system_cursor = app.system_cursor_default;
            app.ui = crate::UiState {
                in_window_menu: menu,
                theme,
                animations,
                interface_language,
                save_format,
                save_version,
                system_cursor,
                ..Default::default()
            };
            Ok(Value::Null)
        }
        "ui.start" => {
            app.ui.start_tab = true;
            Ok(Value::Null)
        }
        // The expanded command history (AutoCAD's text window; TEXTSCR opens it).
        "ui.cmdline.lines" => {
            if let Some(n) = params.get("lines").and_then(Value::as_u64) {
                app.ui.history_lines = n.min(crate::cmdline::MAX_LINES as u64) as usize;
            }
            Ok(json!({ "lines": app.ui.history_lines }))
        }
        "ui.cmdline.clear" => {
            app.session.log.clear();
            Ok(Value::Null)
        }
        "ui.dialog.language"
        | "ui.dialog.layers"
        | "ui.dialog.blocks"
        | "ui.dialog.dsettings"
        | "ui.dialog.about"
        | "ui.dialog.commands"
        | "ui.dialog.qselect"
        | "ui.dialog.parameters" => {
            app.ui.dialog = Some(id.trim_start_matches("ui.dialog.").to_string());
            Ok(Value::Null)
        }
        #[cfg(not(target_arch = "wasm32"))]
        "ui.updates.check" => {
            crate::updates_ui::check_now(app);
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
        "layer" | "la" | "layers" if params.is_null() => {
            app.ui.dialog = Some("layers".into());
            Ok(Value::Null)
        }
        // Typed or menu-invoked (no parameters) these open their dialogs; JSON calls run the command.
        "qselect" | "qs" if params.is_null() => {
            app.ui.dialog = Some("qselect".into());
            Ok(Value::Null)
        }
        "units" | "un" if params.is_null() => {
            app.ui.dialog = Some("units".into());
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
        // Typed or from a menu (no parameters): the Hatch and Gradient dialog (HPDLGMODE 1) and the
        // Hatch Edit dialog. -HATCH, -HATCHEDIT and JSON calls keep the command line and the JSON form.
        "hatch" | "h" | "bh" | "gradient" | "gd" if params.is_null() && crate::hatchdlg::dialog_on(app) => {
            crate::hatchdlg::open_create(app, matches!(id, "gradient" | "gd"));
            Ok(Value::Null)
        }
        // Typed or from the Tools menu: the Command Aliases dialog. JSON calls keep the JSON form.
        "aliasedit" if params.is_null() => {
            crate::aliasdlg::open(app);
            Ok(Value::Null)
        }
        "hatchedit" | "he" if params.is_null() && app.session.state().is_ok() => {
            crate::hatchdlg::open_edit(app);
            Ok(Value::Null)
        }
        "properties" | "pr" | "props" | "ch" if params.is_null() => {
            crate::docking::command(app, false, &json!({"operation":"open", "panel":"properties"}))
        }
        _ => return None,
    };
    Some(r)
}

/// SAVEAS parameters for a path picked in the Save dialog: a drawing that has no file yet is saved
/// in the default save version; one opened from a file keeps that file's version.
fn saveas_params(ui: &crate::UiState, session: &cadcraft_engine::Session, path: &str) -> Value {
    let untitled = session.state().is_ok_and(|s| s.path.is_none());
    match crate::SaveVersion::parse(&ui.save_version) {
        Some(v) if untitled => json!({ "path": path, "version": v.year() }),
        _ => json!({ "path": path }),
    }
}

fn set_save_format(app: &mut CadApp, format: crate::SaveFormat) -> Value {
    app.ui.save_format = format;
    app.session.echo(format!("Save As will suggest {}", format.as_str().to_ascii_uppercase()));
    json!({ "saveFormat": format.as_str() })
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

fn set_animations(app: &mut CadApp, pref: crate::animation::AnimationPref) -> Value {
    app.ui.animations = pref;
    let shown = if app.shows_animations() { "on" } else { "off" };
    app.session.echo(match pref {
        crate::animation::AnimationPref::System => format!("Animation effects follow the system setting ({shown})"),
        _ => format!("Animation effects are {shown}"),
    });
    animations_json(app)
}

/// The animation effects choice and whether the interface animates with it (System resolves
/// against the OS setting).
fn animations_json(app: &CadApp) -> Value {
    json!({ "animations": app.ui.animations.as_str(), "shown": app.shows_animations() })
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
    if let Some((_, file)) = out.iter_mut().find(|(m, _)| m == "File") {
        let at = file.iter().position(|e| matches!(e, Entry::Item { id, .. } if id == "open")).map_or(0, |i| i + 1);
        file.insert(at, Entry::Sub { label: OPEN_RECENT.into(), children: recent_entries(app) });
    }
    out
}

/// The items of File ▸ Open Recent: the recent files by name, newest first, then Clear Recent Files.
pub fn recent_entries(app: &CadApp) -> Vec<Entry> {
    let mut entries: Vec<Entry> = app
        .ui
        .recent_files
        .iter()
        .enumerate()
        .map(|(i, path)| Entry::Item { label: file_name(path), id: format!("{RECENT_ITEM}{i}"), shortcut: None, enabled: true })
        .collect();
    let enabled = !app.ui.recent_files.is_empty();
    entries.push(Entry::Item { label: CLEAR_RECENT.into(), id: RECENT_CLEAR.into(), shortcut: None, enabled });
    entries
}

/// The file name of `path` (the whole path when it has none).
pub(crate) fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().to_string())
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

pub(crate) fn entry_ui(ui: &mut egui::Ui, e: &Entry, clicked: &mut Option<String>) {
    match e {
        Entry::Item { label, id, shortcut, enabled } => {
            let mut b = egui::Button::new(crate::i18n::t(label));
            if let Some(s) = shortcut {
                b = b.shortcut_text(shortcut_label(s, ui.ctx().os().is_mac()));
            }
            if ui.add_enabled(*enabled, b).clicked() {
                *clicked = Some(id.clone());
                ui.close();
            }
        }
        Entry::Sub { label, children } => {
            ui.menu_button(crate::i18n::menu(label), |ui| {
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
                // Titles first, so a hovered title can take over before any menu is drawn.
                let titles: Vec<egui::Response> = tree.iter().map(|(name, _)| ui.add(egui::Button::new(crate::i18n::menu(name)))).collect();
                switch_on_hover(ui.ctx(), &titles);
                for ((_, entries), title) in tree.iter().zip(&titles) {
                    egui::Popup::menu(title).show(|ui| {
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

/// While one menu of the bar is open, pointing at another title opens that menu instead.
fn switch_on_hover(ctx: &egui::Context, titles: &[egui::Response]) {
    let is_open = |title: &egui::Response| egui::Popup::is_id_open(ctx, egui::Popup::default_response_id(title));
    if !titles.iter().any(is_open) {
        return;
    }
    // A click on a title toggles its menu by itself.
    if let Some(title) = titles.iter().find(|t| t.hovered() && !t.clicked() && !is_open(t)) {
        egui::Popup::open_id(ctx, egui::Popup::default_response_id(title));
    }
}

/// Activate a menu command. QuickCalc toggles transparently; other items clear pending
/// command-line text and start their command interactively.
pub fn activate(app: &mut CadApp, id: &str) {
    if id == "ui.toggle.quickcalc" {
        run_transparent(app, id);
        return;
    }
    app.cmd.buffer.clear();
    app.start(id);
}

/// Activate the menu command `id` from a native menu bar, which takes the Edit menu's key
/// equivalents (⌘C, ⌘X, ⌘V, ⌘A, ⌘Z, ⇧⌘Z) before any window sees them: while a text field has
/// the keyboard, in the main window or a floating panel's own window, the field gets the key
/// and the drawing command does not run.
pub fn activate_native(app: &mut CadApp, ctx: &egui::Context, id: &str) {
    let (Some(window), Some(text)) = (text_field_window(ctx), text_command(id, ctx.os().is_mac())) else {
        activate(app, id);
        return;
    };
    match text {
        TextCommand::Paste => ctx.send_viewport_cmd_to(window, egui::ViewportCommand::RequestPaste),
        TextCommand::Events(events) if window == egui::ViewportId::ROOT => {
            app.synthetic.extend(events);
            ctx.request_repaint();
        }
        TextCommand::Events(events) => craft_ui::docking::send_keys(ctx, window, events),
    }
}

/// The window whose text field has the keyboard: a floating panel's own OS window, or the main
/// window.
pub fn text_field_window(ctx: &egui::Context) -> Option<egui::ViewportId> {
    craft_ui::docking::typing_window(ctx).or_else(|| ctx.text_edit_focused().then_some(egui::ViewportId::ROOT))
}

/// Whether a focused text field takes the menu command `id` ([`activate_native`]): a native
/// menu keeps such items enabled while one has the keyboard, whatever the drawing allows.
pub fn edits_text(id: &str) -> bool {
    text_command(id, false).is_some()
}

/// What a focused text field does for an Edit menu command.
#[derive(Debug, PartialEq)]
enum TextCommand {
    /// The clipboard text, which the window asks for and receives as `Paste`.
    Paste,
    Events(Vec<egui::Event>),
}

fn text_command(id: &str, mac: bool) -> Option<TextCommand> {
    let key = |key, shift| {
        let modifiers = egui::Modifiers { mac_cmd: mac, ctrl: !mac, command: true, shift, alt: false };
        TextCommand::Events([true, false].map(|pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }).into())
    };
    Some(match id {
        "copyclip" => TextCommand::Events(vec![egui::Event::Copy]),
        "cutclip" => TextCommand::Events(vec![egui::Event::Cut]),
        "pasteclip" => TextCommand::Paste,
        "u" => key(egui::Key::Z, false),
        "redo" => key(egui::Key::Z, true),
        "selectall" => key(egui::Key::A, false),
        _ => return None,
    })
}

/// Run `id` with default parameters without cancelling the active command, as the drafting
/// toggles (F3, F7-F12, the status bar buttons) are in AutoCAD.
pub fn run_transparent(app: &mut CadApp, id: &str) {
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
}

/// CADCraft's keyboard shortcuts and function keys, each with the command it runs. Every shortcut a
/// command shows (its registry `key`, or a UI command's) must be here, or pressing it does nothing.
pub(crate) fn builtin_shortcuts() -> Vec<(egui::KeyboardShortcut, &'static str)> {
    use egui::{Key, KeyboardShortcut, Modifiers};
    let sc = |m: Modifiers, k: Key| KeyboardShortcut::new(m, k);
    let cmd = Modifiers::COMMAND;
    let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
    vec![
        (sc(cmd_shift, Key::A), "pickstyle"),
        (sc(cmd_shift, Key::Z), "redo"),
        (sc(cmd, Key::Z), "u"),
        (sc(cmd, Key::Y), "redo"),
        (sc(cmd, Key::N), "new"),
        (sc(cmd, Key::W), "close"),
        (sc(cmd, Key::O), "ui.open"),
        (sc(cmd_shift, Key::S), "ui.saveas"),
        (sc(cmd, Key::S), "qsave"),
        (sc(cmd, Key::P), "plot"),
        (sc(cmd, Key::A), "selectall"),
        (sc(cmd_shift, Key::C), "copybase"),
        (sc(cmd, Key::C), "copyclip"),
        (sc(cmd, Key::X), "cutclip"),
        (sc(cmd_shift, Key::V), "pasteblock"),
        (sc(cmd, Key::V), "pasteclip"),
        (sc(cmd, Key::Num1), "ui.panel.properties"),
        (sc(cmd, Key::Num3), "ui.toggle.toolsets"),
        (sc(cmd, Key::Num9), "ui.toggle.cmdline"),
        (sc(Modifiers::NONE, Key::F1), "ui.dialog.commands"),
        (sc(Modifiers::NONE, Key::F2), "ui.toggle.history"),
        (sc(Modifiers::NONE, Key::F3), "osnap"),
        (sc(Modifiers::NONE, Key::F5), "isoplane"),
        (sc(cmd, Key::E), "isoplane"),
        (sc(Modifiers::NONE, Key::F7), "grid"),
        (sc(Modifiers::NONE, Key::F8), "ortho"),
        (sc(Modifiers::NONE, Key::F9), "snap"),
        (sc(Modifiers::NONE, Key::F10), "polar"),
        (sc(Modifiers::NONE, Key::F11), "otrack"),
        (sc(Modifiers::NONE, Key::F12), "dynmode"),
    ]
}

/// Keyboard shortcuts and function keys.
pub fn shortcuts(app: &mut CadApp, ctx: &egui::Context) {
    use egui::{Key, KeyboardShortcut, Modifiers};
    let sc = |m: Modifiers, k: Key| KeyboardShortcut::new(m, k);
    let cmd = Modifiers::COMMAND;
    let pairs = builtin_shortcuts();
    crate::clipboard::mirror(app, ctx);
    if app.closing.is_some() {
        return;
    }
    // A palette toggle is global even while its expression field owns keyboard focus.
    if ctx.input_mut(|i| i.consume_shortcut(&sc(cmd, Key::Num8))) {
        run_transparent(app, "ui.toggle.quickcalc");
    }
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    // Cmd/Ctrl+C, X and V arrive as clipboard events, not key presses.
    let mut fire = crate::clipboard::shortcut_commands(app, ctx);
    ctx.input_mut(|i| {
        for (s, id) in &pairs {
            if i.consume_shortcut(s) && !fire.contains(id) {
                fire.push(*id);
            }
        }
    });
    for id in fire {
        // ISOPLANE (F5, Ctrl+E) switches the plane without cancelling the active command.
        if id == "isoplane" {
            if let Some(m) = app.session.execute(id, &json!({})).ok().and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_string)) {
                app.session.echo(m);
            }
            continue;
        }
        // Toggles run transparently (they don't cancel the active command).
        let transparent = ["osnap", "grid", "ortho", "snap", "polar", "otrack", "dynmode", "pickstyle"].contains(&id);
        if transparent || id.starts_with("ui.") {
            run_transparent(app, id);
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
            ..Services::default()
        };
        (CadApp::new(Session::new(), services), asked)
    }

    /// A registry shortcut ("Cmd+Shift+Z", "F8") as egui reads it.
    fn parse_shortcut(s: &str) -> Option<egui::KeyboardShortcut> {
        let mut m = egui::Modifiers::NONE;
        let (mods, key) = s.rsplit_once('+').unwrap_or(("", s));
        for part in mods.split('+').filter(|p| !p.is_empty()) {
            match part {
                "Cmd" => m |= egui::Modifiers::COMMAND,
                "Shift" => m |= egui::Modifiers::SHIFT,
                "Alt" => m |= egui::Modifiers::ALT,
                _ => return None,
            }
        }
        Some(egui::KeyboardShortcut::new(m, egui::Key::from_name(key)?))
    }

    #[test]
    fn every_shown_shortcut_runs_its_command() {
        // Handled outside the table: Delete by the command line (it erases the selection only when
        // the line is empty), Cmd+8 before it (QuickCalc's own field keeps focus), Cmd+Q by the
        // macOS app menu, the only place that shows it.
        let elsewhere = [("Delete", "erase.selection"), ("Cmd+8", "ui.toggle.quickcalc"), ("Cmd+Q", "ui.quit")];
        let table = super::builtin_shortcuts();
        let shown = cadcraft_engine::command_specs()
            .iter()
            .filter_map(|c| Some((c.shortcut?, c.id)))
            .chain(super::UI_COMMANDS.iter().filter_map(|(id, _, _, sc)| Some(((*sc)?, *id))));
        let mut missing = Vec::new();
        for (key, id) in shown {
            if elsewhere.contains(&(key, id)) {
                continue;
            }
            let sc = parse_shortcut(key).unwrap_or_else(|| panic!("{id}: unreadable shortcut {key}"));
            // The table may run a command's UI form (`ui.open` for `open`), which does the same.
            if !table.iter().any(|(s, i)| *s == sc && (*i == id || i.strip_prefix("ui.") == Some(id))) {
                missing.push(format!("{key} {id}"));
            }
        }
        assert!(missing.is_empty(), "shown but not run: {missing:?}");
    }

    #[test]
    fn plot_and_print_open_the_plot_dialog_and_exportpdf_asks_for_a_file() {
        set_io(IoHooks { read: |_, _| Err("no reader in tests".into()), write: |_, _| Err("no writer in tests".into()), plot: Some(fake_plot) });
        if io().and_then(|h| h.plot).is_none() {
            return; // another test installed hooks without a plotter first
        }
        let path = std::env::temp_dir().join(format!("cadcraft-print-test-{}.pdf", std::process::id()));
        let ps = path.to_string_lossy().to_string();
        let _ = std::fs::remove_file(&path);
        let (mut app, asked) = app_with_picker(Some(ps.clone()));

        // Toolbar, menu, Cmd+P and typed PRINT open the Plot dialog; its OK asks for the file.
        app.start("plot");
        assert_eq!(app.ui.dialog.as_deref(), Some("plot"));
        app.ui.dialog = None;
        app.cmdline("print");
        assert_eq!(app.ui.dialog.as_deref(), Some("plot"));
        assert!(asked.borrow().is_empty());

        // EXPORTPDF asks where to save straight away.
        app.start("exportpdf");
        assert_eq!(*asked.borrow(), vec!["Drawing1.pdf".to_string()]);
        assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF"));
        assert!(app.session.log.iter().any(|l| l.contains(&ps)));
        let _ = std::fs::remove_file(&path);

        // Programmatic calls never open a dialog.
        app.ui.dialog = None;
        let r = app.run("plot", json!({})).unwrap();
        assert!(r["data"].is_string());
        assert_eq!(asked.borrow().len(), 1);
        assert!(app.ui.dialog.is_none());
    }

    #[test]
    fn cancelled_export_writes_nothing() {
        let (mut app, asked) = app_with_picker(None);
        app.start("exportpdf");
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

#[cfg(test)]
mod units_dialog_tests {
    use cadcraft_engine::Session;
    use serde_json::json;

    use crate::{CadApp, Services};

    #[test]
    fn units_typed_opens_dialog_json_runs_command() {
        let mut app = CadApp::new(Session::new(), Services::default());
        app.cmdline("UNITS");
        assert_eq!(app.ui.dialog.as_deref(), Some("units"));
        app.ui.dialog = None;
        app.start("un");
        assert_eq!(app.ui.dialog.as_deref(), Some("units"));
        app.ui.dialog = None;
        let r = app.run("units", json!({ "lunits": 4, "luprec": 3, "insunits": 4 })).unwrap();
        assert_eq!(r["lunits"], 4);
        assert!(app.ui.dialog.is_none(), "JSON calls never open dialogs");
        let h = &app.session.doc().unwrap().header;
        assert_eq!((h.i64("LUNITS", 0), h.i64("LUPREC", 0), h.i64("INSUNITS", 0)), (4, 3, 4));
    }
}
