//! The Attribute Definition dialog: ATTDEF typed or started from a menu or the Tool Sets opens
//! it (-ATTDEF and JSON calls keep the command line). It collects the modes, tag, prompt and
//! default, the insertion point and the text settings. OK runs one `attdef` call, or, with the
//! point to be specified on screen, starts -ATTDEF with every other answer so that only the start
//! point is asked. The dialog itself never changes the drawing.

use cadcraft_engine::doc::{EntityKind, HAlign, VAlign};
use egui::RichText;
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// Justifications as the dialog and the Properties palette name them, with TEXT's codes.
pub const JUSTIFICATIONS: [(&str, &str); 15] = [
    ("Left", "L"),
    ("Center", "C"),
    ("Right", "R"),
    ("Align", "A"),
    ("Middle", "M"),
    ("Fit", "F"),
    ("Top left", "TL"),
    ("Top center", "TC"),
    ("Top right", "TR"),
    ("Middle left", "ML"),
    ("Middle center", "MC"),
    ("Middle right", "MR"),
    ("Bottom left", "BL"),
    ("Bottom center", "BC"),
    ("Bottom right", "BR"),
];

/// The name of a text's justification.
pub fn justification_name(h: HAlign, v: VAlign) -> &'static str {
    let code = match (h, v) {
        (HAlign::Left, VAlign::Baseline) => "L",
        (HAlign::Center, VAlign::Baseline) => "C",
        (HAlign::Right, VAlign::Baseline) => "R",
        (HAlign::Aligned, _) => "A",
        (HAlign::Middle, _) => "M",
        (HAlign::Fit, _) => "F",
        (HAlign::Left, VAlign::Top) => "TL",
        (HAlign::Center, VAlign::Top) => "TC",
        (HAlign::Right, VAlign::Top) => "TR",
        (HAlign::Left, VAlign::Middle) => "ML",
        (HAlign::Center, VAlign::Middle) => "MC",
        (HAlign::Right, VAlign::Middle) => "MR",
        (HAlign::Left, VAlign::Bottom) => "BL",
        (HAlign::Center, VAlign::Bottom) => "BC",
        (HAlign::Right, VAlign::Bottom) => "BR",
    };
    JUSTIFICATIONS.iter().find(|(_, c)| *c == code).map_or("Left", |(n, _)| n)
}

/// The TEXT code of a justification name.
pub fn justification_code(name: &str) -> &'static str {
    JUSTIFICATIONS.iter().find(|(n, _)| *n == name).map_or("L", |(_, c)| c)
}

/// The dialog's state. The modes are kept between uses, as AutoCAD's AFLAGS keeps them.
#[derive(Default)]
pub struct State {
    pub invisible: bool,
    pub constant: bool,
    pub verify: bool,
    pub preset: bool,
    pub lock_position: bool,
    pub tag: String,
    pub prompt: String,
    pub default: String,
    /// The start point is picked on screen after OK.
    pub on_screen: bool,
    pub x: String,
    pub y: String,
    /// A name from [`JUSTIFICATIONS`].
    pub justification: String,
    pub style: String,
    pub height: String,
    /// Degrees, as typed.
    pub rotation: String,
    /// Under the previous definition, with its text settings.
    pub align_below: bool,
    /// Why the last OK failed, shown until the next edit.
    pub error: Option<String>,
}

