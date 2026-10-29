//! Modify commands: ERASE, MOVE, COPY, ROTATE, SCALE, MIRROR, OFFSET, TRIM, EXTEND, FILLET,
//! CHAMFER, EXPLODE, STRETCH, BREAK, JOIN (arrays: `array`; draw order: `draworder`).

use cadcraft_doc::{Entity, EntityKind, Handle, Prim};
use cadcraft_geom::{
    Arc, Circle, Line, Mat3, PolyVertex, Polyline, Segment, TAU, Vec2, angle_in_sweep, ccw_sweep, intersect_ext, line_line_infinite, norm_angle,
};
use serde_json::{Value, json};

use super::helpers::{arc, circle, line, lwpoly, v3};

use super::machines::{SelOutcome, SelectPhase, number};
use super::trimextend::{self, TrimM};
use super::{CommandSpec, bad, bool_or, f64_or, f64_req, point_param, point_req, points_param, str_param, targets};

use crate::select::{SelectKind, SelectMode};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

mod break_join;
mod explode;
mod fillet;
mod offset;
mod select_then;
mod stretch;
mod transform;
mod trim;

use break_join::{BreakM, run_break, run_breakat, run_join};
use explode::run_explode;
pub(crate) use explode::{explode_kind, explode_with};
use fillet::{FilletM, run_chamfer, run_fillet};
use offset::{OffsetM, run_offset};
use select_then::{DisplaceM, Op, SelectThen};
use stretch::{StretchM, run_stretch};
pub(crate) use transform::transform_entities;
use transform::{run_copy, run_erase, run_mirror, run_move, run_rotate, run_scale};
pub(crate) use trim::{Edges, extend, trim};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("erase", "Erase", run_erase)
            .menu(&["Modify", "Erase"])
            .alias(&["e", "delete"])
            .params("{handles?: [hex]} (default: selection)")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Erase)))),
        CommandSpec::new("move", "Move", run_move)
            .menu(&["Modify", "Move"])
            .alias(&["m"])
            .params("{handles?, from?: [x,y], to?: [x,y] | delta: [dx,dy]}")
            .interactive(|_| Ok(Box::new(DisplaceM::new(Op::Move)))),
        CommandSpec::new("copy", "Copy", run_copy)
            .menu(&["Modify", "Copy"])
            .alias(&["co", "cp"])
            .params("{handles?, delta: [dx,dy] | from,to, count?: n}")
            .interactive(|_| Ok(Box::new(DisplaceM::new(Op::Copy)))),
        CommandSpec::new("rotate", "Rotate", run_rotate)
            .menu(&["Modify", "Rotate"])
            .alias(&["ro"])
            .params("{handles?, base: [x,y], angle (degrees), copy?: bool}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Rotate)))),
        CommandSpec::new("scale", "Scale", run_scale)
            .menu(&["Modify", "Scale"])
            .alias(&["sc"])
            .params("{handles?, base: [x,y], factor, copy?: bool}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Scale)))),
        CommandSpec::new("mirror", "Mirror", run_mirror)
            .menu(&["Modify", "Mirror"])
            .alias(&["mi"])
            .params("{handles?, p1, p2, erase?: bool}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Mirror)))),
        CommandSpec::new("stretch", "Stretch", run_stretch)
            .menu(&["Modify", "Stretch"])
            .alias(&["s"])
            .params("{window: [[x,y],[x,y]] | polygon: [[x,y],...] | fence: [[x,y],...], delta: [dx,dy]}")
            .interactive(|_| Ok(Box::new(StretchM::default()))),
        CommandSpec::new("offset", "Offset", run_offset)
            .menu(&["Modify", "Offset"])
            .alias(&["o"])
            .params("{handle, distance, side: [x,y], erase?: bool, layer?: \"source\"|\"current\"}")
            .interactive(|s| Ok(Box::new(OffsetM::new(s)))),
        CommandSpec::new("trim", "Trim", trimextend::run_trim)
            .menu(&["Modify", "Trim"])
            .alias(&["tr"])
            .params(
                "{handle, pick: [x,y] | fence: [[x,y],...] | crossing: [[x,y],[x,y]], edges?: [hex], edgeMode?: \"extend\"|\"none\", mode?: \"quick\"|\"standard\"}",
            )
            .interactive(|_| Ok(Box::new(TrimM::new(false)))),
        CommandSpec::new("extend", "Extend", trimextend::run_extend)
            .menu(&["Modify", "Extend"])
            .alias(&["ex"])
            .params("{handle, pick: [x,y] | fence: [[x,y],...] | crossing: [[x,y],[x,y]], edges?: [hex], edgeMode?: \"extend\"|\"none\"}")
            .interactive(|_| Ok(Box::new(TrimM::new(true)))),
        CommandSpec::new("fillet", "Fillet", run_fillet)
            .menu(&["Modify", "Fillet"])
            .alias(&["f"])
            .params("{h1, p1, h2, p2, radius?, trim?: bool} (lines, arcs, circles) | {handle, polyline: true, radius?}")
            .interactive(|s| Ok(Box::new(FilletM::new(s, false)))),
        CommandSpec::new("chamfer", "Chamfer", run_chamfer)
            .menu(&["Modify", "Chamfer"])
            .alias(&["cha"])
            .params("{h1, p1, h2, p2, d1?, d2? | length, angle (degrees), trim?: bool} | {handle, polyline: true, d1?, d2? | length, angle}")
            .interactive(|s| Ok(Box::new(FilletM::new(s, true)))),
        CommandSpec::new("explode", "Explode", run_explode)
            .menu(&["Modify", "Explode"])
            .alias(&["x"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Explode)))),
        CommandSpec::new("break", "Break", run_break)
            .menu(&["Modify", "Break"])
            .alias(&["br"])
            .params("{handle, p1, p2}")
            .interactive(|_| Ok(Box::new(BreakM::default()))),
        CommandSpec::new("breakatpoint", "Break At Point", run_breakat).menu(&["Modify", "Break At Point"]).params("{handle, at}"),
        CommandSpec::new("join", "Join", run_join)
            .menu(&["Modify", "Join"])
            .alias(&["j"])
            .params("{handles?, close?: bool (a single arc becomes its circle, an elliptical arc its full ellipse)}")
            .interactive(|_| Ok(Box::new(SelectThen::new(Op::Join)))),
    ]
}

pub(crate) fn _unused(_: Circle) {}
