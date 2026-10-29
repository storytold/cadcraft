//! Hot-grip options, typed at the command line as AutoCAD's grip modes take them: Base point,
//! Copy, Undo, Reference and eXit; Enter cycles the modes; several grips can be hot at once.
//! A whole grip edit, copies included, is one undo step.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use serde_json::json;

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    let r = s.execute("line", &json!({ "points": [a, b] })).unwrap();
    Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap()
}

fn lines(s: &Session) -> Vec<(Vec2, Vec2)> {
    let d = s.doc().unwrap();
    d.model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => Some((l.a.xy(), l.b.xy())),
            _ => None,
        })
        .collect()
}

fn ends(s: &Session, h: Handle) -> (Vec2, Vec2) {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Line(l) => (l.a.xy(), l.b.xy()),
        k => panic!("not a line: {k:?}"),
    }
}

fn near(p: Vec2, x: f64, y: f64) -> bool {
    p.near(Vec2::new(x, y), 1e-9)
}

fn grip_index(s: &Session, h: Handle, x: f64, y: f64) -> usize {
    s.grips_of(h).unwrap().iter().position(|g| near(*g, x, y)).unwrap()
}

fn typed(s: &mut Session, inputs: &[&str]) {
    for t in inputs {
        s.cmdline(t).unwrap();
    }
}

#[test]
fn base_point_copy_and_undo_make_one_undo_step() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.set_selection(vec![a]);
    let undo = s.state().unwrap().undo.len();
    s.start_grip_edit(&[(a, grip_index(&s, a, 10.0, 0.0))]).unwrap();
    assert!(s.prompt_text().contains("** STRETCH ** Specify stretch point or [Base point/Copy/Undo/Lengthen/eXit]"), "{}", s.prompt_text());

    // Base point 0,0, then Copy: each point stretches a new copy by (point - base).
    typed(&mut s, &["B", "0,0", "C", "5,5", "0,5", "0,9"]);
    assert_eq!(lines(&s).len(), 4, "the original and three copies");
    // Undo takes back the last copy only.
    typed(&mut s, &["U"]);
    assert_eq!(lines(&s).len(), 3);
    assert!(s.running.is_some(), "Copy mode carries on");
    // Enter after copies ends the edit.
    typed(&mut s, &[""]);
    assert!(s.running.is_none());
    let ls = lines(&s);
    assert!(near(ends(&s, a).1, 10.0, 0.0), "the original stays");
    assert!(ls.iter().any(|(p, q)| near(*p, 0.0, 0.0) && near(*q, 15.0, 5.0)), "{ls:?}");
    assert!(ls.iter().any(|(p, q)| near(*p, 0.0, 0.0) && near(*q, 10.0, 5.0)), "{ls:?}");
    assert_eq!(s.state().unwrap().undo.len(), undo + 1, "one undo step for the whole edit");
    s.undo().unwrap();
    assert_eq!(lines(&s).len(), 1);
}

#[test]
fn enter_cycles_the_modes_and_typed_values_apply() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.set_selection(vec![a]);
    s.start_grip_edit(&[(a, 0)]).unwrap();
    typed(&mut s, &[""]);
    assert!(s.prompt_text().contains("** MOVE **"), "{}", s.prompt_text());
    typed(&mut s, &[""]);
    assert!(s.prompt_text().contains("** ROTATE ** Specify rotation angle or [Base point/Copy/Undo/Reference/eXit]"), "{}", s.prompt_text());
    // A typed angle rotates about the grip; without Copy the edit ends.
    typed(&mut s, &["90"]);
    assert!(s.running.is_none());
    let (p, q) = ends(&s, a);
    assert!(near(p, 0.0, 0.0) && near(q, 0.0, 10.0), "{p:?} {q:?}");

    // SCALE takes a typed factor; MIRROR a second point; after MIRROR comes STRETCH again.
    s.start_grip_edit(&[(a, 0)]).unwrap();
    typed(&mut s, &["", "", "", "2"]);
    assert!(near(ends(&s, a).1, 0.0, 20.0), "{:?}", ends(&s, a));
    s.start_grip_edit(&[(a, 0)]).unwrap();
    typed(&mut s, &["", "", "", ""]);
    assert!(s.prompt_text().contains("** MIRROR ** Specify second point"), "{}", s.prompt_text());
    typed(&mut s, &[""]);
    assert!(s.prompt_text().contains("** STRETCH **"), "{}", s.prompt_text());
    // eXit leaves the objects as they are.
    typed(&mut s, &["X"]);
    assert!(s.running.is_none());
    assert!(near(ends(&s, a).1, 0.0, 20.0));
}

