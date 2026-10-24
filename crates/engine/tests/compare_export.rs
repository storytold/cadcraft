//! COMPAREEXPORT: the comparison as a new drawing, with this drawing's objects in one group and
//! the other drawing's differing objects in another, in the comparison colors.

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::doc::color::{Color, Rgb};
use cadcraft_engine::doc::geom::Vec3;
use cadcraft_engine::doc::{Block, CellStyle, Common, Drawing, Entity, EntityKind, Handle, Insert, Layer, Space};
use serde_json::json;

fn insert(block: &str) -> Insert {
    Insert {
        block: block.into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    }
}

fn line(x: f64) -> EntityKind {
    EntityKind::Line(cadcraft_engine::doc::Line { a: Vec3::ZERO, b: Vec3::new(x, 0.0, 0.0) })
}

fn handle_of(s: &Session, ty: &str) -> String {
    s.doc().unwrap().model.iter().find(|e| e.kind.type_name() == ty).unwrap().handle.hex()
}

/// "Drawing1" holds a line, a circle and a text; "B" is a copy where the circle moved, the text
/// is gone and an arc was added. The comparison runs in "B" against "Drawing1".
fn compared() -> Session {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    s.execute("circle", &json!({"center": [5, 5], "radius": 2})).unwrap();
    s.execute("text", &json!({"at": [0, 10], "height": 1, "text": "Note"})).unwrap();
    let d: Drawing = s.doc().unwrap().clone();
    s.open_drawing(d, "B.dxf", None);
    let circle = handle_of(&s, "Circle");
    let text = handle_of(&s, "Text");
    s.execute("move", &json!({"handles": [circle], "from": [0, 0], "to": [3, 0]})).unwrap();
    s.execute("erase", &json!({"handles": [text]})).unwrap();
    s.execute("arc", &json!({"center": [20, 0], "radius": 1, "start": 0, "end": 90})).unwrap();
    s.execute("compare", &json!({"document": "Drawing1"})).unwrap();
    s
}

/// The objects of a group of the active drawing, in group order.
fn members(s: &Session, group: &str) -> Vec<Arc<Entity>> {
    let d = s.doc().unwrap();
    let g = d.groups.iter().find(|g| g.name == group).unwrap_or_else(|| panic!("no group {group}"));
    g.members.iter().map(|h| d.entity(*h).unwrap().clone()).collect()
}

/// The (type, color) of each object in a group of the active drawing, sorted.
fn contents(s: &Session, group: &str) -> Vec<(String, Color)> {
    let mut v: Vec<(String, Color)> = members(s, group).iter().map(|e| (e.kind.type_name().to_string(), e.common.color)).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// The x of each line's end in a block of the active drawing.
fn ends(s: &Session, block: &str) -> Vec<f64> {
    let d = s.doc().unwrap();
    d.block(block)
        .unwrap()
        .entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => Some(l.b.x),
            _ => None,
        })
        .collect()
}

const GREEN: Color = Color::True(Rgb(0x4c, 0xd9, 0x7b));
const RED: Color = Color::True(Rgb(0xff, 0x5c, 0x5c));
const GRAY: Color = Color::Index(8);

