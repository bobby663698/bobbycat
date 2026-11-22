// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! Bookmarks: named instants on a timeline, the way OpenCut's bookmark
//! command marks a beat to jump back to.
//!
//! A bookmark is a read-only marker. It never touches a clip, so none of the
//! edit commands have to know bookmarks exist. What is held here:
//!
//! - they are kept in time order regardless of insertion order;
//! - a blank label is refused, so a mark is never nameless;
//! - removing or renaming an unknown id is a tolerated no-op;
//! - they survive a save and a reload, and a document made before bookmarks
//!   existed opens with none.

use concat_project::commands::Command;
use concat_project::{DocumentSettings, Editor, from_document, to_document};

fn settings() -> DocumentSettings {
    DocumentSettings {
        name: "Bookmarks".to_owned(),
        width: 1920,
        height: 1080,
        rate_num: 30,
        rate_den: 1,
        canvas: [0, 0, 0, 255],
    }
}

#[test]
fn a_bookmark_lands_at_its_time_with_its_label() {
    let mut editor = Editor::new();
    let id = editor
        .apply(Command::AddBookmark {
            at: 3.5,
            label: "  chorus  ".to_owned(),
        })
        .expect("a bookmark")
        .created_id
        .expect("an id");

    let marks = &editor.project().timelines[0].bookmarks;
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].id, id);
    assert_eq!(marks[0].at, 3.5);
    assert_eq!(marks[0].label, "chorus", "the label is trimmed");
}

#[test]
fn bookmarks_are_kept_in_time_order_not_insertion_order() {
    let mut editor = Editor::new();
    for (at, label) in [(9.0, "last"), (1.0, "first"), (5.0, "middle")] {
        editor
            .apply(Command::AddBookmark {
                at,
                label: label.to_owned(),
            })
            .expect("a bookmark");
    }
    let marks = &editor.project().timelines[0].bookmarks;
    let labels: Vec<&str> = marks.iter().map(|mark| mark.label.as_str()).collect();
    assert_eq!(labels, ["first", "middle", "last"]);
}

#[test]
fn a_negative_time_is_clamped_to_zero() {
    let mut editor = Editor::new();
    editor
        .apply(Command::AddBookmark {
            at: -4.0,
            label: "start".to_owned(),
        })
        .expect("a bookmark");
    assert_eq!(editor.project().timelines[0].bookmarks[0].at, 0.0);
}

#[test]
fn a_blank_label_is_ignored() {
    let mut editor = Editor::new();
    let outcome = editor
        .apply(Command::AddBookmark {
            at: 2.0,
            label: "   ".to_owned(),
        })
        .expect("a blank bookmark is a no-op, not an error");
    assert!(!outcome.applied);
    assert!(editor.project().timelines[0].bookmarks.is_empty());
}

#[test]
fn removing_and_renaming_an_unknown_id_are_no_ops() {
    let mut editor = Editor::new();
    assert!(
        !editor
            .apply(Command::RemoveBookmark {
                bookmark_id: "bm-none".to_owned()
            })
            .expect("no error")
            .applied
    );
    assert!(
        !editor
            .apply(Command::RenameBookmark {
                bookmark_id: "bm-none".to_owned(),
                label: "x".to_owned()
            })
            .expect("no error")
            .applied
    );
}

#[test]
fn a_bookmark_can_be_renamed_and_removed() {
    let mut editor = Editor::new();
    let id = editor
        .apply(Command::AddBookmark {
            at: 2.0,
            label: "old".to_owned(),
        })
        .expect("a bookmark")
        .created_id
        .expect("an id");

    editor
        .apply(Command::RenameBookmark {
            bookmark_id: id.clone(),
            label: "  new  ".to_owned(),
        })
        .expect("the rename");
    assert_eq!(editor.project().timelines[0].bookmarks[0].label, "new");

    editor
        .apply(Command::RemoveBookmark {
            bookmark_id: id.clone(),
        })
        .expect("the removal");
    assert!(editor.project().timelines[0].bookmarks.is_empty());
}

#[test]
fn a_blank_rename_does_not_erase_the_label() {
    let mut editor = Editor::new();
    editor
        .apply(Command::AddBookmark {
            at: 1.0,
            label: "keep me".to_owned(),
        })
        .expect("a bookmark");
    let id = editor.project().timelines[0].bookmarks[0].id.clone();

    editor
        .apply(Command::RenameBookmark {
            bookmark_id: id,
            label: "  ".to_owned(),
        })
        .expect("a no-op");
    assert_eq!(editor.project().timelines[0].bookmarks[0].label, "keep me");
}

#[test]
fn bookmarks_survive_a_save_and_a_reload() {
    let mut editor = Editor::new();
    editor
        .apply(Command::AddBookmark {
            at: 4.25,
            label: "beat".to_owned(),
        })
        .expect("a bookmark");

    let document = to_document(&settings(), editor.project());
    let again = from_document(&document).expect("what it wrote reopens");

    assert_eq!(again.timelines[0].bookmarks.len(), 1);
    assert_eq!(again.timelines[0].bookmarks[0].label, "beat");
    assert_eq!(again.timelines[0].bookmarks[0].at, 4.25);
}

#[test]
fn a_document_without_bookmarks_opens_with_none() {
    const MINIMAL: &str = include_str!("fixtures/minimal-project.json");
    let document: serde_json::Value = serde_json::from_str(MINIMAL).expect("the fixture is JSON");
    let project = from_document(&document).expect("the fixture still opens");
    for timeline in &project.timelines {
        assert!(timeline.bookmarks.is_empty(), "no bookmarks were invented");
    }
}
