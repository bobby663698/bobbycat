// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! Which directories a plugin may reach, and the check that keeps it there.
//!
//! # The two phases
//!
//! A path is judged twice, in two different moods.
//!
//! **Load time** ([`Root::parse`]) sees the manifest's `permissions.read`
//! and `permissions.write`: templates like `${PROJECT}/export`, expanded
//! against the directories the user has open. This is the only place a
//! template is expanded. A manifest is a file a person typed once, so a
//! template that names nothing is refused loudly rather than skipped.
//!
//! **Use time** ([`validate`]) sees each request's absolute path. No
//! template is expanded here: a plugin that sends `${HOME}/x` gets
//! [`Reason::Template`], because honouring it would make the manifest's
//! whitelist decorative.
//!
//! # The two deliberate differences from `restrict_writes_to`
//!
//! The editor's own API confines writes with `Api::restrict_writes_to`
//! (`concat-api/src/lib.rs:375`). This module is stricter in two ways, and
//! neither is a bug to be "fixed" back:
//!
//! 1. **An empty whitelist denies everything.** `restrict_writes_to` treats
//!    an empty list as "confine nothing" (`lib.rs:389-391`), which is right
//!    for the process's own owner at a terminal and wrong for a plugin. A
//!    plugin that declared no roots has been granted no roots; the empty
//!    case must fail closed or the fail-open case is a silent hole.
//! 2. **Reads are judged too.** `restrict_writes_to` confines writes only,
//!    and says so (`lib.rs:369-375`). A plugin's most wanted verb is
//!    `project.document`, so a model that judged only writes would hand it
//!    the disk.
//!
//! The test `empty_whitelist_denies_everything` exists to keep the first
//! one from being "corrected".
//!
//! # The boundary of this module
//!
//! > This module guarantees that the paths *coming in* are inside a root.
//! > It does not guarantee that writes *going out* land inside one; that is
//! > the caller's business.
//!
//! The distinction is real: `Api::save` does not call `writable`, and
//! `projects::save` has exactly one caller (`concat-host/src/session.rs`),
//! so a session opened outside the roots can still be saved back. A plugin
//! may not name such a path, but the host must not hand it one either. Said
//! here because otherwise somebody will read this module and believe the
//! write side is covered.
//!
//! # The lazy path
//!
//! A path that does not exist yet cannot be canonicalized, and an export
//! target almost never exists yet. The shape that works splits a path into
//! the longest part that *does* exist - canonicalized, symlinks resolved -
//! and the tail that does not, which is compared as plain components. A
//! component that does not exist yet cannot point anywhere else, so it
//! needs no resolution; but a root's own tail still has to be compared, or
//! a root of `…\export` would happily accept `…\export_backup`.
//!
//! # Known window
//!
//! [`open`] re-runs the whole check and only then opens the file, which
//! narrows the gap between "checked" and "used" to one `open` call. It does
//! not close it: closing it needs `GetFinalPathNameByHandle` on the open
//! handle, and this crate deliberately does not take a platform dependency
//! for a nanosecond-wide window. The sandbox in the next batch is where
//! that gets solved properly. Anyone tempted to turn the second check into
//! a `debug_assert!`: it is not an assertion, it is the check.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};

/// Every way a path is refused.
///
/// The string each one answers with ([`Reason::as_str`]) is the contract: a
/// published code never changes and is never reused. A code that turns out
/// to be wrong is superseded (`path.blank.v2`), not edited, because scripts
/// and logs branch on the string.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reason {
    /// The path is empty or blank.
    ///
    /// `Path::starts_with("")` is true of every path, so a validator that
    /// does not refuse the empty string first accepts everything.
    Blank,
    /// The path holds a NUL, a control character, or a character Windows
    /// will not take in a name (`< > " | ? *`).
    Invalid,
    /// The path is not absolute.
    Relative,
    /// The path is absolute but written in a form that is refused: a
    /// verbatim path (`\\?\C:\…`), a UNC share, the device namespace
    /// (`\\.\`, `\??\`), or a drive with no root (`C:file`).
    Prefix,
    /// The path holds a `..` component.
    Parent,
    /// The path holds a `.` component.
    Dot,
    /// A component ends in a dot or a space, which Windows strips silently.
    Trailing,
    /// A component holds a `:` - an alternate data stream, or a drive.
    Colon,
    /// A component is one of Windows' reserved device names.
    Device,
    /// The longest part of the path that exists could not be resolved.
    Unresolvable,
    /// The path resolves outside every root the plugin was granted.
    Escape,
    /// A component of the path is a symbolic link.
    Symlink,
    /// The manifest used a template that does not exist, or a request
    /// carried one.
    Template,
}

impl Reason {
    /// The stable string code, `path.escape` and its twelve siblings.
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Blank => "path.blank",
            Reason::Invalid => "path.invalid",
            Reason::Relative => "path.relative",
            Reason::Prefix => "path.prefix",
            Reason::Parent => "path.parent",
            Reason::Dot => "path.dot",
            Reason::Trailing => "path.trailing",
            Reason::Colon => "path.colon",
            Reason::Device => "path.device",
            Reason::Unresolvable => "path.unresolvable",
            Reason::Escape => "path.escape",
            Reason::Symlink => "path.symlink",
            Reason::Template => "path.template",
        }
    }

    /// How this refusal reads at the top level, for the host to turn into a
    /// JSON-RPC number.
    ///
    /// Batch one answers [`Kind::Refused`] for all thirteen: the string code
    /// is what a caller branches on, and spreading thirteen codes over three
    /// numbers would only make log aggregation harder. The method is written
    /// now, returning one value, so that the day the split lands only the
    /// returned value changes - see "API_VERSION 2" in `PATHS_CODES.md`.
    pub fn top_level(self) -> Kind {
        match self {
            Reason::Unresolvable => Kind::Failed,
            Reason::Escape | Reason::Symlink => Kind::Refused,
            _ => Kind::Invalid,
        }
    }
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The three moods a refusal can be in, named after the `ErrorCode` each
/// will become.
///
/// This is deliberately not `concat_api::ErrorCode`: this crate names
/// moods, the host picks numbers, and the one place that conversion happens
/// is `Gate::admit`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// The request itself is wrong; changing it might work. Becomes
    /// `ErrorCode::Invalid` (`-32600`).
    Invalid,
    /// The request and the permissions were fine, and the machine was not.
    /// Becomes `ErrorCode::Failed` (`-32000`).
    Failed,
    /// This way is not allowed. Becomes `ErrorCode::Refused` (`-32003`).
    Refused,
}

