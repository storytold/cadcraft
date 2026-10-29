//! The Hatch and Gradient dialog (HATCH, GRADIENT) and, on existing hatches, the Hatch Edit
//! dialog (HATCHEDIT, double-click): one window in two modes. It never edits the drawing itself:
//! picks go through HATCHPICK, and OK runs one HATCH, GRADIENT or HATCHEDIT call.

use cadcraft_color::Color;
use cadcraft_doc::{Drawing, EntityKind, Handle, Space};
use cadcraft_engine::{HatchPicks, PickMode};
use serde_json::{Value, json};

use egui::vec2;

use crate::CadApp;
use crate::theme::Tokens;

/// The gradient shapes the dialog offers; each can also be inverted (`INV` + name).
pub(crate) const SHAPES: [&str; 5] = ["LINEAR", "CYLINDER", "CURVED", "SPHERICAL", "HEMISPHERICAL"];

/// HPORIGINMODE 1–5, by the names HATCH's `originMode` takes.
pub(crate) const ORIGIN_MODES: [&str; 5] = ["bottomLeft", "bottomRight", "topRight", "topLeft", "center"];

/// The fill and options the dialog shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Fill {
    pub gradient: bool,
    pub pattern: String,
    pub scale: f64,
    /// Pattern or gradient angle, degrees.
    pub angle: f64,
    /// One of [`SHAPES`].
    pub shape: String,
    pub inverted: bool,
    pub color1: Color,
    pub color2: Color,
    pub centered: bool,
    /// "Specified origin" rather than "Use current origin".
    pub specified: bool,
    /// A clicked origin; when `None` a specified origin is `origin_mode` of the extents.
    pub origin: Option<[f64; 2]>,
    /// One of [`ORIGIN_MODES`].
    pub origin_mode: String,
    pub associative: bool,
    /// Island detection: 0 normal, 1 outer, 2 ignore.
    pub style: u8,
}

/// The dialog while open (and while a pick it started runs).
#[derive(Clone, Debug, PartialEq)]
pub struct HatchDlgState {
    pub fill: Fill,
    /// The fill as the dialog opened: edit mode sends only what changed.
    pub opened: Fill,
    /// The hatches being edited; empty in create mode.
    pub edit: Vec<Handle>,
    pub separate: bool,
    /// Internal points and boundary objects picked so far (create mode).
    pub points: Vec<[f64; 2]>,
    pub handles: Vec<Handle>,
    /// The loops of the first edited hatch; geometry edits require exactly one hatch.
    pub loop_outer: Vec<bool>,
    /// Zero-based loop numbers to remove on OK.
    pub remove_loops: Vec<usize>,
    /// A HATCHPICK the dialog started that hasn't ended yet.
    pub picking: Option<PickMode>,
    /// The last command error, shown in the dialog.
    pub error: Option<String>,
    /// The drawing the dialog belongs to (`DocState::uid`).
    pub uid: u64,
    /// The space its picks were made in (`DocState::edit_space`, where HATCH adds the hatch): a
    /// switch to another layout drops the dialog.
    pub space: Space,
}

/// Create mode, from the drawing's HATCH settings (HPNAME, HPSCALE, HPANG, HPASSOC, HPSEPARATE,
/// HPORIGINMODE, HPISLANDDETECTION); values a file got wrong fall back to the defaults.
pub fn create_state(d: &Drawing, uid: u64, gradient: bool) -> HatchDlgState {
    let h = &d.header;
    let scale = Some(h.f64("HPSCALE", 1.0)).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(1.0);
    let angle = Some(h.f64("HPANG", 0.0).to_degrees()).filter(|v| v.is_finite()).unwrap_or(0.0);
    let mode = h.i64("HPORIGINMODE", 0);
    let origin_mode = mode.checked_sub(1).and_then(|i| usize::try_from(i).ok()).and_then(|i| ORIGIN_MODES.get(i)).copied().unwrap_or("bottomLeft");
    let fill = Fill {
        gradient,
        pattern: h.str("HPNAME", "ANSI31"),
        scale,
        angle,
        shape: "LINEAR".into(),
        inverted: false,
        color1: Color::Index(5),
        color2: Color::Index(7),
        centered: true,
        // As HATCH reads HPORIGINMODE: 1–5 a corner or the centre, anything else the current origin.
        specified: (1..=5).contains(&mode),
        origin: None,
        origin_mode: origin_mode.into(),
        associative: h.i64("HPASSOC", 1) != 0,
        style: u8::try_from(h.i64("HPISLANDDETECTION", 0).clamp(0, 2)).unwrap_or(0),
    };
    HatchDlgState {
        opened: fill.clone(),
        fill,
        edit: Vec::new(),
        separate: h.i64("HPSEPARATE", 0) != 0,
        points: Vec::new(),
        handles: Vec::new(),
        loop_outer: Vec::new(),
        remove_loops: Vec::new(),
        picking: None,
        error: None,
        uid,
        space: Space::Model,
    }
}

