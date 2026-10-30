//! BVSTATE, BVHIDE and BVSHOW: the visibility states of a dynamic block in the Block Editor.
//! A block's visibility parameter (BPARAMETER Visibility) names sets of its objects; a reference
//! shows one set at a time. The editor shows one state, hiding the objects that state hides;
//! BVSTATE makes, picks and deletes states, BVHIDE and BVSHOW change what a state shows. As in
//! AutoCAD, an object drawn in the editor shows in the state the editor shows only, and so do the
//! grips of a parameter added there.

use cadcraft_doc::Handle;
use cadcraft_doc::dynblock::{
    MAX_VISIBILITY_STATES, ParameterKind, VisibilityState, adopt_objects, dynamic_block, parameter_objects, set_visibility,
};
use serde_json::{Value, json};

use crate::cmd::curves::{KW, handle_value};
use crate::cmd::machines::{SelOutcome, SelectPhase};
use crate::cmd::{CommandSpec, bad, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("bvstate", "Visibility States", run_bvstate)
            .alias(&["-bvstate"])
            .params(
                "{new?: name, visibility?: \"hide\"|\"show\"|\"current\" (what the new state shows: nothing, every object, or what the current state shows; default current) | set?: name (the state the editor shows) | delete?: name or [names] | rename?: name, to: new name | order?: [every state's name, in the new order] | ops?: [any of these, run in turn as one step (the Visibility States dialog's OK)]} → {current, states} (no option: just the list)",
            )
            .enabled(visibility_editing)
            .interactive(|_| Ok(Box::new(BvstateM::default()))),
        CommandSpec::new("bvhide", "Make Invisible", |s, p| run_hide_show(s, p, false))
            .params("{handles?: [objects] (default the selection), all?: bool (in every state; default the current one)} → {current, hidden}")
            .enabled(visibility_editing)
            .interactive(|_| Ok(Box::new(BvhideM::new(false)))),
        CommandSpec::new("bvshow", "Make Visible", |s, p| run_hide_show(s, p, true))
            .params("{handles?: [objects] (default the selection), all?: bool (in every state; default the current one)} → {current, shown}")
            .enabled(visibility_editing)
            .interactive(|_| Ok(Box::new(BvhideM::new(true)))),
    ]
}

/// The Block Editor is open on a block (visibility states belong to a block).
fn visibility_editing(s: &Session) -> std::result::Result<(), String> {
    match s.state() {
        Ok(st) => match &st.block_edit {
            Some(be) if be.current => Err("Visibility states belong to a block; this editor is open on the drawing itself.".into()),
            Some(_) => Ok(()),
            None => Err("Open a block in the Block Editor first (BEDIT).".into()),
        },
        Err(e) => Err(e.to_string()),
    }
}

/// The edited block's visibility parameter: the block, the objects it controls, its states and
/// the index of the one the editor shows. Objects drawn in the editor since are adopted first.
struct Vis {
    block: String,
    controlled: Vec<Handle>,
    states: Vec<VisibilityState>,
    current: usize,
}

fn read(s: &mut Session) -> Result<Vis> {
    visibility_editing(s).map_err(EngineError::Other)?;
    let (block, shown) = {
        let be = s.state()?.block_edit.as_ref().ok_or_else(|| EngineError::Other("The Block Editor is not open.".into()))?;
        (be.name.clone(), be.visibility.clone())
    };
    let objects: Vec<Handle> = s.doc()?.model.iter().map(|e| e.handle).collect();
    let none = || EngineError::Other("The block has no visibility parameter; add one first (BPARAMETER Visibility).".into());
    // A doc change only when there is something to adopt.
    let mut d = s.doc()?.clone();
    adopt_objects(&mut d, &block, &objects, shown.as_deref()).map_err(EngineError::Other)?;
    if !std::sync::Arc::ptr_eq(&d.foreign, &s.doc()?.foreign) {
        *s.doc_mut()? = d;
    }
    let m = dynamic_block(s.doc()?, &block).ok_or_else(none)?;
    let (controlled, states) = m
        .parameters
        .into_iter()
        .find_map(|p| match p.kind {
            ParameterKind::Visibility { controlled, states, .. } => Some((controlled, states)),
            _ => None,
        })
        .ok_or_else(none)?;
    let current = shown.as_deref().and_then(|v| states.iter().position(|s| s.name.eq_ignore_ascii_case(v))).unwrap_or(0);
    Ok(Vis { block, controlled, states, current })
}

