//! Raster images (IMAGEATTACH, IMAGE, IMAGEADJUST, IMAGECLIP, IMAGEFRAME). An IMAGE entity names
//! its file; the pixels are decoded when the drawing opens or the image is attached and kept with
//! the open drawing ([`Drawing::rasters`]), never in the file. An image whose file can't be found
//! or read stays in the drawing as a crossed-out frame.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use cadcraft_doc::{Drawing, EntityKind, Handle, Image, Raster};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use super::file::base64_decode;
use super::{CommandSpec, bad, f64_or, point_param, point_value, str_param};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Most image files loaded per drawing.
const MAX_IMAGE_FILES: usize = 1000;
/// Largest image file read (bytes).
#[cfg(not(target_arch = "wasm32"))]
const MAX_IMAGE_BYTES: u64 = 256 << 20;
/// Most decoded pixel bytes kept per drawing; images past it stay unloaded.
const MAX_LOADED_BYTES: u64 = 2 << 30;
/// Most image files decoded at the same time when a drawing opens.
const MAX_LOADERS: usize = 4;
/// Longest image path taken from a caller.
const MAX_PATH: usize = 4096;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("imageattach", "Raster Image Reference...", run_imageattach)
            .menu(&["Insert", "Raster Image Reference..."])
            .alias(&["iat"])
            .params(
                "{path: PNG/JPEG/BMP/TIFF file (saved as given; a relative path is relative to this drawing's folder), data?: base64 file contents \
                 (instead of reading `path`), at?: [x,y] lower-left corner (default 0,0), scale?: number (default 1: the image is one drawing unit \
                 wide), width?: width in drawing units (instead of `scale`), rotation? (degrees), name? (default: the file name)} → {name, handle, \
                 path, pixels: [w, h], size: [w, h]}",
            )
            .interactive(|_| Ok(Box::new(ImageAttachM::default()))),
        CommandSpec::new("image", "Raster Images", run_image)
            .alias(&["im"])
            .params("{} → {images: [{name, path, foundAt, status: Loaded|Not Found|Not loaded, references, pixels?: [w, h]}]}")
            .noundo(),
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

/// Whether `ext` (lower case, no dot) is an image file type IMAGEATTACH reads.
pub(crate) fn is_image_ext(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "bmp" | "tif" | "tiff")
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

