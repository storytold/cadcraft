//! Drawing access from LISP: `command`, system variables, entity lists (`entget`, `entmake`,
//! `entmod`, `entdel`, `entlast`, `entnext`, `handent`), selection sets (`ssget` and friends),
//! symbol tables (`tblsearch`, `tblnext`) and unit conversions (`rtos`, `angtos`, `distof`,
//! `angtof`).
//!
//! Entity lists use the DXF groups the file reader and writer use (through
//! [`crate::cmd::file::entity_codec`]), so what `entget` shows is what a DXF file holds, except
//! for angles, which are radians in a list and degrees in a file.

use std::sync::{Arc, Mutex};

use cadcraft_doc::{Entity, Handle, Space};
use cadcraft_dxf::Tag;
use cadcraft_geom::Vec2;

use super::LispError;
use super::builtins::args_err;
use super::eval::{Ctx, Flow, Need};
use super::value::{PickSet, Value, to_text};
use crate::{Input, Session};

fn arity(a: &[Value], min: usize, max: usize) -> Result<(), LispError> {
    if a.len() < min {
        return Err(args_err(true));
    }
    if a.len() > max {
        return Err(args_err(false));
    }
    Ok(())
}

fn ename(v: &Value) -> Result<Handle, LispError> {
    match v {
        Value::Ename(h) => Ok(*h),
        other => Err(LispError::bad_arg("lentityp", other)),
    }
}

fn string(v: &Value) -> Result<String, LispError> {
    match v {
        Value::Str(s) => Ok(s.to_string()),
        other => Err(LispError::bad_arg("stringp", other)),
    }
}

fn pickset(v: &Value) -> Result<PickSet, LispError> {
    match v {
        Value::PickSet(p) => Ok(p.clone()),
        other => Err(LispError::bad_arg("lselsetp", other)),
    }
}

fn new_pickset(ctx: &mut Ctx<'_>, items: Vec<Handle>) -> Value {
    let id = ctx.env.next_pickset;
    ctx.env.next_pickset = ctx.env.next_pickset.wrapping_add(1);
    Value::PickSet(PickSet { id, items: Arc::new(Mutex::new(items)) })
}

fn items(p: &PickSet) -> Vec<Handle> {
    p.items.lock().map(|v| v.clone()).unwrap_or_default()
}

// ---------------- command ----------------

/// `(command arg…)`: arguments go to the command line as typed input: a string starts a command
/// or answers a prompt ("" is Enter), a number or a point is entered, an entity name or a
/// selection set picks objects (a name, alone or with a point as `entsel` gives it, also answers
/// a prompt picking one object: [`Session::input_object`]), and `pause` waits for the user. A
/// command not finished by the arguments stays active for the next `command` call. No
/// arguments: cancel the active command.
pub fn command(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    if a.is_empty() {
        if ctx.s.running.is_some() {
            ctx.s.cancel();
        }
        return Ok(Flow::Value(Value::Nil));
    }
    for (i, arg) in a.iter().enumerate() {
        match arg {
            Value::Nil => {}
            Value::Str(s) if &**s == "\\" => {
                // `pause`: the user answers this prompt, then the rest goes in.
                if ctx.s.running.is_some() {
                    let rest = a[i + 1..].to_vec();
                    let f = super::eval::lookup(ctx.env, &super::value::Sym::new("COMMAND"));
                    return Ok(if rest.is_empty() { Flow::Wait(Need::Pause) } else { Flow::WaitThen(Need::Pause, f, rest) });
                }
            }
            Value::Str(s) => {
                let r = if ctx.s.running.is_none() {
                    if s.trim().is_empty() { Ok(()) } else { ctx.s.start(s) }
                } else if s.is_empty() {
                    ctx.s.input(Input::Enter)
                } else {
                    ctx.s.typed(s)
                };
                if let Err(e) = r {
                    ctx.s.echo(e.to_string());
                }
            }
            Value::Int(_) | Value::Real(_) => {
                if ctx.s.running.is_some() {
                    let text = match arg {
                        Value::Int(n) => n.to_string(),
                        Value::Real(r) => format!("{r}"),
                        _ => String::new(),
                    };
                    if let Err(e) = ctx.s.typed(&text) {
                        ctx.s.echo(e.to_string());
                    }
                }
            }
            Value::Ename(h) => named(ctx.s, *h, None),
            Value::PickSet(p) => feed(ctx.s, Input::Pick(items(p))),
            Value::Cons(_) => {
                if let Some(p) = arg.as_point() {
                    feed(ctx.s, Input::Point(Vec2::new(p[0], p[1])));
                } else if let Value::Ename(h) = arg.car() {
                    // An `entsel` result: (ename point).
                    named(ctx.s, h, arg.cdr().car().as_point().map(|p| Vec2::new(p[0], p[1])));
                } else {
                    return Err(LispError::bad_arg("command argument", arg));
                }
            }
            other => return Err(LispError::bad_arg("command argument", other)),
        }
    }
    Ok(Flow::Value(Value::Nil))
}

fn feed(s: &mut Session, input: Input) {
    if s.running.is_some()
        && let Err(e) = s.input(input)
    {
        s.echo(e.to_string());
    }
}

fn named(s: &mut Session, h: Handle, at: Option<Vec2>) {
    if s.running.is_some()
        && let Err(e) = s.input_object(h, at)
    {
        s.echo(e.to_string());
    }
}

// ---------------- system variables ----------------

/// A JSON value as LISP: integers, reals, strings, lists (points).
pub fn from_json(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(b) => Value::Int(i32::from(*b)),
        serde_json::Value::Number(n) => match n.as_i64().and_then(|i| i32::try_from(i).ok()) {
            Some(i) => Value::Int(i),
            None => Value::Real(n.as_f64().unwrap_or(0.0)),
        },
        serde_json::Value::String(s) => Value::str(s),
        serde_json::Value::Array(items) => Value::list(items.iter().map(from_json).collect::<Vec<_>>()),
        serde_json::Value::Object(_) => Value::str(&v.to_string()),
    }
}

