//! Blocks: BLOCK, INSERT, ATTDEF, ATTEDIT (programmatic), WBLOCK, BASE, PURGE, block listing.

use std::sync::Arc;

use cadcraft_doc::{Attrib, Block, Entity, EntityKind, HAlign, Handle, Insert, Text, VAlign};
use cadcraft_geom::{Mat3, Vec2, Vec3};
use serde_json::{Value, json};

use super::helpers::v3;
use super::machines::{SelOutcome, SelectPhase, number};
use super::*;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("block", "Make...", run_block)
            .menu(&["Draw", "Block", "Make..."])
            .alias(&["b", "-block", "bmake"])
            .params("{name, base: [x,y], handles?, keep?: \"convert\"|\"retain\"|\"delete\", description?}")
            .interactive(|_| Ok(Box::new(BlockM::default()))),
        CommandSpec::new("insert", "Block...", run_insert)
            .menu(&["Insert", "Block..."])
            .alias(&["i", "-insert", "ddinsert"])
            .params("{name, at: [x,y], scale?, rotation? (degrees), attribs?: {TAG: value}, explode?: bool}")
            .interactive(|_| Ok(Box::new(InsertM::default()))),
        CommandSpec::new("attdef", "Define Attributes...", run_attdef)
            .menu(&["Draw", "Block", "Define Attributes..."])
            .alias(&["att", "-attdef"])
            .params("{tag, prompt?, default?, at, height?, invisible?}")
            .interactive(|_| Ok(Box::new(AttdefM::default()))),
        CommandSpec::new("attedit", "Single...", run_attedit)
            .menu(&["Modify", "Object", "Attribute", "Single..."])
            .alias(&["ate", "eattedit"])
            .params("{handle, values: {TAG: value}}"),
        CommandSpec::new("wblock", "Write Block", run_wblock).alias(&["w"]).params("{path, name? | handles?, base?}"),
        CommandSpec::new("base", "Base", run_base).menu(&["Draw", "Block", "Base"]).params("{at: [x,y]}"),
        CommandSpec::new("purge", "Purge", run_purge).alias(&["pu", "-purge"]).params("{} (unused blocks, layers, linetypes, styles)"),
        CommandSpec::new("blocks.list", "List Blocks", run_list).enabled(has_doc).noundo(),
        CommandSpec::new("battman", "Block Attribute Manager...", run_battman)
            .menu(&["Modify", "Object", "Attribute", "Block Attribute Manager..."])
            .params("{name}")
            .noundo(),
    ]
}

fn valid_block_name(n: &str) -> bool {
    !n.trim().is_empty() && n.len() <= 255 && !n.starts_with('*') && !n.chars().any(|c| "<>/\\\":;?*|,=`".contains(c))
}

/// The block an entity references, when it is a block reference.
fn referenced_block(e: &Entity) -> Option<String> {
    if let EntityKind::Insert(i) = &e.kind { Some(i.block.clone()) } else { None }
}

/// Whether any of `ents` refers to block `name`, directly or through nested block references.
fn references_block(d: &cadcraft_doc::Drawing, ents: &[Entity], name: &str) -> bool {
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

/// Create a block from entities. Returns the insert handle when converting.
pub(crate) fn make_block(s: &mut Session, name: &str, base: Vec2, hs: &[Handle], keep: &str, description: &str) -> Result<Option<Handle>> {
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
    if keep != "retain" {
        for h in hs {
            doc.remove_entity(*h);
        }
    }
    s.set_selection(Vec::new());
    if keep == "convert" {
        let h = insert(s, name, base, 1.0, 0.0, &serde_json::Map::new())?;
        return Ok(Some(h));
    }
    Ok(None)
}

/// Insert a block reference, filling attributes from `values` (else their defaults).
pub(crate) fn insert(s: &mut Session, name: &str, at: Vec2, scale: f64, rotation: f64, values: &serde_json::Map<String, Value>) -> Result<Handle> {
    let blk = s.doc()?.block(name).cloned().ok_or_else(|| EngineError::Other(format!("Block \"{name}\" not found.")))?;
    let mut ins = Insert {
        block: blk.name.clone(),
        insert: v3(at),
        scale: Vec3::new(scale, scale, scale),
        rotation,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
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
            t.value = values.get(&ad.tag).and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| ad.text.value.clone());
            ins.attribs.push(Attrib { tag: ad.tag.clone(), text: t, invisible: ad.invisible, constant: false, prompt: String::new() });
        }
    }
    s.add_entity(EntityKind::Insert(ins))
}

fn run_block(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("block", "`name` is required"))?.to_string();
    let base = point_param(p, "base").unwrap_or(Vec2::ZERO);
    let hs = targets(s, p)?;
    let keep = str_param(p, "keep").unwrap_or("convert").to_string();
    let r = make_block(s, &name, base, &hs, &keep, str_param(p, "description").unwrap_or(""))?;
    Ok(json!({ "block": name, "insert": r.map(|h| h.hex()) }))
}

