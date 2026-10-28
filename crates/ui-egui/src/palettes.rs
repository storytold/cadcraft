//! Tool Sets (left) and the Layers / Properties palettes (right).

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::EntityKind;
use egui::{Color32, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

#[cfg(test)]
mod layer_menu_tests;
/// The hatch pattern picker.
pub mod patterns;
/// The QuickCalc palette.
pub mod quickcalc;

type Tool = (Icon, &'static str, &'static str);

/// Tool Sets groups: (name, large tools, small tools).
pub const DRAFTING: &[(&str, &[Tool], &[Tool])] = &[
    (
        "Draw",
        &[
            (Icon::Line, "line", "Line"),
            (Icon::Polyline, "pline", "Polyline"),
            (Icon::Circle, "circle", "Circle"),
            (Icon::Rectangle, "rectang", "Rectangle"),
        ],
        &[
            (Icon::Arc, "arc", "Arc"),
            (Icon::Spline, "spline", "Spline"),
            (Icon::XLine, "xline", "Construction Line"),
            (Icon::Ray, "ray", "Ray"),
            (Icon::Ellipse, "ellipse", "Ellipse"),
            (Icon::Polygon, "polygon", "Polygon"),
            (Icon::Point, "point.multiple", "Multiple Point"),
            (Icon::Donut, "donut", "Donut"),
            (Icon::RevCloud, "revcloud", "Revision Cloud"),
            (Icon::Region, "region", "Region"),
            (Icon::Wipeout, "wipeout", "Wipeout"),
        ],
    ),
    ("Hatch", &[(Icon::Hatch, "hatch", "Hatch"), (Icon::Gradient, "gradient", "Gradient"), (Icon::Boundary, "boundary", "Boundary")], &[]),
    (
        "Block",
        &[
            (Icon::BlockCreate, "block", "Create Block"),
            (Icon::Insert, "insert", "Insert Block"),
            (Icon::BlockEdit, "bedit", "Block Editor"),
            (Icon::AttDef, "attdef", "Define Attributes"),
        ],
        &[],
    ),
    (
        "Modify",
        &[(Icon::Move, "move", "Move")],
        &[
            (Icon::Copy, "copy", "Copy"),
            (Icon::Rotate, "rotate", "Rotate"),
            (Icon::Trim, "trim", "Trim"),
            (Icon::Fillet, "fillet", "Fillet"),
            (Icon::ArrayRect, "arrayrect", "Rectangular Array"),
            (Icon::Mirror, "mirror", "Mirror"),
            (Icon::Scale, "scale", "Scale"),
            (Icon::Extend, "extend", "Extend"),
            (Icon::Offset, "offset", "Offset"),
            (Icon::ArrayPolar, "arraypolar", "Polar Array"),
            (Icon::Stretch, "stretch", "Stretch"),
            (Icon::Chamfer, "chamfer", "Chamfer"),
            (Icon::Explode, "explode", "Explode"),
            (Icon::Break, "break", "Break"),
            (Icon::Join, "join", "Join"),
            (Icon::Erase, "erase", "Erase"),
            (Icon::Lengthen, "lengthen", "Lengthen"),
            (Icon::MatchProp, "matchprop", "Match Properties"),
        ],
    ),
    (
        "Text",
        &[(Icon::Text, "text", "Single Line Text"), (Icon::MText, "mtext", "Multiline Text")],
        &[(Icon::Table, "table", "Table"), (Icon::Search, "find", "Find and Replace")],
    ),
    (
        "Dimension",
        &[(Icon::DimLinear, "dimlinear", "Linear"), (Icon::DimAligned, "dimaligned", "Aligned")],
        &[
            (Icon::DimRadius, "dimradius", "Radius"),
            (Icon::DimDiameter, "dimdiameter", "Diameter"),
            (Icon::DimAngular, "dimangular", "Angular"),
            (Icon::DimArc, "dimarc", "Arc Length"),
            (Icon::DimOrdinate, "dimordinate", "Ordinate"),
            (Icon::DimBaseline, "dimbaseline", "Baseline"),
            (Icon::DimContinue, "dimcontinue", "Continue"),
            (Icon::QDim, "qdim", "Quick Dimension"),
        ],
    ),
    (
        "Leader",
        &[(Icon::MLeader, "mleader", "Multileader")],
        &[
            (Icon::AddLeader, "mleaderedit", "Add Leader"),
            (Icon::RemoveLeader, "mleaderedit.remove", "Remove Leader"),
            (Icon::AlignLeaders, "mleaderalign", "Align"),
        ],
    ),
    ("Table", &[(Icon::Table, "table", "Table")], &[]),
    (
        "Parametric",
        &[
            (Icon::Constraint, "autoconstrain", "Auto Constrain"),
            (Icon::DelConstraint, "delconstraint", "Delete Constraints"),
            (Icon::Parameters, "parameters", "Parameters Manager"),
        ],
        &[
            (Icon::Coincident, "gccoincident", "Coincident"),
            (Icon::Parallel, "gcparallel", "Parallel"),
            (Icon::Perpendicular, "gcperpendicular", "Perpendicular"),
            (Icon::Horizontal, "gchorizontal", "Horizontal"),
            (Icon::Vertical, "gcvertical", "Vertical"),
            (Icon::Tangent, "gctangent", "Tangent"),
            (Icon::Concentric, "gcconcentric", "Concentric"),
            (Icon::Equal, "gcequal", "Equal"),
            (Icon::Collinear, "gccollinear", "Collinear"),
            (Icon::Smooth, "gcsmooth", "Smooth"),
            (Icon::Symmetric, "gcsymmetric", "Symmetric"),
            (Icon::Fix, "gcfix", "Fix"),
            (Icon::DimLinear, "dclinear", "Linear"),
            (Icon::DimAligned, "dcaligned", "Aligned"),
            (Icon::DimAngular, "dcangular", "Angular"),
            (Icon::DimRadius, "dcradius", "Radius"),
            (Icon::DimDiameter, "dcdiameter", "Diameter"),
        ],
    ),
];

fn command_known(id: &str) -> bool {
    cadcraft_engine::find_command(id).is_some() || crate::menus::is_ui_command(id)
}

/// Tool Sets strip with Drafting/Modeling tabs, their command buttons and the collapse control.
pub fn toolsets(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    egui::Frame::NONE.fill(t.panel).show(ui, |ui| {
        let r = ui.max_rect();
        let p = ui.painter().clone();
        // Tabs.
        let tab_h = 28.0;
        let mut x = r.left();
        for name in ["Drafting", "Modeling"] {
            let w = 92.0;
            let tr = Rect::from_min_size(pos2(x, r.top()), vec2(w, tab_h));
            let active = app.ui.toolset_tab == name;
            let resp = ui.interact(tr, ui.scope_id().with(("ts", name)), Sense::click());
            p.rect_filled(tr, 0.0, if active { t.tab_active } else { t.chrome });
            p.text(
                tr.center(),
                egui::Align2::CENTER_CENTER,
                crate::i18n::t(name),
                egui::FontId::proportional(13.5),
                if active { t.text } else { t.text_dim },
            );
            icons::describe_control(ui, &resp, crate::i18n::t(name), Some(active), true);
            if resp.clicked() {
                app.ui.toolset_tab = name.into();
            }
            x += w;
        }
        p.rect_filled(Rect::from_min_max(pos2(x, r.top()), pos2(r.right(), r.top() + tab_h)), 0.0, t.chrome);
        let cr = Rect::from_center_size(pos2(r.right() - 14.0, r.top() + tab_h / 2.0), vec2(14.0, 14.0));
        let cresp = ui.interact(cr, ui.scope_id().with("ts_collapse"), Sense::click());
        icons::paint(&p, cr, Icon::ChevronLeft, false);
        icons::describe_control(ui, &cresp, "Collapse Tool Sets", None, false);
        if cresp.on_hover_text(crate::tl!("Collapse Tool Sets")).clicked() {
            app.ui.show_toolsets = false;
        }
        let body = Rect::from_min_max(pos2(r.left(), r.top() + tab_h), r.max);
        let mut clicked: Option<&'static str> = None;
        let mut toggle_group = None;
        let mut tool_visible: Option<(String, bool)> = None;
        let mut restore: Vec<String> = Vec::new();
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let groups: &[(&str, &[Tool], &[Tool])] = if app.ui.toolset_tab == "Drafting" { DRAFTING } else { MODELING };
                for (name, large, small) in groups {
                    let collapsed = app.ui.collapsed_groups.iter().any(|g| g == name);
                    // Header.
                    let (hr, hresp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                    let pp = ui.painter();
                    icons::paint(
                        pp,
                        Rect::from_center_size(pos2(hr.left() + 11.0, hr.center().y), vec2(11.0, 11.0)),
                        if collapsed { Icon::ChevronRight } else { Icon::ChevronDown },
                        false,
                    );
                    pp.text(
                        pos2(hr.left() + 22.0, hr.center().y),
                        egui::Align2::LEFT_CENTER,
                        crate::i18n::t(name),
                        egui::FontId::proportional(12.5),
                        t.text,
                    );
                    let gear = Rect::from_center_size(pos2(hr.right() - 12.0, hr.center().y), vec2(13.0, 13.0));
                    let gresp = ui.interact(gear, ui.scope_id().with(("ts_gear", name)), Sense::click());
                    icons::paint(pp, Rect::from_center_size(gear.center(), vec2(11.0, 11.0)), Icon::Gear, false);
                    icons::describe_control(ui, &hresp, name, Some(!collapsed), false);
                    icons::describe_control(ui, &gresp, &format!("{name} options"), None, false);
                    let gresp = gresp.on_hover_text(crate::tl!("Show and hide the tools of this category"));
                    if hresp.clicked() {
                        toggle_group = Some(name.to_string());
                    }
                    let hidden = |cmd: &str| app.ui.hidden_tools.iter().any(|h| h == cmd);
                    egui::Popup::from_toggle_button_response(&gresp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                        ui.label(crate::tl!("Show in Tool Sets"));
                        for (_, cmd, tip) in large.iter().chain(small.iter()) {
                            let mut shown = !hidden(cmd);
                            if ui.checkbox(&mut shown, *tip).changed() {
                                tool_visible = Some((cmd.to_string(), shown));
                            }
                        }
                        if ui.selectable_label(false, crate::tl!("Show All")).clicked() {
                            restore = large.iter().chain(small.iter()).map(|(_, cmd, _)| cmd.to_string()).collect();
                        }
                    });
                    if !collapsed {
                        ui.add_space(4.0);
                        // Large icons.
                        if !large.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
                                for (icon, cmd, tip) in large.iter() {
                                    if hidden(cmd) {
                                        continue;
                                    }
                                    ui.add_enabled_ui(command_known(cmd), |ui| {
                                        if icons::button(ui, *icon, 40.0, tip, false).clicked() {
                                            clicked = Some(cmd);
                                        }
                                    });
                                }
                            });
                        }
                        // Small icons, 8 per row.
                        let visible: Vec<Tool> = small.iter().copied().filter(|(_, cmd, _)| !hidden(cmd)).collect();
                        for row in visible.chunks(8) {
                            ui.add_space(2.0);
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
                                for (icon, cmd, tip) in row.iter() {
                                    ui.add_enabled_ui(command_known(cmd), |ui| {
                                        if icons::button(ui, *icon, 23.0, tip, false).clicked() {
                                            clicked = Some(cmd);
                                        }
                                    });
                                }
                            });
                        }
                        ui.add_space(6.0);
                    }
                    let (sr, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                    ui.painter().hline(sr.x_range().shrink(6.0), sr.center().y, Stroke::new(1.0, t.border));
                }
            });
        });
        if let Some((cmd, shown)) = tool_visible {
            if shown {
                app.ui.hidden_tools.retain(|h| h != &cmd);
            } else if !app.ui.hidden_tools.contains(&cmd) {
                app.ui.hidden_tools.push(cmd);
            }
        }
        if !restore.is_empty() {
            app.ui.hidden_tools.retain(|h| !restore.contains(h));
        }
        if let Some(g) = toggle_group {
            if let Some(i) = app.ui.collapsed_groups.iter().position(|x| *x == g) {
                app.ui.collapsed_groups.remove(i);
            } else {
                app.ui.collapsed_groups.push(g);
            }
        }
        if let Some(c) = clicked {
            app.cmd.buffer.clear();
            app.start(c);
        }
    });
}

