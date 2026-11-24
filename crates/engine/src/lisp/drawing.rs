//! Drawing access from LISP: `command`, system variables, entity lists (`entget`, `entmake`,
//! `entmod`, `entdel`, `entlast`, `entnext`, `handent`; non-graphic objects are in
//! [`super::objects`]), selection sets (`ssget` and friends),
//! symbol tables (`tblsearch`, `tblnext`) and unit conversions (`rtos`, `angtos`, `distof`,
//! `angtof`).
//!
//! Entity lists use the DXF groups the file reader and writer use (through
//! [`crate::cmd::file::entity_codec`]), so what `entget` shows is what a DXF file holds, except
//! for angles, which are radians in a list and degrees in a file.

use std::collections::{HashMap, HashSet};

use std::sync::{Arc, Mutex};

use cadcraft_doc::{Entity, Handle, Space};
use cadcraft_geom::Vec2;
use craft_dxf::Tag;

use super::builtins::args_err;
use super::eval::{Ctx, Flow, Need};
use super::value::{PickSet, Value, equal, to_text};
use super::{BlockSlots, LispError};
use crate::{Input, Session};

pub(super) fn arity(a: &[Value], min: usize, max: usize) -> Result<(), LispError> {
    if a.len() < min {
        return Err(args_err(true));
    }
    if a.len() > max {
        return Err(args_err(false));
    }
    Ok(())
}

pub(super) fn ename(v: &Value) -> Result<Handle, LispError> {
    match v {
        Value::Ename(h) => Ok(*h),
        other => Err(LispError::bad_arg("lentityp", other)),
    }
}

pub(super) fn string(v: &Value) -> Result<String, LispError> {
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
                    // A routine's points are in the current UCS, as typed ones are.
                    let w = ctx.s.input_ucs().to_wcs(Vec2::new(p[0], p[1]));
                    feed(ctx.s, Input::Point(w));
                } else if let Value::Ename(h) = arg.car() {
                    // An `entsel` result: (ename point).
                    // The pick point is what `entsel` gave: a UCS point, like the ones above.
                    let frame = ctx.s.input_ucs();
                    named(ctx.s, h, arg.cdr().car().as_point().map(|p| frame.to_wcs(Vec2::new(p[0], p[1]))));
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
        Value::Str(s) => {
            // A string is copied whole each time it occurs: it spends the allowance by its length
            // (one unit per 16 bytes, so about 32 MB in all), shared or not.
            *budget = budget.checked_sub(s.len() as u64 / 16)?;
            serde_json::json!(&**s)
        }
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
        Some(serde_json::Value::Object(o)) if o.contains_key("x") => point_list(&o, &name),
        v => v.map(|v| from_json(&v)).unwrap_or_default(),
    }))
}

/// Header point variables are stored as `{x, y, z}`; AutoLISP gives a point list. LIMMIN, LIMMAX,
/// SNAPBASE and HPORIGIN are 2D points, so they lose z; the rest (INSBASE, UCSORG, ...) keep all three.
fn point_list(o: &serde_json::Map<String, serde_json::Value>, name: &str) -> Value {
    let coord = |k: &str| Value::Real(o.get(k).and_then(serde_json::Value::as_f64).unwrap_or(0.0));
    let planar = ["LIMMIN", "LIMMAX", "SNAPBASE", "HPORIGIN"].iter().any(|v| v.eq_ignore_ascii_case(name.trim()));
    if planar { Value::list(vec![coord("x"), coord("y")]) } else { Value::list(vec![coord("x"), coord("y"), coord("z")]) }
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
            craft_dxf::Value::Real(r) => r,
            craft_dxf::Value::Int(i) => i as f64,
            _ => continue,
        };
        t.value = craft_dxf::Value::Real(f(v));
    }
}

fn tag_value(t: &Tag) -> Value {
    match &t.value {
        craft_dxf::Value::Str(s) | craft_dxf::Value::Hex(s) => Value::str(s),
        craft_dxf::Value::Real(r) => Value::Real(*r),
        craft_dxf::Value::Int(i) => Value::Int(i32::try_from(*i).unwrap_or(if *i < 0 { i32::MIN } else { i32::MAX })),
        craft_dxf::Value::Bool(b) => Value::Int(i32::from(*b)),
    }
}

