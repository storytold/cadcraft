//! 3D solids: BOX, CYLINDER, CONE, SPHERE, TORUS, PYRAMID and the profile commands EXTRUDE,
//! REVOLVE, SWEEP and LOFT.
//!
//! A solid is a closed, outward-oriented triangle mesh ([`cadcraft_geom::Mesh3`]) stored as
//! ordinary `3DFACE` objects (a triangle repeats its third corner as the fourth; edges that
//! only separate coplanar triangles are flagged invisible). No object type was added. The
//! faces of one solid form ONE unnamed, selectable group (`*A<n>`, description
//! [`SOLID_GROUP`]): picking one face selects the whole solid, so MOVE, ROTATE, COPY and ERASE
//! treat it as one object, and the group is saved in the DXF (ACAD_GROUP) and survives reopening.
//! Every command is a single undo step.
//!
//! Shared with the other 3D commands (stable signatures): [`emit_mesh`], [`mesh_of`] and
//! [`solid_groups`].

use std::collections::HashMap;

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Common, Drawing, EntityKind, Face3d, Handle};
use cadcraft_geom::{EPS, Mesh3, TAU, Vec2, Vec3};
use serde_json::{Value, json};

use super::{CommandSpec, bad, bool_or, point_value, targets};
use crate::{EngineError, Result, Session};

mod interactive;
mod profile;

use profile::{MAX_PROFILE, path_of, profile_about_axis, profile_of, signed_area};

/// The description of the group that holds one solid's faces.
pub(crate) const SOLID_GROUP: &str = "3D solid";
/// Most faces one command adds (the geometry crate allows 100 000).
const MAX_FACES: usize = 50_000;
/// Most solids one command makes (one per profile object).
const MAX_SOLIDS: usize = 64;
/// Most sections of a loft.
const MAX_SECTIONS: usize = 256;
const DEFAULT_SEGMENTS: usize = 32;
const MAX_SEGMENTS: usize = 512;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("box", "Box", run_box)
            .menu(&["Draw", "Modeling", "Box"])
            .params("{p1: [x,y,z?], p2: [x,y,z?], height? (p2's Z is replaced by p1.z + height)} | {center|corner: [x,y,z?], length, width, height} | {center|corner, side (cube)}. `center` is the middle of the box, `corner` the lowest-left corner; with `corner` a negative size extends the other way → {handles, faces, group, kind}")
            .interactive(|_| Ok(Box::new(interactive::PrimM::new(interactive::Kind::Box)))),
        CommandSpec::new("cylinder", "Cylinder", run_cylinder)
            .menu(&["Draw", "Modeling", "Cylinder"])
            .params("{center?: [x,y,z?] (centre of the base, default the origin), radius | diameter, height (negative extrudes downward), segments? (3..512, default 32)} → {handles, faces, group, kind}")
            .interactive(|_| Ok(Box::new(interactive::PrimM::new(interactive::Kind::Cylinder)))),
        CommandSpec::new("cone", "Cone", run_cone)
            .menu(&["Draw", "Modeling", "Cone"])
            .params("{center?: [x,y,z?] (centre of the base), radius | diameter, height (negative points the apex down), segments? (3..512, default 32)} → {handles, faces, group, kind}")
            .interactive(|_| Ok(Box::new(interactive::PrimM::new(interactive::Kind::Cone)))),
        CommandSpec::new("sphere", "Sphere", run_sphere)
            .menu(&["Draw", "Modeling", "Sphere"])
            .params("{center?: [x,y,z?], radius | diameter, segments? (3..512, default 32), rings? (2..256, default 16)} → {handles, faces, group, kind}")
            .interactive(|_| Ok(Box::new(interactive::PrimM::new(interactive::Kind::Sphere)))),
        CommandSpec::new("torus", "Torus", run_torus)
            .menu(&["Draw", "Modeling", "Torus"])
            .params("{center?: [x,y,z?], radius | diameter (centre of the tube circle), tubeRadius | tubeDiameter (smaller than radius), segments? (around the ring, default 32), tubeSegments? (default 16)} → {handles, faces, group, kind}")
            .interactive(|_| Ok(Box::new(interactive::PrimM::new(interactive::Kind::Torus)))),
        CommandSpec::new("pyramid", "Pyramid", run_pyramid)
            .menu(&["Draw", "Modeling", "Pyramid"])
            .params("{center?: [x,y,z?] (centre of the base), radius (to the base corners) | side (base edge length), sides? (3..256, default 4), height (negative points the apex down), rotation? (degrees; default: the bottom edge is horizontal)} → {handles, faces, group, kind}")
            .interactive(|_| Ok(Box::new(interactive::PrimM::new(interactive::Kind::Pyramid)))),
        CommandSpec::new("extrude", "Extrude", run_extrude)
            .menu(&["Draw", "Modeling", "Extrude"])
            .params("{profile: [[x,y],...]} | {handles? (default the selection): closed polylines (bulges flattened), circles, full ellipses}, height (negative extrudes downward), baseZ? (default the profile's elevation, 0 for `profile`), segments? (circle corners, default 32), deleteSource?: bool (default false). One solid per profile → {handles, faces, group, groups, solids}")
            .interactive(|_| Ok(Box::new(interactive::ObjM::new(interactive::Op::Extrude)))),
        CommandSpec::new("revolve", "Revolve", run_revolve)
            .menu(&["Draw", "Modeling", "Revolve"])
            .params("{profile: [[radius,z],...]} | {handles? (default the selection): closed profiles drawn in the XY plane}, segments? (default 32), axis? / revolveAxis?: [[x,y,z],[x,y,z]] (axis line; handles must lie in a plane containing it), axisX? (legacy vertical axis for drawn handles), startAngle? (degrees, default 0), angle? (degrees, signed; default 360; partial turns are capped), center?: [x,y,z?] (legacy/default axis origin), deleteSource?. The profile may touch the axis along one edge. → {handles, faces, group, groups, solids}")
            .interactive(|_| Ok(Box::new(interactive::ObjM::new(interactive::Op::Revolve)))),
        CommandSpec::new("sweep", "Sweep", run_sweep)
            .menu(&["Draw", "Modeling", "Sweep"])
            .params("{profile: [[x,y],...] (relative to path points) | handles? (closed profiles, centred on their bounding box or profileOrigin: [x,y]; default the selection)}, path: [[x,y,z],...] | pathHandle (line, polyline, 3D polyline or arc), twist? (degrees, total path twist), scaleStart? (positive, default 1), scaleEnd? (positive, default 1; `scale` aliases the end scale), segments?, deleteSource? → {handles, faces, group, groups, solids}")
            .interactive(|_| Ok(Box::new(interactive::ObjM::new(interactive::Op::Sweep)))),
        CommandSpec::new("loft", "Loft", run_loft)
            .menu(&["Draw", "Modeling", "Loft"])
            .params("{sections: [[[x,y,z],...],...]} (planar sections in order; sections may be tilted and have different point counts) | {handles?: closed profiles with equal point counts, spacing? (stack in selection order `spacing` apart from baseZ?; default: sorted by elevation), segments?}, deleteSource? → {handles, faces, group, groups, solids}")
            .interactive(|_| Ok(Box::new(interactive::ObjM::new(interactive::Op::Loft)))),
    ]
}

