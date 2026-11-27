//! Layer states from LISP (#3001): AutoCAD's `layerstate-save`, `-restore`, `-has`, `-delete`,
//! `-rename` and `-getnames`, and BricsCAD's `vl-layerstates-save`, `-restore`, `-has`,
//! `-delete`, `-rename` and `-list`.
//!
//! They act on the drawing's saved layer states (`Drawing::layer_states`, the store LAYERSTATE
//! and the DXF files use). A state holds every property of every layer. CADCraft keeps no
//! property mask, description or viewport overrides with a state, so the mask given to a save is
//! checked but not kept, a restore puts back every property (unless BricsCAD's restore is given
//! a mask), and viewport arguments are accepted and ignored; AutoCAD's restore flag 4 (restore
//! as viewport overrides) with a viewport gives nil. Names match without regard to case. The
//! snapshot states the named views keep (`*VIEW name`) are not listed and can't be touched here.

use cadcraft_doc::{Drawing, Layer, LayerState};

use super::LispError;
use super::drawing::arity;
use super::eval::{Ctx, Flow};
use super::value::Value;

// Layer property bits of a BricsCAD restore mask (`vl-layerstates-setpropertymask`). Bit 512
// (current viewport) has no CADCraft property and is ignored.
const ON: i32 = 1;
const FROZEN: i32 = 2;
const LOCKED: i32 = 4;
const PLOT: i32 = 8;
const NEW_VP_FREEZE: i32 = 16;
const COLOR: i32 = 32;
const LINETYPE: i32 = 64;
const LINEWEIGHT: i32 = 128;
const PLOT_STYLE: i32 = 256;

/// Saved states a drawing keeps through LISP; a loop that names states forever stops here.
const MAX_STATES: usize = 4096;

fn flow(v: Value) -> Result<Flow, LispError> {
    Ok(Flow::Value(v))
}

fn truth(b: bool) -> Value {
    if b { Value::t() } else { Value::Nil }
}

/// A state name: a string, or (as BricsCAD allows) a symbol.
fn state_name(v: &Value) -> Result<String, LispError> {
    match v {
        Value::Str(s) => Ok(s.to_string()),
        Value::Sym(s) => Ok(s.name().to_string()),
        other => Err(LispError::bad_arg("stringp", other)),
    }
}

/// An optional integer argument; absent or nil is `None`.
fn int_arg(v: Option<&Value>) -> Result<Option<i32>, LispError> {
    match v {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Int(i)) => Ok(Some(*i)),
        Some(other) => Err(LispError::bad_arg("fixnump", other)),
    }
}

/// A usable state name: not empty, not a hidden (`*`) name, none of the characters symbol table
/// names can't hold.
fn valid_name(n: &str) -> bool {
    !n.trim().is_empty() && n.len() <= 255 && !n.trim().starts_with('*') && !n.chars().any(|c| c.is_control() || "<>/\\\":;?*|,=`".contains(c))
}

fn position(d: &Drawing, name: &str) -> Option<usize> {
    d.layer_states.iter().position(|st| !st.name.starts_with('*') && st.name.eq_ignore_ascii_case(name.trim()))
}

/// What a restore does to the drawing's layers that the state doesn't hold.
#[derive(Clone, Copy, Default)]
struct Others {
    off: bool,
    freeze: bool,
}

/// Put the state's layers back onto the drawing's layers of the same name: every property when
/// `mask` is `None`, else the properties its bits name. Layers the state doesn't hold are turned
/// off or frozen as `others` says. The current layer is never frozen. The state's layer names,
/// or `None` when there is no such state.
fn apply(d: &mut Drawing, name: &str, mask: Option<i32>, others: Others) -> Option<Vec<String>> {
    let saved = d.layer_states.get(position(d, name)?)?.layers.clone();
    let current = d.header.str("CLAYER", "0");
    let mut index = std::collections::HashMap::with_capacity(d.layers.len());
    for (i, l) in d.layers.iter().enumerate() {
        // The first of names that differ only in case, as `Drawing::layer_mut` finds it.
        index.entry(l.name.to_ascii_lowercase()).or_insert(i);
    }
    let mut held = vec![false; d.layers.len()];
    for s in &saved {
        let Some(&i) = index.get(&s.name.to_ascii_lowercase()) else { continue };
        let Some(l) = d.layers.get_mut(i) else { continue };
        if let Some(h) = held.get_mut(i) {
            *h = true;
        }
        let was_frozen = l.frozen;
        put(l, s, mask);
        if l.frozen && l.name.eq_ignore_ascii_case(&current) {
            l.frozen = was_frozen;
        }
    }
    for (l, _) in d.layers.iter_mut().zip(&held).filter(|(_, h)| !**h) {
        if others.off {
            l.on = false;
        }
        if others.freeze && !l.name.eq_ignore_ascii_case(&current) {
            l.frozen = true;
        }
    }
    Some(saved.into_iter().map(|l| l.name).collect())
}

