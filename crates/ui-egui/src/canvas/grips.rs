use super::{CadApp, Handle, Input, Pos2, Rect, Vec2, Xf, vec2};

/// A click on grip `g` (idle): Shift+click makes it hot or cold again; a plain click starts
/// editing it, with the other hot grips when it is one of them.
pub(super) fn click_grip(app: &mut CadApp, g: (Handle, usize), shift: bool) {
    let hot = &mut app.canvas.hot_grips;
    if shift {
        match hot.iter().position(|x| *x == g) {
            Some(i) => {
                hot.remove(i);
            }
            None => hot.push(g),
        }
        return;
    }
    if !hot.contains(&g) {
        hot.clear();
    }
    hot.retain(|x| *x != g);
    hot.insert(0, g);
    let hs = hot.clone();
    if let Err(e) = app.session.start_grip_edit(&hs) {
        app.canvas.hot_grips.clear();
        app.session.echo(e.to_string());
    }
}

/// The multifunctional options of grip `g` (Add Vertex, Lengthen…; empty: Stretch only).
pub(super) fn grip_options(app: &CadApp, (h, i): (Handle, usize)) -> Vec<cadcraft_engine::grips::GripAction> {
    let Ok(st) = app.session.state() else { return Vec::new() };
    st.doc.entity(h).map(|e| cadcraft_engine::grips::grip_actions(&e.kind, i)).unwrap_or_default()
}

/// The grip menu's first item, plain Stretch, named for what the grip is: a polyline vertex
/// (Stretch Vertex) or a spline fit point (Move Fit Point).
fn stretch_label(options: &[cadcraft_engine::grips::GripAction]) -> &'static str {
    use cadcraft_engine::grips::GripAction;
    if options.contains(&GripAction::RemoveVertex) {
        "Stretch Vertex"
    } else if options.contains(&GripAction::AddFitPoint) || options.contains(&GripAction::RemoveFitPoint) {
        "Move Fit Point"
    } else {
        "Stretch"
    }
}

/// The menu of a multifunctional grip: Stretch and its options. A choice makes the grip hot and
/// runs the option at the grip prompt, as typing its keyword would.
pub(super) fn grip_menu(app: &mut CadApp, resp: &egui::Response) {
    let Some(g) = app.canvas.grip_menu else { return };
    let options = grip_options(app, g);
    let mut chosen: Option<Option<cadcraft_engine::grips::GripAction>> = None;
    let open = resp
        .context_menu(|ui| {
            if ui.button(crate::i18n::t(stretch_label(&options))).clicked() {
                chosen = Some(None);
                ui.close();
            }
            for a in &options {
                if ui.button(crate::i18n::t(a.label())).clicked() {
                    chosen = Some(Some(*a));
                    ui.close();
                }
            }
        })
        .is_some();
    if !open || chosen.is_some() {
        app.canvas.grip_menu = None;
    }
    let Some(choice) = chosen else { return };
    apply_grip_choice(app, g, choice);
}

/// Seconds the cursor rests on a grip before its menu opens.
const GRIP_HOVER_DELAY: f64 = 0.4;
/// The same with the crosshair locked onto grips (GRIPSNAP).
const GRIP_HOVER_DELAY_SNAPPED: f64 = 0.2;

