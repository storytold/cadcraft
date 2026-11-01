//! AutoLISP values: integers (32-bit, wrapping as AutoLISP's do), reals, strings, symbols,
//! lists built of cons cells, entity names, selection sets, files and functions.
//!
//! Values are shared (`Arc`) so a session stays `Send`. Cons cells (and functions) are dropped
//! iteratively: a list a million long, or nested a million deep, would otherwise overflow the
//! stack when it goes away.
//!
//! Memory is bounded so a runaway routine fails with a LISP error instead of the process running
//! out of memory (which no panic guard can catch): live cons cells are counted ([`live_cells`],
//! checked by the evaluator at every step), no string a built-in makes may be longer than
//! [`MAX_STRING`], and the strings and printed text a run makes share a byte budget
//! ([`charge_bytes`]).

use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cadcraft_doc::Handle;

use super::LispError;

/// A symbol: its name in upper case (AutoLISP symbols are case-insensitive).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sym(pub Arc<str>);

impl Sym {
    pub fn new(name: &str) -> Sym {
        Sym(Arc::from(name.to_ascii_uppercase().as_str()))
    }
    pub fn name(&self) -> &str {
        &self.0
    }
}

/// A cons cell.
#[derive(Debug)]
pub struct Cons {
    pub car: Value,
    pub cdr: Value,
}

/// Most cons cells alive at once (about 64 bytes each): far above real routines (a list of
/// every entity of a large drawing, its `entget` lists), well below memory trouble.
pub const MAX_CELLS: usize = 5_000_000;
/// Longest string a built-in may make, in bytes.
pub const MAX_STRING: usize = 8 << 20;
/// Bytes of new strings, printed text and files read one run may make in all.
pub const STRING_BUDGET: u64 = 2 << 30;

/// Cons cells alive in the process.
static LIVE_CELLS: AtomicUsize = AtomicUsize::new(0);

/// Tests that make or cap many cells run one at a time: the count is shared by the process.
#[cfg(test)]
pub(crate) static HEAVY_CELLS: Mutex<()> = Mutex::new(());

/// Cons cells alive now (all sessions).
pub fn live_cells() -> usize {
    LIVE_CELLS.load(Ordering::Relaxed)
}

thread_local! {
    /// The running slice's string budget: bytes made so far and the most allowed.
    static STRING_BYTES: Cell<(u64, u64)> = const { Cell::new((0, STRING_BUDGET)) };
    /// The running slice's cap on live cons cells.
    static CELL_LIMIT: Cell<usize> = const { Cell::new(MAX_CELLS) };
}

/// Start a slice of a run on this thread: it fails when more than `limit` cells are alive.
pub fn set_cell_limit(limit: usize) {
    CELL_LIMIT.with(|c| c.set(limit));
}

/// Too many cons cells are alive: the error that stops the run.
pub fn check_cells() -> Result<(), LispError> {
    if live_cells() > CELL_LIMIT.with(Cell::get) { Err(LispError::quit("out of memory: too many list cells")) } else { Ok(()) }
}

/// Start a slice of a run on this thread: it has made `used` bytes of its `budget`.
pub fn set_string_budget(used: u64, budget: u64) {
    STRING_BYTES.with(|c| c.set((used, budget)));
}

/// The bytes the run has made so far on this thread.
pub fn string_bytes_used() -> u64 {
    STRING_BYTES.with(|c| c.get().0)
}

/// Count `n` new bytes (a string, printed text, a file read) against the run's budget.
pub fn charge_bytes(n: usize) -> Result<(), LispError> {
    STRING_BYTES.with(|c| {
        let (used, budget) = c.get();
        let used = used.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
        c.set((used, budget));
        if used > budget { Err(LispError::quit("out of memory: the routine made too much text")) } else { Ok(()) }
    })
}

/// A string of `len` bytes may be made.
pub fn check_string_len(len: usize) -> Result<(), LispError> {
    if len > MAX_STRING { Err(LispError::quit(format!("out of memory: string longer than {MAX_STRING} bytes"))) } else { Ok(()) }
}