#[test]
fn exports_both_drawings_as_colored_groups_in_a_new_drawing() {
    let mut s = compared();
    let before = s.docs.len();
    let source = s.doc().unwrap().clone();
    let r = s.execute("compareexport", &json!({})).unwrap();
    assert_eq!(s.docs.len(), before + 1, "{r}");
    assert_eq!(r["title"], "compare_B vs Drawing1.dwg", "{r}");
    assert_eq!(s.state().unwrap().title, "compare_B vs Drawing1.dwg");
    assert_eq!(s.state().unwrap().path, None, "the snapshot is untitled");
    assert!(s.state().unwrap().is_dirty(), "closing the snapshot asks to save it");
    assert_eq!((r["counts"]["current"].as_u64(), r["counts"]["compared"].as_u64()), (Some(3), Some(2)), "{r}");
    assert_eq!(r["truncated"], false);
    let (cur, other) = (r["current"].as_str().unwrap(), r["compared"].as_str().unwrap());
    assert_eq!((cur, other), ("COMPARE_CURRENT", "COMPARE_COMPARED"));
    // This drawing: the moved circle and the new arc in green, the unchanged line in gray.
    assert_eq!(contents(&s, cur), vec![("Arc".into(), GREEN), ("Circle".into(), GREEN), ("Line".into(), GRAY)]);
    // The other drawing: the circle where it was and the erased text, in red, in its draw order.
    assert_eq!(contents(&s, other), vec![("Circle".into(), RED), ("Text".into(), RED)]);
    let order: Vec<&str> = members(&s, other).iter().map(|e| e.kind.type_name()).collect();
    assert_eq!(order, ["Circle", "Text"]);
    let circle_there = members(&s, other).iter().find_map(|e| match &e.kind {
        EntityKind::Circle(c) => Some(c.center),
        _ => None,
    });
    assert_eq!(circle_there.map(|c| (c.x, c.y)), Some((5.0, 5.0)));
    // Model space holds the other drawing's objects first, then this drawing's.
    let d = s.doc().unwrap();
    let order: Vec<Handle> = d.model.iter().map(|e| e.handle).collect();
    let groups: Vec<Handle> = members(&s, other).iter().chain(members(&s, cur).iter()).map(|e| e.handle).collect();
    assert_eq!(order, groups);
    // Every handle is new and unique.
    let mut seen = std::collections::HashSet::new();
    for e in d.model.iter().chain(d.blocks.values().flat_map(|b| b.entities.iter())) {
        assert!(seen.insert(e.handle), "handle {} repeats", e.handle.hex());
    }
    // The drawing COMPARE ran in is unchanged, and the comparison stays on it.
    let back = s.docs.iter().position(|d| d.title == "B.dxf").unwrap();
    assert!(*s.docs[back].doc == source);
    assert!(s.compare.is_some());
}

#[test]
fn takes_the_other_drawings_layers_and_blocks_it_needs() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    // The other drawing holds a reference to a block this one lacks, on a layer this one lacks.
    let mut other = s.doc().unwrap().clone();
    other.handseed = 0x9000;
    let mut part = Block::new("PART");
    part.entities.push(Entity::new(Handle(0x8000), EntityKind::Insert(insert("BOLT"))));
    let mut bolt = Block::new("BOLT");
    bolt.entities.push(Entity::new(Handle(0x8001), line(1.0)));
    other.blocks.insert("PART".into(), Arc::new(part));
    other.blocks.insert("BOLT".into(), Arc::new(bolt));
    other.layers.push(Layer::new("Parts"));
    other.add(&Space::Model, Common { layer: "Parts".into(), ..Common::default() }, EntityKind::Insert(insert("PART"))).unwrap();
    // A group of this drawing's name does not carry over, so the snapshot takes the first name.
    s.execute("group", &json!({"name": "COMPARE_CURRENT", "handles": [handle_of(&s, "Line")]})).unwrap();
    s.open_drawing(other, "Other", None);
    s.execute("document.switch", &json!({"index": 0})).unwrap();
    s.execute("compare", &json!({"document": "Other"})).unwrap();
    let r = s.execute("compareexport", &json!({})).unwrap();
    assert_eq!(r["current"], "COMPARE_CURRENT", "{r}");
    let d = s.doc().unwrap();
    assert_eq!(d.groups.len(), 2);
    assert!(d.layer("Parts").is_some());
    assert!(d.block("PART").is_some() && d.block("BOLT").is_some(), "nested blocks are copied");
    assert_eq!(ends(&s, "COMPARE_COMPARED_BOLT"), vec![1.0], "and shown through red copies");
    assert!(d.blocks.values().flat_map(|b| b.entities.iter()).all(|e| e.handle.0 < 0x8000 || e.handle.0 >= 0x9000));
    let refs: Vec<String> = members(&s, "COMPARE_COMPARED")
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Insert(i) => Some(format!("{} on {}", i.block, e.common.layer)),
            _ => None,
        })
        .collect();
    assert_eq!(refs, vec!["COMPARE_COMPARED_PART on Parts".to_string()]);
}

