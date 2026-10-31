use cadcraft_doc::EntityKind;
use cadcraft_geom::{Arc, Vec2};
use serde_json::json;

use crate::{Input, Session};

fn last(s: &Session) -> EntityKind {
    s.doc().unwrap().model.last().unwrap().kind.clone()
}

fn kinds(s: &Session) -> Vec<&'static str> {
    s.doc().unwrap().model.iter().map(|e| e.kind.type_name()).collect()
}

fn as_arc(k: &EntityKind) -> Arc {
    match k {
        EntityKind::Arc(a) => Arc::new(a.center.xy(), a.radius, a.start, a.end),
        other => panic!("not an arc: {other:?}"),
    }
}

fn as_circle(k: &EntityKind) -> (Vec2, f64) {
    match k {
        EntityKind::Circle(c) => (c.center.xy(), c.radius),
        other => panic!("not a circle: {other:?}"),
    }
}

fn h(r: &serde_json::Value) -> String {
    r["handle"].as_str().unwrap().to_string()
}

fn near(a: Vec2, x: f64, y: f64) -> bool {
    a.near(Vec2::new(x, y), 1e-6)
}

// ---------------- ARC ----------------

#[test]
fn arc_variants_json() {
    let mut s = Session::new();
    let quarter =
        |a: &Arc| near(a.center, 0.0, 0.0) && (a.radius - 1.0).abs() < 1e-9 && near(a.start_point(), 1.0, 0.0) && near(a.end_point(), 0.0, 1.0);
    for (id, p) in [
        ("arc.sce", json!({"start": [1, 0], "center": [0, 0], "end": [0, 5]})),
        ("arc.sca", json!({"start": [1, 0], "center": [0, 0], "angle": 90})),
        ("arc.scl", json!({"start": [1, 0], "center": [0, 0], "length": 2f64.sqrt()})),
        ("arc.sea", json!({"start": [1, 0], "end": [0, 1], "angle": 90})),
        ("arc.sed", json!({"start": [1, 0], "end": [0, 1], "direction": 90})),
        ("arc.ser", json!({"start": [1, 0], "end": [0, 1], "radius": 1})),
        ("arc.cse", json!({"center": [0, 0], "start": [1, 0], "end": [0, 3]})),
        ("arc.csa", json!({"center": [0, 0], "start": [1, 0], "angle": 90})),
        ("arc.csl", json!({"center": [0, 0], "start": [1, 0], "length": 2f64.sqrt()})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert!(quarter(&as_arc(&last(&s))), "{id}: {:?}", last(&s));
    }
    // Negative angle: clockwise from the start.
    s.execute("arc.sca", &json!({"start": [1, 0], "center": [0, 0], "angle": -90})).unwrap();
    let a = as_arc(&last(&s));
    assert!(near(a.start_point(), 0.0, -1.0) && near(a.end_point(), 1.0, 0.0));
    // Negative radius / chord: the major arc.
    s.execute("arc.ser", &json!({"start": [1, 0], "end": [0, 1], "radius": -1})).unwrap();
    assert!((as_arc(&last(&s)).sweep() - 1.5 * std::f64::consts::PI).abs() < 1e-9);
    s.execute("arc.scl", &json!({"start": [1, 0], "center": [0, 0], "length": -(2f64.sqrt())})).unwrap();
    assert!((as_arc(&last(&s)).sweep() - 1.5 * std::f64::consts::PI).abs() < 1e-9);
    // Direction as a vector, and bad input.
    s.execute("arc.sed", &json!({"start": [1, 0], "end": [0, 1], "direction": [0, 1]})).unwrap();
    assert!(quarter(&as_arc(&last(&s))));
    assert!(s.execute("arc.ser", &json!({"start": [0, 0], "end": [10, 0], "radius": 1})).is_err());
    assert!(s.execute("arc.scl", &json!({"start": [1, 0], "center": [0, 0], "length": 5})).is_err());
    assert!(s.execute("arc.sea", &json!({"start": [1, 0], "end": [1, 0], "angle": 90})).is_err());
}

#[test]
fn arc_variants_command_line() {
    for (cmd, expect_quarter) in [
        ("arc.sce 1,0 0,0 0,2", true),
        ("arc.sca 1,0 0,0 90", true),
        ("arc.scl 1,0 0,0 1.41421356237", true),
        ("arc.sea 1,0 0,1 90", true),
        ("arc.sed 1,0 0,1 90", true),
        ("arc.ser 1,0 0,1 1", true),
        ("arc.cse 0,0 1,0 0,4", true),
        ("arc.csa 0,0 1,0 90", true),
        ("arc.csl 0,0 1,0 1.41421356237", true),
        // Angle/direction given by a point.
        ("arc.sed 1,0 0,1 1,5", true),
        ("arc.csa 0,0 1,0 0,7", true),
    ] {
        let mut s = Session::new();
        s.cmdline(cmd).unwrap();
        assert!(s.running.is_none(), "{cmd} still running: {}", s.prompt_text());
        let a = as_arc(&last(&s));
        if expect_quarter {
            assert!(near(a.center, 0.0, 0.0) && near(a.start_point(), 1.0, 0.0) && near(a.end_point(), 0.0, 1.0), "{cmd}: {a:?}");
        }
        assert_eq!(s.state().unwrap().undo.len(), 1, "{cmd}: one undo step");
    }
}

#[test]
fn arc_continue_from_last_line() {
    let mut s = Session::new();
    // Nothing to continue from: the command ends politely.
    s.cmdline("arc.continue").unwrap();
    assert!(s.running.is_none());
    assert!(s.execute("arc.continue", &json!({"end": [2, 1]})).is_err());
    s.cmdline("line 0,0 1,0").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("arc.continue 2,1").unwrap();
    let a = as_arc(&last(&s));
    assert!(near(a.center, 1.0, 1.0), "{a:?}");
    // JSON continues from that arc's end (2,1) heading +Y.
    s.execute("arc.continue", &json!({"end": [1, 2]})).unwrap();
    let b = as_arc(&last(&s));
    assert!(near(b.center, 1.0, 1.0), "{b:?}");
    assert!(s.execute("arc.continue", &json!({"end": [-5, 2]})).is_err(), "end on the tangent line");
    // Explicit start and direction.
    s.execute("arc.continue", &json!({"start": [0, 0], "direction": 0, "end": [1, 1]})).unwrap();
    assert!(near(as_arc(&last(&s)).center, 0.0, 1.0));
}

#[test]
fn arc_continue_from_last_picked_end_of_three_point_arc() {
    // (picked points, end of the continuation, its center): both arcs leave their last
    // picked point heading -Y, so the continuation is a quarter turn on that side.
    let cases = [
        // Clockwise: stored CCW, the last picked point (2,0) is the arc's start.
        ("arc 0,0 1,1 2,0", "3,-1", (3.0, 0.0)),
        // Counter-clockwise: the last picked point (0,0) is the arc's end.
        ("arc 2,0 1,1 0,0", "-1,-1", (-1.0, 0.0)),
    ];
    for (draw, end, center) in cases {
        let mut s = Session::new();
        s.cmdline(draw).unwrap();
        s.cmdline("arc").unwrap();
        s.cmdline("").unwrap();
        s.cmdline(end).unwrap();
        let a = as_arc(&last(&s));
        assert!(near(a.center, center.0, center.1), "{draw}: {a:?}");
        assert!((a.sweep() - std::f64::consts::FRAC_PI_2).abs() < 1e-6, "{draw}: reversed tangent {a:?}");
    }
}

// ---------------- CIRCLE ----------------

#[test]
fn circle_variants_json_and_command_line() {
    let mut s = Session::new();
    s.execute("circle.cd", &json!({"center": [1, 1], "diameter": 4})).unwrap();
    assert_eq!(as_circle(&last(&s)).1, 2.0);
    s.execute("circle.2p", &json!({"p1": [0, 0], "p2": [6, 0]})).unwrap();
    let (c, r) = as_circle(&last(&s));
    assert!(near(c, 3.0, 0.0) && (r - 3.0).abs() < 1e-9);
    s.execute("circle.3p", &json!({"p1": [1, 0], "p2": [0, 1], "p3": [-1, 0]})).unwrap();
    let (c, r) = as_circle(&last(&s));
    assert!(near(c, 0.0, 0.0) && (r - 1.0).abs() < 1e-9);
    assert!(s.execute("circle.3p", &json!({"p1": [0, 0], "p2": [1, 0], "p3": [2, 0]})).is_err());
    assert!(s.execute("circle.cd", &json!({"center": [0, 0], "diameter": 0})).is_err());

    s.cmdline("circle.cd 5,5 3").unwrap();
    assert_eq!(as_circle(&last(&s)).1, 1.5);
    s.cmdline("circle.cd 5,5 5,9").unwrap();
    assert_eq!(as_circle(&last(&s)).1, 2.0);
    s.cmdline("circle.2p 0,0 0,4").unwrap();
    assert!(near(as_circle(&last(&s)).0, 0.0, 2.0));
    s.cmdline("circle.3p 0,0 2,2 4,0").unwrap();
    assert!(near(as_circle(&last(&s)).0, 2.0, 0.0));
    assert!(s.running.is_none());
}

fn line_h(s: &mut Session, pts: serde_json::Value) -> String {
    s.execute("line", &json!({ "points": pts })).unwrap()["handles"][0].as_str().unwrap().to_string()
}

fn corner(s: &mut Session) -> (String, String) {
    (line_h(s, json!([[0, 0], [10, 0]])), line_h(s, json!([[0, 0], [0, 10]])))
}

#[test]
fn circle_ttr_json_and_command_line() {
    let mut s = Session::new();
    let (a, b) = corner(&mut s);
    s.execute("circle.ttr", &json!({"h1": a, "p1": [3, 0], "h2": b, "p2": [0, 3], "radius": 1})).unwrap();
    let (c, r) = as_circle(&last(&s));
    assert!(near(c, 1.0, 1.0) && (r - 1.0).abs() < 1e-9);
    // Circle-circle and line-circle.
    let k = h(&s.execute("circle", &json!({"center": [20, 0], "radius": 2})).unwrap());
    let k2 = h(&s.execute("circle", &json!({"center": [26, 0], "radius": 2})).unwrap());
    s.execute("circle.ttr", &json!({"h1": k, "p1": [21, 1], "h2": k2, "p2": [25, 1], "radius": 2})).unwrap();
    let (c, r) = as_circle(&last(&s));
    assert!((c.x - 23.0).abs() < 1e-9 && c.y > 0.0 && (r - 2.0).abs() < 1e-9, "{c:?}");
    assert!(s.execute("circle.ttr", &json!({"h1": k, "p1": [21, 1], "h2": k2, "p2": [25, 1], "radius": 0.1})).is_err());
    // Command line: picks, then the radius.
    s.cmdline("circle.ttr 5,0 0,5 2").unwrap();
    let (c, r) = as_circle(&last(&s));
    assert!(near(c, 2.0, 2.0) && (r - 2.0).abs() < 1e-9, "{c:?} {r}");
    // CIRCLE's Ttr option.
    s.cmdline("circle ttr 6,0 0,6 0.5").unwrap();
    assert!(s.running.is_none());
    let (c, r) = as_circle(&last(&s));
    assert!(near(c, 0.5, 0.5) && (r - 0.5).abs() < 1e-9);
}

#[test]
fn circle_ttt_json_and_command_line() {
    let mut s = Session::new();
    let (a, b) = corner(&mut s);
    let hyp = s.execute("line", &json!({"points": [[4, 0], [0, 3]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    s.execute("circle.ttt", &json!({"h1": a, "p1": [2, 0], "h2": b, "p2": [0, 1], "h3": hyp, "p3": [2, 1.5]})).unwrap();
    let (c, r) = as_circle(&last(&s));
    assert!(near(c, 1.0, 1.0) && (r - 1.0).abs() < 1e-6, "{c:?} {r}");
    s.cmdline("circle.ttt 2,0 0,1 2,1.5").unwrap();
    assert!(s.running.is_none());
    let (c, _) = as_circle(&last(&s));
    assert!(near(c, 1.0, 1.0));
    // Three circles: the small circle in the middle of three touching circles.
    let mut s = Session::new();
    let r = 1.0;
    let cs: Vec<String> =
        [[0.0, 0.0], [2.0, 0.0], [1.0, 3f64.sqrt()]].iter().map(|c| h(&s.execute("circle", &json!({"center": c, "radius": r})).unwrap())).collect();
    let centroid = Vec2::new(1.0, 3f64.sqrt() / 3.0);
    let picks: Vec<Vec2> = [[0.0, 0.0], [2.0, 0.0], [1.0, 3f64.sqrt()]]
        .iter()
        .map(|c| Vec2::new(c[0], c[1]) + (centroid - Vec2::new(c[0], c[1])).normalized())
        .collect();
    s.execute(
        "circle.ttt",
        &json!({"h1": cs[0], "p1": [picks[0].x, picks[0].y], "h2": cs[1], "p2": [picks[1].x, picks[1].y], "h3": cs[2], "p3": [picks[2].x, picks[2].y]}),
    )
    .unwrap();
    let (c, rr) = as_circle(&last(&s));
    assert!(c.near(centroid, 1e-6) && (rr - (2.0 / 3f64.sqrt() - 1.0)).abs() < 1e-6, "{c:?} {rr}");
}

// ---------------- ELLIPSE ----------------

#[test]
fn ellipse_axis_and_arc() {
    let mut s = Session::new();
    s.execute("ellipse.axis", &json!({"p1": [0, 0], "p2": [4, 0], "distance": 1})).unwrap();
    match last(&s) {
        EntityKind::Ellipse(e) => {
            assert!(near(e.center.xy(), 2.0, 0.0) && (e.ratio - 0.5).abs() < 1e-9);
        }
        k => panic!("{k:?}"),
    }
    s.execute("ellipse.arc", &json!({"center": [0, 0], "major": [2, 0], "ratio": 0.5, "start": 0, "end": 90})).unwrap();
    match last(&s) {
        EntityKind::Ellipse(e) => assert!((e.end - std::f64::consts::FRAC_PI_2).abs() < 1e-9 && e.start.abs() < 1e-9),
        k => panic!("{k:?}"),
    }
    assert!(s.execute("ellipse.arc", &json!({"p1": [0, 0], "p2": [4, 0], "distance": 1})).is_err());
    s.cmdline("ellipse.axis 0,0 6,0 1").unwrap();
    assert!(matches!(last(&s), EntityKind::Ellipse(e) if (e.ratio - 1.0 / 3.0).abs() < 1e-9));
    s.cmdline("ellipse.arc 0,0 4,0 1 0 90").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(matches!(last(&s), EntityKind::Ellipse(e) if (e.end - e.start).abs() > 0.1 && (e.end - e.start).abs() < 6.0));
}

#[test]
fn ellipse_arc_typed_angles_are_true_angles() {
    let want = 2f64.atan2(1.0);
    // Horizontal and rotated major axis, ratio 0.5: typed 0 and 45 degrees match the JSON form.
    for line in ["ellipse.arc 0,0 10,0 2.5 0 45", "ellipse.arc 5,-5 5,5 2.5 0 45"] {
        let mut s = Session::new();
        s.cmdline(line).unwrap();
        assert!(matches!(last(&s), EntityKind::Ellipse(e) if e.start.abs() < 1e-9 && (e.end - want).abs() < 1e-9), "{line}");
    }
}

/// Typed ELLIPSE input against what AutoCAD 2024 stores for the same input (black-box: synthetic
/// input in accoreconsole, read back with `entget`). The major axis points at the first axis
/// endpoint; a point at the distance prompt gives its distance from the center; an other axis
/// longer than the first becomes the major axis, the first turned 90° counterclockwise; the
/// Included angle is added to the start angle.
#[test]
fn ellipse_axis_endpoints_match_autocad() {
    type Case = (&'static str, [f64; 2], [f64; 2], f64, Option<[f64; 2]>);
    // (input after ELLIPSE, center, major axis, ratio, arc start/end parameters)
    let cases: [Case; 10] = [
        // The other axis (27.02) is longer than the first half axis (26.42).
        (
            "16.459468652708743,28.120073391143574 -5.034438825478263,-20.14843889196848 27.021868057769268",
            [5.712514913615239, 3.985817249587547],
            [-24.6850622596004, 10.99222700689644],
            0.9776866627982613,
            None,
        ),
        (
            "33.45015752893423,-27.997665254733153 1.0902141405158297,-9.876490849047837 _R 36.361957504949",
            [17.27018583472502, -18.93707805189049],
            [16.1799716942092, -9.060587202842658],
            0.8052876303433125,
            None,
        ),
        (
            "0.5161247992610992,0.27380397971284964 0.49874786589184156,0.23294359917255586 0.0026647279687818256",
            [0.5074363325764704, 0.2533737894427027],
            [0.00868846668462886, 0.02043019027014691],
            0.1200276615332929,
            None,
        ),
        // Arcs: the angles are taken from the major axis, so a reversed axis draws the arc
        // on the opposite side of the center.
        (
            "_A 612354.66928411,2398796.595432055 612364.9910379789,2398751.70959102 3.5361631006792016 259.40675054209663 162.0895061623204",
            [612359.8301610444, 2398774.152511537],
            [-5.160876934416592, 22.44292051764205],
            0.1535548572193783,
            Some([4.683678586016272, 2.014340486820176]),
        ),
        (
            "_A -32.49789607178266,25.826299666765127 16.360636510348964,-6.670894598441254 9.326202777586586 _P 235.35128996877393 341.0349294763654",
            [-8.068629780716847, 9.577702534161936],
            [-24.42926629106581, 16.24859713260318],
            0.317871961092373,
            Some([4.107654908771007, 5.952182383669234]),
        ),
        (
            "_A _C 15.895131938563559,-16.135236520373613 1.9672122380661037,-18.61293795336168 14.456880177932417 214.65903577683966 _I 232.7163597705933",
            [15.89513193856356, -16.13523652037361],
            [2.53204749973717, -14.23341561039267],
            0.9785367111972643,
            Some([3.756694588158375, 1.5259701966906878]),
        ),
        (
            "_A _C 1.8822645373261935,-7.920292708078534 28.89115893105938,-23.280023918605742 38.7551404612689 136.20071263173944 _I 129.90755425686075",
            [1.882264537326193, -7.920292708078534],
            [19.15838502833398, 33.68853210335513],
            0.8017236937148501,
            Some([2.267136066480607, 4.657903136501946]),
        ),
        // The other axis given by a point.
        (
            "-16.377480557612035,55.44699770062272 -15.579405511585826,-24.406726331471475 -4.27081078679767,42.69814401696656",
            [-15.97844303459893, 15.52013568457562],
            [-0.3990375230131046, 39.92686201604709],
            0.7411292927991227,
            None,
        ),
        (
            "8.137326847228081,16.045209411091605 -2.4500932561568627,22.585405165312338 -2.4956573993017335,12.374292543621266",
            [2.843616795535609, 19.31530728820197],
            [4.602217644463162, 7.450176331143755],
            0.7105482899194384,
            None,
        ),
        (
            "_C 15.418189586495892,-18.741133769301225 43.29367724019212,5.337090633133826 12.043993630141971,-7.0535727363182446",
            [15.41818958649589, -18.74113376930122],
            [27.87548765369622, 24.07822440243505],
            0.3302549104883693,
            None,
        ),
    ];
    use std::f64::consts::{PI, TAU};
    let same = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
    let same_angle = |a: f64, b: f64| {
        let d = (a - b).rem_euclid(TAU);
        d < 1e-9 || TAU - d < 1e-9
    };
    for (input, center, major, ratio, arc) in cases {
        let mut s = Session::new();
        s.cmdline(&format!("ellipse {input}")).unwrap();
        assert!(s.running.is_none(), "{input}: {}", s.prompt_text());
        let EntityKind::Ellipse(e) = last(&s) else { panic!("{input}: no ellipse") };
        assert!(same(e.center.x, center[0]) && same(e.center.y, center[1]), "{input}: center {:?}", e.center);
        assert!(same(e.major.x, major[0]) && same(e.major.y, major[1]), "{input}: major {:?}", e.major);
        assert!(same(e.ratio, ratio), "{input}: ratio {}", e.ratio);
        let [start, end] = arc.unwrap_or([0.0, TAU]);
        assert!(same_angle(e.start, start) && same_angle(e.end, end), "{input}: {} {}", e.start, e.end);
    }
    // Enter takes the default included angle, 180°: half the ellipse.
    let mut s = Session::new();
    s.cmdline("ellipse _A 10,0 -10,0 5 30 _I").unwrap();
    assert!(s.prompt_text().contains("included angle"), "{}", s.prompt_text());
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let EntityKind::Ellipse(e) = last(&s) else { panic!("no ellipse") };
    assert!(same_angle(e.end, e.start + PI), "{} {}", e.start, e.end);
}

// ---------------- DIVIDE / MEASURE ----------------

#[test]
fn divide_and_measure_json() {
    let mut s = Session::new();
    let l = s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    let r = s.execute("divide", &json!({"handle": l, "segments": 5})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 4);
    let pts: Vec<Vec2> =
        s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Point(p) = &e.kind { Some(p.p.xy()) } else { None }).collect();
    assert!(near(pts[0], 2.0, 0.0) && near(pts[3], 8.0, 0.0));
    let r = s.execute("measure", &json!({"handle": l, "length": 3})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 3);
    // From the other end.
    s.execute("measure", &json!({"handle": l, "length": 4, "from": [10, 0]})).unwrap();
    assert!(matches!(last(&s), EntityKind::Point(p) if near(p.p.xy(), 2.0, 0.0)));
    // A circle divides into n points including the start.
    let c = h(&s.execute("circle", &json!({"center": [20, 0], "radius": 1})).unwrap());
    assert_eq!(s.execute("divide", &json!({"handle": c, "segments": 6})).unwrap()["handles"].as_array().unwrap().len(), 6);
    // Blocks, aligned with the object.
    let tick = s.execute("line", &json!({"points": [[0, -0.1], [0, 0.1]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    s.execute("block", &json!({"name": "TICK", "base": [0, 0], "handles": [tick], "keep": "delete"})).unwrap();
    let arc = h(&s.execute("arc", &json!({"center": [0, 20], "radius": 2, "start": 0, "end": 180})).unwrap());
    let r = s.execute("divide", &json!({"handle": arc, "segments": 2, "block": "TICK"})).unwrap();
    let ins = s.doc().unwrap().entity(cadcraft_doc::Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap()).unwrap().kind.clone();
    assert!(matches!(ins, EntityKind::Insert(i) if near(i.insert.xy(), 0.0, 22.0) && (i.rotation - std::f64::consts::PI).abs() < 1e-6));
    assert!(s.execute("divide", &json!({"handle": l, "segments": 3, "block": "NOPE"})).is_err());
    assert!(s.execute("divide", &json!({"handle": l, "segments": 1})).is_err());
    assert!(s.execute("measure", &json!({"handle": l, "length": 0})).is_err());
    assert!(s.execute("measure", &json!({"handle": l, "length": 1e-300})).is_err());
}

/// DIVIDE and MEASURE against the points AutoCAD 2024 places for the same object and pick
/// (black-box: synthetic objects in accoreconsole, `(command "_.MEASURE" (list ename point) …)`,
/// every POINT read back with `entget`). The pick chooses the end by the curve's parameter, not
/// by distance; a closed polyline runs from its first vertex, a circle from angle 0.
#[test]
fn divide_and_measure_match_autocad() {
    type Case = ((&'static str, serde_json::Value), &'static str, f64, [f64; 2], usize, &'static [[f64; 2]]);
    // ((seed command, params), command, segments or length, pick, POINT count, AutoCAD points)
    let cases: Vec<Case> = vec![
        // DIVIDE-001: (list ename point).
        (
            ("arc", json!({"center": [0.0, 0.0], "radius": 20.0, "start": 30.0, "end": 250.0})),
            "divide",
            5.0,
            [-2.090569265353071, 19.890437907365467],
            4,
            &[
                [-19.021130325903, 6.180339887499],
                [-17.975880925983, -8.767422935782],
                [-9.389431255718, 17.658951857179],
                [5.51274711634, 19.225233918766],
            ],
        ),
        // DIVIDE-002: (list ename point).
        (
            ("circle", json!({"center": [0.0, 0.0], "radius": 15.0})),
            "divide",
            5.0,
            [-4.63525491562421, 14.265847744427305],
            5,
            &[
                [-12.135254915624, -8.816778784387],
                [-12.135254915624, 8.816778784387],
                [4.635254915624, -14.265847744427],
                [4.635254915624, 14.265847744427],
                [15.0, 0.0],
            ],
        ),
        // MEASURE-016: (list ename point).
        (
            ("line", json!({"points": [[0.0, 0.0], [37.0, 13.0]]})),
            "measure",
            7.688652713279647,
            [25.9, 9.1],
            5,
            &[
                [0.730313187001, 0.256596525162],
                [7.984250549601, 2.80527722013],
                [15.2381879122, 5.353957915097],
                [22.4921252748, 7.902638610065],
                [29.7460626374, 10.451319305032],
            ],
        ),
        // MEASURE-024: (list ename point).
        (
            (
                "pline",
                json!({"vertices": [{"p": [0.0, 0.0], "bulge": 0.0}, {"p": [100.0, 0.0], "bulge": 0.0}, {"p": [100.0, 10.0], "bulge": 0.0}, {"p": [5.0, 10.0], "bulge": 0.0}], "closed": false}),
            ),
            "measure",
            7.0,
            [90.0, 0.0],
            29,
            &[[7.0, 0.0], [7.0, 10.0], [14.0, 0.0], [98.0, 0.0], [98.0, 10.0], [100.0, 5.0]],
        ),
        // MEASURE-026: (list ename point).
        (
            (
                "pline",
                json!({"vertices": [{"p": [0.0, 0.0], "bulge": 0.0}, {"p": [10.0, 0.0], "bulge": 0.0}, {"p": [10.0, 100.0], "bulge": 0.0}], "closed": false}),
            ),
            "measure",
            7.0,
            [10.0, 30.0],
            15,
            &[[5.0, 0.0], [10.0, 2.0], [10.0, 9.0], [10.0, 79.0], [10.0, 86.0], [10.0, 93.0]],
        ),
        // MEASURE-030: (list ename point).
        (
            (
                "pline",
                json!({"vertices": [{"p": [0.0, 0.0], "bulge": 0.0}, {"p": [30.0, 0.0], "bulge": 0.5}, {"p": [45.0, 20.0], "bulge": 0.0}, {"p": [10.0, 35.0], "bulge": 0.0}], "closed": true}),
            ),
            "measure",
            7.5,
            [5.0, 17.5],
            17,
            &[
                [2.06040845923, 7.211429607306],
                [4.120816918461, 14.422859214612],
                [5.957390538273, 0.0],
                [41.854257779673, 5.445746712402],
                [45.137412221497, 19.497902817843],
                [45.215233654661, 12.07010340037],
            ],
        ),
        // DIVIDE-028: (list ename point).
        (
            (
                "pline",
                json!({"vertices": [{"p": [0.0, 0.0], "bulge": 0.0}, {"p": [30.0, 0.0], "bulge": 0.5}, {"p": [45.0, 20.0], "bulge": 0.0}, {"p": [10.0, 35.0], "bulge": 0.0}], "closed": true}),
            ),
            "divide",
            5.0,
            [37.5, 10.0],
            5,
            &[
                [0.0, 0.0],
                [7.332712970983, 25.664495398441],
                [25.609294779385, 28.310302237406],
                [26.691478107655, 0.0],
                [45.58190593662, 14.465330916891],
            ],
        ),
        // MEASURE-041: (list ename point).
        (
            (
                "pline",
                json!({"vertices": [{"p": [0.0, 0.0], "bulge": 0.0}, {"p": [30.0, 0.0], "bulge": 0.5}, {"p": [45.0, 20.0], "bulge": 0.0}, {"p": [10.0, 35.0], "bulge": 0.0}], "closed": true}),
            ),
            "measure",
            7.5,
            [45.54951565955276, 17.15901034980981],
            17,
            &[
                [1.636621047999, 5.728173667998],
                [3.69702950723, 12.939603275304],
                [5.75743796646, 20.15103288261],
                [42.799868254703, 6.663750217618],
                [44.060611317761, 20.402595149531],
                [45.491536690495, 13.587129550966],
            ],
        ),
        // MEASURE-042: (list ename point).
        (
            (
                "pline",
                json!({"vertices": [{"p": [0.0, 0.0], "bulge": 0.0}, {"p": [30.0, 0.0], "bulge": 0.5}, {"p": [45.0, 20.0], "bulge": 0.0}, {"p": [10.0, 35.0], "bulge": 0.0}], "closed": true}),
            ),
            "measure",
            7.5,
            [44.0, 20.5],
            17,
            &[
                [2.06040845923, 7.211429607306],
                [4.120816918461, 14.422859214612],
                [5.957390538273, 0.0],
                [41.854257779673, 5.445746712402],
                [45.137412221497, 19.497902817843],
                [45.215233654661, 12.07010340037],
            ],
        ),
    ];
    for ((seed, params), what, value, pick, count, want) in cases {
        let mut s = Session::new();
        let r = s.execute(seed, &params).unwrap();
        let handle = r.get("handle").or_else(|| r["handles"].get(0)).and_then(serde_json::Value::as_str).unwrap().to_string();
        // AutoLISP's (list ename point): the object and where it was picked.
        let params = if what == "divide" {
            json!({"handle": handle, "segments": value as u64, "from": pick})
        } else {
            json!({"handle": handle, "length": value, "from": pick})
        };
        s.execute(what, &params).unwrap();
        let pts: Vec<Vec2> =
            s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Point(p) = &e.kind { Some(p.p.xy()) } else { None }).collect();
        assert_eq!(pts.len(), count, "{what} {pick:?}: {pts:?}");
        for w in want {
            let tol = 1e-9 * w[0].abs().max(w[1].abs()).max(1.0);
            assert!(pts.iter().any(|p| p.near(Vec2::new(w[0], w[1]), tol)), "{what} {pick:?}: no point at {w:?} in {pts:?}");
        }
    }
}

/// A spline's end is chosen by its own parameter: this one spends half its parameter range on a
/// zero-length first span, so a pick at x = 3 is already past its middle (parameter 1.3 of 0..2) and
/// MEASURE runs from the end, though x = 3 is short of the middle of the drawn length.
#[test]
fn measure_spline_by_its_own_parameter() {
    let mut s = Session::new();
    let sp = h(&s.execute("spline", &json!({"control": [[0, 0], [0, 0], [10, 0]], "degree": 1})).unwrap());
    s.execute("measure", &json!({"handle": sp, "length": 3, "from": [3, 0]})).unwrap();
    let pts: Vec<Vec2> =
        s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Point(p) = &e.kind { Some(p.p.xy()) } else { None }).collect();
    assert!(pts.len() == 3 && [7.0, 4.0, 1.0].iter().all(|x| pts.iter().any(|p| near(*p, *x, 0.0))), "{pts:?}");
}

/// MEASURE counts every vertex in a polyline's parameter, coincident ones too (AutoCAD's polyline
/// parameter is the vertex index; the chain drops zero-length spans), and picks an ellipse's end
/// by the ellipse parameter like any other curve: AutoCAD 2024 runs both measured cases below from
/// the end (black-box, accoreconsole, POINTs read back with `entget`).
#[test]
fn measure_counts_coincident_vertices_and_runs_ellipses_by_parameter() {
    let pts = |s: &Session| -> Vec<Vec2> {
        s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Point(p) = &e.kind { Some(p.p.xy()) } else { None }).collect()
    };
    // (0,0) (1,0) (101,0) (101,0) picked at x = 1.1: parameter 1.001 of 3, before the middle, so
    // from the start (the chain alone, 1.001 of 2, ran it from the end).
    let mut s = Session::new();
    let pl = h(&s.execute("pline", &json!({"vertices": [[0, 0], [1, 0], [101, 0], [101, 0]]})).unwrap());
    s.execute("measure", &json!({"handle": pl, "length": 30, "from": [1.1, 0]})).unwrap();
    assert!(pts(&s).iter().any(|p| near(*p, 30.0, 0.0)), "{:?}", pts(&s));
    // (0,0) (0,0) (10,0) (20,0) picked at x = 7: parameter 1.7 of 3, past the middle, so from the
    // end; the same as a 3D polyline.
    for (cmd, params) in [
        ("pline", json!({"vertices": [[0, 0], [0, 0], [10, 0], [20, 0]]})),
        ("3dpoly", json!({"points": [[0, 0, 0], [0, 0, 0], [10, 0, 0], [20, 0, 0]]})),
    ] {
        let mut s = Session::new();
        let pl = h(&s.execute(cmd, &params).unwrap());
        s.execute("measure", &json!({"handle": pl, "length": 3, "from": [7, 0]})).unwrap();
        assert!(pts(&s).iter().any(|p| near(*p, 17.0, 0.0)), "{cmd}: {:?}", pts(&s));
    }
    // AutoCAD's points. CADCraft measures along a tessellation (1e-4 of the major axis), so its
    // points sit within 2e-4 of the axis of AutoCAD's; the other end is 4 to 200 units away.
    type Ellipse = ([f64; 2], [f64; 2], f64, f64, f64);
    let cases: [((Ellipse, f64, [f64; 2]), &[[f64; 2]]); 2] = [
        // MEASURE-032-TO: a full ellipse picked at t = pi.
        (
            (([-35000.0, 81000.0], [1000.0, 250.0], 0.5, 0.0, std::f64::consts::TAU), 325.0, [-36000.0, 80750.0]),
            &[
                [-36005.13895764338, 80852.59066530415],
                [-35871.99367437639, 80574.74515309861],
                [-35846.48143804769, 81129.79467385833],
                [-35585.0606965763, 81321.20806711384],
                [-35570.69066464184, 80460.16118260568],
                [-35288.01208649818, 81451.990340526],
                [-35246.80459288685, 80443.34080344986],
                [-34973.3365764063, 81531.8855660685],
                [-34925.28109250207, 80488.09750605426],
                [-34649.89308676404, 81558.94977503068],
                [-34615.08169799787, 80583.88008545575],
                [-34328.55263503237, 81517.06267451325],
                [-34326.49236487653, 80732.20222770158],
                [-34085.24055588816, 80947.67206458953],
                [-34052.78599900089, 81354.8654746607],
            ],
        ),
        // MEASURE-033: an elliptical arc picked past its middle.
        (
            (([0.0, 0.0], [20.0, 5.0], 0.5, 0.5, 4.0), 4.25, [-14.28970951093154, -10.073417468999274]),
            &[
                [-20.14815072114992, -3.432193431111516],
                [-18.74987859459168, -7.329883069345779],
                [-18.68559214389327, 0.5029121733397516],
                [-15.81338990685036, 3.617741906558092],
                [-15.26656288688172, -9.70206428611582],
                [-12.33992351852055, 6.057945492888583],
                [-8.547503008323163, 7.9699830580747],
                [-4.557798848614094, 9.428419587410612],
                [-0.4368018715850841, 10.45983604769949],
                [3.769519107506058, 11.05284388252232],
                [8.015496424961295, 11.15319778065308],
                [12.22912105170455, 10.6383262710487],
                [16.23125506912425, 9.247174221779819],
            ],
        ),
    ];
    for (((center, major, ratio, t0, t1), length, from), want) in cases {
        let mut s = Session::new();
        let e = h(&s
            .execute("ellipse", &json!({"center": center, "major": major, "ratio": ratio, "start": t0.to_degrees(), "end": t1.to_degrees()}))
            .unwrap());
        s.execute("measure", &json!({"handle": e, "length": length, "from": from})).unwrap();
        let got = pts(&s);
        let tol = 2e-4 * major[0].hypot(major[1]);
        assert_eq!(got.len(), want.len(), "{got:?}");
        for w in want {
            assert!(got.iter().any(|p| p.dist(Vec2::new(w[0], w[1])) < tol), "no point near {w:?} in {got:?}");
        }
    }
}

#[test]
fn divide_and_measure_command_line() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    s.cmdline("divide 5,0 4").unwrap();
    assert!(s.running.is_none());
    assert_eq!(kinds(&s).iter().filter(|k| **k == "Point").count(), 3);
    s.cmdline("measure 9,0 2.5").unwrap();
    assert!(s.running.is_none());
    // Measured from the end nearest the pick.
    assert!(matches!(last(&s), EntityKind::Point(p) if near(p.p.xy(), 2.5, 0.0)));
    // Block option.
    let tick = s.execute("circle", &json!({"center": [50, 50], "radius": 0.1})).unwrap();
    s.execute("block", &json!({"name": "DOT", "base": [50, 50], "handles": [h(&tick)], "keep": "delete"})).unwrap();
    s.cmdline("divide 6,0 b DOT n 2").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(matches!(last(&s), EntityKind::Insert(i) if near(i.insert.xy(), 5.0, 0.0) && i.rotation == 0.0));
    // Invalid count re-prompts.
    s.cmdline("divide 6,0 1").unwrap();
    assert!(s.running.is_some());
    s.cancel();
}

// ---------------- REVCLOUD ----------------

#[test]
fn revcloud_json() {
    let mut s = Session::new();
    s.execute("revcloud", &json!({"p1": [0, 0], "p2": [4, 2], "arcLength": 1})).unwrap();
    match last(&s) {
        EntityKind::LwPolyline(p) => {
            assert!(p.closed);
            assert_eq!(p.vertices.len(), 12);
            assert!(p.vertices.iter().all(|v| v.bulge > 0.0));
        }
        k => panic!("{k:?}"),
    }
    s.execute("revcloud.polygonal", &json!({"points": [[0, 0], [0, 3], [3, 0]], "arcLength": 0.5})).unwrap();
    assert!(matches!(last(&s), EntityKind::LwPolyline(p) if p.vertices.len() > 10));
    s.execute("revcloud.freehand", &json!({"freehand": [[0, 0], [5, 0], [5, 5], [0, 5]], "arcLength": 2, "style": "calligraphy"})).unwrap();
    assert!(matches!(last(&s), EntityKind::LwPolyline(p) if p.vertices.len() == 10 && p.vertices[0].end_width > 0.0));
    let c = h(&s.execute("circle", &json!({"center": [10, 10], "radius": 1})).unwrap());
    let n = s.doc().unwrap().model.len();
    s.execute("revcloud", &json!({"handle": c, "arcLength": 0.5})).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), n, "the circle is replaced");
    assert!(s.execute("revcloud", &json!({"points": [[0, 0], [1, 1], [2, 2]]})).is_err());
    assert!(s.execute("revcloud", &json!({"p1": [0, 0], "p2": [1e300, 1e300], "arcLength": 1e-300})).is_ok());
}

#[test]
fn revcloud_command_line() {
    let mut s = Session::new();
    s.cmdline("revcloud a 1").unwrap();
    s.cmdline("0,0 4,2").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(matches!(last(&s), EntityKind::LwPolyline(p) if p.vertices.len() == 12));
    s.cmdline("revcloud.polygonal 0,0 4,0 4,3").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(kinds(&s), vec!["Polyline", "Polyline"]);
    s.cmdline("revcloud.freehand 0,0 3,0 3,3 0,3").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(kinds(&s).len(), 3);
    // Object option converts a closed polyline.
    s.execute("rectang", &json!({"p1": [10, 10], "p2": [12, 12]})).unwrap();
    s.cmdline("revcloud o 10,11").unwrap();
    assert!(s.running.is_none());
    assert_eq!(kinds(&s).len(), 4);
}

// ---------------- BREAKLINE ----------------

fn breakline_points(s: &Session) -> Vec<(f64, f64)> {
    match last(s) {
        EntityKind::LwPolyline(p) => {
            assert!(!p.closed);
            p.vertices.iter().map(|v| ((v.p.x * 1e9).round() / 1e9, (v.p.y * 1e9).round() / 1e9)).collect()
        }
        k => panic!("not a polyline: {k:?}"),
    }
}

#[test]
fn breakline_json_symbol_size_extension_and_location() {
    let mut s = Session::new();
    // One polyline: the line to the symbol, the zigzag centred on the midpoint, the rest of the line.
    s.execute("breakline", &json!({"p1": [0, 0], "p2": [10, 0]})).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 1);
    assert_eq!(breakline_points(&s), vec![(0.0, 0.0), (4.5, 0.0), (4.75, 0.5), (5.25, -0.5), (5.5, 0.0), (10.0, 0.0)]);
    // Size scales the symbol, Extension runs the line past both points, `at` is projected onto the line.
    s.execute("breakline", &json!({"p1": [0, 0], "p2": [10, 0], "size": 2, "extension": 1, "at": [2, 3]})).unwrap();
    assert_eq!(breakline_points(&s), vec![(-1.0, 0.0), (1.0, 0.0), (1.5, 1.0), (2.5, -1.0), (3.0, 0.0), (11.0, 0.0)]);
    // A location past the end keeps the symbol on the line; its end then is the line's end.
    s.execute("breakline", &json!({"p1": [0, 0], "p2": [10, 0], "size": 2, "at": [100, 0]})).unwrap();
    assert_eq!(breakline_points(&s), vec![(0.0, 0.0), (8.0, 0.0), (8.5, 1.0), (9.5, -1.0), (10.0, 0.0)]);
    // The symbol turns with the line.
    s.execute("breakline", &json!({"p1": [0, 0], "p2": [0, 10]})).unwrap();
    assert_eq!(breakline_points(&s), vec![(0.0, 0.0), (0.0, 4.5), (-0.5, 4.75), (0.5, 5.25), (0.0, 5.5), (0.0, 10.0)]);
    // DIMSCALE scales the size.
    s.doc_mut().unwrap().header.set_f64("DIMSCALE", 4.0);
    s.execute("breakline", &json!({"p1": [0, 0], "p2": [10, 0]})).unwrap();
    assert_eq!(breakline_points(&s), vec![(0.0, 0.0), (3.0, 0.0), (4.0, 2.0), (6.0, -2.0), (7.0, 0.0), (10.0, 0.0)]);

    let n = s.doc().unwrap().model.len();
    for p in [
        json!({"p1": [0, 0], "p2": [3, 0]}),
        json!({"p1": [0, 0], "p2": [0, 0]}),
        json!({"p1": [0, 0], "p2": [10, 0], "size": 0}),
        json!({"p1": [0, 0], "p2": [10, 0], "extension": -1}),
        json!({"p1": [0, 0], "p2": [10, 0], "block": "NOPE"}),
        json!({"p1": [0, 0], "p2": [1e308, 0], "extension": 1e308}),
    ] {
        assert!(s.execute("breakline", &p).is_err(), "{p}");
    }
    assert_eq!(s.doc().unwrap().model.len(), n);
}

#[test]
fn breakline_block_symbol_runs_between_its_defpoints() {
    let mut s = Session::new();
    s.execute("layer.current", &json!({"name": "Defpoints"})).unwrap();
    s.execute("point", &json!({"at": [0, 0]})).unwrap();
    s.execute("point", &json!({"at": [2, 0]})).unwrap();
    s.execute("layer.current", &json!({"name": "0"})).unwrap();
    // Drawn in pieces, one of them backwards: they are chained from one point to the other.
    s.execute("line", &json!({"points": [[2, 0], [1, 1]]})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [1, 1]]})).unwrap();
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name": "PEAK", "base": [0, 0], "handles": hs, "keep": "delete"})).unwrap();
    s.execute("breakline", &json!({"p1": [0, 0], "p2": [10, 0], "block": "PEAK"})).unwrap();
    assert_eq!(breakline_points(&s), vec![(0.0, 0.0), (4.0, 0.0), (5.0, 1.0), (6.0, 0.0), (10.0, 0.0)]);
    // Without two Defpoints points the block is refused.
    s.execute("line", &json!({"points": [[0, 5], [1, 5]]})).unwrap();
    let l = s.doc().unwrap().model.last().unwrap().handle.hex();
    s.execute("block", &json!({"name": "BARE", "base": [0, 0], "handles": [l], "keep": "delete"})).unwrap();
    assert!(s.execute("breakline", &json!({"p1": [0, 0], "p2": [10, 0], "block": "BARE"})).is_err());

    s.cmdline("breakline b PEAK 0,0 10,0").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(breakline_points(&s).len(), 5);
    s.cmdline("breakline b nope").unwrap();
    assert!(s.prompt_text().contains("block"), "an unknown block asks again: {}", s.prompt_text());
    s.cmdline(".").unwrap();
    s.cmdline("0,0 10,0").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(breakline_points(&s).len(), 6, "`.` goes back to the built-in symbol");
}

#[test]
fn breakline_command_line_prompts_options_and_undo() {
    let mut s = Session::new();
    s.start("breakline").unwrap();
    assert!(s.prompt_text().contains("Specify first point for breakline"), "{}", s.prompt_text());
    assert!(s.prompt_text().contains("Block") && s.prompt_text().contains("Size") && s.prompt_text().contains("Extension"), "{}", s.prompt_text());
    s.cmdline("0,0").unwrap();
    assert!(s.prompt_text().contains("Specify second point for breakline"), "{}", s.prompt_text());
    // Too short for the symbol: asked again.
    s.cmdline("0.5,0").unwrap();
    assert!(s.prompt_text().contains("second point"), "{}", s.prompt_text());
    s.cmdline("10,0").unwrap();
    assert!(s.prompt_text().contains("Specify location for break symbol") && s.prompt_text().contains("Midpoint"), "{}", s.prompt_text());
    assert!(matches!(s.preview(Vec2::new(3.0, 1.0)).first().map(|e| &e.kind), Some(EntityKind::LwPolyline(p)) if p.vertices.len() == 6));
    s.cmdline("").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(breakline_points(&s), vec![(0.0, 0.0), (4.5, 0.0), (4.75, 0.5), (5.25, -0.5), (5.5, 0.0), (10.0, 0.0)]);
    assert_eq!(s.state().unwrap().undo.len(), 1, "one undo step");

    // Size and Extension at the first prompt, a picked location; remembered for the next time.
    s.cmdline("breakline s 2 e 1 0,0 10,0 2,3").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(breakline_points(&s), vec![(-1.0, 0.0), (1.0, 0.0), (1.5, 1.0), (2.5, -1.0), (3.0, 0.0), (11.0, 0.0)]);
    assert_eq!(s.state().unwrap().undo.len(), 2);
    s.cmdline("breakline s").unwrap();
    assert!(s.prompt_text().contains("2.0000"), "{}", s.prompt_text());
    s.cmdline("0").unwrap();
    assert!(s.prompt_text().contains("size"), "zero is refused: {}", s.prompt_text());
    s.cancel();
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 1, "undo removes the whole breakline");
}

