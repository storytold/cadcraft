//! A centre-justified ATTDEF keeps its insertion point in a DWG (#2464): AutoCAD puts it where the
//! baseline of the displayed tag starts, which for an attribute with an empty default value is
//! further from the alignment point than half a height.

use cadcraft_doc::EntityKind;
use cadcraft_io::{SaveOptions, Version, read, write_with};

/// The `insert` x of the only ATTDEF in `d`.
fn attdef_x(d: &cadcraft_doc::Drawing) -> f64 {
    d.model.iter().find_map(|e| if let EntityKind::AttDef(a) = &e.kind { Some(a.text.insert.x) } else { None }).expect("an ATTDEF")
}

#[test]
fn centred_attdef_with_an_empty_value_keeps_its_insertion_point() {
    // The tag "LONGTAG" at height 2.5, centred on x = 100: its baseline starts half a width left.
    // The stored point is 1 off that start (within the tolerance), so a write that recomputed
    // it from the tag would not pass for keeping it.
    let width = cadcraft_fonts::line_width("LONGTAG", 2.5, 1.0);
    let start = 100.0 - width / 2.0 - 1.0;
    assert!(100.0 - start > 3.0, "the point is well over a height from the alignment point");
    assert_kept(start);
}

/// A stored point that a tag-width baseline would move is kept too (review of #2466): the check
/// does not rely on the font's width of the tag, so this one is only 0.5 from the alignment point.
#[test]
fn centred_attdef_point_near_the_alignment_point_is_kept() {
    assert_kept(99.5);
}

/// A centred ATTDEF with an empty default value and group 10 at `start`, group 11 at x = 100 is
/// read from DXF and kept by a write as DXF and as DWG (both versions). The displayed text is the
/// tag whatever the prompt: with a prompt, an empty one (group 3 empty) and none (no group 3).
fn assert_kept(start: f64) {
    for prompt in ["3\nprompt\n", "3\n\n", ""] {
        let dxf = format!(
            "0\nSECTION\n2\nENTITIES\n0\nATTDEF\n8\n0\n10\n{start}\n20\n5.0\n40\n2.5\n1\n\n7\nSTANDARD\n72\n1\n11\n100.0\n21\n5.0\n{prompt}2\nLONGTAG\n70\n8\n0\nENDSEC\n0\nEOF\n"
        );
        let d = read(dxf.as_bytes(), "t.dxf").unwrap();
        assert!((attdef_x(&d) - start).abs() < 1e-6, "read from DXF");
        let as_dxf = cadcraft_io::write(&d, "t.dxf").unwrap();
        assert!((attdef_x(&read(&as_dxf, "t.dxf").unwrap()) - start).abs() < 1e-6, "DXF, prompt {prompt:?}: insertion point changed");
        for version in [Version::R2000, Version::R2018] {
            let dwg = write_with(&d, "t.dwg", &SaveOptions { version: Some(version), binary: false }).unwrap();
            let back = read(&dwg, "t.dwg").unwrap();
            assert!((attdef_x(&back) - start).abs() < 1e-6, "{version:?}, prompt {prompt:?}: insertion point {} instead of {start}", attdef_x(&back));
        }
    }
}
