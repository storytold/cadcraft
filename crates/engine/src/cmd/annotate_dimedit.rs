//! Dimension edits: DIMEDIT (Home/New/Rotate/Oblique), DIMJOGGED, DIMJOGLINE, DIMBREAK and
//! DIMINSPECT. Each has a JSON form; the prompt sequences run the same JSON form.

use cadcraft_doc::{DimBreak, DimKind, Dimension, EntityKind, Handle, Inspection, JoggedRadius, Prim};
use cadcraft_geom::{Segment, Vec2};
use serde_json::{Value, json};

use super::super::helpers::v3;
use super::super::machines::{SelOutcome, SelectPhase};
use super::super::{CommandSpec, bad, bool_or, point_param, point_req, size_param, str_param, targets};
use super::{DimText, add_dim, circle_of, dim, dims_of, pick_at};

use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Most objects DIMBREAK Auto looks at, and most breaks it adds to one dimension.
const MAX_BREAK_OBJECTS: usize = 20_000;
const MAX_AUTO_BREAKS: usize = 1000;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("dimedit", "Dimension Edit", run_dimedit)
            .alias(&["ded", "dimed"])
            .params("{handles?, mode?: home|new|rotate|oblique (default home), text? (new: \"\" or \"<>\" = the measurement), angle?: degrees (rotate: text angle; oblique: extension line angle, absent = none)}")
            .interactive(|_| Ok(Box::new(DimEditM::new(None)))),
        CommandSpec::new("dimedit.oblique", "Oblique", |s, p| run_dimedit(s, &with_mode(p, "oblique")))
            .menu(&["Dimension", "Oblique"])
            .params("{handles?, angle?: degrees}")
            .interactive(|_| Ok(Box::new(DimEditM::new(Some(Mode::Oblique))))),
        CommandSpec::new("dimjogged", "Jogged", run_jogged)
            .menu(&["Dimension", "Jogged"])
            .alias(&["djo", "jog"])
            .params("{handle (arc or circle), centerOverride: [x,y], at: [x,y] (dimension line location), jog?: [x,y], text?}")
            .interactive(|_| Ok(Box::new(JoggedM::default()))),
        CommandSpec::new("dimjogline", "Jogged Linear", run_jogline)
            .menu(&["Dimension", "Jogged Linear"])
            .alias(&["djl"])
            .params("{handle (linear or aligned dimension), at?: [x,y] (jog location; default between the text and the first extension line), remove?: bool}")
            .interactive(|_| Ok(Box::new(JogLineM::default()))),
        CommandSpec::new("dimbreak", "Dimension Break", run_dimbreak)
            .menu(&["Dimension", "Dimension Break"])
            .params("{handles?, mode?: auto|manual|remove (default auto), objects?: [hex] (auto: the objects that break; default all crossing objects), first?/second?: [x,y] (manual), size?: number (default 0.125 × DIMSCALE)}")
            .interactive(|_| Ok(Box::new(DimBreakM::default()))),
        CommandSpec::new("diminspect", "Inspection", run_diminspect)
            .menu(&["Dimension", "Inspection"])
            .params("{handles?, shape?: round|angular|none (default round), label?: string, rate?: string (default \"100%\"), remove?: bool}")
            .interactive(|_| Ok(Box::new(InspectM::default()))),
    ]
}

fn with_mode(p: &Value, mode: &str) -> Value {
    let mut p = p.clone();
    if let Some(o) = p.as_object_mut() {
        o.insert("mode".into(), json!(mode));
    }
    p
}

fn handles_json(hs: &[Handle]) -> Value {
    json!(hs.iter().map(|h| h.hex()).collect::<Vec<_>>())
}

/// Change each dimension of `hs` with `f` (its block regenerated); returns how many changed.
fn edit_dims(s: &mut Session, hs: &[Handle], f: &dyn Fn(&mut Dimension) -> bool) -> Result<usize> {
    let d = s.doc_mut()?;
    let mut n = 0;
    for h in hs {
        let mut changed = false;
        d.modify_entity(*h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind
                && f(dm)
            {
                dm.block = None;
                changed = true;
            }
        })?;
        n += usize::from(changed);
    }
    Ok(n)
}

fn is_linear(dm: &Dimension) -> bool {
    matches!(dm.kind, DimKind::Linear { .. } | DimKind::Aligned)
}

// ---------------- DIMEDIT ----------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Home,
    New,
    Rotate,
    Oblique,
}

