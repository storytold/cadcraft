use super::{
    Accept, EntityKind, HAlign, Input, Interactive, MText, PolyVertex, Prompt, Result, Session, Step, Text, VAlign, Value, Vec2, added, bad, bool_or,
    f64_or, lwpoly, number, point_param, point_req, str_param, v3,
};

/// Justification code → (halign, valign).
pub(crate) fn justify(j: &str) -> Option<(HAlign, VAlign)> {
    Some(match j.to_ascii_uppercase().as_str() {
        "L" | "LEFT" => (HAlign::Left, VAlign::Baseline),
        "C" | "CENTER" => (HAlign::Center, VAlign::Baseline),
        "R" | "RIGHT" => (HAlign::Right, VAlign::Baseline),
        "A" | "ALIGN" => (HAlign::Aligned, VAlign::Baseline),
        "M" | "MIDDLE" => (HAlign::Middle, VAlign::Baseline),
        "F" | "FIT" => (HAlign::Fit, VAlign::Baseline),
        "TL" => (HAlign::Left, VAlign::Top),
        "TC" => (HAlign::Center, VAlign::Top),
        "TR" => (HAlign::Right, VAlign::Top),
        "ML" => (HAlign::Left, VAlign::Middle),
        "MC" => (HAlign::Center, VAlign::Middle),
        "MR" => (HAlign::Right, VAlign::Middle),
        "BL" => (HAlign::Left, VAlign::Bottom),
        "BC" => (HAlign::Center, VAlign::Bottom),
        "BR" => (HAlign::Right, VAlign::Bottom),
        _ => return None,
    })
}

/// Justification → the name TEXT's status line gives it.
fn justify_name(j: (HAlign, VAlign)) -> &'static str {
    match j {
        (HAlign::Left, VAlign::Baseline) => "Left",
        (HAlign::Center, VAlign::Baseline) => "Center",
        (HAlign::Right, VAlign::Baseline) => "Right",
        (HAlign::Aligned, _) => "Align",
        (HAlign::Middle, _) => "Middle",
        (HAlign::Fit, _) => "Fit",
        (HAlign::Left, VAlign::Top) => "TL",
        (HAlign::Center, VAlign::Top) => "TC",
        (HAlign::Right, VAlign::Top) => "TR",
        (HAlign::Left, VAlign::Middle) => "ML",
        (HAlign::Center, VAlign::Middle) => "MC",
        (HAlign::Right, VAlign::Middle) => "MR",
        (HAlign::Left, VAlign::Bottom) => "BL",
        (HAlign::Center, VAlign::Bottom) => "BC",
        (HAlign::Right, VAlign::Bottom) => "BR",
    }
}

/// The height text style `style` fixes for its text (`None`: asked when text is placed).
pub(crate) fn fixed_height(d: &cadcraft_doc::Drawing, style: &str) -> Option<f64> {
    d.text_style(style).map(|t| t.height).filter(|h| h.is_finite() && *h > 0.0)
}

/// TEXT in `style`, which gives it its width factor, obliquing angle and the Backwards and
/// Upside down effects, as AutoCAD's new text takes them from its style (later style changes
/// don't change them).
pub(crate) fn make_text(s: &Session, style: &str, at: Vec2, value: &str, height: f64, rotation: f64, j: (HAlign, VAlign)) -> EntityKind {
    EntityKind::Text(text_in(s, style, at, value, height, rotation, j))
}

/// The text of [`make_text`] (attribute definitions take theirs the same way).
pub(crate) fn text_in(s: &Session, style: &str, at: Vec2, value: &str, height: f64, rotation: f64, j: (HAlign, VAlign)) -> Text {
    let st = s.doc().ok().and_then(|d| d.text_style(style));
    let (width_factor, oblique) =
        st.map(|t| (t.width_factor, t.oblique)).filter(|(w, o)| w.is_finite() && *w > 0.0 && o.is_finite()).unwrap_or((1.0, 0.0));
    let (backwards, upside_down) = st.map_or((false, false), |t| (t.backwards, t.upside_down));
    let (h, v) = j;
    let left = h == HAlign::Left && v == VAlign::Baseline;
    Text {
        insert: v3(at),
        align_pt: if left { None } else { Some(v3(at)) },
        height,
        value: value.into(),
        rotation,
        width_factor,
        oblique,
        style: style.to_string(),
        halign: h,
        valign: v,
        backwards,
        upside_down,
    }
}

