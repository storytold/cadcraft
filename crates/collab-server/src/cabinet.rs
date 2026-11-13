//! The cabinet commit callback (M87, `INTEGRATION.md` §5.2).
//!
//! The room builds the final `.dxf` and posts it to the cabinet, which owns the file version,
//! the audit and the antivirus scan. The body is the cabinet contract:
//!
//! ```json
//! { "fileId", "name", "bytes": "<base64>", "sha256", "version", "expectedVersion"? }
//! ```
//!
//! `version` is the room's drawing version; `expectedVersion` is the cabinet etag the previous
//! commit returned, so a file changed outside the session is refused with `409` instead of
//! overwritten. The cabinet answers `{ "version", "etag" }`, where `etag` is what the next
//! commit must send as `expectedVersion`.
//!
//! The client is deliberately plain: `http://` only, `Connection: close`, one request per
//! attempt. The room is deployed beside the cabinet behind the ingress (RFC §9), so TLS is the
//! proxy's job; an `https://` endpoint is refused with a message that says so rather than
//! failing the handshake. Neither the token nor the document ever reaches the logs.

use std::fmt;
use std::io::{Read as _, Write as _};
use std::net::{TcpStream, ToSocketAddrs as _};
use std::time::Duration;

use cadcraft_collab::asset_id;
use cadcraft_collab::message::encode_bytes;

/// Deadline for one request (connect, write and the whole answer).
pub const DEFAULT_TIMEOUT_MS: u64 = 10_000;
/// Attempts for one commit: a lost answer or a busy cabinet is retried with the same body; a
/// `409`/`401`/`413` is not retried at all.
pub const DEFAULT_ATTEMPTS: u32 = 3;
/// A response head bigger than this is not the cabinet talking.
const MAX_HEAD: usize = 64 * 1024;
/// The answer is a small JSON object; anything larger is refused rather than buffered.
const MAX_ANSWER: usize = 1 << 20;

/// Where and how the cabinet takes a committed file.
#[derive(Clone)]
pub struct CabinetConfig {
    /// The commit endpoint. `{sessionId}` and `{fileId}` are substituted.
    pub url: String,
    /// The service token the callback is authenticated with. Never logged.
    pub token: String,
    /// The cabinet's id for the file this room serves.
    pub file_id: String,
    /// Deadline for one request.
    pub timeout: Duration,
    /// Attempts before a transient failure is reported.
    pub attempts: u32,
}

impl fmt::Debug for CabinetConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CabinetConfig")
            .field("url", &self.url)
            .field("token", &"<redacted>")
            .field("file_id", &self.file_id)
            .field("timeout", &self.timeout)
            .field("attempts", &self.attempts)
            .finish()
    }
}

/// The cabinet's answer to a stored commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Published {
    /// The cabinet's new file version.
    pub version: u64,
    /// The etag the next commit sends as `expectedVersion`.
    pub etag: String,
}

/// Why a commit was not stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishError {
    /// `409`: the file changed outside the session. The room must not overwrite it and offers
    /// the built file as a copy instead (TASK §6).
    Conflict(String),
    /// `401`: the service token is missing or wrong.
    Unauthorized,
    /// `413`: the cabinet refuses a file this large.
    TooLarge,
    /// The cabinet gave an answer this room does not understand.
    Refused(String),
    /// The cabinet could not be reached.
    Transport(String),
}

impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict(message) => write!(f, "the cabinet holds another version of the file: {message}"),
            Self::Unauthorized => f.write_str("the cabinet refused the service token"),
            Self::TooLarge => f.write_str("the cabinet refused the committed file: too large"),
            Self::Refused(message) => write!(f, "the cabinet refused the commit: {message}"),
            Self::Transport(message) => write!(f, "the cabinet did not answer: {message}"),
        }
    }
}

/// A publisher for one room's commits.
pub struct Cabinet {
    config: CabinetConfig,
}

impl Cabinet {
    /// A publisher for the endpoint in `config`.
    pub fn new(config: CabinetConfig) -> Self {
        Self { config }
    }

    /// The file id the cabinet knows this room by.
    pub fn file_id(&self) -> &str {
        &self.config.file_id
    }

