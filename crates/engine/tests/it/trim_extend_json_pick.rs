//! JSON execute pick roles and diagnostic-only refusals (#2750).

use cadcraft_doc::{Common, EntityKind, Handle, Line, Space};
use cadcraft_engine::{Session, find_command};
use cadcraft_geom::Vec3;
use serde_json::{Value, json};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn fixture(end: f64, edge_x: f64) -> TestResult<(Session, Handle, Handle)> {
    let mut s = Session::new();
    let a = s.doc_mut()?.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(end, 0.0, 0.0) }))?;
    let b = s.doc_mut()?.add(
        &Space::Model,
        Common::default(),
        EntityKind::Line(Line { a: Vec3::new(edge_x, -20.0, 0.0), b: Vec3::new(edge_x, 20.0, 0.0) }),
    )?;
    Ok((s, a, b))
}

fn assert_line(s: &Session, h: Handle, start: f64, end: f64) -> TestResult {
    let e = s.doc()?.entity(h).ok_or("line missing")?;
    let EntityKind::Line(l) = &e.kind else { return Err("expected line".into()) };
    assert_eq!((l.a, l.b), (Vec3::new(start, 0.0, 0.0), Vec3::new(end, 0.0, 0.0)));
    Ok(())
}

fn failure(s: &mut Session, cmd: &str, params: Value, message: &str) -> TestResult {
    let before = s.doc()?.clone();
    let selection = s.state()?.selection.clone();
    let error = s.execute(cmd, &params).err().ok_or("expected refusal")?;
    assert_eq!(error.to_string(), message);
    assert_eq!(s.doc()?, &before, "a failed JSON call must leave the whole drawing unchanged");
    assert_eq!(s.state()?.selection, selection);
    Ok(())
}

#[test]
fn trim_execute_pick_selects_the_portion_to_remove() -> TestResult {
    // Off-object picks that already succeed by projection remain accepted.
    for (pick, start, end) in [([5, 0], 25.0, 50.0), ([40, 0], 0.0, 25.0), ([5, 3], 25.0, 50.0)] {
        let (mut s, a, b) = fixture(50.0, 25.0)?;
        let boundary = s.doc()?.entity(b).cloned().ok_or("boundary missing")?;
        let result = s.execute("trim", &json!({"handle": a.hex(), "edges": [b.hex()], "pick": pick}))?;
        let handles = result["handles"].as_array().ok_or("expected handles")?;
        assert_eq!(handles.len(), 1);
        let h = handles.first().and_then(Value::as_str).and_then(Handle::parse_hex).ok_or("expected trimmed handle")?;
        assert_eq!(h, a, "TRIM keeps the original handle on the surviving piece");
        assert_line(&s, h, start, end)?;
        assert_eq!(s.doc()?.entity(b), Some(&boundary));
    }
    Ok(())
}

#[test]
fn extend_execute_pick_selects_the_end_toward_the_boundary() -> TestResult {
    for pick in [[25, 10], [25, -15], [19, 0], [40, 0]] {
        let (mut s, a, b) = fixture(20.0, 25.0)?;
        let boundary = s.doc()?.entity(b).cloned().ok_or("boundary missing")?;
        let result = s.execute("extend", &json!({"handle": a.hex(), "edges": [b.hex()], "pick": pick}))?;
        assert_eq!(result, Value::Null);
        assert_line(&s, a, 0.0, 25.0)?;
        assert_eq!(s.doc()?.entity(b), Some(&boundary));
    }
    Ok(())
}

#[test]
fn trim_execute_distinguishes_picks_at_or_projecting_onto_a_cut_from_no_cut() -> TestResult {
    let (mut s, a, b) = fixture(50.0, 25.0)?;
    for (pick, message) in [
        ([25, 10], "The `pick` point is off the line and projects onto a cutting intersection; pick away from that intersection."),
        ([25, 0], "The `pick` point is at a cutting intersection; pick a portion of the object away from that intersection."),
    ] {
        failure(&mut s, "trim", json!({"handle": a.hex(), "edges": [b.hex()], "pick": pick, "mode": "standard"}), message)?;
    }
    let (mut s, a, b) = fixture(50.0, 75.0)?;
    failure(
        &mut s,
        "trim",
        json!({"handle": a.hex(), "edges": [b.hex()], "pick": [5, 0]}),
        "No usable cutting-edge intersection in the interior of the object to trim.",
    )
}

