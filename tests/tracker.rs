// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! The tracker end to end: what `draft` and `sent` record in the encrypted state.

mod common;
// Under tests/ itself it would be a test target of its own.
#[path = "tracker/redraft.rs"]
mod redraft;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use age::secrecy::SecretString;
use chrono::{Days, Local, NaiveDate};
use common::{PASSPHRASE, emls, ghostsoap, ghostsoap_with, init, text};
use ghostsoap::core::law::Deadline;
use ghostsoap::core::register::{ChannelKind, Lever};
use ghostsoap::core::tracker::{Send, SendRecord, State};
use ghostsoap::io::state;

/// A state file written by `ghostsoap init` at M2 (format 1), under `PASSPHRASE`.
const M2_FILE: &[u8] = include_bytes!("fixtures/state/format-1.age");

/// Run a command that asks for the passphrase.
fn run(home: &Path, args: &[&str]) -> Output {
    let args = [args, &["--passphrase-stdin"]].concat();
    ghostsoap(home, &args, &format!("{PASSPHRASE}\n"))
}

/// The standard output of a command that succeeded.
fn done(out: &Output) -> String {
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    text(&out.stdout)
}

/// The error from a command that refused.
fn refused(out: &Output) -> String {
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stdout));
    text(&out.stderr)
}

/// The state as GhostSoap stored it.
fn stored(home: &Path) -> State {
    state::load(home, SecretString::from(PASSPHRASE.to_owned())).unwrap()
}

fn state_bytes(home: &Path) -> Vec<u8> {
    fs::read(home.join("state.age")).unwrap()
}

fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// The tags `draft` tells the user to record, in the order it drafted them.
fn tags(stdout: &str) -> Vec<String> {
    let line = stdout
        .lines()
        .find_map(|l| l.strip_prefix("After you send a letter, record it: ghostsoap sent "))
        .expect("draft names its tags");
    line.split(' ').map(str::to_owned).collect()
}

/// A data folder with a profile, and a folder for drafts beside it.
fn start() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let (home, drafts) = (tmp.path().join("home"), tmp.path().join("drafts"));
    init(&home);
    (tmp, home, drafts)
}

fn draft_cotality(home: &Path, drafts: &Path, extra: &[&str]) -> Output {
    let out = drafts.to_str().unwrap();
    let args = [&["draft", "--entry", "cotality-au", "--out", out], extra].concat();
    run(home, &args)
}

/// `start`, then one letter to Cotality drafted: also its tag and its file.
fn start_drafted() -> (tempfile::TempDir, PathBuf, PathBuf, String, PathBuf) {
    let (tmp, home, drafts) = start();
    let [tag] = tags(&done(&draft_cotality(&home, &drafts, &[])))
        .try_into()
        .unwrap();
    let [file] = emls(&drafts).try_into().unwrap();
    (tmp, home, drafts, tag, file)
}

#[test]
fn draft_records_the_letter_it_writes() {
    let (_tmp, home, drafts) = start();
    let before = today();
    let out = draft_cotality(&home, &drafts, &[]);
    let stdout = done(&out);
    assert!(
        !text(&out.stderr).contains("data folder"),
        "a plain folder draws no warning"
    );
    let [tag] = tags(&stdout).try_into().unwrap();
    let [file] = emls(&drafts).try_into().unwrap();
    let subject = format!("Subject: Privacy Act request [{tag}]");
    assert!(fs::read_to_string(&file).unwrap().contains(&subject));

    let state = stored(&home);
    assert_eq!(state.format, 2);
    assert_eq!(state.profile.full_name, "Lara Croft");
    let [message] = state.messages.try_into().unwrap();
    assert_eq!(message.tag, tag);
    assert_eq!(message.entry, "cotality-au");
    assert_eq!(message.channel, ChannelKind::Email);
    assert_eq!(message.address, "AU-Privacy@Cotality.com");
    assert!(before <= message.drafted && message.drafted <= today());
    let recorded = message.draft.as_ref().expect("the draft file is recorded");
    assert_eq!(
        fs::canonicalize(Path::new(&recorded.folder).join(&recorded.name)).unwrap(),
        fs::canonicalize(&file).unwrap()
    );
    assert!(message.sends.is_empty());
    let levers: Vec<Lever> = message.requests.iter().map(|r| r.lever).collect();
    assert_eq!(
        levers,
        [
            Lever::App7NoMarketing,
            Lever::App7NoFacilitation,
            Lever::App7Source
        ]
    );
    let guide = Deadline::Reasonable {
        guide_days: Some(30),
    };
    for request in &message.requests {
        assert_eq!(request.letter_version, 1);
        assert_eq!(request.deadline, guide, "the law table's kind for APP 7");
        assert!(request.events.is_empty());
    }
}

