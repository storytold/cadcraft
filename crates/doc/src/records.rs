//! The parts of a drawing's records that CADCraft's commands don't edit but a file holds: the rest
//! of a layout's plot settings and layout data, a model space viewport's view settings, the rest
//! of a multileader or table style, a dimension link's snap data and the registered applications.
//! Each field is named and typed after the public DXF Reference (and, where it says nothing, the
//! Open Design Alliance's public DWG specification), read from DXF and DWG and written back.
//!
//! Defaults are what a new drawing's records hold, so a record read with default values equals
//! one made here.

use cadcraft_geom::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::{Handle, KeptHandle, PageSetup};

fn is_zero_handle(h: &Handle) -> bool {
    h.is_none()
}

/// The rest of a layout's plot settings (PLOTSETTINGS, the first part of a LAYOUT object).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlotSettings {
    /// Page setup name (group 1).
    pub name: String,
    /// Plot view name (group 6).
    pub view_name: String,
    /// Plot origin in millimetres from the printable area's corner (46, 47).
    pub origin: [f64; 2],
    /// Paper image origin (148, 149).
    pub image_origin: [f64; 2],
    /// Shade plot mode (76): 0 as displayed, 1 wireframe, 2 hidden, 3 rendered.
    pub shade_mode: i16,
    /// Shade plot resolution level (77): 0 draft, 1 preview, 2 normal, 3 presentation, 4 maximum,
    /// 5 custom.
    pub shade_resolution: i16,
    /// Shade plot custom DPI (78), for resolution level 5.
    pub shade_dpi: i16,
    /// The visual style or render preset used to shade the plot (333), when set.
    #[serde(skip_serializing_if = "is_zero_handle")]
    pub shade_plot: Handle,
}

impl Default for PlotSettings {
    fn default() -> Self {
        PlotSettings {
            name: String::new(),
            view_name: String::new(),
            origin: [0.0; 2],
            image_origin: [0.0; 2],
            shade_mode: 0,
            shade_resolution: 2,
            shade_dpi: 300,
            shade_plot: Handle(0),
        }
    }
}

/// A plot settings record's values as the file held them, where CADCraft's page setup keeps its
/// own form of them ([`PageSetup`]: paper size in portrait order, a landscape flag, a scale
/// ratio). Written back while the page is still `basis`, what CADCraft made of them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlotRead {
    pub basis: PageSetup,
    /// Plot configuration (2) and paper size name (4), as written in the file.
    pub device: String,
    pub paper: String,
    /// Paper size (44, 45), in the file's order.
    pub size: [f64; 2],
    /// Plot layout flags (70).
    pub flags: i64,
    /// Plot paper units (72): 0 inches, 1 millimetres, 2 pixels.
    pub paper_units: i16,
    /// Plot rotation (73): 0, 90, 180 or 270 degrees counter-clockwise as 0–3.
    pub rotation: i16,
    /// Custom print scale: paper units (142) per drawing units (143).
    pub scale: [f64; 2],
    /// Standard scale type (75) and the scale factor (147).
    pub standard_scale: i16,
    pub factor: f64,
    /// Plot type (74): 0 display, 1 extents, 2 limits, 3 view, 4 window, 5 layout information.
    pub plot_type: i16,
}

impl PlotRead {
    /// Whether `page` is still what CADCraft made of the record when it was read, so the record's
    /// own values describe it. (The rest of the plot settings are kept apart and don't count.)
    pub fn still(&self, page: &PageSetup) -> bool {
        let mut p = page.clone();
        p.settings = self.basis.settings.clone();
        self.basis == p
    }
}

impl Default for PlotRead {
    fn default() -> Self {
        PlotRead {
            basis: PageSetup::default(),
            device: String::new(),
            paper: String::new(),
            size: [0.0; 2],
            flags: 0,
            paper_units: 0,
            rotation: 0,
            scale: [1.0, 1.0],
            standard_scale: 16,
            factor: 1.0,
            plot_type: 5,
        }
    }
}

