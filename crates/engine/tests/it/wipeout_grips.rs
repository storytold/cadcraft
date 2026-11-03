//! WIPEOUT's optional repeated closing point is not a second editable corner (#2256).
//! Authored contours keep their grips and edit results through DXF/DWG saves, both via the
//! command API and the interactive grip machine, without changing the file's explicit closure.

use cadcraft_doc::{Drawing, EntityKind, Handle, LwPolyline, Space};
use cadcraft_engine::{
    Input, Session,
    grips::{GripMode, stretch_grip},
};
use cadcraft_geom::{PolyVertex, Vec2};
use cadcraft_io::{SaveOptions, Version};
use serde_json::json;

fn fixture(explicit: bool, reversed: bool) -> (Drawing, Handle, Vec<Vec2>) {
    let mut points = vec![Vec2::new(1.0, 2.0), Vec2::new(7.0, 3.0), Vec2::new(4.0, 9.0)];
    if reversed {
        points.reverse();
    }
    let mut input = points.clone();
    if explicit {
        input.push(points[0]);
    }
    let mut session = Session::new();
    let input: Vec<_> = input.iter().map(|p| [p.x, p.y]).collect();
    let reply = session.execute("wipeout", &json!({"points": input})).unwrap();
    let handle = Handle::parse_hex(reply["handle"].as_str().unwrap()).unwrap();
    (session.doc().unwrap().clone(), handle, points)
}

fn points_equal(actual: &[Vec2], expected: &[Vec2]) {
    assert_eq!(actual.len(), expected.len(), "{actual:?} != {expected:?}");
    for (a, b) in actual.iter().zip(expected) {
        assert!(a.near(*b, 1e-12), "{actual:?} != {expected:?}");
    }
}

fn assert_boundary(drawing: &Drawing, handle: Handle, points: &[Vec2], explicit: bool) {
    let EntityKind::Wipeout(w) = &drawing.entity(handle).unwrap().kind else { panic!("WIPEOUT lost") };
    let mut expected = points.to_vec();
    if explicit {
        expected.push(points[0]);
    }
    points_equal(&w.boundary, &expected);
}

fn move_grip(session: &mut Session, handle: Handle, index: usize, to: Vec2, interactive: bool) {
    session.set_selection(vec![handle]);
    if interactive {
        session.start_grip_edit(&[(handle, index)]).unwrap();
        session.input(Input::Point(to)).unwrap();
        assert!(!session.prompting());
    } else {
        session.grip_edit(handle, index, to, GripMode::Stretch).unwrap();
    }
}

fn edit(drawing: Drawing, handle: Handle, points: &[Vec2], explicit: bool, index: usize, interactive: bool) -> Drawing {
    let mut session = Session::new();
    session.open_drawing(drawing, "wipeout", None);
    points_equal(&session.grips_of(handle).unwrap(), points);
    let to = points[index] + Vec2::new(2.0, 1.0);
    let mut expected = points.to_vec();
    expected[index] = to;
    move_grip(&mut session, handle, index, to, interactive);
    assert_boundary(session.doc().unwrap(), handle, &expected, explicit);
    points_equal(&session.grips_of(handle).unwrap(), &expected);
    session.execute("undo", &json!({})).unwrap();
    assert_boundary(session.doc().unwrap(), handle, points, explicit);
    session.execute("redo", &json!({})).unwrap();
    assert_boundary(session.doc().unwrap(), handle, &expected, explicit);
    session.doc().unwrap().clone()
}

