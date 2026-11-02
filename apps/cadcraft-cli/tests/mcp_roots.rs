//! `cadcraft-cli mcp --automation-read-root <dir> --automation-write-root <dir>` (#1472): through
//! the MCP protocol, files open only from inside the read root and are saved, exported, plotted and
//! rendered only inside the write root; `..` and links that leave a root are refused, and nothing is
//! written outside. Without roots nothing changes.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Never crash).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#;

/// A folder holding `in` (the read root, with `a.dxf`), `out` (the write root), `outside` (with
/// `b.dxf`) and `in2` (a sibling with the same prefix as `in`).
fn folders(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("cc-mcp-roots-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    for d in ["in", "out", "outside", "in2"] {
        std::fs::create_dir_all(base.join(d)).unwrap();
    }
    for p in ["in/a.dxf", "outside/b.dxf", "in2/c.dxf"] {
        let drawing = cadcraft_engine::doc::Drawing::new_imperial();
        std::fs::write(base.join(p), cadcraft_io::write(&drawing, "x.dxf").unwrap()).unwrap();
    }
    base
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// One tool call's result: `isError` and its first text block.
struct Tool {
    error: bool,
    text: String,
}

/// Run `cadcraft-cli mcp` with `args`, initialise, then make each call in order.
fn run(args: &[&str], calls: &[(&str, Value)]) -> (Output, Vec<Tool>) {
    let mut input = format!("{INIT}\n");
    for (i, (name, arguments)) in calls.iter().enumerate() {
        let req = json!({"jsonrpc": "2.0", "id": i + 2, "method": "tools/call", "params": {"name": name, "arguments": arguments}});
        input.push_str(&req.to_string());
        input.push('\n');
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_cadcraft-cli"))
        .arg("mcp")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cadcraft-cli");
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).unwrap();
    }
    let out = child.wait_with_output().expect("wait for cadcraft-cli");
    let replies: Vec<Value> = String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let tools = calls
        .iter()
        .enumerate()
        .map(|(i, (name, arguments))| {
            let id = (i + 2) as u64;
            let reply = replies.iter().find(|v| v["id"] == id).unwrap_or_else(|| panic!("no reply to {name} {arguments}: {replies:?}"));
            let result = &reply["result"];
            Tool { error: result["isError"] == true, text: result["content"][0]["text"].as_str().unwrap_or_default().to_string() }
        })
        .collect();
    (out, tools)
}

fn ok(t: &Tool) {
    assert!(!t.error, "should be allowed: {}", t.text);
}

fn refused(t: &Tool, why: &str) {
    assert!(t.error, "should be refused ({why}): {}", t.text);
    assert!(t.text.contains(why), "should say `{why}`: {}", t.text);
}

