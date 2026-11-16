//! Inside an MSPACE viewport an object on a layer frozen in that viewport (VPLAYER, "VP Freeze")
//! is not drawn there, so the selections made through that viewport leave it out: picks, windows,
//! the typed selection modes, the commands that select for themselves, and, for an object that is
//! already selected, its grips and the commands that start from the selection. In a viewport that
//! does not freeze the layer, in paper space and on the Model tab everything is as before.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle, Space};
use cadcraft_engine::geom::Vec2;
use serde_json::{Value, json};

/// Draws a line from `a` to `b` on the current layer and returns its handle.
fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    let r = s.execute("line", &json!({ "points": [a, b] })).unwrap();
    Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap()
}

/// Puts `h` on the layer "Hidden", which [`viewports`] freezes in its first viewport.
fn hide(s: &mut Session, h: Handle) {
    if s.doc().unwrap().layer("Hidden").is_none() {
        s.execute("layer.new", &json!({ "name": "Hidden" })).unwrap();
    }
    s.execute("properties.set", &json!({ "handles": [h.hex()], "layer": "Hidden" })).unwrap();
    assert_eq!(s.doc().unwrap().entity(h).unwrap().common.layer, "Hidden");
}

/// Two viewports on Layout1: the first freezes the layer "Hidden", the second freezes nothing.
/// The session is left in paper space.
fn viewports(s: &mut Session) -> (String, String) {
    if s.doc().unwrap().layer("Hidden").is_none() {
        s.execute("layer.new", &json!({ "name": "Hidden" })).unwrap();
    }
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    let r = s.execute("mview", &json!({ "count": 2, "p1": [20, 20], "p2": [220, 120] })).unwrap();
    let (a, b) = (r["viewports"][0].as_str().unwrap().to_string(), r["viewports"][1].as_str().unwrap().to_string());
    s.execute("vplayer", &json!({ "handles": [a], "freeze": ["Hidden"] })).unwrap();
    (a, b)
}

/// Makes `viewport` the active MSPACE viewport.
fn enter(s: &mut Session, viewport: &str) {
    s.execute("mspace", &json!({ "handle": viewport })).unwrap();
    assert_eq!(s.space(), Space::Model, "inside a viewport the edit space is model space");
}

/// What most tests draw: `shown` on layer 0 and, drawn after it and so on top of it, `hidden` on
/// the layer "Hidden", both from (0, 0) to (10, 0); and the two viewports of [`viewports`].
struct Sheet {
    s: Session,
    shown: Handle,
    hidden: Handle,
    /// The viewport that freezes "Hidden".
    a: String,
    /// The viewport that freezes nothing.
    b: String,
}

/// The drawing of [`Sheet`], in paper space.
fn sheet() -> Sheet {
    let mut s = Session::new();
    let shown = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let hidden = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    hide(&mut s, hidden);
    assert_eq!(s.doc().unwrap().entity(shown).unwrap().common.layer, "0");
    let (a, b) = viewports(&mut s);
    Sheet { s, shown, hidden, a, b }
}

/// The two ends of the line `h`.
fn ends(s: &Session, h: Handle) -> (Vec2, Vec2) {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Line(l) => (l.a.xy(), l.b.xy()),
        k => panic!("not a line: {k:?}"),
    }
}

/// Whether the line `h` still runs from (0, 0) to (10, 0).
fn untouched(s: &Session, h: Handle) -> bool {
    let (a, b) = ends(s, h);
    a.near(Vec2::new(0.0, 0.0), 1e-9) && b.near(Vec2::new(10.0, 0.0), 1e-9)
}

/// Whether the drawing still has the object `h`.
fn exists(s: &Session, h: Handle) -> bool {
    s.doc().unwrap().entity(h).is_some()
}

/// Every selected object, in handle order: the stored selection, which also holds the objects
/// the active viewport does not draw ([`Session::selection`] leaves those out).
fn selected(s: &Session) -> Vec<Handle> {
    let mut v = s.state().unwrap().selection.clone();
    v.sort();
    v
}

/// Whether the line `h` was erased or no longer runs from (0, 0) to (10, 0).
fn edited(s: &Session, h: Handle) -> bool {
    !exists(s, h) || !untouched(s, h)
}

/// The index of the grip of `h` at the origin.
fn origin_grip(s: &Session, h: Handle) -> usize {
    s.grips_of(h).unwrap().iter().position(|g| g.near(Vec2::new(0.0, 0.0), 1e-9)).unwrap()
}

/// `hs` in handle order.
fn sorted(mut hs: Vec<Handle>) -> Vec<Handle> {
    hs.sort();
    hs
}

/// Runs `text` as a script and ends whatever prompt it leaves open with Enter.
fn run(s: &mut Session, text: &str) {
    s.script(text).unwrap();
    for _ in 0..4 {
        if s.running.is_none() {
            break;
        }
        s.cmdline("").unwrap();
    }
    assert!(s.running.is_none(), "`{text}` left a prompt open: {}", s.prompt_text());
}

/// The JSON `select` command replaces the selection with what `how` finds.
fn select(s: &mut Session, how: Value) -> Vec<Handle> {
    s.set_selection(Vec::new());
    s.execute("select", &how).unwrap();
    selected(s)
}

