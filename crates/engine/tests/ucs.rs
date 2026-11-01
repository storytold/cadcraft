//! The user coordinate system (#1106): the UCS command and its options, typed coordinates and
//! directions read in the current UCS, coordinates shown in it, the UCS system variables,
//! UCSICON, paper space's own UCS, undo, and hostile input.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, HVal, Space};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_engine::ucs::UcsFrame;
use serde_json::{Value, json};

const TAU: f64 = std::f64::consts::TAU;

fn session() -> Session {
    let mut s = Session::new();
    s.new_drawing(false);
    s
}

fn typed(s: &mut Session, lines: &[&str]) {
    for l in lines {
        s.cmdline(l).unwrap();
    }
}

fn near(a: Vec2, x: f64, y: f64) -> bool {
    a.dist(Vec2::new(x, y)) < 1e-9
}

fn near3(a: Vec3, b: Vec3) -> bool {
    (a - b).len() < 1e-9
}

fn ang_eq(a: f64, b: f64) -> bool {
    let d = (a - b).rem_euclid(TAU);
    d < 1e-9 || TAU - d < 1e-9
}

fn lines(s: &Session) -> Vec<(Vec2, Vec2)> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some((l.a.xy(), l.b.xy())) } else { None }).collect()
}

fn last_line(s: &Session) -> (Vec2, Vec2) {
    *lines(s).last().expect("a line")
}

fn logged(s: &Session, text: &str) -> bool {
    s.log.iter().any(|l| l.contains(text))
}

fn var(s: &Session, name: &str) -> Value {
    cadcraft_engine::sysvars::get(s, name).unwrap_or(Value::Null)
}

fn vec3(v: &Value) -> Vec3 {
    Vec3::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap())
}

/// A UCS at (100, 50) turned 90°: UCS X is world +Y, UCS Y is world -X.
fn turned(s: &mut Session) {
    s.execute("ucs", &json!({"option": "origin", "origin": [100, 50]})).unwrap();
    s.execute("ucs", &json!({"option": "z", "angle": 90})).unwrap();
    let u = s.ucs();
    assert!(near3(u.origin, Vec3::new(100.0, 50.0, 0.0)) && near3(u.x, Vec3::new(0.0, 1.0, 0.0)), "{u:?}");
}

// ---------------- the UCS command ----------------

#[test]
fn a_new_drawing_is_in_the_world_ucs() {
    let s = session();
    assert!(s.ucs().is_world());
    assert_eq!(var(&s, "WORLDUCS"), json!(1));
    assert_eq!(var(&s, "UCSNAME"), json!(""));
    assert!(near3(vec3(&var(&s, "UCSORG")), Vec3::ZERO));
    assert!(near3(vec3(&var(&s, "UCSXDIR")), Vec3::new(1.0, 0.0, 0.0)));
    assert!(near3(vec3(&var(&s, "UCSYDIR")), Vec3::new(0.0, 1.0, 0.0)));
}

#[test]
fn ucs_origin_then_enter_moves_the_origin_and_keeps_the_axes() {
    let mut s = session();
    typed(&mut s, &["ucs 100,50 "]);
    assert!(logged(&s, "Current ucs name:  *WORLD*"));
    let u = s.ucs();
    assert!(near3(u.origin, Vec3::new(100.0, 50.0, 0.0)) && near3(u.x, Vec3::new(1.0, 0.0, 0.0)));
    assert_eq!(var(&s, "WORLDUCS"), json!(0));
    assert!(near3(vec3(&var(&s, "UCSORG")), Vec3::new(100.0, 50.0, 0.0)));
    typed(&mut s, &["ucs "]);
    assert!(logged(&s, "Current ucs name:  *NO NAME*"), "{:?}", s.log);
    assert!(s.ucs().is_world(), "Enter at the first prompt is World");
}

