//! Raster images: IMAGEATTACH, IMAGECLIP, IMAGEADJUST and IMAGEFRAME.

use cadcraft_doc::{Entity, EntityKind, Handle, Image};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use crate::cmd::{CommandSpec, bad, f64_or, point_param, point_value, str_param};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("imageattach", "Attach Image...", run_imageattach)
            .menu(&["Insert", "Raster Image Reference..."])
            .alias(&["iat", "image"])
            .params("{path: image file (.png, .jpg, .bmp, etc.), at?: [x,y] (default 0,0), scale?: number (default 1.0), rotation?: degrees (default 0.0), size?: [w,h]} → {handle, path, name}")
            .interactive(|_| Ok(Box::new(ImageAttachM::default()))),
        CommandSpec::new("imageadjust", "Adjust Image...", run_imageadjust)
            .menu(&["Modify", "Object", "Image", "Adjust..."])
            .alias(&["iad"])
            .params("{handles?: [image handles], brightness?: 0..100, contrast?: 0..100, fade?: 0..100}")
            .interactive(|_| Ok(Box::new(ImageAdjustM::default()))),
        CommandSpec::new("imageclip", "Clip Image", run_imageclip)
            .menu(&["Modify", "Clip", "Image"])
            .alias(&["icl"])
            .params("{handle: image handle, boundary?: [[x,y],...], off?: bool, delete?: bool}"),
        CommandSpec::new("imageframe", "Image Frame", run_imageframe)
            .menu(&["Modify", "Object", "Image", "Frame"])
            .alias(&["iframe"])
            .params("{mode?: 0|1|2 (0: hide, 1: display & plot, 2: display only)}"),
    ]
}

/// Normalize an image path, resolving Windows paths on Linux/WSL if applicable.
pub fn normalize_path(p: &str) -> String {
    let s = p.trim().trim_matches('"').trim_matches('\'').trim();
    #[cfg(unix)]
    {
        let bytes = s.as_bytes();
        if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && (bytes[2] == b'/' || bytes[2] == b'\\') {
            let drive = (bytes[0] as char).to_ascii_lowercase();
            let rest = &s[3..].replace('\\', "/");
            let wsl_path = format!("/mnt/{drive}/{rest}");
            if std::path::Path::new(&wsl_path).exists() || std::path::Path::new(&format!("/mnt/{drive}")).exists() {
                return wsl_path;
            }
        }
    }
    s.to_string()
}

fn def_name_from_path(p: &str) -> String {
    let file = p.rsplit(['/', '\\']).next().unwrap_or(p);
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file).trim();
    if stem.is_empty() { "Image".to_string() } else { stem.to_string() }
}

/// Find an image file on disk using drawing-relative and system paths.
pub fn find_image_file(path_str: &str, doc_path: Option<&str>) -> Option<std::path::PathBuf> {
    let s = path_str.trim().trim_matches('"').trim_matches('\'').trim();
    if s.is_empty() {
        return None;
    }
    let norm = normalize_path(s);
    let p_norm = std::path::Path::new(&norm);
    if p_norm.is_file() {
        return Some(p_norm.to_path_buf());
    }

    let p_raw = std::path::Path::new(s);
    if p_raw.is_file() {
        return Some(p_raw.to_path_buf());
    }

    let file_name = s.rsplit(['/', '\\']).next().unwrap_or(s).trim();
    let clean_rel = s.replace('\\', "/");
    let clean_rel = clean_rel.trim_start_matches("./").trim_start_matches('/');

    let doc_dir = doc_path.and_then(|dp| {
        let p = std::path::Path::new(dp).parent()?;
        Some(if p.as_os_str().is_empty() { std::path::PathBuf::from(".") } else { p.to_path_buf() })
    });

    if let Some(dir) = &doc_dir {
        let candidates = [
            dir.join(clean_rel),
            dir.join(file_name),
            dir.join("images").join(file_name),
            dir.join("img").join(file_name),
            dir.join("Images").join(file_name),
            dir.join("IMG").join(file_name),
        ];
        if let Some(found) = candidates.into_iter().find(|c| c.is_file()) {
            return Some(found);
        }
    }

    let cwd_candidates = [
        std::path::PathBuf::from(clean_rel),
        std::path::PathBuf::from(file_name),
        std::path::Path::new("images").join(file_name),
        std::path::Path::new("img").join(file_name),
    ];
    cwd_candidates.into_iter().find(|c| c.is_file())
}

