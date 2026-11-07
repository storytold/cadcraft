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
    // A type CADCraft lacks reads both names and reports.
    let n = s.log.len();
    s.script("-RENAME M a b\nLINE 0,0 2,0\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(s.log[n..].iter().any(|l| l.contains("Renaming a material is not available yet.")), "{:?}", &s.log[n..]);
    assert_eq!(s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count(), 2);
}

/// What `script` adds to the log; the command has ended.
fn said(s: &mut Session, script: &str) -> Vec<String> {
    let n = s.log.len();
    s.script(script).unwrap();
    assert!(s.running.is_none(), "{script}: {}", s.prompt_text());
    s.log[n..].to_vec()
}

fn says(log: &[String], text: &str) -> bool {
    log.iter().any(|l| l.contains(text))
}

#[test]
fn rename_ends_on_a_name_it_cannot_take_with_autocads_messages() {
    // AutoCAD 2024 (accoreconsole, measured, the same for blocks, dimension styles, layers,
    // linetypes, text styles, UCSs and views): `Cannot find layer "X".` for an old name that
    // doesn't exist, `A layer named "X" already exists.` and `*Invalid*` for a new name that does,
    // `Invalid layer name.` for one it can't take; each ends the command.
    let mut s = drawing();
    for name in ["A1", "A2"] {
        s.execute("layer.new", &json!({ "name": name })).unwrap();
    }
    let log = said(&mut s, "-RENAME LA NOSUCH\nLINE 0,0 3,0\n\n");
    assert!(says(&log, "Cannot find layer \"NOSUCH\"."), "{log:?}");
    // The command ended there: the next input started LINE.
    assert_eq!(s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count(), 1);
    let log = said(&mut s, "-RENAME LA A1 A2\n");
    assert!(says(&log, "A layer named \"A2\" already exists.") && says(&log, "*Invalid*"), "{log:?}");
    let log = said(&mut s, "-RENAME LA A1 X<Y\n");
    assert!(says(&log, "Invalid layer name."), "{log:?}");
    // No new name at all is an invalid one too.
    s.cmdline("-RENAME LA A1").unwrap();
    let n = s.log.len();
    s.cmdline("").unwrap();
    assert!(s.running.is_none() && says(&s.log[n..], "Invalid layer name."), "{:?}", &s.log[n..]);
    assert!(s.doc().unwrap().layer("A1").is_some());
    said(&mut s, "-RENAME LA A1 B1\n");
    assert!(s.doc().unwrap().layer("B1").is_some() && s.doc().unwrap().layer("A1").is_none());
    // A UCS is "ucs" in the prompts and in "A ucs named", "UCS" in the other two.
    for name in ["U1", "U2"] {
        let ucs = cadcraft_engine::doc::Ucs { name: name.into(), origin: Default::default(), x_axis: Default::default(), y_axis: Default::default() };
        s.doc_mut().unwrap().ucss.push(ucs);
    }
    s.cmdline("-RENAME U").unwrap();
    assert!(s.prompt_text().contains("Enter old ucs name"), "{}", s.prompt_text());
    s.cancel();
    assert!(says(&said(&mut s, "-RENAME U NOSUCH\n"), "Cannot find UCS \"NOSUCH\"."));
    assert!(says(&said(&mut s, "-RENAME U U1 U2\n"), "A ucs named \"U2\" already exists."));
    assert!(says(&said(&mut s, "-RENAME U U1 X<Y\n"), "Invalid UCS name."));
}

#[test]
fn rename_takes_a_saved_viewport_configuration() {
    // AutoCAD 2024 (measured): -RENAME VPort renames a saved configuration, every tile of it, old
    // name in any case, and to another case of its own name; "*ACTIVE" (old or new), names with
    // characters AutoCAD refuses and names already used are refused as for the other types.
    let mut s = Session::new();
    s.execute("-vports", &json!({ "option": "2", "arrangement": "vertical" })).unwrap();
    s.execute("-vports", &json!({ "option": "save", "name": "CFG1" })).unwrap();
    s.execute("-vports", &json!({ "option": "save", "name": "CFG3" })).unwrap();
    let names = |s: &Session| -> Vec<(String, usize)> { s.doc().unwrap().vports.iter().map(|v| (v.name.clone(), v.tiles.len())).collect() };
    said(&mut s, "-RENAME VP cfg1 CFG2\n");
    assert_eq!(names(&s), [("*Active".to_string(), 2), ("CFG2".to_string(), 2), ("CFG3".to_string(), 2)]);
    for (script, message) in [
        // A refused old name ends the command before the new name is asked.
        ("-RENAME VP NOSUCH\n", "Cannot find viewport configuration \"NOSUCH\"."),
        ("-RENAME VP *ACTIVE\n", "Invalid viewport configuration name."),
        ("-RENAME VP CFG2 *ACTIVE\n", "Invalid viewport configuration name."),
        ("-RENAME VP CFG2 A<B\n", "Invalid viewport configuration name."),
        ("-RENAME VP CFG2 CFG3\n", "A viewport configuration named \"CFG3\" already exists."),
    ] {
        assert!(says(&said(&mut s, script), message), "{script}");
    }
    said(&mut s, "-RENAME VP CFG2 cfg2\n");
    assert_eq!(names(&s), [("*Active".to_string(), 2), ("cfg2".to_string(), 2), ("CFG3".to_string(), 2)]);
    // The JSON form too, which never takes the configuration shown.
    s.execute("rename", &json!({ "table": "vport", "from": "CFG3", "to": "Pair" })).unwrap();
    assert_eq!(names(&s)[2].0, "Pair");
    assert!(s.execute("rename", &json!({ "table": "vport", "from": "*Active", "to": "Shown" })).is_err());
    assert_eq!(names(&s)[0].0, "*Active");
}
