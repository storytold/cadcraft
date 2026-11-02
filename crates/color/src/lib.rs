//! CAD colours: the 256-entry colour index (ACI), 24-bit true colour, and the logical
//! ByLayer / ByBlock values. The index palette is generated from its well-known structure
//! (24 hues × 5 values × 2 saturations, plus primaries and greys), not copied from any file.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// An 8-bit sRGB colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn to_u32(self) -> u32 {
        (u32::from(self.0) << 16) | (u32::from(self.1) << 8) | u32::from(self.2)
    }
    pub fn from_u32(v: u32) -> Rgb {
        Rgb(((v >> 16) & 0xff) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8)
    }
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
    pub fn parse_hex(s: &str) -> Option<Rgb> {
        let s = s.trim();
        let s = s.strip_prefix('#').unwrap_or(s);
        if s.len() != 6 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        u32::from_str_radix(s, 16).ok().map(Rgb::from_u32)
    }
    /// Relative luminance (0..1) for contrast decisions.
    pub fn luma(self) -> f64 {
        (0.2126 * f64::from(self.0) + 0.7152 * f64::from(self.1) + 0.0722 * f64::from(self.2)) / 255.0
    }
}

/// An entity or layer colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum Color {
    #[default]
    ByLayer,
    ByBlock,
    /// ACI 1..=255.
    Index(u8),
    True(Rgb),
}

impl Color {
    pub const WHITE: Color = Color::Index(7);
    /// DXF group 62 value: 0 = ByBlock, 256 = ByLayer.
    pub fn to_aci(self) -> i16 {
        match self {
            Color::ByLayer => 256,
            Color::ByBlock => 0,
            Color::Index(i) => i16::from(i),
            Color::True(rgb) => i16::from(nearest_aci(rgb)),
        }
    }
    pub fn from_aci(v: i16) -> Color {
        match v {
            0 => Color::ByBlock,
            1..=255 => Color::Index(v as u8),
            _ => Color::ByLayer,
        }
    }
    /// Resolve to RGB given the layer and block colours.
    pub fn resolve(self, layer: Color, block: Color) -> Rgb {
        match self {
            Color::ByLayer => match layer {
                Color::ByLayer | Color::ByBlock => aci_rgb(7),
                c => c.resolve(Color::Index(7), Color::Index(7)),
            },
            Color::ByBlock => match block {
                Color::ByLayer | Color::ByBlock => aci_rgb(7),
                c => c.resolve(Color::Index(7), Color::Index(7)),
            },
            Color::Index(i) => aci_rgb(i),
            Color::True(c) => c,
        }
    }
    /// Short display name like AutoCAD's Properties palette ("ByLayer", "Red", "Color 34", "255,128,0").
    pub fn name(self) -> String {
        match self {
            Color::ByLayer => "ByLayer".into(),
            Color::ByBlock => "ByBlock".into(),
            Color::Index(i) => match i {
                1 => "Red".into(),
                2 => "Yellow".into(),
                3 => "Green".into(),
                4 => "Cyan".into(),
                5 => "Blue".into(),
                6 => "Magenta".into(),
                7 => "White".into(),
                _ => format!("Color {i}"),
            },
            Color::True(Rgb(r, g, b)) => format!("{r},{g},{b}"),
        }
    }
    /// Parse "red", "7", "bylayer", "255,0,0", "#ff0000".
    pub fn parse(s: &str) -> Option<Color> {
        let t = s.trim().to_ascii_lowercase();
        let named = ["", "red", "yellow", "green", "cyan", "blue", "magenta", "white"];
        if let Some(i) = named.iter().position(|n| !n.is_empty() && *n == t) {
            return Some(Color::Index(i as u8));
        }
        match t.as_str() {
            "bylayer" => return Some(Color::ByLayer),
            "byblock" => return Some(Color::ByBlock),
            "black" => return Some(Color::Index(7)),
            _ => {}
        }
        if let Some(rest) = t.strip_prefix("color ") {
            return parse_digits::<u8>(rest.trim()).filter(|i| *i >= 1).map(Color::Index);
        }
        if let Some(i) = parse_digits::<u16>(&t) {
            return match i {
                0 => Some(Color::ByBlock),
                256 => Some(Color::ByLayer),
                1..=255 => Some(Color::Index(i as u8)),
                _ => None,
            };
        }
        if t.starts_with('#') {
            return Rgb::parse_hex(&t).map(Color::True);
        }
        let parts: Vec<_> = t.split(',').map(|p| parse_digits::<u8>(p.trim())).collect();
        if let [Some(r), Some(g), Some(b)] = parts.as_slice() {
            return Some(Color::True(Rgb(*r, *g, *b)));
        }
        None
    }
}

