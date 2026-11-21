// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! `bobbycat-plugin.json`: what a plugin says it is.
//!
//! The shape is deliberately dull - a name, a version, an api, one entry,
//! and the permissions it wants - because everything else about a plugin
//! is the plugin's own business. This is the only file the host reads
//! before it decides whether to run the program at all.
//!
//! # Why `api` is a major number and not a range
//!
//! A plugin written against api 1 that meets a host at api 2 must be
//! refused, not run. The failure that this prevents is not a crash - it is
//! a *success*: a field serde does not know is ignored, a default stands in
//! for a value the plugin meant to set, and the call returns happily with
//! the old meaning. Nobody finds that. So the host compares the major
//! number, refuses anything else at load, and says which two numbers did
//! not meet. A capability added later is *not* a version bump: an unknown
//! capability is refused by name ([`crate::permissions::Capability`]'s
//! `Deserialize`), and a known one that a build cannot honour is
//! `plugin.unsupported` at the call, not here.
//!
//! # Strict where a person types, lenient where a program speaks
//!
//! Every struct here is `deny_unknown_fields`. A manifest is written once,
//! by hand, and `"permisions"` must be a loud error rather than a plugin
//! that loads with no permissions at all. The RPC side of this crate is the
//! other way round - defaults, unknown fields ignored - because that is
//! where a newer caller is expected.

use std::path::PathBuf;

use serde::Deserialize;
use serde::de::{Deserializer, Error as _};

use crate::paths::{Context, Denied, Root};
use crate::permissions::Capability;

/// The api this build speaks.
pub const API_MAJOR: u32 = 1;

/// The host interface version a manifest names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Api {
    /// Changed only when an old plugin would misread a new host.
    pub major: u32,
    /// Informational; shown in the log, never compared.
    pub minor: u32,
}

impl Api {
    /// The version `text` names: `"1"`, or `"1.2"`.
    pub fn parse(text: &str) -> Option<Api> {
        let mut parts = text.trim().split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = match parts.next() {
            Some(part) => part.parse().ok()?,
            None => 0,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(Api { major, minor })
    }

    /// Whether this build can run a plugin written for `self`.
    pub fn supported(&self) -> bool {
        self.major == API_MAJOR
    }
}

impl std::fmt::Display for Api {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.minor == 0 {
            write!(f, "{}", self.major)
        } else {
            write!(f, "{}.{}", self.major, self.minor)
        }
    }
}

impl<'de> Deserialize<'de> for Api {
    fn deserialize<D>(deserializer: D) -> Result<Api, D::Error>
    where
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        Api::parse(&text).ok_or_else(|| D::Error::custom(format!("api {text:?} is a number")))
    }
}

/// How a plugin's code is started.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    /// A program in its own process, spoken to over its standard streams.
    Process,
    /// A module for the sandbox. Parsed from batch one, refused until the
    /// sandbox ships (`plugin.unsupported`), so that a plugin written for
    /// it says so in its manifest and not by failing to start.
    Wasm,
}

/// What to run, and with what.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Entry {
    /// A process or a module.
    pub kind: EntryKind,
    /// The program: on `$PATH`, or relative to the plugin's own folder.
    pub command: String,
    /// Its arguments, as they are passed.
    #[serde(default)]
    pub args: Vec<String>,
}

/// What the plugin asks for.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase", default)]
pub struct Permissions {
    /// Directories it may read, as templates: `${PROJECT}`, `${HOME}`,
    /// `${PLUGIN}`, then a subpath.
    pub read: Vec<String>,
    /// Directories it may write.
    pub write: Vec<String>,
    /// The API methods it may call, by capability.
    pub capabilities: Vec<Capability>,
}

/// A plugin's manifest.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Manifest {
    /// `author.name`, the id the plugin is known by.
    pub name: String,
    /// The plugin's own version; shown, never compared.
    pub version: String,
    /// The host interface it was written against.
    pub api: Api,
    /// What to run.
    pub entry: Entry,
    /// What it may touch.
    #[serde(default)]
    pub permissions: Permissions,
}

/// Why a manifest is not one this build can run.
///
/// Every one of these is `plugin.incompatible`: the plugin and the host do
/// not meet, which is a different sentence from "you may not do that".
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Problem {
    /// The file is not the JSON this describes.
    Json(String),
    /// `name` is not `author.name`.
    Name(String),
    /// `version` is not three numbers.
    Version(String),
    /// The api is not the one this build speaks.
    Api {
        /// What the manifest asked for.
        found: Api,
        /// What this build speaks.
        wanted: u32,
    },
    /// `entry` names nothing to run.
    Entry(String),
}

