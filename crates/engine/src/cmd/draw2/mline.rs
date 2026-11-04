//! Multilines: MLINE draws one (Justification, Scale, STyle), MLSTYLE keeps the styles, MLEDIT
//! edits crossings, tees, corners, vertices and breaks.

use cadcraft_color::Color;
use cadcraft_doc::{
    Entity, EntityKind, Handle, MAX_MLINE_CUTS, MAX_MLINE_ELEMENTS, MLine, MLineCaps, MLineCut, MLineElement, MLineJust, MLineStyle, MLineVertex,
    STANDARD_MLINE_STYLE,
};
use cadcraft_geom::{Line, Vec2, line_line_infinite};
use serde_json::{Value, json};

use super::super::machines::number;
use super::super::{distinct_points, ok};
use super::curves::{self, MAX_GEN};
use super::{CommandSpec, bad, bool_or, f64_or, point_req, points_param, str_param};

use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("mline", "Multiline", run_mline)
            .menu(&["Draw", "Multiline"])
            .alias(&["ml"])
            .params("{points, scale?, justification?: top|zero|bottom, style?, closed?} → {handle}")
            .interactive(|s| Ok(Box::new(MlineM::new(s)))),
        CommandSpec::new("mlstyle", "Multiline Style...", run_mlstyle).menu(&["Format", "Multiline Style..."]).params(
            "{} → styles | {name, description?, elements?: [{offset, color?, linetype?}], fill?: color|false, joints?, \
                 startCaps?/endCaps?: {line?, outerArc?, innerArcs?, angle? (degrees)}, copyFrom?, current? (default true)} | \
                 {delete: name} | {name, rename: newName}",
        ),
        CommandSpec::new("mledit", "Multiline...", run_mledit)
            .menu(&["Modify", "Object", "Multiline..."])
            .alias(&["-mledit"])
            .params(
                "{tool: cc|oc|mc|ct|ot|cj, first, p1, second, p2} | {tool: av|dv, handle, point} | \
                 {tool: cs|ca|wa, handle, p1, p2} (handles of multilines; points pick segments and sides)",
            )
            .interactive(|_| Ok(Box::new(MleditM::default()))),
    ]
}

fn other(m: impl Into<String>) -> EngineError {
    EngineError::Other(m.into())
}

// =====================================================================================
// MLINE
// =====================================================================================

fn just_of(s: &str) -> Option<MLineJust> {
    match s.trim().to_ascii_lowercase().as_str() {
        "t" | "top" => Some(MLineJust::Top),
        "z" | "zero" => Some(MLineJust::Zero),
        "b" | "bottom" => Some(MLineJust::Bottom),
        _ => None,
    }
}

/// CMLJUST and CMLSCALE (20 in metric drawings when unset).
fn mline_defaults(s: &Session) -> (MLineJust, f64) {
    let d = s.doc().ok();
    let metric = d.is_some_and(|d| d.header.i64("MEASUREMENT", 0) == 1);
    let scale = d.map(|d| d.header.f64("CMLSCALE", if metric { 20.0 } else { 1.0 })).filter(|v| v.is_finite()).unwrap_or(1.0);
    (MLineJust::from_dxf(d.map(|d| d.header.i64("CMLJUST", 0)).unwrap_or(0)), scale)
}

fn style_named(s: &Session, name: &str) -> Result<MLineStyle> {
    s.doc()?.mline_style(name).cloned().ok_or_else(|| other(format!("Cannot find multiline style \"{}\".", name.trim())))
}

fn make(def: &MLineStyle, pts: &[Vec2], closed: bool, just: MLineJust, scale: f64) -> Option<EntityKind> {
    (pts.len() >= 2).then(|| EntityKind::MLine(MLine::new(def, pts, closed, just, scale)))
}

fn run_mline(s: &mut Session, p: &Value) -> Result<Value> {
    let pts = distinct_points(&points_param(p, "points").ok_or_else(|| bad("mline", "`points` (2 or more) is required"))?);
    if pts.len() < 2 || pts.len() > MAX_GEN {
        return Err(bad("mline", "need 2 or more distinct points"));
    }
    let (dj, ds) = mline_defaults(s);
    let j = match str_param(p, "justification") {
        Some(t) => just_of(t).ok_or_else(|| bad("mline", "justification must be top, zero or bottom"))?,
        None => dj,
    };
    let scale = f64_or(p, "scale", ds);
    if scale.abs() > 1e12 {
        return Err(bad("mline", "scale is out of range"));
    }
    let def = match str_param(p, "style") {
        Some(n) => style_named(s, n)?,
        None => s.doc()?.current_mline_style(),
    };
    let closed = bool_or(p, "closed", false) && pts.len() >= 3;
    let k = make(&def, &pts, closed, j, scale).ok_or_else(|| bad("mline", "need 2 or more points"))?;
    Ok(json!({ "handle": s.add_entity(k)?.hex() }))
}

