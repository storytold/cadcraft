//! A drawing the host opened from bytes has a name but no path: where files are delivered to a
//! host (the web embed), QSAVE delivers it under that name, as SAVEAS does.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_deliver, set_io};
use cadcraft_engine::doc::Drawing;
use serde_json::json;
use std::sync::Mutex;

static DELIVERED: Mutex<Vec<(String, Vec<u8>)>> = Mutex::new(Vec::new());

fn deliver(name: &str, _mime: &str, bytes: &[u8]) -> Result<(), String> {
    DELIVERED.lock().map_err(|e| e.to_string())?.push((name.to_string(), bytes.to_vec()));
    Ok(())
}

#[test]
fn qsave_delivers_a_host_opened_drawing_under_its_name() {
    set_io(IoHooks { read: |_, _| Ok(Drawing::new_metric()), write: |_, _| Ok(b"exported".to_vec()), plot: None });
    set_deliver(deliver);
    let mut s = Session::empty();
    s.execute("open", &json!({"data": "MA==", "name": "plan.dxf"})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    s.execute("qsave", &json!({})).expect("QSAVE delivers the opened drawing");
    let got = DELIVERED.lock().unwrap().clone();
    assert_eq!(got, vec![("plan.dxf".to_string(), b"exported".to_vec())]);
    assert!(!s.state().unwrap().is_dirty());

    // An imported SVG is not a drawing file: QSAVE still asks for a name.
    s.execute("open", &json!({"data": "MA==", "name": "Logo.svg"})).unwrap();
    assert!(s.execute("qsave", &json!({})).is_err());
}
