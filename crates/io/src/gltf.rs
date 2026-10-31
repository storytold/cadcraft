//! glTF 2.0 / GLB triangle-mesh exchange.
//!
//! CADCraft currently stores imported 3D geometry as `Face3d` facets. This module deliberately
//! handles that mesh representation only; it does not infer or preserve analytic surfaces.

use std::collections::HashMap;

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::Drawing;
use cadcraft_geom::Vec3;
use serde_json::{Value, json};

use crate::{IoError, Result};

const MAX_BYTES: usize = 64 << 20;
const MAX_VERTICES: usize = 1_000_000;
const MAX_TRIANGLES: usize = 1_000_000;
const MAX_NODES: usize = 100_000;
const GLB_MAGIC: u32 = 0x4654_6c67;
const JSON_CHUNK: u32 = 0x4e4f_534a;
const BIN_CHUNK: u32 = 0x004e_4942;

fn error(s: &str) -> IoError {
    IoError::Format(format!("glTF: {s}"))
}

fn bounded_name(s: &str) -> String {
    let mut out: String = s.chars().filter(|c| !c.is_control()).take(255).collect();
    if out.is_empty() {
        out.push('0');
    }
    out
}

fn finite(v: f64) -> bool {
    v.is_finite() && v.abs() <= 1.0e12
}

fn val_usize(v: &Value, key: &str) -> Result<usize> {
    v.get(key).and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).ok_or_else(|| error("invalid integer field"))
}

fn opt_usize(v: &Value, key: &str, default: usize) -> Result<usize> {
    if v.get(key).is_none() { Ok(default) } else { val_usize(v, key) }
}

fn b64_encode(data: &[u8]) -> String {
    const TAB: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let a = chunk.first().copied().unwrap_or(0);
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        let n = (u32::from(a) << 16) | (u32::from(b) << 8) | u32::from(c);
        for i in 0..4 {
            out.push(if i > chunk.len() { '=' } else { char::from(TAB[((n >> (18 - 6 * i)) & 63) as usize]) });
        }
    }
    out
}

fn b64_decode(s: &str) -> Result<Vec<u8>> {
    if s.len() > MAX_BYTES.saturating_mul(4) / 3 + 8 {
        return Err(error("embedded buffer too large"));
    }
    let bytes: Vec<u8> = s.bytes().filter(|b| !matches!(b, b'\r' | b'\n' | b' ' | b'\t')).collect();
    if !bytes.len().is_multiple_of(4) {
        return Err(error("invalid base64 length"));
    }
    let pad = bytes.iter().rev().take_while(|b| **b == b'=').count();
    if pad > 2 || bytes[..bytes.len().saturating_sub(pad)].contains(&b'=') {
        return Err(error("invalid base64 padding"));
    }
    let mut out = Vec::with_capacity(bytes.len().saturating_div(4).saturating_mul(3).saturating_sub(pad));
    let decode = |b: u8| -> Result<u8> {
        let value = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(error("invalid base64 data URI")),
        };
        Ok(value)
    };
    for (i, chunk) in bytes.chunks_exact(4).enumerate() {
        let last = i + 1 == bytes.len() / 4;
        let a = decode(chunk[0])?;
        let b = decode(chunk[1])?;
        let c = if chunk[2] == b'=' { 0 } else { decode(chunk[2])? };
        let d = if chunk[3] == b'=' { 0 } else { decode(chunk[3])? };
        if (!last && chunk.contains(&b'='))
            || (chunk[2] == b'=' && chunk[3] != b'=')
            || (chunk[2] == b'=' && b & 0x0f != 0)
            || (chunk[3] == b'=' && c & 0x03 != 0)
        {
            return Err(error("invalid base64 padding"));
        }
        out.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' {
            out.push((b << 4) | (c >> 2));
        }
        if chunk[3] != b'=' {
            out.push((c << 6) | d);
        }
        if out.len() > MAX_BYTES {
            return Err(error("embedded buffer too large"));
        }
    }
    Ok(out)
}

