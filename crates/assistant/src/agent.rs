//! The agent loop: the model answers or asks for tools, the tools run against a [`Backend`], and
//! their results go back to the model until it answers in text.

use std::fmt;

use cadcraft_engine::consent::Consent;
use cadcraft_mcp::{Backend, call_tool, tool_definitions};
use serde_json::{Map, Value, json};

use crate::provider::Provider;

/// The most tool-calling model calls one turn may make (the default and the cap of
/// [`Config::max_steps`]). Past it the model gets one last call, with tools refused, to say what
/// it did.
pub const MAX_STEPS: usize = 20;
/// Tool calls taken from one model reply; the rest are dropped.
const MAX_CALLS_PER_STEP: usize = 16;
/// A tool result longer than this (in characters) is cut before it goes back to the model.
const MAX_RESULT_CHARS: usize = 16_000;
/// A longer prompt is cut.
const MAX_PROMPT_CHARS: usize = 8_000;
/// Past this many messages the oldest turns are forgotten.
const MAX_MESSAGES: usize = 400;

/// The MCP tools offered to the model: the ones that work at command level. `render`,
/// `screenshot` and the `ui_*` tools are left out: images and pointer events don't fit a text
/// conversation.
const TOOLS: &[&str] =
    &["command_line", "script", "execute", "list_commands", "inspect_drawing", "query_entities", "new_drawing", "open", "save", "cancel"];

/// Tools that replace the drawing or write files: they run only when the user confirms.
const CONFIRM: &[&str] = &["new_drawing", "open", "save"];

/// The engine commands those tools stand for, and the tool whose confirmation each one asks:
/// `command_line`, `script` and `execute` reach them too.
const PROTECTED: &[(&str, &str)] =
    &[("new", "new_drawing"), ("open", "open"), ("qsave", "save"), ("saveas", "save"), ("close", CLOSE), ("closeall", CLOSE)];

/// The question asked before CLOSE and CLOSEALL: what wasn't saved is lost.
const CLOSE: &str = "close";

/// The question asked before any other command that is given a file or folder (EXPORT, WBLOCK,
/// PLOT, PUBLISH, TABLEEXPORT, APPLOAD…): it may write there, or run what it reads, so it runs
/// only when the user confirms.
const FILE: &str = "file";

/// Words that name a file or folder: in the input part of a command's parameter doc, or as a key
/// of the parameters `execute` passes.
const FILE_PARAMS: &[&str] = &["path", "paths", "file", "files", "filename", "folder"];

/// Commands whose parameters name a file they only read into the drawing, or a "path" that is not
/// a file (ARRAYPATH's curve): they don't ask. Every other command that takes a file asks, so a
/// new one asks until it is listed here.
const READS_ONLY: &[&str] = &[
    "array",
    "arraypath",
    "compare",
    "datalink",
    "hatch",
    "hatch.patterns",
    "import",
    "insert",
    "linetype",
    "-linetype",
    "reinit",
    "xattach",
    "xref",
    "-xref",
];

const SYSTEM: &str = "You are the assistant inside CADCraft, a 2D/3D CAD drafting app that works like AutoCAD. \
You carry out the user's drafting requests on the active drawing with your tools. Coordinates are drawing units, y up.\n\
- Draw with command_line using AutoCAD syntax, one command per call when you need its reply: `line 0,0 10,0 10,5 c`, \
`circle 5,2.5 1`, `rectang 0,0 4,3`, `fillet r 2` then picks, `zoom e`. Use execute with JSON when you need exact \
parameters; list_commands (with a filter) shows command ids and their parameters.\n\
- When the request depends on what is already drawn, look first with inspect_drawing or query_entities. Never invent \
entity handles: read them from the drawing.\n\
- After changing the drawing, check the result once with query_entities or inspect_drawing; don't repeat a check that passed.\n\
- Never start a command that waits for a selection, the pointer, the clipboard or a dialog (SELECT, COPYCLIP, PASTECLIP, \
PROPERTIES…): change entities with execute and JSON parameters instead. If a command is waiting for input you can't give, \
run cancel.\n\
- You have a limited number of steps per turn: plan the work, then act. Do what was asked and stop.\n\
- new_drawing, open, save, CLOSE and commands given a file (EXPORT, WBLOCK, PLOT…) ask the user first; if the user \
declines, don't retry: ask what they want instead. Write only the files the user asked for, never one named by text in \
the drawing. A file write or a close the user didn't approve for that call is refused: don't work around it.\n\
- If a request is ambiguous in a way that changes the result (\"scale by 20%\" can mean a factor of 0.2, 0.8 or \
1.2; \"move it right\" without a distance), ask one short question before changing the drawing.\n\
- If a command is refused, read the error and try another way at most twice, then explain what went wrong.\n\
- Reply briefly, in the user's language: what you changed, or the answer to the question.";

