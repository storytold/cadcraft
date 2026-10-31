//! CHPROP: change object properties at the command line, as AutoCAD 2024 asks for them.
//!
//! "Enter property to change [Color/LAyer/LType/ltScale/LWeight/Thickness/TRansparency]" comes
//! back after each answer until Enter; each property asks for its value and applies it at once
//! (through `properties.set`). Measured from AutoLISP's `command` (accoreconsole, every object read
//! back with `entget`):
//! - a colour is 1–255, a name (red … white), BYLAYER or BYBLOCK, or Truecolor and then "r,g,b";
//!   0, 256 and 300 are refused with "Invalid color number.";
//! - a layer must exist: `Cannot find layer "KHONGCO".`;
//! - a linetype not loaded yet is loaded from the standard ones (PHANTOM2);
//! - a lineweight goes to the nearest standard one, with a note: 0.33 gives "Lineweight rounded to
//!   nearest valid value of 0.35 mm";
//! - `L` alone is "Ambiguous response, please clarify...";
//! - objects on locked layers are left out at "Select objects", which says how many after what was
//!   found ("1 was on a locked layer.", "2 were on a locked layer."); with none left, Enter ends
//!   the command.

use std::collections::HashSet;

use cadcraft_color::Color;
use cadcraft_doc::{Entity, Handle, Lineweight};
use serde_json::{Value, json};

use super::super::curves::{KW, is_locked};
use super::super::machines::{SelOutcome, SelectPhase, number};
use super::super::targets;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

const PROPERTIES: [&str; 7] = ["Color", "LAyer", "LType", "ltScale", "LWeight", "Thickness", "TRansparency"];

/// What CHPROP asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ask {
    Property,
    Color,
    TrueColor,
    Layer,
    Linetype,
    LtScale,
    Lineweight,
    Thickness,
    Transparency,
}

pub(super) struct ChpropM {
    select: SelectPhase,
    handles: Vec<Handle>,
    /// `None` while objects are selected.
    ask: Option<Ask>,
}

impl ChpropM {
    pub(super) fn new() -> Self {
        ChpropM { select: SelectPhase::default(), handles: Vec::new(), ask: None }
    }

    /// The selection's common value of a property, or "varies".
    fn current(&self, s: &Session, f: impl Fn(&Entity) -> String) -> String {
        let Ok(d) = s.doc() else { return String::new() };
        let mut vals = self.handles.iter().filter_map(|h| d.entity(*h)).map(|e| f(e));
        let first = vals.next().unwrap_or_default();
        if vals.all(|v| v == first) { first } else { "varies".into() }
    }

    /// Apply `props` to the selection through `properties.set`.
    fn apply(&self, s: &mut Session, props: Value) -> Result<()> {
        let mut p = json!({ "handles": self.handles.iter().map(|h| h.hex()).collect::<Vec<_>>() });
        if let (Some(p), Value::Object(props)) = (p.as_object_mut(), props) {
            p.extend(props);
        }
        super::run_set(s, &p).map(|_| ())
    }

    /// Take an answer to `ask`; `false` when it was refused (the prompt comes again).
    fn answer(&mut self, s: &mut Session, ask: Ask, t: &str) -> Result<bool> {
        let t = t.trim();
        let word = t.strip_prefix('_').unwrap_or(t);
        match ask {
            Ask::Color => match color(word) {
                Some(c) => self.apply(s, json!({ "color": color_json(c) }))?,
                None => {
                    s.echo("Invalid color number.");
                    return Ok(false);
                }
            },
            Ask::TrueColor => match Color::parse(word) {
                Some(c @ Color::True(_)) => self.apply(s, json!({ "color": color_json(c) }))?,
                _ => {
                    s.echo("Requires three integers from 0 to 255, separated by commas.");
                    return Ok(false);
                }
            },
            Ask::Layer => match s.doc()?.layer(word).map(|l| l.name.clone()) {
                Some(name) => self.apply(s, json!({ "layer": name }))?,
                None => {
                    s.echo(format!("Cannot find layer \"{word}\"."));
                    return Ok(false);
                }
            },
            Ask::Linetype => {
                let by = ["ByLayer", "ByBlock"].into_iter().find(|b| b.eq_ignore_ascii_case(word));
                let name = match by {
                    Some(b) => Some(b.to_string()),
                    None => {
                        // Not loaded yet: load it from the standard linetypes, as AutoCAD does.
                        if s.doc()?.linetype(word).is_none() {
                            let _ = super::run_linetype(s, &json!({ "load": word }));
                        }
                        s.doc()?.linetype(word).map(|l| l.name.clone())
                    }
                };
                match name {
                    Some(n) => self.apply(s, json!({ "linetype": n }))?,
                    None => {
                        s.echo(format!("Cannot find linetype \"{word}\"."));
                        return Ok(false);
                    }
                }
            }
            Ask::LtScale | Ask::Thickness => {
                let v = number(word).filter(|v| v.is_finite() && (ask == Ask::Thickness || *v > 0.0));
                match v {
                    Some(v) => self.apply(s, if ask == Ask::LtScale { json!({ "ltscale": v }) } else { json!({ "thickness": v }) })?,
                    None => {
                        s.echo(if ask == Ask::LtScale { "Requires a positive number." } else { "Requires a number." });
                        return Ok(false);
                    }
                }
            }
            Ask::Lineweight => match lineweight(word) {
                Some((lw, rounded)) => {
                    if let (true, Lineweight::Mm100(v)) = (rounded, lw) {
                        s.echo(format!("Lineweight rounded to nearest valid value of {:.2} mm", f64::from(v) / 100.0));
                    }
                    let v = match lw {
                        Lineweight::Mm100(v) => json!(f64::from(v) / 100.0),
                        Lineweight::ByLayer => json!("ByLayer"),
                        Lineweight::ByBlock => json!("ByBlock"),
                        Lineweight::Default => json!("Default"),
                    };
                    self.apply(s, json!({ "lineweight": v }))?;
                }
                None => {
                    s.echo("Invalid lineweight.");
                    return Ok(false);
                }
            },
            Ask::Transparency => {
                if self.apply(s, json!({ "transparency": word })).is_err() {
                    s.echo("Requires ByLayer, ByBlock or an integer from 0 to 90.");
                    return Ok(false);
                }
            }
            Ask::Property => {}
        }
        Ok(true)
    }
}

