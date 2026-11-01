//! Hot grips: what happens once a grip is clicked. AutoCAD's grip modes STRETCH, MOVE, ROTATE,
//! SCALE and MIRROR (Space/Enter cycles them) with their options: Base point (pick another base),
//! Copy (keep editing copies until Enter or eXit), Undo (take back the last copy), Reference
//! (ROTATE and SCALE) and eXit. Several grips can be hot at once; STRETCH moves them together.
//!
//! The UI starts it with [`Session::start_grip_edit`]; typed, `GRIP` asks for the grips first.
//! The whole grip edit, copies included, is one undo step.

use std::sync::Arc;

use cadcraft_doc::{Drawing, EntityKind, Handle};
use cadcraft_geom::{Mat3, Vec2};

use super::gripcmds::stretch_grips;
use super::helpers::line;
use super::machines::number;
use super::modify::transform_entities;
use crate::grips::{GripAction, GripMode, grip_action, grip_actions, mode_matrix};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Running, Session, Step};

/// At most this many grips are hot at once (input-derived: a cap on the work per edit).
const MAX_HOT: usize = 1000;

/// What the prompt asks for besides the mode's own point.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ask {
    /// The mode prompt ("** STRETCH ** Specify stretch point…").
    Mode,
    /// Base point option.
    Base,
    /// Reference: the reference angle/length, typed or as a first point.
    RefFirst,
    /// Reference: the second point of the reference angle/length.
    RefSecond(Vec2),
    /// Reference: the new angle/length (the reference value is known).
    RefNew(f64),
    /// A multifunctional grip option that takes a point or value (Add Vertex, Lengthen…).
    Action(GripAction),
}

pub(crate) struct GripEdit {
    /// The hot grips (object, grip index); the first is the base grip.
    hot: Vec<(Handle, usize)>,
    /// Typed `GRIP`: still collecting grips ("Select grips").
    picking: bool,
    /// What MOVE/ROTATE/SCALE/MIRROR transform: the hot grips' objects and the selection.
    targets: Vec<Handle>,
    base: Vec2,
    mode: GripMode,
    copy: bool,
    ask: Ask,
    /// The drawing before each edit made in Copy mode (Undo).
    undo: Vec<Arc<Drawing>>,
}

/// The `GRIP` command's machine: select grips, then edit them.
pub(crate) fn factory(s: &Session) -> Result<Box<dyn Interactive>> {
    let mut g = GripEdit::new(s, Vec::new())?;
    g.picking = true;
    Ok(Box::new(g))
}

fn other(msg: &str) -> EngineError {
    EngineError::Other(msg.into())
}

/// A finite length greater than zero.
fn positive(v: Option<f64>) -> Result<f64> {
    v.filter(|f| f.is_finite() && *f > 1e-12).ok_or_else(|| other("Requires a positive number."))
}

impl GripEdit {
    fn new(s: &Session, hot: Vec<(Handle, usize)>) -> Result<Self> {
        let mut g = GripEdit {
            hot: Vec::new(),
            picking: false,
            targets: Vec::new(),
            base: Vec2::ZERO,
            mode: GripMode::Stretch,
            copy: false,
            ask: Ask::Mode,
            undo: Vec::new(),
        };
        for h in hot {
            g.add_hot(s, h)?;
        }
        Ok(g)
    }

    /// Make grip `index` of `h` hot (the first one becomes the base point).
    fn add_hot(&mut self, s: &Session, (h, i): (Handle, usize)) -> Result<()> {
        if self.hot.contains(&(h, i)) || self.hot.len() >= MAX_HOT {
            return Ok(());
        }
        let p = s.grips_of(h)?.get(i).copied().ok_or_else(|| other("no such grip"))?;
        if self.hot.is_empty() {
            self.base = p;
            self.targets.push(h);
            self.targets.extend(s.selection().into_iter().filter(|x| *x != h));
        } else if !self.targets.contains(&h) {
            self.targets.push(h);
        }
        self.hot.push((h, i));
        Ok(())
    }

