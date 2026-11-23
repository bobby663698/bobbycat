// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! What a model proposes: a run of commands and the words that say why.
//!
//! A candidate is not an edit. It is a proposal that has been tried on a
//! [`crate::preview::ProjectCopy`] and may be adopted; until then the
//! project it was made for has not seen it.

use concat_project::Command;
use serde::{Deserialize, Serialize};

/// One thing a director suggests.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    /// Stable within one reply; what a person picks between.
    pub id: String,
    /// One line, shown as the option's name.
    pub title: String,
    /// Why this cut, in the model's words. Shown, never trusted.
    #[serde(default)]
    pub rationale: String,
    /// The edit, as the editor's own commands.
    pub commands: Vec<Command>,
    /// How the model rated it, when it did. A tie-breaker for ordering,
    /// not a reason to adopt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<Score>,
}

impl Candidate {
    /// A candidate with no rationale and no score.
    pub fn new(id: impl Into<String>, title: impl Into<String>, commands: Vec<Command>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            rationale: String::new(),
            commands,
            score: None,
        }
    }

    /// The candidate as the one atomic edit it is.
    pub fn as_batch(&self) -> Command {
        Command::Batch {
            commands: self.commands.clone(),
        }
    }

    /// Whether it proposes nothing - refused before it reaches a copy.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// A rating, whole and in parts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Score {
    /// What the parts add up to, in the scale the director was asked for.
    pub total: f32,
    /// The parts, in the order the prompt asked for them.
    #[serde(default)]
    pub parts: Vec<Part>,
}

/// One ingredient of a [`Score`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    /// What is being weighed - `pace`, `coverage`, `variety`.
    pub name: String,
    /// How much of the total it is.
    pub weight: f32,
    /// The model's one line on why it landed there.
    #[serde(default)]
    pub note: String,
}