// ---------------- shared API ----------------

/// The corners of the triangles of `mesh` as `3DFACE` data. A triangle repeats its third corner;
/// an edge shared with exactly one neighbouring triangle of the same plane is flagged invisible.
pub(crate) fn faces_of(mesh: &Mesh3) -> Vec<Face3d> {
    let hidden = hidden_edges(mesh);
    let mut out = Vec::with_capacity(mesh.triangles.len());
    for (i, t) in mesh.triangles.iter().enumerate() {
        let (Some(a), Some(b), Some(c)) = (mesh.vertices.get(t[0]), mesh.vertices.get(t[1]), mesh.vertices.get(t[2])) else { continue };
        out.push(Face3d { corners: [*a, *b, *c, *c], hidden_edges: hidden.get(i).copied().unwrap_or(0) });
    }
    out
}

fn triangle_normal(m: &Mesh3, t: &[usize; 3]) -> Option<Vec3> {
    let (a, b, c) = (m.vertices.get(t[0])?, m.vertices.get(t[1])?, m.vertices.get(t[2])?);
    let n = (*b - *a).cross(*c - *a);
    (n.len() > 0.0).then(|| n.normalized())
}

/// Per triangle: bits 1 (a-b), 2 (b-c) and 8 (c-a) for edges between coplanar neighbours.
fn hidden_edges(m: &Mesh3) -> Vec<u8> {
    let mut edges: HashMap<(usize, usize), Vec<(usize, u8)>> = HashMap::new();
    for (i, t) in m.triangles.iter().enumerate() {
        for (k, bit) in [(0usize, 1u8), (1, 2), (2, 8)] {
            let (Some(a), Some(b)) = (t.get(k).copied(), t.get((k + 1) % 3).copied()) else { continue };
            edges.entry((a.min(b), a.max(b))).or_default().push((i, bit));
        }
    }
    let mut out = vec![0u8; m.triangles.len()];
    for users in edges.values() {
        let [(i, bi), (j, bj)] = users.as_slice() else { continue };
        let (Some(ti), Some(tj)) = (m.triangles.get(*i), m.triangles.get(*j)) else { continue };
        let (Some(ni), Some(nj)) = (triangle_normal(m, ti), triangle_normal(m, tj)) else { continue };
        if ni.dot(nj) >= 1.0 - 1e-9 {
            if let Some(f) = out.get_mut(*i) {
                *f |= bi;
            }
            if let Some(f) = out.get_mut(*j) {
                *f |= bj;
            }
        }
    }
    out
}

/// Common properties for new faces: the current layer, colour, linetype and lineweight.
fn new_common(s: &Session) -> Common {
    let mut c = s.current_common();
    if let Some(rgb) = s.doc().ok().and_then(|d| d.header.get("CECOLOR_RGB")).and_then(|v| v.as_i64()).and_then(|v| u32::try_from(v).ok()) {
        c.color = Color::True(Rgb::from_u32(rgb));
    }
    c
}

/// Add several solids at once: every triangle becomes a `3DFACE` on the current layer and each
/// solid's faces one group. All or nothing: a failure leaves the drawing as it was.
fn emit_many(s: &mut Session, cmd: &str, meshes: &[(Mesh3, &str)]) -> Result<Value> {
    let total: usize = meshes.iter().map(|(m, _)| m.triangles.len()).sum();
    if meshes.is_empty() || total == 0 {
        return Err(bad(cmd, "no solid to add"));
    }
    if total > MAX_FACES {
        return Err(bad(cmd, format!("{total} faces is more than the {MAX_FACES} a command may add; use fewer segments")));
    }
    let backup = s.state()?.doc.clone();
    let result = add_faces(s, cmd, meshes);
    if result.is_err()
        && let Ok(st) = s.state_mut()
    {
        st.doc = backup;
    }
    result
}

fn add_faces(s: &mut Session, cmd: &str, meshes: &[(Mesh3, &str)]) -> Result<Value> {
    let common = new_common(s);
    let space = s.space();
    let mut all = Vec::new();
    let mut groups = Vec::new();
    for (mesh, _) in meshes {
        let mut hs = Vec::with_capacity(mesh.triangles.len());
        for f in faces_of(mesh) {
            hs.push(s.doc_mut()?.add(&space, common.clone(), EntityKind::Face3d(f))?);
        }
        groups.push(super::groups::create(s, None, SOLID_GROUP, &hs, true)?);
        all.extend(hs);
    }
    let kind = meshes.first().map(|(_, k)| *k).unwrap_or(cmd);
    Ok(json!({
        "handles": all.iter().map(|h| h.hex()).collect::<Vec<_>>(),
        "faces": all.len(),
        "group": groups.first(),
        "groups": groups,
        "solids": meshes.len(),
        "kind": kind,
    }))
}

/// Add `mesh` as a solid: its triangles as `3DFACE` objects on the current layer, in one new
/// unnamed selectable group described [`SOLID_GROUP`]. Returns `{handles, faces, group, ...}`.
/// `label` names the kind of solid (`"box"`, `"union"`…).
pub(crate) fn emit_mesh(s: &mut Session, cmd: &str, mesh: &Mesh3, label: &str) -> Result<Value> {
    emit_many(s, cmd, &[(mesh.clone(), label)])
}

