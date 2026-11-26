//! Which block reference draws attribute text under a point: what a double-click on a block's
//! attribute text edits (#2626).
//!
//! Attribute text is drawn as part of its block reference, not as an object of its own, so the
//! ordinary pick (a reference's geometry, the box selection measures it by) does not find text the
//! renderer draws outside that box. This module finds it without changing the shared spatial
//! index or any ordinary selection:
//!
//! - [`AttribTargets`] is a double-click-only list of the references that have an attribute to
//!   edit, each with the area its attribute text can reach ([`REACH_HEIGHTS_PER_BYTE`]). The
//!   reach comes from the reference's own data alone, never from text styles, fonts, layers,
//!   ATTMODE or block definitions, so a store chunk's identity is a complete key: an edit to a
//!   reference copies its chunk and that chunk's entries are rebuilt. Nothing a layout reads is
//!   kept between clicks.
//! - [`attribute_reference_at`] tests the references whose reach holds the click, topmost first and
//!   above the ordinary pick's object, with the renderer's own layout and visibility rules.
//! - One [`AttribPick`] budget covers the check for an editable ordinary pick and the text search.
//!   A confirmed editable pick stands when the text search runs out of work. If the budget runs
//!   out before that check finds an editable attribute, the editor stays closed.

use std::collections::HashMap;
use std::sync::Arc;

use cadcraft_doc::{AnnoDraw, Attrib, Block, Drawing, Entity, EntityKind, EntityStore, HAlign, Handle, Insert, MAX_CLIP_VERTICES};
use cadcraft_geom::{Bounds2, Vec2};

use crate::clip;
use crate::text_entity::{layout_mtext_contents, layout_mtext_entity, place_text_entity};

/// The most work units one double-click may spend checking the ordinary pick for an editable
/// attribute, building the candidate list for store chunks that changed, and testing candidates.
/// One unit is about one object or attribute read. A text layout costs [`LAYOUT_UNITS`] plus
/// [`LAYOUT_UNITS_PER_BYTE`] per byte it reads or copies.
pub const MAX_ATTRIB_PICK_WORK: u64 = 1 << 22;

/// The work of one text layout before its bytes: the style and font lookups, parsing, shaping.
const LAYOUT_UNITS: u64 = 1024;

/// The work per byte a layout reads or copies: its text, a multiline value's stored contents and
/// column heights, the style's names and the literal an edited value becomes.
const LAYOUT_UNITS_PER_BYTE: u64 = 16;

/// Bytes of a name compared, hashed or searched that one work unit pays for.
const BYTES_PER_UNIT: u64 = 64;

/// The most objects one store chunk holds (the store splits a chunk past twice its size, 1024):
/// what a search inside a chunk is charged for.
const CHUNK_BOUND: u64 = 2048;

/// How far a single-line attribute's text can reach from its anchor points, in text heights per
/// byte of its value (times its width factor when that is wider than 1), plus as many heights
/// all round for the line's height, depth and justification. The renderer's box for such text is
/// its advance width by its height and depth (`cadcraft_fonts::place`), so the reach holds it as
/// long as no character advances more than this many heights: the built-in stroke font advances
/// about one, and TrueType and SHX fonts in use stay well under it. Text drawn wider than that is
/// found only inside the reach; the reference's geometry still opens the editor.
pub const REACH_HEIGHTS_PER_BYTE: f64 = 4.0;

/// What one [`AttribPick`] spent: deterministic counts for tests and diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttribPickStats {
    /// Work units spent, at most the budget.
    pub work: u64,
    /// Objects read while building candidate-list entries for store chunks that changed.
    pub entities_visited: u64,
    /// Store chunks whose entries were built on this click (the others were kept).
    pub chunks_built: u64,
    /// Attributes read: building entries, checking the picked object and testing candidates,
    /// constant and invisible ones included.
    pub attributes_visited: u64,
    /// Candidate-list entries looked at.
    pub candidates_visited: u64,
    /// Block definition entities read to find attribute definitions.
    pub definition_entities: u64,
    /// Attribute texts laid out.
    pub text_layouts: u64,
    /// Clip boundaries tested, only for a text box under the click.
    pub clip_regions: u64,
    /// Vertices of those boundaries.
    pub clip_vertices: u64,
    /// Bytes the text layouts read or copied (paid for before each layout).
    pub payload_bytes: u64,
    /// The budget ran out. A confirmed editable ordinary pick can still open the editor.
    pub exhausted: bool,
}

/// One double-click's work budget. Every step that reads, copies, allocates or lays out text takes
/// its units first; a step that does not fit sets [`AttribPickStats::exhausted`], and from then on
/// nothing more is taken.
#[derive(Clone, Debug)]
pub struct AttribPick {
    remaining: u64,
    stats: AttribPickStats,
    /// The drawing's annotation scale, once read.
    anno_factor: Option<f64>,
}

impl Default for AttribPick {
    fn default() -> Self {
        Self::with_budget(MAX_ATTRIB_PICK_WORK)
    }
}

impl AttribPick {
    /// A budget of [`MAX_ATTRIB_PICK_WORK`] units.
    pub fn new() -> Self {
        Self::default()
    }

    /// A budget of `units` (tests use small ones to reach every way the budget runs out).
    pub fn with_budget(units: u64) -> Self {
        AttribPick { remaining: units, stats: AttribPickStats::default(), anno_factor: None }
    }

    /// The work spent so far.
    pub fn stats(&self) -> AttribPickStats {
        self.stats
    }

    /// The budget ran out.
    pub fn exhausted(&self) -> bool {
        self.stats.exhausted
    }

    /// Take `units`, or nothing (and run out) when they do not fit.
    fn charge(&mut self, units: u64) -> Option<()> {
        if self.stats.exhausted || units > self.remaining {
            self.stats.exhausted = true;
            return None;
        }
        self.remaining -= units;
        self.stats.work = self.stats.work.saturating_add(units);
        Some(())
    }

    /// The text layout of `bytes` bytes of payload.
    fn charge_layout(&mut self, bytes: u64) -> Option<()> {
        self.charge(LAYOUT_UNITS.saturating_add(bytes.saturating_mul(LAYOUT_UNITS_PER_BYTE)))?;
        self.stats.text_layouts += 1;
        self.stats.payload_bytes = self.stats.payload_bytes.saturating_add(bytes);
        Some(())
    }

    /// Whether the object `h` of `store` is a block reference with an attribute that can be
    /// changed: what the ordinary pick's object must be to open the editor. False, too, when the
    /// budget runs out before an editable attribute is found.
    fn editable(&mut self, store: &EntityStore, h: Handle) -> bool {
        if self.charge(store_search_units(store)).is_none() {
            return false;
        }
        let Some(e) = store.get(h) else { return false };
        let EntityKind::Insert(ins) = &e.kind else { return false };
        for a in &ins.attribs {
            if self.charge(1).is_none() {
                return false;
            }
            self.stats.attributes_visited += 1;
            if !a.constant {
                return true;
            }
        }
        false
    }

    /// The smallest and largest scale `e` can be drawn at, or `None` when the budget runs out: 1
    /// (as stored, and on a layout's sheet) and each annotation scale it lists, a little wider for
    /// the tolerance a view's scale is matched with. Not annotative: 1 and 1.
    fn draw_scales(&mut self, e: &Entity) -> Option<(f64, f64)> {
        if !cadcraft_doc::is_annotative(e) {
            return Some((1.0, 1.0));
        }
        self.charge(1u64.saturating_add(len_u64(e.common.anno_scales.len())))?;
        let (mut lo, mut hi) = (1.0f64, 1.0f64);
        for s in &e.common.anno_scales {
            let f = s.factor();
            if f.is_finite() && f > 0.0 {
                lo = lo.min(f);
                hi = hi.max(f);
            }
        }
        Some((lo * (1.0 - 1e-8), hi * (1.0 + 1e-8)))
    }