    /// The grips of the selected objects at `p` (typed `GRIP`): all objects with a grip there.
    fn grips_at(s: &Session, p: Vec2) -> Vec<(Handle, usize)> {
        let tol = (s.pixel_size() * (s.settings.gripsize + 3.0)).max(1e-9 * (1.0 + p.len()));
        let Ok(d) = s.doc() else { return Vec::new() };
        let mut out = Vec::new();
        for h in s.selection().into_iter().take(MAX_HOT) {
            if super::curves::is_locked(s, h) {
                continue;
            }
            let Some(e) = d.entity(h) else { continue };
            let gs = e.kind.grips();
            // The nearest grip of this object within the tolerance.
            let best = gs
                .iter()
                .enumerate()
                .filter(|(_, g)| (g.x - p.x).abs() <= tol && (g.y - p.y).abs() <= tol)
                .min_by(|a, b| a.1.dist(p).partial_cmp(&b.1.dist(p)).unwrap_or(std::cmp::Ordering::Equal));
            if let Some((i, _)) = best {
                out.push((h, i));
            }
        }
        out
    }

    fn label(&self) -> &'static str {
        match self.mode {
            GripMode::Stretch => "** STRETCH **",
            GripMode::Move => "** MOVE **",
            GripMode::Rotate => "** ROTATE **",
            GripMode::Scale => "** SCALE **",
            GripMode::Mirror => "** MIRROR **",
        }
    }

    fn next_mode(&mut self) {
        self.mode = match self.mode {
            GripMode::Stretch => GripMode::Move,
            GripMode::Move => GripMode::Rotate,
            GripMode::Rotate => GripMode::Scale,
            GripMode::Scale => GripMode::Mirror,
            GripMode::Mirror => GripMode::Stretch,
        };
    }

    /// The transform of the current mode for the point `to` (not STRETCH).
    fn matrix(&self, to: Vec2) -> Option<Mat3> {
        match self.ask {
            Ask::RefNew(r) if self.mode == GripMode::Rotate => Some(Mat3::rotate_about(self.base, self.base.angle_to(to) - r)),
            Ask::RefNew(r) if self.mode == GripMode::Scale => {
                let f = self.base.dist(to) / r;
                (f.is_finite() && f > 1e-12).then(|| Mat3::scale_about(self.base, f))
            }
            _ => mode_matrix(self.mode, self.base, to),
        }
    }

    /// The multifunctional options of the hot grip (one hot grip, in STRETCH mode).
    fn actions(&self, s: &Session) -> Vec<GripAction> {
        match self.hot.as_slice() {
            [(h, i)] if self.mode == GripMode::Stretch => {
                s.doc().ok().and_then(|d| d.entity(*h)).map(|e| grip_actions(&e.kind, *i)).unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }

    /// The object after the multifunctional `action` with the point `to` or value `value`.
    fn acted(&self, s: &Session, action: GripAction, to: Option<Vec2>, value: Option<f64>) -> Option<(Handle, EntityKind)> {
        let &(h, i) = self.hot.first()?;
        let e = s.doc().ok()?.entity(h)?;
        grip_action(&e.kind, i, action, to, value).map(|k| (h, k))
    }

    /// Apply a multifunctional option; the grip edit ends.
    fn act(&mut self, s: &mut Session, action: GripAction, to: Option<Vec2>, value: Option<f64>) -> Result<Step> {
        if self.hot.first().is_some_and(|(h, _)| super::curves::is_locked(s, *h)) {
            return Err(other("The object is on a locked layer."));
        }
        let (h, k) = self.acted(s, action, to, value).ok_or_else(|| other("That would make a degenerate object."))?;
        s.doc_mut()?.modify_entity(h, |e| e.kind = k)?;
        Ok(Step::Done)
    }

    fn action_input(&mut self, s: &mut Session, action: GripAction, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => self.act(s, action, Some(p), None),
            Input::Deferred(d) => self.act(s, action, Some(d.at), None),
            Input::Text(t) if matches!(action, GripAction::Lengthen | GripAction::Radius) => {
                let v = number(&t).filter(|v| v.is_finite()).ok_or_else(|| other("Requires a number or point."))?;
                self.act(s, action, None, Some(v))
            }
            Input::Keyword(k) if k == "eXit" => Ok(Step::Done),
            // Enter goes back to STRETCH.
            Input::Enter => {
                self.ask = Ask::Mode;
                s.echo(self.label());
                Ok(Step::Continue)
            }
            _ => Err(other("Requires a point.")),
        }
    }

    /// Edit with the dragged point `to` (STRETCH) or the transform `m` (the other modes).
    fn edit(&mut self, s: &mut Session, to: Vec2, m: Option<Mat3>) -> Result<Step> {
        let before = s.state()?.doc.clone();
        let changed = match (self.mode, m) {
            (GripMode::Stretch, _) => !stretch_grips(s, &self.hot, &self.targets, to - self.base, self.copy)?.is_empty(),
            (_, Some(m)) => !transform_entities(s, &self.targets, &m, self.copy)?.is_empty(),
            (_, None) => return Err(other("Invalid point.")),
        };
        if !changed {
            return Err(other("The objects can't be edited that way (locked, or the result would be degenerate)."));
        }
        self.ask = Ask::Mode;
        if self.copy {
            self.undo.push(before);
            Ok(Step::Continue)
        } else {
            Ok(Step::Done)
        }
    }

    fn mode_input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Keyword(k) => match k.as_str() {
                "Base point" => {
                    self.ask = Ask::Base;
                    Ok(Step::Continue)
                }
                "Copy" => {
                    self.copy = true;
                    Ok(Step::Continue)
                }
                "Undo" => match self.undo.pop() {
                    Some(doc) => {
                        // Settings changed meanwhile stay.
                        let header = s.doc()?.header.clone();
                        s.state_mut()?.doc = doc;
                        s.doc_mut()?.header = header;
                        Ok(Step::Continue)
                    }
                    None => Err(other("Nothing to undo.")),
                },
                "Reference" if matches!(self.mode, GripMode::Rotate | GripMode::Scale) => {
                    self.ask = Ask::RefFirst;
                    Ok(Step::Continue)
                }
                "eXit" => Ok(Step::Done),
                other_kw => match self.actions(s).into_iter().find(|a| a.keyword() == other_kw) {
                    Some(a) if a.needs_point() => {
                        self.ask = Ask::Action(a);
                        s.echo(a.title());
                        Ok(Step::Continue)
                    }
                    Some(a) => self.act(s, a, None, None),
                    None => Ok(Step::Continue),
                },
            },
            // Enter cycles the modes; after copies it ends the edit.
            Input::Enter => {
                if !self.undo.is_empty() {
                    return Ok(Step::Done);
                }
                self.next_mode();
                s.echo(self.label());
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                let m = self.matrix(p);
                self.edit(s, p, m)
            }
            Input::Deferred(d) => {
                let m = self.matrix(d.at);
                self.edit(s, d.at, m)
            }
            Input::Text(t) => match self.mode {
                GripMode::Rotate => {
                    let a = s.angle_settings().rotation(&t).filter(|a| a.is_finite()).ok_or_else(|| other("Requires an angle or point."))?;
                    self.edit(s, self.base, Some(Mat3::rotate_about(self.base, a)))
                }
                GripMode::Scale => {
                    let f = positive(number(&t))?;
                    self.edit(s, self.base, Some(Mat3::scale_about(self.base, f)))
                }
                _ => Err(other("Requires a point or an option keyword.")),
            },
            Input::Pick(_) | Input::Cancel => Ok(Step::Continue),
        }
    }

    /// Reference (ROTATE: angles, SCALE: lengths): the reference value typed or from two points,
    /// then the new value typed or measured from the base point to a picked point.
    fn reference_input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let rotate = self.mode == GripMode::Rotate;
        let au = s.angle_settings();
        let value = |t: &str| -> Result<f64> {
            if rotate { au.direction(t).filter(|a| a.is_finite()).ok_or_else(|| other("Requires an angle or point.")) } else { positive(number(t)) }
        };
        let between = |a: Vec2, b: Vec2| -> Result<f64> {
            if a.near(b, 1e-12) {
                return Err(other("The two points must differ."));
            }
            Ok(if rotate { a.angle_to(b) } else { a.dist(b) })
        };
        match (self.ask, i) {
            (Ask::RefFirst, Input::Point(p)) => self.ask = Ask::RefSecond(p),
            (Ask::RefFirst, Input::Text(t)) => self.ask = Ask::RefNew(value(&t)?),
            (Ask::RefFirst, Input::Enter) => self.ask = Ask::RefNew(if rotate { 0.0 } else { 1.0 }),
            (Ask::RefSecond(a), Input::Point(p)) => self.ask = Ask::RefNew(between(a, p)?),
            (Ask::RefNew(_), Input::Point(p)) => {
                let m = self.matrix(p).ok_or_else(|| other("Requires a positive length."))?;
                return self.edit(s, p, Some(m));
            }
            (Ask::RefNew(r), Input::Text(t)) => {
                let v = value(&t)?;
                let m = if rotate { Mat3::rotate_about(self.base, v - r) } else { Mat3::scale_about(self.base, positive(Some(v / r))?) };
                return self.edit(s, self.base, Some(m));
            }
            (_, Input::Enter) => self.ask = Ask::Mode,
            _ => {}
        }
        Ok(Step::Continue)
    }
}

