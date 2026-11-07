//! RENAME for blocks and the remaining named tables, and -RENAME's prompts.

use std::sync::Arc;

use cadcraft_doc::{Drawing, EntityKind, EntityStore, Handle};
use serde_json::json;

use super::super::bad;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub(super) fn valid_name(n: &str) -> bool {
    !n.trim().is_empty() && n.len() <= 255 && !n.starts_with('*') && !n.chars().any(|c| "<>/\\\":;?*|,=`".contains(c))
}

/// Handles in `store` whose entity `uses` the old name.
fn users(store: &EntityStore, uses: &dyn Fn(&EntityKind) -> bool) -> Vec<Handle> {
    store.iter().filter(|e| uses(&e.kind)).map(|e| e.handle).collect()
}

/// Apply `fix` to every entity that `uses` the old name: model space, layouts and block definitions.
fn repoint(d: &mut Drawing, uses: &dyn Fn(&EntityKind) -> bool, fix: &dyn Fn(&mut EntityKind)) -> Result<()> {
    let mut hs = users(&d.model, uses);
    hs.extend(d.layouts.iter().flat_map(|l| users(&l.entities, uses)));
    for h in hs {
        d.modify_entity(h, |e| fix(&mut e.kind))?;
    }
    for block in d.blocks.values_mut() {
        let hs = users(&block.entities, uses);
        if !hs.is_empty() {
            let b = Arc::make_mut(block);
            for h in hs {
                b.entities.modify(h, |e| fix(&mut e.kind));
            }
        }
    }
    Ok(())
}

/// Rename a block definition; inserts everywhere (nested ones and those with attributes
/// included), INSNAME and dimension-style arrowheads follow.
pub(super) fn block(s: &mut Session, from: &str, to: &str) -> Result<()> {
    let to = to.trim().to_string();
    if !valid_name(&to) {
        return Err(bad("rename", "invalid block name"));
    }
    let d = s.doc_mut()?;
    let key = d.blocks.keys().find(|k| k.eq_ignore_ascii_case(from)).cloned().ok_or_else(|| bad("rename", "no such block"))?;
    if key.starts_with('*') || d.blocks.get(&key).is_some_and(|b| b.anonymous) {
        return Err(bad("rename", "an anonymous block cannot be renamed"));
    }
    if !to.eq_ignore_ascii_case(&key) && d.block(&to).is_some() {
        return Err(bad("rename", format!("a block `{to}` already exists")));
    }
    let mut b = d.blocks.remove(&key).ok_or_else(|| bad("rename", "no such block"))?;
    Arc::make_mut(&mut b).name = to.clone();
    d.blocks.insert(to.clone(), b);
    let uses = |k: &EntityKind| match k {
        EntityKind::Insert(i) => i.block.eq_ignore_ascii_case(&key),
        EntityKind::MLeader(m) => m.block.as_ref().is_some_and(|b| b.block.eq_ignore_ascii_case(&key)),
        _ => false,
    };
    let fix = |k: &mut EntityKind| match k {
        EntityKind::Insert(i) => i.block = to.clone(),
        EntityKind::MLeader(m) => m.block.iter_mut().for_each(|b| b.block = to.clone()),
        _ => {}
    };
    repoint(d, &uses, &fix)?;
    if d.header.str("INSNAME", "").eq_ignore_ascii_case(&key) {
        d.header.set_str("INSNAME", &to);
    }
    for ds in &mut d.dim_styles {
        for a in [&mut ds.arrow_block, &mut ds.arrow_block1, &mut ds.arrow_block2] {
            if a.eq_ignore_ascii_case(&key) {
                *a = to.clone();
            }
        }
    }
    Ok(())
}