/// Resolve image bytes from in-memory assets or disk.
pub fn resolve_image_bytes(doc: &cadcraft_doc::Drawing, img: &cadcraft_doc::Image, _doc_path: Option<&str>) -> Option<Vec<u8>> {
    let file_name = img.path.rsplit(['/', '\\']).next().unwrap_or(&img.path).trim();
    if let Some(b) = doc.get_image_asset(&img.path).or_else(|| doc.get_image_asset(&img.name)).or_else(|| doc.get_image_asset(file_name)) {
        return Some(b.to_vec());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(f) = find_image_file(&img.path, _doc_path)
            && let Ok(bytes) = std::fs::read(f)
        {
            return Some(bytes);
        }
        if !img.name.is_empty()
            && let Some(f) = find_image_file(&img.name, _doc_path)
            && let Ok(bytes) = std::fs::read(f)
        {
            return Some(bytes);
        }
    }
    None
}

/// Preload image assets into `d.image_assets` for all Image entities in drawing.
pub fn preload_images(d: &mut cadcraft_doc::Drawing, doc_path: Option<&str>) {
    let mut to_load: Vec<(String, cadcraft_doc::Image)> = Vec::new();
    let collect = |store: &cadcraft_doc::EntityStore, list: &mut Vec<(String, cadcraft_doc::Image)>| {
        for e in store.iter() {
            if let cadcraft_doc::EntityKind::Image(img) = &e.kind
                && !img.path.is_empty()
                && !d.image_assets.contains_key(&img.path)
            {
                list.push((img.path.clone(), img.clone()));
            }
        }
    };
    collect(&d.model, &mut to_load);
    for l in &d.layouts {
        collect(&l.entities, &mut to_load);
    }
    for b in d.blocks.values() {
        collect(&b.entities, &mut to_load);
    }

    for (path, img) in to_load {
        if let Some(bytes) = resolve_image_bytes(d, &img, doc_path) {
            d.image_assets.insert(path, std::sync::Arc::new(bytes));
        }
    }
}

/// Attach a raster image file to the drawing.
pub fn run_imageattach(s: &mut Session, p: &Value) -> Result<Value> {
    let raw_path = str_param(p, "path").or_else(|| str_param(p, "file")).ok_or_else(|| bad("imageattach", "`path` is required"))?;
    let path = normalize_path(raw_path);
    let at = point_param(p, "at").unwrap_or(Vec2::ZERO);
    let scale = f64_or(p, "scale", 1.0).max(1e-9);
    let rotation = f64_or(p, "rotation", 0.0).to_radians();

    let _doc_path = s.drawing_path();
    let asset_bytes = {
        let d = s.doc()?;
        d.get_image_asset(&path).or_else(|| d.get_image_asset(raw_path)).map(|b| b.to_vec())
    };
    #[cfg(not(target_arch = "wasm32"))]
    let disk_bytes = if asset_bytes.is_none() {
        if let Some(f) = find_image_file(&path, _doc_path.as_deref()) {
            std::fs::read(f).ok()
        } else {
            std::fs::read(&path).ok().or_else(|| std::fs::read(raw_path).ok())
        }
    } else {
        None
    };
    #[cfg(target_arch = "wasm32")]
    let disk_bytes: Option<Vec<u8>> = None;

    if let Some(bytes) = &disk_bytes
        && let Ok(d) = s.doc_mut()
    {
        d.image_assets.insert(path.clone(), std::sync::Arc::new(bytes.clone()));
    }

    let has_bytes = asset_bytes.is_some() || disk_bytes.is_some();
    if !has_bytes && p.get("size").is_none() {
        s.echo(format!("Image file not found: {raw_path}"));
    }

    let size = if let Some(arr) = p.get("size").and_then(Value::as_array)
        && arr.len() >= 2
        && let (Some(w), Some(h)) = (arr[0].as_f64(), arr[1].as_f64())
    {
        Vec2::new(w.max(1.0), h.max(1.0))
    } else if let Some(bytes) = asset_bytes.as_deref().or(disk_bytes.as_deref()) {
        probe_image_size_from_bytes(bytes).unwrap_or(Vec2::new(100.0, 100.0))
    } else {
        Vec2::new(100.0, 100.0)
    };

    let name = str_param(p, "name").map(ToString::to_string).unwrap_or_else(|| def_name_from_path(&path));

    let cos = rotation.cos() * scale;
    let sin = rotation.sin() * scale;
    let u = Vec3::new(cos, sin, 0.0);
    let v = Vec3::new(-sin, cos, 0.0);

    let img = Image { insert: at.to3(0.0), u, v, size, path: path.clone(), name: name.clone(), ..Image::default() };

    let space = s.space();
    let d = s.doc_mut()?;
    if let Some(bytes) = disk_bytes {
        d.store_image_asset(&path, bytes.clone());
        if raw_path != path {
            d.store_image_asset(raw_path, bytes);
        }
    }
    let h = d.new_handle();
    let e = Entity::new(h, EntityKind::Image(img));
    d.ensure_layer(&e.common.layer);
    if let Some(st) = d.space_mut(&space) {
        st.push(e);
    }
    s.set_selection(vec![h]);

    Ok(json!({ "handle": h.hex(), "path": path, "name": name, "size": [size.x, size.y] }))
}

