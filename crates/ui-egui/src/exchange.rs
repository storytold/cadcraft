//! EXPORT, IMPORT, PUBLISH and PREVIEW from a menu, the tool bar or the command line: the first
//! three ask for a file, PREVIEW shows the plot in a window. Calls with parameters (scripts, the
//! control channel, MCP) never get here and never open a dialog: they run the engine commands.

use std::sync::atomic::{AtomicU32, Ordering};

use egui::TextureHandle;
use serde_json::{Value, json};

use crate::CadApp;

/// Bumped each time PREVIEW is started, so the window renders the drawing as it is now.
static PREVIEW_GEN: AtomicU32 = AtomicU32::new(0);

/// Whether a typed or menu-invoked `id` is handled here. DXFIN and DXFOUT ask with a file dialog
/// only while FILEDIA is 1 and there is one; otherwise they ask for the file name at the command
/// line, as AutoCAD does with FILEDIA 0 (the engine's prompt).
pub fn wants_dialog(app: &CadApp, id: &str) -> bool {
    match id {
        "dxfin" => app.session.settings.filedia && app.services.pick_open.is_some(),
        "dxfout" => app.session.settings.filedia && app.services.pick_save.is_some(),
        _ => true,
    }
}

/// Handle a typed or menu-invoked file exchange command (`id` may be an alias).
pub fn route(app: &mut CadApp, id: &str) -> Result<Value, String> {
    let title = app.session.state().map(|s| s.title.clone()).unwrap_or_default();
    let stem = std::path::Path::new(&title).file_stem().map(|s| s.to_string_lossy().to_string()).filter(|s| !s.is_empty());
    let stem = stem.unwrap_or_else(|| "Drawing".into());
    match id {
        "preview" | "pre" => {
            PREVIEW_GEN.fetch_add(1, Ordering::Relaxed);
            app.ui.dialog = Some("preview".into());
            Ok(Value::Null)
        }
        "import" | "imp" | "dxfin" => {
            let Some(pick) = app.services.pick_open.as_ref() else { return no_picker(app, "IMPORT", r#"import {"path": "parts.dxf"}"#) };
            match pick() {
                Some(path) => app.run(if id == "dxfin" { "dxfin" } else { "import" }, json!({ "path": path })),
                None => Ok(Value::Null),
            }
        }
        "dxfout" => {
            let Some(pick) = app.services.pick_save.as_ref() else { return no_picker(app, "DXFOUT", r#"dxfout {"path": "plan.dxf"}"#) };
            let Some(path) = pick(&format!("{stem}.dxf")) else { return Ok(Value::Null) };
            // DXFOUT adds .dxf to a name without it. A dialog that doesn't add it (GTK) asked
            // about replacing the name as typed, not that file: never replace it unasked.
            let is_dxf = std::path::Path::new(&path).extension().is_some_and(|e| e.eq_ignore_ascii_case("dxf"));
            if !is_dxf && std::path::Path::new(&format!("{path}.dxf")).exists() {
                let msg = format!("DXFOUT: {path}.dxf already exists; choose it in the dialog to replace it");
                app.session.echo(msg.clone());
                return Err(msg);
            }
            app.run("dxfout", json!({ "path": path }))
        }
        "publish" => {
            let Some(pick) = app.services.pick_save.as_ref() else { return no_picker(app, "PUBLISH", "publish {}") };
            match pick(&format!("{stem}.pdf")) {
                Some(path) => app.run("publish", json!({ "path": path })),
                None => Ok(Value::Null),
            }
        }
        _ => {
            let Some(pick) = app.services.pick_save.as_ref() else { return no_picker(app, "EXPORT", r#"export {"format": "svg"}"#) };
            match pick(&format!("{stem}.svg")) {
                Some(path) => app.run("export", json!({ "path": path })),
                None => Ok(Value::Null),
            }
        }
    }
}

fn no_picker(app: &mut CadApp, name: &str, example: &str) -> Result<Value, String> {
    let msg = format!("{name}: there is no file dialog here; run it with parameters, e.g. {example}");
    app.session.echo(msg.clone());
    Err(msg)
}

#[derive(Clone)]
struct Preview {
    generation: u32,
    shown: Result<(TextureHandle, String), String>,
}

/// A preview the Plot dialog asked for: its PLOT parameters (PNG output) and a caption.
#[derive(Clone)]
struct FromPlot {
    params: Value,
    info: String,
}

const FROM_PLOT: &str = "cc_plot_preview_from_plot";

/// Show the Plot dialog's settings as they plot (PLOT with `params`, a PNG); Print... and Close
/// return to the dialog.
pub(crate) fn preview_plot(app: &mut CadApp, ctx: &egui::Context, params: Value, info: String) {
    PREVIEW_GEN.fetch_add(1, Ordering::Relaxed);
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(FROM_PLOT), FromPlot { params, info }));
    app.ui.dialog = Some("preview".into());
}

/// Render the plot preview of the current layout (or model), or of the Plot dialog's settings,
/// as a texture.
fn render(app: &mut CadApp, ctx: &egui::Context) -> Result<(TextureHandle, String), String> {
    if let Some(f) = ctx.data(|d| d.get_temp::<FromPlot>(egui::Id::unique(FROM_PLOT))) {
        let r = app.session.execute("plot", &f.params).map_err(|e| e.to_string())?;
        let png = r["data"].as_str().and_then(cadcraft_engine::cmd::file::base64_decode).ok_or("the preview has no image")?;
        let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png).map_err(|e| e.to_string())?.to_rgba8();
        let size = [img.width() as usize, img.height() as usize];
        let tex = ctx.load_texture("plot-preview", egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR);
        return Ok((tex, f.info));
    }
    let r = app.session.execute("preview", &json!({ "width": 1600 })).map_err(|e| e.to_string())?;
    let png = r["data"].as_str().and_then(cadcraft_engine::cmd::file::base64_decode).ok_or("the preview has no image")?;
    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png).map_err(|e| e.to_string())?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let tex = ctx.load_texture("plot-preview", egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR);
    let mut info = format!("{} · {}", r["layout"].as_str().unwrap_or(""), r["paper"].as_str().unwrap_or(""));
    if r["landscape"].as_bool() == Some(true) {
        info.push_str(" · landscape");
    }
    Ok((tex, info))
}

/// The Plot Preview window: the sheet as it plots, with Print and Close.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let id = egui::Id::unique("cc_plot_preview");
    let generation = PREVIEW_GEN.load(Ordering::Relaxed);
    let cached = ctx.data(|d| d.get_temp::<Preview>(id)).filter(|p| p.generation == generation);
    let preview = match cached {
        Some(p) => p,
        None => {
            let p = Preview { generation, shown: render(app, ctx) };
            ctx.data_mut(|d| d.insert_temp(id, p.clone()));
            p
        }
    };
    let (mut close, mut print) = (false, false);
    egui::Window::new("Plot Preview").open(open).collapsible(false).resizable(false).show(ctx, |ui| {
        match &preview.shown {
            Ok((tex, info)) => {
                ui.label(info);
                let size = tex.size_vec2();
                let k = (760.0 / size.x).min(540.0 / size.y).min(1.0);
                ui.image((tex.id(), size * k));
            }
            Err(e) => {
                ui.label(e);
            }
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            print = ui.button("Print...").clicked();
            close = ui.button("Close").clicked();
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if close || print {
        *open = false;
    }
    let from_plot = ctx.data(|d| d.get_temp::<FromPlot>(egui::Id::unique(FROM_PLOT))).is_some();
    if !*open {
        ctx.data_mut(|d| {
            d.remove::<Preview>(id);
            d.remove::<FromPlot>(egui::Id::unique(FROM_PLOT));
        });
    }
    if from_plot && !*open {
        // Back to the Plot dialog with its settings; Print... plots them.
        crate::managers::set_flag(ctx, crate::managers::PLOT_NOW, print);
        app.ui.dialog = Some("plot".into());
    } else if print {
        app.start("plot");
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use cadcraft_engine::Session;
    use serde_json::json;

    use crate::{CadApp, Services};

    /// An app whose file dialogs record what they were asked and cancel.
    fn app() -> (CadApp, Rc<RefCell<Vec<String>>>) {
        let asked = Rc::new(RefCell::new(Vec::new()));
        let (a, b) = (asked.clone(), asked.clone());
        let services = Services {
            pick_open: Some(Box::new(move || {
                a.borrow_mut().push("open".into());
                None
            })),
            pick_save: Some(Box::new(move |name: &str| {
                b.borrow_mut().push(name.to_string());
                None
            })),
            ..Services::default()
        };
        (CadApp::new(Session::new(), services), asked)
    }

    #[test]
    fn typed_and_menu_commands_ask_json_calls_never_do() {
        let (mut app, asked) = app();
        app.start("export");
        app.cmdline("PUBLISH");
        app.start("imp");
        assert_eq!(*asked.borrow(), ["Drawing1.svg", "Drawing1.pdf", "open"]);
        assert!(app.ui.dialog.is_none());
        app.cmdline("pre");
        assert_eq!(app.ui.dialog.as_deref(), Some("preview"));
        app.ui.dialog = None;
        app.start("preview");
        assert_eq!(app.ui.dialog.as_deref(), Some("preview"));
        app.ui.dialog = None;

        let _ = app.run("export", json!({ "format": "svg" }));
        let _ = app.run("import", json!({}));
        let _ = app.run("publish", json!({ "layouts": [] }));
        let r = app.run("preview", json!({ "width": 64 })).unwrap();
        assert!(r["data"].is_string());
        assert_eq!(asked.borrow().len(), 3, "JSON calls never open a file dialog");
        assert!(app.ui.dialog.is_none(), "nor the preview window");

        let e = app.run("share", json!({})).unwrap_err();
        assert!(e.contains("not available yet"), "{e}");
    }

    /// DXFIN and DXFOUT ask with a file dialog like IMPORT and EXPORT (DXFOUT for a .dxf), and at
    /// the command line instead while FILEDIA is 0, as in AutoCAD (#2455).
    #[test]
    fn dxfin_and_dxfout_ask_with_a_dialog_unless_filedia_is_0() {
        let (mut app, asked) = app();
        app.cmdline("DXFIN");
        app.cmdline("dxfout");
        assert_eq!(*asked.borrow(), ["open", "Drawing1.dxf"]);
        assert!(app.session.running.is_none(), "a cancelled dialog starts nothing");

        app.session.settings.filedia = false;
        app.cmdline("DXFOUT");
        assert_eq!(asked.borrow().len(), 2, "no dialog with FILEDIA 0");
        assert_eq!(app.session.current_prompt().map(|p| p.message), Some("Enter file name".to_string()));
        app.session.cancel();
        app.cmdline("DXFIN");
        assert_eq!(app.session.current_prompt().map(|p| p.message), Some("Enter name of file to import".to_string()));
        app.session.cancel();

        // JSON calls never ask either way (this test has no file formats, so they fail instead).
        app.session.settings.filedia = true;
        let _ = app.run("dxfout", json!({}));
        let _ = app.run("dxfin", json!({}));
        assert_eq!(asked.borrow().len(), 2);
    }

    /// A name typed without .dxf in a dialog that doesn't add it (GTK) is written with .dxf; an
    /// existing file of that name, which the dialog never asked about, is not replaced.
    #[test]
    fn dxfout_never_replaces_a_dxf_the_dialog_did_not_ask_about() {
        let dir = std::env::temp_dir().join(format!("cadcraft-dxfout-dialog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let typed = dir.join("plan").to_string_lossy().to_string();
        let services = Services { pick_save: Some(Box::new(move |_: &str| Some(typed.clone()))), ..Services::default() };
        let mut app = CadApp::new(Session::new(), services);
        let e = super::route(&mut app, "dxfout").unwrap_err();
        assert!(!e.contains("already exists"), "nothing to replace: DXFOUT runs ({e})");

        std::fs::write(dir.join("plan.dxf"), "keep").unwrap();
        let e = super::route(&mut app, "dxfout").unwrap_err();
        assert!(e.contains("plan.dxf already exists"), "{e}");
        assert_eq!(std::fs::read_to_string(dir.join("plan.dxf")).unwrap(), "keep");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
