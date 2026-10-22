//! CALIBRATE: scale a drawing by a known distance between two points.

use super::super::machines::{SelOutcome, SelectPhase, number};
use super::transform::transform_entities;
use super::{
    Accept, EngineError, EntityKind, Handle, Input, Interactive, Mat3, Prompt, Result, Session, Step, Value, Vec2, bad, f64_req, json, line,
    point_req, targets,
};
use crate::units::format_distance;

/// What CALIBRATE scales: the whole active space, the current selection, or an underlay.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CalibrateTo {
    All,
    Selection,
    Underlay,
}

impl CalibrateTo {
    fn parse(p: &Value) -> Result<Self> {
        match p.get("apply") {
            // No target: objects given to scale mean the selection; else everything. Scaling the
            // whole drawing is never chosen by accident when the caller named objects.
            None | Some(Value::Null) => {
                let named = p.get("handles").is_some_and(|v| !v.is_null()) || p.get("handle").is_some_and(|v| !v.is_null());
                Ok(if named { CalibrateTo::Selection } else { CalibrateTo::All })
            }
            Some(Value::String(s)) => {
                CalibrateTo::from_name(s).ok_or_else(|| bad("calibrate", "`apply` must be \"all\", \"selection\" or \"underlay\""))
            }
            Some(other) => Err(bad("calibrate", format!("`apply` must be a string, got {other}"))),
        }
    }
    fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "all" | "everything" | "drawing" => Some(CalibrateTo::All),
            "selection" | "selected" | "current" => Some(CalibrateTo::Selection),
            "underlay" | "pdf" | "image" => Some(CalibrateTo::Underlay),
            _ => None,
        }
    }
    fn name(self) -> &'static str {
        match self {
            CalibrateTo::All => "all",
            CalibrateTo::Selection => "selection",
            CalibrateTo::Underlay => "underlay",
        }
    }
}

/// The distance between two calibration points: coincident points, or a distance that overflows
/// with finite coordinates, are a clear error and never a division by zero.
fn measured_distance(cmd: &str, p1: Vec2, p2: Vec2) -> Result<f64> {
    let d = p1.dist(p2);
    if !d.is_finite() {
        return Err(bad(cmd, "the measured distance is too large to calibrate"));
    }
    if d <= 1e-12 {
        return Err(bad(cmd, "the two points must be different: there is no distance to calibrate"));
    }
    Ok(d)
}

/// CALIBRATE's factor: the true `length` divided by the `measured` distance. A zero, negative or
/// non-finite length is refused, as is a factor that overflows.
fn calibrate_factor(cmd: &str, measured: f64, length: f64) -> Result<f64> {
    if !(length.is_finite() && length > 1e-12) {
        return Err(bad(cmd, "the true length must be a positive number"));
    }
    let f = length / measured;
    if f.is_finite() && f > 0.0 { Ok(f) } else { Err(bad(cmd, "the true length is too large for the measured distance")) }
}

/// Every object in the active space (CALIBRATE's "everything"). Locked layers are left out by the
/// transform itself, as in every other modify command.
fn all_in_space(s: &Session) -> Vec<Handle> {
    let Ok(d) = s.doc() else { return Vec::new() };
    d.space(&s.space()).map(|st| st.iter().map(|e| e.handle).collect()).unwrap_or_default()
}

/// Underlays (PDF, raster, DWF, DGN) are not modelled yet: CALIBRATE's underlay target says so
/// instead of pretending to scale one.
fn no_underlay() -> EngineError {
    EngineError::Other("There is no underlay to scale: raster images and PDF underlays are not available yet".into())
}

/// Report a calibration on the command line, with lengths in the drawing's units and precision.
fn report_calibration(s: &mut Session, measured: f64, length: f64, factor: f64, scaled: usize) {
    let (lu, lp) = s.doc().map(|d| (d.header.i64("LUNITS", 2), d.header.i64("LUPREC", 4))).unwrap_or((2, 4));
    let objects = if scaled == 1 { "object" } else { "objects" };
    s.echo(format!(
        "Calibrated {} to {} (scale {factor:.4}): {scaled} {objects} scaled.",
        format_distance(measured, lu, lp),
        format_distance(length, lu, lp)
    ));
}

pub(super) fn run_calibrate(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "calibrate";
    let p1 = point_req(cmd, p, "p1")?;
    let p2 = point_req(cmd, p, "p2")?;
    let length = f64_req(cmd, p, "length")?;
    let measured = measured_distance(cmd, p1, p2)?;
    let factor = calibrate_factor(cmd, measured, length)?;
    let to = CalibrateTo::parse(p)?;
    let hs = match to {
        CalibrateTo::All => all_in_space(s),
        CalibrateTo::Selection => {
            let hs = targets(s, p)?;
            if hs.is_empty() {
                return Err(bad(cmd, "`apply` is \"selection\" but nothing is selected"));
            }
            hs
        }
        CalibrateTo::Underlay => return Err(no_underlay()),
    };
    let scaled = transform_entities(s, &hs, &Mat3::scale_about(p1, factor), false)?.len();
    report_calibration(s, measured, length, factor, scaled);
    Ok(json!({
        "factor": factor,
        "measured": measured,
        "length": length,
        "base": [p1.x, p1.y],
        "apply": to.name(),
        "scaled": scaled,
    }))
}

