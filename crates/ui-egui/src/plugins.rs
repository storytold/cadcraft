//! Tools ▸ Plug-ins and its windows: the installed plug-ins' commands, the Plug-ins window
//! (installed plug-ins, installing from a file, a GitHub repository or a catalog, updates), the
//! install confirmation (permissions, kind, source, SHA-256) and the parameter dialog generated
//! from a plug-in command's `params`. Everything goes through the engine's `plugin.*` commands
//! ([`cadcraft_engine::plugin`], RFC-0046). Plug-in names and command labels are the plug-in's
//! own and are not translated.

use cadcraft_engine::plugin::craft_plugin::{Access, Param, ParamKind, PluginCommand};
use egui::RichText;
use serde_json::{Map, Value, json};

use crate::menus::Entry;
use crate::theme::Tokens;
use crate::{CadApp, Pick};

/// The Tools submenu that lists the plug-ins (built at run time, from what is installed).
pub const MENU: &str = "Plug-ins";
/// Tools ▸ Plug-ins ▸ Manage Plug-ins…: the Plug-ins window.
pub const MANAGE: &str = "ui.plugins";
/// The Plug-ins window's name in `UiState::dialog`.
pub const DIALOG: &str = "plugins";
/// A plug-in command's menu item: `ui.plugin.run:<plugin id>:<command id>` (neither id may hold
/// a `:`).
const RUN: &str = "ui.plugin.run:";
/// Width of the repository and catalog address fields (points).
const ADDRESS_WIDTH: f32 = 260.0;
/// Width of a text parameter's field (points).
const TEXT_FIELD_WIDTH: f32 = 240.0;

/// The install confirmation's content: the `plugin.install` parameters that named the bundle, and
/// what `plugin.install {review: true}` said about it.
#[derive(Clone, Debug)]
pub struct Review {
    pub request: Value,
    pub info: Value,
}

impl Review {
    /// Where the plug-in comes from, in a line.
    pub fn source_text(&self) -> String {
        let source = &self.info["source"];
        let t = |k: &str| source[k].as_str().unwrap_or_default();
        match t("type") {
            "github" => format!("github.com/{} {} ({})", t("repo"), t("tag"), t("asset")),
            "catalog" => format!("{} · {}", t("package"), t("catalog")),
            "other" => format!("{} · {}", t("kind"), t("location")),
            _ => t("path").to_string(),
        }
    }
}

/// A plug-in command waiting for its parameters.
#[derive(Clone, Debug)]
pub struct ParamsDialog {
    pub plugin: String,
    pub command: String,
    pub title: String,
    pub params: Vec<Param>,
    pub values: Map<String, Value>,
}

impl ParamsDialog {
    /// The dialog for `command` of plug-in `plugin` (named `name`), with each parameter's default.
    pub fn new(plugin: &str, name: &str, command: &PluginCommand) -> Self {
        ParamsDialog {
            plugin: plugin.to_string(),
            command: command.id.clone(),
            title: format!("{name}: {}", command.label.trim_end_matches(['…', '.'])),
            params: command.params.clone(),
            values: command.params.iter().map(|p| (p.name.clone(), p.default_value())).collect(),
        }
    }

    /// The `plugin.run` parameters with the values entered.
    pub fn run_params(&self) -> Value {
        json!({"id": self.plugin, "command": self.command, "params": self.values})
    }
}

/// The Plug-ins window's state, the install confirmation and the parameter dialog.
#[derive(Default)]
pub struct PluginsWindow {
    /// The GitHub repository typed (`owner/repo` or `owner/repo@tag`).
    repo: String,
    /// The catalog address typed.
    catalog_url: String,
    /// The last action's message.
    status: String,
    /// The catalogs' packages, after Browse Catalogs.
    packages: Vec<Value>,
    /// The updates found, after Check for Updates.
    updates: Vec<Value>,
    /// The install confirmation, while it waits for an answer.
    pub review: Option<Review>,
    /// A plug-in command's parameters, while they are entered.
    pub params: Option<ParamsDialog>,
}

/// Run `f` on the app with its Plug-ins window state held apart.
fn with_window<T>(app: &mut CadApp, f: impl FnOnce(&mut CadApp, &mut PluginsWindow) -> T) -> T {
    let mut w = std::mem::take(&mut app.plugins);
    let out = f(app, &mut w);
    app.plugins = w;
    out
}