// ---------------- WIPEOUT ----------------

#[test]
fn wipeout_json_and_command_line() {
    let mut s = Session::new();
    s.execute("wipeout", &json!({"points": [[0, 0], [2, 0], [2, 2]]})).unwrap();
    assert!(matches!(last(&s), EntityKind::Wipeout(w) if w.boundary.len() == 3));
    let r = h(&s.execute("rectang", &json!({"p1": [5, 5], "p2": [6, 6]})).unwrap());
    s.execute("wipeout", &json!({"handle": r, "erase": true})).unwrap();
    assert_eq!(kinds(&s), vec!["Wipeout", "Wipeout"]);
    s.execute("wipeout", &json!({"frames": "off"})).unwrap();
    assert_eq!(s.doc().unwrap().header.i64("WIPEOUTFRAME", 1), 0);
    assert!(s.execute("wipeout", &json!({"points": [[0, 0], [1, 1]]})).is_err());
    assert!(s.execute("wipeout", &json!({"frames": "sideways"})).is_err());

    s.cmdline("wipeout 0,0 3,0 3,3").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(kinds(&s).len(), 3);
    s.execute("rectang", &json!({"p1": [20, 20], "p2": [22, 22]})).unwrap();
    s.cmdline("wipeout p 20,21 y").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(kinds(&s), vec!["Wipeout"; 4]);
    s.cmdline("wipeout f on").unwrap();
    assert_eq!(s.doc().unwrap().header.i64("WIPEOUTFRAME", 0), 1);
}

