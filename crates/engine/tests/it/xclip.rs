//! XCLIP and XCLIPFRAME: clipping block and xref references (gap F2).

use cadcraft_engine::doc::{ClipBoundary, Common, EntityKind, Handle, Insert, Space};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_engine::render::{self, Options};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn h(v: &Value) -> String {
    v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap().to_string()
}

fn insert(s: &mut Session, block: &str, at: (f64, f64), scale: f64, rot_deg: f64) -> String {
    let kind = EntityKind::Insert(Insert {
        block: block.into(),
        insert: Vec3::new(at.0, at.1, 0.0),
        scale: Vec3::new(scale, scale, scale),
        rotation: rot_deg.to_radians(),
        ..Insert::default()
    });
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), kind).unwrap().hex()
}

/// Block B: a horizontal line (0,0)-(100,0) and a vertical one (50,-50)-(50,50); two references.
fn setup() -> (Session, String, String) {
    let mut s = Session::new();
    let a = h(&s.execute("line", &json!({ "points": [[0, 0], [100, 0]] })).unwrap());
    let b = h(&s.execute("line", &json!({ "points": [[50, -50], [50, 50]] })).unwrap());
    s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [a, b], "keep": "delete" })).unwrap();
    let r1 = insert(&mut s, "B", (0.0, 0.0), 1.0, 0.0);
    let r2 = insert(&mut s, "B", (1000.0, 0.0), 2.0, 90.0);
    (s, r1, r2)
}

fn clip(s: &Session, hex: &str) -> Option<ClipBoundary> {
    match &s.doc().unwrap().entity(Handle::parse_hex(hex).unwrap()).unwrap().kind {
        EntityKind::Insert(i) => i.clip.clone(),
        _ => None,
    }
}

/// The x extent of what the drawing shows around reference 1 (x < 500).
fn shown_x(s: &Session) -> (f64, f64) {
    let list = render::build(s.doc().unwrap(), &Space::Model, &Options::default());
    let xs: Vec<f64> = list.verts.iter().filter(|v| v.x < 500.0).map(|v| v.x).collect();
    (xs.iter().copied().fold(f64::MAX, f64::min), xs.iter().copied().fold(f64::MIN, f64::max))
}

#[test]
fn rectangular_clip_cuts_only_that_reference() {
    let (mut s, r1, r2) = setup();
    assert_eq!(shown_x(&s), (0.0, 100.0));
    s.execute("setvar", &json!({ "name": "XCLIPFRAME", "value": 0 })).unwrap();
    let out = s.execute("xclip", &json!({ "handle": r1, "rectangle": [[20, -10], [70, 10]] })).unwrap();
    assert_eq!(out["option"], "new");
    let c = clip(&s, &r1).unwrap();
    assert!(c.enabled && !c.inverted && c.points.len() == 2);
    let (lo, hi) = shown_x(&s);
    assert!((lo - 20.0).abs() < 1e-9 && (hi - 70.0).abs() < 1e-9, "{lo} {hi}");
    assert!(clip(&s, &r2).is_none(), "the other reference stays whole");
    // The boundary follows its reference.
    s.execute("move", &json!({ "handles": [r1], "delta": [5, 0] })).unwrap();
    let (lo, hi) = shown_x(&s);
    assert!((lo - 25.0).abs() < 1e-9 && (hi - 75.0).abs() < 1e-9, "{lo} {hi}");
    // Its extents are the clipped part (ZOOM Extents).
    let d = s.doc().unwrap();
    let b = cadcraft_engine::doc::entity_bounds(d, d.entity(Handle::parse_hex(&r1).unwrap()).unwrap(), 0);
    assert!((b.min.x - 25.0).abs() < 1e-9 && (b.max.x - 75.0).abs() < 1e-9 && b.max.y <= 10.0 + 1e-9, "{b:?}");
    // Query reports it in drawing coordinates.
    let q = s.execute("xclip", &json!({ "handle": r1, "option": "query" })).unwrap();
    assert_eq!(q["clips"][0]["boundary"][0], json!([25.0, -10.0]));
    // OFF shows everything, ON clips again, Delete removes it; undo brings it back.
    s.execute("xclip", &json!({ "handle": r1, "option": "off" })).unwrap();
    assert_eq!(shown_x(&s), (5.0, 105.0));
    s.execute("xclip", &json!({ "handle": r1, "option": "on" })).unwrap();
    assert!((shown_x(&s).0 - 25.0).abs() < 1e-9);
    s.execute("xclip", &json!({ "handle": r1, "option": "delete" })).unwrap();
    assert!(clip(&s, &r1).is_none());
    s.execute("undo", &json!({})).unwrap();
    assert!(clip(&s, &r1).is_some());
}

