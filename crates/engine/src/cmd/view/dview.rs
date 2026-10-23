//! DVIEW: the dynamic view, on CADCraft's 2D plan views.
//!
//! It starts with "Select objects or <use DVIEWBLOCK>:": the objects
//! picked are the preview set, and Enter with nothing picked previews the DVIEWBLOCK block (the
//! drawing's own block of that name, else a built-in sample house). While DVIEW runs only the
//! preview set is drawn ([`Interactive::display_only`]); the whole drawing is regenerated when it
//! ends. Then it repeats "Enter option [CAmera/TArget/Distance/POints/PAn/Zoom/TWist/CLip/Hide/
//! Off/Undo]:" until Enter. TWist, PAn, Zoom, Off and Undo work; the options that need a 3D
//! camera say they are not available yet and stay at the option prompt.
//!
//! DVIEW works on the Model tab's view, or on the active (double-clicked) layout viewport; on a
//! layout with no active viewport it is not available. A Model tab twist is a view change like
//! ZOOM (view history, not undo). A layout viewport's twist belongs to the viewport entity, so a
//! DVIEW that changed it is one undo step.

use std::f64::consts::TAU;
use std::sync::Arc;

use cadcraft_doc::{Entity, EntityKind, Handle, Insert, Line, Space};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use super::super::machines::{SelOutcome, SelectPhase, number};
use super::super::*;
use crate::{Accept, DisplayOnly, EngineError, Input, Interactive, Prompt, Result, Session, Snapshot, Step, View};

/// What DVIEW (and a view twist asked for anywhere) says on a layout with no active viewport.
pub(super) const PAPER_SPACE: &str = "This command is not available in Paper space.";

/// The option prompt's keywords, in prompt order.
const OPTIONS: &[&str] = &["CAmera", "TArget", "Distance", "POints", "PAn", "Zoom", "TWist", "CLip", "Hide", "Off", "Undo"];

/// The block DVIEW previews when nothing is selected.
const DVIEWBLOCK: &str = "DVIEWBLOCK";

pub(super) fn spec() -> CommandSpec {
    CommandSpec::new("dview", "DView", run_dview)
        .alias(&["dv"])
        .params("{twist: degrees counterclockwise} (DVIEW TWist: the Model tab's view twist, or the active layout viewport's, an undoable edit; not in Paper space)")
        .noundo()
        .interactive(|s| {
            // Refused as it starts on a layout with no active viewport.
            target(s)?;
            Ok(Box::new(DviewM::default()))
        })
}

/// The view DVIEW changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Target {
    /// The Model tab's view.
    Model,
    /// The active (MSPACE) layout viewport: its own view twist.
    Viewport(Handle),
}

/// The view DVIEW changes: the active layout viewport, else the Model tab's view. A layout with
/// no active viewport has none.
pub(super) fn target(s: &Session) -> Result<Target> {
    let st = s.state()?;
    if let Some((h, _)) = st.active_viewport() {
        return Ok(Target::Viewport(h));
    }
    if st.space == Space::Model {
        return Ok(Target::Model);
    }
    Err(EngineError::Other(PAPER_SPACE.into()))
}

/// A twist kept as files hold it: [0, 2π).
fn normal(twist: f64) -> f64 {
    twist.rem_euclid(TAU)
}

/// The active viewport's twist, if it is `h`.
fn viewport_twist(s: &Session, h: Handle) -> Option<f64> {
    s.state().ok()?.active_viewport().filter(|(vh, _)| *vh == h).map(|(_, vp)| vp.view_twist())
}

/// Turn `target`'s view to `twist` radians. The Model tab's twist is a view change (view
/// history, like ZOOM); a viewport's is a change to the viewport entity, which the caller
/// records as an undo step ([`record_viewport_edit`]).
pub(super) fn set_twist(s: &mut Session, target: Target, twist: f64) -> Result<()> {
    if !twist.is_finite() {
        return Err(bad("dview", "the twist must be a finite angle"));
    }
    let twist = normal(twist);
    match target {
        Target::Model => {
            let st = s.state_mut()?;
            let v = st.view();
            st.set_view(View { twist, ..v });
        }
        Target::Viewport(h) => {
            if viewport_twist(s, h) == Some(twist) {
                return Ok(());
            }
            let space = s.state()?.space.clone();
            let d = s.doc_mut()?;
            if let Some(store) = d.space_mut(&space) {
                store.modify(h, |e| {
                    if let EntityKind::Viewport(vp) = &mut e.kind {
                        vp.twist = twist;
                    }
                });
            }
        }
    }
    Ok(())
}

