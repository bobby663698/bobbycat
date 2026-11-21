// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The child-process loader's skeleton: start a plugin and shake hands.
//!
//! The policy in [`crate::manifest`], [`crate::permissions`] and
//! [`crate::paths`] decides *whether* a plugin may be run and what it may
//! ask for. This module does the one physical thing that policy never did:
//! it starts the program and checks that the thing on the other end of the
//! pipe speaks the same protocol.
//!
//! # The two frames
//!
//! The wire is the same JSON-RPC 2.0 line envelope the editor's stdin
//! transport uses - one object per line, `jsonrpc` / `id` / `method` /
//! `params` - but this crate deliberately does not depend on `concat-api`:
//! the handshake is two frames and its vocabulary must be able to stay put
//! while the editor's RPC grows. So the envelope is written out here in
//! small, and only the pieces the handshake needs are parsed.
//!
//! 1. the host writes `{"jsonrpc":"2.0","id":1,"method":"plugin.hello",
//!    "params":{"api":1}}` - "this is the interface I speak";
//! 2. the child answers `{"jsonrpc":"2.0","id":1,"result":{"api":1}}`,
//!    or an `error` object if it refuses.
//!
//! Anything else - a notification instead of a reply, somebody else's id,
//! a different api major - is refused before a single API method is sent.
//!
//! # The host is not the plugin's babysitter
//!
//! Spawning, handshake and cleanup are contained here. A child that exits
//! non-zero is *recorded*, not raised: [`PluginProcess::poll`] returns its
//! status, [`PluginProcess::failed`] reports it, and the caller's main flow
//! keeps going. A child that never answers [`load`] is killed on a timeout.
//! The [`Drop`] impl kills and reaps whatever is left, so no zombie is left
//! behind when the handle goes out of scope (see `Drop::drop`).

use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};

use crate::manifest::{self, EntryKind, Manifest};

/// The id of the single frame the host writes before anything else.
pub const HELLO_ID: u32 = 1;

/// A handshake frame longer than this is refused rather than buffered.
pub const MAX_FRAME_BYTES: usize = 16 * 1024;

/// How long the host waits for the ready reply before killing the child.
pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// The knobs a caller may tighten without changing the protocol.
#[derive(Clone, Debug)]
pub struct LoadConfig {
    /// Time allowed for the child to answer `hello`.
    pub handshake_timeout: Duration,
    /// Largest single line accepted from the child during the handshake.
    pub max_frame_bytes: usize,
}

impl Default for LoadConfig {
    fn default() -> Self {
        Self {
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
            max_frame_bytes: MAX_FRAME_BYTES,
        }
    }
}

/// A child that answered `ready` with an api this build speaks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ready {
    /// The api major the child reported.
    pub api: u32,
}

/// Why a ready frame was not accepted.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum HandshakeError {
    /// Not the JSON object the protocol describes.
    Malformed(String),
    /// A reply to a different call - or no reply at all.
    WrongId {
        /// The id that was found, or `"none"`.
        found: String,
    },
    /// The child explicitly refused the hello.
    Refused {
        /// The JSON-RPC error code it gave.
        code: i64,
        /// The reason it gave.
        message: String,
    },
    /// The child speaks a different api major.
    IncompatibleApi {
        /// What the child reported.
        found: u32,
        /// What this build speaks.
        wanted: u32,
    },
}

impl fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandshakeError::Malformed(why) => write!(f, "握手帧不合法：{why}"),
            HandshakeError::WrongId { found } => {
                write!(f, "这不是 id {HELLO_ID} 的应答（收到 {found}）")
            }
            HandshakeError::Refused { code, message } => {
                write!(f, "插件拒绝握手（{code}）：{message}")
            }
            HandshakeError::IncompatibleApi { found, wanted } => {
                write!(f, "插件使用 API {found}，本版本使用 API {wanted}")
            }
        }
    }
}

impl std::error::Error for HandshakeError {}

/// The hello frame the host writes, as one line without its newline.
///
/// `id` is echoed in the reply and checked there; `api` is the major this
/// build speaks, so a plugin written for another major says so on the first
/// exchange instead of failing on a method call later.
pub fn hello_frame(id: u32, api: u32) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "plugin.hello",
        "params": { "api": api },
    })
    .to_string()
}

