use cadcraft_doc::{EntityKind, Handle, MLeader};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use crate::{Input, Session};

fn hd(v: &Value) -> Handle {
    Handle::parse_hex(v["handle"].as_str().unwrap_or("0")).unwrap_or(Handle(0))
}

fn ml(s: &Session, h: Handle) -> MLeader {
    match s.doc().unwrap().entity(h).map(|e| e.kind.clone()) {
        Some(EntityKind::MLeader(m)) => m,
        other => panic!("not a multileader: {other:?}"),
    }
}

fn text_ml(s: &mut Session, arrow: [f64; 2], land: [f64; 2]) -> Handle {
    hd(&s.execute("mleader", &json!({ "points": [arrow, land], "text": "N" })).unwrap())
}

#[test]
fn edit_adds_and_removes_leaders() {
    let mut s = Session::new();
    let h = text_ml(&mut s, [0.0, 0.0], [5.0, 5.0]);
    s.execute("mleaderedit", &json!({ "handle": h.hex(), "points": [[1, 9], [8, 0]] })).unwrap();
    assert_eq!(ml(&s, h).leaders.len(), 3);
    // Pick the leader near (8, 0): it is leader 2.
    s.execute("mleaderedit", &json!({ "handle": h.hex(), "mode": "remove", "points": [[7.5, 0.5]] })).unwrap();
    let m = ml(&s, h);
    assert_eq!(m.leaders.len(), 2);
    assert!(m.leaders.iter().all(|l| l[0].x != 8.0));
    assert!(s.execute("mleaderedit", &json!({ "handle": h.hex(), "mode": "remove", "index": 7 })).is_err());
    // Interactive: Add Leader then Remove Leader, one undo step each.
    s.start("mleaderedit.add").unwrap();
    s.input(Input::Pick(vec![h])).unwrap();
    s.input(Input::Point(Vec2::new(-3.0, 2.0))).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    assert_eq!(ml(&s, h).leaders.len(), 3);
    s.start("mleaderedit.remove").unwrap();
    s.input(Input::Pick(vec![h])).unwrap();
    s.input(Input::Point(Vec2::new(-3.0, 2.0))).unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(ml(&s, h).leaders.len(), 2);
}

#[test]
fn align_distribute_parallel_and_spacing() {
    let mut s = Session::new();
    let a = text_ml(&mut s, [0.0, 0.0], [0.0, 4.0]);
    let b = text_ml(&mut s, [3.0, 0.0], [3.0, 7.0]);
    let c = text_ml(&mut s, [9.0, 0.0], [9.0, 2.0]);
    // Current spacing: all landings on the horizontal line through a's landing.
    s.execute("mleaderalign", &json!({ "handles": [b.hex(), c.hex()], "to": a.hex(), "direction": [[0, 0], [1, 0]] })).unwrap();
    assert!((ml(&s, b).landing.y - 4.0).abs() < 1e-9 && (ml(&s, c).landing.y - 4.0).abs() < 1e-9);
    assert!((ml(&s, b).text.unwrap().insert.y - 4.0).abs() < 1e-6, "text moves with the landing");
    assert_eq!(ml(&s, b).leaders[0][0].y, 0.0, "arrowheads stay");
    // Specified spacing along +x.
    s.execute("mleaderalign", &json!({ "handles": [b.hex(), c.hex()], "to": a.hex(), "direction": [[0, 0], [1, 0]], "spacing": 2 })).unwrap();
    assert!((ml(&s, b).landing.x - 2.0).abs() < 1e-9 && (ml(&s, c).landing.x - 4.0).abs() < 1e-9);
    // Distribute between two points.
    s.execute("mleaderalign", &json!({ "handles": [a.hex(), b.hex(), c.hex()], "option": "distribute", "points": [[0, 10], [10, 10]] })).unwrap();
    let xs: Vec<f64> = [a, b, c].iter().map(|h| ml(&s, *h).landing.x).collect();
    assert!(xs.iter().any(|x| (x - 5.0).abs() < 1e-9) && xs.iter().any(|x| (x - 10.0).abs() < 1e-9));
    // Parallel: segments take the reference's direction.
    s.execute("mleaderalign", &json!({ "handles": [b.hex()], "to": a.hex(), "option": "parallel" })).unwrap();
    let (ra, rb) = (ml(&s, a), ml(&s, b));
    let da = (ra.landing.xy() - ra.leaders[0][0].xy()).normalized();
    let db = (rb.landing.xy() - rb.leaders[0][0].xy()).normalized();
    assert!((da.x * db.y - da.y * db.x).abs() < 1e-9);
    // Interactive.
    s.set_selection(Vec::new());
    s.start("mleaderalign").unwrap();
    s.input(Input::Pick(vec![b, c])).unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Pick(vec![a])).unwrap();
    s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    s.input(Input::Point(Vec2::new(0.0, 1.0))).unwrap();
    assert!(s.running.is_none());
    assert!((ml(&s, b).landing.x - ml(&s, a).landing.x).abs() < 1e-9);
}

