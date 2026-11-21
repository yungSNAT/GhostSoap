// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Unit tests of the state file.

use super::*;
use crate::core::register::{ChannelKind, Lever};
use crate::core::tracker::{Message, Request};

mod export;

fn profile() -> Profile {
    Profile {
        full_name: "Lara Croft".to_owned(),
        email: Some("lara@croft-manor.example".to_owned()),
        ..Profile::default()
    }
}

fn secret(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

#[test]
fn round_trip_and_wrong_passphrase() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    create(&home, &State::new(profile()), secret("correct horse")).unwrap();
    let raw = fs::read(state_path(&home)).unwrap();
    assert!(
        !String::from_utf8_lossy(&raw).contains("Croft"),
        "state is not plaintext"
    );
    assert!(!home.join(LOCK_FILE).exists());
    assert_eq!(
        load(&home, secret("correct horse")).unwrap().profile,
        profile()
    );
    let err = load(&home, secret("wrong horse")).unwrap_err();
    assert_eq!(err.to_string(), "wrong passphrase");
    let err = create(&home, &State::new(profile()), secret("x")).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
}

#[test]
fn a_held_lock_stops_a_write() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(LOCK_FILE), "").unwrap();
    let err = create(dir.path(), &State::new(profile()), secret("pass phrase")).unwrap_err();
    assert!(err.to_string().contains("another GhostSoap is writing"));
    assert!(!state_path(dir.path()).exists());
}

/// A state file written by `ghostsoap init` at M2 (main at 2286400, format 1).
const M2_FILE: &[u8] = include_bytes!("../../tests/fixtures/state/format-1.age");
const M2_PASSPHRASE: &str = "present day, present time";

/// A data folder holding the M2 state file.
fn m2_home() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(state_path(dir.path()), M2_FILE).unwrap();
    dir
}

fn m2() -> SecretString {
    secret(M2_PASSPHRASE)
}

fn message(tag: &str) -> Message {
    Message {
        tag: tag.to_owned(),
        entry: "cotality-au".to_owned(),
        channel: ChannelKind::Email,
        address: "privacy@example.com.au".to_owned(),
        drafted: jiff::civil::date(2026, 11, 10),
        draft: None,
        superseded: false,
        unfinished: false,
        sends: Vec::new(),
        requests: vec![Request::new(Lever::App7Source)],
    }
}

fn add(tag: &'static str) -> impl FnOnce(&mut State) -> Result<(), String> {
    move |state| {
        state.messages.push(message(tag));
        Ok(())
    }
}

/// Encrypt `text` as the state file in `dir`, with a low work factor to keep tests quick.
fn put(dir: &Path, text: &str, passphrase: &str) {
    let mut recipient = age::scrypt::Recipient::new(secret(passphrase));
    recipient.set_work_factor(10);
    let bytes = age::encrypt(&recipient, text.as_bytes()).unwrap();
    fs::write(state_path(dir), bytes).unwrap();
}

/// Format 2 as first merged (main at fffb4d1) wrote both fixed periods as `days`.
#[test]
fn a_fixed_period_written_as_days_reads_as_the_kind_its_lever_has() {
    let mut state = State::new(profile());
    let mut letter = message("GS-7K3QX");
    letter.requests = vec![
        Request::new(Lever::S20rAccess),
        Request::new(Lever::S20tCorrection),
    ];
    state.messages.push(letter);
    let text = toml::to_string(&state).unwrap();
    let first = text.replacen("days-after = 10", "days = 10", 1).replacen(
        "days-starting = 30",
        "days = 30",
        1,
    );
    assert_eq!(first.matches("\ndays = ").count(), 2, "{first}");
    assert_eq!(decode(&first).unwrap(), state);
    // A kind written by its own name is read as written, and refused where the
    // lever's law does not give it.
    for (from, to, lever) in [
        ("days-after = 10", "days-starting = 10", "s20r-access"),
        ("days-starting = 30", "days-after = 30", "s20t-correction"),
    ] {
        let wrong = text.replacen(from, to, 1);
        assert_eq!(
            decode(&wrong).unwrap_err().to_string(),
            format!(
                "cannot read the state: GS-7K3QX {lever} has a deadline kind its lever's \
                     law does not give"
            )
        );
    }
}

#[test]
fn a_saved_state_asks_for_no_more_work_than_reading_accepts() {
    let bytes = encrypt(&State::new(profile()), secret("pass phrase")).unwrap();
    let header = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).into_owned();
    let stanza = header
        .lines()
        .find_map(|line| line.strip_prefix("-> scrypt "))
        .unwrap_or_else(|| panic!("no scrypt stanza in {header}"));
    let log_n: u8 = stanza.split(' ').nth(1).unwrap().parse().unwrap();
    assert_eq!(log_n, WORK_FACTOR);
}

