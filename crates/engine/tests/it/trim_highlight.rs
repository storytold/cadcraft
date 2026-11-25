//! #1118: at TRIM's and EXTEND's "Select object" prompt the rollover highlight shows what a click
//! there changes (the piece a trim removes, the stretch an extend adds), worked out by the same
//! code as the click itself, and leaves the drawing alone.

use cadcraft_engine::doc::{Entity, EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::json;

/// The object a command made: its `handle`, or the first of its `handles`.
fn h(v: &serde_json::Value) -> Handle {
    let hex = v["handle"].as_str().or_else(|| v["handles"][0].as_str()).unwrap_or_else(|| panic!("{v}"));
    Handle::parse_hex(hex).unwrap()
}

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    h(&s.execute("line", &json!({ "points": [a, b] })).unwrap())
}

/// The outlines of `ents`, flattened finely.
fn outline(s: &Session, ents: &[Entity]) -> Vec<Vec<Vec2>> {
    let d = s.doc().unwrap();
    ents.iter().flat_map(|e| cadcraft_engine::select::hit_polylines(d, e, 1e-4)).collect()
}

fn length(pls: &[Vec<Vec2>]) -> f64 {
    pls.iter().flat_map(|pl| pl.windows(2).map(|w| w[0].dist(w[1]))).sum()
}

/// The highlighted part, as outlines: `None` when the whole object is highlighted.
fn highlight(s: &Session, h: Handle, at: [f64; 2]) -> Option<Vec<Vec<Vec2>>> {
    let ents = s.highlight(h, Vec2::new(at[0], at[1]))?;
    Some(outline(s, &ents))
}

/// The highlighted part, as the ends of each stretch rounded to 1e-6.
fn ends(s: &Session, h: Handle, at: [f64; 2]) -> Option<Vec<[[i64; 2]; 2]>> {
    let r = |p: Vec2| [(p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64];
    let mut v: Vec<[[i64; 2]; 2]> = highlight(s, h, at)?
        .iter()
        .map(|pl| {
            let (a, b) = (r(pl[0]), r(*pl.last().unwrap()));
            if a <= b { [a, b] } else { [b, a] }
        })
        .collect();
    v.sort();
    Some(v)
}

fn seg(a: [f64; 2], b: [f64; 2]) -> [[i64; 2]; 2] {
    let r = |p: [f64; 2]| [(p[0] * 1e6).round() as i64, (p[1] * 1e6).round() as i64];
    let (a, b) = (r(a), r(b));
    if a <= b { [a, b] } else { [b, a] }
}

/// Two vertical edges at x = 0 and x = 10 and a horizontal line across both, from -5 to 15.
fn ladder() -> (Session, Handle) {
    let mut s = Session::new();
    line(&mut s, [0.0, -5.0], [0.0, 5.0]);
    line(&mut s, [10.0, -5.0], [10.0, 5.0]);
    let l = line(&mut s, [-5.0, 0.0], [15.0, 0.0]);
    s.zoom_extents().unwrap();
    (s, l)
}

#[test]
fn trim_highlights_only_the_piece_a_click_there_removes() {
    let (mut s, l) = ladder();
    s.start("trim").unwrap();
    assert_eq!(ends(&s, l, [5.0, 0.0]), Some(vec![seg([0.0, 0.0], [10.0, 0.0])]), "between the edges");
    assert_eq!(ends(&s, l, [-3.0, 0.0]), Some(vec![seg([-5.0, 0.0], [0.0, 0.0])]), "past the first edge");
    assert_eq!(ends(&s, l, [12.0, 0.0]), Some(vec![seg([10.0, 0.0], [15.0, 0.0])]), "past the second edge");
}

#[test]
fn the_highlight_leaves_the_drawing_and_its_undo_alone() {
    let (mut s, l) = ladder();
    s.start("trim").unwrap();
    let before = s.state().unwrap().doc.clone();
    for x in [-3.0, 5.0, 12.0, 5.0] {
        assert!(highlight(&s, l, [x, 0.0]).is_some());
    }
    assert!(std::sync::Arc::ptr_eq(&before, &s.state().unwrap().doc), "the drawing is the same object");
    s.input(Input::Enter).unwrap();
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().model.iter().count(), 2, "the one undo step is the last line drawn");
}

