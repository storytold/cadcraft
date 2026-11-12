//! Utilities: CAL (expression calculator), FIND (find and replace text), MASSPROP, MEASUREGEOM.

use cadcraft_doc::{EntityKind, Handle};
use cadcraft_geom::{Segment, Vec2};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, bool_or, point_param, points_param, str_param, targets};

use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("cal", "QuickCalc", run_cal)
            .menu(&["Tools", "Inquiry", "Quick"])
            .alias(&["quickcalc", "qc"])
            .params(
                "{expr: \"(3+4)*2^2\" | \"dist([0,0],[3,4])\", vars?: {x: 1, p: [x, y]}} → {value} | {point} | \
                 {convert: {type?: length|area|volume|angle, from, to, value}} → {value} | {units: true} → unit lists | \
                 {pick: point|distance|angle|intersection, points}",
            )
            .enabled(always)
            .noundo()
            .transparent()
            .interactive(|_| Ok(Box::new(CalM))),
        CommandSpec::new("quickcalc.pick", "QuickCalc Pick", run_quickcalc_pick)
            .params("{mode: point|distance|angle|intersection, points: [[x,y],...]} → {value} | {point}")
            .noundo()
            .interactive(|_| Ok(Box::<PickM>::default())),
        CommandSpec::new("find", "Find...", run_find).menu(&["Edit", "Find..."]).params(
            "{find, replace?, matchCase?: bool, wholeWord?: bool, types?: [\"Text\", \"MText\", ...], handles?: [...]} → \
                 {matches: [{handle, type, count, snippet, hit, hitLen}], total, message}",
        ),
        CommandSpec::new("measuregeom", "Measure Geometry", run_measuregeom)
            .params("{mode: distance|radius|angle|area|volume, ...}")
            .noundo()
            .interactive(|_| Ok(Box::<MeasureM>::default())),
        CommandSpec::new("radius.measure", "Radius", run_radius)
            .menu(&["Tools", "Inquiry", "Radius"])
            .params("{handle, point?}")
            .noundo()
            .interactive(|_| Ok(Box::new(RadiusM))),
        CommandSpec::new("angle.measure", "Angle", run_angle)
            .menu(&["Tools", "Inquiry", "Angle"])
            .params("{vertex, p1, p2} | {h1, h2}")
            .noundo()
            .interactive(|_| Ok(Box::<AngleM>::default())),
        CommandSpec::new("volume.measure", "Volume", super::inquiry::run_volume)
            .menu(&["Tools", "Inquiry", "Volume"])
            .params("{points: [[x,y],...] (3 or more) | handle (a closed 2D object), height} → {volume, area, height}")
            .noundo()
            .interactive(|_| Ok(Box::new(super::inquiry::AreaM::volume()))),
    ]
}

// ---------------- CAL ----------------

mod calc;

/// What QuickCalc's "get from the drawing" picks put on the command line before the value (the
/// palette reads it back from there).
const PICKED: &str = "QuickCalc: ";

/// Evaluate an arithmetic expression to a number (degrees for trig, `^` power, functions, pi).
/// Expressions with points are evaluated by [`calc::eval_val`].
pub fn eval(expr: &str, vars: &serde_json::Map<String, Value>) -> std::result::Result<f64, String> {
    match calc::eval_val(expr, vars)? {
        calc::Val::Num(v) => Ok(v),
        calc::Val::Pt(_) => Err("the result is a point, not a number".into()),
    }
}

