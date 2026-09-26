//! Chat model client for any OpenAI-compatible API (Groq by default), via
//! async-openai with plain JSON requests. Streams text tokens as they arrive and
//! accumulates tool calls. Base URL, key, and model come from config, so
//! switching providers is config-only.
//!
//! async-openai retries some failures on its own, so every call is also bounded
//! by our own timeouts: the chat must fail fast to "assistant offline".

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use async_openai::config::OpenAIConfig;
use async_openai::types::stream::StreamResponse;
use async_openai::Client;
use serde_json::{json, Value};
use tokio::time::timeout;
use tokio_stream::StreamExt;

/// Time allowed to get the response started (includes library retries).
const START_TIMEOUT: Duration = Duration::from_secs(20);
/// Time allowed between streamed chunks.
const IDLE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// JSON text as produced by the model (may be invalid).
    pub arguments: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Completion {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug)]
pub struct LlmError(pub String);

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LlmError {}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Receives text tokens as they stream in.
pub type TokenSink<'a> = &'a mut (dyn FnMut(&str) + Send);

pub trait ChatModel: Send + Sync {
    /// One model call. `messages` and `tools` use the OpenAI chat format. With
    /// `allow_tools` false the model must answer in text.
    fn complete<'a>(
        &'a self,
        messages: &'a [Value],
        tools: &'a [Value],
        allow_tools: bool,
        on_token: TokenSink<'a>,
    ) -> BoxFuture<'a, Result<Completion, LlmError>>;
}

pub struct OpenAiChatModel {
    client: Client<OpenAIConfig>,
    model: String,
}

impl OpenAiChatModel {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        let config = OpenAIConfig::new().with_api_base(base_url).with_api_key(api_key);
        Self { client: Client::with_config(config), model: model.to_string() }
    }

    async fn run(
        &self,
        messages: &[Value],
        tools: &[Value],
        allow_tools: bool,
        on_token: TokenSink<'_>,
    ) -> Result<Completion, LlmError> {
        let request = json!({
            "model": self.model,
            "messages": messages,
            "tools": tools,
            "tool_choice": if allow_tools { "auto" } else { "none" },
            "temperature": 0.2,
            "stream": true,
        });
        let mut stream: StreamResponse<Value> =
            timeout(START_TIMEOUT, self.client.chat().create_stream_byot(request))
                .await
                .map_err(|_| LlmError("timed out waiting for the model".into()))?
                .map_err(|e| LlmError(e.to_string()))?;

        let mut completion = Completion::default();
        let mut calls = ToolCallAccumulator::default();
        loop {
            let chunk = match timeout(IDLE_TIMEOUT, stream.next()).await {
                Err(_) => return Err(LlmError("model stream stalled".into())),
                Ok(None) => break,
                Ok(Some(chunk)) => chunk.map_err(|e| LlmError(e.to_string()))?,
            };
            let delta = &chunk["choices"][0]["delta"];
            if let Some(text) = delta["content"].as_str().filter(|t| !t.is_empty()) {
                completion.text.push_str(text);
                on_token(text);
            }
            if let Some(parts) = delta["tool_calls"].as_array() {
                parts.iter().for_each(|part| calls.add(part));
            }
        }
        completion.tool_calls = calls.finish();
        Ok(completion)
    }
}

impl ChatModel for OpenAiChatModel {
    fn complete<'a>(
        &'a self,
        messages: &'a [Value],
        tools: &'a [Value],
        allow_tools: bool,
        on_token: TokenSink<'a>,
    ) -> BoxFuture<'a, Result<Completion, LlmError>> {
        Box::pin(self.run(messages, tools, allow_tools, on_token))
    }
}

/// Streamed tool calls arrive in pieces keyed by `index`: the id and name
/// usually come first, the JSON arguments in fragments.
#[derive(Default)]
struct ToolCallAccumulator {
    calls: BTreeMap<u64, ToolCall>,
}

impl ToolCallAccumulator {
    fn add(&mut self, part: &Value) {
        let index = part["index"].as_u64().unwrap_or(0);
        let call = self.calls.entry(index).or_insert_with(|| ToolCall {
            id: String::new(),
            name: String::new(),
            arguments: String::new(),
        });
        if let Some(id) = part["id"].as_str() {
            call.id = id.to_string();
        }
        if let Some(name) = part["function"]["name"].as_str() {
            call.name.push_str(name);
        }
        if let Some(args) = part["function"]["arguments"].as_str() {
            call.arguments.push_str(args);
        }
    }

    fn finish(self) -> Vec<ToolCall> {
        self.calls
            .into_iter()
            .map(|(index, mut call)| {
                if call.id.is_empty() {
                    call.id = format!("call_{index}");
                }
                call
            })
            .filter(|call| !call.name.is_empty())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates_streamed_tool_call_fragments() {
        let mut acc = ToolCallAccumulator::default();
        for part in [
            json!({"index": 0, "id": "call_a", "function": {"name": "get_summary", "arguments": ""}}),
            json!({"index": 0, "function": {"arguments": "{\"start\":\"2026-09-26T06:00:00Z\","}}),
            json!({"index": 1, "id": "call_b", "function": {"name": "get_targets", "arguments": "{}"}}),
            json!({"index": 0, "function": {"arguments": "\"end\":\"2026-09-26T07:00:00Z\"}"}}),
        ] {
            acc.add(&part);
        }
        let calls = acc.finish();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "call_a");
        assert_eq!(calls[0].name, "get_summary");
        let args: Value = serde_json::from_str(&calls[0].arguments).unwrap();
        assert_eq!(args["end"], "2026-09-26T07:00:00Z");
        assert_eq!((calls[1].name.as_str(), calls[1].arguments.as_str()), ("get_targets", "{}"));
    }

    #[test]
    fn fills_missing_ids_and_drops_nameless_calls() {
        let mut acc = ToolCallAccumulator::default();
        acc.add(&json!({"index": 0, "function": {"name": "get_current", "arguments": "{}"}}));
        acc.add(&json!({"index": 1, "function": {"arguments": "{}"}}));
        let calls = acc.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_0");
    }
}