#[test]
fn quick_mode_highlights_the_whole_object_no_edge_crosses() {
    let (mut s, _) = ladder();
    let lone = line(&mut s, [20.0, 20.0], [25.0, 20.0]);
    s.start("trim").unwrap();
    assert_eq!(ends(&s, lone, [22.0, 20.0]), Some(vec![seg([20.0, 20.0], [25.0, 20.0])]), "a click erases it");
}

#[test]
fn a_click_removes_exactly_what_was_highlighted() {
    // Each kind of object crossed twice by the vertical lines x = -1 and x = 1.
    let shapes: Vec<(&str, serde_json::Value)> = vec![
        ("line", json!({ "points": [[-5, 0.5], [5, 0.5]] })),
        ("arc", json!({ "center": [0, -3], "radius": 4, "start": 20, "end": 160 })),
        ("circle", json!({ "center": [0, 0], "radius": 3 })),
        ("pline", json!({ "points": [[-5, 0], [-2, 2], [2, 2], [5, 0]] })),
        ("pline", json!({ "points": [[-4, -2], [4, -2], [4, 2], [-4, 2]], "closed": true })),
        ("ellipse", json!({ "center": [0, 0], "major": [4, 0], "ratio": 0.5 })),
        ("spline", json!({ "control": [[-5, 0], [-2, 3], [2, -3], [5, 0]] })),
    ];
    for (cmd, p) in shapes {
        let mut s = Session::new();
        line(&mut s, [-1.0, -10.0], [-1.0, 10.0]);
        line(&mut s, [1.0, -10.0], [1.0, 10.0]);
        let o = h(&s.execute(cmd, &p).unwrap());
        s.zoom_extents().unwrap();
        let whole = length(&outline(&s, &[(**s.doc().unwrap().entity(o).unwrap()).clone()]));
        // A point on the object between the two edges.
        let on = cadcraft_engine::select::hit_polylines(s.doc().unwrap(), s.doc().unwrap().entity(o).unwrap(), 1e-4)
            .iter()
            .flat_map(|pl| pl.windows(2).map(|w| (w[0], w[1])).collect::<Vec<_>>())
            .find(|(a, b)| a.x * b.x <= 0.0 && a.x != b.x)
            .map(|(a, b)| a + (b - a) * (-a.x / (b.x - a.x)))
            .unwrap_or_else(|| panic!("{cmd}: it doesn't cross x = 0"));
        s.start("trim").unwrap();
        let shown = length(&highlight(&s, o, [on.x, on.y]).unwrap_or_else(|| panic!("{cmd}: no part highlighted")));
        let total = |s: &Session| length(&outline(s, &s.doc().unwrap().model.iter().map(|e| (**e).clone()).collect::<Vec<_>>()));
        let before = total(&s);
        s.input(Input::Point(on)).unwrap();
        let removed = before - total(&s);
        assert!(removed > 1e-3, "{cmd}: the click trimmed nothing");
        assert!((shown - removed).abs() < 2e-3 * whole, "{cmd}: highlighted {shown}, removed {removed}");
    }
}

#[test]
fn extend_highlights_only_the_stretch_it_adds() {
    let mut s = Session::new();
    line(&mut s, [10.0, -5.0], [10.0, 5.0]);
    let l = line(&mut s, [0.0, 0.0], [4.0, 0.0]);
    s.zoom_extents().unwrap();
    s.start("extend").unwrap();
    assert_eq!(ends(&s, l, [3.0, 0.0]), Some(vec![seg([4.0, 0.0], [10.0, 0.0])]));
    s.input(Input::Point(Vec2::new(3.0, 0.0))).unwrap();
    let EntityKind::Line(x) = &s.doc().unwrap().entity(l).unwrap().kind else { panic!() };
    assert_eq!((x.b.x, x.b.y), (10.0, 0.0), "the click extends it as shown");
}

#[test]
fn the_whole_object_is_highlighted_where_a_click_does_nothing_special() {
    let (mut s, l) = ladder();
    // No command: the ordinary rollover highlight.
    assert!(s.highlight(l, Vec2::new(5.0, 0.0)).is_none());
    // At another of TRIM's prompts.
    s.start("trim").unwrap();
    s.input(Input::Keyword("Fence".into())).unwrap();
    assert!(s.highlight(l, Vec2::new(5.0, 0.0)).is_none());
    s.cancel();
    // On a locked layer, which TRIM refuses.
    s.execute("layer.set", &json!({ "name": "0", "locked": true })).unwrap();
    s.start("trim").unwrap();
    let k = l;
    assert!(s.highlight(k, Vec2::new(5.0, 0.0)).is_none());
}

