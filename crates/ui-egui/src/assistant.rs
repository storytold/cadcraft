//! The AI Assistant palette (Window ▸ AI Assistant): a chat whose agent carries out drafting
//! requests with CADCraft's own commands.
//!
//! The panel is thin, like every other one. The account key, the model and the network belong to
//! the host ([`AssistantHost`], injected by the desktop app); without a host (the web build) the
//! panel says the assistant isn't available yet. The agent loop (`cadcraft-assistant`) runs on a
//! worker thread so the interface never waits for the model; its tools reach the drawing through
//! the control methods, run here on the UI thread ([`tick`]). One turn is one undo step.
//!
//! The key is write-only for the interface: the onboarding stores one when none is set, the
//! settings window can replace it with a new one, and nothing in this crate can read or show it.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use cadcraft_assistant::{Agent, Config, Event, Observer, Provider};
use cadcraft_mcp::Backend;
use egui::{RichText, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::control::{ControlRequest, Outcome};
use crate::theme::Tokens;

/// How long a tool may wait for the UI thread to run it.
const TOOL_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a confirmation card waits for an answer before counting as declined.
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(600);
/// Messages kept in the panel; older ones scroll away for good.
const MAX_MESSAGES: usize = 300;
/// Undo grouping around a turn: handled here, not through `engine.execute`, so the command line
/// doesn't fill with "Group begun".
const UNDO_BEGIN: &str = "assistant.undo.begin";
const UNDO_END: &str = "assistant.undo.end";
/// The account service behind the assistant, as the panel names it. The panel itself doesn't
/// know which service that is: the host says.
#[derive(Clone, Debug, PartialEq)]
pub struct Service {
    /// The service's name, as shown to the user.
    pub name: String,
    /// Where an account key comes from and where the credit is topped up.
    pub url: String,
    /// What an account key starts with, for the onboarding text.
    pub key_prefix: String,
}

impl Service {
    /// The site as it reads in a sentence: the URL without its scheme and trailing slash.
    fn site(&self) -> &str {
        self.url.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/')
    }
}

/// A connected account — never the key.
#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    /// The account's identifier: it can be shown and recorded, unlike the key.
    pub conto_id: String,
    /// The model the account service suggests.
    pub model: String,
    pub remaining_usd: Option<f64>,
}

/// A model the account can use.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelInfo {
    /// The id sent with each request, e.g. `anthropic/claude-sonnet-5`.
    pub id: String,
    /// A readable name, when the account service gives one.
    pub name: Option<String>,
    /// The price of a million output tokens, in USD.
    pub output_usd_per_million: Option<f64>,
}

impl ModelInfo {
    /// Batch variants (`…:batch`) can't answer a chat: they are left out of the list.
    pub fn is_chat(id: &str) -> bool {
        !id.to_ascii_lowercase().ends_with(":batch")
    }

    fn label(&self) -> String {
        let name = self.name.as_deref().unwrap_or(&self.id);
        match self.output_usd_per_million {
            Some(p) => format!("{name}  ·  {p:.2} USD/M"),
            None => name.to_string(),
        }
    }
}

/// What the host is told.
#[derive(Clone, Debug, PartialEq)]
pub enum AssistantEvent {
    /// Onboarding finished: the key of account `conto_id` is now stored on this computer. The
    /// host records it wherever keys are managed (to be changed or disabled there).
    KeyRegistered { conto_id: String },
    /// The user replaced the key: the stored key now belongs to account `conto_id`.
    KeyChanged { conto_id: String },
}

/// The language the assistant replies in (Assistant settings ▸ Assistant language).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    /// The language the user writes in.
    #[default]
    Auto,
    English,
    Italian,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::Auto, Language::English, Language::Italian];

    /// The code kept in the preferences.
    pub fn code(self) -> &'static str {
        match self {
            Language::Auto => "auto",
            Language::English => "en",
            Language::Italian => "it",
        }
    }

    pub fn parse(code: &str) -> Option<Language> {
        Language::ALL.into_iter().find(|l| l.code() == code)
    }

    /// The name shown in the settings: a language in its own words, so anyone can find theirs.
    fn name(self) -> &'static str {
        match self {
            Language::Auto => crate::tl!("Automatic (the language you write in)"),
            Language::English => "English",
            Language::Italian => "Italiano",
        }
    }

    /// The instruction added to the system prompt.
    fn instruction(self) -> Option<String> {
        match self {
            Language::Auto => None,
            Language::English => Some("Always reply in English, whatever language the user writes in.".into()),
            Language::Italian => Some("Always reply in Italian, whatever language the user writes in.".into()),
        }
    }
}

/// The host side of the assistant. Nothing here returns the key: the interface can store one or
/// replace it with a new one, never read it.
pub trait AssistantHost: Send + Sync {
    /// The account service: its name, its site and what its keys look like.
    fn service(&self) -> Service;
    fn has_key(&self) -> bool;
    /// Check `key` with the account service and store it, if no key is stored yet. Blocking: runs
    /// on a worker thread.
    fn connect(&self, key: String) -> Result<Account, String>;
    /// Check `key` with the account service and put it in place of the stored one. Blocking.
    fn change_key(&self, key: String) -> Result<Account, String>;
    /// The account of the stored key, when it can be read. Blocking.
    fn account(&self) -> Option<Account>;
    /// The models the account can use for a chat. Blocking.
    fn models(&self) -> Result<Vec<ModelInfo>, String>;
    /// A model endpoint for a conversation and the model to ask for. Blocking.
    fn provider(&self) -> Result<(Box<dyn Provider + Send>, String), String>;
    /// The credit left, when it can be read. Blocking.
    fn balance(&self) -> Option<f64>;
    fn event(&self, event: &AssistantEvent);
    /// What the user has to agree to before anything is sent, while they haven't: the palette
    /// asks before the turn and records a yes with [`agree`](Self::agree). Blocking.
    fn consent(&self) -> Option<Consent> {
        None
    }
    /// Record the agreement [`consent`](Self::consent) asked for. Blocking.
    fn agree(&self) -> Result<(), String> {
        Ok(())
    }
}