/// An engine command, its error as text.
fn engine(app: &mut CadApp, id: &str, params: Value) -> Result<Value, String> {
    app.session.execute(id, &params).map_err(|e| e.to_string())
}

/// The UI commands of this module: Manage Plug-ins… and each plug-in command's menu item.
pub fn route(app: &mut CadApp, id: &str, _params: &Value) -> Option<Result<Value, String>> {
    if id == MANAGE {
        app.ui.dialog = Some(DIALOG.into());
        return Some(Ok(Value::Null));
    }
    let (plugin, command) = id.strip_prefix(RUN)?.split_once(':')?;
    run_command(app, plugin, command);
    Some(Ok(Value::Null))
}

/// Tools ▸ Plug-ins: each installed plug-in's commands, then Manage Plug-ins….
pub fn menu(app: &CadApp) -> Entry {
    let mut children = Vec::new();
    let enabled = app.session.state().is_ok();
    for p in app.session.plugins.list() {
        let m = &p.manifest;
        let item =
            |label: &str, c: &PluginCommand| Entry::Item { label: label.to_string(), id: format!("{RUN}{}:{}", m.id, c.id), shortcut: None, enabled };
        match m.commands.as_slice() {
            [] => {}
            [one] => children.push(item(&m.name, one)),
            many => children.push(Entry::Sub { label: m.name.clone(), children: many.iter().map(|c| item(&c.label, c)).collect() }),
        }
    }
    children.push(Entry::Item { label: "Manage Plug-ins...".into(), id: MANAGE.into(), shortcut: None, enabled: true });
    Entry::Sub { label: MENU.into(), children }
}

/// Run a plug-in command, after its parameter dialog when it declares parameters.
pub fn run_command(app: &mut CadApp, id: &str, command: &str) {
    let Some(plugin) = app.session.plugins.get(id) else { return };
    let Some(c) = plugin.manifest.command(Some(command)) else { return };
    if c.params.is_empty() {
        let params = json!({"id": plugin.manifest.id, "command": c.id});
        run_plugin(app, params);
        return;
    }
    app.plugins.params = Some(ParamsDialog::new(&plugin.manifest.id, &plugin.manifest.name, c));
}

/// `plugin.run`, its notes (or that it is done) in the command history and the status bar; a
/// failure is echoed by `app.run`.
fn run_plugin(app: &mut CadApp, params: Value) {
    let name = params["id"].as_str().and_then(|id| app.session.plugins.get(id)).map(|p| p.manifest.name.clone()).unwrap_or_default();
    let r = match app.run("plugin.run", params) {
        Ok(r) => r,
        Err(e) => {
            app.set_status(e);
            return;
        }
    };
    let notes: Vec<&str> = r["notes"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let mut line = if notes.is_empty() { crate::tf!("{name}: done", name = name) } else { notes.join(" · ") };
    if r["panel"].is_string() {
        line = format!("{line} ({})", crate::tl!("CADCraft doesn't show plug-in panels yet"));
    }
    app.session.echo(line.clone());
    app.set_status(line);
}

/// Check a bundle (`plugin.install {…, review: true}`) and show its confirmation.
fn review(app: &mut CadApp, w: &mut PluginsWindow, request: Value) {
    let mut asked = request.clone();
    asked["review"] = json!(true);
    match engine(app, "plugin.install", asked) {
        Ok(info) => {
            w.review = Some(Review { request, info });
            w.status.clear();
        }
        Err(e) => w.status = e,
    }
}

/// Install the reviewed bundle (the one with the reviewed SHA-256 only).
fn install_reviewed(app: &mut CadApp, w: &mut PluginsWindow, reviewed: Review) {
    let mut request = reviewed.request;
    request["sha256"] = reviewed.info["sha256"].clone();
    w.status = match engine(app, "plugin.install", request) {
        Ok(_) => crate::tf!("Installed {name}.", name = reviewed.info["name"].as_str().unwrap_or_default()),
        Err(e) => e,
    };
}

/// The install confirmation and the parameter dialog, when they show (every frame).
pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    with_window(app, |app, w| {
        if w.review.is_some() {
            confirmation(app, w, ctx);
        }
        if w.params.is_some() {
            parameters(app, w, ctx);
        }
    });
}

