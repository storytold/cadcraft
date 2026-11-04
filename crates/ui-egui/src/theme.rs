//! Design tokens. Colours were measured from the reference look (dark slate chrome, near-black
//! navy model space) and are CADCraft's own values; every widget reads them from here.
//!
//! Two palettes: [`Tokens::DARK`] (the original look) and [`Tokens::LIGHT`]. The interface theme
//! changes the chrome (title and tool bar, menus, palettes, tabs, command line, status bar,
//! dialogs); the drawing area keeps its model-space colours in both, as drawing colours are not
//! part of the interface theme. [`Tokens::get`] returns the palette of the theme in use, which
//! [`CadApp::logic`](crate::CadApp::logic) resolves once per frame from the user's [`ThemePref`].

use std::cell::Cell;

use egui::{Color32, FontFamily, FontId, Theme, Visuals};
use serde::{Deserialize, Serialize};

/// The interface theme the user chose.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePref {
    /// Follow the operating system's light/dark appearance, live.
    System,
    Light,
    /// The default: CADCraft's original look.
    #[default]
    Dark,
}

/// The theme `System` uses when the OS (or browser) reports no appearance.
pub const SYSTEM_FALLBACK: Theme = Theme::Dark;

impl ThemePref {
    pub const ALL: [ThemePref; 3] = [ThemePref::System, ThemePref::Light, ThemePref::Dark];

    pub fn as_str(self) -> &'static str {
        match self {
            ThemePref::System => "system",
            ThemePref::Light => "light",
            ThemePref::Dark => "dark",
        }
    }

    pub fn parse(s: &str) -> Option<ThemePref> {
        ThemePref::ALL.into_iter().find(|t| t.as_str().eq_ignore_ascii_case(s.trim()))
    }

    /// The theme to show, given what the OS reports (`ctx.system_theme()`).
    pub fn resolve(self, system: Option<Theme>) -> Theme {
        match self {
            ThemePref::System => system.unwrap_or(SYSTEM_FALLBACK),
            ThemePref::Light => Theme::Light,
            ThemePref::Dark => Theme::Dark,
        }
    }
}

thread_local! {
    /// The theme in use on the UI thread (set by [`set_active`] each frame).
    static ACTIVE: Cell<Theme> = const { Cell::new(Theme::Dark) };
}

/// The theme [`Tokens::get`] returns colours for.
pub fn active() -> Theme {
    ACTIVE.with(Cell::get)
}

pub fn set_active(theme: Theme) {
    ACTIVE.with(|a| a.set(theme));
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub chrome: Color32,
    pub chrome_dark: Color32,
    pub panel: Color32,
    pub tab_active: Color32,
    pub control: Color32,
    pub control_hover: Color32,
    pub border: Color32,
    pub canvas: Color32,
    pub grid_minor: Color32,
    pub grid_major: Color32,
    pub axis_x: Color32,
    pub axis_y: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub icon: Color32,
    pub icon_accent: Color32,
    pub icon_point: Color32,
    pub accent: Color32,
    pub selection: Color32,
    pub hover: Color32,
    pub grip: Color32,
    pub grip_hot: Color32,
    /// Grips of dynamic block references (AutoCAD's GRIPDYNCOLOR).
    pub grip_dyn: Color32,
    pub snap: Color32,
    /// COMPARE: objects only in this drawing, only in the other one, and the areas of change.
    pub compare_this: Color32,
    pub compare_other: Color32,
    pub compare_area: Color32,
    pub window_stroke: Color32,
    pub crossing_stroke: Color32,
    pub cmd_bg: Color32,
    pub toggle_on: Color32,
    /// Text fields and other inset (extreme) backgrounds.
    pub field: Color32,
    /// Selected text and selected list rows.
    pub select_bg: Color32,
    /// Lines and labels drawn straight onto the drawing area (UCS icon, viewport label, rubber
    /// bands): they follow the model-space background, not the interface theme.
    pub canvas_ink: Color32,
    /// The command-line history lines behind the input bar.
    pub cmd_history: Color32,
    pub cmd_border: Color32,
    /// Command-line keywords (`[Undo/Close]`) and their hover background.
    pub cmd_keyword: Color32,
    pub cmd_keyword_hover: Color32,
    /// The command-line AutoComplete list: background and the highlighted first row.
    pub list_bg: Color32,
    pub list_row: Color32,
    /// Transient warnings (status bar messages).
    pub warn: Color32,
    /// The drawing-area crosshair cursor and its pick box.
    pub crosshair: Color32,
    /// The sheet of paper on a layout tab.
    pub paper: Color32,
}

