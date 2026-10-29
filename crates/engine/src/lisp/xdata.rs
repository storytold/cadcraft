//! Extended entity data (xdata) between the drawing and LISP.
//!
//! An entity carries other applications' data as a flat run of DXF groups, each application from
//! its `1001` group on ([`cadcraft_doc::Common::xdata`]). AutoLISP shows the same thing under the
//! group `-3`, one sublist per application:
//!
//! ```text
//! (-3 ("ACME" (1000 . "D-07") (1070 . 3)))
//! ```
//!
//! `entget` adds that item only for the applications asked for, as AutoCAD does: without the
//! second argument an entity list has no `-3` at all. `entmod` takes the same shape back, and
//! replaces only the applications named in the list — the ones it doesn't mention are left alone,
//! and an application with no data after its name is removed.

use cadcraft_doc::RawTag;

use super::LispError;
use super::value::Value;

/// The group that opens an application's data.
pub(super) const APP: i32 = 1001;

/// The lowest group code xdata can hold.
///
/// The reader agrees, and it is not gentle about it: `foreign_xdata` drops a whole application
/// if any of its groups is below `1000`. So a list that smuggled one in wrote a file that came
/// back without the data - and worse than that, a `(0 . "CIRCLE")` in a `-3` went into the file
/// as a group `0`, which ends the entity, so every reader took what followed as a new one. The
/// check belongs here, on the way in, where it can still be refused.
const FIRST: i32 = 1000;

/// The most groups one entity's xdata can hold before the reader drops it (its
/// `MAX_XDATA_GROUPS`).
pub(super) const MAX_GROUPS: usize = 16_384;

/// A `1070` is a 16-bit integer in the file, and a `1071` a 32-bit one.
const LIMIT_1070: f64 = 32_767.0;
const LIMIT_1071: f64 = 2_147_483_647.0;

/// A value of a group inside xdata, by what that group holds.
///
/// Group codes carry the type in DXF: `1000`–`1005` are text, `1040`–`1042` reals, `1070` and
/// `1071` integers. Everything is stored as text in [`RawTag`], so the parsing happens here;
/// text that isn't a number stays text, because a drawing from another program is not ours to
/// reject.
fn value_of(t: &RawTag) -> Value {
    let s = t.value.trim();
    match t.code {
        1040..=1042 => s.parse::<f64>().map_or_else(|_| Value::str(&t.value), Value::Real),
        1070 | 1071 => s.parse::<i32>().map_or_else(|_| Value::str(&t.value), Value::Int),
        _ => Value::str(&t.value),
    }
}

/// Is this the x of a point inside xdata? (`1010`–`1013`, with y at `+10` and z at `+20`.)
fn point_x(code: i32) -> bool {
    matches!(code, 1010..=1013)
}

/// The groups of one application as LISP pairs, points joined into `(1010 x y z)`.
fn pairs(tags: &[RawTag]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(t) = tags.get(i) {
        if point_x(t.code) {
            let y = tags.get(i + 1).filter(|n| n.code == t.code + 10);
            let z = tags.get(i + 2).filter(|n| n.code == t.code + 20);
            if let (Some(y), Some(z)) = (y, z) {
                let n = |t: &RawTag| t.value.trim().parse::<f64>().unwrap_or(0.0);
                out.push(Value::list(vec![Value::Int(t.code), Value::Real(n(t)), Value::Real(n(y)), Value::Real(n(z))]));
                i += 3;
                continue;
            }
        }
        out.push(Value::cons(Value::Int(t.code), value_of(t)));
        i += 1;
    }
    out
}

/// Split a flat run of xdata into `(application, its groups)`, in the order the entity holds them.
///
/// Groups before the first `1001` belong to no application and are dropped: they cannot be
/// addressed by name, and carrying them into a `-3` would invent an application.
pub fn blocks(xdata: &[RawTag]) -> Vec<(String, Vec<RawTag>)> {
    let mut out: Vec<(String, Vec<RawTag>)> = Vec::new();
    for t in xdata {
        if t.code == APP {
            out.push((t.value.trim().to_string(), Vec::new()));
        } else if let Some(last) = out.last_mut() {
            last.1.push(t.clone());
        }
    }
    out
}

/// Back to the flat run an entity holds.
fn flatten(blocks: Vec<(String, Vec<RawTag>)>) -> Vec<RawTag> {
    let mut out = Vec::new();
    for (app, tags) in blocks {
        out.push(RawTag { code: APP, value: app });
        out.extend(tags);
    }
    out
}