fn number_text(v: f64) -> String {
    let s = format!("{v:.4}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

/// ATTDEF typed or from a menu or the Tool Sets opens the dialog; -ATTDEF and JSON calls keep the
/// command line and the JSON form.
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    (matches!(id, "attdef" | "att") && params.is_null() && app.session.state().is_ok()).then(|| {
        open(app);
        Ok(Value::Null)
    })
}

/// Open the dialog for a new definition: the current text style and TEXTSIZE, the point on
/// screen, the modes as last used.
pub fn open(app: &mut CadApp) {
    let (style, height) = app
        .session
        .doc()
        .map(|d| (d.header.str("TEXTSTYLE", "Standard"), d.header.f64("TEXTSIZE", 0.2)))
        .unwrap_or_else(|_| ("Standard".into(), 0.2));
    let st = &mut app.attdef;
    st.tag.clear();
    st.prompt.clear();
    st.default.clear();
    st.error = None;
    st.on_screen = true;
    st.x = "0".into();
    st.y = "0".into();
    st.justification = "Left".into();
    st.style = style;
    st.height = number_text(height);
    st.rotation = "0".into();
    st.align_below = false;
    app.ui.dialog = Some("attdef".into());
}

/// Whether the space being edited has an attribute definition to align below.
fn has_previous(app: &CadApp) -> bool {
    app.session.doc().ok().and_then(|d| d.space(&app.session.space())).is_some_and(|sp| sp.iter().any(|e| matches!(e.kind, EntityKind::AttDef(_))))
}

pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let t = Tokens::get();
    let Ok(d) = app.session.doc() else {
        *open = false;
        return;
    };
    let styles: Vec<String> = d.text_styles.iter().map(|s| s.name.clone()).collect();
    let fixed = d.text_style(&app.attdef.style).map(|s| s.height).filter(|h| *h > 0.0);
    let previous = has_previous(app);
    let (mut ok, mut cancel) = (false, false);
    egui::Window::new(crate::tl!("Attribute Definition"))
        .id(egui::Id::unique("Attribute Definition"))
        .open(open)
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            let st = &mut app.attdef;
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(150.0);
                    ui.label(RichText::new(crate::tl!("Mode")).color(t.text_dim));
                    ui.checkbox(&mut st.invisible, crate::tl!("Invisible"));
                    ui.checkbox(&mut st.constant, crate::tl!("Constant"));
                    ui.checkbox(&mut st.verify, crate::tl!("Verify"));
                    ui.checkbox(&mut st.preset, crate::tl!("Preset"));
                    ui.checkbox(&mut st.lock_position, crate::tl!("Lock position"));
                });
                ui.vertical(|ui| {
                    ui.label(RichText::new(crate::tl!("Attribute")).color(t.text_dim));
                    egui::Grid::new("attdef-attribute").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                        ui.label(crate::tl!("Tag"));
                        if ui.add(egui::TextEdit::singleline(&mut st.tag).desired_width(260.0)).changed() {
                            st.error = None;
                        }
                        ui.end_row();
                        ui.label(crate::tl!("Prompt"));
                        ui.add_enabled(!st.constant, egui::TextEdit::singleline(&mut st.prompt).desired_width(260.0));
                        ui.end_row();
                        ui.label(if st.constant { crate::tl!("Value") } else { crate::tl!("Default") });
                        ui.add(egui::TextEdit::singleline(&mut st.default).desired_width(260.0));
                        ui.end_row();
                    });
                });
            });
            ui.add_space(6.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(150.0);
                    ui.label(RichText::new(crate::tl!("Insertion Point")).color(t.text_dim));
                    ui.add_enabled_ui(!st.align_below, |ui| {
                        ui.checkbox(&mut st.on_screen, crate::tl!("Specify on-screen"));
                        ui.add_enabled_ui(!st.on_screen, |ui| {
                            egui::Grid::new("attdef-point").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                                ui.label("X:");
                                ui.add(egui::TextEdit::singleline(&mut st.x).desired_width(90.0));
                                ui.end_row();
                                ui.label("Y:");
                                ui.add(egui::TextEdit::singleline(&mut st.y).desired_width(90.0));
                                ui.end_row();
                            });
                        });
                    });
                });
                ui.vertical(|ui| {
                    ui.label(RichText::new(crate::tl!("Text Settings")).color(t.text_dim));
                    ui.add_enabled_ui(!st.align_below, |ui| {
                        egui::Grid::new("attdef-text").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                            ui.label(crate::tl!("Justification"));
                            craft_ui::ComboBox::from_id_salt("attdef-justify")
                                .width(180.0)
                                .selected_text(crate::i18n::justify(&st.justification))
                                .show_ui(ui, |ui| {
                                    for (name, _) in JUSTIFICATIONS {
                                        ui.selectable_value(&mut st.justification, name.to_string(), crate::i18n::justify(name));
                                    }
                                });
                            ui.end_row();
                            ui.label(crate::tl!("Text style"));
                            craft_ui::ComboBox::from_id_salt("attdef-style").width(180.0).selected_text(st.style.clone()).show_ui(ui, |ui| {
                                for n in &styles {
                                    ui.selectable_value(&mut st.style, n.clone(), n);
                                }
                            });
                            ui.end_row();
                            ui.label(crate::tl!("Height"));
                            match fixed {
                                // A fixed-height style: its height, not to be changed here.
                                Some(h) => {
                                    let mut shown = number_text(h);
                                    ui.add_enabled(false, egui::TextEdit::singleline(&mut shown).desired_width(90.0));
                                }
                                None => {
                                    ui.add(egui::TextEdit::singleline(&mut st.height).desired_width(90.0));
                                }
                            }
                            ui.end_row();
                            ui.label(crate::tl!("Rotation"));
                            ui.add(egui::TextEdit::singleline(&mut st.rotation).desired_width(90.0));
                            ui.end_row();
                        });
                    });
                });
            });
            ui.add_space(6.0);
            ui.add_enabled(previous, egui::Checkbox::new(&mut st.align_below, crate::tl!("Align below previous attribute definition")));
            if let Some(e) = &st.error {
                ui.colored_label(t.warn, e);
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::tl!("Cancel")).clicked() {
                        cancel = true;
                    }
                    if ui.button(crate::tl!("OK")).clicked() {
                        ok = true;
                    }
                });
            });
        });
    if cancel || (ok && confirm(app)) {
        *open = false;
    }
}