#[test]
fn an_m2_file_reads_as_format_2_and_the_first_change_replaces_it() {
    let dir = m2_home();
    let path = state_path(dir.path());
    let state = load(dir.path(), m2()).unwrap();
    assert_eq!(state.format, 2);
    let some = |text: &str| Some(text.to_owned());
    let profile = Profile {
        full_name: "Lara Croft".to_owned(),
        alias_names: vec!["Lara Croft-Henshingly".to_owned()],
        email: some("lara@croft-manor.example"),
        phone: some("0400 000 001"),
        date_of_birth: some("1968-02-14"),
        address: some("1 Croft Manor Lane, Surrey Hills VIC 3127"),
        previous_addresses: Vec::new(),
        postcode: some("3127"),
    };
    assert_eq!(state.profile, profile, "the profile is as M2 stored it");
    assert!(state.messages.is_empty());
    assert_eq!(fs::read(&path).unwrap(), M2_FILE, "reading writes nothing");

    update(dir.path(), m2(), |_| Ok(())).unwrap();
    assert_eq!(
        fs::read(&path).unwrap(),
        M2_FILE,
        "a change that changes nothing writes nothing"
    );

    update(dir.path(), m2(), add("GS-7K3QX")).unwrap();
    assert_ne!(fs::read(&path).unwrap(), M2_FILE);
    let state = load(dir.path(), m2()).unwrap();
    assert_eq!(state.messages, [message("GS-7K3QX")]);
    assert_eq!(state.profile, profile, "and unchanged by the first write");
    let left: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|f| f.unwrap().file_name())
        .collect();
    assert_eq!(left, [STATE_FILE], "no lock or temporary file is left");
}

#[test]
fn a_draft_is_handed_over_after_save_and_before_the_next_writer() {
    let dir = m2_home();
    update_then(
        dir.path(),
        m2(),
        add("GS-7K3QX"),
        |_| {},
        |(), _| {
            let state = load(dir.path(), m2()).unwrap();
            assert!(state.message("GS-7K3QX").is_some());
            let competing = update(dir.path(), m2(), add("GS-00AZ9")).unwrap_err();
            assert_eq!(competing.kind(), io::ErrorKind::AlreadyExists);
            Ok(())
        },
    )
    .unwrap();
    update(dir.path(), m2(), add("GS-00AZ9")).unwrap();
}

#[test]
fn what_the_handover_changes_is_saved_only_once_it_succeeds() {
    let dir = m2_home();
    let unfinished = || load(dir.path(), m2()).unwrap().messages[0].unfinished;
    let start = |state: &mut State| {
        let mut message = message("GS-7K3QX");
        message.unfinished = true;
        state.messages.push(message);
        Ok(())
    };
    let finish = |state: &mut State| state.messages[0].unfinished = false;
    let failed = |(), state: &mut State| {
        finish(state);
        Err::<(), _>("the handover failed".to_owned())
    };
    let err = update_then(dir.path(), m2(), start, |_| {}, failed).unwrap_err();
    assert_eq!(err.to_string(), "the handover failed");
    assert!(unfinished(), "the first change stays saved, and only that");

    // A folder sits where the second save is first written.
    let tmp = state_path(dir.path()).with_extension("age.tmp");
    let blocked = |(), state: &mut State| {
        finish(state);
        fs::create_dir(&tmp).unwrap();
        Ok(())
    };
    let err = update_then(dir.path(), m2(), |_| Ok(()), |_| {}, blocked).unwrap_err();
    let said = err.to_string();
    assert!(
        said.starts_with("could not record that the handover finished: "),
        "{said}"
    );
    assert!(unfinished());
    fs::remove_dir(&tmp).unwrap();

    let finished = |(), state: &mut State| {
        finish(state);
        Ok(())
    };
    update_then(dir.path(), m2(), |_| Ok(()), |_| {}, finished).unwrap();
    assert!(!unfinished());
}