#[test]
fn extend_execute_distinguishes_the_wrong_end_from_no_boundary_hit() -> TestResult {
    let (mut s, a, b) = fixture(20.0, 25.0)?;
    failure(
        &mut s,
        "extend",
        json!({"handle": a.hex(), "edges": [b.hex()], "pick": [10, 0]}),
        "The `pick` point selects an end with no boundary ahead; choose `pick` toward the boundary to extend to.",
    )?;
    s.doc_mut()?.modify_entity(b, |e| {
        e.kind = EntityKind::Line(Line { a: Vec3::new(0.0, 20.0, 0.0), b: Vec3::new(25.0, 20.0, 0.0) });
    })?;
    failure(
        &mut s,
        "extend",
        json!({"handle": a.hex(), "edges": [b.hex()], "pick": [25, 20]}),
        "No usable boundary intersection was found for either end of the object to extend.",
    )
}

#[test]
fn quick_erasure_and_unrelated_errors_keep_their_behavior() -> TestResult {
    let (mut s, a, b) = fixture(50.0, 25.0)?;
    // Quick mode leaves a line picked on its only cut in place (#2648), and the JSON form explains
    // the refusal as Standard mode does.
    failure(
        &mut s,
        "trim",
        json!({"handle": a.hex(), "edges": [b.hex()], "pick": [25, 0], "mode": "quick"}),
        "The `pick` point is at a cutting intersection; pick a portion of the object away from that intersection.",
    )?;
    assert!(s.doc()?.entity(a).is_some());
    assert!(s.doc()?.entity(b).is_some());
    failure(&mut s, "trim", json!({"handle": "DEAD", "pick": [5, 0]}), "no such object")
}

#[test]
fn command_contracts_and_missing_pick_errors_explain_the_roles() -> TestResult {
    let trim = find_command("trim").ok_or("trim not registered")?;
    assert!(trim.params.contains("pick selects the portion of that object to remove"));
    assert!(trim.params.contains("edges are the cutting edges"));
    let extend = find_command("extend").ok_or("extend not registered")?;
    assert!(extend.params.contains("pick points toward the boundary to extend to"));
    assert!(extend.params.contains("selecting the nearer end"));
    assert!(extend.params.contains("edges are the boundary edges"));
    let (mut s, a, b) = fixture(20.0, 25.0)?;
    failure(
        &mut s,
        "trim",
        json!({"handle": a.hex(), "edges": [b.hex()]}),
        "invalid parameters for `trim`: `pick` ([x, y]) is required: select the portion of the object to trim away",
    )?;
    failure(
        &mut s,
        "extend",
        json!({"handle": a.hex(), "edges": [b.hex()]}),
        "invalid parameters for `extend`: `pick` ([x, y]) is required: point toward the boundary to extend to, selecting the nearer end",
    )
}

#[test]
fn a_pick_on_the_line_far_from_the_origin_is_not_called_off_the_line() -> TestResult {
    // Survey coordinates: rounding there is far above 1e-9 (the pick below is 3.7e-9 off the line
    // after rounding), so a pick on the line still reads as on it.
    let (x0, y0, k) = (123_456_789.1, 9_876_543.3, 33.3333);
    let mut s = Session::new();
    let a = s.doc_mut()?.add(
        &Space::Model,
        Common::default(),
        EntityKind::Line(Line { a: Vec3::new(x0, y0, 0.0), b: Vec3::new(x0 + 50.0, y0 + 30.0, 0.0) }),
    )?;
    let b = s.doc_mut()?.add(
        &Space::Model,
        Common::default(),
        EntityKind::Line(Line { a: Vec3::new(x0 + k, y0 - 20.0, 0.0), b: Vec3::new(x0 + k, y0 + 50.0, 0.0) }),
    )?;
    failure(
        &mut s,
        "trim",
        json!({"handle": a.hex(), "edges": [b.hex()], "pick": [x0 + k, y0 + k * 0.6]}),
        "The `pick` point is at a cutting intersection; pick a portion of the object away from that intersection.",
    )
}

#[test]
fn the_extend_contract_names_the_polyline_end_rule() -> TestResult {
    // An open polyline extends the end on the pick's side of its middle parameter (#2693), not
    // the nearer end.
    let extend = find_command("extend").ok_or("extend not registered")?;
    assert!(extend.params.contains("middle"), "{}", extend.params);
    Ok(())
}
