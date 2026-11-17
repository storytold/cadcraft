//! The WebSocket connection: one socket, JSON messages, a short read timeout.
//!
//! The client keeps no protocol state here: [`Connection`] sends and receives the envelope's
//! messages, and [`crate::Client`] applies what arrives. A deployment terminates TLS at its proxy.

use std::net::TcpStream;
use std::time::Duration;

use cadcraft_collab::{ClientMessage, ServerMessage};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

/// A socket to a collaboration room.
pub struct Connection {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
}

impl Connection {
    /// Connect, send `hello` and return the server's first message (normally `welcome`).
    pub fn connect(url: &str, hello: &ClientMessage, timeout: Duration) -> Result<(Self, ServerMessage), String> {
        if let Some(reason) = tls_refusal(url) {
            return Err(reason.to_string());
        }
        let (mut socket, _response) = tungstenite::connect(url).map_err(|e| e.to_string())?;
        send_message(&mut socket, hello)?;
        if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
            stream.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
        }
        let mut connection = Connection { socket };
        let first = connection.receive()?;
        Ok((connection, first))
    }

    /// Send one message.
    pub fn send(&mut self, message: &ClientMessage) -> Result<(), String> {
        send_message(&mut self.socket, message)
    }

    /// Wait for one message; fails after the connection's read timeout.
    pub fn receive(&mut self) -> Result<ServerMessage, String> {
        match self.socket.read() {
            Ok(Message::Text(text)) => serde_json::from_str(text.as_str()).map_err(|e| e.to_string()),
            Ok(_) => self.receive(),
            Err(error) => Err(error.to_string()),
        }
    }

    /// Wait for one message, `Ok(None)` when the read timeout fired: the socket is still open.
    ///
    /// The error kind tells a timeout from a lost connection, the message does not: an OS read
    /// timeout says `Resource temporarily unavailable (os error 11)` on Linux and macOS and names
    /// `os error 10060` on Windows, and neither text contains `timed out`.
    pub fn try_receive(&mut self) -> Result<Option<ServerMessage>, String> {
        loop {
            match self.socket.read() {
                Ok(Message::Text(text)) => {
                    return serde_json::from_str::<ServerMessage>(text.as_str()).map(Some).map_err(|e| e.to_string());
                }
                Ok(_) => {}
                Err(error) if is_read_timeout(&error) => return Ok(None),
                Err(error) => return Err(error.to_string()),
            }
        }
    }

    /// Close the socket.
    pub fn close(&mut self) {
        let _ = self.socket.close(None);
    }
}

fn send_message(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>, message: &ClientMessage) -> Result<(), String> {
    let text = serde_json::to_string(message).map_err(|e| e.to_string())?;
    socket.send(Message::Text(text.into())).map_err(|e| e.to_string())
}

/// Why this build cannot connect to `url` in-process, or `None` when it can.
///
/// No TLS stack is linked on purpose: the deployment terminates WSS at its ingress, and enabling
/// TLS here would choose a backend (`native-tls` or `rustls`) and grow `Cargo.lock` — the owner's
/// supply-chain decision, not a review fix (CodeRabbit on #2936). The refusal names the supported
/// path instead of leaving tungstenite's generic error.
fn tls_refusal(url: &str) -> Option<&'static str> {
    url.trim_start()
        .to_ascii_lowercase()
        .starts_with("wss://")
        .then_some("wss:// is not supported by this build: the deployment terminates TLS at its ingress; connect to the proxy's ws:// address or a local TLS tunnel")
}

/// Whether a read error is the socket's read timeout: nothing arrived within the timeout, the
/// socket itself is fine and the caller should keep draining its queue.
fn is_read_timeout(error: &tungstenite::Error) -> bool {
    matches!(error, tungstenite::Error::Io(io) if matches!(io.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
}

#[cfg(test)]
mod tests {
    use super::{is_read_timeout, tls_refusal};
    use std::io;

    /// A read timeout is not a lost connection: Linux and macOS report `WouldBlock`, Windows
    /// reports `TimedOut`, and the message of neither says `timed out`, so only the kind can tell
    /// them apart (CodeRabbit on #2936).
    #[test]
    fn a_read_timeout_is_not_a_lost_connection() {
        assert!(is_read_timeout(&tungstenite::Error::Io(io::Error::from(io::ErrorKind::WouldBlock))));
        assert!(is_read_timeout(&tungstenite::Error::Io(io::Error::from(io::ErrorKind::TimedOut))));
        assert!(!io::Error::from(io::ErrorKind::WouldBlock).to_string().contains("timed out"), "the text check missed it");
        assert!(!is_read_timeout(&tungstenite::Error::Io(io::Error::from(io::ErrorKind::ConnectionReset))));
        assert!(!is_read_timeout(&tungstenite::Error::ConnectionClosed));
    }

    /// A `wss://` URL is outside the configuration this build ships, and the refusal says where
    /// TLS lives instead of failing as a broken address (CodeRabbit on #2936).
    #[test]
    fn a_wss_url_is_refused_with_the_supported_path() {
        assert!(tls_refusal("wss://cad.example/room").is_some_and(|reason| reason.contains("ingress")));
        assert!(tls_refusal(" WS://127.0.0.1:8787 ").is_none(), "a plain socket is this build's transport");
        assert!(tls_refusal("ws://127.0.0.1:8787").is_none());
    }
}
