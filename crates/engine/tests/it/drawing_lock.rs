//! Integration tests for AutoCAD-compatible `.dwl`/`.dwl2` drawing file locking.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::atomic::{AtomicU64, Ordering};

use cadcraft_engine::drawing_lock::{self, current_machine, current_user, dwl_path, dwl2_path};
use cadcraft_engine::{Session, cmd};
use serde_json::json;

static TEST_SEQ: AtomicU64 = AtomicU64::new(1);

fn install_io() {
    cmd::file::set_io(cmd::file::IoHooks {
        read: |bytes, path| {
            if path.ends_with(".dwg") || path.ends_with(".dxf") {
                Ok(cadcraft_doc::Drawing::new_metric())
            } else {
                Err(format!("unsupported format: {path} ({} bytes)", bytes.len()))
            }
        },
        write: |_drawing, _path| Ok(b"MOCK_DRAWING_BYTES".to_vec()),
        plot: None,
    });
}

fn fresh_temp_dir(label: &str) -> std::path::PathBuf {
    let n = TEST_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("cadcraft-lock-it-{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn open_dwg_acquires_whohas_lock_and_close_releases_it() {
    install_io();
    let dir = fresh_temp_dir("lifecycle");
    let dwg = dir.join("site.dwg");
    std::fs::write(&dwg, b"MOCK_DWG").expect("write mock dwg");
    let dwg_str = dwg.to_string_lossy().to_string();

    let mut s = Session::new();
    let res = s.execute("open", &json!({ "path": dwg_str })).expect("open dwg");
    assert_eq!(res["read_only"], json!(false));

    assert!(dwl_path(&dwg).is_file(), ".dwl must exist while drawing is open");
    assert!(dwl2_path(&dwg).is_file(), ".dwl2 must exist while drawing is open");

    let dwl2_xml = std::fs::read_to_string(dwl2_path(&dwg)).expect("read dwl2");
    assert!(dwl2_xml.contains("<whohas>"));
    assert!(dwl2_xml.contains("<username>"));
    assert!(dwl2_xml.contains("<machinename>"));
    assert!(dwl2_xml.contains("<datetime>"));
    assert!(dwl2_xml.contains(&format!("<pid>{}</pid>", std::process::id())));

    s.execute("qsave", &json!({})).expect("qsave should succeed on locked writable drawing");

    let idx = res["index"].as_u64().expect("doc index");
    s.execute("close", &json!({ "index": idx })).expect("close dwg");

    assert!(!dwl_path(&dwg).exists(), ".dwl must be deleted on close");
    assert!(!dwl2_path(&dwg).exists(), ".dwl2 must be deleted on close");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn locked_dwg_opens_read_only_blocks_qsave_and_blocks_overwrite() {
    install_io();
    let dir = fresh_temp_dir("readonly");
    let dwg = dir.join("plan.dwg");
    std::fs::write(&dwg, b"MOCK_DWG").expect("write mock dwg");
    let dwg_str = dwg.to_string_lossy().to_string();

    std::fs::write(dwl_path(&dwg), "alice\r\nCAD-PC\r\n2026-11-25 10:00:00 UTC\r\n").expect("write dwl");
    std::fs::write(
        dwl2_path(&dwg),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<whohas>\r\n  <username>alice</username>\r\n  <machinename>CAD-PC</machinename>\r\n  <datetime>2026-11-25 10:00:00 UTC</datetime>\r\n</whohas>\r\n",
    )
    .expect("write dwl2");

    let mut s = Session::new();
    let res = s.execute("open", &json!({ "path": dwg_str })).expect("open locked dwg");
    assert_eq!(res["read_only"], json!(true));
    assert_eq!(res["lock_holder"]["user"], json!("alice"));
    assert_eq!(res["lock_holder"]["machine"], json!("CAD-PC"));

    let qsave_err = s.execute("qsave", &json!({})).expect_err("qsave must fail on read-only drawing");
    assert!(qsave_err.to_string().contains("read-only"), "unexpected error: {qsave_err}");

    let saveas_err = s.execute("saveas", &json!({ "path": dwg_str })).expect_err("saveas over locked file must fail");
    assert!(saveas_err.to_string().contains("write-protected"), "unexpected error: {saveas_err}");

    let idx = res["index"].as_u64().expect("index");
    s.execute("close", &json!({ "index": idx })).expect("close");
    // Closing a read-only drawing must NOT delete another user's lock files.
    assert!(dwl_path(&dwg).is_file());
    assert!(dwl2_path(&dwg).is_file());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stale_lock_from_dead_local_process_is_reclaimed_automatically() {
    install_io();
    let dir = fresh_temp_dir("stale");
    let dwg = dir.join("crashed.dwg");
    std::fs::write(&dwg, b"MOCK_DWG").expect("write mock dwg");
    let dwg_str = dwg.to_string_lossy().to_string();

    let user = current_user();
    let machine = current_machine();
    std::fs::write(dwl_path(&dwg), format!("{user}\r\n{machine}\r\n2026-11-24 09:00:00 UTC\r\n")).expect("write stale dwl");
    std::fs::write(
        dwl2_path(&dwg),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<whohas>\r\n  <username>{user}</username>\r\n  <machinename>{machine}</machinename>\r\n  <datetime>2026-11-24 09:00:00 UTC</datetime>\r\n  <pid>999999999</pid>\r\n</whohas>\r\n"
        ),
    )
    .expect("write stale dwl2");

    let mut s = Session::new();
    let res = s.execute("open", &json!({ "path": dwg_str })).expect("open stale-locked dwg");
    assert_eq!(res["read_only"], json!(false), "stale lock from dead local PID must be reclaimed automatically");

    let info = drawing_lock::read_lock_info(&dwg).expect("fresh lock info");
    assert_eq!(info.pid, Some(std::process::id()));
    s.execute("qsave", &json!({})).expect("qsave must succeed after reclaiming stale lock");

    let idx = res["index"].as_u64().expect("index");
    s.execute("close", &json!({ "index": idx })).expect("close");
    assert!(!dwl_path(&dwg).exists());
    assert!(!dwl2_path(&dwg).exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn take_over_lock_switches_read_only_drawing_to_writable() {
    install_io();
    let dir = fresh_temp_dir("takeover");
    let dwg = dir.join("shared.dwg");
    std::fs::write(&dwg, b"MOCK_DWG").expect("write mock dwg");
    let dwg_str = dwg.to_string_lossy().to_string();

    std::fs::write(dwl_path(&dwg), "other_user\r\nOTHER-PC\r\n2026-11-25 08:00:00 UTC\r\n").expect("write dwl");
    std::fs::write(
        dwl2_path(&dwg),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<whohas>\r\n  <username>other_user</username>\r\n  <machinename>OTHER-PC</machinename>\r\n  <datetime>2026-11-25 08:00:00 UTC</datetime>\r\n</whohas>\r\n",
    )
    .expect("write dwl2");

    let mut s = Session::new();
    let res = s.execute("open", &json!({ "path": dwg_str })).expect("open locked dwg");
    assert_eq!(res["read_only"], json!(true));
    let idx = res["index"].as_u64().expect("index") as usize;

    s.take_over_lock(idx).expect("take_over_lock should succeed");
    assert!(!s.docs[idx].read_only);
    assert!(s.docs[idx].lock.is_some());
    assert!(s.docs[idx].lock_info.is_none());
    assert!(s.docs[idx].lock_holder.is_none());

    let info = drawing_lock::read_lock_info(&dwg).expect("updated lock info");
    assert_eq!(info.user, current_user());
    assert_eq!(info.pid, Some(std::process::id()));

    s.execute("qsave", &json!({})).expect("qsave must succeed after taking over lock");

    s.execute("close", &json!({ "index": idx })).expect("close");
    assert!(!dwl_path(&dwg).exists());
    assert!(!dwl2_path(&dwg).exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn saveas_read_only_dwg_to_dxf_resets_read_only_and_allows_qsave() {
    install_io();
    let dir = fresh_temp_dir("saveas-dxf");
    let dwg = dir.join("locked.dwg");
    let dxf = dir.join("copy.dxf");
    std::fs::write(&dwg, b"MOCK_DWG").expect("write mock dwg");
    std::fs::write(dwl_path(&dwg), "alice\r\nCAD-PC\r\n2026-11-25 10:00:00 UTC\r\n").expect("write dwl");

    let mut s = Session::new();
    let res = s.execute("open", &json!({ "path": dwg.to_string_lossy() })).expect("open locked dwg");
    assert_eq!(res["read_only"], json!(true));

    s.execute("saveas", &json!({ "path": dxf.to_string_lossy() })).expect("saveas to dxf must succeed");
    let st = s.state().expect("active state");
    assert!(!st.read_only, "SAVEAS to .dxf must clear read_only");
    assert!(st.lock_info.is_none());
    assert!(st.lock_holder.is_none());
    assert!(st.lock.is_none());
    assert!(!dwl_path(&dxf).exists(), ".dxf must not create .dwl");
    assert!(!dwl2_path(&dxf).exists(), ".dxf must not create .dwl2");

    // Subsequent QSAVE on the new .dxf copy must succeed.
    s.execute("qsave", &json!({})).expect("qsave on copy.dxf must succeed");

    // Original drawing's lock files remain untouched.
    assert!(dwl_path(&dwg).is_file());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn saveas_read_only_dwg_to_new_dwg_acquires_lock_and_allows_qsave() {
    install_io();
    let dir = fresh_temp_dir("saveas-dwg");
    let orig = dir.join("orig.dwg");
    let copy = dir.join("copy.dwg");
    std::fs::write(&orig, b"MOCK_DWG").expect("write orig dwg");
    std::fs::write(dwl_path(&orig), "alice\r\nCAD-PC\r\n2026-11-25 10:00:00 UTC\r\n").expect("write orig dwl");

    let mut s = Session::new();
    let res = s.execute("open", &json!({ "path": orig.to_string_lossy() })).expect("open orig dwg");
    assert_eq!(res["read_only"], json!(true));

    s.execute("saveas", &json!({ "path": copy.to_string_lossy() })).expect("saveas copy.dwg");
    let st = s.state().expect("active state");
    assert!(!st.read_only);
    assert!(st.lock.is_some());
    assert!(dwl_path(&copy).is_file());
    assert!(dwl2_path(&copy).is_file());
    assert!(dwl_path(&orig).is_file(), "original lock must remain untouched");

    s.execute("qsave", &json!({})).expect("qsave on copy.dwg must succeed");

    let idx = s.active;
    s.execute("close", &json!({ "index": idx })).expect("close");
    assert!(!dwl_path(&copy).exists());
    assert!(!dwl2_path(&copy).exists());
    assert!(dwl_path(&orig).is_file());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn take_over_lock_in_same_session_demotes_previous_tab_and_preserves_lock_on_close() {
    install_io();
    let dir = fresh_temp_dir("same-session-takeover");
    let dwg = dir.join("multi.dwg");
    std::fs::write(&dwg, b"MOCK_DWG").expect("write mock dwg");
    let dwg_str = dwg.to_string_lossy().to_string();

    let mut s = Session::new();
    let r0 = s.execute("open", &json!({ "path": dwg_str })).expect("open tab 0");
    assert_eq!(r0["read_only"], json!(false));
    let idx0 = r0["index"].as_u64().expect("idx0") as usize;

    let r1 = s.execute("open", &json!({ "path": dwg_str })).expect("open tab 1");
    assert_eq!(r1["read_only"], json!(true));
    let idx1 = r1["index"].as_u64().expect("idx1") as usize;

    s.take_over_lock(idx1).expect("tab 1 takes over lock");
    assert!(s.docs[idx0].read_only, "tab 0 must be demoted to read-only");
    assert!(s.docs[idx0].lock.is_none(), "tab 0 must drop its lock guard");
    assert!(!s.docs[idx1].read_only, "tab 1 must become writable");
    assert!(s.docs[idx1].lock.is_some(), "tab 1 must hold the new lock guard");

    // Closing the demoted tab 0 must NOT delete tab 1's lock files.
    s.execute("close", &json!({ "index": idx0 })).expect("close tab 0");
    assert!(dwl_path(&dwg).is_file(), "tab 1 .dwl must survive closing tab 0");
    assert!(dwl2_path(&dwg).is_file(), "tab 1 .dwl2 must survive closing tab 0");

    // Closing tab 1 (shifted left by one after closing tab 0) must release the lock files cleanly.
    s.execute("close", &json!({ "index": idx1 - 1 })).expect("close tab 1");
    assert!(!dwl_path(&dwg).exists());
    assert!(!dwl2_path(&dwg).exists());

    let _ = std::fs::remove_dir_all(&dir);
}
