//! LAYMRG and LAYDEL: merge layers into another layer, and delete a layer together with every
//! object on it (model space, layouts and block definitions).
//!
//! The JSON forms (`laymrg {from, to}`, `laydel {names}`) never prompt. Typed, both pick layers by
//! selecting objects on them (or by Name), then ask for confirmation before changing anything.

use std::collections::HashSet;

use cadcraft_doc::{Drawing, EntityKind, EntityStore, Handle};
use serde_json::{Value, json};

use super::super::{bad, find_command, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// Layers whose names carry `|` belong to an attached xref and are managed by it.
fn xref_layer(name: &str) -> bool {
    name.contains('|')
}

fn protected(name: &str) -> bool {
    name == "0" || name.eq_ignore_ascii_case("Defpoints")
}

/// `key` as one name or a list of names.
fn names_param(p: &Value, key: &str) -> Vec<String> {
    match p.get(key) {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).take(10_000).map(str::to_string).collect(),
        _ => Vec::new(),
    }
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") }
}

/// Why `name` can't be merged away or deleted (`None`: it can). `delete`: LAYDEL's rules, which
/// also keep the current layer.
fn source_refusal(d: &Drawing, name: &str, delete: bool) -> Option<String> {
    let verb = if delete { "delete" } else { "merge" };
    if protected(name) {
        return Some(format!("Cannot {verb} layer \"{name}\" (layer 0 and Defpoints always stay)."));
    }
    if xref_layer(name) {
        return Some(format!("Cannot {verb} xref-dependent layer \"{name}\"."));
    }
    if delete && d.header.str("CLAYER", "0").eq_ignore_ascii_case(name) {
        return Some(format!("Cannot delete the current layer \"{name}\"."));
    }
    None
}

/// The drawing's spelling of each named layer, checked as sources. Duplicates collapse.
fn resolve_sources(d: &Drawing, cmd: &str, names: &[String], delete: bool) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        let l = d.layer(n.trim()).ok_or_else(|| bad(cmd, format!("no layer `{}`", n.trim())))?;
        if let Some(why) = source_refusal(d, &l.name, delete) {
            return Err(bad(cmd, why));
        }
        if !out.iter().any(|o| o.eq_ignore_ascii_case(&l.name)) {
            out.push(l.name.clone());
        }
    }
    if out.is_empty() {
        return Err(bad(cmd, if delete { "`names` is required" } else { "`from` is required" }));
    }
    Ok(out)
}

/// Every entity store whose objects belong to this drawing: model space, layouts and the block
/// definitions (an attached xref's blocks are left alone; they come from its file).
/// `f` gets each store with objects on the `set` layers and returns how many it changed.
fn for_each_store(d: &mut Drawing, set: &HashSet<String>, mut f: impl FnMut(&mut EntityStore) -> usize) -> usize {
    let mut n = f(&mut d.model);
    for layout in &mut d.layouts {
        n += f(&mut layout.entities);
    }
    for block in d.blocks.values_mut() {
        // Copy-on-write: only definitions that use the layers are touched.
        if block.xref_path.is_none() && on_layers(&block.entities, set).next().is_some() {
            n += f(&mut std::sync::Arc::make_mut(block).entities);
        }
    }
    n
}

fn on_layers<'a>(store: &'a EntityStore, set: &'a HashSet<String>) -> impl Iterator<Item = Handle> + 'a {
    store.iter().filter(|e| set.contains(&e.common.layer.to_ascii_lowercase())).map(|e| e.handle)
}

/// Drop the removed layers from every layout viewport's frozen list and colour overrides.
pub(super) fn forget_in_viewports(d: &mut Drawing, set: &HashSet<String>) {
    for layout in &mut d.layouts {
        let hs: Vec<Handle> = layout
            .entities
            .iter()
            .filter(|e| match &e.kind {
                EntityKind::Viewport(v) => {
                    v.frozen_layers.iter().any(|l| set.contains(&l.to_ascii_lowercase()))
                        || v.layer_colors.iter().any(|(l, _)| set.contains(&l.to_ascii_lowercase()))
                }
                _ => false,
            })
            .map(|e| e.handle)
            .collect();
        for h in hs {
            layout.entities.modify(h, |e| {
                if let EntityKind::Viewport(v) = &mut e.kind {
                    v.frozen_layers.retain(|l| !set.contains(&l.to_ascii_lowercase()));
                    v.layer_colors.retain(|(l, _)| !set.contains(&l.to_ascii_lowercase()));
                }
            });
        }
    }
}

