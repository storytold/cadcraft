//! The Blocks palette: the drawing's own blocks as a gallery (Current Drawing) and the block
//! libraries the user added (Libraries): folders of drawings and single drawing files, kept in
//! a list that can be added to, refreshed and removed from. A name filter, the gallery's view
//! styles and the insertion options (scale, rotation, explode) apply to both tabs.
//!
//! A thin view over the engine: the lists come from the `blockspalette`, `blocklibraries` and
//! `blocklibrary` commands, a library block is defined here by `blocklibimport`, and a block is
//! placed by starting INSERT and answering its first prompts, so the command line shows what the
//! palette did and the insertion point is picked on the drawing as usual.
//!
//! Block definitions come from files, so they are hostile input: a picture is built only for a
//! tile on screen, a few per frame, under a deadline and a size limit, and a panic or a refused
//! picture never stops the block from being placed.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use cadcraft_engine::Input;
use cadcraft_engine::doc::{Block, Drawing, Entity, EntityKind, HatchLoop, Insert, MAX_BLOCK_DEPTH};
use cadcraft_engine::geom::{Bounds2, Vec2};
use cadcraft_engine::render::{DisplayList, Kind, Options, build_entities};
use egui::{Color32, Pos2, RichText, Sense, Stroke, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::docking::Panel;

/// Points kept per block preview: a block with more geometry shows its first part only.
const PREVIEW_POINTS: usize = 30_000;
/// How finely a preview is drawn: the chord tolerance is the block's size over this.
const PREVIEW_DETAIL: f64 = 300.0;
/// Most objects a block can hold and still get a preview.
const PREVIEW_ENTITIES: usize = 50_000;
/// Most work (vertices, control points and repeated references, counted before drawing) a
/// preview may ask for: a single object can hold millions of vertices.
const PREVIEW_WORK: usize = 500_000;
/// What a character costs in that count: a glyph takes several times the work of a vertex.
const TEXT_WORK: usize = 4;
/// The shortest dash pattern the pass that finds the size draws: none, they do not change it.
const NO_DASHES: f64 = 1e30;
/// How long a preview pass may keep the interface waiting before it is given up.
const PREVIEW_DEADLINE: Duration = Duration::from_millis(100);
/// Most previews built in one frame; the others wait for the next frames.
const BUILDS_PER_FRAME: usize = 4;
/// Most points the previews in memory hold together.
const MAX_CACHED_POINTS: usize = 4_000_000;
/// Most previews remembered, refused ones included.
const MAX_CACHED_PICTURES: usize = 50_000;
/// The most characters of a block name and of a description shown.
const MAX_NAME_CHARS: usize = 120;
const MAX_DESCRIPTION_CHARS: usize = 240;

/// BLOCKSPALETTE typed, Window ▸ Blocks and the toolbar icon open this palette. JSON calls (with parameters)
/// run the engine's command and list the blocks.
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    // The toolbar's Blocks icon and Window ▸ Blocks open the palette too, as the other ui.dialog.*
    // ids do, whatever the parameters.
    if id == "ui.dialog.blocks" {
        return Some(open_palette(app));
    }
    if !params.is_null() {
        return None;
    }
    match cadcraft_engine::cmd::resolve_alias(id).as_str() {
        "blockspalette" => Some(open_palette(app)),
        _ => None,
    }
}

/// Show the palette. The first time it docks below the other palettes; after that it comes back
/// where it was.
fn open_palette(app: &mut CadApp) -> Result<Value, String> {
    crate::docking::command(app, false, &json!({ "operation": "open", "panel": Panel::Blocks.id() }))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Current,
    Libraries,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum View {
    ExtraLarge,
    Large,
    #[default]
    Medium,
    Small,
    Details,
    List,
}

impl View {
    const ALL: [View; 6] = [View::ExtraLarge, View::Large, View::Medium, View::Small, View::Details, View::List];

    /// The name shown on the view's button.
    fn label(self) -> &'static str {
        match self {
            View::ExtraLarge => crate::tl!("Extra Large Icons"),
            View::Large => crate::tl!("Large Icons"),
            View::Medium => crate::tl!("Medium Icons"),
            View::Small => crate::tl!("Small Icons"),
            View::Details => crate::tl!("Details"),
            View::List => crate::tl!("List"),
        }
    }

    /// The side of a gallery tile, for the icon views.
    fn tile(self) -> Option<f32> {
        match self {
            View::ExtraLarge => Some(150.0),
            View::Large => Some(104.0),
            View::Medium => Some(76.0),
            View::Small => Some(52.0),
            View::Details | View::List => None,
        }
    }
}

/// What the palette remembers between frames and drawings.
#[derive(Clone, Debug)]
struct State {
    tab: Tab,
    view: View,
    filter: String,
    selected: Option<String>,
    /// The libraries the user added (the engine keeps the list), once read.
    libraries: Option<Vec<String>>,
    /// The library whose contents the gallery shows.
    library: Option<String>,
    /// The folder or file path being typed to add a library.
    new_library: String,
    /// Bumped to read the library again.
    refresh: u32,
    /// The library is being read: Refresh waits for it, so clicks do not pile up reads.
    reading: bool,
    /// Scale and rotation are asked on the drawing (the INSERT prompts) while these are on;
    /// off, the values below are used without asking.
    scale_on_screen: bool,
    scale: [f64; 3],
    rotation_on_screen: bool,
    rotation: f64,
    explode: bool,
    /// Place the block again once it is placed, until the command is cancelled.
    repeat: bool,
    /// The block being placed again, while its INSERT runs.
    repeating: Option<Repeating>,
    /// The pass that last drew the palette: what ran while it was hidden went unwatched.
    last_pass: u64,
}

/// A placement the palette started and repeats, until its INSERT ends without placing anything,
/// another command starts or the drawing changes.
#[derive(Clone, Debug)]
struct Repeating {
    item: Item,
    /// The drawing it goes into.
    uid: u64,
    /// The INSERT's number in the session's count of commands.
    started: u64,
    explode: bool,
}

impl Repeating {
    /// The placement of `item`, whose INSERT has just started; `None` with no drawing.
    fn started_now(app: &CadApp, item: Item, explode: bool) -> Option<Self> {
        let uid = drawing_uid(app)?;
        Some(Repeating { item, uid, started: app.session.commands_started, explode })
    }
}

impl Default for State {
    fn default() -> Self {
        State {
            tab: Tab::default(),
            view: View::default(),
            filter: String::new(),
            selected: None,
            libraries: None,
            library: None,
            new_library: String::new(),
            refresh: 0,
            reading: false,
            scale_on_screen: false,
            scale: [1.0; 3],
            rotation_on_screen: false,
            rotation: 0.0,
            explode: false,
            repeat: true,
            repeating: None,
            last_pass: 0,
        }
    }
}

/// Where a gallery item comes from.
#[derive(Clone, Debug, PartialEq)]
enum Source {
    /// A block of the current drawing.
    Current,
    /// A block of a library drawing, defined here when placed.
    LibraryBlock { path: String },
    /// A drawing of a library folder, placed as a block named after it.
    LibraryDrawing { path: String },
}

/// A block and the drawing it belongs to, to draw its picture when it is needed.
#[derive(Clone)]
struct Lazy {
    doc: Arc<Drawing>,
    block: Arc<Block>,
}

impl std::fmt::Debug for Lazy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Lazy({})", self.block.name)
    }
}

/// One tile of the gallery.
#[derive(Clone, Debug)]
struct Item {
    /// What the item is selected by: unique within the gallery.
    id: String,
    /// The block name (the file name of a drawing).
    label: String,
    description: String,
    /// What the picture is drawn from once the tile is on screen; `None` has no picture.
    picture: Option<Lazy>,
    source: Source,
}

impl PartialEq for Item {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

/// A gallery's items, and whether a hostile or huge file left some of them out.
#[derive(Clone, Debug, Default)]
struct Listing {
    items: Arc<Vec<Item>>,
    truncated: bool,
}

/// A block's preview, or the fact that it cannot have one.
#[derive(Clone, Debug)]
enum Picture {
    Ready(Arc<Shape>),
    Unavailable,
}

/// A remembered preview.
#[derive(Debug)]
struct Kept {
    picture: Picture,
    /// The frame that last showed it.
    used: u64,
    points: usize,
    /// It belongs to the current drawing's tab.
    current: bool,
}

/// The previews in memory, by item and block definition, bounded in points: what no frame shows
/// goes first, so scrolling back rebuilds as little as possible and visible tiles never thrash.
#[derive(Debug, Default)]
struct Pictures {
    map: HashMap<(String, usize), Kept>,
    points: usize,
    tick: u64,
}

impl Pictures {
    /// The preview kept for `key`, marked as shown on this frame.
    fn get(&mut self, key: &(String, usize)) -> Option<Picture> {
        let tick = self.tick;
        self.map.get_mut(key).map(|k| {
            k.used = tick;
            k.picture.clone()
        })
    }

