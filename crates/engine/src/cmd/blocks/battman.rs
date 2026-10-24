//! BATTMAN: the attribute definitions of a block — listed, edited (tag, prompt, default value,
//! modes, height, layer), removed and reordered. Changes are applied to the existing references
//! (as ATTSYNC does) unless `sync` is false. Tags match in any case, as in AutoCAD.

use std::sync::Arc;

use cadcraft_doc::{Attrib, Block, EntityKind, EntityStore, Handle};
use serde_json::{Value, json};

use super::attribs::{blocks_with_attdefs, has_attdefs, reference_count, rename_tag, sync};
use crate::cmd::curves::KW;
use crate::cmd::{CommandSpec, bad, bool_or, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub(super) fn battman_spec() -> CommandSpec {
    CommandSpec::new("battman", "Block Attribute Manager...", run)
        .menu(&["Modify", "Object", "Attribute", "Block Attribute Manager..."])
        .params(
            "{name, action?: \"list\" (default) | \"edit\" | \"remove\" | \"up\" | \"down\" | \"order\" | \"sync\", tag? (the definition to edit, remove or move; any case), newTag?, prompt?, default?, invisible?, constant?, height?, layer?, order?: [every tag, in the new order], sync?: bool (default true: existing references follow, as with ATTSYNC)} → {block, attributes: [{tag, prompt, default, invisible, constant, height, layer}], references, synced?}",
        )
        .interactive(|s| Ok(Box::new(BattmanM::new(s))))
}

/// A change to one attribute definition; `None` leaves the property as it is.
#[derive(Clone, Debug, Default)]
struct Change {
    tag: Option<String>,
    prompt: Option<String>,
    default: Option<String>,
    invisible: Option<bool>,
    constant: Option<bool>,
    height: Option<f64>,
    layer: Option<String>,
}

/// The attribute definitions of `b` in order: handle, definition, layer.
fn defs(b: &Block) -> Vec<(Handle, &Attrib, &str)> {
    b.entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::AttDef(a) => Some((e.handle, a.as_ref(), e.common.layer.as_str())),
            _ => None,
        })
        .collect()
}

/// The definition tagged `tag` (any case) in `b`.
fn find(b: &Block, tag: &str) -> Result<Handle> {
    defs(b)
        .into_iter()
        .find(|(_, a, _)| a.tag.eq_ignore_ascii_case(tag.trim()))
        .map(|(h, ..)| h)
        .ok_or_else(|| EngineError::Other(format!("Block \"{}\" has no attribute tagged \"{}\".", b.name, tag.trim())))
}

fn block(s: &Session, name: &str) -> Result<Arc<Block>> {
    s.doc()?.block(name).cloned().ok_or_else(|| EngineError::Other(format!("Block \"{name}\" not found.")))
}

/// The block's attribute definitions and how many references it has.
fn listing(s: &Session, name: &str) -> Result<Value> {
    let b = block(s, name)?;
    let atts: Vec<Value> = defs(&b)
        .into_iter()
        .map(|(_, a, layer)| {
            json!({"tag": a.tag, "prompt": a.prompt, "default": a.text.value, "invisible": a.invisible, "constant": a.constant, "height": a.text.height, "layer": layer})
        })
        .collect();
    Ok(json!({ "block": b.name, "attributes": atts, "references": reference_count(s.doc()?, &b.name) }))
}

/// Replace block `name` with `f` applied to a copy of it; nothing changes when `f` fails.
fn update_block(s: &mut Session, name: &str, f: impl FnOnce(&mut Block) -> Result<()>) -> Result<()> {
    let mut b = (*block(s, name)?).clone();
    f(&mut b)?;
    let d = s.doc_mut()?;
    let key = d.blocks.keys().find(|k| k.eq_ignore_ascii_case(name)).cloned().unwrap_or_else(|| b.name.clone());
    d.blocks.insert(key, Arc::new(b));
    Ok(())
}

/// A tag as attribute definitions store it: non-empty, without spaces, in capitals.
fn valid_tag(tag: &str) -> Result<String> {
    let t = tag.trim();
    if t.is_empty() || t.contains(char::is_whitespace) {
        return Err(EngineError::Other("A tag must be non-empty and without spaces.".into()));
    }
    Ok(t.to_ascii_uppercase())
}

