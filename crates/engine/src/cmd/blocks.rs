//! Blocks: BLOCK, INSERT, ATTDEF, ATTEDIT (programmatic), WBLOCK, BASE, PURGE, block listing, the
//! Block Editor.

pub(crate) mod bedit;
pub(in crate::cmd) mod purge;

use std::sync::Arc;

use cadcraft_doc::{Attrib, Block, Entity, EntityKind, HAlign, Handle, Insert, Text, VAlign};
use cadcraft_geom::{Mat3, Vec2, Vec3};
use serde_json::{Value, json};

use super::helpers::v3;
use super::machines::{SelOutcome, SelectPhase, number};
use super::{CommandSpec, bad, bool_or, f64_or, find_command, ok, point_param, point_req, point_value, str_param, targets};

use super::has_doc;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

mod attedit;
mod burst;
mod description;
mod dyngrip;
mod extract;
pub(crate) mod refedit;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("block", "Make...", run_block)
            .menu(&["Draw", "Block", "Make..."])
            .alias(&["b", "bmake"])
            .params("{name, base: [x,y], handles?, keep?: \"convert\"|\"retain\"|\"delete\" (default \"convert\"), description? (up to 255 characters, one line), annotative?}")
            .interactive(|_| Ok(Box::new(BlockM::default()))),
        // -BLOCK erases the selected objects once the block is defined.
        CommandSpec::new("-block", "Create Block", run_dash_block)
            .alias(&["-b"])
            .params("{name, base: [x,y], handles?, keep?: \"delete\"|\"retain\"|\"convert\" (default \"delete\"), description? (up to 255 characters, one line)}")
            .interactive(|_| Ok(Box::new(BlockM { dash: true, ..BlockM::default() }))),
        CommandSpec::new("insert", "Block...", run_insert)
            .menu(&["Insert", "Block..."])
            .alias(&["i", "-insert", "ddinsert"])
            .params("{name (\"*NAME\": exploded), file?: DXF/DWG path (defines or redefines block `name`, default the file name, from its model space; base point $INSBASE), at: [x,y], scale?: number | [x, y], rotation? (degrees), basePoint?: [x,y] (block coordinates), attribs?: {TAG: value}, explode?: bool}")
            .interactive(|_| Ok(Box::new(InsertM::default()))),
        CommandSpec::new("attdef", "Define Attributes...", run_attdef)
            .menu(&["Draw", "Block", "Define Attributes..."])
            .alias(&["att", "-attdef"])
            .params("{tag, prompt?, default?, at, height?, invisible?}")
            .interactive(|_| Ok(Box::new(AttdefM::default()))),
        attedit::attedit_spec(),
        attedit::dash_attedit_spec(),
        CommandSpec::new("wblock", "Write Block", run_wblock).alias(&["w"]).params("{path, name? | handles?, base?}"),
        CommandSpec::new("base", "Base", run_base).menu(&["Draw", "Block", "Base"]).params("{at: [x,y]}"),
        CommandSpec::new("blocks.dynamic", "Dynamic Block Data", run_dynamic)
            .params("{name (a block, or \"*Model_Space\" for a drawing that is a dynamic block's own)} → {parameters, actions, other} | null (not a dynamic block)")
            .enabled(has_doc)
            .noundo(),
        CommandSpec::new("blocks.reference", "Dynamic Block Reference", run_reference)
            .params("{handle (a block reference)} → {definition, values (by parameter graph id), properties: [{name, value, choices}]} | null (not a dynamic block reference)")
            .enabled(has_doc)
            .noundo(),
        CommandSpec::new("blocks.reference.set", "Set Dynamic Property", run_reference_set)
            .params("{handle (a dynamic block reference), name (a custom property as Properties names it: a visibility state, a lookup, a distance or \"Position1 X\"), value (lengths in drawing units)} → {properties}")
            .enabled(has_doc),
        CommandSpec::new("blocks.reference.grips", "Dynamic Block Grips", dyngrip::run_grips)
            .params("{handle} → [{node, part: base|end|at, shape: linear|point|list, at: [x,y], dir: [x,y], property, choices?}] (empty: not a dynamic block reference)")
            .enabled(has_doc)
            .noundo(),
        CommandSpec::new("blocks.reference.grip", "Drag Dynamic Grip", dyngrip::run_grip)
            .params("{handle, node (the parameter's graph id), part: base|end|at, to: [x,y] (drawing coordinates)} → {properties}: a distance's arrow changes the distance (snapped to its values), a position's square moves it")
            .enabled(has_doc),
        CommandSpec::new("purge", "Purge", purge::run)
            .alias(&["pu", "-purge"])
            .params("{type?: all|blocks|dimstyles|groups|layers|linetypes|mleaderstyles|tablestyles|textstyles|zerolength|emptytext (default all: every named type), names?: \"A*,B\" (default *)} → purged")
            .interactive(|_| Ok(Box::new(purge::PurgeM::default()))),
        CommandSpec::new("blocks.list", "List Blocks", run_list).enabled(has_doc).noundo(),
        battman::battman_spec(),
        attribs::attdisp_spec(),
        attribs::attsync_spec(),
        burst::burst_spec(),
        blockreplace::blockreplace_spec(),
        extract::dataextraction_spec(),
        extract::eattext_spec(),
        description::description_spec(),
    ]
    .into_iter()
    .chain(attribs::attdisp_menu_specs())
    .chain(xref::specs())
    .chain(refedit::specs())
    .chain(bedit::specs())
    .collect()
}

