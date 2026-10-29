//! HATCHPICK: the Hatch and Gradient dialog's picks (internal points, boundary objects, a pattern
//! origin, the hatches to edit, the hatch whose properties to inherit), kept in the session for
//! the dialog to take, and its Preview (the hatch OK would make, shown until a pick or Enter).
//! It never changes the drawing.

use cadcraft_doc::{EntityKind, Handle, HatchLoop};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};
use std::sync::Arc;

use super::{HatchSettings, closed_loop, loops_at, loops_from_handles};
use crate::cmd::{bad, points_param, targets};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Upper bound on the points and objects one HATCHPICK keeps.
pub(crate) const MAX_PICKS: usize = 1000;

/// The first prompt's keywords: what to pick.
const MODES: [&str; 6] = ["Points", "Objects", "oRigin", "Hatch", "Inherit", "preView"];

/// What HATCHPICK picks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PickMode {
    /// Internal points, each inside a closed boundary.
    #[default]
    Points,
    /// Closed boundary objects.
    Objects,
    /// One point: the pattern origin.
    Origin,
    /// Hatches to edit.
    Hatch,
    /// The hatch whose properties the dialog takes (Inherit Properties).
    Inherit,
    /// The hatch the dialog would make, shown until a pick (back to the dialog) or Enter (OK).
    Preview,
}

/// What the last HATCHPICK kept ([`Session::hatch_picks`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HatchPicks {
    pub mode: PickMode,
    pub points: Vec<Vec2>,
    /// Closed boundary objects (Objects) or hatches (Hatch).
    pub handles: Vec<Handle>,
    /// The pattern origin (Origin).
    pub origin: Option<Vec2>,
    /// The hatches a Preview shows (set by the JSON form's `preview`).
    pub preview: Vec<EntityKind>,
    /// A Preview ended with Enter: the dialog's OK.
    pub accepted: bool,
}

fn mode_named(t: &str) -> Option<PickMode> {
    Some(match t.trim().to_ascii_lowercase().as_str() {
        "points" | "p" => PickMode::Points,
        "objects" | "o" => PickMode::Objects,
        "origin" | "r" => PickMode::Origin,
        "hatch" | "h" => PickMode::Hatch,
        "inherit" | "i" => PickMode::Inherit,
        "preview" | "v" => PickMode::Preview,
        _ => return None,
    })
}

/// Upper bound on the hatches one Preview shows.
const MAX_PREVIEW: usize = 1000;

/// The hatches `{command: "hatch"|"gradient"|"hatchedit", params}` would make or change, made on
/// the drawing and taken back: the drawing is as it was, whatever the command does.
fn preview_kinds(s: &mut Session, spec: &Value) -> Result<Vec<EntityKind>> {
    let cmd = spec.get("command").and_then(Value::as_str).unwrap_or("hatch");
    let params = spec.get("params").cloned().unwrap_or_else(|| json!({}));
    let (doc, selection) = {
        let st = s.state()?;
        (Arc::clone(&st.doc), st.selection.clone())
    };
    let run = match cmd {
        "hatch" => super::run_hatch(s, &params),
        "gradient" => super::run_gradient(s, &params),
        "hatchedit" => super::edit::run_hatchedit(s, &params),
        _ => Err(bad("hatchpick", "`preview.command` is \"hatch\", \"gradient\" or \"hatchedit\"")),
    };
    let kinds = run.as_ref().ok().map(|r| {
        let listed = |v: Option<&Value>| -> Vec<Handle> {
            v.and_then(Value::as_array).map(|a| a.iter().filter_map(|h| h.as_str().and_then(Handle::parse_hex)).collect()).unwrap_or_default()
        };
        let mut hs = listed(r.get("handles"));
        if cmd == "hatchedit" {
            hs.extend(listed(params.get("handles")));
        }
        let d = s.doc().ok();
        hs.iter()
            .filter_map(|h| d.and_then(|d| d.entity(*h)))
            .filter(|e| matches!(e.kind, EntityKind::Hatch(_)))
            .take(MAX_PREVIEW)
            .map(|e| e.kind.clone())
            .collect::<Vec<_>>()
    });
    let st = s.state_mut()?;
    st.doc = doc;
    st.selection = selection;
    run?;
    Ok(kinds.unwrap_or_default())
}

