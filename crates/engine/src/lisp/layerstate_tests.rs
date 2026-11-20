//! `layerstate-*` and `vl-layerstates-*` (#3001).

#![allow(clippy::unwrap_used)]

use super::tests::session;
use super::value::{Value, to_text};
use crate::Session;

fn eval(s: &mut Session, code: &str) -> String {
    match s.lisp_eval(code, false, true, Vec::<Value>::new()) {
        Ok(Some(Ok(v))) => to_text(&v, true),
        other => panic!("{code}: {:?}", other.map(|o| o.map(|r| r.map(|v| to_text(&v, true))))),
    }
}

/// A drawing with layers WALLS and DOORS (the defaults: on, thawed, unlocked, plotting).
fn drawing() -> Session {
    let mut s = session();
    let d = s.doc_mut().unwrap();
    d.ensure_layer("WALLS");
    d.ensure_layer("DOORS");
    s
}

fn layer(s: &Session, name: &str) -> cadcraft_doc::Layer {
    s.doc().unwrap().layer(name).unwrap().clone()
}

fn change(s: &mut Session, name: &str, f: impl FnOnce(&mut cadcraft_doc::Layer)) {
    f(s.doc_mut().unwrap().layer_mut(name).unwrap());
}

/// The drawing's layer names as a LISP list.
fn names(s: &Session) -> String {
    let n: Vec<String> = s.doc().unwrap().layers.iter().map(|l| format!("{:?}", l.name)).collect();
    format!("({})", n.join(" "))
}

#[test]
fn restore_puts_back_every_property_and_returns_the_layer_names() {
    let mut s = drawing();
    let all = names(&s);
    assert_eq!(eval(&mut s, "(layerstate-save \"base\" 255 nil)"), "T");
    change(&mut s, "WALLS", |l| {
        l.on = false;
        l.locked = true;
        l.color = cadcraft_color::Color::Index(1);
        l.transparency = 40;
    });
    assert_eq!(eval(&mut s, "(layerstate-restore \"BASE\" nil)"), all);
    let w = layer(&s, "WALLS");
    assert!(w.on && !w.locked);
    assert_eq!(w.color, cadcraft_color::Color::Index(7));
    assert_eq!(w.transparency, 0);
}

#[test]
fn restore_flags_turn_off_or_freeze_the_layers_the_state_does_not_hold() {
    let mut s = drawing();
    eval(&mut s, "(layerstate-save \"a\" 255 nil)");
    s.doc_mut().unwrap().ensure_layer("NEW");
    eval(&mut s, "(layerstate-restore \"a\" nil 0)");
    let n = layer(&s, "NEW");
    assert!(n.on && !n.frozen, "flag 0 leaves the other layers alone");
    eval(&mut s, "(layerstate-restore \"a\" nil 1)");
    assert!(!layer(&s, "NEW").on);
    assert!(!layer(&s, "NEW").frozen);
    eval(&mut s, "(layerstate-restore \"a\" nil 2)");
    assert!(layer(&s, "NEW").frozen);
    assert!(layer(&s, "WALLS").on && !layer(&s, "WALLS").frozen, "the state's own layers are put back");
    // The current layer is not frozen, held by the state or not.
    change(&mut s, "NEW", |l| l.frozen = false);
    s.doc_mut().unwrap().header.set_str("CLAYER", "NEW");
    eval(&mut s, "(layerstate-restore \"a\" nil 3)");
    assert!(!layer(&s, "NEW").frozen);
}

#[test]
fn a_state_with_the_current_layer_frozen_leaves_it_thawed() {
    let mut s = drawing();
    change(&mut s, "WALLS", |l| l.frozen = true);
    eval(&mut s, "(layerstate-save \"frz\" 2 nil)");
    change(&mut s, "WALLS", |l| l.frozen = false);
    s.doc_mut().unwrap().header.set_str("CLAYER", "WALLS");
    eval(&mut s, "(layerstate-restore \"frz\" nil)");
    assert!(!layer(&s, "WALLS").frozen);
}

#[test]
fn viewport_overrides_are_refused() {
    let mut s = drawing();
    eval(&mut s, "(layerstate-save \"a\" 255 nil)");
    assert_eq!(eval(&mut s, "(layerstate-restore \"a\" T 4)"), "nil", "flag 4 with a viewport");
    assert_ne!(eval(&mut s, "(layerstate-restore \"a\" nil 4)"), "nil", "flag 4 without one is a plain restore");
}

