//! LAYMRG merges layers into another one and LAYDEL deletes a layer with its objects: model space,
//! layouts and block definitions, typed (with confirmation) and as JSON (#286).

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

fn line_on(s: &mut Session, layer: &str, y: f64) -> String {
    s.execute("layer.current", &json!({ "name": layer })).unwrap();
    s.execute("line", &json!({ "points": [[0, y], [10, y]] })).unwrap()["handles"][0].as_str().unwrap().to_string()
}

/// Layers A and B with a line each in model space, a block "Part" holding an A line, a paper-space
/// A line and a layout viewport that freezes A and colours B.
fn drawing() -> Session {
    let mut s = Session::new();
    for n in ["A", "B", "C"] {
        s.execute("layer.new", &json!({ "name": n })).unwrap();
    }
    let a = line_on(&mut s, "A", 0.0);
    s.execute("block", &json!({ "name": "Part", "base": [0, 0], "handles": [a], "keep": "delete" })).unwrap();
    line_on(&mut s, "A", 1.0);
    line_on(&mut s, "B", 2.0);
    s.execute("layer.current", &json!({ "name": "0" })).unwrap();
    s.execute("insert", &json!({ "name": "Part", "at": [5, 5] })).unwrap();
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    line_on(&mut s, "A", 3.0);
    s.execute("layer.current", &json!({ "name": "0" })).unwrap();
    let vp = s.execute("mview", &json!({ "layout": "Layout1" })).unwrap()["viewports"][0].as_str().unwrap().to_string();
    s.execute("viewport.set", &json!({ "handle": vp, "freeze": ["A", "C"], "colors": { "B": 1 } })).unwrap();
    s.execute("layout.set", &json!({ "name": "Model" })).unwrap();
    s
}

/// Objects per layer name over model space, layouts and block definitions.
fn count(s: &Session, layer: &str) -> usize {
    let d = s.doc().unwrap();
    let paper = d.layouts.iter().flat_map(|l| l.entities.iter());
    let blocks = d.blocks.values().flat_map(|b| b.entities.iter());
    d.model.iter().chain(paper).chain(blocks).filter(|e| e.common.layer.eq_ignore_ascii_case(layer)).count()
}

fn viewport_lists(s: &Session) -> Vec<(Vec<String>, Vec<String>)> {
    let d = s.doc().unwrap();
    d.layouts
        .iter()
        .flat_map(|l| l.entities.iter())
        .filter_map(|e| match &e.kind {
            EntityKind::Viewport(v) => Some((v.frozen_layers.clone(), v.layer_colors.iter().map(|(n, _)| n.clone()).collect())),
            _ => None,
        })
        .collect()
}

#[test]
fn laymrg_typed_moves_everything_and_deletes_the_source() {
    let mut s = drawing();
    assert_eq!(count(&s, "A"), 3);
    let undo = s.state().unwrap().undo.len();
    // A is current: merging it away makes the target current.
    s.execute("layer.current", &json!({ "name": "A" })).unwrap();
    let undo_cur = s.state().unwrap().undo.len();
    assert_eq!(undo_cur, undo + 1);
    s.script("LAYMRG\nN\nA\n\nN\nB\n").unwrap();
    assert!(s.prompt_text().ends_with(" [Yes/No] <No>:"), "{}", s.prompt_text());
    assert_eq!(s.log.last().map(String::as_str), Some("Do you wish to continue?"));
    assert!(s.log.iter().any(|l| l.contains("merge layer \"A\" into layer \"B\"")), "{:?}", s.log);
    s.script("Y\n").unwrap();
    assert!(s.running.is_none());
    assert!(s.doc().unwrap().layer("A").is_none());
    assert_eq!(count(&s, "A"), 0);
    assert_eq!(count(&s, "B"), 4, "model, paper space and the block definition");
    assert_eq!(s.doc().unwrap().header.str("CLAYER", "0"), "B");
    // The viewport forgets A's freeze; B keeps its colour override.
    assert_eq!(viewport_lists(&s), vec![(vec!["C".to_string()], vec!["B".to_string()])]);
    assert_eq!(s.state().unwrap().undo.len(), undo_cur + 1, "one undo step");
    s.undo().unwrap();
    assert_eq!(count(&s, "A"), 3);

    // Answering No (or Enter) changes nothing; picking an object on the layer works too.
    let h = s.doc().unwrap().model.iter().find(|e| e.common.layer == "C" || e.common.layer == "A").map(|e| e.handle).unwrap();
    s.start("laymrg").unwrap();
    assert!(s.prompt_text().contains("Select object on layer to merge or [Name]:"), "{}", s.prompt_text());
    s.input(cadcraft_engine::Input::Pick(vec![h])).unwrap();
    assert!(s.prompt_text().contains("[Name/Undo]"), "{}", s.prompt_text());
    s.script("\nN\nC\n\n").unwrap();
    assert!(s.running.is_none());
    assert!(s.doc().unwrap().layer("A").is_some());
}