#[test]
fn anonymous_blocks_of_the_other_drawing_keep_their_own_definition() {
    let mut s = Session::new();
    let mut mine = Block::new("*U1");
    mine.entities.push(Entity::new(Handle(0x700), line(1.0)));
    s.doc_mut().unwrap().blocks.insert("*U1".into(), Arc::new(mine));
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), EntityKind::Insert(insert("*U1"))).unwrap();
    // The other drawing uses the same anonymous name for other geometry.
    let mut other = Drawing::new_imperial();
    other.handseed = 0x9000;
    let mut theirs = Block::new("*U1");
    theirs.entities.push(Entity::new(Handle(0x8000), line(5.0)));
    other.blocks.insert("*U1".into(), Arc::new(theirs));
    other.add(&Space::Model, Common::default(), EntityKind::Insert(Insert { insert: Vec3::new(9.0, 9.0, 0.0), ..insert("*U1") })).unwrap();
    s.open_drawing(other, "Other", None);
    s.execute("document.switch", &json!({"index": 0})).unwrap();
    s.execute("compare", &json!({"document": "Other"})).unwrap();
    let r = s.execute("compareexport", &json!({})).unwrap();
    assert_eq!(ends(&s, "*U1"), vec![1.0], "this drawing's definition is kept");
    let referenced: Vec<String> = members(&s, r["compared"].as_str().unwrap())
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Insert(i) => Some(i.block.clone()),
            _ => None,
        })
        .collect();
    // The other drawing's `*U1` is copied as `*U2`, and the reference shows it through a red copy.
    assert_eq!(ends(&s, "*U2"), vec![5.0]);
    assert_eq!(referenced, vec!["COMPARE_COMPARED_U2".to_string()], "{r}");
    assert_eq!(ends(&s, "COMPARE_COMPARED_U2"), vec![5.0]);
    assert_eq!(ends(&s, "COMPARE_CURRENT_U1"), vec![1.0]);
}

#[test]
fn block_contents_take_the_comparison_color() {
    let mut s = Session::new();
    let mut part = Block::new("PART");
    part.entities.push(Entity { handle: Handle(0x700), common: Common { color: Color::Index(1), ..Common::default() }, kind: line(1.0) });
    part.entities.push(Entity::new(Handle(0x701), EntityKind::Insert(insert("PART"))));
    s.doc_mut().unwrap().blocks.insert("PART".into(), Arc::new(part));
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), EntityKind::Insert(insert("PART"))).unwrap();
    let mut other = s.doc().unwrap().clone();
    other.model = Default::default();
    s.open_drawing(other, "Other", None);
    s.execute("document.switch", &json!({"index": 0})).unwrap();
    s.execute("compare", &json!({"document": "Other"})).unwrap();
    let r = s.execute("compareexport", &json!({})).unwrap();
    let objs = members(&s, r["current"].as_str().unwrap());
    let EntityKind::Insert(i) = &objs[0].kind else { panic!("{r}") };
    assert_eq!(i.block, "COMPARE_CURRENT_PART");
    let d = s.doc().unwrap();
    let copy = d.block("COMPARE_CURRENT_PART").unwrap();
    assert!(copy.entities.iter().all(|e| e.common.color == GREEN), "the line inside is green too");
    // The block that refers to itself refers to its own copy, which ends the copying.
    assert!(copy.entities.iter().any(|e| matches!(&e.kind, EntityKind::Insert(n) if n.block == "COMPARE_CURRENT_PART")));
    // This drawing's definition is left as it was.
    assert_eq!(d.block("PART").unwrap().entities.iter().next().unwrap().common.color, Color::Index(1));
}

