//! Dialog Control Language text.
//!
//! The parser reads tile definitions from one string. It does not open files, draw tiles, or
//! evaluate Lisp. `@include` is kept as a path. Names are case-sensitive, and base tiles and body
//! elements stay in source order.
//!
//! Input is hostile. These limits are checked before the parser stores another byte, token, or
//! nesting level past them:
//!
//! - [`MAX_INPUT_BYTES`]: the source is rejected before it is scanned.
//! - [`MAX_STRING_BYTES`]: one quoted value or one name.
//! - [`MAX_TOKENS`]: words, numbers, strings, punctuation, and comments.
//! - [`MAX_DEPTH`]: open `{` groups.
//!
//! Quoted text decodes the DCL escapes for quotes, backslashes, newlines, and tabs.
//! Any other backslash sequence is an error. `//` and `/* */` comments are ignored outside
//! strings. A diagnostic carries a 1-based line and column.

/// Largest source the parser will scan, in bytes.
pub const MAX_INPUT_BYTES: usize = 1 << 20;
/// Largest stored name or decoded quoted value, in bytes.
pub const MAX_STRING_BYTES: usize = 1 << 16;
/// Most tokens, including comments, that one source may contain.
pub const MAX_TOKENS: usize = 100_000;
/// Most simultaneously open `{` groups.
pub const MAX_DEPTH: usize = 32;

/// One DCL source, in the order it was written.
#[derive(Clone, Debug, PartialEq)]
pub struct Dcl {
    pub items: Vec<Item>,
}

/// A top-level definition or include directive.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Definition(Definition),
    /// The path from `@include`. The file is not opened.
    Include {
        path: String,
        at: Location,
    },
}

/// `name : base [: base…] { … }`.
#[derive(Clone, Debug, PartialEq)]
pub struct Definition {
    pub name: String,
    /// Bases from left to right, after each colon.
    pub bases: Vec<String>,
    pub body: Vec<Element>,
    pub at: Location,
}

/// One attribute or one tile inside a `{` group, in source order.
#[derive(Clone, Debug, PartialEq)]
pub enum Element {
    Attribute { name: String, value: AttributeValue, at: Location },
    Tile(Tile),
}

/// `name;` or `: name { … }`.
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub name: String,
    /// Set when the reference was written with a colon and braces.
    pub braced: bool,
    pub body: Vec<Element>,
    pub at: Location,
}

/// An attribute value. Words keep their spelling, including `true` and `True`.
#[derive(Clone, Debug, PartialEq)]
pub enum AttributeValue {
    Text(String),
    Int(i32),
    Real(f64),
    Word(String),
}

/// A 1-based point in the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location {
    pub line: u32,
    pub column: u32,
}

/// A syntax or limit failure at one source position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub message: String,
    pub line: u32,
    pub column: u32,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for Error {}

/// Parse DCL text into definitions and include paths.
pub fn parse(src: &str) -> Result<Dcl, Error> {
    if src.len() > MAX_INPUT_BYTES {
        return Err(Error { message: format!("DCL input exceeds {MAX_INPUT_BYTES} bytes"), line: 1, column: 1 });
    }
    let mut parser = Parser { src: src.as_bytes(), pos: 0, line: 1, column: 1, tokens: 0, depth: 0 };
    let mut items = Vec::new();
    let mut names = std::collections::HashSet::new();
    loop {
        parser.skip()?;
        if parser.eof() {
            break;
        }
        if parser.peek() == Some(b'@') {
            items.push(parser.include()?);
            continue;
        }
        let definition = parser.definition()?;
        if !names.insert(definition.name.clone()) {
            return Err(Error {
                message: format!("duplicate definition '{}'", definition.name),
                line: definition.at.line,
                column: definition.at.column,
            });
        }
        items.push(Item::Definition(definition));
    }
    Ok(Dcl { items })
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    line: u32,
    column: u32,
    tokens: usize,
    depth: usize,
}

fn is_name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic()
}

fn is_name_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

