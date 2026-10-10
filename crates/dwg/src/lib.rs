//! DWG support through a DXF bridge.
//!
//! DWG files are read with the `acadrust` crate (MPL-2.0, used unmodified as a dependency) and
//! converted to DXF bytes, which CADCraft's own DXF reader maps to its document model; saving
//! goes the other way. Keeping the dependency behind this byte-level API isolates it: nothing
//! else in CADCraft depends on its types. Native targets only (it memory-maps files).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

/// DWG version codes we can write.
pub const VERSIONS: &[(&str, &str)] =
    &[("AC1015", "2000"), ("AC1018", "2004"), ("AC1021", "2007"), ("AC1024", "2010"), ("AC1027", "2013"), ("AC1032", "2018")];

/// True when the bytes look like a DWG file (`AC10xx` magic).
pub fn is_dwg(bytes: &[u8]) -> bool {
    bytes.len() > 6 && bytes.starts_with(b"AC10") && bytes.get(4..6).is_some_and(|v| v.iter().all(u8::is_ascii_digit))
}

/// The DWG version code of a file (e.g. "AC1032").
pub fn version(bytes: &[u8]) -> Option<String> {
    is_dwg(bytes).then(|| String::from_utf8_lossy(bytes.get(0..6).unwrap_or_default()).to_string())
}

/// Size limits for reading a DWG file. DWG is compressed and the DXF text it converts to is
/// several times larger (about 6x in our tests: a 10 MB DWG becomes about 60 MB of DXF), so
/// the limits are generous for real drawings. They exist because a damaged file, or one the reader
/// misreads, can make the DWG reader produce a drawing thousands of times larger than the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Largest DWG file read at all (checked before reading starts).
    pub max_dwg_bytes: usize,
    /// Largest DXF rendition of a DWG (checked while it is written).
    pub max_dxf_bytes: usize,
}

impl Limits {
    /// 512 MB of DWG, 1 GB of DXF (more than the DXF reader's group-code limit fills).
    pub const DEFAULT: Limits = Limits { max_dwg_bytes: 512 << 20, max_dxf_bytes: 1 << 30 };
}

/// A byte count in megabytes for messages ("9.9 MB", "1024 MB").
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn mb(n: u64) -> String {
    let m = n as f64 / f64::from(1u32 << 20);
    if m >= 100.0 { format!("{m:.0} MB") } else { format!("{m:.1} MB") }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::io::{Cursor, Write};
    use std::panic::AssertUnwindSafe;

    use super::{Limits, mb};

    /// Read DWG bytes and return an ASCII DXF rendition, within [`Limits::DEFAULT`].
    pub fn dwg_to_dxf(bytes: &[u8]) -> Result<Vec<u8>, String> {
        dwg_to_dxf_with(bytes, Limits::DEFAULT)
    }

    /// [`dwg_to_dxf`] with explicit limits. Both are checked before the expensive step they
    /// guard: the input size before the DWG reader runs, the DXF size while it is written, so a
    /// runaway conversion stops at the limit instead of filling memory first.
    pub fn dwg_to_dxf_with(bytes: &[u8], limits: Limits) -> Result<Vec<u8>, String> {
        if bytes.len() > limits.max_dwg_bytes {
            return Err(format!("DWG: the file is {}, larger than the {} CADCraft opens", mb(bytes.len() as u64), mb(limits.max_dwg_bytes as u64)));
        }
        let doc = std::panic::catch_unwind(|| acadrust::DwgReader::from_stream(Cursor::new(bytes)).read())
            .map_err(|_| "the DWG reader failed on this file".to_string())?
            .map_err(|e| format!("DWG: {e}"))?;
        let mut out = CappedWriter { buf: Vec::new(), limit: limits.max_dxf_bytes, exceeded: false };
        let written = std::panic::catch_unwind(AssertUnwindSafe(|| acadrust::DxfWriter::new(&doc).write_to_writer(&mut out)))
            .map_err(|_| "the DWG→DXF conversion failed on this file".to_string())?;
        if out.exceeded {
            return Err(format!(
                "DWG: converting this {} file produced more than {} of drawing data (the limit); \
                 the file is probably damaged, or uses objects the DWG reader misreads",
                mb(bytes.len() as u64),
                mb(limits.max_dxf_bytes as u64)
            ));
        }
        written.map_err(|e| format!("DWG→DXF: {e}"))?;
        Ok(out.buf)
    }

    /// Collects the DXF text and fails the write once it would grow past `limit`.
    struct CappedWriter {
        buf: Vec<u8>,
        limit: usize,
        exceeded: bool,
    }

    impl Write for CappedWriter {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            // Sticky: once over, every later write fails too.
            if self.exceeded || self.buf.len().saturating_add(b.len()) > self.limit {
                self.exceeded = true;
                return Err(std::io::Error::other("DXF size limit reached"));
            }
            self.buf.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Oldest DWG version written; older DXF input is written as this version.
    const MIN_DWG_VERSION: acadrust::DxfVersion = acadrust::DxfVersion::AC1018;

    /// Convert DXF bytes into a DWG file of the DXF's version, and at least AutoCAD 2004
    /// (AC1018).
    pub fn dxf_to_dwg(dxf: &[u8]) -> Result<Vec<u8>, String> {
        let data = dxf.to_vec();
        let mut doc = std::panic::catch_unwind(move || acadrust::DxfReader::from_reader(Cursor::new(data)).and_then(|r| r.read()))
            .map_err(|_| "the DXF→DWG conversion failed".to_string())?
            .map_err(|e| format!("DXF: {e}"))?;
        // Write at least AutoCAD 2004 (AC1018) DWG: R2000 DWG has no true colours, transparency,
        // table styles or gradients, and readers can't tell its layouts apart reliably. The DXF
        // stays R2000; only the DWG is newer.
        if doc.version < MIN_DWG_VERSION {
            doc.version = MIN_DWG_VERSION;
        }
        acadrust::DwgWriter::write_to_vec(&doc).map_err(|e| format!("DWG write: {e}"))
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{dwg_to_dxf, dwg_to_dxf_with, dxf_to_dwg};

#[cfg(target_arch = "wasm32")]
pub fn dwg_to_dxf(_bytes: &[u8]) -> Result<Vec<u8>, String> {
    Err("DWG files can't be opened in the web build yet; save as DXF".into())
}
#[cfg(target_arch = "wasm32")]
pub fn dwg_to_dxf_with(_bytes: &[u8], _limits: Limits) -> Result<Vec<u8>, String> {
    Err("DWG files can't be opened in the web build yet; save as DXF".into())
}
#[cfg(target_arch = "wasm32")]
pub fn dxf_to_dwg(_dxf: &[u8]) -> Result<Vec<u8>, String> {
    Err("DWG files can't be written in the web build yet; save as DXF".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_magic() {
        assert!(is_dwg(b"AC1032\0\0\0\0"));
        assert!(!is_dwg(b"  0\r\nSECTION"));
        assert_eq!(version(b"AC1018xxxx").as_deref(), Some("AC1018"));
    }

    #[test]
    fn garbage_is_an_error_not_a_crash() {
        assert!(dwg_to_dxf(b"AC1032 this is not a dwg file at all").is_err());
        assert!(dwg_to_dxf(b"").is_err());
    }
}
