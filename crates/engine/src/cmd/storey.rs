//! Storeys (see [`cadcraft_doc::Storeys`]): STOREY (programmatic sub-ids, typed prompts), the
//! ghost storey, STOREYCOPY and STOREYMOVE, and the passes that keep objects of other storeys
//! out of reach.
//!
//! Only the current storey can be selected and changed. Three things hold that line:
//! [`Drawing::is_visible`] leaves other storeys out of selection, picking and snapping (the ghost
//! is let back in for snapping only); [`super::targets`] refuses handles of other storeys from
//! scripts and the command line; and [`guard`] takes back any command that changed an object of
//! another storey anyway, except the drawing-wide table commands in [`DRAWING_WIDE`] (renaming
//! or merging a layer changes the objects on it on every storey, as it should).

use std::sync::Arc;

use cadcraft_color::Rgb;
use cadcraft_doc::{Drawing, Entity, Handle, Storey, Storeys};
use serde_json::{Value, json};

use super::machines::{SelOutcome, SelectPhase};
use super::{CommandSpec, bad, bool_or, find_command, str_param, targets};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Commands that may change objects on every storey: they act on the drawing's tables (layers,
/// styles, blocks), and the objects follow, or they repair or swap the whole drawing. Every other
/// command that changes an object of a storey other than the current one is taken back.
pub(crate) const DRAWING_WIDE: &[&str] = &[
    // Typed LAYER / -LAYER renames, deletes and merges through its own prompts.
    "layer",
    "layer.set",
    "layer.delete",
    "laymrg",
    "laydel",
    "rename",
    "audit",
    "recover",
    "attsync",
    "battman",
    "bedit",
    "bsave",
    "bsaveas",
    "bclose",
    "refclose",
    "xref",
    "-xref",
    "xbind",
    "style",
    "style.rename",
    "dimstyle",
    "dimstyle.rename",
    "linetype",
    "-linetype",
    "units",
    "scalelistedit",
];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("storey", "Storeys", run_list)
            .menu(&["Format", "Storeys", "Storeys"])
            .alias(&["-storey", "story", "-story"])
            .params("{} → storeys (bottom to top), current, ghost, ghostColor (typed STOREY: a storey name makes it current; New, Rename, Delete, Ghost)")
            .noundo()
            .interactive(|_| Ok(Box::new(StoreyM::default()))),
        CommandSpec::new("storey.new", "New Storey", run_new)
            .menu(&["Format", "Storeys", "New Storey"])
            .params("{name, below?: bool (default: above the current storey), index?: position from the bottom, current?: bool (default true)} — the first storey takes every object already drawn"),
        CommandSpec::new("storey.rename", "Rename Storey", run_rename).params("{name, newName}"),
        CommandSpec::new("storey.home", "Start Storey", run_home)
            .params("{name} — number 0: the storeys above count up from it, those below down; the drawing opens on it"),
        CommandSpec::new("storey.template", "Storeys from Setup", run_template)
            .params("{names?: [top to bottom], start?} (default: the user's storey setup) — a drawing without storeys gets them, everything drawn on the start storey"),
        CommandSpec::new("storey.delete", "Delete Storey", run_delete)
            .params("{name} (only an empty storey; deleting the last one turns storeys off)"),
        CommandSpec::new("storey.order", "Reorder Storey", run_order).params("{name, index} (0 = bottom)"),
        CommandSpec::new("storey.current", "Go to Storey", run_current).params("{name}").noundo(),
        CommandSpec::new("storey.up", "Storey Above", |s, _| step(s, 1)).menu(&["Format", "Storeys", "Storey Above"]).key("Cmd+PageUp").noundo(),
        CommandSpec::new("storey.down", "Storey Below", |s, _| step(s, -1)).menu(&["Format", "Storeys", "Storey Below"]).key("Cmd+PageDown").noundo(),
        CommandSpec::new("storey.ghost", "Blueprint", run_ghost)
            .menu(&["Format", "Storeys", "Blueprint"])
            .params("{name|null (off), color?: \"#rrggbb\"} — shown beneath the current storey, snappable only")
            .noundo(),
        CommandSpec::new("storey.import", "Import Plan as Storey", run_import)
            .menu(&["Format", "Storeys", "Import Plan as Storey"])
            .params("{file: DXF/DWG path, name? (default: the file name), ghost?: bool (default true)} — a new storey (above the current one) holding the file's model space at its own coordinates, its layers as they are; shown as the ghost"),
        CommandSpec::new("storey.ghost.toggle", "Blueprint On/Off", run_ghost_toggle)
            .menu(&["Format", "Storeys", "Blueprint On/Off"])
            .key("Cmd+Shift+G")
            .params("{} — off, or on again: the last ghost storey, else the one below the current storey")
            .noundo(),
        CommandSpec::new("storey.ghost.up", "Blueprint Storey Above", |s, _| ghost_step(s, 1)).params("{} — the ghost moves one storey up (past the current one)").noundo(),
        CommandSpec::new("storey.ghost.down", "Blueprint Storey Below", |s, _| ghost_step(s, -1)).params("{} — the ghost moves one storey down (past the current one)").noundo(),
        CommandSpec::new("storeycopy", "Copy to Storey", run_copy)
            .params("{handles?, to: name | [names]} (copies in place)")
            .interactive(|_| Ok(Box::new(ToStoreyM::new("storeycopy", "STOREYCOPY")))),
        CommandSpec::new("storeymove", "Move to Storey", run_move)
            .params("{handles?, to: name}")
            .interactive(|_| Ok(Box::new(ToStoreyM::new("storeymove", "STOREYMOVE")))),
    ]
}

