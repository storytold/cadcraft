//! ATTEDIT / EATTEDIT: one block reference's attribute values, asked for one after another.
//! -ATTEDIT: the attributes picked by block name, tag and value specifications, edited one at a
//! time (value, position, height, angle, text style, layer, colour) or by replacing a string in
//! all of them. Attribute tags match in any case, as in AutoCAD.

use cadcraft_doc::color::Color;
use cadcraft_doc::{Attrib, AttribProps, Common, EntityKind, HAlign, Handle, Text, VAlign};
use cadcraft_geom::Vec2;
use serde_json::{Map, Value, json};

use crate::cmd::curves::KW;
use crate::cmd::machines::{SelOutcome, SelectPhase, number};
use crate::cmd::qselect::wildcard;
use crate::cmd::{CommandSpec, bad, point_param, str_param, targets};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// Longest attribute value a string replacement may produce (bytes).
const MAX_VALUE: usize = 65_536;

pub(super) fn attedit_spec() -> CommandSpec {
    CommandSpec::new("attedit", "Single...", run)
        .menu(&["Modify", "Object", "Attribute", "Single..."])
        .alias(&["ate", "eattedit"])
        .params("{handle, values: {TAG: value} (tags in any case)} → {changed, unknownTags?}")
        .interactive(|_| Ok(Box::new(AtteditM::default())))
}

pub(super) fn dash_attedit_spec() -> CommandSpec {
    CommandSpec::new("-attedit", "Global", run_global)
        .menu(&["Modify", "Object", "Attribute", "Global"])
        .alias(&["-ate", "atte"])
        .params("{find?, replace?, position?: [x,y], height?, angle? (degrees), style? (find or one of position / height / angle / style is required), block?, tag?, value? (wildcard specifications, default *), handles? (default: every block reference in the current space)} → {changed}")
        .interactive(|_| Ok(Box::new(DashAtteditM::default())))
}

