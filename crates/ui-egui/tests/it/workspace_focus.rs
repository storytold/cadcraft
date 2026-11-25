//! Real key traversal of the custom painted controls, without assigning widget focus.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::accesskit::Role;
use egui::{Key, Modifiers, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.integrated_titlebar = true;
    app.ui.in_window_menu = false;
    let mut h = Harness::builder().with_context_setup(cadcraft_ui_egui::framework::install_headless).with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            ui.ctx().set_os(egui::os::OperatingSystem::Mac);
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    steps(&mut h, 5);
    h
}

/// Runs `n` frames as the desktop app does, each through its raw input hook; the first takes the events queued in
/// `input_mut`.
fn steps(h: &mut Harness<'_, CadApp>, n: usize) {
    for _ in 0..n {
        let ctx = h.ctx.clone();
        let mut raw = std::mem::take(h.input_mut());
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        h.state_mut().before_input(&ctx);
        *h.input_mut() = raw;
        h.run_steps(1);
    }
}

/// Queues a press and release of `key` with `modifiers`.
fn tap(h: &mut Harness<'_, CadApp>, key: Key, modifiers: Modifiers) {
    let events = &mut h.input_mut().events;
    events.push(egui::Event::ModifiersChanged(modifiers));
    for pressed in [true, false] {
        events.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
    }
    events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
}

/// Focuses the control labelled `label` through its accessibility focus action.
fn focus(h: &mut Harness<'_, CadApp>, label: &str) {
    h.get_by_label(label).focus();
    steps(h, 2);
    assert!(h.get_by_label(label).is_focused(), "{label} takes the focus");
}

fn tab_to(h: &mut Harness<'_, CadApp>, label: &str) {
    for _ in 0..160 {
        if h.query_by_label(label).is_some_and(|node| node.is_focused()) {
            return;
        }
        tap(h, Key::Tab, Modifiers::NONE);
        steps(h, 2);
    }
    panic!("Tab traversal did not reach {label}");
}

/// Presses Tab with `mods`.
fn press(h: &mut Harness<'_, CadApp>, mods: Modifiers) {
    tap(h, Key::Tab, mods);
    steps(h, 2);
}

fn focused(h: &Harness<'_, CadApp>) -> Option<egui::Id> {
    h.ctx.memory(|memory| memory.focused())
}

fn type_text(h: &mut Harness<'_, CadApp>, text: &str) {
    h.input_mut().events.push(egui::Event::Text(text.into()));
    steps(h, 2);
}

fn running<'a>(h: &'a Harness<'_, CadApp>) -> Option<&'a str> {
    h.state().session.running.as_ref().map(|command| command.id.as_str())
}

#[test]
fn toolbar_tab_and_shift_tab_move_and_activate_once() {
    let mut h = harness();
    focus(&mut h, "New Drawing (⌘N)");
    tap(&mut h, Key::Tab, Modifiers::NONE);
    steps(&mut h, 2);
    assert!(h.get_by_label("Open (⌘O)").is_focused());
    tap(&mut h, Key::Tab, Modifiers::SHIFT);
    steps(&mut h, 2);
    assert!(h.get_by_label("New Drawing (⌘N)").is_focused());
    let before = h.state().session.docs.len();
    tap(&mut h, Key::Enter, Modifiers::NONE);
    steps(&mut h, 3);
    assert_eq!(h.state().session.docs.len(), before + 1);
}

#[test]
fn toolset_tabs_traverse_and_activate_without_drafting_commands() {
    let mut h = harness();
    focus(&mut h, "New Drawing (⌘N)");
    // From the document tabs, Tab goes over the drawing into the palettes, not back to the toolbar.
    tab_to(&mut h, "Close Drawing1");
    for _ in 0..40 {
        if h.get_by_label("Drafting").is_focused() {
            break;
        }
        press(&mut h, Modifiers::NONE);
        assert!(focused(&h).is_some() && !h.get_by_label("New Drawing (⌘N)").is_focused(), "Tab from the document tabs enters the palettes");
    }
    assert!(h.get_by_label("Drafting").is_focused(), "Tab from the document tabs reaches the palettes");
    tap(&mut h, Key::Tab, Modifiers::NONE);
    steps(&mut h, 2);
    assert!(h.get_by_label("Modeling").is_focused());
    tap(&mut h, Key::Space, Modifiers::NONE);
    steps(&mut h, 3);
    assert_eq!(h.state().ui.toolset_tab, "Modeling");
    assert!(h.state().session.running.is_none());
    tap(&mut h, Key::Tab, Modifiers::SHIFT);
    steps(&mut h, 2);
    assert!(h.get_by_label("Drafting").is_focused());
    tap(&mut h, Key::Enter, Modifiers::NONE);
    steps(&mut h, 3);
    assert_eq!(h.state().ui.toolset_tab, "Drafting");
    tab_to(&mut h, "Line");
    tap(&mut h, Key::Tab, Modifiers::NONE);
    steps(&mut h, 2);
    assert!(h.get_by_label("Polyline").is_focused());
    tap(&mut h, Key::Tab, Modifiers::SHIFT);
    steps(&mut h, 2);
    assert!(h.get_by_label("Line").is_focused());
    tap(&mut h, Key::Enter, Modifiers::NONE);
    steps(&mut h, 3);
    assert_eq!(h.state().session.running.as_ref().map(|command| command.id.as_str()), Some("line"));
}

