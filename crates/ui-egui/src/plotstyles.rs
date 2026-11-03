//! Plot Style Manager (STYLESMANAGER): the plot style tables the drawing can use, a table's
//! styles in an editable grid, and the current layout's table and "Display plot styles".
//!
//! The dialog reads the drawing and changes it only through STYLESMANAGER and PAGESETUP
//! (`app.run`), so every edit is undoable and scriptable.

use egui::{RichText, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// Open the dialog for a typed or menu-invoked STYLESMANAGER (no parameters).
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if !params.is_null() || id != "stylesmanager" {
        return None;
    }
    app.ui.dialog = Some("stylesmanager".into());
    Some(Ok(Value::Null))
}

fn temp<T: Clone + Send + Sync + 'static>(ctx: &egui::Context, id: &str) -> Option<T> {
    ctx.data_mut(|d| d.get_temp::<T>(egui::Id::unique(id)))
}

fn set_temp<T: Clone + Send + Sync + 'static>(ctx: &egui::Context, id: &str, v: T) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(id), v));
}

/// Lineweights offered for a style, in millimetres.
const LINEWEIGHTS: [f64; 12] = [0.0, 0.09, 0.13, 0.18, 0.25, 0.3, 0.35, 0.5, 0.7, 1.0, 1.4, 2.11];

/// A command to run after the dialog is drawn.
type Action = Option<(&'static str, Value)>;

pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let action = window(app, ctx, open);
    if let Some((cmd, p)) = action {
        // Errors are echoed on the command line by `run`.
        let _ = app.run(cmd, p);
    }
}

