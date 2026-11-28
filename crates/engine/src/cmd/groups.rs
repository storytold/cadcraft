//! Groups: GROUP, UNGROUP, GROUPEDIT, the group listing, and the post-command pass that drops
//! members which no longer exist (an erased member leaves its groups; an empty group goes away).
//!
//! A group is a named (or unnamed, `*A1`, `*A2`…) ordered set of objects in the drawing's
//! `groups`, saved as GROUP objects in the ACAD_GROUP dictionary. The JSON forms never prompt;
//! typed, the commands ask as AutoCAD's command-line versions do.

use std::sync::Arc;

use cadcraft_doc::{Drawing, Group, Handle};
use serde_json::{Value, json};

use super::machines::{SelOutcome, SelectPhase};
use super::{CommandSpec, bad, bool_or, str_param, targets};

use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

mod dash;
mod pick;

pub(crate) use pick::expand;

/// The most groups one drawing holds, and the most names one call lists.
const MAX_GROUPS: usize = 100_000;

pub fn specs() -> Vec<CommandSpec> {
    let mut v = vec![
        CommandSpec::new("group", "Group", run_group)
            .menu(&["Tools", "Group"])
            .alias(&["g"])
            .params("{name? (default: an unnamed group *A1, *A2…), description?, handles? (default the selection), selectable?: bool (default true)} → {name, members}")
            .interactive(|_| Ok(Box::new(GroupM::default()))),
        CommandSpec::new("ungroup", "Ungroup", run_ungroup)
            .menu(&["Tools", "Ungroup"])
            .params("{name?: \"G1\" | \"G1,G2\" | [names]} | {handles?} (default the selection: every group holding one of the objects) → {ungrouped: [names]}")
            .interactive(|_| Ok(Box::new(UngroupM::default()))),
        CommandSpec::new("groupedit", "Group Edit", run_groupedit)
            .params("{name, add?: [handles], remove?: [handles], rename?: new name} → {name, members}. Removing every member deletes the group.")
            .interactive(|_| Ok(Box::new(GroupEditM::default()))),
        CommandSpec::new("groups.list", "List Groups", run_list)
            .params("{} → {groups: [{name, description, selectable, unnamed, members: [handles]}]}")
            .noundo(),
    ];
    v.extend(dash::specs());
    v.extend(pick::specs());
    v
}

// ---------------- model helpers ----------------

/// A name a new or renamed group may take: not empty, not unnamed-style (`*…`), no characters
/// that are special in names or wildcards.
pub(crate) fn valid_name(n: &str) -> bool {
    !n.trim().is_empty() && n.len() <= 255 && !n.starts_with('*') && !n.chars().any(|c| c.is_control() || "<>/\\\":;?*|,=`".contains(c))
}

/// Index of the group called `name` (case-insensitive).
pub(crate) fn find(d: &Drawing, name: &str) -> Option<usize> {
    let n = name.trim();
    d.groups.iter().position(|g| g.name.eq_ignore_ascii_case(n))
}

/// The groups holding `h`, most recently created first.
pub(crate) fn groups_of(d: &Drawing, h: Handle) -> Vec<usize> {
    d.groups.iter().enumerate().rev().filter(|(_, g)| g.members.contains(&h)).map(|(i, _)| i).collect()
}

/// The first free unnamed group name: `*A1`, `*A2`…
fn next_unnamed(d: &Drawing) -> String {
    let used: std::collections::HashSet<String> = d.groups.iter().map(|g| g.name.to_ascii_uppercase()).collect();
    (1..=d.groups.len() + 1).map(|n| format!("*A{n}")).find(|n| !used.contains(n)).unwrap_or_else(|| format!("*A{}", d.groups.len() + 1))
}

/// `hs` without repeats and without handles of objects that don't exist.
fn existing(d: &Drawing, hs: &[Handle]) -> Vec<Handle> {
    let mut seen = std::collections::HashSet::new();
    hs.iter().copied().filter(|h| d.entity(*h).is_some() && seen.insert(*h)).collect()
}