fn probe_image_size_from_bytes(bytes: &[u8]) -> Option<Vec2> {
    // PNG header: width at 16..20, height at 20..24
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        if w > 0 && h > 0 {
            return Some(Vec2::new(w as f64, h as f64));
        }
    }
    // JPEG header: scan SOF0 / SOF2 markers
    if bytes.starts_with(&[0xFF, 0xD8]) {
        let mut idx = 2;
        while idx + 4 < bytes.len() {
            if bytes[idx] != 0xFF {
                break;
            }
            let marker = bytes[idx + 1];
            let len = u16::from_be_bytes([bytes[idx + 2], bytes[idx + 3]]) as usize;
            if matches!(marker, 0xC0..=0xC2) && idx + 9 < bytes.len() {
                let h = u16::from_be_bytes([bytes[idx + 5], bytes[idx + 6]]);
                let w = u16::from_be_bytes([bytes[idx + 7], bytes[idx + 8]]);
                if w > 0 && h > 0 {
                    return Some(Vec2::new(w as f64, h as f64));
                }
            }
            if len < 2 {
                break;
            }
            idx += 2 + len;
        }
    }
    // BMP header: 'BM' followed by DIB header at offset 14 (width at 18..22, height at 22..26)
    if bytes.starts_with(b"BM") && bytes.len() >= 26 {
        let w = i32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]).unsigned_abs();
        let h = i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]).unsigned_abs();
        if w > 0 && h > 0 {
            return Some(Vec2::new(w as f64, h as f64));
        }
    }
    // GIF header: 'GIF87a' or 'GIF89a' (width at 6..8, height at 8..10)
    if bytes.starts_with(b"GIF") && bytes.len() >= 10 {
        let w = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
        let h = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
        if w > 0 && h > 0 {
            return Some(Vec2::new(w as f64, h as f64));
        }
    }
    // WEBP header: 'RIFF....WEBPVP8'
    if bytes.starts_with(b"RIFF") && bytes.len() >= 30 && &bytes[8..12] == b"WEBP" {
        if &bytes[12..16] == b"VP8 " && bytes.len() >= 30 {
            let w = u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3FFF;
            let h = u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3FFF;
            if w > 0 && h > 0 {
                return Some(Vec2::new(w as f64, h as f64));
            }
        } else if &bytes[12..16] == b"VP8L" && bytes.len() >= 25 {
            let b0 = bytes[21] as u32;
            let b1 = bytes[22] as u32;
            let b2 = bytes[23] as u32;
            let b3 = bytes[24] as u32;
            let w = 1 + (b0 | ((b1 & 0x3F) << 8));
            let h = 1 + ((b1 >> 6) | (b2 << 2) | ((b3 & 0xF) << 10));
            if w > 0 && h > 0 {
                return Some(Vec2::new(w as f64, h as f64));
            }
        }
    }
    None
}

/// Modify image brightness, contrast, and fade.
fn run_imageadjust(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = if let Some(arr) = p.get("handles").and_then(Value::as_array) {
        arr.iter().filter_map(|v| v.as_str().and_then(Handle::parse_hex)).collect()
    } else {
        s.selection()
    };
    if hs.is_empty() {
        return Err(bad("imageadjust", "no image selected"));
    }
    let b = p.get("brightness").and_then(Value::as_u64).map(|x| x.clamp(0, 100) as u8);
    let c = p.get("contrast").and_then(Value::as_u64).map(|x| x.clamp(0, 100) as u8);
    let f = p.get("fade").and_then(Value::as_u64).map(|x| x.clamp(0, 100) as u8);

    let space = s.space();
    let d = s.doc_mut()?;
    let mut modified = 0;
    if let Some(store) = d.space_mut(&space) {
        for h in hs {
            store.modify(h, |e| {
                if let EntityKind::Image(img) = &mut e.kind {
                    if let Some(val) = b {
                        img.brightness = val;
                    }
                    if let Some(val) = c {
                        img.contrast = val;
                    }
                    if let Some(val) = f {
                        img.fade = val;
                    }
                    modified += 1;
                }
            });
        }
    }
    Ok(json!({ "adjusted": modified }))
}