/// Apply `c` to the definition tagged `tag` of block `name`. Returns the tag it has afterwards.
fn edit(s: &mut Session, name: &str, tag: &str, c: &Change) -> Result<String> {
    let new_tag = c.tag.as_deref().map(valid_tag).transpose()?;
    if let Some(h) = c.height
        && !(h.is_finite() && h > 0.0)
    {
        return Err(EngineError::Other("The text height must be a positive number.".into()));
    }
    let layer = match &c.layer {
        Some(l) => {
            Some(s.doc()?.layer(l.trim()).map(|l| l.name.clone()).ok_or_else(|| EngineError::Other(format!("Layer \"{}\" not found.", l.trim())))?)
        }
        None => None,
    };
    let mut result = String::new();
    update_block(s, name, |b| {
        let h = find(b, tag)?;
        if let Some(t) = &new_tag
            && defs(b).iter().any(|(o, a, _)| *o != h && a.tag.eq_ignore_ascii_case(t))
        {
            return Err(EngineError::Other(format!("Block \"{}\" already has an attribute tagged \"{t}\".", b.name)));
        }
        b.entities.modify(h, |e| {
            if let Some(l) = &layer {
                e.common.layer = l.clone();
            }
            if let EntityKind::AttDef(a) = &mut e.kind {
                if let Some(t) = &new_tag {
                    a.tag = t.clone();
                }
                if let Some(p) = &c.prompt {
                    a.prompt = p.clone();
                }
                if let Some(v) = &c.default {
                    a.text.value = v.clone();
                }
                if let Some(v) = c.invisible {
                    a.invisible = v;
                }
                if let Some(v) = c.constant {
                    a.constant = v;
                }
                if let Some(v) = c.height {
                    a.text.height = v;
                }
                result = a.tag.clone();
            }
        });
        Ok(())
    })?;
    Ok(result)
}

/// Put the attribute definitions of `b` in the order `order` (handles of all of them), leaving
/// every other object where it is.
fn reorder(b: &mut Block, order: &[Handle]) {
    let mut next = order.iter();
    let mut store = EntityStore::new();
    for e in b.entities.iter() {
        let h = if matches!(e.kind, EntityKind::AttDef(_)) { next.next().copied().unwrap_or(e.handle) } else { e.handle };
        if let Some(e) = b.entities.get(h) {
            store.push((**e).clone());
        }
    }
    b.entities = store;
}

/// Move the definition tagged `tag` one place up (`by` -1) or down (+1); at either end nothing moves.
fn shift(s: &mut Session, name: &str, tag: &str, by: isize) -> Result<()> {
    update_block(s, name, |b| {
        let h = find(b, tag)?;
        let mut order: Vec<Handle> = defs(b).iter().map(|(h, ..)| *h).collect();
        let Some(i) = order.iter().position(|o| *o == h) else { return Ok(()) };
        if let Some(j) = i.checked_add_signed(by).filter(|j| *j < order.len()) {
            order.swap(i, j);
            reorder(b, &order);
        }
        Ok(())
    })
}

/// Put the definitions in the order of `tags`, which must name each of them once.
fn set_order(s: &mut Session, name: &str, tags: &[String]) -> Result<()> {
    update_block(s, name, |b| {
        let all = defs(b).len();
        let mut order: Vec<Handle> = Vec::with_capacity(all);
        for t in tags.iter().take(all.saturating_add(1)) {
            let h = find(b, t)?;
            if order.contains(&h) {
                return Err(EngineError::Other(format!("\"{t}\" is listed twice.")));
            }
            order.push(h);
        }
        if order.len() != all {
            return Err(EngineError::Other(format!("The order must list each of the {all} attribute tags once.")));
        }
        reorder(b, &order);
        Ok(())
    })
}

fn remove(s: &mut Session, name: &str, tag: &str) -> Result<()> {
    update_block(s, name, |b| {
        let h = find(b, tag)?;
        b.entities.remove(h);
        Ok(())
    })
}

