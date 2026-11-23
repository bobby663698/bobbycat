// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The script sheet's engine: run a script through `bobbycat-cli api`.
//!
//! The editor's Script page is a text area and a Run button. Run feeds the
//! script to `bobbycat-cli api`, which reads one API request per line and
//! writes one JSON answer per line, and shows what came back. Spawning the
//! CLI rather than linking the API keeps this crate - and its tests - free
//! of the window's and host's heavy dependency tree: only `std` and a
//! process.
//!
//! The command and working directory come from the caller, never guessed:
//! the window knows where its CLI sits and which project the script is for.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

/// What one run produced.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// One line per answer the CLI wrote, in order. Job events are included:
    /// the CLI writes them as notifications among the replies.
    pub lines: Vec<String>,
    /// The child's exit code, when it exited on its own.
    pub exit: Option<i32>,
    /// A bounded tail of the child's stderr, for a failure with no stdout.
    pub stderr: String,
}

/// A run that could not be started or read.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Failure {
    /// The command could not be spawned.
    Spawn(String),
    /// Writing the script or reading the answers failed.
    Io(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Spawn(why) => write!(f, "cannot start the CLI: {why}"),
            Failure::Io(why) => write!(f, "cannot talk to the CLI: {why}"),
        }
    }
}

impl std::error::Error for Failure {}

/// Where the CLI is and where it runs.
#[derive(Clone, Debug)]
pub struct Config {
    /// The `bobbycat-cli` executable. The window supplies its own path.
    pub command: String,
    /// The child's working directory; the open project's folder when any.
    pub directory: Option<std::path::PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            command: "bobbycat-cli".to_owned(),
            directory: None,
        }
    }
}

/// Feed `script` to a CLI in `config` and collect what it writes back.
///
/// The script is written whole, then stdin is closed: the CLI's `api` loop
/// runs every line and waits for any job it started before exiting, so
/// reading to EOF is waiting for the script to finish. Answers are returned
/// verbatim - the sheet shows them as written - so a job's progress events
/// appear in order among its replies.
pub fn run(script: &str, config: &Config) -> Result<Outcome, Failure> {
    let mut command = Command::new(&config.command);
    command
        .arg("api")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = &config.directory {
        command.current_dir(dir);
    }
    let mut child = command
        .spawn()
        .map_err(|error| Failure::Spawn(error.to_string()))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| Failure::Io("no stdin pipe".to_owned()))?;
    stdin
        .write_all(script.as_bytes())
        .map_err(|error| Failure::Io(error.to_string()))?;
    drop(stdin); // EOF: the CLI knows the script has ended.

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Failure::Io("no stdout pipe".to_owned()))?;
    let mut lines = Vec::new();
    for line in BufReader::new(stdout).lines() {
        lines.push(line.map_err(|error| Failure::Io(error.to_string()))?);
    }

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Failure::Io("no stderr pipe".to_owned()))?;
    let mut err = String::new();
    let _ = BufReader::new(stderr).read_to_string(&mut err);

    let exit = child
        .wait()
        .map_err(|error| Failure::Io(error.to_string()))?
        .code();

    Ok(Outcome {
        lines,
        exit,
        stderr: tail(&err, 2000),
    })
}

/// The last `n` characters, so a long log cannot bury the failure.
fn tail(text: &str, n: usize) -> String {
    let start = text.len().saturating_sub(n);
    // Step back over a partial UTF-8 character if the cut landed in one.
    let mut start = start;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake CLI that echoes each input line back, so a test needs no
    /// built `bobbycat-cli` and no project on disk.
    fn fake_cli() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("scratch");
        #[cfg(windows)]
        {
            let path = dir.path().join("fake-cli.cmd");
            // `more` echoes stdin verbatim and ignores the `api` argument
            // run() passes. findstr /n "^" loses its caret to CMD escaping.
            std::fs::write(&path, "@echo off\r\nmore\r\n").expect("write fake");
            (dir, path.to_string_lossy().into_owned())
        }
        #[cfg(not(windows))]
        {
            let path = dir.path().join("fake-cli.sh");
            std::fs::write(&path, "#!/bin/sh\ncat -n\n").expect("write fake");
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).expect("stat").permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).expect("chmod");
            (dir, path.to_string_lossy().into_owned())
        }
    }

    fn config(command: String) -> Config {
        Config {
            command,
            directory: None,
        }
    }

    #[test]
    fn a_script_is_echoed_back_line_by_line() {
        let (_dir, command) = fake_cli();
        let outcome = run(
            "{\"method\":\"version\"}\n{\"method\":\"project\"}\n",
            &config(command),
        )
        .expect("the fake CLI ran");
        // The fake CLI echoes stdin; shell helpers may add a blank line or a
        // page break, so check what was carried rather than an exact count.
        let echoed = outcome.lines.join("\n");
        assert!(echoed.contains("version"), "{:?}", outcome.lines);
        assert!(echoed.contains("project"), "{:?}", outcome.lines);
        assert_eq!(outcome.exit, Some(0));
    }

    #[test]
    fn a_missing_command_is_a_spawn_failure_not_a_panic() {
        let cfg = config("no-such-bobbycat-cli-binary".to_owned());
        assert!(matches!(run("{}", &cfg), Err(Failure::Spawn(_))));
    }

    #[test]
    fn the_stderr_tail_steps_back_over_a_partial_character() {
        let text = format!("{}{}", "x".repeat(1999), "é");
        let got = tail(&text, 2000);
        assert!(got.ends_with('é'));
        assert!(!got.starts_with('\u{fffd}'));
    }

    #[test]
    fn an_empty_script_closes_stdin_and_returns() {
        let (_dir, command) = fake_cli();
        // The point is that stdin is closed and the child exits; a shell
        // helper may echo one blank line for an empty stream, which is fine.
        let outcome = run("", &config(command)).expect("the fake CLI ran");
        assert_eq!(outcome.exit, Some(0));
    }
}
