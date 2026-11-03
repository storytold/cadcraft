use std::sync::Arc;

use cadcraft_color::Color;
use cadcraft_doc::foreign::{ForeignExtra, ForeignObject};
use cadcraft_doc::*;
use cadcraft_geom::{PolyVertex, Vec2, Vec3};

use super::*;

fn tag(code: i32, value: &str) -> RawTag {
    RawTag { code, value: value.into() }
}

fn line(a: (f64, f64), b: (f64, f64)) -> EntityKind {
    EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) })
}

/// A small drawing touching every part the comparator looks at.
fn drawing() -> Drawing {
    let mut d = Drawing::new_metric();
    d.layers.push(Layer { name: "Walls".into(), color: Color::Index(1), ..Layer::default() });
    let walls = Common { layer: "Walls".into(), ..Common::default() };
    d.add(&Space::Model, walls.clone(), line((0.0, 0.0), (10.0, 0.0))).unwrap();
    d.add(&Space::Model, walls, line((0.0, 5.0), (10.0, 5.0))).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::Circle(Circle { center: Vec3::new(3.0, 4.0, 0.0), radius: 2.5 })).unwrap();
    let tagged = Common { xdata: vec![tag(1001, "ACME"), tag(1000, "door"), tag(1040, "2.5")], ..Common::default() };
    d.add(
        &Space::Model,
        tagged,
        EntityKind::LwPolyline(LwPolyline {
            vertices: vec![PolyVertex::new(Vec2::ZERO), PolyVertex::with_bulge(Vec2::new(4.0, 0.0), 0.5), PolyVertex::new(Vec2::new(4.0, 3.0))],
            closed: true,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        }),
    )
    .unwrap();
    d.add(
        &Space::Model,
        Common::default(),
        EntityKind::Unknown(Unknown { dxf_type: "ACME_WIDGET".into(), tags: vec![tag(100, "AcDbEntity"), tag(10, "1.0"), tag(20, "2.0")] }),
    )
    .unwrap();
    d.add(&Space::Paper("Layout1".into()), Common::default(), line((1.0, 1.0), (2.0, 2.0))).unwrap();
    let mut b = Block::new("DOOR");
    let h = d.new_handle();
    b.entities.push(Entity::new(h, line((0.0, 0.0), (0.0, 1.0))));
    d.blocks.insert("DOOR".into(), Arc::new(b));
    let f = Arc::make_mut(&mut d.foreign);
    f.objects.push(ForeignObject { handle: Handle(0x500), kind: "XRECORD".into(), tags: vec![tag(100, "AcDbXrecord"), tag(1, "kept")] });
    f.dictionary.push(("ACME_DATA".into(), Handle(0x500)));
    f.classes.push(vec![tag(1, "ACME_WIDGET"), tag(2, "AcmeWidget"), tag(3, "Acme")]);
    f.records.insert(("LAYER".into(), "WALLS".into()), ForeignExtra { xdata: vec![tag(1001, "ACME"), tag(1070, "3")], ..ForeignExtra::default() });
    d
}

fn cmp(a: &Drawing, b: &Drawing) -> Comparison {
    compare(a, b, &Options::default())
}

fn only(c: &Comparison, cat: Category, change: Change) -> Vec<&Difference> {
    c.differences.iter().filter(|d| d.category == cat && d.change == change).collect()
}

fn first_handle(d: &Drawing, kind: &str) -> Handle {
    d.model.iter().find(|e| e.kind.dxf_name() == kind).map(|e| e.handle).unwrap()
}

#[test]
fn identical_drawings_have_no_differences() {
    let d = drawing();
    let c = cmp(&d, &d.clone());
    assert!(c.is_empty(), "{:#?}", c.differences);
    assert_eq!(c.total(), Tally::default());
}

#[test]
fn geometry_is_compared_by_handle_within_the_tolerance() {
    let a = drawing();
    let h = first_handle(&a, "LINE");
    let mut b = a.clone();
    b.modify_entity(h, |e| {
        if let EntityKind::Line(l) = &mut e.kind {
            l.b.x += 1e-9;
        }
    })
    .unwrap();
    assert!(cmp(&a, &b).is_empty(), "a change within the tolerance is no difference");
    b.modify_entity(h, |e| {
        if let EntityKind::Line(l) = &mut e.kind {
            l.b.x = 11.0;
        }
    })
    .unwrap();
    let c = cmp(&a, &b);
    let g = only(&c, Category::Geometry, Change::Changed);
    assert_eq!(g.len(), 1, "{:#?}", c.differences);
    assert!(g[0].place.contains(&h.hex()) && g[0].place.starts_with("Model LINE"), "{:?}", g[0]);
    assert!(g[0].detail.contains("b.x: 10") && g[0].detail.contains("11"), "{:?}", g[0]);
    assert_eq!(c.tally(Category::Geometry).changed, 1);
}

