//! Hatch patterns from the user's own `.pat` files.
//!
//! Loaded patterns join the pattern list for the rest of the session, for every drawing (as a
//! pattern file on the support path is available to all of them). A hatch made with one keeps
//! the pattern's line families (`Hatch::pattern_lines`), so the drawing shows and saves it
//! without the file: DXF writes the definition with the hatch, and reading it back restores it.
//! A file can't replace a built-in pattern (its name keeps meaning the library's definition).

use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use cadcraft_doc::library::files::{UserPattern, parse_pat};
use cadcraft_doc::library::{PatternLine, pattern, standard_patterns};
use serde_json::{Value, json};

use super::super::helpers::support_text;
use super::{Result, Session, bad, bool_or};

/// Most user patterns held at once.
pub const MAX_USER_PATTERNS: usize = 4096;

fn store() -> MutexGuard<'static, Vec<UserPattern>> {
    static USER: OnceLock<Mutex<Vec<UserPattern>>> = OnceLock::new();
    USER.get_or_init(|| Mutex::new(Vec::new())).lock().unwrap_or_else(PoisonError::into_inner)
}

/// The user pattern called `name` (case-insensitive), if a file loaded one.
pub(crate) fn user_pattern(name: &str) -> Option<UserPattern> {
    store().iter().find(|p| p.name.eq_ignore_ascii_case(name)).cloned()
}

/// The line families a new hatch of pattern `name` carries: a user pattern's, else none (the
/// library's patterns are drawn and saved from the library).
pub(crate) fn lines_for(name: &str) -> Vec<PatternLine> {
    if pattern(name).is_some() {
        return Vec::new();
    }
    user_pattern(name).map(|p| p.lines).unwrap_or_default()
}

/// Every pattern name offered: the library's, then the loaded user patterns.
pub(crate) fn names() -> Vec<String> {
    let mut v: Vec<String> = standard_patterns().iter().map(|p| p.name.to_string()).collect();
    v.extend(store().iter().map(|p| p.name.clone()));
    v
}

/// A pattern offered for new hatches: a library pattern or one a `.pat` file loaded.
#[derive(Clone, Debug, PartialEq)]
pub struct PatternChoice {
    pub name: String,
    pub description: String,
    /// The line families at unit scale (none for SOLID).
    pub lines: Vec<PatternLine>,
}

/// Every pattern offered for new hatches, in the order of [`names`], for a pattern picker.
pub fn pattern_choices() -> Vec<PatternChoice> {
    let mut v: Vec<PatternChoice> = standard_patterns()
        .into_iter()
        .map(|p| PatternChoice { name: p.name.to_string(), description: p.description.to_string(), lines: p.lines })
        .collect();
    v.extend(store().iter().map(|p| PatternChoice { name: p.name.clone(), description: p.description.clone(), lines: p.lines.clone() }));
    v
}

/// Add the patterns of a `.pat` file's text (a pattern loaded before under the same name is
/// replaced). Returns the names added and the file's problems.
pub(crate) fn load(text: &str) -> (Vec<String>, Vec<String>) {
    let parsed = parse_pat(text);
    let mut warnings = parsed.warnings;
    let mut added = Vec::new();
    let mut user = store();
    for p in parsed.items {
        if pattern(&p.name).is_some() {
            warnings.push(format!("{} is a built-in pattern; the built-in definition is kept", p.name));
            continue;
        }
        let full = user.len() >= MAX_USER_PATTERNS;
        match user.iter_mut().find(|q| q.name.eq_ignore_ascii_case(&p.name)) {
            Some(q) => *q = p.clone(),
            None if full => {
                warnings.push(format!("more than {MAX_USER_PATTERNS} user patterns; {} not loaded", p.name));
                continue;
            }
            None => user.push(p.clone()),
        }
        added.push(p.name);
    }
    (added, warnings)
}

/// Load the pattern file a HATCH call names (`patternFile` path or `patternText` contents).
pub(super) fn load_param(p: &Value) -> Result<()> {
    if let Some((text, source)) = support_text("hatch", p, "patternFile", "patternText")? {
        let (added, warnings) = load(&text);
        if added.is_empty() {
            return Err(bad("hatch", format!("no hatch pattern could be read from {source}: {}", warnings.join("; "))));
        }
    }
    Ok(())
}

/// A `.pat` file typed at HATCH's pattern prompt: load it and return the pattern to use (the
/// one named like the file, else its first). `None` when `t` isn't a `.pat` file name.
pub(super) fn typed_file(s: &mut Session, t: &str) -> Option<String> {
    if !t.to_ascii_lowercase().ends_with(".pat") {
        return None;
    }
    let text = match support_text("hatch", &json!({ "file": t }), "file", "text") {
        Ok(Some((text, _))) => text,
        Ok(None) => return None,
        Err(e) => {
            s.echo(e.to_string());
            return Some(String::new());
        }
    };
    let (added, warnings) = load(&text);
    for w in warnings {
        s.echo(w);
    }
    s.echo(format!("{} pattern(s) loaded from {t}: {}", added.len(), added.join(", ")));
    let file = t.rsplit(['/', '\\']).next().unwrap_or(t);
    let stem = file.get(..file.len().saturating_sub(4)).unwrap_or("");
    Some(added.iter().find(|n| n.eq_ignore_ascii_case(stem)).or(added.first()).cloned().unwrap_or_default())
}

/// HATCH.PATTERNS `{file?: path, text?: contents, clear?: bool}`: load a `.pat` file into the
/// pattern list, or forget the loaded ones; returns the list.
pub(crate) fn run_patterns(_s: &mut Session, p: &Value) -> Result<Value> {
    if bool_or(p, "clear", false) {
        store().clear();
    }
    let (added, warnings) = match support_text("hatch.patterns", p, "file", "text")? {
        Some((text, _)) => load(&text),
        None => (Vec::new(), Vec::new()),
    };
    let user: Vec<Value> = store().iter().map(|p| json!({ "name": p.name, "description": p.description, "families": p.lines.len() })).collect();
    Ok(json!({
        "standard": standard_patterns().iter().map(|p| p.name).collect::<Vec<_>>(),
        "user": user,
        "added": added,
        "warnings": warnings,
    }))
}
