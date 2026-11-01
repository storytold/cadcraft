//! Linear and angular unit formatting and parsing (LUNITS/LUPREC, AUNITS/AUPREC).

use std::f64::consts::PI;

/// Largest scaled value (below `i64::MAX`) that the integer feet/inch splitting handles; larger
/// distances fall back to decimal.
const I64_SAFE_MAX: f64 = 9.0e18;

/// Format a distance per LUNITS (1 scientific, 2 decimal, 3 engineering, 4 architectural,
/// 5 fractional) and LUPREC.
pub fn format_distance(v: f64, lunits: i64, luprec: i64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let prec = luprec.clamp(0, 8) as usize;
    match lunits {
        1 => scientific(v, prec),
        3 => {
            let neg = v < 0.0;
            // Round at the displayed precision before splitting, so 11.999 carries to 1'-0.00".
            let scale = 10f64.powi(prec as i32);
            let rounded = (v.abs() * scale).round() / scale;
            let a = if rounded.is_finite() { rounded } else { v.abs() };
            let ft = (a / 12.0).floor();
            if ft >= I64_SAFE_MAX {
                return format!("{:.*}", prec, v);
            }
            let inch = a - ft * 12.0;
            format!("{}{}'-{:.*}\"", if neg { "-" } else { "" }, ft as i64, prec, inch)
        }
        4 => {
            let neg = v < 0.0;
            let a = v.abs();
            let denom = 1i64 << prec.min(8);
            let scaled = (a * denom as f64).round();
            if scaled >= I64_SAFE_MAX {
                return format!("{:.*}", prec, v);
            }
            let total = scaled as i64;
            let ft = total / (12 * denom);
            let rem = total - ft * 12 * denom;
            let inch = rem / denom;
            let frac = rem % denom;
            let fs = if frac == 0 {
                String::new()
            } else {
                let g = gcd(frac, denom);
                format!(" {}/{}", frac / g, denom / g)
            };
            format!("{}{}'-{}{}\"", if neg { "-" } else { "" }, ft, inch, fs)
        }
        5 => {
            let neg = v < 0.0;
            let a = v.abs();
            let denom = 1i64 << prec.min(8);
            let scaled = (a * denom as f64).round();
            if scaled >= I64_SAFE_MAX {
                return format!("{:.*}", prec, v);
            }
            let total = scaled as i64;
            let whole = total / denom;
            let frac = total % denom;
            let fs = if frac == 0 {
                String::new()
            } else {
                let g = gcd(frac, denom);
                format!(" {}/{}", frac / g, denom / g)
            };
            format!("{}{}{}", if neg { "-" } else { "" }, whole, fs)
        }
        _ => fixed(v, prec),
    }
}

/// `v` with `prec` decimals, rounded as AutoCAD rounds a number it shows (RTOS, LUNITS): at 15
/// significant digits, with a half rounded away from zero. AutoCAD 2024 gives 1.01 for 1.005 to two
/// places (its binary value is just below 1.005), 3 for 2.5 and -0.13 for -0.125, where Rust's
/// formatting rounds the binary value and a half to even (1.00, 2 and -0.12).
pub fn fixed(v: f64, prec: usize) -> String {
    let plain = || format!("{v:.prec$}");
    if !v.is_finite() {
        return plain();
    }
    let sci = format!("{:.14e}", v.abs());
    let Some((mantissa, exp)) = sci.split_once('e') else { return plain() };
    let Ok(exp) = exp.parse::<i64>() else { return plain() };
    let sig: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();
    // Digit `k` of `sig` is worth 10^(exp - k); keep those down to 10^-prec, in units of 10^-prec.
    let digit = |k: i64| usize::try_from(k).ok().and_then(|k| sig.get(k)).copied().unwrap_or(0);
    let keep = exp + 1 + prec as i64;
    let mut units: Vec<u8> = (0..keep).map(digit).collect();
    if keep >= 0 && digit(keep) >= 5 {
        let mut carry = true;
        for d in units.iter_mut().rev() {
            if *d == 9 {
                *d = 0;
            } else {
                *d += 1;
                carry = false;
                break;
            }
        }
        if carry {
            units.insert(0, 1);
        }
    }
    let n: String = units.iter().map(|d| char::from(b'0' + d)).collect();
    let n = format!("{:0>width$}", n.trim_start_matches('0'), width = prec + 1);
    let (int, frac) = n.split_at(n.len() - prec);
    let body = if prec == 0 { int.to_string() } else { format!("{int}.{frac}") };
    if v < 0.0 && n.bytes().any(|b| b != b'0') { format!("-{body}") } else { body }
}

