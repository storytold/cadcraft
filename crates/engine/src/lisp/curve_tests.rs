//! `vlax-curve-*`, file and folder functions, environment variables, `getfiled`, `osnap` and the
//! small helpers.

#![allow(clippy::unwrap_used)]

use cadcraft_doc::{Arc, Circle, Ellipse, EntityKind, Line, LwPolyline, Point, Polyline3d};
use cadcraft_geom::{PolyVertex, Spline, Vec2, Vec3};

use super::value::to_text;
use crate::Session;
use crate::cmd::file::{EntityCodec, set_entity_codec};

fn session() -> Session {
    set_entity_codec(EntityCodec { groups: cadcraft_io::entity_groups, entity: cadcraft_io::entity_from_groups });
    Session::new()
}

fn eval(s: &mut Session, code: &str) -> Result<String, String> {
    match s.lisp_eval(code, false, true, Vec::new()) {
        Ok(Some(Ok(v))) => Ok(to_text(&v, true)),
        Ok(Some(Err(e))) => Err(e),
        Ok(None) => Err("did not finish".into()),
        Err(e) => Err(e.to_string()),
    }
}

fn ok(s: &mut Session, code: &str) -> String {
    match eval(s, code) {
        Ok(v) => v,
        Err(e) => panic!("{code}: {e}"),
    }
}

/// Adds an entity and binds its name to `var`.
fn add(s: &mut Session, var: &str, kind: EntityKind) {
    let h = s.add_entity(kind).unwrap();
    ok(s, &format!("(setq {var} (handent \"{}\"))", h.hex()));
}

fn v3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

/// `code` is T (the checks use `equal` with a fuzz).
fn is_t(s: &mut Session, code: &str) {
    assert_eq!(ok(s, code), "T", "{code}");
}

#[test]
fn a_line_is_measured_by_distance() {
    let mut s = session();
    add(&mut s, "e", EntityKind::Line(Line { a: v3(0.0, 0.0, 0.0), b: v3(3.0, 4.0, 0.0) }));
    is_t(&mut s, "(equal (vlax-curve-getStartParam e) 0.0 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getEndParam e) 5.0 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getDistAtParam e 2.5) 2.5 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getPointAtDist e 2.5) '(1.5 2.0 0.0) 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getStartPoint e) '(0.0 0.0 0.0) 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getEndPoint e) '(3.0 4.0 0.0) 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getClosestPointTo e '(3 0)) '(1.08 1.44 0.0) 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getFirstDeriv e 1.0) '(0.6 0.8 0.0) 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getSecondDeriv e 1.0) '(0.0 0.0 0.0) 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getParamAtPoint e '(1.5 2.0)) 2.5 1e-9)");
    is_t(&mut s, "(equal (vlax-curve-getDistAtPoint e '(1.5 2.0 0.0)) 2.5 1e-9)");
    is_t(&mut s, "(equal (vlax-curve-getArea e) 0.0 1e-12)");
    // Off the line, past its ends: nil.
    assert_eq!(ok(&mut s, "(vlax-curve-getParamAtPoint e '(5 5))"), "nil");
    assert_eq!(ok(&mut s, "(vlax-curve-getPointAtDist e 6)"), "nil");
    assert_eq!(ok(&mut s, "(vlax-curve-getPointAtParam e -1)"), "nil");
    assert_eq!(ok(&mut s, "(vlax-curve-isClosed e)"), "nil");
    assert_eq!(ok(&mut s, "(vlax-curve-isPlanar e)"), "T");
    // Extended, the line goes on past its end.
    is_t(&mut s, "(equal (vlax-curve-getClosestPointTo e '(6 8) T) '(6.0 8.0 0.0) 1e-9)");
    is_t(&mut s, "(equal (vlax-curve-getClosestPointTo e '(6 8)) '(3.0 4.0 0.0) 1e-9)");
}

