//! COMPARE: differences between the drawing and another one, matched by geometry and by handle;
//! next/previous areas; COMPARECLOSE (#563).

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::geom::Vec3;
use cadcraft_engine::doc::{Block, Common, Drawing, Entity, EntityKind, Insert, Line, Space};
use serde_json::{Value, json};

fn line(ax: f64, ay: f64, bx: f64, by: f64) -> EntityKind {
    EntityKind::Line(Line { a: Vec3::new(ax, ay, 0.0), b: Vec3::new(bx, by, 0.0) })
}

/// A drawing with a line, a circle and a text; returns the session (the drawing is "Drawing1").
fn base() -> Session {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    s.execute("circle", &json!({"center": [5, 5], "radius": 2})).unwrap();
    s.execute("text", &json!({"at": [0, 10], "height": 1, "text": "Note"})).unwrap();
    s
}

fn handle_of(s: &Session, ty: &str) -> String {
    s.doc().unwrap().model.iter().find(|e| e.kind.type_name() == ty).unwrap().handle.hex()
}

fn counts(r: &Value) -> (u64, u64, u64, u64) {
    let c = &r["counts"];
    (c["added"].as_u64().unwrap(), c["removed"].as_u64().unwrap(), c["changed"].as_u64().unwrap(), c["common"].as_u64().unwrap())
}

/// Open a copy of the drawing as "B" and edit it: the circle moves, the text goes, an arc comes.
fn edited_copy(s: &mut Session) {
    let d: Drawing = s.doc().unwrap().clone();
    s.open_drawing(d, "B", None);
    let circle = handle_of(s, "Circle");
    let text = handle_of(s, "Text");
    s.execute("move", &json!({"handles": [circle], "from": [0, 0], "to": [3, 0]})).unwrap();
    s.execute("erase", &json!({"handles": [text]})).unwrap();
    s.execute("arc", &json!({"center": [20, 0], "radius": 1, "start": 0, "end": 90})).unwrap();
}

#[test]
fn finds_added_removed_and_changed_objects() {
    let mut s = base();
    edited_copy(&mut s);
    let r = s.execute("compare", &json!({"document": "drawing1"})).unwrap();
    assert_eq!(counts(&r), (1, 1, 1, 1), "{r}");
    assert_eq!(r["added"][0]["type"], "Arc", "{r}");
    assert_eq!(r["removed"][0]["type"], "Text", "{r}");
    assert_eq!((r["changed"][0]["type"].as_str(), r["changed"][0]["what"].as_str()), (Some("Circle"), Some("geometry")), "{r}");
    assert_eq!(r["message"], "Compared with Drawing1: 1 only in this drawing, 1 only in Drawing1, 1 changed, 1 the same.");
    let c = s.compare.as_ref().unwrap();
    assert_eq!((c.added.len(), c.removed.len(), c.changed.len(), c.space.clone()), (1, 1, 1, Space::Model));
    assert!(!c.regions.is_empty());
}

#[test]
fn matches_objects_by_geometry_when_handles_differ() {
    let mut s = base();
    // The same objects written by another program: every handle is new.
    let mut d = Drawing::new_imperial();
    d.handseed = 0x5000;
    for e in s.doc().unwrap().model.iter() {
        d.add(&Space::Model, e.common.clone(), e.kind.clone()).unwrap();
    }
    d.add(&Space::Model, Common::default(), line(0.0, 0.0, 0.0, 10.0)).unwrap();
    let theirs: Vec<_> = d.model.iter().map(|e| e.handle).collect();
    assert!(s.doc().unwrap().model.iter().all(|e| !theirs.contains(&e.handle)), "no handle in common");
    s.open_drawing(d, "Other", None);
    let r = s.execute("compare", &json!({"document": 0})).unwrap();
    assert_eq!(counts(&r), (1, 0, 0, 3), "only the extra line differs: {r}");
    // A colour change on the same handle is a property change.
    let mut s = base();
    let d: Drawing = s.doc().unwrap().clone();
    s.open_drawing(d, "B", None);
    let l = handle_of(&s, "Line");
    s.execute("properties.set", &json!({"handles": [l], "color": "red"})).unwrap();
    let r = s.execute("compare", &json!({"document": 0})).unwrap();
    assert_eq!(counts(&r), (0, 0, 1, 2), "{r}");
    assert_eq!(r["changed"][0]["what"], "properties", "{r}");
}

#[test]
fn navigates_areas_and_closes() {
    let mut s = base();
    edited_copy(&mut s);
    assert!(s.execute("compare.next", &json!({})).is_err(), "no comparison yet");
    s.execute("compare", &json!({"document": "Drawing1"})).unwrap();
    let n = s.compare.as_ref().unwrap().regions.len() as u64;
    let before = s.state().unwrap().view();
    let r = s.execute("compare.next", &json!({})).unwrap();
    assert_eq!((r["region"].as_u64(), r["of"].as_u64()), (Some(0), Some(n)), "{r}");
    assert_ne!(s.state().unwrap().view(), before, "the view zoomed to the area");
    let r = s.execute("compare.previous", &json!({})).unwrap();
    assert_eq!(r["region"].as_u64(), Some(n - 1), "previous wraps around: {r}");
    // The comparison belongs to the drawing it was made in.
    s.execute("document.switch", &json!({"index": 0})).unwrap();
    assert!(s.execute("compare.next", &json!({})).is_err());
    s.execute("document.switch", &json!({"index": 1})).unwrap();
    assert_eq!(s.execute("compareclose", &json!({})).unwrap()["closed"], true);
    assert!(s.compare.is_none());
}

