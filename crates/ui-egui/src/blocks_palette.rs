//! Blocks palette: dockable panel for block management with AutoCAD-parity UI.

use std::collections::HashMap;
use std::sync::Arc;

use cadcraft_color::Rgb;
use cadcraft_engine::Input;
use cadcraft_geom::Vec2;
use cadcraft_render::DisplayList;
use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::CadApp;
use crate::canvas::Xf;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// Presentation mode for the blocks palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViewMode {
    /// Responsive multi-column thumbnail cards.
    Grid,
    /// Compact single-column detailed list rows.
    List,
}

/// Per-drawing and per-revision cached vector display lists for block thumbnail previews.
#[derive(Clone, Default)]
struct ThumbnailCache {
    /// Drawing identity counter.
    uid: u64,
    /// Drawing revision counter when these thumbnails were generated.
    revision: u64,
    /// Cached vector display lists keyed by block name.
    thumbnails: HashMap<String, Option<Arc<DisplayList>>>,
}

/// Render the blocks palette panel with search, view switcher, and block catalog.
pub fn blocks_panel(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();

    let Ok(st) = app.session.state() else {
        ui.vertical_centered(|ui| {
            ui.add_space(24.0);
            ui.label(RichText::new(crate::tl!("No drawing open")).color(t.text_dim).size(13.0));
        });
        return;
    };

    let doc = st.doc.clone();
    let uid = st.uid;
    let revision = st.revision;

    let mut block_infos: Vec<(String, usize, String)> = doc
        .blocks
        .iter()
        .filter(|(k, _)| !k.starts_with('*'))
        .map(|(name, block)| (name.clone(), block.entities.len(), block.description.clone()))
        .collect();

    block_infos.sort_by_key(|a| a.0.to_lowercase());
    let total_count = block_infos.len();

    let search_id = ui.scope_id().with("blocks_palette_search");
    let mut search_query = ui.data_mut(|d| d.get_temp::<String>(search_id)).unwrap_or_default();

    let view_mode_id = ui.scope_id().with("blocks_palette_view_mode");
    let mut view_mode = ui.data_mut(|d| d.get_temp::<ViewMode>(view_mode_id)).unwrap_or(ViewMode::Grid);

    let cache_id = ui.scope_id().with("blocks_palette_thumb_cache");
    let mut cache = ui.data_mut(|d| d.get_temp::<ThumbnailCache>(cache_id)).unwrap_or_default();
    if cache.uid != uid || cache.revision != revision {
        cache.uid = uid;
        cache.revision = revision;
        cache.thumbnails.clear();
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
        let clear_w = if search_query.is_empty() { 0.0 } else { 22.0 };
        let search_w = (ui.available_width() - clear_w).max(40.0);

        let search_resp = ui.add_sized(
            vec2(search_w, 22.0),
            egui::TextEdit::singleline(&mut search_query).hint_text(crate::tl!("Search blocks...")).margin(egui::Margin::symmetric(6, 3)),
        );
        if search_resp.changed() {
            ui.data_mut(|d| d.insert_temp(search_id, search_query.clone()));
        }

        if !search_query.is_empty() && ui.add_sized(vec2(20.0, 22.0), egui::Button::new("✕")).on_hover_text(crate::tl!("Clear search")).clicked() {
            search_query.clear();
            ui.data_mut(|d| d.insert_temp(search_id, String::new()));
        }
    });

    ui.add_space(4.0);

    let filtered_infos: Vec<(String, usize, String)> = block_infos
        .into_iter()
        .filter(|(name, _, _)| search_query.is_empty() || name.to_lowercase().contains(&search_query.to_lowercase()))
        .collect();

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 0.0);

        let count_str = if !search_query.is_empty() { format!("{}/{}", filtered_infos.len(), total_count) } else { format!("{total_count}") };
        ui.label(RichText::new(crate::tf!("{n} blocks", n = count_str)).size(11.0).color(t.text_dim));

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::button(ui, Icon::BlockCreate, 20.0, "Create Block (BLOCK)", false).clicked() {
                app.start("block");
            }

            ui.add_space(2.0);

            if icons::button(ui, Icon::List, 20.0, "List View", view_mode == ViewMode::List).clicked() {
                view_mode = ViewMode::List;
                ui.data_mut(|d| d.insert_temp(view_mode_id, view_mode));
            }
            if icons::button(ui, Icon::Grid, 20.0, "Grid View", view_mode == ViewMode::Grid).clicked() {
                view_mode = ViewMode::Grid;
                ui.data_mut(|d| d.insert_temp(view_mode_id, view_mode));
            }
        });
    });

    ui.add_space(4.0);
    ui.separator();
    ui.add_space(2.0);

    egui::ScrollArea::vertical().id_salt("blocks_palette_scroll").auto_shrink([false, false]).show(ui, |ui| {
        if filtered_infos.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(32.0);
                let (icon_rect, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
                icons::paint(ui.painter(), icon_rect, if search_query.is_empty() { Icon::Blocks } else { Icon::Search }, true);
                ui.add_space(8.0);

                if search_query.is_empty() {
                    ui.label(RichText::new(crate::tl!("No blocks defined in this drawing")).color(t.text_dim).size(13.0));
                    ui.add_space(10.0);
                    if ui.button(crate::tl!("＋ Create Block")).clicked() {
                        app.start("block");
                    }
                } else {
                    ui.label(RichText::new(crate::tf!("No blocks match \"{query}\"", query = search_query)).color(t.text_dim).size(13.0));
                    ui.add_space(8.0);
                    if ui.button(crate::tl!("Clear filter")).clicked() {
                        ui.data_mut(|d| d.insert_temp(search_id, String::new()));
                    }
                }
                ui.add_space(32.0);
            });
            return;
        }

        for (name, _, _) in &filtered_infos {
            if !cache.thumbnails.contains_key(name) {
                let dl = doc.block(name).and_then(|block| {
                    if block.entities.is_empty() {
                        None
                    } else {
                        let rough =
                            cadcraft_render::build_entities(&doc, block.entities.iter().map(|e| e.as_ref()), &cadcraft_render::Options::default());
                        if rough.bounds.is_empty() || rough.prims.is_empty() { None } else { Some(Arc::new(rough)) }
                    }
                });
                cache.thumbnails.insert(name.clone(), dl);
            }
        }

        match view_mode {
            ViewMode::Grid => {
                render_grid_view(ui, &filtered_infos, &cache.thumbnails, t, app);
            }
            ViewMode::List => {
                render_list_view(ui, &filtered_infos, &cache.thumbnails, t, app);
            }
        }
    });

    ui.data_mut(|d| d.insert_temp(cache_id, cache));
}

