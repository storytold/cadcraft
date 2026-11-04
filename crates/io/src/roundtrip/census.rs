//! What a DXF file holds, counted from its groups, and the same counts of a drawing read from it:
//! together they show what opening a file leaves out (the DXF rendition of a DWG for DWG files).

use std::collections::{BTreeMap, HashSet};

use cadcraft_doc::Drawing;
use serde::Serialize;

use super::{Category, Change, Comparison, Options, Out};

/// Counts of what a file (or a drawing) holds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Census {
    /// Entities of model space and every layout, per DXF type.
    pub entities: BTreeMap<String, usize>,
    /// Entities of block definitions (other than the model and paper space blocks), per type.
    pub block_entities: BTreeMap<String, usize>,
    /// Block definitions other than the model and paper space blocks and the generated ones.
    pub blocks: usize,
    /// Blocks CADCraft generates and so doesn't read (tables' blocks, standard arrowheads
    /// nothing inserts); their entities are not counted.
    pub generated_blocks: usize,
    /// Table records per table type (LAYER, LTYPE, STYLE, DIMSTYLE, VIEW, UCS…).
    pub tables: BTreeMap<String, usize>,
    /// Non-graphical objects per type. For a drawing: only those kept without being modelled.
    pub objects: BTreeMap<String, usize>,
    /// CLASS records. For a drawing: only those kept for unmodelled objects.
    pub classes: usize,
}

/// Records that belong to the record before them (or delimit blocks), not entities of their own.
const SUB_RECORDS: &[&str] = &["VERTEX", "SEQEND", "ATTRIB", "BLOCK", "ENDBLK"];

fn bump(m: &mut BTreeMap<String, usize>, k: &str) {
    let n = m.entry(k.to_string()).or_insert(0);
    *n = n.saturating_add(1);
}

/// A layout's own paper-space viewport (VIEWPORT with ID 1, group 69): part of the layout, not
/// an entity of its own for CADCraft.
fn is_overall_viewport(t: &str, g: &[cadcraft_dxf::Tag]) -> bool {
    t == "VIEWPORT" && g.iter().any(|x| x.code == 69 && x.i64() == 1)
}

fn is_space_block(name: &str) -> bool {
    let up = name.trim().to_ascii_uppercase();
    up.starts_with("*MODEL_SPACE") || up.starts_with("*PAPER_SPACE")
}

/// Count what a DXF file (ASCII or binary) holds; `None` when it can't be parsed.
pub fn census(dxf: &[u8]) -> Option<Census> {
    let tags = cadcraft_dxf::parse(dxf).ok()?;
    let mut c = Census::default();
    // Blocks CADCraft generates (tables' blocks, standard arrowheads nothing inserts) are left out
    // on read and rebuilt on save: counted apart.
    let mut table_blocks: HashSet<String> = HashSet::new();
    let mut inserted: HashSet<String> = HashSet::new();
    let mut blocks: Vec<(String, BTreeMap<String, usize>)> = Vec::new();
    let mut note_refs = |t: &str, g: &[cadcraft_dxf::Tag]| {
        let name = || g.iter().find(|x| x.code == 2).map(|x| x.str().trim().to_ascii_uppercase());
        match t {
            "ACAD_TABLE" => table_blocks.extend(name()),
            "INSERT" => inserted.extend(name()),
            _ => {}
        }
    };
    for s in cadcraft_dxf::sections(&tags) {
        let records = cadcraft_dxf::records(&s.tags);
        match s.name.trim().to_ascii_uppercase().as_str() {
            "ENTITIES" => {
                for (t, g) in &records {
                    note_refs(t, g);
                    if !SUB_RECORDS.contains(&t.as_str()) && !is_overall_viewport(t, g) {
                        bump(&mut c.entities, t);
                    }
                }
            }
            "BLOCKS" => {
                let mut space = false;
                for (t, g) in &records {
                    note_refs(t, g);
                    match t.as_str() {
                        "BLOCK" => {
                            let name = g.iter().find(|x| x.code == 2).map(cadcraft_dxf::Tag::str).unwrap_or_default();
                            space = is_space_block(&name);
                            if !space {
                                blocks.push((name.trim().to_ascii_uppercase(), BTreeMap::new()));
                            }
                        }
                        t if SUB_RECORDS.contains(&t) || is_overall_viewport(t, g) => {}
                        t if space => bump(&mut c.entities, t),
                        t => {
                            if let Some((_, m)) = blocks.last_mut() {
                                bump(m, t);
                            }
                        }
                    }
                }
            }
            "TABLES" => {
                for (t, _) in &records {
                    if t != "TABLE" && t != "ENDTAB" {
                        bump(&mut c.tables, t);
                    }
                }
            }
            "OBJECTS" => {
                for (t, _) in &records {
                    bump(&mut c.objects, t);
                }
            }
            "CLASSES" => c.classes = c.classes.saturating_add(records.iter().filter(|(t, _)| t == "CLASS").count()),
            _ => {}
        }
    }
    for (name, m) in blocks {
        if table_blocks.contains(&name) || (crate::dxf_ext::is_standard_arrow(&name) && !inserted.contains(&name)) {
            c.generated_blocks = c.generated_blocks.saturating_add(1);
            continue;
        }
        c.blocks = c.blocks.saturating_add(1);
        for (t, n) in m {
            let slot = c.block_entities.entry(t).or_insert(0);
            *slot = slot.saturating_add(n);
        }
    }
    Some(c)
}

