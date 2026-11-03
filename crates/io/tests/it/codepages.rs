//! Pre-2007 DXF text is in the code page named by `$DWGCODEPAGE` (#505): Cyrillic, Central
//! European, Greek, Japanese, Chinese and Korean drawings open with their layer names, block names
//! and text intact, in ASCII and binary DXF, and a hostile file never panics.

use cadcraft_doc::{Drawing, EntityKind};

/// An ASCII DXF of `version` declaring `code_page`, with a layer, a block and a TEXT whose names
/// and value are the raw bytes given.
fn ascii_dxf(version: &str, code_page: &str, layer: &[u8], block: &[u8], text: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    let mut put = |code: i32, v: &[u8]| {
        b.extend_from_slice(format!("{code:>3}\r\n").as_bytes());
        b.extend_from_slice(v);
        b.extend_from_slice(b"\r\n");
    };
    for (c, v) in [(0, &b"SECTION"[..]), (2, b"HEADER"), (9, b"$ACADVER"), (1, version.as_bytes()), (9, b"$DWGCODEPAGE"), (3, code_page.as_bytes())] {
        put(c, v);
    }
    for (c, v) in [(0, &b"ENDSEC"[..]), (0, b"SECTION"), (2, b"TABLES"), (0, b"TABLE"), (2, b"LAYER"), (0, b"LAYER"), (2, layer), (70, b"0")] {
        put(c, v);
    }
    for (c, v) in [(62, &b"7"[..]), (6, b"CONTINUOUS"), (0, b"ENDTAB"), (0, b"ENDSEC"), (0, b"SECTION"), (2, b"BLOCKS"), (0, b"BLOCK")] {
        put(c, v);
    }
    for (c, v) in [(8, &b"0"[..]), (2, block), (70, b"0"), (10, b"0"), (20, b"0"), (30, b"0"), (0, b"LINE"), (8, b"0")] {
        put(c, v);
    }
    for (c, v) in [(10, &b"0"[..]), (20, b"0"), (11, b"1"), (21, b"1"), (0, b"ENDBLK"), (0, b"ENDSEC"), (0, b"SECTION"), (2, b"ENTITIES")] {
        put(c, v);
    }
    for (c, v) in [(0, &b"TEXT"[..]), (8, layer), (10, b"0"), (20, b"0"), (40, b"1"), (1, text), (0, b"INSERT"), (8, b"0")] {
        put(c, v);
    }
    for (c, v) in [(2, block), (10, b"5"), (20, b"5"), (0, b"ENDSEC"), (0, b"EOF")] {
        put(c, v);
    }
    b
}

fn text_of(d: &Drawing) -> String {
    d.model.iter().find_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None }).unwrap_or_default()
}

fn names(d: &Drawing) -> (String, String, String) {
    // The TEXT's layer, which the LAYER table defines.
    let layer = d.model.iter().find(|e| matches!(e.kind, EntityKind::Text(_))).map(|e| e.common.layer.clone()).unwrap_or_default();
    assert!(d.layers.iter().any(|l| l.name == layer), "layer `{layer}` is defined");
    let insert = d.model.iter().find_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.block.clone()) } else { None });
    let block = insert.unwrap_or_default();
    assert!(d.block(&block).is_some(), "block `{block}` is defined");
    (layer, block, text_of(d))
}

