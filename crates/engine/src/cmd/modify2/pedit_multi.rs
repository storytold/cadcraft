//! PEDIT Multiple: edit several polylines (and lines and arcs turned into polylines) at once, as
//! AutoCAD does after `M` at "Select polyline". Join joins the selected objects with each other.

use cadcraft_doc::{Drawing, EntityKind, Handle};
use cadcraft_geom::PolyVertex;
use serde_json::json;

use super::super::curves;
use super::super::machines::{SelOutcome, SelectPhase, number};
use super::pedit::{PPhase, PeditM, clear_widths, pedit_apply, pedit_join, yes_no};
use super::{as_lwpoly, other, set_kind};
use crate::{Accept, Input, Prompt, Result, Session, Step};

/// Most option steps Undo can take back.
const MAX_UNDO: usize = 64;

/// The options of the Multiple menu.
const OPTIONS: [&str; 10] = ["Close", "Open", "Join", "Width", "Fit", "Spline", "Decurve", "Ltype gen", "Reverse", "Undo"];

impl PeditM {
    /// Start Multiple: select the objects to edit, or take the ones selected first.
    pub(super) fn start_multi(&mut self, s: &mut Session) -> Result<Step> {
        self.phase = PPhase::MSelect;
        self.msel = SelectPhase::begin(s);
        if self.msel.done {
            let hs = std::mem::take(&mut self.msel.picked);
            return self.take_multi(s, hs);
        }
        s.set_selection(Vec::new());
        Ok(Step::Continue)
    }

    /// The objects selected for Multiple: polylines, lines and arcs not on locked layers.
    fn take_multi(&mut self, s: &mut Session, hs: Vec<Handle>) -> Result<Step> {
        let d = s.doc()?;
        let kinds = |h: &Handle| d.entity(*h).map(|e| e.kind.clone());
        let editable: Vec<Handle> = hs
            .iter()
            .copied()
            .filter(|h| matches!(kinds(h), Some(EntityKind::LwPolyline(_) | EntityKind::Line(_) | EntityKind::Arc(_))))
            .filter(|h| !curves::is_locked(s, *h))
            .collect();
        let convert = editable.iter().any(|h| matches!(kinds(h), Some(EntityKind::Line(_) | EntityKind::Arc(_))));
        self.multi = editable;
        if self.multi.is_empty() {
            s.echo("No polylines, lines or arcs selected.");
            return Ok(Step::Done);
        }
        self.phase = if convert { PPhase::MConvert } else { PPhase::MMain };
        s.set_selection(self.multi.clone());
        Ok(Step::Continue)
    }

    /// The prompt of a Multiple phase.
    pub(super) fn multi_prompt(&self) -> Prompt {
        match self.phase {
            PPhase::MConvert => Prompt::new("Convert Lines and Arcs to polylines", curves::KW).kw(&["Yes", "No"]).default("Y"),
            PPhase::MMain => Prompt::new("Enter an option", curves::KW).kw(&OPTIONS),
            PPhase::MWidth => Prompt::new("Specify new width for all segments", Accept::NUMBER),
            PPhase::MFuzz => Prompt::new("Enter fuzz distance", Accept::NUMBER).default("0.0000"),
            _ => self.msel.prompt(),
        }
    }

