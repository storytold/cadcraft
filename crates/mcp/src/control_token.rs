//! A per-session credential for the loopback control channel.
//!
//! The control server listens on `127.0.0.1`, but loopback TCP does not distinguish operating
//! system users: any local account can connect and drive the desktop user's command and file
//! authority (CWE-306). To authenticate the peer without changing the transport, the app issues an
//! unguessable token at startup and writes it to a file only the desktop user can read. A client
//! (the MCP bridge) reads the token from that file — which requires the same user's filesystem
//! access — and sends it with every request. A different local user cannot read the token, so
//! cannot authenticate.

use std::io;
use std::path::PathBuf;

/// Length of the token in bytes (256 bits), rendered as lowercase hex.
const TOKEN_BYTES: usize = 32;

/// The file holding the control token for `port`, inside a per-user private directory.
///
/// On Unix the runtime dir (`XDG_RUNTIME_DIR`, already `0700` and per-user) is preferred, else the
/// user's home; on Windows the local app-data directory. The file itself is created with
/// owner-only permissions regardless.
pub fn token_path(port: u16) -> io::Result<PathBuf> {
    Ok(private_dir()?.join(format!("cadcraft-control-{port}.token")))
}

/// Choose the Unix runtime directory, home cache, or temporary directory for an owner-only token file.
#[cfg(unix)]
fn private_dir() -> io::Result<PathBuf> {
    if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(d));
    }
    if let Some(h) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(h).join(".cache"));
    }
    Ok(std::env::temp_dir())
}

/// Require the Windows per-user local app-data directory for session token storage.
#[cfg(windows)]
fn private_dir() -> io::Result<PathBuf> {
    windows_private_dir(std::env::var_os("LOCALAPPDATA"))
}

/// Validate that Windows supplied a nonempty local app-data path instead of using a shared fallback.
#[cfg(any(windows, test))]
fn windows_private_dir(local: Option<std::ffi::OsString>) -> io::Result<PathBuf> {
    local
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is required for a private control token"))
}

/// Reject token storage on platforms without a supported private-directory convention.
#[cfg(not(any(unix, windows)))]
fn private_dir() -> io::Result<PathBuf> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "no private control token directory on this platform"))
}

/// Generate a fresh token and write it to [`token_path`] with owner-only permissions, returning
/// the token. The file is replaced if it already exists.
pub fn issue(port: u16) -> io::Result<String> {
    let token = random_hex()?;
    let path = token_path(port)?;
    issue_at(&path, token.as_bytes())?;
    Ok(token)
}

/// Create missing parent directories, then write the session token through the platform privacy helper.
fn issue_at(path: &std::path::Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_private(path, bytes)
}

/// Read the token for `port`, trimming trailing whitespace. Used by clients (the MCP bridge).
pub fn read(port: u16) -> io::Result<String> {
    let raw = std::fs::read_to_string(token_path(port)?)?;
    let token = raw.trim().to_string();
    if token.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "empty control token"));
    }
    Ok(token)
}

/// Remove the token file (best effort), called when the server stops.
pub fn revoke(port: u16) {
    if let Ok(path) = token_path(port) {
        let _ = std::fs::remove_file(path);
    }
}

/// Constant-time comparison of two tokens, so a wrong token cannot be guessed byte by byte.
pub fn matches(expected: &str, given: &str) -> bool {
    let (a, b) = (expected.as_bytes(), given.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Encode 256 bits of operating-system randomness as lowercase hexadecimal, propagating entropy failures.
fn random_hex() -> io::Result<String> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|e| io::Error::other(format!("no secure randomness: {e}")))?;
    let mut s = String::with_capacity(TOKEN_BYTES * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    Ok(s)
}

/// Replace a stale token with an exclusively created file using Unix mode `0600`, then flush its bytes.
#[cfg(unix)]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    // Replace any stale file, then create fresh with mode 0600 so no other user can read the token.
    let _ = std::fs::remove_file(path);
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
    f.write_all(bytes)?;
    f.flush()
}

/// Exclusively create and flush a replacement token within the per-user directory selected by the caller.
#[cfg(not(unix))]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    // On Windows the per-user LOCALAPPDATA directory is already ACL-protected to the user.
    let _ = std::fs::remove_file(path);
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(bytes)?;
    f.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An issued token has the expected size, can be read back, and disappears after revocation.
    #[test]
    fn issue_then_read_round_trips() {
        let port = 50000 + (std::process::id() % 10000) as u16;
        let token = issue(port).unwrap();
        assert_eq!(token.len(), TOKEN_BYTES * 2);
        assert_eq!(read(port).unwrap(), token);
        revoke(port);
        assert!(read(port).is_err());
    }

    /// Token comparison rejects different bytes and lengths while accepting identical tokens.
    #[test]
    fn matches_is_exact() {
        assert!(matches("abc", "abc"));
        assert!(!matches("abc", "abd"));
        assert!(!matches("abc", "ab"));
        assert!(!matches("", "x"));
    }

    /// A token issued on Unix grants no file permission bits to group or other users.
    #[cfg(unix)]
    #[test]
    fn token_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let port = 40000 + (std::process::id() % 10000) as u16;
        issue(port).unwrap();
        let mode = std::fs::metadata(token_path(port).unwrap()).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "token file is readable by group/other: {:o}", mode);
        revoke(port);
    }
    /// Windows token storage rejects absent or empty app-data paths and preserves a supplied path.
    #[test]
    fn windows_requires_local_app_data() {
        assert!(windows_private_dir(None).is_err());
        assert!(windows_private_dir(Some("".into())).is_err());
        assert_eq!(windows_private_dir(Some("private-app-data".into())).unwrap(), PathBuf::from("private-app-data"));
    }

    /// Token issuance creates its missing directory hierarchy before writing the credential.
    #[test]
    fn issuance_creates_missing_parent() {
        let dir = std::env::temp_dir().join(format!("cadcraft-token-parent-{}", std::process::id()));
        let path = dir.join("missing/cache/session.token");
        issue_at(&path, b"secret").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"secret");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