/// Edit mode on the hatches among `handles`, showing the first one; `None` when there are none.
pub fn edit_state(d: &Drawing, uid: u64, handles: Vec<Handle>) -> Option<HatchDlgState> {
    let hatches: Vec<Handle> = handles.into_iter().filter(|h| matches!(d.entity(*h).map(|e| &e.kind), Some(EntityKind::Hatch(_)))).collect();
    let first = hatches.first().and_then(|h| match &d.entity(*h)?.kind {
        EntityKind::Hatch(x) => Some(x.clone()),
        _ => None,
    })?;
    let mut st = create_state(d, uid, first.gradient.is_some());
    let f = &mut st.fill;
    // A gradient's own pattern is SOLID; its Pattern tab offers the current pattern (HPNAME).
    if first.gradient.is_none() {
        f.pattern = first.pattern.clone();
    }
    f.scale = if first.scale.is_finite() && first.scale > 0.0 { first.scale } else { 1.0 };
    f.angle = Some(first.angle.to_degrees()).filter(|a| a.is_finite()).unwrap_or(0.0);
    f.associative = first.associative;
    f.style = first.style.min(2);
    f.specified = false;
    f.origin = None;
    if let Some(g) = &first.gradient {
        let up = g.name.trim().to_ascii_uppercase();
        let (inverted, base) = match up.strip_prefix("INV") {
            Some(b) => (true, b.to_string()),
            None => (false, up.clone()),
        };
        f.shape = if SHAPES.contains(&base.as_str()) { base } else { "LINEAR".into() };
        f.inverted = inverted;
        f.color1 = g.color1;
        f.color2 = g.color2;
        f.angle = Some(g.angle.to_degrees()).filter(|a| a.is_finite()).unwrap_or(0.0);
        f.centered = g.centered;
    }
    st.opened = st.fill.clone();
    st.edit = hatches;
    if st.edit.len() == 1 {
        st.loop_outer = first.loops.iter().map(|l| l.outer).collect();
    }
    st.separate = false;
    Some(st)
}

fn style_name(s: u8) -> &'static str {
    match s {
        1 => "outer",
        2 => "ignore",
        _ => "normal",
    }
}

fn gradient_name(f: &Fill) -> String {
    format!("{}{}", if f.inverted { "INV" } else { "" }, f.shape)
}

fn hexes(hs: &[Handle]) -> Value {
    json!(hs.iter().map(|h| h.hex()).collect::<Vec<_>>())
}

