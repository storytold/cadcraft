//! The Ribbon: an opt-in tabbed command bar under the menu bar, as Windows drafting users
//! know it (`UiState::show_ribbon`, off by default; Options ▸ Display or `ui.set
//! {"showRibbon": true}`). Its panels are another view of the Tool Sets definitions, so there
//! is no second list of tools to maintain. First step: the Home tab (#306, #1492).

use egui::{Rect, Sense, Stroke, pos2, vec2};

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::palettes::{DRAFTING, LAYER_TOOLS, Tool};
use crate::theme::Tokens;

/// A ribbon panel: (name, large tools, small tools).
pub type Panel = (&'static str, Vec<Tool>, Vec<Tool>);

/// The tabs, in order. Only Home exists so far.
pub const TABS: &[&str] = &["Home"];

fn group(name: &str) -> (&'static [Tool], &'static [Tool]) {
    DRAFTING.iter().find(|(n, _, _)| *n == name).map_or((&[], &[]), |(_, large, small)| (large, small))
}

/// The Home tab's panels, built from the Tool Sets categories.
pub fn home() -> Vec<Panel> {
    let (draw_l, draw_s) = group("Draw");
    let (hatch_l, hatch_s) = group("Hatch");
    let (modify_l, modify_s) = group("Modify");
    let (text_l, text_s) = group("Text");
    let (dim_l, dim_s) = group("Dimension");
    let (leader_l, leader_s) = group("Leader");
    let (block_l, block_s) = group("Block");
    vec![
        ("Draw", draw_l.to_vec(), [draw_s, hatch_l, hatch_s].concat()),
        ("Modify", modify_l.to_vec(), modify_s.iter().copied().filter(|(_, cmd, _)| *cmd != "matchprop").collect()),
        ("Annotation", [text_l, dim_l].concat(), [text_s, dim_s, leader_l, leader_s].concat()),
        ("Layers", LAYER_TOOLS[..1].to_vec(), LAYER_TOOLS[1..].to_vec()),
        ("Block", block_l.to_vec(), block_s.to_vec()),
        ("Properties", vec![(Icon::Properties, "properties", "Properties")], vec![(Icon::MatchProp, "matchprop", "Match Properties")]),
    ]
}

const TAB_H: f32 = 22.0;
const LARGE: f32 = 40.0;
const SMALL: f32 = 23.0;
const LABEL_H: f32 = 16.0;

/// Draw the ribbon as a top panel; a click starts the tool like a Tool Sets button.
pub fn show(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let body_h = SMALL * 3.0 + 18.0;
    egui::Panel::top("cc_ribbon").exact_size(TAB_H + body_h + LABEL_H).frame(egui::Frame::NONE.fill(t.panel)).show(ui, |ui| {
        let r = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(Rect::from_min_max(r.min, pos2(r.right(), r.top() + TAB_H)), 0.0, t.chrome);
        let mut x = r.left() + 4.0;
        for name in TABS {
            let tr = Rect::from_min_size(pos2(x, r.top()), vec2(64.0, TAB_H));
            let resp = ui.interact(tr, ui.scope_id().with(("ribbon_tab", name)), Sense::click());
            p.rect_filled(tr, 0.0, t.tab_active);
            p.text(tr.center(), egui::Align2::CENTER_CENTER, crate::i18n::t(name), egui::FontId::proportional(13.0), t.text);
            icons::describe_control(ui, &resp, crate::i18n::t(name), Some(true), true);
            x += 64.0;
        }
        let mut clicked: Option<&'static str> = None;
        let body = Rect::from_min_max(pos2(r.left(), r.top() + TAB_H), r.max);
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                    for (name, large, small) in home() {
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                // Every panel name sits on one line, however few tools the panel has.
                                ui.set_min_height(SMALL * 3.0 + 4.0);
                                for (icon, cmd, tip) in &large {
                                    ui.add_enabled_ui(crate::palettes::command_known(cmd), |ui| {
                                        if icons::button(ui, *icon, LARGE, tip, false).clicked() {
                                            clicked = Some(cmd);
                                        }
                                    });
                                }
                                for column in small.chunks(3) {
                                    ui.vertical(|ui| {
                                        for (icon, cmd, tip) in column {
                                            ui.add_enabled_ui(crate::palettes::command_known(cmd), |ui| {
                                                if icons::button(ui, *icon, SMALL, tip, false).clicked() {
                                                    clicked = Some(cmd);
                                                }
                                            });
                                        }
                                    });
                                }
                            });
                            let (lr, _) = ui.allocate_exact_size(vec2(ui.min_rect().width(), LABEL_H), Sense::hover());
                            ui.painter().text(
                                lr.center(),
                                egui::Align2::CENTER_CENTER,
                                crate::i18n::t(name),
                                egui::FontId::proportional(11.5),
                                t.text_dim,
                            );
                        });
                        ui.add_space(6.0);
                        let (sr, _) = ui.allocate_exact_size(vec2(1.0, body_h + LABEL_H), Sense::hover());
                        ui.painter().vline(sr.center().x, sr.y_range(), Stroke::new(1.0, t.border));
                    }
                });
            });
        });
        if let Some(c) = clicked {
            app.cmd.buffer.clear();
            app.start(c);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ribbon_is_off_by_default() {
        assert!(!crate::UiState::default().show_ribbon);
        let saved: crate::UiState = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(!saved.show_ribbon, "older saved UI state has no field and keeps the ribbon off");
    }

    #[test]
    fn the_home_tab_has_the_six_panels_in_order_with_known_commands() {
        assert_eq!(TABS, ["Home"]);
        let panels = home();
        let names: Vec<&str> = panels.iter().map(|(n, _, _)| *n).collect();
        assert_eq!(names, ["Draw", "Modify", "Annotation", "Layers", "Block", "Properties"]);
        for (name, large, small) in &panels {
            assert!(!large.is_empty(), "{name} needs at least one large button");
            let mut ids: Vec<&str> = large.iter().chain(small).map(|(_, cmd, _)| *cmd).collect();
            for id in &ids {
                assert!(crate::palettes::command_known(id), "{name}: {id} is not a command");
            }
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), large.len() + small.len(), "{name} lists a tool twice");
        }
        let mut all: Vec<&str> = panels.iter().flat_map(|(_, l, s)| l.iter().chain(s)).map(|(_, cmd, _)| *cmd).collect();
        let count = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), count, "a tool sits in two panels of the tab");
        let draw: Vec<&str> = panels[0].1.iter().chain(&panels[0].2).map(|(_, cmd, _)| *cmd).collect();
        for id in ["line", "pline", "circle", "arc", "hatch"] {
            assert!(draw.contains(&id), "Draw lacks {id}: {draw:?}");
        }
        let annotation: Vec<&str> = panels[2].1.iter().chain(&panels[2].2).map(|(_, cmd, _)| *cmd).collect();
        for id in ["text", "mtext", "dimlinear", "mleader"] {
            assert!(annotation.contains(&id), "Annotation lacks {id}: {annotation:?}");
        }
    }
}
