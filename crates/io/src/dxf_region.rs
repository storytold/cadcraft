//! Regions in DXF. A REGION entity holds ACIS solid-modeler data, which no public specification
//! describes in enough detail, so CADCraft writes a region as closed LWPOLYLINEs, one per loop,
//! which every DXF reader understands. The first carries `CADCRAFT` xdata naming it a region
//! and holding its other loops (`1000 REGION`); the others are marked `1000 REGIONPART`. Reading
//! our own file builds the region back from the first and drops the others. Foreign REGIONs are
//! kept as unknown objects.

use cadcraft_doc::{EntityKind, HatchLoop, LwPolyline, MAX_REGION_LOOPS, Region};
use cadcraft_geom::{PolyVertex, Vec2};
use craft_dxf::Tag;

use crate::dxf_ext::{APP, xdata, xdata_list};

/// Most vertices of a region's other loops kept in xdata (its size is limited in other
/// programs); past it those loops are written as plain polylines.
const MAX_XDATA_VERTICES: usize = 300;
/// Most vertices read back per region.
const MAX_VERTICES: usize = 1_000_000;

/// One closed LWPOLYLINE: the groups after the common entity groups.
fn polyline_tags(l: &HatchLoop) -> Vec<Tag> {
    let mut t = vec![Tag::i(90, l.vertices.len() as i64), Tag::i(70, 1)];
    for v in &l.vertices {
        t.push(Tag::f(10, v.p.x));
        t.push(Tag::f(20, v.p.y));
        if v.bulge != 0.0 && v.bulge.is_finite() {
            t.push(Tag::f(42, v.bulge));
        }
    }
    t
}

/// The polylines a region is written as (each the groups after the common entity groups,
/// xdata last). The first takes the region's handle.
pub(crate) fn polylines(r: &Region) -> Vec<Vec<Tag>> {
    let Some((first, rest)) = r.loops.split_first() else { return Vec::new() };
    let in_xdata = rest.iter().map(|l| l.vertices.len()).sum::<usize>() <= MAX_XDATA_VERTICES;
    let mut main = polyline_tags(first);
    main.extend([Tag::s(1001, APP), Tag::s(1000, "REGION"), Tag::s(1002, "{"), Tag::i(1070, i64::from(first.outer))]);
    if in_xdata {
        for l in rest {
            main.extend([Tag::s(1000, "LOOP"), Tag::i(1070, i64::from(l.outer)), Tag::i(1071, l.vertices.len() as i64)]);
            for v in &l.vertices {
                main.extend([Tag::f(1010, v.p.x), Tag::f(1020, v.p.y), Tag::f(1030, 0.0), Tag::f(1040, v.bulge)]);
            }
        }
    }
    main.push(Tag::s(1002, "}"));
    let mut out = vec![main];
    for l in rest {
        let mut t = polyline_tags(l);
        if in_xdata {
            t.extend([Tag::s(1001, APP), Tag::s(1000, "REGIONPART")]);
        }
        out.push(t);
    }
    out
}

/// What a closed LWPOLYLINE read as `pl` stands for: a region CADCraft wrote, nothing (a loop
/// of such a region, read with it), or the polyline itself.
pub(crate) fn from_polyline(tags: &[Tag], pl: LwPolyline) -> Option<EntityKind> {
    let x = xdata(tags, APP);
    if x.iter().any(|t| t.code == 1000 && t.str().trim() == "REGIONPART") {
        return None;
    }
    if !x.iter().any(|t| t.code == 1000 && t.str().trim() == "REGION") {
        return Some(EntityKind::LwPolyline(pl));
    }
    let list = xdata_list(x, "REGION");
    let fin = |v: &PolyVertex| v.p.is_finite() && v.bulge.is_finite();
    let first_outer = list.iter().find(|t| t.code == 1070).is_none_or(|t| t.i64() != 0);
    let mut loops = vec![HatchLoop { vertices: pl.vertices.into_iter().filter(fin).collect(), outer: first_outer, sources: Vec::new() }];
    let mut total = 0usize;
    let mut cur: Option<HatchLoop> = None;
    let mut expect_flag = false;
    for t in list {
        match t.code {
            1000 if t.str().trim() == "LOOP" => {
                loops.extend(cur.take());
                if loops.len() >= MAX_REGION_LOOPS {
                    break;
                }
                cur = Some(HatchLoop { vertices: Vec::new(), outer: false, sources: Vec::new() });
                expect_flag = true;
            }
            1070 if expect_flag => {
                if let Some(l) = cur.as_mut() {
                    l.outer = t.i64() != 0;
                }
                expect_flag = false;
            }
            1010 => {
                if let Some(l) = cur.as_mut()
                    && total < MAX_VERTICES
                {
                    l.vertices.push(PolyVertex::new(Vec2::new(t.f64(), 0.0)));
                    total += 1;
                }
            }
            1020 => {
                if let Some(v) = cur.as_mut().and_then(|l| l.vertices.last_mut()) {
                    v.p.y = t.f64();
                }
            }
            1040 => {
                if let Some(v) = cur.as_mut().and_then(|l| l.vertices.last_mut()) {
                    v.bulge = t.f64();
                }
            }
            _ => {}
        }
    }
    loops.extend(cur);
    for l in &mut loops {
        l.vertices.retain(fin);
    }
    loops.retain(|l| l.vertices.len() >= 2);
    Some(EntityKind::Region(Region { loops }))
}
