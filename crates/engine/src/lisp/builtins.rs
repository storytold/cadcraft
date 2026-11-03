//! The built-in functions: special forms, numbers, lists, strings, geometry, output, files and
//! the functions that call functions (`apply`, `mapcar`, `eval`, `load`, `vl-sort`…). Drawing
//! access (`command`, `entget`, `ssget`, `getvar`…) is in `drawing.rs`, user input in `input.rs`.
//!
//! Errors use AutoLISP's wording (`bad argument type: numberp: nil`, `too few arguments`,
//! `divide by zero`), since routines and their `*error*` handlers match on it.

use std::cmp::Ordering;
use std::ffi::OsStr;
use std::sync::Arc;

use super::eval::{Ctx, Flow, Special};
use super::value::{Builtin, BuiltinKind, Sym, Value, charge_bytes, check_string_len, eq, equal, real_text, to_text};
use super::{Env, LispError, drawing, input, read};

macro_rules! cxrs {
    ($($name:ident $path:literal),*) => {
        $(fn $name(a: &[Value]) -> R { cxr(a, $path) })*
    };
}
cxrs!(car "a", cdr "d", caar "aa", cadr "ad", cdar "da", cddr "dd", caaar "aaa", caadr "aad", cadar "ada", caddr "add",
      cdaar "daa", cdadr "dad", cddar "dda", cdddr "ddd", cadddr "addd", cddddr "dddd");

// ---------------- the table ----------------

macro_rules! pure {
    ($name:literal, $f:path) => {
        Builtin { name: $name, kind: BuiltinKind::Pure($f) }
    };
}
macro_rules! ctx {
    ($name:literal, $f:path) => {
        Builtin { name: $name, kind: BuiltinKind::Ctx($f) }
    };
}
macro_rules! special {
    ($name:literal, $s:expr) => {
        Builtin { name: $name, kind: BuiltinKind::Special($s) }
    };
}

mod args;
mod files;
mod functions;
mod geometry;
mod lists;
mod numbers;
mod output;
mod strings;
mod wcmatch;

use args::R;
pub use args::args_err;
pub use files::{LispFile, read_text_file};
use files::{close, findfile, open, read_line, write_line};
pub use functions::catch_all_error;
use functions::{
    apply, autoload, boundp, eval_fn, exit, load, mapcar, noop, set, ver, vl_catch_all_apply, vl_catch_all_error_message, vl_catch_all_error_p,
    vl_every, vl_member_if, vl_remove_if, vl_remove_if_not, vl_some, vl_sort, vl_symbol_value,
};
use geometry::{angle, distance, inters, polar};
use lists::{
    acad_strlsort, append, assoc, atom, cons, consp, cxr, eq_fn, equal_fn, last, length, list, listp, member, nth, null, reverse, subst, type_of,
    vl_list_length, vl_position, vl_remove,
};
use numbers::{
    abs, add, atan, cos, div, exp, expt, fix, float, gcd, ge, gt, le, log, logand, logior, lognot, lsh, lt, max, min, minusp, mul, num_eq, num_ne,
    numberp, one_minus, one_plus, rem, sin, sqrt, sub, zerop,
};
use output::{alert, prin1, princ, print, prompt, terpri};
pub use output::{is_null_symbol, null_symbol, real};
use strings::{
    ascii, atof, atoi, chr, itoa, read_fn, strcase, strcat, strlen, substr, vl_filename_base, vl_filename_directory, vl_filename_extension,
    vl_list_to_string, vl_prin1_to_string, vl_princ_to_string, vl_string_left_trim, vl_string_right_trim, vl_string_search, vl_string_subst,
    vl_string_to_list, vl_string_translate, vl_string_trim, vl_symbol_name, vl_symbolp,
};
use wcmatch::wcmatch;
pub use wcmatch::wild_match;

