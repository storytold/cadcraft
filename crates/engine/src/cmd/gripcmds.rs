//! `grip.*` commands: the undoable, scriptable form of grip editing (see `crate::grips`).

use cadcraft_doc::{Entity, EntityKind, Handle};
use cadcraft_geom::{Mat3, Vec2};
use serde_json::{Value, json};

use super::curves;
use super::{CommandSpec, bad, bool_or, point_param, point_req, str_param, targets};

use crate::grips::{GripAction, GripMode, grip_action, mode_matrix, stretch_grip};
use crate::{EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("grip.move", "Grip Stretch", run_grip_move).params(
            "{handle?, handles?, index, to} (stretch the grip) | {handles?, index, baseHandle?, to, mode: \"move\", copy?} (move about the grip)",
        ),
        CommandSpec::new("grip.stretch", "Grip Stretch", run_grip_stretch).params(
            "{handle?, handles?, index, to} | {grips: [[handle, index], ...] (several hot grips), delta | to (+ base?), handles?, copy?}",
        ),
        CommandSpec::new("grip.rotate", "Grip Rotate", run_grip_rotate).params("{handles?, base | baseHandle+index, angle (degrees) | to, copy?}"),
        CommandSpec::new("grip.scale", "Grip Scale", run_grip_scale).params("{handles?, base | baseHandle+index, factor | to, copy?}"),
        CommandSpec::new("grip.mirror", "Grip Mirror", run_grip_mirror)
            .params("{handles?, base | baseHandle+index, to (second mirror point), copy?}"),
        CommandSpec::new("grip.option", "Grip Option", run_grip_option).params(
            "{handle, index, option: addVertex|removeVertex|toArc|toLine|lengthen|radius, to? (point), value? (lengthen: length change; radius)}",
        ),
        CommandSpec::new("grip", "Grip Edit", run_grip)
            .params("interactive: select grips, then the grip modes with [Base point/Copy/Undo/Reference/eXit]; JSON: use grip.stretch/move/rotate/scale/mirror")
            .interactive(super::griphot::factory),
    ]
}

fn index_param(cmd: &str, p: &Value) -> Result<usize> {
    let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`index` (grip number) is required"))?;
    usize::try_from(i).map_err(|_| bad(cmd, "index out of range"))
}

fn stretch(s: &mut Session, cmd: &str, p: &Value) -> Result<Value> {
    let h = curves::handle_param(p, "handle")
        .or_else(|| targets(s, p).ok().and_then(|t| t.first().copied()))
        .ok_or_else(|| bad(cmd, "`handle` is required"))?;
    let i = index_param(cmd, p)?;
    let to = point_req(cmd, p, "to")?;
    if curves::is_locked(s, h) {
        return Err(EngineError::Other("The object is on a locked layer.".into()));
    }
    let e = curves::entity(s, h)?;
    let k = stretch_grip(&e.kind, i, to).ok_or_else(|| bad(cmd, "no such grip, or the result would be degenerate"))?;
    let mut edits = vec![(h, k)];
    // The other `handles` with a grip on the same point stretch with it (connected objects stay
    // connected); locked ones and edits that would be degenerate are left alone.
    let from = e.kind.grips().get(i).copied().unwrap_or(to);
    let tol = 1e-9 * (1.0 + from.len());
    for o in targets(s, p)? {
        if o == h || edits.iter().any(|(x, _)| *x == o) || curves::is_locked(s, o) {
            continue;
        }
        let Ok(oe) = curves::entity(s, o) else { continue };
        if let Some(j) = oe.kind.grips().iter().position(|g| g.near(from, tol))
            && let Some(k) = stretch_grip(&oe.kind, j, to)
        {
            edits.push((o, k));
        }
    }
    let d = s.doc_mut()?;
    for (o, k) in &mut edits {
        // A dimension's text moved by its grip: with DIMTMOVE 0 the dimension line follows.
        if let cadcraft_doc::EntityKind::Dimension(dm) = k
            && let Some(cadcraft_doc::EntityKind::Dimension(old)) = d.entity(*o).map(|e| &e.kind)
            && dm.text_mid != old.text_mid
        {
            let st = d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides);
            st.follow_text(dm, d.header.f64("DIMSCALE", 1.0));
        }
        d.modify_entity(*o, |e| e.kind = k.clone())?;
    }
    Ok(json!({ "handle": h.hex(), "handles": edits.iter().map(|(o, _)| o.hex()).collect::<Vec<_>>() }))
}