/// The Plug-ins window ([`DIALOG`]).
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    egui::Window::new(crate::tl!("Plug-ins")).id(egui::Id::unique("cc_plugins")).open(open).default_width(460.0).collapsible(false).show(ctx, |ui| {
        let pick = egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| with_window(app, |app, w| manage(app, w, ui))).inner;
        // The picker's answer reviews the file with the window's state in place
        if pick {
            pick_file(app);
        }
    });
}

/// Install ▸ From File…: the `.craftplugin` picker, then the file's confirmation.
fn pick_file(app: &mut CadApp) {
    let picked = app.pick_then_open(Pick::Plugin, |app, path| {
        with_window(app, |app, w| review(app, w, json!({"path": path})));
        Ok(Value::Null)
    });
    if let Some(Err(e)) = picked {
        app.plugins.status = e;
    }
}

/// The Plug-ins window's content; whether From File… was chosen.
fn manage(app: &mut CadApp, w: &mut PluginsWindow, ui: &mut egui::Ui) -> bool {
    let dim = Tokens::get().text_dim;
    installed(app, w, ui, dim);
    ui.add_space(8.0);
    ui.strong(crate::tl!("Install"));
    let pick = app.services.pick_plugin.is_some() && ui.button(crate::tl!("From File...")).clicked();
    ui.horizontal(|ui| {
        ui.label(crate::tl!("GitHub repository"));
        ui.add(egui::TextEdit::singleline(&mut w.repo).desired_width(ADDRESS_WIDTH).hint_text("owner/repo"));
        if ui.add_enabled(!w.repo.trim().is_empty(), egui::Button::new(crate::tl!("Review..."))).clicked() {
            let repo = w.repo.trim().to_string();
            review(app, w, json!({"repo": repo}));
        }
    });
    ui.add_space(8.0);
    catalogs(app, w, ui, dim);
    if !w.status.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new(w.status.as_str()).color(dim));
    }
    pick
}

/// The installed plug-ins, their commands and problems.
fn installed(app: &mut CadApp, w: &mut PluginsWindow, ui: &mut egui::Ui, dim: egui::Color32) {
    ui.strong(crate::tl!("Installed"));
    let plugins: Vec<_> = app.session.plugins.list().to_vec();
    if plugins.is_empty() {
        ui.label(RichText::new(crate::tl!("No plug-ins installed.")).color(dim));
    }
    let has_drawing = app.session.state().is_ok();
    for p in &plugins {
        let m = &p.manifest;
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} {}", m.name, m.version)).strong());
            let access = if m.permissions.access == Access::Write { crate::tl!("edits the drawing") } else { crate::tl!("reads the drawing") };
            ui.label(RichText::new(access).color(dim));
            if !p.stored {
                ui.label(RichText::new(crate::tl!("this session only")).color(dim));
            }
        });
        ui.horizontal_wrapped(|ui| {
            for c in &m.commands {
                if ui.add_enabled(has_drawing, egui::Button::new(c.label.as_str())).clicked() {
                    // A command with parameters opens its dialog over this window
                    std::mem::swap(&mut app.plugins, w);
                    run_command(app, &m.id, &c.id);
                    std::mem::swap(&mut app.plugins, w);
                }
            }
            if ui.button(crate::tl!("Remove")).clicked() {
                w.status = engine(app, "plugin.remove", json!({"id": m.id})).err().unwrap_or_default();
            }
        });
    }
    for problem in app.session.plugins.problems() {
        ui.label(RichText::new(problem).color(dim));
    }
}

