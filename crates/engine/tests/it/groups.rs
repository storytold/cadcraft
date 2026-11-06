//! Groups (#417): GROUP, UNGROUP and GROUPEDIT, typed and as JSON; erased members leave their
//! groups and empty groups go away. (The DXF round trip of groups is tested in
//! `cadcraft-io`'s `tables_objects_roundtrip`.)

use cadcraft_engine::doc::Handle;
use cadcraft_engine::{Input, Session};
use serde_json::json;

/// A session with three lines; their handles.
fn three_lines() -> (Session, Vec<Handle>) {
    let mut s = Session::new();
    for y in [0, 5, 10] {
        s.execute("line", &json!({ "points": [[0, y], [10, y]] })).unwrap();
    }
    let hs = s.doc().unwrap().model.iter().map(|e| e.handle).collect();
    (s, hs)
}

fn hex(hs: &[Handle]) -> Vec<String> {
    hs.iter().map(|h| h.hex()).collect()
}

fn groups(s: &Session) -> Vec<(String, String, Vec<Handle>)> {
    s.doc().unwrap().groups.iter().map(|g| (g.name.clone(), g.description.clone(), g.members.clone())).collect()
}

#[test]
fn group_json_named_and_unnamed() {
    let (mut s, hs) = three_lines();
    let r = s.execute("group", &json!({ "name": "Doors", "description": "front", "handles": hex(&hs[..2]) })).unwrap();
    assert_eq!(r, json!({ "name": "Doors", "members": 2 }));
    // Unnamed groups are *A1, *A2…; the current selection is the default.
    s.set_selection(vec![hs[2]]);
    let r = s.execute("group", &json!({})).unwrap();
    assert_eq!(r["name"], "*A1");
    assert_eq!(groups(&s), vec![("Doors".into(), "front".into(), hs[..2].to_vec()), ("*A1".into(), String::new(), vec![hs[2]])]);
    // A taken name (any case), an invalid name or nothing to group is refused.
    assert!(s.execute("group", &json!({ "name": "DOORS", "handles": hex(&hs) })).is_err());
    assert!(s.execute("group", &json!({ "name": "a*b", "handles": hex(&hs) })).is_err());
    s.set_selection(Vec::new());
    assert!(s.execute("group", &json!({ "name": "Empty" })).is_err());
    assert!(s.execute("group", &json!({ "name": "Gone", "handles": ["FFFFF"] })).is_err());
    assert!(s.execute("group", &json!({ "handles": "1A" })).is_err());
    assert_eq!(s.doc().unwrap().groups.len(), 2);
    let list = s.execute("groups.list", &json!({})).unwrap();
    assert_eq!(list["groups"][1]["unnamed"], true);
    assert_eq!(list["groups"][0]["members"], json!(hex(&hs[..2])));
    assert!(s.running.is_none(), "JSON calls never prompt");
}

#[test]
fn group_typed_name_description_and_undo() {
    let (mut s, hs) = three_lines();
    let undo = s.state().unwrap().undo.len();
    s.cmdline("group").unwrap();
    assert!(s.prompt_text().contains("Select objects or [Name/Description]"), "{}", s.prompt_text());
    s.cmdline("n").unwrap();
    assert!(s.prompt_text().contains("Enter a group name"), "{}", s.prompt_text());
    s.cmdline("Chairs").unwrap();
    s.cmdline("d").unwrap();
    s.cmdline("four legs each").unwrap();
    s.input(Input::Pick(vec![hs[0], hs[1]])).unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(groups(&s), vec![("Chairs".into(), "four legs each".into(), hs[..2].to_vec())]);
    assert!(s.selection().is_empty(), "picked objects don't stay selected");
    assert_eq!(s.state().unwrap().undo.len(), undo + 1, "one undo step");
    // A name in use is refused and asked again.
    s.cmdline("group n chairs").unwrap();
    assert!(s.log.iter().any(|l| l.contains("already exists")), "{:?}", s.log);
    assert!(s.prompt_text().contains("Enter a group name"), "{}", s.prompt_text());
    s.cancel();
    s.execute("u", &json!({})).unwrap();
    assert!(s.doc().unwrap().groups.is_empty(), "undo removes the group");
    // With a pickfirst selection GROUP makes an unnamed group at once.
    s.set_selection(vec![hs[2]]);
    s.cmdline("g").unwrap();
    assert!(s.running.is_none());
    assert_eq!(groups(&s), vec![("*A1".into(), String::new(), vec![hs[2]])]);
}

