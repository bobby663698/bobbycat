// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The copy, the gate, and the adoption, on the same minimal project the
//! engine's stage 0 tests use.

use concat_autoedit::{Candidate, ProjectCopy, Verdict, adopt, check, parse};
use concat_project::Command;
use concat_project::commands::TrimEdge;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

const MINIMAL: &str = include_str!("../../concat-project/tests/fixtures/minimal-project.json");

/// A project folder holding the fixture, under a name nothing else holds.
fn project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "concat-autoedit-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&dir).expect("scratch project");
    fs::write(dir.join("concat.json"), MINIMAL).expect("writes the document");
    dir
}

fn retime() -> Command {
    Command::SetClipSpeed {
        clip_id: "c1".to_owned(),
        speed: 2.0,
        ripple: false,
    }
}

fn unknown_clip() -> Command {
    Command::TrimClip {
        clip_id: "no-such-clip".to_owned(),
        edge: TrimEdge::Start,
        delta: 1.0,
        ripple: false,
    }
}

#[test]
fn a_copy_tries_commands_without_touching_the_project() {
    let dir = project("copy");
    let before = fs::read_to_string(dir.join("concat.json")).expect("reads");
    let root = {
        let copy = ProjectCopy::of(&dir).expect("copy");
        let attempt = copy.try_commands(&[retime()]).expect("applies");
        assert_eq!(attempt.applied, vec![true], "the retime changed something");
        assert!(attempt.diff.changed, "and the document says so");
        assert!(
            attempt.diff.notes.iter().any(|note| note.contains("c1")),
            "the note names the clip: {:?}",
            attempt.diff.notes
        );
        copy.root().to_path_buf()
    };
    assert_eq!(
        fs::read_to_string(dir.join("concat.json")).expect("reads"),
        before,
        "the project's own document was never opened for writing"
    );
    assert!(!root.exists(), "and the copy went away with the guard");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_wrong_id_is_inert_and_never_clean() {
    let dir = project("inert");
    let candidate = Candidate::new(
        "wrong-id",
        "Names a clip that is gone",
        vec![unknown_clip()],
    );
    match check(&dir, &candidate).expect("the check runs") {
        Verdict::Inert(attempt) => {
            assert_eq!(attempt.applied, vec![false], "no error, and no change");
            assert!(!attempt.diff.changed);
            assert!(attempt.diff.notes.is_empty(), "{:?}", attempt.diff.notes);
        }
        other => panic!("a wrong id is not an error and not a cut: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_refusal_carries_the_index_of_the_member() {
    let dir = project("refused");
    let candidate = Candidate::new(
        "bad-media",
        "Imports something that is not there",
        vec![
            retime(),
            Command::AddClip {
                media_id: "no-such-media".to_owned(),
                track_id: "t1".to_owned(),
                start: 9.0,
                ripple: false,
            },
        ],
    );
    match check(&dir, &candidate).expect("the check runs") {
        Verdict::Rejected(rejection) => {
            assert_eq!(rejection.command, Some(1), "the second member refused");
            assert!(!rejection.why.is_empty(), "and it says why");
        }
        other => panic!("expected a rejection: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn adopting_writes_the_document_and_leaves_the_rest_alone() {
    let dir = project("adopt");
    let before: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("concat.json")).expect("reads"))
            .expect("json");
    let candidate = Candidate::new("halve", "Play it twice as fast", vec![retime()]);
    let diff = adopt(&dir, &candidate)
        .expect("the adoption runs")
        .expect("something was written");
    assert!(diff.changed);
    let after: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("concat.json")).expect("reads"))
            .expect("json");
    let clip = after["timelines"][0]["clips"]
        .as_array()
        .expect("clips")
        .iter()
        .find(|clip| clip["id"] == "c1")
        .expect("c1");
    assert!(
        (clip["duration"].as_f64().expect("a number") - 2.0).abs() < 1e-9,
        "4s at 2x covers 2s: {clip:?}"
    );
    assert_eq!(after["version"], before["version"], "identity untouched");
    assert_eq!(
        after["activeTimelineId"], before["activeTimelineId"],
        "which timeline is showing is not the candidate's to change"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_inert_candidate_writes_nothing() {
    let dir = project("no-write");
    let before = fs::read_to_string(dir.join("concat.json")).expect("reads");
    let candidate = Candidate::new("nothing", "Does nothing", vec![unknown_clip()]);
    let written = adopt(&dir, &candidate).expect("the adoption runs");
    assert!(written.is_none(), "nothing to write");
    assert_eq!(
        fs::read_to_string(dir.join("concat.json")).expect("reads"),
        before,
        "and the file was not touched"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_reply_inside_a_fence_parses() {
    let reply = "Here is a cut.\n\n```json\n[\
        {\"id\": \"a\", \"title\": \"Trim the head\", \"commands\": \
        [{\"op\": \"trimClip\", \"clipId\": \"c1\", \"edge\": \"start\", \"delta\": 0.5}]}\
    ]\n```\n";
    let candidates = parse(reply).expect("parses");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, "a");
    assert_eq!(candidates[0].commands.len(), 1);
}

#[test]
fn a_candidate_with_no_commands_is_refused_before_a_copy_is_made() {
    let dir = project("empty");
    let candidate = Candidate::new("empty", "Says nothing", Vec::new());
    match check(&dir, &candidate).expect("the check runs") {
        Verdict::Rejected(rejection) => {
            assert_eq!(rejection.command, None, "the whole candidate is at fault");
        }
        other => panic!("expected a rejection: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}
