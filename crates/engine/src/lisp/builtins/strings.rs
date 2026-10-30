use super::args::{R, arity, int, list_arg, string};
use super::{LispError, OsStr, Value, check_string_len, read, to_text};

// ---------------- strings ----------------

pub(super) fn strcat(a: &[Value]) -> R {
    let parts = a.iter().map(string).collect::<Result<Vec<_>, _>>()?;
    // Check the length before joining: many long arguments could join to more than memory.
    check_string_len(parts.iter().fold(0usize, |n, p| n.saturating_add(p.len())))?;
    Ok(Value::str(&parts.concat()))
}

pub(super) fn strlen(a: &[Value]) -> R {
    let mut n = 0usize;
    for v in a {
        n += string(v)?.chars().count();
    }
    Ok(Value::Int(i32::try_from(n).unwrap_or(i32::MAX)))
}

pub(super) fn substr(a: &[Value]) -> R {
    arity(a, 2, 3)?;
    let s = string(&a[0])?;
    let start = int(&a[1])?;
    if start < 1 {
        return Err(LispError::new(format!("bad argument value: positive {start}")));
    }
    let chars = s.chars().skip(start as usize - 1);
    let out: String = match a.get(2) {
        Some(n) => {
            let n = int(n)?;
            if n < 0 {
                return Err(LispError::new(format!("bad argument value: non-negative {n}")));
            }
            chars.take(n as usize).collect()
        }
        None => chars.collect(),
    };
    Ok(Value::str(&out))
}

pub(super) fn strcase(a: &[Value]) -> R {
    arity(a, 1, 2)?;
    let s = string(&a[0])?;
    let lower = a.get(1).is_some_and(|v| !v.is_nil());
    Ok(Value::str(&if lower { s.to_lowercase() } else { s.to_uppercase() }))
}

pub(super) fn itoa(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::str(&int(&a[0])?.to_string()))
}

/// C's `atoi`: leading blanks, a sign, digits; 0 when there are none.
pub(super) fn atoi(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let s = string(&a[0])?;
    let t = s.trim_start();
    let end = t.char_indices().take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+'))).count();
    Ok(Value::Int(t.get(..end).and_then(|n| n.parse::<i64>().ok()).map(|n| n as i32).unwrap_or(0)))
}

/// C's `atof`: the longest number at the start; 0.0 when there is none.
pub(super) fn atof(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let s = string(&a[0])?;
    // The longest prefix (of the first 64 characters) that reads as a finite number.
    let t: String = s.trim_start().chars().take(64).collect();
    let ends: Vec<usize> = t.char_indices().map(|(i, _)| i).skip(1).chain(std::iter::once(t.len())).collect();
    let v = ends.iter().rev().find_map(|&e| t.get(..e).and_then(|p| p.parse::<f64>().ok()).filter(|v| v.is_finite() && !p_is_word(&t[..e])));
    Ok(Value::Real(v.unwrap_or(0.0)))
}

/// Words Rust reads as numbers that C's `atof` doesn't ("inf", "nan", "infinity").
fn p_is_word(p: &str) -> bool {
    p.chars().any(|c| c.is_ascii_alphabetic() && !matches!(c, 'e' | 'E'))
}

pub(super) fn chr(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let c = u32::try_from(int(&a[0])?).ok().and_then(char::from_u32).map(String::from).unwrap_or_default();
    Ok(Value::str(&c))
}

pub(super) fn ascii(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::Int(string(&a[0])?.chars().next().map(|c| c as i32).unwrap_or(0)))
}

pub(super) fn read_fn(a: &[Value]) -> R {
    arity(a, 0, 1)?;
    match a.first() {
        None | Some(Value::Nil) => Ok(Value::Nil),
        Some(v) => read::read_one(&string(v)?),
    }
}

pub(super) fn vl_prin1_to_string(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::str(&to_text(&a[0], true)))
}

pub(super) fn vl_princ_to_string(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::str(&to_text(&a[0], false)))
}