/// OK: make the definition, or start -ATTDEF to pick its start point. Returns whether the dialog
/// is done; if not, it shows why and stays open.
pub fn confirm(app: &mut CadApp) -> bool {
    let st = &app.attdef;
    let tag = st.tag.trim().to_string();
    if tag.is_empty() || tag.contains(' ') {
        app.attdef.error = Some(crate::tl!("The tag must not be empty or contain spaces").to_string());
        return false;
    }
    let mut p = json!({
        "tag": tag, "prompt": st.prompt, "default": st.default,
        "invisible": st.invisible, "constant": st.constant, "verify": st.verify, "preset": st.preset, "lockPosition": st.lock_position,
    });
    let (align_below, on_screen) = (st.align_below, st.on_screen);
    if align_below {
        p["below"] = json!(true);
    } else {
        p["justify"] = json!(justification_code(&st.justification));
        p["style"] = json!(st.style);
        let fixed = app.session.doc().ok().and_then(|d| d.text_style(&st.style)).is_some_and(|s| s.height > 0.0);
        let mut error = None;
        if !fixed {
            match cadcraft_engine::units::parse_distance(st.height.trim()) {
                Some(h) if h > 0.0 => p["height"] = json!(h),
                _ => error = Some(crate::tf!("Not a positive height: {text}", text = st.height.trim())),
            }
        }
        match app.session.angle_settings().direction(st.rotation.trim()) {
            Some(r) => p["rotation"] = json!(r.to_degrees()),
            None => error = error.or_else(|| Some(crate::tf!("Not an angle: {text}", text = st.rotation.trim()))),
        }
        if !on_screen {
            match (cadcraft_engine::units::parse_distance(st.x.trim()), cadcraft_engine::units::parse_distance(st.y.trim())) {
                (Some(x), Some(y)) => p["at"] = json!([x, y]),
                _ => error = error.or_else(|| Some(crate::tf!("Not a point: {text}", text = format!("{}, {}", st.x.trim(), st.y.trim())))),
            }
        }
        if let Some(e) = error {
            app.attdef.error = Some(e);
            return false;
        }
    }
    if align_below || !on_screen {
        return match app.run("attdef", p) {
            Ok(_) => true,
            Err(e) => {
                app.attdef.error = Some(e);
                false
            }
        };
    }
    // The start point on screen: -ATTDEF takes the other answers and asks only for the point (and,
    // for Align and Fit, the baseline's second endpoint).
    app.session.attdef_answers = Some(p);
    app.start("-attdef");
    if app.session.running.as_ref().is_some_and(|r| r.id == "attdef") {
        true
    } else {
        app.session.attdef_answers = None;
        app.attdef.error = Some(crate::tl!("ATTDEF could not start").to_string());
        false
    }
}
