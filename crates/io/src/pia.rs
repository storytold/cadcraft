//! Plot style table files as AutoCAD writes them (`.ctb` colour-dependent, `.stb` named): a
//! `PIAFILEVERSION_2.0,CTBVER1,compress` header line, `pmzlibcodec`, two 32-bit words (a check
//! value and the text length), the compressed text length, then zlib-compressed text of nested
//! `key=value` and `name{ … }` blocks. Read into CADCraft's [`PlotStyleTable`] so a drawing whose
//! layouts name the user's own table plots with its colours and lineweights.

use cadcraft_color::Rgb;
use cadcraft_doc::{EndStyle, JoinStyle, PlotStyle, PlotStyleKind, PlotStyleTable};

/// Largest file and decompressed text we accept.
const MAX_FILE: usize = 4 << 20;
const MAX_TEXT: usize = 16 << 20;
/// "Use the object's colour" values of a style's `color`.
const OBJECT_COLOR: i64 = -1;
const OBJECT_COLOR2: i64 = -1_006_632_961;

/// Whether `bytes` look like an AutoCAD plot style table file.
pub fn is_pia(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PIAFILEVERSION")
}

/// A node of the text: a value or a block of children, in file order.
enum Node {
    Value(String),
    Block(Vec<(String, Node)>),
}

/// The decompressed text of a PIA file.
fn text(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_FILE {
        return Err("the plot style table file is too large".into());
    }
    if !is_pia(bytes) {
        return Err("not a plot style table file".into());
    }
    let marker = b"pmzlibcodec";
    let at = bytes.windows(marker.len()).position(|w| w == marker).ok_or("the plot style table is not compressed as expected")?;
    let data = bytes.get(at + marker.len() + 12..).ok_or("the plot style table file is cut short")?;
    let out = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, MAX_TEXT)
        .map_err(|_| "the plot style table can't be decompressed".to_string())?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Parse the nested text. Lines are `key=value` (a value starting with `"` runs to the end of
/// the line, without a closing quote), `key{` opening a block and `}` closing it.
fn parse(text: &str) -> Vec<(String, Node)> {
    let mut stack: Vec<(String, Vec<(String, Node)>)> = vec![(String::new(), Vec::new())];
    for line in text.lines().take(200_000) {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        if l == "}" {
            if stack.len() > 1
                && let Some((name, kids)) = stack.pop()
                && let Some(top) = stack.last_mut()
            {
                top.1.push((name, Node::Block(kids)));
            }
            continue;
        }
        if let Some(name) = l.strip_suffix('{') {
            if stack.len() < 8 {
                stack.push((name.trim().to_string(), Vec::new()));
            }
            continue;
        }
        if let Some((k, v)) = l.split_once('=')
            && let Some(top) = stack.last_mut()
        {
            let v = v.strip_prefix('"').unwrap_or(v);
            top.1.push((k.trim().to_string(), Node::Value(v.to_string())));
        }
    }
    while stack.len() > 1 {
        if let Some((name, kids)) = stack.pop()
            && let Some(top) = stack.last_mut()
        {
            top.1.push((name, Node::Block(kids)));
        }
    }
    stack.pop().map(|(_, k)| k).unwrap_or_default()
}

fn value<'a>(nodes: &'a [(String, Node)], key: &str) -> Option<&'a str> {
    nodes.iter().find_map(|(k, n)| match n {
        Node::Value(v) if k == key => Some(v.as_str()),
        _ => None,
    })
}

fn block<'a>(nodes: &'a [(String, Node)], key: &str) -> Option<&'a [(String, Node)]> {
    nodes.iter().find_map(|(k, n)| match n {
        Node::Block(b) if k == key => Some(b.as_slice()),
        _ => None,
    })
}

fn int(nodes: &[(String, Node)], key: &str) -> Option<i64> {
    value(nodes, key).and_then(|v| v.trim().parse().ok())
}

/// Highest table position kept (lineweights, styles): a colour table has 255 styles and a
/// lineweight table a few dozen; a hostile number doesn't make a huge table.
const MAX_INDEX: usize = 4096;

/// A number at the start of `v` (`0.25`, or `0.25 (custom)` with an appendix after it).
fn leading_number(v: &str) -> Option<f64> {
    let t = v.trim();
    let end = t.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '+' | '-' | 'e' | 'E'))).unwrap_or(t.len());
    t.get(..end)?.parse().ok()
}

