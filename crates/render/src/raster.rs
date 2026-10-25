//! CPU rasteriser: draws a display list with tiny-skia (PNG export, plot preview, tests).

use cadcraft_color::Rgb;
use cadcraft_geom::{Bounds2, Vec2};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::{DisplayList, Kind};

/// A 2D view: world point at the image centre and pixels per drawing unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub center: Vec2,
    pub scale: f64,
    pub width: u32,
    pub height: u32,
}

impl View {
    pub fn to_screen(&self, p: Vec2) -> (f32, f32) {
        let x = (p.x - self.center.x) * self.scale + f64::from(self.width) / 2.0;
        let y = f64::from(self.height) / 2.0 - (p.y - self.center.y) * self.scale;
        (x as f32, y as f32)
    }
    /// Fit `b` with a margin fraction.
    pub fn fit(b: &Bounds2, width: u32, height: u32, margin: f64) -> View {
        if b.is_empty() {
            return View { center: Vec2::ZERO, scale: 1.0, width, height };
        }
        let w = b.width().max(1e-9);
        let h = b.height().max(1e-9);
        let s = (f64::from(width) / w).min(f64::from(height) / h) * (1.0 - margin);
        View { center: b.center(), scale: if s.is_finite() && s > 0.0 { s } else { 1.0 }, width, height }
    }
    /// The view of `window` (drawing units), filling the image: `width` × `height` pixels when both
    /// are given; otherwise the side given (default 1600 wide) with the other matching the window's
    /// shape, each side 16..=8192.
    pub fn of_window(window: &Bounds2, width: Option<u32>, height: Option<u32>) -> View {
        let aspect = if window.width() > 1e-12 && window.height() > 1e-12 { window.width() / window.height() } else { 1.0 };
        let side = |v: f64| v.round().clamp(16.0, 8192.0) as u32;
        let (w, h) = match (width, height) {
            (Some(w), Some(h)) => (side(f64::from(w)), side(f64::from(h))),
            (None, Some(h)) => (side(f64::from(h) * aspect), side(f64::from(h))),
            (w, None) => {
                let w = f64::from(w.unwrap_or(1600));
                (side(w), side(w / aspect))
            }
        };
        View::fit(window, w, h, 0.0)
    }
    pub fn world_bounds(&self) -> Bounds2 {
        let hw = f64::from(self.width) / 2.0 / self.scale;
        let hh = f64::from(self.height) / 2.0 / self.scale;
        Bounds2::new(self.center - Vec2::new(hw, hh), self.center + Vec2::new(hw, hh))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RasterOptions {
    pub background: Rgb,
    /// Line width in pixels for zero-lineweight lines.
    pub hairline: f32,
    /// Pixels per mm for lineweights (0 = ignore lineweights).
    pub px_per_mm: f32,
    pub antialias: bool,
}

impl Default for RasterOptions {
    fn default() -> Self {
        RasterOptions { background: Rgb(33, 40, 48), hairline: 1.0, px_per_mm: 0.0, antialias: true }
    }
}

/// A picture drawn among the display list's primitives (a PDF underlay's page): before
/// primitive `at` (see [`crate::DisplayList::underlays`]), its lower-left corner and
/// width/height vectors in drawing units, its RGB pixels and their alpha, rows top to bottom, and
/// the triangles (drawing units) it is clipped to, if any.
pub struct Underlay<'a> {
    pub at: usize,
    pub clip: &'a [Vec2],
    pub origin: Vec2,
    pub u: Vec2,
    pub v: Vec2,
    pub width: u32,
    pub height: u32,
    pub rgb: &'a [u8],
    pub alpha: &'a [u8],
}

/// Render to a pixmap. Returns `None` for zero or absurd sizes.
pub fn render(list: &DisplayList, view: &View, o: &RasterOptions) -> Option<Pixmap> {
    render_over(list, view, o, &[])
}

/// [`render`] with the pictures `under` drawn among the primitives.
pub fn render_over(list: &DisplayList, view: &View, o: &RasterOptions, under: &[Underlay]) -> Option<Pixmap> {
    if view.width == 0 || view.height == 0 || view.width > 16384 || view.height > 16384 {
        return None;
    }
    let mut pm = Pixmap::new(view.width, view.height)?;
    pm.fill(tiny_skia::Color::from_rgba8(o.background.0, o.background.1, o.background.2, 255));
    let mut pictures: Vec<&Underlay> = under.iter().collect();
    pictures.sort_by_key(|im| im.at);
    let mut pictures = pictures.into_iter().peekable();
    let vis = view.world_bounds();
    let mut hairlines = Hairlines::default();
    let mut screen: Vec<(f32, f32)> = Vec::new();
    for (i, p) in list.prims.iter().enumerate() {
        while let Some(im) = pictures.next_if(|im| im.at <= i) {
            // The hairlines gathered so far are under the picture.
            hairlines.flush(&mut pm, o.antialias);
            draw_underlay(&mut pm, view, im);
        }
        let c = p.display_rgb(o.background);
        let mut paint = Paint::default();
        // A transparent object blends with what is drawn under it.
        paint.set_color_rgba8(c.0, c.1, c.2, p.alpha());
        paint.anti_alias = o.antialias;
        let pts = list.points(p);
        if p.kind != Kind::Polyline {
            hairlines.flush(&mut pm, o.antialias);
        }
        match p.kind {
            Kind::Polyline => {
                let bb = Bounds2::from_points(pts.iter().copied());
                if !bb.intersects(&vis) {
                    continue;
                }
                let w = if o.px_per_mm > 0.0 && p.lw > 0.0 { (p.lw * o.px_per_mm).max(o.hairline) } else { o.hairline };
                screen.clear();
                screen.extend(pts.iter().map(|q| view.to_screen(*q)));
                if is_hairline(w, o.antialias) && screen.len() >= 2 && screen.iter().all(|&(x, y)| batchable(x) && batchable(y)) {
                    hairlines.add(&mut pm, [c.0, c.1, c.2, p.alpha()], w, &screen, o.antialias);
                    continue;
                }
                hairlines.flush(&mut pm, o.antialias);
                let mut pb = PathBuilder::new();
                for (i, (x, y)) in screen.iter().enumerate() {
                    if i == 0 {
                        pb.move_to(*x, *y);
                    } else {
                        pb.line_to(*x, *y);
                    }
                }
                if let Some(path) = pb.finish() {
                    let stroke = Stroke { width: w, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
                    pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                }
            }
            Kind::Tris | Kind::Mask => {
                if p.kind == Kind::Mask {
                    paint.set_color_rgba8(o.background.0, o.background.1, o.background.2, 255);
                }
                let mut pb = PathBuilder::new();
                for t in pts.chunks(3) {
                    if let [a, b, c] = t {
                        let (ax, ay) = view.to_screen(*a);
                        let (bx, by) = view.to_screen(*b);
                        let (cx, cy) = view.to_screen(*c);
                        pb.move_to(ax, ay);
                        pb.line_to(bx, by);
                        pb.line_to(cx, cy);
                        pb.close();
                    }
                }
                if let Some(path) = pb.finish() {
                    pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
                }
            }
            Kind::Point => {
                if let Some(q) = pts.first() {
                    let (x, y) = view.to_screen(*q);
                    // Filled as a path: tiny-skia's anti-aliased `fill_rect` trips a debug
                    // assertion on rects under two pixels wide whose sides both fall mid-pixel.
                    if let Some(r) = tiny_skia::Rect::from_xywh(x - 0.5, y - 0.5, 1.5, 1.5) {
                        pm.fill_path(&PathBuilder::from_rect(r), &paint, FillRule::Winding, Transform::identity(), None);
                    }
                }
            }
            Kind::Infinite { ray } => {
                let (Some(base), Some(dir)) = (pts.first(), pts.get(1)) else { continue };
                let far = (vis.width() + vis.height()) * 2.0 + base.dist(vis.center());
                let a = if ray { *base } else { *base - *dir * far };
                let b = *base + *dir * far;
                let mut pb = PathBuilder::new();
                let (ax, ay) = view.to_screen(a);
                let (bx, by) = view.to_screen(b);
                pb.move_to(ax, ay);
                pb.line_to(bx, by);
                if let Some(path) = pb.finish() {
                    pm.stroke_path(&path, &paint, &Stroke { width: o.hairline, ..Default::default() }, Transform::identity(), None);
                }
            }
        }
    }
    hairlines.flush(&mut pm, o.antialias);
    for im in pictures {
        draw_underlay(&mut pm, view, im);
    }
    Some(pm)
}

/// Whether tiny-skia strokes a line `width` pixels wide (untransformed) as a hairline: zero
/// width, or at most one pixel when anti-aliased.
fn is_hairline(width: f32, antialias: bool) -> bool {
    width == 0.0 || (antialias && width <= 1.0)
}

/// Largest screen coordinate (pixels) of a polyline stroked together with others. Beyond it a
/// path's integer bounds could overflow, so such a polyline is stroked on its own as before.
const MAX_BATCH_COORD: f32 = 1.0e8;

fn batchable(v: f32) -> bool {
    v.is_finite() && v.abs() <= MAX_BATCH_COORD
}

/// Consecutive hairline polylines with the same colour, alpha and width, stroked as one path.
///
/// tiny-skia draws a hairline segment by segment straight onto the pixmap (each segment with
/// its own caps, a polyline's first and last segment extended for round caps), so one path of
/// several polylines paints exactly the pixels, in the same order, as stroking them one by one;
/// it saves building a paint pipeline and a path per polyline. Thicker strokes are filled
/// outlines, where overlaps would cover once instead of twice, and are never batched.
#[derive(Default)]
struct Hairlines {
    key: Option<([u8; 4], f32)>,
    pb: PathBuilder,
}

impl Hairlines {
    fn add(&mut self, pm: &mut Pixmap, rgba: [u8; 4], width: f32, pts: &[(f32, f32)], antialias: bool) {
        if self.key.is_some_and(|k| k != (rgba, width)) {
            self.flush(pm, antialias);
        }
        self.key = Some((rgba, width));
        for (i, (x, y)) in pts.iter().enumerate() {
            if i == 0 {
                self.pb.move_to(*x, *y);
            } else {
                self.pb.line_to(*x, *y);
            }
        }
    }

    fn flush(&mut self, pm: &mut Pixmap, antialias: bool) {
        let Some(([r, g, b, a], width)) = self.key.take() else { return };
        let pb = std::mem::take(&mut self.pb);
        let Some(path) = pb.finish() else { return };
        let mut paint = Paint::default();
        paint.set_color_rgba8(r, g, b, a);
        paint.anti_alias = antialias;
        let stroke = Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
        pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        self.pb = path.clear();
    }
}

/// Render and encode as PNG.
pub fn render_png(list: &DisplayList, view: &View, o: &RasterOptions) -> Option<Vec<u8>> {
    render(list, view, o)?.encode_png().ok()
}

/// [`render_png`] over `under`.
pub fn render_png_over(list: &DisplayList, view: &View, o: &RasterOptions, under: &[Underlay]) -> Option<Vec<u8>> {
    render_over(list, view, o, under)?.encode_png().ok()
}

/// Straight RGB pixels and their alpha (rows top to bottom) as a PNG file: a PDF underlay's page
/// for an SVG export.
pub fn pixels_png(width: u32, height: u32, rgb: &[u8], alpha: &[u8]) -> Option<Vec<u8>> {
    let n = (width as usize).checked_mul(height as usize)?;
    if rgb.len() != n.checked_mul(3)? || alpha.len() != n {
        return None;
    }
    let mut pm = Pixmap::new(width, height)?;
    for (px, ([r, g, b], a)) in pm.pixels_mut().iter_mut().zip(rgb.as_chunks::<3>().0.iter().zip(alpha)) {
        *px = tiny_skia::ColorU8::from_rgba(*r, *g, *b, *a).premultiply();
    }
    pm.encode_png().ok()
}

/// Draw one picture onto `pm`, mapped onto its parallelogram.
fn draw_underlay(pm: &mut Pixmap, view: &View, im: &Underlay) {
    let n = (im.width as usize).checked_mul(im.height as usize);
    if n.and_then(|n| n.checked_mul(3)) != Some(im.rgb.len()) || n != Some(im.alpha.len()) {
        return;
    }
    // Premultiplied, as tiny-skia draws.
    let pre = |c: u8, a: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
    let rgba: Vec<u8> =
        im.rgb.as_chunks::<3>().0.iter().zip(im.alpha).flat_map(|([r, g, b], a)| [pre(*r, *a), pre(*g, *a), pre(*b, *a), *a]).collect();
    let Some(src) = tiny_skia::IntSize::from_wh(im.width, im.height).and_then(|size| Pixmap::from_vec(rgba, size)) else { return };
    let s = |p: Vec2| view.to_screen(p);
    let top_left = s(im.origin + im.v);
    let right = s(im.origin + im.v + im.u);
    let bottom = s(im.origin);
    let (w, h) = (im.width as f32, im.height as f32);
    let t = Transform::from_row(
        (right.0 - top_left.0) / w,
        (right.1 - top_left.1) / w,
        (bottom.0 - top_left.0) / h,
        (bottom.1 - top_left.1) / h,
        top_left.0,
        top_left.1,
    );
    if [t.sx, t.ky, t.kx, t.sy, t.tx, t.ty].iter().all(|v| v.is_finite()) {
        let paint = tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bilinear, ..Default::default() };
        let mask = clip_mask(pm.width(), pm.height(), view, im.clip);
        if im.clip.is_empty() || mask.is_some() {
            pm.draw_pixmap(0, 0, src.as_ref(), &paint, t, mask.as_ref());
        }
    }
}

/// A mask covering triangles `tris` (drawing units) seen through `view`; `None` when there are
/// none or they cover nothing.
fn clip_mask(width: u32, height: u32, view: &View, tris: &[Vec2]) -> Option<tiny_skia::Mask> {
    let mut pb = PathBuilder::new();
    for t in tris.as_chunks::<3>().0 {
        let [a, b, c] = t.map(|p| view.to_screen(p));
        pb.move_to(a.0, a.1);
        pb.line_to(b.0, b.1);
        pb.line_to(c.0, c.1);
        pb.close();
    }
    let path = pb.finish()?;
    let mut mask = tiny_skia::Mask::new(width, height)?;
    mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
    Some(mask)
}
