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

    /// Close the socket.
    pub fn close(&mut self) {
        let _ = self.socket.close(None);
    }
}

fn send_message(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>, message: &ClientMessage) -> Result<(), String> {
    let text = serde_json::to_string(message).map_err(|e| e.to_string())?;
    socket.send(Message::Text(text.into())).map_err(|e| e.to_string())
}
