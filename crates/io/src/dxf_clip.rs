//! XCLIP in a file (DXF Reference, SPATIAL_FILTER): a clipped block reference has an extension
//! dictionary with an `ACAD_FILTER` dictionary whose `SPATIAL` entry is a SPATIAL_FILTER object.
//! The filter holds the boundary vertices in the reference's coordinates (group 70 the count,
//! 10/20 the vertices), whether clipping is on (71), the front and back clip (72/73, 40/41) and
//! two 3×4 matrices (group 40, row by row): the inverse of the reference's transform, which takes
//! the boundary into the block's coordinates, and the boundary's own transform (composed with
//! the inverse transform when non-identity, e.g. on clipboard-pasted block references).
//!
//! An inverted clip ("Invert clip") is marked as the reference app writes it in a file: the
//! filter has an extension dictionary whose `ACAD_XREC_ROUNDTRIP` entry is an XRECORD holding
//! `102 ACAD_INVERTEDCLIP_ROUNDTRIP` then the boundary (10/20/30), and
//! `102 ACAD_INVERTEDCLIP_ROUNDTRIP_COMPARE` then the filter's own vertices. While the filter
//! still holds those vertices the clip is inverted, with the first list as its boundary. The
//! reference app shows the flag as group 290 on the object but never writes 290 in a DXF file,
//! and discards a file whose filter has one; 290 is read when another program wrote it, never
//! written. The record goes through DWG as it is.
//!
//! Reading takes the planar filters the model can hold out of the kept foreign data and puts them
//! on the references ([`read`]); others (front/back clipping, non-planar extrusion, sheared boundary transform) stay
//! foreign and are written back untouched. Writing adds the dictionaries and the filter of every
//! clipped reference to a copy of the drawing's foreign data ([`prepare`]), which the raw-data
//! writer (#528) then places on the reference.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cadcraft_doc::foreign::{Foreign, ForeignObject};
use cadcraft_doc::{ClipBoundary, Drawing, EntityKind, Handle, MAX_CLIP_VERTICES, RawTag};
use cadcraft_geom::{Mat3, Vec2};

/// The filter's extension-dictionary entry of the reference app's inverted-clip record.
const ROUNDTRIP_KEY: &str = "ACAD_XREC_ROUNDTRIP";
const ROUNDTRIP: &str = "ACAD_INVERTEDCLIP_ROUNDTRIP";
const COMPARE: &str = "ACAD_INVERTEDCLIP_ROUNDTRIP_COMPARE";
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

/// Where each handle's object is in `Foreign::objects` (the first, as a scan would find it), so
/// a file with many references and objects isn't searched once per reference.
type Index = HashMap<Handle, usize>;

fn index(f: &Foreign) -> Index {
    let mut idx = Index::with_capacity(f.objects.len());
    for (i, o) in f.objects.iter().enumerate() {
        idx.entry(o.handle).or_insert(i);
    }
    idx
}

fn object<'a>(f: &'a Foreign, idx: &Index, h: Handle) -> Option<&'a ForeignObject> {
    f.objects.get(*idx.get(&h)?)
}

