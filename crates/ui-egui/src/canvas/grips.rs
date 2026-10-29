use super::{CadApp, Handle, Input, Pos2, Rect, Xf, vec2};

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

/// The menu of a multifunctional grip: Stretch and its options. A choice makes the grip hot and
/// runs the option at the grip prompt, as typing its keyword would.
pub(super) fn grip_menu(app: &mut CadApp, resp: &egui::Response) {
    let Some(g) = app.canvas.grip_menu else { return };
    let options = grip_options(app, g);
    let mut chosen: Option<Option<cadcraft_engine::grips::GripAction>> = None;
    let open = resp
        .context_menu(|ui| {
            let vertex = options.contains(&cadcraft_engine::grips::GripAction::RemoveVertex);
            if ui.button(crate::i18n::t(if vertex { "Stretch Vertex" } else { "Stretch" })).clicked() {
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
        let grips = e.kind.grips();
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
