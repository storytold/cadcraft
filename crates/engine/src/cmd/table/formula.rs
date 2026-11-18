//! Table cell formulas: `=Sum(A1:B3)`, `=Average(…)`, `=Count(…)`, `=Min(…)`, `=Max(…)`,
//! `=Abs(x)`, `=Round(x, n)`, cell references (`A1`, `$B$2`) and arithmetic (`+ - * / ^`,
//! parentheses). Columns are letters (A, B, … AA), rows count from 1 at the top, the title row
//! included.
//!
//! Everything is bounded: formula length, nesting depth, range size and the number of formulas.
//! Formula cells are evaluated in dependency order (no recursion across cells); cells in a
//! reference cycle show `#CYCLE`.

use std::collections::{HashMap, VecDeque};

use cadcraft_doc::{CellFormat, Table, TableStyle};

/// The longest formula accepted (characters).
pub const MAX_FORMULA_LEN: usize = 1024;
/// Parenthesis / function / sign nesting.
const MAX_DEPTH: usize = 32;
/// Cells one range may cover.
const MAX_RANGE_CELLS: usize = 100_000;
/// Formula cells evaluated per table.
const MAX_FORMULAS: usize = 20_000;
/// Range cells visited while finding dependencies, per table.
const MAX_DEP_VISITS: usize = 4_000_000;
/// Arguments of one function call.
const MAX_ARGS: usize = 256;

/// Why a formula has no value. The texts are CADCraft's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormulaError {
    /// Not a formula we can read.
    Syntax,
    /// A reference outside the table (or to a deleted row/column).
    Ref,
    /// The formula depends on itself.
    Cycle,
    /// Division by zero (or Average of nothing).
    Div0,
    /// Arithmetic on text.
    Value,
    /// Too large: range, nesting or number of formulas.
    Limit,
}

impl FormulaError {
    pub fn text(self) -> &'static str {
        match self {
            FormulaError::Syntax => "#SYNTAX",
            FormulaError::Ref => "#REF",
            FormulaError::Cycle => "#CYCLE",
            FormulaError::Div0 => "#DIV/0",
            FormulaError::Value => "#VALUE",
            FormulaError::Limit => "#LIMIT",
        }
    }
}

type R<T> = std::result::Result<T, FormulaError>;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tok {
    Num(f64),
    /// (row, col), 0-based.
    Cell(usize, usize),
    Name(usize, usize),
    /// A reference that was deleted (`#REF`).
    BadRef,
    Op(char),
}

/// Column letters → index (A = 0); `None` past ZZZ.
fn col_index(letters: &str) -> Option<usize> {
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let mut n = 0usize;
    for ch in letters.chars() {
        let d = (ch.to_ascii_uppercase() as usize).checked_sub('A' as usize)? + 1;
        if d > 26 {
            return None;
        }
        n = n * 26 + d;
    }
    Some(n - 1)
}

/// Column index → letters (0 = A).
pub fn col_name(mut c: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push((b'A' + (c % 26) as u8) as char);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    out.iter().rev().collect()
}

/// A1-style name of cell (`row`, `col`).
pub fn cell_name(row: usize, col: usize) -> String {
    format!("{}{}", col_name(col), row + 1)
}

/// Parse an `A1`-style cell name.
pub fn parse_cell_name(s: &str) -> Option<(usize, usize)> {
    let s = s.trim().replace('$', "");
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = s.split_at(split);
    if !letters.chars().all(|c| c.is_ascii_alphabetic()) || digits.len() > 7 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let row: usize = digits.parse().ok()?;
    Some((row.checked_sub(1)?, col_index(letters)?))
}

