#![allow(clippy::unwrap_used)]

use super::value::{Value, to_text};
use crate::Session;
use crate::cmd::file::{EntityCodec, set_entity_codec};

pub(super) fn session() -> Session {
    set_entity_codec(EntityCodec { groups: cadcraft_io::entity_groups, entity: cadcraft_io::entity_from_groups });
    Session::new()
}

/// Evaluate `code` to the end; its value as `prin1` shows it, or the error message.
fn eval(s: &mut Session, code: &str) -> Result<String, String> {
    eval_with(s, code, Vec::new())
}

fn eval_with(s: &mut Session, code: &str, inputs: Vec<Value>) -> Result<String, String> {
    match s.lisp_eval(code, false, true, inputs) {
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

#[test]
fn real_print_subnormal_and_rounding() {
    let mut s = session();
    assert_eq!(ok(&mut s, "1e-310"), "1.0e-310");
    assert_eq!(ok(&mut s, "5e-324"), "4.94066e-324");
    assert_eq!(ok(&mut s, "9.9999996e10"), "1.0e+011");
    assert_eq!(ok(&mut s, "9.9999996e-7"), "1.0e-006");
    assert_eq!(super::value::real_text(-1e-310), "-1.0e-310");
    assert_eq!(super::value::real_text(1e308), "1.0e+308");
    assert_eq!(super::value::real_text(999999.5), "1.0e+006");
    assert_eq!(super::value::real_text(0.000009999996), "0.00001");
}

#[test]
fn arithmetic_follows_autolisp() {
    let mut s = session();
    assert_eq!(ok(&mut s, "(+ 1 2 3)"), "6");
    assert_eq!(ok(&mut s, "(+ 1 2.0)"), "3.0");
    assert_eq!(ok(&mut s, "(/ 7 2)"), "3");
    assert_eq!(ok(&mut s, "(/ 7 2.0)"), "3.5");
    assert_eq!(ok(&mut s, "(- 5)"), "-5");
    assert_eq!(ok(&mut s, "(rem 7 3)"), "1");
    assert_eq!(ok(&mut s, "(1+ 41)"), "42");
    assert_eq!(ok(&mut s, "(fix 3.7)"), "3");
    assert_eq!(ok(&mut s, "(float 3)"), "3.0");
    assert_eq!(ok(&mut s, "(expt 2 10)"), "1024");
    assert_eq!(ok(&mut s, "(sqrt 16)"), "4.0");
    assert_eq!(ok(&mut s, "(max 1 5 3)"), "5");
    assert_eq!(ok(&mut s, "(min 1 5.0 3)"), "1.0");
    assert_eq!(ok(&mut s, "(abs -4)"), "4");
    assert_eq!(ok(&mut s, "(logand 12 10)"), "8");
    assert_eq!(ok(&mut s, "(lsh 1 4)"), "16");
    assert_eq!(ok(&mut s, "(+ 2147483647 1)"), "-2147483648", "32-bit integers wrap");
    assert_eq!(ok(&mut s, "(< 1 2 3)"), "T");
    assert_eq!(ok(&mut s, "(= 1 1.0)"), "T");
    assert_eq!(ok(&mut s, "(= \"a\" \"a\")"), "T");
    assert_eq!(eval(&mut s, "(/ 1 0)"), Err("divide by zero".into()));
    assert_eq!(eval(&mut s, "(+ 1 nil)"), Err("bad argument type: numberp: nil".into()));
}

/// `=` answers for the symbols `type` returns, which is how AutoLISP routines test a type:
/// `(= (type x) 'INT)`. Coercing the symbol to a number failed the whole routine with "bad
/// argument type" instead, so a routine written for AutoLISP stopped at its first type test
/// (#1597). The ordering comparisons are still numbers and strings, as AutoLISP has them.
#[test]
fn equality_compares_symbols_as_autolisp_does() {
    let mut s = session();
    assert_eq!(ok(&mut s, "(= (type 1) 'INT)"), "T");
    assert_eq!(ok(&mut s, "(= (type 1.0) 'INT)"), "nil");
    assert_eq!(ok(&mut s, "(= (type '(1 2)) 'LIST)"), "T");
    assert_eq!(ok(&mut s, "(= (type \"a\") 'STR)"), "T");
    assert_eq!(ok(&mut s, "(/= (type 1) 'REAL)"), "T");
    assert_eq!(ok(&mut s, "(= 'a 'a 'a)"), "T");
    assert_eq!(ok(&mut s, "(= 'a 'b)"), "nil");

    // The routine from the report runs.
    ok(&mut s, "(defun isint (a) (if (= (type a) 'INT) T nil))");
    assert_eq!(ok(&mut s, "(isint 3)"), "T");
    assert_eq!(ok(&mut s, "(isint 3.0)"), "nil");
    assert_eq!(ok(&mut s, "(isint \"3\")"), "nil");

    // A mismatch of kinds is nil, not an error; numbers and strings are unchanged.
    assert_eq!(ok(&mut s, "(= 1 \"1\")"), "nil");
    assert_eq!(ok(&mut s, "(= '(1) '(1))"), "nil", "`=` is not `equal`");
    assert_eq!(ok(&mut s, "(= nil nil)"), "T");
    assert_eq!(ok(&mut s, "(= 1 1.0)"), "T");
    assert_eq!(ok(&mut s, "(= \"a\" \"a\" \"a\")"), "T");
    assert_eq!(ok(&mut s, "(= 1 1 2)"), "nil");

    // Ordering still refuses what AutoLISP refuses.
    assert_eq!(eval(&mut s, "(< 'a 'b)"), Err("bad argument type: numberp: A".into()));
}

#[test]
fn lists_and_strings() {
    let mut s = session();
    list_builtins(&mut s);
    string_builtins(&mut s);
}

fn list_builtins(s: &mut Session) {
    assert_eq!(ok(s, "(cons 1 '(2 3))"), "(1 2 3)");
    assert_eq!(ok(s, "(cons 8 \"0\")"), "(8 . \"0\")");
    assert_eq!(ok(s, "(cadr '(1 2 3))"), "2");
    assert_eq!(ok(s, "(cdr (assoc 40 '((10 1.0 2.0 0.0) (40 . 2.5))))"), "2.5");
    assert_eq!(ok(s, "(subst '(40 . 5.0) '(40 . 2.5) '((8 . \"0\") (40 . 2.5)))"), "((8 . \"0\") (40 . 5.0))");
    assert_eq!(ok(s, "(append '(1) '(2 3) nil '(4))"), "(1 2 3 4)");
    assert_eq!(ok(s, "(reverse '(1 2 3))"), "(3 2 1)");
    assert_eq!(ok(s, "(nth 1 '(a b c))"), "B");
    assert_eq!(ok(s, "(member 2 '(1 2 3))"), "(2 3)");
    assert_eq!(ok(s, "(length '(1 2 3))"), "3");
    assert_eq!(ok(s, "(last '(1 2 3))"), "3");
    assert_eq!(ok(s, "(mapcar '+ '(1 2 3) '(10 20 30))"), "(11 22 33)");
    assert_eq!(ok(s, "(mapcar '(lambda (x) (* x x)) '(1 2 3))"), "(1 4 9)");
    assert_eq!(ok(s, "(apply 'max '(3 9 2))"), "9");
    assert_eq!(ok(s, "(vl-sort '(3 1 2) '<)"), "(1 2 3)");
    assert_eq!(ok(s, "(vl-sort '(3 1 2) (function (lambda (a b) (> a b))))"), "(3 2 1)");
    assert_eq!(ok(s, "(vl-remove-if 'minusp '(1 -2 3))"), "(1 3)");
}

fn string_builtins(s: &mut Session) {
    assert_eq!(ok(s, "(strcat \"a\" \"b\" \"c\")"), "\"abc\"");
    assert_eq!(ok(s, "(substr \"Hello\" 2 3)"), "\"ell\"");
    assert_eq!(ok(s, "(strcase \"abc\")"), "\"ABC\"");
    assert_eq!(ok(s, "(strlen \"abc\" \"de\")"), "5");
    assert_eq!(ok(s, "(itoa 42)"), "\"42\"");
    assert_eq!(ok(s, "(atoi \"12abc\")"), "12");
    assert_eq!(ok(s, "(atof \"2.5mm\")"), "2.5");
    assert_eq!(ok(s, "(atof \"x\")"), "0.0");
    assert_eq!(ok(s, "(wcmatch \"Name\" \"N*\")"), "T");
    assert_eq!(ok(s, "(wcmatch \"A12\" \"@##\")"), "T");
    assert_eq!(ok(s, "(wcmatch \"abc\" \"x*,a?c\")"), "T");
    assert_eq!(ok(s, "(wcmatch \"abc\" \"~a*\")"), "nil");
    assert_eq!(ok(s, "(wcmatch \"b\" \"[a-c]\")"), "T");
    let many = format!("(wcmatch \"{}\" \"{}\")", "a".repeat(80), "*a".repeat(80));
    assert_eq!(ok(s, &many), "T");
    let too_deep = format!("(wcmatch \"{}\" \"{}\")", "a".repeat(400), "*a".repeat(400));
    assert!(eval(s, &too_deep).is_err());
    assert_eq!(ok(s, "(vl-string-subst \"X\" \"b\" \"abcb\")"), "\"aXcb\"");
    assert_eq!(ok(s, "(vl-string-search \"c\" \"abc\")"), "2");
    assert_eq!(ok(s, "(read \"(1 2)\")"), "(1 2)");
    assert_eq!(ok(s, "(type \"x\")"), "STR");
    assert_eq!(ok(s, "(type 'x)"), "SYM");
    assert_eq!(ok(s, "(equal '(1 2.0) '(1 2.0000001) 1e-3)"), "T");
}

#[test]
fn geometry() {
    let mut s = session();
    assert_eq!(ok(&mut s, "(polar '(0 0) 0 5)"), "(5.0 0.0)");
    assert_eq!(ok(&mut s, "(distance '(0 0) '(3 4))"), "5.0");
    assert_eq!(ok(&mut s, "(angle '(0 0) '(0 1))"), "1.5708");
    assert_eq!(ok(&mut s, "(inters '(0 0) '(10 10) '(0 10) '(10 0))"), "(5.0 5.0)");
    assert_eq!(ok(&mut s, "(inters '(0 0) '(1 1) '(0 10) '(10 0))"), "nil");
    assert_eq!(ok(&mut s, "(inters '(0 0) '(1 1) '(0 10) '(10 0) nil)"), "(5.0 5.0)");
}

#[test]
fn functions_scope_and_control() {
    let mut s = session();
    ok(&mut s, "(defun sq (x) (* x x))");
    assert_eq!(ok(&mut s, "(sq 7)"), "49");
    ok(&mut s, "(defun fact (n) (if (< n 2) 1 (* n (fact (1- n)))))");
    assert_eq!(ok(&mut s, "(fact 10)"), "3628800");
    // Dynamic scope: arguments and locals hide globals while the function runs.
    ok(&mut s, "(setq x 1 y 2)");
    ok(&mut s, "(defun f (x / y) (setq y 5) (g))");
    ok(&mut s, "(defun g () (list x y))");
    assert_eq!(ok(&mut s, "(f 9)"), "(9 5)");
    assert_eq!(ok(&mut s, "(list x y)"), "(1 2)", "restored after the call");
    assert_eq!(ok(&mut s, "(progn (setq i 0 acc nil) (while (< i 3) (setq acc (cons i acc) i (1+ i))) acc)"), "(2 1 0)");
    assert_eq!(ok(&mut s, "(progn (setq n 0) (repeat 4 (setq n (+ n 2))) n)"), "8");
    assert_eq!(ok(&mut s, "(progn (setq tot 0) (foreach v '(1 2 3) (setq tot (+ tot v))) tot)"), "6");
    assert_eq!(ok(&mut s, "(cond ((= 1 2) 'a) ((= 1 1) 'b) (t 'c))"), "B");
    assert_eq!(ok(&mut s, "(and 1 2)"), "T");
    assert_eq!(ok(&mut s, "(or nil nil)"), "nil");
    assert_eq!(ok(&mut s, "((lambda (a b) (+ a b)) 1 2)"), "3");
    assert_eq!(ok(&mut s, "(eval '(+ 1 2))"), "3");
    assert_eq!(eval(&mut s, "(sq)"), Err("too few arguments".into()));
    assert_eq!(eval(&mut s, "(sq 1 2)"), Err("too many arguments".into()));
    assert_eq!(eval(&mut s, "(nosuchfunction 1)"), Err("no function definition: NOSUCHFUNCTION".into()));
}

#[test]
fn errors_unwind_bindings_and_reach_the_handler() {
    let mut s = session();
    ok(&mut s, "(setq v 'outer)");
    ok(&mut s, "(defun boom (v) (/ 1 0))");
    assert!(eval(&mut s, "(boom 'inner)").is_err());
    assert_eq!(ok(&mut s, "v"), "OUTER", "the error restored the binding");
    // *error* runs with the message.
    ok(&mut s, "(defun *error* (msg) (setq caught msg))");
    assert!(eval(&mut s, "(boom 1)").is_err());
    assert_eq!(ok(&mut s, "caught"), "\"divide by zero\"");
    ok(&mut s, "(setq *error* nil)");
    // vl-catch-all-apply turns an error into a value.
    assert_eq!(ok(&mut s, "(vl-catch-all-error-message (vl-catch-all-apply '/ '(1 0)))"), "\"divide by zero\"");
    assert_eq!(ok(&mut s, "(vl-catch-all-apply '+ '(1 2))"), "3");
    assert!(eval(&mut s, "(exit)").is_err());
}

#[test]
fn runaway_routines_are_errors_not_crashes() {
    let mut s = session();
    ok(&mut s, "(defun deep (n) (+ 1 (deep (1+ n))))");
    assert_eq!(eval(&mut s, "(deep 0)"), Err("internal stack limit reached".into()));
    // A deeply nested source text.
    let nested = format!("{}1{}", "(+ ".repeat(50_000), ")".repeat(50_000));
    assert!(eval(&mut s, &nested).is_err());
    // Hostile values.
    assert!(eval(&mut s, "(substr \"abc\" 0)").is_err());
    assert!(eval(&mut s, "(nth -1 '(1))").is_ok());
    assert!(eval(&mut s, "(chr -5)").is_ok());
    assert!(eval(&mut s, "(expt 2.0 99999)").is_err());
    assert!(eval(&mut s, "(gcd 0 0)").is_err());
    assert!(eval(&mut s, "(wcmatch \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaab\" \"*a*a*a*a*a*a*a*a*a*c\")").is_ok());
    assert!(eval(&mut s, "(fix 1e300)").is_ok());
    assert!(eval(&mut s, "(lsh 1 99)").is_ok());
    // An endless loop in a blocking run stops at the step cap.
    let r = eval(&mut s, "(setq n 0) (while (< n 3000000) (setq n (1+ n))) n");
    assert_eq!(r, Ok("3000000".into()));
}

/// An infinite comparator stops at its callback cap, and a later finite comparator still succeeds.
#[test]
fn synchronous_callback_loop_is_an_error_not_a_hang() {
    // A comparator with an infinite loop used to run with u64::MAX fuel and hang the thread; it
    // now terminates at the synchronous-callback step cap (CWE-400, finding row 2).
    let mut s = session();
    s.lisp.limits.callback_steps = 1_000;
    assert!(eval(&mut s, "(vl-sort '(1 2) (function (lambda (a b) (while t))))").is_err());
    // A well-behaved comparator still works afterwards.
    assert_eq!(ok(&mut s, "(vl-sort '(3 1 2) '<)"), "(1 2 3)");
}

/// A zero-step callback limit prevents even the first callback side effect.
#[test]
fn a_zero_callback_budget_does_not_run_the_callback() {
    let mut s = session();
    s.lisp.limits.callback_steps = 0;
    ok(&mut s, "(setq callback-ran nil)");
    let result = eval(&mut s, "(vl-remove-if '(lambda (x) (setq callback-ran t)) '(1))");
    assert_eq!(result, Err("callback stopped after 0 steps".into()));
    assert_eq!(ok(&mut s, "callback-ran"), "nil");
}

/// A callback stops at a cap smaller than one scheduling slice without completing all its assignments.
#[test]
fn a_callback_must_fit_within_a_partial_slice() {
    let mut s = session();
    s.lisp.limits.callback_steps = 1_000;
    ok(&mut s, "(setq callback-count 0)");
    let result = eval(&mut s, "(vl-remove-if '(lambda (x) (repeat 1000 (setq callback-count (1+ callback-count)))) '(1))");
    assert_eq!(result, Err("callback stopped after 1000 steps".into()));
    assert_eq!(ok(&mut s, "(and (> callback-count 0) (< callback-count 1000))"), "T");
}

/// Crossing a scheduling boundary grants only the additional configured step, rather than another slice.
#[test]
fn reaching_a_callback_slice_boundary_does_not_grant_another_slice() {
    let counts: Vec<u64> = [399_999, 400_000]
        .into_iter()
        .map(|cap| {
            let mut s = session();
            s.lisp.limits.callback_steps = cap;
            ok(&mut s, "(setq callback-count 0)");
            let result = eval(&mut s, "(vl-remove-if '(lambda (x) (while t (setq callback-count (1+ callback-count)))) '(1))");
            assert_eq!(result, Err(format!("callback stopped after {cap} steps")));
            ok(&mut s, "callback-count").parse().unwrap()
        })
        .collect();
    // One additional VM step can perform at most one assignment, even at SYNC_SLICE's boundary.
    assert!(counts[0] > 0);
    assert!(counts[1] >= counts[0] && counts[1] - counts[0] <= 1, "{counts:?}");
}

/// System-variable conversion rejects a shared list whose owned JSON expansion would exceed its budget.
#[test]
fn setvar_rejects_an_oversized_shared_structure() {
    // A list repeatedly sharing its own contents expands exponentially; setvar's bounded JSON
    // conversion rejects it instead of allocating gigabytes (CWE-400, finding row 39).
    let mut s = session();
    let build = "(setq x '(1)) (repeat 40 (setq x (list x x))) (setvar \"GRIDUNIT\" x)";
    assert!(eval(&mut s, build).is_err());
}

#[test]
fn printing() {
    let mut s = session();
    let before = s.log.len();
    ok(&mut s, "(princ \"Hello\") (princ \" world\\n\") (prin1 \"q\") (terpri) (print 3) (princ)");
    let out: Vec<_> = s.log[before..].to_vec();
    assert_eq!(out, ["Hello world", "\"q\"", "", "3 "]);
    assert_eq!(ok(&mut s, "(vl-prin1-to-string '(1 \"a\"))"), "\"(1 \\\"a\\\")\"");
    assert_eq!(ok(&mut s, "(rtos 12.5 2 2)"), "\"12.50\"");
}

#[test]
fn command_draws_and_one_routine_is_one_undo_step() {
    let mut s = session();
    let n = |s: &Session| s.doc().unwrap().model.handles().len();
    let undo_before = s.state().unwrap().undo.len();
    ok(&mut s, "(command \"_.LINE\" '(0 0) '(10 0) \"\") (command \"_.CIRCLE\" '(5 5) 2)");
    assert_eq!(n(&s), 2);
    assert_eq!(s.state().unwrap().undo.len(), undo_before + 1, "the whole routine undoes at once");
    // A command left open carries on in the next call.
    ok(&mut s, "(command \"_.LINE\" '(0 10) '(10 10)) (command '(10 20) \"\")");
    assert_eq!(n(&s), 4);
    assert!(s.running.is_none());
    // (defun c:…) makes a command.
    ok(&mut s, "(defun c:twolines () (command \"_.LINE\" '(0 0) '(0 5) '(5 5) \"\") (princ))");
    s.cmdline("TWOLINES").unwrap();
    assert_eq!(n(&s), 6);
    // An expression typed at Command: shows its value.
    s.cmdline("(+ 40 2)").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("42"));
    s.cmdline("(setq dist 7.5)").unwrap();
    s.cmdline("!dist").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("7.5"));
    // An expression at a command's prompt answers it.
    s.cmdline("CIRCLE 0,0 (* 2 dist)").unwrap();
    let r = s.doc().unwrap().model.iter().find_map(|e| match &e.kind {
        cadcraft_doc::EntityKind::Circle(c) if c.center.x == 0.0 && c.center.y == 0.0 => Some(c.radius),
        _ => None,
    });
    assert_eq!(r, Some(15.0));
}