/// A pick takes the topmost object the viewport draws: the frozen line lies on top of the other
/// one, and a pick at the same point finds the line under it. Where the layer is not frozen the
/// pick finds the line on top, as it always did.
#[test]
fn a_pick_finds_the_topmost_object_the_viewport_draws() {
    let Sheet { mut s, shown, hidden, a, b } = sheet();
    let at = json!({ "at": [5, 0], "aperture": 0.5 });
    enter(&mut s, &a);
    assert_eq!(select(&mut s, at.clone()), [shown], "the viewport freezes the layer of the line on top");
    enter(&mut s, &b);
    assert_eq!(select(&mut s, at.clone()), [hidden], "this viewport draws both: the one on top is picked");
    s.execute("layout.set", &json!({ "name": "Model" })).unwrap();
    assert_eq!(select(&mut s, at), [hidden], "the Model tab draws both");
}

/// Window, crossing and fence selection through the viewport leave the frozen line out, and take
/// both lines through the viewport that draws both.
#[test]
fn a_window_a_crossing_and_a_fence_leave_out_what_the_viewport_does_not_draw() {
    let Sheet { mut s, shown, hidden, a, b } = sheet();
    let ways =
        [json!({ "window": [[-1, -1], [11, 1]] }), json!({ "window": [[4, -1], [6, 1]], "crossing": true }), json!({ "fence": [[5, -1], [5, 1]] })];
    for how in &ways {
        enter(&mut s, &a);
        assert_eq!(select(&mut s, how.clone()), [shown], "{how}");
        enter(&mut s, &b);
        assert_eq!(select(&mut s, how.clone()), sorted(vec![shown, hidden]), "{how}");
    }
}

/// Clicks with no command running: a click on the two lines selects the one the viewport draws,
/// and a window dragged around them takes only that one.
#[test]
fn idle_clicks_and_windows_select_what_the_viewport_draws() {
    let Sheet { mut s, shown, hidden, a, b } = sheet();
    enter(&mut s, &a);
    s.idle_click(Vec2::new(5.0, 0.0), false).unwrap();
    assert_eq!(selected(&s), [shown]);
    s.set_selection(Vec::new());
    // A click on nothing opens a window; the next click closes it (left to right: a window).
    s.idle_click(Vec2::new(-1.0, -1.0), false).unwrap();
    assert!(s.pending_window.is_some(), "the first click must be on nothing");
    s.idle_click(Vec2::new(11.0, 1.0), false).unwrap();
    assert_eq!(selected(&s), [shown]);

    enter(&mut s, &b);
    s.idle_click(Vec2::new(5.0, 0.0), false).unwrap();
    assert_eq!(selected(&s), [hidden], "this viewport draws both: the one on top is picked");
    s.set_selection(Vec::new());
    s.idle_click(Vec2::new(-1.0, -1.0), false).unwrap();
    s.idle_click(Vec2::new(11.0, 1.0), false).unwrap();
    assert_eq!(selected(&s), sorted(vec![shown, hidden]));
}

/// Selection cycling lists the objects under the pick box that the viewport draws.
#[test]
fn selection_cycling_lists_what_the_viewport_draws() {
    let Sheet { mut s, shown, hidden, a, b } = sheet();
    enter(&mut s, &a);
    assert_eq!(s.pick_candidates(Vec2::new(5.0, 0.0)), [shown]);
    enter(&mut s, &b);
    assert_eq!(s.pick_candidates(Vec2::new(5.0, 0.0)), [hidden, shown], "topmost first");
}

/// Every way of answering "Select objects" leaves the frozen line out: ERASE through the
/// viewport that freezes its layer erases the other line only, and through the other viewport
/// both.
#[test]
fn the_answers_to_select_objects_leave_out_what_the_viewport_does_not_draw() {
    let Sheet { mut s, shown, hidden, a, b } = sheet();
    enter(&mut s, &a);
    s.execute("group", &json!({ "name": "PAIR", "handles": [shown.hex(), hidden.hex()] })).unwrap();
    // Group selection off: a pick takes the object alone, so each answer is tested by itself.
    s.settings.pickstyle = 0;
    let answers = [
        "5,0",
        "-1,-1 11,1",
        "ALL",
        "W -1,-1 11,1",
        "C 6,1 4,-1",
        "BOX 6,1 4,-1",
        "F 5,-1 5,1\n",
        "WP -1,-1 11,-1 11,1 -1,1\n",
        "CP 4,-1 6,-1 5,1\n",
        "G PAIR",
    ];
    for answer in answers {
        enter(&mut s, &a);
        run(&mut s, &format!("ERASE {answer}\n"));
        assert!(!exists(&s, shown), "`{answer}` must erase the line the viewport draws");
        assert!(exists(&s, hidden), "`{answer}` erased a line the viewport does not draw");
        s.execute("undo", &json!({})).unwrap();
        assert!(exists(&s, shown) && exists(&s, hidden));

        enter(&mut s, &b);
        run(&mut s, &format!("ERASE {answer}\n"));
        // A pick takes one object, the one on top; every other answer takes both.
        assert_eq!((exists(&s, shown), exists(&s, hidden)), (answer == "5,0", false), "`{answer}` where both are drawn");
        s.execute("undo", &json!({})).unwrap();
        assert!(exists(&s, shown) && exists(&s, hidden));
    }
}

