//! Raster images on the canvas. Each image is a pyramid of levels, each half the size of the one
//! before, cut into tiles that are uploaded as egui textures when first seen. The level drawn is
//! the smallest that still has a pixel for every screen pixel, so a large scan stays sharp zoomed
//! in, doesn't shimmer zoomed out, and only what is on screen takes texture memory. Images are
//! drawn as meshes under the drawing's lines (the drawing itself goes through the GPU callback,
//! so images are not yet interleaved with it in draw order).

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Arc, Mutex, Weak};

use cadcraft_doc::Raster;
use cadcraft_geom::{Bounds2, Vec2};
use cadcraft_render::{DImage, DisplayList, Kind};
use egui::{Color32, Mesh, Rect, Shape, TextureHandle, TextureOptions, epaint::Vertex, pos2};

use super::Xf;

/// The longest tile side uploaded, whatever the device allows.
const MAX_TILE_SIDE: usize = 2048;
/// Above this many uploaded texels per image, tiles not drawn this frame are let go.
const MAX_UPLOADED_TEXELS: usize = 64_000_000;
/// The pyramid stops once a level's longer side is at most this.
const SMALLEST_LEVEL: usize = 256;

/// One level of an image's pyramid.
struct Level {
    width: usize,
    height: usize,
    /// Its pixels; empty for the first level, which is the raster's own.
    rgba: Vec<u8>,
    /// Tiles uploaded so far, by column and row.
    tiles: HashMap<(usize, usize), TextureHandle>,
}

/// The pyramid of one image; it goes when its pixels do.
struct Pyramid {
    pixels: Weak<Raster>,
    levels: Vec<Level>,
}

/// Pyramids by the address of the pixels they were made from, kept between frames. Behind a
/// mutex of its own: uploading a texture takes the context's lock, which `Context::data_mut` holds.
#[derive(Clone, Default)]
struct Textures(Arc<Mutex<HashMap<usize, Pyramid>>>);

/// `rgba` (`width` x `height`) at half the size, each pixel the average of up to four.
fn halved(rgba: &[u8], width: usize, height: usize) -> Option<(Vec<u8>, usize, usize)> {
    let (w, h) = (width.div_ceil(2), height.div_ceil(2));
    let mut out = Vec::with_capacity(w.checked_mul(h)?.checked_mul(4)?);
    let pixel = |x: usize, y: usize| rgba.get(y.checked_mul(width)?.checked_add(x)?.checked_mul(4)?..)?.first_chunk::<4>();
    for y in 0..h {
        let (y0, y1) = (y * 2, (y * 2 + 1).min(height - 1));
        for x in 0..w {
            let (x0, x1) = (x * 2, (x * 2 + 1).min(width - 1));
            let (a, b, c, d) = (pixel(x0, y0)?, pixel(x1, y0)?, pixel(x0, y1)?, pixel(x1, y1)?);
            for i in 0..4 {
                let sum = u16::from(a[i]) + u16::from(b[i]) + u16::from(c[i]) + u16::from(d[i]);
                out.push(((sum + 2) / 4) as u8);
            }
        }
    }
    Some((out, w, h))
}

/// The level to draw at `px_per_unit` device pixels per world unit: the smallest with at least one
/// pixel per device pixel. Level `n` is `2^n` raster pixels to its pixel.
fn level_for(img: &DImage, px_per_unit: f64) -> usize {
    let per_device_pixel = 1.0 / (img.right.len().min(img.down.len()) * px_per_unit);
    if !(per_device_pixel.is_finite() && per_device_pixel > 1.0) {
        return 0;
    }
    per_device_pixel.log2().floor() as usize
}

impl Pyramid {
    fn new(raster: &Arc<Raster>) -> Option<Pyramid> {
        let (width, height) = (usize::try_from(raster.width()).ok()?, usize::try_from(raster.height()).ok()?);
        Some(Pyramid { pixels: Arc::downgrade(raster), levels: vec![Level { width, height, rgba: Vec::new(), tiles: HashMap::new() }] })
    }