/// The catalogs added, browsing them and checking for updates.
fn catalogs(app: &mut CadApp, w: &mut PluginsWindow, ui: &mut egui::Ui, dim: egui::Color32) {
    ui.strong(crate::tl!("Catalogs"));
    for url in app.session.plugins.prefs.catalogs.clone() {
        ui.horizontal(|ui| {
            ui.label(url.as_str());
            if ui.small_button(crate::tl!("Remove")).clicked() {
                w.status = engine(app, "plugin.catalog.add", json!({"url": url, "remove": true})).err().unwrap_or_default();
            }
        });
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut w.catalog_url).desired_width(ADDRESS_WIDTH).hint_text("https://raw.githubusercontent.com/…/index.json"),
        );
        if ui.add_enabled(!w.catalog_url.trim().is_empty(), egui::Button::new(crate::tl!("Add"))).clicked() {
            match engine(app, "plugin.catalog.add", json!({"url": w.catalog_url.trim()})) {
                Ok(_) => w.catalog_url.clear(),
                Err(e) => w.status = e,
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.add_enabled(!app.session.plugins.prefs.catalogs.is_empty(), egui::Button::new(crate::tl!("Browse Catalogs"))).clicked() {
            match engine(app, "plugin.catalog.list", json!({})) {
                Ok(r) => {
                    w.packages = r["packages"].as_array().cloned().unwrap_or_default();
                    let warnings: Vec<&str> = r["warnings"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                    w.status = warnings.join("\n");
                }
                Err(e) => w.status = e,
            }
        }
        if ui.add_enabled(!app.session.plugins.list().is_empty(), egui::Button::new(crate::tl!("Check for Updates"))).clicked() {
            match engine(app, "plugin.update", json!({})) {
                Ok(r) => {
                    w.updates = r["updates"].as_array().cloned().unwrap_or_default();
                    w.status = if w.updates.is_empty() { crate::tl!("Every plug-in is up to date.").to_string() } else { String::new() };
                }
                Err(e) => w.status = e,
            }
        }
    });
    for pkg in w.packages.clone() {
        ui.horizontal(|ui| {
            let installed = pkg["installed"].as_str().map(|v| format!(" ({})", crate::tf!("installed: {version}", version = v))).unwrap_or_default();
            ui.label(format!("{} {}{installed}", pkg["name"].as_str().unwrap_or_default(), pkg["version"].as_str().unwrap_or_default()));
            if ui.small_button(crate::tl!("Install...")).clicked() {
                review(app, w, json!({"catalog": pkg["catalog"], "id": pkg["id"]}));
            }
        });
        if let Some(text) = pkg["description"].as_str().filter(|t| !t.is_empty()) {
            ui.label(RichText::new(text).color(dim));
        }
    }
    for u in w.updates.clone() {
        ui.horizontal(|ui| {
            let text = |k: &str| u[k].as_str().unwrap_or_default().to_string();
            ui.label(format!("{} {} → {}", text("id"), text("installed"), text("available")));
            if ui.small_button(crate::tl!("Update...")).clicked() {
                review(app, w, u["install"].clone());
            }
        });
    }
}

/// The install confirmation: nothing is written before Install.
fn confirmation(app: &mut CadApp, w: &mut PluginsWindow, ctx: &egui::Context) {
    let Some(reviewed) = w.review.clone() else { return };
    let info = &reviewed.info;
    let text = |k: &str| info[k].as_str().unwrap_or_default().to_string();
    let mut open = true;
    let mut answer = None;
    egui::Window::new(crate::tl!("Install Plug-in"))
        .id(egui::Id::unique("cc_plugin_install"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            let dim = Tokens::get().text_dim;
            ui.heading(format!("{} {}", text("name"), text("version")));
            for k in ["author", "description", "license", "repo"] {
                if !text(k).is_empty() {
                    ui.label(text(k));
                }
            }
            egui::Grid::new("cc_plugin_confirm").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                ui.label(crate::tl!("Kind"));
                ui.label(text("kind"));
                ui.end_row();
                ui.label(crate::tl!("Access"));
                let write = info["permissions"]["access"] == "write";
                ui.label(if write { crate::tl!("Reads and edits the open drawing") } else { crate::tl!("Reads the open drawing") });
                ui.end_row();
                ui.label(crate::tl!("Commands"));
                let allowed: Vec<&str> =
                    info["permissions"]["commands"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                ui.label(if allowed.is_empty() { crate::tl!("Any command its access allows").to_string() } else { allowed.join(", ") });
                ui.end_row();
                if info["permissions"]["saveFile"] == true {
                    ui.label(crate::tl!("Files"));
                    ui.label(crate::tl!("Asks to save files (CADCraft doesn't offer this yet)"));
                    ui.end_row();
                }
                ui.label(crate::tl!("Source"));
                ui.label(reviewed.source_text());
                ui.end_row();
                ui.label("SHA-256");
                ui.add(egui::Label::new(RichText::new(text("sha256")).monospace()).wrap());
                ui.end_row();
            });
            if info["inCatalog"] == false {
                ui.label(RichText::new(crate::tl!("This plug-in isn't in any catalog you use: install it only if you trust its author.")).strong());
            }
            if let Some(old) = info["replaces"].as_str() {
                ui.label(RichText::new(crate::tf!("Replaces version {version}.", version = old)).color(dim));
            }
            if info["permissionsChanged"] == true {
                ui.label(RichText::new(crate::tl!("This version asks for different permissions.")).strong());
            }
            if info["stored"] == false {
                ui.label(RichText::new(crate::tl!("It is kept for this session only.")).color(dim));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(crate::tl!("Install")).clicked() {
                    answer = Some(true);
                }
                if ui.button(crate::tl!("Cancel")).clicked() {
                    answer = Some(false);
                }
            });
        });
    if !open || answer.is_some() {
        w.review = None;
    }
    if answer == Some(true) {
        install_reviewed(app, w, reviewed);
    }
}

