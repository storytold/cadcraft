//! Shape source (`.shp`) to a compiled SHX file, as COMPILE does.
//!
//! A source is a list of definitions: a header line `*number,defbytes,NAME` followed by its
//! specification bytes (on as many lines as needed, ending with 0). Numbers are decimal, or
//! hexadecimal after a leading `0` (`012`, `08cfb`); parentheses only group; `;` starts a
//! comment. A big font starts with `*BIGFONT nchars,nranges,first,last,…` and a Unicode font
//! with `*UNIFONT,6,name` followed by `above,below,modes,encoding,type,0`. Shape 0 is a text
//! font's record (`above,below,modes,0`).
//!
//! Operands are encoded by what they are: a subshape number takes two bytes in a Unicode font
//! (and after `7,0` in a big font), displacements and bulges are two's complement, and an arc's
//! direction byte (`-043`) keeps its sign in the high bit. `defbytes` is not checked: the
//! program's own length is written.

use super::ShxError;
use super::parse::{BIGFONT, SHAPES_10, UNIFONT};

/// Largest source compiled.
const MAX_SOURCE: usize = 8 << 20;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Shapes,
    Big,
    Uni,
}

struct Shape {
    number: u16,
    name: String,
    program: Vec<u8>,
}

/// A number token: value (sign included).
fn number(tok: &str, line: usize) -> Result<i64, ShxError> {
    let t: String = tok.chars().filter(|c| !matches!(c, '(' | ')') && !c.is_whitespace()).collect();
    let (neg, digits) = match t.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, t.strip_prefix('+').unwrap_or(&t)),
    };
    let err = || ShxError::Source(line, format!("not a number: {tok:?}"));
    let v = if digits.len() > 1 && digits.starts_with('0') {
        i64::from_str_radix(digits.get(1..).unwrap_or_default(), 16).map_err(|_| err())?
    } else {
        digits.parse::<i64>().map_err(|_| err())?
    };
    if v > 0xFFFF {
        return Err(err());
    }
    Ok(if neg { -v } else { v })
}

/// The comma-separated numbers of a line (comment removed, blank fields skipped).
fn numbers(text: &str, line: usize) -> Result<Vec<i64>, ShxError> {
    text.split(',').map(str::trim).filter(|t| !t.is_empty() && *t != "(" && *t != ")").map(|t| number(t, line)).collect()
}

/// The spec-byte encoder: walks the tokens as the reader walks the bytes.
struct Enc<'a> {
    toks: &'a [i64],
    i: usize,
    out: Vec<u8>,
    kind: Kind,
    line: usize,
}