#[test]
fn entity_lists_round_trip() {
    let mut s = session();
    let made = ok(&mut s, "(entmake '((0 . \"CIRCLE\") (8 . \"WALLS\") (10 1.0 2.0 0.0) (40 . 3.0)))");
    assert!(made.contains("CIRCLE"));
    assert!(s.doc().unwrap().layer("WALLS").is_some(), "entmake creates the layer");
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (entget (entlast))))"), "\"CIRCLE\"");
    assert_eq!(ok(&mut s, "(cdr (assoc 8 (entget (entlast))))"), "\"WALLS\"");
    assert_eq!(ok(&mut s, "(cdr (assoc 10 (entget (entlast))))"), "(1.0 2.0 0.0)");
    assert_eq!(ok(&mut s, "(cdr (assoc 410 (entget (entlast))))"), "\"Model\"");
    // entmod changes it in place.
    ok(&mut s, "(setq d (entget (entlast))) (entmod (subst (cons 40 9.0) (assoc 40 d) d))");
    assert_eq!(ok(&mut s, "(cdr (assoc 40 (entget (entlast))))"), "9.0");
    // A list without a layer uses the current one.
    ok(&mut s, "(setvar \"CLAYER\" \"WALLS\") (entmake '((0 . \"LINE\") (10 0 0 0) (11 5 0 0)))");
    assert_eq!(ok(&mut s, "(cdr (assoc 8 (entget (entlast))))"), "\"WALLS\"");
    // entnext walks in creation order; entdel deletes and brings back.
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (entget (entnext))))"), "\"CIRCLE\"");
    assert_eq!(ok(&mut s, "(entnext (entlast))"), "nil");
    ok(&mut s, "(setq e (entlast)) (entdel e)");
    assert_eq!(ok(&mut s, "(entget e)"), "nil");
    ok(&mut s, "(entdel e)");
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (entget e)))"), "\"LINE\"");
    assert_eq!(ok(&mut s, "(entmake '((0 . \"NOSUCHTHING\")))"), "nil");
    assert_eq!(ok(&mut s, "(handent \"ZZZZZZ\")"), "nil");
}