// ---------------- post-command passes ----------------

/// After every command, last of the post-command passes: objects without a valid storey (made by
/// a script, a noundo command) get the current one. Only objects the command changed are looked at.
pub(crate) fn after_command(s: &mut Session, before: Option<&Arc<Drawing>>) {
    let Ok(st) = s.state() else { return };
    let d = &st.doc;
    if !d.storeys.active() || before.is_some_and(|b| Arc::ptr_eq(b, d)) {
        return;
    }
    let lost: Vec<Handle> = match before {
        Some(b) => Diff::of(b, d).after.into_iter().filter(|e| !valid(d, e)).map(|e| e.handle).collect(),
        None => d.model.iter().filter(|e| !valid(d, e)).map(|e| e.handle).collect(),
    };
    if lost.is_empty() {
        return;
    }
    if let Ok(st) = s.state_mut() {
        let d = Arc::make_mut(&mut st.doc);
        let current = d.storeys.current;
        for h in lost {
            d.model.modify(h, |e| e.common.set_storey(Some(current)));
        }
    }
}

fn valid(d: &Drawing, e: &Entity) -> bool {
    e.common.storey().is_some_and(|id| d.storeys.contains(id))
}

/// What a command changed in model space. Only the chunks the two drawings don't share are looked
/// at (copy-on-write: an edit copies its chunk), and inside them an object counts only when the
/// other side doesn't hold the very same object (a changed chunk also holds untouched ones).
struct Diff<'a> {
    /// Objects added or changed, as they are after.
    after: Vec<&'a Arc<Entity>>,
    /// Objects changed or removed, as they were before.
    before: Vec<&'a Arc<Entity>>,
}

impl<'a> Diff<'a> {
    fn of(before: &'a Drawing, after: &'a Drawing) -> Diff<'a> {
        use std::collections::HashSet;
        let keys = |d: &Drawing| d.model.chunk_slices().map(|(k, _)| k).collect::<HashSet<usize>>();
        let (kb, ka) = (keys(before), keys(after));
        let own = |d: &'a Drawing, other: &HashSet<usize>| -> Vec<&'a Arc<Entity>> {
            d.model.chunk_slices().filter(|(k, _)| !other.contains(k)).flat_map(|(_, ents)| ents.iter()).collect()
        };
        let (b, a) = (own(before, &ka), own(after, &kb));
        let ptr = |e: &&Arc<Entity>| Arc::as_ptr(e) as usize;
        let (pb, pa): (HashSet<usize>, HashSet<usize>) = (b.iter().map(ptr).collect(), a.iter().map(ptr).collect());
        Diff { after: a.into_iter().filter(|e| !pb.contains(&ptr(e))).collect(), before: b.into_iter().filter(|e| !pa.contains(&ptr(e))).collect() }
    }
}

/// After a command and all its post-command passes (with the drawing it started from): take it
/// back when it changed or removed an object of a storey that was not current before or after it
/// ran, and return why; otherwise give the objects it added the current storey (an exploded block
/// or a paste brings the storeys its parts were drawn on; STOREYCOPY and STOREY.IMPORT alone place
/// objects elsewhere). `uid` is the document the command started in when known.
pub(crate) fn guard(s: &mut Session, before: &Arc<Drawing>, uid: Option<u64>, id: &str) -> Option<String> {
    if !before.storeys.active() || !find_command(id).is_some_and(|c| c.undoable) {
        return None;
    }
    let st = s.state_mut().ok()?;
    if Arc::ptr_eq(before, &st.doc) || uid.is_some_and(|u| u != st.uid) || st.block_edit.is_some() || !st.doc.storeys.active() {
        return None;
    }
    let current = st.doc.storeys.current;
    let (touches, new, linked) = {
        let d: &Drawing = &st.doc;
        let diff = Diff::of(before, d);
        let open = [before.storeys.current, current];
        let touches = diff.before.iter().any(|e| !before.storey_of(e).is_some_and(|id| open.contains(&id)));
        // New: a handle the drawing never had, or one without a valid storey. An object brought back
        // under its old handle (OOPS) keeps the storey it had.
        let new: Vec<Handle> = if PLACES_ON_STOREYS.contains(&id) {
            Vec::new()
        } else {
            diff.after
                .iter()
                .filter(|e| !before.model.contains(e.handle) && (e.handle.0 >= before.handseed || !valid(d, e)) && e.common.storey() != Some(current))
                .map(|e| e.handle)
                .collect()
        };
        let linked: Vec<Handle> = diff.after.iter().filter(|e| has_links(e)).map(|e| e.handle).collect();
        (touches, new, linked)
    };
    if !DRAWING_WIDE.contains(&id) && touches {
        st.doc = before.clone();
        return Some(format!(
            "{}: only objects on the current storey can be changed; objects on other storeys were left as they were.",
            id.to_ascii_uppercase()
        ));
    }
    if !new.is_empty() {
        let d = Arc::make_mut(&mut st.doc);
        for h in new {
            d.model.modify(h, |e| e.common.set_storey(Some(current)));
        }
    }
    // What the command made or changed keeps no links to other storeys (a dimension snapped to the
    // ghost is not associative to it).
    if !linked.is_empty() {
        cut_links(Arc::make_mut(&mut st.doc), &linked);
    }
    None
}

