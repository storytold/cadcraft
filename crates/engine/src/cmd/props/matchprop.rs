//! MATCHPROP (alias MA, PAINTER): copy properties from a source object to destination objects.
//! Typed: "Select source object", then "Select destination object(s) or [Settings]" until Enter;
//! Settings toggles which properties are copied (remembered for the next typed MATCHPROP). The
//! JSON form copies every property unless `properties` names some.

use cadcraft_doc::{Entity, EntityKind, Handle};
use serde_json::{Value, json};

use super::super::{bad, targets};

use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Which properties MATCHPROP copies (its Settings).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchProps {
    pub color: bool,
    pub layer: bool,
    pub linetype: bool,
    pub ltscale: bool,
    pub lineweight: bool,
    pub transparency: bool,
    pub thickness: bool,
    pub plot_style: bool,
    /// Dimension style and overrides of dimensions; the style of leaders.
    pub dimension: bool,
    /// Text style, height, width factor and oblique angle of text and multiline text.
    pub text: bool,
    /// Pattern, scale, angle and gradient of hatches.
    pub hatch: bool,
    /// Global width and linetype generation of polylines.
    pub polyline: bool,
    /// Display locking and scale of layout viewports.
    pub viewport: bool,
    /// Table style of tables.
    pub table: bool,
    /// Style, arrowhead size, landing and leader type of multileaders.
    pub multileader: bool,
}

impl Default for MatchProps {
    fn default() -> Self {
        MatchProps::all(true)
    }
}

/// Keyword, settings name (JSON and the "Current active settings" echo), field.
type Field = fn(&mut MatchProps) -> &mut bool;
const FIELDS: &[(&str, &str, Field)] = &[
    ("Color", "color", |m| &mut m.color),
    ("LAyer", "layer", |m| &mut m.layer),
    ("LType", "linetype", |m| &mut m.linetype),
    ("LTScale", "ltscale", |m| &mut m.ltscale),
    ("LWeight", "lineweight", |m| &mut m.lineweight),
    ("TRansparency", "transparency", |m| &mut m.transparency),
    ("THickness", "thickness", |m| &mut m.thickness),
    ("PLotstyle", "plotStyle", |m| &mut m.plot_style),
    ("DImension", "dimension", |m| &mut m.dimension),
    ("TExt", "text", |m| &mut m.text),
    ("Hatch", "hatch", |m| &mut m.hatch),
    ("POlyline", "polyline", |m| &mut m.polyline),
    ("Viewport", "viewport", |m| &mut m.viewport),
    ("TAble", "table", |m| &mut m.table),
    ("Multileader", "multileader", |m| &mut m.multileader),
];

impl MatchProps {
    pub fn all(on: bool) -> Self {
        MatchProps {
            color: on,
            layer: on,
            linetype: on,
            ltscale: on,
            lineweight: on,
            transparency: on,
            thickness: on,
            plot_style: on,
            dimension: on,
            text: on,
            hatch: on,
            polyline: on,
            viewport: on,
            table: on,
            multileader: on,
        }
    }
    /// The settings that are on, by name.
    pub fn names(&self) -> Vec<&'static str> {
        let mut m = *self;
        FIELDS.iter().filter(|(_, _, f)| *f(&mut m)).map(|(_, n, _)| *n).collect()
    }
    fn set(&mut self, name: &str, on: bool) -> bool {
        match FIELDS.iter().find(|(k, n, _)| n.eq_ignore_ascii_case(name) || k.eq_ignore_ascii_case(name)) {
            Some((_, _, f)) => {
                *f(self) = on;
                true
            }
            None => false,
        }
    }
    fn toggle(&mut self, name: &str) -> bool {
        match FIELDS.iter().find(|(k, n, _)| n.eq_ignore_ascii_case(name) || k.eq_ignore_ascii_case(name)) {
            Some((_, _, f)) => {
                let b = f(self);
                *b = !*b;
                true
            }
            None => false,
        }
    }
    /// From JSON: a list of names to copy, or `{name: bool}` over the defaults (all on).
    fn from_json(v: &Value) -> Result<Self> {
        let unknown =
            |n: &str| bad("matchprop", format!("unknown property `{n}` (known: {})", FIELDS.iter().map(|f| f.1).collect::<Vec<_>>().join(", ")));
        match v {
            Value::Array(a) => {
                let mut m = MatchProps::all(false);
                for x in a {
                    let n = x.as_str().ok_or_else(|| bad("matchprop", format!("`properties` holds names, got {x}")))?;
                    if !m.set(n, true) {
                        return Err(unknown(n));
                    }
                }
                Ok(m)
            }
            Value::Object(o) => {
                let mut m = MatchProps::default();
                for (n, x) in o {
                    let on = x.as_bool().ok_or_else(|| bad("matchprop", format!("`properties.{n}` must be true or false")))?;
                    if !m.set(n, on) {
                        return Err(unknown(n));
                    }
                }
                Ok(m)
            }
            other => Err(bad("matchprop", format!("`properties` must be a list of names or {{name: bool}}, got {other}"))),
        }
    }
}

