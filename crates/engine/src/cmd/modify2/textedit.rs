use super::{
    Accept, EntityKind, Handle, Input, Interactive, Prompt, Result, Session, Step, Value, bad, curves, json, other, set_kind, str_param, targets,
};

// =====================================================================================
// TEXTEDIT
// =====================================================================================

fn text_of(k: &EntityKind) -> Option<String> {
    match k {
        EntityKind::Text(t) => Some(t.value.clone()),
        EntityKind::AttDef(a) => Some(a.text.value.clone()),
        EntityKind::MText(t) => Some(t.contents.replace("\\P", "\n")),
        EntityKind::Dimension(d) => Some(if d.text.is_empty() { "<>".into() } else { d.text.clone() }),
        EntityKind::MLeader(m) => m.text.as_ref().map(|t| t.contents.replace("\\P", "\n")),
        _ => None,
    }
}

/// A box for the in-place editor showing one line of text where `t` sits.
pub(crate) fn line_box(t: &cadcraft_doc::Text) -> cadcraft_doc::MText {
    use cadcraft_doc::{HAlign, VAlign};
    let col = match t.halign {
        HAlign::Left => 0u8,
        HAlign::Right => 2,
        _ => 1,
    };
    let row = match t.valign {
        VAlign::Top => 0u8,
        VAlign::Middle => 1,
        _ if t.halign == HAlign::Middle => 1,
        _ => 2,
    };
    let left_baseline = t.halign == HAlign::Left && t.valign == VAlign::Baseline;
    let at = if left_baseline { t.insert } else { t.align_pt.unwrap_or(t.insert) };
    cadcraft_doc::MText {
        formula: None,
        insert: at,
        height: t.height,
        width: 0.0,
        attach: row * 3 + col + 1,
        rotation: t.rotation,
        style: t.style.clone(),
        contents: t.value.clone(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    }
}

/// What the in-place editor edits for object `h`: an MTEXT or multileader text as multiline
/// text, a TEXT or attribute definition as one line, a dimension's text with `<>` for its value.
pub(crate) fn editor_for(s: &Session, h: Handle) -> Option<crate::TextEditor> {
    use crate::{EditorKind, TextEditor};
    let d = s.doc().ok()?;
    let key = h.hex();
    match &d.entity(h)?.kind {
        // A text with a formula is edited as its formula.
        EntityKind::MText(t) => {
            let mut m = t.clone();
            // The formula as multiline text: its `\`, `{` and `}` are letters, not codes.
            if let Some(f) = t.formula.as_ref().filter(|f| crate::textformula::is_formula(f)) {
                m.contents = cadcraft_doc::mtext_literal(f);
            }
            Some(TextEditor { mtext: m, kind: EditorKind::MText, key })
        }
        EntityKind::MLeader(m) => m.text.clone().map(|mut mtext| {
            if let Some(f) = mtext.formula.as_ref().filter(|f| crate::textformula::is_formula(f)) {
                mtext.contents = cadcraft_doc::mtext_literal(f);
            }
            TextEditor { mtext, kind: EditorKind::MText, key }
        }),
        EntityKind::Text(t) => {
            let mut b = line_box(t);
            if let Some(f) = t.formula.as_ref().filter(|f| crate::textformula::is_formula(f)) {
                b.contents.clone_from(f);
            }
            Some(TextEditor { mtext: b, kind: EditorKind::Line, key })
        }
        EntityKind::AttDef(a) => {
            let mut b = line_box(&a.text);
            if let Some(f) = a.text.formula.as_ref().filter(|f| crate::textformula::is_formula(f)) {
                b.contents.clone_from(f);
            }
            Some(TextEditor { mtext: b, kind: EditorKind::Line, key })
        }
        EntityKind::Dimension(dm) => {
            let g = cadcraft_render::dimension_in(d, dm);
            let measured = cadcraft_render::dimension_in(
                d,
                &cadcraft_doc::Dimension {
                    text: String::new(),
                    block: None,
                    marks: cadcraft_doc::DimMarks { formula: None, ..dm.marks.clone() },
                    ..dm.clone()
                },
            )
            .value;
            let mtext = cadcraft_doc::MText {
                formula: None,
                insert: g.text_pos.to3(0.0),
                height: if g.text_height > 0.0 && g.text_height.is_finite() { g.text_height } else { 0.18 },
                width: 0.0,
                attach: 5,
                rotation: g.text_angle,
                style: d.dim_style(&dm.style).map(|st| st.text_style.clone()).unwrap_or_else(|| dm.style.clone()),
                contents: match dm.marks.formula.clone().filter(|f| crate::textformula::is_formula(f)) {
                    Some(f) => f,
                    None if dm.text.is_empty() => "<>".into(),
                    None => dm.text.clone(),
                },
                line_spacing: 1.0,
                line_spacing_exact: false,
                columns: None,
            };
            Some(TextEditor { mtext, kind: EditorKind::Dimension { measured }, key })
        }
        _ => None,
    }
}

pub(crate) fn set_text(s: &mut Session, h: Handle, text: &str) -> Result<()> {
    if curves::is_locked(s, h) {
        return Err(other("The object is on a locked layer."));
    }
    let mut k = curves::entity(s, h)?.kind;
    // The text as the editor showed it (a formula, else the value): unchanged, it keeps its
    // formula or AutoCAD field.
    let f = |x: &Option<String>| x.clone().filter(|f| crate::textformula::is_formula(f));
    let shown = match &k {
        EntityKind::Text(t) => f(&t.formula).unwrap_or_else(|| t.value.clone()),
        EntityKind::AttDef(a) => f(&a.text.formula).unwrap_or_else(|| a.text.value.clone()),
        EntityKind::MText(t) => f(&t.formula).map(|f| cadcraft_doc::mtext_literal(&f)).unwrap_or_else(|| t.contents.clone()),
        EntityKind::MLeader(m) => {
            m.text.as_ref().map(|t| f(&t.formula).map(|f| cadcraft_doc::mtext_literal(&f)).unwrap_or_else(|| t.contents.clone())).unwrap_or_default()
        }
        EntityKind::Dimension(d) => f(&d.marks.formula).unwrap_or_else(|| d.text.clone()),
        _ => String::new(),
    };
    // Multiline text comes back from the editor rewritten (its codes in the editor's own form):
    // compared in that form, an edit that changed nothing keeps the formula.
    let canonical = |x: &str| crate::mtext_edit::Formatted::from_contents(x).to_contents();
    let multiline = matches!(k, EntityKind::MText(_) | EntityKind::MLeader(_));
    let unchanged = shown == text || shown == text.replace('\n', "\\P") || (multiline && canonical(&shown) == canonical(&text.replace('\n', "\\P")));
    match &mut k {
        EntityKind::Text(t) => t.value = text.to_string(),
        EntityKind::AttDef(a) => a.text.value = text.to_string(),
        EntityKind::MText(t) => t.contents = text.replace('\n', "\\P"),
        EntityKind::Dimension(d) => {
            d.text = if text == "<>" { String::new() } else { text.to_string() };
            d.block = None;
        }
        EntityKind::MLeader(m) => match &mut m.text {
            Some(t) => t.contents = text.replace('\n', "\\P"),
            None => return Err(other("This multileader has no text.")),
        },
        _ => return Err(other("Select a text, mtext, attribute definition or dimension.")),
    }
    let mut env = crate::textformula::env(s);
    if let EntityKind::Dimension(dm) = &k {
        env.measured = crate::textformula::measured(s.doc()?, dm);
    }
    crate::textformula::apply(&mut k, &env, !unchanged);
    set_kind(s, h, k)
}

pub(super) fn run_updatefield(s: &mut Session, _p: &Value) -> Result<Value> {
    // Without a formula or field the drawing is left as it is (not marked changed).
    if !crate::textformula::has_any(s.doc()?) {
        s.echo("0 field(s) updated");
        return Ok(json!({ "updated": 0 }));
    }
    let env = crate::textformula::env(s);
    let n = crate::textformula::refresh(s.doc_mut()?, &env);
    s.echo(format!("{n} field(s) updated"));
    Ok(json!({ "updated": n }))
}

pub(super) fn run_textedit(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("textedit", "`handle` is required"))?;
    let text = str_param(p, "text").ok_or_else(|| bad("textedit", "`text` is required"))?;
    if text.len() > 1_000_000 {
        return Err(bad("textedit", "text is too long"));
    }
    set_text(s, h, text)?;
    Ok(json!({ "handle": h.hex() }))
}

