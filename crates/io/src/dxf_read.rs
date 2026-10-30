//! DXF → Drawing.

use std::collections::HashMap;

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{
    Arc, AssocSnap, Attrib, AttribModes, AttribProps, Block, Circle, Common, DashElement, DimAssoc, DimKind, DimStyle, Dimension, Drawing, Ellipse,
    Entity, EntityKind, Face3d, Gradient, Group, HAlign, HVal, Handle, Hatch, HatchLoop, Helix, HelixConstrain, Insert, Layer, Layout, Leader, Line,
    Linetype, Lineweight, LwPolyline, MAX_FRAME_CHARS, MLeaderStyle, MText, MultilineAttrib, NamedView, PageSetup, Point, Polyline3d, RawTag,
    RayLine, Solid, Table, TableCell, Text, TextStyle, Tolerance, Transparency, Ucs, Unknown, VAlign, Viewport, VportConfig, VportTile, Wipeout,
};
use cadcraft_dxf::{Tag, records, sections};
use cadcraft_geom::{PolyVertex, Spline, Vec2, Vec3};

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
    apply_sortents, count_skipped, outside_sections, parse_entities, place_blocks, place_entities, read_blocks, read_objects, setup_layouts,
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
    /// LAYER record handle (upper case) → layer name (VIEWPORT group 341 frozen layers).
    layers: HashMap<String, String>,
}

/// Read a DXF file into a drawing.
pub fn read(bytes: &[u8]) -> Result<Drawing> {
    read_budgeted(bytes, 4_000_000)
}

/// A block definition read from BLOCKS: name, base point, description, entities (owner, paper
/// flag, entity) and block flags.
type BlockDef = (String, Vec3, String, Vec<(Option<String>, bool, Entity)>, i64);

/// A layout read from OBJECTS: name, tab order, paper block handle and page setup.
type LayoutDef = (String, u32, String, PageSetup);

/// Import DXF sections while sharing one materialized-table-cell budget across blocks and entity sections.
fn read_budgeted(bytes: &[u8], mut table_cells_left: usize) -> Result<Drawing> {
    let tags = cadcraft_dxf::parse(bytes).map_err(|e| IoError::Format(e.to_string()))?;
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
    // a polyface mesh) would otherwise get handles that records further on still have. A file
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
    d.skipped_records = skipped;
    Ok(d)
}

/// An entity from the groups of a DXF record of type `kind` (without its `0` group), as the
/// reader makes it. AutoLISP's `entmake` and `entmod` are built on it.
pub fn entity_from_groups(kind: &str, tags: &[Tag]) -> Option<(Common, EntityKind)> {
    entity(&kind.to_ascii_uppercase(), tags)
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