/// With group selection on, a pick brings the members of its group that the viewport draws.
#[test]
fn a_pick_brings_the_group_members_the_viewport_draws() {
    let Sheet { mut s, shown, hidden, a, b } = sheet();
    enter(&mut s, &a);
    s.execute("group", &json!({ "name": "PAIR", "handles": [shown.hex(), hidden.hex()] })).unwrap();
    s.settings.pickstyle = 1;
    s.set_selection(Vec::new());
    s.idle_click(Vec2::new(5.0, 0.0), false).unwrap();
    assert_eq!(selected(&s), [shown]);
    enter(&mut s, &b);
    s.idle_click(Vec2::new(5.0, 0.0), false).unwrap();
    assert_eq!(selected(&s), sorted(vec![shown, hidden]));
}

/// Select All and Select Similar take what the viewport draws.
#[test]
fn select_all_and_select_similar_take_what_the_viewport_draws() {
    let Sheet { mut s, shown, hidden, a, b } = sheet();
    // Mode 0: any object of the same type is similar, whatever its layer.
    let similar = json!({ "handles": [shown.hex()], "mode": 0 });
    enter(&mut s, &a);
    s.execute("selectall", &json!({})).unwrap();
    assert_eq!(selected(&s), [shown]);
    s.execute("selectsimilar", &similar).unwrap();
    assert_eq!(selected(&s), [shown]);
    enter(&mut s, &b);
    s.execute("selectall", &json!({})).unwrap();
    assert_eq!(selected(&s), sorted(vec![shown, hidden]));
    s.execute("selectsimilar", &similar).unwrap();
    assert_eq!(selected(&s), sorted(vec![shown, hidden]));
}

/// STRETCH selects for itself: its crossing window, polygon and fence, called with JSON or
/// typed, and its typed All, Last and Previous, move the line the viewport draws and leave the
/// frozen one where it was.
#[test]
fn stretch_moves_what_the_viewport_draws() {
    let calls = [
        json!({ "window": [[9, -1], [11, 1]], "delta": [0, 5] }),
        json!({ "polygon": [[9, -1], [11, -1], [11, 1], [9, 1]], "delta": [0, 5] }),
        json!({ "fence": [[5, -1], [5, 1]], "delta": [0, 5] }),
    ];
    for call in &calls {
        let Sheet { mut s, shown, hidden, a, b } = sheet();
        enter(&mut s, &a);
        s.execute("stretch", call).unwrap();
        assert!(!untouched(&s, shown), "{call}: the line the viewport draws must move");
        assert!(untouched(&s, hidden), "{call}: a line the viewport does not draw moved: {:?}", ends(&s, hidden));
        enter(&mut s, &b);
        s.execute("stretch", call).unwrap();
        assert!(!untouched(&s, hidden), "{call}: where both are drawn both move");
    }
    // Typed: a crossing window, named and implied, All, a pick, Last, and Previous (both lines).
    for objects in ["C 11,1 9,-1", "11,1 9,-1", "ALL", "5,0", "L", "P"] {
        let typed = format!("STRETCH {objects}\n\n0,0 0,5\n");
        let Sheet { mut s, shown, hidden, a, b } = sheet();
        enter(&mut s, &a);
        s.remember_selection(&[shown, hidden]);
        run(&mut s, &typed);
        assert!(!untouched(&s, shown), "{typed:?}: the line the viewport draws must move");
        assert!(untouched(&s, hidden), "{typed:?}: a line the viewport does not draw moved: {:?}", ends(&s, hidden));
        enter(&mut s, &b);
        s.remember_selection(&[shown, hidden]);
        run(&mut s, &typed);
        assert!(!untouched(&s, hidden), "{typed:?}: where both are drawn the one on top moves");
    }
}

/// A multiline whose control vertex is inside STRETCH's crossing window is stretched although
/// the window misses its drawn lines, but not when the viewport does not draw it.
#[test]
fn stretch_leaves_a_multiline_the_viewport_does_not_draw() {
    /// The control vertices of the multiline `h`.
    fn vertices(s: &Session, h: Handle) -> Vec<Vec2> {
        match &s.doc().unwrap().entity(h).unwrap().kind {
            EntityKind::MLine(m) => m.vertices.iter().map(|v| v.p.xy()).collect(),
            k => panic!("not a multiline: {k:?}"),
        }
    }
    let mut s = Session::new();
    // Zero justification at scale 2: the element lines run one unit either side of the vertices.
    let r = s.execute("mline", &json!({ "points": [[0, 0], [10, 0]], "scale": 2, "justification": "zero" })).unwrap();
    let mline = Handle::parse_hex(r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap()).unwrap();
    hide(&mut s, mline);
    let (a, b) = viewports(&mut s);
    let call = json!({ "window": [[9.9, -0.1], [10.1, 0.1]], "delta": [0, 5] });
    enter(&mut s, &a);
    s.execute("stretch", &call).unwrap();
    assert!(vertices(&s, mline)[1].near(Vec2::new(10.0, 0.0), 1e-9), "a multiline the viewport does not draw was stretched");
    enter(&mut s, &b);
    s.execute("stretch", &call).unwrap();
    assert!(vertices(&s, mline)[1].near(Vec2::new(10.0, 5.0), 1e-9), "{:?}", vertices(&s, mline));
}

