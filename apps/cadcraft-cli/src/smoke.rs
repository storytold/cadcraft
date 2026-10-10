//! `cadcraft-cli smoke`: run every registered command headlessly and report what breaks.
//!
//! Each command gets a fresh session (the sample drawing plus `selectall`, so selection-dependent
//! commands reach their body) and so cannot contaminate the next. Two phases per command: the JSON
//! form (`Session::execute` with null params) and, for commands that carry a prompt machine, a
//! generic drive of that machine read off `Prompt::accept` with canned points, numbers, text and
//! picks, which then ends the command the way a user does — Enter, then Escape. Repeat-style
//! commands (LINE, PLINE, POINT) prompt for ever by design, so not terminating on its own is not a
//! failure; ignoring *both* Enter and Escape is, and `ended` records which of the two worked.
//! Artifacts (`results.jsonl`, `summary.json`, `report.md`) follow the v1 smoke schema.
//!
//! What this does **not** cover: the UI path — menus, toolbar, mouse, grips, dialogs — is untested
//! here; a later `--control` transport will drive a running app through the control channel, and
//! the artifact schema already reserves `transport` for that. Nor are *results* checked: a command
//! that returns without an error counts as a pass even if it drew the wrong thing. Parameterised
//! behaviour is not exercised either, because every JSON call passes null, so commands that need
//! arguments report `expected_error` instead of running their body.
//!
//! The timeout is measured, not enforced: a synchronous in-process call cannot be interrupted, so
//! `--timeout-ms` records an overrun after the fact (status `fail`) and a command that genuinely
//! hangs hangs the whole sweep. Panics are caught — `Session::execute` guards the command body and
//! the harness guards `start` / `input` / `current_prompt` as well — and reported as `fail`, though
//! the default panic hook still prints to stderr.

use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cadcraft_engine::doc::Handle;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{CommandSpec, EngineError, Input, Prompt, Session, command_specs};
use serde_json::{Value, json};

const FIXTURE: &str = "sample";
const TRANSPORT: &str = "inproc";
/// Canned answers fed to a prompt machine before the sweep tries to end the command.
const DRIVE_STEPS: usize = 6;
/// How a user ends a command that is still asking for input: Enter, then Escape.
const FINISHERS: [fn() -> Input; 5] = [|| Input::Enter, || Input::Enter, || Input::Enter, || Input::Cancel, || Input::Cancel];
const CANNED: [Vec2; 5] = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(5.0, 15.0), Vec2::new(0.0, 10.0)];

/// Commands the sweep must not run headlessly. No engine command quits the process or opens a
/// browser (`ui.quit` and friends live in the UI crate's `UI_COMMANDS`, outside `command_specs()`),
/// so this list only covers the ones that touch the filesystem at a caller-supplied path: with null
/// params they report `BadParams` instead of writing, but the sweep must not depend on that.
const SKIP: &[(&str, &str)] = &[
    ("open", "reads an arbitrary path from disk; the sweep must stay inside the out dir"),
    ("qsave", "writes the active drawing to its path on disk"),
    ("saveas", "writes a drawing file to a caller-supplied path"),
    ("wblock", "writes a block file to a caller-supplied path"),
];

/// Commands that legitimately leave the session without a document (not state corruption).
const CLOSES_DOC: &[&str] = &["close", "closeall"];

struct Opts {
    out: std::path::PathBuf,
    filter: Option<String>,
    only: Vec<String>,
    json_phase: bool,
    inter_phase: bool,
    timeout: Duration,
    json: bool,
    quiet: bool,
}

#[derive(Clone, Copy, Default)]
struct Tally {
    pass: usize,
    expected_error: usize,
    fail: usize,
    skip: usize,
}

impl Tally {
    fn add(&mut self, status: &str) {
        match status {
            "pass" => self.pass += 1,
            "expected_error" => self.expected_error += 1,
            "fail" => self.fail += 1,
            _ => self.skip += 1,
        }
    }
    fn total(self) -> usize {
        self.pass + self.expected_error + self.fail + self.skip
    }
    fn value(self) -> Value {
        json!({ "pass": self.pass, "expected_error": self.expected_error, "fail": self.fail, "skip": self.skip })
    }
}