#[test]
fn the_edges_are_gathered_again_once_the_drawing_changes() {
    let (mut s, l) = ladder();
    s.start("trim").unwrap();
    assert_eq!(ends(&s, l, [5.0, 0.0]), Some(vec![seg([0.0, 0.0], [10.0, 0.0])]));
    // Trimming the right edge's lower half away leaves the line crossed by the left edge only.
    let right = s.doc().unwrap().model.iter().find(|e| matches!(&e.kind, EntityKind::Line(x) if x.a.x == 10.0)).unwrap().handle;
    s.input(Input::Point(Vec2::new(10.0, -3.0))).unwrap();
    let still = matches!(&s.doc().unwrap().entity(right).unwrap().kind, EntityKind::Line(x) if x.a.y.min(x.b.y) >= 0.0);
    assert!(still, "the right edge now starts on the line");
    assert_eq!(ends(&s, l, [12.0, 0.0]), Some(vec![seg([10.0, 0.0], [15.0, 0.0])]), "it still meets the line at its end");
    assert_eq!(ends(&s, l, [-3.0, 0.0]), Some(vec![seg([-5.0, 0.0], [0.0, 0.0])]));
}

/// The highlight while a fence or a crossing window is drawn, as outlines.
fn highlight_all(s: &Session, at: [f64; 2]) -> Option<Vec<Vec<Vec2>>> {
    let ents = s.highlight_all(Vec2::new(at[0], at[1]))?;
    Some(outline(s, &ents))
}

/// The total length of every object's outline.
fn drawn(s: &Session) -> f64 {
    length(&outline(s, &s.doc().unwrap().model.iter().map(|e| (**e).clone()).collect::<Vec<_>>()))
}