    /// How the view draws top-level object `e` (`entity_parts` in the renderer): as stored, scaled
    /// for its annotation scale, or not at all.
    fn anno_mode(&mut self, d: &Drawing, e: &Entity, view: AnnoView) -> Option<AnnoDraw> {
        if !cadcraft_doc::is_annotative(e) {
            return Some(AnnoDraw::Plain);
        }
        self.charge(1u64.saturating_add(len_u64(e.common.anno_scales.len())))?;
        let scale = match view {
            AnnoView::Sheet => return Some(AnnoDraw::Scaled(1.0)),
            AnnoView::Viewport(v) => v,
            AnnoView::Model => self.current_scale(d)?,
        };
        Some(cadcraft_doc::anno_draw(e, scale, d.anno_all_visible()))
    }

    /// The drawing's annotation scale factor (CANNOSCALE, [`Drawing::current_anno_scale`]), read
    /// once for the click: the header's name is copied and searched for in the scale list, and the
    /// scale found is copied, all paid for first.
    fn current_scale(&mut self, d: &Drawing) -> Option<f64> {
        if let Some(f) = self.anno_factor {
            return Some(f);
        }
        let name = d.header.get("CANNOSCALE").and_then(cadcraft_doc::HVal::as_str).map_or(0, str::len);
        self.charge(name_units(name).saturating_mul(3).saturating_add(table_units(d.scales.len(), name)))?;
        let f = d.current_anno_scale().factor();
        self.anno_factor = Some(f);
        Some(f)
    }

    /// Whether objects on `layer` are drawn: the layer on and thawed, and not one the viewport
    /// freezes, as `resolve_common` decides (names compared without case).
    fn layer_shown(&mut self, d: &Drawing, layer: &str, frozen: &[String]) -> Option<bool> {
        self.charge(table_units(d.layers.len(), layer.len()).saturating_add(table_units(frozen.len(), layer.len())))?;
        Some(d.layer(layer).is_none_or(|l| l.visible()) && !frozen.iter().any(|f| f.eq_ignore_ascii_case(layer)))
    }

    /// Whether the renderer draws block reference `e` itself (`entity`, `entity_parts`): not
    /// hidden by HIDEOBJECTS, its own visibility, its layer and the viewport's frozen layers.
    fn reference_shown(&mut self, d: &Drawing, e: &Entity, frozen: &[String]) -> Option<bool> {
        self.charge(1)?;
        if d.is_hidden_object(e.handle) || !e.common.visible {
            return Some(false);
        }
        self.layer_shown(d, &e.common.layer, frozen)
    }

    /// Whether the renderer draws attribute `a` of reference `e` (`attributes` in `blocks.rs`), with
    /// `def` the block's definition for its tag: with no definition and no properties of its own
    /// it takes the reference's, which are drawn; otherwise its own layer, else the definition's
    /// (layer 0 being the reference's), and the definition's visibility. Read in place: nothing is
    /// copied and no linetype is looked up.
    fn attribute_shown(&mut self, d: &Drawing, e: &Entity, a: &Attrib, def: Option<&Entity>, frozen: &[String]) -> Option<bool> {
        if def.is_none() && a.props.is_empty() {
            return Some(true);
        }
        if !def.is_none_or(|d| d.common.visible) {
            return Some(false);
        }
        let layer = a.props.layer.as_deref().unwrap_or_else(|| def.map_or("0", |d| d.common.layer.as_str()));
        let layer = if layer == "0" { e.common.layer.as_str() } else { layer };
        self.layer_shown(d, layer, frozen)
    }

    /// The layout box of attribute `a` of `ins` at scale `f`, empty when the text draws nothing.
    /// `None` means the budget cannot pay for the layout. Pay for the payload before copying it.
    fn text_box(&mut self, d: &Drawing, ins: &Insert, a: &Attrib, f: f64) -> Option<Bounds2> {
        match &a.multiline {
            None => {
                let t = &a.text;
                let style = self.style_payload(d, &t.style)?;
                // Scaled, the text is copied (its value and style name) before it is laid out.
                let copy = if f == 1.0 { 0 } else { len_u64(t.value.len()).saturating_add(len_u64(t.style.len())) };
                self.charge_layout(len_u64(t.value.len()).saturating_add(style).saturating_add(copy))?;
                // The same transform the renderer applies (`scale_annotative_attrib`), on a copy
                // of only the text already paid for above.
                let scaled;
                let t = if f == 1.0 {
                    t
                } else {
                    scaled = scaled_text_copy(a, ins, f);
                    &scaled.text
                };
                let (shaped, bounds) = place_text_entity(d, t, &t.value);
                Some(if has_ink(&shaped) { bounds } else { Bounds2::EMPTY })
            }
            Some(ml) => {
                let m = &ml.mtext;
                // Whether the value was edited since it was read decides what is laid out: the
                // stored contents, or the value as literal contents (at most twice its length).
                self.charge(name_units(a.text.value.len().min(ml.read_as.len())))?;
                let edited = a.text.value != ml.read_as;
                let contents = if edited { len_u64(a.text.value.len()).saturating_mul(2) } else { len_u64(m.contents.len()) };
                let style = self.style_payload(d, &m.style)?;
                let heights = m.columns.as_ref().map_or(0, |c| len_u64(c.heights.len()).saturating_mul(8));
                // Scaled, the multiline text is copied first: contents, style name, column heights.
                // `scaled_text_copy` also copies the single-line text it carries (value, style name).
                let copy = if f == 1.0 {
                    0
                } else {
                    len_u64(m.contents.len())
                        .saturating_add(len_u64(m.style.len()))
                        .saturating_add(heights)
                        .saturating_add(len_u64(a.text.value.len()))
                        .saturating_add(len_u64(a.text.style.len()))
                };
                self.charge_layout(contents.saturating_add(style).saturating_add(heights).saturating_add(copy))?;
                // Column gutter, height and manual heights stay as stored: `scale_annotative_attrib`
                // is the renderer's block-reference transform, and it does not scale them.
                let scaled_attr;
                let m = if f == 1.0 {
                    m
                } else {
                    scaled_attr = scaled_text_copy(a, ins, f);
                    scaled_attr.multiline.as_ref().map(|ml| &ml.mtext).unwrap_or(m)
                };
                let layout =
                    if edited { layout_mtext_contents(d, m, &cadcraft_doc::mtext_literal(&a.text.value)) } else { layout_mtext_entity(d, m) };
                Some(if layout.pieces.iter().any(|p| has_ink(&p.shaped)) { layout.bounds } else { Bounds2::EMPTY })
            }
        }
    }

    /// The bytes of the text style named `name` that a layout copies (`text_style` clones it), after
    /// paying for this lookup and the layout's own.
    fn style_payload(&mut self, d: &Drawing, name: &str) -> Option<u64> {
        self.charge(table_units(d.text_styles.len(), name.len()).saturating_mul(2))?;
        Some(match d.text_style(name) {
            Some(s) => len_u64(s.name.len()).saturating_add(len_u64(s.font.len())).saturating_add(len_u64(s.big_font.len())),
            // The default style it falls back to: two short names.
            None => 64,
        })
    }

