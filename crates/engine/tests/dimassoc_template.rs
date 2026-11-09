//! A saved template carries the drawing's DIMASSOC mode into a new drawing.
#![cfg(not(any(target_arch = "wasm32", all(target_os = "freebsd", target_arch = "aarch64"))))]

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use serde_json::json;

#[path = "support/temp_dir.rs"]
mod temp_dir;

#[test]
fn setvar_saveas_and_new_from_a_real_template_keep_dimassoc() {
    set_io(IoHooks {
        read: |bytes, name| cadcraft_io::read(bytes, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    let dir = temp_dir::TempDir::new("dimassoc-template");
    for mode in [0, 1, 2] {
        let mut s = Session::new();
        s.execute("setvar", &json!({"name": "DIMASSOC", "value": mode})).unwrap();
        let path = dir.path_str(&format!("mode-{mode}.dwt"));
        s.execute("saveas", &json!({"path": path})).unwrap();
        s.execute("new", &json!({"template": path})).unwrap();
        assert_eq!(s.execute("getvar", &json!({"name": "DIMASSOC"})).unwrap()["value"], mode);
        assert!(s.state().unwrap().path.is_none(), "the new drawing is independent from its template");
        s.execute("setvar", &json!({"name": "DIMASSOC", "value": (mode + 1) % 3})).unwrap();
        s.execute("new", &json!({"template": path})).unwrap();
        assert_eq!(s.execute("getvar", &json!({"name": "DIMASSOC"})).unwrap()["value"], mode, "editing the new drawing leaves the template intact");
    }
}
