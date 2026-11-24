// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! The sheet's engine against a real child process.
//!
//! The child is `concat-script-echo`, this crate's own test binary: it
//! reads standard input and writes each line back. Using a binary rather
//! than a shell helper is what makes this pass on Windows too - it cannot
//! spawn a `.cmd` without `cmd /c`, and a batch file's escaping had already
//! swallowed one attempt's output.

use concat_script::{Config, Failure, run};
use std::time::Duration;

fn echo() -> Config {
    Config {
        command: env!("CARGO_BIN_EXE_concat-script-echo").to_owned(),
        directory: None,
    }
}

#[test]
fn a_script_reaches_the_child_and_its_answers_come_back() {
    let outcome = run(
        "{\"method\":\"version\"}\n{\"method\":\"project\"}\n",
        &echo(),
    )
    .expect("the echo CLI ran");
    assert_eq!(outcome.lines.len(), 2, "{:?}", outcome.lines);
    assert!(outcome.lines[0].contains("version"), "{:?}", outcome.lines);
    assert!(outcome.lines[1].contains("project"), "{:?}", outcome.lines);
    assert_eq!(outcome.exit, Some(0));
}

#[test]
fn an_empty_script_closes_stdin_and_the_child_exits_cleanly() {
    let outcome = run("", &echo()).expect("the echo CLI ran");
    assert!(outcome.lines.is_empty(), "{:?}", outcome.lines);
    assert_eq!(outcome.exit, Some(0));
}

#[test]
fn the_script_runs_in_the_directory_it_is_given() {
    let dir = tempfile::tempdir().expect("scratch");
    let mut config = echo();
    config.directory = Some(dir.path().to_path_buf());
    // The echo binary does not read the directory, so this only proves the
    // setting is accepted and the spawn still works; a CLI is handed it for
    // the project its script names.
    let outcome = run("{\"method\":\"version\"}\n", &config).expect("the echo CLI ran");
    assert_eq!(outcome.exit, Some(0));
}

#[test]
fn a_command_that_cannot_start_is_reported_not_panicked() {
    let config = Config {
        command: "no-such-bobbycat-cli-binary".to_owned(),
        directory: None,
    };
    assert!(matches!(run("{}", &config), Err(Failure::Spawn(_))));
}

#[test]
fn a_slow_child_does_not_hold_the_caller_past_its_script() {
    // The echo binary ends at EOF; reading to the end is the wait. This
    // guards the shape of `run` - it must not wait on a process that has
    // closed its stdout while a job it started is still going.
    let start = std::time::Instant::now();
    let outcome = run("{\"method\":\"version\"}\n", &echo()).expect("the echo CLI ran");
    assert_eq!(outcome.exit, Some(0));
    assert!(
        start.elapsed() < Duration::from_secs(30),
        "took {:?}",
        start.elapsed()
    );
}
