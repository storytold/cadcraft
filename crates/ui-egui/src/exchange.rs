//! EXPORT, IMPORT, PUBLISH and PREVIEW from a menu, the tool bar or the command line: the first
//! three ask for a file, PREVIEW shows the plot in a window. Calls with parameters (scripts, the
//! control channel, MCP) never get here and never open a dialog: they run the engine commands.

use std::sync::atomic::{AtomicU32, Ordering};

use egui::{Pos2, Rect, Sense, TextureHandle, Vec2, pos2, vec2};
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

/// How far the preview can be zoomed in, as a multiple of the whole sheet fitting its area.
const MAX_ZOOM: f32 = 24.0;
/// How long (seconds) the view must rest before the part in view is drawn again in more detail.
const SETTLE: f64 = 0.15;

/// How the sheet's image sits in the preview area: `zoom` 1 fits the whole sheet, `pan` moves the
/// image's centre from the area's centre (points).
#[derive(Clone, Copy, Debug, PartialEq)]
struct PanZoom {
    zoom: f32,
    pan: Vec2,
}

impl Default for PanZoom {
    /// The whole sheet in view.
    fn default() -> Self {
        PanZoom { zoom: 1.0, pan: Vec2::ZERO }
    }
}

impl PanZoom {
    /// Points per image pixel when the whole image fits `area`.
    fn fit(area: Rect, image: Vec2) -> f32 {
        (area.width() / image.x.max(1.0)).min(area.height() / image.y.max(1.0)).max(1e-6)
    }

    /// Where the whole image is drawn.
    fn image_rect(&self, area: Rect, image: Vec2) -> Rect {
        Rect::from_center_size(area.center() + self.pan, image * (Self::fit(area, image) * self.zoom))
    }

    /// Keep the zoom in range and the image over the area: it can't be dragged away, and is
    /// centred along an axis where it is smaller than the area.
    fn clamp(&mut self, area: Rect, image: Vec2) {
        self.zoom = if self.zoom.is_finite() { self.zoom.clamp(1.0, MAX_ZOOM) } else { 1.0 };
        let size = image * (Self::fit(area, image) * self.zoom);
        let room = |image: f32, area: f32| ((image - area) / 2.0).max(0.0);
        let (rx, ry) = (room(size.x, area.width()), room(size.y, area.height()));
        self.pan =
            if self.pan.x.is_finite() && self.pan.y.is_finite() { vec2(self.pan.x.clamp(-rx, rx), self.pan.y.clamp(-ry, ry)) } else { Vec2::ZERO };
    }

    /// Zoom by `factor` about the screen point `at`, which stays over the same point of the image.
    fn zoom_at(&mut self, area: Rect, image: Vec2, at: Pos2, factor: f32) {
        if !(factor.is_finite() && factor > 0.0) {
            return;
        }
        let centre = self.image_rect(area, image).center();
        let zoom = (self.zoom * factor).clamp(1.0, MAX_ZOOM);
        let moved = at + (centre - at) * (zoom / self.zoom);
        self.zoom = zoom;
        self.pan = moved - area.center();
        self.clamp(area, image);
    }
}

/// The part of the image drawn at `shown` that is inside `area`, as fractions of the image from its
/// top-left corner `[x0, y0, x1, y1]`; `None` when none of it is.
fn visible_crop(area: Rect, shown: Rect) -> Option<[f32; 4]> {
    let v = area.intersect(shown);
    if shown.width() < 1.0 || shown.height() < 1.0 || v.width() < 1.0 || v.height() < 1.0 {
        return None;
    }
    let (w, h) = (shown.width(), shown.height());
    Some([(v.min.x - shown.min.x) / w, (v.min.y - shown.min.y) / h, (v.max.x - shown.min.x) / w, (v.max.y - shown.min.y) / h])
}

/// `crop` grown by a quarter of its size on each side (within the sheet): what is drawn in detail,
/// so that a small pan stays sharp.
fn with_margin(c: [f32; 4]) -> [f32; 4] {
    let (mx, my) = ((c[2] - c[0]) / 4.0, (c[3] - c[1]) / 4.0);
    [(c[0] - mx).max(0.0), (c[1] - my).max(0.0), (c[2] + mx).min(1.0), (c[3] + my).min(1.0)]
}