/// Aligned (`aligned`) or fit TEXT along the baseline from `start` to `end`: aligned text gets
/// the height that makes it span the baseline at the style's width factor, fit text keeps
/// `height` and gets the width factor that makes it span it (AutoCAD stores both the same way).
pub(crate) fn make_fitted_text(s: &Session, style: &str, start: Vec2, end: Vec2, value: &str, height: f64, aligned: bool) -> EntityKind {
    let mut k = make_text(s, style, start, value, height, start.angle_to(end), (HAlign::Left, VAlign::Baseline));
    if let EntityKind::Text(t) = &mut k {
        // Measured as the renderer measures it, so the text spans the baseline as drawn.
        let font = s.doc().ok().and_then(|d| d.text_style(style)).map(|st| cadcraft_fonts::TextFont::resolve(&st.font)).unwrap_or_default();
        let width = cadcraft_fonts::text_width(&font, value, t.height, t.width_factor);
        let span = start.dist(end);
        if width.is_finite() && width > 1e-12 && span.is_finite() && span > 1e-12 {
            let f = span / width;
            if aligned {
                t.height *= f;
            } else {
                t.width_factor *= f;
            }
        }
        t.align_pt = Some(v3(end));
        t.halign = if aligned { HAlign::Aligned } else { HAlign::Fit };
    }
    k
}

/// A `style` parameter: the drawing's style of that name, else the current style.
fn style_param(s: &Session, cmd: &str, p: &Value) -> Result<String> {
    let d = s.doc()?;
    match str_param(p, "style") {
        Some(n) => d.text_style(n).map(|t| t.name.clone()).ok_or_else(|| bad(cmd, format!("no text style \"{n}\""))),
        None => Ok(d.header.str("TEXTSTYLE", "Standard")),
    }
}

pub(super) fn run_text(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_req("text", p, "at")?;
    let text = str_param(p, "text").ok_or_else(|| bad("text", "`text` is required"))?;
    let style = style_param(s, "text", p)?;
    let d = s.doc()?;
    let height = f64_or(p, "height", fixed_height(d, &style).unwrap_or_else(|| d.header.f64("TEXTSIZE", 0.2)));
    if !(height > 0.0 && height.is_finite()) {
        return Err(bad("text", "height must be positive"));
    }
    let j = justify(str_param(p, "justify").unwrap_or("L")).ok_or_else(|| bad("text", "unknown justification"))?;
    let rotation = f64_or(p, "rotation", 0.0).to_radians();
    let k = if matches!(j.0, HAlign::Aligned | HAlign::Fit) {
        // Align and Fit span the baseline from `at` to `end`; without `end`, the baseline runs
        // along `rotation` as long as the text is at `height`.
        let end = match point_param(p, "end") {
            Some(end) => end,
            None => {
                let EntityKind::Text(t) = make_text(s, &style, at, text, height, rotation, (HAlign::Left, VAlign::Baseline)) else {
                    return Err(bad("text", "`end` ([x, y]) is required"));
                };
                let font = s.doc()?.text_style(&style).map(|st| cadcraft_fonts::TextFont::resolve(&st.font)).unwrap_or_default();
                let w = cadcraft_fonts::text_width(&font, text, t.height, t.width_factor);
                at + Vec2::from_angle(t.rotation) * if w.is_finite() && w > 1e-9 { w } else { height }
            }
        };
        if at.dist(end) < 1e-9 || !end.is_finite() {
            return Err(bad("text", "`end` must differ from `at`"));
        }
        make_fitted_text(s, &style, at, end, text, height, j.0 == HAlign::Aligned)
    } else {
        make_text(s, &style, at, text, height, rotation, j)
    };
    added(s.add_entity(k)?)
}

