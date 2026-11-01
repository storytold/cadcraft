//! DXF preservation through public Drawing → bytes → Drawing APIs, without disk or UI.
//! Covers modeled entity families, stored block graphs, references and selected drawing
//! collections, not whole-drawing/session equivalence. Each case runs in ASCII and binary,
//! twice, at every representable save version; expectations never come from writer output.
//! WIPEOUT's explicit closing marker is an authored format expectation, not a new corner.
//!
//! Run: `cargo test -p cadcraft-io --test it roundtrip::dxf`.
//! Add `-- --ignored` to reproduce the pending preservation cases. Those tests
//! assert desired preservation, not today's wrong result; remove `ignore` when fixing them.
//! Independent fixtures cover the core entity groups; deep compound-entity interoperability
//! still needs separate format checks beyond the self-round-trip matrix.

mod attributes;
mod common;
mod conformance;
mod entities;
mod extended;
mod geometry_groups;
mod table_flags;

use super::compare;
use cadcraft_doc::Drawing;
use cadcraft_io::{SaveOptions, Version, read, save_version, write_with};

fn check(case: &str, original: &Drawing) {
    for version in Version::ALL {
        check_version(case, original, original, version);
    }
}

fn check_version(case: &str, original: &Drawing, expected: &Drawing, version: Version) {
    check_version_with(case, original, expected, version, |_, _, _| {});
}

fn check_version_with(case: &str, original: &Drawing, expected: &Drawing, version: Version, extra: impl Fn(&Drawing, &Drawing, &str)) {
    for binary in [false, true] {
        let options = SaveOptions { version: Some(version), binary };
        let mut current = original.clone();
        for pass in 1..=2 {
            let context = format!("{case} · {version} · binary={binary} · pass={pass}");
            let bytes = write_with(&current, "memory.dxf", &options).unwrap_or_else(|e| panic!("{context}: write: {e}"));
            assert_eq!(cadcraft_dxf::is_binary(&bytes), binary, "{context}: encoding");
            let back = read(&bytes, "memory.dxf").unwrap_or_else(|e| panic!("{context}: read: {e}"));
            assert_eq!(save_version(&back), version, "{context}: version");
            assert!(back.read_notes.is_empty(), "{context}: read notes: {:?}", back.read_notes);
            assert!(back.skipped_records.is_empty(), "{context}: skipped records: {:?}", back.skipped_records);
            compare::model(expected, &back, &context);
            extra(expected, &back, &context);
            if pass == 2 {
                compare::model(&current, &back, &format!("{context} · resave stability"));
                extra(&current, &back, &format!("{context} · resave stability"));
            }
            current = back;
        }
    }
}