/// A LISP value as JSON (for `setvar`), bounded so a shared cons structure cannot expand into
/// exponentially many owned array elements before the setter can reject it (CWE-400).
///
/// Largest number of elements it may expand, and the deepest nesting it follows.
const TO_JSON_BUDGET: u64 = 2_000_000;
const TO_JSON_MAX_DEPTH: usize = 256;

/// Convert a LISP value to owned JSON within element and nesting limits.
/// Return `None` before a shared cons structure can expand beyond the remaining budget.
fn to_json_bounded(v: &Value, depth: usize, budget: &mut u64) -> Option<serde_json::Value> {
    if depth > TO_JSON_MAX_DEPTH || *budget == 0 {
        return None;
    }
    *budget -= 1;
    Some(match v {
        Value::Nil => serde_json::Value::Null,
        Value::Int(i) => serde_json::json!(i),
        Value::Real(r) => serde_json::json!(r),
        Value::Str(s) => serde_json::json!(&**s),
        Value::Cons(_) => {
            let mut arr = Vec::new();
            for x in v.iter() {
                arr.push(to_json_bounded(&x, depth + 1, budget)?);
            }
            serde_json::Value::Array(arr)
        }
        other => serde_json::json!(to_text(other, false)),
    })
}

/// Angle variables CADCraft reports and takes in degrees, as the SETVAR command shows them, which
/// AutoLISP's `getvar` and `setvar` give and take in radians, as AutoCAD 2024's do: each of these
/// set to 30 by the SETVAR command, or to `(/ pi 6)` by `setvar`, reads 0.5235987755982987.
const RADIAN_VARS: [&str; 6] = ["ANGBASE", "CHAMFERD", "DIMJOGANG", "HPANG", "POLARANG", "SNAPANG"];

fn in_radians(name: &str) -> bool {
    RADIAN_VARS.iter().any(|v| v.eq_ignore_ascii_case(name.trim()))
}

pub fn getvar(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let name = string(&a[0])?;
    Ok(Flow::Value(match crate::sysvars::get(ctx.s, &name) {
        Some(serde_json::Value::Number(n)) if in_radians(&name) => Value::Real(n.as_f64().unwrap_or(0.0).to_radians()),
        v => v.map(|v| from_json(&v)).unwrap_or_default(),
    }))
}

/// Set a system variable after bounded JSON conversion, reporting invalid or oversized values as LISP errors.
pub fn setvar(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let name = string(&a[0])?;
    let mut budget: u64 = TO_JSON_BUDGET;
    let Some(json) = to_json_bounded(&a[1], 0, &mut budget) else {
        return Err(LispError::new(format!("AutoCAD variable setting rejected: {} value too large", to_text(&a[0], true))));
    };
    let json = match json {
        serde_json::Value::Number(n) if in_radians(&name) => serde_json::json!(n.as_f64().unwrap_or(0.0).to_degrees()),
        other => other,
    };
    match crate::sysvars::set(ctx.s, &name, &json) {
        Ok(()) => Ok(Flow::Value(a[1].clone())),
        Err(_) => Err(LispError::new(format!("AutoCAD variable setting rejected: {} {}", to_text(&a[0], true), to_text(&a[1], true)))),
    }
}

// ---------------- entity lists ----------------

/// Groups whose value is a point: 10–18, 110–112, 210, 1010–1013 (x), with y and z 10 and 20
/// higher.
fn point_group(code: i32) -> bool {
    (10..=18).contains(&code) || (110..=112).contains(&code) || code == 210 || (1010..=1013).contains(&code)
}

/// Groups 50–58 are angles: degrees in a DXF file, radians in entity lists (DXF Reference,
/// group code value types). `f` converts each one; an MTEXT's 50 after its column type (75) is
/// a column height.
fn convert_angles(kind: &str, tags: &mut [Tag], f: fn(f64) -> f64) {
    let mtext = kind.eq_ignore_ascii_case("MTEXT");
    for t in tags {
        if mtext && t.code == 75 {
            break;
        }
        if !(50..=58).contains(&t.code) {
            continue;
        }
        let v = match t.value {
            cadcraft_dxf::Value::Real(r) => r,
            cadcraft_dxf::Value::Int(i) => i as f64,
            _ => continue,
        };
        t.value = cadcraft_dxf::Value::Real(f(v));
    }
}

fn tag_value(t: &Tag) -> Value {
    match &t.value {
        cadcraft_dxf::Value::Str(s) | cadcraft_dxf::Value::Hex(s) => Value::str(s),
        cadcraft_dxf::Value::Real(r) => Value::Real(*r),
        cadcraft_dxf::Value::Int(i) => Value::Int(i32::try_from(*i).unwrap_or(if *i < 0 { i32::MIN } else { i32::MAX })),
        cadcraft_dxf::Value::Bool(b) => Value::Int(i32::from(*b)),
    }
}

/// DXF groups as an association list: points combined into lists `(10 x y z)`.
fn groups_to_list(tags: &[Tag]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(t) = tags.get(i) {
        if point_group(t.code) {
            let x = tag_value(t).as_number().unwrap_or(0.0);
            let y = tags.get(i + 1).filter(|n| n.code == t.code + 10).and_then(|n| tag_value(n).as_number());
            let z = tags.get(i + 2).filter(|n| n.code == t.code + 20 && y.is_some()).and_then(|n| tag_value(n).as_number());
            match (y, z) {
                (Some(y), Some(z)) => {
                    out.push(Value::list(vec![Value::Int(t.code), Value::Real(x), Value::Real(y), Value::Real(z)]));
                    i += 3;
                }
                (Some(y), None) => {
                    out.push(Value::list(vec![Value::Int(t.code), Value::Real(x), Value::Real(y)]));
                    i += 2;
                }
                _ => {
                    out.push(Value::cons(Value::Int(t.code), Value::Real(x)));
                    i += 1;
                }
            }
            continue;
        }
        out.push(Value::cons(Value::Int(t.code), tag_value(t)));
        i += 1;
    }
    out
}