/// Weld tolerance of [`mesh_of`], in drawing units (relative to nothing: faces are exact copies).
const WELD: f64 = 1e-6;

/// The triangles of the `3DFACE` objects `handles` as one mesh. Identical corners (within
/// 1e-6) are welded into shared vertices, quads are split in two, degenerate faces are skipped
/// and other objects ignored. Errors when no triangle results or the count exceeds the cap.
pub(crate) fn mesh_of(d: &Drawing, handles: &[Handle]) -> Result<Mesh3> {
    if handles.len() > MAX_FACES * 2 {
        return Err(bad("solid", "too many faces"));
    }
    let mut index: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut vertices: Vec<Vec3> = Vec::new();
    let mut triangles: Vec<[usize; 3]> = Vec::new();
    let mut weld = |p: Vec3| -> Option<usize> {
        if !p.is_finite() {
            return None;
        }
        let key = ((p.x / WELD).round() as i64, (p.y / WELD).round() as i64, (p.z / WELD).round() as i64);
        Some(*index.entry(key).or_insert_with(|| {
            vertices.push(p);
            vertices.len() - 1
        }))
    };
    for h in handles {
        let Some(e) = d.entity(*h) else { continue };
        let EntityKind::Face3d(f) = &e.kind else { continue };
        let ids: Vec<Option<usize>> = f.corners.iter().map(|c| weld(*c)).collect();
        let [Some(a), Some(b), Some(c), Some(dd)] = ids.as_slice() else { continue };
        let tris: &[[usize; 3]] = if c == dd { &[[*a, *b, *c]] } else { &[[*a, *b, *c], [*a, *c, *dd]] };
        for t in tris {
            if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
                triangles.push(*t);
            }
        }
        if triangles.len() > MAX_FACES * 2 {
            return Err(bad("solid", "too many faces"));
        }
    }
    if triangles.is_empty() {
        return Err(bad("solid", "the objects hold no 3D faces"));
    }
    Ok(Mesh3 { vertices, triangles })
}

/// The solids (groups described [`SOLID_GROUP`]) the faces in `selection` belong to, as
/// (group name, member handles that still exist), each once, in the order first met.
pub(crate) fn solid_groups(d: &Drawing, selection: &[Handle]) -> Vec<(String, Vec<Handle>)> {
    let mut out: Vec<(String, Vec<Handle>)> = Vec::new();
    for g in d.groups.iter().filter(|g| g.description == SOLID_GROUP) {
        if out.iter().any(|(n, _)| *n == g.name) || !g.members.iter().any(|m| selection.contains(m)) {
            continue;
        }
        let members: Vec<Handle> = g.members.iter().copied().filter(|m| d.entity(*m).is_some()).collect();
        if !members.is_empty() {
            out.push((g.name.clone(), members));
        }
    }
    out
}

// ---------------- parameters ----------------

fn num(cmd: &str, p: &Value, key: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|x| x.is_finite()).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be a finite number"))),
    }
}

fn num_req(cmd: &str, p: &Value, key: &str) -> Result<f64> {
    num(cmd, p, key)?.ok_or_else(|| bad(cmd, format!("`{key}` (number) is required")))
}

/// A point: `[x, y]`, `[x, y, z]` or `{x, y, z?}` of finite numbers (Z defaults to 0).
fn point3(v: &Value) -> Option<Vec3> {
    let n = |x: Option<&Value>| x.and_then(Value::as_f64).filter(|f| f.is_finite());
    let (x, y, z) = if let Some(a) = v.as_array() {
        if a.len() < 2 || a.len() > 3 {
            return None;
        }
        (n(a.first())?, n(a.get(1))?, if a.len() == 3 { n(a.get(2))? } else { 0.0 })
    } else if v.is_object() {
        (n(v.get("x"))?, n(v.get("y"))?, if v.get("z").is_some() { n(v.get("z"))? } else { 0.0 })
    } else {
        return None;
    };
    Some(Vec3::new(x, y, z))
}

fn p3(cmd: &str, p: &Value, key: &str) -> Result<Option<Vec3>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => point3(v).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be [x, y] or [x, y, z]"))),
    }
}

fn center_of(cmd: &str, p: &Value) -> Result<Vec3> {
    Ok(p3(cmd, p, "center")?.unwrap_or(Vec3::ZERO))
}

/// An integer in `min..=max` (`default` when absent).
fn count(cmd: &str, p: &Value, key: &str, default: usize, min: usize, max: usize) -> Result<usize> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (min..=max).contains(n))
            .ok_or_else(|| bad(cmd, format!("`{key}` must be a whole number from {min} to {max}"))),
    }
}

/// A positive size given as `{key}` or as `{diam_key}` (halved).
fn size_or_diameter(cmd: &str, p: &Value, key: &str, diam_key: &str) -> Result<f64> {
    let v = match num(cmd, p, key)? {
        Some(r) => r,
        None => num(cmd, p, diam_key)?.ok_or_else(|| bad(cmd, format!("`{key}` or `{diam_key}` is required")))? / 2.0,
    };
    if v > 0.0 { Ok(v) } else { Err(bad(cmd, format!("`{key}` must be positive"))) }
}

fn height_of(cmd: &str, p: &Value) -> Result<f64> {
    let h = num_req(cmd, p, "height")?;
    if h == 0.0 { Err(bad(cmd, "`height` must not be zero")) } else { Ok(h) }
}

fn mesh_err(cmd: &str) -> impl Fn(cadcraft_geom::MeshError) -> EngineError + '_ {
    move |e| bad(cmd, e.to_string())
}

/// `[[x, y], ...]` (at most [`MAX_PROFILE`] points).
fn points2(cmd: &str, p: &Value, key: &str) -> Result<Vec<Vec2>> {
    let a = p.get(key).and_then(Value::as_array).ok_or_else(|| bad(cmd, format!("`{key}` must be a list of [x, y] points")))?;
    if a.len() > MAX_PROFILE {
        return Err(bad(cmd, format!("`{key}` holds at most {MAX_PROFILE} points")));
    }
    a.iter().map(|v| point_value(v).ok_or_else(|| bad(cmd, format!("`{key}`: {v} is not an [x, y] point")))).collect()
}