impl Tokens {
    pub const DARK: Tokens = Tokens {
        chrome: Color32::from_rgb(0x3b, 0x44, 0x53),
        chrome_dark: Color32::from_rgb(0x31, 0x38, 0x44),
        panel: Color32::from_rgb(0x3c, 0x44, 0x52),
        tab_active: Color32::from_rgb(0x4e, 0x5a, 0x6e),
        control: Color32::from_rgb(0x50, 0x5a, 0x6d),
        control_hover: Color32::from_rgb(0x5c, 0x67, 0x7c),
        border: Color32::from_rgb(0x23, 0x29, 0x30),
        canvas: Color32::from_rgb(0x22, 0x28, 0x2f),
        grid_minor: Color32::from_rgb(0x29, 0x30, 0x3a),
        grid_major: Color32::from_rgb(0x31, 0x39, 0x45),
        axis_x: Color32::from_rgb(0x56, 0x28, 0x29),
        axis_y: Color32::from_rgb(0x28, 0x4a, 0x2e),
        text: Color32::from_rgb(0xd7, 0xdb, 0xe0),
        text_dim: Color32::from_rgb(0xa3, 0xaa, 0xb4),
        text_faint: Color32::from_rgb(0x87, 0x8d, 0x96),
        icon: Color32::from_rgb(0xcf, 0xd4, 0xdb),
        icon_accent: Color32::from_rgb(0x4f, 0xa3, 0xf7),
        icon_point: Color32::from_rgb(0xf0, 0x7a, 0x3a),
        accent: Color32::from_rgb(0x3d, 0x8b, 0xfd),
        selection: Color32::from_rgb(0x4a, 0x8f, 0xff),
        hover: Color32::from_rgb(0x9c, 0xc6, 0xff),
        grip: Color32::from_rgb(0x2f, 0x6b, 0xff),
        grip_hot: Color32::from_rgb(0xff, 0x3b, 0x3b),
        grip_dyn: Color32::from_rgb(0x26, 0xc6, 0xe8),
        snap: Color32::from_rgb(0x2f, 0xd0, 0x62),
        compare_this: Color32::from_rgb(0x4c, 0xd9, 0x7b),
        compare_other: Color32::from_rgb(0xff, 0x5c, 0x5c),
        compare_area: Color32::from_rgb(0xff, 0xb4, 0x3c),
        window_stroke: Color32::from_rgb(0x6f, 0xa8, 0xff),
        crossing_stroke: Color32::from_rgb(0x7f, 0xe0, 0x7f),
        cmd_bg: Color32::from_rgba_premultiplied(0x34, 0x3c, 0x4a, 0xf0),
        toggle_on: Color32::from_rgb(0x3d, 0x8b, 0xfd),
        field: Color32::from_rgb(0x31, 0x38, 0x44),
        select_bg: Color32::from_rgb(0x3d, 0x8b, 0xfd),
        canvas_ink: Color32::from_rgb(0xa3, 0xaa, 0xb4),
        cmd_history: Color32::from_rgba_unmultiplied_const(0x2a, 0x30, 0x3a, 170),
        cmd_border: Color32::from_rgb(0x55, 0x5f, 0x70),
        cmd_keyword: Color32::from_rgb(0x8f, 0xc1, 0xff),
        cmd_keyword_hover: Color32::from_rgb(0x2f, 0x5e, 0xa8),
        list_bg: Color32::from_rgb(0x2b, 0x31, 0x3b),
        list_row: Color32::from_rgb(0x3a, 0x42, 0x50),
        warn: Color32::from_rgb(0xff, 0xd0, 0x80),
        crosshair: Color32::from_rgb(0xe8, 0xe8, 0xe8),
        paper: Color32::WHITE,
    };

    /// Light grey chrome with dark text. The drawing-area colours (canvas, grid, axes, selection,
    /// grips, snap markers, selection windows) are the same as [`Tokens::DARK`]'s.
    pub const LIGHT: Tokens = Tokens {
        chrome: Color32::from_rgb(0xe4, 0xe7, 0xeb),
        chrome_dark: Color32::from_rgb(0xd5, 0xd9, 0xdf),
        panel: Color32::from_rgb(0xee, 0xf0, 0xf3),
        tab_active: Color32::from_rgb(0xc8, 0xd3, 0xe2),
        control: Color32::from_rgb(0xdc, 0xe0, 0xe6),
        control_hover: Color32::from_rgb(0xcb, 0xd2, 0xdc),
        border: Color32::from_rgb(0xb3, 0xba, 0xc4),
        text: Color32::from_rgb(0x1d, 0x23, 0x2b),
        text_dim: Color32::from_rgb(0x47, 0x50, 0x5c),
        text_faint: Color32::from_rgb(0x62, 0x6a, 0x76),
        icon: Color32::from_rgb(0x34, 0x3c, 0x48),
        icon_accent: Color32::from_rgb(0x1b, 0x6f, 0xd6),
        icon_point: Color32::from_rgb(0xd3, 0x5a, 0x1c),
        accent: Color32::from_rgb(0x2f, 0x7c, 0xf6),
        cmd_bg: Color32::from_rgba_unmultiplied_const(0xf9, 0xfa, 0xfb, 0xf0),
        toggle_on: Color32::from_rgb(0x2f, 0x7c, 0xf6),
        field: Color32::from_rgb(0xff, 0xff, 0xff),
        select_bg: Color32::from_rgb(0xb6, 0xd3, 0xfb),
        cmd_history: Color32::from_rgba_unmultiplied_const(0xf0, 0xf2, 0xf5, 0xdc),
        cmd_border: Color32::from_rgb(0xa9, 0xb0, 0xba),
        cmd_keyword: Color32::from_rgb(0x15, 0x59, 0xc0),
        cmd_keyword_hover: Color32::from_rgb(0xcf, 0xe0, 0xfa),
        list_bg: Color32::from_rgb(0xfb, 0xfc, 0xfd),
        list_row: Color32::from_rgb(0xe3, 0xe8, 0xef),
        warn: Color32::from_rgb(0x8a, 0x53, 0x00),
        ..Tokens::DARK
    };