pub(crate) fn valid_block_name(n: &str) -> bool {
    !n.trim().is_empty() && n.len() <= 255 && !n.starts_with('*') && !n.chars().any(|c| "<>/\\\":;?*|,=`".contains(c))
}

/// The block an entity draws, if any: a block reference's block, a dimension's block or a
/// multileader's block content.
fn referenced_block(e: &Entity) -> Option<String> {
    match &e.kind {
        EntityKind::Insert(i) => Some(i.block.clone()),
        EntityKind::Dimension(dm) => dm.block.clone(),
        EntityKind::MLeader(m) => m.block.as_ref().map(|b| b.block.clone()),
        _ => None,
    }
}

/// Whether any of `ents` refers to block `name`, directly or through nested block references,
/// dimension blocks and multileader block content.
pub(crate) fn references_block(d: &cadcraft_doc::Drawing, ents: &[Entity], name: &str) -> bool {
    let mut todo: Vec<String> = ents.iter().filter_map(referenced_block).collect();
    let mut seen = std::collections::HashSet::new();
    while let Some(b) = todo.pop() {
        if b.eq_ignore_ascii_case(name) {
            return true;
        }
        if seen.insert(b.to_ascii_uppercase())
            && let Some(blk) = d.block(&b)
        {
            todo.extend(blk.entities.iter().filter_map(|e| referenced_block(e)));
        }
    }
    false
}

/// What BLOCK does with the selected objects once the block is defined (the dialog's Convert to
/// block, Retain and Delete).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Keep {
    /// Replace the objects by a reference to the new block.
    Convert,
    /// Leave the objects in the drawing.
    Retain,
    /// Erase the objects.
    Delete,
}

impl Keep {
    /// The `keep` parameter's documented values, in any case; `None` for anything else.
    pub(crate) fn parse(value: &str) -> Option<Keep> {
        match value.trim().to_ascii_lowercase().as_str() {
            "convert" => Some(Keep::Convert),
            "retain" => Some(Keep::Retain),
            "delete" => Some(Keep::Delete),
            _ => None,
        }
    }
}

/// The `keep` parameter of `cmd`, `default` when absent. A value that is not documented is an
/// error (issue #935: a misspelt value used to erase the objects without converting them).
fn keep_param(cmd: &str, p: &Value, default: Keep) -> Result<Keep> {
    match str_param(p, "keep") {
        None => Ok(default),
        Some(v) => Keep::parse(v).ok_or_else(|| bad(cmd, format!("`keep` must be \"convert\", \"retain\" or \"delete\", not {v:?}"))),
    }
}

/// Create a block from entities. Returns the insert handle when converting.
pub(crate) fn make_block(s: &mut Session, name: &str, base: Vec2, hs: &[Handle], keep: Keep, description: &str) -> Result<Option<Handle>> {
    if !valid_block_name(name) {
        return Err(bad("block", "invalid block name"));
    }
    let d = s.doc()?;
    let ents: Vec<Entity> = hs.iter().filter_map(|h| d.entity(*h).map(|e| (**e).clone())).collect();
    if ents.is_empty() {
        return Err(bad("block", "no objects selected"));
    }
    if references_block(d, &ents, name) {
        return Err(bad("block", format!("Block \"{name}\" references itself.")));
    }
    if d.refedit.as_ref().is_some_and(|r| r.block.eq_ignore_ascii_case(name)) {
        return Err(bad("block", format!("Block \"{name}\" is being edited in place; finish with REFCLOSE first.")));
    }
    // Block names are case-insensitive: redefining keeps the existing definition's name.
    let name = d.blocks.keys().find(|k| k.eq_ignore_ascii_case(name)).cloned().unwrap_or_else(|| name.to_string());
    let name = name.as_str();
    let mut b = Block::new(name);
    b.base = v3(base);
    b.description = description.to_string();
    let doc = s.doc_mut()?;
    for mut e in ents {
        e.handle = doc.new_handle();
        b.entities.push(e);
    }
    doc.blocks.insert(name.to_string(), Arc::new(b));
    if keep != Keep::Retain {
        for h in hs {
            doc.remove_entity(*h);
        }
    }
    s.set_selection(Vec::new());
    if keep == Keep::Convert {
        let h = insert(s, name, base, 1.0, 0.0, &serde_json::Map::new())?;
        return Ok(Some(h));
    }
    Ok(None)
}

/// Insert a block reference, filling attributes from `values` (else their defaults).
pub(crate) fn insert(s: &mut Session, name: &str, at: Vec2, scale: f64, rotation: f64, values: &serde_json::Map<String, Value>) -> Result<Handle> {
    insert_with(s, name, at, Vec2::new(scale, scale), rotation, None, values)
}

