//! The CAL / QuickCalc evaluator: numbers and points (`[x,y]`, `[d<a]`), arithmetic and vector
//! arithmetic, numeric and geometric functions; unit conversion; values picked from the drawing.

use cadcraft_geom::Vec2;
use serde_json::{Map, Value, json};

/// Longest expression accepted.
const MAX_LEN: usize = 10_000;
/// Deepest nesting of parentheses, brackets and function calls.
const MAX_DEPTH: usize = 200;
/// Most arguments a function takes.
const MAX_ARGS: usize = 8;

/// A CAL value: a number or a point (vector).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Val {
    Num(f64),
    Pt(Vec2),
}

impl Val {
    pub fn is_finite(self) -> bool {
        match self {
            Val::Num(v) => v.is_finite(),
            Val::Pt(p) => p.is_finite(),
        }
    }
    /// The value as the command line shows it: a number, or `x,y`.
    pub fn text(self) -> String {
        match self {
            Val::Num(v) => num_text(v),
            Val::Pt(p) => format!("{},{}", num_text(p.x), num_text(p.y)),
        }
    }
    /// `{value: n}` or `{point: [x, y]}`.
    pub fn json(self) -> Value {
        match self {
            Val::Num(v) => json!({ "value": v }),
            Val::Pt(p) => json!({ "point": [p.x, p.y] }),
        }
    }
    fn num(self, what: &str) -> Result<f64, String> {
        match self {
            Val::Num(v) => Ok(v),
            Val::Pt(_) => Err(format!("{what} needs a number, not a point")),
        }
    }
    fn pt(self, what: &str) -> Result<Vec2, String> {
        match self {
            Val::Pt(p) => Ok(p),
            Val::Num(_) => Err(format!("{what} needs a point, not a number")),
        }
    }
}