#[test]
fn rotated_and_scaled_reference_gets_a_polygon_in_block_coordinates() {
    let (mut s, _, r2) = setup();
    // r2: scale 2, rotated 90 degrees at (1000, 0): the block's x axis points up.
    s.execute("xclip", &json!({ "handle": r2, "rectangle": [[990, 10], [1010, 50]] })).unwrap();
    let c = clip(&s, &r2).unwrap();
    // A 90 degree rotation keeps the rectangle a rectangle: (5..25, -5..5) in the block.
    let poly = c.polygon();
    let xs: Vec<f64> = poly.iter().map(|p| p.x).collect();
    assert!(xs.iter().any(|x| (x - 5.0).abs() < 1e-9) && xs.iter().any(|x| (x - 25.0).abs() < 1e-9), "{poly:?}");
    // 30 degrees: kept as the four corners.
    let r3 = insert(&mut s, "B", (2000.0, 0.0), 1.0, 30.0);
    s.execute("xclip", &json!({ "handle": r3, "rectangle": [[1990, -10], [2010, 10]] })).unwrap();
    assert_eq!(clip(&s, &r3).unwrap().points.len(), 4);
}

#[test]
fn inverted_polygonal_and_polyline_boundaries() {
    let (mut s, r1, _) = setup();
    s.execute("setvar", &json!({ "name": "XCLIPFRAME", "value": 0 })).unwrap();
    s.execute("xclip", &json!({ "handle": r1, "points": [[40, -10], [60, -10], [60, 10], [40, 10]], "invert": true })).unwrap();
    assert!(clip(&s, &r1).unwrap().inverted);
    let list = render::build(s.doc().unwrap(), &Space::Model, &Options::default());
    // The middle of the horizontal line is hidden, both ends show.
    assert!(!list.verts.iter().any(|v| v.x > 40.5 && v.x < 59.5 && v.y.abs() < 1e-9));
    assert!(list.verts.iter().any(|v| v.x == 0.0) && list.verts.iter().any(|v| v.x == 100.0));
    // A closed polyline as the boundary.
    let pl = h(&s.execute("pline", &json!({ "vertices": [[10, -5], [30, -5], [30, 5], [10, 5]], "closed": true })).unwrap());
    s.execute("xclip", &json!({ "handle": r1, "polyline": pl })).unwrap();
    let c = clip(&s, &r1).unwrap();
    assert!(!c.inverted && c.points.len() == 4);
    // An open polyline is refused, a bad boundary changes nothing.
    let open = h(&s.execute("pline", &json!({ "vertices": [[0, 0], [5, 5], [9, 0]] })).unwrap());
    assert!(s.execute("xclip", &json!({ "handle": r1, "polyline": open })).is_err());
    assert!(s.execute("xclip", &json!({ "handle": r1, "points": [[0, 0], [1, 1]] })).is_err());
    assert!(s.execute("xclip", &json!({ "handle": r1, "rectangle": [[0, 0], [0, 10]] })).is_err());
    assert_eq!(clip(&s, &r1), Some(c));
    // Generate polyline makes the boundary an object.
    let n = s.doc().unwrap().model.len();
    let out = s.execute("xclip", &json!({ "handle": r1, "option": "polyline" })).unwrap();
    assert_eq!(out["polylines"].as_array().unwrap().len(), 1);
    assert_eq!(s.doc().unwrap().model.len(), n + 1);
    // Clip depth has no effect in 2D.
    assert_eq!(s.execute("xclip", &json!({ "handle": r1, "option": "clipdepth" })).unwrap()["ignored"], true);
}

