//! Files and folders (`vl-file-*`, `vl-directory-files`, `vl-mkdir`), environment variables
//! (`getenv`, `setenv`), object snaps (`osnap`), command and symbol names (`getcname`,
//! `snvalid`, `atoms-family`) and small helpers (`boole`, `vl-string-position`,
//! `vl-string-elt`, `vl-string-mismatch`, `read-char`, `write-char`).
//!
//! The file functions answer nil where the file system says no, as AutoLISP does; the web build
//! has no file system, so there they always do.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use cadcraft_geom::Vec2;

use super::LispError;
use super::builtins::{args_err, emit};
use super::eval::{Ctx, Flow};
use super::value::{Sym, Value, charge_bytes, check_string_len};

type R = Result<Value, LispError>;

fn arity(a: &[Value], min: usize, max: usize) -> Result<(), LispError> {
    if a.len() < min {
        return Err(args_err(true));
    }
    if a.len() > max {
        return Err(args_err(false));
    }
    Ok(())
}

fn string(v: &Value) -> Result<String, LispError> {
    match v {
        Value::Str(s) => Ok(s.to_string()),
        other => Err(LispError::bad_arg("stringp", other)),
    }
}

fn int(v: &Value) -> Result<i32, LispError> {
    match v {
        Value::Int(i) => Ok(*i),
        other => Err(LispError::bad_arg("fixnump", other)),
    }
}

/// Most names `vl-directory-files` lists.
const MAX_DIR_ENTRIES: usize = 100_000;

// ---------------- files and folders ----------------

#[cfg(not(target_arch = "wasm32"))]
mod fs {
    use std::path::Path;

    pub fn copy(src: &str, dst: &str, append: bool) -> Option<u64> {
        if !Path::new(src).is_file() {
            return None;
        }
        if append {
            use std::io::Write;
            let data = std::fs::read(src).ok()?;
            let mut f = std::fs::OpenOptions::new().append(true).open(dst).ok()?;
            f.write_all(&data).ok()?;
            Some(data.len() as u64)
        } else if Path::new(dst).exists() {
            // Never over an existing file.
            None
        } else {
            std::fs::copy(src, dst).ok()
        }
    }
    pub fn delete(p: &str) -> bool {
        Path::new(p).is_file() && std::fs::remove_file(p).is_ok()
    }
    pub fn rename(from: &str, to: &str) -> bool {
        Path::new(from).exists() && !Path::new(to).exists() && std::fs::rename(from, to).is_ok()
    }
    pub fn size(p: &str) -> Option<u64> {
        let m = std::fs::metadata(p).ok()?;
        Some(if m.is_dir() { 0 } else { m.len() })
    }
    pub fn modified(p: &str) -> Option<i64> {
        let m = std::fs::metadata(p).ok()?.modified().ok()?;
        let d = m.duration_since(std::time::UNIX_EPOCH).ok()?;
        i64::try_from(d.as_millis()).ok()
    }
    pub fn is_dir(p: &str) -> bool {
        Path::new(p).is_dir()
    }
    pub fn mkdir(p: &str) -> bool {
        !Path::new(p).exists() && std::fs::create_dir(p).is_ok()
    }
    /// (name, is a folder) of every entry, `.` and `..` first, up to `max`.
    pub fn list(dir: &str, max: usize) -> Option<Vec<(String, bool)>> {
        let dir = if dir.is_empty() { "." } else { dir };
        let rd = std::fs::read_dir(dir).ok()?;
        let mut out = vec![(".".to_string(), true), ("..".to_string(), true)];
        for e in rd.flatten().take(max) {
            let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
            out.push((e.file_name().to_string_lossy().into_owned(), is_dir));
        }
        Some(out)
    }
}

