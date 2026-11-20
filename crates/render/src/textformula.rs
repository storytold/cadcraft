//! Formulas in text, as in a spreadsheet (the evaluator; the engine applies it to drawings): a text whose value starts with `=` shows the result
//! of the formula (`=2400-2*18`, `=TEXT(TODAY(),"dd.mm.yyyy")`, `=ROUND(PI()*125^2/1000000,3)`).
//! The formula is kept with the text ([`cadcraft_doc::Text::formula`]) and worked out again
//! when the drawing is opened (dates follow the computer's clock) and when it is edited.
//!
//! AutoCAD fields read from drawings (`%<\AcVar date \f "dd.MM.yyyy">%`) are worked out the same
//! way when CADCraft knows them (dates, the file name); others keep the value AutoCAD saved.
//!
//! The functions are listed in [`FUNCTIONS`] (name, arguments, description), which the editor's
//! completion list and Help show.

/// Longest formula worked out.
const MAX_LEN: usize = 2000;
/// Deepest nesting.
const MAX_DEPTH: usize = 40;
/// Most arguments of one function.
const MAX_ARGS: usize = 255;

/// The functions: (name, arguments, description). Shown by completion and Help.
pub const FUNCTIONS: &[(&str, &str, &str)] = &[
    ("ABS", "number", "The number without its sign."),
    ("ACOS", "number", "Arccosine, in radians."),
    ("AND", "logical1, logical2, ...", "TRUE when all arguments are true."),
    ("ASIN", "number", "Arcsine, in radians."),
    ("ATAN", "number", "Arctangent, in radians."),
    ("AVERAGE", "number1, number2, ...", "The average of the numbers."),
    ("CONCAT", "text1, text2, ...", "Joins texts (as the & operator)."),
    ("COS", "angle", "Cosine of an angle in radians."),
    ("DATE", "year, month, day", "A date."),
    ("DAY", "date", "The day of the month of a date."),
    ("DEGREES", "angle", "Radians to degrees."),
    ("EXP", "number", "e raised to a power."),
    ("FILENAME", "", "The drawing's file name."),
    ("FIXED", "number, decimals", "A number as text with that many decimals."),
    ("HOUR", "time", "The hour of a time."),
    ("IF", "condition, value_if_true, value_if_false", "One of two values, by a condition."),
    ("INT", "number", "Rounds down to an integer."),
    ("LEFT", "text, count", "The first characters of a text."),
    ("LEN", "text", "The number of characters of a text."),
    ("LN", "number", "Natural logarithm."),
    ("LOG10", "number", "Base-10 logarithm."),
    ("LOWER", "text", "A text in lower case."),
    ("MAX", "number1, number2, ...", "The largest number."),
    ("MID", "text, start, count", "Characters from the middle of a text (start = 1 for the first)."),
    ("MIN", "number1, number2, ...", "The smallest number."),
    ("MINUTE", "time", "The minutes of a time."),
    ("MOD", "number, divisor", "The remainder of a division."),
    ("MONTH", "date", "The month of a date (1 to 12)."),
    ("NOT", "logical", "The opposite of a logical value."),
    ("NOW", "", "The current date and time (worked out again when the drawing is opened)."),
    ("OR", "logical1, logical2, ...", "TRUE when any argument is true."),
    ("PI", "", "The number π."),
    ("POWER", "number, power", "A number raised to a power (as ^)."),
    ("RADIANS", "angle", "Degrees to radians."),
    ("RIGHT", "text, count", "The last characters of a text."),
    ("ROUND", "number, decimals", "Rounds to that many decimals."),
    ("ROUNDDOWN", "number, decimals", "Rounds toward zero."),
    ("ROUNDUP", "number, decimals", "Rounds away from zero."),
    ("SHEET", "", "The name of the current layout (\"Model\" in model space)."),
    ("SIN", "angle", "Sine of an angle in radians."),
    ("SQRT", "number", "Square root."),
    ("SUM", "number1, number2, ...", "Adds the numbers."),
    ("TAN", "angle", "Tangent of an angle in radians."),
    ("TEXT", "value, format", "A value as text in a format: \"0.00\", \"#,##0\", \"0%\", \"dd.mm.yyyy\", \"hh:mm\"..."),
    ("TODAY", "", "Today's date (worked out again when the drawing is opened)."),
    ("TRIM", "text", "A text without spaces at its ends and double spaces."),
    ("UPPER", "text", "A text in capitals."),
    ("YEAR", "date", "The year of a date."),
];

/// What a formula can refer to: the time, the file and the layout.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Env {
    /// Local date and time: year, month, day, hour, minute, second; `None` where there is no
    /// clock to read (then date formulas and fields keep the result they have).
    pub now: Option<(i32, u32, u32, u32, u32, u32)>,
    /// The drawing's file name and the sheet (layout) name; empty when not known here (then
    /// FILENAME() and SHEET() keep the result they have).
    pub file: String,
    pub sheet: String,
    /// A dimension's measurement, which `<>` stands for in its formula.
    pub measured: Option<f64>,
}

