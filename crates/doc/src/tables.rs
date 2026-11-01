//! Symbol tables and non-graphical objects: layers, linetypes, text and dimension styles,
//! blocks, layouts, named views and UCSs.

use std::sync::Arc;

use cadcraft_color::Color;
use cadcraft_geom::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::{EntityStore, Lineweight};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Layer {
    pub name: String,
    /// Always an index or true colour (never ByLayer/ByBlock).
    pub color: Color,
    pub linetype: String,
    pub lineweight: Lineweight,
    pub on: bool,
    pub frozen: bool,
    pub locked: bool,
    pub plot: bool,
    /// 0..=90 percent.
    pub transparency: u8,
    pub description: String,
    pub plot_style: String,
    /// Frozen in viewports created from now on (New VP Freeze; DXF layer flag 2).
    pub vp_freeze_new: bool,
}

impl Default for Layer {
    fn default() -> Self {
        Layer {
            name: "0".into(),
            color: Color::Index(7),
            linetype: "Continuous".into(),
            lineweight: Lineweight::Default,
            on: true,
            frozen: false,
            locked: false,
            plot: true,
            transparency: 0,
            description: String::new(),
            plot_style: "Normal".into(),
            vp_freeze_new: false,
        }
    }
}

impl Layer {
    pub fn new(name: &str) -> Self {
        Layer { name: name.into(), ..Default::default() }
    }
    pub fn visible(&self) -> bool {
        self.on && !self.frozen
    }
}

/// A linetype element: dash (> 0), gap (< 0) or dot (0); optional embedded text/shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashElement {
    pub length: f64,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub shape: Option<u16>,
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default)]
    pub scale: f64,
    /// Rotation of embedded text or a shape, in radians.
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub offset: Vec2,
    /// The rotation of embedded text or a shape is absolute (`A=`) instead of relative to the
    /// line direction (`R=`).
    #[serde(default)]
    pub absolute: bool,
}

