//! The arrowhead drop-down (built-in arrowheads and the drawing's user arrowhead blocks, each
//! with its shape), for the Properties palette's multileader rows.

use egui::vec2;

/// The arrowheads, as DIMBLK names and their labels.
pub const ARROWS: &[(&str, &str)] = &[
    ("", "Closed filled"),
    ("_CLOSEDBLANK", "Closed blank"),
    ("_CLOSED", "Closed"),
    ("_DOT", "Dot"),
    ("_ARCHTICK", "Architectural tick"),
    ("_OBLIQUE", "Oblique"),
    ("_OPEN", "Open"),
    ("_ORIGIN", "Origin indicator"),
    ("_ORIGIN2", "Origin indicator 2"),
    ("_OPEN90", "Right angle"),
    ("_OPEN30", "Open 30"),
    ("_DOTSMALL", "Dot small"),
    ("_DOTBLANK", "Dot blank"),
    ("_SMALL", "Dot small blank"),
    ("_BOXBLANK", "Box"),
    ("_BOXFILLED", "Box filled"),
    ("_DATUMBLANK", "Datum triangle"),
    ("_DATUMFILLED", "Datum triangle filled"),
    ("_INTEGRAL", "Integral"),
    ("_NONE", "None"),
];

/// The drawing's names a row may offer: linetypes and text styles.
#[derive(Clone, Default)]
pub struct Names {
    pub linetypes: Vec<String>,
    pub text_styles: Vec<String>,
    /// User arrowhead blocks the drawing's dimension styles use.
    pub arrow_blocks: Vec<String>,
    /// Their shapes for the list's icons: the tip at the origin, pointing along -x, unit size.
    pub arrow_shapes: Vec<(String, cadcraft_render::ArrowGeom)>,
}

impl Names {
    pub fn of(d: &cadcraft_doc::Drawing) -> Names {
        let mut arrow_blocks: Vec<String> = Vec::new();
        let dims = d.dim_styles.iter().flat_map(|st| [&st.arrow_block, &st.arrow_block1, &st.arrow_block2, &st.leader_arrow_block]);
        let mleaders = d.mleader_styles.iter().map(|st| &st.arrow_block);
        for n in dims.chain(mleaders) {
            if !builtin_arrow(n) && cadcraft_render::arrow_block(d, n).is_some() && !arrow_blocks.iter().any(|b| b.eq_ignore_ascii_case(n)) {
                arrow_blocks.push(n.trim().to_string());
            }
        }
        let arrow_shapes = arrow_blocks
            .iter()
            .map(|n| (n.clone(), cadcraft_render::named_arrow(d, n, cadcraft_geom::Vec2::ZERO, cadcraft_geom::Vec2::new(-1.0, 0.0), 1.0)))
            .collect();
        Names {
            linetypes: d.linetypes.iter().map(|l| l.name.clone()).collect(),
            text_styles: d.text_styles.iter().map(|s| s.name.clone()).collect(),
            arrow_blocks,
            arrow_shapes,
        }
    }
}

/// The arrowhead list: the built-in ones, then the drawing's user arrowheads (and `cur` when it
/// is a block none of the styles use).
fn arrow_items(names: &Names, cur: &str) -> Vec<(String, String)> {
    let mut items: Vec<(String, String)> = ARROWS.iter().map(|(n, l)| (n.to_string(), crate::i18n::t(l).to_string())).collect();
    let cur = cur.trim().to_string();
    for b in names.arrow_blocks.iter().chain((!builtin_arrow(&cur)).then_some(&cur)) {
        if !items.iter().any(|(n, _)| n.eq_ignore_ascii_case(b)) {
            items.push((b.clone(), format!("{} {}", crate::tl!("User Arrow:"), b)));
        }
    }
    items
}

/// The entry of [`arrow_items`] that `cur` (any spelling of an arrowhead) is.
fn shown_arrow(names: &Names, cur: &str) -> String {
    if builtin_arrow(cur) {
        ARROWS.iter().find(|(n, _)| same_arrow(n, cur)).map_or(String::new(), |(n, _)| n.to_string())
    } else {
        arrow_items(names, cur).into_iter().find(|(n, _)| n.eq_ignore_ascii_case(cur.trim())).map_or(cur.to_string(), |(n, _)| n)
    }
}