/// A refused path: which rule caught it, what was asked for, and the detail
/// the message and the logs need.
#[derive(Clone, Debug)]
pub struct Denied {
    reason: Reason,
    requested: String,
    data: Map<String, Value>,
}

impl Denied {
    /// A refusal of `requested` by `reason`.
    pub fn new(reason: Reason, requested: impl Into<String>) -> Denied {
        Denied {
            reason,
            requested: requested.into(),
            data: Map::new(),
        }
    }

    /// A refusal with nothing to quote - a rule that fired before any
    /// caller-supplied path was read.
    pub fn only(reason: Reason) -> Denied {
        Denied::new(reason, "")
    }

    /// The same refusal, with one more field for the message.
    #[must_use]
    pub fn at(mut self, key: &'static str, value: impl Into<Value>) -> Denied {
        self.data.insert(key.to_owned(), value.into());
        self
    }

    /// Which rule refused it.
    pub fn reason(&self) -> Reason {
        self.reason
    }

    /// The string code the host puts in `data.code`.
    pub fn code(&self) -> &'static str {
        "plugin.denied"
    }

    /// The extra fields, for `data`.
    pub fn data(&self) -> &Map<String, Value> {
        &self.data
    }

    /// What was asked for, as it was written.
    pub fn requested(&self) -> &str {
        &self.requested
    }

    fn field(&self, key: &str) -> &str {
        self.data.get(key).and_then(Value::as_str).unwrap_or("")
    }

    /// The sentence a person is shown, saying what to do next where there
    /// is something to do.
    pub fn message(&self) -> String {
        let asked = &self.requested;
        match self.reason {
            Reason::Blank => "路径是空的，请填写一个绝对路径，例如 C:\\…".to_owned(),
            Reason::Invalid => format!(
                "路径第 {} 个字符是 “{}”，文件名里不能有它",
                self.data
                    .get("index")
                    .and_then(Value::as_u64)
                    .unwrap_or_default(),
                self.field("character")
            ),
            Reason::Relative => {
                format!("{asked} 不是绝对路径，请从盘符写起，例如 C:\\…")
            }
            Reason::Prefix => format!(
                "不支持这种路径写法（{}），请用普通盘符路径，例如 C:\\…",
                self.field("prefix")
            ),
            Reason::Parent => format!("{asked} 里不能有 `..`，请直接写出目标路径"),
            Reason::Dot => format!("{asked} 里不能有 `.`，请直接写出目标路径"),
            Reason::Trailing => format!(
                "组件 `{}` 以点或空格结尾，Windows 会静默去掉它，请去掉后再试",
                self.field("component")
            ),
            Reason::Colon => format!(
                "组件 `{}` 含 `:`，会被当成驱动器或备用数据流",
                self.field("component")
            ),
            Reason::Device => format!(
                "组件 `{}` 是 Windows 保留设备名 `{}`，写出去不会落到文件",
                self.field("component"),
                self.field("device")
            ),
            Reason::Unresolvable => match self.field("root") {
                "" => format!("{asked} 无法解析：{} 不存在或不可访问", self.field("at")),
                root => format!(
                    "{asked} 无法解析：{} 不存在或不可访问（白名单根 {root} 展开不了）",
                    self.field("at")
                ),
            },
            Reason::Escape => format!("{asked} 不在授权范围内（{}）", self.field("roots")),
            Reason::Symlink => format!(
                "路径经过符号链接 `{}`，插件不允许穿过符号链接",
                self.field("component")
            ),
            Reason::Template => match self.field("token") {
                "" => format!("{asked} 里不能使用 ${{…}}，模板只在清单里展开"),
                token => format!("清单不能使用未知变量 ${{{token}}}"),
            },
        }
    }

    /// The refusal as the `data` object the host sends.
    pub fn into_value(self) -> Value {
        let Denied {
            reason,
            requested,
            data,
        } = self;
        let mut data = data;
        data.insert("code".to_owned(), Value::String("plugin.denied".to_owned()));
        data.insert(
            "reason".to_owned(),
            Value::String(reason.as_str().to_owned()),
        );
        if !requested.is_empty() {
            data.insert("requested".to_owned(), Value::String(requested));
        }
        Value::Object(data)
    }
}

impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for Denied {}

/// The three directories a template can name.
#[derive(Clone, Debug)]
pub struct Context {
    /// Where `${PROJECT}` points: the project folder the user has open.
    pub project: PathBuf,
    /// Where `${HOME}` points: the user's home, as the host reads it.
    pub home: PathBuf,
    /// Where `${PLUGIN}` points: the plugin's own folder.
    pub plugin: PathBuf,
}

