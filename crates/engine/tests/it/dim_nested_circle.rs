//! DIMRADIUS, DIMDIAMETER and DIMJOGGED take a circle or arc inside a block reference, in nested
//! references too, as AutoCAD does (#1967). A pick on a block's line, or on a circle the reference
//! scales unevenly (drawn as an ellipse), is not a circle or arc.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{DimKind, Dimension, EntityKind};
use cadcraft_engine::geom::Vec2;
use serde_json::json;

/// A drawing with block "P": a circle of radius 5 and an arc of radius 3 from 0° to 90° at its
/// origin, and a line from (10, 0) to (20, 0).
fn session() -> Session {
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 5})).unwrap();
    s.execute("arc", &json!({"center": [0, 0], "radius": 3, "start": 0, "end": 90})).unwrap();
    s.execute("line", &json!({"points": [[10, 0], [20, 0]]})).unwrap();
    s.execute("selectall", &json!({})).unwrap();
    s.execute("block", &json!({"name": "P", "base": [0, 0], "keep": "delete"})).unwrap();
    s
}

fn dims(s: &Session) -> Vec<Dimension> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Dimension(d) = &e.kind { Some(d.clone()) } else { None }).collect()
}

/// `p` written for a typed point.
fn pt(p: Vec2) -> String {
    format!("{},{}", p.x, p.y)
}

#[test]
fn radius_and_diameter_of_a_circle_or_arc_in_a_rotated_scaled_reference() {
    let mut s = session();
    s.execute("insert", &json!({"name": "P", "at": [40, 10], "rotation": 30, "scale": 2})).unwrap();
    let c = Vec2::new(40.0, 10.0);

    // The circle: radius 10 in the drawing, picked away from the arc and the line.
    let on_circle = c + Vec2::from_angle(200f64.to_radians()) * 10.0;
    s.script(&format!("dimradius {} {}\n", pt(on_circle), pt(c + Vec2::new(-15.0, -8.0)))).unwrap();
    assert!(s.running.is_none());
    let d = dims(&s);
    assert_eq!(d.len(), 1, "a radius dimension of the nested circle");
    assert!(matches!(d[0].kind, DimKind::Radius));
    assert!(d[0].defpt.xy().dist(c) < 1e-9, "centred on the circle: {:?}", d[0].defpt);
    assert!((d[0].p15.xy().dist(c) - 10.0).abs() < 1e-9, "radius {}", d[0].p15.xy().dist(c));

    // The arc: the block's 0°–90° become 30°–120°, radius 6; its diameter is 12.
    let on_arc = c + Vec2::from_angle(75f64.to_radians()) * 6.0;
    s.script(&format!("dimdiameter {} {}\n", pt(on_arc), pt(c + Vec2::new(2.0, 20.0)))).unwrap();
    let d = dims(&s);
    assert_eq!(d.len(), 2, "a diameter dimension of the nested arc");
    assert!(matches!(d[1].kind, DimKind::Diameter));
    assert!((d[1].p15.xy().dist(d[1].defpt.xy()) - 12.0).abs() < 1e-9, "diameter {}", d[1].p15.xy().dist(d[1].defpt.xy()));

    // DIMJOGGED takes it too: a jogged radius dimension of the nested circle.
    s.script(&format!(
        "dimjogged {} {} {} {}\n",
        pt(on_circle),
        pt(c + Vec2::new(-30.0, -20.0)),
        pt(c + Vec2::new(-12.0, -9.0)),
        pt(c + Vec2::new(-20.0, -12.0))
    ))
    .unwrap();
    let d = dims(&s);
    assert_eq!(d.len(), 3, "a jogged dimension of the nested circle");
    assert!(d[2].marks.jogged.is_some());
    assert!((d[2].p15.xy().dist(c) - 10.0).abs() < 1e-9);
}

#[test]
fn a_circle_two_references_deep_is_taken() {
    let mut s = session();
    // Block Q holds P at (100, 0), half size; Q is inserted at (0, -50).
    let p = s.execute("insert", &json!({"name": "P", "at": [100, 0], "scale": 0.5})).unwrap()["handle"].as_str().unwrap().to_string();
    s.execute("block", &json!({"name": "Q", "base": [0, 0], "handles": [p], "keep": "delete"})).unwrap();
    s.execute("insert", &json!({"name": "Q", "at": [0, -50]})).unwrap();
    s.script("dimradius 100,-52.5 110,-60\n").unwrap();
    let d = dims(&s);
    assert_eq!(d.len(), 1);
    assert!(d[0].defpt.xy().dist(Vec2::new(100.0, -50.0)) < 1e-9, "{:?}", d[0].defpt);
    assert!((d[0].p15.xy().dist(Vec2::new(100.0, -50.0)) - 2.5).abs() < 1e-9);
}

