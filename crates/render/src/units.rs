//! Dimension number formatting: DIMLUNIT (scientific, decimal, engineering, architectural,
//! fractional), DIMDEC precision, DIMZIN zero suppression, DIMRND rounding, DIMDSEP and
//! DIMFRAC fraction stacking; DIMAUNIT angles. Each formatter returns the MTEXT form (stacked
//! fractions as `\S1/2;`) that the dimension text is laid out from.

use cadcraft_doc::DimStyle;

/// Linear number format settings.
#[derive(Clone, Debug, PartialEq)]
pub struct NumFormat {
    /// DIMLUNIT: 1 scientific, 2 decimal, 3 engineering, 4 architectural, 5 fractional, 6 decimal.
    pub unit: u8,
    /// DIMDEC: decimal places, or the fraction precision 1/2^n for architectural/fractional.
    pub decimals: u8,
    /// DIMZIN bits: low two bits feet/inch zeros, 4 leading zeros, 8 trailing zeros.
    pub zin: u8,
    /// DIMRND (0 = none).
    pub round: f64,
    pub separator: String,
    /// DIMFRAC: 0 horizontal stack, 1 diagonal, 2 not stacked.
    pub frac: u8,
}

impl NumFormat {
    pub fn decimal(decimals: u8) -> Self {
        NumFormat { unit: 2, decimals, zin: 0, round: 0.0, separator: ".".into(), frac: 0 }
    }
    pub fn from_style(st: &DimStyle) -> Self {
        NumFormat {
            unit: st.linear_unit,
            decimals: st.decimals,
            zin: st.zero_suppression,
            round: st.round,
            separator: st.decimal_separator.clone(),
            frac: st.fraction_format,
        }
    }
}

/// Decimal number with leading (`zin & 4`) / trailing (`zin & 8`) zero suppression.
pub fn format_decimal(v: f64, decimals: u8, zin: u8, sep: &str) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    let mut s = format!("{:.*}", usize::from(decimals.min(8)), v);
    if s.starts_with('-') && s.trim_start_matches(['-', '0', '.']).is_empty() {
        s.remove(0);
    }
    if zin & 8 != 0 && s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if zin & 4 != 0 {
        if let Some(r) = s.strip_prefix("0.") {
            s = format!(".{r}");
        } else if let Some(r) = s.strip_prefix("-0.") {
            s = format!("-.{r}");
        }
    }
    if s.is_empty() || s == "-" {
        s = "0".into();
    }
    if sep != "." && !sep.is_empty() { s.replace('.', sep) } else { s }
}

fn gcd(a: i64, b: i64) -> i64 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a.max(1)
}

/// A fraction in MTEXT form per DIMFRAC.
fn frac_mtext(num: i64, den: i64, frac: u8) -> (String, String) {
    let plain = format!("{num}/{den}");
    let m = match frac {
        1 => format!("\\S{num}#{den};"),
        2 => plain.clone(),
        _ => format!("\\S{num}/{den};"),
    };
    (plain, m)
}

/// Scientific: `1.2345E+01`.
fn scientific(v: f64, decimals: u8, zin: u8, sep: &str) -> String {
    if v == 0.0 || !v.is_finite() {
        return format!("{}E+00", format_decimal(0.0, decimals, zin & 8, sep));
    }
    let mut exp = v.abs().log10().floor() as i32;
    let mut mant = v / 10f64.powi(exp);
    // Rounding can carry the mantissa to 10.
    let p = 10f64.powi(i32::from(decimals.min(8)));
    if (mant.abs() * p).round() / p >= 10.0 {
        mant /= 10.0;
        exp += 1;
    }
    format!("{}E{}{:02}", format_decimal(mant, decimals, zin & 8, sep), if exp < 0 { '-' } else { '+' }, exp.abs())
}

