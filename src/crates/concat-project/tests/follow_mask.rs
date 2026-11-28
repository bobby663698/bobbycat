// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! `followMask`: a tracked path, from a tracker that lives elsewhere, landing
//! on a mask's own numbers.
//!
//! The path is carried, not computed - nothing in `concat-project` reads a
//! pixel. What is checked here is the part this crate owns: the order the
//! stops are written in, which numbers they become, and the fact that the
//! whole path is one undo step.

use concat_project::{Command, DocumentSettings, Editor, FollowPoint, from_document};

const MINIMAL: &str = include_str!("fixtures/minimal-project.json");

fn settings() -> DocumentSettings {
    DocumentSettings {
        name: "follow".to_owned(),
        width: 1920,
        height: 1080,
        rate_num: 30,
        rate_den: 1,
        canvas: [0, 0, 0, 255],
    }
}

/// The minimal project with one mask on its first clip.
fn document_with_a_mask() -> serde_json::Value {
    let mut document: serde_json::Value =
        serde_json::from_str(MINIMAL).expect("the fixture is JSON");
    document["timelines"][0]["clips"][0]["masks"] = serde_json::json!([{ "id": "m1" }]);
    document
}

fn stop(at: f64, x: f64, width: f64) -> FollowPoint {
    FollowPoint {
        at,
        x,
        y: 0.0,
        width,
        height: width,
    }
}

#[test]
fn a_path_lands_as_keys_on_x_and_y() {
    let document = document_with_a_mask();
    let mut editor = Editor::from_document(&document).expect("the project opens");
    let path = vec![stop(0.0, 0.0, 0.2), stop(0.5, 0.1, 0.2)];
    let outcome = editor
        .apply(Command::FollowMask {
            clip_id: "c1".to_owned(),
            mask_id: "m1".to_owned(),
            path,
        })
        .expect("the command applies");
    assert!(outcome.applied, "a path changes the mask");

    let after = editor.to_document(&settings());
    let mask = &after["timelines"][0]["clips"][0]["masks"][0];
    let xs = mask["keys"]["x"].as_array().expect("x is keyed");
    let ys = mask["keys"]["y"].as_array().expect("y is keyed");
    assert_eq!(xs.len(), 2, "one key per stop");
    assert_eq!(ys.len(), 2);
    assert_eq!(xs[0]["at"], 0.0);
    assert_eq!(xs[1]["at"], 0.5);
    assert_eq!(xs[1]["value"], 0.1);
}

#[test]
fn the_path_is_ordered_by_at_whatever_the_tracker_reported() {
    let document = document_with_a_mask();
    let mut editor = Editor::from_document(&document).expect("the project opens");
    // Reported out of order on purpose: the first stop becomes what the
    // mask's own fields fall back to, so the order cannot be the caller's.
    let path = vec![
        stop(0.8, 0.3, 0.2),
        stop(0.0, 0.05, 0.2),
        stop(0.4, 0.2, 0.2),
    ];
    editor
        .apply(Command::FollowMask {
            clip_id: "c1".to_owned(),
            mask_id: "m1".to_owned(),
            path,
        })
        .expect("the command applies");

    let after = editor.to_document(&settings());
    let mask = &after["timelines"][0]["clips"][0]["masks"][0];
    let xs = mask["keys"]["x"].as_array().expect("x is keyed");
    assert_eq!(xs[0]["at"], 0.0, "sorted, not as given");
    assert_eq!(mask["x"], 0.05, "the fields keep the earliest stop");
}

#[test]
fn a_box_that_keeps_its_size_keys_only_its_position() {
    let document = document_with_a_mask();
    let mut editor = Editor::from_document(&document).expect("the project opens");
    editor
        .apply(Command::FollowMask {
            clip_id: "c1".to_owned(),
            mask_id: "m1".to_owned(),
            path: vec![stop(0.0, 0.0, 0.25), stop(1.0, 0.2, 0.25)],
        })
        .expect("the command applies");

    let after = editor.to_document(&settings());
    let mask = &after["timelines"][0]["clips"][0]["masks"][0];
    assert!(
        mask["keys"].get("width").is_none(),
        "a size that never changes is a field, not a key per frame"
    );
    assert_eq!(mask["keys"]["x"].as_array().map(Vec::len), Some(2));
}