fn put(l: &mut Layer, s: &Layer, mask: Option<i32>) {
    let Some(m) = mask else {
        let name = std::mem::take(&mut l.name);
        *l = s.clone();
        l.name = name;
        return;
    };
    if m & ON != 0 {
        l.on = s.on;
    }
    if m & FROZEN != 0 {
        l.frozen = s.frozen;
    }
    if m & LOCKED != 0 {
        l.locked = s.locked;
    }
    if m & PLOT != 0 {
        l.plot = s.plot;
    }
    if m & NEW_VP_FREEZE != 0 {
        l.vp_freeze_new = s.vp_freeze_new;
    }
    if m & COLOR != 0 {
        l.color = s.color;
    }
    if m & LINETYPE != 0 {
        l.linetype.clone_from(&s.linetype);
    }
    if m & LINEWEIGHT != 0 {
        l.lineweight = s.lineweight;
    }
    if m & PLOT_STYLE != 0 {
        l.plot_style.clone_from(&s.plot_style);
    }
}

/// `(layerstate-save name mask viewport)`, `(vl-layerstates-save name [mask [viewport]])`: T when
/// the layers are saved as `name` (a state of that name is replaced), nil when the name is
/// unusable. The state keeps every layer property; the mask is checked but not kept.
pub fn save(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 3)?;
    let name = state_name(&a[0])?;
    int_arg(a.get(1))?;
    if !valid_name(&name) {
        return flow(Value::Nil);
    }
    let Ok(d) = ctx.s.doc_mut() else { return flow(Value::Nil) };
    let at = position(d, &name);
    if at.is_none() && d.layer_states.len() >= MAX_STATES {
        return flow(Value::Nil);
    }
    let state = LayerState { name: name.trim().to_string(), layers: d.layers.clone() };
    match at.and_then(|i| d.layer_states.get_mut(i)) {
        Some(slot) => *slot = state,
        None => d.layer_states.push(state),
    }
    ctx.s.touch();
    flow(Value::t())
}

/// `(layerstate-restore name viewport [restoreflags])`: put the state's layers back. Flag 1 turns
/// off and flag 2 freezes the layers the state doesn't hold; flag 4 (as viewport overrides) with
/// a viewport is nil, as CADCraft keeps no viewport overrides. The state's layer names, nil when
/// there is no such state or it holds no layers.
pub fn restore(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 3)?;
    let name = state_name(&a[0])?;
    let flags = int_arg(a.get(2))?.unwrap_or(0);
    if flags & 4 != 0 && a.get(1).is_some_and(|v| !matches!(v, Value::Nil)) {
        return flow(Value::Nil);
    }
    let Ok(d) = ctx.s.doc_mut() else { return flow(Value::Nil) };
    let Some(names) = apply(d, &name, None, Others { off: flags & 1 != 0, freeze: flags & 2 != 0 }) else {
        return flow(Value::Nil);
    };
    ctx.s.touch();
    flow(Value::list(names.iter().map(|n| Value::str(n)).collect::<Vec<_>>()))
}

/// `(vl-layerstates-restore name [mask [viewport [undefaction]]])`, BricsCAD's: put back the
/// properties the mask names (all of them when absent, nil or -1); `undefaction` 1 turns off and
/// 2 freezes the layers the state doesn't hold. T, or nil when there is no such state.
pub fn vl_restore(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 4)?;
    let name = state_name(&a[0])?;
    let mask = int_arg(a.get(1))?.filter(|&m| m != -1);
    let undef = int_arg(a.get(3))?.unwrap_or(0);
    let Ok(d) = ctx.s.doc_mut() else { return flow(Value::Nil) };
    let done = apply(d, &name, mask, Others { off: undef == 1, freeze: undef == 2 }).is_some();
    if done {
        ctx.s.touch();
    }
    flow(truth(done))
}

/// `(layerstate-has name)`: T when a state of that name is saved.
pub fn has(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let name = state_name(&a[0])?;
    flow(truth(ctx.s.doc().is_ok_and(|d| position(d, &name).is_some())))
}

/// `(layerstate-delete name)`: T when the state is deleted, nil when there is none.
pub fn delete(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let name = state_name(&a[0])?;
    let Ok(d) = ctx.s.doc_mut() else { return flow(Value::Nil) };
    let Some(i) = position(d, &name) else { return flow(Value::Nil) };
    d.layer_states.remove(i);
    ctx.s.touch();
    flow(Value::t())
}

/// `(layerstate-getnames [hidden [xref]])`, `(vl-layerstates-list)`: the saved state names, nil
/// when there are none. CADCraft keeps no hidden or xref states apart from the named views' own,
/// so the arguments are accepted and ignored.
pub fn getnames(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 2)?;
    let names: Vec<Value> = ctx
        .s
        .doc()
        .map(|d| d.layer_states.iter().filter(|st| !st.name.starts_with('*')).map(|st| Value::str(&st.name)).collect())
        .unwrap_or_default();
    flow(Value::list(names))
}

/// `(layerstate-rename old new)`: T when renamed; nil when there is no `old`, `new` is unusable
/// or another state has that name.
pub fn rename(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let (old, new) = (state_name(&a[0])?, state_name(&a[1])?);
    if !valid_name(&new) {
        return flow(Value::Nil);
    }
    let Ok(d) = ctx.s.doc_mut() else { return flow(Value::Nil) };
    let Some(i) = position(d, &old) else { return flow(Value::Nil) };
    if position(d, &new).is_some_and(|j| j != i) {
        return flow(Value::Nil);
    }
    if let Some(st) = d.layer_states.get_mut(i) {
        st.name = new.trim().to_string();
    }
    ctx.s.touch();
    flow(Value::t())
}