#[test]
fn files_open_only_from_inside_the_read_root() {
    let base = folders("open");
    let (out, t) = run(
        &["--automation-read-root", &s(&base.join("in")), "--automation-write-root", &s(&base.join("out"))],
        &[
            ("open", json!({"path": s(&base.join("in/a.dxf"))})),
            ("open", json!({"path": s(&base.join("outside/b.dxf"))})),
            ("open", json!({"path": s(&base.join("in2/c.dxf"))})),
            ("open", json!({"path": format!("{}/../outside/b.dxf", s(&base.join("in")))})),
            ("execute", json!({"command": "circle", "params": {"center": [0, 0], "radius": 1}})),
        ],
    );
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    ok(&t[0]);
    refused(&t[1], "outside the read root");
    refused(&t[2], "outside the read root");
    refused(&t[3], "outside the read root");
    // A refused path is an ordinary tool error; the session keeps working.
    ok(&t[4]);
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn writes_go_only_into_the_write_root_and_leave_no_temp_file_outside() {
    let base = folders("write");
    let (out, t) = run(
        &["--automation-read-root", &s(&base.join("in")), "--automation-write-root", &s(&base.join("out"))],
        &[
            ("open", json!({"path": s(&base.join("in/a.dxf"))})),
            ("save", json!({"path": s(&base.join("out/saved.dxf"))})),
            ("save", json!({"path": s(&base.join("outside/saved.dxf"))})),
            ("save", json!({"path": s(&base.join("in/a.dxf"))})),
            ("save", json!({"path": format!("{}/../outside/up.dxf", s(&base.join("out")))})),
            ("execute", json!({"command": "export", "params": {"path": s(&base.join("out/art.png")), "format": "png"}})),
            ("execute", json!({"command": "export", "params": {"path": s(&base.join("outside/art.png")), "format": "png"}})),
            ("render", json!({"path": s(&base.join("out/view.png"))})),
            ("render", json!({"path": s(&base.join("outside/view.png"))})),
            ("execute", json!({"command": "circle", "params": {"center": [0, 0], "radius": 1}})),
        ],
    );
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    ok(&t[0]);
    ok(&t[1]);
    assert!(base.join("out/saved.dxf").is_file());
    refused(&t[2], "outside the write root");
    refused(&t[3], "outside the write root");
    refused(&t[4], "outside the write root");
    ok(&t[5]);
    assert!(base.join("out/art.png").is_file());
    refused(&t[6], "outside the write root");
    ok(&t[7]);
    assert!(base.join("out/view.png").is_file());
    refused(&t[8], "outside the write root");
    ok(&t[9]);

    assert!(!base.join("outside/saved.dxf").exists());
    assert!(!base.join("outside/art.png").exists());
    assert!(!base.join("outside/view.png").exists());
    // The save's temporary file is beside the destination, inside the root; none is outside.
    assert!(!base.join("outside/saved.dxf.cadcraft-tmp").exists());
    assert!(!base.join("outside/up.dxf.cadcraft-tmp").exists());
    let leftovers: Vec<String> =
        std::fs::read_dir(base.join("outside")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(leftovers, ["b.dxf"], "nothing was written outside the roots");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn a_root_left_out_grants_none_of_its_access() {
    let base = folders("left-out");
    let (out, t) = run(
        &["--automation-read-root", &s(&base.join("in"))],
        &[("open", json!({"path": s(&base.join("in/a.dxf"))})), ("save", json!({"path": s(&base.join("out/a.dxf"))})), ("save", json!({}))],
    );
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    ok(&t[0]);
    refused(&t[1], "write authority is absent");
    // QSAVE writes over the opened drawing's own path, which is not writable either.
    refused(&t[2], "write authority is absent");

    let (out, t) = run(
        &["--automation-write-root", &s(&base.join("out"))],
        &[("open", json!({"path": s(&base.join("in/a.dxf"))})), ("save", json!({"path": s(&base.join("out/new.dxf"))}))],
    );
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    refused(&t[0], "read authority is absent");
    ok(&t[1]);
    assert!(base.join("out/new.dxf").is_file());
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn without_roots_files_are_reachable_as_before() {
    let base = folders("none");
    let (out, t) =
        run(&[], &[("open", json!({"path": s(&base.join("outside/b.dxf"))})), ("save", json!({"path": s(&base.join("outside/new.dxf"))}))]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    ok(&t[0]);
    ok(&t[1]);
    assert!(base.join("outside/new.dxf").is_file());
    let _ = std::fs::remove_dir_all(base);
}

#[cfg(unix)]
#[test]
fn links_out_of_a_root_are_refused() {
    let base = folders("links");
    std::os::unix::fs::symlink(base.join("outside"), base.join("out/away")).unwrap();
    std::os::unix::fs::symlink(base.join("outside/b.dxf"), base.join("in/leak.dxf")).unwrap();
    let (out, t) = run(
        &["--automation-read-root", &s(&base.join("in")), "--automation-write-root", &s(&base.join("out"))],
        &[("open", json!({"path": s(&base.join("in/leak.dxf"))})), ("save", json!({"path": s(&base.join("out/away/new.dxf"))}))],
    );
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    refused(&t[0], "outside the read root");
    refused(&t[1], "outside the write root");
    assert!(!base.join("outside/new.dxf").exists());
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn every_file_a_command_touches_is_held_to_the_roots() {
    let base = folders("commands");
    let drawing = cadcraft_engine::doc::Drawing::new_imperial();
    std::fs::write(base.join("in/x.dxf"), cadcraft_io::write(&drawing, "x.dxf").unwrap()).unwrap();
    std::fs::write(base.join("in/ok.scr"), "CIRCLE 0,0 1\n").unwrap();
    std::fs::write(base.join("outside/bad.scr"), "CIRCLE 0,0 1\n").unwrap();
    let (out, t) = run(
        &["--automation-read-root", &s(&base.join("in")), "--automation-write-root", &s(&base.join("out"))],
        &[
            ("open", json!({"path": s(&base.join("in/a.dxf"))})),
            // SCRIPT
            ("execute", json!({"command": "script", "params": {"path": s(&base.join("in/ok.scr"))}})),
            ("execute", json!({"command": "script", "params": {"path": s(&base.join("outside/bad.scr"))}})),
            // XATTACH reads the referenced drawing, INSERT and IMPORT read their files.
            ("execute", json!({"command": "xattach", "params": {"path": s(&base.join("in/x.dxf")), "name": "ok"}})),
            ("execute", json!({"command": "xattach", "params": {"path": s(&base.join("outside/b.dxf")), "name": "bad"}})),
            ("execute", json!({"command": "insert", "params": {"name": "blk", "file": s(&base.join("outside/b.dxf")), "at": [0, 0]}})),
            ("execute", json!({"command": "import", "params": {"path": s(&base.join("outside/b.dxf"))}})),
            // Support files (a linetype file here) and a plot style table.
            ("execute", json!({"command": "linetype", "params": {"file": s(&base.join("outside/b.dxf"))}})),
            ("execute", json!({"command": "stylesmanager", "params": {"action": "load", "path": s(&base.join("outside/b.dxf"))}})),
            // PLOT writes only inside the write root; the inside plot works.
            ("execute", json!({"command": "plot", "params": {"path": s(&base.join("out/plot.pdf"))}})),
            ("execute", json!({"command": "plot", "params": {"path": s(&base.join("outside/plot.pdf"))}})),
        ],
    );
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    ok(&t[0]);
    ok(&t[1]);
    refused(&t[2], "outside the read root");
    ok(&t[3]);
    refused(&t[4], "outside the read root");
    refused(&t[5], "outside the read root");
    refused(&t[6], "outside the read root");
    refused(&t[7], "outside the read root");
    refused(&t[8], "outside the read root");
    ok(&t[9]);
    assert!(base.join("out/plot.pdf").is_file());
    refused(&t[10], "outside the write root");
    assert!(!base.join("outside/plot.pdf").exists());
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn missing_roots_and_roots_with_connect_fail_at_startup() {
    let base = folders("bad-roots");
    let input = format!("{INIT}\n");
    let start = |args: &[&str]| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_cadcraft-cli"))
            .arg("mcp")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn cadcraft-cli");
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input.as_bytes());
        }
        child.wait_with_output().expect("wait for cadcraft-cli")
    };
    let out = start(&["--automation-read-root", &s(&base.join("nope"))]);
    assert!(!out.status.success() && out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot open the automation read root"));

    let out = start(&["--automation-write-root", &s(&base.join("in/a.dxf"))]);
    assert!(!out.status.success() && out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot open the automation write root"));

    let out = start(&["--connect", "127.0.0.1:1", "--automation-read-root", &s(&base.join("in"))]);
    assert!(!out.status.success() && out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("headless MCP server"));
    let _ = std::fs::remove_dir_all(base);
}
