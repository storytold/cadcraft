//! Multileader arrowheads and leader type None, and the multileader text grip (Move with
//! Leader, Move Text Only, Reset Text Position).

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use serde_json::json;

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

/// MLEADERSTYLE sets the style's arrowhead: a built-in one, a block, or back to closed filled.
#[test]
fn a_multileader_styles_arrowhead_is_set_by_the_command() {
    let mut s = Session::new();
    let arrow = |s: &Session| s.doc().unwrap().mleader_styles.iter().find(|m| m.name == "Standard").map(|m| m.arrow_block.clone());
    s.execute("mleaderstyle", &json!({ "name": "Standard", "arrowBlock": "_Open" })).unwrap();
    assert_eq!(arrow(&s).as_deref(), Some("_Open"));
    assert!(s.execute("mleaderstyle", &json!({ "name": "Standard", "arrowBlock": "NOSUCHBLOCK" })).is_err());
    s.execute("mleaderstyle", &json!({ "name": "Standard", "arrowBlock": "" })).unwrap();
    assert_eq!(arrow(&s).as_deref(), Some(""));
}

/// Moving a multileader's text from its grip asks for the text's location, not a dimension's
/// (#2329 review).
#[test]
fn multileader_text_grip_prompt_is_not_about_dimensions() {
    let mut s = Session::new();
    s.execute("mleader", &json!({ "points": [[0, 0], [5, 5]], "text": "A" })).unwrap();
    let (h, i) = {
        let e = s.doc().unwrap().model.iter().next().unwrap();
        let EntityKind::MLeader(m) = &e.kind else { panic!() };
        (e.handle, m.text_grip().unwrap())
    };
    s.start_grip_edit(&[(h, i)]).unwrap();
    s.input(cadcraft_engine::Input::Keyword("move with Leader".into())).unwrap();
    let p = s.prompt_text();
    assert!(p.contains("location") && !p.contains("dimension"), "{p}");
}