/// The local date and time now (UTC where the time zone is unknown); `None` on the web build,
/// which has no clock to read here: date formulas and fields then keep their saved results
/// instead of showing a made-up date.
pub fn local_now() -> Option<(i32, u32, u32, u32, u32, u32)> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let z = jiff::Zoned::now();
        let clamp = |v: i8| u32::try_from(v).unwrap_or(0);
        Some((i32::from(z.year()), clamp(z.month()), clamp(z.day()), clamp(z.hour()), clamp(z.minute()), clamp(z.second())))
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

/// Whether `text` is a formula: one line starting with `=` (after spaces).
pub fn is_formula(text: &str) -> bool {
    let t = text.trim();
    t.starts_with('=') && t.len() > 1 && !t.contains(['\n', '\r'])
}

/// Whether `text` holds AutoCAD field codes.
pub fn has_fields(text: &str) -> bool {
    text.contains("%<\\")
}

/// Work out a formula (`=...`) or the fields of a field template: the text to show, or `None`
/// for a field CADCraft doesn't know (the saved value stays).
pub fn evaluate(formula: &str, env: &Env) -> Option<String> {
    if is_formula(formula) {
        return Some(match eval(formula.trim_start().trim_start_matches('='), env) {
            Ok(v) => v.show(),
            // Something this place can't know (the clock, the file or sheet name): the formula
            // keeps the result it has.
            Err(Err::Unknown) => return None,
            Err(e) => e.to_string(),
        });
    }
    if has_fields(formula) {
        return fields(formula, env);
    }
    None
}

// ---------- values ----------

#[derive(Clone, Debug, PartialEq)]
enum Val {
    Num(f64),
    /// A date and time as a serial number (days since 30 December 1899, as spreadsheets).
    Date(f64),
    Text(String),
    Bool(bool),
}

/// Formula errors, shown as spreadsheets show them.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Err {
    /// An unknown function or name.
    Name,
    /// A wrong kind of value.
    Value,
    Div0,
    /// A number out of range (square root of -1...).
    Num,
    /// The formula can't be read.
    Syntax,
    /// A value not known here (no clock, no file or sheet name): not an error to show.
    Unknown,
}

impl std::fmt::Display for Err {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Err::Name => "#NAME?",
            Err::Value => "#VALUE!",
            Err::Div0 => "#DIV/0!",
            Err::Num => "#NUM!",
            Err::Syntax => "#ERROR!",
            Err::Unknown => "#N/A",
        })
    }
}

type R = std::result::Result<Val, Err>;

impl Val {
    fn num(&self) -> std::result::Result<f64, Err> {
        match self {
            Val::Num(v) | Val::Date(v) => Ok(*v),
            Val::Bool(b) => Ok(f64::from(u8::from(*b))),
            Val::Text(t) => t.trim().replace(',', ".").parse::<f64>().map_err(|_| Err::Value),
        }
    }
    fn text(&self) -> String {
        self.show()
    }
    fn truth(&self) -> std::result::Result<bool, Err> {
        match self {
            Val::Bool(b) => Ok(*b),
            Val::Text(t) if t.eq_ignore_ascii_case("true") => Ok(true),
            Val::Text(t) if t.eq_ignore_ascii_case("false") => Ok(false),
            v => Ok(v.num()? != 0.0),
        }
    }
    /// As the text shows it: numbers without trailing zeros (at most 10 decimals), dates as
    /// dd.mm.yyyy (with the time when it has one), TRUE/FALSE.
    fn show(&self) -> String {
        match self {
            Val::Num(v) => general(*v),
            Val::Date(d) => {
                if d.fract().abs() < 1e-9 {
                    format_date(*d, "dd.mm.yyyy")
                } else {
                    format_date(*d, "dd.mm.yyyy hh:mm")
                }
            }
            Val::Text(t) => t.clone(),
            Val::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        }
    }
}