pub(super) fn run_mtext(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_req("mtext", p, "at")?;
    let text = str_param(p, "text").ok_or_else(|| bad("mtext", "`text` is required"))?;
    let style = style_param(s, "mtext", p)?;
    let d = s.doc()?;
    let height = f64_or(p, "height", fixed_height(d, &style).unwrap_or_else(|| d.header.f64("TEXTSIZE", 0.2)));
    if !(height > 0.0 && height.is_finite()) {
        return Err(bad("mtext", "height must be positive"));
    }
    let attach = match str_param(p, "justify") {
        Some(j) => crate::cmd::mtext_opts::attach_code(j).ok_or_else(|| bad("mtext", "`justify` must be TL, TC, TR, ML, MC, MR, BL, BC or BR"))?,
        None => p.get("attach").and_then(Value::as_u64).unwrap_or(1).clamp(1, 9) as u8,
    };
    let width = f64_or(p, "width", 0.0).max(0.0);
    let k = EntityKind::MText(MText {
        insert: v3(at),
        height,
        width,
        attach,
        rotation: f64_or(p, "rotation", 0.0).to_radians(),
        style,
        contents: crate::cmd::mtext_opts::json_paragraphs(p, text.replace('\n', "\\P"), height)?,
        line_spacing: f64_or(p, "lineSpacing", 1.0),
        line_spacing_exact: bool_or(p, "lineSpacingExact", false),
        columns: crate::cmd::mtext_opts::json_columns(p, height, width)?,
    });
    added(s.add_entity(k)?)
}

/// A question TEXT asks besides its main prompts.
#[derive(Clone, Copy, PartialEq)]
enum TextAsk {
    Justify,
    Style,
    StyleList,
}

pub(super) struct TextM {
    at: Option<Vec2>,
    /// The other end of the baseline, for aligned and fit text.
    end: Option<Vec2>,
    height: Option<f64>,
    rotation: Option<f64>,
    justify: (HAlign, VAlign),
    ask: Option<TextAsk>,
    /// How far below the first line the next line goes.
    drop: f64,
    /// The style of the text continued below (Enter at the start point); else the current one.
    style: Option<String>,
    /// Run by a LISP routine (`command`): one line of text ends the command, as AutoLISP's
    /// `command` runs TEXT; typed and in scripts each line asks for the next.
    single_line: bool,
}

impl TextM {
    pub(super) fn new(s: &Session) -> Self {
        TextM {
            at: None,
            end: None,
            height: None,
            rotation: None,
            justify: (HAlign::Left, VAlign::Baseline),
            ask: None,
            drop: 0.0,
            style: None,
            single_line: s.lisp_running(),
        }
    }

    /// Aligned and fit text span the two ends of a baseline.
    fn two_point(&self) -> bool {
        matches!(self.justify.0, HAlign::Aligned | HAlign::Fit)
    }

    /// The style new lines are set in.
    fn style(&self, s: &Session) -> String {
        self.style.clone().unwrap_or_else(|| s.doc().map(|d| d.header.str("TEXTSTYLE", "Standard")).unwrap_or_else(|_| "Standard".into()))
    }

    /// The status line: the style, the height it fixes (else TEXTSIZE), annotative, justify.
    fn status(&self, s: &mut Session) -> Result<()> {
        let style = self.style(s);
        let d = s.doc()?;
        let anno = if d.text_style(&style).is_some_and(|st| st.annotative) { "Yes" } else { "No" };
        let height = fixed_height(d, &style).unwrap_or_else(|| d.header.f64("TEXTSIZE", 0.2));
        s.echo(format!("Current text style:  \"{style}\"  Text height:  {height:.4}  Annotative:  {anno}  Justify:  {}", justify_name(self.justify)));
        Ok(())
    }

    /// The text line `value` makes, where the next line goes (`None` until its points, height
    /// and rotation are known).
    fn next_line(&self, s: &Session, value: &str) -> Option<EntityKind> {
        let (a, h, r) = (self.at?, self.height?, self.rotation?);
        let style = self.style(s);
        let off = Vec2::from_angle(r - std::f64::consts::FRAC_PI_2) * self.drop;
        if self.two_point() {
            let e = self.end?;
            Some(make_fitted_text(s, &style, a + off, e + off, value, h, self.justify.0 == HAlign::Aligned))
        } else {
            Some(make_text(s, &style, a + off, value, h, r, self.justify))
        }
    }

