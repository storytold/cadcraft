//! A standalone DXF tag reader and writer.
//!
//! DXF is a sequence of (group code, value) pairs. This crate parses ASCII and binary DXF into a
//! flat list of [`Tag`]s, splits them into sections and entities, and writes tags back. Mapping
//! tags to a drawing model is left to the caller. Written from Autodesk's published DXF
//! Reference; depends on nothing else in the workspace.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::fmt::Write as _;

mod version;
pub use version::Version;

#[derive(Debug, thiserror::Error)]
pub enum DxfError {
    #[error("not a DXF file")]
    NotDxf,
    #[error("line {line}: bad group code `{text}`")]
    BadCode { line: usize, text: String },
    #[error("unexpected end of file")]
    Eof,
    /// More group codes than [`MAX_TAGS`]: `tags` is the count found (or the count read when the
    /// limit was reached), `limit` the maximum.
    #[error("drawing too large: {tags} DXF group codes, more than the {limit} CADCraft reads")]
    TooLarge { tags: usize, limit: usize },
    /// A streamed rendition ([`TagStream`]) longer than its byte limit.
    #[error("drawing too large: {bytes} bytes of DXF, more than the {limit} CADCraft reads")]
    TooLong { bytes: usize, limit: usize },
}

pub type Result<T> = std::result::Result<T, DxfError>;

/// A typed group value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Real(f64),
    Int(i64),
    Bool(bool),
    /// Binary chunk (310–319, 1004) as hex text.
    Hex(String),
}

