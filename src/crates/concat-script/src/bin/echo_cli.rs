// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! A stand-in for `bobbycat-cli api`, for the sheet's tests only.
//!
//! It reads one line at a time and writes it back, so a test can check that
//! a script reaches a child's standard input and that its answers come back
//! - without a built CLI, a project on disk, or a shell to write a helper
//! script in. A shell helper will not do: Windows cannot spawn a `.cmd`
//! without `cmd /c`, which is why the tests use this binary's own path
//! (`CARGO_BIN_EXE_concat-script-echo`) instead.

use std::io::{BufRead, Write};

fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        // The `api` argument the sheet passes is not read: a real CLI would
        // read it as a subcommand, and this one answers every line the same.
        if writeln!(stdout, "{line}").is_err() {
            break;
        }
        let _ = stdout.flush();
    }
}