struct MlineM {
    pts: Vec<Vec2>,
    just: MLineJust,
    scale: f64,
    style: MLineStyle,
    asking: u8, // 0 none, 1 justification, 2 scale, 3 style
}

impl MlineM {
    fn new(s: &Session) -> Self {
        let (just, scale) = mline_defaults(s);
        let style = s.doc().map(|d| d.current_mline_style()).unwrap_or_default();
        MlineM { pts: Vec::new(), just, scale, style, asking: 0 }
    }
    fn finish(&self, s: &mut Session, closed: bool) -> Result<Step> {
        if let Some(k) = make(&self.style, &self.pts, closed && self.pts.len() >= 3, self.just, self.scale) {
            s.add_entity(k)?;
        }
        Ok(Step::Done)
    }
    fn settings(&self, s: &mut Session) {
        s.echo(format!("Current settings: Justification = {}, Scale = {:.2}, Style = {}", self.just.name(), self.scale, self.style.name));
    }
}

impl Interactive for MlineM {
    fn name(&self) -> &'static str {
        "MLINE"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.settings(s);
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.asking {
            1 => return Prompt::new("Enter justification type", curves::KW).kw(&["Top", "Zero", "Bottom"]).default(self.just.name().to_lowercase()),
            2 => return Prompt::new("Enter mline scale", Accept::NUMBER).default(format!("{:.2}", self.scale)),
            3 => return Prompt::new("Enter mline style name or [?]", Accept::TEXT).default(self.style.name.clone()),
            _ => {}
        }
        match self.pts.len() {
            0 => Prompt::new("Specify start point", Accept::POINT).kw(&["Justification", "Scale", "STyle"]),
            1 => Prompt::new("Specify next point", Accept::POINT).kw(&["Undo"]).base_opt(self.pts.last().copied()),
            2 => Prompt::new("Specify next point", Accept::POINT).kw(&["Undo"]).base_opt(self.pts.last().copied()),
            _ => Prompt::new("Specify next point", Accept::POINT).kw(&["Close", "Undo"]).base_opt(self.pts.last().copied()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.asking {
            1 => {
                if let Input::Keyword(k) | Input::Text(k) = &i {
                    self.just = just_of(k).ok_or_else(|| other("Enter Top, Zero or Bottom."))?;
                    s.doc_mut()?.header.set_i64("CMLJUST", self.just.to_dxf());
                }
                self.asking = 0;
                self.settings(s);
                return Ok(Step::Continue);
            }
            2 => {
                if let Input::Text(t) = &i {
                    self.scale = number(t).filter(|v| v.is_finite() && v.abs() <= 1e12).ok_or_else(|| other("Requires a number."))?;
                    s.doc_mut()?.header.set_f64("CMLSCALE", self.scale);
                }
                self.asking = 0;
                self.settings(s);
                return Ok(Step::Continue);
            }
            3 => {
                if let Input::Text(t) | Input::Keyword(t) = &i {
                    let t = t.trim();
                    if t == "?" {
                        let names: Vec<String> = s.doc()?.mline_styles.iter().map(|st| st.name.clone()).collect();
                        s.echo(format!("Loaded multiline styles: {}", names.join(", ")));
                        return Ok(Step::Continue);
                    }
                    if !t.is_empty() {
                        self.style = style_named(s, t)?;
                        s.doc_mut()?.header.set_str("CMLSTYLE", &self.style.name);
                    }
                }
                self.asking = 0;
                self.settings(s);
                return Ok(Step::Continue);
            }
            _ => {}
        }
        match i {
            Input::Keyword(k) => {
                match k.as_str() {
                    "Justification" => self.asking = 1,
                    "Scale" => self.asking = 2,
                    "STyle" => self.asking = 3,
                    "Undo" => {
                        self.pts.pop();
                    }
                    "Close" if self.pts.len() >= 3 => return self.finish(s, true),
                    _ => {}
                }
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                if self.pts.last().is_none_or(|l| !l.near(p, 1e-12)) && self.pts.len() < MAX_GEN {
                    self.pts.push(p);
                }
                Ok(Step::Continue)
            }
            Input::Enter => {
                if self.pts.len() >= 2 {
                    self.finish(s, false)
                } else {
                    Ok(Step::Cancel)
                }
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.pts.is_empty() || self.asking != 0 {
            return Vec::new();
        }
        let mut pts = self.pts.clone();
        if pts.last().is_some_and(|l| !l.near(c, 1e-12)) {
            pts.push(c);
        }
        make(&self.style, &pts, false, self.just, self.scale).into_iter().collect()
    }
}

// =====================================================================================
// MLSTYLE
// =====================================================================================

fn mline_in_use(d: &cadcraft_doc::Drawing, name: &str) -> bool {
    let uses = |e: &std::sync::Arc<Entity>| matches!(&e.kind, EntityKind::MLine(m) if m.style.eq_ignore_ascii_case(name));
    d.model.iter().any(uses) || d.layouts.iter().any(|l| l.entities.iter().any(uses)) || d.blocks.values().any(|b| b.entities.iter().any(uses))
}

fn caps_param(p: &Value, key: &str, mut c: MLineCaps) -> Result<MLineCaps> {
    let Some(v) = p.get(key).filter(|v| !v.is_null()) else { return Ok(c) };
    if !v.is_object() {
        return Err(bad("mlstyle", format!("`{key}` must be an object")));
    }
    c.line = bool_or(v, "line", c.line);
    c.outer_arc = bool_or(v, "outerArc", c.outer_arc);
    c.inner_arcs = bool_or(v, "innerArcs", c.inner_arcs);
    if let Some(a) = v.get("angle").and_then(Value::as_f64) {
        if !(10.0..=170.0).contains(&a) {
            return Err(bad("mlstyle", "cap angles must be 10..170 degrees"));
        }
        c.angle = a.to_radians();
    }
    Ok(c)
}

fn color_value(v: &Value) -> Option<Color> {
    match v {
        Value::String(t) => Color::parse(t),
        Value::Number(n) => n.as_u64().and_then(|i| u16::try_from(i).ok()).and_then(|i| Color::parse(&i.to_string())),
        _ => None,
    }
}

fn elements_param(p: &Value) -> Result<Option<Vec<MLineElement>>> {
    let Some(v) = p.get("elements").filter(|v| !v.is_null()) else { return Ok(None) };
    let list = v.as_array().ok_or_else(|| bad("mlstyle", "`elements` must be an array"))?;
    if list.is_empty() || list.len() > MAX_MLINE_ELEMENTS {
        return Err(bad("mlstyle", format!("a style has 1 to {MAX_MLINE_ELEMENTS} elements")));
    }
    let mut out = Vec::new();
    for e in list {
        let offset = e
            .get("offset")
            .and_then(Value::as_f64)
            .filter(|o| o.is_finite() && o.abs() < 1e12)
            .ok_or_else(|| bad("mlstyle", "each element needs a finite `offset`"))?;
        let color = match e.get("color") {
            None | Some(Value::Null) => Color::ByLayer,
            Some(c) => color_value(c).ok_or_else(|| bad("mlstyle", format!("unknown colour {c}")))?,
        };
        let linetype = str_param(e, "linetype").map(str::trim).filter(|t| !t.is_empty()).unwrap_or("ByLayer").to_string();
        out.push(MLineElement { offset, color, linetype });
    }
    Ok(Some(out))
}

fn styles_value(d: &cadcraft_doc::Drawing) -> Value {
    json!({ "current": d.header.str("CMLSTYLE", STANDARD_MLINE_STYLE), "styles": serde_json::to_value(&d.mline_styles).unwrap_or(Value::Null) })
}

fn run_mlstyle(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(name) = str_param(p, "delete").map(str::trim) {
        let d = s.doc()?;
        let st = d.mline_style(name).ok_or_else(|| other(format!("Cannot find multiline style \"{name}\".")))?.name.clone();
        if st.eq_ignore_ascii_case(STANDARD_MLINE_STYLE) || st.eq_ignore_ascii_case(&d.header.str("CMLSTYLE", STANDARD_MLINE_STYLE)) {
            return Err(other(format!("The STANDARD and the current multiline style can't be deleted ({st}).")));
        }
        if mline_in_use(d, &st) {
            return Err(other(format!("Multiline style {st} is in use and can't be deleted.")));
        }
        let d = s.doc_mut()?;
        d.mline_styles.retain(|x| !x.name.eq_ignore_ascii_case(&st));
        return Ok(styles_value(d));
    }
    let Some(name) = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()) else { return Ok(styles_value(s.doc()?)) };
    if name.chars().count() > 31 || name.chars().any(|c| "<>/\\\":;?*|,=`".contains(c)) {
        return Err(bad("mlstyle", "style names have up to 31 characters and none of <>/\\\":;?*|,=`"));
    }
    if let Some(to) = str_param(p, "rename").map(str::trim) {
        if to.is_empty() || to.chars().count() > 31 {
            return Err(bad("mlstyle", "`rename` needs a name of up to 31 characters"));
        }
        let d = s.doc_mut()?;
        let from = d.mline_style(name).ok_or_else(|| other(format!("Cannot find multiline style \"{name}\".")))?.name.clone();
        if from.eq_ignore_ascii_case(STANDARD_MLINE_STYLE) {
            return Err(other("The STANDARD multiline style can't be renamed."));
        }
        if d.mline_style(to).is_some_and(|x| !x.name.eq_ignore_ascii_case(&from)) {
            return Err(other(format!("A multiline style named {to} already exists.")));
        }
        if mline_in_use(d, &from) {
            return Err(other(format!("Multiline style {from} is in use and can't be renamed.")));
        }
        if let Some(st) = d.mline_styles.iter_mut().find(|x| x.name.eq_ignore_ascii_case(&from)) {
            st.name = to.to_string();
        }
        if d.header.str("CMLSTYLE", STANDARD_MLINE_STYLE).eq_ignore_ascii_case(&from) {
            d.header.set_str("CMLSTYLE", to);
        }
        return Ok(styles_value(d));
    }
    let elements = elements_param(p)?;
    let d = s.doc()?;
    let existing = d.mline_style(name).cloned();
    let base = match (&existing, str_param(p, "copyFrom")) {
        (Some(st), _) => st.clone(),
        (None, Some(from)) => style_named(s, from)?,
        (None, None) => MLineStyle::default(),
    };
    let mut st = MLineStyle { name: existing.as_ref().map(|e| e.name.clone()).unwrap_or_else(|| name.to_string()), ..base };
    if let Some(t) = str_param(p, "description") {
        st.description = t.chars().take(255).collect();
    }
    if let Some(e) = elements {
        st.elements = e;
    }
    match p.get("fill") {
        None | Some(Value::Null) => {}
        Some(Value::Bool(false)) => st.fill = None,
        Some(Value::Bool(true)) => st.fill = Some(st.fill.unwrap_or(Color::ByLayer)),
        Some(c) => st.fill = Some(color_value(c).ok_or_else(|| bad("mlstyle", format!("unknown fill colour {c}")))?),
    }
    st.joints = bool_or(p, "joints", st.joints);
    st.start_caps = caps_param(p, "startCaps", st.start_caps)?;
    st.end_caps = caps_param(p, "endCaps", st.end_caps)?;
    let st = st.sanitized();
    if let Some(old) = &existing {
        let look = |x: &MLineStyle| MLineStyle { description: String::new(), ..x.clone() };
        if look(old) != look(&st) && mline_in_use(d, &old.name) {
            return Err(other(format!("Multiline style {} is in use: its elements and properties can't be changed.", old.name)));
        }
    }
    let current = bool_or(p, "current", true);
    let d = s.doc_mut()?;
    match d.mline_styles.iter_mut().find(|x| x.name.eq_ignore_ascii_case(&st.name)) {
        Some(x) => *x = st.clone(),
        None => d.mline_styles.push(st.clone()),
    }
    if current {
        d.header.set_str("CMLSTYLE", &st.name);
    }
    Ok(styles_value(d))
}

// =====================================================================================
// Geometry helpers
// =====================================================================================

/// The multiline `h`.
fn mline_of(s: &Session, h: Handle) -> Result<MLine> {
    match curves::entity(s, h)?.kind {
        EntityKind::MLine(m) => Ok(m),
        _ => Err(other("Select a multiline.")),
    }
}

fn store(s: &mut Session, h: Handle, m: MLine) -> Result<()> {
    s.doc_mut()?.modify_entity(h, |e| e.kind = EntityKind::MLine(m))?;
    Ok(())
}

/// Segment `k` of the vertex path.
fn path_seg(m: &MLine, k: usize) -> Option<Line> {
    let n = m.vertices.len();
    Some(Line::new(m.vertices.get(k)?.p.xy(), m.vertices.get((k + 1) % n.max(1))?.p.xy()))
}

/// The segment of the vertex path nearest `p`.
fn nearest_seg(m: &MLine, p: Vec2) -> Option<usize> {
    (0..m.segment_count()).filter_map(|k| Some((k, path_seg(m, k)?.dist(p)))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(k, _)| k)
}

/// Element `e` (offset `d`) on segment `k`: its start and end.
fn element_line(m: &MLine, miters: &[Vec2], k: usize, d: f64) -> Option<Line> {
    let n = m.vertices.len();
    let j = (k + 1) % n.max(1);
    let a = m.vertices.get(k)?.p.xy() + *miters.get(k)? * d;
    let b = m.vertices.get(j)?.p.xy() + *miters.get(j)? * d;
    Some(Line::new(a, b))
}

/// Indices of the outermost elements: (largest offset, smallest offset).
fn outer(offs: &[f64]) -> Option<(usize, usize)> {
    let top = offs.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))?.0;
    let bottom = offs.iter().enumerate().min_by(|a, b| a.1.total_cmp(b.1))?.0;
    Some((top, bottom))
}

