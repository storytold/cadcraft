//! -ARRAY, the classic command-line array, as AutoCAD 2024 runs it from AutoLISP's `command`
//! (accoreconsole, every copy read back with `entget`).

use std::f64::consts::TAU;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{EntityCodec, set_entity_codec};
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

/// A session with the object `made` (an entity list) as `a` in LISP, after `code`.
fn run(made: &str, code: &str) -> Session {
    // `entmake` goes through the DXF codec, as in the app.
    set_entity_codec(EntityCodec { groups: cadcraft_io::entity_groups, entity: cadcraft_io::entity_from_groups });
    let mut s = Session::new();
    s.cmdline(&format!("(progn (setq a (progn (entmake '({made})) (entlast))) {code})")).unwrap();
    s
}

/// Where each object is: a line's start, a circle's or arc's centre, a polyline's first vertex, a
/// text's insertion point.
fn points(s: &Session) -> Vec<[f64; 2]> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .map(|e| match &e.kind {
            EntityKind::Line(l) => [l.a.x, l.a.y],
            EntityKind::Circle(c) => [c.center.x, c.center.y],
            EntityKind::Arc(a) => [a.center.x, a.center.y],
            EntityKind::LwPolyline(p) => [p.vertices[0].p.x, p.vertices[0].p.y],
            EntityKind::Text(t) => [t.insert.x, t.insert.y],
            k => panic!("unexpected {k:?}"),
        })
        .collect()
}

/// `got` holds each of `want` to `tol`, and nothing else.
fn same(got: &[[f64; 2]], want: &[[f64; 2]], tol: f64) -> bool {
    got.len() == want.len() && want.iter().all(|w| got.iter().any(|g| (g[0] - w[0]).abs() <= tol && (g[1] - w[1]).abs() <= tol))
}

fn line(a: [f64; 2], b: [f64; 2]) -> String {
    format!("(0 . \"LINE\") (10 {} {} 0.0) (11 {} {} 0.0)", a[0], a[1], b[0], b[1])
}

fn circle(c: [f64; 2], r: f64) -> String {
    format!("(0 . \"CIRCLE\") (10 {} {} 0.0) (40 . {r})", c[0], c[1])
}

fn arc(c: [f64; 2], r: f64, a1: f64, a2: f64) -> String {
    format!("(0 . \"ARC\") (10 {} {} 0.0) (40 . {r}) (50 . {}) (51 . {})", c[0], c[1], a1.to_radians(), a2.to_radians())
}

/// The measured polyline: four vertices with bulges 0.3, 0, −0.7, 0, from `at`, `scale` × the
/// drawn size, `width` wide.
fn polyline(at: [f64; 2], scale: f64, width: f64) -> String {
    let v = [(0.0, 0.0, 0.3), (20.0, 5.0, 0.0), (35.0, -10.0, -0.7), (50.0, 0.0, 0.0)];
    let vs: String = v.iter().map(|(x, y, b)| format!(" (10 {} {}) (42 . {b})", at[0] + x * scale, at[1] + y * scale)).collect();
    format!("(0 . \"LWPOLYLINE\") (90 . 4) (70 . 0) (43 . {width}){vs}")
}

#[test]
fn rectangular_copies_move_by_rows_and_columns() {
    let s = run(
        &line([-1.1929712440222175, 9.67888030686079], [44.51297824124873, 11.673136668914886]),
        "(command \"_.-ARRAY\" a \"\" \"_R\" 2 2 -12.570757111654931 -18.0084920904546)",
    );
    let want = [
        [-19.20146333447681, -2.891876804794138],
        [-19.20146333447681, 9.67888030686079],
        [-1.192971244022217, -2.891876804794138],
        [-1.192971244022217, 9.67888030686079],
    ];
    assert!(same(&points(&s), &want, 1e-9), "{:?}", points(&s));
    // One row or one column: that distance isn't asked for.
    let s = run(&circle([5.801226536663419, -5.877116775641122], 23.343762414841144), "(command \"_.-ARRAY\" a \"\" \"_R\" 1 3 -26.90504352387681)");
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let want = [[-48.0088605110902, -5.877116775641122], [-21.10381698721339, -5.877116775641122], [5.801226536663419, -5.877116775641122]];
    assert!(same(&points(&s), &want, 1e-9), "{:?}", points(&s));
    let s = run(&polyline([0.5, 0.25], 0.001, 0.0005), "(command \"_.-ARRAY\" a \"\" \"_R\" 3 1 -0.010095474538067209)");
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(same(&points(&s), &[[0.5, 0.2298090509238659], [0.5, 0.239904525461933], [0.5, 0.25]], 1e-12), "{:?}", points(&s));
    let s = run(
        "(0 . \"TEXT\") (10 5.4185272544331635 4.443702697220587 0.0) (40 . 2.5) (1 . \"AB12\") (50 . 0.20972899)",
        "(command \"_.-ARRAY\" a \"\" \"_R\" 1 4 26.03962020976745)",
    );
    let want = [
        [83.53738788373552, 4.443702697220587],
        [57.49776767396807, 4.443702697220587],
        [31.45814746420061, 4.443702697220587],
        [5.418527254433163, 4.443702697220587],
    ];
    assert!(same(&points(&s), &want, 1e-9), "{:?}", points(&s));
}

