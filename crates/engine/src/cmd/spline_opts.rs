//! SPLINE's fit options: Knots, start/end Tangency, toLerance and Object (turn a polyline into a
//! spline), for the prompt machine in `draw.rs` and the JSON form.

use cadcraft_doc::{EntityKind, Handle};
use cadcraft_geom::{FitOptions, KnotParam, Spline, Vec2};
use serde_json::{Value, json};

use super::machines::{SelOutcome, SelectPhase, number};
use super::{bad, curves, distinct_points, f64_or, point_param, targets};
use crate::{Accept, EngineError, Input, Prompt, Result, Session};

fn other(m: impl Into<String>) -> EngineError {
    EngineError::Other(m.into())
}

/// The knot parametrisation SPLINE starts with (SPLKNOTS: 0 chord, 1 square root, 2 uniform).
pub(super) fn current_knots(s: &Session) -> KnotParam {
    s.doc().ok().and_then(|d| KnotParam::parse(&d.header.i64("SPLKNOTS", 0).to_string())).unwrap_or_default()
}

/// A question an option asks before SPLINE goes back to its point prompt.
pub(super) enum SplineAsk {
    Knots,
    StartTangent,
    EndTangent,
    Tolerance,
    Object(SelectPhase),
}

/// What the machine does after an answer.
pub(super) enum Outcome {
    /// Ask again (an invalid answer was reported, or the selection goes on).
    Stay,
    /// Back to the point prompt.
    Back,
    /// Make the spline from the points picked so far and end (after the end tangent).
    Finish,
    /// The command is over (Object converted its selection, or nothing was selected).
    Done,
}

impl SplineAsk {
    /// The option for a keyword at the point prompt, `None` for keywords handled elsewhere.
    pub(super) fn from_keyword(k: &str) -> Option<SplineAsk> {
        Some(match k {
            "Knots" => SplineAsk::Knots,
            "start Tangency" => SplineAsk::StartTangent,
            "end Tangency" => SplineAsk::EndTangent,
            "toLerance" => SplineAsk::Tolerance,
            "Object" => SplineAsk::Object(SelectPhase::default()),
            _ => return None,
        })
    }

    pub(super) fn prompt(&self, opts: &FitOptions, pts: &[Vec2]) -> Prompt {
        match self {
            SplineAsk::Knots => {
                Prompt::new("Enter knot parameterization", curves::KW).kw(&["Chord", "Square root", "Uniform"]).default(opts.knots.name())
            }
            SplineAsk::StartTangent => Prompt::new("Specify start tangent", Accept::POINT_OR_NUMBER).base_opt(pts.first().copied()),
            SplineAsk::EndTangent => Prompt::new("Specify end tangent", Accept::POINT_OR_NUMBER).base_opt(pts.last().copied()),
            SplineAsk::Tolerance => Prompt::new("Specify a fit tolerance", Accept::NUMBER).default(format!("{:.4}", opts.tolerance)),
            SplineAsk::Object(sel) => {
                let p = sel.prompt();
                if sel.removing { p } else { p.reworded("Select spline-fit polylines to convert") }
            }
        }
    }

    pub(super) fn input(&mut self, s: &mut Session, i: Input, opts: &mut FitOptions, pts: &[Vec2]) -> Result<Outcome> {
        match self {
            SplineAsk::Knots => {
                let k = match &i {
                    Input::Keyword(k) | Input::Text(k) => KnotParam::parse(k).ok_or_else(|| other("Invalid option keyword."))?,
                    Input::Enter => opts.knots,
                    _ => return Ok(Outcome::Stay),
                };
                opts.knots = k;
                s.doc_mut()?.header.set_i64("SPLKNOTS", k.code());
                Ok(Outcome::Back)
            }
            SplineAsk::StartTangent | SplineAsk::EndTangent => {
                let start = matches!(self, SplineAsk::StartTangent);
                let from = if start { pts.first() } else { pts.last() }.copied().unwrap_or_default();
                let dir = match i {
                    Input::Point(p) => {
                        let d = p - from;
                        if d.len() < 1e-12 {
                            return Err(other("The tangent point must differ from the fit point."));
                        }
                        Some(d)
                    }
                    // A typed direction (angle), measured as at any angle prompt.
                    Input::Text(t) => Some(Vec2::from_angle(s.angle_settings().direction(&t).ok_or_else(|| other("Requires a point or an angle."))?)),
                    // Enter: no tangent, the end follows the fit points.
                    Input::Enter => None,
                    _ => return Ok(Outcome::Stay),
                };
                if start {
                    opts.start_tangent = dir.map(Vec2::normalized);
                    Ok(Outcome::Back)
                } else {
                    opts.end_tangent = dir.map(Vec2::normalized);
                    Ok(Outcome::Finish)
                }
            }
            SplineAsk::Tolerance => {
                let t = match i {
                    Input::Text(t) => number(&t).filter(|v| v.is_finite() && *v >= 0.0).ok_or_else(|| other("Requires a non-negative number."))?,
                    Input::Enter => opts.tolerance,
                    _ => return Ok(Outcome::Stay),
                };
                opts.tolerance = t;
                Ok(Outcome::Back)
            }
            SplineAsk::Object(sel) => match sel.feed(s, &i)? {
                SelOutcome::More => Ok(Outcome::Stay),
                SelOutcome::Empty => Ok(Outcome::Done),
                SelOutcome::Done(hs) => {
                    let n = convert_objects(s, &hs, *opts)?;
                    s.echo(format!("{n} object(s) converted to splines"));
                    s.set_selection(Vec::new());
                    Ok(Outcome::Done)
                }
            },
        }
    }
}

