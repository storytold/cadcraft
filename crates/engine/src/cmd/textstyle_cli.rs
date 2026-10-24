//! -STYLE, text styles at the command line (scripts, the keyboard): AutoCAD's prompts for a
//! style's name, font, height (or annotative paper height), width factor, obliquing angle,
//! backwards and upside-down text (and vertical text for SHX fonts), which then becomes the
//! current style. Also `style.fonts`, the installed fonts a style can name, by family and style
//! (the Text Style dialog's font lists).

use cadcraft_doc::{Drawing, TextStyle};
use serde_json::{Value, json};

use super::*;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("-style", "Text Style", super::props::run_style)
            .params("same as `style`: {name, font?, bigFont?, height?, widthFactor?, oblique? (degrees), backwards?, upsideDown?, vertical?, annotative?, current?} → styles")
            .interactive(|_| Ok(Box::new(DashStyle::default()))),
        CommandSpec::new("style.fonts", "List Fonts", run_fonts)
            .params("{} → {fonts: [{family, style, file}]} (the built-in stroke font first, then the installed fonts; a text style's `font` takes `file`)")
            .noundo(),
    ]
}

fn run_fonts(_s: &mut Session, _p: &Value) -> Result<Value> {
    let builtin = cadcraft_fonts::BUILTIN_FONT;
    let mut fonts = vec![json!({ "family": builtin, "style": "Regular", "file": builtin })];
    fonts.extend(cadcraft_fonts::ttf::installed_fonts().into_iter().map(|f| json!({ "family": f.family, "style": f.style, "file": f.file })));
    Ok(json!({ "fonts": fonts }))
}

fn other(m: impl Into<String>) -> EngineError {
    EngineError::Other(m.into())
}

/// Whether `font` names an SHX font (the only kind with a vertical form).
fn is_shx(font: &str) -> bool {
    font.trim().to_ascii_lowercase().ends_with(".shx")
}

fn yes(text: Option<&str>, default: bool) -> bool {
    match text.map(str::to_ascii_lowercase).as_deref() {
        Some("yes" | "y") => true,
        Some("no" | "n") => false,
        _ => default,
    }
}

/// A style's settings in one line, as `?` lists them.
pub(super) fn listing(st: &TextStyle) -> String {
    let mut generation: Vec<&str> = Vec::new();
    if st.backwards {
        generation.push("Backwards");
    }
    if st.upside_down {
        generation.push("Upside-down");
    }
    if st.vertical {
        generation.push("Vertical");
    }
    let generation = if generation.is_empty() { "Normal".to_string() } else { generation.join(", ") };
    let annotative = if st.annotative { "  Annotative" } else { "" };
    format!(
        "Style name: \"{}\"  Font files: {}  Height: {:.4}  Width factor: {:.4}  Obliquing angle: {:.0}  Generation: {generation}{annotative}",
        st.name,
        st.font,
        st.height,
        st.width_factor,
        st.oblique.to_degrees()
    )
}

/// Text style names matching `pattern` (`*` and `?` wildcards, any case).
pub(super) fn styles_matching<'a>(d: &'a Drawing, pattern: &str) -> Vec<&'a TextStyle> {
    let p: Vec<char> = pattern.trim().to_lowercase().chars().take(255).collect();
    let p = if p.is_empty() { vec!['*'] } else { p };
    d.text_styles.iter().filter(|st| wildcard(&p, &st.name.to_lowercase().chars().collect::<Vec<_>>())).collect()
}

/// `*` and `?` matching without recursion (bounded by the two lengths).
fn wildcard(p: &[char], s: &[char]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        match p.get(pi) {
            Some('*') => {
                star = Some((pi, si));
                pi += 1;
            }
            Some(c) if *c == '?' || Some(c) == s.get(si) => {
                pi += 1;
                si += 1;
            }
            _ => match star {
                Some((sp, ss)) => {
                    pi = sp + 1;
                    si = ss + 1;
                    star = Some((sp, ss + 1));
                }
                None => return false,
            },
        }
    }
    p.get(pi..).is_some_and(|rest| rest.iter().all(|c| *c == '*'))
}

#[derive(Default)]
enum Ask {
    #[default]
    Name,
    ListPattern,
    Font,
    Height,
    Annotative,
    Orientation,
    PaperHeight,
    Width,
    Oblique,
    Backwards,
    UpsideDown,
    Vertical,
}

