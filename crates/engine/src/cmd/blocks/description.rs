//! Block Description (Modify > Object > Block Description...): changes the description of a
//! block definition. In the Block Editor it changes the edited block's description, which BSAVE
//! saves into the drawing as it saves the block's objects.

use std::sync::Arc;

use cadcraft_doc::Block;
use serde_json::{Value, json};

use super::bedit::selected_reference;
use crate::cmd::{CommandSpec, bad, has_doc, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// The longest description a block takes, in characters.
const MAX_LEN: usize = 255;

pub(super) fn description_spec() -> CommandSpec {
    CommandSpec::new("blocks.description", "Block Description...", run)
        .menu(&["Modify", "Object", "Block Description..."])
        .params("{name? (default the block the Block Editor is editing, else the one selected block reference's block), description (up to 255 characters, one line; \"\" clears it)} → {block, description}")
        .enabled(has_doc)
        .interactive(|_| Ok(Box::new(DescriptionM::default())))
}

/// The block the Block Editor is editing, when it is open on a block (not on the drawing itself).
fn edited_block(s: &Session) -> Option<String> {
    s.state().ok()?.block_editor().filter(|n| !n.eq_ignore_ascii_case(crate::blockedit::CURRENT_DRAWING)).map(str::to_string)
}

/// The block that a call without a name is about: the edited block, else the selected reference's.
fn default_block(s: &Session) -> Option<String> {
    edited_block(s).or_else(|| selected_reference(s))
}

/// Block `name` as the drawing spells it, when its description can be changed: while the Block
/// Editor edits a block, only that block's. The block the editor has started and not saved yet has
/// no definition, and is named as the editor names it.
fn check_block(s: &Session, name: &str) -> Result<String> {
    let name = name.trim();
    if let Some(edited) = edited_block(s)
        && !edited.eq_ignore_ascii_case(name)
    {
        return Err(bad(
            "blocks.description",
            format!("The Block Editor is editing \"{edited}\"; close it (BCLOSE) to change another block's description."),
        ));
    }
    let d = s.doc()?;
    match d.block(name) {
        Some(b) if b.xref_path.is_some() => {
            Err(bad("blocks.description", format!("\"{}\" is an external reference; its description comes from its drawing.", b.name)))
        }
        Some(b) if b.anonymous || b.name.starts_with('*') => {
            Err(bad("blocks.description", format!("\"{}\" is an anonymous block made by another object; it has no description to change.", b.name)))
        }
        Some(b) => Ok(b.name.clone()),
        None => match edited_block(s) {
            Some(e) if e.eq_ignore_ascii_case(name) => Ok(e),
            _ => Err(bad("blocks.description", format!("Block \"{name}\" not found."))),
        },
    }
}

/// A description a block can take (command `cmd`'s): one line of up to [`MAX_LEN`] characters.
pub(super) fn check_text(cmd: &str, text: &str) -> Result<()> {
    if text.contains(['\n', '\r']) {
        return Err(bad(cmd, "the description must be one line"));
    }
    if text.chars().count() > MAX_LEN {
        return Err(bad(cmd, format!("the description can be up to {MAX_LEN} characters")));
    }
    Ok(())
}

/// Block `name`'s definition: the drawing's, else, for the block the Block Editor edits, the one
/// outside the editor (a block saved under a new name with BSAVEAS is not in the editor's tables).
fn definition(s: &Session, name: &str) -> Option<Arc<Block>> {
    let st = s.state().ok()?;
    st.doc.block(name).or_else(|| st.block_edit.as_ref().and_then(|be| be.host.doc.block(name))).cloned()
}

/// The description of block `name` (as [`check_block`] gave it), or "" for one not yet saved.
fn current(s: &Session, name: &str) -> String {
    definition(s, name).map(|b| b.description.clone()).unwrap_or_default()
}

/// Give block `name` (as [`check_block`] gave it) the description `text`.
fn set(s: &mut Session, name: &str, text: &str) -> Result<Value> {
    check_text("blocks.description", text)?;
    let outside = definition(s, name);
    let doc = s.doc_mut()?;
    match doc.blocks.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
        Some((_, b)) => {
            if b.description != text {
                Arc::make_mut(b).description = text.to_string();
            }
        }
        // The block the Block Editor edits and the editor's tables don't hold (one it started, or
        // one BSAVEAS made): BSAVE saves it with this description.
        None => {
            let mut b = outside.map(|b| (*b).clone()).unwrap_or_else(|| Block::new(name));
            b.description = text.to_string();
            doc.blocks.insert(name.to_string(), Arc::new(b));
        }
    }
    if s.state()?.block_editor().is_some() {
        s.echo(format!("Description of block \"{name}\" changed; BSAVE saves it into the drawing."));
    } else {
        s.echo(format!("Description of block \"{name}\" changed."));
    }
    Ok(json!({ "block": name, "description": text }))
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let name = match str_param(p, "name") {
        Some(n) => n.to_string(),
        None => default_block(s)
            .ok_or_else(|| bad("blocks.description", "`name` is required (or open the block in the Block Editor, or select one block reference)"))?,
    };
    let text = str_param(p, "description").ok_or_else(|| bad("blocks.description", "`description` is required"))?;
    let name = check_block(s, &name)?;
    set(s, &name, text)
}

// ---------------- interactive ----------------

/// Block Description: the block (the edited one, the selected reference's, or a name typed or a
/// reference picked), then its new description; Enter keeps the description it has.
#[derive(Default)]
struct DescriptionM {
    /// The block whose description is asked for, once known.
    block: Option<String>,
}

impl Interactive for DescriptionM {
    fn name(&self) -> &'static str {
        "BLOCKS.DESCRIPTION"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        if let Some(name) = default_block(s) {
            self.block = Some(check_block(s, &name)?);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match &self.block {
            Some(name) => {
                let cur = current(s, name);
                let p = Prompt::new(format!("Enter description for block \"{name}\""), Accept::TEXT);
                if cur.is_empty() { p } else { p.default(cur) }
            }
            None => Prompt::new(
                "Enter block name or select a block reference",
                Accept { point: true, number: false, text: true, select: true, enter: true },
            ),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(name) = self.block.clone() {
            return match i {
                Input::Text(t) => set(s, &name, t.trim()).map(|_| Step::Done),
                Input::Enter => Ok(Step::Done),
                _ => Ok(Step::Continue),
            };
        }
        match i {
            Input::Text(t) | Input::Keyword(t) => {
                self.block = Some(check_block(s, &t)?);
                Ok(Step::Continue)
            }
            Input::Pick(hs) => {
                let picked = {
                    let d = s.doc()?;
                    hs.iter().find_map(|h| super::bedit::block_of(d, *h))
                };
                match picked {
                    Some(n) => {
                        self.block = Some(check_block(s, &n)?);
                        Ok(Step::Continue)
                    }
                    None => Err(EngineError::Other("Select a block reference.".into())),
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}