/// TRIM's fence and crossing window trim the line the viewport draws and leave the frozen one.
#[test]
fn trim_by_fence_and_crossing_leaves_what_the_viewport_does_not_draw() {
    for call in [json!({ "fence": [[8, -1], [8, 1]] }), json!({ "crossing": [[7.5, -1], [8.5, 1]] })] {
        let mut s = Session::new();
        let shown = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
        let hidden = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
        // The cutting edge, on a layer every viewport draws.
        line(&mut s, [5.0, -5.0], [5.0, 5.0]);
        hide(&mut s, hidden);
        let (a, b) = viewports(&mut s);
        enter(&mut s, &a);
        s.execute("trim", &call).unwrap();
        assert!(!untouched(&s, shown), "{call}: the line the viewport draws must be trimmed");
        assert!(untouched(&s, hidden), "{call}: a line the viewport does not draw was trimmed: {:?}", ends(&s, hidden));
        enter(&mut s, &b);
        s.execute("trim", &call).unwrap();
        assert!(!untouched(&s, hidden), "{call}: where both are drawn both are trimmed");
    }
}

/// A prompt that picks one object (BREAK's "Select object") finds the one the viewport draws.
#[test]
fn a_prompt_for_one_object_picks_what_the_viewport_draws() {
    let Sheet { mut s, shown, hidden, a, b } = sheet();
    enter(&mut s, &a);
    run(&mut s, "BREAK 4,0 6,0\n");
    assert!(untouched(&s, hidden), "a line the viewport does not draw was broken: {:?}", ends(&s, hidden));
    assert!(!untouched(&s, shown), "the line the viewport draws must be broken");
    enter(&mut s, &b);
    run(&mut s, "BREAK 4,0 6,0\n");
    assert!(!untouched(&s, hidden), "where both are drawn the one on top is broken");
}

/// LEADER's Copy option picks the text to copy: at a point where two texts lie, the one the
/// viewport draws.
#[test]
fn leader_copies_the_text_the_viewport_draws() {
    /// The contents of the multiline texts in model space: LEADER's copies.
    fn copies(s: &Session) -> Vec<String> {
        s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::MText(m) = &e.kind { Some(m.contents.clone()) } else { None }).collect()
    }
    let mut s = Session::new();
    for text in ["SHOWN", "HIDDEN"] {
        s.execute("text", &json!({ "at": [0, 0], "text": text, "height": 2.5 })).unwrap();
    }
    // The text drawn last lies on top.
    let hidden = *s.doc().unwrap().model.handles().last().unwrap();
    hide(&mut s, hidden);
    let (a, b) = viewports(&mut s);
    let leader = "(command \"_.LEADER\" '(20.0 20.0) '(30.0 25.0) \"\" \"\" \"_C\" '(1.0 1.0))";
    enter(&mut s, &a);
    s.cmdline(leader).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(copies(&s), ["SHOWN"], "the viewport freezes the layer of the text on top");
    s.execute("undo", &json!({})).unwrap();
    assert!(copies(&s).is_empty());
    enter(&mut s, &b);
    s.cmdline(leader).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(copies(&s), ["HIDDEN"], "this viewport draws both: the one on top is picked");
}

/// AutoLISP selects through the viewport too: `ssget` with a window, a crossing window, a fence,
/// a polygon, a crossing polygon or a point leaves the frozen line out, and `entsel` picks the
/// line the viewport draws. The sets made earlier (the implied selection, `ssgetfirst`, Previous)
/// come without the frozen line, and Last is the last object the viewport draws.
#[test]
fn lisp_selects_what_the_viewport_draws() {
    use cadcraft_engine::lisp::value::Value as Lisp;
    /// The objects of the selection set that `code` returns (none for nil), in handle order.
    fn set(s: &mut Session, code: &str) -> Vec<Handle> {
        match s.lisp_eval(code, false, true, Vec::new()).unwrap() {
            Some(Ok(Lisp::PickSet(set))) => sorted(set.items.lock().unwrap().clone()),
            Some(Ok(Lisp::Nil)) => Vec::new(),
            other => panic!("`{code}` returned {other:?}"),
        }
    }
    /// The object `entsel` returns for a pick at (5, 0).
    fn entsel(s: &mut Session) -> Handle {
        s.cmdline("(setq picked (car (entsel)))").unwrap();
        s.input(cadcraft_engine::Input::Point(Vec2::new(5.0, 0.0))).unwrap();
        match s.lisp_var("picked") {
            Lisp::Ename(h) => h,
            other => panic!("entsel returned {other:?}: {}", s.prompt_text()),
        }
    }
    let Sheet { mut s, shown, hidden, a, b } = sheet();
    let areas = [
        "(ssget \"W\" '(-1 -1) '(11 1))",
        "(ssget \"C\" '(4 -1) '(6 1))",
        "(ssget \"F\" '((5 -1) (5 1)))",
        "(ssget \"WP\" '((-1 -1) (11 -1) (11 1) (-1 1)))",
        "(ssget \"CP\" '((4 -1) (6 -1) (5 1)))",
    ];
    enter(&mut s, &a);
    for code in areas {
        assert_eq!(set(&mut s, code), [shown], "{code}");
    }
    assert_eq!(set(&mut s, "(ssget '(5 0))"), [shown], "a point picks the object the viewport draws");
    assert_eq!(entsel(&mut s), shown);
    enter(&mut s, &b);
    for code in areas {
        assert_eq!(set(&mut s, code), sorted(vec![shown, hidden]), "{code}");
    }
    assert_eq!(set(&mut s, "(ssget '(5 0))"), [hidden], "this viewport draws both: the one on top is picked");
    assert_eq!(entsel(&mut s), hidden);

    let earlier = ["(ssget \"I\")", "(ssget)", "(cadr (ssgetfirst))", "(ssget \"P\")"];
    for (viewport, expected) in [(&a, vec![shown]), (&b, sorted(vec![shown, hidden]))] {
        enter(&mut s, viewport);
        for code in earlier {
            s.set_selection(vec![shown, hidden]);
            s.remember_selection(&[shown, hidden]);
            assert_eq!(set(&mut s, code), expected, "{code}");
        }
    }
    // Last: of two lines made after everything else, the later one is on the layer the first
    // viewport freezes, so through that viewport Last is the earlier one.
    s.set_selection(Vec::new());
    enter(&mut s, &b);
    let drawn = line(&mut s, [0.0, 2.0], [10.0, 2.0]);
    let last = line(&mut s, [0.0, 3.0], [10.0, 3.0]);
    hide(&mut s, last);
    assert_eq!(set(&mut s, "(ssget \"L\")"), [last]);
    enter(&mut s, &a);
    assert_eq!(set(&mut s, "(ssget \"L\")"), [drawn], "Last must pass over an object the viewport does not draw");
}