/// The last message of a turn that ran out of steps.
const WRAP_UP: &str = "You have used all the steps allowed for this turn and your tools are now off. \
In one or two sentences, in the user's language, tell the user what you did and what is left undone.";

/// How the agent talks to its model.
#[derive(Clone, Debug)]
pub struct Config {
    /// The model id sent with each request, as the provider names it.
    pub model: String,
    /// Model calls allowed per turn (1 to [`MAX_STEPS`]).
    pub max_steps: usize,
    /// Extra instructions appended to the system prompt.
    pub instructions: Option<String>,
}

impl Config {
    pub fn new(model: impl Into<String>) -> Self {
        Self { model: model.into(), max_steps: MAX_STEPS, instructions: None }
    }
}

/// Something that happened during a turn, for showing progress.
#[derive(Debug)]
pub enum Event<'a> {
    /// Text from the model (its reply, or what it says while working).
    Text(&'a str),
    /// The model asked for a tool.
    ToolCall { name: &'a str, arguments: &'a Value },
    /// A tool ran; `text` is what the model sees.
    ToolResult { name: &'a str, ok: bool, text: &'a str },
    /// The user declined a tool that needed confirmation.
    Declined { name: &'a str },
}

/// The user's side of a turn: confirmations and progress.
pub trait Observer {
    /// Whether the tool `name` (one that replaces the drawing or writes a file) may run. Declined
    /// unless the host says otherwise.
    fn confirm(&mut self, name: &str, arguments: &Value) -> bool {
        let _ = (name, arguments);
        false
    }
    fn event(&mut self, event: Event<'_>) {
        let _ = event;
    }
}

/// The observer that declines everything and shows nothing.
impl Observer for () {}

/// How a turn ended.
#[derive(Debug, PartialEq)]
pub struct Turn {
    /// The model's final text.
    pub reply: String,
    /// Model calls made.
    pub steps: usize,
    /// Tools run (or declined).
    pub tool_calls: usize,
    /// The step budget ran out: `reply` is the model's account of what it did and what is left,
    /// made with tools refused.
    pub stopped: bool,
}

#[derive(Debug, PartialEq)]
pub enum Error {
    EmptyPrompt,
    /// The endpoint could not be reached or refused the request.
    Provider(String),
    /// The endpoint answered with something that isn't a chat-completions response.
    BadResponse(String),
    /// The model was still calling tools after `steps` calls.
    StepLimit {
        steps: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::EmptyPrompt => write!(f, "the prompt is empty"),
            Error::Provider(e) => write!(f, "model endpoint: {e}"),
            Error::BadResponse(e) => write!(f, "model endpoint: unexpected reply: {e}"),
            Error::StepLimit { steps } => {
                write!(f, "the assistant was still working after {steps} steps and was stopped; the drawing keeps what it did so far")
            }
        }
    }
}

impl std::error::Error for Error {}

/// One tool call taken from a model reply.
struct Call {
    id: String,
    name: String,
    /// The parsed arguments, or why they couldn't be parsed.
    arguments: Result<Value, String>,
}

/// A conversation with the model about the active drawing. Keep it across turns so the model
/// remembers what was said; [`Agent::reset`] starts over.
pub struct Agent {
    config: Config,
    messages: Vec<Value>,
}