impl Interactive for GripEdit {
    fn name(&self) -> &'static str {
        "GRIP"
    }

    fn begin(&mut self, _s: &mut Session) -> Result<Step> {
        Ok(Step::Continue)
    }

    fn prompt(&self, s: &Session) -> Prompt {
        if self.picking {
            let msg = if self.hot.is_empty() { "Select grip" } else { "Select grip or <edit the hot grips>" };
            return Prompt::new(msg, Accept::POINT);
        }
        let base = self.base;
        match self.ask {
            Ask::Base => Prompt::new("Specify base point", Accept::POINT),
            Ask::RefFirst if self.mode == GripMode::Rotate => Prompt::new("Specify reference angle", Accept::POINT_OR_NUMBER).default("0"),
            Ask::RefFirst => Prompt::new("Specify reference length", Accept::POINT_OR_NUMBER).default("1"),
            Ask::RefSecond(a) => Prompt::new("Specify second point", Accept::POINT).base(a),
            Ask::RefNew(_) if self.mode == GripMode::Rotate => Prompt::new("Specify new angle", Accept::POINT_OR_NUMBER).base(base),
            Ask::RefNew(_) => Prompt::new("Specify new length", Accept::POINT_OR_NUMBER).base(base),
            Ask::Action(a) => {
                let (msg, accept) = match a {
                    GripAction::AddVertex => ("Specify new vertex point", Accept::POINT),
                    GripAction::ToArc => ("Specify a point on the arc", Accept::POINT),
                    GripAction::Lengthen => ("Specify end point or length change", Accept::POINT_OR_NUMBER),
                    GripAction::Radius => ("Specify radius", Accept::POINT_OR_NUMBER),
                    GripAction::RemoveVertex | GripAction::ToLine => ("", Accept::POINT),
                };
                Prompt::new(format!("{} {msg}", a.title()), accept).kw(&["eXit"]).base(base)
            }
            Ask::Mode => {
                let (msg, accept, kw): (&str, Accept, &[&str]) = match self.mode {
                    GripMode::Stretch => ("** STRETCH ** Specify stretch point", Accept::POINT, &["Base point", "Copy", "Undo", "eXit"]),
                    GripMode::Move => ("** MOVE ** Specify move point", Accept::POINT, &["Base point", "Copy", "Undo", "eXit"]),
                    GripMode::Rotate => {
                        ("** ROTATE ** Specify rotation angle", Accept::POINT_OR_NUMBER, &["Base point", "Copy", "Undo", "Reference", "eXit"])
                    }
                    GripMode::Scale => {
                        ("** SCALE ** Specify scale factor", Accept::POINT_OR_NUMBER, &["Base point", "Copy", "Undo", "Reference", "eXit"])
                    }
                    GripMode::Mirror => ("** MIRROR ** Specify second point", Accept::POINT, &["Base point", "Copy", "Undo", "eXit"]),
                };
                let mut p = Prompt::new(msg, accept).kw(kw).base(base);
                // A single hot grip's multifunctional options (Add vertex, Lengthen…) before eXit.
                let at = p.keywords.len().saturating_sub(1);
                for (n, a) in self.actions(s).into_iter().enumerate() {
                    p.keywords.insert(at + n, a.keyword().to_string());
                }
                // One hot grip being stretched: its dimensions can be typed (grip dimensional input).
                match self.hot.as_slice() {
                    [g] if self.mode == GripMode::Stretch => p.grip(*g),
                    _ => p,
                }
            }
        }
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.picking {
            return match i {
                Input::Point(p) => {
                    let found = Self::grips_at(s, p);
                    if found.is_empty() {
                        return Err(other("No grip of a selected object there."));
                    }
                    for g in found {
                        self.add_hot(s, g)?;
                    }
                    Ok(Step::Continue)
                }
                Input::Enter if !self.hot.is_empty() => {
                    self.picking = false;
                    s.echo(self.label());
                    Ok(Step::Continue)
                }
                Input::Enter => Ok(Step::Done),
                _ => Err(other("Requires a point on a grip.")),
            };
        }
        match self.ask {
            Ask::Mode => self.mode_input(s, i),
            Ask::Base => match i {
                Input::Point(p) => {
                    self.base = p;
                    self.ask = Ask::Mode;
                    Ok(Step::Continue)
                }
                Input::Enter => {
                    self.ask = Ask::Mode;
                    Ok(Step::Continue)
                }
                _ => Err(other("Requires a point.")),
            },
            Ask::Action(a) => self.action_input(s, a, i),
            _ => self.reference_input(s, i),
        }
    }

    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.picking {
            return Vec::new();
        }
        let base = match self.ask {
            Ask::Action(a) => {
                let mut out: Vec<EntityKind> = self.acted(s, a, Some(c), None).map(|(_, k)| k).into_iter().collect();
                out.push(line(self.base, c));
                return out;
            }
            Ask::Mode | Ask::RefNew(_) => self.base,
            Ask::RefSecond(a) => return vec![line(a, c)],
            Ask::Base | Ask::RefFirst => return Vec::new(),
        };
        let Ok(d) = s.doc() else { return Vec::new() };
        let mut out: Vec<EntityKind> = if self.mode == GripMode::Stretch {
            super::gripcmds::stretched(s, &self.hot, &self.targets, c - base).into_iter().take(500).map(|(_, k)| k).collect()
        } else {
            match self.matrix(c) {
                Some(m) => self
                    .targets
                    .iter()
                    .take(500)
                    .filter_map(|h| d.entity(*h))
                    .map(|e| {
                        let mut k = e.kind.clone();
                        k.transform(&m);
                        k
                    })
                    .collect(),
                None => Vec::new(),
            }
        };
        out.push(line(base, c));
        out
    }
}

impl Session {
    /// Make grips hot and start editing them (the UI's click on a grip; Shift+click collects
    /// several first). `hot` lists (object, grip index); the first is the base grip. The grip
    /// modes and their options then run at the command line like any prompt.
    pub fn start_grip_edit(&mut self, hot: &[(Handle, usize)]) -> Result<()> {
        if self.running.is_some() {
            self.cancel();
        }
        if hot.is_empty() {
            return Err(other("no grip"));
        }
        let g = GripEdit::new(self, hot.to_vec())?;
        let label = g.label();
        let machine = crate::guard::Guarded::wrap(Box::new(g));
        let st = self.state()?;
        let (before, selection_before) = (st.doc.clone(), st.selection.clone());
        self.running = Some(Running { id: "grip".into(), machine, before, selection_before });
        self.pending_window = None;
        self.point_mods.clear();
        self.echo(label);
        Ok(())
    }
}