impl DashElement {
    pub fn dash(length: f64) -> Self {
        DashElement { length, text: None, shape: None, style: None, scale: 1.0, rotation: 0.0, offset: Vec2::ZERO, absolute: false }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Linetype {
    pub name: String,
    pub description: String,
    pub pattern: Vec<DashElement>,
}

impl Linetype {
    pub fn continuous() -> Self {
        Linetype { name: "Continuous".into(), description: "Solid line".into(), pattern: Vec::new() }
    }
    pub fn simple(name: &str, description: &str, dashes: &[f64]) -> Self {
        Linetype { name: name.into(), description: description.into(), pattern: dashes.iter().map(|d| DashElement::dash(*d)).collect() }
    }
    pub fn pattern_length(&self) -> f64 {
        self.pattern.iter().map(|d| d.length.abs()).sum()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TextStyle {
    pub name: String,
    /// Font file or family name (TTF family or a stroke-font name).
    pub font: String,
    pub big_font: String,
    /// 0 = not fixed.
    pub height: f64,
    pub width_factor: f64,
    pub oblique: f64,
    pub backwards: bool,
    pub upside_down: bool,
    pub vertical: bool,
    pub annotative: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            name: "Standard".into(),
            font: "CADCraft Stroke".into(),
            big_font: String::new(),
            height: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            backwards: false,
            upside_down: false,
            vertical: false,
            annotative: false,
        }
    }
}

/// Dimension style variables (the subset that affects geometry and text).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DimStyle {
    pub name: String,
    /// DIMSCALE overall scale.
    pub scale: f64,
    /// DIMASZ arrow size.
    pub arrow_size: f64,
    /// DIMEXO extension line offset from origin.
    pub ext_offset: f64,
    /// DIMEXE extension beyond dimension line.
    pub ext_extend: f64,
    /// DIMTXT text height.
    pub text_height: f64,
    /// DIMGAP gap around text.
    pub text_gap: f64,
    /// DIMDEC decimal places.
    pub decimals: u8,
    /// DIMADEC angular decimals.
    pub angular_decimals: u8,
    /// DIMLFAC linear scale factor.
    pub linear_factor: f64,
    /// DIMTAD text above line (0 centred, 1 above).
    pub text_above: u8,
    /// DIMTIH / DIMTOH text inside/outside horizontal.
    pub text_inside_horizontal: bool,
    pub text_outside_horizontal: bool,
    /// DIMBLK arrowhead name ("" = closed filled).
    pub arrow_block: String,
    /// DIMTSZ tick size (> 0 replaces arrows with ticks).
    pub tick_size: f64,
    /// DIMCLRD / DIMCLRE / DIMCLRT.
    pub dim_line_color: Color,
    pub ext_line_color: Color,
    pub text_color: Color,
    /// DIMTFILL, the background of the text: 0 none, 1 the drawing's background color,
    /// 2 `text_fill_color`.
    pub text_fill: u8,
    /// DIMTFILLCLR: the text background color when `text_fill` is 2.
    pub text_fill_color: Color,
    pub text_style: String,
    /// DIMPOST prefix/suffix "<>" template.
    pub post: String,
    /// DIMCEN centre mark size (negative = lines).
    pub center_mark: f64,
    /// DIMZIN zero suppression bits.
    pub zero_suppression: u8,
    /// DIMLUNIT: 1 sci, 2 dec, 3 eng, 4 arch, 5 frac, 6 windows.
    pub linear_unit: u8,
    /// DIMTOL / DIMLIM, DIMTP/DIMTM.
    pub tolerance: bool,
    pub tol_plus: f64,
    pub tol_minus: f64,
    /// DIMDLI baseline spacing.
    pub baseline_spacing: f64,
    pub annotative: bool,
    /// DIMBLK1 / DIMBLK2: per-end arrowheads ("" = use `arrow_block`).
    pub arrow_block1: String,
    pub arrow_block2: String,
    /// DIMDLE dimension line extension past the extension lines (ticks / oblique arrows).
    pub dim_line_extend: f64,
    /// DIMJUST: 0 centred, 1 next to ext line 1, 2 next to ext line 2, 3 over ext line 1, 4 over ext line 2.
    pub text_just: u8,
    /// DIMRND rounding increment (0 = none).
    pub round: f64,
    /// DIMDSEP decimal separator.
    pub decimal_separator: String,
    /// DIMFRAC: 0 horizontal stack, 1 diagonal stack, 2 not stacked.
    pub fraction_format: u8,
    /// DIMLIM: show limits (upper over lower) instead of the measurement.
    pub limits: bool,
    /// DIMTDEC tolerance decimals.
    pub tol_decimals: u8,
    /// DIMTFAC tolerance text height relative to the dimension text.
    pub tol_scale: f64,
    /// DIMTZIN zero suppression of tolerance values (DIMZIN bits).
    pub tol_zero_suppression: u8,
    /// DIMALT alternate units, DIMALTF factor, DIMALTD decimals, DIMAPOST template.
    pub alt: bool,
    pub alt_factor: f64,
    pub alt_decimals: u8,
    pub alt_post: String,
    /// DIMALTRND rounding increment of alternate units (0 = none); DIMRND doesn't apply to them.
    pub alt_round: f64,
    /// DIMALTU: 1 sci, 2 dec, 3 eng, 4 arch stacked, 5 frac stacked, 6 arch, 7 frac, 8 windows.
    pub alt_unit: u8,
    /// DIMALTZ zero suppression of alternate units (DIMZIN bits).
    pub alt_zero_suppression: u8,
    /// DIMAUNIT: 0 decimal degrees, 1 deg/min/sec, 2 grads, 3 radians.
    pub angular_unit: u8,
    /// DIMSE1 / DIMSE2 suppress extension lines.
    pub suppress_ext1: bool,
    pub suppress_ext2: bool,
    /// DIMSD1 / DIMSD2 suppress the first / second half of the dimension line and its arrow.
    pub suppress_dim1: bool,
    pub suppress_dim2: bool,
    /// DIMATFIT, what moves outside the extension lines when text and arrows don't both fit:
    /// 0 both, 1 arrows first, 2 text first, 3 whichever fits best.
    pub fit: u8,
    /// DIMTIX: text always between the extension lines.
    pub text_inside: bool,
    /// DIMSOXD: suppress arrowheads that don't fit between the extension lines.
    pub suppress_outside_arrows: bool,
    /// DIMTOFL: a dimension line between the extension lines even when the arrows are outside
    /// (through the circle for radius and diameter dimensions with the text outside).
    pub force_line_inside: bool,
    /// DIMTMOVE, when the text is moved away from its default place: 0 the dimension line
    /// moves with it, 1 a leader joins them, 2 neither.
    pub text_move: u8,
    /// DIMTVP: text above (+) or below (-) the dimension line by this many text heights when
    /// DIMTAD is 0.
    pub text_vertical_pos: f64,
    /// DIMTOLJ, vertical place of tolerance values next to the measurement: 0 bottom,
    /// 1 middle, 2 top.
    pub tol_just: u8,
    /// DIMAZIN zero suppression of angular dimensions: 1 leading, 2 trailing.
    pub angular_zero_suppression: u8,
    /// DIMARCSYM, arc length symbol: 0 before the text, 1 above it, 2 none.
    pub arc_symbol: u8,
    /// DIMLDRBLK leader arrowhead ("" = closed filled).
    pub leader_arrow_block: String,
    /// DIMLTYPE / DIMLTEX1 / DIMLTEX2: linetypes of the dimension line and the extension
    /// lines ("" = ByBlock).
    pub dim_linetype: String,
    pub ext_linetype1: String,
    pub ext_linetype2: String,
    /// DIMLWD / DIMLWE: lineweights of the dimension line and the extension lines in 1/100 mm
    /// (-1 ByLayer, -2 ByBlock, -3 default).
    pub dim_lineweight: i16,
    pub ext_lineweight: i16,
    /// DIMFXLON / DIMFXL: extension lines of a fixed length, measured from the dimension line
    /// toward the origin.
    pub fixed_ext: bool,
    pub fixed_ext_length: f64,
    /// DIMJOGANG: angle (degrees) of the jog's middle segment in jogged radius dimensions.
    pub jog_angle: f64,
}

/// DIMSTYLE system variable names and the [`DimStyle`] fields they set.
pub const DIMVARS: &[(&str, &str)] = &[
    ("DIMSCALE", "scale"),
    ("DIMASZ", "arrowSize"),
    ("DIMEXO", "extOffset"),
    ("DIMEXE", "extExtend"),
    ("DIMTXT", "textHeight"),
    ("DIMGAP", "textGap"),
    ("DIMDEC", "decimals"),
    ("DIMADEC", "angularDecimals"),
    ("DIMLFAC", "linearFactor"),
    ("DIMTAD", "textAbove"),
    ("DIMTIH", "textInsideHorizontal"),
    ("DIMTOH", "textOutsideHorizontal"),
    ("DIMBLK", "arrowBlock"),
    ("DIMBLK1", "arrowBlock1"),
    ("DIMBLK2", "arrowBlock2"),
    ("DIMTSZ", "tickSize"),
    ("DIMCLRD", "dimLineColor"),
    ("DIMCLRE", "extLineColor"),
    ("DIMCLRT", "textColor"),
    ("DIMTFILL", "textFill"),
    ("DIMTFILLCLR", "textFillColor"),
    ("DIMTXSTY", "textStyle"),
    ("DIMPOST", "post"),
    ("DIMCEN", "centerMark"),
    ("DIMZIN", "zeroSuppression"),
    ("DIMLUNIT", "linearUnit"),
    ("DIMTOL", "tolerance"),
    ("DIMTP", "tolPlus"),
    ("DIMTM", "tolMinus"),
    ("DIMDLI", "baselineSpacing"),
    ("DIMDLE", "dimLineExtend"),
    ("DIMJUST", "textJust"),
    ("DIMRND", "round"),
    ("DIMDSEP", "decimalSeparator"),
    ("DIMFRAC", "fractionFormat"),
    ("DIMLIM", "limits"),
    ("DIMTDEC", "tolDecimals"),
    ("DIMTFAC", "tolScale"),
    ("DIMTZIN", "tolZeroSuppression"),
    ("DIMALT", "alt"),
    ("DIMALTF", "altFactor"),
    ("DIMALTD", "altDecimals"),
    ("DIMAPOST", "altPost"),
    ("DIMALTRND", "altRound"),
    ("DIMALTU", "altUnit"),
    ("DIMALTZ", "altZeroSuppression"),
    ("DIMAUNIT", "angularUnit"),
    ("DIMSE1", "suppressExt1"),
    ("DIMSE2", "suppressExt2"),
    ("DIMSD1", "suppressDim1"),
    ("DIMSD2", "suppressDim2"),
    ("DIMATFIT", "fit"),
    ("DIMTIX", "textInside"),
    ("DIMSOXD", "suppressOutsideArrows"),
    ("DIMTOFL", "forceLineInside"),
    ("DIMTMOVE", "textMove"),
    ("DIMTVP", "textVerticalPos"),
    ("DIMTOLJ", "tolJust"),
    ("DIMAZIN", "angularZeroSuppression"),
    ("DIMARCSYM", "arcSymbol"),
    ("DIMLDRBLK", "leaderArrowBlock"),
    ("DIMLTYPE", "dimLinetype"),
    ("DIMLTEX1", "extLinetype1"),
    ("DIMLTEX2", "extLinetype2"),
    ("DIMLWD", "dimLineweight"),
    ("DIMLWE", "extLineweight"),
    ("DIMFXLON", "fixedExt"),
    ("DIMFXL", "fixedExtLength"),
    ("DIMJOGANG", "jogAngle"),
];

impl Default for DimStyle {
    fn default() -> Self {
        DimStyle {
            name: "Standard".into(),
            scale: 1.0,
            arrow_size: 0.18,
            ext_offset: 0.0625,
            ext_extend: 0.18,
            text_height: 0.18,
            text_gap: 0.09,
            decimals: 4,
            angular_decimals: 0,
            linear_factor: 1.0,
            text_above: 0,
            text_inside_horizontal: true,
            text_outside_horizontal: true,
            arrow_block: String::new(),
            tick_size: 0.0,
            dim_line_color: Color::ByBlock,
            ext_line_color: Color::ByBlock,
            text_color: Color::ByBlock,
            text_fill: 0,
            text_fill_color: Color::ByBlock,
            text_style: "Standard".into(),
            post: String::new(),
            center_mark: 0.09,
            zero_suppression: 0,
            linear_unit: 2,
            tolerance: false,
            tol_plus: 0.0,
            tol_minus: 0.0,
            baseline_spacing: 0.38,
            annotative: false,
            arrow_block1: String::new(),
            arrow_block2: String::new(),
            dim_line_extend: 0.0,
            text_just: 0,
            round: 0.0,
            decimal_separator: ".".into(),
            fraction_format: 0,
            limits: false,
            tol_decimals: 4,
            tol_scale: 1.0,
            tol_zero_suppression: 0,
            alt: false,
            alt_factor: 25.4,
            alt_decimals: 2,
            alt_post: String::new(),
            alt_round: 0.0,
            alt_unit: 2,
            alt_zero_suppression: 0,
            angular_unit: 0,
            suppress_ext1: false,
            suppress_ext2: false,
            suppress_dim1: false,
            suppress_dim2: false,
            fit: 3,
            text_inside: false,
            suppress_outside_arrows: false,
            force_line_inside: false,
            text_move: 0,
            text_vertical_pos: 0.0,
            tol_just: 1,
            angular_zero_suppression: 0,
            arc_symbol: 0,
            leader_arrow_block: String::new(),
            dim_linetype: String::new(),
            ext_linetype1: String::new(),
            ext_linetype2: String::new(),
            dim_lineweight: -2,
            ext_lineweight: -2,
            fixed_ext: false,
            fixed_ext_length: 1.0,
            jog_angle: 45.0,
        }
    }
}

impl DimStyle {
    /// The field a name refers to: a camelCase field name or a DIM* variable name.
    pub fn field_name(name: &str) -> Option<&'static str> {
        DIMVARS
            .iter()
            .find(|(var, field)| var.eq_ignore_ascii_case(name) || field.eq_ignore_ascii_case(name))
            .map(|(_, f)| *f)
            .or_else(|| ["name", "annotative"].into_iter().find(|f| f.eq_ignore_ascii_case(name)))
    }

