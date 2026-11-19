use cadcraft_doc::{Drawing, Entity, EntityKind, Space};
use serde_json::json;

use super::*;

const E: f64 = 2_600_000.0;
const N: f64 = 1_200_000.0;

fn record_bytes(content: &[u8], n: usize) -> Vec<u8> {
    let mut r = Vec::new();
    r.extend_from_slice(&(n as i32).to_be_bytes());
    r.extend_from_slice(&((content.len() / 2) as i32).to_be_bytes());
    r.extend_from_slice(content);
    r
}

fn shp(kind: i32, records: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = records.iter().enumerate().flat_map(|(i, c)| record_bytes(c, i + 1)).collect();
    let mut h = vec![0u8; 100];
    h[0..4].copy_from_slice(&9994i32.to_be_bytes());
    h[24..28].copy_from_slice((((100 + body.len()) / 2) as i32).to_be_bytes().as_slice());
    h[28..32].copy_from_slice(&1000i32.to_le_bytes());
    h[32..36].copy_from_slice(&kind.to_le_bytes());
    h.extend(body);
    h
}

fn f(v: &mut Vec<u8>, x: f64) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn i(v: &mut Vec<u8>, x: i32) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn point(x: f64, y: f64) -> Vec<u8> {
    let mut v = Vec::new();
    i(&mut v, 1);
    f(&mut v, x);
    f(&mut v, y);
    v
}

fn null() -> Vec<u8> {
    0i32.to_le_bytes().to_vec()
}

fn parts(kind: i32, parts: &[&[(f64, f64, f64)]]) -> Vec<u8> {
    let mut v = Vec::new();
    i(&mut v, kind);
    for _ in 0..4 {
        f(&mut v, 0.0);
    }
    let n: usize = parts.iter().map(|p| p.len()).sum();
    i(&mut v, parts.len() as i32);
    i(&mut v, n as i32);
    let mut start = 0;
    for p in parts {
        i(&mut v, start);
        start += p.len() as i32;
    }
    for p in parts.iter().flat_map(|p| p.iter()) {
        f(&mut v, p.0);
        f(&mut v, p.1);
    }
    if matches!(kind, 13 | 15) {
        f(&mut v, 0.0);
        f(&mut v, 0.0);
        for p in parts.iter().flat_map(|p| p.iter()) {
            f(&mut v, p.2);
        }
    }
    v
}

fn dbf(fields: &[(&str, u8, u8)], rows: &[&[&[u8]]]) -> Vec<u8> {
    let width = 1 + fields.iter().map(|f| usize::from(f.2)).sum::<usize>();
    let header = 32 + 32 * fields.len() + 1;
    let mut v = vec![0u8; 32];
    v[0] = 3;
    v[4..8].copy_from_slice(&(rows.len() as u32).to_le_bytes());
    v[8..10].copy_from_slice(&(header as u16).to_le_bytes());
    v[10..12].copy_from_slice(&(width as u16).to_le_bytes());
    for (name, kind, len) in fields {
        let mut d = [0u8; 32];
        d[..name.len()].copy_from_slice(name.as_bytes());
        d[11] = *kind;
        d[16] = *len;
        v.extend_from_slice(&d);
    }
    v.push(0x0D);
    for row in rows {
        v.push(b' ');
        for (cell, field) in row.iter().zip(fields) {
            let mut c = cell.to_vec();
            c.resize(usize::from(field.2), b' ');
            v.extend(c);
        }
    }
    v.push(0x1A);
    v
}

fn model(d: &Drawing) -> Vec<&Entity> {
    d.space(&Space::Model).map(|s| s.iter().map(|e| &**e).collect()).unwrap_or_default()
}

fn on<'a>(d: &'a Drawing, layer: &str) -> Vec<&'a Entity> {
    model(d).into_iter().filter(|e| e.common.layer == layer).collect()
}

fn pts(e: &Entity) -> Vec<(f64, f64)> {
    match &e.kind {
        EntityKind::LwPolyline(p) => p.vertices.iter().map(|v| (v.p.x, v.p.y)).collect(),
        EntityKind::Polyline3d(p) => p.points.iter().map(|v| (v.x, v.y)).collect(),
        EntityKind::Point(p) => vec![(p.p.x, p.p.y)],
        _ => Vec::new(),
    }
}

