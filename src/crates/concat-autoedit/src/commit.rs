// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The gate a candidate passes before it may be adopted, and the adoption.
//!
//! The gate exists because of what the engine does *not* say. A command
//! naming an id the project does not hold returns `Ok` with `applied:
//! false`; a candidate whose every command did nothing therefore looks, to
//! a caller watching for errors, exactly like one that worked. So the gate
//! asks three questions in this order:
//!
//! 1. Did any command refuse? - the index and the reason come back.
//! 2. Did every command apply to nothing? - that is [`Verdict::Inert`], and
//!    an inert candidate is not offered to a person as a cut.
//! 3. Did the document change at all? - if not, the same answer.

use crate::candidate::Candidate;
use crate::preview::{Attempt, Diff, PreviewError, ProjectCopy};
use std::path::Path;

/// What a candidate came to on a copy of the project.
#[derive(Debug)]
pub enum Verdict {
    /// Every command applied and the document changed: adoptable.
    Clean(Attempt),
    /// Nothing it said would change anything. Not an error, and not a cut.
    Inert(Attempt),
    /// It was refused before it could be a cut.
    Rejected(Rejection),
}

/// Why a candidate was refused, and which member did it.
#[derive(Debug, Clone)]
pub struct Rejection {
    /// One line, the engine's or ours.
    pub why: String,
    /// Which command, from zero; `None` when the whole candidate is at fault.
    pub command: Option<usize>,
}

impl Rejection {
    fn whole(why: impl Into<String>) -> Self {
        Self {
            why: why.into(),
            command: None,
        }
    }
}

/// The ASR admission gate, called before `check`.
///
/// `pending` holds the manifest's `timing_review_required` asset ids. A
/// candidate that uses one of them *with sound* is blocked; the same asset
/// used as a silent picture is fine and is not listed in `audio_assets`.
/// The returned rejection has no command index: the whole candidate is
/// refused, before any copy of the project is made.
pub fn guard_pending_asr(candidate: &Candidate, pending: &[String]) -> Option<Rejection> {
    candidate
        .audio_assets
        .iter()
        .find(|asset| pending.contains(asset))
        .map(|asset| Rejection {
            why: format!(
                "asr_timing_review: material `{asset}` has transcription timing pending; \
                 it may be used as a silent picture only"
            ),
            command: None,
        })
}

/// Tries a candidate on a copy and says whether it may be adopted.
pub fn check(project: &Path, candidate: &Candidate) -> Result<Verdict, PreviewError> {
    if candidate.is_empty() {
        return Ok(Verdict::Rejected(Rejection::whole(
            "the candidate proposes no commands",
        )));
    }
    let copy = ProjectCopy::of(project)?;
    match copy.try_commands(&candidate.commands) {
        Ok(attempt) => {
            let any = attempt.applied.iter().any(|applied| *applied);
            if any && attempt.diff.changed {
                Ok(Verdict::Clean(attempt))
            } else {
                Ok(Verdict::Inert(attempt))
            }
        }
        Err(PreviewError::Refused { index, source }) => Ok(Verdict::Rejected(Rejection {
            why: source.to_string(),
            command: Some(index),
        })),
        Err(other) => Err(other),
    }
}

/// Adopts a candidate: tries it once more on a fresh copy and writes the
/// result over the project's document.
///
/// Returns the diff when something was written, and `None` when the
/// candidate came to nothing - an inert candidate is not written, so a file
/// that would not have changed is not touched.
///
/// `check` and this are two trips through the copy on purpose: the copy a
/// person looked at is gone by the time they say yes, and re-running the
/// commands is cheaper than keeping a directory alive for as long as a
/// conversation might last.
pub fn adopt(project: &Path, candidate: &Candidate) -> Result<Option<Diff>, PreviewError> {
    if candidate.is_empty() {
        return Ok(None);
    }
    let copy = ProjectCopy::of(project)?;
    let attempt = copy.try_commands(&candidate.commands)?;
    if !attempt.diff.changed {
        return Ok(None);
    }
    copy.adopt(&attempt.diff)?;
    Ok(Some(attempt.diff))
}