// ---------------- XLINE ----------------

fn xline_dir(k: &EntityKind) -> (Vec2, Vec2) {
    match k {
        EntityKind::XLine(r) => (r.base.xy(), r.dir.xy()),
        k => panic!("{k:?}"),
    }
}

#[test]
fn xline_options_json() {
    let mut s = Session::new();
    s.execute("xline", &json!({"vertex": [0, 0], "start": [1, 0], "end": [0, 1]})).unwrap();
    let (_, d) = xline_dir(&last(&s));
    assert!(d.near(Vec2::new(1.0, 1.0).normalized(), 1e-9));
    let l = s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    s.execute("xline", &json!({"handle": l, "distance": 2, "side": [5, -1]})).unwrap();
    let (b, d) = xline_dir(&last(&s));
    assert!((b.y + 2.0).abs() < 1e-9 && d.near(Vec2::X, 1e-9));
    s.execute("xline", &json!({"handle": l, "through": [3, 7]})).unwrap();
    assert!(near(xline_dir(&last(&s)).0, 3.0, 7.0));
    s.execute("xline", &json!({"base": [1, 1], "ver": true})).unwrap();
    assert!(xline_dir(&last(&s)).1.near(Vec2::Y, 1e-9));
    s.execute("xline", &json!({"base": [1, 1], "angle": 30})).unwrap();
    s.execute("xline", &json!({"base": [1, 1], "through": [2, 2]})).unwrap();
    assert!(s.execute("xline", &json!({"base": [1, 1], "through": [1, 1]})).is_err());
}

