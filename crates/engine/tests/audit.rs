//! AUDIT finds and fixes database errors; RECOVER opens a damaged drawing with the fixes applied
//! (#406, docs/gaps.md F4).

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, base64_encode, set_io};
use cadcraft_engine::doc::{AssocSnap, Block, Circle, Common, DimAssoc, Drawing, Entity, EntityKind, Group, HVal, Handle, Insert, Line, Space, Text};
use cadcraft_engine::geom::Vec3;
use serde_json::{Value, json};

fn line(a: (f64, f64), b: (f64, f64)) -> EntityKind {
    EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) })
}

fn insert(block: &str) -> EntityKind {
    EntityKind::Insert(Insert {
        block: block.into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    })
}

fn on(layer: &str) -> Common {
    Common { layer: layer.into(), ..Common::default() }
}

/// Push an object without the checks `Drawing::add` makes (it would create the layer).
fn raw(d: &mut Drawing, space: &Space, common: Common, kind: EntityKind) -> Handle {
    let h = d.new_handle();
    d.space_mut(space).unwrap().push(Entity { handle: h, common, kind });
    h
}

/// A drawing with one of each error AUDIT knows about, plus a good line and an associative
/// dimension. Returns the handles of the good line and the dimension.
fn corrupt(d: &mut Drawing) -> (Handle, Handle) {
    let good = raw(d, &Space::Model, on("0"), line((0.0, 0.0), (10.0, 0.0)));
    raw(d, &Space::Model, on("Ghost"), line((0.0, 5.0), (10.0, 5.0)));
    raw(
        d,
        &Space::Model,
        Common { linetype: "NoSuchDash".into(), ..on("0") },
        EntityKind::Circle(Circle { center: Vec3::new(3.0, 3.0, 0.0), radius: 1.0 }),
    );
    raw(d, &Space::Model, on("0"), EntityKind::Circle(Circle { center: Vec3::ZERO, radius: 0.0 }));
    raw(d, &Space::Model, on("0"), line((f64::NAN, 0.0), (1.0, 1.0)));
    raw(d, &Space::Model, on("0"), line((2.0, 2.0), (2.0, 2.0)));
    raw(
        d,
        &Space::Model,
        on("0"),
        EntityKind::Text(Text {
            insert: Vec3::ZERO,
            align_pt: None,
            height: 1.0,
            value: "A".into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Vanished".into(),
            halign: Default::default(),
            valign: Default::default(),
        }),
    );
    raw(d, &Space::Model, on("0"), insert("Nowhere"));
    // A and B contain each other; C contains itself.
    for (name, inner) in [("A", "B"), ("B", "A"), ("C", "C")] {
        let mut b = Block::new(name);
        let h = d.new_handle();
        b.entities.push(Entity { handle: h, common: on("0"), kind: insert(inner) });
        d.blocks.insert(name.into(), Arc::new(b));
    }
    raw(d, &Space::Model, on("0"), insert("A"));
    // The same handle in model space and in a layout.
    let dup = d.model.iter().nth(2).unwrap().handle;
    d.space_mut(&Space::Paper("Layout1".into())).unwrap().push(Entity { handle: dup, common: on("0"), kind: line((0.0, 0.0), (1.0, 1.0)) });
    d.groups.push(Group { name: "G".into(), description: String::new(), selectable: true, members: vec![good, Handle(0xDEAD)] });
    d.header.set("CLAYER", HVal::Str("Gone".into()));
    d.header.set_str("DIMSTYLE", "Lost");
    let dim = d.model.iter().find(|e| matches!(e.kind, EntityKind::Dimension(_))).map(|e| e.handle).unwrap();
    d.modify_entity(dim, |e| {
        if let EntityKind::Dimension(dm) = &mut e.kind {
            dm.assoc.push(DimAssoc { point: "p13".into(), handle: Handle(0xBEEF), snap: AssocSnap::Start });
        }
    })
    .unwrap();
    (good, dim)
}

/// A session whose drawing has a dimension, then every error from [`corrupt`].
fn corrupt_session() -> (Session, Handle, Handle) {
    let mut s = Session::new();
    s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [4, 0], "at": [2, 2]})).unwrap();
    let mut d = (*s.doc().unwrap()).clone();
    let (good, dim) = corrupt(&mut d);
    let st = s.state_mut().unwrap();
    st.doc = Arc::new(d);
    st.saved = st.doc.clone();
    (s, good, dim)
}

fn kinds(r: &Value) -> Vec<String> {
    r["issues"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap().to_string()).collect()
}