/// Text, layer and block names decode from each code page AutoCAD writes pre-2007 files in.
#[test]
fn reads_text_in_its_code_page() {
    // (code page, layer bytes, layer, text bytes, text); byte sequences from the code page tables.
    let cases: [(&str, &[u8], &str, &[u8], &str); 10] = [
        ("ANSI_1251", b"\xD1\xEB\xEE\xE9", "Слой", b"\xCF\xF0\xE8\xE2\xE5\xF2", "Привет"),
        ("ANSI_1250", b"\xA3\xF3d\x9F", "Łódź", b"\x8Ail\xE1", "Šilá"),
        ("ANSI_1253", b"\xD3\xF7\xDD\xE4\xE9\xEF", "Σχέδιο", b"\xE1\xE2\xE3", "αβγ"),
        ("ANSI_1254", b"\xC7izim", "Çizim", b"\xDE\xF0", "Şğ"),
        ("ANSI_874", b"\xC0\xD2\xBE", "ภาพ", b"\xC0\xD2\xBE", "ภาพ"),
        ("ANSI_932", b"\x90\x7D\x96\xCA", "図面", b"\x90\x7D\x96\xCA", "図面"),
        ("ANSI_936", b"\xCD\xBC\xB2\xE3", "图层", b"\xCD\xBC\xB2\xE3", "图层"),
        ("ANSI_949", b"\xB5\xB5\xB8\xE9", "도면", b"\xB5\xB5\xB8\xE9", "도면"),
        ("ANSI_950", b"\xB9\xCF\xBC\x68", "圖層", b"\xB9\xCF\xBC\x68", "圖層"),
        ("DOS850", b"Gr\x94\xE1e", "Größe", b"\x9B\x9D", "øØ"),
    ];
    for (cp, layer_bytes, layer, text_bytes, text) in cases {
        let mut block_bytes = b"B_".to_vec();
        block_bytes.extend_from_slice(layer_bytes);
        let d = cadcraft_io::read_dxf(&ascii_dxf("AC1015", cp, layer_bytes, &block_bytes, text_bytes)).unwrap();
        assert_eq!(names(&d), (layer.to_string(), format!("B_{layer}"), text.to_string()), "{cp}");
    }
}

/// A Shift-JIS trail byte can be 0x5C (`\`): 表 is 95 5C. Decoding the bytes before the escapes
/// keeps it a kanji instead of starting a `\U+` escape; real escapes still decode.
#[test]
fn shift_jis_backslash_trail_byte_is_not_an_escape() {
    let d = cadcraft_io::read_dxf(&ascii_dxf("AC1015", "ANSI_932", b"L", b"B", b"\x95\x5CU+0041 \\U+00E9 \\M+5BCFE")).unwrap();
    assert_eq!(text_of(&d), "表U+0041 é 件");
}

/// R2007+ files and files whose bytes are valid UTF-8 are UTF-8, whatever code page they declare
/// (the DWG bridge writes UTF-8 with the DWG's code page name); invalid UTF-8 in a 2007+ file
/// falls back to the declared code page.
#[test]
fn utf8_wins_when_valid() {
    let d = cadcraft_io::read_dxf(&ascii_dxf("AC1015", "ANSI_1251", "Слой".as_bytes(), b"B", "Привет".as_bytes())).unwrap();
    assert_eq!(names(&d), ("Слой".into(), "B".into(), "Привет".into()));
    let d = cadcraft_io::read_dxf(&ascii_dxf("AC1032", "ANSI_1252", "图层".as_bytes(), b"B", "Café".as_bytes())).unwrap();
    assert_eq!(names(&d), ("图层".into(), "B".into(), "Café".into()));
    let d = cadcraft_io::read_dxf(&ascii_dxf("AC1021", "ANSI_1251", b"\xD1\xEB\xEE\xE9", b"B", b"x")).unwrap();
    assert_eq!(names(&d).0, "Слой");
}

/// A code page CADCraft doesn't know (or none) reads as ANSI_1252, as before.
#[test]
fn unknown_code_page_falls_back_to_ansi_1252() {
    for cp in ["ANSI_1361", "EBCDIC", "", "\u{1}"] {
        let d = cadcraft_io::read_dxf(&ascii_dxf("AC1015", cp, b"Caf\xE9", b"B", b"\x80 \xD8")).unwrap();
        assert_eq!(names(&d), ("Café".into(), "B".into(), "€ Ø".into()), "{cp:?}");
    }
}

/// Reading converts to Unicode; saving writes pure ASCII with `\U+` escapes, which reads back the
/// same.
#[test]
fn code_page_text_survives_a_round_trip() {
    let d = cadcraft_io::read_dxf(&ascii_dxf("AC1015", "ANSI_1251", b"\xD1\xEB\xEE\xE9", b"\xC1\xEB\xEE\xEA", b"\xCF\xF0\xE8\xE2\xE5\xF2")).unwrap();
    let out = cadcraft_io::write_dxf(&d);
    assert!(out.is_ascii());
    let back = cadcraft_io::read_dxf(out.as_bytes()).unwrap();
    assert_eq!(names(&back), ("Слой".into(), "Блок".into(), "Привет".into()));
}