#[test]
fn xline_options_command_line() {
    let mut s = Session::new();
    s.cmdline("xline b 0,0 1,0 0,1 -1,0").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(kinds(&s), vec!["Xline", "Xline"]);
    s.cmdline("xline a 45 3,3").unwrap();
    s.cmdline("").unwrap();
    assert!(xline_dir(&last(&s)).1.near(Vec2::new(1.0, 1.0).normalized(), 1e-9));
    s.execute("line", &json!({"points": [[20, 0], [30, 0]]})).unwrap();
    s.cmdline("xline o 2 25,0 25,5").unwrap();
    s.cmdline("").unwrap();
    let (b, _) = xline_dir(&last(&s));
    assert!((b.y - 2.0).abs() < 1e-9);
    s.cmdline("xline o t 25,0 0,9").unwrap();
    s.cmdline("").unwrap();
    assert!(near(xline_dir(&last(&s)).0, 0.0, 9.0));
    s.cmdline("xline a r 25,0 90 4,4").unwrap();
    s.cmdline("").unwrap();
    assert!(xline_dir(&last(&s)).1.near(Vec2::Y, 1e-9));
    s.cmdline("xline h 0,7").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
}

// ---------------- RECTANG ----------------

/// The corners of the rectangle just added, so a test can assert its bounds.
fn rect_pts(s: &Session) -> Vec<Vec2> {
    match last(s) {
        EntityKind::LwPolyline(p) => p.vertices.iter().map(|v| v.p).collect(),
        k => panic!("{k:?}"),
    }
}

