// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The two ends of one model call: the prompt that asks for candidates, and
//! the parse of the reply.
//!
//! Nothing in this module runs a model. The host calls whatever client it
//! has - `concat-agent` today - with [`prompt`], and hands the reply back to
//! [`parse`]. Keeping the two here means the wording and the shape it is
//! expected to come back in stay in one file.

use crate::candidate::Candidate;
use serde_json::json;
use thiserror::Error;

/// What the person asked for, in the words the model gets.
#[derive(Clone, Debug, Default)]
pub struct Brief {
    /// The request itself: "cut this into a 60 second teaser".
    pub goal: String,
    /// How long the result should be, when the request says.
    pub target_seconds: Option<f64>,
    /// "warm", "brisk" - free text passed through.
    pub tone: Option<String>,
}

/// Why a reply could not be read as candidates.
#[derive(Debug, Error)]
pub enum ParseError {
    /// No JSON array anywhere in the reply.
    #[error("the reply contains no JSON array")]
    NoJson,
    /// The array did not parse.
    #[error("the reply's JSON does not parse: {0}")]
    Json(#[from] serde_json::Error),
    /// A candidate carries no commands, so there is nothing to try.
    #[error("candidate `{0}` proposes no commands")]
    Empty(String),
}

/// The prompt for one call.
///
/// `timeline` is the compact summary `timeline_read` returns: lanes, clips
/// with their ids and times, and the bin. The ids in it are the ones the
/// commands must name, which is why it is passed in whole rather than
/// paraphrased.
pub fn prompt(brief: &Brief, timeline: &str) -> String {
    let target = match brief.target_seconds {
        Some(seconds) => format!("Aim for about {seconds} seconds."),
        None => "Keep the result as long as the material deserves.".to_owned(),
    };
    let tone = brief
        .tone
        .as_deref()
        .map(|tone| format!("The feel should be {tone}."))
        .unwrap_or_default();
    format!(
        "You are cutting a video in an editor. Its timeline, as JSON:\n\
         \n\
         {timeline}\n\
         \n\
         What the person asked for: {goal}\n\
         {target}\n\
         {tone}\n\
         \n\
         Propose 2 to 3 different cuts. Reply with a JSON array and nothing\n\
         else. Each entry:\n\
         {{\"id\": \"short-slug\", \"title\": \"one line\", \"rationale\": \"why this cut\",\n\
          \"commands\": [ ... ]}}\n\
         \n\
         Each command is the editor's own, a JSON object with an `op` key.\n\
         Use only ids that appear in the timeline above. Do not invent media\n\
         ids. A command that names an id the project does not hold is\n\
         silently ignored, so a wrong id produces a cut that looks applied\n\
         and is not.",
        goal = brief.goal,
    )
}

/// Reads the reply into candidates.
///
/// A model that wraps its JSON in a markdown fence is the normal case, so
/// the fence is taken off first; text around the array is dropped rather
/// than refused, because the array is unambiguous once found.
pub fn parse(reply: &str) -> Result<Vec<Candidate>, ParseError> {
    let body = between_fence(reply).unwrap_or(reply);
    let start = body.find('[').ok_or(ParseError::NoJson)?;
    let end = body.rfind(']').ok_or(ParseError::NoJson)?;
    if end < start {
        return Err(ParseError::NoJson);
    }
    let candidates: Vec<Candidate> = serde_json::from_str(&body[start..=end])?;
    if let Some(empty) = candidates.iter().find(|c| c.is_empty()) {
        return Err(ParseError::Empty(empty.id.clone()));
    }
    Ok(candidates)
}

/// The inside of the first fenced block, when the reply has one.
fn between_fence(reply: &str) -> Option<&str> {
    let open = reply.find("```")?;
    let after_open = &reply[open + 3..];
    // The fence may name its language on the same line: ```json
    let body_start = after_open.find('\n').map(|at| at + 1).unwrap_or(0);
    let body = &after_open[body_start..];
    let close = body.find("```")?;
    Some(&body[..close])
}

/// The reply the director expects, as a value - for tests and for prompts
/// built from a fixture rather than a live timeline.
pub fn shape() -> serde_json::Value {
    json!([{
        "id": "cold-open",
        "title": "Cold open on the busiest shot",
        "rationale": "The strongest frame first, then the story.",
        "commands": [{"op": "trimClip", "clipId": "c1", "edge": "start", "delta": 0.5}]
    }])
}
