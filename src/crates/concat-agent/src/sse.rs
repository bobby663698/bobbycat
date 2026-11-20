//! Server-sent events, the four lines of the spec a model stream actually uses.
//!
//! `data:` lines accumulate into one payload per blank line, `:` lines are
//! comments (the providers send them as keep-alives during a long think), and
//! every other field — `event:`, `id:`, `retry:` — is dropped. Nothing here
//! needs it: OpenAI puts the type inside the JSON, and Anthropic's
//! `content_block_delta` is identifiable from `data` alone.
//!
//! Read as bytes, not as `String`: a delta can split a multi-byte character
//! across two chunks, and decoding each line on its own would panic on the
//! half. `read_until(b'\n')` then `from_utf8` on the complete line is what
//! keeps Chinese and emoji intact.

use std::io::{BufRead, Error, ErrorKind};

/// A stream of SSE payloads over any buffered reader.
pub struct Sse<R> {
    reader: R,
    line: Vec<u8>,
}

impl<R: BufRead> Sse<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            line: Vec::with_capacity(256),
        }
    }

    /// The next event's payload, or `None` at end of stream.
    ///
    /// Returns `Ok(None)` both at a clean EOF and at EOF after a final event
    /// with no trailing blank line — providers differ on whether they send one.
    pub fn next_event(&mut self) -> Result<Option<String>, Error> {
        let mut data = String::new();
        let mut seen = false;

        loop {
            self.line.clear();
            let n = self.reader.read_until(b'\n', &mut self.line)?;
            if n == 0 {
                return if seen { Ok(Some(data)) } else { Ok(None) };
            }

            let text = std::str::from_utf8(&self.line)
                .map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
            let text = text.trim_end_matches(['\n', '\r']);

            if text.is_empty() {
                if seen {
                    return Ok(Some(data));
                }
                continue;
            }

            if text.starts_with(':') {
                continue;
            }

            if let Some(rest) = text.strip_prefix("data:") {
                seen = true;
                let rest = rest.strip_prefix(' ').unwrap_or(rest);
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn events(raw: &str) -> Vec<String> {
        let mut sse = Sse::new(Cursor::new(raw.as_bytes().to_vec()));
        let mut out = Vec::new();
        while let Some(e) = sse.next_event().unwrap() {
            out.push(e);
        }
        out
    }

    #[test]
    fn payloads_are_split_on_blank_lines() {
        assert_eq!(events("data: a\n\ndata: b\n\n"), vec!["a", "b"]);
    }

    #[test]
    fn comments_are_keepalives_not_payloads() {
        assert_eq!(events(": ping\n\ndata: x\n\n"), vec!["x"]);
    }

    #[test]
    fn multi_line_data_joins_with_newline() {
        assert_eq!(events("data: one\ndata: two\n\n"), vec!["one\ntwo"]);
    }

    #[test]
    fn eof_without_blank_line_still_yields_the_event() {
        assert_eq!(events("data: tail"), vec!["tail"]);
    }

    #[test]
    fn other_fields_are_ignored() {
        assert_eq!(events("event: delta\nid: 7\ndata: z\n\n"), vec!["z"]);
    }

    #[test]
    fn chinese_survives_the_split() {
        assert_eq!(events("data: {\"t\":\"好\"}\n\n"), vec!["{\"t\":\"好\"}"]);
    }
}
