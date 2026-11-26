use super::args::{CmdStep, Drawing, RunArgs, Step};
use super::error::{CliError, Result};
use super::open;
use super::output::{Streams, export, export_framed, export_window, model_views, undrawn_message};
use cadcraft_engine::Session;
use cadcraft_engine::doc::Handle;
use cadcraft_mcp::Backend;
use serde_json::{Value, json};

/// A `--cmd` parameter value of exactly `{last}` is filled in with the last object handle an
/// earlier `--cmd` in the same run returned: `mview` answers `{"viewports":["100"]}`, so
/// `viewport.set {"handle":"{last}","scale":1}` changes viewport 100. Any other string is passed
/// through unchanged, and `--script` text is never rewritten.
const LAST_HANDLE: &str = "{last}";

/// Why a `run` step fails.
#[derive(Debug, thiserror::Error)]
pub(crate) enum RunError {
    #[error("{}: {source}", .path.display())]
    ReadScript { path: std::path::PathBuf, source: std::io::Error },
    #[error("--cmd {id}: {source}")]
    CmdLast { id: String, source: NoLastHandle },
    /// Nothing answered at the address: usually CADCraft isn't open, or was started without the port.
    #[error("cannot reach CADCraft at {addr} ({source}): open it with `--control PORT` (or set CADCRAFT_CONTROL_PORT for the app) and try again")]
    NotReachable { addr: String, source: std::io::Error },
    /// A rooted path with no drive, given from a folder on a drive (see [`driveless_root`]).
    #[error(
        "{}: a path with no drive would be taken on the current folder's drive; from WSL, give a relative path, run from a WSL folder, or convert it with `wslpath -w`",
        .0.display()
    )]
    DrivelessPath(std::path::PathBuf),
    /// The running app refused a step, or the connection failed during it.
    #[error("{step}: {message}")]
    Remote { step: String, message: String },
}

/// A `{last}` placeholder with no handle to fill in.
#[derive(Debug, PartialEq, thiserror::Error)]
#[error("`{{last}}` needs an earlier --cmd in this run to have returned a handle")]
pub(crate) struct NoLastHandle;

/// How deep [`result_handles`] walks a command result (results are the engine's own JSON).
const RESULT_DEPTH: usize = 16;

/// Result keys whose values are object references: the engine's result convention for handles.
/// A handle can be a string (`handle`, `viewport`, `insert`…) or a list of them (`handles`,
/// `viewports`, `members`…). Every other field may hold a user-defined name, a message or a
/// count, and is never read as a handle — a group named `100` is a name, not object `100`.
fn handle_field(key: &str) -> bool {
    matches!(
        key,
        "handle"
            | "handles"
            | "viewport"
            | "viewports"
            | "boundary"
            | "clip"
            | "insert"
            | "members"
            | "source"
            | "objects"
            | "entities"
            | "selection"
            | "parts"
            | "refs"
            | "matches"
            | "line"
            | "arc"
            | "blocks"
    )
}