/// Mirror in the plane z = `z0` (triangles keep facing outward).
fn mirror_z(m: &mut Mesh3, z0: f64) {
    for v in &mut m.vertices {
        v.z = 2.0 * z0 - v.z;
    }
    for t in &mut m.triangles {
        t.swap(1, 2);
    }
}

// ---------------- primitives ----------------

fn box_dims(cmd: &str, p: &Value) -> Result<Vec3> {
    if let Some(side) = num(cmd, p, "side")? {
        return Ok(Vec3::new(side, side, side));
    }
    Ok(Vec3::new(num_req(cmd, p, "length")?, num_req(cmd, p, "width")?, num_req(cmd, p, "height")?))
}

pub(super) fn build_box(p: &Value) -> Result<Mesh3> {
    let c = "box";
    let (a, b) = if let (Some(a), Some(mut b)) = (p3(c, p, "p1")?, p3(c, p, "p2")?) {
        match num(c, p, "height")? {
            Some(h) => b.z = a.z + h,
            None if (a.z - b.z).abs() <= EPS => return Err(bad(c, "give `height` (or points with different Z)")),
            None => {}
        }
        (a, b)
    } else if let Some(ctr) = p3(c, p, "center")? {
        let half = box_dims(c, p)? * 0.5;
        (ctr - half, ctr + half)
    } else if let Some(k) = p3(c, p, "corner")? {
        (k, k + box_dims(c, p)?)
    } else {
        return Err(bad(c, "give {p1, p2, height?}, {center, length, width, height} or {corner, length, width, height}"));
    };
    let lo = Vec3::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z));
    let hi = Vec3::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z));
    Mesh3::box_solid(lo, hi).map_err(mesh_err(c))
}

pub(super) fn build_cylinder(p: &Value) -> Result<Mesh3> {
    let c = "cylinder";
    let ctr = center_of(c, p)?;
    let (r, h) = (size_or_diameter(c, p, "radius", "diameter")?, height_of(c, p)?);
    let n = count(c, p, "segments", DEFAULT_SEGMENTS, 3, MAX_SEGMENTS)?;
    let (z0, hh) = if h < 0.0 { (ctr.z + h, -h) } else { (ctr.z, h) };
    Mesh3::cylinder(Vec3::new(ctr.x, ctr.y, z0), r, hh, n).map_err(mesh_err(c))
}

/// A cone (apex up for a positive height, down for a negative one) with `n` base corners.
fn cone_mesh(cmd: &str, ctr: Vec3, r: f64, h: f64, n: usize) -> Result<Mesh3> {
    let mut m = Mesh3::cone(ctr, r, h.abs(), n).map_err(mesh_err(cmd))?;
    if h < 0.0 {
        mirror_z(&mut m, ctr.z);
    }
    Ok(m)
}

pub(super) fn build_cone(p: &Value) -> Result<Mesh3> {
    let c = "cone";
    let n = count(c, p, "segments", DEFAULT_SEGMENTS, 3, MAX_SEGMENTS)?;
    cone_mesh(c, center_of(c, p)?, size_or_diameter(c, p, "radius", "diameter")?, height_of(c, p)?, n)
}

pub(super) fn build_sphere(p: &Value) -> Result<Mesh3> {
    let c = "sphere";
    let n = count(c, p, "segments", DEFAULT_SEGMENTS, 3, MAX_SEGMENTS)?;
    let rings = count(c, p, "rings", DEFAULT_SEGMENTS / 2, 2, MAX_SEGMENTS / 2)?;
    Mesh3::sphere(center_of(c, p)?, size_or_diameter(c, p, "radius", "diameter")?, n, rings).map_err(mesh_err(c))
}

pub(super) fn build_torus(p: &Value) -> Result<Mesh3> {
    let c = "torus";
    let n = count(c, p, "segments", DEFAULT_SEGMENTS, 3, MAX_SEGMENTS)?;
    let tn = count(c, p, "tubeSegments", DEFAULT_SEGMENTS / 2, 3, MAX_SEGMENTS)?;
    let (r, t) = (size_or_diameter(c, p, "radius", "diameter")?, size_or_diameter(c, p, "tubeRadius", "tubeDiameter")?);
    if t >= r {
        return Err(bad(c, "`tubeRadius` must be smaller than `radius`"));
    }
    Mesh3::torus(center_of(c, p)?, r, t, n, tn).map_err(mesh_err(c))
}

pub(super) fn build_pyramid(p: &Value) -> Result<Mesh3> {
    let c = "pyramid";
    let n = count(c, p, "sides", 4, 3, 256)?;
    let r = match num(c, p, "side")? {
        Some(side) if side > 0.0 => side / (2.0 * (TAU / 2.0 / n as f64).sin()),
        Some(_) => return Err(bad(c, "`side` must be positive")),
        None => size_or_diameter(c, p, "radius", "diameter")?,
    };
    let (ctr, h) = (center_of(c, p)?, height_of(c, p)?);
    let rot = num(c, p, "rotation")?.unwrap_or(0.0).to_radians();
    let mut m = cone_mesh(c, ctr, r, h, n)?;
    // The cone's first base corner is at angle 0; turn so that the bottom edge is horizontal.
    let a = -TAU / 4.0 - TAU / 2.0 / n as f64 + rot;
    let (sin, cos) = a.sin_cos();
    for v in &mut m.vertices {
        let (dx, dy) = (v.x - ctr.x, v.y - ctr.y);
        v.x = ctr.x + dx * cos - dy * sin;
        v.y = ctr.y + dx * sin + dy * cos;
    }
    Ok(m)
}

/// The mesh is the display proxy; native xdata retains its exact boundary geometry.
fn emit_analytic(s: &mut Session, cmd: &str, mesh: &Mesh3, body: &cadcraft_geom::brep::Body) -> Result<Value> {
    let backup = s.doc()?.clone();
    let result = (|| {
        let mut r = emit_mesh(s, cmd, mesh, cmd)?;
        let name = r["group"].as_str().ok_or_else(|| bad(cmd, "missing solid group"))?;
        let hs = s.doc()?.groups.iter().find(|g| g.name == name).map(|g| g.members.clone()).ok_or_else(|| bad(cmd, "missing solid group"))?;
        cadcraft_doc::brep::attach(s.doc_mut()?, &hs, body).map_err(|e| bad(cmd, e))?;
        r["representation"] = json!("analyticBrep");
        Ok(r)
    })();
    if result.is_err() {
        *s.doc_mut()? = backup;
    }
    result
}

