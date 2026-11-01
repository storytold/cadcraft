//! LEADER at the command line, as AutoCAD 2024 runs it (accoreconsole, synthetic drawings): the
//! points, Format and Undo, then annotation text, no annotation, or a block.
//!
//! The leader takes a hook line, a horizontal last segment DIMASZ long, when its last segment is
//! straight and more than 15° off horizontal and text follows it; the text sits DIMGAP past the
//! leader's end, above it (DIMTAD on) or centred on it (DIMTAD 0), on the side the leader runs to.
//! All sizes are scaled by DIMSCALE. The text and the leader are separate objects here: the
//! drawing model has no link from a leader to its annotation.

use super::common::style;
use super::{Accept, EntityKind, Handle, Input, Interactive, MText, Prompt, Result, Session, Step, Vec2, v3};
use crate::cmd::curves::KW;

/// Where LEADER is in its prompts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Ask {
    #[default]
    Points,
    Format,
    FirstLine,
    NextLine,
    Option,
    /// Copy: the text or multiline text to copy.
    Copy,
    BlockName,
    BlockPoint,
    BlockX,
    BlockY,
    BlockRotation,
}

#[derive(Default)]
pub(super) struct LeaderM {
    ask: Ask,
    pts: Vec<Vec2>,
    spline: bool,
    no_arrow: bool,
    lines: Vec<String>,
    block: String,
    block_at: Vec2,
    block_scale: Vec2,
}

/// What follows the leader.
enum Annotation {
    Text(Vec<String>),
    /// A copy of a text or multiline text: its contents, height, width and style.
    Copy(MText),
    None,
    Block {
        name: String,
        at: Vec2,
        scale: Vec2,
        rotation: f64,
    },
}