#[test]
fn ucs_by_three_points() {
    let mut s = session();
    // Origin, a point on the new X axis (straight up), Enter: Y square to it.
    typed(&mut s, &["ucs 10,10 10,30 "]);
    let u = s.ucs();
    assert!(near3(u.origin, Vec3::new(10.0, 10.0, 0.0)) && near3(u.x, Vec3::new(0.0, 1.0, 0.0)) && near3(u.y, Vec3::new(-1.0, 0.0, 0.0)), "{u:?}");
    // All three points; typed points are read in the UCS current while UCS runs.
    typed(&mut s, &["ucs w", "ucs 0,0 1,1 -1,1"]);
    let u = s.ucs();
    let r = std::f64::consts::FRAC_1_SQRT_2;
    assert!(near3(u.x, Vec3::new(r, r, 0.0)) && near3(u.y, Vec3::new(-r, r, 0.0)) && u.is_planar(), "{u:?}");
    // A third point below the X axis would turn the UCS over: refused, and the UCS is kept.
    let before = s.ucs();
    let _ = s.cmdline("ucs 0,0 10,0 5,-5");
    assert!(near3(s.ucs().x, before.x), "a turned-over UCS is not taken");
    s.cancel();
    // Points on one line define nothing.
    assert!(s.execute("ucs", &json!({"origin": [0, 0], "xPoint": [10, 0], "xyPoint": [20, 0]})).is_err());
    assert!(s.execute("ucs", &json!({"origin": [0, 0], "xPoint": [0, 0]})).is_err());
}

#[test]
fn ucs_z_turns_about_z_and_defaults_to_ucsaxisang() {
    let mut s = session();
    typed(&mut s, &["ucs z 30"]);
    assert!(ang_eq(s.ucs().rotation(), 30f64.to_radians()));
    typed(&mut s, &["ucs z "]);
    assert!(ang_eq(s.ucs().rotation(), 120f64.to_radians()), "Enter takes UCSAXISANG (90)");
    cadcraft_engine::sysvars::set(&mut s, "UCSAXISANG", &json!(45)).unwrap();
    typed(&mut s, &["ucs z "]);
    assert!(ang_eq(s.ucs().rotation(), 165f64.to_radians()));
    assert!(cadcraft_engine::sysvars::set(&mut s, "UCSAXISANG", &json!(33)).is_err(), "only the listed angles");
    typed(&mut s, &["ucs z -165"]);
    assert!(ang_eq(s.ucs().rotation(), 0.0));
    // 30 is one of the listed angles too.
    cadcraft_engine::sysvars::set(&mut s, "UCSAXISANG", &json!(30)).unwrap();
    typed(&mut s, &["ucs z "]);
    assert!(ang_eq(s.ucs().rotation(), 30f64.to_radians()));
}

#[test]
fn ucs_previous_walks_back_ten_steps_per_space() {
    let mut s = session();
    for i in 1..=12 {
        s.execute("ucs", &json!({"option": "origin", "origin": [i, 0]})).unwrap();
    }
    for i in (2..=11).rev() {
        typed(&mut s, &["ucs p"]);
        assert!(near3(s.ucs().origin, Vec3::new(f64::from(i), 0.0, 0.0)), "back to {i}: {:?}", s.ucs());
    }
    let at = s.ucs();
    typed(&mut s, &["ucs p"]);
    assert!(logged(&s, "There is no previous UCS."));
    assert_eq!(s.ucs(), at, "only ten are kept");
    assert!(s.execute("ucs", &json!({"option": "previous"})).is_err());
}