/// JSON form: `{points?, handles?}`, checked as the prompts check them, or `{preview}`.
pub(super) fn run_hatchpick(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(spec) = p.get("preview").filter(|v| !v.is_null()) {
        let kinds = preview_kinds(s, spec)?;
        let n = kinds.len();
        s.hatch_picks = Some(HatchPicks { mode: PickMode::Preview, preview: kinds, ..HatchPicks::default() });
        return Ok(json!({ "preview": n }));
    }
    let given = |k: &str| p.get(k).is_some_and(|v| !v.is_null());
    if !given("points") && !given("handles") {
        return Err(bad("hatchpick", "give internal `points` or boundary `handles`"));
    }
    let mut picks = HatchPicks::default();
    if given("points") {
        let pts = points_param(p, "points").ok_or_else(|| bad("hatchpick", "`points` is an array of [x, y]"))?;
        if pts.len() > MAX_PICKS {
            return Err(bad("hatchpick", format!("at most {MAX_PICKS} points")));
        }
        for q in pts {
            loops_at(s, q).map_err(|_| bad("hatchpick", format!("no closed boundary around [{}, {}]", q.x, q.y)))?;
            picks.points.push(q);
        }
    }
    if given("handles") {
        let hs = targets(s, p)?;
        if hs.len() > MAX_PICKS {
            return Err(bad("hatchpick", format!("at most {MAX_PICKS} objects")));
        }
        let d = s.doc()?;
        for h in hs {
            match d.entity(h) {
                Some(e) if closed_loop(d, e).is_some() => picks.handles.push(h),
                _ => return Err(bad("hatchpick", format!("{} is not a closed object", h.hex()))),
            }
        }
        if picks.points.is_empty() {
            picks.mode = PickMode::Objects;
        }
    }
    let out = json!({
        "points": picks.points.iter().map(|q| [q.x, q.y]).collect::<Vec<_>>(),
        "handles": picks.handles.iter().map(|h| h.hex()).collect::<Vec<_>>(),
    });
    s.hatch_picks = Some(picks);
    Ok(out)
}

/// One pick, so Undo can drop it.
enum Pick {
    Point(Vec2, Vec<HatchLoop>),
    Objects(Vec<Handle>, Vec<HatchLoop>),
}

#[derive(Default)]
pub(super) struct HatchPickM {
    mode: Option<PickMode>,
    picks: Vec<Pick>,
    /// What Preview shows.
    preview: Vec<EntityKind>,
}

impl HatchPickM {
    fn count(&self) -> usize {
        self.picks
            .iter()
            .map(|p| match p {
                Pick::Point(..) => 1,
                Pick::Objects(hs, _) => hs.len(),
            })
            .sum()
    }
    fn keep(&self, s: &mut Session, mode: PickMode) {
        let mut out = HatchPicks { mode, ..HatchPicks::default() };
        for p in &self.picks {
            match p {
                Pick::Point(q, _) => out.points.push(*q),
                Pick::Objects(hs, _) => out.handles.extend(hs.iter().copied()),
            }
        }
        s.hatch_picks = Some(out);
    }
}