/// Scientific notation with a signed exponent of at least two digits (`1.7500E+01`, `1.00E-05`),
/// the mantissa rounded as [`fixed`] rounds.
fn scientific(v: f64, prec: usize) -> String {
    let s = format!("{:.14e}", v);
    let Some((m, e)) = s.split_once('e') else { return s };
    let (Ok(m), Ok(mut e)) = (m.parse::<f64>(), e.parse::<i64>()) else { return s };
    let mut mantissa = fixed(m, prec);
    // 9.99… rounded up to 10: one more power of ten.
    if mantissa.trim_start_matches('-').starts_with("10") {
        e += 1;
        mantissa = fixed(m / 10.0, prec);
    }
    format!("{mantissa}E{}{:0>2}", if e < 0 { '-' } else { '+' }, e.abs())
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a.abs().max(1) } else { gcd(b, a % b) }
}

/// Degrees (`[0, 360]`) as degrees/minutes/seconds, shown to the AUPREC precision (`12d34'57"`).
fn format_dms(deg: f64, prec: usize) -> String {
    // Round at the displayed precision before splitting, carrying through a full turn.
    let scale = 10i64.pow(prec.saturating_sub(4) as u32);
    let seconds_per_unit = match prec {
        0 => 3600,
        1 | 2 => 60,
        _ => 1,
    };
    let units = (deg * (3600 / seconds_per_unit * scale) as f64).round() as i64;
    let total = units * seconds_per_unit % (360 * 3600 * scale);
    let (d, m) = (total / (3600 * scale), (total / (60 * scale)) % 60);
    match prec {
        0 => format!("{d}d"),
        1 | 2 => format!("{d}d{m}'"),
        _ => {
            let s = (total % (60 * scale)) as f64 / scale as f64;
            format!("{d}d{m}'{:.*}\"", prec.saturating_sub(4), s)
        }
    }
}

/// Format an angle (radians) per AUNITS (0 decimal degrees, 1 deg/min/sec, 2 grads, 3 radians,
/// 4 surveyor's) and AUPREC.
pub fn format_angle(a: f64, aunits: i64, auprec: i64) -> String {
    let prec = auprec.clamp(0, 8) as usize;
    let deg = cadcraft_geom::norm_angle(a).to_degrees();
    match aunits {
        1 => format_dms(deg, prec),
        2 => format!("{:.*}g", prec, wrap_full_turn(deg / 0.9, 400.0, prec)),
        3 => format!("{:.*}r", prec, wrap_full_turn(cadcraft_geom::norm_angle(a), 2.0 * PI, prec)),
        4 => {
            // N/S <angle from N/S toward E/W> E/W
            let (ns, ew, ang) = if deg <= 90.0 {
                ("N", "E", 90.0 - deg)
            } else if deg <= 180.0 {
                ("N", "W", deg - 90.0)
            } else if deg <= 270.0 {
                ("S", "W", 270.0 - deg)
            } else {
                ("S", "E", deg - 270.0)
            };
            // A bearing shown as due north, south, east or west is that letter, as AutoCAD 2024
            // shows it ("E" for 0), except to whole degrees ("N 90d E").
            let dms = format_dms(ang, prec);
            if prec > 0 && dms == format_dms(0.0, prec) {
                ns.to_string()
            } else if prec > 0 && dms == format_dms(90.0, prec) {
                ew.to_string()
            } else {
                format!("{ns} {dms} {ew}")
            }
        }
        _ => format!("{:.*}", prec, wrap_full_turn(deg, 360.0, prec)),
    }
}

/// `v` (in `[0, full)`), or 0 when it rounds at `prec` decimals to the printed full-turn value (only when that value is exact).
fn wrap_full_turn(v: f64, full: f64, prec: usize) -> f64 {
    let full_text = format!("{full:.prec$}");
    // A full turn that cannot be printed exactly (2π) only wraps when `v` is numerically that value.
    let exact = full_text.parse::<f64>().is_ok_and(|f| f == full);
    let reached = exact || full - v < 1e-6;
    if reached && format!("{v:.prec$}") == full_text { 0.0 } else { v }
}

