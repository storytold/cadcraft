use super::*;
use crate::Services;
use cadcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::{Queryable, by};

fn radii(app: &CadApp, handles: &[Value]) -> Vec<f64> {
    let d = app.session.doc().unwrap();
    handles
        .iter()
        .filter_map(|h| cadcraft_doc::Handle::parse_hex(h.as_str()?))
        .filter_map(|h| match &d.entity(h)?.kind {
            EntityKind::Circle(c) => Some(c.radius),
            _ => None,
        })
        .collect()
}

/// The text boxes showing *VARIES* (their text run children carry the same value).
fn varies(h: &Harness<'static, CadApp>) -> usize {
    h.query_all(by().role(egui::accesskit::Role::TextInput).value("*VARIES*")).count()
}

#[test]
fn several_circles_show_varies_and_a_typed_radius_sets_them_all() {
    let mut app = CadApp::new(Session::default(), Services::default());
    for (x, r) in [(0, 5), (30, 8), (60, 12)] {
        app.run("circle", json!({ "center": [x, 0], "radius": r })).unwrap();
    }
    let handles: Vec<Value> = app.session.doc().unwrap().model.iter().map(|e| json!(e.handle.hex())).collect();
    assert_eq!(handles.len(), 3);
    app.run("select", json!({ "handles": handles })).unwrap();

    let mut h = Harness::builder().with_size(vec2(400.0, 800.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            properties_section(app, ui);
        },
        app,
    );
    h.run_steps(4);
    // Radius, diameter, circumference and area differ: each box says so.
    assert_eq!(varies(&h), 4);

    // The first of them is the radius: a value typed there is set for all three.
    let field = || by().role(egui::accesskit::Role::TextInput).value("*VARIES*");
    h.query_all(field()).next().unwrap().focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.query_all(field()).next().unwrap().type_text("7");
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert_eq!(radii(h.state(), &handles), vec![7.0, 7.0, 7.0]);
    // Now they share every value: no box says *VARIES* any more.
    assert_eq!(varies(&h), 0);
}
