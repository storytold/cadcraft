//! AutoLISP: load `.lsp` files, evaluate expressions typed at the command line, and run
//! commands defined with `(defun c:NAME …)`.
//!
//! The language runs on an explicit-stack machine ([`eval`]) that can stop for input
//! (`getpoint`, `pause`) and carry on later, so the interface keeps drawing while a routine
//! waits. Each drawing has its own symbols, as in AutoCAD. A run is one undo step.

pub mod builtins;
mod drawing;
pub mod eval;
pub mod input;
pub mod read;
mod startup;
pub mod value;

use std::collections::HashMap;
use std::sync::Arc;

use cadcraft_doc::{Drawing, Handle};

use crate::{Input, Prompt, Session};
use eval::{Ctx, Machine, Need, Outcome};
use value::{Sym, Value};

/// A LISP error: the message `*error*` receives (`bad argument type: numberp: nil`…).
#[derive(Clone, Debug)]
pub struct LispError {
    pub msg: String,
    /// `exit`/`quit`, or a cancelled input: not caught by `vl-catch-all-apply`.
    pub quit: bool,
}

impl LispError {
    pub fn new(msg: impl Into<String>) -> LispError {
        LispError { msg: msg.into(), quit: false }
    }
    pub fn bad_arg(pred: &str, v: &Value) -> LispError {
        LispError::new(format!("bad argument type: {pred}: {}", value::to_text(v, true)))
    }
    pub fn quit(msg: impl Into<String>) -> LispError {
        LispError { msg: msg.into(), quit: true }
    }
}

/// One drawing's LISP environment.
pub struct Env {
    pub globals: HashMap<Sym, Value>,
    /// Printed text not yet ended by a newline.
    pub out: String,
    /// `initget` settings for the next `get…` call.
    pub initget: Option<input::Initget>,
    pub next_pickset: u32,
    pub files: HashMap<u32, builtins::LispFile>,
    pub next_file: u32,
    /// Entities `entdel` deleted (it brings them back when called again), with their space.
    pub deleted: HashMap<Handle, (cadcraft_doc::Space, Arc<cadcraft_doc::Entity>)>,
    /// Every entity's handle in creation order, for the drawing it was taken from (`entnext`).
    pub order: Option<(usize, Arc<Vec<Handle>>)>,
    /// `tblnext`'s place in each table.
    pub tblnext: HashMap<String, usize>,
    /// A block definition `entmake` has begun (a BLOCK list) and not yet ended (ENDBLK): the
    /// entities made meanwhile go into it, numbered when it ends.
    pub pending_block: Option<(cadcraft_doc::Block, Vec<cadcraft_doc::Entity>)>,
    /// The command history's length after LISP last printed a line: when something else wrote since
    /// (a prompt, a command's echo), a leading newline only ends that line.
    pub printed_at: Option<usize>,
}

impl Default for Env {
    fn default() -> Self {
        let mut e = Env {
            globals: HashMap::new(),
            out: String::new(),
            initget: None,
            next_pickset: 1,
            files: HashMap::new(),
            next_file: 1,
            deleted: HashMap::new(),
            order: None,
            tblnext: HashMap::new(),
            pending_block: None,
            printed_at: None,
        };
        builtins::install(&mut e);
        e
    }
}

impl Env {
    /// Set a variable (nil unbinds it).
    pub fn set(&mut self, s: &Sym, v: Value) {
        if v.is_nil() {
            self.globals.remove(s);
        } else {
            self.globals.insert(s.clone(), v);
        }
    }
    pub fn get(&self, name: &str) -> Value {
        eval::lookup(self, &Sym::new(name))
    }
    /// Text printed by `princ` and friends.
    pub fn print(&mut self, text: &str) {
        self.out.push_str(text);
    }
}

