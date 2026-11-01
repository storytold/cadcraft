//! A tiny expression language for dimensional constraints and user parameters: numbers,
//! parameter names, `+ - * /`, unary minus, parentheses and the constant `pi`.

use std::collections::BTreeMap;

/// Longest expression accepted (characters).
pub const MAX_LEN: usize = 1024;
/// Deepest syntax tree and combined expression/parameter evaluation path.
pub const MAX_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ExprError {
    #[error("syntax error: {0}")]
    Syntax(String),
    #[error("unknown parameter `{0}`")]
    Unknown(String),
    #[error("parameter `{0}` refers to itself")]
    Cycle(String),
    #[error("division by zero")]
    DivZero,
    #[error("expression too long or too deeply nested")]
    TooDeep,
    #[error("result is not a finite number")]
    NotFinite,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Num(f64),
    Var(String),
    Neg(Box<Expr>),
    Bin(char, Box<Expr>, Box<Expr>),
}

/// True for a valid parameter name: a letter or `_`, then letters, digits or `_`.
pub fn valid_name(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
        && c.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        && s.len() <= 64
        && !s.eq_ignore_ascii_case("pi")
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    depth: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(u8::is_ascii_whitespace) {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }
    fn enter(&mut self) -> Result<(), ExprError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { Err(ExprError::TooDeep) } else { Ok(()) }
    }
    fn sum(&mut self) -> Result<Expr, ExprError> {
        self.enter()?;
        let mut e = self.product()?;
        while let Some(c @ (b'+' | b'-')) = self.peek() {
            self.i += 1;
            let r = self.product()?;
            e = Expr::Bin(c as char, Box::new(e), Box::new(r));
        }
        self.depth -= 1;
        Ok(e)
    }
    fn product(&mut self) -> Result<Expr, ExprError> {
        let mut e = self.unary()?;
        while let Some(c @ (b'*' | b'/')) = self.peek() {
            self.i += 1;
            let r = self.unary()?;
            e = Expr::Bin(c as char, Box::new(e), Box::new(r));
        }
        Ok(e)
    }
    fn unary(&mut self) -> Result<Expr, ExprError> {
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                self.enter()?;
                let e = self.unary()?;
                self.depth -= 1;
                Ok(Expr::Neg(Box::new(e)))
            }
            Some(b'+') => {
                self.i += 1;
                self.enter()?;
                let e = self.unary();
                self.depth -= 1;
                e
            }
            _ => self.atom(),
        }
    }
    fn atom(&mut self) -> Result<Expr, ExprError> {
        match self.peek() {
            Some(b'(') => {
                self.i += 1;
                let e = self.sum()?;
                if self.peek() != Some(b')') {
                    return Err(ExprError::Syntax("missing `)`".into()));
                }
                self.i += 1;
                Ok(e)
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => {
                let start = self.i;
                while self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
                    self.i += 1;
                }
                // Exponent: 1e3, 2.5E-2.
                if self.s.get(self.i).is_some_and(|c| *c == b'e' || *c == b'E') {
                    let save = self.i;
                    self.i += 1;
                    if self.s.get(self.i).is_some_and(|c| *c == b'+' || *c == b'-') {
                        self.i += 1;
                    }
                    if self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
                        while self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
                            self.i += 1;
                        }
                    } else {
                        self.i = save;
                    }
                }
                let t = std::str::from_utf8(self.s.get(start..self.i).unwrap_or_default()).unwrap_or_default();
                let v: f64 = t.parse().map_err(|_| ExprError::Syntax(format!("bad number `{t}`")))?;
                Ok(Expr::Num(v))
            }
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                let start = self.i;
                while self.s.get(self.i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
                    self.i += 1;
                }
                let t = std::str::from_utf8(self.s.get(start..self.i).unwrap_or_default()).unwrap_or_default();
                if t.eq_ignore_ascii_case("pi") {
                    return Ok(Expr::Num(std::f64::consts::PI));
                }
                Ok(Expr::Var(t.to_string()))
            }
            Some(c) => Err(ExprError::Syntax(format!("unexpected `{}`", c as char))),
            None => Err(ExprError::Syntax("unexpected end".into())),
        }
    }
}

/// Parse an expression.
pub fn parse(text: &str) -> Result<Expr, ExprError> {
    if text.len() > MAX_LEN {
        return Err(ExprError::TooDeep);
    }
    if text.trim().is_empty() {
        return Err(ExprError::Syntax("empty expression".into()));
    }
    let mut p = Parser { s: text.as_bytes(), i: 0, depth: 0 };
    let e = p.sum()?;
    if p.peek().is_some() {
        return Err(ExprError::Syntax(format!("unexpected `{}`", text.get(p.i..).unwrap_or("").trim())));
    }
    let mut pending = vec![(&e, 1)];
    while let Some((node, depth)) = pending.pop() {
        if depth > MAX_DEPTH {
            return Err(ExprError::TooDeep);
        }
        match node {
            Expr::Neg(a) => pending.push((a, depth + 1)),
            Expr::Bin(_, a, b) => {
                pending.push((a, depth + 1));
                pending.push((b, depth + 1));
            }
            _ => {}
        }
    }
    Ok(e)
}

