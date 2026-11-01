//! PURGE.LIST reports what PURGE would remove, by kind, without removing anything; PURGE takes
//! `names` as wildcard patterns or as a list of exact names (the Purge dialog's checked items).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Common, EntityKind, Line, Space};
use cadcraft_engine::geom::Vec3;
use serde_json::{Value, json};

/// Unused: blocks U1 and U2 (each holding a line on layer Inner), layers Spare, A#1 and A51,
/// linetype DASHED, text style Notes, dimension style Arch; one zero-length line and one empty
/// text in model space.
fn drawing() -> Session {
    let mut s = Session::new();
    s.execute("layer.new", &json!({ "name": "Inner" })).unwrap();
    s.execute("layer.current", &json!({ "name": "Inner" })).unwrap();
    for name in ["U1", "U2"] {
        s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap();
        s.execute("selectall", &json!({})).unwrap();
        s.execute("block", &json!({ "name": name, "base": [0, 0], "keep": "delete" })).unwrap();
    }
    s.execute("layer.current", &json!({ "name": "0" })).unwrap();
    for name in ["Spare", "A#1", "A51"] {
        s.execute("layer.new", &json!({ "name": name })).unwrap();
    }
    s.execute("linetype", &json!({ "load": "DASHED" })).unwrap();
    s.execute("style", &json!({ "name": "Notes", "current": false })).unwrap();
    s.execute("dimstyle", &json!({ "name": "Arch", "current": false })).unwrap();
    let zero = EntityKind::Line(Line { a: Vec3::new(5.0, 5.0, 0.0), b: Vec3::new(5.0, 5.0, 0.0) });
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), zero).unwrap();
    let t = s.execute("text", &json!({ "at": [0, 10], "text": "x" })).unwrap();
    s.execute("textedit", &json!({ "handle": t["handle"], "text": " " })).unwrap();
    s
}

fn list(s: &mut Session) -> Value {
    s.execute("purge.list", &json!({})).unwrap()
}

