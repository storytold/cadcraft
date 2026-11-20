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

/// #2800: a typed PER or TAN finds its point from a pick anywhere on the object, as in AutoCAD.
/// The pick used to have to land within the aperture (0.125 here) of the foot or tangent point,
/// which you can't know in advance; elsewhere it said "No Perpendicular found".
#[test]
fn typed_per_and_tan_snap_from_anywhere_on_the_object() {
    let mut s = drawing();
    // From (5,5), PER picked at (3,0) on the line 0,0–10,0: two units from the foot (5,0).
    s.script("LINE 5,5 PER 3,0\n\n").unwrap();
    let (a, b) = last_line(&s);
    assert!(near(a, 5.0, 5.0) && near(b, 5.0, 0.0), "{a:?} {b:?}");
    // From (30,20), TAN picked at the top of the circle (centre 20,20, radius 3): the tangent
    // points are at cos = 3/10 from the line to the base, and the upper one is nearer the pick.
    s.script("LINE 30,20 TAN 20,23\n\n").unwrap();
    let (a, b) = last_line(&s);
    assert!(near(a, 30.0, 20.0) && near(b, 20.9, 20.0 + 3.0 * 0.91f64.sqrt()), "{a:?} {b:?}");
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
