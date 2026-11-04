//! Reads a plotted PDF back for tests, with craft-pdf's reader: pages, media boxes, decoded
//! content, transparency graphics states, link annotations, fonts, page labels and document
//! information.
#![allow(dead_code)]

use std::ops::Deref;

use craft_pdf::syntax::object::{Array, Dict, Name, Stream, String as PdfString};

/// What a reader sees in a PDF.
pub struct Doc {
    pub pages: Vec<Page>,
    /// Page labels, one per page (empty when the file has none).
    pub labels: Vec<String>,
    pub title: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    /// Optional content groups (PDF layers) in `/OCProperties` order.
    pub ocgs: Vec<Ocg>,
    /// The catalog's `/PageMode`.
    pub page_mode: Option<String>,
}

pub struct Ocg {
    pub name: String,
    /// Listed in the default configuration's `/OFF` array (starts hidden).
    pub off: bool,
}

pub struct Page {
    /// `[x0 y0 x1 y1]` in points.
    pub media: [f64; 4],
    /// The decoded content stream(s), as text (binary string bytes replaced).
    pub content: String,
    /// The decoded content stream(s) as bytes.
    pub content_bytes: Vec<u8>,
    /// Whether a content stream is compressed (has a filter).
    pub compressed: bool,
    /// Stroke and fill alpha (`/CA`, `/ca`) of each graphics state in the page's resources.
    pub alphas: Vec<(f64, f64)>,
    pub links: Vec<Link>,
    pub fonts: Vec<Font>,
    /// The page's marked-content properties: resource name and the name of the optional
    /// content group it refers to.
    pub properties: Vec<(String, String)>,
}

pub struct Link {
    /// `[x0 y0 x1 y1]` in points.
    pub rect: [f64; 4],
    pub uri: String,
}

pub struct Font {
    pub subtype: String,
    pub base_font: String,
    pub encoding: Option<String>,
    /// The descendant font's subtype (Type0 fonts).
    pub descendant: Option<String>,
    /// The descendant font's `/CIDToGIDMap` name.
    pub cid_to_gid: Option<String>,
    /// Glyph widths (`/W` of the descendant font): code and width.
    pub widths: Vec<(u32, f64)>,
    /// The decoded `ToUnicode` CMap.
    pub to_unicode: Option<String>,
    /// The embedded TrueType file (`FontFile2`).
    pub font_file: Option<Vec<u8>>,
}

/// A PDF text string: UTF-16BE with a byte order mark, else one byte per character.
pub fn text_string(b: &[u8]) -> String {
    match b.strip_prefix(&[0xFE, 0xFF]) {
        Some(u) => String::from_utf16_lossy(&u.chunks(2).map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect::<Vec<_>>()),
        None => b.iter().map(|c| char::from(*c)).collect(),
    }
}

fn name(d: &Dict<'_>, key: &[u8]) -> Option<String> {
    d.get::<Name<'_>>(key).map(|n| n.as_str().to_string())
}

fn rect(a: &Array<'_>) -> [f64; 4] {
    let v: Vec<f64> = a.iter::<f64>().collect();
    assert_eq!(v.len(), 4, "a rectangle: {v:?}");
    [v[0], v[1], v[2], v[3]]
}

fn decoded(s: &Stream<'_>) -> Vec<u8> {
    s.decoded().expect("a stream that decodes").to_vec()
}