/// Copy the chosen properties of `src` onto `dst`.
fn copy_props(src: &Entity, dst: &mut Entity, m: &MatchProps) {
    let (c, d) = (&src.common, &mut dst.common);
    if m.color {
        d.color = c.color;
    }
    if m.layer {
        d.layer.clone_from(&c.layer);
    }
    if m.linetype {
        d.linetype.clone_from(&c.linetype);
    }
    if m.ltscale {
        d.ltscale = c.ltscale;
    }
    if m.lineweight {
        d.lineweight = c.lineweight;
    }
    if m.transparency {
        d.transparency = c.transparency;
    }
    if m.thickness {
        d.thickness = c.thickness;
    }
    if m.plot_style {
        d.plot_style.clone_from(&c.plot_style);
    }
    match (&src.kind, &mut dst.kind) {
        (EntityKind::Dimension(a), EntityKind::Dimension(b)) if m.dimension => {
            b.style.clone_from(&a.style);
            b.overrides.clone_from(&a.overrides);
            b.block = None;
        }
        (EntityKind::Leader(a), EntityKind::Leader(b)) if m.dimension => b.style.clone_from(&a.style),
        (EntityKind::Text(a), EntityKind::Text(b)) if m.text => {
            b.style.clone_from(&a.style);
            b.height = a.height;
            b.width_factor = a.width_factor;
            b.oblique = a.oblique;
        }
        (EntityKind::Text(a), EntityKind::MText(b)) if m.text => {
            b.style.clone_from(&a.style);
            b.height = a.height;
        }
        (EntityKind::MText(a), EntityKind::Text(b)) if m.text => {
            b.style.clone_from(&a.style);
            b.height = a.height;
        }
        (EntityKind::MText(a), EntityKind::MText(b)) if m.text => {
            b.style.clone_from(&a.style);
            b.height = a.height;
        }
        (EntityKind::Hatch(a), EntityKind::Hatch(b)) if m.hatch => {
            b.pattern.clone_from(&a.pattern);
            b.solid = a.solid;
            b.scale = a.scale;
            b.angle = a.angle;
            b.gradient.clone_from(&a.gradient);
            b.pattern_lines.clone_from(&a.pattern_lines);
        }
        (EntityKind::LwPolyline(a), EntityKind::LwPolyline(b)) if m.polyline => {
            b.const_width = a.const_width;
            b.plinegen = a.plinegen;
            if a.const_width > 0.0 {
                for v in &mut b.vertices {
                    (v.start_width, v.end_width) = (0.0, 0.0);
                }
            }
        }
        (EntityKind::Viewport(a), EntityKind::Viewport(b)) if m.viewport => {
            b.locked = a.locked;
            // The same scale: paper height per model height.
            let k = a.view_height / a.height;
            if a.height > 0.0 && k.is_finite() && k > 0.0 && b.height > 0.0 {
                b.view_height = b.height * k;
            }
        }
        (EntityKind::Table(a), EntityKind::Table(b)) if m.table => b.style.clone_from(&a.style),
        (EntityKind::MLeader(a), EntityKind::MLeader(b)) if m.multileader => {
            b.style.clone_from(&a.style);
            b.arrow_size = a.arrow_size;
            b.dogleg = a.dogleg;
            b.spline = a.spline;
        }
        _ => {}
    }
}

/// Copy the chosen properties from `src` to `targets`, skipping the source itself and objects on
/// locked layers. Returns how many objects changed.
fn match_onto(s: &mut Session, src: Handle, targets: &[Handle], m: &MatchProps) -> Result<usize> {
    let d = s.doc_mut()?;
    let source = d.entity(src).map(|e| (**e).clone()).ok_or_else(|| bad("matchprop", "no such source object"))?;
    if m.layer {
        d.ensure_layer(&source.common.layer);
    }
    let mut n = 0;
    for h in targets {
        if *h == src {
            continue;
        }
        let locked = d.entity(*h).and_then(|e| d.layer(&e.common.layer)).is_some_and(|l| l.locked);
        if locked || d.entity(*h).is_none() {
            continue;
        }
        d.modify_entity(*h, |e| copy_props(&source, e, m))?;
        n += 1;
    }
    Ok(n)
}