#[test]
fn audit_reports_without_changing_and_fixes_when_asked() {
    let (mut s, good, dim) = corrupt_session();
    let before = s.state().unwrap().doc.clone();
    let undo = s.state().unwrap().undo.len();

    let r = s.execute("audit", &json!({})).unwrap();
    let k = kinds(&r);
    for want in [
        "missingLayer",
        "missingReference",
        "invalidGeometry",
        "duplicateHandle",
        "missingBlock",
        "blockCycle",
        "danglingReference",
        "missingCurrent",
    ] {
        assert!(k.iter().any(|x| x == want), "{want} not reported: {r:#}");
    }
    // NaN line, zero radius circle, zero length line.
    assert_eq!(k.iter().filter(|x| *x == "invalidGeometry").count(), 3, "{r:#}");
    assert_eq!(r["fixed"], 0);
    assert!(r["errors"].as_u64().unwrap() >= 12);
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc), "a report without fix changes nothing");
    assert_eq!(s.state().unwrap().undo.len(), undo);

    let r = s.execute("audit", &json!({"fix": true})).unwrap();
    assert_eq!(r["fixed"], r["errors"]);
    let d = s.doc().unwrap();
    assert!(d.layer("Ghost").is_some(), "missing layer recreated");
    assert!(d.block("Nowhere").is_some_and(|b| b.entities.is_empty()), "missing block recreated empty");
    assert_eq!(d.header.str("CLAYER", ""), "0");
    assert_eq!(d.header.str("DIMSTYLE", ""), "Standard");
    assert_eq!(d.groups[0].members, vec![good]);
    let EntityKind::Dimension(dm) = &d.entity(dim).unwrap().kind else { panic!() };
    assert!(dm.assoc.iter().all(|a| a.handle != Handle(0xBEEF)));
    for e in d.model.iter() {
        match &e.kind {
            EntityKind::Text(t) => assert_eq!(t.style, "Standard"),
            EntityKind::Circle(c) => {
                assert!(c.radius > 0.0);
                assert_eq!(e.common.linetype, "ByLayer");
            }
            EntityKind::Line(l) => assert!(l.a != l.b && l.a.x.is_finite()),
            _ => {}
        }
    }
    // Every handle is unique and below the next one.
    let mut all: Vec<Handle> = d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())).map(|e| e.handle).collect();
    all.extend(d.blocks.values().flat_map(|b| b.entities.iter().map(|e| e.handle)));
    let n = all.len();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), n, "duplicate handles remain");
    assert!(all.iter().all(|h| h.0 < d.handseed));

    // Clean now; one undo step brings the errors back.
    assert_eq!(s.execute("audit", &json!({})).unwrap()["errors"], 0);
    assert!(s.state().unwrap().is_dirty());
    s.undo().unwrap();
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
}

#[test]
fn a_clean_drawing_is_left_alone() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [5, 5]]})).unwrap();
    s.execute("circle", &json!({"center": [1, 1], "radius": 2})).unwrap();
    let before = s.state().unwrap().doc.clone();
    let undo = s.state().unwrap().undo.len();
    let r = s.execute("audit", &json!({"fix": true})).unwrap();
    assert_eq!((r["errors"].as_u64(), r["fixed"].as_u64()), (Some(0), Some(0)));
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    assert_eq!(s.state().unwrap().undo.len(), undo, "no undo step for nothing");

    // What the sample drawings and the drawing commands make is not an error.
    for d in [cadcraft_engine::sample::bracket(), cadcraft_engine::sample::floor_plan()] {
        let r = cadcraft_engine::audit::check(&d);
        assert!(r.issues.is_empty(), "{}", r.message());
    }
    s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [4, 0], "at": [2, 2]})).unwrap();
    s.execute(
        "dimradius",
        &json!({"handle": s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Circle(_))).unwrap().handle.hex(), "at": [4, 4]}),
    )
    .ok();
    s.script("mtext 0,0 5,-5\nNote\n\nhatch p solid 0,0 \n").ok();
    s.execute("block", &json!({"name": "Bolt", "base": [0, 0], "handles": s.doc().unwrap().model.handles()[..2]})).unwrap();
    s.execute("insert", &json!({"name": "Bolt", "at": [20, 20]})).unwrap();
    let r = s.execute("audit", &json!({})).unwrap();
    assert_eq!(r["errors"], 0, "{r:#}");
}

#[test]
fn audit_at_the_command_line_asks_before_fixing() {
    let (mut s, _, _) = corrupt_session();
    let before = s.state().unwrap().doc.clone();
    s.cmdline("audit").unwrap();
    assert_eq!(s.prompt_text(), "AUDIT Fix any errors detected? [Yes/No] <N>:");
    s.cmdline("").unwrap();
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc), "Enter takes No");
    assert!(s.log.iter().any(|l| l.contains("Run AUDIT and answer Yes")), "{:?}", s.log);

    s.cmdline("audit").unwrap();
    s.cmdline("y").unwrap();
    assert!(s.running.is_none());
    assert!(!Arc::ptr_eq(&before, &s.state().unwrap().doc));
    let summary = s.log.iter().rev().find(|l| l.starts_with("Audit:")).unwrap();
    assert!(summary.contains("fixed") && !summary.contains(" 0 fixed"), "{summary}");
    assert_eq!(s.execute("audit", &json!({})).unwrap()["errors"], 0);
}