#[test]
fn colors_carried_apart_from_the_object_color_are_replaced() {
    let mut s = Session::new();
    s.execute("mtext", &json!({"at": [0, 0], "height": 1, "width": 20, "text": "a{\\C1;red} \\c16711680;b \\\\C"})).unwrap();
    s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [10, 0], "at": [5, 3]})).unwrap();
    s.execute("table", &json!({"at": [0, 20], "rows": 1, "cols": 1, "cells": [["\\C1;x"]]})).unwrap();
    s.execute("tolerance", &json!({"at": [0, 30], "symbol": "flatness", "tol1": "0.1"})).unwrap();
    s.execute("mline", &json!({"points": [[0, 40], [10, 40]]})).unwrap();
    let d = s.doc_mut().unwrap();
    d.dim_styles[0].dim_line_color = Color::Index(1);
    d.dim_styles[0].text_color = Color::Index(5);
    let mut data = d.table_styles[0].cell_style(CellStyle::DATA);
    (data.text_color, data.fill, data.borders.top.color) = (Color::Index(1), Some(Color::Index(2)), Color::Index(3));
    d.table_styles[0].cell_styles.push(data);
    let mline = d.model.iter().find(|e| matches!(e.kind, EntityKind::MLine(_))).unwrap().handle;
    d.modify_entity(mline, |e| {
        if let EntityKind::MLine(m) = &mut e.kind {
            m.def.elements.iter_mut().for_each(|el| el.color = Color::Index(3));
            m.def.fill = Some(Color::Index(4));
        }
    })
    .unwrap();
    let other = Drawing::new_imperial();
    s.open_drawing(other, "Other", None);
    s.execute("document.switch", &json!({"index": 0})).unwrap();
    s.execute("compare", &json!({"document": "Other"})).unwrap();
    let r = s.execute("compareexport", &json!({})).unwrap();
    let mut kinds = Vec::new();
    let d = s.doc().unwrap();
    for e in members(&s, r["current"].as_str().unwrap()) {
        match &e.kind {
            EntityKind::MText(t) => assert_eq!(t.contents, "a{red} b \\\\C", "inline colors are taken out"),
            EntityKind::Dimension(dm) => {
                let st = d.dim_style(&dm.style).unwrap().with_overrides(&dm.overrides);
                assert_eq!((st.dim_line_color, st.ext_line_color, st.text_color), (Color::ByBlock, Color::ByBlock, Color::ByBlock));
            }
            EntityKind::Table(t) => {
                assert_eq!(t.style, "COMPARE_CURRENT_Standard");
                assert_eq!(t.cells.iter().flatten().find(|c| !c.text.is_empty()).map(|c| c.text.as_str()), Some("x"));
                let st = d.table_styles.iter().find(|s| s.name == t.style).unwrap();
                let data = st.cell_style(CellStyle::DATA);
                assert_eq!((data.text_color, data.fill, data.borders.top.color), (Color::ByBlock, None, Color::ByBlock));
            }
            EntityKind::Tolerance(t) => {
                assert_eq!(t.style, "COMPARE_CURRENT_Standard");
                let st = d.dim_style(&t.style).unwrap();
                assert_eq!((st.dim_line_color, st.text_color), (Color::ByBlock, Color::ByBlock));
            }
            EntityKind::MLine(m) => {
                assert!(m.def.elements.iter().all(|el| el.color == Color::ByBlock));
                assert_eq!(m.def.fill, None);
            }
            _ => continue,
        }
        kinds.push(e.kind.type_name());
    }
    assert_eq!(kinds.len(), 5, "{kinds:?}");
    // This drawing's styles are left as they were.
    assert_eq!(d.table_styles[0].cell_style(CellStyle::DATA).text_color, Color::Index(1));
    assert_eq!(d.dim_style("Standard").unwrap().dim_line_color, Color::Index(1));
}

