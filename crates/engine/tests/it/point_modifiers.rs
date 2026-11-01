//! Point modifiers at point prompts (#392): FROM, M2P/MTP, point filters, the `*` prefix and the
//! angle override `<a`, from the command line and in scripts.

use cadcraft_doc::EntityKind;
use cadcraft_engine::{Input, Session};
use cadcraft_geom::Vec2;

/// The lines of model space as (start, end).
fn lines(s: &Session) -> Vec<(Vec2, Vec2)> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some((l.a.xy(), l.b.xy())) } else { None }).collect()
}

fn line_is(l: Option<&(Vec2, Vec2)>, a: (f64, f64), b: (f64, f64)) -> bool {
    l.is_some_and(|(p, q)| p.near(Vec2::new(a.0, a.1), 1e-9) && q.near(Vec2::new(b.0, b.1), 1e-9))
}

#[test]
fn from_takes_a_base_point_and_an_offset() {
    let mut s = Session::new();
    s.cmdline("line from").unwrap();
    assert!(s.prompt_text().ends_with("Base point:"), "{}", s.prompt_text());
    s.cmdline("10,10").unwrap();
    assert!(s.prompt_text().ends_with("<Offset>:"), "{}", s.prompt_text());
    assert_eq!(s.current_prompt().unwrap().base, Some(Vec2::new(10.0, 10.0)), "rubber band from the base point");
    s.cmdline("@5,0").unwrap();
    assert!(s.prompt_text().contains("Specify next point"), "back at the command's prompt: {}", s.prompt_text());
    s.cmdline("@0,5").unwrap();
    s.cmdline("").unwrap();
    assert!(line_is(lines(&s).first(), (15.0, 10.0), (15.0, 15.0)), "{:?}", lines(&s));

    // Any command's point prompt takes it: the centre of a circle 3,4 from the origin.
    s.cmdline("circle from 0,0 @3,4 2").unwrap();
    let Some(EntityKind::Circle(c)) = s.doc().unwrap().model.iter().last().map(|e| e.kind.clone()) else { panic!("no circle") };
    assert!(c.center.xy().near(Vec2::new(3.0, 4.0), 1e-9) && (c.radius - 2.0).abs() < 1e-9);
}

#[test]
fn m2p_and_mtp_give_the_midpoint_between_two_points() {
    let mut s = Session::new();
    s.cmdline("line m2p").unwrap();
    assert!(s.prompt_text().ends_with("First point of mid:"), "{}", s.prompt_text());
    s.cmdline("0,0").unwrap();
    assert!(s.prompt_text().ends_with("Second point of mid:"), "{}", s.prompt_text());
    s.cmdline("10,4").unwrap();
    s.cmdline("_mtp 20,0 20,20").unwrap();
    s.cmdline("").unwrap();
    assert!(line_is(lines(&s).first(), (5.0, 2.0), (20.0, 10.0)), "{:?}", lines(&s));
}

#[test]
fn point_filters_take_coordinates_from_two_points() {
    let mut s = Session::new();
    s.cmdline("line 0,0 .x").unwrap();
    assert!(s.prompt_text().ends_with(".X of:"), "{}", s.prompt_text());
    s.cmdline("7,99").unwrap();
    assert!(s.prompt_text().ends_with("(need YZ):"), "{}", s.prompt_text());
    s.cmdline("3,4").unwrap();
    s.cmdline(".Y 99,6").unwrap();
    s.input(Input::Point(Vec2::new(1.0, 99.0))).unwrap();
    s.cmdline(".xy 8,9 0,0,5").unwrap();
    s.cmdline("").unwrap();
    let l = lines(&s);
    assert!(line_is(l.first(), (0.0, 0.0), (7.0, 4.0)), "{l:?}");
    assert!(line_is(l.get(1), (7.0, 4.0), (1.0, 6.0)), "{l:?}");
    assert!(line_is(l.get(2), (1.0, 6.0), (8.0, 9.0)), "{l:?}");
}

#[test]
fn star_prefix_reads_world_coordinates() {
    let mut s = Session::new();
    s.cmdline("line *1,1 *@2,0 @*0,3").unwrap();
    s.cmdline("").unwrap();
    let l = lines(&s);
    assert!(line_is(l.first(), (1.0, 1.0), (3.0, 1.0)) && line_is(l.get(1), (3.0, 1.0), (3.0, 4.0)), "{l:?}");
}

#[test]
fn angle_override_locks_the_direction_of_the_next_point() {
    let mut s = Session::new();
    s.cmdline("line 1,1 <90").unwrap();
    assert!(s.log.iter().any(|l| l == "Angle Override: 90"), "{:?}", s.log);
    assert!(s.prompt_text().contains("Specify next point"), "the command's prompt stays: {}", s.prompt_text());
    // A typed distance runs along the locked angle.
    s.cmdline("5").unwrap();
    // A picked point is brought onto the locked line.
    s.cmdline("<0").unwrap();
    s.input(Input::Point(Vec2::new(4.0, 9.0))).unwrap();
    // The lock lasts one point only.
    s.input(Input::Point(Vec2::new(4.0, 9.0))).unwrap();
    s.cmdline("").unwrap();
    let l = lines(&s);
    assert!(line_is(l.first(), (1.0, 1.0), (1.0, 6.0)), "{l:?}");
    assert!(line_is(l.get(1), (1.0, 6.0), (4.0, 6.0)), "{l:?}");
    assert!(line_is(l.get(2), (4.0, 6.0), (4.0, 9.0)), "{l:?}");

    // A keyword after an override goes to the command (Undo the last segment).
    s.cmdline("line 0,0 5,0 <45 u").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(lines(&s).len(), 3, "the segment was undone");
}

#[test]
fn modifiers_nest_and_work_in_scripts() {
    let mut s = Session::new();
    // FROM whose base point is the midpoint of 0,0 and 10,0, then an offset of @0,3.
    s.script("LINE FROM M2P 0,0 10,0 @0,3\n20,3\n\n").unwrap();
    assert!(s.running.is_none());
    assert!(line_is(lines(&s).first(), (5.0, 3.0), (20.0, 3.0)), "{:?}", lines(&s));

    // One token per line as well.
    s.script("LINE\n_from\n0,0\n@1,1\n.y\n0,5\n2,0\n\n").unwrap();
    assert!(line_is(lines(&s).get(1), (1.0, 1.0), (2.0, 5.0)), "{:?}", lines(&s));
}

#[test]
fn enter_drops_the_modifiers_and_text_is_not_a_point() {
    let mut s = Session::new();
    s.cmdline("line from").unwrap();
    s.cmdline("hello").unwrap();
    assert!(s.log.iter().any(|l| l == "Invalid point."));
    assert!(s.prompt_text().ends_with("Base point:"));
    s.cmdline("").unwrap();
    assert!(s.running.is_some(), "LINE still runs");
    assert!(s.prompt_text().contains("Specify first point"), "{}", s.prompt_text());
    assert!(s.point_mods.is_empty());
    // Esc cancels the command and the modifiers with it.
    s.cmdline("m2p").unwrap();
    s.cancel();
    assert!(s.running.is_none() && s.point_mods.is_empty());
    // A runaway script can't pile up modifiers without bound.
    s.cmdline(&format!("line{}", " from".repeat(100))).unwrap();
    assert!(s.point_mods.len() <= 32);
}
