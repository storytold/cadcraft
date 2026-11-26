//! The live array parameters ([`cadcraft_engine::ArrayLive`]) of a running ARRAYRECT / ARRAYPOLAR:
//! read and written from the array window, and kept in step with the command line.

use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{ArrayLive, ArrayLiveKind, Input, Session};
use serde_json::json;

/// A session with one line selected pickfirst, so the array command goes straight to its Main
/// option loop.
fn selected() -> Session {
    let mut s = Session::new();
    let r = s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap()).unwrap();
    s.set_selection(vec![h]);
    s
}

fn first_handle(s: &Session) -> Handle {
    s.doc().unwrap().model.iter().next().unwrap().handle
}

#[test]
fn rect_live_reads_and_writes_through_the_session() {
    let mut s = selected();
    s.start("arrayrect").unwrap();
    let live = s.array_live().unwrap();
    assert_eq!(live.kind, ArrayLiveKind::Rectangular);
    assert_eq!((live.cols, live.rows, live.levels), (4, 3, 1));
    assert_eq!(live.center, None);
    assert_eq!((live.items, live.rotate), (0, false));

    // Writing through the window.
    let mut v = live.clone();
    (v.cols, v.rows, v.levels) = (2, 5, 2);
    (v.col_spacing, v.row_spacing, v.level_spacing, v.row_elevation) = (3.0, 4.0, 5.0, 0.5);
    v.associative = true;
    s.set_array_live(&v).unwrap();
    let now = s.array_live().unwrap();
    assert_eq!((now.cols, now.rows, now.levels), (2, 5, 2));
    assert_eq!((now.col_spacing, now.row_spacing, now.level_spacing, now.row_elevation), (3.0, 4.0, 5.0, 0.5));
    assert!(now.associative);

    // Commit creates the array and clears the running command.
    assert!(s.commit_array().unwrap());
    assert!(s.running.is_none());
    assert!(s.array_live().is_none());
    let n = s.doc().unwrap().model.iter().filter(|e| matches!(&e.kind, EntityKind::Insert(_))).count();
    assert_eq!(n, 1, "commit did not create an array block reference");
}

#[test]
fn command_line_input_is_reflected_in_array_live() {
    let mut s = selected();
    s.start("arrayrect").unwrap();
    s.cmdline("columns").unwrap();
    s.cmdline("6").unwrap();
    assert_eq!(s.array_live().unwrap().cols, 6);
    // A count is followed by its spacing: Enter keeps it and returns to the option loop.
    s.cmdline("").unwrap();
    s.cmdline("rows").unwrap();
    s.cmdline("2").unwrap();
    assert_eq!(s.array_live().unwrap().rows, 2);
    // Enter keeps the row spacing and the row elevation, back at the option loop.
    s.cmdline("").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(s.array_live().unwrap().rows, 2);
    // `x` (eXit) ends the command, so there is no live array left.
    s.cmdline("x").unwrap();
    assert!(s.array_live().is_none());
}

#[test]
fn rect_set_array_live_refuses_bad_values_without_changing_state() {
    let mut s = selected();
    s.start("arrayrect").unwrap();
    let before = s.array_live().unwrap();

    let mut zero = before.clone();
    zero.cols = 0;
    assert!(s.set_array_live(&zero).is_err());
    assert_eq!(s.array_live().unwrap(), before);

    let mut inf = before.clone();
    inf.col_spacing = f64::INFINITY;
    assert!(s.set_array_live(&inf).is_err());
    assert_eq!(s.array_live().unwrap(), before);

    // Kind mismatch.
    let mut wrong = before.clone();
    wrong.kind = ArrayLiveKind::Polar;
    assert!(s.set_array_live(&wrong).is_err());
    assert_eq!(s.array_live().unwrap(), before);
}

#[test]
fn rect_array_pick_center_is_refused() {
    let mut s = selected();
    s.start("arrayrect").unwrap();
    assert!(!s.array_pick_center());
}

