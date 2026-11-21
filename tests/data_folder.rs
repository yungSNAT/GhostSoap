// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! The commands that keep data, when the data folder or standard input fails them.

use std::process::{Command, Output};

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Run with the data folder settings removed, or with `GHOSTSOAP_HOME` set to `home`, and
/// `input` on standard input.
fn in_data_folder(home: Option<&std::path::Path>, args: &[&str], input: &str) -> Output {
    use std::io::Write;
    let mut command = Command::new(env!("CARGO_BIN_EXE_ghostsoap"));
    command.args(args);
    for name in ["GHOSTSOAP_HOME", "LOCALAPPDATA", "HOME", "XDG_DATA_HOME"] {
        command.env_remove(name);
    }
    if let Some(home) = home {
        command.env("GHOSTSOAP_HOME", home);
    }
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // A child that exits before reading closes the pipe; its output is what is tested.
    if let Err(e) = child.stdin.take().unwrap().write_all(input.as_bytes()) {
        assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe, "write stdin: {e}");
    }
    child.wait_with_output().unwrap()
}

#[test]
fn every_command_that_keeps_data_says_how_to_name_a_data_folder() {
    for args in [
        &["init", "--passphrase-stdin"][..],
        &["checklist"],
        &["checklist", "done", "breach-check", "--passphrase-stdin"],
        &["export", "--passphrase-stdin"],
        &["forget", "--passphrase-stdin"],
    ] {
        let out = in_data_folder(None, args, "");
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            text(&out.stderr).contains("cannot find a data directory; set GHOSTSOAP_HOME"),
            "{args:?}: {}",
            text(&out.stderr)
        );
    }
}

#[test]
fn commands_that_read_standard_input_report_its_end_and_change_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("state.age"), "not a state").unwrap();
    for args in [
        &["checklist", "--passphrase-stdin"][..],
        &["checklist", "done", "breach-check", "--passphrase-stdin"],
        &["export", "--passphrase-stdin"],
        &["forget", "--passphrase-stdin"],
        &["forget", "--no-passphrase", "--passphrase-stdin"],
    ] {
        let out = in_data_folder(Some(tmp.path()), args, "");
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            text(&out.stderr).contains("input ended"),
            "{args:?}: {}",
            text(&out.stderr)
        );
    }
    assert_eq!(
        std::fs::read(tmp.path().join("state.age")).unwrap(),
        b"not a state"
    );
}

#[test]
fn forget_deletes_nothing_when_it_cannot_read_the_state_or_the_folder() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("state.age"), "not a state").unwrap();
    let out = in_data_folder(
        Some(tmp.path()),
        &["forget", "--passphrase-stdin"],
        "passphrase\n",
    );
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    let state = tmp.path().join("state.age");
    assert!(
        text(&out.stderr).contains(&format!("cannot decrypt {}", state.display())),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(std::fs::read(&state).unwrap(), b"not a state");

    let out = in_data_folder(Some(&state), &["forget", "--no-passphrase", "--yes"], "");
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    let not_a_folder = std::fs::read_dir(&state).unwrap_err().to_string();
    assert!(
        text(&out.stderr).contains(&not_a_folder),
        "{}",
        text(&out.stderr)
    );
    assert!(state.is_file());
}