/// A plug-in command's parameter dialog.
fn parameters(app: &mut CadApp, w: &mut PluginsWindow, ctx: &egui::Context) {
    let Some(dialog) = w.params.as_mut() else { return };
    let mut open = true;
    let mut answer = None;
    let title = dialog.title.clone();
    egui::Window::new(title).id(egui::Id::unique("cc_plugin_params")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        egui::Grid::new("cc_plugin_params_grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            for p in &dialog.params {
                ui.label(p.title());
                let v = dialog.values.entry(p.name.clone()).or_insert_with(|| p.default_value());
                field(ui, p, v);
                ui.end_row();
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button(crate::tl!("Run")).clicked() {
                answer = Some(true);
            }
            if ui.button(crate::tl!("Cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    if !open || answer.is_some() {
        let finished = w.params.take();
        if answer == Some(true)
            && let Some(dialog) = finished
        {
            run_plugin(app, dialog.run_params());
        }
    }
}

/// One parameter's control, editing `v`.
fn field(ui: &mut egui::Ui, p: &Param, v: &mut Value) {
    match p.kind {
        ParamKind::Number | ParamKind::Slider => {
            let mut n = v.as_f64().unwrap_or(0.0);
            let changed = match (p.kind, p.min, p.max) {
                (ParamKind::Slider, Some(lo), Some(hi)) if lo < hi => ui.add(egui::Slider::new(&mut n, lo..=hi)).changed(),
                _ => {
                    let mut drag = egui::DragValue::new(&mut n);
                    if let (Some(lo), Some(hi)) = (p.min, p.max)
                        && lo <= hi
                    {
                        drag = drag.range(lo..=hi);
                    }
                    ui.add(drag).changed()
                }
            };
            if changed {
                *v = json!(n);
            }
        }
        ParamKind::Boolean => {
            let mut b = v.as_bool().unwrap_or(false);
            if ui.checkbox(&mut b, "").changed() {
                *v = json!(b);
            }
        }
        ParamKind::Select | ParamKind::List => {
            let current = v.as_str().unwrap_or_default().to_string();
            craft_ui::ComboBox::from_id_salt(("cc_plugin_param", &p.name)).selected_text(current.clone()).show_ui(ui, |ui| {
                for o in &p.options {
                    if ui.selectable_label(*o == current, o.as_str()).clicked() {
                        *v = json!(o);
                    }
                }
            });
        }
        // Text, and a colour as `#RRGGBB`
        _ => {
            let mut s = v.as_str().unwrap_or_default().to_string();
            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(TEXT_FIELD_WIDTH)).changed() {
                *v = json!(s);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use cadcraft_engine::Session;
    use cadcraft_engine::plugin::craft_plugin::bundle::Bundle;

    use super::*;
    use crate::Services;

    /// A plug-in whose commands each draw a line; `asks` declares a parameter.
    fn install(app: &mut CadApp) {
        let wasm = wat::parse_str(
            r#"(module
              (import "craft" "call" (func $call (param i32 i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 0) "line")
              (data (i32.const 32) "{\"points\":[[0,0],[1,1]]}")
              (func (export "craft_run") (param i32) (result i32)
                (drop (call $call (i32.const 0) (i32.const 4) (i32.const 32) (i32.const 24))) (i32.const 0)))"#,
        )
        .unwrap();
        let manifest = json!({"id": "io.example.ui", "name": "UI Test", "version": "1.0.0", "main": "p.wasm",
            "apps": {"cadcraft": {"api": 6}}, "permissions": {"access": "write"},
            "commands": [{"id": "plain", "label": "Plain"},
                         {"id": "asks", "label": "Asks...", "params": [{"name": "count", "type": "number", "default": 2, "min": 1, "max": 9}]}]});
        let bytes = Bundle { manifest: manifest.to_string(), wasm, assets: vec![] }.encode();
        let data = cadcraft_engine::cmd::file::base64_encode(&bytes);

        // Review, then install what was reviewed, as the confirmation does
        with_window(app, |app, w| {
            review(app, w, json!({"data": data}));
            let reviewed = w.review.take().unwrap_or_else(|| panic!("{}", w.status));
            assert_eq!(reviewed.info["permissions"]["access"], "write");
            assert_eq!(reviewed.source_text(), "data");
            install_reviewed(app, w, reviewed);
        });
    }

    fn objects(app: &CadApp) -> usize {
        app.session.doc().unwrap().entity_count()
    }

    #[test]
    fn plugin_commands_are_in_the_tools_menu_run_as_one_step_and_ask_for_their_parameters() {
        let mut app = CadApp::new(Session::new(), Services::default());
        install(&mut app);
        let count = objects(&app);

        // Tools ▸ Plug-ins lists the plug-in's commands, then Manage Plug-ins…
        let tools = crate::menus::tree(&app).into_iter().find(|(m, _)| m == "Tools").map(|(_, e)| e).unwrap();
        let Some(Entry::Sub { children, .. }) = tools.iter().find(|e| matches!(e, Entry::Sub { label, .. } if label == MENU)) else {
            panic!("no Plug-ins submenu")
        };
        let Some(Entry::Sub { children: commands, .. }) = children.first() else { panic!("{children:?}") };
        let Some(Entry::Item { id, .. }) = commands.first() else { panic!("{commands:?}") };
        assert!(matches!(children.last(), Some(Entry::Item { id, .. }) if id == MANAGE));

        // A command without parameters runs at once, as one undo step
        crate::menus::activate(&mut app, id);
        assert_eq!(objects(&app), count + 1);
        assert_eq!(app.session.state().unwrap().undo.last().map(|u| u.label.as_str()), Some("UI Test: Plain"));

        // One with parameters asks first
        run_command(&mut app, "io.example.ui", "asks");
        let dialog = app.plugins.params.clone().unwrap();
        assert_eq!(dialog.values["count"], 2.0);
        assert_eq!(dialog.title, "UI Test: Asks");
        run_plugin(&mut app, dialog.run_params());
        assert_eq!(objects(&app), count + 2);

        crate::menus::activate(&mut app, MANAGE);
        assert_eq!(app.ui.dialog.as_deref(), Some(DIALOG));
    }

    #[test]
    fn a_repository_in_no_catalog_says_where_it_comes_from() {
        let reviewed = Review {
            request: json!({"repo": "example/x"}),
            info: json!({"source": {"type": "github", "repo": "example/x", "tag": "v1", "asset": "x.craftplugin"}, "inCatalog": false}),
        };
        assert_eq!(reviewed.source_text(), "github.com/example/x v1 (x.craftplugin)");
    }

    #[test]
    fn the_plugins_window_and_its_dialogs_draw() {
        let mut app = CadApp::new(Session::new(), Services::default());
        install(&mut app);
        run_command(&mut app, "io.example.ui", "asks");
        app.plugins.review =
            Some(Review { request: json!({}), info: json!({"name": "X", "inCatalog": false, "source": {"type": "file", "path": "x"}}) });
        let ctx = egui::Context::default();
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| {
            let mut open = true;
            dialog(&mut app, ui.ctx(), &mut open);
            show(&mut app, ui.ctx());
        });
        output.textures_delta.clear();
        assert!(app.plugins.params.is_some() && app.plugins.review.is_some(), "nothing was answered");
    }
}
