//! Bounded reads of local files.
//!
//! Reading a path with [`std::fs::read`] buffers until EOF, so a path naming an endless readable
//! device (for example `/dev/urandom` or a FIFO) exhausts process memory before any size check
//! runs (CWE-400). [`read_capped`] opens the path once, refuses anything that is not a regular
//! file, and reads through a reader limited to `max + 1` bytes so oversized input is rejected
//! before it is fully buffered.

#![cfg(not(target_arch = "wasm32"))]

use std::fs::OpenOptions;
use std::io::{self, Read};
use std::path::Path;

/// Read at most `max` bytes from the regular file at `path`.
///
/// Returns an error if the path is not a regular file, cannot be opened, or holds more than `max`
/// bytes. At most `max + 1` bytes are ever buffered, so an endless device cannot exhaust memory.
pub fn read_capped(path: &Path, max: usize) -> io::Result<Vec<u8>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a regular file"));
    }
    // Read one byte past the limit so we can tell "exactly max" from "too big" without trusting
    // the metadata length (which a device may misreport).
    let mut buf = Vec::new();
    let cap = (max as u64).saturating_add(1);
    let read = file.take(cap).read_to_end(&mut buf)?;
    if read as u64 > max as u64 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("file is larger than the {max}-byte limit")));
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regular-file reads accept the byte boundary and reject any larger payload.
    #[test]
    fn reads_within_limit_and_rejects_oversize() {
        let dir = std::env::temp_dir().join(format!("cadcraft-cr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("f.bin");
        std::fs::write(&p, vec![7u8; 100]).unwrap();
        assert_eq!(read_capped(&p, 100).unwrap().len(), 100);
        assert_eq!(read_capped(&p, 200).unwrap().len(), 100);
        assert!(read_capped(&p, 99).is_err());
        assert_eq!(read_capped(&p, usize::MAX).unwrap().len(), 100);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// An endless device is rejected before its contents can be buffered.
    #[cfg(unix)]
    #[test]
    fn rejects_non_regular_file() {
        // /dev/zero is an endless readable device; it must be rejected, not buffered.
        let p = Path::new("/dev/zero");
        if p.exists() {
            assert!(read_capped(p, 8 << 20).is_err());
        }
    }
    /// Opening a FIFO without a writer returns an error without waiting for a peer.
    #[cfg(unix)]
    #[test]
    fn rejects_fifo_without_a_writer() {
        let dir = std::env::temp_dir().join(format!("cadcraft-cr-fifo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fifo = dir.join("pipe");
        assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
        let started = std::time::Instant::now();
        assert_eq!(read_capped(&fifo, 1024).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