#[test]
fn document_tabs_have_distinct_keyboard_identity_and_activation() {
    let mut h = harness();
    h.state_mut().run("new", serde_json::json!({})).unwrap();
    steps(&mut h, 3);
    let title = h.state().session.docs[0].title.clone();
    focus(&mut h, "New Drawing (⌘N)");
    tab_to(&mut h, "Start");
    tap(&mut h, Key::Tab, Modifiers::NONE);
    steps(&mut h, 2);
    assert!(h.get_by_label(&title).is_focused());
    tap(&mut h, Key::Space, Modifiers::NONE);
    steps(&mut h, 3);
    assert_eq!(h.state().session.active, 0);
    assert!(!h.state().ui.start_tab);
}

/// Tab walks one loop over the whole window, never leaving the focus nowhere, and Shift+Tab walks it back.
#[test]
fn shift_tab_walks_the_tab_loop_backward() {
    let mut h = harness();
    focus(&mut h, "New Drawing (⌘N)");
    let start = focused(&h);
    let mut forward = vec![start];
    loop {
        press(&mut h, Modifiers::NONE);
        assert!(focused(&h).is_some(), "Tab {} keeps a focus", forward.len());
        if focused(&h) == start {
            break;
        }
        forward.push(focused(&h));
        assert!(forward.len() < 400, "Tab returns to its start");
    }
    let mut back = vec![start];
    for _ in 1..forward.len() {
        press(&mut h, Modifiers::SHIFT);
        back.push(focused(&h));
    }
    back[1..].reverse();
    assert_eq!(back, forward);
}

/// The drawing as the focused Tab stop: typing runs commands as with nothing focused, and Tab is the command line's
/// while it has something to act on, else it moves on.
#[test]
fn the_focused_drawing_leaves_typing_to_the_command_line_and_keeps_tab_only_for_it() {
    let mut h = harness();
    focus(&mut h, "Close Drawing1");
    press(&mut h, Modifiers::NONE);
    let drawing = focused(&h);
    assert!(drawing.is_some(), "Tab from the document tabs stops at the drawing");
    type_text(&mut h, "LIN");
    press(&mut h, Modifiers::NONE);
    assert_eq!((h.state().cmd.buffer.as_str(), focused(&h)), ("LINE", drawing), "Tab completes the typed command");
    tap(&mut h, Key::Enter, Modifiers::NONE);
    steps(&mut h, 3);
    assert_eq!(running(&h), Some("line"));
    type_text(&mut h, "10");
    press(&mut h, Modifiers::NONE);
    assert_eq!((h.state().cmd.buffer.as_str(), focused(&h)), ("10,", drawing), "Tab goes to the second value box");
    tap(&mut h, Key::Escape, Modifiers::NONE);
    steps(&mut h, 3);
    assert_eq!((running(&h), h.state().cmd.buffer.as_str(), focused(&h)), (None, "", drawing), "Escape cancels the command");
    press(&mut h, Modifiers::NONE);
    assert!(focused(&h).is_some() && focused(&h) != drawing, "Tab moves on from the drawing");
    press(&mut h, Modifiers::SHIFT);
    assert_eq!(focused(&h), drawing);
    press(&mut h, Modifiers::SHIFT);
    assert!(h.get_by_label("Close Drawing1").is_focused(), "and Shift+Tab back");
}

/// The drawing's widget.
fn drawing() -> Option<egui::Id> {
    Some(egui::Id::unique("cadcraft-drawing"))
}

fn move_to(h: &mut Harness<'_, CadApp>, at: egui::Pos2) {
    h.input_mut().events.push(egui::Event::PointerMoved(at));
    steps(h, 2);
}

/// Clicks at `at` with the pointer.
fn click(h: &mut Harness<'_, CadApp>, at: egui::Pos2) {
    move_to(h, at);
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        steps(h, 2);
    }
}

