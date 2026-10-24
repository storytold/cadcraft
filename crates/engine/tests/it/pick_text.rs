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

/// Clicking in the empty wrap width of an MText does not select it.
#[test]
fn mtext_empty_wrap_width_is_not_picked() {
    let mut s = Session::new();
    // A short MText with a wide defined wrap width
    s.execute(
        "mtext",
        &json!({
            "at": [0, 0],
            "text": "HI",
            "height": 5.0,
            "width": 100.0,
            "attach": 1,
        }),
    )
    .unwrap();
    let d = s.doc().unwrap();
    let mtext_h = d.model.last().unwrap().handle;

    // Inside the actual letters "HI" (near origin): should be picked
    assert_eq!(pick(d, &Space::Model, Vec2::new(2.0, -2.5), 0.5), Some(mtext_h), "clicking on actual letters should pick");

    // In the empty space of the defined wrap box (e.g. at x=50.0): should NOT be picked
    assert_eq!(pick(d, &Space::Model, Vec2::new(50.0, -2.5), 0.5), None, "clicking in empty space of wrap width must not pick");
}

/// An MText's unused wrap width does not mask underlying geometry from being picked.
#[test]
fn mtext_empty_wrap_does_not_mask_other_entities() {
    let mut s = Session::new();
    // A line drawn at x=50 first
    s.execute(
        "line",
        &json!({
            "points": [[50.0, -10.0], [50.0, 10.0]]
        }),
    )
    .unwrap();
    let line_h = s.doc().unwrap().model.last().unwrap().handle;

    // An MText added after (topmost), with text 'HI' near x=0, but wide width=100 extending past x=50
    s.execute(
        "mtext",
        &json!({
            "at": [0, 0],
            "text": "HI",
            "height": 5.0,
            "width": 100.0,
            "attach": 1,
        }),
    )
    .unwrap();
    let d = s.doc().unwrap();

    // Clicking on the line at (50.0, 0.0) should pick the line, NOT the MText phantom box!
    assert_eq!(pick(d, &Space::Model, Vec2::new(50.0, 0.0), 0.5), Some(line_h), "should pick the line, not the MText phantom box");
}

/// A rotated MText is picked on its rotated glyphs and not in its rotated unused wrap width.
#[test]
fn mtext_rotated_empty_wrap_does_not_pick() {
    let mut s = Session::new();
    // MText rotated 90 degrees CCW, text 'HI' near origin, wide wrap width=100 extending along +Y
    s.execute(
        "mtext",
        &json!({
            "at": [0, 0],
            "text": "HI",
            "height": 5.0,
            "width": 100.0,
            "rotation": 90.0,
            "attach": 1,
        }),
    )
    .unwrap();
    let d = s.doc().unwrap();
    let mtext_h = d.model.last().unwrap().handle;

    // Inside the rotated letters "HI" (local x in [0, 6.33], y in [-5, 0] rotated 90 deg -> world x in [0, 5], y in [0, 6.33])
    assert_eq!(pick(d, &Space::Model, Vec2::new(2.5, 3.0), 0.5), Some(mtext_h), "clicking on rotated letters should pick");

    // In the empty space of the rotated wrap width (e.g. at world y=50.0, x=2.5): should NOT be picked
    assert_eq!(pick(d, &Space::Model, Vec2::new(2.5, 50.0), 0.5), None, "clicking in empty space of rotated wrap width must not pick");
}

/// Multiline MText uses per-line hit bounds so the ragged-right gap beside shorter lines is not picked.
#[test]
fn mtext_multiline_empty_wrap_does_not_pick() {
    let mut s = Session::new();
    // Multi-line MText with \P and unequal line lengths: line 1 extends past x=80,
    // line 2 is short, and wrap width is 200 extending far to the right.
    s.execute(
        "mtext",
        &json!({
            "at": [0, 0],
            "text": "DÒNG THỨ NHẤT RẤT DÀI CON ĐƯỜNG\\PDòng 2 ngắn",
            "height": 5.0,
            "width": 200.0,
            "attach": 1,
        }),
    )
    .unwrap();
    let d = s.doc().unwrap();
    let mtext_h = d.model.last().unwrap().handle;

    // Inside the actual text on line 1 (including at x=80.0): should be picked
    assert_eq!(pick(d, &Space::Model, Vec2::new(5.0, -2.5), 0.5), Some(mtext_h), "clicking on line 1 start should pick");
    assert_eq!(pick(d, &Space::Model, Vec2::new(80.0, -2.5), 0.5), Some(mtext_h), "x=80 is within longer line 1's horizontal extent");

    // Inside the actual text on shorter line 2: should be picked
    assert_eq!(pick(d, &Space::Model, Vec2::new(10.0, -10.5), 0.5), Some(mtext_h), "clicking on line 2 glyphs should pick");

    // Beside shorter line 2, but within longer line 1's horizontal extent (x=80.0): must NOT be picked
    assert_eq!(pick(d, &Space::Model, Vec2::new(80.0, -10.5), 0.5), None, "clicking in ragged-right gap beside shorter line 2 must not pick");

    // In the empty wrap width beyond both lines: must NOT be picked
    assert_eq!(pick(d, &Space::Model, Vec2::new(150.0, -2.5), 0.5), None, "clicking in empty wrap width beyond both lines must not pick");
}

/// Clicking directly on MText glyphs selects the MText entity.
#[test]
fn mtext_glyph_hit_picks_successfully() {
    let mut s = Session::new();
    s.execute(
        "mtext",
        &json!({
            "at": [10.0, 20.0],
            "text": "HELLO",
            "height": 4.0,
            "width": 50.0,
            "attach": 1,
        }),
    )
    .unwrap();
    let d = s.doc().unwrap();
    let mtext_h = d.model.last().unwrap().handle;

    // Clicking directly within the bounds of "HELLO" (at x=15.0, y=18.0)
    assert_eq!(pick(d, &Space::Model, Vec2::new(15.0, 18.0), 0.5), Some(mtext_h), "direct hit on MText glyphs should pick");
}

/// An MText with empty contents does not panic or get picked.
#[test]
fn mtext_empty_content_does_not_panic_or_pick() {
    let mut s = Session::new();
    s.execute(
        "mtext",
        &json!({
            "at": [0, 0],
            "text": "",
            "height": 5.0,
            "width": 100.0,
            "attach": 1,
        }),
    )
    .unwrap();
    let d = s.doc().unwrap();

    // Clicking anywhere in the empty MText box should not panic and must return None
    assert_eq!(pick(d, &Space::Model, Vec2::new(0.0, 0.0), 0.5), None);
    assert_eq!(pick(d, &Space::Model, Vec2::new(50.0, -2.5), 0.5), None);
}
