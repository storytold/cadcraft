//! Non-graphic objects from LISP: the named object dictionary and its dictionaries, table records
//! and layouts (`namedobjdict`, `dictsearch`, `dictnext`, `tblobjname`, and `handent` and
//! `entget` of such an object).
//!
//! CADCraft writes these objects from its own model and gives them handles only in a file, so
//! they are read from the drawing's records as a DXF file of it holds them
//! ([`crate::cmd::file::EntityCodec::objects`]). Each object gets an entity name that stays the
//! same for the drawing's session: the handle the drawing's file gave it, when the drawing was
//! read from a file that had it, otherwise a name above any file's handles. An object is known by
//! its role (a layer's name, a dictionary key, a layout's name), so its name follows it across
//! edits; one without a role (an object of a deeper dictionary) is known by its handle in the
//! written records, so its name holds only until the drawing changes (then it names nothing). Lists of these objects are
//! read-only: `entmod` and `entmake` don't take them.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use cadcraft_doc::foreign::ForeignKey;
use cadcraft_doc::{Drawing, Handle};
use cadcraft_dxf::Tag;

use super::LispError;
use super::drawing::{arity, ename, groups_to_list, string};
use super::eval::{Ctx, Flow};
use super::value::Value;

/// The first entity name given to an object its drawing's file didn't name.
const OWN_NAMES: u64 = 0x7F00_0000_0000_0000;
/// The kind of a key for an object without a role: its written handle is the name.
const BY_HANDLE: &str = "@";

/// The entity names given to non-graphic objects, and what they are read from.
#[derive(Default)]
pub struct Names {
    by_key: HashMap<ForeignKey, Handle>,
    by_name: HashMap<Handle, ForeignKey>,
    next: u64,
    /// The written records of the drawing they were taken from (held weakly, so its address
    /// can't be reused for another drawing while the records are kept).
    records: Option<(Weak<Drawing>, Arc<Records>)>,
    /// `dictnext`'s place in each dictionary.
    dictnext: HashMap<Handle, usize>,
}

/// A drawing's non-graphic records as written.
struct Records {
    list: Vec<(String, Vec<Tag>, Option<ForeignKey>)>,
    /// Written handle → index in `list`.
    by_handle: HashMap<String, usize>,
    /// Role → written handle.
    by_role: HashMap<ForeignKey, String>,
}

impl Records {
    fn key(&self, written: &str) -> Option<ForeignKey> {
        let i = *self.by_handle.get(written)?;
        let (_, _, role) = self.list.get(i)?;
        Some(role.clone().unwrap_or_else(|| (BY_HANDLE.into(), written.to_string())))
    }

    fn written(&self, key: &ForeignKey) -> Option<&str> {
        if key.0 == BY_HANDLE {
            return self.by_handle.get_key_value(&key.1).map(|(h, _)| h.as_str());
        }
        self.by_role.get(key).map(String::as_str)
    }

    fn record(&self, written: &str) -> Option<&(String, Vec<Tag>, Option<ForeignKey>)> {
        self.list.get(*self.by_handle.get(written)?)
    }
}

/// A record's own handle: group 5, or 105 for a DIMSTYLE (before any xdata).
fn own_handle(tags: &[Tag]) -> Option<&Tag> {
    tags.iter().take_while(|t| t.code < 1000).find(|t| matches!(t.code, 5 | 105))
}

/// A handle as the records spell it (upper-case hex without leading zeros).
fn norm(hex: &str) -> Option<String> {
    Handle::parse_hex(hex.trim()).map(Handle::hex)
}

fn records(names: &mut Names, d: &Arc<Drawing>) -> Option<Arc<Records>> {
    if let Some((w, r)) = &names.records
        && std::ptr::eq(w.as_ptr(), Arc::as_ptr(d))
    {
        return Some(r.clone());
    }
    let codec = crate::cmd::file::entity_codec()?;
    let list = (codec.objects)(d);
    let mut by_handle = HashMap::new();
    let mut by_role = HashMap::new();
    for (i, (_, tags, role)) in list.iter().enumerate() {
        let Some(h) = own_handle(tags).and_then(|t| norm(&t.str())) else { continue };
        if let Some(role) = role {
            by_role.entry(role.clone()).or_insert_with(|| h.clone());
        }
        by_handle.entry(h).or_insert(i);
    }
    let r = Arc::new(Records { list, by_handle, by_role });
    // Written handles change with the drawing: names given by them no longer hold.
    names.by_key.retain(|k, _| k.0 != BY_HANDLE);
    names.by_name.retain(|_, k| k.0 != BY_HANDLE);
    names.records = Some((Arc::downgrade(d), r.clone()));
    Some(r)
}

