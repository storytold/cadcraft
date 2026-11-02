//! A synthetic DXF with a control-point SPLINE and a 3D fit-point SPLINE keeps every point's Z
//! through a save (`tests/data/spline_3d_fit_points.dxf`).

/// The groups of the entity with handle `h` in the ENTITIES section, as (code, value) pairs.
fn record(dxf: &str, h: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let pairs: Vec<(&str, &str)> = lines.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0], c[1])).collect();
    let section = pairs.iter().position(|p| p.0 == "2" && p.1 == "ENTITIES").unwrap();
    let start = section + pairs[section..].iter().position(|p| p.0 == "5" && p.1 == h).unwrap();
    let end = pairs[start..].iter().position(|p| p.0 == "0").map_or(pairs.len(), |n| start + n);
    pairs[start..end].iter().map(|(c, v)| (c.to_string(), v.to_string())).collect()
}

fn values(rec: &[(String, String)], code: &str) -> Vec<f64> {
    rec.iter().filter(|p| p.0 == code).map(|p| p.1.parse().unwrap()).collect()
}

#[test]
fn spline_fixture_keeps_the_z_of_its_points() {
    let src = include_str!("../data/spline_3d_fit_points.dxf");
    let d = cadcraft_io::read(src.as_bytes(), "a.dxf").unwrap();
    let out = String::from_utf8(cadcraft_io::write(&d, "a.dxf").unwrap()).unwrap();
    // The fit-point spline rises in Z; it stays 3D (planar flag clear).
    let (before, after) = (record(src, "102"), record(&out, "102"));
    assert_eq!(values(&after, "31"), values(&before, "31"));
    assert!(values(&before, "31").iter().any(|z| *z != 0.0));
    assert_eq!(values(&after, "70")[0] as i64 & 8, 0);
    // The control-point spline keeps its points, Z included.
    let (before, after) = (record(src, "101"), record(&out, "101"));
    for code in ["10", "20", "30"] {
        let (a, b) = (values(&before, code), values(&after, code));
        assert_eq!(a.len(), b.len(), "group {code}");
        assert!(a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-9), "group {code}: {a:?} -> {b:?}");
    }
}
