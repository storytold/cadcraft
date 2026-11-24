//! SOLID (2D solids) as AutoCAD 2024 draws them from AutoLISP's `command` (accoreconsole, every
//! solid read back with `entget`).

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use serde_json::json;

/// The corners of every solid, in drawing order.
fn solids(s: &Session) -> Vec<[Vec2; 4]> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Solid(so) => Some(so.corners.map(|c| Vec2::new(c.x, c.y))),
            _ => None,
        })
        .collect()
}

#[test]
fn solid_chains_its_corners_as_autocad() {
    let p = |x: f64, y: f64| (x, y);
    // (points, Enters after them, each solid as indexes into the points)
    let cases: [(Vec<(f64, f64)>, usize, Vec<[usize; 4]>); 7] = [
        // Enter at the fourth point: a triangle repeating its third corner.
        (
            vec![p(19.451648928934738, -21.25585202371201), p(34.61693931374715, -3.839951210926415), p(14.55170247613551, -36.00688291519149)],
            1,
            vec![[0, 1, 2, 2]],
        ),
        // Two solids, the second on the first one's last two corners.
        (
            vec![
                p(612336.9748522255, 2398742.6897319513),
                p(612351.3507889743, 2398794.3456611084),
                p(612371.7043989347, 2398771.9517326546),
                p(612379.1755178814, 2398787.9912080113),
                p(612308.5250187548, 2398780.014694856),
                p(612356.1118879627, 2398726.474738389),
            ],
            1,
            vec![[0, 1, 2, 3], [2, 3, 4, 5]],
        ),
        // A solid, then a triangle.
        (
            vec![
                p(15.962585354071173, 3.3526103822549587),
                p(-0.8095657933516947, 13.123790321338404),
                p(-19.412229808271626, -27.60123059493223),
                p(-33.66123919971652, 12.18177290955613),
                p(14.002713873827737, -8.925735622823005),
            ],
            1,
            vec![[0, 1, 2, 3], [2, 3, 4, 4]],
        ),
        // Three solids, the last a triangle.
        (
            vec![
                p(29.3005193409103, -24.874809106460187),
                p(-21.460879071052776, 35.356609837704795),
                p(-31.347336242570112, -21.756568658673665),
                p(-39.08169761088666, 23.588062542950155),
                p(23.0763904721503, 10.219805555956789),
                p(-22.701751810515756, 22.76615204356692),
                p(4.338066830902321, -37.50391578060281),
            ],
            1,
            vec![[0, 1, 2, 3], [2, 3, 4, 5], [4, 5, 6, 6]],
        ),
        // Enter twice: the triangle, then Enter at the third point ends the command.
        (
            vec![p(612325.6793323631, 2398757.396299568), p(612370.1009101832, 2398796.2224137546), p(612378.8199480829, 2398804.1981022176)],
            2,
            vec![[0, 1, 2, 2]],
        ),
        // Corners stay in the order given (a "Z" order makes a rectangle).
        (vec![p(0.0, 0.0), p(30.0, 0.0), p(0.0, 20.0), p(30.0, 20.0)], 1, vec![[0, 1, 2, 3]]),
        // Enter after two points: nothing.
        (vec![p(-34769.59558954922, 81960.0236700466), p(-35927.22173307072, 79264.64372697374)], 1, vec![]),
    ];
    let same = |a: Vec2, b: (f64, f64)| (a.x - b.0).abs() <= 1e-9 * b.0.abs().max(1.0) && (a.y - b.1).abs() <= 1e-9 * b.1.abs().max(1.0);
    for (pts, enters, want) in cases {
        let mut s = Session::new();
        let args: Vec<String> = pts.iter().map(|(x, y)| format!("'({x:?} {y:?})")).chain(std::iter::repeat_n("\"\"".to_string(), enters)).collect();
        s.cmdline(&format!("(command \"_.SOLID\" {})", args.join(" "))).unwrap();
        // One Enter leaves the command at "Specify third point", as AutoCAD's does (its log then
        // shows the measuring routine ending it).
        if s.running.is_some() {
            assert!(s.prompt_text().contains("Specify third point"), "{pts:?}: {}", s.prompt_text());
            s.cancel();
        }
        let got = solids(&s);
        assert_eq!(got.len(), want.len(), "{pts:?}: {got:?}");
        for (g, w) in got.iter().zip(&want) {
            assert!(g.iter().zip(w).all(|(c, &i)| same(*c, pts[i])), "{pts:?}: {got:?}");
        }
    }
}

#[test]
fn solid_json_chains_the_same_way() {
    let mut s = Session::new();
    s.execute("solid", &json!({ "points": [[0, 0], [10, 0], [0, 5], [10, 5], [0, 10]] })).unwrap();
    let got = solids(&s);
    let v = |x: f64, y: f64| Vec2::new(x, y);
    assert_eq!(got, vec![[v(0.0, 0.0), v(10.0, 0.0), v(0.0, 5.0), v(10.0, 5.0)], [v(0.0, 5.0), v(10.0, 5.0), v(0.0, 10.0), v(0.0, 10.0)]]);
    assert!(s.execute("solid", &json!({ "points": [[0, 0], [1, 0]] })).is_err());
    // The alias.
    s.cmdline("so 20,0 30,0 20,5").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(solids(&s).len(), 3);
}