#[test]
fn an_empty_path_or_an_unknown_target_changes_nothing() {
    let document = document_with_a_mask();
    let mut editor = Editor::from_document(&document).expect("the project opens");

    let empty = editor
        .apply(Command::FollowMask {
            clip_id: "c1".to_owned(),
            mask_id: "m1".to_owned(),
            path: Vec::new(),
        })
        .expect("an empty path is a no-op");
    assert!(!empty.applied);

    let nowhere = editor
        .apply(Command::FollowMask {
            clip_id: "no-such-clip".to_owned(),
            mask_id: "m1".to_owned(),
            path: vec![stop(0.0, 0.0, 0.2)],
        })
        .expect("an unknown clip is a no-op");
    assert!(!nowhere.applied);

    let no_mask = editor
        .apply(Command::FollowMask {
            clip_id: "c1".to_owned(),
            mask_id: "no-such-mask".to_owned(),
            path: vec![stop(0.0, 0.0, 0.2)],
        })
        .expect("an unknown mask is a no-op");
    assert!(!no_mask.applied);
}

#[test]
fn the_whole_path_is_one_undo_step() {
    let document = document_with_a_mask();
    let mut editor = Editor::from_document(&document).expect("the project opens");
    editor
        .apply(Command::FollowMask {
            clip_id: "c1".to_owned(),
            mask_id: "m1".to_owned(),
            path: vec![
                stop(0.0, 0.0, 0.2),
                stop(0.5, 0.1, 0.2),
                stop(1.0, 0.25, 0.2),
            ],
        })
        .expect("the command applies");
    assert!(
        editor.to_document(&settings())["timelines"][0]["clips"][0]["masks"][0]["keys"]
            .get("x")
            .is_some()
    );

    assert!(editor.undo(), "one step came off");
    let after = editor.to_document(&settings());
    let mask = &after["timelines"][0]["clips"][0]["masks"][0];
    assert!(
        mask["keys"].get("x").is_none(),
        "one undo takes the whole path off, not one key of it"
    );
}

#[test]
fn a_second_run_replaces_the_first_rather_than_adding_to_it() {
    let document = document_with_a_mask();
    let mut editor = Editor::from_document(&document).expect("the project opens");
    let command = |path| Command::FollowMask {
        clip_id: "c1".to_owned(),
        mask_id: "m1".to_owned(),
        path,
    };
    editor
        .apply(command(vec![stop(0.0, 0.0, 0.2), stop(1.0, 0.4, 0.2)]))
        .expect("the first run applies");
    editor
        .apply(command(vec![stop(0.0, 0.6, 0.2), stop(1.0, 0.6, 0.2)]))
        .expect("a correction applies");

    let after = editor.to_document(&settings());
    let xs = after["timelines"][0]["clips"][0]["masks"][0]["keys"]["x"]
        .as_array()
        .expect("x is keyed");
    assert_eq!(xs.len(), 2, "two stops, not four");
    assert_eq!(xs[1]["value"], 0.6, "the correction won");
}

#[test]
fn a_non_finite_stop_is_refused_before_anything_is_written() {
    let document = document_with_a_mask();
    let mut editor = Editor::from_document(&document).expect("the project opens");
    let refused = editor.apply(Command::FollowMask {
        clip_id: "c1".to_owned(),
        mask_id: "m1".to_owned(),
        path: vec![stop(0.0, f64::NAN, 0.2)],
    });
    assert!(refused.is_err(), "a NaN box is not a path");
    assert!(
        from_document(&editor.to_document(&settings())).is_some(),
        "and the project still reads"
    );
}
