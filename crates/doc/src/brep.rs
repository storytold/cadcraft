//! Native exact-solid persistence, carried by a grouped set of compatible 3DFACE proxies.
//!
//! The analytic body lives once, on the first face, in compressed application xdata. The
//! proxy fingerprint prevents using stale exact geometry after another program edits faces.
//! DXF/DWG readers already preserve xdata and groups, so no proprietary entity is needed.
use crate::{Common, Drawing, EntityKind, Face3d, Group, Handle, RawTag, Space};
use cadcraft_geom::{Mesh3, brep::Body};
use serde::{Deserialize, Serialize};

const APP: &str = "CADCRAFT_BREP_V1";
const MAX_PACKED: usize = 7200;
const MAX_JSON: usize = 1_000_000;

#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    fingerprint: u64,
    body: Body,
}

fn fingerprint(d: &Drawing, members: &[Handle]) -> Result<u64, String> {
    let mut hash = 0xcbf29ce484222325u64;
    for h in members {
        let Some(e) = d.entity(*h) else { return Err("exact solid has a missing display face".into()) };
        let EntityKind::Face3d(face) = &e.kind else { return Err("exact solid group contains a non-face object".into()) };
        for p in face.corners {
            for v in [p.x, p.y, p.z] {
                if !v.is_finite() {
                    return Err("exact display contains a non-finite coordinate".into());
                }
                // DXF text and unit conversion can normalize signed zero.
                let value = if v == 0.0 { 0.0 } else { v };
                for b in value.to_bits().to_le_bytes() {
                    hash = (hash ^ u64::from(b)).wrapping_mul(0x100000001b3);
                }
            }
        }
    }
    Ok(hash ^ members.len() as u64)
}

