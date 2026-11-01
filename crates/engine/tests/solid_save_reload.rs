//! A drawing with an exact solid and a boolean (mesh) solid saved as DXF and opened again keeps
//! both solids: their groups, volumes and the exact body, which still edits and exports as exact
//! geometry (#2544 review).

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use serde_json::{Value, json};

fn hooks() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
}

fn handles(r: &Value) -> Vec<String> {
    r["handles"].as_array().unwrap().iter().map(|h| h.as_str().unwrap().to_string()).collect()
}

/// The signed volume enclosed by the 3DFACE triangles of `group` (a solid's display face repeats
/// its third corner as the fourth).
fn volume(s: &Session, group: &str) -> f64 {
    let d = s.doc().unwrap();
    let g = d.groups.iter().find(|g| g.name == group).unwrap();
    let mut v = 0.0;
    for h in &g.members {
        let e = d.entity(*h).unwrap();
        if let cadcraft_doc::EntityKind::Face3d(f) = &e.kind {
            let [a, b, c, _] = f.corners;
            v += a.dot(b.cross(c)) / 6.0;
        }
    }
    v
}

fn group_of(s: &Session, handle: &str) -> String {
    let d = s.doc().unwrap();
    let h = cadcraft_doc::Handle::parse_hex(handle).unwrap();
    d.groups.iter().find(|g| g.members.contains(&h)).unwrap().name.clone()
}

#[test]
fn exact_and_boolean_solids_survive_save_and_reopen() {
    hooks();
    let dir = std::env::temp_dir().join(format!("cadcraft-solid-save-reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("solids.dxf").to_string_lossy().to_string();

    let mut s = Session::new();
    let prism = s.execute("extrude", &json!({"profile": [[0, 0], [4, 0], [4, 3], [0, 3]], "height": 2})).unwrap();
    assert_eq!(prism["representation"], "analyticBrep");
    let block = handles(&s.execute("box", &json!({"p1": [10, -2, 0], "p2": [14, 2, 2]})).unwrap());
    let cyl = handles(&s.execute("cylinder", &json!({"center": [12, 0, -1], "radius": 1, "height": 4, "segments": 24})).unwrap());
    let holed = handles(&s.execute("subtract", &json!({"from": [block[0]], "remove": [cyl[0]]})).unwrap());
    let prism = handles(&prism);
    let (prism_group, holed_group) = (group_of(&s, &prism[0]), group_of(&s, &holed[0]));
    let (prism_volume, holed_volume) = (volume(&s, &prism_group), volume(&s, &holed_group));
    assert!((prism_volume - 24.0).abs() < 1e-9 && holed_volume > 25.0 && holed_volume < 26.0, "{prism_volume} {holed_volume}");
    let info = s.execute("solidinfo", &json!({"handles": [prism[0]]})).unwrap();
    s.execute("saveas", &json!({"path": path})).unwrap();

    let mut back = Session::new();
    back.execute("open", &json!({"path": path})).unwrap();
    let d = back.doc().unwrap();
    assert_eq!(d.groups.len(), 2);
    assert_eq!(d.groups.iter().map(|g| g.members.len()).sum::<usize>(), prism.len() + holed.len());
    assert!((volume(&back, &prism_group) - prism_volume).abs() < 1e-9);
    assert!((volume(&back, &holed_group) - holed_volume).abs() < 1e-9);
    // The exact body comes back unchanged and still edits as exact geometry.
    let reopened = back.execute("solidinfo", &json!({"handles": [prism[0]]})).unwrap();
    assert_eq!(reopened, info);
    let filleted = back.execute("filletedge", &json!({"handles": [prism[0]], "radius": 0.25})).unwrap();
    assert_eq!(filleted["representation"], "analyticBrep");
    // STEP holds exact solids only: the faceted boolean result is refused by name, not written
    // as triangles.
    let step = cadcraft_io::write(back.doc().unwrap(), "again.step").unwrap_err().to_string();
    assert!(step.contains("no exact source geometry"), "{step}");
    // The reopened boolean result is a closed solid that takes part in further booleans.
    let pin = handles(&back.execute("cylinder", &json!({"center": [10, -2, -1], "radius": 0.5, "height": 4, "segments": 12})).unwrap());
    back.execute("subtract", &json!({"from": [holed[0]], "remove": [pin[0]]})).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
