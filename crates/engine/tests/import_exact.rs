//! IMPORT keeps the source drawing's groups, so exact solids from STEP, IGES or a CADCraft DXF
//! stay exact: editable, inspectable and exportable again, also when the import moves or scales
//! them (#2544 review).

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, base64_encode, set_io};
use serde_json::{Value, json};

fn hooks() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
}

/// A box made by the BOX command, written as `name` (its extension picks the format).
fn box_file(name: &str) -> Vec<u8> {
    let mut s = Session::new();
    s.execute("box", &json!({"p1": [0, 0, 0], "p2": [4, 3, 2]})).unwrap();
    cadcraft_io::write(s.doc().unwrap(), name).unwrap()
}

fn import(s: &mut Session, name: &str, extra: Value) -> Value {
    let mut p = json!({"data": base64_encode(&box_file(name)), "name": name});
    if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
        p.extend(extra.clone());
    }
    s.execute("import", &p).unwrap()
}

#[test]
fn imported_exact_solids_keep_their_group_and_stay_editable() {
    hooks();
    for name in ["part.step", "part.igs", "part.dxf"] {
        for extra in [json!({}), json!({"at": [100, 50], "scale": 2})] {
            let mut s = Session::new();
            // An existing unnamed group must not be overwritten by the imported one.
            s.execute("box", &json!({"p1": [-10, -10, 0], "p2": [-9, -9, 1]})).unwrap();
            let r = import(&mut s, name, extra.clone());
            assert_eq!(r["groups"], 1, "{name} {extra}: {r}");
            assert!(r.get("notes").is_none(), "{name} {extra}: {r}");
            let d = s.doc().unwrap();
            assert_eq!(d.groups.len(), 2);
            let names: std::collections::HashSet<_> = d.groups.iter().map(|g| g.name.to_ascii_uppercase()).collect();
            assert_eq!(names.len(), 2, "group names stay unique");
            let first = r["handles"][0].clone();
            let info = s.execute("solidinfo", &json!({"handles": [first]})).unwrap();
            assert_eq!(info["representation"], "analyticBrep", "{name} {extra}");
            if extra.get("at").is_some() {
                // Moved to 100,50 and scaled 2 in the plane: the exact vertices moved with the faces.
                let xs: Vec<f64> = info["edges"].as_array().unwrap().iter().map(|e| e["start"][0].as_f64().unwrap()).collect();
                assert!(xs.iter().all(|x| (100.0 - 1e-6..=108.0 + 1e-6).contains(x)), "{name}: {xs:?}");
            }
            // Editable and exportable again as exact geometry.
            let filleted = s.execute("filletedge", &json!({"handles": [r["handles"][0]], "radius": 0.2})).unwrap();
            assert_eq!(filleted["representation"], "analyticBrep", "{name} {extra}");
            s.execute("undo", &json!({})).unwrap();
            let again = cadcraft_io::write(s.doc().unwrap(), "again.step");
            assert!(again.is_ok(), "{name} {extra}: {again:?}");
        }
    }
}
