//! XCLIP in a file (DXF Reference, SPATIAL_FILTER): a clipped block reference has an extension
//! dictionary with an `ACAD_FILTER` dictionary whose `SPATIAL` entry is a SPATIAL_FILTER object.
//! The filter holds the boundary vertices in the reference's coordinates (group 70 the count,
//! 10/20 the vertices), whether clipping is on (71), the front and back clip (72/73, 40/41) and
//! two 3×4 matrices (group 40, row by row): the inverse of the reference's transform, which takes
//! the boundary into the block's coordinates, and the boundary's own transform (identity here).
//!
//! An inverted clip is CADCraft's own entry `CADCRAFT_XCLIP` beside `ACAD_FILTER` in the same
//! extension dictionary: an XRECORD with group 70 = 1. Other programs keep it as unknown data and
//! clip normally.
//!
//! Reading takes the planar filters the model can hold out of the kept foreign data and puts them
//! on the references ([`read`]); others (front/back clipping, a skewed boundary transform) stay
//! foreign and are written back untouched. Writing adds the dictionaries and the filter of every
//! clipped reference to a copy of the drawing's foreign data ([`prepare`]), which the raw-data
//! writer (#528) then places on the reference.

use std::sync::Arc;

use cadcraft_doc::foreign::{Foreign, ForeignObject};
use cadcraft_doc::{ClipBoundary, Drawing, EntityKind, Handle, MAX_CLIP_VERTICES, RawTag};
use cadcraft_geom::{Mat3, Vec2};

/// The extension-dictionary entry of CADCraft's inverted-clip flag.
const INVERT_KEY: &str = "CADCRAFT_XCLIP";
/// Handles above this are not handed out (the DXF handle space is 64-bit, keep a margin).
const MAX_HANDLE: u64 = 1 << 48;

fn raw(code: i32, value: impl ToString) -> RawTag {
    RawTag { code, value: value.to_string() }
}

/// The handle of dictionary entry `name` (group 3 followed by 350/360).
fn entry(tags: &[RawTag], name: &str) -> Option<Handle> {
    tags.windows(2)
        .find(|w| matches!(w, [k, v] if k.code == 3 && k.value.eq_ignore_ascii_case(name) && matches!(v.code, 350 | 360)))
        .and_then(|w| w.get(1))
        .and_then(|v| Handle::parse_hex(&v.value))
}

/// Remove dictionary entry `name`; returns the handle it named.
fn remove_entry(tags: &mut Vec<RawTag>, name: &str) -> Option<Handle> {
    let mut old = None;
    let mut i = 0;
    while let (Some(k), Some(v)) = (tags.get(i), tags.get(i + 1)) {
        if k.code == 3 && k.value.eq_ignore_ascii_case(name) && matches!(v.code, 350 | 360) {
            old = old.or_else(|| Handle::parse_hex(&v.value));
            tags.drain(i..i + 2);
        } else {
            i += 1;
        }
    }
    old
}

fn number(tags: &[RawTag], code: i32) -> Option<f64> {
    tags.iter().find(|t| t.code == code)?.value.trim().parse::<f64>().ok().filter(|n| n.is_finite())
}

fn object(f: &Foreign, h: Handle) -> Option<&ForeignObject> {
    f.objects.iter().find(|o| o.handle == h)
}

/// The extension dictionary, ACAD_FILTER dictionary and SPATIAL_FILTER of entity `h`.
fn chain(f: &Foreign, h: Handle) -> Option<(Handle, Handle, &ForeignObject)> {
    let xd = f.entities.get(&h)?.xdict?;
    let fd = entry(&object(f, xd)?.tags, "ACAD_FILTER")?;
    let sf = entry(&object(f, fd)?.tags, "SPATIAL")?;
    Some((xd, fd, object(f, sf).filter(|o| o.kind.eq_ignore_ascii_case("SPATIAL_FILTER"))?))
}

/// A 3×4 matrix (12 values row by row) as a 2D transform, when it is one (no Z mixing).
fn matrix_2d(m: &[f64]) -> Option<Mat3> {
    let [a, c, z1, e, b, d, z2, f, z3, z4, one, z5] = <[f64; 12]>::try_from(m).ok()?;
    let flat = [z1, z2, z3, z4, z5].iter().all(|v| v.abs() <= 1e-9) && (one - 1.0).abs() <= 1e-9;
    (flat && [a, b, c, d, e, f].iter().all(|v| v.is_finite())).then_some(Mat3 { a, b, c, d, e, f })
}

