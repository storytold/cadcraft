//! While TEXT asks for the text, what is typed on the command line is drawn on the canvas
//! where the line goes (AutoCAD types it in place); Enter places it and the next line starts.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Key, Modifiers, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness(render: bool) -> Harness<'static, CadApp> {
    let mut session = Session::new();
    session.execute("view.set", &json!({ "center": [6, -1], "height": 12 })).unwrap();
    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = false;
    let builder = Harness::builder().with_size(vec2(1200.0, 800.0));
    let builder = if render { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn texts(app: &CadApp) -> Vec<String> {
    app.session.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None }).collect()
}

#[test]
fn the_line_being_typed_is_drawn_in_place() {
    let shots = std::env::var_os("CADCRAFT_TEXT_SHOTS").map(std::path::PathBuf::from);
    let mut h = harness(shots.is_some());
    for line in ["TEXT", "0,0", "1", "0"] {
        assert!(h.state_mut().cmdline(line).is_none(), "{line}");
    }
    h.run_steps(2);
    // Spaces are part of the text at this prompt, not Enter.
    for c in "Hello world".chars() {
        h.event(Event::Text(c.to_string()));
        h.run_steps(1);
    }
    h.run_steps(2);
    assert_eq!(h.state().cmd.buffer, "Hello world");
    let preview = h.state().session.typing_preview(&h.state().cmd.buffer);
    assert!(matches!(&preview[..], [e] if matches!(&e.kind, EntityKind::Text(t) if t.value == "Hello world")), "{preview:?}");
    assert!(texts(h.state()).is_empty(), "nothing is placed while typing");
    if let Some(dir) = &shots {
        std::fs::create_dir_all(dir).unwrap();
        h.render().unwrap().save(dir.join("text-typing.png")).unwrap();
    }
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(texts(h.state()), ["Hello world"]);
    assert!(h.state().cmd.buffer.is_empty());
    for c in "Second".chars() {
        h.event(Event::Text(c.to_string()));
        h.run_steps(1);
    }
    h.run_steps(2);
    if let Some(dir) = &shots {
        h.render().unwrap().save(dir.join("text-typing-second.png")).unwrap();
    }
    h.key_press(Key::Enter);
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::NONE, Key::Enter);
    h.run_steps(3);
    assert_eq!(texts(h.state()), ["Hello world", "Second"]);
    assert!(h.state().session.running.is_none());
}
