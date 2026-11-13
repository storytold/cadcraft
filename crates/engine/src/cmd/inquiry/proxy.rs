//! Proxy graphics frames: the metafile an unmodelled entity (ACAD_PROXY_ENTITY, an
//! ARC_DIMENSION read raw, …) keeps in its DXF `310` tags. The frames are counted by type, not
//! decoded — the question downstream asks is "does this proxy draw anything", which the frame
//! types answer without interpreting any geometry.

use std::collections::BTreeMap;

use cadcraft_doc::{Drawing, RawTag, Unknown};
use serde_json::{Value, json};

/// The frame types that draw something (ODA .dwg specification, "Proxy Entity Graphics"). A
/// metafile made only of the others — state changes, transform push and pop — draws nothing:
/// every proxy of the 布吉 corpus is of that kind (762 frames, all matrix pushes and pops).
const DRAWABLE: [u32; 16] = [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 32, 33, 36, 38];

/// A frame type's name as the spec gives it (`None` for the numbers the spec leaves out).
fn type_name(t: u32) -> Option<&'static str> {
    Some(match t {
        1 => "Extents",
        2 => "Circle",
        3 => "Circle3Pt",
        4 => "CircularArc",
        5 => "CircularArc3Pt",
        6 => "Polyline",
        7 => "Polygon",
        8 => "Mesh",
        9 => "Shell",
        10 => "Text",
        11 => "Text2",
        12 => "XLine",
        13 => "Ray",
        14 => "SubentColor",
        16 => "SubentLayer",
        18 => "SubentLinetype",
        19 => "SubentMarker",
        20 => "SubentFillOn",
        21 => "FillOff",
        22 => "SubentTrueColor",
        23 => "SubentLineweight",
        24 => "SubentLtscale",
        25 => "SubentThickness",
        26 => "SubentPlotStyle",
        27 => "PushClip",
        28 => "PopClip",
        29 => "PushModelXform",
        30 => "PushModelXform2",
        31 => "PopModelXform",
        32 => "PolylineNormal",
        33 => "LwPolyline",
        34 => "SubentMaterial",
        35 => "SubentMapper",
        36 => "UnicodeText",
        37 => "Unknown",
        38 => "UnicodeText2",
        _ => return None,
    })
}

/// A frame's name for the histogram: the spec's name, or `T<n>` for a number it leaves out.
fn frame_name(t: u32) -> String {
    type_name(t).map(str::to_string).unwrap_or_else(|| format!("T{t}"))
}

/// Whether a frame of type `t` draws something.
fn draws(t: u32) -> bool {
    DRAWABLE.contains(&t)
}

/// The frames of one entity's `310` tags: their bytes, the length the file announced for them,
/// and how many of each type there are. `None` when the entity has no graphics data at all.
pub(super) struct Frames {
    /// Bytes of `310` data.
    pub(super) bytes: u64,
    /// The length the preceding `92`/`160` group announced (`originalBytes`).
    pub(super) declared: u64,
    /// Frames the walk read.
    pub(super) records: u64,
    /// Frames that draw something.
    pub(super) drawable: u64,
    /// The type histogram, drawable frames and the rest.
    pub(super) histogram: BTreeMap<String, u64>,
    /// Graphics blobs whose frames did not walk cleanly to their end (an OLE2FRAME's `310` is a
    /// picture, not a metafile, and lands here).
    pub(super) unparsed: u64,
}

/// One graphics blob: a `92`/`160` length group followed by its `310` hex chunks.
struct Blob {
    declared: u64,
    data: Vec<u8>,
}

/// The graphics blobs of `tags`: each a length group followed by consecutive `310` chunks.
/// `310` data without a length group before it is read as a blob of its own.
fn blobs(tags: &[RawTag]) -> Vec<Blob> {
    let hex_byte = |pair: &[u8]| -> Option<u8> {
        let hex = |b: u8| (b as char).to_digit(16);
        Some((hex(pair[0])? * 16 + hex(pair[1])?) as u8)
    };
    let mut out: Vec<Blob> = Vec::new();
    let mut current: Option<Blob> = None;
    let flush = |current: &mut Option<Blob>, out: &mut Vec<Blob>| {
        if let Some(b) = current.take().filter(|b| !b.data.is_empty()) {
            out.push(b);
        }
    };
    for t in tags {
        match t.code {
            92 | 160 => {
                // A new length group starts a new blob, even when the last one had no chunks.
                flush(&mut current, &mut out);
                current = Some(Blob { declared: t.value.trim().parse().unwrap_or(0), data: Vec::new() });
            }
            310 => {
                let b = current.get_or_insert_with(|| Blob { declared: 0, data: Vec::new() });
                for pair in t.value.as_bytes().as_chunks::<2>().0 {
                    if let Some(v) = hex_byte(pair) {
                        b.data.push(v);
                    }
                }
            }
            _ => flush(&mut current, &mut out),
        }
    }
    flush(&mut current, &mut out);
    out
}