#[cfg(target_arch = "wasm32")]
mod fs {
    pub fn copy(_src: &str, _dst: &str, _append: bool) -> Option<u64> {
        None
    }
    pub fn delete(_p: &str) -> bool {
        false
    }
    pub fn rename(_from: &str, _to: &str) -> bool {
        false
    }
    pub fn size(_p: &str) -> Option<u64> {
        None
    }
    pub fn modified(_p: &str) -> Option<i64> {
        None
    }
    pub fn is_dir(_p: &str) -> bool {
        false
    }
    pub fn mkdir(_p: &str) -> bool {
        false
    }
    pub fn list(_dir: &str, _max: usize) -> Option<Vec<(String, bool)>> {
        None
    }
}

fn int_value(n: u64) -> Value {
    Value::Int(i32::try_from(n).unwrap_or(i32::MAX))
}

/// `(vl-file-copy source dest [append])`: the bytes copied, or nil. An existing `dest` is only
/// ever appended to.
pub(super) fn vl_file_copy(a: &[Value]) -> R {
    arity(a, 2, 3)?;
    let append = a.get(2).is_some_and(|v| !v.is_nil());
    Ok(fs::copy(&string(&a[0])?, &string(&a[1])?, append).map(int_value).unwrap_or_default())
}
pub(super) fn vl_file_delete(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(fs::delete(&string(&a[0])?)))
}
pub(super) fn vl_file_rename(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    Ok(Value::bool(fs::rename(&string(&a[0])?, &string(&a[1])?)))
}
/// `(vl-file-size file)`: its size in bytes, 0 for a folder, nil when there is none.
pub(super) fn vl_file_size(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(fs::size(&string(&a[0])?).map(int_value).unwrap_or_default())
}
pub(super) fn vl_file_directory_p(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(fs::is_dir(&string(&a[0])?)))
}
pub(super) fn vl_mkdir(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(fs::mkdir(&string(&a[0])?)))
}

/// `(vl-file-systime file)`: when it was last changed, as
/// `(year month weekday day hours minutes seconds milliseconds)` (weekday 0 is Sunday), in UTC.
pub(super) fn vl_file_systime(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let Some(ms) = fs::modified(&string(&a[0])?) else { return Ok(Value::Nil) };
    Ok(Value::list(systime(ms).into_iter().map(|v| Value::Int(v as i32)).collect::<Vec<_>>()))
}

/// Milliseconds since 1970 as (year month weekday day hours minutes seconds ms), UTC.
fn systime(ms: i64) -> [i64; 8] {
    let (days, rest) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let weekday = (days + 4).rem_euclid(7);
    [year, month, weekday, day, rest / 3_600_000, rest / 60_000 % 60, rest / 1000 % 60, rest % 1000]
}

/// `(vl-directory-files [directory pattern what])`: the names in a folder matching `pattern`
/// (`*` and `?`, case-insensitive); `what` -1 lists folders only, 1 files only, nil or 0 both.
/// Sorted, so a routine sees the same order on every system.
pub(super) fn vl_directory_files(a: &[Value]) -> R {
    arity(a, 0, 3)?;
    let dir = match a.first() {
        None | Some(Value::Nil) => String::new(),
        Some(v) => string(v)?,
    };
    let pattern = match a.get(1) {
        None | Some(Value::Nil) => "*".to_string(),
        Some(v) => string(v)?,
    };
    let what = match a.get(2) {
        None | Some(Value::Nil) => 0,
        Some(v) => int(v)?,
    };
    let Some(entries) = fs::list(&dir, MAX_DIR_ENTRIES) else { return Ok(Value::Nil) };
    let pattern = if pattern == "*.*" { "*".to_string() } else { pattern };
    let mut names: Vec<String> = entries
        .into_iter()
        .filter(|(_, is_dir)| match what {
            -1 => *is_dir,
            1 => !*is_dir,
            _ => true,
        })
        .map(|(n, _)| n)
        .filter(|n| glob(&n.to_lowercase(), &pattern.to_lowercase()))
        .collect();
    names.sort();
    let mut out = Vec::with_capacity(names.len());
    for n in names {
        charge_bytes(n.len())?;
        out.push(Value::str(&n));
    }
    Ok(if out.is_empty() { Value::Nil } else { Value::list(out) })
}