#[test]
fn a_failed_migration_leaves_the_m2_file_as_it_was() {
    let dir = m2_home();
    let path = state_path(dir.path());
    let unchanged = |why: &str| assert_eq!(fs::read(&path).unwrap(), M2_FILE, "{why}");

    let err = update(dir.path(), m2(), |state| {
        state.messages.push(message("GS-7K3QX"));
        Err::<(), _>("the change failed".to_owned())
    })
    .unwrap_err();
    assert_eq!(err.to_string(), "the change failed");
    unchanged("a change that fails");

    let err = update(dir.path(), secret("not the passphrase"), add("GS-7K3QX")).unwrap_err();
    assert_eq!(err.to_string(), "wrong passphrase");
    unchanged("a wrong passphrase");

    fs::write(dir.path().join(LOCK_FILE), "").unwrap();
    let err = update(dir.path(), m2(), add("GS-7K3QX")).unwrap_err();
    assert!(err.to_string().contains("another GhostSoap is writing"));
    unchanged("a held lock");
    fs::remove_file(dir.path().join(LOCK_FILE)).unwrap();

    // A folder sits where the new file is first written, so the write fails.
    fs::create_dir(path.with_extension("age.tmp")).unwrap();
    update(dir.path(), m2(), add("GS-7K3QX")).unwrap_err();
    unchanged("a write that fails");
    assert!(!dir.path().join(LOCK_FILE).exists(), "the lock is released");
}

#[test]
fn a_link_at_the_temporary_name_is_replaced_not_written_through() {
    let dir = m2_home();
    let path = state_path(dir.path());
    let tmp = path.with_extension("age.tmp");
    let canary = dir.path().join("canary.txt");
    fs::write(&canary, "unchanged").unwrap();
    fs::hard_link(&canary, &tmp).unwrap();
    update(dir.path(), m2(), add("GS-7K3QX")).unwrap();
    assert_eq!(fs::read_to_string(&canary).unwrap(), "unchanged");
    assert!(!tmp.exists());

    // A link to the state itself: the state must hold the old or the new, never neither.
    fs::hard_link(&path, &tmp).unwrap();
    update(dir.path(), m2(), add("GS-00AZ9")).unwrap();
    assert!(!tmp.exists());
    assert_eq!(load(dir.path(), m2()).unwrap().messages.len(), 2);
}

/// After the rename the new state is in place. A folder that can be written but not
/// read lets the rename through and stops the flush that follows it.
#[cfg(unix)]
#[test]
fn a_failure_after_the_rename_says_the_state_was_saved() {
    use std::os::unix::fs::PermissionsExt;
    let dir = m2_home();
    let mode = |mode| fs::set_permissions(dir.path(), fs::Permissions::from_mode(mode));
    mode(0o300).unwrap();
    let readable = fs::read_dir(dir.path()).is_ok();
    let result = (!readable).then(|| update(dir.path(), m2(), add("GS-7K3QX")));
    mode(0o700).unwrap();
    // Root reads any folder, so the flush cannot be made to fail this way.
    let Some(result) = result else { return };
    let err = result.unwrap_err();
    assert!(
        err.to_string().starts_with("the state was saved, but"),
        "{err}"
    );
    let state = load(dir.path(), m2()).unwrap();
    assert_eq!(state.messages, [message("GS-7K3QX")], "and it reads back");
    assert!(!dir.path().join(LOCK_FILE).exists());
}

#[test]
fn a_state_that_would_not_load_is_never_written() {
    let dir = m2_home();
    let err = update(dir.path(), m2(), |state| {
        state
            .messages
            .extend([message("GS-7K3QX"), message("GS-7K3QX")]);
        Ok(())
    })
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "tag GS-7K3QX is used twice; nothing was saved"
    );
    assert_eq!(fs::read(state_path(dir.path())).unwrap(), M2_FILE);
}

#[test]
fn text_that_would_not_read_back_is_never_written() {
    let state = State::new(profile());
    let unchanged = |text: &str| toml::from_str(text).ok();
    assert!(encode_checked(&state, unchanged).is_ok());
    for read_back in [None, Some(State::new(Profile::default()))] {
        let err = encode_checked(&state, |_| read_back).unwrap_err();
        assert_eq!(
            err.to_string(),
            "the state would not read back as it was written; nothing was saved"
        );
    }
}

#[test]
fn a_newer_format_is_refused_and_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let text = "format = 3\nlater = true\n\n[profile]\nfull_name = \"Lara Croft\"\n";
    put(dir.path(), text, "correct horse");
    let before = fs::read(state_path(dir.path())).unwrap();
    let expected = "state format 3 is newer than this version of GhostSoap reads (format 2); \
                        upgrade GhostSoap";
    let err = load(dir.path(), secret("correct horse")).unwrap_err();
    assert_eq!(err.to_string(), expected);
    let err = update(dir.path(), secret("correct horse"), add("GS-7K3QX")).unwrap_err();
    assert_eq!(err.to_string(), expected);
    assert_eq!(fs::read(state_path(dir.path())).unwrap(), before);
}