impl Interactive for LeaderM {
    fn name(&self) -> &'static str {
        "LEADER"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        let last = self.pts.last().copied();
        match self.ask {
            Ask::Points => match self.pts.len() {
                0 => Prompt::new("Specify leader start point", Accept::POINT),
                1 => Prompt::new("Specify next point", Accept::POINT).base_opt(last),
                _ => Prompt::new("Specify next point", Accept::POINT).kw(&["Annotation", "Format", "Undo"]).default("Annotation").base_opt(last),
            },
            Ask::Format => Prompt::new("Enter leader format option", KW).kw(&["Spline", "STraight", "Arrow", "None"]).default("Exit"),
            // "or <options>": the default is the options, not a keyword list.
            Ask::FirstLine => Prompt::new("Enter first line of annotation text or", Accept::TEXT).default("options"),
            Ask::NextLine => Prompt::new("Enter next line of annotation text", Accept::TEXT),
            Ask::Copy => Prompt::new("Select an object to copy", Accept::SELECT),
            Ask::Option => Prompt::new("Enter an annotation option", KW).kw(&["Tolerance", "Copy", "Block", "None", "Mtext"]).default("Mtext"),
            Ask::BlockName => Prompt::new("Enter block name", Accept::TEXT).kw(&["?"]),
            Ask::BlockPoint => Prompt::new("Specify insertion point", Accept::POINT).kw(&["Basepoint", "Scale", "X", "Y", "Z", "Rotate"]),
            Ask::BlockX => Prompt::new("Enter X scale factor, specify opposite corner", Accept::NUMBER).kw(&["Corner", "XYZ"]).default("1"),
            Ask::BlockY => Prompt::new("Enter Y scale factor", Accept::NUMBER).default("use X scale factor"),
            Ask::BlockRotation => Prompt::new("Specify rotation angle", Accept::NUMBER).default("0"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.ask {
            Ask::Points => match i {
                Input::Point(p) => self.pts.push(p),
                // Enter after one point: no leader.
                Input::Enter if self.pts.len() < 2 => return Ok(Step::Done),
                Input::Enter => self.ask = Ask::FirstLine,
                Input::Keyword(k) => match k.as_str() {
                    "Annotation" => self.ask = Ask::FirstLine,
                    "Format" => self.ask = Ask::Format,
                    _ => {
                        self.pts.pop();
                    }
                },
                _ => {}
            },
            Ask::Format => {
                // Enter, or Exit typed, changes nothing; either way back to the points.
                if let Input::Keyword(k) = &i {
                    match k.as_str() {
                        "Spline" => self.spline = true,
                        "STraight" => self.spline = false,
                        "Arrow" => self.no_arrow = false,
                        _ => self.no_arrow = true,
                    }
                }
                self.ask = Ask::Points;
            }
            Ask::FirstLine | Ask::NextLine => match i {
                Input::Text(t) if !t.is_empty() => {
                    self.lines.push(t);
                    self.ask = Ask::NextLine;
                }
                _ if self.ask == Ask::NextLine => {
                    let lines = std::mem::take(&mut self.lines);
                    return self.finish(s, Annotation::Text(lines));
                }
                _ => self.ask = Ask::Option,
            },
            Ask::Option => match &i {
                Input::Keyword(k) if k == "None" => return self.finish(s, Annotation::None),
                Input::Keyword(k) if k == "Block" => self.ask = Ask::BlockName,
                Input::Keyword(k) if k == "Copy" => self.ask = Ask::Copy,
                Input::Text(_) => s.echo("Invalid option keyword."),
                // Mtext (the default) and Tolerance open an editor or a dialog in AutoCAD; at the
                // command line, as in accoreconsole, they come back to the first line.
                _ => self.ask = Ask::FirstLine,
            },
            Ask::Copy => {
                let picked = match &i {
                    Input::Pick(hs) => hs.first().copied(),
                    Input::Point(p) => crate::select::pick(s.doc()?, &s.state()?.edit_space(), *p, s.settings.pickbox.max(1.0) * s.pixel_size()),
                    _ => None,
                };
                let text = picked.and_then(|h| s.doc().ok()?.entity(h).cloned()).and_then(|e| match &e.kind {
                    EntityKind::Text(t) => Some(MText {
                        insert: t.insert,
                        height: t.height,
                        width: 0.0,
                        attach: 4,
                        rotation: 0.0,
                        style: t.style.clone(),
                        contents: t.value.clone(),
                        line_spacing: 1.0,
                        line_spacing_exact: false,
                        columns: None,
                    }),
                    EntityKind::MText(m) => Some(m.clone()),
                    _ => None,
                });
                match text {
                    Some(m) => return self.finish(s, Annotation::Copy(m)),
                    None if picked.is_some() => s.echo("The object is not text."),
                    None => {}
                }
            }
            Ask::BlockName => {
                let name = match i {
                    Input::Text(t) => t.trim().to_string(),
                    Input::Keyword(k) => k,
                    _ => return Ok(Step::Continue),
                };
                if name == "?" {
                    let names: Vec<String> = s.doc()?.blocks.values().filter(|b| !b.anonymous).map(|b| b.name.clone()).collect();
                    s.echo(format!("Defined blocks: {}", if names.is_empty() { "(none)".into() } else { names.join(", ") }));
                } else if s.doc()?.block(&name).is_none() {
                    s.echo(format!("Block \"{name}\" not found."));
                } else {
                    self.block = name;
                    self.ask = Ask::BlockPoint;
                }
            }
            Ask::BlockPoint => match i {
                Input::Point(p) => {
                    self.block_at = p;
                    self.ask = Ask::BlockX;
                }
                Input::Keyword(_) => s.echo("This option is not available here yet."),
                _ => {}
            },
            Ask::BlockX => match scale(&i) {
                Some(x) => {
                    self.block_scale = Vec2::new(x, x);
                    self.ask = Ask::BlockY;
                }
                None => s.echo("Requires a nonzero number."),
            },
            Ask::BlockY => {
                match scale(&i) {
                    Some(y) if !matches!(i, Input::Enter) => self.block_scale.y = y,
                    Some(_) => {}
                    None => {
                        s.echo("Requires a nonzero number.");
                        return Ok(Step::Continue);
                    }
                }
                self.ask = Ask::BlockRotation;
            }
            Ask::BlockRotation => {
                let rotation = match &i {
                    Input::Enter => 0.0,
                    Input::Text(t) => match s.angle_settings().direction(t) {
                        Some(a) => a,
                        None => {
                            s.echo("Requires a valid angle.");
                            return Ok(Step::Continue);
                        }
                    },
                    _ => return Ok(Step::Continue),
                };
                let a = Annotation::Block { name: self.block.clone(), at: self.block_at, scale: self.block_scale, rotation };
                return self.finish(s, a);
            }
        }
        Ok(Step::Continue)
    }
}

/// A scale factor typed, or 1 for Enter; `None` for zero or anything that isn't a number.
fn scale(i: &Input) -> Option<f64> {
    match i {
        Input::Enter => Some(1.0),
        Input::Text(t) => t.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v != 0.0),
        _ => None,
    }
}

impl LeaderM {
    fn finish(&mut self, s: &mut Session, ann: Annotation) -> Result<Step> {
        let pts = std::mem::take(&mut self.pts);
        add_leader(s, &pts, self.spline, !self.no_arrow, ann)?;
        Ok(Step::Done)
    }
}

