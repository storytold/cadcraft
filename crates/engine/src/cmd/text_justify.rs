//! JUSTIFYTEXT and SCALETEXT (Express Tools): change the justification of TEXT, MTEXT and block
//! attributes without moving what is drawn, and scale their height about a chosen base point.
//!
//! Both work on the text as it is drawn: the point a justification places the text by (its
//! alignment point, or an MTEXT's attachment point) is worked out from the text's own extents with
//! the same font code the renderer places it with, so what the drawing shows does not move.

use cadcraft_doc::{Drawing, EntityKind, HAlign, Handle, MText, Text, VAlign};
use cadcraft_fonts::{TextFont, TextParams, baseline_start, place};
use cadcraft_geom::{Mat3, Vec2};
use serde_json::{Value, json};

use super::curves::KW;
use super::draw::justify;
use super::machines::{SelOutcome, SelectPhase, number};
use super::{CommandSpec, bad, f64_or, str_param, targets};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("justifytext", "Justify Text", run_justifytext)
            .menu(&["Tools", "Additional Tools", "Justify Text"])
            .params(
                "{handles? (default the selection; TEXT, MTEXT, attribute definitions and the attributes of block references), \
                 justify: \"Existing\" | Left | Align | Fit | Center | Middle | Right | TL | TC | TR | ML | MC | MR | BL | BC | BR} \
                 → {changed, locked, ignored}. The text stays where it is drawn; MTEXT takes the attachment points (Left, Align and Fit are bottom left, Center and Right \
                 attach at the baseline, as the bottom)",
            )
            .interactive(|_| Ok(Box::new(JustifyM::default()))),
        CommandSpec::new("scaletext", "Scale Text", run_scaletext)
            .menu(&["Tools", "Additional Tools", "Scale Text"])
            .params(
                "{handles? (as JUSTIFYTEXT), base?: \"Existing\" (default) | Left | Center | Middle | Right | TL | TC | TR | ML | MC | MR | BL | BC | BR, \
                 one of: height (the new height), factor (a scale factor), match (the handle of the text whose height to take)} \
                 → {changed, locked, ignored}. Each text is scaled about its own base point",
            )
            .interactive(|_| Ok(Box::new(ScaleM::default()))),
    ]
}

/// A justification as asked for: the text's own, or one the TEXT command names.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Just {
    Existing,
    Is(HAlign, VAlign),
}

fn parse_just(name: &str) -> Option<Just> {
    let n = name.trim();
    if n.eq_ignore_ascii_case("existing") || n.eq_ignore_ascii_case("e") {
        return Some(Just::Existing);
    }
    justify(n).map(|(h, v)| Just::Is(h, v))
}

/// The MTEXT attachment (1..=9) a TEXT justification stands for, a baseline as the bottom. Align
/// and Fit are bottom left, as AutoCAD's JUSTIFYTEXT has them for MTEXT.
fn attach_of(h: HAlign, v: VAlign) -> Option<u8> {
    let col = match h {
        HAlign::Left | HAlign::Aligned | HAlign::Fit => 0,
        HAlign::Center => 1,
        HAlign::Right => 2,
        HAlign::Middle => 1,
    };
    let row = match (h, v) {
        (HAlign::Middle, _) | (_, VAlign::Middle) => 1,
        (_, VAlign::Top) => 0,
        _ => 2,
    };
    Some(row * 3 + col + 1)
}

fn two_points(h: HAlign) -> bool {
    matches!(h, HAlign::Aligned | HAlign::Fit)
}

// ---------------- TEXT ----------------

/// A TEXT in its own frame: where its baseline starts, its rotation, and the height and width
/// factor it is drawn with (aligned and fit text are drawn between their points).
struct Frame {
    start: Vec2,
    rot: f64,
    height: f64,
    wf: f64,
    width: f64,
    font: TextFont,
    backwards: bool,
    upside_down: bool,
}