/// Parse the child's one ready frame.
///
/// Pure: no pipe, no clock, no child. Every refusal carries the reason so
/// the caller can say which half spoke a different protocol.
pub fn parse_ready(line: &str, expected_id: &Value) -> Result<Ready, HandshakeError> {
    let value: Value =
        serde_json::from_str(line).map_err(|error| HandshakeError::Malformed(error.to_string()))?;
    let Value::Object(mut object) = value else {
        return Err(HandshakeError::Malformed("应答必须是 JSON 对象".to_owned()));
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(HandshakeError::Malformed(
            "jsonrpc 必须是 \"2.0\"".to_owned(),
        ));
    }
    // A notification reads like a call; it is not an answer to our hello.
    if object.contains_key("method") {
        return Err(HandshakeError::Malformed(
            "ready 应答不能带 method".to_owned(),
        ));
    }
    let found_id = object.remove("id").ok_or_else(|| HandshakeError::WrongId {
        found: "none".to_owned(),
    })?;
    if &found_id != expected_id {
        return Err(HandshakeError::WrongId {
            found: found_id.to_string(),
        });
    }
    if let Some(error) = object.remove("error") {
        return Err(HandshakeError::Refused {
            code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
            message: error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        });
    }
    let Some(result) = object.remove("result") else {
        return Err(HandshakeError::Malformed(
            "ready 应答必须带 result 或 error".to_owned(),
        ));
    };
    let Value::Object(mut fields) = result else {
        return Err(HandshakeError::Malformed("result 必须是对象".to_owned()));
    };
    let Some(api) = fields
        .remove("api")
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())
    else {
        return Err(HandshakeError::Malformed(
            "result.api 必须是 u32".to_owned(),
        ));
    };
    let wanted = manifest::API_MAJOR;
    if api != wanted {
        return Err(HandshakeError::IncompatibleApi { found: api, wanted });
    }
    Ok(Ready { api })
}

/// Why a plugin never reached the ready state.
#[derive(Debug)]
pub enum LoadError {
    /// The entry is real but needs a facility this build does not have.
    Unsupported(String),
    /// The OS could not start the program.
    Spawn(io::Error),
    /// A pipe broke while the handshake was in flight.
    Io(io::Error),
    /// The child did not answer in time and was killed.
    Timeout,
    /// The child closed stdout before answering; the status is kept when
    /// the OS could report it.
    Crashed(Option<ExitStatus>),
    /// The child answered, but not with a ready frame.
    Handshake(HandshakeError),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Unsupported(why) => write!(f, "插件入口暂不支持：{why}"),
            LoadError::Spawn(error) => write!(f, "插件进程启动失败：{error}"),
            LoadError::Io(error) => write!(f, "插件握手时管道出错：{error}"),
            LoadError::Timeout => f.write_str("插件在握手时限内没有回答 ready"),
            LoadError::Crashed(Some(status)) => {
                write!(f, "插件在回答 ready 前退出：{status}")
            }
            LoadError::Crashed(None) => f.write_str("插件在回答 ready 前退出，状态不可读"),
            LoadError::Handshake(error) => write!(f, "插件的握手帧非法：{error}"),
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LoadError::Spawn(error) | LoadError::Io(error) => Some(error),
            LoadError::Handshake(error) => Some(error),
            _ => None,
        }
    }
}

/// A started plugin whose handshake succeeded.
///
/// The stdio handles stay open for the RPC of the next batch. Exit status
/// is observed opportunistically: the host is never blocked by a child
/// that dies at the wrong moment.
pub struct PluginProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
    stderr_tail: Arc<Mutex<Vec<u8>>>,
    _stderr_drain: JoinHandle<()>,
    /// The plugin's declared name, carried for logs.
    pub name: String,
    /// The api the child reported in its ready frame.
    pub ready: Ready,
    exit: Option<ExitStatus>,
}

impl PluginProcess {
    /// The child's pid, for the host's process table.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// The child's stdin, still open for RPC calls.
    pub fn stdin(&mut self) -> Option<&mut ChildStdin> {
        self.stdin.as_mut()
    }

    /// The child's stdout, still open for RPC replies.
    pub fn stdout(&mut self) -> Option<&mut BufReader<ChildStdout>> {
        self.stdout.as_mut()
    }

    /// Signal end of input without killing the child.
    pub fn close_stdin(&mut self) {
        drop(self.stdin.take());
    }

    /// Observe a child that has finished, without ever blocking.
    ///
    /// The first status seen is kept. Repeated polls return the same one.
    pub fn poll(&mut self) -> Option<ExitStatus> {
        if self.exit.is_some() {
            return self.exit;
        }
        if let Ok(Some(status)) = self.child.try_wait() {
            self.exit = Some(status);
        }
        self.exit
    }

    /// The recorded exit status, if the child has finished.
    pub fn exit_status(&self) -> Option<ExitStatus> {
        self.exit
    }

    /// Whether the recorded exit was non-zero.
    ///
    /// This is a fact to log, not an error to raise: a crashed plugin must
    /// not take the host's main flow with it.
    pub fn failed(&self) -> bool {
        self.exit.is_some_and(|status| !status.success())
    }

