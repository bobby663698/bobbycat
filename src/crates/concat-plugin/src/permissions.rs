// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! What a plugin's declared capabilities add up to.
//!
//! A manifest names capabilities - `project.read`, `export.run` - and this
//! module turns that list into a set of methods the host will answer. The
//! table is written once, here, so that "what may a plugin do" is one
//! screenful rather than a condition in every handler.
//!
//! # The three numbers a refusal carries
//!
//! Ruling: the string in `data.code` is authoritative and the JSON-RPC
//! number only classifies the transport. So every refusal from this module
//! is `plugin.denied` with `data.capability` naming what was missing, and
//! the host turns it into one number in `Gate::admit`. There are exactly
//! three string codes and they are constants here, not string literals
//! scattered through call sites:
//!
//! - [`DENIED`] - a method asked for that the plugin's capabilities do not
//!   reach.
//! - [`INCOMPATIBLE`] - the manifest is not one this build can run.
//! - [`UNSUPPORTED`] - the capability is real and this build or platform
//!   has no way to honour it (`gpu`, a project on a network share).
//!
//! # Methods no plugin is ever given
//!
//! Some of the API is the user's, not the program's. `project.open` claims
//! a folder, takes a lock file inside it and moves it to the front of the
//! recents list - all of which are things a person did, and none of which a
//! plugin may do on their behalf. The host opens the plugins' whitelisted
//! projects itself and hands over a snapshot; `project.open` stays shut for
//! every plugin, whatever its manifest says. Same for `project.create`,
//! `project.close` and `template.save`, and for `project.list`, which
//! would hand over the paths of everything the user recently opened.

use std::collections::BTreeSet;

use serde::Deserialize;

/// A method asked for that the plugin may not have.
pub const DENIED: &str = "plugin.denied";

/// A manifest this build cannot run.
pub const INCOMPATIBLE: &str = "plugin.incompatible";

/// A real capability that this build or platform cannot honour.
pub const UNSUPPORTED: &str = "plugin.unsupported";

/// One thing a manifest may ask for, spelled as the manifest spells it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Deserialize)]
pub enum Capability {
    /// Read a project the host opened for it.
    #[serde(rename = "project.read")]
    ProjectRead,
    /// Change a project: the edit commands and the save.
    #[serde(rename = "project.write")]
    ProjectWrite,
    /// Bring a file into a project's media bin.
    #[serde(rename = "media.import")]
    MediaImport,
    /// Render the timeline to a file.
    #[serde(rename = "export.run")]
    ExportRun,
    /// Composite one frame.
    #[serde(rename = "preview.frame")]
    PreviewFrame,
    /// Make a project from a template.
    #[serde(rename = "template.use")]
    TemplateUse,
}

impl Capability {
    /// The manifest's spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Capability::ProjectRead => "project.read",
            Capability::ProjectWrite => "project.write",
            Capability::MediaImport => "media.import",
            Capability::ExportRun => "export.run",
            Capability::PreviewFrame => "preview.frame",
            Capability::TemplateUse => "template.use",
        }
    }

    /// The methods this capability unlocks.
    ///
    /// Read and write name methods of *the API*, not of a file: whether a
    /// path is inside a root is [`crate::paths`]'s question, asked after
    /// this one answers yes.
    pub fn methods(self) -> &'static [&'static str] {
        match self {
            Capability::ProjectRead => &[
                "version",
                "project.get",
                "project.document",
                "catalogue.list",
                "template.list",
                // Reading a media file's header is a read of whatever is
                // in the project, and of whatever the plugin wrote itself;
                // the path check admits it against both root sets.
                "media.probe",
            ],
            Capability::ProjectWrite => &[
                "project.save",
                "project.setVideo",
                "edit.apply",
                "edit.undo",
                "edit.redo",
                "media.probe",
            ],
            Capability::MediaImport => &["media.import"],
            Capability::ExportRun => &["export.run", "export.cancel", "export.status"],
            Capability::PreviewFrame => &["preview.frame"],
            Capability::TemplateUse => &["template.instantiate", "template.list"],
        }
    }
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Methods shut to every plugin, and why.
///
/// The reason is carried so the refusal can say it, rather than a plugin
/// author reading the table and guessing.
pub const CLOSED: &[(&str, &str)] = &[
    (
        "project.open",
        "opening a folder claims it, takes a lock and edits the user's recents; \
         the host opens whitelisted projects itself",
    ),
    (
        "project.create",
        "making a project is something the user asked for, not a side effect of a plugin",
    ),
    (
        "project.close",
        "closing can save; only the host ends a session it opened",
    ),
    (
        "project.list",
        "the recents list is where the user has been, not something a plugin reads",
    ),
    (
        "template.save",
        "writes the app's own configuration directory, outside every root a plugin declares",
    ),
];