fn run_cal(_s: &mut Session, p: &Value) -> Result<Value> {
    if bool_or(p, "units", false) {
        return Ok(json!({ "units": calc::units_json() }));
    }
    if let Some(c) = p.get("convert").filter(|v| !v.is_null()) {
        let text = |k: &str| c.get(k).and_then(Value::as_str).ok_or_else(|| bad("cal", format!("`convert.{k}` is required")));
        let vars = p.get("vars").and_then(Value::as_object).cloned().unwrap_or_default();
        let v = match c.get("value") {
            Some(Value::String(e)) => eval(e, &vars).map_err(|m| bad("cal", m))?,
            Some(v) => v.as_f64().ok_or_else(|| bad("cal", "`convert.value` must be a number or an expression"))?,
            None => return Err(bad("cal", "`convert.value` is required")),
        };
        let (from, to) = (text("from")?, text("to")?);
        let r = calc::convert(c.get("type").and_then(Value::as_str), from, to, v).map_err(|m| bad("cal", m))?;
        return Ok(json!({ "value": r, "message": format!("{} {from} = {} {to}", calc::num_text(v), calc::num_text(r)) }));
    }
    if let Some(m) = p.get("pick").filter(|v| !v.is_null()) {
        let mode = m.as_str().and_then(calc::Pick::parse).ok_or_else(|| bad("cal", "`pick` must be point, distance, angle or intersection"))?;
        let pts = points_param(p, "points").ok_or_else(|| bad("cal", "`points` must be a list of [x, y] points"))?;
        let v = mode.value(&pts).map_err(|m| bad("cal", m))?;
        let mut r = v.json();
        r["message"] = json!(v.text());
        return Ok(r);
    }
    let e = str_param(p, "expr").ok_or_else(|| bad("cal", "`expr` is required"))?;
    let vars = p.get("vars").and_then(Value::as_object).cloned().unwrap_or_default();
    let v = calc::eval_val(e, &vars).map_err(|m| bad("cal", m))?;
    let mut r = v.json();
    r["message"] = json!(v.text());
    Ok(r)
}

/// CAL at the command line: one expression, its value echoed.
struct CalM;

impl Interactive for CalM {
    fn name(&self) -> &'static str {
        "CAL"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new(">> Expression", Accept::TEXT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Input::Text(t) = i else { return Ok(Step::Done) };
        if t.trim().is_empty() {
            return Ok(Step::Done);
        }
        match calc::eval_val(&t, &serde_json::Map::new()) {
            Ok(v) => s.echo(v.text()),
            Err(m) => s.echo(format!("CAL: {m}")),
        }
        Ok(Step::Done)
    }
}

/// QuickCalc's "get from the drawing": pick the points for a coordinate, distance, angle or
/// intersection; the value is echoed after [`PICKED`].
#[derive(Default)]
struct PickM {
    mode: Option<calc::Pick>,
    pts: Vec<Vec2>,
}

impl PickM {
    fn finish(&self, s: &mut Session, mode: calc::Pick) -> Result<Step> {
        match mode.value(&self.pts) {
            Ok(v) => s.echo(format!("{PICKED}{}", v.text())),
            Err(m) => s.echo(format!("QuickCalc: {m}")),
        }
        Ok(Step::Done)
    }
}

impl Interactive for PickM {
    fn name(&self) -> &'static str {
        "QUICKCALC"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        let msg = match (self.mode, self.pts.len()) {
            (None, _) => {
                return Prompt::new("Get from the drawing or [Point/Distance/Angle/Intersection]", Accept::POINT).kw(&[
                    "Point",
                    "Distance",
                    "Angle",
                    "Intersection",
                ]);
            }
            (Some(calc::Pick::Point), _) => "Specify point",
            (Some(calc::Pick::Intersection), 0) => "Specify first point of first line",
            (Some(calc::Pick::Intersection), 1) => "Specify second point of first line",
            (Some(calc::Pick::Intersection), 2) => "Specify first point of second line",
            (Some(calc::Pick::Intersection), _) => "Specify second point of second line",
            (Some(_), 0) => "Specify first point",
            (Some(_), _) => "Specify second point",
        };
        let base = if self.pts.len() % 2 == 1 { self.pts.last().copied() } else { None };
        Prompt::new(msg, Accept::POINT).base_opt(base)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Keyword(k) if self.mode.is_none() => {
                self.mode = calc::Pick::parse(&k);
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                let mode = *self.mode.get_or_insert(calc::Pick::Point);
                self.pts.push(p);
                if self.pts.len() >= mode.points() { self.finish(s, mode) } else { Ok(Step::Continue) }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        match self.pts.last() {
            Some(a) if self.pts.len() % 2 == 1 => vec![super::helpers::line(*a, c)],
            _ => Vec::new(),
        }
    }
}

