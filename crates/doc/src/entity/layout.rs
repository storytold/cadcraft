use super::common::Handle;
use super::{Color, Deserialize, Serialize, Vec2, Vec3, standard, yes};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewport {
    pub center: Vec3,
    pub width: f64,
    pub height: f64,
    pub view_center: Vec2,
    pub view_height: f64,
    #[serde(default)]
    pub id: u32,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub frozen_layers: Vec<String>,
    /// Per-viewport layer colour overrides (VP Color in the Layer Properties Manager).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layer_colors: Vec<(String, Color)>,
    /// The paper-space entity whose outline clips the viewport (a closed polyline, circle,
    /// ellipse or spline; DXF group 340). `None`: the rectangle `center`/`width`/`height`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<Handle>,
    /// Display on (MVIEW ON/OFF). An off viewport shows its border but no model space.
    #[serde(default = "yes")]
    pub on: bool,
}

impl Default for Viewport {
    fn default() -> Self {
        Viewport {
            center: Vec3::ZERO,
            width: 1.0,
            height: 1.0,
            view_center: Vec2::ZERO,
            view_height: 1.0,
            id: 0,
            locked: false,
            frozen_layers: Vec::new(),
            layer_colors: Vec::new(),
            clip: None,
            on: true,
        }
    }
}

/// A PDF underlay (PDFUNDERLAY): one page of a PDF file shown under the drawing. Its lines are
/// not objects of the drawing; the file is read again each time the drawing is shown.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfUnderlay {
    /// The lower-left corner of the page.
    pub insert: Vec3,
    /// The page's width and height at scale 1: its size in inches, as drawing units whatever
    /// the drawing's units (the convention DXF and DWG files store underlays in; a millimetre
    /// drawing shows the page at its size on paper at scale 25.4).
    pub size: Vec2,
    /// Scale factor applied to `size` (the DXF/DWG underlay scale).
    #[serde(default = "underlay_scale")]
    pub scale: f64,
    /// Rotation about `insert`, in radians.
    #[serde(default)]
    pub rotation: f64,
    /// The PDF file.
    pub path: String,
    /// The page number, from 1.
    #[serde(default = "underlay_page")]
    pub page: u32,
    /// How much the page fades into the background, 0..=80 (percent).
    #[serde(default = "underlay_fade")]
    pub fade: u8,
    /// On a dark background the page is shown with its lightness turned over (white paper
    /// becomes dark, dark lines light); its hues are kept.
    #[serde(default = "underlay_adjust")]
    pub adjust_colors: bool,
    /// Contrast, 0..=100: below 100 the page's colours are drawn toward middle grey.
    #[serde(default = "underlay_contrast")]
    pub contrast: u8,
    /// The page is shown in shades of grey.
    #[serde(default)]
    pub monochrome: bool,
    /// Clip boundary (PDFCLIP): a polygon's vertices in the page's own units (those of `size`,
    /// from `insert`, before scale and rotation), so it follows the underlay when it is moved,
    /// turned or scaled. Empty: the whole page.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clip: Vec<Vec2>,
    /// The clip boundary is applied (PDFCLIP ON/OFF keeps it either way): the Properties
    /// palette's "Show clip", on for a new underlay (group 280 bit 1) even before
    /// it has a boundary.
    #[serde(default)]
    pub clipping: bool,
    /// Inverted clip: the page is shown outside the boundary, hidden inside it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clip_inverted: bool,
    /// The PDF's layers this underlay shows or hides, by name; layers not listed are shown as
    /// the PDF shows them by default.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<UnderlayLayer>,
    /// Unloaded (External References palette, or PDFIMPORT): not shown, not plotted, not in the
    /// drawing's extents. It is the PDF file's state: every underlay of the same file and page
    /// shares it, as files keep it on the file's definition.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unloaded: bool,
    /// The page not shown (the Properties palette's "Show underlay" No): the page is neither
    /// drawn, plotted nor snapped to; its frame stays as PDFFRAME says, as for a shown one, so
    /// the underlay can still be picked.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