/// One granted directory, resolved: the part that exists, canonicalized, and
/// the part that does not, kept as components.
#[derive(Clone, Debug)]
pub struct Root {
    /// The root as the manifest wrote it, expanded. Kept so that a request
    /// spelled the same way as the root is not refused for passing through
    /// the symlink the root itself was declared with.
    declared: PathBuf,
    /// The longest existing part, canonicalized.
    prefix: PathBuf,
    /// What does not exist yet, in order.
    tail: Vec<OsString>,
}

impl Root {
    /// Expands `template` against `ctx` and resolves it.
    ///
    /// A root that does not exist yet is fine - `${PROJECT}/export` is the
    /// documented example and the folder is made by the first export. What
    /// is not fine is a root whose existing part cannot be resolved at all:
    /// that is [`Reason::Unresolvable`], and the message names the root.
    pub fn parse(template: &str, ctx: &Context) -> Result<Root, Denied> {
        Root::build(template, ctx).map_err(|denied| denied.at("root", template.to_owned()))
    }

    fn build(template: &str, ctx: &Context) -> Result<Root, Denied> {
        if template.trim().is_empty() {
            return Err(Denied::new(Reason::Blank, template));
        }
        let expanded = expand(template, ctx)?;
        let path = inspect(&expanded)?;
        // The root itself is exempt from the no-symlinks rule: a folder the
        // user reached through a link was resolved once, here, and that is
        // the whole of what `${PLUGIN}` means. Requests are judged against
        // the resolved form, so a link retargeted later still cannot take
        // one outside.
        let (prefix, tail) = split_existing(&path)?;
        Ok(Root {
            declared: path,
            prefix,
            tail,
        })
    }

    /// The root as the manifest wrote it, expanded.
    pub fn declared(&self) -> &Path {
        &self.declared
    }

    /// The part of the root that exists, canonicalized.
    pub fn prefix(&self) -> &Path {
        &self.prefix
    }

    /// Whether `path` is this root or inside it.
    ///
    /// Two comparisons, both by component: the resolved prefixes, and then
    /// the tails. The second is what stops a root of `…\export` from
    /// accepting `…\export_backup`.
    fn holds(&self, resolved: &Path, tail: &[OsString]) -> bool {
        resolved.starts_with(&self.prefix) && tail.starts_with(self.tail.as_slice())
    }
}

/// Turns `template` into a path: `${PROJECT}`, `${HOME}` or `${PLUGIN}`,
/// then whatever follows.
///
/// A template is expanded once, at the head, and never re-scanned: a folder
/// whose name happens to contain `${HOME}` is a folder, not a second
/// template. Text after the expansion that still holds `${` is refused,
/// because it means the author expected something this does not do.
fn expand(template: &str, ctx: &Context) -> Result<PathBuf, Denied> {
    let Some(rest) = template.strip_prefix("${") else {
        return if template.contains("${") {
            Err(Denied::new(Reason::Template, template))
        } else {
            Ok(PathBuf::from(template))
        };
    };
    let Some(close) = rest.find('}') else {
        return Err(Denied::new(Reason::Template, template));
    };
    let token = &rest[..close];
    let after = &rest[close + 1..];
    if after.contains("${") {
        return Err(Denied::new(Reason::Template, template).at("residual", after.to_owned()));
    }
    let base = match token {
        "PROJECT" => &ctx.project,
        "HOME" => &ctx.home,
        "PLUGIN" => &ctx.plugin,
        _ => {
            return Err(Denied::new(Reason::Template, template).at("token", token.to_owned()));
        }
    };
    let mut out = base.clone();
    for part in after.split(['\\', '/']).filter(|part| !part.is_empty()) {
        out.push(part);
    }
    // The directory a template names has to be real. `${PROJECT}/export`
    // may name a folder nobody has made yet - the first export makes it -
    // but `${PROJECT}` naming a project that is not there means the root
    // means nothing, and saying that here is far kinder than refusing every
    // call later.
    // Being relative is a stronger complaint than not existing: a relative
    // root is not a folder somebody has yet to make, it is a folder nobody
    // can name. Ask which first, or a typo'd `${PLUGIN}` reads as
    // "unresolvable" and sends the author looking for a missing directory.
    if !out.is_absolute() {
        return Err(Denied::new(Reason::Relative, template));
    }
    if stat(base).is_err() {
        return Err(Denied::new(Reason::Unresolvable, template)
            .at("at", base.to_string_lossy().into_owned()));
    }
    Ok(out)
}

/// The lexical half of the check, in the order the pipeline fixes.
fn inspect(path: &Path) -> Result<PathBuf, Denied> {
    let raw = path.to_string_lossy().into_owned();
    if raw.trim().is_empty() {
        return Err(Denied::new(Reason::Blank, raw));
    }
    shape(path, &raw)?;
    // `Path::components` folds a `.` component away before the loop below
    // can see it (`C:\a\.\b` becomes four components, not five), so the
    // only place left to look for one is the request as it was written.
    for part in raw.split(['\\', '/']) {
        if part == "." {
            return Err(Denied::new(Reason::Dot, raw));
        }
    }
    for part in path.components() {
        if let Component::Normal(name) = part {
            no_bad_characters(name, &raw)?;
        }
    }
    for part in path.components() {
        match part {
            Component::ParentDir => {
                // `raw` is borrowed by the checks below and by the Windows
                // pass further on; a move here would end its life on the
                // first iteration that did not return. Clone on the way out.
                return Err(Denied::new(Reason::Parent, raw.clone()));
            }
            Component::CurDir => {
                return Err(Denied::new(Reason::Dot, raw.clone()));
            }
            _ => {}
        }
    }
    #[cfg(windows)]
    {
        for part in path.components() {
            if let Component::Normal(name) = part {
                let text = name.to_str().ok_or_else(|| {
                    Denied::new(Reason::Invalid, raw.clone()).at("character", "?")
                })?;
                if text.ends_with('.') || text.ends_with(' ') {
                    return Err(
                        Denied::new(Reason::Trailing, raw.clone()).at("component", text.to_owned())
                    );
                }
                if text.contains(':') {
                    return Err(
                        Denied::new(Reason::Colon, raw.clone()).at("component", text.to_owned())
                    );
                }
                if let Some(device) = device_name(text) {
                    return Err(Denied::new(Reason::Device, raw.clone())
                        .at("component", text.to_owned())
                        .at("device", device));
                }
            }
        }
    }
    Ok(path.to_path_buf())
}

