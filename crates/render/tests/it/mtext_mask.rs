//! An MTEXT background mask is a box behind the text, filled with the drawing's background (a
//! mask, like a wipeout) or a color, hiding what was drawn before it; it spans the text, or
//! the defined width, out by the border offset factor.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Common, Drawing, EntityKind, Line, MText, MTextMask, Space};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_render::raster::{RasterOptions, View, render};
use cadcraft_render::{DisplayList, Kind, Options, build, layout_mtext_entity, mask_box};

fn mtext(mask: Option<MTextMask>, width: f64, attach: u8) -> MText {
    MText {
        insert: Vec3::new(5.0, 5.0, 0.0),
        height: 1.0,
        width,
        attach,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "MASKED".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
        direction: Default::default(),
        background: mask,
    }
}

/// A horizontal line through the text, then the text over it.
fn drawing(mask: Option<MTextMask>) -> Drawing {
    let mut d = Drawing::new_imperial();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(0.0, 4.5, 0.0), b: Vec3::new(20.0, 4.5, 0.0) })).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::MText(mtext(mask, 0.0, 1))).unwrap();
    d
}

/// Dark (drawn) pixels of the list on white paper (color 7 prints black).
fn dark_pixels(list: &DisplayList) -> usize {
    let view = View { center: Vec2::new(8.0, 4.5), scale: 20.0, width: 400, height: 120 };
    let pm = render(list, &view, &RasterOptions { background: Rgb(255, 255, 255), antialias: false, ..Default::default() }).unwrap();
    (0..120u32).flat_map(|y| (0..400u32).map(move |x| (x, y))).filter(|(x, y)| pm.pixel(*x, *y).is_some_and(|c| c.red() <= 128)).count()
}

#[test]
fn a_background_mask_hides_what_is_under_the_text() {
    let plain = build(&drawing(None), &Space::Model, &Options::default());
    assert_eq!(plain.prims.iter().filter(|p| p.kind == Kind::Mask).count(), 0);
    let masked = build(&drawing(Some(MTextMask::default())), &Space::Model, &Options::default());
    let kinds: Vec<Kind> = masked.prims.iter().map(|p| p.kind).collect();
    let mask_at = kinds.iter().position(|k| *k == Kind::Mask).expect("a mask");
    assert!(mask_at >= 1, "after the line, before the text: {kinds:?}");
    // The line's pixels under the box are gone; the text is drawn as before.
    assert!(dark_pixels(&masked) < dark_pixels(&plain));
}

#[test]
fn a_colored_mask_fills_with_its_color() {
    let red = MTextMask { use_background: false, color: Color::Index(1), scale: 1.5 };
    let list = build(&drawing(Some(red)), &Space::Model, &Options::default());
    assert_eq!(list.prims.iter().filter(|p| p.kind == Kind::Mask).count(), 0);
    assert!(list.prims.iter().any(|p| p.kind == Kind::Tris && p.color == Rgb(255, 0, 0)), "a red fill");
}

#[test]
fn the_box_spans_the_defined_width_out_by_the_border_offset() {
    let d = Drawing::new_imperial();
    // Top center, 20 wide: the box runs 10 each side of the insertion point.
    let t = mtext(Some(MTextMask { scale: 2.0, ..MTextMask::default() }), 20.0, 2);
    let l = layout_mtext_entity(&d, &t);
    let q = mask_box(&t, &l).unwrap();
    // Factor 2: one text height more, half on each side.
    assert!((q[0].x - (5.0 - 10.0 - 0.5)).abs() < 1e-9 && (q[1].x - (5.0 + 10.0 + 0.5)).abs() < 1e-9, "{q:?}");
    let fit = mtext(Some(MTextMask { scale: 1.0, ..MTextMask::default() }), 0.0, 1);
    let l = layout_mtext_entity(&d, &fit);
    let q = mask_box(&fit, &l).unwrap();
    let b = l.bounds;
    assert!((q[0].x - b.min.x).abs() < 1e-9 && (q[2].y - b.max.y).abs() < 1e-9, "factor 1 fits the text: {q:?} {b:?}");
    // No mask, or nothing drawn: no box.
    assert!(mask_box(&mtext(None, 0.0, 1), &l).is_none());
    let empty = MText { contents: String::new(), ..fit };
    assert!(mask_box(&empty, &layout_mtext_entity(&d, &empty)).is_none());
}

#[test]
fn hostile_masks_never_panic() {
    let d = Drawing::new_imperial();
    for scale in [f64::NAN, f64::INFINITY, -1.0, 1e300] {
        let mut t = mtext(Some(MTextMask { scale, ..MTextMask::default() }), f64::NAN, 9);
        t.rotation = f64::NAN;
        let l = layout_mtext_entity(&d, &t);
        let _ = mask_box(&t, &l);
    }
}