#[derive(Default)]
pub(super) struct TextEditM {
    h: Option<(Handle, String)>,
    /// What the in-place editor shows for the picked object.
    editor: Option<crate::TextEditor>,
    /// Edits made in this run, newest last: each object as it was before (the Undo option).
    edits: Vec<(Handle, EntityKind)>,
    /// Single mode: end after one edit (the Mode option; Multiple by default).
    single: bool,
    /// Asking for the edit mode.
    mode: bool,
}

impl TextEditM {
    /// TEXTEDIT for one object (a double-click): done after it.
    pub(super) fn single() -> Self {
        Self { single: true, ..Self::default() }
    }
}

impl Interactive for TextEditM {
    fn name(&self) -> &'static str {
        "TEXTEDIT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.mode {
            let current = if self.single { "Single" } else { "Multiple" };
            return Prompt::new("Enter a text edit mode option", curves::KW).kw(&["Single", "Multiple"]).default(current);
        }
        match &self.h {
            None => Prompt::new("Select an annotation object", Accept::POINT).kw(&["Undo", "Mode"]),
            Some((_, old)) => Prompt::new("Enter new text", Accept::TEXT).default(old.chars().take(40).collect::<String>()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.mode {
            match i {
                Input::Keyword(k) if k == "Single" => self.single = true,
                Input::Keyword(k) if k == "Multiple" => self.single = false,
                Input::Enter => {}
                _ => return Err(other("Enter Single or Multiple.")),
            }
            self.mode = false;
            return Ok(Step::Continue);
        }
        match (&self.h, i) {
            (None, Input::Point(p)) => {
                let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                let old = text_of(&curves::entity(s, h)?.kind).ok_or_else(|| other("Select a text, mtext, attribute definition or dimension."))?;
                self.editor = editor_for(s, h);
                self.h = Some((h, old));
                Ok(Step::Continue)
            }
            (Some((h, _)), Input::Text(t) | Input::Keyword(t)) => {
                let h = *h;
                let before = curves::entity(s, h)?.kind;
                set_text(s, h, &t)?;
                self.edits.push((h, before));
                self.h = None;
                self.editor = None;
                Ok(if self.single { Step::Done } else { Step::Continue })
            }
            (Some(_), Input::Enter) => {
                self.h = None;
                self.editor = None;
                Ok(if self.single { Step::Done } else { Step::Continue })
            }
            (None, Input::Enter) => Ok(Step::Done),
            (None, Input::Keyword(k)) if k == "Undo" => {
                match self.edits.pop() {
                    Some((h, before)) => set_kind(s, h, before)?,
                    None => s.echo("Everything has been undone"),
                }
                Ok(Step::Continue)
            }
            (None, Input::Keyword(k)) if k == "Mode" => {
                self.mode = true;
                Ok(Step::Continue)
            }
            _ => Ok(Step::Continue),
        }
    }
    fn text_editor(&self) -> Option<crate::TextEditor> {
        if self.mode || self.h.is_none() { None } else { self.editor.clone() }
    }
}
