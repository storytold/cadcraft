//! EXPORT, IMPORT, PUBLISH and PREVIEW from a menu, the tool bar or the command line: the first
//! three ask for a file, PREVIEW shows the plot in a window. Calls with parameters (scripts, the
//! control channel, MCP) never get here and never open a dialog: they run the engine commands.

use std::sync::atomic::{AtomicU32, Ordering};

use egui::TextureHandle;
use serde_json::{Value, json};

use crate::{CadApp, Pick};

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
        "import" | "imp" => app
            .pick_then(Pick::Open, |app, path| app.run("import", json!({ "path": path })))
            .unwrap_or_else(|| no_picker(app, "IMPORT", r#"import {"path": "parts.dxf"}"#)),
        "publish" => app
            .pick_then(Pick::Save(format!("{stem}.pdf")), |app, path| app.run("publish", json!({ "path": path })))
            .unwrap_or_else(|| no_picker(app, "PUBLISH", "publish {}")),
        _ => app
            .pick_then(Pick::Save(format!("{stem}.svg")), |app, path| app.run("export", json!({ "path": path })))
            .unwrap_or_else(|| no_picker(app, "EXPORT", r#"export {"format": "svg"}"#)),
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

    use crate::{CadApp, Pick, Services};

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

    type Done = Box<dyn FnOnce(Option<String>) + Send>;

    /// An app whose host shows its dialogs without waiting (macOS, #1527): the dialogs asked
    /// for, and the callback that answers the open one.
    fn deferred_app() -> (CadApp, Rc<RefCell<Vec<Pick>>>, Rc<RefCell<Option<Done>>>) {
        let asked = Rc::new(RefCell::new(Vec::new()));
        let answer: Rc<RefCell<Option<Done>>> = Rc::default();
        let (log, slot) = (asked.clone(), answer.clone());
        let services = Services {
            pick_open: Some(Box::new(|| None)),
            pick_save: Some(Box::new(|_: &str| None)),
            start_pick: Some(Box::new(move |pick, done| {
                log.borrow_mut().push(pick);
                *slot.borrow_mut() = Some(done);
            })),
            ..Services::default()
        };
        (CadApp::new(Session::new(), services), asked, answer)
    }

    fn uid(app: &CadApp) -> u64 {
        app.session.state().unwrap().uid
    }

    /// Where the host shows its dialogs without waiting (macOS, #1527), the command acts on the
    /// path in a later frame, and no second dialog opens while one is up.
    #[test]
    fn a_deferred_dialog_acts_when_its_answer_arrives() {
        let (mut app, asked, answer) = deferred_app();
        let ctx = egui::Context::default();

        // Cancelled: nothing happens, and the next command may ask again.
        app.start("export");
        assert_eq!(*asked.borrow(), [Pick::Save("Drawing1.svg".into())]);
        answer.borrow_mut().take().unwrap()(None);
        app.finish_pick(&ctx);
        assert!(app.pending_pick.is_none());

        app.start("imp");
        app.start("publish");
        assert_eq!(asked.borrow().len(), 2, "no second dialog while one is open");
        app.finish_pick(&ctx);
        assert!(app.pending_pick.is_some() && app.status.is_none(), "nothing runs before the answer");
        let missing = std::env::temp_dir().join(format!("cadcraft-missing-{}.dxf", std::process::id()));
        answer.borrow_mut().take().unwrap()(Some(missing.to_string_lossy().to_string()));
        app.finish_pick(&ctx);
        assert!(app.pending_pick.is_none());
        assert!(app.status.is_some(), "IMPORT ran with the picked file and reported its error");
    }

    /// A dialog's answer acts on the drawing that asked for it, even when another drawing was
    /// made active before it arrived (a control-channel `document.switch` runs before the answer
    /// in the same frame): Save As must not write the other drawing to the chosen file.
    #[test]
    fn a_deferred_dialog_acts_on_the_drawing_that_asked() {
        let (mut app, _, answer) = deferred_app();
        let ctx = egui::Context::default();
        app.run("new", json!({})).unwrap();
        app.run("document.switch", json!({ "index": 0 })).unwrap();
        let first = uid(&app);
        let acted_on = Rc::new(RefCell::new(None));
        let seen = acted_on.clone();
        let r = app.pick_then(Pick::Save("Drawing1.dxf".into()), move |app, path| {
            *seen.borrow_mut() = Some((uid(app), path));
            Ok(json!(null))
        });
        assert_eq!(r, Some(Ok(json!(null))));

        app.run("document.switch", json!({ "index": 1 })).unwrap();
        assert_ne!(uid(&app), first);
        answer.borrow_mut().take().unwrap()(Some("a.dxf".into()));
        app.finish_pick(&ctx);
        assert_eq!(*acted_on.borrow(), Some((first, "a.dxf".to_string())));
        assert_eq!(app.session.active, 0, "the drawing that asked is shown again");
    }

    /// The drawing that asked was closed before the answer: nothing is done, and the status bar
    /// says so. A dialog that opens a drawing (OPEN, NEW from a template) still acts.
    #[test]
    fn a_deferred_dialog_for_a_closed_drawing_does_nothing() {
        let (mut app, _, answer) = deferred_app();
        let ctx = egui::Context::default();
        app.run("new", json!({})).unwrap();
        let ran = Rc::new(RefCell::new(0));
        let r1 = ran.clone();
        let _ = app.pick_then(Pick::Save("Drawing2.svg".into()), move |_, _| {
            *r1.borrow_mut() += 1;
            Ok(json!(null))
        });
        app.run("close", json!({ "index": 1 })).unwrap();
        answer.borrow_mut().take().unwrap()(Some("b.svg".into()));
        app.finish_pick(&ctx);
        assert_eq!(*ran.borrow(), 0, "the closed drawing's export did not run on another drawing");
        assert!(app.pending_pick.is_none());
        assert!(app.status.as_ref().is_some_and(|(s, _)| s.contains("closed") && s.contains("b.svg")), "{:?}", app.status);

        app.run("new", json!({})).unwrap();
        let r2 = ran.clone();
        let _ = app.pick_then_open(Pick::Open, move |_, _| {
            *r2.borrow_mut() += 1;
            Ok(json!(null))
        });
        app.run("close", json!({ "index": 1 })).unwrap();
        answer.borrow_mut().take().unwrap()(Some("c.dxf".into()));
        app.finish_pick(&ctx);
        assert_eq!(*ran.borrow(), 1, "OPEN goes on whichever drawing closed meanwhile");
    }
}
