//! BPARAMETER and BACTION: authoring a dynamic block in the Block Editor. A parameter is a
//! property references to the block can change (a distance, a position); an action is what
//! changing it does to the block's objects (stretch them, move them). Both are written into the
//! block's dynamic data ([`cadcraft_doc::dynblock::add_parameter`], [`add_action`]) as AutoCAD
//! writes its own, so the block works as a dynamic block here and in AutoCAD. Only while the
//! Block Editor is open on a block; points are in the editor's coordinates (the block's base point
//! at the origin).

use cadcraft_doc::dynblock::{ActionSpec, Mark, ParamPoint, ParameterSpec, add_action, add_parameter, marks};
use cadcraft_doc::{Drawing, EntityKind, Handle};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use crate::cmd::curves::{KW, handle_value};
use crate::cmd::machines::{SelOutcome, SelectPhase};
use crate::cmd::{CommandSpec, bad, point_param, points_param, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("bparameter", "Parameter", run_bparameter)
            .alias(&["-bparameter"])
            .params(
                "{type: \"linear\"|\"point\", linear: start: [x,y], end: [x,y], labelAt?: [x,y] (where the label goes; default on the line); point: at: [x,y], labelAt?: [x,y]; label?: property name (default Distance1/Position1…), grips?: 0|1|2 (linear, default 2) or 0|1 (point, default 1)} → {node, element, label} (editor coordinates)",
            )
            .enabled(authoring)
            .interactive(|_| Ok(Box::new(BparameterM::default()))),
        CommandSpec::new("baction", "Action", run_baction)
            .alias(&["-baction"])
            .params(
                "{parameter: graph id or property name (\"Distance1\"), type: \"stretch\"|\"move\", point?: \"start\"|\"second\" (which end of a linear parameter it follows, default second), handles: [objects], frame?: [[x,y],[x,y]] (stretch: corners) | polygon?: [[x,y]…] (stretch: a crossing polygon), at?: [x,y] (where the action shows; default the parameter's point)} → {node, name} (editor coordinates)",
            )
            .enabled(authoring)
            .interactive(|_| Ok(Box::new(BactionM::default()))),
    ]
}

/// The Block Editor is open on a block (parameters and actions belong to a block).
fn authoring(s: &Session) -> std::result::Result<(), String> {
    match s.state() {
        Ok(st) => match &st.block_edit {
            Some(be) if be.current => Err("Parameters and actions can be added to a block only; this editor is open on the drawing itself.".into()),
            Some(_) => Ok(()),
            None => Err("Open a block in the Block Editor first (BEDIT).".into()),
        },
        Err(e) => Err(e.to_string()),
    }
}

/// The block being edited and where its base point is (the editor's origin).
fn editing(s: &Session) -> Result<(String, Vec2)> {
    authoring(s).map_err(EngineError::Other)?;
    let st = s.state()?;
    let be = st.block_edit.as_ref().ok_or_else(|| EngineError::Other("The Block Editor is not open.".into()))?;
    Ok((be.name.clone(), be.base()))
}

fn to_block(p: Vec2, base: Vec2) -> Vec3 {
    (p + base).to3(0.0)
}

fn grips_param(cmd: &str, p: &Value, most: u8, default: u8) -> Result<u8> {
    match p.get("grips") {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_u64().and_then(|g| u8::try_from(g).ok()).filter(|g| *g <= most).ok_or_else(|| bad(cmd, format!("`grips` is 0 to {most}"))),
    }
}

/// How far `at` is to the left of the line from `a` to `b` (negative: to its right).
fn offset_of(a: Vec2, b: Vec2, at: Vec2) -> f64 {
    let d = b - a;
    let len = d.len();
    if len <= 1e-12 { 0.0 } else { (d.x * (at.y - a.y) - d.y * (at.x - a.x)) / len }
}

