//! DXF → Drawing.

use std::collections::HashMap;

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{
    Arc, AssocSnap, Attrib, AttribModes, AttribProps, Block, Circle, Common, DashElement, DimAssoc, DimKind, DimStyle, Dimension, Drawing, Ellipse,
    Entity, EntityKind, Face3d, Gradient, Group, HAlign, HVal, Handle, Hatch, HatchLoop, Helix, HelixConstrain, Insert, Layer, Layout, Leader, Line,
    Linetype, Lineweight, LwPolyline, MAX_FRAME_CHARS, MLeaderStyle, MText, MultilineAttrib, NamedView, PageSetup, Point, Polyline3d, RawTag,
    RayLine, Solid, Table, TableCell, Text, TextStyle, Tolerance, Transparency, Ucs, Unknown, VAlign, Viewport, VportConfig, VportTile, Wipeout,
};
use cadcraft_geom::{PolyVertex, Spline, Vec2, Vec3};
use craft_dxf::{Tag, records, sections};

use crate::{IoError, Result};

mod common;
mod dimassoc;
mod document;
mod entities;
mod hatch;
mod objects;
mod tables;
mod text;

use document::{
    apply_sortents, count_skipped, dropped_paper_views, outside_sections, parse_entities, place_blocks, place_entities, read_blocks, read_objects,
    setup_layouts,
};
#[cfg(test)]
use entities::acad_table;
use entities::{entity, plot_style_names};
use objects::Objects;
use tables::{header, tables};

pub(crate) mod mtext_dxf;

/// Handle maps and references gathered while reading.
#[derive(Default)]
struct Rx {
    /// STYLE record handle (upper case) → text style name.
    styles: HashMap<String, String>,
    /// BLOCK_RECORD handle (upper case) → block name.
    brs: HashMap<String, String>,
    /// LTYPE record handle (upper case) → linetype name (DIMLTYPE, DIMLTEX1, DIMLTEX2).
    ltypes: HashMap<String, String>,
    /// Upper-case block name → (insertion units, explodable) from its BLOCK_RECORD.
    block_props: HashMap<String, (u8, bool)>,
    /// Upper-case names of anonymous `*T` blocks drawn by ACAD_TABLE entities.
    table_blocks: std::collections::HashSet<String>,
    /// Upper-case block names used by INSERTs.
    inserted: std::collections::HashSet<String>,
    /// Tables whose style is known only by TABLESTYLE handle: (table, style handle).
    table_style_fix: Vec<(Handle, String)>,
    /// Multileaders' style handles and block attribute values, resolved with the objects.
    mleader_fix: Vec<(Handle, crate::dxf_mleader::Pending)>,
    /// Upper-case xref block name → the external drawing's path (BLOCK group 1).
    xref_paths: HashMap<String, String>,
    /// Plot style name placeholder handle (upper case) → plot style name (`ACAD_PLOTSTYLENAME`).
    pstyles: HashMap<String, String>,
    /// Upper-case block name → associative array parameters from its BLOCK_RECORD.
    block_arrays: HashMap<String, cadcraft_doc::ArrayParams>,
    /// LAYER record handle (upper case) → layer name (VIEWPORT group 331/341 frozen layers).
    layers: HashMap<String, String>,
    /// Owner handle, paper flag and enclosing block (upper case) of each generated sheet viewport
    /// (PAPERVIEW) the reader dropped.
    paper_views_dropped: Vec<(Option<String>, bool, Option<String>)>,
    /// Upper-case name of the BLOCK whose entities are being read, if any.
    block: Option<String>,
}

/// Read a DXF file into a drawing.
pub fn read(bytes: &[u8]) -> Result<Drawing> {
    read_budgeted(bytes, 4_000_000)
}

/// A DXF drawing read in bounded steps, for a host with no worker thread.
///
/// The browser has one thread: a large snapshot parsed in one call freezes the page for seconds.
/// [`Incremental`] decodes the file's sections and its entity records in bounded batches, so a
/// caller can spread the work over frames. The result is the same drawing [`read`] produces.
///
/// ```no_run
/// # fn run(bytes: &[u8]) -> Result<(), cadcraft_io::IoError> {
/// let mut reader = cadcraft_io::read_incremental(bytes)?;
/// while !reader.step(4096)? {}
/// let drawing = reader.drawing();
/// # let _ = drawing;
/// # Ok(())
/// # }
/// ```
pub struct Incremental {
    /// The sections, with their tags: the final passes (images, multileaders, raw objects) read
    /// them again, so they are kept until the drawing is finished.
    sections: Vec<craft_dxf::Section>,
    section: usize,
    drawing: Drawing,
    rx: Rx,
    blocks: Vec<BlockDef>,
    layout_objs: Vec<LayoutDef>,
    entities: Vec<(Option<String>, bool, Entity)>,
    objs: Objects,
    skipped: std::collections::BTreeMap<String, usize>,
    cells_left: usize,
    /// The ENTITIES section being read in batches: its record starts and the next one.
    entity_job: Option<EntityJob>,
    total_records: usize,
    read_records: usize,
    finished: bool,
}

