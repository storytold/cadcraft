//! TEXT and MLEADER hand their text to the in-place editor once it is asked for, and show the
//! text's frame / the leader segment while points are picked.

use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{EditorKind, Input, Session};

fn texts(s: &Session) -> Vec<String> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Text(t) => Some(t.value.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn text_opens_a_line_editor_for_each_line_and_an_empty_line_ends_it() {
    let mut s = Session::new();
    s.start("text").unwrap();
    // Before the start point: the frame of a line of text follows the cursor.
    assert!(!s.preview(Vec2::new(1.0, 1.0)).is_empty());
    assert!(s.text_editor().is_none());
    s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    assert!(!s.preview(Vec2::new(0.0, 2.0)).is_empty(), "the frame grows with the height");
    s.input(Input::Enter).unwrap();
    s.input(Input::Enter).unwrap();
    let ed = s.text_editor().expect("the editor opens at the text prompt");
    assert_eq!(ed.kind, EditorKind::Line);
    let first_key = ed.key.clone();
    s.input(Input::Text("first".into())).unwrap();
    let ed = s.text_editor().expect("the next line gets an editor");
    assert_ne!(ed.key, first_key);
    assert!(ed.mtext.insert.y < 0.0, "the next line is below the first");
    s.input(Input::Text("second".into())).unwrap();
    s.input(Input::Text(String::new())).unwrap();
    assert!(s.running.is_none());
    assert_eq!(texts(&s), ["first", "second"]);
}

#[test]
fn mleader_opens_the_editor_after_its_leader_and_previews_the_segment() {
    let mut s = Session::new();
    s.start("mleader").unwrap();
    s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    assert!(!s.preview(Vec2::new(5.0, 5.0)).is_empty(), "the segment to the cursor shows");
    assert!(s.editing_mtext().is_none());
    s.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    let m = s.editing_mtext().expect("the editor opens at the landing");
    assert!(m.insert.x > 5.0 && m.contents.is_empty());
    s.input(Input::Text("NOTE".into())).unwrap();
    assert!(s.running.is_none());
    let found =
        s.doc().unwrap().model.iter().any(|e| matches!(&e.kind, EntityKind::MLeader(m) if m.text.as_ref().is_some_and(|t| t.contents == "NOTE")));
    assert!(found);
}

#[test]
fn leader_type_none_hides_the_leader_and_straight_brings_it_back() {
    let mut s = Session::new();
    let r = s.execute("mleader", &serde_json::json!({ "points": [[0, 0], [5, 5]], "text": "A" })).unwrap();
    let h = r["handle"].as_str().unwrap().to_string();
    let get = |s: &Session| {
        s.doc()
            .unwrap()
            .model
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::MLeader(m) => Some(m.clone()),
                _ => None,
            })
            .unwrap()
    };
    s.execute("properties.set", &serde_json::json!({ "handles": [h], "leaderType": "none" })).unwrap();
    let m = get(&s);
    assert!(m.leaderless() && !m.leaders.is_empty() && m.leader_paths().is_empty());
    s.execute("properties.set", &serde_json::json!({ "handles": [h], "leaderType": "straight" })).unwrap();
    assert!(!get(&s).leaderless());
}

#[test]
fn multileader_text_grip_moves_with_its_leader_and_resets() {
    use cadcraft_engine::grips::{GripAction, grip_action, grip_actions};
    let mut s = Session::new();
    s.execute("mleader", &serde_json::json!({ "points": [[0, 0], [5, 5]], "text": "A" })).unwrap();
    let k = s.doc().unwrap().model.iter().next().unwrap().kind.clone();
    let EntityKind::MLeader(m) = &k else { panic!("not a multileader") };
    let i = m.text_grip().expect("a text grip");
    assert_eq!(k.grips().len(), i + 1);
    assert!(grip_actions(&k, i).contains(&GripAction::MoveWithLeader));
    let moved = grip_action(&k, i, GripAction::MoveWithLeader, Some(Vec2::new(20.0, 12.0)), None).unwrap();
    let EntityKind::MLeader(m2) = &moved else { panic!() };
    let t = m2.text.as_ref().unwrap();
    assert!((t.insert.x - 20.0).abs() < 1e-9 && (t.insert.y - 12.0).abs() < 1e-9);
    assert!((m2.landing.y - 12.0).abs() < 1e-9 && m2.landing.x < 20.0, "the landing follows the text");
    assert_eq!(m2.leaders, m.leaders, "the leader points stay");
    // Text moved alone, then put back beside the landing.
    let alone = grip_action(&k, i, GripAction::MoveTextOnly, Some(Vec2::new(30.0, -4.0)), None).unwrap();
    let back = grip_action(&alone, i, GripAction::ResetTextPosition, None, None).unwrap();
    let EntityKind::MLeader(m3) = &back else { panic!() };
    let t3 = m3.text.as_ref().unwrap();
    assert!((t3.insert.y - m3.landing.y).abs() < 1e-9 && t3.insert.x > m3.landing.x);
}
