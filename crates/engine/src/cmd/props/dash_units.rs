//! -UNITS: the drawing's units at the command line, with AutoCAD 2024's prompts and listings.

use std::f64::consts::TAU;

use crate::units::{AngleSettings, format_angle};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

const REPORT_FORMATS: [&str; 10] = [
    "Report formats:      (Examples)",
    "  1.  Scientific      1.55E+01",
    "  2.  Decimal         15.50",
    "  3.  Engineering     1'-3.50\"",
    "  4.  Architectural   1'-3 1/2\"",
    "  5.  Fractional      15 1/2",
    "With the exception of Engineering and Architectural formats,",
    "these formats can be used with any basic unit of measurement.",
    "For example, Decimal mode is perfect for metric units as well",
    "as decimal English units.",
];

const ANGLE_SYSTEMS: [&str; 6] = [
    "Systems of angle measure:      (Examples)",
    "  1.  Decimal degrees           45.0000",
    "  2.  Degrees/minutes/seconds   45d0'0\"",
    "  3.  Grads                     50.0000g",
    "  4.  Radians                   0.7854r",
    "  5.  Surveyor's units          N 45d0'0\" E",
];

const DENOMINATOR: &str = "Enter denominator of smallest fraction to display";

/// The drawing's units, as -UNITS shows and asks them.
struct Units {
    lunits: i64,
    luprec: i64,
    aunits: i64,
    auprec: i64,
    angbase: f64,
    clockwise: bool,
}

impl Units {
    fn of(s: &Session) -> Units {
        let Ok(d) = s.doc() else { return Units { lunits: 2, luprec: 4, aunits: 0, auprec: 0, angbase: 0.0, clockwise: false } };
        let h = &d.header;
        let angbase = h.f64("ANGBASE", 0.0);
        Units {
            lunits: h.i64("LUNITS", 2).clamp(1, 5),
            luprec: h.i64("LUPREC", 4).clamp(0, 8),
            aunits: h.i64("AUNITS", 0).clamp(0, 4),
            auprec: h.i64("AUPREC", 0).clamp(0, 8),
            angbase: if angbase.is_finite() { angbase } else { 0.0 },
            clockwise: h.i64("ANGDIR", 0) != 0,
        }
    }

    /// Architectural and fractional units give their precision as a fraction's denominator.
    fn fractional(&self) -> bool {
        matches!(self.lunits, 4 | 5)
    }

    /// Direction `a` (radians counterclockwise from +X) in the angle format, measured as the
    /// drawing measures angles but from +X: -UNITS shows north as 270 when angles turn clockwise.
    fn direction_text(&self, a: f64) -> String {
        format_angle(if self.clockwise { -a } else { a }, self.aunits, self.auprec)
    }

    /// A typed direction of angle 0, in radians counterclockwise from +X: an amount in the angle
    /// format, or with its own unit (`45d`, `0.5r`, `100g`, a bearing `N45dE`, and in surveyor's
    /// units N, E, S or W), measured from +X as the drawing measures angles.
    fn direction(&self, t: &str) -> Option<f64> {
        let cardinal = match t.trim().to_ascii_lowercase().as_str() {
            "e" if self.aunits == 4 => Some(0.0),
            "n" if self.aunits == 4 => Some(90f64.to_radians()),
            "w" if self.aunits == 4 => Some(180f64.to_radians()),
            "s" if self.aunits == 4 => Some(270f64.to_radians()),
            _ => None,
        };
        let a = cardinal.or_else(|| AngleSettings { aunits: self.aunits, ..AngleSettings::default() }.amount(t))?;
        let a = if self.clockwise { -a } else { a };
        a.is_finite().then(|| a.rem_euclid(TAU))
    }
}

/// An integer answer in `lo..=hi`, or AutoCAD's message for one that isn't.
fn integer(t: &str, lo: i64, hi: i64) -> Result<i64> {
    let v = t.trim().parse::<i64>().map_err(|_| EngineError::Other("Requires an integer value.".into()))?;
    if (lo..=hi).contains(&v) { Ok(v) } else { Err(EngineError::Other(format!("Requires an integer between {lo} and {hi}."))) }
}

#[derive(Clone, Copy, Default)]
enum Ask {
    #[default]
    LengthFormat,
    LengthPrecision,
    AngleFormat,
    AnglePrecision,
    Direction,
    Clockwise,
}

/// -UNITS: the length format and precision, the angle format and precision, the direction of
/// angle 0 and whether angles turn clockwise, asked in that order. As in AutoCAD, each answer is
/// kept as it is given (Esc keeps the ones before it), and choosing another format resets its
/// precision to 4, the precision then offered.
#[derive(Default)]
pub(super) struct DashUnits {
    ask: Ask,
}