#[test]
fn named_ucs_save_restore_list_delete() {
    let mut s = session();
    turned(&mut s);
    typed(&mut s, &["ucs na s SITE"]);
    assert_eq!(var(&s, "UCSNAME"), json!("SITE"));
    assert_eq!(s.doc().unwrap().ucss.len(), 1);
    typed(&mut s, &["ucs w"]);
    assert!(s.ucs().is_world());
    typed(&mut s, &["ucs na r site"]);
    assert!(near3(s.ucs().x, Vec3::new(0.0, 1.0, 0.0)) && var(&s, "UCSNAME") == json!("SITE"), "restored by any case");
    // Saving over a name asks first.
    typed(&mut s, &["ucs z 10", "ucs na s SITE", "n"]);
    let saved = s.doc().unwrap().ucss[0].clone();
    assert!(near3(saved.x_axis, Vec3::new(0.0, 1.0, 0.0)), "No keeps the saved one");
    typed(&mut s, &["ucs na s SITE", "y"]);
    assert!(!near3(s.doc().unwrap().ucss[0].x_axis, Vec3::new(0.0, 1.0, 0.0)), "Yes replaces it");
    // Listing.
    s.execute("ucs", &json!({"option": "named", "action": "save", "name": "Base 2"})).unwrap();
    let list = s.execute("ucs", &json!({"option": "named", "action": "list"})).unwrap()["list"].as_str().unwrap().to_string();
    assert!(list.contains("SITE") && list.contains("Base 2") && list.contains("Origin = <") && list.contains("Z Axis = <"), "{list}");
    typed(&mut s, &["ucs na ? *"]);
    assert!(logged(&s, "Saved coordinate systems:"));
    // Deleting by wildcard, and the current UCS loses a deleted name.
    let r = s.execute("ucs", &json!({"option": "named", "action": "delete", "name": "BASE*"})).unwrap();
    assert_eq!(r["deleted"], json!(1));
    assert_eq!(var(&s, "UCSNAME"), json!(""), "the current one was Base 2");
    typed(&mut s, &["ucs na d SITE"]);
    assert!(s.doc().unwrap().ucss.is_empty());
    // Unknown and invalid names.
    assert!(s.execute("ucs", &json!({"option": "named", "action": "restore", "name": "NOPE"})).is_err());
    for bad in ["", "   ", "a<b", "x*", "semi;colon", "World", "*WORLD*", &"n".repeat(300)] {
        assert!(s.execute("ucs", &json!({"option": "named", "action": "save", "name": bad})).is_err(), "{bad:?}");
    }
    assert!(s.execute("ucs", &json!({"option": "named", "action": "save", "name": "SITE"})).is_ok());
    assert!(s.execute("ucs", &json!({"option": "named", "action": "save", "name": "site"})).is_err(), "names are case-blind");
    assert!(s.execute("ucs", &json!({"option": "named", "action": "save", "name": "site", "replace": true})).is_ok());
    assert_eq!(s.doc().unwrap().ucss.len(), 1);
}

