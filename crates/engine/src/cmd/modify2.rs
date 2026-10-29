//! Modify menu depth (M2): PEDIT, LENGTHEN, ALIGN, BLEND, REVERSE, SPLINEDIT, NCOPY, MOCORO, CHSPACE,
//! FLATTEN, plus the fillet/trim/extend extensions used by modify.rs (arcs, polylines,
//! ellipses and splines).

use cadcraft_doc::{Common, Entity, EntityKind, Handle, LwPolyline, MAX_BLOCK_DEPTH, Space};
use cadcraft_geom::{
    Arc, Ellipse, Line, Mat3, PolyVertex, Polyline, Segment, Spline, TAU, Vec2, ccw_sweep, intersect_ext, line_circle, line_line_infinite, norm_angle,
};
use serde_json::{Value, json};

use super::curves::{self, Chain, MAX_GEN, TanObj};
use super::helpers::{arc, line, lwpoly, v3};

use super::machines::{SelOutcome, SelectPhase, number};
use super::{CommandSpec, bad, bool_or, f64_or, f64_req, point_param, point_req, str_param, targets};

use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

mod curve_edits;
mod fillet_pline;
mod mocoro;
pub(crate) use curve_edits::{closed_kind, join_close, join_curves};
pub(crate) use fillet_pline::{ChamferDistances, fillet_pline};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("pedit", "Polyline", run_pedit)
            .menu(&["Modify", "Object", "Polyline"])
            .alias(&["pe"])
            .params(
                "{handle | handles, option: close|open|join|width|fit|spline|decurve|reverse|ltypegen|vertex, width?, handles2? (join), fuzz?, \
                 action?: move|insert|straighten|break|width|regen, index?, index2? (straighten, break), to?, startWidth?, endWidth?, \
                 tangents?: [{index, angle (deg) | direction}] (fit), convert?: bool}",
            )
            .interactive(|_| Ok(Box::new(PeditM::default()))),
        CommandSpec::new("splinedit", "Spline", run_splinedit)
            .menu(&["Modify", "Object", "Spline"])
            .alias(&["spe"])
            .params(
                "{handle, option: close|open|join|reverse|refit|purge|polyline|move|fitmove|fitadd|fitdelete|kink|tangents|tolerance|vertexadd|vertexmove|vertexdelete|weight|elevate, \
                 handles2? (join), fit?, index?, to?, before?: bool (fitadd), at? (kink, vertexadd), start?, end? (tangent directions; null = default), tolerance?, weight?, \
                 order? (elevate), precision?}",
            )
            .interactive(|_| Ok(Box::new(SplineditM::default()))),
        CommandSpec::new("lengthen", "Lengthen", run_lengthen)
            .menu(&["Modify", "Lengthen"])
            .alias(&["len"])
            .params("{handle, pick?: [x,y] (end), delta? | deltaAngle? (deg) | percent? | total? | totalAngle? (deg) | to?: [x,y]} ({handle} alone measures)")
            .interactive(|_| Ok(Box::new(LengthenM::default()))),
        CommandSpec::new("align", "Align", run_align)
            .menu(&["Modify", "3D Operations", "Align"])
            .alias(&["al"])
            .params("{handles?, s1, d1, s2?, d2?, scale?: bool}")
            .interactive(|_| Ok(Box::new(AlignM::default()))),
        CommandSpec::new("blend", "Blend", run_blend)
            .menu(&["Modify", "Blend"])
            .alias(&["blendcurves"])
            .params("{h1, p1, h2, p2, continuity?: tangent|smooth}")
            .interactive(|_| Ok(Box::new(BlendM::default()))),
        CommandSpec::new("reverse", "Reverse", run_reverse)
            .params("{handles?} (lines, polylines, 3D polylines, splines)")
            .interactive(|_| Ok(Box::new(SelOp::new(SelKind::Reverse)))),
        CommandSpec::new("ncopy", "Copy Nested Objects", run_ncopy)
            .menu(&["Modify", "Copy Nested Objects"])
            .params("{handle (block reference), pick?: [x,y] (nearest nested object; default all), delta? | from,to}")
            .interactive(|_| Ok(Box::new(NcopyM::default()))),
        CommandSpec::new("mocoro", "Move/Copy/Rotate", mocoro::run_mocoro)
            .menu(&["Tools", "Express Tools", "Move/Copy/Rotate"])
            .params("{handles?, base: [x,y], ops: [{move: [x,y]} | {copy: [x,y]} | {rotate: degrees} | {scale: factor} | {base: [x,y]}]} → {handles (copies)}")
            .interactive(|_| Ok(Box::new(mocoro::MocoroM::default()))),
        CommandSpec::new("chspace", "Change Space", run_chspace)
            .menu(&["Modify", "Change Space"])
            .params("{handles?, to?: \"model\" | layout name, viewport?: handle}")
            .interactive(|_| Ok(Box::new(SelOp::new(SelKind::Chspace)))),
        CommandSpec::new("flatten", "Flatten Objects", run_flatten)
            .menu(&["Modify", "Flatten Objects"])
            .params("{handles?} (z = 0, 3D polylines become polylines)")
            .interactive(|_| Ok(Box::new(SelOp::new(SelKind::Flatten)))),
        CommandSpec::new("arraypath", "Path Array", run_arraypath)
            .menu(&["Modify", "Array", "Path Array"])
            .params("{handles?, path (handle), count? (default 6) | spacing?, align?: bool (default true)}")
            .interactive(|_| Ok(Box::new(PathArrayM::default()))),
        CommandSpec::new("textedit", "Edit Text...", run_textedit)
            .menu(&["Modify", "Object", "Edit Text..."])
            .alias(&["ed", "ddedit"])
            .params("{handle, text} (text, mtext, attribute definitions, dimension text override)")
            .interactive(|_| Ok(Box::new(TextEditM::default()))),
    ]
}