#[test]
fn polar_live_reads_and_writes_and_picks_the_center() {
    let mut s = selected();
    s.start("arraypolar").unwrap();
    // The centre prompt is up: the live values are the defaults.
    let live = s.array_live().unwrap();
    assert_eq!(live.kind, ArrayLiveKind::Polar);
    assert_eq!(live.center, Some(Vec2::new(5.0, 0.0)));
    assert_eq!((live.items, live.rows, live.levels), (6, 1, 1));
    assert_eq!((live.cols, live.col_spacing), (0, 0.0));

    // Pick the centre on the canvas, then feed the point: the command moves to Main.
    assert!(s.array_pick_center());
    s.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    assert_eq!(s.array_live().unwrap().center, Some(Vec2::new(5.0, 5.0)));
    assert!(!s.current_prompt().unwrap().message.contains("center point of array"));

    // Editing the window: items, fill, rotate, rows, spacings.
    let mut v = s.array_live().unwrap();
    (v.items, v.fill, v.rotate) = (4, 180.0, false);
    (v.rows, v.row_spacing, v.level_spacing, v.row_elevation) = (2, 3.0, 4.0, 0.0);
    v.associative = false;
    s.set_array_live(&v).unwrap();
    let now = s.array_live().unwrap();
    assert_eq!((now.items, now.fill, now.rotate), (4, 180.0, false));
    assert_eq!((now.rows, now.row_spacing, now.level_spacing, now.row_elevation), (2, 3.0, 4.0, 0.0));
    assert!(!now.associative);

    assert!(s.commit_array().unwrap());
    assert!(s.running.is_none());
}

#[test]
fn polar_set_array_live_sets_the_center_and_advances_from_center_stage() {
    let mut s = selected();
    s.start("arraypolar").unwrap();
    // Still at the Center stage.
    assert_eq!(s.array_live().unwrap().center, Some(Vec2::new(5.0, 0.0)));
    let mut v = s.array_live().unwrap();
    v.center = Some(Vec2::new(7.0, -2.0));
    s.set_array_live(&v).unwrap();
    let now = s.array_live().unwrap();
    assert_eq!(now.center, Some(Vec2::new(7.0, -2.0)));
    assert!(!s.current_prompt().unwrap().message.contains("center point of array"));
}

#[test]
fn polar_center_none_while_at_center_stays_at_center() {
    let mut s = selected();
    s.start("arraypolar").unwrap();
    let mut v = s.array_live().unwrap();
    v.center = None;
    s.set_array_live(&v).unwrap();
    assert!(s.current_prompt().unwrap().message.contains("center point of array"));
}

#[test]
fn polar_set_array_live_refuses_bad_values() {
    let mut s = selected();
    s.start("arraypolar").unwrap();
    let before = s.array_live().unwrap();

    let mut zero = before.clone();
    zero.fill = 0.0;
    assert!(s.set_array_live(&zero).is_err());
    assert_eq!(s.array_live().unwrap(), before);

    let mut big = before.clone();
    big.fill = 361.0;
    assert!(s.set_array_live(&big).is_err());
    assert_eq!(s.array_live().unwrap(), before);

    let mut no_items = before.clone();
    no_items.items = 0;
    assert!(s.set_array_live(&no_items).is_err());
    assert_eq!(s.array_live().unwrap(), before);

    let mut inf = before.clone();
    inf.row_spacing = f64::NAN;
    assert!(s.set_array_live(&inf).is_err());
    assert_eq!(s.array_live().unwrap(), before);

    let mut wrong = before.clone();
    wrong.kind = ArrayLiveKind::Rectangular;
    assert!(s.set_array_live(&wrong).is_err());
    assert_eq!(s.array_live().unwrap(), before);
}

#[test]
fn no_running_array_is_a_noop() {
    let mut s = Session::new();
    assert!(s.array_live().is_none());
    let v = ArrayLive {
        kind: ArrayLiveKind::Rectangular,
        cols: 2,
        rows: 2,
        col_spacing: 1.0,
        row_spacing: 1.0,
        levels: 1,
        level_spacing: 0.0,
        row_elevation: 0.0,
        associative: true,
        items: 0,
        fill: 0.0,
        rotate: false,
        center: None,
        between: None,
    };
    s.set_array_live(&v).unwrap();
    assert!(!s.array_pick_center());
    assert!(!s.commit_array().unwrap());
    assert!(s.array_live().is_none());
}