/// Distance along `l` (from its start) where it meets the infinite line `o`.
fn meet(l: &Line, o: &Line) -> Option<f64> {
    let (_, t, _) = line_line_infinite(l.a, l.b, o.a, o.b)?;
    let v = t * l.len();
    v.is_finite().then_some(v)
}

/// Break element `e` of segment `k` between distances `a` and `b`.
fn add_cut(m: &mut MLine, k: usize, e: usize, a: f64, b: f64) {
    let (start, end) = (a.min(b), a.max(b));
    if let Some(v) = m.vertices.get_mut(k)
        && end > start
        && v.cuts.len() < MAX_MLINE_CUTS
    {
        v.cuts.push(MLineCut { element: e, start, end });
    }
}

/// Break the elements `which` of `a`'s segment `k` where they cross the outer elements of `b`'s
/// segment `u`.
fn cut_across(a: &mut MLine, k: usize, which: &[usize], b: &MLine, u: usize) {
    let (am, ao, bm, bo) = (a.miters(), a.offsets(), b.miters(), b.offsets());
    let Some((top, bottom)) = outer(&bo) else { return };
    let (Some(bt), Some(bb)) = (bo.get(top).and_then(|d| element_line(b, &bm, u, *d)), bo.get(bottom).and_then(|d| element_line(b, &bm, u, *d)))
    else {
        return;
    };
    for &e in which {
        let Some(l) = ao.get(e).and_then(|d| element_line(a, &am, k, *d)) else { continue };
        if let (Some(x), Some(y)) = (meet(&l, &bt), meet(&l, &bb)) {
            add_cut(a, k, e, x, y);
        }
    }
}

