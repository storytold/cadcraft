#![allow(clippy::unwrap_used)]

use cadcraft_doc::Handle;
use serde_json::json;

use super::value::{Value, to_text};
use crate::Session;
use crate::cmd::file::{EntityCodec, set_entity_codec, set_entity_sub_record, set_object_records};

pub(super) fn session() -> Session {
    set_entity_codec(EntityCodec { groups: cadcraft_io::entity_groups, entity: cadcraft_io::entity_from_groups });
    // ... and one record after an entity's own (an INSERT's ATTRIBs and SEQEND) for `entnext`.
    set_entity_sub_record(cadcraft_io::entity_sub_record);
    // ... and non-graphic objects (tables, dictionaries, layouts) as DXF records.
    set_object_records(cadcraft_io::object_records);
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

/// A step that walks a long list counts its work: one `length` of 32768 cells is past a 1000-step
/// slice, so the slice yields after it.
#[test]
fn walking_a_long_list_counts_against_the_slice() {
    use super::eval::{Ctx, Machine, Need, Outcome};
    let _heavy = super::value::HEAVY_CELLS.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = session();
    let mut env = super::Env::default();
    let mut ctx = Ctx { s: &mut s, env: &mut env, depth: 0 };
    // Built before the measured run, so only the `length` is counted.
    let mut build = Machine::forms(super::read::read_all("(setq l '(1)) (repeat 15 (setq l (append l l)))").unwrap());
    assert!(matches!(build.run(&mut ctx, u64::MAX), Outcome::Done(_)));
    let mut m = Machine::forms(super::read::read_all("(length l)").unwrap());
    // Counted as one step, the walk left the few steps of `(length l)` well inside the slice.
    assert!(matches!(m.run(&mut ctx, 1_000), Outcome::Wait(Need::Yield)), "the walk was not charged");
    assert!(m.used > 32_768, "{}", m.used);
    match m.run(&mut ctx, u64::MAX) {
        Outcome::Done(v) => assert_eq!(to_text(&v, true), "32768"),
        _ => panic!("the length did not finish"),
    }
    assert_eq!(super::value::take_work(), 0, "nothing is left for the next run");
}

/// A routine is charged the work its slice did, not the slice's length: one `length` of 2^19
/// cells is far past a 400000-step slice.
#[test]
fn a_routine_is_charged_the_work_of_its_slice() {
    let _heavy = super::value::HEAVY_CELLS.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = session();
    ok(&mut s, "(setq l '(1)) (repeat 19 (setq l (append l l))) nil");
    let r = s.lisp_eval("(length l) (repeat 10 (setq x 1))", false, false, Vec::new()).unwrap();
    assert!(r.is_none(), "the routine gave the interface a frame");
    assert!(s.lisp_busy());
    let steps = s.lisp.run.as_ref().map(|r| r.steps).unwrap_or(0);
    assert!(steps >= 1 << 19, "{steps}");
    for _ in 0..100 {
        if !s.lisp_busy() {
            break;
        }
        s.lisp_tick();
    }
    assert!(!s.lisp_running());
    ok(&mut s, "(setq l nil)");
}

/// A run's last step walking a long list is charged to that run, not left for the next one: here
/// it fails after the walk (a list of numbers isn't a list of strings), so no step follows it.
#[test]
fn the_last_steps_list_walk_is_charged_to_its_run() {
    let _heavy = super::value::HEAVY_CELLS.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = session();
    let mut env = super::Env::default();
    env.globals.insert(super::value::Sym::new("L"), Value::list(vec![Value::Int(1); 100_000]));
    let mut m = super::eval::Machine::forms(super::read::read_all("(acad_strlsort l)").unwrap());
    let mut ctx = super::eval::Ctx { s: &mut s, env: &mut env, depth: 0 };
    assert!(matches!(m.run(&mut ctx, u64::MAX), super::eval::Outcome::Error(_)));
    assert!(m.used > 100_000, "{}", m.used);
    assert_eq!(super::value::take_work(), 0, "nothing is left for the next run");
}

/// Nested callbacks share one cap: an inner endless callback retried by an outer one stops.
#[test]
fn nested_callback_steps_count_against_the_outer_callback() {
    let mut s = session();
    s.lisp.limits.callback_steps = 10_000;
    // Each level retried the endless level inside it with a fresh cap: 10000^3 steps, a hang.
    let code = "(defun g (a b) (while t)) \
                (defun f (a b) (while t (vl-catch-all-apply 'vl-sort (list '(1 2) 'g)))) \
                (defun h (a b) (while t (vl-catch-all-apply 'vl-sort (list '(1 2) 'f)))) \
                (vl-sort '(1 2) 'h)";
    assert_eq!(eval(&mut s, code), Err("callback stopped after 10000 steps".into()));
    // A callback's own steps count once, however many slices it runs in: three slices do three
    // times the work of one.
    let counts: Vec<u64> = [400_000, 1_200_000]
        .into_iter()
        .map(|cap| {
            s.lisp.limits.callback_steps = cap;
            ok(&mut s, "(setq n 0)");
            assert!(eval(&mut s, "(vl-remove-if '(lambda (x) (while t (setq n (1+ n)))) '(1))").is_err());
            ok(&mut s, "n").parse().unwrap()
        })
        .collect();
    assert!(counts[1] * 10 >= counts[0] * 29, "{counts:?}");
}

/// The callback cap holds for each callback: a sort's comparisons together take more steps than
/// it, and all their work still reaches the enclosing run.
#[test]
fn each_callback_has_its_own_cap_and_its_work_reaches_the_caller() {
    let mut s = session();
    s.lisp.limits.callback_steps = 1_000;
    let mut env = super::Env::default();
    env.globals.insert(super::value::Sym::new("L"), Value::list((0..300).rev().map(Value::Int)));
    let mut m = super::eval::Machine::forms(super::read::read_all("(vl-sort l (function (lambda (a b) (< a b))))").unwrap());
    let mut ctx = super::eval::Ctx { s: &mut s, env: &mut env, depth: 0 };
    let v = match m.run(&mut ctx, u64::MAX) {
        super::eval::Outcome::Done(v) => v,
        super::eval::Outcome::Error(e) => panic!("the sort failed: {e:?}"),
        super::eval::Outcome::Wait(_) => panic!("the sort waited"),
    };
    assert_eq!(to_text(&v, true), to_text(&Value::list((0..300).map(Value::Int)), true));
    // Over a thousand comparisons of several steps each, together well past the cap.
    assert!(m.used > 3_000, "{}", m.used);
    assert_eq!(super::value::take_work(), 0, "nothing is left for the next run");
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

/// Callbacks that call callbacks share one allowance: a branching routine can't multiply it.
#[test]
fn nested_callbacks_share_one_step_allowance() {
    let mut s = session();
    s.lisp.limits.callback_steps = 1_000_000;
    // 4^14 leaf calls if every callback took a fresh allowance; the whole tree gets one.
    let started = std::time::Instant::now();
    let e = eval(&mut s, "(defun f (n) (if (> n 0) (vl-every '(lambda (x) (f (1- n)) t) '(1 2 3 4)) t)) (f 14)").unwrap_err();
    assert!(e.contains("callback stopped after 1000000 steps"), "{e}");
    assert!(started.elapsed() < std::time::Duration::from_secs(60));
    // The next run starts afresh.
    assert_eq!(ok(&mut s, "(f 1)"), "T");
}

/// An outer callback's own steps count against the allowance before the callbacks it calls run.
#[test]
fn an_outer_callbacks_steps_are_charged_before_a_nested_one() {
    let mut s = session();
    s.lisp.limits.callback_steps = 1_000;
    ok(&mut s, "(setq inner-steps 0)");
    // The outer callback spends ~600 steps, then a nested one tries to spend ~600 more.
    let r = eval(
        &mut s,
        "(vl-remove-if '(lambda (x) (repeat 150 (setq a 1)) (vl-remove-if '(lambda (y) (repeat 150 (setq inner-steps (1+ inner-steps)))) '(1)) nil) '(1))",
    );
    assert_eq!(r, Err("callback stopped after 1000 steps".into()));
    assert!(ok(&mut s, "inner-steps").parse::<i64>().unwrap() < 150);
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

/// Output with many lines moves to the history in one pass, not one buffer shift per line.
#[test]
fn printing_many_lines_is_linear() {
    let mut s = session();
    let started = std::time::Instant::now();
    ok(&mut s, "(setq x \"a\\r\\n\") (repeat 17 (setq x (strcat x x))) (princ x) (princ)");
    assert!(started.elapsed() < std::time::Duration::from_secs(10), "took {:?}", started.elapsed());
    assert_eq!(s.log.last().map(String::as_str), Some("a"));
}

#[test]
fn lisp_pline_keeps_current_elevation() {
    let mut s = session();
    ok(&mut s, r#"(setvar "ELEVATION" 800.0)"#);
    ok(&mut s, r#"(command "_PLINE" '(0 0) '(1000 0) '(1000 100) "")"#);
    assert_eq!(ok(&mut s, "(cdr (assoc 38 (entget (entlast))))"), "800.0");

    ok(&mut s, r#"(setvar "ELEVATION" -2.5)"#);
    s.execute("pline", &serde_json::json!({"vertices": [[0, 0], [10, 0]], "width": 2})).unwrap();
    assert_eq!(ok(&mut s, "(cdr (assoc 38 (entget (entlast))))"), "-2.5");
    assert_eq!(ok(&mut s, "(cdr (assoc 43 (entget (entlast))))"), "2.0");
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

/// entlast, entnext and (ssget "L") see what the routine itself just made, changed or deleted:
/// the order they share isn't kept past an edit, even one that leaves the drawing where it was.
#[test]
fn entlast_follows_edits_within_one_routine() {
    let mut s = session();
    assert_eq!(
        ok(
            &mut s,
            "(entmake '((0 . \"LINE\") (10 0.0 0.0 0.0) (11 1.0 0.0 0.0))) (setq a (entlast))
             (entmake '((0 . \"LINE\") (10 0.0 0.0 0.0) (11 2.0 0.0 0.0))) (setq b (entlast))
             (list (eq a b) (cdr (assoc 11 (entget b))) (eq (entnext a) b) (eq (ssname (ssget \"L\") 0) b))"
        ),
        "(nil (2.0 0.0 0.0) T T)"
    );
    // Deleting the last makes the one before it last; entdel brings it back last again.
    assert_eq!(ok(&mut s, "(entdel b) (setq c (entlast)) (entdel b) (list (eq c a) (eq (entlast) b))"), "(T T)");
    // A command, an entmod and entlast, over and over in one routine: each entlast is the line
    // just drawn.
    assert_eq!(
        ok(
            &mut s,
            "(setq seen nil i 0)
             (repeat 40
               (command \"_.LINE\" (list i 10) (list i 20) \"\")
               (setq e (entlast) d (entget e))
               (entmod (subst (cons 8 \"MOVED\") (assoc 8 d) d))
               (if (and (not (member e seen)) (equal (cdr (assoc 10 (entget (entlast)))) (list (float i) 10.0 0.0) 1e-9))
                 (setq seen (cons e seen)))
               (setq i (1+ i)))
             (list (length seen) (cdr (assoc 8 (entget (entlast)))))"
        ),
        "(40 \"MOVED\")"
    );
    // The same across routines, with an undo between them.
    for i in 0..10 {
        ok(&mut s, &format!("(entmake '((0 . \"CIRCLE\") (10 {i}.0 50.0 0.0) (40 . 1.0)))"));
        assert_eq!(ok(&mut s, "(cadr (assoc 10 (entget (entlast))))"), format!("{i}.0"));
        s.undo().unwrap();
        ok(&mut s, &format!("(entmake '((0 . \"CIRCLE\") (10 {i}.0 60.0 0.0) (40 . 1.0)))"));
        assert_eq!(ok(&mut s, "(caddr (assoc 10 (entget (entlast))))"), "60.0");
    }
}

/// entmod of a block reference keeps its attributes and their values: they aren't in its list
/// (in AutoCAD they are entities of their own, which entmod of the INSERT leaves alone).
#[test]
fn entmod_of_a_block_reference_keeps_its_attributes() {
    let mut s = session();
    ok(
        &mut s,
        "(entmake '((0 . \"BLOCK\") (2 . \"TAGGED\") (70 . 0) (10 0.0 0.0 0.0)))
         (entmake '((0 . \"CIRCLE\") (10 0.0 0.0 0.0) (40 . 1.0)))
         (entmake '((0 . \"ATTDEF\") (10 0.0 2.0 0.0) (40 . 1.0) (1 . \"\") (3 . \"Name\") (2 . \"PSTN\") (70 . 0)))
         (entmake '((0 . \"ENDBLK\")))",
    );
    s.execute("insert", &serde_json::json!({"name": "TAGGED", "at": [10, 10], "attribs": {"PSTN": "ARK1"}})).unwrap();
    let insert = |s: &Session| {
        s.doc()
            .unwrap()
            .model
            .iter()
            .find_map(|e| match &e.kind {
                cadcraft_doc::EntityKind::Insert(i) => {
                    Some((e.common.layer.clone(), i.attribs.iter().map(|a| (a.tag.clone(), a.text.value.clone())).collect::<Vec<_>>()))
                }
                _ => None,
            })
            .unwrap()
    };
    let want = vec![("PSTN".to_string(), "ARK1".to_string())];
    assert_eq!(insert(&s).1, want);
    ok(&mut s, "(setq d (entget (entlast))) (entmod (subst (cons 8 \"UGO\") (assoc 8 d) d))");
    assert_eq!(insert(&s), ("UGO".to_string(), want.clone()));
    // A new rotation keeps them too.
    ok(&mut s, "(setq d (entget (entlast))) (entmod (subst (cons 50 (/ pi 2)) (assoc 50 d) d))");
    let rotation = s
        .doc()
        .unwrap()
        .model
        .iter()
        .find_map(|e| match &e.kind {
            cadcraft_doc::EntityKind::Insert(i) => Some(i.rotation),
            _ => None,
        })
        .unwrap();
    assert!((rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "the entmod took: {rotation}");
    assert_eq!(insert(&s).1, want);
}

/// An edit that leaves every handle where it was (a layer change) keeps entlast's cached order;
/// one that adds an entity rebuilds it.
#[test]
fn entlast_keeps_its_order_across_property_edits() {
    let mut s = session();
    ok(&mut s, "(entmake '((0 . \"LINE\") (10 0.0 0.0 0.0) (11 1.0 0.0 0.0))) (entlast)");
    let uid = s.state().unwrap().uid;
    // The cached order itself is held, so a rebuilt one can't take its address.
    let order = |s: &Session| s.lisp.envs[&uid].order.as_ref().unwrap().1.clone();
    let before = order(&s);
    assert_eq!(
        ok(&mut s, "(setq d (entget (entlast))) (entmod (subst (cons 8 \"MOVED\") (assoc 8 d) d)) (cdr (assoc 8 (entget (entlast))))"),
        "\"MOVED\""
    );
    assert!(std::sync::Arc::ptr_eq(&order(&s), &before), "a layer change keeps the order");
    ok(&mut s, "(entmake '((0 . \"LINE\") (10 0.0 0.0 0.0) (11 2.0 0.0 0.0))) (entlast)");
    assert!(!std::sync::Arc::ptr_eq(&order(&s), &before), "a new entity rebuilds it");
}

/// Closing a drawing drops its LISP environment, and with it the entity stores its cached
/// order holds.
#[test]
fn closing_a_drawing_drops_its_environment() {
    let mut s = session();
    ok(&mut s, "(entmake '((0 . \"LINE\") (10 0.0 0.0 0.0) (11 1.0 0.0 0.0))) (entlast)");
    let uid = s.state().unwrap().uid;
    assert!(s.lisp.envs.get(&uid).is_some_and(|e| e.order.is_some()));
    s.execute("close", &serde_json::json!({})).unwrap();
    assert!(!s.lisp.envs.contains_key(&uid));
}

/// HANDSEED is the next handle the drawing gives out, in hex, and moves as objects are made.
#[test]
fn handseed_follows_new_objects() {
    let mut s = session();
    let seed = |s: &Session| format!("\"{:X}\"", s.doc().unwrap().next_handle().0);
    let before = ok(&mut s, "(getvar \"HANDSEED\")");
    assert_eq!(before, seed(&s));
    ok(&mut s, "(entmake '((0 . \"LINE\") (10 0.0 0.0 0.0) (11 1.0 0.0 0.0)))");
    let after = ok(&mut s, "(getvar \"HANDSEED\")");
    assert_eq!(after, seed(&s));
    assert_ne!(after, before);
    assert!(eval(&mut s, "(setvar \"HANDSEED\" \"FFFFFF\")").is_err(), "read-only");
    // A handle the file's own objects hold is skipped, as allocation skips it.
    let d = s.doc_mut().unwrap();
    let held = d.handseed;
    std::sync::Arc::make_mut(&mut d.foreign).handles.insert(cadcraft_doc::Handle(held), ("NOD".into(), String::new()));
    assert_eq!(ok(&mut s, "(getvar \"HANDSEED\")"), format!("\"{:X}\"", held + 1));
    ok(&mut s, "(entmake '((0 . \"LINE\") (10 0.0 0.0 0.0) (11 1.0 0.0 0.0)))");
    assert_eq!(ok(&mut s, "(cdr (assoc 5 (entget (entlast))))"), format!("\"{:X}\"", held + 1));
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
        "(entmake (list '(0 . \"MTEXT\") '(10 0.0 0.0 0.0) '(40 . 2.5) '(41 . 10.0) '(1 . \"abc\") (cons 50 (/ pi 3)) '(75 . 2) '(76 . 2) '(79 . 0) '(48 . 10.0) '(49 . 1.0) '(50 . 2.0) '(50 . 30.0) '(50 . 20.0)))",
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

/// LIMMIN, LIMMAX, SNAPBASE and HPORIGIN read as 2D point lists and INSBASE as a 3D one, not as JSON text (#2988).
#[test]
fn getvar_point_variables_are_point_lists() {
    let mut s = session();
    assert_eq!(ok(&mut s, "(getvar \"LIMMIN\")"), "(0.0 0.0)");
    assert_eq!(ok(&mut s, "(getvar \"LIMMAX\")"), "(12.0 9.0)");
    assert_eq!(ok(&mut s, "(getvar \"SNAPBASE\")"), "(0.0 0.0)");
    assert_eq!(ok(&mut s, "(getvar \"INSBASE\")"), "(0.0 0.0 0.0)");
    assert_eq!(ok(&mut s, "(setvar \"HPORIGIN\" '(3.0 4.0)) (getvar \"HPORIGIN\")"), "(3.0 4.0)");
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
    // AutoLISP sets angle variables in radians: ANGBASE 90 degrees.
    s.cmdline("(setvar \"ANGBASE\" (/ pi 2))").unwrap();
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

/// `getvar` and `setvar` take angle variables in radians, as AutoCAD 2024's do (each set to 30 by
/// SETVAR, or to `(/ pi 6)` by `setvar`, reads 0.5235987755982987; DIMJOGANG's 45 degrees reads
/// 0.785398), while the SETVAR command keeps degrees.
#[test]
fn getvar_and_setvar_take_angle_variables_in_radians() {
    let mut s = session();
    let degrees = |s: &Session, name: &str| crate::sysvars::get(s, name).and_then(|v| v.as_f64()).unwrap();
    // The defaults: polar tracking every 90 degrees, a 45-degree jog.
    assert_eq!(ok(&mut s, "(list (equal (getvar \"POLARANG\") (/ pi 2) 1e-12) (equal (getvar \"DIMJOGANG\") (/ pi 4) 1e-12))"), "(T T)");
    // Set from LISP in radians, shown by SETVAR in degrees.
    for (name, radians, deg) in
        [("SNAPANG", "(/ pi 6)", 30.0), ("CHAMFERD", "0.95993108859688", 55.0), ("ANGBASE", "(/ pi 2)", 90.0), ("POLARANG", "(/ pi 4)", 45.0)]
    {
        ok(&mut s, &format!("(setvar \"{name}\" {radians})"));
        assert!((degrees(&s, name) - deg).abs() < 1e-9, "{name}: {}", degrees(&s, name));
        assert_eq!(ok(&mut s, &format!("(equal (getvar \"{name}\") {radians} 1e-12)")), "T", "{name}");
    }
    // Set by the SETVAR command in degrees, read by LISP in radians.
    s.cmdline("SETVAR HPANG 45").unwrap();
    assert!((degrees(&s, "HPANG") - 45.0).abs() < 1e-9);
    assert_eq!(ok(&mut s, "(equal (getvar \"HPANG\") 0.7853981633974482 1e-12)"), "T");
    // DIMJOGANG set to a whole number of degrees still takes a fraction of one from LISP.
    s.cmdline("SETVAR DIMJOGANG 45").unwrap();
    ok(&mut s, "(setvar \"DIMJOGANG\" 0.5)");
    assert!((degrees(&s, "DIMJOGANG") - 0.5_f64.to_degrees()).abs() < 1e-9, "{}", degrees(&s, "DIMJOGANG"));
    // It takes the value the dimension style takes: `true` is a DIMSCALE of 1.
    crate::sysvars::set(&mut s, "DIMSCALE", &serde_json::json!(true)).unwrap();
    assert_eq!(crate::sysvars::get(&s, "DIMSCALE").and_then(|v| v.as_f64()), Some(1.0));
    // Other variables are as they were.
    assert_eq!(ok(&mut s, "(getvar \"ANGDIR\")"), "0");
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

/// `command` gives an entity name, alone or as `entsel` returns it (name and point), to a prompt
/// picking one object: the point picks that object, and a bare name stands at its DXF group 10
/// point. Each call and its numbers are AutoCAD 2024's (accoreconsole, read back with `entget`);
/// AutoCAD ran TRIM and EXTEND in Standard mode there.
#[test]
fn command_names_objects_at_prompts_picking_one() {
    use cadcraft_doc::EntityKind;
    let near = |a: [f64; 2], b: [f64; 2]| (0..2).all(|i| (a[i] - b[i]).abs() <= 1e-9 * a[i].abs().max(1.0));
    // The drawing's points, lines and polyline widths after `forms`, each evaluated on its own.
    let run = |forms: &[&str]| {
        let mut s = session();
        ok(&mut s, "(setvar \"TRIMEXTENDMODE\" 0)");
        for f in forms {
            ok(&mut s, f);
        }
        assert!(s.running.is_none(), "{forms:?}: a command is still running");
        let (mut points, mut lines, mut widths) = (Vec::new(), Vec::new(), Vec::new());
        for e in s.doc().unwrap().model.iter() {
            match &e.kind {
                EntityKind::Point(p) => points.push([p.p.x, p.p.y]),
                EntityKind::Line(l) => lines.push(([l.a.x, l.a.y], [l.b.x, l.b.y])),
                EntityKind::LwPolyline(pl) => widths.push(pl.const_width),
                _ => {}
            }
        }
        (points, lines, widths)
    };
    let has_point = |points: &[[f64; 2]], p: [f64; 2]| points.iter().any(|q| near(*q, p));
    let has_line = |lines: &[([f64; 2], [f64; 2])], a: [f64; 2], b: [f64; 2]| lines.iter().any(|(p, q)| near(*p, a) && near(*q, b));
    let line = |name: &str, a: [f64; 2], b: [f64; 2]| {
        format!("(setq {name} (progn (entmake '((0 . \"LINE\") (10 {} {} 0.0) (11 {} {} 0.0))) (entlast)))", a[0], a[1], b[0], b[1])
    };

    // A circle named alone stands at its centre, where nothing is drawn.
    let (points, ..) = run(&[
        "(setq a (progn (entmake '((0 . \"CIRCLE\") (10 0.0 0.0 0.0) (40 . 15.0))) (entlast)))",
        "(command \"_.MEASURE\" a 10.275598705220492)",
    ]);
    assert!(points.len() == 9 && has_point(&points, [14.89599800946832, -1.763304653744166]), "{points:?}");
    // An arc named alone stands at its centre too, as near to either end: MEASURE runs from its
    // end (250°), so its points run from 233.4° down to 34.4°, not from 46.6° up.
    let (points, ..) = run(&[
        "(setq a (progn (entmake '((0 . \"ARC\") (10 -35000.0 81000.0 0.0) (40 . 1000.0) (50 . 0.5235987755982988) (51 . 4.363323129985824))) (entlast)))",
        "(command \"_.MEASURE\" a 289.4960530820059)",
    ]);
    assert!(points.len() == 13 && has_point(&points, [-35596.04133310627, 80197.0462471419]), "{points:?}");
    assert!(has_point(&points, [-34174.59349504207, 81564.53883973838]), "{points:?}");
    // PEDIT takes a polyline by name.
    let (_, _, widths) = run(&[
        "(setq a (progn (entmake '((0 . \"LWPOLYLINE\") (100 . \"AcDbEntity\") (100 . \"AcDbPolyline\") (90 . 3) (70 . 0) (10 0.0 0.0) (10 30.0 0.0) (42 . 0.4) (10 40.0 20.0))) (entlast)))",
        "(command \"_.PEDIT\" a \"_W\" 1.25 \"\")",
    ]);
    assert_eq!(widths, [1.25]);
    // `entsel` lists: the point picks the end LENGTHEN moves, BREAK's first point, the piece
    // TRIM removes and the end EXTEND moves.
    let (_, lines, _) = run(&[
        &line("a", [0.0, 0.0], [-11.745250375031729, -27.60523670660726]),
        "(command \"_.LENGTHEN\" \"_DE\" 5.0 (list a '(-10.962233683362948 -25.764887592833446)) \"\")",
    ]);
    assert!(has_line(&lines, [0.0, 0.0], [-13.70279210420368, -32.2061094910418]), "{lines:?}");
    let (_, lines, _) = run(&[
        &line("a", [0.0, 0.0], [-97.11873949964188, 23.831710765295487]),
        "(command \"_.BREAK\" (list a '(-19.77309329359773 4.852067095763353)) '(-77.20177993977879 18.94434070674469))",
    ]);
    assert!(lines.len() == 2 && has_line(&lines, [0.0, 0.0], [-19.77309329359773, 4.852067095763352]), "{lines:?}");
    assert!(has_line(&lines, [-77.20177993977879, 18.94434070674469], [-97.11873949964188, 23.83171076529548]), "{lines:?}");
    let (_, lines, _) = run(&[
        &line("a", [-38.39479850296135, -32.02872847799295], [38.39479850296135, 32.02872847799295]),
        &line("b", [-9.422796057479543, -30.193534825244917], [-15.589739201426854, 9.328216303207391]),
        &line("c", [35.428336962443794, 7.710053620262788], [-2.7893410422311042, 19.517216189270982]),
        "(command \"_.TRIM\" a b c \"\" (list a '(1.9066151653265737 1.5904878218740615)) \"\")",
    ]);
    assert!(lines.len() == 4 && has_line(&lines, [-38.39479850296135, -32.02872847799294], [-12.50626762945319, -10.43265926101876]), "{lines:?}");
    assert!(has_line(&lines, [16.31949796010634, 13.61363490476688], [38.39479850296135, 32.02872847799294]), "{lines:?}");
    let (_, lines, _) = run(&[
        &line("a", [0.0, 0.0], [18.819750237318146, -6.7688256739971955]),
        &line("b", [17.056820153998288, -35.36317014637398], [58.2221807952743, 8.2878674503852]),
        "(command \"_.EXTEND\" a b \"\" (list a '(14.11481267798861 -5.076619255497897)) \"\")",
    ]);
    assert!(has_line(&lines, [0.0, 0.0], [37.63950047463629, -13.53765134799439]), "{lines:?}");
}

#[test]
fn a_named_object_holds_for_its_own_input_in_its_own_space() {
    use cadcraft_doc::Space;
    let mut s = session();
    ok(&mut s, "(setq a (progn (entmake '((0 . \"LINE\") (10 0.0 0.0 0.0) (11 10.0 0.0 0.0))) (entlast)))");
    // BREAK takes A at (5,3), off the line; a later pick there finds nothing.
    ok(&mut s, "(command \"_.BREAK\" (list a '(5.0 3.0)))");
    assert!(s.prompt_text().contains("Specify second break point"), "{}", s.prompt_text());
    let model = s.space();
    assert_eq!(s.pick_object(&model, cadcraft_geom::Vec2::new(5.0, 3.0), 0.5).unwrap(), None);
    s.cancel();
    // A paper-space copy of A named in model space picks nothing, not A under its start.
    let p = {
        let d = s.doc_mut().unwrap();
        let e = d.model.iter().next().unwrap();
        let (common, kind) = (e.common.clone(), e.kind.clone());
        d.add(&Space::Paper("Layout1".into()), common, kind).unwrap()
    };
    ok(&mut s, &format!("(command \"_.BREAK\" (handent \"{}\"))", p.hex()));
    assert!(s.prompt_text().contains("Select object"), "{}", s.prompt_text());
    s.cancel();
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
    // READ makes new strings inside the list it returns: they count, though the result is a list.
    let e = eval(&mut s, "(setq a \"(\\\"0123456789\\\")\") (repeat 100000 (setq b (read a)))").unwrap_err();
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

/// The handle (group 5, a DIMSTYLE's 105) of the `kind` record named `name` (group 2, or 1 for a
/// LAYOUT; any for "") in DXF text.
fn dxf_handle(dxf: &str, kind: &str, name: &str) -> String {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let pairs: Vec<(&str, &str)> = lines.chunks(2).filter_map(|c| Some((*c.first()?, *c.get(1)?))).collect();
    let mut i = 0;
    while i < pairs.len() {
        if pairs[i] == ("0", kind) {
            let rec: Vec<(&str, &str)> = pairs[i + 1..].iter().take_while(|p| p.0 != "0").copied().collect();
            let named = name.is_empty() || rec.iter().any(|p| (p.0 == "2" || (kind == "LAYOUT" && p.0 == "1")) && p.1 == name);
            if named {
                return rec.iter().find(|p| p.0 == "5" || p.0 == "105").unwrap().1.to_string();
            }
        }
        i += 1;
    }
    panic!("no {kind} {name}")
}

#[test]
fn namedobjdict_and_dictsearch_reach_the_layouts() {
    let mut s = session();
    assert!(ok(&mut s, "(namedobjdict)").starts_with("<Entity name: "));
    assert_eq!(ok(&mut s, "(eq (namedobjdict) (namedobjdict))"), "T");
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (entget (namedobjdict))))"), "\"DICTIONARY\"");
    // Keys in any case; the entry's own list, owned by the dictionary it is in.
    ok(&mut s, "(setq layouts (dictsearch (namedobjdict) \"acad_layout\"))");
    assert_eq!(ok(&mut s, "(cdr (assoc 0 layouts))"), "\"DICTIONARY\"");
    assert_eq!(ok(&mut s, "(eq (cdr (assoc 330 layouts)) (namedobjdict))"), "T");
    ok(&mut s, "(setq l1 (dictsearch (cdr (assoc -1 layouts)) \"Layout1\"))");
    assert_eq!(ok(&mut s, "(list (cdr (assoc 0 l1)) (cdr (assoc 1 (member '(100 . \"AcDbLayout\") l1))))"), "(\"LAYOUT\" \"Layout1\")");
    // The plot settings are there to read; the layout's block record is an entity name.
    assert_eq!(ok(&mut s, "(cdr (assoc 2 l1))"), "\"none_device\"");
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (entget (cdr (assoc 330 (reverse l1))))))"), "\"BLOCK_RECORD\"");
    // The name in group 5 leads back to the object.
    assert_eq!(ok(&mut s, "(eq (handent (cdr (assoc 5 l1))) (cdr (assoc -1 l1)))"), "T");
    assert_eq!(ok(&mut s, "(dictsearch (namedobjdict) \"NO_SUCH_KEY\")"), "nil");
    // A layer is not a dictionary: nothing to search.
    assert_eq!(ok(&mut s, "(dictsearch (tblobjname \"LAYER\" \"0\") \"ACAD_LAYOUT\")"), "nil");
    assert!(eval(&mut s, "(dictsearch \"ACAD_LAYOUT\" \"x\")").unwrap_err().contains("lentityp"));
    assert!(eval(&mut s, "(namedobjdict 1)").is_err());
}

#[test]
fn dictnext_walks_a_dictionary() {
    let mut s = session();
    ok(&mut s, "(setq d (cdr (assoc -1 (dictsearch (namedobjdict) \"ACAD_LAYOUT\"))))");
    let names = "(progn (setq out nil e (dictnext d T)) (while e (setq out (cons (cdr (assoc 1 (member '(100 . \"AcDbLayout\") e))) out) e (dictnext d))) (reverse out))";
    assert_eq!(ok(&mut s, names), "(\"Model\" \"Layout1\" \"Layout2\")");
    // Past the end it stays nil until rewound.
    assert_eq!(ok(&mut s, "(dictnext d)"), "nil");
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (dictnext d T)))"), "\"LAYOUT\"");
    // dictsearch with setnext: dictnext goes on after the entry found.
    ok(&mut s, "(dictsearch d \"Model\" T)");
    assert_eq!(ok(&mut s, "(cdr (assoc 1 (member '(100 . \"AcDbLayout\") (dictnext d))))"), "\"Layout1\"");
}

#[test]
fn tblobjname_names_table_records() {
    let mut s = session();
    ok(&mut s, "(setq l (tblobjname \"layer\" \"0\"))");
    assert_eq!(ok(&mut s, "(list (cdr (assoc 0 (entget l))) (cdr (assoc 2 (entget l))))"), "(\"LAYER\" \"0\")");
    // Its owner is the LAYER table.
    assert_eq!(ok(&mut s, "(cdr (assoc 2 (entget (cdr (assoc 330 (entget l))))))"), "\"LAYER\"");
    // The BLOCK table gives the block's BLOCK entity.
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (entget (tblobjname \"BLOCK\" \"*Model_Space\"))))"), "\"BLOCK\"");
    assert_eq!(ok(&mut s, "(tblobjname \"LAYER\" \"NO_SUCH_LAYER\")"), "nil");
    // Names outside ASCII: found in the case they have, ASCII letters in any case.
    s.execute("layer.new", &serde_json::json!({"name": "ébauche"})).unwrap();
    assert_eq!(ok(&mut s, "(cdr (assoc 2 (entget (tblobjname \"LAYER\" \"éBAUCHE\"))))"), "\"ébauche\"");
    assert_eq!(ok(&mut s, "(tblobjname \"NO_SUCH_TABLE\" \"0\")"), "nil");
    // A DIMSTYLE has its handle in group 105.
    ok(&mut s, "(setq ds (tblobjname \"DIMSTYLE\" (cdr (assoc 2 (tblnext \"DIMSTYLE\" T)))))");
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (entget ds)))"), "\"DIMSTYLE\"");
    assert_eq!(ok(&mut s, "(eq ds (handent (cdr (assoc 105 (entget ds)))))"), "T");
    assert!(eval(&mut s, "(tblobjname \"LAYER\")").is_err());
    // The name holds while the drawing changes.
    ok(&mut s, "(entmake '((0 . \"LINE\") (10 0.0 0.0 0.0) (11 10.0 0.0 0.0)))");
    assert_eq!(ok(&mut s, "(eq l (tblobjname \"LAYER\" \"0\"))"), "T");
    assert_eq!(ok(&mut s, "(eq l (handent (cdr (assoc 5 (entget l)))))"), "T");
}

/// #1661: the handles of a layer and a layout in a file lead to them once the file is open.
#[test]
fn handent_names_a_files_layer_and_layout() {
    let mut s = session();
    ok(&mut s, "(entmake '((0 . \"LINE\") (10 0.0 0.0 0.0) (11 10.0 0.0 0.0)))");
    let bytes = cadcraft_io::write(s.doc().unwrap(), "h.dxf").unwrap();
    let dxf = String::from_utf8(bytes.clone()).unwrap();
    let (layer, layout) = (dxf_handle(&dxf, "LAYER", "0"), dxf_handle(&dxf, "LAYOUT", "Layout1"));
    let line = dxf_handle(&dxf, "LINE", "");
    let dimstyle = dxf_handle(&dxf, "DIMSTYLE", "Standard");
    let mut s = session();
    s.open_drawing(cadcraft_io::read(&bytes, "h.dxf").unwrap(), "h.dxf", None);
    assert_eq!(ok(&mut s, &format!("(cdr (assoc 0 (entget (handent \"{line}\"))))")), "\"LINE\"");
    assert_eq!(ok(&mut s, &format!("(cdr (assoc 2 (entget (handent \"{layer}\"))))")), "\"0\"");
    assert_eq!(ok(&mut s, &format!("(eq (handent \"{layer}\") (tblobjname \"LAYER\" \"0\"))")), "T");
    assert_eq!(ok(&mut s, &format!("(cdr (assoc 0 (entget (handent \"{layout}\"))))")), "\"LAYOUT\"");
    assert_eq!(ok(&mut s, &format!("(cdr (assoc 5 (entget (handent \"{layout}\"))))")), format!("\"{layout}\""));
    // A DIMSTYLE's handle is in group 105, in the file and in its list.
    assert_eq!(ok(&mut s, &format!("(cdr (assoc 0 (entget (handent \"{dimstyle}\"))))")), "\"DIMSTYLE\"");
    assert_eq!(ok(&mut s, &format!("(cdr (assoc 105 (entget (handent \"{dimstyle}\"))))")), format!("\"{dimstyle}\""));
    assert_eq!(ok(&mut s, &format!("(eq (handent \"{dimstyle}\") (tblobjname \"DIMSTYLE\" \"Standard\"))")), "T");
    assert_eq!(ok(&mut s, "(handent \"FFFFF\")"), "nil");
    assert_eq!(ok(&mut s, "(handent \"not hex\")"), "nil");
}

/// A drawing with a dictionary two levels below the named object dictionary (kept from its file):
/// NOD › MY_DICT › INNER › LEAF (an XRECORD).
fn nested_dictionary_file() -> Vec<u8> {
    let s = session();
    let bytes = cadcraft_io::write(s.doc().unwrap(), "n.dxf").unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let mut pairs: Vec<(String, String)> = lines.chunks(2).map(|c| (c[0].trim().to_string(), c[1].to_string())).collect();
    let objects = pairs.iter().position(|p| p.0 == "2" && p.1 == "OBJECTS").unwrap();
    let nod = objects + pairs[objects..].iter().position(|p| p.0 == "0" && p.1 == "DICTIONARY").unwrap();
    let nod_handle = pairs[nod + 1].1.clone();
    let after = nod + pairs[nod..].iter().position(|p| p.0 == "281").unwrap() + 1;
    pairs.splice(after..after, [("3".into(), "MY_DICT".into()), ("350".into(), "AAA1".into())]);
    let end = objects + pairs[objects..].iter().position(|p| p.0 == "0" && p.1 == "ENDSEC").unwrap();
    let p = |c: &str, v: &str| (c.to_string(), v.to_string());
    let added = vec![
        p("0", "DICTIONARY"),
        p("5", "AAA1"),
        p("330", &nod_handle),
        p("100", "AcDbDictionary"),
        p("281", "1"),
        p("3", "INNER"),
        p("350", "AAA2"),
        p("0", "DICTIONARY"),
        p("5", "AAA2"),
        p("330", "AAA1"),
        p("100", "AcDbDictionary"),
        p("281", "1"),
        p("3", "LEAF"),
        p("350", "AAA3"),
        p("0", "XRECORD"),
        p("5", "AAA3"),
        p("330", "AAA2"),
        p("100", "AcDbXrecord"),
        p("280", "1"),
        p("1", "hello"),
    ];
    pairs.splice(end..end, added);
    pairs.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect::<String>().into_bytes()
}

/// An object found below a dictionary entry has no role: once the drawing changes, its old name
/// finds nothing rather than whatever now has its written handle.
#[test]
fn a_deeper_objects_name_lapses_when_the_drawing_changes() {
    let mut s = session();
    s.open_drawing(cadcraft_io::read(&nested_dictionary_file(), "n.dxf").unwrap(), "n.dxf", None);
    ok(&mut s, "(setq inner (cdr (assoc -1 (dictsearch (cdr (assoc -1 (dictsearch (namedobjdict) \"MY_DICT\"))) \"INNER\"))))");
    ok(&mut s, "(setq leaf (cdr (assoc -1 (dictsearch inner \"LEAF\"))))");
    assert_eq!(ok(&mut s, "(cdr (assoc 1 (entget leaf)))"), "\"hello\"");
    // Unchanged drawing: the same name.
    assert_eq!(ok(&mut s, "(eq leaf (cdr (assoc -1 (dictsearch inner \"LEAF\"))))"), "T");
    s.execute("layer.new", &serde_json::json!({"name": "Walls"})).unwrap();
    assert_eq!(ok(&mut s, "(entget leaf)"), "nil");
    // Looked up again, it is there (under a new name); the entry with a role kept its name.
    assert_eq!(ok(&mut s, "(cdr (assoc 1 (dictsearch inner \"LEAF\")))"), "\"hello\"");
}

// ---------------- entities of block definitions (#1700) ----------------

/// A drawing with one linear dimension measuring 100 whose `*D1` block, as stored in the file,
/// says 99.5 (a stale block: the defpoints moved, the block wasn't rebuilt).
fn stale_dimension() -> Session {
    let mut s = session();
    s.execute("dimlinear", &serde_json::json!({"p1": [0, 0], "p2": [100, 0], "at": [50, -20], "rotation": 0})).unwrap();
    let dxf = cadcraft_io::write_dxf(s.doc().unwrap());
    // The *D1 block's MTEXT says 100 (100.0000 here); make the file say 99.5.
    let lines: Vec<&str> = dxf.lines().collect();
    let (mut kind, mut block, mut out, mut changed) = (String::new(), String::new(), Vec::new(), 0);
    for (i, l) in lines.iter().enumerate() {
        let code = if i % 2 == 0 { l.trim() } else { "" };
        let value = lines.get(i + 1).map(|v| v.trim()).unwrap_or("");
        if code == "0" {
            kind = value.to_string();
        }
        if code == "2" && kind == "BLOCK" {
            block = value.to_string();
        }
        if i % 2 == 1 && kind == "MTEXT" && block == "*D1" && lines.get(i - 1).map(|c| c.trim()) == Some("1") && l.trim().starts_with("100") {
            out.push("99.5".to_string());
            changed += 1;
            continue;
        }
        out.push(l.to_string());
    }
    assert_eq!(changed, 1, "the fixture found the *D1 text");
    let mut s = session();
    s.open_drawing(cadcraft_io::read_dxf(out.join("\n").as_bytes()).unwrap(), "stale", None);
    s
}

/// The issue's use: a routine reads the text a dimension's block stores and compares it with the
/// measurement, so it finds the stale block.
#[test]
fn a_routine_reads_the_text_a_dimension_block_stores() {
    let mut s = stale_dimension();
    let code = r#"(progn
      (defun dtext (dim / e txt)
        (setq e (cdr (assoc -2 (tblsearch "BLOCK" (cdr (assoc 2 (entget dim)))))))
        (while e
          (if (= (cdr (assoc 0 (entget e))) "MTEXT") (setq txt (cdr (assoc 1 (entget e)))))
          (setq e (entnext e)))
        txt)
      (setq dim (entlast))
      (list (dtext dim) (cdr (assoc 42 (entget dim)))))"#;
    assert_eq!(ok(&mut s, code), "(\"99.5\" 100.0)");
}

/// tblsearch names a block's first entity (-2); entnext walks the definition in order and ends
/// with nil; entget lists each one (without a layout); handent finds them by handle.
#[test]
fn entnext_and_entget_walk_a_block_definition() {
    let mut s = session();
    let l = s.execute("line", &serde_json::json!({"points": [[0, 0], [10, 0]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    let c = s.execute("circle", &serde_json::json!({"center": [5, 5], "radius": 2})).unwrap()["handle"].as_str().unwrap().to_string();
    s.execute("block", &serde_json::json!({"name": "B", "base": [0, 0], "handles": [l, c], "keep": "delete"})).unwrap();
    s.execute("line", &serde_json::json!({"points": [[50, 0], [60, 0]]})).unwrap();
    let walk = r#"(progn (setq e (cdr (assoc -2 (tblsearch "BLOCK" "B"))) types nil)
      (while e (setq types (cons (cdr (assoc 0 (entget e))) types) e (entnext e)))
      (reverse types))"#;
    assert_eq!(ok(&mut s, walk), "(\"LINE\" \"CIRCLE\")", "the block's own entities, not the drawing's");
    let first = "(cdr (assoc -2 (tblsearch \"BLOCK\" \"B\")))";
    assert_eq!(ok(&mut s, &format!("(assoc 410 (entget {first}))")), "nil", "no layout");
    assert_eq!(ok(&mut s, &format!("(equal (handent (cdr (assoc 5 (entget {first})))) {first})")), "T");
    // The drawing's own entities walk as before.
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (entget (entnext))))"), "\"LINE\"");
    assert_eq!(ok(&mut s, "(entnext (entlast))"), "nil");
    // Editing a block's entity through entmod or entdel isn't offered: nil, nothing changes.
    assert_eq!(ok(&mut s, &format!("(entdel {first})")), "nil");
    assert_eq!(ok(&mut s, &format!("(entmod (entget {first}))")), "nil");
    assert_eq!(s.doc().unwrap().block("B").unwrap().entities.len(), 2);
}

#[test]
fn an_empty_block_has_no_first_entity() {
    let mut s = session();
    s.doc_mut().unwrap().blocks.insert("EMPTY".into(), std::sync::Arc::new(cadcraft_doc::Block::new("EMPTY")));
    assert_eq!(ok(&mut s, "(assoc -2 (tblsearch \"BLOCK\" \"EMPTY\"))"), "nil");
}

#[test]
fn angtof_reads_the_string_in_the_given_units() {
    let mut s = Session::new();
    assert_eq!(ok(&mut s, "(angtof \"1.5\" 3)"), "1.5");
    assert_eq!(ok(&mut s, "(angtof \"180\" 0)"), "3.14159");
    assert_eq!(ok(&mut s, "(angtof \"180\" 7)"), "nil");
    assert_eq!(ok(&mut s, "(angtof \"180\" \"x\")"), "nil");
}

/// #1920: the value of an expression typed at a prompt is the answer, not another expression.
/// `(setq q "!q")` returned `"!q"`, which was evaluated again as `!q`, forever, until the stack
/// overflowed and the process aborted (no `catch_unwind` catches that).
#[test]
fn a_lisp_value_at_a_prompt_is_not_evaluated_again() {
    let mut s = session();
    s.cmdline("circle (setq q \"!q\")").unwrap();
    assert_eq!(ok(&mut s, "q"), "\"!q\"");
    // "!q" is no centre point, so CIRCLE still asks for one; the session carries on.
    assert!(s.running.is_some());
    s.cancel();
    // A value that is itself an expression stays text too: `(+ 1 2)` is not evaluated to 3.
    s.cmdline("circle 0,0 (setq r \"(+ 1 2)\")").unwrap();
    assert!(s.running.is_some(), "\"(+ 1 2)\" is no radius");
    s.cancel();
    // Values that answer the prompt still do.
    s.cmdline("(setq r \"4\")").unwrap();
    s.cmdline("circle 0,0 !r").unwrap();
    let r = s.doc().unwrap().model.iter().find_map(|e| match &e.kind {
        cadcraft_doc::EntityKind::Circle(c) => Some(c.radius),
        _ => None,
    });
    assert_eq!(r, Some(4.0));
}

#[test]
fn atof_out_of_range_exponent_reads_whole_token() {
    let mut s = Session::new();
    // Overflow must not fall back to a shorter prefix such as "1e40".
    assert_eq!(ok(&mut s, "(> (atof \"1e400\") 1e300)"), "T");
    assert_eq!(ok(&mut s, "(< (atof \"-1e400\") -1e300)"), "T");
    assert_eq!(ok(&mut s, "(= (atof \"1e-400\") 0.0)"), "T");
}

/// #1927: with a support folder, a relative `load` name is found there, never through the
/// process working folder; an absolute path is still read as given.
#[cfg(unix)]
#[test]
fn relative_load_uses_the_support_folder_not_the_working_folder() {
    let base = std::env::temp_dir().join(format!("cadcraft-lisp-loadpath-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (support, other) = (base.join("support"), base.join("other"));
    std::fs::create_dir_all(&support).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(support.join("tools.lsp"), "(setq which \"support\")").unwrap();
    std::fs::write(other.join("tools.lsp"), "(setq which \"other\")").unwrap();
    std::fs::write(other.join("only.lsp"), "(setq only 1)").unwrap();
    // `other/…` as a path relative to the working folder.
    let cwd = std::env::current_dir().unwrap();
    let up = "../".repeat(cwd.components().count().saturating_sub(1));
    let rel = |f: &str| format!("{up}{}", other.join(f).to_string_lossy().trim_start_matches('/'));

    let mut s = session();
    s.set_lisp_support_dir(Some(support.clone()));
    assert_eq!(ok(&mut s, "(load \"tools\") which"), "\"support\"");
    let r = eval(&mut s, &format!("(load \"{}\") only", rel("only.lsp")));
    assert!(!matches!(r.as_deref(), Ok("1")), "not read through the working folder: {r:?}");
    assert_eq!(ok(&mut s, &format!("(load \"{}\") which", other.join("tools.lsp").to_string_lossy())), "\"other\"");
    let _ = std::fs::remove_dir_all(&base);
}

/// #1927 on Windows: a rooted name (`\folder\tools.lsp`, no drive) or a drive-relative one
/// (`C:tools`) is not absolute, but joined to the support folder it replaces that folder; such a
/// name is refused rather than read from outside the support folder.
#[cfg(windows)]
#[test]
fn rooted_and_drive_relative_load_names_stay_in_the_support_folder() {
    let base = std::env::temp_dir().join(format!("cadcraft-lisp-rooted-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (support, other) = (base.join("support"), base.join("other"));
    std::fs::create_dir_all(&support).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let outside = other.join("only.lsp");
    std::fs::write(&outside, "(setq only 1)").unwrap();
    // The same file without its drive (`\Users\...\other\only.lsp`), escaped for a LISP string.
    let full = outside.to_string_lossy().into_owned();
    let rooted = full.get(full.find('\\').unwrap()..).unwrap().replace('\\', "\\\\");
    let drive = full.get(..2).unwrap().to_string();

    let mut s = session();
    s.set_lisp_support_dir(Some(support.clone()));
    let r = eval(&mut s, &format!("(load \"{rooted}\") only"));
    assert!(!matches!(r.as_deref(), Ok("1")), "a rooted name is not read outside the support folder: {r:?}");
    let r = eval(&mut s, &format!("(load \"{drive}only\") only"));
    assert!(!matches!(r.as_deref(), Ok("1")), "a drive-relative name is refused: {r:?}");
    std::fs::write(support.join("only.lsp"), "(setq only 2)").unwrap();
    assert_eq!(ok(&mut s, "(load \".\\\\only\") only"), "2", "a plain relative name is still read from the support folder");
    let _ = std::fs::remove_dir_all(&base);
}

/// #1930: `append` over many references to one long list is refused before it builds the
/// result, not after it has filled memory.
#[test]
fn append_checks_the_cell_cap_before_building_its_result() {
    let _heavy = super::value::HEAVY_CELLS.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = session();
    s.lisp.limits.cells = 50_000;
    let before = super::value::live_cells();
    let e =
        eval(&mut s, "(setq l nil) (repeat 10000 (setq l (cons 1 l))) (setq m nil) (repeat 1000 (setq m (cons l m))) (apply 'append m)").unwrap_err();
    assert!(e.contains("too many list cells"), "{e}");
    assert!(super::value::live_cells() < before + 50_000, "no oversized result was built");
    assert_eq!(ok(&mut s, "(setq l nil m nil) (length (append '(1) '(2 3)))"), "3");
}

// ---------------- an INSERT's ATTRIBs and SEQEND (#1660) ----------------

/// A session with block K: a circle and attributes (tag, DXF flags, default value). `entmake`
/// goes through the DXF codec, as in the app.
fn with_attributes(defs: &[(&str, i32, &str)]) -> Session {
    let mut s = session();
    let mut code =
        String::from("(entmake '((0 . \"BLOCK\") (2 . \"K\") (70 . 2) (10 0.0 0.0 0.0))) (entmake '((0 . \"CIRCLE\") (10 0.0 0.0 0.0) (40 . 3.0)))");
    for (i, (tag, flags, default)) in defs.iter().enumerate() {
        let y = 5.0 + 3.0 * i as f64;
        code += &format!(
            " (entmake '((0 . \"ATTDEF\") (10 0.0 {y:.1} 0.0) (40 . 2.5) (1 . \"{default}\") (3 . \"P{tag}\") (2 . \"{tag}\") (70 . {flags})))"
        );
    }
    code += " (entmake '((0 . \"ENDBLK\")))";
    s.cmdline(&format!("(progn {code})")).unwrap();
    s.log.clear();
    s
}

/// `s` with one reference of K at (10,10), its values `attribs`, and the reference's handle; the
/// LISP global `i` names it.
fn inserted(s: &mut Session, attribs: serde_json::Value) -> Handle {
    s.execute("insert", &json!({ "name": "K", "at": [10, 10], "attribs": attribs })).unwrap();
    let h = *s.doc().unwrap().model.handles().last().unwrap();
    ok(s, &format!("(setq i (handent \"{}\")) i", h.hex()));
    h
}

/// Every reference's attributes (tag, value) from the model, in order.
fn attribute_values(s: &Session) -> Vec<(String, String)> {
    let d = s.doc().unwrap();
    d.model
        .iter()
        .filter_map(|e| match &e.kind {
            cadcraft_doc::EntityKind::Insert(i) => Some(i.attribs.iter().map(|a| (a.tag.clone(), a.text.value.clone())).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// The records from `i` in order: (type, tag, value) and the SEQEND's owner.
const WALK: &str = r#"(progn
  (setq e (entnext i) out nil)
  (while (and e (= "ATTRIB" (cdr (assoc 0 (setq ed (entget e))))))
    (setq out (append out (list (list (cdr (assoc 0 ed)) (cdr (assoc 2 ed)) (cdr (assoc 1 ed))
                                     (if (equal (cdr (assoc -2 ed)) i) T nil))))
          e (entnext e)))
  (list out (cdr (assoc 0 ed)) (equal (cdr (assoc -2 ed)) i)))"#;

#[test]
fn entnext_walks_an_inserts_attribs_then_the_seqend_then_the_next_entity() {
    let mut s = with_attributes(&[("T1", 0, "D1"), ("T2", 8, "P2"), ("T3", 0, "D3")]);
    inserted(&mut s, json!({ "T1": "A1", "T3": "A3" }));
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (entget i)))"), "\"INSERT\"");
    assert_eq!(ok(&mut s, "(cdr (assoc 66 (entget i)))"), "1");
    assert_eq!(ok(&mut s, WALK), "(((\"ATTRIB\" \"T1\" \"A1\" T) (\"ATTRIB\" \"T2\" \"P2\" T) (\"ATTRIB\" \"T3\" \"A3\" T)) \"SEQEND\" T)");
    // After the SEQEND the main order goes on: a line made later is next, then nil.
    s.execute("line", &json!({ "points": [[0, -10], [10, -10]] })).unwrap();
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (entget (entnext (entnext (entnext (entnext (entnext i))))))))"), "\"LINE\"");
    assert_eq!(ok(&mut s, "(entnext (entlast))"), "nil");
    // A reference of a block without attributes is followed by the next main entity, no SEQEND.
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    s.cmdline("(progn (entmake '((0 . \"BLOCK\") (2 . \"E\") (70 . 2) (10 0.0 0.0 0.0))) (entmake '((0 . \"LINE\") (10 0.0 0.0 0.0) (11 1.0 0.0 0.0))) (entmake '((0 . \"ENDBLK\"))))").unwrap();
    s.execute("insert", &json!({ "name": "E", "at": [0, 0] })).unwrap();
    let plain = *s.doc().unwrap().model.handles().last().unwrap();
    s.execute("line", &json!({ "points": [[0, -20], [10, -20]] })).unwrap();
    assert_eq!(ok(&mut s, &format!("(cdr (assoc 0 (entget (entnext (handent \"{}\")))))", plain.hex())), "\"LINE\"");
    // The owner is group -2 in a record's list and not 330, and the reference's own list has
    // neither (as `entity_groups` keeps it).
    inserted(&mut s, json!({ "T1": "A1" }));
    assert_eq!(ok(&mut s, "(assoc 330 (entget (entnext i)))"), "nil");
    assert!(attribute_values(&s).iter().any(|(t, v)| t == "T1" && v == "A1"));
}

#[test]
fn entmod_of_an_attrib_changes_only_its_value() {
    let mut s = with_attributes(&[("T1", 0, "D1"), ("T2", 8, "P2")]);
    inserted(&mut s, json!({ "T1": "A1" }));
    ok(&mut s, "(setq a (entnext i) d (entget a) seq (entnext (entnext a)))");
    // The value (group 1) is the one edit an ATTRIB's list takes.
    let r = ok(&mut s, "(entmod (subst (cons 1 \"A2\") (assoc 1 d) d))");
    assert!(r.contains("(1 . \"A2\")"), "{r}");
    assert_eq!(ok(&mut s, "(cdr (assoc 1 (entget a)))"), "\"A2\"");
    assert_eq!(ok(&mut s, "(cdr (assoc 2 (entget a)))"), "\"T1\"");
    assert_eq!(attribute_values(&s), [("T1".into(), "A2".into()), ("T2".into(), "P2".into())]);
    // A list asking for anything else, and any edit of the SEQEND, is refused and changes nothing.
    for code in [
        "(entmod (subst (cons 2 \"X\") (assoc 2 d) d))",
        "(entmod (subst (cons 40 9.0) (assoc 40 d) d))",
        "(entmod (subst (cons 8 \"L\") (assoc 8 d) d))",
        "(entmod (subst (cons 0 \"SEQEND\") (assoc 0 d) d))",
        "(entmod (entget seq))",
        // Groups `list_to_groups` does not keep are still the reference's: an edit (or an added
        // group) is refused, also when the same list changes group 1.
        "(entmod (subst (cons 410 \"Layout2\") (assoc 410 d) d))",
        "(entmod (subst (cons -2 a) (assoc -2 d) d))",
        "(entmod (append d (list (cons 5 \"FF\"))))",
        "(entmod (append d (list (cons 330 a))))",
        "(entmod (append d (list (cons 360 \"FF\"))))",
        "(entmod (append d (list (cons 102 \"{X\"))))",
        "(entmod (subst (cons 1 \"A2\") (assoc 1 d) (subst (cons 410 \"Layout2\") (assoc 410 d) d)))",
        // The same items, but the outer list ends in an atom: iterating it would drop the tail
        // and accept a list `entget` never returned.
        "(progn (setq r \"X\" p (reverse d)) (while p (setq r (cons (car p) r) p (cdr p))) (entmod r))",
    ] {
        assert_eq!(ok(&mut s, code), "nil", "{code}");
    }
    assert_eq!(ok(&mut s, "(cdr (assoc 1 (entget a)))"), "\"A2\"", "no refused edit changed the value");
    // `entdel` of a record, and `entdel` of the SEQEND, do nothing either.
    assert_eq!(ok(&mut s, "(progn (entdel a) (entdel seq))"), "nil");
    assert_eq!(attribute_values(&s), [("T1".into(), "A2".into()), ("T2".into(), "P2".into())]);
    assert_eq!(ok(&mut s, "(cdr (assoc 1 (entget a)))"), "\"A2\"", "the name still holds: nothing changed");
    // The edit is one undo step: the value goes back with it.
    s.undo().unwrap();
    assert_eq!(attribute_values(&s), [("T1".into(), "A1".into()), ("T2".into(), "P2".into())]);
}

#[test]
fn a_subentity_name_expires_when_its_record_is_rebuilt() {
    let mut s = with_attributes(&[("T1", 0, "D1"), ("T2", 8, "P2")]);
    let h = inserted(&mut s, json!({ "T1": "A1" }));
    ok(&mut s, "(setq a (entnext i))");
    // Erased: the records go with the reference.
    s.execute("erase", &json!({ "handles": [h.hex()] })).unwrap();
    assert_eq!(ok(&mut s, "(list (entget a) (entnext a))"), "(nil nil)");
    // Undone: the reference is back, the name given before the Undo is not.
    s.undo().unwrap();
    assert_eq!(ok(&mut s, "(entget a)"), "nil");
    assert_eq!(ok(&mut s, &format!("(cdr (assoc 1 (entget (entnext (handent \"{}\")))))", h.hex())), "\"A1\"");
    // ATTSYNC rebuilds the attributes in place: the old name names nothing, the values stay.
    ok(&mut s, "(setq b (entnext i))");
    s.execute("attsync", &json!({ "name": "K" })).unwrap();
    assert_eq!(ok(&mut s, "(entget b)"), "nil");
    assert_eq!(attribute_values(&s), [("T1".into(), "A1".into()), ("T2".into(), "P2".into())]);
    assert_eq!(ok(&mut s, "(cdr (assoc 1 (entget (entnext i))))"), "\"A1\"");
    // A name never resolves to another attribute: when the record at its place has another tag
    // now, it names nothing (the change makes no new epoch), and a new walk reaches the new tag.
    ok(&mut s, "(setq c (entnext i))");
    assert_eq!(ok(&mut s, "(cdr (assoc 2 (entget c)))"), "\"T1\"");
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let cadcraft_doc::EntityKind::Insert(i) = &mut e.kind
                && let Some(a) = i.attribs.first_mut()
            {
                a.tag = "T9".into();
            }
        })
        .unwrap();
    assert_eq!(ok(&mut s, "(entget c)"), "nil", "the record at that place has another tag now");
    assert_eq!(ok(&mut s, "(list (cdr (assoc 0 (entget (entnext i)))) (cdr (assoc 2 (entget (entnext i)))))"), "(\"ATTRIB\" \"T9\")");
}

#[test]
fn an_empty_attrib_and_a_seqend_have_distinct_names() {
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    let h = inserted(&mut s, json!({ "T1": "A1" }));
    ok(&mut s, "(setq a (entnext i) end (entnext a))");
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (entget end)))"), "\"SEQEND\"");
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let cadcraft_doc::EntityKind::Insert(i) = &mut e.kind {
                let mut blank = i.attribs[0].clone();
                blank.tag.clear();
                i.attribs.push(blank);
            }
        })
        .unwrap();
    assert_eq!(ok(&mut s, "(entget end)"), "nil");
    assert_eq!(ok(&mut s, "(setq b (entnext a)) (list (cdr (assoc 0 (entget b))) (eq b (entnext a)))"), "(\"ATTRIB\" T)");
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let cadcraft_doc::EntityKind::Insert(i) = &mut e.kind {
                i.attribs.truncate(1);
            }
        })
        .unwrap();
    assert_eq!(ok(&mut s, "(list (entget b) (cdr (assoc 0 (entget (entnext a)))))"), "(nil \"SEQEND\")");
}

#[test]
fn explode_and_burst_retire_the_removed_inserts_record_names() {
    for command in ["explode", "burst"] {
        let mut s = with_attributes(&[("T1", 0, "D1"), ("T2", 0, "D2")]);
        let h = inserted(&mut s, json!({ "T1": "A1" }));
        ok(&mut s, "(setq a (entnext i) end (entnext (entnext a)))");
        let original = s.doc().unwrap().entity(h).unwrap().as_ref().clone();
        inserted(&mut s, json!({ "T1": "B1" }));
        ok(&mut s, "(setq b (entnext i))");
        // A host can shorten the run before removal and later reuse the handle.
        s.doc_mut()
            .unwrap()
            .modify_entity(h, |e| {
                if let cadcraft_doc::EntityKind::Insert(i) = &mut e.kind {
                    i.attribs.truncate(1);
                }
            })
            .unwrap();
        let result = s.execute(command, &json!({ "handles": [h.hex()] })).unwrap();
        assert!(!result["handles"].as_array().unwrap().is_empty(), "{command} creates pieces");
        assert!(s.doc().unwrap().entity(h).is_none(), "{command} removes the reference");
        s.doc_mut().unwrap().model.push(original);
        assert_eq!(ok(&mut s, "(list (entget a) (entget end) (cdr (assoc 1 (entget b))))"), "(nil nil \"B1\")", "{command}");
    }
}

#[test]
fn entmod_obeys_the_refedit_working_set_for_attrib_owners() {
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    let editing = inserted(&mut s, json!({ "T1": "E1" }));
    let outside = inserted(&mut s, json!({ "T1": "A1" }));
    s.execute("refedit", &json!({ "handle": editing.hex() })).unwrap();
    let read = "(cdr (assoc 1 (entget (entnext i))))";
    let edit = r#"(progn (setq d (entget (entnext i))) (entmod (subst (cons 1 "A2") (assoc 1 d) d)))"#;
    assert_eq!(ok(&mut s, read), "\"A1\"");
    assert_eq!(ok(&mut s, edit), "nil");
    assert_eq!(ok(&mut s, read), "\"A1\"");
    s.execute("refset", &json!({ "add": [outside.hex()] })).unwrap();
    assert_ne!(ok(&mut s, edit), "nil");
    assert_eq!(ok(&mut s, read), "\"A2\"");
}

#[test]
fn a_restored_reference_does_not_revive_an_old_name() {
    // entdel of the reference, then entdel again to bring it back: the name given before the
    // first entdel names nothing, also after the reference returns.
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    inserted(&mut s, json!({ "T1": "A1" }));
    ok(&mut s, "(setq a (entnext i))");
    assert_eq!(ok(&mut s, "(progn (entdel i) (entget a))"), "nil");
    assert_eq!(ok(&mut s, "(progn (entdel i) (entget a))"), "nil", "the name must not come back");
    assert_eq!(ok(&mut s, "(cdr (assoc 1 (entget (entnext i))))"), "\"A1\"");
    // ERASE, then OOPS: the same. Erasing expires the names it took with it.
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    let h = inserted(&mut s, json!({ "T1": "A1" }));
    ok(&mut s, "(setq a (entnext i))");
    s.execute("erase", &json!({ "handles": [h.hex()] })).unwrap();
    assert_eq!(ok(&mut s, "(entget a)"), "nil");
    s.execute("oops", &json!({})).unwrap();
    assert_eq!(ok(&mut s, "(entget a)"), "nil", "OOPS must not revive the name");
    assert_eq!(ok(&mut s, "(cdr (assoc 1 (entget (entnext i))))"), "\"A1\"");
}

#[test]
fn erasing_another_object_keeps_an_attrib_name() {
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    let h = inserted(&mut s, json!({ "T1": "A1" }));
    ok(&mut s, "(setq a (entnext i) d (entget a))");
    s.execute("line", &json!({ "points": [[0, -10], [10, -10]] })).unwrap();
    let line = *s.doc().unwrap().model.handles().last().unwrap();
    // Erasing another object leaves the reference and its records as they were: the name holds
    // and takes the value edit it always took.
    s.execute("erase", &json!({ "handles": [line.hex()] })).unwrap();
    assert_eq!(ok(&mut s, "(cdr (assoc 2 (entget a)))"), "\"T1\"");
    let r = ok(&mut s, "(entmod (subst (cons 1 \"A2\") (assoc 1 d) d))");
    assert!(r.contains("(1 . \"A2\")"), "{r}");
    assert_eq!(attribute_values(&s), [("T1".into(), "A2".into())]);
    // Erasing the reference itself ends its names, and OOPS bringing it back revives none.
    s.execute("erase", &json!({ "handles": [h.hex()] })).unwrap();
    assert_eq!(ok(&mut s, "(entget a)"), "nil");
    s.execute("oops", &json!({})).unwrap();
    assert_eq!(ok(&mut s, "(entget a)"), "nil", "the name of an erased reference stays dead");
    assert_eq!(ok(&mut s, "(cdr (assoc 1 (entget (entnext i))))"), "\"A2\"");
}

#[test]
fn entdel_of_another_object_keeps_an_attrib_name() {
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    inserted(&mut s, json!({ "T1": "A1" }));
    ok(&mut s, "(setq a (entnext i))");
    s.execute("line", &json!({ "points": [[0, -10], [10, -10]] })).unwrap();
    ok(&mut s, "(setq line (entlast))");
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (entget line)))"), "\"LINE\"");
    ok(&mut s, "(entdel line)");
    assert_eq!(ok(&mut s, "(entget line)"), "nil");
    assert_eq!(ok(&mut s, "(cdr (assoc 1 (entget a)))"), "\"A1\"");
}