impl<'a> Parser<'a> {
    fn eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn peek(&self) -> Option<u8> {
        self.peek_at(0)
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.pos.checked_add(offset).and_then(|index| self.src.get(index).copied())
    }

    fn location(&self) -> Location {
        Location { line: self.line, column: self.column }
    }

    fn error(&self, message: impl Into<String>) -> Error {
        Error { message: message.into(), line: self.line, column: self.column }
    }

    /// Move one byte forward. A carriage return, with an optional line feed, is one line break.
    fn bump(&mut self) -> Result<(), Error> {
        let Some(byte) = self.peek() else { return Err(self.error("unexpected end of DCL input")) };
        if byte >= 128 {
            return Err(self.error("DCL input must be ASCII"));
        }
        self.pos += 1;
        if byte == b'\n' {
            self.line = self.line.saturating_add(1);
            self.column = 1;
        } else if byte == b'\r' {
            self.line = self.line.saturating_add(1);
            self.column = 1;
            if self.peek() == Some(b'\n') {
                self.pos += 1;
            }
        } else {
            self.column = self.column.saturating_add(1);
        }
        Ok(())
    }

    fn count_token(&mut self) -> Result<(), Error> {
        if self.tokens >= MAX_TOKENS {
            return Err(self.error(format!("DCL token count exceeds {MAX_TOKENS}")));
        }
        self.tokens += 1;
        Ok(())
    }