impl Enc<'_> {
    fn next(&mut self) -> Result<i64, ShxError> {
        let v = self.toks.get(self.i).copied().ok_or_else(|| ShxError::Source(self.line, "the shape ends inside a command".into()))?;
        self.i += 1;
        Ok(v)
    }
    fn byte(&mut self) -> Result<(), ShxError> {
        let v = self.next()?;
        let b = u8::try_from(v).map_err(|_| ShxError::Source(self.line, format!("{v} is not a byte")))?;
        self.out.push(b);
        Ok(())
    }
    /// Two's complement −128..127.
    fn signed(&mut self) -> Result<i64, ShxError> {
        let v = self.next()?;
        let b = i8::try_from(v).map_err(|_| ShxError::Source(self.line, format!("{v} is not −128..127")))?;
        self.out.extend(b.to_le_bytes());
        Ok(v)
    }
    /// Sign in the high bit (an arc's direction byte).
    fn sign_magnitude(&mut self) -> Result<(), ShxError> {
        let v = self.next()?;
        let m = u8::try_from(v.unsigned_abs())
            .ok()
            .filter(|m| *m < 0x80)
            .ok_or_else(|| ShxError::Source(self.line, format!("{v} is not an octant byte")))?;
        self.out.push(if v < 0 { m | 0x80 } else { m });
        Ok(())
    }
    fn word(&mut self) -> Result<(), ShxError> {
        let v = self.next()?;
        let w = u16::try_from(v).map_err(|_| ShxError::Source(self.line, format!("{v} is not a shape number")))?;
        self.out.extend(w.to_be_bytes());
        Ok(())
    }

    fn program(mut self) -> Result<Vec<u8>, ShxError> {
        while self.i < self.toks.len() {
            let code = self.next()?;
            let c = u8::try_from(code).map_err(|_| ShxError::Source(self.line, format!("{code} is not a code")))?;
            self.out.push(c);
            match c {
                0 => break,
                3 | 4 => self.byte()?,
                7 => match self.kind {
                    Kind::Uni => self.word()?,
                    Kind::Big => {
                        let first = self.toks.get(self.i).copied().unwrap_or(0);
                        if first == 0 {
                            self.byte()?;
                            self.word()?;
                            for _ in 0..4 {
                                self.byte()?;
                            }
                        } else {
                            self.byte()?;
                        }
                    }
                    Kind::Shapes => self.byte()?,
                },
                8 => {
                    self.signed()?;
                    self.signed()?;
                }
                9 => loop {
                    let (x, y) = (self.signed()?, self.signed()?);
                    if x == 0 && y == 0 {
                        break;
                    }
                },
                10 => {
                    self.byte()?;
                    self.sign_magnitude()?;
                }
                11 => {
                    for _ in 0..4 {
                        self.byte()?;
                    }
                    self.sign_magnitude()?;
                }
                12 => {
                    for _ in 0..3 {
                        self.signed()?;
                    }
                }
                13 => loop {
                    let (x, y) = (self.signed()?, self.signed()?);
                    if x == 0 && y == 0 {
                        break;
                    }
                    self.signed()?;
                },
                _ => {}
            }
        }
        if self.out.last() != Some(&0) {
            self.out.push(0);
        }
        Ok(self.out)
    }
}