/// The objects among `hs` on locked layers.
fn locked(s: &Session, hs: &[Handle]) -> HashSet<Handle> {
    hs.iter().copied().filter(|h| is_locked(s, *h)).collect()
}

fn say_locked(s: &mut Session, n: usize) {
    match n {
        0 => {}
        1 => s.echo("1 was on a locked layer."),
        n => s.echo(format!("{n} were on a locked layer.")),
    }
}

/// CHPROP's JSON form: `properties.set` on the objects that are not on locked layers, which the
/// command leaves out too; "locked" says how many were.
pub(super) fn run_chprop(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let locked = locked(s, &hs);
    if locked.is_empty() {
        return super::run_set(s, p).map(|r| with_locked(r, 0));
    }
    let free: Vec<Handle> = hs.into_iter().filter(|h| !locked.contains(h)).collect();
    let r = if free.is_empty() {
        json!({ "changed": 0 })
    } else {
        let mut q = p.as_object().cloned().unwrap_or_default();
        q.insert("handles".into(), json!(free.iter().map(|h| h.hex()).collect::<Vec<_>>()));
        super::run_set(s, &Value::Object(q))?
    };
    Ok(with_locked(r, locked.len()))
}

/// `r` with how many objects were left out on locked layers.
fn with_locked(mut r: Value, n: usize) -> Value {
    if let Some(o) = r.as_object_mut() {
        o.insert("locked".into(), json!(n));
    }
    r
}

/// A colour typed at CHPROP: 1–255, a name, BYLAYER or BYBLOCK (not 0 or 256, which COLOR takes).
fn color(t: &str) -> Option<Color> {
    if t.bytes().all(|b| b.is_ascii_digit()) {
        return t.parse::<u16>().ok().filter(|i| (1..=255).contains(i)).and_then(|i| u8::try_from(i).ok()).map(Color::Index);
    }
    Color::parse(t).filter(|c| !matches!(c, Color::True(_)))
}

fn color_json(c: Color) -> Value {
    match c {
        Color::Index(i) => json!(i),
        Color::True(rgb) => json!(format!("{},{},{}", rgb.0, rgb.1, rgb.2)),
        other => json!(other.name()),
    }
}

fn lineweight_name(lw: Lineweight) -> String {
    match lw {
        Lineweight::Mm100(v) => format!("{:.2} mm", f64::from(v) / 100.0),
        Lineweight::ByLayer => "ByLayer".into(),
        Lineweight::ByBlock => "ByBlock".into(),
        Lineweight::Default => "Default".into(),
    }
}

/// A lineweight typed in millimetres (the nearest standard one, and whether that rounded it), or
/// ByLayer, ByBlock or Default.
fn lineweight(t: &str) -> Option<(Lineweight, bool)> {
    match t.to_ascii_lowercase().as_str() {
        "bylayer" => return Some((Lineweight::ByLayer, false)),
        "byblock" => return Some((Lineweight::ByBlock, false)),
        "default" => return Some((Lineweight::Default, false)),
        _ => {}
    }
    let mm = number(t).filter(|v| v.is_finite() && *v >= 0.0)?;
    let want = mm * 100.0;
    let near = Lineweight::STANDARD.iter().copied().min_by(|a, b| (f64::from(*a) - want).abs().total_cmp(&(f64::from(*b) - want).abs()))?;
    Some((Lineweight::Mm100(near), (f64::from(near) - want).abs() > 1e-9))
}