/// An arrowhead drop-down (built-in arrowheads and the drawing's user arrowhead blocks) showing
/// `cur`; the name picked.
pub fn arrow_combo(ui: &mut egui::Ui, id: egui::Id, names: &Names, cur: &str) -> Option<String> {
    let current = shown_arrow(names, cur);
    let items = arrow_items(names, cur);
    let shown = items.iter().find(|(v, _)| *v == current).map_or_else(String::new, |(_, l)| l.clone());
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let (r, _) = ui.allocate_exact_size(vec2(34.0, 18.0), egui::Sense::hover());
        paint_arrow(ui.painter(), r, names, &current);
        craft_ui::ComboBox::from_id_salt(id)
            .width((ui.available_width() - 14.0).clamp(80.0, 170.0))
            .selected_text(crate::i18n::widget(ui, &shown, egui::TextStyle::Button))
            .height(360.0)
            .show_ui(ui, |ui| {
                for (v, l) in &items {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(34.0, 18.0), egui::Sense::hover());
                        paint_arrow(ui.painter(), r, names, v);
                        if ui.selectable_label(*v == current, crate::i18n::widget(ui, l, egui::TextStyle::Button)).clicked() && *v != current {
                            picked = Some(v.clone());
                        }
                    });
                }
            });
    });
    picked
}

/// The arrowhead `name` as a small picture: a short line ending in the arrowhead at the left.
pub fn paint_arrow(p: &egui::Painter, r: egui::Rect, names: &Names, name: &str) {
    use cadcraft_geom::Vec2;
    thread_local! {
        static BLANK: cadcraft_doc::Drawing = cadcraft_doc::Drawing::default();
    }
    let g = match names.arrow_shapes.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim())) {
        Some((_, g)) => g.clone(),
        None => BLANK.with(|d| cadcraft_render::named_arrow(d, name, Vec2::ZERO, Vec2::new(-1.0, 0.0), 1.0)),
    };
    let col = crate::theme::Tokens::get().text;
    // The arrowhead is one unit long: fit about 1.9 units (arrow and line) across the box.
    let k = f64::from(r.width()) / 2.0;
    let left = f64::from(r.left()) + 0.1 * k;
    let at = |v: &Vec2| egui::pos2((left + v.x * k) as f32, (f64::from(r.center().y) - v.y * k) as f32);
    let clip = p.with_clip_rect(r.expand(1.0));
    let stroke = egui::Stroke::new(1.2, col);
    clip.line_segment([at(&Vec2::ZERO), at(&Vec2::new(1.85, 0.0))], stroke);
    for l in &g.lines {
        if l.len() >= 2 {
            clip.add(egui::Shape::line(l.iter().map(at).collect(), stroke));
        }
    }
    let mut mesh = egui::Mesh::default();
    for t in g.tris.as_chunks::<3>().0 {
        let base = u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX);
        for v in t {
            mesh.colored_vertex(at(v), col);
        }
        mesh.add_triangle(base, base + 1, base + 2);
    }
    if !mesh.is_empty() {
        clip.add(egui::Shape::mesh(mesh));
    }
}

fn same_arrow(a: &str, b: &str) -> bool {
    cadcraft_render::Arrowhead::parse(a) == cadcraft_render::Arrowhead::parse(b)
}

/// Whether a DIMBLK name is one of the arrowheads drawn in code (any of its spellings); other
/// names are user arrowheads (blocks).
pub fn builtin_arrow(name: &str) -> bool {
    let norm = |s: &str| s.trim().trim_start_matches('_').chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    let n = norm(name);
    n.is_empty()
        || ARROWS.iter().any(|(k, l)| norm(k) == n || norm(l) == n)
        || ["filled", "blank", "architecturaltick", "tick", "rightangle", "originindicator", "originindicator2", "box", "datumtriangle"]
            .contains(&n.as_str())
}