    /// Make the levels up to `wanted` (fewer for a small image); the index of the last one to use.
    fn reach(&mut self, raster: &Raster, wanted: usize) -> usize {
        while self.levels.len() <= wanted {
            let Some(last) = self.levels.last() else { return 0 };
            if last.width.max(last.height) <= SMALLEST_LEVEL {
                break;
            }
            let rgba = if last.rgba.is_empty() { raster.rgba() } else { &last.rgba };
            let Some((rgba, width, height)) = halved(rgba, last.width, last.height) else { break };
            self.levels.push(Level { width, height, rgba, tiles: HashMap::new() });
        }
        wanted.min(self.levels.len().saturating_sub(1))
    }

    fn texels(&self) -> usize {
        let tile = |t: &TextureHandle| t.size()[0].saturating_mul(t.size()[1]);
        self.levels.iter().flat_map(|l| l.tiles.values()).map(tile).sum()
    }
}

/// The texture of tile (`col`, `row`), `side` pixels on a side at most, of level `l`.
fn tile(ctx: &egui::Context, l: &mut Level, raster: &Raster, side: usize, col: usize, row: usize) -> Option<TextureHandle> {
    if let Some(t) = l.tiles.get(&(col, row)) {
        return Some(t.clone());
    }
    let rgba = if l.rgba.is_empty() { raster.rgba() } else { &l.rgba };
    let (x, y) = (col.checked_mul(side)?, row.checked_mul(side)?);
    let (w, h) = (side.min(l.width.checked_sub(x)?), side.min(l.height.checked_sub(y)?));
    let mut bytes = Vec::with_capacity(w.checked_mul(h)?.checked_mul(4)?);
    for r in y..y + h {
        let start = r.checked_mul(l.width)?.checked_add(x)?.checked_mul(4)?;
        bytes.extend_from_slice(rgba.get(start..start.checked_add(w.checked_mul(4)?)?)?);
    }
    let name = format!("raster-{:x}-{}x{}-{col}-{row}", std::ptr::from_ref(raster) as usize, l.width, l.height);
    let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &bytes);
    let texture = ctx.load_texture(name, image, TextureOptions::LINEAR);
    l.tiles.insert((col, row), texture.clone());
    Some(texture)
}

/// `poly` cut to the rectangle `min`..`max` (Sutherland-Hodgman; keeps the order of its points).
fn clip_to_rect(poly: &[Vec2], min: Vec2, max: Vec2) -> Vec<Vec2> {
    let mut out = poly.to_vec();
    // Inside is `v * sign >= limit`, for each side in turn.
    for (x_axis, sign, limit) in [(true, 1.0, min.x), (true, -1.0, -max.x), (false, 1.0, min.y), (false, -1.0, -max.y)] {
        let dist = |q: Vec2| (if x_axis { q.x } else { q.y }) * sign - limit;
        let input = std::mem::take(&mut out);
        let mut prev = input.last().copied();
        for cur in input {
            if let Some(p) = prev {
                let (dp, dc) = (dist(p), dist(cur));
                if (dp < 0.0) != (dc < 0.0) {
                    out.push(p + (cur - p) * (dp / (dp - dc)));
                }
            }
            if dist(cur) >= 0.0 {
                out.push(cur);
            }
            prev = Some(cur);
        }
    }
    out
}

/// `outline` as a rectangle when it is one with sides along the axes.
fn axis_rect(outline: &[Vec2]) -> Option<Bounds2> {
    if outline.len() != 4 {
        return None;
    }
    let bounds = Bounds2::from_points(outline.iter().copied());
    // Four points are such a rectangle when each lies on a corner of their bounding box.
    let close = (bounds.width() + bounds.height()) * 1e-9;
    let on_either = |v: f64, low: f64, high: f64| (v - low).abs() <= close || (v - high).abs() <= close;
    let on_a_corner = |q: &Vec2| on_either(q.x, bounds.min.x, bounds.max.x) && on_either(q.y, bounds.min.y, bounds.max.y);
    outline.iter().all(on_a_corner).then_some(bounds)
}

