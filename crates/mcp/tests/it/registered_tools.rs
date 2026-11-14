//! MCP tools and instructions an add-on registers with the engine: listed next to the built-in
//! tools, checked against their input schema, run as their command with the tool's fixed params
//! on top, and never in place of a built-in tool.

use cadcraft_engine::{CommandSpec, Session, ToolSpec, register_mcp_instructions, register_specs, register_tools};
use cadcraft_mcp::{Headless, Server};
use serde_json::{Value, json};

fn echo(_: &mut Session, p: &Value) -> cadcraft_engine::Result<Value> {
    Ok(json!({ "got": p }))
}

fn call(server: &mut Server, id: u64, method: &str, params: Value) -> Value {
    server.handle(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).unwrap_or(Value::Null)
}

#[test]
fn registered_tools_and_instructions_reach_the_agent() {
    register_specs(vec![CommandSpec::new("test.echo", "Echo", echo).noundo()]);
    register_tools(vec![
        ToolSpec {
            name: "test_echo",
            description: "Echo the arguments.",
            input_schema: json!({"type": "object", "properties": {"n": {"type": "number"}}, "required": ["n"]}),
            command: "test.echo",
            fixed: json!({"mode": "fixed"}),
        },
        ToolSpec {
            name: "execute",
            description: "Not the built-in one.",
            input_schema: json!({"type": "object"}),
            command: "test.echo",
            fixed: Value::Null,
        },
    ]);
    register_mcp_instructions("Echo with test_echo {n}.");
    register_mcp_instructions("Echo with test_echo {n}.");

    let mut server = Server::new(Box::new(Headless::default()));
    let init = call(&mut server, 1, "initialize", json!({"protocolVersion": cadcraft_mcp::PROTOCOL_VERSION}));
    let text = init["result"]["instructions"].as_str().unwrap_or_default();
    assert_eq!(text.matches("Echo with test_echo {n}.").count(), 1, "{text}");
    assert!(text.ends_with('.') && text.contains("Backend:"), "{text}");

    let list = call(&mut server, 2, "tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().cloned().unwrap_or_default();
    assert!(tools.iter().any(|t| t["name"] == "test_echo" && t["inputSchema"]["required"] == json!(["n"])));
    let executes: Vec<&Value> = tools.iter().filter(|t| t["name"] == "execute").collect();
    assert_eq!(executes.len(), 1);
    assert_ne!(executes[0]["description"], "Not the built-in one.");

    // Runs the command with the arguments and the fixed params.
    let r = call(&mut server, 3, "tools/call", json!({"name": "test_echo", "arguments": {"n": 2, "mode": "mine"}}));
    let out: Value = serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap_or("null")).unwrap_or(Value::Null);
    assert_eq!(out["got"], json!({"n": 2, "mode": "fixed"}), "{r}");

    // Arguments are checked against the registered schema.
    let r = call(&mut server, 4, "tools/call", json!({"name": "test_echo", "arguments": {}}));
    assert!(r["error"]["message"].as_str().is_some_and(|m| m.contains("needs `n`")), "{r}");
}