/// The grips of a selected object the viewport does not draw take no part in a grip edit: a
/// shared grip stretches only the line that is drawn. Through the other viewport both stretch.
#[test]
fn a_selected_line_the_viewport_does_not_draw_is_not_stretched_with_a_shared_grip() {
    let Sheet { mut s, shown, hidden, a, b } = sheet();
    enter(&mut s, &a);
    s.set_selection(vec![shown, hidden]);
    let i = origin_grip(&s, shown);
    s.start_grip_edit(&[(shown, i)]).unwrap();
    s.cmdline("3,4").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(ends(&s, shown).0.near(Vec2::new(3.0, 4.0), 1e-9), "{:?}", ends(&s, shown));
    assert!(untouched(&s, hidden), "a line the viewport does not draw moved: {:?}", ends(&s, hidden));
    // Its own grip starts no edit there.
    let before = s.state().unwrap().doc.clone();
    let j = origin_grip(&s, hidden);
    assert!(s.start_grip_edit(&[(hidden, j)]).is_err());
    assert!(s.running.is_none());
    assert!(std::sync::Arc::ptr_eq(&before, &s.state().unwrap().doc), "nothing may change");

    s.execute("undo", &json!({})).unwrap();
    enter(&mut s, &b);
    s.set_selection(vec![shown, hidden]);
    s.start_grip_edit(&[(shown, i)]).unwrap();
    s.cmdline("3,4").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(ends(&s, shown).0.near(Vec2::new(3.0, 4.0), 1e-9), "{:?}", ends(&s, shown));
    assert!(ends(&s, hidden).0.near(Vec2::new(3.0, 4.0), 1e-9), "where both are drawn both stretch: {:?}", ends(&s, hidden));
}

/// A layer frozen in the viewport while its object is selected: from then on the object's grips
/// are not there, and thawing the layer in the viewport brings them back.
#[test]
fn freezing_a_layer_in_the_viewport_removes_the_grips_of_a_selected_object() {
    let Sheet { mut s, hidden, b, .. } = sheet();
    enter(&mut s, &b);
    s.set_selection(vec![hidden]);
    // The typed GRIP command finds a grip of the selected line at its start.
    let grip_there = |s: &mut Session| {
        s.cmdline("grip").unwrap();
        s.cmdline("0,0").unwrap();
        let none = s.log.iter().any(|l| l.contains("No grip of a selected object there"));
        s.input(cadcraft_engine::Input::Cancel).unwrap();
        s.log.clear();
        !none
    };
    assert!(grip_there(&mut s), "the viewport draws the line: its grip is there");
    s.execute("vplayer", &json!({ "handles": [b], "freeze": ["Hidden"] })).unwrap();
    s.set_selection(vec![hidden]);
    assert!(!grip_there(&mut s), "the viewport no longer draws the line");
    s.execute("vplayer", &json!({ "handles": [b], "thaw": ["Hidden"] })).unwrap();
    s.set_selection(vec![hidden]);
    assert!(grip_there(&mut s), "thawed in the viewport: the grip is back");
}

