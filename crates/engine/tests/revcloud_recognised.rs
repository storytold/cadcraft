//! Readers recognise a saved revision cloud (#1570): a closed polyline with `RevcloudProps`
//! xdata whose every arc has bulge 0.520567 (they accept 0.02 either way; 0.5 was just outside).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Drawing, EntityKind, Handle, RawTag};
use serde_json::{Value, json};

const BULGE: f64 = 0.520567050552;

fn h(v: &Value) -> Handle {
    v["handle"].as_str().and_then(Handle::parse_hex).unwrap()
}

fn last(s: &Session) -> Handle {
    s.doc().unwrap().model.iter().last().map(|e| e.handle).unwrap()
}

fn props(xdata: &[RawTag]) -> Vec<(i32, String)> {
    let start = xdata.iter().position(|t| t.code == 1001 && t.value == "RevcloudProps").expect("RevcloudProps xdata");
    xdata[start..].iter().take_while(|t| t.code != 1001 || t.value == "RevcloudProps").map(|t| (t.code, t.value.trim().to_string())).collect()
}

/// The cloud `h` is what readers take for a cloud: closed, cloud bulges, `RevcloudProps`.
fn assert_cloud(d: &Drawing, h: Handle, arc_len: f64) {
    let e = d.entity(h).unwrap();
    let EntityKind::LwPolyline(p) = &e.kind else { panic!("{:?}", e.kind) };
    assert!(p.closed);
    assert!(p.vertices.iter().all(|v| (v.bulge.abs() - BULGE).abs() < 1e-9), "{:?}", p.vertices);
    let x = props(&e.common.xdata);
    assert_eq!(x.len(), 3, "{x:?}");
    assert_eq!(x[1], (1070, "0".to_string()));
    assert_eq!(x[2].0, 1040);
    assert!((x[2].1.parse::<f64>().unwrap() - arc_len).abs() < 1e-12, "{x:?}");
}

#[test]
fn every_way_of_drawing_a_cloud_is_recognised() {
    let mut s = Session::new();
    let rect = h(&s.execute("revcloud", &json!({"p1": [0, 0], "p2": [100, 50], "arcLength": 15})).unwrap());
    let poly = h(&s.execute("revcloud.polygonal", &json!({"points": [[0, 0], [0, 30], [30, 0]], "arcLength": 5})).unwrap());
    let rev = h(&s.execute("revcloud.freehand", &json!({"freehand": [[0, 0], [50, 0], [50, 50]], "arcLength": 10, "reverse": true})).unwrap());
    let circle = h(&s.execute("circle", &json!({"center": [200, 0], "radius": 20})).unwrap());
    let obj = h(&s.execute("revcloud", &json!({"handle": circle, "arcLength": 4})).unwrap());
    s.cmdline("revcloud a 2").unwrap();
    s.cmdline("300,0 320,10").unwrap();
    let typed = last(&s);
    s.execute("rectang", &json!({"p1": [400, 0], "p2": [420, 20]})).unwrap();
    s.cmdline("revcloud a 3 o 400,10").unwrap();
    let picked = last(&s);
    let d = s.doc().unwrap();
    for (h, a) in [(rect, 15.0), (poly, 5.0), (rev, 10.0), (obj, 4.0), (typed, 2.0), (picked, 3.0)] {
        assert_cloud(d, h, a);
    }
    let EntityKind::LwPolyline(p) = &d.entity(rev).unwrap().kind else { panic!() };
    assert!(p.vertices.iter().all(|v| v.bulge < 0.0), "a reversed cloud keeps its arcs turned inward");
}

#[test]
fn a_saved_cloud_reads_back_as_a_cloud() {
    let mut s = Session::new();
    s.execute("revcloud", &json!({"p1": [0, 0], "p2": [100, 50], "arcLength": 15})).unwrap();
    let text = cadcraft_io::write_dxf(s.doc().unwrap());
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let appid = lines.windows(2).any(|w| w[0] == "0" && w[1] == "APPID")
        && lines.windows(4).any(|w| w[0] == "AcDbRegAppTableRecord" && w[1] == "2" && w[2] == "RevcloudProps");
    assert!(appid, "RevcloudProps is a registered application");
    let read = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let cloud = read.model.iter().find(|e| matches!(e.kind, EntityKind::LwPolyline(_))).map(|e| e.handle).unwrap();
    assert_cloud(&read, cloud, 15.0);
}
