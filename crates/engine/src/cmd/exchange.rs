//! Moving drawings in and out: EXPORT (DXF, DWG, SVG, PNG or PDF), IMPORT (the objects of another
//! DXF/DWG file), PREVIEW (a picture of the plot) and PUBLISH (plot several sheets at once).
//! ATTACH, SHARE and 3DORBIT are registered so the tool bar and command line say they are not
//! available yet.
//!
//! Files go through the host's io hooks ([`super::file::io`]) like OPEN and SAVEAS, so the engine
//! stays I/O-agnostic; the JSON forms never open dialogs (the UI asks for files when these
//! commands come from a menu, the tool bar or the command line).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use cadcraft_color::Rgb;
use cadcraft_doc::{Drawing, Entity, EntityKind, PageSetup, Space};
use cadcraft_geom::{Bounds2, Mat3, Vec2};
use cadcraft_render::paper::{self, Sheet};
use cadcraft_render::raster::{RasterOptions, View};
use serde_json::{Value, json};

use super::file::{base64_decode, base64_encode, io};
use super::*;
use crate::{EngineError, Result, Session};

/// Formats EXPORT writes (the io layer picks the writer from the extension).
pub const EXPORT_FORMATS: &[&str] = &["dxf", "dwg", "svg", "png", "pdf"];
/// Default and largest plot preview width, in pixels.
const PREVIEW_PX: u64 = 1200;
const MAX_PREVIEW_PX: u64 = 4096;
/// Most sheets one PUBLISH plots.
const MAX_SHEETS: usize = 256;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("import", "Import...", run_import)
            .menu(&["File", "Import..."])
            .alias(&["imp"])
            .params("{path | data: base64, name?: \"x.dwg\"} (DXF or DWG), at?: [x,y] (default 0,0), scale?: (default 1) → {imported, handles, layers, styles, blocks}"),
        CommandSpec::new("export", "Export...", run_export)
            .menu(&["File", "Export..."])
            .alias(&["exp"])
            .params("{path? (else returns base64 `data`), format?: dxf|dwg|svg|png|pdf (default: the path's extension)} → {format, bytes, path? | data}")
            .noundo(),
        CommandSpec::new("preview", "Plot Preview", run_preview)
            .menu(&["File", "Plot Preview"])
            .alias(&["pre"])
            .params("{layout?: current|\"Model\", paper?, landscape?, fit?, scale?, lineweights?, width?: pixels (default 1200)} → {data: base64 PNG, width, height, layout, paper}")
            .noundo(),
        CommandSpec::new("publish", "Batch Publish...", run_publish)
            .menu(&["File", "Batch Publish..."])
            .params("{path?: \"NAME.pdf\" (writes NAME-<sheet>.pdf per sheet; else returns base64 `data` per sheet), layouts?: [name | \"Model\"] (default: Model if it has objects, then every layout in tab order)} → {sheets: [{layout, bytes, path? | data}]}")
            .noundo(),
        CommandSpec::new("attach", "Attach", |_, _| {
            Err(EngineError::Other("ATTACH is not available yet: raster images and PDF/DWF/DGN underlays can't be shown yet".into()))
        })
        .params("not available yet")
        .noundo(),
        CommandSpec::new("share", "Share", |_, _| {
            Err(EngineError::Other("SHARE is not available yet: there is no online service to share through; EXPORT or PUBLISH the drawing and send the file".into()))
        })
        .params("not available yet")
        .noundo(),
        CommandSpec::new("3dorbit", "Orbit", |_, _| Err(EngineError::Other("3DORBIT is not available yet: CADCraft draws in 2D (plan view) only".into())))
            .alias(&["3do", "orbit"])
            .params("not available yet")
            .noundo(),
    ]
}

fn extension(path: &str) -> String {
    std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// Write `bytes` to `path` through a temporary file, so a failed write keeps the old file.
fn write_file(cmd: &str, path: &str, bytes: &[u8]) -> Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let tmp = format!("{path}.cadcraft-tmp");
        std::fs::write(&tmp, bytes).map_err(|e| bad(cmd, format!("{path}: {e}")))?;
        std::fs::rename(&tmp, path).map_err(|e| bad(cmd, format!("{path}: {e}")))?;
        Ok(())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (path, bytes);
        Err(bad(cmd, "paths are not available on the web; omit `path` to get the file as base64 `data`"))
    }
}

