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

pub fn toolsets(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    // Right-click anywhere in Tool Sets: where to put it (float, pin, ribbon, group, close).
    let place = crate::docking::toolsets_place(app);
    let panel_groups = crate::docking::toolsets_groups(app);
    let mut panel_action: Option<crate::docking::ToolSetsAction> = None;
    egui::Frame::NONE.fill(t.panel).show(ui, |ui| {
        let r = ui.max_rect();
        let p = ui.painter().clone();
        ui.interact(r, ui.id().with("ts_background"), Sense::click())
            .context_menu(|ui| crate::docking::toolsets_menu(ui, place, &panel_groups, &mut panel_action));
        // Tabs.
        let tab_h = 28.0;
        let mut x = r.left();
        for name in ["Drafting", "Modeling"] {
            let w = 92.0;
            let tr = Rect::from_min_size(pos2(x, r.top()), vec2(w, tab_h));
            let active = app.ui.toolset_tab == name;
            let resp = ui.interact(tr, ui.id().with(("ts", name)), Sense::click());
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
        let cresp = ui.interact(cr, ui.id().with("ts_collapse"), Sense::click());
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
        // Right-click a tool: put it on a tool bar or take it off.
        let toolbar_items: Vec<(crate::toolbar::Bar, Vec<String>)> =
            crate::toolbar::Bar::ALL.into_iter().map(|bar| (bar, crate::toolbar::state(&app.ui, bar).items)).collect();
        let mut to_toolbar: Option<(&'static str, crate::toolbar::Bar, bool)> = None;
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
                    let gresp = ui.interact(gear, ui.id().with(("ts_gear", name)), Sense::click());
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
                                        let resp = crate::toolbar::tool_button(ui, *icon, 40.0, cmd, tip);
                                        toolbar_menu(&resp, cmd, &toolbar_items, &mut to_toolbar, (place, &panel_groups, &mut panel_action));
                                        if resp.clicked() {
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
                                        let resp = crate::toolbar::tool_button(ui, *icon, 23.0, cmd, tip);
                                        toolbar_menu(&resp, cmd, &toolbar_items, &mut to_toolbar, (place, &panel_groups, &mut panel_action));
                                        if resp.clicked() {
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
        if let Some((cmd, bar, add)) = to_toolbar {
            let id = if add { "ui.toolbar.add" } else { "ui.toolbar.remove" };
            if let Err(e) = app.run(id, json!({ "command": cmd, "bar": bar.id() })) {
                app.set_status(&e);
            }
        }
        if let Some(c) = clicked {
            app.cmd.buffer.clear();
            app.start(c);
        }
    });
    if let Some(a) = panel_action
        && let Err(e) = crate::docking::apply_toolsets(app, a, ui.ctx())
    {
        app.set_status(&e);
    }
}

/// What the Tool Sets panel menu needs: where Tool Sets is, the groups it can join, and where the
/// choice goes.
type PanelMenu<'a> = (crate::docking::ToolSetsPlace, &'a [(crate::docking::Panel, String)], &'a mut Option<crate::docking::ToolSetsAction>);

/// The right-click menu of a Tool Sets button: a tick per tool bar puts the tool on that bar or
/// takes it off.
fn toolbar_menu(
    resp: &egui::Response,
    cmd: &'static str,
    bars: &[(crate::toolbar::Bar, Vec<String>)],
    to_toolbar: &mut Option<(&'static str, crate::toolbar::Bar, bool)>,
    panel: PanelMenu<'_>,
) {
    resp.context_menu(|ui| {
        let (place, groups, out) = panel;
        crate::docking::toolsets_menu(ui, place, groups, out);
        ui.separator();
        ui.label(egui::RichText::new(crate::tl!("Show on Tool Bar")).weak());
        for (bar, items) in bars {
            let on = items.iter().any(|i| i == cmd);
            let mut checked = on;
            if ui.checkbox(&mut checked, crate::i18n::t(bar.title())).changed() {
                *to_toolbar = Some((cmd, *bar, checked));
                ui.close();
            }
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

pub(crate) fn swatch(p: &egui::Painter, r: Rect, c: Color) {
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
        let resp = ui.interact(r, ui.id().with("ts_expand"), Sense::click());
        icons::paint(ui.painter(), Rect::from_center_size(pos2(r.center().x, r.top() + 14.0), vec2(14.0, 14.0)), Icon::ChevronRight, false);
        icons::describe_control(ui, &resp, "Expand Tool Sets", None, false);
        if resp.on_hover_text(crate::tl!("Tool Sets")).clicked() {
            let _ = app.run("ui.toggle.toolsets", json!({"on": true}));
        }
    });
}

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
        let cresp = ui.interact(cr, ui.id().with("palettes_collapse"), Sense::click());
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

/// A layer for the current-layer drop-downs: name, on, frozen, locked, colour.
pub(crate) type LayerRow = (String, bool, bool, bool, Color);

/// The current-layer drop-down's list: on/off, freeze/thaw and lock/unlock per layer; picking a
/// name makes it current. The choice comes back in `set_current` or `toggle` (name, key, value).
pub(crate) fn layer_list(
    ui: &mut egui::Ui,
    layers: &[LayerRow],
    cur: &str,
    set_current: &mut Option<String>,
    toggle: &mut Option<(String, &'static str, bool)>,
) {
    for (name, on, frozen, locked, color) in layers {
        ui.horizontal(|ui| {
            if icons::button(ui, if *on { Icon::Bulb } else { Icon::BulbOff }, 18.0, "On/Off", false).clicked() {
                *toggle = Some((name.clone(), "on", !*on));
            }
            if icons::button(ui, if *frozen { Icon::Snowflake } else { Icon::Sun }, 18.0, "Freeze/Thaw", false).clicked() {
                *toggle = Some((name.clone(), "frozen", !*frozen));
            }
            if icons::button(ui, if *locked { Icon::Lock } else { Icon::Unlock }, 18.0, "Lock/Unlock", false).clicked() {
                *toggle = Some((name.clone(), "locked", !*locked));
            }
            let (sr, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
            swatch(ui.painter(), sr, *color);
            if ui.selectable_label(name.eq_ignore_ascii_case(cur), name).clicked() {
                *set_current = Some(name.clone());
            }
        });
    }
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
    let layers: Vec<LayerRow> = d.layers.iter().map(|l| (l.name.clone(), l.on, l.frozen, l.locked, l.color)).collect();
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
            scroll_list(ui, "cc_props_layers", |ui| layer_list(ui, &layers, &cur, &mut set_current, &mut toggle));
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

fn prop_row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        let mut job = egui::text::LayoutJob::simple(crate::i18n::t(label).to_string(), crate::theme::body(), t.text_dim, 106.0);
        job.halign = egui::Align::RIGHT;
        let galley = ui.painter().layout_job(job);
        let (lr, _) = ui.allocate_exact_size(vec2(118.0, galley.size().y.max(22.0)), Sense::hover());
        ui.painter().galley(pos2(lr.right() - 6.0, lr.center().y - galley.size().y / 2.0), galley, t.text_dim);
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

/// Text justifications: (code, horizontal, vertical, name).
const TEXT_JUSTIFY: [(&str, cadcraft_doc::HAlign, cadcraft_doc::VAlign, &str); 15] = {
    use cadcraft_doc::{HAlign as H, VAlign as V};
    [
        ("L", H::Left, V::Baseline, "Left"),
        ("C", H::Center, V::Baseline, "Center"),
        ("R", H::Right, V::Baseline, "Right"),
        ("A", H::Aligned, V::Baseline, "Aligned"),
        ("M", H::Middle, V::Baseline, "Middle"),
        ("F", H::Fit, V::Baseline, "Fit"),
        ("TL", H::Left, V::Top, "Top left"),
        ("TC", H::Center, V::Top, "Top center"),
        ("TR", H::Right, V::Top, "Top right"),
        ("ML", H::Left, V::Middle, "Middle left"),
        ("MC", H::Center, V::Middle, "Middle center"),
        ("MR", H::Right, V::Middle, "Middle right"),
        ("BL", H::Left, V::Bottom, "Bottom left"),
        ("BC", H::Center, V::Bottom, "Bottom center"),
        ("BR", H::Right, V::Bottom, "Bottom right"),
    ]
};

/// Multiline text attachments 1..=9: (code, name).
const MTEXT_JUSTIFY: [(&str, &str); 9] = [
    ("TL", "Top left"),
    ("TC", "Top center"),
    ("TR", "Top right"),
    ("ML", "Middle left"),
    ("MC", "Middle center"),
    ("MR", "Middle right"),
    ("BL", "Bottom left"),
    ("BC", "Bottom center"),
    ("BR", "Bottom right"),
];

/// A Yes / No property row setting `key`.
fn yes_no(ui: &mut egui::Ui, label: &str, key: &str, on: bool, ids: &[String], set: &mut Option<Value>) {
    prop_row(ui, label, |ui| {
        let cur = if on { crate::tl!("Yes") } else { crate::tl!("No") };
        if let Some(n) = pick_menu(ui, cur, [crate::tl!("Yes").to_string(), crate::tl!("No").to_string()]) {
            *set = Some(json!({ "handles": ids, key: n == crate::tl!("Yes") }));
        }
    });
}

/// Most height a drop-down list of the palettes takes before it scrolls.
const LIST_H: f32 = 360.0;

/// A list in a drop-down: it scrolls, with a scroll bar always shown, once it is taller than
/// [`LIST_H`].
pub(crate) fn scroll_list<R>(ui: &mut egui::Ui, salt: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    egui::ScrollArea::vertical().id_salt(salt).max_height(LIST_H).auto_shrink([true, true]).show(ui, add).inner
}

/// A menu button whose list scrolls when it is long (layers, linetypes, styles...).
pub(crate) fn menu_list<R>(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    ui.menu_button(text, |ui| scroll_list(ui, "cc_menu_list", add))
}

/// A drop-down of `items` showing `current`; returns the item clicked.
fn pick_menu(ui: &mut egui::Ui, current: &str, items: impl IntoIterator<Item = String>) -> Option<String> {
    // Names are the drawing's own; only the ByLayer/ByBlock keywords are translated.
    let shown = |s: &str| if ["ByLayer", "ByBlock"].contains(&s) { crate::i18n::t(s).to_string() } else { s.to_string() };
    let mut out = None;
    menu_list(ui, shown(current), |ui| {
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
    let id = egui::Id::new(("dynamic-block-custom", st.uid, st.revision, h));
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
    ui.interact(my, ui.id().with("prop_my"), Sense::hover()).on_hover_text(crate::tl!("My properties: not available yet"));
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
                scroll_list(ui, "cc_props_types", |ui| {
                    for (ty, hs) in &types {
                        if ui.button(format!("{} ({})", crate::i18n::t(ty), hs.len())).clicked() {
                            run = Some(("select", json!({ "handles": hs })));
                        }
                    }
                });
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
            menu_list(ui, format!("■ {}", crate::i18n::t(&Color::from_aci(h.i64("CECOLOR", 256) as i16).name())), |ui| {
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
            if let Some(f) = number(edit_field(ui, ui.id().with("cur_celtscale"), format!("{:.4}", h.f64("CELTSCALE", 1.0))), &mut bad) {
                act("setvar", json!({ "name": "CELTSCALE", "value": f }));
            }
        });
        prop_row(ui, "Lineweight", |ui| {
            menu_list(ui, crate::i18n::t(&cadcraft_doc::Lineweight::from_dxf(h.i64("CELWEIGHT", -1) as i16).name()), |ui| {
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
            if let Some(f) = number(edit_field(ui, ui.id().with("cur_thickness"), len(h.f64("THICKNESS", 0.0))), &mut bad) {
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
            if let Some(f) = number(edit_field(ui, ui.id().with("cur_textsize"), len(h.f64("TEXTSIZE", 0.2))), &mut bad) {
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
    let mut dim_override: Option<Value> = None;
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
            egui::Id::new(("properties_color", &ids)),
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
        menu_list(ui, if ["ByLayer", "ByBlock", "*VARIES*"].contains(&linetype.as_str()) { crate::i18n::t(&linetype) } else { &linetype }, |ui| {
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
        if let Some(f) = number(edit_field(ui, ui.id().with(("lts", &ids)), lts.clone()), &mut bad) {
            set = Some(json!({ "handles": ids, "ltscale": f }));
        }
    });
    prop_row(ui, "Lineweight", |ui| {
        menu_list(ui, crate::i18n::t(&lw), |ui| {
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
        menu_list(ui, crate::i18n::t(&tr), |ui| {
            for v in ["ByLayer", "ByBlock"] {
                if ui.button(crate::i18n::t(v)).clicked() {
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
    // Text style: for text, multiline text and multileaders with text, one or many at once.
    let style_of = |e: &cadcraft_doc::Entity| -> Option<String> {
        match &e.kind {
            EntityKind::Text(t) => Some(t.style.clone()),
            EntityKind::MText(t) => Some(t.style.clone()),
            EntityKind::AttDef(a) => Some(a.text.style.clone()),
            EntityKind::MLeader(m) => m.text.as_ref().map(|t| t.style.clone()),
            _ => None,
        }
    };
    let text_styles: Option<Vec<String>> = sel.iter().map(|h| d.entity(*h).and_then(|e| style_of(e))).collect();
    // One multileader shows it in its own Text group, below the leader properties.
    let one_mleader = sel.len() == 1 && matches!(e.kind, EntityKind::MLeader(_));
    if let Some(styles) = text_styles.filter(|v| !v.is_empty() && !one_mleader) {
        let cur = if styles.iter().all(|s| s.eq_ignore_ascii_case(&styles[0])) { styles[0].clone() } else { "*VARIES*".to_string() };
        group(ui, "Text");
        prop_row(ui, "Style", |ui| {
            let names = d.text_styles.iter().map(|s| s.name.clone());
            if let Some(n) = pick_menu(ui, if cur == "*VARIES*" { crate::i18n::t(&cur) } else { &cur }, names) {
                set = Some(json!({ "handles": ids, "textStyle": n }));
            }
        });
    }
    if sel.len() == 1 {
        group(ui, "Geometry");
        let real = |v: f64| format!("{v:.4}");
        // Rotations show as the drawing measures directions (ANGBASE, ANGDIR).
        let angles = app.session.angle_settings();
        let num = |ui: &mut egui::Ui, label: &str, key: &str, shown: String, set: &mut Option<Value>, bad: &mut Option<String>, ids: &Vec<String>| {
            prop_row(ui, label, |ui| {
                if let Some(f) = number(edit_field(ui, ui.id().with((key, ids)), shown), bad) {
                    *set = Some(json!({ "handles": ids, key: f }));
                }
            });
        };
        match &e.kind {
            EntityKind::Line(l) => {
                prop_row(ui, "Start X", |ui| {
                    if let Some(f) = number(edit_field(ui, ui.id().with(("sx", &ids)), len(l.a.x)), &mut bad) {
                        set = Some(json!({ "handles": ids, "start": [f, l.a.y] }));
                    }
                });
                prop_row(ui, "Start Y", |ui| {
                    if let Some(f) = number(edit_field(ui, ui.id().with(("sy", &ids)), len(l.a.y)), &mut bad) {
                        set = Some(json!({ "handles": ids, "start": [l.a.x, f] }));
                    }
                });
                prop_row(ui, "End X", |ui| {
                    if let Some(f) = number(edit_field(ui, ui.id().with(("ex", &ids)), len(l.b.x)), &mut bad) {
                        set = Some(json!({ "handles": ids, "end": [f, l.b.y] }));
                    }
                });
                prop_row(ui, "End Y", |ui| {
                    if let Some(f) = number(edit_field(ui, ui.id().with(("ey", &ids)), len(l.b.y)), &mut bad) {
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
                    if let Some(s) = edit_field(ui, ui.id().with(("txt", &ids)), tx.value.clone()) {
                        set = Some(json!({ "handles": ids, "text": s }));
                    }
                });
                num(ui, "Height", "height", len(tx.height), &mut set, &mut bad, &ids);
                num(ui, "Rotation", "rotation", real(angles.shown(tx.rotation).to_degrees()), &mut set, &mut bad, &ids);
                num(ui, "Width factor", "widthFactor", real(tx.width_factor), &mut set, &mut bad, &ids);
                prop_row(ui, "Justify", |ui| {
                    let cur = TEXT_JUSTIFY.iter().find(|(_, h, v, _)| *h == tx.halign && *v == tx.valign).map_or("Left", |j| j.3);
                    if let Some(n) = pick_menu(ui, crate::i18n::t(cur), TEXT_JUSTIFY.iter().map(|j| crate::i18n::t(j.3).to_string()))
                        && let Some(j) = TEXT_JUSTIFY.iter().find(|j| crate::i18n::t(j.3) == n)
                    {
                        set = Some(json!({ "handles": ids, "justify": j.0 }));
                    }
                });
                yes_no(ui, "Backwards", "backwards", tx.backwards, &ids, &mut set);
                yes_no(ui, "Upside down", "upsideDown", tx.upside_down, &ids, &mut set);
            }
            EntityKind::MText(tx) => {
                prop_row(ui, "Contents", |ui| {
                    if let Some(s) = edit_field(ui, ui.id().with(("mtxt", &ids)), tx.contents.clone()) {
                        set = Some(json!({ "handles": ids, "text": s }));
                    }
                });
                num(ui, "Text height", "height", len(tx.height), &mut set, &mut bad, &ids);
                num(ui, "Defined width", "width", len(tx.width), &mut set, &mut bad, &ids);
                num(ui, "Rotation", "rotation", real(tx.rotation.to_degrees()), &mut set, &mut bad, &ids);
                prop_row(ui, "Justify", |ui| {
                    let cur = MTEXT_JUSTIFY.get(usize::from(tx.attach.clamp(1, 9) - 1)).map_or("Top left", |j| j.1);
                    if let Some(n) = pick_menu(ui, crate::i18n::t(cur), MTEXT_JUSTIFY.iter().map(|j| crate::i18n::t(j.1).to_string()))
                        && let Some(j) = MTEXT_JUSTIFY.iter().find(|j| crate::i18n::t(j.1) == n)
                    {
                        set = Some(json!({ "handles": ids, "justify": j.0 }));
                    }
                });
                yes_no(ui, "Text frame", "frame", tx.frame, &ids, &mut set);
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
                    menu_list(ui, hh.pattern.clone(), |ui| {
                        if let Some(name) = patterns::menu(ui, &hh.pattern) {
                            set = Some(json!({ "handles": ids, "pattern": name }));
                            ui.close();
                        }
                    });
                });
                num(ui, "Scale", "scale", real(hh.scale), &mut set, &mut bad, &ids);
                num(ui, "Angle", "angle", real(hh.angle.to_degrees()), &mut set, &mut bad, &ids);
                if !hh.solid {
                    prop_row(ui, "Background color", |ui| {
                        if let Some(v) = crate::hatchdlg::background_menu(ui, hh.background) {
                            set = Some(json!({ "handles": ids, "background": v }));
                        }
                    });
                }
            }
            EntityKind::Dimension(dm) => {
                prop_row(ui, "Dim style", |ui| {
                    if let Some(n) = pick_menu(ui, &dm.style, d.dim_styles.iter().map(|st| st.name.clone())) {
                        set = Some(json!({ "handles": ids, "dimStyle": n }));
                    }
                });
                prop_row(ui, "Measurement", |ui| read_only(ui, &cadcraft_render::dimension_in(d, dm).value));
                prop_row(ui, "Text override", |ui| {
                    if let Some(s) = edit_field(ui, ui.id().with(("dimtxt", &ids)), dm.text.clone()) {
                        set = Some(json!({ "handles": ids, "textOverride": s }));
                    }
                });
                // The dimension's style settings with its own overrides; a change overrides that
                // setting for the selected dimensions (DIMOVERRIDE).
                let style = d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides);
                let before = crate::dimstyles::style_map(&style);
                let mut after = before.clone();
                let names = crate::dimstyles::Names::of(d);
                for (title, rows) in crate::dimstyles::PROPERTIES {
                    group(ui, title);
                    for (label, kind) in rows.iter() {
                        prop_row(ui, label, |ui| {
                            crate::dimstyles::field_row(ui, egui::Id::new(("cc_dimprop", *title, *label)), &names, &mut after, *kind);
                        });
                    }
                }
                let changed = crate::dimstyles::changed_fields(&before, &after);
                if !changed.is_empty() {
                    let mut p = changed;
                    p.insert("handles".into(), json!(ids));
                    dim_override = Some(Value::Object(p));
                }
            }
            EntityKind::MLeader(ml) => {
                let styles: Vec<String> = if d.mleader_styles.is_empty() {
                    vec!["Standard".to_string()]
                } else {
                    d.mleader_styles.iter().map(|s| s.name.clone()).collect()
                };
                prop_row(ui, "Multileader style", |ui| {
                    if let Some(n) = pick_menu(ui, &ml.style, styles) {
                        set = Some(json!({ "handles": ids, "mleaderStyle": n }));
                    }
                });
                group(ui, "Leaders");
                let (straight, spline, none) = (crate::tl!("Straight").to_string(), crate::tl!("Spline").to_string(), crate::tl!("None").to_string());
                prop_row(ui, "Leader type", |ui| {
                    let cur = if ml.leaderless() {
                        &none
                    } else if ml.spline {
                        &spline
                    } else {
                        &straight
                    };
                    // None only hides the leader lines, so a multileader without any can't get them back.
                    let choices = if ml.leaders.is_empty() { vec![none.clone()] } else { vec![straight.clone(), spline.clone(), none.clone()] };
                    if let Some(n) = pick_menu(ui, cur, choices) {
                        let v = if n == spline {
                            "spline"
                        } else if n == none {
                            "none"
                        } else {
                            "straight"
                        };
                        set = Some(json!({ "handles": ids, "leaderType": v }));
                    }
                });
                // The arrowhead: built-in ones and the drawing's arrowhead blocks, as for dimensions.
                let names = crate::dimstyles::Names::of(d);
                prop_row(ui, "Arrowhead", |ui| {
                    if let Some(v) = crate::dimstyles::arrow_combo(ui, egui::Id::new(("cc_mlarrow", &ids)), &names, ml.arrow_in(d)) {
                        set = Some(json!({ "handles": ids, "arrow": v }));
                    }
                });
                num(ui, "Arrowhead size", "arrowSize", real(ml.arrow_size), &mut set, &mut bad, &ids);
                num(ui, "Landing distance", "dogleg", real(ml.dogleg), &mut set, &mut bad, &ids);
                if let Some(t) = &ml.text {
                    group(ui, "Text");
                    prop_row(ui, "Style", |ui| {
                        if let Some(n) = pick_menu(ui, &t.style, d.text_styles.iter().map(|s| s.name.clone())) {
                            set = Some(json!({ "handles": ids, "textStyle": n }));
                        }
                    });
                    num(ui, "Text height", "textHeight", real(t.height), &mut set, &mut bad, &ids);
                }
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
                                (true, true) => edit_field(ui, ui.id().with(("dynprop", e.handle, &p.name)), p.value.clone()),
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
    if let Some(p) = dim_override {
        let _ = app.run("dimstyle.override", p);
    }
}

/// The Tool Sets ribbon: tabs by task, each a row of the Tool Sets groups (by name).
pub const RIBBON: &[(&str, &[&str])] = &[
    ("Draw", &["Draw", "Hatch"]),
    ("Modify", &["Modify"]),
    ("Annotate", &["Text", "Dimension", "Leader"]),
    ("Block", &["Block"]),
    ("Parametric", &["Parametric"]),
    ("3D Modeling", &["Primitives", "Solid", "Boolean", "View"]),
];

/// A Tool Sets group by name (Drafting, then Modeling).
fn tool_group(name: &str) -> Option<&'static (&'static str, &'static [Tool], &'static [Tool])> {
    DRAFTING.iter().chain(MODELING.iter()).find(|(n, _, _)| *n == name)
}

/// Ribbon heights: the tab strip, the tools, the group names under them.
const RIBBON_TABS: f32 = 26.0;
const RIBBON_TOOLS: f32 = 80.0;
const RIBBON_NAMES: f32 = 18.0;
/// Small ribbon buttons are stacked three to a column.
const RIBBON_ROWS: usize = 3;

/// Tool Sets as a ribbon, right under the menu bar: a tab per task, its tool groups side by side
/// (large tools with their names, then small ones in columns of three), the group's name under it.
pub fn ribbon(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let place = crate::docking::toolsets_place(app);
    let groups = crate::docking::toolsets_groups(app);
    let mut panel_action: Option<crate::docking::ToolSetsAction> = None;
    let toolbar_items: Vec<(crate::toolbar::Bar, Vec<String>)> =
        crate::toolbar::Bar::ALL.into_iter().map(|bar| (bar, crate::toolbar::state(&app.ui, bar).items)).collect();
    let mut to_toolbar: Option<(&'static str, crate::toolbar::Bar, bool)> = None;
    let mut clicked: Option<&'static str> = None;
    let mut tab_clicked: Option<&'static str> = None;
    if !RIBBON.iter().any(|(n, _)| *n == app.ui.ribbon_tab) {
        app.ui.ribbon_tab = "Draw".into();
    }
    let current = app.ui.ribbon_tab.clone();
    let hidden: Vec<String> = app.ui.hidden_tools.clone();
    let height = RIBBON_TABS + RIBBON_TOOLS + RIBBON_NAMES + 4.0;
    egui::Panel::top("cc_ribbon").exact_size(height).frame(egui::Frame::NONE.fill(t.panel)).show(ui, |ui| {
        let r = ui.max_rect();
        let p = ui.painter().clone();
        ui.interact(r, ui.id().with("ribbon_background"), Sense::click())
            .context_menu(|ui| crate::docking::toolsets_menu(ui, place, &groups, &mut panel_action));
        // Tabs.
        let strip = Rect::from_min_size(r.min, vec2(r.width(), RIBBON_TABS));
        p.rect_filled(strip, 0.0, t.chrome);
        let mut x = r.left() + 6.0;
        for (name, _) in RIBBON {
            let label = crate::i18n::t(name);
            let w = p.layout_no_wrap(label.to_string(), egui::FontId::proportional(13.0), t.text).size().x + 24.0;
            let tr = Rect::from_min_size(pos2(x, r.top() + 2.0), vec2(w, RIBBON_TABS - 2.0));
            let active = current == *name;
            let resp = ui.interact(tr, ui.id().with(("ribbon_tab", name)), Sense::click());
            if active {
                p.rect_filled(tr, egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, t.panel);
            } else if resp.hovered() {
                p.rect_filled(tr, 3.0, t.control_hover);
            }
            p.text(tr.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(13.0), if active { t.text } else { t.text_dim });
            icons::describe_control(ui, &resp, label, Some(active), true);
            resp.context_menu(|ui| crate::docking::toolsets_menu(ui, place, &groups, &mut panel_action));
            if resp.clicked() {
                tab_clicked = Some(name);
            }
            x += w + 2.0;
        }
        // The open tab's groups.
        let body = Rect::from_min_max(pos2(r.left(), strip.bottom() + 2.0), r.max);
        let groups_here = RIBBON.iter().find(|(n, _)| *n == current).map_or(&[][..], |(_, g)| *g);
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            egui::ScrollArea::horizontal().id_salt("ribbon_scroll").auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                    ui.add_space(6.0);
                    for gname in groups_here {
                        let Some((name, large, small)) = tool_group(gname) else { continue };
                        let shown =
                            |tools: &[Tool]| -> Vec<Tool> { tools.iter().copied().filter(|(_, c, _)| !hidden.iter().any(|h| h == c)).collect() };
                        let (large, small) = (shown(large), shown(small));
                        let group = ui.vertical(|ui| {
                            ui.horizontal_top(|ui| {
                                ui.set_height(RIBBON_TOOLS);
                                ui.spacing_mut().item_spacing = vec2(2.0, 1.0);
                                for (icon, cmd, tip) in &large {
                                    ui.allocate_ui_with_layout(vec2(66.0, RIBBON_TOOLS), egui::Layout::top_down(egui::Align::Center), |ui| {
                                        ui.add_enabled_ui(command_known(cmd), |ui| {
                                            let resp = crate::toolbar::tool_button(ui, *icon, 36.0, cmd, tip);
                                            toolbar_menu(&resp, cmd, &toolbar_items, &mut to_toolbar, (place, &groups, &mut panel_action));
                                            if resp.clicked() {
                                                clicked = Some(cmd);
                                            }
                                        });
                                        ui.add(
                                            egui::Label::new(egui::RichText::new(crate::i18n::t(tip)).size(11.0).color(t.text_dim))
                                                .wrap_mode(egui::TextWrapMode::Wrap)
                                                .halign(egui::Align::Center)
                                                .selectable(false),
                                        );
                                    });
                                }
                                for column in small.chunks(RIBBON_ROWS) {
                                    ui.vertical(|ui| {
                                        ui.add_space(2.0);
                                        for (icon, cmd, tip) in column {
                                            ui.add_enabled_ui(command_known(cmd), |ui| {
                                                let resp = crate::toolbar::tool_button(ui, *icon, 24.0, cmd, tip);
                                                toolbar_menu(&resp, cmd, &toolbar_items, &mut to_toolbar, (place, &groups, &mut panel_action));
                                                if resp.clicked() {
                                                    clicked = Some(cmd);
                                                }
                                            });
                                        }
                                    });
                                }
                            });
                        });
                        // The group's name under its tools, and a line between groups.
                        let gr = group.response.rect;
                        let name_rect =
                            Rect::from_min_size(pos2(gr.left(), body.top() + RIBBON_TOOLS + 1.0), vec2(gr.width().max(40.0), RIBBON_NAMES));
                        ui.painter().text(
                            name_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            crate::i18n::t(name),
                            egui::FontId::proportional(11.5),
                            t.text_dim,
                        );
                        ui.add_space(6.0);
                        let (sep, _) = ui.allocate_exact_size(vec2(1.0, RIBBON_TOOLS + RIBBON_NAMES - 6.0), Sense::hover());
                        ui.painter().vline(sep.center().x, sep.y_range(), Stroke::new(1.0, t.border));
                        ui.add_space(6.0);
                    }
                });
            });
        });
        p.hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, t.border));
    });
    if let Some(name) = tab_clicked {
        app.ui.ribbon_tab = name.into();
    }
    if let Some((cmd, bar, add)) = to_toolbar {
        let id = if add { "ui.toolbar.add" } else { "ui.toolbar.remove" };
        if let Err(e) = app.run(id, json!({ "command": cmd, "bar": bar.id() })) {
            app.set_status(&e);
        }
    }
    if let Some(c) = clicked {
        app.cmd.buffer.clear();
        app.start(c);
    }
    if let Some(a) = panel_action
        && let Err(e) = crate::docking::apply_toolsets(app, a, ui.ctx())
    {
        app.set_status(&e);
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
