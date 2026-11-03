//! A drawing path that isn't valid Unicode is refused: the session keeps paths as text, so it
//! could never be saved back to.

#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;

use cadcraft_doc::Drawing;
use cadcraft_engine::cmd::file::{IoHooks, read_file, set_io};

#[test]
fn read_file_refuses_a_name_that_is_not_unicode() {
    set_io(IoHooks { read: |_, _| Ok(Drawing::new_metric()), write: |_, _| Ok(Vec::new()), plot: None });
    let dir = std::env::temp_dir().join(format!("cadcraft-nonunicode-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let good = dir.join("good.dxf");
    let bad = dir.join(OsString::from_vec(b"bad-\xff.dxf".to_vec()));
    std::fs::write(&good, b"0\nEOF\n").unwrap();
    std::fs::write(&bad, b"0\nEOF\n").unwrap();
    let e = read_file(&bad).err().expect("a non-Unicode name is refused");
    assert!(e.contains("not valid Unicode") && e.contains("bad-"), "{e}");
    assert!(read_file(&good).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}