    /// The value `v` takes for a field whose current value is `old`: coerced to the field's type where
    /// the text or number can be read as it.
    fn coerce_value(old: &serde_json::Value, v: &serde_json::Value, field: &str) -> serde_json::Value {
        use serde_json::Value;
        match (old, v) {
            (Value::Bool(_), Value::Number(n)) => Value::Bool(n.as_f64().is_some_and(|x| x != 0.0)),
            (Value::Bool(_), Value::String(t)) => Value::Bool(matches!(t.to_ascii_lowercase().as_str(), "1" | "on" | "true" | "yes")),
            (Value::Number(_), Value::String(t)) => match t.trim().parse::<f64>() {
                Ok(x) => serde_json::json!(x),
                // Lineweight names.
                Err(_) if matches!(field, "dimLineweight" | "extLineweight") => match t.trim().to_ascii_lowercase().as_str() {
                    "bylayer" => serde_json::json!(-1),
                    "byblock" => serde_json::json!(-2),
                    "default" => serde_json::json!(-3),
                    _ => v.clone(),
                },
                Err(_) => v.clone(),
            },
            (Value::Number(_), Value::Bool(b)) => serde_json::json!(u8::from(*b)),
            (Value::String(_), Value::Number(n)) => Value::String(n.to_string()),
            (Value::Object(_), Value::Number(n)) => {
                let c = n.as_i64().and_then(|i| i16::try_from(i).ok()).map(Color::from_aci);
                c.and_then(|c| serde_json::to_value(c).ok()).unwrap_or_else(|| v.clone())
            }
            (Value::Object(_), Value::String(t)) => Color::parse(t).and_then(|c| serde_json::to_value(c).ok()).unwrap_or_else(|| v.clone()),
            _ => v.clone(),
        }
    }

