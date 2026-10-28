//! ATTDISP (attribute display: ATTMODE) and ATTSYNC (block references brought in line with
//! their block's attribute definitions).

use std::sync::Arc;

use cadcraft_doc::{Attrib, Block, Entity, EntityKind, Handle, Insert};
use serde_json::{Value, json};

use crate::cmd::curves::KW;
use crate::cmd::machines::{SelOutcome, SelectPhase};
use crate::cmd::qselect::wildcard;
use crate::cmd::{CommandSpec, bad, str_param, targets};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub(super) fn attdisp_spec() -> CommandSpec {
    CommandSpec::new("attdisp", "Attribute Display", run_attdisp)
        .params("{mode: \"normal\" | \"on\" | \"off\"} (sets ATTMODE to 1 / 2 / 0) → {attmode}")
        .interactive(|_| Ok(Box::new(AttdispM)))
        .transparent()
}

/// Returns the View > Attribute Display > Normal, On and Off commands, which set ATTMODE as the matching ATTDISP option does.
pub(super) fn attdisp_menu_specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("attdisp.normal", "Normal", |s, _| set_attmode(s, 1))
            .menu(&["View", "Attribute Display", "Normal"])
            .params("{} (ATTDISP Normal: ATTMODE 1) → {attmode}"),
        CommandSpec::new("attdisp.on", "On", |s, _| set_attmode(s, 2))
            .menu(&["View", "Attribute Display", "On"])
            .params("{} (ATTDISP ON: ATTMODE 2) → {attmode}"),
        CommandSpec::new("attdisp.off", "Off", |s, _| set_attmode(s, 0))
            .menu(&["View", "Attribute Display", "Off"])
            .params("{} (ATTDISP OFF: ATTMODE 0) → {attmode}"),
    ]
}

pub(super) fn attsync_spec() -> CommandSpec {
    CommandSpec::new("attsync", "Synchronize Attributes", run_attsync)
        .menu(&["Modify", "Object", "Attribute", "Synchronize Attributes"])
        .params("{name} or {handle} (a reference of the block) → {block, references}")
        .interactive(|_| Ok(Box::new(AttsyncM::default())))
}

// ---------------- ATTDISP ----------------

/// ATTDISP's keyword for an ATTMODE value.
fn attdisp_keyword(mode: i64) -> &'static str {
    match mode {
        0 => "OFF",
        2 => "ON",
        _ => "Normal",
    }
}

/// ATTMODE for an ATTDISP keyword: Normal shows the visible attributes, ON all, OFF none.
fn attmode(keyword: &str) -> Option<i64> {
    match keyword.trim().to_ascii_lowercase().as_str() {
        "normal" | "n" => Some(1),
        "on" => Some(2),
        "off" => Some(0),
        _ => None,
    }
}

fn set_attmode(s: &mut Session, mode: i64) -> Result<Value> {
    s.doc_mut()?.header.set_i64("ATTMODE", mode);
    Ok(json!({ "attmode": mode }))
}

fn run_attdisp(s: &mut Session, p: &Value) -> Result<Value> {
    let mode = str_param(p, "mode").and_then(attmode).ok_or_else(|| bad("attdisp", "`mode` (\"normal\", \"on\" or \"off\") is required"))?;
    set_attmode(s, mode)
}

struct AttdispM;

impl Interactive for AttdispM {
    fn name(&self) -> &'static str {
        "ATTDISP"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let cur = s.doc().map(|d| d.header.i64("ATTMODE", 1)).unwrap_or(1);
        Prompt::new("Enter attribute visibility setting", KW).kw(&["Normal", "ON", "OFF"]).default(attdisp_keyword(cur))
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Keyword(k) | Input::Text(k) => {
                let mode = attmode(&k).ok_or_else(|| EngineError::Other("Invalid option keyword.".into()))?;
                set_attmode(s, mode)?;
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

// ---------------- ATTSYNC ----------------

/// The attributes `ins` gets from the definitions `defs` of its block `blk`: placed, styled and
/// shown as defined, keeping the values of attributes whose tag (any case) is still defined.
/// Properties set on single attributes (layer, colour, …) go back to the definitions', as in AutoCAD.
pub(super) fn synced(blk: &Block, defs: &[&Attrib], ins: &Insert) -> Vec<Attrib> {
    let m = ins.transform(blk.base.xy());
    defs.iter()
        .map(|ad| {
            let mut k = EntityKind::Text(ad.text.clone());
            k.transform(&m);
            let mut t = if let EntityKind::Text(t) = k { t } else { ad.text.clone() };
            t.value = ins
                .attribs
                .iter()
                .find(|a| a.tag.eq_ignore_ascii_case(&ad.tag))
                .map(|a| a.text.value.clone())
                .unwrap_or_else(|| ad.text.value.clone());
            Attrib {
                tag: ad.tag.clone(),
                text: t,
                invisible: ad.invisible,
                constant: false,
                prompt: String::new(),
                props: Default::default(),
                multiline: ad.multiline.as_ref().map(|ml| ml.transformed(&m)),
                modes: ad.modes.for_reference(),
            }
        })
        .collect()
}

/// The references of `name` among `ents`.
fn references<'a>(ents: impl Iterator<Item = &'a Arc<Entity>>, name: &str) -> Vec<Handle> {
    ents.filter(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(name))).map(|e| e.handle).collect()
}