/// Clip an image entity with a rectangular or polygonal boundary.
fn run_imageclip(s: &mut Session, p: &Value) -> Result<Value> {
    let h = str_param(p, "handle").and_then(Handle::parse_hex).ok_or_else(|| bad("imageclip", "`handle` is required"))?;
    let off = p.get("off").and_then(Value::as_bool).unwrap_or(false);
    let del = p.get("delete").and_then(Value::as_bool).unwrap_or(false);

    let space = s.space();
    let d = s.doc_mut()?;
    let Some(store) = d.space_mut(&space) else { return Err(bad("imageclip", "no active space")) };

    let mut found = false;
    store.modify(h, |e| {
        if let EntityKind::Image(img) = &mut e.kind {
            found = true;
            if del {
                img.clip.clear();
                img.clipping = false;
            } else if off {
                img.clipping = false;
            } else if let Some(arr) = p.get("boundary").and_then(Value::as_array) {
                let pts: Vec<Vec2> = arr.iter().filter_map(point_value).collect();
                if !pts.is_empty() {
                    img.clip = pts;
                    img.clipping = true;
                }
            } else {
                img.clipping = true;
            }
        }
    });
    if !found {
        return Err(bad("imageclip", format!("no image with handle {}", h.hex())));
    }
    Ok(json!({ "handle": h.hex() }))
}

/// Control the display of the image frame boundary (IMAGEFRAME: 0, 1, 2).
fn run_imageframe(s: &mut Session, p: &Value) -> Result<Value> {
    let mode = p.get("mode").and_then(Value::as_u64).unwrap_or(1).clamp(0, 2) as u8;
    let d = s.doc_mut()?;
    d.header.set("IMAGEFRAME", cadcraft_doc::HVal::Int(mode as i64));
    Ok(json!({ "mode": mode }))
}

// ---------------- Interactive Prompts ----------------

#[derive(Default)]
enum AttachStage {
    #[default]
    Path,
    InsertPoint,
    Scale,
}

#[derive(Default)]
pub struct ImageAttachM {
    stage: AttachStage,
    path: String,
    size: Vec2,
    at: Vec2,
    scale: f64,
    rotation: f64,
}

impl ImageAttachM {
    /// Create a new interactive attach machine with an already-selected path (e.g. from a file picker).
    pub fn with_path(path: impl Into<String>) -> Self {
        let path = normalize_path(&path.into());
        ImageAttachM { stage: AttachStage::InsertPoint, path, size: Vec2::new(100.0, 100.0), at: Vec2::ZERO, scale: 1.0, rotation: 0.0 }
    }

    fn load_size_and_cache(&mut self, s: &mut Session) {
        let doc_path = s.drawing_path();
        let bytes = s
            .doc()
            .ok()
            .and_then(|d| d.get_image_asset(&self.path).map(|b| b.to_vec()))
            .or_else(|| {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    if let Some(f) = find_image_file(&self.path, doc_path.as_deref()) {
                        std::fs::read(f).ok()
                    } else {
                        std::fs::read(&self.path).ok()
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    None
                }
            });
        if let Some(b) = bytes {
            self.size = probe_image_size_from_bytes(&b).unwrap_or(Vec2::new(100.0, 100.0));
            if let Ok(d) = s.doc_mut() {
                d.store_image_asset(&self.path, b);
            }
        } else {
            self.size = Vec2::new(100.0, 100.0);
        }
    }

    fn default_scale(&self, s: &Session) -> f64 {
        let w = (self.size.x.max(1.0)) as u32;
        let h = (self.size.y.max(1.0)) as u32;
        crate::cmd::clipboard::default_image_scale(s, w, h)
    }

    fn corner_scale(&self, s: &Session, p: Vec2) -> f64 {
        let dx = (p.x - self.at.x).abs();
        let dy = (p.y - self.at.y).abs();
        if dx < 1e-6 && dy < 1e-6 {
            return self.default_scale(s);
        }
        let sx = dx / self.size.x.max(1.0);
        let sy = dy / self.size.y.max(1.0);
        sx.max(sy).max(1e-6)
    }
}