    /// The palette of `theme`.
    pub fn of(theme: Theme) -> Tokens {
        match theme {
            Theme::Dark => Tokens::DARK,
            Theme::Light => Tokens::LIGHT,
        }
    }

    /// The palette of the theme in use, with the user's drawing-area colours ([`set_canvas`]).
    pub fn get() -> Tokens {
        CANVAS.with(Cell::get).over(Tokens::of(active()))
    }

    /// These tokens with `bg` as the model-space background: the grid, axes, crosshair and the
    /// lines drawn on the drawing area are recomputed to stay visible on it.
    pub fn with_model_background(self, bg: Color32) -> Tokens {
        let light = luminance(bg) > 0.5;
        let ink = if light { Color32::BLACK } else { Color32::WHITE };
        Tokens {
            canvas: bg,
            grid_minor: mix(bg, ink, 0.05),
            grid_major: mix(bg, ink, 0.11),
            axis_x: mix(bg, Color32::from_rgb(0xd0, 0x30, 0x30), 0.3),
            axis_y: mix(bg, Color32::from_rgb(0x30, 0xb0, 0x40), 0.3),
            canvas_ink: if light { Color32::from_rgb(0x47, 0x50, 0x5c) } else { Tokens::DARK.canvas_ink },
            crosshair: crosshair_color(bg),
            ..self
        }
    }
}

/// The thin crosshair and pickbox contrast with the surface under their centre.
pub(crate) fn crosshair_color(background: Color32) -> Color32 {
    if luminance(background) > 0.5 { Color32::from_rgb(0x20, 0x20, 0x20) } else { Tokens::DARK.crosshair }
}

/// The user's drawing-area preferences (Options ▸ Display and Drafting); `None` keeps the theme's
/// colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Canvas {
    pub model: Option<Color32>,
    pub paper: Option<Color32>,
    /// The side of an object snap marker, in points.
    pub snap_marker: f32,
    /// The colour selected objects are highlighted in.
    pub selection: Option<Color32>,
}

/// The default object snap marker size.
pub const SNAP_MARKER: f32 = 11.0;

impl Default for Canvas {
    fn default() -> Self {
        Canvas { model: None, paper: None, snap_marker: SNAP_MARKER, selection: None }
    }
}

impl Canvas {
    fn over(self, t: Tokens) -> Tokens {
        let t = match self.model {
            Some(bg) => t.with_model_background(bg),
            None => t,
        };
        Tokens { paper: self.paper.unwrap_or(t.paper), selection: self.selection.unwrap_or(t.selection), ..t }
    }
}

thread_local! {
    /// The drawing-area preferences in use on the UI thread (set by [`set_canvas`] each frame).
    static CANVAS: Cell<Canvas> = Cell::new(Canvas::default());
}

pub fn set_canvas(c: Canvas) {
    CANVAS.with(|v| v.set(c));
}

/// Half the side of an object snap marker, in points.
pub fn snap_marker_half() -> f32 {
    CANVAS.with(Cell::get).snap_marker / 2.0
}

/// Relative luminance of `c` (0 black … 1 white).
pub fn luminance(c: Color32) -> f32 {
    (0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b())) / 255.0
}

/// `a` moved the fraction `f` towards `b`.
fn mix(a: Color32, b: Color32, f: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * f).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

pub fn small() -> FontId {
    FontId::new(11.5, FontFamily::Proportional)
}
pub fn body() -> FontId {
    FontId::new(12.5, FontFamily::Proportional)
}
pub fn mono() -> FontId {
    FontId::new(12.0, FontFamily::Monospace)
}