// ---------- EXPORT ----------

fn run_export(s: &mut Session, p: &Value) -> Result<Value> {
    let hooks = io().ok_or_else(|| bad("export", "file formats are not available in this build"))?;
    let path = str_param(p, "path");
    let format = match str_param(p, "format") {
        Some(f) => f.trim().trim_start_matches('.').to_ascii_lowercase(),
        None => path.map(extension).unwrap_or_default(),
    };
    if !EXPORT_FORMATS.contains(&format.as_str()) {
        return Err(bad("export", "`format` must be dxf, dwg, svg, png or pdf (or give a `path` with one of those extensions)"));
    }
    let bytes = (hooks.write)(s.doc()?, &format!("export.{format}")).map_err(|e| bad("export", e))?;
    match path {
        Some(path) => {
            write_file("export", path, &bytes)?;
            Ok(json!({ "path": path, "format": format, "bytes": bytes.len(), "message": format!("Exported to {path}") }))
        }
        None => Ok(json!({ "data": base64_encode(&bytes), "format": format, "bytes": bytes.len() })),
    }
}

// ---------- IMPORT ----------

fn run_import(s: &mut Session, p: &Value) -> Result<Value> {
    let hooks = io().ok_or_else(|| bad("import", "file formats are not available in this build"))?;
    let at = match p.get("at") {
        Some(v) => point_value(v).ok_or_else(|| bad("import", "`at` must be a point [x, y]"))?,
        None => Vec2::ZERO,
    };
    let scale = match p.get("scale") {
        Some(v) => v
            .as_f64()
            .filter(|k| k.is_finite() && (1e-9..=1e9).contains(&k.abs()))
            .ok_or_else(|| bad("import", "`scale` must be a non-zero number"))?,
        None => 1.0,
    };
    let (bytes, name) = if let Some(path) = str_param(p, "path") {
        (read_file(path)?, path.to_string())
    } else {
        let data = str_param(p, "data").ok_or_else(|| bad("import", "`path` or `data` (base64) is required"))?;
        (base64_decode(data).ok_or_else(|| bad("import", "invalid base64"))?, str_param(p, "name").unwrap_or("import.dxf").to_string())
    };
    let src = (hooks.read)(&bytes, &name).map_err(|e| bad("import", e))?;
    let file = std::path::Path::new(&name).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    import_drawing(s, &src, at, scale, &file)
}

/// Copy `src`'s model space into the current space (scaled about the origin, then moved to
/// `at`), with the layers, linetypes, styles and blocks it needs.
fn import_drawing(s: &mut Session, src: &Drawing, at: Vec2, scale: f64, file: &str) -> Result<Value> {
    let m = Mat3::scale(scale, scale).then(Mat3::translate(at));
    let moved = at != Vec2::ZERO || scale != 1.0;
    let space = s.space();
    let d = s.doc_mut()?;
    let (layers, styles) = merge_tables(d, src);
    let renames = block_names(d, src);
    for (from, to) in &renames {
        let Some(b) = src.blocks.get(from) else { continue };
        let mut nb = (**b).clone();
        nb.name = to.clone();
        nb.entities = Default::default();
        for e in b.entities.iter() {
            let e = adopt(d, e, &renames);
            nb.entities.push(e);
        }
        d.blocks.insert(to.clone(), Arc::new(nb));
    }
    let mut handles = Vec::new();
    for e in src.model.iter() {
        let mut e = adopt(d, e, &renames);
        if moved {
            e.kind.transform(&m);
        }
        d.ensure_layer(&e.common.layer);
        handles.push(e.handle);
        if let Some(st) = d.space_mut(&space) {
            st.push(e);
        }
    }
    s.set_selection(handles.clone());
    Ok(json!({
        "imported": handles.len(),
        "handles": handles.iter().map(|h| h.hex()).collect::<Vec<_>>(),
        "layers": layers,
        "styles": styles,
        "blocks": renames.len(),
        "message": format!("{} objects imported from {file}", handles.len()),
    }))
}