    /// The last 8 KiB the child wrote to stderr, lossily decoded.
    pub fn stderr_tail(&self) -> String {
        match self.stderr_tail.lock() {
            Ok(guard) => String::from_utf8_lossy(&guard).into_owned(),
            Err(poisoned) => String::from_utf8_lossy(&poisoned.into_inner()).into_owned(),
        }
    }
}

impl Drop for PluginProcess {
    fn drop(&mut self) {
        // EOF first; a well-behaved child exits on its own.
        drop(self.stdin.take());
        if self.exit.is_none() {
            let _ = self.child.kill();
        }
        // Reap either way - never leave a zombie for the host to inherit.
        let _ = self.child.wait();
    }
}

/// Start `manifest`'s entry and complete the two-frame handshake.
///
/// `plugin_dir` is the child's working directory (where `${PLUGIN}`
/// points), so a relative `entry.command` and its `args` land where the
/// manifest says. On any failure the child is killed and reaped before the
/// error returns.
pub fn load(
    manifest: &Manifest,
    plugin_dir: &Path,
    config: &LoadConfig,
) -> Result<PluginProcess, LoadError> {
    match manifest.entry.kind {
        EntryKind::Process => {}
        EntryKind::Wasm => {
            return Err(LoadError::Unsupported(
                "wasm 入口要等沙箱（批次四）".to_owned(),
            ));
        }
    }

    let mut child = Command::new(&manifest.entry.command)
        .args(&manifest.entry.args)
        .current_dir(plugin_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(LoadError::Spawn)?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| LoadError::Io(io::Error::other("没有 stdin 管道")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| LoadError::Io(io::Error::other("没有 stdout 管道")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| LoadError::Io(io::Error::other("没有 stderr 管道")))?;

    let tail = Arc::new(Mutex::new(Vec::new()));
    let drain = spawn_stderr_drain(stderr, Arc::clone(&tail));

    let result = handshake(&mut child, &mut stdin, BufReader::new(stdout), config);
    match result {
        Ok((ready, stdout)) => Ok(PluginProcess {
            child,
            stdin: Some(stdin),
            stdout: Some(stdout),
            stderr_tail: tail,
            _stderr_drain: drain,
            name: manifest.name.clone(),
            ready,
            exit: None,
        }),
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = drain.join();
            Err(error)
        }
    }
}

/// Write hello, then wait for one bounded line on stdout.
fn handshake(
    child: &mut Child,
    stdin: &mut ChildStdin,
    stdout: BufReader<ChildStdout>,
    config: &LoadConfig,
) -> Result<(Ready, BufReader<ChildStdout>), LoadError> {
    let frame = hello_frame(HELLO_ID, manifest::API_MAJOR);
    writeln!(stdin, "{frame}").map_err(LoadError::Io)?;
    stdin.flush().map_err(LoadError::Io)?;

    let cap = config.max_frame_bytes;
    type ReadResult = (io::Result<Option<String>>, BufReader<ChildStdout>);
    let (sender, receiver) = mpsc::channel::<ReadResult>();
    // A std pipe has no cross-platform read timeout. The reader owns the
    // pipe so the caller can time out on the channel instead.
    let _ = thread::spawn(move || {
        let mut stdout = stdout;
        let line = read_capped_line(&mut stdout, cap);
        let _ = sender.send((line, stdout));
    });

    let (line, stdout) = match receiver.recv_timeout(config.handshake_timeout) {
        Ok(pair) => pair,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let _ = child.kill();
            let _ = child.wait();
            // Do not join: a plugin that forked could hold the write end
            // open forever, and a join would then stall the host.
            return Err(LoadError::Timeout);
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(LoadError::Io(io::Error::other("握手读取线程提前退出")));
        }
    };

    let line = match line {
        Ok(Some(line)) => line,
        Ok(None) => {
            let status = child.wait().ok();
            return Err(LoadError::Crashed(status));
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(LoadError::Io(error));
        }
    };
    match parse_ready(&line, &json!(HELLO_ID)) {
        Ok(ready) => Ok((ready, stdout)),
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(LoadError::Handshake(error))
        }
    }
}

/// One line, up to `cap` bytes, without its newline.
fn read_capped_line<R: BufRead>(reader: &mut R, cap: usize) -> io::Result<Option<String>> {
    let mut out = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if out.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "一行在换行前就结束了",
                ))
            };
        }
        let end = match available.iter().position(|&byte| byte == b'\n') {
            Some(index) => index + 1,
            None => available.len(),
        };
        if out.len() + end > cap {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "握手帧太长"));
        }
        out.extend_from_slice(&available[..end]);
        let newline = available[end - 1] == b'\n';
        reader.consume(end);
        if newline {
            while out
                .last()
                .is_some_and(|&byte| byte == b'\n' || byte == b'\r')
            {
                out.pop();
            }
            return Ok(Some(String::from_utf8_lossy(&out).into_owned()));
        }
    }
}

