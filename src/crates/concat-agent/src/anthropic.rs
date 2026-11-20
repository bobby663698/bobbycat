//! The Anthropic wire format: `POST {base}/v1/messages`, `stream: true`.
//!
//! Three things differ from the OpenAI shape and each one is load-bearing:
//! the key goes in `x-api-key` not `Authorization`, `anthropic-version` is a
//! required header, and `max_tokens` is required rather than optional. The
//! system prompt is a top-level field, not a message — putting it in `messages`
//! is a 400.

use crate::{Endpoint, Error, Message, Role};
use serde_json::{Value, json};
use std::io::BufReader;

/// Anthropic rejects a request without `max_tokens`; this is the ceiling it
/// gets when the user has not chosen one.
const DEFAULT_MAX_TOKENS: u32 = 4096;

pub fn stream(
    endpoint: &Endpoint,
    messages: &[Message],
    sink: &mut dyn FnMut(&str),
) -> Result<String, Error> {
    let url = format!("{}/v1/messages", endpoint.base.trim_end_matches('/'));

    let mut system = String::new();
    let mut turns: Vec<Value> = Vec::new();
    for m in messages {
        match m.role {
            Role::System => {
                if !system.is_empty() {
                    system.push_str("\n\n");
                }
                system.push_str(&m.content);
            }
            Role::User => turns.push(json!({ "role": "user", "content": m.content })),
            Role::Assistant => turns.push(json!({ "role": "assistant", "content": m.content })),
        }
    }

    let mut body = json!({
        "model": endpoint.model,
        "messages": turns,
        "max_tokens": endpoint.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "stream": true,
    });
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    if let Some(t) = endpoint.temperature {
        body["temperature"] = json!(t);
    }

    let resp = ureq::post(&url)
        .set("x-api-key", &endpoint.resolve_key())
        .set("anthropic-version", "2023-06-01")
        .set("Content-Type", "application/json")
        .set("Accept", "text/event-stream")
        .send_string(&body.to_string())?;

    let mut sse = crate::sse::Sse::new(BufReader::new(resp.into_reader()));
    let mut out = String::new();

    while let Some(payload) = sse
        .next_event()
        .map_err(|e| Error::Transport(e.to_string()))?
    {
        let v: Value = match serde_json::from_str(&payload) {
            Ok(v) => v,
            Err(_) => continue,
        };

        match v["type"].as_str().unwrap_or_default() {
            "content_block_delta" => {
                if let Some(text) = v["delta"]["text"].as_str()
                    && !text.is_empty()
                {
                    out.push_str(text);
                    sink(text);
                }
            }
            "message_stop" => break,
            "error" => return Err(Error::Protocol(v["error"].to_string())),
            // message_start, content_block_start/stop, message_delta, ping:
            // nothing in them the pane draws.
            _ => {}
        }
    }

    if out.is_empty() {
        return Err(Error::Protocol("stream ended with no content".into()));
    }
    Ok(out)
}