fn read_file(_path: &str) -> Result<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::read(_path).map_err(|e| bad("import", format!("{_path}: {e}")))
    }
    #[cfg(target_arch = "wasm32")]
    Err(bad("import", "paths are not available on the web; pass `data`"))
}

/// Add the source's layers, linetypes and styles that this drawing doesn't have (a name that
/// exists keeps this drawing's definition). Returns how many layers and how many other records were added.
fn merge_tables(d: &mut Drawing, src: &Drawing) -> (usize, usize) {
    fn merge<T: Clone>(ours: &mut Vec<T>, theirs: &[T], name: fn(&T) -> &str) -> usize {
        let mut n = 0;
        for t in theirs {
            if !ours.iter().any(|o| name(o).eq_ignore_ascii_case(name(t))) {
                ours.push(t.clone());
                n += 1;
            }
        }
        n
    }
    let layers = merge(&mut d.layers, &src.layers, |l| &l.name);
    let others = merge(&mut d.linetypes, &src.linetypes, |l| &l.name)
        + merge(&mut d.text_styles, &src.text_styles, |t| &t.name)
        + merge(&mut d.dim_styles, &src.dim_styles, |t| &t.name)
        + merge(&mut d.mleader_styles, &src.mleader_styles, |t| &t.name)
        + merge(&mut d.table_styles, &src.table_styles, |t| &t.name);
    (layers, others)
}

/// Which source blocks to copy, and under which name: new names keep theirs, generated
/// (anonymous `*…`) blocks that collide get a fresh generated name, and a named block this drawing
/// already has keeps this drawing's definition (not copied).
fn block_names(d: &Drawing, src: &Drawing) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut taken: BTreeSet<String> = d.blocks.keys().map(|k| k.to_ascii_uppercase()).collect();
    for (name, b) in &src.blocks {
        let upper = name.to_ascii_uppercase();
        if upper.starts_with("*MODEL_SPACE") || upper.starts_with("*PAPER_SPACE") {
            continue;
        }
        let anonymous = b.anonymous || name.starts_with('*');
        let to = if !taken.contains(&upper) {
            name.clone()
        } else if anonymous {
            let prefix: String = name.chars().take_while(|c| !c.is_ascii_digit()).collect();
            match (1..=1_000_000).map(|i| format!("{prefix}{i}")).find(|n| !taken.contains(&n.to_ascii_uppercase())) {
                Some(n) => n,
                None => continue,
            }
        } else {
            continue;
        };
        taken.insert(to.to_ascii_uppercase());
        out.insert(name.clone(), to);
    }
    out
}

/// A copy of a source entity for this drawing: a fresh handle, block references renamed, and
/// links to other source objects (associative dimension points) dropped.
fn adopt(d: &mut Drawing, e: &Entity, renames: &BTreeMap<String, String>) -> Entity {
    let mut e = e.clone();
    e.handle = d.new_handle();
    match &mut e.kind {
        EntityKind::Insert(i) => {
            if let Some(n) = renames.get(&i.block) {
                i.block = n.clone();
            }
        }
        EntityKind::Dimension(dm) => {
            dm.assoc.clear();
            if let Some(n) = dm.block.as_ref().and_then(|b| renames.get(b)) {
                dm.block = Some(n.clone());
            }
        }
        _ => {}
    }
    e
}

// ---------- PREVIEW ----------

/// The space a plot command works on: `layout` ("Model" or a layout name) or the current one.
fn plot_space(cmd: &str, s: &Session, p: &Value) -> Result<Space> {
    Ok(match str_param(p, "layout") {
        Some(n) if n.eq_ignore_ascii_case("model") => Space::Model,
        Some(n) => Space::Paper(s.doc()?.layout(n).map(|l| l.name.clone()).ok_or_else(|| bad(cmd, format!("no layout `{n}`")))?),
        None => s.space(),
    })
}