/// Make a group of `hs`. `name` None (or empty) makes an unnamed group. Returns its name.
pub(crate) fn create(s: &mut Session, name: Option<&str>, description: &str, hs: &[Handle], selectable: bool) -> Result<String> {
    let d = s.doc()?;
    let members = existing(d, hs);
    if members.is_empty() {
        return Err(bad("group", "no objects to group"));
    }
    if d.groups.len() >= MAX_GROUPS {
        return Err(bad("group", "the drawing has too many groups"));
    }
    let name = match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) if !valid_name(n) => return Err(bad("group", format!("\"{n}\" is not a valid group name"))),
        Some(n) if find(d, n).is_some() => return Err(bad("group", format!("a group named \"{n}\" already exists"))),
        Some(n) => n.to_string(),
        None => next_unnamed(d),
    };
    s.doc_mut()?.groups.push(Group { name: name.clone(), description: description.to_string(), selectable, members });
    Ok(name)
}

/// Remove the groups at `idx`; their objects stay. Returns the names removed.
pub(crate) fn ungroup(s: &mut Session, idx: &[usize]) -> Result<Vec<String>> {
    let names: Vec<String> = idx.iter().filter_map(|i| s.doc().ok()?.groups.get(*i).map(|g| g.name.clone())).collect();
    if names.is_empty() {
        return Ok(names);
    }
    let d = s.doc_mut()?;
    let mut i = 0;
    d.groups.retain(|_| {
        let keep = !idx.contains(&i);
        i += 1;
        keep
    });
    Ok(names)
}

/// Add `hs` (existing objects not already members) to the group at `i`. Returns how many joined.
pub(crate) fn add_members(s: &mut Session, i: usize, hs: &[Handle]) -> Result<usize> {
    let add: Vec<Handle> = {
        let d = s.doc()?;
        let g = d.groups.get(i).ok_or_else(|| bad("groupedit", "no such group"))?;
        existing(d, hs).into_iter().filter(|h| !g.members.contains(h)).collect()
    };
    if !add.is_empty()
        && let Some(g) = s.doc_mut()?.groups.get_mut(i)
    {
        g.members.extend(&add);
    }
    Ok(add.len())
}

/// Remove `hs` from the group at `i`; a group left with no members is deleted. Returns how many
/// left and whether the group was deleted.
pub(crate) fn remove_members(s: &mut Session, i: usize, hs: &[Handle]) -> Result<(usize, bool)> {
    let g = s.doc()?.groups.get(i).ok_or_else(|| bad("groupedit", "no such group"))?;
    let n = g.members.iter().filter(|h| hs.contains(h)).count();
    if n == 0 {
        return Ok((0, false));
    }
    let d = s.doc_mut()?;
    let empty = match d.groups.get_mut(i) {
        Some(g) => {
            g.members.retain(|h| !hs.contains(h));
            g.members.is_empty()
        }
        None => false,
    };
    if empty {
        d.groups.remove(i);
    }
    Ok((n, empty))
}

/// Rename the group at `i` (an unnamed group becomes a named one).
pub(crate) fn rename(s: &mut Session, i: usize, new: &str) -> Result<()> {
    let new = new.trim();
    if !valid_name(new) {
        return Err(bad("groupedit", format!("\"{new}\" is not a valid group name")));
    }
    let d = s.doc()?;
    if find(d, new).is_some_and(|j| j != i) {
        return Err(bad("groupedit", format!("a group named \"{new}\" already exists")));
    }
    if let Some(g) = s.doc_mut()?.groups.get_mut(i) {
        g.name = new.to_string();
    }
    Ok(())
}