/// Apply feet/inch zero suppression (DIMZIN low bits) and join the parts.
fn feet_inches(neg: bool, ft: i64, inch_plain: &str, inch_m: &str, inch_zero: bool, zin: u8) -> (String, String) {
    let mode = zin & 3;
    let show_ft = ft != 0 || mode == 1 || mode == 2;
    let show_in = !inch_zero || mode == 1 || mode == 3 || (ft == 0 && !show_ft);
    // A negative value that rounds to zero prints without a sign.
    let sign = if neg && (ft != 0 || !inch_zero) { "-" } else { "" };
    let mut p = String::from(sign);
    let mut m = String::from(sign);
    if show_ft {
        p += &format!("{ft}'");
        m += &format!("{ft}'");
        if show_in {
            p.push('-');
            m.push('-');
        }
    }
    if show_in {
        p += &format!("{inch_plain}\"");
        m += &format!("{inch_m}\"");
    }
    (p, m)
}

/// Whole number plus a fraction of 1/2^prec, in plain and MTEXT form.
fn mixed(a: f64, prec: u8, frac: u8) -> (i64, String, String, bool) {
    let den = 1i64 << prec.min(8);
    let total = (a * den as f64).round().clamp(-9.0e15, 9.0e15) as i64;
    let whole = total / den;
    let rem = total % den;
    if rem == 0 {
        return (whole, whole.to_string(), whole.to_string(), whole == 0);
    }
    let g = gcd(rem, den);
    let (fp, fm) = frac_mtext(rem / g, den / g, frac);
    let (p, m) = if whole == 0 {
        (fp, fm)
    } else if frac == 2 {
        (format!("{whole} {fp}"), format!("{whole} {fm}"))
    } else {
        (format!("{whole} {fp}"), format!("{whole}{fm}"))
    };
    (whole, p, m, false)
}

/// Format a linear distance. Returns (plain text, MTEXT form).
pub fn format_linear(v: f64, f: &NumFormat) -> (String, String) {
    let mut v = if v.is_finite() { v } else { 0.0 };
    if f.round > 0.0 && f.round.is_finite() {
        v = (v / f.round).round() * f.round;
    }
    let sep = f.separator.as_str();
    match f.unit {
        1 => {
            let s = scientific(v, f.decimals, f.zin, sep);
            (s.clone(), s)
        }
        3 => {
            // Engineering: feet and decimal inches.
            let neg = v < 0.0;
            let a = v.abs();
            let p = 10f64.powi(i32::from(f.decimals.min(8)));
            let a = (a * p).round() / p;
            let ft = (a / 12.0 + 1e-12).floor();
            let inch = (a - ft * 12.0).max(0.0);
            let s = format_decimal(inch, f.decimals, f.zin & 12, sep);
            feet_inches(neg, ft as i64, &s, &s, inch.abs() < 0.5 / p, f.zin)
        }
        4 => {
            // Architectural: feet, inches and fractions.
            let neg = v < 0.0;
            let den = 1i64 << f.decimals.min(8);
            let total = (v.abs() * den as f64).round().clamp(0.0, 9.0e15) as i64;
            let ft = total / (12 * den);
            let rem_inch = (total - ft * 12 * den) as f64 / den as f64;
            let (_, ip, im, zero) = mixed(rem_inch, f.decimals, f.frac);
            feet_inches(neg, ft, &ip, &im, zero, f.zin)
        }
        5 => {
            let neg = v < 0.0;
            let (_, p, m, _) = mixed(v.abs(), f.decimals, f.frac);
            let sign = if neg && p != "0" { "-" } else { "" };
            (format!("{sign}{p}"), format!("{sign}{m}"))
        }
        _ => {
            let s = format_decimal(v, f.decimals, f.zin, sep);
            (s.clone(), s)
        }
    }
}

/// Format an angle (radians, non-negative sweep) per DIMAUNIT / DIMADEC, with a decimal point.
pub fn format_angle(rad: f64, unit: u8, decimals: u8, zin: u8) -> String {
    format_angle_with_separator(rad, unit, decimals, zin, ".")
}

