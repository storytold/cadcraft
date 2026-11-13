//! Storeys: the floors of a building drawn on top of each other in model space, as in
//! architectural CAD.
//!
//! Layers and storeys are two independent axes. A layer is a trade (walls, plumbing, furniture)
//! that runs through the whole building: turning it off hides it on every storey. A storey is a
//! property of each object: only the objects of the current storey are shown and edited. One
//! other storey can be shown as a *ghost* beneath the current one, drawn in a single colour: it
//! can be snapped to and nothing else (it is never selected or changed).
//!
//! A drawing without storeys behaves as one without the feature. Once it has storeys, every
//! model-space object belongs to one; an object without a valid storey (drawn by another
//! program, pasted from another drawing) belongs to the current storey and is given it after
//! the next command.
//!
//! In files, the storey list is CADCraft's `CADCRAFT_STOREYS` XRECORD and an object's storey is
//! its xdata of the registered application [`STOREY_APP`] (one `1071` group), which a copy of
//! the object carries and which other programs keep.

use serde::{Deserialize, Serialize};

use crate::{Common, Drawing, Entity, Handle, RawTag};

/// Registered application name of an object's storey xdata.
pub const STOREY_APP: &str = "CADCRAFT_STOREY";
/// Most storeys a drawing keeps (hostile files).
pub const MAX_STOREYS: usize = 1000;
/// Longest storey name.
pub const MAX_STOREY_NAME: usize = 255;
/// The ghost colour when none is chosen: a light beige that reads as "beneath" on dark and
/// light backgrounds alike.
pub const DEFAULT_GHOST_COLOR: cadcraft_color::Rgb = cadcraft_color::Rgb(0xC8, 0xB4, 0x8C);

/// One storey.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Storey {
    /// Stable id: objects refer to it, so renaming or reordering storeys never touches them.
    pub id: u32,
    pub name: String,
}

/// The drawing's storeys, bottom to top, and the current one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Storeys {
    pub list: Vec<Storey>,
    /// Id of the current storey (always one of `list` when it is not empty).
    pub current: u32,
    /// The next id to hand out: above every id ever used, so a deleted storey's id never comes back
    /// (an object restored later with it would land on a different storey).
    pub next: u32,
    /// The start storey (0: none): number 0, the storeys above count 1, 2, 3, those below -1, -2;
    /// a drawing opens on it.
    pub home: u32,
}

/// A start setup for drawings: storey names top to bottom and the start storey among them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StoreyTemplate {
    pub names: Vec<String>,
    pub start: String,
}

impl Default for StoreyTemplate {
    /// A building: roof, two upper floors, the ground floor (the start, number 0), two basements.
    fn default() -> Self {
        StoreyTemplate { names: ["DG", "2.OG", "1.OG", "EG", "1.UG", "2.UG"].map(String::from).to_vec(), start: "EG".into() }
    }
}

impl StoreyTemplate {
    /// Other stacks than buildings: the drawing (number 0), drawings above it, blueprints beneath.
    pub fn blueprints() -> Self {
        StoreyTemplate {
            names: ["Drawing 2", "Drawing 1", "Drawing", "Blueprint 1", "Blueprint 2", "Blueprint 3"].map(String::from).to_vec(),
            start: "Drawing".into(),
        }
    }
}

/// The largest storey id: the DXF group 1071 that stores it is a signed 32-bit integer.
pub const MAX_STOREY_ID: u32 = i32::MAX as u32;

/// Whether two storey names are the same (names are compared without case, in every script).
pub fn same_storey_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

