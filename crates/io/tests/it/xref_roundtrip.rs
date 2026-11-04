//! External references in DXF (#525): an xref is saved as a BLOCK flagged 4 (and 8 for an
//! overlay) with its path and no objects, and the symbols it brought (`xref|name`) are not saved
//! as the drawing's own; loading them again is the engine's job when the drawing opens. Its layers
//! are saved, as xref-dependent records with the host's settings (#886).

use std::sync::Arc;

use cadcraft_color::Color;
use cadcraft_doc::*;
use cadcraft_dxf::{Tag, parse_ascii, records, sections, write_ascii};
use cadcraft_geom::Vec3;
use cadcraft_io::{read_dxf, write_dxf};

fn roundtrip(d: &Drawing) -> Drawing {
    read_dxf(write_dxf(d).as_bytes()).expect("reopen")
}

fn line(layer: &str) -> Entity {
    let mut e = Entity::new(Handle(0x900), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) }));
    e.common.layer = layer.into();
    e
}

/// A host with a loaded attached xref SITE and an unloaded overlay PLAN.
fn host() -> Drawing {
    let mut d = Drawing::new_metric();
    let mut site = Block::new("SITE");
    site.xref_path = Some("..\\shared\\site.dwg".into());
    site.xref_status = XrefStatus::Loaded;
    site.entities.push(line("SITE|Walls"));
    d.blocks.insert("SITE".into(), Arc::new(site));
    let mut plan = Block::new("PLAN");
    plan.xref_path = Some("plan.dxf".into());
    plan.xref_overlay = true;
    d.blocks.insert("PLAN".into(), Arc::new(plan));
    let mut tree = Block::new("SITE|Tree");
    tree.entities.push(line("0"));
    d.blocks.insert("SITE|Tree".into(), Arc::new(tree));
    d.layers.push(Layer { name: "SITE|Walls".into(), color: Color::Index(1), ..Layer::default() });
    d.linetypes.push(Linetype { name: "SITE|Dashed".into(), ..Linetype::continuous() });
    d.text_styles.push(TextStyle { name: "SITE|Notes".into(), ..TextStyle::default() });
    d.dim_styles.push(DimStyle { name: "SITE|Arch".into(), ..DimStyle::default() });
    d.layers.push(Layer { name: "Doors".into(), ..Layer::default() });
    for (i, name) in ["SITE", "PLAN"].into_iter().enumerate() {
        let ins = Insert {
            block: name.into(),
            insert: Vec3::new(i as f64 * 10.0, 0.0, 0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        };
        d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).expect("add");
    }
    d
}

#[test]
fn xrefs_are_saved_as_references_without_their_objects() {
    let r = roundtrip(&host());
    let site = r.block("SITE").expect("xref kept");
    assert_eq!(site.xref_path.as_deref(), Some("..\\shared\\site.dwg"), "the path as the user gave it");
    assert!(!site.xref_overlay);
    assert!(site.entities.is_empty(), "its objects come from the referenced file");
    let plan = r.block("PLAN").expect("overlay kept");
    assert_eq!((plan.xref_path.as_deref(), plan.xref_overlay), (Some("plan.dxf"), true));
    assert_eq!(r.model.iter().filter(|e| matches!(e.kind, EntityKind::Insert(_))).count(), 2);
}

#[test]
fn xref_dependent_symbols_are_not_saved() {
    let r = roundtrip(&host());
    assert!(r.blocks.keys().all(|n| !n.contains('|')), "{:?}", r.blocks.keys().collect::<Vec<_>>());
    assert!(r.linetypes.iter().all(|l| !l.name.contains('|')));
    assert!(r.text_styles.iter().all(|t| !t.name.contains('|')));
    assert!(r.dim_styles.iter().all(|t| !t.name.contains('|')));
    assert!(r.layer("Doors").is_some(), "the drawing's own symbols are");
}

#[test]
fn xref_layers_are_saved_as_dependent_layer_records() {
    // #886: an xref's layers keep the host's settings (AutoCAD's VISRETAIN = 1). DXF Reference,
    // LAYER group 70: 16 = externally dependent on an xref, 32 = with 16, the xref is resolved.
    let mut d = host();
    let walls = d.layer_mut("SITE|Walls").expect("walls");
    walls.color = Color::Index(252);
    walls.frozen = true;
    let text = write_dxf(&d);
    let tags = parse_ascii(&text).expect("parse");
    let layers: Vec<(String, i64)> = sections(&tags)
        .iter()
        .filter(|s| s.name == "TABLES")
        .flat_map(|s| records(&s.tags))
        .filter(|(k, _)| k == "LAYER")
        .map(|(_, t)| {
            let get = |c: i32| t.iter().find(|x| x.code == c);
            (get(2).map(Tag::str).unwrap_or_default(), get(70).map(Tag::i64).unwrap_or(0))
        })
        .collect();
    let flags = |n: &str| layers.iter().find(|l| l.0 == n).map(|l| l.1);
    assert_eq!(flags("SITE|Walls"), Some(1 | 16 | 32), "{layers:?}");
    assert_eq!(flags("Doors"), Some(0), "the drawing's own layers aren't dependent");
    let r = read_dxf(text.as_bytes()).expect("reopen");
    let walls = r.layer("SITE|Walls").expect("saved");
    assert_eq!((walls.color, walls.frozen), (Color::Index(252), true));
}

/// The BLOCK records of a written file as (name, flags, path).
fn block_records(text: &str) -> Vec<(String, i64, String)> {
    let tags = parse_ascii(text).expect("parse");
    sections(&tags)
        .iter()
        .filter(|s| s.name == "BLOCKS")
        .flat_map(|s| records(&s.tags))
        .filter(|(k, _)| k == "BLOCK")
        .map(|(_, t)| {
            let get = |c: i32| t.iter().find(|x| x.code == c);
            (get(2).map(Tag::str).unwrap_or_default(), get(70).map(Tag::i64).unwrap_or(0), get(1).map(Tag::str).unwrap_or_default())
        })
        .collect()
}

#[test]
fn the_written_block_records_carry_the_xref_flags() {
    let recs = block_records(&write_dxf(&host()));
    let rec = |n: &str| recs.iter().find(|r| r.0 == n).map(|r| (r.1 & 12, r.2.clone()));
    assert_eq!(rec("SITE"), Some((4, "..\\shared\\site.dwg".to_string())));
    assert_eq!(rec("PLAN"), Some((12, "plan.dxf".to_string())));
    assert!(recs.iter().all(|r| !r.0.contains('|')));
}

#[test]
fn objects_in_a_read_xref_block_are_ignored() {
    // A file whose xref BLOCK holds objects: only its path counts.
    let mut d = host();
    let mut b = (**d.blocks.get("SITE").expect("site")).clone();
    b.xref_path = None;
    d.blocks.insert("SITE".into(), Arc::new(b));
    let mut tags = parse_ascii(&write_dxf(&d)).expect("parse");
    let (mut in_block, mut site) = (false, false);
    for t in tags.iter_mut() {
        match t.code {
            0 => (in_block, site) = (t.str() == "BLOCK", false),
            2 if in_block => site = t.str() == "SITE",
            70 if site => *t = Tag::i(70, t.i64() | 4),
            1 if site => *t = Tag::s(1, "site.dxf"),
            _ => {}
        }
    }
    let r = read_dxf(write_ascii(&tags).as_bytes()).expect("read");
    let site = r.block("SITE").expect("kept");
    assert_eq!(site.xref_path.as_deref(), Some("site.dxf"));
    assert!(site.entities.is_empty());
}
