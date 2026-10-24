//! The Dimension Style Manager (DIMSTYLE): the drawing's dimension styles with a preview, Set
//! Current, New, Modify, Override, Compare, Rename and Delete. Modify, Override and New edit every
//! style variable in AutoCAD's tabs (Lines, Symbols and Arrows, Text, Fit, Primary Units,
//! Alternate Units, Tolerances), from the engine's table of dimension properties; the preview
//! draws sample dimensions with the settings being edited.

use std::hash::{Hash, Hasher};

use cadcraft_engine::doc::{DimStyle, Drawing, EntityKind};
use cadcraft_engine::properties::{DIM_PROPS, DimProp, DimPropKind, dim_value, join_post, split_post};
use egui::{RichText, vec2};
use serde_json::{Map, Value, json};

use crate::CadApp;
use crate::parametric::expr_field;
use crate::theme::Tokens;

/// The current style's overrides in the list.
const OVERRIDES: &str = "<style overrides>";
const STATE: &str = "dimstyle_manager";
/// The editing tabs, in AutoCAD's order.
const TABS: usize = 7;

fn tab_label(i: usize) -> &'static str {
    match i {
        0 => crate::tl!("Lines"),
        1 => crate::tl!("Symbols and Arrows"),
        2 => crate::tl!("Text"),
        3 => crate::tl!("Fit"),
        4 => crate::tl!("Primary Units"),
        5 => crate::tl!("Alternate Units"),
        _ => crate::tl!("Tolerances"),
    }
}

#[derive(Clone, Default, PartialEq)]
enum Page {
    #[default]
    List,
    New,
    Edit,
    Compare,
}

/// What the editing pages change: an existing style, the current style's overrides, or a style
/// being created.
#[derive(Clone, Default, PartialEq)]
enum Target {
    #[default]
    Modify,
    Override,
    Create,
}

#[derive(Clone, Default)]
struct State {
    page: Page,
    /// The style chosen in the list, or [`OVERRIDES`].
    selected: String,
    in_use_only: bool,
    new_name: String,
    start_with: String,
    new_annotative: bool,
    /// The style the editing pages show, its settings as edited, and what OK does with them.
    editing: String,
    target: Target,
    values: Map<String, Value>,
    original: Map<String, Value>,
    tab: usize,
    compare_with: String,
    /// (style, new name) while renaming.
    renaming: Option<(String, String)>,
    /// OK was clicked on an editing page: the entry the list chooses once its commands succeed
    /// (a refused value keeps the page open with every edit).
    after_ok: Option<String>,
}

/// The tab a variable is edited on.
fn tab_of(p: &DimProp) -> usize {
    match p.group {
        "Lines & Arrows" => {
            let symbol = matches!(
                p.field,
                "arrowBlock"
                    | "arrowBlock1"
                    | "arrowBlock2"
                    | "leaderArrowBlock"
                    | "arrowSize"
                    | "tickSize"
                    | "centerMark"
                    | "arcSymbol"
                    | "jogAngle"
            );
            usize::from(symbol)
        }
        "Text" => 2,
        "Fit" => 3,
        "Primary Units" => 4,
        "Alternate Units" => 5,
        _ => 6,
    }
}

fn style_json(st: &DimStyle) -> Map<String, Value> {
    match serde_json::to_value(st) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

fn from_json(m: &Map<String, Value>) -> DimStyle {
    serde_json::from_value(Value::Object(m.clone())).unwrap_or_default()
}

/// Names of the dimension styles some dimension (or leader) uses.
fn in_use(d: &Drawing) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).chain(d.blocks.values().map(|b| &b.entities));
    for st in stores {
        for e in st.iter() {
            let name = match &e.kind {
                EntityKind::Dimension(dm) => &dm.style,
                EntityKind::Leader(l) => &l.style,
                EntityKind::Tolerance(t) => &t.style,
                _ => continue,
            };
            if !out.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                out.push(name.clone());
            }
        }
    }
    out
}

