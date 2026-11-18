//! Direct manipulation of a page's corner handles. Only the frame changes during
//! dragging; the drawing is edited once, on release, through an undoable command.
use super::{CadApp, Color32, Handle, Stroke, Vec2, Xf};
use egui::{CursorIcon, Key, PointerButton, Pos2, Rect, vec2};

pub(super) struct Drag {
    handle: Handle,
    corner: usize,
    frame: [Vec2; 5],
    document: u64,
    revision: u64,
    to: Vec2,
}

fn frames(app: &CadApp) -> Vec<(Handle, [Vec2; 5])> {
    let Ok(state) = app.session.state() else { return Vec::new() };
    let selection = app.session.selection();
    if app.session.running.is_some() || !app.session.shows_grips(selection.len()) {
        return Vec::new();
    }
    selection
        .into_iter()
        .take(200)
        .filter_map(|handle| {
            let entity = state.doc.entity(handle)?;
            if !state.shows(entity)
                || state.doc.space_of(handle).as_ref() != Some(&app.session.space())
                || state.doc.layer(&entity.common.layer).is_some_and(|layer| layer.locked)
            {
                return None;
            }
            Some((handle, state.doc.imported_page_frame(entity)?))
        })
        .collect()
}

fn at(app: &CadApp, xf: &Xf, pointer: Pos2) -> Option<(Handle, usize, [Vec2; 5])> {
    let radius = app.session.settings.gripsize as f32 + 4.0;
    for (handle, frame) in frames(app) {
        for (corner, point) in frame.iter().take(4).enumerate() {
            if Rect::from_center_size(xf.to_screen(*point), vec2(radius * 2.0, radius * 2.0)).contains(pointer) {
                return Some((handle, corner, frame));
            }
        }
    }
    None
}

pub(super) fn input(app: &mut CadApp, ui: &egui::Ui, xf: &Xf, inside: bool, on_menu: bool) -> bool {
    let (pointer, pressed, released, down, escape) = ui.input(|i| {
        (
            i.pointer.interact_pos(),
            i.pointer.button_pressed(PointerButton::Primary),
            i.pointer.button_released(PointerButton::Primary),
            i.pointer.primary_down(),
            i.key_pressed(Key::Escape),
        )
    });
    if let Some(mut drag) = app.canvas.page_resize.take() {
        let valid = app.session.state().is_ok_and(|s| s.uid == drag.document && s.revision == drag.revision) && app.session.running.is_none();
        if escape || !valid {
            return true;
        }
        if let Some(pointer) = pointer {
            drag.to = xf.to_world(pointer);
        }
        let transform = cadcraft_engine::grips::page_resize_transform(&drag.frame, drag.corner, drag.to);
        ui.ctx().set_cursor_icon(if transform.is_some() { CursorIcon::ResizeNwSe } else { CursorIcon::NotAllowed });
        if released {
            if transform.is_some() && (drag.to - drag.frame[drag.corner]).len() * xf.scale > 1.0 {
                let _ = app.run(
                    "page.resize",
                    serde_json::json!({
                        "handle":drag.handle.hex(), "corner":drag.corner, "to":[drag.to.x,drag.to.y]
                    }),
                );
            }
        } else if down {
            app.canvas.page_resize = Some(drag);
        }
        return true;
    }
    if !inside || on_menu || app.session.pending_window.is_some() {
        return false;
    }
    if let Some((handle, corner, frame)) = pointer.and_then(|point| at(app, xf, point)) {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeNwSe);
        if pressed && let Ok(state) = app.session.state() {
            app.canvas.page_resize = Some(Drag { handle, corner, frame, document: state.uid, revision: state.revision, to: frame[corner] });
            app.canvas.drag_window = false;
            return true;
        }
    }
    false
}