fn run_quickcalc_pick(s: &mut Session, p: &Value) -> Result<Value> {
    let mode = str_param(p, "mode").unwrap_or("point");
    run_cal(s, &json!({ "pick": mode, "points": p.get("points").cloned().unwrap_or(Value::Null) }))
}

// ---------------- FIND ----------------

/// Characters of context kept on each side of a hit in the Find dialog's result list.
const SNIPPET_CONTEXT: usize = 18;

/// The text around the first hit of `find` in `text`, for the result list: at most
/// [`SNIPPET_CONTEXT`] characters before the hit and as many after it, the hit's offset in the
/// snippet (in characters) and its length (in characters).
///
/// The offset and length are character counts, not bytes: the dialog slices the snippet by
/// character so a Chinese or Vietnamese drawing never splits a glyph. Lower-casing can change a
/// string's character count in rare scripts, so every slice goes through `get` and a snippet that
/// cannot be cut is reported as absent rather than guessed at.
fn snippet_at(text: &str, find: &str, case: bool, whole: bool) -> Option<(String, usize, usize)> {
    if find.is_empty() {
        return None;
    }
    let (hay, needle) = if case { (text.to_string(), find.to_string()) } else { (text.to_lowercase(), find.to_lowercase()) };
    let mut at = 0;
    let pos = loop {
        let rest = hay.get(at..)?;
        let p = rest.find(&needle)? + at;
        let end = p + needle.len();
        let boundary = |i: usize| hay.get(..i).and_then(|s| s.chars().last()).is_none_or(|c| !c.is_alphanumeric());
        let after = hay.get(end..).and_then(|s| s.chars().next()).is_none_or(|c| !c.is_alphanumeric());
        if !whole || (boundary(p) && after) {
            break p;
        }
        at = end.max(p + 1);
        if at >= hay.len() {
            return None;
        }
    };
    let start = hay.get(..pos)?.chars().count();
    let hit_len = needle.chars().count();
    let chars: Vec<char> = text.chars().collect();
    let from = start.saturating_sub(SNIPPET_CONTEXT);
    let to = start.saturating_add(hit_len).saturating_add(SNIPPET_CONTEXT).min(chars.len());
    let snippet: String = chars.get(from..to)?.iter().collect();
    Some((snippet, start.saturating_sub(from), hit_len))
}

fn replace_in(text: &str, find: &str, rep: &str, case: bool, whole: bool) -> (String, usize) {
    if find.is_empty() {
        return (text.to_string(), 0);
    }
    let hay = if case { text.to_string() } else { text.to_lowercase() };
    let needle = if case { find.to_string() } else { find.to_lowercase() };
    let mut out = String::new();
    let mut n = 0;
    let mut last = 0;
    let mut start = 0;
    while let Some(pos) = hay.get(start..).and_then(|h| h.find(&needle)).map(|p| p + start) {
        let end = pos + needle.len();
        let boundary = |i: usize| hay.get(..i).and_then(|s| s.chars().last()).is_none_or(|c| !c.is_alphanumeric());
        let after = hay.get(end..).and_then(|s| s.chars().next()).is_none_or(|c| !c.is_alphanumeric());
        if !whole || (boundary(pos) && after) {
            out.push_str(text.get(last..pos).unwrap_or(""));
            out.push_str(rep);
            last = end;
            n += 1;
        }
        start = end.max(pos + 1);
        if start >= hay.len() {
            break;
        }
    }
    out.push_str(text.get(last..).unwrap_or(""));
    (out, n)
}