    /// Whether `coerced` is outside the range its `field` can take.
    fn out_of_range(field: &str, coerced: &serde_json::Value) -> bool {
        let x = coerced.as_f64();
        match field {
            "textHeight" | "tolScale" => x.is_some_and(|x| x <= 0.0),
            "scale" | "arrowSize" | "tickSize" | "fixedExtLength" => x.is_some_and(|x| x < 0.0),
            "fit" | "angularZeroSuppression" => x.is_some_and(|x| x > 3.0),
            "textMove" | "tolJust" | "arcSymbol" | "textFill" => x.is_some_and(|x| x > 2.0),
            "dimLineweight" | "extLineweight" => x.is_some_and(|x| !(-3.0..=211.0).contains(&x)),
            "jogAngle" => x.is_some_and(|x| !(5.0..=90.0).contains(&x)),
            _ => false,
        }
    }

    /// Set fields from a JSON object of field or DIM* variable names (numbers, booleans as
    /// 0/1, colours as ACI numbers or names). Returns the names that were not recognised or
    /// had unusable values.
    pub fn apply_fields(&mut self, src: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
        use serde_json::Value;
        let mut rejected = Vec::new();
        let Ok(mut cur) = serde_json::to_value(&*self) else { return src.keys().cloned().collect() };
        for (k, v) in src {
            let Some(field) = DimStyle::field_name(k).filter(|f| *f != "name") else {
                rejected.push(k.clone());
                continue;
            };
            let Some(obj) = cur.as_object_mut() else { break };
            let Some(old) = obj.get(field).cloned() else {
                rejected.push(k.clone());
                continue;
            };
            let coerced = Self::coerce_value(&old, v, field);
            // Integers stored in u8 fields: round floats.
            let coerced = match (&old, &coerced) {
                // Lineweights (i16): whole hundredths of a mm or -1/-2/-3.
                (Value::Number(_), Value::Number(n)) if matches!(field, "dimLineweight" | "extLineweight") && !n.is_i64() => {
                    serde_json::json!(n.as_f64().filter(|x| x.is_finite()).unwrap_or(0.0).round() as i64)
                }
                (Value::Number(_), Value::Number(_)) if matches!(field, "dimLineweight" | "extLineweight") => coerced,
                // A value below 0 or one that rounds above 255 is refused; DIMADEC -1 reads as 0.
                (Value::Number(o), Value::Number(n)) if o.is_u64() && !n.is_u64() => {
                    let min = if field == "angularDecimals" { -1.0 } else { 0.0 };
                    match n.as_f64().filter(|x| x.is_finite() && *x >= min).map(f64::round).filter(|x| *x <= 255.0) {
                        Some(x) => serde_json::json!(x.max(0.0) as u64),
                        None => {
                            rejected.push(k.clone());
                            continue;
                        }
                    }
                }
                _ => coerced,
            };
            // Sizes that can't be negative (text height and tolerance scale: not zero either).
            let out_of_range = Self::out_of_range(field, &coerced);
            if out_of_range {
                rejected.push(k.clone());
                continue;
            }
            obj.insert(field.to_string(), coerced);
            match serde_json::from_value::<DimStyle>(cur.clone()) {
                Ok(ns) => *self = ns,
                Err(_) => {
                    rejected.push(k.clone());
                    if let Some(o) = cur.as_object_mut() {
                        o.insert(field.to_string(), old);
                    }
                }
            }
        }
        rejected
    }