/// A number with at most ten decimals and no trailing zeros.
pub fn num_text(v: f64) -> String {
    if v != 0.0 && (v.abs() >= 1e15 || v.abs() < 1e-6) {
        return format!("{v:e}");
    }
    let s = format!("{v:.10}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    vars: &'a Map<String, Value>,
    depth: usize,
    unary_depth: usize,
}

impl Parser<'_> {
    fn peek(&mut self) -> Option<u8> {
        while self.s.get(self.i).is_some_and(u8::is_ascii_whitespace) {
            self.i += 1;
        }
        self.s.get(self.i).copied()
    }
    fn eat(&mut self, c: u8) -> bool {
        let hit = self.peek() == Some(c);
        if hit {
            self.i += 1;
        }
        hit
    }
    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.eat(c) { Ok(()) } else { Err(format!("missing {}", c as char)) }
    }
    fn expr(&mut self) -> Result<Val, String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err("expression too deeply nested".into());
        }
        let mut v = self.term()?;
        loop {
            if self.eat(b'+') {
                v = add(v, self.term()?, 1.0)?;
            } else if self.eat(b'-') {
                v = add(v, self.term()?, -1.0)?;
            } else {
                break;
            }
        }
        self.depth -= 1;
        Ok(v)
    }
    fn term(&mut self) -> Result<Val, String> {
        let mut v = self.unary()?;
        loop {
            if self.eat(b'*') {
                v = mul(v, self.unary()?)?;
            } else if self.eat(b'/') {
                v = div(v, self.unary()?)?;
            } else if self.eat(b'&') {
                let (a, b) = (v.pt("&")?, self.unary()?.pt("&")?);
                v = Val::Num(a.cross(b));
            } else {
                break;
            }
        }
        Ok(v)
    }
    /// A unary operand reached by a leading `-`/`+` or after `^`. These recurse without passing
    /// through `expr`, so they are bounded by their own counter (`unary_depth`), leaving the
    /// parenthesis/bracket limit in `depth` untouched.
    fn operand(&mut self) -> Result<Val, String> {
        if self.unary_depth >= MAX_DEPTH {
            return Err("expression too deeply nested".into());
        }
        self.unary_depth += 1;
        let v = self.unary();
        self.unary_depth -= 1;
        v
    }
    fn unary(&mut self) -> Result<Val, String> {
        if self.eat(b'-') {
            return Ok(match self.operand()? {
                Val::Num(v) => Val::Num(-v),
                Val::Pt(p) => Val::Pt(Vec2::ZERO - p),
            });
        }
        if self.eat(b'+') {
            return self.operand();
        }
        let b = self.atom()?;
        if self.eat(b'^') {
            let e = self.operand()?.num("^")?;
            return Ok(Val::Num(b.num("^")?.powf(e)));
        }
        Ok(b)
    }
    fn number(&mut self) -> Result<f64, String> {
        let st = self.i;
        let digits = |p: &Self, i: usize| p.s.get(i).is_some_and(|c| c.is_ascii_digit() || *c == b'.');
        while digits(self, self.i) {
            self.i += 1;
        }
        // An exponent: `e`/`E`, an optional sign, digits.
        if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
            let mut j = self.i + 1;
            if matches!(self.s.get(j), Some(b'+' | b'-')) {
                j += 1;
            }
            if self.s.get(j).is_some_and(u8::is_ascii_digit) {
                self.i = j;
                while self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
                    self.i += 1;
                }
            }
        }
        let t = std::str::from_utf8(self.s.get(st..self.i).unwrap_or(&[])).unwrap_or("");
        t.parse::<f64>().map_err(|_| format!("bad number `{t}`"))
    }
    fn atom(&mut self) -> Result<Val, String> {
        match self.peek() {
            Some(b'(') => {
                self.i += 1;
                let v = self.expr()?;
                self.expect(b')')?;
                Ok(v)
            }
            Some(b'[') => {
                self.i += 1;
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return Err("expression too deeply nested".into());
                }
                let a = self.expr()?.num("a point coordinate")?;
                let p = if self.eat(b'<') {
                    let ang = self.expr()?.num("a polar angle")?.to_radians();
                    Vec2::new(a * ang.cos(), a * ang.sin())
                } else {
                    self.expect(b',')?;
                    let b = self.expr()?.num("a point coordinate")?;
                    // A Z coordinate is accepted and ignored (the drawing is 2D).
                    if self.eat(b',') {
                        self.expr()?.num("a point coordinate")?;
                    }
                    Vec2::new(a, b)
                };
                self.expect(b']')?;
                self.depth -= 1;
                Ok(Val::Pt(p))
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => self.number().map(Val::Num),
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                let st = self.i;
                while self.s.get(self.i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
                    self.i += 1;
                }
                let name = std::str::from_utf8(self.s.get(st..self.i).unwrap_or(&[])).unwrap_or("").to_ascii_lowercase();
                if self.eat(b'(') {
                    let mut args = Vec::new();
                    if !self.eat(b')') {
                        loop {
                            if args.len() >= MAX_ARGS {
                                return Err(format!("too many arguments to `{name}`"));
                            }
                            args.push(self.expr()?);
                            if self.eat(b')') {
                                break;
                            }
                            self.expect(b',')?;
                        }
                    }
                    return call(&name, &args);
                }
                match name.as_str() {
                    "pi" => Ok(Val::Num(std::f64::consts::PI)),
                    "e" => Ok(Val::Num(std::f64::consts::E)),
                    _ => var(self.vars, &name).ok_or_else(|| format!("unknown name `{name}`")),
                }
            }
            _ => Err("expected a number".into()),
        }
    }
}

/// A variable: a number, or a point as `[x, y]`. Names are case-insensitive.
fn var(vars: &Map<String, Value>, name: &str) -> Option<Val> {
    let v = vars.get(name).or_else(|| vars.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v))?;
    if let Some(n) = v.as_f64() {
        return Some(Val::Num(n));
    }
    let a = v.as_array()?;
    let (x, y) = (a.first()?.as_f64()?, a.get(1)?.as_f64()?);
    Some(Val::Pt(Vec2::new(x, y)))
}