/// Record a layout viewport's twist change as one undo step, `before` being the drawing (and
/// selection) before it. Nothing when the twist did not change, and nothing inside a LISP routine
/// (its own single step holds it).
pub(super) fn record_viewport_edit(s: &mut Session, before: (Arc<cadcraft_doc::Drawing>, Vec<Handle>), h: Handle) -> Result<()> {
    let old = before.0.entity(h).and_then(|e| match &e.kind {
        EntityKind::Viewport(vp) => Some(vp.view_twist()),
        _ => None,
    });
    if old == viewport_twist(s, h) || s.lisp_groups_undo() {
        return Ok(());
    }
    let st = s.state_mut()?;
    if !Arc::ptr_eq(&before.0, &st.doc) {
        st.push_undo(Snapshot { label: "DView".into(), doc: before.0, selection: before.1 });
        st.revision += 1;
    }
    Ok(())
}

/// The drawing and selection now, for [`record_viewport_edit`].
fn snapshot(s: &Session) -> Result<(Arc<cadcraft_doc::Drawing>, Vec<Handle>)> {
    let st = s.state()?;
    Ok((st.doc.clone(), st.selection.clone()))
}

fn run_dview(s: &mut Session, p: &Value) -> Result<Value> {
    let t = target(s)?;
    let deg = f64_req("dview", p, "twist")?;
    let before = snapshot(s)?;
    set_twist(s, t, deg.to_radians())?;
    if let Target::Viewport(h) = t {
        record_viewport_edit(s, before, h)?;
    }
    let v = s.state()?.view();
    Ok(json!({ "twist": v.twist.to_degrees() }))
}