fn payload(tags: &[RawTag]) -> Result<Option<Vec<u8>>, String> {
    let Some(start) = tags.iter().position(|t| t.code == 1001 && t.value == APP) else { return Ok(None) };
    let mut hex = String::new();
    for tag in tags.iter().skip(start + 1).take_while(|t| t.code != 1001) {
        if tag.code != 1000 || tag.value.len() > 250 || hex.len().saturating_add(tag.value.len()) > MAX_PACKED * 2 {
            return Err("invalid or oversized exact-solid payload".into());
        }
        hex.push_str(&tag.value);
    }
    if !hex.len().is_multiple_of(2) {
        return Err("invalid exact-solid encoding".into());
    }
    hex.as_bytes()
        .chunks_exact(2)
        .map(|b| {
            let digit = |x: u8| match x {
                b'0'..=b'9' => Some(x - b'0'),
                b'a'..=b'f' => Some(x - b'a' + 10),
                _ => None,
            };
            b.first()
                .copied()
                .and_then(digit)
                .zip(b.get(1).copied().and_then(digit))
                .map(|(a, b)| (a << 4) | b)
                .ok_or_else(|| "invalid exact-solid encoding".into())
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn remove_payload(tags: &mut Vec<RawTag>) {
    let mut keep = true;
    tags.retain(|t| {
        if t.code == 1001 {
            keep = t.value != APP;
        }
        keep
    });
}

/// Remove this application's exact source record while preserving other applications' data.
pub fn strip(common: &mut Common) {
    remove_payload(&mut common.xdata);
}

/// Read a body's checked source geometry; stale proxies are an explicit error, never treated
/// as the original exact solid. Group lookup follows current handles, including copies/imports.
pub fn read(d: &Drawing, members: &[Handle]) -> Result<Option<Body>, String> {
    let mut packed = None;
    for h in members {
        if let Some(e) = d.entity(*h)
            && let Some(bytes) = payload(&e.common.xdata)?
        {
            if packed.is_some() {
                return Err("exact solid has more than one source record".into());
            }
            packed = Some(bytes);
        }
    }
    let Some(packed) = packed else { return Ok(None) };
    let json = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&packed, MAX_JSON).map_err(|_| "damaged exact-solid payload")?;
    let record: Record = serde_json::from_slice(&json).map_err(|e| format!("invalid exact-solid model: {e}"))?;
    if record.version != 1 || record.fingerprint != fingerprint(d, members)? {
        return Err("display faces changed outside the exact-solid editor; source geometry is stale".into());
    }
    record.body.validate().map_err(|e| e.to_string())?;
    Ok(Some(record.body))
}

/// Attach exact geometry to already-created display faces. Payload stays below DXF's 16 KiB
/// application-data limit; individual text chunks stay below the 255-byte string limit.
pub fn attach(d: &mut Drawing, members: &[Handle], body: &Body) -> Result<(), String> {
    body.validate().map_err(|e| e.to_string())?;
    let Some(anchor) = members.first().copied() else { return Err("exact solid needs display faces".into()) };
    let record = Record { version: 1, fingerprint: fingerprint(d, members)?, body: body.clone() };
    let json = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
    if json.len() > MAX_JSON {
        return Err("exact model exceeds native persistence budget".into());
    }
    let packed = miniz_oxide::deflate::compress_to_vec_zlib(&json, 6);
    if packed.len() > MAX_PACKED {
        return Err("exact model exceeds native xdata budget".into());
    }
    let hex = packed.iter().map(|b| format!("{b:02x}")).collect::<String>();
    for h in members {
        d.modify_entity(*h, |e| remove_payload(&mut e.common.xdata)).map_err(|e| e.to_string())?;
    }
    d.modify_entity(anchor, |e| {
        e.common.xdata.push(RawTag { code: 1001, value: APP.into() });
        for part in hex.as_bytes().chunks(250) {
            e.common.xdata.push(RawTag { code: 1000, value: String::from_utf8_lossy(part).into_owned() });
        }
    })
    .map_err(|e| e.to_string())
}

/// Insert a complete exact solid plus its derived display mesh as one selectable group.
/// The caller's transaction owns undo; failures here restore the original drawing.
pub fn insert(d: &mut Drawing, body: &Body, mesh: &Mesh3, common: &Common) -> Result<(String, Vec<Handle>), String> {
    if mesh.triangles.is_empty() || mesh.triangles.len() > 50_000 {
        return Err("exact display exceeds face budget".into());
    }
    let backup = d.clone();
    let result = (|| {
        let mut handles = Vec::new();
        for t in &mesh.triangles {
            let [Some(a), Some(b), Some(c)] = t.map(|i| mesh.vertices.get(i).copied()) else { return Err("invalid exact display triangle".into()) };
            handles.push(
                d.add(&Space::Model, common.clone(), EntityKind::Face3d(Face3d { corners: [a, b, c, c], hidden_edges: 0 }))
                    .map_err(|e| e.to_string())?,
            );
        }
        let name = format!("*A{}", d.new_handle().hex());
        d.groups.push(Group { name: name.clone(), description: "3D solid".into(), selectable: true, members: handles.clone() });
        attach(d, &handles, body)?;
        Ok((name, handles))
    })();
    if result.is_err() {
        *d = backup;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_geom::Vec3;
    #[test]
    fn native_payload_roundtrip_detects_proxy_edits() {
        let body = Body::cylinder(Vec3::ZERO, 2.0, 4.0).unwrap();
        let mesh = body.tessellate(0.1).unwrap();
        let mut d = Drawing::new_metric();
        let (_, hs) = insert(&mut d, &body, &mesh, &Common::default()).unwrap();
        let restored = read(&d, &hs).unwrap().unwrap();
        assert_eq!(restored.geometry.boundaries[0].edges.len(), body.geometry.boundaries[0].edges.len());
        d.modify_entity(hs[0], |e| {
            if let EntityKind::Face3d(f) = &mut e.kind {
                f.corners[0].x += 1.0;
            }
        })
        .unwrap();
        assert!(read(&d, &hs).unwrap_err().contains("stale"));
    }
}
