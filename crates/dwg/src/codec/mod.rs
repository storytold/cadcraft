//! CADCraft's own DWG container layer: the parts of a DWG file that don't depend on what the
//! drawing holds.
//!
//! A DWG file is, from R2004 on, a set of compressed and checksummed pages, and before that a flat
//! file with a table of section locators. This module reads that structure for every version from
//! R13 (AC1012) to 2018 and later (AC1032), except AutoCAD 2007-2009 (AC1021), whose container is a
//! different Reed-Solomon coded variant, and writes the 2018 (AC1032) container:
//!
//! - [`bits`]: the bit-coded values the format is built from (`BitReader`, `BitWriter`).
//! - [`crc`], [`lz77`]: the checksums and the compression of the paged container.
//! - [`read_container`]: sections, object map and class list of a file, with a note for everything
//!   in it that did not check out (a bad checksum, a missing sentinel) instead of a failure.
//! - [`pack`]: a file from its sections.
//!
//! The format knowledge comes from the Open Design Alliance's public DWG specification only.
//! Nothing here depends on CADCraft's drawing model: mapping objects to it comes on top of this.
//!
//! A file is only a claim. Every size, offset and count read from it is checked before it is used,
//! and no input reaches a panic (`tests::hostile_input_never_panics` mutates and truncates files).

// Reading never indexes or slices unchecked: a file is only a claim.
#![deny(clippy::indexing_slicing)]

pub mod bits;
pub mod crc;
mod error;
pub mod lz77;
pub mod pack;
mod read;
mod text;
mod types;
mod version;

mod classes;
mod container;
pub mod edit;
mod objmap;
mod r2000;
mod r2004;
mod section;
mod streams;
#[cfg(test)]
mod tests;

pub use error::{Error, ErrorKind, Result};
pub use read::{CommonData, DwgContainer, EntitySpace, ObjectRef, RawObject, SpaceMarkers, read_container, sniff_version};
pub use types::{ClassDef, Diagnostic, SectionInfo, Severity, fixed_type, type_name};
pub use version::DwgVersion;