/// What the edited view shows: the frozen layer hides its model-space objects inside that
/// viewport only, and never an object on the sheet.
#[test]
fn only_the_viewport_that_freezes_a_layer_hides_its_model_objects() {
    let Sheet { mut s, shown, hidden, a, b } = sheet();
    // A line on the sheet, on the layer the first viewport freezes.
    let on_sheet = line(&mut s, [1.0, 1.0], [2.0, 1.0]);
    hide(&mut s, on_sheet);
    assert_eq!(s.doc().unwrap().space_of(on_sheet), Some(Space::Paper("Layout1".into())));
    let shows = |s: &Session, h: Handle| {
        let st = s.state().unwrap();
        st.shows(st.doc.entity(h).unwrap())
    };
    assert!(s.viewport_frozen().is_empty(), "paper space: no viewport is active");
    assert!(shows(&s, hidden) && shows(&s, shown) && shows(&s, on_sheet));
    enter(&mut s, &a);
    assert_eq!(s.viewport_frozen(), ["Hidden".to_string()]);
    assert!(!shows(&s, hidden), "frozen in the active viewport");
    assert!(shows(&s, shown));
    assert!(shows(&s, on_sheet), "the viewport draws model space: its frozen layers hide nothing on the sheet");
    // A prompt that picks one object on the sheet while the viewport is active finds it.
    let sheet_space = Space::Paper("Layout1".into());
    assert_eq!(s.pick_object(&sheet_space, Vec2::new(1.5, 1.0), 0.5).unwrap(), Some(on_sheet));
    assert_eq!(s.pick_object(&Space::Model, Vec2::new(5.0, 0.0), 0.5).unwrap(), Some(shown), "in model space the frozen line is left out");
    enter(&mut s, &b);
    assert!(s.viewport_frozen().is_empty());
    assert!(shows(&s, hidden));
    // Layer names are not case sensitive: frozen under another spelling, the line is hidden too.
    s.execute("vplayer", &json!({ "handles": [b], "freeze": ["HIDDEN"] })).unwrap();
    assert_eq!(s.viewport_frozen(), ["HIDDEN".to_string()]);
    assert!(!shows(&s, hidden));
    assert_eq!(select(&mut s, json!({ "at": [5, 0], "aperture": 0.5 })), [shown]);
    s.execute("vplayer", &json!({ "handles": [b], "thaw": ["hidden"] })).unwrap();
    assert!(shows(&s, hidden));
    s.execute("layout.set", &json!({ "name": "Model" })).unwrap();
    assert!(s.viewport_frozen().is_empty(), "the Model tab has no layout viewport");
    assert!(shows(&s, hidden));
}

/// An object that is selected when its layer is frozen in the active viewport stays selected, but
/// nothing that starts from the selection takes it there: a typed command with the selection
/// made before it (PICKFIRST), a JSON command that names no objects, and Delete.
#[test]
fn a_command_leaves_out_a_selected_object_the_viewport_does_not_draw() {
    /// One way of editing the selection.
    type Edit = fn(&mut Session);
    let edits: [(&str, Edit); 5] = [
        ("typed ERASE", |s: &mut Session| run(s, "ERASE\n")),
        ("typed MOVE", |s: &mut Session| run(s, "MOVE 0,0 0,5\n")),
        ("JSON erase", |s: &mut Session| drop(s.execute("erase", &json!({})).unwrap())),
        ("JSON move", |s: &mut Session| drop(s.execute("move", &json!({ "delta": [0, 5] })).unwrap())),
        ("Delete", |s: &mut Session| drop(s.execute("erase.selection", &json!({})).unwrap())),
    ];
    for (name, edit) in edits {
        // Where both lines are drawn, both are edited.
        let Sheet { mut s, shown, hidden, b, .. } = sheet();
        enter(&mut s, &b);
        s.set_selection(vec![shown, hidden]);
        edit(&mut s);
        assert!(edited(&s, shown) && edited(&s, hidden), "{name}: where both are drawn both are edited");

        // The layer is frozen in the active viewport while both lines are selected.
        let Sheet { mut s, shown, hidden, b, .. } = sheet();
        enter(&mut s, &b);
        s.set_selection(vec![shown, hidden]);
        s.execute("vplayer", &json!({ "handles": [b], "freeze": ["Hidden"] })).unwrap();
        assert_eq!(selected(&s), sorted(vec![shown, hidden]), "{name}: both lines must still be selected");
        assert_eq!(s.selection(), [shown], "{name}: the selection as the viewport has it");
        edit(&mut s);
        assert!(edited(&s, shown), "{name}: the line the viewport draws must be edited");
        assert!(!edited(&s, hidden), "{name}: a line the viewport does not draw was edited: {:?}", ends(&s, hidden));
    }

    // With only that line selected there is nothing to start from.
    let Sheet { mut s, hidden, a, b, .. } = sheet();
    enter(&mut s, &a);
    s.set_selection(vec![hidden]);
    assert!(s.selection().is_empty());
    assert!(s.execute("erase.selection", &json!({})).is_err(), "Delete must find nothing selected");
    s.script("ERASE\n").unwrap();
    assert!(s.running.is_some() && s.prompt_text().contains("Select objects"), "ERASE must ask for objects: {}", s.prompt_text());
    s.input(cadcraft_engine::Input::Cancel).unwrap();
    assert!(!edited(&s, hidden));
    // The line is still selected, and a viewport that draws it has it again.
    s.set_selection(vec![hidden]);
    s.execute("vplayer", &json!({ "handles": [a], "thaw": ["Hidden"] })).unwrap();
    assert_eq!(s.selection(), [hidden], "thawed in the viewport: selected as before");
    s.execute("vplayer", &json!({ "handles": [a], "freeze": ["Hidden"] })).unwrap();
    assert!(s.selection().is_empty());
    enter(&mut s, &b);
    s.set_selection(vec![hidden]);
    run(&mut s, "ERASE\n");
    assert!(!exists(&s, hidden), "where the line is drawn the selection made before ERASE is erased");
}

