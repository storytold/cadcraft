//! Draw menu depth (M1/M2): every ARC and CIRCLE variant, ELLIPSE axis/arc entries, RECTANG
//! options, DIVIDE/MEASURE, REVCLOUD, WIPEOUT, XLINE options, 3DPOLY, MLINE, HELIX, spline by
//! control vertices, CENTERMARK and CENTERLINE. Each has a JSON form and a prompt sequence.

use cadcraft_doc::{EntityKind, Handle, Helix, HelixConstrain, Point, Polyline3d, RayLine, Wipeout};
use cadcraft_geom::{Arc, Circle, Ellipse, Line, PolyVertex, Polyline, Spline, TAU, Vec2, Vec3, norm_angle, shoelace};
use serde_json::{Value, json};

use super::curves::{self, Chain, MAX_GEN, TanObj};
use super::helpers::{arc, circle, line, lwpoly, rect_vertices, rect_with_corners, v3};

use super::machines::number;
use super::{CommandSpec, bad, bool_or, f64_or, f64_req, point_param, point_req, point_value, points_param, size_param, str_param, targets};

use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub(crate) mod breakline;
pub(crate) mod mline;

pub fn specs() -> Vec<CommandSpec> {
    let mut v = vec![
        CommandSpec::new("arc.sce", "Start, Center, End", run_arc_variant)
            .menu(&["Draw", "Arc", "Start, Center, End"])
            .params("{start, center, end}")
            .interactive(|_| Ok(seq(ArcV::Sce))),
        CommandSpec::new("arc.sca", "Start, Center, Angle", run_arc_variant)
            .menu(&["Draw", "Arc", "Start, Center, Angle"])
            .params("{start, center, angle (degrees, + = CCW)}")
            .interactive(|_| Ok(seq(ArcV::Sca))),
        CommandSpec::new("arc.scl", "Start, Center, Length", run_arc_variant)
            .menu(&["Draw", "Arc", "Start, Center, Length"])
            .params("{start, center, length (chord; negative = major arc)}")
            .interactive(|_| Ok(seq(ArcV::Scl))),
        CommandSpec::new("arc.sea", "Start, End, Angle", run_arc_variant)
            .menu(&["Draw", "Arc", "Start, End, Angle"])
            .params("{start, end, angle (degrees, + = CCW)}")
            .interactive(|_| Ok(seq(ArcV::Sea))),
        CommandSpec::new("arc.sed", "Start, End, Direction", run_arc_variant)
            .menu(&["Draw", "Arc", "Start, End, Direction"])
            .params("{start, end, direction (degrees | [dx,dy])}")
            .interactive(|_| Ok(seq(ArcV::Sed))),
        CommandSpec::new("arc.ser", "Start, End, Radius", run_arc_variant)
            .menu(&["Draw", "Arc", "Start, End, Radius"])
            .params("{start, end, radius (negative = major arc)}")
            .interactive(|_| Ok(seq(ArcV::Ser))),
        CommandSpec::new("arc.cse", "Center, Start, End", run_arc_variant)
            .menu(&["Draw", "Arc", "Center, Start, End"])
            .params("{center, start, end}")
            .interactive(|_| Ok(seq(ArcV::Cse))),
        CommandSpec::new("arc.csa", "Center, Start, Angle", run_arc_variant)
            .menu(&["Draw", "Arc", "Center, Start, Angle"])
            .params("{center, start, angle (degrees)}")
            .interactive(|_| Ok(seq(ArcV::Csa))),
        CommandSpec::new("arc.csl", "Center, Start, Length", run_arc_variant)
            .menu(&["Draw", "Arc", "Center, Start, Length"])
            .params("{center, start, length (chord)}")
            .interactive(|_| Ok(seq(ArcV::Csl))),
        CommandSpec::new("arc.continue", "Continue", run_arc_continue)
            .menu(&["Draw", "Arc", "Continue"])
            .params("{end} (tangent from the last line/arc) | {start, direction, end}")
            .interactive(|_| Ok(Box::new(ArcContinueM::default()))),
        CommandSpec::new("circle.cd", "Center, Diameter", run_circle_variant)
            .menu(&["Draw", "Circle", "Center, Diameter"])
            .params("{center, diameter}")
            .interactive(|_| Ok(seq(ArcV::CircleCd))),
        CommandSpec::new("circle.2p", "2 Points", run_circle_variant)
            .menu(&["Draw", "Circle", "2 Points"])
            .params("{p1, p2}")
            .interactive(|_| Ok(seq(ArcV::Circle2p))),
        CommandSpec::new("circle.3p", "3 Points", run_circle_variant)
            .menu(&["Draw", "Circle", "3 Points"])
            .params("{p1, p2, p3}")
            .interactive(|_| Ok(seq(ArcV::Circle3p))),
        CommandSpec::new("circle.ttr", "Tan, Tan, Radius", run_circle_ttr)
            .menu(&["Draw", "Circle", "Tan, Tan, Radius"])
            .params("{h1, p1, h2, p2, radius} (lines, circles, arcs, polyline segments; nearest solution to the picks)")
            .interactive(|_| Ok(Box::new(TanCircleM::new(true)))),
        CommandSpec::new("circle.ttt", "Tan, Tan, Tan", run_circle_ttt)
            .menu(&["Draw", "Circle", "Tan, Tan, Tan"])
            .params("{h1, p1, h2, p2, h3, p3}")
            .interactive(|_| Ok(Box::new(TanCircleM::new(false)))),
        CommandSpec::new("ellipse.axis", "Axis, End", run_ellipse_axis)
            .menu(&["Draw", "Ellipse", "Axis, End"])
            .params("{p1, p2, distance} | {p1, p2, p3} (axis endpoints, then half the other axis)")
            .interactive(|_| Ok(super::draw::ellipse_machine(false, false))),
        CommandSpec::new("ellipse.arc", "Elliptical Arc", run_ellipse_arc)
            .menu(&["Draw", "Ellipse", "Arc"])
            .params("{center, major: [dx,dy], ratio, start, end (degrees)} | {p1, p2, distance, start, end}")
            .interactive(|_| Ok(super::draw::ellipse_machine(false, true))),
        CommandSpec::new("divide", "Divide", run_divide)
            .menu(&["Draw", "Point", "Divide"])
            .alias(&["div"])
            .params("{handle, segments, block?, align?: bool}")
            .interactive(|_| Ok(Box::new(DivideM::new(false)))),
        CommandSpec::new("measure", "Measure", run_measure)
            .menu(&["Draw", "Point", "Measure"])
            .alias(&["me"])
            .params("{handle, length, block?, align?: bool, from?: [x,y] (end to start at)}")
            .interactive(|_| Ok(Box::new(DivideM::new(true)))),
        CommandSpec::new("revcloud", "Rectangular", run_revcloud)
            .menu(&["Draw", "Revision Cloud", "Rectangular"])
            .params("{p1, p2} | {points: [...]} | {freehand: [...]} | {handle}, arcLength?, style?: normal|calligraphy, reverse?")
            .interactive(|s| Ok(Box::new(RevcloudM::new(s, CloudKind::Rect)))),
        CommandSpec::new("revcloud.polygonal", "Polygonal", run_revcloud)
            .menu(&["Draw", "Revision Cloud", "Polygonal"])
            .params("{points: [[x,y],...], arcLength?}")
            .interactive(|s| Ok(Box::new(RevcloudM::new(s, CloudKind::Poly)))),
        CommandSpec::new("revcloud.freehand", "Freehand", run_revcloud)
            .menu(&["Draw", "Revision Cloud", "Freehand"])
            .params("{freehand: [[x,y],...], arcLength?}")
            .interactive(|s| Ok(Box::new(RevcloudM::new(s, CloudKind::Free)))),
        CommandSpec::new("wipeout", "Wipeout", run_wipeout)
            .menu(&["Draw", "Wipeout"])
            .params("{points: [[x,y],...]} | {handle (closed polyline), erase?: bool} | {frames: on|off|display}")
            .interactive(|_| Ok(Box::new(WipeoutM::default()))),
        CommandSpec::new("3dpoly", "3D Polyline", run_3dpoly)
            .menu(&["Draw", "3D Polyline"])
            .alias(&["3p"])
            .params("{points: [[x,y,z?],...], closed?}")
            .interactive(|_| Ok(Box::new(Poly3dM::default()))),
        CommandSpec::new("helix", "Helix", run_helix)
            .menu(&["Draw", "Helix"])
            .params("{center, baseRadius, topRadius?, height?, turns?, ccw?, startAngle? (degrees)} → {handle}")
            .interactive(|_| Ok(Box::new(HelixM::default()))),
        CommandSpec::new("spline.cv", "Control Vertices", run_spline_cv)
            .menu(&["Draw", "Spline", "Control Vertices"])
            .params("{control: [[x,y],...], degree?: 1..10 (default 3), closed?}")
            .interactive(|_| Ok(Box::new(SplineCvM::default()))),
        CommandSpec::new("centermark", "Center Mark", run_centermark)
            .menu(&["Draw", "Center Mark"])
            .params("{handle | handles} (circles/arcs; lines on the CENTER linetype)")
            .interactive(|_| Ok(Box::new(CenterM { first: None, mark: true }))),
        CommandSpec::new("centerline", "Center Line", run_centerline)
            .menu(&["Draw", "Center Line"])
            .params("{h1, h2} (two lines)")
            .interactive(|_| Ok(Box::new(CenterM { first: None, mark: false }))),
    ];
    v.extend(mline::specs());
    v.extend(breakline::specs());
    v
}

fn added(h: Handle) -> Result<Value> {
    Ok(json!({ "handle": h.hex() }))
}

fn other(m: impl Into<String>) -> EngineError {
    EngineError::Other(m.into())
}

// =====================================================================================
// Generic point/value sequences (arc and circle variants)
// =====================================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArcV {
    /// The ARC command itself: three points, with options switching to the other variants.
    ThreeP,
    /// Start, end, then center.
    Sec,
    Sce,
    Sca,
    Scl,
    Sea,
    Sed,
    Ser,
    Cse,
    Csa,
    Csl,
    CircleCd,
    Circle2p,
    Circle3p,
}

/// What a step asks for.
#[derive(Clone, Copy, Debug)]
enum Ask {
    Point,
    /// Absolute angle from value `base` to the picked point, or a typed angle.
    Angle(usize),
    /// CCW angle at value `center` from the ray to value `from` to the ray to the picked point.
    Sweep(usize, usize),
    /// Distance from value `base`, or a typed distance (may be negative).
    Dist(usize),
}

#[derive(Clone, Copy, Debug)]
enum Val {
    P(Vec2),
    N(f64),
}

impl Val {
    fn p(self) -> Option<Vec2> {
        if let Val::P(p) = self { Some(p) } else { None }
    }
    fn n(self) -> Option<f64> {
        if let Val::N(n) = self { Some(n) } else { None }
    }
}