/// Existing references follow the definitions: attributes renamed from `renamed` (old, new) keep
/// their values. Returns how many references were updated.
fn apply(s: &mut Session, name: &str, renamed: Option<(&str, &str)>) -> Result<usize> {
    if let Some((old, new)) = renamed.filter(|(o, n)| !o.eq_ignore_ascii_case(n)) {
        rename_tag(s, name, old, new)?;
    }
    Ok(sync(s, name)?.1)
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("battman", "`name` is required"))?.to_string();
    let name = block(s, &name)?.name.clone();
    let action = str_param(p, "action").unwrap_or("list").to_ascii_lowercase();
    let tag = || str_param(p, "tag").map(str::to_string).ok_or_else(|| bad("battman", format!("`tag` is required for \"{action}\"")));
    let mut renamed = None;
    match action.as_str() {
        "list" => return listing(s, &name),
        "sync" => {}
        "edit" => {
            let opt_str = |k| str_param(p, k).map(str::to_string);
            let opt_bool = |k| p.get(k).and_then(Value::as_bool);
            let height = match p.get("height") {
                None | Some(Value::Null) => None,
                Some(v) => Some(v.as_f64().ok_or_else(|| bad("battman", "`height` must be a number"))?),
            };
            let c = Change {
                tag: opt_str("newTag"),
                prompt: opt_str("prompt"),
                default: opt_str("default"),
                invisible: opt_bool("invisible"),
                constant: opt_bool("constant"),
                height,
                layer: opt_str("layer"),
            };
            let old = tag()?;
            let new = edit(s, &name, &old, &c)?;
            renamed = Some((old, new));
        }
        "remove" => remove(s, &name, &tag()?)?,
        "up" => shift(s, &name, &tag()?, -1)?,
        "down" => shift(s, &name, &tag()?, 1)?,
        "order" => {
            let tags: Vec<String> = p
                .get("order")
                .and_then(Value::as_array)
                .ok_or_else(|| bad("battman", "`order` (an array of tags) is required"))?
                .iter()
                .map(|v| v.as_str().map(str::to_string).ok_or_else(|| bad("battman", "`order` must hold tags (strings)")))
                .collect::<Result<_>>()?;
            set_order(s, &name, &tags)?;
        }
        other => return Err(bad("battman", format!("unknown action \"{other}\" (list, edit, remove, up, down, order or sync)"))),
    }
    let synced = if action == "sync" || bool_or(p, "sync", true) {
        Some(apply(s, &name, renamed.as_ref().map(|(o, n)| (o.as_str(), n.as_str())))?)
    } else {
        None
    };
    let mut r = listing(s, &name)?;
    if let (Some(n), Some(o)) = (synced, r.as_object_mut()) {
        o.insert("synced".into(), json!(n));
    }
    Ok(r)
}

// ---------------- command line ----------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Edit,
    Remove,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Prop {
    Tag,
    Prompt,
    Default,
    Invisible,
    Constant,
    Height,
    Layer,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Block,
    Option,
    Tag(Op),
    Prop,
    Value(Prop),
}

/// BATTMAN typed at a command line without the dialog: a block name, then options to edit,
/// remove, move and synchronize its attribute definitions. Every change reaches the existing
/// references at once (the dialog's default).
struct BattmanM {
    stage: Stage,
    block: String,
    tag: String,
}

const OPTIONS: &[&str] = &["Edit", "Remove", "moveUp", "moveDown", "Sync", "List", "eXit"];
const PROPS: &[&str] = &["Tag", "Prompt", "Default", "Invisible", "Constant", "Height", "Layer", "eXit"];

impl BattmanM {
    fn new(s: &Session) -> Self {
        // A selected block reference with attributes names the block.
        let picked = s.selection().into_iter().find_map(|h| match s.doc().ok()?.entity(h).map(|e| &e.kind) {
            Some(EntityKind::Insert(i)) if has_attdefs(s, &i.block) => Some(i.block.clone()),
            _ => None,
        });
        match picked {
            Some(b) => BattmanM { stage: Stage::Option, block: b, tag: String::new() },
            None => BattmanM { stage: Stage::Block, block: String::new(), tag: String::new() },
        }
    }

    /// Echo the block's attribute definitions, one per line.
    fn list(&self, s: &mut Session) -> Result<()> {
        let b = block(s, &self.block)?;
        let n = reference_count(s.doc()?, &b.name);
        s.echo(format!("Block {}: {n} reference{} in the drawing.", b.name, if n == 1 { "" } else { "s" }));
        let lines: Vec<String> = defs(&b)
            .into_iter()
            .map(|(_, a, _)| {
                let mut modes: Vec<&str> = Vec::new();
                if a.invisible {
                    modes.push("invisible");
                }
                if a.constant {
                    modes.push("constant");
                }
                let modes = if modes.is_empty() { String::new() } else { format!(" ({})", modes.join(", ")) };
                format!("  {}  prompt \"{}\"  default \"{}\"{modes}", a.tag, a.prompt, a.text.value)
            })
            .collect();
        for l in lines {
            s.echo(l);
        }
        Ok(())
    }

