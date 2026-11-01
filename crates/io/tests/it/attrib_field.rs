//! A placed attribute keeps the field of its attribute definition: the definition's extension
//! dictionary (ACAD_FIELD dictionary and FIELD objects) is written for the attribute too, with the
//! checksum and cached result of the text the attribute shows, and a DWG save keeps the cached
//! values of the FIELD objects.

use cadcraft_io::{SaveOptions, Version, read, write, write_with};

/// An `_text` FIELD object (AutoCAD's R2007 and later layout) that owns the variable field `child`.
fn text_field(handle: &str, owner: &str, child: &str) -> String {
    format!(
        "0\nFIELD\n5\n{handle}\n330\n{owner}\n100\nAcDbField\n1\n_text\n2\n%<\\_FldIdx 0>%\n90\n1\n360\n{child}\n97\n0\n91\n63\n92\n0\n94\n9\n95\n2\n96\n0\n300\n\n\
         93\n2\n6\nACFD_FIELDTEXT_ATTDEF\n93\n2\n90\n1\n91\n1\n94\n0\n300\n\n302\n\n304\nACVALUE_END\n\
         6\nACFD_FIELDTEXT_CHECKSUM\n93\n2\n90\n2\n140\n1914.0\n94\n0\n300\n\n302\n\n304\nACVALUE_END\n\
         7\nACFD_FIELD_VALUE\n93\n3\n90\n0\n94\n0\n300\n\n302\n\n304\nACVALUE_END\n301\n\n98\n0\n"
    )
}

/// The `AcVar Filename` FIELD object under it, with the template's file name cached.
fn variable_field(handle: &str, owner: &str) -> String {
    format!(
        "0\nFIELD\n5\n{handle}\n330\n{owner}\n100\nAcDbField\n1\nAcVar\n2\n\\AcVar Filename \\f \"%tc1%fn2\"\n90\n0\n97\n0\n91\n63\n92\n0\n94\n59\n95\n2\n96\n0\n300\n\n\
         93\n1\n6\nVariable\n93\n2\n90\n4\n1\nFilename\n94\n0\n300\n\n302\n\n304\nACVALUE_END\n\
         7\nACFD_FIELD_VALUE\n93\n4\n90\n4\n1\nC:\\Drawings\\TEMPLATE.dwg\n94\n0\n300\n%tc1%fn2\n302\nTEMPLATE\n304\nACVALUE_END\n301\nTEMPLATE\n98\n8\n"
    )
}

/// A drawing with block B (one ATTDEF NUMBER, with a field when `field`) and a reference to it
/// whose attribute shows `text`.
fn drawing(field: bool, text: &str) -> Vec<u8> {
    let xdict = if field { "102\n{ACAD_XDICTIONARY\n360\nD1\n102\n}\n" } else { "" };
    let objects = if field {
        format!(
            "0\nSECTION\n2\nOBJECTS\n\
             0\nDICTIONARY\n5\nD1\n330\nA1\n100\nAcDbDictionary\n3\nACAD_FIELD\n360\nD2\n\
             0\nDICTIONARY\n5\nD2\n330\nD1\n100\nAcDbDictionary\n3\nTEXT\n360\nF1\n{}{}0\nENDSEC\n",
            text_field("F1", "D2", "F2"),
            variable_field("F2", "F1")
        )
    } else {
        String::new()
    };
    // The class of the field objects, which AutoCAD declares and a DWG needs.
    let classes = if field {
        "0\nSECTION\n2\nCLASSES\n0\nCLASS\n1\nFIELD\n2\nAcDbField\n3\nObjectDBX Classes\n90\n1152\n91\n4\n280\n0\n281\n0\n0\nENDSEC\n"
    } else {
        ""
    };
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n{classes}\
         0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n5\nB1\n8\n0\n2\nB\n70\n0\n10\n0.0\n20\n0.0\n30\n0.0\n\
         0\nATTDEF\n5\nA1\n{xdict}8\n0\n10\n0.0\n20\n0.0\n40\n2.5\n1\nTEMPLATE\n7\nSTANDARD\n3\nNUMBER?\n2\nNUMBER\n70\n0\n0\nENDBLK\n5\nB2\n8\n0\n0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n0\nINSERT\n5\nC1\n8\n0\n2\nB\n10\n0.0\n20\n0.0\n30\n0.0\n66\n1\n\
         0\nATTRIB\n5\nC2\n8\n0\n10\n0.0\n20\n0.0\n40\n2.5\n1\n{text}\n7\nSTANDARD\n2\nNUMBER\n70\n0\n0\nSEQEND\n5\nC3\n8\n0\n0\nENDSEC\n{objects}0\nEOF\n"
    )
    .into_bytes()
}

type Groups = Vec<(i32, String)>;

/// The (code, value) groups of a DXF text.
fn groups(dxf: &[u8]) -> Groups {
    let text = String::from_utf8_lossy(dxf);
    let lines: Vec<&str> = text.lines().collect();
    lines.chunks(2).filter_map(|c| Some((c.first()?.trim().parse().ok()?, c.get(1)?.trim_end().to_string()))).collect()
}

/// The records of the given type: their groups after the `0` group.
fn records(g: &Groups, kind: &str) -> Vec<Groups> {
    g.iter()
        .enumerate()
        .filter(|(_, t)| t.0 == 0 && t.1 == kind)
        .map(|(i, _)| g.iter().skip(i + 1).take_while(|t| t.0 != 0).cloned().collect())
        .collect()
}