/// A layout's own data (the AcDbLayout part of a LAYOUT object).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LayoutRecord {
    /// Layout flags (70): 1 PSLTSCALE, 2 LIMCHECK.
    pub flags: i16,
    /// Limits (10, 11), insertion base (12), extents (14, 15) and elevation (146) of the layout.
    pub limits_min: Vec2,
    pub limits_max: Vec2,
    pub insbase: Vec3,
    pub extents_min: Vec3,
    pub extents_max: Vec3,
    pub elevation: f64,
    /// The layout's UCS: origin (13), X and Y axes (16, 17) and orthographic type (76).
    pub ucs_origin: Vec3,
    pub ucs_x: Vec3,
    pub ucs_y: Vec3,
    pub ortho_type: i16,
    /// The viewport last active in the layout (331), its named UCS (345) and base UCS (346).
    #[serde(skip_serializing_if = "is_zero_handle")]
    pub last_viewport: Handle,
    #[serde(skip_serializing_if = "is_zero_handle")]
    pub named_ucs: Handle,
    #[serde(skip_serializing_if = "is_zero_handle")]
    pub base_ucs: Handle,
}

impl Default for LayoutRecord {
    fn default() -> Self {
        LayoutRecord {
            flags: 1,
            limits_min: Vec2::ZERO,
            limits_max: Vec2::new(12.0, 9.0),
            insbase: Vec3::ZERO,
            extents_min: Vec3::ZERO,
            extents_max: Vec3::ZERO,
            elevation: 0.0,
            ucs_origin: Vec3::ZERO,
            ucs_x: Vec3::new(1.0, 0.0, 0.0),
            ucs_y: Vec3::new(0.0, 1.0, 0.0),
            ortho_type: 0,
            last_viewport: Handle(0),
            named_ucs: Handle(0),
            base_ucs: Handle(0),
        }
    }
}

/// A model space viewport's view settings (the rest of a VPORT table record). Snap and grid
/// spacing, snap base and snap angle are the drawing's header variables.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VportSettings {
    /// View direction from the target (16) and view target (17).
    pub direction: Vec3,
    pub target: Vec3,
    /// Lens length (42), front and back clipping planes (43, 44) and view twist in radians (51).
    pub lens: f64,
    pub front_clip: f64,
    pub back_clip: f64,
    pub twist: f64,
    /// View mode (71, VIEWMODE), circle zoom percent (72), fast zoom (73), UCSICON (74), snap on
    /// (75), grid on (76), snap style (77), snap isopair (78).
    pub view_mode: i16,
    pub circle_zoom: i16,
    pub fast_zoom: i16,
    pub ucs_icon: i16,
    pub snap_on: i16,
    pub grid_on: i16,
    pub snap_style: i16,
    pub snap_isopair: i16,
    /// Plot style sheet (1) and render mode (281).
    pub style_sheet: Option<String>,
    pub render_mode: i16,
    /// The viewport has its own UCS (65, UCSVP): origin (110), X and Y axes (111, 112),
    /// orthographic type (79) and elevation (146); named UCS (345) and base UCS (346).
    pub ucs_per_viewport: i16,
    pub ucs_origin: Vec3,
    pub ucs_x: Vec3,
    pub ucs_y: Vec3,
    pub ortho_type: i16,
    pub elevation: f64,
    #[serde(skip_serializing_if = "is_zero_handle")]
    pub named_ucs: Handle,
    #[serde(skip_serializing_if = "is_zero_handle")]
    pub base_ucs: Handle,
    /// Shade plot setting (170), grid flags (60) and major grid line frequency (61).
    pub shade_plot: i16,
    pub grid_flags: i16,
    pub major_grid: i16,
    /// Background (332), shade plot (333) and visual style (348) objects, when set.
    #[serde(skip_serializing_if = "is_zero_handle")]
    pub background: Handle,
    #[serde(skip_serializing_if = "is_zero_handle")]
    pub shade_plot_object: Handle,
    #[serde(skip_serializing_if = "is_zero_handle")]
    pub visual_style: Handle,
    /// Default lighting on (292) and its type (282), brightness (141), contrast (142), ambient
    /// colour (63, and 421 as a true colour).
    pub default_lighting: bool,
    pub lighting_type: i16,
    pub brightness: f64,
    pub contrast: f64,
    pub ambient: Option<i16>,
    pub ambient_rgb: Option<i64>,
}