/// Load a system UI font at run time when one is installed (not bundled), else keep egui's
/// bundled bounded Latin/icon faces (`default_fonts` + `monochrome_emoji_fonts`, enabled in the
/// workspace manifest for native and web); then CJK fonts after the Latin ones, so Latin text
/// keeps its look, in every interface
/// language: file, layout and layer names can be Japanese or Chinese whatever the interface
/// shows (#513). In a Chinese interface (`language` "zh…") the Chinese font comes first, else a
/// Japanese one, so names take their own glyph forms; in an Arabic interface an installed
/// Arabic font follows. A font embedded from craft-fonts comes
/// last, for systems without any (and the web build, which has no system fonts). A Korean
/// interface adds a Hangul font only when the CJK font found doesn't cover Hangul
/// ([`korean_font`]).
pub fn install_fonts(ctx: &egui::Context, language: &str) {
    crate::mtext_editor::install_diameter_glyph(ctx);
    let mut fonts = egui::FontDefinitions::default();
    let mut added: Vec<(String, std::sync::Arc<egui::FontData>)> = Vec::new();
    #[cfg(not(target_arch = "wasm32"))]
    let chinese = language.starts_with("zh");
    #[cfg(not(target_arch = "wasm32"))]
    {
        for path in system_ui_candidates() {
            // A Vietnamese interface takes the first one with its letters.
            if language == "vi" && vietnamese_font() != Some(std::path::Path::new(path)) {
                continue;
            }
            if let Ok(bytes) = std::fs::read(path) {
                fonts.font_data.insert("system-ui".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
                if let Some(f) = fonts.families.get_mut(&FontFamily::Proportional) {
                    f.insert(0, "system-ui".into());
                }
                // Keep Hack first in Monospace, then use the system face for glyphs Hack lacks.
                if let Some(f) = fonts.families.get_mut(&FontFamily::Monospace) {
                    let at = f.iter().position(|name| name == "Hack").map_or(0, |i| i + 1);
                    f.insert(at, "system-ui".into());
                }
                break;
            }
        }
        let mut paths: Vec<&std::path::Path> = Vec::new();
        if !chinese {
            paths.extend(japanese_font());
        }
        paths.extend(cjk_font());
        paths.dedup();
        added.extend(
            paths
                .into_iter()
                .filter_map(|p| read_capped(p, MAX_UI_FONT_BYTES))
                .enumerate()
                .map(|(i, bytes)| (format!("system-cjk-{i}"), std::sync::Arc::new(egui::FontData::from_owned(bytes)))),
        );
        // العربية: an installed Arabic face after the Latin ones (egui's bundled faces carry no
        // Arabic glyphs), for the Arabic interface.
        if language == "ar"
            && let Some(bytes) = arabic_font().and_then(|p| read_capped(p, MAX_UI_FONT_BYTES))
        {
            added.push(("system-arabic".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes))));
        }
    }
    let embedded: Vec<_> = cadcraft_fonts::CRAFT_FONTS.iter().collect();
    added.extend(
        embedded_ui_faces(&embedded, language)
            .into_iter()
            .map(|f| (format!("craft-{}", f.stem()), std::sync::Arc::new(egui::FontData::from_static(f.bytes)))),
    );
    // Korean: Noto Sans CJK and most CJK fonts above already cover Hangul; else a Hangul font.
    if language == "ko"
        && !cjk_covers_hangul()
        && let Some(face) = korean_font()
    {
        let mut data = egui::FontData::from_owned(face.bytes.as_ref().clone());
        data.index = face.index;
        added.push(("korean-ui".into(), std::sync::Arc::new(data)));
    }
    for (name, data) in added {
        fonts.font_data.insert(name.clone(), data);
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            if let Some(f) = fonts.families.get_mut(&family) {
                f.push(name.clone());
            }
        }
    }
    // `default_fonts` supplies Ubuntu-Light (Proportional) and Hack (Monospace). Hack also
    // carries the modifier symbols (⇧) that proportional shortcut labels use; keep it as a
    // bounded late fallback when Ubuntu-Light lacks one. The monochrome emoji face (enabled by
    // `monochrome_emoji_fonts`) supplies ⌘ in both families.
    if fonts.font_data.contains_key("Hack")
        && let Some(f) = fonts.families.get_mut(&FontFamily::Proportional)
        && !f.iter().any(|name| name == "Hack")
    {
        f.push("Hack".into());
    }
    ctx.set_fonts(fonts);
}

/// The system interface fonts [`install_fonts`] looks for, in order; it loads the first installed
/// one ahead of egui's bundled faces.
#[cfg(not(target_arch = "wasm32"))]
fn system_ui_candidates() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &["/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/Helvetica.ttc", "/Library/Fonts/Arial.ttf"]
    } else if cfg!(windows) {
        &["C:\\Windows\\Fonts\\segoeui.ttf", "C:\\Windows\\Fonts\\arial.ttf"]
    } else {
        &[
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        ]
    }
}

/// The first of `candidates` that is `installed` and, for the Vietnamese interface, has the
/// Vietnamese letters: the system interface font [`install_fonts`] loads ahead of egui's faces.
#[cfg(not(target_arch = "wasm32"))]
fn ui_font_choice<'a>(
    candidates: &[&'a str],
    vietnamese: bool,
    installed: impl Fn(&str) -> bool,
    has_vietnamese: impl Fn(&str) -> bool,
) -> Option<&'a str> {
    candidates.iter().copied().find(|p| installed(p) && (!vietnamese || has_vietnamese(p)))
}

/// Whether `face` has every letter Vietnamese adds to Latin: the tone-marked vowels of Latin
/// Extended Additional (U+1EA0–U+1EF9), and ơ, ư, đ.
#[cfg(not(target_arch = "wasm32"))]
fn covers_vietnamese(face: &cadcraft_fonts::ttf::Face) -> bool {
    ('\u{1ea0}'..='\u{1ef9}').chain(['ơ', 'Ơ', 'ư', 'Ư', 'đ', 'Đ']).all(|c| face.has_glyph(c))
}