/// Drain stderr so a chatty plugin cannot block on a full pipe.
fn spawn_stderr_drain(stderr: ChildStderr, tail: Arc<Mutex<Vec<u8>>>) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut stderr = stderr;
        let mut buf = [0u8; 1024];
        const KEEP: usize = 8 * 1024;
        loop {
            match stderr.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    let mut guard = match tail.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    if guard.len() + count > KEEP {
                        let excess = guard.len() + count - KEEP;
                        let cut = excess.min(guard.len());
                        guard.drain(..cut);
                    }
                    guard.extend_from_slice(&buf[..count]);
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello_id() -> Value {
        json!(HELLO_ID)
    }

    fn ready(id: u32, api: u32) -> String {
        json!({ "jsonrpc": "2.0", "id": id, "result": { "api": api } }).to_string()
    }

    #[test]
    fn hello_frame_is_the_envelope_we_promise() {
        let frame: Value =
            serde_json::from_str(&hello_frame(HELLO_ID, manifest::API_MAJOR)).expect("JSON");
        assert_eq!(
            frame,
            json!({
                "jsonrpc": "2.0",
                "id": HELLO_ID,
                "method": "plugin.hello",
                "params": { "api": manifest::API_MAJOR },
            })
        );
    }

    #[test]
    fn a_ready_reply_yields_the_api_version() {
        let got = parse_ready(&ready(HELLO_ID, manifest::API_MAJOR), &hello_id()).expect("ready");
        assert_eq!(
            got,
            Ready {
                api: manifest::API_MAJOR
            }
        );
    }

    #[test]
    fn a_reply_to_another_id_is_rejected() {
        let error = parse_ready(&ready(HELLO_ID + 1, manifest::API_MAJOR), &hello_id())
            .expect_err("wrong id");
        assert!(matches!(error, HandshakeError::WrongId { .. }), "{error:?}");
    }

    #[test]
    fn a_notification_is_not_a_ready_reply() {
        let line = json!({ "jsonrpc": "2.0", "method": "plugin.hello", "params": { "api": 1 } })
            .to_string();
        let error = parse_ready(&line, &hello_id()).expect_err("not a reply");
        assert!(matches!(error, HandshakeError::Malformed(_)), "{error:?}");
    }

    #[test]
    fn garbage_and_wrong_versions_are_rejected() {
        assert!(matches!(
            parse_ready("{", &hello_id()),
            Err(HandshakeError::Malformed(_))
        ));
        assert!(matches!(
            parse_ready("[]", &hello_id()),
            Err(HandshakeError::Malformed(_))
        ));
        let wrong = json!({ "jsonrpc": "1.0", "id": HELLO_ID, "result": { "api": 1 } }).to_string();
        assert!(matches!(
            parse_ready(&wrong, &hello_id()),
            Err(HandshakeError::Malformed(_))
        ));
    }

    #[test]
    fn a_refusal_and_an_api_mismatch_are_distinguished() {
        let refused = json!({
            "jsonrpc": "2.0", "id": HELLO_ID,
            "error": { "code": -32000, "message": "busy" },
        })
        .to_string();
        assert_eq!(
            parse_ready(&refused, &hello_id()).expect_err("refused"),
            HandshakeError::Refused {
                code: -32000,
                message: "busy".to_owned(),
            }
        );
        let other = manifest::API_MAJOR + 1;
        let future = ready(HELLO_ID, other);
        assert_eq!(
            parse_ready(&future, &hello_id()).expect_err("incompatible"),
            HandshakeError::IncompatibleApi {
                found: other,
                wanted: manifest::API_MAJOR,
            }
        );
    }

    #[test]
    fn a_missing_or_mistyped_api_is_rejected() {
        let empty = json!({ "jsonrpc": "2.0", "id": HELLO_ID, "result": {} }).to_string();
        assert!(matches!(
            parse_ready(&empty, &hello_id()),
            Err(HandshakeError::Malformed(_))
        ));
        let string = json!({
            "jsonrpc": "2.0", "id": HELLO_ID, "result": { "api": "1" },
        })
        .to_string();
        assert!(matches!(
            parse_ready(&string, &hello_id()),
            Err(HandshakeError::Malformed(_))
        ));
    }

    #[test]
    fn a_capped_reader_stops_at_the_line_and_the_limit() {
        let text = b"{\"api\":1}\nrest";
        let mut reader = io::Cursor::new(text.to_vec());
        assert_eq!(
            read_capped_line(&mut reader, 64).expect("a line"),
            Some("{\"api\":1}".to_owned())
        );
        let mut reader = io::Cursor::new(text.to_vec());
        assert!(read_capped_line(&mut reader, 4).is_err());
    }
}
