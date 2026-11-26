//! The angles of a hatch's elliptic boundary edges in the DXF a DWG is read through.
//!
//! A DWG holds an elliptic edge's start and end angles in radians, and so does the DXF the DWG
//! decoder writes (groups 50 and 51 of an edge of type 3). DXF itself keeps them in degrees, which
//! is how [`crate::dxf_read`] reads them: a circle stored as an elliptic edge with the end angle
//! 6.2832 became an arc of 6.28 degrees, and the hatch covered the hole it should have left open.
//! Only the DWG path needs this; a DXF file is read as written.

use std::f64::consts::PI;

/// `dxf` with the start and end angles (groups 50 and 51) of every elliptic hatch edge (edge type
/// 3, group 72) turned from radians to degrees. The text is handled as bytes: a drawing from
/// before 2007 is written in its code page, which is not UTF-8, and every other byte is kept as it
/// was. Binary DXF, and text with no such edge, come back as they were.
pub(crate) fn elliptic_edge_degrees(dxf: Vec<u8>) -> Vec<u8> {
    if dxf.starts_with(b"AutoCAD Binary DXF") || !dxf.windows(5).any(|w| w == b"HATCH") {
        return dxf;
    }
    let mut out = Vec::with_capacity(dxf.len());
    let mut lines = dxf.split_inclusive(|b| *b == b'\n');
    let mut in_hatch = false;
    // After an elliptic edge's type, until the next edge or loop: the angles still to convert.
    let mut ellipse = false;
    let mut changed = false;
    while let Some(code_line) = lines.next() {
        let Some(value_line) = lines.next() else {
            out.extend_from_slice(code_line);
            break;
        };
        let code = code_line.trim_ascii();
        let value = value_line.trim_ascii();
        match code {
            b"0" => {
                in_hatch = value == b"HATCH";
                ellipse = false;
            }
            b"72" if in_hatch => ellipse = value == b"3",
            b"92" | b"97" if in_hatch => ellipse = false,
            b"50" | b"51" if in_hatch && ellipse => {
                if let Some(radians) = std::str::from_utf8(value).ok().and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()) {
                    let ending = &value_line[value_line.trim_ascii_end().len()..];
                    out.extend_from_slice(code_line);
                    out.extend_from_slice(format!("{}", radians * 180.0 / PI).as_bytes());
                    out.extend_from_slice(ending);
                    changed = true;
                    continue;
                }
            }
            _ => {}
        }
        out.extend_from_slice(code_line);
        out.extend_from_slice(value_line);
    }
    if changed { out } else { dxf }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(text: &str) -> Vec<(String, String)> {
        let lines: Vec<&str> = text.lines().collect();
        lines.chunks(2).filter_map(|c| Some((c.first()?.trim().to_string(), c.get(1)?.trim().to_string()))).collect()
    }

    const HATCH: &str = "0\r\nHATCH\r\n91\r\n1\r\n92\r\n1\r\n93\r\n2\r\n72\r\n3\r\n10\r\n5.0\r\n20\r\n6.0\r\n11\r\n-55.5\r\n21\r\n0.0\r\n40\r\n1.0\r\n50\r\n0.0\r\n51\r\n6.283185307179586\r\n73\r\n1\r\n72\r\n1\r\n10\r\n1.0\r\n20\r\n2.0\r\n11\r\n3.0\r\n21\r\n4.0\r\n50\r\n7.0\r\n0\r\nLINE\r\n50\r\n9.0\r\n";

    #[test]
    fn an_elliptic_edge_gets_its_angles_in_degrees_and_nothing_else_changes() {
        let out = String::from_utf8(elliptic_edge_degrees(HATCH.as_bytes().to_vec())).unwrap();
        let p = pairs(&out.replace("\r\n", "\n"));
        let value = |code: &str, n: usize| p.iter().filter(|(c, _)| c == code).nth(n).map(|(_, v)| v.parse::<f64>().unwrap());
        assert!((value("51", 0).unwrap() - 360.0).abs() < 1e-9);
        assert_eq!(value("50", 0), Some(0.0));
        // A line edge's 50 and another entity's are left alone, as is the line ending.
        assert_eq!(value("50", 1), Some(7.0));
        assert_eq!(value("50", 2), Some(9.0));
        assert!(out.contains("\r\n") && !out.replace("\r\n", "").contains('\r'));
    }

    /// A drawing from before 2007 is written in its code page: an accented letter is one byte that
    /// is not valid UTF-8, and it must not stop the angles being converted or change.
    #[test]
    fn a_code_page_drawing_still_has_its_angles_converted() {
        let mut dxf = b"0\r\nTEXT\r\n1\r\ncaf".to_vec();
        dxf.push(0xE9);
        dxf.extend_from_slice(b"\r\n");
        dxf.extend_from_slice(HATCH.as_bytes());
        let out = elliptic_edge_degrees(dxf.clone());
        assert!(std::str::from_utf8(&dxf).is_err(), "the input is not UTF-8");
        assert!(out.windows(4).any(|w| w == b"caf\xE9"), "the accented byte is kept");
        let text: String = out.iter().map(|b| char::from(*b)).collect();
        assert!(text.contains("51\r\n360"), "the end angle is 360 degrees: {text}");
    }

    #[test]
    fn text_without_a_hatch_ellipse_comes_back_untouched() {
        let dxf = b"0\nLINE\n50\n1.5\n".to_vec();
        assert_eq!(elliptic_edge_degrees(dxf.clone()), dxf);
        let binary = b"AutoCAD Binary DXF\r\n\x1a\x00HATCH".to_vec();
        assert_eq!(elliptic_edge_degrees(binary.clone()), binary);
        // A hatch with no elliptic edge is left as it is.
        let plain = "0\nHATCH\n72\n1\n50\n2.0\n".as_bytes().to_vec();
        assert_eq!(elliptic_edge_degrees(plain.clone()), plain);
    }
}
