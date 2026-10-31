//! Triangle mesh exchange. OBJ groups become layers; STL has no grouping.
use crate::{IoError, Result};
use cadcraft_doc::{Common, Drawing, EntityKind, Face3d, Space};
use cadcraft_geom::Vec3;

const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_VERTICES: usize = 1_000_000;
const MAX_FACES: usize = 1_000_000;
fn error(message: &str) -> IoError {
    IoError::Format(format!("mesh: {message}"))
}
fn number(s: &str) -> Result<f64> {
    let n: f64 = s.parse().map_err(|_| error("invalid number"))?;
    if !n.is_finite() {
        return Err(error("non-finite coordinate"));
    }
    Ok(n)
}
fn add(d: &mut Drawing, layer: &str, a: Vec3, b: Vec3, c: Vec3) -> Result<()> {
    let common = Common { layer: layer.to_string(), ..Common::default() };
    d.add(&Space::Model, common, EntityKind::Face3d(Face3d { corners: [a, b, c, c], hidden_edges: 0 }))
        .map_err(|e| IoError::Format(e.to_string()))?;
    Ok(())
}
fn point<'a>(mut parts: impl Iterator<Item = &'a str>) -> Result<Vec3> {
    Ok(Vec3::new(
        number(parts.next().ok_or_else(|| error("missing X"))?)?,
        number(parts.next().ok_or_else(|| error("missing Y"))?)?,
        number(parts.next().ok_or_else(|| error("missing Z"))?)?,
    ))
}

// Fan triangulation is only reliable for planar, convex quadrilaterals.
fn convex_quad(p: &[Vec3]) -> bool {
    let scale = p.iter().flat_map(|v| [v.x.abs(), v.y.abs(), v.z.abs()]).fold(1.0_f64, f64::max);
    let points: Vec<[f64; 3]> = p.iter().map(|v| [v.x / scale, v.y / scale, v.z / scale]).collect();
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let n = cross(sub(points[1], points[0]), sub(points[2], points[1]));
    let norm = dot(n, n).sqrt();
    if norm == 0.0 || dot(n, sub(points[3], points[0])).abs() > norm * 1e-9 {
        return false;
    }
    (0..4).all(|i| dot(n, cross(sub(points[(i + 1) % 4], points[i]), sub(points[(i + 2) % 4], points[(i + 1) % 4]))) > 0.0)
}