#[test]
fn commit_while_selecting_objects_is_refused() {
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    s.start("arrayrect").unwrap();
    // No pickfirst selection: the command is still asking for objects.
    assert!(!s.commit_array().unwrap());
    assert!(s.running.is_some());

    // Feed a selection: the command is ready, so it commits.
    let h = first_handle(&s);
    s.input(Input::Pick(vec![h])).unwrap();
    // A pick does not end the selection prompt; Enter does, and the command is ready.
    assert!(!s.commit_array().unwrap());
    s.input(Input::Enter).unwrap();
    assert!(s.array_live().is_some());
    assert!(s.commit_array().unwrap());
    assert!(s.running.is_none());
}

#[test]
fn commit_creates_polar_copies() {
    let mut s = selected();
    s.start("arraypolar").unwrap();
    s.input(Input::Point(Vec2::new(1.0, 1.0))).unwrap();
    let mut v = s.array_live().unwrap();
    v.items = 5;
    v.fill = 360.0;
    s.set_array_live(&v).unwrap();
    assert!(s.commit_array().unwrap());
    assert!(s.running.is_none());
}
#[test]
fn rect_live_has_no_angle_between() {
    let mut s = selected();
    s.start("arrayrect").unwrap();
    assert_eq!(s.array_live().unwrap().between, None);
    // Writing the rectangular values leaves the polar-only field alone.
    let mut v = s.array_live().unwrap();
    v.between = Some(45.0);
    s.set_array_live(&v).unwrap();
    assert_eq!(s.array_live().unwrap().between, None);
}

#[test]
fn polar_live_exposes_the_angle_between() {
    let mut s = selected();
    s.start("arraypolar").unwrap();
    // The defaults are 6 items over a full turn: 60 degrees between items.
    assert_eq!(s.array_live().unwrap().between, Some(60.0));

    let mut v = s.array_live().unwrap();
    v.items = 1;
    v.fill = 90.0;
    s.set_array_live(&v).unwrap();
    // A single item has no gap: the angle between is the fill angle.
    assert_eq!(s.array_live().unwrap().between, Some(90.0));
}

#[test]
fn polar_set_array_live_derives_the_fill_from_the_angle_between() {
    let mut s = selected();
    s.start("arraypolar").unwrap();

    // Items and Angle between edited together: the fill uses the new item count.
    let mut v = s.array_live().unwrap();
    v.items = 4;
    v.between = Some(45.0);
    s.set_array_live(&v).unwrap();
    let now = s.array_live().unwrap();
    assert_eq!((now.items, now.fill), (4, 135.0));
    assert_eq!(now.between, Some(45.0));

    // A full turn spread over 3 items: 180 degrees each.
    let mut v = now.clone();
    v.items = 3;
    v.between = Some(180.0);
    s.set_array_live(&v).unwrap();
    assert_eq!(s.array_live().unwrap().fill, 360.0);

    // An edited Fill angle wins over the angle between.
    let mut v = s.array_live().unwrap();
    v.items = 4;
    v.fill = 90.0;
    v.between = Some(45.0);
    s.set_array_live(&v).unwrap();
    let now = s.array_live().unwrap();
    assert_eq!((now.items, now.fill), (4, 90.0));
    assert_eq!(now.between, Some(30.0));
}

#[test]
fn polar_set_array_live_refuses_a_bad_angle_between() {
    let mut s = selected();
    s.start("arraypolar").unwrap();
    let before = s.array_live().unwrap();

    let mut zero = before.clone();
    zero.between = Some(0.0);
    assert!(s.set_array_live(&zero).is_err());
    assert_eq!(s.array_live().unwrap(), before);

    let mut inf = before.clone();
    inf.between = Some(f64::NAN);
    assert!(s.set_array_live(&inf).is_err());
    assert_eq!(s.array_live().unwrap(), before);

    // The items would span more than a full turn.
    let mut wide = before.clone();
    wide.items = 4;
    wide.between = Some(180.0);
    assert!(s.set_array_live(&wide).is_err());
    assert_eq!(s.array_live().unwrap(), before);
}