fn frame(d: &Drawing, t: &Text) -> Frame {
    let ts = d.text_style(&t.style).cloned().unwrap_or_default();
    let font = cadcraft_render::style_font(&ts);
    let (h, v) = cadcraft_render::text_align(t.halign, t.valign);
    let p = TextParams {
        insert: t.insert.xy(),
        align_pt: t.align_pt.map(|a| a.xy()),
        height: t.height,
        rotation: t.rotation,
        width_factor: t.width_factor,
        oblique: t.oblique,
        h,
        v,
        backwards: ts.backwards,
        upside_down: ts.upside_down,
    };
    let start = baseline_start(&font, &t.value, &p);
    let mut f = Frame {
        start,
        rot: t.rotation,
        height: t.height,
        wf: t.width_factor,
        width: place(&font, &t.value, &p).0.width,
        font,
        backwards: ts.backwards,
        upside_down: ts.upside_down,
    };
    if let (true, Some(end)) = (two_points(t.halign), t.align_pt) {
        // As the renderer fits it between its points.
        let len = t.insert.xy().dist(end.xy());
        let plain = TextParams { h: cadcraft_fonts::Align::Left, v: cadcraft_fonts::VAlign::Baseline, align_pt: None, ..p };
        let w = place(&f.font, &t.value, &plain).0.width;
        if w > 1e-12 && len > 1e-12 {
            let k = len / w;
            if t.halign == HAlign::Aligned {
                f.height *= k;
            } else {
                f.wf *= k;
            }
        }
        f.rot = t.insert.xy().angle_to(end.xy());
        f.width = len;
    }
    f
}

/// Where the text would be placed by justification `to` (not aligned or fit) for it to draw
/// where it is.
fn text_anchor(t: &Text, f: &Frame, to: (HAlign, VAlign)) -> Vec2 {
    let (h, v) = cadcraft_render::text_align(to.0, to.1);
    let p = TextParams {
        insert: Vec2::ZERO,
        align_pt: Some(Vec2::ZERO),
        height: f.height,
        rotation: f.rot,
        width_factor: f.wf,
        oblique: t.oblique,
        h,
        v,
        backwards: f.backwards,
        upside_down: f.upside_down,
    };
    f.start - baseline_start(&f.font, &t.value, &p)
}

/// The point a base-point option stands for on `t`: its alignment point for Existing.
fn text_base(t: &Text, f: &Frame, j: Just) -> Vec2 {
    match j {
        Just::Is(h, v) if !two_points(h) => text_anchor(t, f, (h, v)),
        _ => t.justify_point(),
    }
}

/// Give `t` justification `to` and keep it drawn where it is. Whether it changed.
fn justify_text(d: &Drawing, t: &mut Text, to: Just) -> bool {
    let Just::Is(h, v) = to else { return false };
    let f = frame(d, t);
    let z = t.insert.z;
    let (v, anchor) = if two_points(h) { (VAlign::Baseline, f.start) } else { (v, text_anchor(t, &f, (h, v))) };
    t.height = f.height;
    t.width_factor = f.wf;
    t.rotation = f.rot;
    t.halign = h;
    t.valign = v;
    t.insert = anchor.to3(z);
    t.align_pt = if two_points(h) {
        Some(Vec2::polar(f.start, f.width, f.rot).to3(z))
    } else if (h, v) == (HAlign::Left, VAlign::Baseline) {
        None
    } else {
        Some(anchor.to3(z))
    };
    true
}

// ---------------- MTEXT ----------------

/// The width and height of an MTEXT's block of text, as it is laid out.
fn mtext_block(d: &Drawing, m: &MText) -> (f64, f64) {
    let at = |attach: u8| {
        let probe = MText { insert: cadcraft_geom::Vec3::new(0.0, 0.0, 0.0), rotation: 0.0, attach, ..m.clone() };
        cadcraft_render::layout_mtext_entity(d, &probe).bounds
    };
    let (tl, tr, bl) = (at(1), at(3), at(7));
    if tl.is_empty() || tr.is_empty() || bl.is_empty() {
        return (m.width.max(0.0), m.height);
    }
    let w = if m.width > 0.0 { m.width } else { tl.min.x - tr.min.x };
    (w.max(0.0), (bl.min.y - tl.min.y).max(0.0))
}

/// The insertion point that attaches the MTEXT at `attach` with its text where it is.
fn mtext_anchor(d: &Drawing, m: &MText, attach: u8) -> Vec2 {
    let parts = |a: u8| (f64::from((a.clamp(1, 9) - 1) % 3), f64::from((a.clamp(1, 9) - 1) / 3));
    let (w, h) = mtext_block(d, m);
    let ((ca, ra), (cb, rb)) = (parts(m.attach), parts(attach));
    m.insert.xy() + Vec2::new((cb - ca) * w / 2.0, (ra - rb) * h / 2.0).rotate(m.rotation)
}