    /// Whether the XCLIP boundary of `ins` (drawn at scale `f`) keeps point `p`, as the renderer cuts
    /// a reference's attribute text (`insert`: the boundary is off, degenerate or without a block
    /// to place it by, and nothing is cut). The outline is paid for by its vertices before it is
    /// copied; the test is a point test, not the renderer's fill mesh.
    fn clip_keeps<'a>(&mut self, d: &'a Drawing, ins: &'a Insert, f: f64, p: Vec2, blocks: &mut Blocks<'a>) -> Option<bool> {
        let Some(c) = ins.clip.as_ref() else { return Some(true) };
        if !c.enabled {
            return Some(true);
        }
        let vertices = len_u64(c.points.len().min(MAX_CLIP_VERTICES.saturating_add(1)));
        self.charge(1u64.saturating_add(vertices))?;
        let poly = c.polygon();
        if poly.len() < 3 {
            return Some(true);
        }
        let Some(blk) = self.block(d, &ins.block, blocks)?.block else { return Some(true) };
        // Transformed, cleaned and tested: about one unit a vertex more.
        self.charge(1u64.saturating_add(len_u64(poly.len())))?;
        self.stats.clip_regions += 1;
        self.stats.clip_vertices = self.stats.clip_vertices.saturating_add(len_u64(poly.len()));
        let m = ins.transform(blk.base.xy());
        let o = ins.insert.xy();
        let world: Vec<Vec2> = poly.iter().map(|q| scaled(o, m.apply(*q), f)).collect();
        Some(match clip::polygon_contains(&world, p) {
            None => true,
            Some(inside) => inside != c.inverted,
        })
    }

    /// The block named `name` (and its attribute definitions once read), looked up once per click.
    fn block<'a, 'b>(&mut self, d: &'a Drawing, name: &'a str, blocks: &'b mut Blocks<'a>) -> Option<&'b mut BlockInfo<'a>> {
        // Hashing the name for this click's lookups; the first one also searches the block table.
        self.charge(name_units(name.len()))?;
        Some(match blocks.entry(name) {
            std::collections::hash_map::Entry::Occupied(o) => o.into_mut(),
            std::collections::hash_map::Entry::Vacant(v) => {
                self.charge(table_units(d.blocks.len(), name.len()))?;
                v.insert(BlockInfo { block: d.block(name).map(|b| b.as_ref()), attdefs: None })
            }
        })
    }

    /// The block's definition for attribute tag `tag`: its first attribute definition with that
    /// tag without case, as `attributes` takes it. `Some(None)` when there is none.
    fn definition<'a>(&mut self, d: &'a Drawing, ins: &'a Insert, tag: &str, blocks: &mut Blocks<'a>) -> Option<Option<&'a Entity>> {
        let info = self.block(d, &ins.block, blocks)?;
        let Some(blk) = info.block else { return Some(None) };
        if info.attdefs.is_none() {
            let mut defs = Vec::new();
            for be in blk.entities.iter() {
                self.charge(1)?;
                self.stats.definition_entities += 1;
                if matches!(be.kind, EntityKind::AttDef(_)) {
                    defs.push(be.as_ref());
                }
            }
            info.attdefs = Some(defs);
        }
        for be in info.attdefs.iter().flatten() {
            let EntityKind::AttDef(ad) = &be.kind else { continue };
            self.charge(name_units(tag.len().min(ad.tag.len())))?;
            if ad.tag.eq_ignore_ascii_case(tag) {
                return Some(Some(*be));
            }
        }
        Some(None)
    }

    /// Whether the renderer draws attribute text of block reference `e` within the click's aperture
    /// of its point, with ATTMODE `attmode` (ATTDISP: 0 shows no attributes, 1 the visible ones,
    /// 2 all of them). `None` when the budget runs out first.
    fn drawn_at<'a>(&mut self, d: &'a Drawing, e: &'a Entity, click: &AttribClick<'_>, attmode: i64, blocks: &mut Blocks<'a>) -> Option<bool> {
        let EntityKind::Insert(ins) = &e.kind else { return Some(false) };
        let (p, aperture) = (click.at, click.aperture);
        if !self.reference_shown(d, e, click.frozen)? {
            return Some(false);
        }
        let f = match self.anno_mode(d, e, click.anno)? {
            AnnoDraw::Hidden => return Some(false),
            AnnoDraw::Scaled(f) if f.is_finite() && f > 0.0 => f,
            _ => 1.0,
        };
        if attmode == 0 {
            return Some(false);
        }
        for a in &ins.attribs {
            // Every attribute is paid for before it is skipped, invisible and constant ones too.
            self.charge(1)?;
            self.stats.attributes_visited += 1;
            if a.invisible && attmode != 2 {
                continue;
            }
            // The attribute's own reach at the scale it is drawn at: text that cannot reach the
            // click is not laid out (a reference reaches as far as its farthest attribute).
            match attrib_reach(ins, a, (f, f)) {
                None => continue,
                Some(Reach::Within(r)) if !r.expand(aperture).contains(p) => continue,
                Some(_) => {}
            }
            let def = self.definition(d, ins, &a.tag, blocks)?;
            if !self.attribute_shown(d, e, a, def, click.frozen)? {
                continue;
            }
            let b = self.text_box(d, ins, a, f)?;
            if !b.is_empty() && b.expand(aperture).contains(p) && self.clip_keeps(d, ins, f, p, blocks)? {
                return Some(true);
            }
        }
        Some(false)
    }
}

/// Spaces can contribute to the layout box without drawing a stroke or outline.
fn has_ink(shaped: &cadcraft_fonts::Shaped) -> bool {
    shaped.strokes.iter().chain(shaped.glyphs.iter().flatten()).any(|path| path.len() >= 2)
}

/// A double-click to answer: where it is, how near counts as on text, what the ordinary pick took
/// there, and what the view draws (the layers the active layout viewport freezes, and the
/// annotation scale annotative references are drawn at).
#[derive(Clone, Copy, Debug)]
pub struct AttribClick<'a> {
    /// The point, in the edited space's coordinates.
    pub at: Vec2,
    /// How near text counts as on it: the pick aperture.
    pub aperture: f64,
    /// The ordinary pick at `at`: the topmost object whose geometry or selection box is there.
    pub picked: Option<Handle>,
    /// The layers frozen in the active MSPACE viewport (empty elsewhere).
    pub frozen: &'a [String],
    /// The annotation scale annotative references are drawn at.
    pub anno: AnnoView,
}

/// The annotation scale a view draws annotative objects at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnnoView {
    /// A layout's sheet: every annotative object at 1:1.
    Sheet,
    /// Model space (or a block being edited) on its own: the drawing's annotation scale,
    /// CANNOSCALE.
    Model,
    /// Model space through a layout viewport: the viewport's scale, drawing units per paper unit.
    Viewport(f64),
}

/// Where a reference's attribute text can be.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Reach {
    /// Inside this box (before the pick aperture is added).
    Within(Bounds2),
    /// Anywhere: no bound from the reference's own data (multiline or aligned text).
    Anywhere,
}

/// One candidate: a block reference with an attribute to edit, where it is in its chunk, and the
/// reach of its attribute text.
#[derive(Clone, Debug)]
struct Target {
    local: usize,
    entity: Arc<Entity>,
    reach: Reach,
}

/// The candidates of one store chunk, with the union of their reaches.
#[derive(Clone, Debug, Default)]
struct ChunkTargets {
    reach: Bounds2,
    anywhere: bool,
    refs: Vec<Target>,
}

/// A store chunk's candidates and where the chunk is: its identity key, length and draw offset.
#[derive(Clone, Debug)]
struct TargetChunk {
    key: usize,
    len: usize,
    offset: usize,
    /// Objects of this chunk already folded into `targets`. `len` when the chunk is finished.
    /// The next click continues at this index, so a chunk that costs more than one budget
    /// still finishes. One reference is read whole or retried from its first attribute.
    built: usize,
    targets: ChunkTargets,
}

/// The block references of one space that have an attribute to edit, with where their attribute
/// text can reach: a double-click's candidates, kept between clicks and rebuilt for the store
/// chunks that change. Separate from the selection index: nothing else reads it.
///
/// Each entry depends only on its reference's own data, so the chunk it came from identifies it:
/// the list holds a clone of the store it was built from, which keeps every chunk's identity key
/// valid (a key is a chunk's address, and an edit copies the chunk it touches).
#[derive(Clone, Debug, Default)]
pub struct AttribTargets {
    store: EntityStore,
    chunks: Vec<TargetChunk>,
    complete: bool,
}