impl Vis {
    fn current_name(&self) -> String {
        self.states.get(self.current).map(|s| s.name.clone()).unwrap_or_default()
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.states.iter().position(|s| s.name.eq_ignore_ascii_case(name.trim()))
    }

    /// Write the states back and show the current one in the editor.
    fn write(self, s: &mut Session) -> Result<Value> {
        set_visibility(s.doc_mut()?, &self.block, &self.controlled, &self.states).map_err(EngineError::Other)?;
        let current = self.current_name();
        show(s, Some(current.clone()))?;
        Ok(self.summary(current))
    }

    fn summary(&self, current: String) -> Value {
        json!({ "current": current, "states": self.states.iter().map(|s| s.name.clone()).collect::<Vec<_>>() })
    }
}

/// Make `state` the one the editor shows and hide what it hides.
fn show(s: &mut Session, state: Option<String>) -> Result<()> {
    if let Some(be) = s.state_mut()?.block_edit.as_mut() {
        be.visibility = state;
    }
    s.refresh_block_visibility()
}

/// A parameter just added (graph id `node`) to the edited block: active in the state the editor
/// shows only, as AutoCAD adds it, so references show its grips in that state.
pub(super) fn parameter_added(s: &mut Session, node: i32) -> Result<()> {
    let Ok(mut v) = read(s) else { return Ok(()) };
    let Some(m) = dynamic_block(s.doc()?, &v.block) else { return Ok(()) };
    let added = parameter_objects(&m, Some(node));
    if added.is_empty() {
        return Ok(());
    }
    if let Some(state) = v.states.get_mut(v.current) {
        let new: Vec<Handle> = added.into_iter().filter(|h| !state.active.contains(h)).collect();
        state.active.extend(new);
    }
    v.write(s).map(|_| ())
}

/// What a new state shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum NewShows {
    Nothing,
    Everything,
    Current,
}

fn new_state(s: &mut Session, name: &str, shows: NewShows) -> Result<Value> {
    let name = name.trim();
    if name.is_empty() {
        return Err(bad("bvstate", "a visibility state needs a name"));
    }
    let mut v = read(s)?;
    if v.find(name).is_some() {
        return Err(bad("bvstate", format!("visibility state \"{name}\" already exists")));
    }
    let state = match shows {
        NewShows::Nothing => VisibilityState { name: name.to_string(), visible: Vec::new(), active: Vec::new() },
        NewShows::Everything => {
            let active = dynamic_block(s.doc()?, &v.block).map(|m| parameter_objects(&m, None)).unwrap_or_default();
            VisibilityState { name: name.to_string(), visible: v.controlled.clone(), active }
        }
        NewShows::Current => VisibilityState { name: name.to_string(), ..v.states.get(v.current).cloned().unwrap_or_default() },
    };
    v.states.push(state);
    // The new state is the one the editor shows.
    v.current = v.states.len().saturating_sub(1);
    v.write(s)
}

fn set_state(s: &mut Session, name: &str) -> Result<Value> {
    let mut v = read(s)?;
    v.current = v.find(name).ok_or_else(|| bad("bvstate", format!("no visibility state \"{}\"", name.trim())))?;
    v.write(s)
}

fn delete_states(s: &mut Session, names: &[String]) -> Result<Value> {
    let mut v = read(s)?;
    let shown = v.current_name();
    for n in names {
        if v.find(n).is_none() {
            return Err(bad("bvstate", format!("no visibility state \"{}\"", n.trim())));
        }
    }
    v.states.retain(|st| !names.iter().any(|n| n.trim().eq_ignore_ascii_case(&st.name)));
    if v.states.is_empty() {
        return Err(bad("bvstate", "a block keeps at least one visibility state"));
    }
    // The editor keeps the state it showed, else shows the first.
    v.current = v.find(&shown).unwrap_or(0);
    v.write(s)
}

