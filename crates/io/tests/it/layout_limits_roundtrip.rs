//! A layout's limits (AcDbLayout groups 10/11) and paper image origin (AcDbPlotSettings groups
//! 148/149) survive a DXF round trip (#2109). They place the sheet in paper space; with fixed
//! values another program drew the sheet away from the entities and shifted a "Layout" plot.
//! Layouts CADCraft creates keep (0,0)-(12,9) and origin 0,0.

use cadcraft_doc::*;
use cadcraft_geom::Vec2;

/// DXF text with groups of the "Layout1" LAYOUT record set to new values: (code, value,
/// whether the group comes after the record's `AcDbLayout` marker).
fn set_paper_layout(text: &str, values: &[(&str, &str, bool)]) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i + 1 < lines.len() {
        let (code, value) = (lines[i].trim(), lines[i + 1].trim());
        if code == "0" && value == "LAYOUT" {
            // Collect the record.
            let mut j = i + 2;
            while j + 1 < lines.len() && lines[j].trim() != "0" {
                j += 2;
            }
            let mut rec: Vec<(String, String)> = (i..j).step_by(2).map(|k| (lines[k].to_string(), lines[k + 1].to_string())).collect();
            let at = rec.iter().position(|(c, v)| c.trim() == "100" && v.trim() == "AcDbLayout").unwrap();
            let name = rec.iter().skip(at).find(|(c, _)| c.trim() == "1").map(|(_, v)| v.trim().to_string()).unwrap();
            if name == "Layout1" {
                for (c, v, after) in values {
                    let k = rec.iter().enumerate().position(|(k, (rc, _))| rc.trim() == *c && (k > at) == *after).unwrap();
                    rec[k].1 = v.to_string();
                }
            }
            for (c, v) in rec {
                out.push(c);
                out.push(v);
            }
            i = j;
        } else {
            out.push(lines[i].to_string());
            out.push(lines[i + 1].to_string());
            i += 2;
        }
    }
    out.extend(lines.iter().skip(i).map(|l| l.to_string()));
    out.join("\n") + "\n"
}

/// Limits (10, 20, 11, 21) and paper image origin (148, 149) of every paper-space LAYOUT.
fn limits(text: &str) -> Vec<[f64; 6]> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut recs: Vec<Vec<(String, String)>> = Vec::new();
    let mut inside = false;
    for pair in lines.chunks(2) {
        let [code, value] = pair else { break };
        if *code == "0" {
            inside = *value == "LAYOUT";
            if inside {
                recs.push(Vec::new());
            }
        } else if inside && let Some(rec) = recs.last_mut() {
            rec.push((code.to_string(), value.to_string()));
        }
    }
    recs.iter()
        .filter_map(|rec| {
            let at = rec.iter().position(|(c, v)| c == "100" && v == "AcDbLayout")?;
            let name = rec.iter().skip(at).find(|(c, _)| c == "1").map(|(_, v)| v.as_str())?;
            if name == "Model" {
                return None;
            }
            let after = |code: &str| rec.iter().skip(at).find(|(c, _)| c == code).and_then(|(_, v)| v.parse().ok()).unwrap_or(f64::NAN);
            let before = |code: &str| rec.iter().take(at).find(|(c, _)| c == code).and_then(|(_, v)| v.parse().ok()).unwrap_or(f64::NAN);
            Some([after("10"), after("20"), after("11"), after("21"), before("148"), before("149")])
        })
        .collect()
}

/// What CADCraft writes for a layout it created.
const DEFAULT: [f64; 6] = [0.0, 0.0, 12.0, 9.0, 0.0, 0.0];

#[test]
fn layout_limits_and_paper_image_origin_survive_a_round_trip() {
    let src = cadcraft_io::write_dxf(&Drawing::new_metric());
    let edited = set_paper_layout(
        &src,
        &[
            ("10", "1.038", true),
            ("20", "0.694", true),
            ("11", "9.506", true),
            ("21", "6.681", true),
            ("148", "-1.054", false),
            ("149", "-0.71", false),
        ],
    );
    let ours = [1.038, 0.694, 9.506, 6.681, -1.054, -0.71];
    assert_eq!(limits(&edited), vec![ours, DEFAULT]);
    let d = cadcraft_io::read_dxf(edited.as_bytes()).unwrap();
    let out = cadcraft_io::write_dxf(&d);
    // Layout1 keeps the file's values; Layout2 had the defaults and keeps them.
    assert_eq!(limits(&out), vec![ours, DEFAULT]);
}

#[test]
fn new_layouts_keep_the_default_limits_and_origin() {
    let out = cadcraft_io::write_dxf(&Drawing::new_metric());
    assert_eq!(limits(&out), vec![DEFAULT, DEFAULT]);
}

#[test]
fn non_finite_limits_and_origin_fall_back_to_the_defaults() {
    let src = cadcraft_io::write_dxf(&Drawing::new_metric());
    let edited = set_paper_layout(&src, &[("10", "NaN", true), ("148", "NaN", false)]);
    let d = cadcraft_io::read_dxf(edited.as_bytes()).unwrap();
    let out = cadcraft_io::write_dxf(&d);
    assert_eq!(limits(&out), vec![DEFAULT, DEFAULT]);
}

#[test]
fn non_finite_limits_and_origin_in_memory_write_the_defaults() {
    let mut d = Drawing::new_metric();
    for l in &mut d.layouts {
        l.limits = Some((Vec2::new(f64::NAN, 0.0), Vec2::new(12.0, f64::INFINITY)));
        l.page.paper_image_origin = Some(Vec2::new(f64::NAN, 1.0));
    }
    assert_eq!(limits(&cadcraft_io::write_dxf(&d)), vec![DEFAULT, DEFAULT]);
}