/// Parse a distance typed by the user: decimal, `1'6"`, `1'-6 1/2"`, `6 1/2`, `3/4`, `1e3`.
pub fn parse_distance(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(v) = t.parse::<f64>() {
        return v.is_finite().then_some(v);
    }
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r.trim()),
        None => (false, t),
    };
    if neg && t.starts_with(['-', '+']) {
        return None;
    }
    let mut total = 0.0;
    let mut rest = t;
    let mut parsed = false;
    if let Some(i) = rest.find('\'') {
        let ft: f64 = rest.get(..i)?.trim().parse().ok()?;
        total += ft * 12.0;
        parsed = true;
        rest = rest.get(i + 1..)?.trim_start_matches('-').trim();
    }
    let rest = rest.trim_end_matches('"').trim();
    if !rest.is_empty() {
        total += parse_mixed(rest)?;
        parsed = true;
    }
    if !parsed {
        return None;
    }
    let v = if neg { -total } else { total };
    v.is_finite().then_some(v)
}

fn parse_mixed(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let frac = |p: &str| -> Option<f64> {
        if let Some((a, b)) = p.split_once('/') {
            let d: f64 = b.trim().parse().ok()?;
            if d == 0.0 {
                return None;
            }
            Some(a.trim().parse::<f64>().ok()? / d)
        } else {
            p.parse().ok()
        }
    };
    match parts.as_slice() {
        [a] => frac(a),
        [a, b] => {
            let (n, d) = b.split_once('/')?;
            if n.trim().starts_with(['-', '+']) || d.trim().starts_with(['-', '+']) {
                return None;
            }
            Some(a.parse::<f64>().ok()? + frac(b)?)
        }
        _ => None,
    }
}

/// How typed angles are read: the drawing's angle settings (UNITS). Bare numbers are in AUNITS,
/// and directions are measured from ANGBASE, turning as ANGDIR says.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AngleSettings {
    /// AUNITS: 0 decimal degrees, 1 degrees/minutes/seconds, 2 grads, 3 radians, 4 surveyor's.
    pub aunits: i64,
    /// ANGBASE: the direction of angle 0, in radians counterclockwise from +X (`$ANGBASE` in DXF is in degrees).
    pub angbase: f64,
    /// ANGDIR 1: positive angles turn clockwise.
    pub clockwise: bool,
}

impl AngleSettings {
    pub fn from_header(h: &cadcraft_doc::Header) -> Self {
        AngleSettings { aunits: h.i64("AUNITS", 0), angbase: h.f64("ANGBASE", 0.0), clockwise: h.i64("ANGDIR", 0) != 0 }
    }

    /// A typed angle size in radians (included angles, sweeps, angles from an axis): a bare
    /// number in AUNITS, or with an explicit unit: `30d`, `45d30'15"`, `0.5r`, `100g`.
    pub fn amount(&self, s: &str) -> Option<f64> {
        parse_units(s, self.aunits).map(|(a, _)| a)
    }

    /// A typed rotation in radians counterclockwise: the amount, turned clockwise when ANGDIR is 1.
    pub fn rotation(&self, s: &str) -> Option<f64> {
        self.amount(s).map(|a| if self.clockwise { -a } else { a })
    }

    /// A typed direction in radians counterclockwise from +X: the amount measured from ANGBASE in
    /// the ANGDIR sense. Surveyor's bearings (`N45dE`) are absolute. The overrides measure from
    /// +X counterclockwise whatever ANGBASE and ANGDIR say: `<<a` in degrees, `<<<a` in AUNITS.
    pub fn direction(&self, s: &str) -> Option<f64> {
        let t = s.trim();
        if let Some(r) = t.strip_prefix("<<<") {
            return parse_units(r, self.aunits).map(|(a, _)| a);
        }
        if let Some(r) = t.strip_prefix("<<") {
            return parse_units(r, 0).map(|(a, _)| a);
        }
        let (a, absolute) = parse_units(t, self.aunits)?;
        if absolute {
            return Some(a);
        }
        let v = self.angbase + if self.clockwise { -a } else { a };
        v.is_finite().then_some(v)
    }

