//! How a render request draws: its `background` and `plot_style` (#803). The app's control
//! channel and the headless MCP backend both answer `ui.render`, and both read these here.
//!
//! Colour 7 (white/black) follows the background, as on a plot: the rasteriser draws it white on
//! a dark background and black on a light one ([`cadcraft_color::display_rgb`]).
//!
//! A render with a plot style table draws on white unless `background` says otherwise, as a plot
//! prints on paper: the tables that change colours print colour 7 (and, in `monochrome.ctb`,
//! every colour) black, which can hardly be seen on the dark canvas.
//!
//! `plot_style: "none"` (or `""`) draws every object with its own properties, also in a layout
//! whose page setup has "Display plot styles" on; without `plot_style` such a layout draws with
//! its own table, as on screen.

use std::sync::Arc;

use cadcraft_color::Rgb;
use cadcraft_doc::{Drawing, PlotStyleTable};
use cadcraft_render::raster::RasterOptions;
use serde_json::Value;

/// The background a render draws on without `background`: the dark canvas.
pub const DARK: Rgb = RasterOptions::DARK_BACKGROUND;
/// The background of `"white"` and `"paper"`.
pub const WHITE: Rgb = Rgb(255, 255, 255);

/// What `background` accepts, for the error message and the docs.
pub const BACKGROUNDS: &str = "\"dark\" (the default without a plot style), \"white\" (the default with one), \"paper\" or \"#rrggbb\"";

/// Most table names an unknown `plot_style` error lists from the drawing's own tables (the
/// built-in ones are always listed).
const MAX_LISTED_TABLES: usize = 10;

/// Longest part of a refused value repeated in its error message, in characters.
const MAX_ECHO_CHARS: usize = 40;

/// The look of a render: what it draws on and with which plot style table.
#[derive(Clone, Debug)]
pub struct RenderLook {
    pub background: Rgb,
    /// `None` = draw with the objects' own properties (or, in a layout showing plot styles, its
    /// page setup's table). `plot_style: "none"` is a table that keeps every property, so such a
    /// layout's table doesn't apply either.
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
        if let Some(name) = text("plot_style")? {
            look.plot_style_table = Some(Arc::new(match plot_style(d, name)? {
                Some(t) => {
                    // Plotted on paper, unless `background` says otherwise.
                    look.background = WHITE;
                    t
                }
                None => object_properties(),
            }));
        }
        if let Some(b) = text("background")? {
            look.background = background(b).ok_or_else(|| format!("unknown background `{}`: use {BACKGROUNDS}", echo(b)))?;
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
    cadcraft_doc::plot_style_table(d, name.trim())
        .map(Some)
        .ok_or_else(|| format!("unknown plot style table `{}`: use none or one of {}", echo(name), table_list(d)))
}

/// The tables a drawing can use, for an error message: the built-in ones, then at most
/// [`MAX_LISTED_TABLES`] of the drawing's own and how many more there are.
fn table_list(d: &Drawing) -> String {
    let builtin = |n: &String| cadcraft_doc::BUILTIN_PLOT_STYLE_TABLES.iter().any(|b| b.eq_ignore_ascii_case(n));
    let names = cadcraft_doc::plot_style_table_names(d);
    let own: Vec<String> = names.iter().filter(|n| !builtin(n)).map(|n| echo(n)).collect();
    let mut list: Vec<String> = cadcraft_doc::BUILTIN_PLOT_STYLE_TABLES.iter().map(|b| (*b).to_string()).collect();
    list.extend(own.iter().take(MAX_LISTED_TABLES).cloned());
    let mut text = list.join(", ");
    if own.len() > MAX_LISTED_TABLES {
        text.push_str(&format!(" and {} more of the drawing's", own.len() - MAX_LISTED_TABLES));
    }
    text
}

/// A table that prints every object with its own properties: what `plot_style: "none"` draws
/// with, so a layout's own table doesn't apply either.
fn object_properties() -> PlotStyleTable {
    PlotStyleTable::color_dependent("None", "Object properties", |_| None)
}

/// The start of a refused value, to repeat in its error message.
fn echo(v: &str) -> String {
    v.chars().take(MAX_ECHO_CHARS).collect()
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
        // "none" keeps the dark canvas and draws every object with its own properties, also in a
        // layout that shows its own plot styles.
        for none in ["none", "", " None "] {
            let look = RenderLook::from_params(&d, &json!({"background": null, "plot_style": none})).unwrap();
            assert_eq!(look.background, DARK, "{none:?}");
            let t = look.plot_style_table.unwrap();
            assert!(t.is_color_dependent() && t.styles.iter().all(|s| s.keeps_color() && s.lineweight.is_none()), "{none:?}");
        }
        let look = RenderLook::from_params(&d, &json!({"background": "white", "plot_style": "Monochrome"})).unwrap();
        assert_eq!(look.raster().background, WHITE);
        assert_eq!(look.plot_style_table.map(|t| t.name.clone()).as_deref(), Some("monochrome.ctb"));
        // A plot style plots on paper unless `background` says otherwise.
        for table in ["monochrome.ctb", "grayscale.ctb", "default.stb"] {
            assert_eq!(RenderLook::from_params(&d, &json!({"plot_style": table})).unwrap().background, WHITE, "{table}");
            let look = RenderLook::from_params(&d, &json!({"plot_style": table, "background": "dark"})).unwrap();
            assert_eq!(look.background, DARK, "{table}");
        }
        let e = RenderLook::from_params(&d, &json!({"plot_style": "nope.ctb"})).unwrap_err();
        assert!(e.contains("nope.ctb") && e.contains("grayscale.ctb") && e.contains("none"), "{e}");
        let e = RenderLook::from_params(&d, &json!({"background": "x".repeat(10_000)})).unwrap_err();
        assert!(e.len() < 200 && e.contains("#rrggbb"), "{e}");
        assert!(RenderLook::from_params(&d, &json!({"background": 1})).is_err());
        assert!(RenderLook::from_params(&d, &json!({"plot_style": {}})).is_err());
    }

    /// An unknown plot style lists the built-in tables and only some of the drawing's own.
    #[test]
    fn unknown_plot_style_lists_a_few_tables() {
        let mut d = Drawing::new_imperial();
        for i in 0..500 {
            d.plot_style_tables.push(PlotStyleTable::color_dependent(&format!("{}{i}.ctb", "own".repeat(30)), "", |_| None));
        }
        let e = RenderLook::from_params(&d, &json!({"plot_style": "nope.ctb"})).unwrap_err();
        for b in cadcraft_doc::BUILTIN_PLOT_STYLE_TABLES {
            assert!(e.contains(b), "{b}: {e}");
        }
        assert!(e.contains("and 490 more"), "{e}");
        assert!(e.len() < 1000, "{} bytes: {e}", e.len());
        assert!(RenderLook::from_params(&d, &json!({"plot_style": d.plot_style_tables[7].name.clone()})).is_ok());
    }
}