#[test]
fn rectang_options_json() {
    let mut s = Session::new();
    s.execute("rectang", &json!({"p1": [0, 0], "p2": [4, 3]})).unwrap();
    assert!(near(rect_pts(&s)[2], 4.0, 3.0));
    s.execute("rectang", &json!({"p1": [0, 0], "dimensions": [3, 2], "p2": [-1, -1]})).unwrap();
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, -3.0, -2.0) && near(b.max, 0.0, 0.0));
    s.execute("rectang", &json!({"p1": [0, 0], "area": 20, "length": 5})).unwrap();
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.max, 5.0, 4.0));
    s.execute("rectang", &json!({"p1": [0, 0], "dimensions": [2, 1], "rotation": 90})).unwrap();
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, -1.0, 0.0) && near(b.max, 0.0, 2.0), "{b:?}");
    s.execute("rectang", &json!({"p1": [0, 0], "p2": [4, 4], "fillet": 1, "width": 0.1, "thickness": 2})).unwrap();
    assert_eq!(rect_pts(&s).len(), 8);
    assert_eq!(s.doc().unwrap().model.last().unwrap().common.thickness, 2.0);
    assert!(s.execute("rectang", &json!({"p1": [0, 0], "area": -1, "length": 1})).is_err());
    assert!(s.execute("rectang", &json!({"p1": [0, 0], "p2": [0, 5]})).is_err());
}