/// The extension dictionary, ACAD_FILTER dictionary and SPATIAL_FILTER of entity `h`.
fn chain<'a>(f: &'a Foreign, idx: &Index, h: Handle) -> Option<(Handle, Handle, &'a ForeignObject)> {
    let xd = f.entities.get(&h)?.xdict?;
    let fd = entry(&object(f, idx, xd)?.tags, "ACAD_FILTER")?;
    let sf = entry(&object(f, idx, fd)?.tags, "SPATIAL")?;
    Some((xd, fd, object(f, idx, sf).filter(|o| o.kind.eq_ignore_ascii_case("SPATIAL_FILTER"))?))
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

/// The `count` vertices (10/20) a SPATIAL_FILTER holds, as written.
fn vertices(tags: &[RawTag], count: usize) -> Option<Vec<Vec2>> {
    // The count is the file's: bound it before allocating.
    if count > MAX_CLIP_VERTICES {
        return None;
    }
    let mut points = Vec::with_capacity(count);
    for w in tags.windows(2) {
        let [x, y] = w else { continue };
        if x.code != 10 {
            continue;
        }
        if y.code != 20 || points.len() >= count {
            return None;
        }
        points.push(Vec2::new(x.value.trim().parse().ok()?, y.value.trim().parse().ok()?));
    }
    (points.len() == count).then_some(points)
}

/// The handle of an object's own extension dictionary (`102 {ACAD_XDICTIONARY`, 360).
fn own_xdict(tags: &[RawTag]) -> Option<Handle> {
    tags.windows(2)
        .find(|w| matches!(w, [k, v] if k.code == 102 && k.value.trim().eq_ignore_ascii_case("{ACAD_XDICTIONARY") && v.code == 360))
        .and_then(|w| w.get(1))
        .and_then(|v| Handle::parse_hex(&v.value))
}

/// The boundary of an inverted clip as the reference app marks it on `filter` (see above), when
/// the filter still holds the vertices the mark was made for.
fn roundtrip(f: &Foreign, idx: &Index, filter: &ForeignObject) -> Option<Vec<Vec2>> {
    let rec = object(f, idx, entry(&object(f, idx, own_xdict(&filter.tags)?)?.tags, ROUNDTRIP_KEY)?)?;
    if !rec.kind.eq_ignore_ascii_case("XRECORD") {
        return None;
    }
    let (mut boundary, mut compare) = (Vec::new(), Vec::new());
    // 0 before either list, 1 in the boundary, 2 in the filter's vertices.
    let mut list = 0;
    for w in rec.tags.windows(2) {
        let [x, y] = w else { continue };
        match x.code {
            102 => {
                list = match x.value.trim() {
                    v if v.eq_ignore_ascii_case(ROUNDTRIP) => 1,
                    v if v.eq_ignore_ascii_case(COMPARE) => 2,
                    _ => 0,
                }
            }
            10 if list != 0 && y.code == 20 => {
                let into = if list == 1 { &mut boundary } else { &mut compare };
                if into.len() >= MAX_CLIP_VERTICES {
                    return None;
                }
                into.push(Vec2::new(x.value.trim().parse().ok()?, y.value.trim().parse().ok()?));
            }
            _ => {}
        }
    }
    let own = vertices(&filter.tags, usize::try_from(number(&filter.tags, 70)? as i64).ok()?)?;
    let near = |a: f64, b: f64| (a - b).abs() <= 1e-9 * (1.0 + a.abs().max(b.abs()));
    let same = own.len() == compare.len() && own.iter().zip(&compare).all(|(p, q)| near(p.x, q.x) && near(p.y, q.y));
    (same && boundary.len() >= 2).then_some(boundary)
}

/// The boundary a planar SPATIAL_FILTER holds, in the block's coordinates.
///
/// Per the Autodesk DXF Reference for `SPATIAL_FILTER`:
/// - Group 40 (first 12 values): 3×4 matrix representing the inverse of the block reference
///   transformation (maps coordinates from WCS to block local space).
/// - Group 40 (second 12 values): 3×4 matrix representing the transformation into the coordinate
///   system of the clip boundary ($WCS \to \text{Boundary}$).
///
/// Since boundary vertices (group 10/20) are stored in the clip boundary's coordinate system,
/// transforming them into the block's local coordinates requires inverting the boundary transform
/// ($\text{Boundary} \to WCS$) and then applying the inverse block transform ($WCS \to \text{Block}$):
/// $P_{\text{block}} = M_{\text{inverse}} \times (M_{\text{boundary\_xf}})^{-1} \times P_{\text{boundary}}$.
/// In code: `boundary_xf.inverse()?.then(inverse)`.
///
/// With `boundary`, the clip is inverted and that is its outline (in the filter's coordinates).
/// Returns `None` for unsupported configurations (e.g. non-planar extrusion, front/back clipping depth).
fn parse_filter(o: &ForeignObject, boundary: Option<&[Vec2]>) -> Option<ClipBoundary> {
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
    let (inverse, boundary_xf) = match m.len() {
        24 => (matrix_2d(m.get(..12)?)?, matrix_2d(m.get(12..)?)?),
        // Without matrices the vertices are taken as the block's own.
        0 => (Mat3::IDENTITY, Mat3::IDENTITY),
        _ => return None,
    };
    // Reject sheared/skewed boundary transforms that cannot be losslessly represented.
    // Scale-independent orthogonality check: |u . v| > 1e-4 * |u| * |v|
    let u_len = (boundary_xf.a * boundary_xf.a + boundary_xf.b * boundary_xf.b).sqrt();
    let v_len = (boundary_xf.c * boundary_xf.c + boundary_xf.d * boundary_xf.d).sqrt();
    if u_len <= 1e-9 || v_len <= 1e-9 {
        return None;
    }
    let dot = boundary_xf.a * boundary_xf.c + boundary_xf.b * boundary_xf.d;
    if dot.abs() > 1e-4 * u_len * v_len {
        return None;
    }
    let verts = vertices(&o.tags, count)?;
    let raw_outline = boundary.unwrap_or(&verts);
    if !(2..=MAX_CLIP_VERTICES).contains(&raw_outline.len()) {
        return None;
    }
    let net = boundary_xf.inverse()?.then(inverse);
    // Expand a 2-corner rectangle to 4 corners when boundary_xf introduces rotation relative
    // to the block, preserving its rotated orientation (e.g. at 45° where 2 corners would collapse).
    let rotated_rect;
    let boundary_rotated = boundary_xf.b.abs() > 1e-9 || boundary_xf.c.abs() > 1e-9;
    let net_rotated = net.b.abs() > 1e-9 || net.c.abs() > 1e-9;
    let outline = match raw_outline {
        [a, b] if boundary_rotated && net_rotated => {
            rotated_rect = vec![*a, Vec2::new(b.x, a.y), *b, Vec2::new(a.x, b.y)];
            &rotated_rect[..]
        }
        other => other,
    };
    let points: Vec<Vec2> = outline.iter().map(|p| net.apply(*p)).collect();
    if !points.iter().all(|p| p.is_finite()) {
        return None;
    }
    let inverted = boundary.is_some() || number(&o.tags, 290).is_some_and(|v| v != 0.0);
    let clip = ClipBoundary { points, enabled: number(&o.tags, 71).unwrap_or(1.0) != 0.0, inverted };
    cadcraft_render::clip::PolyClip::new(&clip.polygon()).is_some().then_some(clip)
}

/// Put the planar filters of the file read into `d` on their references, taking them out of
/// the kept foreign data (the model writes them from now on).
pub(crate) fn read(d: &mut Drawing) {
    if d.foreign.objects.is_empty() {
        return;
    }
    let idx = index(&d.foreign);
    let found: Vec<(Handle, ClipBoundary)> = d
        .foreign
        .entities
        .keys()
        .filter_map(|h| {
            let (_, _, o) = chain(&d.foreign, &idx, *h)?;
            let boundary = roundtrip(&d.foreign, &idx, o);
            Some((*h, parse_filter(o, boundary.as_deref())?))
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
    let mut gone = HashSet::new();
    for h in taken {
        forget(f, &idx, h, &mut gone);
    }
    f.objects.retain(|o| !gone.contains(&o.handle));
}

/// Take the clip of entity `h` out of the foreign data: the ACAD_FILTER entry and what it holds,
/// and the extension dictionary when nothing else is left in it. The
/// objects to remove are added to `gone`; the caller removes them all at once.
fn forget(f: &mut Foreign, idx: &Index, h: Handle, all_gone: &mut HashSet<Handle>) {
    let Some(xd) = f.entities.get(&h).and_then(|x| x.xdict) else { return };
    let mut gone: Vec<Handle> = Vec::new();
    let mut empty = false;
    if let Some(o) = idx.get(&xd).copied().and_then(|i| f.objects.get_mut(i)) {
        gone.extend(remove_entry(&mut o.tags, "ACAD_FILTER"));
        empty = !o.tags.iter().any(|t| t.code == 3);
    }
    // The filter dictionary's entries go with it, and the filter's own extension dictionary.
    let mut i = 0;
    while let Some(g) = gone.get(i).copied() {
        if let Some(o) = object(f, idx, g) {
            if o.kind.eq_ignore_ascii_case("DICTIONARY") {
                gone.extend(o.tags.windows(2).filter_map(|w| match w {
                    [k, v] if k.code == 3 && matches!(v.code, 350 | 360) => Handle::parse_hex(&v.value),
                    _ => None,
                }));
            }
            gone.extend(own_xdict(&o.tags));
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
    all_gone.extend(gone);
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

/// The SPATIAL_FILTER of `clip`; `xdict` is its extension dictionary (an inverted clip's).
/// No group 290: the reference app discards a file whose filter has one.
fn spatial_filter(h: Handle, owner: Handle, clip: &ClipBoundary, to_world: &Mat3, inverse: &Mat3, xdict: Option<Handle>) -> ForeignObject {
    let mut tags = vec![raw(5, h.hex()), raw(102, "{ACAD_REACTORS"), raw(330, owner.hex()), raw(102, "}")];
    if let Some(x) = xdict {
        tags.extend([raw(102, "{ACAD_XDICTIONARY"), raw(360, x.hex()), raw(102, "}")]);
    }
    tags.extend([raw(330, owner.hex()), raw(100, "AcDbFilter"), raw(100, "AcDbSpatialFilter"), raw(70, clip.points.len())]);
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

/// The reference app's inverted-clip record for a filter whose vertices are `points` (drawing
/// coordinates): the boundary and the filter's vertices are the same list.
fn roundtrip_record(h: Handle, owner: Handle, points: &[Vec2]) -> ForeignObject {
    let mut tags = vec![raw(5, h.hex()), raw(102, "{ACAD_REACTORS"), raw(330, owner.hex()), raw(102, "}")];
    tags.extend([raw(330, owner.hex()), raw(100, "AcDbXrecord"), raw(280, 1)]);
    for name in [ROUNDTRIP, COMPARE] {
        tags.push(raw(102, name));
        for p in points {
            tags.extend([raw(10, p.x), raw(20, p.y), raw(30, 0.0)]);
        }
    }
    ForeignObject { handle: h, kind: "XRECORD".into(), tags }
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
    let mut idx = index(f);
    let mut gone = HashSet::new();
    for (insert, clip, to_world) in clips {
        let Some(inverse) = to_world.inverse() else { continue };
        if next.saturating_add(6) >= MAX_HANDLE {
            break;
        }
        // A filter kept from the file for this reference is replaced.
        forget(f, &idx, insert, &mut gone);
        let xd = match f.entities.get(&insert).and_then(|x| x.xdict).filter(|h| idx.contains_key(h) && !gone.contains(h)) {
            Some(h) => h,
            None => {
                let h = fresh(&mut next);
                idx.insert(h, f.objects.len());
                f.objects.push(dictionary(h, insert));
                f.entities.entry(insert).or_default().xdict = Some(h);
                h
            }
        };
        let (fd, sf) = (fresh(&mut next), fresh(&mut next));
        // An inverted clip's filter has an extension dictionary and the record in it.
        let marks = clip.inverted.then(|| (fresh(&mut next), fresh(&mut next)));
        if let Some(o) = idx.get(&xd).copied().and_then(|i| f.objects.get_mut(i)) {
            o.tags.extend([raw(3, "ACAD_FILTER"), raw(360, fd.hex())]);
        }
        let mut filters = dictionary(fd, xd);
        filters.tags.extend([raw(3, "SPATIAL"), raw(360, sf.hex())]);
        let mut add = |o: ForeignObject| {
            idx.insert(o.handle, f.objects.len());
            f.objects.push(o);
        };
        add(filters);
        add(spatial_filter(sf, fd, &clip, &to_world, &inverse, marks.map(|(x, _)| x)));
        if let Some((x, r)) = marks {
            let mut own = dictionary(x, sf);
            own.tags.extend([raw(3, ROUNDTRIP_KEY), raw(360, r.hex())]);
            add(own);
            let world: Vec<Vec2> = clip.points.iter().map(|p| to_world.apply(*p)).collect();
            add(roundtrip_record(r, x, &world));
        }
    }
    f.objects.retain(|o| !gone.contains(&o.handle));
    // The writer hands out handles of its own from the seed on: start it past these.
    out.handseed = out.handseed.max(next);
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
        assert!(parse_filter(&o(vec![raw(70, 1_000_000_000)]), None).is_none());
        assert!(parse_filter(&o(vec![raw(70, 3), raw(10, 0), raw(20, 0)]), None).is_none());
        assert!(parse_filter(&o(vec![raw(70, 2), raw(10, "nan"), raw(20, 0), raw(10, 1), raw(20, 1)]), None).is_none());
        assert!(parse_filter(&o(vec![raw(70, 2), raw(10, 0), raw(20, 0), raw(10, 1), raw(20, 1), raw(72, 1)]), None).is_none());
        let ok = parse_filter(&o(vec![raw(70, 2), raw(10, 0), raw(20, 0), raw(10, 1), raw(20, 1)]), None).unwrap();
        assert_eq!((ok.points.len(), ok.inverted), (2, false));
    }

    #[test]
    fn the_writer_starts_its_handles_past_the_clip_objects() {
        let mut d = Drawing::new_metric();
        d.blocks.insert("B".into(), Arc::new(cadcraft_doc::Block::new("B")));
        for i in 0..3 {
            let clip = ClipBoundary::new(vec![Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)]);
            let ins = cadcraft_doc::Insert { block: "B".into(), clip: Some(ClipBoundary { inverted: i == 0, ..clip }), ..Default::default() };
            d.add(&cadcraft_doc::Space::Model, cadcraft_doc::Common::default(), EntityKind::Insert(ins)).unwrap();
        }
        let out = prepare(&d).unwrap();
        let highest = out.foreign.objects.iter().map(|o| o.handle.0).max().unwrap();
        assert!(out.handseed > highest, "seed {} at or below {highest}", out.handseed);
    }

    #[test]
    fn group_290_is_read_but_never_written() {
        let tags = |invert: &str| vec![raw(70, 2), raw(10, 0), raw(20, 0), raw(10, 1), raw(20, 1), raw(290, invert)];
        let o = |invert| ForeignObject { handle: Handle(1), kind: "SPATIAL_FILTER".into(), tags: tags(invert) };
        assert!(parse_filter(&o("1"), None).unwrap().inverted);
        assert!(!parse_filter(&o("0"), None).unwrap().inverted);
        assert!(!parse_filter(&o("junk"), None).unwrap().inverted);
        // The reference app discards a DXF whose filter has 290.
        let clip = ClipBoundary { inverted: true, ..ClipBoundary::new(vec![Vec2::ZERO, Vec2::new(1.0, 1.0)]) };
        let written = spatial_filter(Handle(2), Handle(1), &clip, &Mat3::IDENTITY, &Mat3::IDENTITY, Some(Handle(3)));
        assert!(!written.tags.iter().any(|t| t.code == 290));
        assert_eq!(own_xdict(&written.tags), Some(Handle(3)));
    }

    /// The filter, its extension dictionary and the reference app's record, as one foreign set.
    fn marked(filter_points: &[(f64, f64)], boundary: &[(f64, f64)], compare: &[(f64, f64)]) -> (Foreign, ForeignObject) {
        let mut tags = vec![raw(5, "10"), raw(102, "{ACAD_XDICTIONARY"), raw(360, "11"), raw(102, "}"), raw(70, filter_points.len())];
        for (x, y) in filter_points {
            tags.extend([raw(10, x), raw(20, y)]);
        }
        let filter = ForeignObject { handle: Handle(0x10), kind: "SPATIAL_FILTER".into(), tags };
        let mut f = Foreign::default();
        let mut dict = dictionary(Handle(0x11), Handle(0x10));
        dict.tags.extend([raw(3, ROUNDTRIP_KEY), raw(360, "12")]);
        f.objects.push(dict);
        let mut rec = vec![raw(100, "AcDbXrecord"), raw(280, 1)];
        for (name, list) in [(ROUNDTRIP, boundary), (COMPARE, compare)] {
            rec.push(raw(102, name));
            for (x, y) in list {
                rec.extend([raw(10, x), raw(20, y), raw(30, 0)]);
            }
        }
        f.objects.push(ForeignObject { handle: Handle(0x12), kind: "XRECORD".into(), tags: rec });
        f.objects.push(filter.clone());
        (f, filter)
    }

    #[test]
    fn the_reference_apps_record_marks_an_inverted_clip() {
        // As the reference app writes an inverted rectangle: a keyhole filter, the rectangle kept.
        let keyhole = [
            (0.0, 0.0),
            (100.0, 0.0),
            (100.0, 50.0),
            (0.0, 50.0),
            (0.0, 10.0),
            (20.0, 10.0),
            (20.0, 40.0),
            (80.0, 40.0),
            (80.0, 10.0),
            (20.0, 10.0),
            (0.0, 10.0),
        ];
        let (f, filter) = marked(&keyhole, &[(20.0, 10.0), (80.0, 40.0)], &keyhole);
        let boundary = roundtrip(&f, &index(&f), &filter).unwrap();
        let clip = parse_filter(&filter, Some(&boundary)).unwrap();
        assert!(clip.inverted);
        assert_eq!(clip.points, vec![Vec2::new(20.0, 10.0), Vec2::new(80.0, 40.0)]);
        // The simpler form: the filter keeps the boundary itself.
        let rect = [(20.0, 10.0), (80.0, 40.0)];
        let (f, filter) = marked(&rect, &rect, &rect);
        assert!(roundtrip(&f, &index(&f), &filter).is_some());
        // A filter changed since the record was made (the clip edited elsewhere) isn't inverted.
        let (f, filter) = marked(&[(0.0, 0.0), (5.0, 5.0)], &rect, &rect);
        assert!(roundtrip(&f, &index(&f), &filter).is_none());
    }

    #[test]
    fn a_hostile_vertex_count_beside_the_record_does_not_allocate() {
        let rect = [(20.0, 10.0), (80.0, 40.0)];
        for count in ["9223372036854775807", "1000000000", "1e300"] {
            let (mut f, mut filter) = marked(&rect, &rect, &rect);
            for t in filter.tags.iter_mut().filter(|t| t.code == 70) {
                t.value = count.into();
            }
            f.objects.retain(|o| o.handle != filter.handle);
            f.objects.push(filter.clone());
            assert!(roundtrip(&f, &index(&f), &filter).is_none(), "{count}");
            assert!(parse_filter(&filter, None).is_none(), "{count}");
        }
    }

    /// Builds a mock SPATIAL_FILTER [`ForeignObject`] with a 10×10 rectangle boundary:
    /// (0,0), (10,0), (10,10), (0,10).
    fn make_test_filter(inverse: &Mat3, own: &Mat3) -> ForeignObject {
        let mut tags = vec![
            raw(100, "AcDbFilter"),
            raw(100, "AcDbSpatialFilter"),
            raw(70, 4),
            raw(10, 0.0),
            raw(20, 0.0),
            raw(10, 10.0),
            raw(20, 0.0),
            raw(10, 10.0),
            raw(20, 10.0),
            raw(10, 0.0),
            raw(20, 10.0),
            raw(210, 0.0),
            raw(220, 0.0),
            raw(230, 1.0),
            raw(71, 1),
            raw(72, 0),
            raw(73, 0),
        ];
        for v in matrix_values(inverse).into_iter().chain(matrix_values(own)) {
            tags.push(raw(40, v));
        }
        ForeignObject { handle: Handle(0x100), kind: "SPATIAL_FILTER".into(), tags }
    }

    /// Baseline check: verifies `parse_filter` accepts filters where the boundary matrix is identity.
    #[test]
    fn parse_filter_accepts_identity_boundary_transform() {
        // Baseline: parse_filter accepts filters where the second matrix is Identity.
        let filter = make_test_filter(&Mat3::IDENTITY, &Mat3::IDENTITY);
        let parsed = parse_filter(&filter, None);
        assert!(parsed.is_some(), "Baseline filter with identity own matrix must be accepted");
    }

    /// Regression test: verifies `parse_filter` preserves filters with a non-identity translation matrix.
    #[test]
    fn parse_filter_preserves_non_identity_boundary_transform() {
        // WHY:
        // SPATIAL_FILTER can contain two transformation matrices.
        // parse_filter previously rejected every non-identity `own` matrix,
        // causing an otherwise valid clipping definition to be discarded.
        //
        // Matrix 2 transforms WCS into boundary coordinates; its inverse transforms
        // boundary coordinates into WCS, mapping (0..10, 0..10) shifted by (-100, -50).

        let inverse = Mat3::IDENTITY;
        let own = Mat3::translate(Vec2::new(100.0, 50.0));

        let filter = make_test_filter(&inverse, &own);
        let parsed = parse_filter(&filter, None);

        assert!(parsed.is_some(), "A valid non-identity boundary transform must not cause the filter to be discarded");
        let clip = parsed.unwrap();
        assert_eq!(clip.points.len(), 4);
        assert!(clip.points[0].near(Vec2::new(-100.0, -50.0), 1e-9));
        assert!(clip.points[1].near(Vec2::new(-90.0, -50.0), 1e-9));
        assert!(clip.points[2].near(Vec2::new(-90.0, -40.0), 1e-9));
        assert!(clip.points[3].near(Vec2::new(-100.0, -40.0), 1e-9));
    }

    /// Verifies `boundary_xf.inverse()?.then(inverse)` correctly composes rotation, scaling, and translation.
    #[test]
    fn parse_filter_applies_rotation_and_scale_boundary_transform() {
        let boundary_xf = Mat3::scale(2.0, 2.0).then(Mat3::rotate(std::f64::consts::FRAC_PI_2)).then(Mat3::translate(Vec2::new(50.0, 20.0)));
        let inverse = Mat3::translate(Vec2::new(-10.0, -5.0));

        let filter = make_test_filter(&inverse, &boundary_xf);
        let parsed = parse_filter(&filter, None);

        assert!(parsed.is_some(), "Combined scale, rotation, and translation boundary transform must be parsed");
        let clip = parsed.unwrap();
        assert_eq!(clip.points.len(), 4);
        assert!(clip.points[0].near(Vec2::new(-20.0, 20.0), 1e-9));
        assert!(clip.points[1].near(Vec2::new(-20.0, 15.0), 1e-9));
        assert!(clip.points[2].near(Vec2::new(-15.0, 15.0), 1e-9));
        assert!(clip.points[3].near(Vec2::new(-15.0, 20.0), 1e-9));
    }
    /// Verifies `parse_filter` rejects sheared boundary matrices to preserve foreign data.
    #[test]
    fn parse_filter_rejects_sheared_boundary_transform() {
        let inverse = Mat3::IDENTITY;
        // Non-orthogonal shear matrix: (x' = x + 0.5y, y' = y)
        let shear = Mat3 { a: 1.0, b: 0.0, c: 0.5, d: 1.0, e: 0.0, f: 0.0 };
        let filter = make_test_filter(&inverse, &shear);
        assert!(parse_filter(&filter, None).is_none(), "Sheared transforms must be rejected to stay foreign");
    }

    /// Verifies that an explicit boundary slice (e.g. inverted clip) is transformed correctly.
    #[test]
    fn parse_filter_applies_transform_to_inverted_boundary() {
        let inverse = Mat3::translate(Vec2::new(-10.0, 0.0));
        let boundary_xf = Mat3::translate(Vec2::new(50.0, 0.0));
        let filter = make_test_filter(&inverse, &boundary_xf);

        let explicit_boundary = vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)];
        let parsed = parse_filter(&filter, Some(&explicit_boundary));
        assert!(parsed.is_some());
        let clip = parsed.unwrap();
        assert!(clip.inverted);
        // Explicit boundary points transformed by net: (x - 50 - 10) = x - 60
        assert_eq!(clip.points.len(), 3);
        assert!(clip.points[0].near(Vec2::new(-60.0, 0.0), 1e-9));
        assert!(clip.points[1].near(Vec2::new(-50.0, 0.0), 1e-9));
        assert!(clip.points[2].near(Vec2::new(-50.0, 10.0), 1e-9));
    }
    /// Verifies that small-scale sheared transforms are rejected by the scale-independent guard.
    #[test]
    fn parse_filter_rejects_small_scale_sheared_boundary_transform() {
        // WHY:
        // A sheared transform with tiny scale factors (e.g. u = (0.001, 0), v = (0.0005, 0.001))
        // yields a small absolute dot product (5e-7), which would bypass a naive absolute tolerance.
        // The orthogonality guard is scale-independent (|u . v| > 1e-4 * |u| * |v|) so that
        // non-orthogonal shears are consistently rejected across all coordinate scales.
        let inverse = Mat3::IDENTITY;
        let small_shear = Mat3 { a: 0.001, b: 0.0, c: 0.0005, d: 0.001, e: 0.0, f: 0.0 };
        let filter = make_test_filter(&inverse, &small_shear);
        assert!(parse_filter(&filter, None).is_none(), "Small-scale sheared transform must be rejected");
    }

    /// Verifies that a 2-corner rectangle rotated by 30°, 45°, 60°, or 90° expands to an exact
    /// 4-corner orthogonal rectangle in block coordinates without collapsing or skewing.
    #[test]
    fn parse_filter_expands_rotated_two_corner_boundary() {
        for deg in [30.0_f64, 45.0, 60.0, 90.0] {
            let mut tags = vec![
                raw(100, "AcDbFilter"),
                raw(100, "AcDbSpatialFilter"),
                raw(70, 2),
                raw(10, 0.0),
                raw(20, 0.0),
                raw(10, 10.0),
                raw(20, 20.0),
                raw(210, 0.0),
                raw(220, 0.0),
                raw(230, 1.0),
                raw(71, 1),
                raw(72, 0),
                raw(73, 0),
            ];
            let boundary_xf = Mat3::rotate(deg.to_radians());
            for v in matrix_values(&Mat3::IDENTITY).into_iter().chain(matrix_values(&boundary_xf)) {
                tags.push(raw(40, v));
            }
            let filter = ForeignObject { handle: Handle(0x102), kind: "SPATIAL_FILTER".into(), tags };
            let clip = parse_filter(&filter, None).expect("rotated 2-corner clip must not collapse");
            assert_eq!(clip.points.len(), 4, "angle {deg}°");
            let e01 = clip.points[1] - clip.points[0];
            let e12 = clip.points[2] - clip.points[1];
            assert!((e01.len() - 10.0).abs() < 1e-9, "width preserved at {deg}°");
            assert!((e12.len() - 20.0).abs() < 1e-9, "height preserved at {deg}°");
            assert!(e01.dot(e12).abs() < 1e-9, "right angle preserved at {deg}°");
        }
    }
}