#[test]
fn reference_rotate_and_scale() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.set_selection(vec![a]);
    // ROTATE Reference: from 45° to 90° turns by 45°.
    s.start_grip_edit(&[(a, 0)]).unwrap();
    typed(&mut s, &["", "", "R", "45", "90"]);
    let q = ends(&s, a).1;
    let r = std::f64::consts::FRAC_1_SQRT_2 * 10.0;
    assert!(near(q, r, r), "{q:?}");

    // SCALE Reference with two points: reference length 5 (two points), new length 10.
    let b = line(&mut s, [0.0, 0.0], [4.0, 0.0]);
    s.set_selection(vec![b]);
    s.start_grip_edit(&[(b, 0)]).unwrap();
    typed(&mut s, &["", "", "", "R", "0,0", "5,0", "10"]);
    assert!(near(ends(&s, b).1, 8.0, 0.0), "{:?}", ends(&s, b));
    assert!(s.running.is_none());
}

#[test]
fn move_with_base_point_and_copy_on_the_selection() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [0.0, 5.0], [10.0, 5.0]);
    s.set_selection(vec![a, b]);
    s.start_grip_edit(&[(a, 0)]).unwrap();
    // MOVE, Copy: both selected lines are copied by each displacement.
    typed(&mut s, &["", "C", "0,20", "0,40", "X"]);
    assert!(s.running.is_none());
    assert_eq!(lines(&s).len(), 6);
    assert!(near(ends(&s, a).0, 0.0, 0.0), "the originals stay");
}

#[test]
fn several_hot_grips_stretch_together() {
    let mut s = Session::new();
    s.execute("pline", &json!({ "vertices": [[0, 0], [10, 0], [10, 10], [0, 10]], "closed": true })).unwrap();
    let h = s.doc().unwrap().model.iter().last().unwrap().handle;
    s.set_selection(vec![h]);
    let (i, j) = (grip_index(&s, h, 10.0, 0.0), grip_index(&s, h, 10.0, 10.0));
    s.start_grip_edit(&[(h, i), (h, j)]).unwrap();
    typed(&mut s, &["15,0"]);
    assert!(s.running.is_none());
    let g = s.grips_of(h).unwrap();
    assert!(g.iter().any(|p| near(*p, 15.0, 0.0)) && g.iter().any(|p| near(*p, 15.0, 10.0)), "{g:?}");

    // Typed: GRIP, pick the hot grips, Enter, then the stretch point.
    typed(&mut s, &["GRIP", "0,0", "0,10", "", "-5,0"]);
    assert!(s.running.is_none());
    let g = s.grips_of(h).unwrap();
    assert!(g.iter().any(|p| near(*p, -5.0, 0.0)) && g.iter().any(|p| near(*p, -5.0, 10.0)), "{g:?}");

    // JSON: grip.stretch with several grips.
    let (i, j) = (grip_index(&s, h, 15.0, 0.0), grip_index(&s, h, 15.0, 10.0));
    s.execute("grip.stretch", &json!({ "grips": [[h.hex(), i], [h.hex(), j]], "delta": [5, 0] })).unwrap();
    let g = s.grips_of(h).unwrap();
    assert!(g.iter().any(|p| near(*p, 20.0, 0.0)) && g.iter().any(|p| near(*p, 20.0, 10.0)), "{g:?}");
}