pub fn read_obj(bytes: &[u8]) -> Result<Drawing> {
    if bytes.len() > MAX_BYTES {
        return Err(error("file too large"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| error("OBJ must be UTF-8"))?;
    let mut d = Drawing::default();
    let mut vertices = Vec::new();
    let mut layer = String::from("0");
    let mut textures = 0usize;
    let mut normals = 0usize;
    let mut ignored_attributes = false;
    let mut count = 0usize;
    let mut groups = std::collections::HashSet::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut fields = line.split_whitespace();
        match fields.next() {
            None => {}
            Some("v") => {
                if vertices.len() >= MAX_VERTICES {
                    return Err(error("too many vertices"));
                }
                let v = point(&mut fields)?;
                if fields.next().is_some() {
                    return Err(error("OBJ vertex weights or colors are unsupported"));
                }
                vertices.push(v);
            }
            Some("g" | "o") => {
                let name = fields.next().unwrap_or("0");
                if name.len() > 255 || fields.next().is_some() {
                    return Err(error("invalid or multiple OBJ group names"));
                }
                groups.insert(name.to_string());
                if groups.len() > 256 {
                    return Err(error("too many OBJ groups"));
                }
                layer = name.to_string();
            }
            Some("f") => {
                let mut face = Vec::new();
                for field in fields {
                    if face.len() >= 4 {
                        return Err(error("OBJ polygons with more than four vertices are unsupported"));
                    }
                    if field.ends_with('/') {
                        return Err(error("incomplete OBJ face reference"));
                    }
                    let mut indices = field.split('/');
                    let index: i64 =
                        indices.next().ok_or_else(|| error("missing vertex index"))?.parse().map_err(|_| error("invalid OBJ vertex index"))?;
                    for (component, size) in [(indices.next(), textures), (indices.next(), normals)] {
                        if let Some(component) = component
                            && !component.is_empty()
                        {
                            let n: i64 = component.parse().map_err(|_| error("invalid OBJ attribute index"))?;
                            let size = i64::try_from(size).map_err(|_| error("too many OBJ attributes"))?;
                            if n == 0 || n > size || n < -size {
                                return Err(error("OBJ attribute index out of range"));
                            }
                        }
                    }
                    if indices.next().is_some() {
                        return Err(error("invalid OBJ face reference"));
                    }
                    let len = i64::try_from(vertices.len()).map_err(|_| error("too many vertices"))?;
                    let idx = if index > 0 {
                        index - 1
                    } else if index < 0 {
                        len + index
                    } else {
                        return Err(error("OBJ index cannot be zero"));
                    };
                    let idx = usize::try_from(idx).map_err(|_| error("OBJ vertex index out of range"))?;
                    face.push(*vertices.get(idx).ok_or_else(|| error("OBJ vertex index out of range"))?);
                }
                if face.len() != 3 && face.len() != 4 {
                    return Err(error("OBJ face needs three or four vertices"));
                }
                if face.len() == 4 && !convex_quad(&face) {
                    return Err(error("OBJ quadrilateral must be planar and convex"));
                }
                count = count.checked_add(face.len() - 2).ok_or_else(|| error("too many faces"))?;
                if count > MAX_FACES {
                    return Err(error("too many faces"));
                }
                add(&mut d, &layer, face[0], face[1], face[2])?;
                if face.len() == 4 {
                    add(&mut d, &layer, face[0], face[2], face[3])?;
                }
            }
            Some("vt") => {
                let mut count = 0;
                for v in fields {
                    count += 1;
                    if count > 3 {
                        return Err(error("invalid OBJ texture coordinate"));
                    }
                    number(v)?;
                }
                if count == 0 {
                    return Err(error("invalid OBJ texture coordinate"));
                }
                textures += 1;
                if textures > MAX_VERTICES {
                    return Err(error("too many OBJ attributes"));
                }
                ignored_attributes = true;
            }
            Some("vn") => {
                point(&mut fields)?;
                if fields.next().is_some() {
                    return Err(error("invalid OBJ normal"));
                }
                normals += 1;
                if normals > MAX_VERTICES {
                    return Err(error("too many OBJ attributes"));
                }
                ignored_attributes = true;
            }
            Some("s" | "usemtl" | "mtllib") => {
                ignored_attributes = true;
            }
            Some(_) => return Err(error("unsupported OBJ statement")),
        }
    }
    if count == 0 {
        return Err(error("OBJ contains no faces"));
    }
    if ignored_attributes {
        d.read_notes.push("OBJ material, texture and normal attributes are not retained".into());
    }
    Ok(d)
}

pub fn read_stl(bytes: &[u8]) -> Result<Drawing> {
    if bytes.len() > MAX_BYTES {
        return Err(error("file too large"));
    }
    if let Some(raw) = bytes.get(80..84) {
        let count = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
        if count <= MAX_FACES && count.checked_mul(50).and_then(|n| n.checked_add(84)) == Some(bytes.len()) {
            let mut d = Drawing::default();
            let mut attributes = false;
            for facet in bytes[84..].chunks_exact(50) {
                for raw in facet[..12].chunks_exact(4) {
                    if !f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]).is_finite() {
                        return Err(error("non-finite STL normal"));
                    }
                }
                // Some exporters keep a facet colour in the two attribute bytes; the geometry is
                // still valid, so the bytes are skipped and reported once.
                attributes |= facet[48] != 0 || facet[49] != 0;
                let mut coords = [0.0; 9];
                for (i, dest) in coords.iter_mut().enumerate() {
                    let p = 12 + i * 4;
                    let raw = facet.get(p..p + 4).ok_or_else(|| error("truncated STL facet"))?;
                    *dest = f64::from(f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]));
                    if !dest.is_finite() {
                        return Err(error("non-finite coordinate"));
                    }
                }
                add(
                    &mut d,
                    "0",
                    Vec3::new(coords[0], coords[1], coords[2]),
                    Vec3::new(coords[3], coords[4], coords[5]),
                    Vec3::new(coords[6], coords[7], coords[8]),
                )?;
            }
            if count == 0 {
                return Err(error("STL contains no facets"));
            }
            if attributes {
                d.read_notes.push("STL facet attribute bytes (such as colours) are not retained".into());
            }
            return Ok(d);
        }
    }
    let text = std::str::from_utf8(bytes).map_err(|_| error("invalid or truncated binary STL"))?;
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    if !lines.next().is_some_and(|l| l == "solid" || l.starts_with("solid ")) {
        return Err(error("invalid STL header"));
    }
    let mut d = Drawing::default();
    let mut count = 0usize;
    loop {
        let line = lines.next().ok_or_else(|| error("missing endsolid"))?;
        if line == "endsolid" || line.starts_with("endsolid ") {
            if lines.next().is_some() || count == 0 {
                return Err(error("empty STL or trailing content"));
            }
            return Ok(d);
        }
        let mut f = line.split_whitespace();
        if f.next() != Some("facet") || f.next() != Some("normal") {
            return Err(error("expected facet normal"));
        }
        point(&mut f)?;
        if f.next().is_some() || lines.next() != Some("outer loop") {
            return Err(error("invalid STL facet"));
        }
        let mut points = Vec::new();
        for _ in 0..3 {
            let mut v = lines.next().ok_or_else(|| error("truncated STL facet"))?.split_whitespace();
            if v.next() != Some("vertex") {
                return Err(error("expected STL vertex"));
            }
            points.push(point(&mut v)?);
            if v.next().is_some() {
                return Err(error("invalid STL vertex"));
            }
        }
        if lines.next() != Some("endloop") || lines.next() != Some("endfacet") {
            return Err(error("incomplete STL facet"));
        }
        count += 1;
        if count > MAX_FACES {
            return Err(error("too many faces"));
        }
        add(&mut d, "0", points[0], points[1], points[2])?;
    }
}

