//! AUDIT and RECOVER (File ▸ Drawing Utilities): check a drawing for errors and fix them, and open
//! a damaged file with the fixes applied. The checks themselves live in [`crate::audit`].

use std::sync::Arc;

use serde_json::{Value, json};

use super::{CommandSpec, bad, bool_or, file, str_param};

use super::always;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("audit", "Audit", run_audit)
            .menu(&["File", "Drawing Utilities", "Audit"])
            .params("{fix?: bool} → {errors, fixed, issues: [{kind, location, handle?, message, fix}], message}")
            .interactive(|_| Ok(Box::new(Audit))),
        CommandSpec::new("recover", "Recover...", run_recover)
            .menu(&["File", "Drawing Utilities", "Recover..."])
            .params("{path, copy?: bool (an unsaved copy without a file name), title?} | {data: base64, name} → {index, uid, errors, fixed, issues, message}")
            .enabled(always)
            .noundo()
            .interactive(|_| Ok(Box::new(Recover))),
        CommandSpec::new("drawingrecovery", "Drawing Recovery Manager...", run_drawingrecovery)
            .menu(&["File", "Drawing Utilities", "Drawing Recovery Manager..."])
            .params("{action?: list|open|discard (default list), path? (one of the listed files)} → {files: [{path, drawing, modified, bytes}]} | (open: as RECOVER)")
            .enabled(always)
            .noundo(),
    ]
}

/// Drawing Recovery: list the autosave files that apps which stopped left in SAVEFILEPATH, open
/// one as an unsaved copy, or delete one. Only listed files can be opened or deleted.
fn run_drawingrecovery(s: &mut Session, p: &Value) -> Result<Value> {
    let action = str_param(p, "action").unwrap_or("list").to_ascii_lowercase();
    let files = recoverable_files();
    let pick = || {
        let path = str_param(p, "path").ok_or_else(|| bad("drawingrecovery", "`path` is required"))?;
        files.iter().find(|f| f.path == path).cloned().ok_or_else(|| bad("drawingrecovery", format!("{path} is not a recoverable autosave file")))
    };
    match action.as_str() {
        "list" => Ok(json!({ "files": files })),
        "open" => {
            let f = pick()?;
            run_recover(s, &json!({ "path": f.path, "copy": true, "title": f.drawing }))
        }
        "discard" => {
            let f = pick()?;
            crate::consent::check_write(&f.path).map_err(|e| bad("drawingrecovery", e.to_string()))?;
            discard_file(&f.path);
            Ok(json!({ "files": recoverable_files() }))
        }
        other => Err(bad("drawingrecovery", format!("unknown action `{other}` (list, open or discard)"))),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn recoverable_files() -> Vec<crate::autosave::Recoverable> {
    let dir = crate::autosave::savefilepath();
    if dir.is_empty() {
        return Vec::new();
    }
    // This app's own files are never listed: its lock file is fresh.
    crate::autosave::recoverable(std::path::Path::new(&dir), "")
}

#[cfg(target_arch = "wasm32")]
fn recoverable_files() -> Vec<crate::autosave::Recoverable> {
    Vec::new()
}

#[cfg(not(target_arch = "wasm32"))]
fn discard_file(path: &str) {
    crate::autosave::discard(path);
}

#[cfg(target_arch = "wasm32")]
fn discard_file(_: &str) {}

/// Check the current drawing; with `fix`, repair it (one undo step).
fn run_audit(s: &mut Session, p: &Value) -> Result<Value> {
    let fix = bool_or(p, "fix", false);
    let mut copy = (*s.doc()?).clone();
    let issues = crate::audit::repair(&mut copy);
    // A drawing without errors stays the same object: no undo step, no unsaved changes.
    if fix && !issues.is_empty() {
        s.state_mut()?.doc = Arc::new(copy);
    }
    let report = crate::audit::Report { issues, fixed: fix };
    if s.settings.auditctl {
        write_audit_log(s, &report);
    }
    Ok(report.to_json())
}

/// Open a drawing file as tolerantly as the readers allow, then fix what AUDIT finds. The
/// drawing keeps its file name; when anything was fixed it opens with unsaved changes. With
/// `copy` it opens as an unsaved copy without a file name (Drawing Recovery opens autosave files
/// this way), titled `title`.
fn run_recover(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, name, path) = match (str_param(p, "path"), str_param(p, "data")) {
        (Some(path), _) => (read_path("recover", path)?, path.to_string(), Some(path.to_string())),
        (None, Some(data)) => {
            let bytes = file::base64_decode(data).ok_or_else(|| bad("recover", "invalid base64"))?;
            (bytes, str_param(p, "name").unwrap_or("Drawing.dxf").to_string(), None)
        }
        (None, None) => return Err(bad("recover", "`path` or `data` (base64) is required")),
    };
    let hooks = file::io().ok_or_else(|| bad("recover", "file formats are not available in this build"))?;
    let d = (hooks.read)(&bytes, &name).map_err(|e| bad("recover", format!("{name} can't be read: {e}")))?;
    let title = match str_param(p, "title") {
        Some(t) => t.to_string(),
        None => std::path::Path::new(&name).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| name.clone()),
    };
    let copy = bool_or(p, "copy", false);
    let i = s.open_drawing(d, &title, if copy { None } else { path });
    let st = s.state_mut()?;
    let report = crate::audit::fix(Arc::make_mut(&mut st.doc));
    if report.issues.is_empty() {
        // Nothing to fix: the drawing opens unchanged, like OPEN.
        st.doc = st.saved.clone();
    }
    if copy && !st.is_dirty() {
        // An unsaved copy (Drawing Recovery): closing it asks to save it under a name.
        st.saved = Arc::new((*st.doc).clone());
    }
    let uid = st.uid;
    if s.settings.auditctl {
        write_audit_log(s, &report);
    }
    let mut r = report.to_json();
    r["index"] = json!(i);
    r["uid"] = json!(uid);
    Ok(r)
}