#[test]
fn bad_grips_are_errors_not_panics() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.set_selection(vec![a]);
    assert!(s.start_grip_edit(&[]).is_err());
    assert!(s.start_grip_edit(&[(a, 99)]).is_err());
    assert!(s.start_grip_edit(&[(Handle(0xFFFF), 0)]).is_err());
    for p in [
        json!({ "grips": "x" }),
        json!({ "grips": [] , "delta": [1, 0] }),
        json!({ "grips": [[a.hex()]], "delta": [1, 0] }),
        json!({ "grips": [[a.hex(), -1]], "delta": [1, 0] }),
        json!({ "grips": [[a.hex(), 0]], "delta": [f64::MAX, 0] }),
        json!({ "grips": [[a.hex(), 0]] }),
    ] {
        let _ = s.execute("grip.stretch", &p);
    }
    assert!(s.execute("grip", &json!({})).is_err());
    // Typed GRIP away from any grip re-prompts; nonsense at the mode prompt too.
    typed(&mut s, &["GRIP", "50,50"]);
    assert!(s.running.is_some());
    typed(&mut s, &["10,0", "", "foo", "1e400,0"]);
    s.cancel();
    assert!(s.running.is_none());
    let (p, q) = ends(&s, a);
    assert!(p.is_finite() && q.is_finite());
}

// ---------------- multifunctional grips ----------------

fn pline(s: &mut Session, pts: &[[f64; 2]], closed: bool) -> Handle {
    s.execute("pline", &json!({ "vertices": pts, "closed": closed })).unwrap();
    s.doc().unwrap().model.iter().last().unwrap().handle
}

fn vertices(s: &Session, h: Handle) -> Vec<(Vec2, f64)> {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::LwPolyline(p) => p.vertices.iter().map(|v| (v.p, v.bulge)).collect(),
        k => panic!("not a polyline: {k:?}"),
    }
}

#[test]
fn polyline_vertex_options() {
    let mut s = Session::new();
    let h = pline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false);
    s.set_selection(vec![h]);
    let undo = s.state().unwrap().undo.len();
    s.start_grip_edit(&[(h, grip_index(&s, h, 10.0, 0.0))]).unwrap();
    assert!(s.prompt_text().contains("[Base point/Copy/Undo/Add vertex/Remove vertex/eXit]"), "{}", s.prompt_text());
    // Add vertex after the hot vertex.
    typed(&mut s, &["A"]);
    assert!(s.prompt_text().contains("** ADD VERTEX ** Specify new vertex point"), "{}", s.prompt_text());
    typed(&mut s, &["15,5"]);
    assert!(s.running.is_none());
    let v: Vec<Vec2> = vertices(&s, h).into_iter().map(|v| v.0).collect();
    assert_eq!(v.len(), 4);
    assert!(near(v[2], 15.0, 5.0), "{v:?}");
    assert_eq!(s.state().unwrap().undo.len(), undo + 1);

    // Remove vertex acts at once.
    s.start_grip_edit(&[(h, grip_index(&s, h, 15.0, 5.0))]).unwrap();
    typed(&mut s, &["R"]);
    assert!(s.running.is_none());
    assert_eq!(vertices(&s, h).len(), 3);

    // Add vertex from a segment midpoint: the new vertex goes on that segment.
    s.start_grip_edit(&[(h, grip_index(&s, h, 5.0, 0.0))]).unwrap();
    typed(&mut s, &["A", "5,-3"]);
    let v: Vec<Vec2> = vertices(&s, h).into_iter().map(|v| v.0).collect();
    assert!(v.len() == 4 && near(v[1], 5.0, -3.0), "{v:?}");

    // Enter at an option prompt goes back to STRETCH.
    s.start_grip_edit(&[(h, 0)]).unwrap();
    typed(&mut s, &["A", ""]);
    assert!(s.prompt_text().contains("** STRETCH **"), "{}", s.prompt_text());
    typed(&mut s, &["X"]);
    assert_eq!(vertices(&s, h).len(), 4);
}

