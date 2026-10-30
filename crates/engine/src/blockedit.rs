//! The Block Editor's state (BEDIT). While it is open, the drawing's tab shows the block's objects
//! as its model space, moved so that the block's base point is at the origin, and every command
//! edits them as it would any drawing. The drawing itself waits, untouched, with its undo history,
//! selection and views: BSAVE writes the objects back into the block definition (so every
//! reference shows them), BCLOSE puts the drawing back. The commands are in
//! `cmd::blocks::bedit`.
//!
//! Layer, style and block table changes made in the editor belong to the drawing too, as in
//! AutoCAD: saving keeps the editor's tables and only puts back what is the drawing's own (its
//! model and paper space objects, groups, constraints, tiled viewports and hidden objects).
//!
//! The editor can also open on the drawing itself ([`CURRENT_DRAWING`], AutoCAD's
//! `<Current Drawing>`): a dynamic block's own drawing, whose model space holds the block's
//! objects and authoring elements. Its objects are edited where they are; BSAVE keeps the
//! changes as the drawing's, BCLOSE without saving puts the drawing back as it was.

use std::sync::Arc;

use cadcraft_doc::{Drawing, EntityKind, EntityStore};
use cadcraft_geom::{Mat3, Vec2};

use crate::{DocState, EngineError, Result, Snapshot};

/// The undo step a save into the block records in the drawing.
const UNDO_LABEL: &str = "Block Editor";

/// The name the Block Editor gives the drawing itself (BEDIT `<Current Drawing>`).
pub const CURRENT_DRAWING: &str = "<Current Drawing>";

/// Whether block `name` (or `*Model_Space`, a drawing's own) carries dynamic block data from
/// another program: parameters and actions in the `ACAD_ENHANCEDBLOCK` dictionary of its
/// BLOCK_RECORD table entry.
pub fn has_dynamic_data(d: &Drawing, name: &str) -> bool {
    let key = ("BLOCK_RECORD".to_string(), name.to_ascii_uppercase());
    let Some(xdict) = d.foreign.records.get(&key).and_then(|x| x.xdict) else { return false };
    d.foreign.objects.iter().any(|o| o.handle == xdict && o.tags.iter().any(|t| t.code == 3 && t.value.eq_ignore_ascii_case("ACAD_ENHANCEDBLOCK")))
}

/// Whether drawing `d` has authoring elements: it is a dynamic block's own drawing, its model
/// space carrying the block's parameters and actions (AutoCAD offers to open it in the Block
/// Editor).
pub fn has_authoring_elements(d: &Drawing) -> bool {
    has_dynamic_data(d, "*Model_Space")
}

/// The Block Editor open in a drawing.
#[derive(Clone, Debug)]
pub struct BlockEdit {
    /// The block being edited, as the drawing names it.
    pub name: String,
    /// The drawing outside the editor: its drawing, undo history, selection and views, put back
    /// by BCLOSE. Its own `block_edit` is always `None`.
    pub host: Box<DocState>,
    /// The editor's drawing as last saved into the block; later changes are unsaved.
    pub saved: Arc<Drawing>,
    /// The block's objects as the drawing has them (block coordinates): objects left unchanged in
    /// the editor are saved back exactly as they were.
    original: EntityStore,
    /// The block's base point, at the origin in the editor.
    base: Vec2,
    /// The editor is open on the drawing itself ([`CURRENT_DRAWING`]).
    pub current: bool,
}

/// Why a block can't be opened in the Block Editor, if it can't.
pub fn refusal(d: &Drawing, name: &str) -> Option<String> {
    let Some(b) = d.block(name) else { return Some(format!("Block \"{name}\" not found.")) };
    if b.xref_path.is_some() {
        return Some(format!("\"{}\" is an external reference; open its drawing (XOPEN) to edit it.", b.name));
    }
    if b.anonymous || b.name.starts_with('*') {
        return Some(format!("\"{}\" is an anonymous block made by another object; it can't be edited in the Block Editor.", b.name));
    }
    None
}

/// `store` moved by `m`, objects in the same order with the same handles.
fn moved(store: &EntityStore, m: &Mat3) -> EntityStore {
    let mut out = EntityStore::new();
    for e in store.iter() {
        let mut e = (**e).clone();
        e.kind.transform(m);
        out.push(e);
    }
    out
}

/// Whether any object of `store` refers to block `name`, directly or through nested blocks of `d`.
fn refers_to(d: &Drawing, store: &EntityStore, name: &str) -> bool {
    let mut todo: Vec<String> = store.iter().filter_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.block.clone()) } else { None }).collect();
    let mut seen = std::collections::HashSet::new();
    while let Some(b) = todo.pop() {
        if b.eq_ignore_ascii_case(name) {
            return true;
        }
        if seen.insert(b.to_ascii_uppercase())
            && let Some(blk) = d.block(&b)
        {
            todo.extend(blk.entities.iter().filter_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.block.clone()) } else { None }));
        }
    }
    false
}