#[test]
fn an_arc_is_measured_by_angle() {
    let mut s = session();
    let half_pi = std::f64::consts::FRAC_PI_2;
    add(&mut s, "e", EntityKind::Arc(Arc { center: v3(0.0, 0.0, 0.0), radius: 1.0, start: 0.0, end: half_pi }));
    is_t(&mut s, "(equal (vlax-curve-getEndParam e) (/ pi 2) 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getDistAtParam e (vlax-curve-getEndParam e)) (/ pi 2) 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getPointAtDist e (/ pi 4)) (list (sqrt 0.5) (sqrt 0.5) 0.0) 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getParamAtPoint e '(0 1)) (/ pi 2) 1e-9)");
    assert_eq!(ok(&mut s, "(vlax-curve-getParamAtPoint e '(2 2))"), "nil");
    // The chord closes it: a quarter disc less the triangle.
    is_t(&mut s, "(equal (vlax-curve-getArea e) (- (/ pi 4) 0.5) 1e-12)");
    // Below the arc: its nearer end, or the circle it lies on when extended.
    is_t(&mut s, "(equal (vlax-curve-getClosestPointTo e '(0 -2)) '(1.0 0.0 0.0) 1e-9)");
    is_t(&mut s, "(equal (vlax-curve-getClosestPointTo e '(0 -2) T) '(0.0 -1.0 0.0) 1e-9)");
    is_t(&mut s, "(equal (vlax-curve-getFirstDeriv e 0.0) '(0.0 1.0 0.0) 1e-12)");
}

#[test]
fn a_circle_is_closed_and_periodic() {
    let mut s = session();
    add(&mut s, "e", EntityKind::Circle(Circle { center: v3(0.0, 0.0, 0.0), radius: 2.0 }));
    is_t(&mut s, "(equal (vlax-curve-getEndParam e) (* 2 pi) 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getArea e) (* 4 pi) 1e-9)");
    is_t(&mut s, "(equal (vlax-curve-getPointAtParam e (/ pi 2)) '(0.0 2.0 0.0) 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getFirstDeriv e 0.0) '(0.0 2.0 0.0) 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getSecondDeriv e 0.0) '(-2.0 0.0 0.0) 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getDistAtParam e pi) (* 2 pi) 1e-12)");
    assert_eq!(ok(&mut s, "(list (vlax-curve-isClosed e) (vlax-curve-isPeriodic e))"), "(T T)");
    is_t(&mut s, "(equal (vlax-curve-getClosestPointTo e '(5 0)) '(2.0 0.0 0.0) 1e-12)");
}

#[test]
fn a_polyline_is_measured_by_vertex() {
    let mut s = session();
    // A line, then a counter-clockwise half circle from (10,0) to (10,10) round (15,5).
    let vertices =
        vec![PolyVertex::new(Vec2::new(0.0, 0.0)), PolyVertex::with_bulge(Vec2::new(10.0, 0.0), 1.0), PolyVertex::new(Vec2::new(10.0, 10.0))];
    let pl = LwPolyline { vertices, closed: false, const_width: 0.0, elevation: 2.0, plinegen: false };
    add(&mut s, "e", EntityKind::LwPolyline(pl.clone()));
    is_t(&mut s, "(equal (vlax-curve-getEndParam e) 2.0 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getDistAtParam e 2.0) (+ 10 (* 5 pi)) 1e-9)");
    is_t(&mut s, "(equal (vlax-curve-getDistAtParam e 1.5) (+ 10 (* 2.5 pi)) 1e-9)");
    is_t(&mut s, "(equal (vlax-curve-getPointAtParam e 1.5) '(15.0 5.0 2.0) 1e-9)");
    is_t(&mut s, "(equal (vlax-curve-getParamAtDist e 5) 0.5 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getEndPoint e) '(10.0 10.0 2.0) 1e-9)");
    is_t(&mut s, "(equal (vlax-curve-getParamAtPoint e '(15 5 2)) 1.5 1e-9)");
    // The triangle the chord closes, and the half disc outside it.
    is_t(&mut s, "(equal (vlax-curve-getArea e) (+ 50 (* 12.5 pi)) 1e-9)");
    assert_eq!(ok(&mut s, "(vlax-curve-isClosed e)"), "nil");
    // Closed: one more segment, back to the start.
    add(&mut s, "c", EntityKind::LwPolyline(LwPolyline { closed: true, ..pl }));
    is_t(&mut s, "(equal (vlax-curve-getEndParam c) 3.0 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getDistAtParam c 3.0) (+ 10 (* 5 pi) (sqrt 200)) 1e-9)");
    assert_eq!(ok(&mut s, "(vlax-curve-isClosed c)"), "T");
}

