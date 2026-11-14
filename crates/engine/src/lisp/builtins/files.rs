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
                // Each line has a String header and a separately allocated buffer. Charge a
                // conservative 64 bytes per line for both, including the allocator's minimum
                // allocation size, before collecting so many short lines cannot evade the budget.
                let count = t.lines().count();
                charge_bytes(t.len().saturating_add(count.saturating_mul(64)))?;
                // Exact capacity: collecting would grow by doubling and hold unbilled slack.
                f.lines = std::collections::VecDeque::with_capacity(count);
                f.lines.extend(t.lines().map(str::to_string));
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

pub(super) fn vl_file_systime(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let p = string(&a[0])?;
    Ok(file_systime(&p).map_or(Value::Nil, |parts| Value::list(parts.into_iter().map(Value::Int))))
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
    // Opened without blocking and without truncating, so a FIFO with no reader fails at once
    // instead of stalling the interpreter; only a regular file is then truncated and written.
    let mut o = std::fs::OpenOptions::new();
    o.create(true).write(true).append(append);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NONBLOCK);
    }
    if let Ok(mut f) = o.open(path)
        && f.metadata().is_ok_and(|m| m.is_file())
        && (append || f.set_len(0).is_ok())
    {
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

#[cfg(not(target_arch = "wasm32"))]
fn file_systime(path: &str) -> Option<[i32; 8]> {
    use chrono::{Datelike, Timelike};
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let (secs, nanos) = match modified.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => (i64::try_from(d.as_secs()).ok()?, d.subsec_nanos()),
        Err(e) => {
            let d = e.duration();
            let s = i64::try_from(d.as_secs()).ok()?;
            if d.subsec_nanos() == 0 { (-s, 0) } else { ((-s).checked_sub(1)?, 1_000_000_000 - d.subsec_nanos()) }
        }
    };
    let dt = chrono::DateTime::from_timestamp(secs, nanos)?;
    let dow = i32::try_from(dt.weekday().num_days_from_sunday()).ok()?;
    let month = i32::try_from(dt.month()).ok()?;
    let day = i32::try_from(dt.day()).ok()?;
    let hour = i32::try_from(dt.hour()).ok()?;
    let minute = i32::try_from(dt.minute()).ok()?;
    let second = i32::try_from(dt.second()).ok()?;
    let millis = i32::try_from((dt.nanosecond() / 1_000_000).min(999)).ok()?;
    Some([dt.year(), month, dow, day, hour, minute, second, millis])
}

#[cfg(target_arch = "wasm32")]
fn file_systime(_path: &str) -> Option<[i32; 8]> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Writing to a FIFO with no reader returns at once, and a regular file is still replaced.
    #[test]
    fn write_to_a_fifo_without_a_reader_does_not_block() {
        // A fresh directory: `create_dir` fails if the name exists, so no stale pipe or planted symlink is reused.
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("cadcraft-lispfifo-{}-{nanos}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let fifo = dir.join("pipe");
        assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
        let started = std::time::Instant::now();
        write_text_file(fifo.to_str().unwrap(), "x", false);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        let f = dir.join("f.txt");
        std::fs::write(&f, "long old text").unwrap();
        write_text_file(f.to_str().unwrap(), "new", false);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "new");
        write_text_file(f.to_str().unwrap(), "+", true);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "new+");
        std::fs::remove_dir_all(&dir).ok();
    }
}