impl BlockEdit {
    /// The editor's objects in block coordinates, ready to be the block's: unchanged objects are
    /// the block's own, the others are moved back from the editor's origin to the base point.
    fn block_objects(&self, editor: &Drawing) -> EntityStore {
        if self.base == Vec2::ZERO {
            return editor.model.clone();
        }
        let (to_editor, back) = (Mat3::translate(-self.base), Mat3::translate(self.base));
        let mut out = EntityStore::new();
        for e in editor.model.iter() {
            let unchanged = self.original.get(e.handle).filter(|o| {
                let mut shown = (***o).clone();
                shown.kind.transform(&to_editor);
                shown == **e
            });
            match unchanged {
                Some(o) => out.push((**o).clone()),
                None => {
                    let mut e = (**e).clone();
                    e.kind.transform(&back);
                    out.push(e);
                }
            }
        }
        out
    }

    /// The drawing with the editor's objects saved into block `name` (the edited block, or a new
    /// name for BSAVEAS), from `editor`, the editor's drawing.
    pub fn compose(&self, editor: &Drawing, name: &str) -> Result<Drawing> {
        // The drawing itself: the editor's drawing is the drawing.
        if self.current {
            if !name.eq_ignore_ascii_case(CURRENT_DRAWING) {
                return Err(EngineError::Other("The current drawing can't be saved as a block from the Block Editor; use WBLOCK or BLOCK.".into()));
            }
            return Ok(editor.clone());
        }
        if refers_to(editor, &editor.model, name) {
            return Err(EngineError::Other(format!("Block \"{name}\" can't contain a reference to itself.")));
        }
        let host = &self.host.doc;
        let mut d = editor.clone();
        // When the block is saved under another name, the edited block keeps the drawing's
        // description and block `name` gets the description changed in the editor.
        if !name.eq_ignore_ascii_case(&self.name) {
            keep_description(&mut d, host, &self.name);
        }
        d.model = host.model.clone();
        d.layouts = host.layouts.clone();
        d.groups = host.groups.clone();
        d.constraints = host.constraints.clone();
        d.vports = host.vports.clone();
        d.hidden_objects = host.hidden_objects.clone();
        let entities = self.block_objects(editor);
        // The edited block's properties (the editor's definition, else the drawing's): the objects
        // are placed for its base point, also when saved under another name (BSAVEAS).
        let template = editor.block(&self.name).or_else(|| host.block(&self.name));
        let mut b = match template {
            Some(b) => (**b).clone(),
            None => cadcraft_doc::Block::new(name),
        };
        // Block names are case-insensitive: an existing block keeps its name's spelling.
        let key = d.blocks.keys().find(|k| k.eq_ignore_ascii_case(name)).cloned().unwrap_or_else(|| name.to_string());
        if !b.name.eq_ignore_ascii_case(name) {
            b.name = key.clone();
        }
        b.base.x = self.base.x;
        b.base.y = self.base.y;
        b.entities = entities;
        d.blocks.insert(key, Arc::new(b));
        Ok(d)
    }
}

/// Give block `name` of `d` the description `host` has for it, or, when `host` has no such block,
/// drop the empty definition a description gives a block the editor started and never saved.
fn keep_description(d: &mut Drawing, host: &Drawing, name: &str) {
    match host.block(name) {
        Some(h) => {
            if let Some((_, b)) = d.blocks.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(name))
                && b.description != h.description
            {
                Arc::make_mut(b).description = h.description.clone();
            }
        }
        None => d.blocks.retain(|k, b| !(k.eq_ignore_ascii_case(name) && b.entities.is_empty())),
    }
}

impl DocState {
    /// The drawing as a file saves it: outside the Block Editor's changes while it is open.
    pub fn drawing(&self) -> &Arc<Drawing> {
        match &self.block_edit {
            Some(be) => &be.host.doc,
            None => &self.doc,
        }
    }
    /// [`DocState::drawing`], to change it (a save's file version).
    pub fn drawing_arc_mut(&mut self) -> &mut Arc<Drawing> {
        match &mut self.block_edit {
            Some(be) => &mut be.host.doc,
            None => &mut self.doc,
        }
    }
    /// The block the Block Editor is editing, when it is open.
    pub fn block_editor(&self) -> Option<&str> {
        self.block_edit.as_ref().map(|b| b.name.as_str())
    }
    /// The Block Editor has changes not yet saved into the block.
    pub fn block_unsaved(&self) -> bool {
        self.block_edit.as_ref().is_some_and(|b| !Arc::ptr_eq(&b.saved, &self.doc))
    }
    /// The drawing to keep for recovery (autosave): with the Block Editor's unsaved changes in the
    /// block, so a crash doesn't lose them. An error when the editor's changes can't be put in the
    /// block (it would contain itself): the host drawing alone would lose them.
    pub fn recovery_drawing(&self) -> Result<Arc<Drawing>> {
        match &self.block_edit {
            Some(be) if self.block_unsaved() => be.compose(&self.doc, &be.name).map(Arc::new),
            Some(be) => Ok(be.host.doc.clone()),
            None => Ok(self.doc.clone()),
        }
    }

