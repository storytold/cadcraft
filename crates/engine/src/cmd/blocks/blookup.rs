//! BLOOKUPTABLE: the lookup table of a dynamic block's lookup action, in the Block Editor. A
//! lookup action (BACTION on a lookup parameter) starts empty; its table lists input properties
//! (a distance, a position's X or Y, a visibility state) and lookup properties (lookup
//! parameters), and each row gives the lookup values for one set of inputs. A reference then shows
//! the lookup value its inputs match ("Custom" when none does) and, when the lookup property
//! allows reverse lookup, setting it sets the row's inputs. The rules are AutoCAD's Property
//! Lookup Table dialog's ([`cadcraft_doc::dynblock::set_lookup_table`]); the UI opens that dialog
//! for BLOOKUPTABLE typed, and JSON calls fill the table directly.

use cadcraft_doc::dynblock::{
    ActionKind, DynamicBlock, MAX_LOOKUP_ROWS, ParameterKind, TableColumn, column_property, dynamic_block, input_properties, marks, set_lookup_table,
};
use serde_json::{Value, json};

use crate::cmd::{CommandSpec, bad};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("blookuptable", "Lookup Table", run)
            .params(
                "{action?: graph id, action name (\"Lookup1\") or its lookup parameter's property name (default the block's only lookup action); to fill it: inputs?: [input property names: \"Distance1\", \"Position1 X\", \"Position1 Y\", \"Visibility1\"] (default the table's), lookups?: [lookup parameter property names] (default the table's; its own first), rows?: [[a cell per input, then per lookup]] (numbers in block units, visibility states by name), unmatched?: text (shown when no row matches; default Custom) | {property: text}, reverse?: bool | [lookup property names] (allow reverse lookup; default as it was)} → {action, name, columns: [{name, parameter, property, lookup, unmatched, reverse}], rows} (no fill: just the table)",
            )
            .enabled(editing)
            .interactive(|_| Ok(Box::new(BlookupM))),
    ]
}

/// The Block Editor is open on a block (lookup tables belong to a block).
fn editing(s: &Session) -> std::result::Result<(), String> {
    match s.state() {
        Ok(st) => match &st.block_edit {
            Some(be) if be.current => Err("Lookup tables belong to a block; this editor is open on the drawing itself.".into()),
            Some(_) => Ok(()),
            None => Err("Open a block in the Block Editor first (BEDIT).".into()),
        },
        Err(e) => Err(e.to_string()),
    }
}

/// The edited block's name and its dynamic data.
fn block(s: &Session) -> Result<(String, DynamicBlock)> {
    editing(s).map_err(EngineError::Other)?;
    let st = s.state()?;
    let be = st.block_edit.as_ref().ok_or_else(|| EngineError::Other("The Block Editor is not open.".into()))?;
    let m = dynamic_block(&st.doc, &be.name).ok_or_else(none_yet)?;
    Ok((be.name.clone(), m))
}

fn none_yet() -> EngineError {
    bad("blookuptable", "the block has no lookup action; add a lookup parameter (BPARAMETER Lookup) and its action (BACTION)")
}

/// The block's lookup actions: (graph id, name).
pub fn lookup_actions(m: &DynamicBlock) -> Vec<(i32, String)> {
    m.actions.iter().filter(|a| matches!(a.kind, ActionKind::Lookup { .. })).map(|a| (a.node, a.name.clone())).collect()
}

/// The lookup action `v` names: its graph id, its name, or the property name of the lookup
/// parameter it gives; none: the block's only one.
fn action_named(m: &DynamicBlock, v: Option<&Value>) -> Result<i32> {
    let actions = lookup_actions(m);
    let by_text = |t: &str| {
        let t = t.trim();
        actions.iter().find(|(_, n)| n.eq_ignore_ascii_case(t)).map(|(id, _)| *id).or_else(|| {
            let p = m.parameters.iter().find(|p| matches!(&p.kind, ParameterKind::Lookup { label, .. } if label.eq_ignore_ascii_case(t)))?;
            m.actions
                .iter()
                .find(|a| matches!(&a.kind, ActionKind::Lookup { columns, .. } if columns.iter().any(|c| c.lookup && c.node == p.node)))
                .map(|a| a.node)
        })
    };
    let found = match v {
        None | Some(Value::Null) => match actions.as_slice() {
            [] => return Err(none_yet()),
            [(id, _)] => Some(*id),
            many => {
                let names: Vec<&str> = many.iter().map(|(_, n)| n.as_str()).collect();
                return Err(bad("blookuptable", format!("the block has {} lookup actions; name one (`action`): {}", many.len(), names.join(", "))));
            }
        },
        Some(Value::Number(n)) => n.as_i64().and_then(|n| i32::try_from(n).ok()).filter(|n| actions.iter().any(|(id, _)| id == n)),
        Some(Value::String(t)) => t.trim().parse::<i32>().ok().filter(|n| actions.iter().any(|(id, _)| id == n)).or_else(|| by_text(t)),
        Some(_) => None,
    };
    found.ok_or_else(|| bad("blookuptable", format!("no lookup action {}", v.map(Value::to_string).unwrap_or_default())))
}