/// One ENTITIES section split into records, read a batch at a time.
struct EntityJob {
    /// The index into [`Incremental::sections`], for its tags.
    section: usize,
    /// Tag offsets where each record (a code-0 group) starts.
    starts: Vec<usize>,
    /// The next record to read.
    next: usize,
    /// Where this section's entities begin in [`Incremental::entities`], for the per-section
    /// duplicate-handle pass.
    first_entity: usize,
}

impl Incremental {
    /// Start reading a DXF file. Parsing the file's groups and splitting the sections happen here;
    /// the entity records and the final passes happen in [`step`](Self::step).
    pub fn new(bytes: &[u8]) -> Result<Self> {
        let tags = craft_dxf::parse(bytes).map_err(|e| IoError::Format(e.to_string()))?;
        let sections = sections(&tags);
        if sections.is_empty() {
            return Err(IoError::Format("no sections found".into()));
        }
        let mut drawing = Drawing::new_imperial();
        drawing.layouts.clear();
        // A file without $INSUNITS declares no units (#1209); only new drawings take the template's.
        drawing.header.set_i64("INSUNITS", 0);
        let rx = Rx { pstyles: plot_style_names(&sections), ..Rx::default() };
        // Every handle another program's file already uses is taken before anything gets a new
        // one: its $HANDSEED can be lower than its handles, and entities made while reading would
        // otherwise get handles that records further on still have. A file CADCraft wrote names
        // its own next handle, above which its rebuilt records are numbered (#495).
        let ours = sections.iter().any(|s| s.name == "OBJECTS" && s.tags.iter().any(|t| t.code == 3 && t.str() == crate::dxf_ext::HANDSEED_KEY));
        for t in sections.iter().filter(|_| !ours).flat_map(|s| s.tags.iter()).filter(|t| t.code == 5 || t.code == 105) {
            if let Some(h) = Handle::parse_hex(t.str().trim()) {
                drawing.bump_handseed(h);
            }
        }
        let mut skipped = std::collections::BTreeMap::new();
        count_skipped(&mut skipped, "(no section)", outside_sections(&tags));
        let total_records =
            sections.iter().filter(|section| section.name == "ENTITIES").map(|section| section.tags.iter().filter(|tag| tag.code == 0).count()).sum();
        Ok(Incremental {
            sections,
            section: 0,
            drawing,
            rx,
            blocks: Vec::new(),
            layout_objs: Vec::new(),
            entities: Vec::new(),
            objs: Objects::default(),
            skipped,
            cells_left: 4_000_000,
            entity_job: None,
            total_records,
            read_records: 0,
            finished: false,
        })
    }

    /// How many entity records the file has, for a progress bar (`read N/M`).
    pub fn total_records(&self) -> usize {
        self.total_records
    }

    /// How many entity records [`step`](Self::step) has read so far.
    pub fn read_records(&self) -> usize {
        self.read_records
    }