impl Interactive for ChpropM {
    fn name(&self) -> &'static str {
        "CHPROP"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.select = SelectPhase::begin(s);
        if self.select.done {
            let locked = locked(s, &self.select.picked);
            if !locked.is_empty() {
                self.select.picked.retain(|h| !locked.contains(h));
                say_locked(s, locked.len());
                s.set_selection(self.select.picked.clone());
                // Nothing left of the pickfirst selection: ask for objects instead.
                self.select.done = !self.select.picked.is_empty();
            }
        }
        if self.select.done {
            self.handles = self.select.picked.clone();
            self.ask = Some(Ask::Property);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let Some(ask) = self.ask else { return self.select.prompt() };
        match ask {
            Ask::Property => Prompt::new("Enter property to change", Accept::TEXT).kw(&PROPERTIES),
            Ask::Color => {
                Prompt::new("New color", Accept::TEXT).kw(&["Truecolor"]).default(self.current(s, |e| e.common.color.name().to_ascii_uppercase()))
            }
            Ask::TrueColor => Prompt::new("Red,Green,Blue", Accept::TEXT),
            Ask::Layer => Prompt::new("Enter new layer name", Accept::TEXT).default(self.current(s, |e| e.common.layer.clone())),
            Ask::Linetype => Prompt::new("Enter new linetype name", Accept::TEXT).default(self.current(s, |e| e.common.linetype.clone())),
            Ask::LtScale => {
                Prompt::new("Specify new linetype scale", Accept::NUMBER).default(self.current(s, |e| format!("{:.4}", e.common.ltscale)))
            }
            Ask::Lineweight => Prompt::new("Enter new lineweight", Accept::TEXT).default(self.current(s, |e| lineweight_name(e.common.lineweight))),
            Ask::Thickness => Prompt::new("Specify new thickness", Accept::NUMBER).default(self.current(s, |e| format!("{:.4}", e.common.thickness))),
            Ask::Transparency => Prompt::new("Enter transparency value", KW).default(self.current(s, |e| e.common.transparency.name())),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Some(ask) = self.ask else {
            // Objects on locked layers are left out, and said after what was found.
            let (i, n) = match i {
                Input::Pick(hs) if !self.select.removing => {
                    let locked = locked(s, &hs);
                    (Input::Pick(hs.into_iter().filter(|h| !locked.contains(h)).collect()), locked.len())
                }
                i => (i, 0),
            };
            let outcome = self.select.feed(s, &i)?;
            say_locked(s, n);
            return Ok(match outcome {
                SelOutcome::More => Step::Continue,
                SelOutcome::Done(hs) => {
                    self.handles = hs;
                    self.ask = Some(Ask::Property);
                    Step::Continue
                }
                SelOutcome::Empty => {
                    s.set_selection(Vec::new());
                    Step::Done
                }
            });
        };
        match (ask, i) {
            (Ask::Property, Input::Enter) => {
                s.set_selection(Vec::new());
                return Ok(Step::Done);
            }
            (Ask::Property, Input::Keyword(k)) => {
                self.ask = Some(match k.as_str() {
                    "Color" => Ask::Color,
                    "LAyer" => Ask::Layer,
                    "LType" => Ask::Linetype,
                    "ltScale" => Ask::LtScale,
                    "LWeight" => Ask::Lineweight,
                    "Thickness" => Ask::Thickness,
                    _ => Ask::Transparency,
                });
            }
            (Ask::Property, Input::Text(t)) => {
                let t = t.trim().trim_start_matches('_').to_ascii_lowercase();
                let n = PROPERTIES.iter().filter(|k| k.to_ascii_lowercase().starts_with(&t)).count();
                s.echo(if n > 1 { "Ambiguous response, please clarify..." } else { "Invalid option keyword." });
            }
            (Ask::Color, Input::Keyword(k)) if k == "Truecolor" => self.ask = Some(Ask::TrueColor),
            // Enter keeps the value.
            (_, Input::Enter) => self.ask = Some(Ask::Property),
            (ask, Input::Text(t) | Input::Keyword(t)) => {
                // A refused answer asks again.
                let taken = self.answer(s, ask, &t)?;
                self.ask = Some(if taken { Ask::Property } else { ask });
            }
            _ => {}
        }
        Ok(Step::Continue)
    }
}