/// The command OK runs: HATCH or GRADIENT with the picks (create mode), or HATCHEDIT with what
/// changed (edit mode). `None` when there is nothing to run.
pub fn ok_call(st: &HatchDlgState) -> Option<(&'static str, Value)> {
    let f = &st.fill;
    let mut p = serde_json::Map::new();
    if st.edit.is_empty() {
        if st.points.is_empty() && st.handles.is_empty() {
            return None;
        }
        if !st.points.is_empty() {
            p.insert("points".into(), json!(st.points));
        }
        if !st.handles.is_empty() {
            p.insert("handles".into(), hexes(&st.handles));
        }
        if f.gradient {
            p.insert("name".into(), json!(gradient_name(f)));
            p.insert("color1".into(), json!(f.color1.name()));
            p.insert("color2".into(), json!(f.color2.name()));
            p.insert("centered".into(), json!(f.centered));
        } else {
            p.insert("pattern".into(), json!(f.pattern));
            p.insert("scale".into(), json!(f.scale));
        }
        p.insert("angle".into(), json!(f.angle));
        p.insert("associative".into(), json!(f.associative));
        p.insert("separate".into(), json!(st.separate));
        p.insert("style".into(), json!(style_name(f.style)));
        match (f.specified, f.origin) {
            (true, Some(o)) => p.insert("origin".into(), json!(o)),
            (true, None) => p.insert("originMode".into(), json!(f.origin_mode)),
            (false, _) => p.insert("originMode".into(), json!("current")),
        };
        return Some((if f.gradient { "gradient" } else { "hatch" }, Value::Object(p)));
    }
    let o = &st.opened;
    if !st.points.is_empty() {
        p.insert("addPoints".into(), json!(st.points));
    }
    if !st.handles.is_empty() {
        p.insert("addBoundary".into(), hexes(&st.handles));
    }
    if !st.remove_loops.is_empty() {
        p.insert("removeLoops".into(), json!(st.remove_loops));
    }
    if f.gradient {
        let changed = !o.gradient
            || f.shape != o.shape
            || f.inverted != o.inverted
            || f.color1 != o.color1
            || f.color2 != o.color2
            || f.angle != o.angle
            || f.centered != o.centered;
        if changed {
            p.insert(
                "gradient".into(),
                json!({ "name": gradient_name(f), "color1": f.color1.name(), "color2": f.color2.name(), "angle": f.angle, "centered": f.centered }),
            );
        }
    } else {
        if o.gradient || f.pattern != o.pattern {
            p.insert("pattern".into(), json!(f.pattern));
        }
        if f.scale != o.scale {
            p.insert("scale".into(), json!(f.scale));
        }
        if f.angle != o.angle {
            p.insert("angle".into(), json!(f.angle));
        }
    }
    if f.associative != o.associative {
        p.insert("associative".into(), json!(f.associative));
    }
    if f.style != o.style {
        p.insert("style".into(), json!(style_name(f.style)));
    }
    if f.specified {
        match f.origin {
            Some(q) => p.insert("origin".into(), json!(q)),
            None => p.insert("originMode".into(), json!(f.origin_mode)),
        };
    }
    if p.is_empty() {
        return None;
    }
    p.insert("handles".into(), hexes(&st.edit));
    Some(("hatchedit", Value::Object(p)))
}

/// Take a HATCHPICK's picks into the dialog (Hatch picks are taken by `resume`).
pub fn merge_picks(st: &mut HatchDlgState, picks: HatchPicks) {
    match picks.mode {
        PickMode::Points | PickMode::Objects => {
            st.points.extend(picks.points.iter().map(|q| [q.x, q.y]));
            for h in picks.handles {
                if !st.handles.contains(&h) {
                    st.handles.push(h);
                }
            }
        }
        PickMode::Origin => {
            if let Some(q) = picks.origin {
                st.fill.origin = Some([q.x, q.y]);
                st.fill.specified = true;
            }
        }
        PickMode::Hatch => {}
    }
    st.error = None;
}

/// Whether typed or menu HATCH and GRADIENT open the dialog (HPDLGMODE, default 1).
pub fn dialog_on(app: &CadApp) -> bool {
    app.session.doc().is_ok_and(|d| d.header.i64("HPDLGMODE", 1) != 0)
}

/// Create mode (HATCH, GRADIENT): the dialog from the drawing's hatch settings.
pub fn open_create(app: &mut CadApp, gradient: bool) {
    let Ok(st) = app.session.state() else { return };
    let mut dlg = create_state(&st.doc, st.uid, gradient);
    dlg.space = st.edit_space();
    app.hatch_dlg = Some(dlg);
    app.ui.dialog = Some("hatch".into());
}