impl ArcV {
    fn name(self) -> &'static str {
        match self {
            ArcV::CircleCd | ArcV::Circle2p | ArcV::Circle3p => "CIRCLE",
            _ => "ARC",
        }
    }
    fn steps(self) -> &'static [(&'static str, Ask)] {
        const S: &str = "Specify start point of arc";
        const C: &str = "Specify center point of arc";
        const E: &str = "Specify end point of arc";
        const A: &str = "Specify included angle";
        const L: &str = "Specify length of chord";
        match self {
            ArcV::ThreeP => &[(S, Ask::Point), ("Specify second point of arc", Ask::Point), (E, Ask::Point)],
            ArcV::Sec => &[(S, Ask::Point), (E, Ask::Point), (C, Ask::Point)],
            ArcV::Sce => &[(S, Ask::Point), (C, Ask::Point), (E, Ask::Point)],
            ArcV::Sca => &[(S, Ask::Point), (C, Ask::Point), (A, Ask::Sweep(1, 0))],
            ArcV::Scl => &[(S, Ask::Point), (C, Ask::Point), (L, Ask::Dist(0))],
            ArcV::Sea => &[(S, Ask::Point), (E, Ask::Point), (A, Ask::Angle(0))],
            ArcV::Sed => &[(S, Ask::Point), (E, Ask::Point), ("Specify tangent direction for the start point of arc", Ask::Angle(0))],
            ArcV::Ser => &[(S, Ask::Point), (E, Ask::Point), ("Specify radius of arc", Ask::Dist(1))],
            ArcV::Cse => &[(C, Ask::Point), (S, Ask::Point), ("Specify end point of arc (hold Ctrl to switch direction)", Ask::Point)],
            ArcV::Csa => &[(C, Ask::Point), (S, Ask::Point), (A, Ask::Sweep(0, 1))],
            ArcV::Csl => &[(C, Ask::Point), (S, Ask::Point), (L, Ask::Dist(1))],
            ArcV::CircleCd => &[("Specify center point for circle", Ask::Point), ("Specify diameter of circle", Ask::Dist(0))],
            ArcV::Circle2p => {
                &[("Specify first end point of circle's diameter", Ask::Point), ("Specify second end point of circle's diameter", Ask::Point)]
            }
            ArcV::Circle3p => &[
                ("Specify first point on circle", Ask::Point),
                ("Specify second point on circle", Ask::Point),
                ("Specify third point on circle", Ask::Point),
            ],
        }
    }
    fn build(self, v: &[Val], ctrl: bool) -> Option<EntityKind> {
        let p = |i: usize| v.get(i).and_then(|x| x.p());
        let n = |i: usize| v.get(i).and_then(|x| x.n());
        let a = match self {
            ArcV::ThreeP => Arc::from_3_points(p(0)?, p(1)?, p(2)?),
            ArcV::Sec => Some(Arc::from_start_center_end(p(0)?, p(2)?, p(1)?)),
            ArcV::Sce => Some(Arc::from_start_center_end(p(0)?, p(1)?, p(2)?)),
            ArcV::Sca => curves::arc_sca(p(0)?, p(1)?, n(2)?),
            ArcV::Scl => curves::arc_scl(p(0)?, p(1)?, n(2)?),
            ArcV::Sea => curves::arc_sea(p(0)?, p(1)?, n(2)?),
            ArcV::Sed => curves::arc_sed(p(0)?, p(1)?, Vec2::from_angle(n(2)?)),
            ArcV::Ser => curves::arc_ser(p(0)?, p(1)?, n(2)?),
            // Center, Start, End. Ctrl keeps the two endpoints on the circle and walks the
            // other way around: swapping the start/end angles is the mirror arc through the
            // same two endpoints. (Continue below flips differently, by negating the start
            // tangent; both are "reverse", but only here is the chord fixed by the picks.)
            ArcV::Cse => {
                let a = Arc::from_start_center_end(p(1)?, p(0)?, p(2)?);
                Some(if ctrl { Arc::new(a.center, a.radius, a.end, a.start) } else { a })
            }
            ArcV::Csa => curves::arc_sca(p(1)?, p(0)?, n(2)?),
            ArcV::Csl => curves::arc_scl(p(1)?, p(0)?, n(2)?),
            ArcV::CircleCd => {
                let d = n(1)?.abs();
                let c = p(0)?;
                return (d > 1e-12 && d.is_finite()).then(|| circle(c, d / 2.0));
            }
            ArcV::Circle2p => {
                let c = Circle::from_2_points(p(0)?, p(1)?);
                return (c.radius > 1e-12).then(|| circle(c.center, c.radius));
            }
            ArcV::Circle3p => {
                let c = Circle::from_3_points(p(0)?, p(1)?, p(2)?)?;
                return Some(circle(c.center, c.radius));
            }
        }?;
        (a.radius > 1e-12 && a.radius.is_finite() && a.center.is_finite()).then(|| arc(&a))
    }
}

impl ArcV {
    /// Options at step `i` and the variant each one switches to (the values so far carry over).
    fn alts(self, i: usize) -> &'static [(&'static str, ArcV)] {
        match (self, i) {
            (ArcV::ThreeP, 0) => &[("Center", ArcV::Cse)],
            (ArcV::ThreeP, 1) => &[("Center", ArcV::Sce), ("End", ArcV::Sec)],
            (ArcV::Sce, 2) => &[("Angle", ArcV::Sca), ("chord Length", ArcV::Scl)],
            (ArcV::Cse, 2) => &[("Angle", ArcV::Csa), ("chord Length", ArcV::Csl)],
            (ArcV::Sec, 2) => &[("Angle", ArcV::Sea), ("Direction", ArcV::Sed), ("Radius", ArcV::Ser)],
            _ => &[],
        }
    }
}

fn seq(v: ArcV) -> Box<dyn Interactive> {
    Box::new(SeqM { v, vals: Vec::new(), cont: None })
}

/// The full ARC prompt sequence (all options; Enter at the first prompt continues the last
/// line or arc).
pub(crate) fn arc_machine() -> Box<dyn Interactive> {
    seq(ArcV::ThreeP)
}

struct SeqM {
    v: ArcV,
    vals: Vec<Val>,
    /// ARC continuation: start point and tangent.
    cont: Option<(Vec2, Vec2)>,
}

impl SeqM {
    fn ask(&self) -> Option<(&'static str, Ask)> {
        self.v.steps().get(self.vals.len()).copied()
    }
    fn value_from_point(&self, ask: Ask, q: Vec2) -> Option<Val> {
        let pt = |i: usize| self.vals.get(i).and_then(|x| x.p());
        Some(match ask {
            Ask::Point => Val::P(q),
            Ask::Angle(b) => Val::N(pt(b)?.angle_to(q)),
            Ask::Sweep(c, f) => {
                let c = pt(c)?;
                Val::N(norm_angle(c.angle_to(q) - c.angle_to(pt(f)?)))
            }
            Ask::Dist(b) => Val::N(pt(b)?.dist(q)),
        })
    }
    fn base(&self) -> Option<Vec2> {
        match self.ask()?.1 {
            Ask::Point => self.vals.last().and_then(|v| v.p()),
            Ask::Angle(b) | Ask::Dist(b) => self.vals.get(b).and_then(|v| v.p()),
            Ask::Sweep(c, _) => self.vals.get(c).and_then(|v| v.p()),
        }
    }
}

impl Interactive for SeqM {
    fn name(&self) -> &'static str {
        self.v.name()
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some((st, _)) = self.cont {
            return Prompt::new("Specify end point of arc (hold Ctrl to switch direction)", Accept::POINT).base(st);
        }
        let kws: Vec<&str> = self.v.alts(self.vals.len()).iter().map(|a| a.0).collect();
        let p = match self.ask() {
            Some((m, Ask::Point)) => Prompt::new(m, Accept::POINT).base_opt(self.base()),
            Some((m, _)) => Prompt::new(m, Accept::POINT_OR_NUMBER).base_opt(self.base()),
            None => Prompt::new("", Accept::default()),
        };
        p.kw(&kws)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some((st, dir)) = self.cont {
            // Ctrl flips the arc: negating the start tangent makes `arc_sed` pick the mirror
            // arc (same two points and center, opposite sweep). `ctrl_down` comes from the
            // UI's per-frame write on `Session`; the `Input` enum itself stays free of
            // modifiers so it remains UI-agnostic.
            let dir = if s.ctrl_down { -dir } else { dir };
            return match i {
                Input::Point(e) => {
                    let a = curves::arc_sed(st, e, dir).ok_or_else(|| other("Invalid end point."))?;
                    s.add_entity(arc(&a))?;
                    Ok(Step::Done)
                }
                Input::Enter => Ok(Step::Cancel),
                _ => Err(other("Point required.")),
            };
        }
        if let Input::Keyword(k) = &i
            && let Some((_, v)) = self.v.alts(self.vals.len()).iter().find(|a| a.0 == k)
        {
            self.v = *v;
            return Ok(Step::Continue);
        }
        if i == Input::Enter && self.v == ArcV::ThreeP && self.vals.is_empty() {
            self.cont = curves::last_curve_end(s);
            if self.cont.is_none() {
                s.echo("No line or arc to continue.");
                return Ok(Step::Cancel);
            }
            return Ok(Step::Continue);
        }
        let Some((_, ask)) = self.ask() else { return Ok(Step::Done) };
        let val = match (&i, ask) {
            (Input::Point(q), _) => self.value_from_point(ask, *q),
            (Input::Text(t), Ask::Angle(_) | Ask::Sweep(..)) => {
                // The tangent direction is a direction; included angles are sizes.
                let au = s.angle_settings();
                let a = if self.v == ArcV::Sed { au.direction(t) } else { au.amount(t) };
                Some(Val::N(a.ok_or_else(|| other("Requires a valid angle."))?))
            }
            (Input::Text(t), Ask::Dist(_)) => Some(Val::N(number(t).ok_or_else(|| other("Requires numeric distance or a point."))?)),
            (Input::Enter, _) => return Ok(Step::Cancel),
            _ => return Err(other("Point or value required.")),
        };
        let Some(val) = val else { return Ok(Step::Continue) };
        self.vals.push(val);
        if self.vals.len() < self.v.steps().len() {
            return Ok(Step::Continue);
        }
        match self.v.build(&self.vals, s.ctrl_down) {
            Some(k) => {
                if let EntityKind::Circle(c) = &k {
                    let r = c.radius;
                    s.add_entity(k)?;
                    s.doc_mut()?.header.set_f64("CIRCLERAD", r);
                } else {
                    s.add_entity(k)?;
                }
                Ok(Step::Done)
            }
            None => {
                self.vals.pop();
                Err(other("Invalid arc or circle for these values."))
            }
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if let Some((st, d)) = self.cont {
            let d = if s.ctrl_down { -d } else { d };
            return curves::arc_sed(st, c, d).map(|a| vec![arc(&a)]).unwrap_or_default();
        }
        let Some((_, ask)) = self.ask() else { return Vec::new() };
        let Some(v) = self.value_from_point(ask, c) else { return Vec::new() };
        let mut vals = self.vals.clone();
        vals.push(v);
        let mut out: Vec<EntityKind> = self.base().map(|b| vec![line(b, c)]).unwrap_or_default();
        if vals.len() == self.v.steps().len() {
            out.extend(self.v.build(&vals, s.ctrl_down));
        }
        out
    }
}

fn direction_param(p: &Value) -> Option<f64> {
    match p.get("direction") {
        Some(v) if v.is_array() || v.is_object() => point_value(v).filter(|d| d.len() > 1e-12).map(Vec2::angle),
        Some(v) => v.as_f64().filter(|x| x.is_finite()).map(f64::to_radians),
        None => None,
    }
}

fn arc_from_params(p: &Value) -> Result<EntityKind> {
    let st = point_param(p, "start");
    let c = point_param(p, "center");
    let e = point_param(p, "end");
    let ang = p.get("angle").and_then(Value::as_f64).filter(|v| v.is_finite()).map(f64::to_radians);
    let len = p.get("length").and_then(Value::as_f64).filter(|v| v.is_finite());
    let rad = p.get("radius").and_then(Value::as_f64).filter(|v| v.is_finite());
    let dir = direction_param(p);
    let a = match (st, c, e) {
        (Some(st), Some(c), Some(e)) => Some(Arc::from_start_center_end(st, c, e)),
        (Some(st), Some(c), None) => match (ang, len) {
            (Some(a), _) => curves::arc_sca(st, c, a),
            (None, Some(l)) => curves::arc_scl(st, c, l),
            _ => return Err(bad("arc", "start+center needs `end`, `angle` or `length`")),
        },
        (Some(st), None, Some(e)) => match (ang, dir, rad) {
            (Some(a), _, _) => curves::arc_sea(st, e, a),
            (None, Some(d), _) => curves::arc_sed(st, e, Vec2::from_angle(d)),
            (None, None, Some(r)) => curves::arc_ser(st, e, r),
            _ => return Err(bad("arc", "start+end needs `angle`, `direction` or `radius`")),
        },
        _ => return Err(bad("arc", "give start with center and/or end (see params)")),
    };
    let a = a.filter(|a| a.radius > 1e-12 && a.radius.is_finite()).ok_or_else(|| bad("arc", "these values do not define an arc"))?;
    Ok(arc(&a))
}

fn run_arc_variant(s: &mut Session, p: &Value) -> Result<Value> {
    let k = arc_from_params(p)?;
    added(s.add_entity(k)?)
}

fn run_arc_continue(s: &mut Session, p: &Value) -> Result<Value> {
    let e = point_req("arc.continue", p, "end")?;
    let (st, dir) = match (point_param(p, "start"), direction_param(p)) {
        (Some(st), Some(d)) => (st, Vec2::from_angle(d)),
        _ => curves::last_curve_end(s).ok_or_else(|| bad("arc.continue", "no line or arc to continue from"))?,
    };
    let a = curves::arc_sed(st, e, dir).ok_or_else(|| bad("arc.continue", "end point is on the tangent line"))?;
    let h = s.add_entity(arc(&a))?;
    // The next continuation starts where this arc was drawn to, not at its stored CCW end.
    s.last_point = e;
    added(h)
}

#[derive(Default)]
struct ArcContinueM {
    from: Option<(Vec2, Vec2)>,
}