static BUILTINS: &[Builtin] = &[
    special!("QUOTE", Special::Quote),
    special!("FUNCTION", Special::Function),
    special!("SETQ", Special::Setq),
    special!("IF", Special::If),
    special!("COND", Special::Cond),
    special!("WHILE", Special::While),
    special!("REPEAT", Special::Repeat),
    special!("FOREACH", Special::Foreach),
    special!("PROGN", Special::Progn),
    special!("AND", Special::And),
    special!("OR", Special::Or),
    special!("DEFUN", Special::Defun),
    special!("DEFUN-Q", Special::Defun),
    special!("LAMBDA", Special::Lambda),
    pure!("+", add),
    pure!("-", sub),
    pure!("*", mul),
    pure!("/", div),
    pure!("REM", rem),
    pure!("1+", one_plus),
    pure!("1-", one_minus),
    pure!("ABS", abs),
    pure!("MIN", min),
    pure!("MAX", max),
    pure!("SQRT", sqrt),
    pure!("EXP", exp),
    pure!("LOG", log),
    pure!("SIN", sin),
    pure!("COS", cos),
    pure!("ATAN", atan),
    pure!("EXPT", expt),
    pure!("FIX", fix),
    pure!("FLOAT", float),
    pure!("GCD", gcd),
    pure!("LOGAND", logand),
    pure!("LOGIOR", logior),
    pure!("LSH", lsh),
    pure!("~", lognot),
    pure!("=", num_eq),
    pure!("/=", num_ne),
    pure!("<", lt),
    pure!(">", gt),
    pure!("<=", le),
    pure!(">=", ge),
    pure!("ZEROP", zerop),
    pure!("MINUSP", minusp),
    pure!("NUMBERP", numberp),
    pure!("ATOM", atom),
    pure!("LISTP", listp),
    pure!("VL-CONSP", consp),
    pure!("NULL", null),
    pure!("NOT", null),
    pure!("TYPE", type_of),
    pure!("EQ", eq_fn),
    pure!("EQUAL", equal_fn),
    pure!("CAR", car),
    pure!("CDR", cdr),
    pure!("CAAR", caar),
    pure!("CADR", cadr),
    pure!("CDAR", cdar),
    pure!("CDDR", cddr),
    pure!("CAAAR", caaar),
    pure!("CAADR", caadr),
    pure!("CADAR", cadar),
    pure!("CADDR", caddr),
    pure!("CDAAR", cdaar),
    pure!("CDADR", cdadr),
    pure!("CDDAR", cddar),
    pure!("CDDDR", cdddr),
    pure!("CADDDR", cadddr),
    pure!("CDDDDR", cddddr),
    pure!("CONS", cons),
    pure!("LIST", list),
    pure!("APPEND", append),
    pure!("REVERSE", reverse),
    pure!("LENGTH", length),
    pure!("NTH", nth),
    pure!("LAST", last),
    pure!("MEMBER", member),
    pure!("ASSOC", assoc),
    pure!("SUBST", subst),
    pure!("VL-POSITION", vl_position),
    pure!("VL-REMOVE", vl_remove),
    pure!("VL-LIST-LENGTH", vl_list_length),
    pure!("ACAD_STRLSORT", acad_strlsort),
    pure!("STRCAT", strcat),
    pure!("STRLEN", strlen),
    pure!("SUBSTR", substr),
    pure!("STRCASE", strcase),
    pure!("ITOA", itoa),
    pure!("ATOI", atoi),
    pure!("ATOF", atof),
    pure!("CHR", chr),
    pure!("ASCII", ascii),
    pure!("READ", read_fn),
    pure!("WCMATCH", wcmatch),
    pure!("VL-PRIN1-TO-STRING", vl_prin1_to_string),
    pure!("VL-PRINC-TO-STRING", vl_princ_to_string),
    pure!("VL-SYMBOL-NAME", vl_symbol_name),
    pure!("VL-SYMBOLP", vl_symbolp),
    pure!("VL-STRING-SEARCH", vl_string_search),
    pure!("VL-STRING-SUBST", vl_string_subst),
    pure!("VL-STRING-TRIM", vl_string_trim),
    pure!("VL-STRING-LEFT-TRIM", vl_string_left_trim),
    pure!("VL-STRING-RIGHT-TRIM", vl_string_right_trim),
    pure!("VL-STRING-TRANSLATE", vl_string_translate),
    pure!("VL-STRING->LIST", vl_string_to_list),
    pure!("VL-LIST->STRING", vl_list_to_string),
    pure!("VL-FILENAME-BASE", vl_filename_base),
    pure!("VL-FILENAME-EXTENSION", vl_filename_extension),
    pure!("VL-FILENAME-DIRECTORY", vl_filename_directory),
    pure!("POLAR", polar),
    pure!("DISTANCE", distance),
    pure!("ANGLE", angle),
    pure!("INTERS", inters),
    pure!("FINDFILE", findfile),
    pure!("VL-CATCH-ALL-ERROR-P", vl_catch_all_error_p),
    pure!("VL-CATCH-ALL-ERROR-MESSAGE", vl_catch_all_error_message),
    pure!("VL-LOAD-COM", noop),
    pure!("GC", noop),
    pure!("REDRAW", noop),
    pure!("VER", ver),
    ctx!("PRINC", princ),
    ctx!("PRIN1", prin1),
    ctx!("PRINT", print),
    ctx!("PROMPT", prompt),
    ctx!("TERPRI", terpri),
    ctx!("ALERT", alert),
    ctx!("OPEN", open),
    ctx!("CLOSE", close),
    ctx!("READ-LINE", read_line),
    ctx!("WRITE-LINE", write_line),
    ctx!("APPLY", apply),
    ctx!("MAPCAR", mapcar),
    ctx!("EVAL", eval_fn),
    ctx!("SET", set),
    ctx!("BOUNDP", boundp),
    ctx!("VL-SYMBOL-VALUE", vl_symbol_value),
    ctx!("VL-SORT", vl_sort),
    ctx!("VL-REMOVE-IF", vl_remove_if),
    ctx!("VL-REMOVE-IF-NOT", vl_remove_if_not),
    ctx!("VL-MEMBER-IF", vl_member_if),
    ctx!("VL-EVERY", vl_every),
    ctx!("VL-SOME", vl_some),
    ctx!("VL-CATCH-ALL-APPLY", vl_catch_all_apply),
    ctx!("EXIT", exit),
    ctx!("QUIT", exit),
    ctx!("LOAD", load),
    ctx!("AUTOLOAD", autoload),
    ctx!("INITGET", input::initget),
    ctx!("GETPOINT", input::getpoint),
    ctx!("GETCORNER", input::getcorner),
    ctx!("GETDIST", input::getdist),
    ctx!("GETANGLE", input::getangle),
    ctx!("GETORIENT", input::getorient),
    ctx!("GETREAL", input::getreal),
    ctx!("GETINT", input::getint),
    ctx!("GETSTRING", input::getstring),
    ctx!("GETKWORD", input::getkword),
    ctx!("ENTSEL", input::entsel),
    ctx!("NENTSEL", input::entsel),
    ctx!("COMMAND", drawing::command),
    ctx!("COMMAND-S", drawing::command),
    ctx!("VL-CMDF", drawing::command),
    ctx!("GETVAR", drawing::getvar),
    ctx!("SETVAR", drawing::setvar),
    ctx!("ENTGET", drawing::entget),
    ctx!("ENTMAKE", drawing::entmake),
    ctx!("ENTMOD", drawing::entmod),
    ctx!("ENTDEL", drawing::entdel),
    ctx!("ENTLAST", drawing::entlast),
    ctx!("ENTNEXT", drawing::entnext),
    ctx!("ENTUPD", drawing::entupd),
    ctx!("HANDENT", drawing::handent),
    ctx!("SSGET", drawing::ssget),
    ctx!("SSADD", drawing::ssadd),
    ctx!("SSDEL", drawing::ssdel),
    ctx!("SSLENGTH", drawing::sslength),
    ctx!("SSNAME", drawing::ssname),
    ctx!("SSMEMB", drawing::ssmemb),
    ctx!("SSSETFIRST", drawing::sssetfirst),
    ctx!("SSGETFIRST", drawing::ssgetfirst),
    ctx!("TBLSEARCH", drawing::tblsearch),
    ctx!("TBLNEXT", drawing::tblnext),
    ctx!("RTOS", drawing::rtos),
    ctx!("ANGTOS", drawing::angtos),
    ctx!("DISTOF", drawing::distof),
    ctx!("ANGTOF", drawing::angtof),
    ctx!("TRANS", drawing::trans),
];

/// Bind the builtins and constants in a new environment.
pub fn install(env: &mut Env) {
    for b in BUILTINS {
        env.globals.insert(Sym::new(b.name), Value::Subr(b));
    }
    env.globals.insert(Sym::new("T"), Value::t());
    env.globals.insert(Sym::new("PI"), Value::Real(std::f64::consts::PI));
    env.globals.insert(Sym::new("PAUSE"), Value::str("\\"));
    #[cfg(test)]
    env.globals.insert(Sym::new(TEST_PANIC.name), Value::Subr(&TEST_PANIC));
}

/// Tests only: a built-in that panics, to prove the guards around the machine.
#[cfg(test)]
static TEST_PANIC: Builtin = Builtin { name: "CADCRAFT-TEST-PANIC", kind: BuiltinKind::Pure(test_panic) };
#[cfg(test)]
#[allow(clippy::panic)]
fn test_panic(_a: &[Value]) -> R {
    panic!("test panic")
}