#[test]
fn a_name_given_in_the_block_editor_never_resolves_in_the_host() {
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    inserted(&mut s, json!({ "T1": "A1" }));
    s.execute("bedit", &json!({ "name": "K" })).unwrap();
    // A reference made in the editor's drawing, named there. The editor starts from the host's
    // drawing, so its handle is the one the host gives its next new object.
    s.execute("insert", &json!({ "name": "K", "at": [40, 40], "attribs": { "T1": "E1" } })).unwrap();
    let editor = *s.doc().unwrap().model.handles().last().unwrap();
    ok(&mut s, &format!("(setq a (entnext (handent \"{}\")))", editor.hex()));
    assert_eq!(ok(&mut s, "(cdr (assoc 1 (entget a)))"), "\"E1\"");
    s.execute("bclose", &json!({ "save": false })).unwrap();
    // The host's next reference takes the handle the editor's reference had: the name given in
    // the editor must not reach its ATTRIB, or `entmod` through it would edit the wrong one.
    s.execute("insert", &json!({ "name": "K", "at": [80, 80], "attribs": { "T1": "H1" } })).unwrap();
    let host = *s.doc().unwrap().model.handles().last().unwrap();
    assert_eq!(host, editor, "the host takes the handle the editor's reference had");
    assert_eq!(ok(&mut s, "(entget a)"), "nil");
    assert_eq!(ok(&mut s, "(entdel a)"), "nil");
    assert_eq!(
        ok(&mut s, &format!("(cdr (assoc 1 (entget (entnext (handent \"{}\")))))", host.hex())),
        "\"H1\"",
        "the host's own attribute is untouched"
    );
}

