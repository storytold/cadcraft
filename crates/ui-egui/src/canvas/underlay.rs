//! PDF underlays' pages, from the display list (model space's seen through a layout's viewports
//! included, clipped to them), drawn in the drawing's order (DRAWORDER): each page is read once (through the engine), kept as
//! a texture and drawn as a quad at the underlay's corners. Its paper is transparent:
//! only what the page draws covers the background, faded toward it. With "adjust
//! colors" on a dark background the page is kept with its lightness turned over; its contrast
//! and monochrome are applied first.
//!
//! Zoomed in past the whole page's pixels, or out well below them (a texture shrunk several
//! times skips its pixels and loses thin lines), the visible part of the page (with a margin) is
//! drawn again at the screen's resolution once the view has stayed still for a moment, on a
//! worker thread (at once on the web, which has no threads), and shown instead of the whole page
//! while it covers the view.

use std::collections::HashMap;

use cadcraft_color::Rgb;
use cadcraft_doc::Handle;
use cadcraft_geom::{Bounds2, Vec2};
use cadcraft_render::ListUnderlay;
use cadcraft_render::clip::PolyClip;
use egui::{Color32, Mesh, Shape, TextureHandle, TextureOptions, pos2};

use super::Xf;
use crate::CadApp;

/// The longest side, in pixels, a page is drawn at for the canvas.
const PAGE_PX: u32 = 3072;
/// Most pages kept as textures at once.
const MAX_PAGES: usize = 16;
/// The longest side, in pixels, of a sharper part of a page.
const DETAIL_PX: u32 = 4096;
/// Seconds the view stays still before the visible part is drawn sharper.
const SETTLE_S: f64 = 0.25;
/// Seconds a sharper part is kept once no underlay drawn shows it (scrolled away, deleted,
/// another drawing): each can hold a texture of up to `DETAIL_PX` squared pixels.
const DETAIL_KEEP_S: f64 = 5.0;
/// Seconds before a part whose drawing failed is asked for again (the view still on it): each
/// attempt reads the whole PDF again.
const DETAIL_RETRY_S: f64 = 5.0;

type Pixels = Result<(u32, u32, Vec<u8>), String>;

/// How a page's pixels are shown: lightness turned over, contrast (0..=100), monochrome.
type Look = (bool, u8, bool);

/// The part of a cache key telling apart the same page with different PDF layers hidden.
fn layers_key(layers: &[(String, bool)]) -> String {
    layers.iter().map(|(n, on)| format!("{}{n}\u{1f}", if *on { '+' } else { '-' })).collect()
}

/// Show `rgba` (a page's straight pixels) with `look`.
fn shade((turned, contrast, monochrome): Look, rgba: &mut [u8]) {
    cadcraft_doc::PdfUnderlay { contrast, monochrome, ..cadcraft_doc::PdfUnderlay::default() }.tone(rgba);
    if turned {
        cadcraft_engine::cmd::underlay::turn_lightness(rgba);
    }
}

/// A sharper part of one underlay's page: the part shown (page fractions `[left, bottom, right,
/// top]`), the screen pixels per page width it was drawn for, its texture, the part being drawn
/// (its pixels to come, part and density), and when (egui time) it was last drawn on screen.
struct Detail {
    region: [f64; 4],
    density: f64,
    shown: Option<TextureHandle>,
    pending: Option<(std::sync::mpsc::Receiver<Pixels>, [f64; 4], f64)>,
    used: f64,
    /// The part and density whose drawing failed last, and when.
    failed: Option<([f64; 4], f64, f64)>,
}

/// A page being read on a worker thread, or read: its texture, or the error, kept so it is not
/// read again every frame.
enum Page {
    Reading(std::sync::mpsc::Receiver<Pixels>),
    Read(Result<TextureHandle, String>),
}

