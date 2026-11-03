//! QLEADER at the command line as AutoCAD 2024 runs it with its default settings (accoreconsole,
//! `(command "_.QLEADER" ...)` on a metric drawing, then `entget` of the LEADER and the MTEXT):
//! up to three points, a text width, lines of text; the text sits DIMGAP past the leader's end
//! with the middle of its first line level with it on the right, of its last line on the left.

use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::json;

/// What QLEADER drew: the leader's vertices, and the text's insertion point, height, width,
/// attachment and contents (rounded to 1e-9).
type Drawn = (Vec<(f64, f64)>, Option<((f64, f64), f64, f64, u8, String)>);

fn drawn(s: &Session) -> Drawn {
    let r = |v: f64| (v * 1e9).round() / 1e9;
    let d = s.doc().unwrap();
    let leader = d
        .model
        .iter()
        .find_map(|e| if let EntityKind::Leader(l) = &e.kind { Some(l.vertices.iter().map(|p| (r(p.x), r(p.y))).collect()) } else { None });
    let text = d.model.iter().find_map(|e| match &e.kind {
        EntityKind::MText(m) => Some(((r(m.insert.x), r(m.insert.y)), r(m.height), r(m.width), m.attach, m.contents.clone())),
        _ => None,
    });
    (leader.unwrap_or_default(), text)
}

/// A drawing with the measured scripts' dimension settings: DIMTAD 1, DIMGAP 0.625, DIMASZ 2.5,
/// DIMSCALE 1 and DIMTXT 2.5.
fn session() -> Session {
    let mut s = Session::new();
    for (name, v) in [("DIMTAD", 1.0), ("DIMGAP", 0.625), ("DIMASZ", 2.5), ("DIMSCALE", 1.0), ("DIMTXT", 2.5)] {
        s.execute("setvar", &json!({ "name": name, "value": v })).unwrap();
    }
    s
}

/// Types QLEADER then each of `inputs`, and checks it ended.
fn qleader(s: &mut Session, inputs: &[&str]) {
    s.cmdline("QLEADER").unwrap();
    for t in inputs {
        s.cmdline(t).unwrap();
    }
    assert!(s.running.is_none(), "{inputs:?}: {}", s.prompt_text());
}

