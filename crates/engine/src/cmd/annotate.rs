//! Dimensions and leaders: DIMLINEAR, DIMALIGNED, DIMRADIUS, DIMDIAMETER, DIMANGULAR, DIMARC,
//! DIMORDINATE, DIMCONTINUE, DIMBASELINE, QDIM, DIM (smart), MLEADER, LEADER, DIMOVERRIDE,
//! DIMREASSOCIATE / DIMDISASSOCIATE, DIMTEDIT, DIMSPACE.

use cadcraft_doc::{AssocSnap, DimAssoc, DimKind, Dimension, EntityKind, Handle, MLeader, MText, Prim};
use cadcraft_geom::{Segment, Vec2, Vec3};
use serde_json::{Value, json};

use super::helpers::v3;
use super::*;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

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
            .interactive(|_| Ok(Box::new(RadialM { diameter: false, target: None }))),
        CommandSpec::new("dimdiameter", "Diameter", run_diameter)
            .menu(&["Dimension", "Diameter"])
            .alias(&["ddi", "dimdia"])
            .params("{handle, at?} | {center, point}")
            .interactive(|_| Ok(Box::new(RadialM { diameter: true, target: None }))),
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
        CommandSpec::new("qdim", "Quick Dimension", run_qdim)
            .menu(&["Dimension", "Quick Dimension"])
            .params("{handles?, at: [x,y], vertical?: bool}"),
        CommandSpec::new("dim", "Dimension", run_linear)
            .menu(&["Dimension", "Dimension"])
            .params("{p1, p2, at}")
            .interactive(|_| Ok(Box::new(LinearM::new(false)))),
        CommandSpec::new("mleader", "Multileader", run_mleader)
            .menu(&["Dimension", "Multileader"])
            .alias(&["mld"])
            .params("{points: [[arrow], ..., [landing]], text}")
            .interactive(|_| Ok(Box::new(MLeaderM::default()))),
        CommandSpec::new("leader", "Leader", run_leader).alias(&["lead"]).params("{points: [[x,y]...], text?}"),
        CommandSpec::new("dimstyle.update", "Update", run_update).menu(&["Dimension", "Update"]).params("{handles?}"),
        CommandSpec::new("dimoverride", "Override", run_override)
            .menu(&["Dimension", "Override"])
            .alias(&["dov", "dimover"])
            .params("{handles?, <DimStyle fields or DIM* variables: values>, clear?: bool, text?: override text}")
            .interactive(|s| Ok(Box::new(OverrideM::new(s)))),
        CommandSpec::new("dimreassociate", "Reassociate Dimensions", run_reassociate)
            .menu(&["Dimension", "Reassociate Dimensions"])
            .alias(&["dre"])
            .params("{handles?} (dimensions; attaches their definition points to the objects under them)")
            .interactive(|s| Ok(Box::new(SelectThen::new(s, "dimreassociate")))),
        CommandSpec::new("dimdisassociate", "Disassociate Dimensions", run_disassociate)
            .alias(&["dda"])
            .params("{handles?}")
            .interactive(|s| Ok(Box::new(SelectThen::new(s, "dimdisassociate")))),
        CommandSpec::new("dimtedit.home", "Home", |s, p| run_tedit(s, p, "home")).menu(&["Dimension", "Align Text", "Home"]).params("{handles?}"),
        CommandSpec::new("dimtedit.angle", "Angle", |s, p| run_tedit(s, p, "angle"))
            .menu(&["Dimension", "Align Text", "Angle"])
            .params("{handles?, angle: degrees}"),
        CommandSpec::new("dimtedit.left", "Left", |s, p| run_tedit(s, p, "left")).menu(&["Dimension", "Align Text", "Left"]).params("{handles?}"),
        CommandSpec::new("dimtedit.center", "Center", |s, p| run_tedit(s, p, "center"))
            .menu(&["Dimension", "Align Text", "Center"])
            .params("{handles?}"),
        CommandSpec::new("dimtedit.right", "Right", |s, p| run_tedit(s, p, "right")).menu(&["Dimension", "Align Text", "Right"]).params("{handles?}"),
        CommandSpec::new("dimtedit", "Dimension Text Edit", run_dimtedit)
            .params("{handles?, at?: [x,y], mode?: home|angle|left|center|right, angle?}"),
        CommandSpec::new("dimspace", "Dimension Space", run_dimspace)
            .menu(&["Dimension", "Dimension Space"])
            .params("{base: hex, handles: [hex], spacing?: number (default DIMDLI)}"),
    ]
}

fn style(s: &Session) -> String {
    s.doc().map(|d| d.header.str("DIMSTYLE", "Standard")).unwrap_or_else(|_| "Standard".into())
}

fn dim(s: &Session, kind: DimKind, defpt: Vec2, p13: Vec2, p14: Vec2, p15: Vec2, p16: Vec2, text: &str) -> EntityKind {
    EntityKind::Dimension(Dimension {
        kind,
        defpt: v3(defpt),
        text_mid: Vec3::ZERO,
        p13: v3(p13),
        p14: v3(p14),
        p15: v3(p15),
        p16: v3(p16),
        text: text.into(),
        style: style(s),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
    })
}

/// Horizontal (0) or vertical (90°) from where the dimension line is placed (AutoCAD's rule).
fn auto_rotation(p1: Vec2, p2: Vec2, at: Vec2) -> f64 {
    let (x0, x1) = (p1.x.min(p2.x), p1.x.max(p2.x));
    let (y0, y1) = (p1.y.min(p2.y), p1.y.max(p2.y));
    let outside_y = at.y < y0 || at.y > y1;
    let outside_x = at.x < x0 || at.x > x1;
    if outside_x && !outside_y { std::f64::consts::FRAC_PI_2 } else { 0.0 }
}

fn linear_kind(p1: Vec2, p2: Vec2, at: Vec2, rotation: Option<f64>, aligned: bool) -> DimKind {
    if aligned { DimKind::Aligned } else { DimKind::Linear { rotation: rotation.unwrap_or_else(|| auto_rotation(p1, p2, at)) } }
}

