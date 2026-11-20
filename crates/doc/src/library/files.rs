//! Readers for the user's own linetype (`.lin`) and hatch pattern (`.pat`) files.
//!
//! Grammar and hostile-input capping live in [`craft_dxf::lin_pat`]; this module maps the
//! parsed definitions into CADCraft's [`Linetype`] and [`UserPattern`] document types.

use cadcraft_geom::Vec2;
pub use craft_dxf::lin_pat::{MAX_DEFINITIONS, MAX_LINE_CHARS, MAX_LINETYPE_ELEMENTS, MAX_PATTERN_DASHES, MAX_PATTERN_FAMILIES, MAX_VALUE, Parsed};

use super::PatternLine;
use crate::{DashElement, Linetype};

/// A hatch pattern read from a user's `.pat` file.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UserPattern {
    pub name: String,
    pub description: String,
    pub lines: Vec<PatternLine>,
}

/// Read the linetypes of a `.lin` file. Simple linetypes and complex ones with embedded text
/// (`["TEXT",STYLE,S=…,R=…,A=…,U=…,X=…,Y=…]`) are read; shape elements (`[SHAPE,file.shx,…]`)
/// need SHX shape fonts, so they are left out with a warning and the dashes around them kept.
/// Lengths are in drawing units as written: a user's file is never rescaled.
pub fn parse_lin(text: &str) -> Parsed<Linetype> {
    let parsed = craft_dxf::lin_pat::parse_lin(text);
    Parsed {
        items: parsed
            .items
            .into_iter()
            .map(|d| Linetype {
                name: d.name,
                description: d.description,
                pattern: d
                    .pattern
                    .into_iter()
                    .map(|e| DashElement {
                        length: e.length,
                        text: e.text,
                        shape: None,
                        style: e.style,
                        scale: e.scale,
                        rotation: e.rotation,
                        offset: Vec2::new(e.offset[0], e.offset[1]),
                        absolute: e.absolute,
                    })
                    .collect(),
            })
            .collect(),
        warnings: parsed.warnings,
    }
}

/// Read the hatch patterns of a `.pat` file. Values are used as written (an inch file for inch
/// drawings, a millimetre file for metric ones).
pub fn parse_pat(text: &str) -> Parsed<UserPattern> {
    let parsed = craft_dxf::lin_pat::parse_pat(text);
    Parsed {
        items: parsed
            .items
            .into_iter()
            .map(|p| UserPattern {
                name: p.name,
                description: p.description,
                lines: p
                    .lines
                    .into_iter()
                    .map(|l| PatternLine { angle: l.angle, origin: (l.origin[0], l.origin[1]), delta: (l.delta[0], l.delta[1]), dashes: l.dashes })
                    .collect(),
            })
            .collect(),
        warnings: parsed.warnings,
    }
}
