// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The Scenes pane: group timelines into named scenes.
//!
//! A scene is a group, not a container: timelines stay flat in the project
//! and each carries the id of the scene it belongs to. The strip shows one
//! chip per scene plus an "ungrouped" chip; the highlighted chip is the one
//! the active timeline is in. Every click applies one project command at
//! once, so undo takes each of them back like any other edit.

use concat_project::Command;
use slint::{ModelRc, SharedString, VecModel};

use crate::studio::Studio;
use crate::ui::{SceneChipData, ScenesSheetData};

/// One thing the sheet did.
#[derive(Debug)]
pub enum ScenesMsg {
    /// Open the sheet.
    Open,
    /// Close it.
    Close,
    /// Put the active timeline in this scene.
    Select { scene_id: String },
    /// Rename a scene; blank names are ignored by the command.
    Rename { scene_id: String, name: String },
    /// Delete a scene. Its timelines fall back to ungrouped.
    Remove { scene_id: String },
    /// Take the active timeline out of its scene.
    ClearScene,
    /// Add a new empty scene, then put the active timeline in it.
    Add,
}

#[derive(Default)]
pub struct ScenesPane {
    open: bool,
}

impl ScenesPane {
    /// The pane's state, as Slint reads it.
    pub fn data(&self, studio: &Studio) -> ScenesSheetData {
        let project = studio.project();
        let active_scene = project.active().scene.clone();
        let no_scene = active_scene.is_empty();

        let mut counts = std::collections::HashMap::new();
        for timeline in &project.timelines {
            if !timeline.scene.is_empty() {
                *counts.entry(timeline.scene.as_str()).or_insert(0) += 1;
            }
        }

        let chips: Vec<SceneChipData> = project
            .scenes
            .iter()
            .map(|scene| SceneChipData {
                id: SharedString::from(scene.id.clone()),
                name: SharedString::from(scene.name.clone()),
                current: scene.id == active_scene,
                count: *counts.get(scene.id.as_str()).unwrap_or(&0) as i32,
            })
            .collect();

        ScenesSheetData {
            open: self.open,
            scenes: ModelRc::new(VecModel::from(chips)),
            // Always offered: the active timeline can be taken out of its
            // scene at any time, and with no scenes at all this is the only
            // chip on the strip.
            ungrouped: true,
            no_scene,
        }
    }

    /// Handle one message. Every edit goes through [`Studio::apply`] so it is
    /// undoable and marks the project dirty in one place.
    pub fn update(&mut self, msg: ScenesMsg, studio: &mut Studio) {
        match msg {
            ScenesMsg::Open => self.open = true,
            ScenesMsg::Close => self.open = false,
            ScenesMsg::Select { scene_id } => {
                let Some(timeline_id) = active_id(studio) else {
                    return;
                };
                studio.apply(Command::MoveTimelineToScene {
                    timeline_id,
                    scene_id,
                });
            }
            ScenesMsg::Rename { scene_id, name } => {
                studio.apply(Command::RenameScene { scene_id, name });
            }
            ScenesMsg::Remove { scene_id } => {
                studio.apply(Command::RemoveScene { scene_id });
            }
            ScenesMsg::ClearScene => {
                let Some(timeline_id) = active_id(studio) else {
                    return;
                };
                studio.apply(Command::MoveTimelineToScene {
                    timeline_id,
                    scene_id: String::new(),
                });
            }
            ScenesMsg::Add => {
                // AddScene mints the id and reports it back through the
                // command's created id, which `apply` returns. Use it to
                // group the active timeline into the new scene in the same
                // gesture, so one click gives a usable scene rather than an
                // empty chip.
                let created = studio.apply(Command::AddScene);
                if let (Some(scene_id), Some(timeline_id)) = (created, active_id(studio)) {
                    studio.apply(Command::MoveTimelineToScene {
                        timeline_id,
                        scene_id,
                    });
                }
            }
        }
    }
}

/// The active timeline's id, or nothing on the launch screen.
fn active_id(studio: &Studio) -> Option<String> {
    (studio.session.is_some()).then(|| studio.project().active().id.clone())
}
