use super::args::{arity, string};
use super::{Arc, Ctx, Flow, LispError, Sym, Value, charge_bytes, real_text, to_text};

// ---------------- output ----------------

/// Text printed to a file (an open file argument) or the command line.
pub(super) fn emit(ctx: &mut Ctx<'_>, text: &str, file: Option<&Value>) -> Result<(), LispError> {
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

pub(super) fn princ(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 2)?;
    let Some(v) = a.first() else { return Ok(Flow::Value(null_symbol())) };
    emit(ctx, &to_text(v, false), a.get(1))?;
    Ok(Flow::Value(v.clone()))
}

pub(super) fn prin1(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 2)?;
    let Some(v) = a.first() else { return Ok(Flow::Value(null_symbol())) };
    emit(ctx, &to_text(v, true), a.get(1))?;
    Ok(Flow::Value(v.clone()))
}

pub(super) fn print(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 2)?;
    let Some(v) = a.first() else { return Ok(Flow::Value(null_symbol())) };
    emit(ctx, &format!("\n{} ", to_text(v, true)), a.get(1))?;
    Ok(Flow::Value(v.clone()))
}

pub(super) fn prompt(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let text = string(&a[0])?;
    charge_bytes(text.len())?;
    ctx.env.print(&text);
    Ok(Flow::Value(Value::Nil))
}

pub(super) fn terpri(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 1)?;
    emit(ctx, "\n", a.first())?;
    Ok(Flow::Value(Value::Nil))
}

pub(super) fn alert(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
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

/// A real as `princ` shows it (for messages).
pub fn real(r: f64) -> String {
    real_text(r)
}