pub(super) fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> super::Action {
    let Ok(d) = app.session.doc() else {
        super::no_drawing(ctx, crate::tl!("Dimension Style Manager"), open);
        return None;
    };
    let mut st: State = super::temp(ctx, STATE).unwrap_or_default();
    let current = d.header.str("DIMSTYLE", "Standard");
    let overrides = d.dim_overrides();
    let valid = |n: &str| n == OVERRIDES && !overrides.is_empty() || d.dim_style(n).is_some();
    if !valid(&st.selected) {
        st.selected = current.clone();
    }
    let mut actions: Vec<(&'static str, Value)> = Vec::new();
    let title = match (&st.page, &st.target) {
        (Page::List, _) => crate::tl!("Dimension Style Manager").to_string(),
        (Page::New, _) => crate::tl!("Create New Dimension Style").to_string(),
        (Page::Edit, Target::Modify) => crate::tf!("Modify Dimension Style: {style}", style = st.editing),
        (Page::Edit, Target::Override) => crate::tf!("Override Current Style: {style}", style = st.editing),
        (Page::Edit, Target::Create) => crate::tf!("New Dimension Style: {style}", style = st.editing),
        (Page::Compare, _) => crate::tl!("Compare Dimension Styles").to_string(),
    };
    egui::Window::new(title).id(egui::Id::new(STATE)).open(open).default_size(vec2(720.0, 480.0)).resizable(true).collapsible(false).show(
        ctx,
        |ui| match st.page {
            Page::List => list_page(ui, d, &mut st, &current, &overrides, &mut actions),
            Page::New => new_page(ui, d, &mut st),
            Page::Edit => edit_page(ui, d, &mut st, &mut actions),
            Page::Compare => compare_page(ui, d, &mut st, &current, &overrides),
        },
    );
    // Errors are echoed on the command line by `run`; a failed step stops the rest.
    let mut ok = true;
    for (cmd, p) in actions {
        if app.run(cmd, p).is_err() {
            ok = false;
            break;
        }
    }
    if let Some(selected) = st.after_ok.take()
        && ok
    {
        st.selected = selected;
        st.page = Page::List;
    }
    // Closed (the title bar's X), it opens on the list next time, not on a page left open.
    super::set_temp(ctx, STATE, if *open { st } else { State::default() });
    None
}

fn list_page(
    ui: &mut egui::Ui,
    d: &Drawing,
    st: &mut State,
    current: &str,
    overrides: &Map<String, Value>,
    actions: &mut Vec<(&'static str, Value)>,
) {
    let t = Tokens::get();
    ui.label(RichText::new(crate::tf!("Current dimension style: {style}", style = current)).color(t.text_dim));
    ui.add_space(4.0);
    let used = in_use(d);
    ui.horizontal_top(|ui| {
        // The styles, the current style's overrides under it.
        ui.vertical(|ui| {
            ui.set_width(190.0);
            ui.label(RichText::new(crate::tl!("Styles")).strong());
            egui::Frame::group(ui.style()).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("dimstyle_list").max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                    for s in &d.dim_styles {
                        if st.in_use_only && !used.iter().any(|n| n.eq_ignore_ascii_case(&s.name)) && !s.name.eq_ignore_ascii_case(current) {
                            continue;
                        }
                        let mut label = RichText::new(&s.name);
                        if s.name.eq_ignore_ascii_case(current) {
                            label = label.strong();
                        }
                        let resp = ui.selectable_label(st.selected == s.name, label);
                        if resp.clicked() {
                            st.selected = s.name.clone();
                        }
                        resp.context_menu(|ui| {
                            if ui.add_enabled(!s.name.eq_ignore_ascii_case(current), egui::Button::new(crate::tl!("Set Current"))).clicked() {
                                actions.push(("dimstyle.current", json!({ "name": s.name })));
                                ui.close();
                            }
                            if ui.button(crate::tl!("Rename")).clicked() {
                                st.renaming = Some((s.name.clone(), s.name.clone()));
                                ui.close();
                            }
                            if ui.add_enabled(deletable(&s.name, current, &used), egui::Button::new(crate::tl!("Delete"))).clicked() {
                                actions.push(("dimstyle.delete", json!({ "name": s.name })));
                                ui.close();
                            }
                        });
                        if s.name.eq_ignore_ascii_case(current) && !overrides.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add_space(14.0);
                                let resp = ui.selectable_label(st.selected == OVERRIDES, crate::tl!("<style overrides>"));
                                if resp.clicked() {
                                    st.selected = OVERRIDES.into();
                                }
                                resp.context_menu(|ui| {
                                    if ui.button(crate::tl!("Save to current style")).clicked() {
                                        let mut p = overrides.clone();
                                        p.insert("name".into(), json!(current));
                                        actions.push(("dimstyle", Value::Object(p)));
                                        ui.close();
                                    }
                                    if ui.button(crate::tl!("Delete")).clicked() {
                                        actions.push(("dimstyle.current", json!({ "name": current })));
                                        ui.close();
                                    }
                                });
                            });
                        }
                    }
                });
            });
            egui::ComboBox::from_id_salt("dimstyle_list_filter")
                .selected_text(if st.in_use_only { crate::tl!("Styles in use") } else { crate::tl!("All styles") })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut st.in_use_only, false, crate::tl!("All styles"));
                    ui.selectable_value(&mut st.in_use_only, true, crate::tl!("Styles in use"));
                });
            if let Some((from, to)) = &mut st.renaming {
                let from = from.clone();
                let mut done = false;
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(to).desired_width(120.0));
                    if ui.button(crate::tl!("OK")).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                        if !bad_name(to.trim()) && *to != from {
                            actions.push(("dimstyle.rename", json!({ "from": from, "to": to.trim() })));
                            if st.selected == from {
                                st.selected = to.trim().to_string();
                            }
                        }
                        done = true;
                    }
                });
                if done {
                    st.renaming = None;
                }
            }
        });
        // The preview and description of the chosen style.
        ui.vertical(|ui| {
            let shown = if st.selected == OVERRIDES {
                d.dim_style(current).cloned().unwrap_or_default().with_overrides(overrides)
            } else {
                d.dim_style(&st.selected).cloned().unwrap_or_default()
            };
            let label = if st.selected == OVERRIDES {
                crate::tf!("Preview of: {style} (overrides)", style = current)
            } else {
                crate::tf!("Preview of: {style}", style = st.selected)
            };
            ui.label(RichText::new(label).strong());
            preview(ui, d, &shown, vec2(320.0, 240.0));
            ui.label(RichText::new(crate::tl!("Description")).strong());
            ui.label(description(d, &st.selected, current, overrides));
        });
        // The buttons.
        ui.vertical(|ui| {
            let w = vec2(110.0, 22.0);
            let is_style = st.selected != OVERRIDES;
            if ui
                .add_enabled(is_style && !st.selected.eq_ignore_ascii_case(current), egui::Button::new(crate::tl!("Set Current")).min_size(w))
                .clicked()
            {
                actions.push(("dimstyle.current", json!({ "name": st.selected })));
            }
            if ui.add(egui::Button::new(crate::tl!("New...")).min_size(w)).clicked() {
                st.start_with = if is_style { st.selected.clone() } else { current.to_string() };
                st.new_name = new_name(d, &st.start_with);
                st.new_annotative = d.dim_style(&st.start_with).is_some_and(|s| s.annotative);
                st.page = Page::New;
            }
            if ui.add_enabled(is_style, egui::Button::new(crate::tl!("Modify...")).min_size(w)).clicked()
                && let Some(s) = d.dim_style(&st.selected)
            {
                start_edit(st, s.name.clone(), Target::Modify, style_json(s));
            }
            if ui.add(egui::Button::new(crate::tl!("Override...")).min_size(w)).clicked()
                && let Some(s) = d.dim_style(current)
            {
                // The current settings: the style with the overrides it already has.
                start_edit(st, s.name.clone(), Target::Override, style_json(&s.with_overrides(overrides)));
                st.original = style_json(s);
            }
            if ui.add(egui::Button::new(crate::tl!("Compare...")).min_size(w)).clicked() {
                st.compare_with = current.to_string();
                st.page = Page::Compare;
            }
            if ui.add_enabled(is_style && deletable(&st.selected, current, &used), egui::Button::new(crate::tl!("Delete")).min_size(w)).clicked() {
                actions.push(("dimstyle.delete", json!({ "name": st.selected })));
            }
        });
    });
}