pub(super) fn run_matchprop(s: &mut Session, p: &Value) -> Result<Value> {
    let src = match p.get("source") {
        None | Some(Value::Null) => return Err(bad("matchprop", "`source` (object handle) is required")),
        Some(v) => targets(s, &json!({ "handle": v }))?.first().copied().ok_or_else(|| bad("matchprop", "`source` is required"))?,
    };
    let tg = match p.get("targets") {
        None | Some(Value::Null) => targets(s, &json!({}))?,
        Some(v) => targets(s, &json!({ "handles": v }))?,
    };
    let m = match p.get("properties") {
        None | Some(Value::Null) => MatchProps::default(),
        Some(v) => MatchProps::from_json(v)?,
    };
    let n = match_onto(s, src, &tg, &m)?;
    Ok(json!({ "changed": n }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Source,
    Dest,
    Settings,
}

pub(super) struct MatchPropM {
    stage: Stage,
    source: Option<Handle>,
}

impl MatchPropM {
    pub(super) fn new() -> Self {
        MatchPropM { stage: Stage::Source, source: None }
    }
    fn echo_settings(s: &mut Session) {
        let on = s.last_used.matchprop.names();
        let text = if on.is_empty() { "None".to_string() } else { on.iter().map(|n| label(n)).collect::<Vec<_>>().join(" ") };
        s.echo(format!("Current active settings: {text}"));
    }
    fn source(&mut self, s: &mut Session, h: Handle) -> Step {
        self.source = Some(h);
        self.stage = Stage::Dest;
        s.set_selection(Vec::new());
        Self::echo_settings(s);
        Step::Continue
    }
}

/// The settings echo's word for a property.
fn label(name: &str) -> &'static str {
    match name {
        "color" => "Color",
        "layer" => "Layer",
        "linetype" => "Ltype",
        "ltscale" => "Ltscale",
        "lineweight" => "Lineweight",
        "transparency" => "Transparency",
        "thickness" => "Thickness",
        "plotStyle" => "PlotStyle",
        "dimension" => "Dim",
        "text" => "Text",
        "hatch" => "Hatch",
        "polyline" => "Polyline",
        "viewport" => "Viewport",
        "table" => "Table",
        _ => "Multileader",
    }
}

impl Interactive for MatchPropM {
    fn name(&self) -> &'static str {
        "MATCHPROP"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        // One preselected object is the source.
        let pre = s.selection();
        if s.settings.pickfirst
            && let [h] = pre.as_slice()
        {
            return Ok(self.source(s, *h));
        }
        s.set_selection(Vec::new());
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            Stage::Source => Prompt::new("Select source object", Accept::SELECT),
            Stage::Dest => Prompt::new("Select destination object(s)", Accept::SELECT).kw(&["Settings"]),
            Stage::Settings => {
                let mut kws: Vec<&str> = FIELDS.iter().map(|f| f.0).collect();
                kws.extend(["ALL", "None"]);
                Prompt::new("Enter a property to turn on or off", Accept::TEXT).kw(&kws).default("Done")
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.stage {
            Stage::Source => match i {
                Input::Pick(hs) => match hs.first() {
                    Some(h) => Ok(self.source(s, *h)),
                    None => Ok(Step::Continue),
                },
                Input::Enter => Ok(Step::Done),
                _ => Ok(Step::Continue),
            },
            Stage::Dest => match i {
                Input::Keyword(k) if k.eq_ignore_ascii_case("settings") => {
                    self.stage = Stage::Settings;
                    Ok(Step::Continue)
                }
                Input::Pick(hs) => {
                    let Some(src) = self.source else { return Ok(Step::Done) };
                    let m = s.last_used.matchprop;
                    match_onto(s, src, &hs, &m)?;
                    s.set_selection(Vec::new());
                    Ok(Step::Continue)
                }
                Input::Enter => Ok(Step::Done),
                Input::Text(_) | Input::Keyword(_) => {
                    s.echo("*Invalid selection*");
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
            Stage::Settings => {
                match &i {
                    Input::Enter => {
                        self.stage = Stage::Dest;
                        return Ok(Step::Continue);
                    }
                    Input::Keyword(k) | Input::Text(k) => {
                        let k = k.trim();
                        if k.eq_ignore_ascii_case("all") {
                            s.last_used.matchprop = MatchProps::all(true);
                        } else if k.eq_ignore_ascii_case("none") {
                            s.last_used.matchprop = MatchProps::all(false);
                        } else if k.eq_ignore_ascii_case("done") {
                            self.stage = Stage::Dest;
                            return Ok(Step::Continue);
                        } else if !s.last_used.matchprop.toggle(k) {
                            s.echo("Invalid option keyword.");
                            return Ok(Step::Continue);
                        }
                        Self::echo_settings(s);
                    }
                    _ => {}
                }
                Ok(Step::Continue)
            }
        }
    }
}
