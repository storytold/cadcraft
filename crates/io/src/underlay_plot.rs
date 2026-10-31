//! PDF underlays on plots: each underlay's page drawn to pixels at the plot's resolution, its
//! paper transparent, with its contrast and monochrome, faded toward the white paper (colours are never adjusted
//! for a dark background on paper).
//!
//! A path stored in a drawing goes through `craft_docpath` (a relative path from the drawing's
//! folder, given as the plot option `document`; other computers refused), then the same file
//! name beside the drawing.

use cadcraft_doc::Drawing;
use cadcraft_geom::{Bounds2, Vec2};
use cadcraft_render::DisplayList;

use crate::pdf_read::{MAX_SIDE, PdfFile};

/// Plot resolution of an underlay's page, in pixels per inch on paper.
pub const DPI: f64 = 200.0;
/// The longest side, in pixels, a page is plotted at.
const MAX_PLOT_SIDE: u32 = 6000;
/// The largest PDF file read for a plot.
const MAX_PDF_BYTES: u64 = 512 * 1024 * 1024;
/// Most underlays drawn on one sheet.
const MAX_UNDERLAYS: usize = 32;

/// One underlay's page, ready to place: the primitive of the plot's display list it is drawn
/// before, the lower-left corner and width/height vectors in the list's units of the part of the
/// page drawn, its RGB pixels and their alpha (rows top to bottom), and the triangles it is
/// clipped to (a viewport's; empty when it isn't clipped).
pub(crate) struct PlotImage {
    pub at: usize,
    pub origin: Vec2,
    pub u: Vec2,
    pub v: Vec2,
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    pub alpha: Vec<u8>,
    pub clip: Vec<Vec2>,
}

/// The pages of the underlays plotted with `list` (built for plotting, so only visible ones on
/// plotted layers, model space's seen through a layout's viewports included), drawn at
/// `px_per_unit` pixels per list unit on paper; only the part each shows is drawn. Underlays
/// whose file can't be read are left out.
pub(crate) fn images(d: &Drawing, list: &DisplayList, document: Option<&str>, px_per_unit: f64) -> Vec<PlotImage> {
    let mut out = Vec::new();
    for lu in list.underlays.iter().take(MAX_UNDERLAYS) {
        let Some(u) = cadcraft_render::pdf_underlay(d, lu.handle) else { continue };
        let Some(region @ [l, b, r, t]) = lu.shown() else { continue };
        let (au, av) = (lu.u * (r - l), lu.v * (t - b));
        let (w, h) = (au.len() * px_per_unit, av.len() * px_per_unit);
        if !(w.is_finite() && h.is_finite() && w >= 1.0 && h >= 1.0) {
            continue;
        }
        let k = (f64::from(MAX_PLOT_SIDE.min(MAX_SIDE)) / w.max(h)).min(1.0);
        let (pw, ph) = (((w * k).round() as u32).max(1), ((h * k).round() as u32).max(1));
        let Some(bytes) = read(&u.path, document) else { continue };
        // The PDF's layers this underlay hides.
        let Ok(bytes) = crate::pdf_layers::with_layers(&bytes, &u.layer_visibility()) else { continue };
        let Ok(mut page) = PdfFile::open(&bytes).and_then(|f| f.render_region(u.page as usize, region, pw, ph)) else { continue };
        u.tone(&mut page.rgba);
        let fade = f64::from(u.fade.min(cadcraft_doc::UNDERLAY_MAX_FADE)) / 100.0;
        let px = page.rgba.as_chunks::<4>().0;
        let rgb: Vec<u8> = px
            .iter()
            .flat_map(|[r, g, b, _]| [*r, *g, *b].map(|c| (f64::from(c) + (255.0 - f64::from(c)) * fade).round().clamp(0.0, 255.0) as u8))
            .collect();
        let alpha: Vec<u8> = px.iter().map(|[.., a]| *a).collect();
        let clip = if lu.is_clipped() { lu.triangles() } else { Vec::new() };
        out.push(PlotImage {
            at: lu.at as usize,
            origin: lu.origin + lu.u * l + lu.v * b,
            u: au,
            v: av,
            width: page.width,
            height: page.height,
            rgb,
            alpha,
            clip,
        });
    }
    out
}