/// One (command, phase) outcome.
struct Rec {
    command: &'static str,
    label: &'static str,
    menu: Vec<&'static str>,
    phase: &'static str,
    status: &'static str,
    duration_us: u64,
    error: Option<String>,
    error_kind: Option<&'static str>,
    entities_before: usize,
    entities_after: usize,
    inputs: Vec<String>,
    /// Interactive phase: how the command ended — `completed`, `enter`, `escape` or `stuck`.
    /// Empty for the JSON phase.
    ended: &'static str,
}

impl Rec {
    fn value(&self, run_id: &str) -> Value {
        json!({
            "schema": "cadcraft.smoke.result/1",
            "run_id": run_id,
            "command": self.command,
            "label": self.label,
            "menu": self.menu,
            "phase": self.phase,
            "status": self.status,
            "duration_us": self.duration_us,
            "error": self.error,
            "error_kind": self.error_kind,
            "entities_before": self.entities_before,
            "entities_after": self.entities_after,
            "inputs": self.inputs,
            "ended": self.ended,
            "fixture": FIXTURE,
        })
    }
}

/// What the guarded body of one phase reports back.
struct Outcome {
    before: usize,
    after: usize,
    lost_doc: bool,
    /// `(engine error variant, message)`; a `None` variant marks a harness-detected failure.
    err: Option<(Option<&'static str>, String)>,
    inputs: Vec<String>,
    ended: &'static str,
}

fn panic_msg(p: Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>().map(|s| (*s).to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into())
}

fn catch<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(panic_msg)
}

fn kind(e: &EngineError) -> &'static str {
    match e {
        EngineError::UnknownCommand(_) => "UnknownCommand",
        EngineError::Disabled(..) => "Disabled",
        EngineError::BadParams { .. } => "BadParams",
        EngineError::NoDocument => "NoDocument",
        EngineError::Other(_) => "Other",
        EngineError::Internal(..) => "Internal",
    }
}

fn describe(i: &Input) -> String {
    match i {
        Input::Point(p) => format!("{},{}", p.x, p.y),
        Input::Keyword(k) => k.clone(),
        Input::Text(t) => t.clone(),
        Input::Pick(h) => format!("<pick {}>", h.len()),
        Input::Enter => "<enter>".into(),
        Input::Cancel => "<esc>".into(),
    }
}

/// A fresh session on the sample drawing with everything selected.
fn fixture() -> Session {
    let mut s = Session::empty();
    s.open_drawing(cadcraft_engine::sample::default_sample(), "Smoke", None);
    let _ = s.execute("selectall", &Value::Null);
    s
}

fn entities(s: &Session) -> usize {
    s.doc().map(cadcraft_engine::doc::Drawing::entity_count).unwrap_or(0)
}

/// The answer to give one prompt: pick real objects where a selection is wanted, then canned
/// points, numbers and text per what the prompt accepts.
fn canned(p: &Prompt, handles: &[Handle], pt: &mut usize, picked: &mut bool) -> Input {
    if p.accept.select && !handles.is_empty() && !*picked {
        *picked = true;
        return Input::Pick(handles.to_vec());
    }
    if p.accept.select {
        *picked = false;
        return Input::Enter;
    }
    if p.accept.point {
        let q = CANNED.get(*pt % CANNED.len()).copied().unwrap_or(Vec2::ZERO);
        *pt += 1;
        return Input::Point(q);
    }
    if p.accept.number {
        return Input::Text("5".into());
    }
    if p.accept.text {
        return Input::Text("SMOKE".into());
    }
    Input::Enter
}

fn run_json(spec: &CommandSpec) -> (Duration, Result<Outcome, String>) {
    let t = Instant::now();
    let caught = catch(|| {
        let mut s = fixture();
        let before = entities(&s);
        let had = !s.docs.is_empty();
        let err = s.execute(spec.id, &Value::Null).err().map(|e| (Some(kind(&e)), e.to_string()));
        let after = entities(&s);
        Outcome { before, after, lost_doc: had && s.docs.is_empty(), err, inputs: Vec::new(), ended: "" }
    });
    (t.elapsed(), caught)
}