    /// Continue directly below the last TEXT in this space, with its style, height, rotation and
    /// justification, as Enter at AutoCAD's start point does. False when there is none.
    fn continue_below(&mut self, s: &Session) -> bool {
        let Ok(d) = s.doc() else { return false };
        let last = d
            .space(&s.space())
            .and_then(|st| st.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.clone()) } else { None }).last());
        let Some(t) = last else { return false };
        let h = if t.height.is_finite() && t.height > 0.0 { t.height } else { d.header.f64("TEXTSIZE", 0.2) };
        let r = if t.rotation.is_finite() { t.rotation } else { 0.0 };
        let below = Vec2::from_angle(r - std::f64::consts::FRAC_PI_2) * (h * 5.0 / 3.0);
        match (t.halign, t.align_pt) {
            // Aligned and fit text continue along a baseline as long, one line below.
            (HAlign::Aligned | HAlign::Fit, Some(e)) => {
                self.at = Some(t.insert.xy() + below);
                self.end = Some(e.xy() + below);
                self.justify = (t.halign, VAlign::Baseline);
            }
            (HAlign::Aligned | HAlign::Fit, None) => {
                self.at = Some(t.insert.xy() + below);
                self.justify = (HAlign::Left, VAlign::Baseline);
            }
            _ => {
                self.at = Some(t.justify_point() + below);
                self.justify = (t.halign, t.valign);
            }
        }
        self.height = Some(h);
        self.rotation = Some(r);
        self.style = d.text_style(&t.style).map(|st| st.name.clone());
        true
    }
}