/// Base point: `base`, or grip `index` of `baseHandle` (default: the first target).
fn base_point(s: &Session, cmd: &str, p: &Value, hs: &[Handle]) -> Result<Vec2> {
    if let Some(b) = point_param(p, "base") {
        return Ok(b);
    }
    let h = curves::handle_param(p, "baseHandle")
        .or_else(|| hs.first().copied())
        .ok_or_else(|| bad(cmd, "`base` or `baseHandle` + `index` is required"))?;
    let i = index_param(cmd, p)?;
    let g = curves::entity(s, h)?.kind.grips();
    g.get(i).copied().ok_or_else(|| bad(cmd, "no such grip"))
}

fn apply(s: &mut Session, hs: &[Handle], m: &Mat3, copy: bool) -> Result<Value> {
    let r = super::modify::transform_entities(s, hs, m, copy)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

fn run_grip_stretch(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("grips").is_none() {
        return stretch(s, "grip.stretch", p);
    }
    let cmd = "grip.stretch";
    let list = p.get("grips").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`grips` must be an array of [handle, index] pairs"))?;
    let mut hot = Vec::new();
    for g in list.iter().take(MAX_GRIPS) {
        let pair = || -> Option<(Handle, usize)> {
            let h = curves::handle_value(g.get(0)?)?;
            Some((h, usize::try_from(g.get(1)?.as_u64()?).ok()?))
        };
        hot.push(pair().ok_or_else(|| bad(cmd, format!("`grips`: {g} is not a [handle, index] pair")))?);
    }
    let &(h0, i0) = hot.first().ok_or_else(|| bad(cmd, "`grips` is empty"))?;
    let delta = match point_param(p, "delta") {
        Some(d) => d,
        None => {
            let base = match point_param(p, "base") {
                Some(b) => b,
                None => curves::entity(s, h0)?.kind.grips().get(i0).copied().ok_or_else(|| bad(cmd, "no such grip"))?,
            };
            point_req(cmd, p, "to")? - base
        }
    };
    let others = if p.get("handles").is_some() { targets(s, p)? } else { s.selection() };
    let hs = stretch_grips(s, &hot, &others, delta, bool_or(p, "copy", false))?;
    if hs.is_empty() {
        return Err(bad(cmd, "no grip could be stretched (locked, missing, or the result would be degenerate)"));
    }
    Ok(json!({ "handles": hs.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

/// A multifunctional grip option (polyline Add/Remove Vertex, Convert to Arc/Line; line and arc
/// Lengthen; arc Radius), as the grip menu runs it.
fn run_grip_option(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "grip.option";
    let h = curves::handle_param(p, "handle").ok_or_else(|| bad(cmd, "`handle` is required"))?;
    let i = index_param(cmd, p)?;
    let action: GripAction = p
        .get("option")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .ok_or_else(|| bad(cmd, "`option` must be addVertex, removeVertex, toArc, toLine, lengthen or radius"))?;
    if curves::is_locked(s, h) {
        return Err(EngineError::Other("The object is on a locked layer.".into()));
    }
    let e = curves::entity(s, h)?;
    let value = p.get("value").and_then(Value::as_f64);
    let k = grip_action(&e.kind, i, action, point_param(p, "to"), value)
        .ok_or_else(|| bad(cmd, "that option doesn't apply to this grip, or the result would be degenerate"))?;
    s.doc_mut()?.modify_entity(h, |e| e.kind = k)?;
    Ok(json!({ "handle": h.hex() }))
}

fn run_grip(_s: &mut Session, _p: &Value) -> Result<Value> {
    Err(bad("grip", "GRIP is interactive; use grip.stretch, grip.move, grip.rotate, grip.scale or grip.mirror"))
}

/// At most this many grips or objects take part in one multi-grip stretch.
const MAX_GRIPS: usize = 1000;

/// The objects as they'd be after moving the grips `hot` by `delta`, with the grips of `others`
/// that sit on a hot grip's point (connected objects stay connected). Locked objects and edits
/// that would be degenerate are left out; several hot grips of one object move together.
pub(super) fn stretched(s: &Session, hot: &[(Handle, usize)], others: &[Handle], delta: Vec2) -> Vec<(Handle, EntityKind)> {
    let Ok(d) = s.doc() else { return Vec::new() };
    // Every grip to move, per object, with its target point.
    let mut moves: Vec<(Handle, Vec<(usize, Vec2)>)> = Vec::new();
    let mut add = |h: Handle, i: usize, to: Vec2| match moves.iter_mut().find(|(x, _)| *x == h) {
        Some((_, v)) => {
            if !v.iter().any(|(j, _)| *j == i) {
                v.push((i, to));
            }
        }
        None => moves.push((h, vec![(i, to)])),
    };
    for &(h, i) in hot.iter().take(MAX_GRIPS) {
        let Some(from) = d.entity(h).and_then(|e| e.kind.grips().get(i).copied()) else { continue };
        add(h, i, from + delta);
        let tol = 1e-9 * (1.0 + from.len());
        for &o in others.iter().take(MAX_GRIPS) {
            if o == h {
                continue;
            }
            if let Some(j) = d.entity(o).and_then(|e| e.kind.grips().iter().position(|g| g.near(from, tol))) {
                add(o, j, from + delta);
            }
        }
    }
    let mut out = Vec::new();
    for (h, mv) in moves {
        if curves::is_locked(s, h) {
            continue;
        }
        let Some(e) = d.entity(h) else { continue };
        let mut k = e.kind.clone();
        let mut changed = false;
        for (i, to) in mv {
            if let Some(nk) = stretch_grip(&k, i, to) {
                k = nk;
                changed = true;
            }
        }
        if changed {
            out.push((h, k));
        }
    }
    out
}

/// Stretch the grips `hot` by `delta` (see [`stretched`]). With `copy` the stretched objects are
/// added as copies and the originals stay. Returns the edited (or new) objects.
pub(super) fn stretch_grips(s: &mut Session, hot: &[(Handle, usize)], others: &[Handle], delta: Vec2, copy: bool) -> Result<Vec<Handle>> {
    let edits = stretched(s, hot, others, delta);
    let d = s.doc_mut()?;
    let mut out = Vec::new();
    for (h, k) in edits {
        if copy {
            let (Some(sp), Some(e)) = (d.space_of(h), d.entity(h).map(|e| (**e).clone())) else { continue };
            let nh = d.new_handle();
            if let Some(st) = d.space_mut(&sp) {
                st.push(Entity { handle: nh, kind: k, ..e });
                out.push(nh);
            }
        } else {
            d.modify_entity(h, |e| e.kind = k)?;
            out.push(h);
        }
    }
    Ok(out)
}

fn run_grip_move(s: &mut Session, p: &Value) -> Result<Value> {
    let mode_move = str_param(p, "mode").is_some_and(|m| m.eq_ignore_ascii_case("move")) || (p.get("index").is_none() && p.get("base").is_some());
    if !mode_move {
        return stretch(s, "grip.move", p);
    }
    let hs = targets(s, p)?;
    let base = base_point(s, "grip.move", p, &hs)?;
    let to = point_req("grip.move", p, "to")?;
    let m = mode_matrix(GripMode::Move, base, to).unwrap_or(Mat3::IDENTITY);
    apply(s, &hs, &m, bool_or(p, "copy", false))
}

fn run_grip_rotate(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let base = base_point(s, "grip.rotate", p, &hs)?;
    let m = match p.get("angle").and_then(Value::as_f64).filter(|a| a.is_finite()) {
        Some(a) => Mat3::rotate_about(base, a.to_radians()),
        None => mode_matrix(GripMode::Rotate, base, point_req("grip.rotate", p, "to")?).ok_or_else(|| bad("grip.rotate", "invalid point"))?,
    };
    apply(s, &hs, &m, bool_or(p, "copy", false))
}

fn run_grip_scale(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let base = base_point(s, "grip.scale", p, &hs)?;
    let m = match p.get("factor").and_then(Value::as_f64).filter(|a| a.is_finite()) {
        Some(f) if f > 0.0 => Mat3::scale_about(base, f),
        Some(_) => return Err(bad("grip.scale", "factor must be positive")),
        None => {
            mode_matrix(GripMode::Scale, base, point_req("grip.scale", p, "to")?).ok_or_else(|| bad("grip.scale", "scale factor would be zero"))?
        }
    };
    apply(s, &hs, &m, bool_or(p, "copy", false))
}

fn run_grip_mirror(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let base = base_point(s, "grip.mirror", p, &hs)?;
    let to = point_req("grip.mirror", p, "to")?;
    let m = mode_matrix(GripMode::Mirror, base, to).ok_or_else(|| bad("grip.mirror", "the mirror line needs two distinct points"))?;
    apply(s, &hs, &m, bool_or(p, "copy", false))
}
