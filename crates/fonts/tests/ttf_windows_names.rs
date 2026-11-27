//! A text style that names a Windows font which is not installed draws in the metric-compatible
//! free font when that is installed or registered (#1669), and an installed original always wins.
//! One test in its own binary: it registers fonts by name, which the font table keeps for the
//! whole process.

use cadcraft_fonts::ttf;

/// The smallest sfnt header the font table accepts (no tables); `extra` makes two distinct files.
fn sfnt(extra: usize) -> Vec<u8> {
    let mut b = vec![0u8, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    b.extend(std::iter::repeat_n(0u8, extra));
    b
}

fn bytes_of(name: &str) -> Option<Vec<u8>> {
    ttf::find(name).map(|f| f.bytes.as_slice().to_vec())
}

#[test]
fn a_missing_windows_font_resolves_to_its_metric_compatible_substitute() {
    // With the real Calibri installed (a Windows runner) there is nothing to substitute.
    if ttf::find("calibriz.ttf").is_some() {
        return;
    }
    ttf::register("Carlito-BoldItalic.ttf", sfnt(0));
    assert_eq!(bytes_of("calibriz.ttf"), Some(sfnt(0)), "calibriz.ttf draws in Carlito Bold Italic");
    assert_eq!(bytes_of("CALIBRIZ.TTF"), Some(sfnt(0)), "case does not matter");
    assert!(ttf::find("calibri.ttf").is_none(), "the regular file of the family is not installed either");
    assert!(ttf::find("verdana.ttf").is_none() || ttf::find("verdana.ttf").is_some_and(|f| f.bytes.as_slice() != sfnt(0)));

    // The original, once installed, takes the name back.
    ttf::register("calibriz.ttf", sfnt(1));
    assert_eq!(bytes_of("calibriz.ttf"), Some(sfnt(1)));
}