/// Add a dimension. With DIMASSOC on, definition points that sit on object endpoints,
/// vertices or centres are attached to them (associative dimensions).
fn add_dim(s: &mut Session, k: EntityKind) -> Result<Handle> {
    let mut k = k;
    if let EntityKind::Dimension(dm) = &mut k
        && dm.assoc.is_empty()
        && assoc_enabled(s)
    {
        dm.assoc = crate::assoc::reassociate(s.doc()?, &s.space(), dm);
    }
    let h = s.add_entity(k)?;
    s.last_dim = Some(h);
    Ok(h)
}

fn assoc_enabled(s: &Session) -> bool {
    s.doc().map(|d| d.header.i64("DIMASSOC", 2) != 0).unwrap_or(false)
}

fn with_assoc(k: EntityKind, assoc: Vec<DimAssoc>) -> EntityKind {
    match k {
        EntityKind::Dimension(mut d) => {
            d.assoc = assoc;
            EntityKind::Dimension(d)
        }
        other => other,
    }
}

/// Angular dimension between two lines, attached to them (vertex = their intersection).
fn angular_from_lines(s: &Session, h1: Handle, h2: Handle, at: Vec2) -> Option<EntityKind> {
    let d = s.doc().ok()?;
    let line = |h: Handle| match &d.entity(h)?.kind {
        EntityKind::Line(l) => Some((l.a.xy(), l.b.xy())),
        _ => None,
    };
    let ((a1, a2), (b1, b2)) = (line(h1)?, line(h2)?);
    let (v, _, _) = cadcraft_geom::line_line_infinite(a1, a2, b1, b2)?;
    // Use the leg ends on the side of the picked arc location (the quadrant the user meant).
    let pick = |p: Vec2, q: Vec2, snap_p: AssocSnap, snap_q: AssocSnap| {
        let dir = if (p - v).len() > (q - v).len() { (p, snap_p) } else { (q, snap_q) };
        let other = if dir.1 == AssocSnap::Start { (q, AssocSnap::End) } else { (p, AssocSnap::Start) };
        let side = |x: Vec2| (x - v).normalized().dot((at - v).normalized());
        if side(other.0) > side(dir.0) && other.0.dist(v) > 1e-9 { other } else { dir }
    };
    let (p1, s1) = pick(a1, a2, AssocSnap::Start, AssocSnap::End);
    let (p2, s2) = pick(b1, b2, AssocSnap::Start, AssocSnap::End);
    let k = dim(s, DimKind::Angular3P, at, p1, p2, v, Vec2::ZERO, "");
    let mut assoc = Vec::new();
    if assoc_enabled(s) {
        assoc = vec![
            DimAssoc { point: "p13".into(), handle: h1, snap: s1 },
            DimAssoc { point: "p14".into(), handle: h2, snap: s2 },
            DimAssoc { point: "p15".into(), handle: h1, snap: AssocSnap::Intersection { other: h2 } },
        ];
    }
    Some(with_assoc(k, assoc))
}

fn run_linear(s: &mut Session, p: &Value) -> Result<Value> {
    let (a, b) = match p.get("object").and_then(|v| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle))) {
        Some(h) => object_extent(s, h).ok_or_else(|| bad("dimlinear", "`object` must be a line, arc, circle or polyline segment"))?,
        None => (point_req("dimlinear", p, "p1")?, point_req("dimlinear", p, "p2")?),
    };
    let at = point_req("dimlinear", p, "at")?;
    let rot = p.get("rotation").and_then(Value::as_f64).map(f64::to_radians);
    let k = dim(s, linear_kind(a, b, at, rot, false), at, a, b, Vec2::ZERO, Vec2::ZERO, str_param(p, "text").unwrap_or(""));
    Ok(json!({ "handle": add_dim(s, k)?.hex() }))
}

fn run_aligned(s: &mut Session, p: &Value) -> Result<Value> {
    let a = point_req("dimaligned", p, "p1")?;
    let b = point_req("dimaligned", p, "p2")?;
    let at = point_req("dimaligned", p, "at")?;
    let k = dim(s, DimKind::Aligned, at, a, b, Vec2::ZERO, Vec2::ZERO, str_param(p, "text").unwrap_or(""));
    Ok(json!({ "handle": add_dim(s, k)?.hex() }))
}

/// Centre and radius of a circle/arc entity.
fn circle_of(s: &Session, h: Handle) -> Option<(Vec2, f64)> {
    match &s.doc().ok()?.entity(h)?.kind {
        EntityKind::Circle(c) => Some((c.center.xy(), c.radius)),
        EntityKind::Arc(a) => Some((a.center.xy(), a.radius)),
        _ => None,
    }
}

fn radial(s: &mut Session, p: &Value, diameter: bool) -> Result<Value> {
    let id = if diameter { "dimdiameter" } else { "dimradius" };
    let (c, pt) = if let (Some(c), Some(pt)) = (point_param(p, "center"), point_param(p, "point")) {
        (c, pt)
    } else {
        let h = targets(s, p)?.first().copied().ok_or_else(|| bad(id, "`handle` of a circle/arc, or {center, point}"))?;
        let (c, r) = circle_of(s, h).ok_or_else(|| bad(id, "object is not a circle or arc"))?;
        let at = point_param(p, "at").unwrap_or(c + Vec2::from_angle(std::f64::consts::FRAC_PI_4));
        (c, c + (at - c).normalized() * r)
    };
    let k = if diameter {
        dim(s, DimKind::Diameter, c + (c - pt), Vec2::ZERO, Vec2::ZERO, pt, Vec2::ZERO, "")
    } else {
        dim(s, DimKind::Radius, c, Vec2::ZERO, Vec2::ZERO, pt, Vec2::ZERO, "")
    };
    Ok(json!({ "handle": add_dim(s, k)?.hex() }))
}

fn run_radius(s: &mut Session, p: &Value) -> Result<Value> {
    radial(s, p, false)
}
fn run_diameter(s: &mut Session, p: &Value) -> Result<Value> {
    radial(s, p, true)
}

fn hex_param(p: &Value, key: &str) -> Option<Handle> {
    p.get(key).and_then(|v| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle)))
}