/// What a turn sends, and where, for the user to agree to once.
#[derive(Clone, Debug, PartialEq)]
pub struct Consent {
    /// The service, as the user knows it.
    pub service: String,
    /// The service's address. Which provider serves the chosen model is the service's business,
    /// so the question names none.
    pub destination: String,
}

/// The question that asks for [`Consent`].
fn consent_text(c: &Consent) -> String {
    crate::tf!(
        "To answer, the AI Assistant sends your requests, what it reads from the drawing and what its commands return to {service} ({destination}) and to the provider of the chosen model. Allow this for CADCraft?",
        service = c.service,
        destination = c.destination,
    )
}

#[derive(Clone, Debug, PartialEq)]
enum Role {
    User,
    Assistant,
    Notice,
    Error,
    /// A link to the account site; the text is its label.
    Link,
}

#[derive(Clone, Debug)]
struct Message {
    role: Role,
    text: String,
}

/// What the assistant is doing right now, in plain words: shown next to the spinner while a turn
/// runs, gone when it ends. The chat keeps what was said, never what was run.
#[derive(Clone, Debug, PartialEq)]
struct Activity {
    text: String,
    /// The last command was refused; the agent reads the error and usually tries another way.
    failed: bool,
}

/// A conversation: the agent (with its history) and the model endpoint (with its key in memory).
struct Conversation {
    agent: Agent,
    provider: Box<dyn Provider + Send>,
}

/// From the worker threads to the panel.
enum Update {
    Connected(Result<Account, String>),
    KeyChanged(Result<Account, String>),
    Account(Option<Account>),
    /// The credit, read again on request.
    Balance(Option<f64>),
    Models(Result<Vec<ModelInfo>, String>),
    Text(String),
    /// The agent asked for a tool.
    Step {
        name: String,
        args: Value,
    },
    /// The last tool was refused.
    StepFailed,
    Declined {
        name: String,
    },
    Confirm {
        name: String,
        args: Value,
        reply: Sender<bool>,
    },
    /// Nothing has been sent: the user is asked to agree first.
    Consent {
        consent: Consent,
        reply: Sender<bool>,
    },
    Done {
        conversation: Option<Conversation>,
        error: Option<String>,
        balance: Option<f64>,
        /// The step budget ran out; the reply says what was done.
        stopped: bool,
        /// Tools ran (or the turn failed mid-way): a command may be left waiting for input.
        ran_tools: bool,
    },
}

/// The settings window.
#[derive(Default)]
struct Settings {
    open: bool,
    new_key: String,
    busy: bool,
    /// The outcome of the last key change: `Ok` message or error.
    result: Option<Result<String, String>>,
    /// The model list: `None` until asked for, then the list or why it couldn't be read.
    models: Option<Result<Vec<ModelInfo>, String>>,
    models_requested: bool,
    model_filter: String,
}

pub struct Assistant {
    host: Option<Arc<dyn AssistantHost>>,
    messages: Vec<Message>,
    input: String,
    key_input: String,
    busy: bool,
    /// The current action while `busy`, for the status line under the messages.
    activity: Option<Activity>,
    conversation: Option<Conversation>,
    /// A question to the user (what the assistant wants to do) and where its answer goes.
    confirm: Option<(String, Sender<bool>)>,
    account: Option<Account>,
    /// The account of the stored key was asked for (once per run).
    account_requested: bool,
    balance: Option<f64>,
    /// The credit is being read again (a click on it).
    balance_busy: bool,
    /// A call of this turn left a LISP routine running: it ticks with no consent, and stops when
    /// the turn ends.
    pub(crate) started_lisp: bool,
    /// The fresh-start question is up.
    clear_asked: bool,
    /// The reply language, kept in the preferences.
    pub language: Language,
    /// The model chosen in the settings, kept in the preferences; `None` follows the account's
    /// suggestion.
    pub model: Option<String>,
    settings: Settings,
    updates_tx: Sender<Update>,
    updates_rx: Option<Receiver<Update>>,
    tools_tx: Sender<ControlRequest>,
    tools_rx: Option<Receiver<ControlRequest>>,
}

impl Default for Assistant {
    fn default() -> Self {
        let (updates_tx, updates_rx) = channel();
        let (tools_tx, tools_rx) = channel();
        Self {
            host: None,
            messages: Vec::new(),
            input: String::new(),
            key_input: String::new(),
            busy: false,
            activity: None,
            conversation: None,
            confirm: None,
            account: None,
            account_requested: false,
            balance: None,
            balance_busy: false,
            started_lisp: false,
            clear_asked: false,
            language: Language::default(),
            model: None,
            settings: Settings::default(),
            updates_tx,
            updates_rx: Some(updates_rx),
            tools_tx,
            tools_rx: Some(tools_rx),
        }
    }
}

impl Assistant {
    /// Give the assistant its host (the desktop app does, at startup).
    pub fn set_host(&mut self, host: Arc<dyn AssistantHost>) {
        self.host = Some(host);
    }

    pub fn available(&self) -> bool {
        self.host.is_some()
    }

    /// Change the reply language; a conversation in hand follows it from the next turn (one on a
    /// worker thread follows it when it comes back).
    pub fn set_language(&mut self, language: Language) {
        self.language = language;
        if let Some(c) = &mut self.conversation {
            c.agent.set_instructions(language.instruction());
        }
    }

