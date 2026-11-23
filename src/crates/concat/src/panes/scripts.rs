// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The Script pane: one API request per line, run through the CLI.
//!
//! The sheet is a text area and a Run button. Run hands the script to
//! [`concat_script`], which starts `bobbycat-cli api` on a worker so a long
//! script - one that exports - never freezes the window. When it returns,
//! the answers are shown verbatim beneath the script. The command path is
//! taken from the running executable's folder, where the app carries its
//! CLI; a missing one is shown as an error in the sheet, not a crash.

use concat_script::{Config, Failure, Outcome};
use slint::SharedString;

use crate::host;
use crate::panes::Msg;
use crate::studio::Studio;
use crate::ui::ScriptsSheetData;

/// One thing the sheet did.
#[derive(Debug)]
pub enum ScriptsMsg {
    /// The tool: open the sheet.
    Open,
    /// Close it. A run already going is not stopped by closing the sheet.
    Close,
    /// The script as typed.
    SourceEdited(String),
    /// Run what is in the editor.
    Run,
    /// Clear the script and its last output.
    Clear,
    /// The worker is done.
    Finished(Result<Outcome, Failure>),
}

#[derive(Default)]
pub struct ScriptsPane {
    open: bool,
    source: String,
    output: String,
    running: bool,
    error: String,
}

impl ScriptsPane {
    /// The pane's state, as Slint reads it.
    pub fn data(&self) -> ScriptsSheetData {
        ScriptsSheetData {
            open: self.open,
            source: SharedString::from(self.source.clone()),
            output: SharedString::from(self.output.clone()),
            running: self.running,
            error: SharedString::from(self.error.clone()),
        }
    }

    /// Handle one message. Takes the pane out of the studio so a worker's
    /// callback can borrow the studio without two owners of the same pane.
    pub fn update(&mut self, msg: ScriptsMsg, studio: &mut Studio) {
        match msg {
            ScriptsMsg::Open => {
                self.open = true;
                self.error.clear();
            }
            ScriptsMsg::Close => self.open = false,
            ScriptsMsg::SourceEdited(text) => self.source = text,
            ScriptsMsg::Clear => {
                self.source.clear();
                self.output.clear();
                self.error.clear();
            }
            ScriptsMsg::Run => {
                if self.running || self.source.trim().is_empty() {
                    return;
                }
                self.running = true;
                self.output.clear();
                self.error.clear();

                let script = self.source.clone();
                let config = cli_config();
                host::spawn_long(
                    move || concat_script::run(&script, &config),
                    |studio, _, _, result| {
                        studio.handle(Msg::Scripts(ScriptsMsg::Finished(result)))
                    },
                );
            }
            ScriptsMsg::Finished(result) => {
                self.running = false;
                match result {
                    Ok(outcome) => {
                        self.output = outcome.lines.join("\n");
                        if self.output.is_empty() {
                            self.error = outcome.stderr;
                        } else if let Some(code) = outcome.exit
                            && code != 0
                        {
                            self.error = format!("CLI exited {code}: {}", outcome.stderr);
                        }
                    }
                    Err(error) => self.error = error.to_string(),
                }
            }
        }
        let _ = studio; // kept in the signature so the worker's `then` can route through it
    }
}

/// Where this build keeps its CLI. Next to the running executable on a
/// desk install; on the development machine the workspace builds it into
/// the same target folder, so the two are the same rule.
fn cli_config() -> Config {
    let command = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(cli_name())))
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "bobbycat-cli".to_owned());
    Config {
        command,
        directory: None,
    }
}

fn cli_name() -> &'static str {
    #[cfg(windows)]
    {
        "bobbycat-cli.exe"
    }
    #[cfg(not(windows))]
    {
        "bobbycat-cli"
    }
}
