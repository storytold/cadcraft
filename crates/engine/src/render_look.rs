//! How a render request draws: its `background` and `plot_style` (#803). The app's control
//! channel and the headless MCP backend both answer `ui.render`, and both read these here.
//!
//! Colour 7 (white/black) follows the background, as on a plot: the rasteriser draws it white on
//! a dark background and black on a light one ([`cadcraft_color::display_rgb`]).

use std::sync::Arc;

use cadcraft_color::Rgb;
use cadcraft_doc::{Drawing, PlotStyleTable};
use cadcraft_render::raster::RasterOptions;
use serde_json::Value;

/// The background a render draws on without `background`: the dark canvas.
pub const DARK: Rgb = Rgb(33, 40, 48);
/// The background of `"white"` and `"paper"`.
pub const WHITE: Rgb = Rgb(255, 255, 255);

/// What `background` accepts, for the error message and the docs.
pub const BACKGROUNDS: &str = "\"dark\" (the default), \"white\", \"paper\" or \"#rrggbb\"";

/// Longest part of a refused value repeated in its error message.
const ECHO: usize = 40;

/// The look of a render: what it draws on and with which plot style table.
#[derive(Clone, Debug)]
pub struct RenderLook {
    pub background: Rgb,
    /// `None` = draw with the objects' own properties (or, in a layout showing plot styles, its
    /// page setup's table).
    pub plot_style_table: Option<Arc<PlotStyleTable>>,
}

impl Default for RenderLook {
    fn default() -> Self {
        RenderLook { background: DARK, plot_style_table: None }
    }
}

impl RenderLook {
    /// `background` and `plot_style` of a request's params for drawing `d`; absent or `null`
    /// keeps the default. `Err` says what is accepted.
    pub fn from_params(d: &Drawing, p: &Value) -> Result<RenderLook, String> {
        let text = |key: &str| match p.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.as_str())),
            Some(_) => Err(format!("`{key}` must be a string")),
        };
        let mut look = RenderLook::default();
        if let Some(b) = text("background")? {
            look.background = background(b).ok_or_else(|| format!("unknown background `{}`: use {BACKGROUNDS}", echo(b)))?;
        }
        if let Some(name) = text("plot_style")? {
            look.plot_style_table = plot_style(d, name)?.map(Arc::new);
        }
        Ok(look)
    }

    /// The raster options of a render with this look.
    pub fn raster(&self) -> RasterOptions {
        RasterOptions { background: self.background, ..RasterOptions::default() }
    }
}

/// The colour of a `background` value (any case): `dark`, `white`, `paper` or `#rrggbb`.
pub fn background(v: &str) -> Option<Rgb> {
    let v = v.trim();
    match v.to_ascii_lowercase().as_str() {
        "dark" => Some(DARK),
        "white" | "paper" => Some(WHITE),
        hex if hex.starts_with('#') => Rgb::parse_hex(hex),
        _ => None,
    }
}

/// The plot style table `name` names: one the drawing holds, or a built-in one (`.ctb` may be
/// left out); `none` or an empty name is no table. `Err` lists the tables there are.
fn plot_style(d: &Drawing, name: &str) -> Result<Option<PlotStyleTable>, String> {
    if cadcraft_doc::no_plot_style_table(name) {
        return Ok(None);
    }
    match cadcraft_doc::plot_style_table(d, name.trim()) {
        Some(t) => Ok(Some(t)),
        None => Err(format!("unknown plot style table `{}`: use one of {}", echo(name), cadcraft_doc::plot_style_table_names(d).join(", "))),
    }
}

/// The start of a refused value, to repeat in its error message.
fn echo(v: &str) -> String {
    v.chars().take(ECHO).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn backgrounds() {
        assert_eq!(background("dark"), Some(DARK));
        assert_eq!(DARK, RasterOptions::default().background, "the default is the rasteriser's canvas");
        assert_eq!(background(" Paper "), Some(WHITE));
        assert_eq!(background("WHITE"), Some(WHITE));
        assert_eq!(background("#336699"), Some(Rgb(0x33, 0x66, 0x99)));
        for bad in ["", "purple", "336699", "#12345", "#1234567", "##336699", "#33669g"] {
            assert_eq!(background(bad), None, "{bad}");
        }
    }

    #[test]
    fn from_params() {
        let d = Drawing::new_imperial();
        let look = RenderLook::from_params(&d, &json!({})).unwrap();
        assert_eq!(look.background, DARK);
        assert!(look.plot_style_table.is_none());
        let look = RenderLook::from_params(&d, &json!({"background": null, "plot_style": "none"})).unwrap();
        assert_eq!(look.background, DARK);
        assert!(look.plot_style_table.is_none());
        let look = RenderLook::from_params(&d, &json!({"background": "white", "plot_style": "Monochrome"})).unwrap();
        assert_eq!(look.raster().background, WHITE);
        assert_eq!(look.plot_style_table.map(|t| t.name.clone()).as_deref(), Some("monochrome.ctb"));
        let e = RenderLook::from_params(&d, &json!({"plot_style": "nope.ctb"})).unwrap_err();
        assert!(e.contains("nope.ctb") && e.contains("grayscale.ctb"), "{e}");
        let e = RenderLook::from_params(&d, &json!({"background": "x".repeat(10_000)})).unwrap_err();
        assert!(e.len() < 200 && e.contains("#rrggbb"), "{e}");
        assert!(RenderLook::from_params(&d, &json!({"background": 1})).is_err());
        assert!(RenderLook::from_params(&d, &json!({"plot_style": {}})).is_err());
    }
}