/// Point every layout viewport's frozen list and colour overrides at a renamed layer.
pub(super) fn rename_in_viewports(d: &mut Drawing, old: &str, new: &str) {
    for layout in &mut d.layouts {
        let hs: Vec<Handle> = layout
            .entities
            .iter()
            .filter(|e| match &e.kind {
                EntityKind::Viewport(v) => {
                    v.frozen_layers.iter().any(|l| l.eq_ignore_ascii_case(old)) || v.layer_colors.iter().any(|(l, _)| l.eq_ignore_ascii_case(old))
                }
                _ => false,
            })
            .map(|e| e.handle)
            .collect();
        for h in hs {
            layout.entities.modify(h, |e| {
                if let EntityKind::Viewport(v) = &mut e.kind {
                    for l in &mut v.frozen_layers {
                        if l.eq_ignore_ascii_case(old) {
                            *l = new.to_string();
                        }
                    }
                    for (l, _) in &mut v.layer_colors {
                        if l.eq_ignore_ascii_case(old) {
                            *l = new.to_string();
                        }
                    }
                }
            });
        }
    }
}

fn lower_set(names: &[String]) -> HashSet<String> {
    names.iter().map(|n| n.to_ascii_lowercase()).collect()
}

/// `laymrg {from: [names] | name, to}`: move every object on the `from` layers to `to`, then
/// delete the `from` layers. Merging away the current layer makes the target current.
pub(super) fn run_laymrg(s: &mut Session, p: &Value) -> Result<Value> {
    let to = str_param(p, "to").map(str::trim).ok_or_else(|| bad("laymrg", "`to` is required"))?;
    let d = s.doc()?;
    let target = d.layer(to).ok_or_else(|| bad("laymrg", format!("no layer `{to}`")))?.name.clone();
    if xref_layer(&target) {
        return Err(bad("laymrg", format!("cannot merge into xref-dependent layer \"{target}\"")));
    }
    let sources = resolve_sources(d, "laymrg", &names_param(p, "from"), false)?;
    if sources.iter().any(|n| n.eq_ignore_ascii_case(&target)) {
        return Err(bad("laymrg", format!("cannot merge layer \"{target}\" into itself")));
    }
    let set = lower_set(&sources);
    let d = s.doc_mut()?;
    let moved = for_each_store(d, &set, |store| {
        let hs: Vec<Handle> = on_layers(store, &set).collect();
        for h in &hs {
            store.modify(*h, |e| e.common.layer = target.clone());
        }
        hs.len()
    });
    forget_in_viewports(d, &set);
    d.layers.retain(|l| !set.contains(&l.name.to_ascii_lowercase()));
    let mut msg = format!("Merged {} into \"{target}\": {} moved.", quoted(&sources), plural(moved, "object"));
    if set.contains(&d.header.str("CLAYER", "0").to_ascii_lowercase()) {
        if let Some(l) = d.layer_mut(&target) {
            l.frozen = false;
        }
        d.header.set_str("CLAYER", &target);
        msg.push_str(&format!("\nLayer \"{target}\" is now current."));
    }
    Ok(json!({ "merged": sources, "into": target, "moved": moved, "message": msg }))
}