/// The block reference INSERT places: X/Y/Z scale factors, and an optional base point (in block
/// coordinates) that lands on `at` instead of the block's own base point.
fn insert_entity(blk: &Block, at: Vec2, scale: Vec3, rotation: f64, base: Option<Vec2>) -> Insert {
    let mut ins = Insert {
        block: blk.name.clone(),
        insert: v3(at),
        scale,
        rotation,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    if let Some(b) = base {
        let landed = ins.transform(blk.base.xy()).apply(b);
        ins.insert = v3(at + (at - landed));
    }
    ins
}

/// [`insert`] with separate X/Y scale factors (Z as X) and an optional base point (see
/// [`insert_entity`]).
pub(crate) fn insert_with(
    s: &mut Session,
    name: &str,
    at: Vec2,
    scale: Vec2,
    rotation: f64,
    base: Option<Vec2>,
    values: &serde_json::Map<String, Value>,
) -> Result<Handle> {
    insert_scaled(s, name, at, Vec3::new(scale.x, scale.y, scale.x), rotation, base, values)
}

/// [`insert_with`] with a Z scale factor of its own.
fn insert_scaled(
    s: &mut Session,
    name: &str,
    at: Vec2,
    scale: Vec3,
    rotation: f64,
    base: Option<Vec2>,
    values: &serde_json::Map<String, Value>,
) -> Result<Handle> {
    let blk = s.doc()?.block(name).cloned().ok_or_else(|| EngineError::Other(format!("Block \"{name}\" not found.")))?;
    let mut ins = insert_entity(&blk, at, scale, rotation, base);
    let m = ins.transform(blk.base.xy());
    for e in blk.entities.iter() {
        if let EntityKind::AttDef(ad) = &e.kind {
            if ad.constant {
                continue;
            }
            let mut t = ad.text.clone();
            let mut k = EntityKind::Text(t.clone());
            k.transform(&m);
            if let EntityKind::Text(tt) = k {
                t = tt;
            }
            t.value = attedit::value_for(values, &ad.tag).unwrap_or_else(|| ad.text.value.clone());
            ins.attribs.push(Attrib {
                tag: ad.tag.clone(),
                text: t,
                invisible: ad.invisible,
                constant: false,
                prompt: String::new(),
                props: Default::default(),
                multiline: ad.multiline.as_ref().map(|ml| ml.transformed(&m)),
                modes: ad.modes.for_reference(),
            });
        }
    }
    s.add_entity(EntityKind::Insert(ins))
}

fn run_block(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("block", "`name` is required"))?.to_string();
    let base = point_param(p, "base").unwrap_or(Vec2::ZERO);
    let hs = targets(s, p)?;
    let keep = keep_param("block", p, Keep::Convert)?;
    let description = str_param(p, "description").unwrap_or("");
    description::check_text("block", description)?;
    let r = make_block(s, &name, base, &hs, keep, description)?;
    // An annotative block: its references (the converted selection too) are annotative.
    if bool_or(p, "annotative", false) {
        let d = s.doc_mut()?;
        let cur = d.current_anno_scale();
        if let Some(b) = d.blocks.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(&name)).map(|(_, b)| b) {
            Arc::make_mut(b).annotative = true;
        }
        if let Some(h) = r {
            d.modify_entity(h, |e| cadcraft_doc::set_annotative(e, true, &cur))?;
        }
    }
    Ok(json!({ "block": name, "insert": r.map(|h| h.hex()) }))
}

fn run_insert(s: &mut Session, p: &Value) -> Result<Value> {
    let p = &insert_file::json_params(s, p)?;
    let name = str_param(p, "name").ok_or_else(|| bad("insert", "`name` is required"))?.to_string();
    let at = point_req("insert", p, "at")?;
    // `scale`: one factor, or [x, y] for different X and Y scale factors.
    let scale = match p.get("scale") {
        Some(v) if v.is_array() => point_value(v).ok_or_else(|| bad("insert", "`scale` must be a number or [x, y]"))?,
        _ => {
            let f = f64_or(p, "scale", 1.0);
            Vec2::new(f, f)
        }
    };
    if scale.x == 0.0 || scale.y == 0.0 {
        return Err(bad("insert", "scale cannot be 0"));
    }
    let rot = f64_or(p, "rotation", 0.0).to_radians();
    let vals = p.get("attribs").and_then(Value::as_object).cloned().unwrap_or_default();
    let h = insert_with(s, &name, at, scale, rot, point_param(p, "basePoint"), &vals)?;
    if bool_or(p, "explode", false) {
        // The EXPLODE command body, not `execute`: INSERT records the one undo step.
        let explode = find_command("explode").ok_or_else(|| EngineError::UnknownCommand("explode".into()))?;
        return (explode.run)(s, &json!({ "handles": [h.hex()] }));
    }
    Ok(attedit::with_unknown_tags(s, h, &vals, json!({ "handle": h.hex() })))
}

fn attdef_kind(s: &Session, tag: &str, prompt: &str, default: &str, at: Vec2, height: f64, invisible: bool) -> EntityKind {
    let style = s.doc().map(|d| d.header.str("TEXTSTYLE", "Standard")).unwrap_or_else(|_| "Standard".into());
    EntityKind::AttDef(Attrib {
        tag: tag.to_ascii_uppercase(),
        text: Text {
            insert: v3(at),
            align_pt: None,
            height,
            value: default.into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style,
            halign: HAlign::Left,
            valign: VAlign::Baseline,
        },
        invisible,
        constant: false,
        prompt: prompt.into(),
        props: Default::default(),
        multiline: None,
        modes: Default::default(),
    })
}

