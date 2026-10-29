//! Drawing variables SETVAR checks and the commands follow: PLINEWID (PLINE's width), PLINEGEN
//! (new polylines), SPLINETYPE (PEDIT Spline), the USERI/USERR variables, and the checked
//! FILLMODE, ATTMODE, PDMODE, PDSIZE, PSLTSCALE, LTSCALE, CELTSCALE and TEXTSIZE; they survive a
//! DXF save.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use serde_json::{Value, json};

fn getvar(s: &mut Session, name: &str) -> Value {
    s.execute("getvar", &json!({ "name": name })).unwrap()["value"].clone()
}

fn setvar(s: &mut Session, name: &str, value: Value) -> bool {
    s.execute("setvar", &json!({ "name": name, "value": value })).is_ok()
}

fn last_poly(s: &Session) -> cadcraft_engine::doc::LwPolyline {
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::LwPolyline(p) => p.clone(),
        k => panic!("expected a polyline, got {k:?}"),
    }
}

const NEW: [&str; 21] = [
    "ATTMODE",
    "CELTSCALE",
    "FILLMODE",
    "LTSCALE",
    "PDMODE",
    "PDSIZE",
    "PLINEGEN",
    "PLINEWID",
    "PSLTSCALE",
    "SPLINETYPE",
    "TEXTSIZE",
    "USERI1",
    "USERI2",
    "USERI3",
    "USERI4",
    "USERI5",
    "USERR1",
    "USERR2",
    "USERR3",
    "USERR4",
    "USERR5",
];

#[test]
fn defaults_and_listing() {
    let mut s = Session::new();
    for (name, v) in [
        ("PLINEWID", json!(0.0)),
        ("PLINEGEN", json!(0)),
        ("SPLINETYPE", json!(6)),
        ("USERI3", json!(0)),
        ("USERR5", json!(0.0)),
        ("FILLMODE", json!(1)),
        ("ATTMODE", json!(1)),
        ("PDMODE", json!(0)),
        ("PSLTSCALE", json!(1)),
        ("LTSCALE", json!(1.0)),
    ] {
        assert_eq!(getvar(&mut s, name), v, "{name}");
    }
    let all = cadcraft_engine::sysvars::list(&s);
    for n in NEW {
        assert!(all.get(n).is_some(), "{n} listed by SETVAR ?");
    }
    // Typed `SETVAR ?` lists them too.
    s.cmdline("setvar ?").unwrap();
    let text = s.log.join("\n");
    assert!(text.contains("PLINEWID") && text.contains("USERI1") && text.contains("SPLINETYPE"), "{text}");
}

#[test]
fn plinewid_is_the_width_pline_starts_with_and_its_width_answers_set_it() {
    let mut s = Session::new();
    assert!(setvar(&mut s, "PLINEWID", json!(0.5)));
    // (A script's line end is one Enter; the second ends PLINE.)
    s.script("PLINE 0,0 10,0\n\n").unwrap();
    assert!(s.log.iter().any(|l| l.contains("Current line-width is 0.5000")), "{:?}", s.log);
    assert_eq!(last_poly(&s).const_width, 0.5);
    // A Width answer becomes the next PLINE's width (the ending width: the width of the
    // segments after the next one).
    s.script("PLINE 0,5 W 2 3 10,5\n\n").unwrap();
    assert_eq!(getvar(&mut s, "PLINEWID"), json!(3.0));
    s.script("PLINE 0,10 H 1 1 10,10\n\n").unwrap();
    assert_eq!(getvar(&mut s, "PLINEWID"), json!(2.0), "a half-width of 1 is a width of 2");
    s.script("PLINE 0,20 10,20\n\n").unwrap();
    assert_eq!(last_poly(&s).const_width, 2.0);
    // The JSON form keeps its documented default (no width).
    s.execute("pline", &json!({ "vertices": [[0, 30], [10, 30]] })).unwrap();
    assert_eq!(last_poly(&s).const_width, 0.0);
}