/// A built-in's result: a new string (one nothing else holds) is checked and charged.
pub fn charge_result(v: &Value) -> Result<(), LispError> {
    if let Value::Str(s) = v
        && Arc::strong_count(s) == 1
    {
        check_string_len(s.len())?;
        charge_bytes(s.len())?;
    }
    Ok(())
}

impl Drop for Cons {
    fn drop(&mut self) {
        // Cells are only made by `Value::cons`, which counts them; never wrap below zero.
        if LIVE_CELLS.fetch_sub(1, Ordering::Relaxed) == 0 {
            LIVE_CELLS.fetch_add(1, Ordering::Relaxed);
        }
        // Unlink everything this cell alone holds (the rest of the list, nested lists in its
        // car, function bodies) one cell at a time, so nothing recurses on the stack.
        let mut work = Vec::new();
        keep_for_drop(&mut work, std::mem::take(&mut self.car));
        keep_for_drop(&mut work, std::mem::take(&mut self.cdr));
        while let Some(v) = work.pop() {
            match v {
                Value::Cons(c) => {
                    if let Ok(mut cell) = Arc::try_unwrap(c) {
                        keep_for_drop(&mut work, std::mem::take(&mut cell.car));
                        keep_for_drop(&mut work, std::mem::take(&mut cell.cdr));
                    }
                }
                Value::Func(f) => {
                    if let Ok(mut func) = Arc::try_unwrap(f) {
                        keep_for_drop(&mut work, std::mem::take(&mut func.body));
                    }
                }
                _ => {}
            }
        }
    }
}

/// Values that can hold others go on the work list; atoms are dropped here.
fn keep_for_drop(work: &mut Vec<Value>, v: Value) {
    if matches!(v, Value::Cons(_) | Value::Func(_)) {
        work.push(v);
    }
}

/// A selection set: the entities it holds, in order. `ssadd` and `ssdel` change it in place.
#[derive(Clone, Debug)]
pub struct PickSet {
    pub id: u32,
    pub items: Arc<Mutex<Vec<Handle>>>,
}

/// A function defined in LISP (`defun`, `lambda`).
#[derive(Debug)]
pub struct Func {
    pub name: Option<Sym>,
    pub params: Vec<Sym>,
    /// Local variables (after the `/` in the argument list): bound to nil during a call.
    pub locals: Vec<Sym>,
    /// The body forms, as a list.
    pub body: Value,
}

/// A built-in function. `special` forms get their arguments unevaluated.
pub struct Builtin {
    pub name: &'static str,
    pub kind: BuiltinKind,
}

impl std::fmt::Debug for Builtin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<Subr: {}>", self.name)
    }
}

