//! 3MF Core triangle-mesh import/export.
//!
//! This is mesh exchange only. 3MF is written as a small ZIP package and currently maps object
//! names to CADCraft layers and base-material colours to per-face entity colours.

use std::collections::{BTreeMap, HashMap};

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Drawing, HVal};
use cadcraft_geom::Vec3;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::{IoError, Result};

const MAX_BYTES: usize = 64 << 20;
const MAX_ENTRIES: usize = 128;
const MAX_TRIANGLES: usize = 1_000_000;
const MAX_VERTICES: usize = 1_000_000;
const MAX_OBJECTS: usize = 4096;

fn error(s: &str) -> IoError {
    IoError::Format(format!("3MF: {s}"))
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for b in data {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn u16at(b: &[u8], p: usize) -> Result<u16> {
    let s = b.get(p..p + 2).ok_or_else(|| error("truncated ZIP"))?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}
fn u32at(b: &[u8], p: usize) -> Result<u32> {
    let s = b.get(p..p + 4).ok_or_else(|| error("truncated ZIP"))?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn zip_model(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() > MAX_BYTES {
        return Err(error("package too large"));
    }
    let from = bytes.len().saturating_sub(65_557);
    let eocd = (from..bytes.len().saturating_sub(3))
        .rev()
        .find(|p| bytes.get(*p..*p + 4) == Some(&[0x50, 0x4b, 0x05, 0x06]))
        .ok_or_else(|| error("ZIP end record missing"))?;
    let comment_len = usize::from(u16at(bytes, eocd + 20)?);
    if eocd.checked_add(22).and_then(|p| p.checked_add(comment_len)) != Some(bytes.len()) {
        return Err(error("invalid ZIP comment length"));
    }
    if u16at(bytes, eocd + 4)? != 0 || u16at(bytes, eocd + 6)? != 0 {
        return Err(error("multi-disk ZIP is unsupported"));
    }
    let entries = usize::from(u16at(bytes, eocd + 10)?);
    if entries == 0 || entries > MAX_ENTRIES || u16at(bytes, eocd + 8)? as usize != entries {
        return Err(error("invalid ZIP entry count"));
    }
    let cd_size = usize::try_from(u32at(bytes, eocd + 12)?).map_err(|_| error("invalid ZIP directory"))?;
    let cd_off = usize::try_from(u32at(bytes, eocd + 16)?).map_err(|_| error("invalid ZIP directory"))?;
    if cd_size == u32::MAX as usize || cd_off == u32::MAX as usize {
        return Err(error("ZIP64 is not supported"));
    }
    let cd_end = cd_off.checked_add(cd_size).filter(|n| *n <= eocd).ok_or_else(|| error("ZIP directory exceeds file"))?;
    let mut pos = cd_off;
    let mut total_unpacked = 0usize;
    let mut found = None;
    for _ in 0..entries {
        if u32at(bytes, pos)? != 0x0201_4b50 {
            return Err(error("invalid ZIP directory entry"));
        }
        let flags = u16at(bytes, pos + 8)?;
        let method = u16at(bytes, pos + 10)?;
        let crc = u32at(bytes, pos + 16)?;
        let csize = usize::try_from(u32at(bytes, pos + 20)?).map_err(|_| error("invalid ZIP size"))?;
        let usize_ = usize::try_from(u32at(bytes, pos + 24)?).map_err(|_| error("invalid ZIP size"))?;
        let nlen = usize::from(u16at(bytes, pos + 28)?);
        let xlen = usize::from(u16at(bytes, pos + 30)?);
        let clen = usize::from(u16at(bytes, pos + 32)?);
        let disk = u16at(bytes, pos + 34)?;
        let local = usize::try_from(u32at(bytes, pos + 42)?).map_err(|_| error("invalid local header offset"))?;
        if disk != 0 || flags & 1 != 0 {
            return Err(error("encrypted or split ZIP entry"));
        }
        if csize == u32::MAX as usize || usize_ == u32::MAX as usize {
            return Err(error("ZIP64 is not supported"));
        }
        total_unpacked = total_unpacked.checked_add(usize_).filter(|n| *n <= MAX_BYTES).ok_or_else(|| error("uncompressed package exceeds limit"))?;
        let name_start = pos.checked_add(46).ok_or_else(|| error("ZIP size overflow"))?;
        let name_end = name_start.checked_add(nlen).filter(|e| *e <= cd_end).ok_or_else(|| error("invalid ZIP entry name"))?;
        let name = std::str::from_utf8(bytes.get(name_start..name_end).ok_or_else(|| error("truncated ZIP name"))?)
            .map_err(|_| error("ZIP names must be UTF-8"))?;
        if name.starts_with('/') || name.split('/').any(|s| s == "..") {
            return Err(error("unsafe ZIP path"));
        }
        let next =
            name_end.checked_add(xlen).and_then(|n| n.checked_add(clen)).filter(|e| *e <= cd_end).ok_or_else(|| error("invalid ZIP entry size"))?;
        pos = next;
        if name == "3D/3dmodel.model" {
            if found.is_some() {
                return Err(error("duplicate model part"));
            }
            if usize_ > MAX_BYTES || u32at(bytes, local)? != 0x0403_4b50 {
                return Err(error("invalid model local header"));
            }
            let ln = usize::from(u16at(bytes, local + 26)?);
            let lx = usize::from(u16at(bytes, local + 28)?);
            let data_start = local
                .checked_add(30)
                .and_then(|n| n.checked_add(ln))
                .and_then(|n| n.checked_add(lx))
                .ok_or_else(|| error("ZIP local offset overflow"))?;
            let data_end = data_start.checked_add(csize).filter(|e| *e <= cd_off).ok_or_else(|| error("truncated ZIP model part"))?;
            let local_name = bytes.get(local + 30..local + 30 + ln).ok_or_else(|| error("truncated local name"))?;
            if local_name != name.as_bytes() {
                return Err(error("ZIP directory/local name mismatch"));
            }
            if u16at(bytes, local + 8)? != method {
                return Err(error("ZIP local/central method mismatch"));
            }
            let local_flags = u16at(bytes, local + 6)?;
            if local_flags != flags {
                return Err(error("ZIP local/central flags mismatch"));
            }
            let data = match method {
                0 if csize == usize_ => bytes[data_start..data_end].to_vec(),
                0 => return Err(error("stored ZIP entry has inconsistent sizes")),
                8 => miniz_oxide::inflate::decompress_to_vec_with_limit(&bytes[data_start..data_end], MAX_BYTES)
                    .map_err(|_| error("damaged deflate stream"))?,
                _ => return Err(error("unsupported ZIP compression method")),
            };
            if data.len() != usize_ || crc32(&data) != crc {
                return Err(error("model part size or checksum mismatch"));
            }
            found = Some(data);
        }
    }
    if pos != cd_end {
        return Err(error("ZIP directory size mismatch"));
    }
    found.ok_or_else(|| error("3D/3dmodel.model part missing"))
}

fn attr(e: &BytesStart<'_>, key: &str, _reader: &Reader<&[u8]>) -> Result<Option<String>> {
    for a in e.attributes().with_checks(true) {
        let a = a.map_err(|_| error("invalid XML attribute"))?;
        if a.key.as_ref() == key {
            return a
                .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                .map(|v| Some(v.into_owned()))
                .map_err(|_| error("invalid XML attribute value"));
        }
    }
    Ok(None)
}

fn req_attr(e: &BytesStart<'_>, key: &str, reader: &Reader<&[u8]>) -> Result<String> {
    attr(e, key, reader)?.ok_or_else(|| error("required XML attribute missing"))
}

fn parse_u32_attr(e: &BytesStart<'_>, key: &str, reader: &Reader<&[u8]>) -> Result<u32> {
    req_attr(e, key, reader)?.parse().map_err(|_| error("invalid numeric XML attribute"))
}

fn color(s: &str) -> Result<Color> {
    let s = s.strip_prefix('#').ok_or_else(|| error("invalid 3MF color"))?;
    // ST_ColorValue: #RRGGBB or #RRGGBBAA (alpha is not kept).
    let n = match s.len() {
        6 => u32::from_str_radix(s, 16).map(|n| n << 8),
        8 => u32::from_str_radix(s, 16),
        _ => return Err(error("3MF color must be RRGGBB or RRGGBBAA")),
    }
    .map_err(|_| error("invalid 3MF color"))?;
    Ok(Color::True(Rgb((n >> 24) as u8, (n >> 16) as u8, (n >> 8) as u8)))
}

fn safe_layer(s: &str) -> String {
    let mut v: String = s.chars().filter(|c| !c.is_control()).take(255).collect();
    if v.is_empty() {
        v = "0".into();
    }
    v
}

#[derive(Default)]
struct Obj {
    name: String,
    verts: Vec<Vec3>,
    tris: Vec<([u32; 3], Option<(u32, u32)>)>,
}
#[derive(Clone, Copy)]
struct Item {
    object: u32,
    m: [[f64; 4]; 4],
}

fn id_transform(s: &str) -> Result<[[f64; 4]; 4]> {
    let v: Vec<f64> = s.split_whitespace().map(|x| x.parse::<f64>().map_err(|_| error("invalid 3MF transform"))).collect::<Result<_>>()?;
    if v.len() != 12 || v.iter().any(|x| !x.is_finite() || x.abs() > 1.0e12) {
        return Err(error("invalid 3MF transform"));
    }
    Ok([[v[0], v[3], v[6], v[9]], [v[1], v[4], v[7], v[10]], [v[2], v[5], v[8], v[11]], [0.0, 0.0, 0.0, 1.0]])
}

fn transformed(m: [[f64; 4]; 4], p: Vec3) -> Result<Vec3> {
    let q = Vec3::new(
        m[0][0] * p.x + m[0][1] * p.y + m[0][2] * p.z + m[0][3],
        m[1][0] * p.x + m[1][1] * p.y + m[1][2] * p.z + m[1][3],
        m[2][0] * p.x + m[2][1] * p.y + m[2][2] * p.z + m[2][3],
    );
    if [q.x, q.y, q.z].iter().any(|x| !x.is_finite() || x.abs() > 1.0e12) {
        return Err(error("invalid transformed coordinate"));
    }
    Ok(q)
}

fn determinant(m: [[f64; 4]; 4]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn nondegenerate(a: Vec3, b: Vec3, c: Vec3) -> bool {
    let u = [b.x - a.x, b.y - a.y, b.z - a.z];
    let v = [c.x - a.x, c.y - a.y, c.z - a.z];
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    n.iter().any(|x| *x != 0.0) && n.iter().all(|x| x.is_finite())
}

fn unit_code(s: Option<&str>) -> Result<i64> {
    match s.unwrap_or("millimeter") {
        "micron" => Ok(13),
        "millimeter" => Ok(4),
        "centimeter" => Ok(5),
        "inch" => Ok(1),
        "foot" => Ok(2),
        "meter" => Ok(6),
        _ => Err(error("unsupported model unit")),
    }
}

#[derive(Default)]
struct ModelParser {
    unit: String,
    mats: HashMap<u32, Vec<Color>>,
    objects: HashMap<u32, Obj>,
    items: Vec<Item>,
    current_obj: Option<u32>,
    current_mat: Option<u32>,
    in_vertices: bool,
    in_triangles: bool,
}

fn model_tag(e: &BytesStart<'_>, empty: bool, reader: &Reader<&[u8]>, state: &mut ModelParser) -> Result<()> {
    let local = e.local_name();
    let tag = local.as_ref();
    match tag {
        "model" => {
            if let Some(u) = attr(e, "unit", reader)? {
                state.unit = u;
            }
        }
        "object" => {
            let id = parse_u32_attr(e, "id", reader)?;
            let name = attr(e, "name", reader)?.unwrap_or_else(|| format!("object-{id}"));
            if state.objects.len() >= MAX_OBJECTS || state.objects.contains_key(&id) {
                return Err(error("duplicate or excessive objects"));
            }
            state.objects.insert(id, Obj { name, ..Obj::default() });
            state.current_obj = Some(id);
        }
        "vertices" => state.in_vertices = true,
        "triangles" => state.in_triangles = true,
        "components" | "component" => return Err(error("component objects are not supported")),
        "basematerials" => state.current_mat = Some(parse_u32_attr(e, "id", reader)?),
        "base" => {
            if let Some(mid) = state.current_mat {
                let c = color(&req_attr(e, "displaycolor", reader)?)?;
                let m = state.mats.entry(mid).or_default();
                if m.len() >= 65_536 {
                    return Err(error("too many base materials"));
                }
                m.push(c);
            }
        }
        "vertex" if state.in_vertices => {
            let id = state.current_obj.ok_or_else(|| error("vertex outside object"))?;
            let obj = state.objects.get_mut(&id).ok_or_else(|| error("unknown object"))?;
            if obj.verts.len() >= MAX_VERTICES {
                return Err(error("too many vertices"));
            }
            let x = req_attr(e, "x", reader)?.parse::<f64>().map_err(|_| error("invalid vertex coordinate"))?;
            let y = req_attr(e, "y", reader)?.parse::<f64>().map_err(|_| error("invalid vertex coordinate"))?;
            let z = req_attr(e, "z", reader)?.parse::<f64>().map_err(|_| error("invalid vertex coordinate"))?;
            let p = Vec3::new(x, y, z);
            if [p.x, p.y, p.z].iter().any(|v| !v.is_finite() || v.abs() > 1e12) {
                return Err(error("invalid vertex coordinate"));
            }
            obj.verts.push(p);
        }
        "triangle" if state.in_triangles => {
            let id = state.current_obj.ok_or_else(|| error("triangle outside object"))?;
            let obj = state.objects.get_mut(&id).ok_or_else(|| error("unknown object"))?;
            if obj.tris.len() >= MAX_TRIANGLES {
                return Err(error("too many triangles"));
            }
            let tri = [parse_u32_attr(e, "v1", reader)?, parse_u32_attr(e, "v2", reader)?, parse_u32_attr(e, "v3", reader)?];
            let prop = match (attr(e, "pid", reader)?, attr(e, "p1", reader)?) {
                (Some(pid), Some(p)) => {
                    Some((pid.parse().map_err(|_| error("invalid material id"))?, p.parse().map_err(|_| error("invalid material index"))?))
                }
                (None, None) => None,
                _ => return Err(error("incomplete triangle material reference")),
            };
            obj.tris.push((tri, prop));
        }
        "item" => {
            if state.items.len() >= MAX_OBJECTS * 16 {
                return Err(error("too many build items"));
            }
            let object = parse_u32_attr(e, "objectid", reader)?;
            let m = attr(e, "transform", reader)?.map(|s| id_transform(&s)).transpose()?.unwrap_or([
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]);
            state.items.push(Item { object, m });
        }
        _ => {}
    }
    if empty {
        match tag {
            "object" => state.current_obj = None,
            "vertices" => state.in_vertices = false,
            "triangles" => state.in_triangles = false,
            "basematerials" => state.current_mat = None,
            _ => {}
        }
    }
    Ok(())
}

pub fn read(bytes: &[u8]) -> Result<Drawing> {
    let model = zip_model(bytes)?;
    let mut reader = Reader::from_reader(model.as_slice());
    reader.config_mut().trim_text(true);
    let mut state = ModelParser { unit: "millimeter".into(), ..ModelParser::default() };
    loop {
        match reader.read_event().map_err(|_| error("malformed model XML"))? {
            Event::Start(e) => model_tag(&e, false, &reader, &mut state)?,
            Event::Empty(e) => model_tag(&e, true, &reader, &mut state)?,
            Event::End(e) => match e.local_name().as_ref() {
                "object" => state.current_obj = None,
                "vertices" => state.in_vertices = false,
                "triangles" => state.in_triangles = false,
                "basematerials" => state.current_mat = None,
                _ => {}
            },
            Event::Eof => break,
            Event::DocType(_) => return Err(error("DOCTYPE is not supported")),
            _ => {}
        }
        if model.len() > MAX_BYTES {
            return Err(error("model part too large"));
        }
    }
    if state.items.is_empty() {
        return Err(error("build has no items"));
    }
    let units = unit_code(Some(&state.unit))?;
    let mut d = Drawing::new_metric();
    d.header.set("INSUNITS", HVal::Int(units));
    let mut total = 0usize;
    for item in state.items {
        let obj = state.objects.get(&item.object).ok_or_else(|| error("build item references missing object"))?;
        let layer = safe_layer(&obj.name);
        let det = determinant(item.m);
        if !det.is_finite() || det == 0.0 {
            return Err(error("singular or invalid build-item transform"));
        }
        for (tri, prop) in &obj.tris {
            let a = *obj.verts.get(tri[0] as usize).ok_or_else(|| error("triangle vertex index out of range"))?;
            let b = *obj.verts.get(tri[1] as usize).ok_or_else(|| error("triangle vertex index out of range"))?;
            let c = *obj.verts.get(tri[2] as usize).ok_or_else(|| error("triangle vertex index out of range"))?;
            let color = if let Some((pid, pi)) = prop {
                *state.mats.get(pid).and_then(|m| m.get(*pi as usize)).ok_or_else(|| error("triangle material reference out of range"))?
            } else {
                Color::Index(7)
            };
            let a = transformed(item.m, a)?;
            let mut b = transformed(item.m, b)?;
            let mut c = transformed(item.m, c)?;
            if det < 0.0 {
                std::mem::swap(&mut b, &mut c);
            }
            if !nondegenerate(a, b, c) {
                return Err(error("degenerate transformed triangle"));
            }
            crate::mesh::add_face3d(&mut d, &layer, color, [a, b, c]).map_err(|_| error("could not add mesh triangle"))?;
            total += 1;
            if total > MAX_TRIANGLES {
                return Err(error("too many built triangles"));
            }
        }
    }
    if total == 0 {
        return Err(error("model has no mesh triangles"));
    }
    d.read_notes.push("3MF: imported triangle geometry; normals are derived from triangle winding".into());
    Ok(d)
}

fn xml(s: &str) -> String {
    quick_xml::escape::escape(s).into_owned()
}

fn model_units(d: &Drawing) -> &'static str {
    match d.header.get("INSUNITS").and_then(HVal::as_i64).unwrap_or(0) {
        13 => "micron",
        1 => "inch",
        2 => "foot",
        4 => "millimeter",
        5 => "centimeter",
        6 => "meter",
        _ => "millimeter",
    }
}

fn effective_rgb(d: &Drawing, color: Color, layer: &str) -> Rgb {
    color.resolve(d.layer(layer).map(|l| l.color).unwrap_or(Color::Index(7)), Color::Index(7))
}

fn model_xml(d: &Drawing) -> Result<Vec<u8>> {
    let mut groups: BTreeMap<String, Vec<([Vec3; 3], Rgb)>> = BTreeMap::new();
    for (layer, color, triangle) in crate::mesh::face3d_triangles(d).map_err(|_| error("drawing contains unsupported or invalid mesh entities"))? {
        let key = safe_layer(&layer);
        let rgb = effective_rgb(d, color, &key);
        groups.entry(key).or_default().push((triangle, rgb));
    }
    if groups.is_empty() {
        return Err(error("drawing contains no 3D faces"));
    }
    let mut all_colors: Vec<Rgb> = groups.values().flat_map(|v| v.iter().map(|(_, c)| *c)).collect();
    all_colors.sort_by_key(|c| (c.0, c.1, c.2));
    all_colors.dedup();
    let color_index: HashMap<Rgb, usize> = all_colors.iter().enumerate().map(|(i, c)| (*c, i)).collect();
    let mut s = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\" unit=\"{}\" xml:lang=\"en-US\"><resources><basematerials id=\"1\">",
        model_units(d)
    );
    for (i, c) in all_colors.iter().enumerate() {
        s.push_str(&format!("<base name=\"material-{i}\" displaycolor=\"#{:02X}{:02X}{:02X}FF\"/>", c.0, c.1, c.2));
    }
    s.push_str("</basematerials>");
    let mut build = String::from("</resources><build>");
    for (oi, (name, tris)) in groups.iter().enumerate() {
        let oid = oi + 2;
        s.push_str(&format!("<object id=\"{oid}\" name=\"{}\" type=\"model\"><mesh><vertices>", xml(name)));
        // Weld corners with identical coordinates so neighbouring triangles share vertices and
        // closed solids are manifold, as the core spec requires of `type="model"` objects.
        let mut index: HashMap<[u64; 3], usize> = HashMap::new();
        let mut triangles = Vec::with_capacity(tris.len());
        for (tri, c) in tris {
            let mut ids = [0usize; 3];
            for (id, p) in ids.iter_mut().zip(tri) {
                if [p.x, p.y, p.z].iter().any(|v| !v.is_finite() || v.abs() > 1.0e12) {
                    return Err(error("coordinate exceeds 3MF exchange bounds"));
                }
                // -0.0 and 0.0 are the same point.
                let key = [p.x, p.y, p.z].map(|v| if v == 0.0 { 0.0f64.to_bits() } else { v.to_bits() });
                let next = index.len();
                *id = *index.entry(key).or_insert_with(|| {
                    s.push_str(&format!("<vertex x=\"{:.17}\" y=\"{:.17}\" z=\"{:.17}\"/>", p.x, p.y, p.z));
                    next
                });
            }
            if s.len() > MAX_BYTES {
                return Err(error("output exceeds size limit"));
            }
            // A triangle whose corners weld together has no area; 3MF requires distinct indices.
            if ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2] {
                triangles.push((ids, *c));
            }
        }
        s.push_str("</vertices><triangles>");
        for ([a, b, c3], c) in &triangles {
            let pi = color_index.get(c).copied().ok_or_else(|| error("material map missing"))?;
            s.push_str(&format!("<triangle v1=\"{a}\" v2=\"{b}\" v3=\"{c3}\" pid=\"1\" p1=\"{pi}\"/>"));
            if s.len() > MAX_BYTES {
                return Err(error("output exceeds size limit"));
            }
        }
        s.push_str("</triangles></mesh></object>");
        build.push_str(&format!("<item objectid=\"{oid}\"/>"));
        if s.len() + build.len() > MAX_BYTES {
            return Err(error("output exceeds size limit"));
        }
    }
    s.push_str(&build);
    s.push_str("</build></model>");
    if s.len() > MAX_BYTES {
        return Err(error("output exceeds size limit"));
    }
    Ok(s.into_bytes())
}

fn zip_write(files: &[(&str, &[u8])]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        if data.len() > MAX_BYTES || out.len() > MAX_BYTES {
            return Err(error("package output exceeds size limit"));
        }
        let n = name.as_bytes();
        let nlen = u16::try_from(n.len()).map_err(|_| error("ZIP name too long"))?;
        let size = u32::try_from(data.len()).map_err(|_| error("ZIP64 output is not supported"))?;
        let crc = crc32(data);
        let off = u32::try_from(out.len()).map_err(|_| error("ZIP64 output is not supported"))?;
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&nlen.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(n);
        out.extend_from_slice(data);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&nlen.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&off.to_le_bytes());
        central.extend_from_slice(n);
    }
    let coff = u32::try_from(out.len()).map_err(|_| error("ZIP64 output is not supported"))?;
    let clen = u32::try_from(central.len()).map_err(|_| error("ZIP64 output is not supported"))?;
    out.extend_from_slice(&central);
    let count = u16::try_from(files.len()).map_err(|_| error("too many ZIP entries"))?;
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&clen.to_le_bytes());
    out.extend_from_slice(&coff.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    if out.len() > MAX_BYTES {
        return Err(error("package output exceeds size limit"));
    }
    Ok(out)
}