    /// Keep `picture` under `key`, dropping the oldest unshown ones while over the point budget.
    fn insert(&mut self, key: (String, usize), picture: Picture, current: bool) {
        if self.map.len() >= MAX_CACHED_PICTURES {
            self.map.clear();
            self.points = 0;
        }
        let points = match &picture {
            Picture::Ready(shape) => shape.points(),
            Picture::Unavailable => 0,
        };
        self.points = self.points.saturating_add(points);
        if let Some(old) = self.map.insert(key, Kept { picture, used: self.tick, points, current }) {
            self.points = self.points.saturating_sub(old.points);
        }
        while self.points > MAX_CACHED_POINTS {
            let oldest = self.map.iter().filter(|(_, k)| k.used < self.tick && k.points > 0).min_by_key(|(_, k)| k.used).map(|(key, _)| key.clone());
            let Some(kept) = oldest.and_then(|key| self.map.remove(&key)) else { break };
            self.points = self.points.saturating_sub(kept.points);
        }
    }

    /// Forget the current drawing's previews: a nested block may look different now.
    fn drop_current(&mut self) {
        self.map.retain(|_, k| !k.current);
        self.recount();
    }

    /// Forget the libraries' previews: a library read again has new block definitions, and one may
    /// sit where a freed one did, under the same key.
    fn drop_libraries(&mut self) {
        self.map.retain(|_, k| k.current);
        self.recount();
    }

