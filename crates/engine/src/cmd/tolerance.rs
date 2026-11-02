//! TOLERANCE: geometric tolerances (feature control frames) and their explosion.

use cadcraft_doc::{Drawing, Entity, EntityKind, GdtDatum, GdtFrame, GdtLine, GdtPrim, GdtSymbol, GdtTol, Handle, MAX_FRAME_CHARS, Tolerance};
use cadcraft_geom::{PolyVertex, Vec2};
use serde_json::{Value, json};

use super::helpers::{line, lwpoly, v3};
use super::{CommandSpec, bad, bool_or, f64_or, point_req, str_param};

use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("tolerance", "Tolerance...", run_tolerance)
            .menu(&["Dimension", "Tolerance..."])
            .alias(&["tol"])
            .params(
                "{at: [x,y], symbol?: position|concentricity|symmetry|parallelism|perpendicularity|angularity|cylindricity|flatness|circularity|straightness|surfaceprofile|lineprofile|circularrunout|totalrunout, tol1?: \"%%c0.1(M)\" | {value, diameter?, material?: M|L|S}, tol2?, datums?: [\"A(M)\", \"B\", {letter, material?}] (up to 3), line2?: {symbol?, tol1?, tol2?, datums?}, height? (projected zone), projected?, datumId?, rotation? (degrees), style? (dimension style; default DIMSTYLE), text? (the frame string as in DXF, instead of the fields)}",
            )
            .interactive(|s| Ok(Box::new(ToleranceM::new(s)))),
    ]
}

/// A tolerance typed as text: an optional diameter prefix (`%%c`, `Ø`, `⌀`, `dia`), the value and
/// an optional material condition suffix (`(M)`, `(L)`, `(S)`).
pub fn parse_tol(s: &str) -> GdtTol {
    let mut t = s.trim();
    let mut diameter = false;
    for p in ["%%c", "%%C", "Ø", "ø", "⌀", "dia ", "DIA "] {
        if let Some(r) = t.strip_prefix(p) {
            diameter = true;
            t = r.trim_start();
            break;
        }
    }
    let (rest, material) = split_material(t);
    GdtTol { diameter, value: rest.to_string(), material }
}

/// A datum reference typed as text: the letter(s) and an optional `(M)` / `(L)` / `(S)` suffix.
pub fn parse_datum(s: &str) -> GdtDatum {
    let (letter, material) = split_material(s.trim());
    GdtDatum { letter: letter.to_string(), material }
}

fn split_material(t: &str) -> (&str, Option<String>) {
    let up = t.to_ascii_uppercase();
    for m in ["M", "L", "S"] {
        if up.ends_with(&format!("({m})")) {
            return (t.get(..t.len().saturating_sub(3)).unwrap_or("").trim_end(), Some(m.to_string()));
        }
    }
    (t, None)
}

fn tol_value(v: Option<&Value>) -> Option<GdtTol> {
    match v? {
        Value::String(s) => Some(parse_tol(s)),
        Value::Number(n) => Some(GdtTol { value: n.to_string(), ..Default::default() }),
        o @ Value::Object(_) => Some(GdtTol {
            diameter: bool_or(o, "diameter", false),
            value: match o.get("value") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => String::new(),
            },
            material: str_param(o, "material").map(str::to_string),
        }),
        _ => None,
    }
}

fn datums_value(v: Option<&Value>) -> Vec<GdtDatum> {
    let Some(a) = v.and_then(Value::as_array) else { return Vec::new() };
    a.iter()
        .take(3)
        .filter_map(|d| match d {
            Value::String(s) => Some(parse_datum(s)),
            o @ Value::Object(_) => {
                Some(GdtDatum { letter: str_param(o, "letter").unwrap_or("").to_string(), material: str_param(o, "material").map(str::to_string) })
            }
            _ => None,
        })
        .collect()
}

fn symbol_value(cmd: &str, v: Option<&Value>) -> Result<Option<GdtSymbol>> {
    let Some(s) = v.and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("none")) else {
        return Ok(None);
    };
    match GdtSymbol::parse(s).or_else(|| s.chars().next().filter(|_| s.chars().count() == 1).and_then(GdtSymbol::from_code)) {
        Some(sym) if sym.is_characteristic() => Ok(Some(sym)),
        _ => Err(bad(cmd, format!("unknown geometric characteristic `{s}`"))),
    }
}

