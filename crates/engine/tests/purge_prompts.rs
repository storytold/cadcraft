//! -PURGE asks for the type, the names and whether to verify each; the JSON form takes
//! `{type, names}`.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

/// Unused: blocks U1 and U2, layer Spare, linetype DASHED, text style Notes, dimension style Arch.
fn drawing() -> Session {
    let mut s = Session::new();
    for name in ["U1", "U2"] {
        s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap();
        s.execute("selectall", &json!({})).unwrap();
        s.execute("block", &json!({ "name": name, "base": [0, 0], "keep": "delete" })).unwrap();
    }
    s.execute("layer.new", &json!({ "name": "Spare" })).unwrap();
    s.execute("linetype", &json!({ "load": "DASHED" })).unwrap();
    s.execute("style", &json!({ "name": "Notes", "current": false })).unwrap();
    s.execute("dimstyle", &json!({ "name": "Arch", "current": false })).unwrap();
    s
}

fn kinds(s: &Session) -> (usize, bool, bool, bool, bool) {
    let d = s.doc().unwrap();
    (d.blocks.len(), d.layer("Spare").is_some(), d.linetype("DASHED").is_some(), d.text_style("Notes").is_some(), d.dim_style("Arch").is_some())
}

fn count<F: Fn(&EntityKind) -> bool>(s: &Session, f: F) -> usize {
    s.doc().unwrap().model.iter().filter(|e| f(&e.kind)).count()
}

#[test]
fn layers_only_then_the_next_command_runs() {
    let mut s = drawing();
    s.script("-PURGE LA * N\nLINE 0,0 5,0\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(kinds(&s), (2, false, true, true, true), "only the layer is purged: {:?}", s.log);
    assert_eq!(count(&s, |k| matches!(k, EntityKind::Line(_))), 1);
    assert_eq!(count(&s, |k| matches!(k, EntityKind::Arc(_))), 0);
}

#[test]
fn all_without_verify_purges_every_named_type() {
    let mut s = drawing();
    s.cmdline("-PURGE").unwrap();
    assert!(
        s.prompt_text().contains(
            "[Blocks/DEtailviewstyles/Dimstyles/Groups/LAyers/LTypes/MAterials/MUltileaderstyles/Plotstyles/SHapes/textSTyles/Mlinestyles/SEctionviewstyles/Tablestyles/Visualstyles/Regapps/Zero-length geometry/Empty text objects/Orphaned data/All]"
        ),
        "{}",
        s.prompt_text()
    );
    s.script("A * N\nCIRCLE 0,0 1\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(kinds(&s), (0, false, false, false, false), "{:?}", s.log);
    assert_eq!(count(&s, |k| matches!(k, EntityKind::Circle(_))), 1);
    assert_eq!(count(&s, |k| matches!(k, EntityKind::Arc(_))), 0);
    // One undo step brings everything back.
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(kinds(&s), (2, true, true, true, true));
}

#[test]
fn verify_asks_for_each_name_and_absent_types_take_their_tokens() {
    let mut s = drawing();
    s.script("-PURGE B * Y\nY\nN\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let d = s.doc().unwrap();
    assert!(d.block("U1").is_none() && d.block("U2").is_some(), "{:?}", s.log);
    // A type CADCraft does not have still reads its name and verify answers, then reports.
    let n = s.log.len();
    s.script("-PURGE MA * N\nLINE 0,0 5,0\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(s.log[n..].iter().any(|l| l.contains("No unreferenced")), "{:?}", &s.log[n..]);
    assert_eq!(count(&s, |k| matches!(k, EntityKind::Line(_))), 1);
    // Zero-length geometry purges at once.
    s.execute("line", &json!({ "points": [[3, 3], [3, 3]] })).unwrap();
    s.script("-PURGE Z\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(count(&s, |k| matches!(k, EntityKind::Line(_))), 1);
}

#[test]
fn json_type_and_names_and_kept_linetypes() {
    let mut s = drawing();
    let r = s.execute("purge", &json!({ "type": "blocks", "names": "U2" })).unwrap();
    assert_eq!(r["purged"]["blocks"], json!(["U2"]));
    assert_eq!(kinds(&s), (1, true, true, true, true));
    assert!(s.execute("purge", &json!({ "type": "nonsense" })).is_err());
    // A linetype used only inside a block definition, or the current linetype, is kept.
    s.execute("linetype", &json!({ "load": "HIDDEN" })).unwrap();
    s.execute("linetype", &json!({ "load": "CENTER" })).unwrap();
    let l = s.execute("line", &json!({ "points": [[0, 0], [1, 0]] })).unwrap()["handles"][0].as_str().unwrap().to_string();
    s.execute("properties.set", &json!({ "handles": [l], "linetype": "HIDDEN" })).unwrap();
    s.execute("block", &json!({ "name": "Part", "base": [0, 0], "handles": [l], "keep": "convert" })).unwrap();
    s.execute("linetype", &json!({ "current": "CENTER" })).unwrap();
    s.execute("purge", &json!({ "type": "ltypes" })).unwrap();
    let d = s.doc().unwrap();
    assert!(d.linetype("HIDDEN").is_some() && d.linetype("CENTER").is_some());
    assert!(d.linetype("DASHED").is_none());
}