/// Why a method was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    /// The method exists and no capability reaches it, or the plugin did
    /// not declare the one that does.
    Denied {
        /// The capability that would have allowed it, when there is one.
        capability: Option<&'static str>,
    },
    /// The method is one no plugin is given.
    Closed {
        /// Why, in the words the refusal shows.
        why: &'static str,
    },
    /// There is no such method: the plugin is speaking a newer API than
    /// this build, or guessing.
    Unknown,
}

/// The capabilities one plugin's manifest declared.
#[derive(Clone, Debug, Default)]
pub struct Grant {
    held: BTreeSet<Capability>,
}

impl Grant {
    /// The grant a list of declared capabilities amounts to.
    pub fn new(declared: impl IntoIterator<Item = Capability>) -> Grant {
        Grant {
            held: declared.into_iter().collect(),
        }
    }

    /// Every capability this grant holds.
    pub fn held(&self) -> impl Iterator<Item = Capability> + '_ {
        self.held.iter().copied()
    }

    /// Whether `method` may be answered for this plugin.
    pub fn check(&self, method: &str) -> Result<(), Refusal> {
        if let Some((_, why)) = CLOSED.iter().copied().find(|(name, _)| *name == method) {
            return Err(Refusal::Closed { why });
        }
        let Some(capability) = capability_of(method) else {
            return Err(Refusal::Unknown);
        };
        if self.held.contains(&capability) {
            Ok(())
        } else {
            Err(Refusal::Denied {
                capability: Some(capability.as_str()),
            })
        }
    }

    /// The `data.capability` a refusal should name, if any.
    pub fn missing(refusal: Refusal) -> Option<&'static str> {
        match refusal {
            Refusal::Denied { capability } => capability,
            Refusal::Closed { .. } | Refusal::Unknown => None,
        }
    }
}

/// The capability a method belongs to, or `None` for a method that is not
/// the API's.
///
/// One method may sit under two capabilities (`media.probe` under read and
/// write); this returns the first, which is only used to name what is
/// missing, and [`Grant::check`] is what decides.
pub fn capability_of(method: &str) -> Option<Capability> {
    const ALL: [Capability; 6] = [
        Capability::ProjectRead,
        Capability::ProjectWrite,
        Capability::MediaImport,
        Capability::ExportRun,
        Capability::PreviewFrame,
        Capability::TemplateUse,
    ];
    ALL.iter()
        .copied()
        .find(|capability| capability.methods().contains(&method))
}

/// Which root set a method's paths are judged against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Roots {
    /// Read only.
    Read,
    /// Write only.
    Write,
    /// Either: `media.probe` reads a file, and a plugin may probe what it
    /// just wrote.
    Either,
}

/// The methods that carry a path, and which roots that path is judged
/// against.
///
/// A method with no path in it still appears here, with an empty list.
/// That is what makes "nobody configured the new method" and "the new
/// method has no paths" different answers: [`paths_of`] returns `None` for
/// a method that is not in the table at all, and the gate refuses it. A
/// method added to the API without a line here is a method plugins cannot
/// call until somebody writes one, which is the safe direction to be
/// wrong in.
pub const PATHS: &[(&str, &[(&str, Roots)])] = &[
    ("version", &[]),
    ("project.create", &[("location", Roots::Write)]),
    ("project.open", &[("path", Roots::Read)]),
    ("project.close", &[("path", Roots::Write)]),
    ("project.list", &[]),
    ("project.get", &[("path", Roots::Read)]),
    ("project.document", &[("path", Roots::Read)]),
    ("project.save", &[("path", Roots::Write)]),
    ("project.setVideo", &[("path", Roots::Write)]),
    ("edit.apply", &[("path", Roots::Write)]),
    ("edit.undo", &[("path", Roots::Write)]),
    ("edit.redo", &[("path", Roots::Write)]),
    ("media.probe", &[("path", Roots::Either)]),
    ("media.import", &[("path", Roots::Write), ("file", Roots::Read)]),
    ("catalogue.list", &[]),
    ("template.list", &[]),
    // `fills[].file` is a list of paths inside one field; the gate walks
    // it separately, because one request may name any number of files.
    ("template.instantiate", &[("template", Roots::Read), ("location", Roots::Write)]),
    ("template.save", &[("path", Roots::Read)]),
    ("export.run", &[("path", Roots::Read), ("output", Roots::Write)]),
    ("export.cancel", &[]),
    ("export.status", &[]),
    (
        "preview.frame",
        &[("path", Roots::Read), ("output", Roots::Write)],
    ),
];