impl Storeys {
    /// Whether the drawing has storeys at all.
    pub fn active(&self) -> bool {
        !self.list.is_empty()
    }
    pub fn get(&self, id: u32) -> Option<&Storey> {
        self.list.iter().find(|s| s.id == id)
    }
    pub fn contains(&self, id: u32) -> bool {
        self.get(id).is_some()
    }
    /// The storey named `name` (case-insensitive).
    pub fn by_name(&self, name: &str) -> Option<&Storey> {
        self.list.iter().find(|s| same_storey_name(&s.name, name))
    }
    /// Position of storey `id` from the bottom.
    pub fn index(&self, id: u32) -> Option<usize> {
        self.list.iter().position(|s| s.id == id)
    }
    /// A fresh id: never one in use or used before; `None` when every id is taken.
    pub fn next_id(&self) -> Option<u32> {
        let above = self.list.iter().map(|s| s.id).max().unwrap_or(0).max(self.next.saturating_sub(1));
        match above.checked_add(1).filter(|id| *id <= MAX_STOREY_ID) {
            Some(id) => Some(id),
            // Past the top (a hostile file): the first free id.
            None => (1..=MAX_STOREY_ID).take(MAX_STOREYS + 1).find(|id| !self.contains(*id)),
        }
    }
    /// Note that `id` was handed out.
    pub fn took(&mut self, id: u32) {
        self.next = self.next.max(id.saturating_add(1));
    }
    /// The current storey.
    pub fn current_storey(&self) -> Option<&Storey> {
        self.get(self.current)
    }
    /// Position of storey number 0 from the bottom: the start storey's, else the bottom one's.
    fn zero(&self) -> usize {
        self.index(self.home).unwrap_or(0)
    }
    /// A storey's number: 0 for the start storey, counting up above it and down below it.
    pub fn number(&self, id: u32) -> Option<i64> {
        let i = i64::try_from(self.index(id)?).ok()?;
        Some(i - i64::try_from(self.zero()).ok()?)
    }
    /// The storey a person means by `text`: its name, else its number ("0", "-1", "2").
    pub fn find(&self, text: &str) -> Option<&Storey> {
        self.by_name(text).or_else(|| {
            let n: i64 = text.trim().parse().ok()?;
            let i = i64::try_from(self.zero()).ok()?.checked_add(n)?;
            self.list.get(usize::try_from(i).ok()?)
        })
    }
    /// Make the list usable after reading it from a file: no duplicate ids or names, names
    /// trimmed and bounded, at most [`MAX_STOREYS`], and a current storey that exists.
    pub fn sanitize(&mut self) {
        let mut ids = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        self.list.truncate(MAX_STOREYS);
        self.list.retain_mut(|s| {
            s.name = s.name.trim().chars().take(MAX_STOREY_NAME).collect();
            (1..=MAX_STOREY_ID).contains(&s.id) && !s.name.is_empty() && ids.insert(s.id) && names.insert(s.name.to_lowercase())
        });
        if !self.contains(self.current) {
            self.current = self.list.first().map_or(0, |s| s.id);
        }
        let top = self.list.iter().map(|s| s.id).max().unwrap_or(0);
        self.next = self.next.clamp(top.saturating_add(1), MAX_STOREY_ID.saturating_add(1));
        if !self.contains(self.home) {
            self.home = 0;
        }
    }
}

/// How an object shows with respect to the storeys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreyView {
    /// On the current storey (or the drawing has no storeys): shown and edited as usual.
    Current,
    /// On the ghost storey: drawn in the ghost colour beneath the current storey, snappable, never
    /// selected or changed.
    Ghost,
    /// On another storey: not there for display, selection, snapping or plotting.
    Hidden,
}

impl Common {
    /// The storey id stored with the object, if any.
    pub fn storey(&self) -> Option<u32> {
        storey_of_xdata(&self.xdata)
    }
    /// Store `id` as the object's storey (`None` removes it). Every group of the application goes
    /// (a damaged file can carry several), so exactly one is left.
    pub fn set_storey(&mut self, id: Option<u32>) {
        while let Some(r) = app_range(&self.xdata, STOREY_APP) {
            self.xdata.drain(r);
        }
        if let Some(id) = id {
            self.xdata.push(RawTag { code: 1001, value: STOREY_APP.into() });
            self.xdata.push(RawTag { code: 1071, value: id.to_string() });
        }
    }
}