fn run_interactive(spec: &CommandSpec) -> (Duration, Result<Outcome, String>) {
    let t = Instant::now();
    let caught = catch(|| {
        let mut s = fixture();
        let before = entities(&s);
        let had = !s.docs.is_empty();
        let handles: Vec<Handle> = s.selection().into_iter().take(2).collect();
        let mut inputs: Vec<String> = Vec::new();
        let mut err = None;
        let mut ended = "completed";
        if let Err(e) = s.start(spec.id) {
            ended = "";
            err = Some((Some(kind(&e)), e.to_string()));
        } else {
            let (mut pt, mut picked) = (0usize, false);
            // Answer the prompts the machine asks for. Repeat-style commands (LINE, PLINE,
            // POINT) never run out of prompts by design, so this is capped rather than looped
            // until the machine is done.
            for _ in 0..DRIVE_STEPS {
                let Some(p) = s.current_prompt() else { break };
                let i = canned(&p, &handles, &mut pt, &mut picked);
                inputs.push(describe(&i));
                if let Err(e) = s.input(i) {
                    err = Some((Some(kind(&e)), e.to_string()));
                    break;
                }
            }
            // Then end it the way a user does: Enter, then Escape. A command that survives
            // both cannot be exited at all, and that is a real bug.
            if err.is_none() && s.current_prompt().is_some() {
                ended = "stuck";
                for make in FINISHERS {
                    let i = make();
                    let esc = matches!(i, Input::Cancel);
                    inputs.push(describe(&i));
                    if let Err(e) = s.input(i) {
                        err = Some((Some(kind(&e)), e.to_string()));
                        break;
                    }
                    if s.current_prompt().is_none() {
                        ended = if esc { "escape" } else { "enter" };
                        break;
                    }
                }
                if err.is_none() && ended == "stuck" {
                    let p = s.current_prompt().map(|p| p.display()).unwrap_or_default();
                    err = Some((None, format!("cannot be exited: still prompting `{p}` after Enter and Escape")));
                    s.cancel();
                }
            }
        }
        let after = entities(&s);
        Outcome { before, after, lost_doc: had && s.docs.is_empty(), err, inputs, ended }
    });
    (t.elapsed(), caught)
}

fn finish(spec: &CommandSpec, phase: &'static str, dt: Duration, caught: Result<Outcome, String>, timeout: Duration) -> Rec {
    let mut r = Rec {
        command: spec.id,
        label: spec.label,
        menu: spec.menu.to_vec(),
        phase,
        status: "pass",
        duration_us: u64::try_from(dt.as_micros()).unwrap_or(u64::MAX),
        error: None,
        error_kind: None,
        entities_before: 0,
        entities_after: 0,
        inputs: Vec::new(),
        ended: "",
    };
    match caught {
        Err(p) => {
            r.status = "fail";
            r.error_kind = Some("Internal");
            r.error = Some(format!("panic escaped the engine guard: {p}"));
        }
        Ok(out) => {
            r.entities_before = out.before;
            r.entities_after = out.after;
            r.inputs = out.inputs;
            r.ended = out.ended;
            match out.err {
                None => {}
                Some((Some("Internal"), m)) => {
                    r.status = "fail";
                    r.error_kind = Some("Internal");
                    r.error = Some(m);
                }
                Some((Some(k), m)) => {
                    r.status = "expected_error";
                    r.error_kind = Some(k);
                    r.error = Some(m);
                }
                Some((None, m)) => {
                    r.status = "fail";
                    r.error = Some(m);
                }
            }
            if out.lost_doc && !CLOSES_DOC.contains(&spec.id) {
                r.status = "fail";
                r.error = Some(match r.error.take() {
                    Some(e) => format!("the document went missing afterwards ({e})"),
                    None => "the document went missing afterwards".into(),
                });
            }
        }
    }
    if dt > timeout {
        r.status = "fail";
        let over = format!("timeout: took {} ms, limit {} ms", dt.as_millis(), timeout.as_millis());
        r.error = Some(match r.error.take() {
            Some(e) => format!("{over} ({e})"),
            None => over,
        });
    }
    r
}

fn skipped(spec: &CommandSpec, phase: &'static str, reason: &str) -> Rec {
    Rec {
        command: spec.id,
        label: spec.label,
        menu: spec.menu.to_vec(),
        phase,
        status: "skip",
        duration_us: 0,
        error: Some(reason.to_string()),
        error_kind: None,
        entities_before: 0,
        entities_after: 0,
        inputs: Vec::new(),
        ended: "",
    }
}

// ---------------- artifacts ----------------

