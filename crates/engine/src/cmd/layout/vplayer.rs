//! VPLAYER typed at the command line: an option loop (ends on Enter) that freezes, thaws, resets
//! or recolours layers in chosen viewports, and sets layers' default visibility in new
//! viewports. Changes go through `viewport.set` / `layer.new` / `layer.set`, so validation
//! matches the JSON forms and the whole session is one undo step.

use cadcraft_color::Color;
use cadcraft_doc::{Handle, Space};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use super::super::find_command;
use super::super::qselect::wildcard;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

const OPTIONS: &[&str] = &["?", "Color", "Freeze", "Thaw", "Reset", "Newfrz", "Vpvisdflt"];

/// Keyword-only: spaces separate inputs.
const KW: Accept = Accept { point: false, number: false, text: false, select: false, enter: true };
/// A name or value: Space ends the input, as in AutoCAD scripts.
const NAME: Accept = Accept { number: true, ..Accept::TEXT };

/// A per-viewport layer change.
#[derive(Clone, Debug)]
enum Change {
    Freeze,
    Thaw,
    /// Back to the layer's default for new viewports (its "VP freeze in new viewports" flag).
    Reset,
    Color(Color),
}

#[derive(Clone, Debug, Default)]
enum Stage {
    #[default]
    Option,
    /// `?`: the viewport whose frozen layers to list.
    List,
    Color,
    TrueColor,
    Layers(Change),
    Apply(Change, Vec<String>),
    Select(Change, Vec<String>, Vec<Handle>),
    Newfrz,
    VisLayers,
    VisDefault(Vec<String>),
}

#[derive(Default)]
pub struct VplayerM {
    stage: Stage,
}

fn retry(msg: impl Into<String>) -> EngineError {
    EngineError::Other(msg.into())
}

fn message(e: &EngineError) -> String {
    match e {
        EngineError::BadParams { msg, .. } => msg.clone(),
        other => other.to_string(),
    }
}

/// Run an engine command's body directly: the running command records the single undo step.
fn call(s: &mut Session, id: &str, p: &Value) -> Result<Value> {
    let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.into()))?;
    (spec.run)(s, p)
}

/// Layers matching a comma-separated name list (wildcards allowed), in drawing order.
fn matching(s: &mut Session, list: &str) -> Vec<String> {
    let Ok(d) = s.doc() else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    let mut missing = Vec::new();
    for pat in list.split(',').map(str::trim).filter(|p| !p.is_empty()).take(1000) {
        let before = out.len();
        for l in &d.layers {
            if wildcard(pat, &l.name) && !out.iter().any(|n| n.eq_ignore_ascii_case(&l.name)) {
                out.push(l.name.clone());
            }
        }
        if out.len() == before && !d.layers.iter().any(|l| wildcard(pat, &l.name)) {
            missing.push(pat.to_string());
        }
    }
    for m in missing {
        s.echo(format!("Cannot find layer \"{m}\"."));
    }
    out
}

fn color_value(c: Color) -> Value {
    match c {
        Color::Index(i) => json!(i),
        Color::True(rgb) => json!(format!("{},{},{}", rgb.0, rgb.1, rgb.2)),
        Color::ByLayer | Color::ByBlock => Value::Null,
    }
}

fn parse_color(t: &str) -> Result<Color> {
    match Color::parse(t) {
        Some(c @ (Color::Index(_) | Color::True(_))) => Ok(c),
        _ => Err(retry("Requires a color name, a number from 1 to 255 or Red,Green,Blue.")),
    }
}

/// The viewport under a picked point: the point is on the sheet in paper space, and in the
/// active viewport's model space inside it (mapped back to the sheet).
fn viewport_at(s: &Session, p: Vec2) -> Option<Handle> {
    let st = s.state().ok()?;
    let at = match st.active_viewport() {
        Some((_, vp)) => vp.model_to_paper(p),
        None => p,
    };
    super::viewports::viewport_at(s, at)
}

fn active(s: &Session) -> Option<Handle> {
    s.state().ok()?.active_viewport().map(|(h, _)| h)
}

impl VplayerM {
    fn option(&mut self, s: &mut Session, k: &str) -> Result<Step> {
        self.stage = match k {
            "?" => match active(s) {
                Some(h) => {
                    Self::list(s, h);
                    Stage::Option
                }
                None => Stage::List,
            },
            "Color" => Stage::Color,
            "Freeze" => Stage::Layers(Change::Freeze),
            "Thaw" => Stage::Layers(Change::Thaw),
            "Reset" => Stage::Layers(Change::Reset),
            "Newfrz" => Stage::Newfrz,
            "Vpvisdflt" => Stage::VisLayers,
            _ => return Err(retry("Invalid option keyword.")),
        };
        Ok(Step::Continue)
    }