/// Lookup action `action`'s table as BLOOKUPTABLE reports it.
pub fn describe(m: &DynamicBlock, action: i32) -> Value {
    let Some(a) = m.actions.iter().find(|a| a.node == action) else { return Value::Null };
    let ActionKind::Lookup { columns, rows } = &a.kind else { return Value::Null };
    let columns: Vec<Value> = columns
        .iter()
        .map(|c| {
            json!({
                "name": column_property(m, c.node, &c.property),
                "parameter": c.node,
                "property": c.property,
                "lookup": c.lookup,
                "unmatched": c.unmatched,
                "reverse": c.lookup && c.reverse,
            })
        })
        .collect();
    json!({ "action": action, "name": a.name, "columns": columns, "rows": rows })
}

/// A cell given in JSON: text, or a number written as text.
fn cell(v: &Value) -> Option<String> {
    match v {
        Value::String(t) => Some(t.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Null => Some(String::new()),
        _ => None,
    }
}

fn names(p: &Value, key: &str) -> Result<Option<Vec<String>>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) if a.len() <= 1_000 => a
            .iter()
            .map(|v| v.as_str().map(str::to_string).ok_or_else(|| bad("blookuptable", format!("`{key}` lists property names"))))
            .collect::<Result<_>>()
            .map(Some),
        Some(_) => Err(bad("blookuptable", format!("`{key}` is a list of property names"))),
    }
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let (name, m) = block(s)?;
    let action = action_named(&m, p.get("action"))?;
    let fill = ["inputs", "lookups", "rows", "unmatched", "reverse"].iter().any(|k| p.get(*k).is_some_and(|v| !v.is_null()));
    if !fill {
        return Ok(describe(&m, action));
    }
    let Some(ActionKind::Lookup { columns: old, rows: old_rows }) = m.actions.iter().find(|a| a.node == action).map(|a| &a.kind) else {
        return Err(none_yet());
    };
    let inputs = input_properties(&m);
    // A column of the table as it is, kept by name.
    let kept = |node: i32, property: &str, lookup: bool| {
        old.iter().find(|c| c.node == node && c.lookup == lookup && c.property.eq_ignore_ascii_case(property))
    };
    let mut columns: Vec<TableColumn> = Vec::new();
    match names(p, "inputs")? {
        Some(list) => {
            for n in list {
                let (_, node, property) = inputs.iter().find(|(shown, ..)| shown.eq_ignore_ascii_case(n.trim())).ok_or_else(|| {
                    bad(
                        "blookuptable",
                        format!(
                            "no input property \"{}\"; the block has: {}",
                            n.trim(),
                            inputs.iter().map(|(n, ..)| n.as_str()).collect::<Vec<_>>().join(", ")
                        ),
                    )
                })?;
                let unmatched = kept(*node, property, false).map(|c| c.unmatched.clone()).unwrap_or_default();
                columns.push(TableColumn { node: *node, property: property.clone(), lookup: false, unmatched, reverse: false });
            }
        }
        None => columns.extend(old.iter().filter(|c| !c.lookup).map(|c| TableColumn {
            node: c.node,
            property: c.property.clone(),
            lookup: false,
            unmatched: c.unmatched.clone(),
            reverse: false,
        })),
    }
    match names(p, "lookups")? {
        Some(list) => {
            for n in list {
                let node = m
                    .parameters
                    .iter()
                    .find(|q| matches!(&q.kind, ParameterKind::Lookup { label, .. } if label.eq_ignore_ascii_case(n.trim())))
                    .map(|q| q.node)
                    .ok_or_else(|| bad("blookuptable", format!("no lookup property \"{}\"", n.trim())))?;
                let k = kept(node, "lookupString", true);
                columns.push(TableColumn {
                    node,
                    property: "lookupString".into(),
                    lookup: true,
                    unmatched: k.map(|c| c.unmatched.clone()).unwrap_or_default(),
                    reverse: k.is_some_and(|c| c.reverse),
                });
            }
        }
        None => columns.extend(old.iter().filter(|c| c.lookup).map(|c| TableColumn {
            node: c.node,
            property: c.property.clone(),
            lookup: true,
            unmatched: c.unmatched.clone(),
            reverse: c.reverse,
        })),
    }
    // What references show when no row matches.
    match p.get("unmatched") {
        None | Some(Value::Null) => {}
        Some(Value::String(t)) => columns.iter_mut().for_each(|c| c.unmatched = t.clone()),
        Some(Value::Object(o)) => {
            for (k, v) in o {
                let t = v.as_str().ok_or_else(|| bad("blookuptable", "`unmatched` values are text"))?;
                let c = columns
                    .iter_mut()
                    .find(|c| column_property(&m, c.node, &c.property).eq_ignore_ascii_case(k.trim()))
                    .ok_or_else(|| bad("blookuptable", format!("no column \"{k}\" in the table")))?;
                c.unmatched = t.to_string();
            }
        }
        Some(_) => Err(bad("blookuptable", "`unmatched` is text or {property: text}"))?,
    }
    match p.get("reverse") {
        None | Some(Value::Null) => {}
        Some(Value::Bool(b)) => columns.iter_mut().filter(|c| c.lookup).for_each(|c| c.reverse = *b),
        Some(Value::Array(list)) => {
            for c in columns.iter_mut().filter(|c| c.lookup) {
                let shown = column_property(&m, c.node, &c.property);
                c.reverse = list.iter().any(|v| v.as_str().is_some_and(|n| n.trim().eq_ignore_ascii_case(&shown)));
            }
        }
        Some(_) => Err(bad("blookuptable", "`reverse` is true, false or a list of lookup property names"))?,
    }
    let rows: Vec<Vec<String>> = match p.get("rows") {
        None | Some(Value::Null) => {
            // The table's rows, when its columns stay as they were.
            let same =
                columns.len() == old.len() && columns.iter().zip(old).all(|(a, b)| a.node == b.node && a.property.eq_ignore_ascii_case(&b.property));
            if same || old_rows.is_empty() {
                old_rows.clone()
            } else {
                return Err(bad("blookuptable", "the columns changed; give `rows` for them"));
            }
        }
        Some(Value::Array(rows)) if rows.len() <= MAX_LOOKUP_ROWS => rows
            .iter()
            .map(|r| match r {
                Value::Array(cells) if cells.len() <= 1_000 => {
                    cells.iter().map(|c| cell(c).ok_or_else(|| bad("blookuptable", "a cell is text or a number"))).collect::<Result<Vec<_>>>()
                }
                _ => Err(bad("blookuptable", "each of `rows` is a list of cells")),
            })
            .collect::<Result<_>>()?,
        Some(_) => return Err(bad("blookuptable", format!("`rows` is a list of at most {MAX_LOOKUP_ROWS} rows"))),
    };
    set_lookup_table(s.doc_mut()?, &name, action, &columns, &rows).map_err(|e| bad("blookuptable", e))?;
    let m = dynamic_block(s.doc()?, &name).ok_or_else(none_yet)?;
    Ok(describe(&m, action))
}

