//! BLOCKREPLACE and -BLOCKREPLACE (Express Tools): every reference of one block, in model space,
//! the layouts and other blocks, becomes a reference of another block at the same insertion
//! point, scale and rotation. The unreferenced block definition can then be purged.

use cadcraft_doc::{Drawing, Entity, EntityKind};
use serde_json::{Value, json};

use super::attribs::{attdefs, synced, update_reference_entities};
use super::purge::{override_arrows, used_blocks};
use super::references_block;
use crate::cmd::curves::KW;
use crate::cmd::machines::{SelOutcome, SelectPhase};
use crate::cmd::{CommandSpec, bad, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub(super) fn blockreplace_spec() -> CommandSpec {
    CommandSpec::new("blockreplace", "Replace Block...", run_blockreplace)
        .menu(&["Tools", "Express Tools", "Replace Block..."])
        .alias(&["-blockreplace"])
        .params("{block (the block whose references are replaced), with (the replacement block), purge?: bool (default false: remove the replaced block's definition when nothing references it any more)} → {block, with, replaced, purged}")
        .interactive(|_| Ok(Box::new(BlockreplaceM::default())))
}

/// The defined name of block `name` (names are case-insensitive), when it is a named block of
/// this drawing: not anonymous and not one an xref brings in (`X|A`), which is never saved.
fn named_block(d: &Drawing, name: &str) -> Option<String> {
    d.block(name.trim()).filter(|b| !b.anonymous && !b.name.starts_with('*') && !b.name.contains('|')).map(|b| b.name.clone())
}

/// Why the references of block `old` cannot be replaced: a dynamic block's references keep their
/// property values in data that names the block.
fn not_replaceable(d: &Drawing, old: &str) -> Option<String> {
    cadcraft_doc::dynblock::dynamic_block(d, old).map(|_| format!("Block \"{old}\" is a dynamic block; its references cannot be replaced."))
}

/// Why block `new` cannot replace block `old`: references of `old` inside `new` would make `new`
/// reference itself.
fn cannot_replace_with(d: &Drawing, old: &str, new: &str) -> Option<String> {
    let blk = d.block(new)?;
    references_block(d, &blk.entities.iter().map(|e| (**e).clone()).collect::<Vec<_>>(), old)
        .then(|| format!("Block \"{new}\" references \"{old}\", so it cannot replace it."))
}

/// Replace every reference of `old` by a reference of `new`; with `purge`, remove `old`'s
/// definition when nothing references it afterwards. Returns (references replaced, purged).
fn replace(s: &mut Session, old: &str, new: &str, purge: bool) -> Result<(usize, bool)> {
    let cmd = "blockreplace";
    if s.state()?.block_edit.is_some() {
        return Err(bad(cmd, "the Block Editor is open; close it with BCLOSE first"));
    }
    let d = s.doc()?;
    if d.refedit.is_some() {
        return Err(bad(cmd, "a block is being edited in place; finish with REFCLOSE first"));
    }
    if let Some(n) = [old, new].into_iter().find(|n| n.contains('|')) {
        return Err(bad(cmd, format!("Block \"{}\" comes from an xref; bind the xref first.", n.trim())));
    }
    let old = named_block(d, old).ok_or_else(|| bad(cmd, format!("Block \"{}\" not found.", old.trim())))?;
    let new = named_block(d, new).ok_or_else(|| bad(cmd, format!("Block \"{}\" not found.", new.trim())))?;
    if old.eq_ignore_ascii_case(&new) {
        return Err(bad(cmd, "the replacement block must be a different block"));
    }
    if let Some(why) = not_replaceable(d, &old).or_else(|| cannot_replace_with(d, &old, &new)) {
        return Err(bad(cmd, why));
    }
    let blk = d.block(&new).cloned().ok_or_else(|| bad(cmd, format!("Block \"{new}\" not found.")))?;
    let defs = attdefs(&blk);
    let scale = d.current_anno_scale();
    let d = s.doc_mut()?;
    let n = update_reference_entities(d, &old, |e| {
        if let EntityKind::Insert(ins) = &mut e.kind {
            ins.block.clone_from(&blk.name);
            ins.attribs = synced(&blk, &defs, ins);
        }
        // A reference of an annotative block is annotative, as INSERT makes it.
        if !blk.annotative {
            e.common.anno_scales.clear();
        } else if e.common.anno_scales.is_empty() {
            e.common.anno_scales = vec![scale.clone()];
        }
    });
    let key = d.blocks.keys().find(|k| k.eq_ignore_ascii_case(&old)).cloned();
    let purged = purge && !referenced(d, &old) && key.is_some_and(|k| d.blocks.remove(&k).is_some());
    Ok((n, purged))
}

/// Whether anything still names block `name`: what PURGE counts as in use, and the dimensions,
/// multileaders and block references of every other block definition, used or not.
fn referenced(d: &Drawing, name: &str) -> bool {
    if used_blocks(d).contains(&name.to_ascii_uppercase()) {
        return true;
    }
    let names = |e: &Entity| -> Vec<String> {
        match &e.kind {
            EntityKind::Insert(i) => vec![i.block.clone()],
            EntityKind::Dimension(dm) => dm.block.iter().cloned().chain(override_arrows(dm)).collect(),
            EntityKind::MLeader(m) => m.block.iter().map(|b| b.block.clone()).collect(),
            _ => Vec::new(),
        }
    };
    d.blocks
        .values()
        .filter(|b| !b.name.eq_ignore_ascii_case(name))
        .flat_map(|b| b.entities.iter())
        .any(|e| names(e).iter().any(|n| n.trim().eq_ignore_ascii_case(name)))
}

fn run_blockreplace(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "blockreplace";
    let old = str_param(p, "block").ok_or_else(|| bad(cmd, "`block` (the block to replace) is required"))?;
    let new = str_param(p, "with").ok_or_else(|| bad(cmd, "`with` (the replacement block) is required"))?;
    let purge = match p.get("purge") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(v) => return Err(bad(cmd, format!("`purge` must be true or false, not {v}"))),
    };
    let (old, new) = (old.to_string(), new.to_string());
    let (n, purged) = replace(s, &old, &new, purge)?;
    let d = s.doc()?;
    let defined = |name: &str| named_block(d, name).unwrap_or_else(|| name.trim().to_string());
    Ok(json!({ "block": defined(&old), "with": defined(&new), "replaced": n, "purged": purged }))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Stage {
    #[default]
    Old,
    New,
    Select,
    Purge,
}

#[derive(Default)]
struct BlockreplaceM {
    stage: Stage,
    /// The stage a picked reference answers.
    back: Stage,
    sel: SelectPhase,
    old: String,
    new: String,
}

impl BlockreplaceM {
    /// Take `name` as the answer to the current name prompt. A block that cannot take part is
    /// refused here, so the prompt asks again.
    fn take(&mut self, s: &Session, stage: Stage, name: &str) -> Result<()> {
        let d = s.doc()?;
        let name = named_block(d, name).ok_or_else(|| EngineError::Other(format!("Block \"{}\" not found.", name.trim())))?;
        if stage == Stage::Old {
            if let Some(why) = not_replaceable(d, &name) {
                return Err(EngineError::Other(why));
            }
            self.old = name;
            self.stage = Stage::New;
        } else {
            if name.eq_ignore_ascii_case(&self.old) {
                return Err(EngineError::Other("The replacement block must be a different block.".into()));
            }
            if let Some(why) = cannot_replace_with(d, &self.old, &name) {
                return Err(EngineError::Other(why));
            }
            self.new = name;
            self.stage = Stage::Purge;
        }
        Ok(())
    }

    fn finish(&mut self, s: &mut Session, purge: bool) -> Result<Step> {
        let (n, purged) = replace(s, &self.old, &self.new, purge)?;
        s.echo(format!(
            "{n} reference{} of block {} replaced with {}.{}",
            if n == 1 { "" } else { "s" },
            self.old,
            self.new,
            if purged { format!(" Block {} purged.", self.old) } else { String::new() }
        ));
        Ok(Step::Done)
    }
}

impl Interactive for BlockreplaceM {
    fn name(&self) -> &'static str {
        "BLOCKREPLACE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            Stage::Old => Prompt::new("Enter the name of the block to replace", Accept::TEXT).kw(&["?", "Select"]),
            Stage::New => Prompt::new(format!("Enter the name of the replacement block for {}", self.old), Accept::TEXT).kw(&["?", "Select"]),
            Stage::Select => Prompt::new("Select a block reference", Accept::SELECT),
            Stage::Purge => Prompt::new(format!("Purge the unreferenced block definition {}?", self.old), KW).kw(&["Yes", "No"]).default("Yes"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.stage {
            stage @ (Stage::Old | Stage::New) => match &i {
                Input::Keyword(k) if k == "?" => {
                    let d = s.doc()?;
                    let mut names: Vec<String> = d.blocks.keys().filter_map(|k| named_block(d, k)).collect();
                    names.sort_by_key(|n| n.to_ascii_lowercase());
                    s.echo(if names.is_empty() { "No blocks defined.".to_string() } else { format!("Blocks: {}", names.join(", ")) });
                }
                Input::Keyword(k) if k == "Select" => {
                    self.back = stage;
                    self.sel = SelectPhase::single();
                    self.stage = Stage::Select;
                }
                Input::Text(t) if !t.trim().is_empty() => self.take(s, stage, t)?,
                Input::Enter | Input::Cancel => return Ok(Step::Done),
                _ => {}
            },
            Stage::Select => match self.sel.feed(s, &i)? {
                SelOutcome::More => {}
                SelOutcome::Empty => self.stage = self.back,
                SelOutcome::Done(hs) => {
                    s.set_selection(Vec::new());
                    self.sel = SelectPhase::single();
                    let block = hs.iter().find_map(|h| match s.doc().ok()?.entity(*h).map(|e| &e.kind) {
                        Some(EntityKind::Insert(ins)) => Some(ins.block.clone()),
                        _ => None,
                    });
                    match block {
                        Some(b) => self.take(s, self.back, &b)?,
                        None => return Err(EngineError::Other("That object is not a block reference.".into())),
                    }
                }
            },
            Stage::Purge => match &i {
                Input::Keyword(k) if k == "No" => return self.finish(s, false),
                Input::Keyword(_) | Input::Enter => return self.finish(s, true),
                Input::Cancel => return Ok(Step::Done),
                _ => return Err(EngineError::Other("Invalid option keyword.".into())),
            },
        }
        Ok(Step::Continue)
    }
}
