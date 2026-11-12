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

mod arc;
mod centermark;
mod curves3d;
mod divide;
mod ellipse;
mod rect;
mod revcloud;
mod tancircle;
mod wipeout;
mod xline;

pub(crate) use arc::arc_machine;
use arc::{ArcContinueM, ArcV, run_arc_continue, run_arc_variant, seq};
use centermark::{CenterM, run_centerline, run_centermark};
use curves3d::{HelixM, Poly3dM, SplineCvM, run_3dpoly, run_helix, run_spline_cv};
use divide::{DivideM, run_divide, run_measure};
use ellipse::{run_ellipse_arc, run_ellipse_axis};
pub(crate) use rect::{RectM2, run_rectang2};
use revcloud::{CloudKind, RevcloudM, run_revcloud};
pub(crate) use tancircle::TanCircleM;
use tancircle::{run_circle_ttr, run_circle_ttt, run_circle_variant};
use wipeout::{WipeoutM, run_wipeout};
pub(crate) use xline::{XlineM2, run_xline2};

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
            .params("{p1, p2, distance} | {p1, p2, p3} (axis endpoints, then half the other axis: a distance, or a point that far from the center)")
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

#[cfg(test)]
mod tests;