fn justify_mtext(d: &Drawing, m: &mut MText, to: Just) -> bool {
    let Just::Is(h, v) = to else { return false };
    let Some(attach) = attach_of(h, v) else { return false };
    let at = mtext_anchor(d, m, attach);
    m.attach = attach;
    m.insert.x = at.x;
    m.insert.y = at.y;
    true
}

// ---------------- the objects ----------------

/// What a command did to the objects it was given.
#[derive(Default)]
struct Tally {
    changed: usize,
    locked: usize,
    ignored: usize,
}

impl Tally {
    fn json(&self) -> Value {
        json!({ "changed": self.changed, "locked": self.locked, "ignored": self.ignored })
    }
    fn note(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.locked > 0 {
            parts.push(format!("{} on a locked layer", self.locked));
        }
        if self.ignored > 0 {
            parts.push(format!("{} not changed", self.ignored));
        }
        (!parts.is_empty()).then(|| format!("{}.", parts.join(", ")))
    }
}

/// Run `text` on each TEXT, attribute definition and block attribute among `hs`, and `mtext` on
/// each MTEXT. Each returns whether it changed the object; the others are counted as ignored, and
/// objects on a locked layer are left alone. One document change per object.
fn edit_texts(
    s: &mut Session,
    hs: &[Handle],
    text: impl Fn(&Drawing, &mut Text) -> bool,
    mtext: impl Fn(&Drawing, &mut MText) -> bool,
) -> Result<Tally> {
    let mut tally = Tally::default();
    let mut seen = std::collections::HashSet::new();
    for h in hs {
        if !seen.insert(*h) {
            continue;
        }
        let d = s.doc()?;
        let Some(e) = d.entity(*h) else { continue };
        if d.layer(&e.common.layer).is_some_and(|l| l.locked) {
            tally.locked += 1;
            continue;
        }
        let mut kind = e.kind.clone();
        let (mut yes, mut no) = (0, 0);
        let mut count = |ok: bool| if ok { yes += 1 } else { no += 1 };
        match &mut kind {
            EntityKind::Text(t) => count(text(d, t)),
            EntityKind::MText(m) => count(mtext(d, m)),
            EntityKind::AttDef(a) if a.multiline.is_none() => count(text(d, &mut a.text)),
            EntityKind::Insert(i) => {
                for a in i.attribs.iter_mut().filter(|a| a.multiline.is_none()) {
                    count(text(d, &mut a.text));
                }
            }
            _ => no += 1,
        }
        if yes > 0 {
            s.doc_mut()?.modify_entity(*h, |e| e.kind = kind)?;
        }
        tally.changed += yes;
        tally.ignored += no;
    }
    Ok(tally)
}

fn done(s: &mut Session, name: &str, hs: &[Handle], t: &Tally) {
    if hs.is_empty() || t.changed == 0 {
        s.echo(format!("{name}: no text objects changed."));
    }
    if let Some(n) = t.note() {
        s.echo(n);
    }
    s.set_selection(Vec::new());
}

// ---------------- JUSTIFYTEXT ----------------

fn run_justifytext(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "justify").ok_or_else(|| bad("justifytext", "`justify` is required (Existing, Left, Center, TL, ...)"))?;
    let to = parse_just(name).ok_or_else(|| bad("justifytext", format!("unknown justification \"{name}\"")))?;
    let hs = targets(s, p)?;
    let t = edit_texts(s, &hs, |d, t| justify_text(d, t, to), |d, m| justify_mtext(d, m, to))?;
    s.set_selection(Vec::new());
    Ok(t.json())
}

const JUSTIFY_KEYWORDS: [&str; 16] =
    ["Left", "Align", "Fit", "Center", "Middle", "Right", "TL", "TC", "TR", "ML", "MC", "MR", "BL", "BC", "BR", "Existing"];

#[derive(Default)]
struct JustifyM {
    sel: SelectPhase,
    picked: Option<Vec<Handle>>,
}

impl Interactive for JustifyM {
    fn name(&self) -> &'static str {
        "JUSTIFYTEXT"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            self.picked = Some(self.sel.picked.clone());
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.picked.is_some() {
            Prompt::new("Enter a justification option", KW).kw(&JUSTIFY_KEYWORDS).default("Existing")
        } else {
            self.sel.prompt()
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Some(hs) = self.picked.clone() else {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.picked = Some(hs);
                    Ok(Step::Continue)
                }
            };
        };
        let to = match &i {
            Input::Keyword(k) | Input::Text(k) => match parse_just(k) {
                Some(j) => j,
                None => {
                    s.echo("Invalid option keyword.");
                    return Ok(Step::Continue);
                }
            },
            Input::Enter => Just::Existing,
            Input::Cancel => return Ok(Step::Cancel),
            _ => return Ok(Step::Continue),
        };
        let t = edit_texts(s, &hs, |d, t| justify_text(d, t, to), |d, m| justify_mtext(d, m, to))?;
        done(s, "JUSTIFYTEXT", &hs, &t);
        Ok(Step::Done)
    }
}