fn run_attdef(s: &mut Session, p: &Value) -> Result<Value> {
    let tag = str_param(p, "tag").ok_or_else(|| bad("attdef", "`tag` is required"))?;
    if tag.contains(' ') || tag.is_empty() {
        return Err(bad("attdef", "tag must be non-empty without spaces"));
    }
    let at = point_req("attdef", p, "at")?;
    let h = f64_or(p, "height", s.doc()?.header.f64("TEXTSIZE", 0.2));
    let k = attdef_kind(s, tag, str_param(p, "prompt").unwrap_or(tag), str_param(p, "default").unwrap_or(""), at, h, bool_or(p, "invisible", false));
    Ok(json!({ "handle": s.add_entity(k)?.hex() }))
}

fn run_wblock(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").ok_or_else(|| bad("wblock", "`path` is required"))?.to_string();
    let src = s.doc()?;
    let mut d = cadcraft_doc::Drawing::new_imperial();
    d.layers = src.layers.clone();
    d.linetypes = src.linetypes.clone();
    d.text_styles = src.text_styles.clone();
    d.dim_styles = src.dim_styles.clone();
    d.blocks = src.blocks.clone();
    // The base point becomes the written drawing's origin, its insertion base: by default 0,0 for
    // objects and the block's base point for a block (`base` is then in block coordinates).
    let base = point_param(p, "base");
    d.header.set("INSBASE", cadcraft_doc::HVal::Point(Vec3::ZERO));
    let m = Mat3::translate(-base.unwrap_or(Vec2::ZERO));
    let ents: Vec<Entity> = if let Some(name) = str_param(p, "name") {
        let b = src.block(name).ok_or_else(|| bad("wblock", format!("no block `{name}`")))?;
        let m = Mat3::translate(-base.unwrap_or(b.base.xy()));
        b.entities
            .iter()
            .map(|e| {
                let mut e = (**e).clone();
                e.kind.transform(&m);
                e
            })
            .collect()
    } else {
        targets(s, p)?
            .iter()
            .filter_map(|h| src.entity(*h))
            .map(|e| {
                let mut e = (**e).clone();
                e.kind.transform(&m);
                e
            })
            .collect()
    };
    for mut e in ents {
        e.handle = d.new_handle();
        d.model.push(e);
    }
    let hooks = super::file::io().ok_or_else(|| bad("wblock", "file formats are not available in this build"))?;
    let bytes = (hooks.write)(&d, &path).map_err(|e| bad("wblock", e))?;
    #[cfg(not(target_arch = "wasm32"))]
    std::fs::write(&path, &bytes).map_err(|e| bad("wblock", format!("{path}: {e}")))?;
    Ok(json!({ "path": path, "entities": d.model.len(), "bytes": bytes.len() }))
}

fn run_base(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_req("base", p, "at")?;
    s.doc_mut()?.header.set("INSBASE", cadcraft_doc::HVal::Point(v3(at)));
    ok()
}

fn run_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let block = |b: &Block| {
        let dynamic = cadcraft_doc::dynblock::dynamic_block(d, &b.name).is_some();
        json!({"name": b.name, "entities": b.entities.len(), "base": [b.base.x, b.base.y], "description": b.description, "dynamic": dynamic})
    };
    Ok(json!({ "blocks": d.blocks.values().filter(|b| !b.anonymous).map(|b| block(b)).collect::<Vec<_>>() }))
}

/// A dynamic block's parameters and actions as read from the file (null for a block that has
/// none).
fn run_dynamic(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("blocks.dynamic", "`name` is required"))?;
    let d = s.doc()?;
    if d.block(name).is_none() && !name.trim().eq_ignore_ascii_case("*Model_Space") {
        return Err(bad("blocks.dynamic", format!("no block `{name}`")));
    }
    serde_json::to_value(cadcraft_doc::dynblock::dynamic_block(d, name)).map_err(|e| bad("blocks.dynamic", e.to_string()))
}

/// The dynamic block a block reference shows, the values it was given and its custom properties
/// (null for any other reference).
fn run_reference(s: &mut Session, p: &Value) -> Result<Value> {
    let h = super::curves::handle_param(p, "handle").ok_or_else(|| bad("blocks.reference", "`handle` is required"))?;
    let d = s.doc()?;
    match d.entity(h).map(|e| &e.kind) {
        Some(EntityKind::Insert(_)) => {}
        Some(_) => return Err(bad("blocks.reference", "`handle` is not a block reference")),
        None => return Err(bad("blocks.reference", format!("no object {}", h.hex()))),
    }
    let (Some(r), Some((_, properties))) = (cadcraft_doc::dynblock::reference(d, h), cadcraft_doc::dynblock::custom_properties(d, h)) else {
        return Ok(Value::Null);
    };
    Ok(json!({ "definition": r.definition, "values": r.values, "properties": properties }))
}

/// Change a custom property of a dynamic block reference (one undo step).
fn run_reference_set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "blocks.reference.set";
    let h = super::curves::handle_param(p, "handle").ok_or_else(|| bad(cmd, "`handle` is required"))?;
    let name = str_param(p, "name").ok_or_else(|| bad(cmd, "`name` is required"))?;
    let value = match p.get("value") {
        Some(Value::String(v)) => v.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => return Err(bad(cmd, "`value` is required")),
    };
    if !matches!(s.doc()?.entity(h).map(|e| &e.kind), Some(EntityKind::Insert(_))) {
        return Err(bad(cmd, "`handle` is not a block reference"));
    }
    if super::curves::is_locked(s, h) {
        return Err(bad(cmd, "the reference is on a locked layer"));
    }
    let mut d = s.doc()?.clone();
    cadcraft_doc::dynblock::set_property(&mut d, h, name, &value).map_err(|e| bad(cmd, e))?;
    let properties = cadcraft_doc::dynblock::custom_properties(&d, h).map(|(_, p)| p).unwrap_or_default();
    *s.doc_mut()? = d;
    Ok(json!({ "properties": properties }))
}

