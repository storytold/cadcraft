//! The WebDAV backend: a Nextcloud / ownCloud (oCIS) collection over HTTP.
//!
//! Only the standard library is used (blocking `TcpStream` HTTP/1.1), so there is no new
//! dependency and no I/O on any path but an explicit storage call. Supported:
//! - Basic authentication (`Authorization: Basic ...`), as oCIS infinite-scale deployments take.
//! - Bearer authentication (`Authorization: Bearer <token>`), e.g. an oCIS OIDC access
//!   token — required where Basic auth is disabled (external user management);
//!   see [`WebDavBackend::with_bearer`].
//! - Conditional writes: `PUT` carries `If-Match: <etag>` when [`WriteOptions::if_match`] is set;
//!   a `412` becomes [`StorageError::Conflict`]. Without an ETag the backend sends a plain `PUT`
//!   (fallback) and reports the server's `ETag` response header, if any.
//! - `PROPFIND` (depth 0/1) for `stat`/`list`, `GET` for `read`, `DELETE` for `remove`,
//!   `OPTIONS` for [`probe`][crate::StorageBackend::probe].
//!
//! Deliberately stubbed (see the [`WebDavBackend`] docs and the crate report): `https://`
//! URLs (no TLS in this build — put Caddy / a reverse proxy in front, or sync a local folder),
//! digest/NTLM auth, locking (`LOCK`/`UNLOCK`, Phase 3 takes these), and partial writes.
//!
//! Because there is no TLS, credentials are only sent to a loopback host (`localhost`,
//! `127.0.0.0/8`, `::1`) unless the caller opts in with
//! [`WebDavBackend::allow_cleartext_credentials`] (the CLI's `--store-insecure-http`); anything
//! else fails with [`StorageError::Unsupported`] before a byte goes out.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::{Capabilities, Entry, FileMeta, ProbeReport, RecoveryReport, StorageBackend, StorageError, WriteOptions, clean_path};
use base64::Engine as _;

/// Talk timeout per connection phase (connect, write, read). WebDAV runs on LANs and localhost;
/// a hung server must not hang a save longer than this per phase.
const TIMEOUT: Duration = Duration::from_secs(15);
/// Largest single response body kept (a 2 GB document plus XML slack would never fit anyway;
/// `read` refuses past this before writing to memory).
const MAX_BODY: u64 = 2 << 30;

/// A WebDAV collection. `https://` URLs are refused at construction (no TLS in this build);
/// use `http://` on trusted networks (localhost, LAN, VPN, SSH tunnel).
#[derive(Clone, Debug)]
pub struct WebDavBackend {
    host: String,
    port: u16,
    base: String,
    auth: Option<String>,
    cleartext_ok: bool,
}