    /// The definition being edited (by its tag).
    fn current(&self, s: &Session) -> Option<Attrib> {
        let b = s.doc().ok()?.block(&self.block)?.clone();
        defs(&b).into_iter().find(|(_, a, _)| a.tag.eq_ignore_ascii_case(&self.tag)).map(|(_, a, _)| a.clone())
    }

    fn current_layer(&self, s: &Session) -> String {
        let Some(b) = s.doc().ok().and_then(|d| d.block(&self.block)).cloned() else { return "0".into() };
        defs(&b).into_iter().find(|(_, a, _)| a.tag.eq_ignore_ascii_case(&self.tag)).map(|(.., l)| l.to_string()).unwrap_or_else(|| "0".into())
    }

    /// Edit the current definition with `c` and bring the references in line.
    fn change(&mut self, s: &mut Session, c: Change) -> Result<()> {
        let old = self.tag.clone();
        let name = self.block.clone();
        let new = edit(s, &name, &old, &c)?;
        apply(s, &name, Some((&old, &new)))?;
        self.tag = new;
        Ok(())
    }
}

/// `v` with at most four decimals and no trailing zeros.
fn num(v: f64) -> String {
    let t = format!("{v:.4}");
    if t.contains('.') { t.trim_end_matches('0').trim_end_matches('.').to_string() } else { t }
}

/// Yes or No typed or picked as a keyword.
fn yes(i: &Input) -> Option<bool> {
    match i {
        Input::Keyword(k) | Input::Text(k) => match k.trim().to_ascii_lowercase().chars().next() {
            Some('y') => Some(true),
            Some('n') => Some(false),
            _ => None,
        },
        _ => None,
    }
}

