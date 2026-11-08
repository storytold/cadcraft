use super::Value;
use super::args::{R, arity, num, point};

// ---------------- geometry ----------------

pub(super) fn polar(a: &[Value]) -> R {
    arity(a, 3, 3)?;
    let p = point(&a[0])?;
    let (ang, d) = (num(&a[1])?, num(&a[2])?);
    let out = [p[0] + d * ang.cos(), p[1] + d * ang.sin(), p[2]];
    Ok(if a[0].iter().count() == 2 { Value::point(&out[..2]) } else { Value::point(&out) })
}

pub(super) fn distance(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    let (p, q) = (point(&a[0])?, point(&a[1])?);
    // Two 2D points, or one: a 2D distance.
    let dz = if a[0].iter().count() == 2 || a[1].iter().count() == 2 { 0.0 } else { q[2] - p[2] };
    Ok(Value::Real(((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + dz * dz).sqrt()))
}

pub(super) fn angle(a: &[Value]) -> R {
    arity(a, 2, 2)?;
    let (p, q) = (point(&a[0])?, point(&a[1])?);
    Ok(Value::Real((q[1] - p[1]).atan2(q[0] - p[0]).rem_euclid(std::f64::consts::TAU)))
}

/// `(inters p1 p2 p3 p4 [onseg])`: where lines p1-p2 and p3-p4 cross; with `onseg` (the default)
/// only within both segments.
pub(super) fn inters(a: &[Value]) -> R {
    arity(a, 4, 5)?;
    let (p1, p2, p3, p4) = (point(&a[0])?, point(&a[1])?, point(&a[2])?, point(&a[3])?);
    let onseg = a.get(4).is_none_or(|v| !v.is_nil());
    let (d1x, d1y, d2x, d2y) = (p2[0] - p1[0], p2[1] - p1[1], p4[0] - p3[0], p4[1] - p3[1]);
    let den = d1x * d2y - d1y * d2x;
    if den.abs() < 1e-12 {
        return Ok(Value::Nil);
    }
    let t = ((p3[0] - p1[0]) * d2y - (p3[1] - p1[1]) * d2x) / den;
    let u = ((p3[0] - p1[0]) * d1y - (p3[1] - p1[1]) * d1x) / den;
    let eps = 1e-10;
    if onseg && (t < -eps || t > 1.0 + eps || u < -eps || u > 1.0 + eps) {
        return Ok(Value::Nil);
    }
    let z = p1[2] + t * (p2[2] - p1[2]);
    let x = [p1[0] + t * d1x, p1[1] + t * d1y, z];
    let two_d = a.iter().take(4).any(|p| p.iter().count() == 2);
    Ok(if two_d { Value::point(&x[..2]) } else { Value::point(&x) })
}
