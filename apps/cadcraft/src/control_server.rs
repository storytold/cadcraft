//! Loopback JSON-lines control server: one request per line, one reply per line.
//! This is the transport the MCP server (`cadcraft-cli mcp --connect`) wraps.
//!
//! The server only reads requests: a line that is not a JSON object with a string `method` (an
//! HTTP request line, a stray `nc`, binary data) gets one error reply and the connection is
//! closed, so nothing sent after it runs. Every resource a client can hold is bounded
//! ([`Limits`]): simultaneous connections, request line length, queued requests, and how long a
//! socket may sit idle or blocked on a write.

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::time::Duration;

use cadcraft_ui_egui::ControlRequest;
use serde_json::{Value, json};

/// Resource bounds for the control server.
#[derive(Clone, Copy, Debug)]
struct Limits {
    /// Connections served at once; more get an error line and are closed.
    max_clients: usize,
    /// Longest request line in bytes (newline excluded); a longer one closes the connection.
    max_line: usize,
    /// Request-line bytes buffered at once across all connections; a request that would pass it
    /// is refused with an error reply.
    total_bytes: usize,
    /// Requests waiting for the app at once; more are answered `busy`.
    queue: usize,
    /// How long a connection may go without sending a byte before it is closed.
    idle_timeout: Duration,
    /// How long a reply write may block before the connection is closed.
    write_timeout: Duration,
    /// How long a request waits for the app's answer.
    reply_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_clients: 16,
            // Generous, so requests that carry whole drawings or images as base64 still fit.
            max_line: 256 * 1024 * 1024,
            // Room for one maximal request plus smaller ones, not `max_clients` maximal ones.
            total_bytes: 512 * 1024 * 1024,
            queue: 64,
            idle_timeout: Duration::from_secs(600),
            write_timeout: Duration::from_secs(30),
            reply_timeout: Duration::from_secs(60),
        }
    }
}

/// Start the loopback server with a fresh session token and a bounded request queue.
/// Binding or token-issuance failure returns a disconnected receiver.
pub fn start(port: u16, ctx: egui::Context) -> Receiver<ControlRequest> {
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => {
            // Loopback TCP cannot tell local users apart, so issue an unguessable per-session
            // token in an owner-only file and require it on every request (CWE-306). A client
            // (the MCP bridge) reads it from the file, which needs the desktop user's filesystem
            // access; another local account cannot.
            let token = match cadcraft_mcp::control_token::issue(port) {
                Ok(t) => Arc::new(t),
                Err(e) => {
                    log::error!("control server not started: could not write the control token: {e}");
                    return sync_channel::<ControlRequest>(0).1;
                }
            };
            if let Ok(path) = cadcraft_mcp::control_token::token_path(port) {
                log::info!("control server listening on 127.0.0.1:{port} (token in {})", path.display());
            }
            serve_listener(l, Limits::default(), ctx, token)
        }
        Err(e) => {
            log::error!("control server failed to bind 127.0.0.1:{port}: {e}");
            sync_channel::<ControlRequest>(0).1
        }
    }
}

/// How long the accepting thread waits after a failed accept. Accepting can keep failing, for
/// example while the process has no file descriptors left, and retrying at once would keep a
/// core busy until it recovers.
const ACCEPT_RETRY: Duration = Duration::from_millis(100);

/// The connections `incoming` accepts, calling `wait` after each failed accept.
fn accepted<T>(incoming: impl Iterator<Item = std::io::Result<T>>, mut wait: impl FnMut()) -> impl Iterator<Item = T> {
    incoming.filter_map(move |stream| stream.map_err(|_| wait()).ok())
}

/// Accept connections on `listener` under `limits`; requests arrive on the returned receiver.
fn serve_listener(listener: TcpListener, limits: Limits, ctx: egui::Context, token: Arc<String>) -> Receiver<ControlRequest> {
    let (tx, rx) = sync_channel::<ControlRequest>(limits.queue.max(1));
    std::thread::spawn(move || accept_loop(listener, tx, ctx, limits, token));
    rx
}

