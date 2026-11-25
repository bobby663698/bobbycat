// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The call itself: [`crate::director::prompt`] goes out through
//! `concat-agent`, the reply comes back through [`crate::director::parse`].
//!
//! The rest of this crate is testable without a network, and stays that way:
//! this module is the single place that touches one, and it is deliberately
//! the thinnest binding that can exist - one round of messages in, candidates
//! or a typed error out. It picks no endpoint, stores no key and retries
//! nothing; the host owns the [`Endpoint`] the person configured, and a
//! dropped connection is the whole of the cancellation story.
//!
//! Two exits are normal and are kept apart on purpose:
//!
//! - candidates, which the host tries on a copy with [`crate::check`];
//! - [`ProposeError::Reply`] carrying [`ParseError::Blocked`], which is a
//!   model saying what material is missing. That is an answer to show, not a
//!   failure of the call.
//!
//! A host that talks to a model by another route - the Codex CLI, a batch
//! file, a fixture - skips this module and calls [`crate::director::parse`]
//! and [`crate::check`] itself. Nothing below is a prerequisite for that.

use crate::candidate::Candidate;
use crate::director::{self, Brief, ParseError};
use crate::preview::{PreviewError, manifest_path};
use concat_agent::{Endpoint, Message, chat};
use std::path::Path;

/// Why one round of proposals produced nothing.
#[derive(Debug, thiserror::Error)]
pub enum ProposeError {
    /// The endpoint refused the request or never answered. The base, the key
    /// and the model are the person's own, so the message is shown as it is.
    #[error("{0}")]
    Agent(#[from] concat_agent::Error),
    /// An answer arrived and could not be read as candidates - including
    /// `BLOCKED:`, which is the model reporting a gap rather than failing.
    #[error("{0}")]
    Reply(#[from] ParseError),
}

/// The messages one round sends.
///
/// One user turn and no system turn: [`director::prompt`] already opens with
/// the role ("You are cutting a video in an editor"), and the twelve rules it
/// carries are the contract's. A second copy in a system turn would put the
/// same twelve rules in front of the model twice, which is how a contract
/// starts drifting from itself.
pub fn messages(brief: &Brief, timeline: &str) -> Vec<Message> {
    vec![Message::user(director::prompt(brief, timeline))]
}

/// The compact summary a prompt takes, read from a project folder.
///
/// This is the document as the editor writes it - every timeline, clip, media
/// item and setting - minus its pretty-printing, which is only bytes to a
/// model. `timeline_read` in `concat-mcp` returns the same object; a host that
/// already holds one passes its own text to [`propose`] and never calls this.
///
/// Read from disk rather than from a live editor on purpose: a `propose` that
/// ran against unsaved state would send ids the project on disk does not hold,
/// and id mismatch is the one failure this pipeline cannot detect afterwards.
pub fn timeline_of(project: &Path) -> Result<String, PreviewError> {
    let path = manifest_path(project);
    if !path.is_file() {
        return Err(PreviewError::NoProject(project.to_path_buf()));
    }
    let text = std::fs::read_to_string(&path)?;
    let document: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| PreviewError::Unreadable(path.clone()))?;
    // `Value`'s own Display is the compact form; there is nothing to fail on.
    Ok(document.to_string())
}

/// Asks the endpoint for candidates and reads the reply.
///
/// `timeline` is the compact summary [`director::prompt`] describes; its ids
/// are the ones the commands must name, so it is passed in whole.
pub fn propose(
    endpoint: &Endpoint,
    brief: &Brief,
    timeline: &str,
) -> Result<Vec<Candidate>, ProposeError> {
    propose_streaming(endpoint, brief, timeline, &mut |_: &str| {})
}

/// [`propose`], reporting the reply as it arrives.
///
/// The sink runs on the calling thread, once per delta: a host that wants the
/// text on screen while the model writes it passes its own; a host that only
/// wants the end result calls [`propose`] and pays nothing for the callback.
pub fn propose_streaming(
    endpoint: &Endpoint,
    brief: &Brief,
    timeline: &str,
    sink: &mut dyn FnMut(&str),
) -> Result<Vec<Candidate>, ProposeError> {
    let reply = chat(endpoint, &messages(brief, timeline), sink)?;
    Ok(director::parse(&reply)?)
}