/// A number with at most 10 decimals, trailing zeros dropped.
fn general(v: f64) -> String {
    if !v.is_finite() {
        return Err::Num.to_string();
    }
    if v.abs() >= 1e15 || (v != 0.0 && v.abs() < 1e-10) {
        return format!("{v:e}");
    }
    let s = format!("{v:.10}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

// ---------- dates ----------

/// Days from 1970-01-01 of a civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The spreadsheet serial of 1970-01-01.
const EPOCH_SERIAL: i64 = 25_569;

fn serial(y: i64, m: i64, d: i64) -> f64 {
    // Months beyond 1..12 roll into the next or previous years, as spreadsheets do.
    let (y, m) = (y + (m - 1).div_euclid(12), (m - 1).rem_euclid(12) + 1);
    (days_from_civil(y, m, 1) + d - 1 + EPOCH_SERIAL) as f64
}

/// (year, month, day, hour, minute, second) of a serial.
fn parts(s: f64) -> (i64, i64, i64, i64, i64, i64) {
    // About ±8000 years around 1900: the calendar arithmetic stays far from overflowing (a
    // file's formula can hold any number, `=YEAR(1E300)`).
    let s = if s.is_finite() { s.clamp(-3_000_000.0, 3_000_000.0) } else { 0.0 };
    let days = s.floor();
    let secs = ((s - days) * 86_400.0).round() as i64;
    let (y, m, d) = civil_from_days(days as i64 - EPOCH_SERIAL);
    (y, m, d, secs / 3600 % 24, secs / 60 % 60, secs % 60)
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// A date in a spreadsheet date format: d dd ddd? m mm mmm mmmm yy yyyy h hh mm (minutes after
/// h) ss; other characters as they are.
fn format_date(s: f64, fmt: &str) -> String {
    let (y, mo, d, h, mi, se) = parts(s);
    let chars: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut after_hour = false;
    while let Some(&c) = chars.get(i) {
        let run = chars.get(i..).unwrap_or_default().iter().take_while(|x| x.eq_ignore_ascii_case(&c)).count();
        match c.to_ascii_lowercase() {
            'y' => {
                out += &if run <= 2 { format!("{:02}", y.rem_euclid(100)) } else { format!("{y:04}") };
                after_hour = false;
            }
            'm' if after_hour && run <= 2 => {
                out += &if run == 1 { mi.to_string() } else { format!("{mi:02}") };
                after_hour = false;
            }
            'm' => {
                let name = MONTHS.get(usize::try_from(mo - 1).unwrap_or(0)).copied().unwrap_or("");
                out += &match run {
                    1 => mo.to_string(),
                    2 => format!("{mo:02}"),
                    3 => name.chars().take(3).collect(),
                    _ => name.into(),
                };
            }
            'd' => {
                out += &if run == 1 { d.to_string() } else { format!("{d:02}") };
                after_hour = false;
            }
            'h' => {
                out += &if run == 1 { h.to_string() } else { format!("{h:02}") };
                after_hour = true;
            }
            's' => {
                out += &if run == 1 { se.to_string() } else { format!("{se:02}") };
            }
            _ => {
                out.extend(std::iter::repeat_n(c, run));
            }
        }
        i += run;
    }
    out
}

/// A number in a spreadsheet number format: `0`, `0.00`, `#,##0.00`, `0%`, `0.0E+0`-less.
/// Text before and after the digits stays (`"L=" 0.0` → `L=12.5`, quotes dropped).
fn format_number(v: f64, fmt: &str) -> String {
    let percent = fmt.contains('%');
    let v = if percent { v * 100.0 } else { v };
    let start = fmt.find(['0', '#']);
    let end = fmt.rfind(['0', '#']).map(|e| e + 1);
    let (Some(a), Some(b)) = (start, end) else { return fmt.replace('"', "") };
    let core = fmt.get(a..b).unwrap_or("0");
    let decimals = core.split_once('.').map_or(0, |(_, f)| f.chars().filter(|c| *c == '0' || *c == '#').count());
    let grouped = core.split_once('.').map_or(core, |(i, _)| i).contains(',');
    let mut s = format!("{:.*}", decimals.min(15), v.abs());
    if grouped {
        let (int, frac) = s.split_once('.').map_or((s.clone(), String::new()), |(i, f)| (i.to_string(), format!(".{f}")));
        let mut g = String::new();
        for (k, ch) in int.chars().enumerate() {
            if k > 0 && (int.len() - k) % 3 == 0 {
                g.push(',');
            }
            g.push(ch);
        }
        s = g + &frac;
    }
    let sign = if v < 0.0 && s.chars().any(|c| c.is_ascii_digit() && c != '0') { "-" } else { "" };
    let pre = fmt.get(..a).unwrap_or("").replace('"', "");
    let post = fmt.get(b..).unwrap_or("").replace('"', "");
    format!("{pre}{sign}{s}{post}")
}

fn looks_like_date_format(f: &str) -> bool {
    // Text in double quotes is literal (`0.0" days"`), and a format with digit placeholders
    // (`0`, `#`) is a number format.
    let mut quoted = false;
    let l: String = f
        .chars()
        .filter(|c| {
            if *c == '"' {
                quoted = !quoted;
                return false;
            }
            !quoted
        })
        .collect::<String>()
        .to_ascii_lowercase();
    if l.contains(['0', '#']) {
        return false;
    }
    l.contains('y') || l.contains('d') || l.contains("mm") || l.contains('h')
}

// ---------- parsing and evaluation ----------

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Name(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
}

fn lex(s: &str) -> std::result::Result<Vec<Tok>, Err> {
    let c: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&ch) = c.get(i) {
        match ch {
            ' ' | '\t' => i += 1,
            '0'..='9' | '.' => {
                let st = i;
                while c.get(i).is_some_and(|x| x.is_ascii_digit() || *x == '.') {
                    i += 1;
                }
                if c.get(i).is_some_and(|x| *x == 'e' || *x == 'E') && c.get(i + 1).is_some_and(|x| x.is_ascii_digit() || *x == '-' || *x == '+') {
                    i += 2;
                    while c.get(i).is_some_and(char::is_ascii_digit) {
                        i += 1;
                    }
                }
                let t: String = c.get(st..i).unwrap_or_default().iter().collect();
                out.push(Tok::Num(t.parse().map_err(|_| Err::Syntax)?));
            }
            '"' => {
                i += 1;
                let mut t = String::new();
                loop {
                    match c.get(i) {
                        None => return Err(Err::Syntax),
                        Some('"') if c.get(i + 1) == Some(&'"') => {
                            t.push('"');
                            i += 2;
                        }
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        Some(x) => {
                            t.push(*x);
                            i += 1;
                        }
                    }
                }
                out.push(Tok::Str(t));
            }
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            // `;` separates arguments too (spreadsheets in languages with a decimal comma).
            ',' | ';' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '<' | '>' => {
                let two: String = c.get(i..i + 2).unwrap_or_default().iter().collect();
                let op = match two.as_str() {
                    "<=" => "<=",
                    ">=" => ">=",
                    "<>" => "<>",
                    _ if ch == '<' => "<",
                    _ => ">",
                };
                i += op.len();
                out.push(Tok::Op(op));
            }
            '+' | '-' | '*' | '/' | '^' | '&' | '=' | '%' => {
                out.push(Tok::Op(match ch {
                    '+' => "+",
                    '-' => "-",
                    '*' => "*",
                    '/' => "/",
                    '^' => "^",
                    '&' => "&",
                    '=' => "=",
                    _ => "%",
                }));
                i += 1;
            }
            x if x.is_alphabetic() || x == '_' => {
                let st = i;
                while c.get(i).is_some_and(|y| y.is_alphanumeric() || *y == '_' || *y == '.') {
                    i += 1;
                }
                out.push(Tok::Name(c.get(st..i).unwrap_or_default().iter().collect::<String>().to_ascii_uppercase()));
            }
            _ => return Err(Err::Syntax),
        }
        if out.len() > MAX_LEN {
            return Err(Err::Syntax);
        }
    }
    Ok(out)
}

struct P<'a> {
    t: Vec<Tok>,
    i: usize,
    depth: usize,
    env: &'a Env,
}

fn eval(src: &str, env: &Env) -> R {
    if src.len() > MAX_LEN {
        return Err(Err::Syntax);
    }
    let mut p = P { t: lex(src)?, i: 0, depth: 0, env };
    let v = p.compare()?;
    if p.i < p.t.len() {
        return Err(Err::Syntax);
    }
    Ok(v)
}

impl P<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn is_op(&self, ops: &[&str]) -> Option<&'static str> {
        match self.peek() {
            Some(Tok::Op(o)) if ops.contains(o) => Some(o),
            _ => None,
        }
    }
    fn compare(&mut self) -> R {
        let a = self.concat()?;
        if let Some(op) = self.is_op(&["=", "<>", "<", ">", "<=", ">="]) {
            self.i += 1;
            let b = self.concat()?;
            let ord = match (&a, &b) {
                (Val::Text(x), Val::Text(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
                _ => a.num()?.partial_cmp(&b.num()?).ok_or(Err::Num)?,
            };
            use std::cmp::Ordering::*;
            return Ok(Val::Bool(match op {
                "=" => ord == Equal,
                "<>" => ord != Equal,
                "<" => ord == Less,
                ">" => ord == Greater,
                "<=" => ord != Greater,
                _ => ord != Less,
            }));
        }
        Ok(a)
    }
    fn concat(&mut self) -> R {
        let mut a = self.sum()?;
        while self.is_op(&["&"]).is_some() {
            self.i += 1;
            let b = self.sum()?;
            a = Val::Text(a.text() + &b.text());
        }
        Ok(a)
    }
    fn sum(&mut self) -> R {
        let mut a = self.product()?;
        while let Some(op) = self.is_op(&["+", "-"]) {
            self.i += 1;
            let b = self.product()?;
            let v = if op == "+" { a.num()? + b.num()? } else { a.num()? - b.num()? };
            // A date plus or minus days is a date; the difference of two dates is a number.
            a = match (&a, &b) {
                (Val::Date(_), Val::Date(_)) => Val::Num(v),
                (Val::Date(_), _) | (_, Val::Date(_)) => Val::Date(v),
                _ => Val::Num(v),
            };
        }
        Ok(a)
    }
    fn product(&mut self) -> R {
        let mut a = self.power()?;
        while let Some(op) = self.is_op(&["*", "/"]) {
            self.i += 1;
            let b = self.power()?;
            let (x, y) = (a.num()?, b.num()?);
            a = Val::Num(if op == "*" {
                x * y
            } else {
                if y == 0.0 {
                    return Err(Err::Div0);
                }
                x / y
            });
        }
        Ok(a)
    }
    fn power(&mut self) -> R {
        // Left to right, as in a spreadsheet: =2^3^2 is (2^3)^2 = 64. A loop, so a long chain
        // takes no stack.
        let mut a = self.unary()?;
        while self.is_op(&["^"]).is_some() {
            self.i += 1;
            let b = self.unary()?;
            let v = a.num()?.powf(b.num()?);
            if !v.is_finite() {
                return Err(Err::Num);
            }
            a = Val::Num(v);
        }
        Ok(a)
    }
    fn unary(&mut self) -> R {
        if let Some(op) = self.is_op(&["-", "+"]) {
            self.depth += 1;
            if self.depth > MAX_DEPTH {
                return Err(Err::Syntax);
            }
            self.i += 1;
            let v = self.unary()?;
            self.depth -= 1;
            return Ok(Val::Num(if op == "-" { -v.num()? } else { v.num()? }));
        }
        let mut v = self.atom()?;
        while self.is_op(&["%"]).is_some() {
            self.i += 1;
            v = Val::Num(v.num()? / 100.0);
        }
        Ok(v)
    }
    /// Where the argument starting here ends: the next comma or closing parenthesis outside
    /// any parentheses it opens (`None`: the formula ends first).
    fn arg_end(&self) -> Option<usize> {
        let mut level = 0usize;
        for (k, t) in self.t.iter().enumerate().skip(self.i) {
            match t {
                Tok::LParen => level += 1,
                Tok::RParen if level == 0 => return Some(k),
                Tok::RParen => level -= 1,
                Tok::Comma if level == 0 => return Some(k),
                _ => {}
            }
        }
        None
    }
    fn atom(&mut self) -> R {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Err::Syntax);
        }
        let tok = self.peek().cloned();
        self.i += 1;
        let r = match tok {
            Some(Tok::Num(v)) => Ok(Val::Num(v)),
            Some(Tok::Str(s)) => Ok(Val::Text(s)),
            Some(Tok::LParen) => {
                let v = self.compare()?;
                if self.peek() != Some(&Tok::RParen) {
                    return Err(Err::Syntax);
                }
                self.i += 1;
                Ok(v)
            }
            // In a dimension's formula, `<>` is its measurement.
            Some(Tok::Op("<>")) if self.env.measured.is_some() => Ok(Val::Num(self.env.measured.unwrap_or(0.0))),
            Some(Tok::Name(n)) if n == "TRUE" => Ok(Val::Bool(true)),
            Some(Tok::Name(n)) if n == "FALSE" => Ok(Val::Bool(false)),
            Some(Tok::Name(n)) => {
                if self.peek() != Some(&Tok::LParen) {
                    return Err(Err::Name);
                }
                self.i += 1;
                // Each argument's value or the error working it out gave: IF uses only the branch
                // it picks, so an error in the other one (`=IF(TRUE;1;1/0)`) doesn't count.
                let mut args: Vec<R> = Vec::new();
                if self.peek() != Some(&Tok::RParen) {
                    loop {
                        let end = self.arg_end().ok_or(Err::Syntax)?;
                        let depth = self.depth;
                        let v = self.compare();
                        self.depth = depth;
                        match v {
                            Err(Err::Syntax) => return Err(Err::Syntax),
                            Ok(_) if self.i != end => return Err(Err::Syntax),
                            _ => {}
                        }
                        self.i = end;
                        args.push(v);
                        if args.len() > MAX_ARGS {
                            return Err(Err::Value);
                        }
                        match self.peek() {
                            Some(Tok::Comma) => self.i += 1,
                            Some(Tok::RParen) => break,
                            _ => return Err(Err::Syntax),
                        }
                    }
                }
                self.i += 1;
                if n == "IF" {
                    let mut it = args.into_iter();
                    let c = it.next().unwrap_or(Err(Err::Value))?.truth()?;
                    let (yes, no) = (it.next(), it.next());
                    if c { yes.unwrap_or(Ok(Val::Bool(true))) } else { no.unwrap_or(Ok(Val::Bool(false))) }
                } else {
                    let args: Vec<Val> = args.into_iter().collect::<std::result::Result<_, _>>()?;
                    call(&n, &args, self.env)
                }
            }
            _ => Err(Err::Syntax),
        };
        self.depth -= 1;
        r
    }
}