    /// Choose the model (`None`: the account's suggestion). A conversation in hand switches at
    /// its next turn; a new one starts with it.
    pub fn set_model(&mut self, model: Option<String>) {
        let model = model.filter(|m| !m.trim().is_empty() && ModelInfo::is_chat(m));
        if let (Some(m), Some(c)) = (&model, &mut self.conversation) {
            c.agent.set_model(m.clone());
        }
        // Back to the suggestion: the next conversation asks the account again.
        if model.is_none() && self.model.is_some() {
            self.conversation = None;
        }
        self.model = model;
    }

    fn push(&mut self, role: Role, text: impl Into<String>) {
        self.messages.push(Message { role, text: text.into() });
        if self.messages.len() > MAX_MESSAGES {
            let extra = self.messages.len() - MAX_MESSAGES;
            self.messages.drain(..extra);
        }
    }

    fn greet(&mut self) {
        if !self.messages.is_empty() {
            return;
        }
        self.push(
            Role::Assistant,
            crate::tl!("Hi! I'm the AI Assistant. Tell me what to draw or change in the drawing, and I'll do it with CADCraft's commands."),
        );
        if let Some(host) = self.host.clone()
            && !host.has_key()
        {
            let s = host.service();
            self.push(
                Role::Assistant,
                crate::tf!(
                    "To start, connect your {service} account: paste its account key (it starts with {prefix}). The key stays on this computer and is never shown again.",
                    service = s.name,
                    prefix = s.key_prefix
                ),
            );
            self.push(Role::Link, crate::tf!("Get an account key or top up your credit at {site}", site = s.site()));
        }
    }
}

/// Window ▸ AI Assistant: show the palette.
pub fn open(app: &mut CadApp) -> Result<Value, String> {
    let r = crate::docking::command(app, false, &json!({ "operation": "open", "panel": crate::docking::Panel::Assistant.id() }))?;
    crate::i18n::with_language(app.language(), || app.assistant.greet());
    Ok(r)
}

/// Run on a worker thread, or report that this build can't.
fn spawn(work: impl FnOnce() + Send + 'static) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::Builder::new().name("cadcraft-assistant".into()).spawn(work).map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(target_arch = "wasm32")]
    {
        drop(work);
        Err("The AI Assistant is not available in this version yet.".into())
    }
}

/// The agent's way into the drawing: control methods, queued for the UI thread.
struct UiBackend {
    tx: Sender<ControlRequest>,
    ctx: egui::Context,
}

impl UiBackend {
    fn send(&mut self, req: ControlRequest, pending: crate::control::PendingReply) -> Result<Value, String> {
        self.tx.send(req).map_err(|_| "CADCraft is closing".to_string())?;
        self.ctx.request_repaint();
        let v = pending.wait(TOOL_TIMEOUT);
        if v.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(v.get("result").cloned().unwrap_or(Value::Null))
        } else {
            Err(v.get("error").and_then(Value::as_str).unwrap_or("error").to_string())
        }
    }
}

impl Backend for UiBackend {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let (req, pending) = ControlRequest::new(method, params);
        self.send(req, pending)
    }
    /// The UI thread runs the call: the consent goes with it.
    fn call_with(&mut self, method: &str, params: Value, consent: cadcraft_engine::consent::Consent) -> Result<Value, String> {
        let (req, pending) = ControlRequest::new(method, params);
        self.send(req.with_consent(consent), pending)
    }
    /// Command-level tools only: the assistant doesn't drive the pointer.
    fn has_ui(&self) -> bool {
        false
    }
    fn describe(&self) -> String {
        "CADCraft".into()
    }
}

/// The agent's progress and confirmations, sent to the panel.
struct UiObserver {
    tx: Sender<Update>,
    ctx: egui::Context,
}

impl UiObserver {
    fn send(&self, u: Update) -> bool {
        let sent = self.tx.send(u).is_ok();
        self.ctx.request_repaint();
        sent
    }
}

fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", s.get(..at).unwrap_or(s)),
        None => s.to_string(),
    }
}

/// The translated label of the command that `word` names, without a menu ellipsis. A short word
/// is taken only as a command's own name, not as an alias: `c`, `e` or `u` typed while a command
/// runs are its keywords (Close, Exit, Undo), and the agent is taught full command names.
fn command_label(word: &str) -> Option<String> {
    let word = word.trim().trim_start_matches(['\'', '_', '-', '.']);
    if word.is_empty() {
        return None;
    }
    let id = if word.chars().count() >= 3 { cadcraft_engine::cmd::resolve_alias(word) } else { word.to_ascii_lowercase() };
    let spec = cadcraft_engine::cmd::find_command(&id)?;
    Some(crate::i18n::t(spec.label).trim_end_matches(['.', '…']).to_string())
}

/// What a tool call does, in the user's words: the command it runs, never its parameters.
/// Command-line text that doesn't start a command (a point, a distance, a keyword, Enter) carries
/// on the command before it, so `previous` stays.
fn describe(name: &str, args: &Value, previous: Option<&str>) -> String {
    let command = match name {
        "command_line" => args.get("text").and_then(Value::as_str).and_then(|t| t.split_whitespace().next()).and_then(command_label),
        "execute" => args.get("command").and_then(Value::as_str).and_then(command_label),
        "script" => return crate::tl!("Running a script…").to_string(),
        "list_commands" => return crate::tl!("Looking up commands…").to_string(),
        "inspect_drawing" | "query_entities" => return crate::tl!("Looking at the drawing…").to_string(),
        "new_drawing" => return crate::tl!("Starting a new drawing…").to_string(),
        "open" => return crate::tl!("Opening a drawing…").to_string(),
        "save" => return crate::tl!("Saving the drawing…").to_string(),
        "cancel" => return crate::tl!("Cancelling…").to_string(),
        _ => None,
    };
    match (command, previous) {
        (Some(c), _) => crate::tf!("Running {command}…", command = c),
        (None, Some(p)) => p.to_string(),
        (None, None) => crate::tl!("Working…").to_string(),
    }
}