/// Whether the path is absolute and written in a form this takes.
#[cfg(windows)]
fn shape(path: &Path, raw: &str) -> Result<(), Denied> {
    use std::path::Prefix;
    let mut parts = path.components();
    match parts.next() {
        Some(Component::Prefix(prefix)) => {
            if !matches!(prefix.kind(), Prefix::Disk(_)) {
                let kind = match prefix.kind() {
                    Prefix::UNC(..) => "unc",
                    Prefix::VerbatimUNC(..) => "verbatim-unc",
                    Prefix::VerbatimDisk(_) | Prefix::Verbatim(_) => "verbatim",
                    _ => "device-ns",
                };
                return Err(Denied::new(Reason::Prefix, raw).at("prefix", kind));
            }
        }
        // `/` or `\`: a root with no drive, which Windows reads as the
        // root of the current drive - a thing that moves.
        Some(Component::RootDir) => {
            return Err(Denied::new(Reason::Prefix, raw).at("prefix", "root-only"));
        }
        _ => return Err(Denied::new(Reason::Relative, raw)),
    }
    match parts.next() {
        Some(Component::RootDir) => Ok(()),
        // `C:` alone, or `C:file`: the current directory on that drive,
        // which the process may change under us.
        _ => Err(Denied::new(Reason::Prefix, raw).at("prefix", "drive-relative")),
    }
}

/// Whether the path is absolute, on a system with one kind of root.
#[cfg(not(windows))]
fn shape(path: &Path, raw: &str) -> Result<(), Denied> {
    match path.components().next() {
        Some(Component::RootDir) => Ok(()),
        _ => Err(Denied::new(Reason::Relative, raw)),
    }
}

/// NUL, control characters, and on Windows the characters a name cannot
/// hold. Reported with the position so the author can find it.
#[cfg(windows)]
fn no_bad_characters(name: &std::ffi::OsStr, raw: &str) -> Result<(), Denied> {
    let text = name
        .to_str()
        .ok_or_else(|| Denied::new(Reason::Invalid, raw).at("character", "?"))?;
    for (index, character) in text.chars().enumerate() {
        if character.is_control() || matches!(character, '<' | '>' | '"' | '|' | '?' | '*') {
            return Err(Denied::new(Reason::Invalid, raw)
                .at("index", index as u64)
                .at("character", character.to_string()));
        }
    }
    Ok(())
}

/// NUL and control characters: the only ones a POSIX name cannot hold.
#[cfg(not(windows))]
fn no_bad_characters(name: &std::ffi::OsStr, raw: &str) -> Result<(), Denied> {
    for (index, character) in name.to_string_lossy().chars().enumerate() {
        if character.is_control() {
            return Err(Denied::new(Reason::Invalid, raw)
                .at("index", index as u64)
                .at("character", character.to_string()));
        }
    }
    Ok(())
}

/// Windows resolves these inside every directory, with or without an
/// extension, and a write to one goes somewhere that is not a file:
/// `COM1` to a serial port, `CON` to the console, `NUL` into nothing. This
/// is an escape of capability rather than of path, which is why it is
/// refused rather than merely warned about.
#[cfg(windows)]
fn device_name(name: &str) -> Option<&'static str> {
    const NAMES: [&str; 24] = [
        "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM1", "COM2", "COM3", "COM4", "COM5",
        "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7",
        "LPT8", "LPT9",
    ];
    // `nul.txt` is the null device too: the extension means nothing.
    let stem = name.split('.').next().unwrap_or(name);
    let upper = stem.to_ascii_uppercase();
    NAMES.iter().copied().find(|device| *device == upper)
}

/// `symlink_metadata`, read through the verbatim form when the plain one
/// is too long for `MAX_PATH`.
///
/// Without this a 300-character path answers "no such file" on a machine
/// where long paths are off, and a path that is merely long would be
/// refused as unresolvable - turning a length into a permission denial,
/// which is the wrong sentence to show anyone.
fn stat(path: &Path) -> io::Result<fs::Metadata> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(meta),
        Err(short) => {
            let long = verbatim(path);
            if long.as_path() == path {
                return Err(short);
            }
            fs::symlink_metadata(&long).map_err(|_| short)
        }
    }
}

/// `canonicalize`, with the same long-path fallback as [`stat`].
fn canonical(path: &Path) -> io::Result<PathBuf> {
    match fs::canonicalize(path) {
        Ok(real) => Ok(real),
        Err(short) => {
            let long = verbatim(path);
            if long.as_path() == path {
                return Err(short);
            }
            fs::canonicalize(&long).or(Err(short))
        }
    }
}