/// A name no new dimension style can have: the engine's rule (-DIMSTYLE's; `~` is its Restore's
/// "compare with").
fn bad_name(name: &str) -> bool {
    !cadcraft_engine::cmd::usable_dim_style_name(name)
}

/// Whether style `name` can be deleted: not Standard, the current style or a style in use.
fn deletable(name: &str, current: &str, used: &[String]) -> bool {
    !name.eq_ignore_ascii_case("Standard") && !name.eq_ignore_ascii_case(current) && !used.iter().any(|n| n.eq_ignore_ascii_case(name))
}

fn start_edit(st: &mut State, name: String, target: Target, values: Map<String, Value>) {
    st.editing = name;
    st.target = target;
    st.original = values.clone();
    st.values = values;
    st.page = Page::Edit;
}

/// "Copy of <style>", numbered when that is taken.
fn new_name(d: &Drawing, base: &str) -> String {
    let first = format!("Copy of {base}");
    (1..1000).map(|i| if i == 1 { first.clone() } else { format!("{first} ({i})") }).find(|n| d.dim_style(n).is_none()).unwrap_or(first)
}

/// The chosen style's description: its name, or the current style and what its overrides change.
fn description(d: &Drawing, selected: &str, current: &str, overrides: &Map<String, Value>) -> String {
    if selected != OVERRIDES {
        let anno = if d.dim_style(selected).is_some_and(|s| s.annotative) { crate::tl!("Annotative").to_string() } else { String::new() };
        return [selected.to_string(), anno].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
    }
    let base = d.dim_style(current).cloned().unwrap_or_default();
    let mut parts = vec![current.to_string()];
    let values = Value::Object(style_json(&base.with_overrides(overrides)));
    for p in DIM_PROPS.iter().filter(|p| overrides.contains_key(p.field)) {
        parts.push(format!("{} = {}", crate::i18n::t(p.label), shown(p, &values)));
    }
    parts.join(" + ")
}