/// The range of `xdata` holding application `app`'s groups (from its `1001` group to the next).
fn app_range(xdata: &[RawTag], app: &str) -> Option<std::ops::Range<usize>> {
    let start = xdata.iter().position(|t| t.code == 1001 && t.value.trim().eq_ignore_ascii_case(app))?;
    let len = xdata.iter().skip(start + 1).position(|t| t.code == 1001).unwrap_or(xdata.len() - start - 1);
    Some(start..start + 1 + len)
}

/// The storey id in an object's xdata.
fn storey_of_xdata(xdata: &[RawTag]) -> Option<u32> {
    if xdata.is_empty() {
        return None;
    }
    let r = app_range(xdata, STOREY_APP)?;
    xdata.get(r)?.iter().find(|t| t.code == 1071).and_then(|t| t.value.trim().parse::<u32>().ok()).filter(|id| (1..=MAX_STOREY_ID).contains(id))
}

impl Drawing {
    /// How `e` shows with respect to the storeys (see [`StoreyView`]). Objects outside model space
    /// (paper space, block contents) are always [`StoreyView::Current`].
    pub fn storey_view(&self, e: &Entity) -> StoreyView {
        if !self.storeys.active() {
            return StoreyView::Current;
        }
        match e.common.storey() {
            // Only model-space objects are on storeys; a block's contents show with their reference.
            Some(id) if id != self.storeys.current && self.storeys.contains(id) && self.model.contains(e.handle) => {
                // Hatches in the blueprint only when the user wants them.
                if self.ghost_storey == Some(id) && (self.ghost_hatches || !matches!(e.kind, crate::EntityKind::Hatch(_))) {
                    StoreyView::Ghost
                } else {
                    StoreyView::Hidden
                }
            }
            _ => StoreyView::Current,
        }
    }

    /// The storey `e` belongs to: its own when the drawing has it, else the current storey
    /// (`None` when the drawing has no storeys).
    pub fn storey_of(&self, e: &Entity) -> Option<u32> {
        if !self.storeys.active() {
            return None;
        }
        Some(e.common.storey().filter(|id| self.storeys.contains(*id)).unwrap_or(self.storeys.current))
    }

    /// Whether `e` is drawn as part of the ghost storey: on it, its own flag and its layer
    /// visible, not hidden by object isolation. Ghost objects can be snapped to and nothing else.
    pub fn is_ghost(&self, e: &Entity) -> bool {
        self.ghost_storey.is_some()
            && self.storey_view(e) == StoreyView::Ghost
            && e.common.visible
            && self.layer(&e.common.layer).is_none_or(crate::Layer::visible)
            && !self.is_hidden_object(e.handle)
    }

    /// Whether `e` and the object `other` are on the same storey (true when the drawing has none or
    /// either is not a model-space object).
    pub fn same_storey(&self, e: &Entity, other: Handle) -> bool {
        if !self.storeys.active() || !self.model.contains(e.handle) {
            return true;
        }
        match self.model.get(other) {
            Some(o) => self.storey_of(o) == self.storey_of(e),
            None => true,
        }
    }

