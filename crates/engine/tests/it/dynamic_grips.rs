//! Grips of dynamic block references: listed, dragged by `blocks.reference.grip` (one undo
//! step), and dragged interactively the way a click on a grip starts it, with a preview and a
//! typed distance. The drawing is the synthetic DOOR block of the io tests, with grips added.

use cadcraft_engine::doc::dynblock::GripPart;
use cadcraft_engine::doc::{Handle, Space};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

/// DXF text from lines of `code value`.
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

/// A session with DOOR (Width's end grip 9 shown in the Open state, the visibility grip 10) and
/// a reference to it at (100, 0).
fn session() -> (Session, Handle) {
    session_with(|src| src)
}

/// As [`session`], with the fixture's text changed by `edit` first.
fn session_with(edit: impl Fn(String) -> String) -> (Session, Handle) {
    let grip = "0 BLOCKLINEARGRIP\n5 68\n330 61\n100 AcDbEvalExpr\n90 9\n98 33\n99 232\n100 AcDbBlockElement\n300 End Grip\n98 33\n99 232\n1071 0\n";
    let src = include_str!("../../../io/tests/data/dynblock_door.txt")
        .replace("\r\n", "\n")
        .replace("170 4\n91 0\n91 0\n", "170 4\n91 8\n91 9\n")
        .replace("1030 0\n93 0\n170 0\n171 0\n100 AcDbBlockVisibilityParameter", "1030 0\n93 10\n170 0\n171 0\n100 AcDbBlockVisibilityParameter")
        .replace("91 5\n93 32\n95 6\n360 67", "91 5\n93 32\n95 6\n360 67\n92 -1\n92 -1\n92 -1\n92 -1\n91 6\n93 32\n95 9\n360 68")
        .replace("95 1\n333 63\n", "95 2\n333 63\n333 68\n")
        .replace("0 BLOCKFLIPGRIP", &format!("{grip}0 BLOCKFLIPGRIP"));
    let d = cadcraft_io::read_dxf(dxf(&edit(src)).as_bytes()).unwrap();
    let mut s = Session::empty();
    s.open_drawing(d, "door", None);
    let r = s.execute("insert", &json!({ "name": "Door", "at": [100, 0] })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    assert!(s.doc().unwrap().space_of(h) == Some(Space::Model));
    (s, h)
}

fn width(s: &mut Session, h: Handle) -> String {
    let r = s.execute("blocks.reference", &json!({ "handle": h.hex() })).unwrap();
    let p = r["properties"].as_array().unwrap().iter().find(|p| p["name"] == "Width").cloned().unwrap_or(Value::Null);
    p["value"].as_str().unwrap_or_default().to_string()
}

#[test]
fn grips_are_listed_and_dragged_in_one_undo_step() {
    let (mut s, h) = session();
    let g = s.execute("blocks.reference.grips", &json!({ "handle": h.hex() })).unwrap();
    let parts: Vec<(i64, &str, &str)> =
        g.as_array().unwrap().iter().map(|g| (g["node"].as_i64().unwrap(), g["part"].as_str().unwrap(), g["shape"].as_str().unwrap())).collect();
    assert_eq!(parts, [(1, "at", "list"), (2, "end", "linear")]);
    assert_eq!(g[1]["at"], json!([136.0, -4.0]));
    s.execute("blocks.reference.grip", &json!({ "handle": h.hex(), "node": 2, "part": "end", "to": [130.4, 7] })).unwrap();
    assert_eq!(width(&mut s, h), "30.0000");
    s.undo().unwrap();
    assert_eq!(width(&mut s, h), "36.0000");
    for bad in [
        json!({ "handle": h.hex(), "node": 2, "part": "middle", "to": [130, 0] }),
        json!({ "handle": h.hex(), "node": 2, "part": "end" }),
        json!({ "handle": h.hex(), "part": "end", "to": [130, 0] }),
        json!({ "handle": h.hex(), "node": 1, "part": "at", "to": [130, 0] }),
        json!({ "handle": h.hex(), "node": 2, "part": "end", "to": [10, 0] }),
        json!({ "handle": "FFFFF", "node": 2, "part": "end", "to": [130, 0] }),
    ] {
        assert!(s.execute("blocks.reference.grip", &bad).is_err(), "{bad}");
    }
    assert_eq!(width(&mut s, h), "36.0000");
}

#[test]
fn a_grip_drag_previews_the_reference_and_takes_a_point_or_a_distance() {
    let (mut s, h) = session();
    s.start_dyn_grip(h, 2, GripPart::End).unwrap();
    // The preview is the reference re-evaluated at the cursor, and the rubber band.
    assert!(s.preview(Vec2::new(132.0, 0.0)).len() > 1);
    s.input(Input::Point(Vec2::new(132.2, 3.0))).unwrap();
    assert!(s.running.is_none());
    assert_eq!(width(&mut s, h), "32.0000");
    // A typed distance (snapped to the list).
    s.start_dyn_grip(h, 2, GripPart::End).unwrap();
    s.input(Input::Text("29".into())).unwrap();
    assert_eq!(width(&mut s, h), "30.0000");
    // A list grip offers its choices instead; a grip that isn't there is refused.
    assert!(s.start_dyn_grip(h, 1, GripPart::At).is_err());
    assert!(s.start_dyn_grip(h, 2, GripPart::Base).is_err());
}

/// Width with its base grip too (grip 8, shown in the Open state).
fn with_base_grip(src: String) -> String {
    let base = "0 BLOCKLINEARGRIP\n5 69\n330 61\n100 AcDbEvalExpr\n90 8\n98 33\n99 232\n100 AcDbBlockElement\n300 Base Grip\n98 33\n99 232\n1071 0\n";
    src.replace("95 9\n360 68", "95 9\n360 68\n92 -1\n92 -1\n92 -1\n92 -1\n91 7\n93 32\n95 8\n360 69")
        .replace("95 2\n333 63\n333 68\n", "95 3\n333 63\n333 68\n333 69\n")
        .replace("0 BLOCKFLIPGRIP", &format!("{base}0 BLOCKFLIPGRIP"))
}

fn parts(s: &mut Session, h: Handle) -> Vec<(i64, String)> {
    let g = s.execute("blocks.reference.grips", &json!({ "handle": h.hex() })).unwrap();
    g.as_array().unwrap().iter().map(|g| (g["node"].as_i64().unwrap(), g["part"].as_str().unwrap().to_string())).collect()
}

#[test]
fn an_end_no_action_follows_has_no_grip() {
    // Width's stretch follows its end only: its base grip, though the block has one, moves
    // nothing and isn't shown (as AutoCAD shows the reference).
    let (mut s, h) = session_with(with_base_grip);
    assert_eq!(parts(&mut s, h), [(1, "at".to_string()), (2, "end".to_string())]);
    // A stretch that follows the base instead: the base grip, not the end's.
    let (mut s, h) = session_with(|src| with_base_grip(src).replace("301 EndXDelta\n302 EndYDelta", "301 BaseXDelta\n302 BaseYDelta"));
    assert_eq!(parts(&mut s, h), [(1, "at".to_string()), (2, "base".to_string())]);
}

#[test]
fn a_state_that_lists_a_parameter_shows_its_grips() {
    // Open lists Width but not its grip: the grip shows (as AutoCAD shows it); Closed lists
    // neither, and the grip doesn't.
    let (mut s, h) = session_with(|src| src.replace("95 2\n333 63\n333 68\n", "95 1\n333 63\n"));
    assert_eq!(parts(&mut s, h), [(1, "at".to_string()), (2, "end".to_string())]);
    s.execute("blocks.reference.set", &json!({ "handle": h.hex(), "name": "Visibility1", "value": "Closed" })).unwrap();
    assert_eq!(parts(&mut s, h), [(1, "at".to_string())]);
}

#[test]
fn a_grip_drag_previews_the_reference_in_its_own_colours() {
    // The reference on layer Doors: the preview's block objects (on layer 0 in the block) draw on
    // Doors as the reference does, not in the preview colour; the rubber band is the current
    // layer's.
    let (mut s, h) = session();
    s.doc_mut().unwrap().modify_entity(h, |e| e.common.layer = "Doors".into()).unwrap();
    s.start_dyn_grip(h, 2, GripPart::End).unwrap();
    let preview = s.preview(Vec2::new(132.0, 0.0));
    // The block's objects first, then the rubber band (and any ticks) on the current layer, in
    // the grip blue so they show over the block's fills.
    let layers: Vec<&str> = preview.iter().map(|e| e.common.layer.as_str()).collect();
    let n = layers.iter().take_while(|l| **l == "Doors").count();
    assert!(n > 0 && n < layers.len() && layers[n..].iter().all(|l| *l == "0"), "{layers:?}");
    let blue = cadcraft_engine::doc::color::Color::True(cadcraft_engine::doc::color::Rgb(0x2f, 0x6b, 0xff));
    assert!(preview[n..].iter().all(|e| e.common.color == blue));
}

#[test]
fn a_grip_with_a_value_list_marks_its_stops_while_dragged() {
    let (mut s, h) = session();
    // Width is a list (30, 32, 36) from (0, -4): its end can stop 30, 32 or 36 along it.
    let stops = cadcraft_engine::doc::dynblock::grip_stops(s.doc().unwrap(), h, 2, GripPart::End);
    let at: Vec<(f64, f64)> = stops.iter().map(|(p, _)| (p.x, p.y)).collect();
    assert_eq!(at, [(130.0, -4.0), (132.0, -4.0), (136.0, -4.0)]);
    assert!(stops.iter().all(|(_, way)| (way.x - 1.0).abs() < 1e-12 && way.y.abs() < 1e-12));
    // A grip that isn't on a list or increments has none; nor a list grip.
    assert!(cadcraft_engine::doc::dynblock::grip_stops(s.doc().unwrap(), h, 1, GripPart::At).is_empty());

    // The preview marks each stop with a tick across the travel, a few pixels long.
    s.start_dyn_grip(h, 2, GripPart::End).unwrap();
    let half = 6.0 * s.pixel_size();
    let ticks: Vec<(f64, f64)> = s
        .preview(Vec2::new(133.0, 0.0))
        .into_iter()
        .filter_map(|e| match e.kind {
            cadcraft_engine::doc::EntityKind::Line(l) if (l.a.x - l.b.x).abs() < 1e-9 && ((l.b.y - l.a.y).abs() - 2.0 * half).abs() < 1e-9 => {
                Some((l.a.x, (l.a.y + l.b.y) / 2.0))
            }
            _ => None,
        })
        .collect();
    assert_eq!(ticks, [(130.0, -4.0), (132.0, -4.0), (136.0, -4.0)]);
}

/// An alignment grip dragged onto a line (the synthetic Align block of the io tests, a line at
/// 30° from (120, 100)): the reference moves onto the line and turns along it, as AutoCAD does;
/// away from objects it only moves.
#[test]
fn an_alignment_grip_aligns_with_the_line_at_the_cursor() {
    let src = include_str!("../../../io/tests/data/dynblock_params.txt").replace("\r\n", "\n");
    let d = cadcraft_io::read_dxf(dxf(&src).as_bytes()).unwrap();
    let mut s = Session::empty();
    s.open_drawing(d, "params", None);
    let r = s.execute("insert", &json!({ "name": "Align", "at": [100, 100] })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let g = s.execute("blocks.reference.grips", &json!({ "handle": h.hex() })).unwrap();
    assert_eq!(g[0]["shape"], "alignment");
    // A point on the line, a little below it.
    let (c, sn) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    let to = [120.0 + 10.0 * c + 0.01 * sn, 100.0 + 10.0 * sn - 0.01 * c];
    s.execute("blocks.reference.grip", &json!({ "handle": h.hex(), "node": 1, "part": "base", "to": to })).unwrap();
    let Some(cadcraft_engine::doc::EntityKind::Insert(i)) = s.doc().unwrap().entity(h).map(|e| &e.kind) else { panic!("a reference") };
    assert!((i.rotation.to_degrees() - 30.0).abs() < 1e-6, "{}", i.rotation.to_degrees());
    assert!((i.insert.x - (120.0 + 10.0 * c)).abs() < 1e-6 && (i.insert.y - (100.0 + 10.0 * sn)).abs() < 1e-6, "{:?}", i.insert);
    // Far from anything: it only moves.
    s.execute("blocks.reference.grip", &json!({ "handle": h.hex(), "node": 1, "part": "base", "to": [0, -50] })).unwrap();
    let Some(cadcraft_engine::doc::EntityKind::Insert(i)) = s.doc().unwrap().entity(h).map(|e| &e.kind) else { panic!("a reference") };
    assert!((i.rotation.to_degrees() - 30.0).abs() < 1e-6 && i.insert.x.abs() < 1e-9 && (i.insert.y + 50.0).abs() < 1e-9);
}