/// Edit mode (HATCHEDIT, right-click, double-click) on the selected hatches; with none selected,
/// pick a hatch first.
pub fn open_edit(app: &mut CadApp) {
    let Ok(st) = app.session.state() else { return };
    let (mut edit, mut create) = (edit_state(&st.doc, st.uid, st.selection.clone()), create_state(&st.doc, st.uid, false));
    create.space = st.edit_space();
    if let Some(e) = edit.as_mut() {
        e.space = st.edit_space();
    }
    match edit {
        Some(e) => {
            app.hatch_dlg = Some(e);
            app.ui.dialog = Some("hatch".into());
        }
        None => {
            app.hatch_dlg = Some(create);
            start_pick(app, PickMode::Hatch);
        }
    }
}

/// Hide the dialog and run HATCHPICK in `mode`; [`resume`] brings the dialog back.
pub fn start_pick(app: &mut CadApp, mode: PickMode) {
    let Some(st) = app.hatch_dlg.as_mut() else { return };
    st.picking = Some(mode);
    st.error = None;
    app.session.hatch_picks = None;
    app.ui.dialog = None;
    app.start("hatchpick");
    let kw = match mode {
        PickMode::Points => "Points",
        PickMode::Objects => "Objects",
        PickMode::Origin => "oRigin",
        PickMode::Hatch => "Hatch",
    };
    if app.session.running.is_some()
        && let Err(e) = app.session.input(cadcraft_engine::Input::Keyword(kw.into()))
    {
        app.session.echo(e.to_string());
    }
}

/// Every frame: once a HATCHPICK the dialog started has ended, take its picks and bring the
/// dialog back (unchanged when the pick was cancelled). A different drawing by then drops the
/// dialog and its picks; a Hatch pick that found none closes it.
pub fn resume(app: &mut CadApp) {
    let Some(mode) = app.hatch_dlg.as_ref().and_then(|s| s.picking) else { return };
    if app.session.running.is_some() {
        return;
    }
    let picks = app.session.hatch_picks.take();
    let here = app.session.state().ok().map(|s| (s.uid, s.edit_space()));
    let Some(mut st) = app.hatch_dlg.take() else { return };
    if here != Some((st.uid, st.space.clone())) {
        return;
    }
    st.picking = None;
    if mode == PickMode::Hatch {
        let next = picks.and_then(|p| app.session.doc().ok().and_then(|d| edit_state(d, st.uid, p.handles)));
        let Some(mut next) = next else { return };
        next.space = st.space.clone();
        st = next;
    } else if let Some(p) = picks {
        merge_picks(&mut st, p);
    }
    app.hatch_dlg = Some(st);
    app.ui.dialog = Some("hatch".into());
}

/// OK: run the dialog's command. True when the dialog closes; a failed command keeps it open
/// with the error (the drawing is unchanged).
pub fn ok(app: &mut CadApp) -> bool {
    let Some(call) = app.hatch_dlg.as_ref().map(ok_call) else { return true };
    let Some((cmd, p)) = call else {
        app.hatch_dlg = None;
        return true;
    };
    match app.run(cmd, p) {
        Ok(_) => {
            app.hatch_dlg = None;
            true
        }
        Err(e) => {
            if let Some(st) = app.hatch_dlg.as_mut() {
                st.error = Some(e);
            }
            false
        }
    }
}

enum Action {
    None,
    Ok,
    Cancel,
    Pick(PickMode),
}

fn shape_label(name: &str) -> &'static str {
    match name {
        "CYLINDER" => crate::tl!("Cylinder"),
        "CURVED" => crate::tl!("Curved"),
        "SPHERICAL" => crate::tl!("Spherical"),
        "HEMISPHERICAL" => crate::tl!("Hemispherical"),
        _ => crate::tl!("Linear"),
    }
}

fn origin_label(mode: &str) -> &'static str {
    match mode {
        "bottomRight" => crate::tl!("Bottom right"),
        "topRight" => crate::tl!("Top right"),
        "topLeft" => crate::tl!("Top left"),
        "center" => crate::tl!("Center"),
        _ => crate::tl!("Bottom left"),
    }
}

