//! Header variables CADCraft doesn't know by name are written back with the group codes they were
//! read with, through DXF. The fixture is synthetic,
//! from the public DXF Reference's HEADER section.

use cadcraft_doc::{HVal, POINT_2D, POINT_3D};
use cadcraft_dxf::{parse, sections};
use cadcraft_geom::Vec3;
use cadcraft_io::{read_dxf, write_dxf};

const SRC: &str = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n\
9\n$SURFTAB1\n70\n12\n\
9\n$USERR1\n40\n2.5\n\
9\n$PROJECTNAME\n1\nBRACKETS\n\
9\n$PUCSORG\n10\n1.0\n20\n2.0\n30\n3.0\n\
9\n$PLIMMAX\n10\n420.0\n20\n297.0\n\
9\n$SOLIDHIST\n280\n1\n\
9\n$REQUIREDVERSIONS\n160\n0\n\
9\n$CMATERIAL\n347\n96\n\
9\n$HANDSEED\n5\n200\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n";

/// The header of a DXF file: each variable's groups as (code, text).
fn header(text: &str) -> Vec<(String, Vec<(i32, String)>)> {
    let tags = parse(text.as_bytes()).unwrap();
    let mut out: Vec<(String, Vec<(i32, String)>)> = Vec::new();
    for s in sections(&tags).into_iter().filter(|s| s.name == "HEADER") {
        for t in s.tags {
            if t.code == 9 {
                out.push((t.str(), Vec::new()));
            } else if let Some(last) = out.last_mut() {
                last.1.push((t.code, t.str()));
            }
        }
    }
    out
}

fn var<'a>(h: &'a [(String, Vec<(i32, String)>)], name: &str) -> Option<&'a Vec<(i32, String)>> {
    h.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

#[test]
fn unknown_variables_keep_their_codes() {
    let d = read_dxf(SRC.as_bytes()).unwrap();
    assert_eq!(d.header.code("SURFTAB1"), Some(70));
    assert_eq!(d.header.code("PUCSORG"), Some(POINT_3D));
    assert_eq!(d.header.code("PLIMMAX"), Some(POINT_2D));
    assert_eq!(d.header.get("PUCSORG"), Some(&HVal::Point(Vec3::new(1.0, 2.0, 3.0))));

    let out = header(&write_dxf(&d));
    let codes = |n: &str| var(&out, n).map(|v| v.iter().map(|g| g.0).collect::<Vec<_>>());
    assert_eq!(codes("$SURFTAB1"), Some(vec![70]));
    assert_eq!(codes("$USERR1"), Some(vec![40]));
    assert_eq!(codes("$PROJECTNAME"), Some(vec![1]));
    assert_eq!(codes("$PUCSORG"), Some(vec![10, 20, 30]));
    assert_eq!(codes("$PLIMMAX"), Some(vec![10, 20]));
    assert_eq!(codes("$SOLIDHIST"), Some(vec![280]));
    assert_eq!(codes("$REQUIREDVERSIONS"), Some(vec![160]));
    assert_eq!(var(&out, "$PROJECTNAME").and_then(|v| v.first()).map(|g| g.1.as_str()), Some("BRACKETS"));
    // A variable pointing at an object by handle is left out while objects get new handles.
    assert!(var(&out, "$CMATERIAL").is_none());
    // Each variable once.
    let mut names: Vec<&String> = out.iter().map(|(n, _)| n).collect();
    let n = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), n, "a header variable was written twice");
}

#[test]
fn a_changed_value_is_written_with_the_kept_code() {
    let mut d = read_dxf(SRC.as_bytes()).unwrap();
    d.header.set_i64("SURFTAB1", 24);
    let out = header(&write_dxf(&d));
    assert_eq!(var(&out, "$SURFTAB1"), Some(&vec![(70, "24".to_string())]));
}
