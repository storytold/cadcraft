//! Object snap overrides typed at a point prompt (#393): `END`, `MID`, `CEN`, `PER` … apply to the
//! next point only, `NON` turns snapping off for it, and they nest with FROM.

use cadcraft_doc::EntityKind;
use cadcraft_engine::pointmod::snap_candidates;
use cadcraft_engine::snap::{self, mode};
use cadcraft_engine::{Input, Session};
use cadcraft_geom::Vec2;

/// A session with a line 0,0–10,0, a line 10,0–10,10 and a circle at 20,20 radius 3. The
/// default view and window put the snap aperture at 0.125 drawing units.
fn drawing() -> Session {
    let mut s = Session::new();
    s.script("LINE 0,0 10,0 10,10\n\nCIRCLE 20,20 3\n").unwrap();
    assert!(s.running.is_none());
    s
}

/// The last line drawn as (start, end).
fn last_line(s: &Session) -> (Vec2, Vec2) {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some((l.a.xy(), l.b.xy())) } else { None }).last().unwrap()
}

/// The last circle drawn as (center, radius).
fn last_circle(s: &Session) -> (Vec2, f64) {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let EntityKind::Circle(c) = &e.kind { Some((c.center.xy(), c.radius)) } else { None })
        .last()
        .unwrap()
}

fn near(p: Vec2, x: f64, y: f64) -> bool {
    p.near(Vec2::new(x, y), 1e-9)
}

#[test]
fn typed_overrides_snap_the_next_typed_point() {
    let mut s = drawing();
    s.cmdline("line end").unwrap();
    assert!(s.prompt_text().ends_with("Endpoint of:"), "{}", s.prompt_text());
    assert_eq!(s.snap_override(), Some(mode::END));
    s.cmdline("9.97,0.03").unwrap();
    assert_eq!(s.snap_override(), None, "one pick only");
    s.cmdline("_mid 5.04,-0.02").unwrap();
    s.cmdline("").unwrap();
    let (a, b) = last_line(&s);
    assert!(near(a, 10.0, 0.0) && near(b, 5.0, 0.0), "{a:?} {b:?}");

    // Long names and the transparent prefix; CEN finds the circle's centre.
    s.script("LINE '_center 20.05,19.98\n_endpoint 0.02,0.01\n\n").unwrap();
    let (a, b) = last_line(&s);
    assert!(near(a, 20.0, 20.0) && near(b, 0.0, 0.0), "{a:?} {b:?}");

    // PER from the previous point onto the first line.
    s.script("LINE 3,5 PER 3.02,0.01\n\n").unwrap();
    let (a, b) = last_line(&s);
    assert!(near(a, 3.0, 5.0) && near(b, 3.0, 0.0), "{a:?} {b:?}");
}

#[test]
fn an_override_lasts_one_point_and_non_turns_snapping_off() {
    let mut s = drawing();
    s.script("LINE END 9.97,0.03 9.97,0.03\n\n").unwrap();
    let (a, b) = last_line(&s);
    assert!(near(a, 10.0, 0.0) && near(b, 9.97, 0.03), "the second point is not snapped: {b:?}");

    s.cmdline("line non").unwrap();
    assert_eq!(s.snap_override(), Some(0), "no object snap for the next pick");
    s.cmdline("0.01,0.01").unwrap();
    // A second override replaces the first.
    s.cmdline("mid end 9.99,9.98").unwrap();
    s.cmdline("").unwrap();
    let (a, b) = last_line(&s);
    assert!(near(a, 0.01, 0.01) && near(b, 10.0, 10.0), "{a:?} {b:?}");
    assert!(s.point_mods.is_empty());
}

#[test]
fn nothing_to_snap_to_rejects_the_point() {
    let mut s = drawing();
    s.cmdline("line end 50,50").unwrap();
    assert!(s.log.iter().any(|l| l == "No Endpoint found for specified point."), "{:?}", s.log);
    assert!(s.prompt_text().contains("Specify first point"), "{}", s.prompt_text());
    // A picked point goes through the same override.
    s.cmdline("end").unwrap();
    s.input(Input::Point(Vec2::new(10.04, 9.96))).unwrap();
    assert!(s.prompt_text().contains("Specify next point"), "{}", s.prompt_text());
    assert!(near(s.last_point, 10.0, 10.0));
}

#[test]
fn from_nests_with_a_snap_override() {
    let mut s = drawing();
    s.script("LINE FROM END 10.02,0.01 @0,2\n@5,0\n\n").unwrap();
    let (a, b) = last_line(&s);
    assert!(near(a, 10.0, 2.0) && near(b, 15.0, 2.0), "{a:?} {b:?}");
}

#[test]
fn tab_cycling_candidates_start_with_the_running_snap() {
    let s = drawing();
    let d = s.doc().unwrap();
    let space = s.space();
    let osmode = mode::END | mode::MID | mode::INT | mode::NEA;
    let cursor = Vec2::new(9.95, 0.02);
    let cands = snap_candidates(d, &space, cursor, 0.125, osmode, None, false);
    assert_eq!(cands.first().copied(), snap::osnap(d, &space, cursor, 0.125, osmode, None, false));
    let modes: Vec<u32> = cands.iter().map(|h| h.mode).collect();
    for m in [mode::END, mode::INT, mode::NEA] {
        assert!(modes.contains(&m), "{modes:?}");
    }
    assert!(!modes.contains(&mode::MID), "no midpoint in the aperture");
    assert!(snap_candidates(d, &space, Vec2::new(50.0, 50.0), 0.125, osmode, None, false).is_empty());
}

