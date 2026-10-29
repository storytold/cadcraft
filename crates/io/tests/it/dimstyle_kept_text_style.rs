//! A dimension style whose text style reference (340) names a record that is no text style (AutoCAD
//! writes some files with it naming a UCS) keeps that reference while the style's text style is
//! unchanged, and a text style the user gives the style wins over it. The fixture is synthetic.

use cadcraft_dxf::{parse, records, sections};
use cadcraft_io::{read_dxf, write_dxf};

/// DXF text from lines of `code value`.
fn dxf(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (code, value) = line.split_once(' ').unwrap_or((line, ""));
        out.push_str(&format!("{code}\n{value}\n"));
    }
    out
}

/// A drawing with text styles STANDARD (handle 15) and NOTES (16), a UCS MYUCS (17) and a dimension
/// style whose text style reference names the UCS.
const SRC: &str = "
0 SECTION
2 HEADER
9 $ACADVER
1 AC1032
9 $HANDSEED
5 300
0 ENDSEC
0 SECTION
2 TABLES
0 TABLE
2 STYLE
5 3
330 0
100 AcDbSymbolTable
70 2
0 STYLE
5 15
330 3
100 AcDbSymbolTableRecord
100 AcDbTextStyleTableRecord
2 Standard
70 0
40 0.0
41 1.0
3 txt
0 STYLE
5 16
330 3
100 AcDbSymbolTableRecord
100 AcDbTextStyleTableRecord
2 Notes
70 0
40 0.0
41 1.0
3 txt
0 ENDTAB
0 TABLE
2 UCS
5 7
330 0
100 AcDbSymbolTable
70 1
0 UCS
5 17
330 7
100 AcDbSymbolTableRecord
100 AcDbUCSTableRecord
2 MYUCS
70 0
10 0.0
20 0.0
30 0.0
11 1.0
21 0.0
31 0.0
12 0.0
22 1.0
32 0.0
79 0
146 0.0
0 ENDTAB
0 TABLE
2 DIMSTYLE
5 A
330 0
100 AcDbSymbolTable
70 1
100 AcDbDimStyleTable
71 0
0 DIMSTYLE
105 18
330 A
100 AcDbSymbolTableRecord
100 AcDbDimStyleTableRecord
2 Standard
70 0
340 17
0 ENDTAB
0 ENDSEC
0 SECTION
2 ENTITIES
0 ENDSEC
0 EOF
";

/// The text style references (340) of the written dimension style Standard.
fn text_style_refs(text: &str) -> Vec<String> {
    let tags = parse(text.as_bytes()).unwrap();
    let mut out = Vec::new();
    for s in sections(&tags) {
        for (kind, g) in records(&s.tags) {
            if kind == "DIMSTYLE" && g.iter().any(|t| t.code == 2 && t.str().eq_ignore_ascii_case("Standard")) {
                out.extend(g.iter().take_while(|t| t.code < 1000).filter(|t| t.code == 340).map(|t| t.str().to_ascii_uppercase()));
            }
        }
    }
    out
}

#[test]
fn the_kept_reference_is_written_while_the_text_style_is_unchanged() {
    let d = read_dxf(dxf(SRC).as_bytes()).unwrap();
    assert_eq!(text_style_refs(&write_dxf(&d)), vec!["17"]);
    // And through another save.
    let again = write_dxf(&read_dxf(write_dxf(&d).as_bytes()).unwrap());
    assert_eq!(text_style_refs(&again), vec!["17"]);
}

#[test]
fn a_text_style_given_to_the_dimension_style_replaces_the_kept_reference() {
    let mut d = read_dxf(dxf(SRC).as_bytes()).unwrap();
    let style = d.dim_styles.iter_mut().find(|s| s.name.eq_ignore_ascii_case("Standard")).unwrap();
    style.text_style = "Notes".into();
    assert_eq!(text_style_refs(&write_dxf(&d)), vec!["16"], "the new text style, once");
}