    /// Add up the points the kept previews hold.
    fn recount(&mut self) {
        self.points = self.map.values().map(|k| k.points).fold(0, usize::saturating_add);
    }
}

/// The previews, shared across frames.
type SharedPictures = Arc<Mutex<Pictures>>;

/// The current drawing's blocks. The list is rebuilt when the drawing changes; the pictures are
/// drawn later, for the tiles on screen, and stay while the definitions stay as they were.
fn current_items(app: &mut CadApp, ui: &egui::Ui, pictures: &SharedPictures) -> Listing {
    let Ok(key) = app.session.state().map(|st| (st.uid, st.revision)) else { return Listing::default() };
    let id = egui::Id::unique("blocks-current");
    if let Some((k, listing)) = ui.data(|d| d.get_temp::<((u64, u64), Listing)>(id))
        && k == key
    {
        return listing;
    }
    let listed = app.run("blockspalette", json!({})).unwrap_or(Value::Null);
    let Ok(st) = app.session.state() else { return Listing::default() };
    let defs: Arc<HashMap<String, usize>> = Arc::new(st.doc.blocks.iter().map(|(k, b)| (k.clone(), Arc::as_ptr(b) as usize)).collect());
    let defs_id = egui::Id::unique("blocks-definitions");
    if let Some((uid, old)) = ui.data(|d| d.get_temp::<(u64, Arc<HashMap<String, usize>>)>(defs_id))
        && uid == key.0
        && old.iter().any(|(name, at)| defs.get(name) != Some(at))
    {
        pictures.lock().unwrap_or_else(PoisonError::into_inner).drop_current();
    }
    let items: Vec<Item> = listed
        .get("blocks")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|b| {
                    let name = b.get("name")?.as_str()?.to_string();
                    Some(Item {
                        id: name.clone(),
                        description: b.get("description").and_then(Value::as_str).unwrap_or_default().to_string(),
                        picture: st.doc.block(&name).map(|block| Lazy { doc: st.doc.clone(), block: block.clone() }),
                        label: name,
                        source: Source::Current,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let truncated = listed.get("truncated").and_then(Value::as_bool).unwrap_or(false);
    let listing = Listing { items: Arc::new(items), truncated };
    ui.data_mut(|d| {
        d.insert_temp(defs_id, (key.0, defs));
        d.insert_temp(id, (key, listing.clone()));
    });
    listing
}

/// The contents of a library, or why it cannot be read.
type Loaded = Result<Listing, String>;

/// Where a library's contents arrive: empty until they are read.
type Slot = Arc<Mutex<Option<Loaded>>>;

/// The library the palette shows.
#[derive(Clone)]
struct Library {
    path: String,
    refresh: u32,
    slot: Slot,
}

/// What a library holds, read once and kept until Refresh, or `None` while it is being read.
fn library_items(ui: &egui::Ui, path: &str, refresh: u32, pictures: &SharedPictures) -> Option<Loaded> {
    let id = egui::Id::unique("blocks-library");
    let known = ui.data(|d| d.get_temp::<Library>(id)).filter(|l| l.path == path && l.refresh == refresh);
    let library = known.unwrap_or_else(|| {
        pictures.lock().unwrap_or_else(PoisonError::into_inner).drop_libraries();
        let library = Library { path: path.to_string(), refresh, slot: read_in_background(ui.ctx(), path) };
        ui.data_mut(|d| d.insert_temp(id, library.clone()));
        library
    });
    library.slot.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Read a library on a worker thread: a big drawing or a slow network folder must not stop the
/// interface. The web has no threads and reads it now, as OPEN does.
fn read_in_background(ctx: &egui::Context, path: &str) -> Slot {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let slot = Slot::default();
        let (out, ctx, owned) = (slot.clone(), ctx.clone(), path.to_string());
        let worker = std::thread::Builder::new().name("block-library".into()).spawn(move || {
            let loaded = catch_unwind(|| read_library(&owned)).unwrap_or_else(|_| Err(format!("{owned}: the file could not be read")));
            *out.lock().unwrap_or_else(PoisonError::into_inner) = Some(loaded);
            ctx.request_repaint();
        });
        // No thread to read on: read it here.
        if worker.is_err() {
            *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(read_library(path));
        }
        slot
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ctx;
        Arc::new(Mutex::new(Some(read_library(path))))
    }
}

/// What a library holds: the blocks of a drawing file, or the drawings of a folder.
fn read_library(path: &str) -> Loaded {
    let listed = cadcraft_engine::cmd::list_library(path).map_err(|e| e.to_string())?;
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let truncated = listed.get("truncated").and_then(Value::as_bool).unwrap_or(false);
    if listed.get("kind").and_then(Value::as_str) == Some("folder") {
        let files = listed.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
        let items = files
            .iter()
            .map(|f| Item {
                id: text(f, "file"),
                label: text(f, "name"),
                description: text(f, "file"),
                picture: None,
                source: Source::LibraryDrawing { path: text(f, "file") },
            })
            .collect();
        return Ok(Listing { items: Arc::new(items), truncated });
    }
    // A drawing file: its blocks, drawn from the file itself.
    let drawing = cadcraft_engine::cmd::read_library_file(path).ok();
    let blocks = listed.get("blocks").and_then(Value::as_array).cloned().unwrap_or_default();
    let items = blocks
        .iter()
        .map(|b| {
            let name = text(b, "name");
            let picture = drawing.as_ref().and_then(|d| d.block(&name).map(|block| Lazy { doc: d.clone(), block: block.clone() }));
            Item {
                id: format!("{path}|{name}"),
                description: text(b, "description"),
                picture,
                label: name,
                source: Source::LibraryBlock { path: path.to_string() },
            }
        })
        .collect();
    Ok(Listing { items: Arc::new(items), truncated })
}

/// A block's picture: polylines and filled triangles in a unit box (aspect kept, centred, Y up
/// as in the drawing), each with its colour (`None`: the interface's text colour, which stands
/// for white and black, colour 7).
#[derive(Clone, Debug, Default)]
struct Shape {
    lines: Vec<(Option<Color32>, Vec<Pos2>)>,
    /// Three points per triangle.
    tris: Vec<(Option<Color32>, Vec<Pos2>)>,
}

impl Shape {
    /// How many points the preview holds.
    fn points(&self) -> usize {
        self.lines.iter().chain(&self.tris).map(|(_, l)| l.len()).fold(0, usize::saturating_add)
    }
}

/// Cancels a build when `deadline` passes, unless the returned channel is dropped first.
#[cfg(not(target_arch = "wasm32"))]
fn watchdog(cancel: Arc<AtomicBool>, deadline: Duration) -> Option<(std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>)> {
    use std::sync::mpsc::{RecvTimeoutError, channel};
    let (done, wait) = channel::<()>();
    let timer = std::thread::Builder::new().name("block-preview-deadline".into()).spawn(move || {
        if matches!(wait.recv_timeout(deadline), Err(RecvTimeoutError::Timeout)) {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    timer.ok().map(|timer| (done, timer))
}

/// Build the display list of `ents`, or `None` when it panics, is cancelled or takes too long
/// (there is no timer on the web, where the block's size and the renderer's own limits apply).
fn build_within(d: &Drawing, ents: &[&Entity], mut opts: Options, cancel: &Arc<AtomicBool>, deadline: Option<Duration>) -> Option<DisplayList> {
    opts.cancel = Some(cancel.clone());
    #[cfg(not(target_arch = "wasm32"))]
    let timer = deadline.and_then(|t| watchdog(cancel.clone(), t));
    #[cfg(target_arch = "wasm32")]
    let _ = deadline;
    let built = catch_unwind(AssertUnwindSafe(|| build_entities(d, ents.iter().copied(), &opts)));
    #[cfg(not(target_arch = "wasm32"))]
    if let Some((done, timer)) = timer {
        drop(done);
        let _ = timer.join();
    }
    if cancel.load(Ordering::Relaxed) { None } else { built.ok() }
}

/// The picture of a block, drawn now.
fn picture(lazy: &Lazy) -> Picture {
    let cancel = Arc::new(AtomicBool::new(false));
    match outline(&lazy.doc, &lazy.block, &cancel, Some(PREVIEW_DEADLINE)) {
        Some(shape) => Picture::Ready(Arc::new(shape)),
        None => Picture::Unavailable,
    }
}

/// `text`, cut to `max` characters.
fn clipped(text: &str, max: usize) -> std::borrow::Cow<'_, str> {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", text.get(..end).unwrap_or_default()).into(),
        None => text.into(),
    }
}

/// Take `n` from the work left, one at least (looking at an empty object is work too); `None`
/// when there is not that much.
fn spend(left: &mut usize, n: usize) -> Option<()> {
    *left = left.checked_sub(n.max(1))?;
    Some(())
}

/// The sum of what `sizes` yields.
fn total(sizes: impl Iterator<Item = usize>) -> usize {
    sizes.fold(0, usize::saturating_add)
}

/// What drawing `text` costs.
fn text_work(text: &str) -> usize {
    text.len().saturating_mul(TEXT_WORK)
}

/// The vertices of filled loops.
fn loop_vertices(loops: &[HatchLoop]) -> usize {
    total(loops.iter().map(|l| l.vertices.len()))
}

/// What the block `name` costs, drawn `times` times: what it costs once, multiplied.
fn block_work(d: &Drawing, name: &str, times: usize, depth: usize, left: &mut usize) -> Option<()> {
    let Some(inner) = d.block(name) else { return Some(()) };
    if depth >= MAX_BLOCK_DEPTH {
        return None;
    }
    let mut once = *left;
    for e in inner.entities.iter() {
        work_of(d, e, depth + 1, &mut once)?;
    }
    spend(left, left.saturating_sub(once).saturating_mul(times))
}

/// What a block reference costs: its attributes, and its block once for each cell of its array.
fn reference_work(d: &Drawing, i: &Insert, depth: usize, left: &mut usize) -> Option<()> {
    spend(left, total(i.attribs.iter().map(|a| text_work(&a.text.value))))?;
    let cells = (i.rows.max(1) as usize).saturating_mul(i.cols.max(1) as usize);
    block_work(d, &i.block, cells, depth, left)
}

/// What drawing `e` costs, counted before it is drawn: its vertices, control points and
/// characters, and the blocks it draws. `None` when the cost is not worth knowing (it is too big).
fn work_of(d: &Drawing, e: &Entity, depth: usize, left: &mut usize) -> Option<()> {
    match &e.kind {
        EntityKind::LwPolyline(p) => spend(left, p.vertices.len()),
        EntityKind::Polyline3d(p) => spend(left, p.points.len()),
        EntityKind::Spline(sp) => spend(left, sp.control.len().saturating_add(sp.fit.len())),
        EntityKind::MLine(m) => spend(left, m.vertices.len().saturating_mul(m.def.elements.len().max(1))),
        EntityKind::Leader(l) => spend(left, l.vertices.len()),
        EntityKind::Wipeout(w) => spend(left, w.boundary.len()),
        EntityKind::Region(r) => spend(left, loop_vertices(&r.loops)),
        EntityKind::Hatch(h) => spend(left, loop_vertices(&h.loops).saturating_add(h.pattern_lines.len())),
        EntityKind::Text(t) => spend(left, text_work(&t.value)),
        EntityKind::MText(t) => spend(left, text_work(&t.contents)),
        EntityKind::AttDef(a) => spend(left, text_work(&a.tag)),
        EntityKind::Table(t) => spend(left, total(t.cells.iter().flatten().map(|c| text_work(&c.text).saturating_add(1)))),
        EntityKind::Insert(i) => reference_work(d, i, depth, left),
        EntityKind::Dimension(dm) => {
            spend(left, 1)?;
            dm.block.as_ref().map_or(Some(()), |name| block_work(d, name, 1, depth, left))
        }
        EntityKind::MLeader(m) => {
            spend(left, total(m.leaders.iter().map(Vec::len)).saturating_add(m.text.as_ref().map_or(0, |t| text_work(&t.contents))))?;
            m.block.as_ref().map_or(Some(()), |i| reference_work(d, i, depth, left))
        }
        _ => spend(left, 1),
    }
}

/// Whether the objects can be drawn within the preview's work limit, counted before any drawing.
fn affordable(d: &Drawing, ents: &[&Entity]) -> bool {
    let mut left = PREVIEW_WORK;
    ents.iter().all(|e| work_of(d, e, 0, &mut left).is_some())
}

/// The block as the canvas draws it (nested blocks, splines, text and fills included), each
/// object in its colour. A huge block's picture holds only a first part. `None`: the block is too
/// big, or drawing it was cancelled, took too long or failed.
fn outline(d: &Drawing, b: &Block, cancel: &Arc<AtomicBool>, deadline: Option<Duration>) -> Option<Shape> {
    let ents: Vec<&Entity> = b.entities.iter().take(PREVIEW_ENTITIES.saturating_add(1)).map(|e| &**e).collect();
    if ents.len() > PREVIEW_ENTITIES {
        return None;
    }
    if ents.is_empty() {
        return Some(Shape::default());
    }
    if !affordable(d, &ents) {
        return None;
    }
    // A coarse pass for the size, then one detailed to match it. The view height is given, so a
    // point is sized by the block and the drawing is not scanned for its extents.
    let coarse = Options { tolerance: 1.0, min_dash: NO_DASHES, lineweights: false, view_height: 1.0, ..Options::default() };
    let first = build_within(d, &ents, coarse, cancel, deadline)?;
    let span = first.bounds.width().max(first.bounds.height());
    if !(span.is_finite() && span > 0.0) {
        return Some(Shape::default());
    }
    let opts =
        Options { tolerance: span / PREVIEW_DETAIL, min_dash: span / PREVIEW_DETAIL, lineweights: false, view_height: span, ..Options::default() };
    let list = build_within(d, &ents, opts, cancel, deadline)?;
    let mut lines: Vec<(Option<Color32>, Vec<Vec2>)> = Vec::new();
    let mut tris: Vec<(Option<Color32>, Vec<Vec2>)> = Vec::new();
    let mut budget = PREVIEW_POINTS;
    for p in &list.prims {
        let color = (!p.aci7).then(|| Color32::from_rgb(p.color.0, p.color.1, p.color.2));
        let (start, len) = (p.start as usize, p.len as usize);
        match p.kind {
            Kind::Polyline => {
                let Some(pts) = list.verts.get(start..start.saturating_add(len)) else { continue };
                if pts.len() > budget {
                    break;
                }
                budget -= pts.len();
                lines.push((color, pts.to_vec()));
            }
            Kind::Tris => {
                let Some(pts) = list.tris.get(start..start.saturating_add(len)) else { continue };
                if pts.len() > budget {
                    break;
                }
                budget -= pts.len();
                tris.push((color, pts.to_vec()));
            }
            _ => {}
        }
    }
    let finite = |p: &&Vec2| p.x.is_finite() && p.y.is_finite();
    let bounds = Bounds2::from_points(lines.iter().chain(tris.iter()).flat_map(|(_, l)| l).filter(finite).copied());
    if bounds.is_empty() {
        return Some(Shape::default());
    }
    let span = bounds.width().max(bounds.height()).max(1e-9);
    let c = bounds.center();
    let norm = |p: &Vec2| Pos2::new((0.5 + (p.x - c.x) / span) as f32, (0.5 - (p.y - c.y) / span) as f32);
    // A triangle with a corner that is not a number is left out whole.
    let whole = |t: &&[Vec2; 3]| t.iter().all(|p| p.x.is_finite() && p.y.is_finite());
    Some(Shape {
        lines: lines.iter().map(|(c, l)| (*c, l.iter().filter(finite).map(norm).collect())).collect(),
        tris: tris.iter().map(|(c, l)| (*c, l.as_chunks::<3>().0.iter().filter(whole).flatten().map(norm).collect())).collect(),
    })
}

/// What a frame of the gallery draws its pictures with.
struct Painting<'a> {
    pictures: &'a mut Pictures,
    builds_left: usize,
    /// When the frame stops building previews (the web has no clock to read, so no limit there).
    #[cfg(not(target_arch = "wasm32"))]
    until: std::time::Instant,
}

impl<'a> Painting<'a> {
    /// A frame's painting, with its whole allowance of previews and time.
    fn new(pictures: &'a mut Pictures) -> Self {
        Self {
            pictures,
            builds_left: BUILDS_PER_FRAME,
            #[cfg(not(target_arch = "wasm32"))]
            until: std::time::Instant::now() + PREVIEW_DEADLINE,
        }
    }

    /// Whether the frame has used its time: what is still to build waits for the next frame.
    fn out_of_time(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return std::time::Instant::now() >= self.until;
        #[cfg(target_arch = "wasm32")]
        false
    }
}

/// The tile's picture: remembered, or drawn now if the frame still has room for it. A tile that
/// has to wait asks for another frame.
fn shape_of(ui: &egui::Ui, item: &Item, painting: &mut Painting<'_>) -> Option<Arc<Shape>> {
    let lazy = item.picture.as_ref()?;
    let key = (item.id.clone(), Arc::as_ptr(&lazy.block) as usize);
    let known = match painting.pictures.get(&key) {
        Some(known) => known,
        None if painting.builds_left > 0 && !painting.out_of_time() => {
            painting.builds_left -= 1;
            let built = picture(lazy);
            // A block that took all its time is enough for one frame.
            if matches!(built, Picture::Unavailable) {
                painting.builds_left = 0;
            }
            painting.pictures.insert(key, built.clone(), item.source == Source::Current);
            built
        }
        None => {
            ui.ctx().request_repaint();
            return None;
        }
    };
    match known {
        Picture::Ready(shape) => Some(shape),
        Picture::Unavailable => None,
    }
}

/// Draw the preview in `rect`, or a placeholder when there is none.
fn paint_preview(ui: &egui::Ui, rect: egui::Rect, shape: Option<&Shape>, color: Color32) {
    let side = rect.width().min(rect.height()) * 0.86;
    let origin = rect.center() - vec2(side, side) / 2.0;
    let painter = ui.painter_at(rect);
    let Some(shape) = shape.filter(|s| !s.lines.is_empty() || !s.tris.is_empty()) else {
        // No picture (a drawing of a library folder, or a block without one): a page with a
        // folded corner.
        let w = side * 0.62;
        let page = egui::Rect::from_center_size(rect.center(), vec2(w, side));
        let fold = w * 0.3;
        let pts = vec![
            page.left_top(),
            egui::pos2(page.right() - fold, page.top()),
            egui::pos2(page.right(), page.top() + fold),
            page.right_bottom(),
            page.left_bottom(),
            page.left_top(),
        ];
        painter.add(egui::Shape::line(pts, Stroke::new(1.0, color)));
        return;
    };
    for (c, line) in &shape.lines {
        let pts: Vec<Pos2> = line.iter().map(|p| origin + vec2(p.x * side, p.y * side)).collect();
        painter.add(egui::Shape::line(pts, Stroke::new(1.0, c.unwrap_or(color))));
    }
    for (c, tris) in &shape.tris {
        let mut mesh = egui::Mesh::default();
        for p in tris {
            mesh.colored_vertex(origin + vec2(p.x * side, p.y * side), c.unwrap_or(color));
        }
        let n = u32::try_from(tris.len()).unwrap_or(0) / 3 * 3;
        mesh.indices.extend(0..n);
        painter.add(egui::Shape::mesh(mesh));
    }
}

/// What a click in the palette asks for, run once the palette is drawn.
enum Act {
    Insert { item: Item, explode: bool },
    AddLibrary(String),
    PickLibrary,
    RemoveLibrary(String),
}

/// The palette body.
pub fn section(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get();
    let state_id = egui::Id::unique("blocks-state");
    let mut st: State = ui.data(|d| d.get_temp(state_id)).unwrap_or_default();
    let mut act = None;
    if st.libraries.is_none() {
        st.libraries = Some(libraries(app, json!({})));
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        for (tab, label) in [(Tab::Current, crate::tl!("Current Drawing")), (Tab::Libraries, crate::tl!("Libraries"))] {
            if ui.selectable_label(st.tab == tab, label).clicked() {
                st.tab = tab;
            }
        }
    });
    if st.tab == Tab::Libraries {
        libraries_bar(ui, &mut st, &mut act);
    }
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let w = (ui.available_width() - 150.0).max(80.0);
        ui.add(egui::TextEdit::singleline(&mut st.filter).hint_text(crate::tl!("Filter")).desired_width(w));
        let view_label = ui.label(crate::tl!("View"));
        craft_ui::ComboBox::from_id_salt("blocks-view")
            .selected_text(st.view.label())
            .show_ui(ui, |ui| {
                for v in View::ALL {
                    ui.selectable_value(&mut st.view, v, v.label());
                }
            })
            .response
            .labelled_by(view_label.id);
    });
    ui.add_space(4.0);

    let pictures: SharedPictures = ui.data_mut(|d| d.get_temp_mut_or_default::<SharedPictures>(egui::Id::unique("blocks-pictures")).clone());
    let mut reading = false;
    let loaded: Loaded = match (st.tab, st.library.clone()) {
        (Tab::Current, _) => Ok(current_items(app, ui, &pictures)),
        (Tab::Libraries, Some(path)) => match library_items(ui, &path, st.refresh, &pictures) {
            Some(loaded) => loaded,
            None => {
                reading = true;
                Ok(Listing::default())
            }
        },
        (Tab::Libraries, None) => Ok(Listing::default()),
    };
    st.reading = reading;
    let wanted = st.filter.trim().to_lowercase();
    let items: Vec<&Item> =
        loaded.as_ref().map(|l| l.items.iter().filter(|i| wanted.is_empty() || named(&i.label, &wanted)).collect()).unwrap_or_default();
    if let Ok(listing) = &loaded
        && listing.truncated
    {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new(crate::tf!("Only the first {n} are shown.", n = listing.items.len())).color(t.warn));
        });
    }

    let options_h = 150.0;
    // The panel scrolls when it is shorter than the gallery and the options together.
    let room = ui.available_height();
    let gallery_h = if room.is_finite() { (room - options_h).clamp(150.0, 600.0) } else { 260.0 };
    egui::ScrollArea::vertical().id_salt("blocks-gallery").max_height(gallery_h).auto_shrink([false, false]).show(ui, |ui| {
        if items.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.add_space(8.0);
                let msg = match (&loaded, st.tab, st.library.is_some(), st.filter.trim().is_empty()) {
                    _ if reading => crate::tl!("Reading the library...").to_string(),
                    (Err(e), ..) => e.clone(),
                    (_, Tab::Libraries, false, _) => crate::tl!("Add a folder of drawings or a drawing file to use its blocks.").to_string(),
                    (_, Tab::Libraries, true, true) => crate::tl!("This library has no drawings or blocks.").to_string(),
                    (_, Tab::Current, _, true) => crate::tl!("This drawing has no blocks.").to_string(),
                    _ => crate::tl!("No block matches the filter.").to_string(),
                };
                ui.label(RichText::new(msg).color(t.text_dim));
            });
            return;
        }
        let mut kept = pictures.lock().unwrap_or_else(PoisonError::into_inner);
        kept.tick = kept.tick.wrapping_add(1);
        let mut painting = Painting::new(&mut kept);
        let mut selected = st.selected.clone();
        gallery(ui, st.view, &items, &mut act, &mut selected, &mut painting);
        st.selected = selected;
    });

    ui.add_space(4.0);
    ui.separator();
    let placing = app.session.prompt_text() != "Command:";
    ui.add_enabled_ui(!placing, |ui| options(ui, &mut st));

    repeat_placement(app, ui, &mut st);
    match act {
        Some(Act::Insert { item, explode }) => {
            st.selected = Some(item.id.clone());
            announce_existing(app, &item);
            if insert(app, &item, &st, explode) && st.repeat {
                st.repeating = Repeating::started_now(app, item, explode);
            }
        }
        Some(Act::PickLibrary) => {
            if let Some(path) = app.services.pick_open.as_ref().and_then(|f| f()) {
                add_library(app, &mut st, &path);
            }
        }
        Some(Act::AddLibrary(path)) => add_library(app, &mut st, &path),
        Some(Act::RemoveLibrary(path)) => {
            st.libraries = Some(libraries(app, json!({ "op": "remove", "path": path })));
            if st.library.as_deref() == Some(path.as_str()) {
                st.library = st.libraries.as_ref().and_then(|l| l.first().cloned());
            }
        }
        None => {}
    }
    ui.data_mut(|d| d.insert_temp(state_id, st));
}