/// One line per group for `?`: name, description, members.
fn list_lines(d: &Drawing) -> Vec<String> {
    if d.groups.is_empty() {
        return vec!["No groups defined.".into()];
    }
    d.groups
        .iter()
        .take(MAX_GROUPS)
        .map(|g| {
            let n = g.members.iter().filter(|h| d.entity(**h).is_some()).count();
            let sel = if g.selectable { "selectable" } else { "not selectable" };
            if g.description.is_empty() {
                format!("{}: {n} object(s), {sel}", g.name)
            } else {
                format!("{}: {}, {n} object(s), {sel}", g.name, g.description)
            }
        })
        .collect()
}

fn echo_list(s: &mut Session) -> Result<()> {
    for l in list_lines(s.doc()?) {
        s.echo(l);
    }
    Ok(())
}

/// The post-command pass: members that no longer exist (erased, moved into a block) leave their
/// groups, and groups left empty are deleted. Cheap when the drawing has no groups or the
/// command changed nothing.
pub(crate) fn after_command(s: &mut Session, before: Option<&Arc<Drawing>>) {
    let Ok(st) = s.state_mut() else { return };
    if st.doc.groups.is_empty() || before.is_some_and(|b| Arc::ptr_eq(b, &st.doc)) {
        return;
    }
    let d: &Drawing = &st.doc;
    let stale = d.groups.iter().any(|g| g.members.is_empty() || g.members.iter().any(|h| d.entity(*h).is_none()));
    if !stale {
        return;
    }
    let groups: Vec<Group> = d
        .groups
        .iter()
        .map(|g| Group { members: g.members.iter().copied().filter(|h| d.entity(*h).is_some()).collect(), ..g.clone() })
        .filter(|g| !g.members.is_empty())
        .collect();
    Arc::make_mut(&mut st.doc).groups = groups;
}

// ---------------- JSON forms ----------------

/// Handles under `key` (hex strings or numbers); absent is empty.
fn handles_at(cmd: &str, p: &Value, key: &str) -> Result<Vec<Handle>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(a)) => a
            .iter()
            .take(MAX_GROUPS * 10)
            .map(|v| {
                v.as_str()
                    .and_then(Handle::parse_hex)
                    .or_else(|| v.as_u64().map(Handle))
                    .ok_or_else(|| bad(cmd, format!("`{key}`: {v} is not an object handle")))
            })
            .collect(),
        Some(v) => Err(bad(cmd, format!("`{key}` must be an array of object handles, got {v}"))),
    }
}

fn run_group(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let name = create(s, str_param(p, "name"), str_param(p, "description").unwrap_or_default(), &hs, bool_or(p, "selectable", true))?;
    let n = s.doc()?.groups.iter().find(|g| g.name == name).map_or(0, |g| g.members.len());
    Ok(json!({ "name": name, "members": n }))
}

fn run_ungroup(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let names: Option<Vec<String>> = match p.get("name") {
        None | Some(Value::Null) => None,
        Some(Value::String(n)) => Some(n.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()),
        Some(Value::Array(a)) => Some(
            a.iter()
                .take(MAX_GROUPS)
                .map(|v| v.as_str().map(|x| x.trim().to_string()).ok_or_else(|| bad("ungroup", "`name` must hold group names")))
                .collect::<Result<_>>()?,
        ),
        Some(v) => return Err(bad("ungroup", format!("`name` must be a group name or an array of names, got {v}"))),
    };
    let mut idx: Vec<usize> = match names {
        Some(names) => names.iter().map(|n| find(d, n).ok_or_else(|| bad("ungroup", format!("no group named \"{n}\"")))).collect::<Result<_>>()?,
        None => targets(s, p)?.iter().flat_map(|h| groups_of(d, *h)).collect(),
    };
    idx.sort_unstable();
    idx.dedup();
    if idx.is_empty() {
        return Err(bad("ungroup", "the objects are not in a group"));
    }
    let names = ungroup(s, &idx)?;
    Ok(json!({ "ungrouped": names }))
}