#[test]
fn an_unreadable_state_is_reported_without_its_text() {
    let dir = tempfile::tempdir().unwrap();
    for (text, expected) in [
        (
            "format = 2\nprofile = \"Lara Croft\"\n",
            "cannot read the state: it is not as expected at line 2 of its text",
        ),
        (
            "format = 1\n\n[profile]\nfull_name = \"Lara Croft\"\nshoe_size = \"Croft\"\n",
            "cannot read the state: it is not as expected at line 5 of its text",
        ),
        (
            "[profile]\nfull_name = \"Lara Croft\"\n",
            "cannot read the state: it has no format this version knows",
        ),
        (
            "format = 0\n\n[profile]\nfull_name = \"Lara Croft\"\n",
            "cannot read the state: it has no format this version knows",
        ),
    ] {
        put(dir.path(), text, "correct horse");
        let err = load(dir.path(), secret("correct horse")).unwrap_err();
        assert_eq!(err.to_string(), expected, "{text}");
    }
}

#[test]
fn a_state_file_holding_a_broken_rule_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = State::new(profile());
    state
        .messages
        .extend([message("GS-7K3QX"), message("GS-7K3QX")]);
    put(
        dir.path(),
        &toml::to_string(&state).unwrap(),
        "correct horse",
    );
    let err = load(dir.path(), secret("correct horse")).unwrap_err();
    assert_eq!(
        err.to_string(),
        "cannot read the state: tag GS-7K3QX is used twice"
    );
}

#[test]
fn update_without_a_state_says_to_run_init() {
    let dir = tempfile::tempdir().unwrap();
    for home in [dir.path().to_owned(), dir.path().join("absent")] {
        let err = update(&home, secret("correct horse"), add("GS-7K3QX")).unwrap_err();
        assert!(err.to_string().contains("run `ghostsoap init` first"));
        assert!(!home.join(LOCK_FILE).exists());
    }
}

#[test]
fn linux_falls_back_to_the_home_folder_without_xdg_data_home() {
    let vars = |name: &str| (name == "HOME").then(|| "home".into());
    assert_eq!(
        data_dir_for("linux", vars).unwrap(),
        Path::new("home/.local/share/ghostsoap")
    );
}

#[test]
fn data_directory_uses_the_platform_and_explicit_override() {
    let env = std::collections::BTreeMap::from([
        ("LOCALAPPDATA", "appdata"),
        ("HOME", "home"),
        ("XDG_DATA_HOME", "xdg"),
        ("GHOSTSOAP_HOME", ""),
    ]);
    let vars = |name: &str| env.get(name).map(std::ffi::OsString::from);
    for (os, parent, rest) in [
        ("windows", "appdata", "GhostSoap"),
        ("macos", "home", "Library/Application Support/GhostSoap"),
        ("linux", "xdg", "ghostsoap"),
    ] {
        assert_eq!(
            data_dir_for(os, vars).unwrap(),
            Path::new(parent).join(rest)
        );
        assert_eq!(
            data_dir_for(os, |name| (name == "GHOSTSOAP_HOME")
                .then(|| "override".into()))
            .unwrap(),
            Path::new("override")
        );
    }
}

#[test]
fn state_reports_non_missing_read_errors_and_rejects_unreadable_contents() {
    let dir = tempfile::tempdir().unwrap();
    let path = state_path(dir.path());
    fs::create_dir(&path).unwrap();
    let err = load(dir.path(), secret("test passphrase")).unwrap_err();
    assert_ne!(err.kind(), io::ErrorKind::NotFound);
    fs::remove_dir(&path).unwrap();
    fs::write(&path, b"not an age file").unwrap();
    let err = load(dir.path(), secret("test passphrase")).unwrap_err();
    assert!(err.to_string().contains("cannot decrypt"));
}

#[test]
fn a_lock_in_a_missing_parent_preserves_the_os_error() {
    let dir = tempfile::tempdir().unwrap();
    let err = Lock::acquire(&dir.path().join("missing"), LOCK_FILE)
        .err()
        .unwrap();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert!(err.raw_os_error().is_some());
}

#[test]
fn state_rename_retries_a_temporarily_locked_file_only() {
    let mut calls = 0;
    rename_with_retry(|| {
        calls += 1;
        if calls == 1 {
            Err(io::ErrorKind::PermissionDenied.into())
        } else {
            Ok(())
        }
    })
    .unwrap();
    assert_eq!(calls, 2);

    let err = rename_with_retry(|| {
        calls += 1;
        Err(io::ErrorKind::NotFound.into())
    })
    .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert_eq!(calls, 3);
}