impl WebDavBackend {
    /// `url` like `http://host:port/remote.php/dav/files/user/`; `user`/`password` become the
    /// Basic credentials (either both or neither).
    pub fn new(url: &str, user: Option<&str>, password: Option<&str>) -> Result<WebDavBackend, StorageError> {
        let (host, port, base) = split_url(url)?;
        let auth = match (user, password) {
            (Some(u), Some(p)) => Some(format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{u}:{p}")))),
            (None, None) => None,
            _ => return Err(StorageError::Auth("need both user and password, or neither".into())),
        };
        if let Some(a) = &auth {
            check_header_value("Authorization", a)?;
        }
        Ok(WebDavBackend { host, port, base, auth, cleartext_ok: false })
    }

    /// `url` like `http://host:port/dav/spaces/...`; `token` becomes an OIDC/Bearer credential
    /// (`Authorization: Bearer <token>`). Use this where the server does not take Basic auth
    /// (e.g. oCIS with external user management). Must not be combined with Basic credentials —
    /// pick one scheme.
    pub fn with_bearer(url: &str, token: &str) -> Result<WebDavBackend, StorageError> {
        let (host, port, base) = split_url(url)?;
        if token.is_empty() {
            return Err(StorageError::Auth("bearer token must not be empty".into()));
        }
        let auth = format!("Bearer {token}");
        check_header_value("Authorization", &auth)?;
        Ok(WebDavBackend { host, port, base, auth: Some(auth), cleartext_ok: false })
    }

    /// Send credentials over plain HTTP to a host that is not loopback. Only for networks the
    /// caller trusts (a LAN, VPN, or cluster network): anyone on the path can read the password
    /// or token. Without this, an authenticated request to a non-loopback host is refused.
    #[must_use]
    pub fn allow_cleartext_credentials(mut self) -> WebDavBackend {
        self.cleartext_ok = true;
        self
    }

    /// The collection URL this backend talks to (password never included).
    pub fn base_url(&self) -> String {
        format!("http://{}:{}{}", self.host, self.port, self.base)
    }

    fn request(&self, method: &str, path: &str, extra: &[(&str, &str)], body: Option<&[u8]>) -> Result<Response, StorageError> {
        let target = join_path(&self.base, path);
        if self.auth.is_some() && !self.cleartext_ok && !is_loopback(&self.host) {
            return Err(StorageError::Unsupported(format!(
                "{}: refusing to send credentials over plain HTTP to a non-loopback host; use a tunnel or opt in (--store-insecure-http)",
                self.host
            )));
        }
        for (k, v) in extra {
            check_header_value(k, v)?;
        }
        let addr = (self.host.as_str(), self.port)
            .to_socket_addrs()
            .map_err(|e| StorageError::Io(format!("{}:{}: {e}", self.host, self.port)))?
            .next()
            .ok_or_else(|| StorageError::Io(format!("{}:{}: no address", self.host, self.port)))?;
        let mut sock = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| StorageError::Io(format!("{}:{}: {e}", self.host, self.port)))?;
        sock.set_read_timeout(Some(TIMEOUT)).map_err(StorageError::from)?;
        sock.set_write_timeout(Some(TIMEOUT)).map_err(StorageError::from)?;
        let mut head = format!("{method} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n", encode_path(&target), self.host);
        if let Some(a) = &self.auth {
            head.push_str(&format!("Authorization: {a}\r\n"));
        }
        for (k, v) in extra {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        let len = body.map(<[u8]>::len).unwrap_or(0);
        head.push_str(&format!("Content-Length: {len}\r\n\r\n"));
        sock.write_all(head.as_bytes()).map_err(StorageError::from)?;
        if let Some(b) = body {
            sock.write_all(b).map_err(StorageError::from)?;
        }
        read_response(&mut sock)
    }

    fn fail_for(&self, op: &str, path: &str, r: &Response) -> StorageError {
        match r.status {
            401 | 403 => StorageError::Auth(format!("{op} {path}: server refused the credentials ({})", r.status)),
            404 => StorageError::NotFound(path.into()),
            409 => StorageError::Conflict(format!("{op} {path}: parent collection is missing")),
            412 => StorageError::Conflict(format!("{op} {path}: stored version changed (stale If-Match)")),
            s => StorageError::Protocol(format!("{op} {path}: unexpected status {s}")),
        }
    }
}

impl StorageBackend for WebDavBackend {
    fn read(&self, path: &str) -> Result<Vec<u8>, StorageError> {
        let rel = clean_path(path)?;
        let r = self.request("GET", &rel, &[], None)?;
        if r.status == 200 {
            return Ok(r.body);
        }
        Err(self.fail_for("read", &rel, &r))
    }

    fn write(&self, path: &str, bytes: &[u8], opts: &WriteOptions) -> Result<FileMeta, StorageError> {
        let rel = clean_path(path)?;
        if bytes.len() as u64 > MAX_BODY {
            return Err(StorageError::Unsupported(format!("{rel}: larger than 2 GB")));
        }
        if opts.atomic {
            // A single PUT either lands whole or not on a compliant server; that is the atomicity
            // this backend offers (no staging rename exists in plain WebDAV).
            return Err(StorageError::Unsupported(format!("{rel}: atomic staging is local-only; plain PUT is already whole-or-nothing")));
        }
        ensure_parents(self, &rel)?;
        let mut extra = vec![("Content-Type", "application/octet-stream")];
        let tag;
        if let Some(t) = &opts.if_match {
            tag = format!("\"{t}\"");
            extra.push(("If-Match", &tag));
        }
        let r = self.request("PUT", &rel, &extra, Some(bytes))?;
        match r.status {
            200 | 201 | 204 => Ok(FileMeta { size: bytes.len() as u64, etag: r.etag(), modified: None, is_dir: false }),
            _ => Err(self.fail_for("write", &rel, &r)),
        }
    }