#[test]
fn without_a_record_provider_entnext_walks_the_next_entity() {
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    inserted(&mut s, json!({ "T1": "A1" }));
    s.execute("line", &json!({ "points": [[0, -20], [10, -20]] })).unwrap();
    // A host that installed no provider has no records to name: the walk goes on to the next
    // main entity as it always did, rather than answering nil at the reference.
    let next = crate::cmd::file::without_entity_sub_record(|| ok(&mut s, "(entnext i)"));
    assert_ne!(next, "nil");
    assert_eq!(crate::cmd::file::without_entity_sub_record(|| ok(&mut s, "(cdr (assoc 0 (entget (entnext i))))")), "\"LINE\"");
}

#[test]
fn a_redo_does_not_revive_an_old_name() {
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    inserted(&mut s, json!({ "T1": "A1" }));
    ok(&mut s, "(setq a (entnext i) d (entget a))");
    ok(&mut s, "(entmod (subst (cons 1 \"A2\") (assoc 1 d) d))");
    s.undo().unwrap();
    assert_eq!(ok(&mut s, "(entget a)"), "nil");
    s.redo().unwrap();
    assert_eq!(ok(&mut s, "(entget a)"), "nil", "Redo must not revive the name");
    assert_eq!(ok(&mut s, "(cdr (assoc 1 (entget (entnext i))))"), "\"A2\"");
}