/// Tokens with their byte spans in `src`.
fn tokens(src: &str) -> R<Vec<(Tok, std::ops::Range<usize>)>> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&ch) = b.get(i) {
        let start = i;
        match ch {
            b' ' | b'\t' => i += 1,
            b'0'..=b'9' | b'.' => {
                while b.get(i).is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
                    i += 1;
                }
                if b.get(i).is_some_and(|c| *c == b'e' || *c == b'E')
                    && b.get(i + 1)
                        .is_some_and(|c| c.is_ascii_digit() || ((*c == b'+' || *c == b'-') && b.get(i + 2).is_some_and(u8::is_ascii_digit)))
                {
                    i += 2;
                    while b.get(i).is_some_and(u8::is_ascii_digit) {
                        i += 1;
                    }
                }
                let v: f64 = src.get(start..i).and_then(|t| t.parse().ok()).ok_or(FormulaError::Syntax)?;
                out.push((Tok::Num(v), start..i));
            }
            b'#' => {
                let rest = src.get(i..).unwrap_or("");
                if rest.len() >= 4 && rest.get(..4).is_some_and(|r| r.eq_ignore_ascii_case("#REF")) {
                    i += 4;
                    out.push((Tok::BadRef, start..i));
                } else {
                    return Err(FormulaError::Syntax);
                }
            }
            b'$' | b'A'..=b'Z' | b'a'..=b'z' => {
                while b.get(i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'$' || *c == b'_') {
                    i += 1;
                }
                let word = src.get(start..i).unwrap_or("");
                match parse_cell_name(word) {
                    Some((r, c)) if word.trim_start_matches('$').starts_with(|c: char| c.is_ascii_alphabetic()) => {
                        out.push((Tok::Cell(r, c), start..i))
                    }
                    _ if word.chars().all(|c| c.is_ascii_alphabetic()) => out.push((Tok::Name(start, i), start..i)),
                    _ => return Err(FormulaError::Syntax),
                }
            }
            b'+' | b'-' | b'*' | b'/' | b'^' | b'(' | b')' | b':' | b',' | b';' => {
                i += 1;
                out.push((Tok::Op(ch as char), start..i));
            }
            _ => return Err(FormulaError::Syntax),
        }
    }
    Ok(out)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Func {
    Sum,
    Average,
    Count,
    Min,
    Max,
    Abs,
    Round,
}

#[derive(Clone, Debug, PartialEq)]
enum Expr {
    Num(f64),
    Cell(usize, usize),
    /// Inclusive corners.
    Range((usize, usize), (usize, usize)),
    BadRef,
    Neg(Box<Expr>),
    /// `first (op operand)*`, evaluated left to right (`+ -` or `* /`).
    Chain(Box<Expr>, Vec<(char, Expr)>),
    /// `base ^ e1 ^ e2 …` (right to left).
    Pow(Vec<Expr>),
    Call(Func, Vec<Expr>),
}