fn has_links(e: &Entity) -> bool {
    match &e.kind {
        cadcraft_doc::EntityKind::Dimension(dm) => !dm.assoc.is_empty(),
        cadcraft_doc::EntityKind::Hatch(h) => h.loops.iter().any(|l| !l.sources.is_empty()),
        _ => false,
    }
}

/// [`Drawing::cut_storey_links`] for each of `hs`; how many lost a link.
fn cut_links(d: &mut Drawing, hs: &[Handle]) -> usize {
    let mut n = 0;
    for h in hs {
        let Some(mut e) = d.model.get(*h).map(|e| (**e).clone()) else { continue };
        if d.cut_storey_links(&mut e) {
            d.model.replace(e);
            n += 1;
        }
    }
    n
}

/// Commands that create objects on storeys other than the current one on purpose. (LISP routines
/// are not undoable commands and skip [`guard`]; the commands they run pass it one by one,
/// `entmake` drops a storey it is given, and `entmod`/`entdel` refuse objects of other storeys.)
const PLACES_ON_STOREYS: &[&str] = &["storeycopy", "storey.import"];

/// Whether `h` may be acted on: it is on the current storey (or the drawing has no storeys, or it
/// is not in model space).
pub(crate) fn editable(d: &Drawing, h: Handle) -> bool {
    !d.storeys.active() || d.model.get(h).is_none_or(|e| d.storey_view(e) == cadcraft_doc::StoreyView::Current)
}

// ---------------- programmatic ----------------

fn storeys(s: &Session) -> Result<&Storeys> {
    Ok(&s.doc()?.storeys)
}

/// The storey `name` means: its name, else its number ("0" the start storey, "-1" the one below).
fn find<'a>(cmd: &str, st: &'a Storeys, name: &str) -> Result<&'a Storey> {
    st.find(name).ok_or_else(|| bad(cmd, format!("no storey `{}`", name.trim())))
}

/// The storey named `name`, by name only: commands that rename, reorder or delete never act on a
/// storey a mistyped name happens to number.
fn named<'a>(cmd: &str, st: &'a Storeys, name: &str) -> Result<&'a Storey> {
    st.by_name(name).ok_or_else(|| bad(cmd, format!("no storey `{}`", name.trim())))
}

fn name_param<'a>(cmd: &str, p: &'a Value, key: &str) -> Result<&'a str> {
    str_param(p, key).map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, format!("`{key}` (a storey name) is required")))
}

/// A storey name that can be stored: not empty, not too long, no control characters.
fn check_name(cmd: &str, name: &str) -> Result<String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > cadcraft_doc::MAX_STOREY_NAME || n.chars().any(char::is_control) {
        return Err(bad(cmd, format!("`{n}` is not a usable storey name")));
    }
    Ok(n.to_string())
}

