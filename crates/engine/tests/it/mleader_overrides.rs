//! A multileader's own leader colour, linetype and lineweight, text colour and frame and block
//! colour (gap FF2): set with `properties.set`, copied by MATCHPROP, refused when bad, kept by
//! undo.

use cadcraft_engine::Session;
use cadcraft_engine::doc::color::{Color, Rgb};
use cadcraft_engine::doc::{EntityKind, Lineweight, MLeader};
use serde_json::{Value, json};

fn ml(s: &Session, h: &Value) -> MLeader {
    let h = cadcraft_engine::doc::Handle::parse_hex(h.as_str().unwrap()).unwrap();
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::MLeader(m) => m.clone(),
        other => panic!("not a multileader: {other:?}"),
    }
}

#[test]
fn properties_set_the_multileaders_own_overrides() {
    let mut s = Session::new();
    s.execute("linetype", &json!({"load": "DASHED"})).ok();
    let h = s.execute("mleader", &json!({"points": [[0, 0], [5, 5]], "text": "NOTE"})).unwrap()["handle"].clone();
    let m = ml(&s, &h);
    assert_eq!((m.leader_color, m.text_color, m.block_color, m.text_frame), (Color::ByBlock, Color::ByBlock, Color::ByBlock, false));
    assert_eq!((m.leader_linetype.as_str(), m.leader_lineweight), ("ByBlock", Lineweight::ByBlock));
    let dashed = s.doc().unwrap().linetype("DASHED").is_some();
    let mut p =
        json!({"handles": [h], "leaderColor": 1, "textColor": "10,20,30", "textFrame": true, "leaderLineweight": 0.5, "blockColor": "ByLayer"});
    if dashed {
        p["leaderLinetype"] = json!("dashed");
    }
    s.execute("properties.set", &p).unwrap();
    let m = ml(&s, &h);
    assert_eq!((m.leader_color, m.text_color, m.block_color), (Color::Index(1), Color::True(Rgb(10, 20, 30)), Color::ByLayer));
    assert!(m.text_frame);
    assert_eq!(m.leader_lineweight, Lineweight::Mm100(50));
    if dashed {
        assert_eq!(m.leader_linetype, "DASHED", "the loaded linetype's own name");
    }
    // Bad values are refused before anything changes.
    for bad in
        [json!({"leaderColor": "purple-ish"}), json!({"leaderLinetype": "NOSUCH"}), json!({"textFrame": 3}), json!({"leaderLineweight": "heavy"})]
    {
        let mut p = bad.clone();
        p["handles"] = json!([h]);
        assert!(s.execute("properties.set", &p).is_err(), "{bad}");
    }
    assert_eq!(ml(&s, &h).leader_color, Color::Index(1));
    // Undo takes an override back.
    s.execute("properties.set", &json!({"handles": [h], "leaderColor": "ByBlock"})).unwrap();
    assert_eq!(ml(&s, &h).leader_color, Color::ByBlock);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(ml(&s, &h).leader_color, Color::Index(1));
}

#[test]
fn matchprop_copies_the_overrides() {
    let mut s = Session::new();
    let a = s.execute("mleader", &json!({"points": [[0, 0], [5, 5]], "text": "A"})).unwrap()["handle"].clone();
    let b = s.execute("mleader", &json!({"points": [[20, 0], [25, 5]], "text": "B"})).unwrap()["handle"].clone();
    s.execute("properties.set", &json!({"handles": [a], "leaderColor": 3, "textFrame": true, "textColor": 5})).unwrap();
    s.execute("matchprop", &json!({"source": a, "targets": [b]})).unwrap();
    let m = ml(&s, &b);
    assert_eq!((m.leader_color, m.text_color, m.text_frame), (Color::Index(3), Color::Index(5), true));
}