#[test]
fn collect_blocks_into_one_multileader() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [2, 2]] })).unwrap();
    let l = s.doc().unwrap().model.iter().next().map(|e| e.handle).unwrap();
    s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [l.hex()], "keep": "delete" })).unwrap();
    let mk = |s: &mut Session, x: f64| hd(&s.execute("mleader", &json!({ "points": [[x, 0], [x, 5]], "block": "B" })).unwrap());
    let (a, b, c) = (mk(&mut s, 10.0), mk(&mut s, 20.0), mk(&mut s, 30.0));
    let r =
        s.execute("mleadercollect", &json!({ "handles": [a.hex(), b.hex(), c.hex()], "at": [0, 20], "mode": "horizontal", "spacing": 1 })).unwrap();
    assert_eq!(r["collected"], 3);
    let m = ml(&s, a);
    assert_eq!(m.leaders.len(), 3, "all arrowheads on the collected multileader");
    assert!(s.doc().unwrap().entity(b).is_none() && s.doc().unwrap().entity(c).is_none());
    assert_eq!(r["blocks"].as_array().map(Vec::len), Some(2));
    // Wrap needs a width.
    assert!(s.execute("mleadercollect", &json!({ "handles": [a.hex()], "at": [0, 0], "mode": "wrap" })).is_err());
}

#[test]
fn hostile_mleader_edit_params_never_panic() {
    let mut s = Session::new();
    let h = text_ml(&mut s, [0.0, 0.0], [5.0, 5.0]);
    for (id, p) in [
        ("mleaderedit", json!({ "handle": "zz", "points": [[1, 1]] })),
        ("mleaderedit", json!({ "handle": h.hex(), "mode": "remove", "index": -1 })),
        ("mleaderedit", json!({ "handle": h.hex(), "mode": "remove", "index": 1e300 })),
        ("mleaderedit", json!({ "handle": h.hex(), "points": [[1e308, f64::MAX], "x"] })),
        ("mleaderalign", json!({ "handles": [h.hex()], "to": h.hex(), "direction": [[0, 0], [0, 0]] })),
        ("mleaderalign", json!({ "handles": [], "option": "distribute", "points": [[0, 0], [1, 1]] })),
        ("mleaderalign", json!({ "handles": [h.hex()], "spacing": -1, "to": "7", "angle": 1e308 })),
        ("mleaderalign", json!({ "handles": [h.hex(), "q"], "option": "nonsense" })),
        ("mleadercollect", json!({ "handles": [h.hex()], "at": [0, 0] })),
        ("mleadercollect", json!({ "handles": [h.hex(), h.hex()], "at": [1e308, 1e308], "mode": "wrap", "width": 0 })),
    ] {
        let _ = s.execute(id, &p);
    }
    let _ = cadcraft_render::build(s.doc().unwrap(), &cadcraft_doc::Space::Model, &cadcraft_render::Options::default());
}

mod units {
    use super::super::{Spacing, align_directional, align_distribute, align_parallel};
    use cadcraft_doc::{Handle, MLeader};
    use cadcraft_geom::{Vec2, Vec3};

    fn unit_ml(arrow: [f64; 2], land: [f64; 2]) -> MLeader {
        MLeader {
            leaders: vec![vec![Vec3::new(arrow[0], arrow[1], 0.0)]],
            landing: Vec3::new(land[0], land[1], 0.0),
            dogleg: 0.0,
            text: None,
            style: String::new(),
            arrow_size: 1.0,
            spline: false,
            block: None,
        }
    }