#[test]
fn ucs_object_follows_each_kind_of_object() {
    let mut s = session();
    let at = |s: &mut Session, kind: EntityKind, pick: [f64; 2]| -> Result<UcsFrame, String> {
        let h = s.add_entity(kind).unwrap();
        s.execute("ucs", &json!({"option": "world"})).unwrap();
        s.execute("ucs", &json!({"option": "object", "handle": h.hex(), "at": pick})).map_err(|e| e.to_string())?;
        Ok(s.ucs())
    };
    use cadcraft_engine::doc::{Arc, Circle, Face3d, Line, LwPolyline, Point};
    // A line: the nearer end; X along the line.
    let u = at(&mut s, EntityKind::Line(Line { a: Vec3::new(10.0, 10.0, 0.0), b: Vec3::new(10.0, 30.0, 0.0) }), [10.0, 29.0]).unwrap();
    assert!(near3(u.origin, Vec3::new(10.0, 30.0, 0.0)) && near3(u.x, Vec3::new(0.0, -1.0, 0.0)), "{u:?}");
    // A circle: the centre; X through the pick.
    let u = at(&mut s, EntityKind::Circle(Circle { center: Vec3::new(5.0, 5.0, 2.0), radius: 3.0 }), [5.0, 8.0]).unwrap();
    assert!(near3(u.origin, Vec3::new(5.0, 5.0, 2.0)) && near3(u.x, Vec3::new(0.0, 1.0, 0.0)), "{u:?}");
    // An arc: the centre; X through the nearer end.
    let u = at(&mut s, EntityKind::Arc(Arc { center: Vec3::ZERO, radius: 2.0, start: 0.0, end: std::f64::consts::FRAC_PI_2 }), [0.1, 2.0]).unwrap();
    assert!(near3(u.x, Vec3::new(0.0, 1.0, 0.0)), "{u:?}");
    // A polyline: its first vertex; X toward the next.
    let pl = LwPolyline {
        vertices: vec![
            cadcraft_engine::geom::PolyVertex::new(Vec2::new(1.0, 1.0)),
            cadcraft_engine::geom::PolyVertex::new(Vec2::new(1.0, 1.0)),
            cadcraft_engine::geom::PolyVertex::new(Vec2::new(-4.0, 1.0)),
        ],
        closed: false,
        const_width: 0.0,
        elevation: 3.0,
        plinegen: false,
    };
    let u = at(&mut s, EntityKind::LwPolyline(pl), [0.0, 0.0]).unwrap();
    assert!(near3(u.origin, Vec3::new(1.0, 1.0, 3.0)) && near3(u.x, Vec3::new(-1.0, 0.0, 0.0)), "{u:?}");
    // Text (and blocks, by the same rule): the insertion point; X along its rotation.
    s.execute("ucs", &json!({"option": "world"})).unwrap();
    typed(&mut s, &["text 2,3 1 28.64788975654116 A", ""]);
    let th = s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Text(_))).map(|e| e.handle).last().unwrap();
    s.execute("ucs", &json!({"option": "object", "handle": th.hex()})).unwrap();
    let u = s.ucs();
    assert!(near3(u.origin, Vec3::new(2.0, 3.0, 0.0)) && ang_eq(u.rotation(), 0.5), "{u:?}");
    // A point: there, the X axis kept.
    let u = at(&mut s, EntityKind::Point(Point { p: Vec3::new(4.0, 4.0, 0.0), angle: 0.0 }), [0.0, 0.0]).unwrap();
    assert!(near3(u.origin, Vec3::new(4.0, 4.0, 0.0)) && near3(u.x, Vec3::new(1.0, 0.0, 0.0)));
    // A flat 3D face works; a tilted one, an ellipse and a zero-length line don't.
    let flat = Face3d { corners: [Vec3::ZERO, Vec3::new(0.0, 5.0, 0.0), Vec3::new(-5.0, 5.0, 0.0), Vec3::new(-5.0, 0.0, 0.0)], hidden_edges: 0 };
    let u = at(&mut s, EntityKind::Face3d(flat), [0.0, 0.0]).unwrap();
    assert!(near3(u.x, Vec3::new(0.0, 1.0, 0.0)) && u.is_planar());
    let tilted = Face3d { corners: [Vec3::ZERO, Vec3::new(5.0, 0.0, 0.0), Vec3::new(5.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 5.0)], hidden_edges: 0 };
    assert!(at(&mut s, EntityKind::Face3d(tilted), [0.0, 0.0]).unwrap_err().contains("not available yet"));
    let ell = cadcraft_engine::doc::Ellipse { center: Vec3::ZERO, major: Vec3::new(2.0, 0.0, 0.0), ratio: 0.5, start: 0.0, end: TAU };
    assert!(at(&mut s, EntityKind::Ellipse(ell), [0.0, 0.0]).is_err());
    assert!(at(&mut s, EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(0.0, 0.0, 9.0) }), [0.0, 0.0]).is_err());
    assert!(s.ucs().is_world(), "a refused object leaves the UCS");
    // Interactively: a pick on the object.
    s.execute("line", &json!({"points": [[50, 50], [50, 80]]})).unwrap();
    typed(&mut s, &["ucs ob 50,79"]);
    assert!(near3(s.ucs().origin, Vec3::new(50.0, 80.0, 0.0)), "{:?}", s.ucs());
    let _ = s.cmdline("ucs ob 900,900");
    assert!(s.running.is_some() || logged(&s, "No object found"), "a miss asks again");
    s.cancel();
}

#[test]
fn ucs_view_and_the_tilted_options() {
    let mut s = session();
    turned(&mut s);
    typed(&mut s, &["ucs v"]);
    let u = s.ucs();
    assert!(near3(u.origin, Vec3::new(100.0, 50.0, 0.0)) && near3(u.x, Vec3::new(1.0, 0.0, 0.0)), "square to the plan view, origin kept");
    for k in ["x", "y", "za", "f"] {
        s.cmdline(&format!("ucs {k}")).unwrap();
        assert!(logged(&s, "not available yet"), "{k}");
        assert!(s.running.is_some(), "{k}: still at the prompt");
        s.cancel();
        assert!(s.execute("ucs", &json!({"option": k})).unwrap_err().to_string().contains("not available yet"), "{k}");
    }
}

