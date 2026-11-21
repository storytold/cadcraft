//! A TrueType text style without a font file takes its family from the style's `ACAD` xdata.

fn style(name: &str, file: &str, xdata: &[(&str, &str)]) -> String {
    let mut s = format!("  0\nSTYLE\n  2\n{name}\n 70\n0\n 40\n0.0\n 41\n1.0\n 50\n0.0\n 71\n0\n 42\n0.2\n  3\n{file}\n  4\n\n");
    if !xdata.is_empty() {
        s.push_str("1001\nACAD\n");
        for (c, v) in xdata {
            s.push_str(&format!("{c}\n{v}\n"));
        }
    }
    s
}

#[test]
fn style_without_a_font_file_uses_the_xdata_typeface() {
    let dxf = format!(
        "  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nSTYLE\n 70\n3\n{}{}{}  0\nENDTAB\n  0\nENDSEC\n  0\nEOF\n",
        style("TT", "", &[("1000", "GOST type A"), ("1071", "2")]),
        style("File", "arial.ttf", &[("1000", "Other"), ("1071", "2")]),
        style("Empty", "", &[]),
    );
    let d = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    let font = |n: &str| d.text_styles.iter().find(|s| s.name == n).map(|s| s.font.clone());
    assert_eq!(font("TT").as_deref(), Some("GOST type A"));
    assert_eq!(font("File").as_deref(), Some("arial.ttf"), "a font file wins over the typeface");
    assert_eq!(font("Empty").as_deref(), Some(""));
}