/// The question a confirmation card asks, in words: what the tool does and the file it touches.
fn confirm_text(name: &str, args: &Value) -> String {
    let path = args.get("path").and_then(Value::as_str).map(|p| clip(p.trim(), 200)).filter(|p| !p.is_empty());
    match (name, path) {
        ("new_drawing", _) => crate::tl!("The assistant wants to start a new drawing: the current one will be closed.").to_string(),
        ("open", Some(p)) => crate::tf!("The assistant wants to open {path}.", path = p),
        ("open", None) => crate::tl!("The assistant wants to open a drawing.").to_string(),
        ("save", Some(p)) => crate::tf!("The assistant wants to save the drawing to {path}.", path = p),
        ("save", None) => crate::tl!("The assistant wants to save the drawing.").to_string(),
        ("close", _) => crate::tl!("The assistant wants to close a drawing: what isn't saved is lost.").to_string(),
        ("file", Some(p)) => crate::tf!("The assistant wants to run a command that writes or reads {path}.", path = p),
        // A command line or script: what it runs.
        ("file", None) => match args.get("text").and_then(Value::as_str).map(|t| clip(t.trim(), 200)).filter(|t| !t.is_empty()) {
            Some(t) => crate::tf!("The assistant wants to run a command that can write files: {command}", command = t),
            None => crate::tl!("The assistant wants to run a command that can write files.").to_string(),
        },
        _ => crate::tf!("The assistant wants to run {tool}.", tool = name),
    }
}

impl Observer for UiObserver {
    fn confirm(&mut self, name: &str, arguments: &Value) -> bool {
        let (reply, answer) = channel();
        if !self.send(Update::Confirm { name: name.to_string(), args: arguments.clone(), reply }) {
            return false;
        }
        answer.recv_timeout(CONFIRM_TIMEOUT).unwrap_or(false)
    }
    fn event(&mut self, event: Event<'_>) {
        let u = match event {
            Event::Text(t) => Update::Text(t.to_string()),
            Event::ToolCall { name, arguments } => Update::Step { name: name.to_string(), args: arguments.clone() },
            Event::ToolResult { ok: false, name, text } => {
                // The error goes back to the model, not into the chat.
                log::debug!("AI Assistant: {name} was refused: {}", clip(text, 300));
                Update::StepFailed
            }
            Event::ToolResult { .. } => return,
            Event::Declined { name } => Update::Declined { name: name.to_string() },
        };
        self.send(u);
    }
}

fn connect(app: &mut CadApp) {
    let a = &mut app.assistant;
    let key = std::mem::take(&mut a.key_input).trim().to_string();
    let Some(host) = a.host.clone() else { return };
    if key.is_empty() || a.busy {
        return;
    }
    a.busy = true;
    a.activity = Some(Activity { text: crate::tl!("Checking the key…").to_string(), failed: false });
    let tx = a.updates_tx.clone();
    if let Err(e) = spawn(move || {
        let _ = tx.send(Update::Connected(host.connect(key)));
    }) {
        a.busy = false;
        a.activity = None;
        a.push(Role::Error, e);
    }
}

fn send(app: &mut CadApp, ctx: &egui::Context) {
    let a = &mut app.assistant;
    let prompt = a.input.trim().to_string();
    let Some(host) = a.host.clone() else { return };
    if prompt.is_empty() || a.busy {
        return;
    }
    a.input.clear();
    a.push(Role::User, prompt.clone());
    a.busy = true;
    let conversation = a.conversation.take();
    let instructions = a.language.instruction();
    let chosen = a.model.clone();
    let (updates, tools, ctx) = (a.updates_tx.clone(), a.tools_tx.clone(), ctx.clone());
    let started = spawn(move || {
        let done = |conversation, error, balance, stopped, ran_tools| {
            let _ = updates.send(Update::Done { conversation, error, balance, stopped, ran_tools });
            ctx.request_repaint();
        };
        // Agreement comes before anything leaves the computer; a no keeps the conversation as it was.
        if let Some(consent) = host.consent() {
            let (reply, answer) = channel();
            let _ = updates.send(Update::Consent { consent, reply });
            ctx.request_repaint();
            if !answer.recv_timeout(CONFIRM_TIMEOUT).unwrap_or(false) {
                return done(
                    conversation,
                    Some(crate::tl!("Nothing was sent: the AI Assistant needs your agreement first.").to_string()),
                    None,
                    false,
                    false,
                );
            }
            if let Err(e) = host.agree() {
                return done(conversation, Some(e), None, false, false);
            }
        }
        let mut conv = match conversation {
            Some(c) => c,
            None => match host.provider() {
                Ok((provider, suggested)) => {
                    let model = chosen.unwrap_or(suggested);
                    Conversation { agent: Agent::new(Config { instructions, ..Config::new(model) }), provider }
                }
                Err(e) => return done(None, Some(e), None, false, false),
            },
        };
        let mut backend = UiBackend { tx: tools, ctx: ctx.clone() };
        let mut observer = UiObserver { tx: updates.clone(), ctx: ctx.clone() };
        let _ = backend.call(UNDO_BEGIN, Value::Null);
        let r = conv.agent.send(conv.provider.as_mut(), &mut backend, &prompt, &mut observer);
        let _ = backend.call(UNDO_END, Value::Null);
        // Messages the panel can translate: the endpoint's own text, or a fixed sentence.
        let (error, stopped, ran_tools) = match r {
            Ok(t) => (None, t.stopped, t.tool_calls > 0),
            Err(cadcraft_assistant::Error::StepLimit { .. }) => (None, true, true),
            Err(cadcraft_assistant::Error::Provider(m)) => (Some(m), false, true),
            Err(other) => (Some(other.to_string()), false, true),
        };
        done(Some(conv), error, host.balance(), stopped, ran_tools);
    });
    if let Err(e) = started {
        let a = &mut app.assistant;
        a.busy = false;
        a.push(Role::Error, e);
    }
}