#[test]
fn an_ellipse_keeps_its_own_parameter() {
    let mut s = session();
    let tau = std::f64::consts::TAU;
    add(&mut s, "e", EntityKind::Ellipse(Ellipse { center: v3(0.0, 0.0, 0.0), major: v3(4.0, 0.0, 0.0), ratio: 0.5, start: 0.0, end: tau }));
    is_t(&mut s, "(equal (vlax-curve-getEndParam e) (* 2 pi) 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getPointAtParam e (/ pi 2)) '(0.0 2.0 0.0) 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getArea e) (* 8 pi) 1e-9)");
    // Ramanujan's perimeter is within 1e-5 of the true one at this ratio.
    is_t(&mut s, "(equal (vlax-curve-getDistAtParam e (* 2 pi)) (* pi (- 18 (sqrt 140))) 1e-4)");
    is_t(&mut s, "(equal (vlax-curve-getParamAtPoint e '(0 -2)) (* 1.5 pi) 1e-6)");
    is_t(&mut s, "(equal (vlax-curve-getFirstDeriv e 0.0) '(0.0 2.0 0.0) 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getClosestPointTo e '(10 0)) '(4.0 0.0 0.0) 1e-9)");
    assert_eq!(ok(&mut s, "(list (vlax-curve-isClosed e) (vlax-curve-isPeriodic e))"), "(T T)");
}

#[test]
fn a_spline_round_trips_parameters_points_and_distances() {
    let mut s = session();
    let sp = Spline::from_fit_points(&[Vec2::new(0.0, 0.0), Vec2::new(3.0, 2.0), Vec2::new(6.0, -1.0), Vec2::new(9.0, 1.0)]);
    add(&mut s, "e", EntityKind::Spline(sp));
    is_t(&mut s, "(equal (vlax-curve-getStartPoint e) '(0.0 0.0 0.0) 1e-9)");
    is_t(&mut s, "(equal (vlax-curve-getEndPoint e) '(9.0 1.0 0.0) 1e-9)");
    // Halfway along: the parameter there gives back the point, and the point the parameter.
    ok(
        &mut s,
        "(setq len (vlax-curve-getDistAtParam e (vlax-curve-getEndParam e)) pm (vlax-curve-getParamAtDist e (/ len 2)) p (vlax-curve-getPointAtParam e pm))",
    );
    is_t(&mut s, "(> len 9.0)");
    is_t(&mut s, "(equal (vlax-curve-getParamAtPoint e p) pm 1e-6)");
    is_t(&mut s, "(equal (vlax-curve-getDistAtPoint e p) (/ len 2) 1e-4)");
    is_t(&mut s, "(equal (vlax-curve-getPointAtDist e (/ len 2)) p 1e-6)");
    // A point off the curve is pulled onto it.
    is_t(&mut s, "(equal (vlax-curve-getClosestPointTo e (mapcar '+ p '(0 0.001 0))) p 1e-3)");
    // The first derivative points the way the curve goes.
    is_t(&mut s, "(> (car (vlax-curve-getFirstDeriv e pm)) 0)");
}

#[test]
fn a_3d_polyline_measures_in_space() {
    let mut s = session();
    let flat = Polyline3d { points: vec![v3(0.0, 0.0, 0.0), v3(3.0, 4.0, 0.0), v3(3.0, 4.0, 12.0)], closed: false };
    add(&mut s, "e", EntityKind::Polyline3d(flat));
    is_t(&mut s, "(equal (vlax-curve-getDistAtParam e 2) 17.0 1e-12)");
    is_t(&mut s, "(equal (vlax-curve-getPointAtDist e 11) '(3.0 4.0 6.0) 1e-12)");
    assert_eq!(ok(&mut s, "(vlax-curve-isPlanar e)"), "T");
    let twisted = Polyline3d { points: vec![v3(0.0, 0.0, 0.0), v3(1.0, 0.0, 0.0), v3(1.0, 1.0, 0.0), v3(1.0, 1.0, 1.0)], closed: false };
    add(&mut s, "tw", EntityKind::Polyline3d(twisted));
    assert_eq!(ok(&mut s, "(vlax-curve-isPlanar tw)"), "nil");
}

#[test]
fn what_is_not_a_curve_is_an_error() {
    let mut s = session();
    add(&mut s, "pt", EntityKind::Point(Point { p: v3(1.0, 1.0, 0.0), angle: 0.0 }));
    assert_eq!(eval(&mut s, "(vlax-curve-getEndParam pt)").map_err(|e| e.starts_with("bad argument type: curvep")), Err(true));
    assert_eq!(eval(&mut s, "(vlax-curve-getEndParam 1)"), Err("bad argument type: lentityp: 1".into()));
    assert_eq!(eval(&mut s, "(vlax-curve-getEndParam)"), Err("too few arguments".into()));
}