    /// Drop the links of `e` (dimension associativity, associative hatch boundaries) to objects on
    /// other storeys: a storey can't follow another one it can't change, so such a link would only
    /// go stale. True when anything went.
    pub fn cut_storey_links(&self, e: &mut Entity) -> bool {
        if !self.storeys.active() || !self.model.contains(e.handle) {
            return false;
        }
        // The storey `e` has (or gets: a new object without one is on the current storey).
        let own = Some(e.common.storey().filter(|id| self.storeys.contains(*id)).unwrap_or(self.storeys.current));
        let same = |h: Handle| self.model.get(h).is_none_or(|o| self.storey_of(o) == own);
        match &mut e.kind {
            crate::EntityKind::Dimension(dm) => {
                let n = dm.assoc.len();
                dm.assoc.retain(|a| same(a.handle) && !matches!(a.snap, crate::AssocSnap::Intersection { other } if !same(other)));
                dm.assoc.len() != n
            }
            crate::EntityKind::Hatch(h) => {
                let mut cut = false;
                for l in &mut h.loops {
                    if l.sources.iter().any(|s| !same(*s)) {
                        l.sources.clear();
                        cut = true;
                    }
                }
                if cut && h.loops.iter().all(|l| l.sources.is_empty()) {
                    h.associative = false;
                }
                cut
            }
            _ => false,
        }
    }

    /// [`Drawing::cut_storey_links`] for every model-space object (a drawing as it is opened: a file
    /// edited elsewhere can bring links between storeys).
    pub fn cut_all_storey_links(&mut self) {
        if !self.storeys.active() {
            return;
        }
        let hs: Vec<Handle> = self.model.iter().map(|e| e.handle).collect();
        for h in hs {
            let Some(mut e) = self.model.get(h).map(|e| (**e).clone()) else { continue };
            if self.cut_storey_links(&mut e) {
                self.model.replace(e);
            }
        }
    }

    /// Give a drawing without storeys the storeys of `t`, every model-space object on its start
    /// storey, which becomes current; false when the drawing has storeys already or `t` has none.
    pub fn apply_storey_template(&mut self, t: &StoreyTemplate) -> bool {
        if self.storeys.active() {
            return false;
        }
        // Ids go on from the drawing's highest ever: an id a deleted storey had never comes back
        // (an object restored with it would land on a new storey).
        let first = self.storeys.next.max(1);
        // All or nothing: a setup that no longer fits the id range changes nothing.
        let fits = u32::try_from(t.names.len()).ok().and_then(|n| first.checked_add(n.saturating_sub(1))).is_some_and(|last| last <= MAX_STOREY_ID);
        if !fits {
            return false;
        }
        let mut s = Storeys { next: first, ..Storeys::default() };
        for name in t.names.iter().rev() {
            let id = u32::try_from(s.list.len()).ok().and_then(|n| first.checked_add(n)).filter(|id| *id <= MAX_STOREY_ID);
            let Some(id) = id else { break };
            s.list.push(Storey { id, name: name.clone() });
        }
        s.sanitize();
        if !s.active() {
            return false;
        }
        let start = s.by_name(&t.start).or_else(|| s.list.first()).map_or(0, |x| x.id);
        s.current = start;
        s.home = start;
        let top = s.list.iter().map(|x| x.id).max().unwrap_or(0);
        s.took(top);
        self.storeys = s;
        // A blueprint left from before means nothing on the new storeys.
        self.ghost_storey = None;
        self.ghost_memory.clear();
        self.assign_storeys();
        true
    }

    /// As a drawing opens: on its start storey.
    pub fn go_home(&mut self) {
        if self.storeys.contains(self.storeys.home) {
            self.storeys.current = self.storeys.home;
            self.fix_ghost();
        }
    }

    /// Keep the ghost storey meaningful: one that exists and is not the current storey (the
    /// current storey is never its own ghost); otherwise none.
    /// Go to storey `id`. With [`Drawing::ghost_per_storey`] the storey left keeps its blueprint
    /// and the storey reached gets back the one it had (a storey not visited yet keeps the
    /// blueprint shown now).
    pub fn switch_storey(&mut self, id: u32) {
        let from = self.storeys.current;
        if self.ghost_per_storey {
            self.ghost_memory.retain(|(s, _)| *s != from);
            self.ghost_memory.push((from, self.ghost_storey));
            if let Some(&(_, ghost)) = self.ghost_memory.iter().find(|(s, _)| *s == id) {
                self.ghost_storey = ghost;
            }
        }
        self.storeys.current = id;
        self.fix_ghost();
    }

