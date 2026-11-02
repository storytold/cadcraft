//! AutoLISP in the app (#758): a dropped `.lsp` file loads (its `c:` functions become
//! commands), and a long routine runs a slice per frame so the window stays live.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};

#[test]
fn a_dropped_lisp_file_loads_and_defines_its_commands() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.open_bytes("Hello.LSP", b"(defun c:hello () (princ \"hi from lisp\") (princ))");
    assert!(app.session.log.iter().any(|l| l == "Hello.LSP successfully loaded."), "{:?}", app.session.log);
    // Still the same drawing: a LISP file isn't opened as one.
    assert_eq!(app.session.docs.len(), 1);
    app.session.cmdline("HELLO").unwrap();
    assert_eq!(app.session.log.last().map(String::as_str), Some("hi from lisp"));
}

#[test]
fn a_long_routine_runs_across_frames() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.open_bytes("loop.lsp", b"(setq i 0) (while (< i 2000000) (setq i (1+ i))) (princ \"done\")");
    assert!(app.session.lisp_busy(), "a slice per frame");
    let ctx = egui::Context::default();
    let mut frames = 0;
    while app.session.lisp_running() && frames < 1000 {
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear(); // no renderer here
        frames += 1;
    }
    assert!(!app.session.lisp_running());
    assert!(frames > 1, "it took more than one frame");
    assert_eq!(app.session.log.last().map(String::as_str), Some("done"));
}

/// A click on the canvas answers a routine's `getpoint`. No command runs while a routine waits,
/// and clicks used to go to object selection instead (found running the app by hand).
#[test]
fn a_canvas_click_answers_a_routine_waiting_for_a_point() {
    use cadcraft_engine::geom::Vec2;
    use egui::{Event, Modifiers, PointerButton, vec2};
    let mut app = CadApp::new(Session::new(), Services::default());
    app.canvas.zoom_pending = false;
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.state_mut().session.cmdline("(setq p (getpoint \"\nPick: \"))").unwrap();
    h.run_steps(2);
    let target = Vec2::new(1.0, 2.0);
    let pos = h.state().canvas.xf.unwrap().to_screen(target);
    h.event(Event::PointerMoved(pos));
    h.run_steps(2);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
    let app = h.state();
    assert!(!app.session.lisp_running(), "the click answered the prompt");
    let p = app.session.lisp_var("P").as_point().expect("a point");
    assert!((p[0] - target.x).abs() < 0.05 && (p[1] - target.y).abs() < 0.05, "{p:?}");
    assert!(app.session.selection().is_empty(), "the click selected nothing");
}