    /// The overall scale: the style's DIMSCALE, else the drawing's `dimscale`, else 1. Zero means
    /// "scale to the layout viewport" in AutoCAD, which isn't computed here.
    pub fn effective_scale(&self, dimscale: f64) -> f64 {
        [self.scale, dimscale].into_iter().find(|k| *k > 0.0 && k.is_finite()).unwrap_or(1.0)
    }

    /// This style with a dimension's overrides applied.
    pub fn with_overrides(&self, o: &serde_json::Map<String, serde_json::Value>) -> DimStyle {
        let mut st = self.clone();
        if !o.is_empty() {
            st.apply_fields(o);
        }
        st
    }

    /// DIMTMOVE 0: the dimension line of a linear or aligned dimension follows text the user
    /// placed (`text_mid`), keeping the DIMTAD / DIMTVP offset. Other kinds, text in its
    /// default place and other DIMTMOVE values are left alone.
    pub fn follow_text(&self, d: &mut crate::Dimension, dimscale: f64) {
        if self.text_move != 0 || !d.user_text_pos {
            return;
        }
        let dir = match d.kind {
            crate::DimKind::Linear { rotation } => Vec2::from_angle(rotation),
            crate::DimKind::Aligned => (d.p14.xy() - d.p13.xy()).normalized(),
            _ => return,
        };
        if dir == Vec2::ZERO || !dir.is_finite() {
            return;
        }
        let n = dir.perp();
        let k = self.effective_scale(dimscale);
        let (th, gap) = (self.text_height * k, self.text_gap.abs() * k);
        let above = if n.y.abs() > 1e-9 {
            if n.y > 0.0 { n } else { -n }
        } else if n.x < 0.0 {
            n
        } else {
            -n
        };
        let away = if (d.defpt.xy() - d.p13.xy()).dot(n) >= 0.0 { n } else { -n };
        let off = match self.text_above {
            0 => above * (self.text_vertical_pos * th),
            2 => away * (gap + th / 2.0),
            4 => -above * (gap + th / 2.0),
            _ => above * (gap + th / 2.0),
        };
        let shift = n * (d.text_mid.xy() - off - d.defpt.xy()).dot(n);
        if shift.is_finite() {
            d.defpt.x += shift.x;
            d.defpt.y += shift.y;
        }
    }