impl Default for VportSettings {
    fn default() -> Self {
        VportSettings {
            direction: Vec3::new(0.0, 0.0, 1.0),
            target: Vec3::ZERO,
            lens: 50.0,
            front_clip: 0.0,
            back_clip: 0.0,
            twist: 0.0,
            view_mode: 0,
            circle_zoom: 1000,
            fast_zoom: 1,
            ucs_icon: 3,
            snap_on: 0,
            grid_on: 0,
            snap_style: 0,
            snap_isopair: 0,
            style_sheet: None,
            render_mode: 0,
            ucs_per_viewport: 1,
            ucs_origin: Vec3::ZERO,
            ucs_x: Vec3::new(1.0, 0.0, 0.0),
            ucs_y: Vec3::new(0.0, 1.0, 0.0),
            ortho_type: 0,
            elevation: 0.0,
            named_ucs: Handle(0),
            base_ucs: Handle(0),
            shade_plot: 0,
            grid_flags: 3,
            major_grid: 5,
            background: Handle(0),
            shade_plot_object: Handle(0),
            visual_style: Handle(0),
            default_lighting: true,
            lighting_type: 1,
            brightness: 0.0,
            contrast: 0.0,
            ambient: None,
            ambient_rgb: None,
        }
    }
}

/// ByBlock as a 32-bit colour value (0xC1000000), as multileader styles write it.
pub const COLOR_BYBLOCK: i64 = -1_056_964_608;

/// The rest of a multileader style (MLEADERSTYLE): everything but the sizes, text style and
/// annotative flag CADCraft's commands use ([`crate::MLeaderStyle`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MLeaderStyleProps {
    /// Style version (179), description (3).
    pub version: i16,
    pub description: String,
    /// Content type (170): 1 block, 2 multiline text, 3 tolerance; draw order of the multileader
    /// (171) and its leaders (172).
    pub content_type: i16,
    pub draw_mleader_order: i16,
    pub draw_leader_order: i16,
    /// Most points of a leader line (90), angle constraints of the first and second segments
    /// (40, 41, radians).
    pub max_leader_points: i32,
    pub first_angle: f64,
    pub second_angle: f64,
    /// Leader line type (173: 0 invisible, 1 straight, 2 spline), colour (91), linetype (340, by
    /// name) and lineweight (92).
    pub leader_type: i16,
    pub leader_color: i64,
    pub leader_linetype: String,
    pub leader_lineweight: i32,
    /// Landing (290), dogleg (291).
    pub landing: bool,
    pub dogleg_on: bool,
    /// Arrowhead block (341, by name; empty: closed filled).
    pub arrow_block: String,
    /// Default text contents (300).
    pub default_text: String,
    /// Text attachment on the left and right (174, 178), angle type (175), alignment (176),
    /// colour (93), frame (292), always left aligned (297), alignment space (46).
    pub text_left_attachment: i16,
    pub text_right_attachment: i16,
    pub text_angle: i16,
    pub text_alignment: i16,
    pub text_color: i64,
    pub text_frame: bool,
    pub text_align_left: bool,
    pub align_space: f64,
    /// Block content (343, by name), its colour (94), scale (47, 49, 140) and whether it applies
    /// (293), rotation (141) and whether it applies (294), connection type (177).
    pub block: String,
    pub block_color: i64,
    pub block_scale: [f64; 3],
    pub block_scale_on: bool,
    pub block_rotation: f64,
    pub block_rotation_on: bool,
    pub block_connection: i16,
    /// Overall scale (142), property values overwritten (295), break gap size (143).
    pub scale: f64,
    pub overwrite: bool,
    pub break_gap: f64,
    /// Text attachment direction (271: 0 horizontal, 1 vertical), bottom (272) and top (273)
    /// attachment.
    pub attachment_direction: i16,
    pub bottom_attachment: i16,
    pub top_attachment: i16,
    /// Group 298 (AutoCAD 2013 and later; not described in the DXF Reference), as read.
    pub group_298: Option<bool>,
}

