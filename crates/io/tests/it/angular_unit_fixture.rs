//! A synthetic DXF with one dimension of every kind keeps each one's group 42 through a save
//! (`tests/data/dimensions_all_kinds.dxf`). Its angular dimension stores 60° in degrees.

/// The groups of the entity with handle `h` in the ENTITIES section, as (code, value) pairs.
fn record(dxf: &str, h: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let pairs: Vec<(&str, &str)> = lines.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0], c[1])).collect();
    let section = pairs.iter().position(|p| p.0 == "2" && p.1 == "ENTITIES").unwrap();
    let start = section + pairs[section..].iter().position(|p| p.0 == "5" && p.1 == h).unwrap();
    let end = pairs[start..].iter().position(|p| p.0 == "0").map_or(pairs.len(), |n| start + n);
    pairs[start..end].iter().map(|(c, v)| (c.to_string(), v.to_string())).collect()
}

/// The first value of group `code` in a record, parsed as a number.
fn value(rec: &[(String, String)], code: &str) -> Option<f64> {
    rec.iter().find(|p| p.0 == code).and_then(|p| p.1.parse().ok())
}

/// Saving the fixture keeps group 42 of every dimension in its unit, so the angular 60 degrees stays in degrees.
#[test]
fn dimension_fixture_keeps_group_42_in_its_unit() {
    let src = include_str!("../data/dimensions_all_kinds.dxf");
    let d = cadcraft_io::read(src.as_bytes(), "a.dxf").unwrap();
    let out = String::from_utf8(cadcraft_io::write(&d, "a.dxf").unwrap()).unwrap();
    // 100 to 106: linear, aligned, angular (102, in degrees), radius, diameter and two overrides.
    for h in ["100", "101", "102", "103", "104", "105", "106"] {
        let (a, b) = (value(&record(src, h), "42").unwrap(), value(&record(&out, h), "42").unwrap());
        assert!((a - b).abs() <= 1e-6 * a.abs().max(1.0), "dimension {h}: group 42 {a} -> {b}");
    }
}
