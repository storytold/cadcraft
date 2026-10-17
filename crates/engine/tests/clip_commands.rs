//! Tests for XCLIP, IMAGECLIP and CLIP commands (#AlphaGate).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Common, EntityKind, Handle, Image, Insert, LwPolyline, Space};
use cadcraft_engine::geom::{PolyVertex, Vec2, Vec3};
use serde_json::json;

fn h(v: &serde_json::Value) -> String {
    v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap().to_string()
}

fn setup_block(s: &mut Session, name: &str) -> String {
    let l = h(&s.execute("line", &json!({ "points": [[0, 0], [20, 20]] })).unwrap());
    s.execute("block", &json!({ "name": name, "base": [0, 0], "handles": [l], "keep": "delete" })).unwrap();
    let ins = Insert {
        block: name.into(),
        insert: Vec3::new(10.0, 10.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
        clip: None,
    };
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap().hex()
}

#[test]
fn xclip_rect_and_invert() {
    let mut s = Session::new();
    let ih = setup_block(&mut s, "BLK1");

    // Clip with rectangular boundary from (12, 12) to (18, 18) world coordinates.
    // Insert is at (10, 10), so in block coordinates this is (2, 2) to (8, 8).
    let r = s
        .execute(
            "xclip",
            &json!({
                "handle": ih,
                "rect": [[12, 12], [18, 18]],
                "invert": true,
            }),
        )
        .unwrap();

    assert_eq!(r["handle"], ih);
    assert_eq!(r["inverted"], true);

    let d = s.doc().unwrap();
    let EntityKind::Insert(ins) = &d.entity(Handle::parse_hex(&ih).unwrap()).unwrap().kind else {
        panic!("expected insert");
    };
    let clip = ins.clip.as_ref().expect("clip boundary present");
    assert!(clip.enabled);
    assert!(clip.inverted);
    assert_eq!(clip.points.len(), 4);
    assert!((clip.points[0].x - 2.0).abs() < 1e-9);
    assert!((clip.points[0].y - 2.0).abs() < 1e-9);
    assert!((clip.points[2].x - 8.0).abs() < 1e-9);
    assert!((clip.points[2].y - 8.0).abs() < 1e-9);
}

#[test]
fn xclip_on_off_and_delete() {
    let mut s = Session::new();
    let ih = setup_block(&mut s, "BLK2");

    s.execute(
        "xclip",
        &json!({
            "handle": ih,
            "rect": [[10, 10], [20, 20]],
        }),
    )
    .unwrap();

    // Turn off
    s.execute("xclip", &json!({ "handle": ih, "off": true })).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::Insert(ins) = &d.entity(Handle::parse_hex(&ih).unwrap()).unwrap().kind else {
        panic!();
    };
    assert!(!ins.clip.as_ref().unwrap().enabled);

    // Turn on
    s.execute("xclip", &json!({ "handle": ih, "on": true })).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::Insert(ins) = &d.entity(Handle::parse_hex(&ih).unwrap()).unwrap().kind else {
        panic!();
    };
    assert!(ins.clip.as_ref().unwrap().enabled);

    // Delete
    s.execute("xclip", &json!({ "handle": ih, "delete": true })).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::Insert(ins) = &d.entity(Handle::parse_hex(&ih).unwrap()).unwrap().kind else {
        panic!();
    };
    assert!(ins.clip.is_none());
}

#[test]
fn xclip_from_polyline_object_and_generate_polyline() {
    let mut s = Session::new();
    let ih = setup_block(&mut s, "BLK3");

    // Draw a closed polyline
    let poly = LwPolyline {
        vertices: vec![PolyVertex::new(Vec2::new(11.0, 11.0)), PolyVertex::new(Vec2::new(19.0, 11.0)), PolyVertex::new(Vec2::new(15.0, 19.0))],
        closed: true,
        const_width: 0.0,
        elevation: 0.0,
        plinegen: false,
    };
    let ph = s.doc_mut().unwrap().add(&Space::Model, Common::default(), EntityKind::LwPolyline(poly)).unwrap().hex();

    // Clip using the polyline
    s.execute(
        "xclip",
        &json!({
            "handle": ih,
            "object": ph,
        }),
    )
    .unwrap();

    let d = s.doc().unwrap();
    let EntityKind::Insert(ins) = &d.entity(Handle::parse_hex(&ih).unwrap()).unwrap().kind else {
        panic!();
    };
    let clip = ins.clip.as_ref().unwrap();
    assert_eq!(clip.points.len(), 3);
    assert!((clip.points[0].x - 1.0).abs() < 1e-9);
    assert!((clip.points[0].y - 1.0).abs() < 1e-9);

    // Generate polyline back from the clipping boundary
    let r = s
        .execute(
            "xclip",
            &json!({
                "handle": ih,
                "generate_polyline": true,
            }),
        )
        .unwrap();

    let gen_ph = r["polyline"].as_str().unwrap();
    let d = s.doc().unwrap();
    let EntityKind::LwPolyline(gen_poly) = &d.entity(Handle::parse_hex(gen_ph).unwrap()).unwrap().kind else {
        panic!();
    };
    assert!(gen_poly.closed);
    assert_eq!(gen_poly.vertices.len(), 3);
    assert!((gen_poly.vertices[0].p.x - 11.0).abs() < 1e-9);
    assert!((gen_poly.vertices[0].p.y - 11.0).abs() < 1e-9);
}

#[test]
fn imageclip_rectangular_boundary_and_toggle() {
    let mut s = Session::new();
    let img = Image {
        insert: Vec3::new(0.0, 0.0, 0.0),
        u: Vec3::new(1.0, 0.0, 0.0),
        v: Vec3::new(0.0, 1.0, 0.0),
        size: Vec2::new(100.0, 100.0),
        pixel_size: Vec2::new(1.0, 1.0),
        ..Default::default()
    };
    let ih = s.doc_mut().unwrap().add(&Space::Model, Common::default(), EntityKind::Image(img)).unwrap().hex();

    // Clip image
    s.execute(
        "imageclip",
        &json!({
            "handle": ih,
            "rect": [[10, 10], [50, 50]],
        }),
    )
    .unwrap();

    let d = s.doc().unwrap();
    let EntityKind::Image(img) = &d.entity(Handle::parse_hex(&ih).unwrap()).unwrap().kind else {
        panic!();
    };
    assert!(img.clipping);
    assert_eq!(img.clip.len(), 4);
    assert!((img.clip[0].x - 10.0).abs() < 1e-9);
    assert!((img.clip[2].x - 50.0).abs() < 1e-9);

    // Toggle off
    s.execute("imageclip", &json!({ "handle": ih, "off": true })).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::Image(img) = &d.entity(Handle::parse_hex(&ih).unwrap()).unwrap().kind else {
        panic!();
    };
    assert!(!img.clipping);

    // Delete
    s.execute("imageclip", &json!({ "handle": ih, "delete": true })).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::Image(img) = &d.entity(Handle::parse_hex(&ih).unwrap()).unwrap().kind else {
        panic!();
    };
    assert!(!img.clipping);
    assert!(img.clip.is_empty());
}

#[test]
fn clip_dispatcher() {
    let mut s = Session::new();
    let b_ih = setup_block(&mut s, "BLK_DISP");

    // Unified CLIP command dispatched to XCLIP
    s.execute(
        "clip",
        &json!({
            "handle": b_ih,
            "rect": [[12, 12], [18, 18]],
        }),
    )
    .unwrap();

    let d = s.doc().unwrap();
    let EntityKind::Insert(ins) = &d.entity(Handle::parse_hex(&b_ih).unwrap()).unwrap().kind else {
        panic!();
    };
    assert!(ins.clip.is_some());
}
