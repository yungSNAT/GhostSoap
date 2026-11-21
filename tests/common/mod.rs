// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Running ghostsoap against a data folder of the test's own.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

pub const PASSPHRASE: &str = "present day, present time";

/// Run ghostsoap with `GHOSTSOAP_HOME` set to `home` and `input` on standard input.
pub fn ghostsoap(home: &Path, args: &[&str], input: &str) -> Output {
    ghostsoap_with(home, args, input, &[])
}

/// As `ghostsoap`, with the environment variables `vars` also set for the child only.
pub fn ghostsoap_with(home: &Path, args: &[&str], input: &str, vars: &[(&str, &Path)]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ghostsoap"))
        .args(args)
        .env("GHOSTSOAP_HOME", home)
        .envs(vars.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run ghostsoap");
    // A command that reads no input can exit before the write, closing the pipe; its output
    // and exit status are what the tests check, so a broken pipe here is not a failure.
    if let Err(e) = child.stdin.take().unwrap().write_all(input.as_bytes()) {
        assert_eq!(
            e.kind(),
            std::io::ErrorKind::BrokenPipe,
            "write to ghostsoap: {e}"
        );
    }
    child.wait_with_output().unwrap()
}

pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Answers to `init`: name, other names, email, phone, date of birth, address, previous
/// addresses, postcode, then the passphrase twice.
pub fn init_answers(passphrase: &str) -> String {
    format!(
        "Lara Croft\n\nlara@croft-manor.example\n0400 000 001\n1968-02-14\n\
         1 Croft Manor Lane, Surrey Hills VIC 3127\n\n3127\n{passphrase}\n{passphrase}\n"
    )
}

pub fn init(home: &Path) {
    let out = ghostsoap(
        home,
        &["init", "--passphrase-stdin"],
        &init_answers(PASSPHRASE),
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
}

/// The `.eml` drafts in `dir`.
pub fn emls(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .unwrap()
        .map(|f| f.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "eml"))
        .collect()
}