#[test]
fn a_block_replace_expires_an_old_attrib_name() {
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    // A replacement block with the same tag as K's first attribute: without the expiry, the old
    // name would keep naming that place and return the new record.
    s.cmdline(
        "(progn (entmake '((0 . \"BLOCK\") (2 . \"L\") (70 . 2) (10 0.0 0.0 0.0))) \
         (entmake '((0 . \"ATTDEF\") (10 0.0 0.0 0.0) (40 . 2.5) (1 . \"Z1\") (3 . \"P\") (2 . \"T1\") (70 . 0))) \
         (entmake '((0 . \"ENDBLK\"))))",
    )
    .unwrap();
    inserted(&mut s, json!({ "T1": "A1" }));
    ok(&mut s, "(setq a (entnext i))");
    s.execute("blockreplace", &json!({ "block": "K", "with": "L" })).unwrap();
    assert_eq!(ok(&mut s, "(entget a)"), "nil", "-BLOCKREPLACE rebuilt the record");
    assert_eq!(ok(&mut s, "(cdr (assoc 1 (entget (entnext i))))"), "\"A1\"");
}

#[test]
fn a_reinsert_after_undo_takes_the_handle_and_no_old_name() {
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    let h = inserted(&mut s, json!({ "T1": "A1" }));
    ok(&mut s, "(setq a (entnext i))");
    // U takes the reference back; INSERT then gives a new one the handle it had.
    s.undo().unwrap();
    assert!(s.doc().unwrap().entity(h).is_none());
    let again = inserted(&mut s, json!({ "T1": "B1" }));
    assert_eq!(again, h, "the same handle");
    assert_eq!(ok(&mut s, "(list (entget a) (cdr (assoc 1 (entget (entnext i)))))"), "(nil \"B1\")");
}

