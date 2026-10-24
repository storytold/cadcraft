//! The built-in functions: special forms, numbers, lists, strings, geometry, output, files and
//! the functions that call functions (`apply`, `mapcar`, `eval`, `load`, `vl-sort`…). Drawing
//! access (`command`, `entget`, `ssget`, `getvar`…) is in `drawing.rs`, user input in `input.rs`.
//!
//! Errors use AutoLISP's wording (`bad argument type: numberp: nil`, `too few arguments`,
//! `divide by zero`), since routines and their `*error*` handlers match on it.

use std::cmp::Ordering;
use std::ffi::OsStr;
use std::sync::Arc;

use super::eval::{Ctx, Flow, Special};
use super::value::{Builtin, BuiltinKind, Sym, Value, charge_bytes, check_string_len, eq, equal, real_text, to_text};
use super::{Env, LispError, drawing, input, read};

type R = Result<Value, LispError>;

/// `too few arguments` / `too many arguments`.
pub fn args_err(few: bool) -> LispError {
    LispError::new(if few { "too few arguments" } else { "too many arguments" })
}

fn arity(a: &[Value], min: usize, max: usize) -> Result<(), LispError> {
    if a.len() < min {
        return Err(args_err(true));
    }
    if a.len() > max {
        return Err(args_err(false));
    }
    Ok(())
}

fn num(v: &Value) -> Result<f64, LispError> {
    v.as_number().ok_or_else(|| LispError::bad_arg("numberp", v))
}

fn int(v: &Value) -> Result<i32, LispError> {
    match v {
        Value::Int(i) => Ok(*i),
        other => Err(LispError::bad_arg("fixnump", other)),
    }
}

fn string(v: &Value) -> Result<Arc<str>, LispError> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        other => Err(LispError::bad_arg("stringp", other)),
    }
}

fn list_arg(v: &Value) -> Result<Vec<Value>, LispError> {
    if v.is_list() { Ok(v.to_vec()) } else { Err(LispError::bad_arg("listp", v)) }
}

fn point(v: &Value) -> Result<[f64; 3], LispError> {
    v.as_point().ok_or_else(|| LispError::bad_arg("2D/3D point", v))
}

fn real_or_int(r: f64, all_int: bool) -> Value {
    if all_int { Value::Int(r as i32) } else { Value::Real(r) }
}

// ---------------- numbers ----------------

/// Integer arithmetic wraps at 32 bits, as AutoLISP's does; any real makes the result real.
fn arith(a: &[Value], op: char) -> R {
    let all_int = a.iter().all(|v| matches!(v, Value::Int(_)));
    let mut it = a.iter();
    let Some(first) = it.next() else {
        return Ok(Value::Int(if op == '*' { 1 } else { 0 }));
    };
    if all_int {
        let mut acc = int(first)?;
        if a.len() == 1 {
            return Ok(Value::Int(match op {
                '-' => acc.wrapping_neg(),
                _ => acc,
            }));
        }
        for v in it {
            let x = int(v)?;
            acc = match op {
                '+' => acc.wrapping_add(x),
                '-' => acc.wrapping_sub(x),
                '*' => acc.wrapping_mul(x),
                _ => {
                    if x == 0 {
                        return Err(LispError::new("divide by zero"));
                    }
                    acc.wrapping_div(x)
                }
            };
        }
        return Ok(Value::Int(acc));
    }
    let mut acc = num(first)?;
    if a.len() == 1 {
        return Ok(Value::Real(if op == '-' { -acc } else { acc }));
    }
    for v in it {
        let x = num(v)?;
        acc = match op {
            '+' => acc + x,
            '-' => acc - x,
            '*' => acc * x,
            _ => {
                if x == 0.0 {
                    return Err(LispError::new("divide by zero"));
                }
                acc / x
            }
        };
    }
    Ok(Value::Real(acc))
}

fn add(a: &[Value]) -> R {
    arith(a, '+')
}
fn sub(a: &[Value]) -> R {
    arith(a, '-')
}
fn mul(a: &[Value]) -> R {
    arith(a, '*')
}
fn div(a: &[Value]) -> R {
    arity(a, 1, usize::MAX)?;
    arith(a, '/')
}

fn rem(a: &[Value]) -> R {
    arity(a, 1, usize::MAX)?;
    if a.iter().all(|v| matches!(v, Value::Int(_))) {
        let mut it = a.iter();
        let mut acc = int(it.next().unwrap_or(&Value::Int(0)))?;
        for v in it {
            let x = int(v)?;
            if x == 0 {
                return Err(LispError::new("divide by zero"));
            }
            acc = acc.wrapping_rem(x);
        }
        return Ok(Value::Int(acc));
    }
    let mut it = a.iter();
    let mut acc = num(it.next().unwrap_or(&Value::Int(0)))?;
    for v in it {
        let x = num(v)?;
        if x == 0.0 {
            return Err(LispError::new("divide by zero"));
        }
        acc %= x;
    }
    Ok(Value::Real(acc))
}

fn one_plus(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    arith(&[a[0].clone(), Value::Int(1)], '+')
}
fn one_minus(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    arith(&[a[0].clone(), Value::Int(1)], '-')
}

fn abs(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(match &a[0] {
        Value::Int(i) => Value::Int(i.wrapping_abs()),
        v => Value::Real(num(v)?.abs()),
    })
}

fn min_max(a: &[Value], max: bool) -> R {
    arity(a, 1, usize::MAX)?;
    let all_int = a.iter().all(|v| matches!(v, Value::Int(_)));
    let mut best = num(&a[0])?;
    for v in &a[1..] {
        let x = num(v)?;
        if (max && x > best) || (!max && x < best) {
            best = x;
        }
    }
    Ok(real_or_int(best, all_int))
}
fn min(a: &[Value]) -> R {
    min_max(a, false)
}
fn max(a: &[Value]) -> R {
    min_max(a, true)
}

fn unary(a: &[Value], f: fn(f64) -> f64) -> R {
    arity(a, 1, 1)?;
    let r = f(num(&a[0])?);
    if r.is_finite() { Ok(Value::Real(r)) } else { Err(LispError::new("function undefined for argument")) }
}
fn sqrt(a: &[Value]) -> R {
    if num(a.first().unwrap_or(&Value::Nil))? < 0.0 {
        return Err(LispError::new("function undefined for argument"));
    }
    unary(a, f64::sqrt)
}
fn exp(a: &[Value]) -> R {
    unary(a, f64::exp)
}
fn log(a: &[Value]) -> R {
    if num(a.first().unwrap_or(&Value::Nil))? <= 0.0 {
        return Err(LispError::new("function undefined for argument"));
    }
    unary(a, f64::ln)
}
fn sin(a: &[Value]) -> R {
    unary(a, f64::sin)
}
fn cos(a: &[Value]) -> R {
    unary(a, f64::cos)
}
fn atan(a: &[Value]) -> R {
    arity(a, 1, 2)?;
    let y = num(&a[0])?;
    Ok(Value::Real(match a.get(1) {
        Some(x) => y.atan2(num(x)?),
        None => y.atan(),
    }))
}