#[cfg(not(target_arch = "wasm32"))]
fn write_audit_log(s: &Session, report: &crate::audit::Report) {
    let (path_str, title) = match s.state() {
        Ok(st) => (st.path.clone(), st.title.clone()),
        Err(_) => (None, "Drawing".to_string()),
    };
    // An unsaved drawing has no folder of its own to hold the log.
    let Some(path) = &path_str else { return };
    let adt_path = std::path::PathBuf::from(path).with_extension("adt");
    let content = report.format_adt(&title, path_str.as_deref());
    // Write through an exclusively-created temporary and rename over the companion path, so a
    // planted `.adt` symlink beside the drawing cannot redirect the report onto another file
    // writable by the victim (CWE-59).
    if let Err(e) = crate::atomic_write::write_atomic(&adt_path, content.as_bytes()) {
        log::warn!("AUDITCTL: could not write audit log {}: {e}", adt_path.display());
    }
}

#[cfg(target_arch = "wasm32")]
fn write_audit_log(_: &Session, _: &crate::audit::Report) {}

#[cfg(not(target_arch = "wasm32"))]
fn read_path(cmd: &str, path: &str) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| bad(cmd, format!("{path}: {e}")))
}

#[cfg(target_arch = "wasm32")]
fn read_path(cmd: &str, _path: &str) -> Result<Vec<u8>> {
    Err(bad(cmd, "paths are not available on the web; pass `data`"))
}

const YES_NO: Accept = Accept { point: false, number: false, text: false, select: false, enter: true };

fn echo_result(s: &mut Session, r: &Value) {
    if let Some(m) = r.get("message").and_then(Value::as_str) {
        for l in m.lines() {
            s.echo(l.to_string());
        }
    }
}

/// AUDIT at the command line: "Fix any errors detected? [Yes/No] <N>:".
struct Audit;

impl Interactive for Audit {
    fn name(&self) -> &'static str {
        "AUDIT"
    }

    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Fix any errors detected?", YES_NO).kw(&["Yes", "No"]).default("N")
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let fix = match i {
            Input::Cancel => return Ok(Step::Cancel),
            Input::Enter => false,
            Input::Keyword(k) | Input::Text(k) => match self.prompt(s).match_keyword(&k).as_deref() {
                Some("Yes") => true,
                Some(_) => false,
                None => return Err(bad("audit", "answer Yes or No")),
            },
            _ => return Ok(Step::Continue),
        };
        let r = run_audit(s, &json!({ "fix": fix }))?;
        echo_result(s, &r);
        Ok(Step::Done)
    }
}

/// RECOVER at the command line (no file dialog): the file name is typed.
struct Recover;

impl Interactive for Recover {
    fn name(&self) -> &'static str {
        "RECOVER"
    }

    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Enter name of drawing file to recover", Accept::TEXT)
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Cancel | Input::Enter => Ok(Step::Cancel),
            Input::Text(t) | Input::Keyword(t) => {
                let path = t.trim().trim_matches('"');
                if path.is_empty() {
                    return Ok(Step::Cancel);
                }
                let r = run_recover(s, &json!({ "path": path }))?;
                echo_result(s, &r);
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
}
