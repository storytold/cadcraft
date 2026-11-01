//! Linux font families that AutoCAD on Windows doesn't have, and the Windows fonts a drawing
//! saved with portable fonts gets instead.

/// Family key (lower case, without spaces, hyphens or underscores; a longer key before a shorter
/// one it starts with) and the Windows fonts for its regular, bold, italic and bold italic styles.
const FAMILIES: [(&str, [&str; 4]); 7] = [
    ("liberationsans", ["arial.ttf", "arialbd.ttf", "ariali.ttf", "arialbi.ttf"]),
    ("liberationserif", ["times.ttf", "timesbd.ttf", "timesi.ttf", "timesbi.ttf"]),
    ("liberationmono", ["cour.ttf", "courbd.ttf", "couri.ttf", "courbi.ttf"]),
    ("opensans", ["arial.ttf", "arialbd.ttf", "ariali.ttf", "arialbi.ttf"]),
    ("dejavusansmono", ["cour.ttf", "courbd.ttf", "couri.ttf", "courbi.ttf"]),
    ("dejavusans", ["verdana.ttf", "verdanab.ttf", "verdanai.ttf", "verdanaz.ttf"]),
    ("dejavuserif", ["times.ttf", "timesbd.ttf", "timesi.ttf", "timesbi.ttf"]),
];

/// The family of a TrueType `font` (file or family name) from [`FAMILIES`], with the rest of its
/// key (the style: `bold`, `italic`, …).
fn family(font: &str) -> Option<(&'static [&'static str; 4], String)> {
    let name = font.trim().rsplit(['/', '\\']).next().unwrap_or_default().to_lowercase();
    let stem = name.strip_suffix(".ttf").or_else(|| name.strip_suffix(".otf")).unwrap_or(&name);
    if stem.contains('.') {
        return None; // another kind of file, such as an SHX font
    }
    let key: String = stem.chars().filter(|c| !matches!(c, ' ' | '-' | '_')).collect();
    let (family, fonts) = FAMILIES.iter().find(|(f, _)| key.starts_with(f))?;
    Some((fonts, key.get(family.len()..).unwrap_or_default().to_string()))
}

/// Whether `font` is a Linux font AutoCAD on Windows doesn't have.
pub fn missing_on_windows(font: &str) -> bool {
    family(font).is_some()
}

/// The Windows font for a Linux `font`. Narrow and condensed styles have none (Arial Narrow comes
/// with Office, not with Windows).
pub fn windows_font(font: &str) -> Option<&'static str> {
    let (fonts, style) = family(font)?;
    if style.contains("narrow") || style.contains("condensed") {
        return None;
    }
    let bold = style.contains("bold");
    let italic = style.contains("italic") || style.contains("oblique");
    fonts.get(usize::from(bold) + 2 * usize::from(italic)).copied()
}

/// The font a drawing file gets for a text style's `font`: with `portable`, the Windows font for
/// a Linux one.
pub fn written_font(font: &str, portable: bool) -> &str {
    if portable { windows_font(font).unwrap_or(font) } else { font }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_fonts_by_file_or_family_name() {
        assert_eq!(windows_font("DejaVuSans.ttf"), Some("verdana.ttf"));
        assert_eq!(windows_font("/usr/share/fonts/truetype/LiberationSerif-Bold.ttf"), Some("timesbd.ttf"));
        assert_eq!(windows_font("Liberation Mono"), Some("cour.ttf"));
        assert_eq!(windows_font("LiberationMono-BoldItalic.ttf"), Some("courbi.ttf"));
        assert_eq!(windows_font("OpenSans-SemiBold.ttf"), Some("arialbd.ttf"));
        assert_eq!(windows_font("Open Sans"), Some("arial.ttf"));
        assert_eq!(windows_font("DejaVuSansMono-BoldOblique.ttf"), Some("courbi.ttf"));
        assert_eq!(windows_font("DejaVuSerif-Italic.ttf"), Some("timesi.ttf"));
        // Narrow and condensed styles are Linux fonts with no Windows one.
        for font in ["Liberation Sans Narrow", "LiberationSansNarrow-Bold.ttf", "OpenSans-CondensedBold.ttf", "DejaVuSansCondensed.ttf"] {
            assert!(missing_on_windows(font) && windows_font(font).is_none(), "{font}");
        }
        // Fonts AutoCAD has, other kinds of file, or names that aren't fonts.
        for font in ["arial.ttf", "txt", "dejavusans.shx", "", "/", "Liberation"] {
            assert!(!missing_on_windows(font) && windows_font(font).is_none(), "{font}");
        }
    }

    #[test]
    fn written_font_only_changes_with_portable_fonts() {
        assert_eq!(written_font("DejaVuSans.ttf", true), "verdana.ttf");
        assert_eq!(written_font("DejaVuSans.ttf", false), "DejaVuSans.ttf");
        assert_eq!(written_font("Liberation Sans Narrow", true), "Liberation Sans Narrow");
    }
}