    fn no_leader(land: [f64; 2]) -> MLeader {
        MLeader {
            leaders: vec![],
            landing: Vec3::new(land[0], land[1], 0.0),
            dogleg: 0.0,
            text: None,
            style: String::new(),
            arrow_size: 1.0,
            spline: false,
            block: None,
        }
    }

    /// A five-unit-long leader whose direction differs from the reference's `+y`: moving the
    /// arrow onto `y` plus `5` (a five-unit parallel push) and back along the landing gives
    /// `(-3, 1)`.
    #[test]
    fn parallel_requires_a_reference_and_skips_leaders() {
        assert!(align_parallel(&[(Handle(1), unit_ml([0.0, 0.0], [3.0, 4.0]))], None).is_err());
        let d = align_parallel(
            &[(Handle(2), no_leader([1.0, 1.0])), (Handle(3), unit_ml([0.0, 0.0], [3.0, 4.0]))],
            Some(&unit_ml([0.0, 0.0], [0.0, 4.0])),
        )
        .unwrap();
        assert_eq!(d.len(), 1, "a multileader without a leader line is skipped");
        assert_eq!(d[0].0, Handle(3));
        assert!((d[0].1.x + 3.0).abs() < 1e-9);
        assert!((d[0].1.y - 1.0).abs() < 1e-9);
    }

    #[test]
    fn distribute_orders_landings_along_the_line() {
        let mk = |h: u64, land: [f64; 2]| (Handle(h), unit_ml([0.0, 0.0], land));
        assert!(align_distribute(vec![mk(1, [2.0, 0.0])], None).is_err(), "needs two points");
        assert!(align_distribute(Vec::new(), Some((Vec2::new(0.0, 0.0), Vec2::new(0.0, 0.0)))).is_err(), "coinciding points give no direction");
        let items = vec![mk(1, [2.0, 0.0]), mk(2, [0.0, 0.0])];
        let d = align_distribute(items, Some((Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0)))).unwrap();
        assert_eq!(d[0].0, Handle(2), "sorted by projection onto the line");
        assert!((d[0].1.x - 0.0).abs() < 1e-9 && (d[0].1.y - 0.0).abs() < 1e-9, "first fraction is 0");
        assert_eq!(d[1].0, Handle(1));
        assert!((d[1].1.x - 2.0).abs() < 1e-9 && (d[1].1.y - 0.0).abs() < 1e-9, "last fraction is 1");
    }

    #[test]
    fn directional_projects_landings_and_supports_spacing() {
        let line = Some((Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0)));
        assert!(align_directional(&[], None, None, Spacing::Current).is_err(), "needs a direction");
        assert!(
            align_directional(&[], None, Some((Vec2::new(0.0, 0.0), Vec2::new(0.0, 0.0))), Spacing::Current).is_err(),
            "a zero direction is an error"
        );
        // Without a reference the origin is the first point: the landing is projected onto it.
        let d = align_directional(&[(Handle(7), unit_ml([0.0, 0.0], [1.0, 5.0]))], None, line, Spacing::Current).unwrap();
        assert_eq!(d.len(), 1);
        assert!((d[0].1.x - 0.0).abs() < 1e-9);
        assert!((d[0].1.y + 5.0).abs() < 1e-9);
        // With a reference as origin, given spacing counts outward on both sides of it.
        let items = vec![(Handle(1), unit_ml([0.0, 0.0], [4.0, 0.0])), (Handle(2), unit_ml([0.0, 0.0], [-4.0, 0.0]))];
        let d = align_directional(&items, Some(&unit_ml([0.0, 0.0], [0.0, 4.0])), line, Spacing::Value(2.0)).unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].0, Handle(1));
        assert!((d[0].1.x + 2.0).abs() < 1e-9 && (d[0].1.y - 4.0).abs() < 1e-9);
        assert_eq!(d[1].0, Handle(2));
        assert!((d[1].1.x - 2.0).abs() < 1e-9 && (d[1].1.y - 4.0).abs() < 1e-9);
    }
}
