//! `cadcraft-cli run --connect` end to end against a stand-in control server: each step becomes
//! one control request, in command-line order, on one connection.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

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

/// The requests the fake app saw. Bounded, so a run that never connects fails instead of hanging.
fn requests(rx: &mpsc::Receiver<Vec<Value>>) -> Vec<Value> {
    rx.recv_timeout(Duration::from_secs(60)).expect("the run connected and closed the connection")
}

/// A `ui.inspect` reply whose open drawing is `title`.
fn inspect(title: &str) -> Value {
    json!({"result": {"session": {"documents": [{"index": 0, "title": title, "path": null, "active": true}]}}})
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).arg("run").args(args).output().expect("run cadcraft-cli")
}

const LINE: &str = r#"line {"points":[[0,0],[1,1]]}"#;
const ERASE_LAST: &str = r#"erase {"handles":["{last}"]}"#;

#[test]
fn steps_become_control_requests_in_order() {
    let drawing = std::env::temp_dir().join(format!("run-connect-{}.dxf", std::process::id()));
    std::fs::write(&drawing, "").expect("write the drawing to open");
    let (addr, rx) = fake_app(|req| match (req["method"].as_str(), req["params"]["command"].as_str()) {
        (Some("cmdline.script"), _) => json!({"result": {"output": ["Command: CIRCLE"], "state": {}}}),
        (Some("ui.inspect"), _) => inspect("Drawing1"),
        (Some("engine.execute"), Some("line")) => json!({"result": {"handles": ["1A"]}}),
        _ => json!({"result": null}),
    });
    let out = run(&[
        "--connect",
        &addr,
        drawing.to_str().expect("utf-8 temp dir"),
        "--script",
        "circle 0,0 5",
        "--cmd",
        LINE,
        "--cmd",
        ERASE_LAST,
        "--save",
        "out.dxf",
    ]);
    let _ = std::fs::remove_file(&drawing);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Command: CIRCLE"), "a script's output goes to stderr: {stderr}");
    assert_eq!(String::from_utf8_lossy(&out.stdout).lines().collect::<Vec<_>>(), [r#"{"handles":["1A"]}"#]);

    let reqs = requests(&rx);
    let sent: Vec<&Value> = reqs.iter().filter(|r| r["method"] != "ui.inspect").collect();
    let methods: Vec<&str> = sent.iter().filter_map(|r| r["method"].as_str()).collect();
    assert_eq!(methods, ["app.open", "cmdline.script", "engine.execute", "engine.execute", "app.save"]);
    assert_eq!(sent[0]["params"]["path"], json!(drawing.to_str()));
    assert_eq!(sent[1]["params"]["text"], "circle 0,0 5");
    assert_eq!(sent[3]["params"], json!({"command": "erase", "params": {"handles": ["1A"]}}), "{{last}} is the handle line returned");
    let cwd = std::env::current_dir().expect("cwd");
    assert_eq!(sent[4]["params"]["path"], json!(cwd.join("out.dxf").to_str().expect("utf-8 cwd")), "paths reach the app absolute");
}

#[test]
fn a_drawing_that_cannot_be_read_fails_before_connecting() {
    // Nothing listens here: reaching the app at all would fail with "cannot reach" instead.
    let free = TcpListener::bind("127.0.0.1:0").expect("bind").local_addr().expect("addr").to_string();
    let missing = std::env::temp_dir().join("run-connect-missing/plan.dxf");
    let out = run(&["--connect", &free, "--metric", missing.to_str().expect("utf-8 temp dir")]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("plan.dxf") && !stderr.contains("cannot reach"), "{stderr}");
}

#[test]
fn a_refused_step_stops_the_run_and_says_which() {
    let (addr, rx) = fake_app(|req| match req["params"]["command"].as_str() {
        Some("bogus") => json!({"error": "unknown command `bogus`"}),
        _ => json!({"result": null}),
    });
    let out = run(&["--connect", &addr, "--cmd", "bogus", "--cmd", "zoom"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--cmd bogus: unknown command `bogus`"), "{stderr}");
    assert_eq!(requests(&rx).len(), 1, "nothing after the refused step is sent");
}

#[test]
fn last_is_refused_once_another_drawing_is_open() {
    // Another drawing comes up through a step (a script that opens one), or outside the run
    // (a click in the window, another client): either way the handle is not applied.
    for switch_on_script in [true, false] {
        let switched = Arc::new(AtomicBool::new(false));
        let s = Arc::clone(&switched);
        let (addr, rx) = fake_app(move |req| match (req["method"].as_str(), req["params"]["command"].as_str()) {
            (Some("ui.inspect"), _) => {
                let r = inspect(if s.load(Ordering::SeqCst) { "Drawing2" } else { "Drawing1" });
                // Outside the run: the drawing changes right after the handle came back.
                if !switch_on_script {
                    s.store(true, Ordering::SeqCst);
                }
                r
            }
            (Some("cmdline.script"), _) => {
                s.store(true, Ordering::SeqCst);
                json!({"result": {"output": [], "state": {}}})
            }
            (_, Some("line")) => json!({"result": {"handles": ["1A"]}}),
            _ => json!({"result": null}),
        });
        let mut args = vec!["--connect", addr.as_str(), "--cmd", LINE];
        if switch_on_script {
            args.extend_from_slice(&["--script", "open other.dxf"]);
        }
        args.extend_from_slice(&["--cmd", ERASE_LAST]);
        let out = run(&args);
        assert_eq!(out.status.code(), Some(1), "switch on script: {switch_on_script}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("another drawing"), "switch on script: {switch_on_script}: {stderr}");
        assert!(requests(&rx).iter().all(|r| r["params"]["command"] != "erase"), "the handle was never sent");
    }
}

#[test]
fn last_survives_a_step_that_keeps_the_same_drawing_open() {
    let (addr, rx) = fake_app(|req| match (req["method"].as_str(), req["params"]["command"].as_str()) {
        (Some("ui.inspect"), _) => inspect("Drawing1"),
        (Some("cmdline.script"), _) => json!({"result": {"output": [], "state": {}}}),
        (_, Some("line")) => json!({"result": {"handles": ["1A"]}}),
        _ => json!({"result": null}),
    });
    let out = run(&["--connect", &addr, "--cmd", LINE, "--script", "zoom e", "--cmd", ERASE_LAST]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let erase = requests(&rx).into_iter().find(|r| r["params"]["command"] == "erase");
    assert_eq!(erase.map(|r| r["params"]["params"].clone()), Some(json!({"handles": ["1A"]})));
}

#[test]
fn last_is_refused_when_the_app_does_not_say_which_drawing_is_open() {
    let (addr, rx) = fake_app(|req| match req["params"]["command"].as_str() {
        Some("line") => json!({"result": {"handles": ["1A"]}}),
        _ => json!({"result": {}}),
    });
    let out = run(&["--connect", &addr, "--cmd", LINE, "--cmd", ERASE_LAST]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("doesn't say which drawing"), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(requests(&rx).iter().all(|r| r["params"]["command"] != "erase"), "the handle was never sent");
}

#[test]
fn nothing_listening_is_an_error_naming_the_address() {
    let free = TcpListener::bind("127.0.0.1:0").expect("bind").local_addr().expect("addr").to_string();
    let out = run(&["--connect", &free, "--script", "circle 0,0 1"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains(&format!("cannot reach CADCraft at {free}")));
}