/// `p` of a style's `values` as the dialog shows it: a choice's label (a precision as 0.00, an
/// arrowhead by name), a color's name, Yes or No.
fn shown(p: &DimProp, values: &Value) -> String {
    let v = dim_value(p, values);
    if let Some(o) = p.kind.options().into_iter().find(|o| o.value == v) {
        return crate::i18n::t(&o.label).to_string();
    }
    match v {
        Value::String(s) => crate::i18n::t(&s).to_string(),
        Value::Bool(b) => crate::i18n::t(if b { "Yes" } else { "No" }).to_string(),
        Value::Null => String::new(),
        v => v.to_string(),
    }
}

fn new_page(ui: &mut egui::Ui, d: &Drawing, st: &mut State) {
    egui::Grid::new("dimstyle_new").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
        ui.label(crate::tl!("New style name"));
        ui.add(egui::TextEdit::singleline(&mut st.new_name).desired_width(220.0));
        ui.end_row();
        ui.label(crate::tl!("Start with"));
        egui::ComboBox::from_id_salt("dimstyle_start_with").selected_text(st.start_with.clone()).width(220.0).show_ui(ui, |ui| {
            for s in &d.dim_styles {
                ui.selectable_value(&mut st.start_with, s.name.clone(), &s.name);
            }
        });
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut st.new_annotative, crate::tl!("Annotative"));
        ui.end_row();
    });
    let name = st.new_name.trim().to_string();
    let taken = d.dim_style(&name).is_some();
    if taken {
        ui.label(RichText::new(crate::tl!("A dimension style with this name already exists.")).color(Tokens::get().text_dim));
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.add_enabled(!bad_name(&name) && !taken, egui::Button::new(crate::tl!("Continue"))).clicked() {
            let mut values = d.dim_style(&st.start_with).map(style_json).unwrap_or_default();
            values.insert("name".into(), json!(name));
            values.insert("annotative".into(), json!(st.new_annotative));
            start_edit(st, name, Target::Create, values);
        }
        if ui.button(crate::tl!("Cancel")).clicked() {
            st.page = Page::List;
        }
    });
}

