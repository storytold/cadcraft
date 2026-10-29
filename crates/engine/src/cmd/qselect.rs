//! QSELECT: build a selection set from an object type and a property filter
//! (`property operator value`), and `qselect.info` for the Quick Select dialog.

use cadcraft_color::Color;
use cadcraft_doc::{Drawing, Entity, Handle};
use serde_json::{Value, json};

use super::{CommandSpec, bad, str_param};

use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("qselect", "Quick Select...", run_qselect)
            .menu(&["Edit", "Quick Select..."])
            .alias(&["qs"])
            .params(
                "{type?: \"Line\"|\"Circle\"|..., property?, operator?: \"=\"|\"!=\"|\">\"|\"<\"|\"*\" (wildcard), value?, mode?: new|append|exclude, \
                 applyTo?: drawing|selection, layer?, color?} → selected handles",
            )
            .noundo(),
        CommandSpec::new("qselect.info", "Quick Select Properties", run_info)
            .params("{applyTo?: drawing|selection, type?} → object types with counts and the properties available for `type`")
            .noundo(),
    ]
}

/// Most entities this command looks at (a hostile drawing can be huge).
const MAX_SCAN: usize = 2_000_000;

/// The comparison operators QSELECT supports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Gt,
    Lt,
    Wild,
}

impl Op {
    pub fn parse(s: &str) -> Option<Op> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "=" | "==" | "equals" | "eq" => Op::Eq,
            "!=" | "<>" | "not equal" | "notequal" | "ne" => Op::Ne,
            ">" | "greater" | "greater than" | "gt" => Op::Gt,
            "<" | "less" | "less than" | "lt" => Op::Lt,
            "*" | "wildcard" | "wildcard match" | "like" => Op::Wild,
            _ => return None,
        })
    }
}

/// Lower-case, no spaces/underscores: `Start X` → `startx`, `arc_length` → `arclength`.
fn norm(k: &str) -> String {
    k.chars().filter(|c| !c.is_whitespace() && *c != '_' && *c != '-').flat_map(char::to_lowercase).collect()
}

/// Flattened scalar properties of an entity: (display name, value).
pub fn properties(d: &Drawing, e: &Entity) -> Vec<(String, Value)> {
    let v = super::props::entity_props(d, e);
    let mut out: Vec<(String, Value)> = Vec::new();
    let mut push = |k: &str, val: &Value| {
        if matches!(val, Value::String(_) | Value::Number(_) | Value::Bool(_)) && !out.iter().any(|(n, _)| norm(n) == norm(k)) {
            out.push((k.to_string(), val.clone()));
        }
    };
    if let Some(o) = v.as_object() {
        for k in ["type", "layer", "color", "linetype", "lineweight", "ltscale", "handle", "visible"] {
            if let Some(x) = o.get(k) {
                push(k, x);
            }
        }
        for (k, x) in o {
            if k != "geometry" && k != "bounds" {
                push(k, x);
            }
        }
        if let Some(g) = o.get("geometry").and_then(Value::as_object) {
            for (k, x) in g {
                if k == "type" {
                    continue;
                }
                match x {
                    // Points: expose their coordinates (`centerX`, `insertY`, …).
                    Value::Object(p) => {
                        for c in ["x", "y", "z"] {
                            if let Some(n) = p.get(c) {
                                push(&format!("{k}{}", c.to_ascii_uppercase()), n);
                            }
                        }
                    }
                    _ => push(k, x),
                }
            }
        }
    }
    out
}

fn property(d: &Drawing, e: &Entity, name: &str) -> Option<Value> {
    let n = norm(name);
    if n == "objecttype" {
        return Some(Value::String(e.kind.type_name().into()));
    }
    properties(d, e).into_iter().find(|(k, _)| norm(k) == n).map(|(_, v)| v)
}

/// Glob match, case-insensitive: `*` any run, `?` one character, `#` one digit.
pub fn wildcard(pat: &str, text: &str) -> bool {
    let p: Vec<char> = pat.to_lowercase().chars().take(1024).collect();
    let t: Vec<char> = text.to_lowercase().chars().take(4096).collect();
    // Iterative matcher with single back-track point (linear-ish, no recursion).
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        let pc = p.get(pi).copied();
        let tc = t.get(ti).copied();
        match (pc, tc) {
            (Some('*'), _) => {
                star = Some((pi, ti));
                pi += 1;
            }
            (Some('?'), Some(_)) => {
                pi += 1;
                ti += 1;
            }
            (Some('#'), Some(c)) if c.is_ascii_digit() => {
                pi += 1;
                ti += 1;
            }
            (Some(a), Some(b)) if a == b && a != '#' => {
                pi += 1;
                ti += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    pi = sp + 1;
                    ti = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    p.iter().skip(pi).all(|c| *c == '*')
}

fn as_num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        Value::String(s) => crate::units::parse_distance(s.trim()).or_else(|| s.trim().parse::<f64>().ok()),
        _ => None,
    }
    .filter(|f| f.is_finite())
}

fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => if *b { "Yes" } else { "No" }.into(),
        other => other.to_string(),
    }
}