/// Saving a pre-2007 DXF writes the text in the file's code page, as the DXF Reference
/// describes: a Portuguese drawing keeps `Elétrica`, `Ação` and `Função` as ANSI_1252 bytes
/// instead of `\U+00E9` escapes, a Cyrillic one its ANSI_1251 bytes, and characters the code
/// page lacks are still escaped. ASCII and binary, R2000 and R2004, read back the same.
#[test]
fn saving_writes_text_in_the_code_page() {
    use cadcraft_io::{SaveOptions, Version};
    let pt = ascii_dxf("AC1015", "ANSI_1252", b"El\xE9trica", b"A\xE7\xE3o", b"Fun\xE7\xE3o \\U+56FE");
    let ru = ascii_dxf("AC1015", "ANSI_1251", b"\xD1\xEB\xEE\xE9", b"\xC1\xEB\xEE\xEA", b"\xCF\xF0\xE8\xE2\xE5\xF2 \\U+00E9");
    let cases: [(&[u8], (&str, &str, &str), &[&[u8]]); 2] = [
        (&pt, ("Elétrica", "Ação", "Função 图"), &[b"El\xE9trica", b"A\xE7\xE3o", b"Fun\xE7\xE3o \\U+56FE"]),
        (&ru, ("Слой", "Блок", "Привет é"), &[b"\xD1\xEB\xEE\xE9", b"\xC1\xEB\xEE\xEA", b"\xCF\xF0\xE8\xE2\xE5\xF2 \\U+00E9"]),
    ];
    for (file, want, raw) in cases {
        let d = cadcraft_io::read_dxf(file).unwrap();
        assert_eq!(names(&d), (want.0.into(), want.1.into(), want.2.into()));
        for version in [Version::R2000, Version::R2004] {
            for binary in [false, true] {
                let out = cadcraft_io::write_with(&d, "x.dxf", &SaveOptions { version: Some(version), binary }).unwrap();
                for r in raw {
                    assert!(out.windows(r.len()).any(|w| w == *r), "{version} binary={binary}: {}", String::from_utf8_lossy(r));
                }
                assert!(!out.windows(7).any(|w| w == b"\\U+00E7"), "{version} binary={binary}: ç is written as its byte");
                let back = cadcraft_io::read_dxf(&out).unwrap();
                assert_eq!(names(&back), (want.0.into(), want.1.into(), want.2.into()), "{version} binary={binary}");
            }
        }
    }
}

/// A file whose header declares `ASCII` keeps that code page and stays ASCII when saved: its
/// non-ASCII text is written as `\U+` escapes, not as Windows-1252 bytes.
#[test]
fn saving_an_ascii_file_keeps_it_ascii() {
    use cadcraft_io::{SaveOptions, Version};
    let d = cadcraft_io::read_dxf(&ascii_dxf("AC1015", "ASCII", b"El\\U+00E9trica", b"B", b"Eletrica \\U+00E9")).unwrap();
    assert_eq!(names(&d), ("Elétrica".into(), "B".into(), "Eletrica é".into()));
    for version in [Version::R2000, Version::R2004] {
        let ascii = cadcraft_io::write_with(&d, "x.dxf", &SaveOptions { version: Some(version), binary: false }).unwrap();
        assert!(ascii.is_ascii(), "{version}");
        let binary = cadcraft_io::write_with(&d, "x.dxf", &SaveOptions { version: Some(version), binary: true }).unwrap();
        for out in [&ascii, &binary] {
            assert!(out.windows(14).any(|w| w == b"El\\U+00E9trica"), "{version}");
            assert!(out.windows(5).any(|w| w == b"ASCII"), "{version}: the header keeps its code page");
            assert_eq!(names(&cadcraft_io::read_dxf(out).unwrap()), ("Elétrica".into(), "B".into(), "Eletrica é".into()), "{version}");
        }
    }
}