fn run_dimedit(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = dims_of(s, &targets(s, p)?);
    if hs.is_empty() {
        return Err(bad("dimedit", "select dimensions"));
    }
    let mode = str_param(p, "mode").unwrap_or("home").to_ascii_lowercase();
    let angle = match p.get("angle") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_f64().filter(|a| a.is_finite()).ok_or_else(|| bad("dimedit", "`angle` must be a number (degrees)"))?.to_radians()),
    };
    let n = match mode.as_str() {
        "home" => edit_dims(s, &hs, &|dm| {
            dm.user_text_pos = false;
            dm.text_rotation = 0.0;
            dm.overrides.remove("textJust");
            true
        })?,
        "new" => {
            let t = str_param(p, "text").unwrap_or("").to_string();
            // "<>" alone is the measurement, which is what no override text means.
            let t = if t.trim() == "<>" { String::new() } else { t };
            edit_dims(s, &hs, &|dm| {
                dm.text = t.clone();
                true
            })?
        }
        "rotate" => {
            let a = angle.ok_or_else(|| bad("dimedit", "`angle` (degrees) is required"))?;
            edit_dims(s, &hs, &|dm| {
                dm.text_rotation = cadcraft_geom::norm_angle(a);
                true
            })?
        }
        "oblique" => edit_dims(s, &hs, &|dm| {
            if !is_linear(dm) {
                return false;
            }
            dm.marks.oblique = angle.map(cadcraft_geom::norm_angle);
            true
        })?,
        other => return Err(bad("dimedit", format!("unknown mode `{other}` (home, new, rotate, oblique)"))),
    };
    Ok(json!({ "changed": n }))
}

/// DIMEDIT at the command line: the kind of edit, then (New) the text or (Rotate) the angle,
/// the dimensions, and (Oblique) the extension line angle.
struct DimEditM {
    mode: Option<Mode>,
    text: Option<String>,
    angle: Option<f64>,
    selecting: Option<SelectPhase>,
    picked: Vec<Handle>,
}

