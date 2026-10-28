//! The QuickCalc palette: a thin face on the engine's CAL command. Expressions, conversions and
//! variables are evaluated by `cal`; values are read from the drawing by `quickcalc.pick`, whose
//! point prompts run on the command line like any other command.

use egui::{RichText, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// What `quickcalc.pick` writes to the command history before the value it read.
const PICKED: &str = "QuickCalc: ";
/// History entries kept.
const MAX_HISTORY: usize = 100;
/// Variables kept.
const MAX_VARS: usize = 50;
const KINDS: [(&str, &str); 4] = [("length", "Length"), ("area", "Area"), ("volume", "Volume"), ("angle", "Angle")];

#[derive(Clone, Default)]
struct State {
    input: String,
    /// (expression, result) newest last.
    history: Vec<(String, String)>,
    /// The last result: a number or `[x, y]`, the `ans` variable.
    ans: Option<Value>,
    error: Option<String>,
    /// Variables: name and value (a number or `[x, y]`).
    vars: Vec<(String, Value)>,
    var_name: String,
    var_expr: String,
    /// Unit tables from `cal {units: true}`: kind → [(id, symbol)].
    units: Option<Vec<(String, Vec<(String, String)>)>>,
    kind: usize,
    from: usize,
    to: usize,
    conv_value: String,
    conv_result: String,
    /// The command history length when a pick started; the pick's value follows it.
    pending: Option<usize>,
}

fn id() -> egui::Id {
    egui::Id::unique("cadcraft-quickcalc")
}

/// A variable's value as typed into an expression: a number, or a point as `[x,y]`.
fn var_text(v: &Value) -> String {
    match v.as_array() {
        Some(a) => format!("[{}]", a.iter().map(|c| c.as_f64().map(|c| c.to_string()).unwrap_or_default()).collect::<Vec<_>>().join(",")),
        None => v.as_f64().map(|n| n.to_string()).unwrap_or_default(),
    }
}

/// `cal`'s value or point, as a variable value.
fn result_value(r: &Value) -> Option<Value> {
    r.get("point").cloned().or_else(|| r.get("value").cloned()).filter(|v| !v.is_null())
}

impl State {
    fn vars_json(&self) -> Value {
        let mut m = serde_json::Map::new();
        for (k, v) in &self.vars {
            m.insert(k.clone(), v.clone());
        }
        if let Some(a) = &self.ans {
            m.entry("ans").or_insert(a.clone());
        }
        Value::Object(m)
    }
    fn evaluate(&mut self, app: &mut CadApp) {
        let expr = self.input.trim().to_string();
        if expr.is_empty() {
            return;
        }
        match app.run("cal", json!({ "expr": expr, "vars": self.vars_json() })) {
            Ok(r) => {
                let text = r.get("message").and_then(Value::as_str).unwrap_or_default().to_string();
                self.ans = result_value(&r);
                self.history.push((expr, text.clone()));
                if self.history.len() > MAX_HISTORY {
                    self.history.remove(0);
                }
                self.input = text;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }
    fn add_var(&mut self, app: &mut CadApp) {
        let name = self.var_name.trim().to_ascii_lowercase();
        let valid = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !["pi", "e", "ans"].contains(&name.as_str());
        if !valid {
            self.error = Some(crate::tl!("A variable name starts with a letter and has only letters, digits and _").into());
            return;
        }
        match app.run("cal", json!({ "expr": self.var_expr.trim(), "vars": self.vars_json() })) {
            Ok(r) => {
                if let Some(v) = result_value(&r) {
                    self.vars.retain(|(k, _)| *k != name);
                    if self.vars.len() < MAX_VARS {
                        self.vars.push((name, v));
                    }
                    self.var_name.clear();
                    self.var_expr.clear();
                    self.error = None;
                }
            }
            Err(e) => self.error = Some(e),
        }
    }
    fn units(&mut self, app: &mut CadApp) -> Vec<(String, Vec<(String, String)>)> {
        if self.units.is_none() {
            let r = app.run("cal", json!({ "units": true })).unwrap_or_default();
            let tables = KINDS
                .iter()
                .map(|(kind, _)| {
                    let list = r["units"][*kind]
                        .as_array()
                        .map(|a| {
                            a.iter().filter_map(|u| Some((u.get("id")?.as_str()?.to_string(), u.get("symbol")?.as_str()?.to_string()))).collect()
                        })
                        .unwrap_or_default();
                    (kind.to_string(), list)
                })
                .collect();
            self.units = Some(tables);
            self.default_pair();
        }
        self.units.clone().unwrap_or_default()
    }
    /// A common pair of units for the chosen kind (millimetres to inches, …).
    fn default_pair(&mut self) {
        let (from, to) = match KINDS.get(self.kind).map(|k| k.0) {
            Some("length") => ("mm", "in"),
            Some("area") => ("m2", "ft2"),
            Some("volume") => ("l", "galus"),
            _ => ("deg", "rad"),
        };
        let list = self.units.as_ref().and_then(|t| t.get(self.kind)).map(|(_, l)| l.as_slice()).unwrap_or_default();
        let at = |id: &str| list.iter().position(|(u, _)| u == id).unwrap_or(0);
        (self.from, self.to) = (at(from), at(to));
    }
    fn convert(&mut self, app: &mut CadApp) {
        let tables = self.units(app);
        let Some((kind, list)) = tables.get(self.kind) else { return };
        let (Some(from), Some(to)) = (list.get(self.from), list.get(self.to)) else { return };
        let value = if self.conv_value.trim().is_empty() { "1".to_string() } else { self.conv_value.trim().to_string() };
        let p = json!({ "convert": { "type": kind, "from": from.0, "to": to.0, "value": value }, "vars": self.vars_json() });
        match app.run("cal", p) {
            Ok(r) => {
                self.conv_result = r.get("value").and_then(Value::as_f64).map(|v| v.to_string()).unwrap_or_default();
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }
    /// Start reading a value from the drawing; the points are asked for on the command line.
    fn pick(&mut self, app: &mut CadApp, mode: &str) {
        self.pending = Some(app.session.log.len());
        app.start("quickcalc.pick");
        app.cmdline(mode);
    }
    /// When the pick has ended, put the value it read into the expression.
    fn collect(&mut self, app: &CadApp) {
        let Some(from) = self.pending else { return };
        if app.session.running.is_some() {
            return;
        }
        self.pending = None;
        let log = &app.session.log;
        // The history is trimmed from the front when it grows long: then look at its end only.
        let from = if from <= log.len() { from } else { log.len().saturating_sub(20) };
        let Some(v) = log.get(from..).unwrap_or_default().iter().rev().find_map(|l| l.strip_prefix(PICKED)) else { return };
        let v = v.trim();
        let text = if v.contains(',') { format!("[{v}]") } else { v.to_string() };
        if !self.input.is_empty() && !self.input.ends_with([' ', '(', ',', '+', '-', '*', '/', '^', '[']) {
            self.input.clear();
        }
        self.input.push_str(&text);
    }
}

/// Close only QuickCalc when present; otherwise restore it through the normal open path.
/// Hiding the panel leaves its expression, history and variables intact.
pub(crate) fn toggle(app: &mut CadApp) -> Result<Value, String> {
    let panel = crate::docking::Panel::QuickCalc;
    if app.ui.docking.layout.contains(&panel) {
        crate::docking::command(app, false, &json!({ "operation": "close", "panel": panel.id() }))
    } else {
        open(app)
    }
}

/// Open the palette. The first time it floats by the right edge of the drawing; afterwards it
/// returns where it was closed.
pub fn open(app: &mut CadApp) -> Result<Value, String> {
    let panel = crate::docking::Panel::QuickCalc;
    let first = !app.ui.docking.layout.contains(&panel) && !app.ui.docking.hidden.iter().any(|(p, _)| *p == panel);
    let r = crate::docking::command(app, false, &json!({ "operation": "open", "panel": panel.id() }))?;
    if !first {
        return Ok(r);
    }
    let area = app.canvas.rect.unwrap_or(egui::Rect::from_min_size(egui::pos2(220.0, 80.0), vec2(1000.0, 800.0)));
    let (w, h) = (300.0, (area.height() - 40.0).clamp(200.0, 560.0));
    let rect = [area.right() - w - 16.0, area.top() + 24.0, w, h];
    crate::docking::command(app, false, &json!({ "operation": "float", "panel": panel.id(), "rect": rect }))
}

/// The unit-conversion value row. The visible "Value" label names the numeric field (its
/// AccessKit label becomes "Value"), and the placeholder repeats the meaning instead of the
/// unexplained "1" example.
fn value_row(ui: &mut egui::Ui, value: &mut String) {
    let label = ui.label(crate::tl!("Value"));
    ui.add(egui::TextEdit::singleline(value).hint_text(crate::tl!("Value")).desired_width(120.0)).labelled_by(label.id);
}

/// Quick Calculator dock body: expression entry with evaluation, variables and unit conversion.
pub fn show(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let mut st: State = ui.ctx().data_mut(|d| d.get_temp(id())).unwrap_or_default();
    st.collect(app);
    egui::ScrollArea::vertical().id_salt("dock-quickcalc").auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(6.0);
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        // The expression and its buttons.
        let w = ui.available_width() - 12.0;
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let field = egui::TextEdit::singleline(&mut st.input).hint_text(crate::tl!("Expression")).font(crate::theme::mono()).desired_width(w);
            let r = ui.add(field);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                st.evaluate(app);
                r.request_focus();
            }
        });
        if let Some(e) = &st.error {
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(e).color(t.text_dim).font(crate::theme::small()));
            });
        }
        ui.horizontal_wrapped(|ui| {
            ui.add_space(6.0);
            if ui.button("=").on_hover_text(crate::tl!("Evaluate")).clicked() {
                st.evaluate(app);
            }
            if ui.button(crate::tl!("Clear")).clicked() {
                st.input.clear();
                st.error = None;
            }
            let has_value = !st.input.trim().is_empty();
            if ui.add_enabled(has_value, egui::Button::new(crate::tl!("Paste to Command Line"))).clicked() {
                let v = st.input.trim().trim_start_matches('[').trim_end_matches(']').to_string();
                app.cmdline(&v);
            }
        });
        // Values read from the drawing.
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            ui.label(RichText::new(crate::tl!("Get from the Drawing")).color(t.text_dim));
        });
        ui.horizontal_wrapped(|ui| {
            ui.add_space(6.0);
            for (mode, label) in [
                ("Point", crate::tl!("Point")),
                ("Distance", crate::tl!("Distance")),
                ("Angle", crate::tl!("Angle")),
                ("Intersection", crate::tl!("Intersection")),
            ] {
                if ui.add_enabled(st.pending.is_none(), egui::Button::new(label)).clicked() {
                    st.pick(app, mode);
                }
            }
        });
        ui.separator();
        // History: click an entry to reuse its expression, its result with the second button.
        egui::CollapsingHeader::new(crate::tl!("History")).id_salt("qc-history").default_open(true).show(ui, |ui| {
            let mut reuse = None;
            for (expr, result) in st.history.iter().rev().take(20) {
                ui.horizontal(|ui| {
                    if ui.add(egui::Label::new(RichText::new(expr).font(crate::theme::mono())).sense(egui::Sense::click())).clicked() {
                        reuse = Some(expr.clone());
                    }
                    ui.label(RichText::new("=").color(t.text_dim));
                    if ui
                        .add(egui::Label::new(RichText::new(result).font(crate::theme::mono()).color(t.accent)).sense(egui::Sense::click()))
                        .clicked()
                    {
                        reuse = Some(if result.contains(',') { format!("[{result}]") } else { result.clone() });
                    }
                });
            }
            if let Some(e) = reuse {
                st.input = e;
            }
            if !st.history.is_empty() && ui.small_button(crate::tl!("Clear History")).clicked() {
                st.history.clear();
            }
        });
        // Units conversion.
        egui::CollapsingHeader::new(crate::tl!("Units Conversion")).id_salt("qc-units").default_open(true).show(ui, |ui| {
            let tables = st.units(app);
            egui::Grid::new("qc-units-grid").num_columns(2).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
                ui.label(crate::tl!("Type"));
                let kind_label = KINDS.get(st.kind).map_or("", |k| k.1);
                egui::ComboBox::from_id_salt("qc-kind").selected_text(crate::i18n::t(kind_label)).show_ui(ui, |ui| {
                    for (i, (_, label)) in KINDS.iter().enumerate() {
                        if ui.selectable_label(st.kind == i, crate::i18n::t(label)).clicked() {
                            st.kind = i;
                            st.default_pair();
                            st.conv_result.clear();
                        }
                    }
                });
                ui.end_row();
                let list = tables.get(st.kind).map(|(_, l)| l.clone()).unwrap_or_default();
                for (label, salt, which) in [(crate::tl!("From"), "qc-from", 0), (crate::tl!("To"), "qc-to", 1)] {
                    ui.label(label);
                    let sel = if which == 0 { &mut st.from } else { &mut st.to };
                    let shown = list.get(*sel).map(|u| u.1.clone()).unwrap_or_default();
                    egui::ComboBox::from_id_salt(salt).selected_text(shown).show_ui(ui, |ui| {
                        for (i, (_, sym)) in list.iter().enumerate() {
                            if ui.selectable_label(*sel == i, sym).clicked() {
                                *sel = i;
                            }
                        }
                    });
                    ui.end_row();
                }
                value_row(ui, &mut st.conv_value);
                ui.end_row();
                if ui.button(crate::tl!("Convert")).clicked() {
                    st.convert(app);
                }
                if !st.conv_result.is_empty() {
                    let r = ui
                        .add(egui::Label::new(RichText::new(&st.conv_result).font(crate::theme::mono()).color(t.accent)).sense(egui::Sense::click()));
                    if r.clicked() {
                        st.input.push_str(&st.conv_result);
                    }
                }
                ui.end_row();
            });
        });
        // Variables: click one to put its name into the expression.
        egui::CollapsingHeader::new(crate::tl!("Variables")).id_salt("qc-vars").default_open(true).show(ui, |ui| {
            let mut remove = None;
            for (i, (name, v)) in st.vars.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.add(egui::Label::new(RichText::new(name).font(crate::theme::mono())).sense(egui::Sense::click())).clicked() {
                        st.input.push_str(name);
                    }
                    ui.label(RichText::new(format!("= {}", var_text(v))).font(crate::theme::mono()).color(t.text_dim));
                    if ui.small_button("×").on_hover_text(crate::tl!("Delete")).clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                st.vars.remove(i);
            }
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut st.var_name).hint_text(crate::tl!("Name")).desired_width(60.0));
                ui.label("=");
                ui.add(egui::TextEdit::singleline(&mut st.var_expr).hint_text(crate::tl!("Expression")).desired_width(110.0));
                if ui.button(crate::tl!("Add")).clicked() {
                    st.add_var(app);
                }
            });
        });
    });
    ui.ctx().data_mut(|d| d.insert_temp(id(), st));
}

#[cfg(test)]
mod tests {
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

    use super::value_row;

    /// The conversion value field is named by its visible label under bundled fonts only, so a
    /// system face cannot hide a missing glyph and a bare "1" placeholder cannot pass as a name.
    #[test]
    fn value_field_is_named_by_its_visible_label() {
        let mut harness = Harness::builder().with_size(egui::vec2(320.0, 80.0)).build_ui(|ui| {
            let mut value = String::new();
            value_row(ui, &mut value);
        });
        harness.run();
        let field = harness.get_by_role_and_label(Role::TextInput, "Value");
        assert!(field.accesskit_node().labelled_by().next().is_some(), "named by the visible Value label");
        assert_eq!(field.accesskit_node().placeholder(), Some("Value"));
        assert!(harness.unnamed_widgets().is_empty(), "unnamed controls: {:?}", harness.unnamed_widgets());
    }
}