impl Interactive for ArcContinueM {
    fn name(&self) -> &'static str {
        "ARC"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.from = curves::last_curve_end(s);
        if self.from.is_none() {
            s.echo("No line or arc to continue.");
            return Ok(Step::Cancel);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Specify end point of arc (hold Ctrl to switch direction)", Accept::POINT).base_opt(self.from.map(|f| f.0))
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Some((st, dir)) = self.from else { return Ok(Step::Cancel) };
        // Ctrl flips the direction, exactly as in `SeqM::input`.
        let dir = if s.ctrl_down { -dir } else { dir };
        match i {
            Input::Point(e) => {
                let a = curves::arc_sed(st, e, dir).ok_or_else(|| other("Invalid end point."))?;
                s.add_entity(arc(&a))?;
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Cancel),
            _ => Err(other("Point required.")),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        self.from
            .and_then(|(st, dir)| {
                let dir = if s.ctrl_down { -dir } else { dir };
                curves::arc_sed(st, c, dir)
            })
            .map(|a| vec![arc(&a)])
            .unwrap_or_default()
    }
}

fn run_circle_variant(s: &mut Session, p: &Value) -> Result<Value> {
    let c = if let (Some(c), Some(d)) = (point_param(p, "center"), p.get("diameter").and_then(Value::as_f64)) {
        Circle::new(c, d / 2.0)
    } else if let (Some(a), Some(b)) = (point_param(p, "p1"), point_param(p, "p2")) {
        match point_param(p, "p3") {
            Some(c3) => Circle::from_3_points(a, b, c3).ok_or_else(|| bad("circle", "points are collinear"))?,
            None => Circle::from_2_points(a, b),
        }
    } else {
        return Err(bad("circle", "give {center, diameter}, {p1, p2} or {p1, p2, p3}"));
    };
    if !(c.radius > 1e-12 && c.radius.is_finite()) {
        return Err(bad("circle", "radius must be positive"));
    }
    let h = s.add_entity(circle(c.center, c.radius))?;
    s.doc_mut()?.header.set_f64("CIRCLERAD", c.radius);
    added(h)
}

// ---------------- tangent circles ----------------

fn tan_obj(s: &Session, h: Handle, pick: Vec2) -> Result<TanObj> {
    let e = curves::entity(s, h)?;
    TanObj::from_kind(&e.kind, pick).ok_or_else(|| other("Select a line, circle, arc or polyline segment."))
}

fn pick_pair(cmd: &str, p: &Value, hk: &str, pk: &str) -> Result<(Handle, Vec2)> {
    let h = curves::handle_param(p, hk).ok_or_else(|| bad(cmd, format!("`{hk}` is required")))?;
    let q = point_req(cmd, p, pk)?;
    Ok((h, q))
}

fn run_circle_ttr(s: &mut Session, p: &Value) -> Result<Value> {
    let (h1, p1) = pick_pair("circle.ttr", p, "h1", "p1")?;
    let (h2, p2) = pick_pair("circle.ttr", p, "h2", "p2")?;
    let r = f64_req("circle.ttr", p, "radius")?;
    let c = curves::ttr(tan_obj(s, h1, p1)?, p1, tan_obj(s, h2, p2)?, p2, r).ok_or_else(|| other("Circle does not exist."))?;
    let h = s.add_entity(circle(c.center, c.radius))?;
    s.doc_mut()?.header.set_f64("CIRCLERAD", c.radius);
    added(h)
}

fn run_circle_ttt(s: &mut Session, p: &Value) -> Result<Value> {
    let (h1, p1) = pick_pair("circle.ttt", p, "h1", "p1")?;
    let (h2, p2) = pick_pair("circle.ttt", p, "h2", "p2")?;
    let (h3, p3) = pick_pair("circle.ttt", p, "h3", "p3")?;
    let objs = [(tan_obj(s, h1, p1)?, p1), (tan_obj(s, h2, p2)?, p2), (tan_obj(s, h3, p3)?, p3)];
    let c = curves::ttt(objs).ok_or_else(|| other("Circle does not exist."))?;
    added(s.add_entity(circle(c.center, c.radius))?)
}

/// TTR (two picks + radius) or TTT (three picks). Also used by CIRCLE's `Ttr` option.
pub(crate) struct TanCircleM {
    radius_mode: bool,
    picks: Vec<(TanObj, Vec2)>,
}

impl TanCircleM {
    pub(crate) fn new(radius_mode: bool) -> Self {
        TanCircleM { radius_mode, picks: Vec::new() }
    }
}

impl Interactive for TanCircleM {
    fn name(&self) -> &'static str {
        "CIRCLE"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let n = self.picks.len();
        if self.radius_mode {
            match n {
                0 => Prompt::new("Specify point on object for first tangent of circle", Accept::POINT),
                1 => Prompt::new("Specify point on object for second tangent of circle", Accept::POINT),
                _ => {
                    let r = s.doc().map(|d| d.header.f64("CIRCLERAD", 0.0)).unwrap_or(0.0);
                    let p = Prompt::new("Specify radius of circle", Accept::POINT_OR_NUMBER).base_opt(self.picks.last().map(|x| x.1)).distance();
                    if r > 0.0 { p.default(format!("{r:.4}")) } else { p }
                }
            }
        } else {
            let m = ["Specify first point on circle: _tan to", "Specify second point on circle: _tan to", "Specify third point on circle: _tan to"];
            Prompt::new(m.get(n).copied().unwrap_or(""), Accept::POINT)
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let need = if self.radius_mode { 2 } else { 3 };
        if self.picks.len() < need {
            return match i {
                Input::Point(p) => {
                    let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                    let o = tan_obj(s, h, p)?;
                    self.picks.push((o, p));
                    if !self.radius_mode && self.picks.len() == 3 {
                        let (Some(a), Some(b), Some(c)) = (self.picks.first(), self.picks.get(1), self.picks.get(2)) else {
                            return Ok(Step::Cancel);
                        };
                        let c = curves::ttt([*a, *b, *c]).ok_or_else(|| {
                            self.picks.clear();
                            other("Circle does not exist.")
                        })?;
                        s.add_entity(circle(c.center, c.radius))?;
                        return Ok(Step::Done);
                    }
                    Ok(Step::Continue)
                }
                Input::Enter => Ok(Step::Cancel),
                _ => Err(other("Select an object.")),
            };
        }
        let r = match i {
            Input::Point(p) => self.picks.last().map(|x| x.1.dist(p)).unwrap_or(0.0),
            Input::Text(t) => number(&t).ok_or_else(|| other("Requires numeric distance or second point."))?,
            Input::Enter => s.doc()?.header.f64("CIRCLERAD", 0.0),
            _ => return Ok(Step::Continue),
        };
        if r.is_nan() || r <= 0.0 {
            return Err(other("Value must be positive and nonzero."));
        }
        let ((o1, p1), (o2, p2)) = match (self.picks.first(), self.picks.get(1)) {
            (Some(a), Some(b)) => (*a, *b),
            _ => return Ok(Step::Cancel),
        };
        match curves::ttr(o1, p1, o2, p2, r) {
            Some(c) => {
                s.add_entity(circle(c.center, c.radius))?;
                s.doc_mut()?.header.set_f64("CIRCLERAD", r);
                Ok(Step::Done)
            }
            None => {
                s.echo("Circle does not exist.");
                Ok(Step::Done)
            }
        }
    }
}

// ---------------- ellipse entries ----------------

fn ellipse_from_axis(p1: Vec2, p2: Vec2, half: f64) -> Option<Ellipse> {
    let c = p1.mid(p2);
    let m = p2 - c;
    let ml = m.len();
    let half = half.abs();
    if ml < 1e-12 || half < 1e-12 || !half.is_finite() {
        return None;
    }
    Some(if half > ml { Ellipse::full(c, m.normalized().perp() * half, ml / half) } else { Ellipse::full(c, m, half / ml) })
}

fn axis_ellipse_params(cmd: &str, p: &Value) -> Result<Ellipse> {
    let p1 = point_req(cmd, p, "p1")?;
    let p2 = point_req(cmd, p, "p2")?;
    let half = match point_param(p, "p3") {
        Some(p3) => {
            let ax = Line::new(p1, p2);
            ax.project(p3).dist(p3)
        }
        None => f64_req(cmd, p, "distance")?,
    };
    ellipse_from_axis(p1, p2, half).ok_or_else(|| bad(cmd, "degenerate ellipse"))
}

fn ellipse_kind(e: &Ellipse) -> EntityKind {
    EntityKind::Ellipse(cadcraft_doc::Ellipse { center: v3(e.center), major: v3(e.major), ratio: e.ratio, start: e.start, end: e.end })
}

fn run_ellipse_axis(s: &mut Session, p: &Value) -> Result<Value> {
    let e = axis_ellipse_params("ellipse.axis", p)?;
    added(s.add_entity(ellipse_kind(&e))?)
}

fn run_ellipse_arc(s: &mut Session, p: &Value) -> Result<Value> {
    let mut e = if let Some(c) = point_param(p, "center") {
        let m = point_req("ellipse.arc", p, "major")?;
        let ratio = f64_req("ellipse.arc", p, "ratio")?;
        if !(ratio > 0.0 && ratio <= 1.0) || m.len() < 1e-12 {
            return Err(bad("ellipse.arc", "ratio must be in (0, 1] and major non-zero"));
        }
        Ellipse::full(c, m, ratio)
    } else {
        axis_ellipse_params("ellipse.arc", p)?
    };
    // Angles are measured from the major axis (like the prompts); convert to parameters.
    let to_param = |deg: f64| {
        let a = deg.to_radians();
        let ge = Ellipse { start: 0.0, end: TAU, ..e };
        ge.param_of(e.center + Vec2::from_angle(a + e.major.angle()))
    };
    let st = to_param(f64_req("ellipse.arc", p, "start")?);
    let en = to_param(f64_req("ellipse.arc", p, "end")?);
    e.start = st;
    e.end = en;
    added(s.add_entity(ellipse_kind(&e))?)
}

// =====================================================================================
// DIVIDE / MEASURE
// =====================================================================================

fn chain_of(s: &Session, h: Handle) -> Result<Chain> {
    let e = curves::entity(s, h)?;
    Chain::of(&e.kind).ok_or_else(|| other("Cannot divide/measure that object."))
}

fn place_markers(s: &mut Session, at: &[(Vec2, Vec2)], block: Option<&str>, align: bool) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for (p, t) in at {
        let h = match block {
            Some(b) => super::blocks::insert(s, b, *p, 1.0, if align { t.angle() } else { 0.0 }, &serde_json::Map::new())?,
            None => s.add_entity(EntityKind::Point(Point { p: v3(*p), angle: 0.0 }))?,
        };
        out.push(h.hex());
    }
    Ok(out)
}

fn check_block(s: &Session, b: Option<&str>) -> Result<()> {
    if let Some(b) = b
        && s.doc()?.block(b).is_none()
    {
        return Err(other(format!("Block \"{b}\" not found.")));
    }
    Ok(())
}

pub(crate) fn divide_points(c: &Chain, n: usize) -> Vec<(Vec2, Vec2)> {
    let l = c.len();
    let range = if c.closed { 0..n } else { 1..n };
    range.filter_map(|i| c.at_length(l * i as f64 / n as f64)).collect()
}

pub(crate) fn measure_points(c: &Chain, step: f64) -> Vec<(Vec2, Vec2)> {
    let l = c.len();
    let mut out = Vec::new();
    let mut k = 1usize;
    while (k as f64) * step < l - 1e-9 && out.len() < MAX_GEN {
        let d = k as f64 * step;
        out.extend(c.at_length(d));
        k += 1;
    }
    out
}

fn run_divide(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("divide", "`handle` is required"))?;
    let n = p.get("segments").and_then(Value::as_u64).ok_or_else(|| bad("divide", "`segments` (2..32767) is required"))?;
    if !(2..=32767).contains(&n) {
        return Err(bad("divide", "segments must be 2..32767"));
    }
    let block = str_param(p, "block").map(str::to_string);
    check_block(s, block.as_deref())?;
    let c = chain_of(s, h)?;
    let pts = divide_points(&c, n as usize);
    let hs = place_markers(s, &pts, block.as_deref(), bool_or(p, "align", true))?;
    Ok(json!({ "handles": hs }))
}

fn run_measure(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("measure", "`handle` is required"))?;
    let step = f64_req("measure", p, "length")?;
    let mut c = chain_of(s, h)?;
    if step.is_nan() || step <= 0.0 || c.len() / step > MAX_GEN as f64 {
        return Err(bad("measure", "length must be positive (and not tiny)"));
    }
    let block = str_param(p, "block").map(str::to_string);
    check_block(s, block.as_deref())?;
    if let Some(from) = point_param(p, "from")
        && !c.closed
        && let (Some(a), Some(b)) = (c.start(), c.end())
        && from.dist(b) < from.dist(a)
    {
        c = c.reversed();
    }
    let pts = measure_points(&c, step);
    let hs = place_markers(s, &pts, block.as_deref(), bool_or(p, "align", true))?;
    Ok(json!({ "handles": hs }))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DivPhase {
    Object,
    Value,
    BlockName,
    Align,
}