fn run_angular(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(ls) = p.get("lines").and_then(Value::as_array) {
        let hs: Vec<Handle> = ls.iter().filter_map(|v| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle))).collect();
        let (Some(h1), Some(h2)) = (hs.first(), hs.get(1)) else { return Err(bad("dimangular", "`lines` needs two line handles")) };
        let at = point_req("dimangular", p, "at")?;
        let k = angular_from_lines(s, *h1, *h2, at).ok_or_else(|| bad("dimangular", "objects must be two non-parallel lines"))?;
        return Ok(json!({ "handle": add_dim(s, k)?.hex() }));
    }
    if let Some(h) = hex_param(p, "arc") {
        let at = point_req("dimangular", p, "at")?;
        let a = match s.doc()?.entity(h).map(|e| e.kind.clone()) {
            Some(EntityKind::Arc(a)) => a,
            _ => return Err(bad("dimangular", "object is not an arc")),
        };
        let g = cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end);
        let k = dim(s, DimKind::Angular3P, at, g.start_point(), g.end_point(), g.center, Vec2::ZERO, "");
        return Ok(json!({ "handle": add_dim(s, k)?.hex() }));
    }
    let v = point_req("dimangular", p, "vertex")?;
    let a = point_req("dimangular", p, "p1")?;
    let b = point_req("dimangular", p, "p2")?;
    let at = point_req("dimangular", p, "at")?;
    let k = dim(s, DimKind::Angular3P, at, a, b, v, Vec2::ZERO, "");
    Ok(json!({ "handle": add_dim(s, k)?.hex() }))
}

fn run_arclen(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("dimarc", "`handle` of an arc is required"))?;
    let at = point_req("dimarc", p, "at")?;
    let a = match s.doc()?.entity(h).map(|e| e.kind.clone()) {
        Some(EntityKind::Arc(a)) => a,
        _ => return Err(bad("dimarc", "object is not an arc")),
    };
    let ga = cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end);
    let k = dim(s, DimKind::ArcLength, at, ga.start_point(), ga.end_point(), ga.center, Vec2::ZERO, "");
    Ok(json!({ "handle": add_dim(s, k)?.hex() }))
}

fn run_ordinate(s: &mut Session, p: &Value) -> Result<Value> {
    let f = point_req("dimordinate", p, "feature")?;
    let l = point_req("dimordinate", p, "leader")?;
    let xtype = p.get("xtype").and_then(Value::as_bool).unwrap_or((l - f).y.abs() > (l - f).x.abs());
    let k = dim(s, DimKind::Ordinate { x_type: xtype }, Vec2::ZERO, f, l, Vec2::ZERO, Vec2::ZERO, "");
    Ok(json!({ "handle": add_dim(s, k)?.hex() }))
}

/// The last linear/aligned dimension (for CONTINUE/BASELINE).
fn last_linear(s: &Session) -> Option<Dimension> {
    let h = s.last_dim?;
    match &s.doc().ok()?.entity(h)?.kind {
        EntityKind::Dimension(d) if matches!(d.kind, DimKind::Linear { .. } | DimKind::Aligned) => Some(d.clone()),
        _ => None,
    }
}

/// Next dimension in a chain from `prev` to a new second origin.
fn chained(s: &Session, prev: &Dimension, next: Vec2, baseline: bool) -> EntityKind {
    let spacing = s.doc().ok().and_then(|d| d.dim_style(&prev.style).map(|st| st.baseline_spacing * st.scale.max(1e-9))).unwrap_or(0.38);
    let dir = match prev.kind {
        DimKind::Linear { rotation } => Vec2::from_angle(rotation),
        _ => (prev.p14.xy() - prev.p13.xy()).normalized(),
    };
    let first = if baseline { prev.p13.xy() } else { prev.p14.xy() };
    let mut defpt = prev.defpt.xy();
    if baseline {
        // Offset the dimension line away from the measured points.
        let n = dir.perp();
        let side = if (defpt - prev.p13.xy()).dot(n) >= 0.0 { 1.0 } else { -1.0 };
        defpt += n * (spacing * side);
    }
    EntityKind::Dimension(Dimension {
        p13: v3(first),
        p14: v3(next),
        defpt: v3(defpt),
        text: String::new(),
        block: None,
        assoc: Vec::new(),
        user_text_pos: false,
        ..prev.clone()
    })
}

fn chain_run(s: &mut Session, p: &Value, baseline: bool) -> Result<Value> {
    let id = if baseline { "dimbaseline" } else { "dimcontinue" };
    let pts = points_param(p, "points").ok_or_else(|| bad(id, "`points` is required"))?;
    let mut out = Vec::new();
    for q in pts {
        let prev = last_linear(s).ok_or_else(|| bad(id, "no linear dimension to continue"))?;
        let k = chained(s, &prev, q, baseline);
        out.push(add_dim(s, k)?.hex());
    }
    Ok(json!({ "handles": out }))
}

fn run_continue(s: &mut Session, p: &Value) -> Result<Value> {
    chain_run(s, p, false)
}
fn run_baseline(s: &mut Session, p: &Value) -> Result<Value> {
    chain_run(s, p, true)
}

/// QDIM: continuous dimensions across the endpoints of the selected objects.
fn run_qdim(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let at = point_req("qdim", p, "at")?;
    let vertical = bool_or(p, "vertical", false);
    let d = s.doc()?;
    let mut pts: Vec<Vec2> = Vec::new();
    for h in &hs {
        if let Some(e) = d.entity(*h) {
            for pr in e.kind.prims() {
                match pr {
                    Prim::Seg(seg) => pts.extend([seg.start(), seg.end()]),
                    Prim::Circle(c) => pts.push(c.center),
                    Prim::Point(q) => pts.push(q),
                    _ => {}
                }
            }
        }
    }
    let key = |q: &Vec2| if vertical { q.y } else { q.x };
    pts.sort_by(|a, b| key(a).total_cmp(&key(b)));
    pts.dedup_by(|a, b| (key(a) - key(b)).abs() < 1e-9);
    if pts.len() < 2 {
        return Err(bad("qdim", "select objects with at least two distinct points"));
    }
    let rot = if vertical { std::f64::consts::FRAC_PI_2 } else { 0.0 };
    let mut out = Vec::new();
    for w in pts.windows(2) {
        if let [a, b] = w {
            let k = dim(s, DimKind::Linear { rotation: rot }, at, *a, *b, Vec2::ZERO, Vec2::ZERO, "");
            out.push(add_dim(s, k)?.hex());
        }
    }
    Ok(json!({ "handles": out }))
}