/// The system interface font the Vietnamese interface takes: the first of
/// [`system_ui_candidates`] whose first face (the one egui uses) has every Vietnamese letter.
/// Segoe UI, Arial, SF, DejaVu Sans and Noto Sans do; egui's bundled faces stop at Latin
/// Extended-A, so without one the interface stays English (as the web build does). Checked once.
pub fn vietnamese_font() -> Option<&'static std::path::Path> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static FOUND: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
        FOUND
            .get_or_init(|| {
                let has_vietnamese = |p: &str| {
                    read_capped(std::path::Path::new(p), MAX_UI_FONT_BYTES)
                        .and_then(|bytes| cadcraft_fonts::ttf::faces(&std::sync::Arc::new(bytes)).into_iter().next())
                        .is_some_and(|face| covers_vietnamese(&face))
                };
                ui_font_choice(system_ui_candidates(), true, |p| std::path::Path::new(p).is_file(), has_vietnamese).map(std::path::PathBuf::from)
            })
            .as_deref()
    }
    #[cfg(target_arch = "wasm32")]
    None
}

/// Whether the interface in `language` shows the Vietnamese letters (Tiếng Việt in the language
/// picker): the Vietnamese interface with [`vietnamese_font`], any other when the system interface
/// font it loads is that one.
pub fn shows_vietnamese(language: &str) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let loaded = || ui_font_choice(system_ui_candidates(), false, |p| std::path::Path::new(p).is_file(), |_| true);
        vietnamese_font().is_some_and(|v| language == "vi" || loaded().is_some_and(|p| std::path::Path::new(p) == v))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = language;
        false
    }
}

/// The faces `build.rs` embedded from craft-fonts that the interface loads, in order: every CJK
/// face, because file, layout and layer names can be Japanese or Chinese whatever the interface
/// shows (#513), with the interface's own face leading. A sans-serif face reads best, and a
/// Chinese interface takes the face with its own glyphs ahead of a Japanese one — a Japanese face
/// shares most ideographs but lacks the simplified-only ones (图 园 测), which would be tofu.
/// العربية adds its Arabic face after those, egui's bundled faces carrying no Arabic glyphs.
fn embedded_ui_faces<'a>(faces: &[&'a cadcraft_fonts::CraftFont], language: &str) -> Vec<&'a cadcraft_fonts::CraftFont> {
    let cjk = |f: &&cadcraft_fonts::CraftFont| f.style == "Regular" && f.scripts.iter().any(|s| matches!(*s, "Jpan" | "Hans" | "Hant"));
    let mut out: Vec<_> = faces.iter().copied().filter(cjk).collect();
    out.sort_by_key(|f| !f.family.contains("Gothic") && !f.family.contains("Sans"));
    if language.starts_with("zh") {
        out.sort_by_key(|f| !f.scripts.contains(&"Hans"));
    }
    if language == "ar" {
        out.extend(faces.iter().copied().filter(|f| f.style == "Regular" && f.scripts.contains(&"Arab")));
    }
    out
}

/// Largest interface font file read or embedded face copied (CJK collections are 15–60 MB).
const MAX_UI_FONT_BYTES: u64 = 96 * 1024 * 1024;

#[cfg(not(target_arch = "wasm32"))]
fn read_capped(path: &std::path::Path, max: u64) -> Option<Vec<u8>> {
    use std::io::Read as _;
    let file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len == 0 || len > max {
        return None;
    }
    // Enforce the cap during the read too, even if the file grows after metadata was read.
    let mut bytes = Vec::new();
    file.take(max.saturating_add(1)).read_to_end(&mut bytes).ok()?;
    (!bytes.is_empty() && bytes.len() as u64 <= max).then_some(bytes)
}

/// An installed font with Chinese characters, for the Chinese interface; fonts are never bundled
/// (only looked for where the OS or common packages put them). Checked once.
pub fn cjk_font() -> Option<&'static std::path::Path> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static FOUND: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
        FOUND
            .get_or_init(|| {
                let candidates: &[&str] = if cfg!(target_os = "macos") {
                    &["/System/Library/Fonts/PingFang.ttc", "/System/Library/Fonts/STHeiti Light.ttc", "/System/Library/Fonts/Hiragino Sans GB.ttc"]
                } else if cfg!(windows) {
                    &["C:\\Windows\\Fonts\\msyh.ttc", "C:\\Windows\\Fonts\\msyh.ttf", "C:\\Windows\\Fonts\\simsun.ttc"]
                } else {
                    &[
                        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
                        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
                        "/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf",
                        "/usr/local/share/fonts/noto/NotoSansCJK-Regular.ttc",
                        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
                        "/usr/share/fonts/wqy-microhei/wqy-microhei.ttc",
                        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
                        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
                    ]
                };
                candidates.iter().map(std::path::PathBuf::from).find(|p| p.is_file())
            })
            .as_deref()
    }
    #[cfg(target_arch = "wasm32")]
    None
}

