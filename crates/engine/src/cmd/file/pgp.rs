//! The user's command alias file (the publicly documented `.pgp` format: `ALIAS, *COMMAND` lines,
//! `;` comments): ALIASEDIT and REINIT.
//!
//! A loaded file adds aliases or overrides the built-in ones for the rest of the session, for
//! every drawing; command names themselves always win. Lines that define external commands
//! (programs the file would have the application start) are never run: they are skipped with a
//! note. CADCraft ships no alias file; the aliases built into the command catalog stay the
//! defaults.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use serde_json::{Value, json};

use super::super::helpers::support_text;
use super::*;

/// Most user aliases held.
pub const MAX_ALIASES: usize = 10_000;
/// Longest alias name.
const MAX_ALIAS_CHARS: usize = 64;
/// Longest line read.
const MAX_LINE_CHARS: usize = 4096;
/// Most warnings reported.
const MAX_WARNINGS: usize = 100;

#[derive(Default)]
struct Aliases {
    /// Lower-case alias → command id.
    map: BTreeMap<String, String>,
    /// The file last loaded (REINIT reads it again).
    file: Option<String>,
}

fn store() -> MutexGuard<'static, Aliases> {
    static USER: OnceLock<Mutex<Aliases>> = OnceLock::new();
    USER.get_or_init(|| Mutex::new(Aliases::default())).lock().unwrap_or_else(PoisonError::into_inner)
}

/// The command a user alias stands for (`name` in lower case).
pub(crate) fn user_alias(name: &str) -> Option<String> {
    store().map.get(name).cloned()
}

/// The command id `target` names (`*LINE`, `_.line`, `-LAYER`…), if there is such a command.
pub fn command_id(target: &str) -> Option<String> {
    let t = target.trim().trim_start_matches('*').trim().trim_start_matches(['_', '.']).to_ascii_lowercase();
    find_command(&t).map(|c| c.id.to_string())
}

/// Whether `a` can be typed as an alias: one word of printable characters, not a command name.
pub fn alias_name(a: &str) -> std::result::Result<String, String> {
    let a = a.trim();
    if a.is_empty() || a.chars().count() > MAX_ALIAS_CHARS || a.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\'' || c == '{') {
        return Err(format!("`{a}` can't be an alias"));
    }
    let l = a.to_ascii_lowercase();
    if find_command(&l).is_some() {
        return Err(format!("{a} is a command name; it stays the command"));
    }
    Ok(l)
}

/// The aliases of a `.pgp` file's text, and its problems.
fn parse(text: &str) -> (Vec<(String, String)>, Vec<String>) {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut warnings = Vec::new();
    let mut warn = |no: usize, msg: String| {
        if warnings.len() < MAX_WARNINGS {
            warnings.push(format!("line {no}: {msg}"));
        }
    };
    for (i, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let no = i.saturating_add(1);
        let line = line.split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.chars().count() > MAX_LINE_CHARS {
            warn(no, "line too long; skipped".into());
            continue;
        }
        let Some((alias, target)) = line.split_once(',') else {
            warn(no, format!("`{line}` is not ALIAS, *COMMAND; skipped"));
            continue;
        };
        if !target.trim_start().starts_with('*') {
            warn(no, format!("{} is an external command; CADCraft doesn't run programs from alias files", alias.trim()));
            continue;
        }
        let name = match alias_name(alias) {
            Ok(n) => n,
            Err(e) => {
                warn(no, format!("{e}; skipped"));
                continue;
            }
        };
        let Some(id) = command_id(target) else {
            warn(no, format!("{}: unknown command {}; skipped", alias.trim(), target.trim()));
            continue;
        };
        if out.len() >= MAX_ALIASES {
            warn(no, format!("more than {MAX_ALIASES} aliases; the rest are skipped"));
            break;
        }
        // A later line for the same alias wins, as it would when typed into the file twice.
        out.retain(|(a, _)| *a != name);
        out.push((name, id));
    }
    (out, warnings)
}