fn run_bparameter(s: &mut Session, p: &Value) -> Result<Value> {
    let (block, base) = editing(s)?;
    let label = str_param(p, "label").map(str::trim).filter(|l| !l.is_empty()).map(str::to_string);
    let spec = match str_param(p, "type").map(str::to_ascii_lowercase).as_deref() {
        Some("linear") => {
            let start = point_param(p, "start").ok_or_else(|| bad("bparameter", "`start` ([x, y]) is required"))?;
            let end = point_param(p, "end").ok_or_else(|| bad("bparameter", "`end` ([x, y]) is required"))?;
            let label_offset = point_param(p, "labelAt").map_or(0.0, |at| offset_of(start, end, at));
            ParameterSpec::Linear {
                base: to_block(start, base),
                end: to_block(end, base),
                label,
                label_offset,
                grips: grips_param("bparameter", p, 2, 2)?,
            }
        }
        Some("point") => {
            let at = point_param(p, "at").ok_or_else(|| bad("bparameter", "`at` ([x, y]) is required"))?;
            let label_at = point_param(p, "labelAt").unwrap_or(at);
            ParameterSpec::Point { at: to_block(at, base), label, label_at: to_block(label_at, base), grips: grips_param("bparameter", p, 1, 1)? }
        }
        Some(other) if ["alignment", "base", "polar", "xy", "rotation", "flip", "visibility", "lookup"].contains(&other) => {
            return Err(bad("bparameter", format!("{other} parameters are not available yet")));
        }
        _ => return Err(bad("bparameter", "`type` is \"linear\" or \"point\"")),
    };
    let node = add_parameter(s.doc_mut()?, &block, &spec).map_err(|e| bad("bparameter", e))?;
    let m = cadcraft_doc::dynblock::dynamic_block(s.doc()?, &block);
    let made = m.as_ref().and_then(|m| m.parameters.iter().find(|p| p.node == node));
    Ok(json!({ "node": node, "element": made.map(|p| p.element.clone()), "label": made.and_then(|p| p.kind.label().map(str::to_string)) }))
}

/// A parameter of the edited block as BACTION follows it, in editor coordinates.
#[derive(Clone, Debug, PartialEq)]
enum Followed {
    Linear { node: i32, base: Vec2, end: Vec2 },
    Point { node: i32, at: Vec2 },
}

impl Followed {
    fn node(&self) -> i32 {
        match self {
            Followed::Linear { node, .. } | Followed::Point { node, .. } => *node,
        }
    }
}

/// The parameters of `block` an action can follow, in editor coordinates.
fn followable(d: &Drawing, block: &str, base: Vec2) -> Vec<Followed> {
    marks(d, block)
        .into_iter()
        .filter_map(|m| match m {
            Mark::Linear { node, base: b, end, .. } => Some(Followed::Linear { node, base: b.xy() - base, end: end.xy() - base }),
            Mark::Point { node, at, .. } => Some(Followed::Point { node, at: at.xy() - base }),
            _ => None,
        })
        .collect()
}

/// The parameter named by `v`: its graph id, or its property name ("Distance1").
fn parameter_named(d: &Drawing, block: &str, base: Vec2, v: &Value) -> Result<Followed> {
    let list = followable(d, block, base);
    let m = cadcraft_doc::dynblock::dynamic_block(d, block);
    let node = match v {
        Value::Number(n) => n.as_i64().and_then(|n| i32::try_from(n).ok()),
        Value::String(t) => t.trim().parse::<i32>().ok().or_else(|| {
            m.as_ref()?
                .parameters
                .iter()
                .find(|p| p.kind.label().is_some_and(|l| l.eq_ignore_ascii_case(t.trim())) || p.element.eq_ignore_ascii_case(t.trim()))
                .map(|p| p.node)
        }),
        _ => None,
    };
    let node = node.ok_or_else(|| bad("baction", format!("no parameter {v} in the block")))?;
    match list.into_iter().find(|f| f.node() == node) {
        Some(f) => Ok(f),
        None if m.is_some_and(|m| m.parameters.iter().any(|p| p.node == node)) => {
            Err(bad("baction", "that parameter can't have stretch or move actions yet"))
        }
        None => Err(bad("baction", format!("no parameter {v} in the block"))),
    }
}