/// An installed font with Arabic glyphs, for the العربية interface; fonts are never bundled
/// (only looked for where the OS or common packages put them). Checked once.
pub fn arabic_font() -> Option<&'static std::path::Path> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static FOUND: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
        FOUND
            .get_or_init(|| {
                let candidates: &[&str] = if cfg!(target_os = "macos") {
                    &[
                        "/System/Library/Fonts/GeezaPro.ttc",
                        "/System/Library/Fonts/Supplemental/Geeza Pro.ttf",
                        "/System/Library/Fonts/Supplemental/Al Nile.ttc",
                        "/System/Library/Fonts/Supplemental/Baghdad.ttf",
                    ]
                } else if cfg!(windows) {
                    &[
                        "C:\\Windows\\Fonts\\segoeui.ttf",
                        "C:\\Windows\\Fonts\\tahoma.ttf",
                        "C:\\Windows\\Fonts\\trado.ttf",
                        "C:\\Windows\\Fonts\\arabtype.ttf",
                        "C:\\Windows\\Fonts\\arial.ttf",
                    ]
                } else {
                    &[
                        "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
                        "/usr/share/fonts/noto/NotoSansArabic-Regular.ttf",
                        "/usr/share/fonts/opentype/noto/NotoSansArabic-Regular.ttf",
                        "/usr/share/fonts/truetype/noto/NotoNaskhArabic-Regular.ttf",
                        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                        "/usr/share/fonts/TTF/DejaVuSans.ttf",
                        "/usr/share/fonts/truetype/amiri/Amiri-Regular.ttf",
                        "/usr/share/fonts/truetype/kacst/KacstOne.ttf",
                    ]
                };
                candidates.iter().map(std::path::PathBuf::from).find(|p| p.is_file())
            })
            .as_deref()
    }
    #[cfg(target_arch = "wasm32")]
    None
}

/// An installed Japanese font, for Japanese names in a non-Chinese interface. Checked once.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn japanese_font() -> Option<&'static std::path::Path> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static FOUND: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
        FOUND
            .get_or_init(|| {
                let candidates: &[&str] = if cfg!(target_os = "macos") {
                    &["/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc", "/System/Library/Fonts/Hiragino Sans GB.ttc"]
                } else if cfg!(windows) {
                    &["C:\\Windows\\Fonts\\YuGothR.ttc", "C:\\Windows\\Fonts\\meiryo.ttc", "C:\\Windows\\Fonts\\msgothic.ttc"]
                } else {
                    &[
                        "/usr/share/fonts/opentype/ipafont-gothic/ipagp.ttf",
                        "/usr/share/fonts/truetype/fonts-japanese-gothic.ttf",
                        "/usr/share/fonts/ipa-pgothic/ipagp.ttf",
                        "/usr/share/fonts/vl-gothic/VL-PGothic-Regular.ttf",
                    ]
                };
                candidates.iter().map(std::path::PathBuf::from).find(|p| p.is_file())
            })
            .as_deref()
    }
    #[cfg(target_arch = "wasm32")]
    None
}

/// The Hangul letters the Korean interface shows: its catalog and its own name.
fn hangul_letters() -> &'static std::collections::BTreeSet<char> {
    static LETTERS: std::sync::OnceLock<std::collections::BTreeSet<char>> = std::sync::OnceLock::new();
    LETTERS.get_or_init(|| {
        include_str!("i18n/ko.tsv")
            .lines()
            .filter_map(|line| line.split('\t').nth(2))
            .flat_map(str::chars)
            .chain("한국어".chars())
            .filter(|c| matches!(c, '\u{1100}'..='\u{11ff}' | '\u{3130}'..='\u{318f}' | '\u{a960}'..='\u{a97f}' | '\u{ac00}'..='\u{d7ff}'))
            .collect()
    })
}

/// Whether `face` has every Hangul letter the Korean interface shows (rejects malformed fonts
/// and Chinese- or Japanese-only faces).
fn covers_hangul(face: &cadcraft_fonts::ttf::Face) -> bool {
    hangul_letters().iter().all(|c| face.has_glyph(*c))
}

/// The first face of a font file (the one egui uses) when it covers the Korean interface.
fn korean_face(bytes: std::sync::Arc<Vec<u8>>) -> Option<cadcraft_fonts::ttf::Face> {
    cadcraft_fonts::ttf::faces(&bytes).into_iter().find(covers_hangul)
}

/// Whether the CJK fonts [`install_fonts`] loads in every language already show Hangul (Noto Sans
/// CJK, Microsoft YaHei, PingFang do). Only asked about Korean, so other interfaces never read the
/// font twice. Checked once.
pub fn cjk_covers_hangul() -> bool {
    static COVERS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *COVERS.get_or_init(|| {
        #[cfg(not(target_arch = "wasm32"))]
        {
            // egui uses a collection's first face, and each letter from the first font with it.
            let first_covers = |p: &std::path::Path| {
                read_capped(p, MAX_UI_FONT_BYTES)
                    .and_then(|bytes| cadcraft_fonts::ttf::faces(&std::sync::Arc::new(bytes)).into_iter().next())
                    .is_some_and(|face| covers_hangul(&face))
            };
            if cjk_font().is_some_and(first_covers) {
                return true;
            }
        }
        cadcraft_fonts::CRAFT_FONTS
            .iter()
            .filter(|f| f.style == "Regular" && f.scripts.iter().any(|s| matches!(*s, "Jpan" | "Hans" | "Hant")))
            .any(|f| cadcraft_fonts::ttf::faces(&std::sync::Arc::new(f.bytes.to_vec())).first().is_some_and(covers_hangul))
    })
}