pub const MODELING: &[(&str, &[Tool], &[Tool])] = &[
    (
        "Primitives",
        &[(Icon::Blocks, "box", "Box"), (Icon::Circle, "cylinder", "Cylinder"), (Icon::Polygon, "pyramid", "Pyramid")],
        &[(Icon::Ellipse, "sphere", "Sphere"), (Icon::Donut, "torus", "Torus"), (Icon::Arc, "cone", "Cone")],
    ),
    (
        "Solid",
        &[(Icon::Extend, "extrude", "Extrude"), (Icon::Rotate, "revolve", "Revolve")],
        &[(Icon::Spline, "sweep", "Sweep"), (Icon::Offset, "loft", "Loft")],
    ),
    ("Boolean", &[(Icon::Join, "union", "Union"), (Icon::Trim, "subtract", "Subtract"), (Icon::Region, "intersect", "Intersect")], &[]),
    ("View", &[(Icon::Orbit, "3dorbit", "Orbit"), (Icon::ZoomExtents, "zoom.extents", "Zoom Extents")], &[]),
];

fn section_header(ui: &mut egui::Ui, title: &str) -> Rect {
    let t = Tokens::get();
    let (hr, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    ui.painter().text(
        pos2(hr.left() + 10.0, hr.center().y),
        egui::Align2::LEFT_CENTER,
        crate::i18n::t(title),
        egui::FontId::proportional(14.0),
        t.text,
    );
    hr
}

fn swatch(p: &egui::Painter, r: Rect, c: Color) {
    let rgb = c.resolve(Color::Index(7), Color::Index(7));
    p.rect_filled(r, 1.0, Color32::from_rgb(rgb.0, rgb.1, rgb.2));
    p.rect_stroke(r, 1.0, Stroke::new(1.0, Color32::from_gray(30)), egui::StrokeKind::Inside);
}

/// The strip collapsed Tool Sets leave on the left edge, like a collapsed palette title bar:
/// without it the only way back was Window > Tool Sets (#1095). The whole strip expands them.
pub fn toolsets_strip(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    egui::Panel::left("cc_toolsets_strip").exact_size(22.0).resizable(false).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        let r = ui.max_rect();
        let resp = ui.interact(r, ui.scope_id().with("ts_expand"), Sense::click());
        icons::paint(ui.painter(), Rect::from_center_size(pos2(r.center().x, r.top() + 14.0), vec2(14.0, 14.0)), Icon::ChevronRight, false);
        icons::describe_control(ui, &resp, "Expand Tool Sets", None, false);
        if resp.on_hover_text(crate::tl!("Tool Sets")).clicked() {
            let _ = app.run("ui.toggle.toolsets", json!({"on": true}));
        }
    });
}