/// The file-reading hook both tests here share (it is set once per process). Test file format:
/// "xref" is a drawing with a reference to xref Y at `y.dwg`; a number x is a drawing holding a
/// line from 0,0 to x,0.
fn hooks() {
    fn read(bytes: &[u8], _name: &str) -> Result<Drawing, String> {
        let mut d = Drawing::new_imperial();
        if bytes == b"xref" {
            let mut y = Block::new("Y");
            y.xref_path = Some("y.dwg".into());
            d.blocks.insert("Y".into(), Arc::new(y));
            let insert = Insert {
                block: "Y".into(),
                insert: Vec3::ZERO,
                scale: Vec3::new(1.0, 1.0, 1.0),
                rotation: 0.0,
                attribs: Vec::new(),
                cols: 1,
                rows: 1,
                col_spacing: 0.0,
                row_spacing: 0.0,
            };
            d.add(&Space::Model, Common::default(), EntityKind::Insert(insert)).map_err(|e| e.to_string())?;
            return Ok(d);
        }
        let x: f64 = String::from_utf8_lossy(bytes).trim().parse().map_err(|_| "bad test file")?;
        d.add(&Space::Model, Common::default(), line(0.0, 0.0, x, 0.0)).map_err(|e| e.to_string())?;
        Ok(d)
    }
    set_io(IoHooks { read, write: |_, _| Err("no writer in this test".into()), plot: None });
}

#[test]
fn compares_a_file_read_through_the_io_hooks() {
    hooks();
    let dir = std::env::temp_dir().join(format!("cadcraft-compare-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("other.dxf");
    std::fs::write(&path, "10").unwrap();
    let mut s = base();
    let r = s.execute("compare", &json!({"path": path.to_string_lossy()})).unwrap();
    assert_eq!(counts(&r), (2, 0, 0, 1), "the circle and the text are only here: {r}");
    // The same through the command line.
    s.cmdline("compare").unwrap();
    s.cmdline(&path.to_string_lossy()).unwrap();
    assert!(s.log.last().unwrap().starts_with("Compared with other.dxf: 2 only in this drawing"), "{:?}", s.log.last());
    let r = s.execute("compare", &json!({"data": "MTA=", "name": "ten.dxf"})).unwrap();
    assert_eq!(r["other"], "ten.dxf");
    assert!(s.execute("compare", &json!({"path": dir.join("missing.dxf").to_string_lossy()})).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn hostile_inputs_are_errors() {
    let mut s = base();
    for p in
        [json!({}), json!({"data": "%%%"}), json!({"document": 99}), json!({"document": "nope"}), json!({"document": 0}), json!({"document": true})]
    {
        assert!(s.execute("compare", &p).is_err(), "{p}");
    }
    // Self-referencing blocks in both drawings: signatures stay bounded.
    let mut b = Block::new("LOOP");
    let ins = Insert {
        block: "LOOP".into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    b.entities.push(Entity::new(cadcraft_engine::doc::Handle(0x900), EntityKind::Insert(ins.clone())));
    let d = s.doc_mut().unwrap();
    d.blocks.insert("LOOP".into(), Arc::new(b));
    d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
    let copy: Drawing = s.doc().unwrap().clone();
    s.open_drawing(copy, "B", None);
    let r = s.execute("compare", &json!({"document": 0})).unwrap();
    assert_eq!(counts(&r), (0, 0, 0, 4), "{r}");
}

/// A drawing written and read back recomputes what dimensions derive from their geometry (the
/// measurement, the text position): that is not a change.
#[test]
fn derived_dimension_fields_are_not_differences() {
    use cadcraft_engine::doc::{DimKind, Dimension};
    let dim = |measurement: f64, text_x: f64| Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(15.0, 5.0, 0.0),
        text_mid: Vec3::new(text_x, 5.0, 0.0),
        p13: Vec3::ZERO,
        p14: Vec3::new(30.0, 0.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    };
    let mut s = Session::new();
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), EntityKind::Dimension(dim(30.0, 7.0))).unwrap();
    let mut other = s.doc().unwrap().clone();
    let h = other.model.iter().next().unwrap().handle;
    other.modify_entity(h, |e| e.kind = EntityKind::Dimension(Dimension { block: Some("*D1".into()), ..dim(0.0, 15.0) })).unwrap();
    s.open_drawing(other, "B", None);
    let r = s.execute("compare", &json!({"document": 0})).unwrap();
    assert_eq!(counts(&r), (0, 0, 0, 1), "{r}");
}

/// COMPAREEXPORT (#2049): an xref path relative to the compared file keeps that file's folder.
#[test]
fn relative_xref_paths_of_a_compared_file_take_that_files_folder() {
    hooks();
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("compareexport-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("other.dxf");
    std::fs::write(&path, "xref").unwrap();
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