// ---------------- interactive ----------------

/// -BLOCK's JSON form: BLOCK with the selection erased unless `keep` says otherwise.
fn run_dash_block(s: &mut Session, p: &Value) -> Result<Value> {
    let mut p = p.clone();
    if let Some(o) = p.as_object_mut() {
        o.entry("keep").or_insert_with(|| json!("delete"));
    }
    run_block(s, &p)
}

/// BLOCK / -BLOCK at the command line. BLOCK converts the selection to a reference to the new
/// block (the dialog's default); -BLOCK erases it.
#[derive(Default)]
struct BlockM {
    /// -BLOCK: erase the selection.
    dash: bool,
    /// A name already in use, waiting for the answer to "Redefine it?".
    existing: Option<String>,
    name: Option<String>,
    base: Option<Vec2>,
    sel: SelectPhase,
}

impl Interactive for BlockM {
    fn name(&self) -> &'static str {
        "BLOCK"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (&self.name, self.base) {
            (None, _) => match &self.existing {
                Some(n) => Prompt::new(format!("Block \"{n}\" already exists. Redefine it?"), super::curves::KW).kw(&["Yes", "No"]).default("N"),
                None => Prompt::new("Enter block name", Accept::TEXT).kw(&["?"]),
            },
            (Some(_), None) => Prompt::new("Specify insertion base point", Accept::POINT).kw(&["Annotative"]),
            _ => self.sel.prompt(),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (&self.name, self.base) {
            (None, _) if self.existing.is_some() => {
                // Yes redefines the block; No (the default) asks for another name.
                let yes = matches!(&i, Input::Keyword(k) | Input::Text(k) if k.trim().to_ascii_lowercase().starts_with('y'));
                self.name = self.existing.take().filter(|_| yes);
                Ok(Step::Continue)
            }
            (None, _) => {
                if let Input::Text(t) = i {
                    let n = t.trim().to_string();
                    if n == "?" {
                        let names: Vec<String> = s.doc()?.blocks.values().filter(|b| !b.anonymous).map(|b| b.name.clone()).collect();
                        s.echo(format!("Defined blocks: {}", if names.is_empty() { "(none)".into() } else { names.join(", ") }));
                    } else if !valid_block_name(&n) {
                        s.echo("Invalid block name.");
                    } else if s.doc()?.block(&n).is_some() {
                        self.existing = Some(n);
                    } else {
                        self.name = Some(n);
                    }
                }
                Ok(Step::Continue)
            }
            (Some(_), None) => {
                if let Input::Point(p) = i {
                    self.base = Some(p);
                    self.sel = SelectPhase::begin(s);
                    if self.sel.done {
                        let picked = self.sel.picked.clone();
                        return self.finish(s, &picked);
                    }
                }
                Ok(Step::Continue)
            }
            _ => match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Cancel),
                SelOutcome::Done(hs) => self.finish(s, &hs),
            },
        }
    }
}

impl BlockM {
    fn finish(&mut self, s: &mut Session, hs: &[Handle]) -> Result<Step> {
        let (Some(name), Some(base)) = (self.name.clone(), self.base) else { return Ok(Step::Cancel) };
        // A definition that can't be made (e.g. one that would reference itself) ends the command.
        let keep = if self.dash { Keep::Delete } else { Keep::Convert };
        if let Err(e) = make_block(s, &name, base, hs, keep, "") {
            s.echo(e.to_string());
            return Ok(Step::Done);
        }
        s.echo(format!("Block \"{name}\" defined with {} object(s).", hs.len()));
        Ok(Step::Done)
    }
}

mod insert_file;
pub(crate) mod xref;

/// Where the command-line INSERT is in its prompt sequence.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum InsStage {
    #[default]
    Name,
    /// A file whose block already exists: redefine it from the file?
    Redefine,
    Point,
    /// Basepoint: a new base point, picked on the block dropped where it was (at the cursor).
    BasePoint,
    /// Scale preset before the insertion point.
    PresetScale,
    /// X, Y or Z scale preset (0, 1, 2) before the insertion point: that axis alone.
    PresetAxis(usize),
    /// Rotate preset before the insertion point.
    PresetRotation,
    XScale,
    Corner,
    YScale,
    /// XYZ: the Z scale factor, after X and Y.
    ZScale,
    Rotation,
    Attribs,
}