/// Pages by drawing and stored file, page number and how they are shown.
#[derive(Default)]
pub(crate) struct Cache {
    pages: HashMap<(String, u32, Look), Page>,
    /// Sharper parts, by underlay and its place in the list (and the file, page and look they show).
    details: HashMap<(Handle, usize, String, u32, Look), Detail>,
    /// The view last drawn and since when (egui time) it hasn't moved.
    view: Option<(Bounds2, f64)>,
}

impl Cache {
    /// The page's texture once it has been read. The first call starts reading it on a worker
    /// thread (at once on the web, which has no threads), so the canvas doesn't stop while a big
    /// page is drawn: until then the underlay shows its frame only.
    fn texture(
        &mut self,
        ctx: &egui::Context,
        path: &str,
        document: Option<&str>,
        page: u32,
        look: Look,
        layers: &[(String, bool)],
    ) -> Result<&TextureHandle, &str> {
        // A relative path means a different file in another drawing's folder; the PDF's layers
        // shown or hidden make another picture of the page.
        let key = (format!("{}|{path}|{}", document.unwrap_or(""), layers_key(layers)), page, look);
        if !self.pages.contains_key(&key) {
            if self.pages.len() >= MAX_PAGES {
                self.pages.clear();
            }
            let (path_owned, doc, layers_owned) = (path.to_string(), document.map(str::to_string), layers.to_vec());
            let (tx, rx) = std::sync::mpsc::channel::<Pixels>();
            let job = move || {
                let r = cadcraft_engine::cmd::underlay::underlay_pixels(&path_owned, doc.as_deref(), page, PAGE_PX, &layers_owned)
                    .map_err(|e| e.to_string())
                    .map(|(w, h, mut rgba)| {
                        shade(look, &mut rgba);
                        (w, h, rgba)
                    });
                let _ = tx.send(r);
            };
            #[cfg(not(target_arch = "wasm32"))]
            let started = std::thread::Builder::new().name("cadcraft-pdf".into()).spawn(job).is_ok();
            #[cfg(target_arch = "wasm32")]
            let started = {
                job();
                true
            };
            let state = if started { Page::Reading(rx) } else { Page::Read(Err("the page could not be read".to_string())) };
            self.pages.insert(key.clone(), state);
        }
        if let Some(Page::Reading(rx)) = self.pages.get(&key) {
            let read = match rx.try_recv() {
                Ok(r) => r.and_then(|(w, h, rgba)| {
                    let size = [usize::try_from(w).unwrap_or(0), usize::try_from(h).unwrap_or(0)];
                    if size[0].checked_mul(size[1]).and_then(|n| n.checked_mul(4)) != Some(rgba.len()) {
                        return Err("the page was drawn with a wrong size".to_string());
                    }
                    let image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                    Ok(ctx.load_texture(format!("pdf-underlay {path} {page}"), image, TextureOptions::LINEAR))
                }),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                    return Err("being read");
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("the page could not be read".to_string()),
            };
            if let Err(e) = &read {
                log::warn!("PDF underlay {path} page {page}: {e}");
            }
            self.pages.insert(key.clone(), Page::Read(read));
        }
        match self.pages.get(&key) {
            Some(Page::Read(Ok(t))) => Ok(t),
            Some(Page::Read(Err(e))) => Err(e.as_str()),
            _ => Err("being read"),
        }
    }
}

/// One underlay's page to draw this frame.
struct Item {
    /// Its index in the display list's underlays.
    index: usize,
    handle: Handle,
    path: String,
    page: u32,
    look: Look,
    /// The PDF's layers shown or hidden.
    layers: Vec<(String, bool)>,
    fade: f32,
    /// Where the page is in the list: lower-left corner and width/height vectors.
    origin: Vec2,
    axes: (Vec2, Vec2),
    /// The part drawn (clipped to a viewport), as triangles in list units, and as page
    /// fractions `[left, bottom, right, top]`.
    tris: Vec<Vec2>,
    shown: [f64; 4],
}