/// A connection's place among [`Limits::max_clients`], given back when the connection ends (also
/// when its thread can't be started, or panics).
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Accept clients within the connection limit and give each worker the same session token.
fn accept_loop(listener: TcpListener, tx: SyncSender<ControlRequest>, ctx: egui::Context, limits: Limits, token: Arc<String>) {
    let open = Arc::new(AtomicUsize::new(0));
    let budget = Budget::new(limits.total_bytes);
    for mut stream in accepted(listener.incoming(), || std::thread::sleep(ACCEPT_RETRY)) {
        let _ = stream.set_write_timeout(Some(limits.write_timeout));
        if open.fetch_add(1, Ordering::SeqCst) >= limits.max_clients {
            open.fetch_sub(1, Ordering::SeqCst);
            let _ = writeln!(stream, "{}", json!({"ok": false, "error": "too many control connections"}));
            continue;
        }
        let slot = Slot(Arc::clone(&open));
        let tx = tx.clone();
        let ctx = ctx.clone();
        let token = Arc::clone(&token);
        let budget = budget.clone();
        let _ = std::thread::Builder::new().spawn(move || {
            let _slot = slot;
            serve(stream, &tx, &ctx, limits, &token, budget);
        });
    }
}

/// A total-bytes budget for request lines buffered across all connections, so
/// [`Limits::max_clients`] peers can't each hold a [`Limits::max_line`] line at once (#1925).
#[derive(Clone, Debug)]
struct Budget {
    used: Arc<AtomicUsize>,
    total: usize,
}

impl Budget {
    fn new(total: usize) -> Self {
        Self { used: Arc::new(AtomicUsize::new(0)), total }
    }

    /// Reserve `n` more bytes, or `false` (nothing reserved) when that would pass the total.
    fn reserve(&self, n: usize) -> bool {
        self.used.try_update(Ordering::SeqCst, Ordering::SeqCst, |u| u.checked_add(n).filter(|&t| t <= self.total)).is_ok()
    }
}

/// Bytes a connection holds from the [`Budget`] for its current request line; given back when the
/// next line starts or the connection ends.
struct Held {
    budget: Budget,
    bytes: usize,
}

impl Held {
    fn new(budget: Budget) -> Self {
        Self { budget, bytes: 0 }
    }

    fn release(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::SeqCst);
        self.bytes = 0;
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.release();
    }
}

/// Read one `\n`-terminated line of at most `max` bytes into `buf` (newline stripped), charging
/// the bytes buffered to `held`. `Ok(false)` at end of stream; `Err` when the line is too long,
/// the shared budget is used up, or the read fails.
fn read_bounded_line(reader: &mut impl BufRead, buf: &mut Vec<u8>, max: usize, held: &mut Held) -> Result<bool, String> {
    buf.clear();
    held.release();
    // A line is at most `max` bytes plus its newline; never buffer more than that.
    let cap = max.saturating_add(1);
    loop {
        let chunk = match reader.fill_buf() {
            Ok(c) => c,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => {
                return Err(match e.kind() {
                    // The idle timeout ran out (Windows reports it as TimedOut, Unix as WouldBlock).
                    ErrorKind::TimedOut | ErrorKind::WouldBlock => "request line not completed in time; closing the connection".to_string(),
                    _ => e.to_string(),
                });
            }
        };
        if chunk.is_empty() {
            // End of stream: nothing read, or a last line without a newline.
            return Ok(!buf.is_empty());
        }
        let line_end = chunk.iter().position(|&b| b == b'\n').map(|i| i + 1);
        let n = line_end.unwrap_or(chunk.len()).min(cap.saturating_sub(buf.len()));
        buf.extend_from_slice(chunk.get(..n).unwrap_or(chunk));
        reader.consume(n);
        // The chunk is already buffered (a few KiB), so it is added before it is charged: a
        // refusal still leaves `buf` non-empty and the connection gets an error reply.
        if !held.budget.reserve(n) {
            return Err("too many large requests in flight; try again later".to_string());
        }
        held.bytes += n;
        if buf.last() == Some(&b'\n') {
            buf.pop();
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            return Ok(true);
        }
        if buf.len() > max {
            return Err(format!("request line longer than {max} bytes; closing the connection"));
        }
    }
}