/// A component of `template` that Windows would read as a device or a
/// stream: the load-time warning, before anyone calls anything.
///
/// Not a refusal - a manifest may name `${PROJECT}/NUL` and load - but the
/// author sees it while still looking at the file instead of at the first
/// call that fails. The sentence names the component, because "one of your
/// three write roots is wrong" is not actionable.
#[cfg(windows)]
pub fn suspicious(template: &str) -> Option<String> {
    for (index, part) in template.split(['\\', '/']).enumerate() {
        let part = part.trim();
        if part.is_empty() || part.starts_with('$') {
            continue;
        }
        // A literal root may start with a drive; `D:` is not a stream.
        if index == 0 && part.len() == 2 && part.ends_with(':') {
            continue;
        }
        if let Some(device) = device_name(part) {
            return Some(format!(
                "组件 `{part}` 是 Windows 保留设备名 `{device}`，写出去不会落到文件"
            ));
        }
        if part.contains(':') {
            return Some(format!("组件 `{part}` 含 `:`，可能被解释为备用数据流"));
        }
    }
    None
}

/// Nothing on a system where a name is only a name.
#[cfg(not(windows))]
pub fn suspicious(_template: &str) -> Option<String> {
    None
}

/// Refuses a path that passes through a symbolic link.
///
/// This runs *before* canonicalization, not after: `canonicalize` follows
/// links, so doing it first would turn "this goes through a link" into
/// "this is outside the root", and the two call for different repairs - one
/// says move the file, the other says stop using the link.
///
/// The roots' own spellings are exempt: a root declared as a symlinked
/// folder was resolved once, at load, and a request written the same way
/// must not be refused for the root's own shape.
fn walk_links(path: &Path, roots: &[Root]) -> Result<(), Denied> {
    let mut chain: Vec<&Path> = path.ancestors().collect();
    chain.reverse();
    for step in chain {
        if step.as_os_str().is_empty() {
            continue;
        }
        if roots.iter().any(|root| root.declared.starts_with(step)) {
            continue;
        }
        match stat(step) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(
                    Denied::new(Reason::Symlink, path.to_string_lossy().into_owned())
                        .at("component", step.to_string_lossy().into_owned()),
                );
            }
            Ok(_) => {}
            // Nothing further exists, so nothing further can be a link.
            Err(_) => break,
        }
    }
    Ok(())
}

/// Splits `path` into the longest part that exists, canonicalized, and the
/// components after it.
///
/// Canonicalization that fails does not fall back to the path as written -
/// that fallback is exactly how a dangling symlink got past the API's own
/// check (`resolved`, `concat-api/src/lib.rs:1122-1141`). It refuses.
fn split_existing(path: &Path) -> Result<(PathBuf, Vec<OsString>), Denied> {
    let mut existing = path.to_path_buf();
    let mut tail: Vec<OsString> = Vec::new();
    loop {
        match stat(&existing) {
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let Some(name) = existing.file_name().map(|name| name.to_owned()) else {
                    return Err(Denied::new(
                        Reason::Unresolvable,
                        path.to_string_lossy().into_owned(),
                    )
                    .at("at", existing.to_string_lossy().into_owned()));
                };
                tail.push(name);
                match existing.parent() {
                    Some(parent) if !parent.as_os_str().is_empty() => {
                        existing = parent.to_path_buf();
                    }
                    _ => {
                        return Err(Denied::new(
                            Reason::Unresolvable,
                            path.to_string_lossy().into_owned(),
                        )
                        .at("at", existing.to_string_lossy().into_owned()));
                    }
                }
            }
            Err(_) => {
                return Err(
                    Denied::new(Reason::Unresolvable, path.to_string_lossy().into_owned())
                        .at("at", existing.to_string_lossy().into_owned()),
                );
            }
        }
    }
    tail.reverse();
    let prefix = canonical(&existing).map_err(|_| {
        Denied::new(Reason::Unresolvable, path.to_string_lossy().into_owned())
            .at("at", existing.to_string_lossy().into_owned())
    })?;
    Ok((prefix, tail))
}

/// Judges one request's path against the roots the plugin was granted, and
/// answers the path to use.
///
/// The answer is the resolved prefix with the not-yet-existing tail pushed
/// back on. It is *not* a verbatim path: that is [`open`]'s business, and
/// keeping the two apart is what stops a caller from handing a `\\?\` path
/// in.
pub fn validate(request: &str, roots: &[Root]) -> Result<PathBuf, Denied> {
    if request.contains("${") {
        return Err(Denied::new(Reason::Template, request));
    }
    let path = inspect(Path::new(request))?;
    // Fail closed before anything else is said: a plugin that declared no
    // roots was granted no roots, and the answer must not depend on
    // whether the path happens to exist.
    if roots.is_empty() {
        return Err(Denied::new(Reason::Escape, request).at("roots", ""));
    }
    walk_links(&path, roots)?;
    let (resolved, tail) = split_existing(&path)?;
    let mut names = Vec::with_capacity(roots.len());
    for root in roots {
        names.push(root.declared().to_string_lossy().into_owned());
        if root.holds(&resolved, &tail) {
            let mut out = resolved.clone();
            for part in &tail {
                out.push(part);
            }
            return Ok(out);
        }
    }
    Err(Denied::new(Reason::Escape, request).at("roots", names.join(", ")))
}

