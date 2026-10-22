//! EXPORT hands its JSON options (the SVG `text` mode) to the host's exporter (#1876).

use std::sync::Mutex;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, base64_decode, set_export_with, set_io};
use serde_json::json;

static SEEN: Mutex<Option<(String, String)>> = Mutex::new(None);

#[test]
fn export_forwards_the_text_option() {
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write: |_, _| Ok(b"plain".to_vec()), plot: None });
    set_export_with(|_, name, opts| {
        let text = opts.get("text").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
        if let Ok(mut seen) = SEEN.lock() {
            *seen = Some((name.to_string(), text));
        }
        Ok(b"exported with options".to_vec())
    });
    let mut s = Session::new();
    s.cmdline("line 0,0 10,0").unwrap();
    let r = s.execute("export", &json!({ "format": "svg", "text": "geometry" })).unwrap();
    assert_eq!(base64_decode(r["data"].as_str().unwrap()).unwrap(), b"exported with options");
    assert_eq!(*SEEN.lock().unwrap(), Some(("export.svg".to_string(), "geometry".to_string())));
}
