//! #2182: PEDIT Multiple at the command line, as AutoCAD: select several objects, turn lines and
//! arcs into polylines, then each option applies to all of them; Join joins them with each other.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

fn polylines(s: &Session) -> Vec<(usize, bool, f64)> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let EntityKind::LwPolyline(p) = &e.kind { Some((p.vertices.len(), p.closed, p.const_width)) } else { None })
        .collect()
}

fn count(s: &Session) -> usize {
    s.doc().unwrap().model.len()
}

fn lines(s: &mut Session, pts: &[[f64; 4]]) {
    for p in pts {
        s.execute("line", &json!({ "points": [[p[0], p[1]], [p[2], p[3]]] })).unwrap();
    }
}

#[test]
fn join_turns_four_lines_into_one_closed_polyline() {
    let mut s = Session::new();
    lines(&mut s, &[[0.0, 0.0, 10.0, 0.0], [10.0, 0.0, 10.0, 5.0], [10.0, 5.0, 0.0, 5.0], [0.0, 5.0, 0.0, 0.0]]);
    for t in ["PEDIT", "M", "ALL", "", "Y", "J", "", ""] {
        s.cmdline(t).unwrap();
    }
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(polylines(&s), vec![(4, true, 0.0)]);
    assert_eq!(count(&s), 1);
    assert!(s.log.iter().any(|l| l.contains("3 segments added to polyline")), "{:?}", s.log);
}

#[test]
fn join_bridges_gaps_within_the_fuzz_distance_only() {
    // Two chains with a 0.5 gap between them.
    let gap = |s: &mut Session| lines(s, &[[0.0, 0.0, 10.0, 0.0], [10.5, 0.0, 20.0, 0.0]]);
    let mut s = Session::new();
    gap(&mut s);
    for t in ["PE", "M", "ALL", "", "Y", "J", "0", ""] {
        s.cmdline(t).unwrap();
    }
    assert_eq!(polylines(&s).len(), 2, "no fuzz: still two");
    let mut s = Session::new();
    gap(&mut s);
    for t in ["PE", "M", "ALL", "", "Y", "J", "1", ""] {
        s.cmdline(t).unwrap();
    }
    assert_eq!(polylines(&s).len(), 1, "a fuzz of 1 joins them");
}

#[test]
fn options_apply_to_all_and_undo_takes_one_back() {
    let mut s = Session::new();
    s.cmdline("pline 0,0 10,0 10,10").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("pline 20,0 30,0 30,10").unwrap();
    s.cmdline("").unwrap();
    for t in ["PEDIT", "M", "ALL", "", "W", "0.5", "C"] {
        s.cmdline(t).unwrap();
    }
    assert_eq!(polylines(&s), vec![(3, true, 0.5), (3, true, 0.5)], "width and close on both");
    s.cmdline("U").unwrap();
    assert_eq!(polylines(&s), vec![(3, false, 0.5), (3, false, 0.5)], "Undo opens them again");
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
}

#[test]
fn lines_left_out_when_not_converted() {
    let mut s = Session::new();
    s.cmdline("pline 0,0 10,0 10,10").unwrap();
    s.cmdline("").unwrap();
    lines(&mut s, &[[20.0, 0.0, 30.0, 0.0]]);
    for t in ["PEDIT", "M", "ALL", "", "N", "C", ""] {
        s.cmdline(t).unwrap();
    }
    assert_eq!(polylines(&s), vec![(3, true, 0.0)], "only the polyline is edited");
    assert!(s.doc().unwrap().model.iter().any(|e| matches!(e.kind, EntityKind::Line(_))), "the line stays a line");
}

#[test]
fn objects_selected_first_go_straight_to_multiple() {
    let mut s = Session::new();
    s.cmdline("pline 0,0 10,0 10,10").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("pline 20,0 30,0 30,10").unwrap();
    s.cmdline("").unwrap();
    let all: Vec<_> = s.doc().unwrap().model.iter().map(|e| e.handle).collect();
    s.set_selection(all);
    for t in ["PEDIT", "W", "0.5", ""] {
        s.cmdline(t).unwrap();
    }
    assert_eq!(polylines(&s), vec![(3, false, 0.5), (3, false, 0.5)], "no second selection asked for");
    assert!(s.running.is_none());
}

