//! Deterministic tag preservation through the public byte API, in both encodings and every
//! supported version. Inputs have the group code's type and representable values; lossy
//! coercions are not accepted as round-trip equality. Real values must retain their bits.

use cadcraft_dxf::{Tag, Value, Version, is_binary, parse, write_ascii_as, write_binary};

fn check(case: &str, body: &[Tag]) {
    for version in Version::ALL {
        let mut original = vec![
            Tag::s(0, "SECTION"),
            Tag::s(2, "HEADER"),
            Tag::s(9, "$ACADVER"),
            Tag::s(1, version.code()),
            Tag::s(9, "$DWGCODEPAGE"),
            Tag::s(3, "ANSI_1252"),
            Tag::s(0, "ENDSEC"),
            Tag::s(0, "SECTION"),
            Tag::s(2, "ENTITIES"),
        ];
        original.extend_from_slice(body);
        original.extend([Tag::s(0, "ENDSEC"), Tag::s(0, "EOF")]);
        for binary in [false, true] {
            let mut current = original.clone();
            for pass in 1..=2 {
                let context = format!("{case} · {version} · binary={binary} · pass={pass}");
                let bytes = if binary { write_binary(&current, version.utf8()) } else { write_ascii_as(&current, version.utf8()).into_bytes() };
                assert_eq!(is_binary(&bytes), binary, "{context}");
                let back = parse(&bytes).unwrap_or_else(|e| panic!("{context}: {e}"));
                assert_eq!(back.len(), original.len(), "{context}: tag count");
                for (index, (expected, actual)) in original.iter().zip(&back).enumerate() {
                    assert_eq!(actual, expected, "{context}: tag[{index}]");
                    if let (Value::Real(a), Value::Real(b)) = (&expected.value, &actual.value) {
                        assert_eq!(b.to_bits(), a.to_bits(), "{context}: tag[{index}] real bits");
                    }
                }
                assert_eq!(back, current, "{context}: no additional changes on resave");
                current = back;
            }
        }
    }
}

#[test]
fn integer_width_boundaries() {
    // Explicit reference ranges, not obtained from the codec's kind_of().
    let widths = [
        (vec![60..=79, 170..=179, 270..=289, 370..=389, 400..=409, 1060..=1070], i64::from(i16::MIN), i64::from(i16::MAX)),
        (vec![90..=99, 420..=429, 440..=459, 1071..=1071], i64::from(i32::MIN), i64::from(i32::MAX)),
        (std::iter::once(160..=169).collect(), i64::MIN, i64::MAX),
    ];
    for (ranges, min, max) in widths {
        for value in [min, min + 1, -1, 0, 1, max - 1, max] {
            let tags: Vec<_> = ranges.iter().cloned().flatten().map(|code| Tag::i(code, value)).collect();
            check(&format!("integer/{min}..{max}/{value}"), &tags);
        }
    }
}

#[test]
fn finite_reals_keep_all_bits() {
    let values = [
        0.0,
        -0.0,
        f64::from_bits(1),
        -f64::from_bits(1),
        f64::MIN_POSITIVE,
        f64::MIN_POSITIVE.next_down(),
        0.1,
        -3.25,
        1.0_f64.next_down(),
        1.0,
        1.0_f64.next_up(),
        1e-100,
        -1e100,
        f64::MAX,
        f64::MIN,
    ];
    for value in values {
        let tags: Vec<_> =
            [10..=59, 110..=149, 210..=239, 460..=469, 1010..=1059].into_iter().flatten().map(|code| Tag::new(code, Value::Real(value))).collect();
        check(&format!("real/{:016X}", value.to_bits()), &tags);
    }
}

#[test]
fn booleans() {
    for value in [false, true] {
        let tags: Vec<_> = (290..=299).map(|code| Tag::new(code, Value::Bool(value))).collect();
        check(&format!("bool/{value}"), &tags);
    }
}

#[test]
fn strings_preserve_whitespace_unicode_and_literal_escapes() {
    // No CR/LF or NUL: those are line/string delimiters, not losslessly writable payloads.
    let values = [
        ("empty", ""),
        ("spaces", "  leading and trailing  "),
        ("tabs", "\tindented\t"),
        ("unicode", "Café Ø 图号 Україна 😀 e\u{301}"),
        ("literal-escapes", r"\U+0041 \M+183A4 \U+D83D\U+DE00 \\U+005C \P %%c"),
        ("malformed-escapes", r"\U+ \U+ZZZZ \U+D800 \U+DC00 \M+9FFFF"),
    ];
    for (case, value) in values {
        let tags: Vec<_> =
            [0..=9, 100..=100, 102..=102, 105..=105, 300..=309, 320..=369, 410..=419, 430..=439, 470..=479, 480..=481, 1000..=1003, 1005..=1005]
                .into_iter()
                .flatten()
                .map(|code| Tag::s(code, value))
                .collect();
        check(case, &tags);
    }
    for length in [1, 254, 255, 256, 1024] {
        check(&format!("string/length={length}"), &[Tag::s(1, "x".repeat(length))]);
    }
}

#[test]
fn hex_chunks_keep_bytes_and_order() {
    // DXF groups 310–319 and 1004 permit 127 payload bytes per chunk. A binary length
    // byte can represent more, but that is reader tolerance, not writer conformance.
    // Larger payloads need chunk-size/reassembly checks rather than exact tag equality;
    // see hex_chunks.rs for the independent wire checks and pending writer regressions.
    for length in [0, 1, 2, 126, 127] {
        let hex: String = (0..length).map(|i| format!("{:02X}", (i * 37) % 256)).collect();
        let tags: Vec<_> = (310..=319).chain([1004]).map(|code| Tag::new(code, Value::Hex(hex.clone()))).collect();
        check(&format!("hex/bytes={length}"), &tags);
    }
}

#[test]
fn mixed_values_and_repeated_codes_keep_order() {
    check(
        "mixed/repeated-codes",
        &[
            Tag::s(0, "TEST"),
            Tag::f(10, 1.25),
            Tag::s(1, "first"),
            Tag::f(10, -2.5),
            Tag::i(70, -7),
            Tag::s(1, "second"),
            Tag::new(290, Value::Bool(true)),
            Tag::new(310, Value::Hex("00FF80".into())),
            Tag::i(160, i64::MAX),
            Tag::s(1, ""),
        ],
    );
}