#[test]
fn entity_lists_hold_angles_in_radians() {
    let mut s = session();
    let near = |s: &mut Session, expr: &str, want: &str| {
        let code = format!("(equal {expr} {want} 1e-9)");
        assert_eq!(ok(s, &code), "T", "{expr} is {}, not {want}", ok(s, expr));
    };
    // A DXF file holds an ARC's 50 and 51 in degrees; entget gives radians (#1165).
    s.script("ARC\nC\n0,0\n10,0\n0,10\n").unwrap();
    near(&mut s, "(cdr (assoc 50 (entget (entlast))))", "0.0");
    near(&mut s, "(cdr (assoc 51 (entget (entlast))))", "(/ pi 2)");
    // ssget filters compare against the values entget shows.
    assert_eq!(ok(&mut s, "(sslength (ssget \"X\" '((0 . \"ARC\") (-4 . \"<\") (51 . 2.0))))"), "1");
    // entmod takes radians.
    ok(&mut s, "(setq d (entget (entlast))) (entmod (subst (cons 51 pi) (assoc 51 d) d))");
    let end = s.doc().unwrap().model.iter().find_map(|e| match &e.kind {
        cadcraft_doc::EntityKind::Arc(a) => Some(a.end),
        _ => None,
    });
    assert!(end.is_some_and(|a| (a - std::f64::consts::PI).abs() < 1e-9), "{end:?}");

    // TEXT rotation (50) and oblique angle (51), through entmake.
    ok(&mut s, "(entmake (list '(0 . \"TEXT\") '(10 0.0 0.0 0.0) '(40 . 2.5) '(1 . \"abc\") (cons 50 (/ pi 6)) '(51 . 0.25)))");
    let text = s.doc().unwrap().model.iter().find_map(|e| match &e.kind {
        cadcraft_doc::EntityKind::Text(t) => Some((t.rotation, t.oblique)),
        _ => None,
    });
    assert!(text.is_some_and(|(r, o)| (r - std::f64::consts::FRAC_PI_6).abs() < 1e-9 && (o - 0.25).abs() < 1e-9), "{text:?}");
    near(&mut s, "(cdr (assoc 50 (entget (entlast))))", "(/ pi 6)");
    near(&mut s, "(cdr (assoc 51 (entget (entlast))))", "0.25");

    // HATCH pattern angle (52) and pattern line angle (53): ANSI31 at 0 has its lines at 45°.
    s.script("CIRCLE\n0,0\n10\n-HATCH\nP\nANSI31\n1\n0\n0,0\n\n").unwrap();
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (entget (entlast))))"), "\"HATCH\"");
    near(&mut s, "(cdr (assoc 52 (entget (entlast))))", "0.0");
    near(&mut s, "(cdr (assoc 53 (entget (entlast))))", "(/ pi 4)");

    // MTEXT: 50 before the column type (75) is the rotation, after it a column height.
    ok(
        &mut s,
        "(entmake (list '(0 . \"MTEXT\") '(10 0.0 0.0 0.0) '(40 . 2.5) '(41 . 10.0) '(1 . \"abc\") (cons 50 (/ pi 3)) '(75 . 2) '(76 . 2) '(79 . 0) '(48 . 10.0) '(49 . 1.0) '(50 . 30.0) '(50 . 20.0)))",
    );
    let mtext = s.doc().unwrap().model.iter().find_map(|e| match &e.kind {
        cadcraft_doc::EntityKind::MText(t) => Some((t.rotation, t.columns.as_ref().map(|c| c.heights.clone()))),
        _ => None,
    });
    assert!(
        mtext.as_ref().is_some_and(|(r, h)| (r - std::f64::consts::FRAC_PI_3).abs() < 1e-9 && h.as_deref() == Some(&[30.0, 20.0][..])),
        "{mtext:?}"
    );
    near(&mut s, "(cdr (assoc 50 (entget (entlast))))", "(/ pi 3)");
    let d = ok(&mut s, "(entget (entlast))");
    assert!(d.contains("(50 . 30.0) (50 . 20.0)"), "{d}");
}