struct Parser<'a> {
    src: &'a str,
    toks: Vec<(Tok, std::ops::Range<usize>)>,
    at: usize,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<Tok> {
        self.toks.get(self.at).map(|t| t.0)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.peek();
        self.at += 1;
        t
    }
    fn eat(&mut self, op: char) -> bool {
        if self.peek() == Some(Tok::Op(op)) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn enter(&mut self) -> R<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { Err(FormulaError::Limit) } else { Ok(()) }
    }
    fn expr(&mut self) -> R<Expr> {
        self.enter()?;
        let first = self.term()?;
        let mut rest = Vec::new();
        while let Some(Tok::Op(op @ ('+' | '-'))) = self.peek() {
            self.at += 1;
            rest.push((op, self.term()?));
        }
        self.depth -= 1;
        Ok(if rest.is_empty() { first } else { Expr::Chain(Box::new(first), rest) })
    }
    fn term(&mut self) -> R<Expr> {
        let first = self.power()?;
        let mut rest = Vec::new();
        while let Some(Tok::Op(op @ ('*' | '/'))) = self.peek() {
            self.at += 1;
            rest.push((op, self.power()?));
        }
        Ok(if rest.is_empty() { first } else { Expr::Chain(Box::new(first), rest) })
    }
    fn power(&mut self) -> R<Expr> {
        let mut parts = vec![self.unary()?];
        while self.eat('^') {
            parts.push(self.unary()?);
        }
        Ok(if parts.len() == 1 { parts.pop().unwrap_or(Expr::Num(0.0)) } else { Expr::Pow(parts) })
    }
    fn unary(&mut self) -> R<Expr> {
        let mut neg = false;
        let mut signs = 0;
        loop {
            if self.eat('-') {
                neg = !neg;
            } else if !self.eat('+') {
                break;
            }
            signs += 1;
            if signs > MAX_DEPTH {
                return Err(FormulaError::Limit);
            }
        }
        let e = self.primary()?;
        Ok(if neg { Expr::Neg(Box::new(e)) } else { e })
    }
    fn primary(&mut self) -> R<Expr> {
        match self.next().ok_or(FormulaError::Syntax)? {
            Tok::Num(v) => Ok(Expr::Num(v)),
            Tok::BadRef => Ok(Expr::BadRef),
            Tok::Cell(r, c) => {
                if self.eat(':') {
                    match self.next() {
                        Some(Tok::Cell(r2, c2)) => Ok(Expr::Range((r.min(r2), c.min(c2)), (r.max(r2), c.max(c2)))),
                        Some(Tok::BadRef) => Ok(Expr::BadRef),
                        _ => Err(FormulaError::Syntax),
                    }
                } else {
                    Ok(Expr::Cell(r, c))
                }
            }
            Tok::Op('(') => {
                let e = self.expr()?;
                if !self.eat(')') {
                    return Err(FormulaError::Syntax);
                }
                Ok(e)
            }
            Tok::Name(a, b) => {
                let name = self.src.get(a..b).unwrap_or("").to_ascii_lowercase();
                let f = match name.as_str() {
                    "sum" => Func::Sum,
                    "average" | "avg" => Func::Average,
                    "count" => Func::Count,
                    "min" => Func::Min,
                    "max" => Func::Max,
                    "abs" => Func::Abs,
                    "round" => Func::Round,
                    _ => return Err(FormulaError::Syntax),
                };
                if !self.eat('(') {
                    return Err(FormulaError::Syntax);
                }
                self.enter()?;
                let mut args = Vec::new();
                if !self.eat(')') {
                    loop {
                        if args.len() >= MAX_ARGS {
                            return Err(FormulaError::Limit);
                        }
                        args.push(self.expr()?);
                        if self.eat(')') {
                            break;
                        }
                        if !(self.eat(',') || self.eat(';')) {
                            return Err(FormulaError::Syntax);
                        }
                    }
                }
                self.depth -= 1;
                let arity_ok = match f {
                    Func::Abs => args.len() == 1,
                    Func::Round => (1..=2).contains(&args.len()),
                    _ => !args.is_empty(),
                };
                if !arity_ok {
                    return Err(FormulaError::Syntax);
                }
                Ok(Expr::Call(f, args))
            }
            _ => Err(FormulaError::Syntax),
        }
    }
}

/// Parse a formula (with or without its leading `=`).
fn parse(formula: &str) -> R<Expr> {
    let src = formula.trim();
    let src = src.strip_prefix('=').unwrap_or(src);
    if src.chars().count() > MAX_FORMULA_LEN {
        return Err(FormulaError::Limit);
    }
    let toks = tokens(src)?;
    let mut p = Parser { src, toks, at: 0, depth: 0 };
    let e = p.expr()?;
    if p.at != p.toks.len() {
        return Err(FormulaError::Syntax);
    }
    Ok(e)
}

/// Check a formula's syntax; the error text when it can't be read.
pub fn check(formula: &str) -> std::result::Result<(), &'static str> {
    parse(formula).map(|_| ()).map_err(FormulaError::text)
}

/// A cell's text as a number: plain numbers, with thousands separators or a percent sign.
pub fn number_of(text: &str) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() || t.len() > 64 {
        return None;
    }
    let (t, scale) = match t.strip_suffix('%') {
        Some(rest) => (rest.trim_end(), 0.01),
        None => (t, 1.0),
    };
    let plain: String = t.chars().filter(|c| *c != ',' && *c != ' ').collect();
    plain.parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| v * scale)
}

/// The cells `e` reads, as visited ranges; `false` when a range is too large.
fn refs(e: &Expr, out: &mut Vec<((usize, usize), (usize, usize))>) -> bool {
    match e {
        Expr::Num(_) | Expr::BadRef => true,
        Expr::Cell(r, c) => {
            out.push(((*r, *c), (*r, *c)));
            true
        }
        Expr::Range(a, b) => {
            let n = (b.0 - a.0 + 1).saturating_mul(b.1 - a.1 + 1);
            out.push((*a, *b));
            n <= MAX_RANGE_CELLS
        }
        Expr::Neg(x) => refs(x, out),
        Expr::Chain(first, rest) => refs(first, out) && rest.iter().all(|(_, x)| refs(x, out)),
        Expr::Pow(parts) => parts.iter().all(|x| refs(x, out)),
        Expr::Call(_, args) => args.iter().all(|x| refs(x, out)),
    }
}