/// Does `prop` satisfy `op value`?
pub fn compare(prop: &Value, op: Op, value: &Value) -> bool {
    let num = matches!(prop, Value::Number(_)).then(|| as_num(prop)).flatten().zip(as_num(value));
    let ptxt = as_text(prop);
    let vtxt = as_text(value);
    let same = || match num {
        Some((a, b)) => (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0),
        None => {
            if ptxt.eq_ignore_ascii_case(vtxt.trim()) {
                return true;
            }
            // Colours: `red` = `1` = `Red`; `bylayer` = `ByLayer`.
            match (Color::parse(&ptxt), Color::parse(vtxt.trim())) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            }
        }
    };
    match op {
        Op::Eq => same(),
        Op::Ne => !same(),
        Op::Gt => match num {
            Some((a, b)) => a > b,
            None => ptxt.to_lowercase() > vtxt.to_lowercase(),
        },
        Op::Lt => match num {
            Some((a, b)) => a < b,
            None => ptxt.to_lowercase() < vtxt.to_lowercase(),
        },
        Op::Wild => wildcard(vtxt.trim(), &ptxt),
    }
}

fn type_matches(e: &Entity, ty: Option<&str>) -> bool {
    match ty {
        None => true,
        Some(t) => {
            let t = t.trim();
            t.is_empty()
                || t == "*"
                || t.eq_ignore_ascii_case("multiple")
                || t.eq_ignore_ascii_case("all")
                || e.kind.type_name().eq_ignore_ascii_case(t)
                || e.kind.dxf_name().eq_ignore_ascii_case(t)
        }
    }
}

/// The candidate objects: the whole edit space, or the current selection.
fn candidates(s: &Session, p: &Value) -> Result<Vec<Handle>> {
    let apply = str_param(p, "applyTo").unwrap_or("drawing").to_ascii_lowercase();
    if apply.starts_with("sel") || apply == "current" {
        return Ok(s.selection());
    }
    let space = s.space();
    Ok(s.doc()?.space(&space).map(|st| st.iter().take(MAX_SCAN).map(|e| e.handle).collect()).unwrap_or_default())
}

fn run_qselect(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "qselect";
    let ty = str_param(p, "type");
    let op = match str_param(p, "operator") {
        Some(o) => Some(Op::parse(o).ok_or_else(|| bad(cmd, format!("unknown operator `{o}` (use =, !=, >, <, *)")))?),
        None => None,
    };
    let prop = str_param(p, "property").map(str::to_string);
    let value = p.get("value").cloned();
    if prop.is_some() && value.is_none() && !matches!(op, Some(Op::Wild)) {
        return Err(bad(cmd, "`value` is required with `property`"));
    }
    let mode = str_param(p, "mode").unwrap_or("new").to_ascii_lowercase();
    if !["new", "append", "exclude", "include"].contains(&mode.as_str()) {
        return Err(bad(cmd, format!("unknown mode `{mode}` (use new, append or exclude)")));
    }
    let layer = str_param(p, "layer");
    let color = p.get("color").filter(|c| !c.is_null());
    let cands = candidates(s, p)?;
    let d = s.doc()?;
    let mut matched = Vec::new();
    let mut excluded = Vec::new();
    for h in &cands {
        let Some(e) = d.entity(*h) else { continue };
        if !type_matches(e, ty) {
            continue;
        }
        let mut ok = true;
        if let Some(l) = layer {
            ok &= e.common.layer.eq_ignore_ascii_case(l);
        }
        if let Some(c) = color {
            ok &= compare(&Value::String(e.common.color.name()), Op::Eq, c);
        }
        if let Some(pr) = &prop {
            let v = value.clone().unwrap_or(Value::String("*".into()));
            ok &= match property(d, e, pr) {
                Some(pv) => compare(&pv, op.unwrap_or(Op::Eq), &v),
                // A property the object doesn't have never matches (`!=` included, like QSELECT).
                None => false,
            };
        }
        if ok {
            matched.push(*h);
        } else {
            excluded.push(*h);
        }
    }
    let sel = match mode.as_str() {
        "exclude" => excluded,
        "append" => {
            let mut cur = s.selection();
            for h in matched {
                if !cur.contains(&h) {
                    cur.push(h);
                }
            }
            cur
        }
        _ => matched,
    };
    s.set_selection(sel);
    let sel = s.selection();
    let n = sel.len();
    Ok(json!({ "selected": n, "handles": sel.iter().map(|h| h.hex()).collect::<Vec<_>>(), "message": format!("{n} item(s) selected") }))
}

