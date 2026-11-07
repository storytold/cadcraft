//! Raster images in DXF (DXF Reference: IMAGE entity; IMAGEDEF, IMAGEDEF_REACTOR and
//! RASTERVARIABLES objects). An IMAGE entity holds the placement, clip boundary and display
//! settings; it points (group 340) to an IMAGEDEF object holding the file path, which the
//! `ACAD_IMAGE_DICT` dictionary names. CADCraft keeps this data so images survive save and
//! reopen; the pixels themselves are not loaded.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cadcraft_doc::{Drawing, EntityKind, EntityStore, Handle, Image};
use cadcraft_dxf::{Section, Tag};
use cadcraft_geom::{Vec2, Vec3};

/// Most clip boundary vertices kept per image (hostile files).
const MAX_CLIP: usize = 100_000;

/// The IMAGE entity's own groups (after the `AcDbRasterImage` marker when present).
pub(crate) fn read_entity(tags: &[Tag]) -> Image {
    let start = tags.iter().position(|t| t.code == 100 && t.str() == "AcDbRasterImage").unwrap_or(0);
    let tags = tags.get(start..).unwrap_or(&[]);
    let f = |c: i32| tags.iter().find(|t| t.code == c).map(Tag::f64);
    let i = |c: i32| tags.iter().find(|t| t.code == c).map(Tag::i64);
    let p = |c: i32| Vec3::new(f(c).unwrap_or(0.0), f(c + 10).unwrap_or(0.0), f(c + 20).unwrap_or(0.0));
    let pct = |c: i32, d: u8| i(c).and_then(|v| u8::try_from(v.clamp(0, 100)).ok()).unwrap_or(d);
    let xs = tags.iter().filter(|t| t.code == 14).map(Tag::f64);
    let ys = tags.iter().filter(|t| t.code == 24).map(Tag::f64);
    let mut clip: Vec<Vec2> = xs.zip(ys).take(MAX_CLIP).map(|(x, y)| Vec2::new(x, y)).collect();
    // A rectangular boundary (type 1) is two opposite corners.
    if i(71) == Some(1) {
        clip.truncate(2);
    }
    let def = Image::default();
    Image {
        insert: p(10),
        u: p(11),
        v: p(12),
        size: Vec2::new(f(13).unwrap_or(1.0), f(23).unwrap_or(1.0)),
        clip,
        clipping: i(280).is_some_and(|v| v != 0),
        display: i(70).and_then(|v| u16::try_from(v).ok()).unwrap_or(def.display),
        brightness: pct(281, def.brightness),
        contrast: pct(282, def.contrast),
        fade: pct(283, def.fade),
        ..def
    }
}

/// Call `f` with each record (`0 TYPE` and the groups after it) of a section, without copying.
fn each_record<'a>(tags: &'a [Tag], mut f: impl FnMut(&str, &'a [Tag])) {
    let mut open: Option<(String, usize)> = None;
    for (i, t) in tags.iter().enumerate() {
        if t.code == 0 {
            if let Some((k, s)) = open.take() {
                f(&k, tags.get(s..i).unwrap_or(&[]));
            }
            open = Some((t.str(), i + 1));
        }
    }
    if let Some((k, s)) = open {
        f(&k, tags.get(s..).unwrap_or(&[]));
    }
}

fn key(s: &str) -> String {
    s.trim().to_ascii_uppercase()
}

/// An IMAGEDEF object's data.
#[derive(Default)]
struct Def {
    name: String,
    path: String,
    pixel_size: Option<Vec2>,
    units: u8,
}

fn has_images(store: &EntityStore) -> bool {
    store.iter().any(|e| matches!(e.kind, EntityKind::Image(_)))
}