fn click_label(h: &mut Harness<'_, CadApp>, label: &str) {
    let at = h.get_by_label(label).rect().center();
    click(h, at);
}

fn nothing_focused(h: &mut Harness<'_, CadApp>) {
    h.ctx.memory_mut(|memory| memory.surrender_focus(memory.focused().unwrap_or(egui::Id::NULL)));
    steps(h, 2);
    assert_eq!(focused(h), None);
}

/// The screen point `x`, `y` from the drawing area's centre.
fn at(h: &Harness<'_, CadApp>, x: f32, y: f32) -> egui::Pos2 {
    h.state().canvas.rect.unwrap().center() + vec2(x, y)
}

/// The drawing's coordinates under [`at`]`(x, y)`.
fn world(h: &Harness<'_, CadApp>, x: f32, y: f32) -> [f64; 2] {
    let p = h.state().canvas.xf.unwrap().to_world(at(h, x, y));
    [p.x, p.y]
}

#[test]
fn tab_with_nothing_focused_moves_into_the_window() {
    let mut h = harness();
    nothing_focused(&mut h);
    press(&mut h, Modifiers::NONE);
    assert!(h.get_by_label("New Drawing (⌘N)").is_focused());
}

/// A click in the drawing gives it the keyboard: Tab completes a typed command, cycles the snaps under the cursor and
/// moves to the next value box.
#[test]
fn tab_after_a_click_in_the_drawing_works_on_the_command_line() {
    let mut h = harness();
    let centre = world(&h, 100.0, 0.0);
    h.state_mut().run("circle", serde_json::json!({"center": centre, "radius": 1})).unwrap();
    nothing_focused(&mut h);
    // Two clicks in empty space: a selection window and its other corner.
    let empty = at(&h, -150.0, -120.0);
    click(&mut h, empty);
    assert_eq!(focused(&h), drawing());
    click(&mut h, empty - vec2(30.0, 30.0));
    type_text(&mut h, "LIN");
    press(&mut h, Modifiers::NONE);
    assert_eq!((h.state().cmd.buffer.as_str(), focused(&h)), ("LINE", drawing()), "Tab completes");
    tap(&mut h, Key::Enter, Modifiers::NONE);
    steps(&mut h, 3);
    let circle = at(&h, 100.0, 0.0);
    move_to(&mut h, circle);
    assert!(h.state().canvas.snap.is_some());
    press(&mut h, Modifiers::NONE);
    assert!(h.state().canvas.snap_cycle.is_some(), "Tab cycles the snaps");
    assert_eq!(focused(&h), drawing());
    move_to(&mut h, empty);
    type_text(&mut h, "10");
    press(&mut h, Modifiers::NONE);
    assert_eq!((h.state().cmd.buffer.as_str(), focused(&h)), ("10,", drawing()), "Tab moves to the next value box");
}

#[test]
fn an_opened_drawing_takes_the_focus() {
    let mut h = harness();
    assert_eq!(focused(&h), drawing(), "the first drawing");
    focus(&mut h, "New Drawing (⌘N)");
    tap(&mut h, Key::Enter, Modifiers::NONE);
    steps(&mut h, 3);
    assert_eq!((h.state().session.docs.len(), focused(&h)), (2, drawing()), "a new drawing");
    focus(&mut h, "Start");
    tap(&mut h, Key::Space, Modifiers::NONE);
    steps(&mut h, 3);
    assert_ne!(focused(&h), drawing(), "the Start page");
    focus(&mut h, "Drawing1");
    tap(&mut h, Key::Space, Modifiers::NONE);
    steps(&mut h, 3);
    assert_eq!((h.state().session.active, focused(&h)), (0, drawing()), "a drawing made active");
}

/// An opened drawing leaves the focus with a text field, and takes it from a modal dialog only once that has closed.
#[test]
fn an_opened_drawing_leaves_the_focus_with_a_text_field_and_a_dialog() {
    let mut h = harness();
    h.state_mut().run("ui.toggle.quickcalc", serde_json::json!({})).unwrap();
    steps(&mut h, 4);
    h.get_by_role_and_label(Role::TabPanel, "QuickCalc").get_all_by_role(Role::TextInput).next().unwrap().focus();
    steps(&mut h, 2);
    let field = focused(&h);
    assert!(h.ctx.text_edit_focused(), "QuickCalc's field");
    h.state_mut().run("new", serde_json::json!({})).unwrap();
    steps(&mut h, 3);
    assert_eq!(focused(&h), field);
    nothing_focused(&mut h);
    h.state_mut().run("circle", serde_json::json!({"center": [0, 0], "radius": 1})).unwrap();
    h.state_mut().request_close(None);
    steps(&mut h, 3);
    h.state_mut().run("new", serde_json::json!({})).unwrap();
    steps(&mut h, 3);
    assert_ne!(focused(&h), drawing(), "Save changes is up");
    h.state_mut().answer_close(cadcraft_ui_egui::closing::Choice::Cancel);
    steps(&mut h, 4);
    assert_eq!(focused(&h), drawing(), "Save changes has closed");
}