#[test]
fn hostile_params_never_panic() {
    let (mut s, r1, _) = setup();
    for p in [
        json!({ "handle": r1, "points": "x" }),
        json!({ "handle": r1, "points": [[1e308, 0], [-1e308, 0], [0, 1e308]] }),
        json!({ "handle": r1, "rectangle": [[f64::MAX, 0]] }),
        json!({ "handle": r1, "rectangle": 5 }),
        json!({ "handle": r1, "polyline": "ZZZZ" }),
        json!({ "handle": r1, "option": "bogus" }),
        json!({ "handle": "nothex" }),
        json!({ "handles": [] }),
        json!({ "handle": r1, "points": vec![[0.0, 0.0]; 5000] }),
    ] {
        let _ = s.execute("xclip", &p);
    }
    assert!(s.execute("xclipframe", &json!({ "value": 7 })).is_err());
    assert!(s.execute("xclip", &json!({ "handle": r1, "option": "on" })).is_err(), "no boundary to turn on");
}

#[test]
fn typed_xclip_follows_the_prompts() {
    let (mut s, r1, r2) = setup();
    let pick = |hex: &str| Input::Pick(vec![Handle::parse_hex(hex).unwrap()]);
    s.start("xclip").unwrap();
    s.input(pick(&r1)).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.prompt_text().contains("Enter clipping option"), "{}", s.prompt_text());
    s.input(Input::Enter).unwrap(); // New
    assert!(s.prompt_text().contains("Invert clip"), "{}", s.prompt_text());
    s.input(Input::Keyword("Invert clip".into())).unwrap();
    assert!(s.log.iter().any(|l| l.contains("Inside mode")));
    s.input(Input::Keyword("Rectangular".into())).unwrap();
    s.input(Input::Point(Vec2::new(10.0, -10.0))).unwrap();
    s.input(Input::Point(Vec2::new(30.0, 10.0))).unwrap();
    let c = clip(&s, &r1).unwrap();
    assert!(c.inverted && c.points.len() == 2);
    // Again on the clipped reference: "Delete old boundary(s)?", then a polygon.
    s.cmdline("xclip").unwrap();
    s.input(pick(&r1)).unwrap();
    s.input(Input::Enter).unwrap();
    s.cmdline("n").unwrap();
    assert!(s.prompt_text().contains("Delete old boundary"), "{}", s.prompt_text());
    s.cmdline("y").unwrap();
    s.cmdline("p").unwrap();
    for p in ["0,-5", "20,-5", "20,5"] {
        s.cmdline(p).unwrap();
    }
    s.input(Input::Enter).unwrap();
    let c = clip(&s, &r1).unwrap();
    assert!(!c.inverted && c.points.len() == 3);
    // OFF, then Delete on two references at once.
    s.start("xclip").unwrap();
    s.input(pick(&r1)).unwrap();
    s.input(Input::Enter).unwrap();
    s.cmdline("off").unwrap();
    assert!(!clip(&s, &r1).unwrap().enabled);
    s.execute("xclip", &json!({ "handle": r2, "rectangle": [[0, 0], [1, 1]] })).unwrap();
    s.start("xclip").unwrap();
    s.input(Input::Pick(vec![Handle::parse_hex(&r1).unwrap(), Handle::parse_hex(&r2).unwrap()])).unwrap();
    s.input(Input::Enter).unwrap();
    s.cmdline("d").unwrap();
    assert!(clip(&s, &r1).is_none() && clip(&s, &r2).is_none());
    // XCLIPFRAME typed.
    s.cmdline("xclipframe").unwrap();
    s.cmdline("1").unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "XCLIPFRAME" })).unwrap()["value"], 1);
}

#[test]
fn the_frame_shows_and_plots_as_xclipframe_says() {
    let (mut s, r1, _) = setup();
    s.execute("xclip", &json!({ "handle": r1, "rectangle": [[20, -10], [70, 10]] })).unwrap();
    let frame_at = |s: &Session, plot: bool| {
        let d = s.doc().unwrap();
        let list =
            if plot { render::build_plot(d, &Space::Model, &Options::default()) } else { render::build(d, &Space::Model, &Options::default()) };
        list.verts.iter().any(|v| (v.y - 10.0).abs() < 1e-9 && (v.x - 20.0).abs() < 1e-9)
    };
    // Default 2: shown, not plotted.
    assert!(frame_at(&s, false) && !frame_at(&s, true));
    s.execute("xclipframe", &json!({ "value": 1 })).unwrap();
    assert!(frame_at(&s, false) && frame_at(&s, true));
    s.execute("xclipframe", &json!({ "value": 0 })).unwrap();
    assert!(!frame_at(&s, false) && !frame_at(&s, true));
}
