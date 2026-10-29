use super::{Entity, Handle, Mat3, Result, Session, Value, Vec2, bad, bool_or, f64_req, json, point_param, point_req, targets};

// ---------------- core operations ----------------

pub(crate) fn transform_entities(s: &mut Session, hs: &[Handle], m: &Mat3, copy: bool) -> Result<Vec<Handle>> {
    transform_entities_dz(s, hs, m, 0.0, copy)
}

/// [`transform_entities`] with an additional translation along Z (MOVE/COPY with a Z delta).
/// Every other caller passes 0 and leaves Z untouched.
pub(crate) fn transform_entities_dz(s: &mut Session, hs: &[Handle], m: &Mat3, dz: f64, copy: bool) -> Result<Vec<Handle>> {
    let d = s.doc_mut()?;
    let mut out = Vec::new();
    for h in hs {
        let Some(sp) = d.space_of(*h) else { continue };
        if copy {
            let Some(e) = d.entity(*h).map(|e| (**e).clone()) else { continue };
            let nh = d.new_handle();
            let mut ne = Entity { handle: nh, ..e };
            ne.kind.transform_dz(m, dz);
            if let Some(st) = d.space_mut(&sp) {
                st.push(ne);
                out.push(nh);
            }
        } else {
            let locked = d.entity(*h).and_then(|e| d.layer(&e.common.layer)).is_some_and(|l| l.locked);
            if locked {
                continue;
            }
            d.modify_entity(*h, |e| e.kind.transform_dz(m, dz))?;
            out.push(*h);
        }
    }
    Ok(out)
}

pub(super) fn erase(s: &mut Session, hs: &[Handle]) -> Result<usize> {
    let d = s.doc_mut()?;
    let mut n = 0;
    for h in hs {
        let locked = d.entity(*h).and_then(|e| d.layer(&e.common.layer)).is_some_and(|l| l.locked);
        if !locked && d.remove_entity(*h).is_some() {
            n += 1;
        }
    }
    s.set_selection(Vec::new());
    Ok(n)
}

pub(super) fn run_erase(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let n = erase(s, &hs)?;
    Ok(json!({ "erased": n }))
}

pub(super) fn delta_of(cmd: &str, p: &Value) -> Result<Vec2> {
    if let Some(d) = point_param(p, "delta") {
        return Ok(d);
    }
    let a = point_req(cmd, p, "from")?;
    let b = point_req(cmd, p, "to")?;
    Ok(b - a)
}

/// A point param's Z offset: the optional third array element (or `z` field), else 0.
fn dz_of(cmd: &str, v: &Value) -> Result<f64> {
    let z = match v {
        Value::Array(a) => match a.get(2) {
            None | Some(Value::Null) => 0.0,
            Some(z) => z.as_f64().ok_or_else(|| bad(cmd, "`delta` [dx, dy, dz] needs numbers"))?,
        },
        Value::Object(_) => match v.get("z") {
            None | Some(Value::Null) => 0.0,
            Some(z) => z.as_f64().ok_or_else(|| bad(cmd, "`delta` z needs a number"))?,
        },
        _ => 0.0,
    };
    if z.is_finite() { Ok(z) } else { Err(bad(cmd, "the Z offset must be finite")) }
}

/// MOVE/COPY displacement with an optional Z: `delta[2]`, or `to[2] - from[2]`.
fn delta3_of(cmd: &str, p: &Value) -> Result<(Vec2, f64)> {
    let d = delta_of(cmd, p)?;
    let dz = if let Some(v) = p.get("delta") {
        dz_of(cmd, v)?
    } else {
        dz_of(cmd, p.get("to").unwrap_or(&Value::Null))? - dz_of(cmd, p.get("from").unwrap_or(&Value::Null))?
    };
    Ok((d, dz))
}

pub(super) fn run_move(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let (d, dz) = delta3_of("move", p)?;
    let moved = transform_entities_dz(s, &hs, &Mat3::translate(d), dz, false)?;
    Ok(json!({ "moved": moved.len() }))
}

pub(super) fn run_copy(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let (d, dz) = delta3_of("copy", p)?;
    let n = p.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 10_000);
    let mut out = Vec::new();
    for k in 1..=n {
        out.extend(transform_entities_dz(s, &hs, &Mat3::translate(d * k as f64), dz * k as f64, true)?.into_iter().map(|h| h.hex()));
    }
    Ok(json!({ "handles": out }))
}

pub(super) fn run_rotate(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let base = point_req("rotate", p, "base")?;
    let a = f64_req("rotate", p, "angle")?.to_radians();
    let r = transform_entities(s, &hs, &Mat3::rotate_about(base, a), bool_or(p, "copy", false))?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

pub(super) fn run_scale(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let base = point_req("scale", p, "base")?;
    let f = f64_req("scale", p, "factor")?;
    if f <= 0.0 {
        return Err(bad("scale", "factor must be positive"));
    }
    let r = transform_entities(s, &hs, &Mat3::scale_about(base, f), bool_or(p, "copy", false))?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

pub(super) fn run_mirror(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let a = point_req("mirror", p, "p1")?;
    let b = point_req("mirror", p, "p2")?;
    if a.near(b, 1e-12) {
        return Err(bad("mirror", "mirror line points coincide"));
    }
    let erase_src = bool_or(p, "erase", false);
    let r = transform_entities(s, &hs, &Mat3::mirror(a, b), !erase_src)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}