/// A command started with the mouse from the palette, and a status bar toggle clicked during it, leave Tab to the
/// command line.
#[test]
fn tab_stays_with_a_command_started_and_steered_with_the_mouse() {
    let mut h = harness();
    click_label(&mut h, "Line");
    assert_eq!((running(&h), focused(&h)), (Some("line"), drawing()));
    type_text(&mut h, "10");
    click_label(&mut h, "Ortho Mode (F8)");
    press(&mut h, Modifiers::NONE);
    assert_eq!((h.state().cmd.buffer.as_str(), focused(&h)), ("10,", drawing()));
}

/// Focuses the text box showing `value` with Tab.
fn tab_to_field(h: &mut Harness<'_, CadApp>, value: &str) {
    for _ in 0..200 {
        let field = |node: &egui_kittest::Node<'_>| node.is_focused() && node.value().as_deref() == Some(value);
        if h.query_all_by_role(Role::TextInput).any(|node| field(&node)) {
            return;
        }
        press(h, Modifiers::NONE);
    }
    panic!("Tab does not reach the field {value}");
}

/// A click in the drawing takes the keyboard from a Properties field Tab reached, whose edit it commits.
#[test]
fn a_click_in_the_drawing_takes_the_keyboard_from_a_field() {
    let mut h = harness();
    let place = world(&h, 100.0, 100.0);
    let note = h.state_mut().run("text", serde_json::json!({"at": place, "text": "Note", "height": 2.5})).unwrap();
    h.state_mut().run("select", serde_json::json!({"handles": [note["handle"]]})).unwrap();
    steps(&mut h, 3);
    tab_to_field(&mut h, "2.5000");
    tap(&mut h, Key::A, Modifiers::COMMAND);
    steps(&mut h, 1);
    type_text(&mut h, "4");
    let empty = at(&h, -150.0, -120.0);
    click(&mut h, empty);
    assert_eq!(focused(&h), drawing());
    let heights: Vec<f64> = h
        .state()
        .session
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let cadcraft_engine::doc::EntityKind::Text(t) = &e.kind { Some(t.height) } else { None })
        .collect();
    assert_eq!(heights, [4.0], "the click commits the field");
    click(&mut h, empty - vec2(30.0, 30.0));
    type_text(&mut h, "LINE");
    assert_eq!(h.state().cmd.buffer, "LINE");
    tap(&mut h, Key::Enter, Modifiers::NONE);
    steps(&mut h, 3);
    assert_eq!(running(&h), Some("line"));
}

/// A click in the drawing takes the keyboard from a palette button Tab reached: Enter and Space are the command line's,
/// not the button's.
#[test]
fn a_click_in_the_drawing_takes_the_keyboard_from_a_button() {
    let mut h = harness();
    tab_to(&mut h, "Line");
    let empty = at(&h, -150.0, -120.0);
    click(&mut h, empty);
    click(&mut h, empty - vec2(30.0, 30.0));
    assert_eq!(focused(&h), drawing());
    type_text(&mut h, "CIRCLE");
    tap(&mut h, Key::Enter, Modifiers::NONE);
    steps(&mut h, 3);
    assert_eq!(running(&h), Some("circle"));
    tap(&mut h, Key::Escape, Modifiers::NONE);
    steps(&mut h, 3);
    assert_eq!(running(&h), None);
    type_text(&mut h, "LINE");
    tap(&mut h, Key::Space, Modifiers::NONE);
    type_text(&mut h, " ");
    assert_eq!(running(&h), Some("line"));
}