/// Find an image file on disk using drawing-relative and system paths.
pub fn find_image_file(path_str: &str, doc_path: Option<&str>) -> Option<std::path::PathBuf> {
    if let Some(found) = super::blocks::xref::find_file(path_str, doc_path) {
        return Some(found);
    }
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
pub fn resolve_image_bytes(doc: &Drawing, img: &Image, _doc_path: Option<&str>) -> Option<Vec<u8>> {
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

/// Preload image assets into `d.image_assets` and `d.rasters` for all Image entities in drawing.
pub fn preload_images(d: &mut Drawing, doc_path: Option<&str>) {
    let mut to_load: Vec<(String, Image)> = Vec::new();
    let collect = |store: &cadcraft_doc::EntityStore, list: &mut Vec<(String, Image)>| {
        for e in store.iter() {
            if let EntityKind::Image(img) = &e.kind
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
            if d.rasters.get(&path).is_none()
                && let Ok(r) = decode(&bytes)
            {
                d.rasters.insert(&path, Arc::new(r));
            }
            d.image_assets.insert(path, Arc::new(bytes));
        }
    }
}

/// Every IMAGE entity of `d`: in model space, on the layouts and in the blocks.
fn images(d: &Drawing) -> impl Iterator<Item = &Image> {
    let model = std::iter::once(&d.model);
    let layouts = d.layouts.iter().map(|l| &l.entities);
    let blocks = d.blocks.values().map(|b| &b.entities);
    model.chain(layouts).chain(blocks).flat_map(|store| store.iter()).filter_map(|e| match &e.kind {
        EntityKind::Image(i) => Some(i),
        _ => None,
    })
}

/// The image files `d` names, each once, in the order they are first used.
fn paths(d: &Drawing) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut paths = Vec::new();
    for i in images(d) {
        if !i.path.trim().is_empty() && seen.insert(i.path.as_str()) {
            paths.push(i.path.clone());
        }
    }
    paths
}

/// The contents of the image file saved as `path` in a drawing at `host`.
fn read_file(path: &str, host: Option<&str>) -> std::result::Result<Vec<u8>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let file = super::blocks::xref::find_file(path, host).or_else(|| find_image_file(path, host)).ok_or("file not found")?;
        let len = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
        if len > MAX_IMAGE_BYTES {
            return Err(format!("the file is larger than {} MB", MAX_IMAGE_BYTES >> 20));
        }
        // Read no more than the limit: the file can grow or change after the check above.
        let limit = usize::try_from(MAX_IMAGE_BYTES).unwrap_or(usize::MAX);
        crate::capped_read::read_capped(&file, limit).map_err(|e| {
            if e.kind() == std::io::ErrorKind::InvalidData { format!("the file is larger than {} MB", MAX_IMAGE_BYTES >> 20) } else { e.to_string() }
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (path, host);
        Err("files are not available on the web".into())
    }
}

/// Decode an image file. A decoder that panics on a damaged file fails this image only: the
/// drawing it belongs to must still open.
fn decode(bytes: &[u8]) -> std::result::Result<Raster, String> {
    let hook = super::file::image_decoder().ok_or("image files can't be read in this build")?;
    match std::panic::catch_unwind(|| hook(bytes)) {
        Ok(r) => r,
        Err(_) => Err("the file is damaged and can't be read".into()),
    }
}

fn bytes_of(r: &Raster) -> u64 {
    u64::try_from(r.rgba().len()).unwrap_or(u64::MAX)
}

/// Load the images of drawing `i`, just opened. Those that can't be loaded are reported and
/// stay crossed-out frames; opening never fails over an image.
pub(crate) fn after_open(s: &mut Session, i: usize) {
    let Some(st) = s.docs.get_mut(i) else { return };
    let paths = paths(&st.doc);
    if paths.is_empty() {
        return;
    }
    let mut rasters = st.doc.rasters.clone();
    let notes = load_missing(&paths, st.path.as_deref(), &mut rasters);
    if !rasters.is_empty() {
        // Loading pixels is not a change to the drawing. A drawing counts as unchanged while
        // it is the very one that was saved, so the saved one follows.
        let unchanged = Arc::ptr_eq(&st.doc, &st.saved);
        Arc::make_mut(&mut st.doc).rasters = rasters;
        if unchanged {
            st.saved = st.doc.clone();
        }
    }
    for n in notes {
        s.echo(n);
    }
}

/// Read and decode into `rasters` the files of `paths` that are not in it yet, for a drawing at
/// `host`. Returns one message per file that could not be loaded.
fn load_missing(paths: &[String], host: Option<&str>, rasters: &mut cadcraft_doc::Rasters) -> Vec<String> {
    let mut notes = Vec::new();
    let mut missing: Vec<&str> = paths.iter().map(String::as_str).filter(|p| rasters.get(p).is_none()).collect();
    if missing.len() > MAX_IMAGE_FILES {
        missing.truncate(MAX_IMAGE_FILES);
        notes.push(format!("Images: only the first {MAX_IMAGE_FILES} image files are loaded"));
    }
    let decoded = decode_files(&missing, host);
    // Kept in the order the drawing uses them, up to the memory limit.
    let mut kept_bytes = 0u64;
    for (path, result) in missing.iter().zip(decoded) {
        match result {
            Ok(r) => {
                kept_bytes = kept_bytes.saturating_add(bytes_of(&r));
                if kept_bytes > MAX_LOADED_BYTES {
                    notes.push(over_limit(path));
                    break;
                }
                rasters.insert(path, Arc::new(r));
            }
            Err(e) => notes.push(format!("Image {path}: {e}")),
        }
    }
    notes
}

fn over_limit(path: &str) -> String {
    format!("Image {path}: not loaded (more than {} MB of images)", MAX_LOADED_BYTES >> 20)
}

/// Read and decode the image files `paths` of a drawing at `host`, a few at a time (decoding is
/// what opening a drawing with large images waits for). The results are in the order of `paths`.
fn decode_files(paths: &[&str], host: Option<&str>) -> Vec<std::result::Result<Raster, String>> {
    // Decoded bytes so far, shared by the loaders: past the limit no further file is started.
    let decoded_bytes = AtomicU64::new(0);
    let load = |path: &str| -> std::result::Result<Raster, String> {
        if decoded_bytes.load(Ordering::Relaxed) > MAX_LOADED_BYTES {
            return Err(format!("not loaded (more than {} MB of images)", MAX_LOADED_BYTES >> 20));
        }
        let r = read_file(path, host).and_then(|bytes| decode(&bytes))?;
        decoded_bytes.fetch_add(bytes_of(&r), Ordering::Relaxed);
        Ok(r)
    };
    let mut results: Vec<Option<std::result::Result<Raster, String>>> = paths.iter().map(|_| None).collect();
    let loaders = loader_count(paths.len());
    if loaders > 1 {
        // Each loader takes the next file nobody has started.
        let next = AtomicUsize::new(0);
        let work = || {
            let mut done = Vec::new();
            loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(path) = paths.get(index) else { break };
                done.push((index, load(path)));
            }
            done
        };
        let finished: Vec<_> = std::thread::scope(|scope| {
            let started: Vec<_> = (0..loaders).filter_map(|_| std::thread::Builder::new().spawn_scoped(scope, work).ok()).collect();
            started.into_iter().filter_map(|loader| loader.join().ok()).flatten().collect()
        });
        for (index, result) in finished {
            if let Some(slot) = results.get_mut(index) {
                *slot = Some(result);
            }
        }
    }
    // One loader, or files a loader that could not start or finish left behind.
    results.into_iter().zip(paths).map(|(result, path)| result.unwrap_or_else(|| load(path))).collect()
}

