//! Picking (and the hover highlight, which picks the same way) finds dimensions and multileaders
//! anywhere on their words, as plain text is found anywhere inside its box.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle, Space};
use cadcraft_engine::geom::{Bounds2, Vec2};
use cadcraft_engine::render;
use cadcraft_engine::select::pick;
use serde_json::json;

/// The last entity of `kind` and the box (centre, half size along the text, angle) of its words.
fn text_of(s: &Session, kind: &str) -> (Handle, Vec2, Vec2, f64) {
    let d = s.doc().unwrap();
    let e = d.model.iter().filter(|e| format!("{:?}", e.kind).starts_with(kind)).last().unwrap();
    let (pts, angle): (Vec<Vec2>, f64) = match &e.kind {
        EntityKind::Dimension(dm) => {
            let g = render::dimension_in(d, dm);
            (g.text.iter().flatten().chain(g.text_glyphs.iter().flatten().flatten()).copied().collect(), g.text_angle)
        }
        EntityKind::MLeader(m) => {
            let t = m.text.as_ref().unwrap();
            (render::layout_mtext_entity(d, t).strokes.into_iter().flatten().collect(), t.rotation)
        }
        _ => unreachable!(),
    };
    let b = Bounds2::from_points(pts.iter().map(|q| q.rotate(-angle)));
    (e.handle, b.center().rotate(angle), Vec2::new(b.width() / 2.0, b.height() / 2.0), angle)
}

/// A point `(u, v)` half-sizes from the text box's centre, along and across the text.
fn at(c: Vec2, half: Vec2, angle: f64, u: f64, v: f64) -> Vec2 {
    c + Vec2::new(half.x * u, half.y * v).rotate(angle)
}

#[test]
fn dimension_and_multileader_text_is_picked_anywhere_on_its_words() {
    let mut s = Session::new();
    s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [100, 0], "at": [50, 30], "text": "TWO WORDS HERE"})).unwrap();
    let (h, c, half, angle) = text_of(&s, "Dimension");
    let tiny = half.y * 0.01;
    let d = s.doc().unwrap();
    // Between letters, at the ends and the corners of the words: all the dimension.
    for (u, v) in [(0.0, 0.0), (0.33, 0.1), (-0.9, -0.8), (0.9, 0.8), (0.5, -0.5)] {
        assert_eq!(pick(d, &Space::Model, at(c, half, angle, u, v), tiny), Some(h), "({u}, {v})");
    }
    assert_eq!(pick(d, &Space::Model, at(c, half, angle, 0.0, 6.0), tiny), None, "well above the words: nothing");

    s.execute("mleader", &json!({"points": [[200, 0], [220, 20]], "text": "LEADER NOTE"})).unwrap();
    let (h, c, half, angle) = text_of(&s, "MLeader");
    let d = s.doc().unwrap();
    for (u, v) in [(0.0, 0.0), (0.4, 0.2), (-0.8, -0.6)] {
        assert_eq!(pick(d, &Space::Model, at(c, half, angle, u, v), half.y * 0.01), Some(h), "leader text ({u}, {v})");
    }
}

#[test]
fn slanted_dimension_text_is_picked_inside_its_turned_box_only() {
    let mut s = Session::new();
    // Text along the dimension line (not kept horizontal), as in most surveying styles.
    for name in ["DIMTIH", "DIMTOH"] {
        s.execute("setvar", &json!({"name": name, "value": 0})).unwrap();
    }
    s.execute("dimaligned", &json!({"p1": [0, 0], "p2": [100, 100], "at": [40, 60], "text": "SLANTED LABEL"})).unwrap();
    let (h, c, half, angle) = text_of(&s, "Dimension");
    assert!((angle - std::f64::consts::FRAC_PI_4).abs() < 1e-6, "aligned with the 45° line: {angle}");
    let d = s.doc().unwrap();
    let tiny = half.y * 0.01;
    assert_eq!(pick(d, &Space::Model, at(c, half, angle, 0.6, 0.0), tiny), Some(h));
    // Inside the axis-aligned box around the turned words, but off them (and off the lines).
    let off = at(c, half, angle, 0.3, 4.0);
    let aabb = Bounds2::from_points([
        at(c, half, angle, -1.0, -1.0),
        at(c, half, angle, 1.0, -1.0),
        at(c, half, angle, 1.0, 1.0),
        at(c, half, angle, -1.0, 1.0),
    ]);
    assert!(aabb.contains(off), "the probe sits in the loose box");
    assert_ne!(pick(d, &Space::Model, off, tiny), Some(h), "but not on the words");
}