#[test]
fn angles_a_whole_turn_apart_are_the_same() {
    let mut a = drawing();
    let h = a
        .add(&Space::Model, Common::default(), EntityKind::Arc(cadcraft_doc::Arc { center: Vec3::ZERO, radius: 2.0, start: -0.5, end: 1.0 }))
        .unwrap();
    let mut b = a.clone();
    b.modify_entity(h, |e| {
        if let EntityKind::Arc(x) = &mut e.kind {
            x.start += std::f64::consts::TAU;
            x.end -= std::f64::consts::TAU;
        }
    })
    .unwrap();
    assert!(cmp(&a, &b).is_empty(), "{:#?}", cmp(&a, &b).differences);
    // A radius is no angle.
    b.modify_entity(h, |e| {
        if let EntityKind::Arc(x) = &mut e.kind {
            x.radius += std::f64::consts::TAU;
        }
    })
    .unwrap();
    assert_eq!(cmp(&a, &b).tally(Category::Geometry).changed, 1);
}

#[test]
fn properties_and_type_changes_are_reported() {
    let a = drawing();
    let h = first_handle(&a, "CIRCLE");
    let mut b = a.clone();
    b.modify_entity(h, |e| e.common.color = Color::Index(3)).unwrap();
    let c = cmp(&a, &b);
    assert_eq!(only(&c, Category::Properties, Change::Changed).len(), 1, "{:#?}", c.differences);
    b.modify_entity(h, |e| e.kind = line((0.0, 0.0), (1.0, 1.0))).unwrap();
    let c = cmp(&a, &b);
    let g = only(&c, Category::Geometry, Change::Changed);
    assert!(g.iter().any(|d| d.detail == "type CIRCLE → LINE"), "{:#?}", c.differences);
}

#[test]
fn lost_and_added_entities_are_counted_per_type_and_space() {
    let a = drawing();
    let h = first_handle(&a, "CIRCLE");
    let mut b = a.clone();
    b.remove_entity(h);
    let mut paper = b.clone();
    paper.add(&Space::Paper("Layout1".into()), Common::default(), EntityKind::Point(Point { p: Vec3::ZERO, angle: 0.0 })).unwrap();
    let c = cmp(&a, &paper);
    let lost = only(&c, Category::Count, Change::Lost);
    assert_eq!(lost.len(), 1, "{:#?}", c.differences);
    assert_eq!(lost[0].place, "Model CIRCLE");
    assert!(lost[0].detail.contains("1 → 0") && lost[0].detail.contains(&h.hex()), "{:?}", lost[0]);
    let added = only(&c, Category::Count, Change::Added);
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].place, "Layout Layout1 POINT");
    assert_eq!(c.tally(Category::Count), Tally { lost: 1, added: 1, changed: 0 });
}

#[test]
fn renumbered_entities_are_matched_by_content() {
    let a = drawing();
    let mut b = a.clone();
    // Every line gets a new handle, in reverse order, and one of them moves.
    let lines: Vec<Arc<Entity>> = b.model.iter().filter(|e| e.kind.dxf_name() == "LINE").cloned().collect();
    for (i, e) in lines.iter().rev().enumerate() {
        b.model.remove(e.handle);
        let mut e = Entity::clone(e);
        e.handle = Handle(0x9000 + i as u64);
        b.model.push(e);
    }
    let c = cmp(&a, &b);
    assert_eq!(c.tally(Category::Count), Tally::default(), "{:#?}", c.differences);
    assert_eq!(c.tally(Category::Geometry), Tally::default(), "pairs by content, not by order: {:#?}", c.differences);
    assert_eq!(c.tally(Category::Handle).changed, 2);
}

