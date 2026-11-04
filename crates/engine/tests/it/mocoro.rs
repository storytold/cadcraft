//! MOCORO (Express Tools Move/Copy/Rotate): select objects and a base point, then Move, Copy,
//! Rotate, Scale, Base and Undo at one prompt until eXit.

use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> String {
    let r = s.execute("line", &json!({"points": [a, b]})).unwrap();
    r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string()
}

/// The end points of a line.
fn ends(s: &Session, h: &str) -> ((f64, f64), (f64, f64)) {
    let e = s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind.clone();
    match e {
        EntityKind::Line(l) => ((l.a.x, l.a.y), (l.b.x, l.b.y)),
        k => panic!("not a line: {k:?}"),
    }
}

/// Every line in model space, by its end points.
fn lines(s: &Session) -> Vec<((f64, f64), (f64, f64))> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some(((l.a.x, l.a.y), (l.b.x, l.b.y))) } else { None })
        .collect()
}

fn near(a: ((f64, f64), (f64, f64)), b: ((f64, f64), (f64, f64))) -> bool {
    let c = |x: f64, y: f64| (x - y).abs() < 1e-9;
    c(a.0.0, b.0.0) && c(a.0.1, b.0.1) && c(a.1.0, b.1.0) && c(a.1.1, b.1.1)
}

fn undo_len(s: &Session) -> usize {
    s.state().unwrap().undo.len()
}