/// `list`'s extents with the part of each underlay's page it shows: a page is drawn as an
/// image, not by the list's primitives, so with its frame off (PDFFRAME 0) nothing else marks
/// where it is, and a plot of the extents would be fitted without it.
pub(crate) fn plotted_bounds(list: &DisplayList) -> Bounds2 {
    list.underlays.iter().take(MAX_UNDERLAYS).fold(list.bounds, |b, lu| match lu.shown() {
        Some([l, bt, r, t]) => {
            let at = |x: f64, y: f64| lu.origin + lu.u * x + lu.v * y;
            b.union(&Bounds2::from_points([at(l, bt), at(r, bt), at(r, t), at(l, t)]))
        }
        None => b,
    })
}

/// The bytes of the PDF `stored` in the drawing saved at `document`.
fn read(stored: &str, document: Option<&str>) -> Option<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Read;
        let host = document.map(std::path::Path::new);
        let found = craft_docpath::resolve_absolute(stored.trim(), host).ok().filter(|p| p.is_file()).or_else(|| {
            let name = stored.trim().rsplit(['/', '\\']).next().filter(|n| !n.is_empty())?;
            Some(host?.parent()?.join(name)).filter(|p| p.is_file())
        })?;
        let mut bytes = Vec::new();
        std::fs::File::open(found).ok()?.take(MAX_PDF_BYTES).read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (stored, document, MAX_PDF_BYTES);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{Common, EntityKind, Line, Lineweight, PdfUnderlay, Space};
    use cadcraft_geom::Vec3;
    use serde_json::json;

    /// A drawing whose only object is an underlay of a PDF (written by CADCraft's own PDF export:
    /// a thick diagonal line on A4) stored at `path`.
    fn drawing_with_underlay(path: &std::path::Path, write_pdf: bool) -> Drawing {
        if write_pdf {
            let mut src = Drawing::new_metric();
            let lw = Common { lineweight: Lineweight::Mm100(200), ..Common::default() };
            src.add(&Space::Model, lw, EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(100.0, 70.0, 0.0) })).ok();
            let o = crate::PdfOptions { paper_mm: Some((297.0, 210.0)), fit: Some(true), lineweights: Some(true), ..crate::PdfOptions::default() };
            std::fs::write(path, crate::pdf(&src, &Space::Model, &o).unwrap()).unwrap();
        }
        let mut d = Drawing::new_metric();
        let u = PdfUnderlay { size: Vec2::new(297.0, 210.0), path: path.to_string_lossy().into(), fade: 0, ..PdfUnderlay::default() };
        d.add(&Space::Model, Common::default(), EntityKind::PdfUnderlay(u)).ok();
        d
    }

    /// A plotted PDF's text with its spacing made one space (the writer's line breaks vary).
    fn flat(pdf: &[u8]) -> String {
        String::from_utf8_lossy(pdf).split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn dark_pixels(png: &[u8]) -> usize {
        let img = tiny_skia::Pixmap::decode_png(png).unwrap();
        img.pixels().iter().filter(|p| p.red() < 100).count()
    }

    /// Dark pixels of a plot, PDF (read back) or PNG.
    fn plot_dark(d: &Drawing, opts: serde_json::Value) -> usize {
        let out = crate::plotter::plot(d, &Space::Model, &opts).unwrap();
        if opts.get("output").is_some() {
            return dark_pixels(&out);
        }
        let r = crate::pdf_read::PdfFile::open(&out).unwrap().render(1, 600).unwrap();
        r.rgba.as_chunks::<4>().0.iter().filter(|c| c[0] < 100 && c[3] > 128).count()
    }

    #[test]
    fn a_plot_of_the_extents_fits_the_pages_with_their_frames_off() {
        let dir = std::env::temp_dir().join(format!("cadcraft-underlay-plot-noframe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut d = drawing_with_underlay(&dir.join("Plan.pdf"), true);
        // A short line in a corner of the page: with the frame off, the only other thing drawn.
        d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(280.0, 190.0, 0.0), b: Vec3::new(285.0, 195.0, 0.0) })).ok();
        for opts in [json!({"paper": "A4", "landscape": true}), json!({"output": "png", "paper": "A4", "landscape": true, "resolution": 100})] {
            d.header.set_i64("PDFFRAME", 1);
            let framed = plot_dark(&d, opts.clone());
            d.header.set_i64("PDFFRAME", 0);
            let unframed = plot_dark(&d, opts.clone());
            // The frame is a few hundred pixels; the page's diagonal line is fitted the same.
            assert!(unframed * 2 > framed && framed * 2 > unframed, "{opts}: {unframed} dark pixels without the frame, {framed} with it");
        }
        // An SVG export of the extents shows the whole page too, not the line alone.
        let svg = crate::svg::export(&d, &Space::Model);
        let view: Vec<f64> =
            svg.split("viewBox=\"").nth(1).and_then(|v| v.split('"').next()).unwrap().split_whitespace().map(|x| x.parse().unwrap()).collect();
        assert!(view.get(2).is_some_and(|w| *w >= 297.0), "{view:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plots_show_the_underlay_page() {
        let dir = std::env::temp_dir().join(format!("cadcraft-underlay-plot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pdf = dir.join("Plan.pdf");
        let d = drawing_with_underlay(&pdf, true);

        let out = crate::plotter::plot(&d, &Space::Model, &json!({"paper": "A4", "landscape": true, "compress": false})).unwrap();
        let text = flat(&out);
        assert!(text.contains("/Subtype/Image") && text.contains("/x0 Do"), "the page is placed as an image");
        assert!(text.contains("/SMask"), "its paper is transparent");
        // Upright: the page's line starts at its lower-left corner (its frame is on every side; the
        // plot's paper is transparent when read back).
        let r = crate::pdf_read::PdfFile::open(&out).unwrap().render(1, 400).unwrap();
        let dark = |x0: u32, y0: u32| {
            let (w, h) = (r.width / 2, r.height / 2);
            (y0 * h + 8..(y0 + 1) * h - 8)
                .flat_map(|y| (x0 * w + 8..(x0 + 1) * w - 8).map(move |x| (x, y)))
                .filter(|(x, y)| {
                    let i = ((y * r.width + x) * 4) as usize;
                    r.rgba.get(i..i + 4).is_some_and(|c| c[0] < 100 && c[3] > 128)
                })
                .count()
        };
        assert!(dark(0, 1) > 3 * dark(0, 0), "lower left {} vs upper left {}", dark(0, 1), dark(0, 0));

        let png = crate::plotter::plot(&d, &Space::Model, &json!({"output": "png", "paper": "A4", "landscape": true, "resolution": 100})).unwrap();
        let with = dark_pixels(&png);
        // Without its PDF only the frame is drawn.
        std::fs::remove_file(&pdf).unwrap();
        let png = crate::plotter::plot(&d, &Space::Model, &json!({"output": "png", "paper": "A4", "landscape": true, "resolution": 100})).unwrap();
        let without = dark_pixels(&png);
        assert!(with > without + 500, "{with} vs {without}");
        let out = crate::plotter::plot(&d, &Space::Model, &json!({})).unwrap();
        assert!(!flat(&out).contains("/Subtype/Image"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The PNG pictures of an SVG (their base64 data URLs decoded) and each one's matrix.
    fn svg_images(svg: &str) -> Vec<(Vec<u8>, Vec<f64>)> {
        let decode = |s: &str| -> Vec<u8> {
            let val = |c: u8| b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/".iter().position(|x| *x == c).map(|i| i as u32);
            let mut out = Vec::new();
            for q in s.as_bytes().chunks(4) {
                let v: Vec<Option<u32>> = q.iter().map(|c| val(*c)).collect();
                let n = v.iter().take(4).enumerate().fold(0u32, |n, (i, x)| n | (x.unwrap_or(0) << (18 - 6 * i)));
                let keep = v.iter().filter(|x| x.is_some()).count().saturating_sub(1);
                out.extend(n.to_be_bytes().iter().skip(1).take(keep));
            }
            out
        };
        svg.split("<image ")
            .skip(1)
            .map(|rest| {
                let m = rest.split("matrix(").nth(1).and_then(|r| r.split(')').next()).unwrap_or("");
                let m: Vec<f64> = m.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                let data = rest.split("base64,").nth(1).and_then(|r| r.split('"').next()).unwrap_or("");
                (decode(data), m)
            })
            .collect()
    }

    #[test]
    fn svg_exports_and_plots_show_the_underlay_page_upright() {
        let dir = std::env::temp_dir().join(format!("cadcraft-underlay-svg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pdf = dir.join("Plan.pdf");
        let d = drawing_with_underlay(&pdf, true);
        let plotted =
            String::from_utf8(crate::plotter::plot(&d, &Space::Model, &json!({"output": "svg", "paper": "A4", "landscape": true})).unwrap()).unwrap();
        let exported = crate::svg::export(&d, &Space::Model);
        for svg in [plotted, exported] {
            let images = svg_images(&svg);
            assert_eq!(images.len(), 1, "the page is one picture");
            let (png, m) = &images[0];
            // Not turned, the right way up: x to the right, rows downwards.
            assert!(m.len() == 6 && m[0] > 0.0 && m[3] > 0.0 && m[1].abs() < 1e-9 && m[2].abs() < 1e-9, "{m:?}");
            let img = tiny_skia::Pixmap::decode_png(png).unwrap();
            let (w, h) = (img.width(), img.height());
            let dark = |x0: u32, y0: u32| {
                (y0 * h / 2..(y0 + 1) * h / 2)
                    .flat_map(|y| (x0 * w / 2 + 4..(x0 + 1) * w / 2 - 4).map(move |x| (x, y)))
                    .filter(|(x, y)| img.pixel(*x, *y).is_some_and(|p| p.alpha() > 128 && p.red() < 100))
                    .count()
            };
            // The line starts at the page's lower-left corner.
            assert!(dark(0, 1) > 3 * dark(0, 0).max(1), "lower left {} vs upper left {}", dark(0, 1), dark(0, 0));
        }
        // Without its PDF, the frame only.
        std::fs::remove_file(&pdf).unwrap();
        assert!(svg_images(&crate::svg::export(&d, &Space::Model)).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plots_apply_the_underlay_contrast() {
        let dir = std::env::temp_dir().join(format!("cadcraft-underlay-contrast-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut d = drawing_with_underlay(&dir.join("Contrast.pdf"), true);
        let png = |d: &Drawing| {
            crate::plotter::plot(d, &Space::Model, &json!({"output": "png", "paper": "A4", "landscape": true, "resolution": 100})).unwrap()
        };
        let full = dark_pixels(&png(&d));
        let h = d.model.iter().find(|e| matches!(e.kind, EntityKind::PdfUnderlay(_))).unwrap().handle;
        d.modify_entity(h, |e| {
            if let EntityKind::PdfUnderlay(u) = &mut e.kind {
                u.contrast = 0;
            }
        })
        .unwrap();
        let none = dark_pixels(&png(&d));
        // What stays dark is the underlay's frame, drawn as an object.
        assert!(full > 5000 && none < full / 4, "at contrast 0 the black line is plotted mid grey: {full} vs {none}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plots_draw_only_the_part_inside_the_clip_boundary() {
        let dir = std::env::temp_dir().join(format!("cadcraft-underlay-clip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut d = drawing_with_underlay(&dir.join("Clip.pdf"), true);
        let png = |d: &Drawing| {
            // The whole page's window, whatever the extents (a clipped underlay's are its boundary).
            let o = json!({"output": "png", "paper": "A4", "landscape": true, "resolution": 100, "window": [[0, 0], [297, 210]]});
            crate::plotter::plot(d, &Space::Model, &o).unwrap()
        };
        let whole = dark_pixels(&png(&d));
        let h = d.model.iter().find(|e| matches!(e.kind, EntityKind::PdfUnderlay(_))).unwrap().handle;
        // The page's top-left corner, away from its diagonal line.
        let set = |d: &mut Drawing, clipping: bool| {
            d.modify_entity(h, |e| {
                if let EntityKind::PdfUnderlay(u) = &mut e.kind {
                    u.clip = vec![Vec2::new(0.0, 150.0), Vec2::new(60.0, 150.0), Vec2::new(60.0, 210.0), Vec2::new(0.0, 210.0)];
                    u.clipping = clipping;
                }
            })
            .unwrap();
        };
        set(&mut d, true);
        let clipped = dark_pixels(&png(&d));
        assert!(whole > 5000 && clipped < whole / 4, "only the boundary's part of the page is plotted: {whole} vs {clipped}");
        // Inverted: the corner is hidden, the rest of the page (its line) plotted.
        d.modify_entity(h, |e| {
            if let EntityKind::PdfUnderlay(u) = &mut e.kind {
                u.clip_inverted = true;
            }
        })
        .unwrap();
        let inverted = dark_pixels(&png(&d));
        assert!(inverted > whole / 2, "inverted: the page outside the boundary is plotted: {whole} vs {inverted}");
        set(&mut d, false);
        assert_eq!(dark_pixels(&png(&d)), whole, "clipping off: the whole page again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Black pixels (the underlay's line; the green fill and white paper aren't).
    fn black_pixels(png: &[u8]) -> usize {
        let img = tiny_skia::Pixmap::decode_png(png).unwrap();
        img.pixels().iter().filter(|p| p.red() < 100 && p.green() < 100 && p.blue() < 100).count()
    }

    #[test]
    fn plots_draw_the_page_in_the_drawing_order() {
        let dir = std::env::temp_dir().join(format!("cadcraft-underlay-order-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A green solid drawn first, larger than the page, then the underlay over it.
        let mut d = drawing_with_underlay(&dir.join("Order.pdf"), true);
        let green = Common { color: cadcraft_color::Color::Index(3), ..Common::default() };
        let c = |x: f64, y: f64| Vec3::new(x, y, 0.0);
        let solid = cadcraft_doc::Solid { corners: [c(-20.0, -20.0), c(317.0, -20.0), c(-20.0, 230.0), c(317.0, 230.0)] };
        let s = d.add(&Space::Model, green, EntityKind::Solid(solid)).unwrap();
        let u = d.model.iter().find(|e| matches!(e.kind, EntityKind::PdfUnderlay(_))).unwrap().handle;
        d.model.bring_to_front(u);
        let png = |d: &Drawing| {
            crate::plotter::plot(d, &Space::Model, &json!({"output": "png", "paper": "A4", "landscape": true, "resolution": 100})).unwrap()
        };
        let pdf = |d: &Drawing| {
            let out = crate::plotter::plot(d, &Space::Model, &json!({"paper": "A4", "landscape": true, "compress": false})).unwrap();
            let text = flat(&out);
            let (image, fill) = (text.find("/x0 Do").unwrap(), text.find(" h f ").unwrap());
            image < fill
        };
        assert!(black_pixels(&png(&d)) > 500, "the page is over the solid drawn before it");
        assert!(!pdf(&d), "the page is placed after the solid");
        // DRAWORDER back: the solid hides it.
        d.model.send_to_back(u);
        assert!(black_pixels(&png(&d)) < 50, "the solid drawn after the page hides it");
        assert!(pdf(&d), "the page is placed before the solid");
        assert!(d.model.iter().any(|e| e.handle == s));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_layout_plots_the_page_seen_through_its_viewport() {
        let dir = std::env::temp_dir().join(format!("cadcraft-underlay-layout-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pdf = dir.join("Layout.pdf");
        let mut d = drawing_with_underlay(&pdf, true);
        // A viewport at 1:1 showing the page's middle only.
        let (u, v) = d.model.iter().find_map(|e| if let EntityKind::PdfUnderlay(u) = &e.kind { Some(u.axes()) } else { None }).unwrap();
        let middle = (u + v) * 0.5;
        let paper = Space::Paper("Layout1".into());
        let vp = cadcraft_doc::Viewport {
            center: Vec3::new(100.0, 100.0, 0.0),
            width: u.len() / 3.0,
            height: v.len() / 3.0,
            view_center: middle,
            view_height: v.len() / 3.0,
            id: 2,
            ..cadcraft_doc::Viewport::default()
        };
        d.add(&paper, Common::default(), EntityKind::Viewport(vp)).unwrap();
        let list = cadcraft_render::build_plot(&d, &paper, &cadcraft_render::Options::default());
        let im = images(&d, &list, None, 2.0);
        assert_eq!(im.len(), 1, "model space's underlay through the viewport");
        let im = im.first().unwrap();
        // Only the part in the viewport is drawn, at its place on paper, clipped to it.
        assert!((im.u.len() - u.len() / 3.0).abs() < 1.0, "{} vs {}", im.u.len(), u.len() / 3.0);
        assert!((im.origin - Vec2::new(100.0 - u.len() / 6.0, 100.0 - v.len() / 6.0)).len() < 1.0, "{:?}", im.origin);
        assert!(!im.clip.is_empty());

        let out = crate::plotter::plot(&d, &paper, &json!({"compress": false})).unwrap();
        let text = flat(&out);
        let (clip, image) = (text.rfind("W n").unwrap(), text.find("/x0 Do").unwrap());
        assert!(clip < image, "the page is clipped to the viewport");
        let png = |d: &Drawing| crate::plotter::plot(d, &paper, &json!({"output": "png", "resolution": 100})).unwrap();
        let with = black_pixels(&png(&d));
        std::fs::remove_file(&pdf).unwrap();
        let without = black_pixels(&png(&d));
        assert!(with > without + 100, "{with} vs {without}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_relative_path_is_found_from_the_drawing() {
        let dir = std::env::temp_dir().join(format!("cadcraft-underlay-rel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut d = drawing_with_underlay(&dir.join("Rel.pdf"), true);
        let hs: Vec<_> = d.model.iter().map(|e| e.handle).collect();
        for h in hs {
            d.model.modify(h, |e| {
                if let EntityKind::PdfUnderlay(u) = &mut e.kind {
                    u.path = "Rel.pdf".into();
                }
            });
        }
        let doc = dir.join("Site.dxf").to_string_lossy().to_string();
        let list = cadcraft_render::build_plot(&d, &Space::Model, &cadcraft_render::Options::default());
        assert_eq!(images(&d, &list, Some(&doc), 1.0).len(), 1);
        assert!(images(&d, &list, None, 1.0).is_empty(), "no drawing folder: not found");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