/// Apply `f` to every reference of block `name` (in model space, layouts and other blocks);
/// returns how many references there were.
fn update_references(d: &mut cadcraft_doc::Drawing, name: &str, mut f: impl FnMut(&mut Insert)) -> usize {
    update_reference_entities(d, name, |e| {
        if let EntityKind::Insert(ins) = &mut e.kind {
            f(ins);
        }
    })
}

/// [`update_references`], with `f` given the whole object.
pub(super) fn update_reference_entities(d: &mut cadcraft_doc::Drawing, name: &str, mut update: impl FnMut(&mut Entity)) -> usize {
    let mut n = 0;
    let spaces: Vec<cadcraft_doc::Space> =
        std::iter::once(cadcraft_doc::Space::Model).chain(d.layouts.iter().map(|l| cadcraft_doc::Space::Paper(l.name.clone()))).collect();
    for sp in spaces {
        let Some(store) = d.space_mut(&sp) else { continue };
        let hs = references(store.iter(), name);
        for h in hs {
            n += usize::from(store.modify(h, &mut update));
        }
    }
    for (bname, b) in d.blocks.iter_mut() {
        if bname.eq_ignore_ascii_case(name) {
            continue;
        }
        let hs = references(b.entities.iter(), name);
        if !hs.is_empty() {
            let b = Arc::make_mut(b);
            for h in hs {
                n += usize::from(b.entities.modify(h, &mut update));
            }
        }
    }
    n
}

/// How many references block `name` has (in model space, layouts and other blocks).
pub(super) fn reference_count(d: &cadcraft_doc::Drawing, name: &str) -> usize {
    let spaces = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities));
    spaces
        .chain(d.blocks.iter().filter(|(k, _)| !k.eq_ignore_ascii_case(name)).map(|(_, b)| &b.entities))
        .map(|st| references(st.iter(), name).len())
        .sum()
}

/// Rename the attributes tagged `old` (any case) to `new` on every reference of block `name`, so
/// their values survive a tag change of the definition. Returns how many references there were.
pub(super) fn rename_tag(s: &mut Session, name: &str, old: &str, new: &str) -> Result<usize> {
    let d = s.doc_mut()?;
    Ok(update_references(d, name, |ins| {
        for a in ins.attribs.iter_mut().filter(|a| a.tag.eq_ignore_ascii_case(old)) {
            a.tag = new.to_string();
        }
    }))
}

/// The attribute definitions of `blk` that references carry (constant ones are not).
pub(super) fn attdefs(blk: &Block) -> Vec<&Attrib> {
    blk.entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::AttDef(a) if !a.constant => Some(a),
            _ => None,
        })
        .collect()
}

/// Synchronize every reference of block `name` (in model space, layouts and other blocks).
/// Returns the block's name as defined and how many references were updated.
pub(super) fn sync(s: &mut Session, name: &str) -> Result<(String, usize)> {
    let blk = s.doc()?.block(name).cloned().ok_or_else(|| EngineError::Other(format!("Block \"{name}\" not found.")))?;
    let defs = attdefs(&blk);
    let d = s.doc_mut()?;
    let n = update_references(d, &blk.name, |ins| ins.attribs = synced(&blk, &defs, ins));
    Ok((blk.name.clone(), n))
}

/// Whether block `name` has attribute definitions.
pub(super) fn has_attdefs(s: &Session, name: &str) -> bool {
    s.doc().ok().and_then(|d| d.block(name)).is_some_and(|b| b.entities.iter().any(|e| matches!(&e.kind, EntityKind::AttDef(a) if !a.constant)))
}

fn run_attsync(s: &mut Session, p: &Value) -> Result<Value> {
    let name = match str_param(p, "name") {
        Some(n) => n.to_string(),
        None => {
            let h = targets(s, p)?.first().copied().ok_or_else(|| bad("attsync", "`name` (or `handle` of a block reference) is required"))?;
            match s.doc()?.entity(h).map(|e| &e.kind) {
                Some(EntityKind::Insert(i)) => i.block.clone(),
                _ => return Err(bad("attsync", "`handle` is not a block reference")),
            }
        }
    };
    let (block, n) = sync(s, &name)?;
    Ok(json!({ "block": block, "references": n }))
}

