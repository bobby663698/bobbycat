// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The Plugins pane: the programs this app did not write, and what they may
//! do. Every plugin is a folder under `<data>/plugins` holding a
//! `bobbycat-plugin.json`. The folder is scanned when the window opens and
//! on demand; a manifest that does not read is kept in the list with the
//! sentence from [`concat_plugin::manifest::Problem`], so a broken plugin
//! is visible instead of silently absent.
//!
//! Enabling a plugin completes the `hello`/`ready` handshake once, so the
//! list says whether the program answers before it is trusted. The child is
//! then dropped: this pane is the register of what is installed and
//! allowed, not the long-lived RPC host - that is the bridge's job. What is
//! remembered across runs is which plugins the person turned off.

use concat_plugin::loader::{self, LoadConfig};
use concat_plugin::manifest::{Manifest, Problem};
use concat_plugin::permissions::{Capability, Grant};
use slint::{ModelRc, SharedString, VecModel};

use crate::panes::Msg;
use crate::studio::Studio;
use crate::ui::{PluginData, PluginsSheetData};

/// The file a plugin folder must hold to be one.
const MANIFEST: &str = "bobbycat-plugin.json";

/// One thing the sheet did.
#[derive(Debug)]
pub enum PluginsMsg {
    /// Open the sheet.
    Open,
    /// Close it.
    Close,
    /// Read the plugins folder again.
    Rescan,
    /// Turn a plugin on: complete its handshake, then remember it.
    Enable { id: String },
    /// Turn a plugin off. Its files stay.
    Disable { id: String },
    /// Delete a plugin's folder.
    Uninstall { id: String },
    /// The handshake worker's verdict: the api it spoke, or why not.
    Loaded {
        id: String,
        result: Result<u32, String>,
    },
}

/// One row of the list: a plugin folder and what became of it.
struct Row {
    /// Folder name, the id used by prefs and the messages.
    id: String,
    /// Its manifest, when it read and passed its own checks.
    manifest: Option<Manifest>,
    /// Why it cannot be used, in words a person can act on.
    problem: String,
    /// The handshake result when it was last enabled.
    state: String,
}

impl Row {
    fn name(&self) -> String {
        self.manifest
            .as_ref()
            .map(|m| m.name.clone())
            .unwrap_or_else(|| self.id.clone())
    }

    fn enabled(&self, studio: &Studio) -> bool {
        self.manifest.is_some()
            && !studio
                .prefs
                .disabled_plugins
                .iter()
                .any(|id| id == &self.id)
    }

    fn capabilities(&self) -> String {
        match &self.manifest {
            Some(m) => {
                let grant = Grant::new(m.permissions.capabilities.iter().copied());
                let names: Vec<&str> = grant.held().map(Capability::as_str).collect();
                if names.is_empty() {
                    "-".to_owned()
                } else {
                    names.join(", ")
                }
            }
            None => String::new(),
        }
    }
}

#[derive(Default)]
pub struct PluginsPane {
    open: bool,
    rows: Vec<Row>,
    scanned: bool,
    error: String,
}

impl PluginsPane {
    /// The pane's state, as Slint reads it.
    pub fn data(&self, studio: &Studio) -> PluginsSheetData {
        let rows: Vec<PluginData> = self
            .rows
            .iter()
            .map(|row| PluginData {
                id: SharedString::from(row.id.clone()),
                name: SharedString::from(row.name()),
                version: SharedString::from(
                    row.manifest
                        .as_ref()
                        .map(|m| m.version.clone())
                        .unwrap_or_default(),
                ),
                capabilities: SharedString::from(row.capabilities()),
                detail: SharedString::from(if row.problem.is_empty() {
                    row.state.clone()
                } else {
                    row.problem.clone()
                }),
                enabled: row.enabled(studio),
            })
            .collect();
        PluginsSheetData {
            open: self.open,
            plugins: ModelRc::new(VecModel::from(rows)),
            error: SharedString::from(self.error.clone()),
        }
    }