/// The `(-3 ...)` item of an entity list, for the applications asked for.
///
/// `apps` holds the names from `entget`'s second argument; `"*"` means every application. Returns
/// `None` when nothing matches, because an entity list with an empty `-3` would say "this entity
/// has data for that application, and it is empty" — which is not the same as having none.
pub fn item(xdata: &[RawTag], apps: &[String]) -> Option<Value> {
    let all = apps.iter().any(|a| a == "*");
    let mut sublists = Vec::new();
    for (app, tags) in blocks(xdata) {
        if !all && !apps.iter().any(|a| a.eq_ignore_ascii_case(&app)) {
            continue;
        }
        let mut items = vec![Value::str(&app)];
        items.extend(pairs(&tags));
        sublists.push(Value::list(items));
    }
    if sublists.is_empty() {
        return None;
    }
    let mut items = vec![Value::Int(-3)];
    items.extend(sublists);
    Some(Value::list(items))
}

/// The application names of `entget`'s second argument.
pub fn wanted(v: &Value) -> Result<Vec<String>, LispError> {
    if v.is_nil() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for item in v.iter() {
        match item {
            Value::Str(s) => out.push(s.trim().to_string()),
            other => return Err(LispError::bad_arg("application name", &other)),
        }
    }
    Ok(out)
}

/// Any text a group can hold.
fn text_of(v: &Value) -> Result<String, LispError> {
    Ok(match v {
        Value::Str(s) => s.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Real(r) => to_num(*r),
        Value::Ename(h) => h.hex(),
        other => return Err(LispError::bad_arg("DXF group value", other)),
    })
}

/// An integer group's text, refused if it doesn't fit the group's width in the file.
fn int_of(code: i32, v: &Value, limit: f64) -> Result<String, LispError> {
    let n = v.as_number().filter(|n| n.fract() == 0.0 && (-limit - 1.0..=limit).contains(n));
    match n {
        // integral and inside the range checked just above, so this is exact
        Some(n) => Ok((n as i64).to_string()),
        None => Err(LispError::new(format!("{code} inside -3: an integer of at most {limit} was wanted"))),
    }
}

/// One group's value as text, by what that group holds in the file.
///
/// DXF gives each xdata code a type, and another program reading the file expects it: a `1002`
/// is one brace, a `1070` is 16 bits wide. A code at or above [`FIRST`] that DXF gives no type
/// to is kept as text, because a drawing from elsewhere may carry one and a round trip through
/// LISP must not drop it.
fn value_for(code: i32, v: &Value) -> Result<String, LispError> {
    match code {
        1002 => match v {
            Value::Str(s) if matches!(s.trim(), "{" | "}") => Ok(s.trim().to_string()),
            _ => Err(LispError::new("1002 inside -3: only \"{\" or \"}\"")),
        },
        1040..=1042 => match v.as_number() {
            Some(r) => Ok(to_num(r)),
            None => Err(LispError::new(format!("{code} inside -3: a real was wanted"))),
        },
        1070 => int_of(code, v, LIMIT_1070),
        1071 => int_of(code, v, LIMIT_1071),
        _ => text_of(v),
    }
}

/// A group inside a `-3` sublist, back to a stored tag.
///
/// What is refused here is what the file cannot carry: see [`FIRST`] for why that refusal is
/// the point of this module rather than an inconvenience.
fn tag_from(item: &Value) -> Result<Vec<RawTag>, LispError> {
    let code = match item.car() {
        Value::Int(c) => c,
        other => return Err(LispError::bad_arg("fixnump", &other)),
    };
    if code == APP {
        // the application's name is the sublist's head, never a group inside it; accepting it
        // here would let one application write another's name
        return Err(LispError::new("1001 inside -3: the application name is the sublist's first element"));
    }
    if code < FIRST {
        return Err(LispError::new(format!("{code} inside -3: xdata holds group codes {FIRST} and up")));
    }
    let v = item.cdr();
    // a point comes as (1010 x y z) and is stored as three groups, like the file holds it
    if point_x(code) {
        let Some(p) = v.as_point() else {
            return Err(LispError::new(format!("{code} inside -3: a point was wanted")));
        };
        return Ok(vec![
            RawTag { code, value: to_num(p[0]) },
            RawTag { code: code + 10, value: to_num(p[1]) },
            RawTag { code: code + 20, value: to_num(p[2]) },
        ]);
    }
    Ok(vec![RawTag { code, value: value_for(code, &v)? }])
}

/// A real on its way into a tag.
///
/// Not `value::to_text`, which is the AutoLISP *display* format: it rounds to six significant
/// digits, so storing through it turned a coordinate's last digits into nothing on the way in,
/// before anything could read them back. `to_string` round-trips, and it is what the document
/// already uses for a real in a tag.
fn to_num(v: f64) -> String {
    v.to_string()
}

/// Do this application's `1002` braces open and close, as the reader insists?
fn balanced(tags: &[RawTag]) -> bool {
    let mut depth = 0i32;
    for t in tags.iter().filter(|t| t.code == 1002) {
        match t.value.trim() {
            "{" => depth += 1,
            _ => depth -= 1,
        }
        if depth < 0 {
            return false;
        }
    }
    depth == 0
}