/// Place the block again when the INSERT the palette started has finished and placed something;
/// a cancelled one (nothing changed), the Repeat option turned off or another command stops it.
fn repeat_placement(app: &mut CadApp, ui: &egui::Ui, st: &mut State) {
    let pass = ui.ctx().cumulative_pass_nr();
    let watched = pass == st.last_pass.wrapping_add(1);
    st.last_pass = pass;
    let Some(placing) = st.repeating.clone() else { return };
    if !st.repeat || !watched || drawing_uid(app) != Some(placing.uid) {
        st.repeating = None;
        return;
    }
    let prompt = app.session.prompt_text();
    if prompt != "Command:" {
        // Still placing it; any other running command ends the repeating.
        if !prompt.starts_with("INSERT") {
            st.repeating = None;
        }
        return;
    }
    st.repeating = None;
    // Only this INSERT placing something repeats it: it may have been cancelled, and a command
    // started since means the user moved on, even one that ended before this frame (queued control
    // requests do that). A JSON command does not count: it never stops a running command, so it
    // leaves the repeat alone whichever side of this frame it lands.
    let session = &app.session;
    if session.commands_started != placing.started || session.last_finished != Some((placing.started, true)) {
        return;
    }
    if insert(app, &placing.item, st, placing.explode) {
        st.repeating = Repeating::started_now(app, placing.item, placing.explode);
        ui.ctx().request_repaint();
    }
}

