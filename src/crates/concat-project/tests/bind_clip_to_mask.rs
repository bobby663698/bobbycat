// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! `bindClipToMask`: a clip that rides a tracked path.
//!
//! The two ends of this live in different clocks - a mask's keys are
//! fractions of the clip it sits on, a driven clip's keys are fractions of
//! its own length - so what is checked here is the remapping, the distance
//! kept from the box, and the stops that fall where the clip is not.

use concat_project::{Command, DocumentSettings, Editor, FollowPoint};

const MINIMAL: &str = include_str!("fixtures/minimal-project.json");

fn settings() -> DocumentSettings {
    DocumentSettings {
        name: "bind".to_owned(),
        width: 1920,
        height: 1080,
        rate_num: 30,
        rate_den: 1,
        canvas: [0, 0, 0, 255],
    }
}

/// The fixture with a mask on `c1` (0–4 s) and `c2` (0–2.5 s) as the clip
/// that will be made to follow it.
fn document() -> serde_json::Value {
    let mut document: serde_json::Value =
        serde_json::from_str(MINIMAL).expect("the fixture is JSON");
    document["timelines"][0]["clips"][0]["masks"] = serde_json::json!([{ "id": "m1" }]);
    document
}

fn tracked(editor: &mut Editor, path: Vec<FollowPoint>) {
    editor
        .apply(Command::FollowMask {
            clip_id: "c1".to_owned(),
            mask_id: "m1".to_owned(),
            path,
        })
        .expect("the path lands");
}

fn stop(at: f64, x: f64, y: f64) -> FollowPoint {
    FollowPoint {
        at,
        x,
        y,
        width: 0.2,
        height: 0.2,
    }
}

/// One key read back: where in the clip, and the value there.
type Stop = (f64, f64);