/// One request line, parsed.
enum Line {
    Request {
        id: Value,
        method: String,
        params: Value,
        token: Option<String>,
    },
    Blank,
    /// Not a request: reply with this error and close the connection.
    Reject(String),
}

/// Parse a UTF-8 JSON request and its optional token, distinguishing blank lines from rejected input.
fn parse_line(bytes: &[u8]) -> Line {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Line::Reject("not a control request (invalid UTF-8); closing the connection".into());
    };
    let text = text.trim();
    if text.is_empty() {
        return Line::Blank;
    }
    if is_http_request_line(text) {
        return Line::Reject("HTTP is not served here: this port takes JSON lines; closing the connection".into());
    }
    let msg = match serde_json::from_str::<Value>(text) {
        Ok(v) => v,
        Err(e) => return Line::Reject(format!("bad JSON ({e}); expected one JSON request object per line; closing the connection")),
    };
    let Some(obj) = msg.as_object() else {
        return Line::Reject("not a control request (expected a JSON object); closing the connection".into());
    };
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        return Line::Reject("not a control request (missing string `method`); closing the connection".into());
    };
    Line::Request {
        id: obj.get("id").cloned().unwrap_or(Value::Null),
        method: method.to_string(),
        params: obj.get("params").cloned().unwrap_or(json!({})),
        token: obj.get("token").and_then(Value::as_str).map(str::to_string),
    }
}

/// An HTTP/1 request line (`POST / HTTP/1.1`) or the HTTP/2 preface (`PRI * HTTP/2.0`).
fn is_http_request_line(text: &str) -> bool {
    const METHODS: [&str; 10] = ["GET", "HEAD", "POST", "PUT", "DELETE", "CONNECT", "OPTIONS", "TRACE", "PATCH", "PRI"];
    text.split_once(' ').is_some_and(|(method, _)| METHODS.contains(&method))
        || text.rsplit_once(' ').is_some_and(|(_, version)| version.starts_with("HTTP/"))
}