fn mleader_kind(s: &Session, pts: &[Vec2], text: &str) -> Option<EntityKind> {
    let (landing, arrow_pts) = pts.split_last()?;
    let d = s.doc().ok()?;
    let cur = d.header.str("CMLEADERSTYLE", "Standard");
    let st = d.mleader_styles.iter().find(|m| m.name.eq_ignore_ascii_case(&cur)).or(d.mleader_styles.first()).cloned().unwrap_or_default();
    let k = d.header.f64("DIMSCALE", 1.0);
    let th = st.text_height * k;
    let dir = if arrow_pts.first().is_some_and(|a| a.x > landing.x) { -1.0 } else { 1.0 };
    let attach = if dir > 0.0 { 4 } else { 6 };
    let tpos = *landing + Vec2::new((st.dogleg * k + st.landing_gap * k) * dir, 0.0);
    Some(EntityKind::MLeader(MLeader {
        leaders: vec![arrow_pts.iter().map(|p| v3(*p)).collect()],
        landing: v3(*landing),
        dogleg: st.dogleg * k,
        text: (!text.is_empty()).then(|| MText {
            insert: v3(tpos),
            height: th,
            width: 0.0,
            attach,
            rotation: 0.0,
            style: st.text_style.clone(),
            contents: text.replace('\n', "\\P"),
            line_spacing: 1.0,
        }),
        style: st.name.clone(),
        arrow_size: st.arrow_size * k,
    }))
}

fn run_mleader(s: &mut Session, p: &Value) -> Result<Value> {
    let pts = points_param(p, "points").ok_or_else(|| bad("mleader", "`points` (arrow … landing) is required"))?;
    if pts.len() < 2 {
        return Err(bad("mleader", "need an arrowhead and a landing point"));
    }
    let k = mleader_kind(s, &pts, str_param(p, "text").unwrap_or("")).ok_or_else(|| bad("mleader", "bad points"))?;
    Ok(json!({ "handle": s.add_entity(k)?.hex() }))
}

fn run_leader(s: &mut Session, p: &Value) -> Result<Value> {
    let pts = points_param(p, "points").ok_or_else(|| bad("leader", "`points` is required"))?;
    if pts.len() < 2 {
        return Err(bad("leader", "need 2+ points"));
    }
    let st = style(s);
    let h = s.add_entity(EntityKind::Leader(cadcraft_doc::Leader {
        vertices: pts.iter().map(|q| v3(*q)).collect(),
        arrow: true,
        spline: false,
        style: st,
    }))?;
    let mut out = vec![h.hex()];
    if let (Some(t), Some(last)) = (str_param(p, "text"), pts.last()) {
        let th = s.doc()?.dim_style(&style(s)).map(|d| d.text_height).unwrap_or(0.18);
        out.push(
            s.add_entity(EntityKind::MText(MText {
                insert: v3(*last + Vec2::new(th * 0.5, th * 0.5)),
                height: th,
                width: 0.0,
                attach: 4,
                rotation: 0.0,
                style: "Standard".into(),
                contents: t.into(),
                line_spacing: 1.0,
            }))?
            .hex(),
        );
    }
    Ok(json!({ "handles": out }))
}

fn run_update(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let st = style(s);
    let d = s.doc_mut()?;
    let mut n = 0;
    for h in hs {
        let _ = d.modify_entity(h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.style = st.clone();
                dm.block = None;
                n += 1;
            }
        });
    }
    Ok(json!({ "updated": n }))
}

/// DIMOVERRIDE: per-dimension style overrides (`clear` removes them); `text` sets the text.
fn run_override(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("text").is_none() {
        return super::props::dim_override(s, p);
    }
    let hs = targets(s, p)?;
    let t = str_param(p, "text").unwrap_or("").to_string();
    // Dimension variables given next to `text` are applied first, so an unknown name still errors before anything changes.
    let vars: serde_json::Map<String, Value> = p
        .as_object()
        .map(|o| o.iter().filter(|(k, _)| !matches!(k.as_str(), "text" | "handles" | "handle")).map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    let mut ignored = Value::Null;
    if !vars.is_empty() {
        let mut rest = vars;
        rest.insert("handles".into(), json!(hs.iter().map(|h| h.hex()).collect::<Vec<_>>()));
        ignored = super::props::dim_override(s, &Value::Object(rest))?.get("ignored").cloned().unwrap_or(Value::Null);
    }
    let d = s.doc_mut()?;
    for h in &hs {
        let _ = d.modify_entity(*h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.text = t.clone();
                dm.block = None;
            }
        });
    }
    match ignored {
        Value::Null => Ok(json!({ "changed": hs.len() })),
        ig => Ok(json!({ "changed": hs.len(), "ignored": ig })),
    }
}

/// The two points a picked object is dimensioned between (DIMLINEAR "select object").
fn object_extent(s: &Session, h: Handle) -> Option<(Vec2, Vec2)> {
    let e = s.doc().ok()?.entity(h)?;
    if let Some((c, r)) = circle_of(s, h).filter(|_| matches!(e.kind, EntityKind::Circle(_))) {
        return Some((c - Vec2::X * r, c + Vec2::X * r));
    }
    e.kind.prims().into_iter().find_map(|pr| match pr {
        Prim::Seg(sg @ (Segment::Line(_) | Segment::Arc { .. })) => Some((sg.start(), sg.end())),
        _ => None,
    })
}

fn dims_of(s: &Session, hs: &[Handle]) -> Vec<Handle> {
    let Ok(d) = s.doc() else { return Vec::new() };
    hs.iter().copied().filter(|h| d.entity(*h).is_some_and(|e| matches!(e.kind, EntityKind::Dimension(_)))).collect()
}

