//! Tool Sets (left) and the Layers / Properties palettes (right).

use cadcraft_color::Color;
use cadcraft_doc::EntityKind;
use egui::{Color32, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

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
        &[(Icon::Constraint, "autoconstrain", "Auto Constrain")],
        &[
            (Icon::Coincident, "gccoincident", "Coincident"),
            (Icon::Parallel, "gcparallel", "Parallel"),
            (Icon::Perpendicular, "gcperpendicular", "Perpendicular"),
            (Icon::Horizontal, "gchorizontal", "Horizontal"),
            (Icon::Vertical, "gcvertical", "Vertical"),
            (Icon::Tangent, "gctangent", "Tangent"),
            (Icon::Concentric, "gcconcentric", "Concentric"),
            (Icon::Equal, "gcequal", "Equal"),
        ],
    ),
];

fn command_known(id: &str) -> bool {
    cadcraft_engine::find_command(id).is_some() || crate::menus::is_ui_command(id)
}

pub fn toolsets(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    egui::Panel::left("cc_toolsets").exact_size(220.0).resizable(false).frame(egui::Frame::NONE.fill(t.panel)).show(ui, |ui| {
        let r = ui.max_rect();
        let p = ui.painter().clone();
        // Tabs.
        let tab_h = 28.0;
        let mut x = r.left();
        for name in ["Drafting", "Modeling"] {
            let w = 92.0;
            let tr = Rect::from_min_size(pos2(x, r.top()), vec2(w, tab_h));
            let active = app.ui.toolset_tab == name;
            let resp = ui.interact(tr, ui.id().with(("ts", name)), Sense::click());
            p.rect_filled(tr, 0.0, if active { t.tab_active } else { t.chrome });
            p.text(tr.center(), egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(13.5), if active { t.text } else { t.text_dim });
            if resp.clicked() {
                app.ui.toolset_tab = name.into();
            }
            x += w;
        }
        p.rect_filled(Rect::from_min_max(pos2(x, r.top()), pos2(r.right(), r.top() + tab_h)), 0.0, t.chrome);
        let cr = Rect::from_center_size(pos2(r.right() - 14.0, r.top() + tab_h / 2.0), vec2(14.0, 14.0));
        let cresp = ui.interact(cr, ui.id().with("ts_collapse"), Sense::click());
        icons::paint(&p, cr, Icon::ChevronLeft, false);
        if cresp.on_hover_text("Collapse Tool Sets").clicked() {
            app.ui.show_toolsets = false;
        }
        let body = Rect::from_min_max(pos2(r.left(), r.top() + tab_h), r.max);
        let mut clicked: Option<&'static str> = None;
        let mut toggle_group = None;
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
                    pp.text(pos2(hr.left() + 22.0, hr.center().y), egui::Align2::LEFT_CENTER, *name, egui::FontId::proportional(12.5), t.text);
                    icons::paint(pp, Rect::from_center_size(pos2(hr.right() - 12.0, hr.center().y), vec2(11.0, 11.0)), Icon::Gear, false);
                    if hresp.clicked() {
                        toggle_group = Some(name.to_string());
                    }
                    if !collapsed {
                        ui.add_space(4.0);
                        // Large icons.
                        if !large.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
                                for (icon, cmd, tip) in large.iter() {
                                    ui.add_enabled_ui(command_known(cmd), |ui| {
                                        if icons::button(ui, *icon, 40.0, tip, false).clicked() {
                                            clicked = Some(cmd);
                                        }
                                    });
                                }
                            });
                        }
                        // Small icons, 8 per row.
                        for row in small.chunks(8) {
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
    ui.painter().text(pos2(hr.left() + 10.0, hr.center().y), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(14.0), t.text);
    hr
}

fn swatch(p: &egui::Painter, r: Rect, c: Color) {
    let rgb = c.resolve(Color::Index(7), Color::Index(7));
    p.rect_filled(r, 1.0, Color32::from_rgb(rgb.0, rgb.1, rgb.2));
    p.rect_stroke(r, 1.0, Stroke::new(1.0, Color32::from_gray(30)), egui::StrokeKind::Inside);
}

pub fn right_palettes(app: &mut CadApp, ui: &mut egui::Ui) {
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
        icons::paint(&p, Rect::from_center_size(pos2(r.right() - 14.0, r.top() + tab_h / 2.0), vec2(14.0, 14.0)), Icon::ChevronRight, false);
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

fn layers_section(app: &mut CadApp, ui: &mut egui::Ui) {
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
        ui.painter().text(pos2(rect.left() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, "Unsaved Layer State", crate::theme::body(), t.text);
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
        ui.painter().text(pos2(rect.left() + 20.0, rect.center().y), egui::Align2::LEFT_CENTER, label, crate::theme::body(), t.text);
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
                if ui.selectable_label(name.eq_ignore_ascii_case(&cur), name).clicked() {
                    set_current = Some(name.clone());
                }
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

fn prop_row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        let (lr, _) = ui.allocate_exact_size(vec2(118.0, 22.0), Sense::hover());
        ui.painter().text(pos2(lr.right() - 6.0, lr.center().y), egui::Align2::RIGHT_CENTER, label, crate::theme::body(), t.text_dim);
        add(ui);
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

/// A drop-down of `items` showing `current`; returns the item clicked.
fn pick_menu(ui: &mut egui::Ui, current: &str, items: impl IntoIterator<Item = String>) -> Option<String> {
    let mut out = None;
    ui.menu_button(current.to_string(), |ui| {
        for it in items {
            if ui.button(&it).clicked() {
                out = Some(it);
                ui.close();
            }
        }
    });
    out
}

/// A greyed value with no command behind it yet: visibly inert, never a silent dead button.
fn unavailable(ui: &mut egui::Ui, text: &str) {
    value_box(ui, text, false, false).on_hover_text("Not available yet");
}

/// A greyed value computed from the geometry.
fn read_only(ui: &mut egui::Ui, text: &str) {
    value_box(ui, text, false, false).on_hover_text("Read-only");
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
    ui.painter().text(half.center(), egui::Align2::CENTER_CENTER, "All", crate::theme::small(), t.text);
    ui.painter().text(my.center(), egui::Align2::CENTER_CENTER, "My", crate::theme::small(), t.text_faint);
    ui.interact(my, ui.id().with("prop_my"), Sense::hover()).on_hover_text("My properties: not available yet");
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
        (0, _) => "No selection".to_string(),
        (1, [(n, _)]) => n.to_string(),
        (n, [(ty, _)]) => format!("{ty} ({n})"),
        (n, _) => format!("All ({n})"),
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
                    if ui.button(format!("{ty} ({})", hs.len())).clicked() {
                        run = Some(("select", json!({ "handles": hs })));
                    }
                }
            });
        }
        let tip = if pickadd { "Toggle PICKADD (on: picks add to the selection)" } else { "Toggle PICKADD (off: each pick replaces the selection)" };
        if icons::button(ui, Icon::PickAdd, 22.0, tip, pickadd).clicked() {
            run = Some(("setvar", json!({ "name": "PICKADD", "value": i32::from(!pickadd) })));
        }
        if icons::button(ui, Icon::SelectObjects, 22.0, "Select Objects", false).clicked() {
            start = Some("select");
        }
        if icons::button(ui, Icon::QuickSelect, 22.0, "Quick Select", false).clicked() {
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

fn properties_section(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    properties_header(app, ui);
    let sel = app.session.selection();
    let Ok(d) = app.session.doc() else { return };
    ui.add_space(4.0);
    let h = &d.header;
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
            ui.menu_button(format!("■ {}", Color::from_aci(h.i64("CECOLOR", 256) as i16).name()), |ui| {
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
                    if ui.button(n).clicked() {
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
            if let Some(v) = edit_field(ui, ui.id().with("cur_celtscale"), format!("{:.4}", h.f64("CELTSCALE", 1.0)))
                && let Ok(f) = v.trim().parse::<f64>()
            {
                act("setvar", json!({ "name": "CELTSCALE", "value": f }));
            }
        });
        prop_row(ui, "Lineweight", |ui| {
            ui.menu_button(cadcraft_doc::Lineweight::from_dxf(h.i64("CELWEIGHT", -1) as i16).name(), |ui| {
                for n in ["ByLayer", "ByBlock", "Default"] {
                    if ui.button(n).clicked() {
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
            if let Some(v) = edit_field(ui, ui.id().with("cur_thickness"), format!("{:.4}", h.f64("THICKNESS", 0.0)))
                && let Ok(f) = v.trim().parse::<f64>()
            {
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
            if let Some(v) = edit_field(ui, ui.id().with("cur_textsize"), format!("{:.4}", h.f64("TEXTSIZE", 0.2)))
                && let Ok(f) = v.trim().parse::<f64>()
            {
                act("setvar", json!({ "name": "TEXTSIZE", "value": f }));
            }
        });
        prop_row(ui, "Plot style", |ui| unavailable(ui, "ByColor"));
        prop_row(ui, "Plot style table", |ui| unavailable(ui, "None"));
        prop_row(ui, "Plot style attached to", |ui| read_only(ui, "Model"));
        prop_row(ui, "Plot table type", |ui| read_only(ui, "Not available"));
        if let Some((c, p)) = action {
            let _ = app.run(c, p);
        }
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
    let color = same(&|e| e.common.color.name());
    let layer = same(&|e| e.common.layer.clone());
    let linetype = same(&|e| e.common.linetype.clone());
    let lts = same(&|e| format!("{:.4}", e.common.ltscale));
    let lw = same(&|e| e.common.lineweight.name());
    let tr = same(&|e| e.common.transparency.name());
    let layers: Vec<String> = d.layers.iter().map(|l| l.name.clone()).collect();
    let linetypes: Vec<String> = d.linetypes.iter().map(|l| l.name.clone()).collect();
    let ids: Vec<String> = sel.iter().map(|h| h.hex()).collect();
    let mut set: Option<Value> = None;
    let group = |ui: &mut egui::Ui, title: &str| {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
        ui.painter().rect_filled(r.shrink2(vec2(6.0, 1.0)), 2.0, t.chrome_dark);
        ui.painter().text(pos2(r.left() + 12.0, r.center().y), egui::Align2::LEFT_CENTER, title, crate::theme::small(), t.text_dim);
    };
    group(ui, "General");
    prop_row(ui, "Color", |ui| {
        ui.menu_button(format!("■ {color}"), |ui| {
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
                ("Color 8", 8),
                ("Color 9", 9),
            ] {
                if ui.button(n).clicked() {
                    set = Some(json!({ "handles": ids, "color": c }));
                    ui.close();
                }
            }
        });
    });
    prop_row(ui, "Layer", |ui| {
        ui.menu_button(layer.clone(), |ui| {
            for l in &layers {
                if ui.button(l).clicked() {
                    set = Some(json!({ "handles": ids, "layer": l }));
                    ui.close();
                }
            }
        });
    });
    prop_row(ui, "Linetype", |ui| {
        ui.menu_button(linetype.clone(), |ui| {
            for l in std::iter::once("ByLayer".to_string())
                .chain(std::iter::once("ByBlock".to_string()))
                .chain(linetypes.iter().filter(|l| !["ByLayer", "ByBlock"].contains(&l.as_str())).cloned())
            {
                if ui.button(&l).clicked() {
                    set = Some(json!({ "handles": ids, "linetype": l }));
                    ui.close();
                }
            }
        });
    });
    prop_row(ui, "Linetype scale", |ui| {
        if let Some(v) = edit_field(ui, ui.id().with(("lts", &ids)), lts.clone())
            && let Ok(f) = v.trim().parse::<f64>()
        {
            set = Some(json!({ "handles": ids, "ltscale": f }));
        }
    });
    prop_row(ui, "Lineweight", |ui| {
        ui.menu_button(lw.clone(), |ui| {
            for n in ["ByLayer", "ByBlock", "Default"] {
                if ui.button(n).clicked() {
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
        ui.menu_button(tr.clone(), |ui| {
            for v in ["ByLayer", "ByBlock"] {
                if ui.button(v).clicked() {
                    set = Some(json!({ "handles": ids, "transparency": v }));
                    ui.close();
                }
            }
            ui.separator();
            // A fixed percentage, committed when the drag or edit ends.
            let tid = egui::Id::new(("prop_tr", &ids));
            let cur = tr.parse::<u8>().ok();
            let mut pct = ui.data_mut(|d| d.get_temp::<u8>(tid)).or(cur).unwrap_or(0);
            ui.horizontal(|ui| {
                ui.label("Percent");
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
        let num = |ui: &mut egui::Ui, label: &str, key: &str, v: f64, set: &mut Option<Value>, ids: &Vec<String>| {
            prop_row(ui, label, |ui| {
                if let Some(s) = edit_field(ui, ui.id().with((key, ids)), format!("{v:.4}"))
                    && let Ok(f) = s.trim().parse::<f64>()
                {
                    *set = Some(json!({ "handles": ids, key: f }));
                }
            });
        };
        match &e.kind {
            EntityKind::Line(l) => {
                prop_row(ui, "Start X", |ui| {
                    if let Some(s) = edit_field(ui, ui.id().with(("sx", &ids)), format!("{:.4}", l.a.x))
                        && let Ok(f) = s.trim().parse::<f64>()
                    {
                        set = Some(json!({ "handles": ids, "start": [f, l.a.y] }));
                    }
                });
                prop_row(ui, "Start Y", |ui| {
                    if let Some(s) = edit_field(ui, ui.id().with(("sy", &ids)), format!("{:.4}", l.a.y))
                        && let Ok(f) = s.trim().parse::<f64>()
                    {
                        set = Some(json!({ "handles": ids, "start": [l.a.x, f] }));
                    }
                });
                prop_row(ui, "End X", |ui| {
                    if let Some(s) = edit_field(ui, ui.id().with(("ex", &ids)), format!("{:.4}", l.b.x))
                        && let Ok(f) = s.trim().parse::<f64>()
                    {
                        set = Some(json!({ "handles": ids, "end": [f, l.b.y] }));
                    }
                });
                prop_row(ui, "End Y", |ui| {
                    if let Some(s) = edit_field(ui, ui.id().with(("ey", &ids)), format!("{:.4}", l.b.y))
                        && let Ok(f) = s.trim().parse::<f64>()
                    {
                        set = Some(json!({ "handles": ids, "end": [l.b.x, f] }));
                    }
                });
                let dlt = l.b.xy() - l.a.xy();
                prop_row(ui, "Delta X", |ui| read_only(ui, &format!("{:.4}", dlt.x)));
                prop_row(ui, "Delta Y", |ui| read_only(ui, &format!("{:.4}", dlt.y)));
                // Length and angle keep the start point and move the end point.
                num(ui, "Length", "length", dlt.len(), &mut set, &ids);
                num(ui, "Angle", "angle", dlt.angle().to_degrees(), &mut set, &ids);
            }
            EntityKind::Circle(c) => {
                num(ui, "Center X", "cx", c.center.x, &mut set, &ids);
                num(ui, "Center Y", "cy", c.center.y, &mut set, &ids);
                num(ui, "Radius", "radius", c.radius, &mut set, &ids);
                num(ui, "Diameter", "diameter", c.radius * 2.0, &mut set, &ids);
                num(ui, "Circumference", "circumference", c.radius * std::f64::consts::TAU, &mut set, &ids);
                num(ui, "Area", "area", c.radius * c.radius * std::f64::consts::PI, &mut set, &ids);
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
                num(ui, "Radius", "radius", a.radius, &mut set, &ids);
                num(ui, "Start angle", "startAngle", a.start.to_degrees(), &mut set, &ids);
                num(ui, "End angle", "endAngle", a.end.to_degrees(), &mut set, &ids);
            }
            EntityKind::Text(tx) => {
                prop_row(ui, "Contents", |ui| {
                    if let Some(s) = edit_field(ui, ui.id().with(("txt", &ids)), tx.value.clone()) {
                        set = Some(json!({ "handles": ids, "text": s }));
                    }
                });
                num(ui, "Height", "height", tx.height, &mut set, &ids);
                num(ui, "Rotation", "rotation", tx.rotation.to_degrees(), &mut set, &ids);
                num(ui, "Width factor", "widthFactor", tx.width_factor, &mut set, &ids);
            }
            EntityKind::MText(tx) => {
                prop_row(ui, "Contents", |ui| {
                    if let Some(s) = edit_field(ui, ui.id().with(("mtxt", &ids)), tx.contents.clone()) {
                        set = Some(json!({ "handles": ids, "text": s }));
                    }
                });
                num(ui, "Text height", "height", tx.height, &mut set, &ids);
                num(ui, "Defined width", "width", tx.width, &mut set, &ids);
            }
            EntityKind::LwPolyline(pl) => {
                num(ui, "Global width", "width", pl.const_width, &mut set, &ids);
                let g = cadcraft_geom::Polyline { vertices: pl.vertices.clone(), closed: pl.closed };
                prop_row(ui, "Length", |ui| read_only(ui, &format!("{:.4}", g.len())));
                if pl.closed {
                    prop_row(ui, "Area", |ui| read_only(ui, &format!("{:.4}", g.area().abs())));
                }
                prop_row(ui, "Closed", |ui| {
                    let mut c = pl.closed;
                    let label = if c { "Yes" } else { "No" };
                    if ui.checkbox(&mut c, label).changed() {
                        set = Some(json!({ "handles": ids, "closed": c }));
                    }
                });
            }
            EntityKind::Hatch(hh) => {
                prop_row(ui, "Pattern", |ui| {
                    ui.menu_button(hh.pattern.clone(), |ui| {
                        for pat in cadcraft_doc::library::standard_patterns() {
                            if ui.button(pat.name).clicked() {
                                set = Some(json!({ "handles": ids, "pattern": pat.name }));
                                ui.close();
                            }
                        }
                    });
                });
                num(ui, "Scale", "scale", hh.scale, &mut set, &ids);
                num(ui, "Angle", "angle", hh.angle.to_degrees(), &mut set, &ids);
            }
            EntityKind::Dimension(dm) => {
                prop_row(ui, "Text override", |ui| {
                    if let Some(s) = edit_field(ui, ui.id().with(("dimtxt", &ids)), dm.text.clone()) {
                        set = Some(json!({ "handles": ids, "textOverride": s }));
                    }
                });
                prop_row(ui, "Dim style", |ui| {
                    if let Some(n) = pick_menu(ui, &dm.style, d.dim_styles.iter().map(|st| st.name.clone())) {
                        set = Some(json!({ "handles": ids, "dimStyle": n }));
                    }
                });
            }
            EntityKind::Insert(i) => {
                prop_row(ui, "Name", |ui| read_only(ui, &i.block));
                num(ui, "Rotation", "rotation", i.rotation.to_degrees(), &mut set, &ids);
                num(ui, "Scale", "scale", i.scale.x, &mut set, &ids);
            }
            _ => {}
        }
    }
    if let Some(p) = set {
        let _ = app.run("properties.set", p);
    }
}
