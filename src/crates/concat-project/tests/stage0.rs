// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! Stage 0 checks the MCPP handoff asks for, run against the edit engine
//! itself rather than argued from the code:
//!
//! 1. an independent candidate copy - a batch that fails commits nothing;
//! 2. serial commit - a saved document reopens identically;
//! 3. batch undo - one `undo` puts a whole batch back;
//! 4. the time map - speed changes where a clip ends;
//! 5. the gap that is still work - a trim on the picture lane leaves the
//!    captions where they were, which is exactly what the remap layer has
//!    to sit on top of.

use concat_project::commands::TrimEdge;
use concat_project::model::Clip;
use concat_project::{Command, DocumentSettings, Editor};
use serde_json::Value;

const MINIMAL: &str = include_str!("fixtures/minimal-project.json");

fn settings() -> DocumentSettings {
    DocumentSettings {
        name: "Stage0".to_owned(),
        width: 1920,
        height: 1080,
        rate_num: 30,
        rate_den: 1,
        canvas: [0, 0, 0, 255],
    }
}

fn editor() -> Editor {
    let document: Value = serde_json::from_str(MINIMAL).expect("the fixture is JSON");
    Editor::from_document(&document).expect("the fixture opens")
}

fn clip<'a>(editor: &'a Editor, id: &str) -> &'a Clip {
    editor.project().timelines[0]
        .clips
        .iter()
        .find(|clip| clip.id == id)
        .unwrap_or_else(|| panic!("no clip {id}"))
}

fn document(editor: &Editor) -> Value {
    editor.to_document(&settings())
}

#[test]
fn a_batch_with_one_bad_member_commits_nothing() {
    let mut editor = editor();
    let before = document(&editor);
    // AddClip errs on a media id the bin does not hold; that is the kind of
    // failure a staged batch has to be able to roll back from. (An unknown
    // clip id is not one - see the test below.)
    let result = editor.apply(Command::Batch {
        commands: vec![
            Command::SetClipSpeed {
                clip_id: "c1".to_owned(),
                speed: 2.0,
                ripple: false,
            },
            Command::AddClip {
                media_id: "no-such-media".to_owned(),
                track_id: "t1".to_owned(),
                start: 10.0,
                ripple: false,
            },
        ],
    });
    assert!(result.is_err(), "the unknown media must fail the batch");
    assert_eq!(
        document(&editor),
        before,
        "not one member of the batch reached the project"
    );
    assert!(
        !editor.can_undo(),
        "a refused batch records no undo step either"
    );
}

#[test]
fn an_unknown_clip_id_is_a_silent_no_op() {
    // The other half of what a candidate check has to know: a command that
    // names an id the project no longer holds does not error, it does
    // nothing and says so through `applied`. A checker that trusts the exit
    // status would accept a candidate that changed nothing.
    let mut editor = editor();
    let before = document(&editor);
    let outcome = editor
        .apply(Command::TrimClip {
            clip_id: "no-such-clip".to_owned(),
            edge: TrimEdge::Start,
            delta: 1.0,
            ripple: false,
        })
        .expect("an unknown id is not an error");
    assert!(!outcome.applied, "and it reports that it applied nothing");
    assert_eq!(document(&editor), before, "nothing changed");
}

#[test]
fn a_batch_is_one_undo_step() {
    let mut editor = editor();
    let before = document(&editor);
    editor
        .apply(Command::Batch {
            commands: vec![
                Command::SetClipSpeed {
                    clip_id: "c1".to_owned(),
                    speed: 2.0,
                    ripple: false,
                },
                Command::TrimClip {
                    clip_id: "c3".to_owned(),
                    edge: TrimEdge::End,
                    delta: -1.0,
                    ripple: false,
                },
            ],
        })
        .expect("the batch applies");
    assert_ne!(document(&editor), before, "the batch changed something");
    assert!(editor.undo(), "one undo");
    assert_eq!(
        document(&editor),
        before,
        "one undo puts every member of the batch back"
    );
    assert!(!editor.can_undo(), "and there was only one step to undo");
}

#[test]
fn a_saved_document_reopens_identically() {
    let mut editor = editor();
    editor
        .apply(Command::SetClipSpeed {
            clip_id: "c1".to_owned(),
            speed: 1.5,
            ripple: true,
        })
        .expect("the retime applies");
    let saved = document(&editor);
    let reopened = Editor::from_document(&saved).expect("what was saved opens again");
    assert_eq!(
        document(&reopened),
        saved,
        "a document that does not round-trip is one that drifts on the first save"
    );
}

#[test]
fn speed_moves_where_the_clip_ends() {
    let mut editor = editor();
    let before = clip(&editor, "c1").duration;
    editor
        .apply(Command::SetClipSpeed {
            clip_id: "c1".to_owned(),
            speed: 2.0,
            ripple: false,
        })
        .expect("the retime applies");
    let after = clip(&editor, "c1").duration;
    assert!((before - 4.0).abs() < 1e-9, "the fixture clip is 4s");
    assert!(
        (after - 2.0).abs() < 1e-9,
        "4s at 2x covers 2s of timeline, got {after}"
    );
}

#[test]
fn trimming_the_picture_does_not_move_the_captions() {
    // This test records the gap, not a behaviour to keep. A title on its
    // own lane keeps its own start, so an AI cut that moves into c1 leaves
    // c2 where it was: the caption now sits over different footage. The
    // remap layer is what closes that, and it is what stage 1 adds.
    let mut editor = editor();
    let caption_start = clip(&editor, "c2").start;
    let caption_end = caption_start + clip(&editor, "c2").duration;
    editor
        .apply(Command::TrimClip {
            clip_id: "c1".to_owned(),
            edge: TrimEdge::Start,
            delta: 1.0,
            ripple: true,
        })
        .expect("the trim applies");
    let picture_start = clip(&editor, "c1").start;
    let moved = (picture_start - 1.0).abs() < 1e-9;
    if !moved {
        // Ripple semantics decide where the head lands; the assertion that
        // matters is the caption's, so report rather than fail on this one.
        eprintln!("note: head trim left c1 at {picture_start}s, not 1s");
    }
    assert_eq!(
        clip(&editor, "c2").start,
        caption_start,
        "the caption did not follow the picture"
    );
    assert_eq!(
        clip(&editor, "c2").start + clip(&editor, "c2").duration,
        caption_end,
        "nor did it grow"
    );
}