/// A Hangul font for the Korean interface when the CJK fonts don't cover it: a Korean face from
/// the optional craft-fonts input, else one installed where the OS or common packages put it.
/// Nothing is bundled or downloaded; without one the interface stays English. Only looked for
/// when Korean is asked about. Checked once; collections are scanned with the fonts crate's
/// face-count cap and reads are capped.
pub fn korean_font() -> Option<&'static cadcraft_fonts::ttf::Face> {
    static FOUND: std::sync::OnceLock<Option<cadcraft_fonts::ttf::Face>> = std::sync::OnceLock::new();
    FOUND
        .get_or_init(|| {
            for font in cadcraft_fonts::CRAFT_FONTS.iter().filter(|f| f.scripts.iter().any(|s| matches!(*s, "Hang" | "Kore"))) {
                if font.bytes.len() as u64 > MAX_UI_FONT_BYTES {
                    continue;
                }
                if let Some(face) = korean_face(std::sync::Arc::new(font.bytes.to_vec())) {
                    return Some(face);
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                let candidates: &[&str] = if cfg!(target_os = "macos") {
                    &[
                        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
                        "/System/Library/Fonts/Supplemental/AppleGothic.ttf",
                        "/Library/Fonts/NanumGothic.ttf",
                    ]
                } else if cfg!(windows) {
                    &["C:\\Windows\\Fonts\\malgun.ttf", "C:\\Windows\\Fonts\\gulim.ttc"]
                } else {
                    &[
                        "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
                        "/usr/share/fonts/nanum/NanumGothic.ttf",
                        "/usr/share/fonts/opentype/noto/NotoSansCJKkr-Regular.otf",
                        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
                        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
                        "/usr/local/share/fonts/nanum/NanumGothic.ttf",
                        "/usr/local/share/fonts/noto/NotoSansCJK-Regular.ttc",
                    ]
                };
                for path in candidates {
                    if let Some(face) =
                        read_capped(std::path::Path::new(path), MAX_UI_FONT_BYTES).and_then(|bytes| korean_face(std::sync::Arc::new(bytes)))
                    {
                        return Some(face);
                    }
                }
            }
            None
        })
        .as_ref()
}

/// Whether the interface in `language` shows Hangul (the Korean name in the language picker):
/// in every language when the CJK fonts cover it, in a Korean interface with [`korean_font`].
pub fn shows_hangul(language: &str) -> bool {
    cjk_covers_hangul() || (language == "ko" && korean_font().is_some())
}

/// Whether the interface can show `language` without missing-glyph boxes: Chinese needs a font
/// with its glyphs (installed, or the CJK face this build embedded from craft-fonts), Korean a
/// font with Hangul, العربية an installed Arabic font or the embedded Arabic face, Tiếng Việt a
/// system interface font with its tone-marked letters ([`vietnamese_font`]).
pub fn can_show(language: &str) -> bool {
    match language {
        "ko" => shows_hangul("ko"),
        "vi" => vietnamese_font().is_some(),
        "ar" => arabic_font().is_some() || embedded_script("Arab"),
        l if l.starts_with("zh") => cjk_font().is_some() || embedded_script("Hans") || embedded_script("Hant"),
        _ => true,
    }
}

/// Whether a face this build embedded from craft-fonts carries `script` (ISO 15924). The web
/// build has no system fonts, so there the interface rides on the embedded faces alone; the
/// Korean gate already asks this way ([`korean_font`]) and Arabic and Chinese match it. Only a
/// build made with `CRAFT_FONTS_DIR` embeds any (`build.rs`), so the rest is unchanged.
fn embedded_script(script: &str) -> bool {
    any_script(cadcraft_fonts::CRAFT_FONTS, script)
}

/// Whether any face of `faces` carries `script` (ISO 15924), the question [`embedded_script`]
/// asks of the table this build embedded.
fn any_script(faces: &[cadcraft_fonts::CraftFont], script: &str) -> bool {
    faces.iter().any(|f| f.style == "Regular" && f.scripts.contains(&script))
}

/// egui's visuals for `theme`, from its tokens.
fn visuals(theme: Theme) -> Visuals {
    let t = Tokens::of(theme);
    let mut v = match theme {
        Theme::Dark => Visuals::dark(),
        Theme::Light => Visuals::light(),
    };
    v.panel_fill = t.panel;
    v.window_fill = t.chrome;
    v.extreme_bg_color = t.field;
    v.faint_bg_color = t.chrome_dark;
    v.override_text_color = Some(t.text);
    v.widgets.noninteractive.bg_fill = t.panel;
    v.widgets.noninteractive.fg_stroke.color = t.text;
    v.widgets.noninteractive.bg_stroke.color = t.border;
    v.widgets.inactive.bg_fill = t.control;
    v.widgets.inactive.weak_bg_fill = t.control;
    v.widgets.inactive.fg_stroke.color = t.text;
    v.widgets.hovered.bg_fill = t.control_hover;
    v.widgets.hovered.weak_bg_fill = t.control_hover;
    v.widgets.active.bg_fill = t.accent;
    v.widgets.active.weak_bg_fill = t.accent;
    v.selection.bg_fill = t.select_bg;
    v.window_stroke = egui::Stroke::new(1.0, t.border);
    let shadow = Color32::from_black_alpha(if theme == Theme::Dark { 90 } else { 40 });
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 12, spread: 0, color: shadow };
    v.window_corner_radius = egui::CornerRadius::same(6);
    v.menu_corner_radius = egui::CornerRadius::same(5);
    v
}