pub(super) fn draw(app: &CadApp, painter: &egui::Painter, xf: &Xf) {
    let color = Color32::from_rgb(60, 130, 255);
    let size = app.session.settings.gripsize as f32;
    for (_, frame) in frames(app) {
        for point in frame.iter().take(4) {
            let rectangle = Rect::from_center_size(xf.to_screen(*point), vec2(size * 2.0, size * 2.0));
            painter.rect_filled(rectangle, 0.0, color);
            painter.rect_stroke(rectangle, 0.0, Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Middle);
        }
    }
    if let Some(drag) = &app.canvas.page_resize
        && let Some(transform) = cadcraft_engine::grips::page_resize_transform(&drag.frame, drag.corner, drag.to)
    {
        let points = drag.frame.map(|p| xf.to_screen(transform.apply(p)));
        painter.add(egui::Shape::line(points.to_vec(), Stroke::new(2.0, color)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{
        Block, Common, Entity, EntityKind, Insert, Solid, Space,
        color::{Color, Rgb},
    };
    use cadcraft_engine::{Session, geom::Vec3};
    use egui::{Event, Modifiers, RawInput};
    use std::sync::Arc;

    fn app() -> (CadApp, Handle) {
        let mut session = Session::new();
        let mut block = Block::new("PDF_ANTET");
        let mut entity = Entity::new(
            Handle(1),
            EntityKind::Solid(Solid { corners: [Vec3::ZERO, Vec3::new(20.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(20.0, 10.0, 0.0)] }),
        );
        entity.common.color = Color::True(Rgb(255, 255, 255));
        block.entities.push(entity);
        session.doc_mut().unwrap().blocks.insert(block.name.clone(), Arc::new(block));
        let handle = session
            .doc_mut()
            .unwrap()
            .add(&Space::Model, Common::default(), EntityKind::Insert(Insert { block: "PDF_ANTET".into(), ..Default::default() }))
            .unwrap();
        session.set_selection(vec![handle]);
        (CadApp::new(session, crate::Services::default()), handle)
    }

    fn step(ctx: &egui::Context, app: &mut CadApp, xf: &Xf, events: Vec<Event>) {
        let mut output = ctx.run_ui(RawInput { events, ..Default::default() }, |ui| {
            input(app, ui, xf, true, false);
        });
        output.textures_delta.clear();
    }

    #[test]
    fn pointer_drag_commits_once_and_escape_cancels() {
        let (mut app, handle) = app();
        let ctx = egui::Context::default();
        let xf = Xf { rect: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)), center: Vec2::ZERO, scale: 10.0, twist: 0.0 };
        let origin = xf.to_screen(Vec2::ZERO);
        let target = xf.to_screen(Vec2::new(-20.0, -10.0));
        step(
            &ctx,
            &mut app,
            &xf,
            vec![
                Event::PointerMoved(origin),
                Event::PointerButton { pos: origin, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
            ],
        );
        assert!(app.canvas.page_resize.is_some());
        step(&ctx, &mut app, &xf, vec![Event::PointerMoved(target)]);
        let frame = app.session.doc().unwrap().imported_page_frame(app.session.doc().unwrap().entity(handle).unwrap()).unwrap();
        assert_eq!(frame[0], Vec2::ZERO, "drag preview has not edited the drawing");
        step(
            &ctx,
            &mut app,
            &xf,
            vec![Event::PointerButton { pos: target, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }],
        );
        assert!(app.canvas.page_resize.is_none());
        let frame = app.session.doc().unwrap().imported_page_frame(app.session.doc().unwrap().entity(handle).unwrap()).unwrap();
        assert!((frame[0] - Vec2::new(-20.0, -10.0)).len() < 1e-9);
        let undo = app.session.state().unwrap().undo.len();
        step(
            &ctx,
            &mut app,
            &xf,
            vec![
                Event::PointerMoved(target),
                Event::PointerButton { pos: target, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
            ],
        );
        step(
            &ctx,
            &mut app,
            &xf,
            vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }],
        );
        assert!(app.canvas.page_resize.is_none());
        assert_eq!(app.session.state().unwrap().undo.len(), undo);
    }
}
