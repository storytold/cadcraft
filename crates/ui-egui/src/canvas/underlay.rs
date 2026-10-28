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
/// Seconds before a page that could not be read is read again (the view still on it): the PDF
/// may have come back meanwhile.
const PAGE_RETRY_S: f64 = 5.0;
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

/// A page being read on a worker thread, read (its texture), or not read: the error and when
/// (egui time), kept so it is not read again every frame but only after [`PAGE_RETRY_S`].
enum Page {
    Reading(std::sync::mpsc::Receiver<Pixels>),
    Read(TextureHandle),
    Failed(String, f64),
}

/// Which of a frame's two calls of [`shapes`] draws: the active viewport's list (model space
/// seen through it) or the layout's (or model space's) own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Scope {
    Viewport,
    Main,
}

/// A sharper part's key: the underlay, the list it is drawn from and its place there, then the
/// file, page and look it shows.
type DetailKey = (Handle, Scope, usize, String, u32, Look);

/// Pages by drawing and stored file, page number and how they are shown, each with when (egui
/// time) it was last drawn.
#[derive(Default)]
pub(crate) struct Cache {
    pages: HashMap<(String, u32, Look), (Page, f64)>,
    /// Sharper parts.
    details: HashMap<DetailKey, Detail>,
    /// Per list, the view last drawn and since when (egui time) it hasn't moved: the two lists
    /// are drawn through different transforms, so one view would never settle.
    views: HashMap<Scope, (Bounds2, f64)>,
    /// When (egui time) the sizes of underlays without one were last looked for.
    size_retry: Option<f64>,
    /// The page sizes being read on a worker thread.
    sizing: Option<std::sync::mpsc::Receiver<Vec<((String, u32), SizeRead)>>>,
    /// Pages whose PDF is there but gives no size, with the file's modification time then: not
    /// read again until it changes.
    unreadable: HashMap<(String, u32), Option<std::time::SystemTime>>,
}

/// The page sizes read on a worker for underlays shown without one.
type SizeRead = cadcraft_engine::cmd::underlay::SizeRead;

