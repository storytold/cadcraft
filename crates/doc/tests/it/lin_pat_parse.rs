//! Reading the user's own `.lin` and `.pat` files (#552). Every file here is a tiny synthetic one
//! written for these tests, with our own names.

use cadcraft_doc::library::files::{MAX_LINETYPE_ELEMENTS, MAX_PATTERN_DASHES, MAX_PATTERN_FAMILIES, parse_lin, parse_pat};

const LIN: &str = "\
;; my own linetypes
*MYDASH,My dash __ __ __
A,.5,-.25

*MYDOTS
A,0,-.1,0,-.2
*MYTEXT,Text line --MY--MY--
A,.5,-.2,[\"MY\",STANDARD,S=.1,R=0.0,X=-0.1,Y=-.05],-.2
*MYSHAPE,Shape line
A,.25,-.1,[MYBOX,myshapes.shx,S=.1],-.1,1
";

#[test]
fn simple_and_text_linetypes_are_read() {
    let r = parse_lin(LIN);
    let names: Vec<&str> = r.items.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["MYDASH", "MYDOTS", "MYTEXT", "MYSHAPE"]);
    let dash = &r.items[0];
    assert_eq!(dash.description, "My dash __ __ __");
    assert_eq!(dash.pattern.iter().map(|e| e.length).collect::<Vec<_>>(), [0.5, -0.25]);
    assert_eq!(r.items[1].description, "");
    assert_eq!(r.items[1].pattern.len(), 4);

    // The text belongs to the gap before it: drawn at that gap's end.
    let text = &r.items[2].pattern;
    assert_eq!(text.len(), 3);
    assert_eq!(text[1].length, -0.2);
    assert_eq!(text[1].text.as_deref(), Some("MY"));
    assert_eq!(text[1].style.as_deref(), Some("STANDARD"));
    assert!((text[1].scale - 0.1).abs() < 1e-12);
    assert!((text[1].offset.x + 0.1).abs() < 1e-12 && (text[1].offset.y + 0.05).abs() < 1e-12);
    assert!(!text[1].absolute);
    assert!(text[2].text.is_none());

    // Shapes need SHX files: left out with a note, the dashes kept.
    let shape = &r.items[3].pattern;
    assert_eq!(shape.iter().map(|e| e.length).collect::<Vec<_>>(), [0.25, -0.1, -0.1, 1.0]);
    assert!(shape.iter().all(|e| e.text.is_none() && e.shape.is_none()));
    assert!(r.warnings.iter().any(|w| w.contains("MYBOX")), "{:?}", r.warnings);
}

