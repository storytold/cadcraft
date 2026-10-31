//! The Block Manager / Blocks palette: displays the drawing's blocks as a list, medium icons or
//! large icons (with a slider and view-mode menu), reuses previews in a bounded LRU cache,
//! lets the user grab a block by clicking or dragging it
//! onto the drawing to place with a single click, and keeps all block editing actions.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cadcraft_doc::{Drawing, EntityKind};
use cadcraft_geom::{Bounds2, Vec2};
use egui::{Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::CadApp;
use crate::theme::Tokens;

/// How the Blocks palette displays the drawing's blocks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewMode {
    List,
    #[default]
    MediumIcons,
    LargeIcons,
}

impl ViewMode {
    /// Convert a slider index (`0..=2`) to its corresponding [`ViewMode`].
    pub fn from_index(idx: u8) -> Self {
        match idx {
            0 => Self::List,
            2 => Self::LargeIcons,
            _ => Self::MediumIcons,
        }
    }

    /// Numeric slider index (`0` = List, `1` = Medium Icons, `2` = Large Icons).
    pub fn index(self) -> u8 {
        match self {
            Self::List => 0,
            Self::MediumIcons => 1,
            Self::LargeIcons => 2,
        }
    }

    /// Human-readable English label for UI translation lookup.
    pub fn label(self) -> &'static str {
        match self {
            Self::List => "List",
            Self::MediumIcons => "Medium Icons",
            Self::LargeIcons => "Large Icons",
        }
    }
}

/// The palette's own state between frames.
#[derive(Clone)]
pub struct State {
    pub selected: Option<String>,
    pub filter: String,
    pub view_mode: ViewMode,
    /// Specify scale on screen during insertion instead of using `(scale_x, scale_y)`.
    pub prompt_scale: bool,
    pub scale_x: f64,
    pub scale_y: f64,
    /// Specify rotation angle on screen during insertion instead of using `rotation`.
    pub prompt_rotation: bool,
    pub rotation: f64,
    /// Keep placing copies of the grabbed block after each click on the drawing.
    pub repeat: bool,
    /// Insert the block exploded (`*name`).
    pub explode: bool,
    /// Put a block in hand ready to drop on the canvas as soon as it is clicked or dragged.
    pub grab_on_click: bool,
    /// Whether the insertion options section is expanded.
    pub show_options: bool,
    /// Whether the currently running `INSERT` was started by this palette with repeat placement enabled.
    pub repeating: bool,
    /// Whether the active insertion was grabbed from this palette.
    pub grabbed: bool,
    /// The run of `INSERT` this palette started, which no other `INSERT` (typed, or from a menu) can be mistaken for.
    pub owner: Option<u64>,
    /// The new name while renaming the selected block.
    pub rename: Option<String>,
    /// The last message (a refused delete or rename).
    pub note: Option<String>,
}

impl Default for State {
    /// Construct the default palette state (`MediumIcons`, uniform scale `1.0`, `grab_on_click` enabled).
    fn default() -> Self {
        Self {
            selected: None,
            filter: String::new(),
            view_mode: ViewMode::MediumIcons,
            prompt_scale: false,
            scale_x: 1.0,
            scale_y: 1.0,
            prompt_rotation: false,
            rotation: 0.0,
            repeat: false,
            explode: false,
            grab_on_click: true,
            show_options: true,
            repeating: false,
            grabbed: false,
            owner: None,
            rename: None,
            note: None,
        }
    }
}

/// A block's preview, built once while the block is visible on screen.
#[derive(Clone)]
struct Preview {
    list: Arc<cadcraft_render::DisplayList>,
    bounds: Bounds2,
}

/// Bounded LRU of complete results (including refusals) for `(drawing_uid, revision)`.
/// Off-screen entries are preferred eviction candidates; visibility never bypasses the caps.
#[derive(Clone, Default)]
pub struct PreviewCache {
    key: (u64, u64),
    entries: HashMap<String, CacheEntry>,
    clock: u64,
    names: Option<Arc<Vec<String>>>,
    references: Option<(String, usize)>,
}

#[derive(Clone)]
struct CacheEntry {
    result: PreviewResult,
    used: u64,
    bytes: usize,
}
const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_CACHE_RESULTS: usize = 512;
// Leave room for hash-table growth when the 513th result triggers eviction.
const MAX_ENTRY_BYTES: usize = MAX_CACHE_BYTES - 2 * (MAX_CACHE_RESULTS + 1) * (std::mem::size_of::<(String, CacheEntry)>() + 16);

/// Count capacities, not lengths. Thumbnail lists never own plotted glyph/font data.
fn retained_bytes(result: &PreviewResult) -> Option<usize> {
    let mut bytes = std::mem::size_of::<CacheEntry>();
    if let PreviewResult::Ready(p) = result {
        let list = &p.list;
        if !list.texts.is_empty() {
            return None;
        }
        bytes = bytes.checked_add(std::mem::size_of::<cadcraft_render::DisplayList>() + 2 * std::mem::size_of::<usize>())?;
        for (capacity, size) in [
            (list.prims.capacity(), std::mem::size_of::<cadcraft_render::DPrim>()),
            (list.verts.capacity(), std::mem::size_of::<Vec2>()),
            (list.tris.capacity(), std::mem::size_of::<Vec2>()),
            (list.prim_layers.capacity(), std::mem::size_of::<u32>()),
            (list.texts.capacity(), std::mem::size_of::<cadcraft_render::PlotText>()),
            (list.pens.capacity(), std::mem::size_of::<cadcraft_render::pen::Pen>()),
        ] {
            bytes = bytes.checked_add(capacity.checked_mul(size)?)?;
        }
    }
    Some(bytes)
}

/// At most three synchronous builds start per frame. An individual build can still stall a frame.
const MAX_BUILDS_PER_FRAME: usize = 3;

impl PreviewCache {
    /// Ensure the cache matches `(uid, revision)`, clearing stale entries if the drawing changed.
    pub fn sync_revision(&mut self, uid: u64, revision: u64) {
        if self.key != (uid, revision) {
            self.key = (uid, revision);
            self.entries.clear();
            self.names = None;
            self.references = None;
        }
    }

    /// Touch a result without rebuilding it, even when it has no geometry.
    fn touch(&mut self, name: &str) {
        self.clock = match self.clock.checked_add(1) {
            Some(v) => v,
            None => {
                for entry in self.entries.values_mut() {
                    entry.used = 0;
                }
                1
            }
        };
        if let Some(entry) = self.entries.get_mut(name) {
            entry.used = self.clock;
        }
    }

    /// Caches `result` under `name` (an oversized one becomes a small refusal), then evicts beyond the limits.
    fn insert(&mut self, name: String, mut result: PreviewResult, visible: &HashSet<String>) {
        let mut bytes = retained_bytes(&result).and_then(|v| v.checked_add(name.capacity()));
        // Cache a small refusal instead of rebuilding a result that can never fit.
        if bytes.is_none_or(|v| v > MAX_ENTRY_BYTES) {
            result = PreviewResult::Unavailable(PreviewFailure::Output);
            bytes = std::mem::size_of::<CacheEntry>().checked_add(name.capacity());
        }
        self.entries.insert(name.clone(), CacheEntry { result, used: 0, bytes: bytes.unwrap_or(MAX_CACHE_BYTES) });
        self.touch(&name);
        self.evict(visible);
    }

    /// Conservative retained allocation estimate, including hash capacity and owned key buffers.
    fn estimated_bytes(&self) -> Option<usize> {
        let slots = std::mem::size_of::<(String, CacheEntry)>().checked_add(16)?;
        self.entries.values().try_fold(self.entries.capacity().checked_mul(slots)?, |sum, e| sum.checked_add(e.bytes))
    }

    /// Drops least-recently-used entries until the count and byte limits hold, sparing `visible` names while possible.
    fn evict(&mut self, visible: &HashSet<String>) {
        while self.entries.len() > MAX_CACHE_RESULTS || self.estimated_bytes().is_none_or(|v| v > MAX_CACHE_BYTES) {
            let victim = self.entries.iter().min_by_key(|(name, e)| (visible.contains(*name), e.used)).map(|(name, _)| name.clone());
            let Some(victim) = victim else { break };
            self.entries.remove(&victim);
        }
    }