fn run_find(s: &mut Session, p: &Value) -> Result<Value> {
    let find = str_param(p, "find").ok_or_else(|| bad("find", "`find` is required"))?.to_string();
    let rep = str_param(p, "replace").map(str::to_string);
    let case = bool_or(p, "matchCase", false);
    let whole = bool_or(p, "wholeWord", false);
    // Only the named object types, when the dialog narrows the search; `type_name()` is the same
    // name the result list shows, so a caller never has to guess the spelling.
    let types: Option<Vec<String>> =
        p.get("types").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_ascii_lowercase).collect());
    let space = s.space();
    let d = s.doc()?;
    // The whole current space by default; the Find window narrows it to the objects its result
    // list already holds, so Replace All touches exactly what the user can see.
    let hs: Vec<Handle> = match p.get("handles").and_then(Value::as_array) {
        Some(a) => a.iter().filter_map(|v| v.as_str().and_then(Handle::parse_hex)).collect(),
        None => d.space(&space).map(|st| st.handles()).unwrap_or_default(),
    };
    let mut found: Vec<Value> = Vec::new();
    let mut total = 0;
    let mut edits: Vec<(Handle, EntityKind)> = Vec::new();
    for h in hs {
        let Some(e) = d.entity(h) else { continue };
        if let Some(names) = &types
            && !names.iter().any(|n| n == &e.kind.type_name().to_ascii_lowercase())
        {
            continue;
        }
        let mut k = e.kind.clone();
        let mut hits = 0;
        // The first hit's surroundings, taken before a replacement rewrites the text.
        let mut snippet: Option<(String, usize, usize)> = None;
        let r = rep.as_deref().unwrap_or(&find);
        let mut apply = |t: &mut String| {
            let (nt, n) = replace_in(t, &find, r, case, whole);
            if n > 0 {
                hits += n;
                if snippet.is_none() {
                    snippet = snippet_at(t, &find, case, whole);
                }
                if rep.is_some() {
                    *t = nt;
                }
            }
        };
        match &mut k {
            EntityKind::Text(t) => apply(&mut t.value),
            EntityKind::MText(t) => apply(&mut t.contents),
            EntityKind::Insert(i) => i.attribs.iter_mut().for_each(|a| apply(&mut a.text.value)),
            EntityKind::Dimension(dm) => apply(&mut dm.text),
            EntityKind::MLeader(m) => {
                if let Some(t) = &mut m.text {
                    apply(&mut t.contents)
                }
            }
            EntityKind::Table(t) => t.cells.iter_mut().flatten().for_each(|c| apply(&mut c.text)),
            _ => {}
        }
        if hits > 0 {
            total += hits;
            let (text, at, len) = snippet.unwrap_or_default();
            found.push(json!({
                "handle": h.hex(),
                "type": e.kind.type_name(),
                "count": hits,
                "snippet": text,
                "hit": at,
                "hitLen": len,
            }));
            if rep.is_some() {
                edits.push((h, k));
            }
        }
    }
    let n = edits.len();
    if rep.is_some() {
        let doc = s.doc_mut()?;
        for (h, k) in edits {
            doc.modify_entity(h, |e| {
                e.kind = k;
                if let EntityKind::Dimension(dm) = &mut e.kind {
                    dm.block = None;
                }
            })?;
        }
    } else {
        let sel: Vec<Handle> = found.iter().filter_map(|v| v["handle"].as_str().and_then(Handle::parse_hex)).collect();
        s.set_selection(sel);
    }
    let msg =
        if rep.is_some() { format!("{total} replacement(s) in {n} object(s)") } else { format!("{total} match(es) in {} object(s)", found.len()) };
    Ok(json!({ "matches": found, "total": total, "message": msg }))
}

// ---------------- MEASUREGEOM ----------------
// (MASSPROP is in `inquiry/regions.rs`.)

fn run_radius(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("radius.measure", "`handle` is required"))?;
    let r = match s.doc()?.entity(h).map(|e| e.kind.clone()) {
        Some(EntityKind::Circle(c)) => c.radius,
        Some(EntityKind::Arc(a)) => a.radius,
        Some(EntityKind::LwPolyline(_)) => {
            let at = point_param(p, "point").ok_or_else(|| bad("radius.measure", "`point` on the polyline's arc segment is required"))?;
            match super::annotate::segment_at(s, h, at) {
                Some(Segment::Arc { arc, .. }) => arc.radius,
                _ => return Err(bad("radius.measure", "the polyline segment at `point` is not an arc")),
            }
        }
        _ => return Err(bad("radius.measure", "select an arc, circle or polyline arc")),
    };
    Ok(json!({ "radius": r, "diameter": 2.0 * r, "message": format!("Radius = {r:.4}\nDiameter = {:.4}", 2.0 * r) }))
}

