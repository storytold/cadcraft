//! RECTANG and POLYGON vertices in AutoCAD 2024's order, and the RECTANG modes it remembers from
//! one rectangle to the next. The scripts and numbers were measured with accoreconsole; the
//! RECTANG scripts reset Chamfer, Fillet, Width, Elevation and Thickness first, but not Rotation.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

type Vertex = (f64, f64, f64);

const RESET: &str = "\"_C\" 0 0 \"_F\" 0 \"_W\" 0 \"_E\" 0 \"_T\" 0";

/// The last polyline's vertices (x, y, bulge).
fn last_polyline(s: &Session) -> Vec<Vertex> {
    let d = s.doc().unwrap();
    match &d.model.iter().last().unwrap().kind {
        EntityKind::LwPolyline(p) => p.vertices.iter().map(|v| (v.p.x, v.p.y, v.bulge)).collect(),
        k => panic!("not a polyline: {k:?}"),
    }
}

fn expect(got: &[Vertex], want: &[Vertex]) {
    assert_eq!(got.len(), want.len(), "{got:?}");
    for (g, w) in got.iter().zip(want) {
        let tol = 1e-9 * (1.0 + w.0.abs().max(w.1.abs()));
        assert!((g.0 - w.0).abs() < tol && (g.1 - w.1).abs() < tol && (g.2 - w.2).abs() < 1e-9, "{got:?}\n!= {want:?}");
    }
}

fn lisp(s: &mut Session, expr: &str) {
    s.cmdline(expr).unwrap();
    assert!(s.running.is_none(), "{expr}: {}", s.prompt_text());
}

fn rectang(s: &mut Session, rest: &str) {
    lisp(s, &format!("(command \"_.RECTANG\" {RESET} {rest})"));
}

#[test]
fn a_rectangle_starts_at_its_first_corner_and_runs_along_x_first() {
    let mut s = Session::new();
    rectang(&mut s, "'(-2.2557479240466947 -9.488221049171962) '(-31.36203661716106 -47.886714708394166)");
    expect(
        &last_polyline(&s),
        &[
            (-2.255747924046694, -9.488221049171962, 0.0),
            (-31.36203661716105, -9.488221049171962, 0.0),
            (-31.36203661716105, -47.88671470839417, 0.0),
            (-2.255747924046694, -47.88671470839417, 0.0),
        ],
    );
}

#[test]
fn chamfers_cut_the_edge_into_each_corner_by_the_first_distance() {
    let mut s = Session::new();
    rectang(&mut s, "\"_C\" 2.2518104656366162 3.766337568886034 '(4.8089419289768 29.66648885915597) '(28.114797694689763 43.85884965263156)");
    expect(
        &last_polyline(&s),
        &[
            (4.8089419289768, 31.91829932479258, 0.0),
            (8.575279497862834, 29.66648885915597, 0.0),
            (25.86298722905314, 29.66648885915597, 0.0),
            (28.11479769468976, 33.432826428042, 0.0),
            (28.11479769468976, 41.60703918699494, 0.0),
            (24.34846012580373, 43.85884965263156, 0.0),
            (7.060752394613416, 43.85884965263156, 0.0),
            (4.8089419289768, 40.09251208374553, 0.0),
        ],
    );
}

#[test]
fn fillet_arcs_start_on_the_edge_into_each_corner() {
    let mut s = Session::new();
    rectang(&mut s, "\"_F\" 0.6042729201106594 '(-13.457476758444333 -29.884431038180395) '(14.673991980287038 27.14695046607133)");
    let b = 0.414213562373095;
    expect(
        &last_polyline(&s),
        &[
            (-13.45747675844433, -29.28015811806973, b),
            (-12.85320383833367, -29.88443103818039, 0.0),
            (14.06971906017638, -29.88443103818039, b),
            (14.67399198028703, -29.28015811806973, 0.0),
            (14.67399198028703, 26.54267754596067, b),
            (14.06971906017638, 27.14695046607133, 0.0),
            (-12.85320383833367, 27.14695046607133, b),
            (-13.45747675844433, 26.54267754596067, 0.0),
        ],
    );
}

#[test]
fn fillet_arcs_turn_clockwise_when_the_second_corner_is_below_the_first() {
    // The second corner down and right: the outline runs clockwise and the bulges are negative
    // (AutoCAD gave -0.414213562373095 for such a rectangle, there also rotated).
    let mut s = Session::new();
    rectang(&mut s, "\"_F\" 1.0 '(0.0 0.0) '(10.0 -5.0)");
    let b = -0.414213562373095;
    expect(
        &last_polyline(&s),
        &[(0.0, -1.0, b), (1.0, 0.0, 0.0), (9.0, 0.0, b), (10.0, -1.0, 0.0), (10.0, -4.0, b), (9.0, -5.0, 0.0), (1.0, -5.0, b), (0.0, -4.0, 0.0)],
    );
}