fn unpack(bytes: &[u8]) -> Result<(Value, Vec<u8>)> {
    if bytes.len() > MAX_BYTES {
        return Err(error("file too large"));
    }
    let (json_bytes, bin) = if bytes.get(..4) == Some(b"glTF") {
        if bytes.len() < 12 || u32::from_le_bytes(bytes[4..8].try_into().map_err(|_| error("truncated GLB header"))?) != 2 {
            return Err(error("invalid GLB header or version"));
        }
        let total = u32::from_le_bytes(bytes[8..12].try_into().map_err(|_| error("truncated GLB header"))?) as usize;
        if total != bytes.len() {
            return Err(error("GLB length does not match file"));
        }
        let mut pos = 12usize;
        let mut json_part = None;
        let mut bin_part = None;
        while pos < bytes.len() {
            let h = bytes.get(pos..pos + 8).ok_or_else(|| error("truncated GLB chunk header"))?;
            let len = u32::from_le_bytes(h[0..4].try_into().map_err(|_| error("invalid GLB chunk"))?) as usize;
            let kind = u32::from_le_bytes(h[4..8].try_into().map_err(|_| error("invalid GLB chunk"))?);
            if json_part.is_none() && kind != JSON_CHUNK {
                return Err(error("GLB JSON chunk must be first"));
            }
            pos = pos.checked_add(8).ok_or_else(|| error("GLB size overflow"))?;
            let end = pos.checked_add(len).filter(|end| *end <= bytes.len()).ok_or_else(|| error("GLB chunk exceeds file"))?;
            if !len.is_multiple_of(4) {
                return Err(error("GLB chunk is not 4-byte aligned"));
            }
            if kind == JSON_CHUNK {
                if json_part.is_some() || bin_part.is_some() {
                    return Err(error("invalid GLB chunk order"));
                }
                json_part = Some(&bytes[pos..end]);
            } else if kind == BIN_CHUNK {
                if json_part.is_none() || bin_part.is_some() {
                    return Err(error("invalid GLB binary chunk"));
                }
                bin_part = Some(bytes[pos..end].to_vec());
            }
            pos = end;
        }
        (json_part.ok_or_else(|| error("GLB has no JSON chunk"))?, bin_part.unwrap_or_default())
    } else {
        (bytes, Vec::new())
    };
    let root: Value = serde_json::from_slice(json_bytes).map_err(|_| error("invalid JSON"))?;
    if root.get("asset").and_then(|a| a.get("version")).and_then(Value::as_str).is_none_or(|v| !v.starts_with("2.")) {
        return Err(error("only glTF 2.x is supported"));
    }
    let buffers = root.get("buffers").and_then(Value::as_array).ok_or_else(|| error("missing buffers"))?;
    if buffers.len() != 1 {
        return Err(error("exactly one embedded buffer is supported"));
    }
    let desc = buffers.first().ok_or_else(|| error("missing buffer"))?;
    if desc.get("byteLength").and_then(Value::as_u64).is_none() {
        return Err(error("invalid buffer byteLength"));
    }
    let need = val_usize(desc, "byteLength")?;
    if need > MAX_BYTES {
        return Err(error("buffer too large"));
    }
    let data = if let Some(uri) = desc.get("uri").and_then(Value::as_str) {
        let payload = uri
            .strip_prefix("data:application/octet-stream;base64,")
            .or_else(|| uri.strip_prefix("data:application/gltf-buffer;base64,"))
            .ok_or_else(|| error("external or unsupported buffer URI"))?;
        b64_decode(payload)?
    } else if !bin.is_empty() {
        bin
    } else {
        return Err(error("missing embedded buffer data"));
    };
    if data.len() < need {
        return Err(error("buffer is shorter than byteLength"));
    }
    Ok((root, data))
}

type Mat4 = [[f64; 4]; 4];

fn identity() -> Mat4 {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}

fn mul(a: Mat4, b: Mat4) -> Mat4 {
    let mut out = [[0.0; 4]; 4];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[r][k] * b[k][c]).sum();
        }
    }
    out
}