    /// Open the Block Editor on block `name` (BEDIT).
    /// With `create`, a name no block has yet starts a new, empty block, which BSAVE creates.
    pub fn open_block_editor(&mut self, name: &str, create: bool) -> Result<()> {
        if let Some(open) = self.block_editor() {
            return Err(EngineError::Other(format!("The Block Editor is already open on \"{open}\"; close it first (BCLOSE).")));
        }
        if name.trim().eq_ignore_ascii_case(CURRENT_DRAWING) {
            let host = Box::new(self.clone());
            let editor = self.doc.clone();
            self.enter_block_editor(BlockEdit {
                name: CURRENT_DRAWING.into(),
                host,
                saved: editor,
                original: EntityStore::new(),
                base: Vec2::ZERO,
                current: true,
            });
            return Ok(());
        }
        let block = match self.doc.block(name) {
            None if create => Arc::new(cadcraft_doc::Block::new(name)),
            _ => {
                if let Some(why) = refusal(&self.doc, name) {
                    return Err(EngineError::Other(why));
                }
                self.doc.block(name).cloned().ok_or_else(|| EngineError::Other(format!("Block \"{name}\" not found.")))?
            }
        };
        let base = block.base.xy();
        let mut d = (*self.doc).clone();
        d.model = if base == Vec2::ZERO { block.entities.clone() } else { moved(&block.entities, &Mat3::translate(-base)) };
        // The drawing's own groups, constraints and hidden objects stay with it.
        d.groups.clear();
        d.constraints.clear();
        d.hidden_objects.clear();
        let host = Box::new(self.clone());
        let editor = Arc::new(d);
        self.doc = editor.clone();
        self.enter_block_editor(BlockEdit { name: block.name.clone(), host, saved: editor, original: block.entities.clone(), base, current: false });
        Ok(())
    }

    /// Start editing in the Block Editor `be` (the drawing is already the editor's): a fresh
    /// undo history, nothing selected, model space.
    fn enter_block_editor(&mut self, be: BlockEdit) {
        self.undo.clear();
        self.redo.clear();
        self.undo_ctl = Default::default();
        self.selection.clear();
        self.previous_selection.clear();
        self.space = cadcraft_doc::Space::Model;
        self.mspace = None;
        self.tiles = Default::default();
        self.views.clear();
        self.view_history.clear();
        self.block_edit = Some(be);
        self.revision += 1;
    }

    /// Save the Block Editor's objects into the block (BSAVE), or into a new block `as_name`
    /// (BSAVEAS), which the editor then edits. One undo step in the drawing.
    pub fn save_block_editor(&mut self, as_name: Option<&str>) -> Result<()> {
        let Some(be) = &self.block_edit else { return Err(EngineError::Other("The Block Editor is not open.".into())) };
        let name = as_name.unwrap_or(&be.name).to_string();
        let d = be.compose(&self.doc, &name)?;
        let original = d.block(&name).map(|b| b.entities.clone()).unwrap_or_default();
        let name = d.block(&name).map(|b| b.name.clone()).unwrap_or(name);
        // When BSAVEAS saves the block under another name, the editor's drawing and each undo and
        // redo step get the block as saved and the edited block as the drawing has it, so a later
        // save and the description shown start from them after an undo too.
        if !name.eq_ignore_ascii_case(&be.name)
            && let Some(b) = d.block(&name).cloned()
        {
            let was = be.name.clone();
            let reconcile = |doc: &mut Arc<Drawing>| {
                let editor = Arc::make_mut(doc);
                keep_description(editor, &d, &was);
                editor.blocks.retain(|k, _| !k.eq_ignore_ascii_case(&name));
                editor.blocks.insert(name.clone(), b.clone());
            };
            reconcile(&mut self.doc);
            for step in self.undo.iter_mut().chain(self.redo.iter_mut()) {
                reconcile(&mut step.doc);
            }
        }
        let editor = self.doc.clone();
        let Some(be) = &mut self.block_edit else { return Ok(()) };
        let before = be.host.doc.clone();
        let selection = be.host.selection.clone();
        be.host.push_undo(Snapshot { label: UNDO_LABEL.into(), doc: before, selection });
        be.host.doc = Arc::new(d);
        be.host.revision += 1;
        be.original = original;
        be.saved = editor;
        be.name = name;
        self.revision += 1;
        Ok(())
    }

    /// Close the Block Editor (BCLOSE), saving its changes into the block first when `save`.
    pub fn close_block_editor(&mut self, save: bool) -> Result<()> {
        if self.block_edit.is_none() {
            return Err(EngineError::Other("The Block Editor is not open.".into()));
        }
        if save && self.block_unsaved() {
            self.save_block_editor(None)?;
        }
        let Some(be) = self.block_edit.take() else { return Ok(()) };
        let host = *be.host;
        self.doc = host.doc;
        self.undo = host.undo;
        self.redo = host.redo;
        self.undo_ctl = host.undo_ctl;
        // The drawing comes back with nothing selected; Previous is what was selected before.
        self.selection.clear();
        self.previous_selection = if host.selection.is_empty() { host.previous_selection } else { host.selection };
        self.space = host.space;
        self.mspace = host.mspace;
        self.tiles = host.tiles;
        self.views = host.views;
        self.view_history = host.view_history;
        self.revision += 1;
        Ok(())
    }
}