/// Emit profile solids as one undoable batch, attaching each exact source model to its mesh proxy.
fn emit_analytic_many(s: &mut Session, cmd: &str, solids: &[(Mesh3, cadcraft_geom::brep::Body, &str)]) -> Result<Value> {
    if solids.is_empty() {
        return Err(bad(cmd, "no solid to add"));
    }
    if solids.iter().map(|(mesh, _, _)| mesh.triangles.len()).sum::<usize>() > MAX_FACES {
        return Err(bad(cmd, format!("a command may add at most {MAX_FACES} faces")));
    }
    let backup = s.state()?.doc.clone();
    let result = (|| {
        let mut handles = Vec::new();
        let mut groups = Vec::new();
        let mut faces = 0usize;
        for (mesh, body, label) in solids {
            let result = emit_analytic(s, cmd, mesh, body)?;
            faces += result["faces"].as_u64().unwrap_or(0) as usize;
            if let Some(values) = result["handles"].as_array() {
                handles.extend(values.iter().cloned());
            }
            if let Some(group) = result["group"].as_str() {
                groups.push(group.to_owned());
            }
            let _ = label;
        }
        Ok(json!({
            "handles": handles,
            "faces": faces,
            "group": groups.first(),
            "groups": groups,
            "solids": solids.len(),
            "kind": cmd,
            "representation": "analyticBrep",
        }))
    })();
    if result.is_err()
        && let Ok(state) = s.state_mut()
    {
        state.doc = backup;
    }
    result
}
fn run_box(s: &mut Session, p: &Value) -> Result<Value> {
    let m = build_box(p)?;
    let lo =
        m.vertices.iter().fold(Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), |a, b| Vec3::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z)));
    let hi = m
        .vertices
        .iter()
        .fold(Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |a, b| Vec3::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z)));
    let body = cadcraft_geom::brep::Body::box_solid(lo, hi).map_err(mesh_err("box"))?;
    emit_analytic(s, "box", &m, &body)
}
fn run_cylinder(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "cylinder";
    let m = build_cylinder(p)?;
    let body = cadcraft_geom::brep::Body::cylinder(center_of(c, p)?, size_or_diameter(c, p, "radius", "diameter")?, height_of(c, p)?)
        .map_err(mesh_err(c))?;
    emit_analytic(s, c, &m, &body)
}
fn run_cone(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "cone";
    let m = build_cone(p)?;
    let body =
        cadcraft_geom::brep::Body::cone(center_of(c, p)?, size_or_diameter(c, p, "radius", "diameter")?, height_of(c, p)?).map_err(mesh_err(c))?;
    emit_analytic(s, c, &m, &body)
}
fn run_sphere(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "sphere";
    let m = build_sphere(p)?;
    let body = cadcraft_geom::brep::Body::sphere(center_of(c, p)?, size_or_diameter(c, p, "radius", "diameter")?).map_err(mesh_err(c))?;
    emit_analytic(s, c, &m, &body)
}
fn run_torus(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "torus";
    let m = build_torus(p)?;
    let body = cadcraft_geom::brep::Body::torus(
        center_of(c, p)?,
        size_or_diameter(c, p, "radius", "diameter")?,
        size_or_diameter(c, p, "tubeRadius", "tubeDiameter")?,
    )
    .map_err(mesh_err(c))?;
    emit_analytic(s, c, &m, &body)
}
fn run_pyramid(s: &mut Session, p: &Value) -> Result<Value> {
    emit_mesh(s, "pyramid", &build_pyramid(p)?, "pyramid")
}

// ---------------- profile commands ----------------

/// One profile to build a solid from.
struct Source {
    pts: Vec<Vec2>,
    z: f64,
    from: Option<Handle>,
}

/// The profiles of a profile command: `profile` points, else the closed polylines, circles and
/// ellipses among `handles` (or the selection; an unusable object is an error only when it was
/// named).
fn sources(s: &Session, p: &Value, cmd: &str, segments: usize) -> Result<Vec<Source>> {
    if p.get("profile").is_some_and(|v| !v.is_null()) {
        return Ok(vec![Source { pts: points2(cmd, p, "profile")?, z: 0.0, from: None }]);
    }
    let named = ["handles", "handle"].iter().any(|k| p.get(*k).is_some_and(|v| !v.is_null()));
    let hs = targets(s, p)?;
    if hs.len() > MAX_SOLIDS * 64 {
        return Err(bad(cmd, "too many objects"));
    }
    let d = s.doc()?;
    let mut out = Vec::new();
    for h in hs {
        match profile_of(cmd, d, h, segments) {
            Ok(pr) => out.push(Source { pts: pr.pts, z: pr.z, from: Some(h) }),
            Err(e) if named => return Err(e),
            Err(_) => {}
        }
    }
    if out.len() > MAX_SOLIDS {
        return Err(bad(cmd, format!("at most {MAX_SOLIDS} profiles at once")));
    }
    if out.is_empty() {
        return Err(bad(cmd, "give `profile` points, `handles` of closed profiles, or select closed polylines or circles first"));
    }
    Ok(out)
}

/// Counter-clockwise copy (the first point stays first).
fn ccw(mut pts: Vec<Vec2>) -> Vec<Vec2> {
    if signed_area(&pts) < 0.0
        && let Some(rest) = pts.get_mut(1..)
    {
        rest.reverse();
    }
    pts
}

/// Erase the profile objects when `deleteSource` is true.
fn finish_sources(s: &mut Session, p: &Value, src: &[Source]) -> Result<()> {
    if bool_or(p, "deleteSource", false) {
        let d = s.doc_mut()?;
        for h in src.iter().filter_map(|x| x.from) {
            d.remove_entity(h);
        }
    }
    Ok(())
}