impl Interactive for HatchPickM {
    fn name(&self) -> &'static str {
        "HATCHPICK"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.mode {
            None => Prompt::new("Enter what to pick", Accept::TEXT).kw(&MODES),
            Some(PickMode::Points) => Prompt::new("Pick internal point", Accept::POINT).kw(&["Undo"]),
            Some(PickMode::Objects) => Prompt::new("Select objects", Accept::SELECT).kw(&["Undo"]),
            Some(PickMode::Origin) => Prompt::new("Specify the pattern origin", Accept::POINT),
            Some(PickMode::Hatch | PickMode::Inherit) => Prompt::new("Select hatch object", Accept::SELECT),
            Some(PickMode::Preview) => Prompt::new("Pick or press Esc to return to dialog or <Right-click to accept hatch>", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Some(mode) = self.mode else {
            match &i {
                Input::Keyword(t) | Input::Text(t) => {
                    self.mode = mode_named(t);
                    // The hatches to show were set by the JSON form just before.
                    if self.mode == Some(PickMode::Preview) {
                        self.preview = s.hatch_picks.take().map(|p| p.preview).unwrap_or_default();
                    }
                }
                Input::Enter | Input::Cancel => return Ok(Step::Cancel),
                _ => {}
            }
            return Ok(Step::Continue);
        };
        match (mode, i) {
            (_, Input::Cancel) => Ok(Step::Cancel),
            (PickMode::Points | PickMode::Objects, Input::Keyword(k)) if k == "Undo" => {
                if self.picks.pop().is_none() {
                    s.echo("Nothing to undo.");
                }
                Ok(Step::Continue)
            }
            (PickMode::Points | PickMode::Objects, Input::Enter) => {
                self.keep(s, mode);
                Ok(Step::Done)
            }
            (PickMode::Points, Input::Point(q)) => {
                if self.count() >= MAX_PICKS {
                    s.echo(format!("At most {MAX_PICKS} picks."));
                    return Ok(Step::Continue);
                }
                match loops_at(s, q) {
                    Ok(l) => {
                        s.echo(format!("Analyzing the selected data... {} loop(s)", l.len()));
                        self.picks.push(Pick::Point(q, l));
                    }
                    Err(e) => s.echo(e.to_string()),
                }
                Ok(Step::Continue)
            }
            (PickMode::Objects, Input::Pick(hs)) => {
                let room = MAX_PICKS.saturating_sub(self.count());
                let closed: Vec<Handle> = {
                    let d = s.doc()?;
                    hs.iter().copied().filter(|h| d.entity(*h).is_some_and(|e| closed_loop(d, e).is_some())).take(room).collect()
                };
                let others = hs.len().saturating_sub(closed.len());
                if !closed.is_empty() {
                    let loops = loops_from_handles(s, &closed)?;
                    self.picks.push(Pick::Objects(closed.clone(), loops));
                }
                s.echo(if others == 0 { format!("{} found", closed.len()) } else { format!("{} found, {others} not closed", closed.len()) });
                Ok(Step::Continue)
            }
            (PickMode::Origin, Input::Point(q)) => {
                s.hatch_picks = Some(HatchPicks { mode, origin: Some(q), ..HatchPicks::default() });
                Ok(Step::Done)
            }
            (PickMode::Hatch, Input::Pick(hs)) => {
                let hatches: Vec<Handle> = {
                    let d = s.doc()?;
                    hs.into_iter().filter(|h| matches!(d.entity(*h).map(|e| &e.kind), Some(EntityKind::Hatch(_)))).take(MAX_PICKS).collect()
                };
                if hatches.is_empty() {
                    s.echo("No hatch selected.");
                    return Ok(Step::Continue);
                }
                s.hatch_picks = Some(HatchPicks { mode, handles: hatches, ..HatchPicks::default() });
                Ok(Step::Done)
            }
            (PickMode::Inherit, Input::Pick(hs)) => {
                let hatch = {
                    let d = s.doc()?;
                    hs.into_iter().find(|h| matches!(d.entity(*h).map(|e| &e.kind), Some(EntityKind::Hatch(_))))
                };
                let Some(h) = hatch else {
                    s.echo("No hatch selected.");
                    return Ok(Step::Continue);
                };
                s.hatch_picks = Some(HatchPicks { mode, handles: vec![h], ..HatchPicks::default() });
                Ok(Step::Done)
            }
            (PickMode::Preview, Input::Enter) => {
                s.hatch_picks = Some(HatchPicks { mode, accepted: true, ..HatchPicks::default() });
                Ok(Step::Done)
            }
            (PickMode::Preview, Input::Point(_)) => {
                s.hatch_picks = Some(HatchPicks { mode, ..HatchPicks::default() });
                Ok(Step::Done)
            }
            (PickMode::Origin | PickMode::Hatch | PickMode::Inherit, Input::Enter) => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, _c: Vec2) -> Vec<EntityKind> {
        if self.mode == Some(PickMode::Preview) {
            return self.preview.clone();
        }
        let loops: Vec<HatchLoop> = self
            .picks
            .iter()
            .flat_map(|p| match p {
                Pick::Point(_, l) | Pick::Objects(_, l) => l.iter().cloned(),
            })
            .collect();
        if loops.is_empty() {
            return Vec::new();
        }
        vec![HatchSettings::from(s, &Value::Null, false).hatch(loops)]
    }
}