/// Settings ▸ Change key: check the new key and put it in place of the stored one.
fn change_key(app: &mut CadApp) {
    let a = &mut app.assistant;
    let key = std::mem::take(&mut a.settings.new_key).trim().to_string();
    let Some(host) = a.host.clone() else { return };
    if key.is_empty() || a.settings.busy || a.busy {
        return;
    }
    a.settings.busy = true;
    a.settings.result = None;
    let tx = a.updates_tx.clone();
    if let Err(e) = spawn(move || {
        let _ = tx.send(Update::KeyChanged(host.change_key(key)));
    }) {
        a.settings.busy = false;
        a.settings.result = Some(Err(e));
    }
}

/// Read the credit again (a click on it in the header).
fn refresh_balance(a: &mut Assistant, ctx: &egui::Context) {
    let Some(host) = a.host.clone() else { return };
    // During a turn the conversation is on the worker thread, and the turn's end brings a
    // fresh reading anyway.
    if a.balance_busy || a.busy || !host.has_key() {
        return;
    }
    a.balance_busy = true;
    let (tx, ctx) = (a.updates_tx.clone(), ctx.clone());
    let started = spawn(move || {
        let _ = tx.send(Update::Balance(host.balance()));
        ctx.request_repaint();
    });
    if started.is_err() {
        a.balance_busy = false;
    }
}

/// Forget the conversation and start over with the greeting.
fn new_conversation(a: &mut Assistant) {
    a.conversation = None;
    a.messages.clear();
    a.clear_asked = false;
    a.greet();
}

/// Read the account of the stored key once, for the credit and the settings window.
fn request_account(a: &mut Assistant, ctx: &egui::Context) {
    let Some(host) = a.host.clone() else { return };
    if a.account_requested || a.account.is_some() || !host.has_key() {
        return;
    }
    a.account_requested = true;
    let (tx, ctx) = (a.updates_tx.clone(), ctx.clone());
    let _ = spawn(move || {
        let _ = tx.send(Update::Account(host.account()));
        ctx.request_repaint();
    });
}

/// Read the model list once, when the settings window first shows it.
fn request_models(a: &mut Assistant, ctx: &egui::Context) {
    let Some(host) = a.host.clone() else { return };
    if a.settings.models_requested || !host.has_key() {
        return;
    }
    a.settings.models_requested = true;
    let (tx, ctx) = (a.updates_tx.clone(), ctx.clone());
    if let Err(e) = spawn(move || {
        let _ = tx.send(Update::Models(host.models()));
        ctx.request_repaint();
    }) {
        a.settings.models = Some(Err(e));
    }
}

/// Per frame: run the tools the agent asked for, and take in its progress.
pub fn tick(app: &mut CadApp, ctx: &egui::Context) {
    if let Some(rx) = app.assistant.tools_rx.take() {
        while let Ok(req) = rx.try_recv() {
            if !req.begin() {
                continue;
            }
            let v = match req.method.as_str() {
                UNDO_BEGIN | UNDO_END => {
                    let mode = if req.method == UNDO_BEGIN { "begin" } else { "end" };
                    match app.session.execute("undo", &json!({ "mode": mode })) {
                        Ok(r) => json!({"ok": true, "result": r}),
                        Err(e) => json!({"ok": false, "error": e.to_string()}),
                    }
                }
                _ => {
                    let lisp_before = app.session.lisp_running();
                    let handle = |app: &mut CadApp| match crate::control::handle(app, ctx, &req) {
                        Outcome::Done(v) => v,
                        Outcome::Screenshot { .. } => json!({"ok": false, "error": "screenshots are not available to the assistant"}),
                    };
                    let v = match req.consent {
                        Some(c) => cadcraft_engine::consent::with(c, || handle(app)),
                        None => handle(app),
                    };
                    // A LISP routine the call left running goes on in later frames, outside the
                    // call: it runs with no consent at all, and stops when the turn ends.
                    if !lisp_before && app.session.lisp_running() {
                        app.assistant.started_lisp = true;
                    }
                    v
                }
            };
            let _ = req.reply.send(v);
            ctx.request_repaint();
        }
        app.assistant.tools_rx = Some(rx);
    }
    let Some(rx) = app.assistant.updates_rx.take() else { return };
    while let Ok(u) = rx.try_recv() {
        let turn_ended = crate::i18n::with_language(app.language(), || update(&mut app.assistant, u));
        // A command the assistant left waiting for input (a selection, a point) would otherwise
        // sit on the command line until the user noticed.
        if turn_ended && app.session.running.is_some() {
            let _ = app.session.input(cadcraft_engine::Input::Cancel);
        }
        if turn_ended && std::mem::take(&mut app.assistant.started_lisp) && app.session.lisp_running() {
            app.session.lisp_abort("Function cancelled: the AI Assistant's turn ended");
        }
    }
    app.assistant.updates_rx = Some(rx);
}