#[test]
fn laymrg_refusals() {
    let mut s = drawing();
    for (p, why) in [
        (json!({ "from": ["A"], "to": "A" }), "itself"),
        (json!({ "from": ["0"], "to": "A" }), "layer 0"),
        (json!({ "from": ["Defpoints"], "to": "A" }), "Defpoints"),
        (json!({ "from": ["Nope"], "to": "A" }), "no layer"),
        (json!({ "from": ["A"], "to": "Nope" }), "no layer"),
        (json!({ "from": [], "to": "A" }), "required"),
    ] {
        s.execute("layer.new", &json!({ "name": "Defpoints" })).ok();
        let e = s.execute("laymrg", &p).unwrap_err().to_string();
        assert!(e.contains(why), "{p}: {e}");
    }
    assert_eq!(count(&s, "A"), 3);
    // Typed: the target can't be a source, layer 0 can't be picked as a source.
    s.script("LAYMRG\nN\n0\nN\nA\n\nN\nA\n").unwrap();
    assert!(s.log.iter().any(|l| l.contains("Cannot merge layer \"0\"")), "{:?}", s.log);
    assert!(s.log.iter().any(|l| l.contains("into itself")), "{:?}", s.log);
    assert!(s.prompt_text().contains("Select object on target layer"), "{}", s.prompt_text());
    s.cancel();
    // JSON merges several layers at once.
    let r = s.execute("laymrg", &json!({ "from": ["A", "C"], "to": "0" })).unwrap();
    assert_eq!(r["moved"], 3);
    assert!(s.doc().unwrap().layer("C").is_none());
}

#[test]
fn laydel_deletes_the_layer_and_its_objects() {
    let mut s = drawing();
    let undo = s.state().unwrap().undo.len();
    s.script("LAYDEL\nN\nA\n\n").unwrap();
    assert!(s.prompt_text().ends_with(" [Yes/No] <No>:"), "{}", s.prompt_text());
    assert_eq!(s.log.last().map(String::as_str), Some("Do you wish to continue?"));
    s.script("Y\n").unwrap();
    assert!(s.running.is_none());
    assert!(s.doc().unwrap().layer("A").is_none());
    assert_eq!(count(&s, "A"), 0);
    assert_eq!(count(&s, "B"), 1);
    assert!(s.doc().unwrap().block("Part").is_some_and(|b| b.entities.is_empty()), "emptied, not removed");
    assert_eq!(viewport_lists(&s), vec![(vec!["C".to_string()], vec!["B".to_string()])]);
    assert_eq!(s.state().unwrap().undo.len(), undo + 1, "one undo step");

    // Refused: layer 0, Defpoints, the current layer, xref-dependent layers.
    s.execute("layer.current", &json!({ "name": "B" })).unwrap();
    let d = std::sync::Arc::make_mut(&mut s.state_mut().unwrap().doc);
    d.layers.push(cadcraft_engine::doc::Layer::new("Site|Walls"));
    d.layers.push(cadcraft_engine::doc::Layer::new("Defpoints"));
    for n in ["0", "Defpoints", "B", "Site|Walls"] {
        assert!(s.execute("laydel", &json!({ "names": [n] })).is_err(), "{n}");
        assert!(s.doc().unwrap().layer(n).is_some(), "{n}");
    }
    s.script("LAYDEL\nN\nB\n").unwrap();
    assert!(s.log.iter().any(|l| l.contains("Cannot delete the current layer")), "{:?}", s.log);
    s.cancel();
    let r = s.execute("laydel", &json!({ "names": ["C"] })).unwrap();
    assert_eq!(r["erased"], 0);
    assert!(viewport_lists(&s).iter().all(|(f, _)| f.is_empty()));
}

#[test]
fn layer_delete_clears_viewport_overrides_for_the_layer() {
    let mut s = drawing();
    s.execute("layer.delete", &json!({ "name": "C" })).unwrap();
    assert!(s.doc().unwrap().layer("C").is_none());
    assert_eq!(viewport_lists(&s), vec![(vec!["A".to_string()], vec!["B".to_string()])]);
    s.execute("layer.delete", &json!({ "name": "b" })).unwrap_err();
    s.execute("layer.new", &json!({ "name": "C" })).unwrap();
    assert!(viewport_lists(&s).iter().all(|(f, _)| !f.contains(&"C".to_string())));
}

#[test]
fn renaming_a_layer_repoints_viewport_lists() {
    let mut s = drawing();
    assert_eq!(viewport_lists(&s), vec![(vec!["A".to_string(), "C".to_string()], vec!["B".to_string()])]);
    s.execute("rename", &json!({ "table": "layer", "from": "A", "to": "Murs" })).unwrap();
    assert_eq!(viewport_lists(&s), vec![(vec!["Murs".to_string(), "C".to_string()], vec!["B".to_string()])]);
    // layer.set's newName form (what the Properties palette sends) goes through the same path.
    s.execute("layer.set", &json!({ "name": "B", "newName": "Cloisons" })).unwrap();
    assert_eq!(viewport_lists(&s), vec![(vec!["Murs".to_string(), "C".to_string()], vec!["Cloisons".to_string()])]);
    s.undo().unwrap();
    assert_eq!(viewport_lists(&s), vec![(vec!["Murs".to_string(), "C".to_string()], vec!["B".to_string()])], "undo brings the old name back");
}
