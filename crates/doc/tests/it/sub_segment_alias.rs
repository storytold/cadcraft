//! `Sub::parse` accepts every spelling its documentation lists (#710).

use cadcraft_doc::Sub;

#[test]
fn short_segment_name_matches_the_documented_forms() {
    for spelling in ["seg2", "segment2", "s2"] {
        assert_eq!(Sub::parse(spelling), Some(Sub::Segment(2)), "spelling: {spelling}");
    }
    for spelling in ["s", "start"] {
        assert_eq!(Sub::parse(spelling), Some(Sub::Start), "spelling: {spelling}");
    }
    assert_eq!(Sub::parse("unknown"), None);
}
