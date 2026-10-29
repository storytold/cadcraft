use super::{DimStyle, Dimension, NumFormat, TextFont, format_decimal, format_linear};

/// Format a linear measurement per the style (plain text).
pub fn format_linear_value(v: f64, st: &DimStyle) -> String {
    format_linear(v * st.linear_factor, &NumFormat::from_style(st)).0
}

/// The text of a linear measurement: (plain, MTEXT) with DIMPOST, tolerances/limits and
/// alternate units, then the user's override text (`<>` = the measurement). DIMRND rounds the
/// measurement (and so the limits) only: tolerances are never rounded and alternate units
/// round by DIMALTRND.
pub(super) fn linear_text(meas: f64, d: &Dimension, st: &DimStyle, prefix: &str) -> String {
    let v = meas * st.linear_factor;
    let nf = NumFormat::from_style(st);
    let main = if st.limits {
        let up = format_linear(v + st.tol_plus, &nf).1;
        let lo = format_linear(v - st.tol_minus, &nf).1;
        format!("{{\\H{};\\S{up}^{lo};}}", tol_height_code(st))
    } else {
        format!("{prefix}{}", format_linear(v, &nf).1)
    };
    let mut s = post(&st.post, &main);
    if st.tolerance && !st.limits {
        let tf = NumFormat { decimals: st.tol_decimals, zin: st.tol_zero_suppression, round: 0.0, ..nf.clone() };
        // DIMPOST's suffix (what follows `<>`, or all of it) also goes on the tolerance values.
        let suffix = st.post.split_once("<>").map_or(st.post.as_str(), |(_, after)| after);
        if (st.tol_plus - st.tol_minus).abs() < 1e-12 {
            s += &format!("%%p{}{suffix}", format_linear(st.tol_plus, &tf).1);
        } else {
            let plus = format_linear(st.tol_plus, &tf).1 + suffix;
            let minus = format_linear(st.tol_minus, &tf).1 + suffix;
            let sign = |x: &str, pos: char| if x.trim_start_matches(['0', '.', ',']).is_empty() { x.to_string() } else { format!("{pos}{x}") };
            let (plus, minus) =
                (sign(&plus, '+'), if st.tol_minus < 0.0 { format!("+{}", minus.trim_start_matches('-')) } else { sign(&minus, '-') });
            // DIMTOLJ: bottom / middle / top (MTEXT `\A`; middle is the default).
            let tolj = if st.tol_just == 1 { String::new() } else { format!("\\A{};", st.tol_just.min(2)) };
            s += &format!("{{\\H{};{tolj}\\S{plus}^{minus};}}", tol_height_code(st));
        }
    }
    if st.alt {
        // DIMALTU 6 and 7 are architectural and fractional without stacking, 8 is decimal.
        let (unit, frac) = match st.alt_unit {
            u @ 1..=5 => (u, nf.frac),
            6 => (4, 2),
            7 => (5, 2),
            _ => (2, nf.frac),
        };
        let af = NumFormat { unit, decimals: st.alt_decimals, zin: st.alt_zero_suppression, round: st.alt_round, frac, ..nf };
        let alt = format_linear(v * st.alt_factor, &af).1;
        s += &format!(" [{}]", post(&st.alt_post, &alt));
    }
    inspected(apply_override(&s, d), d)
}

/// DIMINSPECT: the label and the rate either side of the value, two spaces apart.
pub(super) fn inspected(value: String, d: &Dimension) -> String {
    match &d.marks.inspect {
        Some(ins) => [ins.label.as_str(), value.as_str(), ins.rate.as_str()].iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join("  "),
        None => value,
    }
}

/// `\H` code for stacked tolerance parts (DIMTFAC relative to the 70% stack scale).
fn tol_height_code(st: &DimStyle) -> String {
    let f = if st.tol_scale.is_finite() && st.tol_scale > 0.0 { st.tol_scale } else { 1.0 };
    format!("{}x", format_decimal(f / 0.7, 4, 8, "."))
}

/// Apply measurement placeholders or append a dimension suffix, bounding the resulting UTF-8 text.
fn post(template: &str, meas: &str) -> String {
    if template.contains("<>") {
        bounded_replace(template, meas)
    } else {
        let mut out = String::new();
        append_dim_text(&mut out, meas);
        append_dim_text(&mut out, template);
        out
    }
}

/// Apply dimension text overrides: empty uses the base, whitespace suppresses text, and other text substitutes it.
pub(super) fn apply_override(base: &str, d: &Dimension) -> String {
    if d.text.is_empty() {
        base.to_string()
    } else if d.text.trim().is_empty() {
        String::new()
    } else {
        bounded_replace(&d.text, base)
    }
}

/// Largest dimension text produced by substituting the measurement into style/override text. Two
/// modest strings can otherwise expand into gigabytes when the template holds many `<>` markers
/// (CWE-789); past this the output is truncated rather than allocated unbounded.
const MAX_DIM_TEXT: usize = 64 * 1024;

/// Replace every `<>` in `template` with `value`, with the expanded length checked against
/// [`MAX_DIM_TEXT`] before allocating so a many-placeholder template cannot blow up memory.
fn bounded_replace(template: &str, value: &str) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find("<>") {
        if !append_dim_text(&mut out, &rest[..i]) || !append_dim_text(&mut out, value) {
            return out;
        }
        rest = &rest[i + 2..];
    }
    append_dim_text(&mut out, rest);
    out
}

// Returns false when truncation consumed the remaining UTF-8-safe space.
/// Append within the remaining dimension-text byte allowance without splitting a UTF-8 character.
/// Return false if any input text had to be truncated.
fn append_dim_text(out: &mut String, text: &str) -> bool {
    let mut end = text.len().min(MAX_DIM_TEXT.saturating_sub(out.len()));
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    out.push_str(&text[..end]);
    end == text.len()
}

/// Font and fixed height of the dimension text (from DIMTXSTY's text style).
#[derive(Clone, Debug, Default)]
pub struct DimText {
    pub font: TextFont,
    /// Fixed text height of the style (0 = use DIMTXT × scale).
    pub fixed_height: f64,
    pub width_factor: f64,
    pub oblique: f64,
}

#[cfg(test)]
mod text_budget_tests {
    use super::*;
    /// A dimension suffix without placeholders still obeys the total text limit.
    #[test]
    fn bounds_post_without_markers() {
        let text = post(&"x".repeat(MAX_DIM_TEXT * 2), "12");
        assert_eq!(text.len(), MAX_DIM_TEXT);
        assert!(text.starts_with("12x"));
    }
    /// Oversized substituted text retains its prefix and ends at a valid UTF-8 boundary.
    #[test]
    fn oversized_value_is_substituted_and_truncated_at_a_character_boundary() {
        let text = bounded_replace("prefix <> mm", &"é".repeat(MAX_DIM_TEXT));
        assert!(text.starts_with("prefix é"));
        assert!(!text.contains("<>"));
        assert!(text.len() <= MAX_DIM_TEXT && text.len() >= MAX_DIM_TEXT - 1);
    }
}