#[test]
fn polyline_segment_converts_to_arc_and_back() {
    let mut s = Session::new();
    let h = pline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false);
    s.set_selection(vec![h]);
    let mid = grip_index(&s, h, 5.0, 0.0);
    s.start_grip_edit(&[(h, mid)]).unwrap();
    assert!(s.prompt_text().contains("Add vertex/conVert to arc/eXit"), "{}", s.prompt_text());
    typed(&mut s, &["V", "5,3"]);
    let b = vertices(&s, h)[0].1;
    assert!(b.abs() > 0.1, "an arc segment: bulge {b}");
    // The arc's midpoint grip now offers Convert to Line.
    let g = s.grips_of(h).unwrap();
    assert!(near(g[mid], 5.0, 3.0), "{g:?}");
    s.start_grip_edit(&[(h, mid)]).unwrap();
    assert!(s.prompt_text().contains("conVert to line"), "{}", s.prompt_text());
    typed(&mut s, &["V"]);
    assert!(s.running.is_none());
    assert_eq!(vertices(&s, h)[0].1, 0.0);
}

#[test]
fn line_and_arc_lengthen_and_radius() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.set_selection(vec![a]);
    s.start_grip_edit(&[(a, 2)]).unwrap();
    assert!(s.prompt_text().contains("Undo/Lengthen/eXit"), "{}", s.prompt_text());
    // A picked point is projected onto the line.
    typed(&mut s, &["L", "15,3"]);
    assert!(near(ends(&s, a).1, 15.0, 0.0), "{:?}", ends(&s, a));
    // A typed value is the change in length; from the start grip.
    s.start_grip_edit(&[(a, 0)]).unwrap();
    typed(&mut s, &["L", "-5"]);
    assert!(near(ends(&s, a).0, 5.0, 0.0), "{:?}", ends(&s, a));
    // Shortening past the other end is refused (re-prompts).
    s.start_grip_edit(&[(a, 2)]).unwrap();
    typed(&mut s, &["L", "-50"]);
    assert!(s.running.is_some());
    s.cancel();
    assert!(near(ends(&s, a).1, 15.0, 0.0));

    // Arc: Radius from the midpoint grip, Lengthen from an end grip.
    s.execute("arc", &json!({ "center": [0, 0], "radius": 5, "start": 0, "end": 90 })).unwrap();
    let arc = s.doc().unwrap().model.iter().last().unwrap().handle;
    s.set_selection(vec![arc]);
    s.start_grip_edit(&[(arc, 1)]).unwrap();
    assert!(s.prompt_text().contains("Undo/Radius/eXit"), "{}", s.prompt_text());
    typed(&mut s, &["R", "8"]);
    s.start_grip_edit(&[(arc, 2)]).unwrap();
    typed(&mut s, &["L", "-1,0.0001"]);
    match &s.doc().unwrap().entity(arc).unwrap().kind {
        EntityKind::Arc(k) => {
            assert!((k.radius - 8.0).abs() < 1e-9, "{k:?}");
            assert!((k.end.to_degrees() - 180.0).abs() < 0.01, "{k:?}");
        }
        k => panic!("{k:?}"),
    }
}

#[test]
fn grip_option_json_form() {
    let mut s = Session::new();
    let h = pline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false);
    s.execute("grip.option", &json!({ "handle": h.hex(), "index": 0, "option": "addVertex", "to": [-5, 0] })).unwrap();
    assert_eq!(vertices(&s, h).len(), 4);
    s.execute("grip.option", &json!({ "handle": h.hex(), "index": 0, "option": "removeVertex" })).unwrap();
    assert_eq!(vertices(&s, h).len(), 3);
    // Options that don't apply, unknown options and hostile values are errors, not panics.
    for p in [
        json!({ "handle": h.hex(), "index": 0, "option": "radius", "value": 3 }),
        json!({ "handle": h.hex(), "index": 0, "option": "explode" }),
        json!({ "handle": h.hex(), "index": 99, "option": "addVertex", "to": [1, 1] }),
        json!({ "handle": h.hex(), "index": 1, "option": "toArc", "to": [1e308, 1e308] }),
        json!({ "handle": h.hex(), "index": 1, "option": "toArc" }),
        json!({ "index": 1, "option": "toLine" }),
    ] {
        assert!(s.execute("grip.option", &p).is_err(), "{p}");
    }
}