/// `laydel {names: [...] | name}`: delete the layers and every object on them.
pub(super) fn run_laydel(s: &mut Session, p: &Value) -> Result<Value> {
    let mut names = names_param(p, "names");
    names.extend(names_param(p, "name"));
    let sources = resolve_sources(s.doc()?, "laydel", &names, true)?;
    let set = lower_set(&sources);
    let d = s.doc_mut()?;
    let erased = for_each_store(d, &set, |store| {
        let hs: Vec<Handle> = on_layers(store, &set).collect();
        for h in &hs {
            store.remove(*h);
        }
        hs.len()
    });
    forget_in_viewports(d, &set);
    d.layers.retain(|l| !set.contains(&l.name.to_ascii_lowercase()));
    s.set_selection(Vec::new());
    let msg = format!("Deleted {} and {}.", quoted(&sources), plural(erased, "object"));
    Ok(json!({ "deleted": sources, "erased": erased, "message": msg }))
}

fn quoted(names: &[String]) -> String {
    let list = names.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(", ");
    if names.len() == 1 { format!("layer {list}") } else { format!("layers {list}") }
}

// ---------------- typed: LAYMRG / LAYDEL ----------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Picking the layers to merge or delete.
    Source,
    SourceName,
    Target,
    TargetName,
    Confirm,
}

/// Keyword-only prompt.
const KW: Accept = Accept { point: false, number: false, text: false, select: false, enter: true };
/// A layer name (may contain spaces); numbers are names too.
const NAME: Accept = Accept { number: true, ..Accept::TEXT };

/// The LAYMRG and LAYDEL prompt sequence.
pub(super) struct LayerRemove {
    delete: bool,
    stage: Stage,
    sources: Vec<String>,
    target: Option<String>,
}

impl LayerRemove {
    pub(super) fn merge() -> Self {
        LayerRemove { delete: false, stage: Stage::Source, sources: Vec::new(), target: None }
    }
    pub(super) fn delete() -> Self {
        LayerRemove { delete: true, ..LayerRemove::merge() }
    }

    fn id(&self) -> &'static str {
        if self.delete { "laydel" } else { "laymrg" }
    }

    fn add_source(&mut self, s: &mut Session, name: &str) {
        let Ok(d) = s.doc() else { return };
        let Some(l) = d.layer(name.trim()) else {
            s.echo(format!("Layer \"{}\" not found.", name.trim()));
            return;
        };
        let name = l.name.clone();
        if let Some(why) = source_refusal(d, &name, self.delete) {
            s.echo(why);
            return;
        }
        if !self.sources.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            self.sources.push(name);
        }
        s.echo(format!("Selected layers: {}.", self.sources.join(", ")));
    }

    fn set_target(&mut self, s: &mut Session, name: &str) {
        let Ok(d) = s.doc() else { return };
        let Some(l) = d.layer(name.trim()) else {
            s.echo(format!("Layer \"{}\" not found.", name.trim()));
            return;
        };
        let name = l.name.clone();
        if xref_layer(&name) {
            s.echo(format!("Cannot merge into xref-dependent layer \"{name}\"."));
        } else if self.sources.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            s.echo(format!("Cannot merge layer \"{name}\" into itself."));
        } else {
            self.target = Some(name);
            self.confirm(s);
        }
    }

    /// Show the warning and ask.
    fn confirm(&mut self, s: &mut Session) {
        s.echo("******** WARNING ********");
        match &self.target {
            Some(t) if !self.delete => s.echo(format!("You are about to merge {} into layer \"{t}\".", quoted(&self.sources))),
            _ => s.echo(format!("You are about to delete {} and every object on it from this drawing.", quoted(&self.sources))),
        }
        s.echo("Do you wish to continue?");
        self.stage = Stage::Confirm;
    }

    fn layers_of(s: &Session, hs: &[Handle]) -> Vec<String> {
        let Ok(d) = s.doc() else { return Vec::new() };
        let mut out: Vec<String> = Vec::new();
        for l in hs.iter().filter_map(|h| d.entity(*h)).map(|e| e.common.layer.clone()) {
            if !out.iter().any(|o| o.eq_ignore_ascii_case(&l)) {
                out.push(l);
            }
        }
        out
    }

    /// Run the JSON body directly: the running command records the single undo step.
    fn apply(&self, s: &mut Session) -> Result<Step> {
        let spec = find_command(self.id()).ok_or_else(|| EngineError::UnknownCommand(self.id().into()))?;
        let p = match &self.target {
            Some(t) if !self.delete => json!({ "from": self.sources, "to": t }),
            _ => json!({ "names": self.sources }),
        };
        let r = (spec.run)(s, &p)?;
        if let Some(m) = r.get("message").and_then(Value::as_str) {
            for l in m.lines() {
                s.echo(l.to_string());
            }
        }
        Ok(Step::Done)
    }
}