fn expt(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    match (&a[0], &a[1]) {
        (Value::Int(b), Value::Int(e)) if *e >= 0 => Ok(Value::Int(b.wrapping_pow(u32::try_from(*e).unwrap_or(0)))),
        (b, e) => {
            let r = num(b)?.powf(num(e)?);
            if r.is_finite() { Ok(Value::Real(r)) } else { Err(LispError::new("function undefined for argument")) }
        }
    }
}

fn fix(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(match &a[0] {
        Value::Int(i) => Value::Int(*i),
        v => {
            let r = num(v)?.trunc();
            if r.is_finite() && r >= f64::from(i32::MIN) && r <= f64::from(i32::MAX) { Value::Int(r as i32) } else { Value::Real(r) }
        }
    })
}

fn float(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::Real(num(&a[0])?))
}

fn gcd(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    let (mut x, mut y) = (i64::from(int(&a[0])?).abs(), i64::from(int(&a[1])?).abs());
    if x == 0 && y == 0 {
        return Err(LispError::new("bad argument value: 0"));
    }
    while y != 0 {
        let t = x % y;
        x = y;
        y = t;
    }
    Ok(Value::Int(i32::try_from(x).unwrap_or(i32::MAX)))
}

fn logand(a: &[Value]) -> R {
    a.iter().try_fold(-1i32, |acc, v| Ok(acc & int(v)?)).map(|r| Value::Int(if a.is_empty() { 0 } else { r }))
}
fn logior(a: &[Value]) -> R {
    a.iter().try_fold(0i32, |acc, v| Ok(acc | int(v)?)).map(Value::Int)
}
fn lsh(a: &[Value]) -> R {
    arity(a, 0, 2)?;
    let x = a.first().map(int).transpose()?.unwrap_or(0);
    let n = a.get(1).map(int).transpose()?.unwrap_or(0);
    let r = if n >= 32 || n <= -32 {
        0
    } else if n >= 0 {
        x.wrapping_shl(n as u32)
    } else {
        ((x as u32) >> ((-n) as u32)) as i32
    };
    Ok(Value::Int(r))
}
fn lognot(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::Int(!int(&a[0])?))
}

/// Numbers compare by value; strings by text.
fn compare(a: &Value, b: &Value) -> Result<std::cmp::Ordering, LispError> {
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => Ok(x.cmp(y)),
        _ => num(a)?.partial_cmp(&num(b)?).ok_or_else(|| LispError::new("bad argument value")),
    }
}

fn chain(a: &[Value], ok: fn(std::cmp::Ordering) -> bool) -> R {
    arity(a, 1, usize::MAX)?;
    for w in a.windows(2) {
        if let [x, y] = w
            && !ok(compare(x, y)?)
        {
            return Ok(Value::Nil);
        }
    }
    Ok(Value::t())
}
/// `=` and `/=`: numbers and strings compare as [`compare`] does, and any other atom compares by
/// what it is, as `eq` does. AutoLISP answers `(= (type x) 'LIST)` -- the idiom for a type test,
/// since `type` returns a symbol -- where coercing that symbol to a number failed the whole
/// routine with "bad argument type" instead (#1597). A mismatch is `nil`, not an error, so
/// `(= 1 "1")` and `(= '(1) '(1))` answer too. The ordering comparisons stay numbers and strings
/// only, as AutoLISP has them.
fn eq_chain(a: &[Value], want_equal: bool) -> R {
    arity(a, 1, usize::MAX)?;
    for w in a.windows(2) {
        let [x, y] = w else { continue };
        let same = match (x, y) {
            (Value::Str(_), Value::Str(_)) => compare(x, y)?.is_eq(),
            _ if x.as_number().is_some() && y.as_number().is_some() => compare(x, y)?.is_eq(),
            _ => eq(x, y),
        };
        if same != want_equal {
            return Ok(Value::Nil);
        }
    }
    Ok(Value::t())
}
fn num_eq(a: &[Value]) -> R {
    eq_chain(a, true)
}
fn num_ne(a: &[Value]) -> R {
    eq_chain(a, false)
}
fn lt(a: &[Value]) -> R {
    chain(a, Ordering::is_lt)
}
fn gt(a: &[Value]) -> R {
    chain(a, Ordering::is_gt)
}
fn le(a: &[Value]) -> R {
    chain(a, Ordering::is_le)
}
fn ge(a: &[Value]) -> R {
    chain(a, Ordering::is_ge)
}

fn zerop(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(num(&a[0])? == 0.0))
}
fn minusp(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(num(&a[0])? < 0.0))
}
fn numberp(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(a[0].as_number().is_some()))
}

// ---------------- predicates and lists ----------------

fn atom(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(!matches!(a[0], Value::Cons(_))))
}
fn listp(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(matches!(a[0], Value::Cons(_) | Value::Nil)))
}
fn consp(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(matches!(a[0], Value::Cons(_))))
}
fn null(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(a[0].is_nil()))
}
fn type_of(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(a[0].type_name().map(Value::sym).unwrap_or_default())
}
fn eq_fn(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    Ok(Value::bool(eq(&a[0], &a[1])))
}
fn equal_fn(a: &[Value]) -> R {
    arity(a, 2, 3)?;
    let fuzz = a.get(2).map(num).transpose()?.unwrap_or(0.0);
    Ok(Value::bool(equal(&a[0], &a[1], fuzz)))
}

fn cxr(a: &[Value], path: &str) -> R {
    arity(a, 1, 1)?;
    let mut v = a[0].clone();
    // The path is read right to left: cadr = car of cdr.
    for c in path.chars().rev() {
        if !matches!(v, Value::Cons(_) | Value::Nil) {
            return Err(LispError::bad_arg("consp", &v));
        }
        v = if c == 'a' { v.car() } else { v.cdr() };
    }
    Ok(v)
}

macro_rules! cxrs {
    ($($name:ident $path:literal),*) => {
        $(fn $name(a: &[Value]) -> R { cxr(a, $path) })*
    };
}
cxrs!(car "a", cdr "d", caar "aa", cadr "ad", cdar "da", cddr "dd", caaar "aaa", caadr "aad", cadar "ada", caddr "add",
      cdaar "daa", cdadr "dad", cddar "dda", cdddr "ddd", cadddr "addd", cddddr "dddd");