impl DimEditM {
    fn new(mode: Option<Mode>) -> Self {
        DimEditM { mode, text: None, angle: None, selecting: None, picked: Vec::new() }
    }
    /// "Select objects", or the pickfirst selection at once.
    fn select(&mut self, s: &mut Session) -> Result<Step> {
        let ph = SelectPhase::begin(s);
        if ph.done {
            return self.after_selection(s, ph.picked);
        }
        self.selecting = Some(ph);
        Ok(Step::Continue)
    }
    fn run(&self, s: &mut Session, hs: &[Handle], angle: Option<f64>) -> Result<Step> {
        let (mode, extra) = match self.mode {
            Some(Mode::New) => ("new", json!({ "text": self.text.clone().unwrap_or_default() })),
            Some(Mode::Rotate) => ("rotate", json!({ "angle": angle.unwrap_or(0.0).to_degrees() })),
            Some(Mode::Oblique) => ("oblique", json!({ "angle": angle.map(f64::to_degrees) })),
            _ => ("home", json!({})),
        };
        let mut p = extra;
        if let Some(o) = p.as_object_mut() {
            o.insert("handles".into(), handles_json(hs));
            o.insert("mode".into(), json!(mode));
        }
        let r = run_dimedit(s, &p)?;
        s.echo(format!("{} dimension(s) changed.", r.get("changed").and_then(Value::as_u64).unwrap_or(0)));
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
    fn after_selection(&mut self, s: &mut Session, hs: Vec<Handle>) -> Result<Step> {
        let hs = dims_of(s, &hs);
        if hs.is_empty() {
            s.echo("No dimensions selected.");
            return Ok(Step::Done);
        }
        if self.mode == Some(Mode::Oblique) {
            self.picked = hs;
            return Ok(Step::Continue);
        }
        self.run(s, &hs, self.angle)
    }
}

impl Interactive for DimEditM {
    fn name(&self) -> &'static str {
        "DIMEDIT"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        // Dimension ▸ Oblique asks for the dimensions first.
        if self.mode == Some(Mode::Oblique) {
            return self.select(s);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some(sel) = &self.selecting {
            return sel.prompt();
        }
        match self.mode {
            None => Prompt::new("Enter type of dimension editing", Accept::TEXT).kw(&["Home", "New", "Rotate", "Oblique"]).default("Home"),
            Some(Mode::New) => Prompt::new("Enter new dimension text", Accept::TEXT).default("<>"),
            Some(Mode::Rotate) => Prompt::new("Specify angle for dimension text", Accept::NUMBER),
            _ => Prompt::new("Enter obliquing angle (press ENTER for none)", Accept::NUMBER),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(sel) = &mut self.selecting {
            return match sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.selecting = None;
                    self.after_selection(s, hs)
                }
            };
        }
        match (self.mode, i) {
            (None, Input::Keyword(k)) => {
                self.mode = match k.as_str() {
                    "New" => Some(Mode::New),
                    "Rotate" => Some(Mode::Rotate),
                    "Oblique" => Some(Mode::Oblique),
                    _ => Some(Mode::Home),
                };
                if matches!(self.mode, Some(Mode::Home | Mode::Oblique)) {
                    return self.select(s);
                }
            }
            (None, Input::Enter) => {
                self.mode = Some(Mode::Home);
                return self.select(s);
            }
            (None, Input::Text(_)) => s.echo("Invalid option keyword."),
            (Some(Mode::New), Input::Text(t)) if self.text.is_none() => {
                self.text = Some(t);
                return self.select(s);
            }
            (Some(Mode::New), Input::Enter) if self.text.is_none() => {
                self.text = Some(String::new());
                return self.select(s);
            }
            (Some(Mode::Rotate), Input::Text(t)) if self.angle.is_none() => match s.angle_settings().direction(&t) {
                Some(a) => {
                    self.angle = Some(a);
                    return self.select(s);
                }
                None => s.echo("Requires a valid angle."),
            },
            (Some(Mode::Oblique), Input::Text(t)) if !self.picked.is_empty() => match s.angle_settings().direction(&t) {
                Some(a) => return self.run(s, &self.picked.clone(), Some(a)),
                None => s.echo("Requires a valid angle."),
            },
            (Some(Mode::Oblique), Input::Enter) if !self.picked.is_empty() => return self.run(s, &self.picked.clone(), None),
            (_, Input::Cancel) => return Ok(Step::Cancel),
            (Some(_), Input::Enter) => return Ok(Step::Cancel),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, _c: Vec2) -> Vec<EntityKind> {
        Vec::new()
    }
}

// ---------------- DIMJOGGED ----------------

/// A jogged radius dimension of the circle (`c`, `r`): the arrow where the direction of `at`
/// meets it, the line from `center` (the override) with a jog at `jog` (default halfway).
fn jogged_kind(s: &Session, c: Vec2, r: f64, center: Vec2, at: Vec2, jog: Option<Vec2>, text: &str) -> Option<EntityKind> {
    let dir = (at - c).normalized();
    if dir == Vec2::ZERO || !dir.is_finite() || r.is_nan() || r <= 0.0 {
        return None;
    }
    let tip = c + dir * r;
    let jog = jog.unwrap_or_else(|| center.mid(tip));
    let mut k = dim(s, DimKind::Radius, c, Vec2::ZERO, Vec2::ZERO, tip, text);
    if let EntityKind::Dimension(dm) = &mut k {
        dm.marks.jogged = Some(JoggedRadius { center: v3(center), jog: v3(jog) });
        if at.dist(c) > r + 1e-9 {
            dm.text_mid = v3(at);
            dm.user_text_pos = true;
        }
    }
    Some(k)
}

fn run_jogged(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("dimjogged", "`handle` of an arc or circle is required"))?;
    let (c, r) = circle_of(s, h).ok_or_else(|| bad("dimjogged", "object is not a circle or arc"))?;
    let center = point_req("dimjogged", p, "centerOverride")?;
    let at = point_req("dimjogged", p, "at")?;
    let k = jogged_kind(s, c, r, center, at, point_param(p, "jog"), str_param(p, "text").unwrap_or(""))
        .ok_or_else(|| bad("dimjogged", "`at` must differ from the centre"))?;
    Ok(json!({ "handle": add_dim(s, k)?.hex() }))
}

#[derive(Default)]
struct JoggedM {
    circle: Option<(Vec2, f64)>,
    center: Option<Vec2>,
    at: Option<Vec2>,
    text: DimText,
}