/// The session's LISP: each drawing's environment and the routine running, if any.
#[derive(Default)]
pub struct LispState {
    envs: HashMap<u64, Env>,
    run: Option<Box<Run>>,
    /// A routine runs, from its start to its end (also while its machine is out of `run`).
    active: bool,
    /// Files loaded by APPLOAD, by name, for `(load name)` where there is no file system.
    files: HashMap<String, String>,
    /// Where startup files are read from (see `startup.rs`).
    support_dir: Option<std::path::PathBuf>,
    /// Drawings that had their startup files.
    autoloaded: std::collections::HashSet<u64>,
    /// `acad.lsp` was loaded this session.
    acad_loaded: bool,
    /// Memory caps (lowered by tests).
    pub(crate) limits: Limits,
}

/// What a run may use (see [`value`]'s memory notes).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    /// Live cons cells (all sessions).
    pub cells: usize,
    /// Bytes of strings, printed text and files read per run.
    pub string_bytes: u64,
    /// Maximum interpreter steps per synchronous callback.
    pub callback_steps: u64,
}

impl Default for Limits {
    /// Provide finite cons-cell, string-byte, and synchronous-callback limits for a LISP run.
    fn default() -> Self {
        Limits { cells: value::MAX_CELLS, string_bytes: value::STRING_BUDGET, callback_steps: 500_000_000 }
    }
}

impl std::fmt::Debug for LispState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LispState {{ drawings: {}, running: {} }}", self.envs.len(), self.run.is_some())
    }
}

/// A routine on its way: the machine, what it waits for, and its undo step.
struct Run {
    machine: Machine,
    /// The drawing it runs in.
    uid: u64,
    waiting: Option<Need>,
    /// A command-line expression: its value is printed at the end.
    print: bool,
    /// Undo label: the command (`c:` function) or "LISP".
    label: String,
    /// The drawing and selection before the run (one undo step for the whole run).
    before: Option<(Arc<Drawing>, Vec<Handle>)>,
    /// Inputs given up front (`lisp {code, inputs}`): what `get…` calls answer.
    inputs: std::collections::VecDeque<Value>,
    /// Run to the end without waiting (programmatic calls): `get…` without an input gets nil.
    blocking: bool,
    /// Steps taken, for the cap on blocking runs.
    steps: u64,
    /// The `*error*` handler is the one running.
    handler: bool,
    /// Bytes of strings and printed text the run has made ([`value::charge_bytes`]).
    string_bytes: u64,
    /// Cons cells alive when the run started: cells the drawing's variables already hold don't
    /// stop a run (it can still set them to nil) unless it adds more.
    cells_at_start: usize,
}

/// Steps per slice before the interface gets a frame.
const SLICE: u64 = 400_000;
/// Steps a blocking run (a programmatic call) may take before it is stopped.
const BLOCKING_CAP: u64 = 500_000_000;

impl Session {
    /// The LISP environment of the active drawing.
    fn lisp_env_take(&mut self) -> Env {
        let uid = self.state().map(|s| s.uid).unwrap_or(0);
        self.lisp.envs.remove(&uid).unwrap_or_default()
    }

    fn lisp_env_put(&mut self, uid: u64, env: Env) {
        self.lisp.envs.insert(uid, env);
    }

    /// A routine is running (possibly waiting for input).
    pub fn lisp_running(&self) -> bool {
        self.lisp.active
    }

    /// A prompt waits for input: a command's, or a LISP routine's `get…` function. Clicks and
    /// typing answer it; with none, the interface is idle (clicks select, Delete erases).
    pub fn prompting(&self) -> bool {
        self.running.is_some() || self.lisp_prompt().is_some()
    }

    /// A routine is stopped, waiting for input or for a frame.
    pub(crate) fn lisp_waiting(&self) -> bool {
        self.lisp.run.is_some()
    }

    /// The value of a LISP variable in the active drawing (nil when unbound).
    pub fn lisp_var(&self, name: &str) -> Value {
        let uid = self.state().map(|s| s.uid).unwrap_or(0);
        self.lisp.envs.get(&uid).map(|e| e.get(name)).unwrap_or_default()
    }