    /// Number of block previews currently held in memory.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no block previews are currently cached.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Egui temporary data id storing the [`State`] of the Blocks palette.
fn state_id() -> egui::Id {
    egui::Id::unique("cc_block_manager")
}

/// Egui temporary data id storing the [`PreviewCache`] of visible blocks.
fn cache_id() -> egui::Id {
    egui::Id::unique("cc_block_manager_previews")
}

/// How many block previews are currently loaded in memory for `ctx`.
pub fn cached_preview_count(ctx: &egui::Context) -> usize {
    ctx.data(|d| d.get_temp::<PreviewCache>(cache_id())).map_or(0, |c| c.len())
}

/// The named blocks a user works with: not the generated (`*D12`) ones nor layout blocks.
pub fn names(d: &Drawing) -> Vec<String> {
    let mut v: Vec<String> = d.blocks.keys().filter(|k| !k.starts_with('*')).cloned().collect();
    v.sort_by_cached_key(|n| n.to_lowercase());
    v
}

/// How many block references to `name` the drawing holds (model, layouts and other blocks).
pub fn reference_count(d: &Drawing, name: &str) -> usize {
    std::iter::once(&d.model)
        .chain(d.layouts.iter().map(|l| &l.entities))
        .chain(d.blocks.values().map(|b| &b.entities))
        .flat_map(|s| s.iter())
        .filter(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(name)))
        .count()
}

mod preview;
use preview::{PreviewFailure, PreviewResult, build};

/// Draw a display list fitted into `r` on a framed square of the canvas colour.
pub fn paint_list(p: &egui::Painter, r: Rect, list: &cadcraft_render::DisplayList, bounds: &Bounds2) {
    let t = Tokens::get();
    p.rect_filled(r, 3.0, t.canvas);
    let pad = (r.width().min(r.height()) * 0.1).clamp(2.0, 8.0);
    let inner = r.shrink(pad);
    if !bounds.is_empty() && inner.width() > 1.0 && inner.height() > 1.0 {
        let (w, h) = (bounds.width().max(1e-12), bounds.height().max(1e-12));
        let k = (f64::from(inner.width()) / w).min(f64::from(inner.height()) / h);
        let c = bounds.center();
        let mid = inner.center();
        let at = |v: &Vec2| pos2(mid.x + ((v.x - c.x) * k) as f32, mid.y - ((v.y - c.y) * k) as f32);
        let bg = cadcraft_color::Rgb(t.canvas.r(), t.canvas.g(), t.canvas.b());
        let clip = p.with_clip_rect(r.shrink(1.0));
        for prim in &list.prims {
            let rgb = prim.display_rgb(bg);
            let col = egui::Color32::from_rgb(rgb.0, rgb.1, rgb.2);
            let pts = list.points(prim);
            match prim.kind {
                cadcraft_render::Kind::Polyline if pts.len() >= 2 => {
                    clip.add(egui::Shape::line(pts.iter().map(at).collect(), Stroke::new(1.0, col)));
                }
                cadcraft_render::Kind::Tris | cadcraft_render::Kind::Mask => {
                    let fill = if prim.kind == cadcraft_render::Kind::Mask { t.canvas } else { col };
                    let mut mesh = egui::Mesh::default();
                    for tri in pts.as_chunks::<3>().0 {
                        let base = u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX);
                        for v in tri {
                            mesh.colored_vertex(at(v), fill);
                        }
                        mesh.add_triangle(base, base + 1, base + 2);
                    }
                    clip.add(egui::Shape::mesh(mesh));
                }
                cadcraft_render::Kind::Point => {
                    if let Some(v) = pts.first() {
                        clip.circle_filled(at(v), 1.2, col);
                    }
                }
                _ => {}
            }
        }
    }
    p.rect_stroke(r, 3.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
}

/// Paint a mini glyph for the view-mode button (`List`, `MediumIcons`, `LargeIcons`).
fn paint_view_mode_icon(p: &egui::Painter, r: Rect, mode: ViewMode, col: egui::Color32) {
    let inner = r.shrink(3.0);
    let stroke = Stroke::new(1.2, col);
    match mode {
        ViewMode::LargeIcons => {
            p.rect_stroke(inner, 1.5, stroke, egui::StrokeKind::Inside);
            p.rect_filled(inner.shrink(2.5), 1.0, col.gamma_multiply(0.35));
        }
        ViewMode::MediumIcons => {
            let gap = 2.0;
            let w = (inner.width() - gap) * 0.5;
            let h = (inner.height() - gap) * 0.5;
            for row in 0..2 {
                for col_i in 0..2 {
                    let min = pos2(inner.left() + col_i as f32 * (w + gap), inner.top() + row as f32 * (h + gap));
                    p.rect_stroke(Rect::from_min_size(min, vec2(w, h)), 1.0, stroke, egui::StrokeKind::Inside);
                }
            }
        }
        ViewMode::List => {
            for i in 0..3 {
                let y = inner.top() + (i as f32 + 0.5) * (inner.height() / 3.0);
                p.rect_filled(Rect::from_center_size(pos2(inner.left() + 1.5, y), vec2(2.5, 2.5)), 0.5, col);
                p.line_segment([pos2(inner.left() + 5.0, y), pos2(inner.right(), y)], stroke);
            }
        }
    }
}