#[test]
fn rectang_options_command_line() {
    let mut s = Session::new();
    s.cmdline("rectang 0,0 a 20 l 5").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(near(cadcraft_geom::Bounds2::from_points(rect_pts(&s)).max, 5.0, 4.0));
    s.cmdline("rectang 0,0 a 12 w 3").unwrap();
    assert!(near(cadcraft_geom::Bounds2::from_points(rect_pts(&s)).max, 4.0, 3.0));
    s.cmdline("rectang 0,0 d 3 2 -1,-1").unwrap();
    assert!(near(cadcraft_geom::Bounds2::from_points(rect_pts(&s)).min, -3.0, -2.0));
    s.cmdline("rectang 0,0 r 90 2,1").unwrap();
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, 0.0, 0.0) && near(b.max, 2.0, 1.0), "{b:?}");
    s.cmdline("rectang 0,0 r p 0,0 1,1 d 2 2 5,5").unwrap();
    let pts = rect_pts(&s);
    assert!(pts.iter().any(|p| near(*p, 0.0, 8f64.sqrt())), "{pts:?}");
    // The 45° rotation picked above is remembered, as AutoCAD remembers it: set it back.
    s.cmdline("rectang w 0.5 c 0.5 0.5 0,0 r 0 4,4").unwrap();
    match last(&s) {
        EntityKind::LwPolyline(p) => assert!(p.const_width == 0.5 && p.vertices.len() == 8),
        k => panic!("{k:?}"),
    }
    s.cmdline("rectang f 1 e 2 t 3 0,0 4,4").unwrap();
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::LwPolyline(p) => assert!(p.elevation == 2.0 && p.vertices.len() == 8),
        k => panic!("{k:?}"),
    }
}

/// RECTANG against what AutoCAD 2024 stores for the same typed input (black-box: synthetic
/// input in accoreconsole, read back with `entget`): vertex order, chamfer and fillet corners,
/// corners too large for the sides, Area with the corners taken off, and options remembered
/// between invocations.
#[test]
fn rectang_vertices_match_autocad() {
    type Case = (&'static [&'static str], &'static [[f64; 3]], [f64; 3]);
    // (typed input, vertices [x, y, bulge], [width, elevation, thickness])
    let cases: [Case; 12] = [
        // RECTANG-000: the first corner first, along the first axis: clockwise here.
        (
            &["rectang -2.2557479240466947,-9.488221049171962 _R 0 -31.36203661716106,-47.886714708394166"],
            &[
                [-2.255747924046694, -9.488221049171962, 0.0],
                [-31.36203661716105, -9.488221049171962, 0.0],
                [-31.36203661716105, -47.88671470839417, 0.0],
                [-2.255747924046694, -47.88671470839417, 0.0],
            ],
            [0.0, 0.0, 0.0],
        ),
        // RECTANG-001: chamfer: the first distance on the edge into each corner, the second on the edge out.
        (
            &["rectang _C 2.2518104656366162 3.766337568886034 4.8089419289768,29.66648885915597 _R 0 28.114797694689763,43.85884965263156"],
            &[
                [4.8089419289768, 31.91829932479258, 0.0],
                [8.575279497862834, 29.66648885915597, 0.0],
                [25.86298722905314, 29.66648885915597, 0.0],
                [28.11479769468976, 33.432826428042, 0.0],
                [28.11479769468976, 41.60703918699494, 0.0],
                [24.34846012580373, 43.85884965263156, 0.0],
                [7.060752394613416, 43.85884965263156, 0.0],
                [4.8089419289768, 40.09251208374553, 0.0],
            ],
            [0.0, 0.0, 0.0],
        ),
        // RECTANG-009: the same, running clockwise.
        (
            &["rectang _C 1.6573799714125943 2.8497357251231277 24.04847098725292,25.59232167490267 _R 0 6.633832923690861,65.99502168115245"],
            &[
                [24.04847098725292, 27.24970164631526, 0.0],
                [21.19873526212979, 25.59232167490267, 0.0],
                [8.291212895103454, 25.59232167490267, 0.0],
                [6.63383292369086, 28.4420574000258, 0.0],
                [6.63383292369086, 64.33764170973986, 0.0],
                [9.48356864881399, 65.99502168115245, 0.0],
                [22.39109101584032, 65.99502168115245, 0.0],
                [24.04847098725292, 63.14528595602933, 0.0],
            ],
            [0.0, 0.0, 0.0],
        ),
        // RECTANG-018: a fillet running clockwise bulges the other way.
        (
            &["rectang _F 1.5960102281545085 -8.446136404870288,-1.0540502598928434 _R 0 41.8290524978361,-18.73385540701941"],
            &[
                [-8.446136404870288, -2.650060488047352, -0.414213562373095],
                [-6.850126176715778, -1.054050259892843, 0.0],
                [40.23304226968159, -1.054050259892843, -0.414213562373095],
                [41.8290524978361, -2.650060488047352, 0.0],
                [41.8290524978361, -17.1378451788649, -0.414213562373095],
                [40.23304226968159, -18.7338554070194, 0.0],
                [-6.850126176715778, -18.7338554070194, -0.414213562373095],
                [-8.446136404870288, -17.1378451788649, 0.0],
            ],
            [0.0, 0.0, 0.0],
        ),
        // RECTANG-025: Chamfer then Fillet: the fillet wins.
        (
            &["rectang _C 1.0 2.0 _F 3.0 -23.770950967162776,10.634038271168158 _R 0 5.6012940597800664,50.84914942110857"],
            &[
                [-23.77095096716277, 13.63403827116815, 0.414213562373095],
                [-20.77095096716277, 10.63403827116815, 0.0],
                [2.601294059780066, 10.63403827116815, 0.414213562373095],
                [5.601294059780066, 13.63403827116815, 0.0],
                [5.601294059780066, 47.84914942110857, 0.414213562373095],
                [2.601294059780066, 50.84914942110857, 0.0],
                [-20.77095096716277, 50.84914942110857, 0.414213562373095],
                [-23.77095096716277, 47.84914942110857, 0.0],
            ],
            [0.0, 0.0, 0.0],
        ),
        // RECTANG-026: Fillet then Chamfer: the chamfer wins.
        (
            &["rectang _F 3.0 _C 1.0 2.0 8.075900385075748,-27.116639225773056 _R 0 20.00095065903931,8.864147463029973"],
            &[
                [8.075900385075748, -26.11663922577305, 0.0],
                [10.07590038507574, -27.11663922577305, 0.0],
                [19.00095065903931, -27.11663922577305, 0.0],
                [20.00095065903931, -25.11663922577305, 0.0],
                [20.00095065903931, 7.864147463029972, 0.0],
                [18.00095065903931, 8.864147463029973, 0.0],
                [9.075900385075748, 8.864147463029973, 0.0],
                [8.075900385075748, 6.864147463029972, 0.0],
            ],
            [0.0, 0.0, 0.0],
        ),
        // RECTANG-028: Area counts the chamfered corners off.
        (
            &["rectang _C 1.5 2.5 26.36462345136234,-19.7291241693893 _R 0 _A 1442.9297064464572 _W 35.53488991311575"],
            &[
                [26.36462345136234, -18.2291241693893, 0.0],
                [28.86462345136234, -19.7291241693893, 0.0],
                [65.68168268505678, -19.7291241693893, 0.0],
                [67.18168268505678, -17.2291241693893, 0.0],
                [67.18168268505678, 14.30576574372644, 0.0],
                [64.68168268505678, 15.80576574372644, 0.0],
                [27.86462345136234, 15.80576574372644, 0.0],
                [26.36462345136234, 13.30576574372644, 0.0],
            ],
            [0.0, 0.0, 0.0],
        ),
        // RECTANG-035-VN: Area counts the fillets off, at survey coordinates.
        (
            &["rectang _F 2.0 612322.0691653182,2398792.948957577 _R 0 _A 592.8936847254046 _L 19.287858563349488"],
            &[
                [612322.0691653182, 2398794.948957577, 0.414213562373095],
                [612324.0691653182, 2398792.948957577, 0.0],
                [612339.3570238815, 2398792.948957577, 0.414213562373095],
                [612341.3570238815, 2398794.948957577, 0.0],
                [612341.3570238815, 2398821.866195597, 0.414213562373095],
                [612339.3570238815, 2398823.866195598, 0.0],
                [612324.0691653182, 2398823.866195598, 0.414213562373095],
                [612322.0691653182, 2398821.866195597, 0.0],
            ],
            [0.0, 0.0, 0.0],
        ),
        // RECTANG-029: a fillet too large: each corner two coincident vertices.
        (
            &["rectang _F 11.37084114989061 -21.46774953784037,-0.023473793210982308 _R 0 -2.5163476213560187,49.88119923361903"],
            &[
                [-21.46774953784036, -0.0234737932109823, 0.0],
                [-21.46774953784036, -0.0234737932109823, 0.0],
                [-2.516347621356018, -0.0234737932109823, 0.0],
                [-2.516347621356018, -0.0234737932109823, 0.0],
                [-2.516347621356018, 49.88119923361903, 0.0],
                [-2.516347621356018, 49.88119923361903, 0.0],
                [-21.46774953784036, 49.88119923361903, 0.0],
                [-21.46774953784036, 49.88119923361903, 0.0],
            ],
            [0.0, 0.0, 0.0],
        ),
        // RECTANG-039-VN: a chamfer too large, rotated, with Dimensions.
        (
            &[
                "rectang _C 18.656567079201356 18.656567079201356 612317.9599920243,2398739.0200147885 _R 30 _D 26.652238684573366 32.64533161964191 612344.6122307089,2398771.6653464083",
            ],
            &[
                [612317.9599920243, 2398739.020014788, 0.0],
                [612317.9599920243, 2398739.020014788, 0.0],
                [612341.041507793, 2398752.34613413, 0.0],
                [612341.041507793, 2398752.34613413, 0.0],
                [612324.7188419831, 2398780.617820628, 0.0],
                [612324.7188419831, 2398780.617820628, 0.0],
                [612301.6373262145, 2398767.291701286, 0.0],
                [612301.6373262145, 2398767.291701286, 0.0],
            ],
            [0.0, 0.0, 0.0],
        ),
        // RECTANG-030: rotation by two points, elevation and thickness.
        (
            &["rectang _E 7.5 _T 2.25 -18.42481827666694,-20.50343704206707 _R _P 0.0,0.0 10.0,6.0 10.197290578707722,25.5222006119985"],
            &[
                [-18.42481827666694, -20.50343704206706, 0.0],
                [22.92627837613747, 4.307220949615576, 0.0],
                [10.19729057870772, 25.5222006119985, 0.0],
                [-31.15380607409669, 0.7115426203158534, 0.0],
            ],
            [0.0, 7.5, 2.25],
        ),
        // RECTANG-024: the fillet is remembered for the next RECTANG.
        (
            &[
                "rectang _F 2.0 -12.364845593042645,8.36037449067684 _R 0 31.044931977794846,17.998089850295315",
                "rectang 41.044931977794846,27.998089850295315 84.45470954863234,37.63580520991379",
            ],
            &[
                [41.04493197779484, 29.99808985029531, 0.414213562373095],
                [43.04493197779485, 27.99808985029531, 0.0],
                [82.45470954863234, 27.99808985029531, 0.414213562373095],
                [84.45470954863234, 29.99808985029531, 0.0],
                [84.45470954863234, 35.63580520991379, 0.414213562373095],
                [82.45470954863234, 37.63580520991379, 0.0],
                [43.04493197779485, 37.63580520991379, 0.414213562373095],
                [41.04493197779484, 35.63580520991379, 0.0],
            ],
            [0.0, 0.0, 0.0],
        ),
    ];
    let same = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
    for (input, want, [width, elevation, thickness]) in cases {
        let mut s = Session::new();
        for line in input {
            s.cmdline(line).unwrap();
            assert!(s.running.is_none(), "{line}: {}", s.prompt_text());
        }
        let e = s.doc().unwrap().model.last().unwrap().clone();
        let EntityKind::LwPolyline(p) = &e.kind else { panic!("{input:?}: {:?}", e.kind) };
        let got: Vec<[f64; 3]> = p.vertices.iter().map(|v| [v.p.x, v.p.y, v.bulge]).collect();
        let ok = got.len() == want.len() && got.iter().zip(want).all(|(g, w)| g.iter().zip(w).all(|(a, b)| same(*a, *b)));
        assert!(
            ok,
            "{input:?}
 got {got:?}
want {want:?}"
        );
        assert!(p.closed && same(p.const_width, width) && same(p.elevation, elevation) && same(e.common.thickness, thickness), "{input:?}");
    }
    // Rotation, elevation and thickness are remembered too: AutoCAD opens the next RECTANG with
    // "Current rectangle modes:  Elevation=7.5000  Thickness=2.2500  Rotation=30.96".
    let mut s = Session::new();
    s.cmdline("rectang _E 7.5 _T 2.25 0,0 _R _P 0,0 10,6 10,20").unwrap();
    s.cmdline("rectang 100,100 110,105").unwrap();
    let e = s.doc().unwrap().model.last().unwrap().clone();
    let EntityKind::LwPolyline(p) = &e.kind else { panic!("{:?}", e.kind) };
    let edge = p.vertices[1].p - p.vertices[0].p;
    assert!(p.vertices[0].p == Vec2::new(100.0, 100.0) && same(edge.angle(), 6f64.atan2(10.0)), "{p:?}");
    assert!(p.elevation == 7.5 && e.common.thickness == 2.25);
}