fn run_extrude(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "extrude";
    let h = height_of(c, p)?;
    let segs = count(c, p, "segments", DEFAULT_SEGMENTS, 3, MAX_SEGMENTS)?;
    let base = num(c, p, "baseZ")?;
    let src = sources(s, p, c, segs)?;
    let mut solids = Vec::with_capacity(src.len());
    for x in &src {
        let z = base.unwrap_or(x.z);
        let (z0, hh) = if h < 0.0 { (z + h, -h) } else { (z, h) };
        let mesh = Mesh3::extrude(&x.pts, z0, hh).map_err(mesh_err(c))?;
        let body = if let Some(handle) = x.from
            && let Some(entity) = s.doc()?.entity(handle)
            && let EntityKind::Circle(circle) = &entity.kind
        {
            cadcraft_geom::brep::Body::cylinder(Vec3::new(circle.center.x, circle.center.y, z), circle.radius, h).map_err(mesh_err(c))?
        } else {
            cadcraft_geom::brep::Body::extrude_profile(&x.pts, z, h).map_err(mesh_err(c))?
        };
        solids.push((mesh, body, "extrude"));
    }
    let r = emit_analytic_many(s, c, &solids)?;
    finish_sources(s, p, &src)?;
    Ok(r)
}

fn run_revolve(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "revolve";
    let n = count(c, p, "segments", DEFAULT_SEGMENTS, 3, MAX_SEGMENTS)?;
    let ctr = center_of(c, p)?;
    let raw = p.get("profile").is_some_and(|v| !v.is_null());
    let axis_value = p.get("axis").filter(|v| !v.is_null()).or_else(|| p.get("revolveAxis").filter(|v| !v.is_null()));
    if p.get("axis").is_some_and(|v| !v.is_null()) && p.get("revolveAxis").is_some_and(|v| !v.is_null()) {
        return Err(bad(c, "give either `axis` or `revolveAxis`, not both"));
    }
    if axis_value.is_some() && num(c, p, "axisX")?.is_some() {
        return Err(bad(c, "`axisX` cannot be combined with `axis`"));
    }
    let (axis_origin, axis_direction, explicit_axis) = if let Some(v) = axis_value {
        if p.get("center").is_some_and(|v| !v.is_null()) {
            return Err(bad(c, "`center` cannot be combined with an explicit `axis` line"));
        }
        let line = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| bad(c, "`axis` must be [[x,y,z],[x,y,z]]"))?;
        let a =
            point3(line.first().ok_or_else(|| bad(c, "`axis` needs two points"))?).ok_or_else(|| bad(c, "`axis` must contain finite 3D points"))?;
        let b =
            point3(line.get(1).ok_or_else(|| bad(c, "`axis` needs two points"))?).ok_or_else(|| bad(c, "`axis` must contain finite 3D points"))?;
        let direction = b - a;
        if direction.len() <= EPS {
            return Err(bad(c, "`axis` points must be distinct"));
        }
        (a, direction, true)
    } else {
        (ctr, Vec3::Z, false)
    };
    let axis_x = if raw || explicit_axis { 0.0 } else { num(c, p, "axisX")?.unwrap_or(0.0) };
    let start = num(c, p, "startAngle")?.unwrap_or(0.0);
    let angle = num(c, p, "angle")?.unwrap_or(360.0);
    if start.abs() > 360_000.0 || angle.abs() > 360.0 || angle.abs() <= 1e-8 {
        return Err(bad(c, "`startAngle` must be within ±360000° and `angle` must be nonzero and within ±360°"));
    }
    let src = sources(s, p, c, n)?;
    let mut solids = Vec::with_capacity(src.len());
    for x in &src {
        let (pts, radial_hint) = if explicit_axis && !raw {
            let (pts, radial) = profile_about_axis(c, &x.pts, x.z, axis_origin, axis_direction)?;
            (pts, Some(radial))
        } else {
            // Preserve the long-standing 2D convention: X is radius, Y is axial distance.
            (
                x.pts
                    .iter()
                    .map(|q| {
                        let r = q.x - axis_x;
                        Vec2::new(if r.abs() <= 1e-9 { 0.0 } else { r }, q.y)
                    })
                    .collect(),
                None,
            )
        };
        let m = Mesh3::revolve_axis_oriented(&pts, axis_origin, axis_direction, radial_hint, n, start.to_radians(), angle.to_radians())
            .map_err(mesh_err(c))?;
        let exact_radial = radial_hint.unwrap_or_else(|| {
            let axis = axis_direction.normalized();
            if axis.x.abs() < 0.99 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) }
        });
        let circle = if !raw {
            x.from
                .and_then(|handle| s.doc().ok()?.entity(handle))
                .and_then(|entity| if let EntityKind::Circle(circle) = &entity.kind { Some(circle.clone()) } else { None })
        } else {
            None
        };
        let body = if let Some(circle) = circle {
            let center = if explicit_axis {
                let axis = axis_direction.normalized();
                let delta = circle.center - axis_origin;
                let radial_center = delta - axis * delta.dot(axis);
                Vec2::new(radial_center.dot(exact_radial), delta.dot(axis))
            } else {
                Vec2::new(circle.center.x - axis_x, circle.center.y)
            };
            cadcraft_geom::brep::Body::revolve_circle(
                center,
                circle.radius,
                axis_origin,
                axis_direction,
                exact_radial,
                start.to_radians(),
                angle.to_radians(),
            )
        } else {
            cadcraft_geom::brep::Body::revolve_profile(&pts, axis_origin, axis_direction, exact_radial, start.to_radians(), angle.to_radians())
        }
        .map_err(mesh_err(c))?;
        solids.push((m, body, "revolve"));
    }
    let r = emit_analytic_many(s, c, &solids)?;
    finish_sources(s, p, &src)?;
    Ok(r)
}

