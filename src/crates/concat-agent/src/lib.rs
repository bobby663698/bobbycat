//! The assistant: one conversation against a model, streamed token by token.
//!
//! Two wire formats are spoken, because they are the two that every provider a
//! user brings either implements or proxies: the OpenAI one (`/chat/completions`
//! with SSE `choices[].delta.content`), and the Anthropic one (`/v1/messages`
//! with `content_block_delta`). Everything else — DeepSeek, Moonshot, SiliconFlow,
//! Ollama, LM Studio, a company gateway — is the OpenAI shape behind another
//! `base`, which is why `base` is a field and not a constant.
//!
//! Deliberately synchronous: the caller owns a thread, calls [`chat`] on it, and
//! gets each delta through a sink as it arrives. No runtime, no channels, no
//! cancellation token — a dropped connection is the cancellation.

mod anthropic;
mod config;
mod openai;
mod sse;

pub use config::{Config, Endpoint, Flavour};

use serde::{Deserialize, Serialize};

/// Why a conversation could not be had.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The endpoint refused the request; the body is the provider's own words.
    #[error("returned {0}: {1}")]
    Status(u16, String),
    /// The request never got an answer.
    #[error("transport: {0}")]
    Transport(String),
    /// The stream carried something that was not the documented shape.
    #[error("protocol: {0}")]
    Protocol(String),
    /// The config file could not be read or written.
    #[error("config: {0}")]
    Config(String),
}

impl From<ureq::Error> for Error {
    fn from(e: ureq::Error) -> Self {
        match e {
            ureq::Error::Status(code, resp) => {
                let body = resp.into_string().unwrap_or_default();
                Error::Status(code, body)
            }
            ureq::Error::Transport(t) => Error::Transport(t.to_string()),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Protocol(e.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Transport(e.to_string())
    }
}

/// Who is speaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

/// One turn on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// Run one conversation to completion.
///
/// `sink` is called once per delta with the text of that delta; the return value
/// is the whole reply, so a caller that only wants the end result can ignore the
/// sink. The sink runs on the calling thread.
pub fn chat(
    endpoint: &Endpoint,
    messages: &[Message],
    sink: &mut dyn FnMut(&str),
) -> Result<String, Error> {
    match endpoint.flavour {
        Flavour::Anthropic => anthropic::stream(endpoint, messages, sink),
        Flavour::OpenAi => openai::stream(endpoint, messages, sink),
    }
}

/// Ask the endpoint what models it serves, for the picker.
///
/// OpenAI-compatible endpoints answer `GET {base}/models`; Anthropic answers
/// `GET {base}/v1/models`. A failure here is not fatal — the picker falls back
/// to the model already in the config plus free text.
pub fn models(endpoint: &Endpoint) -> Result<Vec<String>, Error> {
    let url = match endpoint.flavour {
        Flavour::Anthropic => format!("{}/v1/models", endpoint.base.trim_end_matches('/')),
        Flavour::OpenAi => format!("{}/models", endpoint.base.trim_end_matches('/')),
    };
    let key = endpoint.resolve_key();
    let req = ureq::get(&url);
    let req = match endpoint.flavour {
        Flavour::Anthropic => req
            .set("x-api-key", &key)
            .set("anthropic-version", "2023-06-01"),
        Flavour::OpenAi => req.set("Authorization", &format!("Bearer {key}")),
    };
    let resp = req.call()?;
    let v: serde_json::Value = resp.into_json()?;
    let mut out = Vec::new();
    if let Some(items) = v["data"].as_array() {
        for item in items {
            if let Some(id) = item["id"].as_str() {
                out.push(id.to_owned());
            }
        }
    }
    Ok(out)
}

/// The system prompt BobbyCat ships with, when the user has not written one.
pub const DEFAULT_SYSTEM: &str = "You are the assistant inside BobbyCat, a video editor. \
Answer in the user's language. When the user asks for an edit, reply with the \
concrete steps in BobbyCat's own terms — tracks, clips, keyframes, effects — \
not with generic advice.";
