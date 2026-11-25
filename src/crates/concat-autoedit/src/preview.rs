// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! A copy of a project folder, what commands come to on it, and how it is
//! adopted back.
//!
//! The copy is the document, not the media. A candidate changes where clips
//! sit, not what they point at, so a few kilobytes of `concat.json` buy the
//! isolation a folder buys: the original is never opened for writing, and
//! the copy goes away with this value whether the candidate was adopted or
//! dropped.
//!
//! Measured on the engine (see `concat-project/tests/stage0.rs`): a command
//! naming an id the project does not hold is a silent no-op that reports
//! `applied: false`, and `Command::Batch` is atomic in memory only. So the
//! commands are applied one at a time here, each outcome is kept, and the
//! document is compared before and after rather than trusted to a status.

use concat_project::{Command, CommandError, DocumentSettings, Editor};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

/// The document a project folder holds.
const MANIFEST: &str = "concat.json";
/// Folders written before the name changed.
const LEGACY: &[&str] = &["wolfcut.json"];

/// Why a copy could not be made or a command could not be tried.
#[derive(Debug, Error)]
pub enum PreviewError {
    /// The folder holds no project document.
    #[error("no project at {0}")]
    NoProject(PathBuf),
    /// The document is there but does not read as one.
    #[error("the document at {0} does not read")]
    Unreadable(PathBuf),
    /// One command was refused; the batch stops there.
    #[error("command {index} was refused: {source}")]
    Refused {
        /// Which command, from zero.
        index: usize,
        /// What the engine said.
        #[source]
        source: CommandError,
    },
    /// No scratch directory could be claimed.
    #[error("could not claim a scratch directory under {0}")]
    NoScratch(PathBuf),
    /// The file system said no.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// A document that read as text did not read as JSON.
    #[error("{0}")]
    Json(#[from] serde_json::Error),
}

/// What the commands came to.
#[derive(Clone, Debug)]
pub struct Attempt {
    /// Per command, in order: whether it changed anything. A `false` here is
    /// what a wrong id looks like - no error, no change.
    pub applied: Vec<bool>,
    /// What the document looks like now.
    pub diff: Diff,
}

/// The difference between the document before and after.
#[derive(Clone, Debug)]
pub struct Diff {
    /// Whether anything a person would see changed.
    pub changed: bool,
    /// One line per clip added, removed or altered, and per media item.
    pub notes: Vec<String>,
    /// The document after; what [`ProjectCopy::adopt`] writes back.
    after: Value,
}

/// A copy of one project's document, in a scratch directory of its own.
pub struct ProjectCopy {
    root: PathBuf,
    manifest: PathBuf,
    original: PathBuf,
}

impl ProjectCopy {
    /// Copies `project`'s document into a fresh scratch directory.
    pub fn of(project: &Path) -> Result<Self, PreviewError> {
        let source = manifest_path(project);
        if !source.is_file() {
            return Err(PreviewError::NoProject(project.to_path_buf()));
        }
        let root = scratch_dir()?;
        let manifest = root.join(MANIFEST);
        fs::copy(&source, &manifest)?;
        Ok(Self {
            root,
            manifest,
            original: project.to_path_buf(),
        })
    }

    /// The scratch directory this copy lives in.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The project it was copied from.
    pub fn original(&self) -> &Path {
        &self.original
    }

    /// The copy's document, as it stands.
    pub fn document(&self) -> Result<Value, PreviewError> {
        let text = fs::read_to_string(&self.manifest)?;
        serde_json::from_str(&text).map_err(|_| PreviewError::Unreadable(self.manifest.clone()))
    }

    /// Applies the commands to the copy and reports what they came to.
    ///
    /// One at a time, not as a batch: a batch stops at the first refusal
    /// without saying which member refused, and the caller wants that
    /// index. A refusal leaves the copy as it was before that command, and
    /// the error carries the index and the engine's reason.
    pub fn try_commands(&self, commands: &[Command]) -> Result<Attempt, PreviewError> {
        let document = self.document()?;
        let mut editor = Editor::from_document(&document)
            .ok_or_else(|| PreviewError::Unreadable(self.manifest.clone()))?;
        let settings = settings_of(&document, &self.original);
        // Both sides of the comparison come out of the same writer, so a
        // field the reader fills in or leaves out cannot read as a change.
        // Comparing the file on disk with `to_document`'s output would make
        // every candidate look like it changed something.
        let before = editor.to_document(&settings);
        let mut applied = Vec::with_capacity(commands.len());
        for (index, command) in commands.iter().enumerate() {
            let outcome = editor
                .apply(command.clone())
                .map_err(|source| PreviewError::Refused { index, source })?;
            applied.push(outcome.applied);
        }
        let after = editor.to_document(&settings);
        Ok(Attempt {
            applied,
            diff: Diff::between(&before, &after),
        })
    }