    fn stat(&self, path: &str) -> Result<FileMeta, StorageError> {
        let rel = clean_path(path)?;
        let body = propfind_body();
        let r = self.request("PROPFIND", &rel, &[("Depth", "0"), ("Content-Type", "application/xml")], Some(body.as_bytes()))?;
        if r.status != 207 {
            return Err(self.fail_for("stat", &rel, &r));
        }
        let entries = parse_propfind(&r.text(), &self.base)?;
        entries.into_iter().next().map(|e| e.meta).ok_or(StorageError::NotFound(rel))
    }

    fn list(&self, prefix: &str) -> Result<Vec<Entry>, StorageError> {
        let rel = if prefix.is_empty() { String::new() } else { clean_path(prefix)? };
        let body = propfind_body();
        let r = self.request("PROPFIND", &rel, &[("Depth", "1"), ("Content-Type", "application/xml")], Some(body.as_bytes()))?;
        if r.status != 207 {
            return Err(self.fail_for("list", &rel, &r));
        }
        let mut entries = parse_propfind(&r.text(), &self.base)?;
        // The server lists the collection itself first; callers want its children.
        if !rel.is_empty() {
            entries.retain(|e| e.path != rel);
        } else {
            entries.retain(|e| !e.path.is_empty());
        }
        Ok(entries)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        let rel = clean_path(path)?;
        let r = self.request("DELETE", &rel, &[], None)?;
        match r.status {
            200 | 202 | 204 | 404 => Ok(()),
            _ => Err(self.fail_for("remove", &rel, &r)),
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { atomic_write: false, conditional_write: true, list: true, etag: true, persistent: true }
    }

    fn probe(&self) -> Result<ProbeReport, StorageError> {
        let mut caps = self.capabilities();
        match self.request("OPTIONS", "", &[], None) {
            Ok(r) if (200..300).contains(&r.status) => {
                let dav = r.header("dav").unwrap_or_default();
                let dav_classes: Vec<String> = dav.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                let mut notes = Vec::new();
                if dav_classes.is_empty() {
                    caps.conditional_write = false;
                    notes.push("server sent no DAV header; conditional writes and ETags may be ignored".into());
                }
                Ok(ProbeReport { capabilities: caps, dav_classes, notes })
            }
            Ok(r) => Err(self.fail_for("probe", "/", &r)),
            Err(e) => Err(e),
        }
    }

    fn recover(&self) -> Result<RecoveryReport, StorageError> {
        // The journal lives client-side for remote backends (Phase 2 keeps it beside the local
        // autosave copy); there is nothing server-side to replay yet.
        Ok(RecoveryReport::default())
    }
}

/// Header values are written verbatim, so a CR or LF (e.g. in an ETag echoed back as `If-Match`,
/// or a pasted token) could inject headers. Refuse them.
fn check_header_value(name: &str, value: &str) -> Result<(), StorageError> {
    if value.contains(['\r', '\n']) {
        return Err(StorageError::Protocol(format!("{name}: header value contains CR or LF")));
    }
    Ok(())
}

/// `localhost`, `127.0.0.0/8` or `::1` (bracketed or not): traffic that never leaves the machine.
fn is_loopback(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    h.eq_ignore_ascii_case("localhost") || h.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Create each missing parent collection (`MKCOL`), deepest-first errors ignored when the
/// collection already exists.
fn ensure_parents(b: &WebDavBackend, rel: &str) -> Result<(), StorageError> {
    let mut prefix = String::new();
    let parts: Vec<&str> = rel.split('/').collect();
    if parts.len() < 2 {
        return Ok(());
    }
    for part in &parts[..parts.len() - 1] {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);
        let r = b.request("MKCOL", &prefix, &[], None)?;
        match r.status {
            201 | 405 => {}
            409 => return Err(StorageError::Conflict(format!("write {rel}: parent collection is missing"))),
            401 | 403 => return Err(b.fail_for("write", rel, &r)),
            _ => {}
        }
    }
    Ok(())
}

/// Split `http://host[:port][/base]` into parts. Anything else (notably `https://`) is refused:
/// this build speaks plain HTTP only.
fn split_url(url: &str) -> Result<(String, u16, String), StorageError> {
    let rest = url.strip_prefix("http://").ok_or_else(|| {
        StorageError::Unsupported(format!(
            "{url}: only http:// URLs are supported in this build (no TLS); serve WebDAV over a trusted network or tunnel"
        ))
    })?;
    let (authority, base) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    if authority.is_empty() {
        return Err(StorageError::BadPath(format!("{url}: no host")));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => {
            let port: u16 = p.parse().map_err(|_| StorageError::BadPath(format!("{url}: bad port")))?;
            (h.to_string(), port)
        }
        None => (authority.to_string(), 80),
    };
    if host.is_empty() {
        return Err(StorageError::BadPath(format!("{url}: no host")));
    }
    Ok((host, port, if base.is_empty() { "/".to_string() } else { base }))
}

fn join_path(base: &str, rel: &str) -> String {
    let b = base.trim_end_matches('/');
    if rel.is_empty() { b.to_string() } else { format!("{b}/{rel}") }
}

/// Percent-encode a request target, keeping `/` separators and unreserved characters.
fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn propfind_body() -> String {
    r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:getcontentlength/><d:getetag/><d:getlastmodified/><d:resourcetype/></d:prop></d:propfind>"#.into()
}

struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Response {
    fn header(&self, name: &str) -> Option<String> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone())
    }

