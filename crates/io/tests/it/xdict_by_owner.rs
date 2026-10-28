//! An extension dictionary its owner's record doesn't name is still that record's: a dictionary that
//! names a table style as its owner, and that no dictionary lists as an entry, is the table style's
//! extension dictionary. The DWG bridge writes table styles that way (AutoCAD's table style round
//! trip data, a CELLSTYLEMAP, under it), so without this the data was lost on every save.

use cadcraft_doc::{Drawing, TableStyle};

/// (code, value) pairs of DXF text.
fn pairs(text: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    lines.chunks(2).filter_map(|p| Some((p.first()?.to_string(), p.get(1)?.to_string()))).collect()
}

fn text(pairs: &[(String, String)]) -> String {
    pairs.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
}

/// The records of type `kind`, each as its pairs (from its `0` to the next).
fn records<'a>(p: &'a [(String, String)], kind: &str) -> Vec<&'a [(String, String)]> {
    let starts: Vec<usize> = p.iter().enumerate().filter(|(_, (c, _))| c == "0").map(|(i, _)| i).collect();
    starts.iter().enumerate().filter(|(_, s)| p[**s].1 == kind).map(|(n, s)| &p[*s..starts.get(n + 1).copied().unwrap_or(p.len())]).collect()
}

fn value<'a>(rec: &'a [(String, String)], code: &str) -> Option<&'a str> {
    rec.iter().find(|(c, _)| c == code).map(|(_, v)| v.as_str())
}

/// A DXF with the table style "Costs" and, as the bridge writes it, its extension dictionary
/// (FF00, entry `ROUNDTRIP` → XRECORD FF01) that names the style as owner while the style's record
/// has no `{ACAD_XDICTIONARY` group.
fn dxf() -> (String, String) {
    let mut d = Drawing::new_imperial();
    d.table_styles.push(TableStyle { name: "Costs".into(), ..TableStyle::default() });
    let mut p = pairs(&String::from_utf8(cadcraft_io::write(&d, "t.dxf").unwrap()).unwrap());
    let style = records(&p, "TABLESTYLE")
        .into_iter()
        .find(|r| value(r, "3") == Some("Costs") || value(r, "2") == Some("Costs"))
        .map(|r| value(r, "5").unwrap().to_string());
    let style = style.unwrap_or_else(|| {
        // The style's handle by its entry in the ACAD_TABLESTYLE dictionary.
        let i = p.iter().position(|(c, v)| c == "3" && v == "Costs").unwrap();
        p[i + 1].1.clone()
    });
    assert!(records(&p, "TABLESTYLE").iter().all(|r| !r.iter().any(|(c, v)| c == "102" && v == "{ACAD_XDICTIONARY")));
    let objects = p.iter().position(|(c, v)| c == "2" && v == "OBJECTS").unwrap();
    let end = objects + p[objects..].iter().position(|(c, v)| c == "0" && v == "ENDSEC").unwrap();
    let add = [
        ("0", "DICTIONARY"),
        ("5", "FF00"),
        ("330", style.as_str()),
        ("100", "AcDbDictionary"),
        ("280", "1"),
        ("281", "1"),
        ("3", "ROUNDTRIP"),
        ("360", "FF01"),
        ("0", "XRECORD"),
        ("5", "FF01"),
        ("102", "{ACAD_REACTORS"),
        ("330", "FF00"),
        ("102", "}"),
        ("330", "FF00"),
        ("100", "AcDbXrecord"),
        ("280", "1"),
        ("1", "round trip data"),
    ];
    p.splice(end..end, add.iter().map(|(c, v)| (c.to_string(), v.to_string())));
    if let Some(seed) = p.iter().position(|(c, v)| c == "9" && v == "$HANDSEED") {
        p[seed + 1].1 = "FFFF".into();
    }
    (text(&p), style)
}

#[test]
fn a_dictionary_owned_by_a_table_style_is_its_extension_dictionary() {
    let (input, style) = dxf();
    let d = cadcraft_io::read(input.as_bytes(), "t.dxf").unwrap();
    let out = pairs(&String::from_utf8(cadcraft_io::write(&d, "t.dxf").unwrap()).unwrap());
    // The style now names its extension dictionary, which is written with its entry.
    let st = records(&out, "TABLESTYLE").into_iter().find(|r| value(r, "5") == Some(style.as_str())).expect("the table style");
    let at = st.iter().position(|(c, v)| c == "102" && v == "{ACAD_XDICTIONARY").expect("the style names its extension dictionary");
    assert_eq!(st[at + 1], ("360".into(), "FF00".into()));
    let dict = records(&out, "DICTIONARY").into_iter().find(|r| value(r, "5") == Some("FF00")).expect("the dictionary");
    assert_eq!(value(dict, "330"), Some(style.as_str()));
    assert_eq!(value(dict, "3"), Some("ROUNDTRIP"));
    let x = records(&out, "XRECORD").into_iter().find(|r| value(r, "5") == Some("FF01")).expect("the xrecord");
    assert_eq!(value(x, "1"), Some("round trip data"));
    // And it stays so: read and written again, the same.
    let again = pairs(&String::from_utf8(cadcraft_io::write(&cadcraft_io::read(text(&out).as_bytes(), "t.dxf").unwrap(), "t.dxf").unwrap()).unwrap());
    assert_eq!(records(&again, "XRECORD").into_iter().filter(|r| value(r, "5") == Some("FF01")).count(), 1);
}