fn node_matrix(node: &Value) -> Result<Mat4> {
    if let Some(m) = node.get("matrix") {
        if node.get("translation").is_some() || node.get("rotation").is_some() || node.get("scale").is_some() {
            return Err(error("node cannot combine matrix and TRS transforms"));
        }
        let values = m.as_array().filter(|v| v.len() == 16).ok_or_else(|| error("invalid node matrix"))?;
        let mut out = [[0.0; 4]; 4];
        for (r, row) in out.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = values.get(c * 4 + r).and_then(Value::as_f64).filter(|x| finite(*x)).ok_or_else(|| error("invalid node matrix value"))?;
            }
        }
        return Ok(out);
    }
    let vector = |key: &str, defaults: [f64; 3]| -> Result<[f64; 3]> {
        let Some(v) = node.get(key) else { return Ok(defaults) };
        let a = v.as_array().filter(|a| a.len() == 3).ok_or_else(|| error("invalid node transform"))?;
        Ok([
            a[0].as_f64().filter(|n| finite(*n)).ok_or_else(|| error("invalid node transform"))?,
            a[1].as_f64().filter(|n| finite(*n)).ok_or_else(|| error("invalid node transform"))?,
            a[2].as_f64().filter(|n| finite(*n)).ok_or_else(|| error("invalid node transform"))?,
        ])
    };
    let t = vector("translation", [0.0; 3])?;
    let s = vector("scale", [1.0; 3])?;
    let q = if let Some(v) = node.get("rotation") {
        let a = v.as_array().filter(|a| a.len() == 4).ok_or_else(|| error("invalid node rotation"))?;
        [
            a[0].as_f64().filter(|n| finite(*n)).ok_or_else(|| error("invalid node rotation"))?,
            a[1].as_f64().filter(|n| finite(*n)).ok_or_else(|| error("invalid node rotation"))?,
            a[2].as_f64().filter(|n| finite(*n)).ok_or_else(|| error("invalid node rotation"))?,
            a[3].as_f64().filter(|n| finite(*n)).ok_or_else(|| error("invalid node rotation"))?,
        ]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    };
    let norm = q[0].hypot(q[1]).hypot(q[2]).hypot(q[3]);
    if norm == 0.0 || !norm.is_finite() {
        return Err(error("invalid node quaternion"));
    }
    let [x, y, z, w] = [q[0] / norm, q[1] / norm, q[2] / norm, q[3] / norm];
    let mut m = identity();
    m[0][0] = (1.0 - 2.0 * (y * y + z * z)) * s[0];
    m[0][1] = (2.0 * (x * y - z * w)) * s[1];
    m[0][2] = (2.0 * (x * z + y * w)) * s[2];
    m[1][0] = (2.0 * (x * y + z * w)) * s[0];
    m[1][1] = (1.0 - 2.0 * (x * x + z * z)) * s[1];
    m[1][2] = (2.0 * (y * z - x * w)) * s[2];
    m[2][0] = (2.0 * (x * z - y * w)) * s[0];
    m[2][1] = (2.0 * (y * z + x * w)) * s[1];
    m[2][2] = (1.0 - 2.0 * (x * x + y * y)) * s[2];
    m[0][3] = t[0];
    m[1][3] = t[1];
    m[2][3] = t[2];
    Ok(m)
}

fn transform(m: Mat4, p: Vec3) -> Result<Vec3> {
    let q = [p.x, p.y, p.z, 1.0];
    let mut o = [0.0; 4];
    for r in 0..4 {
        o[r] = (0..4).map(|c| m[r][c] * q[c]).sum();
    }
    if o[3] == 0.0 || !o[3].is_finite() {
        return Err(error("invalid transformed vertex"));
    }
    let p = Vec3::new(o[0] / o[3], o[1] / o[3], o[2] / o[3]);
    if !finite(p.x) || !finite(p.y) || !finite(p.z) {
        return Err(error("invalid transformed vertex"));
    }
    Ok(p)
}