#[test]
fn entities_moving_between_spaces_are_reported() {
    let a = drawing();
    let h = first_handle(&a, "CIRCLE");
    let mut b = a.clone();
    let e = b.remove_entity(h).unwrap();
    if let Some(l) = b.layouts.first_mut() {
        l.entities.push(Entity::clone(&e));
    }
    let c = cmp(&a, &b);
    let moved = only(&c, Category::Handle, Change::Changed);
    assert_eq!(moved.len(), 1, "{:#?}", c.differences);
    assert_eq!(moved[0].detail, format!("1 entities moved from Model to Layout Layout1 ({})", h.hex()));
    assert_eq!(c.tally(Category::Count), Tally::default());
}

#[test]
fn unreferenced_anonymous_blocks_dropped_on_save_are_one_row() {
    let mut a = drawing();
    for name in ["*D5", "*D6"] {
        let mut blk = Block::new(name);
        for _ in 0..2 {
            let h = a.new_handle();
            blk.entities.push(Entity::new(h, line((0.0, 0.0), (1.0, 0.0))));
        }
        a.blocks.insert(name.into(), Arc::new(blk));
    }
    let mut b = a.clone();
    b.blocks.remove("*D5");
    b.blocks.remove("*D6");
    let c = cmp(&a, &b);
    let lost = only(&c, Category::Count, Change::Lost);
    assert_eq!(lost.len(), 1, "{:#?}", c.differences);
    assert_eq!(lost[0].detail, "4 entities of 2 anonymous blocks nothing refers to (e.g. *D5) were not saved");
    assert_eq!(c.tally(Category::Count).lost, 4);
    assert_eq!(c.tally(Category::Block), Tally { lost: 2, added: 0, changed: 0 }, "{:#?}", c.differences);
    assert_eq!(only(&c, Category::Block, Change::Lost).len(), 1);
}

#[test]
fn renamed_anonymous_blocks_are_matched_by_their_entities() {
    let mut a = drawing();
    let mut blk = Block::new("*D1");
    let h = a.new_handle();
    blk.entities.push(Entity::new(h, line((0.0, 0.0), (5.0, 0.0))));
    a.blocks.insert("*D1".into(), Arc::new(blk.clone()));
    let key = ("BLOCK_RECORD".to_string(), "*D1".to_string());
    Arc::make_mut(&mut a.foreign).records.insert(key, ForeignExtra { xdata: vec![tag(1001, "ACME"), tag(1070, "1")], ..ForeignExtra::default() });
    let mut b = a.clone();
    b.blocks.remove("*D1");
    blk.name = "*D7".into();
    b.blocks.insert("*D7".into(), Arc::new(blk));
    let f = Arc::make_mut(&mut b.foreign);
    let extra = f.records.remove(&("BLOCK_RECORD".to_string(), "*D1".to_string())).unwrap();
    f.records.insert(("BLOCK_RECORD".into(), "*D7".into()), extra);
    let c = cmp(&a, &b);
    assert_eq!(c.tally(Category::Count), Tally::default(), "{:#?}", c.differences);
    assert_eq!(c.tally(Category::Xdata), Tally::default(), "{:#?}", c.differences);
    assert_eq!(c.tally(Category::Block), Tally { lost: 0, added: 0, changed: 1 }, "{:#?}", c.differences);
    assert!(c.differences.iter().any(|d| d.detail == "1 anonymous blocks saved under new names (e.g. *D1 → *D7)"), "{:#?}", c.differences);
}

#[test]
fn xdata_extension_dictionaries_and_opaque_entities_are_compared() {
    let a = drawing();
    let pl = first_handle(&a, "LWPOLYLINE");
    let unknown = first_handle(&a, "ACME_WIDGET");
    let mut b = a.clone();
    b.modify_entity(pl, |e| e.common.xdata.clear()).unwrap();
    b.modify_entity(unknown, |e| {
        if let EntityKind::Unknown(u) = &mut e.kind {
            u.tags.pop();
        }
    })
    .unwrap();
    Arc::make_mut(&mut b.foreign).records.clear();
    let mut a2 = a.clone();
    Arc::make_mut(&mut a2.foreign)
        .entities
        .insert(pl, ForeignExtra { xdict: Some(Handle(0x600)), reactors: vec![Handle(0x601)], ..ForeignExtra::default() });
    let c = cmp(&a2, &b);
    let x = only(&c, Category::Xdata, Change::Lost);
    assert_eq!(x.len(), 2, "the entity's and the layer's: {:#?}", c.differences);
    assert!(x.iter().any(|d| d.place == "LAYER WALLS" && d.detail.starts_with("application ACME")), "{x:#?}");
    assert!(x.iter().any(|d| d.place.contains("LWPOLYLINE")), "{x:#?}");
    assert_eq!(c.tally(Category::ExtDict).lost, 2, "the dictionary and a reactor: {:#?}", c.differences);
    let o = only(&c, Category::Opaque, Change::Changed);
    assert_eq!(o.len(), 1, "{:#?}", c.differences);
    assert!(o[0].detail.contains("3 groups → 2"), "{:?}", o[0]);
    // Numbers in kept groups compare within the tolerance.
    let mut b2 = a.clone();
    b2.modify_entity(unknown, |e| {
        if let EntityKind::Unknown(u) = &mut e.kind {
            u.tags[1].value = "1.0000000000001".into();
        }
    })
    .unwrap();
    assert!(cmp(&a, &b2).is_empty());
}

