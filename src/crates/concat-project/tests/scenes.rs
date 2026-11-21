// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! Scenes: a named group of timelines, held flat.
//!
//! The shape is the point. `Project::timelines` stays a single list and
//! `Timeline::scene` names the group, rather than timelines nesting inside
//! scenes. Every command that walks timelines - split, ripple, consolidate,
//! the whole clip layer - therefore keeps working without knowing scenes
//! exist, and a document written before scenes existed opens with all its
//! timelines ungrouped.
//!
//! What these tests hold to:
//!
//! - making a scene moves nothing into it; grouping is its own command;
//! - a second timeline is born in the scene you were looking at;
//! - removing a scene ungroups its timelines rather than deleting them,
//!   because a command named "remove scene" must not lose an afternoon;
//! - a scene id that names nothing ungroups rather than dangling.

use concat_project::commands::Command;
use concat_project::{DocumentSettings, Editor, from_document, to_document};

fn settings() -> DocumentSettings {
    DocumentSettings {
        name: "Scenes".to_owned(),
        width: 1920,
        height: 1080,
        rate_num: 30,
        rate_den: 1,
        canvas: [0, 0, 0, 255],
    }
}

#[test]
fn a_new_scene_starts_empty_and_moves_nothing_into_itself() {
    let mut editor = Editor::new();
    let created = editor
        .apply(Command::AddScene)
        .expect("a scene")
        .created_id
        .expect("an id");

    assert_eq!(created, "sc1");
    assert_eq!(editor.project().scenes.len(), 1);
    assert_eq!(editor.project().scenes[0].name, "Scene 1");
    assert_eq!(
        editor.project().timelines[0].scene,
        "",
        "the first timeline was not swept into the new scene"
    );
}

#[test]
fn a_timeline_put_in_a_scene_reads_back_from_it() {
    let mut editor = Editor::new();
    let scene = editor
        .apply(Command::AddScene)
        .expect("a scene")
        .created_id
        .expect("an id");

    editor
        .apply(Command::MoveTimelineToScene {
            timeline_id: "TL1".to_owned(),
            scene_id: scene.clone(),
        })
        .expect("the move");

    assert_eq!(editor.project().timelines[0].scene, scene);
}

#[test]
fn a_second_timeline_is_born_in_the_scene_you_were_looking_at() {
    let mut editor = Editor::new();
    let scene = editor
        .apply(Command::AddScene)
        .expect("a scene")
        .created_id
        .expect("an id");
    editor
        .apply(Command::MoveTimelineToScene {
            timeline_id: "TL1".to_owned(),
            scene_id: scene.clone(),
        })
        .expect("the move");

    let added = editor
        .apply(Command::AddTimeline)
        .expect("a timeline")
        .created_id
        .expect("an id");

    let born = editor
        .project()
        .timelines
        .iter()
        .find(|timeline| timeline.id == added)
        .expect("the new timeline is in the list");
    assert_eq!(
        born.scene, scene,
        "a second cut of the scene belongs to that scene"
    );
}

#[test]
fn removing_a_scene_ungroups_its_timelines_rather_than_deleting_them() {
    let mut editor = Editor::new();
    let scene = editor
        .apply(Command::AddScene)
        .expect("a scene")
        .created_id
        .expect("an id");
    editor
        .apply(Command::MoveTimelineToScene {
            timeline_id: "TL1".to_owned(),
            scene_id: scene.clone(),
        })
        .expect("the move");
    let before = editor.project().timelines.len();

    editor
        .apply(Command::RemoveScene { scene_id: scene })
        .expect("the removal");

    assert!(editor.project().scenes.is_empty(), "the scene is gone");
    assert_eq!(
        editor.project().timelines.len(),
        before,
        "the timeline outlives the scene it was in"
    );
    assert_eq!(
        editor.project().timelines[0].scene,
        "",
        "and reads as ungrouped again"
    );
}

#[test]
fn a_scene_id_that_names_nothing_ungroups_rather_than_dangling() {
    let mut editor = Editor::new();
    let scene = editor
        .apply(Command::AddScene)
        .expect("a scene")
        .created_id
        .expect("an id");
    editor
        .apply(Command::MoveTimelineToScene {
            timeline_id: "TL1".to_owned(),
            scene_id: scene,
        })
        .expect("the move");

    editor
        .apply(Command::MoveTimelineToScene {
            timeline_id: "TL1".to_owned(),
            scene_id: "sc-nonexistent".to_owned(),
        })
        .expect("an unknown scene is a no-op that still ungroups");

    assert_eq!(
        editor.project().timelines[0].scene,
        "",
        "no reference to a scene that is not there"
    );
}

#[test]
fn a_blank_scene_name_never_lands_and_a_padded_one_is_trimmed() {
    let mut editor = Editor::new();
    let scene = editor
        .apply(Command::AddScene)
        .expect("a scene")
        .created_id
        .expect("an id");

    editor
        .apply(Command::RenameScene {
            scene_id: scene.clone(),
            name: "   ".to_owned(),
        })
        .expect("a blank rename");
    assert_eq!(
        editor.project().scenes[0].name,
        "Scene 1",
        "blank never lands, so a scene can never be nameless"
    );

    editor
        .apply(Command::RenameScene {
            scene_id: scene,
            name: "  Opening  ".to_owned(),
        })
        .expect("a real rename");
    assert_eq!(editor.project().scenes[0].name, "Opening", "trimmed");
}

#[test]
fn scenes_survive_a_save_and_a_reload() {
    let mut editor = Editor::new();
    let scene = editor
        .apply(Command::AddScene)
        .expect("a scene")
        .created_id
        .expect("an id");
    editor
        .apply(Command::RenameScene {
            scene_id: scene.clone(),
            name: "Opening".to_owned(),
        })
        .expect("the rename");
    editor
        .apply(Command::MoveTimelineToScene {
            timeline_id: "TL1".to_owned(),
            scene_id: scene.clone(),
        })
        .expect("the move");

    let document = to_document(&settings(), editor.project());
    let again = from_document(&document).expect("what it wrote reopens");

    assert_eq!(again.scenes.len(), 1, "the scene survived");
    assert_eq!(again.scenes[0].name, "Opening");
    assert_eq!(again.scenes[0].id, scene);
    assert_eq!(
        again.timelines[0].scene, scene,
        "and so did the timeline's link to it"
    );
}

#[test]
fn a_document_without_scenes_opens_with_everything_ungrouped() {
    // `minimal-project.json` predates scenes, so its timelines have no
    // `scene` key at all - which has to read as ungrouped, not as an error.
    const MINIMAL: &str = include_str!("fixtures/minimal-project.json");
    let document: serde_json::Value = serde_json::from_str(MINIMAL).expect("the fixture is JSON");
    let project = from_document(&document).expect("the fixture still opens");

    assert!(project.scenes.is_empty(), "no scenes were invented");
    for timeline in &project.timelines {
        assert_eq!(
            timeline.scene, "",
            "every timeline of an older document is ungrouped"
        );
    }
}
