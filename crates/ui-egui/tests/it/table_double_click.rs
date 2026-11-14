//! Double-clicking a table cell edits it in place, as in AutoCAD (#3136).

use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

/// A 2 × 2 table from (0, 0) down, its cells 4 × 4: B2 has "d" and is centred at (6, -6).
fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("table", json!({ "at": [0, 0], "rows": 2, "cols": 2, "rowHeight": 4, "colWidth": 4, "cells": [["a", "b"], ["c", "d"]] })).unwrap();
    app.session.execute("view.set", &json!({ "center": [4, -4], "height": 30 })).unwrap();
    app.canvas.zoom_pending = false;
    // 60 frames a second: the default step (0.25 s) spreads two clicks past the double-click delay.
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn double_click(h: &mut Harness<'_, CadApp>, at: Vec2) {
    let pos: Pos2 = h.state().canvas.xf.unwrap().to_screen(at);
    h.event(Event::PointerMoved(pos));
    h.run_steps(2);
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            h.run_steps(1);
        }
    }
    h.run_steps(2);
}

fn cell_text(h: &Harness<'_, CadApp>, row: usize, col: usize) -> String {
    let d = h.state().session.doc().unwrap();
    let t = d.model.iter().find_map(|e| if let EntityKind::Table(t) = &e.kind { Some(t) } else { None }).expect("the table");
    t.cells[row][col].text.clone()
}

#[test]
fn double_click_on_a_table_cell_opens_its_in_place_editor() {
    let mut h = harness();
    double_click(&mut h, Vec2::new(6.0, -6.0));
    let s = &h.state().session;
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("tabledit"));
    let p = s.current_prompt().unwrap();
    assert_eq!(p.message, "Enter text for cell B2 (=formula)");
    assert_eq!(p.default.as_deref(), Some("d"), "the cell's text is the default");
    let mt = s.editing_mtext().expect("the in-place editor opens on the clicked cell");
    assert_eq!(mt.contents, "d");
    // B2's text starts at the cell's left margin (the data style is left/middle aligned) and is
    // centred down the row, 4..8 wide and 4..8 down.
    assert!((mt.insert.x - (4.0 + 0.06)).abs() < 1e-9, "{}", mt.insert.x);
    assert!((mt.insert.y + 6.0).abs() < 1e-9, "{}", mt.insert.y);
    assert_eq!(mt.attach, 4);
    assert!(mt.width > 0.0 && mt.width < 4.0);
    h.state_mut().session.input(Input::Text("edited".into())).unwrap();
    h.run_steps(2);
    assert!(h.state().session.running.is_none());
    assert_eq!(cell_text(&h, 1, 1), "edited");
    assert_eq!(cell_text(&h, 1, 0), "c", "only the clicked cell changed");
}

#[test]
fn double_click_away_from_a_table_starts_no_command() {
    let mut h = harness();
    double_click(&mut h, Vec2::new(-10.0, -10.0));
    assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), None);
}