#[test]
fn missing_standard_entries_and_layer_zero_are_recreated() {
    let mut s = Session::new();
    let d = s.doc_mut().unwrap();
    d.layers.retain(|l| l.name != "0");
    d.text_styles.clear();
    d.dim_styles.clear();
    d.linetypes.retain(|l| !l.name.eq_ignore_ascii_case("Continuous"));
    d.layers.push(cadcraft_engine::doc::Layer { linetype: "Dashed9".into(), ..cadcraft_engine::doc::Layer::new("L1") });
    let r = s.execute("audit", &json!({"fix": true})).unwrap();
    assert!(r["errors"].as_u64().unwrap() >= 5, "{r:#}");
    let d = s.doc().unwrap();
    assert!(d.layer("0").is_some() && d.text_style("Standard").is_some() && d.dim_style("Standard").is_some());
    assert!(d.linetype("Continuous").is_some());
    assert_eq!(d.layer("L1").unwrap().linetype, "Continuous");
}

#[test]
fn a_long_block_cycle_is_broken_without_deep_recursion() {
    let mut s = Session::new();
    let d = s.doc_mut().unwrap();
    let n = 20_000;
    for i in 0..n {
        let mut b = Block::new(&format!("B{i}"));
        let h = d.new_handle();
        b.entities.push(Entity { handle: h, common: on("0"), kind: insert(&format!("B{}", (i + 1) % n)) });
        d.blocks.insert(b.name.clone(), Arc::new(b));
    }
    let r = s.execute("audit", &json!({"fix": true})).unwrap();
    assert_eq!(kinds(&r).iter().filter(|k| *k == "blockCycle").count(), 1, "one reference closes the loop");
    assert_eq!(s.execute("audit", &json!({})).unwrap()["errors"], 0);
}

fn read_corrupt(_: &[u8], name: &str) -> Result<Drawing, String> {
    if name.ends_with(".bad") {
        return Err("not a drawing".into());
    }
    let mut d = Drawing::new_imperial();
    raw(&mut d, &Space::Model, on("Ghost"), line((0.0, 0.0), (1.0, 0.0)));
    raw(&mut d, &Space::Model, on("0"), EntityKind::Circle(Circle { center: Vec3::ZERO, radius: -1.0 }));
    Ok(d)
}

#[test]
fn recover_opens_the_file_with_the_fixes_applied() {
    set_io(IoHooks { read: read_corrupt, write: |_, _| Ok(Vec::new()), plot: None });
    let mut s = Session::new();
    let r = s.execute("recover", &json!({"data": base64_encode(b"0\nEOF\n"), "name": "plan.dxf"})).unwrap();
    assert_eq!(r["errors"], 2, "{r:#}");
    assert_eq!(r["fixed"], 2);
    assert_eq!(r["index"], 1);
    let st = s.state().unwrap();
    assert_eq!(st.title, "plan.dxf");
    assert!(st.is_dirty(), "the fixes are unsaved changes");
    assert!(st.doc.layer("Ghost").is_some());
    assert_eq!(st.doc.model.len(), 1);

    // By path, typed at the command line.
    let dir = std::env::temp_dir().join(format!("cadcraft-recover-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("site.dxf");
    std::fs::write(&path, b"0\nEOF\n").unwrap();
    s.cmdline("recover").unwrap();
    assert_eq!(s.prompt_text(), "RECOVER Enter name of drawing file to recover:");
    s.cmdline(&path.to_string_lossy()).unwrap();
    assert!(s.running.is_none());
    let st = s.state().unwrap();
    assert_eq!((st.title.as_str(), st.path.as_deref()), ("site.dxf", Some(path.to_string_lossy().as_ref())));
    assert!(s.log.iter().any(|l| l == "Audit: 2 errors found, 2 fixed."), "{:?}", s.log);

    // A file the reader rejects is an error; nothing opens.
    let docs = s.docs.len();
    assert!(s.execute("recover", &json!({"data": "", "name": "x.bad"})).is_err());
    assert!(s.execute("recover", &json!({})).is_err());
    assert_eq!(s.docs.len(), docs);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn auditctl_generates_adt_report_file() {
    let mut s = Session::new();
    // Default is 0 (off)
    assert_eq!(s.execute("getvar", &json!({"name": "AUDITCTL"})).unwrap()["value"], json!(0));
    s.execute("setvar", &json!({"name": "AUDITCTL", "value": 1})).unwrap();
    assert_eq!(s.execute("getvar", &json!({"name": "AUDITCTL"})).unwrap()["value"], json!(1));

    let dir = std::env::temp_dir().join(format!("cadcraft-auditctl-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dxf_path = dir.join("drawing_test.dxf");
    std::fs::write(&dxf_path, b"dummy").unwrap();

    s.state_mut().unwrap().path = Some(dxf_path.to_string_lossy().to_string());
    s.state_mut().unwrap().title = "drawing_test.dxf".to_string();

    let r = s.execute("audit", &json!({"fix": false})).unwrap();
    assert_eq!(r["errors"], 0);

    let adt_path = dxf_path.with_extension("adt");
    assert!(adt_path.exists(), "AUDITCTL 1 must generate .adt log file");
    let content = std::fs::read_to_string(&adt_path).unwrap();
    assert!(content.contains("Audit of drawing:"), "{content}");
    assert!(content.contains("Audit: 0 errors found"), "{content}");

    let _ = std::fs::remove_dir_all(&dir);
}
