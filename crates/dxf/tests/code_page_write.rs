//! Pre-R2007 DXF text is written in the `$DWGCODEPAGE` code page: characters it has as their
//! bytes, the others as `\U+XXXX` escapes; every form reads back to the same text.

use cadcraft_dxf::{Tag, parse, write_ascii, write_ascii_in, write_binary_in};

fn file(code_page: &str, text: &str) -> Vec<Tag> {
    vec![
        Tag::s(0, "SECTION"),
        Tag::s(2, "HEADER"),
        Tag::s(9, "$ACADVER"),
        Tag::s(1, "AC1015"),
        Tag::s(9, "$DWGCODEPAGE"),
        Tag::s(3, code_page),
        Tag::s(0, "ENDSEC"),
        Tag::s(0, "SECTION"),
        Tag::s(2, "ENTITIES"),
        Tag::s(0, "TEXT"),
        Tag::s(1, text),
        Tag::s(0, "ENDSEC"),
        Tag::s(0, "EOF"),
    ]
}

fn contains(bytes: &[u8], part: &[u8]) -> bool {
    bytes.windows(part.len()).any(|w| w == part)
}

#[test]
fn text_is_written_in_the_code_page() {
    // (code page, text, the bytes written for it)
    let cases: [(&str, &str, &[u8]); 6] = [
        ("ANSI_1252", "Ação € 图", b"A\xE7\xE3o \x80 \\U+56FE"),
        ("ANSI_1251", "Слой é", b"\xD1\xEB\xEE\xE9 \\U+00E9"),
        ("ANSI_932", "表図面 é", b"\x95\x5C\x90\x7D\x96\xCA \\U+00E9"),
        ("DOS850", "Größe €", b"Gr\x94\xE1e \\U+20AC"),
        ("ANSI_1252", "😀", b"\\U+D83D\\U+DE00"),
        // A code page CADCraft doesn't know: everything non-ASCII is escaped.
        ("ANSI_1361", "Ação", b"A\\U+00E7\\U+00E3o"),
    ];
    for (cp, text, raw) in cases {
        let tags = file(cp, text);
        for (bytes, what) in [(write_ascii_in(&tags, cp), "ASCII"), (write_binary_in(&tags, cp), "binary")] {
            assert!(contains(&bytes, raw), "{cp} {what}: {:?}", String::from_utf8_lossy(&bytes));
            assert_eq!(parse(&bytes).unwrap()[10].str(), text, "{cp} {what}");
        }
    }
}

/// Without a known code page, and for `ASCII`, `UTF8` and `UNICODE` headers, the output is what
/// `write_ascii` writes (pure ASCII with escapes), line breaks included.
#[test]
fn unknown_ascii_and_utf8_code_pages_are_pure_ascii() {
    for cp in ["EBCDIC", "ASCII", "ascii ", "UTF8", "UNICODE"] {
        let tags = file(cp, "Ação\r\nfim");
        let bytes = write_ascii_in(&tags, cp);
        assert!(bytes.is_ascii(), "{cp}");
        assert_eq!(bytes, write_ascii(&tags).into_bytes(), "{cp}");
        assert!(contains(&bytes, b"A\\U+00E7\\U+00E3o  fim"), "{cp}");
        assert!(write_binary_in(&tags, cp).iter().all(u8::is_ascii), "{cp} binary");
    }
}

/// Code page bytes that happen to form valid UTF-8 (`СІ` in ANSI_1251 is D1 B2, which is U+0472
/// in UTF-8) would be read back as UTF-8 from an ASCII file, so that file is written with
/// escapes. Binary DXF reads strings by the header's code page and keeps the bytes.
#[test]
fn code_page_bytes_that_are_valid_utf8_are_escaped_in_ascii() {
    let tags = file("ANSI_1251", "СІ");
    let ascii = write_ascii_in(&tags, "ANSI_1251");
    assert!(ascii.is_ascii(), "{:?}", String::from_utf8_lossy(&ascii));
    assert!(contains(&ascii, b"\\U+0421\\U+0406"));
    assert_eq!(parse(&ascii).unwrap()[10].str(), "СІ");
    let binary = write_binary_in(&tags, "ANSI_1251");
    assert!(contains(&binary, b"\xD1\xB2"));
    assert_eq!(parse(&binary).unwrap()[10].str(), "СІ");
    // Bytes that are not valid UTF-8 stay as they are.
    assert!(contains(&write_ascii_in(&file("ANSI_1251", "Слой"), "ANSI_1251"), b"\xD1\xEB\xEE\xE9"));
}