/// The dimension settings LEADER draws with, from the current dimension style and DIMSCALE.
struct Sizes {
    arrow: f64,
    gap: f64,
    text: f64,
    above: bool,
    text_style: String,
}

fn sizes(s: &Session) -> Sizes {
    // The current style with the drawing's DIM* overrides (what SETVAR DIMTXT… sets).
    let st = s.doc().ok().and_then(|d| d.dim_style(&style(s)).map(|st| st.with_overrides(&d.dim_overrides()))).unwrap_or_default();
    let k = if st.scale > 0.0 { st.scale } else { 1.0 };
    let text_style = if st.text_style.is_empty() { "Standard".to_string() } else { st.text_style.clone() };
    Sizes { arrow: st.arrow_size * k, gap: st.text_gap.abs() * k, text: st.text_height * k, above: st.text_above != 0, text_style }
}

/// LEADER's JSON form: a straight leader with an arrow through `pts`, and `text` (lines split at
/// `\n`) placed as the command line places it. The handles of the leader and its text.
pub(super) fn json_leader(s: &mut Session, pts: &[Vec2], text: Option<&str>) -> Result<Vec<Handle>> {
    let ann = match text {
        Some(t) if !t.is_empty() => Annotation::Text(t.split('\n').map(str::to_string).collect()),
        _ => Annotation::None,
    };
    add_leader(s, pts, false, true, ann)
}

/// Draw the leader through `pts` and its annotation, as AutoCAD 2024 places them; their handles.
fn add_leader(s: &mut Session, pts: &[Vec2], spline: bool, arrow: bool, ann: Annotation) -> Result<Vec<Handle>> {
    let (Some(&end), Some(&before)) = (pts.last(), pts.len().checked_sub(2).and_then(|i| pts.get(i))) else { return Ok(Vec::new()) };
    let z = sizes(s);
    // The side the leader runs to: right unless its last segment runs left.
    let dir = if end.x < before.x { -1.0 } else { 1.0 };
    let mut vertices = pts.to_vec();
    if let Annotation::Text(_) | Annotation::Copy(_) = ann {
        let d = end - before;
        // A copied text is centred on its line, and with DIMTAD on that line is above the leader:
        // the leader ends half the text height lower.
        if let (Annotation::Copy(_), true, Some(last)) = (&ann, z.above, vertices.last_mut()) {
            last.y -= z.text / 2.0;
        }
        let end = vertices.last().copied().unwrap_or(end);
        // More than 15° off horizontal: a hook line DIMASZ long into the end.
        if !spline && d.y.abs() > d.x.abs() * 15f64.to_radians().tan() {
            let at = vertices.len() - 1;
            vertices.insert(at, Vec2::new(end.x - dir * z.arrow, end.y));
        }
    }
    let leader = s.add_entity(EntityKind::Leader(cadcraft_doc::Leader {
        vertices: vertices.iter().map(|p| v3(*p)).collect(),
        arrow,
        spline,
        style: style(s),
    }))?;
    let annotation = match ann {
        Annotation::Text(lines) => {
            let (attach, lift) = match (z.above, dir > 0.0) {
                (true, true) => (7, z.gap),
                (true, false) => (9, z.gap),
                (false, true) => (4, 0.0),
                (false, false) => (6, 0.0),
            };
            Some(s.add_entity(EntityKind::MText(MText {
                insert: v3(end + Vec2::new(dir * z.gap, lift)),
                height: z.text,
                width: 0.0,
                attach,
                rotation: 0.0,
                style: z.text_style,
                contents: lines.join("\\P"),
                line_spacing: 1.0,
                line_spacing_exact: false,
                columns: None,
            }))?)
        }
        Annotation::Copy(m) => {
            let lift = if z.above { z.gap } else { 0.0 };
            let attach = if dir > 0.0 { 4 } else { 6 };
            Some(s.add_entity(EntityKind::MText(MText { insert: v3(end + Vec2::new(dir * z.gap, lift)), attach, rotation: 0.0, ..m }))?)
        }
        Annotation::Block { name, at, scale, rotation } => {
            match crate::cmd::blocks::insert_with(s, &name, at, scale, rotation, None, &serde_json::Map::new()) {
                Ok(h) => Some(h),
                Err(e) => {
                    s.doc_mut()?.remove_entity(leader);
                    return Err(e);
                }
            }
        }
        Annotation::None => None,
    };
    Ok(std::iter::once(leader).chain(annotation).collect())
}