/// An association list back to DXF groups (points split into x, y, z groups). The type
/// (`0`), the entity name (`-1`), the handle (`5`), the owner (`330`) and the layout (`410`) are
/// left out: they aren't the entity's to set.
fn list_to_groups(list: &Value) -> Result<(String, Vec<Tag>), LispError> {
    let mut kind = None;
    let mut tags = Vec::new();
    for item in list.iter() {
        let Value::Cons(c) = &item else { return Err(LispError::bad_arg("listp", &item)) };
        let Value::Int(code) = c.car else { return Err(LispError::bad_arg("fixnump", &c.car)) };
        let v = c.cdr.clone();
        match code {
            0 => {
                kind = Some(match &v {
                    Value::Str(s) => s.to_ascii_uppercase(),
                    other => return Err(LispError::bad_arg("stringp", other)),
                })
            }
            -1 | 5 | 330 | 360 | 410 | 102 => {}
            _ if point_group(code) && matches!(v, Value::Cons(_)) => {
                let p = v.as_point().ok_or_else(|| LispError::bad_arg("2D/3D point", &v))?;
                tags.push(Tag::f(code, p[0]));
                tags.push(Tag::f(code + 10, p[1]));
                tags.push(Tag::f(code + 20, p[2]));
            }
            _ => tags.push(match &v {
                Value::Int(i) => Tag::i(code, i64::from(*i)),
                Value::Real(r) => Tag::f(code, *r),
                Value::Str(s) => Tag::s(code, &**s),
                Value::Ename(h) => Tag::s(code, h.hex()),
                other => return Err(LispError::bad_arg("DXF group value", other)),
            }),
        }
    }
    Ok((kind.ok_or_else(|| LispError::new("bad DXF group: no entity type (0)"))?, tags))
}

fn codec() -> Result<&'static crate::cmd::file::EntityCodec, LispError> {
    crate::cmd::file::entity_codec().ok_or_else(|| LispError::new("entity lists are not available in this build"))
}

fn space_of(d: &cadcraft_doc::Drawing, h: Handle) -> Option<Space> {
    d.space_of(h)
}

pub fn entget(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 2)?;
    let h = ename(&a[0])?;
    let Ok(d) = ctx.s.doc() else { return Ok(Flow::Value(Value::Nil)) };
    let Some(e) = d.entity(h) else { return Ok(Flow::Value(Value::Nil)) };
    let (kind, mut tags) = (codec()?.groups)(d, e);
    convert_angles(&kind, &mut tags, f64::to_radians);
    let layout = match space_of(d, h) {
        Some(Space::Paper(name)) => name,
        _ => "Model".into(),
    };
    let mut items = vec![Value::cons(Value::Int(-1), Value::Ename(h)), Value::cons(Value::Int(0), Value::str(&kind))];
    let mut placed_layout = false;
    for v in groups_to_list(&tags) {
        // The layout name follows the AcDbEntity marker, as AutoCAD lists it.
        let after_marker = !placed_layout && v.car().as_number() == Some(100.0) && matches!(v.cdr(), Value::Str(ref s) if &**s == "AcDbEntity");
        items.push(v);
        if after_marker {
            items.push(Value::cons(Value::Int(410), Value::str(&layout)));
            placed_layout = true;
        }
    }
    Ok(Flow::Value(Value::list(items)))
}

/// The entity a list describes, as the reader makes it; a missing layer (8) is the current one.
fn entity_from_list(ctx: &Ctx<'_>, list: &Value) -> Result<(String, cadcraft_doc::Common, cadcraft_doc::EntityKind), LispError> {
    let (kind, mut tags) = list_to_groups(list)?;
    convert_angles(&kind, &mut tags, f64::to_degrees);
    if !tags.iter().any(|t| t.code == 8) {
        let layer = crate::sysvars::get(ctx.s, "CLAYER").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| "0".into());
        tags.insert(0, Tag::s(8, layer));
    }
    match (codec()?.entity)(&kind, &tags) {
        // A type the reader only carries through (an unknown or invalid name) isn't made.
        Some((_, cadcraft_doc::EntityKind::Unknown(_))) | None => Err(LispError::new(format!("bad DXF group: {kind} can't be made from this list"))),
        Some((common, k)) => Ok((kind, common, k)),
    }
}

pub fn entmake(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 1)?;
    let Some(list) = a.first().filter(|v| !v.is_nil()) else { return Ok(Flow::Value(Value::Nil)) };
    // A block definition is a BLOCK list, its entities, then an ENDBLK list (#1662).
    match list_to_groups(list) {
        Ok((k, tags)) if k == "BLOCK" => return Ok(Flow::Value(begin_block(ctx, list, &tags))),
        Ok((k, _)) if k == "ENDBLK" => return Ok(Flow::Value(end_block(ctx))),
        _ => {}
    }
    let (_, common, kind) = match entity_from_list(ctx, list) {
        Ok(x) => x,
        Err(_) => return Ok(Flow::Value(Value::Nil)),
    };
    if let Some((_, ents)) = ctx.env.pending_block.as_mut() {
        // Into the block being defined; it is added to the drawing, with handles and the layers
        // its entities name, at ENDBLK (a refused or dropped definition leaves nothing behind).
        if ents.len() >= MAX_PENDING_BLOCK_ENTITIES {
            return Ok(Flow::Value(Value::Nil));
        }
        ents.push(Entity { handle: Handle(0), common, kind });
        return Ok(Flow::Value(list.clone()));
    }
    let space = ctx.s.space();
    let Ok(d) = ctx.s.doc_mut() else { return Ok(Flow::Value(Value::Nil)) };
    // A layer entmake names that doesn't exist yet is created.
    d.ensure_layer(&common.layer);
    match d.add(&space, common, kind) {
        Ok(_) => {
            ctx.s.touch();
            Ok(Flow::Value(list.clone()))
        }
        Err(_) => Ok(Flow::Value(Value::Nil)),
    }
}

