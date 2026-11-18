//! #3252: `%%c` (the diameter sign, U+2300) in a text style whose TrueType face does not map it
//! is drawn by the built-in stroke font, as SHX text draws it, not as the face's missing-glyph
//! rectangle. Few fonts map U+2300, so an AutoCAD drawing whose text style names one (Arial,
//! Liberation Serif, …) showed a box for every diameter sign.

use std::sync::Arc;

use cadcraft_fonts::{Shaped, shape_stroke, ttf};

/// A TrueType face with one mapped letter (`A`) and a rectangular `.notdef` glyph, so a
/// character it lacks is drawn as that rectangle unless a fallback font draws it. Built here so
/// the test does not depend on the fonts installed on the machine.
fn face_with_a_rectangular_notdef() -> ttf::Face {
    fn push_u16(v: &mut Vec<u8>, x: u16) {
        v.extend_from_slice(&x.to_be_bytes());
    }
    fn push_i16(v: &mut Vec<u8>, x: i16) {
        v.extend_from_slice(&x.to_be_bytes());
    }
    /// A one-contour on-curve polygon, padded to an even length (`loca` uses short offsets).
    fn glyph(points: &[(i16, i16)]) -> Vec<u8> {
        let (x_min, x_max) = points.iter().fold((0, 0), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
        let (y_min, y_max) = points.iter().fold((0, 0), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
        let mut v = Vec::new();
        push_i16(&mut v, 1); // one contour
        for x in [x_min, y_min, x_max, y_max] {
            push_i16(&mut v, x);
        }
        push_u16(&mut v, u16::try_from(points.len().saturating_sub(1)).unwrap_or(0)); // last point index
        push_u16(&mut v, 0); // no instructions
        v.extend(std::iter::repeat_n(0x01, points.len())); // on-curve, 16-bit coordinate deltas
        let mut last = 0;
        for (x, _) in points {
            push_i16(&mut v, x - last);
            last = *x;
        }
        last = 0;
        for (_, y) in points {
            push_i16(&mut v, y - last);
            last = *y;
        }
        if !v.len().is_multiple_of(2) {
            v.push(0);
        }
        v
    }

    let notdef = glyph(&[(0, 0), (500, 0), (500, 800), (0, 800)]);
    let letter = glyph(&[(0, 0), (250, 800), (500, 0)]);
    let mut glyf = notdef.clone();
    glyf.extend_from_slice(&letter);
    let mut loca = Vec::new();
    for offset in [0, notdef.len(), notdef.len() + letter.len()] {
        push_u16(&mut loca, u16::try_from(offset / 2).unwrap_or(0));
    }

    let mut head = vec![0_u8; 54];
    head[..4].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
    head[12..16].copy_from_slice(&0x5F0F_3CF5_u32.to_be_bytes());
    head[18..20].copy_from_slice(&1000_u16.to_be_bytes());
    head[38..40].copy_from_slice(&(-200_i16).to_be_bytes());
    head[40..42].copy_from_slice(&500_i16.to_be_bytes());
    head[42..44].copy_from_slice(&800_i16.to_be_bytes());
    head[50..52].copy_from_slice(&0_u16.to_be_bytes()); // short `loca` offsets
    let mut hhea = vec![0_u8; 36];
    hhea[..4].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
    hhea[4..6].copy_from_slice(&800_i16.to_be_bytes());
    hhea[6..8].copy_from_slice(&(-200_i16).to_be_bytes());
    hhea[34..36].copy_from_slice(&2_u16.to_be_bytes());
    let mut maxp = vec![0_u8; 32];
    maxp[..4].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&2_u16.to_be_bytes()); // two glyphs
    let mut hmtx = Vec::new();
    for _ in 0..2 {
        push_u16(&mut hmtx, 500); // advance
        push_i16(&mut hmtx, 0); // left side bearing
    }
    // A format 6 cmap: 'A' is glyph 1.
    let mut cmap = Vec::new();
    push_u16(&mut cmap, 0); // version
    push_u16(&mut cmap, 1); // one subtable
    push_u16(&mut cmap, 3); // Windows
    push_u16(&mut cmap, 1); // Unicode BMP
    cmap.extend_from_slice(&12_u32.to_be_bytes());
    push_u16(&mut cmap, 6); // format 6
    push_u16(&mut cmap, 12); // length
    push_u16(&mut cmap, 0); // language
    push_u16(&mut cmap, u16::from(b'A'));
    push_u16(&mut cmap, 1); // one entry
    push_u16(&mut cmap, 1); // 'A' -> glyph 1

    let tables: [(&[u8; 4], Vec<u8>); 7] =
        [(b"cmap", cmap), (b"glyf", glyf), (b"head", head), (b"hhea", hhea), (b"hmtx", hmtx), (b"loca", loca), (b"maxp", maxp)];
    let mut bytes = vec![0_u8; 12 + tables.len() * 16];
    bytes[..4].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
    bytes[4..6].copy_from_slice(&u16::try_from(tables.len()).unwrap_or(0).to_be_bytes());
    bytes[6..8].copy_from_slice(&64_u16.to_be_bytes());
    bytes[8..10].copy_from_slice(&2_u16.to_be_bytes());
    for (i, (tag, data)) in tables.iter().enumerate() {
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        let record = 12 + i * 16;
        let offset = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
        let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
        bytes[record..record + 4].copy_from_slice(*tag);
        bytes[record + 8..record + 12].copy_from_slice(&offset.to_be_bytes());
        bytes[record + 12..record + 16].copy_from_slice(&len.to_be_bytes());
        bytes.extend_from_slice(data);
    }
    ttf::Face::new(Arc::new(bytes), 0).expect("the synthetic face parses")
}

/// Whether shaped text contains the fixture's `.notdef` rectangle (comparing against the
/// face's own `.notdef` geometry so valid fallback glyphs with four-corner contours are not
/// mistaken for `.notdef`).
fn has_missing_glyph_box(face: &ttf::Face, sh: &Shaped) -> bool {
    let notdef = ttf::shape(face, "\u{E000}", 10.0, 1.0, 0.0).expect("notdef shapes");
    let Some(notdef_glyph) = notdef.glyphs.first() else {
        return false;
    };
    sh.glyphs.iter().any(|g| g == notdef_glyph)
}

#[test]
fn the_diameter_sign_is_drawn_when_no_font_has_it() {
    let face = face_with_a_rectangular_notdef();
    // The face maps 'A' but not the diameter sign: its own shape for the sign is the box.
    assert!(ttf::shape(&face, "A", 10.0, 1.0, 0.0).is_some_and(|sh| !sh.glyphs.is_empty()), "the letter is drawn from the face");
    let shaped = ttf::shape(&face, "%%c", 10.0, 1.0, 0.0).expect("the line shapes");
    assert!(!has_missing_glyph_box(&face, &shaped), "the diameter sign is a glyph, not a missing-glyph box");
    if ttf::fallback_for('\u{2300}').is_none() {
        // No installed font has the sign either: the built-in stroke font draws it, at its
        // own advance.
        let stroke = shape_stroke("\u{2300}", 10.0, 1.0, 0.0);
        assert_eq!(shaped.strokes, stroke.strokes, "the built-in stroke font's diameter sign");
        assert!(shaped.glyphs.is_empty(), "no outline glyph for a sign no font has");
        assert!((shaped.width - cadcraft_fonts::char_advance('\u{2300}') * 10.0).abs() < 1e-9, "the stroke font's advance");
    } else {
        assert!(!shaped.glyphs.is_empty(), "mapped fallback draws an outline glyph");
    }
}

#[test]
fn the_diameter_sign_keeps_its_width_when_no_font_has_it() {
    let face = face_with_a_rectangular_notdef();
    let Some(width) = ttf::width(&face, "%%c", 10.0, 1.0) else { panic!("the line is measured") };
    if ttf::fallback_for('\u{2300}').is_none() {
        assert!((width - cadcraft_fonts::char_advance('\u{2300}') * 10.0).abs() < 1e-9, "the stroke font's advance, not the .notdef advance");
    }
    assert!(width.is_finite() && width > 0.0);
}

#[test]
fn the_diameter_sign_is_drawn_in_right_to_left_text_when_no_font_has_it() {
    let face = face_with_a_rectangular_notdef();
    // A right-to-left line takes the bidi path, which picks a font per character.
    let shaped = ttf::shape_dir(&face, "A%%c", 10.0, 1.0, 0.0, Some(true)).expect("the line shapes");
    if ttf::fallback_for('\u{2300}').is_none() {
        let stroke = shape_stroke("\u{2300}", 10.0, 1.0, 0.0);
        assert_eq!(shaped.strokes.len(), stroke.strokes.len(), "the built-in stroke font's diameter sign");
        assert_eq!(shaped.glyphs.len(), 1, "only the letter is an outline glyph");
    } else {
        assert_eq!(shaped.glyphs.len(), 2, "both characters draw outline glyphs");
    }
    assert!(!has_missing_glyph_box(&face, &shaped), "the diameter sign is a glyph, not a missing-glyph box");
}