fn num(v: f64) -> String {
    let s = format!("{v:.4}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

/// The Hatch and Gradient / Hatch Edit window (`app.ui.dialog == "hatch"`).
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let here = app.session.state().ok().map(|s| (s.uid, s.edit_space()));
    // Opened by name (control channel) with no state: create mode from the header.
    if app.hatch_dlg.is_none() {
        open_create(app, false);
    }
    // No drawing, or another drawing or layout than the one the dialog was opened on: close.
    let Some(mut st) = app.hatch_dlg.take().filter(|s| here.as_ref() == Some(&(s.uid, s.space.clone()))) else {
        *open = false;
        return;
    };
    let title = if st.edit.is_empty() { crate::tl!("Hatch and Gradient") } else { crate::tl!("Hatch Edit") };
    let mut action = Action::None;
    egui::Window::new(title).id(egui::Id::unique("hatch_dialog")).open(open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(340.0);
                fill_ui(ui, &mut st.fill, &mut action);
            });
            // Space, not a vertical separator: a separator takes the height available, and the
            // window grew by it every frame until the buttons left the screen.
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(230.0);
                options_ui(ui, &mut st, &mut action);
            });
        });
        if let Some(e) = &st.error {
            ui.colored_label(Tokens::get().warn, e);
        }
        ui.separator();
        ui.horizontal(|ui| {
            let ready = !st.edit.is_empty() || !(st.points.is_empty() && st.handles.is_empty());
            if ui.add_enabled(ready, egui::Button::new(crate::tl!("OK"))).clicked() {
                action = Action::Ok;
            }
            if ui.button(crate::tl!("Cancel")).clicked() {
                action = Action::Cancel;
            }
        });
    });
    app.hatch_dlg = Some(st);
    match action {
        Action::Ok => {
            if ok(app) {
                *open = false;
            }
        }
        Action::Cancel => {
            app.hatch_dlg = None;
            *open = false;
        }
        Action::Pick(mode) => start_pick(app, mode),
        Action::None => {
            if !*open {
                app.hatch_dlg = None;
            }
        }
    }
}