/// `YYYY-MM-DD` from a day number relative to the Unix epoch (Hinnant's civil-from-days).
fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn stamp(t: SystemTime, compact: bool) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let secs = i64::try_from(secs).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (h, mi, sec) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil(days);
    if compact { format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{sec:02}") } else { format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{sec:02}Z") }
}

fn run_id(t: SystemTime) -> String {
    let nanos = t.duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    format!("{}-{:04x}", stamp(t, true), (nanos ^ std::process::id()) & 0xffff)
}

fn git_commit() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

/// One markdown table cell: no pipes, no newlines.
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\n', '\r'], " ")
}

fn summary(recs: &[Rec], id: &str, started: SystemTime, finished: SystemTime, wall: Duration, tested: usize) -> Value {
    let mut all = Tally::default();
    let (mut js, mut it) = (Tally::default(), Tally::default());
    for r in recs {
        all.add(r.status);
        let t = if r.phase == "json" { &mut js } else { &mut it };
        t.add(r.status);
    }
    let failures: Vec<Value> =
        recs.iter().filter(|r| r.status == "fail").map(|r| json!({ "command": r.command, "phase": r.phase, "error": r.error })).collect();
    let mut slow: Vec<&Rec> = recs.iter().filter(|r| r.status != "skip").collect();
    slow.sort_by(|a, b| b.duration_us.cmp(&a.duration_us).then(a.command.cmp(b.command)));
    let slowest: Vec<Value> = slow.iter().take(20).map(|r| json!({ "command": r.command, "phase": r.phase, "duration_us": r.duration_us })).collect();
    let mut ended: BTreeMap<&str, usize> = BTreeMap::new();
    for r in recs.iter().filter(|r| r.phase == "interactive" && !r.ended.is_empty()) {
        *ended.entry(r.ended).or_default() += 1;
    }
    json!({
        "schema": "cadcraft.smoke.summary/1",
        "run_id": id,
        "started_at": stamp(started, false),
        "finished_at": stamp(finished, false),
        "wall_ms": u64::try_from(wall.as_millis()).unwrap_or(u64::MAX),
        "transport": TRANSPORT,
        "version": env!("CARGO_PKG_VERSION"),
        "git_commit": git_commit(),
        "commands_total": command_specs().len(),
        "commands_tested": tested,
        "totals": all.value(),
        "by_phase": { "json": js.value(), "interactive": it.value() },
        "ended": ended,
        "failures": failures,
        "slowest": slowest,
    })
}

fn report(recs: &[Rec], sum: &Value, id: &str) -> String {
    let n = |p: &str, k: &str| sum.pointer(&format!("/{p}/{k}")).and_then(Value::as_u64).unwrap_or(0);
    let mut m = String::new();
    m.push_str(&format!("# CADCraft command smoke sweep `{id}`\n\n"));
    m.push_str(&format!(
        "cadcraft-cli {} @ {} — transport `{}`, fixture `{}`\n\n",
        sum["version"].as_str().unwrap_or("?"),
        sum["git_commit"].as_str().unwrap_or("?"),
        TRANSPORT,
        FIXTURE
    ));
    m.push_str(&format!(
        "{} → {} ({:.1} s), {} of {} commands tested, {} results\n\n",
        sum["started_at"].as_str().unwrap_or("?"),
        sum["finished_at"].as_str().unwrap_or("?"),
        sum["wall_ms"].as_u64().unwrap_or(0) as f64 / 1000.0,
        sum["commands_tested"].as_u64().unwrap_or(0),
        sum["commands_total"].as_u64().unwrap_or(0),
        recs.len()
    ));
    m.push_str("| status | total | json | interactive |\n|---|---:|---:|---:|\n");
    for k in ["pass", "expected_error", "fail", "skip"] {
        m.push_str(&format!("| {k} | {} | {} | {} |\n", n("totals", k), n("by_phase/json", k), n("by_phase/interactive", k)));
    }

    let fails: Vec<&Rec> = recs.iter().filter(|r| r.status == "fail").collect();
    m.push_str(&format!("\n## Failures ({})\n\n", fails.len()));
    if fails.is_empty() {
        m.push_str("None.\n");
    } else {
        m.push_str("| command | phase | kind | error | inputs |\n|---|---|---|---|---|\n");
        for r in &fails {
            m.push_str(&format!(
                "| `{}` | {} | {} | {} | {} |\n",
                r.command,
                r.phase,
                r.error_kind.unwrap_or("-"),
                cell(r.error.as_deref().unwrap_or("-")),
                cell(&r.inputs.join(" "))
            ));
        }
    }

    m.push_str("\n## 20 slowest\n\n| command | phase | ms |\n|---|---|---:|\n");
    if let Some(rows) = sum["slowest"].as_array() {
        for r in rows {
            m.push_str(&format!(
                "| `{}` | {} | {:.1} |\n",
                r["command"].as_str().unwrap_or("?"),
                r["phase"].as_str().unwrap_or("?"),
                r["duration_us"].as_u64().unwrap_or(0) as f64 / 1000.0
            ));
        }
    }

    let mut by_menu: BTreeMap<&str, Tally> = BTreeMap::new();
    for r in recs {
        by_menu.entry(r.menu.first().copied().unwrap_or("(no menu)")).or_default().add(r.status);
    }
    m.push_str("\n## By menu\n\n| menu | results | pass | expected_error | fail | skip |\n|---|---:|---:|---:|---:|---:|\n");
    for (k, t) in &by_menu {
        m.push_str(&format!("| {k} | {} | {} | {} | {} | {} |\n", t.total(), t.pass, t.expected_error, t.fail, t.skip));
    }
    m
}

