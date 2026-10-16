use super::*;
use crate::{MTextParams, TextFont, layout_mtext_with, shape_line, text_width};

// Original synthetic glyph programs. These bytes are assembled here, never compiled in AutoCAD.
fn unifont(rows: &[(u16, &[u8])], mode: u8) -> Vec<u8> {
    let mut data = b"AutoCAD-86 unifont 1.0\r\n\x1a".to_vec();
    data.extend_from_slice(&(rows.len() as u16 + 1).to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    let definition = [b't', b'e', b's', b't', 0, 6, 2, mode, 0, 0, 0];
    data.extend_from_slice(&(definition.len() as u16).to_le_bytes());
    data.extend_from_slice(&definition);
    for (id, program) in rows {
        data.extend_from_slice(&id.to_le_bytes());
        data.extend_from_slice(&(program.len() as u16 + 1).to_le_bytes());
        data.push(0);
        data.extend_from_slice(program);
    }
    data
}

fn legacy(version: &str, rows: &[(u16, &[u8])]) -> Vec<u8> {
    let mut data = format!("AutoCAD-86 shapes {version}\r\n\x1a").into_bytes();
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&rows.last().unwrap().0.to_le_bytes());
    data.extend_from_slice(&(rows.len() as u16 + 1).to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&5u16.to_le_bytes());
    for (id, program) in rows {
        data.extend_from_slice(&id.to_le_bytes());
        data.extend_from_slice(&(program.len() as u16 + 1).to_le_bytes());
    }
    data.extend_from_slice(&[0, 6, 2, 2, 0]);
    for (_, program) in rows {
        data.push(0);
        data.extend_from_slice(program);
    }
    data.extend_from_slice(b"EOF");
    data
}

const A: &[u8] = &[2, 8, 1, 0, 1, 8, 0, 6, 8, 4, 250, 2, 8, 2, 0, 0];
const ACUTE: &[u8] = &[7, 0, 65, 5, 2, 8, 251, 7, 1, 8, 2, 1, 6, 0];
const SPACE: &[u8] = &[2, 8, 3, 0, 0];

#[test]
fn unicode_subshapes_use_high_byte_first_and_preserve_advance() {
    let font = Font::parse(&unifont(&[(65, A), (193, ACUTE), (32, SPACE)], 2)).unwrap();
    let a = font.glyph('A').unwrap();
    let acute = font.glyph('\u{00C1}').unwrap();
    assert_eq!(a.advance, 7.0);
    assert_eq!(acute.advance, a.advance);
    assert_eq!(acute.strokes.first(), a.strokes.first());
    assert!(acute.strokes.iter().flatten().any(|p| p.y == 8.0));
    let face = TextFont::Shx(Arc::new(font));
    let shaped = shape_line(&face, "A\u{00C1} A", 3.0, 0.8, 0.2);
    assert!(shaped.glyphs.is_empty(), "all glyphs stay single-stroke");
    assert!((shaped.width - 24.0 * 3.0 / 6.0 * 0.8).abs() < 1e-9);
    assert!((text_width(&face, "A\u{00C1} A", 3.0, 0.8) - shaped.width).abs() < 1e-9);
    assert!((crate::space_width(&face, 3.0, 0.8) - 1.2).abs() < 1e-9);
    assert!(shaped.strokes.iter().flatten().all(|p| p.is_finite()));
}

#[test]
fn legacy_versions_map_dimension_symbols_and_latin_characters() {
    for version in ["1.0", "1.1"] {
        let bytes = legacy(version, &[(65, A), (193, &[7, 65, 0]), (256, A), (257, A), (258, A)]);
        let font = Font::parse(&bytes).unwrap();
        for c in ['A', '\u{00C1}', '°', '±', '⌀'] {
            assert_eq!(font.glyph(c).unwrap().advance, 7.0);
        }
        assert_eq!(font.shape("%%d%%p%%c", 6.0, 1.0, 0.0).width, 21.0);
    }
}

