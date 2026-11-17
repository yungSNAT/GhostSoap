// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! The M4 checklist end to end: listing, `--next` and the marks kept in the state.

#[allow(dead_code)] // the shared helpers include some only other test files use
mod common;

use std::fs;
use std::path::Path;
use std::process::Output;

use common::{PASSPHRASE, ghostsoap, init, text};

const ITEMS: [&str; 12] = [
    "breach-check",
    "credit-reports",
    "credit-ban",
    "do-not-call",
    "silent-number",
    "silent-elector",
    "results-about-you",
    "unused-accounts",
    "my-health-record",
    "loyalty-marketing",
    "social-media",
    "doxxing",
];

fn done(out: &Output) -> String {
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    text(&out.stdout)
}

fn with_passphrase(home: &Path, args: &[&str]) -> Output {
    let args = [args, &["--passphrase-stdin"]].concat();
    ghostsoap(home, &args, &format!("{PASSPHRASE}\n"))
}

#[test]
fn without_a_state_the_checklist_lists_every_item_and_asks_no_passphrase() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let out = ghostsoap(&home, &["checklist"], "");
    let stdout = done(&out);
    assert!(text(&out.stderr).contains("Progress is not shown"));
    let ids: Vec<&str> = stdout
        .lines()
        .map(|l| l.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(ids, ITEMS);
    assert!(stdout.starts_with(
        "breach-check       Check whether your email address was in a data breach  (minutes)\n"
    ));

    let next = done(&ghostsoap(&home, &["checklist", "--next"], ""));
    assert!(next.starts_with(
        "Check whether your email address was in a data breach  (breach-check)\n\
         Applies: Everyone\nEffort: minutes. Do it every 6 months.\n\nWhy: "
    ));
    for line in [
        "\nSteps:\n1. Search each email address you use on Have I Been Pwned.\n",
        "\nLinks:\n- Have I Been Pwned: https://haveibeenpwned.com/ (checked ",
        "\nWhen it is done: ghostsoap checklist done breach-check\n\
         If it is not for you: ghostsoap checklist not-for-me breach-check\n",
    ] {
        assert!(next.contains(line), "missing {line:?} in:\n{next}");
    }
    assert!(!next.contains("Status:"), "{next}");
}

#[test]
fn marks_are_kept_encrypted_and_shown_as_progress() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    init(&home);
    let marked = done(&with_passphrase(
        &home,
        &[
            "checklist",
            "done",
            "do-not-call",
            "breach-check",
            "--on",
            "2026-01-02",
        ],
    ));
    // breach-check is checked again every 6 months; do-not-call once is enough.
    assert_eq!(
        marked,
        "breach-check  check again (done 2026-01-02)\ndo-not-call  done 2026-01-02\n"
    );
    let state = fs::read(home.join("state.age")).unwrap();
    assert!(
        !text(&state).contains("breach-check"),
        "marks are encrypted"
    );
    done(&with_passphrase(
        &home,
        &["checklist", "not-for-me", "silent-elector"],
    ));

    let list = done(&with_passphrase(&home, &["checklist"]));
    let line = |id: &str| {
        list.lines()
            .find(|l| l.starts_with(&format!("{id} ")))
            .unwrap()
            .to_owned()
    };
    assert!(line("silent-elector").contains(" not for me "), "{list}");
    assert!(line("do-not-call").contains(" done 2026-01-02 "), "{list}");
    assert!(line("doxxing").contains(" to do "), "{list}");
    assert!(list.ends_with("\nto do 9  check again 1  done 1  not for me 1\n"));

    let next = done(&with_passphrase(&home, &["checklist", "--next"]));
    assert!(next.contains("(breach-check)\nStatus: check again (done 2026-01-02)\n"));
    let cleared = done(&with_passphrase(
        &home,
        &["checklist", "clear", "breach-check"],
    ));
    assert_eq!(cleared, "breach-check  to do\n");
    let next = done(&with_passphrase(&home, &["checklist", "--next"]));
    assert!(next.contains("(breach-check)\nStatus: to do\n"), "{next}");

    let out = ghostsoap(&home, &["checklist", "--no-passphrase"], "");
    assert!(!done(&out).contains("done 2026-01-02"));
    assert!(text(&out.stderr).contains("Progress is not shown"));
}

/// A mark survives the M2 state's migration, a letter recorded after it and `export`.
#[test]
fn marks_survive_the_m2_migration_other_changes_and_export() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir(&home).unwrap();
    fs::write(
        home.join("state.age"),
        include_bytes!("fixtures/state/format-1.age"),
    )
    .unwrap();
    let marked = done(&with_passphrase(
        &home,
        &["checklist", "done", "do-not-call", "--on", "2026-01-02"],
    ));
    assert_eq!(marked, "do-not-call  done 2026-01-02\n");
    let drafted = done(&with_passphrase(
        &home,
        &["draft", "--entry", "cotality-au", "--copy"],
    ));
    assert!(drafted.contains("Name: Lara Croft"), "the M2 profile stays");

    let export = tmp.path().join("export.toml");
    done(&with_passphrase(
        &home,
        &["export", "--out", export.to_str().unwrap()],
    ));
    let exported = fs::read_to_string(&export).unwrap();
    for wanted in [
        "item = \"do-not-call\"",
        "on = \"2026-01-02\"",
        "mark = \"done\"",
        "entry = \"cotality-au\"",
    ] {
        assert!(
            exported.contains(wanted),
            "{wanted} missing from {exported}"
        );
    }
    let list = done(&with_passphrase(&home, &["checklist"]));
    assert!(list.contains(" done 2026-01-02 "), "{list}");
}

#[test]
fn an_unknown_item_or_a_wrong_passphrase_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    init(&home);
    let before = fs::read(home.join("state.age")).unwrap();
    let out = ghostsoap(
        &home,
        &["checklist", "done", "breach-check", "no-such-item"],
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("no checklist item is called no-such-item"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("Passphrase"),
        "refused before the passphrase"
    );
    let out = ghostsoap(
        &home,
        &["checklist", "--passphrase-stdin"],
        "not the passphrase\n",
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("error: wrong passphrase"));
    let out = ghostsoap(
        &home,
        &["checklist", "done", "breach-check", "--passphrase-stdin"],
        "not the passphrase\n",
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("error: wrong passphrase"));
    assert_eq!(fs::read(home.join("state.age")).unwrap(), before);
}

#[test]
fn next_says_so_when_nothing_is_left() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    init(&home);
    let marks = [&["checklist", "done"][..], &ITEMS[..]].concat();
    done(&with_passphrase(&home, &marks));
    let next = done(&with_passphrase(&home, &["checklist", "--next"]));
    assert_eq!(next, "Nothing left: every item is done or not for you.\n");
}