#[test]
fn a_ucs_change_is_undone_like_an_edit() {
    let mut s = session();
    typed(&mut s, &["ucs 100,50 "]);
    typed(&mut s, &["ucs z 45"]);
    s.undo().unwrap();
    let u = s.ucs();
    assert!(near3(u.origin, Vec3::new(100.0, 50.0, 0.0)) && near3(u.x, Vec3::new(1.0, 0.0, 0.0)), "{u:?}");
    s.undo().unwrap();
    assert!(s.ucs().is_world());
    s.redo().unwrap();
    assert!(near3(s.ucs().origin, Vec3::new(100.0, 50.0, 0.0)));
}

/// Undo restores UCS Previous's history with the UCS: undoing UCS Previous gives back the entry
/// it used, and undoing a UCS change takes away the entry it added.
#[test]
fn undo_restores_the_ucs_previous_history() {
    let previous = |s: &mut Session| s.execute("ucs", &json!({"option": "previous"})).is_ok();
    let mut s = session();
    typed(&mut s, &["ucs z 30"]);
    assert!(previous(&mut s) && s.ucs().is_world());
    s.undo().unwrap();
    assert!(ang_eq(s.ucs().rotation(), 30f64.to_radians()), "undo of UCS Previous");
    // Redo (before any new command clears the redo stack) does UCS Previous again.
    assert!(s.redo().unwrap().is_some(), "UCS Previous is on the redo stack");
    assert!(s.ucs().is_world(), "redo of UCS Previous");
    s.undo().unwrap();
    assert!(previous(&mut s) && s.ucs().is_world(), "its history entry came back too");
    let mut s = session();
    typed(&mut s, &["ucs z 30", "ucs z 30"]);
    assert!(ang_eq(s.ucs().rotation(), 60f64.to_radians()));
    s.undo().unwrap();
    assert!(ang_eq(s.ucs().rotation(), 30f64.to_radians()));
    // The 30° the undone step remembered is gone: Previous goes straight to the World UCS.
    assert!(previous(&mut s) && s.ucs().is_world());
    assert!(!previous(&mut s), "nothing before the World UCS");
}

/// A layout's UCS Previous history goes with the layout: it follows a rename, and a layout
/// deleted and made again under the same name starts without one.
#[test]
fn a_layouts_ucs_history_follows_it_and_goes_with_it() {
    let previous = |s: &mut Session| s.execute("ucs", &json!({"option": "previous"})).is_ok();
    let set = |s: &mut Session, name: &str| s.execute("layout.set", &json!({"name": name})).unwrap();
    let mut s = session();
    s.execute("layout", &json!({"option": "new", "name": "A"})).unwrap();
    set(&mut s, "A");
    typed(&mut s, &["ucs z 30"]);
    set(&mut s, "Model");
    s.execute("layout", &json!({"option": "rename", "name": "A", "to": "B"})).unwrap();
    set(&mut s, "B");
    assert!(ang_eq(s.ucs().rotation(), 30f64.to_radians()));
    assert!(previous(&mut s) && s.ucs().is_world(), "the history followed the rename");
    typed(&mut s, &["ucs z 45"]);
    set(&mut s, "Model");
    s.execute("layout", &json!({"option": "delete", "name": "B"})).unwrap();
    s.execute("layout", &json!({"option": "new", "name": "B"})).unwrap();
    set(&mut s, "B");
    assert!(s.ucs().is_world() && !previous(&mut s), "a new layout B has none of the old B's history");
    // Model space's history is its own throughout.
    set(&mut s, "Model");
    assert!(!previous(&mut s));
}

#[test]
fn paper_space_has_its_own_ucs() {
    let mut s = session();
    turned(&mut s);
    let layout = s.doc().unwrap().layouts[0].name.clone();
    s.execute("layout.set", &json!({"name": layout})).unwrap();
    assert!(s.ucs().is_world(), "a layout starts in the World UCS");
    typed(&mut s, &["ucs z 30"]);
    assert!(ang_eq(s.ucs().rotation(), 30f64.to_radians()));
    assert!(s.doc().unwrap().layouts[0].ucs.is_some());
    // UCS Previous stays in its space.
    typed(&mut s, &["ucs p"]);
    assert!(s.ucs().is_world());
    typed(&mut s, &["ucs p"]);
    assert!(logged(&s, "There is no previous UCS."), "model space's history isn't paper space's");
    s.execute("layout.set", &json!({"name": "Model"})).unwrap();
    assert!(near3(s.ucs().x, Vec3::new(0.0, 1.0, 0.0)), "model space kept its own");
    let (m, _) = cadcraft_engine::ucs::current(s.doc().unwrap(), &Space::Model);
    assert!(near3(m.origin, Vec3::new(100.0, 50.0, 0.0)));
}