#[test]
fn tables_blocks_and_layouts_are_compared_by_name() {
    let a = drawing();
    let mut b = a.clone();
    b.layers.retain(|l| l.name != "Walls");
    b.linetypes.push(Linetype { name: "Dashed".into(), description: String::new(), pattern: Vec::new() });
    if let Some(s) = b.text_styles.first_mut() {
        s.height = 7.0;
    }
    b.blocks.remove("DOOR");
    b.layouts.retain(|l| l.name != "Layout2");
    let c = cmp(&a, &b);
    let t = |cat, ch, place: &str| c.differences.iter().any(|d| d.category == cat && d.change == ch && d.place == place);
    assert!(t(Category::Table, Change::Lost, "LAYER Walls"), "{:#?}", c.differences);
    assert!(t(Category::Table, Change::Added, "LTYPE Dashed"));
    assert!(t(Category::Table, Change::Changed, "STYLE Standard"));
    assert!(t(Category::Block, Change::Lost, "BLOCK DOOR"));
    assert!(t(Category::Layout, Change::Lost, "LAYOUT Layout2"));
    // The block's entity is lost with it, counted where it was.
    assert!(t(Category::Count, Change::Lost, "Block DOOR LINE"), "{:#?}", c.differences);
    // Names match without case.
    let mut upper = a.clone();
    if let Some(l) = upper.layers.iter_mut().find(|l| l.name == "Walls") {
        l.name = "WALLS".into();
    }
    let c = cmp(&a, &upper);
    assert_eq!(c.tally(Category::Table).lost, 0, "{:#?}", c.differences);
}

#[test]
fn unmodelled_objects_dictionary_entries_and_classes_are_compared() {
    let a = drawing();
    let mut b = a.clone();
    let f = Arc::make_mut(&mut b.foreign);
    f.objects.clear();
    f.dictionary.clear();
    f.classes.clear();
    let c = cmp(&a, &b);
    assert_eq!(c.tally(Category::Object).lost, 1, "{:#?}", c.differences);
    assert_eq!(c.tally(Category::Dictionary).lost, 1);
    assert_eq!(c.tally(Category::Class).lost, 1);
    // A renumbered object is matched by its groups.
    let mut b = a.clone();
    Arc::make_mut(&mut b.foreign).objects[0].handle = Handle(0x700);
    let c = cmp(&a, &b);
    assert_eq!(c.tally(Category::Object), Tally::default(), "{:#?}", c.differences);
    assert_eq!(c.tally(Category::Handle).changed, 1);
}

#[test]
fn header_variables_are_compared_without_volatile_ones() {
    let a = drawing();
    let mut b = a.clone();
    b.header.set_f64("LTSCALE", 2.0);
    b.header.vars.remove("TEXTSIZE");
    b.header.set_str("HANDSEED", "FFFF");
    b.header.set_str("TDUPDATE", "2461000.5");
    // 1 and 1.0 are the same value.
    b.header.set_i64("DIMSCALE", 1);
    let c = cmp(&a, &b);
    assert!(only(&c, Category::Header, Change::Changed).iter().any(|d| d.place == "$LTSCALE"), "{:#?}", c.differences);
    assert!(only(&c, Category::Header, Change::Lost).iter().any(|d| d.place == "$TEXTSIZE"));
    assert_eq!(c.tally(Category::Header).total(), 2, "{:#?}", c.differences);
}

#[test]
fn the_listing_limit_keeps_counting() {
    let a = drawing();
    let mut b = a.clone();
    for i in 0..20 {
        b.header.set_i64(&format!("USERI{i}"), i);
    }
    let c = compare(&a, &b, &Options { max_differences: 5, ..Options::default() });
    assert_eq!(c.differences.len(), 5);
    assert_eq!(c.omitted, 15);
    assert_eq!(c.tally(Category::Header).added, 20);
}