impl AttribTargets {
    /// Bring the list up to date with `store`, building entries for the chunks that changed, each
    /// object and attribute paid for. False when the budget ran out first: the list then cannot
    /// answer this click, and keeps what it has built, including a part of a chunk, so the next
    /// click goes on from there. One reference is read whole or retried from its first attribute,
    /// so a single reference that itself costs more than the budget is not finished by later clicks.
    pub fn refresh(&mut self, store: &EntityStore, pick: &mut AttribPick) -> bool {
        // Comparing with the store the list holds looks at each of its chunks; setting them aside
        // by key, below, takes one more look each.
        if pick.charge(1u64.saturating_add(len_u64(self.chunks.len()).saturating_mul(2))).is_none() {
            return false;
        }
        if self.complete && self.store.same_as(store) {
            return true;
        }
        // Paid for before anything changes: two units a chunk of the new store, one to count it
        // and keep its pointer in the clone below, one to find its entries; one for the clone's
        // shard pointers.
        let mut count = 0usize;
        for _ in store.chunk_slices() {
            if pick.charge(2).is_none() {
                return false;
            }
            count += 1;
        }
        if pick.charge(1).is_none() {
            return false;
        }
        let mut old: HashMap<usize, TargetChunk> = self.chunks.drain(..).map(|c| (c.key, c)).collect();
        let mut chunks = Vec::with_capacity(count);
        let mut offset = 0usize;
        let mut complete = true;
        for (key, items) in store.chunk_slices() {
            let kept = old.remove(&key).filter(|c| c.len == items.len());
            // A finished chunk is kept. A chunk started earlier continues from the object it
            // reached. Past the budget, a partial chunk is kept and nothing more is built.
            let progress = match kept {
                Some(c) if c.built >= items.len() || !complete => ChunkProgress { targets: c.targets, built: c.built },
                Some(c) => build_chunk(items, pick, ChunkProgress { targets: c.targets, built: c.built }),
                None if complete => build_chunk(items, pick, ChunkProgress { targets: ChunkTargets::default(), built: 0 }),
                None => {
                    complete = false;
                    offset = offset.saturating_add(items.len());
                    continue;
                }
            };
            if progress.built < items.len() {
                complete = false;
            }
            chunks.push(TargetChunk { key, len: items.len(), offset, built: progress.built, targets: progress.targets });
            offset = offset.saturating_add(items.len());
        }
        self.store = store.clone();
        self.chunks = chunks;
        self.complete = complete;
        complete
    }

    /// The topmost candidate drawn above draw position `above` (anywhere with `None`) whose
    /// attribute text the renderer draws under `click`. `None` when there is none or the budget ran
    /// out first.
    fn find<'a>(&'a self, d: &'a Drawing, above: Option<usize>, click: &AttribClick<'_>, attmode: i64, pick: &mut AttribPick) -> Option<Handle> {
        let (p, aperture) = (click.at, click.aperture);
        let mut blocks: Blocks<'a> = HashMap::new();
        for c in self.chunks.iter().rev() {
            pick.charge(1)?;
            // This chunk and the ones under it are all at or below the ordinary pick's object.
            if above.is_some_and(|s| c.offset.saturating_add(c.len) <= s.saturating_add(1)) {
                return None;
            }
            let t = &c.targets;
            if !t.anywhere && !t.reach.expand(aperture).contains(p) {
                continue;
            }
            for target in t.refs.iter().rev() {
                pick.charge(1)?;
                pick.stats.candidates_visited += 1;
                if above.is_some_and(|s| c.offset.saturating_add(target.local) <= s) {
                    return None;
                }
                if let Reach::Within(b) = target.reach
                    && !b.expand(aperture).contains(p)
                {
                    continue;
                }
                if pick.drawn_at(d, &target.entity, click, attmode, &mut blocks)? {
                    return Some(target.entity.handle);
                }
            }
        }
        None
    }
}

/// The blocks one click has looked up, by the name references give.
type Blocks<'a> = HashMap<&'a str, BlockInfo<'a>>;

/// A block looked up for a click, with its attribute definitions once they are read.
struct BlockInfo<'a> {
    block: Option<&'a Block>,
    attdefs: Option<Vec<&'a Entity>>,
}

/// How far [`build_chunk`] got. `built` is the next object index; the targets hold only objects
/// before it.
struct ChunkProgress {
    targets: ChunkTargets,
    built: usize,
}

/// The candidates of one store chunk, each object and attribute paid for before it is read.
/// Starts from `progress` (empty, or the part an earlier click kept) and stops at the first
/// object the budget cannot finish. That object is retried next time, from its first attribute.
fn build_chunk(items: &[Arc<Entity>], pick: &mut AttribPick, mut progress: ChunkProgress) -> ChunkProgress {
    pick.stats.chunks_built += 1;
    for (local, e) in items.iter().enumerate().skip(progress.built) {
        if pick.charge(1).is_none() {
            return progress;
        }
        pick.stats.entities_visited += 1;
        let EntityKind::Insert(ins) = &e.kind else {
            progress.built = local.saturating_add(1);
            continue;
        };
        let Some(scales) = pick.draw_scales(e) else { return progress };
        let mut editable = false;
        let mut reach: Option<Reach> = None;
        for a in &ins.attribs {
            if pick.charge(1).is_none() {
                return progress;
            }
            pick.stats.attributes_visited += 1;
            editable |= !a.constant;
            reach = match (reach, attrib_reach(ins, a, scales)) {
                (Some(Reach::Anywhere), _) | (_, Some(Reach::Anywhere)) => Some(Reach::Anywhere),
                (Some(Reach::Within(x)), Some(Reach::Within(y))) => Some(Reach::Within(x.union(&y))),
                (r, None) | (None, r) => r,
            };
        }
        progress.built = local.saturating_add(1);
        // A reference whose attributes are all constant has nothing to edit; one whose text is
        // nowhere (no finite anchor) can't be under a click.
        let (true, Some(reach)) = (editable, reach) else { continue };
        match reach {
            Reach::Anywhere => progress.targets.anywhere = true,
            Reach::Within(b) => progress.targets.reach = progress.targets.reach.union(&b),
        }
        progress.targets.refs.push(Target { local, entity: e.clone(), reach });
    }
    progress
}

/// Where attribute `a` of `ins` can draw its text, at any scale in `scales` (smallest, largest),
/// from their own data only: `None` when nowhere (no finite anchor point).
///
/// Single-line text reaches its insertion and alignment points (scaled about the reference's
/// insertion point, as an annotative reference draws them), widened by [`REACH_HEIGHTS_PER_BYTE`]
/// heights per byte of its value and as many more. The height is the layout's (one where it is
/// not positive and finite). Fit text spans its two points at its own height, and a backwards
/// style mirrors that span about the insertion point, so both sides of it are anchors too.
/// Multiline text (inline codes change heights, widths and fonts, and lines wrap) and aligned
/// text (its height follows its width) have no such bound and are candidates anywhere.
/// A finite anchor that is not finite once scaled reaches anywhere too: the box of the scales
/// that stay finite would miss text drawn at a scale in between.
fn attrib_reach(ins: &Insert, a: &Attrib, (lo, hi): (f64, f64)) -> Option<Reach> {
    if a.multiline.is_some() || a.text.halign == HAlign::Aligned {
        return Some(Reach::Anywhere);
    }
    let t = &a.text;
    let height = [lo, hi].iter().map(|f| repaired_height(t.height * f)).fold(0.0, f64::max);
    let wf = t.width_factor.abs();
    let wf = if wf.is_finite() { wf.max(1.0) } else { 1.0 };
    let bytes = t.value.len() as f64;
    let r = height * (REACH_HEIGHTS_PER_BYTE * (bytes + 1.0) * wf + REACH_HEIGHTS_PER_BYTE);
    if !r.is_finite() {
        return Some(Reach::Anywhere);
    }
    let (o, at) = (ins.insert.xy(), t.insert.xy());
    let span = t.align_pt.filter(|_| t.halign == HAlign::Fit).map(|q| at - (q.xy() - at));
    let mut b = Bounds2::EMPTY;
    let mut overflow = false;
    for q in std::iter::once(at).chain(t.align_pt.map(|q| q.xy())).chain(span) {
        // An anchor that is not finite is nowhere. One that overflows when scaled has no box.
        if !q.is_finite() {
            continue;
        }
        for f in [lo, hi] {
            let p = scaled(o, q, f);
            if p.is_finite() {
                b.add(p);
            } else {
                overflow = true;
            }
        }
    }
    if overflow {
        return Some(Reach::Anywhere);
    }
    (!b.is_empty()).then_some(Reach::Within(b.expand(r)))
}