/// DXF groups as an association list: points combined into lists `(10 x y z)`.
pub(super) fn groups_to_list(tags: &[Tag]) -> Vec<Value> {
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

/// The most string bytes an entity list may hold (counted per occurrence, shared or not).
const MAX_GROUP_TEXT: usize = 32 << 20;

/// An association list back to DXF groups (points split into x, y, z groups). The type
/// (`0`), the entity name (`-1`), the owner of a subrecord (`-2`) and of the entity (`330`), the
/// handle (`5`) and the layout (`410`) are left out: they aren't the entity's to set.
fn list_to_groups(list: &Value) -> Result<(String, Vec<Tag>), LispError> {
    let mut kind = None;
    let mut tags = Vec::new();
    let mut text_left = MAX_GROUP_TEXT;
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
            -1 | -2 | 5 | 330 | 360 | 410 | 102 => {}
            _ if point_group(code) && matches!(v, Value::Cons(_)) => {
                let p = v.as_point().ok_or_else(|| LispError::bad_arg("2D/3D point", &v))?;
                tags.push(Tag::f(code, p[0]));
                tags.push(Tag::f(code + 10, p[1]));
                tags.push(Tag::f(code + 20, p[2]));
            }
            _ => tags.push(match &v {
                Value::Int(i) => Tag::i(code, i64::from(*i)),
                Value::Real(r) => Tag::f(code, *r),
                Value::Str(s) => {
                    // Each occurrence of a shared string becomes its own copy in the tag.
                    text_left = text_left.checked_sub(s.len()).ok_or_else(|| LispError::new("out of memory: the entity list holds too much text"))?;
                    Tag::s(code, &**s)
                }
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

// ---------------- an INSERT's ATTRIBs and SEQEND ----------------

/// The most names a drawing's LISP keeps for an INSERT's records at once: a routine can't fill
/// memory by walking. At the limit `entnext` gives no *new* names (`nil`) until an edit that
/// expires names (Undo, ATTSYNC) makes room; the names already given still name their records.
const MAX_SUB_NAMES: usize = 65_536;
/// The most handles a drawing reserves for names over its life (sixteen full walks' worth): once
/// it has that many, `entnext` gives no more *new* names, so a routine that keeps editing cannot
/// grow the set without bound. The names it already gave still name their records.
#[cfg(not(test))]
const MAX_SUB_HANDLES: usize = MAX_SUB_NAMES * 16;
/// The same bound, two full walks' worth, in test builds: a test reaches it in bounded time.
#[cfg(test)]
const MAX_SUB_HANDLES: usize = MAX_SUB_NAMES * 2;
/// How many names a lookup tries past taken ones before giving up: a hostile file may hold any
/// handle, including one this range gives out.
const SUB_NAME_PROBES: u64 = 1_024;
/// The top of the range names are taken from, exclusive: the first is one below it, and the
/// drawing's top handle itself is always left to [`cadcraft_doc::Drawing::new_handle`], which
/// cannot step past a reserved handle at the end of the range.
const FIRST_SUB_NAME: u64 = u64::MAX;

/// What a name given to an INSERT's ATTRIB or SEQEND stands for, while it holds.
struct SubEnt {
    owner: Handle,
    /// The record's place after the INSERT's own: an ATTRIB's index, or `attribs.len()` for the
    /// SEQEND.
    slot: usize,
    /// The attribute's tag when it was named (`""` for the SEQEND): a record at that place with
    /// another tag is another object, so the name no longer holds.
    tag: String,
}

/// Names given to an INSERT's ATTRIBs and SEQEND (#1660): records with no handle of their own in
/// the drawing, so a routine can reach one only by a name LISP gives out. A name holds while its
/// reference keeps the record in that place with that tag and the drawing's name epoch is the one
/// it was given in; otherwise it names nothing (`nil`), never another record.
#[derive(Default)]
pub(crate) struct SubEnts {
    /// Every handle a name was given to in this drawing's session, also after its name stopped
    /// holding and through undo or redo (and the Block Editor swapping the drawing): a name is
    /// never given to another record. Bounded by [`MAX_SUB_HANDLES`].
    issued: Vec<Handle>,
    /// The same handles, for lookups: a name that no longer holds is told from a real object
    /// that has its handle.
    issued_set: HashSet<Handle>,
    /// The name of each record a walk reached, by its INSERT and place.
    by_slot: HashMap<(Handle, usize), Handle>,
    by_name: HashMap<Handle, SubEnt>,
    /// Names dropped in this epoch because their record moved or another object took their
    /// handle. They name nothing, even when a real entity now has the handle.
    retired: HashSet<Handle>,
    /// The next handle to try, counting down from just below the top. It only descends: a name
    /// given once is never given to another record. `None` when names are exhausted, or before
    /// the first is given (`started` false).
    cursor: Option<u64>,
    started: bool,
    /// The drawing it gave the names in: its session id and name epoch.
    doc: u64,
    epoch: u64,
}

/// The attributes of the block reference `h` in `d` (none for anything else, and for a reference
/// with no attributes: an empty run of records is not written).
fn insert_attribs(d: &cadcraft_doc::Drawing, h: Handle) -> Option<&[cadcraft_doc::Attrib]> {
    match d.entity(h).map(|e| &e.kind) {
        Some(cadcraft_doc::EntityKind::Insert(i)) if !i.attribs.is_empty() => Some(&i.attribs),
        _ => None,
    }
}

/// The drawing and the epoch its names were given in, without holding a borrow of the session
/// while the names are looked at: the drawing, its session id, and its name epoch.
fn doc_and_epoch(ctx: &Ctx<'_>) -> Option<(Arc<cadcraft_doc::Drawing>, u64, u64)> {
    let st = ctx.s.state().ok()?;
    Some((st.doc.clone(), st.uid, st.names_epoch))
}

impl SubEnts {
    /// Whether a name was dropped in this epoch because another object took its handle: it names
    /// nothing, not that object.
    fn retired(&self, h: Handle) -> bool {
        self.retired.contains(&h)
    }

    /// Whether `h` was given to one of the drawing's records as a name in this session.
    pub(super) fn was_issued(&self, h: Handle) -> bool {
        self.issued_set.contains(&h)
    }

    /// How many handles names were given to over this drawing's session: a lifetime bound for a
    /// routine that keeps walking and editing.
    pub(crate) fn issued_count(&self) -> usize {
        self.issued.len()
    }

    /// The handles given out as names, in the order they were given.
    pub(crate) fn issued_handles(&self) -> impl Iterator<Item = u64> + '_ {
        self.issued.iter().map(|h| h.0)
    }

    /// Note that `h` was given as a name: it is never given again, and `Drawing::new_handle`
    /// skips it. The skip lives beside the drawing, so a copy allocates with the same exclusion
    /// and remembering the name does not change the drawing.
    fn note_issued(&mut self, uid: u64, h: Handle) {
        self.issued.push(h);
        self.issued_set.insert(h);
        if !cadcraft_doc::note_host_reserved(uid, h.0) {
            cadcraft_doc::replace_host_reserved(uid, self.issued_handles());
        }
    }

    /// Drop the names of records whose owner `gone` picks: they went with those objects, and
    /// OOPS or undo bringing an owner back must not revive its names. Other names keep holding.
    pub(crate) fn retire_owned_by(&mut self, gone: impl Fn(Handle) -> bool) {
        let dropped: Vec<Handle> = self.by_name.iter().filter(|(_, e)| gone(e.owner)).map(|(h, _)| *h).collect();
        for h in dropped {
            self.retire(h);
        }
    }

    /// The next handle to try. It only descends, so a name given once is never given to another
    /// record, also after it stopped holding.
    fn next_candidate(&mut self) -> Option<u64> {
        let c = match self.cursor {
            Some(c) => c,
            None if !self.started => FIRST_SUB_NAME.checked_sub(1)?,
            None => return None,
        };
        self.started = true;
        self.cursor = c.checked_sub(1);
        Some(c)
    }

    /// Whether the record a name was given for is still there, with that tag for an attribute.
    fn holds(&self, attribs: &[cadcraft_doc::Attrib], h: Handle) -> bool {
        let Some(e) = self.by_name.get(&h) else { return false };
        match attribs.get(e.slot) {
            Some(a) => a.tag.eq_ignore_ascii_case(&e.tag),
            None => e.slot == attribs.len() && e.tag.is_empty(),
        }
    }

    /// Drop `h` as a name; it names nothing now, also if another object takes its handle.
    fn retire(&mut self, h: Handle) {
        if let Some(e) = self.by_name.remove(&h)
            && self.by_slot.get(&(e.owner, e.slot)) == Some(&h)
        {
            self.by_slot.remove(&(e.owner, e.slot));
        }
        self.retired.insert(h);
    }

    /// Forget every name: the drawing it was given in changed (undo or redo, ATTSYNC,
    /// -BLOCKREPLACE, closing the Block Editor). The handles given stay issued, and the cursor is
    /// kept: a name given once is never given again.
    fn expire(&mut self, doc: u64, epoch: u64) {
        if self.doc != doc || self.epoch != epoch {
            self.by_slot.clear();
            self.by_name.clear();
            self.retired.clear();
            self.doc = doc;
            self.epoch = epoch;
        }
    }

    /// The INSERT and record `h` names now, if it still names one (a stale name is dropped).
    fn lookup(&mut self, doc: u64, epoch: u64, attribs: &[cadcraft_doc::Attrib], h: Handle) -> Option<(Handle, usize)> {
        self.expire(doc, epoch);
        if !self.holds(attribs, h) {
            self.retire(h);
            return None;
        }
        self.by_name.get(&h).map(|e| (e.owner, e.slot))
    }

    /// The name record `slot` after INSERT `owner`'s own already has, if it still holds one: a
    /// stale name, or one whose handle another object took, is retired and answers `None`. A
    /// record no earlier walk named gets none: this never reserves a new name.
    fn issued(
        &mut self,
        doc: u64,
        epoch: u64,
        owner: Handle,
        slot: usize,
        attribs: &[cadcraft_doc::Attrib],
        mut taken: impl FnMut(Handle) -> bool,
    ) -> Option<Handle> {
        self.expire(doc, epoch);
        let h = self.by_slot.get(&(owner, slot)).copied()?;
        self.lookup(doc, epoch, attribs, h)?;
        if taken(h) {
            // Another object took the handle: the name is retired, never given back.
            self.retire(h);
            return None;
        }
        Some(h)
    }

    /// The name of record `slot` after INSERT `owner`'s own (its attributes, then its SEQEND),
    /// given out now if it has none: the same one while the record holds, so a repeated walk is
    /// stable. `taken` says whether the drawing already gives a candidate handle a meaning; a
    /// cached name is checked again, so a name is never left on a handle another object took.
    fn name(
        &mut self,
        doc: u64,
        epoch: u64,
        owner: Handle,
        slot: usize,
        attribs: &[cadcraft_doc::Attrib],
        mut taken: impl FnMut(Handle) -> bool,
    ) -> Option<Handle> {
        if let Some(h) = self.issued(doc, epoch, owner, slot, attribs, &mut taken) {
            return Some(h);
        }
        let tag = match attribs.get(slot) {
            Some(a) => a.tag.clone(),
            None if slot == attribs.len() => String::new(),
            None => return None,
        };
        if self.by_name.len() >= MAX_SUB_NAMES {
            return None;
        }
        for _ in 0..SUB_NAME_PROBES {
            let Some(c) = self.next_candidate() else { break };
            let h = Handle(c);
            if self.by_name.contains_key(&h) || self.retired.contains(&h) || taken(h) {
                continue;
            }
            self.by_slot.insert((owner, slot), h);
            self.by_name.insert(h, SubEnt { owner, slot, tag });
            return Some(h);
        }
        None
    }
}

/// Whether the drawing already gives `h` a meaning: an entity, a block definition's entity, an
/// object kept from a file, or one `entdel` took (which `entdel` brings back).
fn name_taken(d: &cadcraft_doc::Drawing, slots: &BlockSlots, deleted: &HashMap<Handle, (Space, Arc<Entity>)>, h: Handle) -> bool {
    deleted.contains_key(&h) || slots.contains_key(&h) || d.entity(h).is_some() || d.foreign.handles.contains_key(&h)
}

/// The INSERT and record the name `h` stands for now (none for anything else).
fn sub_of(ctx: &mut Ctx<'_>, h: Handle) -> Option<(Handle, usize)> {
    let (d, doc, epoch) = doc_and_epoch(ctx)?;
    // Every issued name stays issued in the session, also after its epoch passed; a handle that
    // was merely probed (the INSERT's own, or a real entity's) is not a name.
    if !name_given(ctx, h) {
        return None;
    }
    // A real entity always wins: names are given only where the drawing has nothing.
    if d.entity(h).is_some() {
        return None;
    }
    let owner = ctx.s.subents()?.by_name.get(&h)?.owner;
    let attribs = insert_attribs(&d, owner)?;
    ctx.s.subents_mut()?.lookup(doc, epoch, attribs, h)
}

/// Whether `h` was given as a name in this session (whether or not it still holds).
fn name_given(ctx: &Ctx<'_>, h: Handle) -> bool {
    ctx.s.subents().is_some_and(|s| s.was_issued(h))
}

/// Whether the name `h` was dropped because another object took its handle: it names nothing,
/// not that object.
fn name_retired(ctx: &Ctx<'_>, h: Handle) -> bool {
    ctx.s.subents().is_some_and(|s| s.retired(h))
}

/// The name of record `slot` after INSERT `owner`'s own (#1660): an ATTRIB, or its SEQEND (one
/// past the last attribute). None when it has no such record, or when no name is left to give.
fn sub_name(ctx: &mut Ctx<'_>, owner: Handle, slot: usize) -> Option<Handle> {
    let (d, doc, epoch) = doc_and_epoch(ctx)?;
    let attribs = insert_attribs(&d, owner)?;
    if slot > attribs.len() {
        return None;
    }
    // A host without the writer's record mapping has no records to name.
    crate::cmd::file::entity_sub_record()?;
    let slots = block_slots(ctx);
    let deleted = &ctx.env.deleted;
    let objects = &ctx.env.objects;
    let mut taken = |h| name_taken(&d, &slots, deleted, h) || objects.names_handle(h);
    let subents = ctx.s.subents_mut()?;
    // The name a walk gave this record holds, also when the drawing's lifetime cap is reached.
    if let Some(h) = subents.issued(doc, epoch, owner, slot, attribs, &mut taken) {
        return Some(h);
    }
    // A routine that keeps walking and editing gets no more new names once its drawing issued
    // this many: memory stays bounded. The cap stops new names only.
    if subents.issued_count() >= MAX_SUB_HANDLES {
        return None;
    }
    let chosen = subents.name(doc, epoch, owner, slot, attribs, taken)?;
    subents.note_issued(doc, chosen);
    Some(chosen)
}

/// What follows the record an INSERT's name stands for (#1660).
enum SubNext {
    /// Not the name of such a record.
    NotSub,
    /// The next record's name, or none when the session gives out no more.
    Record(Option<Handle>),
    /// The run ended (the SEQEND): the main order goes on from this INSERT.
    Ended(Handle),
}

fn sub_next(ctx: &mut Ctx<'_>, h: Handle) -> SubNext {
    let Some((owner, slot)) = sub_of(ctx, h) else { return SubNext::NotSub };
    let len = ctx.s.doc().ok().and_then(|d| insert_attribs(d, owner)).map(<[cadcraft_doc::Attrib]>::len);
    match len {
        Some(len) if slot < len => SubNext::Record(sub_name(ctx, owner, slot.saturating_add(1))),
        Some(_) => SubNext::Ended(owner),
        None => SubNext::Record(None),
    }
}

/// An entity list: its name (`-1`), type (`0`), the owner of an INSERT's ATTRIB or SEQEND (`-2`)
/// and the groups of its record, with the layout name after the AcDbEntity marker, as AutoCAD
/// lists it. Without a layout of its own (an entity of a block definition) it has none.
fn entity_list(d: &cadcraft_doc::Drawing, h: Handle, kind: &str, tags: &[Tag], owner: Option<Handle>, in_block: bool) -> Value {
    let layout = match space_of(d, owner.unwrap_or(h)) {
        Some(Space::Paper(name)) => name,
        _ => "Model".into(),
    };
    let mut items = vec![Value::cons(Value::Int(-1), Value::Ename(h)), Value::cons(Value::Int(0), Value::str(kind))];
    if let Some(o) = owner {
        items.push(Value::cons(Value::Int(-2), Value::Ename(o)));
    }
    let mut placed_layout = in_block;
    for v in groups_to_list(tags) {
        // The layout name follows the AcDbEntity marker, as AutoCAD lists it.
        let after_marker = !placed_layout && v.car().as_number() == Some(100.0) && matches!(v.cdr(), Value::Str(ref s) if &**s == "AcDbEntity");
        items.push(v);
        if after_marker {
            items.push(Value::cons(Value::Int(410), Value::str(&layout)));
            placed_layout = true;
        }
    }
    Value::list(items)
}

/// `entget` of an INSERT's ATTRIB or SEQEND (#1660), when `h` names one now.
fn sub_entget(ctx: &mut Ctx<'_>, h: Handle) -> Option<Value> {
    let (owner, slot) = sub_of(ctx, h)?;
    let d = ctx.s.doc().ok()?;
    let e = d.entity(owner)?;
    let (kind, mut tags) = (crate::cmd::file::entity_sub_record()?)(d, e, slot)?;
    convert_angles(&kind, &mut tags, f64::to_radians);
    Some(entity_list(d, h, &kind, &tags, Some(owner), false))
}

/// What an ATTRIB's submitted list asks to change: `Some(None)` when it asks for nothing, and
/// `Some(Some(value))` for its value (group 1). `None` when it asks for any other edit: a group
/// added, dropped, moved or changed other than the attribute's own value.
fn value_edit(now: &[Tag], asked: &[Tag]) -> Option<Option<String>> {
    if now.len() != asked.len() {
        return None;
    }
    let mut value = None;
    let mut seen = false;
    for (n, a) in now.iter().zip(asked) {
        if n.code != a.code {
            return None;
        }
        if n.code == 1 {
            // A multiline attribute's embedded text has a group 1 of its own, after its marker
            // (101): only the attribute's value, before it, may differ.
            match (&n.value, &a.value) {
                (craft_dxf::Value::Str(old), craft_dxf::Value::Str(new)) if !seen => {
                    seen = true;
                    if old != new {
                        value = Some(new.clone());
                    }
                }
                _ if n == a => {}
                _ => return None,
            }
        } else if n != a {
            return None;
        }
    }
    Some(value)
}

/// The submitted list has the items `entget` would return, in order, except at group 1 (the
/// value): `value_edit` checks which group 1 may differ. Every other group, including the ones
/// an entity cannot set (`-2`, `5`, `330`, `360`, `410`, `102`), must be the same.
fn only_value_diff(expected: &Value, asked: &Value) -> bool {
    let (mut expected, mut asked) = (expected.iter(), asked.iter());
    loop {
        match (expected.next(), asked.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) => {
                let code = x.car().as_number();
                if code != y.car().as_number() {
                    return false;
                }
                if code != Some(1.0) && !equal(&x, &y, 0.0) {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// `entmod` of the INSERT's record the name `h` stands for (#1660): only an ATTRIB's value may
/// change, the group 1 value; its tag, place, flags and properties are the reference's. A SEQEND
/// holds nothing to change, and a name that no longer holds names nothing: both give `nil`, the
/// drawing as it is.
fn entmod_attrib(ctx: &mut Ctx<'_>, list: &Value, h: Handle) -> Value {
    let Some((owner, slot)) = sub_of(ctx, h) else { return Value::Nil };
    // An outer list with an atom tail is not the list `entget` returned: iterating it would drop
    // the tail and accept the malformed list. The group pairs inside are dotted, as DXF has them.
    if !list.is_list() {
        return Value::Nil;
    }
    let Ok((kind, asked)) = list_to_groups(list) else { return Value::Nil };
    if !kind.eq_ignore_ascii_case("ATTRIB") {
        return Value::Nil;
    }
    let Ok(d) = ctx.s.doc() else { return Value::Nil };
    let Some(e) = d.entity(owner) else { return Value::Nil };
    let Some(sub_record) = crate::cmd::file::entity_sub_record() else { return Value::Nil };
    let Some((now_kind, mut now)) = sub_record(d, e, slot) else { return Value::Nil };
    if !now_kind.eq_ignore_ascii_case(&kind) {
        return Value::Nil;
    }
    convert_angles(&now_kind, &mut now, f64::to_radians);
    // The layout, owner, handle and any other group are not the attribute's to change: a list
    // that differs from `entget` anywhere but its group 1 value is refused, the drawing untouched.
    let expected = entity_list(d, h, &now_kind, &now, Some(owner), false);
    if !only_value_diff(&expected, list) {
        return Value::Nil;
    }
    match value_edit(&now, &asked) {
        None => Value::Nil,
        Some(None) => list.clone(),
        Some(Some(value)) => {
            let ok = ctx.s.doc_mut().is_ok_and(|d| {
                d.modify_entity(owner, |e| {
                    if let cadcraft_doc::EntityKind::Insert(i) = &mut e.kind
                        && let Some(a) = i.attribs.get_mut(slot)
                    {
                        a.text.value = value.clone();
                    }
                })
                .is_ok()
            });
            if ok {
                ctx.s.touch();
            }
            if ok { list.clone() } else { Value::Nil }
        }
    }
}

pub fn entget(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 2)?;
    let h = ename(&a[0])?;
    // An INSERT's ATTRIB or SEQEND (#1660) is not an entity of the drawing: LISP's name reaches it.
    if let Some(v) = sub_entget(ctx, h) {
        return Ok(Flow::Value(v));
    }
    let drawn = ctx.s.doc().is_ok_and(|d| d.entity(h).is_some());
    let from_block = if drawn { None } else { block_entity(ctx, h) };
    let Ok(d) = ctx.s.doc() else { return Ok(Flow::Value(Value::Nil)) };
    // An entity of a block definition (#1700) has no layout.
    let (e, in_block) = match (d.entity(h), &from_block) {
        (Some(e), _) => (e, false),
        (None, Some(e)) => (e, true),
        // Not an entity: a table record, layout or dictionary (#1659, #1661).
        (None, None) => return Ok(Flow::Value(super::objects::entget(ctx, h))),
    };
    let (kind, mut tags) = (codec()?.groups)(d, e);
    convert_angles(&kind, &mut tags, f64::to_radians);
    Ok(Flow::Value(entity_list(d, h, &kind, &tags, None, in_block)))
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
    ctx.env.block_next = None;
    ctx.s.touch();
    Value::str(&name)
}

pub fn entmod(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let list = &a[0];
    let Some(Value::Ename(h)) = list.iter().find(|v| v.car().as_number() == Some(-1.0)).map(|v| v.cdr()) else {
        return Err(LispError::bad_arg("lentityp", &Value::Nil));
    };
    // A name retired because another object took its handle names nothing, not that object.
    if name_retired(ctx, h) {
        return Ok(Flow::Value(Value::Nil));
    }
    // An INSERT's record takes a value edit only (#1660); any other edit of one is refused. A
    // real entity with a handle in the names' range (a hostile file's) is still its own.
    if name_given(ctx, h) && ctx.s.doc().is_ok_and(|d| d.entity(h).is_none()) {
        return Ok(Flow::Value(entmod_attrib(ctx, list, h)));
    }
    let Some(old) = ctx.s.doc().ok().and_then(|d| d.entity(h)).cloned() else { return Ok(Flow::Value(Value::Nil)) };
    let (kind_name, common, mut kind) = match entity_from_list(ctx, list) {
        Ok(x) => x,
        Err(_) => return Ok(Flow::Value(Value::Nil)),
    };
    // entmod changes an entity, never its type.
    if !kind_name.eq_ignore_ascii_case(old.kind.dxf_name()) {
        return Ok(Flow::Value(Value::Nil));
    }
    // A block reference's attributes are entities of their own in AutoCAD, which entmod of the
    // INSERT leaves alone; its list doesn't hold them, so they are kept from the reference.
    if let (cadcraft_doc::EntityKind::Insert(new), cadcraft_doc::EntityKind::Insert(was)) = (&mut kind, &old.kind)
        && new.attribs.is_empty()
    {
        new.attribs = was.attribs.clone();
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
    // A name retired because another object took its handle names nothing, not that object.
    if name_retired(ctx, h) {
        return Ok(Flow::Value(Value::Nil));
    }
    // An INSERT's ATTRIB or SEQEND is the reference's: it is not deleted on its own (#1660).
    if name_given(ctx, h) && ctx.s.doc().is_ok_and(|d| d.entity(h).is_none()) {
        return Ok(Flow::Value(Value::Nil));
    }
    let space = ctx.s.doc().ok().and_then(|d| d.space_of(h));
    if let Some(space) = space {
        let removed = ctx.s.doc_mut().ok().and_then(|d| d.remove_entity(h));
        return Ok(match removed {
            Some(e) => {
                ctx.env.deleted.insert(h, (space, e));
                ctx.s.touch();
                // The names of records that went with it must not come back on `entdel`; names of
                // other references keep holding.
                if let Some(subents) = ctx.s.subents_mut() {
                    subents.retire_owned_by(|owner| owner == h);
                }
                Flow::Value(Value::Ename(h))
            }
            None => Flow::Value(Value::Nil),
        });
    }
    match ctx.env.deleted.remove(&h) {
        Some((space, e)) => {
            let restored = ctx.s.doc_mut().ok().and_then(|d| d.space_mut(&space)).map(|store| store.push(Entity::clone(&e))).is_some();
            ctx.s.touch();
            Ok(Flow::Value(if restored { Value::Ename(h) } else { Value::Nil }))
        }
        None => Ok(Flow::Value(Value::Nil)),
    }
}

/// `b` holds the same handles as `a`, in the same chunks and order. A chunk `b` still shares
/// with `a` (which is held, so its chunks stay alive) is the same; only a chunk an edit copied is
/// compared handle by handle.
fn same_handles(a: &cadcraft_doc::EntityStore, b: &cadcraft_doc::EntityStore) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let (mut x, mut y) = (a.chunk_slices(), b.chunk_slices());
    loop {
        match (x.next(), y.next()) {
            (None, None) => return true,
            (Some((ka, sa)), Some((kb, sb))) => {
                if ka != kb && (sa.len() != sb.len() || sa.iter().zip(sb).any(|(p, q)| p.handle != q.handle)) {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// Every entity's handle in creation order (model space and layouts), cached per drawing.
///
/// The cache is keyed by content identity, as the spatial index is: it holds clones of the
/// spaces it was taken from, so any edit (`entmake`, `entmod`, `entdel`, a command, undo) copies
/// the chunk it touches and the cache no longer matches. The drawing's address is no key: an
/// edit in place keeps it, and a new drawing can be given the address of a dropped one.
fn handles_in_order(ctx: &mut Ctx<'_>) -> Arc<Vec<Handle>> {
    let Ok(d) = ctx.s.doc() else { return Arc::default() };
    let spaces = || std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities));
    if let Some((stores, v)) = &mut ctx.env.order
        && stores.len() == 1 + d.layouts.len()
        && stores.iter().zip(spaces()).all(|(a, b)| same_handles(a, b))
    {
        // An edit that left every handle in place (a property `entmod`) keeps the order: hold
        // the spaces as they are now, so the next call finds their chunks shared again.
        if !stores.iter().zip(spaces()).all(|(a, b)| a.same_as(b)) {
            *stores = spaces().cloned().collect();
        }
        return v.clone();
    }
    let mut v: Vec<Handle> = spaces().flat_map(|s| s.iter().map(|e| e.handle)).collect();
    v.sort_unstable();
    let v = Arc::new(v);
    ctx.env.order = Some((spaces().cloned().collect(), v.clone()));
    v
}

/// Every block-definition entity with its successor within its block, cached per drawing like
/// [`handles_in_order`]: one pass over the blocks, then `entnext`, `entget` and `handent` on a
/// block's entity are hash lookups instead of scans of the block (and of every block, to find
/// which holds the entity).
fn block_slots(ctx: &mut Ctx<'_>) -> Arc<BlockSlots> {
    let Ok(st) = ctx.s.state() else { return Arc::default() };
    let key = Arc::as_ptr(&st.doc) as usize;
    if let Some((k, v)) = &ctx.env.block_next
        && *k == key
    {
        return v.clone();
    }
    let mut m = HashMap::new();
    for b in st.doc.blocks.values() {
        let mut it = b.entities.iter().peekable();
        while let Some(e) = it.next() {
            m.insert(e.handle, (e.clone(), it.peek().map(|n| n.handle)));
        }
    }
    let m = Arc::new(m);
    ctx.env.block_next = Some((key, m.clone()));
    m
}

pub fn entlast(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 0)?;
    Ok(Flow::Value(handles_in_order(ctx).last().map(|h| Value::Ename(*h)).unwrap_or_default()))
}

pub fn entnext(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 0, 1)?;
    let mut after = None;
    if let Some(v) = a.first().filter(|v| !v.is_nil()) {
        let h = ename(v)?;
        // From an entity of a block definition: the next one in that block, nil after the last (#1700).
        if ctx.s.doc().is_ok_and(|d| d.entity(h).is_none())
            && let Some((_, next)) = block_slots(ctx).get(&h)
        {
            return Ok(Flow::Value(next.map(Value::Ename).unwrap_or_default()));
        }
        // An INSERT's own record is followed by its ATTRIBs and SEQEND (#1660), and each record of
        // that run by the next; after the SEQEND the main order goes on from the INSERT.
        match sub_next(ctx, h) {
            SubNext::Record(name) => return Ok(Flow::Value(name.map(Value::Ename).unwrap_or_default())),
            SubNext::Ended(owner) => after = Some(owner),
            SubNext::NotSub => {
                // An INSERT with attributes: its first ATTRIB, or nil when no name is left to give
                // (the walk stops rather than skipping the records it can't name). A host without
                // the optional sub-record provider has no such records: the walk goes on to the
                // next main entity as it always did.
                if crate::cmd::file::entity_sub_record().is_some() && ctx.s.doc().is_ok_and(|d| insert_attribs(d, h).is_some()) {
                    return Ok(Flow::Value(sub_name(ctx, h, 0).map(Value::Ename).unwrap_or_default()));
                }
                // A name that no longer holds names nothing. A real entity with a handle in the
                // names' range (a hostile file's) is never filtered: the walk goes on from it.
                if name_given(ctx, h) && ctx.s.doc().is_ok_and(|d| d.entity(h).is_none()) {
                    return Ok(Flow::Value(Value::Nil));
                }
                after = Some(h);
            }
        }
    }
    let order = handles_in_order(ctx);
    let next = match after {
        Some(h) => order.get(order.partition_point(|x| *x <= h)).copied(),
        None => order.first().copied(),
    };
    Ok(Flow::Value(next.map(Value::Ename).unwrap_or_default()))
}

pub fn handent(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 1, 1)?;
    let s = string(&a[0])?;
    let Some(h) = Handle::parse_hex(&s) else { return Ok(Flow::Value(Value::Nil)) };
    let found = ctx.s.doc().is_ok_and(|d| d.entity(h).is_some()) || block_entity(ctx, h).is_some() || super::objects::exists(ctx, h);
    Ok(Flow::Value(if found { Value::Ename(h) } else { Value::Nil }))
}

/// Entity `h` of a block definition (#1700), by the cached index.
fn block_entity(ctx: &mut Ctx<'_>, h: Handle) -> Option<Arc<Entity>> {
    block_slots(ctx).get(&h).map(|(e, _)| e.clone())
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
    // The points are UCS points, as `getpoint` and `getcorner` give them: the selection runs in
    // the world.
    let frame = ctx.s.input_ucs();
    let point = |i: usize| -> Result<Vec2, LispError> {
        let v = pts.get(i).ok_or_else(|| args_err(true))?;
        let p = v.as_point().ok_or_else(|| LispError::bad_arg("2D/3D point", v))?;
        Ok(frame.to_wcs(Vec2::new(p[0], p[1])))
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
            // Inside an MSPACE viewport, the objects that viewport does not draw are passed over.
            let st = ctx.s.state().ok();
            let drawn = |h: &Handle| st.is_none_or(|st| st.doc.entity(*h).is_none_or(|e| !st.hidden_in_viewport(e)));
            order.iter().rev().copied().find(drawn).into_iter().collect()
        }
        Some("P") => ctx.s.state().map(|st| st.drawn_in_viewport(&st.previous_selection)).unwrap_or_default(),
        Some("I") => ctx.s.selection(),
        Some("W") | Some("C") => {
            // A selection window is the box around its corners in the world, like SELECT's: the
            // same corners typed at either pick the same objects.
            let (p1, p2) = (point(0)?, point(1)?);
            let s = &*ctx.s;
            let Ok(d) = s.doc() else { return Ok(Flow::Value(Value::Nil)) };
            let bx = cadcraft_geom::Bounds2::from_points([p1, p2]);
            crate::select::select_window_in(d, &space, s.viewport_frozen(), bx, mode.as_deref() == Some("C"))
        }
        Some("F") | Some("WP") | Some("CP") => {
            let poly: Vec<Vec2> =
                pts.first().map(|l| l.iter().filter_map(|p| p.as_point().map(|q| frame.to_wcs(Vec2::new(q[0], q[1])))).collect()).unwrap_or_default();
            let s = &*ctx.s;
            let Ok(d) = s.doc() else { return Ok(Flow::Value(Value::Nil)) };
            match mode.as_deref() {
                Some("F") => crate::select::select_fence_in(d, &space, s.viewport_frozen(), &poly),
                m => crate::select::select_polygon_in(d, &space, s.viewport_frozen(), &poly, m == Some("CP")),
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
                crate::select::pick_in(d, &space, s.viewport_frozen(), p, ap).into_iter().collect()
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
                let mut items = vec![
                    pair(0, Value::str("BLOCK")),
                    pair(2, Value::str(&b.name)),
                    pair(70, Value::Int(0)),
                    Value::list(vec![Value::Int(10), Value::Real(b.base.x), Value::Real(b.base.y), Value::Real(b.base.z)]),
                ];
                // Its first entity (-2): entnext walks the rest of the definition (#1700).
                if let Some(first) = b.entities.iter().next() {
                    items.push(pair(-2, Value::Ename(first.handle)));
                }
                Value::list(items)
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
    // A pure string-to-angle conversion: ANGBASE and ANGDIR apply, the UCS does not.
    let mut settings = ctx.s.angle_settings().world();
    // AUNITS 0..=4; any other units argument is an error in AutoCAD, nil here.
    match a.get(1) {
        None | Some(Value::Nil) => {}
        Some(Value::Int(u)) if (0..=4).contains(u) => settings.aunits = i64::from(*u),
        Some(_) => return Ok(Flow::Value(Value::Nil)),
    }
    Ok(Flow::Value(settings.direction(&s).map(Value::Real).unwrap_or_default()))
}

/// A coordinate system `trans` converts from or to: its origin and axes in world coordinates.
fn trans_system(ctx: &mut Ctx<'_>, v: &Value) -> Result<crate::ucs::UcsFrame, LispError> {
    use crate::ucs::UcsFrame;
    match v {
        // 0 the WCS; 3 paper space's display system, which lines up with paper space's WCS (from
        // or to the active layout viewport's display system, `viewport_to_paper` applies).
        Value::Int(0 | 3) => Ok(UcsFrame::WORLD),
        // 2 the display system: in a plan view, the WCS turned back by the view's twist (X runs
        // right across the screen).
        Value::Int(2) => {
            let twist = cadcraft_geom::norm_angle(ctx.s.state().map_or(0.0, |st| st.view().twist));
            Ok(UcsFrame::WORLD.rotated_z(-twist))
        }
        Value::Int(1) => Ok(ctx.s.input_ucs()),
        // An object: its OCS. An entity of a block definition is found as `entget` finds it.
        // LINE and POINT keep their points in world coordinates whatever their extrusion (the
        // DXF Reference), so their system is the WCS.
        Value::Ename(h) => {
            let frame = |e: &Entity| match e.kind {
                cadcraft_doc::EntityKind::Line(_) | cadcraft_doc::EntityKind::Point(_) => Some(UcsFrame::WORLD),
                _ => ocs(e.common.extrusion),
            };
            let found = match ctx.s.doc().map_err(|e| LispError::new(e.to_string()))?.entity(*h) {
                Some(e) => frame(e),
                None => frame(&*block_entity(ctx, *h).ok_or_else(|| LispError::bad_arg("lentityp", v))?),
            };
            found.ok_or_else(|| LispError::bad_arg("lentityp", v))
        }
        // An extrusion direction: the OCS of objects with it.
        Value::Cons(_) => {
            let n = v.as_point().ok_or_else(|| LispError::bad_arg("coordinate system", v))?;
            ocs(cadcraft_geom::Vec3::new(n[0], n[1], n[2])).ok_or_else(|| LispError::bad_arg("coordinate system", v))
        }
        other => Err(LispError::bad_arg("coordinate system", other)),
    }
}

/// The object coordinate system of extrusion direction `n` (the arbitrary axis algorithm of the
/// DXF Reference): `None` for a direction with no length.
fn ocs(n: cadcraft_geom::Vec3) -> Option<crate::ucs::UcsFrame> {
    use cadcraft_geom::Vec3;
    let l = n.len();
    if !(l > 1e-12 && l.is_finite()) {
        return None;
    }
    let n = n * (1.0 / l);
    let w = if n.x.abs() < 1.0 / 64.0 && n.y.abs() < 1.0 / 64.0 { Vec3::new(0.0, 1.0, 0.0) } else { Vec3::Z };
    let ax = w.cross(n);
    crate::ucs::UcsFrame::new(Vec3::ZERO, ax, n.cross(ax))
}

/// `trans` between the display system (2) of the active layout viewport and paper space's
/// display system (3): the viewport's scale, and where its view sits on the sheet. `None` for any
/// other pair of systems, or with no active viewport (on the Model tab, or on the sheet itself,
/// the two line up).
fn viewport_to_paper(ctx: &Ctx<'_>, from: &Value, to: &Value, p: cadcraft_geom::Vec3, disp: bool) -> Option<cadcraft_geom::Vec3> {
    let to_paper = match (from, to) {
        (Value::Int(2), Value::Int(3)) => true,
        (Value::Int(3), Value::Int(2)) => false,
        _ => return None,
    };
    let (_, vp) = ctx.s.state().ok()?.active_viewport()?;
    // Paper units per model unit; display coordinates are the world turned by the view twist, so
    // the view centre in them is turned too.
    let k = vp.scale();
    let (view, sheet) = (vp.view_center.rotate(vp.view_twist()), vp.center.xy());
    let q = if to_paper {
        let xy = if disp { p.xy() * k } else { sheet + (p.xy() - view) * k };
        xy.to3(p.z * k)
    } else {
        let xy = if disp { p.xy() / k } else { view + (p.xy() - sheet) / k };
        xy.to3(p.z / k)
    };
    Some(q)
}

/// `(trans pt from to [disp])`: `pt` from coordinate system `from` to `to` (0 WCS, 1 UCS, 2 and 3
/// display, an entity name for its OCS, or an extrusion direction); with `disp` non-nil `pt` is a
/// displacement, which the systems' origins don't move.
pub fn trans(ctx: &mut Ctx<'_>, a: Vec<Value>) -> Result<Flow, LispError> {
    arity(&a, 3, 4)?;
    let p = a[0].as_point().ok_or_else(|| LispError::bad_arg("2D/3D point", &a[0]))?;
    let from = trans_system(ctx, &a[1])?;
    let to = trans_system(ctx, &a[2])?;
    let disp = a.get(3).is_some_and(|v| !v.is_nil());
    let p = cadcraft_geom::Vec3::new(p[0], p[1], p[2]);
    let q = if let Some(q) = viewport_to_paper(ctx, &a[1], &a[2], p, disp) {
        q
    } else if disp {
        let w = from.x * p.x + from.y * p.y + from.z() * p.z;
        cadcraft_geom::Vec3::new(w.dot(to.x), w.dot(to.y), w.dot(to.z()))
    } else {
        to.to_ucs3(from.to_wcs3(p))
    };
    if !q.is_finite() {
        return Err(LispError::bad_arg("2D/3D point", &a[0]));
    }
    Ok(Flow::Value(Value::point(&[q.x, q.y, q.z])))
}

/// What `(load name)` reads: a file loaded through APPLOAD (the web), or one on disk, with
/// `.lsp` added when the name has no extension.
pub fn load_source(s: &Session, name: &str) -> Option<String> {
    if let Some(t) = s.lisp_file(name) {
        return Some(t);
    }
    let candidates =
        if std::path::Path::new(name).extension().is_none() { vec![format!("{name}.lsp"), name.to_string()] } else { vec![name.to_string()] };
    // An absolute path is read as given. A relative name is looked up in the support folder only,
    // never in the process working folder: that can be the folder of a drawing just opened from a
    // file manager, and a `tools.lsp` shipped beside it must not answer a startup file's
    // `(load "tools")` (#1927). Without a support folder (the command-line tool, which has no
    // startup files) a relative name is read as given.
    let dir = match s.lisp_support_dir() {
        Some(d) if !std::path::Path::new(name).is_absolute() => d,
        _ => return candidates.iter().find_map(super::builtins::read_text_file),
    };
    // A relative name stays inside the support folder: plain folder and file names only. `..` would
    // climb out of it, and on Windows a rooted name (`\tools`) or a drive-relative one (`C:tools`,
    // the working folder of drive C) is not absolute, yet joined to the folder it replaces it.
    use std::path::Component;
    if !std::path::Path::new(name).components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir)) {
        return None;
    }
    candidates.iter().find_map(|c| super::builtins::read_text_file(dir.join(c)))
}

#[cfg(test)]
mod budget_tests {
    use super::*;

    /// One 64 KB string occurring many times is charged by its length each time.
    #[test]
    fn shared_strings_spend_the_conversion_budgets() {
        let big = Value::str(&"x".repeat(64 << 10));
        // 600 x 64 KB is over the 32 MiB an entity list may hold; 100 x 64 KB is not.
        let list = |n: usize| {
            let mut items = vec![Value::cons(Value::Int(0), Value::str("TEXT"))];
            items.extend(std::iter::repeat_n(Value::cons(Value::Int(1), big.clone()), n));
            Value::list(items)
        };
        assert!(list_to_groups(&list(600)).is_err());
        assert!(list_to_groups(&list(100)).is_ok());
        // 1,000 x 64 KB is 4,096,000 units of the 2,000,000 SETVAR allows; 100 x 64 KB is not.
        let mut budget = TO_JSON_BUDGET;
        assert!(to_json_bounded(&Value::list(vec![big.clone(); 1_000]), 0, &mut budget).is_none());
        let mut budget = TO_JSON_BUDGET;
        assert!(to_json_bounded(&Value::list(vec![big; 100]), 0, &mut budget).is_some());
    }
}
