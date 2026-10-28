//! A DWG that AutoCAD saved, opened and saved again by CADCraft, keeps every object: each object of
//! the file is in the saved file under the same handle, with the same type, as CADCraft's own DWG
//! reader (`cadcraft_dwg::codec`) sees both files. Needs the MI-201 corpus (one part AutoCAD saved
//! in every version, not part of the repository): `MI201_CORPUS` names its folder.

use std::collections::BTreeMap;

use cadcraft_dwg::codec::{DwgContainer, read_container, type_name};

/// Each object's type by handle.
fn types(c: &DwgContainer) -> BTreeMap<u64, String> {
    c.objects_iter().filter_map(|(r, o)| o.ok().map(|o| (r.handle, type_name(o.type_code, &c.classes).0))).collect()
}

#[test]
fn autocad_dwgs_keep_every_objects_handle_and_type() {
    let Some(dir) = std::env::var_os("MI201_CORPUS").map(std::path::PathBuf::from).filter(|d| d.is_dir()) else { return };
    // Saved in their own version. The 2000 file's VX table control has no place in the 2004 and
    // later structure the bridge writes.
    for (name, may_go) in [
        ("mi201-r2018.dwg", &[][..]),
        ("mi201-r2013.dwg", &[]),
        ("mi201-r2010.dwg", &[]),
        ("mi201-r2004.dwg", &[]),
        ("mi201-r2000.dwg", &["VX_CONTROL"]),
    ] {
        let Ok(bytes) = std::fs::read(dir.join(name)) else { continue };
        let d = cadcraft_io::read(&bytes, name).unwrap();
        let saved = cadcraft_io::write(&d, name).unwrap();
        let (before, after) = (types(&read_container(&bytes).unwrap()), types(&read_container(&saved).unwrap()));
        let lost: Vec<String> = before
            .iter()
            .filter(|(h, t)| after.get(h) != Some(t) && !may_go.contains(&t.as_str()))
            .map(|(h, t)| format!("{h:X} {t} -> {:?}", after.get(h)))
            .collect();
        assert!(lost.is_empty(), "{name}: {} of {} objects lost their handle or type: {lost:?}", lost.len(), before.len());
    }
}