/// Truncate `text` with `…` so it fits within `max_w` pixels in `font`.
fn truncate_text(p: &egui::Painter, text: &str, font: &egui::FontId, col: egui::Color32, max_w: f32) -> String {
    let g = p.layout_no_wrap(text.to_string(), font.clone(), col);
    if g.size().x <= max_w {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars() {
        let cand = format!("{out}{ch}…");
        if p.layout_no_wrap(cand, font.clone(), col).size().x > max_w {
            break;
        }
        out.push(ch);
    }
    format!("{out}…")
}

/// Start `INSERT` with `name` in hand and the palette's scale/rotation options preset, so the
/// block follows the crosshairs on the canvas and a single click places it.
pub fn grab_block(app: &mut CadApp, name: &str, st: &State) {
    let chosen = if st.explode { format!("*{name}") } else { name.to_string() };
    crate::dialogs::insert_block(app, &chosen);
    if app.session.running.as_ref().is_none_or(|r| r.id != "insert") {
        return;
    }
    if app.session.current_prompt().is_none_or(|p| !p.accept.point) {
        return;
    }
    if !st.prompt_scale {
        let sx = if st.scale_x.is_finite() && st.scale_x != 0.0 { st.scale_x } else { 1.0 };
        let sy = if st.scale_y.is_finite() && st.scale_y != 0.0 { st.scale_y } else { sx };
        let scale_text = if (sx - sy).abs() < 1e-12 || st.explode { format!("{sx}") } else { format!("{sx},{sy}") };
        let _ = app.session.input(cadcraft_engine::Input::Keyword("Scale".into()));
        let _ = app.session.input(cadcraft_engine::Input::Text(scale_text));
    }
    if !st.prompt_rotation {
        let rot = if st.rotation.is_finite() { st.rotation } else { 0.0 };
        let _ = app.session.input(cadcraft_engine::Input::Keyword("Rotate".into()));
        let _ = app.session.input(cadcraft_engine::Input::Text(format!("<<{rot}")));
    }
    if st.repeat {
        let _ = app.session.input(cadcraft_engine::Input::Keyword("Repeat".into()));
    }
}

/// What a control in the palette asked for, executed once the UI frame is drawn.
enum Act {
    /// Grab a block in hand (from clicking or dragging a tile, or the Insert button).
    Grab(String),
    Edit(String),
    Create,
    StartRename(String),
    Rename(String, String),
    Delete(String),
    Write(String),
    /// Pin the window as a panel on the right; float the panel as a window.
    ToPanel,
    ToWindow,
}

/// The data the Block Manager shows this frame.
struct View {
    all: Arc<Vec<String>>,
    selected: Option<String>,
    info: Option<(usize, usize, cadcraft_geom::Vec3, bool, String, Option<String>)>,
    can_write: bool,
}

/// Gather the block list, selected block metadata and write capability for this frame.
fn view(app: &CadApp, st: &mut State, cache: &mut PreviewCache) -> Option<View> {
    let sess = app.session.state().ok()?;
    let d = &sess.doc;
    cache.sync_revision(sess.uid, sess.revision);
    let all = Arc::clone(cache.names.get_or_insert_with(|| Arc::new(names(d))));
    if st.selected.as_ref().is_some_and(|s| !d.blocks.contains_key(s)) {
        st.selected = None;
        st.rename = None;
    }
    if st.selected.is_none() {
        st.selected = all.first().cloned();
    }
    let selected = st.selected.clone();
    let info = selected.as_deref().and_then(|n| {
        let b = d.block(n)?;
        if cache.references.as_ref().is_none_or(|(name, _)| name != n) {
            cache.references = Some((n.to_string(), reference_count(d, n)));
        }
        let refs = cache.references.as_ref().map_or(0, |(_, count)| *count);
        Some((b.entities.len(), refs, b.base, b.annotative, b.description.clone(), b.xref_path.clone()))
    });
    Some(View { all, selected, info, can_write: app.services.pick_save.is_some() })
}

/// One frame's visible set and allowance for starting synchronous builds. The elapsed-time
/// check defers the next build; it cannot interrupt a build already in progress.
struct FrameBuilds {
    visible: HashSet<String>,
    builds_left: usize,
    started: web_time::Instant,
}

/// Render a single block tile/row according to `st.view_mode`, updating `frame` and `cache`
/// only when the tile is actually visible inside the scroll viewport.
fn draw_block_item(
    ui: &mut egui::Ui,
    d: &Drawing,
    name: &str,
    selected: bool,
    mode: ViewMode,
    tile_size: egui::Vec2,
    (cache, frame): (&mut PreviewCache, &mut FrameBuilds),
) -> egui::Response {
    let t = Tokens::get();
    let (rect, resp) = ui.allocate_exact_size(tile_size, Sense::click_and_drag());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    frame.visible.insert(name.to_string());
    if !cache.entries.contains_key(name) {
        if frame.builds_left > 0 && frame.started.elapsed().as_millis() < 30 {
            frame.builds_left -= 1;
            cache.insert(name.to_string(), build(d, name), &frame.visible);
        } else {
            ui.ctx().request_repaint();
        }
    }
    cache.touch(name);
    let p = ui.painter();
    let result = cache.entries.get(name).map(|e| &e.result);
    let status = preview_status(result, d.block(name).is_some_and(|b| b.entities.is_empty()));
    if selected {
        p.rect_filled(rect, 3.0, t.accent.gamma_multiply(0.28));
        p.rect_stroke(rect, 3.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_filled(rect, 3.0, t.control_hover);
    }
    match mode {
        ViewMode::List => {
            let thumb = Rect::from_min_size(pos2(rect.left() + 4.0, rect.top() + 3.0), vec2(26.0, (rect.height() - 6.0).max(16.0)));
            paint_preview(p, thumb, result, &status, true);
            let text_x = thumb.right() + 6.0;
            let max_w = (rect.right() - text_x - 4.0).max(20.0);
            let label = truncate_text(p, name, &crate::theme::body(), t.text, max_w);
            p.text(pos2(text_x, rect.top() + 2.0), egui::Align2::LEFT_TOP, label, crate::theme::body(), t.text);
            if let Some(status) = &status {
                p.text(pos2(text_x, rect.bottom() - 2.0), egui::Align2::LEFT_BOTTOM, status, crate::theme::small(), t.text_dim);
            }
        }
        ViewMode::MediumIcons | ViewMode::LargeIcons => {
            let label_h = 18.0;
            let pad = 4.0;
            let thumb = Rect::from_min_max(
                pos2(rect.left() + pad, rect.top() + pad),
                pos2(rect.right() - pad, (rect.bottom() - label_h).max(rect.top() + 16.0)),
            );
            paint_preview(p, thumb, result, &status, false);
            let max_w = (rect.width() - 6.0).max(20.0);
            let font = crate::theme::small();
            let label = truncate_text(p, name, &font, t.text, max_w);
            p.text(pos2(rect.center().x, rect.bottom() - label_h * 0.5), egui::Align2::CENTER_CENTER, label, font, t.text);
        }
    }
    if let Some(PreviewResult::Unavailable(reason)) = result {
        resp.on_hover_text(format!("{}\n{}", crate::tl!("Preview unavailable"), reason.message()))
    } else if let Some(status) = status {
        resp.on_hover_text(status)
    } else {
        resp
    }
}

/// Short text explaining why a block shows no drawing, or `None` when it has a preview.
fn preview_status(result: Option<&PreviewResult>, empty_definition: bool) -> Option<&'static str> {
    match result {
        None => Some(crate::tl!("Preview pending")),
        Some(PreviewResult::Ready(_)) => None,
        Some(PreviewResult::Empty) if empty_definition => Some(crate::tl!("Empty block")),
        Some(PreviewResult::Empty) => Some(crate::tl!("No visible content")),
        Some(PreviewResult::Unavailable(_)) => Some(crate::tl!("Preview unavailable")),
    }
}

/// Paints the preview, or the `status` text in `rect` when there is none (`compact` shows a one-character marker instead).
fn paint_preview(p: &egui::Painter, rect: Rect, result: Option<&PreviewResult>, status: &Option<&'static str>, compact: bool) {
    if let Some(PreviewResult::Ready(pv)) = result {
        paint_list(p, rect, &pv.list, &pv.bounds);
        return;
    }
    let t = Tokens::get();
    p.rect_filled(rect, 3.0, t.canvas);
    p.rect_stroke(rect, 3.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    if let Some(text) = status {
        let text = if compact { if matches!(result, Some(PreviewResult::Unavailable(_))) { "!" } else { "…" } } else { *text };
        let galley = p.layout(text.to_string(), crate::theme::small(), t.text_dim, (rect.width() - 6.0).max(1.0));
        p.with_clip_rect(rect).galley(rect.center() - galley.size() / 2.0, galley, t.text_dim);
    }
}

/// A double-click's second release must not restart placement begun by its first click.
fn should_grab(drag: bool, click: bool, double_click: bool, grab_on_click: bool) -> bool {
    drag || if grab_on_click { click && !double_click } else { double_click }
}

/// The manager's contents: top filter + view-mode slider/menu, scrollable block grid/list with
/// cached previews, insertion options, and block editing controls.
fn body(ui: &mut egui::Ui, d: &Drawing, v: &View, st: &mut State, cache: &mut PreviewCache, as_panel: bool, act: &mut Option<Act>) {
    let View { all, selected, info, can_write, .. } = v;
    let can_write = *can_write;
    let t = Tokens::get();

    // Top bar: Filter + View mode slider & dropdown + Float/Pin toggle.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let reserved = if as_panel { 112.0 } else { 220.0 };
        let w = (ui.available_width() - reserved).max(64.0);
        ui.add(egui::TextEdit::singleline(&mut st.filter).hint_text(crate::tl!("Filter")).desired_width(w));

        // Slider ("curseur") for List (0) / Medium Icons (1) / Large Icons (2).
        let mut mode_idx = st.view_mode.index();
        let slider_resp = ui
            .scope(|ui| {
                ui.spacing_mut().slider_width = 44.0;
                ui.add(egui::Slider::new(&mut mode_idx, 0..=2).show_value(false))
            })
            .inner
            .on_hover_text(format!("{}: {}", crate::tl!("View mode"), crate::i18n::t(ViewMode::from_index(mode_idx).label())));
        if slider_resp.changed() {
            st.view_mode = ViewMode::from_index(mode_idx);
        }

        // View mode icon dropdown button (like AutoCAD's Blocks palette menu).
        let (btn_rect, btn_resp) = ui.allocate_exact_size(vec2(28.0, 20.0), Sense::click());
        if btn_resp.hovered() {
            ui.painter().rect_filled(btn_rect, 3.0, t.control_hover);
        } else {
            ui.painter().rect_filled(btn_rect, 3.0, t.control);
        }
        let icon_rect = Rect::from_min_size(pos2(btn_rect.left() + 2.0, btn_rect.top() + 2.0), vec2(16.0, 16.0));
        paint_view_mode_icon(ui.painter(), icon_rect, st.view_mode, t.text);
        crate::icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(btn_rect.right() - 5.0, btn_rect.center().y), vec2(8.0, 8.0)),
            crate::icons::Icon::ChevronDown,
            false,
        );
        let btn_resp = btn_resp.on_hover_text(crate::tl!("View mode"));
        egui::Popup::from_toggle_button_response(&btn_resp).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
            for mode in [ViewMode::LargeIcons, ViewMode::MediumIcons, ViewMode::List] {
                if ui.selectable_label(st.view_mode == mode, crate::i18n::t(mode.label())).clicked() {
                    st.view_mode = mode;
                }
            }
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (icon, tip) = if as_panel {
                (crate::icons::Icon::Workspace, crate::tl!("Float as a window"))
            } else {
                (crate::icons::Icon::Properties, crate::tl!("Pin as a panel on the right"))
            };
            if crate::icons::button(ui, icon, 20.0, tip, false).clicked() {
                *act = Some(if as_panel { Act::ToWindow } else { Act::ToPanel });
            }
            if !as_panel && ui.button(crate::tl!("Create...")).on_hover_text(crate::tl!("Make a block from objects (BLOCK)")).clicked() {
                *act = Some(Act::Create);
            }
        });
    });

    ui.add_space(4.0);
    let avail = ui.available_size();
    let bottom_reserve = if st.show_options { 210.0 } else { 115.0 };
    let list_h = (avail.y - bottom_reserve).clamp(140.0, 420.0);
    let list_w = avail.x;

    let mut frame = FrameBuilds { visible: HashSet::new(), builds_left: MAX_BUILDS_PER_FRAME, started: web_time::Instant::now() };

    egui::Frame::NONE.stroke(Stroke::new(1.0, t.border)).inner_margin(4.0).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("cc_block_list").auto_shrink([false, false]).max_height(list_h).show(ui, |ui| {
            let inner_w = (list_w - 14.0).max(80.0);
            ui.set_width(inner_w);
            let filter = st.filter.trim().to_lowercase();
            let filtered: Vec<&String> = all.iter().filter(|n| filter.is_empty() || n.to_lowercase().contains(&filter)).collect();
            if filtered.is_empty() {
                let msg = if all.is_empty() { crate::tl!("No blocks defined in this drawing.") } else { crate::tl!("No blocks match the search.") };
                ui.label(egui::RichText::new(msg).color(t.text_dim));
                return;
            }

            let mut interact = |n: &String, r: egui::Response| {
                let drag = r.drag_started();
                let click = r.clicked();
                let dbl = r.double_clicked();
                if click || drag || dbl {
                    st.selected = Some((*n).clone());
                    st.rename = None;
                    st.note = None;
                }
                if drag {
                    r.ctx.stop_dragging();
                }
                if should_grab(drag, click, dbl, st.grab_on_click) {
                    *act = Some(Act::Grab((*n).clone()));
                }
                r.on_hover_text(format!("{n}\n{}", crate::tl!("Click or drag to take in hand, then click on the drawing to place")));
            };

            match st.view_mode {
                ViewMode::List => {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for n in filtered {
                        let on = selected.as_deref() == Some(n.as_str());
                        let r = draw_block_item(ui, d, n, on, ViewMode::List, vec2(inner_w, 28.0), (cache, &mut frame));
                        interact(n, r);
                    }
                }
                ViewMode::MediumIcons => {
                    let gap = 6.0;
                    let cols = ((inner_w + gap) / 76.0).floor().max(1.0) as usize;
                    let tile_w = ((inner_w - gap * (cols.saturating_sub(1) as f32)) / cols as f32).floor().max(58.0);
                    let tile_h = 76.0;
                    for row in filtered.chunks(cols) {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            for n in row {
                                let on = selected.as_deref() == Some(n.as_str());
                                let r = draw_block_item(ui, d, n, on, ViewMode::MediumIcons, vec2(tile_w, tile_h), (cache, &mut frame));
                                interact(n, r);
                            }
                        });
                        ui.add_space(4.0);
                    }
                }
                ViewMode::LargeIcons => {
                    let gap = 8.0;
                    let cols = ((inner_w + gap) / 118.0).floor().max(1.0) as usize;
                    let tile_w = ((inner_w - gap * (cols.saturating_sub(1) as f32)) / cols as f32).floor().max(88.0);
                    let tile_h = 112.0;
                    for row in filtered.chunks(cols) {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            for n in row {
                                let on = selected.as_deref() == Some(n.as_str());
                                let r = draw_block_item(ui, d, n, on, ViewMode::LargeIcons, vec2(tile_w, tile_h), (cache, &mut frame));
                                interact(n, r);
                            }
                        });
                        ui.add_space(6.0);
                    }
                }
            }
        });
    });

    cache.evict(&frame.visible);

    ui.add_space(4.0);

    // Insertion Options section (matching AutoCAD's Options panel at the bottom of the Blocks palette).
    let (opt_rect, opt_resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click());
    ui.painter().rect_filled(opt_rect, 2.0, t.chrome);
    crate::icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(opt_rect.left() + 10.0, opt_rect.center().y), vec2(10.0, 10.0)),
        if st.show_options { crate::icons::Icon::ChevronDown } else { crate::icons::Icon::ChevronRight },
        false,
    );
    ui.painter().text(
        pos2(opt_rect.left() + 22.0, opt_rect.center().y),
        egui::Align2::LEFT_CENTER,
        crate::tl!("Options"),
        crate::theme::body(),
        t.text,
    );
    if opt_resp.clicked() {
        st.show_options = !st.show_options;
    }
    if st.show_options {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.checkbox(&mut st.prompt_scale, crate::tl!("Scale")).on_hover_text(crate::tl!("Specify scale on screen when placing"));
            ui.add_enabled_ui(!st.prompt_scale, |ui| {
                let x_label = ui.label("X:");
                ui.add(egui::DragValue::new(&mut st.scale_x).speed(0.05).range(0.0001..=1e6)).labelled_by(x_label.id);
                let y_label = ui.label("Y:");
                // An exploded insertion scales both axes by X, so Y would be ignored.
                ui.add_enabled_ui(!st.explode, |ui| {
                    let y = ui.add(egui::DragValue::new(&mut st.scale_y).speed(0.05).range(0.0001..=1e6)).labelled_by(y_label.id);
                    y.on_disabled_hover_text(crate::tl!("Exploded insertion uses the X scale for both axes"));
                });
            });
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut st.prompt_rotation, crate::tl!("Rotation")).on_hover_text(crate::tl!("Specify rotation angle on screen when placing"));
            ui.add_enabled_ui(!st.prompt_rotation, |ui| {
                let angle_label = ui.label(crate::tl!("Angle"));
                ui.add(egui::DragValue::new(&mut st.rotation).speed(1.0).range(-360.0..=360.0)).labelled_by(angle_label.id);
            });
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut st.repeat, crate::tl!("Repeat placement"));
            ui.checkbox(&mut st.explode, crate::tl!("Explode"));
            ui.checkbox(&mut st.grab_on_click, crate::tl!("Grab on click"))
                .on_hover_text(crate::tl!("Click or drag a block to take it in hand, then click on the drawing to place"));
        });
    }

    ui.separator();

    // Selected block details and editing options.
    let Some(name) = selected.clone() else {
        if as_panel && ui.button(crate::tl!("Create...")).on_hover_text(crate::tl!("Make a block from objects (BLOCK)")).clicked() {
            *act = Some(Act::Create);
        }
        return;
    };
    if let Some(mut new) = st.rename.take() {
        let mut keep = true;
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut new).desired_width(150.0));
            if ui.button(crate::tl!("OK")).clicked() && !new.trim().is_empty() {
                *act = Some(Act::Rename(name.clone(), new.trim().to_string()));
            }
            if ui.button(crate::tl!("Cancel")).clicked() {
                keep = false;
            }
        });
        if keep {
            st.rename = Some(new);
        }
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&name).strong());
            if let Some((objects, refs, base, annotative, _, _)) = info {
                let summary = format!("({} {}, {} {})", objects, crate::tl!("Objects"), refs, crate::tl!("References"));
                let tip = format!(
                    "{}: {:.4}, {:.4}\n{}: {}",
                    crate::tl!("Base point"),
                    base.x,
                    base.y,
                    crate::tl!("Annotative"),
                    if *annotative { crate::tl!("Yes") } else { crate::tl!("No") }
                );
                ui.label(egui::RichText::new(summary).small().color(t.text_dim)).on_hover_text(tip);
            }
        });
    }
    if let Some((_, _, _, _, description, xref)) = info {
        if let Some(path) = xref {
            ui.label(egui::RichText::new(format!("{}: {path}", crate::tl!("External reference"))).small().color(t.text_dim));
        }
        if !description.is_empty() {
            ui.label(egui::RichText::new(description).small().color(t.text_dim));
        }
    }
    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        if ui.button(crate::tl!("Insert")).clicked() {
            *act = Some(Act::Grab(name.clone()));
        }
        if ui.button(crate::tl!("Edit Block")).on_hover_text(crate::tl!("Open in the Block Editor")).clicked() {
            *act = Some(Act::Edit(name.clone()));
        }
        if ui.button(crate::tl!("Rename...")).clicked() {
            *act = Some(Act::StartRename(name.clone()));
        }
        if ui.button(crate::tl!("Delete")).on_hover_text(crate::tl!("Delete a block that no object uses")).clicked() {
            *act = Some(Act::Delete(name.clone()));
        }
        if can_write && ui.button(crate::tl!("Save to File...")).on_hover_text(crate::tl!("Write the block as a drawing (WBLOCK)")).clicked() {
            *act = Some(Act::Write(name.clone()));
        }
        if as_panel && ui.button(crate::tl!("Create...")).on_hover_text(crate::tl!("Make a block from objects (BLOCK)")).clicked() {
            *act = Some(Act::Create);
        }
    });
    if let Some(n) = &st.note {
        ui.label(egui::RichText::new(n).color(t.text_dim));
    }
}