#[test]
fn selection_sets_and_filters() {
    let mut s = session();
    ok(&mut s, "(command \"_.CIRCLE\" '(0 0) 1) (command \"_.CIRCLE\" '(10 0) 2) (command \"_.LINE\" '(0 5) '(20 5) \"\")");
    assert_eq!(ok(&mut s, "(sslength (ssget \"_X\" '((0 . \"CIRCLE\"))))"), "2");
    assert_eq!(ok(&mut s, "(sslength (ssget \"X\" '((0 . \"CIRCLE,LINE\"))))"), "3");
    assert_eq!(ok(&mut s, "(sslength (ssget \"X\" '((-4 . \"<OR\") (0 . \"LINE\") (40 . 2.0) (-4 . \"OR>\"))))"), "2");
    assert_eq!(ok(&mut s, "(sslength (ssget \"X\" '((0 . \"CIRCLE\") (-4 . \">\") (40 . 1.5))))"), "1");
    assert_eq!(ok(&mut s, "(ssget \"X\" '((0 . \"ARC\")))"), "nil");
    assert_eq!(ok(&mut s, "(sslength (ssget \"_W\" '(-5 -5) '(5 5)))"), "1");
    assert_eq!(ok(&mut s, "(sslength (ssget \"_C\" '(-5 -5) '(5 6)))"), "2");
    assert_eq!(ok(&mut s, "(setq ss (ssadd)) (ssadd (entlast) ss) (sslength ss)"), "1");
    assert_eq!(ok(&mut s, "(ssdel (entlast) ss) (sslength ss)"), "0");
    assert_eq!(ok(&mut s, "(ssname (ssget \"X\") 99)"), "nil");
}

#[test]
fn tables_and_variables() {
    let mut s = session();
    ok(&mut s, "(command \"_.-LAYER\" \"_M\" \"DOORS\" \"_C\" \"1\" \"\" \"\")");
    assert_eq!(ok(&mut s, "(cdr (assoc 2 (tblsearch \"LAYER\" \"doors\")))"), "\"DOORS\"");
    assert_eq!(ok(&mut s, "(tblsearch \"LAYER\" \"nope\")"), "nil");
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (tblnext \"LAYER\" t)))"), "\"LAYER\"");
    assert_eq!(ok(&mut s, "(getvar \"CLAYER\")"), "\"DOORS\"");
    assert_eq!(ok(&mut s, "(setvar \"OSMODE\" 0)"), "0");
    assert_eq!(ok(&mut s, "(getvar \"OSMODE\")"), "0");
    assert_eq!(ok(&mut s, "(getvar \"NOSUCHVAR\")"), "nil");
}

#[test]
fn getvar_drawing_and_session_variables() {
    let mut s = session();
    assert_eq!(ok(&mut s, "(list (getvar \"DWGPREFIX\") (getvar \"DWGTITLED\"))"), "(\"\" 0)");
    assert_eq!(ok(&mut s, "(list (getvar \"FILEDIA\") (getvar \"CMDECHO\") (getvar \"TILEMODE\"))"), "(1 1 1)");
    assert_eq!(ok(&mut s, "(setvar \"FILEDIA\" 0) (setvar \"CMDECHO\" 0) (getvar \"FILEDIA\")"), "0");
    // Both belong to the session, so they survive a switch to another drawing.
    s.new_drawing(true);
    assert_eq!(ok(&mut s, "(list (getvar \"FILEDIA\") (getvar \"CMDECHO\"))"), "(0 0)");
    assert_eq!(ok(&mut s, "(getvar \"CMDACTIVE\")"), "0");
    let dir = std::env::temp_dir();
    let file = dir.join("lisp_getvar_test.dxf");
    s.state_mut().unwrap().path = Some(file.to_string_lossy().into_owned());
    let want = format!("{}{}", dir.to_string_lossy().trim_end_matches(std::path::MAIN_SEPARATOR), std::path::MAIN_SEPARATOR);
    assert_eq!(crate::sysvars::get(&s, "DWGPREFIX"), Some(serde_json::json!(want)));
    assert_eq!(ok(&mut s, "(getvar \"DWGTITLED\")"), "1");
}

/// CECOLOR is a string in AutoCAD, whatever it was set from (#1999).
#[test]
fn getvar_cecolor_is_a_string() {
    let mut s = session();
    assert_eq!(ok(&mut s, "(list (getvar \"CECOLOR\") (type (getvar \"CECOLOR\")))"), "(\"BYLAYER\" STR)");
    assert_eq!(ok(&mut s, "(setvar \"CECOLOR\" \"red\") (getvar \"CECOLOR\")"), "\"1\"");
    assert_eq!(ok(&mut s, "(setvar \"CECOLOR\" \"10,20,30\") (getvar \"CECOLOR\")"), "\"RGB:10,20,30\"");
}

#[test]
fn input_functions_take_supplied_answers() {
    let mut s = session();
    let r = eval_with(
        &mut s,
        "(list (getpoint \"\\nPick: \") (getreal) (getstring) (getint))",
        vec![Value::point(&[1.0, 2.0]), Value::Real(2.5), Value::str("hi"), Value::Int(3)],
    );
    assert_eq!(r, Ok("((1.0 2.0) 2.5 \"hi\" 3)".into()));
    // No answer in a programmatic run: nil.
    assert_eq!(eval(&mut s, "(getpoint)"), Ok("nil".into()));
}

#[test]
fn interactive_routine_waits_for_the_user() {
    let mut s = session();
    s.cmdline("(defun c:pt () (setq p (getpoint \"\\nWhere: \")) (command \"_.CIRCLE\" p 1) (princ))").unwrap();
    s.cmdline("PT").unwrap();
    assert!(s.lisp_running());
    assert_eq!(s.prompt_text(), "Where: ");
    s.input(crate::Input::Point(cadcraft_geom::Vec2::new(3.0, 4.0))).unwrap();
    assert!(!s.lisp_running());
    assert!(s.doc().unwrap().model.iter().any(|e| matches!(&e.kind, cadcraft_doc::EntityKind::Circle(c) if c.center.x == 3.0 && c.center.y == 4.0)));
    // Typed answers and keywords.
    s.cmdline("(progn (initget 1 \"Yes No\") (setq ans (getkword \"\\nSure? [Yes/No]: \")))").unwrap();
    s.cmdline("n").unwrap();
    assert_eq!(to_text(&s.lisp_var("ANS"), true), "\"No\"");
    // Bit 128 lets getkword return arbitrary text; a keyword still wins.
    s.cmdline("(progn (initget 128 \"SElect CUrrent\") (setq ans (getkword \"\\nOption: \")))").unwrap();
    s.cmdline("banana").unwrap();
    assert_eq!(to_text(&s.lisp_var("ANS"), true), "\"banana\"");
    s.cmdline("(progn (initget 128 \"SElect CUrrent\") (setq ans (getkword \"\\nOption: \")))").unwrap();
    s.cmdline("cu").unwrap();
    assert_eq!(to_text(&s.lisp_var("ANS"), true), "\"CUrrent\"");
    s.cmdline("(setq d (getdist '(0 0)))").unwrap();
    s.cmdline("3,4").unwrap();
    assert_eq!(to_text(&s.lisp_var("D"), true), "5.0");
    // Esc cancels the routine.
    s.cmdline("(setq z (getint))").unwrap();
    s.cancel();
    assert!(!s.lisp_running());
    // `pause` lets the user answer one prompt of a command.
    s.cmdline("(command \"_.CIRCLE\" pause 2)").unwrap();
    assert!(s.lisp_running());
    s.input(crate::Input::Point(cadcraft_geom::Vec2::new(50.0, 50.0))).unwrap();
    assert!(!s.lisp_running());
    assert!(s.doc().unwrap().model.iter().any(|e| matches!(&e.kind, cadcraft_doc::EntityKind::Circle(c) if c.center.x == 50.0 && c.radius == 2.0)));
}

