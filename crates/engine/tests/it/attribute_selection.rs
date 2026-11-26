//! Ordinary selection and block attribute text (#2626). A double-click finds a block's attribute
//! text through a list of its own. The selection index measures a block reference as the document
//! does. The index and plain scan give the same window, crossing, fence and pick results around
//! the reference's geometry and near its attribute text.

use cadcraft_engine::doc::{
    Attrib, AttribModes, AttribProps, Block, Common, Drawing, Entity, EntityKind, HAlign, HVal, Handle, Insert, Line, MText, MultilineAttrib, Space,
    Text, VAlign, entity_bounds,
};
use cadcraft_engine::geom::{Bounds2, Vec2, Vec3};
use cadcraft_engine::{select, spatial};

fn attrib(at: Vec3, value: &str) -> Attrib {
    Attrib {
        tag: "T".into(),
        text: Text {
            insert: at,
            align_pt: None,
            height: 1.0,
            value: value.into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: HAlign::Left,
            valign: VAlign::Baseline,
        },
        invisible: false,
        constant: false,
        prompt: String::new(),
        props: AttribProps::default(),
        multiline: None,
        modes: AttribModes::default(),
    }
}

/// A drawing with block "B" (a line from (0,0) to (1,0)), `filler` short lines far away, and one
/// reference to "B" at the origin with `a`, ATTMODE 1. Returns the drawing and the reference.
fn drawing(filler: usize, a: Attrib) -> (Drawing, Handle) {
    let mut d = Drawing::default();
    d.ensure_layer("OFF");
    if let Some(l) = d.layer_mut("OFF") {
        l.on = false;
    }
    d.header.set("ATTMODE", HVal::Int(1));
    let mut blk = Block::new("B");
    blk.entities.push(Entity::new(Handle(9_000), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) })));
    d.blocks.insert("B".into(), std::sync::Arc::new(blk));
    for i in 0..filler {
        let x = 1_000.0 + i as f64 * 3.0;
        d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(x, 500.0, 0.0), b: Vec3::new(x + 1.0, 500.0, 0.0) })).unwrap();
    }
    let ins = Insert { block: "B".into(), insert: Vec3::ZERO, attribs: vec![a], ..Insert::default() };
    let h = d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
    (d, h)
}

/// The reference's attribute variants: invisible (ATTMODE 1 hides it), on a layer turned off, and
/// multiline text whose inline height draws it far beyond the document's estimate.
fn variants() -> Vec<(&'static str, Attrib)> {
    let mut invisible = attrib(Vec3::new(100.0, 1.0, 0.0), "INVISIBLE");
    invisible.invisible = true;
    let mut hidden = attrib(Vec3::new(100.0, 1.0, 0.0), "HIDDEN");
    hidden.props.layer = Some("OFF".into());
    let mut formatted = attrib(Vec3::new(100.0, 1.0, 0.0), "A");
    formatted.multiline = Some(MultilineAttrib {
        mtext: MText {
            insert: Vec3::new(100.0, 1.0, 0.0),
            height: 1.0,
            width: 0.0,
            attach: 1,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "\\H1000x;A".into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        },
        read_as: "A".into(),
    });
    vec![("invisible", invisible), ("hidden layer", hidden), ("formatted multiline", formatted)]
}

/// Window and crossing selection of a reference with an invisible attribute far from its
/// geometry (review case): the window around the geometry takes it, a crossing box in the empty
/// space towards the invisible text does not, with the index (300 objects) and without (10).
#[test]
fn window_and_crossing_selection_ignore_attribute_text_that_is_not_drawn() {
    for filler in [10, 300] {
        let (name, a) = variants().remove(0);
        let (d, r) = drawing(filler, a);
        let window = select::select_window(&d, &Space::Model, Bounds2::new(Vec2::new(-1.0, -1.0), Vec2::new(2.0, 2.0)), false);
        assert!(window.contains(&r), "{name}, {filler} objects: the window around the geometry selects the reference: {window:?}");
        let crossing = select::select_window(&d, &Space::Model, Bounds2::new(Vec2::new(50.0, 0.25), Vec2::new(51.0, 0.75)), true);
        assert!(!crossing.contains(&r), "{name}, {filler} objects: an empty crossing box selects nothing: {crossing:?}");
    }
}

/// For each attribute variant, every ordinary selection gives the same answer with the index as
/// with the scan: a window, a crossing box and a fence where the renderer draws (or would draw)
/// the attribute's text, and a pick there.
#[test]
fn the_index_and_the_scan_select_references_alike_whatever_their_attributes() {
    let boxes = [
        (Bounds2::new(Vec2::new(-1.0, -1.0), Vec2::new(2.0, 2.0)), false),
        (Bounds2::new(Vec2::new(-1.0, -1.0), Vec2::new(2.0, 2.0)), true),
        (Bounds2::new(Vec2::new(50.0, 0.25), Vec2::new(51.0, 0.75)), true),
        (Bounds2::new(Vec2::new(99.0, -600.0), Vec2::new(140.0, 0.5)), true),
        (Bounds2::new(Vec2::new(-1.0, -1.0), Vec2::new(120.0, 3.0)), false),
    ];
    for (name, a) in variants() {
        let (small, r_small) = drawing(10, a.clone());
        let (large, r_large) = drawing(300, a);
        for (bx, crossing) in boxes {
            let s = select::select_window(&small, &Space::Model, bx, crossing).contains(&r_small);
            let l = select::select_window(&large, &Space::Model, bx, crossing).contains(&r_large);
            assert_eq!(s, l, "{name}: box {bx:?} crossing {crossing}: scan {s}, index {l}");
        }
        let fence = [Vec2::new(100.5, 5.0), Vec2::new(100.5, -400.0)];
        let s = select::select_fence(&small, &Space::Model, &fence).contains(&r_small);
        let l = select::select_fence(&large, &Space::Model, &fence).contains(&r_large);
        assert_eq!(s, l, "{name}: fence through the attribute text");
        assert!(!s, "{name}: the text-only fence misses the reference");
        for (p, expected) in [(Vec2::new(100.3, 1.4), false), (Vec2::new(100.3, -300.0), false), (Vec2::new(0.5, 0.0), true)] {
            let s = select::pick(&small, &Space::Model, p, 0.05) == Some(r_small);
            let l = select::pick(&large, &Space::Model, p, 0.05) == Some(r_large);
            assert_eq!(s, l, "{name}: scan and index picks at {p:?}");
            assert_eq!(s, expected, "{name}: expected reference hit at {p:?}");
        }
    }
}

/// The selection index measures every object as the document measures it: attribute text is not
/// laid out to measure a reference, whatever its attributes.
#[test]
fn the_selection_index_measures_references_as_the_document_does() {
    for (name, a) in variants() {
        let (d, _) = drawing(300, a);
        let store = d.space(&Space::Model).unwrap();
        let ix = spatial::index_store(&d, store);
        for (i, e) in store.iter().enumerate() {
            let i = u32::try_from(i).unwrap();
            assert_eq!(ix.bounds(i), entity_bounds(&d, e, 0), "{name}: object {i} ({:?})", e.handle);
        }
    }
}