/// Serve bounded JSON lines, authenticating each request before enqueueing it for the UI.
/// Invalid credentials close the connection; queue and reply limits report whether a request ran.
fn serve(stream: TcpStream, tx: &SyncSender<ControlRequest>, ctx: &egui::Context, limits: Limits, token: &str, budget: Budget) {
    if stream.set_read_timeout(Some(limits.idle_timeout)).is_err() || stream.set_write_timeout(Some(limits.write_timeout)).is_err() {
        return;
    }
    let Ok(read) = stream.try_clone() else { return };
    let mut out = stream;
    let mut reader = BufReader::new(read);
    let mut buf = Vec::new();
    let mut held = Held::new(budget);
    loop {
        let reject = match read_bounded_line(&mut reader, &mut buf, limits.max_line, &mut held) {
            Ok(false) => break,
            // Nothing of a line arrived (idle timeout, or the peer is gone): close without a word.
            // A line nobody asked for would be read as the reply to the client's next request.
            Err(_) if buf.is_empty() => break,
            Err(e) => e,
            Ok(true) => match parse_line(&buf) {
                Line::Blank => continue,
                Line::Reject(e) => e,
                Line::Request { id, method, params, token: given } => {
                    // Authenticate the peer before dispatching: loopback TCP does not identify the
                    // OS user, so only a client holding the owner-only session token may drive the
                    // desktop user's commands (CWE-306).
                    if !given.map(|g| cadcraft_mcp::control_token::matches(token, &g)).unwrap_or(false) {
                        let mut r = json!({"ok": false, "error": "unauthorized: missing or invalid control token", "state": "not-run"});
                        if let Some(o) = r.as_object_mut() {
                            o.insert("id".into(), id);
                        }
                        let _ = writeln!(out, "{r}");
                        break;
                    }
                    let (req, pending) = ControlRequest::new(method, params);
                    let mut r = match tx.try_send(req) {
                        Ok(()) => {
                            ctx.request_repaint();
                            // On timeout the request is cancelled if the app hasn't started it, and
                            // the reply's `state` says whether it can still have run.
                            pending.wait(limits.reply_timeout)
                        }
                        Err(TrySendError::Full(_)) => json!({"ok": false, "error": "busy: too many queued requests", "state": "not-run"}),
                        Err(TrySendError::Disconnected(_)) => break,
                    };
                    if let Some(o) = r.as_object_mut() {
                        o.insert("id".into(), id);
                    }
                    if writeln!(out, "{r}").is_err() {
                        break;
                    }
                    continue;
                }
            },
        };
        // The connection closes either way; a peer that can't read the reason loses nothing.
        let _ = writeln!(out, "{}", json!({"ok": false, "error": reject}));
        break;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// A failed accept is followed by a wait before the next one, so a listener that keeps
    /// failing does not spin, and the connections around the failures still come through.
    #[test]
    fn a_failed_accept_waits_before_the_next() {
        let failed = || Err(std::io::Error::other("no file descriptors left"));
        let incoming = vec![failed(), Ok(1), failed(), failed(), Ok(2)];
        let mut waits = 0;
        let connections: Vec<i32> = super::accepted(incoming.into_iter(), || waits += 1).collect();
        assert_eq!(connections, [1, 2]);
        assert_eq!(waits, 3, "one wait per failed accept");
    }

    #[test]
    fn lines_are_read_up_to_the_limit() {
        let mut buf = Vec::new();
        let mut input: &[u8] = b"aaaaaaaaaa\nbb\r\ncc";
        assert_eq!(read_bounded_line(&mut input, &mut buf, 10, &mut held()), Ok(true));
        assert_eq!(buf, b"aaaaaaaaaa", "a line exactly at the limit is accepted");
        assert_eq!(read_bounded_line(&mut input, &mut buf, 10, &mut held()), Ok(true));
        assert_eq!(buf, b"bb", "CRLF is stripped");
        assert_eq!(read_bounded_line(&mut input, &mut buf, 10, &mut held()), Ok(true));
        assert_eq!(buf, b"cc", "a last line without a newline is still read");
        assert_eq!(read_bounded_line(&mut input, &mut buf, 10, &mut held()), Ok(false), "then end of stream");
    }

    #[test]
    fn an_overlong_line_is_refused_without_reading_the_rest() {
        let mut buf = Vec::new();
        let long = [b'x'; 100];
        let mut input: &[u8] = &long;
        assert!(read_bounded_line(&mut input, &mut buf, 10, &mut held()).is_err());
        assert!(buf.len() <= 11, "at most limit + 1 bytes are buffered, got {}", buf.len());
        assert_eq!(input.len(), 100 - 11, "the rest of the line is left unread");
    }

    fn held() -> Held {
        Held::new(Budget::new(usize::MAX))
    }

    fn short() -> Limits {
        Limits {
            queue: 1,
            idle_timeout: Duration::from_millis(300),
            write_timeout: Duration::from_millis(300),
            reply_timeout: Duration::from_millis(300),
            ..Limits::default()
        }
    }

    /// A fixed token for tests; real tokens are random (see `cadcraft_mcp::control_token`).
    const TEST_TOKEN: &str = "test-control-token";

    /// The control server on a loopback port under `limits`; requests arrive on the receiver.
    fn serve_with(limits: Limits) -> (std::net::SocketAddr, Receiver<ControlRequest>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        (addr, serve_listener(listener, limits, egui::Context::default(), Arc::new(TEST_TOKEN.to_string())))
    }

    /// The control server on a loopback port, with a stand-in UI that echoes each method.
    fn serve_echo() -> std::net::SocketAddr {
        let (addr, rx) = serve_with(Limits::default());
        std::thread::spawn(move || {
            for req in rx {
                let _ = req.reply.send(json!({"ok": true, "method": req.method}));
            }
        });
        addr
    }

    /// A request line carrying the test token, so it authenticates.
    fn req_line(id: i64, method: &str) -> String {
        json!({"id": id, "method": method, "token": TEST_TOKEN}).to_string() + "\n"
    }

    /// Send `input` on a new connection and read reply lines until the server closes it.
    fn exchange(addr: std::net::SocketAddr, input: &[u8]) -> Vec<Value> {
        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(input).unwrap();
        let mut replies = Vec::new();
        for line in BufReader::new(c).lines() {
            // A read timeout here means the server kept the connection open.
            let line = line.unwrap();
            replies.push(serde_json::from_str(&line).unwrap());
        }
        replies
    }

    /// A browser can POST to the port: its request line closes the connection before the JSON
    /// line in its body is read, so nothing is dispatched.
    #[test]
    fn an_http_request_closes_the_connection_before_its_body_is_dispatched() {
        let (addr, rx) = serve_with(short());
        let http = b"POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: text/plain\r\n\r\n\
            {\"id\":1,\"method\":\"cmdline.input\",\"params\":{\"text\":\"circle 0,0 5\"}}\n";
        let replies = exchange(addr, http);
        assert_eq!(replies.len(), 1, "{replies:?}");
        assert_eq!(replies[0]["ok"], false);
        assert!(replies[0]["error"].as_str().unwrap().contains("closing the connection"), "{replies:?}");
        assert!(rx.try_recv().is_err(), "the body was dispatched");
    }

    #[test]
    fn a_line_that_is_not_a_request_closes_the_connection() {
        for junk in [&b"hello\n"[..], b"[1,2]\n", b"{\"id\":1}\n", b"{\"method\":5}\n", b"\xff\xfe\n", b"GET /x HTTP/1.1\n"] {
            let addr = serve_echo();
            let mut input = junk.to_vec();
            input.extend_from_slice(b"{\"id\":2,\"method\":\"ping\"}\n");
            let replies = exchange(addr, &input);
            assert_eq!(replies.len(), 1, "{replies:?}");
            assert_eq!(replies[0]["ok"], false);
        }
        assert!(is_http_request_line("OPTIONS * HTTP/1.1"));
        assert!(is_http_request_line("PRI * HTTP/2.0"));
        assert!(!is_http_request_line("{\"method\":\"GET /\"}"));
    }

    #[test]
    fn an_idle_connection_is_closed() {
        let (addr, _rx) = serve_with(short());
        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let t = std::time::Instant::now();
        let mut got = Vec::new();
        c.read_to_end(&mut got).unwrap();
        assert!(got.is_empty(), "{}", String::from_utf8_lossy(&got));
        assert!(t.elapsed() < Duration::from_secs(4));
    }

    /// Authenticated requests rejected by a full queue retain their IDs and report that they did not run.
    #[test]
    fn a_full_queue_answers_busy() {
        let (addr, rx) = serve_with(short());
        // No UI drains the one-place queue: the first request fills it, the second finds it full.
        let first = std::thread::spawn(move || exchange(addr, req_line(1, "m").as_bytes()));
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        let busy = loop {
            let mut c = TcpStream::connect(addr).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            c.write_all(req_line(2, "m").as_bytes()).unwrap();
            let mut line = String::new();
            BufReader::new(c).read_line(&mut line).unwrap();
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["error"].as_str().is_some_and(|e| e.starts_with("busy")) || std::time::Instant::now() > deadline {
                break v;
            }
        };
        assert_eq!((busy["id"].as_i64(), busy["state"].as_str()), (Some(2), Some("not-run")), "{busy}");
        assert_eq!(first.join().unwrap()[0]["state"], "not-run");
        drop(rx);
    }

    /// Connections can't together buffer more request-line bytes than the budget: a second large
    /// request is refused with an error reply while the first still holds its bytes (#1925).
    #[test]
    fn requests_past_the_total_budget_are_refused() {
        let (addr, _rx) = serve_with(Limits { max_line: 1000, total_bytes: 300, idle_timeout: Duration::from_secs(5), ..short() });
        // The first peer sends 200 bytes of an unfinished line and keeps the connection open.
        let mut first = TcpStream::connect(addr).unwrap();
        first.write_all(&[b'a'; 200]).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        // The second would take 200 more, past the 300 byte total.
        let mut second = TcpStream::connect(addr).unwrap();
        second.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        second.write_all(&[b'b'; 200]).unwrap();
        let mut line = String::new();
        BufReader::new(second).read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["ok"], json!(false));
        assert!(v["error"].as_str().unwrap_or("").contains("too many large requests"), "{line}");
        drop(first);
    }

    #[test]
    fn the_budget_is_given_back_when_the_connection_ends() {
        let budget = Budget::new(100);
        {
            let mut h = Held::new(budget.clone());
            let mut buf = Vec::new();
            let mut input: &[u8] = &[b'x'; 80];
            assert_eq!(read_bounded_line(&mut input, &mut buf, 1000, &mut h), Ok(true));
            assert!(!budget.reserve(30), "80 of 100 bytes are held");
        }
        assert!(budget.reserve(100), "all given back on drop");
    }

    /// Replies on one authenticated connection preserve the ID of each request.
    #[test]
    fn requests_are_answered_with_their_ids() {
        let addr = serve_echo();
        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let input = format!("{}\n{}", req_line(3, "ping").trim_end(), req_line(4, "pong"));
        c.write_all(input.as_bytes()).unwrap();
        let mut r = BufReader::new(c);
        for (id, method) in [(3, "ping"), (4, "pong")] {
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let v: Value = serde_json::from_str(&line).unwrap();
            assert_eq!((v["id"].as_i64(), v["method"].as_str()), (Some(id), Some(method)), "{line}");
        }
    }

    /// Missing and incorrect session tokens are rejected before dispatch.
    #[test]
    fn a_request_without_the_token_is_unauthorized() {
        // A local peer that does not hold the session token cannot drive the app (CWE-306).
        let addr = serve_echo();
        let replies = exchange(addr, b"{\"id\":9,\"method\":\"ping\"}\n");
        assert_eq!(replies.len(), 1, "{replies:?}");
        assert_eq!(replies[0]["ok"], false);
        assert!(replies[0]["error"].as_str().unwrap().contains("unauthorized"), "{replies:?}");
        // A wrong token is also refused.
        let wrong = exchange(addr, b"{\"id\":9,\"method\":\"ping\",\"token\":\"nope\"}\n");
        assert_eq!(wrong[0]["ok"], false);
        assert!(wrong[0]["error"].as_str().unwrap().contains("unauthorized"), "{wrong:?}");
    }

    /// A request carrying the session token reaches the control dispatcher.
    #[test]
    fn a_request_with_the_token_is_dispatched() {
        let addr = serve_echo();
        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(req_line(10, "ping").as_bytes()).unwrap();
        let mut line = String::new();
        BufReader::new(c).read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!((v["ok"].as_bool(), v["method"].as_str()), (Some(true), Some("ping")), "{line}");
    }

    #[test]
    fn extra_connections_are_refused() {
        let addr = serve_echo();
        // Idle connections are accepted in order and held open.
        let held: Vec<TcpStream> = (0..Limits::default().max_clients).map(|_| TcpStream::connect(addr).unwrap()).collect();
        let mut extra = TcpStream::connect(addr).unwrap();
        extra.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut line = String::new();
        BufReader::new(&mut extra).read_line(&mut line).unwrap();
        assert!(line.contains("too many control connections"), "{line}");
        drop(held);
    }
}
