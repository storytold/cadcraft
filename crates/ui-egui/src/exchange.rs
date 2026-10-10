//! EXPORT, IMPORT, PUBLISH and PREVIEW from a menu, the tool bar or the command line: the first
//! three ask for a file, PREVIEW shows the plot in a window. Calls with parameters (scripts, the
//! control channel, MCP) never get here and never open a dialog: they run the engine commands.

use std::sync::atomic::{AtomicU32, Ordering};

use egui::TextureHandle;
use serde_json::{Value, json};

use crate::CadApp;

/// Bumped each time PREVIEW is started, so the window renders the drawing as it is now.
static PREVIEW_GEN: AtomicU32 = AtomicU32::new(0);

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
        "import" | "imp" => {
            let Some(pick) = app.services.pick_open.as_ref() else { return no_picker(app, "IMPORT", r#"import {"path": "parts.dxf"}"#) };
            match pick() {
                Some(path) => app.run("import", json!({ "path": path })),
                None => Ok(Value::Null),
            }
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

/// Render the plot preview of the current layout (or model) as a texture.
fn render(app: &mut CadApp, ctx: &egui::Context) -> Result<(TextureHandle, String), String> {
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
    let id = egui::Id::new("cc_plot_preview");
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
    if !*open {
        ctx.data_mut(|d| d.remove::<Preview>(id));
    }
    if print {
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
}