/// The OLE2FRAME / OLEFRAME entity's geometry and placeholder properties.
pub(crate) fn read_ole_placeholder(tags: &[Tag]) -> Image {
    let f = |c: i32| tags.iter().find(|t| t.code == c).map(Tag::f64);
    let p = |c: i32| Vec3::new(f(c).unwrap_or(0.0), f(c + 10).unwrap_or(0.0), f(c + 20).unwrap_or(0.0));
    let p10 = p(10);
    let p11 = p(11);
    let insert = Vec3::new(p10.x.min(p11.x), p10.y.min(p11.y), p10.z);
    let dx = (p11.x - p10.x).abs().max(1e-6);
    let dy = (p10.y - p11.y).abs().max(1e-6);
    let h = tags.iter().find(|t| t.code == 5).and_then(|t| Handle::parse_hex(&t.str())).unwrap_or(Handle(0));
    let name = tags.iter().find(|t| t.code == 3).map(Tag::str).unwrap_or_else(|| "OLE Picture".to_string());
    let path = format!("__ole_{}__", h.hex());
    Image { insert, u: Vec3::new(dx, 0.0, 0.0), v: Vec3::new(0.0, dy, 0.0), size: Vec2::new(1.0, 1.0), path, name, ..Image::default() }
}

fn decode_hex_tags(tags: &[Tag]) -> Vec<u8> {
    let mut out = Vec::new();
    for t in tags.iter().filter(|t| t.code == 310) {
        let s = t.str();
        let bytes = s.trim().as_bytes();
        let mut i = 0;
        while i + 1 < bytes.len() {
            let h = match bytes[i] {
                b'0'..=b'9' => bytes[i] - b'0',
                b'a'..=b'f' => bytes[i] - b'a' + 10,
                b'A'..=b'F' => bytes[i] - b'A' + 10,
                _ => {
                    i += 1;
                    continue;
                }
            };
            let l = match bytes[i + 1] {
                b'0'..=b'9' => bytes[i + 1] - b'0',
                b'a'..=b'f' => bytes[i + 1] - b'a' + 10,
                b'A'..=b'F' => bytes[i + 1] - b'A' + 10,
                _ => {
                    i += 2;
                    continue;
                }
            };
            out.push((h << 4) | l);
            i += 2;
        }
    }
    out
}

/// Extract an embedded image (PNG, JPEG, BMP, or DIB) from the raw binary payload of an OLE object.
pub(crate) fn extract_ole_image(payload: &[u8]) -> Option<(Vec<u8>, Vec2)> {
    if payload.len() < 24 {
        return None;
    }

    // 1. Check for PNG signature: 89 50 4E 47 0D 0A 1A 0A
    const PNG_SIG: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    if let Some(pos) = payload.windows(8).position(|w| w == PNG_SIG)
        && pos + 24 <= payload.len()
    {
        let w = u32::from_be_bytes([payload[pos + 16], payload[pos + 17], payload[pos + 18], payload[pos + 19]]);
        let h = u32::from_be_bytes([payload[pos + 20], payload[pos + 21], payload[pos + 22], payload[pos + 23]]);
        if w > 0 && h > 0 && w <= 32768 && h <= 32768 {
            return Some((payload[pos..].to_vec(), Vec2::new(w as f64, h as f64)));
        }
    }

    // 2. Check for JPEG signature: FF D8 FF
    if let Some(pos) = payload.windows(3).position(|w| w == [0xFF, 0xD8, 0xFF]) {
        let mut cur = pos + 2;
        let mut size = None;
        while cur + 8 < payload.len() {
            if payload[cur] == 0xFF {
                let marker = payload[cur + 1];
                if marker == 0xC0 || marker == 0xC2 {
                    let h = u16::from_be_bytes([payload[cur + 5], payload[cur + 6]]);
                    let w = u16::from_be_bytes([payload[cur + 7], payload[cur + 8]]);
                    if w > 0 && h > 0 {
                        size = Some(Vec2::new(w as f64, h as f64));
                        break;
                    }
                }
                let len = u16::from_be_bytes([payload[cur + 2], payload[cur + 3]]) as usize;
                if len < 2 {
                    break;
                }
                cur += 2 + len;
            } else {
                cur += 1;
            }
        }
        let px = size.unwrap_or(Vec2::new(800.0, 600.0));
        return Some((payload[pos..].to_vec(), px));
    }

    // 3. Check for standard BMP signature: "BM"
    if let Some(pos) = payload.windows(2).position(|w| w == b"BM")
        && pos + 26 <= payload.len()
    {
        let header_size = u32::from_le_bytes([payload[pos + 14], payload[pos + 15], payload[pos + 16], payload[pos + 17]]);
        let w = i32::from_le_bytes([payload[pos + 18], payload[pos + 19], payload[pos + 20], payload[pos + 21]]);
        let h = i32::from_le_bytes([payload[pos + 22], payload[pos + 23], payload[pos + 24], payload[pos + 25]]).abs();
        if header_size >= 40 && w > 0 && h > 0 && w <= 32768 && h <= 32768 {
            let file_len = u32::from_le_bytes([payload[pos + 2], payload[pos + 3], payload[pos + 4], payload[pos + 5]]) as usize;
            let end = if file_len >= 26 && pos + file_len <= payload.len() { pos + file_len } else { payload.len() };
            return Some((payload[pos..end].to_vec(), Vec2::new(w as f64, h as f64)));
        }
    }

    // 4. Check for DIB header (BITMAPINFOHEADER): biSize = 40 (0x28, 0x00, 0x00, 0x00)
    for pos in 0..payload.len().saturating_sub(40) {
        if payload[pos..pos + 4] == [0x28, 0x00, 0x00, 0x00] {
            let w = i32::from_le_bytes([payload[pos + 4], payload[pos + 5], payload[pos + 6], payload[pos + 7]]);
            let h = i32::from_le_bytes([payload[pos + 8], payload[pos + 9], payload[pos + 10], payload[pos + 11]]).abs();
            let planes = u16::from_le_bytes([payload[pos + 12], payload[pos + 13]]);
            let bit_count = u16::from_le_bytes([payload[pos + 14], payload[pos + 15]]);
            let compression = u32::from_le_bytes([payload[pos + 16], payload[pos + 17], payload[pos + 18], payload[pos + 19]]);
            if planes == 1
                && (bit_count == 1 || bit_count == 4 || bit_count == 8 || bit_count == 16 || bit_count == 24 || bit_count == 32)
                && w > 0
                && h > 0
                && w <= 32768
                && h <= 32768
                && compression <= 3
            {
                let dib = &payload[pos..];
                let clr_used = u32::from_le_bytes([dib[32], dib[33], dib[34], dib[35]]);
                let palette_colors = if clr_used > 0 {
                    (clr_used as usize).min(256)
                } else if bit_count <= 8 {
                    1usize << bit_count
                } else {
                    0
                };
                let bf_off_bits = 14 + 40 + (palette_colors * 4) as u32;
                let file_size = 14 + dib.len() as u32;
                let mut bmp = Vec::with_capacity(14 + dib.len());
                bmp.extend_from_slice(b"BM");
                bmp.extend_from_slice(&file_size.to_le_bytes());
                bmp.extend_from_slice(&[0, 0, 0, 0]);
                bmp.extend_from_slice(&bf_off_bits.to_le_bytes());
                bmp.extend_from_slice(dib);
                return Some((bmp, Vec2::new(w as f64, h as f64)));
            }
        }
    }

    None
}