fn check(format: &str, interactive: bool) {
    for explicit in [false, true] {
        for reversed in [false, true] {
            let (drawing, handle, points) = fixture(explicit, reversed);
            for index in 0..points.len() {
                edit(drawing.clone(), handle, &points, explicit, index, interactive);
            }
            for version in Version::ALL {
                for binary in [false, true].into_iter().filter(|binary| !*binary || format == "wipeout.dxf") {
                    let options = SaveOptions { version: Some(version), binary };
                    let mut current = drawing.clone();
                    for _pass in 0..2 {
                        let bytes = cadcraft_io::write_with(&current, format, &options).unwrap();
                        current = cadcraft_io::read(&bytes, format).unwrap();
                        assert!(current.read_notes.is_empty() && current.skipped_records.is_empty());
                        assert_boundary(&current, handle, &points, true);
                        for index in 0..points.len() {
                            let edited = edit(current.clone(), handle, &points, true, index, interactive);
                            let mut expected = points.clone();
                            expected[index] += Vec2::new(2.0, 1.0);
                            let bytes = cadcraft_io::write_with(&edited, format, &options).unwrap();
                            let back = cadcraft_io::read(&bytes, format).unwrap();
                            assert_boundary(&back, handle, &expected, true);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn dxf_api_grips_survive_roundtrip() {
    check("wipeout.dxf", false);
}

#[test]
fn dxf_interactive_grips_survive_roundtrip() {
    check("wipeout.dxf", true);
}

#[test]
fn dwg_api_grips_survive_roundtrip() {
    check("wipeout.dwg", false);
}

#[test]
fn dwg_interactive_grips_survive_roundtrip() {
    check("wipeout.dwg", true);
}

#[test]
fn polyline_closedness_is_not_inferred_from_coincident_endpoints() {
    for closed in [false, true] {
        for repeated in [false, true] {
            let mut points = vec![Vec2::ZERO, Vec2::new(7.0, 1.0), Vec2::new(3.0, 7.0)];
            if repeated {
                points.push(points[0]);
            }
            let polyline = LwPolyline {
                vertices: points.iter().copied().map(PolyVertex::new).collect(),
                closed,
                const_width: 0.0,
                elevation: 0.0,
                plinegen: false,
            };
            let kind = EntityKind::LwPolyline(polyline);
            let segments = if repeated || closed { 3 } else { 2 };
            assert_eq!(kind.prims().len(), segments);
            assert_eq!(kind.grips().len(), points.len() + segments);
            let to = Vec2::new(2.0, 3.0);
            let EntityKind::LwPolyline(edited) = stretch_grip(&kind, 0, to).unwrap() else { panic!("polyline") };
            assert_eq!(edited.closed, closed);
            let mut expected = points.clone();
            expected[0] = to;
            assert_eq!(edited.vertices.iter().map(|v| v.p).collect::<Vec<_>>(), expected);

            // WIPEOUT conversion requires the closed flag, even with coincident endpoints.
            let mut session = Session::new();
            let handle = session.doc_mut().unwrap().add(&Space::Model, Default::default(), kind).unwrap();
            let result = session.execute("wipeout", &json!({"handle": handle.hex()}));
            assert_eq!(result.is_ok(), closed);
            if let Ok(result) = result {
                let wipeout = Handle::parse_hex(result["handle"].as_str().unwrap()).unwrap();
                assert_boundary(session.doc().unwrap(), wipeout, &points[..3], repeated);
                points_equal(&session.grips_of(wipeout).unwrap(), &points[..3]);
            } else {
                assert_eq!(session.doc().unwrap().model.len(), 1);
            }
        }
    }
}

#[test]
fn coincident_endpoints_remain_separately_editable_after_save() {
    let original = [Vec2::ZERO, Vec2::new(8.0, 0.0), Vec2::new(8.0, 6.0), Vec2::new(0.0, 6.0)];
    for explicit in [false, true] {
        for interactive in [false, true] {
            for index in [0, 3] {
                let other = 3 - index;
                let mut input: Vec<_> = original.iter().map(|p| [p.x, p.y]).collect();
                if explicit {
                    input.push(input[0]);
                }
                let mut session = Session::new();
                let reply = session.execute("wipeout", &json!({"points": input})).unwrap();
                let handle = Handle::parse_hex(reply["handle"].as_str().unwrap()).unwrap();
                let mut coincident = original;
                coincident[index] = original[other];
                move_grip(&mut session, handle, index, coincident[index], interactive);
                points_equal(&session.grips_of(handle).unwrap(), &coincident);
                assert_boundary(session.doc().unwrap(), handle, &coincident, true);
                session.undo().unwrap();
                assert_boundary(session.doc().unwrap(), handle, &original, explicit);
                session.execute("redo", &json!({})).unwrap();
                points_equal(&session.grips_of(handle).unwrap(), &coincident);
                let collapsed = session.doc().unwrap().clone();
                for version in Version::ALL {
                    for (format, binary) in [("wipeout.dxf", false), ("wipeout.dxf", true), ("wipeout.dwg", false)] {
                        let options = SaveOptions { version: Some(version), binary };
                        let bytes = cadcraft_io::write_with(&collapsed, format, &options).unwrap();
                        let back = cadcraft_io::read(&bytes, format).unwrap();
                        session.open_drawing(back, "coincident corners", None);
                        points_equal(&session.grips_of(handle).unwrap(), &coincident);
                        assert_boundary(session.doc().unwrap(), handle, &coincident, true);
                        let mut separated = coincident;
                        separated[other] = Vec2::new(-2.0, 4.0);
                        move_grip(&mut session, handle, other, separated[other], interactive);
                        points_equal(&session.grips_of(handle).unwrap(), &separated);
                        assert_boundary(session.doc().unwrap(), handle, &separated, true);
                        let bytes = cadcraft_io::write_with(session.doc().unwrap(), format, &options).unwrap();
                        let back = cadcraft_io::read(&bytes, format).unwrap();
                        assert_boundary(&back, handle, &separated, true);
                    }
                }
            }
        }
    }
}

#[test]
fn multiple_hot_grips_keep_the_closing_point_attached() {
    let (drawing, handle, mut points) = fixture(true, false);
    let mut session = Session::new();
    session.open_drawing(drawing, "wipeout", None);
    session.set_selection(vec![handle]);
    session.start_grip_edit(&[(handle, 0), (handle, 1)]).unwrap();
    let delta = Vec2::new(2.0, 1.0);
    session.input(Input::Point(points[0] + delta)).unwrap();
    points[0] += delta;
    points[1] += delta;
    assert_boundary(session.doc().unwrap(), handle, &points, true);
    points_equal(&session.grips_of(handle).unwrap(), &points);
}
