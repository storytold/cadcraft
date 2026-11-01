//! Binary DXF writing (#421): every value kind round-trips through `write_binary` and `parse`,
//! and hostile binary input never panics.

use cadcraft_dxf::{Tag, Value, is_binary, parse, write_ascii_as, write_binary};

fn sample() -> Vec<Tag> {
    vec![
        Tag::s(0, "SECTION"),
        Tag::s(2, "ENTITIES"),
        Tag::s(0, "TEXT"),
        Tag::s(5, "1F"),
        Tag::f(10, -3.25),
        Tag::i(62, 5),
        Tag::i(90, 70_000),
        Tag::i(160, 1 << 40),
        Tag::new(290, Value::Bool(true)),
        Tag::new(310, Value::Hex("00FFA5".into())),
        Tag::s(1, "图号 Café 😀"),
        Tag::i(1071, -2),
        Tag::s(0, "ENDSEC"),
        Tag::s(0, "EOF"),
    ]
}

#[test]
fn binary_roundtrips_every_value_kind() {
    for utf8 in [false, true] {
        let bytes = write_binary(&sample(), utf8);
        assert!(is_binary(&bytes));
        assert_eq!(parse(&bytes).unwrap(), sample(), "utf8 = {utf8}");
    }
    // Pre-2007 strings are escaped to ASCII, 2007+ ones are UTF-8.
    let old = write_binary(&sample(), false);
    assert!(old.windows(7).any(|w| w == b"\\U+56FE"));
    let new = write_binary(&sample(), true);
    assert!(new.windows("图号".len()).any(|w| w == "图号".as_bytes()));
}

#[test]
fn binary_converts_values_to_their_codes_kind() {
    // A number held as text at a numeric code, an out-of-range 16-bit value, a long chunk.
    let long = "AB".repeat(300);
    let tags = vec![Tag::s(70, "12"), Tag::i(62, 1 << 20), Tag::new(310, Value::Hex(long)), Tag::s(0, "EOF")];
    let back = parse(&write_binary(&tags, true)).unwrap();
    assert_eq!(back[0], Tag::i(70, 12));
    assert_eq!(back[1], Tag::i(62, i64::from(i16::MAX)));
    assert_eq!(back[2].code, 310);
    assert_eq!(back[3].code, 310, "300 bytes are split into chunks of at most 255");
    assert_eq!(back[2].str().len() + back[3].str().len(), 600);
}

#[test]
fn utf8_ascii_keeps_text_and_drops_line_breaks() {
    let text = write_ascii_as(&[Tag::s(1, "图号\r\nÄ"), Tag::s(0, "EOF")], true);
    assert!(text.contains("图号  Ä\r\n"));
    assert_eq!(parse(text.as_bytes()).unwrap()[0].str(), "图号  Ä");
}

#[test]
fn hostile_binary_input_never_panics() {
    let good = write_binary(&sample(), true);
    // Every truncation, and every single-byte corruption of the body.
    for n in 0..good.len() {
        let _ = parse(good.get(..n).unwrap());
    }
    for i in 22..good.len() {
        for b in [0u8, 0x7f, 0xff] {
            let mut bad = good.clone();
            bad[i] = b;
            let _ = parse(&bad);
        }
    }
    // A sentinel and nothing else is not a drawing.
    assert!(parse(b"AutoCAD Binary DXF\r\n\x1a\0").is_err());
}
