//! One conversation, streamed to the terminal. The smallest possible caller:
//! everything the pane does, minus the pane.
//!
//! ```text
//! set OPENAI_API_KEY=sk-...
//! cargo run -p concat-agent --example chat -- "给这三段素材排一个 15 秒卡点"
//! ```
//!
//! A gateway instead of OpenAI:
//!
//! ```text
//! set AGENT_BASE=https://your-gateway.example.com/v1
//! set AGENT_MODEL=deepseek-chat
//! ```

use concat_agent::{chat, Endpoint, Message};
use std::io::Write;

fn main() {
    let prompt = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let prompt = if prompt.is_empty() {
        "用一句话说明你能在这个剪辑软件里帮上什么忙。".to_owned()
    } else {
        prompt
    };

    let key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    let mut endpoint = Endpoint::openai(key);
    if let Ok(base) = std::env::var("AGENT_BASE") {
        endpoint.base = base;
    }
    if let Ok(model) = std::env::var("AGENT_MODEL") {
        endpoint.model = model;
    }
    if let Ok(var) = std::env::var("AGENT_KEY_ENV") {
        endpoint.key_env = Some(var);
    }

    let messages = vec![
        Message::system(concat_agent::DEFAULT_SYSTEM),
        Message::user(prompt),
    ];

    let mut stdout = std::io::stdout();
    let result = chat(&endpoint, &messages, &mut |delta: &str| {
        print!("{delta}");
        let _ = stdout.flush();
    });

    match result {
        Ok(reply) => {
            println!("\n\n[{} 字符 / {} 字节]", reply.chars().count(), reply.len());
        }
        Err(error) => {
            eprintln!("\n失败: {error}");
            std::process::exit(1);
        }
    }
}