/// `entmake` of a BLOCK list (2 name, 70 flags, 10 base point, 4 description): begin a block
/// definition. Flag 1 makes an anonymous block (`*U…`, named at ENDBLK); xrefs (flag 4) can't be
/// made this way. nil, and the definition already begun kept, when one is open.
fn begin_block(ctx: &mut Ctx<'_>, list: &Value, tags: &[Tag]) -> Value {
    if ctx.env.pending_block.is_some() {
        return Value::Nil;
    }
    let group = |code: i32| tags.iter().find(|t| t.code == code);
    let flags = group(70).map(Tag::i64).unwrap_or(0);
    let name = group(2).map(|t| t.str().trim().to_string()).unwrap_or_default();
    let anonymous = flags & 1 != 0;
    if flags & 4 != 0 || !(anonymous && name.starts_with('*') || crate::cmd::blocks::valid_block_name(&name)) {
        return Value::Nil;
    }
    let mut b = cadcraft_doc::Block::new(&name);
    b.anonymous = anonymous;
    b.base = cadcraft_geom::Vec3::new(
        group(10).map(Tag::f64).unwrap_or(0.0),
        group(20).map(Tag::f64).unwrap_or(0.0),
        group(30).map(Tag::f64).unwrap_or(0.0),
    );
    if !b.base.is_finite() {
        return Value::Nil;
    }
    b.description = group(4).map(Tag::str).unwrap_or_default();
    ctx.env.pending_block = Some((b, Vec::new()));
    list.clone()
}

/// The most entities one `entmake` block definition takes (a runaway loop can't fill memory).
const MAX_PENDING_BLOCK_ENTITIES: usize = 1_000_000;

/// `entmake` of an ENDBLK list: add the block begun to the drawing (redefining one of the same
/// name) and return its name, generated for an anonymous block. nil without one begun, or when
/// its entities reference the block itself.
fn end_block(ctx: &mut Ctx<'_>) -> Value {
    let Some((mut b, ents)) = ctx.env.pending_block.take() else { return Value::Nil };
    let Ok(d) = ctx.s.doc_mut() else { return Value::Nil };
    if b.anonymous {
        // Its kind letter (`*U`, `*X`…) is kept; a name that isn't one gets `*U`.
        let prefix: String = b.name.chars().take(2).collect();
        let prefix = if prefix.chars().count() == 2 && prefix.starts_with('*') { prefix.as_str() } else { "*U" };
        b.name = d.anonymous_block_name(prefix);
    } else {
        // Block names are case-insensitive: a redefinition keeps the existing name.
        if let Some(existing) = d.blocks.keys().find(|k| k.eq_ignore_ascii_case(&b.name)) {
            b.name = existing.clone();
        }
        // As BLOCK: a block being edited in place isn't redefined until REFCLOSE.
        if d.refedit.as_ref().is_some_and(|r| r.block.eq_ignore_ascii_case(&b.name)) {
            return Value::Nil;
        }
    }
    // Named or anonymous (a routine can know the name an anonymous block will get), a block
    // that inserts itself isn't added.
    if crate::cmd::blocks::references_block(d, &ents, &b.name) {
        return Value::Nil;
    }
    for mut e in ents {
        d.ensure_layer(&e.common.layer);
        e.handle = d.new_handle();
        b.entities.push(e);
    }
    let name = b.name.clone();
    d.blocks.insert(name.clone(), Arc::new(b));
    ctx.s.touch();
    Value::str(&name)
}

pub fn entmod(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let list = &a[0];
    let Some(Value::Ename(h)) = list.iter().find(|v| v.car().as_number() == Some(-1.0)).map(|v| v.cdr()) else {
        return Err(LispError::bad_arg("lentityp", &Value::Nil));
    };
    let Some(old) = ctx.s.doc().ok().and_then(|d| d.entity(h)).cloned() else { return Ok(Flow::Value(Value::Nil)) };
    let (kind_name, common, kind) = match entity_from_list(ctx, list) {
        Ok(x) => x,
        Err(_) => return Ok(Flow::Value(Value::Nil)),
    };
    // entmod changes an entity, never its type.
    if !kind_name.eq_ignore_ascii_case(old.kind.dxf_name()) {
        return Ok(Flow::Value(Value::Nil));
    }
    let Ok(d) = ctx.s.doc_mut() else { return Ok(Flow::Value(Value::Nil)) };
    d.ensure_layer(&common.layer);
    let ok = d
        .modify_entity(h, |e| {
            e.common = common;
            e.kind = kind;
        })
        .is_ok();
    if ok {
        ctx.s.touch();
    }
    Ok(Flow::Value(if ok { list.clone() } else { Value::Nil }))
}

/// `(entupd ename)`: redraw an entity (the canvas redraws on its own).
pub fn entupd(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let h = ename(&a[0])?;
    ctx.s.touch();
    Ok(Flow::Value(if ctx.s.doc().is_ok_and(|d| d.entity(h).is_some()) { Value::Ename(h) } else { Value::Nil }))
}

/// `(entdel ename)`: delete an entity, or bring back one this routine's drawing deleted.
pub fn entdel(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let h = ename(&a[0])?;
    let Ok(d) = ctx.s.doc_mut() else { return Ok(Flow::Value(Value::Nil)) };
    if let Some(space) = d.space_of(h) {
        if let Some(e) = d.remove_entity(h) {
            ctx.env.deleted.insert(h, (space, e));
            ctx.s.touch();
            return Ok(Flow::Value(Value::Ename(h)));
        }
        return Ok(Flow::Value(Value::Nil));
    }
    match ctx.env.deleted.remove(&h) {
        Some((space, e)) => {
            let restored = d.space_mut(&space).map(|store| store.push(Entity::clone(&e))).is_some();
            ctx.s.touch();
            Ok(Flow::Value(if restored { Value::Ename(h) } else { Value::Nil }))
        }
        None => Ok(Flow::Value(Value::Nil)),
    }
}

