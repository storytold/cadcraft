//! The panel end to end with a fake host and a scripted model: onboarding, a turn that draws
//! through the UI thread, one undo step per turn, confirmations.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use cadcraft_assistant::{Scripted, text_reply, tool_reply};
use cadcraft_engine::Session;

use super::*;
use crate::Services;

struct FakeHost {
    key: AtomicBool,
    replies: Mutex<Vec<Value>>,
    events: Mutex<Vec<AssistantEvent>>,
    /// The host asks for the user's agreement until it is given.
    wants_consent: AtomicBool,
    /// How many conversations were started (each asks `provider`).
    providers: std::sync::atomic::AtomicUsize,
}

impl FakeHost {
    fn new(replies: Vec<Value>) -> Arc<Self> {
        Arc::new(Self {
            key: AtomicBool::new(false),
            replies: Mutex::new(replies),
            events: Mutex::default(),
            wants_consent: AtomicBool::new(false),
            providers: Default::default(),
        })
    }
}

impl AssistantHost for FakeHost {
    fn service(&self) -> Service {
        Service { name: "Fake Accounts".into(), url: "https://accounts.example/top-up/".into(), key_prefix: "ck-fant-".into() }
    }
    fn has_key(&self) -> bool {
        self.key.load(Ordering::SeqCst)
    }
    fn connect(&self, key: String) -> Result<Account, String> {
        if !key.starts_with("ck-fant-") {
            return Err("not an account key".into());
        }
        if self.key.swap(true, Ordering::SeqCst) {
            return Err("a key is already set".into());
        }
        Ok(Account { conto_id: "conto-1".into(), model: "m".into(), remaining_usd: Some(0.5) })
    }
    fn change_key(&self, key: String) -> Result<Account, String> {
        if !key.starts_with("ck-fant-") {
            return Err("not an account key".into());
        }
        Ok(Account { conto_id: "conto-2".into(), model: "m2".into(), remaining_usd: Some(9.0) })
    }
    fn account(&self) -> Option<Account> {
        self.has_key().then(|| Account { conto_id: "conto-1".into(), model: "m".into(), remaining_usd: Some(0.5) })
    }
    fn models(&self) -> Result<Vec<ModelInfo>, String> {
        Ok(["a/one", "b/two", "b/two:batch"]
            .iter()
            .map(|id| ModelInfo { id: id.to_string(), name: None, output_usd_per_million: Some(1.0) })
            .collect())
    }
    fn provider(&self) -> Result<(Box<dyn Provider + Send>, String), String> {
        self.providers.fetch_add(1, Ordering::SeqCst);
        let replies = std::mem::take(&mut *self.replies.lock().unwrap());
        Ok((Box::new(Scripted::new(replies)), "m".into()))
    }
    fn balance(&self) -> Option<f64> {
        Some(0.03)
    }
    fn event(&self, event: &AssistantEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
    fn consent(&self) -> Option<Consent> {
        self.wants_consent
            .load(Ordering::SeqCst)
            .then(|| Consent { service: "Fake Accounts".into(), destination: "https://accounts.example/api".into() })
    }
    fn agree(&self) -> Result<(), String> {
        self.wants_consent.store(false, Ordering::SeqCst);
        Ok(())
    }
}

fn app_with(host: Arc<FakeHost>) -> CadApp {
    let mut app = CadApp::new(Session::empty(), Services::default());
    app.session.new_drawing(false);
    app.assistant.set_host(host);
    app
}

/// Run frames until the worker is done.
fn pump(app: &mut CadApp, ctx: &egui::Context) {
    for _ in 0..1000 {
        tick(app, ctx);
        if !app.assistant.busy {
            return;
        }
        if let Some((_, reply)) = app.assistant.confirm.take() {
            let _ = reply.send(false);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the assistant did not finish");
}

fn circles(app: &mut CadApp) -> u64 {
    app.session.execute("entities", &json!({"type": "Circle"})).unwrap()["count"].as_u64().unwrap()
}

#[test]
fn onboarding_then_a_turn_draws_as_one_undo_step() {
    let host = FakeHost::new(vec![
        tool_reply(&[("a", "command_line", json!({"text": "circle 0,0 60"}))]),
        tool_reply(&[("b", "execute", json!({"command": "circle", "params": {"center": [0, 0], "radius": 20}}))]),
        text_reply("Fatto."),
    ]);
    let mut app = app_with(host.clone());
    let ctx = egui::Context::default();
    open(&mut app).unwrap();
    assert!(app.ui.docking.layout.contains(&crate::docking::Panel::Assistant));

    // No key: the onboarding stores one and tells the host which account it belongs to.
    app.assistant.key_input = "  ck-fant-secret  ".into();
    connect(&mut app);
    pump(&mut app, &ctx);
    assert!(host.has_key());
    assert_eq!(*host.events.lock().unwrap(), [AssistantEvent::KeyRegistered { conto_id: "conto-1".into() }]);
    assert!(app.assistant.key_input.is_empty());
    assert!(app.assistant.messages.iter().all(|m| !m.text.contains("ck-fant-secret")), "the key is never shown");

    // A turn draws through the UI thread.
    app.assistant.input = "disegna una flangia".into();
    send(&mut app, &ctx);
    pump(&mut app, &ctx);
    assert_eq!(circles(&mut app), 2);
    assert_eq!(app.assistant.balance, Some(0.03));
    assert!(app.assistant.messages.iter().any(|m| m.role == Role::Assistant && m.text == "Fatto."));
    // The chat keeps what was said, not what was run: no command, no JSON, no status line left.
    assert!(app.assistant.messages.iter().all(|m| matches!(m.role, Role::User | Role::Assistant | Role::Link)), "{:?}", app.assistant.messages);
    assert!(app.assistant.messages.iter().all(|m| !m.text.contains("circle") && !m.text.contains('{')));
    assert_eq!(app.assistant.activity, None);
    // The undo grouping stays off the command line.
    assert!(app.session.log.iter().all(|l| !l.contains("Group")), "{:?}", app.session.log);

    // One undo takes the whole turn back.
    app.session.execute("undo", &json!({})).unwrap();
    assert_eq!(circles(&mut app), 0);
}

#[test]
fn the_status_line_reads_as_plain_words_and_goes_away_when_the_turn_ends() {
    let mut a = Assistant::default();
    let text = |a: &Assistant| a.activity.as_ref().map(|s| s.text.clone());
    let step = |a: &mut Assistant, name: &str, args: Value| update(a, Update::Step { name: name.into(), args });
    a.busy = true;
    step(&mut a, "command_line", json!({"text": "circle 0,0 60"}));
    assert_eq!(text(&a).as_deref(), Some("Running Circle…"));
    // Input that answers a prompt (a point, a keyword, Enter) carries on the command before.
    for input in ["5", "@10,0", "c", "e", ""] {
        step(&mut a, "command_line", json!({"text": input}));
        assert_eq!(text(&a).as_deref(), Some("Running Circle…"), "{input:?}");
    }
    // A full alias names a command; the parameters never show.
    step(&mut a, "execute", json!({"command": "rec", "params": {"corner1": [0, 0], "corner2": [4, 3]}}));
    assert_eq!(text(&a).as_deref(), Some("Running Rectangle…"));
    step(&mut a, "command_line", json!({"text": "OPEN"}));
    assert_eq!(text(&a).as_deref(), Some("Running Open…"));
    step(&mut a, "inspect_drawing", json!({}));
    assert_eq!(text(&a).as_deref(), Some("Looking at the drawing…"));
    // A refused command marks the line, and the error stays out of the chat.
    update(&mut a, Update::StepFailed);
    assert_eq!(a.activity, Some(Activity { text: "Looking at the drawing…".into(), failed: true }));
    assert!(a.messages.is_empty());
    // Confirmations ask in words, not JSON.
    update(&mut a, Update::Confirm { name: "save".into(), args: json!({"path": " C:/plans/bracket.dxf "}), reply: channel().0 });
    assert_eq!(a.confirm.as_ref().map(|c| c.0.as_str()), Some("The assistant wants to save the drawing to C:/plans/bracket.dxf."));
    update(&mut a, Update::Confirm { name: "file".into(), args: json!({"path": "C:/notes.txt", "format": "dxf"}), reply: channel().0 });
    assert_eq!(a.confirm.as_ref().map(|c| c.0.as_str()), Some("The assistant wants to run a command that writes or reads C:/notes.txt."));
    update(&mut a, Update::Confirm { name: "file".into(), args: json!({"text": " wblock "}), reply: channel().0 });
    assert_eq!(a.confirm.as_ref().map(|c| c.0.as_str()), Some("The assistant wants to run a command that can write files: wblock"));
    update(&mut a, Update::Confirm { name: "close".into(), args: json!({"text": "close"}), reply: channel().0 });
    assert_eq!(a.confirm.as_ref().map(|c| c.0.as_str()), Some("The assistant wants to close a drawing: what isn't saved is lost."));
    update(&mut a, Update::Confirm { name: "new_drawing".into(), args: json!({}), reply: channel().0 });
    assert_eq!(a.confirm.as_ref().map(|c| c.0.as_str()), Some("The assistant wants to start a new drawing: the current one will be closed."));
    assert!(update(&mut a, Update::Done { conversation: None, error: None, balance: None, stopped: false, ran_tools: true }));
    assert!(!a.busy);
    assert_eq!(a.activity, None);
    assert!(a.confirm.is_none());
    assert!(a.messages.is_empty(), "nothing that was run is kept in the chat");
    // Out of steps: one short notice, in words.
    assert!(!update(&mut a, Update::Done { conversation: None, error: None, balance: None, stopped: true, ran_tools: false }));
    assert_eq!(a.messages.len(), 1);
    assert!(a.messages[0].role == Role::Notice && a.messages[0].text.contains("ran out of steps"));
}

#[test]
fn without_a_key_the_greeting_links_to_the_account_site_and_the_key_stays_secret() {
    let mut app = app_with(FakeHost::new(vec![]));
    open(&mut app).unwrap();
    // The service's name, key prefix and site come from the host, never from the panel.
    let greeting = app.assistant.messages.iter().find(|m| m.text.contains("To start")).expect("the onboarding text");
    assert!(greeting.text.contains("Fake Accounts") && greeting.text.contains("ck-fant-"), "{}", greeting.text);
    let link = app.assistant.messages.iter().find(|m| m.role == Role::Link).expect("a link to the account site");
    assert!(link.text.ends_with("accounts.example/top-up"), "{}", link.text);
    // The panel renders with the link and the card, and nothing in it is the key.
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| show(&mut app, ui));
    out.textures_delta.clear();
}

#[test]
fn the_credit_is_read_again_on_request_and_a_fresh_start_keeps_only_the_greeting() {
    let host = FakeHost::new(vec![text_reply("ok")]);
    host.key.store(true, Ordering::SeqCst);
    let mut app = app_with(host);
    let ctx = egui::Context::default();
    app.assistant.balance = Some(6.2);
    refresh_balance(&mut app.assistant, &ctx);
    assert!(app.assistant.balance_busy);
    for _ in 0..200 {
        tick(&mut app, &ctx);
        if !app.assistant.balance_busy {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.assistant.balance, Some(0.03));
    app.assistant.input = "ciao".into();
    send(&mut app, &ctx);
    pump(&mut app, &ctx);
    assert!(app.assistant.conversation.is_some());
    assert!(app.assistant.messages.len() > 1);
    // The broom only asks; clearing is the answer.
    app.assistant.clear_asked = true;
    new_conversation(&mut app.assistant);
    assert!(!app.assistant.clear_asked);
    assert!(app.assistant.conversation.is_none());
    assert_eq!(app.assistant.messages.len(), 1, "{:?}", app.assistant.messages);
    assert_eq!(app.assistant.messages[0].role, Role::Assistant);
}

#[test]
fn a_command_the_assistant_left_waiting_is_cancelled_when_the_turn_ends() {
    let host = FakeHost::new(vec![tool_reply(&[("a", "command_line", json!({"text": "line 0,0"}))]), text_reply("Ho iniziato una linea.")]);
    host.key.store(true, Ordering::SeqCst);
    let mut app = app_with(host);
    let ctx = egui::Context::default();
    app.assistant.input = "linea".into();
    send(&mut app, &ctx);
    pump(&mut app, &ctx);
    assert!(app.session.running.is_none(), "the LINE command was left waiting for its next point");
    assert!(app.assistant.messages.iter().any(|m| m.text == "Ho iniziato una linea."));
}

/// A CLOSE built at run time asks nothing, and the engine refuses it on the UI thread too: in the
/// call, and in the frames a LISP routine the call left running ticks in. The routine stops when
/// the turn ends.
#[test]
fn what_a_call_builds_at_run_time_can_t_close_the_drawing() {
    let close = "(command (strcat \"clo\" \"se\"))";
    let host = FakeHost::new(vec![
        tool_reply(&[("a", "execute", json!({"command": "lisp", "params": {"code": close}}))]),
        tool_reply(&[("b", "command_line", json!({"text": format!("(progn (repeat 300000 (setq n 1)) {close})")}))]),
        text_reply("Fatto."),
    ]);
    host.key.store(true, Ordering::SeqCst);
    let mut app = app_with(host);
    let ctx = egui::Context::default();
    let docs = app.session.docs.len();
    app.assistant.input = "chiudi".into();
    send(&mut app, &ctx);
    pump(&mut app, &ctx);
    assert_eq!(app.session.docs.len(), docs);
    // Nothing was asked (a question would have been declined).
    assert!(!app.assistant.messages.iter().any(|m| m.text.contains("Declined")), "{:?}", app.assistant.messages);
    assert!(!app.session.lisp_running() && !app.assistant.started_lisp);
}

/// A routine a call left running ticks in later frames with no consent: its CLOSE is refused. The
/// same routine started by the user closes the drawing.
#[test]
fn a_routine_the_assistant_left_running_ticks_without_consent() {
    let ctx = egui::Context::default();
    let run = |assistants: bool| {
        let mut app = app_with(FakeHost::new(vec![]));
        app.session.new_drawing(true);
        let docs = app.session.docs.len();
        app.session.lisp_eval("(repeat 300000 (setq n 1)) (command \"close\")", false, false, Vec::new()).unwrap();
        assert!(app.session.lisp_running(), "the routine should take several frames");
        app.assistant.started_lisp = assistants;
        for _ in 0..10_000 {
            if !app.session.lisp_running() {
                break;
            }
            app.logic(&ctx);
        }
        assert!(!app.session.lisp_running());
        docs - app.session.docs.len()
    };
    assert_eq!(run(true), 0);
    assert_eq!(run(false), 1);
}

#[test]
fn a_declined_save_writes_nothing_and_the_conversation_continues() {
    let host = FakeHost::new(vec![
        tool_reply(&[("a", "save", json!({"path": "never-written.dxf"}))]),
        text_reply("Va bene, non salvo."),
        text_reply("Ciao di nuovo."),
    ]);
    host.key.store(true, Ordering::SeqCst);
    let mut app = app_with(host);
    let ctx = egui::Context::default();
    app.assistant.input = "salva".into();
    send(&mut app, &ctx);
    pump(&mut app, &ctx);
    assert!(!std::path::Path::new("never-written.dxf").exists());
    assert!(app.assistant.messages.iter().any(|m| m.text.contains("Declined")));
    // The same conversation (and model endpoint) carries on.
    app.assistant.input = "ciao".into();
    send(&mut app, &ctx);
    pump(&mut app, &ctx);
    assert!(app.assistant.messages.iter().any(|m| m.text == "Ciao di nuovo."));
}

#[test]
fn errors_are_shown_not_raised() {
    // The scripted model runs out of replies: the provider fails mid-conversation.
    let host = FakeHost::new(vec![]);
    host.key.store(true, Ordering::SeqCst);
    let mut app = app_with(host);
    let ctx = egui::Context::default();
    app.assistant.input = "ciao".into();
    send(&mut app, &ctx);
    pump(&mut app, &ctx);
    assert!(app.assistant.messages.iter().any(|m| m.role == Role::Error));
    // A bad key is refused and nothing is stored.
    let host = FakeHost::new(vec![]);
    let mut app = app_with(host.clone());
    app.assistant.key_input = "sk-or-wrong".into();
    connect(&mut app);
    pump(&mut app, &ctx);
    assert!(!host.has_key());
    assert!(app.assistant.messages.iter().any(|m| m.role == Role::Error));
}

#[test]
fn without_a_host_the_panel_says_so() {
    let mut app = CadApp::new(Session::empty(), Services::default());
    app.session.new_drawing(false);
    assert!(!app.assistant.available());
    let ctx = egui::Context::default();
    app.assistant.input = "ciao".into();
    send(&mut app, &ctx);
    assert!(!app.assistant.busy);
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| show(&mut app, ui));
    out.textures_delta.clear();
}

#[test]
fn language_and_model_choices_reach_the_agent_and_persist() {
    let host = FakeHost::new(vec![text_reply("Ciao."), text_reply("Hello.")]);
    host.key.store(true, Ordering::SeqCst);
    let mut app = app_with(host);
    let ctx = egui::Context::default();
    app.assistant.set_language(Language::Italian);
    app.assistant.set_model(Some("b/two".into()));
    // A batch variant can't answer a chat: refused, the choice falls back to the suggestion.
    app.assistant.set_model(Some("b/two:batch".into()));
    assert_eq!(app.assistant.model, None);
    app.assistant.set_model(Some("b/two".into()));
    app.assistant.input = "ciao".into();
    send(&mut app, &ctx);
    pump(&mut app, &ctx);
    let agent = &app.assistant.conversation.as_ref().unwrap().agent;
    assert_eq!(agent.config().model, "b/two");
    assert!(agent.messages()[0]["content"].as_str().unwrap().contains("Always reply in Italian"));
    // Changed mid-conversation: the next turn follows, history kept.
    app.assistant.set_language(Language::English);
    app.assistant.set_model(Some("a/one".into()));
    let agent = &app.assistant.conversation.as_ref().unwrap().agent;
    assert!(agent.messages()[0]["content"].as_str().unwrap().contains("Always reply in English"));
    assert_eq!(agent.config().model, "a/one");
    assert!(agent.messages().len() > 1);
    // Kept across restarts; defaults are not written.
    let prefs = app.prefs_json();
    let mut again = CadApp::new(Session::empty(), Services::default());
    again.load_prefs(&prefs);
    assert_eq!((again.assistant.language, again.assistant.model.as_deref()), (Language::English, Some("a/one")));
    let fresh = CadApp::new(Session::empty(), Services::default());
    assert!(!fresh.prefs_json().contains("assistant"));
    again.load_prefs(r#"{"assistantLanguage": "klingon", "assistantModel": "x/y:batch"}"#);
    assert_eq!(again.assistant.language, Language::English);
}

#[test]
fn the_model_list_leaves_out_batch_variants() {
    let host = FakeHost::new(vec![]);
    host.key.store(true, Ordering::SeqCst);
    let mut app = app_with(host);
    let ctx = egui::Context::default();
    request_models(&mut app.assistant, &ctx);
    for _ in 0..200 {
        tick(&mut app, &ctx);
        if app.assistant.settings.models.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let ids: Vec<String> = match &app.assistant.settings.models {
        Some(Ok(list)) => list.iter().map(|m| m.id.clone()).collect(),
        other => panic!("{other:?}"),
    };
    assert_eq!(ids, ["a/one", "b/two"]);
}

#[test]
fn changing_the_key_switches_account_and_starts_a_new_conversation() {
    let host = FakeHost::new(vec![text_reply("ok")]);
    host.key.store(true, Ordering::SeqCst);
    let mut app = app_with(host.clone());
    let ctx = egui::Context::default();
    app.assistant.input = "ciao".into();
    send(&mut app, &ctx);
    pump(&mut app, &ctx);
    assert!(app.assistant.conversation.is_some());
    app.assistant.settings.new_key = "ck-fant-new".into();
    change_key(&mut app);
    for _ in 0..200 {
        tick(&mut app, &ctx);
        if !app.assistant.settings.busy {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.assistant.settings.new_key.is_empty());
    assert!(matches!(app.assistant.settings.result, Some(Ok(_))));
    assert!(app.assistant.conversation.is_none());
    assert_eq!(app.assistant.account.as_ref().map(|a| a.conto_id.as_str()), Some("conto-2"));
    assert_eq!(host.events.lock().unwrap().last(), Some(&AssistantEvent::KeyChanged { conto_id: "conto-2".into() }));
    assert!(app.assistant.messages.iter().all(|m| !m.text.contains("ck-fant-new")), "the key is never shown");
    // A bad key is refused and the old one stays.
    app.assistant.settings.new_key = "sk-or-x".into();
    change_key(&mut app);
    for _ in 0..200 {
        tick(&mut app, &ctx);
        if !app.assistant.settings.busy {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(matches!(app.assistant.settings.result, Some(Err(_))));
}

/// Nothing leaves the computer before the user agrees: a no sends nothing and says why, a yes is
/// asked once and the turn goes on.
#[test]
fn the_first_request_waits_for_the_users_agreement() {
    let host = FakeHost::new(vec![text_reply("Eccomi.")]);
    host.key.store(true, Ordering::SeqCst);
    host.wants_consent.store(true, Ordering::SeqCst);
    let mut app = app_with(host.clone());
    let ctx = egui::Context::default();

    // `pump` answers every question with a no.
    app.assistant.input = "disegna un cerchio".into();
    send(&mut app, &ctx);
    pump(&mut app, &ctx);
    assert_eq!(host.providers.load(Ordering::SeqCst), 0, "no conversation was started");
    assert!(app.assistant.messages.iter().any(|m| m.role == Role::Error && m.text.contains("agreement")), "{:?}", app.assistant.messages);

    // A yes: the question names the service and where the requests go, and the turn runs.
    app.assistant.input = "disegna un cerchio".into();
    send(&mut app, &ctx);
    let mut asked = None;
    for _ in 0..1000 {
        tick(&mut app, &ctx);
        if let Some((question, reply)) = app.assistant.confirm.take() {
            asked = Some(question);
            let _ = reply.send(true);
        }
        if !app.assistant.busy {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let asked = asked.unwrap();
    assert!(asked.contains("Fake Accounts") && asked.contains("https://accounts.example/api"), "{asked}");
    assert!(app.assistant.messages.iter().any(|m| m.text == "Eccomi."));
    assert!(host.consent().is_none(), "the agreement is recorded");
}

/// The palette drawn in the window: the greeting and the credit, then the agreement question
/// before the first request, then the reply. With `CADCRAFT_ASSISTANT_SHOTS=<dir>` each stage is
/// also rendered to a PNG there.
#[test]
fn the_palette_draws_its_greeting_the_agreement_and_the_reply() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

    let shots = std::env::var_os("CADCRAFT_ASSISTANT_SHOTS").map(std::path::PathBuf::from);
    let host = FakeHost::new(vec![text_reply("Done: a circle of radius 5 at the origin.")]);
    host.key.store(true, Ordering::SeqCst);
    host.wants_consent.store(true, Ordering::SeqCst);
    let mut app = CadApp::new(Session::default(), Services::default());
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    app.assistant.set_host(host);
    let builder = Harness::builder().with_size(egui::vec2(1280.0, 800.0));
    let builder = if shots.is_some() { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    // egui gives a label's text as the node's value.
    let shows = |h: &Harness<'_, CadApp>, text: &str| {
        h.root().get_all(egui_kittest::kittest::by()).any(|n| {
            let node = n.accesskit_node();
            node.label().is_some_and(|l| l.contains(text)) || node.value().is_some_and(|v| v.contains(text))
        })
    };
    let until = |h: &mut Harness<'_, CadApp>, text: &str| {
        for _ in 0..400 {
            h.run_steps(2);
            if shows(h, text) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the palette never showed {text:?}");
    };
    let shot = |h: &mut Harness<'_, CadApp>, name: &str| {
        if let Some(dir) = &shots {
            std::fs::create_dir_all(dir).unwrap();
            h.render().unwrap().save(dir.join(name)).unwrap();
        }
    };

    h.state_mut().run("ui.panel.assistant", json!({})).unwrap();
    until(&mut h, "Credit available: 0.50 USD");
    assert!(shows(&h, "Hi! I'm the AI Assistant"));
    shot(&mut h, "cadcraft-assistant.png");

    h.state_mut().assistant.input = "draw a circle of radius 5 at the origin".into();
    let ctx = h.ctx.clone();
    send(h.state_mut(), &ctx);
    until(&mut h, "Allow this for CADCraft?");
    assert!(shows(&h, "Fake Accounts (https://accounts.example/api)"));
    shot(&mut h, "cadcraft-assistant-consent.png");

    assert!(h.query_by_label("Allow").is_some() && h.query_by_label("Decline").is_some());
    let (_, reply) = h.state_mut().assistant.confirm.take().unwrap();
    reply.send(true).unwrap();
    until(&mut h, "Done: a circle of radius 5 at the origin.");
    shot(&mut h, "cadcraft-assistant-reply.png");
}