/// Why an [`open`] did not open anything.
#[derive(Debug)]
pub enum Failure {
    /// The path is not one the plugin may use.
    Denied(Denied),
    /// It is, and the file system said no.
    Io(io::Error),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Denied(denied) => f.write_str(&denied.message()),
            Failure::Io(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Failure {}

/// Judges the path *again* and then opens it, in that order.
///
/// The second judgement is not an assertion: between the first one and the
/// `open`, a directory on the way can be replaced with a link, which is the
/// one race a path check cannot reason away. Re-running it shrinks the
/// window to a single call. See "Known window" in this module's
/// documentation for why it is not closed here.
pub fn open(request: &str, roots: &[Root], write: bool) -> Result<File, Failure> {
    let resolved = validate(request, roots).map_err(Failure::Denied)?;
    let target = verbatim(&resolved);
    let mut options = OpenOptions::new();
    options.read(true);
    if write {
        options.write(true).create(true);
    }
    options.open(&target).map_err(Failure::Io)
}

/// `path` in the form `CreateFile` reads without its own normalizing, so a
/// path past `MAX_PATH` still opens.
///
/// The host produces this form and never accepts it: two spellings of one
/// file is how a whitelist gets walked past.
#[cfg(windows)]
pub fn verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if text.starts_with(r"\\?\") {
        return path.to_path_buf();
    }
    if let Some(rest) = text.strip_prefix(r"\\") {
        return PathBuf::from(format!(r"\\?\UNC\{rest}"));
    }
    PathBuf::from(format!(r"\\?\{text}"))
}

/// Nothing to add on a system where every path is already final.
#[cfg(not(windows))]
pub fn verbatim(path: &Path) -> PathBuf {
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(dir: &Path) -> Context {
        Context {
            project: dir.to_path_buf(),
            home: dir.to_path_buf(),
            plugin: dir.join("plugin"),
        }
    }

    /// A root that is `dir` itself, and the context that produced it.
    fn one_root(dir: &Path) -> (Context, Root) {
        let ctx = context(dir);
        let root = Root::parse("${PROJECT}", &ctx).expect("a root");
        (ctx, root)
    }

    fn text(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    /// `dir`, then `tail`, written with `/` and translated to whatever this
    /// machine separates with - so the same table runs everywhere.
    fn joined(dir: &Path, tail: &str) -> String {
        let separator = std::path::MAIN_SEPARATOR.to_string();
        let rest = tail.replace(['\\', '/'], &separator);
        format!("{}{separator}{rest}", text(dir))
    }

    // ---- the pipeline -------------------------------------------------

    #[test]
    fn a_blank_path_is_refused() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let (_, root) = one_root(dir.path());
        for blank in ["", "   ", "\t"] {
            assert_eq!(
                validate(blank, std::slice::from_ref(&root))
                    .expect_err("blank")
                    .reason(),
                Reason::Blank,
                "{blank:?}"
            );
        }
    }

    #[test]
    fn a_relative_path_is_refused() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let (_, root) = one_root(dir.path());
        for relative in ["a.txt", "./a.txt", ".", ".."] {
            let reason = validate(relative, std::slice::from_ref(&root))
                .expect_err(relative)
                .reason();
            assert!(
                matches!(reason, Reason::Relative | Reason::Dot | Reason::Parent),
                "{relative:?} gave {reason}"
            );
        }
    }