/// The options a RECTANG carries over are named when it starts, in AutoCAD's words (its log for
/// the same inputs: "Current rectangle modes:  Chamfer=1.6574 x 2.8497  Rotation=50.50" and
/// "Current rectangle modes:  Elevation=7.5000  Thickness=2.2500  Rotation=30.96"); none, no line.
#[test]
fn rectang_names_the_modes_it_carries_over() {
    let modes = |s: &Session| s.log.iter().filter(|l| l.starts_with("Current rectangle modes:")).cloned().collect::<Vec<_>>();
    let mut s = Session::new();
    s.start("rectang").unwrap();
    s.cancel();
    assert!(modes(&s).is_empty());
    s.cmdline("rectang _C 1.6573799714125943 2.8497357251231277 0,0 _R 50.50299224048081 10,5").unwrap();
    s.start("rectang").unwrap();
    s.cancel();
    assert_eq!(modes(&s), ["Current rectangle modes:  Chamfer=1.6574 x 2.8497  Rotation=50.50"]);
    let mut s = Session::new();
    s.cmdline("rectang _E 7.5 _T 2.25 0,0 _R _P 0,0 10,6 10,20").unwrap();
    s.start("rectang").unwrap();
    assert_eq!(modes(&s), ["Current rectangle modes:  Elevation=7.5000  Thickness=2.2500  Rotation=30.96"]);
    s.cancel();
    // With ANGBASE 90 and ANGDIR 1, AutoCAD 2024 turns a rectangle given Rotation 30 clockwise
    // by 30° from +X (ANGBASE aside), and shows that rotation from ANGBASE: "Rotation=120.00",
    // and <120.00> as the next rotation default.
    let mut s = Session::new();
    s.cmdline("setvar angbase 90").unwrap();
    s.cmdline("setvar angdir 1").unwrap();
    s.cmdline("rectang 50,0 _R 30 60,5").unwrap();
    let e = s.doc().unwrap().model.last().unwrap().clone();
    let EntityKind::LwPolyline(p) = &e.kind else { panic!("{:?}", e.kind) };
    let edge = p.vertices[1].p - p.vertices[0].p;
    assert!((edge.y.atan2(edge.x) + 30f64.to_radians()).abs() < 1e-9, "{p:?}");
    s.start("rectang").unwrap();
    assert_eq!(modes(&s), ["Current rectangle modes:  Rotation=120.00"]);
    s.cmdline("50,0").unwrap();
    s.cmdline("_R").unwrap();
    assert!(s.prompt_text().contains("<120.00>"), "{}", s.prompt_text());
    s.cancel();
    // In degrees/minutes/seconds: "Rotation=30d30'".
    let mut s = Session::new();
    s.cmdline("setvar aunits 1").unwrap();
    s.cmdline("rectang 50,0 _R 30.5 60,5").unwrap();
    s.start("rectang").unwrap();
    assert_eq!(modes(&s), ["Current rectangle modes:  Rotation=30d30'"]);
    s.cancel();
}

/// Area allows for cut corners only while they fit: a fillet too large for the resulting sides
/// leaves square corners, so the rectangle is the full area asked for (4 × 1, not 4 × 1.858).
#[test]
fn rectang_area_with_corners_too_large_stays_exact() {
    let check = |s: &Session| {
        let pts = rect_pts(s);
        let b = cadcraft_geom::Bounds2::from_points(pts.clone());
        assert!(near(b.min, 0.0, 0.0) && near(b.max, 4.0, 1.0), "{pts:?}");
        assert!(pts.len() == 8 && pts.chunks(2).all(|c| c[0] == c[1]), "{pts:?}");
    };
    let mut s = Session::new();
    s.cmdline("rectang _F 2 0,0 _A 4 _L 4").unwrap();
    check(&s);
    s.execute("rectang", &json!({"p1": [0, 0], "area": 4, "length": 4, "fillet": 2})).unwrap();
    check(&s);
}

/// Guards the JSON `center: true` form for every sizing route: a picked corner mirrored about
/// the centre, full-size `dimensions`, full `area`, rotation about the centre, that the default
/// corner mode is untouched, and that centre mode without a size is rejected.
#[test]
fn rectang_center_mode_json() {
    let mut s = Session::new();
    // The first point is the centre; the second is a corner mirrored about it.
    s.execute("rectang", &json!({"center": true, "p1": [10, 10], "p2": [14, 16]})).unwrap();
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, 6.0, 4.0) && near(b.max, 14.0, 16.0), "{b:?}");
    // Dimensions are the rectangle's full size, still centred on the first point.
    s.execute("rectang", &json!({"center": true, "p1": [10, 10], "dimensions": [8, 12]})).unwrap();
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, 6.0, 4.0) && near(b.max, 14.0, 16.0), "{b:?}");
    // A second point only picks the quadrant.
    s.execute("rectang", &json!({"center": true, "p1": [10, 10], "dimensions": [8, 12], "p2": [0, 0]})).unwrap();
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, 6.0, 4.0) && near(b.max, 14.0, 16.0), "{b:?}");
    // The area is the full area, centred too.
    s.execute("rectang", &json!({"center": true, "p1": [10, 10], "area": 96, "length": 8})).unwrap();
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, 6.0, 4.0) && near(b.max, 14.0, 16.0), "{b:?}");
    // Rotation is about the centre.
    s.execute("rectang", &json!({"center": true, "p1": [10, 10], "dimensions": [8, 12], "rotation": 90})).unwrap();
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, 4.0, 6.0) && near(b.max, 16.0, 14.0), "{b:?}");
    // The default corner mode is unchanged.
    s.execute("rectang", &json!({"p1": [10, 10], "p2": [14, 16]})).unwrap();
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, 10.0, 10.0) && near(b.max, 14.0, 16.0), "{b:?}");
    // Centre mode still needs a size.
    assert!(s.execute("rectang", &json!({"center": true, "p1": [10, 10]})).is_err());
}

/// Guards that fillet, chamfer, width and thickness still apply in centre mode, and that an
/// oversized fillet leaves sharp corners without moving the rectangle off centre.
#[test]
fn rectang_center_mode_honours_options() {
    let mut s = Session::new();
    s.execute("rectang", &json!({"center": true, "p1": [10, 10], "p2": [14, 16], "fillet": 1, "width": 0.25, "thickness": 2})).unwrap();
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::LwPolyline(p) => assert!(p.vertices.len() == 8 && p.const_width == 0.25),
        k => panic!("{k:?}"),
    }
    assert_eq!(s.doc().unwrap().model.last().unwrap().common.thickness, 2.0);
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, 6.0, 4.0) && near(b.max, 14.0, 16.0), "{b:?}");
    // Chamfer, still centred.
    s.execute("rectang", &json!({"center": true, "p1": [10, 10], "p2": [14, 16], "chamfer": [1, 1]})).unwrap();
    assert_eq!(rect_pts(&s).len(), 8);
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, 6.0, 4.0) && near(b.max, 14.0, 16.0), "{b:?}");
    // A fillet too large for the rectangle leaves sharp corners, each one two coincident vertices
    // as in AutoCAD, still centred.
    s.execute("rectang", &json!({"center": true, "p1": [10, 10], "p2": [14, 16], "fillet": 5})).unwrap();
    let pts = rect_pts(&s);
    assert!(pts.len() == 8 && pts.chunks(2).all(|c| c[0] == c[1]), "{pts:?}");
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, 6.0, 4.0) && near(b.max, 14.0, 16.0), "{b:?}");
}

/// Guards the `Center` keyword at the command line, before or after other options, with
/// dimensions and area still centred, the prompt wording and centred rubber band, and that the
/// default corner mode is unchanged.
#[test]
fn rectang_center_mode_command_line() {
    let mut s = Session::new();
    s.cmdline("rectang ce 10,10 14,16").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, 6.0, 4.0) && near(b.max, 14.0, 16.0), "{b:?}");
    // Options before the centre point, and the centre picked after them.
    s.cmdline("rectang center f 1 w 0.1 10,10 14,16").unwrap();
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::LwPolyline(p) => assert!(p.vertices.len() == 8 && p.const_width == 0.1),
        k => panic!("{k:?}"),
    }
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, 6.0, 4.0) && near(b.max, 14.0, 16.0), "{b:?}");
    // Dimensions after the centre: the pick only chooses the quadrant.
    s.cmdline("rectang ce 10,10 d 8 12 14,16").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, 6.0, 4.0) && near(b.max, 14.0, 16.0), "{b:?}");
    // Area too: the rectangle of that area is centred on the first point (without the fillet
    // remembered from above, which would take its corners off the area).
    s.cmdline("rectang f 0 ce 10,10 a 96 l 8").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, 6.0, 4.0) && near(b.max, 14.0, 16.0), "{b:?}");
    // The rubber band is centred while the second point is picked.
    s.start("rectang").unwrap();
    s.cmdline("ce").unwrap();
    assert!(s.prompt_text().contains("Specify center point for rectangle"), "{}", s.prompt_text());
    s.cmdline("10,10").unwrap();
    let pv = s.preview(Vec2::new(14.0, 16.0));
    let pb = match pv.first().map(|e| &e.kind) {
        Some(EntityKind::LwPolyline(p)) => cadcraft_geom::Bounds2::from_points(p.vertices.iter().map(|v| v.p).collect::<Vec<_>>()),
        k => panic!("{k:?}"),
    };
    assert!(near(pb.min, 6.0, 4.0) && near(pb.max, 14.0, 16.0), "{pb:?}");
    s.cancel();
    // The default corner mode is unchanged.
    s.cmdline("rectang 10,10 14,16").unwrap();
    let b = cadcraft_geom::Bounds2::from_points(rect_pts(&s));
    assert!(near(b.min, 10.0, 10.0) && near(b.max, 14.0, 16.0), "{b:?}");
}