#[test]
fn getangle_is_relative_to_angbase() {
    let mut s = session();
    s.cmdline("(setvar \"ANGBASE\" 90)").unwrap();
    // A picked point at 180° in world terms is 90° from ANGBASE.
    s.cmdline("(setq a (getangle '(0 0)))").unwrap();
    s.input(crate::Input::Point(cadcraft_geom::Vec2::new(-1.0, 0.0))).unwrap();
    assert_eq!(to_text(&s.lisp_var("A"), true), "1.5708");
    // getorient ignores ANGBASE.
    s.cmdline("(setq o (getorient '(0 0)))").unwrap();
    s.input(crate::Input::Point(cadcraft_geom::Vec2::new(-1.0, 0.0))).unwrap();
    assert_eq!(to_text(&s.lisp_var("O"), true), "3.14159");
    // A typed angle is already measured from ANGBASE.
    s.cmdline("(setq t2 (getangle))").unwrap();
    s.cmdline("180").unwrap();
    assert_eq!(to_text(&s.lisp_var("T2"), true), "3.14159");
}

#[test]
fn appload_and_load() {
    let mut s = session();
    let r =
        s.execute("appload", &serde_json::json!({"name": "tools.lsp", "text": "(defun c:hi () (princ \"hi\") (princ)) (setq loaded 1)"})).unwrap();
    assert!(r.get("error").is_none(), "{r}");
    assert!(s.log.iter().any(|l| l == "tools.lsp successfully loaded."));
    s.cmdline("HI").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("hi"));
    // (load) finds a file APPLOAD loaded, by name.
    assert_eq!(ok(&mut s, "(setq loaded 0) (load \"tools\") loaded"), "1");
    assert!(eval(&mut s, "(load \"no-such-file-anywhere\")").is_err());
    assert_eq!(ok(&mut s, "(load \"no-such-file-anywhere\" 'missing)"), "MISSING");
    let r = s.execute("lisp", &serde_json::json!({"code": "(princ \"x\") (* 6 7)"})).unwrap();
    assert_eq!(r["value"], "42");
    assert_eq!(r["output"][0], "x");
    let r = s.execute("lisp", &serde_json::json!({"code": "(car 1)"})).unwrap();
    assert!(r["error"].as_str().is_some_and(|e| e.contains("bad argument type")));
}

