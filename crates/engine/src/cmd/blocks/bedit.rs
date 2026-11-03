//! BEDIT, BSAVE, BSAVEAS and BCLOSE: the Block Editor. BEDIT opens a block definition in the
//! drawing's own tab, its base point at the origin; the block's objects are edited with the
//! ordinary commands; BSAVE saves them into the definition, so every reference to the block in the
//! drawing shows the change; BCLOSE goes back to the drawing, saving or discarding what isn't saved.
//! BEDIT while the Block Editor is open switches it to another block: changes not saved yet are
//! saved or discarded first (asked at the command line; `save` in JSON).
//! The state is [`crate::blockedit`].

use cadcraft_doc::{Drawing, EntityKind, Handle};
use serde_json::{Value, json};

use crate::cmd::curves::{KW, handle_value};
use crate::cmd::{CommandSpec, bad, bool_or, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("bedit", "Block Editor", run_bedit)
            .menu(&["Tools", "Block Editor"])
            .alias(&["be", "-bedit"])
            .params("{name? (\"<Current Drawing>\": the drawing itself, a dynamic block's own drawing) | handle? (a block reference: edits its block); default the one selected block reference, create?: bool (a name no block has yet starts a new block; BSAVE creates it), save?: bool (in the Block Editor, switching to another block: save (true) or discard (false) the changes not saved yet; required when there are some), check?: bool (in the Block Editor: only check that the block can be switched to)} → {block, new, saved?}")
            .noundo()
            .interactive(|_| Ok(Box::new(BeditM::default()))),
        CommandSpec::new("bsave", "Save Block", run_bsave).params("{} → {block}").enabled(editing).noundo(),
        CommandSpec::new("bsaveas", "Save Block As", run_bsaveas)
            .params("{name, replace?: bool (redefine an existing block of that name)} → {block}")
            .enabled(editing_a_block)
            .noundo()
            .interactive(|_| Ok(Box::new(BsaveasM::default()))),
        CommandSpec::new("bclose", "Close Block Editor", run_bclose)
            .params("{save?: bool (default true; false discards the changes not saved into the block)} → {block, saved}")
            .enabled(editing)
            .noundo()
            .interactive(|_| Ok(Box::new(BcloseM))),
    ]
}