/// Plot Style Manager window; returns the command (`stylesmanager` or `pagesetup`) to run after drawing.
fn window(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> Action {
    let t = Tokens::get();
    let title = crate::tl!("Plot Style Manager");
    let Ok(d) = app.session.doc() else {
        egui::Window::new(title).open(open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(crate::tl!("Open a drawing first."));
        });
        return None;
    };
    let names = cadcraft_doc::plot_style_table_names(d);
    let space = app.session.layout_space();
    let layout = space.layout().and_then(|n| d.layout(n).map(|l| (n.to_string(), l.page.plot_style_table.clone(), l.page.show_plot_styles)));
    let mut sel = temp::<String>(ctx, "pstyle_sel")
        .filter(|s| names.iter().any(|n| n.eq_ignore_ascii_case(s)))
        .or_else(|| layout.as_ref().map(|l| l.1.clone()).filter(|n| names.iter().any(|x| x.eq_ignore_ascii_case(n))))
        .or_else(|| names.first().cloned())
        .unwrap_or_default();
    let table = cadcraft_doc::plot_style_table(d, &sel);
    let in_drawing = d.plot_style_tables.iter().any(|x| x.name.eq_ignore_ascii_case(&sel));
    let linetypes: Vec<String> =
        d.linetypes.iter().map(|l| l.name.clone()).filter(|n| !n.eq_ignore_ascii_case("ByLayer") && !n.eq_ignore_ascii_case("ByBlock")).collect();
    let mut new_style = temp::<String>(ctx, "pstyle_new").unwrap_or_default();
    let mut action: Action = None;
    let set = |table: &str, style: Value| Some(("stylesmanager", json!({ "action": "set", "name": table, "style": style })));
    egui::Window::new(title).id(egui::Id::unique("Plot Style Manager")).open(open).default_size(vec2(760.0, 460.0)).resizable(true).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(crate::tl!("Plot style table"));
            craft_ui::ComboBox::from_id_salt("pstyle_table").selected_text(sel.as_str()).width(200.0).show_ui(ui, |ui| {
                for n in &names {
                    if ui.selectable_label(n.eq_ignore_ascii_case(&sel), n).clicked() {
                        sel = n.clone();
                    }
                }
            });
            if ui.button(crate::tl!("Copy")).on_hover_text(crate::tl!("Keep an editable copy of this table in the drawing")).clicked() {
                let stem = sel.rsplit_once('.').map(|(s, _)| s).unwrap_or(&sel);
                let copy = format!("{stem}-copy");
                action = Some(("stylesmanager", json!({ "action": "new", "name": copy, "from": sel })));
            }
            if ui.button(crate::tl!("New CTB")).clicked() {
                action = Some(("stylesmanager", json!({ "action": "new", "name": "new.ctb" })));
            }
            if ui.button(crate::tl!("New STB")).clicked() {
                action = Some(("stylesmanager", json!({ "action": "new", "name": "new.stb" })));
            }
            if ui.add_enabled(in_drawing, egui::Button::new(crate::tl!("Delete"))).clicked() {
                action = Some(("stylesmanager", json!({ "action": "delete", "name": sel })));
            }
        });
        if let Some((name, current, show)) = &layout {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(crate::tf!(
                        "Layout {name}: {table}",
                        name = name,
                        table = if current.is_empty() { crate::i18n::t("None") } else { current.as_str() }
                    ))
                    .color(t.text_dim),
                );
                if ui.add_enabled(!current.eq_ignore_ascii_case(&sel), egui::Button::new(crate::tl!("Use for this layout"))).clicked() {
                    action = Some(("pagesetup", json!({ "layout": name, "plotStyleTable": sel })));
                }
                let mut s = *show;
                if ui.checkbox(&mut s, crate::tl!("Display plot styles")).changed() {
                    action = Some(("pagesetup", json!({ "layout": name, "displayPlotStyles": s })));
                }
            });
        }
        ui.separator();
        let Some(table) = &table else {
            ui.label(crate::tl!("No plot style tables."));
            return;
        };
        let named = !table.is_color_dependent();
        ui.label(RichText::new(&table.description).color(t.text_dim));
        if !in_drawing {
            ui.label(RichText::new(crate::tl!("A built-in table: editing it keeps an edited copy in the drawing.")).color(t.text_faint));
        }
        egui::ScrollArea::vertical().id_salt("pstyle_rows").max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("pstyle_grid").striped(true).num_columns(if named { 7 } else { 6 }).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
                for h in [
                    crate::tl!("Plot style"),
                    crate::tl!("Color"),
                    crate::tl!("Grayscale"),
                    crate::tl!("Screening"),
                    crate::tl!("Linetype"),
                    crate::tl!("Lineweight"),
                ] {
                    ui.label(RichText::new(h).strong());
                }
                if named {
                    ui.label("");
                }
                ui.end_row();
                for st in &table.styles {
                    let style = |k: &str, v: Value| json!({ "name": st.name, k: v });
                    ui.label(&st.name);
                    ui.horizontal(|ui| {
                        let mut object = st.color.is_none();
                        if ui.checkbox(&mut object, crate::tl!("Object")).changed() {
                            action = set(&table.name, style("color", if object { Value::Null } else { json!("#000000") }));
                        }
                        if let Some(c) = st.color {
                            let mut rgb = [c.0, c.1, c.2];
                            if ui.color_edit_button_srgb(&mut rgb).changed() {
                                action = set(&table.name, style("color", json!(cadcraft_color::Rgb(rgb[0], rgb[1], rgb[2]).hex())));
                            }
                        }
                    });
                    let mut gray = st.grayscale;
                    if ui.checkbox(&mut gray, "").changed() {
                        action = set(&table.name, style("grayscale", json!(gray)));
                    }
                    let mut screen = st.screening.min(100);
                    if ui.add(egui::DragValue::new(&mut screen).range(0..=100).suffix(" %")).changed() {
                        action = set(&table.name, style("screening", json!(screen)));
                    }
                    let lt = st.linetype.clone().unwrap_or_else(|| crate::tl!("Use object linetype").to_string());
                    craft_ui::ComboBox::from_id_salt(("pstyle_lt", &st.name)).selected_text(lt).width(140.0).show_ui(ui, |ui| {
                        if ui.selectable_label(st.linetype.is_none(), crate::tl!("Use object linetype")).clicked() {
                            action = set(&table.name, style("linetype", Value::Null));
                        }
                        for n in &linetypes {
                            if ui.selectable_label(st.linetype.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(n)), n).clicked() {
                                action = set(&table.name, style("linetype", json!(n)));
                            }
                        }
                    });
                    let lw = st
                        .lineweight
                        .map(|w| format!("{:.2} mm", f64::from(w) / 100.0))
                        .unwrap_or_else(|| crate::tl!("Use object lineweight").to_string());
                    craft_ui::ComboBox::from_id_salt(("pstyle_lw", &st.name)).selected_text(lw).width(140.0).show_ui(ui, |ui| {
                        if ui.selectable_label(st.lineweight.is_none(), crate::tl!("Use object lineweight")).clicked() {
                            action = set(&table.name, style("lineweight", Value::Null));
                        }
                        for mm in LINEWEIGHTS {
                            let hundredths = (mm * 100.0).round() as u16;
                            if ui.selectable_label(st.lineweight == Some(hundredths), format!("{mm:.2} mm")).clicked() {
                                action = set(&table.name, style("lineweight", json!(mm)));
                            }
                        }
                    });
                    if named {
                        let normal = st.name.eq_ignore_ascii_case(cadcraft_doc::NORMAL_STYLE);
                        if ui.add_enabled(!normal, egui::Button::new(crate::tl!("Remove"))).clicked() {
                            action = Some(("stylesmanager", json!({ "action": "set", "name": table.name, "removeStyle": st.name })));
                        }
                    }
                    ui.end_row();
                }
            });
        });
        if named {
            ui.separator();
            ui.horizontal(|ui| {
                let label = ui.label(crate::tl!("New style"));
                ui.add(egui::TextEdit::singleline(&mut new_style).desired_width(160.0)).labelled_by(label.id);
                if ui.add_enabled(!new_style.trim().is_empty(), egui::Button::new(crate::tl!("Add"))).clicked() {
                    action = set(&table.name, json!({ "name": new_style.trim() }));
                    new_style.clear();
                }
            });
        }
    });
    set_temp(ctx, "pstyle_sel", sel);
    set_temp(ctx, "pstyle_new", new_style);
    action
}