/// The spline for a polyline's vertices, or why it can't be converted.
///
/// Our polylines keep no spline frame: PEDIT Spline replaces the vertices with points on the
/// curve. Those points become the fit points, so the spline follows the smoothed shape; a
/// closed polyline gives a closed (periodic) spline. Arc segments (PEDIT Fit, or drawn arcs) are
/// refused: fit points would lose the arcs.
pub(super) fn polyline_spline(k: &EntityKind, opts: FitOptions) -> std::result::Result<Spline, &'static str> {
    let (pts, closed): (Vec<Vec2>, bool) = match k {
        EntityKind::LwPolyline(pl) => {
            if pl.vertices.iter().any(|v| v.bulge.abs() > 1e-12) {
                return Err("it has arc segments");
            }
            (pl.vertices.iter().map(|v| v.p).collect(), pl.closed)
        }
        EntityKind::Polyline3d(pl) => {
            let z0 = pl.points.first().map(|p| p.z).unwrap_or(0.0);
            if pl.points.iter().any(|p| (p.z - z0).abs() > 1e-9) {
                return Err("it is not planar");
            }
            (pl.points.iter().map(|p| p.xy()).collect(), pl.closed)
        }
        _ => return Err("it is not a polyline"),
    };
    let mut pts = distinct_points(&pts);
    if closed && pts.len() > 2 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.near(*b, 1e-12)) {
        pts.pop();
    }
    if pts.len() < 2 || pts.len() > curves::MAX_GEN {
        return Err("it has too few or too many vertices");
    }
    if closed && pts.len() < 3 {
        return Err("it has too few vertices to close");
    }
    Ok(Spline::fit_with(&pts, closed, opts))
}

/// Replace each convertible polyline in `hs` by its spline (same handle, layer and properties);
/// reports the others. Returns how many were converted.
pub(super) fn convert_objects(s: &mut Session, hs: &[Handle], opts: FitOptions) -> Result<usize> {
    let mut n = 0;
    for h in hs {
        let e = curves::entity(s, *h)?;
        if curves::is_locked(s, *h) {
            s.echo("1 object was on a locked layer.");
            continue;
        }
        match polyline_spline(&e.kind, opts) {
            Ok(sp) => {
                s.doc_mut()?.modify_entity(*h, |e| e.kind = EntityKind::Spline(sp))?;
                n += 1;
            }
            Err(why) => s.echo(format!("Object not converted: {why}.")),
        }
    }
    Ok(n)
}

/// Fit options from the JSON form: `knots` ("chord" | "sqrt" | "uniform"), `startTangent` and
/// `endTangent` (directions [dx, dy]), `tolerance` (≥ 0).
pub(super) fn json_options(s: &Session, p: &Value) -> Result<FitOptions> {
    let knots = match p.get("knots") {
        None | Some(Value::Null) => current_knots(s),
        Some(v) => v
            .as_str()
            .map(str::to_string)
            .or_else(|| v.as_u64().map(|n| n.to_string()))
            .and_then(|t| KnotParam::parse(&t))
            .ok_or_else(|| bad("spline", "`knots` must be \"chord\", \"sqrt\" or \"uniform\""))?,
    };
    let tangent = |key: &str| -> Result<Option<Vec2>> {
        match p.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => point_param(p, key)
                .filter(|d| d.len() > 1e-12)
                .map(|d| Some(d.normalized()))
                .ok_or_else(|| bad("spline", format!("`{key}` must be a non-zero direction [dx, dy]"))),
        }
    };
    let tolerance = f64_or(p, "tolerance", 0.0);
    if tolerance < 0.0 {
        return Err(bad("spline", "`tolerance` must be zero or positive"));
    }
    Ok(FitOptions { knots, start_tangent: tangent("startTangent")?, end_tangent: tangent("endTangent")?, tolerance })
}

/// `{object: true, handles?}`: convert polylines (or the selection) into splines.
pub(super) fn run_object(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = match p.get("object") {
        Some(Value::Bool(true)) => targets(s, p)?,
        Some(v) => vec![curves::handle_value(v).ok_or_else(|| bad("spline", "`object` must be a handle or true"))?],
        None => return Err(bad("spline", "`object` is required")),
    };
    if hs.is_empty() {
        return Err(bad("spline", "no polyline to convert: give `object` a handle or select polylines"));
    }
    let opts = json_options(s, p)?;
    let mut converted = Vec::new();
    for h in &hs {
        let e = curves::entity(s, *h)?;
        if curves::is_locked(s, *h) {
            return Err(other("The object is on a locked layer."));
        }
        let sp = polyline_spline(&e.kind, opts).map_err(|why| other(format!("Object {} not converted: {why}.", h.hex())))?;
        s.doc_mut()?.modify_entity(*h, |e| e.kind = EntityKind::Spline(sp))?;
        converted.push(h.hex());
    }
    Ok(json!({ "handles": converted }))
}