/// Take in one update. True when a turn that ran tools just ended.
fn update(a: &mut Assistant, u: Update) -> bool {
    match u {
        Update::Connected(Ok(account)) => {
            a.busy = false;
            a.activity = None;
            a.push(Role::Assistant, crate::tf!("Connected to account {conto}.", conto = account.conto_id));
            if let Some(host) = &a.host {
                host.event(&AssistantEvent::KeyRegistered { conto_id: account.conto_id.clone() });
            }
            a.balance = account.remaining_usd;
            a.account = Some(account);
            a.push(Role::Assistant, crate::tl!("What shall I draw?"));
        }
        Update::Connected(Err(e)) => {
            a.busy = false;
            a.activity = None;
            a.push(Role::Error, crate::i18n::t(&e).to_string());
        }
        Update::KeyChanged(Ok(account)) => {
            a.settings.busy = false;
            if let Some(host) = &a.host {
                host.event(&AssistantEvent::KeyChanged { conto_id: account.conto_id.clone() });
            }
            let text = crate::tf!("The account key was changed: now connected to account {conto}.", conto = account.conto_id);
            a.settings.result = Some(Ok(text.clone()));
            a.push(Role::Notice, text);
            // The conversation's endpoint holds the old key: the next turn starts with the new one.
            a.conversation = None;
            // Another account may offer other models.
            a.settings.models = None;
            a.settings.models_requested = false;
            a.balance = account.remaining_usd;
            a.account = Some(account);
        }
        Update::KeyChanged(Err(e)) => {
            a.settings.busy = false;
            a.settings.result = Some(Err(crate::i18n::t(&e).to_string()));
        }
        Update::Models(models) => {
            a.settings.models = Some(models.map(|list| list.into_iter().filter(|m| ModelInfo::is_chat(&m.id)).collect()));
        }
        Update::Account(account) => {
            if let Some(acc) = account {
                a.balance = acc.remaining_usd.or(a.balance);
                a.account = Some(acc);
            }
        }
        Update::Balance(balance) => {
            a.balance_busy = false;
            if let Some(b) = balance {
                a.balance = Some(b);
            }
        }
        Update::Text(t) => a.push(Role::Assistant, t),
        Update::Step { name, args } => {
            let previous = a.activity.take().map(|s| s.text);
            a.activity = Some(Activity { text: describe(&name, &args, previous.as_deref()), failed: false });
        }
        Update::StepFailed => {
            if let Some(s) = &mut a.activity {
                s.failed = true;
            }
        }
        Update::Declined { name } => a.push(Role::Notice, crate::tf!("Declined: {tool}", tool = name)),
        Update::Confirm { name, args, reply } => {
            // A new question replaces an unanswered one, which counts as declined.
            if let Some((_, old)) = a.confirm.replace((confirm_text(&name, &args), reply)) {
                let _ = old.send(false);
            }
        }
        Update::Consent { consent, reply } => {
            if let Some((_, old)) = a.confirm.replace((consent_text(&consent), reply)) {
                let _ = old.send(false);
            }
        }
        Update::Done { conversation, error, balance, stopped, ran_tools } => {
            a.busy = false;
            a.activity = None;
            a.conversation = conversation;
            // The language and the model may have changed while the turn ran.
            let language = a.language;
            a.set_language(language);
            if let (Some(m), Some(c)) = (&a.model, &mut a.conversation) {
                c.agent.set_model(m.clone());
            }
            if let Some(b) = balance {
                a.balance = Some(b);
            }
            if stopped {
                a.push(Role::Notice, crate::tl!("The assistant ran out of steps for this turn and stopped; the drawing keeps what it did."));
            }
            if let Some(e) = error {
                a.push(Role::Error, crate::i18n::t(&e).to_string());
            }
            if let Some((_, reply)) = a.confirm.take() {
                let _ = reply.send(false);
            }
            return ran_tools;
        }
    }
    false
}

fn bubble(ui: &mut egui::Ui, m: &Message, t: Tokens, account_url: &str) {
    let width = ui.available_width();
    match m.role {
        Role::User | Role::Assistant | Role::Error => {
            let user = m.role == Role::User;
            let (fill, color) = match m.role {
                Role::User => (t.select_bg, t.text),
                Role::Error => (t.field, t.warn),
                _ => (t.control, t.text),
            };
            let layout = if user { egui::Layout::right_to_left(egui::Align::Min) } else { egui::Layout::left_to_right(egui::Align::Min) };
            ui.with_layout(layout, |ui| {
                egui::Frame::NONE.fill(fill).corner_radius(6.0).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                    ui.set_max_width(width * 0.85);
                    ui.add(egui::Label::new(RichText::new(&m.text).color(color)).wrap());
                });
            });
        }
        Role::Notice => {
            ui.add(egui::Label::new(RichText::new(&m.text).font(crate::theme::small()).color(t.text_dim)).wrap());
        }
        Role::Link => {
            let r = ui.hyperlink_to(&m.text, account_url).on_hover_text(account_url);
            crate::control::record_widget(ui, &r, "Account site", "link", None);
        }
    }
}