/// Paper units per drawing unit, as the PDF plotter computes it.
fn plot_scale(b: &Bounds2, sheet: &Sheet, fit: bool, scale: Option<f64>) -> f64 {
    if let Some(s) = scale
        && !fit
    {
        return s;
    }
    if !fit || b.is_empty() {
        return 1.0;
    }
    let s = (sheet.printable.width() / b.width().max(1e-12)).min(sheet.printable.height() / b.height().max(1e-12));
    if s.is_finite() && s > 0.0 { s } else { 1.0 }
}

fn run_preview(s: &mut Session, p: &Value) -> Result<Value> {
    let space = plot_space("preview", s, p)?;
    let d = s.doc()?;
    let mut page = match &space {
        Space::Paper(n) => d.layout(n).map(|l| l.page.clone()).unwrap_or_default(),
        Space::Model => {
            // Model space plots on A4 (metric) or ANSI A, like the PDF plotter.
            let mut page = PageSetup::default();
            if paper::paper_unit_mm(d) == 1.0
                && let Some(a4) = paper::paper_size("A4")
            {
                page.paper = a4.name.into();
                page.width_mm = a4.width_mm;
                page.height_mm = a4.height_mm;
            }
            page
        }
    };
    if let Some(name) = str_param(p, "paper") {
        let ps = paper::paper_size(name).ok_or_else(|| bad("preview", format!("unknown paper size `{name}`")))?;
        page.paper = ps.name.into();
        page.width_mm = ps.width_mm;
        page.height_mm = ps.height_mm;
    }
    if let Some(l) = p.get("landscape").and_then(Value::as_bool) {
        page.landscape = l;
    }
    let sheet = Sheet::from_page(&page, paper::paper_unit_mm(d));
    let lineweights = p.get("lineweights").and_then(Value::as_bool).unwrap_or(match space {
        Space::Paper(_) => page.lineweights,
        Space::Model => true,
    });
    let fit = p.get("fit").and_then(Value::as_bool).unwrap_or(matches!(space, Space::Model));
    let scale = p.get("scale").and_then(Value::as_f64).filter(|v| v.is_finite() && *v > 0.0);
    let width = p.get("width").and_then(Value::as_u64).unwrap_or(PREVIEW_PX).clamp(16, MAX_PREVIEW_PX);
    let height = ((width as f64) * sheet.size.y / sheet.size.x).round();
    let height = if height.is_finite() { (height as u64).clamp(16, MAX_PREVIEW_PX) } else { width };
    let px_per_paper_unit = width as f64 / sheet.size.x;
    // Curves are flattened to about half a pixel.
    let est = plot_scale(&d.extents(&space), &sheet, fit, scale);
    let tol = 0.5 / (px_per_paper_unit * est);
    let ropts = cadcraft_render::Options {
        tolerance: if tol.is_finite() && tol > 0.0 { tol } else { 0.01 },
        min_dash: 0.0,
        text: true,
        fill: true,
        lineweights,
    };
    let list = cadcraft_render::build_plot(d, &space, &ropts);
    let b = list.bounds;
    // Paper point q shows drawing point (q - to) / k + from.
    let (from, k, to) = if fit || space == Space::Model {
        (if b.is_empty() { Vec2::ZERO } else { b.center() }, plot_scale(&b, &sheet, fit, scale), sheet.printable.center())
    } else {
        (Vec2::ZERO, 1.0, Vec2::ZERO)
    };
    let world = Bounds2::new((Vec2::ZERO - to) / k + from, (sheet.size - to) / k + from);
    if ![world.min.x, world.min.y, world.max.x, world.max.y].iter().all(|v| v.is_finite()) {
        return Err(bad("preview", "the drawing is too large to preview at this scale"));
    }
    let (w, h) = (u32::try_from(width).unwrap_or(1), u32::try_from(height).unwrap_or(1));
    let view = View::fit(&world, w, h, 0.0);
    let px_per_mm = (px_per_paper_unit / sheet.unit_mm) as f32;
    let ro = RasterOptions {
        background: Rgb(255, 255, 255),
        hairline: 1.0,
        px_per_mm: if lineweights && px_per_mm.is_finite() { px_per_mm } else { 0.0 },
        antialias: true,
    };
    let png = cadcraft_render::raster::render_png(&list, &view, &ro).ok_or_else(|| bad("preview", "rendering the preview failed"))?;
    let layout = match &space {
        Space::Model => "Model".to_string(),
        Space::Paper(n) => n.clone(),
    };
    Ok(json!({ "data": base64_encode(&png), "width": w, "height": h, "layout": layout, "paper": page.paper, "landscape": page.landscape }))
}

