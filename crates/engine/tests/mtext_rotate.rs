//! ROTATE turns MTEXT about the base point, its own angle adding to the turn, and the Properties
//! command sets any angle without touching the text (#900).

use cadcraft_doc::{EntityKind, MText};
use cadcraft_engine::Session;
use serde_json::json;

fn mtext(s: &Session) -> MText {
    s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::MText(t) = &e.kind { Some(t.clone()) } else { None }).unwrap()
}

#[test]
fn rotate_turns_the_text_and_its_insertion_point() {
    let mut s = Session::new();
    s.script("MTEXT 0,10 r 30 20,0\nHello world\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    for line in ["ROTATE", "ALL", "", "0,0", "45"] {
        s.cmdline(line).unwrap();
    }
    let t = mtext(&s);
    assert!((t.rotation.to_degrees() - 75.0).abs() < 1e-9, "{}", t.rotation.to_degrees());
    let r = std::f64::consts::FRAC_1_SQRT_2 * 10.0;
    assert!(t.insert.xy().near(cadcraft_geom::Vec2::new(-r, r), 1e-9), "{:?}", t.insert);
    assert_eq!(t.contents, "Hello world");
}

#[test]
fn properties_set_rotation_changes_only_the_angle() {
    let mut s = Session::new();
    s.script("MTEXT 0,10 20,0\nHello world\n\n").unwrap();
    let before = mtext(&s);
    let h = s.doc().unwrap().model.iter().last().unwrap().handle;
    for degrees in [90.0, 225.0, 0.0] {
        s.execute("properties.set", &json!({"handles": [h.hex()], "rotation": degrees})).unwrap();
        let t = mtext(&s);
        assert!((t.rotation.to_degrees() - degrees).abs() < 1e-9, "{degrees}: {}", t.rotation.to_degrees());
        assert_eq!(
            (&t.contents, t.height, t.width, t.attach, t.insert),
            (&before.contents, before.height, before.width, before.attach, before.insert)
        );
    }
}