/// Cancel an in-progress `INSERT` and stop repeat placement.
fn cancel_pending_insert(app: &mut CadApp, st: &mut State) {
    let is_ours = |r: &cadcraft_engine::Running| r.id == "insert" && st.owner == Some(r.serial);
    // Only the palette's own INSERT, running or suspended by a transparent command; never another one.
    let owned = app.session.running.as_ref().is_some_and(is_ours) || app.session.suspended.as_ref().is_some_and(is_ours);
    st.grabbed = false;
    st.owner = None;
    st.repeating = false;
    if owned {
        app.session.cancel();
    }
}

/// An `INSERT` the palette did not start is in progress (running, or suspended by a transparent command).
fn foreign_insert(app: &CadApp, st: &State) -> bool {
    let foreign = |r: &cadcraft_engine::Running| r.id == "insert" && st.owner != Some(r.serial);
    app.session.running.as_ref().is_some_and(foreign) || app.session.suspended.as_ref().is_some_and(foreign)
}

/// Refuse a drawing edit while someone else's `INSERT` runs: its undo snapshot would predate the
/// edit, so undoing the insertion would also undo the palette's change.
fn refuse_during_foreign_insert(app: &CadApp, st: &mut State) -> bool {
    if !foreign_insert(app, st) {
        return false;
    }
    st.note = Some(crate::tl!("Finish or cancel the running INSERT first").to_string());
    true
}