/// Draw the images of `list`.
pub(super) fn draw(p: &egui::Painter, xf: &Xf, list: &DisplayList) {
    if list.images.is_empty() {
        return;
    }
    let vis = xf.world_bounds();
    for prim in &list.prims {
        if let Kind::Image { index } = prim.kind
            && let Some(img) = list.images.get(index as usize)
            && Bounds2::from_points(img.corners()).intersects(&vis)
        {
            draw_one(p, xf, img, prim.alpha());
        }
    }
}

/// Draw one image as textured meshes, one per tile on screen: its rectangle, or the outline it is
/// clipped to.
fn draw_one(p: &egui::Painter, xf: &Xf, img: &DImage, alpha: u8) {
    let ctx = p.ctx();
    let raster = &img.raster;
    let cut = cut(img, xf, p.clip_rect());
    if !cut.rect.is_positive() {
        return;
    }
    // Where in the raster the triangles are.
    let Some(tris) = triangles(img, cut.outline).iter().map(|q| img.pixel_of(*q)).collect::<Option<Vec<Vec2>>>() else { return };
    let color = Color32::from_white_alpha((img.opacity(alpha) * 255.0) as u8);
    let textures = ctx.data_mut(|d| d.get_temp_mut_or_default::<Textures>(egui::Id::unique("cadcraft-raster-textures")).clone());
    let Ok(mut pyramids) = textures.0.lock() else { return };
    // Pyramids whose pixels are gone (drawing closed, image undone) go too.
    pyramids.retain(|_, pyramid| pyramid.pixels.strong_count() > 0);
    let key = Arc::as_ptr(raster) as usize;
    let pyramid = match pyramids.entry(key) {
        Entry::Occupied(e) => e.into_mut(),
        Entry::Vacant(e) => {
            let Some(new) = Pyramid::new(raster) else { return };
            e.insert(new)
        }
    };
    let index = pyramid.reach(raster, level_for(img, xf.scale * f64::from(ctx.pixels_per_point())));
    let Some(level) = pyramid.levels.get_mut(index) else { return };
    let side = ctx.input(|i| i.max_texture_side).clamp(1, MAX_TILE_SIDE);
    // Raster pixels per pixel of this level (not quite 2^index for an odd size).
    let (kx, ky) = (f64::from(raster.width()) / level.width as f64, f64::from(raster.height()) / level.height as f64);
    let mut drawn = Vec::new();
    for row in 0..level.height.div_ceil(side) {
        for col in 0..level.width.div_ceil(side) {
            let (tw, th) = (side.min(level.width - col * side), side.min(level.height - row * side));
            let min = Vec2::new((col * side) as f64 * kx, (row * side) as f64 * ky);
            let max = Vec2::new(((col * side + tw) as f64) * kx, ((row * side + th) as f64) * ky);
            let corners = [img.at(min.x, min.y), img.at(max.x, min.y), img.at(max.x, max.y), img.at(min.x, max.y)];
            let on_screen = Rect::from_points(&corners.map(|q| xf.to_screen(q)));
            if !on_screen.intersects(cut.rect) {
                continue;
            }
            let mut mesh = Mesh::default();
            for t in tris.as_chunks::<3>().0 {
                let piece = clip_to_rect(t, min, max);
                // A fan over the piece, each triangle laid out in turn.
                for pair in piece.get(1..).unwrap_or(&[]).windows(2) {
                    for q in [piece.first(), pair.first(), pair.last()].into_iter().flatten() {
                        let at = img.at(q.x, q.y);
                        let uv =
                            pos2(((q.x - min.x) / (max.x - min.x)).clamp(0.0, 1.0) as f32, ((q.y - min.y) / (max.y - min.y)).clamp(0.0, 1.0) as f32);
                        mesh.indices.push(mesh.vertices.len() as u32);
                        mesh.vertices.push(Vertex { pos: xf.to_screen(at), uv, color });
                    }
                }
            }
            if mesh.indices.is_empty() {
                continue;
            }
            let Some(texture) = tile(ctx, level, raster, side, col, row) else { continue };
            mesh.texture_id = texture.id();
            p.with_clip_rect(cut.rect).add(Shape::mesh(mesh));
            drawn.push((col, row));
        }
    }
    if pyramid.texels() > MAX_UPLOADED_TEXELS {
        for (i, l) in pyramid.levels.iter_mut().enumerate() {
            l.tiles.retain(|at, _| i == index && drawn.contains(at));
        }
    }
}