    #[test]
    fn a_parent_component_is_refused_even_when_it_folds_inside() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let (_, root) = one_root(dir.path());
        let cases = [
            joined(dir.path(), "..\\evil"),
            joined(dir.path(), "a\\..\\..\\evil"),
            joined(dir.path(), "a\\b\\.."),
            joined(dir.path(), "a\\..\\a\\b"),
        ];
        for case in cases {
            assert_eq!(
                validate(&case, std::slice::from_ref(&root))
                    .expect_err(&case)
                    .reason(),
                Reason::Parent,
                "{case}"
            );
        }
    }

    #[test]
    fn a_dot_component_is_refused() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let (_, root) = one_root(dir.path());
        let case = joined(dir.path(), ".\\a");
        assert_eq!(
            validate(&case, std::slice::from_ref(&root))
                .expect_err(&case)
                .reason(),
            Reason::Dot
        );
    }

    #[test]
    fn a_path_with_a_control_character_is_refused() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let (_, root) = one_root(dir.path());
        let case = joined(dir.path(), "a\tb.txt");
        assert_eq!(
            validate(&case, std::slice::from_ref(&root))
                .expect_err("a tab")
                .reason(),
            Reason::Invalid
        );
    }

    #[test]
    fn a_refusal_carries_the_five_fields() {
        // The meta-test: a caller branches on `data.code` and `data.reason`,
        // so a refusal missing either reads as `undefined` on the far side.
        let dir = tempfile::tempdir().expect("a temp dir");
        let (_, root) = one_root(dir.path());
        let case = joined(dir.path(), "..\\evil");
        let value = validate(&case, std::slice::from_ref(&root))
            .expect_err("parent")
            .into_value();
        assert_eq!(value["code"], "plugin.denied");
        assert_eq!(value["reason"], "path.parent");
        assert_eq!(value["requested"], Value::String(case));
    }

    #[test]
    fn a_refusal_names_the_roots_it_checked_and_not_a_resolved_target() {
        // The roots are what the plugin already knows, and the sentence
        // quotes the path as it was written. What it must not carry is the
        // resolved form: that is where the file really is.
        let dir = tempfile::tempdir().expect("a temp dir");
        let elsewhere = dir.path().join("private");
        fs::create_dir_all(&elsewhere).expect("private");
        fs::create_dir_all(dir.path().join("kept")).expect("kept");
        let (_, root) = one_root(&dir.path().join("kept"));
        let wanted = joined(&elsewhere, "secret.mp4");
        let denied = validate(&wanted, std::slice::from_ref(&root)).expect_err("outside");
        assert_eq!(denied.reason(), Reason::Escape);
        assert!(
            denied.message().contains("kept"),
            "the sentence names no root: {}",
            denied.message()
        );
        assert!(denied.data().get("resolved").is_none());
    }

    #[test]
    fn a_template_in_a_request_is_refused() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let (_, root) = one_root(dir.path());
        for case in ["${HOME}/x", "${PROJECT}"] {
            assert_eq!(
                validate(case, std::slice::from_ref(&root))
                    .expect_err(case)
                    .reason(),
                Reason::Template,
                "{case}"
            );
        }
    }

    #[test]
    fn a_path_outside_every_root_is_refused() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let outside = dir.path().join("elsewhere");
        fs::create_dir_all(&outside).expect("elsewhere");
        fs::create_dir_all(dir.path().join("kept")).expect("kept");
        let (_, root) = one_root(&dir.path().join("kept"));
        assert_eq!(
            validate(&text(&outside), std::slice::from_ref(&root))
                .expect_err("outside")
                .reason(),
            Reason::Escape
        );
    }

    #[test]
    fn empty_whitelist_denies_everything() {
        // Deliberately the opposite of `restrict_writes_to`, which reads an
        // empty list as "confine nothing" (concat-api/src/lib.rs:389-391).
        // A plugin that declared no roots was granted no roots.
        let dir = tempfile::tempdir().expect("a temp dir");
        assert_eq!(
            validate(&text(dir.path()), &[])
                .expect_err("nothing granted")
                .reason(),
            Reason::Escape
        );
    }

    // ---- templates ----------------------------------------------------

    #[test]
    fn the_three_templates_expand_and_nothing_else_does() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let ctx = context(dir.path());
        // `${PLUGIN}` names a folder that has to be there already, so make
        // it the way an installation would.
        fs::create_dir_all(&ctx.plugin).expect("the plugin folder");
        for template in ["${PROJECT}/export", "${HOME}/cache", "${PLUGIN}/assets"] {
            Root::parse(template, &ctx).unwrap_or_else(|error| panic!("{template}: {error}"));
        }
        for (template, reason) in [
            ("${PROJ}/x", Reason::Template),
            ("x${PROJECT}/y", Reason::Template),
            ("${PROJECT}${PLUGIN}", Reason::Template),
        ] {
            assert_eq!(
                Root::parse(template, &ctx).expect_err(template).reason(),
                reason,
                "{template}"
            );
        }
    }

    #[test]
    fn a_relative_expansion_is_refused() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let ctx = Context {
            project: dir.path().to_path_buf(),
            home: dir.path().to_path_buf(),
            plugin: PathBuf::from("plugins/credits"),
        };
        assert_eq!(
            Root::parse("${PLUGIN}/assets", &ctx)
                .expect_err("relative")
                .reason(),
            Reason::Relative
        );
    }

    // ---- the lazy path ------------------------------------------------

    #[test]
    fn a_root_that_does_not_exist_yet_is_granted() {
        // `${PROJECT}/export` is the documented example: the folder is made
        // by the first export.
        let dir = tempfile::tempdir().expect("a temp dir");
        let ctx = context(dir.path());
        let root = Root::parse("${PROJECT}/export", &ctx).expect("a root");
        assert_eq!(root.declared(), dir.path().join("export").as_path());
        let wanted = joined(dir.path(), "export\\a.mp4");
        validate(&wanted, &[root]).expect("the export path");
    }

    #[test]
    fn a_sibling_of_a_root_that_does_not_exist_yet_is_refused() {
        // `export_backup` starts with `export` as a string and not as a
        // component; comparing strings rather than components accepts it.
        let dir = tempfile::tempdir().expect("a temp dir");
        let ctx = context(dir.path());
        let root = Root::parse("${PROJECT}/export", &ctx).expect("a root");
        let wanted = joined(dir.path(), "export_backup\\a.mp4");
        assert_eq!(
            validate(&wanted, &[root]).expect_err("sibling").reason(),
            Reason::Escape
        );
    }

    #[test]
    fn a_tail_two_directories_deep_is_granted() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let ctx = context(dir.path());
        let root = Root::parse("${PROJECT}/export", &ctx).expect("a root");
        let wanted = joined(dir.path(), "export\\2026-11\\a.mp4");
        validate(&wanted, &[root]).expect("two levels that do not exist");
    }

    #[test]
    fn a_root_with_nothing_under_it_is_unresolvable() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let ctx = Context {
            project: dir.path().join("gone"),
            home: dir.path().to_path_buf(),
            plugin: dir.path().to_path_buf(),
        };
        let denied = Root::parse("${PROJECT}/export", &ctx).expect_err("gone");
        assert_eq!(denied.reason(), Reason::Unresolvable);
        assert_eq!(denied.field("root"), "${PROJECT}/export");
        assert!(
            denied.message().contains("${PROJECT}/export"),
            "{}",
            denied.message()
        );
    }

    #[test]
    fn roots_that_are_the_same_folder_are_one_root() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let ctx = context(dir.path());
        let first = Root::parse("${PROJECT}", &ctx).expect("a root");
        let second = Root::parse(&text(dir.path()), &ctx).expect("the same root");
        assert_eq!(first.prefix(), second.prefix());
        assert_eq!(first.declared(), second.declared());
    }

    // ---- symlinks -----------------------------------------------------

    #[cfg(windows)]
    #[test]
    fn a_symlinked_component_is_refused() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let inside = dir.path().join("proj");
        let target = dir.path().join("target");
        let outside = dir.path().join("outside");
        for folder in [&inside, &target, &outside] {
            fs::create_dir_all(folder).expect("a folder");
        }
        let root = Root::parse(&text(&inside), &context(&inside)).expect("a root");

        // Pointing inside, pointing outside, and pointing nowhere.
        let cases = [
            (&target, "into"),
            (&outside, "out"),
            (&dir.path().join("gone"), "dangling"),
        ];
        for (target, which) in cases {
            let link = inside.join(format!("link-{which}"));
            match std::os::windows::fs::symlink_dir(target, &link) {
                Ok(()) => {}
                Err(error) => {
                    // A machine without the symlink privilege (Developer
                    // Mode off) cannot run this one; say so rather than
                    // pass silently.
                    eprintln!("skipping {which}: {error}");
                    continue;
                }
            }
            let wanted = joined(&link, "a.txt");
            assert_eq!(
                validate(&wanted, std::slice::from_ref(&root))
                    .expect_err(&wanted)
                    .reason(),
                Reason::Symlink,
                "{which}"
            );
        }
    }

    // ---- the second check ---------------------------------------------

    #[cfg(windows)]
    #[test]
    fn the_open_rechecks_the_path_it_validated() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let inside = dir.path().join("proj");
        let outside = dir.path().join("outside");
        fs::create_dir_all(&inside).expect("proj");
        fs::create_dir_all(&outside).expect("outside");
        let root = Root::parse(&text(&inside), &context(&inside)).expect("a root");

        let folder = inside.join("sub");
        fs::create_dir_all(&folder).expect("sub");
        let wanted = joined(&folder, "a.mp4");
        validate(&wanted, std::slice::from_ref(&root)).expect("before");

        // The swap a path check cannot reason about: the directory the
        // validated path went through is replaced by a link.
        fs::remove_dir_all(&folder).expect("remove");
        if std::os::windows::fs::symlink_dir(&outside, &folder).is_err() {
            eprintln!("skipping: no symlink privilege");
            return;
        }
        let failure = open(&wanted, std::slice::from_ref(&root), true).expect_err("swapped");
        match failure {
            Failure::Denied(denied) => assert_eq!(denied.reason(), Reason::Symlink),
            Failure::Io(error) => panic!("opened anyway: {error}"),
        }
    }

    // ---- Windows' own rules -------------------------------------------

    #[cfg(windows)]
    #[test]
    fn a_component_ending_in_dot_or_space_is_refused() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let (_, root) = one_root(dir.path());
        for case in ["evil.", "evil ", "evil. . ."] {
            let wanted = joined(dir.path(), case);
            assert_eq!(
                validate(&wanted, std::slice::from_ref(&root))
                    .expect_err(case)
                    .reason(),
                Reason::Trailing,
                "{case}"
            );
        }
        // Mid-path, not just at the end: Win32 strips each component.
        let wanted = format!("{}\\sub \\a.txt", text(dir.path()));
        assert_eq!(
            validate(&wanted, std::slice::from_ref(&root))
                .expect_err("mid")
                .reason(),
            Reason::Trailing
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_reserved_device_name_is_refused() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let (_, root) = one_root(dir.path());
        for case in [
            "NUL", "nul.txt", "CON", "AUX", "PRN", "COM1", "LPT9", "CONIN$",
        ] {
            let wanted = joined(dir.path(), case);
            assert_eq!(
                validate(&wanted, std::slice::from_ref(&root))
                    .expect_err(case)
                    .reason(),
                Reason::Device,
                "{case}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_colon_component_is_refused() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let (_, root) = one_root(dir.path());
        for case in ["file.txt:hidden", "file.txt::$DATA"] {
            let wanted = joined(dir.path(), case);
            assert_eq!(
                validate(&wanted, std::slice::from_ref(&root))
                    .expect_err(case)
                    .reason(),
                Reason::Colon,
                "{case}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn every_foreign_prefix_is_refused() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let (_, root) = one_root(dir.path());
        let cases = [
            (r"\\?\C:\proj\a.txt", "verbatim"),
            (r"\\.\C:\proj\a.txt", "device-ns"),
            (r"\\server\share\a.txt", "unc"),
            (r"\\localhost\C$\proj\a.txt", "unc"),
            ("C:", "drive-relative"),
        ];
        for (case, expected) in cases {
            let denied = validate(case, std::slice::from_ref(&root)).expect_err(case);
            assert_eq!(denied.reason(), Reason::Prefix, "{case}");
            assert_eq!(denied.field("prefix"), expected, "{case}");
        }
        // `/` has a root and no drive.
        assert_eq!(
            validate("/", std::slice::from_ref(&root))
                .expect_err("/")
                .reason(),
            Reason::Prefix
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_case_differing_path_is_accepted_after_canonicalizing() {
        // `Path::starts_with` compares components case-sensitively, so
        // comparing the two as written would refuse this. The refusal
        // would be a wrong answer, not a safe one.
        let dir = tempfile::tempdir().expect("a temp dir");
        let (_, root) = one_root(dir.path());
        let shouted = text(dir.path()).to_uppercase();
        let wanted = joined(Path::new(shouted.as_str()), "a.txt");
        validate(&wanted, std::slice::from_ref(&root)).expect("the same folder, spelled louder");
    }

    #[cfg(windows)]
    #[test]
    fn a_long_path_is_opened_through_its_verbatim_form() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let ctx = context(dir.path());
        let root = Root::parse("${PROJECT}", &ctx).expect("a root");
        // 280 characters of folders. Each level is made through its
        // verbatim form, because `create_dir` reads the plain one and a
        // path past MAX_PATH is exactly what this test is about.
        let mut deep = dir.path().to_path_buf();
        for step in 0..14 {
            deep.push(format!("folder-{step:02}-aaaaaaaaaaaaaaaaa"));
            fs::create_dir(verbatim(&deep)).expect("a folder");
        }
        let wanted = joined(&deep, "a.txt");
        assert!(wanted.len() > 260, "{} characters", wanted.len());
        let mut file = open(&wanted, std::slice::from_ref(&root), true).expect("open");
        std::io::Write::write_all(&mut file, b"hello").expect("write");
        drop(file);
        // Read back through `open` too: `fs::read` on the plain spelling is
        // exactly the call that a length must not break.
        let mut back = open(&wanted, std::slice::from_ref(&root), false).expect("reopen");
        let mut text = String::new();
        std::io::Read::read_to_string(&mut back, &mut text).expect("read");
        assert_eq!(text, "hello");
    }
}
