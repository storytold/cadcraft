//! A file whose `$HANDSEED` is the top of the handle range must not hang the importer: the seed
//! saturates at `u64::MAX`, so allocating past a taken handle has to stop and fall back to a free
//! one instead of retrying the same handle forever.

use std::collections::HashSet;

use cadcraft_doc::Handle;

/// One line holding the top handle, and one line with no handle at all, so the importer has to
/// allocate one after the seed is exhausted.
fn dxf() -> String {
    "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n9\n$HANDSEED\n5\nFFFFFFFFFFFFFFFF\n0\nENDSEC\n\
     0\nSECTION\n2\nENTITIES\n\
     0\nLINE\n5\nFFFFFFFFFFFFFFFF\n8\n0\n10\n0.0\n20\n0.0\n30\n0.0\n11\n1.0\n21\n1.0\n31\n0.0\n\
     0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n30\n0.0\n11\n2.0\n21\n2.0\n31\n0.0\n\
     0\nENDSEC\n0\nEOF\n"
        .to_string()
}

#[test]
fn an_exhausted_handle_seed_still_finds_a_free_handle() {
    let d = cadcraft_io::read_dxf(dxf().as_bytes()).unwrap();
    assert_eq!(d.model.iter().count(), 2);
    let handles: Vec<Handle> = d.model.iter().map(|e| e.handle).collect();
    let unique: HashSet<Handle> = handles.iter().copied().collect();
    assert_eq!(unique.len(), handles.len(), "duplicate handles: {handles:?}");
    // The file's own top handle stays with its entity.
    assert!(handles.contains(&Handle(u64::MAX)));
}

/// A `*MODEL_SPACE` block entity with no handle, a handleless `ENTITIES` record, and a later
/// `ENTITIES` record declaring handle 1: the two generated handles must differ (both lists land
/// in the model space) and must not take the handle the later record declares.
fn dxf_with_model_space_block() -> String {
    "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n9\n$HANDSEED\n5\nFFFFFFFFFFFFFFFF\n0\nENDSEC\n\
     0\nSECTION\n2\nBLOCKS\n\
     0\nBLOCK\n5\n20\n8\n0\n2\n*MODEL_SPACE\n70\n0\n10\n0.0\n20\n0.0\n30\n0.0\n\
     0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n30\n0.0\n11\n1.0\n21\n1.0\n31\n0.0\n\
     0\nENDBLK\n5\n21\n8\n0\n\
     0\nENDSEC\n\
     0\nSECTION\n2\nENTITIES\n\
     0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n30\n0.0\n11\n2.0\n21\n2.0\n31\n0.0\n\
     0\nLINE\n5\n1\n8\n0\n10\n0.0\n20\n0.0\n30\n0.0\n11\n3.0\n21\n3.0\n31\n0.0\n\
     0\nENDSEC\n0\nEOF\n"
        .to_string()
}

#[test]
fn generated_handles_are_unique_across_lists_and_avoid_later_source_handles() {
    let d = cadcraft_io::read_dxf(dxf_with_model_space_block().as_bytes()).unwrap();
    let handles: Vec<Handle> = d.model.iter().map(|e| e.handle).collect();
    assert_eq!(handles.len(), 3, "{handles:?}");
    let unique: HashSet<Handle> = handles.iter().copied().collect();
    assert_eq!(unique.len(), handles.len(), "duplicate handles: {handles:?}");
    // The record that declares handle 1 keeps it; the generated ones went elsewhere.
    assert!(handles.contains(&Handle(1)), "{handles:?}");
}
