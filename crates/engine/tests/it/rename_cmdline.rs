//! RENAME: blocks in the JSON form, and -RENAME's type / old name / new name prompts.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Drawing, EntityKind};
use serde_json::json;

/// Names of the blocks every INSERT (model, layouts and block definitions) refers to.
fn insert_targets(d: &Drawing) -> Vec<String> {
    let ents = d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())).chain(d.blocks.values().flat_map(|b| b.entities.iter()));
    let mut v: Vec<String> = ents.filter_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.block.clone()) } else { None }).collect();
    v.sort();
    v
}

/// Valve (with an attribute) inserted in model space, in Layout1 and nested inside Assy.
fn drawing() -> Session {
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 1 })).unwrap();
    s.execute("attdef", &json!({ "tag": "TAG", "at": [0, 2] })).unwrap();
    s.execute("selectall", &json!({})).unwrap();
    s.execute("block", &json!({ "name": "Valve", "base": [0, 0], "keep": "delete" })).unwrap();
    let h =
        s.execute("insert", &json!({ "name": "Valve", "at": [0, 0], "attribs": { "TAG": "V-1" } })).unwrap()["handle"].as_str().unwrap().to_string();
    s.execute("block", &json!({ "name": "Assy", "base": [0, 0], "handles": [h], "keep": "convert" })).unwrap();
    s.execute("insert", &json!({ "name": "Valve", "at": [5, 0], "attribs": { "TAG": "V-2" } })).unwrap();
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    s.execute("insert", &json!({ "name": "Valve", "at": [1, 1] })).unwrap();
    s.execute("layout.set", &json!({ "name": "Model" })).unwrap();
    s
}

#[test]
fn json_block_rename_repoints_every_insert() {
    let mut s = drawing();
    assert_eq!(insert_targets(s.doc().unwrap()), ["Assy", "Valve", "Valve", "Valve"]);
    let undo = s.state().unwrap().undo.len();
    s.execute("rename", &json!({ "table": "block", "from": "valve", "to": "Pump" })).unwrap();
    assert_eq!(s.state().unwrap().undo.len(), undo + 1);
    let d = s.doc().unwrap();
    assert!(d.block("Valve").is_none());
    assert_eq!(d.block("Pump").unwrap().name, "Pump");
    assert_eq!(insert_targets(d), ["Assy", "Pump", "Pump", "Pump"]);
    // The attribute values stay on the renamed inserts.
    assert!(d.model.iter().any(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block == "Pump" && i.attribs.iter().any(|a| a.text.value == "V-2"))));
    // Duplicates (any case), invalid names and unknown blocks are refused.
    assert!(s.execute("rename", &json!({ "table": "block", "from": "Pump", "to": "ASSY" })).is_err());
    assert!(s.execute("rename", &json!({ "table": "block", "from": "Pump", "to": "a*b" })).is_err());
    assert!(s.execute("rename", &json!({ "table": "block", "from": "Nope", "to": "X" })).is_err());
    assert_eq!(insert_targets(s.doc().unwrap()), ["Assy", "Pump", "Pump", "Pump"]);
    // Multileader styles: the current-style variable follows; Standard stays.
    s.execute("mleaderstyle", &json!({ "name": "Callout", "current": true })).unwrap();
    s.execute("rename", &json!({ "table": "mleaderstyle", "from": "Callout", "to": "Notes" })).unwrap();
    assert_eq!(s.doc().unwrap().header.str("CMLEADERSTYLE", ""), "Notes");
    assert!(s.execute("rename", &json!({ "table": "mleaderstyle", "from": "Standard", "to": "X" })).is_err());
    // A layer rename is one undo step too.
    s.execute("layer.new", &json!({ "name": "Old" })).unwrap();
    let undo = s.state().unwrap().undo.len();
    s.execute("rename", &json!({ "table": "layer", "from": "Old", "to": "New" })).unwrap();
    assert_eq!(s.state().unwrap().undo.len(), undo + 1);
}