/// A mirrored reference (scale -1 in X) still draws a circle: the dimension is centred on the
/// mirrored centre with the circle's radius, in drawing units. It is not associative: the
/// attachments name objects of the drawing, and the circle is inside the block.
#[test]
fn a_circle_in_a_mirrored_reference_is_taken_unassociated() {
    let mut s = session();
    s.execute("insert", &json!({"name": "P", "at": [-60, 0], "scale": [-1.5, 1.5]})).unwrap();
    let c = Vec2::new(-60.0, 0.0);
    // The circle has radius 7.5 here; picked on its left, away from the arc and the mirrored line.
    s.script(&format!("dimradius {} {}\n", pt(c + Vec2::new(0.0, -7.5)), pt(c + Vec2::new(-5.0, -15.0)))).unwrap();
    let d = dims(&s);
    assert_eq!(d.len(), 1, "a radius dimension of the circle in the mirrored reference");
    assert!(d[0].defpt.xy().dist(c) < 1e-9, "centred on the mirrored centre: {:?}", d[0].defpt);
    assert!((d[0].p15.xy().dist(c) - 7.5).abs() < 1e-9, "radius {}", d[0].p15.xy().dist(c));
    assert!(d[0].assoc.is_empty(), "{:?}", d[0].assoc);
}

#[test]
fn a_blocks_line_or_an_unevenly_scaled_circle_is_not_a_circle() {
    let mut s = session();
    // Twice as wide as tall: the circle is drawn as an ellipse.
    s.execute("insert", &json!({"name": "P", "at": [0, 100], "scale": [2, 1]})).unwrap();
    s.script("dimradius 10,100\n").unwrap();
    assert!(s.log.iter().any(|l| l == "Object selected is not a circle or arc."), "{:?}", s.log);
    s.cancel();
    // The block's line, in an evenly scaled reference.
    s.execute("insert", &json!({"name": "P", "at": [0, 200]})).unwrap();
    s.script("dimdiameter 15,200\n").unwrap();
    assert!(s.log.iter().filter(|l| *l == "Object selected is not a circle or arc.").count() == 2, "{:?}", s.log);
    s.cancel();
    assert!(dims(&s).is_empty(), "no dimension made");
}

/// A pick on a circle is not lost to an unevenly scaled circle nearby that comes first in the
/// block. That one is measured as the ellipse it is drawn as, not as a circle of its average
/// radius, which here passes through the very point picked.
#[test]
fn an_unevenly_scaled_circle_nearby_does_not_hide_a_circle() {
    let mut s = session();
    let avg = 50f64.sqrt(); // P's circle of radius 5 scaled 2 × 1: average radius 5·√2
    let mut handle = |p: serde_json::Value| s.execute("insert", &p).unwrap()["handle"].as_str().unwrap().to_string();
    let uneven = handle(json!({"name": "P", "at": [0, 0], "scale": [2, 1]}));
    let even = handle(json!({"name": "P", "at": [avg + 5.0, 0]}));
    s.execute("block", &json!({"name": "R", "base": [0, 0], "handles": [uneven, even], "keep": "delete"})).unwrap();
    s.execute("insert", &json!({"name": "R", "at": [0, 300]})).unwrap();
    s.script(&format!("dimradius {avg},300 {},310\n", avg + 5.0)).unwrap();
    let d = dims(&s);
    assert_eq!(d.len(), 1, "the even circle under the cursor is taken: {:?}", s.log.last());
    let c = Vec2::new(avg + 5.0, 300.0);
    assert!(d[0].defpt.xy().dist(c) < 1e-9, "{:?}", d[0].defpt);
    assert!((d[0].p15.xy().dist(c) - 5.0).abs() < 1e-9);
}