/// Rename a multileader style, a table style, a named view or a named UCS.
pub(super) fn simple(s: &mut Session, table: &str, from: &str, to: &str) -> Result<()> {
    let to = to.trim().to_string();
    if !valid_name(&to) {
        return Err(bad("rename", "invalid name"));
    }
    if matches!(table, "mleaderstyle" | "tablestyle") && from.eq_ignore_ascii_case("Standard") {
        return Err(bad("rename", "the Standard style cannot be renamed"));
    }
    let d = s.doc_mut()?;
    let mut names: Vec<&mut String> = match table {
        "mleaderstyle" => d.mleader_styles.iter_mut().map(|t| &mut t.name).collect(),
        "tablestyle" => d.table_styles.iter_mut().map(|t| &mut t.name).collect(),
        "view" => d.views.iter_mut().map(|t| &mut t.name).collect(),
        "ucs" => d.ucss.iter_mut().map(|t| &mut t.name).collect(),
        // A saved viewport configuration, every tile of it; never the one shown (`*Active`).
        "vport" => d.vports.iter_mut().filter(|v| !v.is_active()).map(|t| &mut t.name).collect(),
        _ => return Err(bad("rename", "unsupported table")),
    };
    if !to.eq_ignore_ascii_case(from) && names.iter().any(|n| n.eq_ignore_ascii_case(&to)) {
        return Err(bad("rename", format!("`{to}` already exists")));
    }
    let n = names.iter_mut().find(|n| n.eq_ignore_ascii_case(from)).ok_or_else(|| bad("rename", format!("no {table} `{from}`")))?;
    let old = std::mem::replace(&mut **n, to.clone());
    let current = match table {
        "mleaderstyle" => "CMLEADERSTYLE",
        "tablestyle" => "CTABLESTYLE",
        _ => return Ok(()),
    };
    if d.header.str(current, "Standard").eq_ignore_ascii_case(&old) {
        d.header.set_str(current, &to);
    }
    let uses = |k: &EntityKind| match k {
        EntityKind::MLeader(m) => table == "mleaderstyle" && m.style.eq_ignore_ascii_case(&old),
        EntityKind::Table(t) => table == "tablestyle" && t.style.eq_ignore_ascii_case(&old),
        _ => false,
    };
    let fix = |k: &mut EntityKind| match k {
        EntityKind::MLeader(m) => m.style = to.clone(),
        EntityKind::Table(t) => t.style = to.clone(),
        _ => {}
    };
    repoint(d, &uses, &fix)
}

// ---------------- -RENAME prompts ----------------

/// Prompt keyword, `rename` table (None: not available yet), noun.
const TYPES: &[(&str, Option<&str>, &str)] = &[
    ("Block", Some("block"), "block"),
    ("Dimstyle", Some("dimstyle"), "dimension style"),
    ("LAyer", Some("layer"), "layer"),
    ("LType", Some("linetype"), "linetype"),
    ("Material", None, "material"),
    ("multileadeRstyle", Some("mleaderstyle"), "multileader style"),
    ("Plotstyle", None, "plot style"),
    ("textStyle", Some("style"), "text style"),
    ("Tablestyle", Some("tablestyle"), "table style"),
    ("Ucs", Some("ucs"), "ucs"),
    ("VIew", Some("view"), "view"),
    ("VPort", Some("vport"), "viewport configuration"),
];

/// The noun in "Cannot find …" and "Invalid … name.": AutoCAD 2024 writes "UCS" there, "ucs" in
/// its prompts and in "A ucs named … already exists." (measured).
fn title(noun: &str) -> &str {
    if noun == "ucs" { "UCS" } else { noun }
}

/// Keyword-only: spaces separate inputs.
const KW: Accept = Accept { point: false, number: false, text: false, select: false, enter: true };
/// A name: Space ends the input.
const NAME: Accept = Accept { number: true, ..Accept::TEXT };

fn any_named<'a>(mut names: impl Iterator<Item = &'a String>, name: &str) -> Option<String> {
    names.find(|n| n.eq_ignore_ascii_case(name)).cloned()
}

/// The name as the drawing spells it of the `table` entry named `name` (without regard to case).
fn stored(d: &Drawing, table: &str, name: &str) -> Option<String> {
    match table {
        "block" => d.block(name).map(|t| t.name.clone()),
        "layer" => d.layer(name).map(|t| t.name.clone()),
        "linetype" => d.linetype(name).map(|t| t.name.clone()),
        "style" => d.text_style(name).map(|t| t.name.clone()),
        "dimstyle" => d.dim_style(name).map(|t| t.name.clone()),
        "mleaderstyle" => any_named(d.mleader_styles.iter().map(|t| &t.name), name),
        "tablestyle" => any_named(d.table_styles.iter().map(|t| &t.name), name),
        "view" => any_named(d.views.iter().map(|t| &t.name), name),
        "ucs" => any_named(d.ucss.iter().map(|t| &t.name), name),
        "vport" => any_named(d.vports.iter().filter(|v| !v.is_active()).map(|t| &t.name), name),
        _ => Some(name.to_string()),
    }
}

fn exists(d: &Drawing, table: &str, name: &str) -> bool {
    stored(d, table, name).is_some()
}

#[derive(Default)]
enum Stage {
    #[default]
    Type,
    Old(usize),
    New(usize, String),
}

#[derive(Default)]
pub(in crate::cmd) struct RenameM {
    stage: Stage,
}

fn text(i: &Input) -> Option<String> {
    match i {
        Input::Text(t) | Input::Keyword(t) => Some(t.trim().to_string()).filter(|t| !t.is_empty()),
        _ => None,
    }
}