fn run_reassociate(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = dims_of(s, &targets(s, p)?);
    let space = s.space();
    let mut plan = Vec::new();
    {
        let d = s.doc()?;
        for h in &hs {
            if let Some(EntityKind::Dimension(dm)) = d.entity(*h).map(|e| &e.kind) {
                plan.push((*h, crate::assoc::reassociate(d, &space, dm)));
            }
        }
    }
    let mut points = 0;
    let mut n = 0;
    let d = s.doc_mut()?;
    for (h, a) in plan {
        points += a.len();
        n += usize::from(!a.is_empty());
        let _ = d.modify_entity(h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.assoc = a;
            }
        });
    }
    Ok(json!({ "dimensions": hs.len(), "associated": n, "points": points, "message": format!("{n} of {} dimension(s) associated.", hs.len()) }))
}

fn run_disassociate(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = dims_of(s, &targets(s, p)?);
    let d = s.doc_mut()?;
    let mut n = 0;
    for h in &hs {
        let _ = d.modify_entity(*h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind
                && !dm.assoc.is_empty()
            {
                dm.assoc.clear();
                n += 1;
            }
        });
    }
    Ok(json!({ "disassociated": n, "message": format!("{n} disassociated.") }))
}

/// DIMTEDIT: move/rotate/justify dimension text.
fn run_tedit(s: &mut Session, p: &Value, mode: &str) -> Result<Value> {
    let mut q = p.as_object().cloned().unwrap_or_default();
    q.insert("mode".into(), json!(mode));
    run_dimtedit(s, &Value::Object(q))
}

fn run_dimtedit(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = dims_of(s, &targets(s, p)?);
    if hs.is_empty() {
        return Err(bad("dimtedit", "select dimensions"));
    }
    let mode = str_param(p, "mode").unwrap_or("").to_ascii_lowercase();
    let at = point_param(p, "at");
    let angle = p.get("angle").and_then(Value::as_f64).filter(|a| a.is_finite()).map(f64::to_radians);
    if mode == "angle" && angle.is_none() {
        return Err(bad("dimtedit", "`angle` (degrees) is required"));
    }
    let d = s.doc_mut()?;
    for h in &hs {
        d.modify_entity(*h, |e| {
            let EntityKind::Dimension(dm) = &mut e.kind else { return };
            dm.block = None;
            if let Some(q) = at {
                dm.text_mid = v3(q);
                dm.user_text_pos = true;
            }
            match mode.as_str() {
                "home" => {
                    dm.user_text_pos = false;
                    dm.text_rotation = 0.0;
                    dm.overrides.remove("textJust");
                }
                "angle" => dm.text_rotation = angle.unwrap_or(0.0),
                "left" | "center" | "right" => {
                    dm.user_text_pos = false;
                    let j = match mode.as_str() {
                        "left" => 1,
                        "right" => 2,
                        _ => 0,
                    };
                    dm.overrides.insert("textJust".into(), json!(j));
                }
                _ => {}
            }
        })?;
    }
    Ok(json!({ "changed": hs.len() }))
}

/// DIMSPACE: equal spacing of parallel linear dimensions from a base dimension.
fn run_dimspace(s: &mut Session, p: &Value) -> Result<Value> {
    let base = hex_param(p, "base").ok_or_else(|| bad("dimspace", "`base` dimension is required"))?;
    let hs: Vec<Handle> = dims_of(s, &targets(s, p)?).into_iter().filter(|h| *h != base).collect();
    let d = s.doc()?;
    let Some(EntityKind::Dimension(b)) = d.entity(base).map(|e| e.kind.clone()) else { return Err(bad("dimspace", "`base` is not a dimension")) };
    let st = d.dim_style(&b.style).cloned().unwrap_or_default().with_overrides(&b.overrides);
    let spacing =
        p.get("spacing").and_then(Value::as_f64).filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(st.baseline_spacing * st.scale.max(1e-9) * 2.0);
    let dir = match b.kind {
        DimKind::Linear { rotation } => Vec2::from_angle(rotation),
        DimKind::Aligned => (b.p14.xy() - b.p13.xy()).normalized(),
        _ => return Err(bad("dimspace", "base must be a linear or aligned dimension")),
    };
    let n = dir.perp();
    let base_off = (b.defpt.xy() - b.p13.xy()).dot(n);
    let side = if base_off >= 0.0 { 1.0 } else { -1.0 };
    // Order the others by their current distance from the base line.
    let mut others: Vec<(Handle, f64)> = hs
        .iter()
        .filter_map(|h| match d.entity(*h).map(|e| &e.kind) {
            Some(EntityKind::Dimension(o)) if matches!(o.kind, DimKind::Linear { .. } | DimKind::Aligned) => {
                Some((*h, (o.defpt.xy() - b.defpt.xy()).dot(n) * side))
            }
            _ => None,
        })
        .collect();
    others.sort_by(|a, c| a.1.total_cmp(&c.1));
    let line_pt = b.defpt.xy();
    let dm = s.doc_mut()?;
    for (i, (h, _)) in others.iter().enumerate() {
        let target = line_pt + n * (side * spacing * (i as f64 + 1.0));
        dm.modify_entity(*h, |e| {
            if let EntityKind::Dimension(o) = &mut e.kind {
                let cur = o.defpt.xy();
                let moved = cur + n * (target - cur).dot(n);
                o.defpt = v3(moved);
                o.user_text_pos = false;
                o.block = None;
            }
        })?;
    }
    Ok(json!({ "spaced": others.len() }))
}

/// Select objects, then run a JSON command on them.
struct SelectThen {
    sel: super::machines::SelectPhase,
    id: &'static str,
}

impl SelectThen {
    fn new(_s: &Session, id: &'static str) -> Self {
        SelectThen { sel: super::machines::SelectPhase::default(), id }
    }
    fn run(&self, s: &mut Session, hs: Vec<Handle>) -> Result<Step> {
        let hex: Vec<String> = hs.iter().map(|h| h.hex()).collect();
        let r = s.execute(self.id, &json!({ "handles": hex }))?;
        if let Some(m) = r.get("message").and_then(Value::as_str) {
            s.echo(m.to_string());
        }
        Ok(Step::Done)
    }
}