pub fn write(d: &Drawing) -> Result<Vec<u8>> {
    let model = model_xml(d)?;
    const TYPES:&[u8]=b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/></Types>";
    const RELS:&[u8]=b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/></Relationships>";
    zip_write(&[("[Content_Types].xml", TYPES), ("_rels/.rels", RELS), ("3D/3dmodel.model", &model)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_color::{Color, Rgb};
    use cadcraft_doc::{Common, EntityKind, Face3d, Space};
    #[test]
    fn package_round_trip_keeps_name_color_geometry_and_units() {
        let mut d = Drawing::new_metric();
        d.header.set("INSUNITS", HVal::Int(1));
        d.add(
            &Space::Model,
            Common { layer: "Part A".into(), color: Color::True(Rgb(20, 40, 60)), ..Common::default() },
            EntityKind::Face3d(Face3d {
                corners: [Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0), Vec3::new(0.0, 3.0, 0.0), Vec3::new(0.0, 3.0, 0.0)],
                hidden_edges: 0,
            }),
        )
        .unwrap();
        let b = write(&d).unwrap();
        let r = read(&b).unwrap();
        let e = r.model.iter().next().unwrap();
        assert_eq!(e.common.layer, "Part A");
        assert_eq!(e.common.color, Color::True(Rgb(20, 40, 60)));
        assert_eq!(r.header.get("INSUNITS").and_then(HVal::as_i64), Some(1));
        assert!(matches!(e.kind, EntityKind::Face3d(_)));
    }
    #[test]
    fn malformed_package_is_bounded_and_rejected() {
        assert!(read(b"not a zip").is_err());
        assert!(read(&vec![0u8; MAX_BYTES + 1]).is_err());
    }

    #[test]
    fn reflected_build_item_preserves_triangle_winding() {
        let model = br#"<?xml version="1.0"?><model xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" unit="millimeter"><resources><object id="1" name="mirror"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources><build><item objectid="1" transform="-1 0 0 0 1 0 0 0 1 0 0 0"/></build></model>"#;
        let package = zip_write(&[("3D/3dmodel.model", model)]).unwrap();
        let d = read(&package).unwrap();
        let entity = d.model.iter().next().unwrap();
        assert!(matches!(entity.kind, EntityKind::Face3d(_)));
        let EntityKind::Face3d(f) = &entity.kind else { return };
        let [a, b, c, _] = f.corners;
        let cross_z = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        assert!(cross_z > 0.0);
    }

    #[test]
    fn rejects_degenerate_mesh_triangles() {
        let model = br#"<?xml version="1.0"?><model xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" unit="millimeter"><resources><object id="1"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="2" y="0" z="0"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources><build><item objectid="1"/></build></model>"#;
        let package = zip_write(&[("3D/3dmodel.model", model)]).unwrap();
        assert!(read(&package).unwrap_err().to_string().contains("degenerate"));
    }

    #[test]
    fn writer_preserves_f64_coordinate_precision() {
        let value = 0.123_456_789_123_456_78;
        let mut d = Drawing::new_metric();
        crate::mesh::add_face3d(
            &mut d,
            "precision",
            Color::Index(7),
            [Vec3::new(value, 0.0, 0.0), Vec3::new(value, 1.0, 0.0), Vec3::new(value, 0.0, 1.0)],
        )
        .unwrap();
        let reread = read(&write(&d).unwrap()).unwrap();
        let EntityKind::Face3d(face) = &reread.model.iter().next().unwrap().kind else { return };
        assert_eq!(face.corners[0].x, value);
    }

    /// Slicers often write `#RRGGBB`; both lengths of ST_ColorValue read (#2544 review).
    #[test]
    fn six_and_eight_digit_display_colours_read() {
        assert_eq!(color("#1A2B3C").unwrap(), Color::True(Rgb(0x1a, 0x2b, 0x3c)));
        assert_eq!(color("#1A2B3C80").unwrap(), Color::True(Rgb(0x1a, 0x2b, 0x3c)));
        for bad in ["#1A2B3", "1A2B3C", "#GGGGGG", "#1A2B3C8"] {
            assert!(color(bad).is_err(), "{bad}");
        }
        let model = br##"<?xml version="1.0"?><model xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" unit="millimeter"><resources><basematerials id="1"><base name="red" displaycolor="#FF0000"/></basematerials><object id="2"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices><triangles><triangle v1="0" v2="1" v3="2" pid="1" p1="0"/></triangles></mesh></object></resources><build><item objectid="2"/></build></model>"##;
        let d = read(&zip_write(&[("3D/3dmodel.model", model)]).unwrap()).unwrap();
        assert_eq!(d.model.iter().next().unwrap().common.color, Color::True(Rgb(255, 0, 0)));
    }

    /// Triangles that share corners share vertex indices, so a closed solid is a manifold
    /// `type="model"` object: a tetrahedron writes 4 vertices, every edge used twice.
    #[test]
    fn writer_welds_shared_vertices() {
        let mut d = Drawing::new_metric();
        let p = [Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)];
        for [a, b, c] in [[0, 2, 1], [0, 1, 3], [1, 2, 3], [0, 3, 2]] {
            crate::mesh::add_face3d(&mut d, "solid", Color::Index(7), [p[a], p[b], p[c]]).unwrap();
        }
        let xml = String::from_utf8(model_xml(&d).unwrap()).unwrap();
        assert_eq!(xml.matches("<vertex ").count(), 4);
        assert_eq!(xml.matches("<triangle ").count(), 4);
        let mut edges = std::collections::HashMap::new();
        for t in xml.split("<triangle ").skip(1) {
            let v = |k: &str| t.split(&format!("{k}=\"")).nth(1).and_then(|r| r.split('"').next()).unwrap().parse::<usize>().unwrap();
            let (a, b, c) = (v("v1"), v("v2"), v("v3"));
            for (x, y) in [(a, b), (b, c), (c, a)] {
                *edges.entry((x.min(y), x.max(y))).or_insert(0) += 1;
            }
        }
        assert_eq!(edges.len(), 6);
        assert!(edges.values().all(|n| *n == 2));
        assert_eq!(read(&write(&d).unwrap()).unwrap().model.len(), 4);
    }
}