fn run_list(s: &mut Session, _: &Value) -> Result<Value> {
    let d = s.doc()?;
    let mut count = std::collections::HashMap::<u32, usize>::new();
    for e in d.model.iter() {
        if let Some(id) = d.storey_of(e) {
            *count.entry(id).or_default() += 1;
        }
    }
    let list: Vec<Value> = d
        .storeys
        .list
        .iter()
        .map(|st| {
            json!({
                "name": st.name,
                "number": d.storeys.number(st.id),
                "start": st.id == d.storeys.home,
                "current": st.id == d.storeys.current,
                "ghost": d.ghost_storey == Some(st.id),
                "objects": count.get(&st.id).copied().unwrap_or(0),
            })
        })
        .collect();
    let ghost = d.ghost_storey.and_then(|g| d.storeys.get(g)).map(|g| g.name.clone());
    Ok(json!({
        "storeys": list,
        "current": d.storeys.current_storey().map(|c| c.name.clone()),
        "ghost": ghost,
        "ghostColor": hex(d.ghost_color),
    }))
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

fn parse_hex(t: &str) -> Option<Rgb> {
    let t = t.trim().strip_prefix('#')?;
    if t.len() != 6 || !t.is_ascii() {
        return None;
    }
    let v = u32::from_str_radix(t, 16).ok()?;
    Some(Rgb::from_u32(v))
}

fn run_new(s: &mut Session, p: &Value) -> Result<Value> {
    let name = check_name("storey.new", name_param("storey.new", p, "name")?)?;
    let below = bool_or(p, "below", false);
    let make_current = bool_or(p, "current", true);
    let st = storeys(s)?;
    if st.by_name(&name).is_some() {
        return Err(bad("storey.new", format!("a storey `{name}` already exists")));
    }
    if st.list.len() >= cadcraft_doc::MAX_STOREYS {
        return Err(bad("storey.new", "too many storeys"));
    }
    let first = !st.active();
    let id = st.next_id().ok_or_else(|| bad("storey.new", "no storey id left"))?;
    let at = match (p.get("index").and_then(Value::as_u64), st.index(st.current)) {
        (Some(i), _) => usize::try_from(i).unwrap_or(usize::MAX).min(st.list.len()),
        (None, Some(i)) if below => i,
        (None, Some(i)) => i + 1,
        (None, None) => st.list.len(),
    };
    let d = s.doc_mut()?;
    // Objects without a valid storey belong to the current one: settle that before the list
    // changes, or they would follow a new current storey (or take a new id they happen to carry).
    d.assign_storeys();
    d.storeys.list.insert(at.min(d.storeys.list.len()), Storey { id, name: name.clone() });
    d.storeys.took(id);
    if first {
        // The drawing so far is the first storey.
        d.storeys.current = id;
        d.assign_storeys();
    } else if make_current {
        d.storeys.current = id;
    }
    d.fix_ghost();
    if d.storeys.current == id {
        s.set_selection(Vec::new());
    }
    Ok(json!({ "name": name, "current": first || make_current }))
}

fn run_rename(s: &mut Session, p: &Value) -> Result<Value> {
    let name = name_param("storey.rename", p, "name")?;
    let new = check_name("storey.rename", name_param("storey.rename", p, "newName")?)?;
    let st = storeys(s)?;
    let id = named("storey.rename", st, name)?.id;
    if st.by_name(&new).is_some_and(|o| o.id != id) {
        return Err(bad("storey.rename", format!("a storey `{new}` already exists")));
    }
    let d = s.doc_mut()?;
    if let Some(x) = d.storeys.list.iter_mut().find(|x| x.id == id) {
        x.name = new.clone();
    }
    Ok(json!({ "name": new }))
}

fn run_home(s: &mut Session, p: &Value) -> Result<Value> {
    let name = name_param("storey.home", p, "name")?;
    let st = find("storey.home", storeys(s)?, name)?;
    let (id, name) = (st.id, st.name.clone());
    s.doc_mut()?.storeys.home = id;
    Ok(json!({ "home": name }))
}

fn run_template(s: &mut Session, p: &Value) -> Result<Value> {
    let t = match p.get("names").and_then(Value::as_array) {
        Some(a) => cadcraft_doc::StoreyTemplate {
            names: a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
            start: str_param(p, "start").unwrap_or_default().to_string(),
        },
        None => s.storey_template.clone().unwrap_or_default(),
    };
    if storeys(s)?.active() {
        return Err(bad("storey.template", "the drawing has storeys already"));
    }
    if !s.doc_mut()?.apply_storey_template(&t) {
        return Err(bad("storey.template", "the setup has no usable storey names"));
    }
    s.set_selection(Vec::new());
    Ok(json!({ "storeys": storeys(s)?.list.len() }))
}

fn run_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let name = name_param("storey.delete", p, "name")?;
    let d = s.doc()?;
    let id = named("storey.delete", &d.storeys, name)?.id;
    let n = d.model.iter().filter(|e| d.storey_of(e) == Some(id)).count();
    // The last storey goes with its objects kept: storeys are turned off, the drawing is one level.
    if n > 0 && d.storeys.list.len() > 1 {
        return Err(bad("storey.delete", format!("storey `{}` still has {n} object(s); erase or move them first", name.trim())));
    }
    let d = s.doc_mut()?;
    d.storeys.list.retain(|x| x.id != id);
    if d.storeys.home == id {
        d.storeys.home = 0;
    }
    if d.ghost_storey == Some(id) {
        d.ghost_storey = None;
    }
    if d.storeys.list.is_empty() {
        // Storeys off: the objects (on the deleted storey, so none) keep nothing behind. The ids
        // handed out stay used (an object restored later must not land on a new storey).
        d.storeys = Storeys { next: d.storeys.next, ..Storeys::default() };
        d.ghost_storey = None;
        for h in d.model.handles() {
            d.model.modify(h, |e| e.common.set_storey(None));
        }
    } else if d.storeys.current == id {
        d.storeys.current = d.storeys.list.first().map_or(0, |x| x.id);
        d.fix_ghost();
        s.set_selection(Vec::new());
    }
    Ok(json!({ "deleted": name.trim() }))
}

fn run_order(s: &mut Session, p: &Value) -> Result<Value> {
    let name = name_param("storey.order", p, "name")?;
    let index = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("storey.order", "`index` (0 = bottom) is required"))?;
    let st = storeys(s)?;
    let from = st.index(named("storey.order", st, name)?.id).unwrap_or(0);
    let d = s.doc_mut()?;
    let to = usize::try_from(index).unwrap_or(usize::MAX).min(d.storeys.list.len().saturating_sub(1));
    let x = d.storeys.list.remove(from);
    d.storeys.list.insert(to, x);
    Ok(json!({ "index": to }))
}