    fn list(s: &mut Session, h: Handle) {
        let Some(v) = super::layout_viewports(s).ok().and_then(|vs| vs.into_iter().find(|(x, _)| *x == h)).map(|(_, v)| v) else { return };
        s.echo(format!("Layers currently frozen in viewport {}:", v.id));
        if v.frozen_layers.is_empty() {
            s.echo("  (none)");
        }
        for l in v.frozen_layers {
            s.echo(format!("  {l}"));
        }
    }

    /// Apply `change` to `layers` in the viewports `hs`; refusals are reported.
    fn apply(s: &mut Session, change: &Change, layers: &[String], hs: &[Handle]) {
        if hs.is_empty() || layers.is_empty() {
            return;
        }
        let handles: Vec<String> = hs.iter().map(|h| h.hex()).collect();
        let mut calls = Vec::new();
        match change {
            Change::Freeze => calls.push(json!({ "handles": handles, "freeze": layers })),
            Change::Thaw => calls.push(json!({ "handles": handles, "thaw": layers })),
            Change::Reset => {
                let Ok(d) = s.doc() else { return };
                let (frz, thaw): (Vec<String>, Vec<String>) = layers.iter().cloned().partition(|n| d.layer(n).is_some_and(|l| l.vp_freeze_new));
                calls.push(json!({ "handles": handles, "freeze": frz, "thaw": thaw }));
            }
            Change::Color(c) => {
                let colors: serde_json::Map<String, Value> = layers.iter().map(|l| (l.clone(), color_value(*c))).collect();
                calls.push(json!({ "handles": handles, "colors": colors }));
            }
        }
        for p in calls {
            if let Err(e) = super::run_viewport_set(s, &p) {
                s.echo(message(&e));
            }
        }
    }

    fn sub(&mut self, s: &mut Session, stage: Stage, i: Input) -> Result<Step> {
        let text = match &i {
            Input::Text(t) | Input::Keyword(t) => Some(t.trim().to_string()).filter(|t| !t.is_empty()),
            _ => None,
        };
        self.stage = match stage {
            Stage::Option => Stage::Option,
            Stage::List => match i {
                Input::Point(p) => {
                    match viewport_at(s, p) {
                        Some(h) => Self::list(s, h),
                        None => s.echo("No viewport found there."),
                    }
                    Stage::Option
                }
                Input::Enter => Stage::Option,
                _ => {
                    self.stage = Stage::List;
                    return Err(retry("Requires a point inside a viewport."));
                }
            },
            Stage::Color => match (&i, text) {
                (Input::Keyword(k), _) if k == "Truecolor" => Stage::TrueColor,
                (_, Some(t)) => match parse_color(&t) {
                    Ok(c) => Stage::Layers(Change::Color(c)),
                    Err(e) => {
                        self.stage = Stage::Color;
                        return Err(e);
                    }
                },
                _ => Stage::Option,
            },
            Stage::TrueColor => match text {
                Some(t) => match parse_color(&t) {
                    Ok(c) => Stage::Layers(Change::Color(c)),
                    Err(e) => {
                        self.stage = Stage::TrueColor;
                        return Err(e);
                    }
                },
                None => Stage::Option,
            },
            Stage::Layers(change) => match text {
                Some(t) => {
                    let layers = matching(s, &t);
                    if layers.is_empty() { Stage::Option } else { Stage::Apply(change, layers) }
                }
                None => Stage::Option,
            },
            Stage::Apply(change, layers) => {
                let in_vp = active(s);
                let k = match (&i, in_vp) {
                    (Input::Keyword(k), _) => k.as_str(),
                    (Input::Enter, Some(_)) => "Current",
                    (Input::Enter, None) => "Select",
                    _ => {
                        self.stage = Stage::Apply(change, layers);
                        return Err(retry("Invalid option keyword."));
                    }
                };
                let all: Vec<Handle> = super::layout_viewports(s).map(|v| v.into_iter().map(|(h, _)| h).collect()).unwrap_or_default();
                match k {
                    "Select" => Stage::Select(change, layers, Vec::new()),
                    "All" => {
                        Self::apply(s, &change, &layers, &all);
                        Stage::Option
                    }
                    "Current" => {
                        Self::apply(s, &change, &layers, &in_vp.into_iter().collect::<Vec<_>>());
                        Stage::Option
                    }
                    _ => {
                        let others: Vec<Handle> = all.into_iter().filter(|h| Some(*h) != in_vp).collect();
                        Self::apply(s, &change, &layers, &others);
                        Stage::Option
                    }
                }
            }
            Stage::Select(change, layers, mut picked) => match i {
                Input::Point(p) => {
                    match viewport_at(s, p) {
                        Some(h) if !picked.contains(&h) => {
                            picked.push(h);
                            s.echo(format!("{} viewport(s) selected.", picked.len()));
                        }
                        Some(_) => {}
                        None => s.echo("No viewport found there."),
                    }
                    Stage::Select(change, layers, picked)
                }
                Input::Enter => {
                    Self::apply(s, &change, &layers, &picked);
                    Stage::Option
                }
                _ => {
                    self.stage = Stage::Select(change, layers, picked);
                    return Err(retry("Requires a point inside a viewport."));
                }
            },
            Stage::Newfrz => {
                if let Some(t) = text {
                    let all: Vec<Handle> = s
                        .doc()?
                        .layouts
                        .iter()
                        .flat_map(|l| l.entities.iter())
                        .filter(|e| matches!(&e.kind, cadcraft_doc::EntityKind::Viewport(v) if v.id != 1))
                        .map(|e| e.handle)
                        .collect();
                    let mut made = Vec::new();
                    for n in t.split(',').map(str::trim).filter(|n| !n.is_empty()).take(1000) {
                        match call(s, "layer.new", &json!({ "name": n, "newVpFreeze": true })) {
                            Ok(_) => made.push(n.to_string()),
                            Err(e) => s.echo(format!("Layer \"{n}\": {}", message(&e))),
                        }
                    }
                    Self::apply(s, &Change::Freeze, &made, &all);
                }
                Stage::Option
            }
            Stage::VisLayers => match text {
                Some(t) => {
                    let layers = matching(s, &t);
                    if layers.is_empty() { Stage::Option } else { Stage::VisDefault(layers) }
                }
                None => Stage::Option,
            },
            Stage::VisDefault(layers) => {
                let frozen = match &i {
                    Input::Keyword(k) => k == "Frozen",
                    Input::Enter => false,
                    _ => {
                        self.stage = Stage::VisDefault(layers);
                        return Err(retry("Invalid option keyword."));
                    }
                };
                for n in &layers {
                    if let Err(e) = call(s, "layer.set", &json!({ "name": n, "newVpFreeze": frozen })) {
                        s.echo(format!("Layer \"{n}\": {}", message(&e)));
                    }
                }
                Stage::Option
            }
        };
        Ok(Step::Continue)
    }
}