/// The applications a `(-3 ...)` item names, with their groups.
pub fn from_item(item: &Value) -> Result<Vec<(String, Vec<RawTag>)>, LispError> {
    let mut out = Vec::new();
    // the item is (-3 (app …) (app …)): skip the -3 itself
    for sublist in item.cdr().iter() {
        let app = match sublist.car() {
            Value::Str(s) => s.trim().to_string(),
            other => return Err(LispError::bad_arg("application name", &other)),
        };
        if app.is_empty() {
            return Err(LispError::new("-3: an application with no name"));
        }
        let mut tags = Vec::new();
        for g in sublist.cdr().iter() {
            tags.extend(tag_from(&g)?);
        }
        // the braces are an application's own business, but they have to close: the reader
        // drops the whole application when they don't, so an unbalanced list would be data
        // that reads back as nothing
        if !balanced(&tags) {
            return Err(LispError::new(format!("{app} inside -3: the 1002 braces do not close")));
        }
        out.push((app, tags));
    }
    Ok(out)
}

/// What the entity should hold after `entmod`.
///
/// ONLY THE APPLICATIONS THE LIST NAMES CHANGE. This is the whole point: another program's data
/// (a hyperlink, an annotation scale, a foreign tag) sits in the same place, and an `entmod` that
/// rebuilt the list from scratch would throw it away without saying so. An application named with
/// no groups after it is removed, as AutoCAD does.
pub fn merge(old: &[RawTag], incoming: Vec<(String, Vec<RawTag>)>) -> Vec<RawTag> {
    let mut out = blocks(old);
    for (app, tags) in incoming {
        let at = out.iter().position(|(a, _)| a.eq_ignore_ascii_case(&app));
        match (at, tags.is_empty()) {
            (Some(i), true) => {
                out.remove(i);
            }
            // get_mut and not out[i]: the index comes from `position` just above and cannot
            // be out of bounds, but the house rule is never to write a panic that a reader has
            // to prove impossible
            (Some(i), false) => {
                if let Some(slot) = out.get_mut(i) {
                    *slot = (app, tags);
                }
            }
            (None, true) => {}
            (None, false) => out.push((app, tags)),
        }
    }
    flatten(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(code: i32, value: &str) -> RawTag {
        RawTag { code, value: value.into() }
    }

    fn xd() -> Vec<RawTag> {
        vec![t(1001, "PE_URL"), t(1000, "https://x/7"), t(1001, "ACME"), t(1000, "D-07"), t(1070, "3")]
    }

    #[test]
    fn blocks_split_by_application() {
        let b = blocks(&xd());
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].0, "PE_URL");
        assert_eq!(b[1].0, "ACME");
        assert_eq!(b[1].1.len(), 2);
    }

    #[test]
    fn groups_before_the_first_application_are_dropped() {
        let b = blocks(&[t(1000, "orphan"), t(1001, "ACME"), t(1000, "D-07")]);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].1.len(), 1);
    }

    #[test]
    fn item_holds_only_the_applications_asked_for() {
        let v = item(&xd(), &["ACME".into()]).unwrap();
        let text = crate::lisp::value::to_text(&v, false);
        assert!(text.contains("ACME"), "{text}");
        assert!(!text.contains("PE_URL"), "{text}");
    }

    #[test]
    fn the_star_asks_for_every_application() {
        let text = crate::lisp::value::to_text(&item(&xd(), &["*".into()]).unwrap(), false);
        assert!(text.contains("ACME") && text.contains("PE_URL"), "{text}");
    }

    #[test]
    fn an_application_the_entity_does_not_have_gives_nothing() {
        assert!(item(&xd(), &["NOBODY".into()]).is_none());
    }

    #[test]
    fn a_number_keeps_its_type() {
        let text = crate::lisp::value::to_text(&item(&xd(), &["ACME".into()]).unwrap(), false);
        // 1070 is an integer, and must not come back as "3.0"
        assert!(text.contains("(1070 . 3)"), "{text}");
    }

    #[test]
    fn merge_replaces_only_what_was_named() {
        let merged = merge(&xd(), vec![("ACME".into(), vec![t(1000, "D-99")])]);
        let b = blocks(&merged);
        assert_eq!(b.len(), 2);
        // another program's hyperlink is still there
        assert_eq!(b[0].0, "PE_URL");
        assert_eq!(b[0].1[0].value, "https://x/7");
        assert_eq!(b[1].1.len(), 1);
        assert_eq!(b[1].1[0].value, "D-99");
    }

    #[test]
    fn merge_adds_an_application_that_was_not_there() {
        let merged = merge(&xd(), vec![("NEWAPP".into(), vec![t(1000, "panel=P-1")])]);
        assert_eq!(blocks(&merged).len(), 3);
    }

    #[test]
    fn an_application_named_with_no_groups_is_removed() {
        let merged = merge(&xd(), vec![("ACME".into(), Vec::new())]);
        let b = blocks(&merged);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].0, "PE_URL");
    }

    #[test]
    fn merge_keeps_everything_when_nothing_is_named() {
        assert_eq!(merge(&xd(), Vec::new()), xd());
    }
}
