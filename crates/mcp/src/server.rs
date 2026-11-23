//! JSON-RPC 2.0 framing and the MCP lifecycle / tools / resources methods.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

use crate::backend::Backend;
use crate::tools::{call_tool, tool_definitions};

pub const PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED: &[&str] = &[PROTOCOL_VERSION, "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

const INSTRUCTIONS: &str = "CADCraft is a 2D/3D CAD drafting app (an AutoCAD clone). Coordinates are drawing units, \
y up. Fastest path: command_line with AutoCAD syntax, e.g. `line 0,0 10,0 10,5 c`, `circle 5,2.5 1`, \
`rectang 0,0 4,3`, `offset 1` then pick, `zoom e`. Or execute with JSON (list_commands shows ids and params). \
Verify with inspect_drawing / query_entities and look with render.";

pub struct Server {
    backend: Box<dyn Backend>,
    session: craft_mcp::Session,
}

impl Server {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Self {
            backend,
            session: craft_mcp::Session::new("cadcraft", env!("CARGO_PKG_VERSION"), craft_mcp::Policy::Compatible)
                .with_capabilities(json!({"tools": {"listChanged": false}, "resources": {"listChanged": false}}))
                .unwrap_or_else(|_| craft_mcp::Session::new("cadcraft", env!("CARGO_PKG_VERSION"), craft_mcp::Policy::Compatible))
                .with_legacy_initialize(SUPPORTED, json!({"name": "cadcraft", "title": "CADCraft", "version": env!("CARGO_PKG_VERSION")})),
        }
    }
    pub fn backend(&mut self) -> &mut dyn Backend {
        self.backend.as_mut()
    }
    pub fn is_initialized(&self) -> bool {
        self.session.is_initialized()
    }

    /// Serve newline-delimited JSON-RPC until `input` closes (logs to stderr only).
    pub fn serve(&mut self, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if let Some(reply) = self.handle_line(&line) {
                output.write_all(reply.as_bytes())?;
                output.write_all(b"\n")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(msg) => self.handle_message(&msg),
            Err(e) => Some(craft_jsonrpc::error_response(Value::Null, craft_jsonrpc::RpcError::new(PARSE_ERROR, format!("parse error: {e}")))),
        };
        reply.map(|r| r.to_string())
    }

    pub fn handle(&mut self, msg: Value) -> Option<Value> {
        // `handle` historically routes only one envelope; batches belong to `handle_line`.
        if msg.is_array() {
            return Some(craft_jsonrpc::error_response(Value::Null, craft_jsonrpc::RpcError::new(INVALID_REQUEST, "message must be an object")));
        }
        self.handle_message(&msg)
    }

    fn handle_message(&mut self, msg: &Value) -> Option<Value> {
        let server = std::cell::RefCell::new(self);
        craft_jsonrpc::process_legacy(
            msg,
            |request| {
                let mut server = server.borrow_mut();
                if request.method == "initialize" {
                    let instructions = format!("{INSTRUCTIONS} Backend: {}.", server.backend.describe());
                    server.session.set_instructions(instructions);
                }
                if let Some(result) = server.session.request(request, msg.is_array()) {
                    return result;
                }
                server
                    .request(request.method, request.params.unwrap_or(&Value::Null))
                    .map_err(|(code, message)| craft_jsonrpc::RpcError::new(code, message))
            },
            |request| server.borrow_mut().session.notification(request),
        )
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "tools/list" => Ok(tool_definitions(self.backend.has_ui())),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).ok_or((INVALID_PARAMS, "missing tool `name`".to_string()))?;
                let args = params.get("arguments").unwrap_or(&Value::Null);
                call_tool(self.backend.as_mut(), name, args).map_err(|m| (INVALID_PARAMS, m))
            }
            "resources/list" => Ok(json!({"resources": [
                {"uri": "cadcraft://drawing", "name": "Active drawing", "mimeType": "application/json"},
                {"uri": "cadcraft://commands", "name": "Command catalog", "mimeType": "application/json"},
            ]})),
            "resources/read" => {
                let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
                let v = match uri {
                    "cadcraft://drawing" => self.backend.call("drawing.inspect", json!({})),
                    "cadcraft://commands" => self.backend.call("engine.commands", json!({})),
                    _ => return Err((-32002, format!("unknown resource `{uri}`"))),
                }
                .map_err(|e| (-32603, e))?;
                Ok(json!({"contents": [{"uri": uri, "mimeType": "application/json", "text": v.to_string()}]}))
            }
            _ => Err((METHOD_NOT_FOUND, format!("method `{method}` not found"))),
        }
    }
}