/// A binary DXF: R2000 strings are in the code page, R2007+ strings UTF-8.
#[test]
fn binary_dxf_honours_the_code_page() {
    let bin = |version: &str, cp: &str, text: &[u8]| {
        let mut b = b"AutoCAD Binary DXF\r\n\x1a\0".to_vec();
        let mut s = |code: i16, v: &[u8]| {
            b.extend_from_slice(&code.to_le_bytes());
            b.extend_from_slice(v);
            b.push(0);
        };
        s(0, b"SECTION");
        s(2, b"HEADER");
        s(9, b"$ACADVER");
        s(1, version.as_bytes());
        s(9, b"$DWGCODEPAGE");
        s(3, cp.as_bytes());
        s(0, b"ENDSEC");
        s(0, b"SECTION");
        s(2, b"ENTITIES");
        s(0, b"TEXT");
        s(8, b"0");
        b.extend_from_slice(&10i16.to_le_bytes());
        b.extend_from_slice(&0f64.to_le_bytes());
        b.extend_from_slice(&20i16.to_le_bytes());
        b.extend_from_slice(&0f64.to_le_bytes());
        b.extend_from_slice(&40i16.to_le_bytes());
        b.extend_from_slice(&1f64.to_le_bytes());
        let mut s = |code: i16, v: &[u8]| {
            b.extend_from_slice(&code.to_le_bytes());
            b.extend_from_slice(v);
            b.push(0);
        };
        s(1, text);
        s(0, b"ENDSEC");
        s(0, b"EOF");
        b
    };
    let read = |b: &[u8]| text_of(&cadcraft_io::read_dxf(b).unwrap());
    assert_eq!(read(&bin("AC1015", "ANSI_1251", b"\xCF\xF0\xE8\xE2\xE5\xF2")), "Привет");
    assert_eq!(read(&bin("AC1018", "ANSI_932", b"\x95\x5C\\U+00E9")), "表é");
    // A short Cyrillic string that is also valid UTF-8 (D1 B2) is still read in the code page.
    assert_eq!(read(&bin("AC1015", "ANSI_1251", b"\xD1\xB2")), "СІ");
    assert_eq!(read(&bin("AC1021", "ANSI_1251", "Привет".as_bytes())), "Привет");
    assert_eq!(read(&bin("AC1015", "ANSI_1361", b"Caf\xE9")), "Café");
}

/// Truncated and invalid multibyte sequences become U+FFFD; nothing panics and the structure of
/// the file (line ends, group codes) survives.
#[test]
fn hostile_multibyte_never_panics() {
    // A lead byte cut off by the line end, a lone trail byte, an invalid pair.
    let d = cadcraft_io::read_dxf(&ascii_dxf("AC1015", "ANSI_932", b"\x90", b"\x81", b"A\x90")).unwrap();
    assert_eq!(names(&d), ("\u{FFFD}".into(), "\u{FFFD}".into(), "A\u{FFFD}".into()));
    let d = cadcraft_io::read_dxf(&ascii_dxf("AC1015", "ANSI_936", b"\xFF\xFF", b"B", b"\xA1")).unwrap();
    assert_eq!(text_of(&d), "\u{FFFD}");
    // A file cut off inside a double-byte character, garbage after the header, every byte value.
    let mut cut = ascii_dxf("AC1015", "ANSI_949", b"L", b"B", b"x");
    cut.truncate(cut.len() - 10);
    cut.push(0xB5);
    let _ = cadcraft_io::read_dxf(&cut);
    let all: Vec<u8> = (0..=255u8).collect();
    for cp in ["ANSI_932", "ANSI_936", "ANSI_949", "ANSI_950", "ANSI_1251", "DOS437", "DOS852", "MACINTOSH"] {
        let d = cadcraft_io::read_dxf(&ascii_dxf(
            "AC1015",
            cp,
            b"L",
            b"B",
            &all.iter().copied().filter(|&b| b != b'\n' && b != b'\r').collect::<Vec<_>>(),
        ));
        assert!(d.is_ok(), "{cp}");
        let mut junk = ascii_dxf("AC1015", cp, b"L", b"B", b"x");
        junk.extend_from_slice(&all);
        let _ = cadcraft_io::read_dxf(&junk);
    }
}

/// A pre-2007 DWG keeps its code page through the acadrust bridge: a Cyrillic R2000 DXF saved as
/// DWG (acadrust writes 2004, text in ANSI_1251) opens with its text.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pre_2007_dwg_text_decodes_through_the_bridge() {
    let dxf = ascii_dxf("AC1015", "ANSI_1251", b"\xD1\xEB\xEE\xE9", b"\xC1\xEB\xEE\xEA", b"\xCF\xF0\xE8\xE2\xE5\xF2");
    let dwg = cadcraft_dwg::dxf_to_dwg(&dxf).unwrap();
    let d = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    assert_eq!(names(&d), ("Слой".into(), "Блок".into(), "Привет".into()));
}