/// The focused drawing leaves the shortcuts and the system clipboard to CADCraft, as with nothing focused.
#[test]
fn shortcuts_and_paste_work_with_the_drawing_focused() {
    let mut h = harness();
    focus(&mut h, "Close Drawing1");
    press(&mut h, Modifiers::NONE);
    assert_eq!(focused(&h), drawing());
    let ortho = h.state().session.settings.orthomode;
    tap(&mut h, Key::F8, Modifiers::NONE);
    steps(&mut h, 2);
    assert_ne!(h.state().session.settings.orthomode, ortho, "F8");
    let line = h.state_mut().run("line", serde_json::json!({"points": [[0, 0], [10, 0]]})).unwrap();
    assert!(!line.is_null());
    tap(&mut h, Key::Z, Modifiers::COMMAND);
    steps(&mut h, 2);
    assert!(h.state().session.doc().unwrap().model.is_empty(), "Undo");
    h.state_mut().run("line", serde_json::json!({"points": [[0, 0], [10, 0]]})).unwrap();
    h.state_mut().run("selectall", serde_json::json!({})).unwrap();
    h.input_mut().events.push(egui::Event::Copy);
    steps(&mut h, 2);
    assert_eq!(h.state().session.clipboard.len(), 1, "Copy");
    h.state_mut().session.set_selection(Vec::new());
    h.state_mut().start("pasteclip");
    steps(&mut h, 1);
    assert_eq!(running(&h), None, "a menu's Paste waits for the system clipboard");
    h.input_mut().events.push(egui::Event::Paste("text from another program".into()));
    steps(&mut h, 2);
    assert_eq!((running(&h), focused(&h)), (Some("pasteclip"), drawing()), "Paste");
    let before = h.state().session.settings.orthomode;
    tap(&mut h, Key::Escape, Modifiers::NONE);
    steps(&mut h, 2);
    assert_eq!((running(&h), h.state().session.settings.orthomode), (None, before), "Escape");
}

/// Presses and releases the right button at `at` with `modifiers` held.
fn right_click(h: &mut Harness<'_, CadApp>, at: egui::Pos2, modifiers: Modifiers) {
    move_to(h, at);
    h.input_mut().events.push(egui::Event::ModifiersChanged(modifiers));
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers });
        steps(h, 2);
    }
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
    steps(h, 2);
    assert!(drawing_menu(h), "the right-click opens the menu");
    assert_eq!(focused(h), drawing());
}

/// Whether a right-click menu of the drawing is open.
fn drawing_menu(h: &Harness<'_, CadApp>) -> bool {
    drawing().is_some_and(|id| egui::Popup::is_id_open(&h.ctx, id.with("popup")))
}

/// Presses Enter or Space as the keyboard does, Space with its text.
fn enter_or_space(h: &mut Harness<'_, CadApp>, key: Key) {
    let event = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    let events = &mut h.input_mut().events;
    events.push(event(true));
    if key == Key::Space {
        events.push(egui::Event::Text(" ".into()));
    }
    events.push(event(false));
    steps(h, 3);
}

/// Enter and Space with a menu of the focused drawing open go to the command line and close no menu.
#[test]
fn enter_and_space_leave_the_drawings_menus_open() {
    let mut h = harness();
    h.state_mut().options.time_sensitive_right_click = false;
    let empty = at(&h, -150.0, -120.0);
    // The shortcut menu, with nothing typed and no command to repeat.
    right_click(&mut h, empty, Modifiers::NONE);
    for key in [Key::Enter, Key::Space] {
        enter_or_space(&mut h, key);
        assert!(drawing_menu(&h), "{key:?} with the shortcut menu open");
    }
    type_text(&mut h, "LINE");
    enter_or_space(&mut h, Key::Space);
    assert_eq!(running(&h), Some("line"), "Space enters what was typed");
    tap(&mut h, Key::Escape, Modifiers::NONE);
    steps(&mut h, 3);
    // The object snap menu stays through the command Enter or Space repeats.
    for key in [Key::Enter, Key::Space] {
        right_click(&mut h, empty, Modifiers::SHIFT);
        enter_or_space(&mut h, key);
        assert_eq!((running(&h), drawing_menu(&h)), (Some("line"), true), "{key:?} with the object snap menu open");
        tap(&mut h, Key::Escape, Modifiers::NONE);
        steps(&mut h, 3);
        assert_eq!((running(&h), drawing_menu(&h)), (None, false), "Escape");
    }
    // The menu of a line's end grip.
    let (from, to) = (world(&h, 100.0, 0.0), world(&h, 200.0, -60.0));
    h.state_mut().run("line", serde_json::json!({"points": [from, to]})).unwrap();
    let line = h.state().session.doc().unwrap().model.handles();
    h.state_mut().session.set_selection(line);
    h.state_mut().session.last_command = None;
    steps(&mut h, 2);
    let others = h.query_all_by_label("Lengthen").count();
    let grip = at(&h, 100.0, 0.0);
    right_click(&mut h, grip, Modifiers::NONE);
    for key in [Key::Enter, Key::Space] {
        enter_or_space(&mut h, key);
        assert!(drawing_menu(&h) && h.query_all_by_label("Lengthen").count() > others, "{key:?} with the grip menu open");
    }
}
