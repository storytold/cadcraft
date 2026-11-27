use egui::{Color32, Stroke};

use super::{CadApp, Handle, Input, Pos2, Rect, Xf, vec2};
use crate::theme::Tokens;

/// Grips on selected objects (when idle or editing grips), including GROUPDISPLAYMODE boxes.
pub(super) fn draw_selection_grips(app: &mut CadApp, painter: &egui::Painter, xf: &Xf, rect: Rect, hover_pos: Option<Pos2>, t: Tokens) {
    let sel = app.session.selection();
    let grip_edit = app.session.running.as_ref().is_some_and(|r| r.id == "grip");
    if (!grip_edit && (app.canvas.grip_editing || app.session.prompting())) || sel.is_empty() {
        app.canvas.hot_grips.clear();
    }
    if !grip_edit {
        app.canvas.dyn_hot = None;
    }
    app.canvas.grip_editing = grip_edit;
    if !(!app.session.prompting() || grip_edit) || !app.session.shows_grips(sel.len()) {
        return;
    }
    let Ok(st) = app.session.state() else { return };
    let gs = app.session.settings.gripsize as f32;
    let gdm = app.session.settings.groupdisplaymode;
    let mut grouped_members = std::collections::HashSet::new();
    if gdm > 0 && app.session.settings.pickstyle & 1 != 0 {
        for g in &st.doc.groups {
            if g.selectable && !g.members.is_empty() && g.members.iter().all(|h| sel.contains(h)) {
                let mut b = cadcraft_geom::Bounds2::EMPTY;
                for h in &g.members {
                    if let Some(e) = st.doc.entity(*h) {
                        grouped_members.insert(*h);
                        let eb = cadcraft_engine::doc::entity_bounds(&st.doc, e, 0);
                        b = b.union(&eb);
                    }
                }
                if !b.is_empty() {
                    let min_sp = xf.to_screen(b.min);
                    let max_sp = xf.to_screen(b.max);
                    let box_rect = Rect::from_two_pos(min_sp, max_sp).expand(4.0);
                    if rect.intersects(box_rect) {
                        painter.rect_stroke(
                            box_rect,
                            0.0,
                            Stroke::new(1.0, Color32::from_rgba_unmultiplied(0x80, 0x90, 0xb0, 0xb0)),
                            egui::StrokeKind::Middle,
                        );
                        let center_sp = xf.to_screen(b.center());
                        if rect.contains(center_sp) {
                            let cr = Rect::from_center_size(center_sp, egui::vec2(gs * 2.0, gs * 2.0));
                            let warm = hover_pos.is_some_and(|hp| cr.expand(2.0).contains(hp));
                            painter.rect_filled(cr, 0.0, if warm { Color32::from_rgb(0xff, 0x7f, 0x9f) } else { t.grip });
                            painter.rect_stroke(cr, 0.0, Stroke::new(1.0, Color32::from_rgb(0x10, 0x30, 0x80)), egui::StrokeKind::Middle);
                        }
                    }
                }
            }
        }
    }
    for h in sel.iter().take(200) {
        if gdm == 1 && grouped_members.contains(h) {
            continue;
        }
        if let Some(e) = st.doc.entity(*h)
            && st.shows(e)
        {
            let grips = e.kind.grips();
            let count = grips.len();
            for (gi, g) in grips.into_iter().enumerate() {
                let sp = xf.to_screen(g);
                if rect.contains(sp) {
                    let r = grip_rect(&e.kind, count, gi, sp, gs);
                    let hot = app.canvas.hot_grips.contains(&(*h, gi));
                    let warm = !hot && hover_pos.is_some_and(|hp| r.expand(2.0).contains(hp));
                    painter.rect_filled(
                        r,
                        0.0,
                        if hot {
                            t.grip_hot
                        } else if warm {
                            Color32::from_rgb(0xff, 0x7f, 0x9f)
                        } else {
                            t.grip
                        },
                    );
                    painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(0x10, 0x30, 0x80)), egui::StrokeKind::Middle);
                }
            }
        }
    }
    crate::dyngrips::draw(app, painter, xf, rect, hover_pos);
}

/// A click on grip `g` (idle): Shift+click makes it hot or cold again; a plain click starts
/// editing it, with the other hot grips when it is one of them.
pub(super) fn click_grip(app: &mut CadApp, g: (Handle, usize), shift: bool) {
    // A hot grip whose object was hidden since (a layer turned off, HIDEOBJECTS, a layer frozen
    // in the active viewport) is no longer painted, so it must not be stretched along with the
    // visible ones.
    let Ok(st) = app.session.state() else { return };
    app.canvas.hot_grips.retain(|(h, _)| st.doc.entity(*h).is_some_and(|e| st.shows(e)));
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
    let open = crate::context_menu::popup(resp)
        .show(|ui| {
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
        if !st.shows(e) || st.doc.layer(&e.common.layer).is_some_and(|l| l.locked) {
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