pub(super) fn vl_symbol_name(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    match &a[0] {
        Value::Sym(s) => Ok(Value::str(s.name())),
        other => Err(LispError::bad_arg("symbolp", other)),
    }
}

pub(super) fn vl_symbolp(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(matches!(a[0], Value::Sym(_))))
}

pub(super) fn vl_string_search(a: &[Value]) -> R {
    arity(a, 2, 3)?;
    let (pat, s) = (string(&a[0])?, string(&a[1])?);
    let start = a.get(2).map(int).transpose()?.unwrap_or(0).max(0) as usize;
    let chars: Vec<char> = s.chars().collect();
    let tail: String = chars.iter().skip(start).collect();
    Ok(tail.find(&*pat).map(|b| Value::Int((start + tail[..b].chars().count()) as i32)).unwrap_or_default())
}

pub(super) fn vl_string_subst(a: &[Value]) -> R {
    arity(a, 3, 4)?;
    let (new, pat, s) = (string(&a[0])?, string(&a[1])?, string(&a[2])?);
    let start = a.get(3).map(int).transpose()?.unwrap_or(0).max(0) as usize;
    let byte_start = s.char_indices().nth(start).map(|(b, _)| b).unwrap_or(s.len());
    let out = match s.get(byte_start..).and_then(|t| t.find(&*pat)) {
        Some(i) if !pat.is_empty() => format!("{}{}{}", &s[..byte_start + i], new, &s[byte_start + i + pat.len()..]),
        _ => s.to_string(),
    };
    Ok(Value::str(&out))
}

fn trim_fn(a: &[Value], left: bool, right: bool) -> R {
    arity(a, 2, 2)?;
    let (set, s) = (string(&a[0])?, string(&a[1])?);
    let is = |c: char| set.contains(c);
    let mut t: &str = &s;
    if left {
        t = t.trim_start_matches(is);
    }
    if right {
        t = t.trim_end_matches(is);
    }
    Ok(Value::str(t))
}

pub(super) fn vl_string_trim(a: &[Value]) -> R {
    trim_fn(a, true, true)
}

pub(super) fn vl_string_left_trim(a: &[Value]) -> R {
    trim_fn(a, true, false)
}

pub(super) fn vl_string_right_trim(a: &[Value]) -> R {
    trim_fn(a, false, true)
}

pub(super) fn vl_string_translate(a: &[Value]) -> R {
    arity(a, 3, 3)?;
    let (from, to, s) = (string(&a[0])?, string(&a[1])?, string(&a[2])?);
    let to: Vec<char> = to.chars().collect();
    let out: String = s.chars().map(|c| from.chars().position(|f| f == c).and_then(|i| to.get(i).or(to.last()).copied()).unwrap_or(c)).collect();
    Ok(Value::str(&out))
}

pub(super) fn vl_string_to_list(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::list(string(&a[0])?.chars().map(|c| Value::Int(c as i32)).collect::<Vec<_>>()))
}

pub(super) fn vl_list_to_string(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let mut s = String::new();
    for v in list_arg(&a[0])? {
        s.extend(u32::try_from(int(&v)?).ok().and_then(char::from_u32));
    }
    Ok(Value::str(&s))
}

pub(super) fn vl_filename_base(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let s = string(&a[0])?;
    Ok(Value::str(std::path::Path::new(&*s.replace('\\', "/")).file_stem().and_then(OsStr::to_str).unwrap_or("")))
}

pub(super) fn vl_filename_extension(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let s = string(&a[0])?;
    Ok(std::path::Path::new(&*s.replace('\\', "/")).extension().and_then(OsStr::to_str).map(|e| Value::str(&format!(".{e}"))).unwrap_or_default())
}

pub(super) fn vl_filename_directory(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let s = string(&a[0])?;
    let cut = s.rfind(['/', '\\']).unwrap_or(0);
    Ok(Value::str(&s[..cut]))
}
