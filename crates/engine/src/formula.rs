//! Formulas typed at a prompt: `=1200+350*2`, `=(2400-50)/3`. A value starting with `=` is
//! worked out with `+`, `-`, `*`, `/`, parentheses and unary minus, and the result is the answer
//! (a distance, a length, a number of items...).

/// Longest formula read.
const MAX_LEN: usize = 256;
/// Deepest nesting of parentheses and signs.
const MAX_DEPTH: usize = 32;

/// The value of `text` when it is a formula (`=` then an expression); `None` when it is not one,
/// `Some(Err)` with the reason when it is one that can't be worked out.
pub fn eval(text: &str) -> Option<Result<f64, String>> {
    let body = text.trim().strip_prefix('=')?;
    if body.len() > MAX_LEN {
        return Some(Err("The formula is too long.".into()));
    }
    let mut p = Parser { s: body.as_bytes(), i: 0, depth: 0 };
    let v = p.expr().and_then(|v| {
        p.ws();
        if p.i < p.s.len() { Err(format!("Unexpected \"{}\" in the formula.", &body[p.i..])) } else { Ok(v) }
    });
    Some(v.and_then(|v| if v.is_finite() { Ok(v) } else { Err("The formula has no finite value (division by zero?).".into()) }))
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
    fn expr(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        while let Some(c @ (b'+' | b'-')) = self.peek() {
            self.i += 1;
            let r = self.term()?;
            v = if c == b'+' { v + r } else { v - r };
        }
        Ok(v)
    }
    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.factor()?;
        while let Some(c @ (b'*' | b'/' | b'x' | b'X')) = self.peek() {
            self.i += 1;
            let r = self.factor()?;
            v = if c == b'/' { v / r } else { v * r };
        }
        Ok(v)
    }
    fn factor(&mut self) -> Result<f64, String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err("The formula is nested too deeply.".into());
        }
        let r = match self.peek() {
            Some(b'-') => {
                self.i += 1;
                self.factor().map(|v| -v)
            }
            Some(b'+') => {
                self.i += 1;
                self.factor()
            }
            Some(b'(') => {
                self.i += 1;
                let v = self.expr()?;
                if self.peek() != Some(b')') {
                    return Err("A \")\" is missing in the formula.".into());
                }
                self.i += 1;
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() || c == b'.' || c == b',' => self.number(),
            Some(_) => Err("The formula takes numbers, + - * / and parentheses.".into()),
            None => Err("The formula ends too soon.".into()),
        };
        self.depth -= 1;
        r
    }
    /// A number; a comma is read as the decimal separator (`2,5`).
    fn number(&mut self) -> Result<f64, String> {
        let start = self.i;
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || *c == b'.' || *c == b',') {
            self.i += 1;
        }
        // An exponent: 1e3, 2.5E-2.
        if self.s.get(self.i).is_some_and(|c| *c == b'e' || *c == b'E')
            && self.s.get(self.i + 1).is_some_and(|c| c.is_ascii_digit() || *c == b'-' || *c == b'+')
        {
            self.i += 2;
            while self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
                self.i += 1;
            }
        }
        let t = String::from_utf8_lossy(self.s.get(start..self.i).unwrap_or_default()).replace(',', ".");
        t.parse::<f64>().map_err(|_| format!("\"{t}\" is not a number."))
    }
}

#[cfg(test)]
mod tests {
    use super::eval;

    #[test]
    fn formulas_work_out_with_precedence_and_parentheses() {
        assert_eq!(eval("=1200+350*2"), Some(Ok(1900.0)));
        assert_eq!(eval("= (2400 - 50) / 2"), Some(Ok(1175.0)));
        assert_eq!(eval("=-3*-2"), Some(Ok(6.0)));
        assert_eq!(eval("=2,5*2"), Some(Ok(5.0)));
        assert_eq!(eval("=10/4"), Some(Ok(2.5)));
        assert_eq!(eval("=1e3/4"), Some(Ok(250.0)));
        assert_eq!(eval("1200"), None);
    }

    #[test]
    fn bad_formulas_say_why_and_never_panic() {
        for f in ["=", "=1+", "=(1+2", "=1/0", "=abc", "=1)2", &format!("={}", "(".repeat(500)), &format!("={}1", "-".repeat(100))] {
            assert!(matches!(eval(f), Some(Err(_))), "{f}");
        }
    }
}