// ---------------- hatch grips ----------------

fn hatch_of(s: &Session, h: Handle) -> cadcraft_engine::doc::Hatch {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Hatch(x) => x.clone(),
        k => panic!("not a hatch: {k:?}"),
    }
}

/// A 10 x 10 square filled with ANSI31, its centre grip at (5, 5).
fn pattern_hatch(s: &mut Session, pattern: &str) -> Handle {
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    let r = s.execute("hatch", &json!({ "points": [[5, 5]], "pattern": pattern })).unwrap();
    Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap()
}

#[test]
fn hatch_centre_grip_origin_angle_and_scale() {
    let mut s = Session::new();
    let h = pattern_hatch(&mut s, "ANSI31");
    s.set_selection(vec![h]);
    assert!(near(s.grips_of(h).unwrap()[0], 5.0, 5.0));
    let undo = s.state().unwrap().undo.len();
    s.start_grip_edit(&[(h, 0)]).unwrap();
    assert!(s.prompt_text().contains("[Base point/Copy/Undo/Origin point/hatch Angle/hatch Scale/eXit]"), "{}", s.prompt_text());
    // Origin Point: the pattern's origin moves, the fill stays.
    let loops = hatch_of(&s, h).loops;
    typed(&mut s, &["O"]);
    assert!(s.prompt_text().contains("** ORIGIN POINT ** Specify origin point"), "{}", s.prompt_text());
    typed(&mut s, &["2,3"]);
    assert!(s.running.is_none());
    let x = hatch_of(&s, h);
    assert!(near(x.origin, 2.0, 3.0), "{:?}", x.origin);
    assert_eq!(x.loops, loops);
    assert_eq!(s.state().unwrap().undo.len(), undo + 1);
    // Hatch Angle: typed in degrees, or toward a picked point from the grip.
    s.start_grip_edit(&[(h, 0)]).unwrap();
    typed(&mut s, &["A", "30"]);
    assert!((hatch_of(&s, h).angle.to_degrees() - 30.0).abs() < 1e-9);
    s.start_grip_edit(&[(h, 0)]).unwrap();
    typed(&mut s, &["A", "5,10"]);
    assert!((hatch_of(&s, h).angle.to_degrees() - 90.0).abs() < 1e-9);
    // Hatch Scale: typed, or the distance from the grip.
    s.start_grip_edit(&[(h, 0)]).unwrap();
    typed(&mut s, &["S", "2.5"]);
    assert!((hatch_of(&s, h).scale - 2.5).abs() < 1e-12);
    s.start_grip_edit(&[(h, 0)]).unwrap();
    typed(&mut s, &["S", "9,5"]);
    assert!((hatch_of(&s, h).scale - 4.0).abs() < 1e-12);
    // A scale of zero or a hostile one is refused; the prompt stays.
    s.start_grip_edit(&[(h, 0)]).unwrap();
    for bad in ["0", "-1", "1e300"] {
        typed(&mut s, &["S"]);
        assert!(s.cmdline(bad).is_err() || s.running.is_some(), "{bad}");
        s.cancel();
        s.start_grip_edit(&[(h, 0)]).unwrap();
    }
    s.cancel();
    assert!((hatch_of(&s, h).scale - 4.0).abs() < 1e-12);
    // Stretch still moves the whole hatch.
    s.start_grip_edit(&[(h, 0)]).unwrap();
    typed(&mut s, &["15,5"]);
    assert!(near(s.grips_of(h).unwrap()[0], 15.0, 5.0));
}