impl Interactive for RenameM {
    fn name(&self) -> &'static str {
        "-RENAME"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        let noun = |t: &usize| TYPES.get(*t).map_or("object", |(_, _, n)| *n);
        match &self.stage {
            Stage::Type => {
                let kws: Vec<&str> = TYPES.iter().map(|(k, _, _)| *k).collect();
                Prompt::new("Enter object type to rename", KW).kw(&kws)
            }
            Stage::Old(t) => Prompt::new(format!("Enter old {} name", noun(t)), NAME),
            Stage::New(t, _) => Prompt::new(format!("Enter new {} name", noun(t)), NAME),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        // No new name is an invalid one (AutoCAD 2024, measured for layers, blocks, views and
        // viewport configurations).
        if i == Input::Enter
            && let Stage::New(t, _) = &self.stage
            && let Some((_, Some(_), noun)) = TYPES.get(*t)
        {
            s.echo(format!("Invalid {} name.", title(noun)));
            return Ok(Step::Done);
        }
        if matches!(i, Input::Cancel | Input::Enter) {
            return Ok(if i == Input::Cancel { Step::Cancel } else { Step::Done });
        }
        match std::mem::take(&mut self.stage) {
            Stage::Type => {
                let t = match &i {
                    Input::Keyword(k) => TYPES.iter().position(|(kw, _, _)| kw == k),
                    _ => None,
                };
                let Some(t) = t else { return Err(EngineError::Other("Invalid option keyword.".into())) };
                self.stage = Stage::Old(t);
                Ok(Step::Continue)
            }
            Stage::Old(t) => {
                let (Some(name), Some((_, table, noun))) = (text(&i), TYPES.get(t)) else {
                    self.stage = Stage::Old(t);
                    return Ok(Step::Continue);
                };
                // AutoCAD 2024 ends -RENAME on an old name it won't rename (measured): "Invalid … name."
                // for a viewport configuration or block whose name starts with "*" (*Active,
                // *Model_Space, an anonymous block's *U0) and for linetypes ByLayer and ByBlock;
                // "Cannot rename …" and "*Invalid*" for layer 0 and linetype Continuous.
                let invalid = match *table {
                    Some("vport" | "block") => name.starts_with('*'),
                    Some("linetype") => name.eq_ignore_ascii_case("bylayer") || name.eq_ignore_ascii_case("byblock"),
                    _ => false,
                };
                if invalid {
                    s.echo(format!("Invalid {} name.", title(noun)));
                    return Ok(Step::Done);
                }
                let fixed = match *table {
                    Some("layer") => name == "0",
                    Some("linetype") => name.eq_ignore_ascii_case("continuous"),
                    _ => false,
                };
                if fixed {
                    s.echo(format!("Cannot rename {} \"{name}\".", title(noun)));
                    s.echo("*Invalid*");
                    return Ok(Step::Done);
                }
                // The old name as the drawing spells it: a new name in another case than that is a
                // rename, even when it is the old name as typed (AutoCAD 2024, measured).
                let name = match table {
                    Some(table) => match stored(s.doc()?, table, &name) {
                        Some(n) => n,
                        None => {
                            s.echo(format!("Cannot find {} \"{name}\".", title(noun)));
                            return Ok(Step::Done);
                        }
                    },
                    None => name,
                };
                self.stage = Stage::New(t, name);
                Ok(Step::Continue)
            }
            Stage::New(t, old) => {
                let (Some(name), Some((_, table, noun))) = (text(&i), TYPES.get(t)) else {
                    self.stage = Stage::New(t, old);
                    return Ok(Step::Continue);
                };
                let Some(table) = table else {
                    s.echo(format!("Renaming a {noun} is not available yet."));
                    return Ok(Step::Done);
                };
                // AutoCAD 2024's messages, each ending the command (measured).
                if !valid_name(&name) {
                    s.echo(format!("Invalid {} name.", title(noun)));
                    return Ok(Step::Done);
                }
                if !name.eq_ignore_ascii_case(&old) && exists(s.doc()?, table, &name) {
                    s.echo(format!("A {noun} named \"{name}\" already exists."));
                    s.echo("*Invalid*");
                    return Ok(Step::Done);
                }
                if name == old {
                    return Ok(Step::Done);
                }
                match super::run_rename(s, &json!({ "table": table, "from": old, "to": name })) {
                    Ok(_) => Ok(Step::Done),
                    Err(e) => {
                        self.stage = Stage::New(t, old);
                        Err(EngineError::Other(match e {
                            EngineError::BadParams { msg, .. } => msg,
                            other => other.to_string(),
                        }))
                    }
                }
            }
        }
    }
}