/// The value `values` gives for `tag`, matching the tag in any case. Numbers and booleans count
/// as their text.
pub(super) fn value_for(values: &Map<String, Value>, tag: &str) -> Option<String> {
    let v = values.get(tag).or_else(|| values.iter().find(|(k, _)| k.eq_ignore_ascii_case(tag)).map(|(_, v)| v))?;
    match v {
        Value::String(t) => Some(t.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// `r` with `unknownTags`: the keys of `values` that block reference `h` has no attribute for
/// (left out when there are none).
pub(super) fn with_unknown_tags(s: &Session, h: Handle, values: &Map<String, Value>, mut r: Value) -> Value {
    let tags: Vec<&str> = match s.doc().ok().and_then(|d| d.entity(h)).map(|e| &e.kind) {
        Some(EntityKind::Insert(i)) => i.attribs.iter().map(|a| a.tag.as_str()).collect(),
        _ => Vec::new(),
    };
    let unknown: Vec<&String> = values.keys().filter(|k| !tags.iter().any(|t| t.eq_ignore_ascii_case(k))).collect();
    if !unknown.is_empty()
        && let Some(o) = r.as_object_mut()
    {
        o.insert("unknownTags".into(), json!(unknown));
    }
    r
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("attedit", "`handle` is required"))?;
    let vals = p.get("values").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut n = 0;
    s.doc_mut()?.modify_entity(h, |e| {
        if let EntityKind::Insert(i) = &mut e.kind {
            for a in &mut i.attribs {
                if let Some(v) = value_for(&vals, &a.tag) {
                    set_value(&mut a.text, v);
                    n += 1;
                }
            }
        }
    })?;
    Ok(with_unknown_tags(s, h, &vals, json!({ "changed": n })))
}

/// A block name, tag or value specification: comma-separated wildcard patterns, any case. For
/// values, `\` matches an empty value.
fn spec_matches(spec: &str, text: &str) -> bool {
    spec.split(',').any(|p| if p.trim() == "\\" { text.is_empty() } else { wildcard(p.trim(), text) })
}

/// Specifications choosing attributes: block name, tag, value.
#[derive(Clone, Debug)]
struct Specs {
    block: String,
    tag: String,
    value: String,
}

impl Default for Specs {
    fn default() -> Self {
        Specs { block: "*".into(), tag: "*".into(), value: "*".into() }
    }
}

/// The attributes of block references `hs` that match `specs`, as (block reference, attribute
/// index). Invisible attributes count only with `invisible` (or while ATTMODE shows them).
fn matching(s: &Session, hs: &[Handle], specs: &Specs, invisible: bool) -> Result<Vec<(Handle, usize)>> {
    let d = s.doc()?;
    let invisible = invisible || d.header.i64("ATTMODE", 1) == 2;
    let mut out = Vec::new();
    for h in hs {
        let Some(EntityKind::Insert(ins)) = d.entity(*h).map(|e| &e.kind) else { continue };
        if !spec_matches(&specs.block, &ins.block) {
            continue;
        }
        for (k, a) in ins.attribs.iter().enumerate() {
            if !a.constant && (invisible || !a.invisible) && spec_matches(&specs.tag, &a.tag) && spec_matches(&specs.value, &a.text.value) {
                out.push((*h, k));
            }
        }
    }
    Ok(out)
}

/// Every object in the current space.
fn all_in_space(s: &Session) -> Result<Vec<Handle>> {
    let d = s.doc()?;
    Ok(d.space(&s.space()).map(|st| st.iter().map(|e| e.handle).collect()).unwrap_or_default())
}

/// Change attribute `k` of block reference `h`; returns whether its value changed.
fn edit(s: &mut Session, (h, k): (Handle, usize), f: impl FnOnce(&str) -> Option<String>) -> Result<bool> {
    let mut changed = false;
    s.doc_mut()?.modify_entity(h, |e| {
        if let EntityKind::Insert(ins) = &mut e.kind
            && let Some(a) = ins.attribs.get_mut(k)
            && let Some(v) = f(&a.text.value).filter(|v| *v != a.text.value)
        {
            set_value(&mut a.text, v);
            changed = true;
        }
    })?;
    Ok(changed)
}

/// Refresh the derived baseline after a value change, even one within the writer's
/// imported-font tolerance. Equal XY points request recomputation on export; retain
/// both elevations and the independent endpoints used by Aligned/Fit text.
fn set_value(text: &mut Text, value: String) {
    if text.value == value {
        return;
    }
    text.value = value;
    rebase(text);
}

/// Justified text is placed by its alignment point: equal XY points request the start point's
/// recomputation on export. Left-baseline, aligned and fitted text keep their start point.
fn rebase(text: &mut Text) {
    if !matches!((text.halign, text.valign), (HAlign::Left, VAlign::Baseline) | (HAlign::Aligned | HAlign::Fit, _))
        && let Some(anchor) = text.align_pt
    {
        text.insert.x = anchor.x;
        text.insert.y = anchor.y;
    }
}

/// Change the properties set on attribute `k` of block reference `h`.
fn set_props(s: &mut Session, (h, k): (Handle, usize), f: impl FnOnce(&mut AttribProps)) -> Result<()> {
    s.doc_mut()?.modify_entity(h, |e| {
        if let EntityKind::Insert(ins) = &mut e.kind
            && let Some(a) = ins.attribs.get_mut(k)
        {
            f(&mut a.props);
        }
    })?;
    Ok(())
}

/// A change to an attribute's text that -ATTEDIT makes one at a time.
#[derive(Clone, Debug, PartialEq)]
enum Reshape {
    /// Move it so the point it is placed by (its justification point) lands here.
    Position(Vec2),
    Height(f64),
    /// Rotation in radians counterclockwise from +X.
    Angle(f64),
    /// A text style the drawing has (its name as the drawing spells it).
    Style(String),
}

/// Apply `r` to attribute `a`: its text and, for a multiline attribute, its multiline text.
fn reshape(a: &mut Attrib, r: &Reshape) {
    let t = &mut a.text;
    let m = a.multiline.as_mut().map(|m| &mut m.mtext);
    match r {
        Reshape::Position(p) => {
            let at = t.justify_point();
            let (dx, dy) = (p.x - at.x, p.y - at.y);
            if !(dx.is_finite() && dy.is_finite()) {
                return;
            }
            t.insert.x += dx;
            t.insert.y += dy;
            if let Some(ap) = &mut t.align_pt {
                ap.x += dx;
                ap.y += dy;
            }
            if let Some(m) = m {
                m.insert.x += dx;
                m.insert.y += dy;
            }
        }
        Reshape::Height(h) => {
            t.height = *h;
            if let Some(m) = m {
                m.height = *h;
            }
        }
        Reshape::Angle(r) => {
            // Aligned and fitted text run from their start point to their end point: turn the
            // end point about the start point.
            if matches!(t.halign, HAlign::Aligned | HAlign::Fit)
                && let Some(ap) = &mut t.align_pt
            {
                let len = t.insert.xy().dist(ap.xy());
                let (x, y) = (t.insert.x + len * r.cos(), t.insert.y + len * r.sin());
                if x.is_finite() && y.is_finite() {
                    ap.x = x;
                    ap.y = y;
                }
            }
            t.rotation = *r;
            if let Some(m) = m {
                m.rotation = *r;
            }
        }
        Reshape::Style(n) => {
            t.style = n.clone();
            if let Some(m) = m {
                m.style = n.clone();
            }
        }
    }
    rebase(t);
}

/// Apply `r` to attribute `k` of block reference `h`; returns whether it changed.
fn reshape_at(s: &mut Session, (h, k): (Handle, usize), r: &Reshape) -> Result<bool> {
    let mut changed = false;
    s.doc_mut()?.modify_entity(h, |e| {
        if let EntityKind::Insert(ins) = &mut e.kind
            && let Some(a) = ins.attribs.get_mut(k)
        {
            let before = (a.text.clone(), a.multiline.clone());
            reshape(a, r);
            changed = before != (a.text.clone(), a.multiline.clone());
        }
    })?;
    Ok(changed)
}

/// The text of attribute `k` of block reference `h`.
fn current_text(s: &Session, (h, k): (Handle, usize)) -> Option<Text> {
    match &s.doc().ok()?.entity(h)?.kind {
        EntityKind::Insert(ins) => ins.attribs.get(k).map(|a| a.text.clone()),
        _ => None,
    }
}

/// A text style the drawing has, as the drawing spells its name.
fn style_named(s: &Session, name: &str) -> Result<String> {
    let name = name.trim();
    s.doc()?.text_style(name).map(|st| st.name.clone()).ok_or_else(|| EngineError::Other(format!("Cannot find text style \"{name}\".")))
}

/// The reshapes the JSON form asks for: `position`, `height`, `angle` (degrees), `style`.
fn json_reshapes(s: &Session, p: &Value) -> Result<Vec<Reshape>> {
    let cmd = "-attedit";
    let mut v = Vec::new();
    if p.get("position").is_some() {
        v.push(Reshape::Position(point_param(p, "position").ok_or_else(|| bad(cmd, "`position` must be a point [x, y]"))?));
    }
    if let Some(h) = p.get("height") {
        let h = h.as_f64().filter(|h| h.is_finite() && *h > 0.0).ok_or_else(|| bad(cmd, "`height` must be a positive number"))?;
        v.push(Reshape::Height(h));
    }
    if let Some(a) = p.get("angle") {
        let a = a.as_f64().map(f64::to_radians).filter(|a| a.is_finite()).ok_or_else(|| bad(cmd, "`angle` must be a number of degrees"))?;
        v.push(Reshape::Angle(a));
    }
    if let Some(n) = p.get("style") {
        let n = n.as_str().ok_or_else(|| bad(cmd, "`style` must be a text style name"))?;
        v.push(Reshape::Style(style_named(s, n).map_err(|e| bad(cmd, e.to_string()))?));
    }
    Ok(v)
}

/// The layer and colour attribute `k` of block reference `h` is drawn with: its own, else its
/// definition's (layer 0 is the reference's layer).
fn current_props(s: &Session, (h, k): (Handle, usize)) -> Option<(String, Color)> {
    let d = s.doc().ok()?;
    let e = d.entity(h)?;
    let EntityKind::Insert(ins) = &e.kind else { return None };
    let a = ins.attribs.get(k)?;
    let def = d
        .block(&ins.block)
        .and_then(|b| b.entities.iter().find(|be| matches!(&be.kind, EntityKind::AttDef(ad) if ad.tag.eq_ignore_ascii_case(&a.tag))));
    let c = a.props.over(&def.map(|de| de.common.clone()).unwrap_or_else(Common::default));
    let layer = if c.layer == "0" { e.common.layer.clone() } else { c.layer };
    Some((layer, c.color))
}

/// Replace every `find` in `value` with `with`, unless the result would be too long.
fn replaced(value: &str, find: &str, with: &str) -> Option<String> {
    if find.is_empty() || !value.contains(find) {
        return None;
    }
    let n = value.matches(find).count();
    let len = value.len().checked_sub(n.checked_mul(find.len())?)?.checked_add(n.checked_mul(with.len())?)?;
    (len <= MAX_VALUE).then(|| value.replace(find, with))
}

/// Replace `find` with `with` in the given attributes; returns how many changed.
fn replace_all(s: &mut Session, items: &[(Handle, usize)], find: &str, with: &str) -> Result<usize> {
    let mut n = 0;
    for it in items {
        if edit(s, *it, |v| replaced(v, find, with))? {
            n += 1;
        }
    }
    Ok(n)
}

fn run_global(s: &mut Session, p: &Value) -> Result<Value> {
    let reshapes = json_reshapes(s, p)?;
    let find = match p.get("find") {
        None | Some(Value::Null) if !reshapes.is_empty() => None,
        _ => Some(
            str_param(p, "find")
                .filter(|f| !f.is_empty())
                .ok_or_else(|| bad("-attedit", "`find` (a non-empty string) is required, unless position, height, angle or style is given"))?
                .to_string(),
        ),
    };
    let with = str_param(p, "replace").unwrap_or("").to_string();
    let spec = |k| str_param(p, k).unwrap_or("*").to_string();
    let specs = Specs { block: spec("block"), tag: spec("tag"), value: spec("value") };
    let hs = if p.get("handles").is_some() || p.get("handle").is_some() { targets(s, p)? } else { all_in_space(s)? };
    let items = matching(s, &hs, &specs, true)?;
    let mut n = 0usize;
    for it in &items {
        let mut changed = match &find {
            Some(f) => edit(s, *it, |v| replaced(v, f, &with))?,
            None => false,
        };
        for r in &reshapes {
            changed |= reshape_at(s, *it, r)?;
        }
        n += usize::from(changed);
    }
    Ok(json!({ "changed": n }))
}

// ---------------- ATTEDIT / EATTEDIT ----------------

#[derive(Default)]
struct AtteditM {
    block: Option<Handle>,
    /// Index, tag and current value of each editable attribute of the picked block reference.
    attribs: Vec<(usize, String, String)>,
    /// The values answered so far, in attribute order.
    values: Vec<String>,
}

impl AtteditM {
    fn pick(&mut self, s: &Session, hs: &[Handle]) -> Result<()> {
        let d = s.doc()?;
        let mut msg = "Select a block reference.";
        for h in hs {
            let Some(EntityKind::Insert(ins)) = d.entity(*h).map(|e| &e.kind) else { continue };
            let atts: Vec<(usize, String, String)> =
                ins.attribs.iter().enumerate().filter(|(_, a)| !a.constant).map(|(k, a)| (k, a.tag.clone(), a.text.value.clone())).collect();
            if atts.is_empty() {
                msg = "That block reference has no editable attributes.";
                continue;
            }
            self.block = Some(*h);
            self.attribs = atts;
            return Ok(());
        }
        Err(EngineError::Other(msg.into()))
    }
}

impl Interactive for AtteditM {
    fn name(&self) -> &'static str {
        "ATTEDIT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.attribs.get(self.values.len()) {
            Some((_, tag, cur)) if self.block.is_some() => Prompt::new(format!("Enter value for {tag}"), Accept::TEXT).default(cur.clone()),
            _ => Prompt::new("Select block reference", Accept::SELECT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Some(h) = self.block else {
            return match i {
                Input::Pick(hs) => self.pick(s, &hs).map(|_| Step::Continue),
                Input::Enter => Ok(Step::Done),
                Input::Text(_) | Input::Keyword(_) => Err(EngineError::Other("Select a block reference.".into())),
                _ => Ok(Step::Continue),
            };
        };
        let cur = self.attribs.get(self.values.len()).map(|a| a.2.clone()).unwrap_or_default();
        self.values.push(match i {
            Input::Text(t) => t,
            Input::Enter => cur,
            _ => return Ok(Step::Continue),
        });
        if self.values.len() < self.attribs.len() {
            return Ok(Step::Continue);
        }
        // All answered: apply them together (one undo step).
        for ((k, _, _), v) in self.attribs.iter().zip(&self.values) {
            edit(s, (h, *k), |_| Some(v.clone()))?;
        }
        Ok(Step::Done)
    }
}

// ---------------- -ATTEDIT ----------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Stage {
    #[default]
    OneAtATime,
    VisibleOnly,
    BlockSpec,
    TagSpec,
    ValueSpec,
    Select,
    /// One at a time: what to do with the current attribute.
    Option,
    ValueKind,
    NewValue,
    Position,
    Height,
    Angle,
    Style,
    Layer,
    Color,
    TrueColor,
    Find,
    Replace,
}

#[derive(Default)]
struct DashAtteditM {
    stage: Stage,
    one_at_a_time: bool,
    visible_only: bool,
    specs: Specs,
    sel: SelectPhase,
    items: Vec<(Handle, usize)>,
    /// The attribute being edited one at a time (index into `items`).
    cur: usize,
    find: String,
}

/// A colour name, number or `r,g,b` (typed numbers are answers, not distances).
const NAME: Accept = Accept { number: true, ..Accept::TEXT };

fn invalid_keyword() -> EngineError {
    EngineError::Other("Invalid option keyword.".into())
}

impl DashAtteditM {
    fn yes(i: &Input) -> Result<bool> {
        match i {
            Input::Keyword(k) => Ok(k == "Yes"),
            Input::Enter => Ok(true),
            _ => Err(invalid_keyword()),
        }
    }
    /// The attributes are chosen: start editing them.
    fn chosen(&mut self, s: &mut Session, hs: &[Handle]) -> Result<Step> {
        self.items = matching(s, hs, &self.specs, !self.visible_only && !self.one_at_a_time)?;
        s.set_selection(Vec::new());
        let n = self.items.len();
        s.echo(format!("{n} attribute{} selected.", if n == 1 { "" } else { "s" }));
        if n == 0 {
            return Ok(Step::Done);
        }
        if self.one_at_a_time {
            self.cur = 0;
            self.show_current(s);
            self.stage = Stage::Option;
        } else {
            self.stage = Stage::Find;
        }
        Ok(Step::Continue)
    }
    fn show_current(&self, s: &mut Session) {
        let Some((h, k)) = self.items.get(self.cur).copied() else { return };
        if let Ok(d) = s.doc()
            && let Some(EntityKind::Insert(ins)) = d.entity(h).map(|e| &e.kind)
            && let Some(a) = ins.attribs.get(k)
        {
            let line = format!("{} = {}", a.tag, a.text.value);
            s.echo(line);
        }
    }
}

impl Interactive for DashAtteditM {
    fn name(&self) -> &'static str {
        "-ATTEDIT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let cur = self.items.get(self.cur).and_then(|it| current_props(s, *it));
        match self.stage {
            Stage::OneAtATime => Prompt::new("Edit attributes one at a time?", KW).kw(&["Yes", "No"]).default("Y"),
            Stage::VisibleOnly => Prompt::new("Edit only attributes visible on screen?", KW).kw(&["Yes", "No"]).default("Y"),
            Stage::BlockSpec => Prompt::new("Enter block name specification", Accept::TEXT).default("*"),
            Stage::TagSpec => Prompt::new("Enter attribute tag specification", Accept::TEXT).default("*"),
            Stage::ValueSpec => Prompt::new("Enter attribute value specification", Accept::TEXT).default("*"),
            Stage::Select => Prompt::new("Select attributes", Accept::SELECT),
            Stage::Option => {
                Prompt::new("Enter an option", KW).kw(&["Value", "Position", "Height", "Angle", "Style", "Layer", "Color", "Next"]).default("N")
            }
            Stage::ValueKind => Prompt::new("Enter type of value modification", KW).kw(&["Change", "Replace"]).default("R"),
            Stage::NewValue => Prompt::new("Enter new attribute value", Accept::TEXT),
            Stage::Position => Prompt::new("Specify new text insertion point", Accept::POINT).default("no change"),
            Stage::Height | Stage::Angle => {
                let t = self.items.get(self.cur).and_then(|it| current_text(s, *it));
                let height = self.stage == Stage::Height;
                let msg = if height { "Specify new height" } else { "Specify new rotation angle" };
                let cur = t.as_ref().map(|t| {
                    if height {
                        format!("{:.4}", t.height)
                    } else {
                        let (au, ap) = s.doc().map(|d| (d.header.i64("AUNITS", 0), d.header.i64("AUPREC", 0))).unwrap_or((0, 0));
                        crate::units::format_angle(s.angle_settings().shown(t.rotation), au, ap)
                    }
                });
                let mut p = Prompt::new(msg, Accept::POINT_OR_NUMBER);
                if let Some(t) = &t {
                    p = p.base(t.justify_point());
                }
                match cur {
                    Some(c) => p.default(c),
                    None => p,
                }
            }
            Stage::Style => {
                let p = Prompt::new("Enter new text style", Accept::TEXT);
                match self.items.get(self.cur).and_then(|it| current_text(s, *it)) {
                    Some(t) => p.default(t.style),
                    None => p,
                }
            }
            Stage::Layer => {
                let p = Prompt::new("Enter new layer name", Accept::TEXT);
                match cur {
                    Some((layer, _)) => p.default(layer),
                    None => p,
                }
            }
            Stage::Color => {
                let p = Prompt::new("Enter new color", NAME).kw(&["Truecolor", "COlorbook"]);
                match cur {
                    Some((_, c)) => p.default(c.name()),
                    None => p,
                }
            }
            Stage::TrueColor => Prompt::new("Red, Green, Blue", NAME),
            Stage::Find => Prompt::new("Enter string to change", Accept::TEXT),
            Stage::Replace => Prompt::new("Enter new string", Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        // Free-text answers; Enter gives "" (the specifications then take their default "*").
        let text = match &i {
            Input::Text(t) => Some(t.clone()),
            Input::Enter => Some(String::new()),
            _ => None,
        };
        match self.stage {
            Stage::OneAtATime => {
                self.one_at_a_time = Self::yes(&i)?;
                self.stage = if self.one_at_a_time {
                    Stage::BlockSpec
                } else {
                    s.echo("Performing global editing of attribute values.");
                    Stage::VisibleOnly
                };
            }
            Stage::VisibleOnly => {
                self.visible_only = Self::yes(&i)?;
                self.stage = Stage::BlockSpec;
            }
            Stage::BlockSpec | Stage::TagSpec | Stage::ValueSpec => {
                let Some(t) = text else { return Ok(Step::Continue) };
                let t = if t.trim().is_empty() { "*".to_string() } else { t.trim().to_string() };
                match self.stage {
                    Stage::BlockSpec => {
                        self.specs.block = t;
                        self.stage = Stage::TagSpec;
                    }
                    Stage::TagSpec => {
                        self.specs.tag = t;
                        self.stage = Stage::ValueSpec;
                    }
                    _ => {
                        self.specs.value = t;
                        if !self.one_at_a_time && !self.visible_only {
                            let hs = all_in_space(s)?;
                            return self.chosen(s, &hs);
                        }
                        self.stage = Stage::Select;
                    }
                }
            }
            Stage::Select => {
                return match self.sel.feed(s, &i)? {
                    SelOutcome::More => Ok(Step::Continue),
                    SelOutcome::Empty => self.chosen(s, &[]),
                    SelOutcome::Done(hs) => self.chosen(s, &hs),
                };
            }
            Stage::Option => match &i {
                Input::Keyword(k) if k == "Value" => self.stage = Stage::ValueKind,
                Input::Keyword(k) if k == "Position" => self.stage = Stage::Position,
                Input::Keyword(k) if k == "Height" => self.stage = Stage::Height,
                Input::Keyword(k) if k == "Angle" => self.stage = Stage::Angle,
                Input::Keyword(k) if k == "Style" => self.stage = Stage::Style,
                Input::Keyword(k) if k == "Layer" => self.stage = Stage::Layer,
                Input::Keyword(k) if k == "Color" => self.stage = Stage::Color,
                Input::Keyword(_) | Input::Enter => {
                    self.cur += 1;
                    if self.cur >= self.items.len() {
                        return Ok(Step::Done);
                    }
                    self.show_current(s);
                }
                Input::Text(_) => return Err(invalid_keyword()),
                _ => {}
            },
            Stage::Position | Stage::Height | Stage::Angle | Stage::Style => {
                // Enter keeps the attribute as it is; a refused answer asks again.
                let at = self.items.get(self.cur).and_then(|it| current_text(s, *it)).map(|t| t.justify_point()).unwrap_or(Vec2::ZERO);
                let r = match (self.stage, &i) {
                    (_, Input::Enter) => None,
                    (Stage::Position, Input::Point(p)) => Some(Reshape::Position(*p)),
                    (Stage::Height, Input::Point(p)) => Some(Reshape::Height(at.dist(*p))),
                    (Stage::Angle, Input::Point(p)) => Some(Reshape::Angle(at.angle_to(*p))),
                    // A distance, as TEXT takes its height (1'6" in feet and inches, too).
                    (Stage::Height, Input::Text(t)) => Some(Reshape::Height(number(t).unwrap_or(f64::NAN))),
                    (Stage::Angle, Input::Text(t)) => Some(Reshape::Angle(s.angle_settings().direction(t).unwrap_or(f64::NAN))),
                    (Stage::Style, Input::Text(t)) if t.trim().is_empty() => None,
                    (Stage::Style, Input::Text(t)) => Some(Reshape::Style(style_named(s, t)?)),
                    _ => return Ok(Step::Continue),
                };
                match &r {
                    Some(Reshape::Height(h)) if !(h.is_finite() && *h > 0.0) => {
                        return Err(EngineError::Other("Requires a positive height.".into()));
                    }
                    Some(Reshape::Angle(a)) if !a.is_finite() => return Err(EngineError::Other("Requires an angle.".into())),
                    _ => {}
                }
                if let (Some(r), Some(it)) = (r, self.items.get(self.cur).copied()) {
                    reshape_at(s, it, &r)?;
                }
                self.stage = Stage::Option;
            }
            Stage::ValueKind => match &i {
                Input::Keyword(k) if k == "Change" => self.stage = Stage::Find,
                Input::Keyword(_) | Input::Enter => self.stage = Stage::NewValue,
                _ => return Err(invalid_keyword()),
            },
            Stage::NewValue => {
                let Some(v) = text else { return Ok(Step::Continue) };
                if let Some(it) = self.items.get(self.cur).copied() {
                    edit(s, it, |_| Some(v))?;
                }
                self.show_current(s);
                self.stage = Stage::Option;
            }
            Stage::Layer => {
                let Some(t) = text else { return Ok(Step::Continue) };
                let t = t.trim();
                if !t.is_empty() {
                    let name = s.doc()?.layer(t).map(|l| l.name.clone()).ok_or_else(|| EngineError::Other(format!("Cannot find layer \"{t}\".")))?;
                    if let Some(it) = self.items.get(self.cur).copied() {
                        set_props(s, it, |p| p.layer = Some(name))?;
                    }
                }
                self.stage = Stage::Option;
            }
            Stage::Color | Stage::TrueColor => {
                if self.stage == Stage::Color
                    && let Input::Keyword(k) = &i
                {
                    if k == "Truecolor" {
                        self.stage = Stage::TrueColor;
                        return Ok(Step::Continue);
                    }
                    return Err(EngineError::Other("Color books are not available yet.".into()));
                }
                let Some(t) = text else { return Ok(Step::Continue) };
                if !t.trim().is_empty() {
                    let c = match (self.stage, Color::parse(&t)) {
                        (Stage::TrueColor, Some(c @ Color::True(_))) | (Stage::Color, Some(c)) => c,
                        (Stage::TrueColor, _) => return Err(EngineError::Other("Enter three values from 0 to 255, separated by commas.".into())),
                        _ => return Err(EngineError::Other("Enter a color name or a number from 0 to 256.".into())),
                    };
                    if let Some(it) = self.items.get(self.cur).copied() {
                        set_props(s, it, |p| p.color = Some(c))?;
                    }
                }
                self.stage = Stage::Option;
            }
            Stage::Find => match text {
                Some(t) if !t.is_empty() => {
                    self.find = t;
                    self.stage = Stage::Replace;
                }
                Some(_) => return Err(EngineError::Other("Requires the string to change.".into())),
                None => {}
            },
            Stage::Replace => {
                let Some(with) = text else { return Ok(Step::Continue) };
                let find = std::mem::take(&mut self.find);
                if self.one_at_a_time {
                    let items: Vec<(Handle, usize)> = self.items.get(self.cur).copied().into_iter().collect();
                    replace_all(s, &items, &find, &with)?;
                    self.show_current(s);
                    self.stage = Stage::Option;
                } else {
                    let n = replace_all(s, &self.items, &find, &with)?;
                    s.echo(format!("{n} attribute{} changed.", if n == 1 { "" } else { "s" }));
                    return Ok(Step::Done);
                }
            }
        }
        Ok(Step::Continue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_is_bounded_and_specs_take_lists() {
        assert_eq!(replaced("a-b-c", "-", "+").as_deref(), Some("a+b+c"));
        assert_eq!(replaced("abc", "x", "y"), None);
        assert_eq!(replaced("aaaa", "a", &"x".repeat(MAX_VALUE)), None);
        assert!(spec_matches("door,WIN*", "window"));
        assert!(spec_matches("\\", ""));
        assert!(!spec_matches("\\", "x"));
    }
}