/// Make storey `id` current: the selection (objects of the storey left) is cleared.
/// A change of what is shown (storey, blueprint): no change to the drawing, so a drawing without
/// changes stays without changes.
fn view_change(s: &mut Session, f: impl FnOnce(&mut cadcraft_doc::Drawing)) -> Result<()> {
    let st = s.state_mut()?;
    let clean = st.block_edit.is_none() && !st.is_dirty();
    f(std::sync::Arc::make_mut(&mut st.doc));
    if clean {
        st.saved = st.doc.clone();
    }
    Ok(())
}

fn go(s: &mut Session, id: u32) -> Result<Value> {
    let d = s.doc()?;
    let name = d.storeys.get(id).map(|x| x.name.clone()).unwrap_or_default();
    if d.storeys.current != id {
        // No object is without a storey between commands (each command's new objects get one after
        // it, a drawing gets them as it opens), so the switch itself touches no object.
        // Each storey brings back its own blueprint; going to the blueprint storey ends it there:
        // the current storey is never its own blueprint.
        view_change(s, |d| d.switch_storey(id))?;
        s.set_selection(Vec::new());
        s.touch();
    }
    Ok(json!({ "current": name }))
}

fn run_current(s: &mut Session, p: &Value) -> Result<Value> {
    let name = name_param("storey.current", p, "name")?;
    let id = find("storey.current", storeys(s)?, name)?.id;
    go(s, id)
}

fn step(s: &mut Session, by: isize) -> Result<Value> {
    let st = storeys(s)?;
    if !st.active() {
        return Err(bad("storey", "the drawing has no storeys (STOREY New makes the first one)"));
    }
    let i = st.index(st.current).unwrap_or(0);
    let j = i.saturating_add_signed(by).min(st.list.len() - 1);
    if j == i {
        let msg = if by > 0 { "already on the top storey" } else { "already on the bottom storey" };
        return Ok(json!({ "current": st.list.get(i).map(|x| x.name.clone()), "message": msg }));
    }
    let id = st.list.get(j).map_or(st.current, |x| x.id);
    go(s, id)
}

fn run_ghost(s: &mut Session, p: &Value) -> Result<Value> {
    let color = match p.get("color") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_str().and_then(parse_hex).ok_or_else(|| bad("storey.ghost", format!("`color` must be \"#rrggbb\", got {v}")))?),
    };
    let ghost = match p.get("name") {
        None => s.doc()?.ghost_storey,
        Some(Value::Null) => None,
        Some(v) => {
            let name = v.as_str().map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad("storey.ghost", "`name` is a storey name or null"))?;
            let st = storeys(s)?;
            let id = find("storey.ghost", st, name)?.id;
            if id == st.current {
                return Err(bad("storey.ghost", "the current storey can't be its own ghost"));
            }
            Some(id)
        }
    };
    view_change(s, |d| {
        if d.ghost_storey.is_some() && ghost.is_none() {
            d.ghost_last = d.ghost_storey;
        }
        d.ghost_storey = ghost;
        if let Some(c) = color {
            d.ghost_color = c;
        }
    })?;
    s.touch();
    let d = s.doc()?;
    Ok(json!({ "ghost": ghost.and_then(|g| d.storeys.get(g)).map(|g| g.name.clone()), "ghostColor": hex(d.ghost_color) }))
}

/// STOREY.IMPORT: another office's plan as a storey of its own, to measure and trace from. Its
/// objects keep their layers (they join the drawing's, however many), and as the ghost they all
/// draw in one colour, so their layer names never need tidying.
fn run_import(s: &mut Session, p: &Value) -> Result<Value> {
    let file = str_param(p, "file")
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .ok_or_else(|| bad("storey.import", "`file` (a DXF or DWG path) is required"))?;
    let stem = file.rsplit(['/', '\\']).next().unwrap_or(file);
    let stem = stem.rsplit_once('.').map_or(stem, |(a, _)| a).to_string();
    let name = check_name("storey.import", str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&stem))?;
    let ghost = bool_or(p, "ghost", true);
    let first = !storeys(s)?.active();
    if first {
        return Err(bad("storey.import", "make the drawing's first storey (STOREY New) before importing a plan as another one"));
    }
    // Everything below changes the drawing in several steps; a failure anywhere puts it back whole.
    let snapshot = s.doc()?.clone();
    let r = import_into(s, file, &name, ghost, &snapshot);
    if r.is_err() {
        *s.doc_mut()? = snapshot;
    }
    r
}

