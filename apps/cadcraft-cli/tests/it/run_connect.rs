//! `cadcraft-cli run --connect` end to end against a stand-in control server: each step becomes
//! one control request, in command-line order, and a run keeps to one connection.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
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

/// A `ui.inspect` reply whose open drawing has id `uid`. The index, title and path never change, as
/// when one file is closed and opened again in the same slot: only the id tells the drawings apart.
fn inspect(uid: u64) -> Value {
    json!({"result": {"session": {"documents": [{"index": 0, "uid": uid, "title": "plan.dxf", "path": "/plans/plan.dxf", "active": true}]}}})
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
        (Some("ui.inspect"), _) => inspect(1),
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

/// A FIFO with no writer as the drawing: refused at once, never waited on.
#[cfg(unix)]
#[test]
fn a_drawing_that_is_not_a_regular_file_is_refused_without_waiting() {
    let dir = std::env::temp_dir().join(format!("run-connect-fifo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let fifo = dir.join("plan.dxf");
    let made = Command::new("mkfifo").arg(&fifo).status().expect("run mkfifo");
    assert!(made.success(), "mkfifo failed");
    let free = TcpListener::bind("127.0.0.1:0").expect("bind").local_addr().expect("addr").to_string();
    let mut child = Command::new(env!("CARGO_BIN_EXE_cadcraft-cli"))
        .args(["run", "--connect", &free])
        .arg(&fifo)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("run cadcraft-cli");
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait for cadcraft-cli") {
            break Some(status);
        }
        if start.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let out = child.wait_with_output().expect("collect output");
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(status.and_then(|s| s.code()), Some(1), "the run must fail, not wait for a writer");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not a regular file") && !stderr.contains("cannot reach"), "{stderr}");
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

/// When the fake app's open drawing changes, relative to `run`'s requests.
#[derive(Clone, Copy, Debug)]
enum Switch {
    /// A script step opens another drawing, or closes the file and opens it again.
    ByScript,
    /// Outside the run (a click in the window, another client), after the handle's drawing was read.
    AfterTheHandle,
    /// Outside the run, while the command that returns the handle runs.
    WhileTheCommandRuns,
}

/// A fake app whose drawing id goes from 1 to 2 at `switch`, where `line` returns handle 1A.
fn switching_app(switch: Switch) -> (String, mpsc::Receiver<Vec<Value>>) {
    let uid = Arc::new(AtomicU64::new(1));
    let inspects = Arc::new(AtomicU64::new(0));
    fake_app(move |req| match (req["method"].as_str(), req["params"]["command"].as_str()) {
        (Some("ui.inspect"), _) => {
            let r = inspect(uid.load(Ordering::SeqCst));
            // The second inspect is the one right after `line`: the handle's drawing is known.
            if matches!(switch, Switch::AfterTheHandle) && inspects.fetch_add(1, Ordering::SeqCst) == 1 {
                uid.store(2, Ordering::SeqCst);
            }
            r
        }
        (Some("cmdline.script"), _) => {
            if matches!(switch, Switch::ByScript) {
                uid.store(2, Ordering::SeqCst);
            }
            json!({"result": {"output": [], "state": {}}})
        }
        (_, Some("line")) => {
            if matches!(switch, Switch::WhileTheCommandRuns) {
                uid.store(2, Ordering::SeqCst);
            }
            json!({"result": {"handles": ["1A"]}})
        }
        _ => json!({"result": null}),
    })
}

#[test]
fn last_is_refused_once_another_drawing_could_be_open() {
    for (switch, says) in [
        (Switch::ByScript, "another drawing"),
        (Switch::AfterTheHandle, "another drawing"),
        (Switch::WhileTheCommandRuns, "doesn't say which drawing"),
    ] {
        let (addr, rx) = switching_app(switch);
        let mut args = vec!["--connect", addr.as_str(), "--cmd", LINE];
        if matches!(switch, Switch::ByScript) {
            args.extend_from_slice(&["--script", "close\nopen /plans/plan.dxf"]);
        }
        args.extend_from_slice(&["--cmd", ERASE_LAST]);
        let out = run(&args);
        assert_eq!(out.status.code(), Some(1), "{switch:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(says), "{switch:?}: {stderr}");
        assert!(requests(&rx).iter().all(|r| r["params"]["command"] != "erase"), "{switch:?}: the handle was never sent");
    }
}

#[test]
fn last_is_never_sent_after_a_read_that_saw_another_drawing() {
    // The drawing switches right after the read that checks `{last}`. A run that reads it again
    // before the command sees the new id; it must not then send the old handle.
    let uid = Arc::new(AtomicU64::new(1));
    let inspects = Arc::new(AtomicU64::new(0));
    let last_seen = Arc::new(AtomicU64::new(0));
    let seen_at_erase = Arc::new(AtomicU64::new(0));
    let (u, n, seen, at_erase) = (Arc::clone(&uid), Arc::clone(&inspects), Arc::clone(&last_seen), Arc::clone(&seen_at_erase));
    let (addr, rx) = fake_app(move |req| match (req["method"].as_str(), req["params"]["command"].as_str()) {
        (Some("ui.inspect"), _) => {
            let id = u.load(Ordering::SeqCst);
            seen.store(id, Ordering::SeqCst);
            // Reads: before `line`, after it, then the one that checks `{last}` for `erase`.
            if n.fetch_add(1, Ordering::SeqCst) + 1 == 3 {
                u.store(2, Ordering::SeqCst);
            }
            inspect(id)
        }
        (_, Some("line")) => json!({"result": {"handles": ["1A"]}}),
        (_, Some("erase")) => {
            at_erase.store(seen.load(Ordering::SeqCst), Ordering::SeqCst);
            json!({"result": null})
        }
        _ => json!({"result": null}),
    });
    let out = run(&["--connect", &addr, "--cmd", LINE, "--cmd", ERASE_LAST]);
    let reqs = requests(&rx);
    if reqs.iter().any(|r| r["params"]["command"] == "erase") {
        assert_eq!(seen_at_erase.load(Ordering::SeqCst), 1, "{{last}} was sent after a read saw drawing 2");
    } else {
        assert_eq!(out.status.code(), Some(1));
    }
}

/// What a stand-in app does after answering a request.
enum Then {
    Stay,
    /// Close the connection, as an app that quits does.
    HangUp,
}

/// Stand-in apps on one port, one after another: each serves one connection, as an app that was
/// restarted would. `answer` gets the app's number (from 0) and the request. Hands back every
/// request with the number of the app it reached, once the last connection has closed.
fn fake_apps(apps: usize, answer: impl Fn(usize, &Value) -> (Value, Then) + Send + 'static) -> (String, mpsc::Receiver<Vec<(usize, Value)>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().expect("local addr").to_string();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut seen = Vec::new();
        for app in 0..apps {
            let Ok((conn, _)) = listener.accept() else { break };
            let mut reader = BufReader::new(conn.try_clone().expect("clone"));
            let mut w = conn;
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 0) {
                let req: Value = serde_json::from_str(&line).expect("json request");
                line.clear();
                let (reply, then) = answer(app, &req);
                seen.push((app, req.clone()));
                let reply = json!({"id": req["id"], "ok": reply.get("error").is_none(), "result": reply["result"], "error": reply["error"], "state": reply["state"]});
                writeln!(w, "{reply}").expect("write reply");
                if matches!(then, Then::HangUp) {
                    break;
                }
            }
        }
        let _ = tx.send(seen);
    });
    (addr, rx)
}

/// Two apps in a row whose open drawing is `uid` 1 both times: a restarted app numbers its drawings
/// from 1 again. `line` returns handle 1A. The first app quits once the handle's drawing has been
/// read (the read after `line`).
fn restarting_app() -> (String, mpsc::Receiver<Vec<(usize, Value)>>) {
    let inspects = AtomicU64::new(0);
    fake_apps(2, move |app, req| match (req["method"].as_str(), req["params"]["command"].as_str()) {
        (Some("ui.inspect"), _) => {
            let quit = app == 0 && inspects.fetch_add(1, Ordering::SeqCst) == 1;
            (inspect(1), if quit { Then::HangUp } else { Then::Stay })
        }
        (_, Some("line")) => (json!({"result": {"handles": ["1A"]}}), Then::Stay),
        _ => (json!({"result": null}), Then::Stay),
    })
}

/// The requests that reached the app after the restart, other than reads.
fn sent_after_the_restart(rx: &mpsc::Receiver<Vec<(usize, Value)>>) -> Vec<Value> {
    let reqs = rx.recv_timeout(Duration::from_secs(60)).expect("the run connected and closed its connections");
    reqs.into_iter().filter(|(app, r)| *app > 0 && r["method"] != "ui.inspect").map(|(_, r)| r).collect()
}

#[test]
fn last_is_refused_after_the_app_restarts_with_the_same_drawing_id() {
    // The id matches, but the app doesn't: the handle must not reach the new app.
    let (addr, rx) = restarting_app();
    let out = run(&["--connect", &addr, "--cmd", LINE, "--cmd", ERASE_LAST]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("opened again"), "{stderr}");
    assert_eq!(sent_after_the_restart(&rx), Vec::<Value>::new(), "nothing but the read reached the new app");
}

#[test]
fn no_step_reaches_an_app_that_restarted_during_the_run() {
    // `circle` and the save don't use `{last}`, but the read before `circle` is resent on a new
    // connection: without the one-connection rule they would change and save the new app's drawing.
    let (addr, rx) = restarting_app();
    let out = run(&["--connect", &addr, "--cmd", LINE, "--cmd", "circle", "--cmd", ERASE_LAST, "--save", "out.dxf"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--cmd circle") && stderr.contains("opened again"), "the error names the step: {stderr}");
    assert_eq!(sent_after_the_restart(&rx), Vec::<Value>::new(), "nothing but the read reached the new app");
}

#[test]
fn a_first_request_resent_on_a_new_connection_is_not_a_restart() {
    // The app refuses the run's first request unrun over its token and closes the connection, and
    // the client sends it again on a new one. Nothing was learned or changed before it, so the run
    // goes on there.
    let (addr, rx) = fake_apps(2, |app, req| match (app, req["params"]["command"].as_str()) {
        (0, _) => (json!({"error": "unauthorized: missing or invalid control token", "state": "not-run"}), Then::HangUp),
        (_, Some("line")) => (json!({"result": {"handles": ["1A"]}}), Then::Stay),
        (_, _) if req["method"] == "ui.inspect" => (inspect(1), Then::Stay),
        _ => (json!({"result": null}), Then::Stay),
    });
    let out = run(&["--connect", &addr, "--cmd", LINE, "--cmd", ERASE_LAST]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let sent = sent_after_the_restart(&rx);
    assert_eq!(sent.last().map(|r| r["params"]["params"].clone()), Some(json!({"handles": ["1A"]})), "{sent:?}");
}

#[test]
fn a_drawing_read_the_app_does_not_answer_stops_the_run() {
    // The read before a command failed: the run doesn't go on without knowing what is open.
    let (addr, rx) = fake_app(|req| match req["method"].as_str() {
        Some("ui.inspect") => json!({"error": "busy: too many requests are waiting"}),
        _ => json!({"result": {"handles": ["1A"]}}),
    });
    let out = run(&["--connect", &addr, "--cmd", LINE, "--cmd", ERASE_LAST]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--cmd line") && stderr.contains("busy"), "{stderr}");
    assert!(requests(&rx).iter().all(|r| r["method"] == "ui.inspect"), "no command was sent");
}

#[test]
fn what_the_app_left_out_of_a_file_is_reported() {
    let drawing = std::env::temp_dir().join(format!("run-connect-notes-{}.dxf", std::process::id()));
    std::fs::write(&drawing, "").expect("write the drawing to open");
    let (addr, _rx) = fake_app(|req| match req["method"].as_str() {
        Some("app.open") => json!({"result": {"index": 0, "warnings": ["2 records were skipped"]}}),
        Some("app.save") => json!({"result": {"path": "out.dwg", "bytes": 10, "notSaved": {"WIPEOUT": 3}, "createdFolder": "/plans/new"}}),
        _ => json!({"result": null}),
    });
    let out = run(&["--connect", &addr, drawing.to_str().expect("utf-8 temp dir"), "--save", "out.dwg"]);
    let _ = std::fs::remove_file(&drawing);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stderr = String::from_utf8_lossy(&out.stderr);
    for said in ["2 records were skipped", "3 WIPEOUT", "/plans/new"] {
        assert!(stderr.contains(said), "{said}: {stderr}");
    }
}

#[test]
fn last_survives_a_step_that_keeps_the_same_drawing_open() {
    let (addr, rx) = fake_app(|req| match (req["method"].as_str(), req["params"]["command"].as_str()) {
        (Some("ui.inspect"), _) => inspect(1),
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
fn last_is_refused_when_the_app_does_not_report_drawing_ids() {
    // A release before `uid`: the drawing list without ids.
    let (addr, rx) = fake_app(|req| match (req["method"].as_str(), req["params"]["command"].as_str()) {
        (Some("ui.inspect"), _) => json!({"result": {"session": {"documents": [{"index": 0, "title": "plan.dxf", "active": true}]}}}),
        (_, Some("line")) => json!({"result": {"handles": ["1A"]}}),
        _ => json!({"result": null}),
    });
    let out = run(&["--connect", &addr, "--cmd", LINE, "--cmd", ERASE_LAST]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("doesn't say which drawing"), "{stderr}");
    assert!(requests(&rx).iter().all(|r| r["params"]["command"] != "erase"), "the handle was never sent");
}

#[test]
fn nothing_listening_is_an_error_naming_the_address() {
    let free = TcpListener::bind("127.0.0.1:0").expect("bind").local_addr().expect("addr").to_string();
    let out = run(&["--connect", &free, "--script", "circle 0,0 1"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains(&format!("cannot reach CADCraft at {free}")));
}