/// Collect the handles of `v`, a value under a handle field: a handle string, an array of them,
/// or objects that carry handle fields of their own (`entities`, `parts`).
fn collect_result_handles(exists: &dyn Fn(Handle) -> bool, v: &Value, depth: usize, out: &mut Vec<Handle>) {
    if depth > RESULT_DEPTH {
        return;
    }
    match v {
        Value::String(t) => {
            if let Some(h) = Handle::parse_hex(t)
                && exists(h)
            {
                out.push(h);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_result_handles(exists, x, depth + 1, out)),
        Value::Object(m) => {
            for (k, x) in m {
                if handle_field(k) {
                    collect_result_handles(exists, x, depth + 1, out);
                } else {
                    find_result_handles(exists, x, depth + 1, out);
                }
            }
        }
        _ => {}
    }
}

/// Find handle fields nested in containers that are not handles themselves (`groups` in
/// `groups.list`, whose members are handles): strings here are names, never references.
fn find_result_handles(exists: &dyn Fn(Handle) -> bool, v: &Value, depth: usize, out: &mut Vec<Handle>) {
    if depth > RESULT_DEPTH {
        return;
    }
    match v {
        Value::Array(a) => a.iter().for_each(|x| find_result_handles(exists, x, depth + 1, out)),
        Value::Object(m) => {
            for (k, x) in m {
                if handle_field(k) {
                    collect_result_handles(exists, x, depth + 1, out);
                } else {
                    find_result_handles(exists, x, depth + 1, out);
                }
            }
        }
        _ => {}
    }
}

/// The object handles a command result mentions, in the order it lists them. A handle-only result
/// (a handle string or an array of them) counts as it is; otherwise only handle fields do.
fn result_handles(exists: &dyn Fn(Handle) -> bool, v: &Value, depth: usize, out: &mut Vec<Handle>) {
    match v {
        Value::String(_) => collect_result_handles(exists, v, depth, out),
        Value::Array(a) if a.iter().all(Value::is_string) => collect_result_handles(exists, v, depth, out),
        Value::Array(_) | Value::Object(_) => find_result_handles(exists, v, depth, out),
        _ => {}
    }
}

/// The last handle `r` returned: what a later `{last}` stands for.
pub(crate) fn returned_handle(s: &Session, r: &Value) -> Option<Handle> {
    last_handle_where(&|h| s.doc().is_ok_and(|d| d.entity(h).is_some()), r)
}

/// The last handle `r` returned, as [`returned_handle`], for a drawing this process cannot see
/// (the running app's): a handle field's value counts when it parses as a handle.
fn returned_handle_remote(r: &Value) -> Option<Handle> {
    last_handle_where(&|_| true, r)
}

/// The last handle in `r` that `exists` accepts.
fn last_handle_where(exists: &dyn Fn(Handle) -> bool, r: &Value) -> Option<Handle> {
    let mut hs = Vec::new();
    result_handles(exists, r, 0, &mut hs);
    hs.last().copied()
}

/// The active drawing's identity ([`cadcraft_engine::DocState::uid`]): a handle is only meaningful
/// in the drawing it came from, and `new`, `open`, `document.switch` and `close` can make another
/// drawing current.
fn active_drawing(s: &Session) -> Option<u64> {
    s.docs.get(s.active).map(|st| st.uid)
}

/// Every `{last}` in `p`, at any depth, replaced by `h`. With no handle to fill in it is an
/// error, never a token reaching the command as a (bad) handle.
pub(crate) fn fill_last(p: &Value, h: Option<Handle>) -> Result<Value, NoLastHandle> {
    match p {
        Value::String(t) if t.as_str() == LAST_HANDLE => match h {
            Some(h) => Ok(Value::String(h.hex())),
            None => Err(NoLastHandle),
        },
        Value::Array(a) => a.iter().map(|x| fill_last(x, h)).collect::<Result<Vec<_>, _>>().map(Value::Array),
        Value::Object(m) => {
            m.iter().map(|(k, x)| Ok((k.clone(), fill_last(x, h)?))).collect::<Result<serde_json::Map<String, Value>, _>>().map(Value::Object)
        }
        other => Ok(other.clone()),
    }
}

/// A step ready to run: script text, or a command.
enum Ready<'a> {
    Script(String),
    Cmd(&'a CmdStep),
}

pub(crate) fn run_cmds(a: &RunArgs, io: &mut Streams) -> Result {
    if let Some(addr) = &a.connect {
        return run_connected(a, addr, io);
    }
    let framing = a.framing.framing();
    let (save, export_to) = (a.save.as_deref(), a.export.as_deref());
    let mut s = Session::empty();
    for d in &a.drawings {
        match d {
            Drawing::File(f) => open(&mut s, f)?,
            Drawing::Sample => {
                s.open_drawing(cadcraft_engine::sample::default_sample(), "Bracket", None);
            }
            Drawing::Metric => {
                s.new_drawing(true);
            }
        }
    }
    let mut steps: Vec<Ready> = Vec::new();
    for step in &a.steps {
        steps.push(match step {
            Step::Script(text) => Ready::Script(text.replace("\\n", "\n")),
            Step::ScriptFile(p) => Ready::Script(std::fs::read_to_string(p).map_err(|source| RunError::ReadScript { path: p.clone(), source })?),
            Step::Cmd(c) => Ready::Cmd(c),
        });
    }
    if s.docs.is_empty() {
        s.new_drawing(false);
    }
    let before = model_views(&s);
    let mut last_handle: Option<Handle> = None;
    for step in steps {
        // A handle belongs to the drawing that returned it: if this step makes another drawing
        // current (`new`, `open`, `document.switch`, `close`, or a script running one of them),
        // the remembered handle is stale. A handle the step itself returns is taken afterwards,
        // so it is a handle of the drawing it made current.
        let drawing = active_drawing(&s);
        match step {
            Ready::Script(body) => {
                s.script(&body)?;
                if active_drawing(&s) != drawing {
                    last_handle = None;
                }
            }
            Ready::Cmd(CmdStep { id, params }) => {
                let params = fill_last(params, last_handle).map_err(|source| RunError::CmdLast { id: id.clone(), source })?;
                let r = s.execute(id, &params)?;
                if active_drawing(&s) != drawing {
                    last_handle = None;
                }
                if let Some(h) = returned_handle(&s, &r) {
                    last_handle = Some(h);
                }
                if !r.is_null() {
                    io.outln(format_args!("{r}"));
                }
            }
        }
    }
    for l in &s.log {
        io.errln(format_args!("{l}"));
    }
    if let Some(p) = save {
        export(&s, p, io)?;
    }
    if let Some(p) = export_to {
        let window = if cadcraft_io::is_image(p) {
            let window = export_window(&s, framing, Some(&before))?;
            if let Some(message) = undrawn_message(s.doc()?, &p.to_string_lossy()) {
                io.errln(format_args!("{message}"));
            }
            window
        } else {
            None
        };
        export_framed(&s, p, window, io)?;
    }
    Ok(())
}

/// Commands that can make another drawing current in the running app. The control channel does not
/// say which drawing is current, so after one of these `{last}` is forgotten rather than checked.
const SWITCHES_DRAWING: &[&str] = &["new", "open", "close", "closeall", "document.switch"];

/// `path` as the app should see it: absolute, since the app does not share this process's folder.
fn app_path(path: &std::path::Path) -> Result<String> {
    let cwd = std::env::current_dir()?;
    if let Some(e) = driveless_root(path, &cwd, cfg!(windows)) {
        return Err(e.into());
    }
    let abs = std::path::absolute(path)?;
    abs.to_str().map(str::to_string).ok_or(CliError::NotUnicode(abs))
}

/// On Windows a path such as `/home/me/plan.dxf` has no drive, so it is taken on the current
/// folder's drive. Run from WSL in a WSL folder (`\\wsl.localhost\<distro>\…`) that is the
/// Linux path meant; from a drive folder it would silently be `C:\home\me\plan.dxf`, so refuse it.
fn driveless_root(path: &std::path::Path, cwd: &std::path::Path, windows: bool) -> Option<RunError> {
    let text = path.to_str()?;
    let rooted = (text.starts_with('/') || text.starts_with('\\')) && !text.starts_with("//") && !text.starts_with("\\\\");
    if !windows || !rooted || in_wsl_share(cwd) {
        return None;
    }
    Some(RunError::DrivelessPath(path.to_path_buf()))
}

/// Whether `dir` is inside a WSL distribution's share (`\\wsl.localhost\…` or `\\wsl$\…`).
fn in_wsl_share(dir: &std::path::Path) -> bool {
    let t = dir.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
    let t = t.strip_prefix("\\\\?\\unc\\").map(|r| format!("\\\\{r}")).unwrap_or(t);
    t.starts_with("\\\\wsl.localhost\\") || t.starts_with("\\\\wsl$\\")
}

/// `run --connect`: the same drawings and steps, sent to the running app's control channel. Every
/// input is read and every path resolved before the first request, so a bad argument changes
/// nothing in the open drawing.
fn run_connected(a: &RunArgs, addr: &str, io: &mut Streams) -> Result {
    let mut opens: Vec<(String, &'static str, Value)> = Vec::new();
    for d in &a.drawings {
        opens.push(match d {
            Drawing::File(f) => {
                let path = app_path(f)?;
                (format!("open {}", f.display()), "app.open", json!({"path": path}))
            }
            Drawing::Metric => ("--metric".into(), "engine.execute", json!({"command": "new", "params": {"metric": true}})),
            // clap refuses `--sample` with `--connect`: the built-in sample lives in this process.
            Drawing::Sample => return Err(RunError::Remote { step: "--sample".into(), message: "headless only".into() }.into()),
        });
    }
    let mut steps: Vec<Ready> = Vec::new();
    for step in &a.steps {
        steps.push(match step {
            Step::Script(text) => Ready::Script(text.replace("\\n", "\n")),
            Step::ScriptFile(p) => Ready::Script(std::fs::read_to_string(p).map_err(|source| RunError::ReadScript { path: p.clone(), source })?),
            Step::Cmd(c) => Ready::Cmd(c),
        });
    }
    let outputs: Vec<String> = [&a.save, &a.export].into_iter().flatten().map(|p| app_path(p)).collect::<Result<_>>()?;

    let mut app = cadcraft_mcp::Remote::connect(addr).map_err(|source| RunError::NotReachable { addr: addr.to_string(), source })?;
    let mut call = |step: &str, method: &str, params: Value| -> Result<Value> {
        app.call(method, params).map_err(|message| RunError::Remote { step: step.to_string(), message }.into())
    };
    for (step, method, params) in opens {
        call(&step, method, params)?;
    }
    let mut last_handle: Option<Handle> = None;
    for step in steps {
        match step {
            Ready::Script(body) => {
                let r = call("--script", "cmdline.script", json!({"text": body}))?;
                for l in r.get("output").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                    io.errln(format_args!("{l}"));
                }
                // A script can open, switch or close drawings, and the app does not say whether it did.
                last_handle = None;
            }
            Ready::Cmd(CmdStep { id, params }) => {
                let params = fill_last(params, last_handle).map_err(|source| RunError::CmdLast { id: id.clone(), source })?;
                let r = call(&format!("--cmd {id}"), "engine.execute", json!({"command": id, "params": params}))?;
                if SWITCHES_DRAWING.contains(&id.to_ascii_lowercase().as_str()) {
                    last_handle = None;
                }
                if let Some(h) = returned_handle_remote(&r) {
                    last_handle = Some(h);
                }
                if !r.is_null() {
                    io.outln(format_args!("{r}"));
                }
            }
        }
    }
    for path in outputs {
        call(&format!("save {path}"), "app.save", json!({"path": path}))?;
        io.errln(format_args!("saved {path} (in the app at {addr})"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::driveless_root;
    use std::path::Path;

    /// From WSL, a Linux path is right in a WSL folder and silently wrong on a drive.
    #[test]
    fn a_driveless_path_is_refused_only_on_windows_outside_a_wsl_folder() {
        let on_c = Path::new(r"C:\Users\me");
        let in_wsl = [r"\\wsl.localhost\Ubuntu\home\me", r"\\wsl$\Ubuntu\home\me", r"\\?\UNC\wsl.localhost\Ubuntu\home"];
        for p in ["/home/me/plan.dxf", r"\home\me\plan.dxf"] {
            assert!(driveless_root(Path::new(p), on_c, true).is_some(), "{p} from C:");
            for w in in_wsl {
                assert!(driveless_root(Path::new(p), Path::new(w), true).is_none(), "{p} from {w}");
            }
            assert!(driveless_root(Path::new(p), on_c, false).is_none(), "{p} is a real path off Windows");
        }
        for p in ["plan.dxf", r"C:\plans\plan.dxf", r"\\server\share\plan.dxf", "//server/share/plan.dxf"] {
            assert!(driveless_root(Path::new(p), on_c, true).is_none(), "{p}");
        }
    }
}