fn cons(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    Ok(Value::cons(a[0].clone(), a[1].clone()))
}
fn list(a: &[Value]) -> R {
    Ok(Value::list(a.to_vec()))
}
fn append(a: &[Value]) -> R {
    let mut items = Vec::new();
    for v in a {
        items.extend(list_arg(v)?);
    }
    Ok(Value::list(items))
}
fn reverse(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let mut v = list_arg(&a[0])?;
    v.reverse();
    Ok(Value::list(v))
}
fn length(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::Int(i32::try_from(list_arg(&a[0])?.len()).unwrap_or(i32::MAX)))
}
fn nth(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    let n = int(&a[0])?;
    if n < 0 {
        return Ok(Value::Nil);
    }
    Ok(a[1].iter().nth(n as usize).unwrap_or_default())
}
fn last(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(list_arg(&a[0])?.pop().unwrap_or_default())
}
fn member(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    let mut v = a[1].clone();
    while let Value::Cons(c) = &v {
        if equal(&c.car, &a[0], 0.0) {
            return Ok(v);
        }
        let next = c.cdr.clone();
        v = next;
    }
    Ok(Value::Nil)
}
fn assoc(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    Ok(a[1].iter().find(|item| matches!(item, Value::Cons(_)) && equal(&item.car(), &a[0], 0.0)).unwrap_or_default())
}
fn subst(a: &[Value]) -> R {
    arity(a, 3, 3)?;
    let (new, old) = (&a[0], &a[1]);
    if !a[2].is_list() {
        return Ok(if equal(&a[2], old, 0.0) { new.clone() } else { a[2].clone() });
    }
    Ok(Value::list(a[2].iter().map(|v| if equal(&v, old, 0.0) { new.clone() } else { v }).collect::<Vec<_>>()))
}
fn vl_position(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    Ok(a[1].iter().position(|v| equal(&v, &a[0], 0.0)).map(|i| Value::Int(i32::try_from(i).unwrap_or(i32::MAX))).unwrap_or_default())
}
fn vl_remove(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    Ok(Value::list(list_arg(&a[1])?.into_iter().filter(|v| !equal(v, &a[0], 0.0)).collect::<Vec<_>>()))
}
fn vl_list_length(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(if a[0].is_list() { Value::Int(i32::try_from(a[0].to_vec().len()).unwrap_or(i32::MAX)) } else { Value::Nil })
}
fn acad_strlsort(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let mut v = list_arg(&a[0])?.iter().map(string).collect::<Result<Vec<_>, _>>()?;
    v.sort_by_key(|s| s.to_lowercase());
    Ok(Value::list(v.into_iter().map(Value::Str).collect::<Vec<_>>()))
}

// ---------------- strings ----------------

fn strcat(a: &[Value]) -> R {
    let parts = a.iter().map(string).collect::<Result<Vec<_>, _>>()?;
    // Check the length before joining: many long arguments could join to more than memory.
    check_string_len(parts.iter().fold(0usize, |n, p| n.saturating_add(p.len())))?;
    Ok(Value::str(&parts.concat()))
}
fn strlen(a: &[Value]) -> R {
    let mut n = 0usize;
    for v in a {
        n += string(v)?.chars().count();
    }
    Ok(Value::Int(i32::try_from(n).unwrap_or(i32::MAX)))
}
fn substr(a: &[Value]) -> R {
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
fn strcase(a: &[Value]) -> R {
    arity(a, 1, 2)?;
    let s = string(&a[0])?;
    let lower = a.get(1).is_some_and(|v| !v.is_nil());
    Ok(Value::str(&if lower { s.to_lowercase() } else { s.to_uppercase() }))
}
fn itoa(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::str(&int(&a[0])?.to_string()))
}
/// C's `atoi`: leading blanks, a sign, digits; 0 when there are none.
fn atoi(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let s = string(&a[0])?;
    let t = s.trim_start();
    let end = t.char_indices().take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+'))).count();
    Ok(Value::Int(t.get(..end).and_then(|n| n.parse::<i64>().ok()).map(|n| n as i32).unwrap_or(0)))
}
/// C's `atof`: the longest number at the start; 0.0 when there is none.
fn atof(a: &[Value]) -> R {
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

fn chr(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let c = u32::try_from(int(&a[0])?).ok().and_then(char::from_u32).map(String::from).unwrap_or_default();
    Ok(Value::str(&c))
}
fn ascii(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::Int(string(&a[0])?.chars().next().map(|c| c as i32).unwrap_or(0)))
}
fn read_fn(a: &[Value]) -> R {
    arity(a, 0, 1)?;
    match a.first() {
        None | Some(Value::Nil) => Ok(Value::Nil),
        Some(v) => read::read_one(&string(v)?),
    }
}
fn vl_prin1_to_string(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::str(&to_text(&a[0], true)))
}
fn vl_princ_to_string(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::str(&to_text(&a[0], false)))
}
fn vl_symbol_name(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    match &a[0] {
        Value::Sym(s) => Ok(Value::str(s.name())),
        other => Err(LispError::bad_arg("symbolp", other)),
    }
}
fn vl_symbolp(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(matches!(a[0], Value::Sym(_))))
}
fn vl_string_search(a: &[Value]) -> R {
    arity(a, 2, 3)?;
    let (pat, s) = (string(&a[0])?, string(&a[1])?);
    let start = a.get(2).map(int).transpose()?.unwrap_or(0).max(0) as usize;
    let chars: Vec<char> = s.chars().collect();
    let tail: String = chars.iter().skip(start).collect();
    Ok(tail.find(&*pat).map(|b| Value::Int((start + tail[..b].chars().count()) as i32)).unwrap_or_default())
}
fn vl_string_subst(a: &[Value]) -> R {
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
fn vl_string_trim(a: &[Value]) -> R {
    trim_fn(a, true, true)
}
fn vl_string_left_trim(a: &[Value]) -> R {
    trim_fn(a, true, false)
}
fn vl_string_right_trim(a: &[Value]) -> R {
    trim_fn(a, false, true)
}
fn vl_string_translate(a: &[Value]) -> R {
    arity(a, 3, 3)?;
    let (from, to, s) = (string(&a[0])?, string(&a[1])?, string(&a[2])?);
    let to: Vec<char> = to.chars().collect();
    let out: String = s.chars().map(|c| from.chars().position(|f| f == c).and_then(|i| to.get(i).or(to.last()).copied()).unwrap_or(c)).collect();
    Ok(Value::str(&out))
}
fn vl_string_to_list(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::list(string(&a[0])?.chars().map(|c| Value::Int(c as i32)).collect::<Vec<_>>()))
}
fn vl_list_to_string(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let mut s = String::new();
    for v in list_arg(&a[0])? {
        s.extend(u32::try_from(int(&v)?).ok().and_then(char::from_u32));
    }
    Ok(Value::str(&s))
}
fn vl_filename_base(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let s = string(&a[0])?;
    Ok(Value::str(std::path::Path::new(&*s.replace('\\', "/")).file_stem().and_then(OsStr::to_str).unwrap_or("")))
}
fn vl_filename_extension(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let s = string(&a[0])?;
    Ok(std::path::Path::new(&*s.replace('\\', "/")).extension().and_then(OsStr::to_str).map(|e| Value::str(&format!(".{e}"))).unwrap_or_default())
}
fn vl_filename_directory(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let s = string(&a[0])?;
    let cut = s.rfind(['/', '\\']).unwrap_or(0);
    Ok(Value::str(&s[..cut]))
}