    pub fn fix_ghost(&mut self) {
        let st = &self.storeys;
        self.ghost_storey = self.ghost_storey.filter(|g| *g != st.current && st.contains(*g));
    }

    /// Give every model-space object without a valid storey the current one; true when any
    /// changed. Run after each command, so objects created by any command, paste or script land
    /// on the storey they were drawn on.
    pub fn assign_storeys(&mut self) -> bool {
        if !self.storeys.active() {
            return false;
        }
        let current = self.storeys.current;
        let storeys = &self.storeys;
        let lost: Vec<crate::Handle> =
            self.model.iter().filter(|e| e.common.storey().is_none_or(|id| !storeys.contains(id))).map(|e| e.handle).collect();
        for h in &lost {
            self.model.modify(*h, |e| e.common.set_storey(Some(current)));
        }
        !lost.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntityKind, Line, Space};
    use cadcraft_geom::Vec3;

    fn two_storeys() -> Drawing {
        let mut d = Drawing::new_metric();
        d.storeys =
            Storeys { list: vec![Storey { id: 1, name: "EG".into() }, Storey { id: 2, name: "OG1".into() }], current: 1, ..Storeys::default() };
        d
    }

    fn line(d: &mut Drawing, storey: Option<u32>) -> crate::Handle {
        let mut c = Common::default();
        c.set_storey(storey);
        d.add(&Space::Model, c, EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) })).unwrap_or_default()
    }

    #[test]
    fn storey_xdata_round_trips_and_keeps_other_apps() {
        let mut c =
            Common { xdata: vec![RawTag { code: 1001, value: "ACME".into() }, RawTag { code: 1000, value: "hello".into() }], ..Common::default() };
        assert_eq!(c.storey(), None);
        c.set_storey(Some(7));
        assert_eq!(c.storey(), Some(7));
        c.set_storey(Some(3));
        assert_eq!(c.storey(), Some(3));
        assert_eq!(c.xdata.iter().filter(|t| t.code == 1001).count(), 2, "one storey application, not two");
        c.set_storey(None);
        assert_eq!(c.xdata, vec![RawTag { code: 1001, value: "ACME".into() }, RawTag { code: 1000, value: "hello".into() }]);
    }

    #[test]
    fn storey_app_in_the_middle_is_replaced_in_place() {
        let mut c = Common {
            xdata: vec![
                RawTag { code: 1001, value: STOREY_APP.into() },
                RawTag { code: 1071, value: "2".into() },
                RawTag { code: 1001, value: "ACME".into() },
                RawTag { code: 1000, value: "x".into() },
            ],
            ..Common::default()
        };
        assert_eq!(c.storey(), Some(2));
        c.set_storey(Some(5));
        assert_eq!(c.storey(), Some(5));
        assert!(c.xdata.windows(2).any(|w| w[0].value == "ACME" && w[1].value == "x"), "the other application stays whole");
    }

    #[test]
    fn hostile_storey_values_read_as_none() {
        for v in ["", "-1", "abc", "99999999999"] {
            let c =
                Common { xdata: vec![RawTag { code: 1001, value: STOREY_APP.into() }, RawTag { code: 1071, value: v.into() }], ..Common::default() };
            assert_eq!(c.storey(), None, "{v:?}");
        }
    }

    #[test]
    fn views_follow_current_and_ghost() {
        let mut d = two_storeys();
        let eg = line(&mut d, Some(1));
        let og = line(&mut d, Some(2));
        let none = line(&mut d, None);
        let unknown = line(&mut d, Some(99));
        let view = |d: &Drawing, h| d.entity(h).map(|e| d.storey_view(e));
        assert_eq!(view(&d, eg), Some(StoreyView::Current));
        assert_eq!(view(&d, og), Some(StoreyView::Hidden));
        assert_eq!(view(&d, none), Some(StoreyView::Current), "no storey: the current one");
        assert_eq!(view(&d, unknown), Some(StoreyView::Current), "unknown storey: the current one");
        d.ghost_storey = Some(2);
        assert_eq!(view(&d, og), Some(StoreyView::Ghost));
        assert!(d.entity(og).is_some_and(|e| d.is_ghost(e) && !d.is_visible(e)), "a ghost is drawn, not visible for editing");
        d.storeys.current = 2;
        assert_eq!(view(&d, og), Some(StoreyView::Current), "the current storey is never its own ghost");
        assert_eq!(view(&d, eg), Some(StoreyView::Hidden));
    }

    #[test]
    fn ghost_follows_the_layer_switches() {
        let mut d = two_storeys();
        d.ensure_layer("Walls");
        let mut c = Common { layer: "Walls".into(), ..Common::default() };
        c.set_storey(Some(2));
        let h = d.add(&Space::Model, c, EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) })).unwrap_or_default();
        d.ghost_storey = Some(2);
        assert!(d.entity(h).is_some_and(|e| d.is_ghost(e)));
        if let Some(l) = d.layers.iter_mut().find(|l| l.name == "Walls") {
            l.on = false;
        }
        assert!(d.entity(h).is_some_and(|e| !d.is_ghost(e)), "walls off: no ghost walls either");
    }

    #[test]
    fn no_storeys_means_no_filter() {
        let mut d = Drawing::new_metric();
        let h = line(&mut d, Some(5));
        assert!(d.entity(h).is_some_and(|e| d.storey_view(e) == StoreyView::Current && d.is_visible(e)));
        assert!(!d.assign_storeys());
    }

    #[test]
    fn assign_storeys_tags_new_and_foreign_objects_only() {
        let mut d = two_storeys();
        let og = line(&mut d, Some(2));
        let none = line(&mut d, None);
        let unknown = line(&mut d, Some(99));
        d.storeys.current = 2;
        assert!(d.assign_storeys());
        let of = |d: &Drawing, h| d.entity(h).and_then(|e| e.common.storey());
        assert_eq!(of(&d, og), Some(2));
        assert_eq!(of(&d, none), Some(2));
        assert_eq!(of(&d, unknown), Some(2));
        assert!(!d.assign_storeys(), "nothing left to assign");
    }

    #[test]
    fn sanitize_drops_duplicates_and_fixes_current() {
        let mut s = Storeys {
            list: vec![
                Storey { id: 1, name: " EG ".into() },
                Storey { id: 1, name: "dup id".into() },
                Storey { id: 2, name: "eg".into() },
                Storey { id: 3, name: "   ".into() },
                Storey { id: 4, name: "OG1".into() },
            ],
            current: 42,
            ..Storeys::default()
        };
        s.sanitize();
        assert_eq!(s.list, vec![Storey { id: 1, name: "EG".into() }, Storey { id: 4, name: "OG1".into() }]);
        assert_eq!(s.current, 1);
        assert_eq!(s.next_id(), Some(5));
    }

    #[test]
    fn every_storey_group_goes_when_the_storey_is_set() {
        let mut c = Common {
            xdata: vec![
                RawTag { code: 1001, value: STOREY_APP.into() },
                RawTag { code: 1071, value: "1".into() },
                RawTag { code: 1001, value: "ACME".into() },
                RawTag { code: 1001, value: STOREY_APP.into() },
                RawTag { code: 1071, value: "2".into() },
            ],
            ..Common::default()
        };
        c.set_storey(Some(3));
        assert_eq!(c.storey(), Some(3));
        assert_eq!(c.xdata.iter().filter(|t| t.value == STOREY_APP).count(), 1);
    }

    #[test]
    fn ids_never_come_back_and_stay_in_dxf_range() {
        let mut s =
            Storeys { list: vec![Storey { id: 1, name: "EG".into() }, Storey { id: 2, name: "OG".into() }], current: 1, ..Storeys::default() };
        s.took(2);
        s.list.retain(|x| x.id != 2);
        assert_eq!(s.next_id(), Some(3), "the deleted storey's id is not handed out again");
        let mut top = Storeys { list: vec![Storey { id: MAX_STOREY_ID, name: "A".into() }], current: MAX_STOREY_ID, ..Storeys::default() };
        top.sanitize();
        assert_eq!(top.next_id(), Some(1), "past the top: the first free id, never a duplicate");
        let mut wild =
            Storeys { list: vec![Storey { id: u32::MAX, name: "A".into() }, Storey { id: 0, name: "B".into() }], current: 0, ..Storeys::default() };
        wild.sanitize();
        assert!(wild.list.is_empty(), "ids outside 1..=i32::MAX are not kept");
    }

    #[test]
    fn names_compare_without_case_in_every_script() {
        let s = Storeys { list: vec![Storey { id: 1, name: "Ärger".into() }], current: 1, ..Storeys::default() };
        assert!(s.by_name("ärger").is_some());
        assert!(s.by_name(" ÄRGER ").is_some());
    }

    #[test]
    fn the_current_storey_is_never_the_ghost() {
        let mut d = two_storeys();
        d.ghost_storey = Some(1);
        d.fix_ghost();
        assert_eq!(d.ghost_storey, None);
        d.ghost_storey = Some(2);
        d.fix_ghost();
        assert_eq!(d.ghost_storey, Some(2));
        d.ghost_storey = Some(9);
        d.fix_ghost();
        assert_eq!(d.ghost_storey, None, "a storey that doesn't exist");
    }

    #[test]
    fn numbers_count_from_the_start_storey() {
        let mut d = Drawing::new_metric();
        assert!(d.apply_storey_template(&StoreyTemplate::default()));
        let st = &d.storeys;
        let num = |name: &str| st.by_name(name).and_then(|x| st.number(x.id));
        assert_eq!(num("EG"), Some(0));
        assert_eq!(num("DG"), Some(3));
        assert_eq!(num("2.UG"), Some(-2));
        assert_eq!(st.current, st.home, "a new drawing is on its start storey");
        assert_eq!(st.find("-1").map(|x| x.name.as_str()), Some("1.UG"));
        assert_eq!(st.find("0").map(|x| x.name.as_str()), Some("EG"));
        assert_eq!(st.find("1.og").map(|x| x.name.as_str()), Some("1.OG"), "a name wins over a number");
        assert!(st.find("7").is_none());
        assert!(!d.apply_storey_template(&StoreyTemplate::blueprints()), "only on a drawing without storeys");
    }

    #[test]
    fn a_drawing_opens_on_its_start_storey() {
        let mut d = Drawing::new_metric();
        d.apply_storey_template(&StoreyTemplate::default());
        d.storeys.current = d.storeys.by_name("DG").map_or(0, |x| x.id);
        d.ghost_storey = Some(d.storeys.home);
        d.go_home();
        assert_eq!(d.storeys.current, d.storeys.home);
        assert_eq!(d.ghost_storey, None, "the start storey is not its own blueprint");
    }

    #[test]
    fn a_setup_never_reuses_the_ids_of_deleted_storeys() {
        let mut d = Drawing::new_metric();
        // Storeys 1..6 were made and deleted again: the next id is 7, and a blueprint is left over.
        d.storeys.next = 7;
        d.ghost_storey = Some(2);
        assert!(d.apply_storey_template(&StoreyTemplate::default()));
        assert_eq!(d.storeys.list.iter().map(|x| x.id).min(), Some(7));
        assert_eq!(d.ghost_storey, None);
        // No room left for six storeys: nothing changes.
        let mut full = Drawing::new_metric();
        full.storeys.next = MAX_STOREY_ID - 2;
        assert!(!full.apply_storey_template(&StoreyTemplate::default()));
        assert!(!full.storeys.active());
    }
}