#[test]
fn ungroup_by_pick_name_and_json() {
    let (mut s, hs) = three_lines();
    s.execute("group", &json!({ "name": "A", "handles": hex(&hs[..2]) })).unwrap();
    s.execute("group", &json!({ "name": "B", "handles": hex(&hs[2..]) })).unwrap();
    s.execute("group", &json!({ "name": "C", "handles": hex(&hs[2..]) })).unwrap();
    // Typed: pick a member; a non-member is reported and asked again.
    s.execute("line", &json!({ "points": [[0, 20], [10, 20]] })).unwrap();
    let loose = s.doc().unwrap().model.last().unwrap().handle;
    s.cmdline("ungroup").unwrap();
    assert!(s.prompt_text().contains("Select group or [Name]"), "{}", s.prompt_text());
    s.input(Input::Pick(vec![loose])).unwrap();
    assert!(s.running.is_some());
    s.input(Input::Pick(vec![hs[1]])).unwrap();
    assert!(s.running.is_none());
    assert_eq!(groups(&s).iter().map(|g| g.0.as_str()).collect::<Vec<_>>(), ["B", "C"]);
    assert_eq!(s.doc().unwrap().model.len(), 4, "the objects stay");
    // By name.
    s.cmdline("ungroup n c").unwrap();
    assert!(s.running.is_none());
    assert_eq!(groups(&s).iter().map(|g| g.0.as_str()).collect::<Vec<_>>(), ["B"]);
    // JSON: unknown names and objects in no group are errors.
    assert!(s.execute("ungroup", &json!({ "name": "nope" })).is_err());
    assert!(s.execute("ungroup", &json!({ "handles": [loose.hex()] })).is_err());
    let r = s.execute("ungroup", &json!({ "handles": [hs[2].hex()] })).unwrap();
    assert_eq!(r, json!({ "ungrouped": ["B"] }));
    assert!(s.doc().unwrap().groups.is_empty());
}

#[test]
fn groupedit_add_remove_rename() {
    let (mut s, hs) = three_lines();
    s.execute("group", &json!({ "handles": hex(&hs[..1]) })).unwrap();
    // Typed: pick the group, add an object, then rename the unnamed group.
    s.cmdline("groupedit").unwrap();
    s.input(Input::Pick(vec![hs[0]])).unwrap();
    assert!(s.prompt_text().contains("[Add objects/Remove objects/REName]"), "{}", s.prompt_text());
    s.cmdline("a").unwrap();
    s.input(Input::Pick(vec![hs[1]])).unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(groups(&s), vec![("*A1".into(), String::new(), hs[..2].to_vec())]);
    s.cmdline("groupedit n *a1 ren Pair").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(groups(&s)[0].0, "Pair");
    // Remove one; removing the last deletes the group.
    s.cmdline("groupedit n pair r").unwrap();
    s.input(Input::Pick(vec![hs[0]])).unwrap();
    s.cmdline("").unwrap();
    assert_eq!(groups(&s), vec![("Pair".into(), String::new(), vec![hs[1]])]);
    // JSON.
    let r = s.execute("groupedit", &json!({ "name": "pair", "add": hex(&hs), "rename": "Trio" })).unwrap();
    assert_eq!(r, json!({ "name": "Trio", "members": 3 }));
    assert!(s.execute("groupedit", &json!({ "name": "Trio", "add": ["FFFFF"] })).is_err());
    assert!(s.execute("groupedit", &json!({ "name": "Trio", "rename": "bad?" })).is_err());
    assert!(s.execute("groupedit", &json!({ "name": "nope" })).is_err());
    let r = s.execute("groupedit", &json!({ "name": "Trio", "remove": hex(&hs) })).unwrap();
    assert_eq!(r["deleted"], true);
    assert!(s.doc().unwrap().groups.is_empty());
}

#[test]
fn erased_members_leave_groups() {
    let (mut s, hs) = three_lines();
    s.execute("group", &json!({ "name": "A", "handles": hex(&hs[..2]) })).unwrap();
    s.execute("group", &json!({ "name": "B", "handles": hex(&hs[2..]) })).unwrap();
    s.execute("erase", &json!({ "handles": [hs[0].hex(), hs[2].hex()] })).unwrap();
    assert_eq!(groups(&s), vec![("A".into(), String::new(), vec![hs[1]])], "B lost its only member");
    s.execute("u", &json!({})).unwrap();
    assert_eq!(groups(&s).len(), 2, "undo restores both groups");
    // Typed ERASE too.
    s.cmdline("erase").unwrap();
    s.input(Input::Pick(vec![hs[0], hs[1]])).unwrap();
    s.cmdline("").unwrap();
    assert_eq!(groups(&s).iter().map(|g| g.0.as_str()).collect::<Vec<_>>(), ["B"]);
}