    /// Post one built file for `session_id`; `expected` is the etag of the previous commit.
    ///
    /// A transient failure (a closed socket, `408`, `429`, `5xx`) is retried with the same body;
    /// `409`, `401`, `413` and the other `4xx` answers are not.
    pub fn publish(&self, session_id: &str, bytes: &[u8], version: u64, expected: Option<&str>) -> Result<Published, PublishError> {
        let (host, port, path) = parse_url(&self.config.url, session_id, &self.config.file_id)?;
        let sha256 = asset_id(bytes);
        let mut body = serde_json::json!({
            "fileId": self.config.file_id,
            "name": "commit.dxf",
            "bytes": encode_bytes(bytes),
            "sha256": sha256,
            "version": version,
        });
        if let Some(expected) = expected {
            body["expectedVersion"] = serde_json::Value::String(expected.to_string());
        }
        let body = serde_json::to_vec(&body).map_err(|e| PublishError::Refused(e.to_string()))?;
        let attempts = self.config.attempts.max(1);
        let mut last = PublishError::Transport("no attempt was made".into());
        for attempt in 0..attempts {
            match self.attempt(&host, port, &path, &body) {
                Ok(published) => return Ok(published),
                Err(PublishError::Transport(message)) if attempt + 1 < attempts => last = PublishError::Transport(message),
                Err(error) => return Err(error),
            }
        }
        Err(last)
    }

    fn attempt(&self, host: &str, port: u16, path: &str, body: &[u8]) -> Result<Published, PublishError> {
        let address = (host, port)
            .to_socket_addrs()
            .map_err(|e| PublishError::Transport(e.to_string()))?
            .next()
            .ok_or_else(|| PublishError::Transport(format!("{host}:{port} does not resolve")))?;
        let mut stream = TcpStream::connect_timeout(&address, self.config.timeout).map_err(|e| PublishError::Transport(e.to_string()))?;
        stream.set_read_timeout(Some(self.config.timeout)).map_err(|e| PublishError::Transport(e.to_string()))?;
        stream.set_write_timeout(Some(self.config.timeout)).map_err(|e| PublishError::Transport(e.to_string()))?;
        let head = format!(
            "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n",
            token = self.config.token,
            len = body.len(),
        );
        stream.write_all(head.as_bytes()).map_err(|e| PublishError::Transport(e.to_string()))?;
        stream.write_all(body).map_err(|e| PublishError::Transport(e.to_string()))?;
        let answer = read_answer(&mut stream)?;
        classify(&answer)
    }
}

/// Split an `http://host[:port]/path` URL, substituting `sessionId` and `fileId`.
fn parse_url(url: &str, session_id: &str, file_id: &str) -> Result<(String, u16, String), PublishError> {
    let Some(rest) = url.strip_prefix("http://") else {
        return Err(PublishError::Refused("the cabinet URL must start with http:// (TLS belongs to the proxy)".into()));
    };
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, "/".to_string()),
    };
    if authority.is_empty() || authority.contains('@') {
        return Err(PublishError::Refused("the cabinet URL has no usable host".into()));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => {
            (host.to_string(), port.parse::<u16>().map_err(|_| PublishError::Refused("the cabinet URL has a bad port".into()))?)
        }
        _ => (authority.to_string(), 80),
    };
    let path = path.replace("{sessionId}", session_id).replace("{fileId}", file_id);
    Ok((host, port, path))
}

/// The status line and body of one answer.
struct Answer {
    status: u16,
    body: String,
}

fn read_answer(stream: &mut TcpStream) -> Result<Answer, PublishError> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = stream.read(&mut chunk).map_err(|e| PublishError::Transport(e.to_string()))?;
        if read == 0 {
            break;
        }
        if buffer.len() + read > MAX_HEAD + MAX_ANSWER {
            return Err(PublishError::Refused("the cabinet answer is too large".into()));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(head) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let head_end = head + 4;
            if buffer.len() >= head_end
                && let Some(length) = content_length(&buffer[..head])
                && buffer.len() >= head_end + length
            {
                break;
            }
        }
    }
    let Some(head_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") else {
        return Err(PublishError::Transport("the cabinet closed without an answer".into()));
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).to_string();
    let body = String::from_utf8_lossy(&buffer[head_end + 4..]).to_string();
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| PublishError::Transport("the cabinet answer has no HTTP status".into()))?;
    Ok(Answer { status, body })
}

/// The `Content-Length` of a response head.
fn content_length(head: &[u8]) -> Option<usize> {
    let text = String::from_utf8_lossy(head);
    text.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.eq_ignore_ascii_case("content-length") { value.trim().parse::<usize>().ok() } else { None }
    })
}