#[test]
fn rename_prompts_type_old_and_new_name() {
    let mut s = drawing();
    s.cmdline("-RENAME").unwrap();
    assert!(
        s.prompt_text().contains("[Block/Dimstyle/LAyer/LType/Material/multileadeRstyle/Plotstyle/textStyle/Tablestyle/Ucs/VIew/VPort]"),
        "{}",
        s.prompt_text()
    );
    s.cancel();
    s.execute("layer.new", &json!({ "name": "Old" })).unwrap();
    s.script("-RENAME B Valve Pump\n-RENAME LA Old New\nLINE 0,0 1,0\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let d = s.doc().unwrap();
    assert!(d.block("Pump").is_some() && d.layer("New").is_some() && d.layer("Old").is_none(), "{:?}", s.log);
    assert_eq!(insert_targets(d), ["Assy", "Pump", "Pump", "Pump"]);
    assert_eq!(d.model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count(), 1);
    // An unknown old name re-prompts; a type CADCraft lacks reads both names and reports.
    s.script("-RENAME LA Missing\nNew\nNewer\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(s.doc().unwrap().layer("Newer").is_some());
    let n = s.log.len();
    s.script("-RENAME M a b\nLINE 0,0 2,0\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(s.log[n..].iter().any(|l| l.contains("Renaming a material is not available")), "{:?}", &s.log[n..]);
    assert_eq!(s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count(), 2);
}

/// Layer P and a line on it use plot style "Heavy"; a circle uses "Thin"; Heavy is current.
fn plot_styled() -> Session {
    let mut s = Session::new();
    s.execute("layer.new", &json!({ "name": "P" })).unwrap();
    s.execute("layer.set", &json!({ "name": "P", "plotStyle": "Heavy" })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let l = s.doc().unwrap().model.last().unwrap().handle.hex();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 1 })).unwrap();
    let c = s.doc().unwrap().model.last().unwrap().handle.hex();
    s.execute("plotstyle", &json!({ "name": "Heavy", "handles": [l] })).unwrap();
    s.execute("plotstyle", &json!({ "name": "Thin", "handles": [c] })).unwrap();
    s.execute("plotstyle", &json!({ "name": "Heavy" })).unwrap();
    s
}

/// The plot styles of layer P, of each object, and the current one.
fn plot_styles(d: &Drawing) -> (String, Vec<String>, String) {
    let layer = d.layer("P").unwrap().plot_style.clone();
    let objects = d.model.iter().map(|e| e.common.plot_style.clone()).collect();
    (layer, objects, d.header.str("CPLOTSTYLE", ""))
}

#[test]
fn plot_styles_rename_with_their_layers_objects_and_the_current_style() {
    let mut s = plot_styled();
    let undo = s.state().unwrap().undo.len();
    s.execute("rename", &json!({ "table": "plotstyle", "from": "heavy", "to": "Bold" })).unwrap();
    assert_eq!(s.state().unwrap().undo.len(), undo + 1);
    let want = ("Bold".to_string(), vec!["Bold".to_string(), "Thin".to_string()], "Bold".to_string());
    assert_eq!(plot_styles(s.doc().unwrap()), want);
    // The new name is written and read back with the layer and the objects (DXF: the DWG writer
    // keeps no plot style names).
    let d = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    let (layer, objects, _) = plot_styles(&d);
    assert_eq!((layer, objects), (want.0.clone(), want.1.clone()));
    // Refused: a name in use, Normal, ByLayer, an unused name, invalid names.
    for (from, to) in [("Bold", "thin"), ("Bold", "Normal"), ("Normal", "X"), ("ByLayer", "X"), ("Nope", "X"), ("Bold", "a*b"), ("Bold", "ByBlock")] {
        assert!(s.execute("rename", &json!({ "table": "plotstyle", "from": from, "to": to })).is_err(), "{from} -> {to}");
    }
    assert_eq!(plot_styles(s.doc().unwrap()), want);
    // -RENAME at the command line: an unused old name is asked again.
    s.script("-RENAME P Nope\nThin Fine\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(plot_styles(s.doc().unwrap()).1, ["Bold", "Fine"]);
    s.undo().unwrap();
    assert_eq!(plot_styles(s.doc().unwrap()).1, ["Bold", "Thin"]);
}

/// The plot styles of the objects in Layout1 and in block definition B.
fn layout_and_block_styles(d: &Drawing) -> (Vec<String>, Vec<String>) {
    let layout = d.layout("Layout1").unwrap().entities.iter().map(|e| e.common.plot_style.clone()).collect();
    let block = d.block("B").unwrap().entities.iter().map(|e| e.common.plot_style.clone()).collect();
    (layout, block)
}

#[test]
fn plot_style_rename_reaches_layouts_and_block_definitions_and_undoes() {
    let mut s = plot_styled();
    // A line in block definition B and a circle in Layout1 use Heavy too.
    s.execute("line", &json!({ "points": [[50, 50], [60, 50]] })).unwrap();
    let l = s.doc().unwrap().model.last().unwrap().handle.hex();
    s.execute("plotstyle", &json!({ "name": "Heavy", "handles": [l.clone()] })).unwrap();
    s.execute("block", &json!({ "name": "B", "base": [50, 50], "handles": [l], "keep": "delete" })).unwrap();
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    s.execute("circle", &json!({ "center": [1, 1], "radius": 1 })).unwrap();
    let c = s.doc().unwrap().layout("Layout1").unwrap().entities.last().unwrap().handle.hex();
    s.execute("plotstyle", &json!({ "name": "Heavy", "handles": [c] })).unwrap();
    s.execute("layout.set", &json!({ "name": "Model" })).unwrap();
    let before = layout_and_block_styles(s.doc().unwrap());
    assert!(before.0.iter().any(|n| n == "Heavy") && before.1 == ["Heavy"], "{before:?}");

    s.execute("rename", &json!({ "table": "plotstyle", "from": "Heavy", "to": "Bold" })).unwrap();
    let (layout, block) = layout_and_block_styles(s.doc().unwrap());
    assert!(layout.iter().any(|n| n == "Bold") && !layout.iter().any(|n| n == "Heavy"), "{layout:?}");
    assert_eq!(block, ["Bold"]);
    s.undo().unwrap();
    assert_eq!(layout_and_block_styles(s.doc().unwrap()), before, "undo restores layouts and block definitions");
    assert_eq!(plot_styles(s.doc().unwrap()).1, ["Heavy", "Thin"]);

    // A rename that only changes the case changes it everywhere.
    s.execute("rename", &json!({ "table": "plotstyle", "from": "Heavy", "to": "heavy" })).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(plot_styles(d), ("heavy".to_string(), vec!["heavy".to_string(), "Thin".to_string()], "heavy".to_string()));
    assert_eq!(layout_and_block_styles(d).1, ["heavy"]);
    assert!(layout_and_block_styles(d).0.iter().any(|n| n == "heavy"));
}

#[test]
fn named_viewport_configurations_rename() {
    let mut s = Session::new();
    s.viewport_px = (1200.0, 800.0);
    s.execute("-vports", &json!({ "option": "2", "arrangement": "vertical" })).unwrap();
    s.execute("-vports", &json!({ "option": "save", "name": "Two" })).unwrap();
    s.execute("-vports", &json!({ "option": "save", "name": "Other" })).unwrap();
    let names = |d: &Drawing| -> Vec<String> { d.vports.iter().filter(|c| !c.is_active()).map(|c| c.name.to_uppercase()).collect() };
    let undo = s.state().unwrap().undo.len();
    s.execute("rename", &json!({ "table": "vport", "from": "two", "to": "Pair" })).unwrap();
    assert_eq!(s.state().unwrap().undo.len(), undo + 1);
    assert_eq!(names(s.doc().unwrap()), ["PAIR", "OTHER"]);
    for (from, to) in [("Pair", "other"), ("Nope", "X"), ("*Active", "X"), ("Pair", "a<b")] {
        assert!(s.execute("rename", &json!({ "table": "vport", "from": from, "to": to })).is_err(), "{from} -> {to}");
    }
    assert!(s.execute("vports.named", &json!({ "name": "pair" })).is_ok(), "the renamed configuration restores");
    // -RENAME: an unknown old name is asked again.
    s.script("-RENAME VP Nope\nOther Side\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(names(s.doc().unwrap()), ["PAIR", "SIDE"]);
    for name in ["v.dxf", "v.dwg"] {
        let d = cadcraft_io::read(&cadcraft_io::write(s.doc().unwrap(), name).unwrap(), name).unwrap();
        let mut got = names(&d);
        got.sort();
        assert_eq!(got, ["PAIR", "SIDE"], "{name}");
    }
}
