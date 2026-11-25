// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! Auto-edit: candidates a model proposes, tried on a copy of the project,
//! adopted or dropped.
//!
//! The five pieces are the five steps of one sentence:
//!
//! - [`expand`] - an `ip_plan_v1` plan becoming the editor's commands;
//! - [`candidate`] - what a model proposes: a run of commands and the words
//!   that say why;
//! - [`director`] - the prompt that asks for one, and the parse of the reply;
//! - [`agent`] - the call between those two ends, made through
//!   `concat-agent`;
//! - [`preview`] - a copy of the project folder, what the commands come to
//!   on it, and how the copy is adopted back;
//! - [`commit`] - the check a candidate passes before it may be adopted.
//!
//! Two facts shape the design, both measured on this engine rather than
//! assumed (see `concat-project/tests/stage0.rs`):
//!
//! 1. **An unknown id is a silent no-op.** A command that names a clip the
//!    project no longer holds returns `Ok` with `applied: false`. So a
//!    candidate is never judged by whether its commands returned `Err`; it
//!    is judged by what the document looks like afterwards.
//! 2. **`Command::Batch` is atomic in memory, not on disk.** It clones the
//!    project, applies to the clone and commits only a fully successful run.
//!    A candidate that has to be looked at *before* it is adopted needs a
//!    folder of its own, which is what [`preview`] gives it.
//!
//! One call is [`agent::propose`]: the prompt out through `concat-agent`, the
//! reply back through [`director::parse`]. A host with another route to a
//! model - the Codex CLI, a fixture, a hand-written reply - skips that and
//! joins [`director::parse`] to [`commit::check`] itself.

pub mod agent;
pub mod candidate;
pub mod commit;
pub mod director;
pub mod expand;
pub mod preview;

pub use agent::{ProposeError, messages, propose, propose_streaming, timeline_of};
pub use candidate::{Candidate, Part, Score};
pub use commit::{Rejection, Verdict, adopt, check, guard_pending_asr};
pub use director::{Brief, ParseError, parse, prompt};
pub use expand::{ExpandError, Expansion, Probe, db_to_linear, expand};
pub use preview::{Diff, PreviewError, ProjectCopy};
