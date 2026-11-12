//! RECOVER on a DXF cut inside a group code / value pair (#3017): the pairs before the cut open
//! and the command says the file was truncated, in its reply and at the command line. Its own
//! test binary: the io hooks are process-wide, and this one installs the real reader.

#![cfg(not(target_arch = "wasm32"))]

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, base64_encode, set_io};
use serde_json::json;

#[test]
fn recover_opens_a_dxf_cut_inside_a_group_code_pair_and_says_it_was_truncated() {
    set_io(IoHooks { read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()), write: |_, _| Ok(Vec::new()), plot: None });
    // One whole LINE, then a second cut after its `21` group code: no value, no ENDSEC, no EOF.
    let data = b"0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n5\n21\n5\n0\nLINE\n8\n0\n10\n5\n20\n0\n11\n9\n21";
    let mut s = Session::new();
    let r = s.execute("recover", &json!({ "data": base64_encode(data), "name": "cut.dxf" })).unwrap();
    assert_eq!(r["index"], 1, "{r:#}");
    let warnings = r["warnings"].as_array().cloned().unwrap_or_default();
    assert_eq!(warnings.len(), 1, "{r:#}");
    assert!(warnings[0].as_str().unwrap().starts_with("DXF: the file was cut"), "{warnings:#?}");
    assert!(s.log.iter().any(|l| l.starts_with("DXF: the file was cut")), "{:?}", s.log);
    assert_eq!(s.state().unwrap().doc.model.len(), 2);
}