impl Interactive for TextM {
    fn name(&self) -> &'static str {
        "TEXT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let th = s.doc().map(|d| d.header.f64("TEXTSIZE", 0.2)).unwrap_or(0.2);
        match self.ask {
            Some(TextAsk::Justify) => {
                return Prompt::new("Enter an option", Accept::TEXT)
                    .kw(&["Left", "Center", "Right", "Align", "Middle", "Fit", "TL", "TC", "TR", "ML", "MC", "MR", "BL", "BC", "BR"]);
            }
            Some(TextAsk::Style) => return Prompt::new("Enter style name", Accept::TEXT).kw(&["?"]).default(self.style(s)),
            Some(TextAsk::StyleList) => return Prompt::new("Enter text style(s) to list", Accept::TEXT).default("*"),
            None => {}
        }
        let two = self.two_point();
        match (self.at, self.end, self.height, self.rotation) {
            (None, ..) if two => Prompt::new("Specify first endpoint of text baseline", Accept::POINT),
            (None, ..) => Prompt::new("Specify start point of text", Accept::POINT).kw(&["Justify", "Style"]),
            (Some(a), None, ..) if two => Prompt::new("Specify second endpoint of text baseline", Accept::POINT).base(a),
            (Some(a), _, None, _) => {
                // An annotative style's text is sized as it measures on paper.
                let annotative = s.doc().is_ok_and(|d| d.text_style(&self.style(s)).is_some_and(|st| st.annotative));
                let ask = if annotative { "Specify paper text height" } else { "Specify height" };
                Prompt::new(ask, Accept::POINT_OR_NUMBER).default(format!("{th:.4}")).base(a)
            }
            (Some(a), _, Some(_), None) => Prompt::new("Specify rotation angle of text", Accept::POINT_OR_NUMBER).default("0").base(a),
            _ => Prompt::new("Enter text", Accept::TEXT),
        }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.status(s)?;
        Ok(Step::Continue)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let word = match &i {
            Input::Keyword(k) | Input::Text(k) => Some(k.trim().to_string()),
            _ => None,
        };
        match self.ask {
            Some(TextAsk::Justify) => {
                // Enter leaves the question with the justification as it was (as Style's does).
                let Some(code) = word.filter(|w| !w.is_empty()) else {
                    self.ask = None;
                    return Ok(Step::Continue);
                };
                // As MTEXT's: an answer that is no option is refused, and asked again.
                self.justify = justify(&code).ok_or_else(|| crate::EngineError::Other("Invalid option keyword.".into()))?;
                self.ask = None;
                return Ok(Step::Continue);
            }
            Some(TextAsk::Style) => {
                match word.as_deref() {
                    None | Some("") => self.ask = None,
                    Some("?") => self.ask = Some(TextAsk::StyleList),
                    Some(name) => {
                        let found = s.doc()?.text_style(name).map(|t| t.name.clone());
                        let name = found.ok_or_else(|| crate::EngineError::Other(format!("Cannot find text style \"{name}\".")))?;
                        // Like AutoCAD's Style option, the style becomes current.
                        s.doc_mut()?.header.set_str("TEXTSTYLE", &name);
                        self.style = None;
                        self.ask = None;
                        self.status(s)?;
                    }
                }
                return Ok(Step::Continue);
            }
            Some(TextAsk::StyleList) => {
                let pattern = word.filter(|w| !w.is_empty()).unwrap_or_else(|| "*".into());
                let d = s.doc()?;
                let lines: Vec<String> =
                    crate::cmd::textstyle_cli::styles_matching(d, &pattern).into_iter().map(crate::cmd::textstyle_cli::listing).collect();
                for l in lines {
                    s.echo(l);
                }
                self.ask = Some(TextAsk::Style);
                return Ok(Step::Continue);
            }
            None => {}
        }
        let th = s.doc()?.header.f64("TEXTSIZE", 0.2);
        let two = self.two_point();
        match (self.at, self.height, self.rotation, i) {
            (None, _, _, Input::Keyword(k)) if k == "Justify" => self.ask = Some(TextAsk::Justify),
            (None, _, _, Input::Keyword(k)) if k == "Style" => self.ask = Some(TextAsk::Style),
            (None, _, _, Input::Point(p)) => {
                self.at = Some(p);
                // A fixed-height style isn't asked for a height (aligned text works its height out).
                if !two {
                    self.height = fixed_height(s.doc()?, &self.style(s));
                }
            }
            (None, _, _, Input::Enter) => {
                if !self.continue_below(s) {
                    return Ok(Step::Done);
                }
            }
            // The second end of an aligned or fit baseline gives the rotation; aligned text's
            // height comes from the text, fit text keeps a fixed style's height or asks.
            (Some(a), _, _, Input::Point(p)) if two && self.end.is_none() => {
                if a.dist(p) < 1e-9 {
                    return Err(crate::EngineError::Other("The two ends of the baseline must differ.".into()));
                }
                self.end = Some(p);
                self.rotation = Some(a.angle_to(p));
                let fixed = fixed_height(s.doc()?, &self.style(s));
                self.height = if self.justify.0 == HAlign::Aligned { Some(fixed.unwrap_or(th)) } else { fixed };
            }
            (Some(_), _, _, _) if two && self.end.is_none() => {}
            (Some(_), None, _, Input::Enter) => self.height = Some(th),
            (Some(a), None, _, Input::Point(p)) => self.height = Some(a.dist(p).max(1e-9)),
            (Some(_), None, _, Input::Text(t)) => {
                let h = number(&t)
                    .filter(|h| *h > 0.0 && h.is_finite())
                    .ok_or_else(|| crate::EngineError::Other("Requires a positive height.".into()))?;
                self.height = Some(h);
                s.doc_mut()?.header.set_f64("TEXTSIZE", h);
            }
            (Some(_), Some(_), None, Input::Enter) => self.rotation = Some(0.0),
            (Some(a), Some(_), None, Input::Point(p)) => self.rotation = Some(a.angle_to(p)),
            (Some(_), Some(_), None, Input::Text(t)) => {
                self.rotation = Some(s.angle_settings().direction(&t).ok_or_else(|| crate::EngineError::Other("Requires an angle.".into()))?)
            }
            // An empty line ends the command; no empty text is made.
            (Some(_), Some(_), Some(_), Input::Text(t)) if t.is_empty() => return Ok(Step::Done),
            (Some(_), Some(h), Some(_), Input::Text(t)) => {
                let Some(k) = self.next_line(s, &t) else { return Ok(Step::Continue) };
                let line_height = if let EntityKind::Text(x) = &k { x.height } else { h };
                s.add_entity(k)?;
                self.drop += line_height * 5.0 / 3.0;
                if self.single_line {
                    return Ok(Step::Done);
                }
            }
            (Some(_), Some(_), Some(_), Input::Enter) => return Ok(Step::Done),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn typing(&self, s: &Session, typed: &str) -> Vec<EntityKind> {
        if self.ask.is_some() || typed.is_empty() {
            return Vec::new();
        }
        self.next_line(s, typed).into_iter().collect()
    }
}

#[derive(Default)]
pub(super) struct MTextM {
    first: Option<Vec2>,
    second: Option<Vec2>,
    /// Paragraphs typed so far: one per input line, an empty line ends the text (as on
    /// AutoCAD's command line and in scripts).
    lines: Vec<String>,
    /// Height, justification, line spacing, rotation, style and width set by the options.
    opts: crate::cmd::mtext_opts::MTextOpts,
    /// An option asking a further question.
    ask: Option<crate::cmd::mtext_opts::MTextAsk>,
}

impl MTextM {
    /// The box is known: the opposite corner was picked or the Width option answered.
    fn boxed(&self) -> bool {
        self.second.is_some() || self.opts.width.is_some()
    }
}

impl Interactive for MTextM {
    fn name(&self) -> &'static str {
        "MTEXT"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.opts = crate::cmd::mtext_opts::MTextOpts::new(s);
        let anno = s.doc()?.text_style(&self.opts.style).is_some_and(|st| st.annotative);
        s.echo(format!(
            "Current text style:  \"{}\"  Text height:  {:.4}  Annotative:  {}",
            self.opts.style,
            self.opts.height,
            if anno { "Yes" } else { "No" }
        ));
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let (Some(a), Some(q)) = (self.first, self.ask) {
            return q.prompt(&self.opts, a);
        }
        match self.first {
            None => Prompt::new("Specify first corner", Accept::POINT),
            Some(a) if !self.boxed() => Prompt::new("Specify opposite corner", Accept::POINT)
                .kw(&["Height", "Justify", "Line spacing", "Rotation", "Style", "Width", "Columns"])
                .base(a),
            _ if self.lines.is_empty() => Prompt::new("Enter text (use \\P for new paragraphs)", Accept::TEXT),
            _ => Prompt::new("Enter next line of text (empty line to finish)", Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let (Some(a), Some(q)) = (self.first, self.ask) {
            use crate::cmd::mtext_opts::Outcome;
            self.ask = match q.input(s, i, &mut self.opts, a)? {
                Outcome::Ask(next) => Some(next),
                Outcome::Back | Outcome::Text => None,
            };
            return Ok(Step::Continue);
        }
        match (self.first, self.boxed(), i) {
            (None, _, Input::Point(p)) => self.first = Some(p),
            (Some(_), false, Input::Point(p)) => self.second = Some(p),
            (Some(_), false, Input::Keyword(k)) => {
                self.ask = Some(
                    crate::cmd::mtext_opts::MTextAsk::from_keyword(&k).ok_or_else(|| crate::EngineError::Other("Invalid option keyword.".into()))?,
                );
            }
            (Some(_), true, Input::Text(t)) if !t.is_empty() => self.lines.push(t),
            (Some(a), true, Input::Text(_) | Input::Enter) => {
                if self.lines.is_empty() {
                    return Ok(Step::Done);
                }
                s.add_entity(EntityKind::MText(self.opts.mtext(a, self.second, self.lines.join("\\P"))))?;
                return Ok(Step::Done);
            }
            (_, _, Input::Enter) => return Ok(Step::Done),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn editing_mtext(&self) -> Option<MText> {
        let a = self.first?;
        (self.boxed() && self.ask.is_none()).then(|| self.opts.mtext(a, self.second, String::new()))
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        match (self.first, self.boxed(), self.ask) {
            (Some(a), false, None) => {
                // The box, turned by the rotation about the first corner.
                let r = self.opts.rotation;
                let l = (c - a).rotate(-r);
                let corners = [Vec2::ZERO, Vec2::new(l.x, 0.0), l, Vec2::new(0.0, l.y)];
                vec![lwpoly(corners.iter().map(|q| PolyVertex::new(a + q.rotate(r))).collect(), true)]
            }
            _ => Vec::new(),
        }
    }
}
