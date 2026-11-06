//! A drafting ribbon for people moving from a modern tabbed CAD workspace.
//!
//! This is original CADCraft UI, using CADCraft's own artwork and commands. It
//! never reads or embeds another application's resource files.

use egui::{Align2, FontId, Rect, Sense, pos2, vec2};

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

type Tool = (Icon, &'static str, &'static str);

struct Group {
    label: &'static str,
    tools: &'static [Tool],
}

// Keys are stable in saved preferences. The names on screen follow the
// familiar drafting tabs; both Portuguese and English are supported.
const TABS: &[(&str, &str, &str)] = &[
    ("home", "Início", "Home"),
    ("insert", "Inserir", "Insert"),
    ("annotate", "Anotar", "Annotate"),
    ("parametric", "Paramétrico", "Parametric"),
    ("view", "Vista", "View"),
    ("manage", "Gerenciar", "Manage"),
    ("output", "Saída", "Output"),
    ("3d", "3D", "3D"),
];

const HOME: &[Group] = &[
    Group {
        label: "Draw",
        tools: &[
            (Icon::Line, "line", "Line"),
            (Icon::Polyline, "pline", "Polyline"),
            (Icon::Circle, "circle", "Circle"),
            (Icon::Arc, "arc", "Arc"),
            (Icon::Rectangle, "rectang", "Rectangle"),
            (Icon::Hatch, "hatch", "Hatch"),
            (Icon::Ellipse, "ellipse", "Ellipse"),
            (Icon::Polygon, "polygon", "Polygon"),
        ],
    },
    Group {
        label: "Modify",
        tools: &[
            (Icon::Move, "move", "Move"),
            (Icon::Copy, "copy", "Copy"),
            (Icon::Rotate, "rotate", "Rotate"),
            (Icon::Mirror, "mirror", "Mirror"),
            (Icon::Trim, "trim", "Trim"),
            (Icon::Offset, "offset", "Offset"),
            (Icon::Fillet, "fillet", "Fillet"),
            (Icon::Erase, "erase", "Erase"),
        ],
    },
    Group {
        label: "Layers",
        tools: &[
            (Icon::LayerProps, "ui.dialog.layers", "Layer Properties Manager"),
            (Icon::LayerOff, "layoff", "Layer Off"),
            (Icon::LayerIso, "layiso", "Isolate Layer"),
            (Icon::MakeCurrent, "laymcur", "Make Current"),
        ],
    },
    Group {
        label: "Annotation",
        tools: &[
            (Icon::MText, "mtext", "Multiline Text"),
            (Icon::DimLinear, "dimlinear", "Linear"),
            (Icon::DimAligned, "dimaligned", "Aligned"),
            (Icon::MLeader, "mleader", "Multileader"),
        ],
    },
    Group {
        label: "Block",
        tools: &[(Icon::Insert, "insert", "Insert Block"), (Icon::BlockCreate, "block", "Create Block"), (Icon::BlockEdit, "bedit", "Block Editor")],
    },
];

const INSERT: &[Group] = &[
    Group {
        label: "Block",
        tools: &[
            (Icon::Insert, "insert", "Insert Block"),
            (Icon::BlockCreate, "block", "Create Block"),
            (Icon::BlockEdit, "bedit", "Block Editor"),
            (Icon::AttDef, "attdef", "Define Attributes"),
        ],
    },
    Group { label: "Reference", tools: &[(Icon::Attach, "attach", "Attach"), (Icon::Import, "import", "Import")] },
    Group { label: "Table", tools: &[(Icon::Table, "table", "Table")] },
];

const ANNOTATE: &[Group] = &[
    Group {
        label: "Text",
        tools: &[(Icon::Text, "text", "Single Line Text"), (Icon::MText, "mtext", "Multiline Text"), (Icon::TextStyle, "style", "Text Style")],
    },
    Group {
        label: "Dimension",
        tools: &[
            (Icon::DimLinear, "dimlinear", "Linear"),
            (Icon::DimAligned, "dimaligned", "Aligned"),
            (Icon::DimAngular, "dimangular", "Angular"),
            (Icon::DimRadius, "dimradius", "Radius"),
            (Icon::DimDiameter, "dimdiameter", "Diameter"),
            (Icon::DimBaseline, "dimbaseline", "Baseline"),
            (Icon::DimContinue, "dimcontinue", "Continue"),
            (Icon::DimStyle, "dimstyle", "Dimension Style"),
        ],
    },
    Group { label: "Leader", tools: &[(Icon::MLeader, "mleader", "Multileader"), (Icon::AddLeader, "mleaderedit", "Add Leader")] },
    Group { label: "Table", tools: &[(Icon::Table, "table", "Table")] },
];

const PARAMETRIC: &[Group] = &[Group {
    label: "Parametric",
    tools: &[
        (Icon::Constraint, "autoconstrain", "Auto Constrain"),
        (Icon::Coincident, "gccoincident", "Coincident"),
        (Icon::Parallel, "gcparallel", "Parallel"),
        (Icon::Perpendicular, "gcperpendicular", "Perpendicular"),
        (Icon::Horizontal, "gchorizontal", "Horizontal"),
        (Icon::Vertical, "gcvertical", "Vertical"),
        (Icon::DelConstraint, "delconstraint", "Delete Constraints"),
        (Icon::Parameters, "parameters", "Parameters Manager"),
    ],
}];