fn run_insert(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("insert", "`name` is required"))?.to_string();
    let at = point_req("insert", p, "at")?;
    let scale = f64_or(p, "scale", 1.0);
    if scale == 0.0 {
        return Err(bad("insert", "scale cannot be 0"));
    }
    let rot = f64_or(p, "rotation", 0.0).to_radians();
    let vals = p.get("attribs").and_then(Value::as_object).cloned().unwrap_or_default();
    let h = insert(s, &name, at, scale, rot, &vals)?;
    if bool_or(p, "explode", false) {
        return s.execute("explode", &json!({ "handles": [h.hex()] }));
    }
    Ok(json!({ "handle": h.hex() }))
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

fn run_attedit(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("attedit", "`handle` is required"))?;
    let vals = p.get("values").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut n = 0;
    s.doc_mut()?.modify_entity(h, |e| {
        if let EntityKind::Insert(i) = &mut e.kind {
            for a in &mut i.attribs {
                if let Some(v) = vals.get(&a.tag).and_then(Value::as_str) {
                    a.text.value = v.to_string();
                    n += 1;
                }
            }
        }
    })?;
    Ok(json!({ "changed": n }))
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
    let base = point_param(p, "base").unwrap_or(Vec2::ZERO);
    let m = Mat3::translate(-base);
    let ents: Vec<Entity> = if let Some(name) = str_param(p, "name") {
        let b = src.block(name).ok_or_else(|| bad("wblock", format!("no block `{name}`")))?;
        let m = Mat3::translate(-b.base.xy());
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

fn run_purge(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc_mut()?;
    // Blocks referenced anywhere (including nested references), iterated to a fixed point.
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    let all: Vec<Arc<Entity>> = d.model.iter().cloned().chain(d.layouts.iter().flat_map(|l| l.entities.iter().cloned())).collect();
    let mut frontier: Vec<String> =
        all.iter().filter_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.block.clone()) } else { None }).collect();
    for e in &all {
        if let EntityKind::Dimension(dm) = &e.kind
            && let Some(b) = &dm.block
        {
            frontier.push(b.clone());
        }
    }
    let mut guard = 0;
    while let Some(n) = frontier.pop() {
        guard += 1;
        if guard > 100_000 || !used.insert(n.to_ascii_uppercase()) {
            continue;
        }
        if let Some(b) = d.block(&n) {
            for e in b.entities.iter() {
                if let EntityKind::Insert(i) = &e.kind {
                    frontier.push(i.block.clone());
                }
            }
        }
    }
    let before_b = d.blocks.len();
    d.blocks.retain(|k, _| used.contains(&k.to_ascii_uppercase()));
    let blocks = before_b - d.blocks.len();
    // Layers used by the model, layouts and the block definitions that survived above.
    let used_layers = super::layer::used_layers(d);
    let cur = d.header.str("CLAYER", "0").to_ascii_lowercase();
    let before_l = d.layers.len();
    d.layers.retain(|l| {
        l.name == "0"
            || l.name.eq_ignore_ascii_case("Defpoints")
            || l.name.to_ascii_lowercase() == cur
            || used_layers.contains(&l.name.to_ascii_lowercase())
    });
    let layers = before_l - d.layers.len();
    let used_lt: std::collections::HashSet<String> =
        all.iter().map(|e| e.common.linetype.to_ascii_lowercase()).chain(d.layers.iter().map(|l| l.linetype.to_ascii_lowercase())).collect();
    let before_t = d.linetypes.len();
    d.linetypes.retain(|l| {
        ["byblock", "bylayer", "continuous"].contains(&l.name.to_ascii_lowercase().as_str()) || used_lt.contains(&l.name.to_ascii_lowercase())
    });
    let linetypes = before_t - d.linetypes.len();
    Ok(
        json!({ "blocks": blocks, "layers": layers, "linetypes": linetypes, "message": format!("{blocks} blocks, {layers} layers, {linetypes} linetypes purged.") }),
    )
}

fn run_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    Ok(
        json!({ "blocks": d.blocks.values().filter(|b| !b.anonymous).map(|b| json!({"name": b.name, "entities": b.entities.len(), "base": [b.base.x, b.base.y], "description": b.description})).collect::<Vec<_>>() }),
    )
}

fn run_battman(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("battman", "`name` is required"))?;
    let b = s.doc()?.block(name).cloned().ok_or_else(|| bad("battman", "no such block"))?;
    let atts: Vec<Value> = b
        .entities
        .iter()
        .filter_map(|e| {
            if let EntityKind::AttDef(a) = &e.kind {
                Some(json!({"tag": a.tag, "prompt": a.prompt, "default": a.text.value, "invisible": a.invisible}))
            } else {
                None
            }
        })
        .collect();
    Ok(json!({ "block": name, "attributes": atts }))
}

// ---------------- interactive ----------------