/// `wcmatch`: `#` a digit, `@` a letter, `.` neither, `*` any run, `?` any character, `~` (first)
/// not, `[…]` a set (`[~…]` not in it, ranges `a-z`), `` ` `` the next character as is, `,`
/// between alternative patterns. Letters compare case-sensitively.
fn wcmatch(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    let (s, pat) = (string(&a[0])?, string(&a[1])?);
    let text: Vec<char> = s.chars().collect();
    // Native wildcard matching runs as a single interpreter step, so its internal `*` backtracking
    // must not explore exponentially many placements unchecked (CWE-400). Charge every glob step
    // against a shared budget and fail the call if it is exhausted.
    let mut budget: u64 = WCMATCH_BUDGET;
    for p in split_patterns(&pat, &mut budget).map_err(|()| LispError::new("wcmatch: pattern too complex"))? {
        match match_pattern(&text, &p, &mut budget) {
            Ok(true) => return Ok(Value::bool(true)),
            Ok(false) => {}
            Err(()) => return Err(LispError::new("wcmatch: pattern too complex")),
        }
    }
    Ok(Value::bool(false))
}

/// Shared step budget for one `wcmatch` call's wildcard matching. Large enough for the patterns
/// AutoLISP routines legitimately use (including the pathological-but-bounded cases the tests
/// cover), but finite so truly exponential `*`-heavy patterns are rejected rather than hanging.
const WCMATCH_BUDGET: u64 = 200_000_000;

/// Spend one wildcard work unit, returning an error when the shared budget is exhausted.
fn charge_match(budget: &mut u64) -> Result<(), ()> {
    *budget = budget.checked_sub(1).ok_or(())?;
    Ok(())
}

/// Split comma-separated wildcard alternatives while preserving escapes and bracket sets.
/// Every consumed character spends from the same budget used for matching.
fn split_patterns(pat: &str, budget: &mut u64) -> Result<Vec<Vec<char>>, ()> {
    let mut out = vec![Vec::new()];
    let mut chars = pat.chars().peekable();
    let mut in_set = false;
    while let Some(c) = chars.next() {
        charge_match(budget)?;
        match c {
            '`' => {
                if let Some(cur) = out.last_mut() {
                    cur.push('`');
                    if let Some(escaped) = chars.next() {
                        charge_match(budget)?;
                        cur.push(escaped);
                    }
                }
            }
            '[' => {
                in_set = true;
                if let Some(cur) = out.last_mut() {
                    cur.push(c);
                }
            }
            ']' => {
                in_set = false;
                if let Some(cur) = out.last_mut() {
                    cur.push(c);
                }
            }
            ',' if !in_set => out.push(Vec::new()),
            c => {
                if let Some(cur) = out.last_mut() {
                    cur.push(c);
                }
            }
        }
    }
    Ok(out)
}

/// Match one wildcard alternative, including leading `~` negation, within the shared budget.
fn match_pattern(text: &[char], pat: &[char], budget: &mut u64) -> Result<bool, ()> {
    if let Some(('~', rest)) = pat.split_first().map(|(c, r)| (*c, r))
        && !rest.is_empty()
    {
        return glob(text, rest, 0, budget).map(|m| !m);
    }
    glob(text, pat, 0, budget)
}

/// Iterative-enough glob with `*` backtracking; `depth` bounds the recursion on `*`. Every step is
/// charged against `budget`; `Err(())` means the shared budget ran out (treated as too complex).
fn glob(text: &[char], pat: &[char], depth: usize, budget: &mut u64) -> Result<bool, ()> {
    glob_cached(text, pat, depth, budget, &mut std::collections::HashSet::new())
}

/// Match suffixes while memoizing failures; both matching work and retained failure entries are bounded.
fn glob_cached(
    text: &[char],
    pat: &[char],
    depth: usize,
    budget: &mut u64,
    failed: &mut std::collections::HashSet<(usize, usize, usize)>,
) -> Result<bool, ()> {
    charge_match(budget)?;
    let key = (text.len(), pat.len(), depth);
    if failed.contains(&key) {
        return Ok(false);
    }
    let matched = glob_inner(text, pat, depth, budget, failed)?;
    // Avoid revisiting the same failed suffix exponentially. Bound the memo table independently
    // of the step limit so a very large pattern cannot turn the optimization into an allocation bomb.
    if !matched && failed.len() < 100_000 {
        failed.insert(key);
    }
    Ok(matched)
}

/// Evaluate wildcard tokens and backtracking branches under shared work and recursion limits.
fn glob_inner(
    text: &[char],
    pat: &[char],
    depth: usize,
    budget: &mut u64,
    failed: &mut std::collections::HashSet<(usize, usize, usize)>,
) -> Result<bool, ()> {
    if *budget == 0 {
        return Err(());
    }
    *budget -= 1;
    if depth > 256 {
        return Err(());
    }
    let (mut t, mut p) = (0usize, 0usize);
    while p < pat.len() {
        charge_match(budget)?;
        match pat[p] {
            '*' => {
                let rest = &pat[p + 1..];
                for i in t..=text.len() {
                    if glob_cached(&text[i..], rest, depth + 1, budget, failed)? {
                        return Ok(true);
                    }
                }
                return Ok(false);
            }
            '`' => {
                let Some(&lit) = pat.get(p + 1) else { return Ok(false) };
                if text.get(t) != Some(&lit) {
                    return Ok(false);
                }
                t += 1;
                p += 2;
            }
            '[' => {
                let mut close = p;
                while let Some(c) = pat.get(close) {
                    charge_match(budget)?;
                    if *c == ']' {
                        break;
                    }
                    close += 1;
                }
                if close == pat.len() {
                    return Ok(false);
                }
                let Some(&c) = text.get(t) else { return Ok(false) };
                let set = &pat[p + 1..close];
                let (neg, set) = match set.split_first() {
                    Some(('~', r)) if !r.is_empty() => (true, r),
                    _ => (false, set),
                };
                let mut hit = false;
                let mut i = 0;
                while i < set.len() {
                    charge_match(budget)?;
                    if set.get(i + 1) == Some(&'-') && i + 2 < set.len() {
                        if set[i] <= c && c <= set[i + 2] {
                            hit = true;
                        }
                        i += 3;
                    } else {
                        if set[i] == c {
                            hit = true;
                        }
                        i += 1;
                    }
                }
                if hit == neg {
                    return Ok(false);
                }
                t += 1;
                p = close + 1;
            }
            pc => {
                let Some(&c) = text.get(t) else { return Ok(false) };
                let ok = match pc {
                    '#' => c.is_ascii_digit(),
                    '@' => c.is_alphabetic(),
                    '.' => !c.is_alphanumeric(),
                    '?' => true,
                    other => other == c,
                };
                if !ok {
                    return Ok(false);
                }
                t += 1;
                p += 1;
            }
        }
    }
    Ok(t == text.len())
}

