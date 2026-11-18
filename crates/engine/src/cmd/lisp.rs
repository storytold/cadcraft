//! AutoLISP commands: APPLOAD loads a `.lsp` file; `lisp` evaluates LISP for programmatic
//! callers (scripts, the control channel, MCP, the CLI).

use serde_json::{Value, json};

use super::{bad, str_param};
use crate::lisp::value::{Value as Lv, to_text};
use crate::{Accept, CommandSpec, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("appload", "Load Application...", run_appload)
            .menu(&["Tools", "Load Application..."])
            .alias(&["ap"])
            .params("{path: .lsp file} | {name, data: base64 | text} → {value?, commands: [commands the file defined], error?}")
            .noundo()
            .interactive(|_| Ok(Box::new(ApploadM))),
        CommandSpec::new("lisp", "Evaluate LISP", run_lisp)
            .params(
                "{code: LISP source, inputs?: [answers to get… calls: numbers, strings, [x,y] points]} → {value (printed as prin1), output: lines printed, error?}",
            )
            .noundo(),
    ]
}

/// Whether `path` names an AutoLISP routine: APPLOAD loads `.lsp` files only, in any case.
fn is_lisp_path(path: &str) -> bool {
    std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("lsp"))
}

/// The text of a LISP file from `{path}` or `{name, data | text}`, and its name.
fn source(p: &Value) -> Result<(String, String)> {
    if let Some(text) = str_param(p, "text") {
        return Ok((str_param(p, "name").unwrap_or("untitled.lsp").to_string(), text.to_string()));
    }
    if let Some(data) = str_param(p, "data") {
        let bytes = super::file::base64_decode(data).ok_or_else(|| bad("appload", "invalid base64"))?;
        return Ok((str_param(p, "name").unwrap_or("untitled.lsp").to_string(), String::from_utf8_lossy(&bytes).into_owned()));
    }
    // The path is read as it is given (a JSON caller may name a file or folder with spaces);
    // only a path of blanks alone is refused.
    let path = str_param(p, "path")
        .filter(|p| !p.trim().is_empty())
        .ok_or_else(|| bad("appload", "`path`, or `name` with `data` (base64) or `text`, is required"))?;
    if !is_lisp_path(path) {
        return Err(bad("appload", format!("{path}: not an AutoLISP file (APPLOAD loads .lsp files)")));
    }
    let text = crate::lisp::builtins::read_text_file(path).ok_or_else(|| bad("appload", format!("{path}: can't read the file")))?;
    Ok((path.to_string(), text))
}

/// The result of a blocking LISP run as JSON, with the lines it printed.
fn report(s: &Session, from_log: usize, r: Option<std::result::Result<Lv, String>>) -> Value {
    let output: Vec<String> = s.log.get(from_log..).map(|l| l.to_vec()).unwrap_or_default();
    match r {
        Some(Ok(v)) => json!({ "value": to_text(&v, true), "output": output }),
        Some(Err(e)) => json!({ "error": e, "output": output }),
        None => json!({ "output": output }),
    }
}

fn run_appload(s: &mut Session, p: &Value) -> Result<Value> {
    let (name, text) = source(p)?;
    let before = s.lisp_commands();
    let from = s.log.len();
    let r = s.lisp_load(&name, &text, true)?;
    let mut out = report(s, from, r);
    // The commands the file defined, so a caller can run them next (a failed file may still
    // have defined some before the error).
    out["commands"] = json!(s.lisp_commands().into_iter().filter(|c| !before.contains(c)).collect::<Vec<_>>());
    Ok(out)
}

/// A JSON input as a LISP value: numbers, strings, `[x, y(, z)]` points, null for Enter.
fn input_value(v: &Value) -> Lv {
    match v {
        Value::Array(a) if a.len() >= 2 && a.iter().all(Value::is_number) => Lv::point(&a.iter().filter_map(Value::as_f64).collect::<Vec<_>>()),
        other => crate::lisp::drawing_from_json(other),
    }
}

fn run_lisp(s: &mut Session, p: &Value) -> Result<Value> {
    let code = str_param(p, "code").ok_or_else(|| bad("lisp", "`code` is required"))?;
    let inputs = p.get("inputs").and_then(Value::as_array).map(|a| a.iter().map(input_value).collect()).unwrap_or_default();
    let from = s.log.len();
    let r = s.lisp_eval(code, false, true, inputs).map_err(|e| match e {
        EngineError::Other(m) => bad("lisp", m),
        e => e,
    })?;
    Ok(report(s, from, r))
}

/// APPLOAD typed or chosen from the menu: asks for the file by name.
struct ApploadM;

impl Interactive for ApploadM {
    fn name(&self) -> &'static str {
        "APPLOAD"
    }
    fn prompt(&self, _: &Session) -> Prompt {
        Prompt::new("Enter name of LISP file to load", Accept::TEXT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if matches!(i, Input::Cancel | Input::Enter) {
            return Ok(Step::Cancel);
        }
        let Input::Text(t) = i else { return Ok(Step::Continue) };
        let path = t.trim().trim_matches('"').to_string();
        if path.is_empty() {
            return Ok(Step::Cancel);
        }
        match source(&json!({ "path": path })) {
            Ok((name, text)) => {
                // The routine may wait for the user: it carries on after this command ends.
                if let Err(e) = s.lisp_load(&name, &text, false) {
                    s.echo(e.to_string());
                }
                Ok(Step::Done)
            }
            Err(EngineError::BadParams { msg, .. }) => {
                s.echo(msg);
                Ok(Step::Done)
            }
            Err(e) => Err(e),
        }
    }
}