// ---------------- typed input in the UCS ----------------

#[test]
fn typed_points_are_read_in_the_ucs() {
    let mut s = session();
    turned(&mut s);
    // Absolute: from the UCS origin along its axes.
    typed(&mut s, &["line 10,0 20,0 "]);
    let (a, b) = last_line(&s);
    assert!(near(a, 100.0, 60.0) && near(b, 100.0, 70.0), "{a:?} {b:?}");
    // Relative Cartesian along the UCS axes; polar angles from the UCS X axis.
    typed(&mut s, &["line 0,0 @5,0 @0,5 @10<90 "]);
    let ls = lines(&s);
    let n = ls.len();
    assert!(near(ls[n - 3].1, 100.0, 55.0) && near(ls[n - 2].1, 95.0, 55.0) && near(ls[n - 1].1, 85.0, 55.0), "{ls:?}");
    // Absolute polar: from the UCS origin.
    typed(&mut s, &["line 0,0 10<0 "]);
    assert!(near(last_line(&s).1, 100.0, 60.0), "{:?}", last_line(&s));
    // `#` is absolute in the UCS; `*` and `@*` are world coordinates.
    typed(&mut s, &["line #1,2 *1,2 @*3,0 @*5<0 "]);
    let ls = lines(&s);
    let n = ls.len();
    assert!(near(ls[n - 3].0, 98.0, 51.0) && near(ls[n - 3].1, 1.0, 2.0) && near(ls[n - 2].1, 4.0, 2.0) && near(ls[n - 1].1, 9.0, 2.0), "{ls:?}");
    // A 3D point keeps working (Z is ignored as before).
    typed(&mut s, &["line 1,1,5 2,1,5 "]);
    let (a, _) = last_line(&s);
    assert!(near(a, 99.0, 51.0), "{a:?}");
}

#[test]
fn the_world_ucs_reads_points_as_before() {
    let script = ["line 1,2 @3,4 @5<30 #7,8 *9,10 @*1,1 "];
    let mut a = session();
    typed(&mut a, &script);
    let mut b = session();
    typed(&mut b, &["ucs 100,50 ", "ucs z 33", "ucs w"]);
    typed(&mut b, &script);
    let (la, lb) = (lines(&a), lines(&b));
    assert_eq!(la.len(), lb.len());
    for (x, y) in la.iter().zip(&lb) {
        assert!(x.0.dist(y.0) < 1e-12 && x.1.dist(y.1) < 1e-12, "{x:?} vs {y:?}");
    }
}

#[test]
fn directions_are_measured_from_the_ucs_x_axis() {
    let mut s = session();
    typed(&mut s, &["ucs z 30"]);
    // TEXT's rotation 0 lies along the UCS X axis.
    typed(&mut s, &["text 0,0 2 0 Hello", ""]);
    let rot = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.rotation) } else { None }).unwrap();
    assert!(ang_eq(rot, 30f64.to_radians()), "{rot}");
    // ANGBASE is measured from the UCS X axis too.
    cadcraft_engine::sysvars::set(&mut s, "ANGBASE", &json!(90)).unwrap();
    typed(&mut s, &["line 0,0 @10<0 "]);
    let (a, b) = last_line(&s);
    assert!(ang_eq((b - a).angle(), 120f64.to_radians()), "{:?}", (b - a).angle().to_degrees());
    // `<<` overrides ANGBASE but stays in the UCS.
    typed(&mut s, &["line 0,0 @10<<0 "]);
    let (a, b) = last_line(&s);
    assert!(ang_eq((b - a).angle(), 30f64.to_radians()));
    cadcraft_engine::sysvars::set(&mut s, "ANGBASE", &json!(0)).unwrap();
    // A rotation by an angle is a size, the same in any UCS.
    let h = s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    typed(&mut s, &["rotate l  0,0 90"]);
    let l = s.doc().unwrap().model.iter().find(|e| e.handle.hex() == h).map(|e| e.kind.clone()).unwrap();
    let EntityKind::Line(l) = l else { panic!() };
    assert!(near(l.b.xy(), 0.0, 10.0), "{:?}", l.b);
    // JSON parameters stay in world coordinates.
    s.execute("line", &json!({"points": [[0, 0], [3, 0]]})).unwrap();
    assert!(near(last_line(&s).1, 3.0, 0.0));
}