// ---------------- 3DPOLY / MLINE / HELIX / SPLINE CV ----------------

#[test]
fn poly3d_json_and_command_line() {
    let mut s = Session::new();
    s.execute("3dpoly", &json!({"points": [[0, 0, 0], [1, 0, 2], [1, 1, 3]], "closed": true})).unwrap();
    assert!(matches!(last(&s), EntityKind::Polyline3d(p) if p.points.len() == 3 && p.points[2].z == 3.0 && p.closed));
    assert!(s.execute("3dpoly", &json!({"points": [[0, 0]]})).is_err());
    s.cmdline("3dpoly 0,0 1,0 1,1 u 2,2 c").unwrap();
    assert!(s.running.is_none());
    assert!(matches!(last(&s), EntityKind::Polyline3d(p) if p.points.len() == 3 && p.closed));
    assert_eq!(s.state().unwrap().undo.len(), 2);
}

#[test]
fn mline_json_and_command_line() {
    // Element offsets from the vertex path, sorted.
    let offs = |k: EntityKind| match k {
        EntityKind::MLine(m) => {
            let mut o = m.offsets();
            o.sort_by(f64::total_cmp);
            o
        }
        k => panic!("{k:?}"),
    };
    let mut s = Session::new();
    s.execute("mline", &json!({"points": [[0, 0], [10, 0], [10, 10]], "scale": 2, "justification": "zero"})).unwrap();
    assert_eq!(offs(last(&s)), vec![-1.0, 1.0]);
    assert!(s.execute("mline", &json!({"points": [[0, 0], [1, 0]], "justification": "middle"})).is_err());
    let mut s = Session::new();
    s.cmdline("mline j b s 3 0,0 10,0 10,5").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(offs(last(&s)), vec![0.0, 3.0]);
    s.cmdline("mline 0,20 5,20 5,25 c").unwrap();
    assert!(matches!(last(&s), EntityKind::MLine(m) if m.closed && m.vertices.len() == 3));
}

#[test]
fn helix_json_and_command_line() {
    let mut s = Session::new();
    s.execute("helix", &json!({"center": [0, 0], "baseRadius": 2, "topRadius": 1, "height": 6, "turns": 2})).unwrap();
    match last(&s) {
        EntityKind::Helix(h) => {
            assert!(near(h.start_point().xy(), 2.0, 0.0));
            let top = h.end_point();
            assert!((top.z - 6.0).abs() < 1e-9 && near(top.xy(), 1.0, 0.0));
        }
        k => panic!("{k:?}"),
    }
    assert!(s.execute("helix", &json!({"center": [0, 0], "baseRadius": 1, "turns": 1e9})).is_err());
    s.cmdline("helix 0,0 1 2 t 4 w cw 5").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    match last(&s) {
        EntityKind::Helix(h) => {
            let top = h.end_point();
            assert!((top.z - 5.0).abs() < 1e-9 && near(top.xy(), 2.0, 0.0));
            assert!(!h.ccw && h.point_at(0.05).y < 0.0, "clockwise twist");
        }
        k => panic!("{k:?}"),
    }
}

#[test]
fn spline_cv_json_and_command_line() {
    let mut s = Session::new();
    s.execute("spline.cv", &json!({"control": [[0, 0], [1, 2], [3, 2], [4, 0]], "degree": 2})).unwrap();
    assert!(matches!(last(&s), EntityKind::Spline(sp) if sp.degree == 2 && sp.fit.is_empty()));
    assert!(s.execute("spline.cv", &json!({"control": [[0, 0]]})).is_err());
    s.cmdline("spline.cv d 3 0,0 1,1 2,0 3,1").unwrap();
    s.cmdline("").unwrap();
    assert!(matches!(last(&s), EntityKind::Spline(sp) if sp.control.len() == 4 && sp.degree == 3));
    s.cmdline("spline.cv 0,0 1,1 2,0 c").unwrap();
    assert!(matches!(last(&s), EntityKind::Spline(sp) if sp.closed));
}

// ---------------- CENTER MARK / CENTER LINE ----------------

#[test]
fn centermark_and_centerline() {
    let mut s = Session::new();
    let c = h(&s.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap());
    let r = s.execute("centermark", &json!({"handle": c})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 2);
    let e = (**s.doc().unwrap().model.last().unwrap()).clone();
    assert_eq!(e.common.linetype, "CENTER");
    assert!(s.doc().unwrap().linetype("CENTER").is_some());
    assert!(matches!(e.kind, EntityKind::Line(l) if (l.b.y - 2.12).abs() < 1e-9));
    let a = s.execute("line", &json!({"points": [[10, 0], [20, 0]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    let b = s.execute("line", &json!({"points": [[20, 4], [10, 4]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    s.execute("centerline", &json!({"h1": a, "h2": b})).unwrap();
    assert!(matches!(last(&s), EntityKind::Line(l) if (l.a.y - 2.0).abs() < 1e-9 && (l.a.x - 9.88).abs() < 1e-9));
    assert!(s.execute("centerline", &json!({"h1": a, "h2": c})).is_err());
    assert!(s.execute("centermark", &json!({"handle": a})).is_err());

    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap();
    s.execute("arc", &json!({"center": [10, 0], "radius": 1, "start": 0, "end": 90})).unwrap();
    s.cmdline("centermark 2,0 11,0").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(kinds(&s).iter().filter(|k| **k == "Line").count(), 4);
    s.execute("line", &json!({"points": [[0, 10], [10, 10]]})).unwrap();
    s.execute("line", &json!({"points": [[0, 12], [10, 12]]})).unwrap();
    s.cmdline("centerline 5,10 5,12").unwrap();
    assert!(s.running.is_none());
    assert!(matches!(last(&s), EntityKind::Line(l) if (l.a.y - 11.0).abs() < 1e-9));
}

#[test]
fn new_draw_commands_survive_garbage_input() {
    let ids = [
        "arc.sce",
        "arc.sca",
        "arc.scl",
        "arc.sea",
        "arc.sed",
        "arc.ser",
        "arc.cse",
        "arc.csa",
        "arc.csl",
        "arc.continue",
        "circle.cd",
        "circle.2p",
        "circle.3p",
        "circle.ttr",
        "circle.ttt",
        "ellipse.axis",
        "ellipse.arc",
        "divide",
        "measure",
        "revcloud",
        "revcloud.polygonal",
        "revcloud.freehand",
        "wipeout",
        "breakline",
        "3dpoly",
        "mline",
        "helix",
        "spline.cv",
        "centermark",
        "centerline",
        "xline",
        "rectang",
    ];
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    for id in ids {
        for p in [
            json!({"start": [0, 0], "center": [0, 0], "end": [0, 0], "angle": 1e308, "length": -1e308, "radius": 0, "direction": [0, 0]}),
            json!({"p1": [0, 0], "p2": [0, 0], "p3": [0, 0], "distance": -0.0, "diameter": 1e308, "radius": f64::MAX}),
            json!({"handle": "100", "segments": 1e18, "length": 1e-320, "block": ""}),
            json!({"points": [[0, 0], [1e308, 1e308], [-1e308, 1e308]], "arcLength": 1e-300, "scale": 1e308}),
            json!({"center": [0, 0], "baseRadius": 1e308, "turns": 499, "height": 1e308}),
            json!({"control": [[0, 0], [0, 0]], "degree": 1e9}),
            json!({"p1": [0, 0], "dimensions": [1e308, -1e308], "rotation": 1e308, "area": 1e308, "length": 1e-308}),
            json!({"h1": "100", "p1": [0, 0], "h2": "100", "p2": [0, 0], "h3": "100", "p3": [0, 0], "radius": 1}),
            json!({"vertex": [0, 0], "start": [0, 0], "end": [0, 0]}),
        ] {
            let _ = s.execute(id, &p);
        }
        // Interactive: garbage text and points.
        s.start(id).unwrap();
        for t in ["", "x", "1e999", "-5", "0,0", "0,0", "nan", "b", "a", "1", "2,2", ""] {
            if s.running.is_none() {
                break;
            }
            let _ = s.cmdline(t);
        }
        let _ = s.preview(Vec2::new(1e300, -1e300));
        s.cancel();
        let _ = s.input(Input::Cancel);
    }
}

#[test]
fn arc_command_options() {
    for (cmd, center) in [
        ("arc 1,0 0,1 -1,0", (0.0, 0.0)),
        ("arc c 0,0 1,0 0,1", (0.0, 0.0)),
        ("arc c 0,0 1,0 a 90", (0.0, 0.0)),
        ("arc c 0,0 1,0 l 1.41421356237", (0.0, 0.0)),
        ("arc 1,0 c 0,0 0,1", (0.0, 0.0)),
        ("arc 1,0 c 0,0 a 90", (0.0, 0.0)),
        ("arc 1,0 e 0,1 0,0", (0.0, 0.0)),
        ("arc 1,0 e 0,1 a 90", (0.0, 0.0)),
        ("arc 1,0 e 0,1 d 90", (0.0, 0.0)),
        ("arc 1,0 e 0,1 r 1", (0.0, 0.0)),
    ] {
        let mut s = Session::new();
        s.cmdline(cmd).unwrap();
        assert!(s.running.is_none(), "{cmd}: {}", s.prompt_text());
        let a = as_arc(&last(&s));
        assert!(near(a.center, center.0, center.1) && (a.radius - 1.0).abs() < 1e-6, "{cmd}: {a:?}");
    }
    // Enter at the first prompt continues from the last line.
    let mut s = Session::new();
    s.cmdline("line 0,0 1,0").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("arc").unwrap();
    s.cmdline("").unwrap();
    assert!(s.prompt_text().contains("end point"), "{}", s.prompt_text());
    s.cmdline("2,1").unwrap();
    assert!(near(as_arc(&last(&s)).center, 1.0, 1.0));
    assert!(s.prompt_text() == "Command:");
}