fn sweep_path(s: &Session, p: &Value) -> Result<Vec<Vec3>> {
    let c = "sweep";
    if let Some(a) = p.get("path").filter(|v| !v.is_null()) {
        let a = a.as_array().ok_or_else(|| bad(c, "`path` must be a list of [x, y, z] points"))?;
        if a.len() > MAX_PROFILE {
            return Err(bad(c, format!("`path` holds at most {MAX_PROFILE} points")));
        }
        return a.iter().map(|v| point3(v).ok_or_else(|| bad(c, format!("`path`: {v} is not an [x, y, z] point")))).collect();
    }
    let h = match p.get("pathHandle") {
        Some(Value::String(t)) => Handle::parse_hex(t),
        Some(v) => v.as_u64().map(Handle),
        None => return Err(bad(c, "`path` or `pathHandle` is required")),
    };
    path_of(c, s.doc()?, h.ok_or_else(|| bad(c, "`pathHandle` is not an object handle"))?)
}

fn run_sweep(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "sweep";
    let segs = count(c, p, "segments", DEFAULT_SEGMENTS, 3, MAX_SEGMENTS)?;
    let path = sweep_path(s, p)?;
    let origin = match p.get("profileOrigin") {
        None | Some(Value::Null) => None,
        Some(v) => Some(point_value(v).ok_or_else(|| bad(c, "`profileOrigin` must be [x, y]"))?),
    };
    let raw = p.get("profile").is_some_and(|v| !v.is_null());
    let twist = num(c, p, "twist")?.unwrap_or(0.0);
    let scale_start = num(c, p, "scaleStart")?.unwrap_or(1.0);
    let scale = num(c, p, "scale")?;
    let scale_end = num(c, p, "scaleEnd")?.or(scale).unwrap_or(1.0);
    if twist.abs() > 360_000.0 {
        return Err(bad(c, "`twist` must be within ±360000°"));
    }
    let src = sources(s, p, c, segs)?;
    let mut solids = Vec::with_capacity(src.len());
    for x in &src {
        // A drawn profile is centred (bounding box) so that its centre follows the path.
        let o = match (origin, raw) {
            (Some(o), _) => o,
            (None, true) => Vec2::ZERO,
            (None, false) => bbox_centre(&x.pts),
        };
        let pts = ccw(x.pts.iter().map(|q| *q - o).collect());
        let mesh = Mesh3::sweep_with_options(&pts, &path, twist.to_radians(), scale_start, scale_end).map_err(mesh_err(c))?;
        let body = cadcraft_geom::brep::Body::sweep_profile(&pts, &path, twist.to_radians(), scale_start, scale_end).map_err(mesh_err(c))?;
        solids.push((mesh, body, "sweep"));
    }
    let r = emit_analytic_many(s, c, &solids)?;
    finish_sources(s, p, &src)?;
    Ok(r)
}

fn bbox_centre(pts: &[Vec2]) -> Vec2 {
    let Some(f) = pts.first().copied() else { return Vec2::ZERO };
    let (lo, hi) = pts.iter().fold((f, f), |(lo, hi), q| (lo.min(*q), hi.max(*q)));
    lo.mid(hi)
}

/// Sections as given (`sections`), each made counter-clockwise.
fn given_sections(p: &Value) -> Result<Vec<Vec<Vec3>>> {
    let c = "loft";
    let a = p.get("sections").and_then(Value::as_array).ok_or_else(|| bad(c, "`sections` must be a list of sections"))?;
    if a.len() > MAX_SECTIONS {
        return Err(bad(c, format!("a loft has at most {MAX_SECTIONS} sections")));
    }
    let mut out = Vec::with_capacity(a.len());
    for sec in a {
        let pts = sec.as_array().filter(|x| x.len() <= MAX_PROFILE).ok_or_else(|| bad(c, "a section must be a list of at most 2048 points"))?;
        let pts: Vec<Vec3> = pts.iter().map(|v| point3(v).ok_or_else(|| bad(c, format!("{v} is not an [x, y, z] point")))).collect::<Result<_>>()?;
        out.push(pts);
    }
    Ok(out)
}

/// Sections from profile objects: stacked `spacing` apart (selection order) or at their own
/// elevations, lowest first.
fn object_sections(p: &Value, src: &[Source]) -> Result<Vec<Vec<Vec3>>> {
    let c = "loft";
    let spacing = num(c, p, "spacing")?;
    let base = num(c, p, "baseZ")?.unwrap_or(0.0);
    let mut secs: Vec<(f64, Vec<Vec3>)> = Vec::with_capacity(src.len());
    for (i, x) in src.iter().enumerate() {
        let z = match spacing {
            Some(sp) => base + sp * i as f64,
            None => x.z,
        };
        secs.push((z, x.pts.iter().map(|q| q.to3(z)).collect()));
    }
    if spacing.is_none() {
        secs.sort_by(|a, b| a.0.total_cmp(&b.0));
        if secs.windows(2).any(|w| w.first().zip(w.get(1)).is_some_and(|(a, b)| (b.0 - a.0).abs() <= EPS)) {
            return Err(bad(c, "profiles lie at the same elevation; pass `spacing` to stack them or give them different elevations"));
        }
    }
    Ok(secs.into_iter().map(|(_, v)| v).collect())
}

fn run_loft(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "loft";
    let segs = count(c, p, "segments", DEFAULT_SEGMENTS, 3, MAX_SEGMENTS)?;
    let (sections, src) = if p.get("sections").is_some_and(|v| !v.is_null()) {
        (given_sections(p)?, Vec::new())
    } else {
        let src = sources(s, p, c, segs)?;
        (object_sections(p, &src)?, src)
    };
    let sections = orient_sections(c, sections)?;
    let m = Mesh3::loft_resampled(&sections).map_err(mesh_err(c))?;
    let body = cadcraft_geom::brep::Body::loft_sections(&sections).map_err(mesh_err(c))?;
    let r = emit_analytic_many(s, c, &[(m, body, "loft")])?;
    finish_sources(s, p, &src)?;
    Ok(r)
}