fn edit_page(ui: &mut egui::Ui, d: &Drawing, st: &mut State, actions: &mut Vec<(&'static str, Value)>) {
    ui.horizontal(|ui| {
        for i in 0..TABS {
            ui.selectable_value(&mut st.tab, i, tab_label(i));
        }
    });
    ui.separator();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(360.0);
            egui::ScrollArea::vertical().id_salt(("dimstyle_tab", st.tab)).max_height(330.0).auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new(("dimstyle_fields", st.tab)).num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                    fields(ui, d, st);
                });
            });
        });
        ui.vertical(|ui| {
            preview(ui, d, &from_json(&st.values), vec2(300.0, 225.0));
        });
    });
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button(crate::tl!("OK")).clicked() {
            let changed: Map<String, Value> = st
                .values
                .iter()
                .filter(|(k, v)| k.as_str() != "name" && st.original.get(k.as_str()) != Some(*v))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            match st.target {
                Target::Modify if !changed.is_empty() => {
                    let mut p = changed;
                    p.insert("name".into(), json!(st.editing));
                    p.insert("current".into(), json!(false));
                    actions.push(("dimstyle", Value::Object(p)));
                }
                // What differs from the style is the whole override set: a value set back to the
                // style's drops its override.
                Target::Override if !changed.is_empty() || !d.dim_overrides().is_empty() => {
                    let mut p = changed;
                    p.insert("clear".into(), json!(true));
                    actions.push(("dimstyle.overrides", Value::Object(p)));
                }
                Target::Create => {
                    // Every variable, so the new style is what was shown whatever the current style is.
                    let mut p: Map<String, Value> =
                        st.values.iter().filter(|(k, _)| k.as_str() != "name").map(|(k, v)| (k.clone(), v.clone())).collect();
                    p.insert("name".into(), json!(st.editing));
                    p.insert("current".into(), json!(false));
                    actions.push(("dimstyle", Value::Object(p)));
                }
                _ => {}
            }
            st.after_ok = Some(if st.target == Target::Override { OVERRIDES.into() } else { st.editing.clone() });
        }
        if ui.button(crate::tl!("Cancel")).clicked() {
            st.page = Page::List;
        }
    });
}

