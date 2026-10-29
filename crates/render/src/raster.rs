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

/// Render to a pixmap. Returns `None` for zero or absurd sizes.
pub fn render(list: &DisplayList, view: &View, o: &RasterOptions) -> Option<Pixmap> {
    if view.width == 0 || view.height == 0 || view.width > 16384 || view.height > 16384 {
        return None;
    }
    let mut pm = Pixmap::new(view.width, view.height)?;
    pm.fill(tiny_skia::Color::from_rgba8(o.background.0, o.background.1, o.background.2, 255));
    let vis = view.world_bounds();
    for img in &list.images {
        let mut ib = Bounds2::EMPTY;
        for q in [img.origin, img.origin + img.u, img.origin + img.u + img.v, img.origin + img.v] {
            ib.add(q);
        }
        if !ib.intersects(&vis) {
            continue;
        }
        let Ok(dyn_img) = image::load_from_memory(&img.bytes) else { continue };
        let (iw, ih) = (dyn_img.width(), dyn_img.height());
        if iw == 0 || ih == 0 || iw > 16384 || ih > 16384 {
            continue;
        }
        let Some(mut src_pm) = Pixmap::new(iw, ih) else { continue };
        let rgba = dyn_img.to_rgba8();
        for (dst, px) in src_pm.pixels_mut().iter_mut().zip(rgba.pixels()) {
            if let Some(c) = tiny_skia::PremultipliedColorU8::from_rgba(
                (u16::from(px[0]) * u16::from(px[3]) / 255) as u8,
                (u16::from(px[1]) * u16::from(px[3]) / 255) as u8,
                (u16::from(px[2]) * u16::from(px[3]) / 255) as u8,
                px[3],
            ) {
                *dst = c;
            }
        }
        let (tl_x, tl_y) = view.to_screen(img.origin + img.v);
        let (tr_x, tr_y) = view.to_screen(img.origin + img.u + img.v);
        let (bl_x, bl_y) = view.to_screen(img.origin);
        let sx = (tr_x - tl_x) / (iw as f32);
        let ky = (tr_y - tl_y) / (iw as f32);
        let kx = (bl_x - tl_x) / (ih as f32);
        let sy = (bl_y - tl_y) / (ih as f32);
        let xf = Transform::from_row(sx, ky, kx, sy, tl_x, tl_y);
        let opacity = (100u32.saturating_sub(u32::from(img.fade)) as f32 / 100.0).clamp(0.0, 1.0);
        let paint = tiny_skia::PixmapPaint { opacity, quality: tiny_skia::FilterQuality::Bilinear, ..Default::default() };
        pm.draw_pixmap(0, 0, src_pm.as_ref(), &paint, xf, None);
    }
    let mut hairlines = Hairlines::default();
    let mut screen: Vec<(f32, f32)> = Vec::new();
    for p in &list.prims {
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