    /// Whether the drawing is complete.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Read up to `records` entity records, or one non-entity section, and return whether the
    /// drawing is complete. A browser calls this once per frame with a budget it can afford.
    pub fn step(&mut self, budget: usize) -> Result<bool> {
        if self.finished {
            return Ok(true);
        }
        loop {
            if let Some(job) = self.entity_job.as_mut() {
                let budget = budget.max(1);
                let section = job.section;
                let starts_len = job.starts.len();
                if job.next >= starts_len {
                    self.entity_job = None;
                    continue;
                }
                let start = job.next;
                let mut end = (start + budget).min(starts_len);
                // A batch must not split a POLYLINE (or an INSERT with attributes) from the
                // VERTEX/ATTRIB records that follow it: end only at a record that starts a new
                // object. `starts_len` is always a safe end (the section is over).
                let tags = &self.sections[section].tags;
                while end < starts_len && matches!(tags[job.starts[end]].str().as_str(), "VERTEX" | "ATTRIB" | "SEQEND") {
                    end += 1;
                }
                let from = job.starts[start];
                let to = if end < starts_len { job.starts[end] } else { tags.len() };
                let chunk = records(&tags[from..to]);
                let mut skipped = 0;
                let parsed = parse_entities(&chunk, &mut self.drawing, &mut self.rx, &mut skipped, &mut self.cells_left);
                count_skipped(&mut self.skipped, "ENTITIES", skipped);
                self.entities.extend(parsed);
                self.read_records = self.read_records.saturating_add(end - start);
                job.next = end;
                if end < starts_len {
                    return Ok(false);
                }
                // The section is over: a duplicate handle is only a duplicate within it, as in
                // the one-call reader.
                let Some(job) = self.entity_job.take() else { continue };
                let mut seen = std::collections::HashSet::new();
                for (_, _, entity) in &mut self.entities[job.first_entity..] {
                    if !seen.insert(entity.handle) {
                        entity.handle = self.drawing.new_handle();
                        seen.insert(entity.handle);
                    }
                }
                self.section = section + 1;
                continue;
            }
            if self.section >= self.sections.len() {
                self.finish()?;
                return Ok(true);
            }
            let name = self.sections[self.section].name.clone();
            if name == "ENTITIES" {
                let starts: Vec<usize> =
                    self.sections[self.section].tags.iter().enumerate().filter(|(_, tag)| tag.code == 0).map(|(i, _)| i).collect();
                self.entity_job = Some(EntityJob { section: self.section, starts, next: 0, first_entity: self.entities.len() });
                continue;
            }
            let tags = &self.sections[self.section].tags;
            match name.as_str() {
                "HEADER" => header(tags, &mut self.drawing),
                "TABLES" => tables(tags, &mut self.drawing, &mut self.rx),
                "BLOCKS" => self.blocks.extend(read_blocks(tags, &mut self.drawing, &mut self.rx, &mut self.cells_left, &mut self.skipped)),
                "OBJECTS" => read_objects(tags, &mut self.objs, &mut self.layout_objs),
                _ => {}
            }
            self.section += 1;
            return Ok(false);
        }
    }

    /// The finished drawing. Calling it before the last step is a programming error: the caller
    /// gets what has been read so far, but the last sections are missing.
    pub fn drawing(self) -> Drawing {
        self.drawing
    }

    /// The passes that need every section at once: layouts, blocks, entities, sortents, images
    /// and the records kept for the writer.
    fn finish(&mut self) -> Result<()> {
        let own_seed = self.objs.xrecord_text(crate::dxf_ext::HANDSEED_KEY).and_then(|t| Handle::parse_hex(t.trim()));
        let (paper_block_for, first_layout) = setup_layouts(&mut self.drawing, &mut self.layout_objs, &self.rx);
        place_blocks(&mut self.drawing, std::mem::take(&mut self.blocks), &mut self.rx, &paper_block_for, &first_layout);
        place_entities(&mut self.drawing, std::mem::take(&mut self.entities), &self.rx, &paper_block_for, &first_layout);
        number_unknown_viewport_ids(&mut self.drawing, &dropped_paper_views(&self.rx, &paper_block_for, &first_layout));
        apply_sortents(&mut self.drawing, &self.objs.sortents, &self.rx.brs, &paper_block_for, &first_layout);
        std::mem::take(&mut self.objs).apply(&mut self.drawing, &self.rx);
        crate::dxf_image::resolve(&self.sections, &mut self.drawing);
        crate::dxf_mline::resolve(&self.sections, &mut self.drawing);
        // Keep handles unique against the header's seed.
        let seed = self.drawing.header.str("HANDSEED", "");
        if let Some(h) = own_seed.or_else(|| Handle::parse_hex(&seed)) {
            self.drawing.bump_handseed(Handle(h.0.saturating_sub(1)));
        }
        if self.drawing.layer("0").is_none() {
            self.drawing.layers.insert(0, Layer::default());
        }
        crate::dxf_raw::read(&self.sections, &mut self.drawing);
        crate::dxf_clip::read(&mut self.drawing);
        self.drawing.skipped_records = std::mem::take(&mut self.skipped);
        self.finished = true;
        Ok(())
    }
}