/// Install the styles of both themes and show `theme` (egui's own widgets, popups and menus
/// follow it; our own painting follows [`Tokens::get`]). `follow_system` is true for the
/// [`ThemePref::System`] choice, whose `theme` was resolved from the OS report.
pub fn apply(ctx: &egui::Context, theme: Theme, follow_system: bool) {
    set_active(theme);
    ctx.set_visuals_of(Theme::Dark, visuals(Theme::Dark));
    ctx.set_visuals_of(Theme::Light, visuals(Theme::Light));
    // A System choice keeps egui's own System preference: a concrete Light/Dark would pin the native
    // window appearance (winit then stops reporting OS changes) and the app would never follow again.
    ctx.set_theme(if follow_system { egui::ThemePreference::System } else { egui::ThemePreference::from(theme) });
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(6.0, 3.0);
        s.spacing.interact_size.y = 20.0;
        s.text_styles.insert(egui::TextStyle::Body, body());
        s.text_styles.insert(egui::TextStyle::Button, body());
        s.text_styles.insert(egui::TextStyle::Small, small());
        s.text_styles.insert(egui::TextStyle::Monospace, mono());
    });
}

#[cfg(test)]
mod tests {
    /// The system interface font loaded: the first installed one, and for Vietnamese the first
    /// installed one with its letters (a later one when the first lacks them).
    #[test]
    fn the_vietnamese_interface_takes_the_first_installed_font_with_its_letters() {
        let candidates = ["a.ttf", "b.ttf", "c.ttf", "d.ttf"];
        let installed = |p: &str| p != "a.ttf";
        let has_vietnamese = |p: &str| p == "c.ttf" || p == "d.ttf";
        assert_eq!(super::ui_font_choice(&candidates, false, installed, has_vietnamese), Some("b.ttf"));
        assert_eq!(super::ui_font_choice(&candidates, true, installed, has_vietnamese), Some("c.ttf"));
        assert_eq!(super::ui_font_choice(&candidates, true, installed, |_| false), None);
    }

    use cadcraft_fonts::CraftFont;

    /// A face shaped like `build.rs` writes them, with no font file (as in `cadcraft-fonts`).
    fn face(family: &'static str, scripts: &'static [&'static str]) -> CraftFont {
        CraftFont { family, style: "Regular", scripts, bytes: &[] }
    }

    fn families<'a>(faces: &[&'a CraftFont]) -> Vec<&'a str> {
        faces.iter().map(|f| f.family).collect()
    }

    #[test]
    fn korean_font_rejects_invalid_or_glyphless_files() {
        for bytes in [vec![], vec![0xff; 32], vec![0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]] {
            assert!(super::korean_face(std::sync::Arc::new(bytes)).is_none());
        }
    }

    /// The interface's own script leads the embedded faces it loads, and العربية adds its face.
    #[test]
    fn embedded_faces_lead_with_the_interfaces_own_script() {
        let japanese = face("BIZ UDPGothic", &["Jpan", "Latn"]);
        let chinese = face("Han Test Face", &["Hans", "Latn"]);
        let arabic = face("Noto Sans Arabic", &["Arab"]);
        let faces = [&japanese, &chinese, &arabic];
        // Every interface reads CJK names, so both CJK faces load, the sans-serif face first.
        assert_eq!(families(&super::embedded_ui_faces(&faces, "en")), ["BIZ UDPGothic", "Han Test Face"]);
        // A Chinese interface takes the Han face first: a Japanese face lacks 图 园 测.
        assert_eq!(families(&super::embedded_ui_faces(&faces, "zh")), ["Han Test Face", "BIZ UDPGothic"]);
        // العربية adds the Arabic face, after the CJK ones egui's bundled faces have no glyphs for.
        assert_eq!(families(&super::embedded_ui_faces(&faces, "ar")), ["BIZ UDPGothic", "Han Test Face", "Noto Sans Arabic"]);
        // Another weight is a style of a face, not an interface face of its own.
        let bold = CraftFont { family: "BIZ UDPGothic", style: "Bold", scripts: &["Jpan"], bytes: &[] };
        assert!(super::embedded_ui_faces(&[&bold], "en").is_empty());
    }

    /// What the gate asks of the table: an embedded face of the script stands in for an installed
    /// font, which the web build never has. The default (no `CRAFT_FONTS_DIR`) table answers no,
    /// leaving the installed-font answer the gate had before.
    #[test]
    fn embedded_faces_answer_for_their_scripts() {
        let faces = [face("BIZ UDPGothic", &["Jpan", "Latn"]), face("Noto Sans Arabic", &["Arab"])];
        assert!(super::any_script(&faces, "Jpan") && super::any_script(&faces, "Arab"));
        assert!(!super::any_script(&faces, "Hans") && !super::any_script(&faces, "Hant"));
        // A `Bold`-only face is not the interface's, and a script code is matched whole.
        let bold = CraftFont { family: "Some Han Bold", style: "Bold", scripts: &["Hans"], bytes: &[] };
        assert!(!super::any_script(&[bold], "Hans") && !super::any_script(&faces, "Ara"));
        assert!(!super::embedded_script("Hans") || super::can_show("zh"));
        assert!(super::can_show("en") && super::can_show("uk"));
    }
}
