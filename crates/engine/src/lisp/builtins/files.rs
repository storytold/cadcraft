use super::args::{R, arity, string};
use super::output::emit;
use super::{Ctx, Flow, LispError, Value, charge_bytes};

// ---------------- files ----------------

/// A file opened by `open`: read files are read whole; written ones are written at `close`.
pub struct LispFile {
    pub path: String,
    pub writing: bool,
    pub append: bool,
    pub lines: std::collections::VecDeque<String>,
    pub text: String,
}

pub(super) fn open(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 3)?;
    let path = string(&a[0])?.to_string();
    let mode = string(&a[1])?.to_ascii_lowercase();
    let (writing, append) = match mode.as_str() {
        "r" => (false, false),
        "w" => (true, false),
        "a" => (true, true),
        _ => return Err(LispError::new(format!("bad argument value: file mode {mode}"))),
    };
    let mut f = LispFile { path: path.clone(), writing, append, lines: Default::default(), text: String::new() };
    if !writing {
        match read_text_file(&path) {
            Some(t) => {
                charge_bytes(t.len())?;
                f.lines = t.lines().map(str::to_string).collect();
            }
            None => return Ok(Flow::Value(Value::Nil)),
        }
    } else if !can_write(&path) {
        return Ok(Flow::Value(Value::Nil));
    }
    let id = ctx.env.next_file;
    ctx.env.next_file = ctx.env.next_file.wrapping_add(1);
    ctx.env.files.insert(id, f);
    Ok(Flow::Value(Value::File(id)))
}

pub(super) fn close(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let Value::File(id) = a[0] else { return Err(LispError::bad_arg("streamp", &a[0])) };
    let Some(f) = ctx.env.files.remove(&id) else { return Err(LispError::bad_arg("streamp", &a[0])) };
    if f.writing {
        write_text_file(&f.path, &f.text, f.append);
    }
    Ok(Flow::Value(Value::Nil))
}

pub(super) fn read_line(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 1)?;
    let Some(Value::File(id)) = a.first() else { return Ok(Flow::Value(Value::Nil)) };
    let line = ctx.env.files.get_mut(id).filter(|f| !f.writing).and_then(|f| f.lines.pop_front());
    Ok(Flow::Value(line.map(|l| Value::str(&l)).unwrap_or_default()))
}

pub(super) fn write_line(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 2)?;
    let s = string(&a[0])?;
    emit(ctx, &format!("{s}\n"), a.get(1))?;
    Ok(Flow::Value(Value::Str(s)))
}

pub(super) fn findfile(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let p = string(&a[0])?;
    Ok(if file_exists(&p) { Value::Str(p) } else { Value::Nil })
}

/// Read a regular text file up to 64 MiB, replacing invalid UTF-8 and returning `None` on read failure.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_text_file(path: impl AsRef<std::path::Path>) -> Option<String> {
    // Read through a byte-limited, regular-file-only reader so a path naming an endless device
    // cannot exhaust memory before the 64 MiB cap is enforced (CWE-400).
    const MAX: usize = 64 * 1024 * 1024;
    let bytes = crate::capped_read::read_capped(path.as_ref(), MAX).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Return `None` on the web, where native filesystem paths are unavailable.
#[cfg(target_arch = "wasm32")]
pub fn read_text_file(_path: impl AsRef<std::path::Path>) -> Option<String> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn can_write(path: &str) -> bool {
    std::path::Path::new(path).parent().is_none_or(|d| d.as_os_str().is_empty() || d.exists())
}

#[cfg(target_arch = "wasm32")]
fn can_write(_path: &str) -> bool {
    false
}

#[cfg(not(target_arch = "wasm32"))]
fn write_text_file(path: &str, text: &str, append: bool) {
    use std::io::Write;
    let f = std::fs::OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(path);
    if let Ok(mut f) = f {
        let _ = f.write_all(text.as_bytes());
    }
}

#[cfg(target_arch = "wasm32")]
fn write_text_file(_path: &str, _text: &str, _append: bool) {}

#[cfg(not(target_arch = "wasm32"))]
fn file_exists(path: &str) -> bool {
    std::path::Path::new(path).is_file()
}

#[cfg(target_arch = "wasm32")]
fn file_exists(_path: &str) -> bool {
    false
}