/// Names referenced by an expression.
pub fn names(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Num(_) => {}
        Expr::Var(n) => {
            if !out.contains(n) {
                out.push(n.clone());
            }
        }
        Expr::Neg(a) => names(a, out),
        Expr::Bin(_, a, b) => {
            names(a, out);
            names(b, out);
        }
    }
}

/// `text` with every whole-name use of `old` replaced by `new`; numbers (`1e3`), other names
/// (`d10` for `d1`) and spacing are left as written.
pub fn rename(text: &str, old: &str, new: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        let start = i;
        if c.is_ascii_digit() || c == b'.' {
            while b.get(i).is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
                i += 1;
            }
            if b.get(i).is_some_and(|c| *c == b'e' || *c == b'E') {
                let mut j = i + 1;
                if b.get(j).is_some_and(|c| *c == b'+' || *c == b'-') {
                    j += 1;
                }
                if b.get(j).is_some_and(u8::is_ascii_digit) {
                    i = j;
                    while b.get(i).is_some_and(u8::is_ascii_digit) {
                        i += 1;
                    }
                }
            }
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while b.get(i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
                i += 1;
            }
        } else {
            i += text.get(i..).and_then(|r| r.chars().next()).map_or(1, char::len_utf8);
        }
        let token = text.get(start..i).unwrap_or_default();
        out.push_str(if token == old { new } else { token });
    }
    out
}

/// Evaluate with a variable lookup.
pub fn eval_with(e: &Expr, lookup: &mut dyn FnMut(&str) -> Result<f64, ExprError>) -> Result<f64, ExprError> {
    eval_at_depth(e, &mut |n, _| lookup(n), 0)
}

fn eval_at_depth(e: &Expr, lookup: &mut dyn FnMut(&str, usize) -> Result<f64, ExprError>, depth: usize) -> Result<f64, ExprError> {
    if depth >= MAX_DEPTH {
        return Err(ExprError::TooDeep);
    }
    let v = match e {
        Expr::Num(v) => *v,
        Expr::Var(n) => lookup(n, depth + 1)?,
        Expr::Neg(a) => -eval_at_depth(a, lookup, depth + 1)?,
        Expr::Bin(op, a, b) => {
            let (x, y) = (eval_at_depth(a, lookup, depth + 1)?, eval_at_depth(b, lookup, depth + 1)?);
            match op {
                '+' => x + y,
                '-' => x - y,
                '*' => x * y,
                _ => {
                    if y == 0.0 {
                        return Err(ExprError::DivZero);
                    }
                    x / y
                }
            }
        }
    };
    if v.is_finite() { Ok(v) } else { Err(ExprError::NotFinite) }
}

/// Evaluate an expression that uses no names.
pub fn eval_const(text: &str) -> Result<f64, ExprError> {
    eval_with(&parse(text)?, &mut |n| Err(ExprError::Unknown(n.to_string())))
}

/// A set of named expressions (user parameters and dimensional constraint names), evaluated
/// lazily with cycle detection.
#[derive(Clone, Debug, Default)]
pub struct Env {
    exprs: BTreeMap<String, String>,
    cache: BTreeMap<String, f64>,
}