/// The height the layout draws text of height `h` at: `h`, or 1 where it is not positive and
/// finite (`cadcraft_fonts::place`).
fn repaired_height(h: f64) -> f64 {
    if h > 0.0 && h.is_finite() { h } else { 1.0 }
}

/// `q` scaled by `f` about `o`, as an annotative reference draws its attributes; `q` itself at 1.
fn scaled(o: Vec2, q: Vec2, f: f64) -> Vec2 {
    if f == 1.0 { q } else { o + (q - o) * f }
}

fn len_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// The units a name of `bytes` bytes costs to compare, hash or copy.
fn name_units(bytes: usize) -> u64 {
    1u64.saturating_add(len_u64(bytes) / BYTES_PER_UNIT)
}

/// The units a search of a table of `n` entries for a name of `bytes` bytes costs: one compare
/// per entry (a compare stops at a length that differs).
fn table_units(n: usize, bytes: usize) -> u64 {
    1u64.saturating_add(len_u64(n).saturating_mul(name_units(bytes)))
}

/// The units finding one object in `store` costs ([`EntityStore::get`], [`EntityStore::position`]):
/// its chunk among at most as many chunks as objects, then a search of that chunk.
fn store_search_units(store: &EntityStore) -> u64 {
    1u64.saturating_add(len_u64(store.len()) / BYTES_PER_UNIT).saturating_add(CHUNK_BOUND / BYTES_PER_UNIT)
}

/// The block reference with an attribute that can be changed whose attribute text, or whose
/// geometry, a double-click lands on.
///
/// `click.picked` is the ordinary pick at the click in `store` (the space being edited): the
/// topmost object whose geometry or selection box is there. A reference drawn above it whose
/// attribute text the renderer draws within the aperture of the click wins (the topmost such
/// one); otherwise the picked object itself when it is a reference with an attribute to edit;
/// otherwise nothing. So an object drawn over a reference's attribute text (TEXT, say) still takes
/// the click.
///
/// The budget in `pick` covers both the ordinary pick's editability check and the text search.
/// When it runs out, the answer is the ordinary pick if the check found an editable attribute,
/// or `None` otherwise. `targets` is the space's candidate list, kept between clicks.
pub fn attribute_reference_at(
    d: &Drawing,
    store: &EntityStore,
    click: AttribClick<'_>,
    targets: &mut AttribTargets,
    pick: &mut AttribPick,
) -> Option<Handle> {
    let fallback = click.picked.filter(|h| pick.editable(store, *h));
    // ATTMODE (ATTDISP), read once for the click: 0 draws no attribute text, so only the
    // reference's geometry is there to click.
    if pick.charge(1).is_none() {
        return fallback;
    }
    let attmode = d.header.i64("ATTMODE", 1);
    if attmode == 0 {
        return fallback;
    }
    if !targets.refresh(store, pick) {
        return fallback;
    }
    // Only references drawn above the ordinary pick's object compete with it.
    let above = match click.picked {
        None => None,
        Some(h) => {
            if pick.charge(store_search_units(store)).is_none() {
                return fallback;
            }
            match store.position(h) {
                Some(i) => Some(i),
                None => return fallback,
            }
        }
    };
    targets.find(d, above, &click, attmode, pick).or(fallback)
}

/// Attribute `a` of `ins` scaled by `f` as the renderer draws an annotative reference's attributes
/// (`scale_annotative_attrib`), copying only its text and multiline text: the tag, prompt,
/// properties and the multiline value it was read as stay behind, so their size costs nothing.
/// The callers pay for the text, style names and column heights before calling.
fn scaled_text_copy(a: &Attrib, ins: &Insert, f: f64) -> Attrib {
    let mut s = Attrib {
        tag: String::new(),
        text: a.text.clone(),
        invisible: a.invisible,
        constant: a.constant,
        prompt: String::new(),
        props: cadcraft_doc::AttribProps::default(),
        multiline: a.multiline.as_ref().map(|ml| cadcraft_doc::MultilineAttrib { mtext: ml.mtext.clone(), read_as: String::new() }),
        modes: a.modes,
    };
    cadcraft_doc::scale_annotative_attrib(&mut s, ins.insert, f);
    s
}

#[cfg(test)]
mod tests {
    use cadcraft_doc::{Attrib, AttribModes, AttribProps, Common, Drawing, EntityKind, HAlign, Insert, Text, TextStyle, VAlign};
    use cadcraft_geom::{Vec2, Vec3};

    use super::{AnnoView, AttribClick, AttribPick, Blocks, Reach, attrib_reach};

    /// Whether the renderer draws anything of reference `e` inside `b`: what a display list of the
    /// reference alone holds there (its block's own geometry is far away).
    fn renderer_draws_in(d: &Drawing, e: &cadcraft_doc::Entity, b: &cadcraft_geom::Bounds2) -> bool {
        let list = crate::build_entities(d, std::iter::once(e), &crate::Options::default());
        list.prims.iter().any(|p| list.points(p).iter().any(|q| b.contains(*q)))
    }

    /// One visibility case: ATTMODE, the attribute's invisible flag and own layer, its
    /// definition's layer and visibility, the reference's layer, and its clip boundary.
    type Case = (i64, bool, Option<&'static str>, (Option<&'static str>, bool), &'static str, Option<cadcraft_doc::ClipBoundary>);

    /// Every combination of the visibility settings the renderer draws attributes by.
    fn visibility_cases() -> Vec<Case> {
        let square = |x0: f64, inverted: bool| {
            let mut c =
                cadcraft_doc::ClipBoundary::new(vec![Vec2::new(x0, 0.0), Vec2::new(x0 + 20.0, 0.0), Vec2::new(x0 + 20.0, 20.0), Vec2::new(x0, 20.0)]);
            c.inverted = inverted;
            c
        };
        let clips = [None, Some(square(-5.0, false)), Some(square(30.0, false)), Some(square(-5.0, true)), Some(square(30.0, true))];
        let mut out = Vec::new();
        for attmode in [0, 1, 2] {
            for invisible in [false, true] {
                for own in [None, Some("ON"), Some("OFF"), Some("FRZ"), Some("0")] {
                    for def in [(None, true), (Some("0"), true), (Some("OFF"), true), (Some("ON"), false)] {
                        for reference in ["0", "ON", "OFF"] {
                            out.extend(clips.iter().map(|c| (attmode, invisible, own, def, reference, c.clone())));
                        }
                    }
                }
            }
        }
        out
    }