#[test]
fn a_command_keyword_wins_over_a_snap_word() {
    let mut s = drawing();
    // ARC's Center keyword is still `cen`; `_cen` is the override.
    s.cmdline("arc cen").unwrap();
    assert!(s.prompt_text().contains("center"), "{}", s.prompt_text());
    assert!(s.point_mods.is_empty());
    s.cancel();
    s.cmdline("arc _cen").unwrap();
    assert!(s.prompt_text().ends_with("Center of:"), "{}", s.prompt_text());
}

#[test]
fn tan_for_a_circle_radius_touches_the_picked_circle() {
    // #1512: the circle around the center that touches the circle at 20,20 radius 3, on the side
    // it was picked: the near side (radius 7), the far side (13), and through Diameter.
    for (script, center, r) in [
        ("CIRCLE 30,20 TAN 23.02,20.03\n", (30.0, 20.0), 7.0),
        ("CIRCLE 30,20 TAN 16.98,19.97\n", (30.0, 20.0), 13.0),
        ("CIRCLE 20,10 D TAN 20.03,17.02\n", (20.0, 10.0), 7.0),
    ] {
        let mut s = drawing();
        s.script(script).unwrap();
        assert!(s.running.is_none(), "{script}: {}", s.prompt_text());
        let (c, got) = last_circle(&s);
        assert!(near(c, center.0, center.1) && (got - r).abs() < 1e-9, "{script}: {c:?} r={got}");
    }
}

#[test]
fn tan_for_a_3p_circle_touches_the_picked_circles() {
    // #1512: through two points and tangent to the circle at 20,20 radius 3, the tangent pick
    // first or last.
    for script in ["CIRCLE 3P 26,20 26,26 TAN 23.02,20.03\n", "CIRCLE 3P TAN 23.02,20.03 26,20 26,26\n"] {
        let mut s = drawing();
        s.script(script).unwrap();
        assert!(s.running.is_none(), "{script}: {}", s.prompt_text());
        let (c, r) = last_circle(&s);
        assert!((c.dist(Vec2::new(26.0, 20.0)) - r).abs() < 1e-9 && (c.dist(Vec2::new(26.0, 26.0)) - r).abs() < 1e-9, "{script}");
        assert!((c.dist(Vec2::new(20.0, 20.0)) - (3.0 + r)).abs() < 1e-9, "{script}: {c:?} r={r}");
    }
    // Tan, Tan, Tan: the circle between three circles of radius 1, picked on their inner sides.
    let mut s = Session::new();
    s.script("CIRCLE 0,0 1\nCIRCLE 6,0 1\nCIRCLE 3,5 1\nCIRCLE 3P TAN 0.98,0.2 TAN 5.02,0.2 TAN 3.01,3.98\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let (c, r) = last_circle(&s);
    for k in [(0.0, 0.0), (6.0, 0.0), (3.0, 5.0)] {
        assert!((c.dist(Vec2::new(k.0, k.1)) - (1.0 + r)).abs() < 1e-9, "{c:?} r={r}");
    }
}

#[test]
fn tan_for_a_3p_circle_touches_an_arc_within_its_sweep() {
    // #1512: through 26,20 and 26,26, the circles tangent to the circle at 20,20 radius 3 touch it
    // on the right or the far left, both off an arc swept from 90° to 100°: no circle.
    let mut s = Session::new();
    s.execute("arc", &serde_json::json!({"center": [20, 20], "radius": 3, "start": 90, "end": 100})).unwrap();
    s.script("CIRCLE 3P 26,20 26,26 TAN 19.74,22.99\n").unwrap();
    assert!(s.prompt_text().contains("third point"), "{}", s.prompt_text());
    assert!(!s.doc().unwrap().model.iter().any(|e| matches!(e.kind, EntityKind::Circle(_))));
}

#[test]
fn tan_for_a_3p_circle_takes_the_solution_that_touches_the_arc() {
    // #1512: through 26,20 and 26,26, the circles tangent to the circle at 20,20 radius 3 touch it
    // at about 29.6° and -103.3°. Picked at 20° on an arc swept from 200° to 25°, the 29.6°
    // circle is nearer the pick but off the arc: the -103.3° one (radius 6.0826) is drawn.
    let mut s = Session::new();
    s.execute("arc", &serde_json::json!({"center": [20, 20], "radius": 3, "start": 200, "end": 25})).unwrap();
    s.script("CIRCLE 3P 26,20 26,26 TAN 22.82,21.03\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let (c, r) = last_circle(&s);
    assert!((c.dist(Vec2::new(20.0, 20.0)) - (r - 3.0)).abs() < 1e-9 && (r - 6.082575694955838).abs() < 1e-9, "{c:?} r={r}");
}

#[test]
fn tan_for_a_circle_radius_refuses_the_picked_circles_own_center() {
    // #1512: a circle around 20,20 would lie on the circle there, not touch it.
    let mut s = drawing();
    s.script("CIRCLE 20,20 TAN 23.02,20.03\n").unwrap();
    assert!(s.prompt_text().contains("radius of circle"), "{}", s.prompt_text());
    assert!(s.log.iter().any(|l| l.contains("No circle around the center is tangent there.")), "{:?}", s.log);
    assert_eq!(s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Circle(_))).count(), 1);
}

#[test]
fn tan_for_a_3p_circle_refuses_the_picked_circle_itself() {
    // #1512: tangent twice to the circle and through a point on it is only the circle itself.
    let mut s = drawing();
    s.script("CIRCLE 3P TAN 23.02,20.03 TAN 16.98,20.03 20,23\n").unwrap();
    assert!(s.prompt_text().contains("third point"), "{}", s.prompt_text());
    assert!(s.log.iter().any(|l| l.contains("Circle does not exist.")), "{:?}", s.log);
    assert_eq!(s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Circle(_))).count(), 1);
}