/// How far `p` is from the segment from `a` to `b`.
fn segment_distance(a: Vec2, b: Vec2, p: Vec2) -> f64 {
    let d = b - a;
    let t = if d.len2() <= 1e-24 { 0.0 } else { ((p - a).dot(d) / d.len2()).clamp(0.0, 1.0) };
    (a + d * t).dist(p)
}

/// Whether `p` is inside the stretch frame: a box's two corners, or a polygon.
fn in_frame(frame: &[Vec2], p: Vec2) -> bool {
    match frame {
        [a, b] => cadcraft_geom::Bounds2::new(*a, *b).contains(p),
        poly => cadcraft_geom::point_in_polygon(poly, p),
    }
}

/// What a stretch with `frame` does to each of `handles` (editor objects): lines and polylines
/// move their vertices inside it (listed; left out with none inside), other objects move whole when their
/// first grip (a circle's centre, a text's insertion point) is inside, else not at all and they
/// are left out. Returns (selection, vertex lists).
fn stretch_lists(d: &Drawing, handles: &[Handle], frame: &[Vec2]) -> (Vec<Handle>, Vec<(Handle, Vec<i32>)>) {
    let (mut selection, mut vertices) = (Vec::new(), Vec::new());
    for h in handles {
        let Some(e) = d.entity(*h) else { continue };
        let listed: Option<Vec<i32>> = match &e.kind {
            EntityKind::Line(l) => {
                Some([l.a.xy(), l.b.xy()].iter().enumerate().filter(|(_, p)| in_frame(frame, **p)).map(|(i, _)| i as i32).collect())
            }
            EntityKind::LwPolyline(pl) => {
                Some(pl.vertices.iter().enumerate().filter(|(_, v)| in_frame(frame, v.p)).filter_map(|(i, _)| i32::try_from(i).ok()).collect())
            }
            k => {
                if !k.grips().first().is_some_and(|p| in_frame(frame, *p)) {
                    continue;
                }
                None
            }
        };
        // A line or polyline with no vertex inside wouldn't change.
        if listed.as_ref().is_some_and(Vec::is_empty) {
            continue;
        }
        selection.push(*h);
        if let Some(v) = listed {
            vertices.push((*h, v));
        }
    }
    (selection, vertices)
}

/// Add the action: `kind` "stretch" or "move" following `param` (at its `point`), on `handles`.
fn add(
    s: &mut Session,
    param: &Followed,
    kind: &str,
    point: ParamPoint,
    handles: &[Handle],
    frame: Option<Vec<Vec2>>,
    at: Option<Vec2>,
) -> Result<Value> {
    let (block, base) = editing(s)?;
    if handles.len() > cadcraft_doc::dynblock::MAX_ACTION_ITEMS {
        return Err(bad("baction", "too many objects"));
    }
    let missing: Vec<String> = handles.iter().filter(|h| s.doc().ok().and_then(|d| d.entity(**h)).is_none()).map(|h| h.hex()).collect();
    if !missing.is_empty() {
        return Err(bad("baction", format!("no such object in the block: {}", missing.join(", "))));
    }
    let shown = match param {
        Followed::Linear { base: b, end, .. } => {
            if point == ParamPoint::Base {
                *b
            } else {
                *end
            }
        }
        Followed::Point { at, .. } => *at,
    };
    let at = to_block(at.unwrap_or(shown), base);
    let spec = match kind {
        "stretch" => {
            let frame = frame.ok_or_else(|| bad("baction", "a stretch needs `frame` (two corners) or `polygon`"))?;
            if frame.len() < 2 || frame.len() > cadcraft_doc::dynblock::MAX_ACTION_ITEMS || frame.iter().any(|p| !p.is_finite()) {
                return Err(bad("baction", "`frame` needs two corners, `polygon` three or more points"));
            }
            let (selection, vertices) = stretch_lists(s.doc()?, handles, &frame);
            if selection.is_empty() {
                return Err(bad("baction", "none of the objects is in the stretch frame"));
            }
            let frame = frame.iter().map(|p| *p + base).collect();
            ActionSpec::Stretch { parameter: param.node(), point, frame, selection, vertices, at }
        }
        "move" => ActionSpec::Move { parameter: param.node(), point, selection: handles.to_vec(), at },
        "array" | "scale" | "rotate" | "flip" | "polarstretch" | "lookup" => {
            return Err(bad("baction", format!("{kind} actions are not available yet")));
        }
        _ => return Err(bad("baction", "`type` is \"stretch\" or \"move\"")),
    };
    let node = add_action(s.doc_mut()?, &block, &spec).map_err(|e| bad("baction", e))?;
    let name = cadcraft_doc::dynblock::dynamic_block(s.doc()?, &block).and_then(|m| m.actions.into_iter().find(|a| a.node == node)).map(|a| a.name);
    Ok(json!({ "node": node, "name": name }))
}

