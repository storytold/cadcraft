//! Version-independent view of the sections the reader needs.

use super::types::{Diagnostic, SectionInfo};

pub(crate) struct Container {
    pub maintenance: u8,
    pub codepage: u16,
    /// Header section bytes, starting at its start sentinel.
    pub header: Option<Vec<u8>>,
    /// Classes section bytes, starting at its start sentinel.
    pub classes: Option<Vec<u8>>,
    /// Object map bytes.
    pub handles: Vec<u8>,
    /// Bytes that object-map locations index into: the whole file for
    /// R13–R15, the AcDb:AcDbObjects section for R2004+.
    pub objects: Vec<u8>,
    pub sections: Vec<SectionInfo>,
    /// The other named sections of an R2004-family file (AcDb:SummaryInfo,
    /// AcDb:Preview, AcDb:AuxHeader, …), decompressed, in section-map
    /// order. Sections that could not be read are left out. Empty for
    /// R13–R15.
    pub others: Vec<(String, Vec<u8>)>,
    /// Each section of an R2004-family file as the section map describes it, in section-map order
    /// (the unnamed one included): what rewriting the file needs to lay it out again. Empty for
    /// R13–R15.
    pub layout: Vec<SectionLayout>,
    pub notes: Vec<Diagnostic>,
}

/// How one section of an R2004-family file is stored ([`Container::layout`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SectionLayout {
    pub name: String,
    /// Bytes per page before compression.
    pub page_size: u32,
    pub compressed: bool,
    pub encrypted: bool,
    /// The number of its first page (0 for none): pages are numbered in file order.
    pub first_page: i32,
}
