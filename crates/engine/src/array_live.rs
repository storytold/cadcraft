//! Live array parameters of a running ARRAYRECT / ARRAYPOLAR: what the interactive command has
//! while it runs, read and written from the array window and kept in step with the command line.

use crate::Vec2;

/// Which array the live parameters belong to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayLiveKind {
    Rectangular,
    Polar,
}

/// The parameters of the running array command, with the fields of the other kind left at zero.
#[derive(Clone, Debug, PartialEq)]
pub struct ArrayLive {
    pub kind: ArrayLiveKind,
    /// Rectangular: number of columns.
    pub cols: u64,
    /// Both: number of rows.
    pub rows: u64,
    /// Rectangular: distance between columns.
    pub col_spacing: f64,
    /// Both: distance between rows.
    pub row_spacing: f64,
    /// Both: number of levels.
    pub levels: u64,
    /// Both: distance between levels.
    pub level_spacing: f64,
    /// Both: elevation added per row.
    pub row_elevation: f64,
    /// Both: whether the array is associative.
    pub associative: bool,
    /// Polar: number of items.
    pub items: u64,
    /// Polar: fill angle in degrees (positive = counterclockwise).
    pub fill: f64,
    /// Polar: whether items are rotated.
    pub rotate: bool,
    /// Polar: the centre; `None` for a rectangular array.
    pub center: Option<Vec2>,
    /// Polar only: the angle between items, degrees, derived from items and fill.
    /// `None` for Rectangular.
    pub between: Option<f64>,
}
