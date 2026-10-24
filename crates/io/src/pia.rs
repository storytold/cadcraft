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

/// One style: its colour (object or RGB, grey, screened), lineweight (`0` = the object's, else
/// a 1-based index into the file's lineweight table, in millimetres), ends and joins.
fn style(nodes: &[(String, Node)], weights: &[f64]) -> PlotStyle {
    let name = value(nodes, "name").unwrap_or("").to_string();
    let mut s = PlotStyle::object(&name);
    s.description = value(nodes, "description").unwrap_or("").to_string();
    let policy = int(nodes, "color_policy").unwrap_or(1);
    if let Some(c) = int(nodes, "color").or_else(|| int(nodes, "mode_color"))
        && c != OBJECT_COLOR
        && c != OBJECT_COLOR2
    {
        let v = c as u32;
        s.color = Some(Rgb(((v >> 16) & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, (v & 0xFF) as u8));
    }
    s.grayscale = policy & 2 != 0;
    s.screening = int(nodes, "screen").map_or(100, |v| v.clamp(0, 100) as u8);
    s.lineweight = match int(nodes, "lineweight") {
        Some(i) if i > 0 => weights.get((i - 1) as usize).map(|mm| (mm * 100.0).round().clamp(0.0, 211.0) as u16),
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
    let named = bytes.get(..60).is_some_and(|h| h.windows(6).any(|w| w == b"STBVER"));
    // Lineweights in millimetres; the file may list them in inches (display units 1).
    let inches = int(&nodes, "custom_lineweight_display_units") == Some(1);
    let mut weights: Vec<(usize, f64)> = block(&nodes, "custom_lineweight_table")
        .unwrap_or(&[])
        .iter()
        .filter_map(|(k, n)| match n {
            Node::Value(v) => Some((k.parse().ok()?, v.trim().parse::<f64>().ok()?)),
            Node::Block(_) => None,
        })
        .filter(|(_, v)| v.is_finite() && *v >= 0.0)
        .collect();
    weights.sort_by_key(|(i, _)| *i);
    let weights: Vec<f64> = weights.into_iter().map(|(_, v)| if inches { v * 25.4 } else { v }).collect();
    let mut styles: Vec<(usize, PlotStyle)> = block(&nodes, "plot_style")
        .unwrap_or(&[])
        .iter()
        .filter_map(|(k, n)| match n {
            Node::Block(b) => Some((k.parse().ok()?, style(b, &weights))),
            Node::Value(_) => None,
        })
        .collect();
    styles.sort_by_key(|(i, _)| *i);
    if styles.is_empty() {
        return Err("the plot style table has no styles".into());
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
        let mut text = String::from(
            "description=\"Test\naci_table_available=TRUE\nscale_factor=1.0\napply_factor=FALSE\ncustom_lineweight_display_units=0\nplot_style{\n",
        );
        for i in 0..255 {
            let (color, lw) = match i {
                0 => ("-1006698496", 11),
                1 => ("-16777216", 0),
                _ => ("-1", 0),
            };
            text.push_str(&format!(
                " {i}{{\n  name=\"Color_{}\n  color={color}\n  color_policy=5\n  screen=100\n  linetype=31\n  lineweight={lw}\n  end_style=4\n  join_style=5\n }}\n",
                i + 1
            ));
        }
        text.push_str("}\ncustom_lineweight_table{\n");
        for (i, w) in [0.0, 0.05, 0.09, 0.1, 0.13, 0.15, 0.18, 0.2, 0.25, 0.3, 0.35, 0.4].iter().enumerate() {
            text.push_str(&format!(" {i}={w}\n"));
        }
        text.push_str("}\n");
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

    #[test]
    fn hostile_files_are_refused_not_panicked_on() {
        assert!(super::read(b"PIAFILEVERSION_2.0,CTBVER1,compress\r\npmzlibcodec\x01\x02", "x.ctb").is_err());
        assert!(super::read(b"hello", "x.ctb").is_err());
        let mut f = file();
        f.truncate(f.len() / 2);
        assert!(super::read(&f, "x.ctb").is_err());
    }
}