#[test]
fn hatch_grip_options_json_form_and_solid_fills() {
    let mut s = Session::new();
    let h = pattern_hatch(&mut s, "ANSI31");
    s.execute("grip.option", &json!({ "handle": h.hex(), "index": 0, "option": "hatchAngle", "value": 45 })).unwrap();
    assert!((hatch_of(&s, h).angle.to_degrees() - 45.0).abs() < 1e-9, "JSON takes degrees");
    s.execute("grip.option", &json!({ "handle": h.hex(), "index": 0, "option": "hatchScale", "value": 3 })).unwrap();
    s.execute("grip.option", &json!({ "handle": h.hex(), "index": 0, "option": "hatchOrigin", "to": [1, 1] })).unwrap();
    let x = hatch_of(&s, h);
    assert!((x.scale - 3.0).abs() < 1e-12 && near(x.origin, 1.0, 1.0));
    for p in [
        json!({ "handle": h.hex(), "index": 0, "option": "hatchScale", "value": 0 }),
        json!({ "handle": h.hex(), "index": 0, "option": "hatchScale", "value": 1e300 }),
        json!({ "handle": h.hex(), "index": 0, "option": "hatchAngle" }),
        json!({ "handle": h.hex(), "index": 0, "option": "hatchAngle", "to": [5, 5] }),
        json!({ "handle": h.hex(), "index": 1, "option": "hatchOrigin", "to": [1, 1] }),
        json!({ "handle": h.hex(), "index": 0, "option": "hatchOrigin" }),
    ] {
        assert!(s.execute("grip.option", &p).is_err(), "{p}");
    }
    // A solid fill has no pattern: Stretch only.
    let solid = pattern_hatch(&mut s, "SOLID");
    s.set_selection(vec![solid]);
    s.start_grip_edit(&[(solid, 0)]).unwrap();
    assert!(!s.prompt_text().contains("hatch Angle"), "{}", s.prompt_text());
    s.cancel();
    assert!(s.execute("grip.option", &json!({ "handle": solid.hex(), "index": 0, "option": "hatchScale", "value": 2 })).is_err());
}

/// An out-of-range Hatch Scale says what to type (positive, within 1e-6..1e9), at the prompt and
/// in the JSON form, instead of "degenerate object"; the prompt stays for another try.
#[test]
fn hatch_scale_out_of_range_says_what_to_type() {
    let mut s = Session::new();
    let h = pattern_hatch(&mut s, "ANSI31");
    s.set_selection(vec![h]);
    s.start_grip_edit(&[(h, 0)]).unwrap();
    typed(&mut s, &["S"]);
    for bad in ["0", "-2", "2e9", "1e-7"] {
        s.log.clear();
        typed(&mut s, &[bad]);
        let e = s.log.join(" | ");
        assert!(e.contains("positive") && e.contains("1e-6") && e.contains("1e9"), "{bad}: {e}");
        assert!(!e.contains("degenerate"), "{bad}: {e}");
        assert!(s.prompt_text().contains("** HATCH SCALE **"), "{bad}: {}", s.prompt_text());
    }
    // A picked point on the grip (scale 0) says the same.
    s.log.clear();
    typed(&mut s, &["5,5"]);
    assert!(s.log.iter().any(|l| l.contains("1e-6")), "{:?}", s.log);
    typed(&mut s, &["2"]);
    assert!((hatch_of(&s, h).scale - 2.0).abs() < 1e-12);
    for v in [0.0, -2.0, 2e9, 1e-7] {
        let e = s.execute("grip.option", &json!({ "handle": h.hex(), "index": 0, "option": "hatchScale", "value": v })).unwrap_err();
        let e = e.to_string();
        assert!(e.contains("positive") && e.contains("1e-6") && e.contains("1e9") && !e.contains("degenerate"), "{v}: {e}");
    }
    assert!((hatch_of(&s, h).scale - 2.0).abs() < 1e-12);
}