// ---------- PUBLISH ----------

/// A layout name made safe for a file name.
fn file_part(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') { c } else { '_' }).collect()
}

fn run_publish(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let sheets: Vec<String> = match p.get("layouts") {
        Some(v) => {
            let a = v.as_array().ok_or_else(|| bad("publish", "`layouts` must be a list of layout names"))?;
            a.iter()
                .map(|n| {
                    let n = n.as_str().ok_or_else(|| bad("publish", "`layouts` must be a list of layout names"))?;
                    if n.eq_ignore_ascii_case("model") {
                        return Ok("Model".to_string());
                    }
                    d.layout(n).map(|l| l.name.clone()).ok_or_else(|| bad("publish", format!("no layout `{n}`")))
                })
                .collect::<Result<_>>()?
        }
        None => {
            let mut layouts: Vec<_> = d.layouts.iter().collect();
            layouts.sort_by_key(|l| l.tab_order);
            let model = (!d.model.is_empty()).then(|| "Model".to_string());
            model.into_iter().chain(layouts.iter().map(|l| l.name.clone())).collect()
        }
    };
    if sheets.is_empty() {
        return Err(bad("publish", "there are no sheets to publish"));
    }
    if sheets.len() > MAX_SHEETS {
        return Err(bad("publish", format!("at most {MAX_SHEETS} sheets can be published at once")));
    }
    let base = str_param(p, "path").map(|path| {
        let pb = std::path::Path::new(path);
        let stem = pb.file_stem().map(|s| s.to_string_lossy().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "Drawing".into());
        (pb.parent().map(std::path::Path::to_path_buf).unwrap_or_default(), stem)
    });
    let mut out = Vec::new();
    for name in &sheets {
        let mut q = json!({ "layout": name });
        if let Some((dir, stem)) = &base {
            q["path"] = json!(dir.join(format!("{stem}-{}.pdf", file_part(name))).to_string_lossy());
        }
        let mut r = s.execute("plot", &q).map_err(|e| bad("publish", format!("{name}: {e}")))?;
        if let Some(o) = r.as_object_mut() {
            o.insert("layout".into(), json!(name));
        }
        out.push(r);
    }
    let message = match &base {
        Some(_) => format!("Published {} sheet(s): {}", out.len(), out.iter().filter_map(|r| r["path"].as_str()).collect::<Vec<_>>().join(", ")),
        None => format!("Published {} sheet(s)", out.len()),
    };
    Ok(json!({ "sheets": out, "message": message }))
}

#[cfg(test)]
mod tests {
    use cadcraft_doc::{Block, Circle, Common, Handle, Insert, Layer, Line};
    use cadcraft_geom::Vec3;

    use super::super::file::{IoHooks, set_io};
    use super::*;

    fn fake_plot(_: &Drawing, space: &Space, opts: &Value) -> std::result::Result<Vec<u8>, String> {
        // Same output as the layout tests' stand-in: whichever test installs first wins.
        Ok(format!("%PDF-1.4 fake {space:?} {opts}").into_bytes())
    }

    /// Install stand-in io hooks; true when they are the ones installed (the hooks are
    /// process-wide, and other tests install writers that always fail).
    fn our_hooks() -> bool {
        set_io(IoHooks {
            read: |_, _| Err("no reader in tests".into()),
            write: |d, name| Ok(if name == "probe.test" { b"exchange".to_vec() } else { format!("{name}:{}", d.model.len()).into_bytes() }),
            plot: Some(fake_plot),
        });
        io().is_some_and(|h| (h.write)(&Drawing::new_imperial(), "probe.test").is_ok_and(|b| b == b"exchange"))
    }