/// The entity name of the object with `key`, given out now if it has none yet.
fn name_of(names: &mut Names, d: &Drawing, key: &ForeignKey) -> Option<Handle> {
    if let Some(h) = names.by_key.get(key) {
        return Some(*h);
    }
    let free = |names: &Names, h: Handle| d.entity(h).is_none() && !names.by_name.contains_key(&h);
    let file = (key.0 != BY_HANDLE).then(|| d.foreign.handles.iter().find(|(_, k)| *k == key).map(|(h, _)| *h)).flatten();
    let h = match file.filter(|h| free(names, *h)) {
        Some(h) => h,
        None => {
            // A name of its own, past any that is taken (a hostile file can use this range too).
            let mut found = None;
            for _ in 0..1024 {
                let h = Handle(OWN_NAMES.checked_add(names.next)?);
                names.next = names.next.checked_add(1)?;
                if free(names, h) && !d.foreign.handles.contains_key(&h) {
                    found = Some(h);
                    break;
                }
            }
            found?
        }
    };
    names.by_key.insert(key.clone(), h);
    names.by_name.insert(h, key.clone());
    Some(h)
}

/// What an entity name of a non-graphic object stands for.
fn key_of(names: &Names, d: &Drawing, h: Handle) -> Option<ForeignKey> {
    names.by_name.get(&h).cloned().or_else(|| d.foreign.handles.get(&h).cloned())
}

/// Groups that hold a handle (DXF Reference, group code value types): soft and hard pointers and
/// owners, arbitrary object handles and plot style handles.
fn pointer_group(code: i32) -> bool {
    matches!(code, 320..=369 | 390..=399 | 480..=481)
}

/// The object `written` as an entity list: `(-1 . name)`, its type and its groups, with its
/// handle (group 5, a DIMSTYLE's 105) as its name's and pointers as entity names.
fn object_list(names: &mut Names, d: &Drawing, recs: &Records, name: Handle, written: &str) -> Value {
    let Some((kind, tags, _)) = recs.record(written) else { return Value::Nil };
    let mut items = vec![Value::cons(Value::Int(-1), Value::Ename(name)), Value::cons(Value::Int(0), Value::str(kind))];
    for v in groups_to_list(tags) {
        let code = v.car().as_number().map(|c| c as i32);
        let text = match v.cdr() {
            Value::Str(s) => Some(s.to_string()),
            _ => None,
        };
        match (code, text) {
            (Some(c @ (5 | 105)), Some(_)) => items.push(Value::cons(Value::Int(c), Value::str(&name.hex()))),
            (Some(c), Some(t)) if pointer_group(c) => {
                let Some(h) = norm(&t).filter(|h| h != "0") else { continue };
                let target = Handle::parse_hex(&h).filter(|e| d.entity(*e).is_some());
                let target = target.or_else(|| recs.key(&h).and_then(|k| name_of(names, d, &k)));
                items.push(match target {
                    Some(e) => Value::cons(Value::Int(c), Value::Ename(e)),
                    None => v,
                });
            }
            _ => items.push(v),
        }
    }
    Value::list(items)
}

/// The written handle of the non-graphic object named `h`, if it still exists.
fn written_of(names: &Names, d: &Drawing, recs: &Records, h: Handle) -> Option<String> {
    let key = key_of(names, d, h)?;
    recs.written(&key).map(str::to_string)
}

/// `entget` of a non-graphic object: nil when `h` names none.
pub fn entget(ctx: &mut Ctx<'_>, h: Handle) -> Value {
    let Ok(st) = ctx.s.state() else { return Value::Nil };
    let names = &mut ctx.env.objects;
    let Some(recs) = records(names, &st.doc) else { return Value::Nil };
    let Some(w) = written_of(names, &st.doc, &recs, h) else { return Value::Nil };
    object_list(names, &st.doc, &recs, h, &w)
}

/// `handent` of a non-graphic object: whether `h` names one that exists.
pub fn exists(ctx: &mut Ctx<'_>, h: Handle) -> bool {
    let Ok(st) = ctx.s.state() else { return false };
    let names = &mut ctx.env.objects;
    let Some(recs) = records(names, &st.doc) else { return false };
    written_of(names, &st.doc, &recs, h).is_some()
}