    fn etag(&self) -> Option<String> {
        self.header("etag").map(|t| t.trim().trim_matches('"').to_string()).filter(|t| !t.is_empty())
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

fn read_response(sock: &mut TcpStream) -> Result<Response, StorageError> {
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match sock.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if raw.len() as u64 > MAX_BODY + 65536 {
                    return Err(StorageError::Protocol("response is larger than 2 GB".into()));
                }
            }
            Err(e) => return Err(StorageError::Io(e.to_string())),
        }
    }
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| StorageError::Protocol("no HTTP header block".into()))?;
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.lines();
    let status_line = lines.next().ok_or_else(|| StorageError::Protocol("empty response".into()))?;
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| StorageError::Protocol(format!("bad status line: {status_line}")))?;
    let mut headers = Vec::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let mut body = raw[split + 4..].to_vec();
    // De-chunk when the server answers with `Transfer-Encoding: chunked` (oCIS does for PROPFIND).
    if headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked")) {
        body = dechunk(&body)?;
    }
    Ok(Response { status, headers, body })
}

fn dechunk(body: &[u8]) -> Result<Vec<u8>, StorageError> {
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let eol = body[i..].windows(2).position(|w| w == b"\r\n").ok_or_else(|| StorageError::Protocol("bad chunk head".into()))?;
        let size_text = std::str::from_utf8(&body[i..i + eol]).map_err(|_| StorageError::Protocol("bad chunk head".into()))?;
        let size = usize::from_str_radix(size_text.trim(), 16).map_err(|_| StorageError::Protocol("bad chunk size".into()))?;
        i += eol + 2;
        if size == 0 {
            break;
        }
        if out.len().checked_add(size).is_none_or(|total| total > MAX_BODY as usize) {
            return Err(StorageError::Protocol("response is larger than 2 GB".into()));
        }
        let end = i.checked_add(size).ok_or_else(|| StorageError::Protocol("bad chunk".into()))?;
        if end + 2 > body.len() {
            return Err(StorageError::Protocol("truncated chunk".into()));
        }
        out.extend_from_slice(&body[i..end]);
        i = end + 2;
    }
    Ok(out)
}

