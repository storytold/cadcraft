//! Block Description (blocks.description): a block's description changes in the drawing, and in
//! the Block Editor BSAVE saves the edited block's new description.

use cadcraft_engine::Session;
use serde_json::{Value, json};

/// A drawing with block "Door" (one line, described "Hinged door") and one reference to it, which
/// is returned.
fn door(s: &mut Session) -> Value {
    door_with(s, false)
}

fn door_with(s: &mut Session, annotative: bool) -> Value {
    let l = s.execute("line", &json!({ "points": [[10, 10], [12, 10]] })).unwrap()["handles"][0].clone();
    let p = json!({ "name": "Door", "base": [10, 10], "handles": [l], "description": "Hinged door", "annotative": annotative });
    s.execute("block", &p).unwrap()["insert"].clone()
}

fn description(s: &Session, block: &str) -> Option<String> {
    s.doc().unwrap().block(block).map(|b| b.description.clone())
}

/// The description of `block` in the drawing outside the Block Editor.
fn saved_description(s: &Session, block: &str) -> Option<String> {
    let st = s.state().unwrap();
    let d = st.block_edit.as_ref().map_or(&st.doc, |be| &be.host.doc);
    d.block(block).map(|b| b.description.clone())
}

#[test]
fn the_description_of_a_block_changes_and_undo_puts_it_back() {
    let mut s = Session::new();
    door(&mut s);
    let r = s.execute("blocks.description", &json!({ "name": "door", "description": "Sliding door" })).unwrap();
    assert_eq!(r, json!({ "block": "Door", "description": "Sliding door" }));
    assert_eq!(description(&s, "Door").as_deref(), Some("Sliding door"));
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(description(&s, "Door").as_deref(), Some("Hinged door"));

    // An empty description clears it; 255 characters are the most a description takes.
    s.execute("blocks.description", &json!({ "name": "Door", "description": "" })).unwrap();
    assert_eq!(description(&s, "Door").as_deref(), Some(""));
    let longest = "é".repeat(255);
    s.execute("blocks.description", &json!({ "name": "Door", "description": longest })).unwrap();
    assert_eq!(description(&s, "Door"), Some(longest));
}

#[test]
fn what_block_description_refuses() {
    let mut s = Session::new();
    door(&mut s);
    for p in [
        json!({ "name": "Door", "description": "x".repeat(256) }),
        json!({ "name": "Door", "description": "two\nlines" }),
        json!({ "name": "Door", "description": "carriage\rreturn" }),
        json!({ "name": "Window", "description": "x" }),
        json!({ "name": "Door" }),
        json!({ "name": "Door", "description": 7 }),
        json!({ "description": "nothing selected" }),
    ] {
        assert!(s.execute("blocks.description", &p).is_err(), "{p}");
    }
    assert_eq!(description(&s, "Door").as_deref(), Some("Hinged door"));
}

#[test]
fn block_refuses_a_description_a_block_cannot_take() {
    let mut s = Session::new();
    for cmd in ["block", "-block"] {
        for description in ["x".repeat(256), "two\nlines".into()] {
            let l = s.execute("line", &json!({ "points": [[0, 0], [1, 0]] })).unwrap()["handles"][0].clone();
            let p = json!({ "name": "B", "base": [0, 0], "handles": [l], "description": description });
            assert!(s.execute(cmd, &p).is_err(), "{cmd} {description:?}");
            assert!(s.doc().unwrap().block("B").is_none());
        }
    }
    for (cmd, name) in [("block", "B"), ("-block", "C")] {
        let l = s.execute("line", &json!({ "points": [[0, 0], [1, 0]] })).unwrap()["handles"][0].clone();
        s.execute(cmd, &json!({ "name": name, "base": [0, 0], "handles": [l], "description": "x".repeat(255) })).unwrap();
        assert_eq!(description(&s, name), Some("x".repeat(255)), "{cmd}");
    }
}

#[test]
fn the_selected_reference_names_the_block_at_the_prompts() {
    let mut s = Session::new();
    let r = door(&mut s);
    s.execute("select", &json!({ "handles": [r] })).unwrap();
    s.cmdline("blocks.description").unwrap();
    assert!(s.prompt_text().contains("Enter description for block \"Door\" <Hinged door>"), "{}", s.prompt_text());
    // Enter keeps the description.
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(description(&s, "Door").as_deref(), Some("Hinged door"));

    s.execute("select", &json!({ "handles": [] })).unwrap();
    s.script("blocks.description\nDoor\nSliding door\n").unwrap();
    assert!(s.running.is_none());
    assert_eq!(description(&s, "Door").as_deref(), Some("Sliding door"));
}

#[test]
fn the_block_editor_saves_the_new_description_with_the_block() {
    let mut s = Session::new();
    door(&mut s);
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    s.execute("blocks.description", &json!({ "description": "Sliding door" })).unwrap();
    assert!(s.state().unwrap().block_unsaved());
    assert_eq!(saved_description(&s, "Door").as_deref(), Some("Hinged door"));
    s.execute("bsave", &json!({})).unwrap();
    assert_eq!(saved_description(&s, "Door").as_deref(), Some("Sliding door"));

    // Closing without saving leaves the drawing's description as it was.
    s.execute("blocks.description", &json!({ "description": "Pocket door" })).unwrap();
    s.execute("bclose", &json!({ "save": false })).unwrap();
    assert_eq!(description(&s, "Door").as_deref(), Some("Sliding door"));
}