pub fn show(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    app.assistant.greet();
    let Some(host) = app.assistant.host.clone() else {
        ui.add_space(12.0);
        ui.label(RichText::new(crate::tl!("The AI Assistant is not available in this version yet.")).color(t.text_dim));
        return;
    };
    let has_key = host.has_key();
    let service = host.service();
    request_account(&mut app.assistant, ui.ctx());
    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
    // Header: the credit (a click reads it again), a fresh start and the settings — icons, so
    // the credit keeps its room in a narrow panel.
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            let open = app.assistant.settings.open;
            if crate::icons::button(ui, crate::icons::Icon::Gear, 22.0, crate::tl!("AI Assistant settings"), open).clicked() {
                app.assistant.settings.open = !open;
                app.assistant.settings.result = None;
            }
            let can_reset = has_key && !app.assistant.busy && !app.assistant.messages.is_empty();
            let clear = ui
                .add_enabled_ui(can_reset, |ui| crate::icons::button(ui, crate::icons::Icon::Broom, 22.0, crate::tl!("New conversation"), false))
                .inner;
            crate::control::record_widget(ui, &clear, "New conversation", "button", None);
            if clear.clicked() {
                app.assistant.clear_asked = true;
            }
            if let Some(b) = app.assistant.balance {
                ui.add_space(4.0);
                let mut refresh = false;
                if app.assistant.balance_busy {
                    ui.spinner();
                } else {
                    let r = crate::icons::button(ui, crate::icons::Icon::Refresh, 18.0, crate::tl!("Refresh credit"), false);
                    crate::control::record_widget(ui, &r, "Refresh credit", "button", None);
                    refresh = r.clicked();
                }
                // Top up: the account site, in the browser.
                let card = crate::icons::button(ui, crate::icons::Icon::CreditCard, 18.0, crate::tl!("Top up credit"), false);
                crate::control::record_widget(ui, &card, "Top up credit", "button", None);
                if card.clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(&service.url));
                }
                // The credit takes what is left and is cut short, never run over.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let text = crate::tf!("Credit available: {usd} USD", usd = format!("{b:.2}"));
                    let label =
                        egui::Label::new(RichText::new(text).font(crate::theme::small()).color(t.text_dim)).truncate().sense(egui::Sense::click());
                    let r = ui.add(label).on_hover_text(crate::tl!("Refresh credit"));
                    crate::control::record_widget(ui, &r, "Credit available", "button", None);
                    refresh |= r.clicked();
                });
                if refresh {
                    refresh_balance(&mut app.assistant, ui.ctx());
                }
            }
        });
    });
    ui.separator();
    // The confirmation card's height, measured: a long question (the agreement, or a translated
    // one) wraps to more lines than a short one, and its buttons must stay in view.
    let card = app.assistant.confirm.as_ref().map_or(0.0, |(question, _)| {
        let wrap = (ui.available_width() - 18.0).max(40.0);
        let font = egui::TextStyle::Body.resolve(ui.style());
        let text = ui.fonts_mut(|f| f.layout(question.clone(), font, t.text, wrap)).size().y;
        // The text, the row of buttons, the gap between them, the card's margins and stroke, and
        // the space below it.
        text + ui.spacing().interact_size.y + ui.spacing().item_spacing.y + 18.0 + 18.0
    });
    let footer = if has_key { 76.0 } else { 64.0 } + card;
    // While a question waits, the messages may shrink to nothing in a short palette: the answer
    // is what the user has to see.
    let list_height = (ui.available_height() - footer).max(if card > 0.0 { 0.0 } else { 80.0 });
    egui::ScrollArea::vertical().id_salt("assistant-messages").max_height(list_height).auto_shrink([false, false]).stick_to_bottom(true).show(
        ui,
        |ui| {
            ui.add_space(4.0);
            // A margin frame, not a horizontal row: a right-aligned bubble inside a row takes the
            // row's unbounded width and runs past the panel's edge.
            egui::Frame::NONE.inner_margin(egui::Margin::symmetric(6, 0)).show(ui, |ui| {
                for m in &app.assistant.messages {
                    bubble(ui, m, t, &service.url);
                }
            });
            // One status line: the spinner and the current action, replaced by the next one and
            // gone when the turn ends.
            if app.assistant.busy {
                let (text, failed) = match &app.assistant.activity {
                    Some(s) => (s.text.clone(), s.failed),
                    None => (crate::tl!("Working…").to_string(), false),
                };
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.spinner();
                    ui.add(egui::Label::new(RichText::new(text).color(if failed { t.warn } else { t.text_dim })).wrap());
                });
            }
        },
    );
    ui.separator();
    // A tool that replaces the drawing or writes a file waits for an answer.
    let mut answer = None;
    if let Some((question, _)) = &app.assistant.confirm {
        egui::Frame::NONE.fill(t.field).stroke(egui::Stroke::new(1.0, t.accent)).corner_radius(6.0).inner_margin(8).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(question).color(t.text)).wrap());
            ui.horizontal(|ui| {
                if ui.button(crate::tl!("Allow")).clicked() {
                    answer = Some(true);
                }
                if ui.button(crate::tl!("Decline")).clicked() {
                    answer = Some(false);
                }
            });
        });
    }
    if let Some(yes) = answer
        && let Some((_, reply)) = app.assistant.confirm.take()
    {
        let _ = reply.send(yes);
    }
    let ctx = ui.ctx().clone();
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        let busy = app.assistant.busy;
        if has_key {
            let w = ui.available_width() - 70.0;
            let field = egui::TextEdit::singleline(&mut app.assistant.input).hint_text(crate::tl!("Ask the assistant…")).desired_width(w);
            let r = ui.add_enabled(!busy, field);
            crate::control::record_widget(ui, &r, "Ask the assistant", "textbox", None);
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let b = ui.add_enabled(!busy && !app.assistant.input.trim().is_empty(), egui::Button::new(crate::tl!("Send")));
            crate::control::record_widget(ui, &b, "Send", "button", None);
            if enter || b.clicked() {
                send(app, &ctx);
                r.request_focus();
            }
        } else {
            let w = ui.available_width() - 90.0;
            let field = egui::TextEdit::singleline(&mut app.assistant.key_input).password(true).hint_text(crate::tl!("Account key")).desired_width(w);
            let r = ui.add_enabled(!busy, field);
            crate::control::record_widget(ui, &r, "Account key", "textbox", None);
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let b = ui.add_enabled(!busy && !app.assistant.key_input.trim().is_empty(), egui::Button::new(crate::tl!("Connect")));
            crate::control::record_widget(ui, &b, "Connect", "button", None);
            if enter || b.clicked() {
                connect(app);
            }
        }
    });
    clear_question(app, ui.ctx());
    settings_window(app, ui.ctx(), has_key);
}

/// A fresh start asks first: the messages go away for good.
fn clear_question(app: &mut CadApp, ctx: &egui::Context) {
    if !app.assistant.clear_asked {
        return;
    }
    let t = Tokens::get();
    let mut answer = None;
    let modal = egui::Modal::new(egui::Id::unique("assistant-clear")).show(ctx, |ui| {
        ui.set_width(320.0);
        ui.label(RichText::new(crate::tl!("New conversation")).strong());
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new(crate::tl!("Start a new conversation? The messages so far will be cleared.")).color(t.text)).wrap());
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clear = ui.button(crate::tl!("Clear"));
            crate::control::record_widget(ui, &clear, "Clear", "button", None);
            let keep = ui.button(crate::tl!("Keep"));
            crate::control::record_widget(ui, &keep, "Keep", "button", None);
            if clear.clicked() {
                answer = Some(true);
            } else if keep.clicked() {
                answer = Some(false);
            }
        });
    });
    // Escape or a click outside keeps the conversation.
    if answer.is_none() && modal.should_close() {
        answer = Some(false);
    }
    match answer {
        Some(true) => new_conversation(&mut app.assistant),
        Some(false) => app.assistant.clear_asked = false,
        None => {}
    }
}