/// Map a cabinet answer to the room's result.
fn classify(answer: &Answer) -> Result<Published, PublishError> {
    match answer.status {
        200..=299 => {
            let value: serde_json::Value = serde_json::from_str(&answer.body).map_err(|e| PublishError::Refused(e.to_string()))?;
            let version = value.get("version").and_then(|v| v.as_u64()).ok_or_else(|| PublishError::Refused("the answer has no version".into()))?;
            let etag = value.get("etag").and_then(|v| v.as_str()).ok_or_else(|| PublishError::Refused("the answer has no etag".into()))?;
            Ok(Published { version, etag: etag.to_string() })
        }
        401 | 403 => Err(PublishError::Unauthorized),
        409 => Err(PublishError::Conflict(answer.body.chars().take(200).collect())),
        413 => Err(PublishError::TooLarge),
        408 | 429 | 500..=599 => Err(PublishError::Transport(format!("HTTP {}", answer.status))),
        other => Err(PublishError::Refused(format!("HTTP {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    /// A loopback cabinet that answers each request with the next canned answer.
    fn fake_cabinet(answers: Vec<&'static str>) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a listener");
        let addr = listener.local_addr().expect("an address");
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for answer in answers {
                let Ok((mut stream, _)) = listener.accept() else { return };
                let mut request = Vec::new();
                let mut chunk = [0u8; 4096];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let Ok(read) = stream.read(&mut chunk) else { return };
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..read]);
                }
                let head_end = request.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4).unwrap_or(request.len());
                let length = content_length(&request[..head_end]).unwrap_or(0);
                while request.len() < head_end + length {
                    let Ok(read) = stream.read(&mut chunk) else { return };
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..read]);
                }
                let _ = tx.send(String::from_utf8_lossy(&request).to_string());
                let _ = stream.write_all(answer.as_bytes());
            }
        });
        (format!("http://{addr}"), rx)
    }

    fn a_cabinet(url: String) -> Cabinet {
        Cabinet::new(CabinetConfig {
            url: format!("{url}/api/v1/internal/office/sessions/{{sessionId}}/commit"),
            token: "service-token".into(),
            file_id: "42".into(),
            timeout: Duration::from_secs(2),
            attempts: 3,
        })
    }

    #[test]
    fn a_stored_commit_returns_the_cabinet_version_and_etag() {
        let (url, requests) = fake_cabinet(vec!["HTTP/1.1 200 OK\r\nContent-Length: 30\r\n\r\n{\"version\":7,\"etag\":\"abc\"}"]);
        let published = a_cabinet(url).publish("s1", b"drawing", 3, Some("old")).expect("stored");
        assert_eq!(published, Published { version: 7, etag: "abc".into() });
        let request = requests.recv().expect("one request");
        assert!(request.starts_with("POST /api/v1/internal/office/sessions/s1/commit HTTP/1.1\r\n"), "{request}");
        assert!(request.contains("Authorization: Bearer service-token\r\n"), "{request}");
        assert!(request.contains("\"expectedVersion\":\"old\""), "{request}");
        assert!(request.contains("\"version\":3"), "{request}");
        assert!(request.contains("\"fileId\":\"42\""), "{request}");
    }

    #[test]
    fn a_409_is_a_conflict_and_is_not_retried() {
        let (url, requests) = fake_cabinet(vec!["HTTP/1.1 409 Conflict\r\nContent-Length: 2\r\n\r\n{}"]);
        let error = a_cabinet(url).publish("s1", b"drawing", 3, Some("old")).expect_err("conflict");
        assert!(matches!(error, PublishError::Conflict(_)), "{error:?}");
        let _ = requests.recv().expect("one request");
        assert!(requests.try_recv().is_err(), "409 gets exactly one attempt");
    }

    #[test]
    fn an_unauthorized_and_a_too_large_commit_are_not_retried() {
        for (answer, expected) in [
            ("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n", PublishError::Unauthorized),
            ("HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\n\r\n", PublishError::TooLarge),
        ] {
            let (url, requests) = fake_cabinet(vec![answer]);
            let error = a_cabinet(url).publish("s1", b"drawing", 3, None).expect_err("refused");
            assert_eq!(error, expected, "{answer}");
            let _ = requests.recv().expect("one request");
            assert!(requests.try_recv().is_err(), "a hard refusal gets exactly one attempt");
        }
    }

    #[test]
    fn a_transient_failure_is_retried_with_the_same_body() {
        let (url, requests) = fake_cabinet(vec![
            "HTTP/1.1 503 Busy\r\nContent-Length: 0\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 30\r\n\r\n{\"version\":7,\"etag\":\"abc\"}",
        ]);
        let published = a_cabinet(url).publish("s1", b"drawing", 3, None).expect("stored after one retry");
        assert_eq!(published.version, 7);
        let first = requests.recv().expect("first request");
        let second = requests.recv().expect("second request");
        assert_eq!(first, second, "the retry sends the same body");
    }

    #[test]
    fn an_https_endpoint_is_refused_before_a_connection() {
        let cabinet = Cabinet::new(CabinetConfig {
            url: "https://cabinet.example/commit".into(),
            token: "t".into(),
            file_id: "1".into(),
            timeout: Duration::from_secs(1),
            attempts: 1,
        });
        assert!(matches!(cabinet.publish("s1", b"d", 1, None), Err(PublishError::Refused(_))));
    }
}