#[test]
fn edits_after_compare_count_and_objects_keep_their_layers() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let copy: Drawing = s.doc().unwrap().clone();
    s.open_drawing(copy, "Other", None);
    s.execute("document.switch", &json!({"index": 0})).unwrap();
    s.execute("compare", &json!({"document": "Other"})).unwrap();
    // A line drawn after COMPARE ran is only in this drawing.
    s.execute("line", &json!({"points": [[0, 5], [10, 5]]})).unwrap();
    let r = s.execute("compareexport", &json!({})).unwrap();
    let mut got = contents(&s, r["current"].as_str().unwrap());
    got.sort_by_key(|(_, c)| *c == GRAY);
    assert_eq!(got, vec![("Line".into(), GREEN), ("Line".into(), GRAY)], "{r}");
    // The copies sit in model space on their own layer, not inside a block.
    let d = s.doc().unwrap();
    assert!(d.model.iter().all(|e| e.common.layer == "0" && matches!(e.kind, EntityKind::Line(_))));
}

#[test]
fn copies_drop_associations_to_the_source_objects() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let l = handle_of(&s, "Line");
    s.execute("dimlinear", &json!({"object": l, "at": [5, 3]})).unwrap();
    let assoc = |d: &Drawing| d.model.iter().find_map(|e| if let EntityKind::Dimension(dm) = &e.kind { Some(dm.assoc.len()) } else { None });
    assert!(assoc(s.doc().unwrap()).is_some_and(|n| n > 0), "the dimension follows the line");
    s.open_drawing(Drawing::new_imperial(), "Other", None);
    s.execute("document.switch", &json!({"index": 0})).unwrap();
    s.execute("compare", &json!({"document": "Other"})).unwrap();
    s.execute("compareexport", &json!({})).unwrap();
    assert_eq!(assoc(s.doc().unwrap()), Some(0));
}

#[test]
fn relative_xref_paths_become_paths_from_the_source_folder() {
    let folder = std::env::temp_dir().join("cadcraft-compareexport");
    let host = folder.join("host.dxf");
    let mut d = Drawing::new_imperial();
    let mut x = Block::new("X");
    x.xref_path = Some("parts/x.dwg".into());
    d.blocks.insert("X".into(), Arc::new(x));
    d.add(&Space::Model, Common::default(), EntityKind::Insert(insert("X"))).unwrap();
    let image = cadcraft_engine::doc::Image { path: "img/a.png".into(), ..Default::default() };
    d.add(&Space::Model, Common::default(), EntityKind::Image(image.clone())).unwrap();
    let mut b = Block::new("PIC");
    b.entities.push(Entity::new(Handle(0x900), EntityKind::Image(image)));
    d.blocks.insert("PIC".into(), Arc::new(b));
    let mut s = Session::new();
    s.open_drawing(d, "host.dxf", Some(host.to_string_lossy().to_string()));
    s.open_drawing(Drawing::new_imperial(), "Other", None);
    s.execute("document.switch", &json!({"index": 1})).unwrap();
    s.execute("compare", &json!({"document": "Other"})).unwrap();
    s.execute("compareexport", &json!({})).unwrap();
    let d = s.doc().unwrap();
    let got = d.block("X").unwrap().xref_path.clone().unwrap();
    assert_eq!(std::path::Path::new(&got), folder.join("parts").join("x.dwg"));
    // Images in model space and in blocks too.
    let want = folder.join("img").join("a.png");
    let images = d.model.iter().chain(d.block("PIC").unwrap().entities.iter()).filter_map(|e| match &e.kind {
        EntityKind::Image(i) => Some(i.path.clone()),
        _ => None,
    });
    let images: Vec<String> = images.collect();
    assert_eq!(images.len(), 2);
    assert!(images.iter().all(|p| std::path::Path::new(p) == want), "{images:?}");
}