fn matrix_values(m: &Mat3) -> [f64; 12] {
    [m.a, m.c, 0.0, m.e, m.b, m.d, 0.0, m.f, 0.0, 0.0, 1.0, 0.0]
}

/// The boundary a planar SPATIAL_FILTER holds, in the block's coordinates. `None` for anything
/// the model can't hold as it is (it then stays foreign).
fn parse_filter(o: &ForeignObject) -> Option<ClipBoundary> {
    let count = usize::try_from(number(&o.tags, 70)? as i64).ok()?;
    if !(2..=MAX_CLIP_VERTICES).contains(&count) {
        return None;
    }
    let normal_up = number(&o.tags, 210).unwrap_or(0.0).abs() <= 1e-9
        && number(&o.tags, 220).unwrap_or(0.0).abs() <= 1e-9
        && (number(&o.tags, 230).unwrap_or(1.0) - 1.0).abs() <= 1e-9;
    let no_depth = number(&o.tags, 72).unwrap_or(0.0) == 0.0 && number(&o.tags, 73).unwrap_or(0.0) == 0.0;
    if !normal_up || !no_depth {
        return None;
    }
    let m: Vec<f64> = o.tags.iter().filter(|t| t.code == 40).map(|t| t.value.trim().parse::<f64>().unwrap_or(f64::NAN)).collect();
    let (inverse, own) = match m.len() {
        24 => (matrix_2d(m.get(..12)?)?, matrix_2d(m.get(12..)?)?),
        // Without matrices the vertices are taken as the block's own.
        0 => (Mat3::IDENTITY, Mat3::IDENTITY),
        _ => return None,
    };
    if (own.a - 1.0).abs() > 1e-9
        || own.b.abs() > 1e-9
        || own.c.abs() > 1e-9
        || (own.d - 1.0).abs() > 1e-9
        || own.e.abs() > 1e-9
        || own.f.abs() > 1e-9
    {
        return None;
    }
    let mut points = Vec::with_capacity(count);
    for w in o.tags.windows(2) {
        let [x, y] = w else { continue };
        if x.code != 10 {
            continue;
        }
        if y.code != 20 || points.len() >= count {
            return None;
        }
        let p = inverse.apply(Vec2::new(x.value.trim().parse().ok()?, y.value.trim().parse().ok()?));
        if !p.is_finite() {
            return None;
        }
        points.push(p);
    }
    let clip = ClipBoundary { points, enabled: number(&o.tags, 71).unwrap_or(1.0) != 0.0, inverted: false };
    (clip.points.len() == count && cadcraft_render::clip::PolyClip::new(&clip.polygon()).is_some()).then_some(clip)
}

/// Whether the extension dictionary `xd` marks the clip inverted.
fn inverted(f: &Foreign, xd: Handle) -> bool {
    object(f, xd)
        .and_then(|o| entry(&o.tags, INVERT_KEY))
        .and_then(|h| object(f, h))
        .is_some_and(|o| o.kind.eq_ignore_ascii_case("XRECORD") && number(&o.tags, 70).is_some_and(|v| v != 0.0))
}

/// Put the planar filters of the file read into `d` on their references, taking them out of
/// the kept foreign data (the model writes them from now on).
pub(crate) fn read(d: &mut Drawing) {
    if d.foreign.objects.is_empty() {
        return;
    }
    let found: Vec<(Handle, ClipBoundary)> = d
        .foreign
        .entities
        .keys()
        .filter_map(|h| {
            let (xd, _, o) = chain(&d.foreign, *h)?;
            let mut clip = parse_filter(o)?;
            clip.inverted = inverted(&d.foreign, xd);
            Some((*h, clip))
        })
        .collect();
    if found.is_empty() {
        return;
    }
    let mut taken = Vec::new();
    for (h, clip) in found {
        let set = |e: &mut cadcraft_doc::Entity| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.clip = Some(clip.clone());
            }
        };
        let is_insert = |e: &cadcraft_doc::Entity| matches!(e.kind, EntityKind::Insert(_));
        let done = if d.entity(h).is_some_and(|e| is_insert(e)) {
            d.modify_entity(h, set).is_ok()
        } else {
            let mut done = false;
            for b in d.blocks.values_mut() {
                if b.entities.get(h).is_some_and(|e| is_insert(e)) {
                    done = Arc::make_mut(b).entities.modify(h, set);
                    break;
                }
            }
            done
        };
        if done {
            taken.push(h);
        }
    }
    let f = Arc::make_mut(&mut d.foreign);
    for h in taken {
        forget(f, h);
    }
}