/// The Previous and Last options of "Select objects" take what the viewport draws: of the set a
/// command used before, and the last object that is not frozen there.
#[test]
fn previous_and_last_leave_out_what_the_viewport_does_not_draw() {
    let Sheet { mut s, shown, hidden, a, b } = sheet();
    for option in ["P", "L"] {
        enter(&mut s, &a);
        s.set_selection(Vec::new());
        s.remember_selection(&[shown, hidden]);
        run(&mut s, &format!("ERASE {option}\n"));
        assert!(!exists(&s, shown), "`{option}` must erase the line the viewport draws");
        assert!(exists(&s, hidden), "`{option}` erased a line the viewport does not draw");
        s.execute("undo", &json!({})).unwrap();
        assert!(exists(&s, shown) && exists(&s, hidden));

        enter(&mut s, &b);
        s.set_selection(Vec::new());
        s.remember_selection(&[shown, hidden]);
        run(&mut s, &format!("ERASE {option}\n"));
        // Previous is both lines; Last is the one made last, the line on the layer "Hidden".
        assert_eq!((exists(&s, shown), exists(&s, hidden)), (option == "L", false), "`{option}` where both are drawn");
        s.execute("undo", &json!({})).unwrap();
        assert!(exists(&s, shown) && exists(&s, hidden));
    }
}

/// The JSON grip edits that cover the selection (`grip.stretch` with hot grips, and
/// [`Session::grip_edit`]) drag the grips of the other selected objects on the same point, but
/// not the grip of a selected line the viewport does not draw.
#[test]
fn the_grip_edits_that_cover_the_selection_leave_out_what_the_viewport_does_not_draw() {
    use cadcraft_engine::grips::GripMode;
    /// Drags grip `i` of the line `h` to (3, 4).
    type Edit = fn(&mut Session, Handle, usize);
    let edits: [(&str, Edit); 2] = [
        ("grip.stretch", |s: &mut Session, h: Handle, i: usize| {
            drop(s.execute("grip.stretch", &json!({ "grips": [[h.hex(), i]], "to": [3, 4] })).unwrap())
        }),
        ("grip_edit", |s: &mut Session, h: Handle, i: usize| s.grip_edit(h, i, Vec2::new(3.0, 4.0), GripMode::Stretch).unwrap()),
    ];
    for (name, edit) in edits {
        for frozen in [true, false] {
            let Sheet { mut s, shown, hidden, a, b } = sheet();
            enter(&mut s, if frozen { &a } else { &b });
            s.set_selection(vec![shown, hidden]);
            let i = origin_grip(&s, shown);
            edit(&mut s, shown, i);
            assert!(ends(&s, shown).0.near(Vec2::new(3.0, 4.0), 1e-9), "{name}: {:?}", ends(&s, shown));
            assert_eq!(untouched(&s, hidden), frozen, "{name}, layer frozen in the viewport: {frozen}: {:?}", ends(&s, hidden));
        }
    }
}

/// What the session reports as the selection is the selection as the viewport has it, while
/// every selected object stays in the stored selection.
#[test]
fn the_reported_selection_leaves_out_what_the_viewport_does_not_draw() {
    /// The handles in `list`, a JSON array of hex strings, in handle order.
    fn handles(list: &Value) -> Vec<Handle> {
        sorted(list.as_array().unwrap().iter().map(|h| Handle::parse_hex(h.as_str().unwrap()).unwrap()).collect())
    }
    let Sheet { mut s, shown, hidden, a, b } = sheet();
    for (viewport, expected) in [(&a, vec![shown]), (&b, sorted(vec![shown, hidden]))] {
        enter(&mut s, viewport);
        s.set_selection(vec![shown, hidden]);
        assert_eq!(selected(&s), sorted(vec![shown, hidden]), "both lines are selected");
        assert_eq!(sorted(s.selection()), expected);
        assert_eq!(handles(&s.summary()["selection"]), expected);
        let inspect = s.execute("drawing.inspect", &json!({})).unwrap();
        assert_eq!(handles(&inspect["selection"]), expected);
    }
    // A selected object on the sheet is never left out, whatever its layer.
    s.execute("pspace", &json!({})).unwrap();
    let on_sheet = line(&mut s, [1.0, 1.0], [2.0, 1.0]);
    hide(&mut s, on_sheet);
    enter(&mut s, &a);
    s.set_selection(vec![shown, hidden, on_sheet]);
    assert_eq!(sorted(s.selection()), sorted(vec![shown, on_sheet]));
    // Where no viewport is active nothing is left out: in paper space and on the Model tab.
    s.execute("pspace", &json!({})).unwrap();
    s.set_selection(vec![shown, hidden, on_sheet]);
    assert_eq!(sorted(s.selection()), sorted(vec![shown, hidden, on_sheet]), "paper space");
    s.execute("layout.set", &json!({ "name": "Model" })).unwrap();
    s.set_selection(vec![shown, hidden]);
    assert_eq!(sorted(s.selection()), sorted(vec![shown, hidden]), "the Model tab");
}