    /// Direction `dir` (radians counterclockwise from +X) as the drawing shows directions, in
    /// radians: from ANGBASE in the ANGDIR sense, 0 up to a full turn. (A block turned 2.23°
    /// shows as 87.77 in a drawing measured clockwise from north, as in AutoCAD's Properties.)
    pub fn shown(&self, dir: f64) -> f64 {
        let a = if self.clockwise { self.angbase - dir } else { dir - self.angbase };
        a.rem_euclid(std::f64::consts::TAU)
    }

    /// The direction (radians counterclockwise from +X) the drawing shows as `v` (radians): the
    /// inverse of [`AngleSettings::shown`].
    pub fn unshown(&self, v: f64) -> f64 {
        (self.angbase + if self.clockwise { -v } else { v }).rem_euclid(std::f64::consts::TAU)
    }
}

/// Parse a direction typed with the default angle settings (degrees, 0 = +X, counterclockwise):
/// `45`, `45d30'`, `0.5r`, `100g`, `N45dE`. Typed input reads the drawing's settings instead
/// (`Session::angle_settings`).
pub fn parse_angle(s: &str) -> Option<f64> {
    AngleSettings::default().direction(s)
}

/// A typed angle in radians and whether it is absolute (a surveyor's bearing).
fn parse_units(s: &str, aunits: i64) -> Option<(f64, bool)> {
    let t = s.trim().to_ascii_lowercase();
    if t.is_empty() {
        return None;
    }
    let num = |v: &str| v.trim().parse::<f64>().ok();
    let v = if let Some(r) = t.strip_suffix('r') {
        (num(r)?, false)
    } else if let Some(g) = t.strip_suffix('g') {
        ((num(g)? * 0.9).to_radians(), false)
    } else if let Some(b) = bearing(&t) {
        (b, true)
    } else if let Some(b) = (aunits == 4).then(|| cardinal(&t)).flatten() {
        // A due bearing as surveyor's units show it: one letter.
        (b, true)
    } else if let Some(v) = num(&t) {
        let a = match aunits {
            2 => (v * 0.9).to_radians(),
            3 => v,
            _ => v.to_radians(),
        };
        (a, false)
    } else {
        (parse_dms(&t)?.to_radians(), false)
    };
    v.0.is_finite().then_some(v)
}

/// A due bearing (`n`, `e`, `s`, `w`, lower case) as radians counterclockwise from +X.
fn cardinal(t: &str) -> Option<f64> {
    match t {
        "e" => Some(0.0),
        "n" => Some(PI / 2.0),
        "w" => Some(PI),
        "s" => Some(1.5 * PI),
        _ => None,
    }
}

/// A surveyor's bearing (`n45d30'e`, lower case) as radians counterclockwise from +X.
fn bearing(t: &str) -> Option<f64> {
    let north = t.starts_with('n');
    let rest = t.strip_prefix('n').or_else(|| t.strip_prefix('s'))?;
    let (ang_s, east) = match rest.strip_suffix('e') {
        Some(a) => (a, true),
        None => (rest.strip_suffix('w')?, false),
    };
    let a = parse_dms(ang_s.trim())?;
    if !(0.0..=90.0).contains(&a) {
        return None;
    }
    let deg = match (north, east) {
        (true, true) => 90.0 - a,
        (true, false) => 90.0 + a,
        (false, false) => 270.0 - a,
        (false, true) => 270.0 + a,
    };
    Some(deg.to_radians())
}

/// Minutes or seconds of a D'" angle: a number in `0..=60`.
fn dms_part(t: &str) -> Option<f64> {
    let v: f64 = t.trim().parse().ok()?;
    (0.0..=60.0).contains(&v).then_some(v)
}

/// Degrees, `45d`, `45d30'` or `45d30'15"` (a leading `-` negates the whole angle).
fn parse_dms(t: &str) -> Option<f64> {
    let (neg, t) = match t.trim().strip_prefix('-') {
        Some(r) => (true, r.trim()),
        None => (false, t.trim()),
    };
    let (d, rest) = t.split_once('d').unwrap_or((t, ""));
    let mut deg: f64 = d.trim().parse().ok()?;
    let rest = rest.trim();
    if !rest.is_empty() {
        let (m, rest2) = rest.split_once('\'').unwrap_or((rest, ""));
        if !m.trim().is_empty() {
            deg += dms_part(m)? / 60.0;
        }
        let s = rest2.trim().trim_end_matches('"');
        if !s.is_empty() {
            deg += dms_part(s)? / 3600.0;
        }
    }
    let deg = if neg { -deg } else { deg };
    deg.is_finite().then_some(deg)
}