fn attrs(e: &Entity) -> Vec<(String, String)> {
    attributes(&e.common.xdata)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6 * b.abs().max(1.0)
}

#[test]
fn points_with_attributes_from_the_dbf() {
    let s = shp(1, &[point(E, N), point(E + 10.0, N + 5.0)]);
    let t = dbf(&[("NAME", b'C', 10), ("HEIGHT", b'N', 6)], &[&[b"Marker A", b"  12.5"], &[b"Marker B", b""]]);
    let d = read_shapefile(&s, Some(&t), None, "survey.shp", &Options::default()).unwrap();
    let p = on(&d, POINTS);
    assert_eq!(model(&d).len(), 2);
    assert_eq!(pts(p[0]), vec![(E, N)]);
    assert_eq!(pts(p[1]), vec![(E + 10.0, N + 5.0)]);
    assert_eq!(p[0].common.xdata[0].code, 1001);
    assert_eq!(p[0].common.xdata[0].value, "GIS");
    assert_eq!(attrs(p[0]), vec![("NAME".into(), "Marker A".into()), ("HEIGHT".into(), "12.5".into())]);
    assert_eq!(attrs(p[1]), vec![("NAME".into(), "Marker B".into())], "an empty value is left out");
    assert!(!d.read_notes.iter().any(|n| n.contains("Web Mercator")), "projected coordinates stay: {:?}", d.read_notes);
}

#[test]
fn lines_by_part_and_roads_by_attribute() {
    let a: &[(f64, f64, f64)] = &[(E, N, 0.0), (E + 100.0, N, 0.0), (E + 100.0, N + 50.0, 0.0)];
    let b: &[(f64, f64, f64)] = &[(E, N + 200.0, 0.0), (E + 10.0, N + 210.0, 0.0)];
    let s = shp(3, &[parts(3, &[a, b]), parts(3, &[b])]);
    let t = dbf(&[("HIGHWAY", b'C', 10)], &[&[b"primary"], &[b""]]);
    let d = read_shapefile(&s, Some(&t), None, "network.shp", &Options::default()).unwrap();
    let roads = on(&d, ROADS);
    assert_eq!(roads.len(), 2, "one polyline per part");
    assert_eq!(pts(roads[0]), vec![(E, N), (E + 100.0, N), (E + 100.0, N + 50.0)]);
    assert!(matches!(&roads[0].kind, EntityKind::LwPolyline(p) if !p.closed));
    assert_eq!(on(&d, LINES).len(), 1, "a line without road attributes");
}

fn square(x0: f64, y0: f64, s: f64, clockwise: bool) -> Vec<(f64, f64, f64)> {
    let mut v = vec![(x0, y0, 0.0), (x0 + s, y0, 0.0), (x0 + s, y0 + s, 0.0), (x0, y0 + s, 0.0)];
    if clockwise {
        v.reverse();
    }
    let first = v[0];
    v.push(first);
    v
}

#[test]
fn polygon_with_a_hole_and_a_label_at_its_centroid() {
    let outer = square(E, N, 10.0, true);
    let hole = square(E + 6.0, N + 6.0, 2.0, false);
    let s = shp(5, &[parts(5, &[&outer, &hole])]);
    let t = dbf(&[("BUILDING", b'C', 5), ("NAME", b'C', 12)], &[&[b"yes", b"Town Hall"]]);
    let d = read_shapefile(&s, Some(&t), None, "city.shp", &Options::default()).unwrap();
    let b = on(&d, BUILDINGS);
    assert_eq!(b.len(), 2, "outer ring and hole");
    for e in &b {
        assert!(matches!(&e.kind, EntityKind::LwPolyline(p) if p.closed && p.vertices.len() == 4), "{:?}", e.kind);
        assert_eq!(attrs(e)[1], ("NAME".into(), "Town Hall".into()));
    }
    assert!(pts(b[1]).contains(&(E + 6.0, N + 6.0)));
    let labels = on(&d, LABELS);
    assert_eq!(labels.len(), 1);
    let EntityKind::Text(t) = &labels[0].kind else { panic!("{:?}", labels[0].kind) };
    assert_eq!(t.value, "Town Hall");
    let c = (500.0 - 4.0 * 7.0) / 96.0;
    assert!(near(t.insert.x, E + c) && near(t.insert.y, N + c), "{:?}", t.insert);
    assert!((MIN_LABEL_HEIGHT..=MAX_LABEL_HEIGHT).contains(&t.height));
    assert!(t.height <= 0.8 * 10.0 / (0.7 * 9.0) + 1e-9, "the label fits the polygon: {}", t.height);
    let none = read_shapefile(&s, Some(&t_bytes()), None, "a.shp", &Options { labels: false, ..Options::default() }).unwrap();
    assert!(on(&none, LABELS).is_empty());
}

