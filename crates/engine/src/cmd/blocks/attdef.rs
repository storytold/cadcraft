//! ATTDEF. -ATTDEF at the command line, as AutoCAD 2024 runs it (accoreconsole): the attribute
//! modes (kept in AFLAGS), the tag, prompt and default value (a constant attribute's value
//! instead), then the text's point, justification, style, height and rotation. Align and Fit ask
//! for the baseline's two endpoints, and Enter at the point puts the definition one line under the
//! previous one. The Attribute Definition dialog answers everything but the points
//! ([`Session::attdef_answers`]); the JSON form takes it all at once.

use cadcraft_doc::{Attrib, AttribModes, EntityKind, HAlign, Text, VAlign};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use super::{Input, Interactive, Prompt, Result, Session, Step, bad, bool_or, f64_or, number, point_param, point_req, str_param, v3};
use crate::Accept;
use crate::cmd::curves::KW;
use crate::cmd::draw::{baseline_end, justify, spans_baseline, styled_width};

/// AFLAGS bits, as AutoCAD numbers the modes.
const INVISIBLE: i64 = 1;
const CONSTANT: i64 = 2;
const VERIFY: i64 = 4;
const PRESET: i64 = 8;
const LOCK: i64 = 16;
const MULTILINE: i64 = 32;

const MODES: [&str; 7] = ["Invisible", "Constant", "Verify", "Preset", "Lock position", "Annotative", "Multiple lines"];
const JUSTIFY: [&str; 15] = ["Left", "Center", "Right", "Align", "Middle", "Fit", "TL", "TC", "TR", "ML", "MC", "MR", "BL", "BC", "BR"];

const JUSTIFY_ERROR: &str = "`justify` must be L, C, R, A, M, F, TL, TC, TR, ML, MC, MR, BL, BC or BR";
const TAG_ERROR: &str = "tag must be non-empty without spaces";

/// What a new attribute definition is made from: -ATTDEF's answers, the dialog's or the JSON
/// form's.
#[derive(Clone, Debug)]
struct AttdefSpec {
    tag: String,
    prompt: String,
    default: String,
    at: Vec2,
    height: f64,
    /// Radians.
    rotation: f64,
    justify: (HAlign, VAlign),
    style: String,
    width_factor: f64,
    /// Radians.
    oblique: f64,
    invisible: bool,
    constant: bool,
    modes: AttribModes,
    /// Align and Fit: the baseline's second endpoint (`at` is the first); [`AttdefSpec::baseline`]
    /// fills it in when none was given.
    end: Option<Vec2>,
}

impl AttdefSpec {
    /// An aligned or fit definition's baseline, as AutoCAD stores it: with a given second endpoint,
    /// the baseline's angle and the height (Align) or width factor (Fit) that make the tag span it;
    /// without one, the tag at its own size along the rotation.
    fn baseline(&mut self, s: &Session) -> Result<()> {
        if !spans_baseline(self.justify.0) {
            self.end = None;
            return Ok(());
        }
        let d = s.doc()?;
        let style = d.text_style(&self.style);
        let tag = self.tag.to_ascii_uppercase();
        let Some(end) = self.end else {
            self.end = Some(baseline_end(style, &tag, self.height, self.width_factor, self.at, self.rotation));
            return Ok(());
        };
        let len = self.at.dist(end);
        if !len.is_finite() || len <= 1e-9 {
            return Err(bad("attdef", "the baseline's two endpoints must differ"));
        }
        self.rotation = self.at.angle_to(end);
        let w = styled_width(style, &tag, self.height, self.width_factor);
        if w.is_finite() && w > 1e-9 {
            if self.justify.0 == HAlign::Aligned {
                self.height *= len / w;
            } else {
                self.width_factor *= len / w;
            }
        }
        Ok(())
    }