/// Values of a table's cells during evaluation.
struct Values<'a> {
    t: &'a Table,
    rows: usize,
    cols: usize,
    /// Results of the formula cells evaluated so far.
    done: HashMap<(usize, usize), R<f64>>,
}

/// A value a function sees: a number, nothing (empty or text cell) or an error.
enum Arg {
    Num(f64),
    Blank,
}

impl Values<'_> {
    fn in_table(&self, r: usize, c: usize) -> bool {
        r < self.rows && c < self.cols
    }
    /// A referenced cell: `None` for an empty or text cell.
    fn cell(&self, r: usize, c: usize) -> R<Option<f64>> {
        if !self.in_table(r, c) {
            return Err(FormulaError::Ref);
        }
        if let Some(v) = self.done.get(&(r, c)) {
            return v.map(Some);
        }
        let cell = self.t.cells.get(r).and_then(|row| row.get(c));
        if cell.is_some_and(|x| x.formula.is_some()) {
            // A formula cell that was not evaluated depends on a cycle.
            return Err(FormulaError::Cycle);
        }
        Ok(cell.and_then(|x| number_of(&x.text)))
    }
    fn eval(&self, e: &Expr) -> R<f64> {
        match e {
            Expr::Num(v) => Ok(*v),
            Expr::BadRef => Err(FormulaError::Ref),
            Expr::Cell(r, c) => match self.cell(*r, *c)? {
                Some(v) => Ok(v),
                None => {
                    let empty = self.t.cells.get(*r).and_then(|row| row.get(*c)).is_none_or(|x| x.text.trim().is_empty());
                    if empty { Ok(0.0) } else { Err(FormulaError::Value) }
                }
            },
            // A bare range is its sum.
            Expr::Range(..) => self.aggregate(Func::Sum, std::slice::from_ref(e)),
            Expr::Neg(x) => Ok(-self.eval(x)?),
            Expr::Chain(first, rest) => {
                let mut acc = self.eval(first)?;
                for (op, x) in rest {
                    let v = self.eval(x)?;
                    acc = match op {
                        '+' => acc + v,
                        '-' => acc - v,
                        '*' => acc * v,
                        _ if v == 0.0 => return Err(FormulaError::Div0),
                        _ => acc / v,
                    };
                }
                finite(acc)
            }
            Expr::Pow(parts) => {
                let mut it = parts.iter().rev();
                let mut acc = match it.next() {
                    Some(x) => self.eval(x)?,
                    None => return Err(FormulaError::Syntax),
                };
                for x in it {
                    acc = self.eval(x)?.powf(acc);
                }
                finite(acc)
            }
            Expr::Call(f @ (Func::Abs | Func::Round), args) => {
                let x = self.eval(args.first().ok_or(FormulaError::Syntax)?)?;
                match f {
                    Func::Abs => Ok(x.abs()),
                    _ => {
                        let n = match args.get(1) {
                            Some(a) => self.eval(a)?,
                            None => 0.0,
                        };
                        let k = 10f64.powi(n.round().clamp(-8.0, 8.0) as i32);
                        finite((x * k).round() / k)
                    }
                }
            }
            Expr::Call(f, args) => self.aggregate(*f, args),
        }
    }
    fn args(&self, args: &[Expr], out: &mut Vec<Arg>) -> R<()> {
        for a in args {
            match a {
                Expr::Range((r0, c0), (r1, c1)) => {
                    if !self.in_table(*r1, *c1) {
                        return Err(FormulaError::Ref);
                    }
                    if (r1 - r0 + 1).saturating_mul(c1 - c0 + 1) > MAX_RANGE_CELLS {
                        return Err(FormulaError::Limit);
                    }
                    for r in *r0..=*r1 {
                        for c in *c0..=*c1 {
                            out.push(self.cell(r, c)?.map(Arg::Num).unwrap_or(Arg::Blank));
                        }
                    }
                }
                Expr::Cell(r, c) => out.push(self.cell(*r, *c)?.map(Arg::Num).unwrap_or(Arg::Blank)),
                e => out.push(Arg::Num(self.eval(e)?)),
            }
        }
        Ok(())
    }
    fn aggregate(&self, f: Func, args: &[Expr]) -> R<f64> {
        let mut vals = Vec::new();
        self.args(args, &mut vals)?;
        let nums: Vec<f64> = vals.iter().filter_map(|a| if let Arg::Num(v) = a { Some(*v) } else { None }).collect();
        let v = match f {
            Func::Count => nums.len() as f64,
            Func::Average if nums.is_empty() => return Err(FormulaError::Div0),
            Func::Average => nums.iter().sum::<f64>() / nums.len() as f64,
            Func::Min => nums.iter().copied().fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.min(v)))).unwrap_or(0.0),
            Func::Max => nums.iter().copied().fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v)))).unwrap_or(0.0),
            _ => nums.iter().sum(),
        };
        finite(v)
    }
}

