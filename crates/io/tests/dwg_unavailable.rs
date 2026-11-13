//! The targets without the native DWG bridge still handle DXF and report an actionable error
//! for DWG instead of pretending that opening or saving succeeded.

#![cfg(any(target_arch = "wasm32", all(target_os = "freebsd", target_arch = "aarch64")))]

use cadcraft_doc::Drawing;
use cadcraft_io::{read, write};

#[test]
fn no_dwg_bridge_preserves_dxf_and_reports_dwg_unavailable() {
    let drawing = Drawing::new_metric();
    let dxf = write(&drawing, "drawing.dxf").expect("DXF remains available");
    assert!(read(&dxf, "drawing.dxf").is_ok());

    let read_error = read(b"AC1032unavailable", "drawing.dwg").unwrap_err().to_string();
    let write_error = write(&drawing, "drawing.dwg").unwrap_err().to_string();
    for error in [read_error, write_error] {
        assert!(error.contains("DWG"), "{error}");
        assert!(error.contains("DXF"), "{error}");
    }
}