fn round_to(x: f64, n: f64, how: fn(f64) -> f64) -> f64 {
    let k = 10f64.powi(n.clamp(-15.0, 15.0) as i32);
    how(x * k) / k
}

fn arg(a: &[Val], i: usize) -> std::result::Result<&Val, Err> {
    a.get(i).ok_or(Err::Value)
}

fn nums(a: &[Val]) -> std::result::Result<Vec<f64>, Err> {
    a.iter().map(Val::num).collect()
}

fn call(name: &str, a: &[Val], env: &Env) -> R {
    let n0 = |a: &[Val]| arg(a, 0).and_then(Val::num);
    let n1 = |a: &[Val]| arg(a, 1).and_then(Val::num);
    let checked = |v: f64| if v.is_finite() { Ok(Val::Num(v)) } else { Err(Err::Num) };
    let chars = |v: &Val| v.text().chars().collect::<Vec<char>>();
    let count = |v: f64| if v.is_finite() { usize::try_from(v.clamp(0.0, 1e6) as i64).unwrap_or(0) } else { 0 };
    let now = env.now.map(|(y, mo, d, h, mi, s)| {
        serial(i64::from(y), i64::from(mo), i64::from(d)) + (f64::from(h) * 3600.0 + f64::from(mi) * 60.0 + f64::from(s)) / 86_400.0
    });
    let known = |t: &str| if t.is_empty() { Err(Err::Unknown) } else { Ok(Val::Text(t.to_string())) };
    match name {
        "ABS" => Ok(Val::Num(n0(a)?.abs())),
        "ACOS" => checked(n0(a)?.acos()),
        "ASIN" => checked(n0(a)?.asin()),
        "ATAN" => Ok(Val::Num(n0(a)?.atan())),
        "AND" => Ok(Val::Bool(a.iter().map(Val::truth).collect::<std::result::Result<Vec<_>, _>>()?.iter().all(|b| *b))),
        "OR" => Ok(Val::Bool(a.iter().map(Val::truth).collect::<std::result::Result<Vec<_>, _>>()?.iter().any(|b| *b))),
        "NOT" => Ok(Val::Bool(!arg(a, 0)?.truth()?)),
        "AVERAGE" => {
            let v = nums(a)?;
            if v.is_empty() { Err(Err::Div0) } else { Ok(Val::Num(v.iter().sum::<f64>() / v.len() as f64)) }
        }
        "SUM" => Ok(Val::Num(nums(a)?.iter().sum())),
        "MIN" => Ok(Val::Num(nums(a)?.into_iter().fold(f64::INFINITY, f64::min)).clone())
            .and_then(|v| if v.num()?.is_finite() { Ok(v) } else { Ok(Val::Num(0.0)) }),
        "MAX" => Ok(Val::Num(nums(a)?.into_iter().fold(f64::NEG_INFINITY, f64::max)))
            .and_then(|v| if v.num()?.is_finite() { Ok(v) } else { Ok(Val::Num(0.0)) }),
        "CONCAT" => Ok(Val::Text(a.iter().map(Val::text).collect())),
        "COS" => Ok(Val::Num(n0(a)?.cos())),
        "SIN" => Ok(Val::Num(n0(a)?.sin())),
        "TAN" => checked(n0(a)?.tan()),
        "DEGREES" => Ok(Val::Num(n0(a)?.to_degrees())),
        "RADIANS" => Ok(Val::Num(n0(a)?.to_radians())),
        "EXP" => checked(n0(a)?.exp()),
        "LN" => checked(n0(a)?.ln()),
        "LOG10" => checked(n0(a)?.log10()),
        "SQRT" => checked(n0(a)?.sqrt()),
        "POWER" => checked(n0(a)?.powf(n1(a)?)),
        "MOD" => {
            let (x, m) = (n0(a)?, n1(a)?);
            if m == 0.0 { Err(Err::Div0) } else { Ok(Val::Num(x - m * (x / m).floor())) }
        }
        "INT" => Ok(Val::Num(n0(a)?.floor())),
        "PI" => Ok(Val::Num(std::f64::consts::PI)),
        "ROUND" => Ok(Val::Num(round_to(n0(a)?, n1(a).unwrap_or(0.0), f64::round))),
        "ROUNDUP" => Ok(Val::Num(round_to(n0(a)?, n1(a).unwrap_or(0.0), |x| if x < 0.0 { x.floor() } else { x.ceil() }))),
        "ROUNDDOWN" => Ok(Val::Num(round_to(n0(a)?, n1(a).unwrap_or(0.0), f64::trunc))),
        "FIXED" => Ok(Val::Text(format!("{:.*}", count(n1(a).unwrap_or(2.0)).min(15), n0(a)?))),
        "IF" => {
            let c = arg(a, 0)?.truth()?;
            Ok(if c { a.get(1).cloned().unwrap_or(Val::Bool(true)) } else { a.get(2).cloned().unwrap_or(Val::Bool(false)) })
        }
        "LEN" => Ok(Val::Num(chars(arg(a, 0)?).len() as f64)),
        "UPPER" => Ok(Val::Text(arg(a, 0)?.text().to_uppercase())),
        "LOWER" => Ok(Val::Text(arg(a, 0)?.text().to_lowercase())),
        "TRIM" => Ok(Val::Text(arg(a, 0)?.text().split_whitespace().collect::<Vec<_>>().join(" "))),
        "LEFT" => Ok(Val::Text(chars(arg(a, 0)?).into_iter().take(count(n1(a).unwrap_or(1.0))).collect())),
        "RIGHT" => {
            let c = chars(arg(a, 0)?);
            let k = count(n1(a).unwrap_or(1.0)).min(c.len());
            Ok(Val::Text(c.get(c.len() - k..).unwrap_or_default().iter().collect()))
        }
        "MID" => {
            let c = chars(arg(a, 0)?);
            let st = count(n1(a)? - 1.0);
            let k = count(arg(a, 2)?.num()?);
            Ok(Val::Text(c.into_iter().skip(st).take(k).collect()))
        }
        "TEXT" => {
            let v = arg(a, 0)?;
            let f = arg(a, 1)?.text();
            Ok(Val::Text(match v {
                Val::Date(x) => format_date(*x, &f),
                _ if looks_like_date_format(&f) => format_date(v.num()?, &f),
                Val::Text(t) if t.trim().parse::<f64>().is_err() => t.clone(),
                _ => format_number(v.num()?, &f),
            }))
        }
        "TODAY" => Ok(Val::Date(now.ok_or(Err::Unknown)?.floor())),
        "NOW" => Ok(Val::Date(now.ok_or(Err::Unknown)?)),
        "DATE" => {
            let (yy, mm, dd) = (n0(a)?, n1(a)?, arg(a, 2)?.num()?);
            if !(yy.is_finite() && mm.is_finite() && dd.is_finite()) || yy.abs() > 9999.0 || mm.abs() > 12_000.0 || dd.abs() > 1e6 {
                return Err(Err::Num);
            }
            Ok(Val::Date(serial(yy as i64, mm as i64, dd as i64)))
        }
        "YEAR" => Ok(Val::Num(parts(n0(a)?).0 as f64)),
        "MONTH" => Ok(Val::Num(parts(n0(a)?).1 as f64)),
        "DAY" => Ok(Val::Num(parts(n0(a)?).2 as f64)),
        "HOUR" => Ok(Val::Num(parts(n0(a)?).3 as f64)),
        "MINUTE" => Ok(Val::Num(parts(n0(a)?).4 as f64)),
        "FILENAME" => known(&env.file),
        "SHEET" => known(&env.sheet),
        _ => Err(Err::Name),
    }
}