/// The style being defined: its settings so far, starting from the existing style (or, for a
/// new one, the current style).
#[derive(Default)]
struct DashStyle {
    ask: Ask,
    style: TextStyle,
}

impl DashStyle {
    fn finish(&self, s: &mut Session) -> Result<Step> {
        let st = &self.style;
        super::props::run_style(
            s,
            &json!({
                "name": st.name,
                "font": st.font,
                // A new style keeps the big font of the style it starts from.
                "bigFont": st.big_font,
                "height": st.height,
                "widthFactor": st.width_factor,
                "oblique": st.oblique.to_degrees(),
                "backwards": st.backwards,
                "upsideDown": st.upside_down,
                "vertical": st.vertical,
                "annotative": st.annotative,
                "current": true,
            }),
        )?;
        s.echo(format!("\"{}\" is now the current text style.", st.name));
        Ok(Step::Done)
    }
}

fn number(t: &str) -> Option<f64> {
    t.trim().parse::<f64>().ok().or_else(|| crate::units::parse_distance(t)).filter(|v| v.is_finite())
}

impl Interactive for DashStyle {
    fn name(&self) -> &'static str {
        "-STYLE"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let d = s.doc()?;
        let current = d.header.str("TEXTSTYLE", "Standard");
        self.style = d.text_style(&current).cloned().unwrap_or_default();
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let st = &self.style;
        let (lu, lp) = s.doc().map(|d| (d.header.i64("LUNITS", 2), d.header.i64("LUPREC", 4))).unwrap_or((2, 4));
        let dist = |v: f64| crate::units::format_distance(v, lu, lp);
        let yes_no = |b: bool| if b { "Yes" } else { "No" };
        match self.ask {
            Ask::Name => Prompt::new("Enter name of text style", Accept::TEXT).kw(&["?"]).default(st.name.clone()),
            Ask::ListPattern => Prompt::new("Enter text style(s) to list", Accept::TEXT).default("*"),
            Ask::Font => Prompt::new("Specify full font name or font filename (TTF or SHX)", Accept::TEXT).default(st.font.clone()),
            Ask::Height => Prompt::new("Specify height of text", Accept::TEXT).kw(&["Annotative"]).default(dist(st.height)),
            // Asked after Annotative was picked: Enter says yes.
            Ask::Annotative => Prompt::new("Create annotative text style?", Accept::TEXT).kw(&["Yes", "No"]).default("Yes"),
            Ask::Orientation => Prompt::new("Match text orientation to layout?", Accept::TEXT).kw(&["Yes", "No"]).default("No"),
            Ask::PaperHeight => Prompt::new("Specify paper text height", Accept::TEXT).default(dist(st.height)),
            Ask::Width => Prompt::new("Specify width factor", Accept::TEXT).default(format!("{:.4}", st.width_factor)),
            Ask::Oblique => Prompt::new("Specify obliquing angle", Accept::TEXT).default(format!("{:.0}", st.oblique.to_degrees())),
            Ask::Backwards => Prompt::new("Display text backwards?", Accept::TEXT).kw(&["Yes", "No"]).default(yes_no(st.backwards)),
            Ask::UpsideDown => Prompt::new("Display text upside-down?", Accept::TEXT).kw(&["Yes", "No"]).default(yes_no(st.upside_down)),
            Ask::Vertical => Prompt::new("Vertical?", Accept::TEXT).kw(&["Yes", "No"]).default(yes_no(st.vertical)),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let ask = std::mem::take(&mut self.ask);
        let text = match &i {
            Input::Text(t) | Input::Keyword(t) => Some(t.trim().to_string()).filter(|t| !t.is_empty()),
            _ => None,
        };
        let t = text.as_deref();
        match ask {
            Ask::Name => match t {
                Some("?") => self.ask = Ask::ListPattern,
                _ => {
                    let name = t.map_or_else(|| self.style.name.clone(), str::to_string);
                    if name.len() > 255 || name.contains(['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=', '`']) {
                        self.ask = Ask::Name;
                        return Err(other("Invalid text style name."));
                    }
                    let d = s.doc()?;
                    match d.text_style(&name) {
                        Some(existing) => {
                            self.style = existing.clone();
                            s.echo("Existing style.".to_string());
                        }
                        None => {
                            // A new style starts with the current style's settings.
                            self.style.name = name;
                            s.echo("New style.".to_string());
                        }
                    }
                    self.ask = Ask::Font;
                }
            },
            Ask::ListPattern => {
                let d = s.doc()?;
                let lines: Vec<String> = styles_matching(d, t.unwrap_or("*")).into_iter().map(listing).collect();
                let current = d.header.str("TEXTSTYLE", "Standard");
                for l in lines {
                    s.echo(l);
                }
                s.echo(format!("Current text style: {current}"));
                self.ask = Ask::Name;
            }
            Ask::Font => {
                if let Some(f) = t {
                    let f: String = f.trim_matches('"').chars().take(255).collect();
                    if cadcraft_fonts::ttf::find(&f).is_none() && !is_shx(&f) && !f.eq_ignore_ascii_case(cadcraft_fonts::BUILTIN_FONT) {
                        s.echo(format!("Font {f} is not installed; text is drawn with a substitute."));
                    }
                    self.style.font = f;
                }
                self.ask = Ask::Height;
            }
            Ask::Height => match t {
                Some(k) if k.eq_ignore_ascii_case("annotative") || k.eq_ignore_ascii_case("a") => self.ask = Ask::Annotative,
                Some(v) => match number(v).filter(|h| *h >= 0.0) {
                    Some(h) => {
                        self.style.height = h;
                        self.ask = Ask::Width;
                    }
                    None => {
                        self.ask = Ask::Height;
                        return Err(other("Requires a distance, or Annotative."));
                    }
                },
                None => self.ask = Ask::Width,
            },
            Ask::Annotative => {
                self.style.annotative = yes(t, true);
                // No asks for the height again, as it measures in the drawing.
                self.ask = if self.style.annotative { Ask::Orientation } else { Ask::Height };
            }
            // CADCraft doesn't model the layout orientation; the answer is taken and not kept.
            Ask::Orientation => self.ask = Ask::PaperHeight,
            Ask::PaperHeight => match t.map(number) {
                Some(Some(h)) if h >= 0.0 => {
                    self.style.height = h;
                    self.ask = Ask::Width;
                }
                Some(_) => {
                    self.ask = Ask::PaperHeight;
                    return Err(other("Requires a positive distance or 0."));
                }
                None => self.ask = Ask::Width,
            },
            Ask::Width => match t.map(number) {
                Some(Some(w)) if w > 0.0 && w <= 100.0 => {
                    self.style.width_factor = w;
                    self.ask = Ask::Oblique;
                }
                Some(_) => {
                    self.ask = Ask::Width;
                    return Err(other("Requires a width factor greater than 0 and at most 100."));
                }
                None => self.ask = Ask::Oblique,
            },
            Ask::Oblique => {
                let angle = t.map(|v| {
                    v.trim().parse::<f64>().ok().or_else(|| {
                        crate::units::parse_angle(v).map(|a| {
                            let d = a.to_degrees().rem_euclid(360.0);
                            if d > 180.0 { d - 360.0 } else { d }
                        })
                    })
                });
                match angle {
                    Some(Some(a)) if a.abs() <= 85.0 => {
                        self.style.oblique = a.to_radians();
                        self.ask = Ask::Backwards;
                    }
                    Some(_) => {
                        self.ask = Ask::Oblique;
                        return Err(other("Requires an angle between -85 and 85."));
                    }
                    None => self.ask = Ask::Backwards,
                }
            }
            Ask::Backwards => {
                self.style.backwards = yes(t, self.style.backwards);
                self.ask = Ask::UpsideDown;
            }
            Ask::UpsideDown => {
                self.style.upside_down = yes(t, self.style.upside_down);
                if !is_shx(&self.style.font) {
                    self.style.vertical = false;
                    return self.finish(s);
                }
                self.ask = Ask::Vertical;
            }
            Ask::Vertical => {
                self.style.vertical = yes(t, self.style.vertical);
                return self.finish(s);
            }
        }
        Ok(Step::Continue)
    }
}

#[cfg(test)]
mod tests {
    use super::wildcard;

    #[test]
    fn wildcards() {
        let m = |p: &str, s: &str| wildcard(&p.chars().collect::<Vec<_>>(), &s.chars().collect::<Vec<_>>());
        assert!(m("*", "anything") && m("st*", "standard") && m("s?andard", "standard") && m("*d", "standard"));
        assert!(!m("st", "standard") && !m("x*", "standard"));
        assert!(m(&"*".repeat(10_000), "abc"));
    }
}