/// The multiline cut at distance `at` along segment `k` of its path, keeping the part before
/// (`keep_before`) or after; the new end gets no caps.
fn trim_at(m: &MLine, k: usize, at: f64, keep_before: bool) -> Option<MLine> {
    let seg = path_seg(m, k)?;
    let p = seg.a + seg.dir() * at;
    if !p.is_finite() {
        return None;
    }
    let mut out = m.clone();
    if m.closed {
        // Opened at the cut: from the cut round to it again.
        let n = m.vertices.len();
        let mut vs = vec![MLineVertex::new(p.to3(0.0))];
        vs.extend((1..=n).filter_map(|i| m.vertices.get((k + i) % n).cloned()));
        if let Some(last) = vs.last_mut() {
            last.cuts.clear();
        }
        vs.push(MLineVertex::new(p.to3(0.0)));
        out.vertices = vs;
        out.closed = false;
        out.no_start_caps = true;
        out.no_end_caps = true;
        return Some(out);
    }
    if keep_before {
        let mut vs: Vec<MLineVertex> = m.vertices.get(..=k)?.to_vec();
        if let Some(last) = vs.last_mut() {
            last.cuts.clear();
        }
        vs.push(MLineVertex::new(p.to3(0.0)));
        out.vertices = vs;
        out.no_end_caps = true;
    } else {
        let mut vs = vec![MLineVertex::new(p.to3(0.0))];
        vs.extend(m.vertices.get(k + 1..)?.iter().cloned());
        out.vertices = vs;
        out.no_start_caps = true;
    }
    (out.vertices.len() >= 2 && out.segment_count() > 0).then_some(out)
}