// ---------------- SCALETEXT ----------------

/// How SCALETEXT is told the new size.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Size {
    Height(f64),
    Factor(f64),
}

fn scale_text(d: &Drawing, t: &mut Text, base: Just, size: Size) -> bool {
    let f = frame(d, t);
    let at = text_base(t, &f, base);
    let k = match size {
        Size::Height(h) => h / t.height,
        Size::Factor(k) => k,
    };
    if !(k.is_finite() && k > 0.0) || !at.x.is_finite() || !at.y.is_finite() || t.height <= 0.0 {
        return false;
    }
    let mut k3 = EntityKind::Text(t.clone());
    k3.transform(&Mat3::scale_about(at, k));
    if let EntityKind::Text(new) = k3 {
        *t = new;
    }
    true
}

fn scale_mtext(d: &Drawing, m: &mut MText, base: Just, size: Size) -> bool {
    let at = match base {
        Just::Is(h, v) => attach_of(h, v).map_or_else(|| m.insert.xy(), |a| mtext_anchor(d, m, a)),
        Just::Existing => m.insert.xy(),
    };
    let k = match size {
        Size::Height(h) => h / m.height,
        Size::Factor(k) => k,
    };
    if !(k.is_finite() && k > 0.0) || m.height <= 0.0 {
        return false;
    }
    let mut kind = EntityKind::MText(m.clone());
    kind.transform(&Mat3::scale_about(at, k));
    if let EntityKind::MText(new) = kind {
        *m = new;
    }
    true
}

fn scale_texts(s: &mut Session, hs: &[Handle], base: Just, size: Size) -> Result<Tally> {
    edit_texts(s, hs, |d, t| scale_text(d, t, base, size), |d, m| scale_mtext(d, m, base, size))
}

/// The height of the text `h` is, for Match object: a TEXT, MTEXT, attribute definition, or the
/// first attribute of a block reference.
fn height_of(s: &Session, h: Handle) -> Option<f64> {
    let e = s.doc().ok()?.entity(h)?;
    let v = match &e.kind {
        EntityKind::Text(t) => t.height,
        EntityKind::MText(m) => m.height,
        EntityKind::AttDef(a) => a.text.height,
        EntityKind::Insert(i) => i.attribs.first()?.text.height,
        _ => return None,
    };
    (v.is_finite() && v > 0.0).then_some(v)
}

fn run_scaletext(s: &mut Session, p: &Value) -> Result<Value> {
    let base = match str_param(p, "base") {
        None => Just::Existing,
        Some(n) => match parse_just(n) {
            Some(j @ Just::Is(h, _)) if two_points(h) => return Err(bad("scaletext", format!("`base` \"{n}\" is not a base point option: {j:?}"))),
            Some(j) => j,
            None => return Err(bad("scaletext", format!("unknown base point \"{n}\""))),
        },
    };
    let num = |k: &str| p.get(k).filter(|v| !v.is_null()).map(|_| f64_or(p, k, f64::NAN));
    let given = [num("height"), num("factor"), p.get("match").filter(|v| !v.is_null()).map(|_| 0.0)];
    if given.iter().flatten().count() != 1 {
        return Err(bad("scaletext", "give exactly one of `height`, `factor` or `match`"));
    }
    let positive = |k: &str, v: f64| if v.is_finite() && v > 0.0 { Ok(v) } else { Err(bad("scaletext", format!("`{k}` must be a positive number"))) };
    let size = if let Some(v) = num("height") {
        Size::Height(positive("height", v)?)
    } else if let Some(v) = num("factor") {
        Size::Factor(positive("factor", v)?)
    } else {
        let handle = p.get("match").and_then(|v| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle)));
        let h = handle.ok_or_else(|| bad("scaletext", "`match` must be an object handle"))?;
        Size::Height(height_of(s, h).ok_or_else(|| bad("scaletext", "`match` must be a text object"))?)
    };
    let hs = targets(s, p)?;
    let t = scale_texts(s, &hs, base, size)?;
    s.set_selection(Vec::new());
    Ok(t.json())
}

