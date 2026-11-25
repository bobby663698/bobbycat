// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The one call, against a stub endpoint on loopback.
//!
//! A real request is made and a real SSE stream is read, so the wiring is
//! proven rather than argued: if `concat-autoedit` stops depending on
//! `concat-agent`, or the reply stops being parsed, these fail. Nothing
//! leaves the machine - the "endpoint" is a `TcpListener` on `127.0.0.1` that
//! says one thing, once.

use concat_agent::Endpoint;
use concat_autoedit::{
    Brief, ParseError, ProposeError, Verdict, adopt, check, messages, propose, timeline_of,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;

/// A candidate as the model would write it: one command the editor knows.
const REPLY: &str = r#"[
  {"id": "cold-open", "title": "Cold open", "rationale": "Strongest frame first.",
   "commands": [{"op": "trimClip", "clipId": "c1", "edge": "start", "delta": 0.5}]}
]"#;

/// Serves `reply` as one OpenAI-shaped SSE stream, once, and hands back the
/// endpoint that reaches it.
fn stub(reply: &'static str) -> Endpoint {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let port = listener.local_addr().expect("the port it got").port();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        // The request is read to the end of its body before answering:
        // closing underneath a client that is still writing costs it a reset,
        // not an answer.
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            match stream.read(&mut byte) {
                Ok(1) => head.push(byte[0]),
                _ => return,
            }
        }
        let text = String::from_utf8_lossy(&head).to_ascii_lowercase();
        let length: usize = text
            .split("content-length:")
            .nth(1)
            .and_then(|rest| rest.split("\r\n").next())
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; length];
        if stream.read_exact(&mut body).is_err() {
            return;
        }
        let event = serde_json::json!({"choices": [{"delta": {"content": reply}}]});
        let payload = format!("data: {event}\n\ndata: [DONE]\n\n");
        let response = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: text/event-stream\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\r\n{payload}",
            payload.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    });
    Endpoint::custom("stub", format!("http://127.0.0.1:{port}/v1"), "stub-model")
}

#[test]
fn one_round_comes_back_as_candidates() {
    let endpoint = stub(REPLY);
    let brief = Brief {
        goal: "cut this into a 60 second teaser".to_owned(),
        target_seconds: Some(60.0),
        tone: Some("brisk".to_owned()),
    };
    let candidates = propose(&endpoint, &brief, r#"{"timelines": []}"#)
        .expect("the stub answers with a candidate");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, "cold-open");
    assert_eq!(candidates[0].commands.len(), 1);
}

#[test]
fn a_blocked_reply_is_an_answer_not_a_failure() {
    let endpoint = stub("BLOCKED: the main voice track is missing.");
    match propose(&endpoint, &Brief::default(), "{}") {
        Err(ProposeError::Reply(ParseError::Blocked(gap))) => {
            assert!(gap.contains("voice track"), "got {gap}");
        }
        other => panic!("BLOCKED must arrive as a gap, got {other:?}"),
    }
}

#[test]
fn a_dead_endpoint_is_a_transport_error() {
    // Bound then dropped: a port on this machine that nothing now listens on.
    let port = {
        let probe = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        probe.local_addr().expect("the port").port()
    };
    let endpoint = Endpoint::custom("dead", format!("http://127.0.0.1:{port}/v1"), "none");
    let outcome = propose(&endpoint, &Brief::default(), "{}");
    assert!(
        matches!(outcome, Err(ProposeError::Agent(_))),
        "expected a transport failure, got {outcome:?}"
    );
}

#[test]
fn the_messages_are_one_user_turn_carrying_the_contract() {
    let brief = Brief {
        goal: "a 30 second cut".to_owned(),
        ..Brief::default()
    };
    let sent = messages(&brief, r#"{"clips": [{"id": "c1"}]}"#);
    assert_eq!(sent.len(), 1, "one turn, no second copy of the rules");
    assert_eq!(sent[0].role, concat_agent::Role::User);
    let text = &sent[0].content;
    assert!(text.contains("a 30 second cut"));
    assert!(text.contains("\"c1\""), "the timeline's ids are sent whole");
    for number in 1..=12 {
        assert!(
            text.contains(&format!("\n{number}. ")),
            "rule {number} is missing"
        );
    }
    assert!(text.contains("BLOCKED:"));
}

/// A project folder holding the engine's minimal document, under a name
/// nothing else holds.
fn project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "concat-autoedit-agent-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let document = include_str!("../../concat-project/tests/fixtures/minimal-project.json");
    std::fs::create_dir_all(&dir).expect("scratch project");
    std::fs::write(dir.join("concat.json"), document).expect("writes the document");
    dir
}

#[test]
fn the_whole_chain_runs_from_a_folder_to_an_adopted_cut() {
    let dir = project("chain");
    let timeline = timeline_of(&dir).expect("the document reads");
    assert!(
        timeline.starts_with('{') && !timeline.contains("\n"),
        "the summary is compact JSON, not the pretty file"
    );

    let brief = Brief {
        goal: "open on the strongest frame".to_owned(),
        ..Brief::default()
    };
    let candidates = propose(&stub(REPLY), &brief, &timeline).expect("the stub proposes");
    let candidate = &candidates[0];

    // The commands name ids the document holds, so they apply and change it.
    match check(&dir, candidate).expect("the candidate is tried") {
        Verdict::Clean(attempt) => assert_eq!(attempt.applied, vec![true]),
        other => panic!("the stub's cut must be adoptable, got {other:?}"),
    }

    let diff = adopt(&dir, candidate)
        .expect("adoption runs")
        .expect("a clean candidate writes");
    assert!(diff.changed);

    // The trim landed in the file itself, not only in the copy.
    let before = include_str!("../../concat-project/tests/fixtures/minimal-project.json");
    let after = std::fs::read_to_string(dir.join("concat.json")).expect("reads back");
    assert_ne!(after, before, "adoption wrote the adopted document");
    let _ = std::fs::remove_dir_all(&dir);
}