/// A block of `make` (run in model space, then made into block "N"), inserted at the origin.
fn nested(make: impl FnOnce(&mut Session)) -> Session {
    let mut s = Session::new();
    make(&mut s);
    s.execute("selectall", &json!({})).unwrap();
    s.execute("block", &json!({"name": "N", "base": [0, 0], "keep": "delete"})).unwrap();
    s.execute("insert", &json!({"name": "N", "at": [0, 0]})).unwrap();
    s
}

/// A pick on what else a block draws is measured as any pick measures it: on a text's box or
/// inside a SOLID it is that object, not a circle passing a hair away; behind a ray's start it is
/// not the ray.
#[test]
fn text_solids_and_rays_in_a_block_are_measured_as_they_are_drawn() {
    // Inside the box of a text, far from its insertion point.
    let mut s = nested(|s| {
        s.execute("text", &json!({"at": [0, 0], "text": "HELLO", "height": 2})).unwrap();
        s.execute("circle", &json!({"center": [8.000001, 1], "radius": 5})).unwrap();
    });
    // The pick is refused: DIMRADIUS says so and still asks for an arc or circle, where taking the
    // circle would have moved on to the dimension line's location.
    let refused = |s: &Session| {
        s.log.last().is_some_and(|l| l == "Object selected is not a circle or arc.") && s.prompt_text().contains("Select arc or circle")
    };
    s.script("dimradius 3,1\n").unwrap();
    assert!(refused(&s), "the pick is on the text: {:?} / {}", s.log, s.prompt_text());
    s.cancel();
    assert!(dims(&s).is_empty(), "the pick is on the text");

    // Inside a SOLID, far from its edges.
    let mut s = nested(|s| {
        s.execute("solid", &json!({"points": [[0, 0], [10, 0], [0, 10], [10, 10]]})).unwrap();
        s.execute("circle", &json!({"center": [10.000001, 5], "radius": 5})).unwrap();
    });
    let kinds: Vec<&str> = s.doc().unwrap().block("N").unwrap().entities.iter().map(|e| e.kind.type_name()).collect();
    assert!(kinds.contains(&"2D Solid"), "the block holds the SOLID: {kinds:?}");
    s.script("dimradius 5,5\n").unwrap();
    assert!(refused(&s), "the pick is on the SOLID: {:?} / {}", s.log, s.prompt_text());
    s.cancel();
    assert!(dims(&s).is_empty(), "the pick is on the SOLID");

    // Behind the start of a ray: the circle passing there is the object picked.
    let mut s = nested(|s| {
        s.execute("ray", &json!({"base": [0, 0], "through": [1, 0]})).unwrap();
        s.execute("circle", &json!({"center": [-5, 5.000001], "radius": 5})).unwrap();
    });
    s.script("dimradius -5,0 -12,-4\n").unwrap();
    let d = dims(&s);
    assert_eq!(d.len(), 1, "the circle behind the ray's start is taken: {:?}", s.log.last());
    assert!((d[0].p15.xy().dist(d[0].defpt.xy()) - 5.0).abs() < 1e-9);
}

/// In a layout viewport, a circle on a layer frozen there is not drawn, so it can't be picked.
#[test]
fn a_circle_on_a_layer_frozen_in_the_viewport_is_not_taken() {
    // Inside a layout viewport, the circle's layer frozen there or not.
    let viewport = |freeze: bool| {
        let mut s = nested(|s| {
            s.execute("layer.new", &json!({"name": "HID", "current": true})).unwrap();
            s.execute("circle", &json!({"center": [0, 0], "radius": 4})).unwrap();
            s.execute("layer.current", &json!({"name": "0"})).unwrap();
            s.execute("line", &json!({"points": [[-10, -10], [-9, -10]]})).unwrap();
        });
        s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
        let r = s.execute("mview", &json!({"p1": [20, 20], "p2": [220, 120]})).unwrap();
        let vp = r["viewports"][0].as_str().unwrap().to_string();
        s.execute("mspace", &json!({"handle": vp})).unwrap();
        if freeze {
            s.execute("vplayer", &json!({"handle": vp, "freeze": ["HID"]})).unwrap();
        }
        s.script("dimradius 4,0 6,3\n").unwrap();
        s.cancel();
        s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Dimension(_))).count()
    };
    assert_eq!(viewport(false), 1, "thawed, the circle is taken through the viewport");
    assert_eq!(viewport(true), 0, "frozen in the viewport, it is not drawn there and not taken");
}