#[test]
fn relative_xref_paths_of_a_compared_file_take_that_files_folder() {
    // Test file format: any bytes read as a drawing with a reference to xref Y at `y.dwg`.
    fn read(_bytes: &[u8], _name: &str) -> Result<Drawing, String> {
        let mut d = Drawing::new_imperial();
        let mut y = Block::new("Y");
        y.xref_path = Some("y.dwg".into());
        d.blocks.insert("Y".into(), Arc::new(y));
        d.add(&Space::Model, Common::default(), EntityKind::Insert(insert("Y"))).map_err(|e| e.to_string())?;
        Ok(d)
    }
    cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks { read, write: |_, _| Err("no writer in this test".into()), plot: None });
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("compareexport-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("other.dxf");
    std::fs::write(&path, "x").unwrap();
    // Named by its absolute path and by a path relative to the working directory.
    let mut names = vec![path.to_string_lossy().to_string()];
    names.extend(relative_to_cwd(&path));
    for name in names {
        let mut s = Session::new();
        s.execute("compare", &json!({"path": name})).unwrap();
        // The compared file's xref is only there; the copy keeps the file's folder.
        s.execute("compareexport", &json!({})).unwrap();
        let got = s.doc().unwrap().block("Y").unwrap().xref_path.clone().unwrap();
        let got = std::path::Path::new(&got);
        assert!(got.is_absolute(), "{got:?}, compared as {name}");
        assert_eq!(got.file_name(), Some(std::ffi::OsStr::new("y.dwg")));
        assert_eq!(got.parent().unwrap().canonicalize().unwrap(), dir.canonicalize().unwrap(), "compared as {name}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// `p` as a path relative to the working directory (`..` steps up to the shared folder), when
/// the two share a root.
fn relative_to_cwd(p: &std::path::Path) -> Option<String> {
    use std::path::Component;
    let cwd = std::env::current_dir().ok()?;
    let (a, b): (Vec<Component>, Vec<Component>) = (cwd.components().collect(), p.components().collect());
    let shared = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if shared == 0 {
        return None;
    }
    let mut rel = std::path::PathBuf::new();
    a[shared..].iter().for_each(|_| rel.push(".."));
    b[shared..].iter().for_each(|c| rel.push(c));
    Some(rel.to_string_lossy().to_string())
}

#[test]
fn says_when_blocks_of_the_other_drawing_were_left_out() {
    let mut other = Drawing::new_imperial();
    for i in 0..10_001 {
        let name = format!("B{i}");
        let mut b = Block::new(&name);
        b.entities.push(Entity::new(Handle(0x10_0000 + i), line(1.0)));
        other.blocks.insert(name.clone(), Arc::new(b));
        other.add(&Space::Model, Common::default(), EntityKind::Insert(insert(&name))).unwrap();
    }
    other.handseed = other.handseed.max(0x20_0000);
    let mut s = Session::new();
    s.open_drawing(other, "Other", None);
    s.execute("document.switch", &json!({"index": 0})).unwrap();
    s.execute("compare", &json!({"document": "Other"})).unwrap();
    let r = s.execute("compareexport", &json!({})).unwrap();
    assert_eq!(r["truncated"], true, "{}", r["message"]);
    assert!(r["message"].as_str().unwrap().contains("some blocks of the other drawing were too many or too large to copy"), "{}", r["message"]);
    // The same blocks in this drawing, compared with an empty one: past the limit of recolored
    // copies some references keep their blocks' own colors.
    let ours = s.docs[1].doc.as_ref().clone();
    let mut s = Session::new();
    s.open_drawing(ours, "Ours", None);
    s.execute("compare", &json!({"document": 0})).unwrap();
    let r = s.execute("compareexport", &json!({})).unwrap();
    assert_eq!(r["truncated"], true, "{}", r["message"]);
    assert!(r["message"].as_str().unwrap().contains("some blocks keep their own colors"), "{}", r["message"]);
}

#[test]
fn needs_a_comparison_of_the_current_drawing() {
    let mut s = Session::new();
    assert!(s.execute("compareexport", &json!({})).is_err(), "no comparison yet");
    let mut s = compared();
    s.execute("document.switch", &json!({"index": 0})).unwrap();
    assert!(s.execute("compareexport", &json!({})).is_err(), "the comparison belongs to the other drawing");
    s.execute("document.switch", &json!({"index": 1})).unwrap();
    s.execute("compareclose", &json!({})).unwrap();
    assert!(s.execute("compareexport", &json!({})).is_err(), "closed");
    // Typed at the command line it runs too.
    let mut s = compared();
    let before = s.docs.len();
    s.cmdline("compareexport").unwrap();
    assert_eq!(s.docs.len(), before + 1);
}
