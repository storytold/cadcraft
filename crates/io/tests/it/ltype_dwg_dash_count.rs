//! A DWG counts a linetype's dash elements in one byte (acadrust 0.6.6 writes and reads the
//! count with `write_byte` / `read_byte`), so a linetype saved as DWG keeps its first 255
//! elements instead of a count that wrapped past 255, and one note says how many linetypes were
//! cut (#1923, F-05; the DXF cap of #3482 lets 256 through).

use std::io::Cursor;

use cadcraft_doc::{DashElement, Drawing, Layer, Linetype};
use cadcraft_io::{DWG_MAX_LINETYPE_ELEMENTS as MAX, SaveOptions, Version, dwg_notes};

/// Element `i`: a dash `(i + 1) / 100` long for even `i`, a gap that long for odd `i`.
fn length(i: usize) -> f64 {
    let l = (i + 1) as f64 / 100.0;
    if i.is_multiple_of(2) { l } else { -l }
}

/// A linetype `name` of `elements` elements.
fn linetype(name: &str, elements: usize) -> Linetype {
    Linetype { name: name.into(), description: format!("{name} dashes"), pattern: (0..elements).map(|i| DashElement::dash(length(i))).collect() }
}

/// A drawing with linetypes of these names and element counts.
fn drawing(linetypes: &[(&str, usize)]) -> Drawing {
    let mut d = Drawing::new_metric();
    d.linetypes.extend(linetypes.iter().map(|(name, n)| linetype(name, *n)));
    d
}

/// The element lengths of linetype `name` in `d`.
fn lengths(d: &Drawing, name: &str) -> Option<Vec<f64>> {
    d.linetype(name).map(|l| l.pattern.iter().map(|e| e.length).collect())
}

#[test]
fn a_dwg_keeps_the_first_255_dash_elements_of_a_longer_linetype() {
    cadcraft_io::dwg_worker::allow_in_process();
    let d = drawing(&[("FULL", MAX), ("OVER", MAX + 1), ("LONG", 300)]);
    let first = (0..MAX).map(length).collect::<Vec<_>>();
    for version in [Version::R2000, Version::R2004, Version::R2007, Version::R2010, Version::R2013, Version::R2018] {
        let options = SaveOptions { version: Some(version), binary: false };
        let dwg = cadcraft_io::write_with(&d, "ltypes.dwg", &options).expect("save as DWG");
        let back = cadcraft_io::read(&dwg, "ltypes.dwg").expect("reopen");
        for name in ["FULL", "OVER", "LONG"] {
            assert_eq!(lengths(&back, name).as_ref(), Some(&first), "{name} {version:?}");
        }
        // The record's total length is that of the elements it keeps.
        let doc = acadrust::DwgReader::from_stream(Cursor::new(dwg)).read().expect("acadrust reads the DWG");
        let long = doc.line_types.iter().find(|l| l.name == "LONG").expect("LONG");
        let kept: f64 = first.iter().map(|l| l.abs()).sum();
        assert!((long.pattern_length - kept).abs() < 1e-9, "{version:?}: {} for {kept}", long.pattern_length);
    }
}

/// One note counts the linetypes a DWG cuts; linetypes that fit get none.
#[test]
fn one_note_counts_the_linetypes_a_dwg_cuts() {
    assert_eq!(dwg_notes(&drawing(&[("FULL", MAX), ("SHORT", 4)])), Vec::<String>::new());
    let one = dwg_notes(&drawing(&[("FULL", MAX), ("OVER", MAX + 1)]));
    assert!(matches!(one.as_slice(), [note] if note.contains("1 linetype ") && note.contains(&MAX.to_string())), "{one:?}");
    let two = dwg_notes(&drawing(&[("OVER", MAX + 1), ("FULL", MAX), ("LONG", 300)]));
    assert!(matches!(two.as_slice(), [note] if note.contains("2 linetypes")), "{two:?}");
}

/// An xref's own linetype is saved only when a layer uses it, so only then is it counted.
#[test]
fn an_xref_linetype_the_file_leaves_out_is_not_counted() {
    let mut d = drawing(&[("SITE|LONG", 300)]);
    assert_eq!(dwg_notes(&d), Vec::<String>::new());
    d.layers.push(Layer { linetype: "SITE|LONG".into(), ..Layer::new("SITE|WALLS") });
    assert!(matches!(dwg_notes(&d).as_slice(), [note] if note.contains("1 linetype ")), "{:?}", dwg_notes(&d));
}