fn read_accessor(root: &Value, bin: &[u8], index: usize, ty: &str, components: usize, component_type: u64) -> Result<Vec<f64>> {
    let accessor = root.get("accessors").and_then(Value::as_array).and_then(|a| a.get(index)).ok_or_else(|| error("accessor index out of range"))?;
    if accessor.get("type").and_then(Value::as_str) != Some(ty)
        || accessor.get("componentType").and_then(Value::as_u64) != Some(component_type)
        || accessor.get("normalized").and_then(Value::as_bool) == Some(true)
    {
        return Err(error("unsupported accessor type"));
    }
    let count = val_usize(accessor, "count")?;
    if count == 0 || count > MAX_VERTICES {
        return Err(error("accessor count out of range"));
    }
    let view_i = val_usize(accessor, "bufferView")?;
    let view =
        root.get("bufferViews").and_then(Value::as_array).and_then(|a| a.get(view_i)).ok_or_else(|| error("buffer view index out of range"))?;
    let base = opt_usize(view, "byteOffset", 0)?;
    let len = val_usize(view, "byteLength")?;
    if view.get("buffer").and_then(Value::as_u64) != Some(0) {
        return Err(error("accessor must use embedded buffer 0"));
    }
    let stride = opt_usize(view, "byteStride", components * 4)?;
    if stride < components * 4 || stride > 252 || stride % 4 != 0 {
        return Err(error("invalid accessor stride"));
    }
    if accessor.get("sparse").is_some() {
        return Err(error("sparse accessors are not supported"));
    }
    let offset = opt_usize(accessor, "byteOffset", 0)?;
    let start = base.checked_add(offset).ok_or_else(|| error("accessor offset overflow"))?;
    if start % 4 != 0 {
        return Err(error("misaligned accessor"));
    }
    let last = count
        .checked_sub(1)
        .and_then(|n| n.checked_mul(stride))
        .and_then(|n| start.checked_add(n))
        .and_then(|n| n.checked_add(components * 4))
        .ok_or_else(|| error("accessor size overflow"))?;
    let view_end = base.checked_add(len).ok_or_else(|| error("buffer view size overflow"))?;
    if last > view_end || last > bin.len() {
        return Err(error("accessor exceeds buffer view"));
    }
    let mut out = Vec::with_capacity(count * components);
    for i in 0..count {
        let off = start + i * stride;
        for c in 0..components {
            let p = off + c * 4;
            let n = f32::from_le_bytes(bin[p..p + 4].try_into().map_err(|_| error("truncated accessor"))?) as f64;
            if !finite(n) {
                return Err(error("non-finite accessor value"));
            }
            out.push(n);
        }
    }
    Ok(out)
}

fn read_indices(root: &Value, bin: &[u8], index: usize, vertex_count: usize) -> Result<Vec<u32>> {
    let accessor = root.get("accessors").and_then(Value::as_array).and_then(|a| a.get(index)).ok_or_else(|| error("index accessor out of range"))?;
    if accessor.get("type").and_then(Value::as_str) != Some("SCALAR") {
        return Err(error("indices must use SCALAR accessor"));
    }
    let ct = accessor.get("componentType").and_then(Value::as_u64).ok_or_else(|| error("missing index component type"))?;
    let size = match ct {
        5121 => 1,
        5123 => 2,
        5125 => 4,
        _ => return Err(error("unsupported index component type")),
    };
    let count = val_usize(accessor, "count")?;
    if count == 0 || count > MAX_TRIANGLES * 3 || count % 3 != 0 {
        return Err(error("index count out of range"));
    }
    let view_i = val_usize(accessor, "bufferView")?;
    let view =
        root.get("bufferViews").and_then(Value::as_array).and_then(|a| a.get(view_i)).ok_or_else(|| error("index buffer view out of range"))?;
    if view.get("buffer").and_then(Value::as_u64) != Some(0) || view.get("byteStride").is_some() {
        return Err(error("unsupported index buffer view"));
    }
    if accessor.get("sparse").is_some() {
        return Err(error("sparse indices are not supported"));
    }
    let base = opt_usize(view, "byteOffset", 0)?;
    let len = val_usize(view, "byteLength")?;
    let offset = opt_usize(accessor, "byteOffset", 0)?;
    let start = base.checked_add(offset).ok_or_else(|| error("index offset overflow"))?;
    if start % size != 0 {
        return Err(error("misaligned indices"));
    }
    let end = start.checked_add(count.checked_mul(size).ok_or_else(|| error("index size overflow"))?).ok_or_else(|| error("index size overflow"))?;
    if end > base.saturating_add(len) || end > bin.len() {
        return Err(error("indices exceed buffer view"));
    }
    let mut out = Vec::with_capacity(count);
    for chunk in bin[start..end].chunks_exact(size) {
        let n = match size {
            1 => u32::from(chunk[0]),
            2 => u32::from(u16::from_le_bytes([chunk[0], chunk[1]])),
            _ => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
        };
        if n as usize >= vertex_count {
            return Err(error("index out of range"));
        }
        out.push(n);
    }
    Ok(out)
}