#[test]
fn a_dimension_drawn_from_its_block_is_picked_on_the_block_text() {
    // As AutoCAD saves dimensions: the geometry in an anonymous block, here with the text moved
    // well away from where the dimension's own geometry would set it.
    let mut s = Session::new();
    s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [100, 0], "at": [50, 30]})).unwrap();
    s.execute("mtext", &json!({"at": [400, 400], "text": "MOVED TEXT", "height": 5})).unwrap();
    let d = s.doc_mut().unwrap();
    let text = d.model.iter().find(|e| matches!(e.kind, EntityKind::MText(_))).unwrap().as_ref().clone();
    let dim = d.model.iter().find(|e| matches!(e.kind, EntityKind::Dimension(_))).unwrap().handle;
    d.model.remove(text.handle);
    let mut entities = cadcraft_engine::doc::EntityStore::new();
    entities.push(text);
    let block = cadcraft_engine::doc::Block {
        xref_overlay: false,
        xref_status: Default::default(),
        name: "*D1".into(),
        annotative: false,
        base: Default::default(),
        entities,
        description: String::new(),
        anonymous: true,
        xref_path: None,
        explodable: true,
        units: 0,
        array: None,
    };
    d.blocks.insert("*D1".into(), std::sync::Arc::new(block));
    d.model.modify(dim, |e| {
        if let EntityKind::Dimension(dm) = &mut e.kind {
            dm.block = Some("*D1".into());
        }
    });
    let d = s.doc().unwrap();
    let m = render::layout_mtext_entity(
        d,
        match &d.block("*D1").unwrap().entities.iter().next().unwrap().kind {
            EntityKind::MText(t) => t,
            _ => unreachable!(),
        },
    );
    let mid = m.bounds.center();
    assert_eq!(pick(d, &Space::Model, mid, 0.01), Some(dim), "on the block's text");
    assert_eq!(pick(d, &Space::Model, Vec2::new(50.0, 33.0), 0.01), None, "not where the dimension would have set it");
}

/// Dimensions and block references are picked on what they draw (and their text inside its box),
/// not along the edges of their bounding box, which run through empty space: a dimension's box
/// reaches out to its definition points, so its bottom edge lay on the line between them.
#[test]
fn bounding_box_edges_pick_nothing() {
    use cadcraft_engine::doc::entity_bounds;
    let mut s = Session::new();
    s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [100, 0], "at": [50, 30], "text": "16.00"})).unwrap();
    let d = s.doc().unwrap();
    let dim = d.model.iter().last().unwrap();
    let b = entity_bounds(d, dim, 0);
    let (h, mid_x) = (dim.handle, (b.min.x + b.max.x) / 2.0);
    let d = s.doc().unwrap();
    assert_eq!(pick(d, &Space::Model, Vec2::new(mid_x, b.min.y), 0.01), None, "bottom edge, between the definition points");
    assert_eq!(pick(d, &Space::Model, Vec2::new(mid_x, 30.0), 0.01), Some(h), "the dimension line still is");

    // A block of a diagonal line and a text, and a reference with an attribute.
    let mut s = Session::new();
    let l = s.execute("line", &json!({"points": [[0, 0], [10, 10]]})).unwrap()["handles"][0].clone();
    let t = s.execute("text", &json!({"at": [20, 0], "height": 2, "text": "TAG"})).unwrap()["handle"].clone();
    let a = s.execute("attdef", &json!({"tag": "ID", "prompt": "Id", "default": "A1", "at": [20, 5], "height": 2})).unwrap()["handle"].clone();
    s.execute("block", &json!({"name": "Mark", "base": [0, 0], "handles": [l, t, a]})).unwrap();
    let r = s.execute("insert", &json!({"name": "Mark", "at": [100, 100]})).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let d = s.doc().unwrap();
    let b = entity_bounds(d, d.entity(h).unwrap(), 0);
    // On the box's bottom edge, away from the line: nothing.
    assert_eq!(pick(d, &Space::Model, Vec2::new(108.0, b.min.y), 0.01), None);
    // On the line, and inside the block's text and the attribute: the reference.
    assert_eq!(pick(d, &Space::Model, Vec2::new(105.0, 105.0), 0.01), Some(h));
    assert_eq!(pick(d, &Space::Model, Vec2::new(121.5, 100.8), 0.01), Some(h), "the block's text");
    assert_eq!(pick(d, &Space::Model, Vec2::new(121.0, 105.8), 0.01), Some(h), "the attribute");
}

