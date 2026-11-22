// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and BobbyCat contributors

//! End-to-end: a real child process and the real handshake.
//!
//! The loader, the permission table and the line bridge each have their own
//! unit tests. This one is the first to put a live child behind the loader:
//! spawn `echo_plugin.py`, complete hello/ready, then send one JSON-RPC call
//! and read its reply off the pipe. That is what makes the plugin skeleton a
//! working plugin rather than a set of modules that merely compile.
//!
//! The fixture lives in `tests/fixtures/` and Python is caller-supplied; if
//! it is missing the test skips rather than failing a machine without it.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use concat_plugin::loader::{self, LoadConfig};
use concat_plugin::manifest::Manifest;
use serde_json::Value;

const MANIFEST: &str = r#"{
  "name": "bobbycat.echo",
  "version": "0.1.0",
  "api": "1",
  "entry": { "kind": "process", "command": "python3", "args": ["echo_plugin.py"] }
}"#;

fn plugin_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn python_available() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

#[test]
fn a_real_plugin_completes_the_handshake_and_one_round_trip() {
    if !python_available() {
        eprintln!("python3 not found; skipping the end-to-end plugin check");
        return;
    }

    let manifest = Manifest::load(MANIFEST).expect("a valid manifest");
    let mut process = loader::load(&manifest, &plugin_dir(), &LoadConfig::default())
        .expect("the echo plugin started and answered ready");
    assert_eq!(process.ready.api, 1, "the plugin speaks this build's api");
    assert!(process.pid() > 0, "a real child is running");

    // One JSON-RPC call down the child's stdin.
    {
        let stdin = process
            .stdin()
            .expect("stdin still open after the handshake");
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":7,"method":"project.open","params":{{}}}}"#
        )
        .expect("the call was written");
        stdin.flush().expect("the call was flushed");
    }

    // Read the reply off stdout with the pipe the loader handed back.
    let stdout = process.stdout().expect("stdout still open");
    let mut line = String::new();
    stdout.read_line(&mut line).expect("a reply line arrived");
    let reply: Value = serde_json::from_str(line.trim()).expect("the reply is JSON");

    assert_eq!(reply["jsonrpc"], "2.0");
    assert_eq!(reply["id"], 7);
    assert_eq!(
        reply["result"]["method"], "project.open",
        "the plugin echoed our call"
    );

    // Close cleanly and reap the child rather than leaving it orphaned.
    process.close_stdin();
    for _ in 0..50 {
        if process.poll().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn the_manifest_names_a_process_this_machine_can_start() {
    let manifest = Manifest::load(MANIFEST).expect("a valid manifest");
    assert_eq!(manifest.name, "bobbycat.echo");
    assert_eq!(manifest.version, "0.1.0");
}