/// Do what a button asked; false when the manager window should close (a command takes over).
fn apply(app: &mut CadApp, st: &mut State, act: Option<Act>, esc_pressed: bool) -> bool {
    let is_ours = |r: &cadcraft_engine::Running| r.id == "insert" && st.owner == Some(r.serial);
    let ours = app.session.running.as_ref().is_some_and(is_ours);
    // A transparent command in front of it only suspends the palette's INSERT.
    let owned = ours || app.session.suspended.as_ref().is_some_and(is_ours);
    if !owned {
        st.grabbed = false;
        st.owner = None;
    }
    if esc_pressed {
        if st.repeating {
            st.repeat = false;
        }
        cancel_pending_insert(app, st);
    }
    let edits = match &act {
        Some(Act::Edit(_) | Act::Create | Act::Delete(_)) => true,
        Some(Act::Rename(from, to)) => from != to,
        _ => false,
    };
    if edits && refuse_during_foreign_insert(app, st) {
        return true;
    }
    match act {
        Some(Act::Grab(n)) => {
            st.rename = None;
            grab_block(app, &n, st);
            // Only track repeat-placement when the palette's own INSERT started.
            st.owner = app.session.running.as_ref().filter(|r| r.id == "insert").map(|r| r.serial);
            st.grabbed = st.owner.is_some();
            st.repeating = st.repeat && st.grabbed;
            return true;
        }
        Some(Act::Edit(n)) => {
            cancel_pending_insert(app, st);
            if let Err(e) = app.run("bedit", json!({ "name": n })) {
                app.session.echo(e);
            }
            return false;
        }
        Some(Act::Create) => {
            cancel_pending_insert(app, st);
            app.start("block");
            return false;
        }
        Some(Act::StartRename(n)) => {
            cancel_pending_insert(app, st);
            st.rename = Some(n);
            return true;
        }
        // Confirming the name unchanged is not an edit: keep the placement in progress.
        Some(Act::Rename(from, to)) if from == to => st.rename = None,
        Some(Act::Rename(from, to)) => {
            cancel_pending_insert(app, st);
            match app.run("rename", json!({ "table": "block", "from": from, "to": to })) {
                Ok(_) => {
                    st.selected = Some(to);
                    st.rename = None;
                    st.note = None;
                }
                Err(e) => st.note = Some(e),
            }
        }
        Some(Act::Delete(n)) => {
            cancel_pending_insert(app, st);
            let refs = app.session.doc().map(|d| reference_count(d, &n)).unwrap_or(0);
            if refs > 0 {
                st.note = Some(format!("{} ({refs})", crate::tl!("The block is in use and can't be deleted")));
            } else {
                match app.run("purge", json!({ "type": "blocks", "names": n })) {
                    Ok(_) => {
                        st.selected = None;
                        st.note = None;
                    }
                    Err(e) => st.note = Some(e),
                }
            }
        }
        Some(Act::Write(n)) => {
            cancel_pending_insert(app, st);
            if let Some(pick) = app.services.pick_save.as_ref()
                && let Some(path) = pick(&format!("{n}.dwg"))
                && let Err(e) = app.run("wblock", json!({ "path": path, "name": n }))
            {
                st.note = Some(e);
            }
        }
        Some(Act::ToPanel) => {
            app.ui.blocks_window = false;
            let _ = open_panel(app);
            return false;
        }
        Some(Act::ToWindow) => {
            app.ui.blocks_window = true;
            let _ = crate::docking::command(app, false, &json!({ "operation": "close", "panel": crate::docking::Panel::Blocks.id() }));
            app.ui.dialog = Some("blocks".into());
        }
        None => {
            if st.repeating {
                if esc_pressed || !owned {
                    // Repeat INSERT ended (cancelled with Esc or replaced by another command).
                    st.repeating = false;
                    st.repeat = false;
                } else if !st.repeat && ours && app.session.input(cadcraft_engine::Input::Keyword("NoRepeat".into())).is_ok() {
                    // Repeat placement was unchecked in the palette while INSERT was running;
                    // clear palette tracking only after the engine accepts NoRepeat.
                    st.repeating = false;
                }
            } else if st.repeat && st.grabbed && ours && app.session.input(cadcraft_engine::Input::Keyword("Repeat".into())).is_ok() {
                // Repeat placement was checked after grabbing the block; arm repeat on the running INSERT.
                st.repeating = true;
            }
        }
    }
    true
}

/// Open the Block Manager the way it was last used: a panel pinned on the right (the first
/// time) or a window.
pub fn open(app: &mut CadApp) -> Result<serde_json::Value, String> {
    if app.ui.blocks_window {
        app.ui.dialog = Some("blocks".into());
        return Ok(serde_json::Value::Null);
    }
    open_panel(app)
}