impl Item {
    /// Page fractions of list point `p`.
    fn fraction(&self, p: Vec2) -> Vec2 {
        let (u, v) = self.axes;
        let det = u.x * v.y - u.y * v.x;
        let d = p - self.origin;
        let f = Vec2::new((d.x * v.y - d.y * v.x) / det, (u.x * d.y - u.y * d.x) / det);
        if f.is_finite() { f } else { Vec2::ZERO }
    }
}

/// The pages of PDF underlays `pages` (a display list's, seen through `xf`) on background `bg`,
/// each with its index in `pages`, for the canvas to draw at its place among the objects.
/// `outline`: a clipped viewport's outline in list units, which the pages are clipped to too
/// (the GPU's stencil doesn't reach them).
pub(crate) fn shapes(
    app: &mut CadApp,
    painter: &egui::Painter,
    xf: &Xf,
    pages: &[ListUnderlay],
    outline: Option<&PolyClip>,
    bg: Rgb,
) -> Vec<(usize, Shape)> {
    let mut out = Vec::new();
    if pages.is_empty() {
        return out;
    }
    let dark = u32::from(bg.0) * 299 + u32::from(bg.1) * 587 + u32::from(bg.2) * 114 < 128_000;
    let Ok(st) = app.session.state() else { return out };
    let view = xf.world_bounds();
    let mut items = Vec::new();
    for (index, lu) in pages.iter().enumerate() {
        let Some(u) = cadcraft_render::pdf_underlay(&st.doc, lu.handle) else { continue };
        let mut tris = lu.triangles();
        if let Some(c) = outline {
            tris = c.clip_triangles(&tris);
        }
        if tris.is_empty() || !Bounds2::from_points(tris.iter().copied()).intersects(&view) {
            continue;
        }
        let Some(shown) = lu.shown() else { continue };
        items.push(Item {
            index,
            handle: lu.handle,
            path: u.path.clone(),
            page: u.page,
            look: (u.adjust_colors && dark, u.contrast, u.monochrome),
            layers: u.layer_visibility(),
            fade: f32::from(u.fade.min(cadcraft_doc::UNDERLAY_MAX_FADE)) / 100.0,
            origin: lu.origin,
            axes: (lu.u, lu.v),
            tris,
            shown,
        });
    }
    let document = st.path.clone();
    let ctx = painter.ctx().clone();
    let now = ctx.input(|i| i.time);
    let cache = &mut app.canvas.underlays;
    let settled = match cache.view {
        Some((v, since)) if v == view => now - since >= SETTLE_S,
        _ => {
            cache.view = Some((view, now));
            false
        }
    };
    let ppp = f64::from(ctx.pixels_per_point());
    // A page wants a sharper part: one more frame once the view has settled draws it.
    let mut want_sharp = false;
    for it in &items {
        let Ok(tex) = cache.texture(&ctx, &it.path, document.as_deref(), it.page, it.look, &it.layers) else { continue };
        let base = (tex.id(), f64::from(tex.size()[0].max(1) as u32));
        // Faded: the page's content shows through to the background (its paper is transparent).
        let tint = Color32::WHITE.gamma_multiply(1.0 - it.fade);
        // Screen pixels per page width: away from the texture's own, draw the visible part at it.
        let density = it.axes.0.len() * xf.scale * ppp;
        let key = (it.handle, it.index, format!("{}|{}|{}", document.as_deref().unwrap_or(""), it.path, layers_key(&it.layers)), it.page, it.look);
        let mut sharp = None;
        if redrawn(density, base.1) {
            if let Some(region) = visible_region(it, &view, 0.25) {
                want_sharp = true;
                cache.detail(&ctx, &key, it, document.as_deref(), region, density, settled, now);
            }
            // The sharper part replaces the whole page while it covers the view.
            if let (Some(d), Some(seen)) = (cache.details.get(&key), visible_region(it, &view, 0.0))
                && let Some(t) = &d.shown
                && d.region[0] <= seen[0] + 1e-9
                && d.region[1] <= seen[1] + 1e-9
                && d.region[2] >= seen[2] - 1e-9
                && d.region[3] >= seen[3] - 1e-9
            {
                sharp = Some((t.id(), d.region));
            }
        } else {
            cache.details.remove(&key);
        }
        let (texture, region) = sharp.unwrap_or((base.0, [0.0, 0.0, 1.0, 1.0]));
        out.push((it.index, Shape::mesh(mesh(texture, it, region, xf, tint))));
    }
    if !settled && (want_sharp || cache.details.values().any(|d| d.shown.is_some() || d.pending.is_some())) {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(SETTLE_S));
    }
    // Not by the keys drawn in this call: the viewports' pages and the main list's are drawn in
    // two calls a frame, which would drop each other's.
    cache.forget_details(now);
    out
}

