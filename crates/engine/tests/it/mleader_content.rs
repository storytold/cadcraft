//! MLEADER leader types (straight, spline, none) and block content, at the command line and in
//! the JSON form (#408).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, MLeader};
use cadcraft_engine::geom::Vec2;
use serde_json::json;

fn last_mleader(s: &Session) -> MLeader {
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::MLeader(m) => m.clone(),
        other => panic!("not a multileader: {other:?}"),
    }
}

fn logged(s: &Session, text: &str) -> bool {
    s.log.iter().any(|l| l.contains(text))
}

/// A session with block "TAG": a 2 x 2 square around its base point (0, 0).
fn with_block() -> Session {
    let mut s = Session::new();
    let sq = s.execute("pline", &json!({"vertices": [[-1, -1], [1, -1], [1, 1], [-1, 1]], "closed": true})).unwrap()["handle"].clone();
    s.execute("block", &json!({"name": "TAG", "base": [-1, -1], "handles": [sq], "keep": "delete"})).unwrap();
    s
}

#[test]
fn spline_leader_curves_through_its_points() {
    let mut s = Session::new();
    s.script("mleader o l p m 3 x 0,0 5,5 10,0 note\n").unwrap();
    let m = last_mleader(&s);
    assert!(m.spline && m.leaders[0].len() == 2);
    let path = &m.leader_paths()[0];
    assert!(path.len() > 3, "tessellated: {path:?}");
    assert!(path.first().unwrap().near(Vec2::ZERO, 1e-9) && path.last().unwrap().near(Vec2::new(10.0, 0.0), 1e-9));
    // The straight leader runs along y = x up to the middle point; the curve rounds the corner above it.
    assert!(path.iter().any(|p| p.x > 2.0 && p.x < 4.0 && p.y > p.x + 0.1), "not a smooth curve: {path:?}");
    // Remembered, and Straight switches back.
    s.script("mleader 0,0 5,5 10,0 again\n").unwrap();
    assert!(last_mleader(&s).spline);
    s.script("mleader o l s x 0,0 5,5 10,0 straight\n").unwrap();
    assert!(!last_mleader(&s).spline);
}

#[test]
fn no_leader_places_only_the_content() {
    let mut s = Session::new();
    s.script("mleader o l n x 3,4 Only text\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let m = last_mleader(&s);
    assert!(m.leaders.is_empty() && m.dogleg == 0.0);
    let t = m.text.unwrap();
    assert_eq!(t.contents, "Only text");
    assert!(t.insert.xy().near(Vec2::new(3.0, 4.0), 1e-9) && t.attach == 1);
    // No leader and no content: nothing to draw.
    s.script("mleader o c n x 1,1\n").unwrap();
    assert!(logged(&s, "nothing was drawn"));
    assert_eq!(s.doc().unwrap().model.iter().count(), 1);
    assert!(s.running.is_none());
}

#[test]
fn block_content_inserts_the_block_at_the_landing() {
    let mut s = with_block();
    // Unknown names are reported and asked again; Center extents puts the block's centre at the
    // end of the landing.
    s.script("mleader o c b nosuch\n").unwrap();
    assert!(logged(&s, "Block \"nosuch\" not found."));
    s.script("tag c x 0,0 10,5\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let m = last_mleader(&s);
    assert!(m.text.is_none());
    let b = m.block.clone().expect("block content");
    assert_eq!(b.block, "TAG");
    let end = m.landing.xy() + Vec2::new(m.dogleg, 0.0);
    // Block base (-1,-1) maps to the insertion point; the square's centre is base + (1, 1).
    assert!((b.insert.xy() + Vec2::new(1.0, 1.0)).near(end, 1e-9), "{:?} vs {end:?}", b.insert);
    // Insertion point attachment, remembered for the next MLEADER together with the block.
    s.script("mleader o c b tag i x 0,0 10,5\n").unwrap();
    let m = last_mleader(&s);
    assert!(m.block.unwrap().insert.xy().near(m.landing.xy() + Vec2::new(m.dogleg, 0.0), 1e-9));
    s.script("mleader 20,0 30,5\n").unwrap();
    assert!(last_mleader(&s).block.is_some(), "block content remembered");
    // Block content without a leader line.
    s.script("mleader o l n x 50,50\n").unwrap();
    let m = last_mleader(&s);
    assert!(m.leaders.is_empty() && m.block.unwrap().insert.xy().near(Vec2::new(50.0, 50.0), 1e-9));
}

#[test]
fn block_content_keeps_the_block_alive() {
    let mut s = with_block();
    let h = s.execute("mleader", &json!({"points": [[0, 0], [10, 5]], "block": "tag", "blockAttach": "insertion"})).unwrap()["handle"].clone();
    // PURGE keeps a block used only as multileader content.
    s.execute("purge", &json!({"type": "blocks"})).ok();
    assert!(s.doc().unwrap().block("TAG").is_some(), "purged a block in use");
    // RENAME follows into the multileader.
    s.execute("rename", &json!({"table": "block", "from": "TAG", "to": "NOTE"})).unwrap();
    assert_eq!(last_mleader(&s).block.unwrap().block, "NOTE");
    // MOVE carries the block along.
    let before = last_mleader(&s).block.unwrap().insert.xy();
    s.execute("move", &json!({"handles": [h], "from": [0, 0], "to": [5, 5]})).unwrap();
    assert!(last_mleader(&s).block.unwrap().insert.xy().near(before + Vec2::new(5.0, 5.0), 1e-9));
}

#[test]
fn json_form_takes_leader_type_and_block() {
    let mut s = with_block();
    s.execute("mleader", &json!({"points": [[0, 0], [5, 5], [10, 0]], "text": "s", "leaderType": "spline"})).unwrap();
    assert!(last_mleader(&s).spline);
    s.execute("mleader", &json!({"points": [[7, 7]], "text": "alone", "leaderType": "none"})).unwrap();
    let m = last_mleader(&s);
    assert!(m.leaders.is_empty() && m.text.unwrap().insert.xy().near(Vec2::new(7.0, 7.0), 1e-9));
    for bad in [
        json!({"points": [[0, 0], [5, 5]], "leaderType": "wavy"}),
        json!({"points": [[0, 0], [5, 5]], "block": "missing"}),
        json!({"points": [[0, 0], [5, 5]], "block": "tag", "blockAttach": "corner"}),
        json!({"points": [[0, 0]]}),
    ] {
        assert!(s.execute("mleader", &bad).is_err(), "{bad}");
    }
}

/// DIMSCALE belongs to dimension styles: a multileader keeps its style's text height, arrowhead
/// and dogleg whatever DIMSCALE is (#1648).
#[test]
fn mleader_sizes_ignore_dimscale() {
    let mut s = Session::new();
    s.execute("mleaderstyle", &json!({"name": "M5", "textHeight": 5, "arrowSize": 5, "current": true})).unwrap();
    s.execute("setvar", &json!({"name": "DIMSCALE", "value": 20})).unwrap();
    s.execute("mleader", &json!({"points": [[0, 0], [30, 20]], "text": "CADCRAFT"})).unwrap();
    let m = last_mleader(&s);
    assert_eq!(m.arrow_size, 5.0);
    let st = s.doc().unwrap().mleader_styles.iter().find(|x| x.name == "M5").unwrap().clone();
    assert_eq!(m.dogleg, st.dogleg);
    let t = m.text.unwrap();
    assert_eq!(t.height, 5.0);
    assert!((t.insert.x - (30.0 + st.dogleg + st.landing_gap)).abs() < 1e-9, "{}", t.insert.x);
}