fn finite(v: f64) -> R<f64> {
    if v.is_finite() { Ok(v) } else { Err(FormulaError::Value) }
}

/// Evaluate every formula of `t` and store the results as the cells' text (formatted by each
/// cell's style). Returns whether any text changed.
pub fn recalc(t: &mut Table, style: &TableStyle) -> bool {
    let rows = t.row_heights.len().min(t.cells.len());
    let cols = t.col_widths.len();
    let mut cells: Vec<(usize, usize)> = Vec::new();
    for (r, row) in t.cells.iter().enumerate().take(rows) {
        for (c, cell) in row.iter().enumerate().take(cols) {
            if cell.formula.is_some() {
                cells.push((r, c));
            }
        }
    }
    if cells.is_empty() {
        return false;
    }
    let mut results: HashMap<(usize, usize), R<f64>> = HashMap::new();
    let over = cells.len() > MAX_FORMULAS;
    // Parse, then order by dependencies between formula cells (Kahn's algorithm).
    let index: HashMap<(usize, usize), usize> = cells.iter().enumerate().map(|(i, rc)| (*rc, i)).collect();
    let mut parsed: Vec<Option<Expr>> = Vec::with_capacity(cells.len());
    let mut users: Vec<Vec<usize>> = vec![Vec::new(); cells.len()];
    let mut pending = vec![0usize; cells.len()];
    let mut visits = 0usize;
    for (i, (r, c)) in cells.iter().enumerate() {
        let f = t.cells.get(*r).and_then(|row| row.get(*c)).and_then(|x| x.formula.clone()).unwrap_or_default();
        let e = if over { Err(FormulaError::Limit) } else { parse(&f) };
        let e = match e {
            Ok(e) => e,
            Err(err) => {
                results.insert((*r, *c), Err(err));
                parsed.push(None);
                continue;
            }
        };
        let mut rs = Vec::new();
        if !refs(&e, &mut rs) {
            results.insert((*r, *c), Err(FormulaError::Limit));
            parsed.push(None);
            continue;
        }
        let mut deps: Vec<usize> = Vec::new();
        'ranges: for ((r0, c0), (r1, c1)) in rs {
            for rr in r0..=r1.min(rows.saturating_sub(1)) {
                for cc in c0..=c1.min(cols.saturating_sub(1)) {
                    visits += 1;
                    if visits > MAX_DEP_VISITS {
                        break 'ranges;
                    }
                    if let Some(&j) = index.get(&(rr, cc)) {
                        deps.push(j);
                    }
                }
            }
        }
        if visits > MAX_DEP_VISITS {
            results.insert((*r, *c), Err(FormulaError::Limit));
            parsed.push(None);
            continue;
        }
        deps.sort_unstable();
        deps.dedup();
        if let Some(p) = pending.get_mut(i) {
            *p = deps.len();
        }
        for j in deps {
            if let Some(u) = users.get_mut(j) {
                u.push(i);
            }
        }
        parsed.push(Some(e));
    }
    // Cells that failed to parse count as evaluated (their error propagates).
    let mut queue: VecDeque<usize> = (0..cells.len()).filter(|i| pending.get(*i) == Some(&0)).collect();
    let mut vals = Values { t, rows, cols, done: results };
    while let Some(i) = queue.pop_front() {
        let Some(&(r, c)) = cells.get(i) else { continue };
        if let Some(Some(e)) = parsed.get(i) {
            let v = vals.eval(e);
            vals.done.insert((r, c), v);
        }
        for &u in users.get(i).map(Vec::as_slice).unwrap_or(&[]) {
            if let Some(p) = pending.get_mut(u) {
                *p = p.saturating_sub(1);
                if *p == 0 {
                    queue.push_back(u);
                }
            }
        }
    }
    let mut done = vals.done;
    for rc in &cells {
        done.entry(*rc).or_insert(Err(FormulaError::Cycle));
    }
    // Show the results.
    let mut changed = false;
    for (r, c) in cells {
        let cs = style.cell_style(t.cell_style_name(r, c));
        let Some(cell) = t.cells.get_mut(r).and_then(|row| row.get_mut(c)) else { continue };
        let text = match (done.get(&(r, c)), cs.format) {
            (_, CellFormat::Text) => cell.formula.clone().unwrap_or_default(),
            (Some(Ok(v)), _) => cs.format_number(*v),
            (Some(Err(e)), _) => e.text().to_string(),
            (None, _) => FormulaError::Cycle.text().to_string(),
        };
        if cell.text != text {
            cell.text = text;
            changed = true;
        }
    }
    changed
}