    fn kind(&self) -> EntityKind {
        let (h, v) = self.justify;
        let left = h == HAlign::Left && v == VAlign::Baseline;
        EntityKind::AttDef(Attrib {
            tag: self.tag.to_ascii_uppercase(),
            text: Text {
                insert: v3(self.at),
                align_pt: if left { None } else { Some(v3(self.end.filter(|_| spans_baseline(h)).unwrap_or(self.at))) },
                height: self.height,
                value: self.default.clone(),
                rotation: self.rotation,
                width_factor: self.width_factor,
                oblique: self.oblique,
                style: self.style.clone(),
                halign: h,
                valign: v,
            },
            invisible: self.invisible,
            constant: self.constant,
            prompt: self.prompt.clone(),
            props: Default::default(),
            multiline: None,
            modes: self.modes,
        })
    }
}

/// A tag for the JSON form and the dialog: not empty, no spaces (typed -ATTDEF takes spaces, as
/// AutoCAD's command line does).
fn valid_tag(t: &str) -> bool {
    !t.trim().is_empty() && !t.trim().contains(' ')
}

/// The text style a new definition is written with: `name` (which must exist) or the current
/// TEXTSTYLE, as (name, fixed height or 0, width factor, obliquing angle).
fn attdef_style(s: &Session, name: Option<&str>) -> Result<(String, f64, f64, f64)> {
    let d = s.doc()?;
    let name = name.map_or_else(|| d.header.str("TEXTSTYLE", "Standard"), str::to_string);
    let st = d.text_style(&name).ok_or_else(|| bad("attdef", format!("Cannot find text style \"{name}\".")))?;
    Ok((st.name.clone(), st.height.max(0.0), if st.width_factor > 0.0 { st.width_factor } else { 1.0 }, st.oblique))
}

/// The attribute definition last added to the space being edited: Enter at -ATTDEF's point (and
/// the dialog's "Align below previous attribute definition") puts the new one under it.
fn previous_attdef(s: &Session) -> Option<Attrib> {
    let d = s.doc().ok()?;
    d.space(&s.space())?
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::AttDef(a) => Some(a.clone()),
            _ => None,
        })
        .last()
}

/// Where a definition one line under text `t` goes: its justification point moved down by the
/// line spacing TEXT continues with (5/3 of the height), along the text's own down direction.
fn below_point(t: &Text) -> Vec2 {
    t.justify_point() + Vec2::from_angle(t.rotation - std::f64::consts::FRAC_PI_2) * (t.height * 5.0 / 3.0)
}

/// ATTDEF's JSON form.
pub(super) fn run_attdef(s: &mut Session, p: &Value) -> Result<Value> {
    let tag = str_param(p, "tag").ok_or_else(|| bad("attdef", "`tag` is required"))?;
    if !valid_tag(tag) {
        return Err(bad("attdef", TAG_ERROR));
    }
    let tag = tag.trim();
    let constant = bool_or(p, "constant", false);
    let modes =
        AttribModes { verify: bool_or(p, "verify", false), preset: bool_or(p, "preset", false), lock_position: bool_or(p, "lockPosition", false) };
    let texts = |at, height, rotation, justify, style: String, width_factor, oblique| AttdefSpec {
        tag: tag.to_string(),
        // A constant attribute is never asked for, so it has no prompt.
        prompt: if constant { String::new() } else { str_param(p, "prompt").unwrap_or(tag).to_string() },
        default: str_param(p, "default").unwrap_or("").to_string(),
        at,
        height,
        rotation,
        justify,
        style,
        width_factor,
        oblique,
        invisible: bool_or(p, "invisible", false),
        constant,
        modes,
        end: None,
    };
    let mut spec = if bool_or(p, "below", false) {
        let prev = previous_attdef(s).ok_or_else(|| bad("attdef", "no attribute definition to align below"))?;
        let t = &prev.text;
        texts(below_point(t), t.height, t.rotation, (t.halign, t.valign), t.style.clone(), t.width_factor, t.oblique)
    } else {
        let at = point_req("attdef", p, "at")?;
        let (style, fixed, width_factor, oblique) = attdef_style(s, str_param(p, "style"))?;
        let justify = match str_param(p, "justify") {
            Some(j) => justify(j).ok_or_else(|| bad("attdef", JUSTIFY_ERROR))?,
            None => (HAlign::Left, VAlign::Baseline),
        };
        let height = match p.get("height").and_then(Value::as_f64) {
            Some(h) if h > 0.0 && h.is_finite() => h,
            Some(_) => return Err(bad("attdef", "`height` must be positive")),
            None if fixed > 0.0 => fixed,
            None => s.doc()?.header.f64("TEXTSIZE", 0.2),
        };
        let end = point_param(p, "end");
        if end.is_some() && !spans_baseline(justify.0) {
            return Err(bad("attdef", "`end` is for the A and F justifications"));
        }
        AttdefSpec { end, ..texts(at, height, f64_or(p, "rotation", 0.0).to_radians(), justify, style, width_factor, oblique) }
    };
    spec.baseline(s)?;
    Ok(json!({ "handle": s.add_entity(spec.kind())?.hex() }))
}