/// Parse an unsigned integer made of ASCII digits only (no sign, unlike `str::parse`).
fn parse_digits<T: std::str::FromStr>(s: &str) -> Option<T> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// The RGB of ACI `i` (0 and out-of-range map to white/7).
pub fn aci_rgb(i: u8) -> Rgb {
    PALETTE.get(usize::from(i)).copied().unwrap_or(Rgb(255, 255, 255))
}

/// The palette as a 256-entry table (entry 0 = ByBlock placeholder = black).
pub static PALETTE: std::sync::LazyLock<[Rgb; 256]> = std::sync::LazyLock::new(build_palette);

fn build_palette() -> [Rgb; 256] {
    let mut p = [Rgb(0, 0, 0); 256];
    let base = [
        Rgb(0, 0, 0),
        Rgb(255, 0, 0),
        Rgb(255, 255, 0),
        Rgb(0, 255, 0),
        Rgb(0, 255, 255),
        Rgb(0, 0, 255),
        Rgb(255, 0, 255),
        Rgb(255, 255, 255),
        Rgb(128, 128, 128),
        Rgb(192, 192, 192),
    ];
    for (slot, c) in p.iter_mut().zip(base) {
        *slot = c;
    }
    let values = [255.0, 204.0, 153.0, 127.0, 76.0];
    for i in 10..250usize {
        let hue = (i - 10) / 10; // 0..24, 15° each
        let sub = i % 10;
        let v: f64 = values.get(sub / 2).copied().unwrap_or(255.0);
        let half = sub % 2 == 1;
        let (r, g, b) = hue_fractions(hue);
        let ch = |f: f64| -> u8 {
            let f = if half { 0.5 + 0.5 * f } else { f };
            (v * f).floor().clamp(0.0, 255.0) as u8
        };
        if let Some(slot) = p.get_mut(i) {
            *slot = Rgb(ch(r), ch(g), ch(b));
        }
    }
    let greys = [51u8, 91, 132, 173, 214, 255];
    for (k, g) in greys.iter().enumerate() {
        if let Some(slot) = p.get_mut(250 + k) {
            *slot = Rgb(*g, *g, *g);
        }
    }
    p
}

/// Channel fractions for hue step 0..24 (red → yellow → green → cyan → blue → magenta → red).
fn hue_fractions(h: usize) -> (f64, f64, f64) {
    let seg = h / 4;
    let t = (h % 4) as f64 / 4.0;
    match seg {
        0 => (1.0, t, 0.0),
        1 => (1.0 - t, 1.0, 0.0),
        2 => (0.0, 1.0, t),
        3 => (0.0, 1.0 - t, 1.0),
        4 => (t, 0.0, 1.0),
        _ => (1.0, 0.0, 1.0 - t),
    }
}

/// The closest index colour to an RGB value (1..=255).
pub fn nearest_aci(c: Rgb) -> u8 {
    let mut best = (u32::MAX, 7u8);
    for i in 1..=255u8 {
        let p = aci_rgb(i);
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2) as u32;
        let dist = d(p.0, c.0) + d(p.1, c.1) + d(p.2, c.2);
        if dist < best.0 {
            best = (dist, i);
        }
    }
    best.1
}