fn other(m: impl Into<String>) -> EngineError {
    EngineError::Other(m.into())
}

fn set_kind(s: &mut Session, h: Handle, k: EntityKind) -> Result<()> {
    s.doc_mut()?.modify_entity(h, |e| e.kind = k)?;
    Ok(())
}

fn push_entity(s: &mut Session, space: &Space, common: Common, kind: EntityKind) -> Result<Handle> {
    let d = s.doc_mut()?;
    d.ensure_layer(&common.layer);
    let h = d.new_handle();
    let st = d.space_mut(space).ok_or_else(|| other("no such space"))?;
    st.push(Entity { handle: h, common, kind });
    Ok(h)
}

// =====================================================================================
// PEDIT
// =====================================================================================

/// The polyline form of a line, arc or polyline.
pub(crate) fn as_lwpoly(k: &EntityKind) -> Option<LwPolyline> {
    match k {
        EntityKind::LwPolyline(p) => Some(p.clone()),
        EntityKind::Line(_) | EntityKind::Arc(_) => {
            let c = Chain::of(k)?;
            Some(LwPolyline { vertices: c.vertices(), closed: false, const_width: 0.0, elevation: 0.0, plinegen: false })
        }
        _ => None,
    }
}

mod pedit_vertex;

// =====================================================================================
// SPLINEDIT
// =====================================================================================

mod splinedit;
use splinedit::{SplineditM, run_splinedit};

mod align;
mod blend;
mod fillet;
mod lengthen;
mod ncopy;
mod patharray;
mod pedit;
mod selop;
mod textedit;
mod trimext;

use align::{AlignM, run_align};
use blend::{BlendM, run_blend};
use fillet::trim_for_fillet;
pub(crate) use fillet::{fillet_curves, fillet_polyline};
pub(crate) use lengthen::LenMode;
use lengthen::{LengthenM, run_lengthen};
use ncopy::{NcopyM, run_ncopy};
pub(super) use patharray::PathArrayM;
use patharray::run_arraypath;
use pedit::{PPhase, PeditM, run_pedit};
use selop::{SelKind, SelOp, run_chspace, run_flatten, run_reverse};
use textedit::{TextEditM, run_textedit};
use trimext::{ellipse_kind, ellipse_of};
pub(crate) use trimext::{extend_other, trim_other};

#[cfg(test)]
mod tests;