/// The path fields of `method`, or `None` when the method is not in the
/// table.
pub fn paths_of(method: &str) -> Option<&'static [(&'static str, Roots)]> {
    PATHS
        .iter()
        .copied()
        .find(|(name, _)| *name == method)
        .map(|(_, fields)| fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every method `concat-api`'s `Request` has - kept here by hand
    /// because this crate deliberately does not depend on it.
    const EVERY_METHOD: [&str; 22] = [
        "version",
        "project.create",
        "project.open",
        "project.close",
        "project.list",
        "project.get",
        "project.document",
        "project.save",
        "project.setVideo",
        "edit.apply",
        "edit.undo",
        "edit.redo",
        "media.probe",
        "media.import",
        "catalogue.list",
        "template.list",
        "template.instantiate",
        "template.save",
        "export.run",
        "export.cancel",
        "export.status",
        "preview.frame",
    ];

    #[test]
    fn every_method_has_a_verdict() {
        // Not one may answer `Unknown`: a method nobody classified would
        // be refused for the wrong reason, and nobody would notice.
        for method in EVERY_METHOD {
            let grant = Grant::new([
                Capability::ProjectRead,
                Capability::ProjectWrite,
                Capability::MediaImport,
                Capability::ExportRun,
                Capability::PreviewFrame,
                Capability::TemplateUse,
            ]);
            assert_ne!(
                grant.check(method),
                Err(Refusal::Unknown),
                "{method} is not in any capability"
            );
        }
    }

    #[test]
    fn every_method_is_in_the_path_table() {
        // `paths_of` answering `None` is the fail-closed case; a real
        // method must never reach it.
        for method in EVERY_METHOD {
            assert!(paths_of(method).is_some(), "{method} has no path row");
        }
    }

    #[test]
    fn a_read_only_plugin_cannot_export() {
        // The acceptance criterion: a clear refusal, not a silent nothing.
        let grant = Grant::new([Capability::ProjectRead]);
        assert_eq!(
            grant.check("export.run"),
            Err(Refusal::Denied {
                capability: Some("export.run")
            })
        );
        assert_eq!(
            Grant::missing(grant.check("export.run").unwrap_err()),
            Some("export.run")
        );
        assert!(grant.check("project.get").is_ok());
    }

    #[test]
    fn opening_a_project_is_closed_to_every_plugin() {
        let everything = Grant::new([
            Capability::ProjectRead,
            Capability::ProjectWrite,
            Capability::MediaImport,
            Capability::ExportRun,
            Capability::PreviewFrame,
            Capability::TemplateUse,
        ]);
        for (method, _) in CLOSED {
            match everything.check(method) {
                Err(Refusal::Closed { why }) => assert!(!why.is_empty(), "{method}"),
                other => panic!("{method} answered {other:?}"),
            }
        }
    }

    #[test]
    fn a_method_nobody_classified_is_unknown_rather_than_denied() {
        assert_eq!(
            Grant::new([Capability::ProjectRead]).check("plugin.teleport"),
            Err(Refusal::Unknown)
        );
    }

    #[test]
    fn a_request_with_two_paths_names_both() {
        assert_eq!(
            paths_of("media.import").expect("media.import"),
            &[("path", Roots::Write), ("file", Roots::Read)][..]
        );
        assert_eq!(
            paths_of("preview.frame").expect("preview.frame"),
            &[("path", Roots::Read), ("output", Roots::Write)][..]
        );
    }

    #[test]
    fn a_method_with_no_paths_says_so_rather_than_nothing() {
        assert!(paths_of("version").expect("version").is_empty());
    }
}
