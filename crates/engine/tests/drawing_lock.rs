//! Drawing lock tests: AutoCAD-compatible `.dwl` and `.dwl2` locking, read-only enforcement and cleanup.

#![cfg(not(target_arch = "wasm32"))]

use cadcraft_doc::Drawing;
use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::drawing_lock::{dwl_path, dwl2_path};
use serde_json::json;

fn setup_io() {
    set_io(IoHooks {
        read: |_, _| Ok(Drawing::new_metric()),
        write: |_, _| Ok(b"0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n".to_vec()),
        plot: None,
    });
}

fn minimal_dxf() -> &'static [u8] {
    b"0\nSECTION\n2\nHEADER\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n"
}

#[test]
fn opening_drawing_creates_dwl_and_dwl2_and_closing_removes_them() {
    setup_io();
    let dir = std::env::temp_dir().join(format!("cadcraft-lock-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let dwg = dir.join("test_plan.dwg");
    std::fs::write(&dwg, minimal_dxf()).unwrap();

    let mut s = Session::new();
    let r = s.execute("open", &json!({ "path": dwg.to_string_lossy() })).unwrap();
    assert_eq!(r["read_only"], false);

    let dwl = dwl_path(&dwg);
    let dwl2 = dwl2_path(&dwg);

    assert!(dwl.is_file(), "dwl file must exist after opening");
    assert!(dwl2.is_file(), "dwl2 file must exist after opening");

    let dwl_content = std::fs::read_to_string(&dwl).unwrap();
    let lines: Vec<&str> = dwl_content.lines().collect();
    assert!(lines.len() >= 2, "dwl must have at least user and machine");

    let dwl2_content = std::fs::read_to_string(&dwl2).unwrap();
    assert!(dwl2_content.contains("<DrawingLock"));
    assert!(dwl2_content.contains("<UserName>"));
    assert!(dwl2_content.contains("<MachineName>"));

    // Closing the drawing must clean up both lock files
    s.execute("close", &json!({})).unwrap();
    assert!(!dwl.exists(), "dwl file must be removed after close");
    assert!(!dwl2.exists(), "dwl2 file must be removed after close");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn existing_lock_opens_drawing_read_only_and_qsave_is_blocked() {
    setup_io();
    let dir = std::env::temp_dir().join(format!("cadcraft-lock-conflict-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let dwg = dir.join("shared_plan.dwg");
    std::fs::write(&dwg, minimal_dxf()).unwrap();

    // Plant an existing .dwl file as if opened by colleague "bob" on "ENGINEERING-PC"
    let dwl = dwl_path(&dwg);
    let dwl2 = dwl2_path(&dwg);
    std::fs::write(&dwl, "bob\nENGINEERING-PC\nFriday, October 30, 2026 8:00:00 PM\n").unwrap();
    std::fs::write(
        &dwl2,
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<DrawingLock xmlns=\"dwl.xsd\">\n  <UserName>bob</UserName>\n  <MachineName>ENGINEERING-PC</MachineName>\n  <DateTime>2026-10-30T20:00:00</DateTime>\n</DrawingLock>\n",
    ).unwrap();

    let mut s = Session::new();
    let r = s.execute("open", &json!({ "path": dwg.to_string_lossy() })).unwrap();
    assert_eq!(r["read_only"], true, "drawing must open read-only when lock file exists");
    let info = r["lock_info"].as_str().unwrap();
    assert!(info.contains("bob"), "lock info should state bob holds the lock");
    assert!(info.contains("ENGINEERING-PC"));

    // Document state in session must be read_only
    let st = s.state().unwrap();
    assert!(st.read_only);

    // QSAVE must fail and report read-only
    let save_err = s.execute("qsave", &json!({})).unwrap_err();
    let msg = save_err.to_string();
    assert!(msg.contains("read-only"), "qsave error must state drawing is read-only: {msg}");
    assert!(msg.contains("SAVEAS"), "qsave error must instruct to use SAVEAS: {msg}");

    // SAVEAS to a new file should succeed and acquire a new lock on the new file
    let new_dwg = dir.join("my_copy.dwg");
    let saveas_res = s.execute("saveas", &json!({ "path": new_dwg.to_string_lossy() }));
    assert!(saveas_res.is_ok(), "SAVEAS must succeed for read-only drawing: {saveas_res:?}");

    let st_after = s.state().unwrap();
    assert!(!st_after.read_only, "new drawing after SAVEAS must no longer be read-only");
    assert!(dwl_path(&new_dwg).is_file(), "new file must have its own lock");

    // Original lock files for shared_plan.dwg must still remain untouched
    assert!(dwl.is_file(), "original dwl file must not be modified by another user's session");
    assert_eq!(std::fs::read_to_string(&dwl).unwrap(), "bob\nENGINEERING-PC\nFriday, October 30, 2026 8:00:00 PM\n");

    s.execute("close", &json!({})).unwrap();
    assert!(!dwl_path(&new_dwg).exists(), "my_copy lock must be cleaned up on close");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn saveas_to_already_locked_file_is_blocked() {
    setup_io();
    let dir = std::env::temp_dir().join(format!("cadcraft-lock-saveas-block-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let my_dwg = dir.join("my_plan.dwg");
    std::fs::write(&my_dwg, minimal_dxf()).unwrap();

    let locked_target = dir.join("colleague_plan.dwg");
    std::fs::write(&locked_target, minimal_dxf()).unwrap();
    std::fs::write(dwl_path(&locked_target), "alice\nPC-ALICE\nFriday\n").unwrap();

    let mut s = Session::new();
    s.execute("open", &json!({ "path": my_dwg.to_string_lossy() })).unwrap();

    // Trying to saveas over colleague's locked drawing must fail
    let err = s.execute("saveas", &json!({ "path": locked_target.to_string_lossy() })).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("write-protected"), "saveas error must report write-protected: {msg}");
    assert!(msg.contains("alice"), "saveas error must name lock owner: {msg}");

    // Target lock must remain intact
    assert!(dwl_path(&locked_target).is_file());

    s.execute("close", &json!({})).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn parse_dwl_with_bom_and_xml_special_chars() {
    use cadcraft_engine::drawing_lock::{parse_dwl, parse_dwl2};

    // DWL with UTF-8 BOM
    let dwl_bom = "\u{feff}john_doe\nMY-PC\nToday\n";
    let info = parse_dwl(dwl_bom).unwrap();
    assert_eq!(info.user, "john_doe");
    assert_eq!(info.machine, "MY-PC");

    // DWL2 with XML escaped characters
    let dwl2_xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<DrawingLock xmlns=\"dwl.xsd\">\n  <UserName>Tom &amp; Jerry</UserName>\n  <MachineName>&lt;SERVER&gt;</MachineName>\n  <DateTime>2026-10-30T20:00:00Z</DateTime>\n  <DrawingPath>C:\\Dev\\A &amp; B\\test.dwg</DrawingPath>\n</DrawingLock>\n";
    let info2 = parse_dwl2(dwl2_xml).unwrap();
    assert_eq!(info2.user, "Tom &amp; Jerry");
    assert_eq!(info2.machine, "&lt;SERVER&gt;");
}
