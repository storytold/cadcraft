//! The agent loop against a headless session, with scripted model replies.

use cadcraft_mcp::{Backend, Headless};
use serde_json::{Value, json};

use crate::{Agent, Config, Error, Event, MAX_STEPS, Observer, Scripted, Turn, text_reply, tool_reply};

fn count(b: &mut Headless, kind: Option<&str>) -> u64 {
    let params = kind.map_or(json!({}), |k| json!({"type": k}));
    let r = b.call("engine.execute", json!({"command": "entities", "params": params})).unwrap();
    r["count"].as_u64().unwrap()
}

/// Records events and answers confirmations with `yes`.
#[derive(Default)]
struct Log {
    yes: bool,
    asked: Vec<String>,
    events: Vec<String>,
}

impl Observer for Log {
    fn confirm(&mut self, name: &str, _: &Value) -> bool {
        self.asked.push(name.to_string());
        self.yes
    }
    fn event(&mut self, e: Event<'_>) {
        self.events.push(match e {
            Event::Text(t) => format!("text {t}"),
            Event::ToolCall { name, .. } => format!("call {name}"),
            Event::ToolResult { name, ok, .. } => format!("result {name} {ok}"),
            Event::Declined { name } => format!("declined {name}"),
        });
    }
}

#[test]
fn draws_a_plate_with_holes_and_reports() {
    let mut b = Headless::default();
    let mut p = Scripted::new([
        tool_reply(&[("a", "command_line", json!({"text": "rectang 0,0 100,60"}))]),
        tool_reply(&[
            ("b", "execute", json!({"command": "circle", "params": {"center": [10, 10], "radius": 4}})),
            ("c", "execute", json!({"command": "circle", "params": {"center": [90, 10], "radius": 4}})),
        ]),
        tool_reply(&[("d", "query_entities", json!({"type": "Circle"}))]),
        text_reply("Drew a 100×60 plate with two holes."),
    ]);
    let mut agent = Agent::new(Config::new("test-model"));
    let mut log = Log::default();
    let turn = agent.send(&mut p, &mut b, "draw a plate with two holes", &mut log).unwrap();
    assert_eq!(turn.reply, "Drew a 100×60 plate with two holes.");
    assert_eq!((turn.steps, turn.tool_calls), (4, 4));
    assert_eq!(count(&mut b, Some("Circle")), 2);
    assert_eq!(count(&mut b, None), 3);
    assert!(log.events.iter().all(|e| !e.ends_with("false")), "{:?}", log.events);
    // Every request carries the model, the history so far and the command-level tools only.
    let last = p.requests.last().unwrap();
    assert_eq!(last["model"], "test-model");
    let tools: Vec<&str> = last["tools"].as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap()).collect();
    assert!(tools.contains(&"command_line") && tools.contains(&"execute"));
    assert!(!tools.contains(&"render") && !tools.contains(&"ui_click"));
    // Each tool call is answered by a tool message with its id.
    let msgs = agent.messages();
    assert_eq!(msgs[0]["role"], "system");
    let tool_ids: Vec<&str> = msgs.iter().filter(|m| m["role"] == "tool").map(|m| m["tool_call_id"].as_str().unwrap()).collect();
    assert_eq!(tool_ids, ["a", "b", "c", "d"]);
    assert!(msgs.iter().any(|m| m["role"] == "tool" && m["content"].as_str().unwrap().contains("\"count\": 2")));
}