// =====================================================================================
// MLEDIT
// =====================================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    ClosedCross,
    OpenCross,
    MergedCross,
    ClosedTee,
    OpenTee,
    MergedTee,
    CornerJoint,
    AddVertex,
    DeleteVertex,
    CutSingle,
    CutAll,
    WeldAll,
}

const TOOLS: [(&str, Tool); 12] = [
    ("CC", Tool::ClosedCross),
    ("OC", Tool::OpenCross),
    ("MC", Tool::MergedCross),
    ("CT", Tool::ClosedTee),
    ("OT", Tool::OpenTee),
    ("MT", Tool::MergedTee),
    ("CJ", Tool::CornerJoint),
    ("AV", Tool::AddVertex),
    ("DV", Tool::DeleteVertex),
    ("CS", Tool::CutSingle),
    ("CA", Tool::CutAll),
    ("WA", Tool::WeldAll),
];

fn tool_of(t: &str) -> Option<Tool> {
    TOOLS.iter().find(|(k, _)| k.eq_ignore_ascii_case(t.trim())).map(|(_, t)| *t)
}

impl Tool {
    /// Two multilines (else one multiline and one or two points on it).
    fn pair(self) -> bool {
        matches!(
            self,
            Tool::ClosedCross | Tool::OpenCross | Tool::MergedCross | Tool::ClosedTee | Tool::OpenTee | Tool::MergedTee | Tool::CornerJoint
        )
    }
    fn two_points(self) -> bool {
        matches!(self, Tool::CutSingle | Tool::CutAll | Tool::WeldAll)
    }
}

fn run_mledit(s: &mut Session, p: &Value) -> Result<Value> {
    let tool = str_param(p, "tool").and_then(tool_of).ok_or_else(|| bad("mledit", "`tool` must be one of CC OC MC CT OT MT CJ AV DV CS CA WA"))?;
    let h = |k: &str| curves::handle_param(p, k).ok_or_else(|| bad("mledit", format!("`{k}` (multiline handle) is required")));
    if tool.pair() {
        mledit_pair(s, tool, h("first")?, point_req("mledit", p, "p1")?, h("second")?, point_req("mledit", p, "p2")?)?;
    } else if tool.two_points() {
        mledit_cut(s, tool, h("handle")?, point_req("mledit", p, "p1")?, point_req("mledit", p, "p2")?)?;
    } else {
        mledit_vertex(s, tool, h("handle")?, point_req("mledit", p, "point")?)?;
    }
    ok()
}