fn rename_state(s: &mut Session, from: &str, to: &str) -> Result<Value> {
    let to = to.trim();
    if to.is_empty() {
        return Err(bad("bvstate", "`to`: the state's new name"));
    }
    let mut v = read(s)?;
    let i = v.find(from).ok_or_else(|| bad("bvstate", format!("no visibility state \"{}\"", from.trim())))?;
    if v.find(to).is_some_and(|j| j != i) {
        return Err(bad("bvstate", format!("visibility state \"{to}\" already exists")));
    }
    if let Some(st) = v.states.get_mut(i) {
        st.name = to.to_string();
    }
    v.write(s)
}

/// Most operations one `ops` list runs.
const MAX_OPS: usize = 1_000;

fn run_bvstate(s: &mut Session, p: &Value) -> Result<Value> {
    match p.get("ops") {
        None | Some(Value::Null) => one_op(s, p),
        Some(Value::Array(ops)) if ops.len() <= MAX_OPS => {
            if ops.iter().any(|op| !op.is_object() || op.get("ops").is_some()) {
                return Err(bad("bvstate", "each of `ops` is one operation: {new}, {set}, {delete}, {rename, to} or {order}"));
            }
            // All or nothing: a refused operation leaves the states as they were.
            let before = s.doc()?.clone();
            let shown = s.state()?.block_edit.as_ref().and_then(|b| b.visibility.clone());
            for op in ops {
                if let Err(e) = one_op(s, op) {
                    *s.doc_mut()? = before;
                    show(s, shown)?;
                    return Err(e);
                }
            }
            let v = read(s)?;
            let current = v.current_name();
            Ok(v.summary(current))
        }
        Some(_) => Err(bad("bvstate", format!("`ops` is a list of at most {MAX_OPS} operations"))),
    }
}

/// Put the states in the order of `names` (every state once).
fn order_states(s: &mut Session, names: &[String]) -> Result<Value> {
    let mut v = read(s)?;
    let shown = v.current_name();
    let mut states = Vec::with_capacity(v.states.len());
    for n in names {
        let i = v.find(n).ok_or_else(|| bad("bvstate", format!("no visibility state \"{}\"", n.trim())))?;
        if states.iter().any(|st: &VisibilityState| st.name.eq_ignore_ascii_case(n.trim())) {
            return Err(bad("bvstate", format!("\"{}\" is listed twice in `order`", n.trim())));
        }
        states.push(v.states.get(i).cloned().unwrap_or_default());
    }
    if states.len() != v.states.len() {
        return Err(bad("bvstate", "`order` lists every visibility state once"));
    }
    v.states = states;
    v.current = v.find(&shown).unwrap_or(0);
    v.write(s)
}

/// A list of names, or one name.
fn names_param(p: &Value, key: &str) -> Result<Option<Vec<String>>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(n)) => Ok(Some(vec![n.clone()])),
        Some(Value::Array(a)) if a.len() <= MAX_VISIBILITY_STATES => {
            let names: Vec<String> = a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
            if names.len() != a.len() || names.is_empty() {
                return Err(bad("bvstate", format!("`{key}` is a state name or a list of names")));
            }
            Ok(Some(names))
        }
        Some(_) => Err(bad("bvstate", format!("`{key}` is a state name or a list of names"))),
    }
}

fn one_op(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(name) = str_param(p, "new") {
        let shows = match str_param(p, "visibility").map(|t| t.trim().to_ascii_lowercase()).as_deref() {
            None | Some("current") => NewShows::Current,
            Some("hide") | Some("hideall") | Some("hide all") => NewShows::Nothing,
            Some("show") | Some("showall") | Some("show all") => NewShows::Everything,
            Some(o) => return Err(bad("bvstate", format!("`visibility` is \"hide\", \"show\" or \"current\", not \"{o}\""))),
        };
        return new_state(s, name, shows);
    }
    if let Some(name) = str_param(p, "set") {
        return set_state(s, name);
    }
    if let Some(from) = str_param(p, "rename") {
        let to = str_param(p, "to").ok_or_else(|| bad("bvstate", "`to`: the state's new name"))?;
        return rename_state(s, from, to);
    }
    if let Some(names) = names_param(p, "delete")? {
        return delete_states(s, &names);
    }
    if let Some(names) = names_param(p, "order")? {
        return order_states(s, &names);
    }
    let v = read(s)?;
    let current = v.current_name();
    Ok(v.summary(current))
}