fn line_value(cmd: &str, p: &Value) -> Result<GdtLine> {
    Ok(GdtLine {
        symbol: symbol_value(cmd, p.get("symbol"))?,
        tol1: tol_value(p.get("tol1")),
        tol2: tol_value(p.get("tol2")),
        datums: datums_value(p.get("datums")),
    })
}

/// The frame string the parameters describe.
fn frame_text(p: &Value) -> Result<String> {
    if let Some(t) = str_param(p, "text") {
        return Ok(t.chars().take(MAX_FRAME_CHARS).collect());
    }
    let mut lines = vec![line_value("tolerance", p)?];
    if let Some(l2) = p.get("line2").filter(|v| v.is_object()) {
        lines.push(line_value("tolerance", l2)?);
    }
    let height = match p.get("height") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    let f = GdtFrame { lines, height, projected: bool_or(p, "projected", false), datum_id: str_param(p, "datumId").unwrap_or("").to_string() };
    Ok(cadcraft_doc::compose_frame(&f))
}

fn current_dimstyle(s: &Session) -> String {
    s.doc().map(|d| d.header.str("DIMSTYLE", "Standard")).unwrap_or_else(|_| "Standard".into())
}

fn make(style: String, at: Vec2, rotation: f64, text: String) -> Tolerance {
    Tolerance { insert: v3(at), dir: Vec2::from_angle(rotation).to3(0.0), style, text }
}

fn run_tolerance(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_req("tolerance", p, "at")?;
    let text = frame_text(p)?;
    if cadcraft_doc::parse_frame(&text).iter().flatten().all(cadcraft_doc::compartment_is_empty) {
        return Err(bad("tolerance", "the feature control frame is empty"));
    }
    let style = str_param(p, "style").map(str::to_string).unwrap_or_else(|| current_dimstyle(s));
    if s.doc()?.dim_style(&style).is_none() {
        return Err(bad("tolerance", format!("no dimension style `{style}`")));
    }
    let t = make(style, at, f64_or(p, "rotation", 0.0).to_radians(), text.clone());
    let h = s.add_entity(EntityKind::Tolerance(t))?;
    Ok(json!({ "handle": h.hex(), "text": text }))
}

/// TOLERANCE at the command line: characteristic, two tolerance values, up to three datums,
/// then the location.
struct ToleranceM {
    step: usize,
    line: GdtLine,
    style: String,
}

const SYMBOL_KW: &[&str] = &[
    "POsition",
    "COncentricity",
    "SYmmetry",
    "PArallelism",
    "PErpendicularity",
    "ANgularity",
    "CYlindricity",
    "Flatness",
    "CIrcularity",
    "STraightness",
    "SUrface",
    "LIne",
    "Runout",
    "TOtal",
    "None",
];

/// Free input split at spaces, Enter meaningful.
const VALUE: Accept = Accept { point: false, number: false, text: false, select: false, enter: true };

impl ToleranceM {
    fn new(s: &Session) -> Self {
        ToleranceM { step: 0, line: GdtLine { symbol: Some(GdtSymbol::Position), ..Default::default() }, style: current_dimstyle(s) }
    }
    fn text(&self) -> String {
        cadcraft_doc::compose_frame(&GdtFrame { lines: vec![self.line.clone()], ..Default::default() })
    }
    fn at_location(&mut self, s: &mut Session) -> Step {
        self.step = 6;
        if cadcraft_doc::parse_frame(&self.text()).iter().flatten().all(cadcraft_doc::compartment_is_empty) {
            s.echo("The feature control frame is empty.");
            return Step::Cancel;
        }
        Step::Continue
    }
}

