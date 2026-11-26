//! An LTYPE record keeps at most `MAX_LINETYPE_ELEMENTS` dash elements on DXF read, the cap a
//! `.lin` file has (#1923, F-05): the rest of its pattern is dropped, opening the file says so,
//! and saving writes the elements kept.

use cadcraft_doc::library::files::MAX_LINETYPE_ELEMENTS as MAX;
use cadcraft_io::{read_dxf, write_dxf};

/// Element `i`: a dash `(i + 1) / 100` long for even `i`, a gap that long for odd `i`.
fn length(i: usize) -> f64 {
    let l = (i + 1) as f64 / 100.0;
    if i.is_multiple_of(2) { l } else { -l }
}

/// An LTYPE record `name` of `elements` elements, the last one carrying the text `last_text`.
fn ltype(name: &str, elements: usize, last_text: &str) -> String {
    let mut s = format!("0\nLTYPE\n2\n{name}\n70\n0\n3\n{name} dashes\n72\n65\n73\n{elements}\n40\n1\n");
    for i in 0..elements {
        s.push_str(&format!("49\n{}\n", length(i)));
        if i + 1 == elements {
            s.push_str(&format!("74\n2\n75\n0\n46\n0.1\n50\n0\n44\n0\n45\n0\n9\n{last_text}\n"));
        } else {
            s.push_str("74\n0\n");
        }
    }
    s
}

/// A DXF of these LTYPE records and one LINE drawn with the first.
fn dxf(ltypes: &[String], line_type: &str) -> String {
    let mut s = String::from("0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLTYPE\n70\n1\n");
    for l in ltypes {
        s.push_str(l);
    }
    s.push_str(&format!("0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n6\n{line_type}\n10\n0\n20\n0\n30\n0\n11\n10\n21\n0\n31\n0\n"));
    s.push_str("0\nENDSEC\n0\nEOF\n");
    s
}

/// The value of group `code` in the LTYPE record named `name` of a written DXF.
fn ltype_group<'a>(text: &'a str, name: &str, code: &str) -> Option<&'a str> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let pairs: Vec<(&str, &str)> = lines.chunks(2).filter_map(|p| Some((*p.first()?, *p.get(1)?))).collect();
    // A record runs from its `0` group to the next one.
    let starts: Vec<usize> = (0..pairs.len()).filter(|&i| pairs[i].0 == "0").chain([pairs.len()]).collect();
    starts
        .windows(2)
        .map(|w| &pairs[w[0]..w[1]])
        .find(|r| r[0] == ("0", "LTYPE") && r.contains(&("2", name)))?
        .iter()
        .find(|p| p.0 == code)
        .map(|p| p.1)
}

#[test]
fn an_ltype_past_the_element_cap_keeps_its_first_elements_and_says_so() {
    let d = read_dxf(dxf(&[ltype("LONG", MAX + 1, "DROPPED")], "LONG").as_bytes()).expect("read");
    let lt = d.linetype("LONG").expect("the linetype is kept");
    let lengths: Vec<f64> = lt.pattern.iter().map(|e| e.length).collect();
    assert_eq!(lengths, (0..MAX).map(length).collect::<Vec<_>>());
    // The text of the dropped element is not moved onto the last element kept.
    assert!(lt.pattern.iter().all(|e| e.text.is_none() && e.shape.is_none()), "{:?}", lt.pattern.last());
    assert!(matches!(d.read_notes.as_slice(), [note] if note.contains("1 linetype") && note.contains(&MAX.to_string())), "{:?}", d.read_notes);
    assert!(d.model.iter().any(|e| e.common.linetype == "LONG"), "the line keeps its linetype");
    // Saved, the record holds and counts the elements kept; reopened, it reads whole, without a note.
    let text = write_dxf(&d);
    assert_eq!(ltype_group(&text, "LONG", "73"), Some(MAX.to_string().as_str()));
    let back = read_dxf(text.as_bytes()).expect("reopen");
    assert_eq!(back.linetype("LONG"), Some(lt));
    assert!(back.read_notes.is_empty(), "{:?}", back.read_notes);
}

#[test]
fn an_ltype_at_the_element_cap_reads_whole() {
    let d = read_dxf(dxf(&[ltype("FULL", MAX, "KEPT")], "FULL").as_bytes()).expect("read");
    let lt = d.linetype("FULL").expect("FULL");
    let lengths: Vec<f64> = lt.pattern.iter().map(|e| e.length).collect();
    assert_eq!(lengths, (0..MAX).map(length).collect::<Vec<_>>());
    assert_eq!(lt.pattern.last().and_then(|e| e.text.as_deref()), Some("KEPT"));
    assert!(d.read_notes.is_empty(), "{:?}", d.read_notes);
}

/// One note counts every linetype cut; a repeated name is not read again (the first definition
/// wins), so it is not counted.
#[test]
fn one_note_counts_the_linetypes_cut() {
    let records = [ltype("A", MAX + 1, "x"), ltype("B", MAX + 40, "y"), ltype("SHORT", 3, "z"), ltype("a", MAX + 1, "again")];
    let d = read_dxf(dxf(&records, "A").as_bytes()).expect("read");
    assert!(matches!(d.read_notes.as_slice(), [note] if note.contains("2 linetypes")), "{:?}", d.read_notes);
    for (name, n) in [("A", MAX), ("B", MAX), ("SHORT", 3)] {
        assert_eq!(d.linetype(name).map(|l| l.pattern.len()), Some(n), "{name}");
    }
}
