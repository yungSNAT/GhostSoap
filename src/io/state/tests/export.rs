// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Plaintext exports of the state.

use super::*;

/// Only a regular file is an export, and only its first bytes are read, so a pipe or a
/// link named like one never blocks `forget` and a large file is not read whole.
#[test]
fn only_a_regular_file_with_the_header_is_an_export() {
    let tmp = tempfile::tempdir().unwrap();
    let named = |n: &str| {
        tmp.path()
            .join(format!("ghostsoap-export-2026-11-{n}.toml"))
    };
    let export = named("17");
    fs::write(&export, format!("{EXPORT_HEADER}2026-11-17. x")).unwrap();
    assert!(is_export(&export));
    let large = named("18");
    let mut text = EXPORT_HEADER.to_owned();
    text.push_str(&"x".repeat(1 << 20));
    fs::write(&large, text).unwrap();
    assert!(is_export(&large));
    let undated = named("31");
    fs::write(&undated, EXPORT_HEADER).unwrap();
    assert!(!is_export(&undated), "30 November is the last day");
    let folder = named("19");
    fs::create_dir(&folder).unwrap();
    assert!(!is_export(&folder));
    #[cfg(unix)]
    {
        let link = named("20");
        std::os::unix::fs::symlink(&export, &link).unwrap();
        assert!(!is_export(&link), "a link is not followed");
        let fifo = named("21");
        let made = std::process::Command::new("mkfifo").arg(&fifo).status();
        assert!(made.unwrap().success());
        assert!(!is_export(&fifo), "a pipe is never opened");
    }
}

#[test]
fn a_failed_export_deletes_its_plaintext_or_says_where_it_is() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("ghostsoap-export-2026-11-17.toml");
    fs::write(&path, "# GhostSoap export of 2026-11-17. part").unwrap();
    let error = discard(&path, &io::Error::other("disk full"));
    assert_eq!(
        error.to_string(),
        format!(
            "disk full; the part-written export at {} was deleted",
            path.display()
        )
    );
    assert!(!path.exists());
    // A folder cannot be deleted as a file, so the message says to delete it.
    let error = discard(tmp.path(), &io::Error::other("disk full")).to_string();
    assert!(
        error.contains("it is plaintext, so delete it yourself"),
        "{error}"
    );
}

#[test]
fn an_export_reads_back_as_the_state_it_was_made_from() {
    let mut state = State::new(profile());
    let mut letter = message("GS-7K3QX");
    letter.requests = vec![
        Request::new(Lever::S20rAccess),
        Request::new(Lever::App7Source),
    ];
    state.messages.push(letter);
    let text = export_text(&state, "2026-11-17").unwrap();
    assert!(text.starts_with("# GhostSoap export of 2026-11-17. This file is plaintext"));
    assert_eq!(decode(&text).unwrap(), state);
}