/// Assistant settings: the reply language and the account key.
fn settings_window(app: &mut CadApp, ctx: &egui::Context, has_key: bool) {
    if !app.assistant.settings.open {
        return;
    }
    let t = Tokens::get();
    let mut open = true;
    let mut change = false;
    egui::Window::new(crate::tl!("AI Assistant settings"))
        .id(egui::Id::unique("assistant-settings"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(360.0)
        .show(ctx, |ui| {
            let a = &mut app.assistant;
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            ui.label(RichText::new(crate::tl!("Assistant language")).strong());
            let mut language = a.language;
            for l in Language::ALL {
                let r = ui.radio_value(&mut language, l, l.name());
                crate::control::record_widget(ui, &r, l.name(), "radio", Some(language == l));
            }
            if language != a.language {
                a.set_language(language);
            }
            if has_key {
                ui.separator();
                ui.label(RichText::new(crate::tl!("Model")).strong());
                let suggestion = match a.account.as_ref().map(|acc| acc.model.as_str()) {
                    Some(m) => crate::tf!("Suggested by the account ({model})", model = m),
                    None => crate::tl!("Suggested by the account").to_string(),
                };
                request_models(a, ctx);
                let current = match (&a.model, &a.settings.models) {
                    (None, _) => suggestion.clone(),
                    (Some(id), Some(Ok(list))) => list.iter().find(|m| &m.id == id).map_or_else(|| id.clone(), ModelInfo::label),
                    (Some(id), _) => id.clone(),
                };
                ui.label(RichText::new(current).color(t.accent));
                // A list, not a drop-down: there can be hundreds of models, and a search field
                // inside a drop-down closes it at the first click.
                let filter =
                    ui.add(egui::TextEdit::singleline(&mut a.settings.model_filter).hint_text(crate::tl!("Search models…")).desired_width(330.0));
                crate::control::record_widget(ui, &filter, "Search models", "textbox", None);
                let mut choice = None;
                egui::Frame::NONE.fill(t.field).corner_radius(4.0).inner_margin(4).show(ui, |ui| {
                    egui::ScrollArea::vertical().id_salt("assistant-models").max_height(200.0).auto_shrink([false, true]).show(ui, |ui| {
                        ui.set_min_width(322.0);
                        if ui.selectable_label(a.model.is_none(), &suggestion).clicked() {
                            choice = Some(None);
                        }
                        match &a.settings.models {
                            None => {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(RichText::new(crate::tl!("Loading models…")).color(t.text_dim));
                                });
                            }
                            Some(Err(e)) => {
                                ui.label(RichText::new(crate::tl!("The models could not be loaded")).color(t.warn)).on_hover_text(e);
                            }
                            Some(Ok(list)) => {
                                let needle = a.settings.model_filter.trim().to_lowercase();
                                for m in list.iter().filter(|m| {
                                    needle.is_empty()
                                        || m.id.to_lowercase().contains(&needle)
                                        || m.name.as_deref().is_some_and(|n| n.to_lowercase().contains(&needle))
                                }) {
                                    let r = ui.selectable_label(a.model.as_deref() == Some(m.id.as_str()), m.label()).on_hover_text(&m.id);
                                    crate::control::record_widget(ui, &r, &m.id, "option", Some(a.model.as_deref() == Some(m.id.as_str())));
                                    if r.clicked() {
                                        choice = Some(Some(m.id.clone()));
                                    }
                                }
                            }
                        }
                    });
                });
                if let Some(c) = choice {
                    a.set_model(c);
                }
            }
            ui.separator();
            ui.label(RichText::new(crate::tl!("Account")).strong());
            match &a.account {
                Some(acc) => {
                    ui.label(crate::tf!("Connected account: {conto}", conto = acc.conto_id));
                    if let Some(b) = a.balance {
                        ui.label(RichText::new(crate::tf!("Credit available: {usd} USD", usd = format!("{b:.2}"))).color(t.text_dim));
                    }
                }
                None if has_key => {
                    ui.label(RichText::new(crate::tl!("Reading the account…")).color(t.text_dim));
                }
                None => {
                    ui.label(RichText::new(crate::tl!("No account key is set")).color(t.text_dim));
                }
            }
            if has_key {
                ui.add_space(4.0);
                ui.label(crate::tl!("Change account key"));
                ui.horizontal(|ui| {
                    let busy = a.settings.busy || a.busy;
                    let field = egui::TextEdit::singleline(&mut a.settings.new_key)
                        .password(true)
                        .hint_text(crate::tl!("New account key"))
                        .desired_width(220.0);
                    let r = ui.add_enabled(!busy, field);
                    crate::control::record_widget(ui, &r, "New account key", "textbox", None);
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let b = ui.add_enabled(!busy && !a.settings.new_key.trim().is_empty(), egui::Button::new(crate::tl!("Change key")));
                    crate::control::record_widget(ui, &b, "Change key", "button", None);
                    change = enter || b.clicked();
                    if a.settings.busy {
                        ui.spinner();
                    }
                });
                match &a.settings.result {
                    Some(Ok(m)) => {
                        ui.label(RichText::new(m).color(t.text_dim));
                    }
                    Some(Err(e)) => {
                        ui.label(RichText::new(e).color(t.warn));
                    }
                    None => {}
                }
            }
        });
    if change {
        change_key(app);
    }
    if !open {
        app.assistant.settings.open = false;
        app.assistant.settings.new_key.clear();
    }
}

#[cfg(test)]
mod tests;
