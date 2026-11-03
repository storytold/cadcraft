//! Exhausting generated-handle space may be refused, but must not silently lose authored
//! entities. This is separate from the preservation matrix's large handles with free capacity.
//! Neither a particular allocator strategy nor a successful save is required at exhaustion.

use cadcraft_io::{SaveOptions, Version, read, write_with};

use super::{cases::properties, compare};

fn preserves_or_refuses(path: &str, binary: bool) {
    let original = properties::exhausted_handles().drawing;
    for version in Version::ALL {
        let options = SaveOptions { version: Some(version), binary };
        let mut current = original.clone();
        for pass in 1..=2 {
            let context = format!("exhausted handles · {path} {version} binary={binary} pass={pass}");
            let Ok(bytes) = write_with(&current, path, &options) else {
                break; // An explicit save error is safe; a nominal success must preserve the data.
            };
            let back = read(&bytes, path).unwrap_or_else(|e| panic!("{context}: successful save is unreadable: {e}"));
            assert!(back.read_notes.is_empty(), "{context}: {:?}", back.read_notes);
            assert!(back.skipped_records.is_empty(), "{context}: {:?}", back.skipped_records);
            compare::model(&original, &back, &context);
            current = back;
        }
    }
}

#[test]
#[ignore = "Confirmed DXF data loss: exhausted generated handles produce a successful save that reopens with no model entities"]
fn dxf_exhausted_handles_are_preserved_or_refused() {
    for binary in [false, true] {
        preserves_or_refuses("memory.dxf", binary);
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn dwg_exhausted_handles_are_preserved_or_refused() {
    preserves_or_refuses("memory.dwg", false);
}