/// `*` and `?` file-name matching.
fn glob(text: &str, pattern: &str) -> bool {
    let (t, p): (Vec<char>, Vec<char>) = (text.chars().collect(), pattern.chars().collect());
    let (mut ti, mut pi) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        match p.get(pi) {
            Some('*') => {
                star = Some((pi, ti));
                pi += 1;
            }
            Some(c) if *c == '?' || Some(c) == t.get(ti) => {
                pi += 1;
                ti += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    pi = sp + 1;
                    ti = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    p.get(pi..).is_some_and(|rest| rest.iter().all(|c| *c == '*'))
}

// ---------------- environment ----------------

/// Variables `setenv` set this session, by upper-case name. AutoCAD keeps them in its own
/// settings; here they live as long as the program.
fn env_table() -> &'static Mutex<HashMap<String, String>> {
    static T: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Most variables `setenv` keeps.
const MAX_ENV_VARS: usize = 4096;

/// `(getenv name)`: what `setenv` set, else the operating system's variable, else nil.
pub(super) fn getenv(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let name = string(&a[0])?;
    let set = env_table().lock().unwrap_or_else(PoisonError::into_inner).get(&name.to_ascii_uppercase()).cloned();
    Ok(set.or_else(|| std::env::var(&name).ok()).map(|v| Value::str(&v)).unwrap_or_default())
}

/// `(setenv name value)`: `value`.
pub(super) fn setenv(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    let (name, value) = (string(&a[0])?, string(&a[1])?);
    if name.is_empty() {
        return Err(LispError::bad_arg("variable name", &a[0]));
    }
    check_string_len(name.len().saturating_add(value.len()))?;
    let mut t = env_table().lock().unwrap_or_else(PoisonError::into_inner);
    let key = name.to_ascii_uppercase();
    if t.len() >= MAX_ENV_VARS && !t.contains_key(&key) {
        return Err(LispError::new("too many environment variables"));
    }
    t.insert(key, value);
    Ok(a[1].clone())
}

// ---------------- object snaps ----------------

/// `(osnap point modes)`: the snap point the modes ("_end,_mid", "cen"…) find within the
/// aperture of `point`, or nil.
pub(super) fn osnap(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let p = a[0].as_point().ok_or_else(|| LispError::bad_arg("2D/3D point", &a[0]))?;
    let modes = string(&a[1])?;
    let mut bits = 0u32;
    for m in modes.split(',').map(str::trim).filter(|m| !m.is_empty()) {
        match crate::pointmod::parse_snap(m) {
            Some(b) => bits |= b,
            None => return Ok(Flow::Value(Value::Nil)),
        }
    }
    if bits == 0 {
        return Ok(Flow::Value(Value::Nil));
    }
    let aperture = ctx.s.settings.aperture.max(1.0) * ctx.s.pixel_size();
    let Ok(space) = ctx.s.state().map(|st| st.edit_space()) else { return Ok(Flow::Value(Value::Nil)) };
    let Ok(d) = ctx.s.doc() else { return Ok(Flow::Value(Value::Nil)) };
    let hit = crate::snap::osnap(d, &space, Vec2::new(p[0], p[1]), aperture, bits, None, false);
    let two_d = a[0].iter().count() == 2;
    Ok(Flow::Value(match hit {
        Some(h) if two_d => Value::point(&[h.point.x, h.point.y]),
        Some(h) => Value::point(&[h.point.x, h.point.y, p[2]]),
        None => Value::Nil,
    }))
}

// ---------------- names ----------------

/// `(getcname name)`: a command's other name, as AutoLISP's English and global names pair them:
/// `"line"` gives `"_LINE"`, `"_line"` gives `"LINE"`; nil for a name no command has.
pub(super) fn getcname(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let name = string(&a[0])?;
    let (global, bare) = match name.strip_prefix('_') {
        Some(b) => (true, b),
        None => (false, name.as_str()),
    };
    let bare = bare.trim_start_matches(['.', '+']);
    if bare.is_empty() || crate::cmd::find_command(bare).is_none() {
        return Ok(Value::Nil);
    }
    let up = bare.to_ascii_uppercase();
    Ok(Value::str(&if global { up } else { format!("_{up}") }))
}

/// `(snvalid name [flag])`: whether `name` may name a symbol table entry (a layer, a style…).
/// With flag 1 a `|` is allowed inside the name, as in an xref-dependent one.
pub(super) fn snvalid(a: &[Value]) -> R {
    arity(a, 1, 2)?;
    let name = string(&a[0])?;
    let bar_ok = a.get(1).is_some_and(|v| matches!(v, Value::Int(1)));
    let ok = !name.is_empty()
        && name.chars().count() <= 255
        && !name.starts_with(' ')
        && !name.ends_with(' ')
        && name.chars().all(|c| !c.is_control() && !"<>/\\\":;?*,=`".contains(c))
        && (if bar_ok { !name.starts_with('|') && !name.ends_with('|') } else { !name.contains('|') });
    Ok(Value::bool(ok))
}

/// `(atoms-family format [names])`: the symbols with a value (format 0: symbols, 1: names). With
/// `names`, each name that has one, nil for each that hasn't.
pub(super) fn atoms_family(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 2)?;
    let as_names = match int(&a[0])? {
        0 => false,
        1 => true,
        _ => return Err(LispError::bad_arg("format", &a[0])),
    };
    let show = |s: &Sym| if as_names { Value::str(s.name()) } else { Value::Sym(s.clone()) };
    let out = match a.get(1) {
        None | Some(Value::Nil) => {
            let mut syms: Vec<&Sym> = ctx.env.globals.keys().collect();
            syms.sort_by(|x, y| x.name().cmp(y.name()));
            syms.into_iter().map(show).collect::<Vec<_>>()
        }
        Some(list) => {
            let mut out = Vec::new();
            for v in list.iter() {
                let s = Sym::new(&string(&v)?.to_ascii_uppercase());
                out.push(if ctx.env.globals.contains_key(&s) { show(&s) } else { Value::Nil });
            }
            out
        }
    };
    Ok(Flow::Value(Value::list(out)))
}

// ---------------- numbers and strings ----------------

/// `(boole op int1 int2 …)`: a bitwise function given as its truth table: bit 1 where both
/// are set, 2 where only the first is, 4 where only the second is, 8 where neither is (1 AND,
/// 6 XOR, 7 OR, 8 NOR).
pub(super) fn boole(a: &[Value]) -> R {
    arity(a, 2, usize::MAX)?;
    let op = int(&a[0])?;
    let mut acc = int(&a[1])?;
    for v in &a[2..] {
        let x = int(v)?;
        let mut r = 0i32;
        for (bit, set) in [(1, acc & x), (2, acc & !x), (4, !acc & x), (8, !acc & !x)] {
            if op & bit != 0 {
                r |= set;
            }
        }
        acc = r;
    }
    Ok(Value::Int(acc))
}

/// `(vl-string-position code string [start [from-end]])`: where character `code` is, or nil.
pub(super) fn vl_string_position(a: &[Value]) -> R {
    arity(a, 2, 4)?;
    let code = int(&a[0])?;
    let s: Vec<char> = string(&a[1])?.chars().collect();
    let start = match a.get(2) {
        None | Some(Value::Nil) => 0,
        Some(v) => usize::try_from(int(v)?).map_err(|_| LispError::bad_arg("fixnump", v))?,
    };
    let from_end = a.get(3).is_some_and(|v| !v.is_nil());
    let Some(want) = u32::try_from(code).ok().and_then(char::from_u32) else { return Ok(Value::Nil) };
    let found = if from_end {
        // From the end, looking no further back than `start`.
        s.iter().enumerate().rev().take_while(|(i, _)| *i >= start).find(|(_, c)| **c == want).map(|(i, _)| i)
    } else {
        s.iter().enumerate().skip(start).find(|(_, c)| **c == want).map(|(i, _)| i)
    };
    Ok(found.map(|i| Value::Int(i32::try_from(i).unwrap_or(i32::MAX))).unwrap_or_default())
}

/// `(vl-string-elt string position)`: the code of the character at `position` (from 0).
pub(super) fn vl_string_elt(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    let s = string(&a[0])?;
    let i = int(&a[1])?;
    let c = usize::try_from(i).ok().and_then(|i| s.chars().nth(i));
    match c {
        Some(c) => Ok(Value::Int(i32::try_from(u32::from(c)).unwrap_or(0))),
        None => Err(LispError::new(format!("bad argument value: string position out of range {i}"))),
    }
}

/// `(vl-string-mismatch str1 str2 [pos1 pos2 ignore-case])`: how many characters, from `pos1`
/// and `pos2`, the two have in common.
pub(super) fn vl_string_mismatch(a: &[Value]) -> R {
    arity(a, 2, 5)?;
    let (s1, s2) = (string(&a[0])?, string(&a[1])?);
    let pos = |v: Option<&Value>| -> Result<usize, LispError> {
        match v {
            None | Some(Value::Nil) => Ok(0),
            Some(v) => usize::try_from(int(v)?).map_err(|_| LispError::bad_arg("fixnump", v)),
        }
    };
    let (p1, p2) = (pos(a.get(2))?, pos(a.get(3))?);
    let fold = a.get(4).is_some_and(|v| !v.is_nil());
    let same = |x: char, y: char| if fold { x.to_lowercase().eq(y.to_lowercase()) } else { x == y };
    let n = s1.chars().skip(p1).zip(s2.chars().skip(p2)).take_while(|(x, y)| same(*x, *y)).count();
    Ok(Value::Int(i32::try_from(n).unwrap_or(i32::MAX)))
}

/// `(read-char [file])`: the next character's code from a file opened for reading (10 at each
/// line end), nil at its end or without a file.
pub(super) fn read_char(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 1)?;
    let Some(Value::File(id)) = a.first() else { return Ok(Flow::Value(Value::Nil)) };
    let Some(f) = ctx.env.files.get_mut(id).filter(|f| !f.writing) else { return Err(LispError::bad_arg("streamp", &a[0])) };
    let Some(line) = f.lines.front_mut() else { return Ok(Flow::Value(Value::Nil)) };
    if line.is_empty() {
        f.lines.pop_front();
        return Ok(Flow::Value(Value::Int(10)));
    }
    let c = line.remove(0);
    Ok(Flow::Value(Value::Int(i32::try_from(u32::from(c)).unwrap_or(0))))
}