impl Interactive for LayerRemove {
    fn name(&self) -> &'static str {
        if self.delete { "LAYDEL" } else { "LAYMRG" }
    }

    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            Stage::Source => {
                let msg = if self.delete { "Select object on layer to delete" } else { "Select object on layer to merge" };
                let kw: &[&str] = if self.sources.is_empty() { &["Name"] } else { &["Name", "Undo"] };
                Prompt::new(msg, Accept::SELECT).kw(kw)
            }
            Stage::SourceName => Prompt::new("Enter layer name", NAME),
            Stage::Target => Prompt::new("Select object on target layer", Accept::SELECT).kw(&["Name"]),
            Stage::TargetName => Prompt::new("Enter target layer name", NAME),
            // The question itself is echoed with the warning: "[Yes/No] <No>:" follows it.
            Stage::Confirm => Prompt::new("", KW).kw(&["Yes", "No"]).default("No"),
        }
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        // At "Select object" prompts the selection layer hands typed keywords over as text.
        let i = match i {
            Input::Text(t) if matches!(self.stage, Stage::Source | Stage::Target) => match self.prompt(s).match_keyword(&t) {
                Some(k) => Input::Keyword(k),
                None => Input::Text(t),
            },
            other => other,
        };
        match (self.stage, i) {
            (_, Input::Cancel) => return Ok(Step::Cancel),
            (Stage::Source, Input::Pick(hs)) => {
                for l in Self::layers_of(s, &hs) {
                    self.add_source(s, &l);
                }
            }
            (Stage::Source, Input::Keyword(k)) if k == "Name" => self.stage = Stage::SourceName,
            (Stage::Source, Input::Keyword(k)) if k == "Undo" => {
                self.sources.pop();
                s.echo(format!("Selected layers: {}.", self.sources.join(", ")));
            }
            (Stage::Source, Input::Enter) => {
                if self.sources.is_empty() {
                    return Ok(Step::Done);
                }
                if self.delete {
                    self.confirm(s);
                } else {
                    self.stage = Stage::Target;
                }
            }
            (Stage::SourceName, Input::Text(t) | Input::Keyword(t)) => {
                // Layer names never contain commas: a list adds each one.
                for n in t.split(',').map(str::trim).filter(|n| !n.is_empty()).take(1000) {
                    self.add_source(s, n);
                }
                self.stage = Stage::Source;
            }
            (Stage::SourceName, Input::Enter) => self.stage = Stage::Source,
            (Stage::Target, Input::Pick(hs)) => {
                if let Some(l) = Self::layers_of(s, &hs).first() {
                    self.set_target(s, l);
                }
            }
            (Stage::Target, Input::Keyword(k)) if k == "Name" => self.stage = Stage::TargetName,
            (Stage::Target, Input::Enter) => return Ok(Step::Done),
            (Stage::TargetName, Input::Text(t) | Input::Keyword(t)) => {
                self.stage = Stage::Target;
                self.set_target(s, &t);
            }
            (Stage::TargetName, Input::Enter) => self.stage = Stage::Target,
            (Stage::Confirm, Input::Keyword(k)) if k == "Yes" => return self.apply(s),
            (Stage::Confirm, Input::Keyword(_) | Input::Enter) => return Ok(Step::Done),
            (Stage::Confirm, _) => s.echo("Enter Yes or No."),
            (_, Input::Text(_) | Input::Keyword(_)) => s.echo("*Invalid selection*"),
            _ => {}
        }
        Ok(Step::Continue)
    }
}