/// BVHIDE (`visible` false) or BVSHOW: `handles` in the current state, or in every state.
fn hide_show(s: &mut Session, handles: &[Handle], all: bool, visible: bool) -> Result<Value> {
    let cmd = if visible { "bvshow" } else { "bvhide" };
    if handles.is_empty() {
        return Err(bad(cmd, "`handles`: the objects to change"));
    }
    if handles.len() > cadcraft_doc::dynblock::MAX_ACTION_ITEMS {
        return Err(bad(cmd, "too many objects"));
    }
    let missing: Vec<String> = handles.iter().filter(|h| s.doc().ok().and_then(|d| d.model.get(**h)).is_none()).map(|h| h.hex()).collect();
    if !missing.is_empty() {
        return Err(bad(cmd, format!("no such object in the block: {}", missing.join(", "))));
    }
    let mut v = read(s)?;
    let current = v.current;
    for (i, st) in v.states.iter_mut().enumerate() {
        if !all && i != current {
            continue;
        }
        if visible {
            let new: Vec<Handle> = handles.iter().filter(|h| !st.visible.contains(h)).copied().collect();
            st.visible.extend(new);
        } else {
            st.visible.retain(|h| !handles.contains(h));
        }
    }
    let n = handles.len();
    // A hidden object can't stay selected; nothing is selected afterwards.
    s.set_selection(Vec::new());
    let mut out = v.write(s)?;
    out[if visible { "shown" } else { "hidden" }] = json!(n);
    Ok(out)
}

fn run_hide_show(s: &mut Session, p: &Value, visible: bool) -> Result<Value> {
    let cmd = if visible { "bvshow" } else { "bvhide" };
    let handles: Vec<Handle> = match p.get("handles") {
        Some(Value::Array(a)) => {
            a.iter().map(|v| handle_value(v).ok_or_else(|| bad(cmd, format!("{v} is not an object handle")))).collect::<Result<_>>()?
        }
        Some(_) => return Err(bad(cmd, "`handles` is an array of object handles")),
        None => s.selection(),
    };
    let all = match p.get("all") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(cmd, "`all` is true or false")),
    };
    hide_show(s, &handles, all, visible)
}

/// Read keyword input: `text` (any case, an optional leading `_`) against `options`, each
/// (keyword, its shortcut).
fn keyword<'a>(text: &str, options: &[(&'a str, &str)]) -> Option<&'a str> {
    let t = text.trim();
    let t = t.strip_prefix('_').unwrap_or(t);
    options.iter().find(|(k, short)| t.eq_ignore_ascii_case(k) || t.eq_ignore_ascii_case(short)).map(|(k, _)| *k)
}