fn font(d: &Dict<'_>) -> Font {
    let descendant = d.get::<Array<'_>>(b"DescendantFonts").and_then(|a| a.iter::<Dict<'_>>().next());
    let font_file =
        descendant.as_ref().unwrap_or(d).get::<Dict<'_>>(b"FontDescriptor").and_then(|fd| fd.get::<Stream<'_>>(b"FontFile2")).map(|s| decoded(&s));
    let mut widths = Vec::new();
    if let Some(w) = descendant.as_ref().and_then(|c| c.get::<Array<'_>>(b"W")) {
        let mut it = w.flex_iter();
        while let Some(first) = it.next::<u32>() {
            if let Some(list) = it.next::<Array<'_>>() {
                widths.extend(list.iter::<f64>().enumerate().map(|(i, v)| (first + i as u32, v)));
            } else {
                let (last, v) = (it.next::<u32>().expect("a /W range end"), it.next::<f64>().expect("a /W range width"));
                widths.extend((first..=last).map(|c| (c, v)));
            }
        }
    }
    Font {
        encoding: name(d, b"Encoding"),
        cid_to_gid: descendant.as_ref().and_then(|c| name(c, b"CIDToGIDMap")),
        widths,
        subtype: name(d, b"Subtype").unwrap_or_default(),
        base_font: name(d, b"BaseFont").unwrap_or_default(),
        descendant: descendant.as_ref().and_then(|c| name(c, b"Subtype")),
        to_unicode: d.get::<Stream<'_>>(b"ToUnicode").map(|s| String::from_utf8_lossy(&decoded(&s)).into_owned()),
        font_file,
    }
}