/// A block definition read from BLOCKS: name, base point, description, entities (owner, paper
/// flag, entity) and block flags.
type BlockDef = (String, Vec3, String, Vec<(Option<String>, bool, Entity)>, i64);

/// A layout read from OBJECTS: name, tab order, paper block handle and page setup.
/// A LAYOUT object: name, tab order, block record, page setup and its limits (groups 10/11).
type LayoutDef = (String, u32, String, PageSetup, Option<(Vec2, Vec2)>);

/// Import DXF sections while sharing one materialized-table-cell budget across blocks and entity sections.
fn read_budgeted(bytes: &[u8], mut table_cells_left: usize) -> Result<Drawing> {
    let tags = craft_dxf::parse(bytes).map_err(|e| IoError::Format(e.to_string()))?;
    let secs = sections(&tags);
    if secs.is_empty() {
        return Err(IoError::Format("no sections found".into()));
    }
    let mut d = Drawing::new_imperial();
    d.layouts.clear();
    // A file without $INSUNITS declares no units (#1209); only new drawings take the template's.
    d.header.set_i64("INSUNITS", 0);
    let mut rx = Rx { pstyles: plot_style_names(&secs), ..Rx::default() };
    let mut blocks: Vec<BlockDef> = Vec::new();
    let mut layout_objs: Vec<LayoutDef> = Vec::new();
    let mut entities = Vec::new();
    let mut objs = Objects::default();
    // Every handle another program's file already uses is taken before anything gets a new one:
    // its $HANDSEED can be lower than its handles, and entities made while reading (the faces of
    // a polygon mesh) would otherwise get handles that records further on still have. A file
    // CADCraft wrote names its own next handle, above which its rebuilt records are numbered
    // (#495), so it is left to that.
    let ours = secs.iter().any(|s| s.name == "OBJECTS" && s.tags.iter().any(|t| t.code == 3 && t.str() == crate::dxf_ext::HANDSEED_KEY));
    for t in secs.iter().filter(|_| !ours).flat_map(|s| s.tags.iter()).filter(|t| t.code == 5 || t.code == 105) {
        if let Some(h) = Handle::parse_hex(t.str().trim()) {
            d.bump_handseed(h);
        }
    }
    let mut skipped = std::collections::BTreeMap::new();
    count_skipped(&mut skipped, "(no section)", outside_sections(&tags));
    for s in &secs {
        match s.name.as_str() {
            "HEADER" => header(&s.tags, &mut d),
            "TABLES" => tables(&s.tags, &mut d, &mut rx),
            "BLOCKS" => blocks.extend(read_blocks(&s.tags, &mut d, &mut rx, &mut table_cells_left, &mut skipped)),
            // A damaged file may split its entities over several ENTITIES sections (#865).
            "ENTITIES" => {
                let mut n = 0;
                entities.extend(parse_entities(&records(&s.tags), &mut d, &mut rx, &mut n, &mut table_cells_left));
                count_skipped(&mut skipped, "ENTITIES", n);
            }
            "OBJECTS" => read_objects(&s.tags, &mut objs, &mut layout_objs),
            _ => {}
        }
    }
    // A file CADCraft wrote names its own next handle (#495): its table records and objects
    // are numbered above it and rebuilt on save, so following $HANDSEED would drift every save.
    let own_seed = objs.xrecord_text(crate::dxf_ext::HANDSEED_KEY).and_then(|t| Handle::parse_hex(t.trim()));
    let (paper_block_for, first_layout) = setup_layouts(&mut d, &mut layout_objs, &rx);
    place_blocks(&mut d, blocks, &mut rx, &paper_block_for, &first_layout);
    place_entities(&mut d, entities, &rx, &paper_block_for, &first_layout);
    number_unknown_viewport_ids(&mut d, &dropped_paper_views(&rx, &paper_block_for, &first_layout));
    apply_sortents(&mut d, &objs.sortents, &rx.brs, &paper_block_for, &first_layout);
    objs.apply(&mut d, &rx);
    crate::dxf_image::resolve(&secs, &mut d);
    crate::dxf_mline::resolve(&secs, &mut d);
    // Keep handles unique against the header's seed.
    let seed = d.header.str("HANDSEED", "");
    if let Some(h) = own_seed.or_else(|| Handle::parse_hex(&seed)) {
        d.bump_handseed(Handle(h.0.saturating_sub(1)));
    }
    if d.layer("0").is_none() {
        d.layers.insert(0, Layer::default());
    }
    crate::dxf_raw::read(&secs, &mut d);
    crate::dxf_clip::read(&mut d);
    d.skipped_records = skipped;
    Ok(d)
}

