//! -UNITS asks for the drawing's units at the command line, as AutoCAD 2024 asks (measured with
//! accoreconsole, running these scripts from these settings): length format and precision, angle
//! format and precision, the direction of angle 0, and whether angles turn clockwise.

use std::f64::consts::FRAC_PI_2;

use cadcraft_engine::Session;

/// LUNITS, LUPREC, AUNITS, AUPREC, ANGBASE (radians; AutoCAD's 1.570796326794896 is pi/2), ANGDIR.
type Units = (i64, i64, i64, i64, f64, i64);

const DEFAULT: Units = (2, 4, 0, 0, 0.0, 0);

fn session(u: Units) -> Session {
    let mut s = Session::new();
    let h = &mut s.doc_mut().unwrap().header;
    h.set_i64("LUNITS", u.0);
    h.set_i64("LUPREC", u.1);
    h.set_i64("AUNITS", u.2);
    h.set_i64("AUPREC", u.3);
    h.set_f64("ANGBASE", u.4);
    h.set_i64("ANGDIR", u.5);
    s
}

fn units(s: &Session) -> Units {
    let h = &s.doc().unwrap().header;
    (h.i64("LUNITS", 2), h.i64("LUPREC", 4), h.i64("AUNITS", 0), h.i64("AUPREC", 0), h.f64("ANGBASE", 0.0), h.i64("ANGDIR", 0))
}

fn same(a: Units, b: Units) -> bool {
    (a.0, a.1, a.2, a.3, a.5) == (b.0, b.1, b.2, b.3, b.5) && (a.4 - b.4).abs() <= 1e-12
}

fn run(s: &mut Session, answers: &str) {
    s.cmdline(&format!("(command \"_.-UNITS\" {answers})")).unwrap();
    assert!(s.running.is_none(), "{answers}: {}", s.prompt_text());
}

#[test]
fn the_measured_scripts_set_the_units() {
    let cases: [(Units, &str, Units); 21] = [
        (DEFAULT, "4 16 1 0 0 \"_N\"", (4, 4, 0, 0, 0.0, 0)),
        (DEFAULT, "3 2 2 4 90 \"_Y\"", (3, 2, 1, 4, FRAC_PI_2, 1)),
        (DEFAULT, "2 3 3 2 100 \"_N\"", (2, 3, 2, 2, FRAC_PI_2, 0)),
        (DEFAULT, "5 8 5 2 \"N\" \"_N\"", (5, 3, 4, 2, FRAC_PI_2, 0)),
        ((4, 3, 1, 2, 0.5, 1), "\"\" \"\" \"\" \"\" \"\" \"\"", (4, 3, 1, 2, 0.5, 1)),
        ((2, 4, 0, 0, FRAC_PI_2, 1), "2 4 1 0 45 \"_N\"", (2, 4, 0, 0, 5.497787143782137, 0)),
        (DEFAULT, "1 2 4 3 30 \"_N\"", (1, 2, 3, 3, 4.867258771281656, 0)),
        ((2, 2, 0, 2, 0.0, 0), "3 \"\" 2 \"\" \"\" \"\"", (3, 4, 1, 4, 0.0, 0)),
        ((2, 2, 0, 2, 0.0, 0), "2 \"\" 1 \"\" \"\" \"\"", (2, 2, 0, 2, 0.0, 0)),
        (DEFAULT, "4 3 \"\" \"\" \"\" \"\" \"\"", (4, 4, 0, 0, 0.0, 0)),
        ((2, 4, 0, 0, 0.0, 1), "2 4 5 4 \"N45dE\" \"\"", (2, 4, 4, 4, 5.497787143782137, 1)),
        (DEFAULT, "6 \"\" \"\" \"\" \"\" \"\" \"\"", (2, 4, 0, 0, 0.0, 0)),
        ((2, 2, 0, 0, 0.0, 0), "4 \"\" \"\" \"\" \"\" \"\"", (4, 4, 0, 0, 0.0, 0)),
        ((5, 6, 0, 0, 0.0, 0), "\"\" \"\" \"\" \"\" \"\" \"\"", (5, 6, 0, 0, 0.0, 0)),
        ((2, 4, 0, 0, 0.0, 1), "\"\" \"\" 5 \"\" \"N\" \"\"", (2, 4, 4, 4, 4.712388980384689, 1)),
        ((2, 4, 3, 4, 0.0, 0), "\"\" \"\" \"\" \"\" \"45d\" \"\"", (2, 4, 3, 4, 0.7853981633974482, 0)),
        (DEFAULT, "5 1 \"\" \"\" \"\" \"\"", (5, 0, 0, 0, 0.0, 0)),
        ((1, 7, 0, 7, 0.0, 0), "3 \"\" 1 \"\" \"\" \"\"", (3, 4, 0, 7, 0.0, 0)),
        (DEFAULT, "\"\" \"\" \"\" \"\" \"\" \"Yes\"", (2, 4, 0, 0, 0.0, 1)),
        ((2, 4, 2, 6, 0.0, 0), "\"\" \"\" \"\" \"\" \"\" \"\"", (2, 4, 2, 6, 0.0, 0)),
        ((2, 4, 4, 2, 0.0, 1), "\"\" \"\" \"\" \"\" \"S45dE\" \"\"", (2, 4, 4, 2, 0.7853981633974482, 1)),
    ];
    for (from, answers, to) in cases {
        let mut s = session(from);
        run(&mut s, answers);
        assert!(same(units(&s), to), "{answers}: {:?}, AutoCAD {to:?}", units(&s));
    }
}