/// MEASUREGEOM's Radius option and Tools > Inquiry > Radius: the radius and diameter of a picked
/// arc, circle or polyline arc segment. Tools > Inquiry > Radius measures a selected arc or circle
/// at once.
struct RadiusM;

impl RadiusM {
    /// Measure the arc, circle or polyline arc segment at the picked point; a pick on anything else keeps the prompt.
    fn pick(s: &mut Session, i: Input) -> Result<Step> {
        let Input::Point(p) = i else { return Ok(Step::Done) };
        let Some(h) = super::curves::pick_at(s, p) else { return Ok(Step::Continue) };
        match run_radius(s, &json!({ "handle": h.0, "point": [p.x, p.y] })) {
            Ok(r) => {
                echo_message(s, &r);
                Ok(Step::Done)
            }
            Err(_) => {
                s.echo("Select an arc, circle or polyline arc.");
                Ok(Step::Continue)
            }
        }
    }
}

impl Interactive for RadiusM {
    fn name(&self) -> &'static str {
        "MEASUREGEOM"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Select arc or circle", Accept::POINT)
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let Some(h) = s.selection().first().copied() else { return Ok(Step::Continue) };
        match run_radius(s, &json!({ "handle": h.0 })) {
            Ok(r) => {
                echo_message(s, &r);
                Ok(Step::Done)
            }
            Err(_) => Ok(Step::Continue),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        Self::pick(s, i)
    }
}

/// Echo each line of a result's `message`.
fn echo_message(s: &mut Session, r: &Value) {
    for l in r["message"].as_str().unwrap_or_default().lines() {
        s.echo(l.to_string());
    }
}

fn run_angle(s: &mut Session, p: &Value) -> Result<Value> {
    let (v, a, b) = if let (Some(v), Some(a), Some(b)) = (point_param(p, "vertex"), point_param(p, "p1"), point_param(p, "p2")) {
        (v, a, b)
    } else {
        let hp = |k: &str| p.get(k).and_then(Value::as_str).and_then(Handle::parse_hex);
        let (Some(h1), Some(h2)) = (hp("h1"), hp("h2")) else { return Err(bad("angle.measure", "give {vertex,p1,p2} or {h1,h2}")) };
        let d = s.doc()?;
        let line = |h: Handle| match d.entity(h).map(|e| e.kind.clone()) {
            Some(EntityKind::Line(l)) => Some((l.a.xy(), l.b.xy())),
            _ => None,
        };
        let (l1, l2) =
            (line(h1).ok_or_else(|| bad("angle.measure", "h1 is not a line"))?, line(h2).ok_or_else(|| bad("angle.measure", "h2 is not a line"))?);
        lines_vertex(l1, l2).ok_or_else(|| bad("angle.measure", "lines are parallel"))?
    };
    let ang = vertex_angle(v, a, b);
    Ok(json!({ "angle": ang, "message": angle_text(ang) }))
}

/// The point where two lines (extended) meet and the end of each line farther from it; `None` when they are parallel.
fn lines_vertex((a1, a2): (Vec2, Vec2), (b1, b2): (Vec2, Vec2)) -> Option<(Vec2, Vec2, Vec2)> {
    let (x, _, _) = cadcraft_geom::line_line_infinite(a1, a2, b1, b2)?;
    let far = |p: Vec2, q: Vec2| if p.dist(x) > q.dist(x) { p } else { q };
    Some((x, far(a1, a2), far(b1, b2)))
}

/// The angle at `v` between the directions to `a` and `b`, in degrees from 0 to 180.
fn vertex_angle(v: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ang = (cadcraft_geom::norm_angle(v.angle_to(b) - v.angle_to(a))).to_degrees();
    if ang > 180.0 { 360.0 - ang } else { ang }
}

fn angle_text(ang: f64) -> String {
    format!("Angle = {ang:.4}°")
}

