//! LEADER annotation text height: DIMTXT times DIMSCALE (#1056). The leader arrows already
//! honour DIMSCALE; the MTEXT the leader creates must reach the same size, in the drawing and
//! in the saved DXF.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, MText};
use serde_json::{Value, json};

fn setvar(s: &mut Session, name: &str, value: Value) {
    s.execute("setvar", &json!({ "name": name, "value": value })).unwrap();
}

fn dimstyle(s: &mut Session, vars: Value) {
    let mut p = json!({ "name": "S", "current": true });
    if let Value::Object(o) = vars {
        for (k, v) in o {
            p[&k] = v;
        }
    }
    s.execute("dimstyle", &p).unwrap();
}

fn leader(s: &mut Session, text: Option<&str>) {
    let mut p = json!({ "points": [[0, 0], [20, 20], [30, 20]] });
    if let Some(t) = text {
        p["text"] = json!(t);
    }
    s.execute("leader", &p).unwrap();
}

fn leader_text(s: &Session) -> Option<MText> {
    s.doc().unwrap().model.iter().find_map(|e| match &e.kind {
        EntityKind::MText(m) => Some(m.clone()),
        _ => None,
    })
}

#[test]
fn dimtxt_times_dimscale_is_the_text_height() {
    let mut s = Session::new();
    dimstyle(&mut s, json!({ "DIMSCALE": 25, "DIMTXT": 0.25 }));
    leader(&mut s, Some("NOTE"));
    let m = leader_text(&s).expect("the leader text is an MTEXT");
    assert!((m.height - 6.25).abs() < 1e-9, "DIMTXT 0.25 x DIMSCALE 25 is 6.25, got {}", m.height);
    // The text offset above the landing follows the same height.
    assert!((m.insert.x - (30.0 + 6.25 / 2.0)).abs() < 1e-9, "insert {:?}", m.insert);
    assert!((m.insert.y - (20.0 + 6.25 / 2.0)).abs() < 1e-9, "insert {:?}", m.insert);
}

#[test]
fn the_default_scale_keeps_dimtxt() {
    let mut s = Session::new();
    leader(&mut s, Some("NOTE"));
    let m = leader_text(&s).unwrap();
    assert!((m.height - 0.18).abs() < 1e-9, "DIMTXT x DIMSCALE 1 is 0.18, got {}", m.height);
}

#[test]
fn a_zero_style_scale_uses_the_drawing_dimscale() {
    let mut s = Session::new();
    dimstyle(&mut s, json!({ "DIMSCALE": 0, "DIMTXT": 0.25 }));
    setvar(&mut s, "DIMSCALE", json!(2.5));
    leader(&mut s, Some("NOTE"));
    let m = leader_text(&s).unwrap();
    assert!((m.height - 0.625).abs() < 1e-9, "DIMTXT 0.25 x DIMSCALE 2.5 is 0.625, got {}", m.height);
}

#[test]
fn a_hostile_negative_scale_falls_back_to_one() {
    let mut s = Session::new();
    dimstyle(&mut s, json!({ "DIMTXT": 0.25 }));
    for st in &mut s.doc_mut().unwrap().dim_styles {
        st.scale = -5.0;
    }
    leader(&mut s, Some("NOTE"));
    let m = leader_text(&s).unwrap();
    assert!((m.height - 0.25).abs() < 1e-9, "a negative scale falls back to 1, got {}", m.height);
}

#[test]
fn the_saved_dxf_carries_the_scaled_height() {
    let mut s = Session::new();
    dimstyle(&mut s, json!({ "DIMSCALE": 25, "DIMTXT": 0.25 }));
    leader(&mut s, Some("NOTE"));
    let text = cadcraft_io::write_dxf(s.doc().unwrap());
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let m = back.model.iter().find_map(|e| match &e.kind {
        EntityKind::MText(m) => Some(m.clone()),
        _ => None,
    });
    assert!((m.expect("MTEXT in the DXF").height - 6.25).abs() < 1e-9);
}

#[test]
fn a_leader_without_text_adds_no_mtext() {
    let mut s = Session::new();
    leader(&mut s, None);
    assert!(leader_text(&s).is_none());
}