#[test]
fn plinegen_reaches_new_polylines() {
    let mut s = Session::new();
    s.execute("pline", &json!({ "vertices": [[0, 0], [10, 0], [10, 10]] })).unwrap();
    assert!(!last_poly(&s).plinegen);
    s.cmdline("setvar plinegen 1").unwrap();
    s.execute("pline", &json!({ "vertices": [[0, 0], [10, 0], [10, 10]] })).unwrap();
    assert!(last_poly(&s).plinegen);
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [5, 5] })).unwrap();
    assert!(last_poly(&s).plinegen, "RECTANG makes a polyline too");
}

#[test]
fn splinetype_picks_a_quadratic_or_cubic_spline_fit() {
    let fit = |t: i64| {
        let mut s = Session::new();
        assert!(setvar(&mut s, "SPLINETYPE", json!(t)));
        assert!(setvar(&mut s, "SPLINESEGS", json!(4)));
        let out = s.execute("pline", &json!({ "vertices": [[0, 0], [10, 10], [20, 0], [30, 10]] })).unwrap();
        let h = out["handle"].as_str().unwrap().to_string();
        s.execute("pedit", &json!({ "handle": h, "option": "spline" })).unwrap();
        last_poly(&s).vertices.iter().map(|v| v.p).collect::<Vec<_>>()
    };
    let (quad, cubic) = (fit(5), fit(6));
    // Both run from the first to the last frame point, along different curves.
    for c in [&quad, &cubic] {
        assert!(c.first().unwrap().near(Vec2::new(0.0, 0.0), 1e-9), "{c:?}");
        assert!(c.last().unwrap().near(Vec2::new(30.0, 10.0), 1e-9), "{c:?}");
    }
    assert_eq!(quad.len(), cubic.len());
    assert!(quad.iter().zip(&cubic).any(|(a, b)| !a.near(*b, 1e-6)), "quadratic and cubic fits differ");
    // A quadratic B-spline passes through the middle of the frame's middle segment (15, 5).
    assert!(quad.iter().any(|p| p.near(Vec2::new(15.0, 5.0), 1e-9)), "{quad:?}");
    // A closed frame: each periodic quadratic piece starts at the midpoint of a frame segment.
    let mut s = Session::new();
    assert!(setvar(&mut s, "SPLINETYPE", json!(5)));
    let out = s.execute("pline", &json!({ "vertices": [[0, 0], [10, 0], [10, 10], [0, 10]], "closed": true })).unwrap();
    let h = out["handle"].as_str().unwrap().to_string();
    s.execute("pedit", &json!({ "handle": h, "option": "spline" })).unwrap();
    let p = last_poly(&s);
    assert!(p.closed);
    assert!(p.vertices.iter().any(|v| v.p.near(Vec2::new(5.0, 0.0), 1e-9)), "{:?}", p.vertices);
}