/// `(namedobjdict)`: the named object dictionary's entity name.
pub fn namedobjdict(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 0)?;
    let Ok(st) = ctx.s.state() else { return Ok(Flow::Value(Value::Nil)) };
    let names = &mut ctx.env.objects;
    let key: ForeignKey = ("NOD".into(), String::new());
    let found = records(names, &st.doc).filter(|r| r.written(&key).is_some()).and_then(|_| name_of(names, &st.doc, &key));
    Ok(Flow::Value(found.map(Value::Ename).unwrap_or_default()))
}

/// `(tblobjname table name)`: the entity name of a symbol table record (of the BLOCK entity for
/// the BLOCK table), nil when there is none.
pub fn tblobjname(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 2)?;
    let table = string(&a[0])?.trim().to_ascii_uppercase();
    // As the roles spell names (upper case in ASCII only).
    let name = string(&a[1])?.trim().to_ascii_uppercase();
    let Ok(st) = ctx.s.state() else { return Ok(Flow::Value(Value::Nil)) };
    let names = &mut ctx.env.objects;
    let key: ForeignKey = (table, name);
    let found = records(names, &st.doc).filter(|r| r.written(&key).is_some()).and_then(|_| name_of(names, &st.doc, &key));
    Ok(Flow::Value(found.map(Value::Ename).unwrap_or_default()))
}

/// A dictionary's entries: (key, written handle of the entry), in order.
fn entries(kind: &str, tags: &[Tag]) -> Vec<(String, String)> {
    if !matches!(kind, "DICTIONARY" | "ACDBDICTIONARYWDFLT") {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut key: Option<String> = None;
    for t in tags {
        match t.code {
            3 => key = Some(t.str()),
            350 | 360 => {
                if let (Some(k), Some(h)) = (key.take(), norm(&t.str())) {
                    out.push((k, h));
                }
            }
            _ => {}
        }
    }
    out
}

/// The entity list of entry `i` of the dictionary named `dict`, and the number of entries.
fn entry(ctx: &mut Ctx<'_>, dict: Handle, pick: impl Fn(&[(String, String)]) -> Option<usize>) -> (Value, Option<usize>) {
    let Ok(st) = ctx.s.state() else { return (Value::Nil, None) };
    let names = &mut ctx.env.objects;
    let Some(recs) = records(names, &st.doc) else { return (Value::Nil, None) };
    let Some(w) = written_of(names, &st.doc, &recs, dict) else { return (Value::Nil, None) };
    let Some((kind, tags, _)) = recs.record(&w) else { return (Value::Nil, None) };
    let list = entries(kind, tags);
    let Some(i) = pick(&list) else { return (Value::Nil, None) };
    let Some((_, eh)) = list.get(i) else { return (Value::Nil, None) };
    let Some(name) = recs.key(eh).and_then(|k| name_of(names, &st.doc, &k)) else { return (Value::Nil, Some(i)) };
    (object_list(names, &st.doc, &recs, name, eh), Some(i))
}

/// `(dictsearch dict key [setnext])`: the entity list of the entry `key` (any case) of a
/// dictionary, nil when it has none; with `setnext`, `dictnext` goes on after it.
pub fn dictsearch(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 2, 3)?;
    let dict = ename(&a[0])?;
    let key = string(&a[1])?;
    let (v, i) = entry(ctx, dict, |list| list.iter().position(|(k, _)| k.eq_ignore_ascii_case(&key)));
    if let Some(i) = i
        && a.get(2).is_some_and(|v| !v.is_nil())
    {
        ctx.env.objects.dictnext.insert(dict, i.saturating_add(1));
    }
    Ok(Flow::Value(v))
}

/// `(dictnext dict [rewind])`: the entity list of a dictionary's next entry (its first with
/// `rewind`, or on the first call), nil after the last.
pub fn dictnext(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 2)?;
    let dict = ename(&a[0])?;
    let rewind = a.get(1).is_some_and(|v| !v.is_nil());
    let at = if rewind { 0 } else { ctx.env.objects.dictnext.get(&dict).copied().unwrap_or(0) };
    let (v, _) = entry(ctx, dict, |list| (at < list.len()).then_some(at));
    ctx.env.objects.dictnext.insert(dict, at.saturating_add(1));
    Ok(Flow::Value(v))
}
