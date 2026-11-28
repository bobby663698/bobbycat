// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! Tries proposed candidates on a copy of a project, from the command line.
//!
//! The point is the question it asks. In this engine a command that names an
//! id the project does not hold is a *success* that changes nothing, so "no
//! error came back" says almost nothing about a proposal. What this prints is
//! whether the document actually changed, which commands changed nothing, and
//! what the difference was.
//!
//! Nothing is written: every candidate is tried on a scratch copy that is
//! dropped afterwards.
//!
//! ```text
//! cargo run -p concat-autoedit --example check -- <project folder> <candidates.json>
//! ```
//!
//! The candidates file holds a JSON array of candidates, or a single one. The
//! exit code is a failure when any candidate is not clean, so a script can
//! gate on it.

use concat_autoedit::preview::Attempt;
use concat_autoedit::{Candidate, Verdict, check};
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let (Some(project), Some(file), None) = (arguments.next(), arguments.next(), arguments.next())
    else {
        eprintln!("usage: check <project folder> <candidates.json>");
        return ExitCode::from(2);
    };
    match run(Path::new(&project), Path::new(&file)) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("check: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Reports every candidate; `Ok(false)` when one of them had a problem.
fn run(project: &Path, file: &Path) -> Result<bool, String> {
    let text =
        std::fs::read_to_string(file).map_err(|error| format!("{}: {error}", file.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", file.display()))?;
    let candidates: Vec<Candidate> = match value {
        serde_json::Value::Array(_) => {
            serde_json::from_value(value).map_err(|error| format!("{}: {error}", file.display()))?
        }
        single => vec![
            serde_json::from_value(single)
                .map_err(|error| format!("{}: {error}", file.display()))?,
        ],
    };
    if candidates.is_empty() {
        return Err(format!("{} holds no candidates", file.display()));
    }

    let mut clean = 0usize;
    let mut problem = 0usize;
    for one in &candidates {
        println!("candidate {} - {}", one.id, one.title);
        match check(project, one) {
            Ok(Verdict::Clean(attempt)) => {
                clean += 1;
                describe(&attempt, "clean - the document changed");
            }
            Ok(Verdict::Inert(attempt)) => {
                problem += 1;
                describe(&attempt, "inert - every command did nothing");
            }
            Ok(Verdict::Rejected(rejection)) => {
                problem += 1;
                match rejection.command {
                    Some(index) => println!(
                        "  verdict: rejected - command {index} was refused: {}",
                        rejection.why
                    ),
                    None => println!("  verdict: rejected - {}", rejection.why),
                }
            }
            Err(error) => {
                problem += 1;
                println!("  verdict: could not be tried - {error}");
            }
        }
        println!();
    }

    println!(
        "{} candidate(s): {clean} clean, {problem} with a problem",
        candidates.len()
    );
    Ok(problem == 0)
}

/// One candidate's commands, and what they came to.
fn describe(attempt: &Attempt, verdict: &str) {
    let idle: Vec<String> = attempt
        .applied
        .iter()
        .enumerate()
        .filter(|(_, applied)| !**applied)
        .map(|(index, _)| index.to_string())
        .collect();
    let changed = attempt.applied.len() - idle.len();
    println!("  verdict: {verdict}");
    println!(
        "  commands: {} total, {changed} changed something, {} did nothing",
        attempt.applied.len(),
        idle.len()
    );
    if !idle.is_empty() {
        println!("  inert at: {}", idle.join(", "));
    }
    if !attempt.diff.notes.is_empty() {
        let shown: Vec<&str> = attempt
            .diff
            .notes
            .iter()
            .take(8)
            .map(String::as_str)
            .collect();
        println!("  document: {}", shown.join(" | "));
        if attempt.diff.notes.len() > shown.len() {
            println!(
                "  document: ... and {} more",
                attempt.diff.notes.len() - shown.len()
            );
        }
    }
}