fn run_baction(s: &mut Session, p: &Value) -> Result<Value> {
    let (block, base) = editing(s)?;
    let which = p.get("parameter").ok_or_else(|| bad("baction", "`parameter` (graph id or property name) is required"))?;
    let param = parameter_named(s.doc()?, &block, base, which)?;
    let kind = str_param(p, "type").map(|t| t.trim().to_ascii_lowercase().replace(' ', "")).ok_or_else(|| bad("baction", "`type` is required"))?;
    let point = match str_param(p, "point").map(str::to_ascii_lowercase).as_deref() {
        None | Some("second") | Some("end") => ParamPoint::End,
        Some("start") | Some("base") => ParamPoint::Base,
        Some(o) => return Err(bad("baction", format!("`point` is \"start\" or \"second\", not \"{o}\""))),
    };
    let handles: Vec<Handle> = match p.get("handles") {
        Some(Value::Array(a)) => {
            a.iter().map(|v| handle_value(v).ok_or_else(|| bad("baction", format!("{v} is not an object handle")))).collect::<Result<_>>()?
        }
        Some(_) => return Err(bad("baction", "`handles` is an array of object handles")),
        None => s.selection(),
    };
    if handles.is_empty() {
        return Err(bad("baction", "`handles`: the objects the action acts on"));
    }
    let frame = match (points_param(p, "frame"), points_param(p, "polygon")) {
        (Some(f), _) if f.len() == 2 => Some(f),
        (Some(_), _) => return Err(bad("baction", "`frame` is two corners")),
        (None, Some(poly)) if poly.len() >= 3 => Some(poly),
        (None, Some(_)) => return Err(bad("baction", "`polygon` needs three or more points")),
        (None, None) => None,
    };
    add(s, &param, &kind, point, &handles, frame, point_param(p, "at"))
}

/// Read keyword input: `text` (any case, an optional leading `_`) against `options`, each
/// (keyword, its shortcut). The keyword's own spelling, when one matches.
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

