//! MLEADERSTYLE's content, leader and block settings (gap F10): set with `mleaderstyle`, taken
//! by new multileaders (JSON form and the MLEADER prompts), checked before anything changes, and
//! kept by PURGE and RENAME for the style's block.

use cadcraft_engine::Input;
use cadcraft_engine::Session;
use cadcraft_engine::doc::color::Color;
use cadcraft_engine::doc::{EntityKind, Lineweight, MLeader, MLeaderContent, MLeaderLineType, MLeaderStyle};
use cadcraft_engine::geom::Vec2;
use serde_json::json;

fn last_mleader(s: &Session) -> MLeader {
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::MLeader(m) => m.clone(),
        other => panic!("not a multileader: {other:?}"),
    }
}

fn style(s: &Session, name: &str) -> MLeaderStyle {
    s.doc().unwrap().mleader_styles.iter().find(|x| x.name == name).cloned().unwrap()
}

/// A session with block "TAG" (a 2 x 2 square around its base point).
fn with_block() -> Session {
    let mut s = Session::new();
    let sq = s.execute("pline", &json!({"vertices": [[-1, -1], [1, -1], [1, 1], [-1, 1]], "closed": true})).unwrap()["handle"].clone();
    s.execute("block", &json!({"name": "TAG", "base": [0, 0], "handles": [sq], "keep": "delete"})).unwrap();
    s
}

#[test]
fn new_multileaders_take_the_styles_leader_text_and_scale() {
    let mut s = Session::new();
    s.execute(
        "mleaderstyle",
        &json!({"name": "Red", "arrowSize": 1, "textHeight": 2, "dogleg": 3, "landingGap": 0.5, "scale": 2, "leaderType": "spline",
                "leaderColor": 1, "leaderLineweight": 0.35, "textColor": 5, "textFrame": true, "defaultText": "SEE NOTE"}),
    )
    .unwrap();
    s.execute("mleader", &json!({"points": [[0, 0], [5, 5], [10, 5]]})).unwrap();
    let m = last_mleader(&s);
    assert!(m.spline, "spline leader from the style");
    assert_eq!((m.arrow_size, m.dogleg), (2.0, 6.0), "sizes times the style's scale");
    let t = m.text.as_ref().unwrap();
    assert_eq!((t.contents.as_str(), t.height), ("SEE NOTE", 4.0), "the style's default text");
    assert_eq!((m.leader_color, m.leader_lineweight, m.text_color, m.text_frame), (Color::Index(1), Lineweight::Mm100(35), Color::Index(5), true));
    // An explicit leader type and text win over the style's.
    s.execute("mleader", &json!({"points": [[0, 0], [5, 5]], "leaderType": "straight", "text": "A"})).unwrap();
    let m = last_mleader(&s);
    assert!(!m.spline && m.text.as_ref().is_some_and(|t| t.contents == "A"));
    // No landing.
    s.execute("mleaderstyle", &json!({"name": "Red", "landing": false})).unwrap();
    s.execute("mleader", &json!({"points": [[0, 0], [5, 5]], "text": "B"})).unwrap();
    assert_eq!(last_mleader(&s).dogleg, 0.0);
}

#[test]
fn block_content_comes_from_the_style() {
    let mut s = with_block();
    // Block content without a block is refused, and changes nothing.
    assert!(s.execute("mleaderstyle", &json!({"name": "Standard", "content": "block", "textFrame": true})).is_err());
    assert!(!style(&s, "Standard").text_frame);
    assert!(s.execute("mleaderstyle", &json!({"name": "Standard", "block": "NOSUCH"})).is_err());
    s.execute(
        "mleaderstyle",
        &json!({"name": "Standard", "content": "block", "block": "tag", "blockScale": 2, "blockRotation": 90, "blockCenter": false, "blockColor": 3}),
    )
    .unwrap();
    assert_eq!(style(&s, "Standard").block, "TAG", "the block's own name");
    s.execute("mleader", &json!({"points": [[0, 0], [10, 5]]})).unwrap();
    let m = last_mleader(&s);
    let b = m.block.as_ref().expect("block content");
    assert_eq!(b.block, "TAG");
    assert_eq!(b.scale.x, 2.0);
    assert!((b.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert_eq!(m.block_color, Color::Index(3));
    assert!(m.text.is_none());
    // By its insertion point: at the end of the landing.
    assert!((b.insert.xy() - Vec2::new(10.0 + m.dogleg, 5.0)).len() < 1e-9, "{:?}", b.insert);
    // PURGE keeps the style's block; RENAME renames it in the style.
    s.execute("purge", &json!({"type": "blocks"})).unwrap();
    assert!(s.doc().unwrap().block("TAG").is_some());
    s.execute("rename", &json!({"table": "block", "from": "TAG", "to": "PIN"})).unwrap();
    assert_eq!(style(&s, "Standard").block, "PIN");
}

#[test]
fn mleader_prompts_start_from_the_style() {
    let mut s = with_block();
    s.execute("mleaderstyle", &json!({"name": "Bubble", "content": "block", "block": "TAG", "leaderType": "spline", "maxPoints": 3})).unwrap();
    s.start("mleader").unwrap();
    for p in [Vec2::new(0.0, 0.0), Vec2::new(4.0, 6.0), Vec2::new(10.0, 6.0)] {
        s.input(Input::Point(p)).unwrap();
    }
    assert!(s.running.is_none(), "three points (the style's maximum) finish it: {}", s.prompt_text());
    let m = last_mleader(&s);
    assert!(m.spline);
    assert_eq!(m.block.as_ref().map(|b| b.block.as_str()), Some("TAG"));
    assert_eq!(m.leaders.first().map(Vec::len), Some(2));
}

#[test]
fn hostile_style_values_are_refused() {
    let mut s = Session::new();
    for bad in [
        json!({"content": "picture"}),
        json!({"leaderType": 7}),
        json!({"maxPoints": 1000}),
        json!({"maxPoints": f64::NAN}),
        json!({"firstAngle": -5}),
        json!({"scale": 0}),
        json!({"blockScale": 1e300}),
        json!({"leaderColor": "chartreuse-ish"}),
        json!({"leaderLinetype": "NOSUCH"}),
        json!({"landing": "yes"}),
    ] {
        let mut p = bad.clone();
        p["name"] = json!("Standard");
        assert!(s.execute("mleaderstyle", &p).is_err(), "{bad}");
    }
    s.execute("mleader", &json!({"points": [[0, 0], [5, 5]], "text": "OK"})).unwrap();
    assert_eq!(last_mleader(&s).leader_color, Color::ByBlock);
    let st = style(&s, "Standard");
    assert_eq!((st.content, st.leader_type, st.max_points), (MLeaderContent::MText, MLeaderLineType::Straight, 2));
}