fn run_groupedit(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("groupedit", "`name` is required"))?;
    let i = find(s.doc()?, name).ok_or_else(|| bad("groupedit", format!("no group named \"{name}\"")))?;
    let add = handles_at("groupedit", p, "add")?;
    let remove = handles_at("groupedit", p, "remove")?;
    if let Some(h) = add.iter().find(|h| s.doc().is_ok_and(|d| d.entity(**h).is_none())) {
        return Err(bad("groupedit", format!("`add`: no object with handle {}", h.hex())));
    }
    let new_name = str_param(p, "rename");
    // Check the new name before changing anything.
    if let Some(n) = new_name {
        let n = n.trim();
        if !valid_name(n) || find(s.doc()?, n).is_some_and(|j| j != i) {
            return Err(bad("groupedit", format!("\"{n}\" is not a valid new group name, or a group of that name exists")));
        }
    }
    add_members(s, i, &add)?;
    let (_, deleted) = remove_members(s, i, &remove)?;
    if deleted {
        return Ok(json!({ "name": name, "members": 0, "deleted": true }));
    }
    if let Some(n) = new_name {
        rename(s, i, n)?;
    }
    let g = s.doc()?.groups.get(i).ok_or_else(|| bad("groupedit", "no such group"))?;
    Ok(json!({ "name": g.name, "members": g.members.len() }))
}

fn run_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let groups: Vec<Value> = d
        .groups
        .iter()
        .map(|g| {
            let members: Vec<String> = g.members.iter().filter(|h| d.entity(**h).is_some()).map(|h| h.hex()).collect();
            json!({ "name": g.name, "description": g.description, "selectable": g.selectable, "unnamed": g.name.starts_with('*'), "members": members })
        })
        .collect();
    Ok(json!({ "groups": groups }))
}

// ---------------- prompts ----------------

