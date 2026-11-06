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

/// Whether `n` names no plot style of its own: ByLayer, ByBlock or nothing.
fn logical_plot_style(n: &str) -> bool {
    let n = n.trim();
    n.is_empty() || n.eq_ignore_ascii_case("ByLayer") || n.eq_ignore_ascii_case("ByBlock")
}

/// Whether the current plot style (CPLOTSTYLE) or a layer or object of the drawing (model space,
/// layouts, block definitions) is the named plot style `name`.
pub(super) fn plot_style_used(d: &Drawing, name: &str) -> bool {
    let name = name.trim();
    !logical_plot_style(name)
        && (d.header.str("CPLOTSTYLE", "ByLayer").trim().eq_ignore_ascii_case(name)
            || d.layers.iter().any(|l| l.plot_style.trim().eq_ignore_ascii_case(name))
            || d.model
                .iter()
                .chain(d.layouts.iter().flat_map(|l| l.entities.iter()))
                .chain(d.blocks.values().flat_map(|b| b.entities.iter()))
                .any(|e| e.common.plot_style.trim().eq_ignore_ascii_case(name)))
}

/// Rename a named plot style the drawing uses: the layers and objects that use it follow. Normal
/// (every named table has it), ByLayer and ByBlock stay.
pub(super) fn plot_style(s: &mut Session, from: &str, to: &str) -> Result<()> {
    let (from, to) = (from.trim().to_string(), to.trim().to_string());
    if !valid_name(&to) || logical_plot_style(&to) {
        return Err(bad("rename", "invalid plot style name"));
    }
    if logical_plot_style(&from) || from.eq_ignore_ascii_case(cadcraft_doc::NORMAL_STYLE) {
        return Err(bad("rename", format!("the {from} plot style cannot be renamed")));
    }
    let d = s.doc_mut()?;
    if !plot_style_used(d, &from) {
        return Err(bad("rename", format!("no plot style `{from}` is used in the drawing")));
    }
    if !to.eq_ignore_ascii_case(&from) && (to.eq_ignore_ascii_case(cadcraft_doc::NORMAL_STYLE) || plot_style_used(d, &to)) {
        return Err(bad("rename", format!("a plot style `{to}` already exists")));
    }
    let is_old = |n: &str| n.trim().eq_ignore_ascii_case(&from);
    if is_old(&d.header.str("CPLOTSTYLE", "ByLayer")) {
        d.header.set_str("CPLOTSTYLE", &to);
    }
    for l in &mut d.layers {
        if is_old(&l.plot_style) {
            l.plot_style = to.clone();
        }
    }
    let users = |st: &EntityStore| -> Vec<Handle> { st.iter().filter(|e| is_old(&e.common.plot_style)).map(|e| e.handle).collect() };
    let mut hs = users(&d.model);
    hs.extend(d.layouts.iter().flat_map(|l| users(&l.entities)));
    for h in hs {
        d.modify_entity(h, |e| e.common.plot_style = to.clone())?;
    }
    for block in d.blocks.values_mut() {
        let hs = users(&block.entities);
        if !hs.is_empty() {
            let b = Arc::make_mut(block);
            for h in hs {
                b.entities.modify(h, |e| e.common.plot_style = to.clone());
            }
        }
    }
    Ok(())
}

/// Rename a named model space viewport configuration (VPORTS Save). The one shown, `*Active`,
/// has no name to change.
pub(super) fn vport(s: &mut Session, from: &str, to: &str) -> Result<()> {
    let to = to.trim().to_string();
    if !valid_name(&to) {
        return Err(bad("rename", "invalid viewport configuration name"));
    }
    let d = s.doc_mut()?;
    let named = |c: &cadcraft_doc::VportConfig, n: &str| !c.is_active() && c.name.eq_ignore_ascii_case(n.trim());
    if !to.eq_ignore_ascii_case(from.trim()) && d.vports.iter().any(|c| named(c, &to)) {
        return Err(bad("rename", format!("a viewport configuration `{to}` already exists")));
    }
    let c = d.vports.iter_mut().find(|c| named(c, from)).ok_or_else(|| bad("rename", format!("no viewport configuration `{}`", from.trim())))?;
    c.name = to;
    Ok(())
}

// ---------------- -RENAME prompts ----------------

/// Prompt keyword, `rename` table (None: not available: CADCraft keeps no materials, which are
/// for 3D rendering), noun.
const TYPES: &[(&str, Option<&str>, &str)] = &[
    ("Block", Some("block"), "block"),
    ("Dimstyle", Some("dimstyle"), "dimension style"),
    ("LAyer", Some("layer"), "layer"),
    ("LType", Some("linetype"), "linetype"),
    ("Material", None, "material"),
    ("multileadeRstyle", Some("mleaderstyle"), "multileader style"),
    ("Plotstyle", Some("plotstyle"), "plot style"),
    ("textStyle", Some("style"), "text style"),
    ("Tablestyle", Some("tablestyle"), "table style"),
    ("Ucs", Some("ucs"), "UCS"),
    ("VIew", Some("view"), "view"),
    ("VPort", Some("vport"), "viewport configuration"),
];

/// Keyword-only: spaces separate inputs.
const KW: Accept = Accept { point: false, number: false, text: false, select: false, enter: true };
/// A name: Space ends the input.
const NAME: Accept = Accept { number: true, ..Accept::TEXT };

fn any_named<'a>(mut names: impl Iterator<Item = &'a String>, name: &str) -> bool {
    names.any(|n| n.eq_ignore_ascii_case(name))
}

fn exists(d: &Drawing, table: &str, name: &str) -> bool {
    match table {
        "block" => d.block(name).is_some(),
        "layer" => d.layer(name).is_some(),
        "linetype" => d.linetype(name).is_some(),
        "style" => d.text_style(name).is_some(),
        "dimstyle" => d.dim_style(name).is_some(),
        "mleaderstyle" => any_named(d.mleader_styles.iter().map(|t| &t.name), name),
        "tablestyle" => any_named(d.table_styles.iter().map(|t| &t.name), name),
        "view" => any_named(d.views.iter().map(|t| &t.name), name),
        "ucs" => any_named(d.ucss.iter().map(|t| &t.name), name),
        "plotstyle" => plot_style_used(d, name),
        "vport" => d.vports.iter().any(|c| !c.is_active() && c.name.eq_ignore_ascii_case(name)),
        _ => true,
    }
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
                if let Some(table) = table
                    && !exists(s.doc()?, table, &name)
                {
                    self.stage = Stage::Old(t);
                    return Err(EngineError::Other(format!("No {noun} named \"{name}\".")));
                }
                self.stage = Stage::New(t, name);
                Ok(Step::Continue)
            }
            Stage::New(t, old) => {
                let (Some(name), Some((_, table, noun))) = (text(&i), TYPES.get(t)) else {
                    self.stage = Stage::New(t, old);
                    return Ok(Step::Continue);
                };
                let Some(table) = table else {
                    s.echo(format!("Renaming a {noun} is not available: CADCraft keeps no {noun}s."));
                    return Ok(Step::Done);
                };
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
