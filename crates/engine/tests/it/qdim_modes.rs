//! QDIM modes: Continuous, Staggered, Baseline, Ordinate, Radius and Diameter, the datum point,
//! Edit (add and remove dimension points) and the prompt sequence.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{DimKind, Dimension, EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use serde_json::{Value, json};

fn dims(s: &Session, r: &Value) -> Vec<Dimension> {
    let d = s.doc().unwrap();
    r["handles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| match &d.entity(Handle::parse_hex(h.as_str().unwrap()).unwrap()).unwrap().kind {
            EntityKind::Dimension(dm) => dm.clone(),
            k => panic!("not a dimension: {k:?}"),
        })
        .collect()
}

/// Lines from x = 0 to 10, 10 to 30 and 30 to 60 on y = 0: dimension points 0, 10, 30, 60.
fn session() -> (Session, Vec<String>) {
    let mut s = Session::new();
    for (a, b) in [(0, 10), (10, 30), (30, 60)] {
        s.execute("line", &json!({ "points": [[a, 0], [b, 0]] })).unwrap();
    }
    let hs = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    (s, hs)
}

fn span(d: &Dimension) -> (f64, f64) {
    let (a, b) = (d.p13.x, d.p14.x);
    (a.min(b), a.max(b))
}

#[test]
fn linear_modes() {
    let (mut s, hs) = session();
    let r = s.execute("qdim", &json!({ "handles": hs, "at": [0, 5] })).unwrap();
    let c = dims(&s, &r);
    assert_eq!(c.iter().map(span).collect::<Vec<_>>(), vec![(0.0, 10.0), (10.0, 30.0), (30.0, 60.0)]);
    assert!(c.iter().all(|d| (d.defpt.y - 5.0).abs() < 1e-9));

    // Baseline: from the first point, each one DIMDLI further out.
    let r = s.execute("qdim", &json!({ "handles": hs, "at": [0, 5], "mode": "baseline" })).unwrap();
    let b = dims(&s, &r);
    assert_eq!(b.iter().map(span).collect::<Vec<_>>(), vec![(0.0, 10.0), (0.0, 30.0), (0.0, 60.0)]);
    let ys: Vec<f64> = b.iter().map(|d| d.defpt.y).collect();
    assert!((ys[1] - ys[0] - 0.38).abs() < 1e-9 && (ys[2] - ys[1] - 0.38).abs() < 1e-9, "{ys:?}");
    // A datum point picks the base.
    let r = s.execute("qdim", &json!({ "handles": hs, "at": [0, -5], "mode": "baseline", "datum": [61, 0] })).unwrap();
    let b = dims(&s, &r);
    assert_eq!(b.iter().map(span).collect::<Vec<_>>(), vec![(30.0, 60.0), (10.0, 60.0), (0.0, 60.0)]);
    assert!(b[1].defpt.y < b[0].defpt.y, "below the geometry: stacked downwards");

    // Staggered: symmetric pairs, the innermost at the line position.
    let r = s.execute("qdim", &json!({ "handles": hs, "at": [0, 5], "mode": "staggered" })).unwrap();
    let st = dims(&s, &r);
    assert_eq!(st.iter().map(span).collect::<Vec<_>>(), vec![(10.0, 30.0), (0.0, 60.0)]);
    assert!((st[0].defpt.y - 5.0).abs() < 1e-9 && st[1].defpt.y > st[0].defpt.y);

    // Beside the geometry: vertical dimensions (here of points on one line: refused).
    let e = s.execute("qdim", &json!({ "handles": hs, "at": [-5, 0] })).unwrap_err();
    assert!(e.to_string().contains("two distinct points"), "{e}");
    assert!(s.execute("qdim", &json!({ "handles": hs, "at": [0, 5], "mode": "zigzag" })).is_err());
}

#[test]
fn ordinate_radius_diameter_and_edit() {
    let (mut s, hs) = session();
    // Ordinate from the origin (or the datum): leaders up to the line position.
    let r = s.execute("qdim", &json!({ "handles": hs, "at": [0, 5], "mode": "ordinate" })).unwrap();
    let o = dims(&s, &r);
    assert_eq!(o.len(), 4);
    assert!(o.iter().all(|d| matches!(d.kind, DimKind::Ordinate { x_type: true }) && (d.p14.y - 5.0).abs() < 1e-9 && d.defpt.xy() == Vec2::ZERO));
    let r = s.execute("qdim", &json!({ "handles": hs, "at": [0, 5], "mode": "ordinate", "datum": [10, 0] })).unwrap();
    let v: Vec<String> = dims(&s, &r).iter().map(|d| cadcraft_engine::render::dimension_in(s.doc().unwrap(), d).value).collect();
    assert_eq!(v, vec!["10.0000", "0.0000", "20.0000", "50.0000"]);

    // Edit: remove a point, add one.
    let r = s.execute("qdim", &json!({ "handles": hs, "at": [0, 5], "remove": [[30, 0]], "add": [[45, 0]] })).unwrap();
    assert_eq!(dims(&s, &r).iter().map(span).collect::<Vec<_>>(), vec![(0.0, 10.0), (10.0, 45.0), (45.0, 60.0)]);

    // Radius and diameter of every circle and arc.
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 5 })).unwrap();
    s.execute("circle", &json!({ "center": [20, 0], "radius": 3 })).unwrap();
    s.execute("arc", &json!({ "center": [40, 0], "radius": 4, "start": 0, "end": 180 })).unwrap();
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    let r = s.execute("qdim", &json!({ "handles": hs, "at": [20, 20], "mode": "radius" })).unwrap();
    let rd = dims(&s, &r);
    assert_eq!(rd.len(), 3);
    assert!(rd.iter().all(|d| d.kind == DimKind::Radius));
    assert!((rd[1].p15.xy().dist(Vec2::new(20.0, 3.0))) < 1e-9, "towards the line position: {:?}", rd[1].p15);
    let r = s.execute("qdim", &json!({ "handles": hs, "at": [20, 20], "mode": "diameter" })).unwrap();
    assert!(dims(&s, &r).iter().all(|d| d.kind == DimKind::Diameter));
    let line = handles_of_new_line(&mut s);
    assert!(s.execute("qdim", &json!({ "handles": [line], "at": [0, 5], "mode": "radius" })).is_err());
}

fn handles_of_new_line(s: &mut Session) -> String {
    s.execute("line", &json!({ "points": [[0, 50], [5, 50]] })).unwrap();
    s.doc().unwrap().model.last().unwrap().handle.hex()
}

#[test]
fn qdim_prompts() {
    let (mut s, _) = session();
    s.cmdline("qdim").unwrap();
    assert!(s.prompt_text().contains("Select geometry to dimension"), "{}", s.prompt_text());
    // Window-free picks: each line, then Enter.
    s.script("5,0\n20,0\n45,0\n\n").unwrap();
    let p = s.prompt_text();
    assert!(p.contains("[Continuous/Staggered/Baseline/Ordinate/Radius/Diameter/datumPoint/Edit/seTtings] <Continuous>"), "{p}");
    // Baseline from a datum point, with one point removed in Edit.
    let before = s.doc().unwrap().model.iter().count();
    s.script("b\np\n60,0\ne\n10,0\nx\n0,5\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let d = s.doc().unwrap();
    let new: Vec<Dimension> = d
        .model
        .iter()
        .skip(before)
        .filter_map(|e| match &e.kind {
            EntityKind::Dimension(dm) => Some(dm.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(new.iter().map(span).collect::<Vec<_>>(), vec![(30.0, 60.0), (0.0, 60.0)]);
}