fn add(a: Val, b: Val, sign: f64) -> Result<Val, String> {
    match (a, b) {
        (Val::Num(a), Val::Num(b)) => Ok(Val::Num(a + sign * b)),
        (Val::Pt(a), Val::Pt(b)) => Ok(Val::Pt(a + b * sign)),
        _ => Err("can't add or subtract a number and a point".into()),
    }
}

fn mul(a: Val, b: Val) -> Result<Val, String> {
    Ok(match (a, b) {
        (Val::Num(a), Val::Num(b)) => Val::Num(a * b),
        (Val::Num(k), Val::Pt(p)) | (Val::Pt(p), Val::Num(k)) => Val::Pt(p * k),
        // Two vectors: the scalar (dot) product.
        (Val::Pt(a), Val::Pt(b)) => Val::Num(a.dot(b)),
    })
}

fn div(a: Val, b: Val) -> Result<Val, String> {
    let d = b.num("/")?;
    if d == 0.0 {
        return Err("division by zero".into());
    }
    Ok(match a {
        Val::Num(a) => Val::Num(a / d),
        Val::Pt(p) => Val::Pt(p * (1.0 / d)),
    })
}

/// Angle of `v` in degrees, 0..360.
fn deg(v: Vec2) -> f64 {
    v.y.atan2(v.x).to_degrees().rem_euclid(360.0)
}

