//! -ATTDEF at the command line, as AutoCAD 2024 runs it (accoreconsole): the attribute modes
//! (kept in AFLAGS), the tag, prompt and default value (a constant attribute's value instead),
//! then the text's point, justification, style, height and rotation.

use cadcraft_doc::{AttribModes, HAlign, VAlign};

use super::{Accept, Attrib, EntityKind, Input, Interactive, Prompt, Result, Session, Step, Text, Vec2, number, v3};
use crate::cmd::curves::KW;
use crate::cmd::draw::justify;

/// AFLAGS bits, as AutoCAD numbers the modes.
const INVISIBLE: i64 = 1;
const CONSTANT: i64 = 2;
const VERIFY: i64 = 4;
const PRESET: i64 = 8;
const LOCK: i64 = 16;
const MULTILINE: i64 = 32;

const MODES: [&str; 7] = ["Invisible", "Constant", "Verify", "Preset", "Lock position", "Annotative", "Multiple lines"];
const JUSTIFY: [&str; 15] = ["Left", "Center", "Right", "Align", "Middle", "Fit", "TL", "TC", "TR", "ML", "MC", "MR", "BL", "BC", "BR"];

/// Where -ATTDEF is in its prompts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Ask {
    #[default]
    Modes,
    Tag,
    Prompt,
    Value,
    Point,
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
    height: f64,
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
            height: 0.0,
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

    fn finish(&mut self, s: &mut Session, rotation: f64) -> Result<Step> {
        let (h, v) = self.justify;
        let left = h == HAlign::Left && v == VAlign::Baseline;
        s.add_entity(EntityKind::AttDef(Attrib {
            tag: self.tag.to_uppercase(),
            text: Text {
                insert: v3(self.at),
                align_pt: if left { None } else { Some(v3(self.at)) },
                height: self.height,
                value: std::mem::take(&mut self.value),
                rotation,
                width_factor: 1.0,
                oblique: 0.0,
                style: self.style.clone(),
                halign: h,
                valign: v,
            },
            invisible: self.flags & INVISIBLE != 0,
            constant: self.flags & CONSTANT != 0,
            prompt: std::mem::take(&mut self.prompt),
            props: Default::default(),
            multiline: None,
            modes: AttribModes { verify: self.flags & VERIFY != 0, preset: self.flags & PRESET != 0, lock_position: self.flags & LOCK != 0 },
        }))?;
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
        self.echo_modes(s);
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match self.ask {
            Ask::Modes => Prompt::new("Enter an option to change", KW).kw(&MODES).default("done"),
            Ask::Tag => Prompt::new("Enter attribute tag name", Accept::TEXT),
            Ask::Prompt => Prompt::new("Enter attribute prompt", Accept::TEXT),
            Ask::Value if self.flags & CONSTANT != 0 => Prompt::new("Enter attribute value", Accept::TEXT),
            Ask::Value => Prompt::new("Enter default attribute value", Accept::TEXT),
            // Once a justification is chosen AutoCAD asks for its point alone.
            Ask::Point if self.justify == (HAlign::Left, VAlign::Baseline) => {
                Prompt::new(self.point_prompt(), Accept::POINT).kw(&["Justify", "Style"])
            }
            Ask::Point => Prompt::new(self.point_prompt(), Accept::POINT),
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
                    self.ask = Ask::Height;
                }
                Input::Keyword(k) if k == "Justify" => self.ask = Ask::Justify,
                Input::Keyword(_) => self.ask = Ask::Style,
                _ => {}
            },
            Ask::Justify => {
                // A justification goes on to its point; anything else is asked again.
                if let Input::Keyword(k) | Input::Text(k) = &i {
                    match justify(k) {
                        // Aligned and fitted text take two points: not here yet.
                        Some((HAlign::Aligned | HAlign::Fit, _)) => s.echo("Align and Fit are not available here yet."),
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
                self.height = h;
                s.doc_mut()?.header.set_f64("TEXTSIZE", h);
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
                return self.finish(s, r);
            }
        }
        Ok(Step::Continue)
    }
}