/// The fields of the chosen tab.
fn fields(ui: &mut egui::Ui, d: &Drawing, st: &mut State) {
    let tab = st.tab;
    // A fixed-height text style decides the text height (as in AutoCAD the field is off).
    let fixed = st
        .values
        .get("textStyle")
        .and_then(Value::as_str)
        .and_then(|n| d.text_style(n))
        .filter(|ts| ts.height > 0.0)
        .map(|ts| (ts.name.clone(), ts.height));
    let (lu, lp) = (d.header.i64("LUNITS", 2), d.header.i64("LUPREC", 4));
    for p in DIM_PROPS.iter().filter(|p| tab_of(p) == tab) {
        match p.field {
            // Shown as a prefix and a suffix.
            "post" | "altPost" => {
                let template = st.values.get(p.field).and_then(Value::as_str).unwrap_or_default().to_string();
                let (pre, suf) = split_post(&template);
                let alt = p.field == "altPost";
                let (lp_, ls_) = if alt { ("Alt prefix", "Alt suffix") } else { ("Dim prefix", "Dim suffix") };
                let (mut pre2, mut suf2) = (pre.clone(), suf.clone());
                ui.label(crate::i18n::t(lp_));
                ui.add(egui::TextEdit::singleline(&mut pre2).desired_width(160.0));
                ui.end_row();
                ui.label(crate::i18n::t(ls_));
                ui.add(egui::TextEdit::singleline(&mut suf2).desired_width(160.0));
                ui.end_row();
                if pre2 != pre || suf2 != suf {
                    st.values.insert(p.field.into(), json!(join_post(&pre2, &suf2)));
                }
                continue;
            }
            // Leading and trailing zeros (DIMZIN bits 4 and 8; DIMAZIN 1 and 2).
            "zeroSuppression" | "altZeroSuppression" | "tolZeroSuppression" | "angularZeroSuppression" => {
                let bits = st.values.get(p.field).and_then(Value::as_u64).unwrap_or(0);
                let (lead, trail) = if p.field == "angularZeroSuppression" { (1, 2) } else { (4, 8) };
                ui.label(crate::i18n::t(p.label));
                let mut b = bits;
                ui.horizontal(|ui| {
                    let mut l = b & lead != 0;
                    let mut t = b & trail != 0;
                    if ui.checkbox(&mut l, crate::tl!("Leading")).changed() {
                        b ^= lead;
                    }
                    if ui.checkbox(&mut t, crate::tl!("Trailing")).changed() {
                        b ^= trail;
                    }
                });
                if b != bits {
                    st.values.insert(p.field.into(), json!(b));
                }
                ui.end_row();
                continue;
            }
            _ => {}
        }
        ui.label(crate::i18n::t(p.label));
        let id = ("dimstyle_field", &st.editing, p.field);
        // A per-end arrowhead left empty shows the one DIMBLK draws.
        let cur = if p.kind == DimPropKind::Arrow {
            dim_value(p, &Value::Object(st.values.clone()))
        } else {
            st.values.get(p.field).cloned().unwrap_or(Value::Null)
        };
        let new = match p.kind {
            DimPropKind::Distance | DimPropKind::Number | DimPropKind::Angle => {
                if p.field == "textHeight"
                    && let Some((name, h)) = &fixed
                {
                    ui.add_enabled(false, egui::Label::new(cadcraft_engine::units::format_distance(*h, lu, lp))).on_disabled_hover_text(crate::tf!(
                        "Set by text style \"{style}\", which has a fixed height. Give the style a height of 0 to use the dimension text height.",
                        style = name
                    ));
                    None
                } else {
                    let v = cur.as_f64().unwrap_or(0.0);
                    let shown = if p.kind == DimPropKind::Distance { cadcraft_engine::units::format_distance(v, lu, lp) } else { format!("{v}") };
                    expr_field(ui, egui::Id::new(id), &shown)
                        .and_then(|t| cadcraft_engine::units::parse_distance(&t))
                        .filter(|f| f.is_finite())
                        .map(|f| json!(f))
                }
            }
            DimPropKind::Bool => {
                let mut b = cur.as_bool().unwrap_or(false);
                ui.checkbox(&mut b, "").changed().then(|| json!(b))
            }
            DimPropKind::Color => {
                let c = serde_json::from_value::<cadcraft_color::Color>(cur.clone()).unwrap_or(cadcraft_color::Color::ByBlock);
                let mut out = None;
                egui::ComboBox::from_id_salt(id).selected_text(crate::i18n::t(&c.name())).width(160.0).show_ui(ui, |ui| {
                    for (n, aci) in [
                        ("ByBlock", 0),
                        ("ByLayer", 256),
                        ("Red", 1),
                        ("Yellow", 2),
                        ("Green", 3),
                        ("Cyan", 4),
                        ("Blue", 5),
                        ("Magenta", 6),
                        ("White", 7),
                    ] {
                        if ui
                            .selectable_label(c.to_aci() == aci && c.name() == cadcraft_color::Color::from_aci(aci).name(), crate::i18n::t(n))
                            .clicked()
                        {
                            out = serde_json::to_value(cadcraft_color::Color::from_aci(aci)).ok();
                        }
                    }
                });
                out
            }
            DimPropKind::TextStyle => combo(ui, egui::Id::new(id), &cur, d.text_styles.iter().map(|s| (json!(s.name), s.name.clone())).collect()),
            DimPropKind::Linetype => {
                let mut names = vec![(json!(""), crate::i18n::t("ByBlock").to_string())];
                names.extend(
                    d.linetypes
                        .iter()
                        .filter(|l| !l.name.eq_ignore_ascii_case("ByLayer") && !l.name.eq_ignore_ascii_case("ByBlock"))
                        .map(|l| (json!(l.name), l.name.clone())),
                );
                combo(ui, egui::Id::new(id), &cur, names)
            }
            _ => {
                let options = p.kind.options().into_iter().map(|o| (o.value, crate::i18n::t(&o.label).to_string())).collect();
                combo(ui, egui::Id::new(id), &cur, options)
            }
        };
        if let Some(v) = new.filter(|v| *v != cur) {
            st.values.insert(p.field.into(), v);
        }
        ui.end_row();
    }
}