/// Where the lines through `a`,`b` and `c`,`d` cross.
fn intersection(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Result<Vec2, String> {
    let (r, s) = (b - a, d - c);
    let den = r.cross(s);
    if den.abs() <= 1e-12 * r.len().max(1e-300) * s.len().max(1e-300) {
        return Err("the lines are parallel".into());
    }
    Ok(a + r * ((c - a).cross(s) / den))
}

fn call(name: &str, args: &[Val]) -> Result<Val, String> {
    let n = |i: usize| args.get(i).copied().ok_or_else(|| format!("`{name}` needs more arguments")).and_then(|v| v.num(name));
    let p = |i: usize| args.get(i).copied().ok_or_else(|| format!("`{name}` needs more arguments")).and_then(|v| v.pt(name));
    let arity = |lo: usize, hi: usize| {
        if (lo..=hi).contains(&args.len()) {
            Ok(())
        } else {
            Err(format!("`{name}` takes {lo}{} argument(s)", if hi > lo { format!("–{hi}") } else { String::new() }))
        }
    };
    let num1 = |f: fn(f64) -> f64| -> Result<Val, String> {
        arity(1, 1)?;
        Ok(Val::Num(f(n(0)?)))
    };
    match name {
        "sin" => num1(|a| a.to_radians().sin()),
        "cos" => num1(|a| a.to_radians().cos()),
        "tan" => num1(|a| a.to_radians().tan()),
        "asin" => num1(|a| a.asin().to_degrees()),
        "acos" => num1(|a| a.acos().to_degrees()),
        "atan" => {
            arity(1, 2)?;
            Ok(Val::Num(if args.len() == 2 { n(0)?.atan2(n(1)?) } else { n(0)?.atan() }.to_degrees()))
        }
        "sqrt" => num1(f64::sqrt),
        "sqr" => num1(|a| a * a),
        "ln" => num1(f64::ln),
        "log" => num1(f64::log10),
        "exp" => num1(f64::exp),
        "exp10" => num1(|a| 10f64.powf(a)),
        "round" => num1(f64::round),
        "trunc" => num1(f64::trunc),
        "r2d" => num1(f64::to_degrees),
        "d2r" => num1(f64::to_radians),
        "pow" => {
            arity(2, 2)?;
            Ok(Val::Num(n(0)?.powf(n(1)?)))
        }
        "min" | "max" => {
            arity(1, MAX_ARGS)?;
            let vs = (0..args.len()).map(n).collect::<Result<Vec<_>, _>>()?;
            let f = if name == "min" { f64::min } else { f64::max };
            Ok(Val::Num(vs.into_iter().reduce(f).unwrap_or(0.0)))
        }
        // The absolute value of a number; the length of a vector.
        "abs" => {
            arity(1, 1)?;
            Ok(Val::Num(match args.first() {
                Some(Val::Pt(v)) => v.len(),
                _ => n(0)?.abs(),
            }))
        }
        "dist" => {
            arity(2, 2)?;
            Ok(Val::Num(p(0)?.dist(p(1)?)))
        }
        // ang(v): the vector's angle; ang(p1, p2): the line's; ang(apex, p1, p2): the angle at apex.
        "ang" => {
            arity(1, 3)?;
            Ok(Val::Num(match args.len() {
                1 => deg(p(0)?),
                2 => deg(p(1)? - p(0)?),
                _ => {
                    let a = deg(p(2)? - p(0)?) - deg(p(1)? - p(0)?);
                    a.rem_euclid(360.0)
                }
            }))
        }
        "vec" => {
            arity(2, 2)?;
            Ok(Val::Pt(p(1)? - p(0)?))
        }
        "vec1" => {
            arity(2, 2)?;
            let v = p(1)? - p(0)?;
            if v.len() == 0.0 {
                return Err("vec1 of two equal points".into());
            }
            Ok(Val::Pt(v * (1.0 / v.len())))
        }
        "ill" => {
            arity(4, 4)?;
            intersection(p(0)?, p(1)?, p(2)?, p(3)?).map(Val::Pt)
        }
        // The point at parameter t (0 at p1, 1 at p2) / at distance d from p1 towards p2.
        "plt" => {
            arity(3, 3)?;
            let (a, b) = (p(0)?, p(1)?);
            Ok(Val::Pt(a + (b - a) * n(2)?))
        }
        "pld" => {
            arity(3, 3)?;
            let (a, b) = (p(0)?, p(1)?);
            let l = a.dist(b);
            if l == 0.0 {
                return Err("pld of two equal points".into());
            }
            Ok(Val::Pt(a + (b - a) * (n(2)? / l)))
        }
        // rot(p, origin, angle): p rotated about origin.
        "rot" => {
            arity(3, 3)?;
            let (q, o, a) = (p(0)?, p(1)?, n(2)?.to_radians());
            let d = q - o;
            Ok(Val::Pt(o + Vec2::new(d.x * a.cos() - d.y * a.sin(), d.x * a.sin() + d.y * a.cos())))
        }
        _ => Err(format!("unknown function `{name}`")),
    }
}

/// Evaluate an expression: numbers, points `[x,y]` / `[d<a]`, `+ - * / ^ &`, functions, `pi`, `e`
/// and the given variables (numbers or `[x, y]`). Degrees for trigonometry.
pub fn eval_val(expr: &str, vars: &Map<String, Value>) -> Result<Val, String> {
    if expr.len() > MAX_LEN {
        return Err("expression too long".into());
    }
    let mut p = Parser { s: expr.as_bytes(), i: 0, vars, depth: 0, unary_depth: 0 };
    let v = p.expr()?;
    if p.peek().is_some() {
        return Err(format!("unexpected `{}`", expr.get(p.i..).unwrap_or("")));
    }
    if !v.is_finite() {
        return Err("result is not a finite number".into());
    }
    Ok(v)
}

// ---------------- units ----------------

/// Unit tables: (kind, [(id, symbol, size in the base unit)]). Base units: metre, square metre,
/// cubic metre, radian.
const UNITS: &[(&str, &[(&str, &str, f64)])] = &[
    (
        "length",
        &[
            ("um", "µm", 1e-6),
            ("mm", "mm", 1e-3),
            ("cm", "cm", 1e-2),
            ("dm", "dm", 0.1),
            ("m", "m", 1.0),
            ("km", "km", 1e3),
            ("in", "in", 0.0254),
            ("ft", "ft", 0.3048),
            ("yd", "yd", 0.9144),
            ("mi", "mi", 1609.344),
            ("nmi", "nmi", 1852.0),
        ],
    ),
    (
        "area",
        &[
            ("mm2", "mm²", 1e-6),
            ("cm2", "cm²", 1e-4),
            ("m2", "m²", 1.0),
            ("ha", "ha", 1e4),
            ("km2", "km²", 1e6),
            ("in2", "in²", 0.00064516),
            ("ft2", "ft²", 0.09290304),
            ("yd2", "yd²", 0.83612736),
            ("acre", "acre", 4046.8564224),
            ("mi2", "mi²", 2589988.110336),
        ],
    ),
    (
        "volume",
        &[
            ("mm3", "mm³", 1e-9),
            ("cm3", "cm³", 1e-6),
            ("ml", "ml", 1e-6),
            ("l", "l", 1e-3),
            ("m3", "m³", 1.0),
            ("in3", "in³", 1.6387064e-5),
            ("ft3", "ft³", 0.028316846592),
            ("yd3", "yd³", 0.764554857984),
            ("galus", "gal (US)", 3.785411784e-3),
            ("galuk", "gal (UK)", 4.54609e-3),
        ],
    ),
    (
        "angle",
        &[
            ("deg", "°", std::f64::consts::PI / 180.0),
            ("rad", "rad", 1.0),
            ("grad", "grad", std::f64::consts::PI / 200.0),
            ("rev", "rev", std::f64::consts::TAU),
        ],
    ),
];

/// The unit tables as JSON: `{length: [{id, symbol}], area: …, volume: …, angle: …}`.
pub fn units_json() -> Value {
    let mut m = Map::new();
    for (kind, units) in UNITS {
        m.insert((*kind).into(), units.iter().map(|(id, sym, _)| json!({ "id": id, "symbol": sym })).collect());
    }
    Value::Object(m)
}

/// Convert `v` between two units of one kind (ids or symbols, case-insensitive). With no kind,
/// the kind holding both units.
pub fn convert(kind: Option<&str>, from: &str, to: &str, v: f64) -> Result<f64, String> {
    let find = |units: &[(&str, &str, f64)], u: &str| {
        let u = u.trim();
        units.iter().find(|(id, sym, _)| id.eq_ignore_ascii_case(u) || *sym == u).map(|(_, _, f)| *f)
    };
    for (k, units) in UNITS {
        if kind.is_some_and(|kind| !kind.eq_ignore_ascii_case(k)) {
            continue;
        }
        if let (Some(a), Some(b)) = (find(units, from), find(units, to)) {
            let r = significant(v * a / b);
            return if r.is_finite() { Ok(r) } else { Err("result is not a finite number".into()) };
        }
    }
    match kind {
        Some(k) if !UNITS.iter().any(|(n, _)| n.eq_ignore_ascii_case(k)) => {
            Err(format!("unit type must be length, area, volume or angle, got `{k}`"))
        }
        _ => Err(format!("can't convert `{from}` to `{to}`")),
    }
}

/// `v` to 14 significant digits: unit factors are exact decimals, so this drops the binary
/// rounding noise of the division (2 ft is 24 in, not 24.000000000000004).
fn significant(v: f64) -> f64 {
    if v == 0.0 || !v.is_finite() {
        return v;
    }
    let k = 13 - v.abs().log10().floor() as i32;
    if !(-300..=300).contains(&k) {
        return v;
    }
    let m = 10f64.powi(k);
    let r = (v * m).round() / m;
    if r.is_finite() { r } else { v }
}

// ---------------- values from the drawing ----------------

/// What QuickCalc reads from picked points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// One point: its coordinates.
    Point,
    /// Two points: the distance between them.
    Distance,
    /// Two points: the angle of the line through them.
    Angle,
    /// Four points: where the two lines through them cross.
    Intersection,
}