fn offsets(editor: &Editor) -> (Vec<Stop>, Vec<Stop>) {
    let document = editor.to_document(&settings());
    let clip = &document["timelines"][0]["clips"][1];
    // A clip's keys are one list, each entry naming its property; a mask's are
    // grouped by number. This is the former.
    let read = |name: &str| {
        clip["keys"]
            .as_array()
            .map(|all| {
                all.iter()
                    .filter(|key| key["property"] == name)
                    .map(|key| {
                        (
                            key["at"].as_f64().expect("at"),
                            key["value"].as_f64().expect("value"),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    (read("offsetX"), read("offsetY"))
}

#[test]
fn a_stop_outside_the_driven_clip_is_dropped_not_clamped() {
    let document = document();
    let mut editor = Editor::from_document(&document).expect("the project opens");
    // 0, 1 and 3 seconds on a four-second clip; c2 ends at 2.5 s.
    tracked(
        &mut editor,
        vec![
            stop(0.0, 0.0, 0.0),
            stop(0.25, 0.1, 0.0),
            stop(0.75, 0.2, 0.0),
        ],
    );
    let outcome = editor
        .apply(Command::BindClipToMask {
            clip_id: "c2".to_owned(),
            mask_clip_id: "c1".to_owned(),
            mask_id: "m1".to_owned(),
            shift_x: 0.0,
            shift_y: 0.0,
        })
        .expect("the bind applies");
    assert!(outcome.applied);

    let (xs, _) = offsets(&editor);
    assert_eq!(xs.len(), 2, "the stop at 3 s is off the clip");
    assert_eq!(xs[0].0, 0.0, "0 s is 0 of c2's 2.5 s");
    assert!((xs[1].0 - 0.4).abs() < 1e-9, "1 s is 0.4 of 2.5 s");
}

#[test]
fn the_distance_from_the_box_is_kept() {
    let document = document();
    let mut editor = Editor::from_document(&document).expect("the project opens");
    // A sticker above the head: 0.3 to the right and 0.1 up from the box.
    tracked(&mut editor, vec![stop(0.0, 0.0, 0.0), stop(0.5, 0.2, 0.2)]);
    editor
        .apply(Command::BindClipToMask {
            clip_id: "c2".to_owned(),
            mask_clip_id: "c1".to_owned(),
            mask_id: "m1".to_owned(),
            shift_x: 0.3,
            shift_y: -0.1,
        })
        .expect("the bind applies");

    let (xs, ys) = offsets(&editor);
    assert_eq!(xs.len(), 2);
    assert_eq!(xs[0].1, 0.3, "the box plus the shift");
    assert!((xs[1].1 - 0.5).abs() < 1e-9, "the path's own 0.2, plus 0.3");
    assert!((ys[0].1 + 0.1).abs() < 1e-9);
    assert!((ys[1].1 - 0.1).abs() < 1e-9);
}

#[test]
fn a_mask_with_no_path_changes_nothing() {
    let document = document();
    let mut editor = Editor::from_document(&document).expect("the project opens");
    let outcome = editor
        .apply(Command::BindClipToMask {
            clip_id: "c2".to_owned(),
            mask_clip_id: "c1".to_owned(),
            mask_id: "m1".to_owned(),
            shift_x: 0.0,
            shift_y: 0.0,
        })
        .expect("a mask with no path is a no-op");
    assert!(!outcome.applied);
}

#[test]
fn an_unknown_clip_or_mask_changes_nothing() {
    let document = document();
    let mut editor = Editor::from_document(&document).expect("the project opens");
    tracked(&mut editor, vec![stop(0.0, 0.0, 0.0)]);

    for (clip, mask_clip, mask) in [
        ("no-clip", "c1", "m1"),
        ("c2", "no-clip", "m1"),
        ("c2", "c1", "no-mask"),
    ] {
        let outcome = editor
            .apply(Command::BindClipToMask {
                clip_id: clip.to_owned(),
                mask_clip_id: mask_clip.to_owned(),
                mask_id: mask.to_owned(),
                shift_x: 0.0,
                shift_y: 0.0,
            })
            .expect("an unknown target is a no-op");
        assert!(!outcome.applied, "{clip} / {mask_clip} / {mask}");
    }
}

#[test]
fn one_undo_takes_the_whole_binding_off() {
    let document = document();
    let mut editor = Editor::from_document(&document).expect("the project opens");
    tracked(
        &mut editor,
        vec![
            stop(0.0, 0.0, 0.0),
            stop(0.5, 0.2, 0.0),
            stop(1.0, 0.4, 0.0),
        ],
    );
    editor
        .apply(Command::BindClipToMask {
            clip_id: "c2".to_owned(),
            mask_clip_id: "c1".to_owned(),
            mask_id: "m1".to_owned(),
            shift_x: 0.0,
            shift_y: 0.0,
        })
        .expect("the bind applies");
    // c2 runs 2.5 s: the stop at 4 s is off it.
    assert_eq!(offsets(&editor).0.len(), 2);

    assert!(editor.undo(), "the binding comes off in one step");
    assert!(offsets(&editor).0.is_empty());

    assert!(editor.undo(), "and the path comes off in one more");
    let document = editor.to_document(&settings());
    assert!(
        document["timelines"][0]["clips"][0]["masks"][0]["keys"]
            .get("x")
            .is_none()
    );
}

#[test]
fn binding_again_replaces_the_first_binding() {
    let document = document();
    let mut editor = Editor::from_document(&document).expect("the project opens");
    tracked(&mut editor, vec![stop(0.0, 0.0, 0.0), stop(1.0, 0.4, 0.0)]);
    let bind = |editor: &mut Editor| {
        editor
            .apply(Command::BindClipToMask {
                clip_id: "c2".to_owned(),
                mask_clip_id: "c1".to_owned(),
                mask_id: "m1".to_owned(),
                shift_x: 0.0,
                shift_y: 0.0,
            })
            .expect("the bind applies")
    };
    bind(&mut editor);
    // A second run of the tracker rewrites the path; binding again must not
    // leave the first path's keys behind.
    tracked(&mut editor, vec![stop(0.0, 0.5, 0.0)]);
    bind(&mut editor);

    let (xs, _) = offsets(&editor);
    assert_eq!(xs.len(), 1, "one stop, not three");
    assert_eq!(xs[0].1, 0.5);
}
