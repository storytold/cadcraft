//! A multileader's landing (dogleg) length and arrowhead size through `properties.set`, and
//! EXPLODE of a multileader into the objects it draws (#3134).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle, MLeader};
use serde_json::json;

fn mleader(s: &mut Session) -> Handle {
    s.execute("mleader", &json!({"points": [[0, 0], [10, 10]], "text": "LABEL"})).unwrap();
    s.doc().unwrap().model.last().unwrap().handle
}

fn get(s: &Session, h: Handle) -> MLeader {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::MLeader(m) => m.clone(),
        other => panic!("not a multileader: {other:?}"),
    }
}

#[test]
fn properties_set_changes_dogleg_and_arrow_size() {
    let mut s = Session::new();
    let h = mleader(&mut s);
    let hex = format!("{:X}", h.0);
    let r = s.execute("properties.set", &json!({"handles": [hex], "dogleg": 0.1, "arrowSize": 0.05})).unwrap();
    assert_eq!(r["changed"], json!(1));
    let m = get(&s, h);
    assert_eq!((m.dogleg, m.arrow_size), (0.1, 0.05));
    // Zero is allowed (no landing, no arrowhead); a negative or non-numeric value is an error
    // and changes nothing.
    s.execute("properties.set", &json!({"handles": [hex], "dogleg": 0.0, "arrowSize": 0.0})).unwrap();
    assert_eq!((get(&s, h).dogleg, get(&s, h).arrow_size), (0.0, 0.0));
    for bad in [json!({"dogleg": -1.0}), json!({"dogleg": "long"}), json!({"arrowSize": -1.0}), json!({"arrowSize": "big"})] {
        let mut p = bad.clone();
        p["handles"] = json!([hex]);
        p["height"] = json!(7.0);
        assert!(s.execute("properties.set", &p).is_err(), "{bad}");
    }
    let m = get(&s, h);
    assert_eq!((m.dogleg, m.arrow_size), (0.0, 0.0));
    assert_ne!(m.text.map(|t| t.height), Some(7.0));
}

#[test]
fn explode_turns_a_multileader_into_mtext_lines_and_a_solid() {
    let mut s = Session::new();
    let h = mleader(&mut s);
    let m = get(&s, h);
    let hex = format!("{:X}", h.0);
    let r = s.execute("explode", &json!({"handles": [hex]})).unwrap();
    let new = r["handles"].as_array().unwrap();
    assert!(!new.is_empty(), "{r}");
    let d = s.doc().unwrap();
    assert!(d.entity(h).is_none(), "the multileader is replaced");
    let kinds: Vec<&EntityKind> = d.model.iter().map(|e| &e.kind).collect();
    assert!(!kinds.iter().any(|k| matches!(k, EntityKind::MLeader(_))));
    let text = kinds.iter().find_map(|k| if let EntityKind::MText(t) = k { Some(t) } else { None }).expect("MTEXT");
    assert_eq!(Some(&text.contents), m.text.as_ref().map(|t| &t.contents));
    // The leader line from the arrow tip to the landing, then the landing line.
    let lines: Vec<_> = kinds.iter().filter_map(|k| if let EntityKind::Line(l) = k { Some((l.a.xy(), l.b.xy())) } else { None }).collect();
    assert_eq!(lines.len(), 2, "{kinds:?}");
    assert!(lines.iter().any(|(a, b)| a.near(m.leaders[0][0].xy(), 1e-9) && b.near(m.landing.xy(), 1e-9)), "{lines:?}");
    assert!(lines.iter().any(|(a, b)| a.near(m.landing.xy(), 1e-9) && (b.dist(*a) - m.dogleg).abs() < 1e-9), "{lines:?}");
    // The arrowhead: a filled triangle with its tip at the first leader point.
    let solid = kinds.iter().find_map(|k| if let EntityKind::Solid(so) = k { Some(so) } else { None }).expect("SOLID");
    assert!(solid.corners[0].xy().near(m.leaders[0][0].xy(), 1e-9));
}
