//! MIRROR retains a hatch pattern's direction by default, while reflecting its boundary.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle, Hatch};
use cadcraft_engine::geom::Vec2;
use serde_json::{Value, json};

fn fixture() -> (Session, Handle) {
    fixture_with(json!({"pattern": "ANSI31", "angle": 30}))
}

fn fixture_with(mut params: Value) -> (Session, Handle) {
    let mut s = Session::new();
    let outline = s.execute("rectang", &json!({"p1": [2, 1], "p2": [8, 4]})).unwrap()["handle"].clone();
    params["handles"] = json!([outline]);
    params["associative"] = json!(false);
    let result = s.execute("hatch", &params).unwrap();
    let h = result["handles"][0].as_str().unwrap();
    let h = Handle::parse_hex(h).unwrap();
    (s, h)
}

fn hatch(s: &Session, h: Handle) -> Hatch {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Hatch(h) => h.clone(),
        _ => panic!("expected hatch"),
    }
}

#[test]
fn diagonal_mirror_keeps_pattern_direction_and_reflects_boundary() {
    let (mut s, source) = fixture();
    let before = hatch(&s, source);
    let result = s.execute("mirror", &json!({"handles": [source.hex()], "p1": [0, 0], "p2": [10, 10]})).unwrap();
    let mirrored = Handle::parse_hex(result["handles"][0].as_str().unwrap()).unwrap();
    let after = hatch(&s, mirrored);
    assert!((after.angle - before.angle).abs() < 1e-12, "pattern turned from {} to {} degrees", before.angle.to_degrees(), after.angle.to_degrees());
    for (a, b) in before.loops[0].vertices.iter().zip(&after.loops[0].vertices) {
        assert!((a.p.x - b.p.y).abs() < 1e-12);
        assert!((a.p.y - b.p.x).abs() < 1e-12);
    }
    assert_eq!(hatch(&s, source), before, "retained source is unchanged");
    let saved = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    let EntityKind::Hatch(saved) = &saved.entity(mirrored).unwrap().kind else { panic!("hatch lost on save") };
    assert!((saved.angle - before.angle).abs() < 1e-12);
}

#[test]
fn typed_mirror_previews_match_the_result_and_undo_redo() {
    let (mut s, h) = fixture();
    let before = hatch(&s, h);
    s.set_selection(vec![h]);
    s.cmdline("MIRROR 0,0").unwrap();
    let cursor = Vec2::new(10., 10.);
    let previews = s.preview(cursor);
    let preview = previews
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Hatch(h) => Some(h),
            _ => None,
        })
        .unwrap();
    assert_eq!(preview.angle, before.angle);
    let (drag, _) = s.drag(cursor).unwrap();
    assert!(!drag.handles.contains(&h), "GPU reflection would turn the hatch pattern");
    let drag_hatch = drag
        .extra
        .iter()
        .find_map(|k| match k {
            EntityKind::Hatch(h) => Some(h),
            _ => None,
        })
        .unwrap();
    assert_eq!(drag_hatch, preview);
    s.cmdline("10,10").unwrap();
    s.cmdline("Y").unwrap();
    assert!(s.running.is_none());
    let after = hatch(&s, h);
    assert_eq!(&after, preview);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(hatch(&s, h), before);
    s.execute("redo", &json!({})).unwrap();
    assert_eq!(hatch(&s, h), after);
}

#[test]
fn another_pattern_and_gradient_directions_are_retained() {
    for gradient in [false, true] {
        let (mut s, h) = fixture();
        if gradient {
            s.execute("hatchedit", &json!({"handles": [h.hex()], "gradient": {"name": "LINEAR", "angle": 20}})).unwrap();
        } else {
            s.execute("hatchedit", &json!({"handles": [h.hex()], "pattern": "ANSI32"})).unwrap();
        }
        let before = hatch(&s, h);
        s.execute("mirror", &json!({"handles": [h.hex()], "p1": [1, 2], "p2": [5, -1], "erase": true})).unwrap();
        let after = hatch(&s, h);
        assert_eq!(after.angle, before.angle);
        assert_eq!(after.pattern, before.pattern);
        assert_eq!(after.pattern_lines, before.pattern_lines);
        assert_eq!(after.gradient, before.gradient);
    }
}

#[test]
fn custom_pattern_line_families_are_not_reflected() {
    let (mut s, h) = fixture_with(json!({
        "pattern": "ORIGINAL", "angle": 30,
        "patternText": "*ORIGINAL,Original asymmetric regression pattern\n17,0.2,0.4,0.3,0.8,0.5,-0.2\n67,0.1,0.3,0.2,0.9,0.1,-0.4\n"
    }));
    let before = hatch(&s, h);
    assert_eq!(before.pattern_lines.len(), 2);
    s.execute("mirror", &json!({"handles": [h.hex()], "p1": [0, 0], "p2": [10, 10], "erase": true})).unwrap();
    let after = hatch(&s, h);
    assert_eq!(after.angle, before.angle);
    assert_eq!(after.pattern_lines, before.pattern_lines);
}

#[test]
fn solid_fills_can_still_use_the_gpu_preview() {
    let (mut s, h) = fixture_with(json!({"pattern": "SOLID"}));
    s.set_selection(vec![h]);
    s.cmdline("MIRROR 0,0").unwrap();
    let (drag, _) = s.drag(Vec2::new(10., 10.)).unwrap();
    assert!(drag.handles.contains(&h));
    assert!(!drag.extra.iter().any(|k| matches!(k, EntityKind::Hatch(_))));
}