/// Which drawing is active: a repeat placement belongs to the one it started in.
fn drawing_uid(app: &CadApp) -> Option<u64> {
    app.session.state().ok().map(|st| st.uid)
}

/// The library list after `params` (`{}` just reads it). An engine error goes to the status line.
fn libraries(app: &mut CadApp, params: Value) -> Vec<String> {
    match app.run("blocklibraries", params) {
        Ok(v) => v
            .get("libraries")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|p| p.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        Err(e) => {
            app.set_status(e);
            Vec::new()
        }
    }
}

/// Add a library to the list and show it.
fn add_library(app: &mut CadApp, st: &mut State, path: &str) {
    match app.run("blocklibraries", json!({ "op": "add", "path": path })) {
        Ok(v) => {
            let list: Vec<String> = v
                .get("libraries")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|p| p.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            st.library = v.get("path").and_then(Value::as_str).map(str::to_string).or_else(|| list.last().cloned());
            st.libraries = Some(list);
            st.new_library.clear();
            st.selected = None;
            st.refresh = st.refresh.wrapping_add(1);
        }
        Err(e) => app.set_status(e),
    }
}

/// The last part of a path: what a library is called in the list.
fn short(path: &str) -> &str {
    path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The library list with what manages it: add (picked or typed), refresh, remove.
fn libraries_bar(ui: &mut egui::Ui, st: &mut State, act: &mut Option<Act>) {
    let t = crate::theme::Tokens::get();
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        if ui.button(crate::tl!("Open Library...")).on_hover_text(crate::tl!("Add a drawing file as a block library")).clicked() {
            *act = Some(Act::PickLibrary);
        }
        if ui
            .add_enabled(st.library.is_some() && !st.reading, egui::Button::new(crate::tl!("Refresh")))
            .on_hover_text(crate::tl!("Read the library again"))
            .clicked()
        {
            // The files are parsed again, even one whose size and time look the same.
            cadcraft_engine::cmd::clear_library_cache();
            st.refresh = st.refresh.wrapping_add(1);
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let label = ui.label(crate::tl!("Folder or file"));
        let w = (ui.available_width() - 70.0).max(60.0);
        let edit = ui.add(egui::TextEdit::singleline(&mut st.new_library).desired_width(w)).labelled_by(label.id);
        let typed = !st.new_library.trim().is_empty();
        let add = ui.add_enabled(typed, egui::Button::new(crate::tl!("Add"))).clicked()
            || (typed && edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        if add {
            *act = Some(Act::AddLibrary(st.new_library.trim().to_string()));
        }
    });
    let libs = st.libraries.clone().unwrap_or_default();
    if libs.is_empty() {
        return;
    }
    egui::ScrollArea::vertical().id_salt("blocks-libraries").max_height(88.0).auto_shrink([false, true]).show(ui, |ui| {
        for path in &libs {
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                let on = st.library.as_deref() == Some(path.as_str());
                let w = (ui.available_width() - 34.0).max(40.0);
                let row = ui.add_sized([w, 20.0], egui::Button::selectable(on, short(path)).frame_when_inactive(false).truncate());
                if row.on_hover_text(path).clicked() && !on {
                    st.library = Some(path.clone());
                    st.selected = None;
                }
                let remove = ui.small_button("×").on_hover_text(crate::tl!("Remove from the list"));
                remove.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{} {}", crate::tl!("Remove"), short(path))));
                if remove.clicked() {
                    *act = Some(Act::RemoveLibrary(path.clone()));
                }
            });
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        if let Some(p) = &st.library {
            ui.add(egui::Label::new(RichText::new(p).color(t.text_dim).small()).truncate());
        }
    });
}

/// The tiles of the icon views; a click or a drag sets `act`.
fn gallery(ui: &mut egui::Ui, view: View, items: &[&Item], act: &mut Option<Act>, selected: &mut Option<String>, painting: &mut Painting<'_>) {
    let t = crate::theme::Tokens::get();
    let on_menu = |ui: &mut egui::Ui, item: &Item, act: &mut Option<Act>| {
        if ui.button(crate::tl!("Insert")).clicked() {
            *act = Some(Act::Insert { item: item.clone(), explode: false });
            ui.close();
        }
        if ui.button(crate::tl!("Insert and Explode")).clicked() {
            *act = Some(Act::Insert { item: item.clone(), explode: true });
            ui.close();
        }
    };
    let react = |resp: &egui::Response, item: &Item, act: &mut Option<Act>, selected: &mut Option<String>| {
        // A click selects the block and starts placing it.
        if resp.clicked() {
            *selected = Some(item.id.clone());
            *act = Some(Act::Insert { item: item.clone(), explode: false });
        }
    };
    match view.tile() {
        Some(side) => {
            ui.horizontal_wrapped(|ui| {
                ui.add_space(8.0);
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for &item in items {
                    let (rect, resp) = ui.allocate_exact_size(vec2(side, side + 32.0), Sense::click());
                    let name = clipped(&item.label, MAX_NAME_CHARS);
                    // Only the tiles on screen are drawn, and only they build a picture.
                    if ui.is_rect_visible(rect) {
                        let on = selected.as_deref() == Some(item.id.as_str());
                        if on || resp.hovered() {
                            ui.painter().rect_filled(rect, 3.0, if on { t.selection } else { t.hover });
                        }
                        let art = egui::Rect::from_min_size(rect.min, vec2(side, side));
                        paint_preview(ui, art, shape_of(ui, item, painting).as_deref(), t.text);
                        // The name under the icon: up to two lines, cut with an ellipsis, centred in the tile.
                        let mut job = egui::text::LayoutJob::simple(name.to_string(), egui::FontId::proportional(11.0), t.text, side - 4.0);
                        job.wrap.max_rows = 2;
                        job.wrap.break_anywhere = true;
                        job.halign = egui::Align::Center;
                        let galley = ui.painter().layout_job(job);
                        let at = egui::pos2(rect.center().x - galley.rect.center().x, rect.min.y + side + 2.0);
                        ui.painter().galley(at, galley, t.text);
                    }
                    let tip = if item.description.is_empty() {
                        name.to_string()
                    } else {
                        format!("{name}\n{}", clipped(&item.description, MAX_DESCRIPTION_CHARS))
                    };
                    let resp = resp.on_hover_text(tip);
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.to_string()));
                    react(&resp, item, act, selected);
                    resp.context_menu(|ui| on_menu(ui, item, act));
                }
            });
        }
        None => {
            let details = view == View::Details;
            for &item in items {
                let h = if details { 30.0 } else { 20.0 };
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
                let name = clipped(&item.label, MAX_NAME_CHARS);
                if ui.is_rect_visible(rect) {
                    let on = selected.as_deref() == Some(item.id.as_str());
                    if on || resp.hovered() {
                        ui.painter().rect_filled(rect, 2.0, if on { t.selection } else { t.hover });
                    }
                    let mut x = rect.min.x + 8.0;
                    if details {
                        let art = egui::Rect::from_min_size(egui::pos2(x, rect.min.y), vec2(h, h));
                        paint_preview(ui, art, shape_of(ui, item, painting).as_deref(), t.text);
                        x += h + 8.0;
                    }
                    let font = egui::FontId::proportional(12.0);
                    ui.painter().text(egui::pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, &name, font.clone(), t.text);
                    if details {
                        let x2 = rect.min.x + (rect.width() * 0.45).max(120.0);
                        let description = clipped(&item.description, MAX_DESCRIPTION_CHARS);
                        ui.painter().text(egui::pos2(x2, rect.center().y), egui::Align2::LEFT_CENTER, &description, font, t.text_dim);
                    }
                }
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.to_string()));
                react(&resp, item, act, selected);
                resp.context_menu(|ui| on_menu(ui, item, act));
            }
        }
    }
}