// ---------- AutoCAD fields ----------

/// The fields of a field template worked out (`%<\AcVar date \f "dd.MM.yyyy">%` and the file
/// name ones); `None` when one is a field CADCraft doesn't know.
fn fields(template: &str, env: &Env) -> Option<String> {
    let mut out = String::new();
    let mut rest = template;
    let mut guard = 0;
    while let Some(a) = rest.find("%<") {
        guard += 1;
        if guard > 64 {
            return None;
        }
        out.push_str(rest.get(..a)?);
        let tail = rest.get(a + 2..)?;
        let b = tail.find(">%")?;
        out.push_str(&field(tail.get(..b)?, env)?);
        rest = tail.get(b + 2..)?;
    }
    out.push_str(rest);
    Some(out)
}

/// One field code (between `%<` and `>%`).
fn field(code: &str, env: &Env) -> Option<String> {
    let code = code.trim();
    let body = code.strip_prefix("\\AcVar")?.trim();
    let (name, fmt) = match body.split_once("\\f") {
        Some((n, f)) => (n.trim(), Some(f.trim().trim_matches('"'))),
        None => (body, None),
    };
    let now = env.now.map(|(y, mo, d, h, mi, s)| {
        serial(i64::from(y), i64::from(mo), i64::from(d)) + (f64::from(h) * 3600.0 + f64::from(mi) * 60.0 + f64::from(s)) / 86_400.0
    });
    match name.to_ascii_lowercase().as_str() {
        // AutoCAD's date formats are .NET's: MM month, mm minutes, HH hours. Without a clock the
        // field is not known here.
        "date" => Some(format_date(now?, &net_to_sheet(fmt.unwrap_or("dd.MM.yyyy")))),
        "filename" => Some(env.file.clone()).filter(|f| !f.is_empty()),
        _ => None,
    }
}

