//! -LAYER: the command-line form of LAYER. An option loop that ends on Enter; every option runs
//! the `layer.*` commands, so validation matches the JSON form and the whole session is one undo
//! step.

use cadcraft_color::Color;
use cadcraft_doc::Lineweight;
use serde_json::{Value, json};

use super::super::find_command;
use super::super::qselect::wildcard;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

const OPTIONS: &[&str] = &[
    "?",
    "Make",
    "Set",
    "New",
    "Rename",
    "ON",
    "OFF",
    "Color",
    "Ltype",
    "LWeight",
    "TRansparency",
    "MATerial",
    "Plot",
    "Freeze",
    "Thaw",
    "LOck",
    "Unlock",
    "stAte",
    "Description",
    "rEconcile",
];

/// Keyword-only: spaces separate inputs.
const KW: Accept = Accept { point: false, number: false, text: false, select: false, enter: true };
/// A name or value: Space ends the input, as it does in AutoCAD scripts.
const NAME: Accept = Accept { number: true, ..Accept::TEXT };

/// A per-layer change applied to a list of layer names.
#[derive(Clone, Debug)]
enum Change {
    On,
    Off,
    Freeze,
    Thaw,
    Lock,
    Unlock,
    Color(String),
    Ltype(String),
    Lweight(f64),
    Transparency(u64),
    Plot(bool),
    Description(String),
}

impl Change {
    fn params(&self) -> Value {
        match self {
            Change::On => json!({ "on": true }),
            Change::Off => json!({ "on": false }),
            Change::Freeze => json!({ "frozen": true }),
            Change::Thaw => json!({ "frozen": false }),
            Change::Lock => json!({ "locked": true }),
            Change::Unlock => json!({ "locked": false }),
            Change::Color(c) => json!({ "color": c }),
            Change::Ltype(l) => json!({ "linetype": l }),
            Change::Lweight(mm) => json!({ "lineweight": mm }),
            Change::Transparency(t) => json!({ "transparency": t }),
            Change::Plot(p) => json!({ "plot": p }),
            Change::Description(d) => json!({ "description": d }),
        }
    }
    /// The layer-list prompt for this change; `true` when Enter means the current layer.
    fn prompt(&self) -> (String, bool) {
        match self {
            Change::On => ("Enter name list of layer(s) to turn on".into(), false),
            Change::Off => ("Enter name list of layer(s) to turn off".into(), false),
            Change::Freeze => ("Enter name list of layer(s) to freeze".into(), false),
            Change::Thaw => ("Enter name list of layer(s) to thaw".into(), false),
            Change::Lock => ("Enter name list of layer(s) to lock".into(), false),
            Change::Unlock => ("Enter name list of layer(s) to unlock".into(), false),
            Change::Color(c) => (format!("Enter name list of layer(s) for color {c}"), true),
            Change::Ltype(l) => (format!("Enter name list of layer(s) for linetype \"{l}\""), true),
            Change::Lweight(mm) => (format!("Enter name list of layer(s) for lineweight {mm:.2}mm"), true),
            Change::Transparency(t) => (format!("Enter name list of layer(s) for transparency {t}%"), true),
            Change::Plot(_) => ("Enter layer name(s) for this plot preference".into(), true),
            Change::Description(_) => ("Enter name list of layer(s) to apply description".into(), true),
        }
    }
}

#[derive(Clone, Debug, Default)]
enum Stage {
    #[default]
    Option,
    List,
    Make,
    Set,
    New,
    RenameFrom,
    RenameTo(String),
    Color,
    TrueColor,
    Ltype,
    Lweight,
    Transparency,
    Plot,
    Description,
    Layers(Change),
    /// Turning the current layer off asks first.
    ConfirmOff(Vec<String>),
}

#[derive(Default)]
pub struct LayerM {
    stage: Stage,
}

fn current_layer(s: &Session) -> String {
    s.doc().map(|d| d.header.str("CLAYER", "0")).unwrap_or_else(|_| "0".into())
}

/// Run a layer command's body directly: the running command records the single undo step.
fn call(s: &mut Session, id: &str, p: &Value) -> Result<Value> {
    let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.into()))?;
    (spec.run)(s, p)
}