// ---------------- geometry ----------------

fn polar(a: &[Value]) -> R {
    arity(a, 3, 3)?;
    let p = point(&a[0])?;
    let (ang, d) = (num(&a[1])?, num(&a[2])?);
    let out = [p[0] + d * ang.cos(), p[1] + d * ang.sin(), p[2]];
    Ok(if a[0].iter().count() == 2 { Value::point(&out[..2]) } else { Value::point(&out) })
}
fn distance(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    let (p, q) = (point(&a[0])?, point(&a[1])?);
    // Two 2D points, or one: a 2D distance.
    let dz = if a[0].iter().count() == 2 || a[1].iter().count() == 2 { 0.0 } else { q[2] - p[2] };
    Ok(Value::Real(((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + dz * dz).sqrt()))
}
fn angle(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    let (p, q) = (point(&a[0])?, point(&a[1])?);
    Ok(Value::Real((q[1] - p[1]).atan2(q[0] - p[0]).rem_euclid(std::f64::consts::TAU)))
}
/// `(inters p1 p2 p3 p4 [onseg])`: where lines p1-p2 and p3-p4 cross; with `onseg` (the default)
/// only within both segments.
fn inters(a: &[Value]) -> R {
    arity(a, 4, 5)?;
    let (p1, p2, p3, p4) = (point(&a[0])?, point(&a[1])?, point(&a[2])?, point(&a[3])?);
    let onseg = a.get(4).is_none_or(|v| !v.is_nil());
    let (d1x, d1y, d2x, d2y) = (p2[0] - p1[0], p2[1] - p1[1], p4[0] - p3[0], p4[1] - p3[1]);
    let den = d1x * d2y - d1y * d2x;
    if den.abs() < 1e-12 {
        return Ok(Value::Nil);
    }
    let t = ((p3[0] - p1[0]) * d2y - (p3[1] - p1[1]) * d2x) / den;
    let u = ((p3[0] - p1[0]) * d1y - (p3[1] - p1[1]) * d1x) / den;
    let eps = 1e-10;
    if onseg && (t < -eps || t > 1.0 + eps || u < -eps || u > 1.0 + eps) {
        return Ok(Value::Nil);
    }
    let z = p1[2] + t * (p2[2] - p1[2]);
    let x = [p1[0] + t * d1x, p1[1] + t * d1y, z];
    let two_d = a.iter().take(4).any(|p| p.iter().count() == 2);
    Ok(if two_d { Value::point(&x[..2]) } else { Value::point(&x) })
}

// ---------------- output ----------------

/// Text printed to a file (an open file argument) or the command line.
fn emit(ctx: &mut Ctx<'_>, text: &str, file: Option<&Value>) -> Result<(), LispError> {
    charge_bytes(text.len())?;
    match file {
        None | Some(Value::Nil) => {
            ctx.env.print(text);
            Ok(())
        }
        Some(Value::File(id)) => match ctx.env.files.get_mut(id) {
            Some(f) if f.writing => {
                f.text.push_str(text);
                Ok(())
            }
            _ => Err(LispError::bad_arg("streamp", &Value::File(*id))),
        },
        Some(other) => Err(LispError::bad_arg("streamp", other)),
    }
}

/// `(princ)` with no argument: a symbol that prints as nothing, ending a routine quietly.
pub fn null_symbol() -> Value {
    Value::Sym(Sym(Arc::from("")))
}
pub fn is_null_symbol(v: &Value) -> bool {
    matches!(v, Value::Sym(s) if s.name().is_empty())
}

fn princ(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 2)?;
    let Some(v) = a.first() else { return Ok(Flow::Value(null_symbol())) };
    emit(ctx, &to_text(v, false), a.get(1))?;
    Ok(Flow::Value(v.clone()))
}
fn prin1(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 2)?;
    let Some(v) = a.first() else { return Ok(Flow::Value(null_symbol())) };
    emit(ctx, &to_text(v, true), a.get(1))?;
    Ok(Flow::Value(v.clone()))
}
fn print(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 2)?;
    let Some(v) = a.first() else { return Ok(Flow::Value(null_symbol())) };
    emit(ctx, &format!("\n{} ", to_text(v, true)), a.get(1))?;
    Ok(Flow::Value(v.clone()))
}
fn prompt(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let text = string(&a[0])?;
    charge_bytes(text.len())?;
    ctx.env.print(&text);
    Ok(Flow::Value(Value::Nil))
}
fn terpri(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 1)?;
    emit(ctx, "\n", a.first())?;
    Ok(Flow::Value(Value::Nil))
}
fn alert(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    // No dialog from LISP: the message goes to the command line.
    let msg = string(&a[0])?;
    charge_bytes(msg.len())?;
    let out = std::mem::take(&mut ctx.env.out);
    if !out.is_empty() {
        ctx.s.echo(out);
    }
    for l in msg.lines() {
        ctx.s.echo(l.to_string());
    }
    Ok(Flow::Value(Value::Nil))
}

// ---------------- files ----------------

/// A file opened by `open`: read files are read whole; written ones are written at `close`.
pub struct LispFile {
    pub path: String,
    pub writing: bool,
    pub append: bool,
    pub lines: std::collections::VecDeque<String>,
    pub text: String,
}

fn open(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
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

fn close(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let Value::File(id) = a[0] else { return Err(LispError::bad_arg("streamp", &a[0])) };
    let Some(f) = ctx.env.files.remove(&id) else { return Err(LispError::bad_arg("streamp", &a[0])) };
    if f.writing {
        write_text_file(&f.path, &f.text, f.append);
    }
    Ok(Flow::Value(Value::Nil))
}

fn read_line(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 1)?;
    let Some(Value::File(id)) = a.first() else { return Ok(Flow::Value(Value::Nil)) };
    let line = ctx.env.files.get_mut(id).filter(|f| !f.writing).and_then(|f| f.lines.pop_front());
    Ok(Flow::Value(line.map(|l| Value::str(&l)).unwrap_or_default()))
}

fn write_line(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 2)?;
    let s = string(&a[0])?;
    emit(ctx, &format!("{s}\n"), a.get(1))?;
    Ok(Flow::Value(Value::Str(s)))
}

fn findfile(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let p = string(&a[0])?;
    Ok(if file_exists(&p) { Value::Str(p) } else { Value::Nil })
}