/// Which way a row or column insertion/deletion moves references.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shift {
    InsertRow(usize),
    DeleteRow(usize),
    InsertCol(usize),
    DeleteCol(usize),
}

/// The `lo..=hi` extent of a range after inserting a row/column at `at`.
fn shift_span(lo: usize, hi: usize, at: usize) -> (usize, usize) {
    (if lo >= at { lo + 1 } else { lo }, if hi >= at { hi + 1 } else { hi })
}

/// The `lo..=hi` extent of a range after deleting row/column `at` (the start stays, the next
/// row moves up into it; the end moves up); `None` when the range was only that row/column.
fn shift_span_delete(lo: usize, hi: usize, at: usize) -> Option<(usize, usize)> {
    if lo == at && hi == at {
        return None;
    }
    Some((if lo > at { lo - 1 } else { lo }, if hi >= at { hi.checked_sub(1)? } else { hi }))
}

/// `formula` with its references moved for a row/column insertion or deletion; references to a
/// deleted row/column become `#REF` (range corners move inward instead).
pub fn shift_refs(formula: &str, shift: Shift) -> String {
    let Ok(toks) = tokens(formula.trim().strip_prefix('=').unwrap_or(formula.trim())) else { return formula.to_string() };
    let src = formula.trim();
    let body_start = src.len() - src.strip_prefix('=').unwrap_or(src).len();
    let body = src.get(body_start..).unwrap_or("");
    let mut out = String::from(src.get(..body_start).unwrap_or(""));
    let mut last = 0;
    let mut skip_to = 0;
    for (k, (tok, span)) in toks.iter().enumerate() {
        if k < skip_to {
            continue;
        }
        let Tok::Cell(r, c) = *tok else { continue };
        // `A5:A2` is the same range as `A2:A5`: move the normalised corners as one unit.
        if let (Some((Tok::Op(':'), _)), Some((Tok::Cell(r2, c2), end))) = (toks.get(k + 1), toks.get(k + 2)) {
            skip_to = k + 3;
            let (rlo, rhi, clo, chi) = (r.min(*r2), r.max(*r2), c.min(*c2), c.max(*c2));
            let moved = match shift {
                Shift::InsertRow(at) => Some((shift_span(rlo, rhi, at), (clo, chi))),
                Shift::DeleteRow(at) => shift_span_delete(rlo, rhi, at).map(|rr| (rr, (clo, chi))),
                Shift::InsertCol(at) => Some(((rlo, rhi), shift_span(clo, chi, at))),
                Shift::DeleteCol(at) => shift_span_delete(clo, chi, at).map(|cc| ((rlo, rhi), cc)),
            };
            out.push_str(body.get(last..span.start).unwrap_or(""));
            match moved {
                Some(((r0, r1), (c0, c1))) if (r0, r1, c0, c1) == (rlo, rhi, clo, chi) => {
                    out.push_str(body.get(span.start..end.end).unwrap_or(""));
                }
                Some(((r0, r1), (c0, c1))) => {
                    out.push_str(&format!("{}:{}", cell_name(r0, c0), cell_name(r1, c1)));
                }
                None => out.push_str("#REF"),
            }
            last = end.end;
            continue;
        }
        let moved = |v: usize, at: usize, insert: bool| -> Option<usize> {
            if insert {
                Some(if v >= at { v + 1 } else { v })
            } else if v > at {
                Some(v - 1)
            } else if v == at {
                None
            } else {
                Some(v)
            }
        };
        let new = match shift {
            Shift::InsertRow(at) => moved(r, at, true).map(|r| (r, c)),
            Shift::DeleteRow(at) => moved(r, at, false).map(|r| (r, c)),
            Shift::InsertCol(at) => moved(c, at, true).map(|c| (r, c)),
            Shift::DeleteCol(at) => moved(c, at, false).map(|c| (r, c)),
        };
        out.push_str(body.get(last..span.start).unwrap_or(""));
        match new {
            Some((r2, c2)) if (r2, c2) == (r, c) => out.push_str(body.get(span.clone()).unwrap_or("")),
            Some((r2, c2)) => out.push_str(&cell_name(r2, c2)),
            None => out.push_str("#REF"),
        }
        last = span.end;
    }
    out.push_str(body.get(last..).unwrap_or(""));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(25), "Z");
        assert_eq!(col_name(26), "AA");
        assert_eq!(parse_cell_name("AA10"), Some((9, 26)));
        assert_eq!(parse_cell_name("$b$2"), Some((1, 1)));
        assert_eq!(parse_cell_name("A0"), None);
        assert_eq!(parse_cell_name("ABCD1"), None);
    }

    #[test]
    fn syntax() {
        assert!(check("=Sum(A1:B3)").is_ok());
        assert!(check("=(A1+2)*-B2^2/3").is_ok());
        assert_eq!(check("=Sum(A1:"), Err("#SYNTAX"));
        assert_eq!(check("=Foo(1)"), Err("#SYNTAX"));
        assert_eq!(check(&format!("={}1{}", "(".repeat(100), ")".repeat(100))), Err("#LIMIT"));
        assert_eq!(check(&format!("={}", "1+".repeat(600))), Err("#LIMIT"));
        assert_eq!(check(&format!("={}1", "-".repeat(100))), Err("#LIMIT"));
    }

    #[test]
    fn shifting_references() {
        assert_eq!(shift_refs("=A1+A3", Shift::InsertRow(1)), "=A1+A4");
        assert_eq!(shift_refs("=Sum(A2:A5)", Shift::DeleteRow(1)), "=Sum(A2:A4)");
        assert_eq!(shift_refs("=Sum(A2:A5)", Shift::DeleteRow(4)), "=Sum(A2:A4)");
        assert_eq!(shift_refs("=B2*2", Shift::DeleteCol(1)), "=#REF*2");
        assert_eq!(shift_refs("=C1", Shift::InsertCol(0)), "=D1");
        assert_eq!(shift_refs("=Sum(A3:A3)", Shift::DeleteRow(2)), "=Sum(#REF)");
        assert_eq!(shift_refs("=Sum(A3:C3)", Shift::DeleteCol(1)), "=Sum(A3:B3)");
        assert_eq!(shift_refs("=Sum(B3:B3)", Shift::DeleteCol(1)), "=Sum(#REF)");
        assert_eq!(shift_refs("=Sum(A5:A2)", Shift::DeleteRow(1)), "=Sum(A2:A4)");
        assert_eq!(shift_refs("=Sum(A5:A2)", Shift::DeleteRow(4)), "=Sum(A2:A4)");
        assert_eq!(shift_refs("=Sum(C1:A1)", Shift::DeleteCol(0)), "=Sum(A1:B1)");
        assert_eq!(shift_refs("=Sum(A5:A2)", Shift::InsertRow(2)), "=Sum(A2:A6)");
        assert_eq!(shift_refs("=Sum(A5:A2)", Shift::InsertRow(9)), "=Sum(A5:A2)");
    }
}