impl Pick {
    pub fn parse(s: &str) -> Option<Pick> {
        match s.trim().to_ascii_lowercase().as_str() {
            "point" | "p" | "coordinates" => Some(Pick::Point),
            "distance" | "d" => Some(Pick::Distance),
            "angle" | "a" => Some(Pick::Angle),
            "intersection" | "i" => Some(Pick::Intersection),
            _ => None,
        }
    }
    pub fn points(self) -> usize {
        match self {
            Pick::Point => 1,
            Pick::Distance | Pick::Angle => 2,
            Pick::Intersection => 4,
        }
    }
    /// The value read from exactly [`Pick::points`] points.
    pub fn value(self, pts: &[Vec2]) -> Result<Val, String> {
        let p = |i: usize| pts.get(i).copied().filter(|p| p.is_finite()).ok_or_else(|| format!("{} point(s) are needed", self.points()));
        if pts.len() != self.points() {
            return Err(format!("{} point(s) are needed", self.points()));
        }
        Ok(match self {
            Pick::Point => Val::Pt(p(0)?),
            Pick::Distance => Val::Num(p(0)?.dist(p(1)?)),
            Pick::Angle => Val::Num(deg(p(1)? - p(0)?)),
            Pick::Intersection => Val::Pt(intersection(p(0)?, p(1)?, p(2)?, p(3)?)?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str) -> Result<Val, String> {
        eval_val(s, &Map::new())
    }

    #[test]
    fn points_and_vectors() {
        assert_eq!(ev("[1,2]+[3,4]*2"), Ok(Val::Pt(Vec2::new(7.0, 10.0))));
        assert_eq!(ev("dist([0,0],[3,4])"), Ok(Val::Num(5.0)));
        assert_eq!(ev("[1,0]*[0,1]"), Ok(Val::Num(0.0)));
        assert_eq!(ev("[1,0]&[0,1]"), Ok(Val::Num(1.0)));
        assert_eq!(ev("ill([0,0],[2,2],[0,2],[2,0])"), Ok(Val::Pt(Vec2::new(1.0, 1.0))));
        assert!(ev("ill([0,0],[1,0],[0,1],[1,1])").is_err());
        assert!((ev("ang([0,0],[1,1])").unwrap().num("").unwrap() - 45.0).abs() < 1e-12);
        assert!(ev("[1,2]+3").is_err());
        assert!(ev("[1").is_err());
        assert!(ev(&"[".repeat(5000)).is_err());
        assert_eq!(ev("min(3,1,2)"), Ok(Val::Num(1.0)));
        assert_eq!(ev("1e-3*2"), Ok(Val::Num(0.002)));
        assert!(ev("max(1,2,3,4,5,6,7,8,9)").is_err());
    }

    #[test]
    fn deep_unary_chain_does_not_overflow_the_stack() {
        assert!(ev(&format!("{}1", "-".repeat(600))).is_err());
        assert!(ev(&format!("{}1", "+".repeat(600))).is_err());
        assert!(ev(&format!("2{}", "^2".repeat(600))).is_err());
        // The size from the report: under MAX_LEN, but ~10,000 frames deep without a guard.
        assert!(ev(&format!("{}1", "-".repeat(9990))).is_err());
        assert!(ev(&format!("2{}", "^2".repeat(4990))).is_err());
        assert_eq!(ev("--1"), Ok(Val::Num(1.0)));
        assert_eq!(ev("2^3^2"), Ok(Val::Num(512.0)));
    }

    #[test]
    fn parenthesis_nesting_just_under_the_limit_still_parses() {
        let n = MAX_DEPTH - 1;
        assert_eq!(ev(&format!("{}1{}", "(".repeat(n), ")".repeat(n))), Ok(Val::Num(1.0)));
        assert!(ev(&format!("{}1{}", "(".repeat(n + 1), ")".repeat(n + 1))).is_err());
    }

    #[test]
    fn number_text() {
        assert_eq!(num_text(0.1 + 0.2), "0.3");
        assert_eq!(num_text(28.0), "28");
        assert_eq!(num_text(-0.0), "0");
        assert_eq!(Val::Pt(Vec2::new(1.5, -2.0)).text(), "1.5,-2");
    }
}
