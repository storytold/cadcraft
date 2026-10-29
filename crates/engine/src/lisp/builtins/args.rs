use super::{Arc, LispError, Value};

pub(super) type R = Result<Value, LispError>;

/// `too few arguments` / `too many arguments`.
pub fn args_err(few: bool) -> LispError {
    LispError::new(if few { "too few arguments" } else { "too many arguments" })
}

pub(super) fn arity(a: &[Value], min: usize, max: usize) -> Result<(), LispError> {
    if a.len() < min {
        return Err(args_err(true));
    }
    if a.len() > max {
        return Err(args_err(false));
    }
    Ok(())
}

pub(super) fn num(v: &Value) -> Result<f64, LispError> {
    v.as_number().ok_or_else(|| LispError::bad_arg("numberp", v))
}

pub(super) fn int(v: &Value) -> Result<i32, LispError> {
    match v {
        Value::Int(i) => Ok(*i),
        other => Err(LispError::bad_arg("fixnump", other)),
    }
}

pub(super) fn string(v: &Value) -> Result<Arc<str>, LispError> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        other => Err(LispError::bad_arg("stringp", other)),
    }
}

pub(super) fn list_arg(v: &Value) -> Result<Vec<Value>, LispError> {
    if v.is_list() { Ok(v.to_vec()) } else { Err(LispError::bad_arg("listp", v)) }
}

pub(super) fn point(v: &Value) -> Result<[f64; 3], LispError> {
    v.as_point().ok_or_else(|| LispError::bad_arg("2D/3D point", v))
}

pub(super) fn real_or_int(r: f64, all_int: bool) -> Value {
    if all_int { Value::Int(r as i32) } else { Value::Real(r) }
}