impl Default for MLeaderStyleProps {
    fn default() -> Self {
        MLeaderStyleProps {
            version: 2,
            description: String::new(),
            content_type: 2,
            draw_mleader_order: 1,
            draw_leader_order: 0,
            max_leader_points: 2,
            first_angle: 0.0,
            second_angle: 0.0,
            leader_type: 1,
            leader_color: COLOR_BYBLOCK,
            leader_linetype: "ByBlock".into(),
            leader_lineweight: -2,
            landing: true,
            dogleg_on: true,
            arrow_block: String::new(),
            default_text: String::new(),
            text_left_attachment: 1,
            text_right_attachment: 1,
            text_angle: 1,
            text_alignment: 0,
            text_color: COLOR_BYBLOCK,
            text_frame: false,
            text_align_left: false,
            align_space: 4.0,
            block: String::new(),
            block_color: COLOR_BYBLOCK,
            block_scale: [1.0; 3],
            block_scale_on: true,
            block_rotation: 0.0,
            block_rotation_on: true,
            block_connection: 0,
            scale: 1.0,
            overwrite: false,
            break_gap: 0.125,
            attachment_direction: 0,
            bottom_attachment: 9,
            top_attachment: 9,
            group_298: None,
        }
    }
}

/// The data format of one cell type of a table style: data type (90), unit type (91) and format
/// string (1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TableCellData {
    pub data_type: i32,
    pub unit_type: i32,
    pub format: String,
}

impl Default for TableCellData {
    fn default() -> Self {
        TableCellData { data_type: 512, unit_type: 0, format: String::new() }
    }
}

/// The rest of a table style (TABLESTYLE): its version, description, flow direction and flags,
/// and the data format of the data, header and title cell types.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TableStyleProps {
    /// Version (the 280 before the description; AutoCAD 2007 and later), description (3).
    pub version: Option<i16>,
    pub description: String,
    /// Flow direction (70: 0 down, 1 up) and flags (71).
    pub flow_up: bool,
    pub flags: i16,
    /// Data, column header and title.
    pub cells: [TableCellData; 3],
}

/// A DIMASSOC point reference as the file held it, and the link CADCraft made of it: written back
/// while the link is still `snap` on `handle`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OsnapRead {
    pub handle: Handle,
    pub snap: Option<crate::AssocSnap>,
    /// Object snap type (72), subentity type (73), GS marker (91), xref handle (301, `None` when
    /// the file had none), near osnap parameter (40), the other object of an intersection (332).
    pub osnap: i16,
    pub subent: i16,
    pub gs_marker: i64,
    pub xref: Option<String>,
    pub param: f64,
    pub other: Handle,
}

impl Default for OsnapRead {
    fn default() -> Self {
        OsnapRead { handle: Handle(0), snap: None, osnap: 0, subent: 0, gs_marker: 0, xref: None, param: 0.0, other: Handle(0) }
    }
}

/// A registered application (APPID table record): the applications whose extended data the
/// drawing may carry, in the case they were registered with.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppId {
    pub name: String,
    #[serde(skip_serializing_if = "KeptHandle::is_none")]
    pub handle: KeptHandle,
    /// Flags (70).
    pub flags: i16,
}