/// Crosses, tees and the corner joint: `a` (picked at `pa`) against `b` (picked at `pb`).
fn mledit_pair(s: &mut Session, tool: Tool, ha: Handle, pa: Vec2, hb: Handle, pb: Vec2) -> Result<()> {
    if ha == hb {
        return Err(other("Select two different multilines."));
    }
    let (mut a, mut b) = (mline_of(s, ha)?, mline_of(s, hb)?);
    let k = nearest_seg(&a, pa).ok_or_else(|| other("The first multiline has no segments."))?;
    let u = nearest_seg(&b, pb).ok_or_else(|| other("The second multiline has no segments."))?;
    let (ao, bo) = (a.offsets(), b.offsets());
    let all_a: Vec<usize> = (0..ao.len()).collect();
    let outer_of = |o: &[f64]| outer(o).map(|(t, b)| if t == b { vec![t] } else { vec![t, b] }).unwrap_or_default();
    match tool {
        Tool::ClosedCross => cut_across(&mut a, k, &all_a, &b, u),
        Tool::OpenCross => {
            let bb = b.clone();
            cut_across(&mut a, k, &all_a, &bb, u);
            let aa = a.clone();
            cut_across(&mut b, u, &outer_of(&bo), &aa, k);
        }
        Tool::MergedCross => {
            let bb = b.clone();
            cut_across(&mut a, k, &outer_of(&ao), &bb, u);
            let aa = a.clone();
            cut_across(&mut b, u, &outer_of(&bo), &aa, k);
        }
        Tool::ClosedTee | Tool::OpenTee | Tool::MergedTee => {
            let path = path_seg(&a, k).ok_or_else(|| other("*Invalid selection*"))?;
            let bm = b.miters();
            let (top, bottom) = outer(&bo).ok_or_else(|| other("*Invalid selection*"))?;
            let line = |e: usize| bo.get(e).and_then(|d| element_line(&b, &bm, u, *d));
            let (lt, lb) = (line(top).ok_or_else(|| other("*Invalid selection*"))?, line(bottom).ok_or_else(|| other("*Invalid selection*"))?);
            let (st, sb) = (meet(&path, &lt), meet(&path, &lb));
            let (Some(st), Some(sb)) = (st, sb) else { return Err(other("The multilines are parallel.")) };
            let pick = (pa - path.a).dot(path.dir());
            let mid = (st + sb) / 2.0;
            let keep_before = pick < mid;
            let (at, near_e) = match (keep_before, st < sb) {
                (true, true) | (false, false) => (st, top),
                _ => (sb, bottom),
            };
            let near = if near_e == top { lt } else { lb };
            // Merged tee: the first one's inner elements run on to the second one's nearest inner element.
            let inner = if tool == Tool::MergedTee && ao.len() > 2 {
                (0..bo.len())
                    .filter(|e| *e != top && *e != bottom)
                    .filter_map(|e| meet(&path, &line(e)?))
                    .min_by(|x, y| (x - at).abs().total_cmp(&(y - at).abs()))
            } else {
                None
            };
            a = trim_at(&a, k, inner.unwrap_or(at), keep_before).ok_or_else(|| other("Nothing would be left of the first multiline."))?;
            if tool != Tool::ClosedTee {
                // Open the near outer element of the second multiline where the first meets it.
                let ak = if keep_before { a.vertices.len().saturating_sub(2) } else { 0 };
                let (am, aoff) = (a.miters(), a.offsets());
                if let Some((t2, b2)) = outer(&aoff) {
                    let al = |e: usize| aoff.get(e).and_then(|d| element_line(&a, &am, ak, *d));
                    let (lt2, lb2) = (al(t2), al(b2));
                    if let (Some(x), Some(y)) = (lt2.and_then(|l| meet(&near, &l)), lb2.and_then(|l| meet(&near, &l))) {
                        add_cut(&mut b, u, near_e, x, y);
                    }
                    // ...and the first one's outer elements stop at it.
                    if inner.is_some() {
                        for (e, l) in [(t2, lt2), (b2, lb2)] {
                            if let Some(l) = l
                                && let Some(x) = meet(&l, &near)
                            {
                                add_cut(&mut a, ak, e, x, if keep_before { l.len() } else { 0.0 });
                            }
                        }
                    }
                }
            }
        }
        Tool::CornerJoint => {
            let (la, lb) =
                (path_seg(&a, k).ok_or_else(|| other("*Invalid selection*"))?, path_seg(&b, u).ok_or_else(|| other("*Invalid selection*"))?);
            let (_, ta, tb) = line_line_infinite(la.a, la.b, lb.a, lb.b).ok_or_else(|| other("The multilines are parallel."))?;
            let (xa, xb) = (ta * la.len(), tb * lb.len());
            let a_before = (pa - la.a).dot(la.dir()) < xa;
            let b_before = (pb - lb.a).dot(lb.dir()) < xb;
            if a.closed || b.closed {
                return Err(other("Corner joints need open multilines."));
            }
            let na = trim_at(&a, k, xa, a_before).ok_or_else(|| other("Nothing would be left of the first multiline."))?;
            let nb = trim_at(&b, u, xb, b_before).ok_or_else(|| other("Nothing would be left of the second multiline."))?;
            let same = na.style.eq_ignore_ascii_case(&nb.style) && na.def == nb.def && na.justification == nb.justification && na.scale == nb.scale;
            // One multiline round the corner when the two run on from one into the other.
            let merged = match (a_before, b_before) {
                (true, false) if same => Some((na.clone(), nb.clone())),
                (false, true) if same => Some((nb.clone(), na.clone())),
                _ => None,
            };
            match merged {
                Some((first, second)) => {
                    let mut m = first;
                    m.no_end_caps = second.no_end_caps;
                    m.vertices.extend(second.vertices.into_iter().skip(1));
                    if m.vertices.len() > MAX_GEN {
                        return Err(other("Too many vertices."));
                    }
                    store(s, ha, m)?;
                    s.doc_mut()?.remove_entity(hb);
                    return Ok(());
                }
                None => {
                    a = na;
                    b = nb;
                }
            }
        }
        _ => {}
    }
    store(s, ha, a)?;
    store(s, hb, b)
}