impl crate::Session {
    /// How typed angles are read in the current drawing (the defaults without one).
    pub fn angle_settings(&self) -> AngleSettings {
        self.doc().map(|d| AngleSettings::from_header(&d.header)).unwrap_or_default()
    }
}

pub const DEG: f64 = PI / 180.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dms_minutes_and_seconds_must_be_within_0_to_60() {
        for t in ["45d-30'", "45d30'-15\"", "45d75'", "45d30'75\"", "n45d-30'e"] {
            assert_eq!(parse_angle(t), None, "{t}");
        }
        assert!(parse_angle("45d30'15\"").is_some());
    }

    #[test]
    fn engineering_huge_distance_falls_back_to_decimal() {
        assert_eq!(format_distance(1e30, 3, 2), format!("{:.2}", 1e30));
        assert_eq!(format_distance(-1e300, 3, 8), format!("{:.8}", -1e300));
    }

    #[test]
    fn mixed_distance_needs_a_positive_fraction_after_the_whole_number() {
        assert_eq!(parse_distance("6 1/2"), Some(6.5));
        assert_eq!(parse_distance("5 5"), None);
        assert_eq!(parse_distance("1 -1/2"), None);
    }

    #[test]
    fn huge_architectural_and_fractional_distances_do_not_saturate() {
        for lunits in [4, 5] {
            for v in [1e30, -1e30] {
                assert_eq!(format_distance(v, lunits, 4), format!("{:.4}", v), "lunits {lunits}");
            }
        }
        assert_eq!(format_distance(1e15, 5, 4), "1000000000000000");
        assert_eq!(format_distance(12.5, 4, 4), "1'-0 1/2\"");
    }

    #[test]
    fn directions_show_from_angbase_in_the_angdir_sense() {
        let north_clockwise = AngleSettings { aunits: 0, angbase: 90f64.to_radians(), clockwise: true };
        // AutoCAD shows a block turned 2.22729505553431° as 87.77 in such a drawing.
        let shown = north_clockwise.shown(2.22729505553431f64.to_radians()).to_degrees();
        assert!((shown - 87.77270494446569).abs() < 1e-9, "{shown}");
        let back = north_clockwise.unshown(87.77270494446569f64.to_radians()).to_degrees();
        assert!((back - 2.22729505553431).abs() < 1e-9, "{back}");
        // The defaults show directions as they are.
        let plain = AngleSettings::default();
        assert!((plain.shown(0.5) - 0.5).abs() < 1e-12 && (plain.unshown(0.5) - 0.5).abs() < 1e-12);
        // Counterclockwise from a base of 30°: 45° shows as 15.
        let based = AngleSettings { aunits: 0, angbase: 30f64.to_radians(), clockwise: false };
        assert!((based.shown(45f64.to_radians()).to_degrees() - 15.0).abs() < 1e-9);
    }

    #[test]
    fn decimal_and_arch() {
        assert_eq!(format_distance(28.98524, 2, 4), "28.9852");
        assert_eq!(format_distance(18.5, 4, 4), "1'-6 1/2\"");
        assert_eq!(format_distance(6.25, 5, 2), "6 1/4");
        assert_eq!(format_distance(30.0, 3, 2), "2'-6.00\"");
    }

    #[test]
    fn engineering_rounding_carries_to_feet() {
        assert_eq!(format_distance(11.999, 3, 2), "1'-0.00\"");
        assert_eq!(format_distance(23.999, 3, 2), "2'-0.00\"");
        assert_eq!(format_distance(-11.999, 3, 2), "-1'-0.00\"");
        assert_eq!(format_distance(11.5, 3, 2), "0'-11.50\"");
        assert_eq!(format_distance(12.0, 3, 2), "1'-0.00\"");
        assert_eq!(format_distance(11.6, 3, 0), "1'-0\"");
    }

    /// RTOS in AutoCAD 2024 (accoreconsole, DIMZIN 0): a half rounds away from zero, at 15
    /// significant digits.
    #[test]
    fn numbers_round_as_autocad_rounds_them() {
        for (v, prec, want) in [
            (1500.5, 0, "1501"),
            (2.5, 0, "3"),
            (-2.5, 0, "-3"),
            (0.125, 2, "0.13"),
            (0.5, 0, "1"),
            (1.005, 2, "1.01"),
            (3.5, 0, "4"),
            (0.0625, 3, "0.063"),
            (-0.125, 2, "-0.13"),
            (1234.5678, 3, "1234.568"),
        ] {
            assert_eq!(format_distance(v, 2, prec), want, "{v} to {prec}");
        }
        assert_eq!(format_distance(2.5, 1, 0), "3E+00");
        assert_eq!(format_distance(0.125, 1, 1), "1.3E-01");
        assert_eq!(format_distance(9.96, 1, 1), "1.0E+01");
        assert_eq!(fixed(-0.004, 2), "0.00");
        assert_eq!(fixed(1e-20, 3), "0.000");
        assert_eq!(fixed(123456789012.0, 1), "123456789012.0");
    }

    #[test]
    fn scientific_exponent_is_signed_and_padded() {
        assert_eq!(format_distance(17.5, 1, 4), "1.7500E+01");
        assert_eq!(format_distance(0.00001, 1, 2), "1.00E-05");
        // 1.2345 is halfway: away from zero, as AutoCAD rounds 2.5 to 3E+00.
        assert_eq!(format_distance(1234.5, 1, 3), "1.235E+03");
        assert_eq!(format_distance(12345678.0, 1, 3), "1.235E+07");
        assert_eq!(format_distance(-17.5, 1, 4), "-1.7500E+01");
        assert_eq!(format_distance(0.0, 1, 2), "0.00E+00");
        assert_eq!(format_distance(1e100, 1, 1), "1.0E+100");
    }

    #[test]
    fn dms_display_precision() {
        let angle = (12.0 + 34.0 / 60.0 + 56.78912 / 3600.0_f64).to_radians();
        let expected = ["13d", "12d35'", "12d35'", "12d34'57\"", "12d34'57\"", "12d34'56.8\"", "12d34'56.79\"", "12d34'56.789\"", "12d34'56.7891\""];
        for (prec, expected) in expected.into_iter().enumerate() {
            assert_eq!(format_angle(angle, 1, prec as i64), expected, "AUPREC {prec}");
        }
    }

    #[test]
    fn dms_rounding_carries() {
        let dms = |d: f64, m: f64, s: f64| (d + m / 60.0 + s / 3600.0).to_radians();
        let negative = (-0.0001_f64).to_radians();
        let cases = [
            (dms(29.0, 59.0, 59.6), 0, "30d"),
            (dms(29.0, 59.0, 59.6), 2, "30d0'"),
            (dms(29.0, 59.0, 59.6), 4, "30d0'0\""),
            (dms(29.0, 0.0, 59.99996), 2, "29d1'"),
            (dms(29.0, 0.0, 59.99996), 4, "29d1'0\""),
            (dms(29.0, 59.0, 59.99996), 5, "30d0'0.0\""),
            (dms(29.0, 59.0, 59.99996), 8, "30d0'0.0000\""),
            (negative, 0, "0d"),
            (negative, 4, "0d0'0\""),
            (negative, 5, "359d59'59.6\""),
        ];
        for (angle, prec, expected) in cases {
            assert_eq!(format_angle(angle, 1, prec), expected, "AUPREC {prec}");
        }
    }

    #[test]
    fn surveyors_units_read_a_due_bearing_typed_as_one_letter() {
        // What surveyor's units show for a due bearing can be typed back, e.g. `@10<E`.
        let a = AngleSettings { aunits: 4, angbase: 1.0, clockwise: true };
        for (t, want) in [("E", 0.0), ("n", PI / 2.0), ("W", PI), ("s", 1.5 * PI)] {
            let got = a.direction(t).unwrap();
            assert!((got - want).abs() < 1e-12, "{t}: {got}");
        }
        // Other angle units don't take a bare letter.
        assert_eq!(AngleSettings { aunits: 0, angbase: 0.0, clockwise: false }.direction("E"), None);
    }

    #[test]
    fn surveyors_units_show_a_due_bearing_as_one_letter() {
        // AutoCAD 2024's ANGTOS in surveyor's units (measured): due east, north, west and south are
        // one letter, also when the bearing only rounds to one; to whole degrees east stays a
        // bearing.
        let cases = [
            (0.0, 0, "N 90d E"),
            (0.0, 1, "E"),
            (0.0, 2, "E"),
            (0.0, 4, "E"),
            (PI / 2.0, 1, "N"),
            (PI, 4, "W"),
            (3.0 * PI / 2.0, 4, "S"),
            (0.00001, 2, "E"),
            (1.0, 1, "N 32d42' E"),
            (1.0, 3, "N 32d42'15\" E"),
            (1.0, 7, "N 32d42'15.194\" E"),
        ];
        for (angle, prec, expected) in cases {
            assert_eq!(format_angle(angle, 4, prec), expected, "{angle} AUPREC {prec}");
        }
    }

    #[test]
    fn rounding_to_a_full_turn_wraps_to_zero() {
        let almost_turn = 359.9999_f64.to_radians();
        assert_eq!(format_angle(almost_turn, 0, 2), "0.00");
        assert_eq!(format_angle(almost_turn, 2, 2), "0.00g");
        assert_eq!(format_angle(2.0 * PI - 1e-9, 3, 2), "0.00r");
        assert_eq!(format_angle(359.99_f64.to_radians(), 0, 1), "0.0");
        assert_eq!(format_angle(359.9_f64.to_radians(), 0, 1), "359.9");
        assert_eq!(format_angle(1.0, 3, 2), "1.00r");
        assert_eq!(format_angle(6.25, 3, 1), "6.2r");
    }

    #[test]
    fn parse_distances() {
        assert_eq!(parse_distance("12.5"), Some(12.5));
        assert_eq!(parse_distance("1'6\""), Some(18.0));
        assert_eq!(parse_distance("1'-6 1/2\""), Some(18.5));
        assert_eq!(parse_distance("3/4"), Some(0.75));
        assert_eq!(parse_distance("1/0"), None);
        assert_eq!(parse_distance("abc"), None);
        assert_eq!(parse_distance(""), None);
    }

    #[test]
    fn parse_distance_rejects_doubled_minus() {
        assert_eq!(parse_distance("--5"), None);
        assert_eq!(parse_distance("- -3/4"), None);
        assert_eq!(parse_distance("-5"), Some(-5.0));
        assert_eq!(parse_distance("-3/4"), Some(-0.75));
    }

    #[test]
    fn parse_distance_needs_a_number() {
        assert_eq!(parse_distance("-"), None);
        assert_eq!(parse_distance("\""), None);
        assert_eq!(parse_distance("-\""), None);
        assert_eq!(parse_distance("1'"), Some(12.0));
    }

    #[test]
    fn angles() {
        assert!((parse_angle("90").unwrap() - PI / 2.0).abs() < 1e-12);
        assert!((parse_angle("45d30'").unwrap().to_degrees() - 45.5).abs() < 1e-9);
        assert!((parse_angle("N45dE").unwrap().to_degrees() - 45.0).abs() < 1e-9);
        assert!((parse_angle("1.5r").unwrap() - 1.5).abs() < 1e-12);
        assert_eq!(format_angle(PI / 2.0, 0, 0), "90");
        assert_eq!(format_angle(PI / 4.0, 4, 0), "N 45d E");
    }

    #[test]
    fn surveyor_display_is_degrees_minutes_seconds() {
        // 73.6975 degrees south of east: 73d41'51" (0.6975 deg = 41.85' = 41'51")
        let angle = (270.0 + 73.6975_f64).to_radians();
        assert_eq!(format_angle(angle, 4, 4), "S 73d41'51\" E");
        assert_eq!(format_angle(angle, 4, 2), "S 73d42' E");
        assert_eq!(format_angle(angle, 4, 0), "S 74d E");
        assert_eq!(format_angle(angle, 4, 6), "S 73d41'51.00\" E");
    }

    #[test]
    fn angle_radians_low_precision_not_wrapped() {
        assert_eq!(format_angle(6.2, 3, 0), "6r");
        assert_eq!(format_angle(6.26, 3, 1), "6.3r");
        assert_eq!(format_angle(0.0, 3, 1), "0.0r");
    }
}
