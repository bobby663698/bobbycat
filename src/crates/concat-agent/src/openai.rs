//! The OpenAI wire format: `POST {base}/chat/completions`, `stream: true`.
//!
//! This is also DeepSeek, Moonshot, Qwen, SiliconFlow, Together, Groq, Ollama
//! (`/v1`), LM Studio (`/v1`), and most company gateways. The only thing that
//! changes is `base` and `model`.

use crate::{Endpoint, Error, Message, Role};
use serde_json::{Value, json};
use std::io::BufReader;

pub fn stream(
    endpoint: &Endpoint,
    messages: &[Message],
    sink: &mut dyn FnMut(&str),
) -> Result<String, Error> {
    let url = format!("{}/chat/completions", endpoint.base.trim_end_matches('/'));

    let msgs: Vec<Value> = messages
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            json!({ "role": role, "content": m.content })
        })
        .collect();

    let mut body = json!({
        "model": endpoint.model,
        "messages": msgs,
        "stream": true,
    });
    if let Some(t) = endpoint.temperature {
        body["temperature"] = json!(t);
    }
    if let Some(n) = endpoint.max_tokens {
        body["max_tokens"] = json!(n);
    }

    let resp = ureq::post(&url)
        .set(
            "Authorization",
            &format!("Bearer {}", endpoint.resolve_key()),
        )
        .set("Content-Type", "application/json")
        .set("Accept", "text/event-stream")
        .send_string(&body.to_string())?;

    let mut sse = crate::sse::Sse::new(BufReader::new(resp.into_reader()));
    let mut out = String::new();

    while let Some(payload) = sse
        .next_event()
        .map_err(|e| Error::Transport(e.to_string()))?
    {
        if payload == "[DONE]" {
            break;
        }
        let v: Value = match serde_json::from_str(&payload) {
            Ok(v) => v,
            // A keep-alive that is not JSON is not a failure worth stopping for.
            Err(_) => continue,
        };

        if let Some(err) = v.get("error") {
            return Err(Error::Protocol(err.to_string()));
        }

        if let Some(delta) = v["choices"][0]["delta"]["content"].as_str() {
            if !delta.is_empty() {
                out.push_str(delta);
                sink(delta);
            }
        }
    }

    if out.is_empty() {
        return Err(Error::Protocol("stream ended with no content".into()));
    }
    Ok(out)
}