/// A drawing with one reference of `attribs` attributes (all value V), for the bounds.
fn drawing_with_attributes(attribs: usize) -> (Session, Handle) {
    let text = |v: &str| cadcraft_doc::Text {
        insert: cadcraft_geom::Vec3::ZERO,
        align_pt: None,
        height: 1.0,
        value: v.into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: cadcraft_doc::HAlign::Left,
        valign: cadcraft_doc::VAlign::Baseline,
    };
    let mut d = cadcraft_doc::Drawing::new_metric();
    let list = (0..attribs)
        .map(|i| cadcraft_doc::Attrib {
            tag: format!("T{i}"),
            text: text("V"),
            invisible: false,
            constant: false,
            prompt: String::new(),
            props: cadcraft_doc::AttribProps::default(),
            multiline: None,
            modes: cadcraft_doc::AttribModes::default(),
        })
        .collect();
    let insert = cadcraft_doc::Insert { block: "B".into(), attribs: list, ..cadcraft_doc::Insert::default() };
    let h = d.add(&cadcraft_doc::Space::Model, cadcraft_doc::Common::default(), cadcraft_doc::EntityKind::Insert(insert)).expect("add");
    let mut s = session();
    s.open_drawing(d, "attributes", None);
    ok(&mut s, &format!("(setq i (handent \"{}\"))", h.hex()));
    (s, h)
}