/// Right palette panel (Layers/Blocks/Properties icon strip and collapse control) containing the Layers and Properties sections.
pub fn right_palettes(app: &mut CadApp, ui: &mut egui::Ui) {
    if !app.ui.show_palettes {
        return;
    }
    let t = Tokens::get();
    egui::Panel::right("cc_palettes").exact_size(300.0).resizable(false).frame(egui::Frame::NONE.fill(t.panel)).show(ui, |ui| {
        let r = ui.max_rect();
        // Palette icon tabs.
        let p = ui.painter().clone();
        let tab_h = 30.0;
        p.rect_filled(Rect::from_min_size(r.min, vec2(r.width(), tab_h)), 0.0, t.chrome);
        for (i, icon) in [Icon::Layers, Icon::Blocks, Icon::Properties].iter().enumerate() {
            let br = Rect::from_center_size(pos2(r.left() + 30.0 + i as f32 * 52.0, r.top() + tab_h / 2.0), vec2(20.0, 20.0));
            icons::paint(&p, br, *icon, false);
        }
        let cr = Rect::from_center_size(pos2(r.right() - 14.0, r.top() + tab_h / 2.0), vec2(14.0, 14.0));
        let cresp = ui.interact(cr, ui.scope_id().with("palettes_collapse"), Sense::click());
        icons::paint(&p, cr, Icon::ChevronRight, false);
        icons::describe_control(ui, &cresp, "Collapse Palettes", None, false);
        if cresp.on_hover_text(crate::tl!("Collapse Palettes")).clicked() {
            app.ui.show_palettes = false;
        }
        let body = Rect::from_min_max(pos2(r.left(), r.top() + tab_h), r.max);
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                layers_section(app, ui);
                ui.add_space(8.0);
                properties_section(app, ui);
            });
        });
    });
}

pub(crate) fn layers_section(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    section_header(ui, "Layers");
    // Layer tools row.
    let mut cmd = None;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.spacing_mut().item_spacing = vec2(3.0, 0.0);
        for (icon, c, tip) in [
            (Icon::LayerProps, "ui.dialog.layers", "Layer Properties"),
            (Icon::MakeCurrent, "laymcur", "Make Object's Layer Current"),
            (Icon::LayerMatch, "laymch", "Match Layer"),
            (Icon::LayerPrev, "layerp", "Previous Layer"),
            (Icon::LayerIso, "layiso", "Isolate"),
            (Icon::LayerOff, "layoff", "Turn Off Layer"),
            (Icon::LayerFreeze, "layfrz", "Freeze Layer"),
            (Icon::LayerLock, "laylck", "Lock Layer"),
            (Icon::Unlock, "layulk", "Unlock Layer"),
        ] {
            if icons::button(ui, icon, 26.0, tip, false).clicked() {
                cmd = Some(c);
            }
        }
    });
    if let Some(c) = cmd {
        if c.starts_with("ui.") {
            app.start(c);
        } else {
            let _ = app.run(c, json!({}));
        }
    }
    ui.add_space(4.0);
    // Current layer combo.
    let Ok(d) = app.session.doc() else { return };
    let cur = d.header.str("CLAYER", "0");
    let layers: Vec<(String, bool, bool, bool, Color)> = d.layers.iter().map(|l| (l.name.clone(), l.on, l.frozen, l.locked, l.color)).collect();
    let cur_info = layers.iter().find(|l| l.0.eq_ignore_ascii_case(&cur)).cloned();
    let mut set_current = None;
    let mut toggle: Option<(String, &str, bool)> = None;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let w = ui.available_width() - 10.0;
        let (rect, resp) = ui.allocate_exact_size(vec2(w, 24.0), Sense::click());
        let p = ui.painter();
        p.rect_filled(rect, 3.0, t.control);
        if let Some((name, on, frozen, locked, color)) = &cur_info {
            let mut x = rect.left() + 6.0;
            for icon in [
                if *on { Icon::Bulb } else { Icon::BulbOff },
                if *locked { Icon::Lock } else { Icon::Unlock },
                if *frozen { Icon::Snowflake } else { Icon::Sun },
            ] {
                icons::paint(p, Rect::from_min_size(pos2(x, rect.top() + 4.0), vec2(16.0, 16.0)), icon, false);
                x += 18.0;
            }
            swatch(p, Rect::from_min_size(pos2(x + 2.0, rect.top() + 7.0), vec2(10.0, 10.0)), *color);
            p.text(pos2(x + 20.0, rect.center().y), egui::Align2::LEFT_CENTER, name, crate::theme::body(), t.text);
        }
        icons::paint(p, Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), vec2(12.0, 12.0)), Icon::ChevronDown, false);
        egui::Popup::from_toggle_button_response(&resp).width(w).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            for (name, on, frozen, locked, color) in &layers {
                ui.horizontal(|ui| {
                    if icons::button(ui, if *on { Icon::Bulb } else { Icon::BulbOff }, 18.0, "On/Off", false).clicked() {
                        toggle = Some((name.clone(), "on", !*on));
                    }
                    if icons::button(ui, if *frozen { Icon::Snowflake } else { Icon::Sun }, 18.0, "Freeze/Thaw", false).clicked() {
                        toggle = Some((name.clone(), "frozen", !*frozen));
                    }
                    if icons::button(ui, if *locked { Icon::Lock } else { Icon::Unlock }, 18.0, "Lock/Unlock", false).clicked() {
                        toggle = Some((name.clone(), "locked", !*locked));
                    }
                    let (sr, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    swatch(ui.painter(), sr, *color);
                    if ui.selectable_label(name.eq_ignore_ascii_case(&cur), name).clicked() {
                        set_current = Some(name.clone());
                    }
                });
            }
        });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let w = ui.available_width() - 10.0;
        let (rect, _) = ui.allocate_exact_size(vec2(w, 24.0), Sense::hover());
        ui.painter().rect_filled(rect, 3.0, t.control);
        ui.painter().text(
            pos2(rect.left() + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            crate::tl!("Unsaved Layer State"),
            crate::theme::body(),
            t.text,
        );
        icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), vec2(12.0, 12.0)), Icon::ChevronDown, false);
    });
    ui.add_space(4.0);
    let label = if app.ui.show_layer_list { "Hide Layer List" } else { "Show Layer List" };
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(160.0, 20.0), Sense::click());
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(rect.left() + 7.0, rect.center().y), vec2(11.0, 11.0)),
            if app.ui.show_layer_list { Icon::ChevronDown } else { Icon::ChevronRight },
            false,
        );
        ui.painter().text(pos2(rect.left() + 20.0, rect.center().y), egui::Align2::LEFT_CENTER, crate::i18n::t(label), crate::theme::body(), t.text);
        if resp.clicked() {
            app.ui.show_layer_list = !app.ui.show_layer_list;
        }
    });
    if app.ui.show_layer_list {
        ui.add_space(2.0);
        for (name, on, frozen, locked, color) in &layers {
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                if icons::button(ui, if *on { Icon::Bulb } else { Icon::BulbOff }, 18.0, "On/Off", false).clicked() {
                    toggle = Some((name.clone(), "on", !*on));
                }
                if icons::button(ui, if *frozen { Icon::Snowflake } else { Icon::Sun }, 18.0, "Freeze/Thaw", false).clicked() {
                    toggle = Some((name.clone(), "frozen", !*frozen));
                }
                if icons::button(ui, if *locked { Icon::Lock } else { Icon::Unlock }, 18.0, "Lock/Unlock", false).clicked() {
                    toggle = Some((name.clone(), "locked", !*locked));
                }
                let (sr, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                swatch(ui.painter(), sr, *color);
                let r = ui.selectable_label(name.eq_ignore_ascii_case(&cur), name);
                if r.clicked() {
                    set_current = Some(name.clone());
                }
                crate::shortcut_menus::attach(app, &r, |app| crate::shortcut_menus::layer_entries(app, name));
            });
        }
    }
    if let Some(n) = set_current {
        let _ = app.run("layer.current", json!({ "name": n }));
    }
    if let Some((n, k, v)) = toggle {
        let _ = app.run("layer.set", json!({ "name": n, k: v }));
    }
}