/// Show the Blocks panel, docked on the right of the drawing.
fn open_panel(app: &mut CadApp) -> Result<serde_json::Value, String> {
    if app.ui.dialog.as_deref() == Some("blocks") {
        app.ui.dialog = None;
    }
    let panel = crate::docking::Panel::Blocks;
    let first = !app.ui.docking.layout.contains(&panel) && !app.ui.docking.hidden.iter().any(|(p, _)| *p == panel);
    let r = crate::docking::command(app, false, &json!({ "operation": "open", "panel": panel.id() }))?;
    // The first time: pinned at the right edge, beside the Layers and Properties palettes.
    if first && app.ui.docking.layout.contains(&crate::docking::Panel::Properties) {
        return crate::docking::command(app, false, &json!({ "operation": "move", "panel": panel.id(), "target": "properties", "zone": "right" }));
    }
    Ok(r)
}

/// The Blocks panel.
pub fn panel(app: &mut CadApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let mut st = ctx.data(|d| d.get_temp::<State>(state_id())).unwrap_or_default();
    let mut cache = ctx.data(|d| d.get_temp::<PreviewCache>(cache_id())).unwrap_or_default();
    let Some(v) = view(app, &mut st, &mut cache) else { return };
    let mut act = None;
    if let Ok(doc) = app.session.state().map(|s| Arc::clone(&s.doc)) {
        egui::ScrollArea::vertical().id_salt("dock-blocks").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::NONE.inner_margin(6.0).show(ui, |ui| body(ui, &doc, &v, &mut st, &mut cache, true, &mut act));
        });
    }
    // The panel stays open whatever the button did.
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    apply(app, &mut st, act, esc);
    ctx.data_mut(|d| {
        d.insert_temp(state_id(), st);
        d.insert_temp(cache_id(), cache);
    });
}