fn editing(s: &Session) -> std::result::Result<(), String> {
    match s.state() {
        Ok(st) if st.block_edit.is_some() => Ok(()),
        Ok(_) => Err("The Block Editor is not open.".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// The Block Editor is open on a block (not on the drawing itself, which BSAVEAS can't save as one).
fn editing_a_block(s: &Session) -> std::result::Result<(), String> {
    editing(s)?;
    match s.state() {
        Ok(st) if st.block_edit.as_ref().is_some_and(|b| b.current) => {
            Err("The Block Editor is open on the current drawing; there is no block to save under another name.".into())
        }
        _ => Ok(()),
    }
}

/// Commands that leave model space, which the Block Editor is: not available while it is open.
const NOT_IN_EDITOR: &[&str] = &[
    "layout",
    "layout.new",
    "layout.delete",
    "layout.rename",
    "layout.copy",
    "layout.set",
    "mview",
    "mview.polygonal",
    "mspace",
    "pspace",
    "chspace",
];

/// Why command `id` can't run now, when the Block Editor is open and it is one of [`NOT_IN_EDITOR`].
pub(crate) fn refused_in_editor(s: &Session, id: &str) -> Option<String> {
    let name = s.state().ok()?.block_editor()?;
    NOT_IN_EDITOR.contains(&id).then(|| format!("Not available in the Block Editor (editing \"{name}\"); close it first (BCLOSE)."))
}

/// The block a block reference refers to.
pub(super) fn block_of(d: &Drawing, h: Handle) -> Option<String> {
    match d.entity(h).map(|e| &e.kind) {
        Some(EntityKind::Insert(i)) => Some(dynamic_definition(d, &i.block).unwrap_or_else(|| i.block.clone())),
        _ => None,
    }
}

/// The dynamic block whose anonymous block `name` is (a dynamic block reference with values of
/// its own shows one, `*U12`): its BLOCK_RECORD's `AcDbBlockRepBTag` xdata names the dynamic
/// block's record. `None` for other blocks.
fn dynamic_definition(d: &Drawing, name: &str) -> Option<String> {
    if !name.starts_with('*') {
        return None;
    }
    let x = &d.foreign.records.get(&("BLOCK_RECORD".to_string(), name.to_ascii_uppercase()))?.xdata;
    let at = x.iter().position(|t| t.code == 1001 && t.value.eq_ignore_ascii_case("AcDbBlockRepBTag"))?;
    let record = x.iter().skip(at + 1).take_while(|t| t.code != 1001).find(|t| t.code == 1005).and_then(|t| Handle::parse_hex(&t.value))?;
    let (kind, block) = d.foreign.targets.get(&record)?;
    (kind == "BLOCK_RECORD").then(|| d.block(block).map(|b| b.name.clone())).flatten()
}

/// Whether the open Block Editor can switch to block `name`: the block is there (or `create` makes
/// it) and isn't one the editor refuses. Checked before anything is saved or closed, or asked.
fn check_target(s: &Session, name: &str, create: bool) -> Result<()> {
    let host = s.state()?.block_edit.as_ref().map(|b| b.host.doc.clone()).ok_or_else(|| bad("bedit", "the Block Editor is not open"))?;
    if name.trim().eq_ignore_ascii_case(crate::blockedit::CURRENT_DRAWING) {
        return Ok(());
    }
    match host.block(name) {
        None if !create => Err(bad("bedit", format!("Block \"{name}\" not found."))),
        None => Ok(()),
        Some(_) => crate::blockedit::refusal(&host, name).map_or(Ok(()), |why| Err(bad("bedit", why))),
    }
}

/// Open the Block Editor on block `name` and frame its objects. While it is open on another
/// block it switches: the changes not saved yet are saved first with `save` Some(true),
/// discarded with Some(false); with None there must be none. The block to switch to is checked
/// before anything is saved or closed, so a refused switch leaves the editor as it was.
fn open(s: &mut Session, name: &str, create: bool, save: Option<bool>) -> Result<Value> {
    let mut saved = None;
    if let Some(current) = s.state()?.block_editor().map(str::to_string) {
        if current.eq_ignore_ascii_case(name.trim()) {
            s.echo(format!("The Block Editor is already editing \"{current}\"."));
            return Ok(json!({ "block": current, "new": false }));
        }
        check_target(s, name, create)?;
        let save = match (s.state()?.block_unsaved(), save) {
            (false, _) => false,
            (true, Some(save)) => save,
            (true, None) => {
                return Err(bad(
                    "bedit",
                    format!("Block \"{current}\" has changes not saved; pass `save: true` to save them or `save: false` to discard them"),
                ));
            }
        };
        saved = close(s, save)?.get("saved").cloned();
    }
    let mut r = open_closed(s, name, create)?;
    if let (Some(v), Some(o)) = (saved, r.as_object_mut()) {
        o.insert("saved".into(), v);
    }
    Ok(r)
}

/// [`open`] with the Block Editor closed.
fn open_closed(s: &mut Session, name: &str, create: bool) -> Result<Value> {
    let new = create && !name.eq_ignore_ascii_case(crate::blockedit::CURRENT_DRAWING) && s.doc()?.block(name).is_none();
    s.state_mut()?.open_block_editor(name, create)?;
    let name = s.state()?.block_editor().unwrap_or(name).to_string();
    // The block's objects with room around them (and the editor's bar above them).
    s.zoom_extents()?;
    let c = s.state()?.view().center;
    s.zoom_about(0.75, c)?;
    s.echo(if new {
        format!("Block Editor: new block \"{name}\". BSAVE creates it in the drawing; BCLOSE closes the editor.")
    } else {
        format!("Block Editor: editing \"{name}\". BSAVE saves it into the drawing; BCLOSE closes the editor.")
    });
    let dynamic =
        |be: &crate::blockedit::BlockEdit| crate::blockedit::has_dynamic_data(&be.host.doc, if be.current { "*Model_Space" } else { &be.name });
    if s.state().is_ok_and(|st| st.block_edit.as_ref().is_some_and(dynamic)) {
        s.echo("This is a dynamic block: its parameters and actions are kept but can't be edited in CADCraft yet.");
    }
    Ok(json!({ "block": name, "new": new }))
}

/// The one block reference selected, if exactly one is.
pub(super) fn selected_reference(s: &Session) -> Option<String> {
    let d = s.doc().ok()?;
    let refs: Vec<String> = s.selection().iter().filter_map(|h| block_of(d, *h)).collect();
    match refs.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    }
}

fn run_bedit(s: &mut Session, p: &Value) -> Result<Value> {
    let name = match (str_param(p, "name"), p.get("handle")) {
        (Some(n), _) => n.to_string(),
        (None, Some(h)) => {
            let h = handle_value(h).ok_or_else(|| bad("bedit", "`handle` must be a handle"))?;
            block_of(s.doc()?, h).ok_or_else(|| bad("bedit", "`handle` is not a block reference"))?
        }
        (None, None) => selected_reference(s).ok_or_else(|| bad("bedit", "`name` or `handle` is required (or select one block reference)"))?,
    };
    let create = bool_or(p, "create", false);
    let name = name.trim();
    let current = name.eq_ignore_ascii_case(crate::blockedit::CURRENT_DRAWING);
    if create && !current && s.doc()?.block(name).is_none() && !super::valid_block_name(name) {
        return Err(bad("bedit", format!("\"{name}\" is not a valid block name")));
    }
    if p.get("save").is_some_and(|v| !v.is_null() && !v.is_boolean()) {
        return Err(bad("bedit", "`save` must be true or false"));
    }
    // Only whether the Block Editor, when open, can switch to the block: nothing opens or closes.
    if bool_or(p, "check", false) && s.state()?.block_editor().is_some() {
        check_target(s, name, create)?;
        return Ok(json!({ "block": name, "checked": true }));
    }
    open(s, name, create, p.get("save").and_then(Value::as_bool))
}

fn run_bsave(s: &mut Session, _p: &Value) -> Result<Value> {
    s.state_mut()?.save_block_editor(None)?;
    let name = s.state()?.block_editor().unwrap_or_default().to_string();
    s.echo(format!("Block \"{name}\" saved."));
    Ok(json!({ "block": name }))
}

/// Check a BSAVEAS name: valid, and not another existing block unless `replace`.
fn check_save_as(s: &Session, name: &str, replace: bool) -> Result<()> {
    if !super::valid_block_name(name) {
        return Err(bad("bsaveas", format!("\"{name}\" is not a valid block name")));
    }
    let d = s.doc()?;
    let current = s.state()?.block_editor().is_some_and(|b| b.eq_ignore_ascii_case(name));
    if !current && !replace && d.block(name).is_some() {
        return Err(bad("bsaveas", format!("Block \"{name}\" already exists; pass `replace: true` to redefine it")));
    }
    if let Some(why) = d.block(name).filter(|_| !current).and_then(|_| crate::blockedit::refusal(d, name)) {
        return Err(bad("bsaveas", why));
    }
    Ok(())
}

fn run_bsaveas(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("bsaveas", "`name` is required"))?.trim().to_string();
    check_save_as(s, &name, bool_or(p, "replace", false))?;
    s.state_mut()?.save_block_editor(Some(&name))?;
    let name = s.state()?.block_editor().unwrap_or_default().to_string();
    s.echo(format!("Block \"{name}\" saved; the Block Editor now edits it."));
    Ok(json!({ "block": name }))
}

fn close(s: &mut Session, save: bool) -> Result<Value> {
    let st = s.state()?;
    let name = st.block_editor().unwrap_or_default().to_string();
    let saved = save && st.block_unsaved();
    s.state_mut()?.close_block_editor(save)?;
    if saved {
        s.echo(format!("Block \"{name}\" saved."));
    }
    Ok(json!({ "block": name, "saved": saved }))
}

fn run_bclose(s: &mut Session, p: &Value) -> Result<Value> {
    close(s, bool_or(p, "save", true))
}

// ---------------- interactive ----------------

/// BEDIT: the selected block reference's block, else a block name typed or a reference picked.
/// In the Block Editor with changes not saved yet, it asks whether to save them before switching.
#[derive(Default)]
struct BeditM {
    /// The block to switch to, waiting for the answer to "Save changes?".
    pending: Option<String>,
}

impl BeditM {
    /// Go to block `name`: at once, or after asking about the changes not saved yet.
    fn go(&mut self, s: &mut Session, name: &str) -> Result<Step> {
        let st = s.state()?;
        let other = st.block_editor().is_some_and(|b| !b.eq_ignore_ascii_case(name.trim()));
        if other && st.block_unsaved() {
            // A block it can't switch to is refused before the question, which would trap the user.
            check_target(s, name.trim(), false)?;
            self.pending = Some(name.trim().to_string());
            return Ok(Step::Continue);
        }
        open(s, name, false, Some(false)).map(|_| Step::Done)
    }
}

impl Interactive for BeditM {
    fn name(&self) -> &'static str {
        "BEDIT"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        match selected_reference(s) {
            Some(name) => self.go(s, &name),
            None => Ok(Step::Continue),
        }
    }
    fn prompt(&self, s: &Session) -> Prompt {
        if self.pending.is_some() {
            let name = s.state().ok().and_then(|st| st.block_editor().map(str::to_string)).unwrap_or_default();
            return Prompt::new(format!("Save changes to block \"{name}\"?"), KW).kw(&["Yes", "No"]).default("Y");
        }
        Prompt::new("Enter block name or select a block reference [?]", Accept { point: true, number: false, text: true, select: true, enter: true })
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(name) = self.pending.clone() {
            let answer = match i {
                Input::Enter => "Y".to_string(),
                Input::Keyword(k) | Input::Text(k) => k,
                _ => return Ok(Step::Continue),
            };
            return match answer.trim().chars().next().map(|c| c.to_ascii_uppercase()) {
                Some('Y') => open(s, &name, false, Some(true)).map(|_| Step::Done),
                Some('N') => open(s, &name, false, Some(false)).map(|_| Step::Done),
                _ => Err(EngineError::Other("Enter Yes or No.".into())),
            };
        }
        match i {
            Input::Text(t) if t.trim() == "?" => {
                let names: Vec<String> = {
                    let d = s.doc()?;
                    d.blocks.values().filter(|b| crate::blockedit::refusal(d, &b.name).is_none()).map(|b| b.name.clone()).collect()
                };
                s.echo(if names.is_empty() { "No blocks to edit.".to_string() } else { names.join(", ") });
                Ok(Step::Continue)
            }
            Input::Text(t) | Input::Keyword(t) => self.go(s, t.trim()),
            Input::Pick(hs) => {
                let name = {
                    let d = s.doc()?;
                    hs.iter().find_map(|h| block_of(d, *h))
                };
                match name {
                    Some(n) => self.go(s, &n),
                    None => Err(EngineError::Other("Select a block reference.".into())),
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

/// BCLOSE: asks whether to save changes not saved yet.
struct BcloseM;

impl Interactive for BcloseM {
    fn name(&self) -> &'static str {
        "BCLOSE"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        if s.state()?.block_unsaved() { Ok(Step::Continue) } else { close(s, false).map(|_| Step::Done) }
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let name = s.state().ok().and_then(|st| st.block_editor().map(str::to_string)).unwrap_or_default();
        Prompt::new(format!("Save changes to block \"{name}\"?"), KW).kw(&["Yes", "No"]).default("Y")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let answer = match i {
            Input::Enter => "Y".to_string(),
            Input::Keyword(k) | Input::Text(k) => k,
            _ => return Ok(Step::Continue),
        };
        match answer.trim().chars().next().map(|c| c.to_ascii_uppercase()) {
            Some('Y') => close(s, true).map(|_| Step::Done),
            Some('N') => close(s, false).map(|_| Step::Done),
            _ => Err(EngineError::Other("Enter Yes or No.".into())),
        }
    }
}

/// BSAVEAS: a new block name; an existing block is redefined only when confirmed.
#[derive(Default)]
struct BsaveasM {
    /// The name given, waiting for "redefine?" because a block of that name exists.
    name: Option<String>,
}

impl BsaveasM {
    fn save(s: &mut Session, name: &str, replace: bool) -> Result<Step> {
        run_bsaveas(s, &json!({ "name": name, "replace": replace })).map(|_| Step::Done)
    }
}

impl Interactive for BsaveasM {
    fn name(&self) -> &'static str {
        "BSAVEAS"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match &self.name {
            Some(n) => Prompt::new(format!("Block \"{n}\" already exists. Redefine it?"), KW).kw(&["Yes", "No"]).default("N"),
            None => Prompt::new("Enter block name", Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(n) = self.name.clone() {
            let yes = matches!(&i, Input::Keyword(k) | Input::Text(k) if k.trim().to_ascii_uppercase().starts_with('Y'));
            return if yes { Self::save(s, &n, true) } else { Ok(Step::Done) };
        }
        let name = match i {
            Input::Text(t) => t.trim().to_string(),
            Input::Enter => return Ok(Step::Done),
            _ => return Ok(Step::Continue),
        };
        let current = s.state()?.block_editor().is_some_and(|b| b.eq_ignore_ascii_case(&name));
        if !current && s.doc()?.block(&name).is_some() {
            check_save_as(s, &name, true)?;
            self.name = Some(name);
            return Ok(Step::Continue);
        }
        Self::save(s, &name, false)
    }
}