/// The insertion options: what INSERT asks on the drawing, or the values used instead.
fn options(ui: &mut egui::Ui, st: &mut State) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(crate::tl!("Options")).strong());
    });
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.checkbox(&mut st.scale_on_screen, crate::tl!("Scale"));
        ui.add_enabled_ui(!st.scale_on_screen, |ui| {
            for (axis, v) in ["X", "Y", "Z"].iter().zip(st.scale.iter_mut()) {
                let label = ui.label(*axis);
                ui.add(egui::DragValue::new(v).speed(0.1)).labelled_by(label.id);
            }
        });
    });
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.checkbox(&mut st.rotation_on_screen, crate::tl!("Rotation"));
        ui.add_enabled_ui(!st.rotation_on_screen, |ui| {
            let label = ui.label(crate::tl!("Angle"));
            ui.add(egui::DragValue::new(&mut st.rotation).speed(1.0).suffix("°")).labelled_by(label.id);
        });
    });
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.checkbox(&mut st.explode, crate::tl!("Explode"));
        ui.checkbox(&mut st.repeat, crate::tl!("Repeat Placement"));
    });
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(crate::tl!("Click a block, then pick its insertion point.")).color(crate::theme::Tokens::get().text_dim));
    });
}

/// A usable scale factor: finite and not zero.
fn factor(v: f64) -> f64 {
    if v.is_finite() && v != 0.0 { v } else { 1.0 }
}

/// Say so when a click on a library tile places the drawing's own block of that name: the tile
/// shows the library's version. Only a click calls this, not each placement of a repeat.
fn announce_existing(app: &mut CadApp, item: &Item) {
    let here = app.session.doc().ok().is_some_and(|d| d.block(&item.label).is_some());
    if here && !matches!(item.source, Source::Current) {
        app.set_status(crate::tl!("This block is already in the drawing, so its definition is used.").to_string());
    }
}

/// Start INSERT for the item and answer the prompts the options settle, leaving the insertion
/// point to the drawing. A library block or drawing is defined in this drawing first, unless a
/// block of that name is already here (it is never replaced). `true` when INSERT started.
fn insert(app: &mut CadApp, item: &Item, st: &State, explode: bool) -> bool {
    let here = app.session.doc().ok().is_some_and(|d| d.block(&item.label).is_some());
    let name = match &item.source {
        // The block (or the whole drawing) comes with only the layers, linetypes and styles it needs.
        Source::LibraryBlock { path } | Source::LibraryDrawing { path } if !here => {
            let mut params = json!({ "path": path, "name": item.label });
            if matches!(item.source, Source::LibraryBlock { .. }) {
                params["block"] = json!(item.label);
            }
            match app.run("blocklibimport", params) {
                Ok(v) => v.get("name").and_then(Value::as_str).unwrap_or(&item.label).to_string(),
                Err(e) => {
                    app.set_status(e);
                    return false;
                }
            }
        }
        _ => item.label.clone(),
    };
    if let Err(e) = app.session.start("insert") {
        app.set_status(e.to_string());
        return false;
    }
    let star = explode || st.explode;
    if let Err(e) = app.session.input(Input::Text(if star { format!("*{name}") } else { name })) {
        app.set_status(e.to_string());
        return false;
    }
    // INSERT must have taken the name: the answers below would be read as block names otherwise.
    if !app.session.current_prompt().is_some_and(|p| p.accept.point) {
        let _ = app.session.input(Input::Cancel);
        return false;
    }
    let mut feed = Vec::new();
    if !st.scale_on_screen {
        let [x, y, z] = st.scale.map(factor);
        if x == y && y == z {
            feed.extend([Input::Keyword("Scale".into()), Input::Text(x.to_string())]);
        } else {
            for (k, v) in [("X", x), ("Y", y), ("Z", z)] {
                feed.extend([Input::Keyword(k.into()), Input::Text(v.to_string())]);
            }
        }
    }
    if !st.rotation_on_screen {
        let r = if st.rotation.is_finite() { st.rotation } else { 0.0 };
        // The palette shows absolute degrees: `<<` reads them whatever AUNITS, ANGBASE and ANGDIR say.
        feed.extend([Input::Keyword("Rotate".into()), Input::Text(format!("<<{r}"))]);
    }
    for i in feed {
        if let Err(e) = app.session.input(i) {
            app.set_status(e.to_string());
            return false;
        }
    }
    true
}

