//! Hex payload conformance is distinct from binary framing capacity: DXF groups 310–319 and
//! 1004 specify at most 127 payload bytes per chunk, even though a length byte represents 255.
//! Inspect writer bytes without our parser; accept any compliant partition preserving payload,
//! code and following-record framing. Oversized binary read fixtures test tolerance only.
//! Pending writer regressions extend the chunk-size finding in issue #1025.
//! Public DXF Reference, Group Codes in Numerical Order and Binary DXF Files:
//! https://help.autodesk.com/cloudhelp/2025/ENU/AutoCAD-DXF/files/GUID-3F0380A5-1C15-464D-BC66-2C5F094BCFB9.htm
//! https://help.autodesk.com/cloudhelp/2025/ENU/AutoCAD-DXF/files/GUID-FC1C3C69-DBC2-49E4-893A-000D6538C0FE.htm

use cadcraft_dxf::{Tag, Value, parse, write_ascii_as, write_binary};

const SENTINEL: &[u8] = b"AutoCAD Binary DXF\r\n\x1a\0";
const FOLLOWING_BINARY: &[u8] = b"\x46\0\xf9\xff"; // group 70, signed i16 -7

fn payload(length: usize) -> Vec<u8> {
    (0..length).map(|index| u8::try_from(index * 37 % 256).unwrap()).collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

fn input(code: i32, bytes: &[u8]) -> [Tag; 2] {
    [Tag::new(code, Value::Hex(hex(bytes))), Tag::i(70, -7)]
}

fn check_ascii(code: i32, bytes: &[u8], utf8: bool) {
    let wire = write_ascii_as(&input(code, bytes), utf8);
    let lines: Vec<_> = wire.lines().collect();
    let (chunks, following) = lines.split_at(lines.len().checked_sub(2).unwrap());
    assert_eq!(following[0].trim(), "70");
    assert_eq!(following[1], "-7");
    assert!(!chunks.is_empty(), "an empty payload must still have a tag");
    let (records, remainder) = chunks.as_chunks::<2>();
    assert!(remainder.is_empty(), "incomplete group/value pair");
    let context = format!("ASCII code={code} bytes={} utf8={utf8}", bytes.len());
    let mut back = Vec::new();
    for record in records {
        assert_eq!(record[0].trim().parse::<i32>().unwrap(), code, "{context}");
        let text = record[1];
        assert!(text.len() <= 254, "{context}: {} hex characters exceed 127 payload bytes", text.len());
        assert!(text.bytes().all(|b| b.is_ascii_hexdigit()), "{context}: invalid hex");
        let (pairs, remainder) = text.as_bytes().as_chunks::<2>();
        assert!(remainder.is_empty(), "{context}: partial byte");
        back.extend(pairs.iter().map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap()));
    }
    assert_eq!(back, bytes, "{context}: complete payload and byte order");
}

fn check_binary(code: i32, bytes: &[u8], utf8: bool) {
    let wire = write_binary(&input(code, bytes), utf8);
    let mut chunks = wire.strip_prefix(SENTINEL).unwrap().strip_suffix(FOLLOWING_BINARY).unwrap();
    assert!(!chunks.is_empty(), "an empty payload must still have a tag");
    let context = format!("binary code={code} bytes={} utf8={utf8}", bytes.len());
    let mut back = Vec::new();
    while !chunks.is_empty() {
        let (&[lo, hi, length], rest) = chunks.split_first_chunk::<3>().unwrap();
        assert_eq!(i32::from(u16::from_le_bytes([lo, hi])), code, "{context}");
        assert!(length <= 127, "{context}: {length} bytes exceed the documented group limit");
        let (chunk, next) = rest.split_at(usize::from(length));
        back.extend_from_slice(chunk);
        chunks = next;
    }
    assert_eq!(back, bytes, "{context}: complete payload and byte order");
}

#[test]
fn conforming_payloads_have_independent_ascii_and_binary_wire_checks() {
    for code in (310..=319).chain([1004]) {
        for length in [0, 1, 2, 126, 127] {
            for utf8 in [false, true] {
                let bytes = payload(length);
                check_ascii(code, &bytes, utf8);
                check_binary(code, &bytes, utf8);
            }
        }
    }
}

#[test]
fn binary_reader_tolerates_the_full_length_byte_domain() {
    // This is deliberately permissive input, not an assertion that oversized chunks conform
    // or that the writer should produce them. The following record catches framing mistakes.
    for code in (310_u16..=319).chain([1004]) {
        for length in 0..=u8::MAX {
            let bytes = payload(usize::from(length));
            let mut wire = SENTINEL.to_vec();
            wire.extend_from_slice(&code.to_le_bytes());
            wire.push(length);
            wire.extend_from_slice(&bytes);
            wire.extend_from_slice(FOLLOWING_BINARY);
            assert_eq!(parse(&wire).unwrap(), input(i32::from(code), &bytes), "code={code} length={length}");
        }
    }
}

#[test]
#[ignore = "Confirmed ASCII DXF writer nonconformance: hex payloads above 127 bytes are emitted as one oversized chunk"]
fn ascii_writer_splits_large_payloads_into_conforming_chunks() {
    for code in (310..=319).chain([1004]) {
        for length in [128, 254, 255, 256, 1024] {
            for utf8 in [false, true] {
                check_ascii(code, &payload(length), utf8);
            }
        }
    }
}

#[test]
#[ignore = "Confirmed binary DXF writer nonconformance: 255-byte partitioning exceeds the documented 127-byte group limit"]
fn binary_writer_splits_large_payloads_into_conforming_chunks() {
    for code in (310..=319).chain([1004]) {
        for length in [128, 254, 255, 256, 1024] {
            for utf8 in [false, true] {
                check_binary(code, &payload(length), utf8);
            }
        }
    }
}