/// The number of records a walk from `i` names.
const COUNT: &str = "(progn (setq e (entnext i) n 0) (while e (setq n (1+ n) e (entnext e))) n)";

#[test]
fn a_session_names_at_most_65536_records_and_nothing_changes() {
    let (mut s, h) = drawing_with_attributes(65_537);
    assert_eq!(ok(&mut s, COUNT), "65536");
    // The names given hold: a second walk is the same, and the drawing kept every attribute.
    assert_eq!(ok(&mut s, COUNT), "65536");
    // Erasing and bringing the reference back frees the 65,536 names it held: the old name names
    // nothing, and a new walk can name the records again.
    ok(&mut s, "(setq a (entnext i))");
    ok(&mut s, "(progn (entdel i) (entdel i))");
    assert_eq!(ok(&mut s, "(entget a)"), "nil");
    assert_eq!(ok(&mut s, COUNT), "65536");
    let d = s.doc().unwrap();
    let kept = d.entity(h).and_then(|e| match &e.kind {
        cadcraft_doc::EntityKind::Insert(i) => Some(i.attribs.len()),
        _ => None,
    });
    assert_eq!(kept, Some(65_537));
}

#[test]
fn a_new_object_at_any_handle_is_never_a_record() {
    // A hostile $HANDSEED at the top of the range: the line made after the walk takes that
    // handle, and the record keeps its own name, which is no handle.
    let (mut s, _) = drawing_with_attributes(1);
    s.doc_mut().unwrap().handseed = u64::MAX - 1;
    ok(&mut s, "(setq a (entnext i))");
    assert_eq!(
        ok(&mut s, r#"(progn (entmake '((0 . "LINE") (10 0.0 0.0 0.0) (11 1.0 0.0 0.0))) (cdr (assoc 5 (entget (entlast)))))"#),
        "\"FFFFFFFFFFFFFFFE\""
    );
    assert_eq!(
        ok(&mut s, "(list (cdr (assoc 0 (entget a))) (eq a (entlast)) (entdel a) (cdr (assoc 0 (entget (entlast)))))"),
        "(\"ATTRIB\" nil nil \"LINE\")"
    );
}

#[test]
fn objects_at_the_top_of_the_handle_range_walk_as_objects() {
    let (mut s, _) = drawing_with_attributes(1);
    {
        let d = s.doc_mut().unwrap();
        d.handseed = u64::MAX;
        for handle in [Handle(u64::MAX - 1), Handle(u64::MAX)] {
            let line =
                cadcraft_doc::EntityKind::Line(cadcraft_doc::Line { a: cadcraft_geom::Vec3::ZERO, b: cadcraft_geom::Vec3::new(1.0, 0.0, 0.0) });
            d.model.push(cadcraft_doc::Entity { handle, common: cadcraft_doc::Common::default(), kind: line });
        }
    }
    ok(&mut s, "(setq a (entnext i))");
    // The lines' handles are theirs: `entnext` of the first finds the second, then the end.
    assert_eq!(ok(&mut s, "(equal (entnext (handent \"FFFFFFFFFFFFFFFE\")) (handent \"FFFFFFFFFFFFFFFF\"))"), "T");
    assert_eq!(ok(&mut s, "(entnext (handent \"FFFFFFFFFFFFFFFF\"))"), "nil");
    // The record, a table record and the lines each keep their own name.
    assert_eq!(
        ok(&mut s, "(list (cdr (assoc 0 (entget a))) (equal a (tblobjname \"LAYER\" \"0\")) (equal a (handent \"FFFFFFFFFFFFFFFE\")))"),
        "(\"ATTRIB\" nil nil)"
    );
}

#[test]
fn a_record_name_is_an_entity_name_but_no_handle() {
    let mut s = with_attributes(&[("T1", 0, "D1"), ("T2", 0, "D2")]);
    let h = inserted(&mut s, json!({ "T1": "A1" }));
    ok(&mut s, "(setq a (entnext i) b (entnext a))");
    assert_eq!(ok(&mut s, "(type a)"), "ENAME");
    // The same record has the same name on every walk; another record and the reference have
    // their own.
    assert_eq!(ok(&mut s, "(list (eq a (entnext i)) (equal a (entnext i)) (eq a b) (eq a i) (equal a i))"), "(T T nil nil nil)");
    // It prints as the reference's handle, a dot and its number: never a handle's text.
    let printed = ok(&mut s, "a");
    assert!(printed.starts_with(&format!("<Entity name: {:x}.", h.0)) && printed.ends_with('>'), "{printed}");
    // No handle reaches it: its list has no group 5, and `handent` of the reference's handle is
    // the reference.
    assert_eq!(ok(&mut s, "(list (assoc 5 (entget a)) (eq a (handent (cdr (assoc 5 (entget i))))))"), "(nil nil)");
    // A DXF group cannot point at it: `entmake` of a list that does is refused.
    assert_eq!(ok(&mut s, "(entmake (list '(0 . \"LINE\") '(10 0.0 0.0 0.0) '(11 1.0 0.0 0.0) (cons 340 a)))"), "nil");
}

#[test]
fn selection_sets_and_commands_take_no_record() {
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    let h = inserted(&mut s, json!({ "T1": "A1" }));
    ok(&mut s, "(setq a (entnext i) ss (ssadd i))");
    // A selection set holds objects of the drawing; a record is not one on its own.
    assert_eq!(ok(&mut s, "(list (ssadd a) (ssadd a ss) (sslength ss) (ssmemb a ss) (ssdel a ss) (sslength ss))"), "(nil nil 1 nil nil 1)");
    // A command given the name picks nothing: the reference stays, and the name holds.
    ok(&mut s, "(command \"_.ERASE\" a \"\")");
    assert!(s.doc().unwrap().entity(h).is_some());
    assert_eq!(ok(&mut s, "(cdr (assoc 1 (entget a)))"), "\"A1\"");
}

#[test]
fn entupd_of_a_record_answers_its_name_while_it_holds() {
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    let h = inserted(&mut s, json!({ "T1": "A1" }));
    ok(&mut s, "(setq a (entnext i) d (entget a))");
    assert_eq!(ok(&mut s, "(progn (entmod (subst (cons 1 \"A2\") (assoc 1 d) d)) (eq (entupd a) a))"), "T");
    s.execute("erase", &json!({ "handles": [h.hex()] })).unwrap();
    assert_eq!(ok(&mut s, "(entupd a)"), "nil");
}

#[test]
fn a_record_name_holds_while_commands_keep_its_record() {
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    let h = inserted(&mut s, json!({ "T1": "A1" }));
    ok(&mut s, "(setq a (entnext i))");
    // Moving the reference, or changing its colour, keeps its records in their places.
    s.execute("move", &json!({ "handles": [h.hex()], "delta": [5, 5] })).unwrap();
    s.execute("chprop", &json!({ "handles": [h.hex()], "color": 1 })).unwrap();
    assert_eq!(ok(&mut s, "(list (cdr (assoc 1 (entget a))) (eq a (entnext i)))"), "(\"A1\" T)");
}

#[test]
fn each_record_name_is_one_entry_and_walking_again_adds_none() {
    const N: usize = 64;
    let (mut s, _) = drawing_with_attributes(N);
    let before = s.state().unwrap().doc.clone();
    ok(&mut s, "(setq e nil)");
    // The R3 pattern: each new name, then state access. Nothing is copied or replayed.
    for _ in 0..=N {
        ok(&mut s, "(setq e (if e (entnext e) (entnext i)))");
        let _ = s.state_mut().unwrap();
    }
    let uid = s.state().unwrap().uid;
    let held = |s: &Session| s.lisp.records.get(&uid).map_or(0, super::drawing::RecordNames::count);
    assert_eq!(held(&s), N + 1, "the attributes and the SEQEND, one entry each");
    assert_eq!(ok(&mut s, COUNT), (N + 1).to_string());
    assert_eq!(held(&s), N + 1, "a walk again gives the same names");
    assert!(std::sync::Arc::ptr_eq(&before, &s.state().unwrap().doc), "naming changes nothing in the drawing");
    // Undo or redo ends them: the next walk starts over with as many.
    s.execute("line", &json!({ "points": [[0, -4], [1, -4]] })).unwrap();
    s.undo().unwrap();
    assert_eq!(ok(&mut s, COUNT), (N + 1).to_string());
    assert_eq!(held(&s), N + 1);
}

/// A saved drawing with one named attribute. The name comes from the command line, which does
/// not run the after-command pass.
fn clean_named() -> Session {
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    inserted(&mut s, json!({ "T1": "A1" }));
    {
        let st = s.state_mut().unwrap();
        st.saved = st.doc.clone();
        st.undo.clear();
        st.redo.clear();
    }
    s.cmdline("(setq a (entnext i))").unwrap();
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (entget a)))"), "\"ATTRIB\"");
    assert!(!s.state().unwrap().is_dirty(), "naming from the command line saves nothing");
    s
}