#[test]
fn a_fence_highlights_every_piece_it_will_remove() {
    let (mut s, _) = ladder();
    let upper = line(&mut s, [-5.0, 2.0], [15.0, 2.0]);
    s.start("trim").unwrap();
    s.input(Input::Keyword("Fence".into())).unwrap();
    assert!(s.highlight_all(Vec2::new(5.0, 3.0)).is_none(), "nothing before the fence's first point");
    s.input(Input::Point(Vec2::new(5.0, -3.0))).unwrap();
    let before = s.state().unwrap().doc.clone();
    let shown = highlight_all(&s, [5.0, 3.0]).unwrap();
    let mut spans: Vec<[[i64; 2]; 2]> = shown
        .iter()
        .map(|pl| {
            let r = |p: Vec2| [(p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64];
            let (a, b) = (r(pl[0]), r(*pl.last().unwrap()));
            if a <= b { [a, b] } else { [b, a] }
        })
        .collect();
    spans.sort();
    assert_eq!(spans, vec![seg([0.0, 0.0], [10.0, 0.0]), seg([0.0, 2.0], [10.0, 2.0])], "both lines between the edges");
    assert!(std::sync::Arc::ptr_eq(&before, &s.state().unwrap().doc), "the drawing is left alone");
    // Short of the upper line, only the lower one.
    assert_eq!(highlight_all(&s, [5.0, 1.0]).unwrap().len(), 1);
    assert!(s.highlight(upper, Vec2::new(5.0, 2.0)).is_none(), "no single-object highlight while fencing");
}

#[test]
fn a_fence_removes_exactly_what_it_highlighted() {
    // A grid of lines and a circle, with a fence through several of them: some of what it trims
    // is an edge of what it trims next, so the preview must follow the run trim by trim.
    let mut s = Session::new();
    for x in [0.0, 10.0, 20.0] {
        line(&mut s, [x, -10.0], [x, 10.0]);
    }
    for y in [0.0, 5.0] {
        line(&mut s, [-5.0, y], [25.0, y]);
    }
    s.execute("circle", &json!({ "center": [10, 2.5], "radius": 4 })).unwrap();
    s.zoom_extents().unwrap();
    let fence = [[3.0, -4.0], [12.0, 3.0], [17.0, 8.0]];
    s.start("trim").unwrap();
    s.input(Input::Keyword("Fence".into())).unwrap();
    for p in &fence[..2] {
        s.input(Input::Point(Vec2::new(p[0], p[1]))).unwrap();
    }
    let shown = length(&highlight_all(&s, fence[2]).unwrap());
    let before = drawn(&s);
    s.input(Input::Point(Vec2::new(fence[2][0], fence[2][1]))).unwrap();
    s.input(Input::Enter).unwrap();
    let removed = before - drawn(&s);
    assert!(removed > 1.0, "the fence trimmed something");
    assert!((shown - removed).abs() < 1e-3 * before, "highlighted {shown}, removed {removed}");
}

#[test]
fn a_quick_fence_and_a_crossing_window_are_previewed_too() {
    // Quick mode: a pick in empty space starts a two-point fence.
    let (mut s, _) = ladder();
    s.start("trim").unwrap();
    s.input(Input::Point(Vec2::new(5.0, -3.0))).unwrap();
    let shown = highlight_all(&s, [5.0, 3.0]).expect("the two-point fence is previewed");
    assert!((length(&shown) - 10.0).abs() < 1e-6, "{}", length(&shown));
    s.cancel();
    // A crossing window round the middle of the line.
    s.start("trim").unwrap();
    s.input(Input::Keyword("Crossing".into())).unwrap();
    s.input(Input::Point(Vec2::new(4.0, -1.0))).unwrap();
    let shown = highlight_all(&s, [6.0, 1.0]).expect("the window is previewed");
    assert!((length(&shown) - 10.0).abs() < 1e-6, "{}", length(&shown));
}

#[test]
fn an_extend_fence_highlights_the_stretches_it_adds() {
    let mut s = Session::new();
    line(&mut s, [10.0, -5.0], [10.0, 5.0]);
    line(&mut s, [0.0, 0.0], [4.0, 0.0]);
    line(&mut s, [0.0, 2.0], [6.0, 2.0]);
    s.zoom_extents().unwrap();
    s.start("extend").unwrap();
    s.input(Input::Keyword("Fence".into())).unwrap();
    s.input(Input::Point(Vec2::new(4.0, -1.0))).unwrap();
    let shown = length(&highlight_all(&s, [4.0, 3.0]).unwrap());
    assert!((shown - (6.0 + 4.0)).abs() < 1e-6, "both lines run on to x = 10: {shown}");
    // And the fence extends them by just that.
    let before = drawn(&s);
    s.input(Input::Point(Vec2::new(4.0, 3.0))).unwrap();
    s.input(Input::Enter).unwrap();
    assert!((drawn(&s) - before - shown).abs() < 1e-6);
}

#[test]
fn a_fence_across_more_objects_than_the_preview_cap_shows_no_part() {
    use cadcraft_engine::cmd::TRIM_PREVIEW_MAX_PICKS as CAP;
    // Rungs crossed by one fence, between two rails that cut them. Each rung is one pick.
    let mut s = Session::new();
    line(&mut s, [0.0, 0.0], [0.0, 100.0]);
    line(&mut s, [10.0, 0.0], [10.0, 100.0]);
    let rung = |s: &mut Session, i: usize| {
        let y = 1.0 + i as f64;
        line(s, [-5.0, y], [15.0, y]);
    };
    for i in 0..CAP {
        rung(&mut s, i);
    }
    s.zoom_extents().unwrap();
    let fence = |s: &mut Session| {
        s.start("trim").unwrap();
        s.input(Input::Keyword("Fence".into())).unwrap();
        s.input(Input::Point(Vec2::new(5.0, 0.5))).unwrap();
    };
    // The fence runs up between the rails, crossing every rung: CAP picks, the most the preview takes.
    fence(&mut s);
    let shown = highlight_all(&s, [5.5, 99.0]).expect("up to the cap, the fence is previewed");
    assert!((length(&shown) - CAP as f64 * 10.0).abs() < 1e-6, "{}", length(&shown));
    s.cancel();
    // One more rung: past the cap there is no part highlight, and the fence still trims them all.
    rung(&mut s, CAP);
    fence(&mut s);
    assert!(s.highlight_all(Vec2::new(5.5, 99.0)).is_none(), "past the cap nothing is previewed");
    let before = drawn(&s);
    s.input(Input::Point(Vec2::new(5.5, 99.0))).unwrap();
    s.input(Input::Enter).unwrap();
    assert!((before - drawn(&s) - (CAP + 1) as f64 * 10.0).abs() < 1e-6, "every rung lost its middle");
}