/// Render the block catalog as a responsive grid of cards filling available width.
fn render_grid_view(
    ui: &mut egui::Ui,
    items: &[(String, usize, String)],
    thumbnails: &HashMap<String, Option<Arc<DisplayList>>>,
    t: Tokens,
    app: &mut CadApp,
) {
    let avail_w = ui.available_width().max(100.0);
    let spacing = 6.0;
    let min_card_w = 96.0;
    let cols = ((avail_w + spacing) / (min_card_w + spacing)).floor().max(1.0) as usize;
    let card_w = ((avail_w - spacing * (cols as f32 - 1.0)) / cols as f32).floor();
    let card_h = (card_w * 0.90).clamp(86.0, 102.0);

    for chunk in items.chunks(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(spacing, spacing);

            for (name, count, desc) in chunk {
                draw_grid_card(ui, name, *count, desc, card_w, card_h, thumbnails.get(name).and_then(|o| o.as_ref()), t, app);
            }
        });
        ui.add_space(spacing);
    }
}

/// Render an individual block card in grid view with vector thumbnail and interaction.
#[allow(clippy::too_many_arguments)]
fn draw_grid_card(
    ui: &mut egui::Ui,
    name: &str,
    count: usize,
    desc: &str,
    w: f32,
    h: f32,
    preview: Option<&Arc<DisplayList>>,
    t: Tokens,
    app: &mut CadApp,
) {
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let is_down = resp.is_pointer_button_down_on();
    let is_hovered = resp.hovered();

    let bg = if is_down {
        t.tab_active
    } else if is_hovered {
        t.control_hover
    } else {
        t.control
    };

    let border_color = if is_hovered { t.accent } else { t.border };
    let border_width = if is_hovered { 1.5 } else { 1.0 };

    let p = ui.painter();
    p.rect(rect, 6.0, bg, Stroke::new(border_width, border_color), StrokeKind::Inside);

    let pad = 5.0;
    let thumb_w = w - (pad * 2.0);
    let thumb_h = (h * 0.52).floor();
    let thumb_rect = Rect::from_min_size(pos2(rect.left() + pad, rect.top() + pad), vec2(thumb_w, thumb_h));

    p.rect(thumb_rect, 4.0, t.canvas, Stroke::new(0.5, t.border), StrokeKind::Inside);

    let mut drawn = false;
    if let Some(list) = preview.filter(|l| !l.bounds.is_empty() && !l.prims.is_empty()) {
        let b = list.bounds;
        let inner = thumb_rect.shrink(3.0);
        let bw = b.width().max(1e-9);
        let bh = b.height().max(1e-9);
        let scale = (f64::from(inner.width()) / bw).min(f64::from(inner.height()) / bh);
        let xf = Xf { rect: thumb_rect, center: Vec2::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0), scale };
        let clipped = p.with_clip_rect(thumb_rect);
        crate::canvas::draw_list(&clipped, &xf, list, Rgb(t.canvas.r(), t.canvas.g(), t.canvas.b()), false);
        drawn = true;
    }

    if !drawn {
        icons::paint(p, Rect::from_center_size(thumb_rect.center(), vec2(22.0, 22.0)), Icon::Blocks, false);
    }

    let max_chars = ((w - 8.0) / 6.8).floor().max(3.0) as usize;
    let short_name = truncate_text(name, max_chars);

    let text_center_y = thumb_rect.bottom() + (h - thumb_rect.bottom() + rect.top()) * 0.32;
    p.text(pos2(rect.center().x, text_center_y), Align2::CENTER_CENTER, short_name, FontId::proportional(11.0), t.text);

    let count_center_y = thumb_rect.bottom() + (h - thumb_rect.bottom() + rect.top()) * 0.72;
    p.text(pos2(rect.center().x, count_center_y), Align2::CENTER_CENTER, format!("{count} ent."), FontId::proportional(9.5), t.text_dim);

    let tooltip_name = name.to_string();
    let tooltip_desc = desc.to_string();
    let resp = resp.on_hover_ui(|ui| {
        ui.horizontal(|ui| {
            let (ir, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            icons::paint(ui.painter(), ir, Icon::Blocks, false);
            ui.label(RichText::new(&tooltip_name).strong().size(12.5));
        });
        if !tooltip_desc.is_empty() {
            ui.label(RichText::new(&tooltip_desc).color(t.text_dim).size(11.0));
        }
        ui.label(RichText::new(crate::tf!("{n} entities", n = count)).color(t.text_dim).size(11.0));
        ui.add_space(2.0);
        ui.label(RichText::new(crate::tl!("💡 Click to insert • Right-click for options")).color(t.accent).size(10.0));
    });

    if resp.clicked() {
        insert_block(app, name);
    }

    resp.context_menu(|ui| {
        ui.style_mut().spacing.item_spacing = vec2(4.0, 4.0);
        if ui.button(crate::tl!("Insert Block")).clicked() {
            insert_block(app, name);
            ui.close();
        }
        if ui.button(crate::tl!("Edit in Block Editor (BEDIT)")).clicked() {
            let _ = app.run("bedit", serde_json::json!({ "name": name }));
            ui.close();
        }
    });
}