/// Add or delete a vertex.
fn mledit_vertex(s: &mut Session, tool: Tool, h: Handle, p: Vec2) -> Result<()> {
    let mut m = mline_of(s, h)?;
    match tool {
        Tool::AddVertex => {
            let k = nearest_seg(&m, p).ok_or_else(|| other("*Invalid selection*"))?;
            let seg = path_seg(&m, k).ok_or_else(|| other("*Invalid selection*"))?;
            let q = seg.closest(p);
            if q.near(seg.a, 1e-9) || q.near(seg.b, 1e-9) || m.vertices.len() >= MAX_GEN {
                return Err(other("A vertex is already there."));
            }
            let at = seg.a.dist(q);
            let v = m.vertices.get_mut(k).ok_or_else(|| other("*Invalid selection*"))?;
            // Breaks past the new vertex move to its segment.
            let (keep, moved): (Vec<MLineCut>, Vec<MLineCut>) = v.cuts.drain(..).partition(|c| c.start.min(c.end) < at);
            v.cuts = keep;
            let moved = moved.into_iter().map(|c| MLineCut { start: c.start - at, end: c.end - at, ..c }).collect();
            m.vertices.insert(k + 1, MLineVertex { p: q.to3(m.vertices.get(k).map(|v| v.p.z).unwrap_or(0.0)), cuts: moved });
        }
        Tool::DeleteVertex => {
            let min = if m.closed { 4 } else { 3 };
            if m.vertices.len() < min {
                return Err(other("The multiline has too few vertices to delete one."));
            }
            let i = m
                .vertices
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.p.xy().dist(p).total_cmp(&b.1.p.xy().dist(p)))
                .map(|(i, _)| i)
                .ok_or_else(|| other("*Invalid selection*"))?;
            m.vertices.remove(i);
            let n = m.vertices.len();
            // The segment that now spans the gap loses its breaks.
            let prev = if i == 0 { if m.closed { n - 1 } else { 0 } } else { i - 1 };
            if let Some(v) = m.vertices.get_mut(prev) {
                v.cuts.clear();
            }
        }
        _ => {}
    }
    store(s, h, m)
}