fn listing(warnings: &[String], added: usize) -> Value {
    let st = store();
    json!({ "aliases": st.map, "file": st.file, "loaded": added, "warnings": warnings })
}

/// Replace the user aliases with a file's (`path` is remembered for REINIT).
fn load(text: &str, path: Option<String>) -> (usize, Vec<String>) {
    let (aliases, warnings) = parse(text);
    let mut st = store();
    st.map = aliases.into_iter().collect();
    if path.is_some() {
        st.file = path;
    }
    (st.map.len(), warnings)
}

/// ALIASEDIT `{file? | text?, set?: {alias: command}, remove?: [alias], clear?: bool}`: load an
/// alias file (replacing the user aliases), clear them, or add and remove single aliases.
fn run_aliasedit(_s: &mut Session, p: &Value) -> Result<Value> {
    let mut warnings = Vec::new();
    let mut added = 0;
    if bool_or(p, "clear", false) {
        let mut st = store();
        st.map.clear();
        st.file = None;
    }
    if let Some((text, source)) = support_text("aliasedit", p, "file", "text")? {
        let path = p.get("file").and_then(Value::as_str).map(|_| source);
        (added, warnings) = load(&text, path);
    }
    match p.get("set") {
        None | Some(Value::Null) => {}
        Some(Value::Object(m)) => {
            for (alias, target) in m {
                let name = alias_name(alias).map_err(|e| bad("aliasedit", e))?;
                let target = target.as_str().ok_or_else(|| bad("aliasedit", format!("{alias}: the command must be a name")))?;
                let id = command_id(target).ok_or_else(|| bad("aliasedit", format!("{alias}: unknown command {target}")))?;
                let mut st = store();
                if !st.map.contains_key(&name) && st.map.len() >= MAX_ALIASES {
                    return Err(bad("aliasedit", format!("more than {MAX_ALIASES} aliases")));
                }
                st.map.insert(name, id);
                added += 1;
            }
        }
        Some(_) => return Err(bad("aliasedit", "`set` is an object {alias: command}")),
    }
    match p.get("remove") {
        None | Some(Value::Null) => {}
        Some(Value::Array(a)) => {
            let mut st = store();
            for v in a {
                if let Some(n) = v.as_str() {
                    st.map.remove(&n.trim().to_ascii_lowercase());
                }
            }
        }
        Some(_) => return Err(bad("aliasedit", "`remove` is a list of aliases")),
    }
    Ok(listing(&warnings, added))
}

/// REINIT `{file? | text?}`: read the alias file again (the one last loaded, or the given one).
fn run_reinit(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some((text, source)) = support_text("reinit", p, "file", "text")? {
        let path = p.get("file").and_then(Value::as_str).map(|_| source);
        let (n, warnings) = load(&text, path);
        s.echo(format!("{n} command alias(es) loaded."));
        return Ok(listing(&warnings, n));
    }
    let Some(path) = store().file.clone() else {
        s.echo("No command alias file is loaded; ALIASEDIT {file} loads one.");
        return Ok(listing(&[], 0));
    };
    let (text, _) = support_text("reinit", &json!({ "file": path }), "file", "text")?.ok_or_else(|| bad("reinit", "no alias file"))?;
    let (n, warnings) = load(&text, Some(path.clone()));
    s.echo(format!("{n} command alias(es) loaded from {path}."));
    Ok(listing(&warnings, n))
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("aliasedit", "Command Aliases", run_aliasedit)
            .menu(&["Tools", "Customize", "Edit Command Aliases (PGP)..."])
            .params("{file?: path to a .pgp alias file | text?: its contents, set?: {alias: command}, remove?: [alias], clear?: bool} → {aliases, file, loaded, warnings}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("reinit", "Reinitialize", run_reinit).params("{file? | text?} (default: the alias file loaded last)").enabled(always).noundo(),
    ]
}