fn t_bytes() -> Vec<u8> {
    dbf(&[("NAME", b'C', 4)], &[&[b"Lot1"]])
}

#[test]
fn z_polygons_and_polylines_keep_heights() {
    let flat: Vec<(f64, f64, f64)> = square(E, N, 5.0, true).into_iter().map(|(x, y, _)| (x, y, 430.0)).collect();
    let mut sloped = flat.clone();
    sloped[1].2 = 431.0;
    let s = shp(15, &[parts(15, &[&flat]), parts(15, &[&sloped])]);
    let d = read_shapefile(&s, None, None, "terrain.shp", &Options::default()).unwrap();
    let p = on(&d, POLYGONS);
    assert!(matches!(&p[0].kind, EntityKind::LwPolyline(l) if l.closed && l.elevation == 430.0));
    assert!(matches!(&p[1].kind, EntityKind::Polyline3d(l) if l.closed && l.points.len() == 4 && l.points.iter().any(|q| q.z == 431.0)));
    assert!(d.read_notes.iter().any(|n| n.contains(".dbf")), "the missing table is reported");
}

#[test]
fn multipoints_and_null_shapes_keep_records_in_line() {
    let mut mp = Vec::new();
    i(&mut mp, 8);
    for _ in 0..4 {
        f(&mut mp, 0.0);
    }
    i(&mut mp, 3);
    for k in 0..3 {
        f(&mut mp, E + f64::from(k));
        f(&mut mp, N);
    }
    let s = shp(8, &[null(), mp]);
    let t = dbf(&[("ID", b'N', 4)], &[&[b"   1"], &[b"   2"]]);
    let d = read_shapefile(&s, Some(&t), None, "m.shp", &Options::default()).unwrap();
    let p = on(&d, POINTS);
    assert_eq!(p.len(), 3);
    assert!(p.iter().all(|e| attrs(e) == vec![("ID".into(), "2".into())]));
    assert!(d.read_notes.iter().any(|n| n.contains("1 null shapes")), "{:?}", d.read_notes);
}

#[test]
fn geographic_prj_projects_to_web_mercator() {
    let s = shp(1, &[point(8.5, 47.0)]);
    let prj = r#"GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]]]"#;
    let d = read_shapefile(&s, None, Some(prj), "p.shp", &Options::default()).unwrap();
    let (x, y) = pts(on(&d, POINTS)[0])[0];
    assert!(near(x, 6_378_137.0 * 8.5f64.to_radians()), "{x}");
    assert!(near(y, 6_378_137.0 * (std::f64::consts::FRAC_PI_4 + 47f64.to_radians() / 2.0).tan().ln()), "{y}");
    assert!(d.read_notes.iter().any(|n| n.contains("EPSG:3857")));
    let projected = r#"PROJCS["CH1903+_LV95",GEOGCS["GCS_CH1903+"]]"#;
    let kept = read_shapefile(&s, None, Some(projected), "p.shp", &Options::default()).unwrap();
    assert_eq!(pts(on(&kept, POINTS)[0]), vec![(8.5, 47.0)], "a projected .prj keeps the coordinates");
}