#[test]
fn values_are_checked() {
    let mut s = Session::new();
    for (name, v) in [
        ("PLINEWID", json!(-1)),
        ("PLINEWID", json!("wide")),
        ("PLINEGEN", json!(2)),
        ("SPLINETYPE", json!(4)),
        ("SPLINETYPE", json!(7)),
        ("USERI1", json!(40000)),
        ("USERI2", json!(1.5)),
        ("USERR1", json!("abc")),
        ("FILLMODE", json!(2)),
        ("ATTMODE", json!(3)),
        ("PDMODE", json!(5)),
        ("PDMODE", json!(31)),
        ("PDMODE", json!(128)),
        ("PSLTSCALE", json!(-1)),
        ("LTSCALE", json!(0)),
        ("CELTSCALE", json!(-2)),
        ("TEXTSIZE", json!(0)),
    ] {
        assert!(!setvar(&mut s, name, v.clone()), "{name} = {v} refused");
    }
    for (name, v, back) in [
        ("PLINEWID", json!("0.25"), json!(0.25)),
        ("PLINEGEN", json!(true), json!(1)),
        ("SPLINETYPE", json!("5"), json!(5)),
        ("USERI1", json!(-32768), json!(-32768)),
        ("USERI5", json!(32767), json!(32767)),
        ("USERR2", json!(-1.5e10), json!(-1.5e10)),
        ("FILLMODE", json!(0), json!(0)),
        ("ATTMODE", json!(2), json!(2)),
        ("PDMODE", json!(99), json!(99)),
        ("PDSIZE", json!(-5), json!(-5.0)),
        ("PSLTSCALE", json!(0), json!(0)),
        ("LTSCALE", json!(0.5), json!(0.5)),
        ("CELTSCALE", json!(2), json!(2.0)),
        ("TEXTSIZE", json!(3.5), json!(3.5)),
    ] {
        assert!(setvar(&mut s, name, v.clone()), "{name} = {v} accepted");
        assert_eq!(getvar(&mut s, name), back, "{name}");
    }
    // Typed at the command line, through SETVAR or the bare name.
    s.cmdline("setvar useri4 12").unwrap();
    assert_eq!(getvar(&mut s, "USERI4"), json!(12));
    s.cmdline("userr4 2.5").unwrap();
    assert_eq!(getvar(&mut s, "USERR4"), json!(2.5));
}

#[test]
fn hostile_values_never_panic() {
    let mut s = Session::new();
    let hostile = [
        json!(f64::MAX),
        json!(f64::MIN),
        json!(i64::MAX),
        json!(i64::MIN),
        json!(u64::MAX),
        json!("NaN"),
        json!("inf"),
        json!("-inf"),
        json!("1e400"),
        json!(""),
        json!(null),
        json!([1, 2, 3]),
        json!({ "x": 1 }),
        json!("9".repeat(400)),
    ];
    for name in NEW {
        for v in &hostile {
            let _ = setvar(&mut s, name, v.clone());
            let g = getvar(&mut s, name);
            if let Some(f) = g.as_f64() {
                assert!(f.is_finite(), "{name} = {g} after {v}");
            }
        }
    }
    // The commands that read them still work.
    s.script("PLINE 0,0 10,0\n\n").unwrap();
    let out = s.execute("pline", &json!({ "vertices": [[0, 0], [10, 10], [20, 0]] })).unwrap();
    s.execute("pedit", &json!({ "handle": out["handle"], "option": "spline" })).unwrap();
    s.execute("point", &json!({ "at": [1, 1] })).unwrap();
}

#[test]
fn header_variables_survive_a_dxf_save() {
    let values = [
        ("PLINEWID", json!(0.75)),
        ("PLINEGEN", json!(1)),
        ("SPLINETYPE", json!(5)),
        ("SPLINESEGS", json!(12)),
        ("CHAMFERC", json!(4.5)),
        ("USERI1", json!(7)),
        ("USERI5", json!(-3)),
        ("USERR1", json!(2.25)),
        ("USERR5", json!(-0.5)),
        ("FILLMODE", json!(0)),
        ("PDMODE", json!(35)),
    ];
    let mut s = Session::new();
    for (name, v) in &values {
        assert!(setvar(&mut s, name, v.clone()), "{name}");
    }
    let text = cadcraft_io::write_dxf(s.doc().unwrap());
    for var in ["$PLINEWID", "$PLINEGEN", "$SPLINETYPE", "$SPLINESEGS", "$CHAMFERC", "$USERI1", "$USERI5", "$USERR1", "$USERR5"] {
        assert!(text.lines().any(|l| l.trim() == var), "{var} written");
    }
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let mut t = Session::new();
    t.open_drawing(back, "back", None);
    for (name, v) in &values {
        assert_eq!(&getvar(&mut t, name), v, "{name} after a DXF round trip");
    }
    // And they still work: the next PLINE starts at the width read back.
    t.script("PLINE 0,0 10,0\n\n").unwrap();
    let p = last_poly(&t);
    assert_eq!(p.const_width, 0.75);
    assert!(p.plinegen);
}