#[derive(Clone, Copy)]
pub enum BuiltinKind {
    /// Evaluated arguments to a value.
    Pure(fn(&[Value]) -> Result<Value, LispError>),
    /// Evaluated arguments, with the interpreter (and its session) at hand.
    Ctx(fn(&mut super::eval::Ctx<'_>, Vec<Value>) -> Result<super::eval::Flow, LispError>),
    /// A special form, handled by the evaluator itself.
    Special(super::eval::Special),
}

#[derive(Clone, Debug, Default)]
pub enum Value {
    #[default]
    Nil,
    Int(i32),
    Real(f64),
    Str(Arc<str>),
    Sym(Sym),
    Cons(Arc<Cons>),
    Ename(Handle),
    PickSet(PickSet),
    File(u32),
    Subr(&'static Builtin),
    Func(Arc<Func>),
}

impl Value {
    pub fn t() -> Value {
        Value::Sym(Sym::new("T"))
    }
    pub fn bool(b: bool) -> Value {
        if b { Value::t() } else { Value::Nil }
    }
    pub fn str(s: &str) -> Value {
        Value::Str(Arc::from(s))
    }
    pub fn sym(s: &str) -> Value {
        Value::Sym(Sym::new(s))
    }
    pub fn cons(car: Value, cdr: Value) -> Value {
        LIVE_CELLS.fetch_add(1, Ordering::Relaxed);
        Value::Cons(Arc::new(Cons { car, cdr }))
    }
    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }
    /// A proper list of `items`.
    pub fn list(items: impl IntoIterator<Item = Value, IntoIter: DoubleEndedIterator>) -> Value {
        items.into_iter().rev().fold(Value::Nil, |acc, v| Value::cons(v, acc))
    }
    /// `items` followed by `tail` (a dotted list when `tail` is an atom).
    pub fn list_with_tail(items: Vec<Value>, tail: Value) -> Value {
        items.into_iter().rev().fold(tail, |acc, v| Value::cons(v, acc))
    }
    /// A point as a list of reals.
    pub fn point(p: &[f64]) -> Value {
        Value::list(p.iter().map(|v| Value::Real(*v)).collect::<Vec<_>>())
    }
    pub fn car(&self) -> Value {
        match self {
            Value::Cons(c) => c.car.clone(),
            _ => Value::Nil,
        }
    }
    pub fn cdr(&self) -> Value {
        match self {
            Value::Cons(c) => c.cdr.clone(),
            _ => Value::Nil,
        }
    }
    /// The elements of a list (a dotted tail is left out).
    pub fn iter(&self) -> ListIter {
        ListIter { next: self.clone() }
    }
    pub fn to_vec(&self) -> Vec<Value> {
        self.iter().collect()
    }
    /// A proper list (nil or cons cells ending in nil).
    pub fn is_list(&self) -> bool {
        let mut v = self.clone();
        loop {
            match v {
                Value::Nil => return true,
                Value::Cons(c) => v = c.cdr.clone(),
                _ => return false,
            }
        }
    }
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(f64::from(*i)),
            Value::Real(r) => Some(*r),
            _ => None,
        }
    }
    pub fn as_sym(&self) -> Option<&Sym> {
        match self {
            Value::Sym(s) => Some(s),
            _ => None,
        }
    }
    /// The type name `type` returns (nil for nil).
    pub fn type_name(&self) -> Option<&'static str> {
        Some(match self {
            Value::Nil => return None,
            Value::Int(_) => "INT",
            Value::Real(_) => "REAL",
            Value::Str(_) => "STR",
            Value::Sym(_) => "SYM",
            Value::Cons(_) => "LIST",
            Value::Ename(_) => "ENAME",
            Value::PickSet(_) => "PICKSET",
            Value::File(_) => "FILE",
            Value::Subr(_) => "SUBR",
            Value::Func(_) => "USUBR",
        })
    }
    /// A 2D or 3D point given as a list of two or three numbers.
    pub fn as_point(&self) -> Option<[f64; 3]> {
        let mut it = self.iter();
        let x = it.next()?.as_number()?;
        let y = it.next()?.as_number()?;
        let z = match it.next() {
            Some(v) => v.as_number()?,
            None => 0.0,
        };
        (it.next().is_none() && self.is_list()).then_some([x, y, z])
    }
}

pub struct ListIter {
    next: Value,
}

impl Iterator for ListIter {
    type Item = Value;
    fn next(&mut self) -> Option<Value> {
        match std::mem::take(&mut self.next) {
            Value::Cons(c) => {
                self.next = c.cdr.clone();
                Some(c.car.clone())
            }
            _ => None,
        }
    }
}

/// Deepest nesting `equal`, printing and reading follow before giving up (hostile input).
pub const MAX_DEPTH: usize = 2_000;

/// `eq`: the same atom or the same cons cell.
pub fn eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Cons(x), Value::Cons(y)) => Arc::ptr_eq(x, y),
        (Value::Str(x), Value::Str(y)) => Arc::ptr_eq(x, y) || x == y,
        (Value::PickSet(x), Value::PickSet(y)) => x.id == y.id,
        (Value::Func(x), Value::Func(y)) => Arc::ptr_eq(x, y),
        (Value::Subr(x), Value::Subr(y)) => std::ptr::eq(*x, *y),
        _ => atom_equal(a, b, 0.0),
    }
}

