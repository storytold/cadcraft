use super::args::{R, arity, list_arg, string};
use super::{Arc, Ctx, Flow, LispError, Sym, Value, read, to_text};

// ---------------- functions of functions ----------------

/// A function argument: a function, or a symbol naming one (`'car`, `'1+`).
fn callable(ctx: &Ctx<'_>, f: &Value) -> Result<Value, LispError> {
    match f {
        Value::Sym(s) => {
            let v = crate::lisp::eval::lookup(ctx.env, s);
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
            Ok(Value::Func(Arc::new(crate::lisp::value::Func { name: None, params, locals, body: def.cdr() })))
        }
        other => Err(LispError::new(format!("bad function: {}", to_text(other, true)))),
    }
}

pub(super) fn apply(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let f = callable(ctx, &a[0])?;
    Ok(Flow::Call(f, list_arg(&a[1])?))
}

pub(super) fn mapcar(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, usize::MAX)?;
    let f = callable(ctx, &a[0])?;
    for l in &a[1..] {
        list_arg(l)?;
    }
    Ok(Flow::Map(f, a[1..].to_vec()))
}

pub(super) fn eval_fn(_ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    Ok(Flow::Eval(a[0].clone()))
}

pub(super) fn set(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let Value::Sym(s) = &a[0] else { return Err(LispError::bad_arg("symbolp", &a[0])) };
    ctx.env.set(s, a[1].clone());
    Ok(Flow::Value(a[1].clone()))
}

pub(super) fn boundp(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let Value::Sym(s) = &a[0] else { return Err(LispError::bad_arg("symbolp", &a[0])) };
    Ok(Flow::Value(Value::bool(ctx.env.globals.contains_key(s))))
}

pub(super) fn vl_symbol_value(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let Value::Sym(s) = &a[0] else { return Err(LispError::bad_arg("symbolp", &a[0])) };
    Ok(Flow::Value(crate::lisp::eval::lookup(ctx.env, s)))
}

pub(super) fn vl_sort(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
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

pub(super) fn vl_remove_if(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    filter(ctx, a, false)
}

pub(super) fn vl_remove_if_not(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    filter(ctx, a, true)
}

pub(super) fn vl_member_if(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
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

pub(super) fn vl_every(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    every_some(ctx, a, true)
}

pub(super) fn vl_some(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    every_some(ctx, a, false)
}

/// The value `vl-catch-all-apply` returns for an error.
pub fn catch_all_error(msg: &str) -> Value {
    Value::cons(Value::sym("*VL-CATCH-ALL-APPLY-ERROR*"), Value::str(msg))
}

fn catch_all_message(v: &Value) -> Option<Value> {
    (v.car().as_sym().is_some_and(|s| s.name() == "*VL-CATCH-ALL-APPLY-ERROR*")).then(|| v.cdr())
}

pub(super) fn vl_catch_all_apply(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let f = callable(ctx, &a[0])?;
    Ok(Flow::Catch(f, list_arg(&a[1])?))
}

pub(super) fn vl_catch_all_error_p(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    Ok(Value::bool(catch_all_message(&a[0]).is_some()))
}

pub(super) fn vl_catch_all_error_message(a: &[Value]) -> R {
    arity(a, 1, 1)?;
    catch_all_message(&a[0]).ok_or_else(|| LispError::bad_arg("vl-catch-all-apply-error-p", &a[0]))
}

pub(super) fn exit(_ctx: &mut Ctx<'_>, _a: Vec<Value>) -> Result<Flow, LispError> {
    Err(LispError::quit("quit / exit abort"))
}

/// `(load file [onfailure])`: evaluate a file's forms; its last value is the result.
pub(super) fn load(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 2)?;
    let name = string(&a[0])?.to_string();
    let text = crate::lisp::load_source(ctx.s, &name);
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
pub(super) fn autoload(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
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

pub(super) fn noop(_a: &[Value]) -> R {
    Ok(Value::Nil)
}

pub(super) fn ver(_a: &[Value]) -> R {
    Ok(Value::str("Visual LISP 2026 (en)"))
}
