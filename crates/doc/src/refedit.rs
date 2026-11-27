//! In-place editing of a block reference (REFEDIT / REFSET / REFCLOSE): the state kept with the
//! drawing while it lasts.
//!
//! The reference being edited is hidden and the block's objects stand in the drawing as ordinary
//! objects (the *working set*, transformed to where the reference shows them), the rest of the
//! block as faded copies (*ghosts*) that cannot be edited. REFCLOSE writes the working set back
//! into the block definition. The state lives in the [`Drawing`] itself, so undo and redo move
//! the working copies and the state together and can never leave one without the other.

use std::borrow::Cow;

use cadcraft_geom::Mat3;

use crate::{Drawing, Entity, Handle, Space};

/// One object of the working set.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkItem {
    /// The object in the drawing (a copy of the block object, or a drawing object added to the set).
    pub copy: Handle,
    /// The block object it was copied from; `None` for an object added from the drawing.
    pub source: Option<Handle>,
}

/// An active in-place edit of a block reference. Never written to a file.
#[derive(Clone, Debug, PartialEq)]
pub struct RefEdit {
    /// The block being edited.
    pub block: String,
    /// The reference that was picked (hidden while editing).
    pub insert: Handle,
    /// Whether the reference was visible before it was hidden.
    pub insert_was_visible: bool,
    /// The space the reference is in.
    pub space: Space,
    /// Block coordinates to drawing coordinates, through the picked reference.
    pub to_world: Mat3,
    /// The working set.
    pub items: Vec<WorkItem>,
    /// The rest of the block as faded copies: (copy, block object).
    pub ghosts: Vec<(Handle, Handle)>,
    /// Block objects taken out of the working set with their edits (block coordinates, the block
    /// object's handle).
    pub released: Vec<Entity>,
    /// The nested references picked through to reach the edited block: block references inside
    /// block definitions, outermost first. Empty when the picked reference's own block is edited.
    pub path: Vec<Handle>,
    /// Set when the edited block comes from an external reference: REFCLOSE Save writes it back
    /// into its source drawing.
    pub xref: Option<XrefSource>,
}

/// Where an xref edited in place is written back to.
#[derive(Clone, Debug, PartialEq)]
pub struct XrefSource {
    /// The xref (its block name in this drawing).
    pub xref: String,
    /// The source drawing file.
    pub file: String,
    /// `file` with every symbolic link resolved when the edit began: the file that is written.
    /// The save is refused when `file` leads somewhere else by then.
    pub real: String,
    /// The version the file was in when the edit began, as its `$ACADVER` code ("AC1015"): it
    /// is written back in that version (`None`: in the drawing's own).
    pub version: Option<String>,
    /// Whether the file is a binary DXF (written back as one).
    pub binary: bool,
    /// The block of the source drawing being edited; `None` for its model space.
    pub block: Option<String>,
    /// The file's size and modification time when the edit began.
    pub stamp: Option<(u64, std::time::SystemTime)>,
    /// Edited block objects (their handles in this drawing) and the objects of the source
    /// drawing they were loaded from.
    pub sources: Vec<(Handle, Handle)>,
}

impl RefEdit {
    /// Whether `h` is a working set object.
    pub fn is_working(&self, h: Handle) -> bool {
        self.items.iter().any(|i| i.copy == h)
    }
    /// Whether `h` is something the edit manages (working set, ghost, the hidden reference).
    pub fn owns(&self, h: Handle) -> bool {
        h == self.insert || self.is_working(h) || self.ghosts.iter().any(|(g, _)| *g == h)
    }
}

impl Drawing {
    /// The drawing as REFCLOSE Discard would leave it: the working copies and ghosts gone, the
    /// reference shown, the block untouched. Borrowed when no edit is active. Files are written
    /// from this, so a saved or autosaved drawing never holds half an edit.
    pub fn committed(&self) -> Cow<'_, Drawing> {
        let Some(r) = &self.refedit else { return Cow::Borrowed(self) };
        let mut d = self.clone();
        d.end_refedit_discard(r);
        Cow::Owned(d)
    }

    /// Remove the edit's working copies and ghosts, show the reference again and clear the state.
    pub fn end_refedit_discard(&mut self, r: &RefEdit) {
        for i in &r.items {
            if i.source.is_some() {
                self.remove_entity(i.copy);
            }
        }
        for (g, _) in &r.ghosts {
            self.remove_entity(*g);
        }
        let vis = r.insert_was_visible;
        let _ = self.modify_entity(r.insert, |e| e.common.visible = vis);
        self.refedit = None;
    }
}