fn import_into(s: &mut Session, file: &str, name: &str, ghost: bool, before: &Drawing) -> Result<Value> {
    let previous = before.storeys.current;
    run_new(s, &json!({ "name": name, "current": true }))?;
    let id = storeys(s)?.current;
    // The file's model space, exploded, at its own coordinates (block point 0,0 at 0,0), through a
    // block of a name no drawing uses (a file named like one of this drawing's blocks must not
    // redefine it), removed again afterwards. Called as a body, so the import is one undo step.
    let insert = find_command("insert").ok_or_else(|| crate::EngineError::UnknownCommand("insert".into()))?;
    let d = s.doc()?;
    let block = (1..=10_000u32)
        .map(|n| format!("STOREYIMPORT${n}"))
        .find(|b| d.block(b).is_none())
        .ok_or_else(|| bad("storey.import", "no free block name"))?;
    // Blocks of the file that this drawing has too come along renamed (see `define_foreign`).
    let block = super::blocks::insert_file::define_foreign(s, file, &block)?;
    (insert.run)(s, &json!({ "name": format!("*{block}"), "at": [0, 0], "basePoint": [0, 0] }))?;
    let d = s.doc_mut()?;
    let new: Vec<Handle> = d.model.iter().filter(|e| !before.model.contains(e.handle)).map(|e| e.handle).collect();
    for h in &new {
        d.model.modify(*h, |e| e.common.set_storey(Some(id)));
    }
    let used = d.model.iter().any(|e| matches!(&e.kind, cadcraft_doc::EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(&block)));
    if !used {
        d.blocks.retain(|k, _| !k.eq_ignore_ascii_case(&block));
    }
    d.storeys.current = previous;
    if ghost {
        d.ghost_storey = Some(id);
    }
    d.fix_ghost();
    s.set_selection(Vec::new());
    Ok(json!({ "storey": name, "objects": new.len(), "ghost": ghost }))
}

/// Set the ghost storey (`None`: off), remembering the one turned off.
fn set_ghost(s: &mut Session, ghost: Option<u32>) -> Result<Value> {
    view_change(s, |d| {
        if d.ghost_storey.is_some() && ghost.is_none() {
            d.ghost_last = d.ghost_storey;
        }
        d.ghost_storey = ghost;
        d.fix_ghost();
    })?;
    s.touch();
    let d = s.doc()?;
    Ok(json!({ "ghost": d.ghost_storey.and_then(|g| d.storeys.get(g)).map(|g| g.name.clone()) }))
}

fn run_ghost_toggle(s: &mut Session, _: &Value) -> Result<Value> {
    let d = s.doc()?;
    if !d.storeys.active() {
        return Err(bad("storey.ghost.toggle", "the drawing has no storeys"));
    }
    if d.ghost_storey.is_some() {
        return set_ghost(s, None);
    }
    let st = &d.storeys;
    let last = d.ghost_last.filter(|g| *g != st.current && st.contains(*g));
    // Else the storey below the current one (above, from the bottom storey).
    let i = st.index(st.current).unwrap_or(0);
    let near = i.checked_sub(1).or(Some(i + 1)).and_then(|j| st.list.get(j)).map(|x| x.id);
    match last.or(near) {
        Some(g) => set_ghost(s, Some(g)),
        None => Err(bad("storey.ghost.toggle", "there is no other storey to show as the ghost")),
    }
}

/// Move the ghost one storey up (`by` 1) or down (-1), past the current storey; from no ghost, start
/// next to the current storey.
fn ghost_step(s: &mut Session, by: isize) -> Result<Value> {
    let d = s.doc()?;
    let st = &d.storeys;
    if !st.active() {
        return Err(bad("storey.ghost", "the drawing has no storeys"));
    }
    let from = d.ghost_storey.or(Some(st.current)).and_then(|g| st.index(g)).unwrap_or(0);
    let mut j = from;
    loop {
        match j.checked_add_signed(by).filter(|j| *j < st.list.len()) {
            Some(n) => j = n,
            None => {
                let msg = if by > 0 { "the ghost is the top storey already" } else { "the ghost is the bottom storey already" };
                return Ok(json!({ "message": msg }));
            }
        }
        if st.list.get(j).is_some_and(|x| x.id != st.current) {
            break;
        }
    }
    let g = st.list.get(j).map(|x| x.id);
    set_ghost(s, g)
}