#[test]
fn dbf_text_logical_and_memo_fields() {
    let t = dbf(&[("STREET", b'C', 8), ("OK", b'L', 1), ("NOTE", b'M', 10)], &[&[b"Stra\xdfe", b"T", b"        12"]]);
    let table = dbf::read(&t).unwrap();
    assert_eq!(table.rows, vec![vec![("STREET".to_string(), "Straße".to_string()), ("OK".into(), "true".into())]]);
    assert!(dbf::read(&t[..20]).is_err());
}

#[test]
fn damaged_files_never_panic() {
    let outer = square(E, N, 10.0, true);
    let s = shp(5, &[parts(5, &[&outer]), point(E, N), parts(15, &[&outer])]);
    let t = dbf(&[("NAME", b'C', 10)], &[&[b"a"], &[b"b"], &[b"c"]]);
    for n in 0..s.len() {
        let _ = read_shapefile(&s[..n], Some(&t[..n.min(t.len())]), None, "x.shp", &Options::default());
    }
    let mut bad = s.clone();
    for k in (100..bad.len()).step_by(7) {
        bad[k] = 0xFF;
        let _ = read_shapefile(&bad, Some(&t), None, "x.shp", &Options::default());
    }
    let d = read_shapefile(&s[..s.len() - 10], None, None, "x.shp", &Options::default()).unwrap();
    assert!(d.read_notes.iter().any(|n| n.contains("ends inside record 3")), "{:?}", d.read_notes);
    assert!(read_shapefile(b"not a shapefile at all", None, None, "x.shp", &Options::default()).is_err());
    for g in ["", "[]", "{}", r#"{"type":"FeatureCollection"}"#, r#"{"type":"Point","coordinates":"x"}"#, "{\"type\":"] {
        let _ = read_geojson(g.as_bytes(), "x.geojson", &Options::default());
    }
    let deep =
        format!("{}{}{}", r#"{"type":"GeometryCollection","geometries":["#.repeat(40), r#"{"type":"Point","coordinates":[1,2]}"#, "]}".repeat(40));
    assert!(read_geojson(deep.as_bytes(), "x.geojson", &Options::default()).is_err());
}

const COLLECTION: &str = r#"{
  "type": "FeatureCollection",
  "features": [
    {"type": "Feature", "properties": {"name": "Well 7", "depth": 12.5, "active": true, "note": null},
     "geometry": {"type": "Point", "coordinates": [9.5, 47.0, 500]}},
    {"type": "Feature", "properties": {"highway": "residential"},
     "geometry": {"type": "LineString", "coordinates": [[9.5, 47.0], [9.501, 47.001]]}},
    {"type": "Feature", "properties": {"name": "Parcel 12", "parcel_no": "12"},
     "geometry": {"type": "Polygon", "coordinates": [
        [[9.5, 47.0], [9.502, 47.0], [9.502, 47.002], [9.5, 47.002], [9.5, 47.0]],
        [[9.5005, 47.0005], [9.5005, 47.0010], [9.5010, 47.0010], [9.5010, 47.0005], [9.5005, 47.0005]]]}},
    {"type": "Feature", "properties": {"kind": "pond"},
     "geometry": {"type": "MultiPolygon", "coordinates": [
        [[[9.6, 47.1], [9.61, 47.1], [9.61, 47.11], [9.6, 47.1]]],
        [[[9.7, 47.1], [9.71, 47.1], [9.71, 47.11], [9.7, 47.1]]]]}},
    {"type": "Feature", "properties": {"name": "orphan"}, "geometry": null}
  ]
}"#;

