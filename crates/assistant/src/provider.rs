//! Where the model's answers come from.

use std::collections::VecDeque;

use serde_json::{Value, json};

/// A chat-completions endpoint. `request` is an OpenAI-style body (`model`, `messages`,
/// `tools`); the reply is the endpoint's JSON response (`choices[0].message`…).
pub trait Provider {
    fn complete(&mut self, request: &Value) -> Result<Value, String>;
}

/// A provider that replays canned responses, for tests and demos without a network. It records
/// every request it was sent.
#[derive(Default)]
pub struct Scripted {
    replies: VecDeque<Value>,
    pub requests: Vec<Value>,
}

impl Scripted {
    pub fn new(replies: impl IntoIterator<Item = Value>) -> Self {
        Self { replies: replies.into_iter().collect(), requests: Vec::new() }
    }
}

impl Provider for Scripted {
    fn complete(&mut self, request: &Value) -> Result<Value, String> {
        self.requests.push(request.clone());
        self.replies.pop_front().ok_or_else(|| "the scripted provider has no more replies".to_string())
    }
}

/// A chat-completions response whose message is plain text.
pub fn text_reply(text: &str) -> Value {
    json!({"choices": [{"message": {"role": "assistant", "content": text}, "finish_reason": "stop"}]})
}

/// A chat-completions response asking for tool calls: `(id, tool name, arguments)`.
pub fn tool_reply(calls: &[(&str, &str, Value)]) -> Value {
    let calls: Vec<Value> = calls
        .iter()
        .map(|(id, name, args)| json!({"id": id, "type": "function", "function": {"name": name, "arguments": args.to_string()}}))
        .collect();
    json!({"choices": [{"message": {"role": "assistant", "content": null, "tool_calls": calls}, "finish_reason": "tool_calls"}]})
}
