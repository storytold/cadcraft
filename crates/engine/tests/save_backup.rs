//! Saving over a drawing keeps the previous version as a backup (`plan.dxf.bak`), but a damaged
//! file never replaces a good backup (#540, phase 0).
#![cfg(not(target_arch = "wasm32"))]

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use serde_json::json;

fn hooks() {
    set_io(IoHooks {
        read: |b, _| if b.starts_with(b"BAD") { Err("damaged".into()) } else { Ok(cadcraft_engine::doc::Drawing::new_metric()) },
        write: |_, _| Ok(b"NEW".to_vec()),
        plot: None,
    });
}

#[test]
fn save_keeps_the_previous_version_as_bak_unless_it_is_damaged() {
    hooks();
    let d = std::env::temp_dir().join(format!("cadcraft-save-bak-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let path = d.join("plan.dxf");
    let p = path.to_string_lossy().to_string();

    std::fs::write(&path, b"GOOD").unwrap();
    let mut s = Session::new();
    s.execute("saveas", &json!({ "path": p })).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"NEW");
    assert_eq!(std::fs::read(d.join("plan.dxf.bak")).unwrap(), b"GOOD");

    // The file is damaged behind our back; saving again must keep the good backup.
    std::fs::write(&path, b"BAD bytes").unwrap();
    s.execute("saveas", &json!({ "path": p })).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"NEW");
    assert_eq!(std::fs::read(d.join("plan.dxf.bak")).unwrap(), b"GOOD");

    // No staging file is left behind.
    let mut names: Vec<_> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    names.sort();
    assert_eq!(names, ["plan.dxf", "plan.dxf.bak"]);
    let _ = std::fs::remove_dir_all(&d);
}