/// A PDF layer an underlay shows (`on`) or hides.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnderlayLayer {
    pub name: String,
    pub on: bool,
}

impl PdfUnderlay {
    /// Apply the underlay's monochrome and contrast to straight RGBA pixels of its page
    /// (transparency kept), before any lightness turned over for the background or fade.
    pub fn tone(&self, rgba: &mut [u8]) {
        let k = i32::from(self.contrast.min(100));
        if !self.monochrome && k == 100 {
            return;
        }
        for [r, g, b, _] in rgba.as_chunks_mut::<4>().0 {
            if self.monochrome {
                // Rec. 601 luma.
                let y = ((u32::from(*r) * 299 + u32::from(*g) * 587 + u32::from(*b) * 114 + 500) / 1000).min(255) as u8;
                (*r, *g, *b) = (y, y, y);
            }
            for c in [r, g, b] {
                *c = (128 + (i32::from(*c) - 128) * k / 100).clamp(0, 255) as u8;
            }
        }
    }
}
/// The largest fade an underlay takes.
pub const UNDERLAY_MAX_FADE: u8 = 80;
fn underlay_fade() -> u8 {
    0
}
fn underlay_adjust() -> bool {
    true
}
fn underlay_contrast() -> u8 {
    100
}
impl Default for PdfUnderlay {
    fn default() -> Self {
        PdfUnderlay {
            insert: Vec3::ZERO,
            size: Vec2::new(1.0, 1.0),
            scale: underlay_scale(),
            rotation: 0.0,
            path: String::new(),
            page: underlay_page(),
            fade: underlay_fade(),
            adjust_colors: underlay_adjust(),
            contrast: underlay_contrast(),
            monochrome: false,
            clip: Vec::new(),
            clipping: true,
            clip_inverted: false,
            layers: Vec::new(),
            unloaded: false,
            hidden: false,
        }
    }
}
fn underlay_scale() -> f64 {
    1.0
}
fn underlay_page() -> u32 {
    1
}
impl PdfUnderlay {
    /// The page's width (along its bottom edge) and height vectors in drawing units.
    pub fn axes(&self) -> (Vec2, Vec2) {
        let (s, c) = self.rotation.sin_cos();
        let (w, h) = (self.size.x * self.scale, self.size.y * self.scale);
        (Vec2::new(c * w, s * w), Vec2::new(-s * h, c * h))
    }
    /// The four corners, counter-clockwise from the lower-left one.
    pub fn corners(&self) -> [Vec2; 4] {
        let o = self.insert.xy();
        let (u, v) = self.axes();
        [o, o + u, o + u + v, o + v]
    }
    /// The PDF layers set on this underlay, as (name, shown).
    pub fn layer_visibility(&self) -> Vec<(String, bool)> {
        self.layers.iter().map(|l| (l.name.clone(), l.on)).collect()
    }
    /// A key telling apart the looks of the same page with different layers hidden (empty when
    /// none is set).
    pub fn layers_key(&self) -> String {
        self.layers.iter().map(|l| format!("{}{}\u{1f}", if l.on { '+' } else { '-' }, l.name)).collect()
    }
    /// Drawing point of page point `p` (in the page's own units, see [`PdfUnderlay::clip`]).
    pub fn to_drawing(&self, p: Vec2) -> Vec2 {
        let (s, c) = self.rotation.sin_cos();
        let q = p * self.scale;
        self.insert.xy() + Vec2::new(c * q.x - s * q.y, s * q.x + c * q.y)
    }
    /// Page point (in the page's own units) of drawing point `p`; `None` at scale 0.
    pub fn to_page(&self, p: Vec2) -> Option<Vec2> {
        let (s, c) = self.rotation.sin_cos();
        let d = p - self.insert.xy();
        let q = Vec2::new(c * d.x + s * d.y, -s * d.x + c * d.y) * (1.0 / self.scale);
        q.is_finite().then_some(q)
    }
    /// The clip boundary in drawing units when it is applied (at least three vertices).
    pub fn clip_outline(&self) -> Option<Vec<Vec2>> {
        (self.clipping && self.clip.len() >= 3).then(|| self.clip.iter().map(|p| self.to_drawing(*p)).collect())
    }
    /// The outline the underlay covers: its clip boundary, else (not clipped, or clipped
    /// inverted) the page's edges.
    pub fn outline(&self) -> Vec<Vec2> {
        match self.clip_outline() {
            Some(c) if !self.clip_inverted => c,
            _ => self.corners().to_vec(),
        }
    }
    /// The closed outlines drawn as the underlay's frame: its clip boundary when clipped
    /// (inverted too), else the page's edges.
    pub fn frames(&self) -> Vec<Vec<Vec2>> {
        vec![self.clip_outline().unwrap_or_else(|| self.corners().to_vec())]
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub insert: Vec3,
    /// One pixel's width (U) and height (V) vectors in drawing units.
    pub u: Vec3,
    pub v: Vec3,
    /// Image size in pixels.
    pub size: Vec2,
    /// The image file (its IMAGEDEF object's path).
    pub path: String,
    /// The image definition's name (its key in the drawing's image dictionary).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Clip boundary in pixel coordinates (origin at the top-left corner of the image, y down):
    /// two opposite corners of a rectangle, or a polygon's vertices. Empty: the whole image.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clip: Vec<Vec2>,
    /// The clip boundary is applied.
    #[serde(default)]
    pub clipping: bool,
    /// Display flags: 1 show the image, 2 show it when not aligned with the screen, 4 use the
    /// clip boundary, 8 transparency on.
    #[serde(default = "image_display")]
    pub display: u16,
    /// Brightness, contrast and fade, 0..=100.
    #[serde(default = "image_fifty")]
    pub brightness: u8,
    #[serde(default = "image_fifty")]
    pub contrast: u8,
    #[serde(default)]
    pub fade: u8,
    /// The definition's default size of one pixel and its resolution unit (0 none, 2 cm, 5 inch).
    #[serde(default = "image_pixel")]
    pub pixel_size: Vec2,
    #[serde(default)]
    pub resolution_units: u8,
}

fn image_display() -> u16 {
    7
}

fn image_fifty() -> u8 {
    50
}

fn image_pixel() -> Vec2 {
    Vec2::new(1.0, 1.0)
}

impl Default for Image {
    fn default() -> Self {
        Image {
            insert: Vec3::ZERO,
            u: Vec3::new(1.0, 0.0, 0.0),
            v: Vec3::new(0.0, 1.0, 0.0),
            size: Vec2::new(1.0, 1.0),
            path: String::new(),
            name: String::new(),
            clip: Vec::new(),
            clipping: false,
            display: image_display(),
            brightness: image_fifty(),
            contrast: image_fifty(),
            fade: 0,
            pixel_size: image_pixel(),
            resolution_units: 0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableCell {
    pub text: String,
    #[serde(default)]
    pub merged: Option<(u32, u32)>,
    /// The cell's own cell style; empty: its row's.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub style: String,
    /// A formula (`=Sum(A1:B3)`); `text` holds its value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
    /// The cell's own text height (DXF 140 in the cell record), before the table's scale;
    /// `None`: its cell style's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_height: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Table {
    pub insert: Vec3,
    pub col_widths: Vec<f64>,
    pub row_heights: Vec<f64>,
    pub cells: Vec<Vec<TableCell>>,
    #[serde(default = "standard")]
    pub style: String,
    #[serde(default = "default_text_h")]
    pub text_height: f64,
    /// First row is a title (merged across, larger text).
    #[serde(default)]
    pub title: bool,
    /// The row after the title holds column headers.
    #[serde(default)]
    pub header: bool,
    /// The cell style of each row; missing or empty: by position (Title, Header, Data).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_styles: Vec<String>,
    /// Table breaking (TABLEBREAK): the table drawn in several parts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breaks: Option<crate::TableBreak>,
    /// The CSV file the cells come from (DATALINK).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_link: Option<crate::DataLink>,
}

fn default_text_h() -> f64 {
    0.18
}