/// The Block Manager as a window.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let mut st = ctx.data(|d| d.get_temp::<State>(state_id())).unwrap_or_default();
    let mut cache = ctx.data(|d| d.get_temp::<PreviewCache>(cache_id())).unwrap_or_default();
    let Some(v) = view(app, &mut st, &mut cache) else { return };
    let mut act = None;
    if let Ok(doc) = app.session.state().map(|s| Arc::clone(&s.doc)) {
        egui::Window::new(crate::tl!("Blocks"))
            .id(egui::Id::unique("Blocks"))
            .open(open)
            .default_size(vec2(640.0, 480.0))
            .min_size(vec2(420.0, 320.0))
            .resizable(true)
            .show(ctx, |ui| body(ui, &doc, &v, &mut st, &mut cache, false, &mut act));
    }
    let to_panel = matches!(act, Some(Act::ToPanel));
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if !apply(app, &mut st, act, esc) && !to_panel {
        *open = false;
    }
    if to_panel {
        *open = false;
    }
    if !*open {
        cache.entries.clear();
    }
    ctx.data_mut(|d| {
        d.insert_temp(state_id(), st);
        d.insert_temp(cache_id(), cache);
    });
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    use cadcraft_engine::doc::EntityKind;
    use cadcraft_engine::geom::Vec2;
    use cadcraft_engine::{Input, Session};
    use serde_json::json;

    use super::{PreviewCache, PreviewResult, State, ViewMode, grab_block};

    fn ready(d: &cadcraft_doc::Drawing, name: &str) -> (cadcraft_render::DisplayList, cadcraft_geom::Bounds2) {
        let PreviewResult::Ready(p) = super::build(d, name) else {
            panic!("expected ready {name}");
        };
        ((*p.list).clone(), p.bounds)
    }
    use crate::{CadApp, Services};

    /// Programmatic INSERT (including aliases and missing parameters) never opens a palette.
    #[test]
    fn insert_dispatch_distinguishes_programmatic_and_interactive_calls() {
        for id in ["insert", "i", "ddinsert"] {
            let mut app = CadApp::new(Session::new(), Services::default());
            app.ui.blocks_window = true;
            for params in [serde_json::Value::Null, json!({})] {
                assert!(app.run(id, params).is_err());
                assert!(app.ui.dialog.is_none());
                assert!(!app.ui.docking.layout.contains(&crate::docking::Panel::Blocks));
            }
            app.start(id);
            assert_eq!(app.ui.dialog.as_deref(), Some("blocks"));
            app.ui.dialog = None;
            assert!(app.cmdline(id).is_none());
            assert_eq!(app.ui.dialog.as_deref(), Some("blocks"));
            app.ui.dialog = None;
            assert!(app.cmdline(&format!("{id} {{}}")).is_some());
            assert!(app.ui.dialog.is_none());
            // Inline arguments (e.g. `insert Pin 10,20`) must not open the palette and lose arguments.
            let _ = app.cmdline(&format!("{id} Pin 10,20"));
            assert!(app.ui.dialog.is_none());
            assert!(app.session.running.is_some());
        }
        let mut app = CadApp::new(Session::new(), Services::default());
        app.ui.blocks_window = true;
        app.run("ui.dialog.blocks", serde_json::Value::Null).unwrap();
        assert_eq!(app.ui.dialog.as_deref(), Some("blocks"));
    }

    /// Reuse unchanged data; refresh counts after selection, edits, undo and drawing switches.
    #[test]
    fn metadata_cache_follows_revision_selection_and_drawing() {
        let mut app = CadApp::new(Session::new(), Services::default());
        for name in ["Bolt", "Pin"] {
            let c = app.session.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap();
            app.session.execute("block", &json!({"name": name, "base": [0, 0], "handles": [c["handle"]], "keep": "delete"})).unwrap();
        }
        let mut st = State::default();
        let mut cache = PreviewCache::default();
        let first = super::view(&app, &mut st, &mut cache).unwrap();
        let again = super::view(&app, &mut st, &mut cache).unwrap();
        assert!(Arc::ptr_eq(&first.all, &again.all));
        assert_eq!(first.info.unwrap().1, 0);
        app.run("insert", json!({"name": "Bolt", "at": [5, 5]})).unwrap();
        let edited = super::view(&app, &mut st, &mut cache).unwrap();
        assert!(!Arc::ptr_eq(&first.all, &edited.all));
        assert_eq!(edited.info.unwrap().1, 1);
        st.selected = Some("Pin".into());
        let other = super::view(&app, &mut st, &mut cache).unwrap();
        assert!(Arc::ptr_eq(&edited.all, &other.all));
        assert_eq!(other.info.unwrap().1, 0);
        st.selected = Some("Bolt".into());
        assert_eq!(super::view(&app, &mut st, &mut cache).unwrap().info.unwrap().1, 1);
        app.session.undo().unwrap();
        assert_eq!(super::view(&app, &mut st, &mut cache).unwrap().info.unwrap().1, 0);
        app.session.new_drawing(true);
        let empty = super::view(&app, &mut st, &mut cache).unwrap();
        assert!(empty.all.is_empty());
        assert!(empty.selected.is_none());
        assert!(cache.references.is_none());
    }

    /// Ordinary thumbnails retain fills, motifs and curved boundaries, matching normal rendering.
    #[test]
    fn ordinary_hatches_keep_fills_and_patterns_in_previews() {
        use cadcraft_doc::{Block, Drawing, Entity, Handle, Hatch};
        let mut d = Drawing::default();
        for (name, solid, bulge) in [("Solid", true, 0.0), ("Pattern", false, 0.0), ("Curved", true, 1.0)] {
            let mut hatch: Hatch = serde_json::from_value(json!({
                "pattern": if solid { "SOLID" } else { "ANSI31" }, "solid": solid,
                "loops": []
            }))
            .unwrap();
            let mut vertices: Vec<_> = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]
                .into_iter()
                .map(cadcraft_geom::PolyVertex::new)
                .collect();
            vertices[0].bulge = bulge;
            hatch.loops.push(cadcraft_doc::HatchLoop { vertices, outer: true, sources: Vec::new() });
            let mut b = Block::new(name);
            b.entities.push(Entity::new(Handle(1), EntityKind::Hatch(hatch)));
            d.blocks.insert(name.into(), Arc::new(b));
            let (list, bounds) = ready(&d, name);
            assert!(!bounds.is_empty(), "{name}");
            assert!(!list.prims.is_empty(), "{name}");
            if solid {
                assert!(!list.tris.is_empty(), "{name}: filled thumbnail, not just an outline");
            } else {
                let polylines = list.prims.iter().filter(|p| p.kind == cadcraft_render::Kind::Polyline).count();
                assert!(polylines > 1, "pattern lines are retained");
            }
        }
        let normal = cadcraft_render::build_entities(&d, d.block("Solid").unwrap().entities.iter().map(|e| &**e), &Default::default());
        assert!(!normal.tris.is_empty(), "normal canvas rendering still fills hatches");
    }

    /// Escape in a filter, dialog or unrelated command must preserve the user's preference.
    #[test]
    fn unrelated_escape_preserves_repeat_preference() {
        let mut app = CadApp::new(Session::new(), Services::default());
        let mut st = State { repeat: true, ..Default::default() };
        super::apply(&mut app, &mut st, None, true);
        assert!(st.repeat);
        app.session.start("line").unwrap();
        super::apply(&mut app, &mut st, None, true);
        assert!(st.repeat);
        assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("line"));
    }

    /// Feed the first and second click releases as egui reports them, plus drag-only gestures.
    #[test]
    fn double_click_grabs_only_once_in_either_mode() {
        for grab_on_click in [false, true] {
            let grabs = [(false, true, false), (false, true, true)]
                .into_iter()
                .filter(|&(drag, click, dbl)| super::should_grab(drag, click, dbl, grab_on_click))
                .count();
            assert_eq!(grabs, 1);
            assert!(super::should_grab(true, false, false, grab_on_click));
            assert!(!super::should_grab(false, false, false, grab_on_click));
        }
    }

    /// Verify block enumeration, reference counting and preview display-list generation.
    #[test]
    fn blocks_list_their_references_and_build_a_preview() {
        let mut s = Session::new();
        let c = s.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap();
        let h = c["handle"].as_str().unwrap().to_string();
        s.execute("block", &json!({"name": "Bolt", "base": [0, 0], "handles": [h], "keep": "delete"})).unwrap();
        s.execute("insert", &json!({"name": "Bolt", "at": [5, 5]})).unwrap();
        s.execute("insert", &json!({"name": "Bolt", "at": [9, 5]})).unwrap();
        let d = s.doc().unwrap();
        assert_eq!(super::names(d), ["Bolt"]);
        assert_eq!(super::reference_count(d, "bolt"), 2);
        let (list, bounds) = ready(d, "Bolt");
        assert!(!list.prims.is_empty());
        assert!((bounds.width() - 4.0).abs() < 1e-6);
    }

    /// Verify view-mode index round-tripping and retention across scrolling.
    #[test]
    fn offscreen_previews_are_reused_and_view_modes_roundtrip() {
        for (idx, mode) in [(0, ViewMode::List), (1, ViewMode::MediumIcons), (2, ViewMode::LargeIcons)] {
            assert_eq!(ViewMode::from_index(idx), mode);
            assert_eq!(mode.index(), idx);
        }
        let mut s = Session::new();
        for name in ["B1", "B2", "B3"] {
            let c = s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
            let h = c["handle"].as_str().unwrap().to_string();
            s.execute("block", &json!({"name": name, "base": [0, 0], "handles": [h], "keep": "delete"})).unwrap();
        }
        let d = s.doc().unwrap();
        let mut cache = PreviewCache::default();
        cache.sync_revision(1, 1);
        for name in ["B1", "B2", "B3"] {
            let (list, bounds) = ready(d, name);
            cache.insert(name.into(), PreviewResult::Ready(super::Preview { list: Arc::new(list), bounds }), &HashSet::new());
        }
        assert_eq!(cache.len(), 3);

        // Only B2 remains visible; a return to B1/B3 reuses their cached results.
        let visible: HashSet<String> = ["B2".to_string()].into_iter().collect();
        cache.evict(&visible);
        assert_eq!(cache.len(), 3);
        assert!(cache.entries.contains_key("B2"));
        assert!(cache.entries.contains_key("B1"));
        assert!(cache.entries.contains_key("B3"));

        // A new drawing revision clears everything.
        cache.sync_revision(1, 2);
        assert!(cache.is_empty());
    }

    /// Verify that grabbing a block presets scale/rotation and places it on a single canvas click.
    #[test]
    fn grab_block_puts_it_in_hand_and_places_on_single_click() {
        let mut app = CadApp::new(Session::new(), Services::default());
        let c = app.session.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap();
        let h = c["handle"].as_str().unwrap().to_string();
        app.session.execute("block", &json!({"name": "Valve", "base": [0, 0], "handles": [h], "keep": "delete"})).unwrap();

        let header = &mut app.session.doc_mut().unwrap().header;
        header.set_i64("AUNITS", 3);
        header.set_f64("ANGBASE", 0.5);
        header.set_i64("ANGDIR", 1);
        let st = State { scale_x: 2.0, scale_y: 1.5, rotation: 90.0, ..Default::default() };
        grab_block(&mut app, "Valve", &st);

        // The block is now in hand at the insertion point prompt, with live preview under the cursor.
        assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("insert"));
        assert_eq!(app.session.current_prompt().map(|p| p.message), Some("Specify insertion point".to_string()));
        let preview = app.session.preview(Vec2::new(10.0, 20.0));
        assert_eq!(preview.len(), 1);

        // A single click on the canvas drops the block immediately without extra scale/rotation prompts.
        app.session.input(Input::Point(Vec2::new(10.0, 20.0))).unwrap();
        assert!(app.session.running.is_none());

        let d = app.session.doc().unwrap();
        let ins: Vec<_> = d
            .model
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::Insert(i) => Some(i),
                _ => None,
            })
            .collect();
        assert_eq!(ins.len(), 1);
        assert_eq!(ins[0].block, "Valve");
        assert!((ins[0].insert.x - 10.0).abs() < 1e-9 && (ins[0].insert.y - 20.0).abs() < 1e-9);
        assert!((ins[0].scale.x - 2.0).abs() < 1e-9 && (ins[0].scale.y - 1.5).abs() < 1e-9);
        assert!((ins[0].rotation.to_degrees() - 90.0).abs() < 1e-6);
    }

    /// Verify that the scroll viewport only builds and retains previews for visible blocks.
    #[test]
    fn palette_loads_only_visible_blocks_in_scroll_viewport() {
        let mut app = CadApp::new(Session::new(), Services::default());
        for i in 0..40 {
            let c = app.session.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
            let h = c["handle"].as_str().unwrap().to_string();
            app.session.execute("block", &json!({"name": format!("BLK_{i:02}"), "base": [0, 0], "handles": [h], "keep": "delete"})).unwrap();
        }
        let ctx = egui::Context::default();
        let screen = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 420.0)));
        for mode in [ViewMode::LargeIcons, ViewMode::MediumIcons, ViewMode::List] {
            ctx.data_mut(|d| {
                let mut st = d.get_temp::<State>(super::state_id()).unwrap_or_default();
                st.view_mode = mode;
                d.insert_temp(super::state_id(), st);
            });
            for _ in 0..6 {
                let input = egui::RawInput { screen_rect: screen, ..Default::default() };
                let mut out = ctx.run_ui(input, |ui| super::panel(&mut app, ui));
                out.textures_delta.clear();
            }
            let count = super::cached_preview_count(&ctx);
            assert!(count > 0 && count < 40, "mode {mode:?} should keep only visible blocks in memory, got {count}");
        }
    }

    #[test]
    fn repeat_preference_only_arms_palette_insertions() {
        let mut app = CadApp::new(Session::new(), Services::default());
        let circle = app.session.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap();
        app.session.execute("block", &json!({"name": "Pin", "base": [0, 0], "handles": [circle["handle"]], "keep": "delete"})).unwrap();
        let mut st = State { repeat: true, ..Default::default() };

        // A command-line insertion must finish after one placement despite the palette preference.
        app.session.start("insert").unwrap();
        app.session.input(Input::Text("Pin".into())).unwrap();
        super::apply(&mut app, &mut st, None, false);
        assert!(!st.repeating);
        app.session.input(Input::Point(Vec2::new(10.0, 0.0))).unwrap();
        app.session.input(Input::Enter).unwrap(); // X scale
        app.session.input(Input::Enter).unwrap(); // Y scale
        app.session.input(Input::Text("0".into())).unwrap(); // Rotation
        assert!(app.session.running.is_none());
        super::apply(&mut app, &mut st, None, true);
        assert!(st.repeat);

        // Enabling Repeat after grabbing from the palette still arms repeated placement.
        st.repeat = false;
        super::apply(&mut app, &mut st, Some(super::Act::Grab("Pin".into())), false);
        assert!(st.grabbed);
        st.repeat = true;
        super::apply(&mut app, &mut st, None, false);
        app.session.input(Input::Point(Vec2::new(20.0, 0.0))).unwrap();
        assert!(app.session.running.is_some());
        assert!(st.repeating);
        super::apply(&mut app, &mut st, None, true);
        assert!(!st.grabbed);
        assert!(app.session.running.is_none());
    }

    /// Verify repeat-placement re-grabbing, Escape cancellation, and cancellation when another command starts.
    #[test]
    fn repeat_placement_places_multiple_times_and_escape_cancels_it() {
        let mut app = CadApp::new(Session::new(), Services::default());
        let c = app.session.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap();
        let h = c["handle"].as_str().unwrap().to_string();
        app.session.execute("block", &json!({"name": "Pin", "base": [0, 0], "handles": [h], "keep": "delete"})).unwrap();

        let mut st = State { repeat: true, ..Default::default() };

        // Grab "Pin" with Repeat placement enabled.
        super::apply(&mut app, &mut st, Some(super::Act::Grab("Pin".into())), false);
        assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("insert"));
        assert!(st.repeating);

        // First click places the first reference and keeps INSERT running for the next point.
        app.session.input(Input::Point(Vec2::new(10.0, 0.0))).unwrap();
        assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("insert"));
        super::apply(&mut app, &mut st, None, false);
        assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("insert"));
        assert_eq!(super::reference_count(app.session.doc().unwrap(), "Pin"), 1);

        // Second click places the second reference and still keeps INSERT running.
        app.session.input(Input::Point(Vec2::new(20.0, 0.0))).unwrap();
        super::apply(&mut app, &mut st, None, false);
        assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("insert"));
        assert_eq!(super::reference_count(app.session.doc().unwrap(), "Pin"), 2);

        // Pressing Escape cancels the running INSERT and stops repeat placement.
        app.session.cancel();
        super::apply(&mut app, &mut st, None, false);
        assert!(app.session.running.is_none());
        assert!(!st.repeating);
        assert!(!st.repeat);

        // Pressing Escape while INSERT is still running cancels both and unchecks repeat.
        st.repeat = true;
        super::apply(&mut app, &mut st, Some(super::Act::Grab("Pin".into())), false);
        assert!(app.session.running.is_some());
        super::apply(&mut app, &mut st, None, true);
        assert!(app.session.running.is_none());
        assert!(!st.repeating);
        assert!(!st.repeat);

        // Starting another command (e.g. LINE) replaces INSERT and clears repeating so finishing LINE does not re-grab.
        st.repeat = true;
        super::apply(&mut app, &mut st, Some(super::Act::Grab("Pin".into())), false);
        assert!(st.repeating);
        app.session.start("line").unwrap();
        super::apply(&mut app, &mut st, None, false);
        assert!(!st.repeating);
        assert!(!st.repeat);

        // A transparent command only suspends the palette's INSERT: the palette keeps it.
        app.session.cancel();
        st.repeat = true;
        super::apply(&mut app, &mut st, Some(super::Act::Grab("Pin".into())), false);
        app.session.start("'dist").unwrap();
        assert!(app.session.suspended.is_some());
        super::apply(&mut app, &mut st, None, false);
        assert!(st.repeating);
        assert!(st.grabbed);

        // An INSERT started by anything else (typed with an apostrophe, say) is not the palette's to repeat.
        app.session.cancel();
        st.repeat = true;
        super::apply(&mut app, &mut st, Some(super::Act::Grab("Pin".into())), false);
        assert!(st.repeating);
        app.session.cancel();
        app.session.start("insert").unwrap();
        super::apply(&mut app, &mut st, None, false);
        assert!(!st.repeating);
        assert!(!st.grabbed);
        assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("insert"));
    }

    /// Confirming a block name unchanged closes the editor and leaves a placement in progress alone.
    #[test]
    fn renaming_to_the_same_name_keeps_the_insert_running() {
        let mut app = CadApp::new(Session::new(), Services::default());
        let circle = app.session.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap();
        app.session.execute("block", &json!({"name": "Pin", "base": [0, 0], "handles": [circle["handle"]], "keep": "delete"})).unwrap();
        let mut st = State::default();
        super::apply(&mut app, &mut st, Some(super::Act::Grab("Pin".into())), false);
        assert_eq!(app.session.current_prompt().map(|p| p.message), Some("Specify insertion point".to_string()));
        st.rename = Some("Pin".into());
        super::apply(&mut app, &mut st, Some(super::Act::Rename("Pin".into(), "Pin".into())), false);
        assert!(st.rename.is_none());
        assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("insert"));
        assert_eq!(app.session.current_prompt().map(|p| p.message), Some("Specify insertion point".to_string()));
    }

    /// Clicking Insert while the rename editor is open clears rename and starts insertion without cancelling it on the next frame.
    #[test]
    fn insert_while_renaming_clears_rename_and_keeps_insert() {
        let mut app = CadApp::new(Session::new(), Services::default());
        let mut st = State { rename: Some("NewName".into()), ..Default::default() };
        super::apply(&mut app, &mut st, Some(super::Act::Grab("Pin".into())), false);
        assert!(st.rename.is_none());
        assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("insert"));

        // On the next frame, INSERT is still running.
        super::apply(&mut app, &mut st, None, false);
        assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("insert"));
    }

    /// Edits that change the drawing are refused while an INSERT the palette did not start is running.
    #[test]
    fn the_palette_refuses_edits_during_a_foreign_insert() {
        let mut app = CadApp::new(Session::new(), Services::default());
        let circle = app.session.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap();
        app.session.execute("block", &json!({"name": "Pin", "base": [0, 0], "handles": [circle["handle"]], "keep": "delete"})).unwrap();
        let mut st = State::default();
        app.session.start("insert").unwrap();
        for act in [super::Act::Rename("Pin".into(), "Pin2".into()), super::Act::Delete("Pin".into()), super::Act::Edit("Pin".into()), super::Act::Create] {
            assert!(super::apply(&mut app, &mut st, Some(act), false));
            assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("insert"));
            assert!(st.note.is_some());
            st.note = None;
        }
        assert!(app.session.doc().is_ok_and(|d| d.blocks.contains_key("Pin") && !d.blocks.contains_key("Pin2")));
    }

    /// A palette action cancels only the INSERT the palette started, which Esc ends even while a transparent command has it suspended.
    #[test]
    fn the_palette_cancels_only_its_own_insert() {
        let mut app = CadApp::new(Session::new(), Services::default());
        let circle = app.session.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap();
        app.session.execute("block", &json!({"name": "Pin", "base": [0, 0], "handles": [circle["handle"]], "keep": "delete"})).unwrap();
        let mut st = State::default();

        // An INSERT started elsewhere survives a palette action.
        app.session.start("insert").unwrap();
        super::cancel_pending_insert(&mut app, &mut st);
        assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("insert"));
        app.session.cancel();

        // The palette's own INSERT ends with Esc, with a transparent command in front of it too.
        super::apply(&mut app, &mut st, Some(super::Act::Grab("Pin".into())), false);
        app.session.start("'dist").unwrap();
        assert!(app.session.suspended.is_some());
        super::apply(&mut app, &mut st, None, true);
        assert!(app.session.running.is_none());
        assert!(app.session.suspended.is_none());
    }
}

#[cfg(test)]
mod library_audit;