/// Every entity's handle in creation order (model space and layouts), cached per drawing.
fn handles_in_order(ctx: &mut Ctx<'_>) -> Arc<Vec<Handle>> {
    let Ok(st) = ctx.s.state() else { return Arc::default() };
    let key = Arc::as_ptr(&st.doc) as usize;
    if let Some((k, v)) = &ctx.env.order
        && *k == key
    {
        return v.clone();
    }
    let d = &st.doc;
    let mut v: Vec<Handle> = d.model.handles();
    for l in &d.layouts {
        v.extend(l.entities.handles());
    }
    v.sort_unstable();
    let v = Arc::new(v);
    ctx.env.order = Some((key, v.clone()));
    v
}

pub fn entlast(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 0)?;
    Ok(Flow::Value(handles_in_order(ctx).last().map(|h| Value::Ename(*h)).unwrap_or_default()))
}

pub fn entnext(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 1)?;
    let order = handles_in_order(ctx);
    let next = match a.first() {
        None | Some(Value::Nil) => order.first().copied(),
        Some(v) => {
            let h = ename(v)?;
            let i = order.partition_point(|x| *x <= h);
            order.get(i).copied()
        }
    };
    Ok(Flow::Value(next.map(Value::Ename).unwrap_or_default()))
}

pub fn handent(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let s = string(&a[0])?;
    let h = Handle::parse_hex(&s).filter(|h| ctx.s.doc().is_ok_and(|d| d.entity(*h).is_some()));
    Ok(Flow::Value(h.map(Value::Ename).unwrap_or_default()))
}

// ---------------- selection sets ----------------

/// `(ssget [mode] [pt1 [pt2]] [filter])`. Modes: "X" every entity, "L" the last, "P" the
/// previous selection, "I" the implied (pickfirst) one, "W"/"C" window and crossing with two
/// points, "F" fence, "WP"/"CP" polygons with a point list. With no mode the pickfirst
/// selection, or the user selects objects. The filter is a list of DXF groups (`-4` operators,
/// wildcards). nil when nothing matches.
pub fn ssget(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    let mut args = a.into_iter().peekable();
    let mode = match args.peek() {
        Some(Value::Str(s)) => {
            let m = s.trim_start_matches(['_', '+', '.', ':']).to_ascii_uppercase();
            args.next();
            Some(m)
        }
        _ => None,
    };
    let rest: Vec<Value> = args.collect();
    // A trailing list of dotted pairs is the filter.
    let (pts, filter) = match rest.last() {
        Some(f @ Value::Cons(_)) if f.car().car().as_number().is_some() && f.car().as_point().is_none() => {
            (rest[..rest.len() - 1].to_vec(), Some(f.clone()))
        }
        _ => (rest, None),
    };
    let point = |i: usize| -> Result<Vec2, LispError> {
        let v = pts.get(i).ok_or_else(|| args_err(true))?;
        let p = v.as_point().ok_or_else(|| LispError::bad_arg("2D/3D point", v))?;
        Ok(Vec2::new(p[0], p[1]))
    };
    let s = &mut *ctx.s;
    let all = |s: &Session, all_spaces: bool| -> Vec<Handle> {
        let Ok(d) = s.doc() else { return Vec::new() };
        if all_spaces {
            let mut v = d.model.handles();
            for l in &d.layouts {
                v.extend(l.entities.handles());
            }
            v
        } else {
            d.space(&s.state().map(|st| st.edit_space()).unwrap_or(Space::Model)).map(|st| st.handles()).unwrap_or_default()
        }
    };
    let space = s.state().map(|st| st.edit_space()).unwrap_or(Space::Model);
    let mut found: Vec<Handle> = match mode.as_deref() {
        Some("X") => all(s, true),
        Some("L") => {
            let order = handles_in_order(ctx);
            order.last().copied().into_iter().collect()
        }
        Some("P") => ctx.s.state().map(|st| st.previous_selection.clone()).unwrap_or_default(),
        Some("I") => ctx.s.selection(),
        Some("W") | Some("C") => {
            let (p1, p2) = (point(0)?, point(1)?);
            let s = &*ctx.s;
            let Ok(d) = s.doc() else { return Ok(Flow::Value(Value::Nil)) };
            crate::select::select_window(d, &space, cadcraft_geom::Bounds2::from_points([p1, p2]), mode.as_deref() == Some("C"))
        }
        Some("F") | Some("WP") | Some("CP") => {
            let poly: Vec<Vec2> =
                pts.first().map(|l| l.iter().filter_map(|p| p.as_point().map(|q| Vec2::new(q[0], q[1]))).collect()).unwrap_or_default();
            let s = &*ctx.s;
            let Ok(d) = s.doc() else { return Ok(Flow::Value(Value::Nil)) };
            match mode.as_deref() {
                Some("F") => crate::select::select_fence(d, &space, &poly),
                m => crate::select::select_polygon(d, &space, &poly, m == Some("CP")),
            }
        }
        Some(":S") | Some("A") | None => {
            let implied = ctx.s.selection();
            if !implied.is_empty() {
                implied
            } else if pts.is_empty() {
                // The user selects objects; the result arrives when SELECT ends.
                return Ok(Flow::Value(Value::Nil)).and_then(|_| interactive_ssget(ctx, filter));
            } else {
                // A point: the object there.
                let p = point(0)?;
                let s = &*ctx.s;
                let ap = s.settings.pickbox.max(1.0) * s.pixel_size();
                let Ok(d) = s.doc() else { return Ok(Flow::Value(Value::Nil)) };
                crate::select::pick(d, &space, p, ap).into_iter().collect()
            }
        }
        Some(m) => return Err(LispError::new(format!("bad argument value: ssget mode {m}"))),
    };
    if let Some(f) = &filter {
        let d = ctx.s.doc().map_err(|e| LispError::new(e.to_string()))?;
        let f = parse_filter(f)?;
        found.retain(|h| d.entity(*h).is_some_and(|e| matches_filter(d, e, &f)));
    }
    if found.is_empty() {
        return Ok(Flow::Value(Value::Nil));
    }
    Ok(Flow::Value(new_pickset(ctx, found)))
}