/// `(write-char code [file])`: writes one character; `code`.
pub(super) fn write_char(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 2)?;
    let code = int(&a[0])?;
    let c = u32::try_from(code).ok().and_then(char::from_u32).ok_or_else(|| LispError::bad_arg("character code", &a[0]))?;
    emit(ctx, &c.to_string(), a.get(1))?;
    Ok(Flow::Value(Value::Int(code)))
}

pub(super) fn t(_a: &[Value]) -> R {
    Ok(Value::t())
}
pub(super) fn nil(_a: &[Value]) -> R {
    Ok(Value::Nil)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systime_turns_milliseconds_into_a_utc_date() {
        // 2026-10-16 09:05:07.250 UTC, a Friday.
        let ms = 1_792_141_507_250;
        assert_eq!(systime(ms), [2026, 10, 5, 16, 9, 5, 7, 250]);
        assert_eq!(systime(0), [1970, 1, 4, 1, 0, 0, 0, 0]);
        // Before 1970 and on a leap day.
        assert_eq!(systime(-86_400_000), [1969, 12, 3, 31, 0, 0, 0, 0]);
        assert_eq!(systime(951_782_400_000), [2000, 2, 2, 29, 0, 0, 0, 0]);
    }

    #[test]
    fn glob_matches_stars_and_question_marks() {
        assert!(glob("plan.lsp", "*.lsp"));
        assert!(glob("plan.lsp", "p?an.*"));
        assert!(!glob("plan.dwg", "*.lsp"));
        assert!(glob("a", "*"));
        assert!(glob("", "*"));
        assert!(!glob("ab", "a"));
        assert!(glob("abcabc", "*bc"));
    }
}