#[test]
fn issued_name_reads_keep_the_drawing_clean() {
    let mut s = clean_named();
    s.cmdline("(getvar \"DBMOD\")").unwrap();
    assert_eq!(ok(&mut s, "(getvar \"DBMOD\")"), "0");
    assert!(std::sync::Arc::ptr_eq(&s.state().unwrap().doc, &s.state().unwrap().saved));
    // A programmatic read runs the after-command pass. That pass must not make a saved drawing dirty.
    s.execute("lisp", &json!({ "code": "(entget a)" })).unwrap();
    assert_eq!(ok(&mut s, "(getvar \"DBMOD\")"), "0");
    assert!(std::sync::Arc::ptr_eq(&s.state().unwrap().doc, &s.state().unwrap().saved));
    assert_eq!(s.state().unwrap().undo.len(), 0, "a read is not an undo step");
}

#[test]
fn issued_name_selection_keeps_the_drawing_clean() {
    let mut s = clean_named();
    let saved = s.state().unwrap().saved.clone();
    s.execute("select", &json!({ "clear": true })).unwrap();
    assert!(s.state().unwrap().selection.is_empty());
    assert!(std::sync::Arc::ptr_eq(&s.state().unwrap().doc, &saved));
    assert!(!s.state().unwrap().is_dirty());
    assert_eq!(ok(&mut s, "(getvar \"DBMOD\")"), "0");
    assert_eq!(s.state().unwrap().undo.len(), 0, "clearing an empty selection is not an undo step");
}