/// Whether crop `outer` covers crop `inner`.
fn covers(outer: [f32; 4], inner: [f32; 4]) -> bool {
    outer[0] <= inner[0] + 1e-4 && outer[1] <= inner[1] + 1e-4 && outer[2] >= inner[2] - 1e-4 && outer[3] >= inner[3] - 1e-4
}

/// The sheet as plotted: the whole of it drawn once, and what the preview needs to draw parts of it
/// again in more detail.
#[derive(Clone)]
struct Shown {
    base: TextureHandle,
    info: String,
    /// The sheet's width and height in mm, as plotted (the orientation applied).
    sheet_mm: Vec2,
    /// The `plot` parameters of the whole sheet.
    params: Value,
}

/// A part of the sheet drawn at the resolution of the screen it is shown on.
#[derive(Clone)]
struct Detail {
    tex: TextureHandle,
    /// The part, as fractions of the sheet from its top-left corner.
    crop: [f32; 4],
    /// The width in points the whole sheet had on screen when it was drawn.
    sheet_width: f32,
}

impl Detail {
    /// Whether it is drawn at the scale the sheet has on screen now.
    fn matches(&self, sheet_width: f32) -> bool {
        sheet_width > 0.0 && (self.sheet_width / sheet_width - 1.0).abs() < 0.01
    }
}

#[derive(Clone)]
struct Preview {
    generation: u32,
    shown: Result<Shown, String>,
    view: PanZoom,
    detail: Option<Detail>,
    /// When the view, or the size the sheet is drawn at, last changed (seconds), to draw in detail
    /// once it rests.
    changed: f64,
    /// Where the sheet was drawn last: a resized window changes it with the view unchanged.
    drawn: Rect,
    /// The part and scale last drawn in detail, so a failure is not retried every frame.
    tried: Option<([i32; 4], i32)>,
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

/// Plot `params` (a PNG) and load it as a texture called `name`.
fn plot_texture(app: &mut CadApp, ctx: &egui::Context, params: &Value, name: &str) -> Result<TextureHandle, String> {
    let r = app.session.execute("plot", params).map_err(|e| e.to_string())?;
    let png = r["data"].as_str().and_then(cadcraft_engine::cmd::file::base64_decode).ok_or("the preview has no image")?;
    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png).map_err(|e| e.to_string())?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Ok(ctx.load_texture(name, egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR))
}

/// Render the plot preview of the tab being shown, or of the Plot dialog's settings, as it plots
/// (page setup, plot style table, lineweights): the whole sheet at about 1600 pixels.
fn render(app: &mut CadApp, ctx: &egui::Context) -> Result<Shown, String> {
    let (params, info) = match ctx.data(|d| d.get_temp::<FromPlot>(egui::Id::unique(FROM_PLOT))) {
        Some(f) => (f.params, f.info),
        None => crate::managers::current_preview(app)?,
    };
    let base = plot_texture(app, ctx, &params, "plot-preview")?;
    let mm = |k: &str| params.get(k).and_then(Value::as_f64).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(210.0) as f32;
    let (a, b) = (mm("width"), mm("height"));
    let (long, short) = (a.max(b), a.min(b));
    let sheet_mm = if params.get("landscape").and_then(Value::as_bool) == Some(true) { vec2(long, short) } else { vec2(short, long) };
    Ok(Shown { base, info, sheet_mm, params })
}

/// Draw the part `crop` of the sheet again at the resolution of the screen, where the whole sheet is
/// `sheet_width` points wide.
fn render_detail(app: &mut CadApp, ctx: &egui::Context, shown: &Shown, crop: [f32; 4], sheet_width: f32) -> Result<Detail, String> {
    // One image pixel per screen pixel: the sheet's pixel width over its width in inches.
    let pixels = f64::from(sheet_width) * f64::from(ctx.pixels_per_point());
    let dpi = (pixels / (f64::from(shown.sheet_mm.x) / 25.4)).clamp(50.0, 2400.0);
    let mut params = shown.params.clone();
    if let Some(o) = params.as_object_mut() {
        o.insert("resolution".into(), json!(dpi));
        o.insert("crop".into(), json!(crop));
    }
    let tex = plot_texture(app, ctx, &params, "plot-preview-detail")?;
    Ok(Detail { tex, crop, sheet_width })
}

