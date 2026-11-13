//! The plot stamp (PLOT's `stamp` option): one line of text plotted at the lower-left corner of
//! the printable area, as a record of what was plotted where. The text is given; `{layout}`,
//! `{paper}` and `{scale}` in it become each sheet's own, so a plot of several sheets stamps
//! every sheet with its name.

use cadcraft_color::Color;
use cadcraft_doc::{Common, Drawing, Entity, EntityKind, Handle, Text};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::Value;

/// Height of the stamp's characters on paper (mm).
pub const STAMP_HEIGHT_MM: f64 = 2.0;
/// Gap between the printable area's corner and the stamp (mm).
const STAMP_GAP_MM: f64 = 1.0;
/// Most characters of a stamp.
pub const MAX_STAMP: usize = 300;
/// A text style no drawing is expected to have, so the stamp is always in CADCraft's own
/// stroke font (a missing style plots with the default one).
const STAMP_STYLE: &str = "CADCraft plot stamp";

/// The `stamp` option's text: printable characters, at most [`MAX_STAMP`]; `None` without
/// one (or an empty one).
pub fn stamp_text(opts: &Value) -> Option<String> {
    let t: String = opts.get("stamp")?.as_str()?.chars().filter(|c| !c.is_control()).take(MAX_STAMP).collect();
    let t = t.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// A plot scale as people write it: `1:100`, `2:1`, `1:1`.
pub fn scale_label(paper_per_drawing: f64) -> String {
    if !(paper_per_drawing.is_finite() && paper_per_drawing > 0.0) {
        return "1:1".into();
    }
    let trim = |v: f64| {
        let s = format!("{v:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    if paper_per_drawing >= 1.0 { format!("{}:1", trim(paper_per_drawing)) } else { format!("1:{}", trim(1.0 / paper_per_drawing)) }
}

/// The stamp with `{layout}`, `{paper}` and `{scale}` filled in for one sheet.
pub fn fill(text: &str, layout: &str, paper: &str, scale: &str) -> String {
    text.replace("{layout}", layout).replace("{paper}", paper).replace("{scale}", scale).chars().take(MAX_STAMP).collect()
}

/// What the stamp draws on a sheet whose printable area starts at `corner` (paper units, `unit_mm`
/// millimetres each): `to_world` maps a paper point to the coordinates the sheet is built in, where
/// one paper unit is `world_per_paper` units.
pub fn display_list(
    d: &Drawing,
    text: &str,
    corner: Vec2,
    unit_mm: f64,
    to_world: impl Fn(Vec2) -> Vec2,
    world_per_paper: f64,
    opts: &cadcraft_render::Options,
) -> cadcraft_render::DisplayList {
    let unit = if unit_mm.is_finite() && unit_mm > 0.0 { unit_mm } else { 1.0 };
    let gap = STAMP_GAP_MM / unit;
    let at = to_world(corner + Vec2::new(gap, gap));
    let height = STAMP_HEIGHT_MM / unit * world_per_paper;
    if !(at.x.is_finite() && at.y.is_finite() && height.is_finite() && height > 0.0) {
        return cadcraft_render::DisplayList::default();
    }
    let t = Text {
        insert: Vec3::new(at.x, at.y, 0.0),
        align_pt: None,
        height,
        value: text.to_string(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: STAMP_STYLE.into(),
        halign: Default::default(),
        valign: Default::default(),
    };
    let e = Entity { handle: Handle(0), common: Common { color: Color::Index(7), ..Common::default() }, kind: EntityKind::Text(t) };
    cadcraft_render::build_entities(d, [&e], opts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The stamp's text is trimmed, capped and filled in, and scales read as AutoCAD writes them.
    #[test]
    fn stamp_text_scale_and_placeholders() {
        assert_eq!(stamp_text(&json!({ "stamp": "  Plan\u{7}.dwg  " })).as_deref(), Some("Plan.dwg"));
        assert_eq!(stamp_text(&json!({ "stamp": "   " })), None);
        assert_eq!(stamp_text(&json!({ "stamp": true })), None);
        assert_eq!(stamp_text(&json!({ "stamp": "x".repeat(1000) })).map(|s| s.len()), Some(MAX_STAMP));
        assert_eq!(scale_label(0.01), "1:100");
        assert_eq!(scale_label(2.0), "2:1");
        assert_eq!(scale_label(1.0), "1:1");
        assert_eq!(scale_label(1.0 / 66.6667), "1:66.667");
        assert_eq!(scale_label(f64::NAN), "1:1");
        assert_eq!(fill("{layout} · {paper} · {scale}", "Sheet 1", "A3", "1:50"), "Sheet 1 · A3 · 1:50");
    }

    /// The stamp is drawn just inside the printable area's lower-left corner, about 2 mm high.
    #[test]
    fn the_stamp_draws_text_at_the_corner() {
        let d = Drawing::default();
        let opts = cadcraft_render::Options { text: true, fill: true, ..cadcraft_render::Options::default() };
        let list = display_list(&d, "Plan.dwg", Vec2::new(10.0, 20.0), 1.0, |p| p, 1.0, &opts);
        assert!(!list.prims.is_empty(), "the stamp's characters are drawn");
        // Just inside the printable area's lower-left corner, about 2 mm high.
        assert!(list.bounds.min.x >= 10.0 && list.bounds.min.y >= 20.0, "{:?}", list.bounds);
        assert!(list.bounds.max.y < 20.0 + STAMP_GAP_MM + STAMP_HEIGHT_MM * 1.5, "{:?}", list.bounds);
        // A mapping that goes nowhere draws nothing rather than failing.
        assert!(display_list(&d, "x", Vec2::ZERO, 1.0, |_| Vec2::new(f64::NAN, 0.0), 1.0, &opts).prims.is_empty());
    }

    /// PDF and PNG plots draw the stamp; without one they are as before.
    #[test]
    fn pdf_and_png_plots_carry_the_stamp() {
        let mut d = Drawing::default();
        let line = cadcraft_doc::Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(100.0, 50.0, 0.0) };
        d.add(&cadcraft_doc::Space::Model, Common::default(), EntityKind::Line(line)).unwrap();
        let plot = |extra: Value| {
            let mut o = json!({ "paper": "A4", "compress": false });
            if let (Some(o), Some(e)) = (o.as_object_mut(), extra.as_object()) {
                o.extend(e.clone());
            }
            crate::plotter::plot(&d, &cadcraft_doc::Space::Model, &o).unwrap()
        };
        let pdf_plain = plot(json!({}));
        let pdf_stamped = plot(json!({ "stamp": "Plan.dwg {layout} {paper} {scale}" }));
        assert!(pdf_stamped.len() > pdf_plain.len() + 200, "{} vs {}", pdf_stamped.len(), pdf_plain.len());
        let png_plain = plot(json!({ "output": "png", "resolution": 100 }));
        let png_stamped = plot(json!({ "output": "png", "resolution": 100, "stamp": "Plan.dwg {layout}" }));
        assert_ne!(png_plain, png_stamped);
        // An empty stamp is no stamp.
        assert_eq!(plot(json!({ "output": "png", "resolution": 100, "stamp": " " })), png_plain);
    }
}