impl Problem {
    /// The string code the host puts in `data.code`.
    pub fn code(&self) -> &'static str {
        crate::permissions::INCOMPATIBLE
    }

    /// The sentence a person is shown, saying what to change.
    pub fn message(&self) -> String {
        match self {
            Problem::Json(why) => format!("清单不是合法的 bobbycat-plugin.json：{why}"),
            Problem::Name(name) => {
                format!("插件名 {name:?} 不是 `作者.名字` 的形式（小写字母、数字、- 和 _）")
            }
            Problem::Version(version) => {
                format!("版本 {version:?} 不是 主.次.修订 三个数字")
            }
            Problem::Api { found, wanted } => format!(
                "这个插件按 API {found} 写，本版本只支持 API {wanted}，请升级插件或换用旧版"
            ),
            Problem::Entry(why) => format!("清单的 entry 无法执行：{why}"),
        }
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for Problem {}

impl Manifest {
    /// Reads and checks a manifest.
    ///
    /// Checks the four things that decide whether the program can be run at
    /// all: the name is an id, the version is a version, the api is one
    /// this build speaks, and the entry names something.
    pub fn load(text: &str) -> Result<Manifest, Problem> {
        let manifest: Manifest =
            serde_json::from_str(text).map_err(|error| Problem::Json(error.to_string()))?;
        manifest.check()?;
        Ok(manifest)
    }

    /// The checks [`Manifest::load`] makes, for a manifest already built.
    pub fn check(&self) -> Result<(), Problem> {
        if !is_name(&self.name) {
            return Err(Problem::Name(self.name.clone()));
        }
        if !is_version(&self.version) {
            return Err(Problem::Version(self.version.clone()));
        }
        if !self.api.supported() {
            return Err(Problem::Api {
                found: self.api,
                wanted: API_MAJOR,
            });
        }
        if self.entry.command.trim().is_empty() {
            return Err(Problem::Entry("没有 command".to_owned()));
        }
        if self.entry.command.contains('\0') || self.entry.args.iter().any(|arg| arg.contains('\0'))
        {
            return Err(Problem::Entry("参数里有 NUL".to_owned()));
        }
        Ok(())
    }

    /// The directories the manifest grants, resolved against `ctx`.
    ///
    /// A template that names nothing existing is fine - `${PROJECT}/export`
    /// is made by the first export - but one that cannot be resolved at all
    /// is refused here, at load, rather than at the first call.
    pub fn roots(&self, ctx: &Context) -> Result<(Vec<Root>, Vec<Root>), Denied> {
        let mut read = Vec::with_capacity(self.permissions.read.len());
        for template in &self.permissions.read {
            read.push(Root::parse(template, ctx)?);
        }
        let mut write = Vec::with_capacity(self.permissions.write.len());
        for template in &self.permissions.write {
            write.push(Root::parse(template, ctx)?);
        }
        Ok((read, write))
    }

    /// The load-time warnings: a template a person probably did not mean.
    ///
    /// These are not refusals. A manifest may name `${PROJECT}/NUL` and be
    /// accepted; the first call that tries to use it is refused with
    /// `path.device`. Saying it at load means the author sees it when they
    /// are still looking at the file.
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (index, template) in self.permissions.read.iter().enumerate() {
            if let Some(hint) = crate::paths::suspicious(template) {
                out.push(format!("permissions.read[{index}]: {hint}"));
            }
        }
        for (index, template) in self.permissions.write.iter().enumerate() {
            if let Some(hint) = crate::paths::suspicious(template) {
                out.push(format!("permissions.write[{index}]: {hint}"));
            }
        }
        out
    }

    /// Where the plugin's own folder would be, given where the manifest is.
    pub fn folder(manifest: &std::path::Path) -> PathBuf {
        manifest
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

/// `author.name`: one dot, two segments, lowercase, digits, `-` and `_`.
fn is_name(name: &str) -> bool {
    let mut parts = name.split('.');
    let (Some(author), Some(plugin), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let segment = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    };
    segment(author) && segment(plugin)
}

/// Three dot-separated numbers.
fn is_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{
      "name": "bobbycat.batch-credits",
      "version": "1.0.0",
      "api": "1",
      "entry": { "kind": "process", "command": "node", "args": ["main.js"] },
      "permissions": {
        "read": ["${PROJECT}"],
        "write": ["${PROJECT}/export"],
        "capabilities": ["project.read", "media.import", "export.run"]
      }
    }"#;

    fn context(dir: &std::path::Path) -> Context {
        Context {
            project: dir.to_path_buf(),
            home: dir.to_path_buf(),
            plugin: dir.join("plugin"),
        }
    }

    #[test]
    fn the_documented_manifest_loads() {
        let manifest = Manifest::load(GOOD).expect("a manifest");
        assert_eq!(manifest.name, "bobbycat.batch-credits");
        assert_eq!(manifest.api, Api { major: 1, minor: 0 });
        assert_eq!(manifest.entry.kind, EntryKind::Process);
        assert_eq!(
            manifest.permissions.capabilities,
            vec![
                Capability::ProjectRead,
                Capability::MediaImport,
                Capability::ExportRun
            ]
        );
    }

    #[test]
    fn a_misspelled_field_is_loud_rather_than_silent() {
        // `permisions` must not load as a plugin with no permissions.
        let text = GOOD.replace("\"permissions\"", "\"permisions\"");
        assert!(matches!(
            Manifest::load(&text).expect_err("misspelled"),
            Problem::Json(_)
        ));
    }

    #[test]
    fn a_capability_nobody_knows_is_refused_by_name() {
        let text = GOOD.replace("project.read", "project.teleport");
        let problem = Manifest::load(&text).expect_err("unknown capability");
        assert!(
            problem.message().contains("project.teleport"),
            "{}",
            problem.message()
        );
        assert_eq!(problem.code(), "plugin.incompatible");
    }

    #[test]
    fn an_api_from_the_future_is_refused_and_says_both_numbers() {
        let text = GOOD.replace("\"api\": \"1\"", "\"api\": \"2\"");
        let problem = Manifest::load(&text).expect_err("api 2");
        assert_eq!(
            problem,
            Problem::Api {
                found: Api { major: 2, minor: 0 },
                wanted: 1
            }
        );
        assert!(problem.message().contains("API 2"), "{}", problem.message());
        assert!(problem.message().contains("API 1"), "{}", problem.message());
    }

    #[test]
    fn a_minor_number_is_information_not_a_compatibility_test() {
        let text = GOOD.replace("\"api\": \"1\"", "\"api\": \"1.7\"");
        let manifest = Manifest::load(&text).expect("api 1.7");
        assert_eq!(manifest.api, Api { major: 1, minor: 7 });
        assert!(manifest.api.supported());
    }

    #[test]
    fn a_name_is_one_dot_and_two_lowercase_segments() {
        for good in ["a.b", "bobbycat.batch-credits", "acme.x_1"] {
            assert!(is_name(good), "{good}");
        }
        for bad in ["batch-credits", "A.b", "a.b.c", "a.", ".b", "a..b", "a b"] {
            assert!(!is_name(bad), "{bad}");
        }
    }

    #[test]
    fn a_version_is_three_numbers() {
        for good in ["1.0.0", "10.20.30"] {
            assert!(is_version(good), "{good}");
        }
        for bad in ["1.0", "1.0.0.0", "1..0", "v1.0.0", ""] {
            assert!(!is_version(bad), "{bad}");
        }
    }

    #[test]
    fn an_entry_that_names_nothing_is_refused() {
        let text = GOOD.replace("\"command\": \"node\"", "\"command\": \"  \"");
        let problem = Manifest::load(&text).expect_err("no command");
        assert_eq!(problem.code(), "plugin.incompatible");
    }

    #[test]
    fn the_roots_are_the_templates_resolved() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let manifest = Manifest::load(GOOD).expect("a manifest");
        let (read, write) = manifest.roots(&context(dir.path())).expect("roots");
        assert_eq!(read.len(), 1);
        assert_eq!(write.len(), 1);
        assert_eq!(read[0].declared(), dir.path());
        // `${PROJECT}/export` does not exist yet, and is granted anyway.
        assert_eq!(write[0].declared(), dir.path().join("export").as_path());
    }

    #[test]
    fn a_root_that_cannot_be_resolved_is_refused_at_load() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let manifest = Manifest::load(GOOD).expect("a manifest");
        let ctx = Context {
            project: dir.path().join("gone"),
            home: dir.path().to_path_buf(),
            plugin: dir.path().to_path_buf(),
        };
        let denied = manifest.roots(&ctx).expect_err("gone");
        assert_eq!(denied.reason(), crate::paths::Reason::Unresolvable);
        assert!(
            denied.message().contains("${PROJECT}"),
            "{}",
            denied.message()
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_template_that_names_a_device_warns_at_load() {
        let text = GOOD.replace("\"${PROJECT}/export\"", "\"${PROJECT}/NUL\"");
        let manifest = Manifest::load(&text).expect("a manifest");
        let warnings = manifest.warnings();
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].starts_with("permissions.write[0]: "),
            "{:?}",
            warnings
        );
        assert!(warnings[0].contains("NUL"), "{:?}", warnings);
    }

    #[test]
    fn a_manifest_with_no_permissions_asks_for_nothing() {
        let text = r#"{
          "name": "bobbycat.quiet",
          "version": "0.1.0",
          "api": "1",
          "entry": { "kind": "process", "command": "quiet" }
        }"#;
        let manifest = Manifest::load(text).expect("a manifest");
        assert!(manifest.permissions.read.is_empty());
        assert!(manifest.permissions.write.is_empty());
        assert!(manifest.permissions.capabilities.is_empty());
    }
}