fn names(v: &Value, kind: &str) -> Vec<String> {
    v["named"][kind].as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

#[test]
fn purge_list_reports_without_removing() {
    let mut s = drawing();
    let before = s.doc().unwrap().clone();
    let v = list(&mut s);
    let mut blocks = names(&v, "blocks");
    blocks.sort();
    assert_eq!(blocks, ["U1", "U2"]);
    // Inner is still used by the blocks' lines.
    assert_eq!(names(&v, "layers"), ["Spare", "A#1", "A51"]);
    assert_eq!(names(&v, "linetypes"), ["DASHED"]);
    assert_eq!(names(&v, "textstyles"), ["Notes"]);
    assert_eq!(names(&v, "dimstyles"), ["Arch"]);
    for kind in ["groups", "mleaderstyles", "mlinestyles", "tablestyles"] {
        assert!(names(&v, kind).is_empty(), "{kind}: {v}");
    }
    assert_eq!((v["zeroLength"].as_u64(), v["emptyText"].as_u64()), (Some(1), Some(1)));
    assert_eq!(*s.doc().unwrap(), before, "nothing changed");
    // Purging the blocks frees the layer their lines were on: the next list has it.
    s.execute("purge", &json!({ "type": "blocks" })).unwrap();
    assert!(names(&list(&mut s), "layers").contains(&"Inner".to_string()));
}

#[test]
fn exact_names_purge_only_those() {
    let mut s = drawing();
    // `#` is a wildcard for a digit: the pattern A#1 matches A51 and not A#1 itself.
    s.execute("purge", &json!({ "type": "layers", "names": "A#1" })).unwrap();
    let d = s.doc().unwrap();
    assert!(d.layer("A#1").is_some() && d.layer("A51").is_none());
    // As an exact name it is A#1 and nothing else, in any case.
    let mut s = drawing();
    let r = s.execute("purge", &json!({ "type": "layers", "names": ["a#1", "Spare"] })).unwrap();
    let d = s.doc().unwrap();
    assert!(d.layer("A#1").is_none() && d.layer("Spare").is_none() && d.layer("A51").is_some(), "{r}");
    // A name in use, or not there, is left out.
    let r = s.execute("purge", &json!({ "type": "blocks", "names": ["U1", "Nope"] })).unwrap();
    assert_eq!(r["purged"]["blocks"], json!(["U1"]));
    assert!(s.doc().unwrap().blocks.contains_key("U2"));
}

/// Names by kind, in one call: only those names of those kinds, with zero-length geometry and
/// empty text when `unnamed` asks, all one undo step.
#[test]
fn names_by_kind_and_unnamed_in_one_step() {
    let mut s = drawing();
    s.execute("style", &json!({ "name": "Spare", "current": false })).unwrap();
    let r = s.execute("purge", &json!({ "type": "all", "names": { "layers": ["Spare"], "Blocks": ["u2"] }, "unnamed": true })).unwrap();
    let d = s.doc().unwrap();
    // The text style called Spare isn't listed under textstyles: it stays.
    assert!(d.layer("Spare").is_none() && d.text_style("Spare").is_some(), "{r}");
    assert!(d.blocks.contains_key("U1") && !d.blocks.contains_key("U2"));
    assert!(d.layer("A#1").is_some() && d.linetype("DASHED").is_some(), "unlisted kinds and names stay");
    let v = list(&mut s);
    assert_eq!((v["zeroLength"].as_u64(), v["emptyText"].as_u64()), (Some(0), Some(0)));
    s.execute("undo", &json!({})).unwrap();
    let v = list(&mut s);
    assert!(names(&v, "layers").contains(&"Spare".to_string()) && names(&v, "blocks").len() == 2);
    assert_eq!((v["zeroLength"].as_u64(), v["emptyText"].as_u64()), (Some(1), Some(1)), "one undo brings it all back");
    // Without `unnamed`, they stay.
    s.execute("purge", &json!({ "type": "all" })).unwrap();
    assert_eq!(list(&mut s)["zeroLength"].as_u64(), Some(1));
}

/// The list and the purge read one scan: purging every listed name (the dialog with everything
/// checked) removes exactly the list. Purge All goes on to what that frees in the same step (layer
/// Inner, used only by the purged blocks' lines), which the next list would have shown.
#[test]
fn every_listed_item_is_what_purge_removes() {
    let mut s = drawing();
    let v = list(&mut s);
    let r = s.execute("purge", &json!({ "type": "all", "names": v["named"].clone() })).unwrap();
    for (kind, listed) in v["named"].as_object().unwrap() {
        let mut want = names(&v, kind);
        let mut got: Vec<String> =
            r["purged"][kind].as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        want.sort();
        got.sort();
        assert_eq!(got, want, "{kind}: listed {listed}");
    }
    assert!(s.doc().unwrap().layer("Inner").is_some(), "Inner wasn't listed, so it stays");

    let mut s = drawing();
    s.execute("purge", &json!({ "type": "all" })).unwrap();
    let d = s.doc().unwrap();
    assert!(d.blocks.is_empty() && d.layer("Inner").is_none(), "Purge All takes the nested layer too");
    assert!(d.layer("0").is_some() && d.text_style("Standard").is_some() && d.dim_style("Standard").is_some(), "the defaults stay");
}

#[test]
fn names_that_are_not_text_purge_nothing() {
    for bad in [
        json!(7),
        json!(["Spare", 3]),
        json!({ "layers": "Spare" }),
        json!({ "nope": ["x"] }),
        json!({ "zerolength": [] }),
        json!(vec!["x"; 100_001]),
        json!({ "layers": vec!["x"; 60_000], "blocks": vec!["y"; 60_000] }),
    ] {
        let mut s = drawing();
        let before = s.doc().unwrap().clone();
        assert!(s.execute("purge", &json!({ "type": "all", "names": bad })).is_err(), "{bad}");
        assert_eq!(*s.doc().unwrap(), before);
    }
    let mut s = drawing();
    assert!(s.execute("purge", &json!({ "unnamed": "yes" })).is_err());
    // No names, or null: every unused object, as before.
    let mut s = drawing();
    s.execute("purge", &json!({ "type": "layers", "names": null })).unwrap();
    assert!(names(&list(&mut s), "layers").is_empty());
}
