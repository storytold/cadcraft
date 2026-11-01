//! Dimensions and leaders: DIMLINEAR, DIMALIGNED, DIMRADIUS, DIMDIAMETER, DIMANGULAR, DIMARC,
//! DIMORDINATE, DIMCONTINUE, DIMBASELINE, QDIM, DIM (smart), MLEADER, LEADER, DIMOVERRIDE,
//! DIMREASSOCIATE / DIMDISASSOCIATE, DIMTEDIT, DIMSPACE; DIMEDIT, DIMJOGGED, DIMJOGLINE,
//! DIMBREAK and DIMINSPECT in `annotate_dimedit.rs`.

use cadcraft_doc::{AssocSnap, DimAssoc, DimKind, Dimension, EntityKind, Handle, MLeader, MText, Prim};
use cadcraft_geom::{Segment, Vec2, Vec3};
use serde_json::{Value, json};

use super::helpers::v3;
use super::machines::{SelOutcome, SelectPhase, SelectRun};
use super::{CommandSpec, bad, point_param, point_req, points_param, str_param, targets};

use crate::{Accept, EngineError, Input, Interactive, LeaderOrder, MLeaderOptions, Prompt, Result, Session, Step};

mod angular;
mod common;
mod edit;
mod linear;
mod mleader;
mod radial;

