// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The loop that carries a plugin's requests to the host, and the host's
//! replies back.
//!
//! [`loader`](crate::loader) answers "may this process start, and is it
//! still alive". This module answers the next question: once it is running,
//! who reads the lines it writes and who writes the answers.
//!
//! It deliberately does not know what a request *means*. Parsing the
//! envelope, deciding whether a method is allowed, and doing the work all
//! belong to the host, which has the project open and the permission table
//! loaded. What is here is the part that must not be got wrong twice: one
//! line in, at most one line out, in order, with a cap so a plugin cannot
//! exhaust memory by never writing a newline.
//!
//! Both ends are generic over [`BufRead`] / [`Write`], so the loop is tested
//! against a `Cursor` rather than a real child process - a test that spawns
//! a program is a test that fails on somebody else's machine.

use std::io::{self, BufRead, Write};

/// What the host side must supply.
///
/// One line in, at most one line out. `None` means the line needs no answer:
/// a notification, or a request the host has already dealt with. Nothing is
/// written back for those.
pub trait Host {
    /// Handle one request line, exactly as the plugin wrote it, without its
    /// newline. Returning `Some` writes that line back and flushes.
    fn request(&mut self, line: &str) -> Option<String>;
}

/// Why a session ended.
#[derive(Debug)]
pub enum BridgeError {
    /// A pipe broke: the plugin died, or the host closed its end.
    Io(io::Error),
    /// One line grew past the cap without a newline. Refused rather than
    /// buffered, because the point of the cap is that memory stays bounded
    /// whoever is on the other end.
    TooLong {
        /// The cap, in bytes, the line would have crossed.
        cap: usize,
    },
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeError::Io(error) => write!(f, "插件管道读写失败：{error}"),
            BridgeError::TooLong { cap } => {
                write!(f, "插件写入的一行超过 {cap} 字节仍未换行，已断开")
            }
        }
    }
}

impl std::error::Error for BridgeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BridgeError::Io(error) => Some(error),
            BridgeError::TooLong { .. } => None,
        }
    }
}

/// Pump requests until the plugin closes its end of the pipe.
///
/// Returns how many lines the host answered. End of input is a normal end,
/// not an error: a plugin that exits after its last call is a plugin that
/// finished. A blank line is skipped - some runtimes emit one on shutdown.
pub fn serve<R: BufRead, W: Write>(
    from_plugin: &mut R,
    to_plugin: &mut W,
    host: &mut impl Host,
    cap: usize,
) -> Result<u64, BridgeError> {
    let mut answered = 0u64;
    let mut line = Vec::new();
    loop {
        line.clear();
        if !read_line_capped(from_plugin, &mut line, cap)? {
            return Ok(answered);
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\n', '\r']);
        if text.is_empty() {
            continue;
        }
        if let Some(reply) = host.request(text) {
            writeln!(to_plugin, "{reply}").map_err(BridgeError::Io)?;
            to_plugin.flush().map_err(BridgeError::Io)?;
            answered += 1;
        }
    }
}

/// Read one line into `out`, newline included, up to `cap` bytes.
///
/// `false` means end of input and nothing was read; `out` is then empty.
/// A final line with no newline is still a line - a plugin that writes its
/// last reply and exits without one should not have it dropped.
fn read_line_capped<R: BufRead>(
    reader: &mut R,
    out: &mut Vec<u8>,
    cap: usize,
) -> Result<bool, BridgeError> {
    loop {
        let available = reader.fill_buf().map_err(BridgeError::Io)?;
        if available.is_empty() {
            return Ok(!out.is_empty());
        }
        let end = match available.iter().position(|&byte| byte == b'\n') {
            Some(index) => index + 1,
            None => available.len(),
        };
        if out.len() + end > cap {
            return Err(BridgeError::TooLong { cap });
        }
        out.extend_from_slice(&available[..end]);
        let newline = available[end - 1] == b'\n';
        reader.consume(end);
        if newline {
            return Ok(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BridgeError, Host, serve};
    use std::io::Cursor;

    /// Answers `ping` and stays quiet for everything else, recording what it
    /// was asked so the tests can assert the loop passed lines through
    /// untouched.
    struct Pinger {
        seen: Vec<String>,
    }

    impl Host for Pinger {
        fn request(&mut self, line: &str) -> Option<String> {
            self.seen.push(line.to_owned());
            (line == r#"{"method":"ping"}"#).then(|| r#"{"result":"pong"}"#.to_owned())
        }
    }

    #[test]
    fn every_request_is_answered_in_order() {
        let mut from = Cursor::new(b"{\"method\":\"ping\"}\n{\"method\":\"ping\"}\n".to_vec());
        let mut to = Vec::new();
        let mut host = Pinger { seen: Vec::new() };

        let answered = serve(&mut from, &mut to, &mut host, 1024).expect("a clean session");

        assert_eq!(answered, 2);
        assert_eq!(host.seen.len(), 2, "both lines reached the host");
        assert_eq!(
            String::from_utf8(to).expect("utf-8"),
            "{\"result\":\"pong\"}\n{\"result\":\"pong\"}\n"
        );
    }

    #[test]
    fn a_notification_writes_nothing_back() {
        let mut from = Cursor::new(b"{\"method\":\"note\"}\n".to_vec());
        let mut to = Vec::new();
        let mut host = Pinger { seen: Vec::new() };

        let answered = serve(&mut from, &mut to, &mut host, 1024).expect("a clean session");

        assert_eq!(answered, 0, "nothing was answered");
        assert_eq!(host.seen.len(), 1, "but the host still saw it");
        assert!(to.is_empty(), "and nothing was written back");
    }

    #[test]
    fn an_empty_pipe_ends_the_session_quietly() {
        let mut from = Cursor::new(Vec::new());
        let mut to = Vec::new();
        let mut host = Pinger { seen: Vec::new() };

        let answered =
            serve(&mut from, &mut to, &mut host, 1024).expect("end of input is not an error");

        assert_eq!(answered, 0);
        assert!(host.seen.is_empty());
    }

    #[test]
    fn a_last_line_without_a_newline_is_still_answered() {
        let mut from = Cursor::new(b"{\"method\":\"ping\"}".to_vec());
        let mut to = Vec::new();
        let mut host = Pinger { seen: Vec::new() };

        let answered = serve(&mut from, &mut to, &mut host, 1024).expect("a clean session");

        assert_eq!(answered, 1, "the unterminated line counts");
        assert_eq!(
            String::from_utf8(to).expect("utf-8"),
            "{\"result\":\"pong\"}\n"
        );
    }

    #[test]
    fn a_line_past_the_cap_is_refused_rather_than_buffered() {
        let mut from = Cursor::new(vec![b'x'; 64]);
        let mut to = Vec::new();
        let mut host = Pinger { seen: Vec::new() };

        match serve(&mut from, &mut to, &mut host, 32) {
            Err(BridgeError::TooLong { cap }) => assert_eq!(cap, 32),
            other => panic!("expected TooLong, got {other:?}"),
        }
        assert!(host.seen.is_empty(), "the host never saw the line");
    }

    #[test]
    fn a_blank_line_is_skipped() {
        let mut from = Cursor::new(b"\n{\"method\":\"ping\"}\n\n".to_vec());
        let mut to = Vec::new();
        let mut host = Pinger { seen: Vec::new() };

        let answered = serve(&mut from, &mut to, &mut host, 1024).expect("a clean session");

        assert_eq!(answered, 1, "one request among the blanks");
        assert_eq!(host.seen.len(), 1);
    }
}
