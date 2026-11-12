//! Helices (#510): HELIX makes a helix object drawn as a spline, which edits, grips, snaps and
//! explodes like other curves.

use cadcraft_doc::{EntityKind, Handle, Helix, Space};
use cadcraft_engine::Session;
use cadcraft_engine::snap::{self, mode};
use cadcraft_geom::{Spline, Vec2};
use serde_json::{Value, json};

fn h(v: &Value) -> Handle {
    Handle::parse_hex(v["handle"].as_str().unwrap()).unwrap()
}

fn helix(s: &Session, hd: Handle) -> Helix {
    match &s.doc().unwrap().entity(hd).unwrap().kind {
        EntityKind::Helix(x) => x.clone(),
        k => panic!("not a helix: {k:?}"),
    }
}

#[test]
fn helix_is_an_object_on_its_points() {
    let mut s = Session::new();
    let a = h(&s.execute("helix", &json!({"center": [1, 2], "baseRadius": 3, "topRadius": 1, "height": 8, "turns": 4})).unwrap());
    let x = helix(&s, a);
    assert_eq!((x.base_radius, x.top_radius, x.height, x.turns, x.ccw), (3.0, 1.0, 8.0, 4.0, true));
    assert_eq!(s.doc().unwrap().entity(a).unwrap().kind.type_name(), "Helix");
    assert!((x.turn_height() - 2.0).abs() < 1e-12);
    // The spline seen from above runs through the helix's points, and the heights of its
    // control points make the 3D spline run through them too.
    let (sp, z) = x.spline3();
    let height = Spline { control: z.iter().map(|z| Vec2::new(*z, 0.0)).collect(), ..sp.clone() };
    let (lo, hi) = sp.domain();
    let fit = x.fit_points();
    for (i, p) in fit.iter().enumerate() {
        let t = lo + (hi - lo) * i as f64 / (fit.len() - 1) as f64;
        assert!(sp.eval(t).near(p.xy(), 1e-6), "{i}: {:?} vs {p:?}", sp.eval(t));
        assert!((height.eval(t).x - p.z).abs() < 1e-6, "{i}: z {} vs {}", height.eval(t).x, p.z);
    }
    // Between the points the spline stays on the helix's radius closely.
    for i in 0..200 {
        let t = i as f64 / 199.0;
        let q = sp.eval(lo + (hi - lo) * t);
        let r = x.base_radius + (x.top_radius - x.base_radius) * t;
        assert!((q.dist(Vec2::new(1.0, 2.0)) - r).abs() < 0.02 * r, "{t}: {q:?}");
    }
    assert!(s.execute("helix", &json!({"center": [0, 0], "baseRadius": -1})).is_err());
    assert!(s.execute("helix", &json!({"center": [0, 0], "baseRadius": 1, "height": 1e300})).is_err());
}

#[test]
fn helix_edits_grips_snaps_and_explode() {
    let mut s = Session::new();
    let a = h(&s.execute("helix", &json!({"center": [0, 0], "baseRadius": 2, "topRadius": 2, "height": 5, "turns": 3})).unwrap());
    s.execute("move", &json!({"handles": [a.hex()], "delta": [10, 0]})).unwrap();
    s.execute("scale", &json!({"handles": [a.hex()], "base": [10, 0], "factor": 2})).unwrap();
    let x = helix(&s, a);
    assert_eq!((x.center.xy(), x.base_radius, x.height), (Vec2::new(10.0, 0.0), 4.0, 10.0));
    s.execute("rotate", &json!({"handles": [a.hex()], "base": [10, 0], "angle": 90})).unwrap();
    assert!(helix(&s, a).start_point().xy().near(Vec2::new(10.0, 4.0), 1e-9));
    s.execute("mirror", &json!({"handles": [a.hex()], "p1": [0, 0], "p2": [0, 1], "erase": true})).unwrap();
    let a = s.doc().unwrap().model.iter().last().unwrap().handle;
    let x = helix(&s, a);
    assert!(!x.ccw, "mirrored twist");
    assert!(x.center.xy().near(Vec2::new(-10.0, 0.0), 1e-9) && x.start_point().xy().near(Vec2::new(-10.0, 4.0), 1e-9));

    // Grips: centre, base radius, top radius.
    let k = s.doc().unwrap().entity(a).unwrap().kind.clone();
    assert_eq!(k.grips().len(), 3);
    let EntityKind::Helix(y) = cadcraft_engine::grips::stretch_grip(&k, 2, Vec2::new(-10.0, 1.0)).unwrap() else { panic!() };
    assert!((y.top_radius - 1.0).abs() < 1e-9 && (y.base_radius - 4.0).abs() < 1e-9);
    let EntityKind::Helix(y) = cadcraft_engine::grips::stretch_grip(&k, 0, Vec2::new(0.0, 0.0)).unwrap() else { panic!() };
    assert_eq!(y.center.xy(), Vec2::ZERO);

    // The start point snaps as an endpoint; picking the curve picks the helix.
    let d = s.doc().unwrap();
    let start = x.start_point().xy();
    let hit = snap::osnap(d, &Space::Model, start + Vec2::new(0.05, 0.02), 0.3, mode::END, None, false).unwrap();
    assert!(hit.point.near(start, 1e-9));
    assert_eq!(cadcraft_engine::select::pick(d, &Space::Model, start, 0.1), Some(a));
    let ext = d.extents(&Space::Model);
    assert!(ext.min.x > -14.2 && ext.max.x < -5.8 && ext.min.x < -13.9, "{ext:?}");

    // EXPLODE gives the spline; FLATTEN too.
    let b = h(&s.execute("helix", &json!({"center": [0, 0], "baseRadius": 1})).unwrap());
    s.execute("explode", &json!({"handles": [a.hex()]})).unwrap();
    s.execute("flatten", &json!({"handles": [b.hex()]})).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.model.iter().filter(|e| matches!(e.kind, EntityKind::Spline(_))).count(), 2);
    assert!(!d.model.iter().any(|e| matches!(e.kind, EntityKind::Helix(_))));
}

#[test]
fn helix_command_line_preview_and_hostile_values() {
    let mut s = Session::new();
    s.cmdline("helix 0,0 d 4 1 t 2.5 h 2").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let x = match &s.doc().unwrap().model.iter().last().unwrap().kind {
        EntityKind::Helix(x) => x.clone(),
        k => panic!("{k:?}"),
    };
    assert_eq!((x.base_radius, x.top_radius, x.turns), (2.0, 1.0, 2.5));
    assert!((x.height - 5.0).abs() < 1e-9, "turn height 2 × 2.5 turns: {}", x.height);
    for p in [
        json!({"center": [0, 0], "baseRadius": 1e308, "topRadius": 1e308, "turns": 500, "height": -1e308}),
        json!({"center": [1e308, -1e308], "baseRadius": 1e-300, "turns": 1e-300}),
        json!({"center": [0, 0], "baseRadius": 1, "turns": 500, "height": 1e11}),
    ] {
        let _ = s.execute("helix", &p);
    }
    let d = s.doc().unwrap();
    let _ = cadcraft_render::build(d, &Space::Model, &cadcraft_render::Options::default());
}