/// One style: its colour (object or RGB, grey, screened), lineweight (`0` = the object's, else
/// an index into the file's lineweight table, in millimetres), ends and joins.
fn style(nodes: &[(String, Node)], weights: &[Option<f64>]) -> PlotStyle {
    let name = value(nodes, "name").unwrap_or("").to_string();
    let mut s = PlotStyle::object(&name);
    s.description = value(nodes, "description").unwrap_or("").to_string();
    let policy = int(nodes, "color_policy").unwrap_or(1);
    // `color` says whether the object's colour is used; the plot colour itself is `mode_color`
    // where the file has it (`color` may hold another coding of it).
    let object = |c: i64| c == OBJECT_COLOR || c == OBJECT_COLOR2;
    let plot = match int(nodes, "color") {
        Some(c) if object(c) => None,
        Some(c) => Some(int(nodes, "mode_color").filter(|m| !object(*m)).unwrap_or(c)),
        None => int(nodes, "mode_color").filter(|m| !object(*m)),
    };
    if let Some(c) = plot {
        let v = c as u32;
        s.color = Some(Rgb(((v >> 16) & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, (v & 0xFF) as u8));
    }
    s.grayscale = policy & 2 != 0;
    s.screening = int(nodes, "screen").map_or(100, |v| v.clamp(0, 100) as u8);
    s.lineweight = match int(nodes, "lineweight") {
        Some(i) if i > 0 => {
            usize::try_from(i).ok().and_then(|i| weights.get(i).copied().flatten()).map(|mm| (mm * 100.0).round().clamp(0.0, 211.0) as u16)
        }
        _ => None,
    };
    s.end = match int(nodes, "end_style") {
        Some(0) => EndStyle::Butt,
        Some(1) => EndStyle::Square,
        Some(2) => EndStyle::Round,
        Some(3) => EndStyle::Diamond,
        _ => EndStyle::Object,
    };
    s.join = match int(nodes, "join_style") {
        Some(0) => JoinStyle::Miter,
        Some(1) => JoinStyle::Bevel,
        Some(2) => JoinStyle::Round,
        Some(3) => JoinStyle::Diamond,
        _ => JoinStyle::Object,
    };
    s
}

/// Read a `.ctb` or `.stb` file into a table named `name` (the file's name, as page setups
/// name it).
pub fn read(bytes: &[u8], name: &str) -> Result<PlotStyleTable, String> {
    let t = text(bytes)?;
    let nodes = parse(&t);
    // A named table says it has no colour (ACI) table; both kinds share the CTBVER1 header.
    let named = match value(&nodes, "aci_table_available") {
        Some(v) => v.trim().eq_ignore_ascii_case("FALSE"),
        None => bytes.get(..60).is_some_and(|h| h.windows(6).any(|w| w == b"STBVER")),
    };
    // Lineweights are stored in millimetres; `custom_lineweight_display_units` only says how the
    // plot style editor shows them.
    // Each at its own number: a style's lineweight is that number, a colour style's colour its
    // number + 1, so a missing entry leaves a gap rather than moving the later ones up.
    let mut weights: Vec<Option<f64>> = Vec::new();
    for (k, n) in block(&nodes, "custom_lineweight_table").unwrap_or(&[]) {
        let Node::Value(v) = n else { continue };
        let (Some(i), Some(mm)) = (k.trim().parse::<usize>().ok().filter(|i| *i < MAX_INDEX), leading_number(v)) else { continue };
        if !mm.is_finite() || mm < 0.0 {
            continue;
        }
        if weights.len() <= i {
            weights.resize(i + 1, None);
        }
        if let Some(w) = weights.get_mut(i) {
            *w = Some(mm);
        }
    }
    let mut styles: Vec<(usize, PlotStyle)> = block(&nodes, "plot_style")
        .unwrap_or(&[])
        .iter()
        .filter_map(|(k, n)| match n {
            Node::Block(b) => Some((k.trim().parse::<usize>().ok().filter(|i| *i < MAX_INDEX)?, style(b, &weights))),
            Node::Value(_) => None,
        })
        .collect();
    styles.sort_by_key(|(i, _)| *i);
    styles.dedup_by_key(|(i, _)| *i);
    if styles.is_empty() {
        return Err("the plot style table has no styles".into());
    }
    // A colour table's style n is colour n + 1: a missing one plots as the object.
    if !named {
        let mut at: Vec<(usize, PlotStyle)> = Vec::new();
        for (i, s) in styles {
            while at.len() < i {
                let n = at.len();
                at.push((n, PlotStyle::object(&format!("Color_{}", n + 1))));
            }
            at.push((i, s));
        }
        styles = at;
    }
    let table = PlotStyleTable {
        name: name.to_string(),
        description: value(&nodes, "description").unwrap_or("").to_string(),
        kind: if named { PlotStyleKind::Named } else { PlotStyleKind::ColorDependent },
        styles: styles.into_iter().map(|(_, s)| s).collect(),
    };
    Ok(table.sanitized())
}

#[cfg(test)]
mod tests {
    /// A small table in the file format, made here (a colour-dependent table whose colour 1 is red
    /// at 0.35 mm and colour 2 black at the object's lineweight).
    fn file() -> Vec<u8> {
        made(true, 0)
    }

    /// The test table: colour-dependent (`aci`) or named, lineweights shown in millimetres (0) or
    /// inches (1). Style 0 plots at lineweight index 10, 0.35 mm.
    fn made(aci: bool, units: u8) -> Vec<u8> {
        let mut text = format!(
            "description=\"Test\naci_table_available={}\nscale_factor=1.0\napply_factor=FALSE\ncustom_lineweight_display_units={units}\nplot_style{{\n",
            if aci { "TRUE" } else { "FALSE" }
        );
        let count = if aci { 255 } else { 3 };
        for i in 0..count {
            let (color, lw) = match i {
                0 => ("-1006698496", 10),
                1 => ("-16777216", 0),
                _ => ("-1", 0),
            };
            let name = if aci { format!("Color_{}", i + 1) } else { ["Normal", "Walls", "Hidden"].get(i).copied().unwrap_or("Style").to_string() };
            text.push_str(&format!(
                " {i}{{\n  name=\"{name}\n  color={color}\n  color_policy=5\n  screen=100\n  linetype=31\n  lineweight={lw}\n  end_style=4\n  join_style=5\n }}\n"
            ));
        }
        text.push_str("}\ncustom_lineweight_table{\n");
        for (i, w) in [0.0, 0.05, 0.09, 0.1, 0.13, 0.15, 0.18, 0.2, 0.25, 0.3, 0.35, 0.4].iter().enumerate() {
            text.push_str(&format!(" {i}={w}\n"));
        }
        text.push_str("}\n");
        pack(&text)
    }

    /// Table text in the file's compressed form.
    fn pack(text: &str) -> Vec<u8> {
        let z = miniz_oxide::deflate::compress_to_vec_zlib(text.as_bytes(), 6);
        let mut out = b"PIAFILEVERSION_2.0,CTBVER1,compress\r\npmzlibcodec".to_vec();
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(&(z.len() as u32).to_le_bytes());
        out.extend_from_slice(&z);
        out
    }

    #[test]
    fn a_color_table_file_gives_colours_and_lineweights() {
        let t = super::read(&file(), "Mine.ctb").unwrap();
        assert!(t.is_color_dependent());
        let red = t.by_color(1).unwrap();
        assert_eq!(red.lineweight, Some(35));
        assert_eq!(red.color.map(|c| (c.0, c.1, c.2)), Some((255, 0, 0)));
        let black = t.by_color(2).unwrap();
        assert_eq!(black.color.map(|c| (c.0, c.1, c.2)), Some((0, 0, 0)));
        assert_eq!(black.lineweight, None);
        assert!(t.by_color(3).unwrap().color.is_none());
    }

    /// A named table has the same CTBVER1 header as a colour one; it says `aci_table_available=FALSE`.
    /// Its style names are kept, and a lineweight index reads the table at that index.
    #[test]
    fn a_named_table_is_read_as_named_with_its_style_names() {
        let t = super::read(&made(false, 0), "Mine.stb").unwrap();
        assert!(!t.is_color_dependent());
        let names: Vec<&str> = t.styles.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Walls") && names.contains(&"Hidden"), "{names:?}");
        let normal = t.styles.iter().find(|s| s.name == "Normal").unwrap();
        assert_eq!(normal.lineweight, Some(35), "index 10 is 0.35 mm, not the one before it");
    }

    /// Lineweights shown in inches are still stored in millimetres.
    #[test]
    fn inch_display_units_do_not_scale_stored_lineweights() {
        let t = super::read(&made(true, 1), "Inch.ctb").unwrap();
        assert_eq!(t.by_color(1).unwrap().lineweight, Some(35));
    }

    /// Entries stay at their numbers: a missing lineweight or colour style leaves a gap instead
    /// of moving the later ones; a lineweight with an appendix is read; an explicit plot colour
    /// comes from `mode_color` (#2328 review).
    #[test]
    fn sparse_tables_keep_their_positions() {
        let text = "description=\"Sparse\naci_table_available=TRUE\nplot_style{\n              1{\n  name=\"Color_2\n  color=-1006698496\n  mode_color=-16711936\n  color_policy=1\n  lineweight=2\n }\n              2{\n  name=\"Color_3\n  color=-1\n  lineweight=0\n }\n}\n\
             custom_lineweight_table{\n 0=0.0\n 2=0.50 (custom)\n}\n";
        let t = super::read(&pack(text), "Sparse.ctb").unwrap();
        let first = t.by_color(1).unwrap();
        assert!(first.color.is_none() && first.lineweight.is_none(), "colour 1 has no style: the object's");
        let second = t.by_color(2).unwrap();
        assert_eq!(second.lineweight, Some(50), "lineweight 2 is 0.50 mm");
        assert_eq!(second.color.map(|c| (c.0, c.1, c.2)), Some((0, 255, 0)), "mode_color");
        assert!(t.by_color(3).unwrap().color.is_none());
        assert_eq!(super::leading_number("0.50 (custom)"), Some(0.5));
    }

    #[test]
    fn hostile_files_are_refused_not_panicked_on() {
        assert!(super::read(b"PIAFILEVERSION_2.0,CTBVER1,compress\r\npmzlibcodec\x01\x02", "x.ctb").is_err());
        assert!(super::read(b"hello", "x.ctb").is_err());
        let mut f = file();
        f.truncate(f.len() / 2);
        assert!(super::read(&f, "x.ctb").is_err());
    }
}