#[test]
fn pen_commands_have_known_geometry() {
    let cases: &[(u16, &[u8], Vec2)] = &[
        (65, &[0x24, 0x20, 2, 0x2c, 0], Vec2::new(2.0, 0.0)),
        (66, &[4, 2, 8, 3, 2, 3, 2, 8, 1, 0, 0], Vec2::new(7.0, 4.0)),
        (67, &[9, 3, 1, 3, 255, 0, 0, 0], Vec2::new(6.0, 0.0)),
        (68, &[10, 3, 0x02, 0], Vec2::new(-3.0, 3.0)),
        (69, &[10, 3, 0x82, 0], Vec2::new(-3.0, -3.0)),
        (70, &[11, 0, 0, 0, 3, 0x02, 0], Vec2::new(-3.0, 3.0)),
        (71, &[12, 6, 0, 127, 0], Vec2::new(6.0, 0.0)),
        (72, &[13, 6, 0, 127, 6, 0, 129, 0, 0, 0], Vec2::new(12.0, 0.0)),
        (73, &[5, 8, 0, 6, 6, 2, 8, 7, 0, 0], Vec2::new(7.0, 0.0)),
        (74, &[14, 9, 50, 50, 50, 50, 0, 0, 8, 3, 0, 0], Vec2::new(3.0, 0.0)),
        (75, &[14, 7, 0, 65, 8, 3, 0, 0], Vec2::new(3.0, 0.0)),
    ];
    let rows: Vec<_> = cases.iter().map(|(id, bytes, _)| (*id, *bytes)).collect();
    let font = Font::parse(&unifont(&rows, 2)).unwrap();
    for (id, _, end) in cases {
        let glyph = program::decode(&font, *id).unwrap();
        assert!((glyph.advance - end.x).abs() < 1e-8, "glyph {id}");
        if !matches!(*id, 65 | 73) {
            let last = glyph.strokes.last().unwrap().last().unwrap();
            assert!(last.dist(*end) < 1e-8, "glyph {id}: {last:?}");
        }
    }
    let semicircle = font.glyph('G').unwrap();
    assert!(semicircle.strokes.iter().flatten().any(|p| (p.y + 3.0).abs() < 1e-8));
    let cw = font.glyph('E').unwrap();
    assert!(cw.strokes.iter().flatten().all(|p| p.y <= 1e-8));
}

#[test]
fn resolution_mtext_wrapping_and_inline_font_use_shx_metrics() {
    register("cadcraft-synthetic-unifont.shx", &unifont(&[(65, A), (193, ACUTE), (32, SPACE)], 2)).unwrap();
    let font = TextFont::resolve("CADCRAFT-SYNTHETIC-UNIFONT.SHX");
    assert!(matches!(font, TextFont::Shx(_)));
    assert!(matches!(TextFont::resolve("cadcraft-synthetic-unifont"), TextFont::Shx(_)));
    let mut params = MTextParams::new(Vec2::ZERO, 6.0);
    params.font = font;
    params.width = 18.0;
    let layout = layout_mtext_with("A\u{00C1} A\u{00C1}", &params);
    assert_eq!(layout.lines, ["A\u{00C1}", "A\u{00C1}"]);
    assert!(layout.pieces.iter().all(|p| p.shaped.glyphs.is_empty()));
    params.width = 0.0;
    params.font = TextFont::Stroke;
    let inline = layout_mtext_with("\\Fcadcraft-synthetic-unifont.shx;A\u{00C1}", &params);
    assert_eq!(inline.lines, ["A\u{00C1}"]);
    assert!(inline.pieces.iter().all(|p| p.shaped.glyphs.is_empty()));
    assert!(inline.strokes.iter().flatten().any(|p| (p.x - 12.0).abs() < 1e-9), "native pen-end spacing");
}