/// How many files are decoded at once: one per processor, at most [`MAX_LOADERS`] (each large
/// image needs several hundred megabytes while it is decoded) and no more than there are files.
fn loader_count(files: usize) -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    let processors = std::thread::available_parallelism().map(std::num::NonZero::get).unwrap_or(1);
    #[cfg(target_arch = "wasm32")]
    let processors = 1;
    processors.min(MAX_LOADERS).min(files)
}

/// The file name without folders and extension (either separator).
fn stem(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match file.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => file.to_string(),
    }
}

/// The definition name for a new image of `path`: the name that file already has in `d`, else
/// `wanted`, numbered (`wanted_2`, `wanted_3`, …) when another file has taken it.
fn def_name(d: &Drawing, path: &str, wanted: &str) -> String {
    if let Some(i) = images(d).find(|i| i.path == path && !i.name.is_empty()) {
        return i.name.clone();
    }
    // The names other files have, compared without regard to case.
    let taken: BTreeSet<String> = images(d).filter(|i| i.path != path).map(|i| i.name.to_ascii_lowercase()).collect();
    let free = |name: &str| !taken.contains(&name.to_ascii_lowercase());
    if free(wanted) {
        return wanted.to_string();
    }
    // One of these is free: there are more candidates than names taken.
    let candidates = 2..taken.len().saturating_add(3);
    candidates.map(|n| format!("{wanted}_{n}")).find(|name| free(name)).unwrap_or_else(|| wanted.to_string())
}

const ID: &str = "imageattach";

pub(crate) fn run_imageattach(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").map(str::trim).filter(|x| !x.is_empty()).ok_or_else(|| bad(ID, "`path` is required"))?.to_string();
    if path.len() > MAX_PATH {
        return Err(bad(ID, "`path` is too long"));
    }
    let place = placement(p)?;
    let raster = pixels(s, p, &path)?;
    let (w, h) = (f64::from(raster.width()), f64::from(raster.height()));
    // One pixel in drawing units, along the image's bottom edge (`u`) and its left edge (`v`).
    let pixel = place.width / w;
    let u = Vec2::from_angle(place.rotation) * pixel;
    let v = u.perp();
    if !(pixel.is_normal() && (pixel * h).is_normal() && u.x.is_finite() && u.y.is_finite()) {
        return Err(bad(ID, "the image would be too small or too large to draw"));
    }
    let wanted = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| stem(&path));
    let name = def_name(s.doc()?, &path, &wanted);
    let image = Image {
        insert: place.at.to3(0.0),
        u: u.to3(0.0),
        v: v.to3(0.0),
        size: Vec2::new(w, h),
        path: path.clone(),
        name: name.clone(),
        ..Image::default()
    };
    s.doc_mut()?.rasters.insert(&path, raster);
    let handle = s.add_entity(EntityKind::Image(image))?;
    Ok(json!({ "name": name, "handle": handle.hex(), "path": path, "pixels": [w, h], "size": [place.width, pixel * h] }))
}

/// Where and how large IMAGEATTACH puts the image.
struct Placement {
    /// The lower-left corner.
    at: Vec2,
    /// Radians, counter-clockwise.
    rotation: f64,
    /// The image's width in drawing units.
    width: f64,
}