fn run_measuregeom(s: &mut Session, p: &Value) -> Result<Value> {
    match str_param(p, "mode").unwrap_or("distance").to_ascii_lowercase().as_str() {
        "distance" | "d" => s.execute("dist", p),
        "radius" | "r" => run_radius(s, p),
        "angle" | "a" => run_angle(s, p),
        "area" | "ar" => s.execute("area", p),
        "volume" | "v" => super::inquiry::run_volume(s, p),
        _ => Err(bad("measuregeom", "mode must be distance, radius, angle, area or volume")),
    }
}

/// MEASUREGEOM typed or picked: choose what to measure, then continue as DIST, AREA, a radius pick or an angle pick.
#[derive(Default)]
struct MeasureM {
    inner: Option<Box<dyn Interactive>>,
    /// Waiting for the arc or circle of the Radius option.
    radius: bool,
}

impl Interactive for MeasureM {
    fn name(&self) -> &'static str {
        "MEASUREGEOM"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match &self.inner {
            Some(m) => m.prompt(s),
            None if self.radius => Prompt::new("Select arc or circle", Accept::POINT),
            None => Prompt::new("Enter an option", Accept { point: false, number: false, text: false, select: false, enter: true })
                .kw(&["Distance", "Radius", "Angle", "ARea", "Volume"])
                .default("Distance"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(m) = &mut self.inner {
            return m.input(s, i);
        }
        if self.radius {
            return RadiusM::pick(s, i);
        }
        match i {
            Input::Enter => self.inner = Some(Box::new(super::inquiry::DistM::default())),
            Input::Keyword(k) => match k.as_str() {
                "Radius" => self.radius = true,
                "Angle" => self.inner = Some(Box::<AngleM>::default()),
                "ARea" => self.inner = Some(Box::new(super::inquiry::AreaM::default())),
                "Volume" => self.inner = Some(Box::new(super::inquiry::AreaM::volume())),
                _ => self.inner = Some(Box::new(super::inquiry::DistM::default())),
            },
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        self.inner.as_ref().map(|m| m.preview(s, cursor)).unwrap_or_default()
    }
}

/// MEASUREGEOM's Angle option and Tools > Inquiry > Angle: the angle of an arc, of two points on a
/// circle about its center, between two lines or polyline line segments, or (Enter) at a vertex
/// between two endpoints.
#[derive(Default)]
struct AngleM {
    /// The first line picked.
    first: Option<(Vec2, Vec2)>,
    /// The vertex and endpoints given so far (Enter, or a circle's center and the point picked on it).
    vertex: Option<Vec<Vec2>>,
}

impl AngleM {
    fn finish(s: &mut Session, ang: f64) -> Result<Step> {
        s.echo(angle_text(ang));
        Ok(Step::Done)
    }
}

impl Interactive for AngleM {
    fn name(&self) -> &'static str {
        "MEASUREGEOM"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some(pts) = &self.vertex {
            let msg = ["Specify angle vertex", "Specify first angle endpoint", "Specify second angle endpoint"].get(pts.len()).copied().unwrap_or("");
            return Prompt::new(msg, Accept::POINT).base_opt(pts.first().copied());
        }
        match self.first {
            None => Prompt::new("Select arc, circle, line", Accept::POINT).default("specify vertex"),
            Some(_) => Prompt::new("Select second line", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(pts) = &mut self.vertex {
            return match i {
                Input::Point(p) => {
                    pts.push(p);
                    match pts.as_slice() {
                        [v, a, b] => Self::finish(s, vertex_angle(*v, *a, *b)),
                        _ => Ok(Step::Continue),
                    }
                }
                Input::Enter => Ok(Step::Cancel),
                _ => Ok(Step::Continue),
            };
        }
        let p = match i {
            Input::Enter if self.first.is_none() => {
                self.vertex = Some(Vec::new());
                return Ok(Step::Continue);
            }
            Input::Point(p) => p,
            Input::Enter => return Ok(Step::Cancel),
            _ => return Ok(Step::Continue),
        };
        let Some(h) = super::curves::pick_at(s, p) else {
            s.echo("*Invalid selection*");
            return Ok(Step::Continue);
        };
        let circle = s
            .doc()?
            .entity(h)
            .and_then(|e| if let EntityKind::Circle(c) = &e.kind { Some(cadcraft_geom::Circle::new(c.center.xy(), c.radius)) } else { None });
        if let Some(c) = circle.filter(|_| self.first.is_none()) {
            self.vertex = Some(vec![c.center, c.closest(p)]);
            return Ok(Step::Continue);
        }
        match super::annotate::segment_at(s, h, p) {
            Some(Segment::Arc { arc, .. }) if self.first.is_none() => Self::finish(s, arc.sweep().to_degrees()),
            Some(Segment::Line(l)) => match self.first {
                None => {
                    self.first = Some((l.a, l.b));
                    Ok(Step::Continue)
                }
                Some(first) => match lines_vertex(first, (l.a, l.b)) {
                    Some((v, a, b)) => Self::finish(s, vertex_angle(v, a, b)),
                    None => {
                        s.echo("The lines are parallel.");
                        Ok(Step::Done)
                    }
                },
            },
            _ => {
                s.echo(if self.first.is_none() { "Select an arc, circle or line." } else { "Select a line." });
                Ok(Step::Continue)
            }
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        match self.vertex.as_deref() {
            Some([v]) => vec![super::helpers::line(*v, c)],
            Some([v, a]) => vec![super::helpers::line(*v, *a), super::helpers::line(*v, c)],
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculator() {
        let v = serde_json::Map::new();
        assert_eq!(eval("(3+4)*2^2", &v), Ok(28.0));
        assert!((eval("sin(30)", &v).unwrap() - 0.5).abs() < 1e-12);
        assert!((eval("2*pi", &v).unwrap() - std::f64::consts::TAU).abs() < 1e-12);
        assert_eq!(eval("-2^2", &v), Ok(-4.0));
        assert!(eval("1/0", &v).is_err());
        assert!(eval("((((1", &v).is_err());
        assert!(eval("foo(1)", &v).is_err());
        assert!(eval(&"(".repeat(5000), &v).is_err());
        let mut vars = serde_json::Map::new();
        vars.insert("x".into(), json!(3));
        assert_eq!(eval("x*x", &vars), Ok(9.0));
    }

    #[test]
    fn find_and_replace() {
        assert_eq!(replace_in("Bolt bolt BOLTS", "bolt", "Pin", false, true), ("Pin Pin BOLTS".to_string(), 2));
        assert_eq!(replace_in("aaa", "a", "b", true, false), ("bbb".to_string(), 3));
        assert_eq!(replace_in("x", "", "y", true, false).1, 0);
    }

    /// `snippet_at` returns the whole text when the hit fills it, with the hit at offset 0.
    #[test]
    fn snippet_reports_the_hit_offset() {
        assert_eq!(snippet_at("BOLT-01", "BOLT-01", true, false), Some(("BOLT-01".to_string(), 0, 7)));
    }

    /// Matching ignores case but the snippet keeps the text as it is written, so the result list
    /// shows the drawing's own spelling.
    #[test]
    fn snippet_keeps_the_original_case() {
        assert_eq!(snippet_at("法兰 bolt-01 安装孔", "BOLT-01", false, false), Some(("法兰 bolt-01 安装孔".to_string(), 3, 7)));
    }

    /// A long text keeps [`SNIPPET_CONTEXT`] characters on each side of the hit, counted in
    /// characters so a CJK hit is never split.
    #[test]
    fn snippet_is_windowed_in_characters() {
        let text = format!("{}命中{}", "前".repeat(30), "后".repeat(30));
        let (snippet, hit, len) = snippet_at(&text, "命中", true, false).expect("a hit");
        assert_eq!((hit, len), (SNIPPET_CONTEXT, 2));
        assert_eq!(snippet.chars().count(), SNIPPET_CONTEXT * 2 + 2);
    }

    /// A whole-word search skips an embedded hit, and text without a hit has no snippet.
    #[test]
    fn snippet_respects_whole_word() {
        assert_eq!(snippet_at("BOLTS", "BOLT", true, true), None);
        assert_eq!(snippet_at("a BOLT b", "BOLT", true, true).map(|s| s.1), Some(2));
        assert_eq!(snippet_at("nothing", "BOLT", true, false), None);
    }
}
