use super::{
    Accept, EntityKind, HAlign, Input, Interactive, MText, PolyVertex, Prompt, Result, Session, Step, Text, VAlign, Value, Vec2, added, bad, bool_or,
    f64_or, lwpoly, number, point_req, str_param, v3,
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

pub(crate) fn make_text(s: &Session, at: Vec2, value: &str, height: f64, rotation: f64, j: (HAlign, VAlign)) -> EntityKind {
    let style = s.doc().map(|d| d.header.str("TEXTSTYLE", "Standard")).unwrap_or_else(|_| "Standard".into());
    let (h, v) = j;
    let left = h == HAlign::Left && v == VAlign::Baseline;
    EntityKind::Text(Text {
        insert: v3(at),
        align_pt: if left { None } else { Some(v3(at)) },
        height,
        value: value.into(),
        rotation,
        width_factor: 1.0,
        oblique: 0.0,
        style,
        halign: h,
        valign: v,
    })
}

pub(super) fn run_text(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_req("text", p, "at")?;
    let text = str_param(p, "text").ok_or_else(|| bad("text", "`text` is required"))?;
    let height = f64_or(p, "height", s.doc()?.header.f64("TEXTSIZE", 0.2));
    if height <= 0.0 {
        return Err(bad("text", "height must be positive"));
    }
    let j = justify(str_param(p, "justify").unwrap_or("L")).ok_or_else(|| bad("text", "unknown justification"))?;
    let k = make_text(s, at, text, height, f64_or(p, "rotation", 0.0).to_radians(), j);
    added(s.add_entity(k)?)
}

pub(super) fn run_mtext(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_req("mtext", p, "at")?;
    let text = str_param(p, "text").ok_or_else(|| bad("mtext", "`text` is required"))?;
    let height = f64_or(p, "height", s.doc()?.header.f64("TEXTSIZE", 0.2));
    if height <= 0.0 {
        return Err(bad("mtext", "height must be positive"));
    }
    let style = match str_param(p, "style") {
        Some(n) => s.doc()?.text_style(n).map(|t| t.name.clone()).ok_or_else(|| bad("mtext", format!("no text style \"{n}\"")))?,
        None => s.doc()?.header.str("TEXTSTYLE", "Standard"),
    };
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

pub(super) struct TextM {
    at: Option<Vec2>,
    height: Option<f64>,
    rotation: Option<f64>,
    justify: (HAlign, VAlign),
    asking_justify: bool,
    line: usize,
    /// Run by a LISP routine (`command`): one line of text ends the command, as AutoLISP's
    /// `command` runs TEXT; typed and in scripts each line asks for the next.
    single_line: bool,
}

impl TextM {
    pub(super) fn new(s: &Session) -> Self {
        TextM {
            at: None,
            height: None,
            rotation: None,
            justify: (HAlign::Left, VAlign::Baseline),
            asking_justify: false,
            line: 0,
            single_line: s.lisp_running(),
        }
    }
}

impl Interactive for TextM {
    fn name(&self) -> &'static str {
        "TEXT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let th = s.doc().map(|d| d.header.f64("TEXTSIZE", 0.2)).unwrap_or(0.2);
        if self.asking_justify {
            return Prompt::new("Enter an option", Accept::TEXT)
                .kw(&["Left", "Center", "Right", "Align", "Middle", "Fit", "TL", "TC", "TR", "ML", "MC", "MR", "BL", "BC", "BR"]);
        }
        match (self.at, self.height, self.rotation) {
            (None, _, _) => Prompt::new("Specify start point of text", Accept::POINT).kw(&["Justify", "Style"]),
            (Some(a), None, _) => Prompt::new("Specify height", Accept::POINT_OR_NUMBER).default(format!("{th:.4}")).base(a),
            (Some(a), Some(_), None) => Prompt::new("Specify rotation angle of text", Accept::POINT_OR_NUMBER).default("0").base(a),
            _ => Prompt::new("Enter text", Accept::TEXT),
        }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let d = s.doc()?;
        let style = d.header.str("TEXTSTYLE", "Standard");
        let anno = if d.text_style(&style).is_some_and(|st| st.annotative) { "Yes" } else { "No" };
        let msg =
            format!("Current text style:  \"{style}\"  Text height:  {:.4}  Annotative:  {anno}  Justify:  Left", d.header.f64("TEXTSIZE", 0.2));
        s.echo(msg);
        Ok(Step::Continue)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.asking_justify {
            let code = match &i {
                Input::Keyword(k) | Input::Text(k) => k.clone(),
                _ => return Ok(Step::Continue),
            };
            self.justify = justify(&code).unwrap_or(self.justify);
            self.asking_justify = false;
            return Ok(Step::Continue);
        }
        let th = s.doc()?.header.f64("TEXTSIZE", 0.2);
        match (self.at, self.height, self.rotation, i) {
            (None, _, _, Input::Keyword(k)) if k == "Justify" => self.asking_justify = true,
            (None, _, _, Input::Point(p)) => self.at = Some(p),
            (Some(_), None, _, Input::Enter) => self.height = Some(th),
            (Some(a), None, _, Input::Point(p)) => self.height = Some(a.dist(p).max(1e-9)),
            (Some(_), None, _, Input::Text(t)) => {
                let h = number(&t).filter(|h| *h > 0.0).ok_or_else(|| crate::EngineError::Other("Requires a positive height.".into()))?;
                self.height = Some(h);
                s.doc_mut()?.header.set_f64("TEXTSIZE", h);
            }
            (Some(_), Some(_), None, Input::Enter) => self.rotation = Some(0.0),
            (Some(a), Some(_), None, Input::Point(p)) => self.rotation = Some(a.angle_to(p)),
            (Some(_), Some(_), None, Input::Text(t)) => {
                self.rotation = Some(s.angle_settings().direction(&t).ok_or_else(|| crate::EngineError::Other("Requires an angle.".into()))?)
            }
            (Some(a), Some(h), Some(r), Input::Text(t)) => {
                let at = a + Vec2::from_angle(r - std::f64::consts::FRAC_PI_2) * (h * 5.0 / 3.0 * self.line as f64);
                let k = make_text(s, at, &t, h, r, self.justify);
                s.add_entity(k)?;
                self.line += 1;
                if self.single_line {
                    return Ok(Step::Done);
                }
            }
            (Some(_), Some(_), Some(_), Input::Enter) => return Ok(Step::Done),
            _ => {}
        }
        Ok(Step::Continue)
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