#[test]
fn polar_copies_turn_about_the_centre() {
    // Five items around a full circle: 72° apart.
    let s = run(
        &arc([8.737608057965836, 8.018370437941456], 25.76749991913238, 246.54512163341687, 4.3030133419271355),
        "(command \"_.-ARRAY\" a \"\" \"_P\" '(10.631794214160564 -5.07952370466397) 5 360 \"_Y\")",
    );
    let want = [
        [22.50329607545565, 0.769426262850251],
        [19.86297201814329, -14.56256796884801],
        [4.465473991920418, -16.78931734426366],
        [-2.410379072682379, -2.833529910999876],
        [8.737608057965836, 8.018370437941456],
    ];
    assert!(same(&points(&s), &want, 1e-9), "{:?}", points(&s));
    let starts: Vec<f64> =
        s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Arc(a) = &e.kind { Some(a.start) } else { None }).collect();
    for want in [3.046387065797107, 1.789750004361188, 0.5331129429252733, 5.559661188668941, 4.303024127233024] {
        assert!(starts.iter().any(|a| ((a - want).rem_euclid(TAU) + 1e-9).rem_euclid(TAU) < 2e-9), "{want}: {starts:?}");
    }
    // Six items in 270°: 54° apart.
    let s = run(
        &line([14.349623930607876, -18.237839127538827], [57.5895083326729, 8.515990272720988]),
        "(command \"_.-ARRAY\" a \"\" \"_P\" '(9.968618268950948 -10.513614962917108) 6 270 \"_Y\")",
    );
    let want = [
        [2.244394104329227, -14.89462062457403],
        [1.884125186851836, -6.839676863508278],
        [8.18895082173591, -1.813636035924282],
        [15.96098679213407, -3.960114445242588],
        [18.79273740462331, -11.50949197954958],
        [14.34962393060787, -18.23783912753882],
    ];
    assert!(same(&points(&s), &want, 1e-9), "{:?}", points(&s));
}

#[test]
fn polar_copies_not_rotated_move_with_the_group_10_point() {
    // A polyline moves with its first vertex: three items in 270°.
    let s = run(
        &polyline([612345.678, 2398765.432], 1.0, 0.5),
        "(command \"_.-ARRAY\" a \"\" \"_P\" '(612362.9804954464 2398768.822523769) 3 270 \"_N\")",
    );
    let want = [[612359.5899716774, 2398786.125019215], [612377.612669657, 2398758.985274256], [612345.678, 2398765.432]];
    assert!(same(&points(&s), &want, 1e-6), "{:?}", points(&s));
    // A circle with its centre, five items in −90° (clockwise).
    let s = run(
        &circle([612340.715727557, 2398755.830080357], 26.60109393014347),
        "(command \"_.-ARRAY\" a \"\" \"_P\" '(612346.260090821 2398771.804712544) 5 -90 \"_N\")",
    );
    let want = [
        [612330.2854586337, 2398777.349075808],
        [612329.3797191395, 2398770.813809208],
        [612331.0438632132, 2398764.429398659],
        [612335.0245400043, 2398759.16781279],
        [612340.715727557, 2398755.830080357],
    ];
    assert!(same(&points(&s), &want, 1e-6), "{:?}", points(&s));
    // An arc with its centre, its angles unchanged.
    let s = run(
        &arc([612339.185864084, 2398766.23253733], 13.970352776505141, 238.67958380809327, 80.40871664680844),
        "(command \"_.-ARRAY\" a \"\" \"_P\" '(612358.7800675274 2398749.633182222) 7 360 \"_N\")",
    );
    let want = [
        [612359.541179917, 2398775.302075975],
        [612379.3233625971, 2398765.042414074],
        [612383.6360250826, 2398743.179286296],
        [612369.2316445602, 2398726.176073787],
        [612346.957013359, 2398726.836542366],
        [612333.585383093, 2398744.663345723],
        [612339.185864084, 2398766.23253733],
    ];
    assert!(same(&points(&s), &want, 1e-6), "{:?}", points(&s));
    assert!(s.doc().unwrap().model.iter().all(|e| matches!(&e.kind, EntityKind::Arc(a) if (a.start - 4.165744594740973).abs() < 1e-9)));
}

#[test]
fn the_type_asked_defaults_to_the_one_used_last() {
    let mut s = run(&circle([0.0, 0.0], 1.0), "(command \"_.-ARRAY\" a \"\" \"_P\" '(10.0 0.0) 3 360 \"_Y\")");
    s.cmdline("(command \"_.-ARRAY\" a \"\")").unwrap();
    assert!(s.prompt_text().contains("[Rectangular/Polar] <P>"), "{}", s.prompt_text());
}

#[test]
fn classic_array_by_json() {
    let mut s = run(&circle([0.0, 0.0], 1.0), "");
    let first = s.doc().unwrap().model.iter().next().unwrap().handle.hex();
    let r = s.execute("-array", &json!({ "handles": [first], "type": "polar", "center": [10, 0], "count": 4, "rotate": false })).unwrap();
    assert_eq!(r["created"].as_u64(), Some(3), "{r}");
    assert!(same(&points(&s), &[[0.0, 0.0], [10.0, -10.0], [20.0, 0.0], [10.0, 10.0]], 1e-9), "{:?}", points(&s));
    let r = s.execute("-array", &json!({ "handles": [first], "rows": 2, "cols": 3, "rowSpacing": 5, "colSpacing": 7 })).unwrap();
    assert_eq!(r["created"].as_u64(), Some(5), "{r}");
    // An unknown type, or an array past MAXARRAY, is refused before anything is made.
    assert!(s.execute("-array", &json!({ "handles": [first], "type": "path" })).is_err());
    assert!(s.execute("-array", &json!({ "handles": [first], "rows": 4294967296u64, "cols": 4294967296u64 })).is_err());
    assert!(s.execute("-array", &json!({ "handles": [first], "type": "polar", "center": [0, 0], "count": u64::MAX })).is_err());
    assert_eq!(s.doc().unwrap().model.iter().count(), 4 + 5);
}