/// Display colour of `c` on `background`. Colour 7 (`aci7`) flips to contrast with the
/// background, white on dark and black on light (paper), as CAD programs draw it; every other
/// colour, including a true-colour or index white or black, keeps its RGB.
pub fn display_rgb(c: Rgb, aci7: bool, background: Rgb) -> Rgb {
    if !aci7 {
        c
    } else if background.luma() < 0.5 {
        Rgb(255, 255, 255)
    } else {
        Rgb(0, 0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_rejects_non_hex_digits() {
        assert_eq!(Rgb::parse_hex("+12345"), None);
        assert_eq!(Rgb::parse_hex("-12345"), None);
        assert_eq!(Color::parse("#+12345"), None);
        assert_eq!(Rgb::parse_hex("#012345"), Some(Rgb(1, 35, 69)));
    }

    #[test]
    fn parse_rejects_plus_sign() {
        assert_eq!(Color::parse("+5"), None);
        assert_eq!(Color::parse(" +256"), None);
        assert_eq!(Color::parse("color +5"), None);
        assert_eq!(Color::parse("+1,+2,+3"), None);
        assert_eq!(Color::parse("1,+2,3"), None);
        assert_eq!(Color::parse("5"), Some(Color::Index(5)));
        assert_eq!(Color::parse("256"), Some(Color::ByLayer));
        assert_eq!(Color::parse("color 5"), Some(Color::Index(5)));
        assert_eq!(Color::parse("1, 2, 3"), Some(Color::True(Rgb(1, 2, 3))));
    }

    #[test]
    fn parse_hex_rejects_repeated_hash() {
        assert_eq!(Rgb::parse_hex("##ff0000"), None);
        assert_eq!(Rgb::parse_hex("###ff0000"), None);
        assert_eq!(Color::parse("##ff0000"), None);
        assert_eq!(Rgb::parse_hex("#ff0000"), Some(Rgb(255, 0, 0)));
        assert_eq!(Rgb::parse_hex("ff0000"), Some(Rgb(255, 0, 0)));
    }

    #[test]
    fn palette_known_entries() {
        assert_eq!(aci_rgb(1), Rgb(255, 0, 0));
        assert_eq!(aci_rgb(10), Rgb(255, 0, 0));
        assert_eq!(aci_rgb(11), Rgb(255, 127, 127));
        assert_eq!(aci_rgb(12), Rgb(204, 0, 0));
        assert_eq!(aci_rgb(13), Rgb(204, 102, 102));
        assert_eq!(aci_rgb(14), Rgb(153, 0, 0));
        assert_eq!(aci_rgb(15), Rgb(153, 76, 76));
        assert_eq!(aci_rgb(16), Rgb(127, 0, 0));
        assert_eq!(aci_rgb(18), Rgb(76, 0, 0));
        assert_eq!(aci_rgb(20), Rgb(255, 63, 0));
        assert_eq!(aci_rgb(30), Rgb(255, 127, 0));
        assert_eq!(aci_rgb(50), Rgb(255, 255, 0));
        assert_eq!(aci_rgb(90), Rgb(0, 255, 0));
        assert_eq!(aci_rgb(130), Rgb(0, 255, 255));
        assert_eq!(aci_rgb(170), Rgb(0, 0, 255));
        assert_eq!(aci_rgb(210), Rgb(255, 0, 255));
        assert_eq!(aci_rgb(250), Rgb(51, 51, 51));
        assert_eq!(aci_rgb(255), Rgb(255, 255, 255));
    }

    #[test]
    fn aci_roundtrip_and_parse() {
        assert_eq!(Color::from_aci(256), Color::ByLayer);
        assert_eq!(Color::from_aci(0), Color::ByBlock);
        assert_eq!(Color::Index(5).to_aci(), 5);
        assert_eq!(Color::parse("Red"), Some(Color::Index(1)));
        assert_eq!(Color::parse("bylayer"), Some(Color::ByLayer));
        assert_eq!(Color::parse("255,128,0"), Some(Color::True(Rgb(255, 128, 0))));
        assert_eq!(Color::parse("#00ff00"), Some(Color::True(Rgb(0, 255, 0))));
        assert_eq!(Color::parse("300"), None);
        assert_eq!(Color::parse("color 34"), Some(Color::Index(34)));
        assert_eq!(nearest_aci(Rgb(250, 2, 3)), 1);
    }

    #[test]
    fn white_flips_on_light_background() {
        assert_eq!(display_rgb(Rgb(255, 255, 255), true, Rgb(255, 255, 255)), Rgb(0, 0, 0));
        assert_eq!(display_rgb(Rgb(255, 255, 255), true, Rgb(33, 40, 48)), Rgb(255, 255, 255));
        // Only colour 7 flips: ACI 255 / true-colour white and dark index colours keep their RGB.
        assert_eq!(display_rgb(Rgb(255, 255, 255), false, Rgb(255, 255, 255)), Rgb(255, 255, 255));
        assert_eq!(display_rgb(aci_rgb(250), false, Rgb(33, 40, 48)), aci_rgb(250));
    }
}