impl Value {
    pub fn as_str(&self) -> String {
        match self {
            Value::Str(s) | Value::Hex(s) => s.clone(),
            Value::Real(r) => format_real(*r),
            Value::Int(i) => i.to_string(),
            Value::Bool(b) => i64::from(*b).to_string(),
        }
    }
    pub fn as_f64(&self) -> f64 {
        match self {
            Value::Real(r) if r.is_finite() => *r,
            Value::Int(i) => *i as f64,
            Value::Str(s) => s.trim().parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(0.0),
            Value::Bool(b) => f64::from(u8::from(*b)),
            _ => 0.0,
        }
    }
    pub fn as_i64(&self) -> i64 {
        match self {
            Value::Int(i) => *i,
            Value::Real(r) if r.is_finite() => *r as i64,
            Value::Str(s) => s.trim().parse::<i64>().unwrap_or_else(|_| s.trim().parse::<f64>().map(|f| f as i64).unwrap_or(0)),
            Value::Bool(b) => i64::from(*b),
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tag {
    pub code: i32,
    pub value: Value,
}

impl Tag {
    pub fn new(code: i32, value: Value) -> Self {
        Tag { code, value }
    }
    pub fn s(code: i32, v: impl Into<String>) -> Self {
        Tag { code, value: Value::Str(v.into()) }
    }
    pub fn f(code: i32, v: f64) -> Self {
        Tag { code, value: Value::Real(if v.is_finite() { v } else { 0.0 }) }
    }
    pub fn i(code: i32, v: i64) -> Self {
        Tag { code, value: Value::Int(v) }
    }
    pub fn str(&self) -> String {
        self.value.as_str()
    }
    pub fn f64(&self) -> f64 {
        self.value.as_f64()
    }
    pub fn i64(&self) -> i64 {
        self.value.as_i64()
    }
}

/// The value type for a group code (DXF Reference, "Group Code Value Types").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Str,
    Real,
    Int16,
    Int32,
    Int64,
    Bool,
    Hex,
}

pub fn kind_of(code: i32) -> Kind {
    match code {
        0..=9 => Kind::Str,
        10..=59 => Kind::Real,
        60..=79 => Kind::Int16,
        90..=99 => Kind::Int32,
        100..=109 => Kind::Str,
        110..=149 => Kind::Real,
        160..=169 => Kind::Int64,
        170..=179 => Kind::Int16,
        210..=239 => Kind::Real,
        270..=289 => Kind::Int16,
        290..=299 => Kind::Bool,
        300..=309 => Kind::Str,
        310..=319 => Kind::Hex,
        320..=369 => Kind::Str,
        370..=389 => Kind::Int16,
        390..=399 => Kind::Str,
        400..=409 => Kind::Int16,
        410..=419 => Kind::Str,
        420..=429 => Kind::Int32,
        430..=439 => Kind::Str,
        440..=449 => Kind::Int32,
        450..=459 => Kind::Int32,
        460..=469 => Kind::Real,
        470..=479 => Kind::Str,
        480..=481 => Kind::Str,
        999 => Kind::Str,
        1000..=1003 => Kind::Str,
        1004 => Kind::Hex,
        1005..=1009 => Kind::Str,
        1010..=1059 => Kind::Real,
        1060..=1070 => Kind::Int16,
        1071 => Kind::Int32,
        _ => Kind::Str,
    }
}

fn typed(code: i32, text: &str) -> Value {
    let t = text.trim();
    match kind_of(code) {
        Kind::Real => t.parse::<f64>().ok().filter(|v| v.is_finite()).map(Value::Real).unwrap_or(Value::Real(0.0)),
        Kind::Int16 | Kind::Int32 | Kind::Int64 => {
            t.parse::<i64>().map(Value::Int).unwrap_or_else(|_| Value::Int(t.parse::<f64>().map(|f| f as i64).unwrap_or(0)))
        }
        Kind::Bool => Value::Bool(t.parse::<i64>().unwrap_or(0) != 0),
        Kind::Hex => Value::Hex(t.to_string()),
        Kind::Str => Value::Str(text.trim_end_matches(['\r', '\n']).to_string()),
    }
}

const BINARY_SENTINEL: &[u8] = b"AutoCAD Binary DXF\r\n\x1a\0";
/// Most group codes read from one file. A `Tag` takes 40 bytes plus its text, so this bounds the
/// tag list to a few GB; real drawings stay far below (a 10 MB DWG is typically a few million).
pub const MAX_TAGS: usize = 50_000_000;

mod codepage;
pub use codepage::decode_code_page;

/// Parse a DXF file (ASCII or binary).
pub fn parse(bytes: &[u8]) -> Result<Vec<Tag>> {
    parse_with_limit(bytes, MAX_TAGS)
}

/// [`parse`] with an explicit group-code limit. For ASCII DXF the limit is checked up front from
/// the line count, before the text is decoded or any tag allocated, so an oversized file fails in
/// milliseconds instead of after it has filled memory.
pub fn parse_with_limit(bytes: &[u8], max_tags: usize) -> Result<Vec<Tag>> {
    if let Some(body) = bytes.strip_prefix(BINARY_SENTINEL) {
        return parse_binary_with_limit(body, max_tags);
    }
    // Every group code takes two lines (code, value).
    let tags = bytes.iter().filter(|&&b| b == b'\n').count().div_ceil(2);
    if tags > max_tags {
        return Err(DxfError::TooLarge { tags, limit: max_tags });
    }
    parse_ascii_with_limit(&codepage::decode_ascii_file(bytes), max_tags)
}

/// The four hex digits of a `\U+XXXX` escape starting at `chars[i]`, if there is one.
fn escape_at(chars: &[char], i: usize) -> Option<u32> {
    if chars.get(i) != Some(&'\\') || chars.get(i + 1) != Some(&'U') || chars.get(i + 2) != Some(&'+') {
        return None;
    }
    let hex = chars.get(i + 3..i + 7)?;
    if !hex.iter().all(char::is_ascii_hexdigit) {
        return None;
    }
    hex.iter().try_fold(0u32, |acc, c| Some(acc * 16 + c.to_digit(16)?))
}

/// The character of a `\M+nXXXX` escape starting at `chars[i]`, if there is one: `n` names a
/// double-byte code page and `XXXX` is the character's two bytes in hex.
fn mif_escape_at(chars: &[char], i: usize) -> Option<char> {
    if chars.get(i) != Some(&'\\') || chars.get(i + 1) != Some(&'M') || chars.get(i + 2) != Some(&'+') {
        return None;
    }
    let enc = codepage::mif_encoding(*chars.get(i + 3)?)?;
    let hex = chars.get(i + 4..i + 8)?;
    if !hex.iter().all(char::is_ascii_hexdigit) {
        return None;
    }
    let code = hex.iter().try_fold(0u16, |acc, c| Some(acc * 16 + u16::try_from(c.to_digit(16)?).ok()?))?;
    let bytes = code.to_be_bytes();
    let text = enc.decode_without_bom_handling_and_without_replacement(&bytes)?;
    let mut it = text.chars();
    it.next().filter(|_| it.next().is_none())
}

/// Unescape the `\U+XXXX` sequences pre-R2007 DXF uses for characters outside the file's code
/// page (four hex digits, one UTF-16 code unit; characters above U+FFFF are a surrogate pair of
/// escapes), and the `\M+nXXXX` sequences for double-byte East Asian characters. Malformed
/// escapes, unpaired surrogates and unknown `\M+` code pages are kept as literal text.
fn unescape_unicode(s: &str) -> String {
    if !s.contains("\\U+") && !s.contains("\\M+") {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if let Some(ch) = mif_escape_at(&chars, i) {
            out.push(ch);
            i += 8;
            continue;
        }
        if let Some(unit) = escape_at(&chars, i) {
            if let Some(ch) = char::from_u32(unit) {
                out.push(ch);
                i += 7;
                continue;
            }
            if (0xD800..0xDC00).contains(&unit)
                && let Some(low) = escape_at(&chars, i + 7).filter(|u| (0xDC00..0xE000).contains(u))
                && let Some(ch) = char::from_u32(0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00))
            {
                out.push(ch);
                i += 14;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

pub fn parse_ascii(text: &str) -> Result<Vec<Tag>> {
    parse_ascii_with_limit(text, MAX_TAGS)
}

#[allow(clippy::while_let_loop)]
fn parse_ascii_with_limit(text: &str, max_tags: usize) -> Result<Vec<Tag>> {
    let mut tags = Vec::new();
    let mut lines = text.lines().enumerate();
    loop {
        let Some((ln, code_line)) = lines.next() else { break };
        let ct = code_line.trim();
        if ct.is_empty() {
            continue;
        }
        let code: i32 = ct.parse().map_err(|_| DxfError::BadCode { line: ln + 1, text: ct.chars().take(40).collect() })?;
        let Some((_, val)) = lines.next() else { return Err(DxfError::Eof) };
        let v = typed(code, val);
        let v = match v {
            Value::Str(s) => Value::Str(unescape_unicode(&s)),
            o => o,
        };
        tags.push(Tag { code, value: v });
        if tags.len() > max_tags {
            return Err(DxfError::TooLarge { tags: tags.len(), limit: max_tags });
        }
        if code == 0 && tags.last().is_some_and(|t| t.str() == "EOF") {
            break;
        }
    }
    if tags.is_empty() {
        return Err(DxfError::NotDxf);
    }
    Ok(tags)
}

struct Cur<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.p.checked_add(n).ok_or(DxfError::Eof)?;
        let s = self.b.get(self.p..end).ok_or(DxfError::Eof)?;
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(*self.take(1)?.first().ok_or(DxfError::Eof)?)
    }
    fn i16(&mut self) -> Result<i16> {
        let s = self.take(2)?;
        Ok(i16::from_le_bytes([s[0], s[1]]))
    }
    fn i32(&mut self) -> Result<i32> {
        let s = self.take(4)?;
        Ok(i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i64(&mut self) -> Result<i64> {
        let s = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(i64::from_le_bytes(a))
    }
    fn f64(&mut self) -> Result<f64> {
        let s = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(f64::from_le_bytes(a))
    }
    /// The bytes of a zero-terminated string (without the zero), decoded by the caller.
    fn cstr(&mut self) -> Result<&'a [u8]> {
        let start = self.p;
        while self.p < self.b.len() && self.b.get(self.p) != Some(&0) {
            self.p += 1;
        }
        let s = self.b.get(start..self.p).unwrap_or(&[]);
        if self.p >= self.b.len() {
            return Err(DxfError::Eof);
        }
        self.p += 1;
        Ok(s)
    }
}

/// Binary DXF (R13+: 2-byte group codes).
pub fn parse_binary(b: &[u8]) -> Result<Vec<Tag>> {
    parse_binary_with_limit(b, MAX_TAGS)
}

/// The record at the front of `b`: its tag and the bytes it took, or `None` when `b` ends before
/// the record does (nothing is consumed and `text` is left as it was, so the caller can try again
/// with more bytes). A record never spans more than [`MAX_RECORD`] bytes.
fn binary_record(b: &[u8], text: &mut codepage::BinaryText) -> Result<Option<(Tag, usize)>> {
    let mut c = Cur { b, p: 0 };
    let Ok(mut code) = c.i16().map(i32::from) else { return Ok(None) };
    if code == 255 {
        let Ok(ext) = c.i16() else { return Ok(None) };
        code = i32::from(ext);
    }
    let v = match kind_of(code) {
        Kind::Str => {
            let Ok(raw) = c.cstr() else {
                // No terminator yet: a string that long is not a DXF record at all.
                return if b.len() > MAX_RECORD { Err(DxfError::Eof) } else { Ok(None) };
            };
            Value::Str(unescape_unicode(&text.string(code, raw)))
        }
        Kind::Real => {
            let Ok(v) = c.f64() else { return Ok(None) };
            Value::Real(v)
        }
        Kind::Int16 => {
            let Ok(v) = c.i16() else { return Ok(None) };
            Value::Int(i64::from(v))
        }
        Kind::Int32 => {
            let Ok(v) = c.i32() else { return Ok(None) };
            Value::Int(i64::from(v))
        }
        Kind::Int64 => {
            let Ok(v) = c.i64() else { return Ok(None) };
            Value::Int(v)
        }
        Kind::Bool => {
            let Ok(v) = c.u8() else { return Ok(None) };
            Value::Bool(v != 0)
        }
        Kind::Hex => {
            let Ok(n) = c.u8().map(usize::from) else { return Ok(None) };
            let Ok(bytes) = c.take(n) else { return Ok(None) };
            let mut s = String::with_capacity(n * 2);
            for x in bytes {
                let _ = write!(s, "{x:02X}");
            }
            Value::Hex(s)
        }
    };
    Ok(Some((Tag { code, value: v }, c.p)))
}

/// Longest binary record: a string of this many bytes without its terminator is an error, not a
/// record still waiting for the rest (DXF strings are at most a few thousand bytes; this bounds
/// what a streaming parse holds back for a hostile stream).
const MAX_RECORD: usize = 1 << 20;

fn parse_binary_with_limit(b: &[u8], max_tags: usize) -> Result<Vec<Tag>> {
    let mut text = codepage::BinaryText::default();
    let mut tags = Vec::new();
    let mut p = 0;
    while p < b.len() {
        let rest = b.get(p..).unwrap_or(&[]);
        let Some((tag, n)) = binary_record(rest, &mut text)? else { return Err(DxfError::Eof) };
        p = p.saturating_add(n);
        let eof = tag.code == 0 && matches!(&tag.value, Value::Str(s) if s == "EOF");
        tags.push(tag);
        if tags.len() > max_tags {
            return Err(DxfError::TooLarge { tags: tags.len(), limit: max_tags });
        }
        if eof {
            break;
        }
    }
    if tags.is_empty() {
        return Err(DxfError::NotDxf);
    }
    Ok(tags)
}

/// A DXF parser fed through [`std::io::Write`], for a file that is produced in chunks (a DWG
/// converted to DXF): the tags are built as the bytes arrive, so the text never exists in memory
/// as a whole. The result is what [`parse`] gives for the same bytes, ASCII or binary (the
/// binary sentinel at the start decides).
///
/// Limits: `max_tags` group codes (as [`parse`]'s), and `max_bytes` of input in all; past either
/// the stream fails, and stays failed. A line or record cut by a chunk boundary is held back
/// until the rest arrives (at most [`MAX_RECORD`] bytes); everything after the `0`/`EOF` tag is
/// ignored.
///
/// Strings of an ASCII stream are decoded line by line: valid UTF-8 as UTF-8, anything else in
/// the `$DWGCODEPAGE` the header declared (Windows-1252 without one), which is what [`parse`]
/// does for a whole file except that it decides from the whole text at once.
pub struct TagStream {
    tags: Vec<Tag>,
    text: codepage::BinaryText,
    /// Bytes not yet parsed: the first bytes until the form is known, then a cut line or record.
    carry: Vec<u8>,
    seen: usize,
    max_tags: usize,
    max_bytes: usize,
    form: Option<Form>,
    /// ASCII: the group code whose value line comes next, and the line count for messages.
    pending: Option<i32>,
    line: usize,
    done: bool,
    failed: Option<DxfError>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Form {
    Ascii,
    Binary,
}

impl TagStream {
    pub fn new(max_tags: usize, max_bytes: usize) -> Self {
        TagStream {
            tags: Vec::new(),
            text: codepage::BinaryText::default(),
            carry: Vec::new(),
            seen: 0,
            max_tags,
            max_bytes,
            form: None,
            pending: None,
            line: 0,
            done: false,
            failed: None,
        }
    }

    /// The tags so far (complete records only).
    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    /// Bytes fed so far.
    pub fn bytes_seen(&self) -> usize {
        self.seen
    }

    /// The tags, once the whole file has been written: an error if the stream failed, ends
    /// inside a record, or holds no tags at all.
    pub fn finish(mut self) -> Result<Vec<Tag>> {
        if let Some(e) = self.failed {
            return Err(e);
        }
        if !self.done && !self.carry.is_empty() {
            match self.form {
                // A last line without its newline, as `str::lines` would give it.
                Some(Form::Ascii) => {
                    let last = std::mem::take(&mut self.carry);
                    self.ascii_line(&last)?;
                }
                Some(Form::Binary) => return Err(DxfError::Eof),
                None => return Err(DxfError::NotDxf),
            }
        }
        if !self.done && self.pending.is_some() {
            return Err(DxfError::Eof);
        }
        if self.tags.is_empty() {
            return Err(DxfError::NotDxf);
        }
        Ok(self.tags)
    }

    fn fail(&mut self, e: DxfError) -> std::io::Error {
        let msg = e.to_string();
        self.failed = Some(e);
        std::io::Error::other(msg)
    }

    fn push(&mut self, tag: Tag) -> Result<()> {
        self.done = tag.code == 0 && matches!(&tag.value, Value::Str(s) if s == "EOF");
        self.tags.push(tag);
        if self.tags.len() > self.max_tags {
            return Err(DxfError::TooLarge { tags: self.tags.len(), limit: self.max_tags });
        }
        Ok(())
    }

    /// Parse every complete binary record at the front of `b`; the bytes taken.
    fn binary_records(&mut self, b: &[u8]) -> Result<usize> {
        let mut p = 0;
        while !self.done {
            let rest = b.get(p..).unwrap_or(&[]);
            if rest.is_empty() {
                break;
            }
            let Some((tag, n)) = binary_record(rest, &mut self.text)? else { break };
            p = p.saturating_add(n);
            self.push(tag)?;
        }
        Ok(if self.done { b.len() } else { p })
    }

    /// One ASCII line (without its newline): a group code, or the value of the pending one.
    fn ascii_line(&mut self, raw: &[u8]) -> Result<()> {
        if self.done {
            return Ok(());
        }
        self.line = self.line.saturating_add(1);
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        match self.pending.take() {
            None => {
                let code_line = String::from_utf8_lossy(raw);
                let ct = code_line.trim();
                if ct.is_empty() {
                    return Ok(());
                }
                let code: i32 = ct.parse().map_err(|_| DxfError::BadCode { line: self.line, text: ct.chars().take(40).collect() })?;
                self.pending = Some(code);
            }
            Some(code) => {
                let v = match kind_of(code) {
                    Kind::Str => Value::Str(unescape_unicode(&self.text.ascii_string(code, raw))),
                    Kind::Hex => Value::Hex(self.text.ascii_string(code, raw).trim().to_string()),
                    _ => typed(code, &String::from_utf8_lossy(raw)),
                };
                self.push(Tag { code, value: v })?;
            }
        }
        Ok(())
    }

    /// Parse every complete ASCII line at the front of `b`; the bytes taken.
    fn ascii_lines(&mut self, b: &[u8]) -> Result<usize> {
        let mut p = 0;
        while !self.done {
            let rest = b.get(p..).unwrap_or(&[]);
            let Some(i) = rest.iter().position(|&c| c == b'\n') else { break };
            self.ascii_line(rest.get(..i).unwrap_or(&[]))?;
            p = p.saturating_add(i + 1);
        }
        Ok(if self.done { b.len() } else { p })
    }

    fn take(&mut self, b: &[u8]) -> Result<usize> {
        match self.form {
            Some(Form::Binary) => self.binary_records(b),
            _ => self.ascii_lines(b),
        }
    }

    fn feed(&mut self, buf: &[u8]) -> Result<()> {
        if self.done {
            return Ok(());
        }
        self.seen = self.seen.saturating_add(buf.len());
        if self.seen > self.max_bytes {
            return Err(DxfError::TooLong { bytes: self.seen, limit: self.max_bytes });
        }
        if self.form.is_none() {
            // The form is known once the first bytes are here: binary files start with the
            // sentinel, ASCII files can't (a group code line is digits).
            self.carry.extend_from_slice(buf);
            let head: &[u8] = self.carry.get(..BINARY_SENTINEL.len().min(self.carry.len())).unwrap_or(&[]);
            if self.carry.len() < BINARY_SENTINEL.len() && BINARY_SENTINEL.starts_with(head) {
                return Ok(());
            }
            if head == BINARY_SENTINEL {
                self.form = Some(Form::Binary);
                self.carry.drain(..BINARY_SENTINEL.len());
            } else {
                self.form = Some(Form::Ascii);
            }
            let carry = std::mem::take(&mut self.carry);
            let n = self.take(&carry)?;
            self.carry.extend_from_slice(carry.get(n..).unwrap_or(&[]));
        } else if self.carry.is_empty() {
            let n = self.take(buf)?;
            self.carry.extend_from_slice(buf.get(n..).unwrap_or(&[]));
        } else {
            self.carry.extend_from_slice(buf);
            let carry = std::mem::take(&mut self.carry);
            let n = self.take(&carry)?;
            self.carry.extend_from_slice(carry.get(n..).unwrap_or(&[]));
        }
        if self.carry.len() > MAX_RECORD {
            return Err(DxfError::Eof);
        }
        Ok(())
    }
}

impl std::io::Write for TagStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Some(e) = &self.failed {
            return Err(std::io::Error::other(e.to_string()));
        }
        if let Err(e) = self.feed(buf) {
            return Err(self.fail(e));
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Format a real the way DXF writers usually do (shortest round-trip, with a decimal point).
pub fn format_real(v: f64) -> String {
    if !v.is_finite() {
        return "0.0".into();
    }
    let s = format!("{v}");
    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("NaN") { s } else { format!("{s}.0") }
}

/// Write tags as ASCII DXF (CRLF line ends, codes right-aligned to 3 as is conventional).
///
/// The output is pure ASCII: non-ASCII characters in strings are written as `\U+XXXX` escapes,
/// the pre-R2007 convention, so it is correct whatever `$DWGCODEPAGE` the header declares.
pub fn write_ascii(tags: &[Tag]) -> String {
    write_ascii_as(tags, false)
}

/// Write tags as ASCII DXF with text in UTF-8 (`utf8`, the R2007+ convention: non-ASCII
/// characters as they are) or escaped as `\U+XXXX` (older versions, see [`write_ascii`]). Line
/// breaks inside strings become spaces either way.
pub fn write_ascii_as(tags: &[Tag], utf8: bool) -> String {
    let mut out = String::with_capacity(tags.len() * 16);
    for t in tags {
        let _ = write!(out, "{:>3}\r\n", t.code);
        match &t.value {
            Value::Str(s) if utf8 => out.extend(s.chars().map(|c| if matches!(c, '\r' | '\n') { ' ' } else { c })),
            Value::Str(s) => escape_unicode(s, &mut out),
            Value::Real(r) => out.push_str(&format_real(*r)),
            Value::Int(i) => out.push_str(&i.to_string()),
            Value::Bool(b) => out.push_str(&i64::from(*b).to_string()),
            Value::Hex(h) => out.push_str(h),
        }
        out.push_str("\r\n");
    }
    out
}

/// Write tags as binary DXF (DXF Reference, "Binary DXF Files", R13+ layout): the sentinel,
/// then per tag a 2-byte little-endian group code and the value in the binary form of the code's
/// [`Kind`]: a zero-terminated string, an 8-byte real, a 2-, 4- or 8-byte integer, a 1-byte
/// boolean, or a binary chunk (one length byte, then up to 255 bytes; longer data is split into
/// several groups of the same code). Strings are UTF-8 when `utf8` (R2007+), otherwise escaped as
/// in [`write_ascii`]. A tag's value is converted to its code's kind, so a number written as text
/// is stored as a number; a code outside the 16-bit range is skipped.
pub fn write_binary(tags: &[Tag], utf8: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(BINARY_SENTINEL.len() + tags.len() * 12);
    out.extend_from_slice(BINARY_SENTINEL);
    for t in tags {
        let Ok(code) = i16::try_from(t.code) else { continue };
        let code_bytes = code.to_le_bytes();
        match kind_of(t.code) {
            Kind::Str => {
                out.extend_from_slice(&code_bytes);
                let s = match &t.value {
                    Value::Str(s) => s.clone(),
                    v => v.as_str(),
                };
                let mut text = String::with_capacity(s.len());
                if utf8 {
                    text.extend(s.chars().map(|c| if matches!(c, '\r' | '\n') { ' ' } else { c }));
                } else {
                    escape_unicode(&s, &mut text);
                }
                out.extend(text.bytes().filter(|&b| b != 0));
                out.push(0);
            }
            Kind::Real => {
                out.extend_from_slice(&code_bytes);
                out.extend_from_slice(&t.value.as_f64().to_le_bytes());
            }
            Kind::Int16 => {
                out.extend_from_slice(&code_bytes);
                let v = t.value.as_i64().clamp(i64::from(i16::MIN), i64::from(i16::MAX));
                out.extend_from_slice(&i16::try_from(v).unwrap_or_default().to_le_bytes());
            }
            Kind::Int32 => {
                out.extend_from_slice(&code_bytes);
                let v = t.value.as_i64().clamp(i64::from(i32::MIN), i64::from(i32::MAX));
                out.extend_from_slice(&i32::try_from(v).unwrap_or_default().to_le_bytes());
            }
            Kind::Int64 => {
                out.extend_from_slice(&code_bytes);
                out.extend_from_slice(&t.value.as_i64().to_le_bytes());
            }
            Kind::Bool => {
                out.extend_from_slice(&code_bytes);
                out.push(u8::from(t.value.as_i64() != 0));
            }
            Kind::Hex => {
                let bytes = hex_bytes(&t.value.as_str());
                // An empty chunk is still one group, as in ASCII.
                let mut chunks: Vec<&[u8]> = bytes.chunks(255).collect();
                if chunks.is_empty() {
                    chunks.push(&[]);
                }
                for chunk in chunks {
                    out.extend_from_slice(&code_bytes);
                    out.push(u8::try_from(chunk.len()).unwrap_or(u8::MAX));
                    out.extend_from_slice(chunk);
                }
            }
        }
    }
    out
}

/// The bytes of hex text, two digits each; anything that isn't a pair of hex digits is skipped.
fn hex_bytes(s: &str) -> Vec<u8> {
    let digits: Vec<u8> = s.bytes().filter_map(|b| char::from(b).to_digit(16).and_then(|d| u8::try_from(d).ok())).collect();
    digits.as_chunks::<2>().0.iter().map(|[hi, lo]| (hi << 4) | lo).collect()
}

/// Whether `bytes` are a binary DXF file (they start with the binary sentinel).
pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.starts_with(BINARY_SENTINEL)
}

/// Escape a string for a pre-R2007 DXF, whose text is in the `$DWGCODEPAGE` code page: ASCII
/// stays as it is and every other character becomes `\U+XXXX` (a surrogate pair of escapes
/// above U+FFFF), so the bytes are valid in any code page. Line breaks become spaces.
fn escape_unicode(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '\r' | '\n' => out.push(' '),
            c if c.is_ascii() => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    let _ = write!(out, "\\U+{u:04X}");
                }
            }
        }
    }
}

/// A section: `0 SECTION / 2 NAME ... 0 ENDSEC`.
#[derive(Clone, Debug, Default)]
pub struct Section {
    pub name: String,
    pub tags: Vec<Tag>,
}

/// Split a tag stream into sections.
pub fn sections(tags: &[Tag]) -> Vec<Section> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tags.len() {
        let is_sec = tags.get(i).is_some_and(|t| t.code == 0 && t.str() == "SECTION");
        if is_sec {
            let name = tags.get(i + 1).filter(|t| t.code == 2).map(Tag::str).unwrap_or_default();
            let start = i + 2;
            let mut j = start;
            while j < tags.len() && !(tags.get(j).is_some_and(|t| t.code == 0 && t.str() == "ENDSEC")) {
                j += 1;
            }
            out.push(Section { name, tags: tags.get(start..j).unwrap_or(&[]).to_vec() });
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// Split a run of tags into records starting at each code-0 tag: (type, tags after the 0).
pub fn records(tags: &[Tag]) -> Vec<(String, Vec<Tag>)> {
    let mut out: Vec<(String, Vec<Tag>)> = Vec::new();
    for t in tags {
        if t.code == 0 {
            out.push((t.str(), Vec::new()));
        } else if let Some(last) = out.last_mut() {
            last.1.push(t.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_roundtrip() {
        let tags = vec![
            Tag::s(0, "SECTION"),
            Tag::s(2, "ENTITIES"),
            Tag::s(0, "LINE"),
            Tag::s(8, "0"),
            Tag::f(10, 1.5),
            Tag::f(20, 2.0),
            Tag::i(62, 1),
            Tag::s(0, "ENDSEC"),
            Tag::s(0, "EOF"),
        ];
        let text = write_ascii(&tags);
        let back = parse(text.as_bytes()).unwrap();
        assert_eq!(back, tags);
        let secs = sections(&back);
        assert_eq!(secs.len(), 1);
        assert_eq!(secs[0].name, "ENTITIES");
        let recs = records(&secs[0].tags);
        assert_eq!(recs[0].0, "LINE");
        assert_eq!(recs[0].1.len(), 4);
    }

    #[test]
    fn binary_parse() {
        let mut b = BINARY_SENTINEL.to_vec();
        let push_str = |b: &mut Vec<u8>, code: i16, s: &str| {
            b.extend_from_slice(&code.to_le_bytes());
            b.extend_from_slice(s.as_bytes());
            b.push(0);
        };
        push_str(&mut b, 0, "LINE");
        b.extend_from_slice(&10i16.to_le_bytes());
        b.extend_from_slice(&3.25f64.to_le_bytes());
        b.extend_from_slice(&62i16.to_le_bytes());
        b.extend_from_slice(&5i16.to_le_bytes());
        push_str(&mut b, 0, "EOF");
        let tags = parse(&b).unwrap();
        assert_eq!(tags[1], Tag::f(10, 3.25));
        assert_eq!(tags[2], Tag::i(62, 5));
    }

    #[test]
    fn malformed_inputs() {
        assert!(parse(b"").is_err());
        assert!(parse(b"abc\nxyz\n").is_err());
        assert!(parse(b"0\n").is_err());
        let mut b = BINARY_SENTINEL.to_vec();
        b.extend_from_slice(&10i16.to_le_bytes());
        b.extend_from_slice(&[1, 2, 3]);
        assert!(parse(&b).is_err());
        // Bad numbers become 0 rather than failing.
        let t = parse(b"10\nnotanumber\n0\nEOF\n").unwrap();
        assert_eq!(t[0].f64(), 0.0);
    }

    #[test]
    fn unicode_escapes_and_latin1() {
        let t = parse(b"1\nCaf\\U+00E9\n0\nEOF\n").unwrap();
        assert_eq!(t[0].str(), "Café");
        let t = parse(b"1\nCaf\xe9\n0\nEOF\n").unwrap();
        assert_eq!(t[0].str(), "Café");
    }

    #[test]
    fn surrogate_pair_escapes() {
        let t = parse(b"1\n\\U+D83D\\U+DE00 \\u+56fe\\U+53f7\n0\nEOF\n").unwrap();
        assert_eq!(t[0].str(), "😀 \\u+56fe号");
    }

    #[test]
    fn hostile_escapes_stay_literal() {
        for s in [
            "\\U+",
            "\\U",
            "\\",
            "a\\U+ZZZZ",
            "\\U+12",
            "\\U+D800",
            "\\U+DC00",
            "\\U+D800\\U+0041",
            "\\U+D83D\\U+DE",
            "\\U++123",
            "\\U+-123",
            "x\\U+12\u{e9}4",
        ] {
            let t = parse(format!("1\n{s}\n0\nEOF\n").as_bytes()).unwrap();
            let expect = if s == "\\U+D800\\U+0041" { "\\U+D800A" } else { s };
            assert_eq!(t[0].str(), expect, "{s:?}");
        }
    }

    #[test]
    fn writes_non_ascii_as_escapes() {
        let tags = vec![Tag::s(1, "图号 Ä°ø 😀\r\nend"), Tag::s(1000, "Ø"), Tag::s(0, "EOF")];
        let text = write_ascii(&tags);
        assert!(text.is_ascii());
        assert!(text.contains("\\U+56FE\\U+53F7 \\U+00C4\\U+00B0\\U+00F8 \\U+D83D\\U+DE00  end\r\n"));
        let back = parse(text.as_bytes()).unwrap();
        assert_eq!(back[0].str(), "图号 Ä°ø 😀  end");
        assert_eq!(back[1].str(), "Ø");
    }

    #[test]
    fn binary_strings_unescape() {
        let mut b = BINARY_SENTINEL.to_vec();
        b.extend_from_slice(&1i16.to_le_bytes());
        b.extend_from_slice(b"\\U+56FE\\U+53F7\0");
        b.extend_from_slice(&0i16.to_le_bytes());
        b.extend_from_slice(b"EOF\0");
        assert_eq!(parse(&b).unwrap()[0].str(), "图号");
    }

    /// Every value kind, a header with a code page (a pre-2007 version when the strings are
    /// escaped, R2018 when they are UTF-8), and strings the decoding has to get right.
    fn stream_sample(utf8: bool) -> Vec<Tag> {
        vec![
            Tag::s(0, "SECTION"),
            Tag::s(2, "HEADER"),
            Tag::s(9, "$ACADVER"),
            Tag::s(1, if utf8 { "AC1032" } else { "AC1015" }),
            Tag::s(9, "$DWGCODEPAGE"),
            Tag::s(3, "ANSI_1252"),
            Tag::s(0, "ENDSEC"),
            Tag::s(0, "SECTION"),
            Tag::s(2, "ENTITIES"),
            Tag::s(0, "TEXT"),
            Tag::s(5, "1F"),
            Tag::f(10, -3.25),
            Tag::i(62, 5),
            Tag::i(90, 70_000),
            Tag::i(160, 1 << 40),
            Tag::new(290, Value::Bool(true)),
            Tag::new(310, Value::Hex("00FFA5".into())),
            Tag::s(1, "图号 Café"),
            Tag::i(1071, -2),
            Tag::s(0, "ENDSEC"),
            Tag::s(0, "EOF"),
        ]
    }

    fn stream_in_chunks(bytes: &[u8], sizes: &[usize]) -> Result<Vec<Tag>> {
        use std::io::Write;
        let mut s = TagStream::new(MAX_TAGS, usize::MAX);
        let mut p = 0;
        let mut k = 0;
        while p < bytes.len() {
            let n = sizes[k % sizes.len()].max(1).min(bytes.len() - p);
            s.write_all(&bytes[p..p + n]).map_err(|_| DxfError::Eof)?;
            p += n;
            k += 1;
        }
        s.finish()
    }

    #[test]
    fn streamed_binary_matches_the_batch_parse_whatever_the_chunking() {
        for utf8 in [false, true] {
            let bytes = write_binary(&stream_sample(utf8), utf8);
            let expected = parse(&bytes).unwrap();
            assert_eq!(expected, stream_sample(utf8));
            for sizes in [&[bytes.len()][..], &[1], &[2], &[3, 1, 7, 2, 5], &[13], &[64 * 1024]] {
                assert_eq!(stream_in_chunks(&bytes, sizes).unwrap(), expected, "utf8 = {utf8}, chunks {sizes:?}");
            }
        }
    }

    #[test]
    fn streamed_binary_decodes_the_code_page_across_a_split_record() {
        use std::io::Write;
        // The code page (ANSI_1252) comes from the header; the string "Café" is one Latin-1 byte
        // for the é, with the terminator in the next chunk.
        let mut b = BINARY_SENTINEL.to_vec();
        let push_str = |b: &mut Vec<u8>, code: i16, s: &[u8]| {
            b.extend_from_slice(&code.to_le_bytes());
            b.extend_from_slice(s);
            b.push(0);
        };
        push_str(&mut b, 9, b"$ACADVER");
        push_str(&mut b, 1, b"AC1015");
        push_str(&mut b, 9, b"$DWGCODEPAGE");
        push_str(&mut b, 3, b"ANSI_1252");
        let cut = b.len() + 2 + 4;
        push_str(&mut b, 1, b"Caf\xe9");
        push_str(&mut b, 0, b"EOF");
        let mut s = TagStream::new(MAX_TAGS, usize::MAX);
        s.write_all(&b[..cut]).unwrap();
        assert_eq!(s.tags().len(), 4, "the cut string is held back");
        s.write_all(&b[cut..]).unwrap();
        let tags = s.finish().unwrap();
        assert_eq!(tags, parse(&b).unwrap());
        assert_eq!(tags[4].str(), "Café");
    }

    #[test]
    fn streamed_binary_limits_and_malformed_input() {
        use std::io::Write;
        let bytes = write_binary(&stream_sample(true), true);
        // Group-code limit: the write past it fails, and so does everything after.
        let mut s = TagStream::new(3, usize::MAX);
        assert!(s.write_all(&bytes).is_err());
        assert!(s.write_all(b"x").is_err());
        assert!(matches!(s.finish(), Err(DxfError::TooLarge { tags: 4, limit: 3 })));
        // Byte limit.
        let mut s = TagStream::new(MAX_TAGS, 40);
        assert!(s.write_all(&bytes).is_err());
        assert!(matches!(s.finish(), Err(DxfError::TooLong { limit: 40, .. })));
        // A stream that ends inside a record.
        let mut s = TagStream::new(MAX_TAGS, usize::MAX);
        s.write_all(&bytes[..bytes.len() - 2]).unwrap();
        assert!(matches!(s.finish(), Err(DxfError::Eof)));
        // Not DXF at all, and nothing at all.
        let mut s = TagStream::new(MAX_TAGS, usize::MAX);
        assert!(s.write_all(b"hello\nworld\n").is_err());
        assert!(matches!(s.finish(), Err(DxfError::BadCode { line: 1, .. })));
        assert!(matches!(TagStream::new(MAX_TAGS, usize::MAX).finish(), Err(DxfError::NotDxf)));
        let mut s = TagStream::new(MAX_TAGS, usize::MAX);
        s.write_all(&BINARY_SENTINEL[..5]).unwrap();
        assert!(matches!(s.finish(), Err(DxfError::NotDxf)));
        // Bytes after EOF are ignored, as the batch parser ignores them.
        let mut s = TagStream::new(MAX_TAGS, usize::MAX);
        s.write_all(&bytes).unwrap();
        s.write_all(b"trailing garbage").unwrap();
        assert_eq!(s.finish().unwrap(), stream_sample(true));
        // A string record with no terminator in sight is an error, not a record to wait for.
        let mut s = TagStream::new(MAX_TAGS, usize::MAX);
        let mut long = BINARY_SENTINEL.to_vec();
        long.extend_from_slice(&1i16.to_le_bytes());
        long.extend(std::iter::repeat_n(b'a', MAX_RECORD + 1));
        assert!(s.write_all(&long).is_err());
        assert!(matches!(s.finish(), Err(DxfError::Eof)));
    }

    /// ASCII text: a pre-2007 header with a code page, Windows-1252 bytes, `\U+` escapes, CRLF
    /// and LF line ends, blank lines, and a value that is only whitespace.
    fn ascii_sample(crlf: bool) -> Vec<u8> {
        let nl = if crlf { "\r\n" } else { "\n" };
        let lines: &[&[u8]] = &[
            b"  0",
            b"SECTION",
            b"  2",
            b"HEADER",
            b"  9",
            b"$ACADVER",
            b"  1",
            b"AC1015",
            b"  9",
            b"$DWGCODEPAGE",
            b"  3",
            b"ANSI_1252",
            b"  0",
            b"ENDSEC",
            b"",
            b"  0",
            b"SECTION",
            b"  2",
            b"ENTITIES",
            b"  0",
            b"TEXT",
            b"  5",
            b"1F",
            b" 10",
            b"-3.25",
            b" 62",
            b"5",
            b" 90",
            b"70000",
            b"290",
            b"1",
            b"310",
            b"00FFA5",
            b"  1",
            b"Caf\xe9 \\U+56FE",
            b"  3",
            b"   ",
            b"1071",
            b"-2",
            b"  0",
            b"ENDSEC",
            b"  0",
            b"EOF",
        ];
        let mut out = Vec::new();
        for l in lines {
            out.extend_from_slice(l);
            out.extend_from_slice(nl.as_bytes());
        }
        out
    }

    #[test]
    fn streamed_ascii_matches_the_batch_parse_whatever_the_chunking() {
        for crlf in [false, true] {
            let bytes = ascii_sample(crlf);
            let expected = parse(&bytes).unwrap();
            assert_eq!(expected.iter().rev().find(|t| t.code == 1).map(Tag::str).as_deref(), Some("Café 图"));
            assert_eq!(expected.iter().rev().find(|t| t.code == 3).map(Tag::str).as_deref(), Some("   "), "a string value keeps its spaces");
            assert_eq!(expected.iter().find(|t| t.code == 310).map(Tag::str).as_deref(), Some("00FFA5"));
            for sizes in [&[bytes.len()][..], &[1], &[2], &[3, 1, 7, 2, 5], &[13], &[64 * 1024]] {
                assert_eq!(stream_in_chunks(&bytes, sizes).unwrap(), expected, "crlf = {crlf}, chunks {sizes:?}");
            }
            // UTF-8 text with the same header (a writer that ignored the code page) reads as UTF-8,
            // as `parse` reads it.
            let utf8 = String::from_utf8(bytes.iter().map(|&b| if b == 0xe9 { b'e' } else { b }).collect()).unwrap().replace("Cafe", "Café 😀");
            assert_eq!(stream_in_chunks(utf8.as_bytes(), &[5]).unwrap(), parse(utf8.as_bytes()).unwrap());
        }
    }

    #[test]
    fn streamed_ascii_ends_and_fails_like_the_batch_parse() {
        use std::io::Write;
        // A last line without its newline still counts.
        let text = b"  0\nSECTION\n  2\nENTITIES\n  0\nENDSEC\n  0\nEOF";
        assert_eq!(stream_in_chunks(text, &[4]).unwrap(), parse(text).unwrap());
        // A code line without its value: unexpected end of file.
        let cut = b"  0\nSECTION\n  2\n";
        assert!(matches!(parse(cut), Err(DxfError::Eof)));
        assert!(matches!(stream_in_chunks(cut, &[3]), Err(DxfError::Eof)));
        // A bad group code, reported with its line number.
        let bad = b"  0\nSECTION\n\nabc\nx\n";
        let want = parse(bad).unwrap_err().to_string();
        assert_eq!(want, "line 4: bad group code `abc`");
        let mut s = TagStream::new(MAX_TAGS, usize::MAX);
        assert!(s.write_all(bad).is_err());
        assert_eq!(s.finish().unwrap_err().to_string(), want);
        // Nothing after EOF matters; a limit on group codes or bytes applies as in binary.
        let mut s = TagStream::new(MAX_TAGS, usize::MAX);
        s.write_all(b"  0\nEOF\n  0\nSECTION\n").unwrap();
        assert_eq!(s.finish().unwrap(), vec![Tag::s(0, "EOF")]);
        let mut s = TagStream::new(2, usize::MAX);
        assert!(s.write_all(b"  0\nSECTION\n  2\nENTITIES\n  0\nENDSEC\n").is_err());
        assert!(matches!(s.finish(), Err(DxfError::TooLarge { tags: 3, limit: 2 })));
        // A line that never ends is not a line to wait for.
        let mut s = TagStream::new(MAX_TAGS, usize::MAX);
        let mut long = b"  0\n".to_vec();
        long.extend(std::iter::repeat_n(b'a', MAX_RECORD + 1));
        assert!(s.write_all(&long).is_err());
        assert!(matches!(s.finish(), Err(DxfError::Eof)));
        // A binary record with no terminator in sight, likewise.
        let mut s = TagStream::new(MAX_TAGS, usize::MAX);
        let mut long = BINARY_SENTINEL.to_vec();
        long.extend_from_slice(&1i16.to_le_bytes());
        long.extend(std::iter::repeat_n(b'a', MAX_RECORD + 1));
        assert!(s.write_all(&long).is_err());
        assert!(matches!(s.finish(), Err(DxfError::Eof)));
    }
}