/// Target storeys from `to` (a name or a list of names).
fn targets_to(cmd: &str, s: &Session, p: &Value) -> Result<Vec<u32>> {
    let st = storeys(s)?;
    if !st.active() {
        return Err(bad(cmd, "the drawing has no storeys"));
    }
    let names: Vec<&str> = match p.get("to") {
        Some(Value::String(n)) => vec![n.as_str()],
        Some(Value::Array(a)) => {
            a.iter().map(|v| v.as_str().ok_or_else(|| bad(cmd, format!("`to`: {v} is not a storey name")))).collect::<Result<_>>()?
        }
        _ => return Err(bad(cmd, "`to` (a storey name or a list of names) is required")),
    };
    let mut ids = Vec::new();
    for n in names {
        let id = find(cmd, st, n)?.id;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Objects to copy or move: model-space objects of the current storey.
fn model_targets(cmd: &str, s: &Session, p: &Value) -> Result<Vec<Handle>> {
    let hs = targets(s, p)?;
    let d = s.doc()?;
    let mut seen = std::collections::HashSet::new();
    let hs: Vec<Handle> = hs.into_iter().filter(|h| d.model.contains(*h) && seen.insert(*h)).collect();
    if hs.is_empty() {
        return Err(bad(cmd, "no objects in model space selected"));
    }
    Ok(hs)
}

fn run_copy(s: &mut Session, p: &Value) -> Result<Value> {
    // A copy onto the current storey would lie exactly on its original.
    let current = storeys(s)?.current;
    let to: Vec<u32> = targets_to("storeycopy", s, p)?.into_iter().filter(|id| *id != current).collect();
    if to.is_empty() {
        return Err(bad("storeycopy", "`to` names only the current storey; copies go to other storeys"));
    }
    let hs = model_targets("storeycopy", s, p)?;
    let d = s.doc_mut()?;
    let mut out = Vec::new();
    for id in &to {
        // The copies on each storey link to each other as the originals did (a dimension to its
        // copied line); links to objects that were not copied go (they are on another storey).
        let map: std::collections::HashMap<Handle, Handle> = hs.iter().map(|h| (*h, d.new_handle())).collect();
        for h in &hs {
            let Some(mut e) = d.model.get(*h).map(|e| (**e).clone()) else { continue };
            e.handle = map[h];
            e.common.set_storey(Some(*id));
            relink(&mut e.kind, &map);
            out.push(e.handle);
            d.model.push(e);
        }
    }
    Ok(json!({ "copied": hs.len(), "handles": out.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

/// Point a copy's links at the copies in `map`; links to anything else go.
fn relink(kind: &mut cadcraft_doc::EntityKind, map: &std::collections::HashMap<Handle, Handle>) {
    match kind {
        cadcraft_doc::EntityKind::Dimension(dm) => {
            dm.assoc.retain_mut(|a| {
                let Some(h) = map.get(&a.handle) else { return false };
                a.handle = *h;
                match &mut a.snap {
                    cadcraft_doc::AssocSnap::Intersection { other } => map.get(other).map(|o| *other = *o).is_some(),
                    _ => true,
                }
            });
        }
        cadcraft_doc::EntityKind::Hatch(h) => {
            for l in &mut h.loops {
                if l.sources.iter().all(|s| map.contains_key(s)) {
                    l.sources.iter_mut().for_each(|s| *s = map[s]);
                } else {
                    l.sources.clear();
                }
            }
            if h.loops.iter().all(|l| l.sources.is_empty()) {
                h.associative = false;
            }
        }
        _ => {}
    }
}

fn run_move(s: &mut Session, p: &Value) -> Result<Value> {
    let to = targets_to("storeymove", s, p)?;
    let [id] = to.as_slice() else { return Err(bad("storeymove", "`to` is one storey")) };
    let id = *id;
    let hs = model_targets("storeymove", s, p)?;
    let d = s.doc_mut()?;
    for h in &hs {
        d.model.modify(*h, |e| e.common.set_storey(Some(id)));
    }
    // A constraint between objects now on different storeys would let the solver move an object
    // of a storey that can't be changed: it goes.
    let storey = |d: &Drawing, h: Handle| d.model.get(h).and_then(|e| d.storey_of(e));
    let before = d.constraints.len();
    let snapshot = d.clone();
    d.constraints.retain(|c| {
        let mut ids = c.refs.iter().filter_map(|r| storey(&snapshot, r.handle));
        ids.next().is_none_or(|first| ids.all(|x| x == first))
    });
    let cut = before - d.constraints.len();
    // The same for dimensions and associative hatches: the moved objects, and those left on the
    // current storey, lose links that now cross storeys.
    let current = d.storeys.current;
    let linked: Vec<Handle> =
        d.model.iter().filter(|e| has_links(e) && (hs.contains(&e.handle) || d.storey_of(e) == Some(current))).map(|e| e.handle).collect();
    cut_links(d, &linked);
    if id != d.storeys.current {
        s.set_selection(Vec::new());
    }
    if cut > 0 {
        return Ok(json!({ "moved": hs.len(), "message": format!("{cut} constraint(s) between storeys removed.") }));
    }
    Ok(json!({ "moved": hs.len() }))
}

// ---------------- interactive ----------------

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Stage {
    #[default]
    Main,
    NewName,
    RenameWhich,
    RenameTo(u32),
    Delete,
    Ghost,
}

/// Typed STOREY: a storey name makes it current; New, Rename, Delete and Ghost ask further.
#[derive(Default)]
pub(crate) struct StoreyM {
    stage: Stage,
}

impl StoreyM {
    fn run(&mut self, s: &mut Session, id: &str, p: Value) -> Result<Step> {
        match s.execute(id, &p) {
            Ok(v) => {
                if let Some(m) = v.get("message").and_then(Value::as_str) {
                    s.echo(m.to_string());
                }
            }
            Err(e) => s.echo(e.to_string()),
        }
        self.stage = Stage::Main;
        Ok(Step::Continue)
    }
}

impl Interactive for StoreyM {
    fn name(&self) -> &'static str {
        "STOREY"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let d = s.doc()?;
        if d.storeys.active() {
            let names: Vec<String> = d
                .storeys
                .list
                .iter()
                .rev()
                .map(|x| {
                    let mark = if x.id == d.storeys.current {
                        " (current)"
                    } else if d.ghost_storey == Some(x.id) {
                        " (ghost)"
                    } else {
                        ""
                    };
                    format!("{}{mark}", x.name)
                })
                .collect();
            s.echo(format!("Storeys, top to bottom: {}", names.join(", ")));
        } else {
            s.echo("The drawing has no storeys yet: New makes the first one, holding everything drawn so far.");
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match self.stage {
            Stage::Main => {
                let cur = s.doc().ok().and_then(|d| d.storeys.current_storey().map(|c| c.name.clone()));
                let p = Prompt::new("Enter a storey name to make it current", Accept::TEXT).kw(&["New", "Rename", "Delete", "Ghost"]);
                match cur {
                    Some(c) => p.default(c),
                    None => p,
                }
            }
            Stage::NewName => Prompt::new("Name of the new storey (above the current one)", Accept::TEXT),
            Stage::RenameWhich => Prompt::new("Storey to rename", Accept::TEXT),
            Stage::RenameTo(_) => Prompt::new("New storey name", Accept::TEXT),
            Stage::Delete => Prompt::new("Empty storey to delete", Accept::TEXT),
            Stage::Ghost => Prompt::new("Storey to show as the ghost", Accept::TEXT).kw(&["Off"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let text = match i {
            Input::Enter if self.stage == Stage::Main => return Ok(Step::Done),
            Input::Enter => {
                self.stage = Stage::Main;
                return Ok(Step::Continue);
            }
            Input::Cancel => return Ok(Step::Cancel),
            Input::Text(t) | Input::Keyword(t) => t.trim().to_string(),
            _ => return Ok(Step::Continue),
        };
        let kw = text.to_ascii_lowercase();
        match self.stage {
            Stage::Main => {
                // A storey's own name wins over a keyword spelled the same.
                if storeys(s)?.find(&text).is_some() {
                    return self.run(s, "storey.current", json!({ "name": text }));
                }
                self.stage = match kw.as_str() {
                    "n" | "new" => Stage::NewName,
                    "r" | "rename" => Stage::RenameWhich,
                    "d" | "delete" => Stage::Delete,
                    "g" | "ghost" => Stage::Ghost,
                    _ => {
                        s.echo(format!("No storey `{text}`."));
                        Stage::Main
                    }
                };
                Ok(Step::Continue)
            }
            Stage::NewName => self.run(s, "storey.new", json!({ "name": text })),
            Stage::RenameWhich => {
                match storeys(s)?.by_name(&text) {
                    Some(x) => self.stage = Stage::RenameTo(x.id),
                    None => s.echo(format!("No storey `{text}`.")),
                }
                Ok(Step::Continue)
            }
            Stage::RenameTo(id) => {
                let old = storeys(s)?.get(id).map(|x| x.name.clone()).unwrap_or_default();
                self.run(s, "storey.rename", json!({ "name": old, "newName": text }))
            }
            Stage::Delete => self.run(s, "storey.delete", json!({ "name": text })),
            Stage::Ghost => {
                let name = if matches!(kw.as_str(), "o" | "off") && storeys(s)?.by_name(&text).is_none() { Value::Null } else { json!(text) };
                self.run(s, "storey.ghost", json!({ "name": name }))
            }
        }
    }
}

/// STOREYCOPY / STOREYMOVE typed: select objects, then the target storey.
pub(crate) struct ToStoreyM {
    id: &'static str,
    name: &'static str,
    sel: SelectPhase,
    picked: Vec<Handle>,
}

impl ToStoreyM {
    fn new(id: &'static str, name: &'static str) -> Self {
        ToStoreyM { id, name, sel: SelectPhase::default(), picked: Vec::new() }
    }
}

impl Interactive for ToStoreyM {
    fn name(&self) -> &'static str {
        self.name
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        if !storeys(s)?.active() {
            s.echo("The drawing has no storeys (STOREY New makes the first one).");
            return Ok(Step::Cancel);
        }
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            self.picked = self.sel.picked.clone();
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.picked.is_empty() { self.sel.prompt() } else { Prompt::new("Target storey", Accept::TEXT) }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.picked.is_empty() {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.picked = hs;
                    Ok(Step::Continue)
                }
            };
        }
        let text = match i {
            Input::Text(t) | Input::Keyword(t) => t,
            Input::Enter | Input::Cancel => return Ok(Step::Cancel),
            _ => return Ok(Step::Continue),
        };
        let spec = find_command(self.id).ok_or_else(|| crate::EngineError::UnknownCommand(self.id.into()))?;
        let hs: Vec<String> = self.picked.iter().map(|h| h.hex()).collect();
        // Call the command body directly: the running command records the single undo step.
        match (spec.run)(s, &json!({ "handles": hs, "to": text.trim() })) {
            Ok(v) => {
                let n = v.get("copied").or_else(|| v.get("moved")).and_then(Value::as_u64).unwrap_or(0);
                s.echo(format!("{n} object(s) to storey {}.", text.trim()));
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            Err(e) => {
                s.echo(e.to_string());
                Ok(Step::Continue)
            }
        }
    }
}

#[cfg(test)]
#[path = "storey_tests.rs"]
mod tests;