/// The Plot Preview window: the sheet as it plots, with Print and Close. The mouse wheel zooms
/// about the pointer, dragging pans and a double click shows the whole sheet again; the part in
/// view is drawn again at the screen's resolution once the view rests.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let id = egui::Id::unique("cc_plot_preview");
    let generation = PREVIEW_GEN.load(Ordering::Relaxed);
    let now = ctx.input(|i| i.time);
    let cached = ctx.data(|d| d.get_temp::<Preview>(id)).filter(|p| p.generation == generation);
    let mut pv = match cached {
        Some(p) => p,
        None => {
            Preview { generation, shown: render(app, ctx), view: PanZoom::default(), detail: None, changed: now, drawn: Rect::NOTHING, tried: None }
        }
    };
    let (mut close, mut print) = (false, false);
    // What to draw in detail after the window: the part and the sheet's width on screen.
    let mut want: Option<([f32; 4], f32)> = None;
    egui::Window::new("Plot Preview").open(open).collapsible(false).resizable(true).default_size(vec2(900.0, 680.0)).show(ctx, |ui| {
        match &pv.shown {
            Ok(shown) => {
                let image = shown.base.size_vec2();
                let mut view = pv.view;
                let (mut zoom_out, mut fit, mut zoom_in) = (false, false, false);
                ui.horizontal(|ui| {
                    ui.label(&shown.info);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(format!("{:.0}%", view.zoom * 100.0));
                        fit = ui.small_button("Fit").on_hover_text("The whole sheet (or double-click)").clicked();
                        zoom_in = ui.small_button("+").on_hover_text("Zoom in (or the mouse wheel)").clicked();
                        zoom_out = ui.small_button("−").on_hover_text("Zoom out").clicked();
                    });
                });
                let size = vec2(ui.available_width().max(160.0), (ui.available_height() - 36.0).max(160.0));
                let (area, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
                let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| area.contains(*p));
                if resp.hovered() {
                    let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
                    if let Some(at) = hover {
                        view.zoom_at(area, image, at, (scroll * 0.002).exp() * pinch);
                    }
                }
                if resp.dragged() {
                    view.pan += resp.drag_delta();
                }
                if resp.double_clicked() || fit {
                    view = PanZoom::default();
                }
                if zoom_in {
                    view.zoom_at(area, image, area.center(), 1.5);
                }
                if zoom_out {
                    view.zoom_at(area, image, area.center(), 1.0 / 1.5);
                }
                view.clamp(area, image);
                if resp.dragged() || resp.hovered() {
                    ui.ctx().set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
                }
                let sheet = view.image_rect(area, image);
                let painter = ui.painter_at(area);
                painter.rect_filled(area, 0.0, egui::Color32::from_gray(70));
                let full = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
                painter.image(shown.base.id(), sheet, full, egui::Color32::WHITE);
                if let Some(d) = pv.detail.as_ref().filter(|d| d.matches(sheet.width())) {
                    let [x0, y0, x1, y1] = d.crop;
                    let (w, h) = (sheet.width(), sheet.height());
                    let part = Rect::from_min_max(sheet.min + vec2(x0 * w, y0 * h), sheet.min + vec2(x1 * w, y1 * h));
                    painter.image(d.tex.id(), part, full, egui::Color32::WHITE);
                }
                painter.rect_stroke(sheet, 0.0, egui::Stroke::new(1.0, egui::Color32::from_gray(120)), egui::StrokeKind::Outside);
                // A change of the view or of the size the sheet is drawn at (the window was resized)
                // restarts the wait, so the detail is not drawn again at every step of a resize.
                if view != pv.view || sheet != pv.drawn {
                    pv.changed = now;
                    pv.view = view;
                    pv.drawn = sheet;
                }
                // Drawn in detail when the sheet is shown larger than its image, once the view rests.
                let larger = sheet.width() * ui.ctx().pixels_per_point() > image.x * 1.1;
                if let Some(crop) = visible_crop(area, sheet).filter(|_| larger) {
                    let covered = pv.detail.as_ref().is_some_and(|d| d.matches(sheet.width()) && covers(d.crop, crop));
                    let key = (crop.map(|v| (v * 1000.0) as i32), sheet.width() as i32);
                    if !covered && pv.tried != Some(key) {
                        if now - pv.changed >= SETTLE {
                            want = Some((with_margin(crop), sheet.width()));
                            pv.tried = Some(key);
                        } else {
                            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(SETTLE));
                        }
                    }
                } else {
                    pv.detail = None;
                }
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
    if let (Some((crop, width)), Ok(shown)) = (want, pv.shown.as_ref()) {
        // A failure keeps the sheet as it is and is not tried again for this view.
        pv.detail = render_detail(app, ctx, shown, crop, width).ok();
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(id, pv));
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

    use egui::{Rect, Vec2, pos2, vec2};

    use super::{MAX_ZOOM, PanZoom, covers, visible_crop, with_margin};

    /// The preview area the zoom tests use.
    fn area() -> Rect {
        Rect::from_min_size(pos2(10.0, 20.0), vec2(800.0, 600.0))
    }

    const IMAGE: Vec2 = vec2(1600.0, 1000.0);

    /// The sheet fits the area and is centred until zoomed.
    #[test]
    fn the_sheet_fits_the_area_and_is_centred_until_zoomed() {
        let r = PanZoom::default().image_rect(area(), IMAGE);
        assert!((r.width() - 800.0).abs() < 1e-3 && (r.height() - 500.0).abs() < 1e-3, "fits by width: {r:?}");
        assert!((r.center() - area().center()).length() < 1e-3);
    }

    /// Zooming keeps the image point under the pointer in place.
    #[test]
    fn zoom_keeps_the_point_under_the_pointer() {
        let mut v = PanZoom::default();
        let at = pos2(430.0, 330.0);
        let r = v.image_rect(area(), IMAGE);
        let u = ((at.x - r.min.x) / r.width(), (at.y - r.min.y) / r.height());
        v.zoom_at(area(), IMAGE, at, 3.0);
        assert!((v.zoom - 3.0).abs() < 1e-5);
        let r = v.image_rect(area(), IMAGE);
        let now = pos2(r.min.x + u.0 * r.width(), r.min.y + u.1 * r.height());
        assert!((now - at).length() < 0.01, "the point under the pointer stayed: {now:?} vs {at:?}");
        // And back out: the whole sheet again, centred.
        v.zoom_at(area(), IMAGE, at, 1.0 / 3.0);
        assert_eq!(v, PanZoom::default());
    }

    /// The zoom is limited and the sheet stays over the area.
    #[test]
    fn zoom_is_limited_and_the_sheet_stays_over_the_area() {
        let mut v = PanZoom::default();
        v.zoom_at(area(), IMAGE, area().center(), 1e6);
        assert_eq!(v.zoom, MAX_ZOOM);
        v.zoom_at(area(), IMAGE, area().center(), 1e-6);
        assert_eq!(v.zoom, 1.0, "no further out than the whole sheet");
        // Dragged far away, the sheet still covers the area along the axes where it is larger.
        v.zoom_at(area(), IMAGE, area().center(), 4.0);
        v.pan = vec2(1e6, -1e6);
        v.clamp(area(), IMAGE);
        let r = v.image_rect(area(), IMAGE);
        assert!(
            r.min.x <= area().min.x + 1e-3 && r.max.x >= area().max.x - 1e-3 && r.min.y <= area().min.y + 1e-3 && r.max.y >= area().max.y - 1e-3,
            "{r:?}"
        );
        // Nonsense values change nothing or reset.
        let before = v;
        v.zoom_at(area(), IMAGE, area().center(), f32::NAN);
        v.zoom_at(area(), IMAGE, area().center(), -2.0);
        assert_eq!(v, before);
        v.pan = vec2(f32::NAN, 0.0);
        v.zoom = f32::INFINITY;
        v.clamp(area(), IMAGE);
        assert!(v.zoom == 1.0 && v.pan == Vec2::ZERO);
    }

    /// The part in view is what is drawn again in detail.
    #[test]
    fn the_visible_part_is_what_is_drawn_again_in_detail() {
        // Whole sheet in view: all of it.
        let whole = PanZoom::default().image_rect(area(), IMAGE);
        assert_eq!(visible_crop(area(), whole), Some([0.0, 0.0, 1.0, 1.0]));
        // Zoomed 4x on the centre: the middle eighth... of each side.
        let mut v = PanZoom::default();
        v.zoom_at(area(), IMAGE, area().center(), 4.0);
        let c = visible_crop(area(), v.image_rect(area(), IMAGE)).unwrap();
        assert!((c[0] - 0.375).abs() < 1e-4 && (c[2] - 0.625).abs() < 1e-4, "{c:?}");
        assert!(c[1] > 0.0 && c[3] < 1.0 && c[1] < c[3]);
        // The detail drawn covers it with a margin all round, within the sheet.
        let d = with_margin(c);
        assert!(covers(d, c) && !covers(c, d));
        assert!(d.iter().all(|v| (0.0..=1.0).contains(v)));
        assert_eq!(with_margin([0.0, 0.0, 1.0, 1.0]), [0.0, 0.0, 1.0, 1.0], "the margin stays inside the sheet");
        // Nothing of the sheet in the area: no part.
        let away = Rect::from_min_size(pos2(5000.0, 5000.0), vec2(100.0, 100.0));
        assert_eq!(visible_crop(area(), away), None);
    }

    /// The red-looking pixels of a plotted PNG, and its size.
    fn red_pixels(png: &[u8]) -> (usize, u32, u32) {
        let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).unwrap().to_rgba8();
        let red = img.pixels().filter(|p| p.0[0] > 180 && p.0[1] < 90 && p.0[2] < 90).count();
        (red, img.width(), img.height())
    }

    /// The preview draws the page setup's plot style table, and `None` overrides it.
    #[test]
    fn the_preview_draws_the_plot_style_table_of_the_page_setup() {
        let (mut app, _) = app();
        app.run("color", json!({ "color": 1 })).unwrap();
        app.run("line", json!({ "points": [[0, 0], [100, 60]] })).unwrap();
        app.run("mview", json!({ "layout": "Layout1" })).unwrap();
        app.run("layout.set", json!({ "name": "Layout1" })).unwrap();
        // The real plotter is called directly: the plot hook is global to the test process, which
        // other tests replace.
        let plot = |app: &CadApp, params: &serde_json::Value| {
            cadcraft_io::plotter::plot(app.session.doc().unwrap(), &cadcraft_doc::Space::Paper("Layout1".into()), params).unwrap()
        };
        let (params, info) = crate::managers::current_preview(&app).unwrap();
        assert!(info.starts_with("Layout1"), "{info}");
        let (plain, w, h) = red_pixels(&plot(&app, &params));
        assert!(plain > 50, "without a table the red line is drawn red ({plain} red pixels)");
        // The page setup's table is what the preview shows, as it is what plots.
        app.run("pagesetup", json!({ "layout": "Layout1", "plotStyleTable": "monochrome.ctb" })).unwrap();
        let (mut params, _) = crate::managers::current_preview(&app).unwrap();
        assert_eq!(red_pixels(&plot(&app, &params)).0, 0, "monochrome.ctb prints it black");
        // The Plot dialog's "None" is sent as such, so it overrides the layout's own table.
        params["plotStyleTable"] = json!("None");
        assert!(red_pixels(&plot(&app, &params)).0 > 50, "None overrides the page setup's table");
        // A crop is that part of the sheet, at the same scale.
        params["crop"] = json!([0.25, 0.25, 0.75, 0.75]);
        let (_, cw, ch) = red_pixels(&plot(&app, &params));
        assert!((i64::from(cw) - i64::from(w) / 2).abs() <= 1 && (i64::from(ch) - i64::from(h) / 2).abs() <= 1, "{cw}x{ch} of {w}x{h}");
    }
}