impl Interactive for JoggedM {
    fn name(&self) -> &'static str {
        "DIMJOGGED"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some(p) = self.text.prompt() {
            return p;
        }
        match (self.circle, self.center, self.at) {
            (None, ..) => Prompt::new("Select arc or circle", Accept::POINT),
            (Some((c, _)), None, _) => Prompt::new("Specify center location override", Accept::POINT).base(c),
            (Some(_), Some(o), None) => Prompt::new("Specify dimension line location", Accept::POINT).kw(&["Mtext", "Text", "Angle"]).base(o),
            (Some(_), Some(o), Some(_)) => Prompt::new("Specify jog location", Accept::POINT).base(o),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.text.answer(s, &i) {
            return Ok(Step::Continue);
        }
        match (self.circle, self.center, self.at, i) {
            (None, _, _, Input::Point(p)) => match pick_at(s, p).and_then(|h| circle_of(s, h)) {
                Some(t) => self.circle = Some(t),
                None => s.echo("Object selected is not a circle or arc."),
            },
            (Some(_), None, _, Input::Point(p)) => self.center = Some(p),
            (Some(_), Some(_), None, Input::Keyword(k)) => {
                self.text.keyword(&k);
            }
            (Some(_), Some(_), None, Input::Point(p)) => self.at = Some(p),
            (Some((c, r)), Some(o), Some(at), Input::Point(j)) => {
                if let Some(k) = jogged_kind(s, c, r, o, at, Some(j), "") {
                    add_dim(s, self.text.apply(k))?;
                }
                return Ok(Step::Done);
            }
            (_, _, _, Input::Enter | Input::Cancel) => return Ok(Step::Cancel),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, s: &Session, p: Vec2) -> Vec<EntityKind> {
        let Some((c, r)) = self.circle else { return Vec::new() };
        let (o, at, jog) = match (self.center, self.at) {
            (Some(o), None) => (o, p, None),
            (Some(o), Some(at)) => (o, at, Some(p)),
            _ => return Vec::new(),
        };
        jogged_kind(s, c, r, o, at, jog, "").map(|k| vec![self.text.apply(k)]).unwrap_or_default()
    }
}

// ---------------- DIMJOGLINE ----------------

/// Where a dimension line meets the first extension line, and the dimension line direction.
fn first_foot(dm: &Dimension) -> Option<(Vec2, Vec2)> {
    let dir = match dm.kind {
        DimKind::Linear { rotation } => Vec2::from_angle(rotation),
        DimKind::Aligned => (dm.p14.xy() - dm.p13.xy()).normalized(),
        _ => return None,
    };
    if dir == Vec2::ZERO || !dir.is_finite() {
        return None;
    }
    let on = dm.defpt.xy();
    Some((on + dir * (dm.p13.xy() - on).dot(dir), dir))
}

fn run_jogline(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = dims_of(s, &targets(s, p)?);
    let h = *hs.first().ok_or_else(|| bad("dimjogline", "`handle` of a linear or aligned dimension is required"))?;
    let d = s.doc()?;
    let Some(EntityKind::Dimension(dm)) = d.entity(h).map(|e| &e.kind) else { return Err(bad("dimjogline", "not a dimension")) };
    let (f1, dir) = first_foot(dm).ok_or_else(|| bad("dimjogline", "only linear and aligned dimensions take a jog"))?;
    let remove = bool_or(p, "remove", false);
    let at = match point_param(p, "at") {
        Some(at) => at,
        None => {
            // Halfway between the text and the first extension line.
            let t = cadcraft_render::dimension_in(d, dm).text_pos;
            f1.mid(f1 + dir * (t - f1).dot(dir))
        }
    };
    edit_dims(s, &[h], &|dm| {
        dm.marks.jog_line = if remove { None } else { Some(v3(at)) };
        true
    })?;
    Ok(json!({ "handle": h.hex() }))
}

#[derive(Default)]
struct JogLineM {
    dim: Option<Handle>,
    removing: bool,
}

impl Interactive for JogLineM {
    fn name(&self) -> &'static str {
        "DIMJOGLINE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (self.dim, self.removing) {
            (None, false) => Prompt::new("Select dimension to add jog", Accept::SELECT).kw(&["Remove"]),
            (None, true) => Prompt::new("Select jog to remove", Accept::SELECT),
            (Some(_), _) => Prompt::new("Specify jog location (or press ENTER)", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.dim, i) {
            (None, Input::Keyword(k)) if k == "Remove" => self.removing = true,
            (None, Input::Pick(hs)) => {
                let lin = |s: &Session, h: Handle| {
                    s.doc().ok().and_then(|d| d.entity(h)).is_some_and(|e| matches!(&e.kind, EntityKind::Dimension(dm) if is_linear(dm)))
                };
                match dims_of(s, &hs).into_iter().find(|h| lin(s, *h)) {
                    Some(h) if self.removing => {
                        run_jogline(s, &json!({ "handle": h.hex(), "remove": true }))?;
                        return Ok(Step::Done);
                    }
                    Some(h) => self.dim = Some(h),
                    None if hs.is_empty() => {}
                    None => s.echo("Select a linear or aligned dimension."),
                }
            }
            (Some(h), Input::Point(at)) => {
                run_jogline(s, &json!({ "handle": h.hex(), "at": [at.x, at.y] }))?;
                return Ok(Step::Done);
            }
            (Some(h), Input::Enter) => {
                run_jogline(s, &json!({ "handle": h.hex() }))?;
                return Ok(Step::Done);
            }
            (_, Input::Enter | Input::Cancel) => return Ok(Step::Cancel),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, _c: Vec2) -> Vec<EntityKind> {
        Vec::new()
    }
}

// ---------------- DIMBREAK ----------------

/// Where the dimension's drawn lines cross the objects: (point, direction of the line there).
fn crossings(dm_lines: &[Vec<Vec2>], prims: &[Prim]) -> Vec<Vec2> {
    let mut out = Vec::new();
    for l in dm_lines {
        for w in l.windows(2) {
            let seg = Segment::Line(cadcraft_geom::Line::new(w[0], w[1]));
            for p in prims {
                let pts = match p {
                    Prim::Seg(o) => cadcraft_geom::intersect_segments(&seg, o),
                    Prim::Circle(c) => cadcraft_geom::line_circle(&cadcraft_geom::Line::new(w[0], w[1]), c)
                        .into_iter()
                        .filter(|(_, t)| (-1e-9..=1.0 + 1e-9).contains(t))
                        .map(|(q, _)| q)
                        .collect(),
                    Prim::Infinite { base, dir, ray } => cadcraft_geom::line_line_infinite(w[0], w[1], *base, *base + *dir)
                        .filter(|(_, t, u)| (-1e-9..=1.0 + 1e-9).contains(t) && (!ray || *u >= -1e-9))
                        .map(|(q, _, _)| vec![q])
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                for q in pts {
                    if out.len() >= MAX_AUTO_BREAKS {
                        return out;
                    }
                    if !out.iter().any(|o: &Vec2| o.dist(q) < 1e-9) {
                        out.push(q);
                    }
                }
            }
        }
    }
    out
}

fn run_dimbreak(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = dims_of(s, &targets(s, p)?);
    if hs.is_empty() {
        return Err(bad("dimbreak", "select dimensions"));
    }
    let mode = str_param(p, "mode").unwrap_or("auto").to_ascii_lowercase();
    let d = s.doc()?;
    let size = match size_param("dimbreak", p, "size", false)? {
        Some(x) => x,
        None => 0.125 * Some(d.header.f64("DIMSCALE", 1.0)).filter(|k| *k > 0.0 && *k < 1e12).unwrap_or(1.0),
    };
    match mode.as_str() {
        "remove" => {
            let n = edit_dims(s, &hs, &|dm| {
                let had = !dm.marks.breaks.is_empty();
                dm.marks.breaks.clear();
                had
            })?;
            Ok(json!({ "changed": n }))
        }
        "manual" => {
            let (a, b) = (point_req("dimbreak", p, "first")?, point_req("dimbreak", p, "second")?);
            if a.near(b, 1e-12) {
                return Err(bad("dimbreak", "`first` and `second` must differ"));
            }
            let br = DimBreak { at: v3(a.mid(b)), size: a.dist(b), auto: false };
            let n = edit_dims(s, &hs[..1], &|dm| {
                dm.marks.breaks.push(br.clone());
                true
            })?;
            Ok(json!({ "changed": n }))
        }
        "auto" => {
            // The breaking objects: those given, else every object (not a dimension or text).
            let given: Option<Vec<Handle>> = match p.get("objects") {
                None | Some(Value::Null) => None,
                Some(_) => Some(targets(s, &json!({ "handles": p.get("objects") }))?),
            };
            let space = s.space();
            let store = d.space(&space).ok_or_else(|| bad("dimbreak", "no current space"))?;
            let others: Vec<(Handle, Vec<Prim>)> = store
                .iter()
                .filter(|e| match &given {
                    Some(g) => g.contains(&e.handle),
                    None => !matches!(e.kind, EntityKind::Dimension(_) | EntityKind::Text(_) | EntityKind::MText(_)),
                })
                .take(MAX_BREAK_OBJECTS)
                .map(|e| (e.handle, e.kind.prims()))
                .collect();
            let mut plan = Vec::new();
            for h in &hs {
                let Some(EntityKind::Dimension(dm)) = d.entity(*h).map(|e| &e.kind) else { continue };
                let mut plain = dm.clone();
                plain.marks.breaks.clear();
                let g = cadcraft_render::dimension_in(d, &plain);
                let prims: Vec<Prim> = others.iter().filter(|(o, _)| o != h).flat_map(|(_, p)| p.iter().cloned()).collect();
                plan.push((*h, crossings(&g.lines, &prims)));
            }
            let mut n = 0;
            for (h, pts) in plan {
                n += edit_dims(s, &[h], &|dm| {
                    dm.marks.breaks.retain(|b| !b.auto);
                    dm.marks.breaks.extend(pts.iter().map(|q| DimBreak { at: v3(*q), size, auto: true }));
                    true
                })?;
            }
            Ok(json!({ "changed": n }))
        }
        other => Err(bad("dimbreak", format!("unknown mode `{other}` (auto, manual, remove)"))),
    }
}

#[derive(Default)]
struct DimBreakM {
    dims: Vec<Handle>,
    multiple: Option<SelectPhase>,
    objects: Vec<Handle>,
    manual: Vec<Vec2>,
    manual_mode: bool,
}

impl DimBreakM {
    fn run(&self, s: &mut Session, mut p: Value) -> Result<Step> {
        if let Some(o) = p.as_object_mut() {
            o.insert("handles".into(), handles_json(&self.dims));
        }
        let r = run_dimbreak(s, &p)?;
        s.echo(format!("{} dimension(s) changed.", r.get("changed").and_then(Value::as_u64).unwrap_or(0)));
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
}

impl Interactive for DimBreakM {
    fn name(&self) -> &'static str {
        "DIMBREAK"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some(sel) = &self.multiple {
            return sel.prompt();
        }
        if self.dims.is_empty() {
            return Prompt::new("Select dimension to add/remove break", Accept::SELECT).kw(&["Multiple"]);
        }
        if self.manual_mode {
            return Prompt::new(if self.manual.is_empty() { "Specify first break point" } else { "Specify second break point" }, Accept::POINT)
                .base_opt(self.manual.first().copied());
        }
        if self.objects.is_empty() {
            let kws: &[&str] = if self.dims.len() == 1 { &["Auto", "Manual", "Remove"] } else { &["Auto", "Remove"] };
            Prompt::new("Select object to break dimension", Accept::SELECT).kw(kws).default("Auto")
        } else {
            Prompt::new("Select object to break dimension", Accept::SELECT)
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(sel) = &mut self.multiple {
            return match sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.multiple = None;
                    self.dims = dims_of(s, &hs);
                    Ok(if self.dims.is_empty() { Step::Done } else { Step::Continue })
                }
            };
        }
        if self.dims.is_empty() {
            match i {
                Input::Keyword(k) if k == "Multiple" => self.multiple = Some(SelectPhase::default()),
                Input::Pick(hs) => match dims_of(s, &hs).first() {
                    Some(h) => self.dims = vec![*h],
                    None if hs.is_empty() => {}
                    None => s.echo("Object selected is not a dimension."),
                },
                Input::Enter | Input::Cancel => return Ok(Step::Cancel),
                _ => {}
            }
            return Ok(Step::Continue);
        }
        if self.manual_mode {
            return match i {
                Input::Point(p) if self.manual.is_empty() => {
                    self.manual.push(p);
                    Ok(Step::Continue)
                }
                Input::Point(p) => {
                    let a = self.manual.first().copied().unwrap_or(p);
                    self.run(s, json!({ "mode": "manual", "first": [a.x, a.y], "second": [p.x, p.y] }))
                }
                Input::Enter | Input::Cancel => Ok(Step::Cancel),
                _ => Ok(Step::Continue),
            };
        }
        match i {
            Input::Keyword(k) if k == "Auto" => return self.run(s, json!({ "mode": "auto" })),
            Input::Keyword(k) if k == "Remove" => return self.run(s, json!({ "mode": "remove" })),
            Input::Keyword(k) if k == "Manual" => self.manual_mode = true,
            Input::Pick(hs) => {
                for h in hs {
                    if !self.dims.contains(&h) && !self.objects.contains(&h) {
                        self.objects.push(h);
                    }
                }
            }
            Input::Enter if self.objects.is_empty() => return self.run(s, json!({ "mode": "auto" })),
            Input::Enter => return self.run(s, json!({ "mode": "auto", "objects": handles_json(&self.objects) })),
            Input::Cancel => return Ok(Step::Cancel),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, _c: Vec2) -> Vec<EntityKind> {
        Vec::new()
    }
}

// ---------------- DIMINSPECT ----------------

fn run_diminspect(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = dims_of(s, &targets(s, p)?);
    if hs.is_empty() {
        return Err(bad("diminspect", "select dimensions"));
    }
    if bool_or(p, "remove", false) {
        let n = edit_dims(s, &hs, &|dm| dm.marks.inspect.take().is_some())?;
        return Ok(json!({ "changed": n }));
    }
    let shape = match str_param(p, "shape").unwrap_or("round").to_ascii_lowercase().as_str() {
        "round" => 0,
        "angular" => 1,
        "none" => 2,
        other => return Err(bad("diminspect", format!("unknown shape `{other}` (round, angular, none)"))),
    };
    let cap = |t: &str| t.chars().take(256).collect::<String>();
    let ins = Inspection { shape, label: cap(str_param(p, "label").unwrap_or("")), rate: cap(str_param(p, "rate").unwrap_or("100%")) };
    let n = edit_dims(s, &hs, &|dm| {
        dm.marks.inspect = Some(ins.clone());
        true
    })?;
    Ok(json!({ "changed": n }))
}

/// DIMINSPECT at the command line: the dimensions, then shape (or Remove), label and rate.
#[derive(Default)]
struct InspectM {
    selecting: Option<SelectPhase>,
    dims: Vec<Handle>,
    shape: Option<&'static str>,
    label: Option<String>,
}

impl Interactive for InspectM {
    fn name(&self) -> &'static str {
        "DIMINSPECT"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let ph = SelectPhase::begin(s);
        if ph.done {
            self.dims = dims_of(s, &ph.picked);
            if self.dims.is_empty() {
                return Ok(Step::Done);
            }
        } else {
            self.selecting = Some(ph);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some(sel) = &self.selecting {
            return sel.prompt();
        }
        match (self.shape, &self.label) {
            (None, _) => Prompt::new("Enter inspection shape", Accept::TEXT).kw(&["Round", "Angular", "None", "Remove"]).default("Round"),
            (Some(_), None) => Prompt::new("Enter label (press ENTER for none)", Accept::TEXT),
            (Some(_), Some(_)) => Prompt::new("Enter inspection rate", Accept::TEXT).default("100%"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(sel) = &mut self.selecting {
            return match sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.selecting = None;
                    self.dims = dims_of(s, &hs);
                    if self.dims.is_empty() {
                        s.echo("No dimensions selected.");
                        return Ok(Step::Done);
                    }
                    Ok(Step::Continue)
                }
            };
        }
        let handles = handles_json(&self.dims);
        match (self.shape, self.label.clone(), i) {
            (None, _, Input::Keyword(k)) if k == "Remove" => {
                run_diminspect(s, &json!({ "handles": handles, "remove": true }))?;
                s.set_selection(Vec::new());
                return Ok(Step::Done);
            }
            (None, _, Input::Keyword(k)) => {
                self.shape = Some(match k.as_str() {
                    "Angular" => "angular",
                    "None" => "none",
                    _ => "round",
                })
            }
            (None, _, Input::Enter) => self.shape = Some("round"),
            (Some(_), None, Input::Text(t)) => self.label = Some(t),
            (Some(_), None, Input::Enter) => self.label = Some(String::new()),
            (Some(shape), Some(label), Input::Text(t)) => {
                run_diminspect(s, &json!({ "handles": handles, "shape": shape, "label": label, "rate": t }))?;
                s.set_selection(Vec::new());
                return Ok(Step::Done);
            }
            (Some(shape), Some(label), Input::Enter) => {
                run_diminspect(s, &json!({ "handles": handles, "shape": shape, "label": label }))?;
                s.set_selection(Vec::new());
                return Ok(Step::Done);
            }
            (_, _, Input::Cancel) => return Ok(Step::Cancel),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, _c: Vec2) -> Vec<EntityKind> {
        Vec::new()
    }
}