/// Whether a page `texture_px` wide, shown `density` screen pixels wide, is drawn again at the
/// screen's resolution: zoomed in, the texture's pixels show; zoomed out, a texture shrunk without
/// averaging skips pixels and thin lines (a frame, letters) vanish.
fn redrawn(density: f64, texture_px: f64) -> bool {
    density > texture_px * 1.3 || density < texture_px / 1.3
}

/// The part of item `it`'s page in `view`, with a margin of `margin` times its size each side,
/// in page fractions `[left, bottom, right, top]` clamped to the page.
fn visible_region(it: &Item, view: &Bounds2, margin: f64) -> Option<[f64; 4]> {
    let (u, v) = it.axes;
    let det = u.x * v.y - u.y * v.x;
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let frac = |p: Vec2| {
        let d = p - it.origin;
        Vec2::new((d.x * v.y - d.y * v.x) / det, (u.x * d.y - u.y * d.x) / det)
    };
    let b = Bounds2::from_points(view.corners().map(frac));
    let (mx, my) = (b.width() * margin, b.height() * margin);
    let [l, bt, r, t] = it.shown;
    let r = [(b.min.x - mx).max(l), (b.min.y - my).max(bt), (b.max.x + mx).min(r), (b.max.y + my).min(t)];
    (r.iter().all(|x| x.is_finite()) && r[2] > r[0] && r[3] > r[1]).then_some(r)
}

impl Cache {
    /// Drop the sharper parts not drawn for [`DETAIL_KEEP_S`] seconds at egui time `now`.
    fn forget_details(&mut self, now: f64) {
        self.details.retain(|_, d| now - d.used < DETAIL_KEEP_S);
    }

