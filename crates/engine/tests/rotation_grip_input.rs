//! The rotation grip of a dynamic block asks for an angle: Dynamic Input shows one angle box,
//! holding the angle the cursor would give as Properties will show it. The drawing is the
//! synthetic Symbol block of the io tests (Angle1 about its base, its grip at the radius' end).

use cadcraft_engine::Session;
use cadcraft_engine::doc::Handle;
use cadcraft_engine::doc::dynblock::GripPart;
use cadcraft_engine::geom::Vec2;
use serde_json::json;

fn dxf(src: &str) -> String {
    src.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (code, value) = l.split_once(' ').unwrap_or((l, ""));
            format!("{code}\n{value}\n")
        })
        .collect()
}

/// A session with Symbol and a reference to it at (100, 100).
fn session() -> (Session, Handle) {
    let src = include_str!("../../io/tests/data/dynblock_symbol.txt").replace("\r\n", "\n");
    let d = cadcraft_io::read_dxf(dxf(&src).as_bytes()).unwrap();
    let mut s = Session::empty();
    s.open_drawing(d, "symbol", None);
    let r = s.execute("insert", &json!({ "name": "Symbol", "at": [100, 100] })).unwrap();
    (s, Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap())
}

#[test]
fn the_rotation_grip_asks_for_an_angle_and_shows_it_live() {
    let (mut s, h) = session();
    s.start_dyn_grip(h, 15, GripPart::End).unwrap();
    assert!(s.current_prompt().is_some_and(|p| p.angle), "{:?}", s.current_prompt());
    // The grip is at (102, 100); straight above the base is a quarter turn.
    assert_eq!(s.live_value(Vec2::new(100.0, 102.0)).as_deref(), Some("90°"));
    assert_eq!(s.live_value(Vec2::new(98.0, 100.0)).as_deref(), Some("180°"));
    // Nothing is changed by looking.
    let props = s.execute("blocks.reference", &json!({ "handle": h.hex() })).unwrap();
    let angle = props["properties"].as_array().unwrap().iter().find(|p| p["name"] == "Angle1").unwrap()["value"].clone();
    assert_eq!(angle, json!("0"));
}
