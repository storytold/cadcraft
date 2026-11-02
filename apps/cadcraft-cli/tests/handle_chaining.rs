//! A `--cmd` can act on a handle an earlier `--cmd` in the same run returned (#1663).
//!
//! `mview` answers `{"viewports":["100"]}`, but a following `viewport.set` could not use that
//! handle: it had to be guessed, selected by point, or activated with `mspace`. A `--cmd` parameter
//! value of exactly `{last}` is now filled in with the last handle an earlier `--cmd` returned.

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cadcraft-cli"))
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("run cadcraft-cli")
}

/// The issue's sequence: `viewport.set` straight after `mview`, addressing the new viewport by the
/// handle the runner remembers.
#[test]
fn viewport_set_after_mview_uses_the_returned_handle() {
    let out = run(&[
        "run",
        "--metric",
        "--cmd",
        r#"layout.set {"name":"Layout1"}"#,
        "--cmd",
        r#"mview {"p1":[20,20],"p2":[200,150]}"#,
        "--cmd",
        r#"viewport.set {"handle":"{last}","scale":1}"#,
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "exit {:?}\nstdout: {stdout}\nstderr: {stderr}", out.status);
    let mview = stdout.lines().find(|l| l.contains("\"viewports\"")).unwrap_or_else(|| panic!("no mview result in {stdout:?}"));
    let created: serde_json::Value = serde_json::from_str(mview).expect("mview result is JSON");
    let handle = created["viewports"].as_array().and_then(|a| a.first()).and_then(|h| h.as_str()).expect("a viewport handle");
    assert!(handle.eq_ignore_ascii_case("100"), "{mview}");
    let set = stdout.lines().find(|l| l.contains("\"viewHeight\"")).unwrap_or_else(|| panic!("no viewport.set result in {stdout:?}"));
    let vp: serde_json::Value = serde_json::from_str(set).expect("viewport.set result is JSON");
    assert_eq!(vp[0]["handle"].as_str(), Some(handle), "viewport.set changed the viewport `{handle}`: {set}");
    assert_eq!(vp[0]["scale"].as_f64(), Some(1.0), "{set}");
}

/// `{last}` with no handle to fill in is a clear error, not the token reaching the command.
#[test]
fn last_without_an_earlier_handle_is_an_error() {
    let out = run(&["run", "--metric", "--cmd", r#"viewport.set {"handle":"{last}","scale":1}"#]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stdout: {:?}\nstderr: {stderr}", String::from_utf8_lossy(&out.stdout));
    assert!(stderr.contains("{last}"), "the error names the placeholder: {stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

/// The last JSON value a run printed: command results are one JSON value a line (`null` is not
/// printed).
fn last_json(stdout: &str) -> serde_json::Value {
    let line = stdout
        .lines()
        .rev()
        .find(|l| {
            let t = l.trim_start();
            t.starts_with('{') || t.starts_with('[')
        })
        .unwrap_or_else(|| panic!("no JSON result in {stdout:?}"));
    serde_json::from_str(line).expect("command result is JSON")
}

/// `{last}` was not replaced: the error names the placeholder, and the run fails cleanly.
fn assert_needs_an_earlier_handle(out: &Output) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stdout: {:?}\nstderr: {stderr}", String::from_utf8_lossy(&out.stdout));
    assert!(stderr.contains("{last}"), "the error names the placeholder: {stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

/// A user-defined name that happens to look like a handle is not a returned handle. `group`
/// answers `{"name":"100","members":1}`: `100` is the first line's handle, but the name is not an
/// object reference and must not replace the last handle the `line` command returned (`101`).
#[test]
fn a_group_name_that_looks_like_a_handle_is_not_last() {
    let out = run(&[
        "run",
        "--metric",
        "--cmd",
        r#"line {"points":[[0,0],[10,0]]}"#,
        "--cmd",
        r#"line {"points":[[0,10],[10,10]]}"#,
        "--cmd",
        r#"group {"name":"100","handles":["101"]}"#,
        "--cmd",
        r#"erase {"handles":["{last}"]}"#,
        "--cmd",
        r#"entities {"type":"LINE"}"#,
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "exit {:?}\nstdout: {stdout}\nstderr: {stderr}", out.status);
    let r = last_json(&stdout);
    assert_eq!(r["count"].as_u64(), Some(1), "one line is left: {stdout}");
    let left: Vec<&str> = r["entities"].as_array().into_iter().flatten().filter_map(|e| e["handle"].as_str()).collect();
    assert_eq!(left, ["100"], "the group name 100 did not replace the handle 101 the line command returned: {stdout}");
}

/// `new` makes another drawing current: the handle an earlier `--cmd` returned in the old drawing
/// is not a handle of the new one.
#[test]
fn last_is_cleared_when_new_switches_drawings() {
    let out = run(&[
        "run",
        "--metric",
        "--cmd",
        r#"line {"points":[[0,0],[10,0]]}"#,
        "--cmd",
        r#"new {"metric":true}"#,
        "--cmd",
        r#"erase {"handles":["{last}"]}"#,
    ]);
    assert_needs_an_earlier_handle(&out);
}

/// `open` makes the opened file the current drawing: a handle from the drawing before it is stale.
#[test]
fn last_is_cleared_when_open_switches_drawings() {
    let dir = std::env::temp_dir().join(format!("cc-cli-last-open-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let dxf = dir.join("opened.dxf");
    let sample = run(&["sample", "bracket", dxf.to_str().expect("temp path")]);
    assert!(sample.status.success(), "sample failed: {}", String::from_utf8_lossy(&sample.stderr));
    let open = format!(r#"open {{"path":{}}}"#, serde_json::to_string(dxf.to_str().expect("temp path")).expect("path JSON"));
    let out = run(&["run", "--metric", "--cmd", r#"line {"points":[[0,0],[10,0]]}"#, "--cmd", &open, "--cmd", r#"erase {"handles":["{last}"]}"#]);
    assert_needs_an_earlier_handle(&out);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `document.switch` changes the current drawing without adding one: a handle remembered from the
/// drawing left behind must not be used in the one switched to.
#[test]
fn last_is_cleared_when_document_switch_changes_drawings() {
    let out = run(&[
        "run",
        "--metric",
        "--cmd",
        r#"line {"points":[[0,0],[10,0]]}"#,
        "--cmd",
        r#"new {"metric":true}"#,
        "--cmd",
        r#"line {"points":[[0,10],[10,10]]}"#,
        "--cmd",
        r#"document.switch {"index":0}"#,
        "--cmd",
        r#"erase {"handles":["{last}"]}"#,
    ]);
    assert_needs_an_earlier_handle(&out);
}

/// A handle a command returns after a switch belongs to the drawing now current, so it is kept.
#[test]
fn a_handle_returned_after_a_switch_is_last() {
    let out = run(&[
        "run",
        "--metric",
        "--cmd",
        r#"line {"points":[[0,0],[10,0]]}"#,
        "--cmd",
        r#"new {"metric":true}"#,
        "--cmd",
        r#"document.switch {"index":0}"#,
        "--cmd",
        r#"line {"points":[[0,10],[10,10]]}"#,
        "--cmd",
        r#"erase {"handles":["{last}"]}"#,
        "--cmd",
        r#"entities {"type":"LINE"}"#,
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "exit {:?}\nstdout: {stdout}\nstderr: {stderr}", out.status);
    let r = last_json(&stdout);
    assert_eq!(r["count"].as_u64(), Some(1), "one line is left: {stdout}");
    let left: Vec<&str> = r["entities"].as_array().into_iter().flatten().filter_map(|e| e["handle"].as_str()).collect();
    assert_eq!(left, ["100"], "the line created after the switch was erased: {stdout}");
}

/// A script can change the current drawing too (`new {"metric":true}` at a script line), so the
/// remembered handle is cleared after a script that changed it.
#[test]
fn last_is_cleared_by_a_script_that_changes_drawings() {
    let out = run(&[
        "run",
        "--metric",
        "--cmd",
        r#"line {"points":[[0,0],[10,0]]}"#,
        "--script",
        r#"new {"metric":true}"#,
        "--cmd",
        r#"erase {"handles":["{last}"]}"#,
    ]);
    assert_needs_an_earlier_handle(&out);
}

/// Only the exact token is a placeholder: ordinary JSON strings are left alone.
#[test]
fn ordinary_values_are_not_placeholders() {
    let dir = std::env::temp_dir().join(format!("cc-cli-last-placeholder-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let out_dxf = dir.join("text.dxf");
    let out = run(&[
        "run",
        "--metric",
        "--cmd",
        r#"layout.set {"name":"Layout1"}"#,
        "--cmd",
        r#"mview {"p1":[20,20],"p2":[200,150]}"#,
        "--cmd",
        r#"text {"at":[0,0],"height":2,"text":"last and {last}"}"#,
        "--save",
        out_dxf.to_str().expect("temp path"),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "exit {:?}\nstderr: {stderr}", out.status);
    let saved = std::fs::read_to_string(&out_dxf).expect("saved DXF");
    assert!(saved.contains("last and {last}"), "the text is stored literally");
    let _ = std::fs::remove_dir_all(&dir);
}