/// Where -ATTDEF is in its prompts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Ask {
    #[default]
    Modes,
    Tag,
    Prompt,
    Value,
    Point,
    /// Align and Fit: the baseline's second endpoint.
    End,
    Justify,
    Style,
    Height,
    Rotation,
}

pub(super) struct AttdefM {
    ask: Ask,
    flags: i64,
    tag: String,
    prompt: String,
    value: String,
    justify: (HAlign, VAlign),
    style: String,
    at: Vec2,
    /// Align and Fit: the baseline's second endpoint.
    end: Option<Vec2>,
    /// Typed, the dialog's, a fixed-height style's or the previous definition's; else TEXTSIZE.
    height: Option<f64>,
    /// Radians: typed, the dialog's or the previous definition's.
    rotation: Option<f64>,
    /// Width factor and obliquing angle: the previous definition's (Enter at the point); else the
    /// style's.
    shape: Option<(f64, f64)>,
    /// The dialog answered everything but the points.
    from_dialog: bool,
}

impl Default for AttdefM {
    fn default() -> Self {
        AttdefM {
            ask: Ask::Modes,
            flags: LOCK,
            tag: String::new(),
            prompt: String::new(),
            value: String::new(),
            justify: (HAlign::Left, VAlign::Baseline),
            style: "Standard".into(),
            at: Vec2::ZERO,
            end: None,
            height: None,
            rotation: None,
            shape: None,
            from_dialog: false,
        }
    }
}

impl AttdefM {
    fn echo_modes(&self, s: &mut Session) {
        let yn = |bit: i64| if self.flags & bit != 0 { "Y" } else { "N" };
        s.echo("Current attribute modes:");
        s.echo(format!(
            "Invisible={}  Constant={}  Verify={}  Preset={}  Lock position={}  Annotative=N  Multiple line={}",
            yn(INVISIBLE),
            yn(CONSTANT),
            yn(VERIFY),
            yn(PRESET),
            yn(LOCK),
            yn(MULTILINE)
        ));
    }

    fn echo_style(&self, s: &mut Session) -> Result<()> {
        let th = s.doc()?.header.f64("TEXTSIZE", 0.2);
        s.echo(format!("Current text style:  \"{}\"  Text height:  {th:.4}  Justify:  {}", self.style, justify_name(self.justify)));
        Ok(())
    }

