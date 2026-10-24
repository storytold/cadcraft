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
            // The Batch Plot window: which sheets, in what order, into one file or several.
            app.ui.dialog = Some("publish".into());
            Ok(Value::Null)
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

/// The preview's zoom (1 = the sheet fits the window) and a scroll position to apply.
#[derive(Clone)]
struct View {
    generation: u32,
    zoom: f32,
    offset: Option<egui::Vec2>,
    last_offset: egui::Vec2,
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
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(FROM_PLOT), FromPlot { params, info }));
    app.ui.dialog = Some("preview".into());
}

/// `img` made small enough for the graphics card's largest texture (egui refuses a larger one).
fn fit_texture(ctx: &egui::Context, img: image::RgbaImage) -> image::RgbaImage {
    let max = u32::try_from(ctx.input(|i| i.max_texture_side)).unwrap_or(2048).max(256);
    let (w, h) = (img.width(), img.height());
    if w <= max && h <= max {
        return img;
    }
    let k = f64::from(max) / f64::from(w.max(h));
    let nw = ((f64::from(w) * k).floor() as u32).clamp(1, max);
    let nh = ((f64::from(h) * k).floor() as u32).clamp(1, max);
    image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle)
}

/// Render the plot preview of the current layout (or model), or of the Plot dialog's settings,
/// as a texture.
fn render(app: &mut CadApp, ctx: &egui::Context) -> Result<(TextureHandle, String), String> {
    if let Some(f) = ctx.data(|d| d.get_temp::<FromPlot>(egui::Id::new(FROM_PLOT))) {
        let r = app.session.execute("plot", &f.params).map_err(|e| e.to_string())?;
        let png = r["data"].as_str().and_then(cadcraft_engine::cmd::file::base64_decode).ok_or("the preview has no image")?;
        let img = fit_texture(ctx, image::load_from_memory_with_format(&png, image::ImageFormat::Png).map_err(|e| e.to_string())?.to_rgba8());
        let size = [img.width() as usize, img.height() as usize];
        let tex = ctx.load_texture("plot-preview", egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR);
        return Ok((tex, f.info));
    }
    let r = app.session.execute("preview", &json!({ "width": 2400 })).map_err(|e| e.to_string())?;
    let png = r["data"].as_str().and_then(cadcraft_engine::cmd::file::base64_decode).ok_or("the preview has no image")?;
    let img = fit_texture(ctx, image::load_from_memory_with_format(&png, image::ImageFormat::Png).map_err(|e| e.to_string())?.to_rgba8());
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
    let view_id = id.with("view");
    let mut view = ctx.data(|d| d.get_temp::<View>(view_id)).filter(|v| v.generation == generation).unwrap_or(View {
        generation,
        zoom: 1.0,
        offset: None,
        last_offset: egui::Vec2::ZERO,
    });
    egui::Window::new(crate::tl!("Plot Preview"))
        .id(egui::Id::new("Plot Preview"))
        .open(open)
        .collapsible(false)
        .resizable(true)
        .default_size(egui::vec2(820.0, 640.0))
        .min_size(egui::vec2(360.0, 300.0))
        .show(ctx, |ui| {
            // Print, zoom in and out, fit, the zoom level and Close.
            let mut zoom_by: Option<f32> = None;
            ui.horizontal(|ui| {
                print = ui.button(crate::tl!("Print...")).clicked();
                ui.separator();
                if crate::icons::button(ui, crate::icons::Icon::ZoomIn, 22.0, crate::tl!("Zoom In"), false).clicked() {
                    zoom_by = Some(1.25);
                }
                if crate::icons::button(ui, crate::icons::Icon::ZoomOut, 22.0, crate::tl!("Zoom Out"), false).clicked() {
                    zoom_by = Some(0.8);
                }
                if crate::icons::button(ui, crate::icons::Icon::ZoomFit, 22.0, crate::tl!("Zoom to Fit"), false).clicked() {
                    view.zoom = 1.0;
                    view.offset = Some(egui::Vec2::ZERO);
                }
                ui.label(format!("{:.0}%", view.zoom * 100.0));
                ui.separator();
                close = ui.button(crate::tl!("Close")).clicked();
                if let Ok((_, info)) = &preview.shown {
                    ui.label(egui::RichText::new(info).weak());
                }
            });
            ui.separator();
            match &preview.shown {
                Ok((tex, _)) => {
                    let size = tex.size_vec2();
                    let avail = ui.available_size();
                    let fit = (avail.x / size.x).min(avail.y / size.y).max(1e-3);
                    let viewport = ui.available_rect_before_wrap();
                    let wheel = ui.ctx().input(|i| {
                        if viewport.contains(i.pointer.hover_pos().unwrap_or_default()) {
                            i.smooth_scroll_delta.y + i.zoom_delta().ln() * 200.0
                        } else {
                            0.0
                        }
                    });
                    let pointer = ui.ctx().pointer_hover_pos().filter(|p| viewport.contains(*p));
                    // The mouse wheel zooms about the cursor; the buttons about the middle.
                    let (factor, about) = match zoom_by {
                        Some(k) => (k, viewport.center()),
                        None if wheel.abs() > 0.0 && pointer.is_some() => ((wheel / 200.0).exp(), pointer.unwrap_or(viewport.center())),
                        None => (1.0, viewport.center()),
                    };
                    let old_zoom = view.zoom;
                    view.zoom = (view.zoom * factor).clamp(1.0 / 4.0, 16.0);
                    let mut area = egui::ScrollArea::both()
                        .id_salt("cc_plot_preview_scroll")
                        .auto_shrink([false, false])
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                        .scroll_source(egui::scroll_area::ScrollSource {
                            scroll_bar: true,
                            drag: egui::scroll_area::DragScroll::Never,
                            mouse_wheel: false,
                        });
                    if (view.zoom - old_zoom).abs() > f32::EPSILON {
                        // Keep the point under `about` where it is.
                        let ratio = view.zoom / old_zoom;
                        let rel = about - viewport.min;
                        let old = view.offset.unwrap_or(view.last_offset);
                        view.offset = Some(((old + rel) * ratio - rel).max(egui::Vec2::ZERO));
                    }
                    if let Some(o) = view.offset.take() {
                        area = area.scroll_offset(o);
                    }
                    let shown = size * fit * view.zoom;
                    // Scroll bars that stay visible (not the thin floating ones) once the sheet is larger.
                    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                    let mut pan = egui::Vec2::ZERO;
                    let out = area.show(ui, |ui| {
                        // Centred while smaller than the window.
                        let pad = ((ui.available_size() - shown) / 2.0).max(egui::Vec2::ZERO);
                        let (r, resp) = ui.allocate_exact_size(shown + pad * 2.0, egui::Sense::drag());
                        let img = egui::Rect::from_min_size(r.min + pad, shown);
                        ui.painter().rect_filled(img.translate(egui::vec2(3.0, 3.0)), 0.0, egui::Color32::from_black_alpha(90));
                        ui.painter().image(tex.id(), img, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                        if resp.hovered() || resp.dragged() {
                            ui.ctx().set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
                        }
                        // Dragging the sheet moves it with the pointer.
                        if resp.dragged() {
                            pan = resp.drag_delta();
                        }
                    });
                    view.last_offset = out.state.offset;
                    if pan != egui::Vec2::ZERO {
                        view.offset = Some((out.state.offset - pan).max(egui::Vec2::ZERO));
                        ui.ctx().request_repaint();
                    }
                }
                Err(e) => {
                    ui.label(e);
                }
            }
        });
    ctx.data_mut(|d| d.insert_temp(view_id, view));
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if close || print {
        *open = false;
    }
    let from_plot = ctx.data(|d| d.get_temp::<FromPlot>(egui::Id::new(FROM_PLOT))).is_some();
    if !*open {
        ctx.data_mut(|d| {
            d.remove::<Preview>(id);
            d.remove::<FromPlot>(egui::Id::new(FROM_PLOT));
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
        assert_eq!(*asked.borrow(), ["Drawing1.svg", "open"]);
        assert_eq!(app.ui.dialog.as_deref(), Some("publish"), "PUBLISH opens the Batch Plot window");
        app.ui.dialog = None;
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
        assert_eq!(asked.borrow().len(), 2, "JSON calls never open a file dialog");
        assert!(app.ui.dialog.is_none(), "nor the preview window");

        let e = app.run("share", json!({})).unwrap_err();
        assert!(e.contains("not available yet"), "{e}");
    }
}