impl Interactive for DashUnits {
    fn name(&self) -> &'static str {
        "-UNITS"
    }

    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        for l in REPORT_FORMATS {
            s.echo(l);
        }
        Ok(Step::Continue)
    }

    fn prompt(&self, s: &Session) -> Prompt {
        let u = Units::of(s);
        match self.ask {
            Ask::LengthFormat => Prompt::new("Enter choice, 1 to 5", Accept::NUMBER).default(u.lunits.to_string()),
            Ask::LengthPrecision if u.fractional() => {
                Prompt::new("(1, 2, 4, 8, 16, 32, 64, 128, or 256)", Accept::NUMBER).default((1_i64 << u.luprec).to_string())
            }
            Ask::LengthPrecision => {
                Prompt::new("Enter number of digits to right of decimal point (0 to 8)", Accept::NUMBER).default(u.luprec.to_string())
            }
            Ask::AngleFormat => Prompt::new("Enter choice, 1 to 5", Accept::NUMBER).default((u.aunits + 1).to_string()),
            Ask::AnglePrecision => {
                Prompt::new("Enter number of fractional places for display of angles (0 to 8)", Accept::NUMBER).default(u.auprec.to_string())
            }
            Ask::Direction => {
                Prompt::new(format!("Enter direction for angle {}", u.direction_text(0.0)), Accept::TEXT).default(u.direction_text(u.angbase))
            }
            Ask::Clockwise => Prompt::new("Measure angles clockwise?", Accept::TEXT).kw(&["Yes", "No"]).default(if u.clockwise { "Y" } else { "N" }),
        }
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let u = Units::of(s);
        let text = match &i {
            Input::Text(t) => Some(t.as_str()),
            Input::Enter => None,
            Input::Keyword(k) => Some(k.as_str()),
            _ => return Ok(Step::Continue),
        };
        let h = &mut s.doc_mut()?.header;
        match self.ask {
            Ask::LengthFormat => {
                let l = text.map_or(Ok(u.lunits), |t| integer(t, 1, 5))?;
                if l != u.lunits {
                    h.set_i64("LUNITS", l);
                    h.set_i64("LUPREC", 4);
                }
                self.ask = Ask::LengthPrecision;
                if matches!(l, 4 | 5) {
                    s.echo(DENOMINATOR);
                }
            }
            Ask::LengthPrecision => {
                let prec = if u.fractional() {
                    let den = text.map_or(Some(1 << u.luprec), |t| t.trim().parse::<i64>().ok());
                    let Some(p) = den.and_then(|d| (0..=8).find(|p| 1_i64 << p == d)) else {
                        s.echo("Invalid choice.");
                        s.echo(DENOMINATOR);
                        return Ok(Step::Continue);
                    };
                    p
                } else {
                    text.map_or(Ok(u.luprec), |t| integer(t, 0, 8))?
                };
                h.set_i64("LUPREC", prec);
                self.ask = Ask::AngleFormat;
                for l in ANGLE_SYSTEMS {
                    s.echo(l);
                }
            }
            Ask::AngleFormat => {
                let a = text.map_or(Ok(u.aunits + 1), |t| integer(t, 1, 5))? - 1;
                if a != u.aunits {
                    h.set_i64("AUNITS", a);
                    h.set_i64("AUPREC", 4);
                }
                self.ask = Ask::AnglePrecision;
            }
            Ask::AnglePrecision => {
                h.set_i64("AUPREC", text.map_or(Ok(u.auprec), |t| integer(t, 0, 8))?);
                self.ask = Ask::Direction;
                let u = Units::of(s);
                s.echo(format!("Direction for angle {}:", u.direction_text(0.0)));
                for (name, deg) in [("East    3", 0.0), ("North  12", 90.0), ("West    9", 180.0), ("South   6", 270.0)] {
                    s.echo(format!("   {name} o'clock  =  {}", u.direction_text(f64::to_radians(deg))));
                }
            }
            Ask::Direction => {
                if let Some(t) = text {
                    let a = u.direction(t).ok_or_else(|| EngineError::Other("Requires valid numeric angle or two points.".into()))?;
                    h.set_f64("ANGBASE", a);
                }
                self.ask = Ask::Clockwise;
            }
            Ask::Clockwise => {
                let clockwise = match text.map(|t| t.trim().trim_start_matches('_').to_ascii_lowercase()) {
                    None => u.clockwise,
                    Some(t) if t == "y" || t == "yes" => true,
                    Some(t) if t == "n" || t == "no" => false,
                    Some(_) => return Err(EngineError::Other("Invalid option keyword.".into())),
                };
                h.set_i64("ANGDIR", i64::from(clockwise));
                return Ok(Step::Done);
            }
        }
        Ok(Step::Continue)
    }
}