    /// Writes the diff's document back over the original project's.
    ///
    /// Only `timelines` and `media` move: the identity fields around them
    /// (`version`, which timeline is active) are the original's and are
    /// left exactly as they were.
    pub fn adopt(&self, diff: &Diff) -> Result<(), PreviewError> {
        let target = manifest_path(&self.original);
        let text = fs::read_to_string(&target)?;
        let mut document: Value = serde_json::from_str(&text)?;
        if let Some(timelines) = diff.after.get("timelines") {
            document["timelines"] = timelines.clone();
        }
        if let Some(media) = diff.after.get("media") {
            document["media"] = media.clone();
        }
        let mut out = serde_json::to_string_pretty(&document)?;
        out.push('\n');
        fs::write(&target, out)?;
        Ok(())
    }
}

impl Drop for ProjectCopy {
    fn drop(&mut self) {
        // Ours, made a moment ago in a name nothing else holds; see
        // `scratch_dir`.
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Diff {
    fn between(before: &Value, after: &Value) -> Self {
        let before_clips = clips(before);
        let after_clips = clips(after);
        let mut notes = Vec::new();
        for id in after_clips.keys() {
            if !before_clips.contains_key(id) {
                notes.push(format!("+ clip {id}"));
            }
        }
        for id in before_clips.keys() {
            if !after_clips.contains_key(id) {
                notes.push(format!("- clip {id}"));
            }
        }
        for (id, was) in &before_clips {
            if let Some(is) = after_clips.get(id)
                && was != is
            {
                notes.push(format!("~ clip {id}"));
            }
        }
        let before_media = ids_of(before, "media");
        let after_media = ids_of(after, "media");
        for id in after_media.difference(&before_media) {
            notes.push(format!("+ media {id}"));
        }
        for id in before_media.difference(&after_media) {
            notes.push(format!("- media {id}"));
        }
        let changed = before.get("timelines") != after.get("timelines")
            || before.get("media") != after.get("media");
        Self {
            changed,
            notes,
            after: after.clone(),
        }
    }

    /// The document after the commands, for a caller that wants to look at
    /// it whole rather than at the notes.
    pub fn document(&self) -> &Value {
        &self.after
    }
}

/// The manifest a folder actually uses, current name first.
pub(crate) fn manifest_path(root: &Path) -> PathBuf {
    let current = root.join(MANIFEST);
    if current.is_file() {
        return current;
    }
    for legacy in LEGACY {
        let candidate = root.join(legacy);
        if candidate.is_file() {
            return candidate;
        }
    }
    current
}

/// Claims a directory under the system temp folder that did not exist.
///
/// The name carries the clock, the pid and a counter, and the directory is
/// made with `create_dir`, which fails when the name is taken - so a name
/// left behind by a run that died is stepped past, never reused and never
/// deleted. The same rule the handoff verifier learned: a name is not
/// proof that this run made what is at it.
fn scratch_dir() -> Result<PathBuf, PreviewError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let base = std::env::temp_dir();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let pid = std::process::id();
    for _ in 0..100 {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = base.join(format!("concat-preview-{stamp}-{pid}-{n}"));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(PreviewError::Io(e)),
        }
    }
    Err(PreviewError::NoScratch(base))
}

/// Every clip in the document, by id, across all timelines.
fn clips(document: &Value) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for timeline in array_of(document, "timelines") {
        for clip in array_of(timeline, "clips") {
            if let Some(id) = clip.get("id").and_then(Value::as_str) {
                out.insert(id.to_owned(), clip.clone());
            }
        }
    }
    out
}

/// The ids of the document's `key` array.
fn ids_of(document: &Value, key: &str) -> std::collections::BTreeSet<String> {
    array_of(document, key)
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

fn array_of<'a>(value: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// The settings the document implies, so `to_document` writes the same
/// frame and rate the copy was read with.
fn settings_of(document: &Value, project: &Path) -> DocumentSettings {
    let video = array_of(document, "timelines")
        .next()
        .and_then(|timeline| timeline.get("video"));
    let number = |key: &str, fallback: u64| {
        video
            .and_then(|v| v.get(key))
            .and_then(Value::as_u64)
            .unwrap_or(fallback)
    };
    DocumentSettings {
        name: project
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        width: number("width", 1920) as u32,
        height: number("height", 1080) as u32,
        rate_num: number("rateNum", 30) as i64,
        rate_den: number("rateDen", 1) as i64,
        canvas: [0, 0, 0, 255],
    }
}