/// Fill in each image's definition (path, name, pixel size) from the IMAGEDEF it points to,
/// and extract embedded images from OLE frames.
pub(crate) fn resolve(secs: &[Section], d: &mut Drawing) {
    resolve_images(secs, d);
    resolve_ole(secs, d);
}

fn resolve_images(secs: &[Section], d: &mut Drawing) {
    if !has_images(&d.model) && !d.layouts.iter().any(|l| has_images(&l.entities)) && !d.blocks.values().any(|b| has_images(&b.entities)) {
        return;
    }
    // IMAGE handle → IMAGEDEF handle.
    let mut refs: HashMap<Handle, Handle> = HashMap::new();
    for s in secs.iter().filter(|s| s.name == "ENTITIES" || s.name == "BLOCKS") {
        each_record(&s.tags, |kind, tags| {
            if kind != "IMAGE" {
                return;
            }
            let h = tags.iter().find(|t| t.code == 5).and_then(|t| Handle::parse_hex(&t.str()));
            let def = tags.iter().find(|t| t.code == 340).and_then(|t| Handle::parse_hex(&t.str()));
            if let (Some(h), Some(def)) = (h, def) {
                refs.insert(h, def);
            }
        });
    }
    // IMAGEDEF objects, and the names dictionaries give them.
    let mut defs: HashMap<Handle, Def> = HashMap::new();
    let mut names: HashMap<Handle, String> = HashMap::new();
    for s in secs.iter().filter(|s| s.name == "OBJECTS") {
        each_record(&s.tags, |kind, tags| match kind {
            "IMAGEDEF" => {
                let Some(h) = tags.iter().find(|t| t.code == 5).and_then(|t| Handle::parse_hex(&t.str())) else { return };
                let start = tags.iter().position(|t| t.code == 100 && t.str() == "AcDbRasterImageDef").unwrap_or(0);
                let body = tags.get(start..).unwrap_or(&[]);
                let f = |c: i32| body.iter().find(|t| t.code == c).map(Tag::f64);
                let pixel = match (f(11), f(21)) {
                    (Some(x), Some(y)) if x > 0.0 && y > 0.0 => Some(Vec2::new(x, y)),
                    _ => None,
                };
                let units = body.iter().find(|t| t.code == 281).and_then(|t| u8::try_from(t.i64()).ok()).unwrap_or(0);
                let path = body.iter().find(|t| t.code == 1).map(Tag::str).unwrap_or_default();
                defs.insert(h, Def { path, pixel_size: pixel, units, ..Def::default() });
            }
            "DICTIONARY" => {
                let mut name: Option<String> = None;
                for t in tags {
                    match t.code {
                        3 => name = Some(t.str()),
                        350 | 360 => {
                            if let Some(n) = name.take()
                                && let Some(h) = Handle::parse_hex(&t.str())
                            {
                                names.insert(h, n);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        });
    }
    for (h, def) in &mut defs {
        if let Some(n) = names.get(h) {
            def.name = n.clone();
        }
    }
    let by_image: HashMap<Handle, &Def> = refs.iter().filter_map(|(img, dh)| Some((*img, defs.get(dh)?))).collect();
    if by_image.is_empty() {
        return;
    }
    // Aggregate budget on the image-definition strings copied into referring images, so an
    // attacker cannot multiply a long path/name across many references into gigabytes (CWE-400).
    const MAX_RESOLVED_STRING_BYTES: usize = 64 << 20;
    let mut budget = MAX_RESOLVED_STRING_BYTES;
    fill(&mut d.model, &by_image, &mut budget);
    for l in &mut d.layouts {
        fill(&mut l.entities, &by_image, &mut budget);
    }
    for b in d.blocks.values_mut() {
        if b.entities.iter().any(|e| matches!(e.kind, EntityKind::Image(_)) && by_image.contains_key(&e.handle)) {
            fill(&mut Arc::make_mut(b).entities, &by_image, &mut budget);
        }
    }
}

fn resolve_ole(secs: &[Section], d: &mut Drawing) {
    for s in secs.iter().filter(|s| s.name == "ENTITIES" || s.name == "BLOCKS") {
        each_record(&s.tags, |kind, tags| {
            if kind != "OLE2FRAME" && kind != "OLEFRAME" {
                return;
            }
            let Some(h) = tags.iter().find(|t| t.code == 5).and_then(|t| Handle::parse_hex(&t.str())) else { return };
            let raw_bytes = decode_hex_tags(tags);
            let asset_key = format!("__ole_{}__", h.hex());
            let name = tags.iter().find(|t| t.code == 3).map(Tag::str).unwrap_or_else(|| "Picture (Device Independent Bitmap)".to_string());
            let ole_type = tags.iter().find(|t| t.code == 71).map(|t| t.i64() as i32).unwrap_or(3);
            d.store_ole_item(asset_key.clone(), name, ole_type, raw_bytes.clone());
            let Some((bytes, px)) = extract_ole_image(&raw_bytes) else { return };
            d.image_assets.insert(asset_key, Arc::new(bytes));

            let update_store = |store: &mut EntityStore| {
                if let Some(e) = store.get(h)
                    && let EntityKind::Image(i) = &e.kind
                {
                    let dx = i.u.x.abs().max(1e-6);
                    let dy = i.v.y.abs().max(1e-6);
                    let u = Vec3::new(dx / px.x, 0.0, 0.0);
                    let v = Vec3::new(0.0, dy / px.y, 0.0);
                    store.modify(h, |e| {
                        if let EntityKind::Image(img) = &mut e.kind {
                            img.u = u;
                            img.v = v;
                            img.size = px;
                            img.pixel_size = px;
                        }
                    });
                }
            };
            update_store(&mut d.model);
            for l in &mut d.layouts {
                update_store(&mut l.entities);
            }
            for b in d.blocks.values_mut() {
                if b.entities.get(h).is_some() {
                    update_store(&mut Arc::make_mut(b).entities);
                }
            }
        });
    }
}

/// Resolve image definitions under a shared copied-string budget.
/// When strings no longer fit, keep applying pixel-size and resolution metadata.
fn fill(store: &mut EntityStore, by_image: &HashMap<Handle, &Def>, budget: &mut usize) {
    let hs: Vec<Handle> =
        store.iter().filter(|e| matches!(e.kind, EntityKind::Image(_)) && by_image.contains_key(&e.handle)).map(|e| e.handle).collect();
    for h in hs {
        let Some(def) = by_image.get(&h) else { continue };
        // Charge the bytes this copy adds; once the shared budget is spent, stop copying strings
        // (geometry fields are still applied) so retained memory stays bounded.
        let cost = def.path.len() + def.name.len();
        let copy_strings = budget.checked_sub(cost).map(|left| *budget = left).is_some();
        store.modify(h, |e| {
            if let EntityKind::Image(i) = &mut e.kind {
                if copy_strings {
                    i.path = def.path.clone();
                    i.name = def.name.clone();
                }
                if let Some(p) = def.pixel_size {
                    i.pixel_size = p;
                }
                i.resolution_units = def.units;
            }
        });
    }
}

/// One IMAGEDEF to write, shared by the images with the same path and name.
struct PlannedDef {
    handle: String,
    name: String,
    path: String,
    size: Vec2,
    pixel_size: Vec2,
    units: u8,
    /// (IMAGEDEF_REACTOR handle, IMAGE handle) per image using it.
    reactors: Vec<(String, Handle)>,
}

/// The objects behind a drawing's images, with handles allocated before anything is written.
#[derive(Default)]
pub(crate) struct Plan {
    /// IMAGE entity → (IMAGEDEF handle, IMAGEDEF_REACTOR handle).
    pub by_entity: HashMap<Handle, (String, String)>,
    defs: Vec<PlannedDef>,
    /// The `ACAD_IMAGE_DICT` dictionary and the `ACAD_IMAGE_VARS` RASTERVARIABLES object.
    pub dict: String,
    pub vars: String,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }
}

/// The image's definition name: its own, or the file name without extension.
fn def_name(i: &Image) -> String {
    let n = i.name.trim();
    if !n.is_empty() {
        return n.to_string();
    }
    let file = i.path.rsplit(['/', '\\']).next().unwrap_or("");
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file).trim();
    if stem.is_empty() { "Image".to_string() } else { stem.to_string() }
}

/// Allocate the IMAGEDEF, IMAGEDEF_REACTOR and dictionary handles for every image entity.
pub(crate) fn plan<'a>(
    entities: impl Iterator<Item = &'a cadcraft_doc::Entity>,
    ole_items: &std::collections::BTreeMap<String, cadcraft_doc::OleItem>,
    mut next: impl FnMut() -> String,
) -> Plan {
    let mut p = Plan::default();
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    let mut used: HashSet<String> = HashSet::new();
    // Next suffix to try for each normalised base name, so allocating a unique name resumes where
    // the previous collision left off instead of re-probing `_2, _3, …` from the start — otherwise
    // many images sharing a base name make name allocation quadratic (CWE-407).
    let mut next_suffix: HashMap<String, usize> = HashMap::new();
    for e in entities {
        let EntityKind::Image(i) = &e.kind else { continue };
        if ole_items.contains_key(&i.path) {
            continue;
        }
        if p.by_entity.contains_key(&e.handle) {
            continue;
        }
        let k = (i.path.clone(), i.name.clone());
        let at = match index.get(&k) {
            Some(&at) => at,
            None => {
                // Dictionary keys are unique regardless of case.
                let base = def_name(i);
                let mut name = base.clone();
                let mut n = next_suffix.get(&key(&base)).copied().unwrap_or(2);
                while !used.insert(key(&name)) {
                    name = format!("{base}_{n}");
                    n = n.saturating_add(1);
                }
                next_suffix.insert(key(&base), n);
                p.defs.push(PlannedDef {
                    handle: next(),
                    name,
                    path: i.path.clone(),
                    size: i.size,
                    pixel_size: i.pixel_size,
                    units: i.resolution_units,
                    reactors: Vec::new(),
                });
                let at = p.defs.len().saturating_sub(1);
                index.insert(k, at);
                at
            }
        };
        let reactor = next();
        if let Some(def) = p.defs.get_mut(at) {
            def.reactors.push((reactor.clone(), e.handle));
            p.by_entity.insert(e.handle, (def.handle.clone(), reactor));
        }
    }
    if !p.defs.is_empty() {
        p.dict = next();
        p.vars = next();
    }
    p
}

fn p3(t: &mut Vec<Tag>, c: i32, v: Vec3) {
    t.extend([Tag::f(c, v.x), Tag::f(c + 10, v.y), Tag::f(c + 20, v.z)]);
}

/// The IMAGE entity's groups after its common entity groups.
pub(crate) fn entity_tags(i: &Image, def: &str, reactor: &str) -> Vec<Tag> {
    let mut t = vec![Tag::i(90, 0)];
    p3(&mut t, 10, i.insert);
    p3(&mut t, 11, i.u);
    p3(&mut t, 12, i.v);
    t.extend([Tag::f(13, i.size.x), Tag::f(23, i.size.y), Tag::s(340, def), Tag::i(70, i64::from(i.display))]);
    t.extend([
        Tag::i(280, i64::from(i.clipping)),
        Tag::i(281, i64::from(i.brightness.min(100))),
        Tag::i(282, i64::from(i.contrast.min(100))),
        Tag::i(283, i64::from(i.fade.min(100))),
        Tag::s(360, reactor),
    ]);
    // Without a boundary of its own the image is clipped to its full extent (pixel corners).
    let full = [Vec2::new(-0.5, -0.5), Vec2::new(i.size.x - 0.5, i.size.y - 0.5)];
    let clip: &[Vec2] = if i.clip.len() >= 2 { &i.clip } else { &full };
    t.push(Tag::i(71, if clip.len() == 2 { 1 } else { 2 }));
    t.push(Tag::i(91, clip.len() as i64));
    for q in clip {
        t.extend([Tag::f(14, q.x), Tag::f(24, q.y)]);
    }
    t
}

/// The image dictionary, IMAGEDEF and IMAGEDEF_REACTOR objects and RASTERVARIABLES; `root` is
/// the named object dictionary, which lists `ACAD_IMAGE_DICT` and `ACAD_IMAGE_VARS`.
pub(crate) fn objects(p: &Plan, root: &str) -> Vec<Tag> {
    let mut t = Vec::new();
    if p.is_empty() {
        return t;
    }
    t.extend([Tag::s(0, "DICTIONARY"), Tag::s(5, &p.dict), Tag::s(330, root), Tag::s(100, "AcDbDictionary"), Tag::i(281, 1)]);
    for d in &p.defs {
        t.extend([Tag::s(3, &d.name), Tag::s(350, &d.handle)]);
    }
    for d in &p.defs {
        t.extend([Tag::s(0, "IMAGEDEF"), Tag::s(5, &d.handle), Tag::s(102, "{ACAD_REACTORS"), Tag::s(330, &p.dict)]);
        t.extend(d.reactors.iter().map(|(r, _)| Tag::s(330, r)));
        t.extend([Tag::s(102, "}"), Tag::s(330, &p.dict), Tag::s(100, "AcDbRasterImageDef"), Tag::i(90, 0), Tag::s(1, &d.path)]);
        t.extend([Tag::f(10, d.size.x), Tag::f(20, d.size.y), Tag::f(11, d.pixel_size.x), Tag::f(21, d.pixel_size.y)]);
        t.extend([Tag::i(280, 1), Tag::i(281, i64::from(d.units))]);
        for (r, img) in &d.reactors {
            let ih = img.hex();
            t.extend([Tag::s(0, "IMAGEDEF_REACTOR"), Tag::s(5, r), Tag::s(330, &ih), Tag::s(100, "AcDbRasterImageDefReactor")]);
            t.extend([Tag::i(90, 2), Tag::s(330, ih)]);
        }
    }
    // Image frame shown, high display quality, no insertion units.
    t.extend([Tag::s(0, "RASTERVARIABLES"), Tag::s(5, &p.vars), Tag::s(102, "{ACAD_REACTORS"), Tag::s(330, root), Tag::s(102, "}")]);
    t.extend([Tag::s(330, root), Tag::s(100, "AcDbRasterVariables"), Tag::i(90, 0), Tag::i(70, 1), Tag::i(71, 1), Tag::i(72, 0)]);
    t
}