/// `ssget` asking the user: the SELECT command collects the objects.
fn interactive_ssget(ctx: &mut Ctx<'_>, filter: Option<Value>) -> Result<Flow, LispError> {
    if ctx.s.running.is_some() {
        return Ok(Flow::Value(Value::Nil));
    }
    if let Err(e) = ctx.s.start("select") {
        return Err(LispError::new(e.to_string()));
    }
    let f = super::eval::lookup(ctx.env, &super::value::Sym::new("SSGET"));
    // When SELECT ends, its objects are the previous selection: (ssget "P" [filter]).
    let mut args = vec![Value::str("P")];
    args.extend(filter);
    Ok(Flow::WaitThen(Need::Pause, f, args))
}

/// A parsed `ssget` filter.
enum Filter {
    /// One group to test, with the relational operator of a preceding `(-4 . "op")`.
    Group {
        code: i32,
        value: Value,
        op: String,
    },
    And(Vec<Filter>),
    Or(Vec<Filter>),
    Xor(Vec<Filter>),
    Not(Box<Filter>),
}

fn parse_filter(list: &Value) -> Result<Filter, LispError> {
    let items = list.to_vec();
    let mut i = 0;
    let mut out = Vec::new();
    while i < items.len() {
        out.push(parse_filter_item(&items, &mut i, 0)?);
    }
    Ok(Filter::And(out))
}

fn parse_filter_item(items: &[Value], i: &mut usize, depth: usize) -> Result<Filter, LispError> {
    if depth > 64 {
        return Err(LispError::new("bad ssget filter: nested too deeply"));
    }
    let Some(item) = items.get(*i) else { return Err(LispError::new("bad ssget filter")) };
    *i += 1;
    let code = item.car().as_number().map(|c| c as i32).ok_or_else(|| LispError::bad_arg("ssget filter", item))?;
    let value = item.cdr();
    if code != -4 {
        return Ok(Filter::Group { code, value, op: "=".into() });
    }
    let op = match &value {
        Value::Str(s) => s.to_ascii_uppercase(),
        other => return Err(LispError::bad_arg("stringp", other)),
    };
    let group = |items: &[Value], i: &mut usize, depth: usize, end: &str| -> Result<Vec<Filter>, LispError> {
        let mut v = Vec::new();
        loop {
            let Some(next) = items.get(*i) else { return Err(LispError::new(format!("bad ssget filter: no {end}"))) };
            if next.car().as_number() == Some(-4.0) && matches!(next.cdr(), Value::Str(ref s) if s.eq_ignore_ascii_case(end)) {
                *i += 1;
                return Ok(v);
            }
            v.push(parse_filter_item(items, i, depth + 1)?);
        }
    };
    Ok(match op.as_str() {
        "<AND" => Filter::And(group(items, i, depth, "AND>")?),
        "<OR" => Filter::Or(group(items, i, depth, "OR>")?),
        "<XOR" => Filter::Xor(group(items, i, depth, "XOR>")?),
        "<NOT" => {
            let mut v = group(items, i, depth, "NOT>")?;
            Filter::Not(Box::new(if v.len() == 1 { v.remove(0) } else { Filter::And(v) }))
        }
        _ => {
            // A relational operator applies to the next group.
            let Some(next) = items.get(*i) else { return Err(LispError::new("bad ssget filter")) };
            *i += 1;
            let code = next.car().as_number().map(|c| c as i32).ok_or_else(|| LispError::bad_arg("ssget filter", next))?;
            Filter::Group { code, value: next.cdr(), op }
        }
    })
}

fn matches_filter(d: &cadcraft_doc::Drawing, e: &Entity, f: &Filter) -> bool {
    let groups = || match crate::cmd::file::entity_codec() {
        Some(c) => {
            let (kind, mut tags) = (c.groups)(d, e);
            convert_angles(&kind, &mut tags, f64::to_radians);
            let mut l = vec![Value::cons(Value::Int(0), Value::str(&kind))];
            l.extend(groups_to_list(&tags));
            l
        }
        None => vec![Value::cons(Value::Int(0), Value::str(e.kind.dxf_name())), Value::cons(Value::Int(8), Value::str(&e.common.layer))],
    };
    let groups = groups();
    eval_filter(&groups, f)
}

fn eval_filter(groups: &[Value], f: &Filter) -> bool {
    match f {
        Filter::And(v) => v.iter().all(|x| eval_filter(groups, x)),
        Filter::Or(v) => v.iter().any(|x| eval_filter(groups, x)),
        Filter::Xor(v) => v.iter().filter(|x| eval_filter(groups, x)).count() == 1,
        Filter::Not(x) => !eval_filter(groups, x),
        Filter::Group { code, value, op } => {
            let found: Vec<Value> = groups.iter().filter(|g| g.car().as_number() == Some(f64::from(*code))).map(Value::cdr).collect();
            if found.is_empty() {
                // A group the entity doesn't have: layer 0 / colour BYLAYER defaults count.
                return match (code, value) {
                    (62, Value::Int(256)) => op == "=",
                    _ => false,
                };
            }
            found.iter().any(|have| group_matches(have, value, op))
        }
    }
}