/// BLOOKUPTABLE on the command line (the UI opens the dialog instead): pick a lookup action, and
/// its table is listed.
struct BlookupM;

impl Interactive for BlookupM {
    fn name(&self) -> &'static str {
        "BLOOKUPTABLE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Select lookup action", Accept::POINT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let at = match i {
            Input::Cancel | Input::Enter => return Ok(Step::Cancel),
            Input::Point(p) => p,
            Input::Text(t) | Input::Keyword(t) => {
                let (_, m) = block(s)?;
                let action = action_named(&m, Some(&Value::String(t)))?;
                list(s, &m, action);
                return Ok(Step::Done);
            }
            _ => return Ok(Step::Continue),
        };
        let (name, m) = block(s)?;
        let base = s.state()?.block_edit.as_ref().map(|b| b.base()).unwrap_or_default();
        let reach = (s.pixel_size() * s.settings.pickbox.max(1.0) * 3.0).max(1e-9);
        let actions = lookup_actions(&m);
        let hit = marks(s.doc()?, &name)
            .into_iter()
            .filter_map(|k| match k {
                cadcraft_doc::dynblock::Mark::Action { node, at: a, .. } if actions.iter().any(|(id, _)| *id == node) => {
                    Some((node, (a.xy() - base - at).len()))
                }
                _ => None,
            })
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match hit {
            Some((action, _)) => {
                list(s, &m, action);
                Ok(Step::Done)
            }
            None => {
                s.echo("Nothing Selected.");
                Ok(Step::Continue)
            }
        }
    }
}

/// Echo action `action`'s table: its columns, then each row.
fn list(s: &mut Session, m: &DynamicBlock, action: i32) {
    let t = describe(m, action);
    let heads: Vec<String> =
        t["columns"].as_array().map(|c| c.iter().map(|c| c["name"].as_str().unwrap_or_default().to_string()).collect()).unwrap_or_default();
    s.echo(format!("{}: {}", t["name"].as_str().unwrap_or_default(), heads.join(" | ")));
    for r in t["rows"].as_array().into_iter().flatten() {
        let cells: Vec<&str> = r.as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        s.echo(format!("  {}", cells.join(" | ")));
    }
}