/// Take the clip of entity `h` out of the foreign data: the ACAD_FILTER entry and what it holds,
/// CADCraft's inverted flag, and the extension dictionary when nothing else is left in it.
fn forget(f: &mut Foreign, h: Handle) {
    let Some(xd) = f.entities.get(&h).and_then(|x| x.xdict) else { return };
    let mut gone: Vec<Handle> = Vec::new();
    let mut empty = false;
    if let Some(o) = f.objects.iter_mut().find(|o| o.handle == xd) {
        gone.extend(remove_entry(&mut o.tags, "ACAD_FILTER"));
        gone.extend(remove_entry(&mut o.tags, INVERT_KEY));
        empty = !o.tags.iter().any(|t| t.code == 3);
    }
    // The filter dictionary's entries go with it.
    let mut i = 0;
    while let Some(g) = gone.get(i).copied() {
        if let Some(o) = object(f, g)
            && o.kind.eq_ignore_ascii_case("DICTIONARY")
        {
            gone.extend(o.tags.windows(2).filter_map(|w| match w {
                [k, v] if k.code == 3 && matches!(v.code, 350 | 360) => Handle::parse_hex(&v.value),
                _ => None,
            }));
        }
        i += 1;
        if i > MAX_CLIP_VERTICES {
            break;
        }
    }
    if empty {
        gone.push(xd);
        if let Some(x) = f.entities.get_mut(&h) {
            x.xdict = None;
        }
        if f.entities.get(&h).is_some_and(|x| x.is_empty()) {
            f.entities.remove(&h);
        }
    }
    f.objects.retain(|o| !gone.contains(&o.handle));
}

fn dictionary(h: Handle, owner: Handle) -> ForeignObject {
    ForeignObject {
        handle: h,
        kind: "DICTIONARY".into(),
        tags: vec![
            raw(5, h.hex()),
            raw(102, "{ACAD_REACTORS"),
            raw(330, owner.hex()),
            raw(102, "}"),
            raw(330, owner.hex()),
            raw(100, "AcDbDictionary"),
            raw(280, 1),
            raw(281, 1),
        ],
    }
}

fn spatial_filter(h: Handle, owner: Handle, clip: &ClipBoundary, to_world: &Mat3, inverse: &Mat3) -> ForeignObject {
    let mut tags = vec![
        raw(5, h.hex()),
        raw(102, "{ACAD_REACTORS"),
        raw(330, owner.hex()),
        raw(102, "}"),
        raw(330, owner.hex()),
        raw(100, "AcDbFilter"),
        raw(100, "AcDbSpatialFilter"),
        raw(70, clip.points.len()),
    ];
    for p in &clip.points {
        let q = to_world.apply(*p);
        tags.push(raw(10, q.x));
        tags.push(raw(20, q.y));
    }
    tags.extend([raw(210, 0.0), raw(220, 0.0), raw(230, 1.0), raw(11, 0.0), raw(21, 0.0), raw(31, 0.0)]);
    tags.extend([raw(71, i32::from(clip.enabled)), raw(72, 0), raw(73, 0)]);
    for v in matrix_values(inverse).into_iter().chain(matrix_values(&Mat3::IDENTITY)) {
        tags.push(raw(40, v));
    }
    ForeignObject { handle: h, kind: "SPATIAL_FILTER".into(), tags }
}

fn invert_record(h: Handle, owner: Handle) -> ForeignObject {
    ForeignObject {
        handle: h,
        kind: "XRECORD".into(),
        tags: vec![
            raw(5, h.hex()),
            raw(102, "{ACAD_REACTORS"),
            raw(330, owner.hex()),
            raw(102, "}"),
            raw(330, owner.hex()),
            raw(100, "AcDbXrecord"),
            raw(280, 1),
            raw(70, 1),
        ],
    }
}