/// Compile shape source into an SHX file (a shape file or text font, a big font, or a Unicode
/// font, as its first definition says).
pub fn compile(source: &str) -> Result<Vec<u8>, ShxError> {
    if source.len() > MAX_SOURCE {
        return Err(ShxError::TooLarge);
    }
    let mut kind = Kind::Shapes;
    let mut ranges: Vec<(u16, u16)> = Vec::new();
    let mut uni_info: Option<(String, Vec<u8>)> = None;
    // Definitions: header (number, name, line) and the numbers that follow it.
    let mut defs: Vec<(i64, String, usize, Vec<i64>)> = Vec::new();
    let mut uni_header_pending = false;
    for (no, raw) in source.lines().enumerate() {
        let line = no + 1;
        let text = raw.split(';').next().unwrap_or_default().trim();
        if text.is_empty() {
            continue;
        }
        if let Some(head) = text.strip_prefix('*') {
            let upper = head.trim_start().to_ascii_uppercase();
            if let Some(rest) = upper.strip_prefix("BIGFONT") {
                kind = Kind::Big;
                let v = numbers(rest, line)?;
                let n = usize::try_from(v.get(1).copied().unwrap_or(0)).unwrap_or(0);
                for k in 0..n {
                    let lo = v.get(2 + 2 * k).copied().unwrap_or(0);
                    let hi = v.get(3 + 2 * k).copied().unwrap_or(lo);
                    let to = |x: i64| u16::try_from(x).map_err(|_| ShxError::Source(line, "bad escape range".into()));
                    ranges.push((to(lo)?, to(hi)?));
                }
                continue;
            }
            if upper.starts_with("UNIFONT") {
                kind = Kind::Uni;
                let name = head.splitn(3, ',').nth(2).unwrap_or_default().trim().to_string();
                uni_info = Some((name, Vec::new()));
                uni_header_pending = true;
                continue;
            }
            uni_header_pending = false;
            let mut parts = head.splitn(3, ',');
            let num = number(parts.next().unwrap_or_default().trim(), line)?;
            let _defbytes = parts.next();
            let name = parts.next().unwrap_or_default().trim().to_string();
            defs.push((num, name, line, Vec::new()));
            continue;
        }
        let v = numbers(text, line)?;
        if uni_header_pending {
            if let Some((_, info)) = uni_info.as_mut() {
                for x in v {
                    info.push(u8::try_from(x).map_err(|_| ShxError::Source(line, "bad font record".into()))?);
                }
            }
            continue;
        }
        match defs.last_mut() {
            Some(d) => d.3.extend(v),
            None => return Err(ShxError::Source(line, "specification bytes before any *number,defbytes,NAME line".into())),
        }
    }
    let mut shapes: Vec<Shape> = Vec::new();
    for (num, name, line, toks) in defs {
        let number = u16::try_from(num).map_err(|_| ShxError::Source(line, format!("shape number {num} out of range")))?;
        let program = if number == 0 {
            // The font record: plain bytes.
            toks.iter().map(|x| u8::try_from(*x).map_err(|_| ShxError::Source(line, "bad font record".into()))).collect::<Result<Vec<u8>, _>>()?
        } else {
            Enc { toks: &toks, i: 0, out: Vec::new(), kind, line }.program()?
        };
        if shapes.iter().any(|s| s.number == number) {
            return Err(ShxError::Source(line, format!("shape {number} is defined twice")));
        }
        shapes.push(Shape { number, name, program });
    }
    if shapes.is_empty() {
        return Err(ShxError::Empty);
    }
    shapes.sort_by_key(|s| s.number);
    let record = |s: &Shape| -> Result<Vec<u8>, ShxError> {
        let mut r: Vec<u8> = s.name.bytes().filter(|b| *b != 0).collect();
        r.push(0);
        r.extend(&s.program);
        if r.len() > usize::from(u16::MAX) { Err(ShxError::TooLarge) } else { Ok(r) }
    };
    let count = u16::try_from(shapes.len()).map_err(|_| ShxError::TooLarge)?;
    let len16 = |r: &[u8]| u16::try_from(r.len()).unwrap_or(u16::MAX);
    let mut out = Vec::new();
    match kind {
        Kind::Shapes => {
            out.extend(SHAPES_10);
            out.extend(shapes.first().map_or(0, |s| s.number).to_le_bytes());
            out.extend(shapes.last().map_or(0, |s| s.number).to_le_bytes());
            out.extend(count.to_le_bytes());
            let recs: Vec<Vec<u8>> = shapes.iter().map(record).collect::<Result<_, _>>()?;
            for (s, r) in shapes.iter().zip(&recs) {
                out.extend(s.number.to_le_bytes());
                out.extend(len16(r).to_le_bytes());
            }
            for r in &recs {
                out.extend(r);
            }
            out.extend(b"EOF");
        }
        Kind::Big => {
            out.extend(BIGFONT);
            let nranges = u16::try_from(ranges.len()).map_err(|_| ShxError::TooLarge)?;
            out.extend(count.to_le_bytes());
            out.extend(count.to_le_bytes());
            out.extend(nranges.to_le_bytes());
            for (lo, hi) in &ranges {
                out.extend(lo.to_le_bytes());
                out.extend(hi.to_le_bytes());
            }
            let recs: Vec<Vec<u8>> = shapes.iter().map(record).collect::<Result<_, _>>()?;
            let mut off = out.len() + 8 * recs.len();
            for (s, r) in shapes.iter().zip(&recs) {
                out.extend(s.number.to_le_bytes());
                out.extend(len16(r).to_le_bytes());
                out.extend(u32::try_from(off).map_err(|_| ShxError::TooLarge)?.to_le_bytes());
                off += r.len();
            }
            for r in &recs {
                out.extend(r);
            }
        }
        Kind::Uni => {
            out.extend(UNIFONT);
            let (name, info) = uni_info.unwrap_or_default();
            out.extend((u32::from(count) + 1).to_le_bytes());
            let mut rec: Vec<u8> = name.bytes().filter(|b| *b != 0).collect();
            rec.push(0);
            rec.extend(&info);
            out.extend(len16(&rec).to_le_bytes());
            out.extend(&rec);
            for s in &shapes {
                let r = record(s)?;
                out.extend(s.number.to_le_bytes());
                out.extend(len16(&r).to_le_bytes());
                out.extend(r);
            }
        }
    }
    Ok(out)
}
