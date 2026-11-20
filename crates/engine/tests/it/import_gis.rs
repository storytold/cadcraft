//! IMPORTGIS (and IMPORT of a .shp or .geojson): a shapefile with the .dbf beside it, and GeoJSON
//! data, into the current space with their attributes as GIS xdata.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{base64_encode, set_gis_reader};
use cadcraft_engine::doc::{EntityKind, Space};
use serde_json::json;

fn session() -> Session {
    set_gis_reader(cadcraft_io::gis::import);
    Session::new()
}

fn polygon_shp() -> Vec<u8> {
    let ring = [(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0), (0.0, 0.0)];
    let mut c = Vec::new();
    c.extend_from_slice(&5i32.to_le_bytes());
    c.extend_from_slice(&[0u8; 32]);
    c.extend_from_slice(&1i32.to_le_bytes());
    c.extend_from_slice(&5i32.to_le_bytes());
    c.extend_from_slice(&0i32.to_le_bytes());
    for (x, y) in ring {
        c.extend_from_slice(&(500_000.0f64 + x).to_le_bytes());
        c.extend_from_slice(&(100_000.0f64 + y).to_le_bytes());
    }
    let mut b = vec![0u8; 100];
    b[0..4].copy_from_slice(&9994i32.to_be_bytes());
    b[32..36].copy_from_slice(&5i32.to_le_bytes());
    b.extend_from_slice(&1i32.to_be_bytes());
    b.extend_from_slice(&((c.len() / 2) as i32).to_be_bytes());
    b.extend(c);
    b
}

fn name_dbf(name: &str) -> Vec<u8> {
    let mut b = vec![0u8; 32];
    b[0] = 3;
    b[4] = 1;
    b[8] = 65;
    b[10] = 11;
    let mut d = [0u8; 32];
    d[..4].copy_from_slice(b"NAME");
    d[11] = b'C';
    d[16] = 10;
    b.extend_from_slice(&d);
    b.push(0x0D);
    b.push(b' ');
    let mut v = name.as_bytes().to_vec();
    v.resize(10, b' ');
    b.extend(v);
    b
}

fn layers(s: &Session) -> Vec<(String, String)> {
    let d = s.doc().unwrap();
    d.space(&Space::Model)
        .unwrap()
        .iter()
        .map(|e| {
            let kind = match &e.kind {
                EntityKind::LwPolyline(_) => "polyline",
                EntityKind::Text(_) => "text",
                EntityKind::Point(_) => "point",
                _ => "other",
            };
            (e.common.layer.clone(), kind.to_string())
        })
        .collect()
}

#[test]
fn a_shapefile_with_its_dbf_imports_by_path() {
    let dir = std::env::temp_dir().join(format!("cadcraft-importgis-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let shp = dir.join("parcels.shp");
    std::fs::write(&shp, polygon_shp()).unwrap();
    std::fs::write(dir.join("parcels.dbf"), name_dbf("Lot 7")).unwrap();
    let mut s = session();
    let r = s.execute("importgis", &json!({ "path": shp.to_string_lossy() })).unwrap();
    assert_eq!(r["imported"], 2, "{r}");
    assert_eq!(layers(&s), vec![("GIS_PARCELS".into(), "polyline".into()), ("GIS_TEXT".into(), "text".into())]);
    let d = s.doc().unwrap();
    let ring = d.space(&Space::Model).unwrap().iter().next().unwrap();
    assert!(ring.common.xdata.iter().any(|t| t.code == 1000 && t.value == "NAME=Lot 7"), "{:?}", ring.common.xdata);
    assert_eq!(s.selection().len(), 2, "the imported objects are selected");
    let r = s.execute("import", &json!({ "path": shp.to_string_lossy(), "at": [10, 0] })).unwrap();
    assert_eq!(r["imported"], 2, "IMPORT hands a .shp to IMPORTGIS");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn geojson_data_imports_by_alias_and_undoes() {
    let mut s = session();
    let gj = br#"{"type":"FeatureCollection","features":[
        {"type":"Feature","properties":{"ref":"P1"},"geometry":{"type":"Point","coordinates":[9.5,47.1]}},
        {"type":"Feature","properties":{"highway":"service"},"geometry":{"type":"LineString","coordinates":[[9.5,47.1],[9.51,47.11]]}}]}"#;
    let data = base64_encode(gj);
    assert_eq!(cadcraft_engine::cmd::resolve_alias("MAPIMPORT"), "importgis");
    assert_eq!(cadcraft_engine::cmd::resolve_alias("geoimport"), "importgis");
    let r = s.execute("importgis", &json!({ "data": data, "name": "site.geojson", "layerField": "missing" })).unwrap();
    assert_eq!(r["imported"], 2, "{r}");
    assert!(r["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("EPSG:3857")));
    assert_eq!(layers(&s), vec![("GIS_POINTS".into(), "point".into()), ("GIS_ROADS".into(), "polyline".into())]);
    s.execute("u", &json!({})).unwrap();
    assert!(layers(&s).is_empty(), "one undo removes the import");
    let bad = s.execute("importgis", &json!({ "data": base64_encode(b"{}"), "name": "x.geojson" }));
    assert!(bad.is_err());
}

#[test]
fn projected_metres_are_converted_to_the_drawings_units() {
    let gj = br#"{"type":"LineString","coordinates":[[0,0],[0.001,0]]}"#;
    let import = |insunits: i64, extra: serde_json::Value| {
        let mut s = session();
        s.doc_mut().unwrap().header.set_i64("INSUNITS", insunits);
        let mut p = json!({ "data": base64_encode(gj), "name": "l.geojson" });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        s.execute("importgis", &p).unwrap();
        let d = s.doc().unwrap();
        let e = d.space(&Space::Model).unwrap().iter().next().unwrap();
        match &e.kind {
            EntityKind::LwPolyline(l) => l.vertices[1].p.x,
            _ => panic!("not a polyline"),
        }
    };
    let metres = 6_378_137.0 * 0.001f64.to_radians();
    let close = |a: f64, b: f64| (a - b).abs() < 1e-6 * b.abs();
    assert!(close(import(6, json!({})), metres));
    assert!(close(import(4, json!({})), metres * 1000.0), "a millimetre drawing gets millimetres");
    assert!(close(import(1, json!({})), metres / 0.0254));
    assert!(close(import(4, json!({ "scale": 2 })), metres * 2000.0), "an explicit scale still applies");
    assert!(close(import(0, json!({})), metres), "a unitless drawing is left as is");
}