#[test]
fn coordinates_are_shown_in_the_ucs() {
    let mut s = session();
    turned(&mut s);
    // ID: the message in the UCS; the JSON result keeps world values beside it.
    let r = s.execute("id", &json!({"at": [100, 60]})).unwrap();
    assert!(r["message"].as_str().unwrap().starts_with("X = 10.0000     Y = 0.0000"), "{r}");
    assert_eq!((r["x"].as_f64(), r["y"].as_f64()), (Some(100.0), Some(60.0)));
    assert!((r["ucs"][0].as_f64().unwrap() - 10.0).abs() < 1e-9);
    typed(&mut s, &["id 3,4"]);
    assert!(logged(&s, "X = 3.0000     Y = 4.0000"), "{:?}", s.log.last());
    // DIST: the deltas and the angle in the UCS.
    let r = s.execute("dist", &json!({"p1": [100, 50], "p2": [100, 60]})).unwrap();
    let m = r["message"].as_str().unwrap();
    assert!(m.contains("Angle in XY Plane = 0") && m.contains("Delta X = 10.0000") && m.contains("Delta Y = 0.0000"), "{m}");
    // The helpers the status bar and dynamic input use.
    assert!(near(s.ucs_point(Vec2::new(100.0, 60.0)), 10.0, 0.0));
    assert!(ang_eq(s.ucs_angle(std::f64::consts::FRAC_PI_2), 0.0));
}

// ---------------- variables, UCSICON, tilted UCSs ----------------

#[test]
fn ucs_variables_are_read_only() {
    let mut s = session();
    for v in ["UCSNAME", "UCSORG", "UCSXDIR", "UCSYDIR", "WORLDUCS"] {
        assert!(cadcraft_engine::sysvars::set(&mut s, v, &json!(1)).is_err(), "{v}");
    }
    typed(&mut s, &["ucs 5,6 ", "ucs na s A"]);
    assert_eq!(var(&s, "UCSNAME"), json!("A"));
    assert!(near3(vec3(&var(&s, "UCSORG")), Vec3::new(5.0, 6.0, 0.0)));
}

#[test]
fn ucsicon_options() {
    let mut s = session();
    assert_eq!(s.ucsicon_bits(), 3, "shown, at the origin");
    typed(&mut s, &["ucsicon off"]);
    assert_eq!(s.ucsicon_bits(), 2);
    typed(&mut s, &["ucsicon on", "ucsicon n"]);
    assert_eq!(s.ucsicon_bits(), 1);
    typed(&mut s, &["ucsicon or"]);
    assert_eq!(s.ucsicon_bits(), 3);
    s.cmdline("ucsicon s").unwrap();
    assert!(logged(&s, "not available yet"));
    s.cancel();
    let r = s.execute("ucsicon", &json!({"on": false, "origin": false})).unwrap();
    assert_eq!(r, json!({"on": false, "origin": false}));
    assert_eq!(var(&s, "UCSICON"), json!(0));
    assert!(s.execute("ucsicon", &json!({"on": "yes"})).is_err());
    assert!(cadcraft_engine::sysvars::set(&mut s, "UCSICON", &json!(9)).is_err());
}

/// UCSICON is its own undo step: undoing it brings the icon back, and an undo never undoes the
/// edit before it while leaving the icon setting behind (or the other way round).
#[test]
fn ucsicon_is_undone_on_its_own() {
    let mut s = session();
    let lines = |s: &Session| s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    typed(&mut s, &["ucsicon off"]);
    assert_eq!(s.ucsicon_bits() & 1, 0);
    s.undo().unwrap();
    assert_eq!(s.ucsicon_bits() & 1, 1, "undo takes back UCSICON OFF");
    assert_eq!(lines(&s), 1, "and leaves the line drawn before it");
    s.undo().unwrap();
    assert_eq!(lines(&s), 0);
    assert_eq!(s.ucsicon_bits() & 1, 1);
    s.redo().unwrap();
    s.redo().unwrap();
    assert_eq!((lines(&s), s.ucsicon_bits() & 1), (1, 0), "redo brings both back");
}