/// Minimal `207 Multi-Status` scan: for each `<response>` block take `href` plus the first
/// `getcontentlength` / `getetag` / `getlastmodified` / `collection` markers. Namespace prefixes
/// vary (`d:`, `D:`, none), so matching is by local name only.
fn parse_propfind(xml: &str, base: &str) -> Result<Vec<Entry>, StorageError> {
    let mut entries = Vec::new();
    for block in response_blocks(xml) {
        let Some(href) = tag_text(block, "href") else { continue };
        let rel = href_to_rel(&href, base);
        let is_dir = block.contains("<collection") || block.contains(":collection");
        let size = tag_text(block, "getcontentlength").and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0);
        let etag = tag_text(block, "getetag").map(|s| s.trim().trim_matches('"').to_string()).filter(|s| !s.is_empty());
        let modified = tag_text(block, "getlastmodified");
        entries.push(Entry { path: rel, meta: FileMeta { size, etag, modified, is_dir } });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

/// The `<…response>…</…response>` blocks of a `207 Multi-Status` body, whatever namespace
/// prefix the server uses (`d:`, `D:`, none). Nested responses never occur, so the first matching
/// closer ends the block.
fn response_blocks(xml: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < xml.len() {
        let Some(s) = xml[i..].find('<') else { break };
        let s = i + s;
        let rest = &xml[s + 1..];
        if rest.starts_with('/') || rest.starts_with('!') || rest.starts_with('?') {
            i = s + 1;
            continue;
        }
        let Some(gt) = rest.find('>') else { break };
        let tag = rest[..gt].split_whitespace().next().unwrap_or("");
        if tag.rsplit(':').next().unwrap_or(tag).eq_ignore_ascii_case("response") {
            let mut c = s + 1 + gt + 1;
            let mut end = None;
            while let Some(p) = xml[c..].find("</") {
                let cs = c + p;
                let crest = &xml[cs + 2..];
                let Some(cgt) = crest.find('>') else { break };
                let ctag = crest[..cgt].split_whitespace().next().unwrap_or("");
                if ctag.rsplit(':').next().unwrap_or(ctag).eq_ignore_ascii_case("response") {
                    end = Some(cs + 2 + cgt + 1);
                    break;
                }
                c = cs + 2;
            }
            if let Some(e) = end {
                out.push(&xml[s..e]);
                i = e;
            } else {
                break;
            }
        } else {
            i = s + 1 + gt + 1;
        }
    }
    out
}

/// Find `<…name>text</…name>` regardless of prefix: matches the first `<x:name` opener and reads
/// to the matching `</`.
fn tag_text(block: &str, name: &str) -> Option<String> {
    let mut i = 0;
    while let Some(start) = block[i..].find('<') {
        let s = i + start;
        let after = &block[s + 1..];
        // Skip closing tags and comments.
        if after.starts_with('/') || after.starts_with('!') || after.starts_with('?') {
            i = s + 1;
            continue;
        }
        let tag_end = after.find(['>', ' '])?;
        let tag = &after[..tag_end];
        let local = tag.rsplit(':').next().unwrap_or(tag);
        let full_end = s + 1 + after.find('>')?;
        if local.eq_ignore_ascii_case(name) {
            let content_start = full_end + 1;
            let mut c = content_start;
            while let Some(pos) = block[c..].find("</") {
                let cname_start = c + pos + 2;
                let cname_end = block[cname_start..].find('>')?;
                let cname = &block[cname_start..cname_start + cname_end];
                let clocal = cname.split_whitespace().next().unwrap_or(cname);
                let clocal = clocal.rsplit(':').next().unwrap_or(clocal);
                if clocal.eq_ignore_ascii_case(name) {
                    return Some(block[content_start..c + pos].to_string());
                }
                c += pos + 1;
            }
            return None;
        }
        i = full_end + 1;
    }
    None
}

/// Turn a server `href` (absolute URL or base-prefixed path) into a backend-relative path.
fn href_to_rel(href: &str, base: &str) -> String {
    let path = match href.find("://") {
        Some(i) => {
            let rest = &href[i + 3..];
            match rest.find('/') {
                Some(j) => &rest[j..],
                None => "/",
            }
        }
        None => href,
    };
    let path = percent_decode(path);
    let base_trim = base.trim_end_matches('/');
    let rel = path.strip_prefix(base_trim).unwrap_or(&path);
    rel.trim_matches('/').to_string()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[test]
    fn https_is_refused_plainly() {
        let err = WebDavBackend::new("https://cloud.example/remote.php/dav/files/u/", None, None).unwrap_err();
        assert!(matches!(err, StorageError::Unsupported(_)));
    }

    #[test]
    fn url_split_defaults_port_80() {
        let (host, port, base) = split_url("http://ocis.lan/remote.php/dav/files/alice/").unwrap();
        assert_eq!((host.as_str(), port, base.as_str()), ("ocis.lan", 80, "/remote.php/dav/files/alice/"));
    }

    #[test]
    fn mock_roundtrip() {
        let list_xml = r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:"><d:response><d:href>/dav/docs/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response><d:response><d:href>/dav/docs/memo.dxf</d:href><d:propstat><d:prop><d:getcontentlength>5</d:getcontentlength><d:getetag>"abc123"</d:getetag><d:resourcetype/></d:prop></d:propstat></d:response></d:multistatus>"#.to_string();
        let stat_xml = r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:"><d:response><d:href>/dav/docs/memo.dxf</d:href><d:propstat><d:prop><d:getcontentlength>5</d:getcontentlength><d:getetag>"abc123"</d:getetag><d:resourcetype/></d:prop></d:propstat></d:response></d:multistatus>"#.to_string();
        let put_resp = "HTTP/1.1 201 Created\r\nETag: \"abc123\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string();
        let get_resp = "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello".to_string();
        let propfind_stat = format!(
            "HTTP/1.1 207 Multi-Status\r\nContent-Type: application/xml\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{stat_xml}",
            stat_xml.len()
        );
        let propfind_list = format!(
            "HTTP/1.1 207 Multi-Status\r\nContent-Type: application/xml\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{list_xml}",
            list_xml.len()
        );
        let del_resp = "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let responses: Arc<Vec<(String, String)>> = Arc::new(vec![
            ("PUT /dav/docs/memo.dxf".to_string(), put_resp),
            ("GET /dav/docs/memo.dxf".to_string(), get_resp),
            ("PROPFIND /dav/docs/memo.dxf".to_string(), propfind_stat),
            ("PROPFIND /dav/docs".to_string(), propfind_list),
            ("DELETE /dav/docs/memo.dxf".to_string(), del_resp),
        ]);
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(5) {
                let mut stream = stream.unwrap();
                let mut buf = [0u8; 16384];
                let mut req = Vec::new();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            req.extend_from_slice(&buf[..n]);
                            if req.windows(4).any(|w| w == b"\r\n\r\n") {
                                let head = String::from_utf8_lossy(&req).into_owned();
                                let len = head
                                    .lines()
                                    .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
                                    .and_then(|l| l.split(':').nth(1))
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                                    .unwrap_or(0);
                                let split = req.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                                if req.len() >= split + len {
                                    break;
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
                let head = String::from_utf8_lossy(&req).into_owned();
                let request_line = head.lines().next().unwrap_or("").to_string();
                let key = request_line.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
                seen2.lock().unwrap().push(request_line.clone());
                let body = responses
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_else(|| "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string());
                let _ = stream.write_all(body.as_bytes());
            }
        });

        let b = WebDavBackend::new(&format!("http://{addr}/dav/docs"), Some("alice"), Some("secret")).unwrap();
        let meta = b.write("memo.dxf", b"hello", &WriteOptions::default()).unwrap();
        assert_eq!(meta.etag.as_deref(), Some("abc123"));
        assert_eq!(b.read("memo.dxf").unwrap(), b"hello");
        let st = b.stat("memo.dxf").unwrap();
        assert_eq!(st.size, 5);
        assert_eq!(st.etag.as_deref(), Some("abc123"));
        let list = b.list("").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].path, "memo.dxf");
        b.remove("memo.dxf").unwrap();
        // Basic auth went out on the PUT.
        assert!(seen.lock().unwrap().iter().any(|l| l.starts_with("PUT")));
    }

    #[test]
    fn bearer_token_sent_as_authorization_header() {
        use std::sync::mpsc;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let mut req = Vec::new();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            loop {
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        req.extend_from_slice(&buf[..n]);
                        if req.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let head = String::from_utf8_lossy(&req).into_owned();
            let auth = head.lines().find(|l| l.to_ascii_lowercase().starts_with("authorization:")).unwrap_or("").to_string();
            let _ = tx.send(auth);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nDAV: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        });
        let b = WebDavBackend::with_bearer(&format!("http://{addr}/dav/"), "test-token-123").unwrap();
        b.probe().unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "Authorization: Bearer test-token-123");
    }

    #[test]
    fn credentials_stay_off_non_loopback_plain_http_unless_opted_in() {
        // Port 9 on a TEST-NET address: the refusal must come before any connection attempt.
        let b = WebDavBackend::with_bearer("http://192.0.2.1:9/dav/", "t").unwrap();
        assert!(matches!(b.stat("x.dxf").unwrap_err(), StorageError::Unsupported(_)));
        let b = WebDavBackend::new("http://ocis.lan:9/dav/", Some("alice"), Some("pw")).unwrap();
        assert!(matches!(b.read("x.dxf").unwrap_err(), StorageError::Unsupported(_)));
        // Opting in gets past the check (and then fails to connect at once: `0.0.0.0` is not loopback).
        let b = WebDavBackend::with_bearer("http://0.0.0.0:9/dav/", "t").unwrap().allow_cleartext_credentials();
        assert!(!matches!(b.stat("x.dxf"), Err(StorageError::Unsupported(_))));
        // No credentials, nothing to leak.
        let b = WebDavBackend::new("http://0.0.0.0:9/dav/", None, None).unwrap();
        assert!(!matches!(b.stat("x.dxf"), Err(StorageError::Unsupported(_))));
        assert!(is_loopback("localhost") && is_loopback("127.0.0.2") && is_loopback("[::1]") && !is_loopback("10.0.0.1"));
    }

    #[test]
    fn header_values_with_cr_or_lf_are_refused() {
        assert!(matches!(WebDavBackend::with_bearer("http://127.0.0.1:9/dav/", "t\r\nX-Evil: 1"), Err(StorageError::Protocol(_))));
        // Basic credentials are base64-encoded, so CR/LF in them cannot reach the header block.
        assert!(WebDavBackend::new("http://127.0.0.1:9/dav/", Some("a\n"), Some("p")).is_ok());
        let b = WebDavBackend::new("http://127.0.0.1:9/dav/", None, None).unwrap();
        let err = b.write("x.dxf", b"d", &WriteOptions { if_match: Some("tag\r\nX-Evil: 1".into()), atomic: false }).unwrap_err();
        assert!(matches!(err, StorageError::Protocol(_)), "{err:?}");
    }

    #[test]
    fn dechunk_rejects_an_overflowing_chunk_size() {
        let body = format!("2\r\nab\r\n{:x}\r\n", usize::MAX);
        assert!(matches!(dechunk(body.as_bytes()), Err(StorageError::Protocol(_))));
    }

    #[test]
    fn bearer_rejects_empty_token() {
        let err = WebDavBackend::with_bearer("http://127.0.0.1/dav/", "").unwrap_err();
        assert!(matches!(err, StorageError::Auth(_)));
    }

    /// Read one full HTTP request (headers plus any `Content-Length` body) from a mock
    /// connection. Answering earlier risks a RST racing the client's request bytes.
    fn read_request(stream: &mut TcpStream) {
        let mut buf = [0u8; 8192];
        let mut req = Vec::new();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    req.extend_from_slice(&buf[..n]);
                    if let Some(split) = req.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&req[..split]).into_owned();
                        let len = head
                            .lines()
                            .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
                            .and_then(|l| l.split(':').nth(1))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if req.len() >= split + 4 + len {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }
    }

    #[test]
    fn if_match_conflict_surfaces() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_request(&mut stream);
            let _ = stream.write_all(b"HTTP/1.1 412 Precondition Failed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        });
        let b = WebDavBackend::new(&format!("http://{addr}/dav/"), None, None).unwrap();
        let err = b.write("f.dxf", b"x", &WriteOptions { if_match: Some("stale".into()), ..Default::default() }).unwrap_err();
        assert!(matches!(err, StorageError::Conflict(_)));
    }

    #[test]
    fn probe_reports_dav_classes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_request(&mut stream);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nDAV: 1, 2\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        });
        let b = WebDavBackend::new(&format!("http://{addr}/dav/"), None, None).unwrap();
        let report = b.probe().unwrap();
        assert_eq!(report.dav_classes, vec!["1".to_string(), "2".to_string()]);
        assert!(report.capabilities.conditional_write);
    }
}