fn first(r: &Groups, code: i32) -> Option<&str> {
    r.iter().find(|t| t.0 == code).map(|t| t.1.as_str())
}

/// The checksum a field on `text` carries: each character's code times its place (from 1).
fn checksum(text: &str) -> f64 {
    text.chars().zip(1u32..).map(|(c, i)| f64::from(u32::from(c) * i)).sum()
}

/// Asserts the placed NUMBER attribute in `dxf` has its own field chain for `text`.
fn assert_field_on_attribute(dxf: &[u8], text: &str) {
    let g = groups(dxf);
    let attrib = records(&g, "ATTRIB").into_iter().find(|r| first(r, 2) == Some("NUMBER")).expect("the placed attribute");
    assert_eq!(first(&attrib, 1), Some(text));
    let xdict = first(&attrib, 360).expect("the attribute has an extension dictionary");
    let dictionaries = records(&g, "DICTIONARY");
    let of = |h: &str| dictionaries.iter().find(|r| first(r, 5) == Some(h)).cloned();
    let xd = of(xdict).expect("the extension dictionary is written");
    assert_eq!(first(&xd, 330), first(&attrib, 5), "the dictionary is owned by the attribute");
    let field_dict = of(entry(&xd).expect("ACAD_FIELD entry")).expect("the field dictionary is written");
    let fields = records(&g, "FIELD");
    let text_field = fields.iter().find(|r| first(r, 5) == entry(&field_dict)).expect("the _text field");
    assert_eq!(first(text_field, 330), first(&field_dict, 5), "the field is owned by its dictionary");
    assert!(text_field.iter().any(|t| t.0 == 102 && t.1 == "{ACAD_REACTORS"), "the field reacts to its dictionary");
    let sum = text_field.iter().skip_while(|t| t.1 != "ACFD_FIELDTEXT_CHECKSUM").find(|t| t.0 == 140).expect("a checksum");
    assert_eq!(sum.1.parse::<f64>().unwrap(), checksum(text), "checksum of {text:?}");
    let var = fields.iter().find(|r| first(r, 5) == first(text_field, 360)).expect("the variable field");
    assert_eq!(first(var, 330), first(text_field, 5));
    assert_eq!(first(var, 301), Some(text), "cached result");
    let formatted = var.iter().skip_while(|t| t.1 != "ACFD_FIELD_VALUE").find(|t| t.0 == 302).map(|t| t.1.as_str());
    assert_eq!(formatted, Some(text), "cached formatted result");
    assert_eq!(first(var, 302), Some(""), "the result before the value stays empty, as AutoCAD writes it");
    assert_eq!(first(var, 98), Some(text.chars().count().to_string().as_str()), "cached length");
}

#[test]
fn placed_attribute_gets_a_copy_of_the_definitions_field() {
    for text in ["246-302-A", "X-1", "", "a longer drawing number than usual 0123456789"] {
        let d = read(&drawing(true, text), "t.dxf").unwrap();
        assert_field_on_attribute(&write(&d, "t.dxf").unwrap(), text);
    }
}

#[test]
fn the_definition_keeps_its_own_field() {
    let d = read(&drawing(true, "A-1"), "t.dxf").unwrap();
    let g = groups(&write(&d, "t.dxf").unwrap());
    let def = records(&g, "ATTDEF").into_iter().next().expect("the definition");
    assert!(first(&def, 360).is_some(), "the definition's extension dictionary");
    assert_eq!(records(&g, "FIELD").len(), 4, "two fields for the definition, two for the attribute");
}

#[test]
fn a_block_without_a_field_is_unchanged() {
    let d = read(&drawing(false, "A-1"), "t.dxf").unwrap();
    let g = groups(&write(&d, "t.dxf").unwrap());
    assert!(records(&g, "FIELD").is_empty());
    assert!(records(&g, "FIELDLIST").is_empty());
    let attrib = records(&g, "ATTRIB").into_iter().next().expect("the attribute");
    assert_eq!(first(&attrib, 360), None);
}

/// The FIELD objects keep their cached values through a DWG save and reopen, and the attribute
/// still has its field after saving the reopened drawing again.
#[test]
fn the_field_survives_a_dwg_save_and_reopen() {
    let d = read(&drawing(true, "246-302-A"), "t.dxf").unwrap();
    for version in [Version::R2007, Version::R2010, Version::R2013, Version::R2018] {
        let save = |d: &_| write_with(d, "t.dwg", &SaveOptions { version: Some(version), binary: false }).unwrap();
        let back = read(&save(&d), "t.dwg").unwrap();
        assert_field_on_attribute(&write(&back, "t.dxf").unwrap(), "246-302-A");
        let again = read(&save(&back), "t.dwg").unwrap();
        assert_field_on_attribute(&write(&again, "t.dxf").unwrap(), "246-302-A");
        let g = groups(&write(&again, "t.dxf").unwrap());
        let cached =
            records(&g, "FIELD").iter().any(|r| first(r, 1) == Some("AcVar") && r.iter().any(|t| t.0 == 1 && t.1.ends_with("246-302-A.dwg")));
        assert!(cached, "{version:?}: the variable field keeps its cached path");
    }
}

/// The object a dictionary's first entry names (hard owner 360 or soft owner 350).
fn entry(r: &Groups) -> Option<&str> {
    r.iter().find(|t| t.0 == 350 || t.0 == 360).map(|t| t.1.as_str())
}
