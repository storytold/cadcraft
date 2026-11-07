//! The registered applications (APPID) of a file are saved back, in DXF and in DWG, save after
//! save, also those no xdata names.

use cadcraft_doc::Drawing;
use cadcraft_io::{read, read_dxf, write, write_dxf};

/// The records of a DXF: each `(code, value)` from its `0` group on.
fn records(text: &str) -> Vec<Vec<(i32, String)>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<Vec<(i32, String)>> = Vec::new();
    for chunk in lines.chunks(2) {
        let [code, value] = chunk else { break };
        let Ok(c) = code.trim().parse::<i32>() else { continue };
        let pair = (c, value.trim().to_string());
        if c == 0 {
            out.push(vec![pair]);
        } else if let Some(r) = out.last_mut() {
            r.push(pair);
        }
    }
    out
}

fn value(r: &[(i32, String)], code: i32) -> Option<&str> {
    r.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str())
}

/// A file whose APPID table has `Other_App` (handle 7A00), which no xdata names.
fn source() -> String {
    let mut out = String::new();
    let mut put = |c: i32, v: &str| out.push_str(&format!("{c:>3}\n{v}\n"));
    for r in records(&write_dxf(&Drawing::new_metric())) {
        for (c, v) in &r {
            put(*c, v);
        }
        if r[0].1 == "TABLE" && value(&r, 2) == Some("APPID") {
            let table = value(&r, 5).unwrap_or("0").to_string();
            for (c, v) in [
                (0, "APPID"),
                (5, "7A00"),
                (330, table.as_str()),
                (100, "AcDbSymbolTableRecord"),
                (100, "AcDbRegAppTableRecord"),
                (2, "Other_App"),
                (70, "0"),
            ] {
                put(c, v);
            }
        }
    }
    out
}

/// Whether the DXF has an APPID record named `Other_App`.
fn appid(text: &str) -> bool {
    records(text).into_iter().any(|r| r[0].1 == "APPID" && value(&r, 2) == Some("Other_App"))
}

#[test]
fn registered_applications_no_xdata_names_are_kept() {
    let mut d = read_dxf(source().as_bytes()).expect("read");
    for generation in 1..=2 {
        let text = write_dxf(&d);
        assert!(appid(&text), "DXF {generation}");
        d = read_dxf(text.as_bytes()).expect("reread");
    }
    for generation in 1..=2 {
        let dwg = write(&d, "kept.dwg").expect("DWG");
        d = read(&dwg, "kept.dwg").expect("DWG read");
        assert!(appid(&write_dxf(&d)), "DWG {generation}");
    }
}