#[test]
fn the_next_rectangle_keeps_the_rotation_and_lists_the_modes() {
    let mut s = Session::new();
    rectang(&mut s, "'(27.647422687394844 28.925709913163516) \"_R\" 50.50299224048081 '(46.86449612780332 4.979921576806525)");
    expect(
        &last_polyline(&s),
        &[
            (27.64742268739484, 28.92570991316351, 0.0),
            (23.66889931823719, 24.09886156025833, 0.0),
            (46.86449612780331, 4.979921576806525, 0.0),
            (50.84301949696097, 9.80676992971171, 0.0),
        ],
    );
    // No Rotation option this time: the rectangle still turns by 50.50 degrees.
    rectang(&mut s, "'(14.3947813348612 23.649728466870016) \"_D\" 19.35761675393126 49.775239210761946 '(-4.9628354190700605 -26.12551074389193)");
    assert!(s.log.iter().any(|l| l == "Current rectangle modes:  Rotation=50.50"), "{:?}", s.log);
    // The next Rotation prompt offers it as its default, as AutoCAD shows it.
    s.cmdline("RECTANG 0,0 R").unwrap();
    assert!(s.prompt_text().ends_with("Specify rotation angle or [Pick points] <50.50>:"), "{}", s.prompt_text());
    s.cancel();
    expect(
        &last_polyline(&s),
        &[
            (14.3947813348612, 23.64972846687001, 0.0),
            (2.082603004810238, 8.712272486646885, 0.0),
            (40.4920546468795, -22.94666721518322, 0.0),
            (52.80423297693047, -8.009211234960095, 0.0),
        ],
    );
}

#[test]
fn elevation_thickness_and_a_picked_rotation_carry_over_together() {
    let mut s = Session::new();
    rectang(
        &mut s,
        "\"_E\" 7.5 \"_T\" 2.25 '(-18.42481827666694 -20.50343704206707) \"_R\" \"_P\" '(0.0 0.0) '(10.0 6.0) '(10.197290578707722 25.5222006119985)",
    );
    expect(
        &last_polyline(&s),
        &[
            (-18.42481827666694, -20.50343704206706, 0.0),
            (22.92627837613747, 4.307220949615576, 0.0),
            (10.19729057870772, 25.5222006119985, 0.0),
            (-31.15380607409669, 0.7115426203158534, 0.0),
        ],
    );
    let d = s.doc().unwrap();
    let e = d.model.iter().last().unwrap();
    let EntityKind::LwPolyline(p) = &e.kind else { panic!("not a polyline") };
    assert_eq!((p.elevation, e.common.thickness), (7.5, 2.25));
    s.cmdline("RECTANG").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("Current rectangle modes:  Elevation=7.5000  Thickness=2.2500  Rotation=30.96"));
    s.cancel();
}

#[test]
fn chamfer_and_fillet_are_remembered_one_at_a_time() {
    let mut s = Session::new();
    rectang(&mut s, "\"_C\" 2.2518104656366162 3.766337568886034 '(0.0 0.0) '(40.0 30.0)");
    rectang(&mut s, "\"_F\" 0.6042729201106594 '(0.0 0.0) '(40.0 30.0)");
    let logs: Vec<&String> = s.log.iter().filter(|l| l.starts_with("Current rectangle modes:")).collect();
    assert_eq!(logs, ["Current rectangle modes:  Chamfer=2.2518 x 3.7663"]);
    s.cmdline("RECTANG").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("Current rectangle modes:  Fillet=0.6043"));
    s.cancel();
}

#[test]
fn setting_a_fillet_clears_the_chamfer_and_back() {
    // A chamfer set after a fillet makes a chamfered rectangle, and the other way round.
    let mut s = Session::new();
    s.script("RECTANG F 1 C 2 3 0,0 40,30\n").unwrap();
    assert!(last_polyline(&s).iter().all(|v| v.2 == 0.0), "{:?}", last_polyline(&s));
    assert_eq!(last_polyline(&s).first().map(|v| (v.0, v.1)), Some((0.0, 2.0)));
    s.script("RECTANG C 2 3 F 1 0,0 40,30\n").unwrap();
    assert!(s.log.iter().any(|l| l == "Current rectangle modes:  Chamfer=2.0000 x 3.0000"), "{:?}", s.log);
    expect(&last_polyline(&s)[..1], &[(0.0, 1.0, 0.414213562373095)]);
    s.cmdline("RECTANG").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("Current rectangle modes:  Fillet=1.0000"));
    s.cancel();
}

#[test]
fn the_json_form_ignores_the_remembered_modes() {
    let mut s = Session::new();
    rectang(&mut s, "'(0.0 0.0) \"_R\" 30 '(10.0 10.0)");
    s.execute("rectang", &json!({ "p1": [100, 0], "p2": [110, 10] })).unwrap();
    expect(&last_polyline(&s), &[(100.0, 0.0, 0.0), (110.0, 0.0, 0.0), (110.0, 10.0, 0.0), (100.0, 10.0, 0.0)]);
}

#[test]
fn a_circumscribed_polygon_starts_counter_clockwise_from_the_picked_edge() {
    let mut s = Session::new();
    lisp(&mut s, "(command \"_.POLYGON\" 4 '(-13.660465775726184 6.051830973104977) \"_C\" '(-33.469717497592136 8.913096552751803))");
    expect(
        &last_polyline(&s),
        &[
            (-36.33098307723896, -10.89615516911414, 0.0),
            (3.287520366492948, -16.61868632840779, 0.0),
            (9.01005152578659, 22.9998171153241, 0.0),
            (-30.60845191794531, 28.72234827461775, 0.0),
        ],
    );
    lisp(&mut s, "(command \"_.POLYGON\" 3 '(-4.866064513702604 -0.5675999154110016) \"_C\" '(-33.004110114447 14.119144448101377))");
    expect(
        &last_polyline(&s),
        &[(-58.44229754982628, -34.61738015807783, 0.0), (51.41002668778618, -29.94108864243579, 0.0), (-7.565922679067647, 62.85566905428061, 0.0)],
    );
}