fn group_matches(have: &Value, want: &Value, op: &str) -> bool {
    if op == "*" {
        return true;
    }
    match (have, want) {
        (Value::Str(h), Value::Str(w)) => {
            // Strings match as wildcards, case-insensitively (type and layer names).
            let hit = super::builtins::wild_match(&h.to_uppercase(), &w.to_uppercase());
            if op == "!=" || op == "/=" || op == "<>" { !hit } else { hit }
        }
        _ => {
            if let (Some(h), Some(w)) = (have.as_number(), want.as_number()) {
                return compare(h, w, op);
            }
            if let (Some(h), Some(w)) = (have.as_point(), want.as_point()) {
                // Points: an operator per coordinate ("<,>,*") or one for all.
                let ops: Vec<&str> = op.split(',').map(str::trim).collect();
                return (0..3).all(|i| compare(h[i], w[i], ops.get(i).or(ops.first()).copied().unwrap_or("=")));
            }
            super::value::equal(have, want, 0.0)
        }
    }
}

fn compare(h: f64, w: f64, op: &str) -> bool {
    match op {
        "=" => h == w,
        "!=" | "/=" | "<>" => h != w,
        "<" => h < w,
        "<=" => h <= w,
        ">" => h > w,
        ">=" => h >= w,
        "&" => (h as i64) & (w as i64) != 0,
        "&=" => (h as i64) & (w as i64) == (w as i64),
        "*" => true,
        _ => h == w,
    }
}

pub fn ssadd(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 2)?;
    match (a.first(), a.get(1)) {
        (None, _) => Ok(Flow::Value(new_pickset(ctx, Vec::new()))),
        (Some(e), None) => {
            let h = ename(e)?;
            Ok(Flow::Value(new_pickset(ctx, vec![h])))
        }
        (Some(e), Some(ss)) => {
            let h = ename(e)?;
            let p = pickset(ss)?;
            if let Ok(mut v) = p.items.lock()
                && !v.contains(&h)
            {
                v.push(h);
            }
            Ok(Flow::Value(ss.clone()))
        }
    }
}

pub fn ssdel(_ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let h = ename(&a[0])?;
    let p = pickset(&a[1])?;
    let removed = p.items.lock().map(|mut v| {
        let n = v.len();
        v.retain(|x| *x != h);
        n != v.len()
    });
    Ok(Flow::Value(if removed.unwrap_or(false) { a[1].clone() } else { Value::Nil }))
}

pub fn sslength(_ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    Ok(Flow::Value(Value::Int(i32::try_from(items(&pickset(&a[0])?).len()).unwrap_or(i32::MAX))))
}

pub fn ssname(_ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let p = pickset(&a[0])?;
    let i = a[1].as_number().ok_or_else(|| LispError::bad_arg("numberp", &a[1]))?;
    if i < 0.0 {
        return Ok(Flow::Value(Value::Nil));
    }
    Ok(Flow::Value(items(&p).get(i as usize).map(|h| Value::Ename(*h)).unwrap_or_default()))
}

pub fn ssmemb(_ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let h = ename(&a[0])?;
    Ok(Flow::Value(if items(&pickset(&a[1])?).contains(&h) { Value::Ename(h) } else { Value::Nil }))
}

pub fn sssetfirst(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 2)?;
    let sel = match a.get(1) {
        Some(Value::PickSet(p)) => items(p),
        _ => Vec::new(),
    };
    ctx.s.set_selection(sel);
    Ok(Flow::Value(Value::list(a)))
}

pub fn ssgetfirst(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 0)?;
    let sel = ctx.s.selection();
    let ss = if sel.is_empty() { Value::Nil } else { new_pickset(ctx, sel) };
    Ok(Flow::Value(Value::list(vec![Value::Nil, ss])))
}

// ---------------- symbol tables ----------------

fn color_index(c: cadcraft_color::Color) -> i32 {
    match c {
        cadcraft_color::Color::Index(i) => i32::from(i),
        cadcraft_color::Color::ByBlock => 0,
        cadcraft_color::Color::ByLayer => 256,
        cadcraft_color::Color::True(_) => 7,
    }
}

/// The records of a symbol table, as `tblsearch` lists them.
fn table_records(d: &cadcraft_doc::Drawing, table: &str) -> Vec<Value> {
    let pair = |c: i32, v: Value| Value::cons(Value::Int(c), v);
    match table {
        "LAYER" => d
            .layers
            .iter()
            .map(|l| {
                let flags = i32::from(l.frozen) | (i32::from(l.locked) << 2);
                let color = color_index(l.color);
                Value::list(vec![
                    pair(0, Value::str("LAYER")),
                    pair(2, Value::str(&l.name)),
                    pair(70, Value::Int(flags)),
                    pair(62, Value::Int(if l.on { color } else { -color })),
                    pair(6, Value::str(&l.linetype)),
                ])
            })
            .collect(),
        "LTYPE" => d
            .linetypes
            .iter()
            .map(|l| {
                Value::list(vec![
                    pair(0, Value::str("LTYPE")),
                    pair(2, Value::str(&l.name)),
                    pair(70, Value::Int(0)),
                    pair(3, Value::str(&l.description)),
                ])
            })
            .collect(),
        "STYLE" => d
            .text_styles
            .iter()
            .map(|s| {
                Value::list(vec![
                    pair(0, Value::str("STYLE")),
                    pair(2, Value::str(&s.name)),
                    pair(70, Value::Int(0)),
                    pair(40, Value::Real(s.height)),
                    pair(41, Value::Real(s.width_factor)),
                    pair(3, Value::str(&s.font)),
                ])
            })
            .collect(),
        "BLOCK" => d
            .blocks
            .values()
            .map(|b| {
                Value::list(vec![
                    pair(0, Value::str("BLOCK")),
                    pair(2, Value::str(&b.name)),
                    pair(70, Value::Int(0)),
                    Value::list(vec![Value::Int(10), Value::Real(b.base.x), Value::Real(b.base.y), Value::Real(b.base.z)]),
                ])
            })
            .collect(),
        "DIMSTYLE" => d
            .dim_styles
            .iter()
            .map(|s| Value::list(vec![pair(0, Value::str("DIMSTYLE")), pair(2, Value::str(&s.name)), pair(70, Value::Int(0))]))
            .collect(),
        "APPID" => vec![Value::list(vec![pair(0, Value::str("APPID")), pair(2, Value::str("ACAD")), pair(70, Value::Int(0))])],
        _ => Vec::new(),
    }
}