/// Read a regular text file up to 64 MiB, replacing invalid UTF-8 and returning `None` on read failure.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_text_file(path: &str) -> Option<String> {
    // Read through a byte-limited, regular-file-only reader so a path naming an endless device
    // cannot exhaust memory before the 64 MiB cap is enforced (CWE-400).
    const MAX: usize = 64 * 1024 * 1024;
    let bytes = crate::capped_read::read_capped(std::path::Path::new(path), MAX).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}
/// Return `None` on the web, where native filesystem paths are unavailable.
#[cfg(target_arch = "wasm32")]
pub fn read_text_file(_path: &str) -> Option<String> {
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

// ---------------- functions of functions ----------------

/// A function argument: a function, or a symbol naming one (`'car`, `'1+`).
fn callable(ctx: &Ctx<'_>, f: &Value) -> Result<Value, LispError> {
    match f {
        Value::Sym(s) => {
            let v = super::eval::lookup(ctx.env, s);
            if matches!(v, Value::Subr(_) | Value::Func(_)) { Ok(v) } else { Err(LispError::new(format!("no function definition: {}", s.name()))) }
        }
        Value::Subr(_) | Value::Func(_) => Ok(f.clone()),
        // A quoted lambda expression: '(lambda (x) …).
        Value::Cons(c) if c.car.as_sym().is_some_and(|s| s.name() == "LAMBDA") => {
            let def = c.cdr.clone();
            let arglist = def.car();
            if !arglist.is_list() {
                return Err(LispError::bad_arg("listp", &arglist));
            }
            let (mut params, mut locals, mut slash) = (Vec::new(), Vec::new(), false);
            for p in arglist.iter() {
                let Value::Sym(s) = p else { return Err(LispError::bad_arg("symbolp", &p)) };
                if s.name() == "/" {
                    slash = true;
                } else if slash {
                    locals.push(s);
                } else {
                    params.push(s);
                }
            }
            Ok(Value::Func(Arc::new(super::value::Func { name: None, params, locals, body: def.cdr() })))
        }
        other => Err(LispError::new(format!("bad function: {}", to_text(other, true)))),
    }
}

fn apply(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let f = callable(ctx, &a[0])?;
    Ok(Flow::Call(f, list_arg(&a[1])?))
}

fn mapcar(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, usize::MAX)?;
    let f = callable(ctx, &a[0])?;
    for l in &a[1..] {
        list_arg(l)?;
    }
    Ok(Flow::Map(f, a[1..].to_vec()))
}

fn eval_fn(_ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    Ok(Flow::Eval(a[0].clone()))
}

fn set(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let Value::Sym(s) = &a[0] else { return Err(LispError::bad_arg("symbolp", &a[0])) };
    ctx.env.set(s, a[1].clone());
    Ok(Flow::Value(a[1].clone()))
}

fn boundp(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let Value::Sym(s) = &a[0] else { return Err(LispError::bad_arg("symbolp", &a[0])) };
    Ok(Flow::Value(Value::bool(ctx.env.globals.contains_key(s))))
}

fn vl_symbol_value(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let Value::Sym(s) = &a[0] else { return Err(LispError::bad_arg("symbolp", &a[0])) };
    Ok(Flow::Value(super::eval::lookup(ctx.env, s)))
}

fn vl_sort(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let f = callable(ctx, &a[1])?;
    let mut items = list_arg(&a[0])?;
    // A merge sort that calls back into LISP for each comparison.
    let mut err = None;
    merge_sort(&mut items, &mut |x, y| {
        if err.is_some() {
            return false;
        }
        match ctx.call_sync(&f, vec![x.clone(), y.clone()]) {
            Ok(v) => !v.is_nil(),
            Err(e) => {
                err = Some(e);
                false
            }
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    Ok(Flow::Value(Value::list(items)))
}

/// A stable sort with a fallible-free comparison (`less`).
fn merge_sort(v: &mut Vec<Value>, less: &mut dyn FnMut(&Value, &Value) -> bool) {
    if v.len() <= 1 {
        return;
    }
    let mut right = v.split_off(v.len() / 2);
    merge_sort(v, less);
    merge_sort(&mut right, less);
    let left = std::mem::take(v);
    let (mut l, mut r) = (left.into_iter().peekable(), right.into_iter().peekable());
    while let (Some(x), Some(y)) = (l.peek(), r.peek()) {
        if less(y, x) {
            v.extend(r.next());
        } else {
            v.extend(l.next());
        }
    }
    v.extend(l);
    v.extend(r);
}

fn filter(ctx: &mut Ctx<'_>, a: Vec<Value>, keep_true: bool) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let f = callable(ctx, &a[0])?;
    let mut out = Vec::new();
    for v in list_arg(&a[1])? {
        let r = ctx.call_sync(&f, vec![v.clone()])?;
        if r.is_nil() != keep_true {
            out.push(v);
        }
    }
    Ok(Flow::Value(Value::list(out)))
}
fn vl_remove_if(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    filter(ctx, a, false)
}
fn vl_remove_if_not(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    filter(ctx, a, true)
}
fn vl_member_if(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let f = callable(ctx, &a[0])?;
    let mut v = a[1].clone();
    while let Value::Cons(c) = v.clone() {
        if !ctx.call_sync(&f, vec![c.car.clone()])?.is_nil() {
            return Ok(Flow::Value(v));
        }
        v = c.cdr.clone();
    }
    Ok(Flow::Value(Value::Nil))
}
fn every_some(ctx: &mut Ctx<'_>, a: Vec<Value>, every: bool) -> Result<Flow, LispError> {
    arity(&a, 2, usize::MAX)?;
    let f = callable(ctx, &a[0])?;
    let mut lists: Vec<Value> = a[1..].to_vec();
    loop {
        if lists.iter().any(|l| !matches!(l, Value::Cons(_))) {
            return Ok(Flow::Value(Value::bool(every)));
        }
        let r = ctx.call_sync(&f, lists.iter().map(Value::car).collect())?;
        if every && r.is_nil() {
            return Ok(Flow::Value(Value::Nil));
        }
        if !every && !r.is_nil() {
            return Ok(Flow::Value(r));
        }
        lists = lists.iter().map(Value::cdr).collect();
    }
}
fn vl_every(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    every_some(ctx, a, true)
}
fn vl_some(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    every_some(ctx, a, false)
}

/// The value `vl-catch-all-apply` returns for an error.
pub fn catch_all_error(msg: &str) -> Value {
    Value::cons(Value::sym("*VL-CATCH-ALL-APPLY-ERROR*"), Value::str(msg))
}
fn catch_all_message(v: &Value) -> Option<Value> {
    (v.car().as_sym().is_some_and(|s| s.name() == "*VL-CATCH-ALL-APPLY-ERROR*")).then(|| v.cdr())
}
fn vl_catch_all_apply(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let f = callable(ctx, &a[0])?;
    Ok(Flow::Catch(f, list_arg(&a[1])?))
}
fn vl_catch_all_error_p(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(catch_all_message(&a[0]).is_some()))
}
fn vl_catch_all_error_message(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    catch_all_message(&a[0]).ok_or_else(|| LispError::bad_arg("vl-catch-all-apply-error-p", &a[0]))
}

fn exit(_ctx: &mut Ctx<'_>, _a: Vec<Value>) -> Result<Flow, LispError> {
    Err(LispError::quit("quit / exit abort"))
}

/// `(load file [onfailure])`: evaluate a file's forms; its last value is the result.
fn load(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 2)?;
    let name = string(&a[0])?.to_string();
    let text = super::load_source(ctx.s, &name);
    match text {
        Some(t) => Ok(Flow::Forms(read::read_all(&t)?)),
        None => match a.get(1) {
            Some(v) => Ok(Flow::Value(v.clone())),
            None => Err(LispError::new(format!("LOAD failed: \"{name}\""))),
        },
    }
}

/// `(autoload file '("CMD" …))`: each command loads `file` the first time it is typed, then runs
/// the definition the file gave it. Startup files register commands this way without loading
/// every routine up front.
fn autoload(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let file = string(&a[0])?;
    for cmd in list_arg(&a[1])? {
        let name = string(&cmd)?.to_ascii_uppercase();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')) {
            return Err(LispError::bad_arg("command name", &cmd));
        }
        // The stub forgets itself, loads the file and calls what it defined.
        let file_lit = to_text(&Value::Str(file.clone()), true);
        let src = format!(
            "(lambda () (setq C:{name} nil) (load {file_lit}) (if C:{name} (C:{name}) (princ \"\\n; error: {} did not define {name}\")) (princ))",
            file.replace(['"', '\\'], "/")
        );
        let stub = callable(ctx, &read::read_one(&src)?)?;
        ctx.env.set(&Sym::new(&format!("C:{name}")), stub);
    }
    Ok(Flow::Value(Value::Nil))
}