#[test]
fn sent_finds_the_letter_by_its_tag_and_records_it_once() {
    let (_tmp, home, _drafts, tag, _file) = start_drafted();
    let drafted = stored(&home).messages[0].drafted;
    let on = drafted.to_string();
    // The tag as someone might retype it from the subject line.
    let typed = format!("[{}]", tag.to_lowercase());
    let stdout = done(&run(&home, &["sent", &typed, "--on", &on]));
    assert_eq!(
        stdout,
        format!("sent    {tag}  cotality-au  recorded as sent on {on} by email\n")
    );
    let email = Send {
        on: drafted,
        channel: ChannelKind::Email,
    };
    assert_eq!(stored(&home).messages[0].sends, [SendRecord::Sent(email)]);

    let once = state_bytes(&home);
    let stdout = done(&run(&home, &["sent", &tag, "--on", &on]));
    assert_eq!(
        stdout,
        format!("sent    {tag}  cotality-au  already recorded as sent on {on} by email\n")
    );
    assert_eq!(state_bytes(&home), once, "a repeated record writes nothing");

    let by_post = ["sent", &tag, "--on", &on, "--channel", "post"];
    assert!(refused(&run(&home, &by_post)).contains(&format!(
        "error: {tag} is already recorded as sent on {on} by email; add --correct to replace \
         that record"
    )));
    assert_eq!(state_bytes(&home), once);
    done(&run(&home, &[&by_post[..], &["--correct"]].concat()));
    let post = Send {
        channel: ChannelKind::Post,
        ..email
    };
    let state = stored(&home);
    assert_eq!(state.messages.len(), 1);
    assert_eq!(
        state.messages[0].sends,
        [email, post].map(SendRecord::Sent),
        "the first stays as history"
    );
}

#[test]
fn a_withdrawn_record_of_sending_makes_the_letter_a_draft_again() {
    let (_tmp, home, drafts, tag, file) = start_drafted();
    done(&run(&home, &["sent", &tag]));
    for clash in [&["--on", "2026-11-10"][..], &["--correct"]] {
        let args = [&["sent", &tag, "--withdraw"], clash].concat();
        assert_eq!(run(&home, &args).status.code(), Some(2), "{clash:?}");
    }
    let lone_note = run(&home, &["sent", &tag, "--note", "why"]);
    assert_eq!(
        lone_note.status.code(),
        Some(2),
        "a note is for a withdrawal"
    );

    let why = "that was the other letter";
    let stdout = done(&run(&home, &["sent", &tag, "--withdraw", "--note", why]));
    assert_eq!(
        stdout,
        format!("sent    {tag}  cotality-au  no longer recorded as sent\n")
    );
    let message = stored(&home).messages.remove(0);
    assert_eq!(message.sent(), None);
    let withdrawn = SendRecord::Withdrawn {
        note: Some(why.to_owned()),
    };
    assert_eq!(message.sends.len(), 2, "the record stays as history");
    assert_eq!(message.sends[1], withdrawn);
    let again = refused(&run(&home, &["sent", &tag, "--withdraw"]));
    assert!(again.contains(&format!("error: {tag} is not recorded as sent")));

    // A new tag keeps the history attached to the exact letter that was recorded.
    let stdout = done(&draft_cotality(&home, &drafts, &[]));
    let new_tag = tags(&stdout).remove(0);
    assert_ne!(new_tag, tag);
    assert!(!file.exists());
    assert_eq!(emls(&drafts).len(), 1);
    let state = stored(&home);
    let earlier = state.message(&tag).unwrap();
    assert_eq!(earlier.sends, message.sends);
    assert!(earlier.draft.is_none());
    assert!(earlier.superseded);
    assert!(state.message(&new_tag).unwrap().sends.is_empty());

    done(&run(&home, &["sent", &new_tag]));
    let third = done(&draft_cotality(&home, &drafts, &["--lever", "app7-source"]));
    assert!(
        !third.contains("the earlier record stays as history"),
        "the superseded draft cannot be replaced again"
    );
    assert_eq!(stored(&home).message(&tag).unwrap().sends, message.sends);
}