use angular::{AngularM, ArcLenM, run_angular, run_arclen};
pub(super) use common::segment_at;
use common::{DimText, add_dim, assoc_enabled, circle_of, dim, pick_at};
use edit::{DimSpaceM, OverrideM, dims_of, run_dimspace, run_dimtedit, run_disassociate, run_override, run_reassociate, run_tedit, run_update};
use linear::{ChainM, LinearM, MAX_CHAIN, OrdinateM, run_aligned, run_baseline, run_continue, run_linear, run_ordinate};
use mleader::{MLeaderM, run_leader, run_mleader};
use radial::{RadialM, run_diameter, run_radius};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("dimlinear", "Linear", run_linear)
            .menu(&["Dimension", "Linear"])
            .alias(&["dli", "dimlin"])
            .params("{p1, p2, at, rotation? (degrees; default: horizontal/vertical from `at`), text?} | {object: hex, at} (associative)")
            .interactive(|_| Ok(Box::new(LinearM::new(false)))),
        CommandSpec::new("dimaligned", "Aligned", run_aligned)
            .menu(&["Dimension", "Aligned"])
            .alias(&["dal", "dimali"])
            .params("{p1, p2, at, text?}")
            .interactive(|_| Ok(Box::new(LinearM::new(true)))),
        CommandSpec::new("dimradius", "Radius", run_radius)
            .menu(&["Dimension", "Radius"])
            .alias(&["dra", "dimrad"])
            .params("{handle, at?} | {center, point}")
            .interactive(|_| Ok(Box::new(RadialM::new(false)))),
        CommandSpec::new("dimdiameter", "Diameter", run_diameter)
            .menu(&["Dimension", "Diameter"])
            .alias(&["ddi", "dimdia"])
            .params("{handle, at?} | {center, point}")
            .interactive(|_| Ok(Box::new(RadialM::new(true)))),
        CommandSpec::new("dimangular", "Angular", run_angular)
            .menu(&["Dimension", "Angular"])
            .alias(&["dan", "dimang"])
            .params("{vertex, p1, p2, at} | {lines: [hex, hex], at} | {arc: hex, at} (associative)")
            .interactive(|_| Ok(Box::new(AngularM::default()))),
        CommandSpec::new("dimarc", "Arc Length", run_arclen)
            .menu(&["Dimension", "Arc Length"])
            .alias(&["dar"])
            .params("{handle, at}")
            .interactive(|_| Ok(Box::new(ArcLenM::default()))),
        CommandSpec::new("dimordinate", "Ordinate", run_ordinate)
            .menu(&["Dimension", "Ordinate"])
            .alias(&["dor", "dimord"])
            .params("{feature, leader, xtype?: bool}")
            .interactive(|_| Ok(Box::new(OrdinateM::default()))),
        CommandSpec::new("dimcontinue", "Continue", run_continue)
            .menu(&["Dimension", "Continue"])
            .alias(&["dco", "dimcont"])
            .params("{points: [[x,y]...]} (continues the last linear dimension)")
            .interactive(|s| Ok(Box::new(ChainM::new(s, false)))),
        CommandSpec::new("dimbaseline", "Baseline", run_baseline)
            .menu(&["Dimension", "Baseline"])
            .alias(&["dba", "dimbase"])
            .params("{points: [[x,y]...]} (baseline from the last linear dimension)")
            .interactive(|s| Ok(Box::new(ChainM::new(s, true)))),
        CommandSpec::new("qdim", "Quick Dimension", qdim::run)
            .menu(&["Dimension", "Quick Dimension"])
            .params(qdim::PARAMS)
            .interactive(|_| Ok(Box::new(qdim::QDimM::new()))),
        CommandSpec::new("dim", "Dimension", run_linear)
            .menu(&["Dimension", "Dimension"])
            .params("{p1, p2, at}")
            .interactive(|_| Ok(Box::new(LinearM::new(false)))),
        CommandSpec::new("mleader", "Multileader", run_mleader)
            .menu(&["Dimension", "Multileader"])
            .alias(&["mld"])
            .params("{points: [[arrow], ..., [landing]] ([location] with leaderType none), text?, leaderType?: straight|spline|none, block?: name, blockAttach?: center|insertion}")
            .interactive(|s| Ok(Box::new(MLeaderM::new(s)))),
        CommandSpec::new("leader", "Leader", run_leader).alias(&["lead"]).params("{points: [[x,y]...], text?}"),
        CommandSpec::new("dimstyle.update", "Update", run_update)
            .menu(&["Dimension", "Update"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectRun::new("dimstyle.update", "DIMSTYLE")))),
        CommandSpec::new("dimoverride", "Override", run_override)
            .menu(&["Dimension", "Override"])
            .alias(&["dov", "dimover"])
            .params("{handles?, <DimStyle fields or DIM* variables: values>, clear?: bool, text?: override text}")
            .interactive(|s| Ok(Box::new(OverrideM::new(s)))),
        CommandSpec::new("dimreassociate", "Reassociate Dimensions", run_reassociate)
            .menu(&["Dimension", "Reassociate Dimensions"])
            .alias(&["dre"])
            .params("{handles?} (dimensions; attaches their definition points to the objects under them)")
            .interactive(|_| Ok(Box::new(SelectRun::new("dimreassociate", "DIMREASSOCIATE")))),
        CommandSpec::new("dimdisassociate", "Disassociate Dimensions", run_disassociate)
            .alias(&["dda"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectRun::new("dimdisassociate", "DIMDISASSOCIATE")))),
        CommandSpec::new("dimtedit.home", "Home", |s, p| run_tedit(s, p, "home")).menu(&["Dimension", "Align Text", "Home"]).params("{handles?}"),
        CommandSpec::new("dimtedit.angle", "Angle", |s, p| run_tedit(s, p, "angle"))
            .menu(&["Dimension", "Align Text", "Angle"])
            .params("{handles?, angle: degrees}")
            .interactive(|_| Ok(Box::new(tedit::TEditM::angle()))),
        CommandSpec::new("dimtedit.left", "Left", |s, p| run_tedit(s, p, "left")).menu(&["Dimension", "Align Text", "Left"]).params("{handles?}"),
        CommandSpec::new("dimtedit.center", "Center", |s, p| run_tedit(s, p, "center"))
            .menu(&["Dimension", "Align Text", "Center"])
            .params("{handles?}"),
        CommandSpec::new("dimtedit.right", "Right", |s, p| run_tedit(s, p, "right")).menu(&["Dimension", "Align Text", "Right"]).params("{handles?}"),
        CommandSpec::new("dimtedit", "Dimension Text Edit", run_dimtedit)
            .alias(&["dimted"])
            .params("{handles?, at?: [x,y], mode?: home|angle|left|center|right, angle?}")
            .interactive(|_| Ok(Box::new(tedit::TEditM::default()))),
        CommandSpec::new("dimspace", "Dimension Space", run_dimspace)
            .menu(&["Dimension", "Dimension Space"])
            .params("{base: hex, handles: [hex], spacing?: number (default DIMDLI)}")
            .interactive(|_| Ok(Box::new(DimSpaceM::default()))),
    ]
    .into_iter()
    .chain(dimedit::specs())
    .collect()
}

/// QDIM: its modes and prompts.
#[path = "annotate_qdim.rs"]
mod qdim;

#[path = "annotate_tedit.rs"]
mod tedit;

#[path = "annotate_dimedit.rs"]
mod dimedit;

#[cfg(test)]
mod tests;
