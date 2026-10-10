//! `cadcraft-cli mcp --connect` end to end: a missing address must fail startup instead of
//! serving drawing edits from a headless session (#103); a real address still bridges.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#;
const CIRCLE: &str = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"execute","arguments":{"command":"circle","params":{"center":[5,5],"radius":2}}}}"#;

fn mcp(args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cadcraft-cli"))
        .arg("mcp")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cadcraft-cli");
    if let Some(mut stdin) = child.stdin.take() {
        // The process may exit before reading stdin (that is the point); ignore a broken pipe.
        let _ = stdin.write_all(input.as_bytes());
    }
    child.wait_with_output().expect("wait for cadcraft-cli")
}

fn replies(out: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

#[test]
fn connect_without_address_fails_before_serving() {
    let input = format!("{INIT}\n{CIRCLE}\n");
    for args in [&["--connect"][..], &["--connect", ""], &["--connect", "--foo"]] {
        let out = mcp(args, &input);
        assert!(!out.status.success(), "{args:?} must exit non-zero");
        assert!(out.stdout.is_empty(), "{args:?} must not answer MCP requests: {}", String::from_utf8_lossy(&out.stdout));
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("--connect needs HOST:PORT"), "{args:?}: {err}");
    }
}

#[test]
fn no_connect_still_serves_headless() {
    let out = mcp(&[], &format!("{INIT}\n{CIRCLE}\n"));
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let r = replies(&out);
    let instructions = r.iter().find(|v| v["id"] == 1).and_then(|v| v["result"]["instructions"].as_str()).unwrap_or_default();
    assert!(instructions.contains("Backend: headless"), "{instructions}");
    let circle = r.iter().find(|v| v["id"] == 2).map(|v| &v["result"]);
    assert!(circle.is_some_and(|c| c.is_object() && c.get("isError").is_none()), "{r:?}");
}

#[test]
fn connect_with_address_routes_to_that_endpoint() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().expect("local addr").to_string();
    let server = std::thread::spawn(move || {
        let (conn, _) = listener.accept().expect("accept");
        let mut reader = BufReader::new(conn.try_clone().expect("clone"));
        let mut line = String::new();
        reader.read_line(&mut line).expect("read request");
        let req: Value = serde_json::from_str(&line).expect("json request");
        let reply = json!({"id": req["id"], "ok": true, "result": {"fixture": "loopback"}});
        let mut w = conn;
        writeln!(w, "{reply}").expect("write reply");
        req
    });
    let call = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"ui_inspect","arguments":{}}}"#;
    let out = mcp(&["--connect", &addr], &format!("{INIT}\n{call}\n"));
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let req = server.join().expect("server thread");
    assert_eq!(req["method"], "ui.inspect");
    let r = replies(&out);
    let instructions = r.iter().find(|v| v["id"] == 1).and_then(|v| v["result"]["instructions"].as_str()).unwrap_or_default();
    assert!(instructions.contains(&format!("connected to CADCraft at {addr}")), "{instructions}");
    let text = r.iter().find(|v| v["id"] == 2).and_then(|v| v["result"]["content"][0]["text"].as_str()).unwrap_or_default();
    assert!(text.contains("loopback"), "{r:?}");
}

#[test]
fn connect_to_closed_port_fails() {
    let addr = {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        l.local_addr().expect("local addr").to_string()
    };
    let out = mcp(&["--connect", &addr], &format!("{INIT}\n"));
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot connect to"));
}