#[test]
fn copy_records_each_letter_without_a_file() {
    let (_tmp, home, drafts) = start();
    let out = drafts.to_str().unwrap();
    let args = [
        "draft",
        "--entry",
        "experian-marketing-au",
        "--copy",
        "--out",
        out,
    ];
    let stdout = done(&run(&home, &args));
    let [form, post] = tags(&stdout).try_into().unwrap();
    assert!(stdout.contains(&format!("[{form}]")) && stdout.contains(&format!("[{post}]")));
    assert!(!drafts.exists());
    let state = stored(&home);
    let recorded: Vec<_> = state
        .messages
        .iter()
        .map(|m| (m.tag.as_str(), m.channel, m.draft.is_some()))
        .collect();
    assert_eq!(
        recorded,
        [
            (form.as_str(), ChannelKind::WebForm, false),
            (post.as_str(), ChannelKind::Post, false)
        ]
    );

    // Copying again before sending keeps both tags and adds no message.
    assert_eq!(
        tags(&done(&run(&home, &args))),
        [form.as_str(), post.as_str()]
    );
    assert_eq!(stored(&home).messages.len(), 2);

    let stdout = done(&run(&home, &["sent", &form, &post]));
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with(&format!("sent    {form}  experian-marketing-au  recorded")));
    assert!(lines[0].ends_with("by web form") && lines[1].ends_with("by post"));
    assert!(stored(&home).messages.iter().all(|m| m.sends.len() == 1));
}

#[test]
fn sent_refuses_what_it_cannot_record_and_then_records_nothing() {
    let (_tmp, home, _drafts, tag, _file) = start_drafted();
    let before = state_bytes(&home);
    let drafted = stored(&home).messages[0].drafted;
    let day_before = (drafted - Days::new(1)).to_string();
    let tomorrow = (today() + Days::new(1)).to_string();

    // One tag of 2 is wrong, so neither is recorded.
    let error = refused(&run(&home, &["sent", &tag, "GS-00000"]));
    assert!(error.contains(
        "error: no letter is recorded under GS-00000; the tag is in the letter's subject line"
    ));
    let error = refused(&run(&home, &["sent", &tag, "--on", &tomorrow]));
    assert!(error.contains(&format!("error: {tomorrow} is in the future")));
    let error = refused(&run(&home, &["sent", &tag, "--on", &day_before]));
    assert!(error.contains(&format!(
        "error: {tag} was drafted on {drafted}, after {day_before}"
    )));
    let error = refused(&run(&home, &["sent", &tag, "--correct"]));
    assert!(error.contains(&format!("error: {tag} has no record of sending to correct")));
    let error = refused(&ghostsoap(
        &home,
        &["sent", &tag, "--passphrase-stdin"],
        "not the passphrase\n",
    ));
    assert!(error.contains("error: wrong passphrase"));

    for (flag, value, hint) in [
        ("--on", "10/11/2026", "use a date such as 2026-11-30"),
        ("--channel", "fax", "use email, web-form, post or phone"),
    ] {
        let out = run(&home, &["sent", &tag, flag, value]);
        assert_eq!(out.status.code(), Some(2));
        assert!(text(&out.stderr).contains(hint), "{hint}");
    }
    assert_eq!(
        run(&home, &["sent"]).status.code(),
        Some(2),
        "a tag is required"
    );

    assert_eq!(state_bytes(&home), before);
    assert!(stored(&home).messages[0].sends.is_empty());
}