impl Interactive for BattmanM {
    fn name(&self) -> &'static str {
        "BATTMAN"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        if self.stage == Stage::Option {
            s.set_selection(Vec::new());
            self.list(s)?;
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let yes_no = |v: bool| if v { "Yes" } else { "No" };
        match self.stage {
            Stage::Block => Prompt::new("Enter name of block to manage", Accept::TEXT).kw(&["?"]),
            Stage::Option => Prompt::new(format!("Block {}: enter an option", self.block), KW).kw(OPTIONS).default("eXit"),
            Stage::Tag(_) => Prompt::new("Enter attribute tag", Accept::TEXT),
            Stage::Prop => Prompt::new(format!("Attribute {}: enter property to change", self.tag), KW).kw(PROPS).default("eXit"),
            Stage::Value(p) => {
                let a = self.current(s);
                let text = |f: fn(&Attrib) -> String| a.as_ref().map(f).unwrap_or_default();
                match p {
                    Prop::Tag => Prompt::new("Enter new tag", Accept::TEXT).default(self.tag.clone()),
                    Prop::Prompt => Prompt::new("Enter new prompt", Accept::TEXT).default(text(|a| a.prompt.clone())),
                    Prop::Default => Prompt::new("Enter new default value", Accept::TEXT).default(text(|a| a.text.value.clone())),
                    Prop::Invisible => Prompt::new("Invisible?", KW).kw(&["Yes", "No"]).default(yes_no(a.as_ref().is_some_and(|a| a.invisible))),
                    Prop::Constant => Prompt::new("Constant?", KW).kw(&["Yes", "No"]).default(yes_no(a.as_ref().is_some_and(|a| a.constant))),
                    Prop::Height => Prompt::new("Specify text height", Accept::NUMBER).default(num(a.as_ref().map_or(0.0, |a| a.text.height))),
                    Prop::Layer => Prompt::new("Enter layer name", Accept::TEXT).default(self.current_layer(s)),
                }
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.stage {
            Stage::Block => match &i {
                Input::Keyword(k) if k == "?" => {
                    let names = blocks_with_attdefs(s, "*");
                    s.echo(if names.is_empty() {
                        "No blocks with attributes.".to_string()
                    } else {
                        format!("Blocks with attributes: {}", names.join(", "))
                    });
                }
                Input::Text(t) if !t.trim().is_empty() => {
                    let b = block(s, t.trim())?;
                    if !b.entities.iter().any(|e| matches!(e.kind, EntityKind::AttDef(_))) {
                        return Err(EngineError::Other(format!("Block \"{}\" has no attribute definitions.", b.name)));
                    }
                    self.block = b.name.clone();
                    self.stage = Stage::Option;
                    self.list(s)?;
                }
                Input::Enter => return Ok(Step::Done),
                _ => {}
            },
            Stage::Option => match &i {
                Input::Keyword(k) => match k.as_str() {
                    "Edit" => self.stage = Stage::Tag(Op::Edit),
                    "Remove" => self.stage = Stage::Tag(Op::Remove),
                    "moveUp" => self.stage = Stage::Tag(Op::Up),
                    "moveDown" => self.stage = Stage::Tag(Op::Down),
                    "Sync" => {
                        let (name, n) = sync(s, &self.block)?;
                        s.echo(format!("ATTSYNC block {name}: {n} reference{} synchronized.", if n == 1 { "" } else { "s" }));
                    }
                    "List" => self.list(s)?,
                    _ => return Ok(Step::Done),
                },
                Input::Enter => return Ok(Step::Done),
                _ => return Err(EngineError::Other("Invalid option keyword.".into())),
            },
            Stage::Tag(op) => match &i {
                Input::Text(t) if !t.trim().is_empty() => {
                    let name = self.block.clone();
                    let b = block(s, &name)?;
                    let h = find(&b, t)?;
                    let tag = defs(&b).into_iter().find(|(o, ..)| *o == h).map(|(_, a, _)| a.tag.clone()).unwrap_or_default();
                    match op {
                        Op::Edit => {
                            self.tag = tag;
                            self.stage = Stage::Prop;
                            return Ok(Step::Continue);
                        }
                        Op::Remove => remove(s, &name, &tag)?,
                        Op::Up => shift(s, &name, &tag, -1)?,
                        Op::Down => shift(s, &name, &tag, 1)?,
                    }
                    apply(s, &name, None)?;
                    self.stage = Stage::Option;
                }
                Input::Enter => self.stage = Stage::Option,
                _ => {}
            },
            Stage::Prop => match &i {
                Input::Keyword(k) => {
                    self.stage = match k.as_str() {
                        "Tag" => Stage::Value(Prop::Tag),
                        "Prompt" => Stage::Value(Prop::Prompt),
                        "Default" => Stage::Value(Prop::Default),
                        "Invisible" => Stage::Value(Prop::Invisible),
                        "Constant" => Stage::Value(Prop::Constant),
                        "Height" => Stage::Value(Prop::Height),
                        "Layer" => Stage::Value(Prop::Layer),
                        _ => Stage::Option,
                    }
                }
                Input::Enter => self.stage = Stage::Option,
                _ => return Err(EngineError::Other("Invalid option keyword.".into())),
            },
            Stage::Value(p) => {
                if i == Input::Enter {
                    self.stage = Stage::Prop;
                    return Ok(Step::Continue);
                }
                let text = match &i {
                    Input::Text(t) | Input::Keyword(t) => t.clone(),
                    _ => return Ok(Step::Continue),
                };
                let c = match p {
                    Prop::Tag => Change { tag: Some(text), ..Change::default() },
                    Prop::Prompt => Change { prompt: Some(text), ..Change::default() },
                    Prop::Default => Change { default: Some(text), ..Change::default() },
                    Prop::Invisible => {
                        Change { invisible: Some(yes(&i).ok_or_else(|| EngineError::Other("Enter Yes or No.".into()))?), ..Change::default() }
                    }
                    Prop::Constant => {
                        Change { constant: Some(yes(&i).ok_or_else(|| EngineError::Other("Enter Yes or No.".into()))?), ..Change::default() }
                    }
                    Prop::Height => {
                        let h = crate::units::parse_distance(&text).ok_or_else(|| EngineError::Other("Requires a positive number.".into()))?;
                        Change { height: Some(h), ..Change::default() }
                    }
                    Prop::Layer => Change { layer: Some(text), ..Change::default() },
                };
                self.change(s, c)?;
                self.stage = Stage::Prop;
            }
        }
        Ok(Step::Continue)
    }
}