// ---------------- driver ----------------

fn parse(args: &[String], id: &str) -> Result<Opts, String> {
    let mut o = Opts {
        out: std::path::PathBuf::from(format!("target/smoke/{id}")),
        filter: None,
        only: Vec::new(),
        json_phase: true,
        inter_phase: true,
        timeout: Duration::from_millis(5000),
        json: false,
        quiet: false,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => o.out = std::path::PathBuf::from(it.next().ok_or("--out needs a directory")?),
            "--filter" => o.filter = Some(it.next().ok_or("--filter needs a substring")?.to_ascii_lowercase()),
            "--only" => {
                let v = it.next().ok_or("--only needs ID[,ID]")?;
                o.only = v.split(',').map(|x| x.trim().to_ascii_lowercase()).filter(|x| !x.is_empty()).collect();
            }
            "--phase" => match it.next().map(String::as_str) {
                Some("json") => o.inter_phase = false,
                Some("interactive") => o.json_phase = false,
                Some("both") => {}
                _ => return Err("--phase needs json, interactive or both".into()),
            },
            "--timeout-ms" => {
                let v = it.next().ok_or("--timeout-ms needs a number")?;
                o.timeout = Duration::from_millis(v.parse().map_err(|_| format!("--timeout-ms: bad number {v}"))?);
            }
            "--json" => o.json = true,
            "--quiet" => o.quiet = true,
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(o)
}

pub fn run(args: &[String]) -> Result<(), String> {
    crate::install_io();
    let started = SystemTime::now();
    let id = run_id(started);
    let o = parse(args, &id)?;
    let specs: Vec<&'static CommandSpec> = command_specs()
        .iter()
        .filter(|c| o.only.is_empty() || o.only.iter().any(|x| x.as_str() == c.id))
        .filter(|c| o.filter.as_ref().is_none_or(|f| c.id.contains(f.as_str()) || c.label.to_ascii_lowercase().contains(f.as_str())))
        .collect();
    if specs.is_empty() {
        return Err("smoke: no command matched --filter/--only".into());
    }

    let clock = Instant::now();
    let mut recs: Vec<Rec> = Vec::new();
    let mut tested = 0usize;
    for spec in specs.iter().copied() {
        let skip = SKIP.iter().find(|(x, _)| *x == spec.id).map(|(_, why)| *why);
        let mut any = false;
        if o.json_phase {
            recs.push(match skip {
                Some(why) => skipped(spec, "json", why),
                None => {
                    any = true;
                    let (dt, caught) = run_json(spec);
                    finish(spec, "json", dt, caught, o.timeout)
                }
            });
        }
        if o.inter_phase && spec.interactive.is_some() {
            recs.push(match skip {
                Some(why) => skipped(spec, "interactive", why),
                None => {
                    any = true;
                    let (dt, caught) = run_interactive(spec);
                    finish(spec, "interactive", dt, caught, o.timeout)
                }
            });
        }
        if any {
            tested += 1;
        }
        if !o.quiet {
            for r in recs.iter().rev().take(2).filter(|r| r.status == "fail") {
                eprintln!("FAIL {} [{}]: {}", r.command, r.phase, r.error.as_deref().unwrap_or(""));
            }
        }
    }
    let wall = clock.elapsed();
    let sum = summary(&recs, &id, started, SystemTime::now(), wall, tested);
    let md = report(&recs, &sum, &id);

    std::fs::create_dir_all(&o.out).map_err(|e| format!("{}: {e}", o.out.display()))?;
    let mut jsonl = String::new();
    for r in &recs {
        jsonl.push_str(&r.value(&id).to_string());
        jsonl.push('\n');
    }
    let write = |name: &str, body: &str| -> Result<(), String> {
        let p = o.out.join(name);
        std::fs::write(&p, body).map_err(|e| format!("{}: {e}", p.display()))
    };
    write("results.jsonl", &jsonl)?;
    write("summary.json", &format!("{}\n", serde_json::to_string_pretty(&sum).unwrap_or_default()))?;
    write("report.md", &md)?;

    let fails = sum.pointer("/totals/fail").and_then(Value::as_u64).unwrap_or(0);
    if o.json {
        println!("{}", serde_json::to_string_pretty(&sum).unwrap_or_default());
    } else if !o.quiet {
        println!(
            "smoke {id}: {} results over {tested} commands in {:.1} s — {} pass, {} expected_error, {fails} fail, {} skip",
            recs.len(),
            wall.as_secs_f64(),
            sum.pointer("/totals/pass").and_then(Value::as_u64).unwrap_or(0),
            sum.pointer("/totals/expected_error").and_then(Value::as_u64).unwrap_or(0),
            sum.pointer("/totals/skip").and_then(Value::as_u64).unwrap_or(0)
        );
        eprintln!("wrote {}/{{results.jsonl,summary.json,report.md}}", o.out.display());
    }
    if fails > 0 {
        return Err(format!("smoke: {fails} failing result(s); see {}/report.md", o.out.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_writes_parseable_artifacts_with_no_failures() {
        let dir = std::env::temp_dir().join(format!("cadcraft-smoke-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let out = dir.to_str().expect("temp path is utf-8").to_string();
        let args = vec!["--out".to_string(), out, "--filter".to_string(), "zoom".to_string(), "--quiet".to_string()];
        let r = run(&args);

        let jsonl = std::fs::read_to_string(dir.join("results.jsonl")).expect("results.jsonl");
        let recs: Vec<Value> = jsonl.lines().map(|l| serde_json::from_str(l).expect("a result line is JSON")).collect();
        assert!(recs.len() >= 8, "only {} results", recs.len());
        assert!(recs.iter().all(|r| r["schema"] == "cadcraft.smoke.result/1" && r["fixture"] == FIXTURE));
        let bad: Vec<&Value> = recs.iter().filter(|r| r["status"] == "fail").collect();
        assert!(bad.is_empty(), "failing results: {bad:?}");

        let sum: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("summary.json")).expect("summary.json")).expect("summary is JSON");
        assert_eq!(sum["schema"], "cadcraft.smoke.summary/1");
        assert_eq!(sum["totals"]["fail"], 0);
        assert_eq!(sum["transport"], TRANSPORT);
        assert!(sum["commands_tested"].as_u64().unwrap() >= 5);
        assert!(sum["commands_total"].as_u64().unwrap() >= 200);

        let md = std::fs::read_to_string(dir.join("report.md")).expect("report.md");
        assert!(md.contains("## Failures (0)") && md.contains("## By menu"));
        assert!(r.is_ok(), "{r:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skip_list_names_real_commands_and_phase_flags_narrow_the_sweep() {
        for (id, why) in SKIP {
            assert!(cadcraft_engine::find_command(id).is_some(), "skip entry `{id}` is not a command");
            assert!(!why.is_empty());
        }
        let o = parse(&["--phase".to_string(), "json".to_string()], "x").unwrap();
        assert!(o.json_phase && !o.inter_phase);
        let o = parse(&["--only".to_string(), "line, circle".to_string()], "x").unwrap();
        assert_eq!(o.only, vec!["line".to_string(), "circle".to_string()]);
        assert!(parse(&["--phase".to_string()], "x").is_err());
        assert!(parse(&["--nope".to_string()], "x").is_err());
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(19_000), (2022, 1, 8));
        assert_eq!(stamp(UNIX_EPOCH + Duration::from_secs(1_760_000_000), false), "2025-10-09T08:53:20Z");
    }
}