/// Properties row: a right-aligned translated label in a 106-point layout box that exposes a Label node to accessibility and names the control added by `add`.
fn prop_row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        let mut job = egui::text::LayoutJob::simple(crate::i18n::t(label).to_string(), crate::theme::body(), t.text_dim, 106.0);
        job.halign = egui::Align::RIGHT;
        let galley = ui.painter().layout_job(job);
        let (lr, label_response) = ui.allocate_exact_size(vec2(118.0, galley.size().y.max(22.0)), Sense::hover());
        label_response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), crate::i18n::t(label)));
        ui.painter().galley(pos2(lr.right() - 6.0, lr.center().y - galley.size().y / 2.0), galley, t.text_dim);
        ui.label_inputs_by(label_response.id, add);
    });
}

fn value_box(ui: &mut egui::Ui, text: &str, combo: bool, enabled: bool) -> egui::Response {
    let t = Tokens::get();
    let w = ui.available_width() - 10.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 20.0), Sense::click());
    ui.painter().rect_filled(rect, 2.0, if enabled { t.control } else { t.panel });
    if !enabled {
        ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0, t.control), egui::StrokeKind::Inside);
    }
    ui.painter().text(
        pos2(rect.left() + 6.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        crate::theme::body(),
        if enabled { t.text } else { t.text_faint },
    );
    if combo {
        icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.right() - 10.0, rect.center().y), vec2(11.0, 11.0)), Icon::ChevronDown, false);
    }
    resp
}

/// An editable numeric/text field that commits on Enter or focus loss.
fn edit_field(ui: &mut egui::Ui, id: egui::Id, value: String) -> Option<String> {
    let mut buf = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| value.clone());
    let w = ui.available_width() - 10.0;
    let te = id.with("te");
    let resp = ui.add_sized([w, 20.0], egui::TextEdit::singleline(&mut buf).id(te).font(crate::theme::body()));
    let mut out = None;
    // Keyboard focus from memory: `Response::has_focus` is false while the window itself is
    // unfocused, which dropped the typed text every frame.
    if ui.memory(|m| m.has_focus(te)) {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    if resp.lost_focus() && buf != value {
        out = Some(buf);
    }
    out
}

/// A committed numeric field read as the command line reads numbers (`36`, `3'`, `3'-6 1/2"`,
/// `6 1/2`). Text it can't read goes to `bad`, to be reported rather than dropped.
fn number(typed: Option<String>, bad: &mut Option<String>) -> Option<f64> {
    let typed = typed?;
    let v = cadcraft_engine::units::parse_distance(&typed);
    if v.is_none() {
        *bad = Some(typed);
    }
    v
}

/// Reports a numeric field the user typed that isn't a number; the field shows its value again.
fn report_bad(app: &mut CadApp, bad: Option<String>) {
    if let Some(text) = bad {
        app.session.echo(crate::tf!("Not a number or distance: {text}", text = text.trim()));
    }
}

/// A drop-down of `items` showing `current`; returns the item clicked.
fn pick_menu(ui: &mut egui::Ui, current: &str, items: impl IntoIterator<Item = String>) -> Option<String> {
    // Names are the drawing's own; only the ByLayer/ByBlock keywords are translated.
    let shown = |s: &str| if ["ByLayer", "ByBlock"].contains(&s) { crate::i18n::t(s).to_string() } else { s.to_string() };
    let mut out = None;
    ui.menu_button(shown(current), |ui| {
        for it in items {
            if ui.button(shown(&it)).clicked() {
                out = Some(it);
                ui.close();
            }
        }
    });
    out
}

/// A greyed value with no command behind it yet: visibly inert, never a silent dead button.
fn unavailable(ui: &mut egui::Ui, text: &str) {
    value_box(ui, crate::i18n::t(text), false, false).on_hover_text(crate::tl!("Not available yet"));
}

type Custom = Option<(String, Vec<cadcraft_engine::doc::dynblock::CustomProperty>)>;

/// The dynamic block name and custom properties of block reference `h`, worked out once per
/// drawing revision.
fn custom_properties(ui: &egui::Ui, app: &CadApp, h: cadcraft_engine::doc::Handle) -> Custom {
    let st = app.session.state().ok()?;
    let id = egui::Id::unique(("dynamic-block-custom", st.uid, st.revision, h));
    if let Some(c) = ui.data(|d| d.get_temp::<Custom>(id)) {
        return c;
    }
    let c = cadcraft_engine::doc::dynblock::custom_properties(&st.doc, h);
    ui.data_mut(|d| d.insert_temp(id, c.clone()));
    c
}

/// A greyed value computed from the geometry.
fn read_only(ui: &mut egui::Ui, text: &str) {
    value_box(ui, crate::i18n::t(text), false, false).on_hover_text(crate::tl!("Read-only"));
}

