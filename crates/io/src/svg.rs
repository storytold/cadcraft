//! SVG export of a space (vector, for the web and documents).

use cadcraft_color::Rgb;
use cadcraft_doc::{Drawing, Space};
use cadcraft_geom::Bounds2;
use cadcraft_render::Kind;

/// Export with a white background (colour 7 prints black), fitted to the extents.
pub fn export(d: &Drawing, space: &Space) -> String {
    export_window(d, space, None)
}

/// Export showing exactly `window` (the `viewBox`; geometry outside it is clipped by the viewer),
/// or the extents with a 2% margin when `window` is `None`. Objects are drawn with their
/// transparency, as on screen.
pub fn export_window(d: &Drawing, space: &Space, window: Option<Bounds2>) -> String {
    export_window_with(d, space, window, true)
}

/// [`export_window`], also returning the dashed runs it drew continuous because of a dash limit
/// ([`cadcraft_render::DisplayList::undashed`]).
pub fn export_window_noted(d: &Drawing, space: &Space, window: Option<Bounds2>) -> (String, usize) {
    export_noted(d, space, window, true)
}

/// [`export_window`], drawing transparency only when `transparency` is on (plots).
pub fn export_window_with(d: &Drawing, space: &Space, window: Option<Bounds2>, transparency: bool) -> String {
    export_noted(d, space, window, transparency).0
}

/// [`export_window_with`] and the list's [`cadcraft_render::DisplayList::undashed`].
pub(crate) fn export_noted(d: &Drawing, space: &Space, window: Option<Bounds2>, transparency: bool) -> (String, usize) {
    let list = cadcraft_render::build(d, space, &cadcraft_render::Options { transparency, ..cadcraft_render::Options::default() });
    (export_list(&list, window), list.undashed)
}

/// Opacity for a transparent primitive (none for an opaque one or a wipeout's mask).
fn opacity(p: &cadcraft_render::DPrim) -> Option<String> {
    (p.kind != Kind::Mask && p.transparency != 0).then(|| format!("{:.2}", f64::from(p.alpha()) / 255.0))
}

/// Write a display list (as [`export_window`] does for a space's own).
pub fn export_list(list: &cadcraft_render::DisplayList, window: Option<Bounds2>) -> String {
    let window = window.filter(|w| !w.is_empty());
    let b = window.unwrap_or(list.bounds);
    // A single point (or a zero-size window) has no extent: show a unit box centred on it.
    let point = !b.is_empty() && b.width().max(b.height()) <= 1e-9;
    let (w, h) = if b.is_empty() {
        (1.0, 1.0)
    } else if point {
        (0.0, 0.0)
    } else {
        (b.width().max(1e-9), b.height().max(1e-9))
    };
    let m = if point {
        0.5
    } else if window.is_some() {
        0.0
    } else {
        (w.max(h)) * 0.02
    };
    let (x0, y1) = if b.is_empty() { (0.0, 1.0) } else { (b.min.x - m, b.max.y + m) };
    let vw = w + 2.0 * m;
    let vh = h + 2.0 * m;
    let stroke = vw.max(vh) / 1500.0;
    // Decimals: at least 6, more when the stroke would otherwise lose its leading digits.
    let dp = (3.0 - stroke.log10().floor()).clamp(6.0, 15.0) as usize;
    use craft_svg::write::{SvgWriter, num};
    let mut writer = SvgWriter::fragment();
    writer
        .root()
        .attr("viewBox", format!("0 0 {} {}", num(vw, dp), num(vh, dp)))
        .attr("width", format!("{:.0}", 1600.0))
        .attr("height", format!("{:.0}", 1600.0 * vh / vw))
        .open();
    writer.tag("rect").attr("width", "100%").attr("height", "100%").attr("fill", "white").empty();
    let white = Rgb(255, 255, 255);
    let tx = |x: f64| x - x0;
    let ty = |y: f64| y1 - y;
    for p in &list.prims {
        let c = p.display_rgb(white).hex();
        let op = opacity(p);
        let pts = list.points(p);
        match p.kind {
            Kind::Polyline => {
                let points = craft_svg::drawing_write::points(pts.iter().map(|q| [tx(q.x), ty(q.y)]), dp);
                let mut tag = writer.tag("polyline").attr("fill", "none").attr("stroke", &c);
                if let Some(op) = &op {
                    tag = tag.attr("opacity", op);
                }
                tag.attr("stroke-width", num(stroke, dp))
                    .attr("stroke-linecap", "round")
                    .attr("stroke-linejoin", "round")
                    .attr("points", points)
                    .empty();
            }
            Kind::Tris | Kind::Mask => {
                let c = if p.kind == Kind::Mask { white.hex() } else { c };
                let triangles = pts
                    .chunks(3)
                    .filter_map(|t| if let [a, b, c] = t { Some([[tx(a.x), ty(a.y)], [tx(b.x), ty(b.y)], [tx(c.x), ty(c.y)]]) } else { None });
                let path = craft_svg::drawing_write::triangles(triangles, dp);
                let mut tag = writer.tag("path").attr("fill", &c);
                if let Some(op) = &op {
                    tag = tag.attr("opacity", op);
                }
                tag.attr("d", path).empty();
            }
            Kind::Image { .. } => {}
            Kind::Point => {
                if let Some(q) = pts.first() {
                    let mut tag =
                        writer.tag("circle").attr("cx", num(tx(q.x), dp)).attr("cy", num(ty(q.y), dp)).attr("r", num(stroke, dp)).attr("fill", &c);
                    if let Some(op) = &op {
                        tag = tag.attr("opacity", op);
                    }
                    tag.empty();
                }
            }
            Kind::Infinite { ray } => {
                if let (Some(bp), Some(dir)) = (pts.first(), pts.get(1)) {
                    let far = (vw + vh) * 2.0 + if window.is_some() { bp.dist(b.center()) } else { 0.0 };
                    let a = if ray { *bp } else { *bp - *dir * far };
                    let e = *bp + *dir * far;
                    let mut tag = writer
                        .tag("line")
                        .attr("x1", num(tx(a.x), dp))
                        .attr("y1", num(ty(a.y), dp))
                        .attr("x2", num(tx(e.x), dp))
                        .attr("y2", num(ty(e.y), dp))
                        .attr("stroke", &c);
                    if let Some(op) = &op {
                        tag = tag.attr("opacity", op);
                    }
                    tag.attr("stroke-width", num(stroke, dp)).empty();
                }
            }
        }
    }
    writer.close();
    writer.finish()
}
