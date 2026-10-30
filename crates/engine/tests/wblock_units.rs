//! WBLOCK writes the drawing's $INSUNITS, so INSERT of the written file, which converts the file's
//! units to the drawing's (#1997), brings it back at its size (#1996).

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

#[test]
fn a_wblock_file_from_a_metric_drawing_inserts_back_at_its_size() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    let path = std::env::temp_dir().join(format!("cadcraft-wblock-units-{}.dxf", std::process::id())).to_string_lossy().into_owned();
    let mut s = Session::new();
    s.execute("units", &json!({ "insunits": 6 })).unwrap();
    let h = s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap()["handles"][0].as_str().unwrap().to_string();
    s.execute("wblock", &json!({ "handles": [h], "path": &path })).unwrap();
    s.execute("insert", &json!({ "name": "PART", "file": &path, "at": [0, 0] })).unwrap();
    let _ = std::fs::remove_file(&path);
    let scale = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::Insert(i) = &e.kind { Some((i.scale.x, i.scale.y)) } else { None });
    assert_eq!(scale, Some((1.0, 1.0)));
}