/// Adding to the selection or removing from it inside the viewport keeps a selected object the
/// viewport does not draw: nothing drops it behind the user's back, and it counts again once its
/// layer is thawed there.
#[test]
fn changing_the_selection_keeps_a_selected_object_the_viewport_does_not_draw() {
    let Sheet { mut s, shown, hidden, b, .. } = sheet();
    enter(&mut s, &b);
    // A third line, on layer 0 like `shown`, away from the other two.
    let other = line(&mut s, [0.0, 5.0], [10.0, 5.0]);
    // Selects `hidden` where it is drawn, then freezes its layer in the active viewport.
    let freeze = |s: &mut Session| {
        s.execute("vplayer", &json!({ "handles": [b], "thaw": ["Hidden"] })).unwrap();
        s.set_selection(vec![hidden]);
        s.execute("vplayer", &json!({ "handles": [b], "freeze": ["Hidden"] })).unwrap();
        assert_eq!(selected(s), [hidden], "the line must still be selected");
        assert!(s.selection().is_empty(), "the viewport does not draw it");
    };

    // A click adds (PICKADD), Shift+click removes.
    freeze(&mut s);
    s.idle_click(Vec2::new(5.0, 5.0), false).unwrap();
    assert_eq!(selected(&s), sorted(vec![hidden, other]), "a click dropped the selected line the viewport does not draw");
    assert_eq!(s.selection(), [other]);
    s.idle_click(Vec2::new(5.0, 5.0), true).unwrap();
    assert_eq!(selected(&s), [hidden], "Shift+click must remove only the line clicked");

    // A window adds.
    freeze(&mut s);
    s.idle_click(Vec2::new(-1.0, 4.0), false).unwrap();
    assert!(s.pending_window.is_some(), "the first click must be on nothing");
    s.idle_click(Vec2::new(11.0, 6.0), false).unwrap();
    assert_eq!(selected(&s), sorted(vec![hidden, other]), "a window dropped the selected line the viewport does not draw");

    // The JSON `select` with `add`, and QSELECT appending the objects on layer 0.
    freeze(&mut s);
    s.execute("select", &json!({ "at": [5, 5], "aperture": 0.5, "add": true })).unwrap();
    assert_eq!(selected(&s), sorted(vec![hidden, other]), "`select` with `add`");
    // Given nothing to select with, `select` leaves the selection as it is.
    s.execute("select", &json!({})).unwrap();
    assert_eq!(selected(&s), sorted(vec![hidden, other]), "`select` with nothing to select with");
    freeze(&mut s);
    s.execute("qselect", &json!({ "layer": "0", "mode": "append" })).unwrap();
    assert_eq!(selected(&s), sorted(vec![shown, hidden, other]), "QSELECT appending");
    assert_eq!(sorted(s.selection()), sorted(vec![shown, other]));

    // Thawed in the viewport, the line counts as selected again.
    s.execute("vplayer", &json!({ "handles": [b], "thaw": ["Hidden"] })).unwrap();
    assert_eq!(sorted(s.selection()), sorted(vec![shown, hidden, other]));
}

/// [`Session::selection`] looks a few selected objects up one by one and, when there are many,
/// passes over model space once. Both leave out the same objects and keep the order of the
/// selection: in a drawing of some hundreds of lines whose draw order is not the order of their
/// handles, a selection of a few lines and the selection of every line.
#[test]
fn a_small_and_a_large_selection_leave_out_the_same_objects() {
    let Sheet { mut s, shown, hidden, a, b } = sheet();
    // A line on the sheet, on the layer the first viewport freezes: no viewport hides it.
    let on_sheet = line(&mut s, [1.0, 1.0], [2.0, 1.0]);
    hide(&mut s, on_sheet);
    assert!(!s.doc().unwrap().model.contains(on_sheet), "the line must be on the sheet");
    enter(&mut s, &b);
    let more: Vec<Handle> = (0..400).map(|i| line(&mut s, [f64::from(i), 5.0], [f64::from(i), 6.0])).collect();
    // Every second one of them goes on the layer the first viewport freezes.
    let frozen: Vec<String> = more.iter().step_by(2).map(|h| h.hex()).collect();
    s.execute("properties.set", &json!({ "handles": frozen, "layer": "Hidden" })).unwrap();
    // The frozen line made last goes to the back: it is now the first object in draw order.
    s.execute("draworder", &json!({ "handles": [more[398].hex()], "order": "back" })).unwrap();
    assert_eq!(s.doc().unwrap().model.iter().next().map(|e| e.handle), Some(more[398]));
    let all: Vec<Handle> = [shown, hidden].into_iter().chain(more.iter().copied()).collect();
    let drawn: Vec<Handle> = std::iter::once(shown).chain(more.iter().copied().skip(1).step_by(2)).collect();
    assert_eq!(s.doc().unwrap().model.len(), all.len());

    enter(&mut s, &a);
    s.set_selection(vec![shown, hidden]);
    assert_eq!(s.selection(), [shown], "two selected lines of some hundreds");
    s.set_selection(vec![hidden, on_sheet, more[0], more[1]]);
    assert_eq!(s.selection(), [on_sheet, more[1]], "a line on the sheet is never left out");
    s.set_selection(all.clone());
    assert_eq!(s.selection(), drawn, "every line selected");
    assert_eq!(s.state().unwrap().selection, all, "every line must still be selected");

    // The viewport that freezes nothing has them all, few or many.
    enter(&mut s, &b);
    s.set_selection(vec![shown, hidden]);
    assert_eq!(s.selection(), [shown, hidden]);
    s.set_selection(all.clone());
    assert_eq!(s.selection(), all);
}