/// The built-in DVIEWBLOCK: a house front in the unit square, origin at the lower-left corner
/// (the size and origin a DVIEWBLOCK of one's own is expected to have). CADCraft's own drawing.
fn sample_house() -> Vec<(Vec2, Vec2)> {
    let p = Vec2::new;
    let mut lines = Vec::new();
    let mut poly = |pts: &[Vec2], closed: bool| {
        for w in pts.windows(2) {
            lines.push((w[0], w[1]));
        }
        if closed && let (Some(a), Some(b)) = (pts.last(), pts.first()) {
            lines.push((*a, *b));
        }
    };
    // Walls.
    poly(&[p(0.1, 0.0), p(0.9, 0.0), p(0.9, 0.6), p(0.1, 0.6)], true);
    // Roof with eaves, and a chimney on its right slope.
    poly(&[p(0.0, 0.55), p(0.5, 1.0), p(1.0, 0.55)], false);
    poly(&[p(0.7, 0.82), p(0.7, 0.95), p(0.8, 0.95), p(0.8, 0.73)], false);
    // Door.
    poly(&[p(0.42, 0.0), p(0.42, 0.35), p(0.58, 0.35), p(0.58, 0.0)], false);
    // Two windows with their bars.
    for x in [0.17, 0.67] {
        poly(&[p(x, 0.25), p(x + 0.16, 0.25), p(x + 0.16, 0.45), p(x, 0.45)], true);
        poly(&[p(x + 0.08, 0.25), p(x + 0.08, 0.45)], false);
        poly(&[p(x, 0.35), p(x + 0.16, 0.35)], false);
    }
    lines
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Stage {
    #[default]
    Select,
    Option,
    Twist,
    PanBase,
    PanSecond(Vec2),
    Zoom,
}

/// DVIEW, interactive.
#[derive(Default)]
struct DviewM {
    stage: Stage,
    sel: SelectPhase,
    /// The preview set; empty with `sample` set: the DVIEWBLOCK.
    preview: Vec<Handle>,
    /// Where the DVIEWBLOCK preview stands: its lower-left corner and size (drawing units), fixed
    /// as the options start so that PAn and Zoom move the view, not the house.
    sample: Option<(Vec2, f64)>,
    /// The views before each DVIEW action, for Undo.
    undo: Vec<View>,
    /// What DVIEW works on, and the drawing as it started (a viewport's twist change is one
    /// undo step).
    target: Option<Target>,
    before: Option<(Arc<cadcraft_doc::Drawing>, Vec<Handle>)>,
}

impl DviewM {
    fn target(&self, s: &Session) -> Result<Target> {
        match self.target {
            Some(t) => Ok(t),
            None => target(s),
        }
    }
    /// Start the options with this preview set (empty: the DVIEWBLOCK).
    fn options(&mut self, s: &mut Session, picked: Vec<Handle>) -> Result<Step> {
        // Objects picked at the prompt don't stay selected.
        s.set_selection(Vec::new());
        if picked.is_empty() {
            let v = s.state()?.view();
            let size = v.height * 0.5;
            self.sample = Some((v.center - Vec2::new(size, size) * 0.5, size));
        }
        self.preview = picked;
        self.stage = Stage::Option;
        Ok(Step::Continue)
    }
    /// Show view `v` (centre, height and twist) on the target.
    fn show(&self, s: &mut Session, v: View) -> Result<()> {
        let t = self.target(s)?;
        let before = s.state()?.doc.clone();
        let twist = normal(v.twist);
        if let Target::Viewport(_) = t {
            // The twist is the viewport's; the view inside it then moves as asked.
            set_twist(s, t, twist)?;
        }
        if !s.state_mut()?.set_view(View { twist, ..v }) {
            s.state_mut()?.doc = before;
            return Err(EngineError::Other("The view is too large to show.".into()));
        }
        Ok(())
    }
    /// Apply a view as one DVIEW action (Undo takes it back).
    fn change(&mut self, s: &mut Session, f: impl FnOnce(View) -> View) -> Result<()> {
        let v = s.state()?.view();
        self.show(s, f(v))?;
        self.undo.push(v);
        Ok(())
    }
    /// The command ends: a viewport twist change becomes one undo step, and the whole drawing is
    /// regenerated.
    fn end(&mut self, s: &mut Session) -> Result<()> {
        if let (Some(Target::Viewport(h)), Some(before)) = (self.target, self.before.take()) {
            record_viewport_edit(s, before, h)?;
        }
        s.touch();
        Ok(())
    }
}

impl Interactive for DviewM {
    fn name(&self) -> &'static str {
        "DVIEW"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.target = Some(target(s)?);
        self.before = Some(snapshot(s)?);
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match self.stage {
            Stage::Select if self.sel.removing => self.sel.prompt(),
            Stage::Select => Prompt::new("Select objects or", Accept::SELECT).default("use DVIEWBLOCK"),
            Stage::Option => Prompt::new("Enter option", Accept::TEXT).kw(OPTIONS).options_only(),
            Stage::Twist => {
                let (au, ap) = s.doc().map(|d| (d.header.i64("AUNITS", 0), d.header.i64("AUPREC", 0))).unwrap_or((0, 0));
                let cur = s.state().map(|st| st.view().twist).unwrap_or(0.0);
                // Offered as it would be typed: clockwise is positive when ANGDIR is 1.
                let cur = if s.angle_settings().clockwise { -cur } else { cur };
                let center = s.state().map(|st| st.view().center).ok();
                Prompt::new("Specify view twist angle", Accept::POINT_OR_NUMBER).default(crate::units::format_angle(cur, au, ap)).base_opt(center)
            }
            Stage::PanBase => Prompt::new("Specify displacement base point", Accept::POINT),
            Stage::PanSecond(b) => Prompt::new("Specify second point", Accept::POINT).base(b),
            Stage::Zoom => Prompt::new("Specify zoom scale factor", Accept::NUMBER).default("1"),
        }
    }
    fn cancelled(&mut self, s: &mut Session) {
        if let Err(e) = self.end(s) {
            s.echo(e.to_string());
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.stage {
            Stage::Select => match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => self.options(s, Vec::new()),
                SelOutcome::Done(hs) => self.options(s, hs),
            },
            Stage::Option => {
                let word = match &i {
                    Input::Enter => {
                        self.end(s)?;
                        return Ok(Step::Done);
                    }
                    Input::Keyword(t) | Input::Text(t) => self.prompt(s).match_keyword(t),
                    _ => None,
                };
                match word.as_deref() {
                    Some("TWist") => self.stage = Stage::Twist,
                    Some("PAn") => self.stage = Stage::PanBase,
                    Some("Zoom") => self.stage = Stage::Zoom,
                    // Perspective is never on in a 2D view: there is nothing to turn off.
                    Some("Off") => {}
                    Some("Undo") => {
                        if let Some(v) = self.undo.pop() {
                            self.show(s, v)?;
                        }
                    }
                    // CAmera, TArget, Distance, POints, CLip and Hide need a 3D view.
                    Some(other) => s.echo(format!("DVIEW {other} is not available yet.")),
                    None => return Err(EngineError::Other("Invalid option keyword.".into())),
                }
                Ok(Step::Continue)
            }
            Stage::Twist => {
                let twist = match &i {
                    Input::Enter => None,
                    Input::Point(p) => {
                        let c = s.state()?.view().center;
                        let d = *p - c;
                        if d.len() < 1e-12 {
                            return Err(EngineError::Other("Requires a valid angle.".into()));
                        }
                        Some(d.y.atan2(d.x))
                    }
                    Input::Text(t) | Input::Keyword(t) if t.trim().is_empty() => None,
                    Input::Text(t) | Input::Keyword(t) => {
                        Some(s.angle_settings().rotation(t).ok_or_else(|| EngineError::Other("Requires a valid angle.".into()))?)
                    }
                    _ => return Ok(Step::Continue),
                };
                if let Some(a) = twist {
                    self.change(s, |v| View { twist: normal(a), ..v })?;
                }
                self.stage = Stage::Option;
                Ok(Step::Continue)
            }
            Stage::PanBase => match i {
                Input::Point(p) => {
                    self.stage = Stage::PanSecond(p);
                    Ok(Step::Continue)
                }
                Input::Enter => {
                    self.stage = Stage::Option;
                    Ok(Step::Continue)
                }
                _ => Err(EngineError::Other("Invalid point.".into())),
            },
            Stage::PanSecond(b) => match i {
                Input::Point(p) => {
                    self.change(s, |v| View { center: v.center - (p - b), ..v })?;
                    self.stage = Stage::Option;
                    Ok(Step::Continue)
                }
                Input::Enter => {
                    self.stage = Stage::Option;
                    Ok(Step::Continue)
                }
                _ => Err(EngineError::Other("Invalid point.".into())),
            },
            Stage::Zoom => {
                let f = match &i {
                    Input::Enter => None,
                    Input::Text(t) | Input::Keyword(t) if t.trim().is_empty() => None,
                    Input::Text(t) | Input::Keyword(t) => match number(t) {
                        Some(f) if f.is_finite() && f > 0.0 => Some(f),
                        _ => return Err(EngineError::Other("Requires a positive number.".into())),
                    },
                    _ => return Ok(Step::Continue),
                };
                if let Some(f) = f.filter(|f| *f != 1.0) {
                    self.change(s, |v| View { height: v.height / f, ..v })?;
                }
                self.stage = Stage::Option;
                Ok(Step::Continue)
            }
        }
    }
    fn display_only(&self, s: &Session) -> Option<DisplayOnly> {
        if self.stage == Stage::Select {
            return None;
        }
        let mut extra = Vec::new();
        if let Some((corner, size)) = self.sample {
            let common = s.current_common();
            let d = s.doc().ok()?;
            if d.block(DVIEWBLOCK).is_some() {
                // The drawing's own DVIEWBLOCK, defined in a 1 by 1 unit area.
                let ins = Insert {
                    block: DVIEWBLOCK.into(),
                    insert: corner.to3(0.0),
                    scale: Vec3::new(size, size, size),
                    rotation: 0.0,
                    attribs: Vec::new(),
                    cols: 1,
                    rows: 1,
                    col_spacing: 0.0,
                    row_spacing: 0.0,
                };
                extra.push(Entity { handle: Handle(0), common, kind: EntityKind::Insert(ins) });
            } else {
                extra.extend(sample_house().into_iter().map(|(a, b)| Entity {
                    handle: Handle(0),
                    common: common.clone(),
                    kind: EntityKind::Line(Line { a: (corner + a * size).to3(0.0), b: (corner + b * size).to3(0.0) }),
                }));
            }
        }
        Some(DisplayOnly { handles: self.preview.clone(), extra })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_house_fits_the_unit_square() {
        let lines = sample_house();
        assert!(lines.len() > 10);
        for (a, b) in lines {
            for q in [a, b] {
                assert!((0.0..=1.0).contains(&q.x) && (0.0..=1.0).contains(&q.y), "{q:?}");
            }
        }
    }
}