#[test]
fn issued_name_undo_restores_the_saved_drawing() {
    let mut s = clean_named();
    let saved = s.state().unwrap().saved.clone();
    // A selection change writes no undo step. If it also changes the drawing, the next command
    // records that copy, and undo cannot return to the saved drawing.
    s.execute("select", &json!({ "clear": true })).unwrap();
    assert_eq!(s.state().unwrap().undo.len(), 0);
    s.execute("line", &json!({ "points": [[0, -3], [1, -3]] })).unwrap();
    assert!(s.undo().unwrap().is_some());
    assert!(std::sync::Arc::ptr_eq(&s.state().unwrap().doc, &saved), "undo returns the saved drawing, not a reservation copy");
    assert!(!s.state().unwrap().is_dirty());
    assert_eq!(ok(&mut s, "(getvar \"DBMOD\")"), "0");
}

#[test]
fn a_name_given_in_the_host_never_resolves_in_the_block_editor() {
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    let h = inserted(&mut s, json!({ "T1": "A1" }));
    ok(&mut s, "(setq a (entnext i))");
    let mut same = cadcraft_doc::Entity::clone(s.doc().unwrap().entity(h).unwrap());
    if let cadcraft_doc::EntityKind::Insert(ins) = &mut same.kind
        && let Some(at) = ins.attribs.first_mut()
    {
        at.text.value = "E1".into();
    }
    s.execute("bedit", &json!({ "name": "K" })).unwrap();
    // An object of the editor's drawing with the handle the host's reference has: the host's
    // name must not reach its ATTRIB.
    s.doc_mut().unwrap().model.push(same);
    assert_eq!(ok(&mut s, "(entget a)"), "nil");
    s.execute("bclose", &json!({ "save": false })).unwrap();
    assert_eq!(ok(&mut s, "(cdr (assoc 1 (entget (entnext i))))"), "\"A1\"");
}

#[test]
fn a_drawing_put_back_after_a_refused_change_ends_its_names() {
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    let before = s.state().unwrap().doc.clone();
    let h = inserted(&mut s, json!({ "T1": "A1" }));
    ok(&mut s, "(setq a (entnext i))");
    // A change with a number that is not finite: the drawing goes back as it was, without the
    // reference the name was given for.
    let bad = cadcraft_doc::EntityKind::Line(cadcraft_doc::Line { a: cadcraft_geom::Vec3::new(f64::NAN, 0.0, 0.0), b: cadcraft_geom::Vec3::ZERO });
    s.doc_mut().unwrap().add(&cadcraft_doc::Space::Model, cadcraft_doc::Common::default(), bad).unwrap();
    assert!(s.refuse_non_finite(None, &before, &[], "line").is_err());
    assert!(s.doc().unwrap().entity(h).is_none());
    // The next reference takes the handle back: the old name names nothing in it.
    assert_eq!(inserted(&mut s, json!({ "T1": "B1" })), h);
    assert_eq!(ok(&mut s, "(entget a)"), "nil");
    assert_eq!(ok(&mut s, "(cdr (assoc 1 (entget (entnext i))))"), "\"B1\"");
}

/// MOCORO's own Undo puts the drawing from before the Copy back, handle seed included.
/// The next Copy is given that handle again. The name of the removed copy's attribute
/// must not read it, walk from it, or change it.
#[test]
fn mocoro_undo_does_not_let_an_old_record_name_reach_the_next_copy() {
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    inserted(&mut s, json!({ "T1": "A1" }));
    ok(
        &mut s,
        r#"(progn
        (sssetfirst nil nil)
        (command "_.MOCORO" i "" '(0 0) "_Copy" '(10 0))
        (setq c (entlast) ch (cdr (assoc 5 (entget c))) a (entnext c) v (cdr (assoc 1 (entget a))))
        (command "_Undo" "_Copy" '(20 0) "_eXit"))"#,
    );
    // The name was given for a copy with a handle of its own, and it read that copy's attribute.
    // The copy made after the Undo has the removed copy's handle.
    assert_eq!(ok(&mut s, "(list (= ch (cdr (assoc 5 (entget i)))) v (= ch (cdr (assoc 5 (entget (entlast))))))"), "(nil \"A1\" T)");
    assert_eq!(
        ok(
            &mut s,
            r#"(progn
            (setq d (entget a) w (entnext a))
            (if d (entmod (subst (cons 1 "WRONG") (assoc 1 d) d)))
            (list (null d) (null w) (cdr (assoc 1 (entget (entnext (entlast)))))))"#
        ),
        "(T T \"A1\")"
    );
}

/// A grip Copy's Undo is the same kind of restore: the next copy reuses the handle.
#[test]
fn grip_copy_undo_does_not_let_an_old_record_name_reach_the_next_copy() {
    let mut s = with_attributes(&[("T1", 0, "D1")]);
    let h = inserted(&mut s, json!({ "T1": "A1" }));
    s.start_grip_edit(&[(h, 0)]).unwrap();
    // Enter leaves STRETCH for MOVE, which copies the reference whole.
    s.input(crate::Input::Enter).unwrap();
    s.input(crate::Input::Keyword("Copy".into())).unwrap();
    s.input(crate::Input::Point(cadcraft_geom::Vec2::new(20.0, 10.0))).unwrap();
    // The name is given for a copy with a handle of its own, and it reads that copy's attribute.
    assert_eq!(
        ok(
            &mut s,
            r#"(progn
            (setq c (entlast) ch (cdr (assoc 5 (entget c))) a (entnext c))
            (list (= ch (cdr (assoc 5 (entget i)))) (cdr (assoc 1 (entget a)))))"#
        ),
        "(nil \"A1\")"
    );
    s.input(crate::Input::Keyword("Undo".into())).unwrap();
    s.input(crate::Input::Point(cadcraft_geom::Vec2::new(30.0, 10.0))).unwrap();
    assert_eq!(ok(&mut s, "(= ch (cdr (assoc 5 (entget (entlast)))))"), "T", "the next copy takes the removed copy's handle");
    assert_eq!(
        ok(
            &mut s,
            r#"(progn
            (setq d (entget a) w (entnext a))
            (if d (entmod (subst (cons 1 "WRONG") (assoc 1 d) d)))
            (list (null d) (null w) (cdr (assoc 1 (entget (entnext (entlast)))))))"#
        ),
        "(T T \"A1\")"
    );
}
