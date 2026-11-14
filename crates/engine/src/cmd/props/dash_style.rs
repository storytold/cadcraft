//! -STYLE: a text style at the command line (name, font, height, width factor, obliquing angle,
//! backwards, upside down, vertical). It ends in the same code as the JSON `style` command.

use serde_json::json;

use super::{Result, Session, run_style};
use crate::{Accept, Input, Interactive, Prompt, Step};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    Name,
    Font,
    Height,
    Width,
    Oblique,
    Backwards,
    UpsideDown,
    Vertical,
}

/// The values a style starts from: the existing style's, or the defaults of a new one.
#[derive(Clone, Debug)]
struct Values {
    font: String,
    height: f64,
    width: f64,
    oblique: f64,
    flags: [bool; 3],
}

pub(super) struct DashStyle {
    ask: Ask,
    name: String,
    v: Values,
}

impl Default for DashStyle {
    fn default() -> Self {
        DashStyle { ask: Ask::Name, name: String::new(), v: Values { font: "txt".into(), height: 0.0, width: 1.0, oblique: 0.0, flags: [false; 3] } }
    }
}

fn num(v: f64) -> String {
    format!("{v}")
}

impl DashStyle {
    /// Start from the named style when it exists.
    fn pick(&mut self, s: &Session, name: &str) {
        self.name = name.to_string();
        if let Some(st) = s.doc().ok().and_then(|d| d.text_style(name)) {
            self.name = st.name.clone();
            self.v = Values {
                font: st.font.clone(),
                height: st.height,
                width: st.width_factor,
                oblique: st.oblique.to_degrees(),
                flags: [st.backwards, st.upside_down, st.vertical],
            };
        }
    }

    fn finish(&self, s: &mut Session) -> Result<Step> {
        let p = json!({
            "name": self.name, "font": self.v.font, "height": self.v.height, "widthFactor": self.v.width,
            "oblique": self.v.oblique, "backwards": self.v.flags[0], "upsideDown": self.v.flags[1], "vertical": self.v.flags[2],
        });
        match run_style(s, &p) {
            Ok(_) => {
                s.echo(format!("Text style {} is now current.", self.name));
                Ok(Step::Done)
            }
            Err(e) => {
                // The name was refused (empty or too long): the drawing is untouched.
                s.echo(e.to_string());
                Ok(Step::Done)
            }
        }
    }
}

impl Interactive for DashStyle {
    fn name(&self) -> &'static str {
        "-STYLE"
    }

    fn prompt(&self, s: &Session) -> Prompt {
        match self.ask {
            Ask::Name => {
                let cur = s.doc().map(|d| d.header.str("TEXTSTYLE", "Standard")).unwrap_or_default();
                Prompt::new("Enter name of text style or [?]", Accept::TEXT).kw(&["?"]).default(cur)
            }
            Ask::Font => Prompt::new("Specify full font name or font file name (TTF or SHX)", Accept::TEXT).default(self.v.font.clone()),
            Ask::Height => Prompt::new("Specify height of text", Accept::NUMBER).default(num(self.v.height)),
            Ask::Width => Prompt::new("Specify width factor", Accept::NUMBER).default(num(self.v.width)),
            Ask::Oblique => Prompt::new("Specify obliquing angle", Accept::NUMBER).default(num(self.v.oblique)),
            Ask::Backwards => yes_no("Display text backwards?", self.v.flags[0]),
            Ask::UpsideDown => yes_no("Display text upside-down?", self.v.flags[1]),
            Ask::Vertical => yes_no("Vertical?", self.v.flags[2]),
        }
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let t = match &i {
            Input::Cancel => return Ok(Step::Cancel),
            Input::Text(t) | Input::Keyword(t) => t.trim().to_string(),
            Input::Enter => String::new(),
            _ => return Ok(Step::Continue),
        };
        match self.ask {
            Ask::Name if t == "?" => {
                let lines: Vec<String> =
                    s.doc().map(|d| d.text_styles.iter().map(|st| format!("{:<20} {}", st.name, st.font)).collect()).unwrap_or_default();
                s.echo("Text styles:");
                for l in lines {
                    s.echo(l);
                }
            }
            Ask::Name => {
                let name = if t.is_empty() { s.doc()?.header.str("TEXTSTYLE", "Standard") } else { t };
                self.pick(s, &name);
                self.ask = Ask::Font;
            }
            Ask::Font => {
                if !t.is_empty() {
                    self.v.font = t;
                }
                self.ask = Ask::Height;
            }
            Ask::Height => {
                if let Some(h) = self.number(s, &t, |h| h >= 0.0, "Requires a non-negative number.") {
                    self.v.height = h;
                    self.ask = Ask::Width;
                }
            }
            Ask::Width => {
                if let Some(w) = self.number(s, &t, |w| w > 0.0 && w <= 100.0, "Requires a number above 0 and up to 100.") {
                    self.v.width = w;
                    self.ask = Ask::Oblique;
                }
            }
            Ask::Oblique => {
                if let Some(o) = self.number(s, &t, |o| o.abs() <= 85.0, "Requires an angle from -85 to 85 degrees.") {
                    self.v.oblique = o;
                    self.ask = Ask::Backwards;
                }
            }
            Ask::Backwards | Ask::UpsideDown | Ask::Vertical => {
                let slot = match self.ask {
                    Ask::Backwards => 0,
                    Ask::UpsideDown => 1,
                    _ => 2,
                };
                let lower = t.to_ascii_lowercase();
                let value = match lower.as_str() {
                    "" => None,
                    "y" | "yes" => Some(true),
                    "n" | "no" => Some(false),
                    _ => {
                        s.echo("Enter Yes or No.");
                        return Ok(Step::Continue);
                    }
                };
                if let (Some(v), Some(f)) = (value, self.v.flags.get_mut(slot)) {
                    *f = v;
                }
                match self.ask {
                    Ask::Backwards => self.ask = Ask::UpsideDown,
                    Ask::UpsideDown => self.ask = Ask::Vertical,
                    _ => return self.finish(s),
                }
            }
        }
        Ok(Step::Continue)
    }
}

impl DashStyle {
    /// An empty answer keeps the default (`None` is not returned for it: the caller moves on with
    /// the current value); a bad one is explained and asked again.
    fn number(&self, s: &mut Session, t: &str, ok: impl Fn(f64) -> bool, msg: &str) -> Option<f64> {
        if t.is_empty() {
            return Some(match self.ask {
                Ask::Height => self.v.height,
                Ask::Width => self.v.width,
                _ => self.v.oblique,
            });
        }
        match t.parse::<f64>().ok().filter(|v| v.is_finite() && ok(*v)) {
            Some(v) => Some(v),
            None => {
                s.echo(msg.to_string());
                None
            }
        }
    }
}

fn yes_no(msg: &str, cur: bool) -> Prompt {
    Prompt::new(msg, Accept::TEXT).kw(&["Yes", "No"]).default(if cur { "Yes" } else { "No" })
}
