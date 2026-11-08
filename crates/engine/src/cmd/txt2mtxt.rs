//! TXT2MTXT (Express Tools): single-line TEXT, and MTEXT when combining, turned into MTEXT.
//!
//! Combined, the texts become one MTEXT in place of the first of them (its handle, layer and
//! other properties kept), one line each, from the top down or in selection order, at the top
//! left of their extents. Otherwise each TEXT becomes its own MTEXT, attached at the point it
//! is justified by.

use cadcraft_doc::{EntityKind, HAlign, Handle, MText, Text, VAlign, entity_bounds, mtext_literal};
use cadcraft_geom::Bounds2;
use serde_json::{Value, json};

use super::curves::KW;
use super::helpers::v3;
use super::machines::{SelOutcome, SelectPhase};
use super::{CommandSpec, bad, bool_or, str_param, targets};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("txt2mtxt", "Convert Text to Mtext", run_txt2mtxt)
            .menu(&["Tools", "Express Tools", "Convert Text to Mtext"])
            .params(
                "{handles? (default the selection; TEXT, and MTEXT when combining), combine? (default true: one MTEXT; false: one per TEXT), \
                 sort?: \"topdown\" (default) | \"selection\", wrap? (default false: one line per text)} → {handles}",
            )
            .interactive(|s| Ok(Box::new(Txt2MtxtM::new(s)))),
    ]
}

/// TXT2MTXT's settings: one MTEXT or one per TEXT, the order of the lines and word wrap.
#[derive(Clone, Debug, PartialEq)]
pub struct Txt2MtxtOptions {
    pub combine: bool,
    pub top_down: bool,
    pub wrap: bool,
}

impl Default for Txt2MtxtOptions {
    fn default() -> Self {
        Txt2MtxtOptions { combine: true, top_down: true, wrap: false }
    }
}