#[derive(Default)]
struct BlockM {
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
            (None, _) => Prompt::new("Enter block name", Accept::TEXT).kw(&["?"]),
            (Some(_), None) => Prompt::new("Specify insertion base point", Accept::POINT).kw(&["Annotative"]),
            _ => self.sel.prompt(),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (&self.name, self.base) {
            (None, _) => {
                if let Input::Text(t) = i {
                    let n = t.trim().to_string();
                    if n == "?" {
                        let names: Vec<String> = s.doc()?.blocks.values().filter(|b| !b.anonymous).map(|b| b.name.clone()).collect();
                        s.echo(format!("Defined blocks: {}", if names.is_empty() { "(none)".into() } else { names.join(", ") }));
                    } else if !valid_block_name(&n) {
                        s.echo("Invalid block name.");
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
        if let Err(e) = make_block(s, &name, base, hs, "convert", "") {
            s.echo(e.to_string());
            return Ok(Step::Done);
        }
        s.echo(format!("Block \"{name}\" defined with {} object(s).", hs.len()));
        Ok(Step::Done)
    }
}

#[derive(Default)]
struct InsertM {
    name: Option<String>,
    at: Option<Vec2>,
    scale: Option<f64>,
    rotation: Option<f64>,
    attdefs: Vec<(String, String, String)>,
    values: serde_json::Map<String, Value>,
}

impl Interactive for InsertM {
    fn name(&self) -> &'static str {
        "INSERT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let last = s.doc().map(|d| d.header.str("INSNAME", "")).unwrap_or_default();
        match (&self.name, self.at, self.scale, self.rotation) {
            (None, ..) => {
                let p = Prompt::new("Enter block name", Accept::TEXT).kw(&["?"]);
                if last.is_empty() { p } else { p.default(last) }
            }
            (Some(_), None, ..) => Prompt::new("Specify insertion point", Accept::POINT).kw(&["Basepoint", "Scale", "Rotate"]),
            (Some(_), Some(a), None, _) => Prompt::new("Enter scale factor", Accept::POINT_OR_NUMBER).default("1").base(a),
            (Some(_), Some(a), Some(_), None) => Prompt::new("Specify rotation angle", Accept::POINT_OR_NUMBER).default("0").base(a),
            _ => {
                let (tag, prompt, default) = self.attdefs.get(self.values.len()).cloned().unwrap_or_default();
                let _ = tag;
                Prompt::new(if prompt.is_empty() { "Enter attribute value".into() } else { prompt }, Accept::TEXT).default(default)
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (&self.name, self.at, self.scale, self.rotation) {
            (None, ..) => {
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
                let Some(b) = s.doc()?.block(&n).cloned() else {
                    s.echo(format!("Block \"{n}\" not found."));
                    return Ok(Step::Continue);
                };
                self.attdefs = b
                    .entities
                    .iter()
                    .filter_map(|e| {
                        if let EntityKind::AttDef(a) = &e.kind {
                            (!a.constant).then(|| (a.tag.clone(), a.prompt.clone(), a.text.value.clone()))
                        } else {
                            None
                        }
                    })
                    .collect();
                s.doc_mut()?.header.set_str("INSNAME", &b.name);
                self.name = Some(b.name.clone());
            }
            (Some(_), None, ..) => {
                if let Input::Point(p) = i {
                    self.at = Some(p);
                }
            }
            (Some(_), Some(a), None, _) => {
                self.scale = Some(match i {
                    Input::Point(p) => a.dist(p).max(1e-9),
                    Input::Text(t) => number(&t).filter(|v| *v != 0.0).ok_or_else(|| EngineError::Other("Requires a non-zero number.".into()))?,
                    _ => 1.0,
                });
            }
            (Some(_), Some(a), Some(_), None) => {
                self.rotation = Some(match i {
                    Input::Point(p) => a.angle_to(p),
                    Input::Text(t) => crate::units::parse_angle(&t).ok_or_else(|| EngineError::Other("Requires an angle.".into()))?,
                    _ => 0.0,
                });
            }
            _ => {
                let (tag, _, default) = self.attdefs.get(self.values.len()).cloned().unwrap_or_default();
                let v = match i {
                    Input::Text(t) => t,
                    _ => default,
                };
                self.values.insert(tag, Value::String(v));
            }
        }
        if let (Some(n), Some(a), Some(sc), Some(r)) = (self.name.clone(), self.at, self.scale, self.rotation)
            && self.values.len() >= self.attdefs.len()
        {
            insert(s, &n, a, sc, r, &self.values)?;
            return Ok(Step::Done);
        }
        Ok(Step::Continue)
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        let (Some(n), Ok(d)) = (&self.name, s.doc()) else { return Vec::new() };
        let Some(b) = d.block(n) else { return Vec::new() };
        let at = self.at.unwrap_or(c);
        let sc = self.scale.unwrap_or(if self.at.is_some() { at.dist(c).max(1e-9) } else { 1.0 });
        let rot = self.rotation.unwrap_or(if self.scale.is_some() { at.angle_to(c) } else { 0.0 });
        vec![EntityKind::Insert(Insert {
            block: b.name.clone(),
            insert: v3(at),
            scale: Vec3::new(sc, sc, sc),
            rotation: rot,
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        })]
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