impl Cache {
    /// The page's texture once it has been read. The first call starts reading it on a worker
    /// thread (at once on the web, which has no threads), so the canvas doesn't stop while a big
    /// page is drawn: until then the underlay shows its frame only. At most [`MAX_PAGES`] are
    /// kept: a new one replaces the one drawn longest ago, never one drawn at egui time `now`
    /// (this frame), so more pages on screen than that don't read each other out every frame;
    /// those over the limit show their frame until one is free.
    #[allow(clippy::too_many_arguments)]
    fn texture(
        &mut self,
        ctx: &egui::Context,
        path: &str,
        document: Option<&str>,
        page: u32,
        look: Look,
        layers: &[(String, bool)],
        now: f64,
    ) -> Result<&TextureHandle, &str> {
        // A relative path means a different file in another drawing's folder; the PDF's layers
        // shown or hidden make another picture of the page.
        let key = (format!("{}|{path}|{}", document.unwrap_or(""), layers_key(layers)), page, look);
        // A page that could not be read is read again after a while: the PDF may be back.
        if let Some((Page::Failed(_, at), _)) = self.pages.get(&key)
            && now - at >= PAGE_RETRY_S
        {
            self.pages.remove(&key);
        }
        if !self.pages.contains_key(&key) {
            if self.pages.len() >= MAX_PAGES {
                let oldest = self.pages.iter().filter(|(_, (_, used))| *used < now).min_by(|a, b| a.1.1.total_cmp(&b.1.1)).map(|(k, _)| k.clone());
                match oldest {
                    Some(k) => {
                        self.pages.remove(&k);
                    }
                    None => return Err("too many pages on screen"),
                }
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
            let state = if started { Page::Reading(rx) } else { Page::Failed("the page could not be read".to_string(), now) };
            self.pages.insert(key.clone(), (state, now));
        }
        if let Some((_, used)) = self.pages.get_mut(&key) {
            *used = now;
        }
        if let Some((Page::Reading(rx), _)) = self.pages.get(&key) {
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
            let state = match read {
                Ok(t) => Page::Read(t),
                Err(e) => {
                    log::warn!("PDF underlay {path} page {page}: {e}");
                    Page::Failed(e, now)
                }
            };
            self.pages.insert(key.clone(), (state, now));
        }
        match self.pages.get(&key) {
            Some((Page::Read(t), _)) => Ok(t),
            Some((Page::Failed(e, _), _)) => Err(e.as_str()),
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
/// (the GPU's stencil doesn't reach them). `scope`: which list `pages` comes from.
#[allow(clippy::too_many_arguments)]
pub(crate) fn shapes(
    app: &mut CadApp,
    painter: &egui::Painter,
    xf: &Xf,
    scope: Scope,
    pages: &[ListUnderlay],
    outline: Option<&PolyClip>,
    bg: Rgb,
) -> Vec<(usize, Shape)> {
    let mut out = Vec::new();
    if pages.is_empty() {
        // The last underlay deleted, or a drawing without any: its sharper parts go too (each
        // can hold up to `DETAIL_PX` squared pixels).
        let now = painter.ctx().input(|i| i.time);
        app.canvas.underlays.forget_details(now);
        return out;
    }
    // Underlays shown without a page size (their PDF couldn't be read when the drawing opened):
    // their PDF is looked for again once in a while, on a worker thread, and the drawing redrawn
    // at once when it is back.
    if pages.iter().any(|lu| lu.u == Vec2::ZERO || lu.v == Vec2::ZERO) {
        retry_missing_sizes(app, painter.ctx());
    }
    let dark = u32::from(bg.0) * 299 + u32::from(bg.1) * 587 + u32::from(bg.2) * 114 < 128_000;
    let Ok(st) = app.session.state() else { return out };
    let view = xf.world_bounds();
    let mut items = Vec::new();
    for (index, lu) in pages.iter().enumerate() {
        let Some(u) = cadcraft_render::pdf_underlay(&st.doc, lu.handle) else { continue };
        let mut tris = lu.triangles();
        // The part of the page shown, before a viewport's outline clips it.
        let Some(shown) = lu.shown_of(&tris) else { continue };
        if let Some(c) = outline {
            tris = c.clip_triangles(&tris);
        }
        if tris.is_empty() || !Bounds2::from_points(tris.iter().copied()).intersects(&view) {
            continue;
        }
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
    let settled = cache.settled(scope, view, now);
    let ppp = f64::from(ctx.pixels_per_point());
    // A page wants a sharper part: one more frame once the view has settled draws it.
    let mut want_sharp = false;
    for it in &items {
        let Ok(tex) = cache.texture(&ctx, &it.path, document.as_deref(), it.page, it.look, &it.layers, now) else { continue };
        let base = (tex.id(), f64::from(tex.size()[0].max(1) as u32));
        // Faded: the page's content shows through to the background (its paper is transparent).
        let tint = Color32::WHITE.gamma_multiply(1.0 - it.fade);
        // Screen pixels per page width: away from the texture's own, draw the visible part at it.
        let density = it.axes.0.len() * xf.scale * ppp;
        let key =
            (it.handle, scope, it.index, format!("{}|{}|{}", document.as_deref().unwrap_or(""), it.path, layers_key(&it.layers)), it.page, it.look);
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

/// Look again for the page sizes of underlays shown without one: apply what a worker found, and
/// start another reading every [`PAGE_RETRY_S`] seconds.
fn retry_missing_sizes(app: &mut CadApp, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    if let Some(sizes) = app.canvas.underlays.take_sizes()
        && cadcraft_engine::cmd::underlay::set_sizes(&mut app.session, &sizes)
    {
        // The display list was built from the drawing without the size: draw it again now.
        ctx.request_repaint();
    }
    if app.canvas.underlays.size_retry_due(now) {
        let (document, pages) = cadcraft_engine::cmd::underlay::unsized_pages(&app.session);
        app.canvas.underlays.start_sizing(document, pages);
    }
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(PAGE_RETRY_S));
}

impl Cache {
    /// Read the sizes of `pages` (stored path, page) of the drawing saved at `document` on a
    /// worker thread (at once on the web, which has no threads), unless a reading is going on.
    fn start_sizing(&mut self, document: Option<String>, pages: Vec<(String, u32)>) {
        if self.sizing.is_some() || pages.is_empty() {
            return;
        }
        let unreadable = self.unreadable.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let job = move || {
            let read: Vec<((String, u32), SizeRead)> = pages
                .into_iter()
                .map(|(stored, page)| {
                    let since = unreadable.get(&(stored.clone(), page)).copied().flatten();
                    let r = cadcraft_engine::cmd::underlay::page_size(&stored, document.as_deref(), page, since);
                    ((stored, page), r)
                })
                .collect();
            let _ = tx.send(read);
        };
        #[cfg(not(target_arch = "wasm32"))]
        if std::thread::Builder::new().name("cadcraft-pdf-size".into()).spawn(job).is_err() {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        job();
        self.sizing = Some(rx);
    }

    /// The sizes the worker found, once it is done (`None` while it reads, or without a
    /// reading); pages it found unreadable are noted so they aren't read again unchanged.
    fn take_sizes(&mut self) -> Option<HashMap<(String, u32), Vec2>> {
        let read = match self.sizing.as_ref()?.try_recv() {
            Ok(read) => read,
            Err(std::sync::mpsc::TryRecvError::Empty) => return None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Vec::new(),
        };
        self.sizing = None;
        let mut sizes = HashMap::new();
        for (key, r) in read {
            match r {
                SizeRead::Found(size) => {
                    self.unreadable.remove(&key);
                    sizes.insert(key, size);
                }
                SizeRead::Unreadable(since, why) => {
                    if !self.unreadable.contains_key(&key) {
                        log::warn!("PDF underlay {why}");
                    }
                    self.unreadable.insert(key, since);
                }
                SizeRead::Missing | SizeRead::Unchanged => {}
            }
        }
        Some(sizes)
    }

    /// Whether it is time, at egui time `now`, to look again for the page size of underlays
    /// that have none (their PDF couldn't be read when the drawing opened): at once the first
    /// time, then every [`PAGE_RETRY_S`] seconds.
    fn size_retry_due(&mut self, now: f64) -> bool {
        if self.size_retry.is_some_and(|at| now - at < PAGE_RETRY_S) {
            return false;
        }
        self.size_retry = Some(now);
        true
    }

    /// Whether the view of list `scope` has stayed `view` for [`SETTLE_S`] seconds at egui time
    /// `now`.
    fn settled(&mut self, scope: Scope, view: Bounds2, now: f64) -> bool {
        match self.views.get(&scope) {
            Some((v, since)) if *v == view => now - since >= SETTLE_S,
            _ => {
                self.views.insert(scope, (view, now));
                false
            }
        }
    }

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
        key: &DetailKey,
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
                Ok(Ok((w, h, rgba))) => {
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
        let (path, doc, page, layers, look) = (it.path.clone(), document.map(str::to_string), it.page, it.layers.clone(), it.look);
        let (tx, rx) = std::sync::mpsc::channel::<Pixels>();
        // Shaded on the worker too: a part can be 4096 pixels square.
        let job = move || {
            let r = cadcraft_engine::cmd::underlay::underlay_region_pixels(&path, doc.as_deref(), page, want, w as u32, h as u32, &layers)
                .map_err(|e| e.to_string())
                .map(|(w, h, mut rgba)| {
                    shade(look, &mut rgba);
                    (w, h, rgba)
                });
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
        let key = (Handle(1), Scope::Main, 0, "plan.pdf".to_string(), 1, (false, 0, false));
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
        let key = (Handle(1), Scope::Main, 0, "plan.pdf".to_string(), 1, (false, 0, false));
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
        assert_eq!(cache.texture(&ctx, "/nowhere/plan.pdf", None, 1, look, &[], 0.0).err(), Some("being read"));
        // A later frame gets what the worker read (here an error: this test has no PDF reader).
        let mut got = Err("being read".to_string());
        for _ in 0..200 {
            got = cache.texture(&ctx, "/nowhere/plan.pdf", None, 1, look, &[], 0.0).map(|_| ()).map_err(str::to_string);
            if got.as_ref().err().is_none_or(|e| e != "being read") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(got.is_err_and(|e| e != "being read"), "the reading ended");
    }

    /// Reads the page of `path` at egui time `now` until the worker is done; the outcome.
    fn read_page(ctx: &egui::Context, cache: &mut Cache, path: &str, now: f64) -> Result<(), String> {
        for _ in 0..200 {
            match cache.texture(ctx, path, None, 1, (false, 100, false), &[], now) {
                Err("being read") => std::thread::sleep(std::time::Duration::from_millis(10)),
                r => return r.map(|_| ()).map_err(str::to_string),
            }
        }
        Err("being read".into())
    }

    #[test]
    fn a_page_that_could_not_be_read_is_read_again_after_a_while() {
        // The PDF isn't there when the drawing opens (a network share not yet reachable, a file
        // copied in later): the page is read again once in a while, not only after a restart.
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let path = "/nowhere/later.pdf";
        assert!(read_page(&ctx, &mut cache, path, 1.0).is_err_and(|e| e != "being read"), "the first reading fails");
        // The next frames, still on screen, don't read it again every frame.
        for now in [1.05, 2.0, 1.0 + PAGE_RETRY_S - 0.1] {
            assert!(cache.texture(&ctx, path, None, 1, (false, 100, false), &[], now).is_err_and(|e| e != "being read"), "at {now} s");
        }
        // After a while it is read again.
        assert_eq!(cache.texture(&ctx, path, None, 1, (false, 100, false), &[], 1.0 + PAGE_RETRY_S).err(), Some("being read"));
    }

    #[test]
    fn page_sizes_are_read_on_a_worker_one_reading_at_a_time() {
        let mut cache = Cache::default();
        let pages = vec![("/nowhere/a.pdf".to_string(), 1), ("/nowhere/b.pdf".to_string(), 2)];
        cache.start_sizing(None, pages.clone());
        assert!(cache.sizing.is_some(), "started without waiting for the files");
        // One reading at a time: a second start while it runs starts nothing.
        let first = cache.sizing.as_ref().map(|rx| rx as *const _);
        cache.start_sizing(None, pages);
        assert_eq!(cache.sizing.as_ref().map(|rx| rx as *const _), first);
        let mut got = None;
        for _ in 0..200 {
            got = cache.take_sizes();
            if got.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(got.map(|s| s.len()), Some(0), "nothing found (no PDF reader in this test), and the reading is over");
        assert!(cache.sizing.is_none());
        assert!(cache.take_sizes().is_none(), "no reading going on");
        // Nothing to read: nothing started.
        cache.start_sizing(None, Vec::new());
        assert!(cache.sizing.is_none());
    }

    #[test]
    fn page_sizes_are_looked_for_again_every_few_seconds() {
        let mut cache = Cache::default();
        assert!(cache.size_retry_due(10.0), "at once the first time");
        assert!(!cache.size_retry_due(10.05));
        assert!(!cache.size_retry_due(10.0 + PAGE_RETRY_S - 0.01));
        assert!(cache.size_retry_due(10.0 + PAGE_RETRY_S));
    }

    #[test]
    fn sharper_parts_go_when_no_underlay_is_left() {
        // The last underlay deleted, or a drawing without any: the canvas still calls `shapes`
        // with no pages, and the parts kept are dropped once their time is up.
        let ctx = egui::Context::default();
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        let tex = texture(&ctx);
        let old = Detail { region: [0.0, 0.0, 1.0, 1.0], density: 1.0, shown: Some(tex), pending: None, used: -DETAIL_KEEP_S - 1.0, failed: None };
        app.canvas.underlays.details.insert((Handle(1), Scope::Main, 0, String::new(), 1, (false, 0, false)), old);
        let painter = egui::Painter::new(ctx.clone(), egui::LayerId::background(), egui::Rect::EVERYTHING);
        let xf = Xf { rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0)), center: Vec2::ZERO, scale: 1.0 };
        assert!(shapes(&mut app, &painter, &xf, Scope::Main, &[], None, Rgb(0, 0, 0)).is_empty());
        assert!(app.canvas.underlays.details.is_empty(), "the sharper part is dropped");
    }

    #[test]
    fn sharper_parts_not_drawn_for_a_while_are_dropped() {
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let detail = |used| Detail { region: [0.0, 0.0, 1.0, 1.0], density: 1.0, shown: Some(texture(&ctx)), pending: None, used, failed: None };
        cache.details.insert((Handle(1), Scope::Main, 0, String::new(), 1, (false, 0, false)), detail(0.0));
        cache.details.insert((Handle(2), Scope::Main, 0, String::new(), 1, (false, 0, false)), detail(9.0));
        cache.forget_details(10.0);
        assert_eq!(cache.details.keys().map(|k| k.0).collect::<Vec<_>>(), vec![Handle(2)]);
    }

    /// The page keys kept, sorted.
    fn kept(cache: &Cache) -> Vec<String> {
        let mut k: Vec<String> = cache.pages.keys().map(|k| k.0.clone()).collect();
        k.sort();
        k
    }

    #[test]
    fn more_pages_on_screen_than_kept_do_not_read_each_other_out_every_frame() {
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let look = (false, 100, false);
        let paths: Vec<String> = (0..=MAX_PAGES).map(|i| format!("/nowhere/p{i}.pdf")).collect();
        let frame = |cache: &mut Cache, now: f64, which: &[String]| {
            for p in which {
                let _ = cache.texture(&ctx, p, None, 1, look, &[], now);
            }
        };
        frame(&mut cache, 1.0, &paths);
        let first = kept(&cache);
        assert_eq!(first.len(), MAX_PAGES);
        // The page over the limit waits; it doesn't throw out the pages already read.
        assert_eq!(cache.texture(&ctx, paths.last().unwrap(), None, 1, look, &[], 1.0).err(), Some("too many pages on screen"));
        for now in [1.02, 1.04, 1.06, 2.0] {
            frame(&mut cache, now, &paths);
            assert_eq!(kept(&cache), first, "frame at {now} s");
        }
        // Once a page is no longer drawn, the waiting one takes its place, and only it.
        let mut fewer = paths.clone();
        let gone = fewer.remove(3);
        frame(&mut cache, 3.0, &fewer);
        let now_kept = kept(&cache);
        assert_eq!(now_kept.len(), MAX_PAGES);
        assert!(!now_kept.iter().any(|k| k.contains(&format!("|{gone}|"))), "the page not drawn is dropped");
        assert!(now_kept.iter().any(|k| k.contains(&format!("|{}|", paths.last().unwrap()))), "the waiting page is read");
    }

    #[test]
    fn a_page_drawn_long_ago_makes_room_for_a_new_one() {
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let look = (false, 100, false);
        for i in 0..MAX_PAGES {
            let _ = cache.texture(&ctx, &format!("/nowhere/p{i}.pdf"), None, 1, look, &[], f64::from(u32::try_from(i).unwrap()));
        }
        let _ = cache.texture(&ctx, "/nowhere/new.pdf", None, 1, look, &[], 100.0);
        let k = kept(&cache);
        assert_eq!(k.len(), MAX_PAGES);
        assert!(!k.iter().any(|k| k.contains("|/nowhere/p0.pdf|")), "the oldest goes");
        assert!(k.iter().any(|k| k.contains("|/nowhere/p1.pdf|")));
        assert!(k.iter().any(|k| k.contains("|/nowhere/new.pdf|")));
    }

    #[test]
    fn each_list_s_view_settles_on_its_own() {
        // The active viewport's list and the layout's are drawn through different transforms,
        // so their views differ every frame; each still settles once it stops moving.
        let mut cache = Cache::default();
        let a = Bounds2::new(Vec2::ZERO, Vec2::new(10.0, 10.0));
        let b = Bounds2::new(Vec2::new(-5.0, -5.0), Vec2::new(50.0, 40.0));
        let mut now = 0.0;
        let mut last = (true, true);
        while now < 1.0 {
            last = (cache.settled(Scope::Viewport, a, now), cache.settled(Scope::Main, b, now));
            if now == 0.0 {
                assert_eq!(last, (false, false));
            }
            now += 0.05;
        }
        assert_eq!(last, (true, true));
        // Panning the viewport doesn't unsettle the layout.
        let moved = Bounds2::new(Vec2::new(1.0, 0.0), Vec2::new(11.0, 10.0));
        assert_eq!((cache.settled(Scope::Viewport, moved, now), cache.settled(Scope::Main, b, now)), (false, true));
    }

    #[test]
    fn one_underlay_in_both_lists_keeps_a_sharper_part_for_each() {
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let key = |scope| (Handle(1), scope, 0, "plan.pdf".to_string(), 1, (false, 0, false));
        let (vp, main) = ([0.1, 0.1, 0.2, 0.2], [0.0, 0.0, 1.0, 1.0]);
        cache.detail(&ctx, &key(Scope::Viewport), &item(), None, vp, 4000.0, true, 1.0);
        cache.detail(&ctx, &key(Scope::Main), &item(), None, main, 800.0, true, 1.0);
        let part = |scope| cache.details.get(&key(scope)).and_then(|d| d.pending.as_ref().map(|p| (p.1, p.2)));
        assert_eq!(part(Scope::Viewport), Some((vp, 4000.0)));
        assert_eq!(part(Scope::Main), Some((main, 800.0)));
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
