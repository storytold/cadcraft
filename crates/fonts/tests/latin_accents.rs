use cadcraft_fonts::{MTextParams, TextFont, char_advance, layout_mtext_with, line_width, shape_stroke};
use cadcraft_geom::Vec2;

#[test]
fn latin_accents_keep_single_strokes_and_the_base_letter_spacing() {
    let groups = [
        ('A', "ÀÁÂÃÄÅ"),
        ('C', "Ç"),
        ('E', "ÈÉÊË"),
        ('I', "ÌÍÎÏ"),
        ('N', "Ñ"),
        ('O', "ÒÓÔÕÖ"),
        ('U', "ÙÚÛÜ"),
        ('Y', "ÝŸ"),
        ('a', "àáâãäå"),
        ('c', "ç"),
        ('e', "èéêë"),
        ('i', "ìíîï"),
        ('n', "ñ"),
        ('o', "òóôõö"),
        ('u', "ùúûü"),
        ('y', "ýÿ"),
    ];
    for (base, accents) in groups {
        for c in accents.chars() {
            let shaped = shape_stroke(&c.to_string(), 6.0, 0.8, 0.2);
            assert!(shaped.glyphs.is_empty(), "{c} must not become a filled system-font outline");
            assert!(!shaped.strokes.is_empty());
            assert_eq!(char_advance(c), char_advance(base));
            assert!((line_width(&c.to_string(), 6.0, 0.8) - shaped.width).abs() < 1e-9);
            assert!(shaped.strokes.iter().flatten().all(|p| p.is_finite()));
            let next = shape_stroke(&format!("{c}B"), 6.0, 0.8, 0.2);
            let plain = shape_stroke(&format!("{base}B"), 6.0, 0.8, 0.2);
            assert_eq!(next.width, plain.width, "accent does not shift the next letter");
        }
    }
    for (accent, base) in [('Ç', 'C'), ('Ã', 'A'), ('Á', 'A'), ('Ó', 'O')] {
        let plain = shape_stroke(&base.to_string(), 6.0, 1.0, 0.0);
        let marked = shape_stroke(&accent.to_string(), 6.0, 1.0, 0.0);
        assert!(marked.strokes.starts_with(&plain.strokes), "same letter body for {accent}");
        assert!(marked.strokes.len() > plain.strokes.len());
    }
}

#[test]
fn portuguese_mtext_wraps_with_consistent_stroke_metrics() {
    let text = "A CONCEPÇÃO SERÁ APROVADA APÓS O PROCESSAMENTO DA ESTRUTURA";
    let params = MTextParams { width: 32.0, font: TextFont::Stroke, ..MTextParams::new(Vec2::ZERO, 1.0) };
    let layout = layout_mtext_with(text, &params);
    assert!(layout.lines.len() >= 2);
    assert_eq!(layout.lines.join(" "), text);
    assert!(layout.pieces.iter().all(|p| p.shaped.glyphs.is_empty()));
    assert!(layout.strokes.iter().flatten().all(|p| p.is_finite()));
}