struct DivideM {
    measure: bool,
    obj: Option<(Handle, Vec2)>,
    phase: DivPhase,
    block: Option<String>,
    align: bool,
}

impl DivideM {
    fn new(measure: bool) -> Self {
        DivideM { measure, obj: None, phase: DivPhase::Object, block: None, align: true }
    }
}

impl Interactive for DivideM {
    fn name(&self) -> &'static str {
        if self.measure { "MEASURE" } else { "DIVIDE" }
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.phase {
            DivPhase::Object => Prompt::new(if self.measure { "Select object to measure" } else { "Select object to divide" }, Accept::POINT),
            DivPhase::Value => {
                let m = if self.measure { "Specify length of segment" } else { "Enter the number of segments" };
                let p = Prompt::new(m, if self.measure { Accept::POINT_OR_NUMBER } else { Accept::NUMBER });
                let p = if self.measure { p.base_opt(self.obj.map(|o| o.1)) } else { p };
                if self.block.is_some() { p } else { p.kw(&["Block"]) }
            }
            DivPhase::BlockName => Prompt::new("Enter name of block to insert", Accept::TEXT),
            DivPhase::Align => Prompt::new("Align block with object?", curves::KW).kw(&["Yes", "No"]).default("Y"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.phase, i) {
            (DivPhase::Object, Input::Point(p)) => {
                let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                chain_of(s, h)?;
                self.obj = Some((h, p));
                self.phase = DivPhase::Value;
                Ok(Step::Continue)
            }
            (DivPhase::Value, Input::Keyword(k)) if k == "Block" => {
                self.phase = DivPhase::BlockName;
                Ok(Step::Continue)
            }
            (DivPhase::BlockName, Input::Text(t) | Input::Keyword(t)) => {
                let name = t.trim().to_string();
                check_block(s, Some(&name))?;
                self.block = Some(name);
                self.phase = DivPhase::Align;
                Ok(Step::Continue)
            }
            (DivPhase::Align, Input::Keyword(k) | Input::Text(k)) => {
                self.align = !k.trim().to_ascii_lowercase().starts_with('n');
                self.phase = DivPhase::Value;
                Ok(Step::Continue)
            }
            (DivPhase::Align, Input::Enter) => {
                self.phase = DivPhase::Value;
                Ok(Step::Continue)
            }
            (DivPhase::Value, inp @ (Input::Text(_) | Input::Point(_))) => {
                let Some((h, pick)) = self.obj else { return Ok(Step::Cancel) };
                let v = match inp {
                    Input::Text(t) => number(&t).ok_or_else(|| other("Requires a number."))?,
                    Input::Point(q) => pick.dist(q),
                    _ => return Ok(Step::Continue),
                };
                let mut params = json!({ "handle": h.hex(), "align": self.align, "from": [pick.x, pick.y] });
                if let Some(b) = &self.block {
                    params["block"] = json!(b);
                }
                if self.measure {
                    params["length"] = json!(v);
                    run_measure(s, &params)?;
                } else {
                    if !(2.0..=32767.0).contains(&v) || v.fract() != 0.0 {
                        return Err(other("Requires an integer between 2 and 32767."));
                    }
                    params["segments"] = json!(v as u64);
                    run_divide(s, &params)?;
                }
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Cancel),
            _ => Err(other("Invalid input.")),
        }
    }
}

// =====================================================================================
// REVCLOUD
// =====================================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloudKind {
    Rect,
    Poly,
    Free,
}

fn default_arc_len(s: &Session) -> f64 {
    let d = s.doc().ok();
    let metric = d.is_some_and(|d| d.header.i64("MEASUREMENT", 0) == 1);
    d.map(|d| d.header.f64("REVCLOUDARCLEN", if metric { 15.0 } else { 0.5 })).unwrap_or(0.5).max(1e-6)
}

/// Bulged closed polyline vertices for a revision cloud around `boundary`.
pub(crate) fn cloud_vertices(boundary: &[Vec2], arc_len: f64, per_edge: bool, reverse: bool) -> Option<Vec<PolyVertex>> {
    let mut pts: Vec<Vec2> = Vec::new();
    for p in boundary {
        if p.is_finite() && pts.last().is_none_or(|l| !l.near(*p, 1e-9)) {
            pts.push(*p);
        }
    }
    while pts.len() > 1 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.near(*b, 1e-9)) {
        pts.pop();
    }
    if pts.len() < 3 || shoelace(&pts).abs() < 1e-12 {
        return None;
    }
    if shoelace(&pts) < 0.0 {
        pts.reverse();
    }
    let n = pts.len();
    let perim: f64 = (0..n).filter_map(|i| Some(pts.get(i)?.dist(*pts.get((i + 1) % n)?))).sum();
    if !(arc_len.is_finite() && arc_len > 0.0) {
        return None;
    }
    let arc_len = arc_len.max(perim / MAX_GEN as f64);
    let mut out: Vec<Vec2> = Vec::new();
    if per_edge {
        for i in 0..n {
            let (Some(a), Some(b)) = (pts.get(i).copied(), pts.get((i + 1) % n).copied()) else { continue };
            let k = ((a.dist(b) / arc_len).round() as usize).clamp(1, MAX_GEN);
            for j in 0..k {
                out.push(a.lerp(b, j as f64 / k as f64));
            }
        }
    } else {
        let chain = Chain {
            segs: (0..n).filter_map(|i| Some(cadcraft_geom::Segment::Line(Line::new(*pts.get(i)?, *pts.get((i + 1) % n)?)))).collect(),
            closed: true,
        };
        let k = ((perim / arc_len).round() as usize).clamp(3, MAX_GEN);
        for j in 0..k {
            out.extend(chain.at_length(perim * j as f64 / k as f64).map(|x| x.0));
        }
    }
    let b = if reverse { -0.5 } else { 0.5 };
    Some(out.into_iter().map(|p| PolyVertex::with_bulge(p, b)).collect())
}

fn object_outline(s: &Session, h: Handle) -> Result<Vec<Vec2>> {
    let e = curves::entity(s, h)?;
    let tol = 1e-3;
    let pts = match &e.kind {
        EntityKind::LwPolyline(p) if p.closed => Polyline { vertices: p.vertices.clone(), closed: true }.tessellate(tol),
        EntityKind::Circle(c) => {
            let mut v = Vec::new();
            Circle::new(c.center.xy(), c.radius).tessellate(c.radius * 1e-3, &mut v);
            v
        }
        EntityKind::Ellipse(el) => {
            let mut v = Vec::new();
            Ellipse { center: el.center.xy(), major: el.major.xy(), ratio: el.ratio, start: el.start, end: el.end }
                .tessellate(el.major.xy().len() * 1e-3, &mut v);
            v
        }
        EntityKind::Spline(sp) if sp.closed => sp.tessellate(tol),
        _ => return Err(other("Select a closed polyline, circle, ellipse or closed spline.")),
    };
    Ok(pts)
}

fn cloud_entity(vs: Vec<PolyVertex>, calligraphy: bool, arc_len: f64) -> EntityKind {
    let vs = if calligraphy { vs.into_iter().map(|v| PolyVertex { start_width: 0.0, end_width: arc_len * 0.15, ..v }).collect() } else { vs };
    lwpoly(vs, true)
}

fn run_revcloud(s: &mut Session, p: &Value) -> Result<Value> {
    let arc_len = p.get("arcLength").and_then(Value::as_f64).filter(|v| v.is_finite() && *v > 0.0).unwrap_or_else(|| default_arc_len(s));
    let reverse = bool_or(p, "reverse", false);
    let calligraphy = str_param(p, "style").is_some_and(|x| x.eq_ignore_ascii_case("calligraphy"));
    let (pts, per_edge, src) = if let (Some(a), Some(b)) = (point_param(p, "p1"), point_param(p, "p2")) {
        (rect_vertices(a, b).into_iter().map(|v| v.p).collect(), true, None)
    } else if let Some(pts) = points_param(p, "points") {
        (pts, true, None)
    } else if let Some(pts) = points_param(p, "freehand") {
        (pts, false, None)
    } else if let Some(h) = curves::handle_param(p, "handle") {
        (object_outline(s, h)?, false, Some(h))
    } else {
        return Err(bad("revcloud", "give {p1, p2}, {points}, {freehand} or {handle}"));
    };
    if pts.len() > MAX_GEN {
        return Err(bad("revcloud", "too many points"));
    }
    let vs = cloud_vertices(&pts, arc_len, per_edge, reverse).ok_or_else(|| bad("revcloud", "the boundary needs an area"))?;
    let h = s.add_entity(cloud_entity(vs, calligraphy, arc_len))?;
    if let Some(src) = src {
        let common = curves::entity(s, src)?.common;
        let d = s.doc_mut()?;
        d.modify_entity(h, |e| e.common = common)?;
        d.remove_entity(src);
    }
    added(h)
}

struct RevcloudM {
    kind: CloudKind,
    pts: Vec<Vec2>,
    arc_len: f64,
    asking_arc: bool,
    object: bool,
    calligraphy: bool,
    asking_style: bool,
}

impl RevcloudM {
    fn new(s: &Session, kind: CloudKind) -> Self {
        RevcloudM { kind, pts: Vec::new(), arc_len: default_arc_len(s), asking_arc: false, object: false, calligraphy: false, asking_style: false }
    }
    fn boundary(&self, extra: Option<Vec2>) -> Vec<Vec2> {
        let mut pts = self.pts.clone();
        pts.extend(extra);
        if self.kind == CloudKind::Rect
            && let (Some(a), Some(b)) = (pts.first().copied(), pts.get(1).copied())
        {
            return rect_vertices(a, b).into_iter().map(|v| v.p).collect();
        }
        pts
    }
    fn finish(&mut self, s: &mut Session) -> Result<Step> {
        let b = self.boundary(None);
        match cloud_vertices(&b, self.arc_len, self.kind != CloudKind::Free, false) {
            Some(vs) => {
                s.add_entity(cloud_entity(vs, self.calligraphy, self.arc_len))?;
                s.echo("Revision cloud finished.");
                Ok(Step::Done)
            }
            None => Err(other("The revision cloud needs at least three points enclosing an area.")),
        }
    }
}