/// The Properties header: All/My, the selection-type filter and the PICKADD / Select Objects /
/// Quick Select buttons.
fn properties_header(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let hr = section_header(ui, "Properties");
    // All | My segmented control ("My" property sets don't exist yet).
    let seg = Rect::from_min_size(pos2(hr.right() - 120.0, hr.top() + 4.0), vec2(70.0, 18.0));
    ui.painter().rect_filled(seg, 3.0, t.chrome_dark);
    let half = Rect::from_min_size(seg.min, vec2(35.0, 18.0));
    let my = half.translate(vec2(35.0, 0.0));
    ui.painter().rect_filled(if app.ui.properties_all { half } else { my }, 3.0, t.control);
    ui.painter().text(half.center(), egui::Align2::CENTER_CENTER, crate::tl!("All"), crate::theme::small(), t.text);
    ui.painter().text(my.center(), egui::Align2::CENTER_CENTER, crate::tl!("My"), crate::theme::small(), t.text_faint);
    ui.interact(my, ui.scope_id().with("prop_my"), Sense::hover()).on_hover_text(crate::tl!("My properties: not available yet"));
    let sel = app.session.selection();
    let pickadd = app.session.settings.pickadd;
    let Ok(d) = app.session.doc() else { return };
    // Selected handles by type, in first-seen order.
    let mut types: Vec<(&'static str, Vec<String>)> = Vec::new();
    for h in &sel {
        if let Some(e) = d.entity(*h) {
            let n = e.kind.type_name();
            match types.iter_mut().find(|(t, _)| *t == n) {
                Some((_, hs)) => hs.push(h.hex()),
                None => types.push((n, vec![h.hex()])),
            }
        }
    }
    let count: usize = types.iter().map(|(_, hs)| hs.len()).sum();
    let header = match (count, types.as_slice()) {
        (0, _) => crate::tl!("No selection").to_string(),
        (1, [(n, _)]) => crate::i18n::t(n).to_string(),
        (n, [(ty, _)]) => format!("{} ({n})", crate::i18n::t(ty)),
        (n, _) => format!("{} ({n})", crate::tl!("All")),
    };
    let mut run: Option<(&str, Value)> = None;
    let mut start: Option<&str> = None;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.spacing_mut().item_spacing = vec2(3.0, 0.0);
        let w = (ui.available_width() - 80.0).max(60.0);
        let resp = ui.allocate_ui(vec2(w, 20.0), |ui| value_box(ui, &header, true, count > 0)).inner;
        if count > 0 {
            // Narrow the selection to one object type.
            egui::Popup::from_toggle_button_response(&resp).width(w - 10.0).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
                for (ty, hs) in &types {
                    if ui.button(format!("{} ({})", crate::i18n::t(ty), hs.len())).clicked() {
                        run = Some(("select", json!({ "handles": hs })));
                    }
                }
            });
        }
        let tip = if pickadd {
            crate::tl!("Toggle PICKADD (on: picks add to the selection)")
        } else {
            crate::tl!("Toggle PICKADD (off: each pick replaces the selection)")
        };
        if icons::button(ui, Icon::PickAdd, 22.0, tip, pickadd).clicked() {
            run = Some(("setvar", json!({ "name": "PICKADD", "value": i32::from(!pickadd) })));
        }
        if icons::button(ui, Icon::SelectObjects, 22.0, crate::tl!("Select Objects"), false).clicked() {
            start = Some("select");
        }
        if icons::button(ui, Icon::QuickSelect, 22.0, crate::tl!("Quick Select"), false).clicked() {
            start = Some("ui.dialog.qselect");
        }
    });
    if let Some((c, p)) = run {
        let _ = app.run(c, p);
    }
    if let Some(c) = start {
        app.start(c);
    }
}

