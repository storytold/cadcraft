use super::args::{R, arity, int, list_arg, num, string};
use super::{LispError, Value, eq, equal};

// ---------------- predicates and lists ----------------

pub(super) fn atom(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(!matches!(a[0], Value::Cons(_))))
}

pub(super) fn listp(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(matches!(a[0], Value::Cons(_) | Value::Nil)))
}

pub(super) fn consp(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(matches!(a[0], Value::Cons(_))))
}

pub(super) fn null(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(a[0].is_nil()))
}

pub(super) fn type_of(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(a[0].type_name().map(Value::sym).unwrap_or_default())
}

pub(super) fn eq_fn(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    Ok(Value::bool(eq(&a[0], &a[1])))
}

pub(super) fn equal_fn(a: &[Value]) -> R {
    arity(a, 2, 3)?;
    let fuzz = a.get(2).map(num).transpose()?.unwrap_or(0.0);
    Ok(Value::bool(equal(&a[0], &a[1], fuzz)))
}

pub(super) fn cxr(a: &[Value], path: &str) -> R {
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

pub(super) fn cons(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    Ok(Value::cons(a[0].clone(), a[1].clone()))
}

pub(super) fn list(a: &[Value]) -> R {
    Ok(Value::list(a.to_vec()))
}

pub(super) fn append(a: &[Value]) -> R {
    let mut items = Vec::new();
    for v in a {
        items.extend(list_arg(v)?);
    }
    Ok(Value::list(items))
}

pub(super) fn reverse(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let mut v = list_arg(&a[0])?;
    v.reverse();
    Ok(Value::list(v))
}

pub(super) fn length(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::Int(i32::try_from(list_arg(&a[0])?.len()).unwrap_or(i32::MAX)))
}

pub(super) fn nth(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    let n = int(&a[0])?;
    if n < 0 {
        return Ok(Value::Nil);
    }
    Ok(a[1].iter().nth(n as usize).unwrap_or_default())
}

pub(super) fn last(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(list_arg(&a[0])?.pop().unwrap_or_default())
}

pub(super) fn member(a: &[Value]) -> R {
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

pub(super) fn assoc(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    Ok(a[1].iter().find(|item| matches!(item, Value::Cons(_)) && equal(&item.car(), &a[0], 0.0)).unwrap_or_default())
}

pub(super) fn subst(a: &[Value]) -> R {
    arity(a, 3, 3)?;
    let (new, old) = (&a[0], &a[1]);
    if !a[2].is_list() {
        return Ok(if equal(&a[2], old, 0.0) { new.clone() } else { a[2].clone() });
    }
    Ok(Value::list(a[2].iter().map(|v| if equal(&v, old, 0.0) { new.clone() } else { v }).collect::<Vec<_>>()))
}

pub(super) fn vl_position(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    Ok(a[1].iter().position(|v| equal(&v, &a[0], 0.0)).map(|i| Value::Int(i32::try_from(i).unwrap_or(i32::MAX))).unwrap_or_default())
}

pub(super) fn vl_remove(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    Ok(Value::list(list_arg(&a[1])?.into_iter().filter(|v| !equal(v, &a[0], 0.0)).collect::<Vec<_>>()))
}

pub(super) fn vl_list_length(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(if a[0].is_list() { Value::Int(i32::try_from(a[0].to_vec().len()).unwrap_or(i32::MAX)) } else { Value::Nil })
}

pub(super) fn acad_strlsort(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    let mut v = list_arg(&a[0])?.iter().map(string).collect::<Result<Vec<_>, _>>()?;
    v.sort_by_key(|s| s.to_lowercase());
    Ok(Value::list(v.into_iter().map(Value::Str).collect::<Vec<_>>()))
}