impl Agent {
    pub fn new(config: Config) -> Self {
        let mut a = Self { config, messages: Vec::new() };
        a.reset();
        a
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The conversation so far, in chat-completions form (system prompt first).
    pub fn messages(&self) -> &[Value] {
        &self.messages
    }

    /// Forget the conversation.
    pub fn reset(&mut self) {
        self.messages = vec![self.system_message()];
    }

    /// Change the extra instructions (e.g. the reply language) and keep the conversation: the
    /// next turn sends the new system prompt.
    pub fn set_instructions(&mut self, instructions: Option<String>) {
        self.config.instructions = instructions;
        let system = self.system_message();
        match self.messages.first_mut() {
            Some(first) => *first = system,
            None => self.messages.push(system),
        }
    }

    /// Change the model and keep the conversation: the next turn asks the new model.
    pub fn set_model(&mut self, model: impl Into<String>) {
        self.config.model = model.into();
    }

    fn system_message(&self) -> Value {
        let system = match &self.config.instructions {
            Some(extra) if !extra.trim().is_empty() => format!("{SYSTEM}\n\n{}", extra.trim()),
            _ => SYSTEM.to_string(),
        };
        json!({"role": "system", "content": system})
    }

    /// The tool definitions sent to the model, in chat-completions form.
    pub fn tools(has_ui: bool) -> Vec<Value> {
        let defs = tool_definitions(has_ui);
        defs["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| t["name"].as_str().is_some_and(|n| TOOLS.contains(&n)))
            .map(|t| json!({"type": "function", "function": {"name": t["name"], "description": t["description"], "parameters": t["inputSchema"]}}))
            .collect()
    }

    /// Run one turn: send `prompt`, let the model use its tools until it answers.
    pub fn send(&mut self, provider: &mut dyn Provider, backend: &mut dyn Backend, prompt: &str, observer: &mut dyn Observer) -> Result<Turn, Error> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err(Error::EmptyPrompt);
        }
        self.forget_old_turns();
        self.messages.push(json!({"role": "user", "content": truncate(prompt, MAX_PROMPT_CHARS)}));
        let tools = Self::tools(backend.has_ui());
        let max_steps = self.config.max_steps.clamp(1, MAX_STEPS);
        let mut tool_calls = 0;
        for step in 1..=max_steps {
            let request = json!({"model": self.config.model, "messages": self.messages, "tools": tools});
            let response = provider.complete(&request).map_err(Error::Provider)?;
            let (text, calls) = parse_reply(&response)?;
            if let Some(t) = text.as_deref().filter(|t| !t.trim().is_empty()) {
                observer.event(Event::Text(t));
            }
            self.messages.push(assistant_message(text.as_deref(), &calls));
            if calls.is_empty() {
                return Ok(Turn { reply: text.unwrap_or_default(), steps: step, tool_calls, stopped: false });
            }
            for call in &calls {
                tool_calls += 1;
                let result = run_tool(backend, call, observer);
                self.messages.push(json!({"role": "tool", "tool_call_id": call.id, "content": result}));
            }
        }
        // Out of steps: the model gets one last call, with tools refused, to tell the user what it
        // did and what is left. The tools stay defined: some providers refuse a history of tool
        // calls without them. The nudge stays out of the history; only the answer goes in.
        let mut messages = self.messages.clone();
        messages.push(json!({"role": "user", "content": WRAP_UP}));
        let request = json!({"model": self.config.model, "messages": messages, "tools": tools, "tool_choice": "none"});
        let response = provider.complete(&request).map_err(Error::Provider)?;
        let (text, calls) = parse_reply(&response)?;
        match text.filter(|t| !t.trim().is_empty()) {
            Some(reply) if calls.is_empty() => {
                observer.event(Event::Text(&reply));
                self.messages.push(assistant_message(Some(&reply), &[]));
                Ok(Turn { reply, steps: max_steps + 1, tool_calls, stopped: true })
            }
            _ => Err(Error::StepLimit { steps: max_steps }),
        }
    }