impl Env {
    pub fn new() -> Self {
        Env::default()
    }
    pub fn insert(&mut self, name: &str, expr: &str) {
        self.exprs.insert(name.to_string(), expr.to_string());
        self.cache.clear();
    }
    pub fn contains(&self, name: &str) -> bool {
        self.exprs.contains_key(name)
    }
    /// Value of a named parameter.
    pub fn value(&mut self, name: &str) -> Result<f64, ExprError> {
        let mut stack = Vec::new();
        self.value_inner(name, &mut stack, 0)
    }
    fn value_inner(&mut self, name: &str, stack: &mut Vec<String>, depth: usize) -> Result<f64, ExprError> {
        if depth >= MAX_DEPTH {
            return Err(ExprError::TooDeep);
        }
        if let Some(v) = self.cache.get(name) {
            return Ok(*v);
        }
        if stack.iter().any(|s| s == name) {
            return Err(ExprError::Cycle(name.to_string()));
        }
        if stack.len() >= MAX_DEPTH {
            return Err(ExprError::TooDeep);
        }
        let text = self.exprs.get(name).cloned().ok_or_else(|| ExprError::Unknown(name.to_string()))?;
        let e = parse(&text)?;
        stack.push(name.to_string());
        let r = eval_at_depth(&e, &mut |n, depth| self.value_inner(n, stack, depth), depth);
        stack.pop();
        let v = r?;
        self.cache.insert(name.to_string(), v);
        Ok(v)
    }
    /// Evaluate a free expression against this environment.
    pub fn eval(&mut self, text: &str) -> Result<f64, ExprError> {
        let e = parse(text)?;
        let mut stack = Vec::new();
        eval_at_depth(&e, &mut |n, depth| self.value_inner(n, &mut stack, depth), 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic() {
        assert_eq!(eval_const("1+2*3").unwrap(), 7.0);
        assert_eq!(eval_const("(1+2)*3").unwrap(), 9.0);
        assert_eq!(eval_const("-2*-3").unwrap(), 6.0);
        assert_eq!(eval_const("10/4").unwrap(), 2.5);
        assert_eq!(eval_const("1e2+.5").unwrap(), 100.5);
        assert_eq!(eval_const("8-2-1").unwrap(), 5.0);
        assert!((eval_const("pi").unwrap() - std::f64::consts::PI).abs() < 1e-15);
    }

    #[test]
    fn errors() {
        assert_eq!(eval_const("1/0"), Err(ExprError::DivZero));
        assert!(matches!(eval_const("1+"), Err(ExprError::Syntax(_))));
        assert!(matches!(eval_const("(1"), Err(ExprError::Syntax(_))));
        assert!(matches!(eval_const("1 2"), Err(ExprError::Syntax(_))));
        assert!(matches!(eval_const("x"), Err(ExprError::Unknown(_))));
        assert!(matches!(eval_const(""), Err(ExprError::Syntax(_))));
        assert!(matches!(eval_const("1e308*1e308"), Err(ExprError::NotFinite)));
        assert!(matches!(eval_const(&"(".repeat(500)), Err(ExprError::TooDeep)));
        assert!(matches!(eval_const(&"-".repeat(500)), Err(ExprError::TooDeep)));
        assert!(matches!(eval_const(&"1+".repeat(600)), Err(ExprError::TooDeep)));
        assert!(eval_const("1.2.3").is_err());
        assert!(eval_const("é").is_err());
    }

    #[test]
    fn env_and_cycles() {
        let mut e = Env::new();
        e.insert("w", "10");
        e.insert("h", "w/2");
        e.insert("a", "b+1");
        e.insert("b", "a+1");
        assert_eq!(e.value("h").unwrap(), 5.0);
        assert_eq!(e.eval("w*h").unwrap(), 50.0);
        assert!(matches!(e.value("a"), Err(ExprError::Cycle(_))));
        assert!(matches!(e.value("zz"), Err(ExprError::Unknown(_))));
        let mut chain = Env::new();
        for i in 0..200 {
            chain.insert(&format!("p{i}"), &format!("p{}+1", i + 1));
        }
        assert!(chain.value("p0").is_err());
    }

    #[test]
    fn expression_and_parameter_depth_share_a_limit_on_a_small_stack() {
        std::thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(|| {
                for op in ["+", "-", "*", "/"] {
                    let within = vec!["1"; MAX_DEPTH].join(op);
                    assert!(eval_const(&within).is_ok());
                    assert_eq!(parse(&format!("{within}{op}1")), Err(ExprError::TooDeep));
                }
                let mut env = Env::new();
                for i in 0..MAX_DEPTH {
                    env.insert(&format!("p{i}"), &format!("p{}+{}", i + 1, vec!["0"; 30].join("+")));
                }
                env.insert(&format!("p{MAX_DEPTH}"), "1");
                assert_eq!(env.value("p0"), Err(ExprError::TooDeep));
                assert_eq!(env.eval("p0+1"), Err(ExprError::TooDeep));
                env.insert("safe", "2*3");
                assert_eq!(env.eval("safe+1"), Ok(7.0));
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn rename_replaces_whole_names_only() {
        assert_eq!(rename("d1+d10*d1 / (d1_x+2e1)", "d1", "w"), "w+d10*w / (d1_x+2e1)");
        assert_eq!(rename("1e2+e", "e", "x"), "1e2+x");
        assert_eq!(rename("é+a", "a", "b"), "é+b");
    }

    #[test]
    fn names_valid() {
        assert!(valid_name("d1") && valid_name("_w") && !valid_name("1d") && !valid_name("a-b") && !valid_name("") && !valid_name("pi"));
        let mut v = Vec::new();
        names(&parse("a+b*a").unwrap(), &mut v);
        assert_eq!(v, vec!["a".to_string(), "b".to_string()]);
    }
}
