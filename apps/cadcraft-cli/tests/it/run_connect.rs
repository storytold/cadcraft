//! `cadcraft-cli run --connect` end to end against a stand-in control server: each step becomes
//! one control request, in command-line order, on one connection.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::sync::mpsc;

use serde_json::{Value, json};

/// Serve one connection, answering each request with `answer`, and hand back every request.
fn fake_app(answer: impl Fn(&Value) -> Value + Send + 'static) -> (String, mpsc::Receiver<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().expect("local addr").to_string();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (conn, _) = listener.accept().expect("accept");
        let mut reader = BufReader::new(conn.try_clone().expect("clone"));
        let mut w = conn;
        let mut seen = Vec::new();
        let mut line = String::new();
        while reader.read_line(&mut line).is_ok_and(|n| n > 0) {
            let req: Value = serde_json::from_str(&line).expect("json request");
            line.clear();
            let reply = answer(&req);
            seen.push(req.clone());
            writeln!(w, "{}", json!({"id": req["id"], "ok": reply.get("error").is_none(), "result": reply["result"], "error": reply["error"]}))
                .expect("write reply");
        }
        let _ = tx.send(seen);
    });
    (addr, rx)
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).arg("run").args(args).output().expect("run cadcraft-cli")
}

#[test]
fn steps_become_control_requests_in_order() {
    let (addr, requests) = fake_app(|req| match (req["method"].as_str(), req["params"]["command"].as_str()) {
        (Some("cmdline.script"), _) => json!({"result": {"output": ["Command: CIRCLE"], "state": {}}}),
        (Some("engine.execute"), Some("line")) => json!({"result": {"handles": ["1A"]}}),
        _ => json!({"result": null}),
    });
    let out = run(&[
        "--connect",
        &addr,
        "drawing.dxf",
        "--script",
        "circle 0,0 5",
        "--cmd",
        r#"line {"points":[[0,0],[1,1]]}"#,
        "--cmd",
        r#"erase {"handles":["{last}"]}"#,
        "--save",
        "out.dxf",
    ]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Command: CIRCLE"), "a script's output goes to stderr: {stderr}");
    assert_eq!(String::from_utf8_lossy(&out.stdout).lines().collect::<Vec<_>>(), [r#"{"handles":["1A"]}"#]);

    let reqs = requests.recv().expect("requests");
    let methods: Vec<&str> = reqs.iter().filter_map(|r| r["method"].as_str()).collect();
    assert_eq!(methods, ["app.open", "cmdline.script", "engine.execute", "engine.execute", "app.save"]);
    let cwd = std::env::current_dir().expect("cwd");
    assert_eq!(reqs[0]["params"]["path"], json!(cwd.join("drawing.dxf").to_str().expect("utf-8 cwd")), "paths reach the app absolute");
    assert_eq!(reqs[1]["params"]["text"], "circle 0,0 5");
    assert_eq!(reqs[3]["params"], json!({"command": "erase", "params": {"handles": ["1A"]}}), "{{last}} is the handle line returned");
    assert_eq!(reqs[4]["params"]["path"], json!(cwd.join("out.dxf").to_str().expect("utf-8 cwd")));
}

#[test]
fn a_refused_step_stops_the_run_and_says_which() {
    let (addr, requests) = fake_app(|req| match req["params"]["command"].as_str() {
        Some("bogus") => json!({"error": "unknown command `bogus`"}),
        _ => json!({"result": null}),
    });
    let out = run(&["--connect", &addr, "--cmd", "bogus", "--cmd", "zoom"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--cmd bogus: unknown command `bogus`"), "{stderr}");
    assert_eq!(requests.recv().expect("requests").len(), 1, "nothing after the refused step is sent");
}

#[test]
fn last_is_forgotten_after_a_drawing_may_have_changed() {
    for between in [&["--script", "zoom e"][..], &["--cmd", "document.switch {\"index\":0}"], &["--cmd", "NEW"]] {
        let (addr, requests) = fake_app(|req| match req["params"]["command"].as_str() {
            Some("line") => json!({"result": {"handles": ["1A"]}}),
            _ => json!({"result": {"output": [], "state": {}}}),
        });
        let mut args = vec!["--connect", addr.as_str(), "--cmd", r#"line {"points":[[0,0],[1,1]]}"#];
        args.extend_from_slice(between);
        args.extend_from_slice(&["--cmd", r#"erase {"handles":["{last}"]}"#]);
        let out = run(&args);
        assert_eq!(out.status.code(), Some(1), "{between:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("`{last}` needs an earlier --cmd"), "{between:?}");
        let reqs = requests.recv().expect("requests");
        assert!(reqs.iter().all(|r| r["params"]["command"] != "erase"), "{between:?}: the stale handle was never sent");
    }
}

#[test]
fn nothing_listening_is_an_error_naming_the_address() {
    let free = TcpListener::bind("127.0.0.1:0").expect("bind").local_addr().expect("addr").to_string();
    let out = run(&["--connect", &free, "--script", "circle 0,0 1"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains(&format!("cannot reach CADCraft at {free}")));
}