/// How an image's clip outlines are applied on the canvas, which can clip a painter to a
/// rectangle but has no general clip.
struct Cut<'a> {
    /// The screen rectangle the painter is clipped to: the canvas, narrowed by every outline
    /// that is a rectangle along the axes (viewports, an upright image's own boundary).
    rect: Rect,
    /// The first outline of another shape, which the mesh itself is cut to. Further ones are
    /// not applied on screen.
    outline: Option<&'a [Vec2]>,
}

fn cut<'a>(img: &'a DImage, xf: &Xf, canvas: Rect) -> Cut<'a> {
    let mut cut = Cut { rect: canvas, outline: None };
    for outline in &img.clips {
        match axis_rect(outline) {
            Some(b) => cut.rect = cut.rect.intersect(Rect::from_two_pos(xf.to_screen(b.min), xf.to_screen(b.max))),
            None if cut.outline.is_none() => cut.outline = Some(outline),
            None => {}
        }
    }
    cut
}

/// World points, three per triangle, covering what is drawn of `img`: the area inside
/// `outline`, or the whole rectangle.
fn triangles(img: &DImage, outline: Option<&[Vec2]>) -> Vec<Vec2> {
    match outline {
        Some(o) => {
            let mut t = cadcraft_render::triangulate_evenodd(&[o.to_vec()]);
            t.truncate(t.len() - t.len() % 3);
            t
        }
        None => {
            let [a, b, c, d] = img.corners();
            vec![a, b, c, a, c, d]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{Common, Drawing, EntityKind, Image, Space};
    use cadcraft_geom::Vec3;
    use cadcraft_render::{Options, build};
    use egui::epaint::ClippedShape;

    fn v(x: f64, y: f64) -> Vec2 {
        Vec2::new(x, y)
    }

    /// `w` x `h` pixels, each with its column in red and its row in green.
    fn raster(w: u32, h: u32) -> Arc<Raster> {
        let px: Vec<u8> = (0..h).flat_map(|y| (0..w).flat_map(move |x| [x as u8, y as u8, 0, 255])).collect();
        Arc::new(Raster::new(w, h, px).unwrap())
    }

    /// A drawing with a 10 x 10 unit image of 2 x 2 pixels at the origin, changed by `f`.
    fn list(f: impl FnOnce(&mut Image)) -> DisplayList {
        let mut i = Image { u: Vec3::new(5.0, 0.0, 0.0), v: Vec3::new(0.0, 5.0, 0.0), size: v(2.0, 2.0), path: "a.png".into(), ..Image::default() };
        f(&mut i);
        let mut d = Drawing::new_imperial();
        d.add(&Space::Model, Common::default(), EntityKind::Image(i)).unwrap();
        d.rasters.insert("a.png", raster(2, 2));
        build(&d, &Space::Model, &Options::default())
    }

    /// A 100 x 100 point canvas showing the 10 x 10 square, and what `draw` painted on it.
    fn painted(list: &DisplayList) -> (egui::Context, Vec<ClippedShape>) {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
        let xf = Xf { rect, center: v(5.0, 5.0), scale: 10.0 };
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| {
            let painter = ui.ctx().layer_painter(egui::LayerId::background()).with_clip_rect(rect);
            draw(&painter, &xf, list);
        });
        // No renderer takes the uploads in a test.
        out.textures_delta.clear();
        let shapes = std::mem::take(&mut out.shapes);
        (ctx, shapes)
    }

    fn meshes(shapes: &[ClippedShape]) -> Vec<(Rect, &Mesh)> {
        shapes.iter().filter_map(|s| if let Shape::Mesh(m) = &s.shape { Some((s.clip_rect, &**m)) } else { None }).collect()
    }

    #[test]
    fn a_rectangle_along_the_axes_is_recognised() {
        assert!(axis_rect(&[v(0.0, 0.0), v(4.0, 0.0), v(4.0, 2.0), v(0.0, 2.0)]).is_some());
        assert!(axis_rect(&[v(4.0, 2.0), v(0.0, 2.0), v(0.0, 0.0), v(4.0, 0.0)]).is_some());
        assert!(axis_rect(&[v(0.0, 0.0), v(4.0, 1.0), v(4.0, 2.0), v(0.0, 2.0)]).is_none(), "a slanted side");
        assert!(axis_rect(&[v(0.0, 0.0), v(4.0, 0.0), v(0.0, 2.0)]).is_none(), "a triangle");
        assert!(axis_rect(&[]).is_none());
        assert!(axis_rect(&[v(f64::NAN, 0.0), v(1.0, 0.0), v(1.0, 1.0), v(0.0, 1.0)]).is_none());
    }

    #[test]
    fn each_level_is_half_the_size_and_averages_its_pixels() {
        let r = raster(4, 3);
        let (half, w, h) = halved(r.rgba(), 4, 3).unwrap();
        assert_eq!((w, h), (2, 2));
        assert_eq!(half.len(), 16);
        // Columns 0 and 1 of rows 0 and 1: red 0.5 rounds up, green likewise.
        assert_eq!(&half[..4], &[1, 1, 0, 255]);
        // The odd last row counts twice in the bottom-right pixel: columns 2 and 3 of row 2.
        assert_eq!(&half[12..16], &[3, 2, 0, 255]);
    }

    #[test]
    fn the_level_follows_the_zoom() {
        let img = |px: f64| DImage { raster: raster(1, 1), origin: v(0.0, 0.0), right: v(px, 0.0), down: v(0.0, -px), clips: Vec::new(), fade: 0 };
        // One raster pixel to a screen pixel or larger: all of them.
        assert_eq!(level_for(&img(1.0), 1.0), 0);
        assert_eq!(level_for(&img(1.0), 5.0), 0);
        // Eight raster pixels to a screen pixel.
        assert_eq!(level_for(&img(1.0), 0.125), 3);
        assert_eq!(level_for(&img(1.0), 0.2), 2);
        // Squashed flat: never a number that means anything.
        assert_eq!(level_for(&img(0.0), 1.0), 0);
    }

    #[test]
    fn clipping_keeps_the_points_inside_and_cuts_the_rest() {
        let tri = [v(0.0, 0.0), v(10.0, 0.0), v(0.0, 10.0)];
        assert_eq!(clip_to_rect(&tri, v(-1.0, -1.0), v(11.0, 11.0)), tri, "all inside, in order");
        assert!(clip_to_rect(&tri, v(20.0, 20.0), v(30.0, 30.0)).is_empty());
        let cut = clip_to_rect(&tri, v(0.0, 0.0), v(5.0, 10.0));
        assert_eq!(cut.len(), 4);
        assert!(cut.iter().all(|q| q.x <= 5.0));
    }

    #[test]
    fn an_image_larger_than_a_tile_is_cut_into_tiles_that_meet() {
        let side = 4096;
        let d = {
            let mut d = Drawing::new_imperial();
            let i = Image {
                u: Vec3::new(1.0, 0.0, 0.0),
                v: Vec3::new(0.0, 1.0, 0.0),
                size: v(side as f64, 1.0),
                path: "a.png".into(),
                ..Image::default()
            };
            d.add(&Space::Model, Common::default(), EntityKind::Image(i)).unwrap();
            d.rasters.insert("a.png", Arc::new(Raster::new(side, 1, vec![255; side as usize * 4]).unwrap()));
            d
        };
        let l = build(&d, &Space::Model, &Options::default());
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(8192.0, 100.0));
        let xf = Xf { rect, center: v(side as f64 / 2.0, 0.5), scale: 2.0 };
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(rect), max_texture_side: Some(1 << 14), ..Default::default() }, |ui| {
            let painter = ui.ctx().layer_painter(egui::LayerId::background()).with_clip_rect(rect);
            draw(&painter, &xf, &l);
        });
        out.textures_delta.clear();
        let m = meshes(&out.shapes);
        assert_eq!(m.len(), 2, "two tiles of {MAX_TILE_SIDE} pixels");
        let edge = |m: &Mesh, right: bool| {
            m.vertices.iter().map(|q| q.pos.x).fold(if right { f32::MIN } else { f32::MAX }, if right { f32::max } else { f32::min })
        };
        assert_eq!(edge(m[0].1, true), edge(m[1].1, false), "no gap between them");
        assert_ne!(m[0].1.texture_id, m[1].1.texture_id);
    }

    #[test]
    fn an_image_is_one_textured_rectangle() {
        let l = list(|_| {});
        let (_ctx, shapes) = painted(&l);
        let m = meshes(&shapes);
        assert_eq!(m.len(), 1);
        let (_, mesh) = m[0];
        assert_ne!(mesh.texture_id, egui::TextureId::default());
        assert_eq!(mesh.indices, [0, 1, 2, 3, 4, 5], "two triangles");
        // On screen and in the texture: top left, top right, bottom right, then top left, bottom
        // right, bottom left of the picture.
        let (tl, tr, br, bl) = (pos2(0.0, 0.0), pos2(100.0, 0.0), pos2(100.0, 100.0), pos2(0.0, 100.0));
        let uv = |p: egui::Pos2| pos2(p.x / 100.0, p.y / 100.0);
        let got: Vec<(egui::Pos2, egui::Pos2)> = mesh.vertices.iter().map(|v| (v.pos, v.uv)).collect();
        assert_eq!(got, [tl, tr, br, tl, br, bl].map(|p| (p, uv(p))));
        assert!(mesh.vertices.iter().all(|v| v.color == Color32::WHITE));
    }

    #[test]
    fn fade_dims_the_image_and_nothing_is_drawn_when_fully_faded_or_out_of_view() {
        let (_c, shapes) = painted(&list(|i| i.fade = 50));
        assert_eq!(meshes(&shapes)[0].1.vertices[0].color, Color32::from_white_alpha(127));
        // Far from the view: skipped.
        let (_c, shapes) = painted(&list(|i| i.insert = Vec3::new(1000.0, 1000.0, 0.0)));
        assert!(meshes(&shapes).is_empty());
        // No images at all.
        let (_c, shapes) = painted(&DisplayList::default());
        assert!(shapes.is_empty());
    }

    #[test]
    fn an_upright_clip_boundary_clips_the_painter_and_another_shapes_the_mesh() {
        // The left pixel column: a rectangle along the axes.
        let (_c, shapes) = painted(&list(|i| {
            i.clip = vec![v(-0.5, -0.5), v(0.5, 1.5)];
            i.clipping = true;
        }));
        let m = meshes(&shapes);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].0, Rect::from_min_max(pos2(0.0, 0.0), pos2(50.0, 100.0)));
        assert_eq!(m[0].1.vertices.len(), 6, "the whole rectangle, cut by the clip");

        // A triangle over the top-left half: the mesh is the triangle, with matching texture points.
        let (_c, shapes) = painted(&list(|i| {
            i.clip = vec![v(-0.5, -0.5), v(1.5, -0.5), v(-0.5, 1.5)];
            i.clipping = true;
        }));
        let m = meshes(&shapes);
        let mesh = m[0].1;
        assert!(mesh.vertices.len() >= 3 && mesh.indices.len().is_multiple_of(3) && mesh.indices.len() == mesh.vertices.len());
        for vert in &mesh.vertices {
            assert!((vert.pos.x / 100.0 - vert.uv.x).abs() < 1e-4 && (vert.pos.y / 100.0 - vert.uv.y).abs() < 1e-4, "{vert:?}");
            assert!(vert.pos.x + vert.pos.y <= 100.0 + 1e-3, "inside the triangle: {vert:?}");
        }
    }

    #[test]
    fn a_tile_is_uploaded_once_and_dropped_with_its_pixels() {
        let r = raster(4, 4);
        let mut l = list(|_| {});
        // The list's pixels are this test's.
        l.images[0].raster = r.clone();
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
        let xf = Xf { rect, center: v(5.0, 5.0), scale: 10.0 };
        let frame = |list: &DisplayList| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| {
                let painter = ui.ctx().layer_painter(egui::LayerId::background()).with_clip_rect(rect);
                draw(&painter, &xf, list);
            });
            let uploads = out.textures_delta.set.len();
            out.textures_delta.clear();
            uploads
        };
        assert!(frame(&l) >= 1, "the first frame also uploads the font atlas");
        assert_eq!(frame(&l), 0, "already uploaded");
        // The pixels gone (drawing closed, image undone): the pyramid goes on the next draw.
        let count = |ctx: &egui::Context| {
            let t = ctx.data_mut(|d| d.get_temp_mut_or_default::<Textures>(egui::Id::unique("cadcraft-raster-textures")).clone());
            t.0.lock().unwrap().len()
        };
        assert_eq!(count(&ctx), 1);
        drop(l);
        drop(r);
        frame(&list(|_| {}));
        assert_eq!(count(&ctx), 1, "only the new image's");
    }

    /// A known limit, kept visible here: on screen images are painted first, so they lie under
    /// lines drawn before them. (Pictures and plots keep the draw order; see the renderer's
    /// `draw_order_is_kept_in_pictures`.) When the canvas learns to interleave them, this test
    /// changes with it.
    #[test]
    fn on_screen_images_lie_under_the_lines_whatever_the_draw_order() {
        use cadcraft_doc::Line;
        let line = EntityKind::Line(Line { a: Vec3::new(0.0, 5.0, 0.0), b: Vec3::new(10.0, 5.0, 0.0) });
        let image = EntityKind::Image(Image {
            u: Vec3::new(5.0, 0.0, 0.0),
            v: Vec3::new(0.0, 5.0, 0.0),
            size: v(2.0, 2.0),
            path: "a.png".into(),
            ..Image::default()
        });
        for order in [[line.clone(), image.clone()], [image, line]] {
            let mut d = Drawing::new_imperial();
            for k in order {
                d.add(&Space::Model, Common::default(), k).unwrap();
            }
            d.rasters.insert("a.png", raster(2, 2));
            let l = build(&d, &Space::Model, &Options::default());
            let ctx = egui::Context::default();
            let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
            let xf = Xf { rect, center: v(5.0, 5.0), scale: 10.0 };
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| {
                let painter = ui.ctx().layer_painter(egui::LayerId::background()).with_clip_rect(rect);
                crate::canvas::draw_list(&painter, &xf, &l, cadcraft_color::Rgb(0, 0, 0), false);
            });
            out.textures_delta.clear();
            let kinds: Vec<bool> = out.shapes.iter().map(|s| matches!(s.shape, Shape::Mesh(_))).collect();
            assert!(kinds.len() >= 2, "the image and the lines: {kinds:?}");
            assert_eq!(kinds.iter().position(|m| *m), Some(0), "the image comes first: {kinds:?}");
            assert_eq!(kinds.iter().filter(|m| **m).count(), 1);
        }
    }

    #[test]
    fn degenerate_images_do_not_panic() {
        for f in [
            (|i: &mut Image| i.u = Vec3::ZERO) as fn(&mut Image),
            |i| i.v = i.u,
            |i| i.u = Vec3::new(1e300, 0.0, 0.0),
            |i| i.insert = Vec3::new(-1e300, 1e300, 0.0),
            |i| {
                i.clip = vec![v(0.0, 0.0), v(0.0, 0.0), v(0.0, 0.0)];
                i.clipping = true;
            },
            |i| {
                i.clip = vec![v(f64::NAN, 0.0), v(1.0, f64::INFINITY), v(-1e300, 1e300)];
                i.clipping = true;
            },
        ] {
            let _ = painted(&list(f));
        }
    }
}