    /// Input in a Multiple phase.
    pub(super) fn multi_input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let kw = |i: &Input| match i {
            Input::Keyword(k) | Input::Text(k) => Some(k.trim().to_ascii_lowercase()),
            _ => None,
        };
        match self.phase {
            PPhase::MSelect => match self.msel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => self.take_multi(s, hs),
            },
            PPhase::MConvert => {
                let no = !yes_no(&i)?;
                let mut kept = Vec::new();
                for h in std::mem::take(&mut self.multi) {
                    let kind = curves::entity(s, h)?.kind;
                    match kind {
                        EntityKind::LwPolyline(_) => kept.push(h),
                        _ if no => {}
                        k => {
                            if let Some(pl) = as_lwpoly(&k) {
                                set_kind(s, h, EntityKind::LwPolyline(pl))?;
                                kept.push(h);
                            }
                        }
                    }
                }
                self.multi = kept;
                if self.multi.is_empty() {
                    s.echo("No polylines to edit.");
                    return Ok(Step::Done);
                }
                s.set_selection(self.multi.clone());
                self.phase = PPhase::MMain;
                Ok(Step::Continue)
            }
            PPhase::MMain => {
                let Some(k) = kw(&i) else {
                    if i == Input::Enter {
                        s.set_selection(Vec::new());
                        return Ok(Step::Done);
                    }
                    return Ok(Step::Continue);
                };
                match k.as_str() {
                    "close" | "c" => self.apply_all(s, "close", &json!({}))?,
                    "open" | "o" => self.apply_all(s, "open", &json!({}))?,
                    "join" | "j" => self.phase = PPhase::MFuzz,
                    "width" | "w" => self.phase = PPhase::MWidth,
                    "fit" | "f" => self.curve_all(s, "fit")?,
                    "spline" | "s" => self.curve_all(s, "spline")?,
                    "decurve" | "d" => self.decurve_all(s)?,
                    "ltype gen" | "l" => {
                        // One setting for all: the opposite of the first one's.
                        let on = self.multi.first().and_then(|h| curves::entity(s, *h).ok()).is_some_and(|e| match e.kind {
                            EntityKind::LwPolyline(p) => !p.plinegen,
                            _ => true,
                        });
                        self.apply_all(s, "ltypegen", &json!({ "on": on }))?;
                    }
                    "reverse" | "r" => self.apply_all(s, "reverse", &json!({}))?,
                    "undo" | "u" => match self.mundo.pop() {
                        Some((d, multi, morig)) => {
                            *s.doc_mut()? = d;
                            self.multi = multi;
                            self.morig = morig;
                            s.set_selection(self.multi.clone());
                        }
                        None => s.echo("Nothing to undo."),
                    },
                    _ => return Err(other("Invalid option keyword.")),
                }
                Ok(Step::Continue)
            }
            PPhase::MWidth => {
                if let Input::Text(t) = &i {
                    let w = number(t).filter(|w| *w >= 0.0).ok_or_else(|| other("Requires a non-negative width."))?;
                    self.apply_all(s, "width", &json!({ "width": w }))?;
                    self.morig.values_mut().for_each(|o| clear_widths(o));
                }
                self.phase = PPhase::MMain;
                Ok(Step::Continue)
            }
            PPhase::MFuzz => {
                let fuzz = match &i {
                    Input::Text(t) => number(t).filter(|f| *f >= 0.0 && f.is_finite()).ok_or_else(|| other("Requires a non-negative distance."))?,
                    _ => 0.0,
                };
                self.join_all(s, fuzz)?;
                self.phase = PPhase::MMain;
                Ok(Step::Continue)
            }
            _ => Ok(Step::Continue),
        }
    }

    /// Keep the drawing as it is for Undo (and the polylines being edited, with their Decurve
    /// record).
    fn remember(&mut self, s: &Session) -> Result<()> {
        let d: Drawing = s.doc()?.clone();
        if self.mundo.len() >= MAX_UNDO {
            self.mundo.remove(0);
        }
        self.mundo.push((d, self.multi.clone(), self.morig.clone()));
        Ok(())
    }

    /// Apply option `opt` to every polyline being edited.
    fn apply_all(&mut self, s: &mut Session, opt: &str, p: &serde_json::Value) -> Result<()> {
        self.remember(s)?;
        self.multi.retain(|h| s.doc().is_ok_and(|d| d.entity(*h).is_some()));
        for h in self.multi.clone() {
            pedit_apply(s, h, opt, p)?;
        }
        Ok(())
    }

    /// Fit or Spline all of them, first keeping each one's vertices for Decurve.
    fn curve_all(&mut self, s: &mut Session, opt: &str) -> Result<()> {
        for h in &self.multi {
            if !self.morig.contains_key(h)
                && let Ok(EntityKind::LwPolyline(p)) = curves::entity(s, *h).map(|e| e.kind)
            {
                self.morig.insert(*h, p.vertices);
            }
        }
        self.apply_all(s, opt, &json!({}))
    }

    /// Decurve all of them: one fitted or splined here gets back its vertices from before (as
    /// single-polyline PEDIT does), the others lose their arcs.
    fn decurve_all(&mut self, s: &mut Session) -> Result<()> {
        self.apply_all(s, "decurve", &json!({}))?;
        for h in self.multi.clone() {
            if let Some(orig) = self.morig.get(&h)
                && let Ok(EntityKind::LwPolyline(mut p)) = curves::entity(s, h).map(|e| e.kind)
            {
                p.vertices = orig.iter().map(|v| PolyVertex { bulge: 0.0, ..*v }).collect();
                set_kind(s, h, EntityKind::LwPolyline(p))?;
            }
        }
        Ok(())
    }

    /// Join the polylines being edited with each other, ends within `fuzz`: each one still open
    /// takes the others that meet it, and those go into it.
    fn join_all(&mut self, s: &mut Session, fuzz: f64) -> Result<()> {
        self.remember(s)?;
        let mut rest: Vec<Handle> = self.multi.iter().copied().filter(|h| s.doc().is_ok_and(|d| d.entity(*h).is_some())).collect();
        let before = rest.len();
        let mut kept = Vec::new();
        while !rest.is_empty() {
            let h = rest.remove(0);
            let open = matches!(curves::entity(s, h)?.kind, EntityKind::LwPolyline(p) if !p.closed);
            if open {
                // A polyline that took on segments no longer goes back past the Join on Decurve.
                if pedit_join(s, h, &rest, fuzz)? > 0 {
                    self.morig.remove(&h);
                }
                rest.retain(|o| s.doc().is_ok_and(|d| d.entity(*o).is_some()));
            }
            kept.push(h);
        }
        let added = before - kept.len();
        s.echo(format!("{added} segments added to polyline"));
        self.multi = kept;
        s.set_selection(self.multi.clone());
        Ok(())
    }
}
