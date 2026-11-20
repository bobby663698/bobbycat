//! Where the endpoints live, and what one endpoint is.
//!
//! One JSON file beside the project folder, because that is what a user can
//! edit by hand and what a support thread can ask them to paste. The key is
//! read from the environment when `key_env` is set, so a user who does not want
//! it on disk does not have to have it on disk.

use crate::Error;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Which wire format an endpoint speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flavour {
    /// `{base}/chat/completions` — OpenAI, DeepSeek, Moonshot, Qwen, Ollama,
    /// LM Studio, any gateway that copies the shape.
    OpenAi,
    /// `{base}/v1/messages` — Anthropic and the proxies that mirror it.
    Anthropic,
}

/// One place to send a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    /// Shown in the picker.
    pub name: String,
    /// Everything before the verb: `https://api.openai.com/v1`.
    pub base: String,
    /// The key itself. Empty when `key_env` supplies it.
    #[serde(default)]
    pub key: String,
    /// Environment variable to read the key from instead of `key`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_env: Option<String>,
    /// Default model for this endpoint.
    pub model: String,
    pub flavour: Flavour,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

impl Endpoint {
    /// The key to send: the environment's if `key_env` names one that is set,
    /// otherwise the stored one.
    pub fn resolve_key(&self) -> String {
        if let Some(var) = &self.key_env {
            if let Ok(v) = std::env::var(var) {
                if !v.is_empty() {
                    return v;
                }
            }
        }
        self.key.clone()
    }

    pub fn openai(key: impl Into<String>) -> Self {
        Self {
            name: "OpenAI".into(),
            base: "https://api.openai.com/v1".into(),
            key: key.into(),
            key_env: None,
            model: "gpt-4o".into(),
            flavour: Flavour::OpenAi,
            temperature: None,
            max_tokens: None,
        }
    }

    pub fn anthropic(key: impl Into<String>) -> Self {
        Self {
            name: "Anthropic".into(),
            base: "https://api.anthropic.com".into(),
            key: key.into(),
            key_env: None,
            model: "claude-sonnet-4-20250514".into(),
            flavour: Flavour::Anthropic,
            temperature: None,
            max_tokens: None,
        }
    }

    /// A gateway or a local server: anything that copies the OpenAI shape.
    pub fn custom(name: impl Into<String>, base: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            base: base.into(),
            key: String::new(),
            key_env: None,
            model: model.into(),
            flavour: Flavour::OpenAi,
            temperature: None,
            max_tokens: None,
        }
    }
}

/// Everything the assistant pane remembers between runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub endpoints: Vec<Endpoint>,
    /// Index into `endpoints`; clamped on load.
    #[serde(default)]
    pub active: usize,
    #[serde(default)]
    pub system_prompt: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            endpoints: Vec::new(),
            active: 0,
            system_prompt: crate::DEFAULT_SYSTEM.to_string(),
        }
    }
}

impl Config {
    /// The endpoint in use, if any.
    pub fn active_endpoint(&self) -> Option<&Endpoint> {
        self.endpoints.get(self.active.min(self.endpoints.len().saturating_sub(1)))
    }

    /// The endpoint in use, mutably.
    pub fn active_endpoint_mut(&mut self) -> Option<&mut Endpoint> {
        let i = self.active.min(self.endpoints.len().saturating_sub(1));
        self.endpoints.get_mut(i)
    }

    /// Read the config, or a default when there is no file yet.
    ///
    /// A file that exists but does not parse is an error rather than a silently
    /// replaced default: overwriting a user's key because a comma was missing
    /// is the worse outcome.
    pub fn load() -> Result<Self, Error> {
        let path = config_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::Config(format!("{}: {e}", path.display())))?;
        serde_json::from_str(&text)
            .map_err(|e| Error::Config(format!("{}: {e}", path.display())))
    }

    /// Write the config back, creating the folder if needed.
    pub fn save(&self) -> Result<(), Error> {
        let path = config_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| Error::Config(format!("{}: {e}", dir.display())))?;
        }
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, text)
            .map_err(|e| Error::Config(format!("{}: {e}", path.display())))
    }
}

/// `%APPDATA%\BobbyCat\agent.json`, `~/.config/bobbycat/agent.json`, or
/// `$BOBBYCAT_HOME/agent.json` when the user keeps their settings elsewhere.
pub fn config_path() -> PathBuf {
    if let Ok(home) = std::env::var("BOBBYCAT_HOME") {
        return PathBuf::from(home).join("agent.json");
    }
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("BobbyCat").join("agent.json");
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            return PathBuf::from(xdg).join("bobbycat").join("agent.json");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".config/bobbycat/agent.json");
        }
    }
    PathBuf::from("agent.json")
}