impl Interactive for ToleranceM {
    fn name(&self) -> &'static str {
        "TOLERANCE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.step {
            0 => Prompt::new("Enter geometric characteristic", VALUE).kw(SYMBOL_KW).default("Position"),
            1 => Prompt::new("Enter first tolerance value", VALUE).default("none"),
            2 => Prompt::new("Enter second tolerance value", VALUE).default("none"),
            n @ 3..=5 => Prompt::new(format!("Enter datum reference {}", n - 2), VALUE).default("none"),
            _ => Prompt::new("Enter tolerance location", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if i == Input::Cancel {
            return Ok(Step::Cancel);
        }
        let text = match &i {
            Input::Text(t) | Input::Keyword(t) => Some(t.trim().to_string()),
            _ => None,
        };
        let none = |t: &Option<String>| t.as_deref().is_none_or(|t| t.is_empty() || t.eq_ignore_ascii_case("none"));
        match self.step {
            0 => {
                match (&i, &text) {
                    (Input::Enter, _) => {}
                    (_, Some(t)) if t.eq_ignore_ascii_case("none") => self.line.symbol = None,
                    (_, Some(t)) => match GdtSymbol::parse(t).filter(|g| g.is_characteristic()) {
                        Some(g) => self.line.symbol = Some(g),
                        None => {
                            s.echo("Requires a geometric characteristic or None.");
                            return Ok(Step::Continue);
                        }
                    },
                    _ => return Ok(Step::Continue),
                }
                self.step = 1;
            }
            1 | 2 => {
                if !matches!(i, Input::Enter | Input::Text(_) | Input::Keyword(_)) {
                    return Ok(Step::Continue);
                }
                let v = (!none(&text)).then(|| parse_tol(text.as_deref().unwrap_or("")));
                if self.step == 1 {
                    self.line.tol1 = v;
                } else {
                    self.line.tol2 = v;
                }
                self.step += 1;
            }
            3..=5 => {
                if !matches!(i, Input::Enter | Input::Text(_) | Input::Keyword(_)) {
                    return Ok(Step::Continue);
                }
                if none(&text) {
                    return Ok(self.at_location(s));
                }
                self.line.datums.push(parse_datum(text.as_deref().unwrap_or("")));
                if self.step == 5 {
                    return Ok(self.at_location(s));
                }
                self.step += 1;
            }
            _ => match i {
                Input::Point(p) => {
                    let t = make(self.style.clone(), p, 0.0, self.text());
                    s.add_entity(EntityKind::Tolerance(t))?;
                    return Ok(Step::Done);
                }
                Input::Enter => return Ok(Step::Cancel),
                _ => {}
            },
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.step == 6 { vec![EntityKind::Tolerance(make(self.style.clone(), c, 0.0, self.text()))] } else { Vec::new() }
    }
}

/// EXPLODE of a feature control frame: lines, circles, arcs, solids (arrowheads) and texts in the
/// dimension style's text style.
pub(crate) fn explode(d: &Drawing, e: &Entity, t: &Tolerance) -> Vec<Entity> {
    let g = cadcraft_render::tolerance_in(d, t);
    let st = d.dim_style(&t.style).cloned().unwrap_or_default();
    let wf = d.text_style(&st.text_style).map(|s| s.width_factor).filter(|w| w.is_finite() && *w > 0.0).unwrap_or(1.0);
    let mk = |kind| Entity { handle: Handle(0), common: e.common.clone(), kind };
    let mut v = Vec::new();
    for p in g.frame.iter().chain(&g.symbols) {
        match p {
            GdtPrim::Line(pts) => match pts.as_slice() {
                [a, b] => v.push(mk(line(*a, *b))),
                pts if pts.len() > 2 => v.push(mk(lwpoly(pts.iter().map(|q| PolyVertex::new(*q)).collect(), false))),
                _ => {}
            },
            GdtPrim::Circle { center, radius } => v.push(mk(super::helpers::circle(*center, *radius))),
            GdtPrim::Arc { center, radius, start, end } => v.push(mk(EntityKind::Arc(cadcraft_doc::Arc {
                center: v3(*center),
                radius: *radius,
                start: cadcraft_geom::norm_angle(*start),
                end: cadcraft_geom::norm_angle(*end),
            }))),
            GdtPrim::Fill([a, b, c]) => v.push(mk(EntityKind::Solid(cadcraft_doc::Solid { corners: [v3(*a), v3(*b), v3(*c), v3(*c)] }))),
        }
    }
    for tx in &g.texts {
        v.push(mk(EntityKind::Text(cadcraft_doc::Text {
            insert: v3(tx.pos),
            align_pt: None,
            height: tx.height,
            value: tx.value.clone(),
            rotation: tx.rotation,
            width_factor: wf,
            oblique: 0.0,
            style: st.text_style.clone(),
            halign: Default::default(),
            valign: Default::default(),
        })));
    }
    v
}
