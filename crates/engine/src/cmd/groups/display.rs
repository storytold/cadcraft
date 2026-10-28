//! How a selected group shows (GROUPDISPLAYMODE: 0 the grips of every member, 1 one grip at the
//! group's centre, 2 the group's bounding box and that grip — the default), and copies of whole
//! groups becoming new unnamed groups.
//!
//! The centre grip is a grip of the group's first selected member with the index
//! [`GROUP_GRIP`]; dragging it moves the selection ([`crate::cmd::griphot`]).

use std::collections::HashSet;

use cadcraft_doc::{Drawing, Group, Handle};
use cadcraft_geom::{Bounds2, Vec2};

use crate::Session;

/// The grip index of a group's centre grip (no object has this many grips).
pub const GROUP_GRIP: usize = usize::MAX - 7;
/// Most groups shown with a single grip at once.
const MAX_SHOWN: usize = 1000;

/// A selected group drawn as one: its bounding box (mode 2) and centre grip.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupGrip {
    pub name: String,
    pub bounds: Bounds2,
    pub center: Vec2,
    /// Its members in the space being edited (all selected).
    pub members: Vec<Handle>,
    /// Draw the bounding box (GROUPDISPLAYMODE 2).
    pub frame: bool,
}

impl Session {
    /// The selected groups drawn with a single grip (GROUPDISPLAYMODE 1 and 2, with group
    /// selection on): a group shows so when all its members here are selected.
    pub fn group_grips(&self) -> Vec<GroupGrip> {
        let Ok(d) = self.doc() else { return Vec::new() };
        let mode = d.header.i64("GROUPDISPLAYMODE", 2);
        if mode == 0 || self.settings.pickstyle & 1 == 0 || d.groups.is_empty() {
            return Vec::new();
        }
        let sel: HashSet<Handle> = self.selection().into_iter().collect();
        if sel.is_empty() {
            return Vec::new();
        }
        let Some(store) = d.space(&self.space()) else { return Vec::new() };
        let mut out = Vec::new();
        for g in d.groups.iter().filter(|g| g.selectable) {
            let here: Vec<Handle> = g.members.iter().copied().filter(|m| store.get(*m).is_some()).collect();
            if here.is_empty() || !here.iter().all(|m| sel.contains(m)) {
                continue;
            }
            let mut bounds = Bounds2::EMPTY;
            for m in &here {
                if let Some(e) = store.get(*m) {
                    bounds = bounds.union(&cadcraft_doc::entity_bounds(d, e, 0));
                }
            }
            if bounds.is_empty() {
                continue;
            }
            out.push(GroupGrip { name: g.name.clone(), center: bounds.center(), bounds, members: here, frame: mode == 2 });
            if out.len() >= MAX_SHOWN {
                break;
            }
        }
        out
    }

    /// The centre of the shown group holding `h` (its [`GROUP_GRIP`]).
    pub fn group_grip_of(&self, h: Handle) -> Option<Vec2> {
        self.group_grips().into_iter().find(|g| g.members.contains(&h)).map(|g| g.center)
    }
}

/// After objects were copied (`pairs`: original, copy): every group all of whose members were
/// copied gets a new unnamed group of the copies, as in AutoCAD.
pub(crate) fn copy_groups(d: &mut Drawing, pairs: &[(Handle, Handle)]) {
    if d.groups.is_empty() || pairs.is_empty() {
        return;
    }
    let copied: std::collections::HashMap<Handle, Handle> = pairs.iter().copied().collect();
    let mut new = Vec::new();
    for g in &d.groups {
        if g.members.is_empty() || !g.members.iter().all(|m| copied.contains_key(m)) {
            continue;
        }
        let members: Vec<Handle> = g.members.iter().filter_map(|m| copied.get(m).copied()).collect();
        new.push(Group { name: String::new(), description: g.description.clone(), selectable: g.selectable, members });
    }
    for mut g in new {
        if d.groups.len() >= super::MAX_GROUPS {
            break;
        }
        g.name = super::next_unnamed(d);
        d.groups.push(g);
    }
}