/// Pattern/gradient fill selector and its controls; an origin pick is reported through `action`.
fn fill_ui(ui: &mut egui::Ui, f: &mut Fill, action: &mut Action) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut f.gradient, false, crate::tl!("Pattern"));
        ui.selectable_value(&mut f.gradient, true, crate::tl!("Gradient"));
    });
    ui.separator();
    if f.gradient {
        egui::Grid::new("hatch_gradient").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
            ui.label(crate::tl!("Color 1"));
            if let Some(c) = crate::layers::color_button(ui, egui::Id::unique("hatch_color1"), f.color1, false, 160.0) {
                f.color1 = c;
            }
            ui.end_row();
            ui.label(crate::tl!("Color 2"));
            if let Some(c) = crate::layers::color_button(ui, egui::Id::unique("hatch_color2"), f.color2, false, 160.0) {
                f.color2 = c;
            }
            ui.end_row();
            ui.label(crate::tl!("Shape"));
            craft_ui::ComboBox::from_id_salt("hatch_shape").selected_text(shape_label(&f.shape)).show_ui(ui, |ui| {
                for name in SHAPES {
                    ui.selectable_value(&mut f.shape, name.to_string(), shape_label(name));
                }
            });
            ui.end_row();
            ui.label("");
            ui.checkbox(&mut f.inverted, crate::tl!("Inverted"));
            ui.end_row();
            ui.label("");
            ui.checkbox(&mut f.centered, crate::tl!("Centered"));
            ui.end_row();
            ui.label(crate::tl!("Angle"));
            ui.add(egui::DragValue::new(&mut f.angle).speed(1.0).range(-360.0..=360.0).clamp_existing_to_range(false).suffix("°"));
            ui.end_row();
        });
    } else {
        egui::ScrollArea::vertical().id_salt("hatch_swatches").max_height(220.0).show(ui, |ui| {
            if let Some(name) = crate::palettes::patterns::grid(ui, &f.pattern, "") {
                f.pattern = name;
            }
        });
        egui::Grid::new("hatch_pattern").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
            ui.label(crate::tl!("Angle"));
            ui.add(egui::DragValue::new(&mut f.angle).speed(1.0).range(-360.0..=360.0).clamp_existing_to_range(false).suffix("°"));
            ui.end_row();
            ui.label(crate::tl!("Scale"));
            ui.add(egui::DragValue::new(&mut f.scale).speed(0.05).range(0.0001..=1.0e6).clamp_existing_to_range(false));
            ui.end_row();
        });
    }
    ui.separator();
    ui.strong(crate::tl!("Hatch origin"));
    ui.radio_value(&mut f.specified, false, crate::tl!("Use current origin"));
    ui.radio_value(&mut f.specified, true, crate::tl!("Specified origin"));
    ui.add_enabled_ui(f.specified, |ui| {
        if ui.button(crate::tl!("Click to set new origin")).clicked() {
            *action = Action::Pick(PickMode::Origin);
        }
        if let Some([x, y]) = f.origin {
            ui.label(crate::tf!("Origin: {x}, {y}", x = num(x), y = num(y)));
        }
        let mut extents = f.origin.is_none();
        if ui.add_enabled(f.origin.is_some(), egui::Checkbox::new(&mut extents, crate::tl!("Default to boundary extents"))).changed() && extents {
            f.origin = None;
        }
        ui.add_enabled_ui(f.origin.is_none(), |ui| {
            craft_ui::ComboBox::from_id_salt("hatch_origin_mode").selected_text(origin_label(&f.origin_mode)).show_ui(ui, |ui| {
                for m in ORIGIN_MODES {
                    ui.selectable_value(&mut f.origin_mode, m.to_string(), origin_label(m));
                }
            });
        });
    });
}

fn options_ui(ui: &mut egui::Ui, st: &mut HatchDlgState, action: &mut Action) {
    if st.edit.is_empty() || st.edit.len() == 1 {
        ui.strong(crate::tl!("Boundaries"));
        if ui.button(crate::tl!("Add: Pick points")).clicked() {
            *action = Action::Pick(PickMode::Points);
        }
        if ui.button(crate::tl!("Add: Select objects")).clicked() {
            *action = Action::Pick(PickMode::Objects);
        }
        ui.horizontal(|ui| {
            ui.label(crate::tf!("Boundaries picked: {n}", n = st.points.len() + st.handles.len()));
            let any = !(st.points.is_empty() && st.handles.is_empty());
            if ui.add_enabled(any, egui::Button::new(crate::tl!("Clear"))).clicked() {
                st.points.clear();
                st.handles.clear();
            }
        });
        if !st.edit.is_empty() {
            egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                for (i, outer) in st.loop_outer.iter().enumerate() {
                    let mut remove = st.remove_loops.contains(&i);
                    let label = format!("{} {} ({})", crate::tl!("Boundary"), i + 1, if *outer { crate::tl!("Outer") } else { crate::tl!("Island") });
                    if ui.checkbox(&mut remove, label).changed() {
                        if remove {
                            st.remove_loops.push(i);
                        } else {
                            st.remove_loops.retain(|n| *n != i);
                        }
                    }
                }
            });
            ui.label(crate::tl!("Select boundaries to remove"));
        }
        ui.separator();
    }
    ui.strong(crate::tl!("Options"));
    ui.checkbox(&mut st.fill.associative, crate::tl!("Associative"));
    if st.edit.is_empty() {
        ui.checkbox(&mut st.separate, crate::tl!("Create separate hatches"));
    }
    ui.label(crate::tl!("Island detection"));
    ui.radio_value(&mut st.fill.style, 0, crate::tl!("Normal"));
    ui.radio_value(&mut st.fill.style, 1, crate::tl!("Outer"));
    ui.radio_value(&mut st.fill.style, 2, crate::tl!("Ignore"));
}

#[cfg(test)]
mod tests;