/// Properties palette: the selection's common properties, or the current defaults when nothing is selected; formats lengths per drawing units and dispatches setter commands.
pub(crate) fn properties_section(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    properties_header(app, ui);
    let sel = app.session.selection();
    let Ok(d) = app.session.doc() else { return };
    ui.add_space(4.0);
    let h = &d.header;
    // Lengths are shown as the drawing's units say (UNITS), and read back in any format.
    let (lu, lp) = (h.i64("LUNITS", 2), h.i64("LUPREC", 4));
    let len = |v: f64| cadcraft_engine::units::format_distance(v, lu, lp);
    let mut bad: Option<String> = None;
    if sel.is_empty() {
        // Current properties for new objects; each row runs the command that sets it.
        let layers: Vec<String> = d.layers.iter().map(|l| l.name.clone()).collect();
        let linetypes: Vec<String> = d.linetypes.iter().map(|l| l.name.clone()).collect();
        let text_styles: Vec<String> = d.text_styles.iter().map(|l| l.name.clone()).collect();
        let dim_styles: Vec<String> = d.dim_styles.iter().map(|l| l.name.clone()).collect();
        let ml_styles: Vec<String> = d.mleader_styles.iter().map(|l| l.name.clone()).collect();
        let tb_styles: Vec<String> = d.table_styles.iter().map(|l| l.name.clone()).collect();
        let mut action: Option<(&str, Value)> = None;
        let mut act = |c: &'static str, p: Value| action = Some((c, p));
        prop_row(ui, "Color", |ui| {
            ui.menu_button(format!("■ {}", crate::i18n::t(&Color::from_aci(h.i64("CECOLOR", 256) as i16).name())), |ui| {
                for (n, c) in [
                    ("ByLayer", 256),
                    ("ByBlock", 0),
                    ("Red", 1),
                    ("Yellow", 2),
                    ("Green", 3),
                    ("Cyan", 4),
                    ("Blue", 5),
                    ("Magenta", 6),
                    ("White", 7),
                ] {
                    if ui.button(crate::i18n::t(n)).clicked() {
                        act("color", json!({ "color": c }));
                        ui.close();
                    }
                }
            });
        });
        prop_row(ui, "Layer", |ui| {
            if let Some(l) = pick_menu(ui, &h.str("CLAYER", "0"), layers) {
                act("layer.current", json!({ "name": l }));
            }
        });
        prop_row(ui, "Linetype", |ui| {
            let items = ["ByLayer", "ByBlock"]
                .map(String::from)
                .into_iter()
                .chain(linetypes.into_iter().filter(|l| !["ByLayer", "ByBlock"].contains(&l.as_str())));
            if let Some(l) = pick_menu(ui, &h.str("CELTYPE", "ByLayer"), items) {
                act("linetype", json!({ "current": l }));
            }
        });
        prop_row(ui, "Linetype scale", |ui| {
            if let Some(f) = number(edit_field(ui, ui.scope_id().with("cur_celtscale"), format!("{:.4}", h.f64("CELTSCALE", 1.0))), &mut bad) {
                act("setvar", json!({ "name": "CELTSCALE", "value": f }));
            }
        });
        prop_row(ui, "Lineweight", |ui| {
            ui.menu_button(crate::i18n::t(&cadcraft_doc::Lineweight::from_dxf(h.i64("CELWEIGHT", -1) as i16).name()), |ui| {
                for n in ["ByLayer", "ByBlock", "Default"] {
                    if ui.button(crate::i18n::t(n)).clicked() {
                        act("lweight", json!({ "lineweight": n }));
                        ui.close();
                    }
                }
                for v in cadcraft_doc::Lineweight::STANDARD {
                    if ui.button(format!("{:.2} mm", f64::from(v) / 100.0)).clicked() {
                        act("lweight", json!({ "lineweight": f64::from(v) / 100.0 }));
                        ui.close();
                    }
                }
            });
        });
        prop_row(ui, "Transparency", |ui| unavailable(ui, "ByLayer"));
        prop_row(ui, "Thickness", |ui| {
            if let Some(f) = number(edit_field(ui, ui.scope_id().with("cur_thickness"), len(h.f64("THICKNESS", 0.0))), &mut bad) {
                act("setvar", json!({ "name": "THICKNESS", "value": f }));
            }
        });
        prop_row(ui, "Text style", |ui| {
            if let Some(n) = pick_menu(ui, &h.str("TEXTSTYLE", "Standard"), text_styles) {
                act("style.current", json!({ "name": n }));
            }
        });
        prop_row(ui, "Dimension style", |ui| {
            if let Some(n) = pick_menu(ui, &h.str("DIMSTYLE", "Standard"), dim_styles) {
                act("dimstyle.current", json!({ "name": n }));
            }
        });
        prop_row(ui, "Multileader style", |ui| {
            if let Some(n) = pick_menu(ui, &h.str("CMLEADERSTYLE", "Standard"), ml_styles) {
                act("mleaderstyle", json!({ "name": n, "current": true }));
            }
        });
        prop_row(ui, "Table style", |ui| {
            if let Some(n) = pick_menu(ui, &h.str("CTABLESTYLE", "Standard"), tb_styles) {
                act("tablestyle", json!({ "name": n, "current": true }));
            }
        });
        prop_row(ui, "Annotation scale", |ui| unavailable(ui, "1:1"));
        prop_row(ui, "Text height", |ui| {
            if let Some(f) = number(edit_field(ui, ui.scope_id().with("cur_textsize"), len(h.f64("TEXTSIZE", 0.2))), &mut bad) {
                act("setvar", json!({ "name": "TEXTSIZE", "value": f }));
            }
        });
        // The current layout's plot style table; named tables give the current plot style.
        let space = app.session.layout_space();
        let layout = space.layout().and_then(|n| d.layout(n).map(|l| (n.to_string(), l.page.plot_style_table.clone())));
        let table = layout.as_ref().and_then(|(_, t)| cadcraft_doc::plot_style_table(d, t));
        match table.as_ref().filter(|t| !t.is_color_dependent()) {
            Some(t) => prop_row(ui, "Plot style", |ui| {
                let items = ["ByLayer", "ByBlock"].map(String::from).into_iter().chain(t.styles.iter().map(|s| s.name.clone()));
                if let Some(n) = pick_menu(ui, &h.str("CPLOTSTYLE", "ByLayer"), items) {
                    act("plotstyle", json!({ "name": n }));
                }
            }),
            None => prop_row(ui, "Plot style", |ui| read_only(ui, "ByColor")),
        }
        match &layout {
            Some((name, current)) => prop_row(ui, "Plot style table", |ui| {
                let shown = if current.trim().is_empty() { "None".to_string() } else { current.clone() };
                let items = std::iter::once("None".to_string()).chain(cadcraft_doc::plot_style_table_names(d));
                if let Some(n) = pick_menu(ui, &shown, items) {
                    act("pagesetup", json!({ "layout": name, "plotStyleTable": n }));
                }
            }),
            None => prop_row(ui, "Plot style table", |ui| unavailable(ui, "None")),
        }
        let attached = layout.as_ref().map(|(n, _)| n.clone()).unwrap_or_else(|| "Model".into());
        prop_row(ui, "Plot style attached to", |ui| read_only(ui, &attached));
        let kind = match &table {
            Some(t) if t.is_color_dependent() => "Color-dependent",
            Some(_) => "Named",
            None => "Not available",
        };
        prop_row(ui, "Plot table type", |ui| read_only(ui, kind));
        if let Some((c, p)) = action {
            let _ = app.run(c, p);
        }
        report_bad(app, bad);
        return;
    }
    // Selected objects: common + geometry for a single object.
    let first = sel.first().and_then(|h| d.entity(*h)).map(|e| (**e).clone());
    let Some(e) = first else { return };
    let same = |f: &dyn Fn(&cadcraft_doc::Entity) -> String| -> String {
        let vals: Vec<String> = sel.iter().filter_map(|h| d.entity(*h)).map(|e| f(e)).collect();
        match vals.first() {
            Some(v0) if vals.iter().all(|v| v == v0) => v0.clone(),
            _ => "*VARIES*".into(),
        }
    };
    let selected_color = Some(e.common.color).filter(|c| sel.iter().filter_map(|h| d.entity(*h)).all(|e| e.common.color == *c));
    let color = selected_color.map_or_else(|| "*VARIES*".into(), Color::name);
    let effective_color = |e: &cadcraft_doc::Entity| {
        let layer = d.layer(&e.common.layer).map_or(Color::WHITE, |l| l.color);
        e.common.color.resolve(layer, Color::WHITE)
    };
    let resolved_color = sel
        .first()
        .and_then(|h| d.entity(*h))
        .map(|e| effective_color(e))
        .filter(|c| sel.iter().filter_map(|h| d.entity(*h)).all(|e| effective_color(e) == *c))
        .unwrap_or(Rgb(255, 255, 255));
    let layer = same(&|e| e.common.layer.clone());
    let linetype = same(&|e| e.common.linetype.clone());
    let lts = same(&|e| format!("{:.4}", e.common.ltscale));
    let lw = same(&|e| e.common.lineweight.name());
    let tr = same(&|e| e.common.transparency.name());
    let layers: Vec<String> = d.layers.iter().map(|l| l.name.clone()).collect();
    let linetypes: Vec<String> = d.linetypes.iter().map(|l| l.name.clone()).collect();
    let ids: Vec<String> = sel.iter().map(|h| h.hex()).collect();
    let mut set: Option<Value> = None;
    let mut custom_set: Option<Value> = None;
    let group = |ui: &mut egui::Ui, title: &str| {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
        ui.painter().rect_filled(r.shrink2(vec2(6.0, 1.0)), 2.0, t.chrome_dark);
        ui.painter().text(pos2(r.left() + 12.0, r.center().y), egui::Align2::LEFT_CENTER, crate::i18n::t(title), crate::theme::small(), t.text_dim);
    };
    group(ui, "General");
    prop_row(ui, "Color", |ui| {
        let width = (ui.available_width() - 10.0).max(0.0);
        if let Some(c) = crate::layers::color_button_resolved(
            ui,
            egui::Id::unique(("properties_color", &ids)),
            selected_color.unwrap_or(Color::ByLayer),
            resolved_color,
            &color,
            true,
            width,
        ) {
            set = Some(json!({ "handles": ids, "color": c.name() }));
        }
    });
    prop_row(ui, "Layer", |ui| {
        ui.menu_button(layer.clone(), |ui| {
            egui::ScrollArea::vertical().id_salt("properties_layer_menu").max_height(300.0).show(ui, |ui| {
                for l in &layers {
                    if ui.button(l).clicked() {
                        set = Some(json!({ "handles": ids, "layer": l }));
                        ui.close();
                    }
                }
            });
        });
    });
    prop_row(ui, "Linetype", |ui| {
        ui.menu_button(if ["ByLayer", "ByBlock", "*VARIES*"].contains(&linetype.as_str()) { crate::i18n::t(&linetype) } else { &linetype }, |ui| {
            for l in std::iter::once("ByLayer".to_string())
                .chain(std::iter::once("ByBlock".to_string()))
                .chain(linetypes.iter().filter(|l| !["ByLayer", "ByBlock"].contains(&l.as_str())).cloned())
            {
                if ui.button(if ["ByLayer", "ByBlock"].contains(&l.as_str()) { crate::i18n::t(&l) } else { &l }).clicked() {
                    set = Some(json!({ "handles": ids, "linetype": l }));
                    ui.close();
                }
            }
        });
    });
    prop_row(ui, "Linetype scale", |ui| {
        if let Some(f) = number(edit_field(ui, ui.scope_id().with(("lts", &ids)), lts.clone()), &mut bad) {
            set = Some(json!({ "handles": ids, "ltscale": f }));
        }
    });
    prop_row(ui, "Lineweight", |ui| {
        ui.menu_button(crate::i18n::t(&lw), |ui| {
            for n in ["ByLayer", "ByBlock", "Default"] {
                if ui.button(crate::i18n::t(n)).clicked() {
                    set = Some(json!({ "handles": ids, "lineweight": n }));
                    ui.close();
                }
            }
            for v in cadcraft_doc::Lineweight::STANDARD {
                if ui.button(format!("{:.2} mm", f64::from(v) / 100.0)).clicked() {
                    set = Some(json!({ "handles": ids, "lineweight": f64::from(v) / 100.0 }));
                    ui.close();
                }
            }
        });
    });
    prop_row(ui, "Transparency", |ui| {
        ui.menu_button(crate::i18n::t(&tr), |ui| {
            for v in ["ByLayer", "ByBlock"] {
                if ui.button(crate::i18n::t(v)).clicked() {
                    set = Some(json!({ "handles": ids, "transparency": v }));
                    ui.close();
                }
            }
            ui.separator();
            // A fixed percentage, committed when the drag or edit ends.
            let tid = egui::Id::unique(("prop_tr", &ids));
            let cur = tr.parse::<u8>().ok();
            let mut pct = ui.data_mut(|d| d.get_temp::<u8>(tid)).or(cur).unwrap_or(0);
            ui.horizontal(|ui| {
                ui.label(crate::tl!("Percent"));
                let r = ui.add(egui::DragValue::new(&mut pct).range(0..=90).speed(0.5));
                if r.dragged() || r.has_focus() {
                    ui.data_mut(|d| d.insert_temp(tid, pct));
                }
                if r.drag_stopped() || r.lost_focus() {
                    if cur != Some(pct) {
                        set = Some(json!({ "handles": ids, "transparency": pct }));
                    }
                    ui.data_mut(|d| d.remove::<u8>(tid));
                }
            });
        });
    });
    if sel.len() == 1 {
        group(ui, "Geometry");
        let real = |v: f64| format!("{v:.4}");
        // Rotations show as the drawing measures directions (ANGBASE, ANGDIR).
        let angles = app.session.angle_settings();
        let num = |ui: &mut egui::Ui, label: &str, key: &str, shown: String, set: &mut Option<Value>, bad: &mut Option<String>, ids: &Vec<String>| {
            prop_row(ui, label, |ui| {
                if let Some(f) = number(edit_field(ui, ui.scope_id().with((key, ids)), shown), bad) {
                    *set = Some(json!({ "handles": ids, key: f }));
                }
            });
        };
        match &e.kind {
            EntityKind::Line(l) => {
                prop_row(ui, "Start X", |ui| {
                    if let Some(f) = number(edit_field(ui, ui.scope_id().with(("sx", &ids)), len(l.a.x)), &mut bad) {
                        set = Some(json!({ "handles": ids, "start": [f, l.a.y] }));
                    }
                });
                prop_row(ui, "Start Y", |ui| {
                    if let Some(f) = number(edit_field(ui, ui.scope_id().with(("sy", &ids)), len(l.a.y)), &mut bad) {
                        set = Some(json!({ "handles": ids, "start": [l.a.x, f] }));
                    }
                });
                prop_row(ui, "End X", |ui| {
                    if let Some(f) = number(edit_field(ui, ui.scope_id().with(("ex", &ids)), len(l.b.x)), &mut bad) {
                        set = Some(json!({ "handles": ids, "end": [f, l.b.y] }));
                    }
                });
                prop_row(ui, "End Y", |ui| {
                    if let Some(f) = number(edit_field(ui, ui.scope_id().with(("ey", &ids)), len(l.b.y)), &mut bad) {
                        set = Some(json!({ "handles": ids, "end": [l.b.x, f] }));
                    }
                });
                let dlt = l.b.xy() - l.a.xy();
                prop_row(ui, "Delta X", |ui| read_only(ui, &len(dlt.x)));
                prop_row(ui, "Delta Y", |ui| read_only(ui, &len(dlt.y)));
                // Length and angle keep the start point and move the end point.
                num(ui, "Length", "length", len(dlt.len()), &mut set, &mut bad, &ids);
                num(ui, "Angle", "angle", real(dlt.angle().to_degrees()), &mut set, &mut bad, &ids);
            }
            EntityKind::Circle(c) => {
                num(ui, "Center X", "cx", len(c.center.x), &mut set, &mut bad, &ids);
                num(ui, "Center Y", "cy", len(c.center.y), &mut set, &mut bad, &ids);
                num(ui, "Radius", "radius", len(c.radius), &mut set, &mut bad, &ids);
                num(ui, "Diameter", "diameter", len(c.radius * 2.0), &mut set, &mut bad, &ids);
                num(ui, "Circumference", "circumference", len(c.radius * std::f64::consts::TAU), &mut set, &mut bad, &ids);
                num(ui, "Area", "area", real(c.radius * c.radius * std::f64::consts::PI), &mut set, &mut bad, &ids);
                if let Some(v) = &mut set
                    && let Some(o) = v.as_object_mut()
                {
                    if let Some(x) = o.remove("cx") {
                        o.insert("center".into(), json!([x, c.center.y]));
                    }
                    if let Some(y) = o.remove("cy") {
                        o.insert("center".into(), json!([c.center.x, y]));
                    }
                }
            }
            EntityKind::Arc(a) => {
                num(ui, "Radius", "radius", len(a.radius), &mut set, &mut bad, &ids);
                num(ui, "Start angle", "startAngle", real(a.start.to_degrees()), &mut set, &mut bad, &ids);
                num(ui, "End angle", "endAngle", real(a.end.to_degrees()), &mut set, &mut bad, &ids);
            }
            EntityKind::Text(tx) => {
                prop_row(ui, "Contents", |ui| {
                    if let Some(s) = edit_field(ui, ui.scope_id().with(("txt", &ids)), tx.value.clone()) {
                        set = Some(json!({ "handles": ids, "text": s }));
                    }
                });
                num(ui, "Height", "height", len(tx.height), &mut set, &mut bad, &ids);
                num(ui, "Rotation", "rotation", real(angles.shown(tx.rotation).to_degrees()), &mut set, &mut bad, &ids);
                num(ui, "Width factor", "widthFactor", real(tx.width_factor), &mut set, &mut bad, &ids);
            }
            EntityKind::MText(tx) => {
                prop_row(ui, "Contents", |ui| {
                    if let Some(s) = edit_field(ui, ui.scope_id().with(("mtxt", &ids)), tx.contents.clone()) {
                        set = Some(json!({ "handles": ids, "text": s }));
                    }
                });
                num(ui, "Text height", "height", len(tx.height), &mut set, &mut bad, &ids);
                num(ui, "Defined width", "width", len(tx.width), &mut set, &mut bad, &ids);
            }
            EntityKind::LwPolyline(pl) => {
                num(ui, "Global width", "width", len(pl.const_width), &mut set, &mut bad, &ids);
                let g = cadcraft_geom::Polyline { vertices: pl.vertices.clone(), closed: pl.closed };
                prop_row(ui, "Length", |ui| read_only(ui, &len(g.len())));
                if pl.closed {
                    prop_row(ui, "Area", |ui| read_only(ui, &format!("{:.4}", g.area().abs())));
                }
                prop_row(ui, "Closed", |ui| {
                    let mut c = pl.closed;
                    let label = if c { "Yes" } else { "No" };
                    if ui.checkbox(&mut c, crate::i18n::t(label)).changed() {
                        set = Some(json!({ "handles": ids, "closed": c }));
                    }
                });
            }
            EntityKind::Hatch(hh) => {
                prop_row(ui, "Pattern", |ui| {
                    ui.menu_button(hh.pattern.clone(), |ui| {
                        if let Some(name) = patterns::menu(ui, &hh.pattern) {
                            set = Some(json!({ "handles": ids, "pattern": name }));
                            ui.close();
                        }
                    });
                });
                num(ui, "Scale", "scale", real(hh.scale), &mut set, &mut bad, &ids);
                num(ui, "Angle", "angle", real(hh.angle.to_degrees()), &mut set, &mut bad, &ids);
            }
            EntityKind::Dimension(dm) => {
                prop_row(ui, "Text override", |ui| {
                    if let Some(s) = edit_field(ui, ui.scope_id().with(("dimtxt", &ids)), dm.text.clone()) {
                        set = Some(json!({ "handles": ids, "textOverride": s }));
                    }
                });
                prop_row(ui, "Dim style", |ui| {
                    if let Some(n) = pick_menu(ui, &dm.style, d.dim_styles.iter().map(|st| st.name.clone())) {
                        set = Some(json!({ "handles": ids, "dimStyle": n }));
                    }
                });
                // The style's values with this dimension's own overrides; editing one overrides
                // it for the selected dimensions, as AutoCAD's Properties palette does.
                let st = d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides);
                num(ui, "Text height", "textHeight", len(st.text_height), &mut set, &mut bad, &ids);
                num(ui, "Arrow size", "arrowSize", len(st.arrow_size), &mut set, &mut bad, &ids);
                num(ui, "Dim scale overall", "dimScale", real(st.scale), &mut set, &mut bad, &ids);
                prop_row(ui, "Precision", |ui| {
                    let cur = st.decimals.to_string();
                    if let Some(n) = pick_menu(ui, &cur, (0..=8).map(|n: u8| n.to_string())).and_then(|n| n.parse::<u8>().ok()) {
                        set = Some(json!({ "handles": ids, "precision": n }));
                    }
                });
            }
            EntityKind::Insert(i) => {
                // A dynamic block reference shows its dynamic block's name, not its anonymous
                // block's (`*U12`), and its custom properties below.
                let custom = custom_properties(ui, app, e.handle);
                let name = custom.as_ref().map_or_else(|| i.block.clone(), |(n, _)| n.clone());
                prop_row(ui, "Name", |ui| read_only(ui, &name));
                prop_row(ui, "Position X", |ui| read_only(ui, &len(i.insert.x)));
                prop_row(ui, "Position Y", |ui| read_only(ui, &len(i.insert.y)));
                prop_row(ui, "Position Z", |ui| read_only(ui, &len(i.insert.z)));
                num(ui, "Rotation", "rotation", real(angles.shown(i.rotation).to_degrees()), &mut set, &mut bad, &ids);
                num(ui, "Scale", "scale", real(i.scale.x), &mut set, &mut bad, &ids);
                if let Some((_, props)) = custom.filter(|(_, p)| !p.is_empty()) {
                    group(ui, "Custom");
                    for p in &props {
                        prop_row(ui, &p.name, |ui| {
                            let picked = match (p.editable, p.choices.is_empty()) {
                                (true, false) => pick_menu(ui, &p.value, p.choices.iter().cloned()),
                                (true, true) => edit_field(ui, ui.scope_id().with(("dynprop", e.handle, &p.name)), p.value.clone()),
                                (false, _) => {
                                    read_only(ui, &p.value);
                                    None
                                }
                            };
                            if let Some(v) = picked.filter(|v| *v != p.value) {
                                custom_set = Some(json!({ "handle": e.handle.hex(), "name": p.name, "value": v }));
                            }
                        });
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(mut p) = set {
        // A rotation is typed as shown (from ANGBASE, in the ANGDIR sense); properties.set takes it
        // from +X counterclockwise.
        if let Some(r) = p.get_mut("rotation")
            && let Some(v) = r.as_f64()
        {
            *r = json!(app.session.angle_settings().unshown(v.to_radians()).to_degrees());
        }
        let _ = app.run("properties.set", p);
    }
    report_bad(app, bad);
    if let Some(p) = custom_set {
        let _ = app.run("blocks.reference.set", p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Services;
    use cadcraft_engine::Session;
    use egui::{Color32, Shape};

    fn contains_swatch(shape: &Shape, color: Color32) -> bool {
        match shape {
            Shape::Rect(r) => r.fill == color && (r.rect.width() - 12.0).abs() < 0.01 && (r.rect.height() - 12.0).abs() < 0.01,
            Shape::Vec(shapes) => shapes.iter().any(|shape| contains_swatch(shape, color)),
            _ => false,
        }
    }

    #[test]
    fn selected_bylayer_entity_paints_its_layer_color_swatch() {
        let mut app = CadApp::new(Session::default(), Services::default());
        app.run("layer.new", json!({ "name": "Blue", "color": 5, "current": true })).unwrap();
        let line = app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap();
        app.run("select", json!({ "handles": [line["handles"][0].clone()] })).unwrap();

        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 1200.0))), ..Default::default() };
        let mut painted_blue = false;
        for _ in 0..2 {
            app.logic(&ctx);
            let mut output = ctx.run_ui(input.clone(), |ui| properties_section(&mut app, ui));
            painted_blue = output.shapes.iter().any(|shape| contains_swatch(&shape.shape, Color32::from_rgb(0, 0, 255)));
            output.textures_delta.clear();
        }
        assert!(painted_blue, "a ByLayer entity on a blue layer needs a blue Properties swatch");
    }
}