#[test]
fn configured_directory_loads_only_named_user_fonts() {
    // A child process isolates startup environment and the once-scanned registry, without
    // unsafe process-wide environment mutation in parallel Rust tests.
    if std::env::var_os("CADCRAFT_SHX_TEST_CHILD").is_some() {
        let font = find("CADCRAFT-SYNTHETIC-PATH.SHX").unwrap();
        assert_eq!(font.glyph('A').unwrap().advance, 7.0);
        assert!(matches!(TextFont::resolve("cadcraft-synthetic-path"), TextFont::Shx(_)));
        assert!(find("cadcraft-broken-path.shx").is_none());
        assert!(find("cadcraft-nonexistent-path.shx").is_none());
        assert!(matches!(TextFont::resolve("cadcraft-nonexistent-path.shx"), TextFont::Shx(_)), "SHX FONTALT");
        assert!(matches!(TextFont::resolve(crate::BUILTIN_FONT), TextFont::Stroke));
        assert!(find("../cadcraft-synthetic-path.ttf").is_none());
        return;
    }
    let dir = std::env::temp_dir().join(format!("cadcraft-shx-path-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("cadcraft-synthetic-path.shx"), unifont(&[(65, A)], 2)).unwrap();
    std::fs::write(dir.join("cadcraft-broken-path.shx"), b"not a font").unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "shx::tests::configured_directory_loads_only_named_user_fonts", "--nocapture"])
        .env("CADCRAFT_SHX_TEST_CHILD", "1")
        .env("CADCRAFT_SHXPATH", &dir)
        .env("CADCRAFT_FONTALT", "cadcraft-synthetic-path.shx")
        .output()
        .unwrap();
    std::fs::remove_file(dir.join("cadcraft-synthetic-path.shx")).unwrap();
    std::fs::remove_file(dir.join("cadcraft-broken-path.shx")).unwrap();
    std::fs::remove_dir(&dir).unwrap();
    assert!(result.status.success(), "{}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
}

#[test]
fn malformed_and_hostile_fonts_never_panic() {
    let valid = unifont(&[(65, A), (193, ACUTE)], 2);
    for end in 0..valid.len() {
        assert!(Font::parse(&valid[..end]).is_err(), "truncated at {end}");
    }
    let malformed: &[&[u8]] = &[
        &[7, 0, 65, 0],               // cycle
        &[6, 0],                      // stack underflow
        &[5, 0],                      // unbalanced stack
        &[3, 0, 0],                   // division by zero
        &[4, 255, 4, 255, 4, 255, 0], // excessive scale
        &[9, 1, 1, 0],                // missing displacement terminator
        &[11, 0],                     // truncated arc
        &[10, 1, 0x79, 0],            // invalid octant span
        &[15, 0],                     // unsupported opcode
    ];
    for program in malformed {
        let font = Font::parse(&unifont(&[(65, program)], 2)).unwrap();
        assert!(program::decode(&font, 65).is_err());
        let fallback = font.shape("A\u{00C1}", 1.0, 1.0, 0.0);
        assert!(!fallback.strokes.is_empty() && fallback.glyphs.is_empty());
    }
    // Deep acyclic calls, excessive points, random mutation, duplicates and bogus lengths.
    let mut chain: Vec<Vec<u8>> = (65u16..110).map(|id| vec![7, 0, (id + 1) as u8, 0]).collect();
    chain.push(vec![0]);
    let rows: Vec<_> = chain.iter().enumerate().map(|(i, p)| (65 + i as u16, p.as_slice())).collect();
    assert!(matches!(program::decode(&Font::parse(&unifont(&rows, 2)).unwrap(), 65), Err(Error::Limit)));
    let mut many_points = vec![0x10; 4096];
    many_points.push(0);
    let font = Font::parse(&unifont(&[(65, &[7, 0, 66, 7, 0, 66, 0]), (66, &many_points)], 2)).unwrap();
    assert!(matches!(program::decode(&font, 65), Err(Error::Limit)));
    let mut state = 0x1234_5678u32;
    for _ in 0..500 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let mut bytes = valid.clone();
        let i = state as usize % bytes.len();
        bytes[i] ^= (state >> 24) as u8;
        if let Ok(font) = Font::parse(&bytes) {
            let _ = font.shape("A\u{00C1}", 1.0, 1.0, 0.0);
        }
    }
    assert!(Font::parse(&unifont(&[(65, A), (65, A)], 2)).is_err());
    assert!(matches!(Font::parse(b"AutoCAD-86 bigfont 1.0\r\n\x1a"), Err(Error::Unsupported)));
    assert!(matches!(Font::parse(&vec![0; MAX_FILE_BYTES + 1]), Err(Error::Limit)));
    let font = Font::parse(&valid).unwrap();
    for n in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0] {
        let shaped = font.shape("A\u{00C1}B", n, n, n);
        assert!(shaped.width.is_finite());
        assert!(shaped.strokes.iter().flatten().all(|p| p.is_finite()));
    }
}
