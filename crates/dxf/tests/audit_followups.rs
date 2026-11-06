//! Follow-ups of the crates/dxf audit (#1269): group code 255, the extra DOS and ISO code pages,
//! `format_real`, and the escape and integer writers that were rewritten without a change of
//! behaviour.

use cadcraft_dxf::{Tag, Value, decode_code_page, format_real, parse, write_ascii, write_binary};

#[test]
fn group_code_255_is_an_ordinary_code_in_binary_dxf() {
    // Code 255 used to be read as an R12 escape (`FF 00 <code>`), so a file with that code could
    // not be read back.
    let tags = vec![Tag::s(255, "x"), Tag::s(0, "EOF")];
    let back = parse(&write_binary(&tags, true)).unwrap();
    assert_eq!(back.len(), 2);
    assert_eq!((back[0].code, back[0].str()), (255, "x".to_string()));
}

#[test]
fn more_dos_and_iso_code_pages_decode() {
    // (code page, byte, the character the code page defines there)
    let cases: [(&str, u8, char); 12] = [
        ("DOS720", 0x82, 'é'),
        ("DOS737", 0x80, 'Α'),
        ("DOS775", 0x80, 'Ć'),
        ("DOS855", 0x80, 'ђ'),
        ("DOS857", 0x8D, 'ı'),
        ("DOS860", 0x84, 'ã'),
        ("DOS861", 0x8B, 'Ð'),
        ("DOS863", 0x84, 'Â'),
        ("DOS864", 0x80, '°'),
        ("DOS865", 0x9B, 'ø'),
        ("DOS869", 0x86, 'Ά'),
        ("ISO8859_15", 0xA4, '€'),
    ];
    for (name, byte, ch) in cases {
        assert_eq!(decode_code_page(&[byte], name), Some(ch.to_string()), "{name}");
        assert_eq!(decode_code_page(b"A", &name.to_lowercase()), Some("A".into()), "{name}");
    }
    // An unassigned byte is the replacement character, the rest decodes.
    assert_eq!(decode_code_page(b"\xD5A", "DOS857").as_deref(), Some("\u{FFFD}A"));
    // Johab still has no decoder.
    assert_eq!(decode_code_page(b"x", "ANSI_1361"), None);
}

#[test]
fn dos_code_pages_decode_known_characters() {
    // Four bytes per table with the character the reference mapping gives there (code point in the
    // comment); U+FFFD marks a byte the reference leaves undefined. DOS737, 775, 855, 857, 860, 861, 863,
    // 864, 865 and 869 come from the Unicode Consortium files in
    // https://unicode.org/Public/MAPPINGS/VENDORS/MICSFT/PC/ (CP<number>.TXT). DOS720 has no such file:
    // its reference is only Python's `cp720` codec, which maps the bytes below that we decode to U+FFFD
    // (0x80, 0x81, 0x84, 0x86, 0x8D, 0x8E, 0x8F, 0x90) to the C1 controls U+0080..U+0090.
    let tables: [(&str, [(u8, char); 4]); 11] = [
        ("DOS720", [(0x82, 'é'), (0x9B, 'ؤ'), (0xB3, '│'), (0xE3, 'ع')]), // U+00E9, U+0624, U+2502, U+0639
        ("DOS737", [(0x80, 'Α'), (0xAB, 'τ'), (0xB3, '│'), (0xF3, '≤')]), // U+0391, U+03C4, U+2502, U+2264
        ("DOS775", [(0x80, 'Ć'), (0xAB, '½'), (0xB3, '│'), (0xE3, 'Ń')]), // U+0106, U+00BD, U+2502, U+0143
        ("DOS855", [(0x80, 'ђ'), (0xA3, 'Б'), (0xB3, '│'), (0xFB, 'ч')]), // U+0452, U+0411, U+2502, U+0447
        ("DOS857", [(0x8D, 'ı'), (0xB3, '│'), (0xE3, 'Ò'), (0xD5, '\u{FFFD}')]), // U+0131, U+2502, U+00D2, undefined
        ("DOS860", [(0x84, 'ã'), (0x9B, '¢'), (0xB3, '│'), (0xE3, 'π')]), // U+00E3, U+00A2, U+2502, U+03C0
        ("DOS861", [(0x8B, 'Ð'), (0xAB, '½'), (0xB3, '│'), (0xFB, '√')]), // U+00D0, U+00BD, U+2502, U+221A
        ("DOS863", [(0x84, 'Â'), (0x9B, '¢'), (0xB3, '│'), (0xE3, 'π')]), // U+00C2, U+00A2, U+2502, U+03C0
        ("DOS864", [(0x80, '°'), (0xAB, 'ﺙ'), (0xB3, '٣'), (0xFF, '\u{FFFD}')]), // U+00B0, U+FE99, U+0663, undefined
        ("DOS865", [(0x9B, 'ø'), (0xAB, '½'), (0xB3, '│'), (0xE3, 'π')]), // U+00F8, U+00BD, U+2502, U+03C0
        ("DOS869", [(0x86, 'Ά'), (0x9B, 'ά'), (0xB3, '│'), (0x80, '\u{FFFD}')]), // U+0386, U+03AC, U+2502, undefined
    ];
    for (name, cases) in tables {
        for (byte, ch) in cases {
            assert_eq!(decode_code_page(&[byte], name), Some(ch.to_string()), "{name} 0x{byte:02X}");
        }
    }
    // DOS720: the 8 bytes the Python codec maps to C1 controls are unassigned here.
    for byte in [0x80u8, 0x81, 0x84, 0x86, 0x8D, 0x8E, 0x8F, 0x90] {
        assert_eq!(decode_code_page(&[byte], "DOS720").as_deref(), Some("\u{FFFD}"), "DOS720 0x{byte:02X}");
    }
}