/// Names of the blocks with attribute definitions that match `spec` (comma-separated wildcards).
pub(super) fn blocks_with_attdefs(s: &Session, spec: &str) -> Vec<String> {
    let Ok(d) = s.doc() else { return Vec::new() };
    d.blocks
        .values()
        .filter(|b| !b.anonymous && spec.split(',').any(|p| wildcard(p.trim(), &b.name)) && has_attdefs(s, &b.name))
        .map(|b| b.name.clone())
        .collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Stage {
    #[default]
    Option,
    List,
    Name,
    Select,
    Confirm,
}

#[derive(Default)]
struct AttsyncM {
    stage: Stage,
    /// The stage to return to after `?`.
    back: Stage,
    sel: SelectPhase,
    /// The block picked by Select, waiting for confirmation.
    picked: String,
}

impl AttsyncM {
    /// Sync `name` (when it has attributes) and end; otherwise say why and keep prompting.
    fn finish(&mut self, s: &mut Session, name: &str) -> Result<Step> {
        if s.doc()?.block(name).is_none() {
            return Err(EngineError::Other(format!("Block \"{name}\" not found.")));
        }
        if !has_attdefs(s, name) {
            return Err(EngineError::Other(format!("Block \"{name}\" has no attribute definitions.")));
        }
        let (block, n) = sync(s, name)?;
        s.echo(format!("ATTSYNC block {block}: {n} reference{} synchronized.", if n == 1 { "" } else { "s" }));
        Ok(Step::Done)
    }
}

impl Interactive for AttsyncM {
    fn name(&self) -> &'static str {
        "ATTSYNC"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            Stage::Option => Prompt::new("Enter an option", KW).kw(&["?", "Name", "Select"]).default("Select"),
            Stage::List => Prompt::new("Enter block(s) to list", Accept::TEXT).default("*"),
            Stage::Name => Prompt::new("Enter name of block to sync", Accept::TEXT).kw(&["?"]),
            Stage::Select => Prompt::new("Select a block", Accept::SELECT),
            Stage::Confirm => Prompt::new(format!("ATTSYNC block {}?", self.picked), KW).kw(&["Yes", "No"]).default("Yes"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.stage {
            Stage::Option => match &i {
                Input::Keyword(k) if k == "?" => {
                    self.back = Stage::Option;
                    self.stage = Stage::List;
                }
                Input::Keyword(k) if k == "Name" => self.stage = Stage::Name,
                Input::Keyword(_) | Input::Enter => {
                    self.sel = SelectPhase::single();
                    self.stage = Stage::Select;
                }
                _ => return Err(EngineError::Other("Invalid option keyword.".into())),
            },
            Stage::List => {
                let spec = match &i {
                    Input::Text(t) if !t.trim().is_empty() => t.trim().to_string(),
                    Input::Text(_) | Input::Enter => "*".to_string(),
                    _ => return Ok(Step::Continue),
                };
                let names = blocks_with_attdefs(s, &spec);
                s.echo(if names.is_empty() {
                    "No blocks with attributes match.".to_string()
                } else {
                    format!("Blocks with attributes: {}", names.join(", "))
                });
                self.stage = self.back;
            }
            Stage::Name => match &i {
                Input::Keyword(k) if k == "?" => {
                    self.back = Stage::Name;
                    self.stage = Stage::List;
                }
                Input::Text(t) if !t.trim().is_empty() => return self.finish(s, t.trim()),
                Input::Enter => return Ok(Step::Done),
                _ => {}
            },
            Stage::Select => match self.sel.feed(s, &i)? {
                SelOutcome::More => {}
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    s.set_selection(Vec::new());
                    let block = hs.iter().find_map(|h| match s.doc().ok()?.entity(*h).map(|e| &e.kind) {
                        Some(EntityKind::Insert(ins)) => Some(ins.block.clone()),
                        _ => None,
                    });
                    self.sel = SelectPhase::single();
                    match block {
                        Some(b) if has_attdefs(s, &b) => {
                            self.picked = b;
                            self.stage = Stage::Confirm;
                        }
                        Some(b) => return Err(EngineError::Other(format!("Block \"{b}\" has no attribute definitions."))),
                        None => return Err(EngineError::Other("That object is not a block reference.".into())),
                    }
                }
            },
            Stage::Confirm => match &i {
                Input::Keyword(k) if k == "No" => return Ok(Step::Done),
                Input::Keyword(_) | Input::Enter => {
                    let name = self.picked.clone();
                    return self.finish(s, &name);
                }
                _ => return Err(EngineError::Other("Invalid option keyword.".into())),
            },
        }
        Ok(Step::Continue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attdisp_keywords_map_to_attmode() {
        assert_eq!([attmode("Normal"), attmode("on"), attmode("OFF"), attmode("x")], [Some(1), Some(2), Some(0), None]);
        assert_eq!([attdisp_keyword(0), attdisp_keyword(1), attdisp_keyword(2)], ["OFF", "Normal", "ON"]);
    }
}