#[derive(Default)]
struct InsertM {
    stage: InsStage,
    name: Option<String>,
    at: Option<Vec2>,
    base: Option<Vec2>,
    x_scale: Option<f64>,
    scale: Option<Vec2>,
    /// The Z scale factor when given (a preset or XYZ); otherwise the X factor's size. AutoCAD
    /// keeps a Z factor typed at XYZ's Z prompt as given, and stores the size of any other.
    z_scale: Option<f64>,
    /// X/Y/Z presets given so far.
    axes: [Option<f64>; 3],
    /// XYZ at the X scale prompt: X, Y and Z are asked in turn.
    xyz: bool,
    /// Where the block was dropped to pick a Basepoint on it.
    dropped: Option<Vec2>,
    rotation: Option<f64>,
    attdefs: Vec<(String, String, String)>,
    values: serde_json::Map<String, Value>,
    /// `*name`: insert the block's objects (one uniform scale factor, no attribute values).
    explode: bool,
    /// The file waiting for the Redefine answer.
    file: Option<insert_file::FileRef>,
}

impl InsertM {
    /// After the insertion point or a scale/rotation answer: the next prompt still needed.
    fn advance(&mut self, s: &mut Session) -> Result<Step> {
        self.stage = if self.scale.is_none() {
            if self.explode { InsStage::PresetScale } else { InsStage::XScale }
        } else if self.rotation.is_none() {
            InsStage::Rotation
        } else {
            InsStage::Attribs
        };
        if self.stage == InsStage::Attribs
            && self.values.len() >= self.attdefs.len()
            && let (Some(n), Some(a), Some(sc), Some(r)) = (self.name.clone(), self.at, self.scale, self.rotation)
        {
            let h = insert_scaled(s, &n, a, self.scale3(sc), r, self.base, &self.values)?;
            if self.explode {
                insert_file::explode_insert(s, h)?;
            }
            return Ok(Step::Done);
        }
        Ok(Step::Continue)
    }

    /// Block `name` chosen: collect its attribute prompts and ask for the insertion point.
    fn choose(&mut self, s: &mut Session, name: &str) -> Result<()> {
        let Some(b) = s.doc()?.block(name).cloned() else {
            s.echo(format!("Block \"{name}\" not found."));
            return Ok(());
        };
        self.attdefs = if self.explode {
            Vec::new()
        } else {
            b.entities
                .iter()
                .filter_map(|e| {
                    if let EntityKind::AttDef(a) = &e.kind {
                        (!a.constant).then(|| (a.tag.clone(), a.prompt.clone(), a.text.value.clone()))
                    } else {
                        None
                    }
                })
                .collect()
        };
        s.doc_mut()?.header.set_str("INSNAME", &b.name);
        self.name = Some(b.name.clone());
        self.stage = InsStage::Point;
        Ok(())
    }

    /// Define the block from a file, then choose it; a file that can't be read is reported.
    fn choose_file(&mut self, s: &mut Session, f: &insert_file::FileRef) -> Result<()> {
        match f.define(s) {
            Ok(name) => self.choose(s, &name),
            Err(e) => {
                s.echo(e.to_string());
                Ok(())
            }
        }
    }

    /// X/Y scale factors with Z: as given, else the X factor's size (AutoCAD 2024: X -1 with Y
    /// as X stores Z +1).
    /// X and Y from a corner: XYZ still asks Z, which defaults to the size of X.
    fn corner_scale(&mut self, s: &mut Session, sc: Vec2) -> Result<Step> {
        self.scale = Some(sc);
        if self.xyz {
            self.x_scale = Some(sc.x);
            self.stage = InsStage::ZScale;
            return Ok(Step::Continue);
        }
        self.advance(s)
    }

    fn scale3(&self, sc: Vec2) -> Vec3 {
        Vec3::new(sc.x, sc.y, self.z_scale.unwrap_or(sc.x.abs()))
    }

    /// Scale factors from an opposite corner: the X and Y distances from the insertion point.
    fn corner(a: Vec2, p: Vec2) -> Result<Vec2> {
        let d = p - a;
        if d.x.abs() < 1e-12 || d.y.abs() < 1e-12 {
            return Err(EngineError::Other("The corner must not be level with the insertion point.".into()));
        }
        Ok(d)
    }
}

fn nonzero(t: &str) -> Result<f64> {
    number(t).filter(|v| *v != 0.0).ok_or_else(|| EngineError::Other("Requires a non-zero number.".into()))
}