#[test]
fn the_conversation_carries_over_to_the_next_turn() {
    let mut b = Headless::default();
    let mut p = Scripted::new([text_reply("Hi."), text_reply("Still here.")]);
    let mut agent = Agent::new(Config::new("m"));
    agent.send(&mut p, &mut b, "hello", &mut ()).unwrap();
    agent.send(&mut p, &mut b, "again", &mut ()).unwrap();
    let roles: Vec<&str> = p.requests[1]["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["system", "user", "assistant", "user"]);
    agent.reset();
    assert_eq!(agent.messages().len(), 1);
}

/// A command line, a script or `execute` that would open, start or save a drawing asks the same
/// question as the tool for it, by command name or alias; ordinary drawing commands ask nothing.
#[test]
fn protected_commands_need_confirmation_by_any_tool() {
    let cases = [
        ("execute", json!({"command": "open", "params": {"path": "x.dxf"}}), Some("open")),
        ("execute", json!({"command": "QSAVE"}), Some("save")),
        ("command_line", json!({"text": "_qnew"}), Some("new_drawing")),
        ("command_line", json!({"text": "saveas 2018 out.dxf"}), Some("save")),
        (
            "script",
            json!({"text": "circle 0,0 5
._save out.dxf"}),
            Some("save"),
        ),
        ("script", json!({"text": "(command \"open\" \"x.dxf\")"}), Some("open")),
        ("command_line", json!({"text": "circle 0,0 5"}), None),
        ("execute", json!({"command": "line", "params": {"from": [0, 0], "to": [1, 1]}}), None),
    ];
    for (tool, args, asked) in cases {
        let mut b = Headless::default();
        let mut log = Log::default();
        let mut p = Scripted::new([tool_reply(&[("a", tool, args.clone())]), text_reply("ok")]);
        Agent::new(Config::new("m")).send(&mut p, &mut b, "go", &mut log).unwrap();
        assert_eq!(log.asked, asked.into_iter().collect::<Vec<_>>(), "{tool} {args}");
        assert_eq!(log.events.contains(&format!("declined {tool}")), asked.is_some(), "{tool} {args}");
    }
}

/// A script with a new drawing and then a save asks both; a yes to the first doesn't approve the
/// second, and one no stops the whole script.
#[test]
fn every_protected_command_of_a_script_is_asked() {
    // Another test may have installed the file hooks: if the save runs, it lands in a scratch folder.
    let dir = std::env::temp_dir().join(format!("cadcraft-assistant-all-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("out.dxf").to_string_lossy().replace('\\', "/");
    let script = json!({"text": format!("new
circle 0,0 5
saveas \"{out}\"
_qnew")});
    let run = |yes: bool| {
        let mut log = Log { yes, ..Log::default() };
        let mut p = Scripted::new([tool_reply(&[("a", "script", script.clone())]), text_reply("ok")]);
        Agent::new(Config::new("m")).send(&mut p, &mut Headless::default(), "go", &mut log).unwrap();
        log
    };
    // Each distinct question once, in order.
    assert_eq!(run(true).asked, ["new_drawing", "save"]);
    // A no stops at the first question and nothing runs.
    let declined = run(false);
    assert_eq!(declined.asked, ["new_drawing"]);
    assert!(declined.events.contains(&"declined script".to_string()));
    assert!(!declined.events.iter().any(|e| e.starts_with("result script")));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Any other command given a file asks `file` first, by any tool: EXPORT, WBLOCK, PLOT… and a
/// command the engine doesn't know, given a path. Commands that only read a file into the drawing,
/// and drawing commands, ask nothing.
#[test]
fn commands_given_a_file_ask_first() {
    let cases = [
        ("execute", json!({"command": "export", "params": {"path": "notes.txt", "format": "dxf"}}), Some("file")),
        ("execute", json!({"command": "WBLOCK", "params": {"path": "b.dwg", "name": "B"}}), Some("file")),
        ("execute", json!({"command": "nosuchcommand", "params": {"path": "x"}}), Some("file")),
        ("command_line", json!({"text": "_exportpdf out.pdf"}), Some("file")),
        ("command_line", json!({"text": "tableexport"}), Some("file")),
        ("script", json!({"text": "circle 0,0 5\npublish"}), Some("file")),
        ("script", json!({"text": "(command \"plot\")"}), Some("file")),
        // The question for opening, once, not a second one for its path.
        ("execute", json!({"command": "open", "params": {"path": "x.dxf"}}), Some("open")),
        ("execute", json!({"command": "insert", "params": {"name": "B", "file": "b.dwg"}}), None),
        ("execute", json!({"command": "arraypath", "params": {"handles": ["2A"], "path": "2B"}}), None),
        ("command_line", json!({"text": "hatch"}), None),
        ("command_line", json!({"text": "array"}), None),
    ];
    for (tool, args, asked) in cases {
        let mut log = Log::default();
        let mut p = Scripted::new([tool_reply(&[("a", tool, args.clone())]), text_reply("ok")]);
        Agent::new(Config::new("m")).send(&mut p, &mut Headless::default(), "go", &mut log).unwrap();
        assert_eq!(log.asked, asked.into_iter().collect::<Vec<_>>(), "{tool} {args}");
        assert_eq!(log.events.contains(&format!("declined {tool}")), asked.is_some(), "{tool} {args}");
    }
}

/// The engine's commands that write a file the caller names, or run one, ask; the ones listed as
/// reading only don't, and nor do drawing commands.
#[test]
fn every_command_that_writes_a_file_asks() {
    let writes = [
        "export",
        "exportpdf",
        "plot",
        "publish",
        "pdfbook",
        "wblock",
        "tableexport",
        "dataextraction",
        "eattext",
        "layout",
        "layout.saveas",
        "stylesmanager",
        "plottermanager",
        "aliasedit",
        "drawingrecovery",
        "appload",
        "script",
    ];
    for id in writes {
        assert!(cadcraft_engine::cmd::find_command(id).is_some(), "{id} is not a command");
        assert!(crate::agent::takes_file(id), "{id} doesn't ask");
    }
    for id in ["import", "insert", "hatch", "array", "arraypath", "xref", "line", "circle", "entities"] {
        assert!(!crate::agent::takes_file(id), "{id} asks");
    }
}

/// A call that builds its file write at run time names nothing the questions can see, so nothing
/// is asked: the engine refuses the write anyway, since the call has no consent to write.
#[test]
fn a_write_built_at_run_time_is_refused_by_the_engine() {
    let dir = std::env::temp_dir().join(format!("cadcraft-assistant-runtime-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("notes.txt");
    std::fs::write(&path, "the user's notes").unwrap();
    let p = path.to_string_lossy().replace('\\', "/");
    // `open` assembled from two halves, the mode from its character code.
    let code =
        format!("(setq o (eval (read (strcat \"op\" \"en\")))) (setq f (o \"{p}\" (chr 119))) (if f (progn (write-line \"gone\" f) (close f)))");
    let mut log = Log { yes: true, ..Log::default() };
    let mut p = Scripted::new([tool_reply(&[("a", "execute", json!({"command": "lisp", "params": {"code": code}}))]), text_reply("ok")]);
    Agent::new(Config::new("m")).send(&mut p, &mut Headless::default(), "go", &mut log).unwrap();
    assert!(log.asked.is_empty(), "{:?}", log.asked);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "the user's notes");
    std::fs::remove_dir_all(&dir).ok();
}

/// CLOSE asks first; one built at run time isn't asked, and the engine refuses it.
#[test]
fn closing_a_drawing_needs_the_users_yes() {
    let run = |yes: bool, tool: &str, args: Value| {
        let mut b = Headless::default();
        b.session.new_drawing(true);
        let mut log = Log { yes, ..Log::default() };
        let mut p = Scripted::new([tool_reply(&[("a", tool, args)]), text_reply("ok")]);
        Agent::new(Config::new("m")).send(&mut p, &mut b, "go", &mut log).unwrap();
        (log.asked, b.session.docs.len())
    };
    assert_eq!(run(false, "command_line", json!({"text": "close"})), (vec!["close".to_string()], 2));
    assert_eq!(run(true, "command_line", json!({"text": "_closeall"})), (vec!["close".to_string()], 0));
    let built = json!({"command": "lisp", "params": {"code": "(command (strcat \"clo\" \"se\"))"}});
    assert_eq!(run(true, "execute", built), (vec![], 2));
}

/// A file named by text in the drawing isn't overwritten by EXPORT unless the user says yes.
#[test]
fn export_doesnt_overwrite_a_file_unasked() {
    cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    let dir = std::env::temp_dir().join(format!("cadcraft-assistant-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("notes.txt");
    std::fs::write(&path, "the user's notes").unwrap();
    let call = json!({"command": "export", "params": {"path": path.to_string_lossy(), "format": "dxf"}});
    let run = |yes: bool| {
        let mut log = Log { yes, ..Log::default() };
        let mut p = Scripted::new([tool_reply(&[("a", "execute", call.clone())]), text_reply("ok")]);
        Agent::new(Config::new("m")).send(&mut p, &mut Headless::default(), "go", &mut log).unwrap();
        log
    };
    let declined = run(false);
    assert_eq!(declined.asked, ["file"]);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "the user's notes");
    let confirmed = run(true);
    assert_eq!(confirmed.asked, ["file"]);
    assert_ne!(std::fs::read_to_string(&path).unwrap_or_default(), "the user's notes", "{:?}", confirmed.events);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn tools_that_replace_the_drawing_or_write_files_need_confirmation() {
    cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    let dir = std::env::temp_dir().join(format!("cadcraft-assistant-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("out.dxf");
    let path_s = path.to_string_lossy().to_string();
    let script = || {
        Scripted::new([
            tool_reply(&[("a", "command_line", json!({"text": "circle 0,0 5"}))]),
            tool_reply(&[("b", "save", json!({"path": path_s}))]),
            text_reply("done"),
        ])
    };

    // Declined: nothing is written and the model is told not to retry.
    let mut b = Headless::default();
    let mut log = Log::default();
    let mut p = script();
    Agent::new(Config::new("m")).send(&mut p, &mut b, "draw and save", &mut log).unwrap();
    assert_eq!(log.asked, ["save"]);
    assert!(log.events.contains(&"declined save".to_string()));
    assert!(!path.exists());
    let told = p.requests[2]["messages"].as_array().unwrap().iter().any(|m| m["content"].as_str().is_some_and(|c| c.contains("declined")));
    assert!(told);

    // The default observer declines too.
    let mut p = script();
    Agent::new(Config::new("m")).send(&mut p, &mut Headless::default(), "draw and save", &mut ()).unwrap();
    assert!(!path.exists());

    // Confirmed: the file is written.
    let mut log = Log { yes: true, ..Log::default() };
    let mut p = script();
    Agent::new(Config::new("m")).send(&mut p, &mut Headless::default(), "draw and save", &mut log).unwrap();
    assert!(path.exists(), "{:?}", log.events);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn hostile_model_replies_never_panic() {
    let huge = "x".repeat(100_000);
    let many: Vec<(String, &str, Value)> = (0..50).map(|i| (format!("c{i}"), "cancel", json!({}))).collect();
    let many: Vec<(&str, &str, Value)> = many.iter().map(|(id, n, a)| (id.as_str(), *n, a.clone())).collect();
    let replies = vec![
        json!({"choices": [{"message": {"role": "assistant", "tool_calls": [
            {"id": "1", "function": {"name": "execute", "arguments": "{not json"}},
            {"id": "2", "function": {"name": "rm_rf", "arguments": "{}"}},
            {"function": {"name": "command_line", "arguments": {"text": "circle 0,0 5"}}},
            {"id": "4", "function": {"name": "execute", "arguments": "[1,2]"}},
            {"id": "5", "function": {"name": "query_entities", "arguments": "{\"limit\": -3}"}},
            {"id": "6", "function": {"name": "command_line", "arguments": json!({"text": huge}).to_string()}},
            {"id": "7", "function": {}},
            {"id": "8"},
            7,
            null,
        ]}}]}),
        tool_reply(&many),
        json!({"choices": [{"message": {"content": 42}}]}),
    ];
    let mut b = Headless::default();
    let mut p = Scripted::new(replies);
    let mut agent = Agent::new(Config::new("m"));
    let turn = agent.send(&mut p, &mut b, "go", &mut ()).unwrap();
    assert_eq!(turn.reply, "");
    // The well-formed call ran despite its neighbours.
    assert_eq!(count(&mut b, Some("Circle")), 1);
    // At most 16 calls per reply; every recorded call has its answer.
    let msgs = agent.messages();
    let asked: usize = msgs.iter().filter_map(|m| m["tool_calls"].as_array()).map(Vec::len).sum();
    let answered = msgs.iter().filter(|m| m["role"] == "tool").count();
    assert_eq!(asked, answered);
    assert!(answered <= 32);
    // Long results are cut.
    assert!(msgs.iter().all(|m| m["content"].as_str().is_none_or(|c| c.chars().count() < 17_000)));

    for bad in [json!({}), json!([]), json!({"choices": []}), json!({"choices": [{"message": null}]}), json!("text"), Value::Null] {
        let r = Agent::new(Config::new("m")).send(&mut Scripted::new([bad.clone()]), &mut Headless::default(), "go", &mut ());
        assert!(matches!(r, Err(Error::BadResponse(_))), "{bad}: {r:?}");
    }
    let r = Agent::new(Config::new("m")).send(
        &mut Scripted::new([json!({"error": {"message": "rate limited"}})]),
        &mut Headless::default(),
        "go",
        &mut (),
    );
    assert_eq!(r, Err(Error::Provider("rate limited".into())));
    let r = Agent::new(Config::new("m")).send(&mut Scripted::default(), &mut Headless::default(), "go", &mut ());
    assert!(matches!(r, Err(Error::Provider(_))));
    let r = Agent::new(Config::new("m")).send(&mut Scripted::default(), &mut Headless::default(), "  \n", &mut ());
    assert_eq!(r, Err(Error::EmptyPrompt));
}

#[test]
fn a_model_that_never_stops_calling_tools_is_stopped() {
    let endless = (0..100).map(|i| tool_reply(&[("x", "list_commands", json!({"filter": format!("z{i}")}))]));
    let mut p = Scripted::new(endless);
    let mut cfg = Config::new("m");
    cfg.max_steps = 1000;
    let r = Agent::new(cfg).send(&mut p, &mut Headless::default(), "loop", &mut ());
    assert_eq!(r, Err(Error::StepLimit { steps: MAX_STEPS }));
    // The tool-calling steps, then one wrap-up call that the model wasted on another tool call.
    assert_eq!(p.requests.len(), MAX_STEPS + 1);
}

#[test]
fn out_of_steps_the_model_gets_one_call_with_tools_refused_to_say_what_it_did() {
    let mut p = Scripted::new([
        tool_reply(&[("a", "list_commands", json!({"filter": "zz"}))]),
        tool_reply(&[("b", "list_commands", json!({"filter": "zz"}))]),
        text_reply("Ho elencato i comandi; non ho disegnato nulla."),
        text_reply("Ciao."),
    ]);
    let mut cfg = Config::new("m");
    cfg.max_steps = 2;
    let mut agent = Agent::new(cfg);
    let turn = agent.send(&mut p, &mut Headless::default(), "loop", &mut ()).unwrap();
    assert_eq!(turn, Turn { reply: "Ho elencato i comandi; non ho disegnato nulla.".into(), steps: 3, tool_calls: 2, stopped: true });
    // The wrap-up request refuses tools; the nudge stays out of the history, the answer goes in.
    assert_eq!(p.requests[2]["tool_choice"], "none");
    assert!(p.requests[2]["tools"].is_array());
    assert!(agent.messages().iter().all(|m| m["content"].as_str().is_none_or(|c| !c.contains("steps allowed"))));
    assert_eq!(agent.messages().last().unwrap()["content"], "Ho elencato i comandi; non ho disegnato nulla.");
    // The conversation carries on.
    let turn = agent.send(&mut p, &mut Headless::default(), "ciao", &mut ()).unwrap();
    assert_eq!((turn.reply.as_str(), turn.stopped), ("Ciao.", false));
}

#[test]
fn old_turns_are_forgotten_in_whole() {
    let mut b = Headless::default();
    let mut agent = Agent::new(Config::new("m"));
    for i in 0..150 {
        let mut p = Scripted::new([tool_reply(&[("t", "list_commands", json!({"filter": "zzz"}))]), text_reply(&format!("turn {i}"))]);
        agent.send(&mut p, &mut b, &format!("prompt {i}"), &mut ()).unwrap();
    }
    let msgs = agent.messages();
    assert!(msgs.len() <= 404, "{}", msgs.len());
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[1]["role"], "user");
    assert_eq!(msgs.last().unwrap()["content"], "turn 149");
}