fn material_color(root: &Value, primitive: &Value) -> Color {
    let Some(mi) = primitive.get("material").and_then(Value::as_u64).and_then(|x| usize::try_from(x).ok()) else { return Color::Index(7) };
    let Some(factor) = root
        .get("materials")
        .and_then(Value::as_array)
        .and_then(|a| a.get(mi))
        .and_then(|m| m.pointer("/pbrMetallicRoughness/baseColorFactor"))
        .and_then(Value::as_array)
    else {
        return Color::Index(7);
    };
    if factor.len() < 3 {
        return Color::Index(7);
    }
    let mut rgb = [255u8; 3];
    for (i, channel) in rgb.iter_mut().enumerate() {
        let Some(n) = factor.get(i).and_then(Value::as_f64).filter(|n| n.is_finite() && (0.0..=1.0).contains(n)) else { return Color::Index(7) };
        *channel = (n * 255.0).round() as u8;
    }
    Color::True(Rgb(rgb[0], rgb[1], rgb[2]))
}

fn add_triangle(d: &mut Drawing, layer: &str, color: Color, a: Vec3, b: Vec3, c: Vec3) -> Result<()> {
    let u = [b.x - a.x, b.y - a.y, b.z - a.z];
    let v = [c.x - a.x, c.y - a.y, c.z - a.z];
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    if n.iter().all(|x| *x == 0.0) || n.iter().any(|x| !x.is_finite()) {
        return Err(error("degenerate triangle"));
    }
    crate::mesh::add_face3d(d, layer, color, [a, b, c]).map_err(|e| error(&e.to_string()))
}

pub fn read(bytes: &[u8]) -> Result<Drawing> {
    let (root, bin) = unpack(bytes)?;
    let meshes = root.get("meshes").and_then(Value::as_array).ok_or_else(|| error("missing meshes"))?;
    if meshes.is_empty() || meshes.len() > MAX_NODES {
        return Err(error("mesh count out of range"));
    }
    let nodes = root.get("nodes").and_then(Value::as_array).cloned().unwrap_or_default();
    if nodes.len() > MAX_NODES {
        return Err(error("too many nodes"));
    }
    let mut d = Drawing::new_metric();
    let mut total = 0usize;
    let mut used = false;
    if !nodes.is_empty() {
        let scenes = root.get("scenes").and_then(Value::as_array).ok_or_else(|| error("nodes require a scene"))?;
        let si = root.get("scene").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).unwrap_or(0);
        let scene = scenes.get(si).ok_or_else(|| error("scene index out of range"))?;
        let roots = scene.get("nodes").and_then(Value::as_array).ok_or_else(|| error("scene has no root nodes"))?;
        let mut stack = Vec::with_capacity(roots.len());
        for root_node in roots {
            let ni = root_node.as_u64().and_then(|i| usize::try_from(i).ok()).ok_or_else(|| error("invalid scene node index"))?;
            stack.push((ni, identity(), 0, Vec::new()));
        }
        while let Some((ni, parent, depth, mut path)) = stack.pop() {
            if depth > 128 {
                return Err(error("node nesting exceeds limit"));
            }
            if path.contains(&ni) {
                return Err(error("node cycle"));
            }
            path.push(ni);
            let node = nodes.get(ni).ok_or_else(|| error("node index out of range"))?;
            let world = mul(parent, node_matrix(node)?);
            if let Some(mesh_value) = node.get("mesh") {
                let mi = mesh_value.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| error("invalid node mesh index"))?;
                let mesh = meshes.get(mi).ok_or_else(|| error("mesh index out of range"))?;
                let name = node.get("name").or_else(|| mesh.get("name")).and_then(Value::as_str).unwrap_or("0");
                import_mesh(&mut d, &root, &bin, mesh, &bounded_name(name), world, &mut total)?;
                used = true;
            }
            if let Some(children) = node.get("children").and_then(Value::as_array) {
                for child_value in children {
                    let child = child_value.as_u64().and_then(|i| usize::try_from(i).ok()).ok_or_else(|| error("invalid child node index"))?;
                    stack.push((child, world, depth + 1, path.clone()));
                }
            } else if node.get("children").is_some() {
                return Err(error("node children must be an array"));
            }
        }
    } else {
        for mesh in meshes {
            let name = mesh.get("name").and_then(Value::as_str).unwrap_or("0");
            import_mesh(&mut d, &root, &bin, mesh, &bounded_name(name), identity(), &mut total)?;
            used = true;
        }
    }
    if !used || total == 0 {
        return Err(error("document contains no triangles"));
    }
    let has_vertex_attrs = meshes
        .iter()
        .filter_map(|m| m.get("primitives").and_then(Value::as_array))
        .flatten()
        .any(|p| p.pointer("/attributes/NORMAL").is_some() || p.pointer("/attributes/COLOR_0").is_some());
    if has_vertex_attrs {
        d.read_notes
            .push("glTF: imported vertex normals and vertex colours are reduced to faceted geometry and per-triangle material colours".into());
    }
    Ok(d)
}