    fn session() -> Session {
        let mut s = Session::new();
        s.cmdline("line 0,0 100,0").unwrap();
        s.cmdline("").unwrap();
        s.cmdline("circle 50,25 20").unwrap();
        s
    }

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("cadcraft-exchange-{}-{name}", std::process::id()))
    }

    #[test]
    fn export_writes_the_chosen_format_and_is_not_save_as() {
        let ours = our_hooks();
        let mut s = session();
        assert!(s.execute("export", &json!({})).is_err(), "a format or a path is required");
        assert!(s.execute("export", &json!({ "path": "x.txt" })).is_err());
        assert!(s.execute("export", &json!({ "format": "exe" })).is_err());
        if !ours {
            return;
        }
        let r = s.execute("export", &json!({ "format": ".SVG" })).unwrap();
        assert_eq!(r["format"], "svg");
        assert_eq!(base64_decode(r["data"].as_str().unwrap()).unwrap(), b"export.svg:2");
        let path = temp("out.png");
        let ps = path.to_string_lossy().to_string();
        let r = s.execute("export", &json!({ "path": ps })).unwrap();
        assert_eq!(r["format"], "png");
        assert_eq!(std::fs::read(&path).unwrap(), b"export.png:2");
        let _ = std::fs::remove_file(&path);
        let st = s.state().unwrap();
        assert!(st.path.is_none() && st.title == "Drawing1", "the drawing keeps its name");
    }

    fn line(a: (f64, f64), b: (f64, f64)) -> EntityKind {
        EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) })
    }

    fn block(name: &str, r: f64) -> Arc<Block> {
        let mut b = Block::new(name);
        b.entities.push(Entity::new(Handle(0x100), EntityKind::Circle(Circle { center: Vec3::ZERO, radius: r })));
        Arc::new(b)
    }

    fn insert(name: &str) -> EntityKind {
        EntityKind::Insert(Insert {
            block: name.into(),
            insert: Vec3::ZERO,
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        })
    }

    #[test]
    fn import_brings_objects_with_their_layers_and_blocks() {
        let mut src = Drawing::new_imperial();
        src.layers.push(Layer::new("WALLS"));
        src.blocks.insert("PART".into(), block("PART", 1.0));
        src.blocks.insert("NEW".into(), block("NEW", 2.0));
        src.blocks.insert("*U1".into(), block("*U1", 3.0));
        let walls = Common { layer: "WALLS".into(), ..Common::default() };
        src.add(&Space::Model, walls, line((0.0, 0.0), (10.0, 0.0))).unwrap();
        for b in ["PART", "NEW", "*U1"] {
            src.add(&Space::Model, Common::default(), insert(b)).unwrap();
        }

        let mut s = session();
        s.doc_mut().unwrap().blocks.insert("PART".into(), block("PART", 5.0));
        s.doc_mut().unwrap().blocks.insert("*U1".into(), block("*U1", 6.0));
        let r = import_drawing(&mut s, &src, Vec2::new(100.0, 0.0), 2.0, "parts.dxf").unwrap();
        assert_eq!(r["imported"], 4);
        assert_eq!(r["message"], "4 objects imported from parts.dxf");
        let d = s.doc().unwrap();
        assert_eq!(d.model.len(), 6);
        assert!(d.layer("WALLS").is_some());
        let mut handles: Vec<Handle> = d.model.handles();
        handles.sort();
        handles.dedup();
        assert_eq!(handles.len(), 6, "imported objects get handles of their own");
        let imported: Vec<&Entity> = d.model.iter().skip(2).map(|e| &**e).collect();
        match &imported[0].kind {
            EntityKind::Line(l) => {
                assert_eq!((l.a.x, l.b.x), (100.0, 120.0), "scaled about the origin, then moved");
                assert_eq!(imported[0].common.layer, "WALLS");
            }
            k => panic!("expected the line, got {k:?}"),
        }
        let names: Vec<String> =
            imported.iter().filter_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.block.clone()) } else { None }).collect();
        assert_eq!(names, ["PART", "NEW", "*U2"], "same-named blocks keep ours; generated ones get a new name");
        let radius = |n: &str| match d.block(n).and_then(|b| b.entities.iter().next().map(|e| e.kind.clone())) {
            Some(EntityKind::Circle(c)) => c.radius,
            _ => 0.0,
        };
        assert_eq!((radius("PART"), radius("NEW"), radius("*U1"), radius("*U2")), (5.0, 2.0, 6.0, 3.0));
        assert_eq!(s.selection().len(), 4);
        s.execute("undo", &json!({})).ok();

        // The JSON form checks its parameters before reading anything.
        our_hooks();
        for p in [json!({}), json!({ "data": "%%" }), json!({ "data": "", "scale": 0 }), json!({ "data": "", "at": "x" })] {
            assert!(s.execute("import", &p).is_err(), "{p}");
        }
    }

    #[test]
    fn preview_draws_the_sheet_as_it_plots() {
        let mut s = session();
        let r = s.execute("preview", &json!({ "width": 400 })).unwrap();
        assert_eq!((r["layout"].as_str(), r["width"].as_u64()), (Some("Model"), Some(400)));
        // Model space plots on ANSI A landscape in an imperial drawing.
        assert_eq!(r["height"], 309);
        let png = base64_decode(r["data"].as_str().unwrap()).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        assert_eq!(png.get(16..24), Some(&[0, 0, 1, 144, 0, 0, 1, 53][..]), "IHDR is 400 x 309");
        let r = s.execute("preview", &json!({ "layout": "layout1", "paper": "A4", "landscape": true, "width": 600 })).unwrap();
        assert_eq!((r["layout"].as_str(), r["width"].as_u64(), r["height"].as_u64()), (Some("Layout1"), Some(600), Some(424)));
        assert_eq!(r["paper"], "ISO A4 (210.00 x 297.00 MM)");
        assert!(s.execute("preview", &json!({ "layout": "Nope" })).is_err());
        assert!(s.execute("preview", &json!({ "paper": "Nope" })).is_err());
        for p in [json!({ "scale": -1, "fit": false }), json!({ "scale": 1e-300, "fit": false }), json!({ "width": -5 }), json!({ "width": 1e30 })] {
            let _ = s.execute("preview", &p);
        }
    }

    #[test]
    fn publish_plots_every_sheet() {
        our_hooks();
        let mut s = session();
        if io().and_then(|h| h.plot).is_none() {
            return;
        }
        let r = s.execute("publish", &json!({})).unwrap();
        let sheets: Vec<&str> = r["sheets"].as_array().unwrap().iter().filter_map(|x| x["layout"].as_str()).collect();
        assert_eq!(sheets, ["Model", "Layout1", "Layout2"]);
        let base = temp("set.pdf");
        let r = s.execute("publish", &json!({ "path": base.to_string_lossy(), "layouts": ["layout2", "MODEL"] })).unwrap();
        for (sheet, name) in r["sheets"].as_array().unwrap().iter().zip(["Layout2", "Model"]) {
            let path = temp(&format!("set-{name}.pdf"));
            assert_eq!(sheet["path"].as_str(), Some(path.to_string_lossy().as_ref()));
            let text = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
            assert!(text.starts_with("%PDF") && text.contains(if name == "Model" { "Model" } else { "Paper(\"Layout2\")" }));
            let _ = std::fs::remove_file(&path);
        }
        assert!(r["message"].as_str().unwrap().starts_with("Published 2 sheet(s)"));
        for p in [json!({ "layouts": [] }), json!({ "layouts": ["Nope"] }), json!({ "layouts": "Layout1" }), json!({ "layouts": [1] })] {
            assert!(s.execute("publish", &p).is_err(), "{p}");
        }
    }

    #[test]
    fn attach_share_and_orbit_say_they_are_not_available_yet() {
        let mut s = session();
        for c in ["attach", "share", "3dorbit", "orbit"] {
            let e = s.execute(&resolve_alias(c), &Value::Null).unwrap_err().to_string();
            assert!(e.contains("not available yet"), "{c}: {e}");
        }
    }
}