/// The keyword typed or picked, in lower case.
fn kw(i: &Input) -> Option<String> {
    match i {
        Input::Keyword(k) => Some(k.to_ascii_lowercase()),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum GroupStage {
    #[default]
    Select,
    Name,
    Description,
}

/// GROUP: "Select objects or [Name/Description]:", ending with Enter. With a pickfirst selection
/// the unnamed group is made at once.
#[derive(Default)]
struct GroupM {
    stage: GroupStage,
    name: Option<String>,
    description: String,
    sel: SelectPhase,
}

impl GroupM {
    fn finish(&mut self, s: &mut Session, hs: &[Handle]) -> Result<Step> {
        match create(s, self.name.as_deref(), &self.description, hs, true) {
            Ok(n) if n.starts_with('*') => s.echo(format!("Unnamed group {n} created with {} object(s).", hs.len())),
            Ok(n) => s.echo(format!("Group \"{n}\" created with {} object(s).", hs.len())),
            Err(e) => s.echo(e.to_string()),
        }
        Ok(Step::Done)
    }
}

impl Interactive for GroupM {
    fn name(&self) -> &'static str {
        "GROUP"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return self.finish(s, &hs);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            GroupStage::Select if self.sel.removing => self.sel.prompt(),
            GroupStage::Select => Prompt::new("Select objects", Accept::SELECT).kw(&["Name", "Description"]),
            GroupStage::Name => Prompt::new("Enter a group name", Accept::TEXT).kw(&["?"]),
            GroupStage::Description => Prompt::new("Enter group description", Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.stage {
            GroupStage::Select => match kw(&i).as_deref() {
                Some("name") => {
                    self.stage = GroupStage::Name;
                    Ok(Step::Continue)
                }
                Some("description") => {
                    self.stage = GroupStage::Description;
                    Ok(Step::Continue)
                }
                _ => match self.sel.feed(s, &i)? {
                    SelOutcome::More => Ok(Step::Continue),
                    SelOutcome::Empty => Ok(Step::Done),
                    SelOutcome::Done(hs) => {
                        let step = self.finish(s, &hs);
                        s.set_selection(Vec::new());
                        step
                    }
                },
            },
            GroupStage::Name => {
                match &i {
                    Input::Keyword(k) if k == "?" => echo_list(s)?,
                    Input::Text(t) => {
                        let n = t.trim();
                        if !valid_name(n) {
                            s.echo(format!("\"{n}\" is not a valid group name."));
                        } else if find(s.doc()?, n).is_some() {
                            s.echo(format!("A group named \"{n}\" already exists."));
                        } else {
                            self.name = Some(n.to_string());
                            self.stage = GroupStage::Select;
                        }
                    }
                    // Enter keeps the group unnamed.
                    Input::Enter => self.stage = GroupStage::Select,
                    _ => {}
                }
                Ok(Step::Continue)
            }
            GroupStage::Description => {
                match &i {
                    Input::Text(t) => self.description = t.trim().to_string(),
                    Input::Enter => self.description.clear(),
                    _ => return Ok(Step::Continue),
                }
                self.stage = GroupStage::Select;
                Ok(Step::Continue)
            }
        }
    }
}

/// The group a "Select group or [Name]" pick means: the most recent group of the first picked
/// object that is in one.
fn picked_group(d: &Drawing, hs: &[Handle]) -> Option<usize> {
    hs.iter().find_map(|h| groups_of(d, *h).first().copied())
}

/// UNGROUP: "Select group or [Name]:". With a pickfirst selection, every group holding a
/// selected object is ungrouped at once.
#[derive(Default)]
struct UngroupM {
    by_name: bool,
}

impl UngroupM {
    fn done(s: &mut Session, idx: &[usize]) -> Result<Step> {
        for n in ungroup(s, idx)? {
            s.echo(format!("Group {n} ungrouped."));
        }
        Ok(Step::Done)
    }
}

impl Interactive for UngroupM {
    fn name(&self) -> &'static str {
        "UNGROUP"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let pre = s.selection();
        if pre.is_empty() || !s.settings.pickfirst {
            return Ok(Step::Continue);
        }
        let d = s.doc()?;
        let mut idx: Vec<usize> = pre.iter().flat_map(|h| groups_of(d, *h)).collect();
        idx.sort_unstable();
        idx.dedup();
        if idx.is_empty() {
            return Ok(Step::Continue);
        }
        s.set_selection(Vec::new());
        Self::done(s, &idx)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.by_name {
            Prompt::new("Enter a group name", Accept::TEXT).kw(&["?"])
        } else {
            Prompt::new("Select group", Accept::SELECT).kw(&["Name"])
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.by_name {
            match &i {
                Input::Keyword(k) if k == "?" => echo_list(s)?,
                Input::Text(t) => match find(s.doc()?, t) {
                    Some(g) => return Self::done(s, &[g]),
                    None => s.echo(format!("No group named \"{}\".", t.trim())),
                },
                Input::Enter => return Ok(Step::Done),
                _ => {}
            }
            return Ok(Step::Continue);
        }
        match (&i, kw(&i).as_deref()) {
            (_, Some("name")) => self.by_name = true,
            (Input::Pick(hs), _) => match picked_group(s.doc()?, hs) {
                Some(g) => {
                    s.set_selection(Vec::new());
                    return Self::done(s, &[g]);
                }
                None => {
                    s.set_selection(Vec::new());
                    s.echo(if hs.is_empty() { "0 found" } else { "The object is not a member of any group." });
                }
            },
            (Input::Enter, _) => return Ok(Step::Done),
            _ => {}
        }
        Ok(Step::Continue)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum EditStage {
    #[default]
    Pick,
    Name,
    Option,
    Add,
    Remove,
    Rename,
}

/// GROUPEDIT: pick a group (or name it), then Add objects, Remove objects or REName.
#[derive(Default)]
struct GroupEditM {
    stage: EditStage,
    /// The group being edited, by name (indices move when groups are deleted).
    group: String,
    sel: SelectPhase,
}

impl GroupEditM {
    fn index(&self, s: &Session) -> Result<usize> {
        find(s.doc()?, &self.group).ok_or_else(|| bad("groupedit", format!("no group named \"{}\"", self.group)))
    }
    fn chosen(&mut self, s: &Session, i: usize) -> Result<()> {
        self.group = s.doc()?.groups.get(i).map(|g| g.name.clone()).unwrap_or_default();
        self.stage = EditStage::Option;
        Ok(())
    }
}

impl Interactive for GroupEditM {
    fn name(&self) -> &'static str {
        "GROUPEDIT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            EditStage::Pick => Prompt::new("Select group", Accept::SELECT).kw(&["Name"]),
            EditStage::Name => Prompt::new("Enter a group name", Accept::TEXT).kw(&["?"]),
            EditStage::Option => Prompt::new("Enter an option", super::curves::KW).kw(&["Add objects", "Remove objects", "REName"]),
            EditStage::Add if self.sel.removing => self.sel.prompt(),
            EditStage::Add => Prompt::new("Select objects to add to the group", Accept::SELECT),
            EditStage::Remove => Prompt::new("Select objects to remove from the group", Accept::SELECT),
            EditStage::Rename => Prompt::new("Enter a new name for the group", Accept::TEXT).kw(&["?"]).default(self.group.clone()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.stage {
            EditStage::Pick => match (&i, kw(&i).as_deref()) {
                (_, Some("name")) => self.stage = EditStage::Name,
                (Input::Pick(hs), _) => {
                    let g = picked_group(s.doc()?, hs);
                    s.set_selection(Vec::new());
                    match g {
                        Some(g) => self.chosen(s, g)?,
                        None => s.echo(if hs.is_empty() { "0 found" } else { "The object is not a member of any group." }),
                    }
                }
                (Input::Enter, _) => return Ok(Step::Done),
                _ => {}
            },
            EditStage::Name => match &i {
                Input::Keyword(k) if k == "?" => echo_list(s)?,
                Input::Text(t) => match find(s.doc()?, t) {
                    Some(g) => self.chosen(s, g)?,
                    None => s.echo(format!("No group named \"{}\".", t.trim())),
                },
                Input::Enter => return Ok(Step::Done),
                _ => {}
            },
            EditStage::Option => match kw(&i).as_deref() {
                Some("add objects") => self.stage = EditStage::Add,
                Some("remove objects") => self.stage = EditStage::Remove,
                Some("rename") => self.stage = EditStage::Rename,
                _ if i == Input::Enter => return Ok(Step::Done),
                _ => s.echo("Invalid option."),
            },
            EditStage::Add | EditStage::Remove => match self.sel.feed(s, &i)? {
                SelOutcome::More => {}
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    s.set_selection(Vec::new());
                    let g = self.index(s)?;
                    if self.stage == EditStage::Add {
                        let n = add_members(s, g, &hs)?;
                        s.echo(format!("{n} object(s) added to group {}.", self.group));
                    } else {
                        match remove_members(s, g, &hs)? {
                            (n, false) => s.echo(format!("{n} object(s) removed from group {}.", self.group)),
                            (_, true) => s.echo(format!("Group {} has no objects left and was deleted.", self.group)),
                        }
                    }
                    return Ok(Step::Done);
                }
            },
            EditStage::Rename => match &i {
                Input::Keyword(k) if k == "?" => echo_list(s)?,
                Input::Text(t) => {
                    let g = self.index(s)?;
                    match rename(s, g, t) {
                        Ok(()) => {
                            s.echo(format!("Group {} renamed to {}.", self.group, t.trim()));
                            return Ok(Step::Done);
                        }
                        Err(e) => s.echo(e.to_string()),
                    }
                }
                Input::Enter => return Ok(Step::Done),
                _ => {}
            },
        }
        Ok(Step::Continue)
    }
}
