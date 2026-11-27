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

use hmac::{Hmac, Mac};
use sha2::Sha256;

/// Length of the token in bytes (256 bits), rendered as lowercase hex.
const TOKEN_BYTES: usize = 32;

/// Longest request line in bytes (newline excluded) the control server reads on a connection
/// before a request on it has carried the right token. The token is checked only once the whole
/// line is read (a client that sorts keys sends it last), so this bounds what a peer without the
/// token can make the app buffer (#1925). A client sends a longer request only after a shorter one
/// with the token has been answered on the same connection.
pub const MAX_UNAUTHENTICATED_LINE: usize = 64 * 1024;

/// How long a connection may stay open before a request on it has carried the right token; the
/// server closes it then, so peers without the token can't hold every connection slot until the
/// idle timeout (#1925). A client that keeps an unauthenticated connection this long opens a new one.
pub const MAX_UNAUTHENTICATED_TIME: std::time::Duration = std::time::Duration::from_secs(10);

/// Shortest and longest `hello` nonce, in hexadecimal digits.
pub const NONCE_DIGITS: std::ops::RangeInclusive<usize> = 32..=128;

/// What the server's proof is computed over besides the nonce, so a proof is never a valid token or
/// a MAC made for anything else.
const PROOF_LABEL: &[u8] = b"cadcraft control server proof v1\n";

/// A fresh nonce for a `hello` (256 bits of operating-system randomness, hex).
pub fn new_nonce() -> io::Result<String> {
    random_hex()
}

/// Whether `nonce` is one the server answers: hexadecimal digits, [`NONCE_DIGITS`] long.
pub fn valid_nonce(nonce: &str) -> bool {
    NONCE_DIGITS.contains(&nonce.len()) && nonce.bytes().all(|b| b.is_ascii_hexdigit())
}

/// HMAC-SHA-256 of the proof label and `nonce`, keyed with the session token (HMAC takes keys of
/// any length, so it is always `Some`).
fn proof_mac(token: &str, nonce: &str) -> Option<Hmac<Sha256>> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(token.as_bytes()).ok()?;
    mac.update(PROOF_LABEL);
    mac.update(nonce.as_bytes());
    Some(mac)
}

/// The server's answer to a `hello` carrying `nonce`: it shows the server holds the session token
/// without sending the token, so a client can tell the app from another program listening on its
/// port before it sends a request (#1925).
pub fn server_proof(token: &str, nonce: &str) -> String {
    proof_mac(token, nonce).map(|mac| hex(&mac.finalize().into_bytes())).unwrap_or_default()
}

/// Whether `proof` is [`server_proof`] for `token` and `nonce`, compared in constant time.
pub fn verify_server_proof(token: &str, nonce: &str, proof: &str) -> bool {
    let (Some(bytes), Some(mac)) = (unhex(proof), proof_mac(token, nonce)) else { return false };
    mac.verify_slice(&bytes).is_ok()
}

/// Lowercase hexadecimal.
fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// The bytes of an even-length hexadecimal string, or `None`.
fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    text.as_bytes().chunks(2).map(|pair| std::str::from_utf8(pair).ok().and_then(|p| u8::from_str_radix(p, 16).ok())).collect()
}

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
    // The shared temporary directory would put the credential at a path any local account can
    // predict and plant a link at: refuse instead.
    Err(io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR or HOME is required for a private control token"))
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
    read_at(&token_path(port)?)
}

/// Read a token file the issuer wrote: not through a link, a plain file, and (Unix) readable by
/// its owner only, so a file another account planted or left open is not taken as the credential.
fn read_at(path: &std::path::Path) -> io::Result<String> {
    use std::io::Read;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "the control token is not a plain file"));
    }
    #[cfg(unix)]
    if std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o077 != 0 {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the control token file is readable by other users"));
    }
    let mut raw = String::new();
    file.take(1024).read_to_string(&mut raw)?;
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
    Ok(hex(&bytes))
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

    /// A proof checks out only for the token and nonce it was made with, and is neither the token
    /// nor readable as one.
    #[test]
    fn a_server_proof_holds_only_for_its_token_and_nonce() {
        let (token, nonce) = (random_hex().unwrap(), new_nonce().unwrap());
        let proof = server_proof(&token, &nonce);
        assert!(verify_server_proof(&token, &nonce, &proof));
        assert!(!verify_server_proof(&random_hex().unwrap(), &nonce, &proof), "another token");
        assert!(!verify_server_proof(&token, &new_nonce().unwrap(), &proof), "another nonce");
        assert!(!matches(&token, &proof));
        let mut flipped = proof.clone().into_bytes();
        flipped[0] = if flipped[0] == b'0' { b'1' } else { b'0' };
        for bad in [String::from_utf8(flipped).unwrap(), proof[1..].to_string(), format!("{}zz", &proof[2..]), String::new()] {
            assert!(!verify_server_proof(&token, &nonce, &bad), "{bad:?}");
        }
    }

    #[test]
    fn nonces_are_hex_of_a_bounded_length() {
        let nonce = new_nonce().unwrap();
        assert!(valid_nonce(&nonce));
        for bad in ["", "abc", &"0".repeat(*NONCE_DIGITS.end() + 1), &format!("{}g", &nonce[1..])] {
            assert!(!valid_nonce(bad), "{bad:?}");
        }
    }

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

    /// A link, or a file other users can read, is not accepted as the token.
    #[cfg(unix)]
    #[test]
    fn read_refuses_links_and_shared_files() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("cadcraft-token-read-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let secret = dir.join("secret");
        issue_at(&secret, b"abc").unwrap();
        assert_eq!(read_at(&secret).unwrap(), "abc");
        let link = dir.join("link");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        assert!(read_at(&link).is_err());
        std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_at(&secret).is_err());
        std::fs::remove_dir_all(dir).unwrap();
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