#[test]
fn what_the_tracker_records_is_not_readable_in_the_data_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    init(&home);
    // No --out, so the drafts go to the data folder's own drafts folder.
    let [tag] = tags(&done(&run(&home, &["draft", "--entry", "cotality-au"])))
        .try_into()
        .unwrap();
    done(&run(&home, &["sent", &tag]));
    let names = |dir: &Path| -> Vec<String> {
        let names = fs::read_dir(dir).unwrap();
        let mut names: Vec<String> = names
            .map(|f| f.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    };
    assert_eq!(
        names(&home),
        ["drafts", "state.age"],
        "no lock or temporary file"
    );
    let draft = format!("cotality-au-{tag}.eml");
    assert_eq!(
        names(&home.join("drafts")),
        [".ghostsoap-drafts", draft.as_str()]
    );

    done(&ghostsoap(&home, &["clean"], ""));
    assert_eq!(names(&home), ["state.age"], "clean leaves the state alone");
    let state = text(&state_bytes(&home));
    for plain in [
        "Croft",
        "cotality",
        "Cotality",
        "app7",
        tag.as_str(),
        "GS-",
        "drafted",
    ] {
        assert!(
            !state.contains(plain),
            "{plain} is readable in the state file"
        );
    }
    assert_eq!(
        stored(&home).messages[0].sends.len(),
        1,
        "and still decrypts"
    );
}

#[test]
fn a_data_folder_in_a_repository_draws_a_warning_at_init_and_each_change() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    fs::write(
        repo.join(".git"),
        "gitdir: ../elsewhere/.git/worktrees/repo\n",
    )
    .unwrap();
    let home = repo.join("home");
    let warning = "warning: GhostSoap's data folder is inside the Git working tree at";
    let warned = |out: &Output| {
        let stderr = text(&out.stderr);
        assert_eq!(
            out.status.code(),
            Some(0),
            "a warning, not a refusal: {stderr}"
        );
        assert_eq!(stderr.matches(warning).count(), 1, "{stderr}");
        assert!(stderr.contains("; your encrypted state there could be committed"));
    };
    let answers = common::init_answers(PASSPHRASE);
    warned(&ghostsoap(&home, &["init", "--passphrase-stdin"], &answers));
    let out = run(&home, &["draft", "--entry", "cotality-au", "--copy"]);
    warned(&out);
    let [tag] = tags(&text(&out.stdout)).try_into().unwrap();
    warned(&run(&home, &["sent", &tag]));
    assert_eq!(stored(&home).messages[0].sends.len(), 1);

    // Commands that never touch the state say nothing about its folder.
    let plan = ghostsoap_with(&home, &["plan", "--entry", "cotality-au"], "", &[]);
    assert_eq!(plan.status.code(), Some(0));
    assert!(!text(&plan.stderr).contains("data folder"));
}

#[test]
fn a_profile_made_at_m2_drafts_and_tracks_without_another_init() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("state.age"), M2_FILE).unwrap();

    let stdout = done(&run(&home, &["draft", "--entry", "cotality-au", "--copy"]));
    assert!(
        stdout.contains("Name: Lara Croft"),
        "the M2 profile fills the letter"
    );
    assert_ne!(
        state_bytes(&home),
        M2_FILE,
        "the first change stores format 2"
    );
    let [tag] = tags(&stdout).try_into().unwrap();
    done(&run(&home, &["sent", &tag]));
    let state = stored(&home);
    assert_eq!(state.format, 2);
    assert_eq!(state.profile.postcode.as_deref(), Some("3127"));
    assert_eq!(state.messages.len(), 1);
    assert_eq!(state.messages[0].sends.len(), 1);
}