fn noop(_a: &[Value]) -> R {
    Ok(Value::Nil)
}
fn ver(_a: &[Value]) -> R {
    Ok(Value::str("Visual LISP 2026 (en)"))
}

// ---------------- the table ----------------

macro_rules! pure {
    ($name:literal, $f:path) => {
        Builtin { name: $name, kind: BuiltinKind::Pure($f) }
    };
}
macro_rules! ctx {
    ($name:literal, $f:path) => {
        Builtin { name: $name, kind: BuiltinKind::Ctx($f) }
    };
}
macro_rules! special {
    ($name:literal, $s:expr) => {
        Builtin { name: $name, kind: BuiltinKind::Special($s) }
    };
}

static BUILTINS: &[Builtin] = &[
    special!("QUOTE", Special::Quote),
    special!("FUNCTION", Special::Function),
    special!("SETQ", Special::Setq),
    special!("IF", Special::If),
    special!("COND", Special::Cond),
    special!("WHILE", Special::While),
    special!("REPEAT", Special::Repeat),
    special!("FOREACH", Special::Foreach),
    special!("PROGN", Special::Progn),
    special!("AND", Special::And),
    special!("OR", Special::Or),
    special!("DEFUN", Special::Defun),
    special!("DEFUN-Q", Special::Defun),
    special!("LAMBDA", Special::Lambda),
    pure!("+", add),
    pure!("-", sub),
    pure!("*", mul),
    pure!("/", div),
    pure!("REM", rem),
    pure!("1+", one_plus),
    pure!("1-", one_minus),
    pure!("ABS", abs),
    pure!("MIN", min),
    pure!("MAX", max),
    pure!("SQRT", sqrt),
    pure!("EXP", exp),
    pure!("LOG", log),
    pure!("SIN", sin),
    pure!("COS", cos),
    pure!("ATAN", atan),
    pure!("EXPT", expt),
    pure!("FIX", fix),
    pure!("FLOAT", float),
    pure!("GCD", gcd),
    pure!("LOGAND", logand),
    pure!("LOGIOR", logior),
    pure!("LSH", lsh),
    pure!("~", lognot),
    pure!("=", num_eq),
    pure!("/=", num_ne),
    pure!("<", lt),
    pure!(">", gt),
    pure!("<=", le),
    pure!(">=", ge),
    pure!("ZEROP", zerop),
    pure!("MINUSP", minusp),
    pure!("NUMBERP", numberp),
    pure!("ATOM", atom),
    pure!("LISTP", listp),
    pure!("VL-CONSP", consp),
    pure!("NULL", null),
    pure!("NOT", null),
    pure!("TYPE", type_of),
    pure!("EQ", eq_fn),
    pure!("EQUAL", equal_fn),
    pure!("CAR", car),
    pure!("CDR", cdr),
    pure!("CAAR", caar),
    pure!("CADR", cadr),
    pure!("CDAR", cdar),
    pure!("CDDR", cddr),
    pure!("CAAAR", caaar),
    pure!("CAADR", caadr),
    pure!("CADAR", cadar),
    pure!("CADDR", caddr),
    pure!("CDAAR", cdaar),
    pure!("CDADR", cdadr),
    pure!("CDDAR", cddar),
    pure!("CDDDR", cdddr),
    pure!("CADDDR", cadddr),
    pure!("CDDDDR", cddddr),
    pure!("CONS", cons),
    pure!("LIST", list),
    pure!("APPEND", append),
    pure!("REVERSE", reverse),
    pure!("LENGTH", length),
    pure!("NTH", nth),
    pure!("LAST", last),
    pure!("MEMBER", member),
    pure!("ASSOC", assoc),
    pure!("SUBST", subst),
    pure!("VL-POSITION", vl_position),
    pure!("VL-REMOVE", vl_remove),
    pure!("VL-LIST-LENGTH", vl_list_length),
    pure!("ACAD_STRLSORT", acad_strlsort),
    pure!("STRCAT", strcat),
    pure!("STRLEN", strlen),
    pure!("SUBSTR", substr),
    pure!("STRCASE", strcase),
    pure!("ITOA", itoa),
    pure!("ATOI", atoi),
    pure!("ATOF", atof),
    pure!("CHR", chr),
    pure!("ASCII", ascii),
    pure!("READ", read_fn),
    pure!("WCMATCH", wcmatch),
    pure!("VL-PRIN1-TO-STRING", vl_prin1_to_string),
    pure!("VL-PRINC-TO-STRING", vl_princ_to_string),
    pure!("VL-SYMBOL-NAME", vl_symbol_name),
    pure!("VL-SYMBOLP", vl_symbolp),
    pure!("VL-STRING-SEARCH", vl_string_search),
    pure!("VL-STRING-SUBST", vl_string_subst),
    pure!("VL-STRING-TRIM", vl_string_trim),
    pure!("VL-STRING-LEFT-TRIM", vl_string_left_trim),
    pure!("VL-STRING-RIGHT-TRIM", vl_string_right_trim),
    pure!("VL-STRING-TRANSLATE", vl_string_translate),
    pure!("VL-STRING->LIST", vl_string_to_list),
    pure!("VL-LIST->STRING", vl_list_to_string),
    pure!("VL-FILENAME-BASE", vl_filename_base),
    pure!("VL-FILENAME-EXTENSION", vl_filename_extension),
    pure!("VL-FILENAME-DIRECTORY", vl_filename_directory),
    pure!("POLAR", polar),
    pure!("DISTANCE", distance),
    pure!("ANGLE", angle),
    pure!("INTERS", inters),
    pure!("FINDFILE", findfile),
    pure!("VL-CATCH-ALL-ERROR-P", vl_catch_all_error_p),
    pure!("VL-CATCH-ALL-ERROR-MESSAGE", vl_catch_all_error_message),
    pure!("VL-LOAD-COM", noop),
    pure!("GC", noop),
    pure!("REDRAW", noop),
    pure!("VER", ver),
    ctx!("PRINC", princ),
    ctx!("PRIN1", prin1),
    ctx!("PRINT", print),
    ctx!("PROMPT", prompt),
    ctx!("TERPRI", terpri),
    ctx!("ALERT", alert),
    ctx!("OPEN", open),
    ctx!("CLOSE", close),
    ctx!("READ-LINE", read_line),
    ctx!("WRITE-LINE", write_line),
    ctx!("APPLY", apply),
    ctx!("MAPCAR", mapcar),
    ctx!("EVAL", eval_fn),
    ctx!("SET", set),
    ctx!("BOUNDP", boundp),
    ctx!("VL-SYMBOL-VALUE", vl_symbol_value),
    ctx!("VL-SORT", vl_sort),
    ctx!("VL-REMOVE-IF", vl_remove_if),
    ctx!("VL-REMOVE-IF-NOT", vl_remove_if_not),
    ctx!("VL-MEMBER-IF", vl_member_if),
    ctx!("VL-EVERY", vl_every),
    ctx!("VL-SOME", vl_some),
    ctx!("VL-CATCH-ALL-APPLY", vl_catch_all_apply),
    ctx!("EXIT", exit),
    ctx!("QUIT", exit),
    ctx!("LOAD", load),
    ctx!("AUTOLOAD", autoload),
    ctx!("INITGET", input::initget),
    ctx!("GETPOINT", input::getpoint),
    ctx!("GETCORNER", input::getcorner),
    ctx!("GETDIST", input::getdist),
    ctx!("GETANGLE", input::getangle),
    ctx!("GETORIENT", input::getorient),
    ctx!("GETREAL", input::getreal),
    ctx!("GETINT", input::getint),
    ctx!("GETSTRING", input::getstring),
    ctx!("GETKWORD", input::getkword),
    ctx!("ENTSEL", input::entsel),
    ctx!("NENTSEL", input::entsel),
    ctx!("COMMAND", drawing::command),
    ctx!("COMMAND-S", drawing::command),
    ctx!("VL-CMDF", drawing::command),
    ctx!("GETVAR", drawing::getvar),
    ctx!("SETVAR", drawing::setvar),
    ctx!("ENTGET", drawing::entget),
    ctx!("ENTMAKE", drawing::entmake),
    ctx!("ENTMOD", drawing::entmod),
    ctx!("ENTDEL", drawing::entdel),
    ctx!("ENTLAST", drawing::entlast),
    ctx!("ENTNEXT", drawing::entnext),
    ctx!("ENTUPD", drawing::entupd),
    ctx!("HANDENT", drawing::handent),
    ctx!("SSGET", drawing::ssget),
    ctx!("SSADD", drawing::ssadd),
    ctx!("SSDEL", drawing::ssdel),
    ctx!("SSLENGTH", drawing::sslength),
    ctx!("SSNAME", drawing::ssname),
    ctx!("SSMEMB", drawing::ssmemb),
    ctx!("SSSETFIRST", drawing::sssetfirst),
    ctx!("SSGETFIRST", drawing::ssgetfirst),
    ctx!("TBLSEARCH", drawing::tblsearch),
    ctx!("TBLNEXT", drawing::tblnext),
    ctx!("RTOS", drawing::rtos),
    ctx!("ANGTOS", drawing::angtos),
    ctx!("DISTOF", drawing::distof),
    ctx!("ANGTOF", drawing::angtof),
    ctx!("TRANS", drawing::trans),
];