#[test]
fn appload_reads_a_lsp_path_and_reports_its_commands() {
    let dir = std::env::temp_dir().join(format!("cadcraft-appload-lisp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Tools.LSP");
    std::fs::write(&path, "(defun c:hi () (princ \"hi\") (princ)) (defun c:bye () (princ))").unwrap();
    let mut s = session();
    let r = s.execute("appload", &serde_json::json!({"path": path.to_string_lossy()})).unwrap();
    assert!(r.get("error").is_none(), "{r}");
    assert_eq!(r["commands"], serde_json::json!(["BYE", "HI"]));
    assert!(s.log.iter().any(|l| l == "Tools.LSP successfully loaded."));
    s.cmdline("HI").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("hi"));
    // Only `.lsp` files are APPLOADed, whatever the case of the extension.
    let txt = dir.join("notes.txt");
    std::fs::write(&txt, "(defun c:no () (princ))").unwrap();
    let e = s.execute("appload", &serde_json::json!({"path": txt.to_string_lossy()})).unwrap_err().to_string();
    assert!(e.contains(".lsp"), "{e}");
    // A missing file and a file that fails to parse are ordinary errors.
    let missing = dir.join("missing.lsp");
    let e = s.execute("appload", &serde_json::json!({"path": missing.to_string_lossy()})).unwrap_err().to_string();
    assert!(e.contains("can't read"), "{e}");
    let broken = dir.join("broken.lsp");
    std::fs::write(&broken, "(defun c:oops (").unwrap();
    let e = s.execute("appload", &serde_json::json!({"path": broken.to_string_lossy()})).unwrap_err().to_string();
    assert!(!e.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn appload_keeps_the_whitespace_of_a_relative_path() {
    /// A folder in the test's current directory, removed even when an assertion fails.
    struct TempDir(std::path::PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    // A JSON call's path is read as it is given: a relative path whose folder starts with a
    // space names that folder, not the one its trimmed name would.
    let dir = TempDir(std::path::PathBuf::from(format!(" cadcraft-appload-space-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&dir.0);
    std::fs::create_dir_all(&dir.0).unwrap();
    let path = dir.0.join("tools.lsp");
    std::fs::write(&path, "(setq space-loaded 1)").unwrap();
    let mut s = session();
    let r = s.execute("appload", &serde_json::json!({"path": path.to_string_lossy()})).unwrap();
    assert!(r.get("error").is_none(), "{r}");
    assert_eq!(ok(&mut s, "space-loaded"), "1");
    // A path of blanks only is still refused, not read as "".
    for blank in ["", " ", "\t "] {
        let e = s.execute("appload", &serde_json::json!({"path": blank})).unwrap_err().to_string();
        assert!(e.contains("`path`"), "{blank:?}: {e}");
    }
}

#[test]
fn hostile_lisp_never_panics() {
    let mut s = session();
    for code in [
        "(",
        ")",
        "'",
        "\"",
        "(1 . 2 3)",
        "(quote)",
        "(setq)",
        "(setq 1 2)",
        "(if)",
        "(defun)",
        "(defun f)",
        "(lambda)",
        "(repeat 1.5)",
        "(repeat -3 1)",
        "(foreach 1 2)",
        "(mapcar)",
        "(mapcar 'car 1)",
        "(apply 'car 1)",
        "(entget 1)",
        "(entmake '(1 2))",
        "(entmod nil)",
        "(ssget \"Q\")",
        "(ssget \"X\" '((-4 . \"<OR\")))",
        "(ssname 1 2)",
        "(command 1.5 nil '(a b))",
        "(tblsearch 1 2)",
        "(rtos 1 9)",
        "(getvar 1)",
        "(setvar \"OSMODE\" \"x\")",
        "(open \"\" \"z\")",
        "(read-line 5)",
        "(close 5)",
        "(vl-sort 1 2)",
        "(eval (read \"(\"))",
        "(substr \"\" 1 -1)",
        "(nth 99999999 '(1))",
        "(polar 1 2 3)",
        "(inters '(0 0) '(0 0) '(0 0) '(0 0))",
        "(chr 1114112)",
    ] {
        let _ = eval(&mut s, code);
    }
    assert!(!s.lisp_running());
}

/// `entsel` and `nentsel` pick one object, so the cursor shows the pick box at their prompt. It
/// was read off the message's first word, and a routine's own message begins with a newline -- so
/// the same call showed the pick box without a message and lost it with one (#1598).
#[test]
fn an_entsel_prompt_picks_objects_whatever_its_message_says() {
    let mut s = session();
    // No message: the default "Select object: ".
    s.cmdline("(nentsel)").unwrap();
    assert!(s.current_prompt().unwrap().picks_objects(), "{}", s.prompt_text());
    s.cancel();

    // The reported case: the routine's own message, starting with a newline.
    s.cmdline("(nentsel \"\nSelect second line or polyline: \")").unwrap();
    assert!(s.current_prompt().unwrap().picks_objects(), "{}", s.prompt_text());
    s.cancel();

    // Wording that never says "select" picks an object just the same.
    s.cmdline("(entsel \"\nPick a line: \")").unwrap();
    assert!(s.current_prompt().unwrap().picks_objects(), "{}", s.prompt_text());
    s.cancel();

    // A point prompt is not a pick, and keeps its point modifiers -- including one whose own
    // message happens to begin with the word the built-in commands are read by. `picks_one` says
    // which calls pick, so the wording is only ever read where the engine wrote it (#1635 review).
    for point in ["(getpoint \"\nSpecify a point: \")", "(getpoint \"\nSelect a point: \")", "(getpoint \"Select a point: \")"] {
        s.cmdline(point).unwrap();
        assert!(!s.current_prompt().unwrap().picks_objects(), "{point}: {}", s.prompt_text());
        s.cancel();
    }
}

#[test]
fn a_waiting_routine_is_a_prompt_with_a_rubber_band() {
    let mut s = session();
    assert!(!s.prompting());
    s.cmdline("(setq c (getcorner '(0 0) \"\nOther corner: \"))").unwrap();
    // No command runs, yet a prompt waits: clicks must answer it (not select).
    assert!(s.running.is_none() && s.prompting());
    let current = s.current_prompt().unwrap();
    assert!(current.raw && current.display() == "Other corner: ");
    // getcorner drags a rectangle from its corner.
    let band = s.preview(cadcraft_geom::Vec2::new(3.0, 2.0));
    assert_eq!(band.len(), 4);
    s.input(crate::Input::Point(cadcraft_geom::Vec2::new(3.0, 2.0))).unwrap();
    assert!(!s.prompting());
    // getpoint with a base point drags a line; without one, nothing.
    s.cmdline("(getpoint '(1 1))").unwrap();
    assert_eq!(s.preview(cadcraft_geom::Vec2::new(4.0, 5.0)).len(), 1);
    s.cancel();
    s.cmdline("(getpoint)").unwrap();
    assert!(s.preview(cadcraft_geom::Vec2::new(4.0, 5.0)).is_empty());
    s.cancel();
}

#[test]
fn a_leading_newline_ends_the_command_line_instead_of_adding_an_empty_one() {
    let mut s = session();
    let from = s.log.len();
    s.cmdline("(progn (princ \"\nHello\") (princ \"\n\nAfter a blank\") (princ))").unwrap();
    assert_eq!(&s.log[from..], ["Hello", "", "After a blank"]);
}

#[test]
fn startup_files_load_from_the_support_folder() {
    let dir = std::env::temp_dir().join(format!("cadcraft-lisp-startup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("acad.lsp"), "(defun c:fromacad () (princ \"acad\") (princ))").unwrap();
    std::fs::write(
        dir.join("acaddoc.lsp"),
        "(setq docvar 1) (autoload \"lazy\" '(\"LAZY\")) (defun S::STARTUP () (setq started (1+ (if started started 0))))",
    )
    .unwrap();
    std::fs::write(dir.join("lazy.lsp"), "(defun c:lazy () (setq lazy-ran 1) (princ \"lazy ran\") (princ))").unwrap();
    // A drawing's own folder is never read: an acaddoc.lsp next to it must not run.
    let drawings = dir.join("drawings");
    std::fs::create_dir_all(&drawings).unwrap();
    std::fs::write(drawings.join("acaddoc.lsp"), "(setq evil 1)").unwrap();

    let mut s = session();
    s.set_lisp_support_dir(Some(dir.clone()));
    s.lisp_tick();
    let v = |s: &Session, n: &str| to_text(&s.lisp_var(n), true);
    assert_eq!(v(&s, "DOCVAR"), "1");
    assert_eq!(v(&s, "STARTED"), "1", "S::STARTUP ran once");
    assert!(matches!(s.lisp_var("C:FROMACAD"), Value::Func(_)));
    s.lisp_tick();
    assert_eq!(v(&s, "STARTED"), "1", "a drawing gets its startup files once");
    // autoload: the command loads its file on first use.
    s.cmdline("LAZY").unwrap();
    assert_eq!(v(&s, "LAZY-RAN"), "1");
    assert_eq!(s.log.last().map(String::as_str), Some("lazy ran"));

    // A new drawing: acaddoc.lsp again, acad.lsp only once per session...
    s.new_drawing(false);
    s.lisp_tick();
    assert_eq!(v(&s, "DOCVAR"), "1");
    assert!(s.lisp_var("C:FROMACAD").is_nil());
    // ...unless ACADLSPASDOC is on.
    crate::sysvars::set(&mut s, "ACADLSPASDOC", &serde_json::json!(1)).unwrap();
    s.new_drawing(false);
    s.lisp_tick();
    assert!(matches!(s.lisp_var("C:FROMACAD"), Value::Func(_)));

    // A drawing opened from a folder with its own acaddoc.lsp: only the support folder's runs.
    let path = drawings.join("plan.dxf").to_string_lossy().into_owned();
    s.open_drawing(cadcraft_doc::Drawing::new_metric(), "plan.dxf", Some(path));
    s.lisp_tick();
    assert_eq!(v(&s, "DOCVAR"), "1");
    assert!(s.lisp_var("EVIL").is_nil());

    // No support folder: nothing loads.
    let mut s = session();
    s.lisp_tick();
    assert!(s.lisp_var("DOCVAR").is_nil());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deeply_nested_lists_drop_without_overflowing_the_stack() {
    let _heavy = super::value::HEAVY_CELLS.lock().unwrap_or_else(|e| e.into_inner());
    // Nested in the car a million deep: dropping it must not recurse.
    let mut v = Value::Nil;
    for _ in 0..1_000_000 {
        v = Value::list(vec![v]);
    }
    drop(v);
    // Functions nested in function bodies too.
    let mut f = Value::Nil;
    for _ in 0..200_000 {
        f = Value::Func(std::sync::Arc::new(super::value::Func { name: None, params: Vec::new(), locals: Vec::new(), body: Value::list(vec![f]) }));
    }
    drop(f);
    // The same built by a routine and dropped when the variable is cleared.
    let mut s = session();
    assert_eq!(ok(&mut s, "(setq x nil) (repeat 100000 (setq x (list x))) (setq x nil) 1"), "1");
}

#[test]
fn too_many_list_cells_stop_the_routine() {
    let _heavy = super::value::HEAVY_CELLS.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = session();
    s.lisp.limits.cells = 50_000;
    let e = eval(&mut s, "(setq l nil) (repeat 2000000 (setq l (cons 1 l)))").unwrap_err();
    assert!(e.contains("too many list cells"), "{e}");
    // The variable still holds what was made, but the drawing's LISP stays usable.
    assert_eq!(ok(&mut s, "(setq l nil) (+ 1 2)"), "3");
    // A doubling list is stopped as well.
    let e = eval(&mut s, "(setq l '(1)) (repeat 100 (setq l (append l l)))").unwrap_err();
    assert!(e.contains("too many list cells"), "{e}");
    assert_eq!(ok(&mut s, "(setq l nil) (length '(1 2 3))"), "3");
}

#[test]
fn strings_are_capped() {
    let _heavy = super::value::HEAVY_CELLS.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = session();
    // A string doubled until it would not fit in memory.
    let e = eval(&mut s, "(setq s \"x\") (repeat 100 (setq s (strcat s s)))").unwrap_err();
    assert!(e.contains("string longer than"), "{e}");
    // Printing a list that shares its parts (exponentially long text) is cut.
    let t = ok(&mut s, "(setq x '(1 2)) (repeat 40 (setq x (list x x))) (strlen (vl-prin1-to-string x))");
    assert!(t.parse::<usize>().unwrap() <= super::value::MAX_STRING + 64, "{t}");
    ok(&mut s, "(setq x nil s nil)");
    // Many strings, each short, add up to the run's budget.
    s.lisp.limits.string_bytes = 1_000_000;
    let e = eval(&mut s, "(setq a \"0123456789\") (repeat 20000 (setq b (strcat a a a a a a a a a a)))").unwrap_err();
    assert!(e.contains("too much text"), "{e}");
    let e = eval(&mut s, "(repeat 200000 (princ \"0123456789\"))").unwrap_err();
    assert!(e.contains("too much text"), "{e}");
    // The budget is per run: the next one starts afresh.
    assert_eq!(ok(&mut s, "(strcat \"a\" \"b\")"), "\"ab\"");
}

#[test]
fn a_panic_inside_a_routine_ends_it_with_an_error() {
    let mut s = session();
    let n = |s: &Session| s.doc().unwrap().model.handles().len();
    let before = n(&s);
    let e = eval(&mut s, "(entmake '((0 . \"LINE\") (10 0 0 0) (11 5 0 0))) (cadcraft-test-panic)").unwrap_err();
    assert!(e.contains("internal error"), "{e}");
    assert_eq!(n(&s), before, "the drawing is put back");
    assert_eq!(ok(&mut s, "(+ 1 2)"), "3");

    // A routine carried on by the per-frame tick.
    let r = s.lisp_eval("(repeat 500000 (setq i 1)) (cadcraft-test-panic)", false, false, Vec::new()).unwrap();
    assert!(r.is_none(), "the routine gave the interface a frame");
    assert!(s.lisp_busy());
    for _ in 0..100 {
        if !s.lisp_busy() {
            break;
        }
        s.lisp_tick();
    }
    assert!(!s.lisp_running());
    assert!(s.log.iter().any(|l| l.contains("internal error")), "{:?}", s.log);
    assert_eq!(ok(&mut s, "(+ 1 2)"), "3");
}

#[test]
fn angtof_reads_the_string_in_the_given_units() {
    let mut s = Session::new();
    assert_eq!(ok(&mut s, "(angtof \"1.5\" 3)"), "1.5");
    assert_eq!(ok(&mut s, "(angtof \"180\" 0)"), "3.14159");
    assert_eq!(ok(&mut s, "(angtof \"180\" 7)"), "nil");
    assert_eq!(ok(&mut s, "(angtof \"180\" \"x\")"), "nil");
}

// ---------------------------------------------------------------------------
//  xdata: a routine's own data on an entity
//
//  A routine that keeps its own data on the drawing (an electrical point knowing which panel
//  feeds it, a conduit knowing which two points it joins) writes it as xdata, because xdata
//  survives COPY, ARRAY and the file. Without these four things it cannot:
//
//    regapp         the first line of every such routine
//    entget + apps  reading it back
//    entmod + (-3)  writing it
//    and above all: an ORDINARY entmod not throwing it away
// ---------------------------------------------------------------------------

/// A LINE to hang data on.
fn a_line(s: &mut Session) {
    ok(s, "(entmake '((0 . \"LINE\") (8 . \"0\") (10 0 0 0) (11 5 0 0))) (setq e (entlast))");
}

#[test]
fn regapp_answers_the_name() {
    let mut s = session();
    assert_eq!(ok(&mut s, "(regapp \"ACME\")"), "\"ACME\"");
    assert_eq!(ok(&mut s, "(regapp \"\")"), "nil");
}

#[test]
fn an_entity_list_has_no_xdata_unless_it_was_asked_for() {
    let mut s = session();
    a_line(&mut s);
    ok(&mut s, "(entmod (append (entget e) '((-3 (\"ACME\" (1000 . \"panel=P-1\"))))))");
    // without the second argument there is no -3 at all, which is how AutoCAD answers
    assert_eq!(ok(&mut s, "(if (assoc -3 (entget e)) \"yes\" \"no\")"), "\"no\"");
    assert_eq!(ok(&mut s, "(if (assoc -3 (entget e '(\"ACME\"))) \"yes\" \"no\")"), "\"yes\"");
}

#[test]
fn xdata_written_from_lisp_reads_back() {
    let mut s = session();
    a_line(&mut s);
    ok(&mut s, "(regapp \"ACME\")");
    ok(&mut s, "(entmod (append (entget e) '((-3 (\"ACME\" (1000 . \"panel=P-1\") (1000 . \"kind=outlet\") (1070 . 7) (1040 . 2.5))))))");
    let v = ok(&mut s, "(cdr (assoc -3 (entget e '(\"ACME\"))))");
    assert_eq!(v, "((\"ACME\" (1000 . \"panel=P-1\") (1000 . \"kind=outlet\") (1070 . 7) (1040 . 2.5)))", "{v}");
    // and the integer comes back an integer, not 7.0: whoever reads it does arithmetic
    assert_eq!(ok(&mut s, "(cdr (assoc 1070 (cdar (cdr (assoc -3 (entget e '(\"ACME\")))))))"), "7");
}

#[test]
fn an_ordinary_entmod_does_not_throw_the_xdata_away() {
    // THIS IS THE BUG THE REST OF THE WORK CAME FROM.
    //
    // `common` comes from the group list, and xdata is not an ordinary group - so it came back
    // empty and `e.common = common` wiped the entity's xdata. Changing a layer destroyed
    // another application's data, silently. For a routine that keeps its model in the drawing
    // that is not a detail: it loses the job on the first command that touches anything.
    let mut s = session();
    a_line(&mut s);
    ok(&mut s, "(entmod (append (entget e) '((-3 (\"ACME\" (1000 . \"panel=P-1\"))))))");
    // an entmod that says nothing about xdata: it only changes the layer
    ok(&mut s, "(setq d (entget e)) (entmod (subst (cons 8 \"OTHER\") (assoc 8 d) d))");
    assert_eq!(ok(&mut s, "(cdr (assoc 8 (entget e)))"), "\"OTHER\"", "the layer changed");
    let v = ok(&mut s, "(cdr (assoc -3 (entget e '(\"ACME\"))))");
    assert_eq!(v, "((\"ACME\" (1000 . \"panel=P-1\")))", "the xdata survived: {v}");
}

#[test]
fn writing_one_application_leaves_another_alone() {
    let mut s = session();
    a_line(&mut s);
    ok(&mut s, "(entmod (append (entget e) '((-3 (\"PE_URL\" (1000 . \"https://x/7\"))))))");
    ok(&mut s, "(entmod (append (entget e) '((-3 (\"ACME\" (1000 . \"kind=conduit\"))))))");
    // both are there, and "*" asks for all of them
    let v = ok(&mut s, "(cdr (assoc -3 (entget e '(\"*\"))))");
    assert!(v.contains("PE_URL") && v.contains("ACME"), "{v}");
    // and writing one of them again does not touch the other
    ok(&mut s, "(entmod (append (entget e) '((-3 (\"ACME\" (1000 . \"kind=feed\"))))))");
    let v = ok(&mut s, "(cdr (assoc -3 (entget e '(\"PE_URL\"))))");
    assert_eq!(v, "((\"PE_URL\" (1000 . \"https://x/7\")))", "{v}");
}

#[test]
fn naming_an_application_with_no_groups_removes_it() {
    let mut s = session();
    a_line(&mut s);
    ok(&mut s, "(entmod (append (entget e) '((-3 (\"ACME\" (1000 . \"kind=conduit\"))))))");
    ok(&mut s, "(entmod (append (entget e) '((-3 (\"ACME\")))))");
    assert_eq!(ok(&mut s, "(if (assoc -3 (entget e '(\"ACME\"))) \"yes\" \"no\")"), "\"no\"");
}

#[test]
fn the_appid_table_reports_what_the_drawing_holds() {
    let mut s = session();
    a_line(&mut s);
    assert_eq!(ok(&mut s, "(if (tblsearch \"APPID\" \"ACME\") \"yes\" \"no\")"), "\"no\"");
    ok(&mut s, "(entmod (append (entget e) '((-3 (\"ACME\" (1000 . \"kind=conduit\"))))))");
    assert_eq!(ok(&mut s, "(if (tblsearch \"APPID\" \"ACME\") \"yes\" \"no\")"), "\"yes\"");
    // and a name already in use is not registered again
    assert_eq!(ok(&mut s, "(regapp \"ACME\")"), "nil");
}

#[test]
fn a_point_in_xdata_survives_the_round_trip() {
    let mut s = session();
    a_line(&mut s);
    ok(&mut s, "(entmod (append (entget e) '((-3 (\"ACME\" (1010 1.5 2.5 0.0))))))");
    let v = ok(&mut s, "(cdr (assoc -3 (entget e '(\"ACME\"))))");
    assert_eq!(v, "((\"ACME\" (1010 1.5 2.5 0.0)))", "{v}");
}

#[test]
fn an_entity_without_xdata_answers_nothing() {
    let mut s = session();
    a_line(&mut s);
    assert_eq!(ok(&mut s, "(if (assoc -3 (entget e '(\"ACME\"))) \"yes\" \"no\")"), "\"no\"");
    assert_eq!(ok(&mut s, "(if (assoc -3 (entget e '(\"*\"))) \"yes\" \"no\")"), "\"no\"");
}

#[test]
fn an_entmod_does_not_throw_a_block_reference_attributes_away() {
    // Same cause as the xdata: `entget` does not emit a block reference's attributes (there
    // is no group for them in the list), so the rebuilt entity came back with none and the
    // entmod wiped every value. In an electrical drawing that is the circuit number of each
    // outlet.
    //
    // The check is on the model and not through LISP on purpose: the attributes are part of
    // the Insert rather than entities of their own, so `entnext` never shows them - which is
    // exactly why the group list could not carry them back.
    const DXF: &str = "0
SECTION
2
ENTITIES
0
INSERT
5
2A
8
0
2
P
10
0
20
0
30
0
66
1
0
ATTRIB
5
2B
8
0
10
0
20
0
30
0
40
0.2
1
7
2
CIRCUIT
70
0
0
SEQEND
5
2C
0
ENDSEC
0
EOF
";

    set_entity_codec(EntityCodec { groups: cadcraft_io::entity_groups, entity: cadcraft_io::entity_from_groups });
    let d = cadcraft_io::read_dxf(DXF.as_bytes()).unwrap();
    let mut s = Session::empty();
    s.open_drawing(d, "attribs", None);

    let values = |s: &Session| -> Vec<String> {
        let doc = s.doc().unwrap();
        doc.model
            .iter()
            .find_map(|e| match &e.kind {
                cadcraft_doc::EntityKind::Insert(i) => Some(i.attribs.iter().map(|a| a.text.value.clone()).collect()),
                _ => None,
            })
            .unwrap_or_default()
    };
    assert_eq!(values(&s), vec!["7".to_string()], "the attribute was read from the file");

    // an entmod that says nothing about attributes: it only changes the layer
    ok(&mut s, "(setq e (handent \"2A\")) (setq d (entget e)) (entmod (subst (cons 8 \"OTHER\") (assoc 8 d) d))");
    assert_eq!(ok(&mut s, "(cdr (assoc 8 (entget (handent \"2A\"))))"), "\"OTHER\"", "the layer changed");
    assert_eq!(values(&s), vec!["7".to_string()], "and the attribute survived");
}

#[test]
fn an_entmake_keeps_the_xdata_its_list_carries() {
    // The other door into the same loss. `list_to_groups` leaves `-3` out, and entmake hands
    // what is left to `entity_from_list` - so the entity was made and the data it arrived with
    // was dropped, by the call that was asked to store it.
    let mut s = session();
    ok(&mut s, "(entmake '((0 . \"LINE\") (8 . \"0\") (10 0 0 0) (11 5 0 0) (-3 (\"ACME\" (1000 . \"panel=P-1\")))))");
    ok(&mut s, "(setq e (entlast))");
    let v = ok(&mut s, "(cdr (assoc -3 (entget e '(\"ACME\"))))");
    assert_eq!(v, "((\"ACME\" (1000 . \"panel=P-1\")))", "the xdata came in with the entity: {v}");
}

#[test]
fn a_real_in_xdata_keeps_all_of_its_digits() {
    // The AutoLISP display format rounds to six significant digits, which is right for showing
    // a number and wrong for storing one: 1234567.89 was stored as 1234570.0, so a coordinate
    // kept in xdata moved two units on the way in and nothing said so.
    let mut s = session();
    a_line(&mut s);
    ok(&mut s, "(entmod (append (entget e) '((-3 (\"ACME\" (1040 . 1234567.89) (1010 1234567.89 0.1234567890123 2.5))))))");
    ok(&mut s, "(setq x (car (cdr (assoc -3 (entget e '(\"ACME\"))))))");
    assert_eq!(ok(&mut s, "(if (equal (cdr (nth 1 x)) 1234567.89 1e-9) \"yes\" \"no\")"), "\"yes\"", "the real kept its digits");
    assert_eq!(ok(&mut s, "(if (equal (nth 1 (nth 2 x)) 1234567.89 1e-9) \"yes\" \"no\")"), "\"yes\"", "and so did the point's x");
    assert_eq!(ok(&mut s, "(if (equal (nth 2 (nth 2 x)) 0.1234567890123 1e-15) \"yes\" \"no\")"), "\"yes\"", "and its y");
}

#[test]
fn an_appid_in_a_block_definition_is_in_the_drawing_too() {
    // An application used only inside a block definition, or on a layout, holds data in this
    // drawing just as much as one used in model space. Reading `model` alone answered that it
    // was not there, so `regapp` handed its name back as free and `tblsearch` denied it.
    let mut s = session();
    ok(&mut s, "(entmake '((0 . \"BLOCK\") (2 . \"P\") (10 0 0 0) (70 . 0)))");
    ok(&mut s, "(entmake '((0 . \"LINE\") (8 . \"0\") (10 0 0 0) (11 1 0 0) (-3 (\"INBLOCK\" (1000 . \"kind=outlet\")))))");
    ok(&mut s, "(entmake '((0 . \"ENDBLK\")))");
    assert_eq!(ok(&mut s, "(if (tblsearch \"APPID\" \"INBLOCK\") \"yes\" \"no\")"), "\"yes\"", "the table sees it");
    assert_eq!(ok(&mut s, "(regapp \"INBLOCK\")"), "nil", "and regapp does not hand the name out as free");
}

#[test]
fn an_entmod_keeps_every_part_of_common_the_group_list_cannot_carry() {
    // The general shape of the two fixes above, as a guard. `entity_from_list` rebuilds `common`
    // from the group list, so every part of Common the list has no group for came back empty and
    // was written over the entity: the xdata did, and so do the annotative scales, which ride in
    // a separate record rather than in the entity's groups. Here every field is set away from its
    // default and only the layer is changed, so anything that does not survive is a part of the
    // entity that a layer change destroys - and a field added to Common later without a group of
    // its own fails here instead of quietly going missing.
    let mut s = session();
    a_line(&mut s);
    let h = s.doc().unwrap().model.handles().first().copied().unwrap();
    s.doc_mut().unwrap().model.modify(h, |e| {
        e.common.color = cadcraft_doc::color::Color::Index(3);
        e.common.linetype = "DASHED".into();
        e.common.lineweight = cadcraft_doc::Lineweight::Mm100(50);
        e.common.ltscale = 2.5;
        e.common.transparency = cadcraft_doc::Transparency::Percent(40);
        e.common.visible = false;
        e.common.thickness = 1.25;
        e.common.extrusion = cadcraft_doc::geom::Vec3::new(0.0, 0.0, -1.0);
        e.common.plot_style = "Style1".into();
        e.common.anno_scales = vec![cadcraft_doc::AnnoScale { name: "1:50".into(), paper_units: 1.0, drawing_units: 50.0 }];
        e.common.xdata =
            vec![cadcraft_doc::RawTag { code: 1001, value: "ACME".into() }, cadcraft_doc::RawTag { code: 1000, value: "panel=P-1".into() }];
    });
    let common = |s: &Session| s.doc().unwrap().model.get(h).unwrap().common.clone();
    let before = common(&s);

    ok(&mut s, "(setq d (entget e)) (entmod (subst (cons 8 \"OTHER\") (assoc 8 d) d))");

    let mut wanted = before;
    wanted.layer = "OTHER".into();
    assert_eq!(common(&s), wanted, "a layer change took something else with it");
}

#[test]
fn a_group_the_file_cannot_hold_is_refused_instead_of_written() {
    // THE HOLE THE REVIEW FOUND, AND THE WORST OF THEM.
    //
    // `(0 . "CIRCLE")` in a `-3` list went into the entity's xdata and then into the file, where
    // a group `0` ends the entity: every reader took what followed as a new one, so a drawing of
    // one line came back as a line and a circle. A typo - `(0 . …)` for `(1000 . …)` - was
    // enough. The check is in `tag_from` now, and this test saves and reopens, because the harm
    // was in the file rather than in memory.
    let mut s = session();
    a_line(&mut s);
    let bad = eval(&mut s, "(entmod (append (entget e) '((-3 (\"ACME\" (0 . \"CIRCLE\") (1000 . \"ok\"))))))");
    assert_eq!(bad, Err("0 inside -3: xdata holds group codes 1000 and up".into()));
    // nothing of it reached the entity, and nothing reaches the file
    assert_eq!(ok(&mut s, "(if (assoc -3 (entget e '(\"*\"))) \"yes\" \"no\")"), "\"no\"");
    let saved = cadcraft_io::write_dxf(s.doc().unwrap());
    let back = cadcraft_io::read_dxf(saved.as_bytes()).unwrap();
    assert_eq!(back.model.len(), 1, "the saved drawing still holds one entity");
}

#[test]
fn an_xdata_group_has_to_be_the_kind_the_file_holds() {
    let mut s = session();
    a_line(&mut s);
    let bad = |s: &mut Session, item: &str| eval(s, &format!("(entmod (append (entget e) '((-3 (\"ACME\" {item})))))"));

    // 1070 is 16 bits wide in the file, and another program reading it expects that
    assert!(bad(&mut s, "(1070 . 99999)").is_err(), "1070 out of range");
    assert!(bad(&mut s, "(1071 . \"abc\")").is_err(), "1071 is not a string");
    assert!(bad(&mut s, "(1040 . \"abc\")").is_err(), "1040 is not a string");
    assert!(bad(&mut s, "(1002 . \"nao\")").is_err(), "1002 is a brace or nothing");
    assert!(bad(&mut s, "(1010 . 1.0)").is_err(), "1010 wants a point");
    // and the braces have to close, or the reader drops the whole application
    assert!(bad(&mut s, "(1002 . \"{\") (1000 . \"x\")").is_err(), "an unclosed brace");
    assert!(bad(&mut s, "(1002 . \"}\") (1002 . \"{\")").is_err(), "closed before it opened");

    // what the file does hold goes in, braces and all
    ok(&mut s, "(entmod (append (entget e) '((-3 (\"ACME\" (1002 . \"{\") (1070 . 7) (1002 . \"}\"))))))");
    let v = ok(&mut s, "(cdr (assoc -3 (entget e '(\"ACME\"))))");
    assert_eq!(v, "((\"ACME\" (1002 . \"{\") (1070 . 7) (1002 . \"}\")))", "{v}");
}

#[test]
fn a_group_dxf_gives_no_type_to_still_makes_the_round_trip() {
    // The refusal is about what the file cannot hold, not about codes this module has not been
    // told about: a drawing from another program may carry one, `entget` shows it, and
    // `(entmod (entget e))` must not be the call that loses it.
    let mut s = session();
    a_line(&mut s);
    ok(&mut s, "(entmod (append (entget e) '((-3 (\"ACME\" (1009 . \"odd but legal\"))))))");
    let v = ok(&mut s, "(cdr (assoc -3 (entget e '(\"ACME\"))))");
    assert_eq!(v, "((\"ACME\" (1009 . \"odd but legal\")))", "{v}");
}
