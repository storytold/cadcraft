//! "CADCraft Stroke": an original single-stroke drafting font, designed for CADCraft on a
//! small grid (public domain / CC0 as part of this codebase).
//!
//! Each glyph is `(width, strokes)`. A stroke is a run of points; each point is two characters:
//! an x digit `0..=6` and a y letter where `a` = -2, `c` = 0 (baseline), `g` = 4 (x-height),
//! `i` = 6 (cap height). Strokes are separated by spaces.

pub(crate) const CAP: f64 = 6.0;
pub(crate) const GAP: f64 = 1.6;

pub(crate) fn glyph(c: char) -> Option<(f64, &'static str)> {
    let g: (f64, &str) = match c {
        ' ' => (2.4, ""),
        'A' => (4.0, "0c2i4c 1f3f"),
        'B' => (4.0, "0c0i3i4h4g3f0f 3f4e4d3c0c"),
        'C' => (4.0, "4h3i1i0h0d1c3c4d"),
        'D' => (4.0, "0c0i3i4h4d3c0c"),
        'E' => (4.0, "4i0i0c4c 0f3f"),
        'F' => (4.0, "4i0i0c 0f3f"),
        'G' => (4.0, "4h3i1i0h0d1c3c4d4f2f"),
        'H' => (4.0, "0c0i 4c4i 0f4f"),
        'I' => (2.0, "0i2i 1i1c 0c2c"),
        'J' => (4.0, "4i4d3c1c0d0e"),
        'K' => (4.0, "0c0i 4i0e 1f4c"),
        'L' => (4.0, "0i0c4c"),
        'M' => (4.0, "0c0i2f4i4c"),
        'N' => (4.0, "0c0i4c4i"),
        'O' => (4.0, "1c0d0h1i3i4h4d3c1c"),
        'P' => (4.0, "0c0i3i4h4g3f0f"),
        'Q' => (4.0, "1c0d0h1i3i4h4d3c1c 2e4b"),
        'R' => (4.0, "0c0i3i4h4g3f0f 2f4c"),
        'S' => (4.0, "4h3i1i0h0g1f3f4e4d3c1c0d"),
        'T' => (4.0, "0i4i 2i2c"),
        'U' => (4.0, "0i0d1c3c4d4i"),
        'V' => (4.0, "0i2c4i"),
        'W' => (4.0, "0i1c2f3c4i"),
        'X' => (4.0, "0i4c 0c4i"),
        'Y' => (4.0, "0i2f4i 2f2c"),
        'Z' => (4.0, "0i4i0c4c"),
        '0' => (4.0, "1c0d0h1i3i4h4d3c1c"),
        '1' => (4.0, "1h2i2c 1c3c"),
        '2' => (4.0, "0h1i3i4h4g0c4c"),
        '3' => (4.0, "0h1i3i4h4g3f2f 3f4e4d3c1c0d"),
        '4' => (4.0, "3c3i0e4e"),
        '5' => (4.0, "4i0i0f3f4e4d3c1c0d"),
        '6' => (4.0, "4h3i1i0h0d1c3c4d4e3f1f0e"),
        '7' => (4.0, "0i4i1c"),
        '8' => (4.0, "1f0g0h1i3i4h4g3f1f0e0d1c3c4d4e3f"),
        '9' => (4.0, "4f1f0g0h1i3i4h4d3c1c0d"),
        'a' => (4.0, "4g4c 4f3g1g0f0d1c3c4d"),
        'b' => (4.0, "0i0c 0d1c3c4d4f3g1g0f"),
        'c' => (4.0, "4f3g1g0f0d1c3c4d"),
        'd' => (4.0, "4i4c 4d3c1c0d0f1g3g4f"),
        'e' => (4.0, "0e4e4f3g1g0f0d1c3c4d"),
        'f' => (3.0, "3i2i1h1c 0g3g"),
        'g' => (4.0, "4g4b3a1a0b 4f3g1g0f0d1c3c4d"),
        'h' => (4.0, "0i0c 0f1g3g4f4c"),
        'i' => (1.0, "0c0g 0h0i"),
        'j' => (2.0, "2g2b1a0a 2h2i"),
        'k' => (4.0, "0i0c 3g0d 1e3c"),
        'l' => (2.0, "1i1d2c"),
        'm' => (4.0, "0c0g 0f1g2f2c 2f3g4f4c"),
        'n' => (4.0, "0c0g 0f1g3g4f4c"),
        'o' => (4.0, "1c0d0f1g3g4f4d3c1c"),
        'p' => (4.0, "0g0a 0f1g3g4f4d3c1c0d"),
        'q' => (4.0, "4g4a 4f3g1g0f0d1c3c4d"),
        'r' => (3.0, "0c0g 0e2g3g"),
        's' => (4.0, "4f3g1g0f1e3e4d3c1c0d"),
        't' => (3.0, "1i1d2c3c 0g3g"),
        'u' => (4.0, "0g0d1c3c4d 4g4c"),
        'v' => (4.0, "0g2c4g"),
        'w' => (4.0, "0g1c2f3c4g"),
        'x' => (4.0, "0g4c 0c4g"),
        'y' => (4.0, "0g2c 4g1a0a"),
        'z' => (4.0, "0g4g0c4c"),
        '!' => (1.0, "0i0e 0d0c"),
        '"' => (2.0, "0i0g 2i2g"),
        '#' => (4.0, "1c1i 3c3i 0e4e 0g4g"),
        '$' => (4.0, "4h3i1i0h0g1f3f4e4d3c1c0d 2i2c"),
        '%' => (4.0, "0c4i 0i0h1h1i0i 3c3d4d4c3c"),
        '&' => (4.0, "4c1h1i2i3h0e0d1c2c4e"),
        '\'' => (1.0, "0i0g"),
        '(' => (2.0, "2i1h0f0e1d2c"),
        ')' => (2.0, "0i1h2f2e1d0c"),
        '*' => (4.0, "2h2d 0g4e 0e4g"),
        '+' => (4.0, "2d2h 0f4f"),
        ',' => (1.0, "1d1c0b"),
        '-' => (4.0, "0f4f"),
        '.' => (1.0, "0c0d"),
        '/' => (4.0, "0c4i"),
        ':' => (1.0, "0c0d 0g0h"),
        ';' => (1.0, "1d1c0b 1g1h"),
        '<' => (4.0, "4h0f4d"),
        '=' => (4.0, "0e4e 0g4g"),
        '>' => (4.0, "0h4f0d"),
        '?' => (4.0, "0h1i3i4h4g2f2e 2d2c"),
        '@' => (4.0, "3e3g1g1e3e4f4h3i1i0h0d1c4c"),
        '[' => (2.0, "2i0i0c2c"),
        '\\' => (4.0, "0i4c"),
        ']' => (2.0, "0i2i2c0c"),
        '^' => (4.0, "0g2i4g"),
        '_' => (4.0, "0b4b"),
        '`' => (1.0, "0i1h"),
        '{' => (2.0, "2i1h1g0f1e1d2c"),
        '|' => (1.0, "0i0a"),
        '}' => (2.0, "0i1h1g2f1e1d0c"),
        '~' => (4.0, "0g1h3f4g"),
        '°' => (2.0, "1i0h1g2h1i"),
        '±' => (4.0, "2e2i 0g4g 0d4d"),
        'Ø' | 'ø' | '⌀' | '∅' => (4.0, "1c0d0h1i3i4h4d3c1c 0c4i"),
        // List bullet: a small crossed diamond at mid x-height.
        '•' => (2.0, "1d0e1f2e1d 0e2e 1d1f"),
        _ => return None,
    };
    Some(g)
}

/// Decode one stroke string into point lists in font units.
pub(crate) fn strokes(spec: &str) -> Vec<Vec<(f64, f64)>> {
    spec.split(' ')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let b = s.as_bytes();
            b.chunks(2)
                .filter_map(|p| {
                    let x = f64::from(p.first()?.checked_sub(b'0')?);
                    let y = f64::from(p.get(1)?.checked_sub(b'a')?) - 2.0;
                    Some((x, y))
                })
                .collect()
        })
        .collect()
}