impl Interactive for SelectThen {
    fn name(&self) -> &'static str {
        if self.id == "dimreassociate" { "DIMREASSOCIATE" } else { "DIMDISASSOCIATE" }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = super::machines::SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return self.run(s, hs);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        self.sel.prompt()
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.sel.feed(s, &i)? {
            super::machines::SelOutcome::More => Ok(Step::Continue),
            super::machines::SelOutcome::Empty => Ok(Step::Done),
            super::machines::SelOutcome::Done(hs) => self.run(s, hs),
        }
    }
    fn preview(&self, _s: &Session, _c: Vec2) -> Vec<EntityKind> {
        Vec::new()
    }
}

/// DIMOVERRIDE at the command line: variable, value (repeat), Enter, then select dimensions.
struct OverrideM {
    vars: serde_json::Map<String, Value>,
    var: Option<String>,
    clear: bool,
    selecting: Option<super::machines::SelectPhase>,
}

impl OverrideM {
    fn new(_s: &Session) -> Self {
        OverrideM { vars: serde_json::Map::new(), var: None, clear: false, selecting: None }
    }
    fn apply(&self, s: &mut Session, hs: Vec<Handle>) -> Result<Step> {
        let mut p = self.vars.clone();
        p.insert("handles".into(), json!(hs.iter().map(|h| h.hex()).collect::<Vec<_>>()));
        if self.clear {
            p.insert("clear".into(), json!(true));
        }
        let r = super::props::dim_override(s, &Value::Object(p))?;
        s.echo(format!("{} dimension(s) changed.", r.get("changed").and_then(Value::as_u64).unwrap_or(0)));
        Ok(Step::Done)
    }
}

