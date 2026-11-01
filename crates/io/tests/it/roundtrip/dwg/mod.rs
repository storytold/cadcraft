//! Native DWG Drawing → bytes → Drawing preservation, using the same cases and assertions as
//! DXF. Every representable case runs at all six save versions and through two saves. R2000's
//! lack of true color/transparency is a format limit, tested separately with explicit expected
//! downgrades; it is not a normalization of modern output. TEXT's single elevation is also
//! checked separately for non-coplanar inputs. Extended cases also specify handle-based name
//! resolution, anonymous dimension-block names and legacy block/dimension-style limits
//! explicitly. Other differences fail.
//! Native-object conformance checks bypass one bridge direction, not the underlying codec.
//!
//! Run `cargo test -p cadcraft-io --test it roundtrip::dwg`; add `-- --ignored` to reproduce
//! pending defects. No DWG bytes are imported from another application or kept as fixtures.

mod conformance;
mod dimensions;
mod entities;
mod extended;

use cadcraft_color::Color;
use cadcraft_doc::{Drawing, EntityKind, Transparency};
use cadcraft_io::{SaveOptions, Version, read, write_with};

use super::{cases, compare};

fn check(cases: Vec<cases::Case>) {
    for case in cases {
        assert!(
            !case.drawing.model.iter().any(|e| matches!(&e.kind, EntityKind::Text(t) if t.align_pt.is_some_and(|p| p.z != t.insert.z))),
            "{}: non-coplanar TEXT belongs in the explicit single-elevation test",
            case.name
        );
        let mut tested = 0;
        for version in Version::ALL {
            if version == Version::R2000 && needs_color_or_transparency(&case.drawing) {
                continue;
            }
            check_version(&case.name, &case.drawing, &case.drawing, version);
            tested += 1;
        }
        assert!(tested > 0, "{}: the version domain must not exclude a whole case", case.name);
    }
}

fn needs_color_or_transparency(d: &Drawing) -> bool {
    d.model.iter().any(|e| matches!(e.common.color, Color::True(_)) || e.common.transparency != Transparency::ByLayer)
        || d.layers.iter().any(|l| matches!(l.color, Color::True(_)) || l.transparency != 0)
}

fn check_version(case: &str, original: &Drawing, expected: &Drawing, version: Version) {
    check_version_with(case, original, expected, version, |_, _, _| {});
}

fn check_version_with(case: &str, original: &Drawing, expected: &Drawing, version: Version, extra: impl Fn(&Drawing, &Drawing, &str)) {
    check_version_using(case, original, expected, version, |a, b, context| {
        compare::model(a, b, context);
        extra(a, b, context);
    });
}

fn check_version_using(case: &str, original: &Drawing, expected: &Drawing, version: Version, verify: impl Fn(&Drawing, &Drawing, &str)) {
    let options = SaveOptions { version: Some(version), binary: false };
    let mut current = original.clone();
    for pass in 1..=2 {
        let context = format!("{case} · DWG {version} · pass={pass}");
        let bytes = write_with(&current, "memory.dwg", &options).unwrap_or_else(|e| panic!("{context}: write: {e}"));
        assert_eq!(cadcraft_dwg::version(&bytes).as_deref(), Some(version.code()), "{context}: wire version");
        let back = read(&bytes, "memory.dwg").unwrap_or_else(|e| panic!("{context}: read: {e}"));
        assert!(back.read_notes.is_empty(), "{context}: read notes: {:?}", back.read_notes);
        assert!(back.skipped_records.is_empty(), "{context}: skipped records: {:?}", back.skipped_records);
        verify(expected, &back, &context);
        if pass == 2 {
            verify(&current, &back, &format!("{context} · resave stability"));
        }
        current = back;
    }
}