/// The same counts of a drawing: its entities, blocks and table records, and the objects and
/// classes it keeps without modelling them.
pub fn census_of(d: &Drawing) -> Census {
    let mut c = Census::default();
    for e in d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())) {
        bump(&mut c.entities, e.kind.dxf_name());
    }
    for b in d.blocks.values() {
        for e in b.entities.iter() {
            bump(&mut c.block_entities, e.kind.dxf_name());
        }
    }
    c.blocks = d.blocks.len();
    for (t, n) in [
        ("LAYER", d.layers.len()),
        ("LTYPE", d.linetypes.len()),
        ("STYLE", d.text_styles.len()),
        ("DIMSTYLE", d.dim_styles.len()),
        ("VIEW", d.views.len()),
        ("UCS", d.ucss.len()),
    ] {
        c.tables.insert(t.to_string(), n);
    }
    for o in &d.foreign.objects {
        bump(&mut c.objects, &o.kind);
    }
    c.classes = d.foreign.classes.len();
    c
}

/// Polylines may change between the old-style POLYLINE and LWPOLYLINE when read: counted
/// together.
fn merged(m: &BTreeMap<String, usize>) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for (k, n) in m {
        let key = if k == "POLYLINE" || k == "LWPOLYLINE" { "POLYLINE+LWPOLYLINE" } else { k.as_str() };
        let slot = out.entry(key.to_string()).or_insert(0usize);
        *slot = slot.saturating_add(*n);
    }
    out
}

fn compare_counts(out: &mut Out, place: &str, file: &BTreeMap<String, usize>, read: &BTreeMap<String, usize>) {
    let (file, read) = (merged(file), merged(read));
    let keys: std::collections::BTreeSet<&String> = file.keys().chain(read.keys()).collect();
    for k in keys {
        let (f, r) = (file.get(k).copied().unwrap_or(0), read.get(k).copied().unwrap_or(0));
        if r < f {
            out.push_n(Category::Read, Change::Lost, f - r, format!("{place} {k}"), format!("{k}: {f} in the file, {r} read"));
        } else if r > f {
            out.push_n(Category::Read, Change::Added, r - f, format!("{place} {k}"), format!("{k}: {f} in the file, {r} read"));
        }
    }
}

/// What a drawing read from a file holds less (or more) than the file: entities per type in the
/// spaces and in blocks, blocks, and the table records CADCraft models. Blocks CADCraft generates
/// (tables' blocks, unused standard arrowheads) are left out on both sides.
pub fn compare_census(file: &Census, d: &Drawing, opts: &Options) -> Comparison {
    let read = census_of(d);
    let mut out = Out::new(opts);
    compare_counts(&mut out, "spaces", &file.entities, &read.entities);
    compare_counts(&mut out, "blocks", &file.block_entities, &read.block_entities);
    if file.blocks != read.blocks {
        let change = if read.blocks < file.blocks { Change::Lost } else { Change::Added };
        out.push_n(
            Category::Read,
            change,
            file.blocks.abs_diff(read.blocks),
            "BLOCK definitions",
            format!("{} in the file, {} read", file.blocks, read.blocks),
        );
    }
    for t in ["LAYER", "LTYPE", "STYLE", "DIMSTYLE", "VIEW", "UCS"] {
        let (f, r) = (file.tables.get(t).copied().unwrap_or(0), read.tables.get(t).copied().unwrap_or(0));
        if f != r {
            let change = if r < f { Change::Lost } else { Change::Added };
            out.push_n(Category::Read, change, f.abs_diff(r), format!("{t} table"), format!("{f} records in the file, {r} read"));
        }
    }
    out.cmp
}