#[test]
fn bsaveas_gives_the_new_description_to_the_new_block_only() {
    let mut s = Session::new();
    door_with(&mut s, true);
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    s.execute("blocks.description", &json!({ "description": "Sliding door" })).unwrap();
    s.execute("bsaveas", &json!({ "name": "Slider" })).unwrap();
    assert_eq!(saved_description(&s, "Slider").as_deref(), Some("Sliding door"));
    assert_eq!(saved_description(&s, "Door").as_deref(), Some("Hinged door"));

    // The editor now edits the new block; saving again leaves the old block's description alone
    // and keeps the new block's other properties.
    s.execute("blocks.description", &json!({ "description": "Sliding door, 900" })).unwrap();
    s.execute("bclose", &json!({})).unwrap();
    assert_eq!(description(&s, "Slider").as_deref(), Some("Sliding door, 900"));
    assert_eq!(description(&s, "Door").as_deref(), Some("Hinged door"));
    assert!(s.doc().unwrap().block("Slider").unwrap().annotative);
}

#[test]
fn bsaveas_over_another_block_keeps_the_saved_description() {
    let mut s = Session::new();
    door(&mut s);
    let l = s.execute("line", &json!({ "points": [[0, 0], [1, 0]] })).unwrap()["handles"][0].clone();
    s.execute("block", &json!({ "name": "Window", "base": [0, 0], "handles": [l], "description": "Casement" })).unwrap();
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    s.execute("blocks.description", &json!({ "description": "Sliding door" })).unwrap();
    s.execute("bsaveas", &json!({ "name": "Window", "replace": true })).unwrap();
    assert_eq!(saved_description(&s, "Window").as_deref(), Some("Sliding door"));
    // The prompt shows the saved description, and a later save keeps it.
    s.cmdline("blocks.description").unwrap();
    assert!(s.prompt_text().contains("<Sliding door>"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [0, 1]] })).unwrap();
    s.execute("bclose", &json!({})).unwrap();
    assert_eq!(description(&s, "Window").as_deref(), Some("Sliding door"));
    assert_eq!(description(&s, "Door").as_deref(), Some("Hinged door"));

    // An undo in the editor after saving over Window leaves Window's new description.
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    s.execute("blocks.description", &json!({ "description": "Pocket door" })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [0, 2]] })).unwrap();
    s.execute("bsaveas", &json!({ "name": "Window", "replace": true })).unwrap();
    s.execute("undo", &json!({})).unwrap();
    s.execute("bsave", &json!({})).unwrap();
    s.execute("bclose", &json!({})).unwrap();
    assert_eq!(description(&s, "Window").as_deref(), Some("Pocket door"));
    assert_eq!(description(&s, "Door").as_deref(), Some("Hinged door"));
}

#[test]
fn undo_after_bsaveas_does_not_give_the_old_block_the_new_description() {
    let mut s = Session::new();
    door(&mut s);
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    s.execute("blocks.description", &json!({ "description": "Sliding door" })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [0, 3]] })).unwrap();
    s.execute("bsaveas", &json!({ "name": "Slider" })).unwrap();
    // Undo goes back past the line to the editor's drawing as it was before BSAVEAS.
    s.execute("undo", &json!({})).unwrap();
    s.execute("bsave", &json!({})).unwrap();
    assert_eq!(saved_description(&s, "Door").as_deref(), Some("Hinged door"));
    assert_eq!(saved_description(&s, "Slider").as_deref(), Some("Sliding door"));

    // In the editor, only the edited block's description changes.
    assert!(s.execute("blocks.description", &json!({ "name": "Door", "description": "x" })).is_err());
    s.execute("bclose", &json!({})).unwrap();
    assert_eq!(description(&s, "Door").as_deref(), Some("Hinged door"));
    assert_eq!(description(&s, "Slider").as_deref(), Some("Sliding door"));
}

#[test]
fn a_new_block_in_the_block_editor_takes_its_description() {
    let mut s = Session::new();
    s.execute("bedit", &json!({ "name": "Table", "create": true })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [5, 0]] })).unwrap();
    s.execute("blocks.description", &json!({ "description": "Desk" })).unwrap();
    assert_eq!(saved_description(&s, "Table"), None);

    // Saved under another name first, the started block is not created empty.
    s.execute("bsaveas", &json!({ "name": "Desk" })).unwrap();
    assert_eq!(saved_description(&s, "Desk").as_deref(), Some("Desk"));
    assert_eq!(saved_description(&s, "Table"), None);
    s.execute("bclose", &json!({})).unwrap();
    assert!(s.doc().unwrap().block("Table").is_none());

    s.execute("bedit", &json!({ "name": "Chair", "create": true })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [1, 0]] })).unwrap();
    s.execute("blocks.description", &json!({ "name": "chair", "description": "Office chair" })).unwrap();
    s.execute("bclose", &json!({})).unwrap();
    assert_eq!(description(&s, "Chair").as_deref(), Some("Office chair"));
    assert_eq!(s.doc().unwrap().block("Chair").unwrap().entities.len(), 1);
}
