// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! Auto-edit: candidates a model proposes, tried on a copy of the project,
//! adopted or dropped.
//!
//! The four pieces are the four steps of one sentence:
//!
//! - [`candidate`] - what a model proposes: a run of commands and the words
//!   that say why;
//! - [`director`] - the prompt that asks for one, and the parse of the reply;
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
//! Nothing here runs a model. [`director::prompt`] and [`director::parse`]
//! are the two ends of a call the host makes with whatever client it has
//! (`concat-agent` today).

pub mod candidate;
pub mod commit;
pub mod director;
pub mod preview;

pub use candidate::{Candidate, Part, Score};
pub use commit::{Rejection, Verdict, adopt, check};
pub use director::{Brief, ParseError, parse, prompt};
pub use preview::{Diff, PreviewError, ProjectCopy};