    fn skip(&mut self) -> Result<(), Error> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.bump()?,
                Some(b'/') if self.peek_at(1) == Some(b'/') => self.line_comment()?,
                Some(b'/') if self.peek_at(1) == Some(b'*') => self.block_comment()?,
                Some(byte) if byte >= 128 => return Err(self.error("DCL input must be ASCII")),
                _ => return Ok(()),
            }
        }
    }

    fn line_comment(&mut self) -> Result<(), Error> {
        self.count_token()?;
        self.bump()?;
        self.bump()?;
        while let Some(byte) = self.peek() {
            if byte == b'\n' || byte == b'\r' {
                break;
            }
            if byte >= 128 {
                return Err(self.error("DCL input must be ASCII"));
            }
            self.bump()?;
        }
        Ok(())
    }

    fn block_comment(&mut self) -> Result<(), Error> {
        let at = self.location();
        self.count_token()?;
        self.bump()?;
        self.bump()?;
        loop {
            match self.peek() {
                None => return Err(Error { message: "unclosed comment".into(), line: at.line, column: at.column }),
                Some(b'*') if self.peek_at(1) == Some(b'/') => {
                    self.bump()?;
                    self.bump()?;
                    return Ok(());
                }
                Some(byte) if byte >= 128 => return Err(self.error("DCL input must be ASCII")),
                Some(_) => self.bump()?,
            }
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), Error> {
        self.skip()?;
        if self.peek() != Some(byte) {
            return Err(self.error(format!("expected '{}'", char::from(byte))));
        }
        self.count_token()?;
        self.bump()
    }

    fn eat(&mut self, byte: u8) -> Result<bool, Error> {
        self.skip()?;
        if self.peek() == Some(byte) {
            self.count_token()?;
            self.bump()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn text(&self, start: usize, end: usize, at: Location) -> Result<&str, Error> {
        let bytes =
            self.src.get(start..end).ok_or_else(|| Error { message: "DCL input ended inside a token".into(), line: at.line, column: at.column })?;
        std::str::from_utf8(bytes).map_err(|_| Error { message: "DCL input must be ASCII".into(), line: at.line, column: at.column })
    }

    fn identifier(&mut self) -> Result<(String, Location), Error> {
        self.skip()?;
        self.raw_name()
    }

    /// A name at the cursor. This does not skip, so `@include` stays one word.
    fn raw_name(&mut self) -> Result<(String, Location), Error> {
        let at = self.location();
        if !self.peek().is_some_and(is_name_start) {
            return Err(self.error("expected a name"));
        }
        self.count_token()?;
        let start = self.pos;
        self.bump()?;
        while self.peek().is_some_and(is_name_continue) {
            self.bump()?;
        }
        if self.pos.saturating_sub(start) > MAX_STRING_BYTES {
            return Err(Error { message: format!("DCL name exceeds {MAX_STRING_BYTES} bytes"), line: at.line, column: at.column });
        }
        Ok((self.text(start, self.pos, at)?.to_string(), at))
    }

    fn include(&mut self) -> Result<Item, Error> {
        let at = self.location();
        self.expect(b'@')?;
        let (word, word_at) = self.raw_name()?;
        if word != "include" {
            return Err(Error { message: "expected include".into(), line: word_at.line, column: word_at.column });
        }
        Ok(Item::Include { path: self.string()?, at })
    }

    fn definition(&mut self) -> Result<Definition, Error> {
        let (name, at) = self.identifier()?;
        self.expect(b':')?;
        let mut bases = Vec::new();
        loop {
            let (base, _) = self.identifier()?;
            bases.push(base);
            if self.eat(b':')? {
                continue;
            }
            break;
        }
        self.expect(b'{')?;
        let body = self.body()?;
        self.expect(b'}')?;
        Ok(Definition { name, bases, body, at })
    }

    /// The elements of one `{` group. `depth` stops the call from nesting past [`MAX_DEPTH`].
    fn body(&mut self) -> Result<Vec<Element>, Error> {
        if self.depth >= MAX_DEPTH {
            return Err(self.error(format!("DCL nesting exceeds {MAX_DEPTH}")));
        }
        self.depth += 1;
        let mut elements = Vec::new();
        loop {
            self.skip()?;
            let element = match self.peek() {
                None | Some(b'}') => break,
                Some(b':') => self.tile()?,
                Some(byte) if is_name_start(byte) => self.attribute_or_reference()?,
                Some(byte) if byte >= 128 => return Err(self.error("DCL input must be ASCII")),
                Some(_) => return Err(self.error("expected an attribute or a tile")),
            };
            elements.push(element);
        }
        self.depth -= 1;
        Ok(elements)
    }

    fn tile(&mut self) -> Result<Element, Error> {
        self.expect(b':')?;
        let (name, at) = self.identifier()?;
        self.expect(b'{')?;
        let nested = self.body()?;
        self.expect(b'}')?;
        Ok(Element::Tile(Tile { name, braced: true, body: nested, at }))
    }

    fn attribute_or_reference(&mut self) -> Result<Element, Error> {
        let (name, at) = self.identifier()?;
        self.skip()?;
        if self.peek() == Some(b'=') {
            self.expect(b'=')?;
            let value = self.value()?;
            self.expect(b';')?;
            Ok(Element::Attribute { name, value, at })
        } else if self.peek() == Some(b';') {
            self.expect(b';')?;
            Ok(Element::Tile(Tile { name, braced: false, body: Vec::new(), at }))
        } else {
            Err(self.error("expected '=' or ';'"))
        }
    }

    fn value(&mut self) -> Result<AttributeValue, Error> {
        self.skip()?;
        match self.peek() {
            Some(b'"') => Ok(AttributeValue::Text(self.string()?)),
            Some(b'-') | Some(b'0'..=b'9') => self.number(),
            Some(byte) if is_name_start(byte) => {
                let (word, at) = self.identifier()?;
                if word.contains('_') {
                    return Err(Error { message: "reserved words use only letters and digits".into(), line: at.line, column: at.column });
                }
                Ok(AttributeValue::Word(word))
            }
            Some(byte) if byte >= 128 => Err(self.error("DCL input must be ASCII")),
            _ => Err(self.error("expected an attribute value")),
        }
    }

    fn number(&mut self) -> Result<AttributeValue, Error> {
        let at = self.location();
        self.count_token()?;
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.bump()?;
        }
        if !self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            return Err(Error { message: "expected a number".into(), line: at.line, column: at.column });
        }
        self.take_digits()?;
        let mut real = false;
        if self.peek() == Some(b'.') {
            real = true;
            self.bump()?;
            if self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.take_digits()?;
            }
        }
        if self.pos.saturating_sub(start) > MAX_STRING_BYTES {
            return Err(Error { message: format!("DCL number exceeds {MAX_STRING_BYTES} bytes"), line: at.line, column: at.column });
        }
        let literal = self.text(start, self.pos, at)?;
        if real {
            let value = literal.parse::<f64>().map_err(|_| Error { message: "invalid real".into(), line: at.line, column: at.column })?;
            if !value.is_finite() {
                return Err(Error { message: "real is out of range".into(), line: at.line, column: at.column });
            }
            Ok(AttributeValue::Real(value))
        } else {
            let value = literal.parse::<i32>().map_err(|_| Error { message: "integer is out of range".into(), line: at.line, column: at.column })?;
            Ok(AttributeValue::Int(value))
        }
    }

    fn take_digits(&mut self) -> Result<(), Error> {
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.bump()?;
        }
        Ok(())
    }

    fn string(&mut self) -> Result<String, Error> {
        self.skip()?;
        let at = self.location();
        if self.peek() != Some(b'"') {
            return Err(self.error("expected a string"));
        }
        self.count_token()?;
        self.bump()?;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err(Error { message: "unclosed string".into(), line: at.line, column: at.column }),
                Some(b'"') => {
                    self.bump()?;
                    return Ok(out);
                }
                Some(b'\\') => self.string_escape(&mut out, at)?,
                Some(byte) if byte >= 128 => return Err(self.error("DCL input must be ASCII")),
                Some(byte) => {
                    let text_byte = if byte == b'\r' && self.peek_at(1) == Some(b'\n') { b'\n' } else { byte };
                    if text_byte.is_ascii_control() && !matches!(text_byte, b'\t' | b'\n') {
                        return Err(self.error("invalid control character in DCL string"));
                    }
                    self.bump()?;
                    self.push_text(&mut out, text_byte, at)?;
                }
            }
        }
    }

    fn string_escape(&mut self, out: &mut String, open: Location) -> Result<(), Error> {
        let at = self.location();
        self.bump()?;
        let Some(next) = self.peek() else {
            return Err(Error { message: "unclosed string".into(), line: open.line, column: open.column });
        };
        if next >= 128 {
            return Err(self.error("DCL input must be ASCII"));
        }
        self.bump()?;
        let decoded = match next {
            b'\\' => b'\\',
            b'"' => b'"',
            b'n' => b'\n',
            b't' => b'\t',
            _ => return Err(Error { message: "unknown string escape".into(), line: at.line, column: at.column }),
        };
        self.push_text(out, decoded, open)
    }

    fn push_text(&self, out: &mut String, byte: u8, at: Location) -> Result<(), Error> {
        if out.len() >= MAX_STRING_BYTES {
            return Err(Error { message: format!("DCL string exceeds {MAX_STRING_BYTES} bytes"), line: at.line, column: at.column });
        }
        out.push(char::from(byte));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn parsed(src: &str) -> Dcl {
        match parse(src) {
            Ok(dcl) => dcl,
            Err(e) => panic!("{e}"),
        }
    }

    fn show(elements: &[Element], indent: usize, out: &mut String) {
        let pad = "  ".repeat(indent);
        for element in elements {
            match element {
                Element::Attribute { name, value, .. } => {
                    out.push_str(&pad);
                    out.push_str(name);
                    out.push('=');
                    out.push_str(&match value {
                        AttributeValue::Text(text) => format!("text:{text}"),
                        AttributeValue::Int(number) => format!("int:{number}"),
                        AttributeValue::Real(number) => format!("real:{number}"),
                        AttributeValue::Word(word) => format!("word:{word}"),
                    });
                    out.push('\n');
                }
                Element::Tile(tile) => {
                    out.push_str(&pad);
                    out.push_str(if tile.braced { "tile " } else { "ref " });
                    out.push_str(&tile.name);
                    out.push('\n');
                    show(&tile.body, indent + 1, out);
                }
            }
        }
    }

    fn outline(dcl: &Dcl) -> String {
        let mut out = String::new();
        for item in &dcl.items {
            match item {
                Item::Include { path, .. } => {
                    out.push_str("include ");
                    out.push_str(path);
                    out.push('\n');
                }
                Item::Definition(definition) => {
                    out.push_str("def ");
                    out.push_str(&definition.name);
                    for base in &definition.bases {
                        out.push_str(" : ");
                        out.push_str(base);
                    }
                    out.push('\n');
                    show(&definition.body, 1, &mut out);
                }
            }
        }
        out
    }

    fn tile_depth(elements: &[Element]) -> usize {
        elements
            .iter()
            .map(|element| match element {
                Element::Tile(tile) => 1 + tile_depth(&tile.body),
                Element::Attribute { .. } => 0,
            })
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn valid_definitions_keep_names_bases_attributes_and_tile_order() {
        let src = r#"
@include "parts.dcl"
proto : tile {
  fixed_height = true;
}
wide : proto : tile {
  width = 12;
}
Big : tile { }
big : tile { }
ask : dialog {
  label = "Ask";
  : column {
    : edit_box {
      label = "Name";
      key = "name";
      value = "a";
      alignment = Left;
    }
    : button {
      label = "Go";
      key = "go";
      is_default = true;
    }
  }
  ok_only;
  initial_focus = "go";
}
"#;
        let dcl = parsed(src);
        assert_eq!(
            outline(&dcl),
            "\
include parts.dcl
def proto : tile
  fixed_height=word:true
def wide : proto : tile
  width=int:12
def Big : tile
def big : tile
def ask : dialog
  label=text:Ask
  tile column
    tile edit_box
      label=text:Name
      key=text:name
      value=text:a
      alignment=word:Left
    tile button
      label=text:Go
      key=text:go
      is_default=word:true
  ref ok_only
  initial_focus=text:go
"
        );
        assert!(parsed("").items.is_empty());
        assert!(parsed("// only a comment\n/* and a block */").items.is_empty());
    }

    #[test]
    fn composition_keeps_base_order_decoded_text_and_ignores_comments() {
        let src = r#"
/* keep
   the bases */
joined : wide : proto { // the line comment is not a tile
  label = "quote \"q\" and \\ slash";
  ratio = 1.5;
}
// hidden : dialog { label = "no"; }
shown : dialog { label = "yes /* inside */"; }
"#;
        assert_eq!(
            outline(&parsed(src)),
            "\
def joined : wide : proto
  label=text:quote \"q\" and \\ slash
  ratio=real:1.5
def shown : dialog
  label=text:yes /* inside */
"
        );
    }

    #[test]
    fn quoted_text_decodes_newline_and_tab() {
        let source = r#"box : dialog { label = "first\nsecond\tthird"; ok_only; }"#;
        assert_eq!(outline(&parsed(source)), "def box : dialog\n  label=text:first\nsecond\tthird\n  ref ok_only\n");
    }

    #[test]
    fn quoted_text_treats_crlf_and_lf_the_same() {
        let lf = "box : dialog {\n  label = \"first\nsecond\";\n  ok_only;\n}";
        let crlf = lf.replace('\n', "\r\n");
        assert_eq!(parsed(&crlf), parsed(lf));
    }

    #[test]
    fn invalid_syntax_reports_line_and_column() {
        let unclosed_text = parse("box : dialog {\n  label = \"open\n}\n").unwrap_err();
        assert_eq!((unclosed_text.line, unclosed_text.column), (2, 11));
        assert!(unclosed_text.message.contains("unclosed string"));

        let unclosed_comment = parse("box : dialog { /*\n  label = \"x\";\n}\n").unwrap_err();
        assert_eq!((unclosed_comment.line, unclosed_comment.column), (1, 16));
        assert!(unclosed_comment.message.contains("unclosed comment"));

        let missing_semicolon = parse("box : dialog {\n  label = \"x\"\n}\n").unwrap_err();
        assert_eq!((missing_semicolon.line, missing_semicolon.column), (3, 1));
        assert!(missing_semicolon.message.contains("expected ';'"));

        let missing_brace = parse("box : dialog {\n  ok_only;\n").unwrap_err();
        assert_eq!((missing_brace.line, missing_brace.column), (3, 1));
        assert!(missing_brace.message.contains("expected '}'"));

        let duplicate = parse("Big : tile { }\nBig : dialog { label = \"x\"; }\n").unwrap_err();
        assert_eq!((duplicate.line, duplicate.column), (2, 1));
        assert!(duplicate.message.contains("duplicate"));

        let unknown_escape = parse("box : dialog {\n  label = \"a\\qb\";\n}\n").unwrap_err();
        assert_eq!((unknown_escape.line, unknown_escape.column), (2, 13));
        assert!(unknown_escape.message.contains("unknown string escape"));
    }

    #[test]
    fn malformed_underscore_in_reserved_word_is_rejected() {
        let error = parse("box : dialog { mode = custom_mode; }").unwrap_err();
        assert_eq!((error.line, error.column), (1, 23));
        assert!(error.message.contains("reserved word"));
    }

    #[test]
    fn malformed_raw_control_characters_in_quoted_text_are_rejected() {
        for control in ['\0', '\x01', '\r', '\x7f'] {
            let source = format!("box : dialog {{\n  label = \"one{control}two\";\n}}");
            let error = parse(&source).unwrap_err();
            assert_eq!((error.line, error.column), (2, 15));
            assert!(error.message.contains("control character"));
        }
    }

    #[test]
    fn input_over_the_limits_is_rejected() {
        let mut exact_bytes = "a : tile { }\n".to_string();
        exact_bytes.push_str(&" ".repeat(MAX_INPUT_BYTES - exact_bytes.len()));
        assert_eq!(exact_bytes.len(), MAX_INPUT_BYTES);
        assert_eq!(parsed(&exact_bytes).items.len(), 1);

        let mut over_bytes = exact_bytes.clone();
        over_bytes.push(' ');
        let size = parse(&over_bytes).unwrap_err();
        assert_eq!((size.line, size.column), (1, 1));
        assert!(size.message.contains(&MAX_INPUT_BYTES.to_string()));

        let mut exact_tokens = String::new();
        for _ in 0..MAX_TOKENS {
            exact_tokens.push_str("/*a*/");
        }
        assert!(parsed(&exact_tokens).items.is_empty());
        exact_tokens.push_str("/*a*/");
        let tokens = parse(&exact_tokens).unwrap_err();
        assert!(tokens.message.contains(&MAX_TOKENS.to_string()));

        let mut exact_text = "a : tile { label = \"".to_string();
        exact_text.push_str(&"b".repeat(MAX_STRING_BYTES));
        exact_text.push_str("\"; }");
        match &parsed(&exact_text).items[0] {
            Item::Definition(definition) => match &definition.body[0] {
                Element::Attribute { value: AttributeValue::Text(text), .. } => assert_eq!(text.len(), MAX_STRING_BYTES),
                other => panic!("attribute, got {other:?}"),
            },
            other => panic!("definition, got {other:?}"),
        }
        let mut over_text = "a : tile { label = \"".to_string();
        over_text.push_str(&"b".repeat(MAX_STRING_BYTES + 1));
        over_text.push_str("\"; }");
        let string_limit = parse(&over_text).unwrap_err();
        assert!(string_limit.message.contains(&MAX_STRING_BYTES.to_string()));

        fn nest(rows: usize) -> String {
            let mut src = String::from("d : dialog {");
            for _ in 0..rows {
                src.push_str(": row {");
            }
            src.push_str("ok_only;");
            for _ in 0..rows {
                src.push('}');
            }
            src.push('}');
            src
        }

        let at_limit = parsed(&nest(MAX_DEPTH - 1));
        match &at_limit.items[0] {
            Item::Definition(definition) => assert_eq!(tile_depth(&definition.body), MAX_DEPTH),
            other => panic!("definition, got {other:?}"),
        }
        let depth = parse(&nest(MAX_DEPTH)).unwrap_err();
        assert!(depth.message.contains(&MAX_DEPTH.to_string()));
    }
}