fn import_mesh(d: &mut Drawing, root: &Value, bin: &[u8], mesh: &Value, name: &str, transform_m: Mat4, total: &mut usize) -> Result<()> {
    let primitives = mesh.get("primitives").and_then(Value::as_array).ok_or_else(|| error("mesh has no primitives"))?;
    for primitive in primitives {
        if primitive.get("mode").and_then(Value::as_u64).is_some_and(|m| m != 4) {
            return Err(error("only triangle primitives are supported"));
        }
        if primitive.get("targets").is_some() {
            return Err(error("morph targets are not supported"));
        }
        let pos_i = primitive
            .pointer("/attributes/POSITION")
            .and_then(Value::as_u64)
            .and_then(|i| usize::try_from(i).ok())
            .ok_or_else(|| error("primitive has no POSITION accessor"))?;
        let vals = read_accessor(root, bin, pos_i, "VEC3", 3, 5126)?;
        let vertices: Vec<Vec3> = vals.chunks_exact(3).map(|p| Vec3::new(p[0], p[1], p[2])).collect();
        if vertices.len() > MAX_VERTICES {
            return Err(error("too many vertices"));
        }
        let indices = if let Some(i) = primitive.get("indices").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok()) {
            read_indices(root, bin, i, vertices.len())?
        } else {
            if !vertices.len().is_multiple_of(3) {
                return Err(error("non-indexed vertex count is not triangular"));
            }
            (0..u32::try_from(vertices.len()).map_err(|_| error("too many vertices"))?).collect()
        };
        let tris = indices.len() / 3;
        *total = total.checked_add(tris).filter(|n| *n <= MAX_TRIANGLES).ok_or_else(|| error("too many triangles"))?;
        let color = material_color(root, primitive);
        let det = transform_m[0][0] * (transform_m[1][1] * transform_m[2][2] - transform_m[1][2] * transform_m[2][1])
            - transform_m[0][1] * (transform_m[1][0] * transform_m[2][2] - transform_m[1][2] * transform_m[2][0])
            + transform_m[0][2] * (transform_m[1][0] * transform_m[2][1] - transform_m[1][1] * transform_m[2][0]);
        for tri in indices.chunks_exact(3) {
            let a = transform(transform_m, *vertices.get(tri[0] as usize).ok_or_else(|| error("position index out of range"))?)?;
            let mut b = transform(transform_m, *vertices.get(tri[1] as usize).ok_or_else(|| error("position index out of range"))?)?;
            let mut c = transform(transform_m, *vertices.get(tri[2] as usize).ok_or_else(|| error("position index out of range"))?)?;
            if det < 0.0 {
                std::mem::swap(&mut b, &mut c);
            }
            add_triangle(d, name, color, a, b, c)?;
        }
    }
    Ok(())
}