impl Interactive for InsertM {
    fn name(&self) -> &'static str {
        "INSERT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let at = self.at.unwrap_or_default();
        match self.stage {
            InsStage::Name => {
                let last = s.doc().map(|d| d.header.str("INSNAME", "")).unwrap_or_default();
                let p = Prompt::new("Enter block name", Accept::TEXT).kw(&["?"]);
                if last.is_empty() { p } else { p.default(last) }
            }
            InsStage::Redefine => {
                let name = self.file.as_ref().map(|f| f.name.clone()).unwrap_or_default();
                Prompt::new(format!("Block \"{name}\" already exists. Redefine it?"), super::curves::KW).kw(&["Yes", "No"]).default("N")
            }
            InsStage::Point => Prompt::new("Specify insertion point", Accept::POINT).kw(&["Basepoint", "Scale", "X", "Y", "Z", "Rotate"]),
            InsStage::BasePoint => Prompt::new("Specify base point", Accept::POINT),
            InsStage::PresetScale => Prompt::new("Specify scale factor for XYZ axes", Accept::NUMBER).default("1"),
            InsStage::PresetAxis(i) => Prompt::new(format!("Specify {} scale factor", ["X", "Y", "Z"][i.min(2)]), Accept::NUMBER).default("1"),
            InsStage::PresetRotation => Prompt::new("Specify rotation angle", Accept::NUMBER).default("0"),
            InsStage::XScale if self.xyz => Prompt::new("Specify X scale factor", Accept::POINT_OR_NUMBER).kw(&["Corner"]).default("1").base(at),
            InsStage::XScale => {
                Prompt::new("Enter X scale factor, specify opposite corner", Accept::POINT_OR_NUMBER).kw(&["Corner", "XYZ"]).default("1").base(at)
            }
            InsStage::Corner => Prompt::new("Specify opposite corner", Accept::POINT).base(at),
            InsStage::YScale => Prompt::new("Enter Y scale factor", Accept::NUMBER).default("use X scale factor"),
            InsStage::ZScale => Prompt::new("Specify Z scale factor or", Accept::NUMBER).default("use X scale factor"),
            InsStage::Rotation => Prompt::new("Specify rotation angle", Accept::POINT_OR_NUMBER).default("0").base(at),
            InsStage::Attribs => {
                let (_, prompt, default) = self.attdefs.get(self.values.len()).cloned().unwrap_or_default();
                Prompt::new(if prompt.is_empty() { "Enter attribute value".into() } else { prompt }, Accept::TEXT).default(default)
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.stage {
            InsStage::Name => {
                let n = match i {
                    Input::Text(t) => t.trim().to_string(),
                    Input::Enter => s.doc()?.header.str("INSNAME", ""),
                    _ => return Ok(Step::Continue),
                };
                if n == "?" {
                    let names: Vec<String> = s.doc()?.blocks.values().filter(|b| !b.anonymous).map(|b| b.name.clone()).collect();
                    s.echo(format!("Defined blocks: {}", names.join(", ")));
                    return Ok(Step::Continue);
                }
                // `*name`: insert exploded.
                self.explode = n.starts_with('*');
                let n = n.strip_prefix('*').map_or(n.as_str(), str::trim).to_string();
                match insert_file::file_ref(s, &n) {
                    Some(f) if f.ask_redefine(s) => {
                        self.file = Some(f);
                        self.stage = InsStage::Redefine;
                    }
                    Some(f) => self.choose_file(s, &f)?,
                    None => self.choose(s, &n)?,
                }
            }
            InsStage::Redefine => {
                // Yes defines the block from the file; No (the default) inserts the existing block.
                let Some(f) = self.file.take() else { return Ok(Step::Continue) };
                let yes = matches!(&i, Input::Keyword(k) | Input::Text(k) if k.trim().to_ascii_lowercase().starts_with('y'));
                if yes {
                    self.choose_file(s, &f)?;
                } else {
                    self.choose(s, &f.name)?;
                }
            }
            InsStage::Point => match i {
                Input::Point(p) => {
                    self.at = Some(p);
                    return self.advance(s);
                }
                Input::Keyword(k) if k == "Basepoint" => {
                    // AutoCAD drops the block where it is being dragged, at the cursor (the origin
                    // in a script: 2024 inserted (15, 5) for base point (612350.678, 2398770.432)
                    // and insertion point (612365.678, 2398775.432)).
                    self.dropped = Some(s.cursor);
                    self.stage = InsStage::BasePoint;
                }
                Input::Keyword(k) if k == "Scale" => self.stage = InsStage::PresetScale,
                Input::Keyword(k) if k == "X" => self.stage = InsStage::PresetAxis(0),
                Input::Keyword(k) if k == "Y" => self.stage = InsStage::PresetAxis(1),
                Input::Keyword(k) if k == "Z" => self.stage = InsStage::PresetAxis(2),
                Input::Keyword(k) if k == "Rotate" => self.stage = InsStage::PresetRotation,
                _ => {}
            },
            InsStage::BasePoint => {
                if let Input::Point(p) = i {
                    // From the dropped block to its own coordinates.
                    let def = s.doc()?.block(self.name.as_deref().unwrap_or_default()).map_or(Vec2::ZERO, |b| b.base.xy());
                    self.base = Some(def + (p - self.dropped.unwrap_or(def)));
                    self.stage = InsStage::Point;
                }
            }
            InsStage::PresetScale => {
                let f = match i {
                    Input::Text(t) => nonzero(&t)?,
                    Input::Enter => 1.0,
                    _ => return Ok(Step::Continue),
                };
                self.scale = Some(Vec2::new(f, f));
                self.z_scale = Some(f.abs());
                // An axis preset after it changes that axis alone (Scale -3 then X 2: 2/-3/3).
                self.axes = [Some(f), Some(f), Some(f.abs())];
                // Exploded insertion asks its one scale factor after the insertion point.
                if self.at.is_some() {
                    return self.advance(s);
                }
                self.stage = InsStage::Point;
            }
            // One axis preset: the others are 1, and no scale factors are asked after the point.
            InsStage::PresetAxis(ax) => {
                let f = match i {
                    Input::Text(t) => nonzero(&t)?,
                    Input::Enter => 1.0,
                    _ => return Ok(Step::Continue),
                };
                if let Some(a) = self.axes.get_mut(ax) {
                    *a = Some(f);
                }
                let [x, y, z] = self.axes.map(|a| a.unwrap_or(1.0));
                self.scale = Some(Vec2::new(x, y));
                self.z_scale = Some(z.abs());
                self.stage = InsStage::Point;
            }
            InsStage::PresetRotation => {
                self.rotation = Some(match i {
                    Input::Text(t) => s.angle_settings().direction(&t).ok_or_else(|| EngineError::Other("Requires an angle.".into()))?,
                    Input::Enter => 0.0,
                    _ => return Ok(Step::Continue),
                });
                self.stage = InsStage::Point;
            }
            InsStage::XScale => match i {
                Input::Keyword(k) if k == "Corner" => self.stage = InsStage::Corner,
                Input::Keyword(k) if k == "XYZ" => self.xyz = true,
                Input::Point(p) => {
                    let sc = Self::corner(self.at.unwrap_or_default(), p)?;
                    return self.corner_scale(s, sc);
                }
                Input::Text(t) => {
                    self.x_scale = Some(nonzero(&t)?);
                    self.stage = InsStage::YScale;
                }
                Input::Enter => {
                    self.x_scale = Some(1.0);
                    self.stage = InsStage::YScale;
                }
                _ => {}
            },
            InsStage::Corner => {
                if let Input::Point(p) = i {
                    let sc = Self::corner(self.at.unwrap_or_default(), p)?;
                    return self.corner_scale(s, sc);
                }
            }
            InsStage::YScale => {
                let x = self.x_scale.unwrap_or(1.0);
                let y = match i {
                    Input::Text(t) => nonzero(&t)?,
                    Input::Enter => x,
                    _ => return Ok(Step::Continue),
                };
                self.scale = Some(Vec2::new(x, y));
                if self.xyz {
                    self.stage = InsStage::ZScale;
                    return Ok(Step::Continue);
                }
                return self.advance(s);
            }
            InsStage::ZScale => {
                self.z_scale = Some(match i {
                    Input::Text(t) => nonzero(&t)?,
                    Input::Enter => self.x_scale.unwrap_or(1.0).abs(),
                    _ => return Ok(Step::Continue),
                });
                return self.advance(s);
            }
            InsStage::Rotation => {
                let a = self.at.unwrap_or_default();
                self.rotation = Some(match i {
                    Input::Point(p) => a.angle_to(p),
                    Input::Text(t) => s.angle_settings().direction(&t).ok_or_else(|| EngineError::Other("Requires an angle.".into()))?,
                    _ => 0.0,
                });
                return self.advance(s);
            }
            InsStage::Attribs => {
                let (tag, _, default) = self.attdefs.get(self.values.len()).cloned().unwrap_or_default();
                let v = match i {
                    Input::Text(t) => t,
                    _ => default,
                };
                self.values.insert(tag, Value::String(v));
                return self.advance(s);
            }
        }
        Ok(Step::Continue)
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        let (Some(n), Ok(d)) = (&self.name, s.doc()) else { return Vec::new() };
        let Some(b) = d.block(n) else { return Vec::new() };
        // Picking a new base point: the block sits at its definition position.
        if self.stage == InsStage::BasePoint {
            return vec![EntityKind::Insert(insert_entity(b, self.dropped.unwrap_or(b.base.xy()), Vec3::new(1.0, 1.0, 1.0), 0.0, None))];
        }
        let at = self.at.unwrap_or(c);
        let scale = self.scale.unwrap_or_else(|| match (self.stage, self.x_scale) {
            (InsStage::XScale | InsStage::Corner, _) => Self::corner(at, c).unwrap_or(Vec2::new(1.0, 1.0)),
            (_, Some(x)) => Vec2::new(x, x),
            _ => Vec2::new(1.0, 1.0),
        });
        let rot = self.rotation.unwrap_or(if self.stage == InsStage::Rotation { at.angle_to(c) } else { 0.0 });
        vec![EntityKind::Insert(insert_entity(b, at, self.scale3(scale), rot, self.base))]
    }
}

#[derive(Default)]
struct AttdefM {
    tag: Option<String>,
    prompt: Option<String>,
    default: Option<String>,
}

impl Interactive for AttdefM {
    fn name(&self) -> &'static str {
        "ATTDEF"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (&self.tag, &self.prompt, &self.default) {
            (None, ..) => Prompt::new("Enter attribute tag name", Accept::TEXT),
            (Some(_), None, _) => Prompt::new("Enter attribute prompt", Accept::TEXT),
            (Some(_), Some(_), None) => Prompt::new("Enter default attribute value", Accept::TEXT),
            _ => Prompt::new("Specify start point of text", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let text = match &i {
            Input::Text(t) => Some(t.clone()),
            Input::Enter => Some(String::new()),
            _ => None,
        };
        match (&self.tag, &self.prompt, &self.default) {
            (None, ..) => {
                if let Some(t) = text.filter(|t| !t.trim().is_empty() && !t.contains(' ')) {
                    self.tag = Some(t.to_ascii_uppercase());
                }
            }
            (Some(_), None, _) => self.prompt = text,
            (Some(_), Some(_), None) => self.default = text,
            (Some(tag), Some(pr), Some(def)) => {
                if let Input::Point(p) = i {
                    let h = s.doc()?.header.f64("TEXTSIZE", 0.2);
                    let k = attdef_kind(s, tag, pr, def, p, h, false);
                    s.add_entity(k)?;
                    return Ok(Step::Done);
                }
            }
        }
        Ok(Step::Continue)
    }
}

mod attribs;
mod battman;
mod blockreplace;