/// `at`, `rotation` and the width (`width`, else `scale`: an image is one unit wide at scale 1).
fn placement(p: &Value) -> Result<Placement> {
    let at = point_param(p, "at").unwrap_or(Vec2::ZERO);
    if !(at.x.is_finite() && at.y.is_finite()) {
        return Err(bad(ID, "`at` must be a point"));
    }
    let width = match p.get("width") {
        Some(v) => v.as_f64().filter(|w| w.is_finite()).ok_or_else(|| bad(ID, "`width` must be a number"))?,
        None => f64_or(p, "scale", 1.0),
    };
    if !(width.is_finite() && width > 0.0) {
        return Err(bad(ID, "`scale` and `width` must be greater than zero"));
    }
    Ok(Placement { at, rotation: f64_or(p, "rotation", 0.0).to_radians(), width })
}

/// The pixels of the image at `path`: decoded from `data` when the caller hands the file over,
/// else the ones already loaded for that path, else read from the file.
fn pixels(s: &Session, p: &Value, path: &str) -> Result<Arc<Raster>> {
    let bytes = match p.get("data") {
        Some(data) => {
            let text = data.as_str().ok_or_else(|| bad(ID, "`data` must be base64 text"))?;
            base64_decode(text).ok_or_else(|| bad(ID, "`data` is not valid base64"))?
        }
        None => {
            if let Some(loaded) = s.doc()?.rasters.get(path) {
                return Ok(loaded.clone());
            }
            if let Some(asset) = s.doc()?.get_image_asset(path) {
                return Ok(Arc::new(decode(&asset).map_err(|e| bad(ID, format!("{path}: {e}")))?));
            }
            let host = s.state()?.path.clone();
            read_file(path, host.as_deref()).map_err(|e| bad(ID, format!("{path}: {e}")))?
        }
    };
    Ok(Arc::new(decode(&bytes).map_err(|e| bad(ID, format!("{path}: {e}")))?))
}

fn run_image(s: &mut Session, _p: &Value) -> Result<Value> {
    let host = s.state()?.path.clone();
    let d = s.doc()?;
    // Per file: its definition name and how many images show it.
    let mut uses: BTreeMap<&str, (&str, usize)> = BTreeMap::new();
    for i in images(d) {
        uses.entry(i.path.as_str()).or_insert((i.name.as_str(), 0)).1 += 1;
    }
    let rows: Vec<Value> = paths(d)
        .iter()
        .map(|path| {
            let (name, references) = uses.get(path.as_str()).copied().unwrap_or_default();
            let mut row = describe(d, path, host.as_deref());
            row["name"] = json!(name);
            row["references"] = json!(references);
            row
        })
        .collect();
    Ok(json!({ "images": rows }))
}

/// The state of image file `path` of drawing `d` (saved at `host`): where it is and whether its
/// pixels are loaded.
fn describe(d: &Drawing, path: &str, host: Option<&str>) -> Value {
    let loaded = d.rasters.get(path);
    let found_at = super::blocks::xref::find_file(path, host).map(|f| f.to_string_lossy().into_owned());
    let status = match (loaded, &found_at) {
        (Some(_), _) => "Loaded",
        (None, None) => "Not Found",
        // The file is there but could not be read as an image.
        (None, Some(_)) => "Not loaded",
    };
    json!({ "path": path, "foundAt": found_at, "status": status, "pixels": loaded.map(|r| json!([r.width(), r.height()])) })
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
        let bytes = s.doc().ok().and_then(|d| d.get_image_asset(&self.path).map(|b| b.to_vec())).or_else(|| {
            #[cfg(not(target_arch = "wasm32"))]
            {
                if let Some(f) = find_image_file(&self.path, doc_path.as_deref()) { std::fs::read(f).ok() } else { std::fs::read(&self.path).ok() }
            }
            #[cfg(target_arch = "wasm32")]
            {
                None
            }
        });
        if let Some(b) = bytes {
            if let Ok(r) = decode(&b) {
                self.size = Vec2::new(f64::from(r.width()), f64::from(r.height()));
                if let Ok(d) = s.doc_mut() {
                    d.rasters.insert(&self.path, Arc::new(r));
                    d.store_image_asset(&self.path, b);
                }
                return;
            }
            if let Ok(d) = s.doc_mut() {
                d.store_image_asset(&self.path, b);
            }
        }
        self.size = Vec2::new(100.0, 100.0);
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
                let width = (self.size.x.max(1.0) * self.scale).max(1e-6);
                run_imageattach(
                    s,
                    &json!({
                        "path": self.path,
                        "at": [self.at.x, self.at.y],
                        "width": width,
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