#[test]
fn commit_at_the_center_refuses_and_leaves_the_command_untouched() {
    let mut s = selected();
    s.start("arraypolar").unwrap();
    let before_doc = s.doc().unwrap().clone();
    let before_prompt = s.current_prompt().unwrap().message.clone();
    let before_live = s.array_live().unwrap();

    assert!(!s.commit_array().unwrap());
    assert!(s.running.is_some());
    assert_eq!(s.current_prompt().unwrap().message, before_prompt);
    assert_eq!(s.doc().unwrap(), &before_doc);
    assert_eq!(s.array_live().unwrap(), before_live);
}

#[test]
fn commit_after_an_edited_center_succeeds() {
    let mut s = selected();
    s.start("arraypolar").unwrap();
    let mut v = s.array_live().unwrap();
    v.center = Some(Vec2::new(7.0, -2.0));
    s.set_array_live(&v).unwrap();
    assert!(!s.current_prompt().unwrap().message.contains("center point of array"));

    assert!(s.commit_array().unwrap());
    assert!(s.running.is_none());
}

#[test]
fn commit_from_the_items_stage_succeeds() {
    let mut s = selected();
    s.start("arraypolar").unwrap();
    s.input(Input::Point(Vec2::new(1.0, 1.0))).unwrap();
    s.cmdline("items").unwrap();
    assert!(s.current_prompt().unwrap().message.contains("items"));

    assert!(s.commit_array().unwrap());
    assert!(s.running.is_none());
}

#[test]
fn commit_at_the_center_base_stage_refuses() {
    let mut s = selected();
    s.start("arraypolar").unwrap();
    s.cmdline("base point").unwrap();
    assert!(s.current_prompt().unwrap().message.contains("base point"));
    let before = s.array_live().unwrap();

    assert!(!s.commit_array().unwrap());
    assert!(s.running.is_some());
    assert_eq!(s.array_live().unwrap(), before);
}

/// The live setter enforces MAXARRAY just like the commit path.
fn low_maxarray(s: &mut Session, max: i64) {
    s.execute("setvar", &json!({ "name": "MAXARRAY", "value": max })).unwrap();
}

#[test]
fn live_rect_refuses_an_oversized_total_unchanged() {
    let mut s = selected();
    s.start("arrayrect").unwrap();
    low_maxarray(&mut s, 100);
    let before = s.array_live().unwrap();

    let mut big = before.clone();
    (big.rows, big.cols) = (10, 11);
    let e = s.set_array_live(&big).unwrap_err();
    assert!(e.to_string().contains("MAXARRAY"), "{e}");
    assert_eq!(s.array_live().unwrap(), before);
}

#[test]
fn live_polar_refuses_an_oversized_total_unchanged() {
    let mut s = selected();
    s.start("arraypolar").unwrap();
    low_maxarray(&mut s, 100);
    let before = s.array_live().unwrap();

    let mut big = before.clone();
    (big.items, big.rows) = (20, 6);
    let e = s.set_array_live(&big).unwrap_err();
    assert!(e.to_string().contains("MAXARRAY"), "{e}");
    assert_eq!(s.array_live().unwrap(), before);
}

#[test]
fn live_rect_accepts_exactly_at_the_limit() {
    let mut s = selected();
    s.start("arrayrect").unwrap();
    low_maxarray(&mut s, 100);
    let mut v = s.array_live().unwrap();
    v.associative = false;
    (v.rows, v.cols) = (10, 10);
    s.set_array_live(&v).unwrap();
    assert_eq!((s.array_live().unwrap().rows, s.array_live().unwrap().cols), (10, 10));
}

#[test]
fn live_rect_excludes_levels_when_not_associative() {
    let mut s = selected();
    s.start("arrayrect").unwrap();
    low_maxarray(&mut s, 100);
    // rows × cols = 100 fits; rows × cols × levels would not, but levels is ignored.
    let mut v = s.array_live().unwrap();
    v.associative = false;
    (v.rows, v.cols, v.levels) = (10, 10, 50);
    s.set_array_live(&v).unwrap();
    assert_eq!(s.array_live().unwrap().levels, 50);
}