/// Options AutoCAD offers that are not available yet: answered with a message.
fn not_yet(s: &mut Session, what: &str) -> Result<Step> {
    s.echo(format!("{what} is not available yet."));
    Ok(Step::Continue)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum PStage {
    #[default]
    Type,
    Start,
    End,
    LinearLabel,
    LinearGrips,
    Location,
    PointLabel,
    PointGrips,
    /// Typing the property name, then back to the stage it was asked at.
    Label(&'static str),
}

/// BPARAMETER's prompts, as AutoCAD asks them on the command line.
#[derive(Default)]
struct BparameterM {
    stage: PStage,
    linear: bool,
    start: Vec2,
    end: Vec2,
    label_at: Vec2,
    label: Option<String>,
}

const TYPES: &[(&str, &str)] = &[
    ("Alignment", "A"),
    ("Base", "B"),
    ("pOint", "O"),
    ("Linear", "L"),
    ("Polar", "P"),
    ("Xy", "X"),
    ("Rotation", "R"),
    ("Flip", "F"),
    ("Visibility", "V"),
    ("looKup", "K"),
];

impl BparameterM {
    fn make(&self, s: &mut Session, grips: u8) -> Result<Step> {
        let label = self.label.clone();
        let p = if self.linear {
            json!({ "type": "linear", "start": [self.start.x, self.start.y], "end": [self.end.x, self.end.y], "labelAt": [self.label_at.x, self.label_at.y], "label": label, "grips": grips })
        } else {
            json!({ "type": "point", "at": [self.start.x, self.start.y], "labelAt": [self.label_at.x, self.label_at.y], "label": label, "grips": grips })
        };
        run_bparameter(s, &p).map(|_| Step::Done)
    }

    /// The option keywords of the first point's prompt.
    fn options(&self, i: &Input) -> Option<&'static str> {
        let opts: &[(&str, &str)] = if self.linear {
            &[("Name", "N"), ("Label", "L"), ("Chain", "C"), ("Description", "D"), ("Base", "B"), ("Palette", "P"), ("Value set", "V")]
        } else {
            &[("Name", "N"), ("Label", "L"), ("Chain", "C"), ("Description", "D"), ("Palette", "P")]
        };
        text_of(i).and_then(|t| keyword(t, opts))
    }
}

impl Interactive for BparameterM {
    fn name(&self) -> &'static str {
        "BPARAMETER"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            PStage::Type => Prompt::new("Enter parameter type", KW).kw(&TYPES.iter().map(|(k, _)| *k).collect::<Vec<_>>()),
            PStage::Start => Prompt::new("Specify start point", Accept::POINT)
                .kw(&["Name", "Label", "Chain", "Description", "Base", "Palette", "Value set"])
                .deferred(),
            PStage::End => Prompt::new("Specify endpoint", Accept::POINT).base(self.start),
            PStage::LinearLabel | PStage::PointLabel => Prompt::new("Specify label location", Accept::POINT),
            PStage::LinearGrips => Prompt::new("Enter number of grips", KW).kw(&["0", "1", "2"]).default("2"),
            PStage::Location => Prompt::new("Specify parameter location", Accept::POINT).kw(&["Name", "Label", "Chain", "Description", "Palette"]),
            PStage::PointGrips => Prompt::new("Enter number of grips", KW).kw(&["0", "1"]).default("1"),
            PStage::Label(what) => Prompt::new(format!("Enter {what} property label"), Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if matches!(i, Input::Cancel) {
            return Ok(Step::Cancel);
        }
        match self.stage {
            PStage::Type => {
                let Some(t) = text_of(&i) else { return Ok(Step::Continue) };
                match keyword(t, TYPES) {
                    Some("Linear") => (self.linear, self.stage) = (true, PStage::Start),
                    Some("pOint") => (self.linear, self.stage) = (false, PStage::Location),
                    Some(other) => return Err(EngineError::Other(format!("{other} parameters are not available yet."))),
                    None => s.echo("Invalid option keyword."),
                }
            }
            PStage::Start | PStage::Location => match (&i, self.options(&i)) {
                (Input::Point(p), _) => {
                    self.start = *p;
                    self.stage = if self.linear { PStage::End } else { PStage::PointLabel };
                }
                (_, Some("Label")) => self.stage = PStage::Label(if self.linear { "distance" } else { "position" }),
                (_, Some(other)) => return not_yet(s, other),
                _ => {}
            },
            PStage::End => {
                if let Input::Point(p) = i {
                    if (p - self.start).len() <= 1e-9 {
                        s.echo("The endpoint must differ from the start point.");
                    } else {
                        self.end = p;
                        self.stage = PStage::LinearLabel;
                    }
                }
            }
            PStage::LinearLabel | PStage::PointLabel => {
                if let Input::Point(p) = i {
                    self.label_at = p;
                    self.stage = if self.stage == PStage::LinearLabel { PStage::LinearGrips } else { PStage::PointGrips };
                }
            }
            PStage::LinearGrips | PStage::PointGrips => {
                let most = if self.stage == PStage::LinearGrips { 2 } else { 1 };
                let grips = match &i {
                    Input::Enter => Some(most),
                    _ => text_of(&i).and_then(|t| t.trim().parse::<u8>().ok()).filter(|g| *g <= most),
                };
                match grips {
                    Some(g) => return self.make(s, g),
                    None => s.echo(format!("Enter 0 to {most}.")),
                }
            }
            PStage::Label(_) => {
                match &i {
                    Input::Text(t) | Input::Keyword(t) if !t.trim().is_empty() => self.label = Some(t.trim().to_string()),
                    _ => {}
                }
                self.stage = if self.linear { PStage::Start } else { PStage::Location };
            }
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        match self.stage {
            PStage::End => vec![crate::cmd::helpers::line(self.start, cursor)],
            PStage::LinearLabel => {
                vec![crate::cmd::helpers::line(self.start, self.end), crate::cmd::helpers::line((self.start + self.end) * 0.5, cursor)]
            }
            PStage::PointLabel => vec![crate::cmd::helpers::line(self.start, cursor)],
            _ => Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum AStage {
    #[default]
    Parameter,
    Type,
    Point,
    Corner,
    Opposite,
    Polygon,
    Objects,
    Location,
}

/// BACTION's prompts, as AutoCAD asks them on the command line.
#[derive(Default)]
struct BactionM {
    stage: AStage,
    param: Option<Followed>,
    kind: &'static str,
    point: ParamPoint,
    frame: Vec<Vec2>,
    select: SelectPhase,
    picked: Vec<Handle>,
}

impl BactionM {
    /// After the parameter (and its action type): what the action asks next.
    fn after_type(&mut self) {
        self.stage = match (&self.param, self.kind) {
            (Some(Followed::Linear { .. }), _) => AStage::Point,
            (_, "stretch") => AStage::Corner,
            _ => AStage::Objects,
        };
    }
    fn after_point(&mut self) {
        self.stage = if self.kind == "stretch" { AStage::Corner } else { AStage::Objects };
    }
}

impl Interactive for BactionM {
    fn name(&self) -> &'static str {
        "BACTION"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            AStage::Parameter => Prompt::new("Select parameter", Accept::POINT),
            AStage::Type => match self.param {
                Some(Followed::Point { .. }) => Prompt::new("Enter action type", KW).kw(&["Move", "sTretch"]),
                _ => Prompt::new("Enter action type", KW).kw(&["Array", "Move", "Scale", "sTretch"]),
            },
            AStage::Point => Prompt::new("Specify parameter point to associate with action or enter", Accept::POINT)
                .kw(&["sTart point", "Second point"])
                .default("Second"),
            AStage::Corner => Prompt::new("Specify first corner of stretch frame", Accept::POINT).kw(&["CPolygon"]),
            AStage::Opposite => match self.frame.first() {
                Some(c) => Prompt::new("Specify opposite corner", Accept::POINT).corner(*c),
                None => Prompt::new("Specify opposite corner", Accept::POINT),
            },
            AStage::Polygon => {
                Prompt::new(if self.frame.is_empty() { "Specify first polygon point" } else { "Specify endpoint of line" }, Accept::POINT)
                    .kw(if self.frame.is_empty() { &[] } else { &["Undo"] })
                    .base_opt(self.frame.last().copied())
            }
            AStage::Objects => self.select.prompt(),
            AStage::Location => Prompt::new("Specify action location", Accept::POINT).kw(&["Multiplier", "Offset"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if matches!(i, Input::Cancel) {
            return Ok(Step::Cancel);
        }
        match self.stage {
            AStage::Parameter => {
                let (block, base) = editing(s)?;
                let found = match &i {
                    Input::Point(p) => {
                        let reach = (s.pixel_size() * s.settings.pickbox.max(1.0) * 3.0).max(1e-9);
                        let near = |f: &Followed| match f {
                            Followed::Linear { base, end, .. } => segment_distance(*base, *end, *p),
                            Followed::Point { at, .. } => (*at - *p).len(),
                        };
                        let list = followable(s.doc()?, &block, base);
                        list.into_iter().map(|f| (near(&f), f)).filter(|(d, _)| *d <= reach).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, f)| f)
                    }
                    Input::Text(t) | Input::Keyword(t) => parameter_named(s.doc()?, &block, base, &Value::String(t.clone())).ok(),
                    Input::Enter => return Ok(Step::Done),
                    _ => return Ok(Step::Continue),
                };
                match found {
                    Some(f) => {
                        self.param = Some(f);
                        self.stage = AStage::Type;
                    }
                    None => s.echo("No linear or point parameter there."),
                }
            }
            AStage::Type => {
                let Some(t) = text_of(&i) else { return Ok(Step::Continue) };
                let opts: &[(&str, &str)] = if matches!(self.param, Some(Followed::Point { .. })) {
                    &[("Move", "M"), ("sTretch", "T")]
                } else {
                    &[("Array", "A"), ("Move", "M"), ("Scale", "S"), ("sTretch", "T")]
                };
                match keyword(t, opts) {
                    Some("Move") => self.kind = "move",
                    Some("sTretch") => self.kind = "stretch",
                    Some(other) => return Err(EngineError::Other(format!("{other} actions are not available yet."))),
                    None => {
                        s.echo("Invalid option keyword.");
                        return Ok(Step::Continue);
                    }
                }
                self.after_type();
            }
            AStage::Point => {
                match (&i, &self.param) {
                    (Input::Enter, _) => self.point = ParamPoint::End,
                    (Input::Point(p), Some(Followed::Linear { base, end, .. })) => {
                        self.point = if (*p - *base).len() < (*p - *end).len() { ParamPoint::Base } else { ParamPoint::End };
                    }
                    _ => match text_of(&i)
                        .and_then(|t| keyword(t, &[("sTart point", "T"), ("Second point", "S"), ("Start", "Start"), ("Second", "Second")]))
                    {
                        Some("sTart point" | "Start") => self.point = ParamPoint::Base,
                        Some(_) => self.point = ParamPoint::End,
                        None => return Ok(Step::Continue),
                    },
                }
                self.after_point();
            }
            AStage::Corner => match &i {
                Input::Point(p) => {
                    self.frame = vec![*p];
                    self.stage = AStage::Opposite;
                }
                _ => {
                    if text_of(&i).and_then(|t| keyword(t, &[("CPolygon", "CP")])).is_some() {
                        self.frame.clear();
                        self.stage = AStage::Polygon;
                    }
                }
            },
            AStage::Opposite => {
                if let Input::Point(p) = i {
                    self.frame.push(p);
                    s.echo("Specify objects to stretch");
                    self.stage = AStage::Objects;
                }
            }
            AStage::Polygon => match &i {
                Input::Point(p) if self.frame.len() < cadcraft_doc::dynblock::MAX_ACTION_ITEMS => self.frame.push(*p),
                Input::Enter if self.frame.len() >= 3 => {
                    s.echo("Specify objects to stretch");
                    self.stage = AStage::Objects;
                }
                Input::Enter => s.echo("A polygon needs three or more points."),
                _ => {
                    if text_of(&i).and_then(|t| keyword(t, &[("Undo", "U")])).is_some() {
                        self.frame.pop();
                    }
                }
            },
            AStage::Objects => match self.select.feed(s, &i)? {
                SelOutcome::More => {}
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.picked = hs;
                    self.stage = AStage::Location;
                }
            },
            AStage::Location => match &i {
                Input::Point(p) => {
                    let Some(param) = self.param.clone() else { return Ok(Step::Done) };
                    let frame = (self.kind == "stretch").then(|| self.frame.clone());
                    return add(s, &param, self.kind, self.point, &self.picked, frame, Some(*p)).map(|_| Step::Done);
                }
                _ => {
                    if let Some(k) = text_of(&i).and_then(|t| keyword(t, &[("Multiplier", "M"), ("Offset", "O")])) {
                        return not_yet(s, k);
                    }
                }
            },
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        match (self.stage, self.frame.first()) {
            (AStage::Opposite, Some(c)) => {
                vec![crate::cmd::helpers::lwpoly(crate::cmd::helpers::rect_vertices(*c, cursor), true)]
            }
            (AStage::Polygon, Some(_)) => {
                let mut pts = self.frame.clone();
                pts.push(cursor);
                vec![crate::cmd::helpers::lwpoly(pts.into_iter().map(cadcraft_geom::PolyVertex::new).collect(), true)]
            }
            _ => Vec::new(),
        }
    }
}