#[test]
fn census_counts_what_a_dxf_holds() {
    let dxf = "  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n  0\nLAYER\n  2\n0\n  0\nLAYER\n  2\nA\n  0\nENDTAB\n  0\nENDSEC\n\
               0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\n*Model_Space\n  0\nENDBLK\n  0\nBLOCK\n  2\n*Paper_Space\n  0\nLINE\n  0\nENDBLK\n\
               0\nBLOCK\n  2\nB1\n  0\nCIRCLE\n  0\nPOLYLINE\n  0\nVERTEX\n  0\nSEQEND\n  0\nENDBLK\n  0\nENDSEC\n\
               0\nSECTION\n  2\nENTITIES\n  0\nLINE\n  0\nINSERT\n  0\nATTRIB\n  0\nSEQEND\n  0\nENDSEC\n\
               0\nSECTION\n  2\nOBJECTS\n  0\nDICTIONARY\n  0\nXRECORD\n  0\nXRECORD\n  0\nENDSEC\n  0\nEOF\n";
    let c = census(dxf.as_bytes()).unwrap();
    assert_eq!(c.entities.get("LINE"), Some(&2));
    assert_eq!(c.entities.get("INSERT"), Some(&1));
    assert!(!c.entities.contains_key("ATTRIB") && !c.entities.contains_key("SEQEND"));
    assert_eq!(c.block_entities.get("CIRCLE"), Some(&1));
    assert_eq!(c.block_entities.get("POLYLINE"), Some(&1));
    assert_eq!(c.blocks, 1);
    assert_eq!(c.tables.get("LAYER"), Some(&2));
    assert_eq!(c.objects.get("XRECORD"), Some(&2));
    let _ = census(b"");
}

#[test]
fn read_census_reports_entities_the_reader_left_out() {
    let d = drawing();
    let mut file = census_of(&d);
    *file.entities.entry("LINE".into()).or_default() += 2;
    *file.entities.entry("WEIRD".into()).or_default() += 1;
    let c = compare_census(&file, &d, &Options::default());
    let lost = only(&c, Category::Read, Change::Lost);
    assert_eq!(c.tally(Category::Read).lost, 3, "{:#?}", c.differences);
    assert!(lost.iter().any(|x| x.place == "spaces LINE" && x.detail == "LINE: 5 in the file, 3 read"), "{lost:#?}");
    assert!(compare_census(&census_of(&d), &d, &Options::default()).is_empty());
}

