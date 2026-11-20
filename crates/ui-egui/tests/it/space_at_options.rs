//! Space is Enter at TRIM's and EXTEND's option prompts, as at AutoCAD's (#3092): typing `O`,
//! Space, `S`, Space switches to Standard mode instead of typing a space into the command line.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Key, Modifiers, vec2};
use egui_kittest::Harness;

fn harness() -> Harness<'static, CadApp> {
    let app = CadApp::new(Session::new(), Services::default());
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

/// Type `text` at the keyboard: a space arrives as the Space key and as text, like a real press.
fn type_keys(h: &mut Harness<'static, CadApp>, text: &str) {
    for c in text.chars() {
        if c == ' ' {
            h.event(egui::Event::Key { key: Key::Space, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        }
        h.event(egui::Event::Text(c.to_string()));
        h.run_steps(1);
    }
    h.run_steps(1);
}

fn header(h: &Harness<'static, CadApp>, name: &str) -> i64 {
    h.state().session.doc().unwrap().header.i64(name, -1)
}

fn prompt(h: &Harness<'static, CadApp>) -> String {
    h.state().session.current_prompt().map(|p| p.message).unwrap_or_default()
}

#[test]
fn space_answers_trim_and_extend_option_prompts() {
    let mut h = harness();
    type_keys(&mut h, "trim o s ");
    assert_eq!(h.state().cmd.buffer, "", "Space typed a space instead of entering the option");
    assert_eq!(header(&h, "TRIMEXTENDMODE"), 0, "S chose Standard mode");
    // Standard mode asks for cutting edges; Enter selects all, then Project and Edge.
    type_keys(&mut h, " p n e e ");
    assert_eq!(h.state().cmd.buffer, "");
    assert_eq!(header(&h, "PROJMODE"), 0, "N chose no projection");
    assert_eq!(header(&h, "EDGEMODE"), 1, "E chose Extend");
    assert!(prompt(&h).starts_with("Select object to trim"), "back at the trim prompt: {}", prompt(&h));
    h.key_press(Key::Escape);
    h.run_steps(2);
    type_keys(&mut h, "extend  o q ");
    assert_eq!(h.state().cmd.buffer, "");
    assert_eq!(header(&h, "TRIMEXTENDMODE"), 1, "Q chose Quick mode in EXTEND");
}