/// Converts the TEXT (and, combining, MTEXT) objects among `hs`; objects on locked layers are
/// left alone. Returns the MTEXT objects made and how many texts were on a locked layer.
fn convert(s: &mut Session, hs: &[Handle], o: &Txt2MtxtOptions) -> Result<(Vec<Handle>, usize)> {
    let d = s.doc()?;
    let mut locked = 0;
    let mut items: Vec<(Handle, EntityKind, Bounds2)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for h in hs {
        let Some(e) = d.entity(*h) else { continue };
        let wanted = match &e.kind {
            EntityKind::Text(_) => true,
            EntityKind::MText(_) => o.combine,
            _ => false,
        };
        if !wanted || !seen.insert(*h) {
            continue;
        }
        if d.layer(&e.common.layer).is_some_and(|l| l.locked) {
            locked += 1;
            continue;
        }
        items.push((*h, e.kind.clone(), entity_bounds(d, e, 0)));
    }
    if !o.combine {
        let mut made = Vec::new();
        for (h, k, _) in items {
            if let EntityKind::Text(t) = k {
                s.doc_mut()?.modify_entity(h, |e| e.kind = EntityKind::MText(single(&t)))?;
                made.push(h);
            }
        }
        return Ok((made, locked));
    }
    if o.top_down {
        // Highest first; a line shared by several texts reads left to right.
        items.sort_by(|a, b| {
            let (pa, pb) = (insertion(&a.1), insertion(&b.1));
            pb.y.total_cmp(&pa.y).then(pa.x.total_cmp(&pb.x))
        });
    }
    // A lone MTEXT is already what TXT2MTXT would make of it.
    if let [(_, EntityKind::MText(_), _)] = &items[..] {
        return Ok((Vec::new(), locked));
    }
    let Some((first, k0, _)) = items.first() else { return Ok((Vec::new(), locked)) };
    let (height, style, rotation) = match k0 {
        EntityKind::Text(t) => (t.height, t.style.clone(), t.rotation),
        EntityKind::MText(m) => (m.height, m.style.clone(), m.rotation),
        _ => return Ok((Vec::new(), locked)),
    };
    let ext = items.iter().fold(Bounds2::EMPTY, |b, (.., x)| b.union(x));
    let corner = if ext.is_empty() { insertion(k0) } else { cadcraft_geom::Vec2::new(ext.min.x, ext.max.y) };
    let sep = if o.wrap { " " } else { "\\P" };
    let contents = items.iter().map(|(_, k, _)| contents(k, height)).collect::<Vec<_>>().join(sep);
    let width = if o.wrap { items.iter().map(|(_, k, _)| line_width(k)).fold(0.0, f64::max) } else { 0.0 };
    let m = MText {
        insert: v3(corner),
        height,
        width,
        attach: 1,
        rotation,
        style,
        contents,
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    let first = *first;
    let rest: Vec<Handle> = items.iter().skip(1).map(|(h, ..)| *h).collect();
    let d = s.doc_mut()?;
    d.modify_entity(first, |e| e.kind = EntityKind::MText(m))?;
    for h in rest {
        d.remove_entity(h);
    }
    Ok((vec![first], locked))
}

/// The point a TEXT or MTEXT is sorted by: its insertion point.
fn insertion(k: &EntityKind) -> cadcraft_geom::Vec2 {
    match k {
        EntityKind::Text(t) => t.insert.xy(),
        EntityKind::MText(m) => m.insert.xy(),
        _ => cadcraft_geom::Vec2::ZERO,
    }
}

/// A text's lines as MTEXT contents of base height `base`: the TEXT's characters read literally
/// (its width factor and oblique angle as inline codes), an MTEXT's contents as they are; either
/// keeps its own height with an absolute `\H` code when that differs from `base`.
fn contents(k: &EntityKind, base: f64) -> String {
    match k {
        EntityKind::Text(t) => text_contents(t, base),
        EntityKind::MText(m) if differs(m.height, base) => format!("{{\\H{};{}}}", m.height, m.contents),
        EntityKind::MText(m) => m.contents.clone(),
        _ => String::new(),
    }
}

/// Whether a line of height `h` needs its own height code in an MTEXT of height `base`.
fn differs(h: f64, base: f64) -> bool {
    h.is_finite() && h > 0.0 && (h - base).abs() > base.abs() * 1e-9
}

fn text_contents(t: &Text, base: f64) -> String {
    let mut codes = String::new();
    if differs(t.height, base) {
        codes.push_str(&format!("\\H{};", t.height));
    }
    if t.width_factor.is_finite() && t.width_factor > 0.0 && (t.width_factor - 1.0).abs() > 1e-9 {
        codes.push_str(&format!("\\W{};", t.width_factor));
    }
    if t.oblique.is_finite() && t.oblique.abs() > 1e-9 {
        codes.push_str(&format!("\\Q{};", t.oblique.to_degrees()));
    }
    let lit = mtext_literal(&t.value);
    if codes.is_empty() { lit } else { format!("{{{codes}{lit}}}") }
}

/// How wide a text's longest line is in the drafting font: word wrap's MTEXT width.
fn line_width(k: &EntityKind) -> f64 {
    let w = match k {
        EntityKind::Text(t) => cadcraft_fonts::line_width(&t.value, t.height, t.width_factor),
        EntityKind::MText(m) if m.width > 0.0 => m.width,
        EntityKind::MText(m) => {
            cadcraft_fonts::plain_mtext(&m.contents).lines().map(|l| cadcraft_fonts::line_width(l, m.height, 1.0)).fold(0.0, f64::max)
        }
        _ => 0.0,
    };
    if w.is_finite() { w } else { 0.0 }
}

/// One TEXT as an MTEXT of its own, attached where the TEXT is justified (a baseline as the
/// bottom).
fn single(t: &Text) -> MText {
    let col = match t.halign {
        HAlign::Left | HAlign::Aligned | HAlign::Fit => 0,
        HAlign::Center | HAlign::Middle => 1,
        HAlign::Right => 2,
    };
    let row = match (t.halign, t.valign) {
        (HAlign::Middle, _) | (_, VAlign::Middle) => 1,
        (_, VAlign::Top) => 0,
        _ => 2,
    };
    MText {
        insert: v3(t.justify_point()),
        height: t.height,
        width: 0.0,
        attach: row * 3 + col + 1,
        rotation: t.rotation,
        style: t.style.clone(),
        contents: text_contents(t, t.height),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    }
}

fn locked_note(n: usize) -> Option<String> {
    (n > 0).then(|| format!("{n} object(s) on a locked layer."))
}

fn run_txt2mtxt(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let top_down = match str_param(p, "sort") {
        None | Some("topdown") => true,
        Some("selection") => false,
        Some(v) => return Err(bad("txt2mtxt", format!("`sort` must be \"topdown\" or \"selection\", got \"{v}\""))),
    };
    let o = Txt2MtxtOptions { combine: bool_or(p, "combine", true), top_down, wrap: bool_or(p, "wrap", false) };
    let (made, locked) = convert(s, &hs, &o)?;
    if made.is_empty() {
        let why = locked_note(locked).unwrap_or_else(|| "No text objects selected.".into());
        return Err(EngineError::Other(why));
    }
    s.set_selection(Vec::new());
    let mut r = json!({ "handles": made.iter().map(|h| h.hex()).collect::<Vec<_>>() });
    if let Some(m) = locked_note(locked) {
        r["message"] = json!(m);
    }
    Ok(r)
}

#[derive(Clone, Copy, PartialEq)]
enum Ask {
    Select,
    Combine,
    Sort,
    Wrap,
}

struct Txt2MtxtM {
    opts: Txt2MtxtOptions,
    ask: Ask,
    sel: SelectPhase,
}

impl Txt2MtxtM {
    fn new(s: &Session) -> Self {
        Txt2MtxtM { opts: s.last_used.txt2mtxt.clone(), ask: Ask::Select, sel: SelectPhase::default() }
    }
    fn finish(&self, s: &mut Session, hs: &[Handle]) -> Result<Step> {
        let (made, locked) = convert(s, hs, &self.opts)?;
        if let Some(m) = locked_note(locked) {
            s.echo(m);
        }
        if made.is_empty() {
            s.echo("No text objects selected.");
        }
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
}

fn yes_no(b: bool) -> &'static str {
    if b { "Yes" } else { "No" }
}

impl Interactive for Txt2MtxtM {
    fn name(&self) -> &'static str {
        "TXT2MTXT"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let o = &self.opts;
        s.echo(format!("Current settings: Combine= {}, Sort top-down= {}, Word-wrap= {}", yes_no(o.combine), yes_no(o.top_down), yes_no(o.wrap)));
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return self.finish(s, &hs);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.ask {
            Ask::Select if self.sel.removing => self.sel.prompt(),
            Ask::Select => Prompt::new("Select objects", Accept::SELECT).kw(&["SEttings"]),
            Ask::Combine => Prompt::new("Combine text into a single MTEXT?", KW).kw(&["Yes", "No"]).default(yes_no(self.opts.combine)),
            Ask::Sort => Prompt::new("Sort text top-down?", KW).kw(&["Yes", "No"]).default(yes_no(self.opts.top_down)),
            Ask::Wrap => Prompt::new("Word-wrap text?", KW).kw(&["Yes", "No"]).default(yes_no(self.opts.wrap)),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let k = match &i {
            Input::Keyword(k) => Some(k.as_str()),
            _ => None,
        };
        match self.ask {
            Ask::Select => {
                if k == Some("SEttings") {
                    self.ask = Ask::Combine;
                    return Ok(Step::Continue);
                }
                return match self.sel.feed(s, &i)? {
                    SelOutcome::More => Ok(Step::Continue),
                    SelOutcome::Empty => Ok(Step::Done),
                    SelOutcome::Done(hs) => self.finish(s, &hs),
                };
            }
            Ask::Combine => {
                match k {
                    Some("Yes") => self.opts.combine = true,
                    Some("No") => self.opts.combine = false,
                    _ if i == Input::Enter => {}
                    _ => return Ok(Step::Continue),
                }
                // Sorting and word wrap only apply to text combined into one MTEXT.
                self.ask = if self.opts.combine { Ask::Sort } else { Ask::Select };
            }
            Ask::Sort => {
                match k {
                    Some("Yes") => self.opts.top_down = true,
                    Some("No") => self.opts.top_down = false,
                    _ if i == Input::Enter => {}
                    _ => return Ok(Step::Continue),
                }
                self.ask = Ask::Wrap;
            }
            Ask::Wrap => {
                match k {
                    Some("Yes") => self.opts.wrap = true,
                    Some("No") => self.opts.wrap = false,
                    _ if i == Input::Enter => {}
                    _ => return Ok(Step::Continue),
                }
                self.ask = Ask::Select;
            }
        }
        s.last_used.txt2mtxt = self.opts.clone();
        Ok(Step::Continue)
    }
}

#[cfg(test)]
mod tests;
