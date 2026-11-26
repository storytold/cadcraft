//! `load_dialog` and `unload_dialog`.
//!
//! A load failure returns a negative integer and does not change the drawing's handle store.
//! `unload_dialog` removes only the handle it is given and always returns nil.

use super::args::{arity, int, string};
use super::{Ctx, Flow, LispError, Value};
use crate::lisp::dcl_load::{self, LoadedDialog};

pub(super) fn load_dialog(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let name = string(&a[0])?;
    if ctx.env.dialogs.len() >= dcl_load::MAX_DIALOGS {
        return Ok(failed());
    }
    let Some(dialog) = dcl_load::load_named(name.as_ref(), ctx.s.lisp_support_dir())? else {
        return Ok(failed());
    };
    let Some(id) = next_dialog_id(&ctx.env.dialogs, ctx.env.next_dialog) else {
        return Ok(failed());
    };
    ctx.env.next_dialog = if id == i32::MAX { 1 } else { id + 1 };
    ctx.env.dialogs.insert(id, dialog);
    Ok(Flow::Value(Value::Int(id)))
}

pub(super) fn unload_dialog(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let id = int(&a[0])?;
    ctx.env.dialogs.remove(&id);
    Ok(Flow::Value(Value::Nil))
}

fn failed() -> Flow {
    Flow::Value(Value::Int(dcl_load::LOAD_FAILED))
}

/// The next free positive handle. At most [`dcl_load::MAX_DIALOGS`] live handles, so a short
/// scan finds a free one or reports that the store cannot take another.
fn next_dialog_id(used: &std::collections::HashMap<i32, LoadedDialog>, start: i32) -> Option<i32> {
    let mut id = if start > 0 { start } else { 1 };
    for _ in 0..=dcl_load::MAX_DIALOGS {
        if !used.contains_key(&id) {
            return Some(id);
        }
        id = if id == i32::MAX { 1 } else { id + 1 };
    }
    None
}
