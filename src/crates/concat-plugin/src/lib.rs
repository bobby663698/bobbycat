// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! What a plugin is allowed to be, and what it may touch.
//!
//! A plugin is a program the app did not write. It runs as a child process,
//! speaks JSON on its standard input and output, and asks the host to do
//! things: read a project, import a file, start an export. None of those
//! asks are taken on trust. This crate holds the three answers the host
//! gives before a plugin's word is acted on:
//!
//! - [`manifest`] - what the plugin says it is (`bobbycat-plugin.json`),
//!   and whether this build can run it at all.
//! - [`permissions`] - which of the API's methods its declared capabilities
//!   add up to, and which no plugin is ever given.
//! - [`paths`] - which directories on this machine those permissions name,
//!   written once as `${PROJECT}` / `${HOME}` / `${PLUGIN}` and resolved
//!   against a project the user opened.
//!
//! # Three boundaries this crate draws
//!
//! **It decides, it does not transport.** Nothing here spawns a process,
//! reads a pipe or writes a frame. `concat-plugin` answers "may this be
//! done"; the loader that asks is the next batch's work. That keeps the
//! policy testable without a child process and without a clock.
//!
//! **It names codes, it does not own numbers.** Every refusal here carries
//! a stable string code (`path.escape`, `plugin.denied`) and no JSON-RPC
//! number. The host maps a [`paths::Reason`] to a number in one place, so
//! the wire vocabulary can change without reaching into this crate - and
//! so this crate never has to know which build of the API is listening.
//!
//! **It is not a sandbox.** A path check on the host side cannot stop a
//! child process from calling `open` itself; it stops the plugin from
//! *asking* the host to do what it may not, and it is the layer that a
//! WebAssembly plugin will inherit unchanged when the sandbox lands. The
//! child-process plugin's own isolation is the process boundary, and the
//! path check is what keeps the host's authority from becoming the
//! plugin's authority.

/// The plugin's own declaration: what it is called, what it asks for, and
/// whether this build's API version can run it at all.
pub mod manifest;
/// The directories a permission resolves to, and the lexical and filesystem
/// checks every one of those paths goes through before the host opens it.
pub mod paths;
/// The capability table: which methods a grant admits, and the methods no
/// plugin is ever given.
pub mod permissions;

pub use manifest::{Entry, EntryKind, Manifest, Permissions as DeclaredPermissions};
pub use paths::{Context, Denied, Kind, Reason, Root};
pub use permissions::{Capability, Grant};