    /// Handle one message. As in every pane, this is the only code that
    /// changes the pane's state.
    pub fn update(&mut self, msg: PluginsMsg, studio: &mut Studio) {
        match msg {
            PluginsMsg::Open => {
                self.open = true;
                self.error.clear();
                if !self.scanned {
                    self.rescan(studio);
                }
            }
            PluginsMsg::Close => self.open = false,
            PluginsMsg::Rescan => self.rescan(studio),
            PluginsMsg::Enable { id } => self.enable(&id, studio),
            PluginsMsg::Disable { id } => {
                self.set_disabled(studio, &id, true);
                if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
                    row.state = crate::i18n::t("plugins.disabled");
                }
            }
            PluginsMsg::Uninstall { id } => self.uninstall(&id, studio),
            PluginsMsg::Loaded { id, result } => {
                // Only a plugin that answered is trusted as enabled. A
                // failed handshake leaves the choice as it was and shows
                // why in the row.
                match result {
                    Ok(api) => {
                        self.set_disabled(studio, &id, false);
                        if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
                            row.state = crate::i18n::tf("plugins.readyApi", &[&api]);
                        }
                    }
                    Err(error) => {
                        if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
                            row.state = error;
                        }
                    }
                }
            }
        }
    }

    /// Read the plugins folder. A folder whose manifest will not read is
    /// kept with its problem so the person can see and remove it.
    fn rescan(&mut self, studio: &Studio) {
        self.scanned = true;
        self.error.clear();
        self.rows.clear();
        let root = plugins_dir(studio);
        let Ok(entries) = std::fs::read_dir(&root) else {
            // No plugins folder yet is the usual case, not an error.
            return;
        };
        let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if !entry.path().is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            let file = entry.path().join(MANIFEST);
            let row = match std::fs::read_to_string(&file) {
                Ok(text) => match Manifest::load(&text) {
                    Ok(m) => match m.check() {
                        Ok(()) => Row {
                            id,
                            manifest: Some(m),
                            problem: String::new(),
                            state: String::new(),
                        },
                        Err(problem) => Row::problem(id, problem),
                    },
                    Err(problem) => Row::problem(id, problem),
                },
                Err(error) => Row {
                    id,
                    manifest: None,
                    problem: format!("{MANIFEST}: {error}"),
                    state: String::new(),
                },
            };
            self.rows.push(row);
        }
    }

    /// Complete the plugin's handshake and, if it answers, remember it as
    /// enabled. A refusal is shown in the row, never trusted.
    fn enable(&mut self, id: &str, studio: &mut Studio) {
        let Some(index) = self.rows.iter().position(|row| row.id == id) else {
            return;
        };
        let Some(manifest) = self.rows[index].manifest.clone() else {
            return;
        };
        let dir = plugins_dir(studio).join(id);
        // The handshake waits at most a few seconds; run it off the window
        // thread so a plugin that never answers cannot freeze the UI. The
        // verdict crosses back as a message, like the Script page's run.
        let who = id.to_owned();
        let config = LoadConfig::default();
        crate::host::spawn_long(
            move || {
                loader::load(&manifest, &dir, &config)
                    .map(|process| process.ready.api)
                    .map_err(|error| error.to_string())
            },
            move |studio, _, _, result| {
                studio.handle(Msg::Plugins(PluginsMsg::Loaded { id: who, result }));
            },
        );
        if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
            row.state = crate::i18n::t("plugins.starting");
        }
    }

    fn set_disabled(&self, studio: &mut Studio, id: &str, disabled: bool) {
        let list = &mut studio.prefs.disabled_plugins;
        if disabled {
            if !list.iter().any(|value| value == id) {
                list.push(id.to_owned());
            }
        } else {
            list.retain(|value| value != id);
        }
        studio.prefs.save(&studio.host.dirs);
    }

    fn uninstall(&mut self, id: &str, studio: &mut Studio) {
        let dir = plugins_dir(studio).join(id);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {
                self.set_disabled(studio, id, false);
                self.rows.retain(|row| row.id != id);
            }
            Err(error) => self.error = format!("{id}: {error}"),
        }
    }
}

impl Row {
    fn problem(id: String, problem: Problem) -> Row {
        Row {
            id,
            manifest: None,
            problem: problem.message(),
            state: String::new(),
        }
    }
}

/// Where plugins live: beside the other large, user-supplied state.
fn plugins_dir(studio: &Studio) -> std::path::PathBuf {
    studio.host.dirs.data.join("plugins")
}