const VIEW: &[Group] = &[
    Group {
        label: "Navigate",
        tools: &[
            (Icon::ZoomExtents, "zoom.extents", "Zoom Extents"),
            (Icon::ZoomWindow, "zoom.window", "Zoom Window"),
            (Icon::Pan, "pan", "Pan"),
            (Icon::Orbit, "3dorbit", "Orbit"),
        ],
    },
    Group {
        label: "Palettes",
        tools: &[(Icon::Layers, "ui.panel.layers", "Layers Palette"), (Icon::Properties, "ui.panel.properties", "Properties Inspector")],
    },
];

const MANAGE: &[Group] = &[
    Group { label: "Utilities", tools: &[(Icon::Measure, "dist", "Measure Distance"), (Icon::Area, "area", "Area"), (Icon::List, "list", "List")] },
    Group {
        label: "Drawing",
        tools: &[(Icon::Layers, "ui.dialog.layers", "Layer Properties Manager"), (Icon::Blocks, "ui.dialog.blocks", "Blocks")],
    },
];

const OUTPUT: &[Group] = &[
    Group {
        label: "Plot",
        tools: &[
            (Icon::Plot, "plot", "Print"),
            (Icon::PageSetup, "pagesetup", "Page Setup Manager"),
            (Icon::Preview, "preview", "Plot Preview"),
            (Icon::Publish, "publish", "Batch Publish"),
        ],
    },
    Group { label: "Export", tools: &[(Icon::Export, "export", "Export")] },
];

const THREE_D: &[Group] = &[
    Group {
        label: "Navigate",
        tools: &[(Icon::Orbit, "3dorbit", "Orbit"), (Icon::Pan, "pan", "Pan"), (Icon::ZoomExtents, "zoom.extents", "Zoom Extents")],
    },
    Group { label: "View", tools: &[(Icon::Workspace, "ui.toggle.viewcube", "ViewCube")] },
];

fn groups(tab: &str) -> &'static [Group] {
    match tab {
        "insert" => INSERT,
        "annotate" => ANNOTATE,
        "parametric" => PARAMETRIC,
        "view" => VIEW,
        "manage" => MANAGE,
        "output" => OUTPUT,
        "3d" => THREE_D,
        _ => HOME,
    }
}

fn tool_button(ui: &mut egui::Ui, icon: Icon, command: &str, label: &str) -> bool {
    let t = Tokens::get();
    let known = cadcraft_engine::find_command(command).is_some() || crate::menus::is_ui_command(command);
    let (rect, response) = ui.allocate_exact_size(vec2(62.0, 46.0), Sense::click());
    if response.hovered() && known {
        ui.painter().rect_filled(rect, 3.0, t.control_hover);
    }
    let tint = if known { t.text } else { t.text_dim.gamma_multiply(0.55) };
    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.center().x, rect.top() + 17.0), vec2(24.0, 24.0)), icon, false);
    let name = crate::i18n::t(label);
    let display = if name.chars().count() > 13 { format!("{}…", name.chars().take(12).collect::<String>()) } else { name.to_string() };
    ui.painter().text(pos2(rect.center().x, rect.bottom() - 9.0), Align2::CENTER_CENTER, display, FontId::proportional(10.0), tint);
    let response = response.on_hover_text(name);
    icons::describe_control(ui, &response, name, None, false);
    known && response.clicked()
}

/// Draw the original CADCraft ribbon immediately beneath the menu bar.
pub fn show(app: &mut CadApp, ui: &mut egui::Ui) {
    if !app.ui.autocad_ribbon {
        return;
    }
    let t = Tokens::get();
    let mut selected = app.ui.ribbon_tab.clone();
    if !TABS.iter().any(|(key, _, _)| *key == selected) {
        selected = "home".into();
    }
    let portuguese = app.language().starts_with("pt");
    let mut clicked = None;
    egui::Panel::top("cc_drafting_ribbon")
        .exact_size(133.0)
        .frame(egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(7, 3)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                for (key, pt, en) in TABS {
                    let title = if portuguese { *pt } else { *en };
                    if ui.selectable_label(selected == *key, title).clicked() {
                        selected = (*key).into();
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::horizontal().id_salt("cc_ribbon_groups").auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for group in groups(&selected) {
                        ui.vertical(|ui| {
                            ui.set_min_height(94.0);
                            egui::Grid::new(("cc_ribbon_group", selected.as_str(), group.label)).num_columns(4).spacing(vec2(1.0, 2.0)).show(
                                ui,
                                |ui| {
                                    for (i, (icon, command, label)) in group.tools.iter().enumerate() {
                                        if tool_button(ui, *icon, command, label) {
                                            clicked = Some(*command);
                                        }
                                        if i % 4 == 3 {
                                            ui.end_row();
                                        }
                                    }
                                },
                            );
                            ui.add_space(2.0);
                            ui.label(egui::RichText::new(crate::i18n::t(group.label)).size(10.5).color(t.text_dim));
                        });
                        ui.separator();
                    }
                });
            });
        });
    app.ui.ribbon_tab = selected;
    if let Some(command) = clicked {
        app.start(command);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ribbon_tab_has_groups_and_all_keys_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for (key, _, _) in TABS {
            assert!(seen.insert(*key), "duplicate ribbon tab: {key}");
            assert!(!groups(key).is_empty(), "empty ribbon tab: {key}");
        }
    }

    #[test]
    fn ribbon_buttons_only_use_available_commands() {
        for (key, _, _) in TABS {
            for group in groups(key) {
                for (_, command, _) in group.tools {
                    assert!(
                        cadcraft_engine::find_command(command).is_some() || crate::menus::is_ui_command(command),
                        "unknown command on ribbon tab {key}: {command}"
                    );
                }
            }
        }
    }
}