    /// Evaluate LISP source (a command-line expression or a file's contents). `print`: show the
    /// value on the command line. `blocking`: run to the end, answering `get…` calls from
    /// `inputs` (nil when they run out), as programmatic calls must; otherwise the routine may
    /// wait for the user.
    pub fn lisp_eval(
        &mut self,
        src: &str,
        print: bool,
        blocking: bool,
        inputs: Vec<Value>,
    ) -> crate::Result<Option<std::result::Result<Value, String>>> {
        let forms = read::read_all(src).map_err(|e| crate::EngineError::Other(format!("; error: {}", e.msg)))?;
        self.lisp_start(Machine::forms(forms), print, blocking, inputs, "LISP".into())
    }

    /// Call a LISP function (`c:` commands typed at the command line).
    fn lisp_call(&mut self, f: Value, label: String) -> crate::Result<Option<std::result::Result<Value, String>>> {
        self.lisp_start(Machine::calling(f, Vec::new()), false, false, Vec::new(), label)
    }

    fn lisp_start(
        &mut self,
        machine: Machine,
        print: bool,
        blocking: bool,
        inputs: Vec<Value>,
        label: String,
    ) -> crate::Result<Option<std::result::Result<Value, String>>> {
        if self.lisp.active {
            return Err(crate::EngineError::Other("a LISP routine is already running".into()));
        }
        self.lisp.active = true;
        let st = self.state()?;
        let before = Some((st.doc.clone(), st.selection.clone()));
        let uid = st.uid;
        self.lisp.run = Some(Box::new(Run {
            machine,
            uid,
            waiting: None,
            print,
            label,
            before,
            inputs: inputs.into(),
            blocking,
            steps: 0,
            handler: false,
            string_bytes: 0,
            cells_at_start: value::live_cells(),
        }));
        self.lisp_drive();
        if blocking {
            // A blocking run that waits for a command's input (`pause`) can't get it: cancel.
            if self.lisp.run.is_some() {
                self.lisp_abort("Function cancelled");
            }
        }
        Ok(self.lisp_last.take())
    }