/// Walk one metafile: total size, record count, then records of (size, type). Only the framing is
/// read — each record is skipped by its size, as the spec's reader does before its type switch.
fn walk(data: &[u8], histogram: &mut BTreeMap<String, u64>, drawable: &mut u64) -> Option<u64> {
    let u32_at = |o: usize| -> Option<u32> { data.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])) };
    let total = u32_at(0)? as usize;
    let count = u32_at(4)? as usize;
    if total < 8 || total > data.len() {
        return None;
    }
    let mut offset = 8;
    for _ in 0..count {
        let size = u32_at(offset)? as usize;
        let ty = u32_at(offset + 4)?;
        let end = offset.checked_add(size).filter(|e| *e <= total && size >= 8)?;
        if draws(ty) {
            *drawable += 1;
        }
        *histogram.entry(frame_name(ty)).or_default() += 1;
        offset = end;
    }
    (offset == total).then_some(count as u64)
}

/// The frames of `tags`, counted by type. `None` when there is no `310` graphics data.
pub(super) fn frames(tags: &[RawTag]) -> Option<Frames> {
    let blobs = blobs(tags);
    if blobs.is_empty() {
        return None;
    }
    let mut f = Frames { bytes: 0, declared: 0, records: 0, drawable: 0, histogram: BTreeMap::new(), unparsed: 0 };
    for b in &blobs {
        f.bytes = f.bytes.saturating_add(b.data.len() as u64);
        f.declared = f.declared.saturating_add(b.declared);
        match walk(&b.data, &mut f.histogram, &mut f.drawable) {
            Some(n) => f.records = f.records.saturating_add(n),
            None => f.unparsed += 1,
        }
    }
    Some(f)
}

/// The `proxy` object of an unmodelled entity's summary: its class, the application its class
/// names, and the frame counts that tell an empty shell (transforms and state only) from proxy
/// graphics that draw. `None` for entities without graphics data, and for an OLE2FRAME, whose
/// `310` is a picture rather than a metafile.
pub(super) fn summary(d: &Drawing, u: &Unknown) -> Option<Value> {
    if u.is_ole_frame() {
        return None;
    }
    let f = frames(&u.tags)?;
    let mut v = json!({
        "class": u.dxf_type,
        "graphicsBytes": f.bytes,
        "originalBytes": f.declared,
        "records": f.records,
        "drawable": f.drawable,
        "drawableChunks": f.histogram,
        "unparsed": f.unparsed,
    });
    // The application the proxy's class (group 91) names, when the file's CLASSES has it.
    if let Some(app) = d.foreign.proxy_class(&u.tags)
        && let Some(o) = v.as_object_mut()
    {
        o.insert("app".into(), json!(app));
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(code: i32, value: &str) -> RawTag {
        RawTag { code, value: value.to_string() }
    }

    /// A metafile of `frames`: (type, payload bytes) each, with the header the format needs.
    fn metafile(frames: &[(u32, &[u8])]) -> Vec<u8> {
        let mut data = vec![0u8; 8];
        for (ty, payload) in frames {
            let size = 8 + payload.len();
            data.extend_from_slice(&(size as u32).to_le_bytes());
            data.extend_from_slice(&ty.to_le_bytes());
            data.extend_from_slice(payload);
        }
        let total = data.len() as u32;
        data[0..4].copy_from_slice(&total.to_le_bytes());
        data[4..8].copy_from_slice(&(frames.len() as u32).to_le_bytes());
        data
    }

    fn hex(data: &[u8]) -> String {
        data.iter().map(|b| format!("{b:02X}")).collect()
    }

    fn tags_of(data: &[u8]) -> Vec<RawTag> {
        vec![tag(92, &data.len().to_string()), tag(310, &hex(data))]
    }

    #[test]
    fn matrix_only_metafile_draws_nothing() {
        let data = metafile(&[(29, [0.0f64; 16].iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<_>>().as_slice()), (31, &[])]);
        let f = frames(&tags_of(&data)).unwrap();
        assert_eq!(f.records, 2);
        assert_eq!(f.drawable, 0, "a metafile of only matrix pushes and pops draws nothing");
        assert_eq!(f.histogram.get("PushModelXform"), Some(&1));
        assert_eq!(f.histogram.get("PopModelXform"), Some(&1));
        assert_eq!(f.unparsed, 0);
    }

    #[test]
    fn a_polyline_frame_draws() {
        let data = metafile(&[(14, &1u32.to_le_bytes()), (6, [0u8; 4].as_slice())]);
        let f = frames(&tags_of(&data)).unwrap();
        assert_eq!(f.drawable, 1);
        assert_eq!(f.histogram.get("SubentColor"), Some(&1));
        assert_eq!(f.histogram.get("Polyline"), Some(&1));
        assert_eq!(f.declared, data.len() as u64);
    }

    #[test]
    fn a_broken_or_foreign_blob_counts_unparsed() {
        // Not a metafile: the announced total does not match the bytes.
        let data = vec![0u8; 32];
        let f = frames(&tags_of(&data)).unwrap();
        assert_eq!(f.records, 0);
        assert_eq!(f.unparsed, 1);
    }

    #[test]
    fn no_graphics_data_is_none() {
        assert!(frames(&[tag(0, "SECTION"), tag(2, "ENTITIES")]).is_none());
    }

    #[test]
    fn unknown_type_numbers_keep_their_number() {
        let data = metafile(&[(99, &[])]);
        let f = frames(&tags_of(&data)).unwrap();
        assert_eq!(f.histogram.get("T99"), Some(&1));
    }
}