impl Interactive for VplayerM {
    fn name(&self) -> &'static str {
        "VPLAYER"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        if s.layout_space() == Space::Model {
            s.echo("** Command not allowed in Model Tab ** (switch to a layout first)");
            return Ok(Step::Done);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match &self.stage {
            Stage::Option => Prompt::new("Enter an option", KW).kw(OPTIONS),
            Stage::List => Prompt::new("Select a viewport", Accept::POINT),
            Stage::Color => Prompt::new("Enter color name or number (1-255)", NAME).kw(&["Truecolor"]),
            Stage::TrueColor => Prompt::new("Red, Green, Blue", NAME),
            Stage::Layers(c) => {
                let what = match c {
                    Change::Freeze => "to freeze",
                    Change::Thaw => "to thaw",
                    Change::Reset => "to reset",
                    Change::Color(_) => "to change color",
                };
                Prompt::new(format!("Enter layer name(s) {what}"), NAME)
            }
            Stage::Apply(..) => {
                if active(s).is_some() {
                    Prompt::new("Enter an option", KW).kw(&["All", "Select", "Current", "Except current"]).default("Current")
                } else {
                    Prompt::new("Enter an option", KW).kw(&["All", "Select"]).default("Select")
                }
            }
            Stage::Select(..) => Prompt::new("Select viewports", Accept::POINT),
            Stage::Newfrz => Prompt::new("Enter name(s) of new layers frozen in all viewports", NAME),
            Stage::VisLayers => Prompt::new("Enter layer name(s) to change default viewport visibility", NAME),
            Stage::VisDefault(_) => Prompt::new("Enter a default viewport visibility option", KW).kw(&["Frozen", "Thawed"]).default("Thawed"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (&self.stage, &i) {
            (_, Input::Cancel) => Ok(Step::Cancel),
            (Stage::Option, Input::Enter) => Ok(Step::Done),
            (Stage::Option, Input::Keyword(k)) => {
                let k = k.clone();
                self.option(s, &k)
            }
            (Stage::Option, _) => Err(retry("Invalid option keyword.")),
            _ => {
                let stage = std::mem::take(&mut self.stage);
                self.sub(s, stage, i)
            }
        }
    }
}