/// Orient planar sections consistently in 3D, preserving each first vertex as the correspondence origin.
fn orient_sections(cmd: &str, mut sections: Vec<Vec<Vec3>>) -> Result<Vec<Vec<Vec3>>> {
    if sections.len() < 2 {
        return Err(bad(cmd, "loft needs at least two sections"));
    }
    let normal_of = |section: &[Vec3]| -> Vec3 {
        let Some(origin) = section.first().copied() else { return Vec3::ZERO };
        (0..section.len())
            .fold(Vec3::ZERO, |n, i| {
                let Some(a) = section.get(i).copied() else { return n };
                let Some(b) = section.get((i + 1) % section.len()).copied() else { return n };
                n + (a - origin).cross(b - origin)
            })
            .normalized()
    };
    let center_of = |section: &[Vec3]| -> Vec3 { section.iter().copied().fold(Vec3::ZERO, |a, b| a + b) * (1.0 / section.len().max(1) as f64) };
    let mut normal = normal_of(sections.first().map(Vec::as_slice).unwrap_or(&[]));
    if normal.len() <= EPS {
        return Err(bad(cmd, "degenerate loft section"));
    }
    let travel = center_of(sections.get(1).map(Vec::as_slice).unwrap_or(&[])) - center_of(sections.first().map(Vec::as_slice).unwrap_or(&[]));
    if travel.len() <= EPS || travel.dot(normal).abs() <= EPS {
        return Err(bad(cmd, "loft sections need separation along their section plane"));
    }
    if travel.dot(normal) < 0.0 {
        if let Some(first) = sections.first_mut()
            && let Some(tail) = first.get_mut(1..)
        {
            tail.reverse();
        }
        normal = -normal;
    }
    let mut prior = normal;
    for section in sections.iter_mut().skip(1) {
        let mut current = normal_of(section);
        if current.len() <= EPS {
            return Err(bad(cmd, "degenerate loft section"));
        }
        let alignment = current.dot(prior);
        if alignment.abs() <= EPS {
            return Err(bad(cmd, "adjacent loft section frames are perpendicular"));
        }
        if alignment < 0.0 {
            if let Some(tail) = section.get_mut(1..) {
                tail.reverse();
            }
            current = -current;
        }
        prior = current;
    }
    Ok(sections)
}

// ---------------- booleans ----------------

/// A solid boolean (UNION, SUBTRACT, INTERSECT on 3D solids).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoolOp {
    Union,
    Subtract,
    Intersect,
}

/// Whether `hs` are 3D solids: `Ok(true)` when every handle is a face of a solid, `Ok(false)`
/// when none is (the 2D region commands then apply), an error for a mix.
pub(crate) fn classify_solids(d: &Drawing, hs: &[Handle]) -> Result<bool> {
    let in_solid: std::collections::HashSet<Handle> =
        d.groups.iter().filter(|g| g.description == SOLID_GROUP).flat_map(|g| g.members.iter().copied()).collect();
    let n = hs.iter().filter(|h| in_solid.contains(h)).count();
    if n == 0 {
        Ok(false)
    } else if n == hs.len() {
        Ok(true)
    } else {
        Err(EngineError::Other("Select either 3D solids or regions, not a mix of both.".into()))
    }
}

/// Combine solids: UNION/INTERSECT fold over all of `a`; SUBTRACT removes every solid of `b`
/// from the union of `a`. The sources (faces and groups) are replaced by the one result solid in
/// the command's single undo step; an empty result is an error and leaves the drawing as it was.
pub(crate) fn solid_boolean(s: &mut Session, cmd: &str, op: BoolOp, a: &[Handle], b: &[Handle]) -> Result<Value> {
    let d = s.doc()?;
    let (ga, gb) = (solid_groups(d, a), solid_groups(d, b));
    if ga.len() + gb.len() > MAX_SOLIDS {
        return Err(bad(cmd, format!("at most {MAX_SOLIDS} solids at once")));
    }
    // A supplied face stands for its entire solid, including faces not directly picked.
    // All operands are consumed on success, so even a locked cutter must be refused.
    for h in ga.iter().chain(&gb).flat_map(|(_, members)| members) {
        if let Some(e) = d.entity(*h)
            && d.layer(&e.common.layer).is_some_and(|layer| layer.locked)
        {
            return Err(bad(cmd, format!("solid contains an object on locked layer `{}`", e.common.layer)));
        }
    }
    if ga.iter().any(|(n, _)| gb.iter().any(|(m, _)| m == n)) {
        return Err(bad(cmd, "a solid can't be combined with itself"));
    }
    match op {
        BoolOp::Subtract if ga.is_empty() || gb.is_empty() => return Err(bad(cmd, "select the solids to subtract from and the solids to remove")),
        BoolOp::Union | BoolOp::Intersect if ga.len() < 2 => return Err(bad(cmd, "select at least two solids")),
        _ => {}
    }
    let fail = |e: cadcraft_geom::MeshError| bad(cmd, e.to_string());
    let mut meshes = ga.iter().map(|(_, m)| mesh_of(d, m));
    let mut acc = meshes.next().ok_or_else(|| bad(cmd, "select 3D solids"))??;
    for m in meshes {
        let m = m?;
        acc = match op {
            BoolOp::Intersect => acc.intersect(&m),
            BoolOp::Union | BoolOp::Subtract => acc.union(&m),
        }
        .map_err(fail)?;
        if acc.triangles.is_empty() {
            return Err(bad(cmd, "the solids do not overlap"));
        }
    }
    if op == BoolOp::Subtract {
        for (_, m) in &gb {
            acc = acc.subtract(&mesh_of(d, m)?).map_err(fail)?;
            if acc.triangles.is_empty() {
                return Err(bad(cmd, "the result would be empty: nothing remains of the solid"));
            }
        }
    }
    let names: Vec<&str> = ga.iter().chain(&gb).map(|(n, _)| n.as_str()).collect();
    let sources: Vec<Handle> = ga.iter().chain(&gb).flat_map(|(_, m)| m.iter().copied()).collect();
    let names: Vec<String> = names.into_iter().map(String::from).collect();
    let backup = s.state()?.doc.clone();
    let result = replace_with(s, cmd, &names, &sources, &acc);
    if result.is_err()
        && let Ok(st) = s.state_mut()
    {
        st.doc = backup;
    }
    result
}

fn replace_with(s: &mut Session, cmd: &str, names: &[String], sources: &[Handle], mesh: &Mesh3) -> Result<Value> {
    let d = s.doc_mut()?;
    for h in sources {
        d.remove_entity(*h);
    }
    d.groups.retain(|g| !names.contains(&g.name));
    emit_mesh(s, cmd, mesh, cmd)
}

#[cfg(test)]
mod tests;
