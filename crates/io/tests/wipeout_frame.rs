//! The drawing's WIPEOUTVARIABLES object sets WIPEOUTFRAME: AutoCAD files with hidden wipeout
//! frames (group 70 = 0) must not show them.

fn dxf(frame: Option<i32>) -> String {
    let mut s = String::from("  0\nSECTION\n  2\nENTITIES\n  0\nENDSEC\n  0\nSECTION\n  2\nOBJECTS\n");
    if let Some(f) = frame {
        s.push_str(&format!("  0\nWIPEOUTVARIABLES\n  5\nA1\n100\nAcDbWipeoutVariables\n 70\n{f}\n"));
    }
    s.push_str("  0\nENDSEC\n  0\nEOF\n");
    s
}

#[test]
fn wipeoutvariables_sets_the_frame_flag() {
    let frame = |f| cadcraft_io::read_dxf(dxf(f).as_bytes()).unwrap().header.i64("WIPEOUTFRAME", 1);
    assert_eq!(frame(Some(0)), 0, "frames hidden");
    assert_eq!(frame(Some(1)), 1);
    assert_eq!(frame(Some(2)), 2);
    assert_eq!(frame(None), 1, "no object: the default shows frames");
    assert_eq!(frame(Some(7)), 1, "an out-of-range flag is ignored");
}