pub(crate) fn face3d_triangles(d: &Drawing) -> Result<Vec<(String, cadcraft_color::Color, [Vec3; 3])>> {
    let mut out = Vec::new();
    for e in d.model.iter() {
        let EntityKind::Face3d(face) = &e.kind else {
            return Err(error("mesh export only supports 3D faces"));
        };
        let [a, b, c, fourth] = face.corners;
        if [a, b, c, fourth].iter().any(|v| !v.x.is_finite() || !v.y.is_finite() || !v.z.is_finite()) {
            return Err(error("non-finite coordinate"));
        }
        if out.len() >= MAX_FACES {
            return Err(error("too many faces"));
        }
        out.push((e.common.layer.clone(), e.common.color, [a, b, c]));
        if fourth != c {
            if !convex_quad(&[a, b, c, fourth]) {
                return Err(error("3D face quadrilateral must be planar and convex"));
            }
            out.push((e.common.layer.clone(), e.common.color, [a, c, fourth]));
        }
    }
    if out.is_empty() {
        return Err(error("drawing contains no 3D faces"));
    }
    if out.len() > MAX_FACES {
        return Err(error("too many faces"));
    }
    Ok(out)
}
fn triangles(d: &Drawing) -> Result<Vec<(String, [Vec3; 3])>> {
    Ok(face3d_triangles(d)?.into_iter().map(|(layer, _, triangle)| (layer, triangle)).collect())
}

pub(crate) fn add_face3d(d: &mut Drawing, layer: &str, color: cadcraft_color::Color, triangle: [Vec3; 3]) -> Result<()> {
    if layer.is_empty() || layer.len() > 255 || layer.chars().any(char::is_control) || triangle.iter().any(|p| !p.is_finite()) {
        return Err(error("invalid imported mesh face"));
    }
    if d.layer(layer).is_none() {
        if d.layers.len() >= 256 {
            return Err(error("too many mesh layers"));
        }
        d.layers.push(cadcraft_doc::Layer { name: layer.to_string(), ..Default::default() });
    }
    let [a, b, c] = triangle;
    d.add(
        &Space::Model,
        Common { layer: layer.to_string(), color, ..Default::default() },
        EntityKind::Face3d(Face3d { corners: [a, b, c, c], hidden_edges: 0 }),
    )
    .map_err(|e| IoError::Format(e.to_string()))?;
    Ok(())
}

pub fn write_obj(d: &Drawing) -> Result<Vec<u8>> {
    use std::fmt::Write;
    let mut out = String::new();
    let mut index = 1usize;
    let mut groups = std::collections::HashSet::new();
    let triangles = triangles(d)?;
    if triangles.len() > MAX_VERTICES / 3 {
        return Err(error("OBJ output exceeds vertex limit"));
    }
    for (layer, points) in triangles {
        let name: String = layer.chars().map(|c| if c.is_whitespace() || c.is_control() || c == '#' { '_' } else { c }).collect();
        if name.is_empty() || name.len() > 255 {
            return Err(error("invalid OBJ group name"));
        }
        groups.insert(name.clone());
        if groups.len() > 256 {
            return Err(error("too many OBJ groups"));
        }
        writeln!(out, "g {name}").map_err(|_| error("could not write OBJ"))?;
        for p in points {
            writeln!(out, "v {} {} {}", p.x, p.y, p.z).map_err(|_| error("could not write OBJ"))?;
        }
        writeln!(out, "f {} {} {}", index, index + 1, index + 2).map_err(|_| error("could not write OBJ"))?;
        if out.len() > MAX_BYTES {
            return Err(error("mesh output too large"));
        }
        index += 3;
    }
    Ok(out.into_bytes())
}
pub fn write_stl(d: &Drawing) -> Result<Vec<u8>> {
    use std::fmt::Write;
    let mut out = String::from("solid cadcraft\n");
    for (_, points) in triangles(d)? {
        let [a, b, c] = points;
        let scale = [a.x, a.y, a.z, b.x, b.y, b.z, c.x, c.y, c.z].iter().fold(1.0_f64, |m, x| m.max(x.abs()));
        let u = [(b.x / scale) - (a.x / scale), (b.y / scale) - (a.y / scale), (b.z / scale) - (a.z / scale)];
        let v = [(c.x / scale) - (a.x / scale), (c.y / scale) - (a.y / scale), (c.z / scale) - (a.z / scale)];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if length == 0.0 {
            return Err(error("degenerate STL triangle"));
        }
        writeln!(out, "facet normal {} {} {}\nouter loop", n[0] / length, n[1] / length, n[2] / length).map_err(|_| error("could not write STL"))?;
        for p in points {
            writeln!(out, "vertex {} {} {}", p.x, p.y, p.z).map_err(|_| error("could not write STL"))?;
        }
        out.push_str("endloop\nendfacet\n");
        if out.len() > MAX_BYTES {
            return Err(error("mesh output too large"));
        }
    }
    out.push_str("endsolid cadcraft\n");
    Ok(out.into_bytes())
}