/// Format an angle with the dimension style's DIMDSEP separator.
pub(crate) fn format_angle_with_separator(rad: f64, unit: u8, decimals: u8, zin: u8, separator: &str) -> String {
    let rad = if rad.is_finite() { rad } else { 0.0 };
    let deg = rad.to_degrees();
    match unit {
        1 => {
            // Round at the displayed precision before splitting into degrees/minutes/seconds.
            let seconds_per_unit = match decimals {
                0 => 3600,
                1 | 2 => 60,
                _ => 1,
            };
            let units = (deg * (3600 / seconds_per_unit) as f64).round().max(0.0) as i64;
            let total = units.saturating_mul(seconds_per_unit);
            let (d, m, s) = (total / 3600, (total / 60) % 60, total % 60);
            match decimals {
                0 => format!("{d}°"),
                1 | 2 => format!("{d}°{m}'"),
                _ => format!("{d}°{m}'{s}\""),
            }
        }
        2 => format!("{}g", format_decimal(deg / 0.9, decimals, zin, separator)),
        3 => format!("{}r", format_decimal(rad, decimals, zin, separator)),
        _ => format!("{}°", format_decimal(deg, decimals, zin, separator)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(unit: u8, decimals: u8, zin: u8) -> NumFormat {
        NumFormat { unit, decimals, zin, ..NumFormat::decimal(decimals) }
    }

    #[test]
    fn decimal_and_zero_suppression() {
        assert_eq!(format_linear(0.5, &f(2, 4, 0)).0, "0.5000");
        assert_eq!(format_linear(0.5, &f(2, 4, 4)).0, ".5000");
        assert_eq!(format_linear(0.5, &f(2, 4, 8)).0, "0.5");
        assert_eq!(format_linear(0.5, &f(2, 4, 12)).0, ".5");
        assert_eq!(format_linear(12.0, &f(2, 2, 8)).0, "12");
        assert_eq!(format_linear(-0.00001, &f(2, 2, 0)).0, "0.00");
        let mut c = f(2, 2, 0);
        c.separator = ",".into();
        assert_eq!(format_linear(12.5, &c).0, "12,50");
    }

    #[test]
    fn rounding() {
        let mut c = f(2, 2, 0);
        c.round = 0.25;
        assert_eq!(format_linear(1.13, &c).0, "1.25");
    }

    #[test]
    fn scientific_units() {
        assert_eq!(format_linear(12.345, &f(1, 2, 0)).0, "1.23E+01");
        assert_eq!(format_linear(0.05, &f(1, 1, 0)).0, "5.0E-02");
        assert_eq!(format_linear(9.999, &f(1, 2, 0)).0, "1.00E+01");
    }

    #[test]
    fn engineering_units() {
        assert_eq!(format_linear(18.5, &f(3, 2, 0)).0, "1'-6.50\"");
        assert_eq!(format_linear(6.0, &f(3, 2, 0)).0, "6.00\"");
        assert_eq!(format_linear(6.0, &f(3, 2, 1)).0, "0'-6.00\"");
    }

    #[test]
    fn architectural_units_and_stacking() {
        let (p, m) = format_linear(18.5, &f(4, 4, 0));
        assert_eq!(p, "1'-6 1/2\"");
        assert_eq!(m, "1'-6\\S1/2;\"");
        assert_eq!(format_linear(24.0, &f(4, 4, 0)).0, "2'");
        assert_eq!(format_linear(24.0, &f(4, 4, 1)).0, "2'-0\"");
        assert_eq!(format_linear(24.0, &f(4, 4, 3)).0, "2'-0\"");
        assert_eq!(format_linear(6.0, &f(4, 4, 0)).0, "6\"");
        assert_eq!(format_linear(6.0, &f(4, 4, 2)).0, "0'-6\"");
        assert_eq!(format_linear(0.25, &f(4, 4, 0)).0, "1/4\"");
        assert_eq!(format_linear(0.0, &f(4, 4, 0)).0, "0\"");
        let mut d = f(4, 4, 0);
        d.frac = 1;
        assert_eq!(format_linear(18.5, &d).1, "1'-6\\S1#2;\"");
    }

    #[test]
    fn fractional_units() {
        assert_eq!(format_linear(6.5, &f(5, 4, 0)).0, "6 1/2");
        assert_eq!(format_linear(6.5, &f(5, 4, 0)).1, "6\\S1/2;");
        assert_eq!(format_linear(6.0, &f(5, 4, 0)).0, "6");
        assert_eq!(format_linear(0.375, &f(5, 3, 0)).0, "3/8");
        assert_eq!(format_linear(0.4, &f(5, 2, 0)).0, "1/2");
    }

    #[test]
    fn negative_values_rounding_to_zero_have_no_sign() {
        for unit in 1..=5 {
            for decimals in [0, 2, 4] {
                for zin in [0, 1] {
                    for v in [-1e-9, -0.0] {
                        let (p, m) = format_linear(v, &f(unit, decimals, zin));
                        assert!(!p.starts_with('-') || unit == 1 && v != 0.0, "unit {unit} decimals {decimals} zin {zin}: {p}");
                        assert!(!m.starts_with('-') || unit == 1 && v != 0.0, "unit {unit} decimals {decimals} zin {zin}: {m}");
                    }
                }
            }
        }
        // DIMRND rounding a small negative value to zero, in every unit.
        for unit in 1..=5 {
            let mut c = f(unit, 4, 0);
            c.round = 0.25;
            let (p, m) = format_linear(-0.1, &c);
            assert!(!p.starts_with('-') && !m.starts_with('-'), "unit {unit} with DIMRND: {p} / {m}");
        }
        assert_eq!(format_linear(-1e-9, &f(4, 4, 0)).0, "0\"");
        assert_eq!(format_linear(-1e-9, &f(3, 4, 0)).0, "0.0000\"");
        // Scientific keeps the sign: the mantissa of a tiny negative value is not zero.
        assert_eq!(format_linear(-1e-9, &f(1, 2, 0)).0, "-1.00E-09");
        // The shared feet/inch joiner never signs an all-zero result.
        assert_eq!(feet_inches(true, 0, "0", "0", true, 0).0, "0\"");
        assert_eq!(feet_inches(true, 0, "0", "0", true, 1).0, "0'-0\"");
        // Genuinely negative values keep the sign.
        assert_eq!(format_linear(-18.5, &f(4, 4, 0)).0, "-1'-6 1/2\"");
        assert_eq!(format_linear(-18.5, &f(3, 2, 0)).0, "-1'-6.50\"");
        assert_eq!(format_linear(-24.0, &f(4, 4, 0)).0, "-2'");
    }

    #[test]
    fn angles() {
        assert_eq!(format_angle(std::f64::consts::FRAC_PI_4, 0, 0, 0), "45°");
        assert_eq!(format_angle(45.5f64.to_radians(), 1, 4, 0), "45°30'0\"");
        assert_eq!(format_angle(std::f64::consts::FRAC_PI_2, 2, 1, 0), "100.0g");
        assert_eq!(format_angle(1.0, 3, 2, 0), "1.00r");
    }

    #[test]
    fn dms_rounds_at_displayed_precision() {
        assert_eq!(format_angle(45.75f64.to_radians(), 1, 0, 0), "46°");
        for decimals in [1, 2] {
            assert_eq!(format_angle(45.5125f64.to_radians(), 1, decimals, 0), "45°31'");
            assert_eq!(format_angle(45.999f64.to_radians(), 1, decimals, 0), "46°0'");
        }
        assert_eq!(format_angle(45.5125f64.to_radians(), 1, 3, 0), "45°30'45\"");
        assert_eq!(format_angle(45.9999f64.to_radians(), 1, 4, 0), "46°0'0\"");
    }

    #[test]
    fn dms_carries_and_non_negative_sweeps() {
        let near_thirty = (29.0 + 59.0 / 60.0 + 40.0 / 3600.0_f64).to_radians();
        let near_minute = (29.0 + 40.0 / 3600.0_f64).to_radians();
        for decimals in 0..=4 {
            let (degree, minute, negative) = match decimals {
                0 => ("30°", "29°", "0°"),
                1 | 2 => ("30°0'", "29°1'", "0°0'"),
                _ => ("29°59'40\"", "29°0'40\"", "0°0'0\""),
            };
            assert_eq!(format_angle(near_thirty, 1, decimals, 0), degree);
            assert_eq!(format_angle(near_minute, 1, decimals, 0), minute);
            assert_eq!(format_angle(-45.75f64.to_radians(), 1, decimals, 0), negative);
        }
    }

    #[test]
    fn hostile_numbers() {
        for u in 0..8 {
            for v in [f64::NAN, f64::INFINITY, -1e300, 1e300, 0.0, -0.0] {
                let _ = format_linear(v, &f(u, 200, 255));
            }
        }
    }
}