/// The grip menu that opens when the cursor rests on a grip with options (a dimension's text
/// grip, a polyline vertex…); it closes when the cursor leaves the grip and the menu.
pub(super) fn hover_grip_menu(app: &mut CadApp, ui: &egui::Ui, xf: &Xf, hover: Option<Pos2>, inside: bool) {
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let busy = ctx.input(|i| i.pointer.any_down()) || ctx.dragged_id().is_some();
    let over = if inside && !busy { grip_at(app, xf, hover).filter(|g| !grip_options(app, *g).is_empty()) } else { None };
    if let Some((g, at)) = app.canvas.grip_popup {
        let options = grip_options(app, g);
        if options.is_empty() {
            app.canvas.grip_popup = None;
            return;
        }
        let mut chosen: Option<Option<cadcraft_engine::grips::GripAction>> = None;
        let area = egui::Area::new(egui::Id::unique("cc_grip_hover_menu")).order(egui::Order::Foreground).fixed_pos(at).show(&ctx, |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_min_width(170.0);
                // Menu items: the whole row lights up, no button frames.
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    ui.spacing_mut().button_padding = vec2(8.0, 4.0);
                    let item = |ui: &mut egui::Ui, label: &str| ui.add(egui::Button::new(crate::i18n::t(label)).frame_when_inactive(false)).clicked();
                    if item(ui, stretch_label(&options)) {
                        chosen = Some(None);
                    }
                    for a in &options {
                        if item(ui, a.label()) {
                            chosen = Some(Some(*a));
                        }
                    }
                });
            });
        });
        let on_menu = ctx.pointer_latest_pos().is_some_and(|p| area.response.rect.expand(8.0).contains(p));
        if let Some(choice) = chosen {
            app.canvas.grip_popup = None;
            app.canvas.grip_hover = None;
            apply_grip_choice(app, g, choice);
        } else if !on_menu && over != Some(g) {
            app.canvas.grip_popup = None;
            app.canvas.grip_hover = over.map(|g| (g, now));
        }
        return;
    }
    match (over, app.canvas.grip_hover) {
        (Some(g), Some((h, since))) if h == g => {
            // A crosshair locked onto the grip (GRIPSNAP) holds still: the menu can come sooner.
            let delay = if app.session.settings.gripsnap { GRIP_HOVER_DELAY_SNAPPED } else { GRIP_HOVER_DELAY };
            if now - since >= delay {
                let point = app.session.grips_of(g.0).ok().and_then(|p| p.get(g.1).copied());
                if let Some(p) = point {
                    app.canvas.grip_popup = Some((g, xf.to_screen(p) + vec2(10.0, 10.0)));
                }
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(delay - (now - since) + 0.01));
            }
        }
        (Some(g), _) => {
            app.canvas.grip_hover = Some((g, now));
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(GRIP_HOVER_DELAY + 0.01));
        }
        (None, _) => app.canvas.grip_hover = None,
    }
}

/// Run a grip menu choice: Stretch (`None`) or an option, on grip `g` made hot.
fn apply_grip_choice(app: &mut CadApp, g: (Handle, usize), choice: Option<cadcraft_engine::grips::GripAction>) {
    app.canvas.hot_grips = vec![g];
    let r = app.session.start_grip_edit(&[g]).and_then(|()| match choice {
        Some(a) => app.session.input(Input::Keyword(a.keyword().to_string())),
        None => Ok(()),
    });
    if let Err(e) = r {
        app.canvas.hot_grips.clear();
        app.session.echo(e.to_string());
    }
}

/// GRIPSNAP: the grip the crosshair locks onto (no command running, a grip of the selection
/// under the cursor): its world point and where it is on screen.
pub(super) fn grip_snap(app: &CadApp, xf: &Xf, hover: Option<Pos2>) -> Option<(Vec2, Pos2)> {
    if !app.session.settings.gripsnap || app.session.prompting() {
        return None;
    }
    let g = grip_at(app, xf, hover)?;
    let p = app.session.grips_of(g.0).ok()?.get(g.1).copied()?;
    Some((p, xf.to_screen(p)))
}

/// Polyline segment midpoint grips are rectangular; vertex grips remain square.
pub(super) fn grip_rect(kind: &cadcraft_doc::EntityKind, count: usize, index: usize, center: Pos2, size: f32) -> Rect {
    // The grip list interleaves vertices and available segment midpoints, then any remaining vertices.
    let midpoint = matches!(kind, cadcraft_doc::EntityKind::LwPolyline(p) if index % 2 == 1 && index / 2 < count.saturating_sub(p.vertices.len()));
    Rect::from_center_size(center, vec2(size * 2.0, if midpoint { size } else { size * 2.0 }))
}

/// The grip of a selected object under the cursor, if any: (object, grip index).
pub(super) fn grip_at(app: &CadApp, xf: &Xf, hover: Option<Pos2>) -> Option<(Handle, usize)> {
    let hp = hover?;
    let st = app.session.state().ok()?;
    let gs = app.session.settings.gripsize as f32;
    let sel = app.session.selection();
    if !app.session.shows_grips(sel.len()) {
        return None;
    }
    for h in sel.iter().take(200) {
        let e = st.doc.entity(*h)?;
        if st.doc.layer(&e.common.layer).is_some_and(|l| l.locked) {
            continue;
        }
        let grips = cadcraft_engine::grips::grip_points(&st.doc, &e.kind);
        let count = grips.len();
        for (i, g) in grips.into_iter().enumerate() {
            let sp = xf.to_screen(g);
            if grip_rect(&e.kind, count, i, sp, gs).expand(3.0).contains(hp) {
                return Some((*h, i));
            }
        }
    }
    None
}