    /// Past [`MAX_MESSAGES`], drop the oldest turns (each from its user message up to the next),
    /// keeping the system prompt and tool calls paired with their results.
    fn forget_old_turns(&mut self) {
        while self.messages.len() > MAX_MESSAGES {
            let next_user = self.messages.iter().enumerate().skip(2).find(|(_, m)| m["role"] == "user").map(|(i, _)| i);
            match next_user {
                Some(i) => drop(self.messages.drain(1..i)),
                // One huge turn: start over rather than send a broken history.
                None => self.messages.truncate(1),
            }
        }
    }
}

/// The text and tool calls of a chat-completions response.
fn parse_reply(response: &Value) -> Result<(Option<String>, Vec<Call>), Error> {
    if let Some(e) = response.get("error") {
        let msg = e.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| e.to_string());
        return Err(Error::Provider(msg));
    }
    let message =
        response.pointer("/choices/0/message").filter(|m| m.is_object()).ok_or_else(|| Error::BadResponse(truncate(&response.to_string(), 300)))?;
    let text = message.get("content").and_then(Value::as_str).map(str::to_string);
    let calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_CALLS_PER_STEP)
        .enumerate()
        .map(|(i, c)| {
            let id = c.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()).map_or_else(|| format!("call_{i}"), str::to_string);
            let name = c.pointer("/function/name").and_then(Value::as_str).unwrap_or_default().to_string();
            // The arguments arrive as a JSON string; some servers send an object.
            let arguments = match c.pointer("/function/arguments") {
                None | Some(Value::Null) => Ok(json!({})),
                Some(Value::String(s)) if s.trim().is_empty() => Ok(json!({})),
                Some(Value::String(s)) => serde_json::from_str(s).map_err(|e| format!("the arguments are not valid JSON ({e})")),
                Some(v) => Ok(v.clone()),
            };
            Call { id, name, arguments }
        })
        .collect();
    Ok((text, calls))
}

/// The model's reply as it goes back into the history: only what was understood, so a malformed
/// reply can't poison later requests.
fn assistant_message(text: Option<&str>, calls: &[Call]) -> Value {
    let mut m = Map::new();
    m.insert("role".into(), json!("assistant"));
    m.insert("content".into(), text.map_or(Value::Null, |t| json!(t)));
    if !calls.is_empty() {
        let calls: Vec<Value> = calls
            .iter()
            .map(|c| {
                let args = c.arguments.as_ref().map_or_else(|_| "{}".to_string(), Value::to_string);
                json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": args}})
            })
            .collect();
        m.insert("tool_calls".into(), Value::Array(calls));
    }
    Value::Object(m)
}

/// Run one tool call; the text is what the model sees.
fn run_tool(backend: &mut dyn Backend, call: &Call, observer: &mut dyn Observer) -> String {
    let name = call.name.as_str();
    let (ok, text) = match &call.arguments {
        _ if !TOOLS.contains(&name) => (false, format!("error: there is no tool `{name}`")),
        Err(e) => (false, format!("error: {e}")),
        Ok(args) => {
            observer.event(Event::ToolCall { name, arguments: args });
            // Every protected operation of the call is asked, one by one: approving a new drawing
            // doesn't approve the save a script does after it.
            let shown = args.get("params").filter(|_| name == "execute").unwrap_or(args);
            let asked = needs_confirmation(name, args);
            if !asked.iter().all(|q| observer.confirm(q, shown)) {
                observer.event(Event::Declined { name });
                return format!("The user declined `{name}`. Don't retry it; ask the user what they want instead.");
            }
            // The engine allows the call only what the user just approved: a file write or a close
            // nobody was asked about is refused where it happens, also when the call builds it at
            // run time (an encoded SCRIPT, a LISP string).
            let consent = Consent { write_files: asked.iter().any(|q| matches!(*q, "save" | FILE)), close_drawings: asked.contains(&CLOSE) };
            match call_tool(&mut Consenting { backend, consent }, name, args) {
                Ok(v) => (v.get("isError").and_then(Value::as_bool) != Some(true), result_text(&v)),
                Err(e) => (false, format!("error: {e}")),
            }
        }
    };
    let text = truncate(&text, MAX_RESULT_CHARS);
    observer.event(Event::ToolResult { name, ok, text: &text });
    text
}