#[test]
fn decurve_after_fit_gives_back_the_vertices() {
    let mut s = Session::new();
    s.cmdline("pline 0,0 10,0 10,10 20,10").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("pline 0,20 10,20 10,30").unwrap();
    s.cmdline("").unwrap();
    for t in ["PEDIT", "M", "ALL", "", "S"] {
        s.cmdline(t).unwrap();
    }
    assert!(polylines(&s).iter().all(|p| p.0 > 4), "Spline adds vertices: {:?}", polylines(&s));
    s.cmdline("D").unwrap();
    assert_eq!(polylines(&s), vec![(4, false, 0.0), (3, false, 0.0)], "Decurve gives back each one's vertices");
    s.cmdline("").unwrap();
}

#[test]
fn decurve_still_gives_back_the_vertices_after_a_join_that_joins_nothing_or_is_undone() {
    let mut s = Session::new();
    s.cmdline("pline 0,0 10,0 10,10 20,10").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("pline 0,20 10,20 10,30").unwrap();
    s.cmdline("").unwrap();
    // Join with nothing in reach joins nothing: Decurve still gives back the vertices.
    for t in ["PEDIT", "M", "ALL", "", "S", "J", "0", "D"] {
        s.cmdline(t).unwrap();
    }
    assert_eq!(polylines(&s), vec![(4, false, 0.0), (3, false, 0.0)], "after a Join that joined nothing");
    // Spline, a Join that joins them, Undo: Decurve gives back the vertices again.
    for t in ["S", "J", "100", "U", "D"] {
        s.cmdline(t).unwrap();
    }
    assert_eq!(polylines(&s), vec![(4, false, 0.0), (3, false, 0.0)], "after an undone Join");
    s.cmdline("").unwrap();
}

fn widths(s: &Session) -> Vec<f64> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let EntityKind::LwPolyline(p) = &e.kind { Some(p) } else { None })
        .flat_map(|p| p.vertices.iter().flat_map(|v| [v.start_width, v.end_width]))
        .collect()
}

#[test]
fn convert_asks_again_for_an_answer_that_is_not_yes_or_no() {
    let mut s = Session::new();
    lines(&mut s, &[[0.0, 0.0, 10.0, 0.0]]);
    for t in ["PEDIT", "M", "ALL", ""] {
        s.cmdline(t).unwrap();
    }
    s.cmdline("foo").unwrap();
    assert!(s.prompt_text().contains("Convert"), "{}", s.prompt_text());
    assert_eq!(polylines(&s), vec![], "nothing converted on a wrong answer");
    s.cmdline("Y").unwrap();
    assert_eq!(polylines(&s).len(), 1);
    s.cmdline("").unwrap();

    // Single-polyline PEDIT asks again too.
    lines(&mut s, &[[0.0, 5.0, 10.0, 5.0]]);
    let l = s.doc().unwrap().model.last().unwrap().handle;
    s.set_selection(vec![l]);
    s.cmdline("PEDIT").unwrap();
    s.cmdline("foo").unwrap();
    assert!(s.prompt_text().contains("turn it into one"), "{}", s.prompt_text());
    s.cmdline("N").unwrap();
    assert!(s.running.is_none());
}

#[test]
fn decurve_after_width_keeps_the_new_width() {
    let mut s = Session::new();
    for t in ["PLINE", "0,0", "W", "0", "2", "10,0", "10,10", "20,10", ""] {
        s.cmdline(t).unwrap();
    }
    assert!(widths(&s).iter().any(|w| *w > 0.0), "{:?}", widths(&s));
    for t in ["PEDIT", "M", "ALL", "", "S", "W", "0", "D", ""] {
        s.cmdline(t).unwrap();
    }
    assert!(widths(&s).iter().all(|w| *w == 0.0), "Multiple: {:?}", widths(&s));

    let mut s = Session::new();
    for t in ["PLINE", "0,0", "W", "0", "2", "10,0", "10,10", "20,10", ""] {
        s.cmdline(t).unwrap();
    }
    let h = s.doc().unwrap().model.last().unwrap().handle;
    s.set_selection(vec![h]);
    for t in ["PEDIT", "S", "W", "0", "D", ""] {
        s.cmdline(t).unwrap();
    }
    assert!(widths(&s).iter().all(|w| *w == 0.0), "single: {:?}", widths(&s));
}