/// `equal`: the same structure, numbers within `fuzz`.
pub fn equal(a: &Value, b: &Value, fuzz: f64) -> bool {
    // A shared cons DAG can expand into exponentially many node visits when compared as a tree.
    // Bound the total comparisons so `equal` on adversarial shared structure cannot monopolise the
    // thread (CWE-407); exhausting the budget reports "not equal" rather than looping.
    let mut budget: u64 = EQUAL_BUDGET;
    equal_depth(a, b, fuzz, 0, &mut budget)
}

/// Largest number of node comparisons one `equal` call may perform.
const EQUAL_BUDGET: u64 = 50_000_000;

/// Compare nested values within depth and node-work limits, returning false when either limit is reached.
fn equal_depth(a: &Value, b: &Value, fuzz: f64, depth: usize, budget: &mut u64) -> bool {
    if depth > MAX_DEPTH || *budget == 0 {
        return false;
    }
    *budget -= 1;
    let (mut a, mut b) = (a.clone(), b.clone());
    loop {
        if *budget == 0 {
            return false;
        }
        match (&a, &b) {
            (Value::Cons(x), Value::Cons(y)) => {
                if !equal_depth(&x.car, &y.car, fuzz, depth + 1, budget) {
                    return false;
                }
                let (na, nb) = (x.cdr.clone(), y.cdr.clone());
                a = na;
                b = nb;
            }
            _ => return atom_equal(&a, &b, fuzz),
        }
    }
}

fn atom_equal(a: &Value, b: &Value, fuzz: f64) -> bool {
    match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Sym(x), Value::Sym(y)) => x == y,
        (Value::Ename(x), Value::Ename(y)) => x == y,
        (Value::File(x), Value::File(y)) => x == y,
        (Value::PickSet(x), Value::PickSet(y)) => x.id == y.id,
        (Value::Subr(x), Value::Subr(y)) => std::ptr::eq(*x, *y),
        (Value::Func(x), Value::Func(y)) => Arc::ptr_eq(x, y),
        (x, y) => match (x.as_number(), y.as_number()) {
            (Some(p), Some(q)) if matches!(x, Value::Real(_)) || matches!(y, Value::Real(_)) || fuzz > 0.0 => (p - q).abs() <= fuzz,
            _ => false,
        },
    }
}