fn run_info(s: &mut Session, p: &Value) -> Result<Value> {
    let cands = candidates(s, p)?;
    let ty = str_param(p, "type");
    let d = s.doc()?;
    let mut types: Vec<(String, usize)> = Vec::new();
    let mut props: Vec<String> = Vec::new();
    for h in &cands {
        let Some(e) = d.entity(*h) else { continue };
        let tn = e.kind.type_name();
        match types.iter_mut().find(|(n, _)| n == tn) {
            Some((_, c)) => *c += 1,
            None => types.push((tn.to_string(), 1)),
        }
        if type_matches(e, ty) && props.len() < 200 {
            for (k, _) in properties(d, e) {
                if k != "handle" && !props.contains(&k) {
                    props.push(k);
                }
            }
        }
    }
    types.sort();
    Ok(json!({
        "count": cands.len(),
        "types": types.iter().map(|(n, c)| json!({ "type": n, "count": c })).collect::<Vec<_>>(),
        "properties": props,
        "operators": ["=", "!=", ">", "<", "*"],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session() -> Session {
        let mut s = Session::new();
        s.execute("layer.new", &json!({ "name": "Walls" })).unwrap();
        s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
        s.execute("line", &json!({ "points": [[0, 0], [3, 4]] })).unwrap();
        s.execute("circle", &json!({ "center": [0, 0], "radius": 2 })).unwrap();
        s.execute("circle", &json!({ "center": [5, 5], "radius": 7 })).unwrap();
        let h = s.doc().unwrap().model.iter().last().unwrap().handle.hex();
        s.execute("properties.set", &json!({ "handles": [h], "layer": "Walls", "color": 1 })).unwrap();
        s.set_selection(Vec::new());
        s
    }

    fn n(v: &Value) -> u64 {
        v["selected"].as_u64().unwrap()
    }

    #[test]
    fn type_and_property_filters() {
        let mut s = session();
        assert_eq!(n(&s.execute("qselect", &json!({ "type": "Line" })).unwrap()), 2);
        assert_eq!(n(&s.execute("qselect", &json!({ "type": "Circle", "property": "radius", "operator": ">", "value": 3 })).unwrap()), 1);
        assert_eq!(n(&s.execute("qselect", &json!({ "type": "Circle", "property": "Radius", "operator": "<", "value": "3" })).unwrap()), 1);
        assert_eq!(n(&s.execute("qselect", &json!({ "type": "Line", "property": "length", "operator": "=", "value": 5 })).unwrap()), 1);
        assert_eq!(n(&s.execute("qselect", &json!({ "property": "layer", "operator": "=", "value": "walls" })).unwrap()), 1);
        assert_eq!(n(&s.execute("qselect", &json!({ "property": "layer", "operator": "!=", "value": "Walls" })).unwrap()), 3);
        assert_eq!(n(&s.execute("qselect", &json!({ "property": "layer", "operator": "*", "value": "W*s" })).unwrap()), 1);
        assert_eq!(n(&s.execute("qselect", &json!({ "property": "color", "operator": "=", "value": "red" })).unwrap()), 1);
        assert_eq!(n(&s.execute("qselect", &json!({ "property": "color", "operator": "=", "value": 1 })).unwrap()), 1);
        assert_eq!(n(&s.execute("qselect", &json!({ "property": "centerX", "operator": "=", "value": 5 })).unwrap()), 1);
        // Back-compatible short form.
        assert_eq!(n(&s.execute("qselect", &json!({ "layer": "Walls" })).unwrap()), 1);
    }

    #[test]
    fn modes_append_exclude_and_scope() {
        let mut s = session();
        s.execute("qselect", &json!({ "type": "Circle" })).unwrap();
        assert_eq!(
            n(&s.execute("qselect", &json!({ "type": "Line", "property": "length", "operator": ">", "value": 6, "mode": "append" })).unwrap()),
            3
        );
        // Exclude: everything that does NOT match.
        assert_eq!(n(&s.execute("qselect", &json!({ "property": "layer", "operator": "=", "value": "Walls", "mode": "exclude" })).unwrap()), 3);
        // Apply to the current selection only.
        let r = s.execute("qselect", &json!({ "applyTo": "selection", "type": "Line" })).unwrap();
        assert_eq!(n(&r), 2);
        let info = s.execute("qselect.info", &json!({ "type": "Circle" })).unwrap();
        assert_eq!(info["types"].as_array().unwrap().len(), 2);
        assert!(info["properties"].as_array().unwrap().iter().any(|p| p == "radius"));
    }

    #[test]
    fn hostile_params_are_errors_not_panics() {
        let mut s = session();
        assert!(s.execute("qselect", &json!({ "operator": "~~" })).is_err());
        assert!(s.execute("qselect", &json!({ "mode": "sideways" })).is_err());
        assert!(s.execute("qselect", &json!({ "property": "radius" })).is_err());
        for v in [json!(null), json!([1, 2]), json!({"a": 1}), json!(f64::MAX), json!("*".repeat(5000))] {
            let _ = s.execute("qselect", &json!({ "property": "radius", "operator": "*", "value": v }));
            let _ = s.execute("qselect", &json!({ "property": v, "operator": ">", "value": v, "type": v }));
        }
    }

    #[test]
    fn wildcards() {
        assert!(wildcard("W*", "walls"));
        assert!(wildcard("*", ""));
        assert!(wildcard("a?c", "abc"));
        assert!(wildcard("L#", "L7"));
        assert!(!wildcard("L#", "Lx"));
        assert!(!wildcard("a*d", "abc"));
        assert!(wildcard("*b*", "abc"));
        assert!(!wildcard(&"*a".repeat(500), &"b".repeat(4000)));
    }
}
