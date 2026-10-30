use super::args::{R, arity, int, num, real_or_int};
use super::{LispError, Ordering, Value, eq};

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

pub(super) fn add(a: &[Value]) -> R {
    arith(a, '+')
}

pub(super) fn sub(a: &[Value]) -> R {
    arith(a, '-')
}

pub(super) fn mul(a: &[Value]) -> R {
    arith(a, '*')
}

pub(super) fn div(a: &[Value]) -> R {
    arity(a, 1, usize::MAX)?;
    arith(a, '/')
}

pub(super) fn rem(a: &[Value]) -> R {
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

pub(super) fn one_plus(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    arith(&[a[0].clone(), Value::Int(1)], '+')
}

pub(super) fn one_minus(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    arith(&[a[0].clone(), Value::Int(1)], '-')
}

pub(super) fn abs(a: &[Value]) -> R {
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

pub(super) fn min(a: &[Value]) -> R {
    min_max(a, false)
}

pub(super) fn max(a: &[Value]) -> R {
    min_max(a, true)
}

fn unary(a: &[Value], f: fn(f64) -> f64) -> R {
    arity(a, 1, 1)?;
    let r = f(num(&a[0])?);
    if r.is_finite() { Ok(Value::Real(r)) } else { Err(LispError::new("function undefined for argument")) }
}

pub(super) fn sqrt(a: &[Value]) -> R {
    if num(a.first().unwrap_or(&Value::Nil))? < 0.0 {
        return Err(LispError::new("function undefined for argument"));
    }
    unary(a, f64::sqrt)
}

pub(super) fn exp(a: &[Value]) -> R {
    unary(a, f64::exp)
}

pub(super) fn log(a: &[Value]) -> R {
    if num(a.first().unwrap_or(&Value::Nil))? <= 0.0 {
        return Err(LispError::new("function undefined for argument"));
    }
    unary(a, f64::ln)
}

pub(super) fn sin(a: &[Value]) -> R {
    unary(a, f64::sin)
}

pub(super) fn cos(a: &[Value]) -> R {
    unary(a, f64::cos)
}

pub(super) fn atan(a: &[Value]) -> R {
    arity(a, 1, 2)?;
    let y = num(&a[0])?;
    Ok(Value::Real(match a.get(1) {
        Some(x) => y.atan2(num(x)?),
        None => y.atan(),
    }))
}

pub(super) fn expt(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    match (&a[0], &a[1]) {
        (Value::Int(b), Value::Int(e)) if *e >= 0 => Ok(Value::Int(b.wrapping_pow(u32::try_from(*e).unwrap_or(0)))),
        (b, e) => {
            let r = num(b)?.powf(num(e)?);
            if r.is_finite() { Ok(Value::Real(r)) } else { Err(LispError::new("function undefined for argument")) }
        }
    }
}

pub(super) fn fix(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(match &a[0] {
        Value::Int(i) => Value::Int(*i),
        v => {
            let r = num(v)?.trunc();
            if r.is_finite() && r >= f64::from(i32::MIN) && r <= f64::from(i32::MAX) { Value::Int(r as i32) } else { Value::Real(r) }
        }
    })
}

pub(super) fn float(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::Real(num(&a[0])?))
}

pub(super) fn gcd(a: &[Value]) -> R {
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

pub(super) fn logand(a: &[Value]) -> R {
    a.iter().try_fold(-1i32, |acc, v| Ok(acc & int(v)?)).map(|r| Value::Int(if a.is_empty() { 0 } else { r }))
}

pub(super) fn logior(a: &[Value]) -> R {
    a.iter().try_fold(0i32, |acc, v| Ok(acc | int(v)?)).map(Value::Int)
}

pub(super) fn lsh(a: &[Value]) -> R {
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

pub(super) fn lognot(a: &[Value]) -> R {
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

pub(super) fn num_eq(a: &[Value]) -> R {
    eq_chain(a, true)
}

pub(super) fn num_ne(a: &[Value]) -> R {
    eq_chain(a, false)
}

pub(super) fn lt(a: &[Value]) -> R {
    chain(a, Ordering::is_lt)
}

pub(super) fn gt(a: &[Value]) -> R {
    chain(a, Ordering::is_gt)
}

pub(super) fn le(a: &[Value]) -> R {
    chain(a, Ordering::is_le)
}

pub(super) fn ge(a: &[Value]) -> R {
    chain(a, Ordering::is_ge)
}

pub(super) fn zerop(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(num(&a[0])? == 0.0))
}

pub(super) fn minusp(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(num(&a[0])? < 0.0))
}

pub(super) fn numberp(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(a[0].as_number().is_some()))
}
