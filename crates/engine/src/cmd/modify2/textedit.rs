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
        // A multileader's text (one with a block instead has none, #1081).
        EntityKind::MLeader(m) => m.text.as_ref().map(|t| t.contents.replace("\\P", "\n")),
        EntityKind::Dimension(d) => Some(if d.text.is_empty() { "<>".into() } else { d.text.clone() }),
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
    // Aligned and fit text is centred between its two points.
    let at = match t.halign {
        HAlign::Aligned | HAlign::Fit => t.align_pt.map_or(t.insert, |b| (t.insert + b) * 0.5),
        _ => t.justify_point().to3(t.insert.z),
    };
    cadcraft_doc::MText {
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

/// The extents of one line of text as drawn (`value` in `t`'s place, style and justification);
/// `None` for empty or unusable extents.
fn drawn(d: &cadcraft_doc::Drawing, t: &cadcraft_doc::Text, value: &str) -> Option<cadcraft_geom::Bounds2> {
    let (_, b) = cadcraft_render::place_text_entity(d, t, value);
    let ok = [b.min.x, b.min.y, b.max.x, b.max.y].iter().all(|v| v.is_finite()) && b.min.x <= b.max.x && b.min.y <= b.max.y;
    ok.then_some(b)
}

/// What the in-place editor edits for object `h`: an MTEXT or multileader text as multiline
/// text, a TEXT or attribute definition as one line, a dimension's text with `<>` for its value.
pub(crate) fn editor_for(s: &Session, h: Handle) -> Option<crate::TextEditor> {
    use crate::{EditorKind, TextEditor};
    let d = s.doc().ok()?;
    let key = h.hex();
    let e: &cadcraft_doc::Entity = d.entity(h)?;
    // Annotative text and dimensions are drawn scaled to the current annotation scale: the box
    // follows what is drawn. Multiline text keeps its stored height (it is written back from it).
    let scaled = match d.anno_draw(e) {
        cadcraft_doc::AnnoDraw::Scaled(f) if matches!(e.kind, EntityKind::Text(_) | EntityKind::AttDef(_) | EntityKind::Dimension(_)) => {
            Some(cadcraft_doc::anno_scaled(e, f))
        }
        _ => None,
    };
    match &scaled.as_ref().unwrap_or(e).kind {
        EntityKind::MText(t) => Some(TextEditor { mtext: t.clone(), kind: EditorKind::MText, key, extents: None }),
        // A multileader's text opens in the same editor (#1081).
        EntityKind::MLeader(m) => m.text.clone().map(|mtext| TextEditor { mtext, kind: EditorKind::MText, key, extents: None }),
        // The box covers the text as drawn: its value, or a definition's tag.
        EntityKind::Text(t) => Some(TextEditor { mtext: line_box(t), kind: EditorKind::Line, key, extents: drawn(d, t, &t.value) }),
        EntityKind::AttDef(a) => Some(TextEditor { mtext: line_box(&a.text), kind: EditorKind::Line, key, extents: drawn(d, &a.text, &a.tag) }),
        EntityKind::Dimension(dm) => {
            let g = cadcraft_render::dimension_in(d, dm);
            let measured = cadcraft_render::dimension_in(d, &cadcraft_doc::Dimension { text: String::new(), block: None, ..dm.clone() }).value;
            let mtext = cadcraft_doc::MText {
                insert: g.text_pos.to3(0.0),
                height: if g.text_height > 0.0 && g.text_height.is_finite() { g.text_height } else { 0.18 },
                width: 0.0,
                attach: 5,
                rotation: if g.text_angle.is_finite() { g.text_angle } else { 0.0 },
                style: d.dim_style(&dm.style).map_or_else(|| "Standard".to_string(), |st| st.text_style.clone()),
                contents: if dm.text.is_empty() { "<>".into() } else { dm.text.clone() },
                line_spacing: 1.0,
                line_spacing_exact: false,
                columns: None,
            };
            Some(TextEditor { mtext, kind: EditorKind::Dimension { measured }, key, extents: None })
        }
        _ => None,
    }
}

pub(crate) fn set_text(s: &mut Session, h: Handle, text: &str) -> Result<()> {
    if curves::is_locked(s, h) {
        return Err(other("The object is on a locked layer."));
    }
    let mut k = curves::entity(s, h)?.kind;
    match &mut k {
        EntityKind::Text(t) => t.value = text.to_string(),
        EntityKind::AttDef(a) => a.text.value = text.to_string(),
        EntityKind::MText(t) => t.contents = text.replace('\n', "\\P"),
        EntityKind::MLeader(m) if m.text.is_some() => {
            if let Some(t) = &mut m.text {
                t.contents = text.replace('\n', "\\P");
            }
        }
        EntityKind::Dimension(d) => {
            d.text = if text == "<>" { String::new() } else { text.to_string() };
            d.block = None;
        }
        _ => return Err(other("Select a text, mtext, multileader, attribute definition or dimension.")),
    }
    set_kind(s, h, k)
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
    /// Edits made in this run, newest last: each object as it was before (the Undo option).
    edits: Vec<(Handle, EntityKind)>,
    /// Single mode: end after one edit (the Mode option; Multiple by default).
    single: bool,
    /// Asking for the edit mode.
    mode: bool,
    /// What the in-place editor shows for the picked object (#842).
    editor: Option<crate::TextEditor>,
}

impl Interactive for TextEditM {
    fn name(&self) -> &'static str {
        "TEXTEDIT"
    }
    /// An MTEXT asking for its new text opens in the in-place editor with its contents.
    fn editing_mtext(&self) -> Option<cadcraft_doc::MText> {
        self.h.as_ref().and(self.editor.as_ref().filter(|e| e.kind == crate::EditorKind::MText).map(|e| e.mtext.clone()))
    }
    /// TEXT, an attribute definition or a dimension asking for its new text opens in the one-line
    /// in-place editor; MTEXT and multileader text in the MTEXT editor.
    fn text_editor(&self) -> Option<crate::TextEditor> {
        if self.mode || self.h.is_none() { None } else { self.editor.clone() }
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
                // Refused when picked, not after the new text has been typed.
                if curves::is_locked(s, h) {
                    return Err(other("The object is on a locked layer."));
                }
                let old = text_of(&curves::entity(s, h)?.kind)
                    .ok_or_else(|| other("Select a text, mtext, multileader, attribute definition or dimension."))?;
                self.editor = editor_for(s, h);
                // The editor would drop part of a text this long, and saving would lose it: the
                // command line asks for it instead.
                if self.editor.as_ref().is_some_and(|e| !crate::mtext_edit::Formatted::fits(&e.mtext.contents)) {
                    self.editor = None;
                    s.echo("This text is too long for the editor; enter the new text at the command line.");
                }
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
}
