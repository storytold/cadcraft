//! Drawing templates (#552): a `.dwt` is written as DWG content and read back, and a template
//! named `.dwt` that holds DXF text reads too.

use cadcraft_doc::{Drawing, Layer};
use cadcraft_io::{read, write};

fn office() -> Drawing {
    let mut d = Drawing::new_metric();
    d.layers.push(Layer::new("A-WALL"));
    d
}

#[test]
fn dwt_is_written_as_dwg_and_read_back() {
    let bytes = write(&office(), "office.dwt").unwrap();
    assert!(bytes.starts_with(b"AC10"), "a template is DWG content");
    let back = read(&bytes, "office.dwt").unwrap();
    assert!(back.layers.iter().any(|l| l.name == "A-WALL"));
    assert_eq!(back.header.i64("MEASUREMENT", 0), 1);
}

#[test]
fn a_dxf_template_reads_under_either_name() {
    let dxf = write(&office(), "office.dxf").unwrap();
    for name in ["office.dxf", "office.dwt"] {
        assert!(read(&dxf, name).unwrap().layers.iter().any(|l| l.name == "A-WALL"), "{name}");
    }
}