/// A drop-down of `options` (value, label) showing `cur`'s label; the value picked.
fn combo(ui: &mut egui::Ui, id: egui::Id, cur: &Value, options: Vec<(Value, String)>) -> Option<Value> {
    let shown = options
        .iter()
        .find(|(v, _)| v == cur)
        .map_or_else(|| cur.as_str().map(str::to_string).unwrap_or_else(|| cur.to_string()), |(_, l)| l.clone());
    let mut out = None;
    egui::ComboBox::from_id_salt(id).selected_text(shown).width(160.0).show_ui(ui, |ui| {
        for (v, label) in options {
            if ui.selectable_label(v == *cur, label).clicked() {
                out = Some(v);
            }
        }
    });
    out
}

fn compare_page(ui: &mut egui::Ui, d: &Drawing, st: &mut State, current: &str, overrides: &Map<String, Value>) {
    let mut names: Vec<String> = d.dim_styles.iter().map(|s| s.name.clone()).collect();
    // The current settings can be compared too: the current style with its overrides.
    if !overrides.is_empty() {
        names.push(OVERRIDES.to_string());
    }
    if !names.contains(&st.selected) {
        st.selected = current.to_string();
    }
    ui.horizontal(|ui| {
        ui.label(crate::tl!("Compare"));
        egui::ComboBox::from_id_salt("dimstyle_compare_a").selected_text(st.selected.clone()).show_ui(ui, |ui| {
            for n in &names {
                ui.selectable_value(&mut st.selected, n.clone(), n);
            }
        });
        ui.label(crate::tl!("With"));
        egui::ComboBox::from_id_salt("dimstyle_compare_b").selected_text(st.compare_with.clone()).show_ui(ui, |ui| {
            for n in &names {
                ui.selectable_value(&mut st.compare_with, n.clone(), n);
            }
        });
    });
    let style = |n: &str| {
        let st = if n == OVERRIDES {
            d.dim_style(current).cloned().unwrap_or_default().with_overrides(overrides)
        } else {
            d.dim_style(n).cloned().unwrap_or_default()
        };
        Value::Object(style_json(&st))
    };
    let (a, b) = (style(&st.selected), style(&st.compare_with));
    let same = st.selected == st.compare_with;
    let rows: Vec<&DimProp> = DIM_PROPS.iter().filter(|p| same || dim_value(p, &a) != dim_value(p, &b)).collect();
    ui.label(if same { crate::tf!("All properties of {style}", style = st.selected) } else { crate::tf!("{count} differences", count = rows.len()) });
    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
        egui::Grid::new("dimstyle_compare").num_columns(4).striped(true).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
            ui.label(RichText::new(crate::tl!("Description")).strong());
            ui.label(RichText::new(crate::tl!("Variable")).strong());
            ui.label(RichText::new(&st.selected).strong());
            if !same {
                ui.label(RichText::new(&st.compare_with).strong());
            }
            ui.end_row();
            for p in rows {
                let var = cadcraft_engine::doc::DIMVARS.iter().find(|(_, f)| *f == p.field).map_or("", |(v, _)| *v);
                ui.label(crate::i18n::t(p.label));
                ui.label(var);
                ui.label(shown(p, &a));
                if !same {
                    ui.label(shown(p, &b));
                }
                ui.end_row();
            }
        });
    });
    if ui.button(crate::tl!("Close")).clicked() {
        st.page = Page::List;
    }
}