impl Interactive for RevcloudM {
    fn name(&self) -> &'static str {
        "REVCLOUD"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let t = match self.kind {
            CloudKind::Rect => "Rectangular",
            CloudKind::Poly => "Polygonal",
            CloudKind::Free => "Freehand",
        };
        s.echo(format!("Minimum arc length: {0:.4}   Maximum arc length: {0:.4}   Style: Normal   Type: {t}", self.arc_len));
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.asking_arc {
            return Prompt::new("Specify minimum length of arc", Accept::NUMBER).default(format!("{:.4}", self.arc_len));
        }
        if self.asking_style {
            return Prompt::new("Select arc style", curves::KW).kw(&["Normal", "Calligraphy"]).default("Normal");
        }
        if self.object {
            return Prompt::new("Select object", Accept::POINT);
        }
        let first_kw: &[&str] = &["Arc length", "Object", "Rectangular", "Polygonal", "Freehand", "Style"];
        match (self.kind, self.pts.len()) {
            (CloudKind::Rect, 0) => Prompt::new("Specify first corner point", Accept::POINT).kw(first_kw).default("Object"),
            (CloudKind::Rect, _) => {
                Prompt { corner: true, ..Prompt::new("Specify opposite corner", Accept::POINT).base_opt(self.pts.first().copied()) }
            }
            (CloudKind::Poly, 0) => Prompt::new("Specify start point", Accept::POINT).kw(first_kw).default("Object"),
            (CloudKind::Poly, 1) => Prompt::new("Specify next point", Accept::POINT).base_opt(self.pts.last().copied()),
            (CloudKind::Poly, _) => Prompt::new("Specify next point", Accept::POINT).kw(&["Undo"]).base_opt(self.pts.last().copied()),
            (CloudKind::Free, 0) => Prompt::new("Specify first point", Accept::POINT).kw(first_kw).default("Object"),
            (CloudKind::Free, _) => Prompt::new("Guide crosshairs along cloud path", Accept::POINT).base_opt(self.pts.last().copied()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.asking_arc {
            if let Input::Text(t) = &i {
                let v = number(t).filter(|v| *v > 0.0).ok_or_else(|| other("Requires a positive value."))?;
                self.arc_len = v;
                s.doc_mut()?.header.set_f64("REVCLOUDARCLEN", v);
            }
            self.asking_arc = false;
            return Ok(Step::Continue);
        }
        if self.asking_style {
            if let Input::Keyword(k) | Input::Text(k) = &i {
                self.calligraphy = k.to_ascii_lowercase().starts_with('c');
            }
            self.asking_style = false;
            return Ok(Step::Continue);
        }
        if self.object {
            let Input::Point(p) = i else { return if i == Input::Enter { Ok(Step::Cancel) } else { Ok(Step::Continue) } };
            let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
            let pts = object_outline(s, h)?;
            let vs = cloud_vertices(&pts, self.arc_len, false, false).ok_or_else(|| other("The object has no area."))?;
            let nh = s.add_entity(cloud_entity(vs, self.calligraphy, self.arc_len))?;
            let common = curves::entity(s, h)?.common;
            let d = s.doc_mut()?;
            d.modify_entity(nh, |e| e.common = common)?;
            d.remove_entity(h);
            s.echo("Revision cloud finished.");
            return Ok(Step::Done);
        }
        match i {
            Input::Keyword(k) => {
                match k.as_str() {
                    "Arc length" => self.asking_arc = true,
                    "Object" => self.object = true,
                    "Rectangular" => self.kind = CloudKind::Rect,
                    "Polygonal" => self.kind = CloudKind::Poly,
                    "Freehand" => self.kind = CloudKind::Free,
                    "Style" => self.asking_style = true,
                    "Undo" => {
                        self.pts.pop();
                    }
                    _ => {}
                }
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                self.pts.push(p);
                if self.kind == CloudKind::Rect && self.pts.len() == 2 {
                    return self.finish(s);
                }
                if self.pts.len() > MAX_GEN {
                    return self.finish(s);
                }
                Ok(Step::Continue)
            }
            Input::Enter if self.pts.is_empty() => {
                self.object = true;
                Ok(Step::Continue)
            }
            Input::Enter => self.finish(s),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.pts.is_empty() || self.object {
            return Vec::new();
        }
        let b = self.boundary(Some(c));
        match cloud_vertices(&b, self.arc_len, self.kind != CloudKind::Free, false) {
            Some(vs) if vs.len() < 2000 => vec![lwpoly(vs, true)],
            _ => {
                let mut pts = self.pts.clone();
                pts.push(c);
                vec![lwpoly(pts.into_iter().map(PolyVertex::new).collect(), false)]
            }
        }
    }
}

// =====================================================================================
// WIPEOUT
// =====================================================================================

fn set_frames(s: &mut Session, mode: &str) -> Result<i64> {
    let v = match mode.trim().to_ascii_lowercase().as_str() {
        "on" | "1" => 1,
        "off" | "0" => 0,
        "display" | "d" | "2" | "display but not plot" => 2,
        _ => return Err(other("Frames mode must be ON, OFF or Display.")),
    };
    s.doc_mut()?.header.set_i64("WIPEOUTFRAME", v);
    Ok(v)
}

fn wipeout_from_polyline(s: &mut Session, h: Handle, erase: bool) -> Result<Handle> {
    let e = curves::entity(s, h)?;
    let EntityKind::LwPolyline(p) = &e.kind else { return Err(other("Select a closed polyline.")) };
    if !p.closed || p.vertices.len() < 3 {
        return Err(other("The polyline must be closed with at least three vertices."));
    }
    if p.vertices.iter().any(|v| v.bulge.abs() > 1e-12) {
        return Err(other("Polylines with arc segments cannot be used for wipeouts."));
    }
    let nh = s.add_entity(EntityKind::Wipeout(Wipeout { boundary: p.vertices.iter().map(|v| v.p).collect() }))?;
    if erase {
        s.doc_mut()?.remove_entity(h);
    }
    Ok(nh)
}

fn run_wipeout(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(f) = str_param(p, "frames") {
        let v = set_frames(s, f)?;
        return Ok(json!({ "WIPEOUTFRAME": v }));
    }
    if let Some(h) = curves::handle_param(p, "handle") {
        return added(wipeout_from_polyline(s, h, bool_or(p, "erase", false))?);
    }
    let pts = points_param(p, "points").ok_or_else(|| bad("wipeout", "`points` (3 or more) or `handle` is required"))?;
    if pts.len() < 3 || pts.len() > MAX_GEN || shoelace(&pts).abs() < 1e-12 {
        return Err(bad("wipeout", "need 3+ points enclosing an area"));
    }
    added(s.add_entity(EntityKind::Wipeout(Wipeout { boundary: pts }))?)
}

#[derive(Default)]
struct WipeoutM {
    pts: Vec<Vec2>,
    phase: u8, // 0 points, 1 select polyline, 2 erase?, 3 frames
    poly: Option<Handle>,
}

impl Interactive for WipeoutM {
    fn name(&self) -> &'static str {
        "WIPEOUT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (self.phase, self.pts.len()) {
            (1, _) => Prompt::new("Select a closed polyline", Accept::POINT),
            (2, _) => Prompt::new("Erase polyline?", curves::KW).kw(&["Yes", "No"]).default("No"),
            (3, _) => Prompt::new("Enter mode", curves::KW).kw(&["ON", "OFF", "Display but not plot"]).default("ON"),
            (_, 0) => Prompt::new("Specify first point", Accept::POINT).kw(&["Frames", "Polyline"]).default("Polyline"),
            (_, 1) => Prompt::new("Specify next point", Accept::POINT).base_opt(self.pts.last().copied()),
            (_, _) => Prompt::new("Specify next point", Accept::POINT).kw(&["Undo"]).base_opt(self.pts.last().copied()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.phase, i) {
            (1, Input::Point(p)) => {
                let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                let e = curves::entity(s, h)?;
                if !matches!(&e.kind, EntityKind::LwPolyline(pl) if pl.closed) {
                    return Err(other("Select a closed polyline."));
                }
                self.poly = Some(h);
                self.phase = 2;
                Ok(Step::Continue)
            }
            (2, inp @ (Input::Keyword(_) | Input::Text(_) | Input::Enter)) => {
                let erase = matches!(&inp, Input::Keyword(k) | Input::Text(k) if k.to_ascii_lowercase().starts_with('y'));
                let h = self.poly.ok_or_else(|| other("No polyline."))?;
                wipeout_from_polyline(s, h, erase)?;
                Ok(Step::Done)
            }
            (3, inp @ (Input::Keyword(_) | Input::Text(_) | Input::Enter)) => {
                let m = match &inp {
                    Input::Keyword(k) | Input::Text(k) => k.clone(),
                    _ => "on".into(),
                };
                let m = if m.to_ascii_lowercase().starts_with('d') { "display".to_string() } else { m };
                set_frames(s, &m)?;
                Ok(Step::Done)
            }
            (0, Input::Keyword(k)) => {
                match k.as_str() {
                    "Frames" => self.phase = 3,
                    "Polyline" => self.phase = 1,
                    "Undo" => {
                        self.pts.pop();
                    }
                    _ => {}
                }
                Ok(Step::Continue)
            }
            (0, Input::Point(p)) => {
                if self.pts.len() < MAX_GEN {
                    self.pts.push(p);
                }
                Ok(Step::Continue)
            }
            (0, Input::Enter) if self.pts.is_empty() => {
                self.phase = 1;
                Ok(Step::Continue)
            }
            (0, Input::Enter) => {
                if self.pts.len() < 3 || shoelace(&self.pts).abs() < 1e-12 {
                    return Err(other("A wipeout needs three or more points enclosing an area."));
                }
                s.add_entity(EntityKind::Wipeout(Wipeout { boundary: self.pts.clone() }))?;
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.phase != 0 || self.pts.is_empty() {
            return Vec::new();
        }
        let mut pts = self.pts.clone();
        pts.push(c);
        vec![lwpoly(pts.into_iter().map(PolyVertex::new).collect(), true)]
    }
}

// =====================================================================================
// XLINE with Hor/Ver/Ang/Bisect/Offset
// =====================================================================================

fn add_xline(s: &mut Session, base: Vec2, dir: Vec2) -> Result<Handle> {
    let d = dir.normalized();
    if d == Vec2::ZERO || !base.is_finite() {
        return Err(other("Invalid direction."));
    }
    s.add_entity(EntityKind::XLine(RayLine { base: v3(base), dir: v3(d) }))
}

/// A line-like object for XLINE Offset/Reference: (base, direction). For a polyline, the
/// segment nearest `pick` (the first without one), which must be straight.
fn linear_of(s: &Session, h: Handle, pick: Option<Vec2>) -> Result<(Vec2, Vec2)> {
    let e = curves::entity(s, h)?;
    match &e.kind {
        EntityKind::Line(l) => Ok((l.a.xy(), (l.b.xy() - l.a.xy()).normalized())),
        EntityKind::XLine(r) | EntityKind::Ray(r) => Ok((r.base.xy(), r.dir.xy().normalized())),
        EntityKind::LwPolyline(p) => {
            let segs = Polyline { vertices: p.vertices.clone(), closed: p.closed }.segments();
            let seg = match pick {
                Some(q) => segs.iter().min_by(|a, b| a.closest(q).dist(q).total_cmp(&b.closest(q).dist(q))),
                None => segs.first(),
            };
            match seg {
                Some(cadcraft_geom::Segment::Line(l)) => Ok((l.a, l.dir())),
                _ => Err(other("Select a line object.")),
            }
        }
        _ => Err(other("Select a line object.")),
    }
}

fn bisect_dir(v: Vec2, a: Vec2, b: Vec2) -> Option<Vec2> {
    let u1 = (a - v).normalized();
    let u2 = (b - v).normalized();
    if u1 == Vec2::ZERO || u2 == Vec2::ZERO {
        return None;
    }
    let s = u1 + u2;
    if s.len() < 1e-12 { Some(u1.perp()) } else { Some(s.normalized()) }
}

fn offset_xline(base: Vec2, dir: Vec2, dist: f64, side: Vec2) -> (Vec2, Vec2) {
    let n = dir.perp();
    let sgn = if n.dot(side - base) >= 0.0 { 1.0 } else { -1.0 };
    (base + n * (dist * sgn), dir)
}

pub(crate) fn run_xline2(s: &mut Session, p: &Value) -> Result<Value> {
    if let (Some(v), Some(a), Some(b)) = (point_param(p, "vertex"), point_param(p, "start"), point_param(p, "end")) {
        let d = bisect_dir(v, a, b).ok_or_else(|| bad("xline", "degenerate angle"))?;
        return added(add_xline(s, v, d)?);
    }
    if let Some(h) = curves::handle_param(p, "handle") {
        let (base, dir) = linear_of(s, h, None)?;
        let (nb, nd) = if let Some(t) = point_param(p, "through") {
            (t, dir)
        } else {
            let dist = f64_req("xline", p, "distance")?;
            let side = point_req("xline", p, "side")?;
            offset_xline(base, dir, dist, side)
        };
        return added(add_xline(s, nb, nd)?);
    }
    let b = point_param(p, "base").or_else(|| point_param(p, "through")).ok_or_else(|| bad("xline", "`base` is required"))?;
    let dir = if bool_or(p, "hor", false) {
        Vec2::X
    } else if bool_or(p, "ver", false) {
        Vec2::Y
    } else if let (Some(t), Some(_)) = (point_param(p, "through"), point_param(p, "base")) {
        t - b
    } else {
        Vec2::from_angle(f64_req("xline", p, "angle")?.to_radians())
    };
    if dir.len() < 1e-12 {
        return Err(bad("xline", "through point equals base"));
    }
    added(add_xline(s, b, dir)?)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum XMode {
    #[default]
    Two,
    Fixed(f64),
    AngAsk,
    RefSelect,
    RefAngle(f64),
    Bisect,
    OffDist,
    OffSelect,
    OffSide,
}

#[derive(Default)]
pub(crate) struct XlineM2 {
    mode: XMode,
    base: Option<Vec2>,
    bis: Vec<Vec2>,
    offset: Option<f64>, // None = Through
    line: Option<(Vec2, Vec2)>,
}

impl Interactive for XlineM2 {
    fn name(&self) -> &'static str {
        "XLINE"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match self.mode {
            XMode::Two => match self.base {
                None => Prompt::new("Specify a point", Accept::POINT).kw(&["Hor", "Ver", "Ang", "Bisect", "Offset"]),
                Some(b) => Prompt::new("Specify through point", Accept::POINT).base(b),
            },
            XMode::Fixed(_) => Prompt::new("Specify through point", Accept::POINT),
            XMode::AngAsk => Prompt::new("Enter angle of xline (0) or", Accept::NUMBER).kw(&["Reference"]),
            XMode::RefSelect => Prompt::new("Select a line object", Accept::POINT),
            XMode::RefAngle(_) => Prompt::new("Enter angle of xline", Accept::NUMBER).default("0"),
            XMode::Bisect => match self.bis.len() {
                0 => Prompt::new("Specify angle vertex point", Accept::POINT),
                1 => Prompt::new("Specify angle start point", Accept::POINT).base_opt(self.bis.first().copied()),
                _ => Prompt::new("Specify angle end point", Accept::POINT).base_opt(self.bis.first().copied()),
            },
            XMode::OffDist => {
                let d = s.doc().map(|d| d.header.f64("OFFSETDIST", 1.0)).unwrap_or(1.0);
                Prompt::new("Specify offset distance or", Accept::NUMBER).kw(&["Through"]).default(if d < 0.0 {
                    "Through".into()
                } else {
                    format!("{d:.4}")
                })
            }
            XMode::OffSelect => Prompt::new("Select a line object", Accept::POINT),
            XMode::OffSide => Prompt::new(if self.offset.is_some() { "Specify side to offset" } else { "Specify through point" }, Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.mode, i) {
            (_, Input::Enter) if matches!(self.mode, XMode::OffDist) => {
                let d = s.doc()?.header.f64("OFFSETDIST", 1.0);
                self.offset = (d >= 0.0).then_some(d);
                self.mode = XMode::OffSelect;
                Ok(Step::Continue)
            }
            (_, Input::Enter) => Ok(Step::Done),
            (XMode::Two, Input::Keyword(k)) => {
                self.mode = match k.as_str() {
                    "Hor" => XMode::Fixed(0.0),
                    "Ver" => XMode::Fixed(std::f64::consts::FRAC_PI_2),
                    "Ang" => XMode::AngAsk,
                    "Bisect" => XMode::Bisect,
                    _ => XMode::OffDist,
                };
                Ok(Step::Continue)
            }
            (XMode::Two, Input::Point(p)) => {
                match self.base {
                    None => self.base = Some(p),
                    Some(b) => {
                        if !b.near(p, 1e-12) {
                            add_xline(s, b, p - b)?;
                        }
                    }
                }
                Ok(Step::Continue)
            }
            (XMode::Fixed(a), Input::Point(p)) => {
                add_xline(s, p, Vec2::from_angle(a))?;
                Ok(Step::Continue)
            }
            (XMode::AngAsk, Input::Keyword(_)) => {
                self.mode = XMode::RefSelect;
                Ok(Step::Continue)
            }
            (XMode::AngAsk, Input::Text(t)) => {
                let a = s.angle_settings().direction(&t).ok_or_else(|| other("Requires a valid angle."))?;
                self.mode = XMode::Fixed(a);
                Ok(Step::Continue)
            }
            (XMode::RefSelect, Input::Point(p)) => {
                let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                let (_, d) = linear_of(s, h, Some(p))?;
                self.mode = XMode::RefAngle(d.angle());
                Ok(Step::Continue)
            }
            (XMode::RefAngle(base), Input::Text(t)) => {
                let a = s.angle_settings().rotation(&t).ok_or_else(|| other("Requires a valid angle."))?;
                self.mode = XMode::Fixed(base + a);
                Ok(Step::Continue)
            }
            (XMode::Bisect, Input::Point(p)) => {
                if self.bis.len() < 2 {
                    self.bis.push(p);
                } else if let (Some(v), Some(a)) = (self.bis.first().copied(), self.bis.get(1).copied()) {
                    let d = bisect_dir(v, a, p).ok_or_else(|| other("Invalid angle."))?;
                    add_xline(s, v, d)?;
                }
                Ok(Step::Continue)
            }
            (XMode::OffDist, Input::Keyword(_)) => {
                self.offset = None;
                s.doc_mut()?.header.set_f64("OFFSETDIST", -1.0);
                self.mode = XMode::OffSelect;
                Ok(Step::Continue)
            }
            (XMode::OffDist, Input::Text(t)) => {
                let d = number(&t).filter(|d| *d >= 0.0).ok_or_else(|| other("Requires a non-negative distance."))?;
                self.offset = Some(d);
                s.doc_mut()?.header.set_f64("OFFSETDIST", d);
                self.mode = XMode::OffSelect;
                Ok(Step::Continue)
            }
            (XMode::OffSelect, Input::Point(p)) => {
                let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                self.line = Some(linear_of(s, h, Some(p))?);
                self.mode = XMode::OffSide;
                Ok(Step::Continue)
            }
            (XMode::OffSide, Input::Point(p)) => {
                let (b, d) = self.line.ok_or_else(|| other("No line selected."))?;
                let (nb, nd) = match self.offset {
                    Some(dist) => offset_xline(b, d, dist, p),
                    None => (p, d),
                };
                add_xline(s, nb, nd)?;
                self.mode = XMode::OffSelect;
                Ok(Step::Continue)
            }
            _ => Err(other("Invalid input.")),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        let xl = |b: Vec2, d: Vec2| {
            let d = d.normalized();
            (d != Vec2::ZERO).then(|| EntityKind::XLine(RayLine { base: v3(b), dir: v3(d) }))
        };
        match self.mode {
            XMode::Two => self.base.and_then(|b| xl(b, c - b)).into_iter().collect(),
            XMode::Fixed(a) => xl(c, Vec2::from_angle(a)).into_iter().collect(),
            XMode::Bisect => match self.bis.as_slice() {
                [v, a] => bisect_dir(*v, *a, c).and_then(|d| xl(*v, d)).into_iter().collect(),
                _ => Vec::new(),
            },
            XMode::OffSide => self
                .line
                .and_then(|(b, d)| {
                    let (nb, nd) = match self.offset {
                        Some(dist) => offset_xline(b, d, dist, c),
                        None => (c, d),
                    };
                    xl(nb, nd)
                })
                .into_iter()
                .collect(),
            _ => Vec::new(),
        }
    }
}

// =====================================================================================
// RECTANG with Area / Dimensions / Rotation / Width / Elevation / Thickness
// =====================================================================================

fn rect_kind(first: Vec2, (w, h): (f64, f64), rot: f64, fillet: f64, chamfer: (f64, f64), width: f64, elevation: f64) -> EntityKind {
    let local = rect_with_corners(Vec2::ZERO, Vec2::new(w, h), fillet, chamfer);
    let vs = local.into_iter().map(|v| PolyVertex { p: first + v.p.rotate(rot), ..v }).collect();
    let mut k = lwpoly(vs, true);
    if let EntityKind::LwPolyline(pl) = &mut k {
        pl.const_width = width.max(0.0);
        pl.elevation = elevation;
    }
    k
}

/// The width and height of a rectangle from its `dimensions`, its `p2` corner, its `area` with
/// `length` or `breadth`, or its second corner `p2`, rotated by `rot` about `a`.
fn rect_size(p: &Value, a: Vec2, rot: f64, centered: bool) -> Result<(f64, f64)> {
    Ok(if let Some(d) = p.get("dimensions").and_then(Value::as_array) {
        let l = d.first().and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| bad("rectang", "dimensions: [length, width]"))?;
        let wd = d.get(1).and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| bad("rectang", "dimensions: [length, width]"))?;
        let (sx, sy) = match point_param(p, "p2") {
            Some(q) => {
                let loc = (q - a).rotate(-rot);
                (if loc.x < 0.0 { -1.0 } else { 1.0 }, if loc.y < 0.0 { -1.0 } else { 1.0 })
            }
            None => (1.0, 1.0),
        };
        (l.abs() * sx, wd.abs() * sy)
    } else if let Some(area) = p.get("area").and_then(Value::as_f64).filter(|v| v.is_finite()) {
        if area <= 0.0 {
            return Err(bad("rectang", "area must be positive"));
        }
        match (p.get("length").and_then(Value::as_f64), p.get("breadth").and_then(Value::as_f64)) {
            (Some(l), _) if l.is_finite() && l > 0.0 => (l, area / l),
            (_, Some(b)) if b.is_finite() && b > 0.0 => (area / b, b),
            _ => return Err(bad("rectang", "area needs `length` or `breadth`")),
        }
    } else {
        let b = point_req("rectang", p, "p2")?;
        let loc = (b - a).rotate(-rot);
        // In centre mode the picked point is one corner, mirrored across the centre.
        if centered { (2.0 * loc.x, 2.0 * loc.y) } else { (loc.x, loc.y) }
    })
}

/// Draws a rectangle from the JSON form: `p1` is a corner, or the centre when `center` is
/// true, and the size comes from `p2`, `dimensions` or `area` plus `length`/`breadth`.
pub(crate) fn run_rectang2(s: &mut Session, p: &Value) -> Result<Value> {
    let a = point_req("rectang", p, "p1")?;
    // Centre mode: `p1` is the rectangle's centre, so the rectangle spans it evenly.
    let centered = bool_or(p, "center", false);
    let rot = f64_or(p, "rotation", 0.0).to_radians();
    let ch = p
        .get("chamfer")
        .and_then(Value::as_array)
        .map(|c| (c.first().and_then(Value::as_f64).unwrap_or(0.0), c.get(1).and_then(Value::as_f64).unwrap_or(0.0)))
        .map(|(x, y)| (if x.is_finite() { x } else { 0.0 }, if y.is_finite() { y } else { 0.0 }))
        .unwrap_or((0.0, 0.0));
    if ch.0 < 0.0 || ch.1 < 0.0 {
        return Err(bad("rectang", "`chamfer` distances must be zero or positive"));
    }
    let fillet = size_param("rectang", p, "fillet", true)?.unwrap_or(0.0);
    let width = size_param("rectang", p, "width", true)?.unwrap_or(0.0);
    let (w, h) = rect_size(p, a, rot, centered)?;
    if w.abs() < 1e-12 || h.abs() < 1e-12 || !w.is_finite() || !h.is_finite() {
        return Err(bad("rectang", "rectangle has no area"));
    }
    let first = if centered { a - Vec2::new(w / 2.0, h / 2.0).rotate(rot) } else { a };
    let k = rect_kind(first, (w, h), rot, fillet, ch, width, f64_or(p, "elevation", 0.0));
    let hd = s.add_entity(k)?;
    // A rectangle has its own thickness option and does not take THICKNESS.
    let th = f64_or(p, "thickness", 0.0);
    s.doc_mut()?.modify_entity(hd, |e| e.common.thickness = th)?;
    added(hd)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum RAsk {
    Chamfer1,
    Chamfer2,
    Fillet,
    Width,
    Elevation,
    Thickness,
    Area,
    AreaBasis,
    AreaLength,
    AreaWidth,
    DimL,
    DimW,
    Rotation,
    RotPick1,
    RotPick2(Vec2),
}

#[derive(Default)]
pub(crate) struct RectM2 {
    first: Option<Vec2>,
    /// The first point is the rectangle's centre (the `Center` option), not a corner.
    center: bool,
    fillet: f64,
    chamfer: (f64, f64),
    width: f64,
    elevation: f64,
    thickness: f64,
    rotation: f64,
    dims: Option<(f64, f64)>,
    area: f64,
    asking: Option<RAsk>,
}

impl RectM2 {
    /// Adds the rectangle of full size `w` × `h` with the collected options, drawing it from
    /// the centre when centre mode is on, then applies the requested thickness.
    fn make(&self, s: &mut Session, w: f64, h: f64) -> Result<Step> {
        let Some(a) = self.first else { return Ok(Step::Cancel) };
        if w.abs() < 1e-12 || h.abs() < 1e-12 {
            return Err(other("The rectangle has no area."));
        }
        let first = self.origin(w, h).unwrap_or(a);
        let hd = s.add_entity(rect_kind(first, (w, h), self.rotation, self.fillet, self.chamfer, self.width, self.elevation))?;
        // A rectangle has its own thickness option and does not take THICKNESS.
        let t = self.thickness;
        s.doc_mut()?.modify_entity(hd, |e| e.common.thickness = t)?;
        Ok(Step::Done)
    }
    /// The rectangle's local origin (the corner `rect_kind` draws from) for a size: the first
    /// point itself as a corner, or the first point less half the size around the centre.
    fn origin(&self, w: f64, h: f64) -> Option<Vec2> {
        let a = self.first?;
        Some(if self.center { a - Vec2::new(w / 2.0, h / 2.0).rotate(self.rotation) } else { a })
    }
    /// The rectangle's signed full size implied by the picked point `p`: explicit `dims` only
    /// take their quadrant from `p`, otherwise `p` is a corner, doubled about the centre in
    /// centre mode.
    fn other_corner(&self, p: Vec2) -> Option<(f64, f64)> {
        let a = self.first?;
        let loc = (p - a).rotate(-self.rotation);
        Some(match self.dims {
            Some((l, w)) => (if loc.x < 0.0 { -l } else { l }, if loc.y < 0.0 { -w } else { w }),
            // In centre mode the picked point is one corner, mirrored across the centre.
            None if self.center => (2.0 * loc.x, 2.0 * loc.y),
            None => (loc.x, loc.y),
        })
    }
}

impl Interactive for RectM2 {
    fn name(&self) -> &'static str {
        "RECTANG"
    }
    /// Asks for the pending option's value, or for the first and second points; in centre mode
    /// the first prompt asks for the centre instead of a corner.
    fn prompt(&self, _s: &Session) -> Prompt {
        let num = |m: &str, d: f64| Prompt::new(m, Accept::NUMBER).default(format!("{d:.4}"));
        if let Some(a) = self.asking {
            return match a {
                RAsk::Chamfer1 => num("Specify first chamfer distance for rectangles", self.chamfer.0),
                RAsk::Chamfer2 => num("Specify second chamfer distance for rectangles", self.chamfer.1),
                RAsk::Fillet => num("Specify fillet radius for rectangles", self.fillet),
                RAsk::Width => num("Specify line width for rectangles", self.width),
                RAsk::Elevation => num("Specify the elevation for rectangles", self.elevation),
                RAsk::Thickness => num("Specify thickness for rectangles", self.thickness),
                RAsk::Area => num("Enter area of rectangle in current units", if self.area > 0.0 { self.area } else { 100.0 }),
                RAsk::AreaBasis => Prompt::new("Calculate rectangle dimensions based on", curves::KW).kw(&["Length", "Width"]).default("Length"),
                RAsk::AreaLength => num("Enter rectangle length", 10.0),
                RAsk::AreaWidth => num("Enter rectangle width", 10.0),
                RAsk::DimL => num("Specify length for rectangles", self.dims.map(|d| d.0).unwrap_or(10.0)),
                RAsk::DimW => num("Specify width for rectangles", self.dims.map(|d| d.1).unwrap_or(10.0)),
                RAsk::Rotation => Prompt::new("Specify rotation angle or", Accept::POINT_OR_NUMBER)
                    .kw(&["Pick points"])
                    .default(format!("{:.0}", self.rotation.to_degrees()))
                    .base_opt(self.first),
                RAsk::RotPick1 => Prompt::new("Specify first point", Accept::POINT),
                RAsk::RotPick2(b) => Prompt::new("Specify second point", Accept::POINT).base(b),
            };
        }
        match self.first {
            None => {
                let msg = if self.center { "Specify center point for rectangle" } else { "Specify first corner point" };
                Prompt::new(msg, Accept::POINT).kw(&["Chamfer", "CEnter", "Elevation", "Fillet", "Thickness", "Width"])
            }
            Some(p) => Prompt::new("Specify other corner point", Accept::POINT).kw(&["Area", "Dimensions", "Rotation"]).corner(p),
        }
    }
    /// Consumes one input: option keywords (`Center` only switches the first point's meaning
    /// to the centre and asks nothing), option values, and the points that size the rectangle.
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(a) = self.asking {
            let val = |i: &Input, default: f64| -> Result<f64> {
                match i {
                    Input::Text(t) => number(t).ok_or_else(|| other("Requires a number.")),
                    Input::Enter => Ok(default),
                    _ => Err(other("Requires a number.")),
                }
            };
            self.asking = None;
            match a {
                RAsk::Chamfer1 => {
                    self.chamfer.0 = val(&i, self.chamfer.0)?.max(0.0);
                    self.chamfer.1 = self.chamfer.0;
                    self.asking = Some(RAsk::Chamfer2);
                }
                RAsk::Chamfer2 => self.chamfer.1 = val(&i, self.chamfer.1)?.max(0.0),
                RAsk::Fillet => self.fillet = val(&i, self.fillet)?.max(0.0),
                RAsk::Width => self.width = val(&i, self.width)?.max(0.0),
                RAsk::Elevation => self.elevation = val(&i, self.elevation)?,
                RAsk::Thickness => self.thickness = val(&i, self.thickness)?,
                RAsk::Area => {
                    let v = val(&i, if self.area > 0.0 { self.area } else { 100.0 })?;
                    if v <= 0.0 {
                        self.asking = Some(RAsk::Area);
                        return Err(other("Value must be positive."));
                    }
                    self.area = v;
                    self.asking = Some(RAsk::AreaBasis);
                }
                RAsk::AreaBasis => {
                    let w = matches!(&i, Input::Keyword(k) | Input::Text(k) if k.to_ascii_lowercase().starts_with('w'));
                    self.asking = Some(if w { RAsk::AreaWidth } else { RAsk::AreaLength });
                }
                RAsk::AreaLength | RAsk::AreaWidth => {
                    let v = val(&i, 10.0)?;
                    if v <= 0.0 {
                        self.asking = Some(a);
                        return Err(other("Value must be positive."));
                    }
                    let (w, h) = if a == RAsk::AreaLength { (v, self.area / v) } else { (self.area / v, v) };
                    return self.make(s, w, h);
                }
                RAsk::DimL => {
                    let l = val(&i, self.dims.map(|d| d.0).unwrap_or(10.0))?.abs();
                    self.dims = Some((l, self.dims.map(|d| d.1).unwrap_or(10.0)));
                    self.asking = Some(RAsk::DimW);
                }
                RAsk::DimW => {
                    let w = val(&i, self.dims.map(|d| d.1).unwrap_or(10.0))?.abs();
                    self.dims = Some((self.dims.map(|d| d.0).unwrap_or(10.0), w));
                }
                RAsk::Rotation => match i {
                    Input::Keyword(_) => self.asking = Some(RAsk::RotPick1),
                    Input::Point(p) => self.rotation = self.first.map(|f| f.angle_to(p)).unwrap_or(0.0),
                    Input::Text(t) => self.rotation = s.angle_settings().direction(&t).ok_or_else(|| other("Requires a valid angle."))?,
                    _ => {}
                },
                RAsk::RotPick1 => {
                    if let Input::Point(p) = i {
                        self.asking = Some(RAsk::RotPick2(p));
                    }
                }
                RAsk::RotPick2(b) => {
                    if let Input::Point(p) = i {
                        self.rotation = b.angle_to(p);
                    }
                }
            }
            return Ok(Step::Continue);
        }
        match i {
            Input::Keyword(k) => {
                // `Center` only switches the first point's meaning; it asks nothing.
                if k == "CEnter" {
                    self.center = true;
                    return Ok(Step::Continue);
                }
                self.asking = Some(match k.as_str() {
                    "Chamfer" => RAsk::Chamfer1,
                    "Elevation" => RAsk::Elevation,
                    "Fillet" => RAsk::Fillet,
                    "Thickness" => RAsk::Thickness,
                    "Width" => RAsk::Width,
                    "Area" => RAsk::Area,
                    "Dimensions" => RAsk::DimL,
                    _ => RAsk::Rotation,
                });
                Ok(Step::Continue)
            }
            Input::Point(p) => match self.first {
                None => {
                    self.first = Some(p);
                    Ok(Step::Continue)
                }
                Some(_) => {
                    let (w, h) = self.other_corner(p).ok_or_else(|| other("No first corner."))?;
                    self.make(s, w, h)
                }
            },
            Input::Enter => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
    /// The rubber-band rectangle for the cursor at `c`, mapped through the same size-and-origin
    /// rules as the committed entity so centre mode previews where it will land.
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.asking.is_some() {
            return Vec::new();
        }
        match self.other_corner(c).and_then(|(w, h)| self.origin(w, h).map(|a| (a, w, h))) {
            Some((a, w, h)) if w.abs() > 1e-12 && h.abs() > 1e-12 => {
                vec![rect_kind(a, (w, h), self.rotation, self.fillet, self.chamfer, self.width, self.elevation)]
            }
            _ => Vec::new(),
        }
    }
}

// =====================================================================================
// 3DPOLY
// =====================================================================================

fn point3_value(v: &Value) -> Option<Vec3> {
    let a = v.as_array()?;
    let x = a.first()?.as_f64()?;
    let y = a.get(1)?.as_f64()?;
    let z = a.get(2).and_then(Value::as_f64).unwrap_or(0.0);
    let p = Vec3::new(x, y, z);
    p.is_finite().then_some(p)
}

fn run_3dpoly(s: &mut Session, p: &Value) -> Result<Value> {
    let pts: Vec<Vec3> = p.get("points").and_then(Value::as_array).map(|a| a.iter().filter_map(point3_value).collect()).unwrap_or_default();
    if pts.len() < 2 || pts.len() > MAX_GEN {
        return Err(bad("3dpoly", "`points` needs 2 or more [x,y,z] points"));
    }
    added(s.add_entity(EntityKind::Polyline3d(Polyline3d { points: pts, closed: bool_or(p, "closed", false) }))?)
}

#[derive(Default)]
struct Poly3dM {
    pts: Vec<Vec2>,
    handle: Option<Handle>,
}

impl Poly3dM {
    fn sync(&mut self, s: &mut Session, closed: bool) -> Result<()> {
        let k = EntityKind::Polyline3d(Polyline3d { points: self.pts.iter().map(|p| v3(*p)).collect(), closed });
        match (self.handle, self.pts.len() >= 2) {
            (Some(h), true) => s.doc_mut()?.modify_entity(h, |e| e.kind = k)?,
            (None, true) => self.handle = Some(s.add_entity(k)?),
            (Some(h), false) => {
                s.doc_mut()?.remove_entity(h);
                self.handle = None;
            }
            (None, false) => {}
        }
        Ok(())
    }
}

impl Interactive for Poly3dM {
    fn name(&self) -> &'static str {
        "3DPOLY"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.pts.len() {
            0 => Prompt::new("Specify start point of polyline", Accept::POINT),
            1 | 2 => Prompt::new("Specify endpoint of line", Accept::POINT).kw(&["Undo"]).base_opt(self.pts.last().copied()),
            _ => Prompt::new("Specify endpoint of line", Accept::POINT).kw(&["Close", "Undo"]).base_opt(self.pts.last().copied()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                if self.pts.last().is_none_or(|l| !l.near(p, 1e-12)) && self.pts.len() < MAX_GEN {
                    self.pts.push(p);
                    self.sync(s, false)?;
                }
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Undo" => {
                self.pts.pop();
                self.sync(s, false)?;
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Close" => {
                self.sync(s, true)?;
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        self.pts.last().map(|l| vec![line(*l, c)]).unwrap_or_default()
    }
}

// =====================================================================================
// HELIX (3D polyline approximation)
// =====================================================================================

/// A helix entity (base centre `c`, radii, height, turns, twist, start angle).
fn helix_kind(c: Vec2, rb: f64, rt: f64, height: f64, turns: f64, ccw: bool, a0: f64) -> EntityKind {
    let h = Helix { center: c.to3(0.0), base_radius: rb, top_radius: rt, height, turns, start_angle: a0, ccw, constrain: HelixConstrain::Height };
    EntityKind::Helix(h.sanitized())
}

fn run_helix(s: &mut Session, p: &Value) -> Result<Value> {
    let c = point_req("helix", p, "center")?;
    let rb = f64_req("helix", p, "baseRadius")?;
    let rt = f64_or(p, "topRadius", rb);
    let turns = f64_or(p, "turns", 3.0);
    if !(rb > 0.0 && rt >= 0.0 && turns > 0.0 && turns <= 500.0) {
        return Err(bad("helix", "radii must be positive and turns in (0, 500]"));
    }
    let height = f64_or(p, "height", 1.0);
    if height.abs() > 1e12 || rb > 1e12 || rt > 1e12 {
        return Err(bad("helix", "sizes are out of range"));
    }
    added(s.add_entity(helix_kind(c, rb, rt, height, turns, bool_or(p, "ccw", true), f64_or(p, "startAngle", 0.0).to_radians()))?)
}

#[derive(Default)]
struct HelixM {
    center: Option<Vec2>,
    rb: Option<f64>,
    rt: Option<f64>,
    turns: Option<f64>,
    cw: bool,
    diameter: bool,
    asking: u8, // 0 main, 1 turns, 2 twist, 3 turn height
    start: Option<f64>,
}

impl HelixM {
    fn turns(&self) -> f64 {
        self.turns.unwrap_or(3.0)
    }
    fn create(&self, s: &mut Session, height: f64) -> Result<Step> {
        let (Some(c), Some(rb)) = (self.center, self.rb) else { return Ok(Step::Cancel) };
        let rt = self.rt.unwrap_or(rb);
        if !height.is_finite() || height.abs() > 1e12 {
            return Err(other("Requires a distance."));
        }
        s.add_entity(helix_kind(c, rb, rt, height, self.turns(), !self.cw, self.start.unwrap_or(0.0)))?;
        Ok(Step::Done)
    }
}

impl Interactive for HelixM {
    fn name(&self) -> &'static str {
        "HELIX"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.echo(format!("Number of turns = {:.4}     Twist=CCW", self.turns()));
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.asking {
            1 => return Prompt::new("Enter number of turns", Accept::NUMBER).default(format!("{:.4}", self.turns())),
            2 => return Prompt::new("Enter twist direction of helix", curves::KW).kw(&["CW", "CCW"]).default("CCW"),
            3 => return Prompt::new("Specify distance between turns", Accept::NUMBER),
            _ => {}
        }
        match (self.center, self.rb, self.rt) {
            (None, ..) => Prompt::new("Specify center point of base", Accept::POINT),
            (Some(c), None, _) => {
                Prompt::new(if self.diameter { "Specify base diameter" } else { "Specify base radius or" }, Accept::POINT_OR_NUMBER)
                    .kw(if self.diameter { &[] } else { &["Diameter"] })
                    .default("1.0000")
                    .base(c)
            }
            (Some(c), Some(rb), None) => {
                Prompt::new(if self.diameter { "Specify top diameter" } else { "Specify top radius or" }, Accept::POINT_OR_NUMBER)
                    .kw(if self.diameter { &[] } else { &["Diameter"] })
                    .default(format!("{rb:.4}"))
                    .base(c)
            }
            (Some(c), Some(_), Some(_)) => Prompt::new("Specify helix height or", Accept::POINT_OR_NUMBER)
                .kw(&["Axis endpoint", "Turns", "turn Height", "tWist"])
                .default("1.0000")
                .base(c),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.asking {
            1 => {
                if let Input::Text(t) = &i {
                    let v = number(t).filter(|v| *v > 0.0 && *v <= 500.0).ok_or_else(|| other("Turns must be in (0, 500]."))?;
                    self.turns = Some(v);
                }
                self.asking = 0;
                return Ok(Step::Continue);
            }
            2 => {
                if let Input::Keyword(k) | Input::Text(k) = &i {
                    self.cw = k.eq_ignore_ascii_case("cw");
                }
                self.asking = 0;
                return Ok(Step::Continue);
            }
            3 => {
                self.asking = 0;
                if let Input::Text(t) = &i {
                    let v = number(t).ok_or_else(|| other("Requires a distance."))?;
                    return self.create(s, v * self.turns());
                }
                return Ok(Step::Continue);
            }
            _ => {}
        }
        let num = |i: &Input, base: Option<Vec2>| -> Option<f64> {
            match i {
                Input::Point(p) => base.map(|b| b.dist(*p)),
                Input::Text(t) => number(t),
                _ => None,
            }
        };
        match (self.center, self.rb, self.rt, &i) {
            (None, _, _, Input::Point(p)) => {
                self.center = Some(*p);
                Ok(Step::Continue)
            }
            (Some(_), None | Some(_), _, Input::Keyword(k)) if k == "Diameter" => {
                self.diameter = true;
                Ok(Step::Continue)
            }
            (Some(c), None, _, _) => {
                let v = match &i {
                    Input::Enter => Some(1.0),
                    _ => num(&i, Some(c)),
                };
                let v = v.filter(|v| *v > 0.0).ok_or_else(|| other("Requires a positive distance."))?;
                if let Input::Point(p) = &i {
                    self.start = Some(c.angle_to(*p));
                }
                self.rb = Some(if self.diameter { v / 2.0 } else { v });
                self.diameter = false;
                Ok(Step::Continue)
            }
            (Some(c), Some(rb), None, _) => {
                let v = match &i {
                    Input::Enter => Some(if self.diameter { rb * 2.0 } else { rb }),
                    _ => num(&i, Some(c)),
                };
                let v = v.filter(|v| *v >= 0.0).ok_or_else(|| other("Requires a distance."))?;
                self.rt = Some(if self.diameter { v / 2.0 } else { v });
                Ok(Step::Continue)
            }
            (Some(c), Some(_), Some(_), _) => match &i {
                Input::Keyword(k) => {
                    self.asking = match k.as_str() {
                        "Turns" => 1,
                        "tWist" => 2,
                        "turn Height" => 3,
                        _ => 0,
                    };
                    Ok(Step::Continue)
                }
                Input::Enter => self.create(s, 1.0),
                _ => {
                    let h = num(&i, Some(c)).ok_or_else(|| other("Requires a distance."))?;
                    self.create(s, h)
                }
            },
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        match (self.center, self.rb, self.rt) {
            (Some(ct), None, _) => vec![circle(ct, ct.dist(c))],
            (Some(ct), Some(rb), None) => {
                vec![helix_kind(ct, rb, ct.dist(c), 0.0, self.turns(), !self.cw, self.start.unwrap_or(0.0))]
            }
            _ => Vec::new(),
        }
    }
}

// =====================================================================================
// SPLINE by control vertices
// =====================================================================================

fn run_spline_cv(s: &mut Session, p: &Value) -> Result<Value> {
    let mut c = points_param(p, "control").ok_or_else(|| bad("spline.cv", "`control` points are required"))?;
    if c.len() < 2 || c.len() > MAX_GEN {
        return Err(bad("spline.cv", "need 2+ control points"));
    }
    let closed = bool_or(p, "closed", false);
    if closed && let Some(f) = c.first().copied() {
        c.push(f);
    }
    let deg = p.get("degree").and_then(Value::as_u64).unwrap_or(3).clamp(1, 10) as usize;
    let mut sp = Spline::from_control(c, deg);
    sp.closed = closed;
    added(s.add_entity(EntityKind::Spline(sp))?)
}

#[derive(Default)]
struct SplineCvM {
    pts: Vec<Vec2>,
    degree: Option<usize>,
    asking_degree: bool,
}

impl Interactive for SplineCvM {
    fn name(&self) -> &'static str {
        "SPLINE"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.echo(format!("Current settings: Method=CV   Degree={}", self.degree.unwrap_or(3)));
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.asking_degree {
            return Prompt::new("Enter degree of spline", Accept::NUMBER).default(self.degree.unwrap_or(3).to_string());
        }
        match self.pts.len() {
            0 => Prompt::new("Specify first point", Accept::POINT).kw(&["Method", "Degree", "Object"]),
            1 => Prompt::new("Enter next point", Accept::POINT).base_opt(self.pts.last().copied()),
            2 => Prompt::new("Enter next point", Accept::POINT).kw(&["Undo"]).base_opt(self.pts.last().copied()),
            _ => Prompt::new("Enter next point", Accept::POINT).kw(&["Close", "Undo"]).base_opt(self.pts.last().copied()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.asking_degree {
            if let Input::Text(t) = &i {
                let d = number(t).filter(|d| (1.0..=10.0).contains(d)).ok_or_else(|| other("Degree must be 1..10."))?;
                self.degree = Some(d as usize);
            }
            self.asking_degree = false;
            return Ok(Step::Continue);
        }
        let deg = self.degree.unwrap_or(3);
        match i {
            Input::Point(p) => {
                if self.pts.len() < MAX_GEN {
                    self.pts.push(p);
                }
                Ok(Step::Continue)
            }
            Input::Keyword(k) => {
                match k.as_str() {
                    "Degree" => self.asking_degree = true,
                    "Undo" => {
                        self.pts.pop();
                    }
                    "Close" => {
                        let mut c = self.pts.clone();
                        c.extend(self.pts.first().copied());
                        let mut sp = Spline::from_control(c, deg);
                        sp.closed = true;
                        s.add_entity(EntityKind::Spline(sp))?;
                        return Ok(Step::Done);
                    }
                    _ => s.echo(format!("{k}: use SPLINE for fit points.")),
                }
                Ok(Step::Continue)
            }
            Input::Enter => {
                if self.pts.len() >= 2 {
                    s.add_entity(EntityKind::Spline(Spline::from_control(self.pts.clone(), deg)))?;
                }
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.pts.is_empty() {
            return Vec::new();
        }
        let mut pts = self.pts.clone();
        pts.push(c);
        vec![EntityKind::Spline(Spline::from_control(pts, self.degree.unwrap_or(3)))]
    }
}

// =====================================================================================
// CENTERMARK / CENTERLINE
// =====================================================================================

/// Add a line on the CENTER linetype (loading it from the standard library when missing).
fn add_center_line(s: &mut Session, a: Vec2, b: Vec2) -> Result<Handle> {
    let d = s.doc_mut()?;
    if d.linetype("CENTER").is_none()
        && let Some(lt) = cadcraft_doc::library::standard_linetypes_for(d).into_iter().find(|l| l.name == "CENTER")
    {
        d.linetypes.push(lt);
    }
    let h = s.add_entity(line(a, b))?;
    s.doc_mut()?.modify_entity(h, |e| e.common.linetype = "CENTER".into())?;
    Ok(h)
}

fn center_ext(s: &Session) -> f64 {
    let d = s.doc().ok();
    let metric = d.is_some_and(|d| d.header.i64("MEASUREMENT", 0) == 1);
    d.map(|d| d.header.f64("CENTEREXE", if metric { 3.5 } else { 0.12 })).unwrap_or(0.12)
}

fn center_mark(s: &mut Session, h: Handle) -> Result<Vec<String>> {
    let e = curves::entity(s, h)?;
    let (c, r) = match &e.kind {
        EntityKind::Circle(c) => (c.center.xy(), c.radius),
        EntityKind::Arc(a) => (a.center.xy(), a.radius),
        _ => return Err(other("Select a circle or arc.")),
    };
    let ext = r + center_ext(s);
    let h1 = add_center_line(s, c - Vec2::X * ext, c + Vec2::X * ext)?;
    let h2 = add_center_line(s, c - Vec2::Y * ext, c + Vec2::Y * ext)?;
    Ok(vec![h1.hex(), h2.hex()])
}

fn center_line(s: &mut Session, h1: Handle, h2: Handle) -> Result<Handle> {
    let l = |h: Handle| -> Result<(Vec2, Vec2)> {
        match curves::entity(s, h)?.kind {
            EntityKind::Line(l) => Ok((l.a.xy(), l.b.xy())),
            _ => Err(other("Select a line.")),
        }
    };
    let (a1, b1) = l(h1)?;
    let (mut a2, mut b2) = l(h2)?;
    if (b1 - a1).dot(b2 - a2) < 0.0 {
        std::mem::swap(&mut a2, &mut b2);
    }
    let (st, en) = (a1.mid(a2), b1.mid(b2));
    let dir = (en - st).normalized();
    if dir == Vec2::ZERO {
        return Err(other("The lines do not define a center line."));
    }
    let ext = center_ext(s);
    add_center_line(s, st - dir * ext, en + dir * ext)
}

fn run_centermark(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    if hs.is_empty() {
        return Err(bad("centermark", "`handle` or `handles` is required"));
    }
    let mut out = Vec::new();
    for h in hs {
        out.extend(center_mark(s, h)?);
    }
    Ok(json!({ "handles": out }))
}

fn run_centerline(s: &mut Session, p: &Value) -> Result<Value> {
    let h1 = curves::handle_param(p, "h1").ok_or_else(|| bad("centerline", "`h1` is required"))?;
    let h2 = curves::handle_param(p, "h2").ok_or_else(|| bad("centerline", "`h2` is required"))?;
    added(center_line(s, h1, h2)?)
}

struct CenterM {
    first: Option<Handle>,
    mark: bool,
}

impl Interactive for CenterM {
    fn name(&self) -> &'static str {
        if self.mark { "CENTERMARK" } else { "CENTERLINE" }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.set_selection(Vec::new());
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (self.mark, self.first) {
            (true, _) => Prompt::new("Select circle or arc to add center mark", Accept::POINT),
            (false, None) => Prompt::new("Select first line", Accept::POINT),
            (false, Some(_)) => Prompt::new("Select second line", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                let h = curves::pick_at(s, p).ok_or_else(|| other("*Invalid selection*"))?;
                if self.mark {
                    center_mark(s, h)?;
                    return Ok(Step::Continue);
                }
                match self.first {
                    None => {
                        if !matches!(curves::entity(s, h)?.kind, EntityKind::Line(_)) {
                            return Err(other("Select a line."));
                        }
                        self.first = Some(h);
                        Ok(Step::Continue)
                    }
                    Some(h1) => {
                        center_line(s, h1, h)?;
                        Ok(Step::Done)
                    }
                }
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

#[cfg(test)]
#[path = "draw2_tests.rs"]
mod tests;