/// A .NET date format (`dd.MM.yyyy HH:mm`) as a spreadsheet one (`dd.mm.yyyy hh:mm`).
fn net_to_sheet(f: &str) -> String {
    // .NET: M month, m minute; spreadsheet: m month, m after h minute. Minutes become "mm" after
    // an hour, months stay "mm" otherwise.
    f.replace("HH", "hh").replace('H', "h").replace("MMMM", "mmmm").replace("MMM", "mmm").replace("MM", "mm").replace('M', "m")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Env {
        Env { now: Some((2026, 10, 23, 14, 5, 9)), file: "plan.dwg".into(), sheet: "4".into(), measured: None }
    }

    fn ev(f: &str) -> String {
        evaluate(f, &env()).unwrap_or_default()
    }

    /// Without a clock (the web build) or a file and sheet name (drawing a dimension), date,
    /// FILENAME() and SHEET() formulas and date fields keep their saved result instead of
    /// showing a made-up one (#2318 review).
    #[test]
    fn unknown_clock_or_names_keep_the_saved_result() {
        let blind = Env::default();
        assert_eq!(evaluate("=TODAY()", &blind), None);
        assert_eq!(evaluate("=FILENAME()", &blind), None);
        assert_eq!(evaluate("=SHEET()", &blind), None);
        assert_eq!(evaluate("%<\\AcVar date \\f \"dd.MM.yyyy\">%", &blind), None);
        assert_eq!(evaluate("=1+1", &blind).as_deref(), Some("2"));
        assert_eq!(ev("=FILENAME()"), "plan.dwg");
    }

    /// A long chain of `^` is refused at the nesting limit, not run into deep recursion (#2318
    /// review); quoted text in a number format doesn't make it a date format.
    #[test]
    fn power_chains_are_bounded_and_quoted_words_are_not_dates() {
        // Longer than any formula may be: refused.
        assert_eq!(ev(&format!("={}2", "2^".repeat(5000))), "#ERROR!");
        // A long chain is no deep nesting: worked out (here too big), no stack used up.
        assert_eq!(ev(&format!("={}2", "2^".repeat(900))), "#NUM!");
        assert_eq!(ev(&format!("={}1", "1^".repeat(900))), "1");
        // Left to right, as in a spreadsheet.
        assert_eq!(ev("=2^3^2"), "64");
        assert_eq!(ev("=-2^2"), "4");
        assert!(!looks_like_date_format("0.0\" days\""));
        assert!(!looks_like_date_format("0 \"thick\""));
        assert!(looks_like_date_format("dd.mm.yyyy"));
        assert!(looks_like_date_format("hh:mm"));
    }

    /// Huge or infinite dates never overflow (a crash in a test build, nonsense otherwise).
    #[test]
    fn huge_date_serials_do_not_overflow() {
        for f in ["=YEAR(1E300)", "=YEAR(-1E300)", "=TEXT(1E300,\"yyyy\")", "=TODAY()+1E300", "=MONTH(1E308*10)"] {
            let _ = ev(f);
        }
        assert_eq!(ev("=YEAR(45000)"), "2023");
    }

    /// IF works out only the branch it picks: an error in the other one is not its result
    /// (#2034 review). Errors in the condition or the picked branch still show.
    #[test]
    fn if_uses_only_the_branch_it_picks() {
        assert_eq!(ev("=IF(TRUE,1,1/0)"), "1");
        assert_eq!(ev("=IF(FALSE,1/0,2)"), "2");
        assert_eq!(ev("=IF(1<2;(1/0)+5;3)"), "#DIV/0!");
        assert_eq!(ev("=IF(1/0,1,2)"), "#DIV/0!");
        assert_eq!(ev("=IF(FALSE,SQRT(-1)*2,IF(TRUE,7,1/0))"), "7");
        assert_eq!(ev("=SUM(1,1/0)"), "#DIV/0!", "other functions still need every argument");
        assert_eq!(ev("=IF(TRUE,1 2,3)"), "#ERROR!", "a malformed branch is still an error");
    }

    #[test]
    fn arithmetic_and_functions() {
        assert_eq!(ev("=2400-2*18"), "2364");
        assert_eq!(ev("=(1+2)^2/4"), "2.25");
        assert_eq!(ev("=ROUND(PI()*2,3)"), "6.283");
        assert_eq!(ev("=SUM(1,2,3)&\" mm\""), "6 mm");
        assert_eq!(ev("=IF(5>3,\"big\",\"small\")"), "big");
        assert_eq!(ev("=MAX(4;9;2)"), "9");
        assert_eq!(ev("=TEXT(1234.5,\"#,##0.00\")"), "1,234.50");
        assert_eq!(ev("=TEXT(0.25,\"0%\")"), "25%");
        assert_eq!(ev("=UPPER(LEFT(\"abc\",2))"), "AB");
        assert_eq!(ev("=MID(\"Hello\",2,3)"), "ell");
        assert_eq!(ev("=50%*10"), "5");
    }

    #[test]
    fn dates_follow_the_clock() {
        assert_eq!(ev("=TODAY()"), "23.10.2026");
        assert_eq!(ev("=TEXT(TODAY(),\"yyyy-mm-dd\")"), "2026-10-23");
        assert_eq!(ev("=TEXT(NOW(),\"hh:mm\")"), "14:05");
        assert_eq!(ev("=TODAY()+10"), "02.11.2026");
        assert_eq!(ev("=YEAR(DATE(2025,14,1))"), "2026");
        assert_eq!(ev("=TEXT(DATE(2026,1,5),\"d mmm yyyy\")"), "5 Jan 2026");
        assert_eq!(ev("%<\\AcVar date \\f \"dd.MM.yyyy\">%"), "23.10.2026");
        assert_eq!(ev("Date: %<\\AcVar Date \\f \"yyyy-MM-dd HH:mm\">%"), "Date: 2026-10-23 14:05");
        assert_eq!(ev("=FILENAME()&\" / \"&SHEET()"), "plan.dwg / 4");
        assert_eq!(evaluate("%<\\AcObjProp Object(%<\\_ObjId 1>%).Area>%", &env()), None, "unknown fields keep their value");
    }

    #[test]
    fn a_dimension_formula_uses_its_measurement() {
        let e = Env { measured: Some(1250.0), ..env() };
        assert_eq!(evaluate("=<>/2", &e).as_deref(), Some("625"));
        assert_eq!(evaluate("=TEXT(<>+30,\"0.0\")&\" mm\"", &e).as_deref(), Some("1280.0 mm"));
        assert_eq!(evaluate("=IF(<><>0,1,2)", &e).as_deref(), Some("1"));
        assert!(!is_formula("=1+1\nsecond line"));
    }

    #[test]
    fn errors_show_as_in_spreadsheets_and_never_panic() {
        assert_eq!(ev("=1/0"), "#DIV/0!");
        assert_eq!(ev("=FOO(1)"), "#NAME?");
        assert_eq!(ev("=\"a\"*2"), "#VALUE!");
        assert_eq!(ev("=SQRT(-1)"), "#NUM!");
        assert_eq!(ev("=(1+"), "#ERROR!");
        for f in [
            "=",
            "==",
            "=(((((",
            &format!("={}", "-".repeat(500)),
            &format!("={}1", "(".repeat(100)),
            "=\"",
            "=MID(\"a\",-5,1e300)",
            "=DATE(1e300,1,1)",
        ] {
            let _ = evaluate(f, &env());
        }
        assert!(!is_formula("plain"));
        assert!(!is_formula("="));
    }
}
