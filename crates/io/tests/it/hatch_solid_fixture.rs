//! A synthetic DXF with a SOLID and an ANSI31 hatch keeps both hatches' angle, scale and double
//! flag through a save (`tests/data/hatch_solid_and_pattern.dxf`).

/// The groups of the entity with handle `h` in the ENTITIES section, as (code, value) pairs.
fn record(dxf: &str, h: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let pairs: Vec<(&str, &str)> = lines.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0], c[1])).collect();
    let section = pairs.iter().position(|p| p.0 == "2" && p.1 == "ENTITIES").unwrap();
    let start = section + pairs[section..].iter().position(|p| p.0 == "5" && p.1 == h).unwrap();
    let end = pairs[start..].iter().position(|p| p.0 == "0").map_or(pairs.len(), |n| start + n);
    pairs[start..end].iter().map(|(c, v)| (c.to_string(), v.to_string())).collect()
}

fn value(rec: &[(String, String)], code: &str) -> Option<f64> {
    rec.iter().find(|p| p.0 == code).and_then(|p| p.1.parse().ok())
}

#[test]
fn hatch_fixture_keeps_angle_scale_and_double_flag() {
    let src = include_str!("../data/hatch_solid_and_pattern.dxf");
    let d = cadcraft_io::read(src.as_bytes(), "a.dxf").unwrap();
    let out = String::from_utf8(cadcraft_io::write(&d, "a.dxf").unwrap()).unwrap();
    // 100 is the SOLID hatch, 101 the ANSI31 one.
    for h in ["100", "101"] {
        let (before, after) = (record(src, h), record(&out, h));
        for code in ["52", "41", "77"] {
            assert_eq!(value(&after, code), value(&before, code), "hatch {h}, group {code}");
        }
    }
}