    /// Call once per frame: loads the startup files into a drawing seen for the first time, and
    /// carries on a routine that gave the interface a frame.
    pub fn lisp_tick(&mut self) {
        // Called every frame, outside any command: a panic in it ends the routine with an error
        // instead of taking down the app.
        if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.lisp_tick_unguarded())) {
            self.lisp_internal_error(&crate::guard::message(&*p));
        }
    }

    fn lisp_tick_unguarded(&mut self) {
        // A drawing seen for the first time gets its startup files.
        self.lisp_autoload();
        if self.lisp.run.as_ref().is_some_and(|r| matches!(r.waiting, Some(Need::Yield))) {
            if let Some(r) = self.lisp.run.as_mut() {
                r.waiting = None;
            }
            self.lisp_drive();
        }
    }

    /// Whether the routine needs ticks ([`Session::lisp_tick`]) to go on.
    pub fn lisp_busy(&self) -> bool {
        self.lisp.run.as_ref().is_some_and(|r| matches!(r.waiting, Some(Need::Yield)))
    }

    /// Run the machine until it finishes or waits.
    fn lisp_drive(&mut self) {
        loop {
            let Some(mut run) = self.lisp.run.take() else { return };
            if run.waiting.is_some() {
                self.lisp.run = Some(run);
                return;
            }
            // The routine belongs to its drawing; another one became active: stop it.
            if self.state().map(|s| s.uid).ok() != Some(run.uid) {
                self.lisp.run = Some(run);
                self.lisp_abort("Function cancelled");
                return;
            }
            let mut env = self.lisp_env_take();
            value::set_string_budget(run.string_bytes, self.lisp.limits.string_bytes);
            value::set_cell_limit(self.lisp.limits.cells.max(run.cells_at_start));
            let outcome = {
                let machine = &mut run.machine;
                let mut ctx = Ctx { s: self, env: &mut env, depth: 0 };
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| machine.run(&mut ctx, SLICE)))
            };
            run.string_bytes = value::string_bytes_used();
            let outcome = match outcome {
                Ok(o) => o,
                Err(p) => {
                    // A panic inside a built-in: put the drawing back as the routine found it.
                    if let Some((doc, sel)) = &run.before
                        && let Some(st) = self.docs.iter_mut().find(|d| d.uid == run.uid)
                    {
                        st.doc = doc.clone();
                        st.selection = sel.clone();
                    }
                    run.handler = true;
                    Outcome::Error(LispError::quit(format!("internal error: {}", crate::guard::message(&*p))))
                }
            };
            let uid = run.uid;
            self.lisp_flush(&mut env, false);
            self.lisp_env_put(uid, env);
            match outcome {
                Outcome::Done(v) => {
                    self.lisp.run = Some(run);
                    self.lisp_end(Ok(v));
                    return;
                }
                Outcome::Error(e) => {
                    self.lisp.run = Some(run);
                    self.lisp_error(e);
                    return;
                }
                Outcome::Wait(Need::Yield) => {
                    run.steps = run.steps.saturating_add(SLICE);
                    if run.blocking {
                        if run.steps > BLOCKING_CAP {
                            self.lisp.run = Some(run);
                            self.lisp_error(LispError::quit(format!("routine stopped after {BLOCKING_CAP} steps")));
                            return;
                        }
                        self.lisp.run = Some(run);
                        continue;
                    }
                    run.waiting = Some(Need::Yield);
                    self.lisp.run = Some(run);
                    return;
                }
                Outcome::Wait(Need::Pause) => {
                    // The command `command` left running takes the next input; with none
                    // running there is nothing to wait for.
                    if self.running.is_none() || run.blocking {
                        run.machine.resume(Value::Nil);
                        if run.blocking && self.running.is_some() {
                            self.cancel();
                        }
                        self.lisp.run = Some(run);
                        continue;
                    }
                    run.waiting = Some(Need::Pause);
                    self.lisp.run = Some(run);
                    return;
                }
                Outcome::Wait(Need::Get(req)) => {
                    if let Some(v) = run.inputs.pop_front() {
                        run.machine.resume(v);
                        self.lisp.run = Some(run);
                        continue;
                    }
                    if run.blocking {
                        run.machine.resume(Value::Nil);
                        self.lisp.run = Some(run);
                        continue;
                    }
                    run.waiting = Some(Need::Get(req));
                    self.lisp.run = Some(run);
                    return;
                }
            }
        }
    }

    /// Move printed text to the command history: complete lines, and with `all` the rest too.
    fn lisp_flush(&mut self, env: &mut Env, all: bool) {
        while let Some(i) = env.out.find('\n') {
            let line: String = env.out.drain(..=i).collect();
            let line = line.trim_end_matches(['\n', '\r']).to_string();
            // `(princ "\nText")` after a prompt or a command's echo ends that line instead of
            // leaving an empty one; after LISP's own line it does print an empty line.
            if line.is_empty() && env.printed_at != Some(self.log.len()) {
                env.printed_at = Some(self.log.len());
                continue;
            }
            self.echo(line);
            env.printed_at = Some(self.log.len());
        }
        if all && !env.out.is_empty() {
            let line = std::mem::take(&mut env.out);
            self.echo(line);
            env.printed_at = Some(self.log.len());
        }
    }

    /// The routine finished: print its value (command-line expressions), record its undo step.
    fn lisp_end(&mut self, result: std::result::Result<Value, String>) {
        let Some(run) = self.lisp.run.take() else { return };
        self.lisp.active = false;
        let mut env = self.lisp.envs.remove(&run.uid).unwrap_or_default();
        self.lisp_flush(&mut env, true);
        self.lisp.envs.insert(run.uid, env);
        if let (Ok(v), true) = (&result, run.print) {
            let text = value::to_text(v, true);
            if !builtins::is_null_symbol(v) {
                self.echo(text);
            }
        }
        if let Some((doc, sel)) = run.before
            && let Some(st) = self.docs.iter_mut().find(|d| d.uid == run.uid)
            && !Arc::ptr_eq(&doc, &st.doc)
        {
            st.push_undo(crate::Snapshot { label: run.label.clone(), doc, selection: sel });
            st.revision += 1;
        }
        self.lisp_last = Some(result);
    }

    /// A LISP error: run `*error*` when the routine defined one, else print it.
    fn lisp_error(&mut self, e: LispError) {
        let Some(mut run) = self.lisp.run.take() else { return };
        // A block definition the failed routine began is dropped: a later routine's entities
        // would otherwise go into it instead of the drawing (#1662). A definition still spans
        // runs that end normally.
        if let Some(env) = self.lisp.envs.get_mut(&run.uid) {
            env.pending_block = None;
        }
        // The command a failed routine left running is cancelled.
        if self.running.is_some() {
            self.cancel();
        }
        let handler = self.lisp.envs.get(&run.uid).map(|env| env.get("*ERROR*")).unwrap_or_default();
        if !run.handler && matches!(handler, Value::Func(_) | Value::Subr(_)) {
            run.handler = true;
            run.waiting = None;
            run.print = false;
            run.machine = Machine::calling(handler, vec![Value::str(&e.msg)]);
            self.lisp.run = Some(run);
            self.lisp_drive();
            // The routine failed whatever its handler returned.
            if let Some(Ok(_)) = &self.lisp_last {
                self.lisp_last = Some(Err(e.msg));
            }
            return;
        }
        self.lisp.run = Some(run);
        let mut env = self.lisp_env_take();
        self.lisp_flush(&mut env, true);
        let uid = self.state().map(|s| s.uid).unwrap_or(0);
        self.lisp_env_put(uid, env);
        self.echo(format!("; error: {}", e.msg));
        self.lisp_end(Err(e.msg));
    }

    /// A panic escaped the LISP machinery outside a routine's steps: end the routine, if any,
    /// with an error, so the session stays usable.
    fn lisp_internal_error(&mut self, msg: &str) {
        let msg = format!("internal error: {msg}");
        if self.lisp.run.is_some() {
            if let Some(r) = self.lisp.run.as_mut() {
                // Don't run the routine's own `*error*` after an internal failure.
                r.handler = true;
            }
            self.lisp_error(LispError::quit(msg));
        } else if self.lisp.active {
            self.lisp.active = false;
            self.echo(format!("; error: {msg}"));
            self.lisp_last = Some(Err(msg));
        } else {
            self.echo(format!("; error: {msg}"));
        }
    }

    /// Stop the routine (Esc while it waits, its drawing closed) with `msg` as its error.
    pub fn lisp_abort(&mut self, msg: &str) {
        if self.lisp.run.is_some() {
            self.lisp_error(LispError::quit(msg));
        }
    }

    /// A routine waits for the user's answer to a `get…` function: its prompt.
    pub(crate) fn lisp_prompt(&self) -> Option<Prompt> {
        match self.lisp.run.as_ref()?.waiting.as_ref()? {
            Need::Get(req) => Some(req.prompt()),
            _ => None,
        }
    }

    /// The rubber band of a waiting `get…` function: a rectangle from `getcorner`'s corner, a
    /// line from the base point of the others. `None` when no routine waits for input.
    pub(crate) fn lisp_preview(&self, cursor: cadcraft_geom::Vec2) -> Option<Vec<cadcraft_doc::EntityKind>> {
        let Need::Get(req) = self.lisp.run.as_ref()?.waiting.as_ref()? else { return None };
        let line =
            |a: cadcraft_geom::Vec2, b: cadcraft_geom::Vec2| cadcraft_doc::EntityKind::Line(cadcraft_doc::Line { a: a.to3(0.0), b: b.to3(0.0) });
        let Some(b) = req.first.or(req.base).map(|p| cadcraft_geom::Vec2::new(p[0], p[1])) else { return Some(Vec::new()) };
        Some(match req.kind {
            input::GetKind::Corner => {
                let (c1, c2) = (cadcraft_geom::Vec2::new(cursor.x, b.y), cadcraft_geom::Vec2::new(b.x, cursor.y));
                vec![line(b, c1), line(c1, cursor), line(cursor, c2), line(c2, b)]
            }
            input::GetKind::Point | input::GetKind::Dist | input::GetKind::Angle { .. } => vec![line(b, cursor)],
            _ => Vec::new(),
        })
    }

    /// Give an input to a waiting routine. `true` when the routine took it.
    pub(crate) fn lisp_input(&mut self, input: Input) -> bool {
        let Some(req) = self.lisp.run.as_ref().and_then(|r| match &r.waiting {
            Some(Need::Get(req)) => Some(req.clone()),
            _ => None,
        }) else {
            return false;
        };
        if input == Input::Cancel {
            self.echo("*Cancel*");
            self.lisp_abort("Function cancelled");
            return true;
        }
        match req.answer(self, input) {
            input::Answer::Value(v) => {
                if let Some(r) = self.lisp.run.as_mut() {
                    r.waiting = None;
                    r.machine.resume(v);
                }
                self.lisp_drive();
            }
            // Not an answer this prompt takes: say why and ask again.
            input::Answer::Again(msg) => self.echo(msg),
            // The first of two points: the prompt asks for the second.
            input::Answer::Second(p) => {
                if let Some(r) = self.lisp.run.as_mut() {
                    r.waiting = Some(Need::Get(input::with_first(&req, p)));
                }
            }
        }
        true
    }

    /// After the running command took an input: a routine paused for it carries on.
    pub(crate) fn lisp_after_command_input(&mut self) {
        if self.lisp.run.as_ref().is_some_and(|r| matches!(r.waiting, Some(Need::Pause))) {
            if let Some(r) = self.lisp.run.as_mut() {
                r.waiting = None;
                r.machine.resume(Value::Nil);
            }
            self.lisp_drive();
        }
    }

    /// Whether commands' undo steps go into a running routine's (one step per routine).
    pub(crate) fn lisp_groups_undo(&self) -> bool {
        self.lisp.active
    }

    /// The commands the active drawing's LISP defines (`c:` functions), uppercase and sorted.
    pub fn lisp_commands(&self) -> Vec<String> {
        let Ok(uid) = self.state().map(|s| s.uid) else { return Vec::new() };
        let Some(env) = self.lisp.envs.get(&uid) else { return Vec::new() };
        let mut names: Vec<String> = env
            .globals
            .iter()
            .filter(|(name, f)| name.name().starts_with("C:") && matches!(f, Value::Func(_) | Value::Subr(_)))
            .filter_map(|(name, _)| name.name().strip_prefix("C:").filter(|n| !n.is_empty()).map(str::to_string))
            .collect();
        names.sort();
        names
    }

    /// A `c:` function for a command name typed at the command line.
    pub(crate) fn lisp_command(&self, name: &str) -> Option<Value> {
        if self.lisp.active {
            return None;
        }
        let uid = self.state().ok()?.uid;
        let f = self.lisp.envs.get(&uid)?.get(&format!("C:{name}"));
        matches!(f, Value::Func(_) | Value::Subr(_)).then_some(f)
    }

    /// Run a `c:` command.
    pub(crate) fn lisp_run_command(&mut self, name: &str, f: Value) -> crate::Result<()> {
        self.lisp_call(f, name.to_ascii_uppercase()).map(|_| ())
    }
}