    fn point_prompt(&self) -> &'static str {
        match self.justify {
            (HAlign::Aligned | HAlign::Fit, _) => "Specify first endpoint of text baseline",
            (HAlign::Center, VAlign::Baseline) => "Specify center point of text",
            (HAlign::Right, VAlign::Baseline) => "Specify right endpoint of text baseline",
            (HAlign::Middle, _) | (HAlign::Center, VAlign::Middle) => "Specify middle point of text",
            (HAlign::Left, VAlign::Top) => "Specify top-left point of text",
            (HAlign::Center, VAlign::Top) => "Specify top-center point of text",
            (HAlign::Right, VAlign::Top) => "Specify top-right point of text",
            (HAlign::Left, VAlign::Middle) => "Specify middle-left point of text",
            (HAlign::Right, VAlign::Middle) => "Specify middle-right point of text",
            (HAlign::Left, VAlign::Bottom) => "Specify bottom-left point of text",
            (HAlign::Center, VAlign::Bottom) => "Specify bottom-center point of text",
            (HAlign::Right, VAlign::Bottom) => "Specify bottom-right point of text",
            _ => "Specify start point of text",
        }
    }

    /// A fixed-height style's height: it is not asked.
    fn fixed_height(&self, s: &Session) -> Option<f64> {
        s.doc().ok()?.text_style(&self.style).map(|st| st.height).filter(|h| h.is_finite() && *h > 0.0)
    }

    /// The dialog's answers: everything but the points.
    fn take_answers(&mut self, s: &Session, a: &Value) -> Result<()> {
        let tag = str_param(a, "tag").filter(|t| valid_tag(t)).ok_or_else(|| bad("attdef", TAG_ERROR))?;
        self.tag = tag.trim().to_string();
        let bit = |key, b| if bool_or(a, key, false) { b } else { 0 };
        self.flags =
            bit("invisible", INVISIBLE) | bit("constant", CONSTANT) | bit("verify", VERIFY) | bit("preset", PRESET) | bit("lockPosition", LOCK);
        self.prompt = if self.flags & CONSTANT != 0 { String::new() } else { str_param(a, "prompt").unwrap_or("").to_string() };
        self.value = str_param(a, "default").unwrap_or("").to_string();
        if let Some(j) = str_param(a, "justify") {
            self.justify = justify(j).ok_or_else(|| bad("attdef", JUSTIFY_ERROR))?;
        }
        if let Some(n) = str_param(a, "style") {
            self.style = attdef_style(s, Some(n))?.0;
        }
        self.height = a.get("height").and_then(Value::as_f64).filter(|h| *h > 0.0 && h.is_finite());
        self.rotation = a.get("rotation").and_then(Value::as_f64).filter(|r| r.is_finite()).map(f64::to_radians);
        self.from_dialog = true;
        Ok(())
    }

    /// Everything is answered: add the definition.
    fn finish(&mut self, s: &mut Session) -> Result<Step> {
        let (style, fixed, width_factor, oblique) = attdef_style(s, Some(&self.style)).unwrap_or_else(|_| (self.style.clone(), 0.0, 1.0, 0.0));
        let height = match self.height {
            Some(h) => h,
            None if fixed > 0.0 => fixed,
            None => s.doc()?.header.f64("TEXTSIZE", 0.2),
        };
        let (width_factor, oblique) = self.shape.unwrap_or((width_factor, oblique));
        let mut spec = AttdefSpec {
            tag: self.tag.to_uppercase(),
            prompt: std::mem::take(&mut self.prompt),
            default: std::mem::take(&mut self.value),
            at: self.at,
            height,
            rotation: self.rotation.unwrap_or(0.0),
            justify: self.justify,
            style,
            width_factor,
            oblique,
            invisible: self.flags & INVISIBLE != 0,
            constant: self.flags & CONSTANT != 0,
            modes: AttribModes { verify: self.flags & VERIFY != 0, preset: self.flags & PRESET != 0, lock_position: self.flags & LOCK != 0 },
            end: self.end,
        };
        spec.baseline(s)?;
        s.add_entity(spec.kind())?;
        Ok(Step::Done)
    }
}

/// The justification's name in "Current text style: … Justify: …".
fn justify_name(j: (HAlign, VAlign)) -> &'static str {
    JUSTIFY.into_iter().find(|k| justify(k) == Some(j)).unwrap_or("Left")
}