/// A fence, a crossing window and a crossing polygon across a block reference's text select it,
/// away from the lines its block also draws; the edges of its bounding box still select nothing.
#[test]
fn a_fence_or_crossing_across_block_text_selects_the_reference() {
    use cadcraft_engine::select::{select_fence, select_polygon, select_window};
    let mut s = Session::new();
    let l = s.execute("line", &json!({"points": [[0, 0], [10, 10]]})).unwrap()["handles"][0].clone();
    let t = s.execute("text", &json!({"at": [20, 0], "height": 2, "text": "TAG"})).unwrap()["handle"].clone();
    s.execute("block", &json!({"name": "Mark", "base": [0, 0], "handles": [l, t]})).unwrap();
    let r = s.execute("insert", &json!({"name": "Mark", "at": [100, 100]})).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let d = s.doc().unwrap();
    // Down through the text (x 120 to about 125, y 100 to 102), well right of the line.
    let fence = [Vec2::new(121.5, 110.0), Vec2::new(121.5, 95.0)];
    assert_eq!(select_fence(d, &Space::Model, &fence), [h]);
    let bx = Bounds2::new(Vec2::new(121.0, 100.5), Vec2::new(122.0, 101.5));
    assert_eq!(select_window(d, &Space::Model, bx, true), [h], "a crossing box inside the text");
    let poly = [Vec2::new(121.0, 95.0), Vec2::new(122.0, 95.0), Vec2::new(122.0, 101.0), Vec2::new(121.0, 101.0)];
    assert_eq!(select_polygon(d, &Space::Model, &poly, true), [h], "a crossing polygon over the text's edge");
    // A fence along the empty middle of the reference, crossing neither the line nor the text.
    assert!(select_fence(d, &Space::Model, &[Vec2::new(112.0, 108.0), Vec2::new(118.0, 108.0)]).is_empty());
}

/// A crossing box or polygon wholly inside a block reference's text, or inside a SOLID its block
/// fills, selects the reference, as it would select the text or the SOLID; one in the reference's
/// empty space, which its bounding box also covers, selects nothing.
#[test]
fn a_crossing_inside_block_text_or_fill_selects_the_reference_and_empty_space_does_not() {
    use cadcraft_engine::doc::{Common, Solid};
    use cadcraft_engine::geom::Vec3;
    use cadcraft_engine::select::{select_polygon, select_window};
    let mut s = Session::new();
    let l = s.execute("line", &json!({"points": [[0, 0], [10, 10]]})).unwrap()["handles"][0].clone();
    let t = s.execute("text", &json!({"at": [20, 0], "height": 2, "text": "TAG"})).unwrap()["handle"].clone();
    // A filled triangle at x 30 to 40.
    let v = |x: f64, y: f64| Vec3::new(x, y, 0.0);
    let solid = s
        .doc_mut()
        .unwrap()
        .add(&Space::Model, Common::default(), EntityKind::Solid(Solid { corners: [v(30.0, 0.0), v(40.0, 0.0), v(30.0, 10.0), v(30.0, 10.0)] }))
        .unwrap();
    s.execute("block", &json!({"name": "Mark", "base": [0, 0], "handles": [l, t, solid.hex()]})).unwrap();
    let r = s.execute("insert", &json!({"name": "Mark", "at": [100, 100]})).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let d = s.doc().unwrap();
    let crossing = |x0: f64, y0: f64, x1: f64, y1: f64| select_window(d, &Space::Model, Bounds2::new(Vec2::new(x0, y0), Vec2::new(x1, y1)), true);
    assert!(crossing(112.0, 106.0, 114.0, 108.0).is_empty(), "empty space between the line and the text");
    assert_eq!(crossing(131.0, 101.0, 133.0, 103.0), [h], "inside the filled triangle");
    assert!(crossing(137.0, 107.0, 139.0, 109.0).is_empty(), "beside the triangle's slope, outside its fill");
    let poly = [Vec2::new(121.0, 100.5), Vec2::new(122.0, 100.5), Vec2::new(122.0, 101.5), Vec2::new(121.0, 101.5)];
    assert_eq!(select_polygon(d, &Space::Model, &poly, true), [h], "a crossing polygon inside the text");
}