    /// Keep the sharper part of `it` up to date: when the view has settled and the part shown
    /// doesn't cover `want` at about `density`, draw it again.
    #[allow(clippy::too_many_arguments)]
    fn detail(
        &mut self,
        ctx: &egui::Context,
        key: &(Handle, usize, String, u32, Look),
        it: &Item,
        document: Option<&str>,
        want: [f64; 4],
        density: f64,
        settled: bool,
        now: f64,
    ) {
        let d = self.details.entry(key.clone()).or_insert(Detail { region: want, density: 0.0, shown: None, pending: None, used: now, failed: None });
        d.used = now;
        if let Some((rx, region, drawn_at)) = &d.pending {
            let (region, drawn_at) = (*region, *drawn_at);
            let ok = match rx.try_recv() {
                Ok(Ok((w, h, mut rgba))) => {
                    shade(it.look, &mut rgba);
                    let size = [w as usize, h as usize];
                    // The part and density go with the texture drawn for them.
                    let fits = size[0].checked_mul(size[1]).and_then(|n| n.checked_mul(4)) == Some(rgba.len());
                    if fits {
                        let image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                        d.shown = Some(ctx.load_texture(format!("pdf-underlay-detail {}", it.path), image, TextureOptions::LINEAR));
                        (d.region, d.density) = (region, drawn_at);
                    }
                    Some(fits)
                }
                Ok(Err(e)) => {
                    log::warn!("PDF underlay {} page {}: {e}", it.path, it.page);
                    Some(false)
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                    None
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(false),
            };
            if let Some(ok) = ok {
                d.pending = None;
                d.failed = (!ok).then_some((region, drawn_at, now));
            }
        }
        let inner = |r: [f64; 4]| {
            // The view itself (the part without its margin) is inside `r`.
            let (mx, my) = ((want[2] - want[0]) / 6.0, (want[3] - want[1]) / 6.0);
            r[0] <= want[0] + mx + 1e-9 && r[1] <= want[1] + my + 1e-9 && r[2] >= want[2] - mx - 1e-9 && r[3] >= want[3] - my - 1e-9
        };
        let good = d.shown.is_some() && inner(d.region) && (0.7..=1.5).contains(&(density / d.density.max(1e-9)));
        // A part that just failed isn't asked for again every frame; another part or density is.
        let failed = d.failed.is_some_and(|(r, at, when)| r == want && at == density && now - when < DETAIL_RETRY_S);
        if good || !settled || d.pending.is_some() || failed {
            return;
        }
        // Pixels for the part at the screen's density, the longest side at most DETAIL_PX.
        let (u, v) = it.axes;
        let ratio = if u.len() > 0.0 { v.len() / u.len() } else { 1.0 };
        let mut w = (want[2] - want[0]) * density;
        let mut h = (want[3] - want[1]) * density * ratio;
        let k = (f64::from(DETAIL_PX) / w.max(h)).min(1.0);
        w = (w * k).round().max(1.0);
        h = (h * k).round().max(1.0);
        let (path, doc, page, layers) = (it.path.clone(), document.map(str::to_string), it.page, it.layers.clone());
        let (tx, rx) = std::sync::mpsc::channel::<Pixels>();
        let job = move || {
            let r = cadcraft_engine::cmd::underlay::underlay_region_pixels(&path, doc.as_deref(), page, want, w as u32, h as u32, &layers)
                .map_err(|e| e.to_string());
            let _ = tx.send(r);
        };
        #[cfg(not(target_arch = "wasm32"))]
        if std::thread::Builder::new().name("cadcraft-pdf".into()).spawn(job).is_err() {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        job();
        d.pending = Some((rx, want, density));
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}

/// Item `it`'s triangles textured with `texture`, which shows the page's part `region` (page
/// fractions `[left, bottom, right, top]`).
fn mesh(texture: egui::TextureId, it: &Item, [l, b, r, t]: [f64; 4], xf: &Xf, color: Color32) -> Mesh {
    let mut m = Mesh::with_texture(texture);
    let (w, h) = ((r - l).max(1e-12), (t - b).max(1e-12));
    for p in &it.tris {
        let f = it.fraction(*p);
        let uv = pos2(((f.x - l) / w) as f32, (1.0 - (f.y - b) / h) as f32);
        m.vertices.push(egui::epaint::Vertex { pos: xf.to_screen(*p), uv, color });
    }
    let n = u32::try_from(it.tris.len() / 3 * 3).unwrap_or(0);
    m.indices.extend(0..n);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item() -> Item {
        Item {
            index: 0,
            handle: Handle(1),
            path: "plan.pdf".into(),
            page: 1,
            look: (false, 0, false),
            layers: Vec::new(),
            fade: 0.0,
            origin: Vec2::ZERO,
            axes: (Vec2::new(297.0, 0.0), Vec2::new(0.0, 210.0)),
            tris: Vec::new(),
            shown: [0.0, 0.0, 1.0, 1.0],
        }
    }

    fn texture(ctx: &egui::Context) -> TextureHandle {
        ctx.load_texture("t", egui::ColorImage::from_rgba_unmultiplied([1, 1], &[0, 0, 0, 255]), TextureOptions::LINEAR)
    }

    #[test]
    fn a_part_being_drawn_leaves_the_part_shown_with_its_own_region() {
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let key = (Handle(1), 0, "plan.pdf".to_string(), 1, (false, 0, false));
        let old = [0.0, 0.0, 0.2, 0.2];
        cache
            .details
            .insert(key.clone(), Detail { region: old, density: 1000.0, shown: Some(texture(&ctx)), pending: None, used: 0.0, failed: None });
        // Panned to another part: a new part is asked for.
        cache.detail(&ctx, &key, &item(), None, [0.6, 0.6, 0.8, 0.8], 1000.0, true, 1.0);
        let d = cache.details.get(&key).unwrap();
        assert!(d.pending.is_some(), "the new part is being drawn");
        assert_eq!(d.region, old, "the texture shown keeps the region it was drawn for");
    }

    #[test]
    fn a_part_that_failed_is_not_asked_for_again_every_frame() {
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let key = (Handle(1), 0, "plan.pdf".to_string(), 1, (false, 0, false));
        let want = [0.6, 0.6, 0.8, 0.8];
        let (tx, rx) = std::sync::mpsc::channel::<Pixels>();
        tx.send(Err("unreadable".into())).unwrap();
        let pending = Some((rx, want, 1000.0));
        cache.details.insert(key.clone(), Detail { region: want, density: 0.0, shown: None, pending, used: 0.0, failed: None });
        // The failure arrives; the next frames, the view still on that part, ask for nothing.
        for now in [1.0, 1.05, 3.0] {
            cache.detail(&ctx, &key, &item(), None, want, 1000.0, true, now);
            assert!(cache.details.get(&key).unwrap().pending.is_none(), "at {now} s");
        }
        // Another part is asked for at once, the failed one again after a while.
        cache.detail(&ctx, &key, &item(), None, [0.1, 0.1, 0.3, 0.3], 1000.0, true, 3.1);
        assert!(cache.details.get(&key).unwrap().pending.is_some(), "another part");
        cache.details.get_mut(&key).unwrap().pending = None;
        cache.detail(&ctx, &key, &item(), None, want, 1000.0, true, 1.0 + DETAIL_RETRY_S);
        assert!(cache.details.get(&key).unwrap().pending.is_some(), "the failed part, later");
    }

    #[test]
    fn a_page_is_read_on_a_worker_and_the_canvas_does_not_wait_for_it() {
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let look = (false, 100, false);
        // The first frame starts the reading and goes on without the page.
        assert_eq!(cache.texture(&ctx, "/nowhere/plan.pdf", None, 1, look, &[]).err(), Some("being read"));
        // A later frame gets what the worker read (here an error: this test has no PDF reader).
        let mut got = Err("being read".to_string());
        for _ in 0..200 {
            got = cache.texture(&ctx, "/nowhere/plan.pdf", None, 1, look, &[]).map(|_| ()).map_err(str::to_string);
            if got.as_ref().err().is_none_or(|e| e != "being read") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(got.is_err_and(|e| e != "being read"), "the reading ended");
    }

    #[test]
    fn sharper_parts_not_drawn_for_a_while_are_dropped() {
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let detail = |used| Detail { region: [0.0, 0.0, 1.0, 1.0], density: 1.0, shown: Some(texture(&ctx)), pending: None, used, failed: None };
        cache.details.insert((Handle(1), 0, String::new(), 1, (false, 0, false)), detail(0.0));
        cache.details.insert((Handle(2), 0, String::new(), 1, (false, 0, false)), detail(9.0));
        cache.forget_details(10.0);
        assert_eq!(cache.details.keys().map(|k| k.0).collect::<Vec<_>>(), vec![Handle(2)]);
    }

    #[test]
    fn a_page_far_from_its_texture_size_is_drawn_again_at_the_screen_s() {
        let tex = f64::from(super::PAGE_PX);
        assert!(!redrawn(tex, tex));
        assert!(!redrawn(tex * 1.2, tex));
        assert!(!redrawn(tex / 1.2, tex));
        assert!(redrawn(tex * 2.0, tex), "zoomed in: sharper");
        assert!(redrawn(800.0, tex), "zoomed out: thin lines kept");
    }
}