/// Read a PDF; panics when it doesn't parse.
pub fn read(bytes: &[u8]) -> Doc {
    assert!(bytes.starts_with(b"%PDF-"), "a PDF header");
    let pdf = craft_pdf::open_slice(bytes, None).expect("the PDF opens");
    let mut pages = Vec::new();
    for page in pdf.pages().iter() {
        let m = page.media_box();
        let raw = page.raw();
        let resources = raw.get::<Dict<'_>>(b"Resources");
        let dicts = |key: &[u8]| -> Vec<Dict<'_>> {
            let Some(d) = resources.as_ref().and_then(|r| r.get::<Dict<'_>>(key)) else { return Vec::new() };
            let keys: Vec<Vec<u8>> = d.keys().map(|k| k.deref().to_vec()).collect();
            keys.iter().filter_map(|k| d.get::<Dict<'_>>(k.as_slice())).collect()
        };
        let alphas = dicts(b"ExtGState").iter().map(|g| (g.get::<f64>(b"CA").unwrap_or(1.0), g.get::<f64>(b"ca").unwrap_or(1.0))).collect();
        let fonts = dicts(b"Font").iter().map(font).collect();
        let properties = match resources.as_ref().and_then(|r| r.get::<Dict<'_>>(b"Properties")) {
            Some(d) => d
                .keys()
                .filter_map(|k| {
                    let ocg = d.get::<Dict<'_>>(k.deref())?;
                    Some((k.as_str().to_string(), ocg.get::<PdfString<'_>>(b"Name").map(|n| text_string(n.as_bytes())).unwrap_or_default()))
                })
                .collect(),
            None => Vec::new(),
        };
        let links = raw
            .get::<Array<'_>>(b"Annots")
            .map(|a| a.iter::<Dict<'_>>().collect::<Vec<_>>())
            .unwrap_or_default()
            .iter()
            .filter(|a| name(a, b"Subtype").as_deref() == Some("Link"))
            .map(|a| Link {
                rect: rect(&a.get::<Array<'_>>(b"Rect").expect("a link rectangle")),
                uri: a
                    .get::<Dict<'_>>(b"A")
                    .and_then(|act| act.get::<PdfString<'_>>(b"URI"))
                    .map(|s| String::from_utf8_lossy(s.as_bytes()).into_owned())
                    .unwrap_or_default(),
            })
            .collect();
        let bytes = page.page_stream().expect("a decodable page content stream");
        // The writer flips its y-down surface back, so the plot area is drawn in PDF points.
        let content = String::from_utf8_lossy(bytes).into_owned();
        if let Some(ctm) = clip_ctm(&content) {
            assert!(ctm.iter().zip(IDENTITY).all(|(a, b)| (a - b).abs() < 1e-6), "the plot area is drawn under {ctm:?}, not in page space");
        }
        pages.push(Page {
            media: [m.x0, m.y0, m.x1, m.y1],
            content,
            content_bytes: bytes.to_vec(),
            compressed: raw
                .get::<Stream<'_>>(b"Contents")
                .map(|c| vec![c])
                .or_else(|| raw.get::<Array<'_>>(b"Contents").map(|a| a.iter::<Stream<'_>>().collect()))
                .unwrap_or_default()
                .iter()
                .any(|c| c.dict().contains_key(b"Filter")),
            alphas,
            links,
            fonts,
            properties,
        });
    }
    let catalog = pdf.xref().get::<Dict<'_>>(pdf.xref().root_id()).expect("a catalog");
    let mut starts: Vec<(usize, String)> = Vec::new();
    if let Some(nums) = catalog.get::<Dict<'_>>(b"PageLabels").and_then(|l| l.get::<Array<'_>>(b"Nums")) {
        let mut it = nums.flex_iter();
        while let Some(i) = it.next::<i64>() {
            let d = it.next::<Dict<'_>>().expect("a page label dictionary");
            starts.push((i as usize, d.get::<PdfString<'_>>(b"P").map(|s| text_string(s.as_bytes())).unwrap_or_default()));
        }
    }
    let labels = if starts.is_empty() {
        Vec::new()
    } else {
        (0..pages.len()).map(|i| starts.iter().rev().find(|(s, _)| *s <= i).map(|(_, p)| p.clone()).unwrap_or_default()).collect()
    };
    let mut ocgs = Vec::new();
    if let Some(oc) = catalog.get::<Dict<'_>>(b"OCProperties") {
        let off: Vec<_> = oc
            .get::<Dict<'_>>(b"D")
            .and_then(|d| d.get::<Array<'_>>(b"OFF"))
            .map(|a| a.iter::<Dict<'_>>().filter_map(|g| g.obj_id()).collect())
            .unwrap_or_default();
        for g in oc.get::<Array<'_>>(b"OCGs").map(|a| a.iter::<Dict<'_>>().collect::<Vec<_>>()).unwrap_or_default() {
            ocgs.push(Ocg {
                name: g.get::<PdfString<'_>>(b"Name").map(|n| text_string(n.as_bytes())).unwrap_or_default(),
                off: g.obj_id().is_some_and(|id| off.contains(&id)),
            });
        }
    }
    let page_mode = name(&catalog, b"PageMode");
    let md = pdf.metadata();
    let s = |v: &Option<Vec<u8>>| v.as_deref().map(text_string);
    Doc { labels, title: s(&md.title), creator: s(&md.creator), producer: s(&md.producer), pages, ocgs, page_mode }
}

impl Doc {
    /// The first page's content.
    pub fn content(&self) -> &str {
        &self.pages.first().expect("a page").content
    }
    pub fn media(&self) -> (f64, f64) {
        let m = self.pages.first().expect("a page").media;
        (m[2] - m[0], m[3] - m[1])
    }
}

/// One content operator with its numeric operands.
#[derive(Debug, Clone, PartialEq)]
pub struct Op {
    pub op: String,
    pub nums: Vec<f64>,
    /// Name operands, without the slash.
    pub names: Vec<String>,
}

/// The index just past the string literal opening at `b[i]`, honouring escapes and nesting.
fn skip_string(b: &[char], mut i: usize) -> usize {
    let mut depth = 0;
    while i < b.len() {
        match b[i] {
            '\\' => i += 1,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    i + 1
}

/// The index just past the inline dictionary opening at `b[i]` (`<<`).
fn skip_dict(b: &[char], mut i: usize) -> usize {
    let mut depth = 0;
    while i < b.len() {
        if b[i] == '<' && b.get(i + 1) == Some(&'<') {
            depth += 1;
            i += 1;
        } else if b[i] == '>' && b.get(i + 1) == Some(&'>') {
            depth -= 1;
            i += 1;
            if depth == 0 {
                i += 1;
                break;
            }
        }
        i += 1;
    }
    i
}

/// The operators of content text in order, each with its numeric operands (other operands
/// are skipped).
pub fn ops(content: &str) -> Vec<Op> {
    let b: Vec<char> = content.chars().collect();
    let mut out = Vec::new();
    let mut nums = Vec::new();
    let mut names = Vec::new();
    let mut i = 0;
    let delim = |c: char| c.is_whitespace() || "()<>[]{}/%".contains(c);
    while i < b.len() {
        let c = b[i];
        if c.is_whitespace() || c == '[' || c == ']' {
            i += 1;
        } else if c == '%' {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if c == '(' {
            i = skip_string(&b, i);
        } else if c == '<' && b.get(i + 1) == Some(&'<') {
            // An inline dictionary (marked-content properties): skip to its end.
            i = skip_dict(&b, i);
        } else if c == '<' {
            while i < b.len() && b[i] != '>' {
                i += 1;
            }
            i += 1;
        } else if c == '/' {
            i += 1;
            let start = i;
            while i < b.len() && !delim(b[i]) {
                i += 1;
            }
            names.push(b[start..i].iter().collect());
        } else {
            let start = i;
            while i < b.len() && !delim(b[i]) {
                i += 1;
            }
            let word: String = b[start..i.max(start + 1)].iter().collect();
            i = i.max(start + 1);
            match word.parse::<f64>() {
                Ok(v) => nums.push(v),
                Err(_) => out.push(Op { op: word, nums: std::mem::take(&mut nums), names: std::mem::take(&mut names) }),
            }
        }
    }
    out
}

/// How many times operator `op` occurs.
pub fn count(content: &str, op: &str) -> usize {
    ops(content).iter().filter(|o| o.op == op).count()
}

const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// Apply matrix `m` to the point `(x, y)`.
fn apply(m: [f64; 6], x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

/// The operators of content text with path coordinates (`m`, `l`, `c`, `v`, `y`, `re`) mapped
/// through the current transformation matrix, so they are in page space. A `re` becomes the
/// rectangle between its mapped corners, which holds for the axis-aligned matrices plots use.
pub fn page_ops(content: &str) -> Vec<Op> {
    let mut stack = vec![IDENTITY];
    let mut out = ops(content);
    for o in &mut out {
        let ctm = *stack.last().expect("a graphics state");
        match o.op.as_str() {
            "q" => stack.push(ctm),
            "Q" => {
                stack.pop();
                assert!(!stack.is_empty(), "unbalanced Q");
            }
            "cm" if o.nums.len() == 6 => {
                let m = [o.nums[0], o.nums[1], o.nums[2], o.nums[3], o.nums[4], o.nums[5]];
                *stack.last_mut().expect("a graphics state") = mul(m, ctm);
            }
            "m" | "l" | "c" | "v" | "y" => {
                for [x, y] in o.nums.as_chunks_mut::<2>().0 {
                    (*x, *y) = apply(ctm, *x, *y);
                }
            }
            "re" if o.nums.len() == 4 => {
                let (x0, y0) = apply(ctm, o.nums[0], o.nums[1]);
                let (x1, y1) = apply(ctm, o.nums[0] + o.nums[2], o.nums[1] + o.nums[3]);
                o.nums = vec![x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs()];
            }
            _ => {}
        }
    }
    out
}

/// The current transformation matrix at the first clip (`W`): what the plot area is drawn under.
pub fn clip_ctm(content: &str) -> Option<[f64; 6]> {
    let mut stack = vec![IDENTITY];
    for o in ops(content) {
        let ctm = *stack.last().expect("a graphics state");
        match o.op.as_str() {
            "W" => return Some(ctm),
            "q" => stack.push(ctm),
            "Q" => {
                stack.pop();
            }
            "cm" if o.nums.len() == 6 => {
                let m = [o.nums[0], o.nums[1], o.nums[2], o.nums[3], o.nums[4], o.nums[5]];
                *stack.last_mut().expect("a graphics state") = mul(m, ctm);
            }
            _ => {}
        }
    }
    None
}

/// The clip rectangle in page space: the bounds of the path closed by the first `W n`.
pub fn clip_rect(content: &str) -> Option<[f64; 4]> {
    let all = page_ops(content);
    let end = all.iter().position(|o| o.op == "W")?;
    let start = all[..end].iter().rposition(|o| !matches!(o.op.as_str(), "m" | "l" | "h" | "re")).map_or(0, |i| i + 1);
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for o in &all[start..end] {
        let pts: Vec<(f64, f64)> = match o.op.as_str() {
            "re" => vec![(o.nums[0], o.nums[1]), (o.nums[0] + o.nums[2], o.nums[1] + o.nums[3])],
            "m" | "l" => vec![(o.nums[0], o.nums[1])],
            _ => vec![],
        };
        for (x, y) in pts {
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
    }
    Some(b)
}

/// The operators after the clip (the marks drawn inside the plot area), with path coordinates
/// in page space.
pub fn drawn(content: &str) -> Vec<Op> {
    let all = page_ops(content);
    let after = all.iter().position(|o| o.op == "W").map_or(0, |i| i + 1);
    all.into_iter().skip(after).collect()
}

/// `a × b` for PDF matrices `[a b c d e f]` (apply `a`, then `b`).
pub fn mul(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

/// The glyph-space-to-page matrix of each text-showing operator (`Tj`/`TJ`), in order: the text
/// matrix scaled by the font size, under the current transformation matrix.
pub fn text_matrices(content: &str) -> Vec<[f64; 6]> {
    let mut stack = vec![IDENTITY];
    let (mut tm, mut size) = (IDENTITY, 1.0);
    let mut out = Vec::new();
    for o in ops(content) {
        let ctm = *stack.last().expect("a graphics state");
        match o.op.as_str() {
            "q" => stack.push(ctm),
            "Q" => {
                stack.pop();
            }
            "cm" if o.nums.len() == 6 => {
                let m = [o.nums[0], o.nums[1], o.nums[2], o.nums[3], o.nums[4], o.nums[5]];
                *stack.last_mut().expect("a graphics state") = mul(m, ctm);
            }
            "BT" => tm = IDENTITY,
            "Tm" if o.nums.len() == 6 => tm = [o.nums[0], o.nums[1], o.nums[2], o.nums[3], o.nums[4], o.nums[5]],
            "Tf" => size = o.nums.first().copied().unwrap_or(1.0),
            "Tj" | "TJ" => out.push(mul(mul([size, 0.0, 0.0, size, 0.0, 0.0], tm), ctm)),
            _ => {}
        }
    }
    out
}

/// The string operands of text-showing operators (`Tj`), as bytes, in order.
pub fn shown(content: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < content.len() {
        if content[i] == b'(' {
            let mut s = Vec::new();
            let mut depth = 1;
            i += 1;
            while i < content.len() {
                match content[i] {
                    b'\\' => {
                        i += 1;
                        let c = content.get(i).copied().unwrap_or(b'\\');
                        if c.is_ascii_digit() {
                            let digits: Vec<u8> = content[i..].iter().take(3).take_while(|d| (b'0'..=b'7').contains(d)).copied().collect();
                            s.push(u8::from_str_radix(std::str::from_utf8(&digits).unwrap_or("0"), 8).unwrap_or(0));
                            i += digits.len() - 1;
                        } else {
                            s.push(match c {
                                b'n' => b'\n',
                                b'r' => b'\r',
                                b't' => b'\t',
                                other => other,
                            });
                        }
                    }
                    b'(' => {
                        depth += 1;
                        s.push(b'(');
                    }
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                        s.push(b')');
                    }
                    c => s.push(c),
                }
                i += 1;
            }
            let rest = &content[(i + 1).min(content.len())..];
            let next: Vec<u8> = rest.iter().skip_while(|c| c.is_ascii_whitespace()).take(2).copied().collect();
            if next == b"Tj" {
                out.push(s);
            }
        }
        i += 1;
    }
    out
}

/// The `bfchar` entries of a `ToUnicode` CMap: code and text.
pub fn to_unicode_map(cmap: &str) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for block in cmap.split("beginbfchar").skip(1) {
        let body = block.split("endbfchar").next().unwrap_or("");
        let hex: Vec<&str> = body.split(['<', '>']).map(str::trim).filter(|t| !t.is_empty()).collect();
        for pair in hex.chunks(2) {
            let [code, text] = pair else { continue };
            let units: Vec<u16> = (0..text.len() / 4).filter_map(|i| u16::from_str_radix(&text[i * 4..i * 4 + 4], 16).ok()).collect();
            if let Ok(c) = u32::from_str_radix(code, 16) {
                out.push((c, String::from_utf16_lossy(&units)));
            }
        }
    }
    out
}
