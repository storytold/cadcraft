//! MLEADER's leader types (straight, spline, none) and block content: applied to the multileader
//! `mleader_kind` builds, for the prompt machine and the JSON form.

use cadcraft_doc::{Entity, EntityKind, Handle, Insert};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::Value;

use super::{bad, str_param};
use crate::{EngineError, LeaderType, Result, Session};

/// How the multileader differs from a straight leader with text.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Content {
    pub leader: LeaderType,
    /// Block content: the block name, and whether its extents centre (rather than its insertion
    /// point) attaches to the end of the landing.
    pub block: Option<(String, bool)>,
}

/// A block MLEADER can use as content: defined, and not an anonymous (`*`) block.
pub(super) fn block_name(s: &Session, name: &str) -> Result<String> {
    block_name_in(s.doc()?, name)
}

/// [`block_name`] in drawing `d`.
pub(super) fn block_name_in(d: &cadcraft_doc::Drawing, name: &str) -> Result<String> {
    let name = name.trim();
    match d.block(name) {
        Some(b) if !b.name.starts_with('*') => Ok(b.name.clone()),
        _ => Err(EngineError::Other(format!("Block \"{name}\" not found."))),
    }
}

/// Apply the leader type and block content to a multileader from `mleader_kind`.
pub(super) fn apply(s: &Session, k: EntityKind, c: &Content) -> Result<EntityKind> {
    let EntityKind::MLeader(mut m) = k else { return Ok(k) };
    m.spline = c.leader == LeaderType::Spline;
    let land = m.landing.xy();
    // The side the content sits on, as `mleader_kind` chose it for the text.
    let dir = if m.leaders.first().and_then(|l| l.first()).is_some_and(|a| a.x > land.x) { -1.0 } else { 1.0 };
    if c.leader == LeaderType::None {
        // Content only, at the picked location (text from its top left corner).
        m.leaders.clear();
        m.roots.clear();
        m.dogleg = 0.0;
        if let Some(t) = &mut m.text {
            t.insert = m.landing;
            t.attach = 1;
        }
    }
    if let Some((name, center)) = &c.block {
        let anchor = land + Vec2::new(m.dogleg * dir, 0.0);
        // The style's block scale and rotation.
        let st = super::annotate::current_mleader_style(s);
        let k = if st.block_scale.is_finite() && st.block_scale > 0.0 { st.block_scale } else { 1.0 };
        let mut ins = Insert {
            block: name.clone(),
            insert: Vec3::new(anchor.x, anchor.y, m.landing.z),
            scale: Vec3::new(k, k, k),
            rotation: if st.block_rotation.is_finite() { st.block_rotation } else { 0.0 },
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        };
        if *center {
            // Move the block so the centre of its extents lands on the anchor.
            let d = s.doc()?;
            let probe = Entity { handle: Handle(0), common: Default::default(), kind: EntityKind::Insert(ins.clone()) };
            let b = cadcraft_doc::entity_bounds(d, &probe, 0);
            if !b.is_empty() {
                let c = b.center();
                if c.is_finite() {
                    ins.insert = Vec3::new(ins.insert.x + anchor.x - c.x, ins.insert.y + anchor.y - c.y, ins.insert.z);
                }
            }
        }
        m.text = None;
        m.block = Some(ins);
    }
    Ok(EntityKind::MLeader(m))
}

/// `leaderType` ("straight" | "spline" | "none"), `block` (a block name) and `blockAttach`
/// ("center" | "insertion") from the JSON form.
pub(super) fn json_content(s: &Session, p: &Value) -> Result<Content> {
    // Without them, the current multileader style's leader type and block content.
    let st = super::annotate::current_mleader_style(s);
    let leader = match str_param(p, "leaderType").map(str::to_ascii_lowercase).as_deref() {
        None => match st.leader_type {
            cadcraft_doc::MLeaderLineType::Straight => LeaderType::Straight,
            cadcraft_doc::MLeaderLineType::Spline => LeaderType::Spline,
            cadcraft_doc::MLeaderLineType::None => LeaderType::None,
        },
        Some("straight") => LeaderType::Straight,
        Some("spline") => LeaderType::Spline,
        Some("none") => LeaderType::None,
        Some(_) => return Err(bad("mleader", "`leaderType` must be \"straight\", \"spline\" or \"none\"")),
    };
    let block = match str_param(p, "block") {
        None if str_param(p, "text").is_none() && st.content == cadcraft_doc::MLeaderContent::Block => {
            block_name(s, &st.block).ok().map(|n| (n, st.block_center))
        }
        None => None,
        Some(n) => {
            let center = match str_param(p, "blockAttach").map(str::to_ascii_lowercase).as_deref() {
                None | Some("center") => true,
                Some("insertion") => false,
                Some(_) => return Err(bad("mleader", "`blockAttach` must be \"center\" or \"insertion\"")),
            };
            Some((block_name(s, n).map_err(|e| bad("mleader", e.to_string()))?, center))
        }
    };
    Ok(Content { leader, block })
}