#[test]
fn ascii_file_in_a_dos_code_page_reads_its_text() {
    let mut dxf = b"0\nSECTION\n2\nHEADER\n9\n$DWGCODEPAGE\n3\nDOS855\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n8\n".to_vec();
    dxf.extend_from_slice(b"\xA1\xA2\n0\nENDSEC\n0\nEOF\n");
    let tags = parse(&dxf).unwrap();
    let layer = tags.iter().find(|t| t.code == 8).unwrap();
    assert_eq!(layer.str(), decode_code_page(b"\xA1\xA2", "DOS855").unwrap());
    assert_eq!(layer.str(), "Аб");
}

#[test]
fn format_real_is_plain_decimal_with_a_point() {
    assert_eq!(format_real(5.0), "5.0");
    assert_eq!(format_real(-0.0), "-0.0");
    assert_eq!(format_real(0.1), "0.1");
    assert_eq!(format_real(1e21), "1000000000000000000000.0");
    assert_eq!(format_real(1e-7), "0.0000001");
    assert_eq!(format_real(f64::MAX).parse::<f64>().unwrap(), f64::MAX);
    assert_eq!(format_real(f64::NAN), "0.0");
    assert_eq!(format_real(f64::INFINITY), "0.0");
    assert_eq!(format_real(f64::NEG_INFINITY), "0.0");
}

#[test]
fn escapes_next_to_other_text_and_backslashes() {
    let one = |s: &str| parse(format!("1\n{s}\n0\nEOF\n").as_bytes()).unwrap()[0].str();
    assert_eq!(one("a\\U+00E9b"), "aéb");
    assert_eq!(one("é\\U+0041\\ ü\\M+5BCFE\\U+D83D\\U+DE00 x\\"), "éA\\ ü件😀 x\\");
    assert_eq!(one("\\\\U+0041"), "\\A");
    assert_eq!(one("\\U+D83D\\U+0041 \\U+DE00"), "\\U+D83DA \\U+DE00");
    assert_eq!(one("日本\\U+00"), "日本\\U+00");
    // A long run of backslashes (hostile) is read in linear time and stays literal.
    let slashes = "\\".repeat(100_000);
    assert_eq!(one(&slashes), slashes);
    // The same strings through the binary reader.
    // Built by hand: the writer escapes a literal `\U+` (#2050), so it would not produce an escape.
    let mut file = b"AutoCAD Binary DXF\r\n\x1a\0".to_vec();
    file.extend_from_slice(&1i16.to_le_bytes());
    file.extend_from_slice("é\\U+0041\\ ü".as_bytes());
    file.push(0);
    file.extend_from_slice(&0i16.to_le_bytes());
    file.extend_from_slice(b"EOF\0");
    assert_eq!(parse(&file).unwrap()[0].str(), "éA\\ ü");
    // And the writer keeps the literal text literal.
    let tags = [Tag::s(1, "é\\U+0041\\ ü"), Tag::s(0, "EOF")];
    assert_eq!(parse(&write_binary(&tags, true)).unwrap()[0].str(), "é\\U+0041\\ ü");
}

#[test]
fn integers_are_written_in_full() {
    let text = write_ascii(&[Tag::i(70, i64::MIN), Tag::i(90, -7), Tag::i(160, 0), Tag::new(290, Value::Bool(true))]);
    assert_eq!(text, " 70\r\n-9223372036854775808\r\n 90\r\n-7\r\n160\r\n0\r\n290\r\n1\r\n");
}

#[test]
fn binary_hex_reads_every_byte_in_upper_case() {
    let all: Vec<u8> = (0..=255).collect();
    let mut file = b"AutoCAD Binary DXF\r\n\x1a\0".to_vec();
    file.extend_from_slice(&[0x36, 0x01, 0xFF]);
    file.extend_from_slice(&all[..255]);
    file.extend_from_slice(&[0x00, 0x00, b'E', b'O', b'F', 0]);
    let tags = parse(&file).unwrap();
    let want: String = all[..255].iter().map(|b| format!("{b:02X}")).collect();
    assert_eq!(tags[0].value, Value::Hex(want));
}