    /// Block "B": a line far from the attribute text and, with `def` set, the attribute's
    /// definition on that layer and with that visibility.
    fn block_with_definition(def: (Option<&str>, bool)) -> cadcraft_doc::Block {
        let mut blk = cadcraft_doc::Block::new("B");
        blk.entities.push(cadcraft_doc::Entity::new(
            cadcraft_doc::Handle(900),
            EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(-50.0, -50.0, 0.0), b: Vec3::new(-49.0, -50.0, 0.0) }),
        ));
        if let (Some(layer), visible) = def {
            let mut e = cadcraft_doc::Entity::new(cadcraft_doc::Handle(901), EntityKind::AttDef(attrib(text(Vec3::ZERO, "d", 1.0))));
            e.common.layer = layer.into();
            e.common.visible = visible;
            blk.entities.push(e);
        }
        blk
    }

    /// The pick takes attribute text exactly where the renderer draws it: ATTMODE, the invisible
    /// flag, the attribute's own layer or its definition's (layer 0 being the reference's), the
    /// definition's visibility, layers off and frozen, the reference's own visibility and layer,
    /// and its XCLIP boundary, normal and inverted (1,800 cases). Each case is drawn, and picked at
    /// the middle of the attribute's box.
    #[test]
    fn the_pick_takes_attribute_text_exactly_where_the_renderer_draws_it() {
        let mut d = Drawing::default();
        for (name, on, frozen) in [("ON", true, false), ("OFF", false, false), ("FRZ", true, true)] {
            d.ensure_layer(name);
            let l = d.layer_mut(name).unwrap();
            l.on = on;
            l.frozen = frozen;
        }
        let value = attrib(text(Vec3::new(5.0, 5.0, 0.0), "WWW", 1.0));
        let drawn_box = crate::place_text_entity(&d, &value.text, &value.text.value).1;
        let at = drawn_box.center();
        let cases = visibility_cases();
        assert_eq!(cases.len(), 1_800);
        let mut drawn_cases = 0;
        for (attmode, invisible, own, def, reference, clip) in cases {
            d.header.set("ATTMODE", cadcraft_doc::HVal::Int(attmode));
            d.blocks.insert("B".into(), std::sync::Arc::new(block_with_definition(def)));
            let mut a = value.clone();
            a.invisible = invisible;
            a.props.layer = own.map(Into::into);
            let ins = Insert { block: "B".into(), insert: Vec3::ZERO, attribs: vec![a], clip, ..Insert::default() };
            let mut e = cadcraft_doc::Entity::new(cadcraft_doc::Handle(1000), EntityKind::Insert(ins));
            e.common.layer = reference.into();
            let drawn = renderer_draws_in(&d, &e, &drawn_box);
            let click = AttribClick { at, aperture: 0.0, picked: None, frozen: &[], anno: AnnoView::Model };
            let mut blocks: Blocks<'_> = std::collections::HashMap::new();
            let picked = AttribPick::new().drawn_at(&d, &e, &click, attmode, &mut blocks);
            assert_eq!(
                picked,
                Some(drawn),
                "ATTMODE {attmode}, invisible {invisible}, own layer {own:?}, definition {def:?}, reference on {reference}, {e:?}"
            );
            drawn_cases += usize::from(drawn);
        }
        assert!(drawn_cases > 0 && drawn_cases < 1_800, "both outcomes are exercised: {drawn_cases} drawn");
    }

    /// Layers a layout viewport freezes hide the attribute text the viewport would draw: the
    /// reference's own layer and the attribute's.
    #[test]
    fn layers_the_viewport_freezes_hide_attribute_text() {
        let mut d = Drawing::default();
        d.ensure_layer("VP");
        let mut blk = cadcraft_doc::Block::new("B");
        blk.entities.push(cadcraft_doc::Entity::new(
            cadcraft_doc::Handle(900),
            EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(-50.0, -50.0, 0.0), b: Vec3::new(-49.0, -50.0, 0.0) }),
        ));
        d.blocks.insert("B".into(), std::sync::Arc::new(blk));
        let a = attrib(text(Vec3::new(5.0, 5.0, 0.0), "WWW", 1.0));
        let at = crate::place_text_entity(&d, &a.text, &a.text.value).1.center();
        let frozen = vec!["vp".to_string()];
        for (own, reference, shown) in [(None, "0", true), (Some("VP"), "0", false), (None, "VP", false), (Some("0"), "VP", false)] {
            let mut a = a.clone();
            a.props.layer = own.map(Into::into);
            let mut e = cadcraft_doc::Entity::new(
                cadcraft_doc::Handle(1000),
                EntityKind::Insert(Insert { block: "B".into(), attribs: vec![a], ..Insert::default() }),
            );
            e.common.layer = reference.into();
            let click = AttribClick { at, aperture: 0.0, picked: None, frozen: &frozen, anno: AnnoView::Viewport(1.0) };
            let mut blocks: Blocks<'_> = std::collections::HashMap::new();
            assert_eq!(AttribPick::new().drawn_at(&d, &e, &click, 1, &mut blocks), Some(shown), "own {own:?}, reference on {reference}");
        }
    }

    fn text(at: Vec3, value: &str, height: f64) -> Text {
        Text {
            insert: at,
            align_pt: None,
            height,
            value: value.into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: HAlign::Left,
            valign: VAlign::Baseline,
        }
    }

    fn attrib(t: Text) -> Attrib {
        Attrib {
            tag: "T".into(),
            text: t,
            invisible: false,
            constant: false,
            prompt: String::new(),
            props: AttribProps::default(),
            multiline: None,
            modes: AttribModes::default(),
        }
    }

    /// Every text of `variants` with each of `values` set by `set`.
    fn vary<T: Copy>(variants: &[Text], values: &[T], set: impl Fn(&mut Text, T)) -> Vec<Text> {
        let mut out = Vec::with_capacity(variants.len() * values.len());
        for t in variants {
            for v in values {
                let mut t = t.clone();
                set(&mut t, *v);
                out.push(t);
            }
        }
        out
    }

    /// The reach holds the box the renderer lays single-line attribute text out in, across the
    /// justifications, turns, width factors, heights (repaired ones too), text lengths, Fit spans
    /// and the style flags that mirror or turn the text: 19,200 combinations.
    #[test]
    fn the_reach_holds_every_box_the_renderer_lays_single_line_text_out_in() {
        let styles: [(&str, bool, bool, bool); 4] =
            [("Standard", false, false, false), ("Back", true, false, false), ("Down", false, true, false), ("Up", false, false, true)];
        let mut d = Drawing::default();
        for (name, backwards, upside_down, vertical) in styles {
            d.text_styles.push(TextStyle { name: name.into(), backwards, upside_down, vertical, ..TextStyle::default() });
        }
        let ins = Insert { block: "B".into(), insert: Vec3::new(3.0, -2.0, 0.0), ..Insert::default() };
        let base = Text { align_pt: Some(Vec3::new(30.0, -4.0, 0.0)), ..text(Vec3::new(10.0, 5.0, 0.0), "W", 1.0) };
        let v = vary(&[base], &[HAlign::Left, HAlign::Center, HAlign::Right, HAlign::Middle, HAlign::Fit], |t, h| t.halign = h);
        let v = vary(&v, &[VAlign::Baseline, VAlign::Bottom, VAlign::Middle, VAlign::Top], |t, x| t.valign = x);
        let v = vary(&v, &styles.map(|s| s.0), |t, s| t.style = s.into());
        let v = vary(&v, &[0.0, 0.7, -2.5], |t, r| t.rotation = r);
        let v = vary(&v, &[1.0, 0.3, 3.5, -2.0], |t, w| t.width_factor = w);
        let v = vary(&v, &[1.0, 0.05, 40.0, 0.0, -3.0], |t, h| t.height = h);
        let v = vary(&v, &["W", "WWWWWWWWWWWW", "%%c50 Ø ü 漢字", " "], |t, s| t.value = s.into());
        assert_eq!(v.len(), 19_200);
        for t in v {
            let a = attrib(t);
            let drawn = crate::place_text_entity(&d, &a.text, &a.text.value).1;
            let Some(Reach::Within(reach)) = attrib_reach(&ins, &a, (1.0, 1.0)) else { panic!("single-line text has a bounded reach") };
            assert!(drawn.is_empty() || reach.contains_box(&drawn), "{drawn:?} outside {reach:?} for {:?}", a.text);
        }
    }

    /// The list keeps nothing a layout reads: after a text style change, the next click lays the
    /// attribute out with the new style. Its unwrapped multiline text widens with the style's
    /// width factor and is found on the widened part.
    #[test]
    fn a_kept_list_sees_a_text_style_change() {
        let mut d = Drawing::default();
        d.blocks.insert("B".into(), std::sync::Arc::new(cadcraft_doc::Block::new("B")));
        let mtext = cadcraft_doc::MText {
            insert: Vec3::new(100.0, 0.0, 0.0),
            height: 1.0,
            width: 0.0,
            attach: 1,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "AAAA".into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        };
        let mut a = attrib(text(Vec3::new(100.0, 0.0, 0.0), "AAAA", 1.0));
        a.multiline = Some(cadcraft_doc::MultilineAttrib { mtext: mtext.clone(), read_as: "AAAA".into() });
        let ins = Insert { block: "B".into(), insert: Vec3::new(100.0, 0.0, 0.0), attribs: vec![a], ..Insert::default() };
        let h = d.add(&cadcraft_doc::Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
        let narrow = crate::layout_mtext_entity(&d, &mtext).bounds;
        let mut targets = super::AttribTargets::default();
        let click = |at: Vec2| AttribClick { at, aperture: 0.01, picked: None, frozen: &[], anno: AnnoView::Model };
        let store = d.model.clone();
        let blank = super::attribute_reference_at(&d, &store, click(Vec2::new(130.0, 15.0)), &mut targets, &mut AttribPick::new());
        assert_eq!(blank, None);
        if let Some(st) = d.text_styles.iter_mut().find(|s| s.name.eq_ignore_ascii_case("Standard")) {
            st.width_factor = 10.0;
        } else {
            d.text_styles.push(TextStyle { width_factor: 10.0, ..TextStyle::default() });
        }
        let wide = crate::layout_mtext_entity(&d, &mtext).bounds;
        let at = Vec2::new(f64::midpoint(narrow.max.x, wide.max.x), wide.center().y);
        assert!(wide.contains(at) && !narrow.expand(2.0).contains(at), "{narrow:?} {wide:?} {at:?}");
        let mut pick = AttribPick::new();
        let found = super::attribute_reference_at(&d, &store, click(at), &mut targets, &mut pick);
        assert_eq!(found, Some(h), "{:?}", pick.stats());
        assert_eq!(pick.stats().chunks_built, 0, "the list was kept: only the layout saw the change");
    }

    /// An annotative reference draws its attributes scaled about its insertion point: the reach
    /// covers every scale it lists.
    #[test]
    fn an_annotative_reach_holds_the_text_at_each_of_its_scales() {
        let d = Drawing::default();
        let ins = Insert { block: "B".into(), insert: Vec3::new(100.0, 100.0, 0.0), ..Insert::default() };
        let a = attrib(text(Vec3::new(104.0, 101.0, 0.0), "TAG-01", 2.5));
        let Some(Reach::Within(reach)) = attrib_reach(&ins, &a, (0.5, 4.0)) else { panic!("bounded") };
        for f in [0.5, 1.0, 2.0, 4.0] {
            let mut e = cadcraft_doc::Entity::new(cadcraft_doc::Handle(1), EntityKind::Insert(Insert { attribs: vec![a.clone()], ..ins.clone() }));
            e.common = Common::default();
            let scaled = cadcraft_doc::anno_scaled(&e, f);
            let EntityKind::Insert(s) = &scaled.kind else { panic!("an insert") };
            let t = &s.attribs.first().map(|a| a.text.clone()).unwrap_or_else(|| panic!("an attribute"));
            let drawn = crate::place_text_entity(&d, t, &t.value).1;
            assert!(reach.contains_box(&drawn), "scale {f}: {drawn:?} outside {reach:?}");
        }
    }

    /// Multiline and aligned text have no bound from their own data: candidates anywhere. Text
    /// with no finite anchor is nowhere.
    #[test]
    fn multiline_and_aligned_text_reach_anywhere_and_text_without_an_anchor_nowhere() {
        let ins = Insert::default();
        let mut aligned = attrib(text(Vec3::ZERO, "A", 1.0));
        aligned.text.halign = HAlign::Aligned;
        assert_eq!(attrib_reach(&ins, &aligned, (1.0, 1.0)), Some(Reach::Anywhere));
        let mut multi = attrib(text(Vec3::ZERO, "A", 1.0));
        let mtext = cadcraft_doc::MText {
            insert: Vec3::ZERO,
            height: 1.0,
            width: 0.0,
            attach: 1,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "A".into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        };
        multi.multiline = Some(cadcraft_doc::MultilineAttrib { mtext, read_as: "A".into() });
        assert_eq!(attrib_reach(&ins, &multi, (1.0, 1.0)), Some(Reach::Anywhere));
        let nowhere = attrib(text(Vec3::new(f64::NAN, 0.0, 0.0), "A", 1.0));
        assert_eq!(attrib_reach(&ins, &nowhere, (1.0, 1.0)), None);
        let huge = attrib(text(Vec3::ZERO, "A", f64::MAX));
        assert_eq!(attrib_reach(&ins, &huge, (1.0, 1.0)), Some(Reach::Anywhere), "a reach too large to box is anywhere");
    }

    /// An annotative reference's multiline columns keep the gutter and column height the renderer
    /// keeps (position, text height and width scale; the column lengths do not). The second column
    /// is found where [`crate::build_entities`] draws it, at a scale below 1 and a scale above 1.
    #[test]
    fn annotative_multiline_columns_are_found_where_the_renderer_draws_them() {
        // Factor 0.5 is the review's click. Factor 2 is the same columns drawn larger.
        for (name, paper, drawing) in [("2:1", 2.0, 1.0), ("1:2", 1.0, 2.0)] {
            let factor = drawing / paper;
            let mut d = Drawing::default();
            let scale = cadcraft_doc::AnnoScale::new(name, paper, drawing);
            d.scales.push(scale.clone());
            d.header.set("CANNOSCALE", cadcraft_doc::HVal::Str(name.into()));
            let mut blk = cadcraft_doc::Block::new("B");
            blk.entities.push(cadcraft_doc::Entity::new(
                cadcraft_doc::Handle(900),
                EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(-1_000.0, -1_000.0, 0.0), b: Vec3::new(-999.0, -1_000.0, 0.0) }),
            ));
            d.blocks.insert("B".into(), std::sync::Arc::new(blk));
            let mtext = cadcraft_doc::MText {
                insert: Vec3::ZERO,
                height: 1.0,
                width: 1.0,
                attach: 1,
                rotation: 0.0,
                style: "Standard".into(),
                contents: "A\\NB".into(),
                line_spacing: 1.0,
                line_spacing_exact: false,
                columns: Some(cadcraft_doc::MTextColumns {
                    kind: cadcraft_doc::MTextColumnType::Static,
                    count: 2,
                    gutter: 100.0,
                    height: 10.0,
                    ..cadcraft_doc::MTextColumns::default()
                }),
            };
            let mut a = attrib(text(Vec3::ZERO, "A\nB", 1.0));
            a.multiline = Some(cadcraft_doc::MultilineAttrib { mtext, read_as: "A\nB".into() });
            let ins = Insert { block: "B".into(), insert: Vec3::ZERO, attribs: vec![a], ..Insert::default() };
            let common = Common { anno_scales: vec![scale], ..Common::default() };
            let h = d.add(&cadcraft_doc::Space::Model, common, EntityKind::Insert(ins)).expect("added");
            let e = d.model.get(h).expect("stored").as_ref().clone();
            let scaled = cadcraft_doc::anno_scaled(&e, factor);
            let EntityKind::Insert(sins) = &scaled.kind else { panic!("an insert") };
            let laid = sins.attribs.first().and_then(|at| at.multiline_text()).expect("multiline");
            let layout = crate::layout_mtext_entity(&d, &laid).bounds;
            assert!(layout.max.x > 100.0 && layout.max.x < 150.0, "the renderer keeps the gutter of 100 at factor {factor}: {layout:?}");
            let list = crate::build_entities(&d, std::iter::once(&e), &crate::Options::default());
            assert!(list.prims.iter().flat_map(|p| list.points(p)).any(|q| q.x > 100.0), "build_entities draws the second column at factor {factor}");
            let store = d.model.clone();
            let mut targets = super::AttribTargets::default();
            let mut pick = |at: Vec2| {
                let click = AttribClick { at, aperture: 0.0, picked: None, frozen: &[], anno: AnnoView::Model };
                super::attribute_reference_at(&d, &store, click, &mut targets, &mut AttribPick::new())
            };
            let inside = layout.center();
            assert_eq!(pick(inside), Some(h), "factor {factor} centre {inside:?} of {layout:?}");
            let past = Vec2::new(layout.max.x + 10.0, inside.y);
            assert_eq!(pick(past), None, "factor {factor}: {past:?} is outside the rendered box {layout:?}");
            if factor < 1.0 {
                let at = Vec2::new(100.6, -0.25);
                assert!(layout.contains(at), "the review click {at:?} is inside the rendered box {layout:?}");
                assert_eq!(pick(at), Some(h), "a click at {at:?} opens the editor");
            }
        }
    }

    /// Scaling an annotative attribute for the pick copies only the text the budget has paid for:
    /// an oversized tag, prompt or multiline read-as value is not copied, and the text and
    /// multiline text come out exactly as the renderer's transform leaves them.
    #[test]
    fn a_scaled_attribute_copy_leaves_its_metadata_behind() {
        let big = "T".repeat(2_000_000);
        let mut a = attrib(text(Vec3::new(10.0, 0.0, 0.0), "A", 1.0));
        a.tag = big.clone();
        a.prompt = big.clone();
        a.props.layer = Some(big.clone());
        let mtext = cadcraft_doc::MText {
            insert: Vec3::new(10.0, 0.0, 0.0),
            height: 1.0,
            width: 4.0,
            attach: 1,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "A".into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        };
        a.multiline = Some(cadcraft_doc::MultilineAttrib { mtext, read_as: big });
        let ins = Insert { block: "B".into(), insert: Vec3::new(2.0, 0.0, 0.0), attribs: vec![], ..Insert::default() };
        let copy = super::scaled_text_copy(&a, &ins, 0.5);
        assert!(copy.tag.is_empty() && copy.prompt.is_empty() && copy.props.layer.is_none(), "metadata is not copied");
        let ml = copy.multiline.as_ref().expect("multiline text kept");
        assert!(ml.read_as.is_empty(), "the read-as value is not copied");
        let mut full = a.clone();
        cadcraft_doc::scale_annotative_attrib(&mut full, ins.insert, 0.5);
        assert_eq!(copy.text, full.text, "the text scales as the renderer scales it");
        assert_eq!(Some(&ml.mtext), full.multiline.as_ref().map(|m| &m.mtext), "the multiline text scales as the renderer scales it");
    }

    /// A scaled text copies its style name as well as its value, so the auxiliary single-line
    /// style of a multiline attribute is paid for before its layout, even though only the
    /// multiline style is read. An oversized single-line style stops `text_box` at that charge:
    /// the pick runs out and lays nothing out rather than laying the text out and reporting its box.
    #[test]
    fn a_scaled_multiline_attributes_auxiliary_is_paid_for_before_its_layout() {
        let d = Drawing::default();
        let mtext = cadcraft_doc::MText {
            insert: Vec3::new(10.0, 0.0, 0.0),
            height: 1.0,
            width: 4.0,
            attach: 1,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "A".into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        };
        let mut a = attrib(text(Vec3::new(10.0, 0.0, 0.0), "A", 1.0));
        a.multiline = Some(cadcraft_doc::MultilineAttrib { mtext, read_as: "A".into() });
        // The single-line style is not the multiline one; `scaled_text_copy` copies both texts.
        a.text.style = "S".repeat(2_000_000);
        let ins = Insert { block: "B".into(), insert: Vec3::new(2.0, 0.0, 0.0), attribs: vec![], ..Insert::default() };
        // Enough for the layout of a one-byte value without that copy (about 1,500 units), far
        // too little for the style's 2,000,000 bytes at 16 units each.
        let mut pick = AttribPick::with_budget(100_000);
        let laid = pick.text_box(&d, &ins, &a, 0.5);
        let stats = pick.stats();
        assert_eq!(stats.text_layouts, 0, "the layout was not paid for: {stats:?}");
        assert!(stats.exhausted, "the auxiliary style's charge did not run out the budget: {stats:?}");
        assert_eq!(laid, None, "a budget that cannot pay for the copy lays nothing out");
    }

    /// One store chunk of 1,024 references costs more than a 1,500-unit budget. Later clicks
    /// continue that chunk and finish it; each object is read once.
    #[test]
    fn a_chunk_bigger_than_the_budget_is_finished_by_later_clicks() {
        const REFS: usize = 1_024;
        let mut d = Drawing::default();
        d.blocks.insert("B".into(), std::sync::Arc::new(cadcraft_doc::Block::new("B")));
        for i in 0..REFS {
            let x = i as f64 * 100.0;
            let a = attrib(text(Vec3::new(x, 1.0, 0.0), "A", 1.0));
            let ins = Insert { block: "B".into(), insert: Vec3::new(x, 0.0, 0.0), attribs: vec![a], ..Insert::default() };
            d.add(&cadcraft_doc::Space::Model, Common::default(), EntityKind::Insert(ins)).expect("added");
        }
        let store = d.model.clone();
        assert_eq!(store.chunk_slices().count(), 1, "the references share one chunk");
        let mut targets = super::AttribTargets::default();
        let mut clicks = 0u32;
        let mut visits = 0u64;
        loop {
            clicks += 1;
            assert!(clicks <= 4, "a chunk of {REFS} references never finished under a 1,500 budget");
            let mut pick = AttribPick::with_budget(1_500);
            let done = targets.refresh(&store, &mut pick);
            let stats = pick.stats();
            visits += stats.entities_visited;
            if clicks == 1 {
                assert!(!done, "one budget cannot build {REFS} references");
                assert!(stats.exhausted, "{stats:?}");
                assert!(stats.entities_visited < REFS as u64, "the first click stops inside the chunk: {stats:?}");
            }
            if done {
                assert!(clicks > 1, "later clicks were not needed");
                break;
            }
            assert!(stats.exhausted, "click {clicks} stopped inside the chunk: {stats:?}");
        }
        assert_eq!(visits, REFS as u64, "each object of the chunk is read once");
    }

    /// Scaling a finite anchor by the reference's largest supported factor overflows. The reach is
    /// then anywhere, so a finite scale between the ends (1.5, between 1 and 2) is still covered.
    /// A box of only the finite endpoint is too small.
    #[test]
    fn a_finite_anchor_that_overflows_when_scaled_reaches_anywhere() {
        let ins = Insert { block: "B".into(), insert: Vec3::ZERO, ..Insert::default() };
        let a = attrib(text(Vec3::new(9.0e307, 0.0, 0.0), "A", 1.0));
        // The same widening `draw_scales` applies to supported factors 1 and 2.
        let lo = 1.0 * (1.0 - 1e-8);
        let hi = 2.0 * (1.0 + 1e-8);
        let at = Vec2::new(9.0e307 * 1.5, 0.0);
        let finite = Vec2::new(9.0e307 * lo, 0.0);
        let upper: f64 = 9.0e307 * hi;
        assert!(at.is_finite() && finite.is_finite(), "{at:?} {finite:?}");
        assert!(!upper.is_finite(), "the cached upper endpoint overflows: {upper}");
        let radius = hi * (super::REACH_HEIGHTS_PER_BYTE * 2.0 * 1.0 + super::REACH_HEIGHTS_PER_BYTE);
        assert!((at.x - finite.x).abs() > radius, "the finite endpoint plus its radius cannot contain scale 1.5");
        assert_eq!(attrib_reach(&ins, &a, (lo, hi)), Some(Reach::Anywhere));
    }
}