/// Whether `label` matches the filter `wanted` (already lowercase), whatever the script.
fn named(label: &str, wanted: &str) -> bool {
    wanted.is_empty() || label.to_lowercase().contains(wanted)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_filter_ignores_case_in_every_script() {
        assert!(super::named("ÉCROU M8", &"écrou".to_lowercase()));
        assert!(super::named("TÜR", "tür"));
        assert!(super::named("Valve", ""));
        assert!(!super::named("Valve", "pump"));
    }

    use cadcraft_engine::Session;
    use serde_json::json;

    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use cadcraft_engine::doc::geom::Vec3;
    use cadcraft_engine::doc::{Block, Drawing, Entity, EntityKind, Handle, Line};
    use egui::Pos2;

    use super::{Item, Lazy, PREVIEW_ENTITIES, PREVIEW_WORK, Picture, Pictures, Shape, Source, State, affordable, clipped, insert, outline, picture};
    use crate::{CadApp, Services};

    /// The block's picture, with no cancelling and no deadline.
    fn shape(d: &Drawing, b: &Block) -> Shape {
        outline(d, b, &Arc::new(AtomicBool::new(false)), None).unwrap()
    }

    fn line(h: u64, a: Vec3, b: Vec3) -> Entity {
        Entity::new(Handle(h), EntityKind::Line(Line { a, b }))
    }

    /// Make block `name` from everything in model space now (the objects are removed).
    fn make_block(s: &mut Session, name: &str) {
        let handles: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
        s.execute("block", &json!({ "name": name, "base": [0, 0], "handles": handles, "keep": "delete" })).unwrap();
    }

    /// A block made only of references to another block, and one with a circle, spline-free
    /// geometry and text, all have a picture, as the canvas would draw them.
    #[test]
    fn nested_blocks_and_text_have_a_preview() {
        let mut s = Session::new();
        s.execute("line", &json!({ "points": [[0, 0], [4, 0]] })).unwrap();
        make_block(&mut s, "INNER");
        s.execute("insert", &json!({ "name": "INNER", "at": [1, 1] })).unwrap();
        s.execute("insert", &json!({ "name": "INNER", "at": [1, 3], "rotation": 90 })).unwrap();
        make_block(&mut s, "OUTER");
        let d = s.doc().unwrap();
        let outer = shape(d, d.block("OUTER").unwrap());
        assert_eq!(outer.lines.len(), 2, "both nested references are drawn");
        let inner = shape(d, d.block("INNER").unwrap());
        assert_eq!(inner.lines.len(), 1);
        // Everything lies in the unit box.
        assert!(outer.lines.iter().flat_map(|(_, l)| l).all(|p| (0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y)));
    }

    /// A line in a colour keeps it; one in the default colour takes the interface's.
    #[test]
    fn previews_keep_the_colours_of_their_objects() {
        let mut s = Session::new();
        s.execute("color", &json!({ "color": "red" })).unwrap();
        s.execute("line", &json!({ "points": [[0, 0], [4, 0]] })).unwrap();
        make_block(&mut s, "RED");
        s.execute("color", &json!({ "color": "ByLayer" })).unwrap();
        s.execute("line", &json!({ "points": [[0, 0], [4, 0]] })).unwrap();
        make_block(&mut s, "PLAIN");
        let d = s.doc().unwrap();
        let red = shape(d, d.block("RED").unwrap());
        assert_eq!(red.lines.first().map(|(c, _)| *c), Some(Some(egui::Color32::from_rgb(255, 0, 0))));
        let plain = shape(d, d.block("PLAIN").unwrap());
        assert_eq!(plain.lines.first().map(|(c, _)| *c), Some(None), "colour 7 follows the theme");
    }

    fn item(name: &str) -> Item {
        Item { id: name.into(), label: name.into(), description: String::new(), picture: None, source: Source::Current }
    }

    /// The rotation shown in degrees is that rotation, in a drawing that measures angles otherwise.
    #[test]
    fn the_rotation_option_is_absolute_degrees() {
        let mut s = Session::new();
        s.execute("line", &json!({ "points": [[0, 0], [4, 0]] })).unwrap();
        make_block(&mut s, "B");
        let header = &mut s.doc_mut().unwrap().header;
        header.set_i64("AUNITS", 2);
        header.set_i64("ANGDIR", 1);
        header.set_f64("ANGBASE", 1.0);
        let mut app = CadApp::new(s, Services::default());
        let st = State { rotation_on_screen: false, rotation: 90.0, ..State::default() };
        assert!(insert(&mut app, &item("B"), &st, false));
        app.session.input(cadcraft_engine::Input::Point(cadcraft_engine::geom::Vec2::new(1.0, 1.0))).unwrap();
        assert_eq!(app.session.prompt_text(), "Command:", "the options answered every prompt");
        let d = app.session.doc().unwrap();
        let rotation = d.model.iter().find_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.rotation) } else { None });
        let rotation = rotation.expect("an insert");
        assert!((rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{rotation}");
    }

    /// A name INSERT refuses ends the placement: the scale and rotation answers are not read as
    /// block names, so block "1" is not placed.
    #[test]
    fn a_refused_name_places_nothing() {
        let mut s = Session::new();
        s.execute("line", &json!({ "points": [[0, 0], [4, 0]] })).unwrap();
        make_block(&mut s, "1");
        let mut app = CadApp::new(s, Services::default());
        let st = State { scale_on_screen: false, ..State::default() };
        assert!(!insert(&mut app, &item("gone"), &st, false));
        assert_eq!(app.session.prompt_text(), "Command:");
        assert_eq!(app.session.doc().unwrap().model.iter().count(), 0);
    }

    /// A cancelled build leaves no partial picture.
    #[test]
    fn a_cancelled_build_has_no_picture() {
        let mut d = Drawing::new_metric();
        let mut b = Block::new("B");
        b.entities.push(line(0x70, Vec3::new(0.0, 0.0, 0.0), Vec3::new(4.0, 0.0, 0.0)));
        d.blocks.insert("B".into(), Arc::new(b));
        let block = d.block("B").unwrap();
        assert!(outline(&d, block, &Arc::new(AtomicBool::new(true)), None).is_none());
    }

    /// A block over the size limit gets no picture, and asks nothing of the renderer.
    #[test]
    fn a_huge_block_has_no_picture() {
        let mut b = Block::new("HUGE");
        for i in 0..=PREVIEW_ENTITIES {
            b.entities.push(line(0x100 + i as u64, Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)));
        }
        let lazy = Lazy { doc: Arc::new(Drawing::new_metric()), block: Arc::new(b) };
        assert!(matches!(picture(&lazy), Picture::Unavailable));
    }

    /// One object with a million vertices, or a reference repeated a million times, is refused
    /// before anything is drawn; ordinary ones are drawn.
    #[test]
    fn work_is_counted_before_drawing() {
        use cadcraft_engine::doc::LwPolyline;
        use cadcraft_engine::geom::{PolyVertex, Vec2};
        let poly = |n: usize| {
            let vertices = (0..n).map(|i| PolyVertex { p: Vec2::new(i as f64, (i % 7) as f64), ..PolyVertex::default() }).collect();
            Entity::new(
                Handle(0x200),
                EntityKind::LwPolyline(LwPolyline { vertices, closed: false, const_width: 0.0, elevation: 0.0, plinegen: false }),
            )
        };
        let d = Drawing::new_metric();
        let mut big = Block::new("BIG");
        big.entities.push(poly(PREVIEW_WORK + 1));
        let cancel = Arc::new(AtomicBool::new(false));
        assert!(outline(&d, &big, &cancel, None).is_none(), "a million vertices are not drawn");
        let mut small = Block::new("SMALL");
        small.entities.push(poly(1_000));
        assert!(outline(&d, &small, &cancel, None).is_some_and(|s| !s.lines.is_empty()));

        // A reference repeated across a huge array costs what one cell costs, times the cells.
        let mut s = Session::new();
        s.execute("line", &json!({ "points": [[0, 0], [4, 0]] })).unwrap();
        make_block(&mut s, "CELL");
        s.execute("insert", &json!({ "name": "CELL", "at": [0, 0] })).unwrap();
        let mut array = (**s.doc().unwrap().model.iter().next().unwrap()).clone();
        if let EntityKind::Insert(i) = &mut array.kind {
            i.rows = 5_000;
            i.cols = 5_000;
        }
        let mut holder = Block::new("GRID");
        holder.entities.push(array);
        assert!(outline(s.doc().unwrap(), &holder, &cancel, None).is_none(), "25 million cells are not drawn");
    }

    /// Lists of every kind, long texts and the blocks a dimension or a multileader draws count.
    #[test]
    fn work_covers_every_list_and_the_blocks_objects_draw() {
        use cadcraft_engine::doc::{HatchLoop, LwPolyline, MLine, MLineJust, MLineStyle, Region, Wipeout};
        use cadcraft_engine::geom::{PolyVertex, Vec2};
        /// Whether an object of this kind is refused before anything is drawn.
        fn too_much(d: &Drawing, kind: EntityKind) -> bool {
            !affordable(d, &[&Entity::new(Handle(0x200), kind)])
        }
        let points: Vec<Vec2> = (0..=PREVIEW_WORK).map(|i| Vec2::new(i as f64, (i % 7) as f64)).collect();
        let vertices: Vec<PolyVertex> = points.iter().map(|p| PolyVertex { p: *p, ..PolyVertex::default() }).collect();
        let d = Drawing::new_metric();
        assert!(too_much(&d, EntityKind::Wipeout(Wipeout { boundary: points.clone() })));
        assert!(too_much(&d, EntityKind::MLine(MLine::new(&MLineStyle::default(), &points, false, MLineJust::default(), 1.0))));
        let region = Region { loops: vec![HatchLoop { vertices: vertices.clone(), outer: true, sources: Vec::new() }] };
        assert!(too_much(&d, EntityKind::Region(region)));
        // An object with nothing in it still takes a visit, for each cell of an array.
        let mut empty = Block::new("EMPTY");
        let nothing = LwPolyline { vertices: Vec::new(), closed: false, const_width: 0.0, elevation: 0.0, plinegen: false };
        empty.entities.push(Entity::new(Handle(0x202), EntityKind::LwPolyline(nothing)));
        let mut grid = Drawing::new_metric();
        grid.blocks.insert("EMPTY".into(), Arc::new(empty));
        let cells = (PREVIEW_WORK as f64).sqrt() as u32 + 1;
        let array = json!({ "block": "EMPTY", "insert": [0, 0, 0], "rows": cells, "cols": cells });
        assert!(too_much(&grid, EntityKind::Insert(serde_json::from_value(array).unwrap())));

        let mut s = Session::new();
        let mut heavy = Block::new("HEAVY");
        let poly = LwPolyline { vertices, closed: false, const_width: 0.0, elevation: 0.0, plinegen: false };
        heavy.entities.push(Entity::new(Handle(0x201), EntityKind::LwPolyline(poly)));
        s.doc_mut().unwrap().blocks.insert("HEAVY".into(), Arc::new(heavy));
        s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [4, 0], "at": [2, 2] })).unwrap();
        s.execute("mleader", &json!({ "points": [[0, 0], [5, 5]], "text": "A" })).unwrap();
        s.execute("mleader", &json!({ "points": [[0, 0], [5, 5]], "block": "HEAVY" })).unwrap();
        let d = s.doc().unwrap();
        let of = |pick: fn(&EntityKind) -> bool| d.model.iter().find(|e| pick(&e.kind)).map(|e| e.kind.clone()).unwrap();
        // The drawn block of the dimension, the block of the multileader and a very long text.
        let mut dim = of(|k| matches!(k, EntityKind::Dimension(_)));
        assert!(!too_much(d, dim.clone()));
        if let EntityKind::Dimension(dm) = &mut dim {
            dm.block = Some("HEAVY".into());
        }
        assert!(too_much(d, dim));
        assert!(too_much(d, of(|k| matches!(k, EntityKind::MLeader(m) if m.block.is_some()))));
        let mut with_text = of(|k| matches!(k, EntityKind::MLeader(m) if m.text.is_some()));
        if let EntityKind::MLeader(m) = &mut with_text
            && let Some(text) = &mut m.text
        {
            text.contents = "x".repeat(PREVIEW_WORK);
            assert!(too_much(d, EntityKind::MText(text.clone())));
        }
        assert!(too_much(d, with_text));
    }

    /// A block of points is sized by itself, not by a drawing a thousand times bigger: each X
    /// stays a small part of the picture.
    #[test]
    fn a_point_preview_does_not_follow_the_drawing_extents() {
        use cadcraft_engine::doc::{Common, Point, Space};
        let mut d = Drawing::new_metric();
        d.header.set_i64("PDMODE", 3);
        let far = Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(1e6, 1e6, 0.0) };
        d.add(&Space::Model, Common::default(), EntityKind::Line(far)).unwrap();
        let mut b = Block::new("DOTS");
        for i in 0..4 {
            b.entities.push(Entity::new(Handle(0x300 + i), EntityKind::Point(Point { p: Vec3::new(i as f64, 0.0, 0.0), angle: 0.0 })));
        }
        let shape = outline(&d, &b, &Arc::new(AtomicBool::new(false)), None).unwrap();
        assert!(!shape.lines.is_empty());
        for (_, line) in &shape.lines {
            let (min, max) = line.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
            assert!(max - min < 0.25, "a point takes {} of the picture", max - min);
        }
    }

    /// Coordinates that are not numbers never reach the picture.
    #[test]
    fn hostile_coordinates_stay_out_of_the_picture() {
        let d = Drawing::new_metric();
        let mut b = Block::new("NAN");
        b.entities.push(line(0x70, Vec3::new(f64::NAN, 0.0, 0.0), Vec3::new(f64::INFINITY, 1.0, 0.0)));
        b.entities.push(line(0x71, Vec3::new(0.0, 0.0, 0.0), Vec3::new(f64::MAX, f64::MIN, 0.0)));
        b.entities.push(line(0x72, Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 0.0)));
        let cancel = Arc::new(AtomicBool::new(false));
        if let Some(s) = outline(&d, &b, &cancel, None) {
            assert!(s.lines.iter().chain(&s.tris).flat_map(|(_, l)| l).all(|p| p.x.is_finite() && p.y.is_finite()));
        }
    }

    /// Blocks that hold each other end, whatever the file claims.
    #[test]
    fn blocks_that_hold_each_other_are_drawn_without_hanging() {
        let mut s = Session::new();
        s.execute("line", &json!({ "points": [[0, 0], [4, 0]] })).unwrap();
        make_block(&mut s, "A");
        s.execute("insert", &json!({ "name": "A", "at": [1, 1] })).unwrap();
        make_block(&mut s, "B");
        // Point A at B, as a crafted file can: A -> B -> A.
        let reference = (**s.doc().unwrap().block("B").unwrap().entities.iter().next().unwrap()).clone();
        let mut back = reference;
        if let EntityKind::Insert(i) = &mut back.kind {
            i.block = "B".into();
        }
        let d = s.doc_mut().unwrap();
        let mut a = (**d.block("A").unwrap()).clone();
        a.entities.push(back);
        d.blocks.insert("A".into(), Arc::new(a));
        let d = s.doc().unwrap();
        // The loop is found while the work is counted: no picture, and nothing drawn.
        assert!(outline(d, d.block("A").unwrap(), &Arc::new(AtomicBool::new(false)), None).is_none());
    }

    /// What no frame shows goes first; what the current frame shows stays, whatever the total.
    #[test]
    fn the_cache_is_bounded_and_keeps_what_is_on_screen() {
        let big = |n: usize| Picture::Ready(Arc::new(Shape { lines: vec![(None, vec![Pos2::ZERO; n])], tris: Vec::new() }));
        let mut p = Pictures { tick: 1, ..Pictures::default() };
        p.insert(("old".into(), 1), big(3_000_000), false);
        p.tick = 2;
        p.insert(("new".into(), 2), big(3_000_000), false);
        assert!(p.get(&("old".into(), 1)).is_none(), "the oldest went");
        assert!(p.get(&("new".into(), 2)).is_some());
        assert_eq!(p.points, 3_000_000);
        // Two on the same frame: both are on screen, so neither goes.
        p.insert(("also".into(), 3), big(3_000_000), false);
        assert_eq!(p.points, 6_000_000);
        assert!(p.get(&("new".into(), 2)).is_some() && p.get(&("also".into(), 3)).is_some());
        // The current drawing's previews can go together.
        p.insert(("cur".into(), 4), Picture::Unavailable, true);
        p.drop_current();
        assert!(p.get(&("cur".into(), 4)).is_none());
        assert_eq!(p.points, 6_000_000);
        // A library read again takes its previews with it, and leaves the current drawing's.
        p.insert(("cur".into(), 4), Picture::Unavailable, true);
        p.drop_libraries();
        assert!(p.get(&("new".into(), 2)).is_none() && p.get(&("also".into(), 3)).is_none());
        assert!(p.get(&("cur".into(), 4)).is_some());
        assert_eq!(p.points, 0);
    }

    /// However many full-size previews come in, the cache stays within its bounds.
    #[test]
    fn a_thousand_full_size_previews_stay_within_the_bounds() {
        let full = |n: usize| Picture::Ready(Arc::new(Shape { lines: vec![(None, vec![Pos2::ZERO; n])], tris: Vec::new() }));
        let mut p = Pictures::default();
        for i in 0..1000 {
            p.tick = i as u64 + 1;
            p.insert((format!("Part{i}"), i), full(super::PREVIEW_POINTS), i % 2 == 0);
            assert!(p.points <= super::MAX_CACHED_POINTS.max(super::PREVIEW_POINTS), "{}", p.points);
            assert!(p.map.len() <= super::MAX_CACHED_PICTURES);
        }
        // The kept points are the recorded ones, and the newest picture is always there.
        assert_eq!(p.points, p.map.values().map(|k| k.points).sum::<usize>());
        assert!(p.get(&("Part999".to_string(), 999)).is_some());
        // The most the points can weigh: four million of two f32 each, 32 MiB (the limit here is 64).
        assert!(super::MAX_CACHED_POINTS * std::mem::size_of::<Pos2>() <= 64 << 20);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_frame_stops_building_previews_when_its_time_is_used() {
        let mut p = Pictures::default();
        let mut painting = super::Painting::new(&mut p);
        assert!(!painting.out_of_time());
        painting.until = std::time::Instant::now();
        assert!(painting.out_of_time());
    }

    #[test]
    fn long_text_is_cut_to_whole_characters() {
        assert_eq!(clipped("court", 10), "court");
        assert_eq!(clipped("éééééé", 3), "ééé…");
    }

    #[test]
    fn an_empty_block_has_no_preview() {
        let mut s = Session::new();
        s.execute("line", &json!({ "points": [[0, 0], [4, 0]] })).unwrap();
        make_block(&mut s, "ONE");
        let d = s.doc().unwrap();
        let mut empty = (**d.block("ONE").unwrap()).clone();
        empty.entities = Default::default();
        assert!(shape(d, &empty).lines.is_empty());
    }
}