impl Interactive for OverrideM {
    fn name(&self) -> &'static str {
        "DIMOVERRIDE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some(sel) = &self.selecting {
            return sel.prompt();
        }
        match &self.var {
            Some(v) => Prompt::new(format!("Enter new value for dimension variable {}", v.to_ascii_uppercase()), Accept::TEXT),
            None => Prompt::new("Enter dimension variable name to override", Accept::TEXT).kw(&["Clear overrides"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(sel) = &mut self.selecting {
            return match sel.feed(s, &i)? {
                super::machines::SelOutcome::More => Ok(Step::Continue),
                super::machines::SelOutcome::Empty => Ok(Step::Done),
                super::machines::SelOutcome::Done(hs) => self.apply(s, hs),
            };
        }
        match (self.var.take(), i) {
            (None, Input::Keyword(k)) if k.starts_with("Clear") => {
                self.clear = true;
                self.selecting = Some(super::machines::SelectPhase::default());
            }
            (None, Input::Text(t)) => {
                let t = t.trim().to_string();
                if t.eq_ignore_ascii_case("c") {
                    self.clear = true;
                    self.selecting = Some(super::machines::SelectPhase::default());
                } else if cadcraft_doc::DimStyle::field_name(&t).is_some() {
                    self.var = Some(t);
                } else {
                    s.echo(format!("Unknown dimension variable \"{t}\"."));
                }
            }
            (None, Input::Enter) => {
                if self.vars.is_empty() {
                    return Ok(Step::Cancel);
                }
                self.selecting = Some(super::machines::SelectPhase::default());
            }
            (Some(v), Input::Text(t)) => {
                let val = t.trim().parse::<f64>().map(|x| json!(x)).unwrap_or_else(|_| json!(t.trim()));
                self.vars.insert(v, val);
            }
            (Some(v), _) => self.var = Some(v),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, _c: Vec2) -> Vec<EntityKind> {
        Vec::new()
    }
}

// ---------------- interactive ----------------

fn pick_at(s: &Session, p: Vec2) -> Option<Handle> {
    let ap = s.pixel_size() * s.settings.pickbox.max(1.0) * 1.5;
    crate::select::pick(s.doc().ok()?, &s.space(), p, ap)
}

struct LinearM {
    aligned: bool,
    pts: Vec<Vec2>,
    rotation: Option<f64>,
    text: String,
    selecting: bool,
    asking: Option<&'static str>,
}

impl LinearM {
    fn new(aligned: bool) -> Self {
        LinearM { aligned, pts: Vec::new(), rotation: None, text: String::new(), selecting: false, asking: None }
    }
}

impl Interactive for LinearM {
    fn name(&self) -> &'static str {
        if self.aligned { "DIMALIGNED" } else { "DIMLINEAR" }
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some(a) = self.asking {
            return Prompt::new(a, Accept::TEXT);
        }
        if self.selecting {
            return Prompt::new("Select object to dimension", Accept::POINT);
        }
        match self.pts.len() {
            0 => Prompt::new("Specify first extension line origin", Accept::POINT).default("select object"),
            1 => Prompt::new("Specify second extension line origin", Accept::POINT).base_opt(self.pts.first().copied()),
            _ if self.aligned => Prompt::new("Specify dimension line location", Accept::POINT).kw(&["Mtext", "Text", "Angle"]),
            _ => Prompt::new("Specify dimension line location", Accept::POINT).kw(&["Mtext", "Text", "Angle", "Horizontal", "Vertical", "Rotated"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(a) = self.asking.take() {
            if let Input::Text(t) = &i {
                if a.starts_with("Enter dimension text") {
                    self.text = t.clone();
                } else if let Some(r) = crate::units::parse_angle(t) {
                    self.rotation = Some(r);
                }
            }
            return Ok(Step::Continue);
        }
        if self.selecting {
            if let Input::Point(p) = i {
                let Some(h) = pick_at(s, p) else {
                    s.echo("*Invalid selection*");
                    return Ok(Step::Continue);
                };
                let seg =
                    s.doc()?.entity(h).and_then(|e| e.kind.prims().into_iter().find_map(|pr| if let Prim::Seg(sg) = pr { Some(sg) } else { None }));
                match seg {
                    Some(sg @ Segment::Line(_)) | Some(sg @ Segment::Arc { .. }) => {
                        self.pts = vec![sg.start(), sg.end()];
                        self.selecting = false;
                    }
                    None => {
                        if let Some((c, r)) = circle_of(s, h) {
                            self.pts = vec![c - Vec2::X * r, c + Vec2::X * r];
                            self.selecting = false;
                        } else {
                            s.echo("Object cannot be dimensioned this way.");
                        }
                    }
                }
            }
            return Ok(Step::Continue);
        }
        match (self.pts.len(), i) {
            (0, Input::Enter) => {
                self.selecting = true;
                Ok(Step::Continue)
            }
            (n, Input::Point(p)) if n < 2 => {
                self.pts.push(p);
                Ok(Step::Continue)
            }
            (_, Input::Point(at)) => {
                let (Some(&a), Some(&b)) = (self.pts.first(), self.pts.get(1)) else { return Ok(Step::Continue) };
                let k = dim(s, linear_kind(a, b, at, self.rotation, self.aligned), at, a, b, Vec2::ZERO, Vec2::ZERO, &self.text);
                add_dim(s, k)?;
                if let Some(EntityKind::Dimension(d)) = s.last_dim.and_then(|h| s.doc().ok()?.entity(h).map(|e| e.kind.clone())) {
                    let st = s.doc()?.dim_style(&d.style).cloned().unwrap_or_default();
                    let g = cadcraft_render::dimension_geometry(&d, &st, 1.0);
                    s.echo(format!("Dimension text = {}", g.value));
                }
                Ok(Step::Done)
            }
            (_, Input::Keyword(k)) => {
                match k.as_str() {
                    "Horizontal" => self.rotation = Some(0.0),
                    "Vertical" => self.rotation = Some(std::f64::consts::FRAC_PI_2),
                    "Rotated" => self.asking = Some("Specify angle of dimension line"),
                    "Text" | "Mtext" => self.asking = Some("Enter dimension text (<> = measurement)"),
                    _ => self.asking = Some("Specify angle of dimension text"),
                }
                Ok(Step::Continue)
            }
            (_, Input::Enter) => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        match self.pts.as_slice() {
            [a] => vec![super::helpers::line(*a, c)],
            [a, b] => vec![dim(s, linear_kind(*a, *b, c, self.rotation, self.aligned), c, *a, *b, Vec2::ZERO, Vec2::ZERO, &self.text)],
            _ => Vec::new(),
        }
    }
}

struct RadialM {
    diameter: bool,
    target: Option<(Vec2, f64)>,
}

impl RadialM {
    fn kind(&self, s: &Session, c: Vec2, r: f64, at: Vec2) -> EntityKind {
        let pt = c + (at - c).normalized() * r;
        if self.diameter {
            dim(s, DimKind::Diameter, c + (c - pt), Vec2::ZERO, Vec2::ZERO, pt, Vec2::ZERO, "")
        } else {
            dim(s, DimKind::Radius, c, Vec2::ZERO, Vec2::ZERO, pt, Vec2::ZERO, "")
        }
    }
}

impl Interactive for RadialM {
    fn name(&self) -> &'static str {
        if self.diameter { "DIMDIAMETER" } else { "DIMRADIUS" }
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.target {
            None => Prompt::new("Select arc or circle", Accept::POINT),
            Some((c, _)) => Prompt::new("Specify dimension line location", Accept::POINT).kw(&["Mtext", "Text", "Angle"]).base(c),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.target, i) {
            (None, Input::Point(p)) => {
                match pick_at(s, p).and_then(|h| circle_of(s, h)) {
                    Some(t) => self.target = Some(t),
                    None => s.echo("Object selected is not a circle or arc."),
                }
                Ok(Step::Continue)
            }
            (Some((c, r)), Input::Point(at)) => {
                let k = self.kind(s, c, r, at);
                add_dim(s, k)?;
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        self.target.map(|(ctr, r)| vec![self.kind(s, ctr, r, c)]).unwrap_or_default()
    }
}

#[derive(Default)]
struct AngularM {
    handles: Vec<Handle>,
    first: Option<(Vec2, Vec2)>,
    second: Option<(Vec2, Vec2)>,
    vertex_mode: Vec<Vec2>,
    vertex: bool,
    arc: Option<(Vec2, Vec2, Vec2)>,
}

impl AngularM {
    /// (vertex, p1, p2) of the angle being dimensioned.
    fn geometry(&self) -> Option<(Vec2, Vec2, Vec2)> {
        if let Some(a) = self.arc {
            return Some(a);
        }
        if let [v, a, b] = self.vertex_mode.as_slice() {
            return Some((*v, *a, *b));
        }
        let ((a1, a2), (b1, b2)) = (self.first?, self.second?);
        let (v, _, _) = cadcraft_geom::line_line_infinite(a1, a2, b1, b2)?;
        let far = |p: Vec2, q: Vec2| if p.dist(v) > q.dist(v) { p } else { q };
        Some((v, far(a1, a2), far(b1, b2)))
    }
}

impl Interactive for AngularM {
    fn name(&self) -> &'static str {
        "DIMANGULAR"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.geometry().is_some() {
            return Prompt::new("Specify dimension arc line location", Accept::POINT).kw(&["Mtext", "Text", "Angle", "Quadrant"]);
        }
        if self.vertex {
            return Prompt::new(
                ["Specify angle vertex", "Specify first angle endpoint", "Specify second angle endpoint"]
                    .get(self.vertex_mode.len())
                    .copied()
                    .unwrap_or(""),
                Accept::POINT,
            );
        }
        match self.first {
            None => Prompt::new("Select arc, circle, line", Accept::POINT).default("specify vertex"),
            Some(_) => Prompt::new("Select second line", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some((v, a, b)) = self.geometry() {
            return match i {
                Input::Point(at) => {
                    let k = match (self.handles.as_slice(), self.arc) {
                        ([h1, h2], None) => angular_from_lines(s, *h1, *h2, at),
                        _ => None,
                    };
                    let k = k.unwrap_or_else(|| dim(s, DimKind::Angular3P, at, a, b, v, Vec2::ZERO, ""));
                    add_dim(s, k)?;
                    Ok(Step::Done)
                }
                Input::Enter => Ok(Step::Cancel),
                _ => Ok(Step::Continue),
            };
        }
        if self.vertex {
            match i {
                Input::Point(p) => self.vertex_mode.push(p),
                Input::Enter => return Ok(Step::Cancel),
                _ => {}
            }
            return Ok(Step::Continue);
        }
        match i {
            Input::Enter if self.first.is_none() => {
                self.vertex = true;
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                let Some(h) = pick_at(s, p) else {
                    s.echo("*Invalid selection*");
                    return Ok(Step::Continue);
                };
                let kind = s.doc()?.entity(h).map(|e| e.kind.clone());
                match kind {
                    Some(EntityKind::Arc(a)) if self.first.is_none() => {
                        let g = cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end);
                        self.arc = Some((g.center, g.start_point(), g.end_point()));
                    }
                    Some(EntityKind::Line(l)) => {
                        self.handles.push(h);
                        let seg = (l.a.xy(), l.b.xy());
                        if self.first.is_none() {
                            self.first = Some(seg);
                        } else {
                            self.second = Some(seg);
                        }
                    }
                    _ => s.echo("Select a line or an arc."),
                }
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        self.geometry().map(|(v, a, b)| vec![dim(s, DimKind::Angular3P, c, a, b, v, Vec2::ZERO, "")]).unwrap_or_default()
    }
}

#[derive(Default)]
struct ArcLenM {
    arc: Option<cadcraft_geom::Arc>,
}

impl Interactive for ArcLenM {
    fn name(&self) -> &'static str {
        "DIMARC"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.arc {
            None => Prompt::new("Select arc or polyline arc segment", Accept::POINT),
            Some(_) => Prompt::new("Specify arc length dimension location", Accept::POINT).kw(&["Mtext", "Text", "Angle", "Partial"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.arc, i) {
            (None, Input::Point(p)) => {
                if let Some(EntityKind::Arc(a)) = pick_at(s, p).and_then(|h| s.doc().ok()?.entity(h).map(|e| e.kind.clone())) {
                    self.arc = Some(cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end));
                } else {
                    s.echo("Select an arc.");
                }
                Ok(Step::Continue)
            }
            (Some(g), Input::Point(at)) => {
                add_dim(s, dim(s, DimKind::ArcLength, at, g.start_point(), g.end_point(), g.center, Vec2::ZERO, ""))?;
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        self.arc.map(|g| vec![dim(s, DimKind::ArcLength, c, g.start_point(), g.end_point(), g.center, Vec2::ZERO, "")]).unwrap_or_default()
    }
}

#[derive(Default)]
struct OrdinateM {
    feature: Option<Vec2>,
    xtype: Option<bool>,
}

impl Interactive for OrdinateM {
    fn name(&self) -> &'static str {
        "DIMORDINATE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.feature {
            None => Prompt::new("Specify feature location", Accept::POINT),
            Some(f) => Prompt::new("Specify leader endpoint", Accept::POINT).kw(&["Xdatum", "Ydatum", "Mtext", "Text", "Angle"]).base(f),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.feature, i) {
            (None, Input::Point(p)) => {
                self.feature = Some(p);
                Ok(Step::Continue)
            }
            (Some(_), Input::Keyword(k)) => {
                self.xtype = Some(k == "Xdatum");
                Ok(Step::Continue)
            }
            (Some(f), Input::Point(l)) => {
                let x = self.xtype.unwrap_or((l - f).y.abs() > (l - f).x.abs());
                add_dim(s, dim(s, DimKind::Ordinate { x_type: x }, Vec2::ZERO, f, l, Vec2::ZERO, Vec2::ZERO, ""))?;
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        self.feature
            .map(|f| {
                vec![dim(
                    s,
                    DimKind::Ordinate { x_type: self.xtype.unwrap_or((c - f).y.abs() > (c - f).x.abs()) },
                    Vec2::ZERO,
                    f,
                    c,
                    Vec2::ZERO,
                    Vec2::ZERO,
                    "",
                )]
            })
            .unwrap_or_default()
    }
}

struct ChainM {
    baseline: bool,
    created: Vec<Handle>,
}

impl ChainM {
    fn new(_s: &Session, baseline: bool) -> Self {
        ChainM { baseline, created: Vec::new() }
    }
}

impl Interactive for ChainM {
    fn name(&self) -> &'static str {
        if self.baseline { "DIMBASELINE" } else { "DIMCONTINUE" }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        if last_linear(s).is_none() {
            s.echo("No linear dimension to continue. Create one with DIMLINEAR first.");
            return Ok(Step::Done);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        Prompt::new("Specify a second extension line origin", Accept::POINT)
            .kw(&["Select", "Undo"])
            .default("Select")
            .base_opt(last_linear(s).map(|d| if self.baseline { d.p13.xy() } else { d.p14.xy() }))
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                let prev = last_linear(s).ok_or_else(|| EngineError::Other("no linear dimension".into()))?;
                let k = chained(s, &prev, p, self.baseline);
                self.created.push(add_dim(s, k)?);
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Undo" => {
                if let Some(h) = self.created.pop() {
                    s.doc_mut()?.remove_entity(h);
                    s.last_dim = self.created.last().copied();
                }
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        last_linear(s).map(|d| vec![chained(s, &d, c, self.baseline)]).unwrap_or_default()
    }
}

#[derive(Default)]
struct MLeaderM {
    pts: Vec<Vec2>,
    landed: bool,
}

impl Interactive for MLeaderM {
    fn name(&self) -> &'static str {
        "MLEADER"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.landed {
            return Prompt::new("Enter leader text", Accept::TEXT);
        }
        match self.pts.len() {
            0 => Prompt::new("Specify leader arrowhead location", Accept::POINT).kw(&["leader Landing first", "Content first", "Options"]),
            _ => Prompt::new("Specify leader landing location", Accept::POINT).base_opt(self.pts.last().copied()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.landed {
            let t = match i {
                Input::Text(t) => t,
                _ => String::new(),
            };
            let k = mleader_kind(s, &self.pts, &t).ok_or_else(|| EngineError::Other("bad leader".into()))?;
            s.add_entity(k)?;
            return Ok(Step::Done);
        }
        match i {
            Input::Point(p) => {
                self.pts.push(p);
                if self.pts.len() >= 2 {
                    self.landed = true;
                }
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.landed || self.pts.is_empty() {
            return Vec::new();
        }
        let mut pts = self.pts.clone();
        pts.push(c);
        mleader_kind(s, &pts, "").into_iter().collect()
    }
}

#[cfg(test)]
#[path = "annotate_tests.rs"]
mod tests;