/// `read-char` reads on from where it left off in a line (multi-byte characters whole, 10 at
/// each line end), and `read-line` gives the rest of a line `read-char` has started.
#[test]
fn read_char_and_read_line_share_their_place_in_the_file() {
    let mut s = session();
    let dir = std::env::temp_dir().join(format!("cadcraft-lisp-readchar-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("r.txt");
    std::fs::write(&path, "h\u{e9}llo\nab\n").unwrap();
    let p = path.to_string_lossy().replace('\\', "/");
    let code = format!(
        "(setq f (open \"{p}\" \"r\")) (list (read-char f) (read-char f) (read-line f) (read-char f) (read-char f) (read-char f) (read-char f) (progn (close f) 1))"
    );
    // h, é (233, one character of two bytes), the rest "llo", then a, b, the line end, the end.
    assert_eq!(ok(&mut s, &code), "(104 233 \"llo\" 97 98 10 nil 1)");
    // A long line read a character at a time comes out whole, in order.
    std::fs::write(&path, format!("{}\n", "xy".repeat(2000))).unwrap();
    let code = format!("(setq f (open \"{p}\" \"r\") n 0 c (read-char f)) (while (and c (/= c 10)) (setq n (1+ n) c (read-char f))) (close f) n");
    assert_eq!(ok(&mut s, &code), "4000");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn files_and_folders() {
    let mut s = session();
    let dir = std::env::temp_dir().join(format!("cadcraft-lisp-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let d = dir.to_string_lossy().replace('\\', "/");
    assert_eq!(ok(&mut s, &format!("(vl-mkdir \"{d}\")")), "T");
    assert_eq!(ok(&mut s, &format!("(vl-mkdir \"{d}\")")), "nil", "it exists");
    assert_eq!(ok(&mut s, &format!("(vl-file-directory-p \"{d}\")")), "T");
    ok(&mut s, &format!("(setq f (open \"{d}/a.txt\" \"w\")) (write-line \"hello\" f) (close f)"));
    let a = format!("{d}/a.txt");
    assert_eq!(ok(&mut s, &format!("(vl-file-size \"{a}\")")), "6");
    assert_eq!(ok(&mut s, &format!("(vl-file-size \"{d}\")")), "0", "a folder");
    assert_eq!(ok(&mut s, &format!("(vl-file-copy \"{a}\" \"{d}/b.txt\")")), "6");
    assert_eq!(ok(&mut s, &format!("(vl-file-copy \"{a}\" \"{d}/b.txt\")")), "nil", "never over an existing file");
    assert_eq!(ok(&mut s, &format!("(vl-file-copy \"{a}\" \"{d}/b.txt\" T)")), "6");
    assert_eq!(ok(&mut s, &format!("(vl-file-size \"{d}/b.txt\")")), "12");
    assert_eq!(ok(&mut s, &format!("(vl-file-rename \"{d}/b.txt\" \"{d}/c.lsp\")")), "T");
    assert_eq!(ok(&mut s, &format!("(vl-file-rename \"{d}/b.txt\" \"{d}/d.lsp\")")), "nil", "gone");
    assert_eq!(ok(&mut s, &format!("(vl-directory-files \"{d}\" \"*.lsp\" 1)")), "(\"c.lsp\")");
    assert_eq!(ok(&mut s, &format!("(vl-directory-files \"{d}\" nil 1)")), "(\"a.txt\" \"c.lsp\")");
    assert_eq!(ok(&mut s, &format!("(vl-directory-files \"{d}\" nil -1)")), "(\".\" \"..\")");
    assert_eq!(ok(&mut s, &format!("(length (vl-file-systime \"{a}\"))")), "8");
    is_t(&mut s, &format!("(>= (car (vl-file-systime \"{a}\")) 2024)"));
    // read-char gives one character at a time.
    assert_eq!(ok(&mut s, &format!("(setq f (open \"{a}\" \"r\")) (list (read-char f) (read-char f) (progn (close f) 1))")), "(104 101 1)");
    assert_eq!(ok(&mut s, &format!("(vl-file-delete \"{d}/c.lsp\")")), "T");
    assert_eq!(ok(&mut s, &format!("(vl-file-delete \"{d}/c.lsp\")")), "nil");
    assert_eq!(ok(&mut s, "(vl-file-size \"no/such/file/anywhere.txt\")"), "nil");
    assert_eq!(ok(&mut s, "(vl-directory-files \"no/such/folder/anywhere\")"), "nil");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn environment_variables() {
    let mut s = session();
    assert_eq!(ok(&mut s, "(setenv \"CadCraftLispTestVar\" \"42\")"), "\"42\"");
    assert_eq!(ok(&mut s, "(getenv \"CADCRAFTLISPTESTVAR\")"), "\"42\"", "names ignore case");
    assert_eq!(ok(&mut s, "(getenv \"CADCRAFT_LISP_NEVER_SET_ANYWHERE\")"), "nil");
    assert!(eval(&mut s, "(setenv \"\" \"x\")").is_err());
}

#[test]
fn getfiled_asks_at_the_command_line() {
    let mut s = session();
    // A new file: any name, the extension added.
    s.cmdline("(setq f (getfiled \"Save drawing as\" \"\" \"dxf\" 1))").unwrap();
    assert!(s.lisp_running());
    assert_eq!(s.prompt_text(), "Save drawing as: ");
    s.cmdline("plans/site").unwrap();
    assert!(!s.lisp_running());
    assert_eq!(to_text(&s.lisp_var("F"), true), "\"plans/site.dxf\"");
    // Enter takes the default.
    s.cmdline("(setq f (getfiled \"Export\" \"out\" \"csv\" 1))").unwrap();
    assert_eq!(s.prompt_text(), "Export <out>: ");
    s.input(crate::Input::Enter).unwrap();
    assert_eq!(to_text(&s.lisp_var("F"), true), "\"out.csv\"");
    // An existing file must exist: ask again; Enter with no default gives nil.
    s.cmdline("(setq f (getfiled \"Open\" \"\" \"lsp\" 0))").unwrap();
    s.cmdline("no-such-routine-anywhere").unwrap();
    assert!(s.lisp_running(), "asks again");
    s.input(crate::Input::Enter).unwrap();
    assert!(!s.lisp_running());
    assert_eq!(to_text(&s.lisp_var("F"), true), "nil");
    // Enter on a default that isn't there, in open mode, asks again as a typed name does; a file
    // that is there is taken.
    s.cmdline("(setq f (getfiled \"Open\" \"no-such-default-anywhere\" \"lsp\" 0))").unwrap();
    s.input(crate::Input::Enter).unwrap();
    assert!(s.lisp_running(), "a default that isn't there asks again");
    let dir = std::env::temp_dir().join(format!("cadcraft-getfiled-enter-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let real = dir.join("here.lsp");
    std::fs::write(&real, "(princ)").unwrap();
    let real = real.to_string_lossy().replace('\\', "/");
    s.cmdline(&real).unwrap();
    assert!(!s.lisp_running());
    assert_eq!(to_text(&s.lisp_var("F"), true), format!("\"{real}\""));
    let _ = std::fs::remove_dir_all(&dir);
}

/// New-file mode asks before a file that is there is replaced, unless flag 32 says not to; No
/// (the default) asks for a name again.
#[test]
fn getfiled_asks_before_replacing_a_file() {
    let mut s = session();
    let dir = std::env::temp_dir().join(format!("cadcraft-getfiled-replace-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let real = dir.join("plan.dxf");
    std::fs::write(&real, "0\nEOF\n").unwrap();
    let real = real.to_string_lossy().replace('\\', "/");
    let ask = format!("{real} already exists. Replace it? [Yes/No] <No>: ");
    // Enter keeps the file: the name is asked again. Yes replaces it.
    s.cmdline("(setq f (getfiled \"Save\" \"\" \"dxf\" 1))").unwrap();
    s.cmdline(&real).unwrap();
    assert!(s.lisp_running());
    assert_eq!(s.prompt_text(), ask);
    s.input(crate::Input::Enter).unwrap();
    assert!(s.lisp_running());
    assert_eq!(s.prompt_text(), "Save: ");
    s.cmdline(&real).unwrap();
    s.cmdline("yes").unwrap();
    assert!(!s.lisp_running());
    assert_eq!(to_text(&s.lisp_var("F"), true), format!("\"{real}\""));
    // No: another name, taken.
    s.cmdline("(setq f (getfiled \"Save\" \"\" \"dxf\" 1))").unwrap();
    s.cmdline(&real).unwrap();
    s.cmdline("n").unwrap();
    assert_eq!(s.prompt_text(), "Save: ");
    s.cmdline("other").unwrap();
    assert_eq!(to_text(&s.lisp_var("F"), true), "\"other.dxf\"");
    // Flag 32 with flag 1: no question.
    s.cmdline("(setq f (getfiled \"Save\" \"\" \"dxf\" 33))").unwrap();
    s.cmdline(&real).unwrap();
    assert!(!s.lisp_running());
    assert_eq!(to_text(&s.lisp_var("F"), true), format!("\"{real}\""));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A default ending in a path delimiter, or any default with flag 16, is the folder relative
/// names are in, not a file name Enter takes.
#[test]
fn getfiled_default_folder() {
    let mut s = session();
    s.cmdline("(setq f (getfiled \"Save\" \"plans/\" \"dxf\" 1))").unwrap();
    assert_eq!(s.prompt_text(), "Save: ");
    s.cmdline("site").unwrap();
    assert_eq!(to_text(&s.lisp_var("F"), false), "plans/site.dxf");
    s.cmdline("(setq f (getfiled \"Save\" \"plans\" \"dxf\" 17))").unwrap();
    s.cmdline("site").unwrap();
    assert_eq!(to_text(&s.lisp_var("F"), false), std::path::Path::new("plans").join("site.dxf").to_string_lossy());
    // Enter with only a folder: no name, nil.
    s.cmdline("(setq f (getfiled \"Save\" \"plans/\" \"dxf\" 1))").unwrap();
    s.input(crate::Input::Enter).unwrap();
    assert!(!s.lisp_running());
    assert_eq!(to_text(&s.lisp_var("F"), true), "nil");
}

#[test]
fn osnap_finds_the_snap_near_a_point() {
    let mut s = session();
    add(&mut s, "e", EntityKind::Line(Line { a: v3(0.0, 0.0, 0.0), b: v3(10.0, 0.0, 0.0) }));
    is_t(&mut s, "(equal (osnap '(0.0001 0.0001) \"_end\") '(0.0 0.0) 1e-9)");
    is_t(&mut s, "(equal (osnap '(5.0001 0.0001 0.0) \"_mid,_end\") '(5.0 0.0 0.0) 1e-9)");
    assert_eq!(ok(&mut s, "(osnap '(5 0) \"nosuchmode\")"), "nil");
    assert_eq!(ok(&mut s, "(osnap '(500 500) \"_end\")"), "nil", "nothing within the aperture");
}

#[test]
fn names_numbers_and_strings() {
    let mut s = session();
    assert_eq!(ok(&mut s, "(list (boole 1 12 10) (boole 6 12 10) (boole 7 12 10) (boole 8 0 0))"), "(8 6 14 -1)");
    assert_eq!(
        ok(
            &mut s,
            "(list (vl-string-position 97 \"banana\") (vl-string-position 97 \"banana\" 2) (vl-string-position 97 \"banana\" 0 T) (vl-string-position 122 \"banana\"))"
        ),
        "(1 3 5 nil)"
    );
    assert_eq!(ok(&mut s, "(vl-string-elt \"abc\" 1)"), "98");
    assert!(eval(&mut s, "(vl-string-elt \"abc\" 3)").is_err());
    assert_eq!(ok(&mut s, "(list (vl-string-mismatch \"VL-FUN\" \"VL-VAR\") (vl-string-mismatch \"abc\" \"ABD\" 0 0 T))"), "(3 2)");
    assert_eq!(
        ok(&mut s, "(list (snvalid \"Layer 1\") (snvalid \"a<b\") (snvalid \"\") (snvalid \"a|b\") (snvalid \"a|b\" 1))"),
        "(T nil nil nil T)"
    );
    assert_eq!(ok(&mut s, "(list (getcname \"line\") (getcname \"_line\") (getcname \"nosuchcommandanywhere\"))"), "(\"_LINE\" \"LINE\" nil)");
    assert_eq!(ok(&mut s, "(atoms-family 1 '(\"CAR\" \"NOSUCHSYMBOLANYWHERE\"))"), "(\"CAR\" nil)");
    assert_eq!(ok(&mut s, "(list (acad-push-dbmod) (acad-pop-dbmod) (textscr) (graphscr))"), "(T T nil nil)");
}

#[test]
fn hostile_arguments_are_errors_not_crashes() {
    let mut s = session();
    add(&mut s, "e", EntityKind::Line(Line { a: v3(0.0, 0.0, 0.0), b: v3(1.0, 0.0, 0.0) }));
    for code in [
        "(vlax-curve-getPointAtParam e 1e308)",
        "(vlax-curve-getPointAtDist e -1e308)",
        "(vlax-curve-getClosestPointTo e '(1e308 -1e308))",
        "(vlax-curve-getClosestPointToProjection e '(0 0 0) '(0 0 0))",
        "(vlax-curve-getParamAtPoint e \"x\")",
        "(vlax-curve-getFirstDeriv e nil)",
        "(vlax-curve-getArea nil)",
        "(vl-file-copy 1 2)",
        "(vl-directory-files 1 2 3)",
        "(vl-directory-files \".\" \"*\" 7)",
        "(getenv 1)",
        "(setenv \"A\" 1)",
        "(osnap 1 2)",
        "(osnap '(0 0) \"\")",
        "(boole 1)",
        "(boole 1 1.5 2)",
        "(vl-string-position -1 \"abc\")",
        "(vl-string-position 97 \"abc\" -4)",
        "(vl-string-elt \"abc\" -1)",
        "(vl-string-mismatch \"a\" \"b\" -1)",
        "(snvalid nil)",
        "(getcname nil)",
        "(atoms-family 2)",
        "(read-char 5)",
        "(write-char -1)",
        "(write-char 1114112)",
        "(getfiled 1 2 3 4)",
        "(getfiled \"a\" \"b\" \"c\")",
    ] {
        // Any outcome but a panic.
        let _ = eval(&mut s, code);
    }
    // A degenerate polyline and a zero-radius circle measure as points.
    add(
        &mut s,
        "p",
        EntityKind::LwPolyline(LwPolyline {
            vertices: vec![PolyVertex::new(Vec2::new(1.0, 1.0))],
            closed: false,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        }),
    );
    is_t(&mut s, "(equal (vlax-curve-getDistAtParam p 0) 0.0 1e-12)");
    add(&mut s, "c", EntityKind::Circle(Circle { center: v3(0.0, 0.0, 0.0), radius: 0.0 }));
    is_t(&mut s, "(equal (vlax-curve-getClosestPointTo c '(3 4)) '(0.0 0.0 0.0) 1e-12)");
}

#[test]
fn curves_through_entsel_ssget_and_the_lisp_command() {
    let mut s = session();
    let h = s.add_entity(EntityKind::Circle(Circle { center: v3(5.0, 5.0, 0.0), radius: 1.0 })).unwrap();
    // A routine walking a selection set, as real ones do.
    let code = "(setq ss (ssget \"X\" '((0 . \"CIRCLE\"))) tot 0.0 i 0)
                (repeat (sslength ss) (setq e (ssname ss i) tot (+ tot (vlax-curve-getDistAtParam e (vlax-curve-getEndParam e))) i (1+ i)))
                tot";
    is_t(&mut s, &format!("(equal (progn {code}) (* 2 pi) 1e-12)"));
    // Programmatically (scripts, the control channel, MCP): the same answer.
    let r = s.execute("lisp", &serde_json::json!({"code": format!("(vlax-curve-getArea (handent \"{}\"))", h.hex())})).unwrap();
    assert_eq!(r["value"], "3.14159", "{r}");
    // An erased object is no longer a curve.
    ok(&mut s, &format!("(setq c (handent \"{}\"))", h.hex()));
    s.execute("erase", &serde_json::json!({"handles": [h.hex()]})).unwrap();
    assert!(eval(&mut s, "(vlax-curve-getArea c)").is_err());
    // vl-catch-all-apply turns the error into a value, as routines guard these calls.
    assert_eq!(ok(&mut s, "(vl-catch-all-error-p (vl-catch-all-apply 'vlax-curve-getArea (list c)))"), "T");
}

#[test]
fn getfiled_extensions_spaces_and_cancel() {
    let mut s = session();
    // The first of several extensions, with or without its dot; a name with spaces stays whole.
    s.cmdline("(setq f (getfiled \"Save\" \"\" \"dwg;dxf\" 1))").unwrap();
    s.cmdline("my plans/site plan").unwrap();
    assert_eq!(to_text(&s.lisp_var("F"), true), "\"my plans/site plan.dwg\"");
    s.cmdline("(setq f (getfiled \"Save\" \"\" \".csv\" 1))").unwrap();
    s.cmdline("\"quoted name\"").unwrap();
    assert_eq!(to_text(&s.lisp_var("F"), true), "\"quoted name.csv\"");
    // A name with its own extension keeps it; "*" adds none.
    s.cmdline("(setq f (getfiled \"Save\" \"\" \"dxf\" 1))").unwrap();
    s.cmdline("report.txt").unwrap();
    assert_eq!(to_text(&s.lisp_var("F"), true), "\"report.txt\"");
    s.cmdline("(setq f (getfiled \"Save\" \"\" \"*\" 1))").unwrap();
    s.cmdline("README").unwrap();
    assert_eq!(to_text(&s.lisp_var("F"), true), "\"README\"");
    // An existing file is found.
    let dir = std::env::temp_dir().join(format!("cadcraft-getfiled-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tool.lsp"), "(princ)").unwrap();
    let base = dir.join("tool").to_string_lossy().replace('\\', "/");
    s.cmdline("(setq f (getfiled \"Load\" \"\" \"lsp\" 0))").unwrap();
    s.cmdline(&base).unwrap();
    assert!(!s.lisp_running());
    assert_eq!(to_text(&s.lisp_var("F"), false), format!("{base}.lsp"));
    let _ = std::fs::remove_dir_all(&dir);
    // Esc stops the routine, as at any prompt; *error* hears "Function cancelled".
    s.cmdline("(defun *error* (m) (setq why m))").unwrap();
    s.cmdline("(setq f (getfiled \"Open\" \"\" \"lsp\" 0))").unwrap();
    assert!(s.lisp_running());
    s.cancel();
    assert!(!s.lisp_running());
    assert_eq!(to_text(&s.lisp_var("WHY"), true), "\"Function cancelled\"");
    // A programmatic run has no one to ask: nil.
    assert_eq!(ok(&mut s, "(getfiled \"Open\" \"\" \"lsp\" 0)"), "nil");
}

#[test]
fn osnap_modes_and_heights() {
    let mut s = session();
    add(&mut s, "c", EntityKind::Circle(Circle { center: v3(20.0, 0.0, 0.0), radius: 3.0 }));
    // A 3D point keeps its height; a 2D one gives a 2D point.
    is_t(&mut s, "(equal (osnap '(23.0001 0.0001 7.0) \"_qua\") '(23.0 0.0 7.0) 1e-9)");
    is_t(&mut s, "(equal (osnap '(23.0001 0.0001) \"QUA\") '(23.0 0.0) 1e-9)");
    is_t(&mut s, "(equal (osnap '(20.0001 0.0001) \"_cen\") '(20.0 0.0) 1e-9)");
    assert_eq!(ok(&mut s, "(osnap '(23 0) \"_non\")"), "nil", "NONE snaps to nothing");
    assert_eq!(ok(&mut s, "(osnap '(23 0) \"\")"), "nil");
}

#[test]
fn files_with_spaces_and_unicode_names() {
    let mut s = session();
    let dir = std::env::temp_dir().join(format!("cadcraft lisp ünï {}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let d = dir.to_string_lossy().replace('\\', "/");
    assert_eq!(ok(&mut s, &format!("(vl-mkdir \"{d}\")")), "T");
    ok(&mut s, &format!("(setq f (open \"{d}/plan é.txt\" \"w\")) (write-line \"x\" f) (close f)"));
    assert_eq!(ok(&mut s, &format!("(vl-directory-files \"{d}\" \"*.TXT\" 1)")), "(\"plan é.txt\")", "patterns ignore case");
    assert_eq!(ok(&mut s, &format!("(vl-file-size \"{d}/plan é.txt\")")), "2");
    assert_eq!(ok(&mut s, &format!("(vl-file-delete \"{d}/plan é.txt\")")), "T");
    assert_eq!(ok(&mut s, &format!("(vl-directory-files \"{d}\" nil 1)")), "nil", "nothing left");
    let _ = std::fs::remove_dir_all(&dir);
    // The current folder lists without arguments.
    assert!(ok(&mut s, "(listp (vl-directory-files))") == "T");
    // getenv reaches the system's own variables.
    assert_eq!(ok(&mut s, "(eq (type (getenv \"PATH\")) 'STR)"), "T");
}

#[test]
fn new_functions_are_listed_and_print() {
    let mut s = session();
    for name in ["VLAX-CURVE-GETAREA", "VL-FILE-COPY", "GETFILED", "OSNAP", "GETENV", "BOOLE"] {
        assert_eq!(ok(&mut s, &format!("(type {name})")), "SUBR", "{name}");
        assert_eq!(ok(&mut s, &format!("(atoms-family 1 '(\"{name}\"))")), format!("(\"{name}\")"));
    }
    // write-char goes to the command line without a file.
    ok(&mut s, "(write-char 65) (write-char 10)");
    assert!(s.log.iter().any(|l| l.contains('A')), "{:?}", s.log);
}