impl Interactive for ImageAttachM {
    fn name(&self) -> &'static str {
        "IMAGEATTACH"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            AttachStage::Path => Prompt::new("Enter image file path", Accept::TEXT),
            AttachStage::InsertPoint => Prompt::new("Specify insertion point <0,0>", Accept::POINT),
            AttachStage::Scale => {
                let mut p = Prompt::new("Specify opposite corner or scale factor <1.0>", Accept::POINT_OR_NUMBER);
                p.base = Some(self.at);
                p.corner = true;
                p
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.stage {
            AttachStage::Path => match i {
                Input::Text(t) => {
                    self.path = normalize_path(&t);
                    if self.path.is_empty() {
                        return Ok(Step::Done);
                    }
                    self.load_size_and_cache(s);
                    self.stage = AttachStage::InsertPoint;
                    Ok(Step::Continue)
                }
                Input::Enter => Ok(Step::Done),
                _ => Ok(Step::Continue),
            },
            AttachStage::InsertPoint => match i {
                Input::Point(p) => {
                    self.at = p;
                    self.stage = AttachStage::Scale;
                    Ok(Step::Continue)
                }
                Input::Enter => {
                    self.at = Vec2::ZERO;
                    self.stage = AttachStage::Scale;
                    Ok(Step::Continue)
                }
                Input::Text(t) => {
                    if let Some(p) = crate::prompt::parse_point(&t, Vec2::ZERO) {
                        self.at = p;
                        self.stage = AttachStage::Scale;
                    }
                    Ok(Step::Continue)
                }
                _ => Ok(Step::Continue),
            },
            AttachStage::Scale => {
                let def_scale = self.default_scale(s);
                match i {
                    Input::Point(p) => {
                        self.scale = self.corner_scale(s, p);
                    }
                    Input::Text(t) => {
                        if let Ok(n) = t.trim().parse::<f64>() {
                            self.scale = (def_scale * n).max(1e-9);
                        } else if let Some(p) = crate::prompt::parse_point(&t, self.at) {
                            self.scale = self.corner_scale(s, p);
                        } else {
                            self.scale = def_scale;
                        }
                    }
                    Input::Enter => {
                        self.scale = def_scale;
                    }
                    _ => return Ok(Step::Continue),
                }
                run_imageattach(
                    s,
                    &json!({
                        "path": self.path,
                        "at": [self.at.x, self.at.y],
                        "scale": self.scale,
                        "rotation": self.rotation,
                    }),
                )?;
                Ok(Step::Done)
            }
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.path.is_empty() {
            return Vec::new();
        }
        let size = if self.size.x > 1.0 && self.size.y > 1.0 { self.size } else { Vec2::new(100.0, 100.0) };
        match self.stage {
            AttachStage::InsertPoint => {
                let scale = self.default_scale(s);
                vec![EntityKind::Image(Image {
                    insert: c.to3(0.0),
                    u: Vec3::new(scale, 0.0, 0.0),
                    v: Vec3::new(0.0, scale, 0.0),
                    size,
                    path: self.path.clone(),
                    ..Image::default()
                })]
            }
            AttachStage::Scale => {
                let scale = self.corner_scale(s, c);
                vec![EntityKind::Image(Image {
                    insert: self.at.to3(0.0),
                    u: Vec3::new(scale, 0.0, 0.0),
                    v: Vec3::new(0.0, scale, 0.0),
                    size,
                    path: self.path.clone(),
                    ..Image::default()
                })]
            }
            AttachStage::Path => Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct ImageAdjustM {
    hs: Vec<Handle>,
}

impl Interactive for ImageAdjustM {
    fn name(&self) -> &'static str {
        "IMAGEADJUST"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.hs = s.selection();
        if self.hs.is_empty() {
            return Err(bad("imageadjust", "select image(s) to adjust first"));
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Enter brightness (0..100) or Enter to keep", Accept::NUMBER)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let b = match i {
            Input::Text(t) => t.trim().parse::<u64>().ok(),
            _ => None,
        };
        let handles: Vec<String> = self.hs.iter().map(|h| h.hex()).collect();
        run_imageadjust(s, &json!({ "handles": handles, "brightness": b }))?;
        Ok(Step::Done)
    }
}