/// A copy of `d` whose foreign data holds the SPATIAL_FILTER of every clipped reference (any
/// filter kept from the file for it is replaced). `None` when no reference is clipped.
pub(crate) fn prepare(d: &Drawing) -> Option<Drawing> {
    let mut clips: Vec<(Handle, ClipBoundary, Mat3)> = Vec::new();
    let mut take = |e: &cadcraft_doc::Entity| {
        if let EntityKind::Insert(i) = &e.kind
            && let Some(c) = &i.clip
            && c.polygon().len() >= 3
            && let Some(base) = d.block(&i.block).map(|b| b.base.xy())
        {
            clips.push((e.handle, c.clone(), i.transform(base)));
        }
    };
    d.model.iter().for_each(|e| take(e));
    d.layouts.iter().flat_map(|l| l.entities.iter()).for_each(|e| take(e));
    d.blocks.values().filter(|b| b.xref_path.is_none()).flat_map(|b| b.entities.iter()).for_each(|e| take(e));
    if clips.is_empty() {
        return None;
    }
    let mut out = d.clone();
    let f = Arc::make_mut(&mut out.foreign);
    let mut next = d.handseed.max(f.objects.iter().map(|o| o.handle.0).max().unwrap_or(0)).saturating_add(1);
    fn fresh(next: &mut u64) -> Handle {
        let h = Handle(*next);
        *next = next.saturating_add(1);
        h
    }
    for (insert, clip, to_world) in clips {
        let Some(inverse) = to_world.inverse() else { continue };
        if next.saturating_add(5) >= MAX_HANDLE {
            break;
        }
        // A filter kept from the file for this reference is replaced.
        forget(f, insert);
        let xd = match f.entities.get(&insert).and_then(|x| x.xdict).filter(|h| object(f, *h).is_some()) {
            Some(h) => h,
            None => {
                let h = fresh(&mut next);
                f.objects.push(dictionary(h, insert));
                f.entities.entry(insert).or_default().xdict = Some(h);
                h
            }
        };
        let (fd, sf) = (fresh(&mut next), fresh(&mut next));
        let flag = clip.inverted.then(|| fresh(&mut next));
        if let Some(o) = f.objects.iter_mut().find(|o| o.handle == xd) {
            o.tags.extend([raw(3, "ACAD_FILTER"), raw(360, fd.hex())]);
            if let Some(x) = flag {
                o.tags.extend([raw(3, INVERT_KEY), raw(360, x.hex())]);
            }
        }
        let mut filters = dictionary(fd, xd);
        filters.tags.extend([raw(3, "SPATIAL"), raw(360, sf.hex())]);
        f.objects.push(filters);
        f.objects.push(spatial_filter(sf, fd, &clip, &to_world, &inverse));
        if let Some(x) = flag {
            f.objects.push(invert_record(x, xd));
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrices_round_trip_row_by_row() {
        let m = Mat3::translate(Vec2::new(10.0, 20.0)).then_before(Mat3::rotate(0.5)).then_before(Mat3::scale(2.0, 2.0));
        let back = matrix_2d(&matrix_values(&m)).unwrap();
        assert!((back.a - m.a).abs() < 1e-12 && (back.c - m.c).abs() < 1e-12 && (back.f - m.f).abs() < 1e-12);
        // Translation is the fourth value of each row.
        let t = matrix_values(&Mat3::translate(Vec2::new(-5.0, 7.0)));
        assert_eq!((t[3], t[7]), (-5.0, 7.0));
        assert!(matrix_2d(&[1.0; 11]).is_none());
    }

    #[test]
    fn hostile_filters_stay_foreign() {
        let o = |tags: Vec<RawTag>| ForeignObject { handle: Handle(1), kind: "SPATIAL_FILTER".into(), tags };
        assert!(parse_filter(&o(vec![raw(70, 1_000_000_000)])).is_none());
        assert!(parse_filter(&o(vec![raw(70, 3), raw(10, 0), raw(20, 0)])).is_none());
        assert!(parse_filter(&o(vec![raw(70, 2), raw(10, "nan"), raw(20, 0), raw(10, 1), raw(20, 1)])).is_none());
        assert!(parse_filter(&o(vec![raw(70, 2), raw(10, 0), raw(20, 0), raw(10, 1), raw(20, 1), raw(72, 1)])).is_none());
        let ok = parse_filter(&o(vec![raw(70, 2), raw(10, 0), raw(20, 0), raw(10, 1), raw(20, 1)])).unwrap();
        assert_eq!(ok.points.len(), 2);
    }
}