fn text_of(i: &Input) -> Option<&str> {
    match i {
        Input::Keyword(t) | Input::Text(t) => Some(t.as_str()),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum VStage {
    #[default]
    Option,
    NewName,
    NewShows,
    Set,
    Delete,
}

/// -BVSTATE's prompts, as AutoCAD asks them on the command line (BVSTATE's dialog isn't here
/// yet, so BVSTATE asks the same).
#[derive(Default)]
struct BvstateM {
    stage: VStage,
    name: String,
}

const NEW_SHOWS: &[(&str, &str)] = &[("Hide all", "H"), ("Show all", "S"), ("Current visibility", "C")];

impl Interactive for BvstateM {
    fn name(&self) -> &'static str {
        "BVSTATE"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let v = read(s)?;
        s.echo(format!("Current visibility state: {}", v.current_name()));
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            VStage::Option => Prompt::new("Enter an option", KW).kw(&["New", "Set", "Delete"]).default("New"),
            VStage::NewName => Prompt::new("Enter name of new visibility state", Accept::TEXT),
            VStage::NewShows => Prompt::new("Enter object visibility for new state", KW)
                .kw(&["Hide all", "Show all", "Current visibility"])
                .default("Current visibility"),
            VStage::Set => Prompt::new("Enter visibility state name to make current or", Accept::TEXT).kw(&["?"]),
            VStage::Delete => Prompt::new("Enter visibility state name(s) to delete", Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if matches!(i, Input::Cancel) {
            return Ok(Step::Cancel);
        }
        match self.stage {
            VStage::Option => {
                let choice = match &i {
                    Input::Enter => Some("New"),
                    _ => text_of(&i).and_then(|t| keyword(t, &[("New", "N"), ("Set", "S"), ("Delete", "D")])),
                };
                match choice {
                    Some("New") => self.stage = VStage::NewName,
                    Some("Set") => self.stage = VStage::Set,
                    Some(_) => self.stage = VStage::Delete,
                    None => s.echo("Invalid option keyword."),
                }
            }
            VStage::NewName => {
                let Some(t) = text_of(&i).map(str::trim).filter(|t| !t.is_empty()) else {
                    return Ok(Step::Cancel);
                };
                let exists = read(s)?.find(t).is_some();
                if exists {
                    s.echo(format!("Visibility state \"{t}\" already exists."));
                } else {
                    self.name = t.to_string();
                    self.stage = VStage::NewShows;
                }
            }
            VStage::NewShows => {
                let shows = match &i {
                    Input::Enter => Some(NewShows::Current),
                    _ => text_of(&i).and_then(|t| keyword(t, NEW_SHOWS)).map(|k| match k {
                        "Hide all" => NewShows::Nothing,
                        "Show all" => NewShows::Everything,
                        _ => NewShows::Current,
                    }),
                };
                match shows {
                    Some(shows) => return new_state(s, &self.name, shows).map(|_| Step::Done),
                    None => s.echo("Invalid option keyword."),
                }
            }
            VStage::Set => match text_of(&i).map(str::trim) {
                Some("?") => {
                    let v = read(s)?;
                    for st in &v.states {
                        s.echo(format!(" {}", st.name));
                    }
                }
                Some(t) if !t.is_empty() => {
                    if read(s)?.find(t).is_some() {
                        return set_state(s, t).map(|_| Step::Done);
                    }
                    s.echo(format!("Visibility state \"{t}\" not found."));
                }
                _ => return Ok(Step::Cancel),
            },
            VStage::Delete => {
                let Some(t) = text_of(&i).map(str::trim).filter(|t| !t.is_empty()) else {
                    return Ok(Step::Cancel);
                };
                let names: Vec<String> = t.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect();
                match delete_states(s, &names) {
                    Ok(_) => return Ok(Step::Done),
                    Err(e) => s.echo(e.to_string()),
                }
            }
        }
        Ok(Step::Continue)
    }
}

/// BVHIDE's or BVSHOW's prompts, as AutoCAD asks them.
struct BvhideM {
    show: bool,
    select: SelectPhase,
    picked: Vec<Handle>,
    asking: bool,
}

impl BvhideM {
    fn new(show: bool) -> Self {
        BvhideM { show, select: SelectPhase::default(), picked: Vec::new(), asking: false }
    }
}

impl Interactive for BvhideM {
    fn name(&self) -> &'static str {
        if self.show { "BVSHOW" } else { "BVHIDE" }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        read(s)?;
        s.echo(if self.show { "Select objects to make visible:" } else { "Select objects to hide:" });
        self.select = SelectPhase::begin(s);
        if self.select.done {
            self.picked = self.select.picked.clone();
            self.asking = true;
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.asking {
            let msg =
                if self.show { "Make visible for current state or all visibility states" } else { "Hide for current state or all visibility states" };
            Prompt::new(msg, KW).kw(&["Current", "All"]).default("Current")
        } else {
            self.select.prompt()
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if matches!(i, Input::Cancel) {
            return Ok(Step::Cancel);
        }
        if !self.asking {
            match self.select.feed(s, &i)? {
                SelOutcome::More => {}
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.picked = hs;
                    self.asking = true;
                }
            }
            return Ok(Step::Continue);
        }
        let all = match &i {
            Input::Enter => Some(false),
            _ => text_of(&i).and_then(|t| keyword(t, &[("Current", "C"), ("All", "A")])).map(|k| k == "All"),
        };
        match all {
            Some(all) => hide_show(s, &self.picked, all, self.show).map(|_| Step::Done),
            None => {
                s.echo("Invalid option keyword.");
                Ok(Step::Continue)
            }
        }
    }
}