#[test]
fn geojson_features_layers_and_projection() {
    let d = read_geojson(COLLECTION.as_bytes(), "site.geojson", &Options::default()).unwrap();
    let p = on(&d, POINTS);
    assert_eq!(p.len(), 1);
    let EntityKind::Point(pt) = &p[0].kind else { panic!() };
    assert!(near(pt.p.x, 6_378_137.0 * 9.5f64.to_radians()) && pt.p.z == 500.0, "{:?}", pt.p);
    let a = attrs(p[0]);
    assert!(a.contains(&("depth".into(), "12.5".into())) && a.contains(&("active".into(), "true".into())));
    assert!(!a.iter().any(|(k, _)| k == "note"), "null properties are left out");
    assert_eq!(on(&d, ROADS).len(), 1);
    assert_eq!(on(&d, PARCELS).len(), 2, "outer ring and hole");
    assert_eq!(on(&d, POLYGONS).len(), 2, "one closed polyline per polygon of a MultiPolygon");
    let labels = on(&d, LABELS);
    assert_eq!(labels.len(), 1);
    assert!(matches!(&labels[0].kind, EntityKind::Text(t) if t.value == "Parcel 12"));
    assert_eq!(model(&d).len(), 7);
    assert!(d.read_notes.iter().any(|n| n.contains("1 features without geometry")), "{:?}", d.read_notes);
    assert_eq!(d.layer(PARCELS).map(|l| l.color), Some(Color::Index(3)));
}

#[test]
fn geojson_crs_and_options() {
    let projected = r#"{"type":"Feature","crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::2056"}},
        "properties":{"zone":"W2"},"geometry":{"type":"LineString","coordinates":[[10,20],[30,40]]}}"#;
    let d = read_geojson(projected.as_bytes(), "a.geojson", &Options::default()).unwrap();
    assert_eq!(pts(on(&d, LINES)[0]), vec![(10.0, 20.0), (30.0, 40.0)], "a projected crs keeps small coordinates");
    let plain = r#"{"type":"LineString","coordinates":[[10,20],[30,40]]}"#;
    let kept = read_geojson(plain.as_bytes(), "a.geojson", &Options { projection: Projection::None, ..Options::default() }).unwrap();
    assert_eq!(pts(model(&kept)[0]), vec![(10.0, 20.0), (30.0, 40.0)]);
    let by_zone = read_geojson(projected.as_bytes(), "a.geojson", &Options { layer_field: Some("ZONE".into()), ..Options::default() }).unwrap();
    assert_eq!(model(&by_zone)[0].common.layer, "W2");
    let o = Options::from_json(&json!({"layerField": "type", "labels": false, "projection": "none"})).unwrap();
    assert_eq!(o, Options { layer_field: Some("type".into()), labels: false, projection: Projection::None });
    assert!(Options::from_json(&json!({"projection": "utm"})).is_err());
    assert!(Options::from_json(&json!({"labels": "yes"})).is_err());
}

#[test]
fn layer_names_from_values_are_cleaned() {
    assert_eq!(layer_name(" a/b:c "), Some("a_b_c".into()));
    assert_eq!(layer_name("   "), None);
}

#[test]
fn attributes_survive_a_dxf_round_trip() {
    let d = read_geojson(COLLECTION.as_bytes(), "site.geojson", &Options::default()).unwrap();
    let dxf = crate::write(&d, "out.dxf").unwrap();
    let text = String::from_utf8_lossy(&dxf).replace("\r\n", "\n");
    assert!(text.contains("AcDbRegAppTableRecord\n  2\nGIS\n"), "GIS is a registered application");
    assert!(text.contains("1001\nGIS\n1000\n"), "entities carry GIS xdata");
    let back = crate::read(&dxf, "out.dxf").unwrap();
    let well = model(&back).into_iter().find(|e| matches!(e.kind, EntityKind::Point(_))).unwrap();
    assert!(attrs(well).contains(&("name".into(), "Well 7".into())), "{:?}", well.common.xdata);
}

#[test]
fn import_picks_the_reader() {
    let s = shp(1, &[point(E, N)]);
    let d = import("x.shp", &s, None, None, &json!({})).unwrap();
    assert_eq!(model(&d).len(), 1);
    let g = import("x.json", br#"{"type":"Point","coordinates":[1,2]}"#, None, None, &json!({"projection": "none"})).unwrap();
    assert_eq!(pts(model(&g)[0]), vec![(1.0, 2.0)]);
    assert!(import("x.json", b"nope", None, None, &json!({})).is_err());
    assert!(is_gis_file("A.SHP") && is_gis_file("b.geojson") && !is_gis_file("c.dxf"));
}