/// The prompt, after the command's name.
fn asks(s: &Session) -> String {
    let p = s.prompt_text();
    p.strip_prefix("-UNITS ").unwrap_or(&p).to_string()
}

fn ends_with(s: &Session, lines: &[&str]) -> bool {
    s.log.len() >= lines.len() && s.log[s.log.len() - lines.len()..].iter().zip(lines).all(|(a, b)| a == b)
}

#[test]
fn prompts_and_listings_show_the_drawings_units() {
    // Architectural to 1/8, degrees/minutes/seconds to 2 places, angle 0 at 0.5 radians, clockwise.
    let mut s = session((4, 3, 1, 2, 0.5, 1));
    s.cmdline("-units").unwrap();
    assert!(s.log.iter().any(|l| l == "  4.  Architectural   1'-3 1/2\""), "{:?}", s.log);
    assert_eq!(asks(&s), "Enter choice, 1 to 5 <4>:");
    s.cmdline("").unwrap();
    assert!(ends_with(&s, &["Enter denominator of smallest fraction to display"]), "{:?}", s.log);
    assert_eq!(asks(&s), "(1, 2, 4, 8, 16, 32, 64, 128, or 256) <8>:");
    s.cmdline("").unwrap();
    assert!(ends_with(&s, &["  4.  Radians                   0.7854r", "  5.  Surveyor's units          N 45d0'0\" E"]), "{:?}", s.log);
    assert_eq!(asks(&s), "Enter choice, 1 to 5 <2>:");
    s.cmdline("").unwrap();
    assert_eq!(asks(&s), "Enter number of fractional places for display of angles (0 to 8) <2>:");
    s.cmdline("").unwrap();
    let directions = [
        "Direction for angle 0d0':",
        "   East    3 o'clock  =  0d0'",
        "   North  12 o'clock  =  270d0'",
        "   West    9 o'clock  =  180d0'",
        "   South   6 o'clock  =  90d0'",
    ];
    assert!(ends_with(&s, &directions), "{:?}", s.log);
    assert_eq!(asks(&s), "Enter direction for angle 0d0' <331d21'>:");
    s.cmdline("").unwrap();
    assert_eq!(asks(&s), "Measure angles clockwise? [Yes/No] <Y>:");
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert!(same(units(&s), (4, 3, 1, 2, 0.5, 1)), "{:?}", units(&s));

    // Surveyor's units, clockwise: north is listed as S, as AutoCAD lists it.
    let mut s = session((2, 4, 0, 0, 0.0, 1));
    s.script(
        "-units


5

",
    )
    .unwrap();
    let directions = [
        "Direction for angle E:",
        "   East    3 o'clock  =  E",
        "   North  12 o'clock  =  S",
        "   West    9 o'clock  =  W",
        "   South   6 o'clock  =  N",
    ];
    assert!(ends_with(&s, &directions), "{:?}", s.log);
    assert_eq!(asks(&s), "Enter direction for angle E <E>:");

    // The listing shows directions to the precision just given.
    let mut s = session(DEFAULT);
    s.script(
        "-units


2
2
",
    )
    .unwrap();
    let directions = [
        "Direction for angle 0d0':",
        "   East    3 o'clock  =  0d0'",
        "   North  12 o'clock  =  90d0'",
        "   West    9 o'clock  =  180d0'",
        "   South   6 o'clock  =  270d0'",
    ];
    assert!(ends_with(&s, &directions), "{:?}", s.log);
    assert_eq!(asks(&s), "Enter direction for angle 0d0' <0d0'>:");
}

#[test]
fn a_rejected_answer_is_asked_again() {
    let mut s = session(DEFAULT);
    s.cmdline("-units").unwrap();
    s.cmdline("6").unwrap();
    assert!(ends_with(&s, &["Requires an integer between 1 and 5."]), "{:?}", s.log);
    s.cmdline("2.5").unwrap();
    assert!(ends_with(&s, &["Requires an integer value."]), "{:?}", s.log);
    assert_eq!(asks(&s), "Enter choice, 1 to 5 <2>:");
    s.cmdline("4").unwrap();
    s.cmdline("3").unwrap();
    assert!(ends_with(&s, &["Invalid choice.", "Enter denominator of smallest fraction to display"]), "{:?}", s.log);
    assert_eq!(asks(&s), "(1, 2, 4, 8, 16, 32, 64, 128, or 256) <16>:");
    s.script(
        "

9
",
    )
    .unwrap();
    assert!(ends_with(&s, &["Requires an integer between 0 and 8."]), "{:?}", s.log);
    s.script(
        "
abc
",
    )
    .unwrap();
    assert!(ends_with(&s, &["Requires valid numeric angle or two points."]), "{:?}", s.log);
    s.script(
        "

",
    )
    .unwrap();
    assert!(s.running.is_none());
    assert!(same(units(&s), (4, 4, 0, 0, 0.0, 0)), "{:?}", units(&s));
}

#[test]
fn each_answer_is_kept_as_it_is_given() {
    // Esc after the angle format: the answers before it are kept, and the new angle format's
    // precision is reset to 4.
    let mut s = session(DEFAULT);
    s.script(
        "-units
4
16
2
",
    )
    .unwrap();
    s.cancel();
    assert!(s.running.is_none());
    assert!(same(units(&s), (4, 4, 1, 4, 0.0, 0)), "{:?}", units(&s));
    let mut s = session(DEFAULT);
    s.script(
        "-units
3
2
3
5
90
",
    )
    .unwrap();
    s.cancel();
    assert!(same(units(&s), (3, 2, 2, 5, 1.413716694115407, 0)), "{:?}", units(&s));
}