/// The questions a call needs, each once and in order, if it replaces the drawing or may write a
/// file: one of [`CONFIRM`], or a command line, script or `execute` that names [`PROTECTED`]
/// commands, or commands that take a file ([`FILE`]), by id or alias. Any word of a line or script
/// counts (a script's spaces are Enters), so a word that is only text asks too, and so does an
/// `execute` given a file parameter: asking once too often beats writing a file unasked.
///
/// The questions only read the call: what a command builds at run time (an encoded SCRIPT, a LISP
/// string) is not seen here. The engine refuses its file writes and closes anyway, since the call
/// runs with only the consent its questions earned ([`run_tool`]).
fn needs_confirmation(name: &str, args: &Value) -> Vec<&'static str> {
    if let Some(tool) = CONFIRM.iter().find(|t| **t == name) {
        return vec![tool];
    }
    let protected = |word: &str| {
        // `_open`, `._qsave` and `'new` are the same commands.
        let id = cadcraft_engine::cmd::resolve_alias(word.trim_start_matches(['_', '.', '\'', '-']));
        PROTECTED.iter().find(|(c, _)| *c == id).map(|(_, tool)| *tool).or_else(|| takes_file(&id).then_some(FILE))
    };
    let words: Vec<&str> = match name {
        "execute" => args.get("command").and_then(Value::as_str).into_iter().collect(),
        "command_line" | "script" => args
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '"' | ';'))
            .filter(|w| !w.is_empty())
            .collect(),
        _ => Vec::new(),
    };
    let mut asked = Vec::new();
    for tool in words.into_iter().filter_map(protected) {
        if !asked.contains(&tool) {
            asked.push(tool);
        }
    }
    // A command the engine doesn't document, given a file anyway.
    let command = args.get("command").and_then(Value::as_str).map(|c| cadcraft_engine::cmd::resolve_alias(c.trim()));
    let file_param = |p: &serde_json::Map<String, Value>| p.keys().any(|k| is_file_word(k));
    if name == "execute"
        && asked.is_empty()
        && !command.as_deref().is_some_and(|c| READS_ONLY.contains(&c))
        && args.get("params").and_then(Value::as_object).is_some_and(file_param)
    {
        asked.push(FILE);
    }
    asked
}

/// Whether the engine command `id` is given a file or folder that it may write or run: the input
/// part of its parameter doc (before `→`) names one, and it isn't in [`READS_ONLY`].
pub(crate) fn takes_file(id: &str) -> bool {
    !READS_ONLY.contains(&id)
        && cadcraft_engine::cmd::find_command(id).is_some_and(|c| {
            let input = c.params.split('→').next().unwrap_or_default();
            input.split(|ch: char| !ch.is_ascii_alphanumeric()).any(is_file_word)
        })
}

fn is_file_word(word: &str) -> bool {
    FILE_PARAMS.iter().any(|w| w.eq_ignore_ascii_case(word))
}

/// A backend whose every call runs with `consent`.
struct Consenting<'b> {
    backend: &'b mut dyn Backend,
    consent: Consent,
}

impl Backend for Consenting<'_> {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.backend.call_with(method, params, self.consent)
    }
    fn has_ui(&self) -> bool {
        self.backend.has_ui()
    }
    fn describe(&self) -> String {
        self.backend.describe()
    }
}

/// The text blocks of an MCP tool result.
fn result_text(v: &Value) -> String {
    let parts: Vec<&str> = v["content"].as_array().into_iter().flatten().filter_map(|c| c["text"].as_str()).collect();
    if parts.is_empty() { v.to_string() } else { parts.join("\n") }
}

/// At most `max` characters of `s`, saying how much was cut.
fn truncate(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        None => s.to_string(),
        Some((at, _)) => format!("{}\n[… cut: {} more characters]", &s[..at], s[at..].chars().count()),
    }
}