fn message(e: &EngineError) -> String {
    match e {
        EngineError::BadParams { msg, .. } => msg.clone(),
        other => other.to_string(),
    }
}

fn retry(msg: impl Into<String>) -> EngineError {
    EngineError::Other(msg.into())
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

/// Nearest standard lineweight to `mm`.
fn snap_lineweight(mm: f64) -> f64 {
    let want = mm * 100.0;
    let best = Lineweight::STANDARD.iter().copied().min_by(|a, b| (f64::from(*a) - want).abs().total_cmp(&(f64::from(*b) - want).abs())).unwrap_or(0);
    f64::from(best) / 100.0
}

impl LayerM {
    fn option(&mut self, s: &mut Session, k: &str) -> Result<Step> {
        self.stage = match k {
            "?" => Stage::List,
            "Make" => Stage::Make,
            "Set" => Stage::Set,
            "New" => Stage::New,
            "Rename" => Stage::RenameFrom,
            "ON" => Stage::Layers(Change::On),
            "OFF" => Stage::Layers(Change::Off),
            "Color" => Stage::Color,
            "Ltype" => Stage::Ltype,
            "LWeight" => Stage::Lweight,
            "TRansparency" => Stage::Transparency,
            "Plot" => Stage::Plot,
            "Freeze" => Stage::Layers(Change::Freeze),
            "Thaw" => Stage::Layers(Change::Thaw),
            "LOck" => Stage::Layers(Change::Lock),
            "Unlock" => Stage::Layers(Change::Unlock),
            "Description" => Stage::Description,
            other => {
                // MATerial, stAte, rEconcile: report and stay at the option prompt.
                s.echo(format!("-LAYER {other} is not available yet."));
                Stage::Option
            }
        };
        Ok(Step::Continue)
    }

    fn list(&mut self, s: &mut Session, pat: &str) {
        let names = matching(s, if pat.trim().is_empty() { "*" } else { pat });
        let Ok(d) = s.doc() else { return };
        let cur = d.header.str("CLAYER", "0");
        let mut lines = vec![format!("{:<24} {:<26} {:<10} {:<14} {}", "Layer name", "State", "Color", "Linetype", "Lineweight")];
        for l in names.iter().filter_map(|n| d.layer(n)) {
            let state = format!(
                "{} {} {}{}",
                if l.on { "On" } else { "Off" },
                if l.frozen { "Frozen" } else { "Thawed" },
                if l.locked { "Locked" } else { "Unlocked" },
                if l.plot { "" } else { " No plot" }
            );
            lines.push(format!("{:<24} {:<26} {:<10} {:<14} {}", l.name, state, l.color.name(), l.linetype, l.lineweight.name()));
        }
        lines.push(format!("Current layer:  \"{cur}\""));
        for l in lines {
            s.echo(l);
        }
    }

    /// Apply `change` to the named layers through `layer.set`; refusals are reported per layer.
    fn apply(&mut self, s: &mut Session, change: &Change, names: &[String]) {
        for n in names {
            let mut p = change.params();
            if let Some(o) = p.as_object_mut() {
                o.insert("name".into(), json!(n));
            }
            if let Err(e) = call(s, "layer.set", &p) {
                s.echo(format!("Layer \"{n}\": {}", message(&e)));
            }
        }
    }

    fn layers(&mut self, s: &mut Session, change: Change, text: Option<&str>) -> Result<Step> {
        let (_, enter_is_current) = change.prompt();
        let names = match text {
            Some(t) => matching(s, t),
            None if enter_is_current => vec![current_layer(s)],
            None => Vec::new(),
        };
        let cur = current_layer(s);
        if matches!(change, Change::Off) && names.iter().any(|n| n.eq_ignore_ascii_case(&cur)) {
            self.stage = Stage::ConfirmOff(names);
            return Ok(Step::Continue);
        }
        if matches!(change, Change::Freeze) && names.iter().any(|n| n.eq_ignore_ascii_case(&cur)) {
            s.echo(format!("Cannot freeze layer \"{cur}\". It is the CURRENT layer."));
        }
        let names: Vec<String> = names.into_iter().filter(|n| !(matches!(change, Change::Freeze) && n.eq_ignore_ascii_case(&cur))).collect();
        self.apply(s, &change, &names);
        Ok(Step::Continue)
    }

    /// One input at a sub-prompt. Returns to the option prompt unless a value must be re-entered.
    fn sub(&mut self, s: &mut Session, stage: Stage, i: Input) -> Result<Step> {
        let text = match &i {
            Input::Text(t) | Input::Keyword(t) => Some(t.trim().to_string()).filter(|t| !t.is_empty()),
            _ => None,
        };
        let back = |m: &mut LayerM, st: Stage| {
            m.stage = st;
            Ok(Step::Continue)
        };
        match stage {
            Stage::Option => Ok(Step::Continue),
            Stage::List => {
                self.list(s, text.as_deref().unwrap_or("*"));
                Ok(Step::Continue)
            }
            Stage::Make => {
                let Some(n) = text else { return Ok(Step::Continue) };
                let exists = s.doc()?.layer(&n).is_some();
                let r = if exists {
                    call(s, "layer.current", &json!({ "name": n }))
                } else {
                    call(s, "layer.new", &json!({ "name": n, "current": true }))
                };
                if let Err(e) = r {
                    s.echo(message(&e));
                }
                Ok(Step::Continue)
            }
            Stage::Set => {
                let Some(n) = text else { return Ok(Step::Continue) };
                if s.doc()?.layer(&n).is_none() {
                    s.echo(format!("Cannot find layer \"{n}\"."));
                } else if let Err(e) = call(s, "layer.current", &json!({ "name": n })) {
                    s.echo(message(&e));
                }
                Ok(Step::Continue)
            }
            Stage::New => {
                for n in text.as_deref().unwrap_or("").split(',').map(str::trim).filter(|n| !n.is_empty()).take(1000) {
                    if let Err(e) = call(s, "layer.new", &json!({ "name": n })) {
                        s.echo(message(&e));
                    }
                }
                Ok(Step::Continue)
            }
            Stage::RenameFrom => match text {
                None => Ok(Step::Continue),
                Some(n) => match s.doc()?.layer(&n).map(|l| l.name.clone()) {
                    Some(old) => back(self, Stage::RenameTo(old)),
                    None => {
                        s.echo(format!("Cannot find layer \"{n}\"."));
                        Ok(Step::Continue)
                    }
                },
            },
            Stage::RenameTo(old) => {
                if let Some(n) = text
                    && let Err(e) = call(s, "layer.set", &json!({ "name": old, "newName": n }))
                {
                    s.echo(message(&e));
                }
                Ok(Step::Continue)
            }
            Stage::Color => match (&i, text) {
                (Input::Keyword(k), _) if k == "Truecolor" => back(self, Stage::TrueColor),
                (Input::Keyword(_), _) => {
                    s.echo("Color books are not available yet.");
                    Ok(Step::Continue)
                }
                (_, None) => Ok(Step::Continue),
                (_, Some(t)) => match Color::parse(&t) {
                    Some(Color::Index(c)) => back(self, Stage::Layers(Change::Color(c.to_string()))),
                    Some(Color::True(rgb)) => back(self, Stage::Layers(Change::Color(format!("{},{},{}", rgb.0, rgb.1, rgb.2)))),
                    _ => {
                        self.stage = Stage::Color;
                        Err(retry("Enter a color name or a number from 1 to 255."))
                    }
                },
            },
            Stage::TrueColor => match text.as_deref().map(Color::parse) {
                None => Ok(Step::Continue),
                Some(Some(Color::True(rgb))) => back(self, Stage::Layers(Change::Color(format!("{},{},{}", rgb.0, rgb.1, rgb.2)))),
                Some(_) => {
                    self.stage = Stage::TrueColor;
                    Err(retry("Enter three values from 0 to 255, separated by commas."))
                }
            },
            Stage::Ltype => match (&i, text) {
                (Input::Keyword(_), _) => {
                    let names: Vec<String> = s.doc()?.linetypes.iter().map(|l| l.name.clone()).collect();
                    s.echo(format!("Loaded linetypes: {}", names.join(", ")));
                    back(self, Stage::Ltype)
                }
                (_, None) => back(self, Stage::Layers(Change::Ltype("Continuous".into()))),
                (_, Some(t)) => {
                    let found =
                        s.doc()?.linetype(&t).map(|l| l.name.clone()).filter(|n| !["bylayer", "byblock"].contains(&n.to_ascii_lowercase().as_str()));
                    match found {
                        Some(n) => back(self, Stage::Layers(Change::Ltype(n))),
                        None => {
                            s.echo(format!("Linetype \"{t}\" is not loaded. Load it with the LINETYPE command."));
                            Ok(Step::Continue)
                        }
                    }
                }
            },
            Stage::Lweight => match text {
                None => Ok(Step::Continue),
                Some(t) => match t.trim_end_matches("mm").trim().parse::<f64>().ok().filter(|v| v.is_finite() && (0.0..=2.11).contains(v)) {
                    Some(mm) => back(self, Stage::Layers(Change::Lweight(snap_lineweight(mm)))),
                    None => {
                        self.stage = Stage::Lweight;
                        Err(retry("Enter a lineweight from 0.0mm to 2.11mm."))
                    }
                },
            },
            Stage::Transparency => {
                let v = match text {
                    None => Some(0),
                    Some(t) => t.parse::<u64>().ok().filter(|v| *v <= 90),
                };
                match v {
                    Some(v) => back(self, Stage::Layers(Change::Transparency(v))),
                    None => {
                        self.stage = Stage::Transparency;
                        Err(retry("Enter a transparency value from 0 to 90."))
                    }
                }
            }
            Stage::Plot => match &i {
                Input::Keyword(k) => back(self, Stage::Layers(Change::Plot(k != "No"))),
                Input::Enter => back(self, Stage::Layers(Change::Plot(true))),
                _ => {
                    self.stage = Stage::Plot;
                    Err(retry("Enter Plot or No."))
                }
            },
            Stage::Description => back(self, Stage::Layers(Change::Description(text.unwrap_or_default()))),
            Stage::Layers(change) => self.layers(s, change, text.as_deref()),
            Stage::ConfirmOff(names) => {
                let cur = current_layer(s);
                let yes = matches!(&i, Input::Keyword(k) if k == "Yes");
                let names: Vec<String> = names.into_iter().filter(|n| yes || !n.eq_ignore_ascii_case(&cur)).collect();
                self.apply(s, &Change::Off, &names);
                Ok(Step::Continue)
            }
        }
    }
}

impl Interactive for LayerM {
    fn name(&self) -> &'static str {
        "-LAYER"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let cur = current_layer(s);
        s.echo(format!("Current layer:  \"{cur}\""));
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let cur = current_layer(s);
        match &self.stage {
            Stage::Option => Prompt::new("Enter an option", KW).kw(OPTIONS),
            Stage::List => Prompt::new("Enter layer name(s) to list", NAME).default("*"),
            Stage::Make => Prompt::new("Enter name for new layer (becomes the current layer)", NAME).default(cur),
            Stage::Set => Prompt::new("Enter layer name to make current", NAME),
            Stage::New => Prompt::new("Enter name list for new layer(s)", NAME),
            Stage::RenameFrom => Prompt::new("Enter old layer name", NAME),
            Stage::RenameTo(_) => Prompt::new("Enter new layer name", NAME),
            Stage::Color => Prompt::new("Enter color name or number (1-255)", NAME).kw(&["Truecolor", "COlorbook"]),
            Stage::TrueColor => Prompt::new("Red, Green, Blue", NAME),
            Stage::Ltype => Prompt::new("Enter a loaded linetype name", NAME).kw(&["?"]).default("Continuous"),
            Stage::Lweight => Prompt::new("Enter lineweight (0.0mm - 2.11mm)", NAME),
            Stage::Transparency => Prompt::new("Enter transparency value (0-90)", NAME).default("0"),
            Stage::Plot => Prompt::new("Enter a plotting preference", KW).kw(&["Plot", "No"]).default("Plot"),
            Stage::Description => Prompt::new("Enter layer description", Accept::TEXT),
            Stage::Layers(c) => {
                let (msg, enter_is_current) = c.prompt();
                let p = Prompt::new(msg, NAME);
                if enter_is_current { p.default(cur) } else { p }
            }
            Stage::ConfirmOff(_) => Prompt::new(format!("Really want layer \"{cur}\" (the CURRENT layer) off?"), KW).kw(&["Yes", "No"]).default("N"),
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