fn record_name(r: &Value) -> String {
    r.iter().find(|p| p.car().as_number() == Some(2.0)).map(|p| to_text(&p.cdr(), false)).unwrap_or_default()
}

pub fn tblsearch(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 3)?;
    let table = string(&a[0])?.to_ascii_uppercase();
    let name = string(&a[1])?;
    let Ok(d) = ctx.s.doc() else { return Ok(Flow::Value(Value::Nil)) };
    let records = table_records(d, &table);
    let found = records.iter().position(|r| record_name(r).eq_ignore_ascii_case(&name));
    if let Some(i) = found
        && a.get(2).is_some_and(|v| !v.is_nil())
    {
        ctx.env.tblnext.insert(table.clone(), i + 1);
    }
    Ok(Flow::Value(found.and_then(|i| records.get(i).cloned()).unwrap_or_default()))
}

pub fn tblnext(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 2)?;
    let table = string(&a[0])?.to_ascii_uppercase();
    let rewind = a.get(1).is_some_and(|v| !v.is_nil());
    let Ok(d) = ctx.s.doc() else { return Ok(Flow::Value(Value::Nil)) };
    let records = table_records(d, &table);
    let i = if rewind { 0 } else { ctx.env.tblnext.get(&table).copied().unwrap_or(0) };
    ctx.env.tblnext.insert(table, i + 1);
    Ok(Flow::Value(records.get(i).cloned().unwrap_or_default()))
}

// ---------------- units ----------------

fn int_var(s: &Session, name: &str, default: i64) -> i64 {
    crate::sysvars::get(s, name).and_then(|v| v.as_i64()).unwrap_or(default)
}

/// `(rtos number [mode [precision]])`: as LUNITS/LUPREC by default; decimal results honour
/// DIMZIN's zero suppression (4 leading, 8 trailing).
pub fn rtos(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 3)?;
    let v = a[0].as_number().ok_or_else(|| LispError::bad_arg("numberp", &a[0]))?;
    let mode = match a.get(1) {
        Some(Value::Int(m)) => i64::from(*m),
        _ => int_var(ctx.s, "LUNITS", 2),
    };
    let prec = match a.get(2) {
        Some(Value::Int(p)) => i64::from(*p),
        _ => int_var(ctx.s, "LUPREC", 4),
    };
    if !(1..=5).contains(&mode) {
        return Err(LispError::new(format!("bad argument value: rtos mode {mode}")));
    }
    let text = if mode == 2 {
        let dimzin = int_var(ctx.s, "DIMZIN", 0);
        decimal(v, prec.clamp(0, 8) as usize, dimzin)
    } else {
        crate::units::format_distance(v, mode, prec)
    };
    Ok(Flow::Value(Value::str(&text)))
}

fn decimal(v: f64, prec: usize, dimzin: i64) -> String {
    let mut s = crate::units::fixed(v, prec);
    if s.starts_with("-0") && s.trim_start_matches(['-', '0', '.']).is_empty() {
        s = s.trim_start_matches('-').to_string();
    }
    if dimzin & 8 != 0 && s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if dimzin & 4 != 0 {
        if let Some(r) = s.strip_prefix("0.") {
            s = format!(".{r}");
        } else if let Some(r) = s.strip_prefix("-0.") {
            s = format!("-.{r}");
        }
    }
    s
}

pub fn angtos(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 3)?;
    let v = a[0].as_number().ok_or_else(|| LispError::bad_arg("numberp", &a[0]))?;
    let mode = match a.get(1) {
        Some(Value::Int(m)) => i64::from(*m),
        _ => int_var(ctx.s, "AUNITS", 0),
    };
    let prec = match a.get(2) {
        Some(Value::Int(p)) => i64::from(*p),
        _ => int_var(ctx.s, "AUPREC", 0),
    };
    Ok(Flow::Value(Value::str(&crate::units::format_angle(v, mode, prec))))
}

pub fn distof(_ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 2)?;
    let s = string(&a[0])?;
    Ok(Flow::Value(crate::units::parse_distance(&s).map(Value::Real).unwrap_or_default()))
}

pub fn angtof(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 2)?;
    let s = string(&a[0])?;
    let mut settings = ctx.s.angle_settings();
    // AUNITS 0..=4; any other units argument is an error in AutoCAD, nil here.
    match a.get(1) {
        None | Some(Value::Nil) => {}
        Some(Value::Int(u)) if (0..=4).contains(u) => settings.aunits = i64::from(*u),
        Some(_) => return Ok(Flow::Value(Value::Nil)),
    }
    Ok(Flow::Value(settings.direction(&s).map(Value::Real).unwrap_or_default()))
}

/// `(trans pt from to [disp])`: points are in the world system throughout (no UCS yet).
pub fn trans(_ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 3, 4)?;
    let p = a[0].as_point().ok_or_else(|| LispError::bad_arg("2D/3D point", &a[0]))?;
    Ok(Flow::Value(Value::point(&p)))
}

/// What `(load name)` reads: a file loaded through APPLOAD (the web), or one on disk, with
/// `.lsp` added when the name has no extension.
pub fn load_source(s: &Session, name: &str) -> Option<String> {
    if let Some(t) = s.lisp_file(name) {
        return Some(t);
    }
    let candidates =
        if std::path::Path::new(name).extension().is_none() { vec![format!("{name}.lsp"), name.to_string()] } else { vec![name.to_string()] };
    // As given (a path, or relative to the working folder), then in the support folder.
    let support =
        s.lisp_support_dir().map(|d| candidates.iter().map(|c| d.join(c).to_string_lossy().into_owned()).collect::<Vec<_>>()).unwrap_or_default();
    candidates.iter().chain(&support).find_map(super::builtins::read_text_file)
}