/// Viewport ids another program left unknown (ZWCAD writes 69 = -1, read as 0). A layout whose
/// viewports all lack an id gets them numbered in file order, as DWG readers do; when only
/// some lack one and none is 1, the first viewport is the sheet and gets id 1. Either way the
/// sheet keeps id 1, so saving does not add a second sheet viewport ahead of it (#1463). A layout
/// whose generated sheet (PAPERVIEW) the reader just dropped is left alone: none of its viewports
/// is the sheet, and numbering one as 1 would lose the generated sheet on the next save.
fn number_unknown_viewport_ids(d: &mut Drawing, sheet_dropped: &std::collections::HashSet<String>) {
    for l in d.layouts.iter_mut().filter(|l| !sheet_dropped.contains(&l.name)) {
        let mut vps: Vec<Entity> = l.entities.iter().filter(|e| matches!(&e.kind, EntityKind::Viewport(_))).map(|e| (**e).clone()).collect();
        let id = |e: &Entity| if let EntityKind::Viewport(v) = &e.kind { v.id } else { 0 };
        if vps.iter().all(|e| id(e) == 0) {
            for (i, e) in vps.iter_mut().enumerate() {
                if let EntityKind::Viewport(v) = &mut e.kind {
                    v.id = i as u32 + 1;
                }
            }
        } else if vps.iter().all(|e| id(e) != 1) && vps.first().is_some_and(|e| id(e) == 0) {
            if let Some(EntityKind::Viewport(v)) = vps.first_mut().map(|e| &mut e.kind) {
                v.id = 1;
            }
        } else {
            continue;
        }
        for e in vps {
            l.entities.replace(e);
        }
    }
}

/// An entity from the groups of a DXF record of type `kind` (without its `0` group), as the
/// reader makes it, with plain strings as [`crate::entity_groups`] gives them (not the file's
/// `^ ` escapes). AutoLISP's `entmake` and `entmod` are built on it.
pub fn entity_from_groups(kind: &str, tags: &[Tag]) -> Option<(Common, EntityKind)> {
    // AutoLISP's strings are plain: escape them as the file has them (see `entity_groups`).
    let mut tags = tags.to_vec();
    mtext_dxf::map_text_groups(kind, &mut tags, mtext_dxf::encode_line);
    entity(&kind.to_ascii_uppercase(), &tags)
}

#[cfg(test)]
mod table_budget_tests {
    use super::*;
    /// Build synthetic table tags with a requested row and column count for allocation-budget checks.
    fn table_tags(rows: i64, cols: i64) -> Vec<Tag> {
        vec![Tag::s(100, "AcDbTable"), Tag::i(91, rows), Tag::i(92, cols), Tag::i(171, 1), Tag::s(1, "cell")]
    }
    /// A table that cannot fit allocates no cell grid, while a smaller table spends only its cell count.
    #[test]
    fn exhausted_budget_rejects_before_materializing_cells() {
        let mut left = 3;
        let table = acad_table(&table_tags(2, 2), &mut left);
        assert!(table.cells.is_empty() && table.row_heights.is_empty() && table.col_widths.is_empty());
        assert_eq!(left, 3);
        let table = acad_table(&table_tags(1, 2), &mut left);
        assert_eq!(table.cells[0].len(), 2);
        assert_eq!(left, 1);
    }
    /// Tables in blocks and repeated entity sections consume one document-wide cell allowance.
    #[test]
    fn blocks_and_entity_sections_share_one_budget() {
        let table = "0\nACAD_TABLE\n100\nAcDbTable\n91\n1\n92\n2\n171\n1\n1\ncell\n";
        let dxf = format!(
            "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nTEST\n10\n0\n20\n0\n{table}0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{table}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{table}0\nENDSEC\n0\nEOF\n"
        );
        let d = read_budgeted(dxf.as_bytes(), 2).unwrap();
        let block = d.block("TEST").unwrap();
        let count = |e: &Entity| match &e.kind {
            EntityKind::Table(t) => t.cells.iter().map(Vec::len).sum::<usize>(),
            _ => 0,
        };
        assert_eq!(block.entities.iter().map(|e| count(e)).sum::<usize>(), 2);
        assert_eq!(d.model.iter().map(|e| count(e)).sum::<usize>(), 0);
    }
}
