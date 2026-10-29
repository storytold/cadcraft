//! AutoLISP commands: APPLOAD loads a `.lsp` file; `lisp` evaluates LISP for programmatic
//! callers (scripts, the control channel, MCP, the CLI).

use serde_json::{Value, json};

use super::{bad, str_param};
use crate::lisp::value::{Value as Lv, to_text};
use crate::{Accept, CommandSpec, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// Command specifications for AutoLISP loading (`appload`, `netload`), the IDE (`vbaide`), and evaluation (`lisp`).
pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("appload", "Load Application...", run_appload)
            .menu(&["Tools", "Load Application..."])
            .alias(&["ap"])
            .params("{path: .lsp file} | {name, data: base64 | text} → {value?, error?}")
            .noundo()
            .interactive(|_| Ok(Box::new(ApploadM))),
        CommandSpec::new("netload", "Load LISP / Application (NETLOAD)...", run_appload)
            .menu(&["Tools", "Load Application (NETLOAD)..."])
            .params("{path: .lsp file} | {name, data: base64 | text} → {value?, error?}")
            .noundo()
            .interactive(|_| Ok(Box::new(ApploadM))),
        CommandSpec::new("vbaide", "AutoLISP / VBA IDE...", run_vbaide)
            .menu(&["Tools", "AutoLISP / VBA IDE..."])
            .alias(&["vlide", "vlisp"])
            .params("{path?: .lsp file, code?: LISP source} → {value?, output?, error?}")
            .noundo()
            .interactive(|_| Ok(Box::new(ApploadM))),
        CommandSpec::new("lisp", "Evaluate LISP", run_lisp)
            .params(
                "{code: LISP source, inputs?: [answers to get… calls: numbers, strings, [x,y] points]} → {value (printed as prin1), output: lines printed, error?}",
            )
            .noundo(),
    ]
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
    let path = str_param(p, "path").ok_or_else(|| bad("appload", "`path`, or `name` with `data` (base64) or `text`, is required"))?;
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

/// Load and evaluate an AutoLISP routine from `{path}`, `{text}`, or base64 `{data}`.
fn run_appload(s: &mut Session, p: &Value) -> Result<Value> {
    let (name, text) = source(p)?;
    let from = s.log.len();
    let r = s.lisp_load(&name, &text, true)?;
    Ok(report(s, from, r))
}

/// Dispatch `VBAIDE` (`VLIDE`, `VLISP`): evaluates `code` when given, loads a file/text when given,
/// or reports `"open"` so the UI opens the AutoLISP IDE window.
fn run_vbaide(s: &mut Session, p: &Value) -> Result<Value> {
    if str_param(p, "code").is_some() {
        return run_lisp(s, p);
    }
    if str_param(p, "path").is_some() || str_param(p, "text").is_some() || str_param(p, "data").is_some() {
        return run_appload(s, p);
    }
    Ok(json!({ "status": "open" }))
}

/// A JSON input as a LISP value: numbers, strings, `[x, y(, z)]` points, null for Enter.
fn input_value(v: &Value) -> Lv {
    match v {
        Value::Array(a) if a.len() >= 2 && a.iter().all(Value::is_number) => Lv::point(&a.iter().filter_map(Value::as_f64).collect::<Vec<_>>()),
        other => crate::lisp::drawing_from_json(other),
    }
}

/// Evaluate a LISP expression from `{code, inputs?}` and return its printed result and output lines.
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;
    use crate::cmd::resolve_alias;

    #[test]
    fn netload_and_vbaide_dispatch_and_aliases_resolve() {
        let mut s = Session::new();
        assert_eq!(resolve_alias("vlide"), "vbaide");
        assert_eq!(resolve_alias("vlisp"), "vbaide");

        let open = s.execute("vbaide", &json!({})).unwrap();
        assert_eq!(open["status"], "open");

        let eval = s.execute("vbaide", &json!({ "code": "(+ 1 2)" })).unwrap();
        assert_eq!(eval["value"], "3");

        let loaded = s.execute("vbaide", &json!({ "name": "test.lsp", "text": "(defun sq (x) (* x x)) (sq 4)" })).unwrap();
        assert_eq!(loaded["value"], "16");

        let nl = s.execute("netload", &json!({ "name": "nl.lsp", "text": "(+ 10 20)" })).unwrap();
        assert_eq!(nl["value"], "30");
    }
}