#[test]
fn a_tilted_ucs_from_a_file_is_kept_and_input_stays_in_the_world_ucs() {
    let mut s = session();
    {
        let h = &mut s.doc_mut().unwrap().header;
        h.set("UCSORG", HVal::Point(Vec3::new(10.0, 0.0, 0.0)));
        h.set("UCSXDIR", HVal::Point(Vec3::new(1.0, 0.0, 0.0)));
        h.set("UCSYDIR", HVal::Point(Vec3::new(0.0, 0.0, 1.0)));
    }
    assert!(!s.ucs().is_planar());
    assert_eq!(s.input_ucs(), UcsFrame::WORLD);
    typed(&mut s, &["line 1,1 2,2 "]);
    assert!(near(last_line(&s).0, 1.0, 1.0), "read in the World UCS");
    assert!(logged(&s, "tilted out of the XY plane"));
    let n = s.log.iter().filter(|l| l.contains("tilted out of the XY plane")).count();
    typed(&mut s, &["line 3,3 4,4 "]);
    assert_eq!(s.log.iter().filter(|l| l.contains("tilted out of the XY plane")).count(), n, "told once");
    // The UCS itself is untouched until the user sets another.
    assert!(near3(vec3(&var(&s, "UCSYDIR")), Vec3::new(0.0, 0.0, 1.0)));
    // A tilted named UCS isn't restored.
    s.doc_mut().unwrap().ucss.push(cadcraft_engine::doc::Ucs {
        name: "FRONT".into(),
        origin: Vec3::ZERO,
        x_axis: Vec3::new(1.0, 0.0, 0.0),
        y_axis: Vec3::new(0.0, 0.0, 1.0),
    });
    s.execute("ucs", &json!({"option": "world"})).unwrap();
    assert!(s.execute("ucs", &json!({"option": "named", "action": "restore", "name": "FRONT"})).unwrap_err().to_string().contains("tilted"));
    assert!(s.ucs().is_world());
}

#[test]
fn hostile_ucs_input_never_panics() {
    let mut s = session();
    let bad: Vec<Value> = vec![
        json!({"option": "origin", "origin": [f64::MAX, -f64::MAX, f64::MAX]}),
        json!({"option": "origin", "origin": ["a", 1]}),
        json!({"option": "origin", "origin": [1]}),
        json!({"option": "origin", "origin": [0, 0], "xPoint": [1e308, 1e308], "xyPoint": [-1e308, 1e308]}),
        json!({"option": "origin", "origin": [0, 0], "xyPoint": [0, 1]}),
        json!({"option": "z", "angle": 1e308}),
        json!({"option": "z", "angle": "ninety"}),
        json!({"option": "object"}),
        json!({"option": "object", "handle": "ZZZZ"}),
        json!({"option": "object", "handle": "FFFFFF"}),
        json!({"option": "named", "action": "explode", "name": "A"}),
        json!({"option": "named", "action": "delete", "name": "*,?,[,]"}),
        json!({"option": "warp"}),
        json!({"option": 7}),
        json!(null),
        json!([1, 2, 3]),
    ];
    for p in &bad {
        let _ = s.execute("ucs", p);
        let u = s.ucs();
        assert!(u.origin.is_finite() && u.x.is_finite() && u.y.is_finite(), "{p}: {u:?}");
    }
    for t in ["ucs 1e400,0", "ucs nan,nan", "ucs 0,0 0,0", "ucs z 1e999", "ucs na s \u{0}", "ucs na r ", "ucs ob", "ucs na ? [", "ucsicon blah"] {
        let _ = s.cmdline(t);
        s.cancel();
    }
    // Broken header values read as the World UCS.
    for v in [HVal::Point(Vec3::new(f64::NAN, 0.0, 0.0)), HVal::Point(Vec3::ZERO), HVal::Str("x".into()), HVal::Int(3)] {
        s.doc_mut().unwrap().header.set("UCSXDIR", v);
        let u = s.ucs();
        assert!(u.origin.is_finite() && u.x.is_finite());
        typed(&mut s, &["line 0,0 1,0 "]);
    }
}