#[test]
fn move_carries_the_base_point_and_rotate_turns_about_it() {
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let undo = undo_len(&s);
    s.script("MOCORO ALL  0,0 M 10,0\n").unwrap();
    assert!(near(ends(&s, &l), ((10.0, 0.0), (20.0, 0.0))), "{:?} {:?}", ends(&s, &l), s.log);
    assert!(s.prompt_text().contains("Move"), "back at the option prompt: {}", s.prompt_text());
    // The base point moved with the objects: the turn is about 10,0.
    s.script("R 90\n").unwrap();
    assert!(near(ends(&s, &l), ((10.0, 0.0), (10.0, 10.0))), "{:?}", ends(&s, &l));
    s.script("X\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(undo_len(&s), undo + 1, "the whole command is one undo step");
    s.undo().unwrap();
    assert!(near(ends(&s, &l), ((0.0, 0.0), (10.0, 0.0))));
}

#[test]
fn copy_leaves_the_originals_and_later_options_act_on_them() {
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.script("MOCORO ALL  0,0 C 0,5\n").unwrap();
    assert_eq!(lines(&s).len(), 2, "{:?}", s.log);
    assert!(near(ends(&s, &l), ((0.0, 0.0), (10.0, 0.0))), "the original stays");
    // The next option moves the original from the same base point, not the copy.
    s.script("M 100,0 X\n").unwrap();
    assert!(near(ends(&s, &l), ((100.0, 0.0), (110.0, 0.0))), "{:?}", ends(&s, &l));
    assert!(lines(&s).iter().any(|x| near(*x, ((0.0, 5.0), (10.0, 5.0)))), "{:?}", lines(&s));
    assert!(s.running.is_none());
}

#[test]
fn scale_by_factor_or_point_and_refuses_a_zero_factor() {
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.script("MOCORO ALL  0,0 S 2\n").unwrap();
    assert!(near(ends(&s, &l), ((0.0, 0.0), (20.0, 0.0))), "{:?} {:?}", ends(&s, &l), s.log);
    // A factor given by a point is its distance from the base point.
    s.script("S 3,4\n").unwrap();
    assert!(near(ends(&s, &l), ((0.0, 0.0), (100.0, 0.0))), "{:?}", ends(&s, &l));
    // Zero is refused and the factor is asked again; Enter goes back to the option prompt.
    s.script("S 0\n").unwrap();
    assert!(s.prompt_text().contains("scale factor"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    assert!(s.prompt_text().contains("Rotate"), "{}", s.prompt_text());
    assert!(near(ends(&s, &l), ((0.0, 0.0), (100.0, 0.0))));
    s.cmdline("").unwrap();
    assert!(s.running.is_none(), "Enter at the option prompt is eXit");
}

#[test]
fn rotate_follows_angdir() {
    let mut s = Session::new();
    s.execute("setvar", &json!({"name": "ANGDIR", "value": 1})).unwrap();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.script("MOCORO ALL  0,0 R 90 X\n").unwrap();
    assert!(near(ends(&s, &l), ((0.0, 0.0), (0.0, -10.0))), "clockwise: {:?}", ends(&s, &l));
}

#[test]
fn undo_steps_back_one_option_and_restores_the_base_point() {
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.script("MOCORO ALL  0,0 M 10,0 C 0,5 R 90\n").unwrap();
    assert_eq!(lines(&s).len(), 2);
    s.script("U\n").unwrap();
    assert!(near(ends(&s, &l), ((10.0, 0.0), (20.0, 0.0))), "{:?}", ends(&s, &l));
    s.script("U\n").unwrap();
    assert_eq!(lines(&s).len(), 1, "the copy is undone");
    s.script("U\n").unwrap();
    assert!(near(ends(&s, &l), ((0.0, 0.0), (10.0, 0.0))));
    s.script("U\n").unwrap();
    assert!(s.log.iter().any(|m| m.contains("Nothing to undo")), "{:?}", s.log);
    // The base point is back at 0,0 too.
    s.script("R 90 X\n").unwrap();
    assert!(near(ends(&s, &l), ((0.0, 0.0), (0.0, 10.0))), "{:?}", ends(&s, &l));
}

#[test]
fn escape_keeps_what_was_done_as_one_undo_step() {
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let undo = undo_len(&s);
    s.script("MOCORO ALL  0,0 M 10,0 C 0,5 S\n").unwrap();
    assert!(s.prompt_text().contains("scale factor"), "{}", s.prompt_text());
    s.cancel();
    assert!(s.running.is_none());
    assert_eq!(lines(&s).len(), 2);
    assert_eq!(undo_len(&s), undo + 1);
    s.undo().unwrap();
    assert_eq!(lines(&s).len(), 1);
    assert!(near(ends(&s, &l), ((0.0, 0.0), (10.0, 0.0))));
}

#[test]
fn a_point_at_the_option_prompt_is_not_an_option() {
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let undo = undo_len(&s);
    s.script("MOCORO ALL  0,0\n").unwrap();
    s.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    assert!(s.log.iter().any(|m| m.contains("Invalid option keyword")), "{:?}", s.log);
    assert!(s.running.is_some());
    assert!(near(ends(&s, &l), ((0.0, 0.0), (10.0, 0.0))));
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(undo_len(&s), undo, "nothing done, no undo step");
}

#[test]
fn objects_on_locked_layers_stay_put_but_can_be_copied() {
    let mut s = Session::new();
    let free = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.execute("layer.new", &json!({"name": "L"})).unwrap();
    s.execute("layer.current", &json!({"name": "L"})).unwrap();
    let held = line(&mut s, [0.0, 5.0], [10.0, 5.0]);
    s.execute("layer.current", &json!({"name": "0"})).unwrap();
    s.execute("layer.set", &json!({"name": "L", "locked": true})).unwrap();
    s.script("MOCORO ALL  0,0 M 1,0\n").unwrap();
    assert!(near(ends(&s, &free), ((1.0, 0.0), (11.0, 0.0))));
    assert!(near(ends(&s, &held), ((0.0, 5.0), (10.0, 5.0))));
    assert!(s.log.iter().any(|m| m.contains("1 object was on a locked layer")), "{:?}", s.log);
    s.script("X\n").unwrap();

    // Everything locked: nothing moves and the command leaves no undo step.
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.execute("layer.set", &json!({"name": "0", "locked": true})).unwrap();
    let undo = undo_len(&s);
    s.script("MOCORO ALL  0,0 M 1,0 R 90 X\n").unwrap();
    assert!(s.running.is_none());
    assert!(near(ends(&s, &l), ((0.0, 0.0), (10.0, 0.0))));
    assert_eq!(undo_len(&s), undo, "{:?}", s.log);
}

#[test]
fn json_form_runs_the_options_in_order() {
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let r = s
        .execute(
            "mocoro",
            &json!({"handles": [l], "base": [0, 0], "ops": [{"copy": [0, 5]}, {"move": [10, 0]}, {"rotate": 90}, {"base": [0, 0]}, {"scale": 2}]}),
        )
        .unwrap();
    assert_eq!(r["handles"].as_array().map(Vec::len), Some(1), "{r}");
    // Moved to 10..20, turned about 10,0 to 10,0..10,10, scaled about 0,0.
    assert!(near(ends(&s, &l), ((20.0, 0.0), (20.0, 20.0))), "{:?}", ends(&s, &l));
    assert!(lines(&s).iter().any(|x| near(*x, ((0.0, 5.0), (10.0, 5.0)))));
}

#[test]
fn json_form_is_checked_before_the_drawing_changes() {
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let bad: [Value; 7] = [
        json!({"handles": [l], "ops": [{"move": [1, 0]}]}),
        json!({"handles": [l], "base": [0, 0]}),
        json!({"handles": [l], "base": [0, 0], "ops": [{"move": [1, 0]}, {"scale": 0}]}),
        json!({"handles": [l], "base": [0, 0], "ops": [{"move": [1, 0]}, {"rotate": "x"}]}),
        json!({"handles": [l], "base": [0, 0], "ops": [{"move": [1, 0]}, {"spin": 3}]}),
        json!({"handles": [l], "base": [0, 0], "ops": [{"move": [1, 0], "copy": [0, 1]}]}),
        json!({"handles": [l], "base": [0, 0], "ops": [{"move": [1, 0]}, {"scale": 1e-300}]}),
    ];
    for p in bad {
        assert!(s.execute("mocoro", &p).is_err(), "{p}");
        assert!(near(ends(&s, &l), ((0.0, 0.0), (10.0, 0.0))), "{p} moved the line");
        assert_eq!(lines(&s).len(), 1);
    }
}

#[test]
fn undo_after_base_undoes_only_the_new_base_point() {
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.script("MOCORO ALL  0,0 M 10,0 B 50,50 U\n").unwrap();
    assert!(near(ends(&s, &l), ((10.0, 0.0), (20.0, 0.0))), "the move stays: {:?}", ends(&s, &l));
    // The base point is back where the move left it: the turn is about 10,0.
    s.script("R 90 X\n").unwrap();
    assert!(near(ends(&s, &l), ((10.0, 0.0), (10.0, 10.0))), "{:?}", ends(&s, &l));
}

#[test]
fn options_on_objects_that_are_gone_record_nothing() {
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.execute("erase", &json!({"handles": [l]})).unwrap();
    let undo = undo_len(&s);
    let r = s.execute("mocoro", &json!({"handles": [l], "base": [0, 0], "ops": [{"move": [1, 0]}, {"copy": [0, 1]}, {"rotate": 90}]})).unwrap();
    assert_eq!(r["handles"].as_array().map(Vec::len), Some(0), "{r}");
    assert!(lines(&s).is_empty());
    assert_eq!(undo_len(&s), undo, "no empty undo step");
}