    /// The ISO-25 style for metric drawings.
    pub fn iso25() -> Self {
        DimStyle {
            name: "ISO-25".into(),
            arrow_size: 2.5,
            ext_offset: 0.625,
            ext_extend: 1.25,
            text_height: 2.5,
            text_gap: 0.625,
            decimals: 2,
            text_above: 1,
            text_inside_horizontal: false,
            text_outside_horizontal: false,
            center_mark: 2.5,
            zero_suppression: 8,
            baseline_spacing: 3.75,
            force_line_inside: true,
            tol_just: 0,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MLeaderStyle {
    pub name: String,
    pub arrow_size: f64,
    pub text_height: f64,
    pub landing_gap: f64,
    pub dogleg: f64,
    pub text_style: String,
    /// New multileaders of this style are annotative.
    pub annotative: bool,
}

impl Default for MLeaderStyle {
    fn default() -> Self {
        MLeaderStyle {
            name: "Standard".into(),
            arrow_size: 0.18,
            text_height: 0.18,
            landing_gap: 0.09,
            dogleg: 0.36,
            text_style: "Standard".into(),
            annotative: false,
        }
    }
}

impl MLeaderStyle {
    /// The Standard style for metric drawings: text and arrows as high as ISO-25's.
    pub fn metric() -> Self {
        MLeaderStyle { arrow_size: 2.5, text_height: 2.5, landing_gap: 0.625, dogleg: 5.0, ..Default::default() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TableStyle {
    pub name: String,
    pub text_height: f64,
    pub margin: f64,
    pub title: bool,
    pub header: bool,
    /// Cell styles: custom ones, and Title/Header/Data where they differ from what the text
    /// height and margin imply ([`TableStyle::cell_style`]).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cell_styles: Vec<crate::CellStyle>,
}

impl Default for TableStyle {
    fn default() -> Self {
        TableStyle { name: "Standard".into(), text_height: 0.18, margin: 0.06, title: true, header: true, cell_styles: Vec::new() }
    }
}

impl TableStyle {
    /// The Standard style for metric drawings: text as high as ISO-25's.
    pub fn metric() -> Self {
        TableStyle { text_height: 2.5, margin: 1.0, ..Default::default() }
    }
}

mod cellstyle;
mod tablebreak;
pub use cellstyle::*;
pub use tablebreak::*;

/// A block definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub name: String,
    /// References to this block are annotative.
    #[serde(default)]
    pub annotative: bool,
    pub base: Vec3,
    #[serde(skip)]
    pub entities: EntityStore,
    #[serde(default)]
    pub description: String,
    /// An xref (`xref_path`: the referenced drawing's path as the user gave it, absolute,
    /// relative to the host drawing's folder or a bare file name) attached as an overlay (DXF
    /// BLOCK flag 8): not loaded when the host drawing is itself referenced by another drawing.
    /// An xref's objects come from that file and are never saved with the host.
    #[serde(default)]
    pub xref_overlay: bool,
    /// Whether an xref's objects are loaded, and why not (session state; not saved).
    #[serde(default)]
    pub xref_status: XrefStatus,
    /// Anonymous (`*D12`, `*U3`…) blocks are generated geometry.
    #[serde(default)]
    pub anonymous: bool,
    #[serde(default)]
    pub xref_path: Option<String>,
    #[serde(default)]
    pub explodable: bool,
    #[serde(default)]
    pub units: u8,
    /// The parameters of an associative array when this is an array block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub array: Option<crate::ArrayParams>,
}

/// The load state of an external reference.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum XrefStatus {
    /// Not loaded yet (read from a file, not resolved).
    #[default]
    NotLoaded,
    Loaded,
    /// Unloaded by the user (XREF Unload): kept, not drawn.
    Unloaded,
    /// The referenced file was not found.
    NotFound,
    /// The file was found but could not be loaded (unreadable, too large, a circular reference).
    Unresolved,
}

impl XrefStatus {
    /// The status as the External References palette shows it.
    pub fn label(self) -> &'static str {
        match self {
            XrefStatus::NotLoaded => "Not loaded",
            XrefStatus::Loaded => "Loaded",
            XrefStatus::Unloaded => "Unloaded",
            XrefStatus::NotFound => "Not Found",
            XrefStatus::Unresolved => "Unresolved",
        }
    }
}

impl Block {
    pub fn new(name: &str) -> Self {
        Block {
            name: name.into(),
            annotative: false,
            base: Vec3::ZERO,
            entities: EntityStore::new(),
            xref_overlay: false,
            xref_status: XrefStatus::NotLoaded,
            description: String::new(),
            anonymous: name.starts_with('*'),
            xref_path: None,
            explodable: true,
            units: 0,
            array: None,
        }
    }
}

/// Paper size and plot settings for a layout (page setup).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PageSetup {
    pub device: String,
    pub paper: String,
    /// Paper size in millimetres (portrait width/height before rotation).
    pub width_mm: f64,
    pub height_mm: f64,
    pub margins_mm: [f64; 4],
    pub landscape: bool,
    /// "layout" | "extents" | "display" | "window" | "limits".
    pub plot_area: String,
    /// Plot window corners `[x1, y1, x2, y2]` (DXF plot layout groups 48, 49, 140, 141); used when `plot_area` is "window".
    pub plot_window: [f64; 4],
    pub scale_to_fit: bool,
    /// Paper units per drawing unit.
    pub scale: f64,
    pub plot_style_table: String,
    pub center: bool,
    pub lineweights: bool,
    /// Draw the layout on screen (and in its PNG/SVG exports) with its plot style table
    /// applied ("Display plot styles"; DXF plot layout flag 2).
    pub show_plot_styles: bool,
    /// Plot objects with their transparency ("Plot transparency"; PLOTTRANSPARENCYOVERRIDE can
    /// override it). The DXF plot layout flags have no bit for it, so it is not saved in DXF.
    pub plot_transparency: bool,
    /// The layout's paper units are millimetres (plot paper units 1 in DXF) whatever the
    /// drawing's MEASUREMENT says; otherwise paper units follow MEASUREMENT (#1058).
    pub paper_mm: bool,
}

impl Default for PageSetup {
    fn default() -> Self {
        PageSetup {
            device: "None".into(),
            paper: "ANSI A (8.50 x 11.00 Inches)".into(),
            width_mm: 215.9,
            height_mm: 279.4,
            margins_mm: [6.35, 6.35, 6.35, 6.35],
            landscape: true,
            plot_area: "layout".into(),
            plot_window: [0.0; 4],
            scale_to_fit: false,
            scale: 1.0,
            plot_style_table: String::new(),
            center: false,
            lineweights: true,
            show_plot_styles: false,
            plot_transparency: false,
            paper_mm: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub name: String,
    pub tab_order: u32,
    pub page: PageSetup,
    #[serde(skip)]
    pub entities: EntityStore,
    /// Saved paper-space view (centre, height).
    #[serde(default)]
    pub view: Option<(Vec2, f64)>,
}

impl Layout {
    pub fn new(name: &str, tab_order: u32) -> Self {
        Layout { name: name.into(), tab_order, page: PageSetup::default(), entities: EntityStore::new(), view: None }
    }
}

/// A named view (VIEW table): the centre and size of the area shown, in model space or a layout.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedView {
    pub name: String,
    pub center: Vec2,
    pub height: f64,
    pub width: f64,
    /// The layer state restored with the view (its layer snapshot).
    #[serde(default)]
    pub layer_state: Option<String>,
    /// View twist (radians, counterclockwise); kept for files, not applied to the 2D display.
    #[serde(default)]
    pub twist: f64,
    /// The layout a paper space view belongs to (`None`: a model space view; an empty name: a
    /// paper space view whose layout is not known).
    #[serde(default)]
    pub layout: Option<String>,
}

/// One tiled model space viewport of a [`VportConfig`] (a VPORT table record): its corners as
/// fractions of the drawing area (0,0 lower left to 1,1 upper right) and its view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VportTile {
    pub min: Vec2,
    pub max: Vec2,
    pub center: Vec2,
    pub height: f64,
    /// Width / height of the tile on screen when it was saved (0: unknown).
    #[serde(default)]
    pub aspect: f64,
}

/// A model space viewport configuration (VPORTS): `*Active` is the one shown, other names are
/// saved configurations. The first tile is the current one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VportConfig {
    pub name: String,
    pub tiles: Vec<VportTile>,
}

impl VportConfig {
    /// The name of the configuration shown.
    pub const ACTIVE: &str = "*Active";
    pub fn is_active(&self) -> bool {
        self.name.eq_ignore_ascii_case(Self::ACTIVE)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ucs {
    pub name: String,
    pub origin: Vec3,
    pub x_axis: Vec3,
    pub y_axis: Vec3,
}

/// A saved layer state (LAYERSTATE).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerState {
    pub name: String,
    pub layers: Vec<Layer>,
}

/// A named group of entities (GROUP).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub name: String,
    pub description: String,
    pub selectable: bool,
    pub members: Vec<crate::Handle>,
}

/// Shared ownership helper: blocks are cloned on write.
pub type BlockRef = Arc<Block>;
