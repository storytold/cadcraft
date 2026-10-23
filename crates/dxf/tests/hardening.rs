//! Hostile-input hardening of the DXF reader and writer (#1025): one regression test per finding.

use cadcraft_dxf::{Tag, Value, parse, parse_with_limit, sections, write_ascii, write_binary};

const SENTINEL: &[u8] = b"AutoCAD Binary DXF\r\n\x1a\0";

fn bin_with(f: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut b = SENTINEL.to_vec();
    f(&mut b);
    b.extend_from_slice(&0i16.to_le_bytes());
    b.extend_from_slice(b"EOF\0");
    b
}

#[test]
fn binary_reals_that_are_not_finite_become_zero() {
    for bits in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let b = bin_with(|b| {
            b.extend_from_slice(&10i16.to_le_bytes());
            b.extend_from_slice(&bits.to_le_bytes());
        });
        let tags = parse(&b).unwrap();
        assert_eq!(tags[0], Tag::f(10, 0.0), "{bits}");
    }
}

#[test]
fn ascii_accepts_bom_and_cr_only_line_ends() {
    let want = vec![Tag::s(0, "SECTION"), Tag::s(2, "ENTITIES"), Tag::s(0, "ENDSEC"), Tag::s(0, "EOF")];
    let text = "0\rSECTION\r2\rENTITIES\r0\rENDSEC\r0\rEOF\r";
    assert_eq!(parse(text.as_bytes()).unwrap(), want);
    let mut bom = b"\xEF\xBB\xBF".to_vec();
    bom.extend_from_slice(text.replace('\r', "\r\n").as_bytes());
    assert_eq!(parse(&bom).unwrap(), want);
    // Mixed terminators.
    assert_eq!(parse(b"0\r\nSECTION\r2\nENTITIES\n0\rENDSEC\r\n0\nEOF").unwrap(), want);
}

#[test]
fn cr_only_files_count_against_the_tag_limit() {
    let mut text = String::new();
    for _ in 0..50 {
        text.push_str("999\rcomment\r");
    }
    text.push_str("0\rEOF\r");
    assert!(parse_with_limit(text.as_bytes(), 10).is_err());
    assert_eq!(parse_with_limit(text.as_bytes(), 100).unwrap().len(), 51);
}

#[test]
fn one_bad_byte_spoils_one_line_not_the_file() {
    // Header says ANSI_1251; one line is UTF-8, one is Windows-1251, one has a stray byte.
    let mut b = b"9\n$DWGCODEPAGE\n3\nANSI_1251\n1\n".to_vec();
    b.extend_from_slice("Größe".as_bytes());
    b.extend_from_slice(b"\n1\n\xCF\xF0\xE8\xE2\xE5\xF2\n0\nEOF\n");
    let t = parse(&b).unwrap();
    assert_eq!(t[1].str(), "ANSI_1251");
    assert_eq!(t[2].str(), "Größe");
    assert_eq!(t[3].str(), "Привет");
}

#[test]
fn ascii_and_binary_decode_alike() {
    // ANSI_1252 file with UTF-8 bytes in one string and a Latin-1 byte in another.
    let utf8 = "Größe".as_bytes();
    let latin = b"Caf\xE9";
    let mut a = b"9\n$DWGCODEPAGE\n3\nANSI_1252\n1\n".to_vec();
    a.extend_from_slice(utf8);
    a.extend_from_slice(b"\n1\n");
    a.extend_from_slice(latin);
    a.extend_from_slice(b"\n0\nEOF\n");
    let from_ascii = parse(&a).unwrap();
    let b = bin_with(|b| {
        for (code, s) in [(9i16, &b"$DWGCODEPAGE"[..]), (3, b"ANSI_1252"), (1, utf8), (1, latin)] {
            b.extend_from_slice(&code.to_le_bytes());
            b.extend_from_slice(s);
            b.push(0);
        }
    });
    let from_binary = parse(&b).unwrap();
    assert_eq!(from_ascii[2].str(), "Größe");
    assert_eq!(from_ascii[3].str(), "Café");
    assert_eq!(from_binary[2].str(), "Größe");
    assert_eq!(from_binary[3].str(), "Café");
}

#[test]
fn trailing_spaces_keep_sections_and_eof() {
    let t = parse(b"0\nSECTION  \n2\nENTITIES \n0\nLINE\n0\nENDSEC \n0\nEOF \n0\nSECTION\n2\nX\n0\nENDSEC\n").unwrap();
    // The EOF with a trailing space still ends the file.
    assert_eq!(t.len(), 5);
    let s = sections(&t);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].name, "ENTITIES");
    assert_eq!(s[0].tags.len(), 1);
    // Binary EOF too.
    let mut b = SENTINEL.to_vec();
    b.extend_from_slice(&0i16.to_le_bytes());
    b.extend_from_slice(b"EOF  \0");
    b.extend_from_slice(&1i16.to_le_bytes());
    b.extend_from_slice(b"after\0");
    assert_eq!(parse(&b).unwrap().len(), 1);
}