/// Bind the builtins and constants in a new environment.
pub fn install(env: &mut Env) {
    for b in BUILTINS {
        env.globals.insert(Sym::new(b.name), Value::Subr(b));
    }
    env.globals.insert(Sym::new("T"), Value::t());
    env.globals.insert(Sym::new("PI"), Value::Real(std::f64::consts::PI));
    env.globals.insert(Sym::new("PAUSE"), Value::str("\\"));
    #[cfg(test)]
    env.globals.insert(Sym::new(TEST_PANIC.name), Value::Subr(&TEST_PANIC));
}

/// Tests only: a built-in that panics, to prove the guards around the machine.
#[cfg(test)]
static TEST_PANIC: Builtin = Builtin { name: "CADCRAFT-TEST-PANIC", kind: BuiltinKind::Pure(test_panic) };
#[cfg(test)]
#[allow(clippy::panic)]
fn test_panic(_a: &[Value]) -> R {
    panic!("test panic")
}

/// A real as `princ` shows it (for messages).
pub fn real(r: f64) -> String {
    real_text(r)
}

/// `wcmatch` for other callers (`ssget` filters).
pub fn wild_match(text: &str, pattern: &str) -> bool {
    let t: Vec<char> = text.chars().collect();
    let mut budget: u64 = WCMATCH_BUDGET;
    split_patterns(pattern, &mut budget).is_ok_and(|patterns| patterns.iter().any(|p| match_pattern(&t, p, &mut budget) == Ok(true)))
}

#[cfg(test)]
mod wildcard_budget_tests {
    use super::*;

    /// Pattern splitting, literal matching, and bracket scans all stop when their shared fuel runs out.
    #[test]
    fn all_native_scans_consume_fuel() {
        assert!(split_patterns("abcdefgh", &mut 4).is_err());
        assert!(split_patterns("`a`b`c", &mut 4).is_err());
        let text: Vec<_> = "abcdefgh".chars().collect();
        assert!(glob(&text, &text, 0, &mut 4).is_err());
        let pattern: Vec<_> = "[abcdefgh]".chars().collect();
        assert!(glob(&['!'], &pattern, 0, &mut 6).is_err());
        // Enough for finding the closing bracket, but not scanning the entire set again.
        assert!(glob(&['!'], &pattern, 0, &mut 14).is_err());
        assert_eq!(glob(&['a'], &pattern, 0, &mut 100), Ok(true));
    }
}