#[test]
fn text_rotation_units_and_absolute_angles() {
    let r = parse_lin("*R1\nA,1,[\"x\",S1,A=90,S=1],-1\n*R2\nA,1,[\"x\",S1,R=1.5r],-1\n*R3\nA,1,[\"a,b]\",S1,U=100g],-1\n");
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    let rot = |i: usize| r.items[i].pattern[0].rotation;
    assert!((rot(0) - std::f64::consts::FRAC_PI_2).abs() < 1e-12 && r.items[0].pattern[0].absolute);
    assert!((rot(1) - 1.5).abs() < 1e-12);
    assert!((rot(2) - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    // Commas and brackets inside the quoted text are text.
    assert_eq!(r.items[2].pattern[0].text.as_deref(), Some("a,b]"));
}

#[test]
fn broken_linetypes_are_skipped_with_warnings() {
    let text = "\
*NOPATTERN
*BADNUM
A,.5,abc
*BADALIGN
B,.5,-.5
*UNCLOSED
A,.5,[\"x\",S1
*
A,1,-1
A,1,-1
*GOOD
A,1,-1
*GOOD
A,2,-2
*ZERO
A,0,0
*NOSTYLE
A,1,[\"x\"],-1
";
    let r = parse_lin(text);
    assert_eq!(r.items.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["GOOD"]);
    assert_eq!(r.items[0].pattern[0].length, 1.0, "the first definition is kept");
    for what in ["NOPATTERN", "BADNUM", "BADALIGN", "UNCLOSED", "without a name", "without a *NAME", "defined twice", "ZERO", "NOSTYLE"] {
        assert!(r.warnings.iter().any(|w| w.contains(what)), "no warning about {what}: {:?}", r.warnings);
    }
}

#[test]
fn hostile_linetype_files_never_panic_and_stay_capped() {
    let long_line = format!("*LONG\nA,{}", vec!["1"; 5000].join(","));
    let many = format!("*MANY\nA,{}", vec!["1,-1"; MAX_LINETYPE_ELEMENTS].join(","));
    let defs: String = (0..5000).map(|i| format!("*L{i}\nA,1,-1\n")).collect();
    let huge = "*HUGE\nA,1e300,-1\n*NAN\nA,NaN,-1\n*INF\nA,inf,-1\n";
    for text in [long_line.as_str(), many.as_str(), defs.as_str(), huge, "", "\u{feff}", "]]][[[\"", "*\u{0}\nA,1", "A,1,-1\n*X,", "*X\nA,[[[[\"]]]"]
    {
        let r = parse_lin(text);
        assert!(r.items.len() <= 4096);
        assert!(r.items.iter().all(|l| l.pattern.len() <= MAX_LINETYPE_ELEMENTS && l.pattern.iter().all(|e| e.length.is_finite())));
        assert!(r.warnings.len() <= 101);
    }
    assert!(parse_lin(huge).items.is_empty());
    assert_eq!(parse_lin(&defs).items.len(), 4096);
}

const PAT: &str = "\
; my own patterns
*MYGRID, My grid
0, 0,0, 0,.5
90, 0,0, 0,.5
*MYBRICK,My brick ; trailing comment
0, 0,0, 0,.25
90, 0,0, .25,.25, .25,-.25
";

#[test]
fn hatch_patterns_are_read() {
    let r = parse_pat(PAT);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(r.items.len(), 2);
    let grid = &r.items[0];
    assert_eq!((grid.name.as_str(), grid.description.as_str()), ("MYGRID", "My grid"));
    assert_eq!(grid.lines.len(), 2);
    assert_eq!(grid.lines[1].angle, 90.0);
    assert_eq!(grid.lines[1].delta, (0.0, 0.5));
    let brick = &r.items[1];
    assert_eq!(brick.description, "My brick");
    assert_eq!(brick.lines[1].origin, (0.0, 0.0));
    assert_eq!(brick.lines[1].dashes, [0.25, -0.25]);
}

#[test]
fn lowercase_pattern_names_are_upper_cased() {
    let r = parse_pat("*mypat\n45,0,0,0,.125\n");
    assert_eq!(r.items[0].name, "MYPAT");
}

#[test]
fn broken_patterns_are_skipped_with_warnings() {
    let text = "\
0,0,0,0,1
*EMPTY
*SHORT
0,0,0
*ZEROGAP
0,0,0,1,0
*BADNUM
0,0,0,0,x
*OK
0,0,0,0,1
*OK
45,0,0,0,1
";
    let r = parse_pat(text);
    assert_eq!(r.items.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["OK"]);
    assert_eq!(r.items[0].lines[0].angle, 0.0, "the first definition is kept");
    for what in ["without a *NAME", "EMPTY", "SHORT", "ZEROGAP", "BADNUM", "defined twice"] {
        assert!(r.warnings.iter().any(|w| w.contains(what)), "no warning about {what}: {:?}", r.warnings);
    }
}

#[test]
fn hostile_pattern_files_never_panic_and_stay_capped() {
    let families = format!("*FAM\n{}", "0,0,0,0,1\n".repeat(MAX_PATTERN_FAMILIES + 5));
    let dashes = format!("*DASH\n0,0,0,0,1,{}\n", vec!["1"; MAX_PATTERN_DASHES + 1].join(","));
    let ok_dashes = format!("*DASHOK\n0,0,0,0,1,{}\n", vec!["1"; MAX_PATTERN_DASHES].join(","));
    let defs: String = (0..5000).map(|i| format!("*P{i}\n0,0,0,0,1\n")).collect();
    let big = "*BIG\n1e300,0,0,0,1\n*NAN\nNaN,0,0,0,1\n";
    for text in [families.as_str(), dashes.as_str(), ok_dashes.as_str(), defs.as_str(), big, "", "*", "***\n,,,,,", "*X\n,,,,,,,,,,"] {
        let r = parse_pat(text);
        assert!(r.items.len() <= 4096);
        assert!(r.items.iter().all(|p| p.lines.len() <= MAX_PATTERN_FAMILIES && p.lines.iter().all(|l| l.dashes.len() <= MAX_PATTERN_DASHES)));
        assert!(r.warnings.len() <= 101);
    }
    assert!(parse_pat(&families).items.is_empty());
    assert!(parse_pat(&dashes).items.is_empty());
    assert_eq!(parse_pat(&ok_dashes).items.len(), 1);
    assert!(parse_pat(big).items.is_empty());
    assert_eq!(parse_pat(&defs).items.len(), 4096);
}