/// A real as AutoLISP prints it: six significant digits, always with a decimal point.
pub fn real_text(r: f64) -> String {
    if !r.is_finite() {
        return if r.is_nan() {
            "-1.#IND".into()
        } else if r > 0.0 {
            "1.#INF".into()
        } else {
            "-1.#INF".into()
        };
    }
    if r == 0.0 {
        return "0.0".into();
    }
    // Round to six significant digits first so the exponent already includes any carry
    // (999999.5 -> 1.00000e6) and subnormals never divide by an underflowed power of ten.
    let sci = format!("{:.5e}", r.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    // As C's %g with six digits: exponent form outside 1e-5 ..< 1e6.
    if !(-5..6).contains(&exp) {
        // Exponent form: d.ddddde+NNN
        let mut s = trim_fraction(mantissa);
        if !s.contains('.') {
            s.push_str(".0");
        }
        let sign = if r < 0.0 { "-" } else { "" };
        return format!("{sign}{s}e{}{:03}", if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    let decimals = (5 - exp).max(0) as usize;
    let mut s = trim_fraction(&format!("{r:.decimals$}"));
    if !s.contains('.') {
        s.push_str(".0");
    }
    s
}

fn trim_fraction(s: &str) -> String {
    if s.contains('.') {
        let t = s.trim_end_matches('0');
        t.strip_suffix('.').map_or(t.to_string(), |u| format!("{u}.0"))
    } else {
        s.to_string()
    }
}

/// `prin1`/`print` text (`escape`: strings quoted with their escapes) or `princ` text.
pub fn to_text(v: &Value, escape: bool) -> String {
    let mut out = String::new();
    write_value(&mut out, v, escape, 0);
    if out.len() > MAX_STRING {
        let mut end = MAX_STRING;
        while !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
    }
    out
}

fn write_value(out: &mut String, v: &Value, escape: bool, depth: usize) {
    // Text past the string cap is cut (a list sharing its parts can print exponentially long).
    if out.len() > MAX_STRING {
        return;
    }
    if depth > MAX_DEPTH {
        out.push_str("...");
        return;
    }
    match v {
        Value::Nil => out.push_str("nil"),
        Value::Int(i) => out.push_str(&i.to_string()),
        Value::Real(r) => out.push_str(&real_text(*r)),
        Value::Str(s) => {
            if escape {
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\t' => out.push_str("\\t"),
                        '\r' => out.push_str("\\r"),
                        '\u{1b}' => out.push_str("\\e"),
                        c => out.push(c),
                    }
                }
                out.push('"');
            } else {
                out.push_str(s);
            }
        }
        Value::Sym(s) => out.push_str(s.name()),
        Value::Cons(_) => {
            // (quote x) prints as 'x.
            if let (Value::Sym(s), Value::Cons(rest)) = (v.car(), v.cdr())
                && s.name() == "QUOTE"
                && rest.cdr.is_nil()
            {
                out.push('\'');
                write_value(out, &rest.car, escape, depth + 1);
                return;
            }
            out.push('(');
            let mut cur = v.clone();
            let mut first = true;
            let mut count = 0usize;
            loop {
                match cur {
                    Value::Cons(c) => {
                        if !first {
                            out.push(' ');
                        }
                        first = false;
                        write_value(out, &c.car, escape, depth + 1);
                        cur = c.cdr.clone();
                        count += 1;
                        if count > 1_000_000 || out.len() > MAX_STRING {
                            out.push_str(" ...");
                            break;
                        }
                    }
                    Value::Nil => break,
                    other => {
                        out.push_str(" . ");
                        write_value(out, &other, escape, depth + 1);
                        break;
                    }
                }
            }
            out.push(')');
        }
        Value::Ename(h) => out.push_str(&format!("<Entity name: {:x}>", h.0)),
        Value::PickSet(p) => out.push_str(&format!("<Selection set: {:x}>", p.id)),
        Value::File(f) => out.push_str(&format!("#<FILE {f:x}>")),
        Value::Subr(b) => out.push_str(&format!("#<SUBR @{} {}>", "0", b.name)),
        Value::Func(f) => out.push_str(&format!("#<USUBR {}>", f.name.as_ref().map_or("-lambda-", |s| s.name()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reals_print_with_six_significant_digits() {
        assert_eq!(real_text(1.0), "1.0");
        assert_eq!(real_text(std::f64::consts::PI), "3.14159");
        assert_eq!(real_text(1.23456789), "1.23457");
        assert_eq!(real_text(0.1), "0.1");
        assert_eq!(real_text(12.5), "12.5");
        assert_eq!(real_text(-2.0), "-2.0");
        assert_eq!(real_text(100000.0), "100000.0");
        assert_eq!(real_text(123456.7), "123457.0");
        assert_eq!(real_text(0.0001), "0.0001");
        assert_eq!(real_text(1_000_000.0), "1.0e+006");
        assert_eq!(real_text(0.000001), "1.0e-006");
    }

    #[test]
    fn a_long_list_drops_without_overflowing_the_stack() {
        let _heavy = HEAVY_CELLS.lock().unwrap_or_else(|e| e.into_inner());
        let mut l = Value::Nil;
        for i in 0..2_000_000 {
            l = Value::cons(Value::Int(i), l);
        }
        drop(l);
    }

    #[test]
    fn printing_and_equality() {
        let l = Value::list(vec![Value::Int(1), Value::str("a\"b"), Value::cons(Value::Int(8), Value::str("0"))]);
        assert_eq!(to_text(&l, true), "(1 \"a\\\"b\" (8 . \"0\"))");
        assert_eq!(to_text(&l, false), "(1 a\"b (8 . 0))");
        assert!(equal(&l, &l.clone(), 0.0));
        assert!(equal(&Value::Real(1.0), &Value::Real(1.0000001), 1e-3));
        assert!(!equal(&Value::Real(1.0), &Value::Real(1.1), 1e-3));
        assert!(eq(&Value::sym("a"), &Value::sym("A")));
    }
}