impl Interactive for AttdefM {
    fn name(&self) -> &'static str {
        "ATTDEF"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let d = s.doc()?;
        self.flags = d.header.i64("AFLAGS", LOCK);
        self.style = d.header.str("TEXTSTYLE", "Standard");
        if let Some(a) = s.attdef_answers.take() {
            self.take_answers(s, &a)?;
            self.echo_style(s)?;
            self.ask = Ask::Point;
        } else {
            self.echo_modes(s);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match self.ask {
            Ask::Modes => Prompt::new("Enter an option to change", KW).kw(&MODES).default("done"),
            Ask::Tag => Prompt::new("Enter attribute tag name", Accept::TEXT),
            Ask::Prompt => Prompt::new("Enter attribute prompt", Accept::TEXT),
            Ask::Value if self.flags & CONSTANT != 0 => Prompt::new("Enter attribute value", Accept::TEXT),
            Ask::Value => Prompt::new("Enter default attribute value", Accept::TEXT),
            // Once a justification is chosen AutoCAD asks for its point alone; so after the dialog.
            Ask::Point if self.justify == (HAlign::Left, VAlign::Baseline) && !self.from_dialog => {
                Prompt::new(self.point_prompt(), Accept::POINT).kw(&["Justify", "Style"])
            }
            Ask::Point => Prompt::new(self.point_prompt(), Accept::POINT),
            Ask::End => Prompt::new("Specify second endpoint of text baseline", Accept::POINT).base(self.at),
            Ask::Justify => Prompt::new("Enter an option", Accept::TEXT).kw(&JUSTIFY),
            Ask::Style => Prompt::new("Enter style name", Accept::TEXT).kw(&["?"]).default(self.style.clone()),
            Ask::Height => {
                let th = s.doc().map(|d| d.header.f64("TEXTSIZE", 0.2)).unwrap_or(0.2);
                Prompt::new("Specify height", Accept::POINT_OR_NUMBER).default(format!("{th:.4}")).base(self.at)
            }
            Ask::Rotation => Prompt::new("Specify rotation angle of text", Accept::POINT_OR_NUMBER).default("0").base(self.at),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.ask {
            Ask::Modes => match &i {
                Input::Keyword(k) => {
                    let bit = match k.as_str() {
                        "Invisible" => INVISIBLE,
                        "Constant" => CONSTANT,
                        "Verify" => VERIFY,
                        "Preset" => PRESET,
                        "Lock position" => LOCK,
                        _ => {
                            s.echo(format!("{k} attributes are not available yet."));
                            return Ok(Step::Continue);
                        }
                    };
                    self.flags ^= bit;
                    s.doc_mut()?.header.set_i64("AFLAGS", self.flags);
                    self.echo_modes(s);
                }
                Input::Enter => self.ask = Ask::Tag,
                Input::Text(_) => s.echo("Invalid option keyword."),
                _ => {}
            },
            Ask::Tag => match &i {
                // AutoCAD takes spaces in a tag here, and upper-cases it.
                Input::Text(t) if !t.trim().is_empty() => {
                    self.tag = t.trim().to_string();
                    self.ask = if self.flags & CONSTANT != 0 { Ask::Value } else { Ask::Prompt };
                }
                Input::Text(_) | Input::Enter => s.echo("Tag cannot be null"),
                _ => {}
            },
            Ask::Prompt | Ask::Value => {
                let t = match i {
                    Input::Text(t) => t,
                    Input::Enter => String::new(),
                    _ => return Ok(Step::Continue),
                };
                if self.ask == Ask::Prompt {
                    self.prompt = t;
                    self.ask = Ask::Value;
                } else {
                    self.value = t;
                    self.echo_style(s)?;
                    self.ask = Ask::Point;
                }
            }
            Ask::Point => match i {
                Input::Point(p) => {
                    self.at = p;
                    if spans_baseline(self.justify.0) {
                        self.ask = Ask::End;
                    } else if self.from_dialog {
                        return self.finish(s);
                    } else {
                        // A fixed-height style gives the height: only the rotation is asked.
                        self.height = self.fixed_height(s);
                        self.ask = if self.height.is_some() { Ask::Rotation } else { Ask::Height };
                    }
                }
                // Enter: one line under the previous definition, with its text settings.
                Input::Enter => {
                    let Some(prev) = previous_attdef(s) else {
                        s.echo("No previous attribute definition to align below.");
                        return Ok(Step::Continue);
                    };
                    let t = &prev.text;
                    self.at = below_point(t);
                    self.justify = (t.halign, t.valign);
                    self.style = t.style.clone();
                    self.height = Some(t.height);
                    self.rotation = Some(t.rotation);
                    self.shape = Some((t.width_factor, t.oblique));
                    self.end = None;
                    return self.finish(s);
                }
                Input::Keyword(k) if k == "Justify" => self.ask = Ask::Justify,
                Input::Keyword(_) => self.ask = Ask::Style,
                _ => {}
            },
            Ask::End => {
                if let Input::Point(p) = i {
                    if self.at.dist(p) <= 1e-9 {
                        s.echo("The second endpoint must differ from the first.");
                        return Ok(Step::Continue);
                    }
                    self.end = Some(p);
                    // Align takes its height from the baseline; Fit asks for it unless it is known.
                    if self.height.is_none() {
                        self.height = self.fixed_height(s);
                    }
                    if self.justify.0 == HAlign::Aligned || self.from_dialog || self.height.is_some() {
                        return self.finish(s);
                    }
                    self.ask = Ask::Height;
                }
            }
            Ask::Justify => {
                // A justification goes on to its point; anything else is asked again.
                if let Input::Keyword(k) | Input::Text(k) = &i {
                    match justify(k) {
                        Some(j) => {
                            self.justify = j;
                            self.ask = Ask::Point;
                        }
                        None => s.echo("Invalid option keyword."),
                    }
                }
            }
            Ask::Style => {
                let name = match i {
                    Input::Enter => self.style.clone(),
                    Input::Text(t) | Input::Keyword(t) => t.trim().to_string(),
                    _ => return Ok(Step::Continue),
                };
                if name == "?" {
                    let names: Vec<String> = s.doc()?.text_styles.iter().map(|st| st.name.clone()).collect();
                    s.echo(format!("Text styles: {}", names.join(", ")));
                    return Ok(Step::Continue);
                }
                match s.doc()?.text_style(&name).map(|st| st.name.clone()) {
                    Some(n) => {
                        self.style = n;
                        self.echo_style(s)?;
                        self.ask = Ask::Point;
                    }
                    None => s.echo(format!("Cannot find text style \"{name}\".")),
                }
            }
            Ask::Height => {
                let th = s.doc()?.header.f64("TEXTSIZE", 0.2);
                let h = match &i {
                    Input::Enter => th,
                    Input::Point(p) => self.at.dist(*p),
                    // A distance, as TEXT takes its height (1'6" in feet and inches, too).
                    Input::Text(t) => number(t).unwrap_or(0.0),
                    _ => return Ok(Step::Continue),
                };
                if !(h.is_finite() && h > 0.0) {
                    s.echo("Requires a positive height.");
                    return Ok(Step::Continue);
                }
                self.height = Some(h);
                s.doc_mut()?.header.set_f64("TEXTSIZE", h);
                // Fit: the angle is the baseline's, so the height is the last question.
                if spans_baseline(self.justify.0) {
                    return self.finish(s);
                }
                self.ask = Ask::Rotation;
            }
            Ask::Rotation => {
                let r = match &i {
                    Input::Enter => 0.0,
                    Input::Point(p) => self.at.angle_to(*p),
                    Input::Text(t) => match s.angle_settings().direction(t) {
                        Some(a) => a,
                        None => {
                            s.echo("Requires an angle.");
                            return Ok(Step::Continue);
                        }
                    },
                    _ => return Ok(Step::Continue),
                };
                self.rotation = Some(r);
                return self.finish(s);
            }
        }
        Ok(Step::Continue)
    }
}