#[test]
fn has_getnames_rename_delete() {
    let mut s = drawing();
    assert_eq!(eval(&mut s, "(layerstate-has \"one\")"), "nil");
    eval(&mut s, "(layerstate-save \"one\" 1 nil)");
    eval(&mut s, "(layerstate-save \"two\" 1 nil)");
    assert_eq!(eval(&mut s, "(layerstate-has \"ONE\")"), "T");
    assert_eq!(eval(&mut s, "(layerstate-getnames)"), "(\"one\" \"two\")");
    assert_eq!(eval(&mut s, "(layerstate-getnames T nil)"), "(\"one\" \"two\")");
    // Saving over a name replaces the state, it doesn't add one.
    eval(&mut s, "(layerstate-save \"One\" 1 nil)");
    assert_eq!(eval(&mut s, "(length (layerstate-getnames))"), "2");
    assert_eq!(eval(&mut s, "(layerstate-rename \"one\" \"first\")"), "T");
    assert_eq!(eval(&mut s, "(layerstate-has \"one\")"), "nil");
    assert_eq!(eval(&mut s, "(layerstate-rename \"first\" \"two\")"), "nil", "the new name is taken");
    assert_eq!(eval(&mut s, "(layerstate-rename \"nope\" \"x\")"), "nil");
    assert_eq!(eval(&mut s, "(layerstate-delete \"first\")"), "T");
    assert_eq!(eval(&mut s, "(layerstate-delete \"first\")"), "nil");
    assert_eq!(eval(&mut s, "(layerstate-restore \"first\" nil)"), "nil");
    assert_eq!(eval(&mut s, "(layerstate-getnames)"), "(\"two\")");
}

#[test]
fn unusable_names_and_hidden_view_snapshots_are_refused() {
    let mut s = drawing();
    assert_eq!(eval(&mut s, "(layerstate-save \"\" 1 nil)"), "nil");
    assert_eq!(eval(&mut s, "(layerstate-save \"*VIEW x\" 1 nil)"), "nil");
    assert_eq!(eval(&mut s, "(layerstate-save \"a/b\" 1 nil)"), "nil");
    s.doc_mut().unwrap().layer_states.push(cadcraft_doc::LayerState { name: "*VIEW v".into(), layers: Vec::new() });
    assert_eq!(eval(&mut s, "(layerstate-has \"*VIEW v\")"), "nil");
    assert_eq!(eval(&mut s, "(layerstate-delete \"*VIEW v\")"), "nil");
    assert_eq!(eval(&mut s, "(layerstate-getnames)"), "nil");
    // A mask that is not an integer is an error, not a panic.
    assert!(matches!(s.lisp_eval("(layerstate-save \"x\" \"1\" nil)", false, true, Vec::<Value>::new()), Ok(Some(Err(_)))));
}

#[test]
fn bricscad_restore_takes_a_property_mask_and_an_action_for_other_layers() {
    let mut s = drawing();
    assert_eq!(eval(&mut s, "(vl-layerstates-save \"base\")"), "T");
    change(&mut s, "WALLS", |l| {
        l.on = false;
        l.locked = true;
        l.color = cadcraft_color::Color::Index(1);
    });
    // Only on/off (1) and color (32): the lock stays as changed.
    assert_eq!(eval(&mut s, "(vl-layerstates-restore \"base\" 33)"), "T");
    let w = layer(&s, "WALLS");
    assert!(w.on, "on/off is in the mask");
    assert_eq!(w.color, cadcraft_color::Color::Index(7), "color is in the mask");
    assert!(w.locked, "lock is outside the mask and keeps its change");
    // -1 is every property; undefaction 1 turns off a layer the state doesn't hold.
    s.doc_mut().unwrap().ensure_layer("NEW");
    assert_eq!(eval(&mut s, "(vl-layerstates-restore \"base\" -1 nil 1)"), "T");
    assert!(!layer(&s, "WALLS").locked);
    assert!(!layer(&s, "NEW").on);
    assert_eq!(eval(&mut s, "(vl-layerstates-restore \"nope\")"), "nil");
}

#[test]
fn the_other_bricscad_names_and_symbol_names() {
    let mut s = drawing();
    assert_eq!(eval(&mut s, "(vl-layerstates-save 'b nil nil)"), "T");
    assert_eq!(eval(&mut s, "(layerstate-has \"b\")"), "T");
    assert_eq!(eval(&mut s, "(vl-layerstates-has 'B)"), "T");
    assert_eq!(eval(&mut s, "(vl-layerstates-rename \"b\" \"c\")"), "T");
    assert_eq!(eval(&mut s, "(vl-layerstates-list)"), "(\"c\")");
    assert_eq!(eval(&mut s, "(vl-layerstates-delete \"c\")"), "T");
    assert_eq!(eval(&mut s, "(vl-layerstates-has \"c\")"), "nil");
}
