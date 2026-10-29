use super::cmd;

/// Options and values the interactive commands remember for the rest of the session, offered as
/// the `<default>` the next time they ask (as AutoCAD does). Values AutoCAD keeps in a drawing
/// system variable (HPNAME, OFFSETDIST, FILLETRAD, CHAMFERA, POLYSIDES…) live in the drawing
/// header instead. JSON calls never read these: their explicit parameters and documented defaults
/// stay authoritative.
#[derive(Clone, Debug, PartialEq)]
pub struct LastUsed {
    /// HATCH/GRADIENT starts at "Select objects" instead of "Pick internal point".
    pub hatch_select: bool,
    /// POLYGON: circumscribed about the circle (the `C` option) instead of inscribed.
    pub polygon_circumscribed: bool,
    /// ROTATE angle (radians).
    pub rotate_angle: f64,
    /// SCALE factor.
    pub scale_factor: f64,
    /// TXT2MTXT's Settings.
    pub txt2mtxt: cmd::Txt2MtxtOptions,
    /// MATCHPROP Settings: the properties it copies.
    pub matchprop: cmd::MatchProps,
    /// MLEADER placement order and Options.
    pub mleader: MLeaderOptions,
    /// MLEADER's block content name (Options > Content type > Block).
    pub mleader_block: String,
    /// BREAKLINE's Block, Size and Extension.
    pub breakline: cmd::BreaklineOptions,
}

impl Default for LastUsed {
    fn default() -> Self {
        LastUsed {
            hatch_select: false,
            polygon_circumscribed: false,
            rotate_angle: 0.0,
            scale_factor: 1.0,
            txt2mtxt: cmd::Txt2MtxtOptions::default(),
            matchprop: cmd::MatchProps::default(),
            mleader: MLeaderOptions::default(),
            mleader_block: String::new(),
            breakline: cmd::BreaklineOptions::default(),
        }
    }
}

/// Which part of a multileader MLEADER places first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LeaderOrder {
    #[default]
    Arrowhead,
    Landing,
    Content,
}

/// The MLEADER placement order and the values set at its Options prompt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MLeaderOptions {
    pub order: LeaderOrder,
    /// Maximum points of the leader line (2..=64).
    pub max_points: usize,
    /// First and second angle constraints (radians; 0 = free).
    pub angles: [f64; 2],
    /// Draw the landing line (dogleg).
    pub landing: bool,
    /// Ask for multiline text content (Content type None turns it off).
    pub content: bool,
    /// Leader lines: straight, spline, or none (content only).
    pub leader_type: LeaderType,
    /// The content is a block (`LastUsed::mleader_block`) instead of multiline text.
    pub block: bool,
    /// The block's extents centre (not its insertion point) sits at the end of the landing.
    pub block_center: bool,
}

/// MLEADER's leader line type (Options > Leader type).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LeaderType {
    #[default]
    Straight,
    Spline,
    None,
}

impl Default for MLeaderOptions {
    fn default() -> Self {
        MLeaderOptions {
            order: LeaderOrder::Arrowhead,
            max_points: 2,
            angles: [0.0; 2],
            landing: true,
            content: true,
            leader_type: LeaderType::Straight,
            block: false,
            block_center: true,
        }
    }
}