/// Render the block catalog as a vertical list of detailed rows.
fn render_list_view(
    ui: &mut egui::Ui,
    items: &[(String, usize, String)],
    thumbnails: &HashMap<String, Option<Arc<DisplayList>>>,
    t: Tokens,
    app: &mut CadApp,
) {
    let row_h = 36.0;
    let avail_w = ui.available_width();

    for (name, count, desc) in items {
        let (rect, resp) = ui.allocate_exact_size(vec2(avail_w, row_h), Sense::click());

        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let bg = if resp.is_pointer_button_down_on() {
            t.tab_active
        } else if resp.hovered() {
            t.control_hover
        } else {
            Color32::TRANSPARENT
        };

        let p = ui.painter();
        if bg != Color32::TRANSPARENT {
            p.rect_filled(rect, 4.0, bg);
        }

        let thumb_rect = Rect::from_min_size(pos2(rect.left() + 4.0, rect.top() + 4.0), vec2(28.0, 28.0));
        p.rect(thumb_rect, 3.0, t.canvas, Stroke::new(0.5, t.border), StrokeKind::Inside);

        let mut drawn = false;
        if let Some(list) = thumbnails.get(name).and_then(|o| o.as_ref()).filter(|l| !l.bounds.is_empty() && !l.prims.is_empty()) {
            let b = list.bounds;
            let inner = thumb_rect.shrink(2.0);
            let bw = b.width().max(1e-9);
            let bh = b.height().max(1e-9);
            let scale = (f64::from(inner.width()) / bw).min(f64::from(inner.height()) / bh);
            let xf = Xf { rect: thumb_rect, center: Vec2::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0), scale };
            let clipped = p.with_clip_rect(thumb_rect);
            crate::canvas::draw_list(&clipped, &xf, list, Rgb(t.canvas.r(), t.canvas.g(), t.canvas.b()), false);
            drawn = true;
        }
        if !drawn {
            icons::paint(p, Rect::from_center_size(thumb_rect.center(), vec2(16.0, 16.0)), Icon::Blocks, false);
        }

        p.text(pos2(thumb_rect.right() + 8.0, rect.top() + 10.0), Align2::LEFT_CENTER, name, FontId::proportional(12.0), t.text);

        let sub = if desc.is_empty() { format!("{count} ent.") } else { format!("{count} ent. — {desc}") };
        p.text(pos2(thumb_rect.right() + 8.0, rect.bottom() - 9.0), Align2::LEFT_CENTER, sub, FontId::proportional(10.0), t.text_dim);

        let insert_rect = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(14.0, 14.0));
        icons::paint(p, insert_rect, Icon::Insert, !resp.hovered());

        let tooltip_name = name.to_string();
        let tooltip_count = *count;
        let resp = resp.on_hover_ui(|ui| {
            ui.label(RichText::new(&tooltip_name).strong().size(12.5));
            ui.label(RichText::new(crate::tf!("{n} entities", n = tooltip_count)).color(t.text_dim).size(11.0));
            ui.label(RichText::new(crate::tl!("💡 Click to insert • Right-click for options")).color(t.accent).size(10.0));
        });

        if resp.clicked() {
            insert_block(app, name);
        }

        resp.context_menu(|ui| {
            ui.style_mut().spacing.item_spacing = vec2(4.0, 4.0);
            if ui.button(crate::tl!("Insert Block")).clicked() {
                insert_block(app, name);
                ui.close();
            }
            if ui.button(crate::tl!("Edit in Block Editor (BEDIT)")).clicked() {
                let _ = app.run("bedit", serde_json::json!({ "name": name }));
                ui.close();
            }
        });

        ui.add_space(2.0);
    }
}

/// Invoke the INSERT command with the specified block name.
fn insert_block(app: &mut CadApp, name: &str) {
    app.start("insert");
    if let Err(e) = app.session.input(Input::Text(name.to_string())) {
        app.session.echo(e.to_string());
    }
}

/// Truncate `text` to `max_len` graphemes with an ellipsis suffix when exceeding bounds.
fn truncate_text(text: &str, max_len: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_len {
        text.to_string()
    } else if max_len <= 1 {
        "…".to_string()
    } else {
        let truncated: String = text.chars().take(max_len - 1).collect();
        format!("{truncated}…")
    }
}