/// CALIBRATE: pick two points, type the true length between them, then choose what to scale —
/// everything, the current selection (or objects picked now), or an underlay. It is SCALE with a
/// reference length and a clear name; the factor and the transform are SCALE's own.
#[derive(Default)]
pub(super) struct CalibrateM {
    /// The two calibration points, as they are picked.
    pts: Vec<Vec2>,
    /// The true length between them, once typed.
    length: Option<f64>,
    /// Objects being selected, when the target is the selection and nothing was preselected.
    sel: Option<SelectPhase>,
}

impl CalibrateM {
    /// Scale `hs` by the calibration and end the command.
    fn scale(&mut self, s: &mut Session, hs: &[Handle]) -> Result<Step> {
        let (Some(p1), Some(p2), Some(length)) = (self.pts.first().copied(), self.pts.get(1).copied(), self.length) else {
            return Ok(Step::Done);
        };
        let measured = measured_distance("calibrate", p1, p2)?;
        let factor = calibrate_factor("calibrate", measured, length)?;
        let scaled = transform_entities(s, hs, &Mat3::scale_about(p1, factor), false)?.len();
        // The next SCALE offers the calibrated factor.
        s.last_used.scale_factor = factor;
        s.set_selection(Vec::new());
        report_calibration(s, measured, length, factor, scaled);
        Ok(Step::Done)
    }
}

impl Interactive for CalibrateM {
    fn name(&self) -> &'static str {
        "CALIBRATE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some(sel) = &self.sel {
            return sel.prompt();
        }
        match (self.pts.len(), self.length) {
            (0, _) => Prompt::new("Specify first point of the distance to calibrate", Accept::POINT),
            (1, _) => Prompt::new("Specify second point of the distance to calibrate", Accept::POINT).base(self.pts[0]),
            (_, None) => Prompt::new("Enter the true length of that distance", Accept::NUMBER),
            (_, Some(_)) => Prompt::new("Apply the scale to", Accept::TEXT).kw(&["Everything", "Selection", "Underlay"]).default("Everything"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(sel) = &mut self.sel {
            return match sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => {
                    s.echo("Nothing selected: nothing was scaled.");
                    Ok(Step::Done)
                }
                SelOutcome::Done(hs) => {
                    self.sel = None;
                    self.scale(s, &hs)
                }
            };
        }
        if self.pts.is_empty() {
            return match i {
                Input::Point(p) => {
                    self.pts.push(p);
                    Ok(Step::Continue)
                }
                Input::Enter => Ok(Step::Done),
                _ => Err(EngineError::Other("Requires a point.".into())),
            };
        }
        if self.pts.len() == 1 {
            return match i {
                Input::Point(p) => {
                    measured_distance("calibrate", self.pts[0], p)?;
                    self.pts.push(p);
                    Ok(Step::Continue)
                }
                Input::Enter => Ok(Step::Done),
                _ => Err(EngineError::Other("Requires a point.".into())),
            };
        }
        if self.length.is_none() {
            return match i {
                Input::Text(t) | Input::Keyword(t) => {
                    let n = number(&t).ok_or_else(|| EngineError::Other("Requires a positive number.".into()))?;
                    let measured = measured_distance("calibrate", self.pts[0], self.pts[1])?;
                    calibrate_factor("calibrate", measured, n)?;
                    self.length = Some(n);
                    Ok(Step::Continue)
                }
                Input::Enter => Ok(Step::Done),
                _ => Err(EngineError::Other("Requires a positive number.".into())),
            };
        }
        let to = match i {
            Input::Keyword(k) => CalibrateTo::from_name(&k),
            Input::Text(t) => CalibrateTo::from_name(&t),
            Input::Enter => Some(CalibrateTo::All),
            _ => None,
        };
        let Some(to) = to else {
            return Err(EngineError::Other("Apply the scale to Everything, Selection or Underlay.".into()));
        };
        match to {
            CalibrateTo::All => self.scale(s, &all_in_space(s)),
            CalibrateTo::Selection => {
                let selected = s.selection();
                if selected.is_empty() {
                    self.sel = Some(SelectPhase::begin(s));
                    Ok(Step::Continue)
                } else {
                    self.scale(s, &selected)
                }
            }
            CalibrateTo::Underlay => Err(no_underlay()),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        match (self.pts.as_slice(), &self.sel) {
            ([a], None) => vec![line(*a, c)],
            _ => Vec::new(),
        }
    }
}