fn triangle_data(d: &Drawing) -> Result<Vec<((String, Color), Vec<[Vec3; 3]>)>> {
    let mut out: HashMap<(String, Color), Vec<[Vec3; 3]>> = HashMap::new();
    let triangles = crate::mesh::face3d_triangles(d).map_err(|_| error("drawing contains unsupported or invalid mesh entities"))?;
    for (layer, color, triangle) in triangles {
        let key = (bounded_name(&layer), color);
        out.entry(key).or_default().push(triangle);
    }
    if out.is_empty() {
        return Err(error("drawing contains no 3D faces"));
    }
    let mut groups: Vec<_> = out.into_iter().collect();
    groups.sort_by_key(|((layer, color), _)| (layer.clone(), color.to_aci(), color.name()));
    Ok(groups)
}

fn effective_rgb(d: &Drawing, color: Color, layer: &str) -> Rgb {
    let layer_color = d.layer(layer).map(|l| l.color).unwrap_or(Color::Index(7));
    color.resolve(layer_color, Color::Index(7))
}

pub fn write(d: &Drawing, binary: bool) -> Result<Vec<u8>> {
    let groups = triangle_data(d)?;
    let mut bin = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut meshes = Vec::new();
    let mut nodes = Vec::new();
    let mut materials = Vec::new();
    for (mi, ((layer, color), triangles)) in groups.iter().enumerate() {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let pos_offset = bin.len();
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        let mut norms = Vec::with_capacity(triangles.len() * 3);
        for [a, b, c] in triangles {
            for p in [*a, *b, *c] {
                for (i, n) in [p.x, p.y, p.z].into_iter().enumerate() {
                    let f = n as f32;
                    if !f.is_finite() {
                        return Err(error("coordinate exceeds glTF float range"));
                    }
                    min[i] = min[i].min(f);
                    max[i] = max[i].max(f);
                    bin.extend_from_slice(&f.to_le_bytes());
                }
            }
            let u = [b.x - a.x, b.y - a.y, b.z - a.z];
            let v = [c.x - a.x, c.y - a.y, c.z - a.z];
            let mut n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len == 0.0 || !len.is_finite() {
                return Err(error("degenerate triangle"));
            }
            for x in &mut n {
                *x /= len;
            }
            for _ in 0..3 {
                norms.push(n);
            }
        }
        let pos_len = bin.len() - pos_offset;
        views.push(json!({"buffer":0,"byteOffset":pos_offset,"byteLength":pos_len,"target":34962}));
        accessors.push(json!({"bufferView":views.len()-1,"componentType":5126,"count":triangles.len()*3,"type":"VEC3","min":min,"max":max}));
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let norm_offset = bin.len();
        for n in norms {
            for v in n {
                let f = v as f32;
                if !f.is_finite() {
                    return Err(error("invalid normal"));
                }
                bin.extend_from_slice(&f.to_le_bytes());
            }
        }
        views.push(json!({"buffer":0,"byteOffset":norm_offset,"byteLength":bin.len()-norm_offset,"target":34962}));
        accessors.push(json!({"bufferView":views.len()-1,"componentType":5126,"count":triangles.len()*3,"type":"VEC3"}));
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let idx_offset = bin.len();
        let index_count = triangles.len().checked_mul(3).ok_or_else(|| error("too many indices"))?;
        for i in 0..u32::try_from(index_count).map_err(|_| error("too many indices"))? {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        views.push(json!({"buffer":0,"byteOffset":idx_offset,"byteLength":bin.len()-idx_offset,"target":34963}));
        accessors.push(json!({"bufferView":views.len()-1,"componentType":5125,"count":index_count,"type":"SCALAR"}));
        let rgb = effective_rgb(d, *color, layer);
        let material_index = materials.len();
        materials.push(json!({"name":format!("{} material", layer),"pbrMetallicRoughness":{"baseColorFactor":[rgb.0 as f64/255.0,rgb.1 as f64/255.0,rgb.2 as f64/255.0,1.0],"metallicFactor":0.0,"roughnessFactor":1.0}}));
        meshes.push(json!({"name":layer,"primitives":[{"attributes":{"POSITION":accessors.len()-3,"NORMAL":accessors.len()-2},"indices":accessors.len()-1,"material":material_index,"mode":4}]}));
        nodes.push(json!({"mesh":mi,"name":layer}));
        if bin.len() > MAX_BYTES {
            return Err(error("output exceeds size limit"));
        }
    }
    let root = json!({"asset":{"version":"2.0","generator":"CADCraft"},"scene":0,"scenes":[{"nodes":(0..nodes.len()).collect::<Vec<_>>()}],"nodes":nodes,"meshes":meshes,"materials":materials,"buffers":[{"byteLength":bin.len()}],"bufferViews":views,"accessors":accessors});
    let json_bytes = serde_json::to_vec(&root).map_err(|_| error("could not encode JSON"))?;
    if binary {
        let json_padded = json_bytes.len().checked_add(3).ok_or_else(|| error("output size overflow"))? & !3;
        let bin_padded = bin.len().checked_add(3).ok_or_else(|| error("output size overflow"))? & !3;
        let total = 12usize
            .checked_add(8 + json_padded)
            .and_then(|n| n.checked_add(8 + bin_padded))
            .filter(|n| *n <= MAX_BYTES)
            .ok_or_else(|| error("output exceeds size limit"))?;
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(&GLB_MAGIC.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json_padded as u32).to_le_bytes());
        out.extend_from_slice(&JSON_CHUNK.to_le_bytes());
        out.extend_from_slice(&json_bytes);
        out.resize(out.len() + json_padded - json_bytes.len(), b' ');
        out.extend_from_slice(&(bin_padded as u32).to_le_bytes());
        out.extend_from_slice(&BIN_CHUNK.to_le_bytes());
        out.extend_from_slice(&bin);
        out.resize(total, 0);
        Ok(out)
    } else {
        let uri = format!("data:application/octet-stream;base64,{}", b64_encode(&bin));
        let mut root = root;
        if let Some(buffer) = root.pointer_mut("/buffers/0") {
            buffer["uri"] = Value::String(uri);
        }
        serde_json::to_vec(&root)
            .map_err(|_| error("could not encode JSON"))
            .and_then(|v| if v.len() <= MAX_BYTES { Ok(v) } else { Err(error("output exceeds size limit")) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::EntityKind;

    #[test]
    fn glb_round_trip_triangle_and_color() {
        let mut d = Drawing::new_metric();
        add_triangle(&mut d, "Blue part", Color::True(Rgb(10, 90, 240)), Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)).unwrap();
        let bytes = write(&d, true).unwrap();
        let back = read(&bytes).unwrap();
        let e = back.model.iter().next().unwrap();
        assert_eq!(e.common.layer, "Blue part");
        assert_eq!(e.common.color, Color::True(Rgb(10, 90, 240)));
        assert!(matches!(e.kind, EntityKind::Face3d(_)));
    }

    #[test]
    fn embedded_json_round_trip_and_malformed_glb_rejected() {
        let mut d = Drawing::new_metric();
        add_triangle(&mut d, "0", Color::Index(1), Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)).unwrap();
        assert_eq!(read(&write(&d, false).unwrap()).unwrap().model.len(), 1);
        assert!(read(b"glTF\x02\x00\x00\x00").is_err());
    }

    #[test]
    fn rejects_noncanonical_base64_and_unbounded_quaternion() {
        for value in ["A===", "AA=A", "AB==", "AAB=", "AAAA="] {
            assert!(b64_decode(value).is_err(), "{value}");
        }
        assert!(node_matrix(&json!({"rotation":[1e308,1e308,0.0,1.0]})).is_err());
    }

    #[test]
    fn rejects_degenerate_import_triangle() {
        let mut d = Drawing::new_metric();
        assert!(add_triangle(&mut d, "0", Color::Index(7), Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0)).is_err());
    }
}