pub(crate) use drawing::from_json as drawing_from_json;
pub(crate) use drawing::load_source;

impl Session {
    /// A file APPLOAD loaded, by its name (with or without `.lsp`, any case).
    pub(crate) fn lisp_file(&self, name: &str) -> Option<String> {
        let key = file_key(name);
        self.lisp.files.get(&key).cloned()
    }

    /// Load a LISP file (APPLOAD, a dropped `.lsp` file): its forms run in the active drawing,
    /// and `(load name)` finds it later even where there is no file system. `blocking`: run to
    /// the end (programmatic calls).
    pub fn lisp_load(&mut self, name: &str, text: &str, blocking: bool) -> crate::Result<Option<std::result::Result<Value, String>>> {
        self.lisp.files.insert(file_key(name), text.to_string());
        let r = self.lisp_eval(text, false, blocking, Vec::new())?;
        if !matches!(r, Some(Err(_))) {
            let shown = std::path::Path::new(name).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or(name).to_string();
            self.echo(format!("{shown} successfully loaded."));
        }
        Ok(r)
    }
}

fn file_key(name: &str) -> String {
    let base = std::path::Path::new(&name.replace('\\', "/")).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or(name).to_ascii_lowercase();
    base.strip_suffix(".lsp").map(str::to_string).unwrap_or(base)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod block_tests;