#[test]
fn integer_groups_reject_non_finite_and_clamp() {
    let t = parse(b"70\ninf\n70\nNaN\n70\n1e30\n70\n-1e30\n90\n1e30\n160\n1e30\n160\n-inf\n62\n12.0\n0\nEOF\n").unwrap();
    assert_eq!(t[0], Tag::i(70, 0));
    assert_eq!(t[1], Tag::i(70, 0));
    assert_eq!(t[2], Tag::i(70, i64::from(i16::MAX)));
    assert_eq!(t[3], Tag::i(70, i64::from(i16::MIN)));
    assert_eq!(t[4], Tag::i(90, i64::from(i32::MAX)));
    assert_eq!(t[5], Tag::i(160, i64::MAX));
    assert_eq!(t[6], Tag::i(160, 0));
    assert_eq!(t[7], Tag::i(62, 12));
    assert_eq!(Value::Str("inf".into()).as_i64(), 0);
}

#[test]
fn hex_groups_pad_odd_digits_and_drop_junk() {
    let t = parse(b"310\nABC\n310\nA-B zC1\n310\n\n1004\n0aF\n0\nEOF\n").unwrap();
    assert_eq!(t[0].value, Value::Hex("ABC0".into()));
    assert_eq!(t[1].value, Value::Hex("ABC1".into()));
    assert_eq!(t[2].value, Value::Hex(String::new()));
    assert_eq!(t[3].value, Value::Hex("0aF0".into()));
    // The binary writer agrees with the reader: the odd digit is padded, nothing shifts.
    let back = parse(&write_binary(&[Tag::new(310, Value::Hex("ABC".into())), Tag::s(0, "EOF")], true)).unwrap();
    assert_eq!(back[0].value, Value::Hex("ABC0".into()));
}

#[test]
fn ascii_hex_cannot_inject_tags_and_is_split_at_254_digits() {
    let evil = Tag::new(310, Value::Hex("AB\r\n0\r\nEOF".into()));
    let text = write_ascii(&[evil, Tag::s(0, "EOF")]);
    let back = parse(text.as_bytes()).unwrap();
    assert_eq!(back.len(), 2, "{text:?}");
    assert_eq!(back[0].value, Value::Hex("AB0EF0".into()));
    let long = "AB".repeat(300);
    let back = parse(write_ascii(&[Tag::new(310, Value::Hex(long)), Tag::s(0, "EOF")]).as_bytes()).unwrap();
    let sizes: Vec<usize> = back.iter().filter(|t| t.code == 310).map(|t| t.str().len()).collect();
    assert_eq!(sizes, vec![254, 254, 92]);
    // An empty chunk is still one group.
    let back = parse(write_ascii(&[Tag::new(310, Value::Hex(String::new())), Tag::s(0, "EOF")]).as_bytes()).unwrap();
    assert_eq!(back[0].code, 310);
}

#[test]
fn binary_sixteen_and_thirty_two_bit_values_wrap_in_range_and_clamp_outside() {
    let tags = [
        Tag::i(70, 65535),
        Tag::i(70, 40000),
        Tag::i(70, 70000),
        Tag::i(70, -40000),
        Tag::i(90, 0xFFFF_FFFF),
        Tag::i(90, 1 << 40),
        Tag::i(90, -(1 << 40)),
        Tag::s(0, "EOF"),
    ];
    let back = parse(&write_binary(&tags, true)).unwrap();
    assert_eq!(back[0], Tag::i(70, -1));
    assert_eq!(back[1], Tag::i(70, 40000 - 65536));
    assert_eq!(back[2], Tag::i(70, i64::from(i16::MAX)));
    assert_eq!(back[3], Tag::i(70, i64::from(i16::MIN)));
    assert_eq!(back[4], Tag::i(90, -1));
    assert_eq!(back[5], Tag::i(90, i64::from(i32::MAX)));
    assert_eq!(back[6], Tag::i(90, i64::from(i32::MIN)));
}

/// Strings of a `$DWGCODEPAGE`-declared file as ASCII text and as a binary file.
fn both_forms(code_page: &str, strings: &[&[u8]]) -> [Vec<Tag>; 2] {
    let mut a = format!("9\n$DWGCODEPAGE\n3\n{code_page}\n").into_bytes();
    for s in strings {
        a.extend_from_slice(b"1\n");
        a.extend_from_slice(s);
        a.push(b'\n');
    }
    a.extend_from_slice(b"0\nEOF\n");
    let b = bin_with(|b| {
        for (code, s) in [(9i16, &b"$DWGCODEPAGE"[..]), (3, code_page.as_bytes())].into_iter().chain(strings.iter().map(|s| (1, *s))) {
            b.extend_from_slice(&code.to_le_bytes());
            b.extend_from_slice(s);
            b.push(0);
        }
    });
    [parse(&a).unwrap(), parse(&b).unwrap()]
}

#[test]
fn east_asian_short_strings_that_look_like_utf8_stay_in_the_code_page() {
    // C3 B1 is valid UTF-8 ("ñ") but is also two half-width katakana in Shift-JIS; the second
    // string is plainly Shift-JIS, so the file is not UTF-8.
    for t in both_forms("ANSI_932", &[b"\xC3\xB1", b"\x90\x7D\x96\xCA"]) {
        assert_eq!(t[2].str(), "\u{FF83}\u{FF71}");
        assert_eq!(t[3].str(), "図面");
    }
}

#[test]
fn east_asian_file_that_is_all_utf8_reads_as_utf8() {
    for t in both_forms("ANSI_932", &["図面".as_bytes(), b"\xC3\xB1", b"plain"]) {
        assert_eq!(t[2].str(), "図面");
        assert_eq!(t[3].str(), "ñ");
        assert_eq!(t[4].str(), "plain");
    }
}