/// The round trips of a synthetic drawing through CADCraft's own DXF and DWG writers: whatever
/// they lose shows in the comparison. Plain geometry, layers, xdata and unmodelled objects must
/// come back from DXF.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dxf_round_trip_of_plain_geometry_is_lossless() {
    use corpus::{Format, round_trip};
    let mut d = drawing();
    // What the comparator should see in a round trip: the drawing as read from its own DXF.
    d = crate::read(crate::write(&d, "a.dxf").unwrap().as_slice(), "a.dxf").unwrap();
    let r = round_trip(&d, Format::Dxf, Format::Dxf, &Options::default(), None);
    assert!(r.ok(), "{r:#?}");
    let core = [Category::Count, Category::Geometry, Category::Table, Category::Xdata, Category::Object, Category::Opaque];
    for cat in core {
        assert_eq!(r.comparison.tally(cat), Tally::default(), "{cat:?}: {:#?}", r.comparison.differences);
    }
    let dwg = round_trip(&d, Format::Dxf, Format::Dwg, &Options::default(), None);
    assert!(dwg.save_error.is_none() && dwg.reopen_error.is_none(), "{dwg:#?}");
    assert!(dwg.saved_bytes > 0);
    // Through DWG, only the custom-class entity is lost (gap FF1: acadrust drops other programs'
    // entities without DWG data of their own) — and the comparator says so.
    let lost: Vec<&str> = dwg.comparison.differences.iter().filter(|d| d.category == Category::Count).map(|d| d.place.as_str()).collect();
    assert_eq!(lost, ["Model ACME_WIDGET"], "{:#?}", dwg.comparison.differences);
    assert_eq!(dwg.comparison.tally(Category::Geometry), Tally::default(), "{:#?}", dwg.comparison.differences);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn hostile_files_are_reported_not_crashed() {
    use corpus::{CorpusOptions, check_bytes};
    let opts = CorpusOptions::default();
    for (bytes, name) in [
        (&b""[..], "empty.dxf"),
        (b"AC1032 not really a dwg file at all", "fake.dwg"),
        (b"  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n 10\nnan\n 20\n1e400\n  0\nENDSEC\n  0\nEOF\n", "odd.dxf"),
        (&[0xFFu8; 64][..], "noise.dxf"),
    ] {
        let f = check_bytes(bytes, name, &opts);
        assert_eq!(f.path, name);
        if f.opened() {
            assert!(!f.routes.is_empty());
        } else {
            assert!(f.routes.is_empty() && f.open_error.as_deref().is_some_and(|e| !e.is_empty()), "{f:#?}");
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn report_renders_as_markdown_and_json() {
    use corpus::{CorpusOptions, CorpusReport, check_bytes};
    let d = drawing();
    let dxf = crate::write(&d, "a.dxf").unwrap();
    let opts = CorpusOptions::default();
    let report = CorpusReport {
        corpus: "corpus".into(),
        generated: "2026-10-21 09:30 UTC".into(),
        cadcraft_version: "0".into(),
        abs_tol: 1e-6,
        rel_tol: 1e-9,
        files: vec![check_bytes(&dxf, "synthetic/a.dxf", &opts), check_bytes(b"AC1032 broken", "dwg/broken.dwg", &opts)],
    };
    let md = report.to_markdown();
    assert!(md.starts_with("# CADCraft round-trip report"), "{md}");
    assert!(md.contains("| `synthetic/a.dxf` | DXF AC1032 |"), "{md}");
    assert!(md.contains("dxf → dxf") && md.contains("dxf → dwg"), "{md}");
    assert!(md.contains("| `dwg/broken.dwg` | DWG AC1032 |") && md.contains("**failed**"), "{md}");
    assert!(md.contains("## By category"));
    let json: serde_json::Value = serde_json::from_str(&report.to_json()).unwrap();
    assert_eq!(json["files"][0]["path"], "synthetic/a.dxf");
    assert_eq!(json["files"][0]["routes"][0]["route"], "dxf → dxf");
    assert!(json["files"][1]["openError"].as_str().is_some_and(|e| !e.is_empty()), "{json}");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn corpus_files_walks_folders_and_skips_the_report() {
    use corpus::corpus_files;
    let dir = std::env::temp_dir().join(format!("cadcraft-corpus-walk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for p in ["dxf/a.DXF", "dwg/b.dwg", "dwg/sub/c.dwg", "notes.txt", "report/a.roundtrip.dxf", ".git/x.dxf"] {
        let path = dir.join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"x").unwrap();
    }
    let files = corpus_files(&dir, &[dir.join("report")]);
    let rel: Vec<String> = files.iter().map(|f| corpus::relative(f, &dir)).collect();
    assert_eq!(rel, ["dwg/b.dwg", "dwg/sub/c.dwg", "dxf/a.DXF"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The real-file corpus: set `CADCRAFT_CORPUS_DIR` to a folder of DXF/DWG files (a checkout of
/// `storytold/cadcraft-corpus`) to open, save and reopen each one and write `report.md` and
/// `report.json` to `CADCRAFT_CORPUS_REPORT` (default: the corpus folder's `report`). Skipped
/// when the variable is unset, so CI passes without the corpus. It reports, it doesn't fail on
/// differences; set `CADCRAFT_CORPUS_STRICT=1` to fail when a file doesn't open, save or reopen,
/// or when a writer leaves records out of a saved file ([`corpus::CorpusReport::strict_failures`]).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn corpus_round_trip_report() {
    let Some(dir) = std::env::var_os("CADCRAFT_CORPUS_DIR").map(std::path::PathBuf::from) else {
        eprintln!("CADCRAFT_CORPUS_DIR is not set: corpus round trip skipped");
        return;
    };
    let out = std::env::var_os("CADCRAFT_CORPUS_REPORT").map_or_else(|| dir.join("report"), std::path::PathBuf::from);
    let files = corpus::corpus_files(&dir, std::slice::from_ref(&out));
    let report = corpus::run(&dir, &files, &corpus::CorpusOptions::default(), &mut |i, n, rel| eprintln!("[{}/{n}] {rel}", i + 1));
    let (md, json) = report.write(&out).unwrap();
    eprintln!("corpus report: {} and {}", md.display(), json.display());
    if std::env::var("CADCRAFT_CORPUS_STRICT").is_ok_and(|v| v == "1") {
        let failed = report.strict_failures();
        assert!(failed.is_empty(), "corpus failures:\n{}", failed.join("\n"));
    }
}

/// A type the DWG writer can't write is reported as lost on save, per type: in the route's
/// `not_written`, on the lost entity rows, in the Markdown and as a strict failure. A type it
/// writes (OLE2FRAME, kept by CADCraft without modelling it) is none of these.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn types_the_dwg_writer_cannot_write_are_reported_lost() {
    use corpus::{CorpusReport, FileReport, Format, round_trip};
    let unknown = |t: &str, tags: Vec<RawTag>| EntityKind::Unknown(Unknown { dxf_type: t.into(), tags });
    let mut d = Drawing::new_metric();
    let widget = vec![tag(100, "AcDbEntity"), tag(100, "AcmeWidget"), tag(10, "1.0"), tag(20, "2.0"), tag(30, "0.0")];
    d.add(&Space::Model, Common::default(), unknown("ACME_WIDGET", widget.clone())).unwrap();
    d.add(&Space::Model, Common::default(), unknown("ACME_WIDGET", widget)).unwrap();
    // Its data has no DWG wrapper, which is where a DWG keeps the corners: the reader's defaults.
    let ole = ["100", "AcDbEntity", "8", "0", "100", "AcDbOle2Frame", "70", "2", "10", "1.0", "20", "1.0", "30", "0.0", "11", "0.0", "21", "0.0"]
        .chunks(2)
        .map(|g| tag(g[0].parse().unwrap(), g[1]))
        .chain(
            ["31", "0.0", "71", "2", "72", "0", "73", "0", "90", "4", "310", "C0FFEE00", "1", "OLE"]
                .chunks(2)
                .map(|g| tag(g[0].parse().unwrap(), g[1])),
        )
        .collect();
    d.add(&Space::Model, Common::default(), unknown("OLE2FRAME", ole)).unwrap();
    let d = crate::read(&crate::write(&d, "a.dxf").unwrap(), "a.dxf").unwrap();

    let r = round_trip(&d, Format::Dxf, Format::Dwg, &Options::default(), None);
    assert!(r.ok(), "{r:#?}");
    assert_eq!(r.not_written, [("ACME_WIDGET".to_string(), 2)].into_iter().collect());
    let lost: Vec<&Difference> = r.comparison.differences.iter().filter(|x| x.category == Category::Count && x.change == Change::Lost).collect();
    assert_eq!(lost.len(), 1, "{lost:#?}");
    assert!(lost[0].place.ends_with("ACME_WIDGET") && lost[0].detail.contains("not written: the DWG writer can't write ACME_WIDGET"), "{lost:#?}");
    // The OLE2FRAME is written (only the CLASS record AutoCAD's files have is added).
    assert!(
        !r.comparison.differences.iter().any(|x| x.category != Category::Class && x.place.contains("OLE2FRAME")),
        "{:#?}",
        r.comparison.differences
    );
    assert!(!r.lossless());

    let report =
        CorpusReport { files: vec![FileReport { path: "a.dxf".into(), routes: vec![r], ..FileReport::default() }], ..CorpusReport::default() };
    assert_eq!(report.strict_failures(), ["a.dxf (dxf → dwg): lost on save, not written: 2 ACME_WIDGET"]);
    let md = report.to_markdown();
    assert!(md.contains("- **Lost on save** (records a writer can't write): 2 ACME_WIDGET (dxf → dwg)"), "{md}");
    assert!(md.contains("| dxf → dwg | ok | ok | **2** |"), "{md}");
    assert!(md.contains("- **Lost on save**, not written (the writer can't write these types): 2 ACME_WIDGET"), "{md}");

    // DXF keeps everything: nothing lost on save, nothing to fail on.
    let r = round_trip(&d, Format::Dxf, Format::Dxf, &Options::default(), None);
    assert!(r.not_written.is_empty());
    let report =
        CorpusReport { files: vec![FileReport { path: "a.dxf".into(), routes: vec![r], ..FileReport::default() }], ..CorpusReport::default() };
    assert!(report.strict_failures().is_empty());
    assert!(report.to_markdown().contains("- Lost on save (records a writer can't write): none"));
}