#[derive(Clone, Copy, PartialEq)]
enum Ask {
    Select,
    Base,
    Size,
    Match,
    Factor,
}

struct ScaleM {
    sel: SelectPhase,
    ask: Ask,
    picked: Vec<Handle>,
    base: Just,
}

impl Default for ScaleM {
    fn default() -> Self {
        ScaleM { sel: SelectPhase::default(), ask: Ask::Select, picked: Vec::new(), base: Just::Existing }
    }
}

impl ScaleM {
    fn finish(&mut self, s: &mut Session, size: Size) -> Result<Step> {
        let t = scale_texts(s, &self.picked, self.base, size)?;
        done(s, "SCALETEXT", &self.picked, &t);
        Ok(Step::Done)
    }
    fn selected(&mut self, hs: Vec<Handle>) -> Step {
        self.picked = hs;
        self.ask = Ask::Base;
        Step::Continue
    }
}

impl Interactive for ScaleM {
    fn name(&self) -> &'static str {
        "SCALETEXT"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return Ok(self.selected(hs));
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match self.ask {
            Ask::Select => self.sel.prompt(),
            Ask::Base => {
                let kw: Vec<&str> = JUSTIFY_KEYWORDS.into_iter().filter(|k| !matches!(*k, "Align" | "Fit")).collect();
                Prompt::new("Enter a base point option for scaling", KW).kw(&kw).default("Existing")
            }
            Ask::Size => {
                let cur = self.picked.iter().find_map(|h| height_of(s, *h));
                let p = Prompt::new("Specify new height", Accept::NUMBER).kw(&["Match object", "Scale factor"]);
                match cur {
                    Some(c) => p.default(format!("{c:.4}")),
                    None => p,
                }
            }
            Ask::Match => Prompt::new("Select a text object with the height to match", Accept::SELECT),
            Ask::Factor => Prompt::new("Specify scale factor", Accept::NUMBER).default("1"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if i == Input::Cancel {
            return Ok(Step::Cancel);
        }
        match self.ask {
            Ask::Select => match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => Ok(self.selected(hs)),
            },
            Ask::Base => {
                self.base = match &i {
                    Input::Enter => Just::Existing,
                    Input::Keyword(k) | Input::Text(k) => match parse_just(k) {
                        Some(Just::Is(h, _)) if two_points(h) => return Ok(Step::Continue),
                        Some(j) => j,
                        None => {
                            s.echo("Invalid option keyword.");
                            return Ok(Step::Continue);
                        }
                    },
                    _ => return Ok(Step::Continue),
                };
                self.ask = Ask::Size;
                Ok(Step::Continue)
            }
            Ask::Size => match i {
                Input::Keyword(k) if k == "Match object" => {
                    self.ask = Ask::Match;
                    Ok(Step::Continue)
                }
                Input::Keyword(k) if k == "Scale factor" => {
                    self.ask = Ask::Factor;
                    Ok(Step::Continue)
                }
                Input::Text(t) => match number(&t).filter(|h| h.is_finite() && *h > 0.0) {
                    Some(h) => self.finish(s, Size::Height(h)),
                    None => {
                        s.echo("Requires a positive height.");
                        Ok(Step::Continue)
                    }
                },
                Input::Enter => match self.picked.iter().find_map(|h| height_of(s, *h)) {
                    Some(h) => self.finish(s, Size::Height(h)),
                    None => Ok(Step::Done),
                },
                _ => Ok(Step::Continue),
            },
            Ask::Match => match i {
                Input::Pick(hs) => match hs.iter().find_map(|h| height_of(s, *h)) {
                    Some(h) => self.finish(s, Size::Height(h)),
                    None => {
                        if !hs.is_empty() {
                            s.echo("Object selected is not text.");
                        }
                        Ok(Step::Continue)
                    }
                },
                Input::Enter => Ok(Step::Cancel),
                _ => Ok(Step::Continue),
            },
            Ask::Factor => match i {
                Input::Text(t) => match number(&t).filter(|k| k.is_finite() && *k > 0.0) {
                    Some(k) => self.finish(s, Size::Factor(k)),
                    None => {
                        s.echo("Requires a positive scale factor.");
                        Ok(Step::Continue)
                    }
                },
                Input::Enter => self.finish(s, Size::Factor(1.0)),
                _ => Ok(Step::Continue),
            },
        }
    }
}