/// Cut one element (the one nearest `p1`), cut all elements, or weld all, between `p1` and `p2`
/// on the segment picked at `p1`.
fn mledit_cut(s: &mut Session, tool: Tool, h: Handle, p1: Vec2, p2: Vec2) -> Result<()> {
    let mut m = mline_of(s, h)?;
    let k = nearest_seg(&m, p1).ok_or_else(|| other("*Invalid selection*"))?;
    let (miters, offs) = (m.miters(), m.offsets());
    let lines: Vec<(usize, Line)> = offs.iter().enumerate().filter_map(|(e, d)| Some((e, element_line(&m, &miters, k, *d)?))).collect();
    let along = |l: &Line, q: Vec2| (q - l.a).dot(l.dir()).clamp(0.0, l.len());
    match tool {
        Tool::CutSingle => {
            let (e, l) = lines.iter().min_by(|a, b| a.1.dist(p1).total_cmp(&b.1.dist(p1))).copied().ok_or_else(|| other("*Invalid selection*"))?;
            add_cut(&mut m, k, e, along(&l, p1), along(&l, p2));
        }
        Tool::CutAll => {
            for (e, l) in &lines {
                add_cut(&mut m, k, *e, along(l, p1), along(l, p2));
            }
        }
        Tool::WeldAll => {
            let mut cuts = Vec::new();
            for c in m.vertices.get(k).map(|v| v.cuts.clone()).unwrap_or_default() {
                let Some((_, l)) = lines.iter().find(|(e, _)| *e == c.element) else { continue };
                let (a, b) = (along(l, p1).min(along(l, p2)), along(l, p1).max(along(l, p2)));
                let (cs, ce) = (c.start.min(c.end), c.start.max(c.end));
                // What is left of the break outside the welded stretch.
                if cs < a {
                    cuts.push(MLineCut { start: cs, end: ce.min(a), ..c });
                }
                if ce > b {
                    cuts.push(MLineCut { start: cs.max(b), end: ce, ..c });
                }
            }
            if let Some(v) = m.vertices.get_mut(k) {
                v.cuts = cuts;
            }
        }
        _ => {}
    }
    store(s, h, m)
}

#[derive(Default)]
struct MleditM {
    tool: Option<Tool>,
    first: Option<(Handle, Vec2)>,
}

impl Interactive for MleditM {
    fn name(&self) -> &'static str {
        "MLEDIT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (self.tool, self.first) {
            (None, _) => Prompt::new("Enter mline editing option", curves::KW).kw(&TOOLS.map(|(k, _)| k)),
            (Some(t), None) if t.pair() => Prompt::new("Select first mline", Accept::POINT),
            (Some(_), None) => Prompt::new("Select mline", Accept::POINT),
            (Some(t), Some(_)) if t.pair() => Prompt::new("Select second mline", Accept::POINT).kw(&["Undo"]),
            (Some(_), Some((_, p))) => Prompt::new("Select second point", Accept::POINT).kw(&["Undo"]).base(p),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Some(tool) = self.tool else {
            return match &i {
                Input::Keyword(k) | Input::Text(k) => {
                    self.tool = Some(tool_of(k).ok_or_else(|| other("Enter one of CC OC MC CT OT MT CJ AV DV CS CA WA."))?);
                    Ok(Step::Continue)
                }
                Input::Enter | Input::Cancel => Ok(Step::Cancel),
                _ => Ok(Step::Continue),
            };
        };
        match i {
            Input::Keyword(k) if k == "Undo" => {
                self.first = None;
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                let pick = || -> Result<Handle> {
                    let h = curves::pick_at(s, p).ok_or_else(|| other("Select a multiline."))?;
                    mline_of(s, h).map(|_| h)
                };
                match self.first {
                    None if tool == Tool::AddVertex || tool == Tool::DeleteVertex => {
                        let h = pick()?;
                        mledit_vertex(s, tool, h, p)?;
                        Ok(Step::Continue)
                    }
                    None => {
                        self.first = Some((pick()?, p));
                        Ok(Step::Continue)
                    }
                    Some((h1, p1)) if tool.pair() => {
                        let h2 = pick()?;
                        mledit_pair(s, tool, h1, p1, h2, p)?;
                        self.first = None;
                        Ok(Step::Continue)
                    }
                    Some((h1, p1)) => {
                        mledit_cut(s, tool, h1, p1, p)?;
                        self.first = None;
                        Ok(Step::Continue)
                    }
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

// =====================================================================================
// EXPLODE
// =====================================================================================

/// The lines and arcs a multiline explodes into: each element piece in its element's colour
/// and linetype, caps and joints in the multiline's.
pub(crate) fn explode(e: &Entity, m: &MLine) -> Vec<Entity> {
    let g = m.geometry();
    let mut out = Vec::new();
    for (i, a, b) in &g.lines {
        let mut c = e.common.clone();
        if let Some(el) = m.def.elements.get(*i) {
            if !matches!(el.color, Color::ByLayer | Color::ByBlock) {
                c.color = el.color;
            }
            let lt = el.linetype.trim();
            if !lt.is_empty() && !lt.eq_ignore_ascii_case("bylayer") && !lt.eq_ignore_ascii_case("byblock") {
                c.linetype = el.linetype.clone();
            }
        }
        out.push(Entity { handle: Handle(0), common: c, kind: crate::cmd::helpers::line(*a, *b) });
    }
    for seg in &g.caps {
        let kind = match seg {
            cadcraft_geom::Segment::Line(l) => crate::cmd::helpers::line(l.a, l.b),
            cadcraft_geom::Segment::Arc { arc, .. } => crate::cmd::helpers::arc(arc),
        };
        out.push(Entity { handle: Handle(0), common: e.common.clone(), kind });
    }
    out
}
