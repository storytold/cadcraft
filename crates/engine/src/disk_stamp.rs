//! What a drawing file held when this session last read or wrote it.
//!
//! CADCraft keeps no file open while a drawing is edited: it reads the bytes and later replaces the
//! file atomically. Nothing then stops another program (another CAD program on a shared folder, a
//! sync client, another CADCraft) from saving the same file in between, and a plain save would
//! silently replace that work. A save over the drawing's own file therefore first checks that the
//! file still holds what was read or last written, and refuses otherwise (QSAVE/SAVEAS
//! `overwrite: true` replaces it anyway).

use std::hash::{DefaultHasher, Hasher};

/// The length and a 64-bit hash of a file's bytes. The hash only has to notice a change made by
/// someone else within this process's lifetime, so the standard library's hasher is enough.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskStamp {
    len: u64,
    hash: u64,
}

impl DiskStamp {
    pub fn of(bytes: &[u8]) -> Self {
        let mut h = DefaultHasher::new();
        h.write(bytes);
        DiskStamp { len: bytes.len() as u64, hash: h.finish() }
    }
}

/// Whether the file at `path` no longer holds `stamp`. A file that is gone has nothing to lose, so
/// it counts as unchanged; one that can't be read, or is larger than `max` (it can't be what was
/// read), counts as changed.
#[cfg(not(target_arch = "wasm32"))]
pub fn changed(path: &std::path::Path, stamp: DiskStamp, max: usize) -> bool {
    match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return false,
        Err(_) => return true,
        Ok(meta) if meta.len() != stamp.len => return true,
        Ok(_) => {}
    }
    match crate::capped_read::read_capped(path, max) {
        Ok(bytes) => DiskStamp::of(&bytes) != stamp,
        Err(e) => e.kind() != std::io::ErrorKind::NotFound,
    }
}