#[test]
fn qleader_places_the_text_as_autocad_does() {
    // (inputs, AutoCAD's leader vertices, text insertion point, width, attachment, contents)
    type Case<'a> = (&'a [&'a str], &'a [(f64, f64)], (f64, f64), f64, u8, &'a str);
    let cases: [Case; 8] = [
        // Three points, the last segment level: no hook line.
        (&["0,0", "20,15", "40,15", "0", "ABC", ""], &[(0.0, 0.0), (20.0, 15.0), (40.0, 15.0)], (40.625, 16.25), 0.0, 1, "ABC"),
        // Two points: a hook line DIMASZ (2.5) long into the end.
        (&["0,0", "20,15", "", "0", "ABC", ""], &[(0.0, 0.0), (17.5, 15.0), (20.0, 15.0)], (20.625, 16.25), 0.0, 1, "ABC"),
        (
            &["0,0", "20,15", "35,30", "20", "AB", "CD", ""],
            &[(0.0, 0.0), (20.0, 15.0), (32.5, 30.0), (35.0, 30.0)],
            (35.625, 31.25),
            20.0,
            1,
            "AB\\PCD",
        ),
        // Running left: the middle of the last line level with the end.
        (&["0,0", "-20,15", "-40,15", "0", "AB", "CD", ""], &[(0.0, 0.0), (-20.0, 15.0), (-40.0, 15.0)], (-40.625, 13.75), 0.0, 9, "AB\\PCD"),
        (&["0,0", "-20,15", "-40,15", "0", "A", "B", "C", ""], &[(0.0, 0.0), (-20.0, 15.0), (-40.0, 15.0)], (-40.625, 13.75), 0.0, 9, "A\\PB\\PC"),
        // A short leader running left: the hook line comes from past the start.
        (&["0.5,0.25", "0.48,0.265", "", "0", "ABC", ""], &[(0.5, 0.25), (2.98, 0.265), (0.48, 0.265)], (-0.145, -0.985), 0.0, 9, "ABC"),
        // Straight up: it counts as running right.
        (&["0,0", "0,20", "", "0", "ABC", ""], &[(0.0, 0.0), (-2.5, 20.0), (0.0, 20.0)], (0.625, 21.25), 0.0, 1, "ABC"),
        // Survey coordinates, running left.
        (
            &["612345.678,2398765.432", "612325.678,2398780.432", "612305.678,2398780.432", "0", "ABC", ""],
            &[(612345.678, 2398765.432), (612325.678, 2398780.432), (612305.678, 2398780.432)],
            (612305.053, 2398779.182),
            0.0,
            9,
            "ABC",
        ),
    ];
    for (inputs, leader, at, width, attach, text) in cases {
        let mut s = session();
        qleader(&mut s, inputs);
        assert_eq!(drawn(&s), (leader.to_vec(), Some((at, 2.5, width, attach, text.to_string()))), "{inputs:?}");
    }
}

#[test]
fn qleader_scales_with_dimscale_and_ignores_dimtad() {
    // DIMGAP 2 and DIMSCALE 2: text 5 high, 4 past the end; the hook line 5 long.
    let mut s = session();
    s.execute("setvar", &json!({ "name": "DIMGAP", "value": 2.0 })).unwrap();
    s.execute("setvar", &json!({ "name": "DIMSCALE", "value": 2.0 })).unwrap();
    qleader(&mut s, &["0,0", "20,15", "35,30", "0", "ABC", ""]);
    let want = vec![(0.0, 0.0), (20.0, 15.0), (30.0, 30.0), (35.0, 30.0)];
    assert_eq!(drawn(&s), (want, Some(((39.0, 32.5), 5.0, 0.0, 1, "ABC".to_string()))));
    // DIMTAD 0 or 1, the same place.
    for tad in [0, 1] {
        let mut s = session();
        s.execute("setvar", &json!({ "name": "DIMTAD", "value": tad })).unwrap();
        qleader(&mut s, &["0,0", "20,15", "40,15", "0", "ABC", ""]);
        assert_eq!(drawn(&s).1, Some(((40.625, 16.25), 2.5, 0.0, 1, "ABC".to_string())), "DIMTAD {tad}");
    }
}

#[test]
fn qleader_prompts_and_its_width() {
    let mut s = session();
    let mut asked = Vec::new();
    s.cmdline("QLEADER").unwrap();
    for t in ["0,0", "20,15", "40,15", "50,15", "ABC", ""] {
        asked.push(s.current_prompt().unwrap().display());
        s.cmdline(t).unwrap();
    }
    assert_eq!(
        asked,
        [
            "Specify first leader point, or [Settings] <Settings>:",
            "Specify next point:",
            "Specify next point:",
            "Specify text width <0>:",
            "Enter first line of annotation text <Mtext>:",
            "Enter next line of annotation text:",
        ]
    );
    // A point gives the width as its distance from the end, so Dynamic Input shows a distance.
    assert_eq!(drawn(&s).1.map(|t| t.2), Some(10.0));
    let mut s = session();
    s.cmdline("QLEADER").unwrap();
    for t in ["0,0", "20,15", "40,15"] {
        s.cmdline(t).unwrap();
    }
    assert!(s.current_prompt().unwrap().distance);
    // Text that isn't a distance is refused and asked again; Enter is no width.
    let _ = s.cmdline("abc");
    assert!(s.log.iter().any(|l| l.contains("Requires numeric distance or two points.")), "{:?}", s.log);
    assert_eq!(s.current_prompt().unwrap().display(), "Specify text width <0>:");
    for t in ["", "ABC", ""] {
        s.cmdline(t).unwrap();
    }
    assert!(s.running.is_none());
    assert_eq!(drawn(&s).1.map(|t| t.2), Some(0.0));
    // A negative width is none. A width isn't remembered: AutoCAD 2024 offers 0 again after 20.
    for width in ["-5", "20"] {
        let mut s = session();
        qleader(&mut s, &["0,0", "20,15", "40,15", width, "ABC", ""]);
        assert_eq!(drawn(&s).1.map(|t| t.2), Some(width.parse::<f64>().unwrap().max(0.0)));
        s.cmdline("QLEADER").unwrap();
        for t in ["0,0", "20,15", "40,15"] {
            s.cmdline(t).unwrap();
        }
        assert_eq!(s.current_prompt().unwrap().display(), "Specify text width <0>:", "{width}");
    }
}

#[test]
fn a_click_at_the_text_prompts_is_no_line_of_text() {
    let mut s = session();
    s.cmdline("QLEADER").unwrap();
    for t in ["0,0", "20,15", "40,15", "0"] {
        s.cmdline(t).unwrap();
    }
    s.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    assert_eq!(s.current_prompt().unwrap().display(), "Enter first line of annotation text <Mtext>:");
    s.cmdline("AB").unwrap();
    s.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    assert_eq!(s.current_prompt().unwrap().display(), "Enter next line of annotation text:");
    s.cmdline("CD").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(drawn(&s).1.map(|t| t.4), Some("AB\\PCD".to_string()));
}

#[test]
fn qleader_without_text_or_points() {
    // Enter at the first line: the leader alone.
    let mut s = session();
    qleader(&mut s, &["0,0", "20,15", "40,15", "0", ""]);
    assert_eq!(drawn(&s), (vec![(0.0, 0.0), (20.0, 15.0), (40.0, 15.0)], None));
    // Sloped, without text: no hook line either (AutoCAD 2024: two vertices).
    let mut s = session();
    qleader(&mut s, &["0,0", "20,15", "", "0", ""]);
    assert_eq!(drawn(&s), (vec![(0.0, 0.0), (20.0, 15.0)], None));
    // Enter after one point: nothing.
    let mut s = session();
    qleader(&mut s, &["0,0", ""]);
    assert_eq!(s.doc().unwrap().model.iter().count(), 0);
    // Settings, typed or by Enter: no dialog yet, the first point is asked again.
    for settings in ["S", ""] {
        let mut s = session();
        s.cmdline("QLEADER").unwrap();
        s.cmdline(settings).unwrap();
        assert!(s.log.iter().any(|l| l == "The QLEADER Settings dialog is not available yet."), "{settings:?}: {:?}", s.log);
        assert_eq!(s.current_prompt().unwrap().display(), "Specify first leader point, or [Settings] <Settings>:", "{settings:?}");
    }
    // LE starts it too.
    let mut s = session();
    s.cmdline("LE").unwrap();
    assert_eq!(s.current_prompt().unwrap().display(), "Specify first leader point, or [Settings] <Settings>:");
}

#[test]
fn qleader_by_json() {
    let mut s = session();
    let r = s.execute("qleader", &json!({ "points": [[0, 0], [20, 15], [35, 30]], "width": 20, "text": "AB\r\nCD" })).unwrap();
    assert_eq!(r["handles"].as_array().map(Vec::len), Some(2));
    let want = vec![(0.0, 0.0), (20.0, 15.0), (32.5, 30.0), (35.0, 30.0)];
    assert_eq!(drawn(&s), (want, Some(((35.625, 31.25), 2.5, 20.0, 1, "AB\\PCD".to_string()))));
    // Text with nothing on its lines is no text: the leader alone, without a hook line.
    for text in ["", "\n", "\r\n\r\n"] {
        let mut s = session();
        let r = s.execute("qleader", &json!({ "points": [[0, 0], [20, 15]], "width": 0, "text": text })).unwrap();
        assert_eq!(r["handles"].as_array().map(Vec::len), Some(1), "{text:?}");
        assert_eq!(drawn(&s), (vec![(0.0, 0.0), (20.0, 15.0)], None), "{text:?}");
    }
    // Empty lines at the end are dropped: running left, the text sits as the command line puts it.
    let mut s = session();
    s.execute("qleader", &json!({ "points": [[0, 0], [-20, 15], [-40, 15]], "text": "AB\n\n" })).unwrap();
    assert_eq!(drawn(&s).1, Some(((-40.625, 13.75), 2.5, 0.0, 9, "AB".to_string())));
    // 2 or 3 points, a width of 0 or more, text as a string.
    for bad in [
        json!({ "points": [[0, 0]] }),
        json!({ "points": [[0, 0], [1, 1], [2, 1], [3, 1]] }),
        json!({ "points": [[0, 0], [1, 1]], "width": -1 }),
        json!({ "points": [[0, 0], [1, 1]], "width": "x" }),
        json!({ "points": [[0, 0], [1, 1]], "text": 5 }),
        json!({ "points": [[0, 0], [1, 1]], "text": ["AB", "CD"] }),
    ] {
        let before = s.doc().unwrap().model.iter().count();
        assert!(s.execute("qleader", &bad).is_err(), "{bad}");
        assert_eq!(s.doc().unwrap().model.iter().count(), before, "{bad}");
    }
}