/// Sample dimensions drawn with `style`: a plate with a hole and a slanted edge, dimensioned
/// linear, aligned, angular and diameter, sized to the style's text.
fn preview(ui: &mut egui::Ui, d: &Drawing, style: &DimStyle, size: egui::Vec2) {
    let (w, h) = (size.x.max(16.0) as u32, size.y.max(16.0) as u32);
    let mut hasher = std::hash::DefaultHasher::new();
    serde_json::to_string(style).unwrap_or_default().hash(&mut hasher);
    serde_json::to_string(&d.text_styles).unwrap_or_default().hash(&mut hasher);
    // The drawing area's color, as the theme and the user's model background give it.
    let bg = Tokens::get().canvas;
    (w, h, bg.to_array()).hash(&mut hasher);
    let key = hasher.finish();
    let id = egui::Id::new(("dimstyle_preview", size.x as u32, size.y as u32));
    let cached = ui.data(|m| m.get_temp::<(u64, egui::TextureHandle)>(id)).filter(|(k, _)| *k == key).map(|(_, t)| t);
    let tex = match cached {
        Some(t) => Some(t),
        None => sample(d, style, w, h, bg).map(|img| {
            let t = ui.ctx().load_texture("dimstyle_preview", img, egui::TextureOptions::LINEAR);
            ui.data_mut(|m| m.insert_temp(id, (key, t.clone())));
            t
        }),
    };
    match tex {
        Some(t) => {
            ui.image((t.id(), size));
        }
        None => {
            ui.allocate_space(size);
        }
    }
}

fn sample(d: &Drawing, style: &DimStyle, w: u32, h: u32, bg: egui::Color32) -> Option<egui::ColorImage> {
    let mut s = cadcraft_engine::Session::new();
    {
        let doc = s.doc_mut().ok()?;
        doc.text_styles.clone_from(&d.text_styles);
        doc.linetypes.clone_from(&d.linetypes);
        doc.dim_styles = vec![DimStyle { name: "Preview".into(), ..style.clone() }];
        doc.header.set_str("DIMSTYLE", "Preview");
        doc.sync_dim_vars();
    }
    // Sizes in text heights, so any style's text reads at preview size.
    let text = d.text_style(&style.text_style).map(|t| t.height).filter(|h| *h > 0.0).unwrap_or(style.text_height * style.effective_scale(1.0));
    let u = if text.is_finite() && text > 1e-9 { text } else { 0.18 };
    let (l, ht) = (14.0 * u, 9.0 * u);
    let run = |s: &mut cadcraft_engine::Session, cmd: &str, p: Value| s.execute(cmd, &p).ok();
    run(&mut s, "pline", json!({ "vertices": [[0, 0], [l, 0], [l, ht * 0.6], [l * 0.7, ht], [0, ht]], "closed": true }))?;
    let c = run(&mut s, "circle", json!({ "center": [l * 0.35, ht * 0.5], "radius": ht * 0.2 }))?;
    let circle = c.get("handle").or_else(|| c.get("handles").and_then(|h| h.get(0))).cloned().unwrap_or(Value::Null);
    run(&mut s, "dimlinear", json!({ "p1": [0, 0], "p2": [l, 0], "at": [l / 2.0, -3.0 * u] }));
    run(&mut s, "dimlinear", json!({ "p1": [0, 0], "p2": [0, ht], "at": [-3.0 * u, ht / 2.0] }));
    run(&mut s, "dimaligned", json!({ "p1": [l, ht * 0.6], "p2": [l * 0.7, ht], "at": [l + 1.5 * u, ht + 1.5 * u] }));
    run(&mut s, "dimangular", json!({ "vertex": [l * 0.7, ht], "p1": [0, ht], "p2": [l, ht * 0.6], "at": [l * 0.55, ht * 0.8] }));
    run(&mut s, "dimdiameter", json!({ "handle": circle, "at": [l * 0.55, ht * 0.22] }));
    let doc = s.doc().ok()?;
    let list = cadcraft_engine::render::build(doc, &cadcraft_engine::doc::Space::Model, &cadcraft_engine::render::Options::default());
    let view = cadcraft_engine::render::raster::View::fit(&list.bounds, w, h, 0.06);
    let background = cadcraft_color::Rgb(bg.r(), bg.g(), bg.b());
    let png = cadcraft_engine::render::raster::render_png(
        &list,
        &view,
        &cadcraft_engine::render::raster::RasterOptions { background, ..Default::default() },
    )?;
    let img = image::load_from_memory(&png).ok()?.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
}
