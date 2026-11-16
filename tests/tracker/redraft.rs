// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! Drafting again before sending, and draft runs that fail part way.

use super::*;

#[test]
fn a_failed_state_replacement_leaves_the_previous_draft_and_history() {
    let (_tmp, home, drafts, tag, file) = start_drafted();
    done(&run(&home, &["sent", &tag]));
    done(&run(&home, &["sent", &tag, "--withdraw"]));
    let before = state_bytes(&home);
    let previous = fs::read(&file).unwrap();
    let history = stored(&home).message(&tag).unwrap().sends.clone();
    let blocked = home.join("state.age.tmp");
    fs::create_dir(&blocked).unwrap();

    let error = refused(&draft_cotality(&home, &drafts, &["--lever", "app7-source"]));
    assert_eq!(state_bytes(&home), before);
    assert_eq!(
        fs::read(&file).unwrap_or_else(|e| panic!("read earlier draft after {error:?}: {e}")),
        previous
    );
    assert_eq!(emls(&drafts).as_slice(), std::slice::from_ref(&file));

    fs::remove_dir(&blocked).unwrap();
    let new = done(&draft_cotality(&home, &drafts, &["--lever", "app7-source"]));
    assert_ne!(tags(&new).as_slice(), std::slice::from_ref(&tag));
    assert_eq!(stored(&home).message(&tag).unwrap().sends, history);
    assert!(!file.exists());
    assert_eq!(emls(&drafts).len(), 1);
}

#[test]
fn a_manifest_error_leaves_no_unconfirmed_replacement_and_allows_retry() {
    let (tmp, home, drafts, tag, file) = start_drafted();
    let earlier_bytes = fs::read(&file).unwrap();
    done(&run(&home, &["sent", &tag]));
    done(&run(&home, &["sent", &tag, "--withdraw"]));
    let history = stored(&home).message(&tag).unwrap().sends.clone();
    let (output, blocked) = blocked_drafts(tmp.path());

    let error = refused(&draft_cotality(&home, &output, &["--lever", "app7-source"]));
    assert!(error.contains(".ghostsoap-drafts"), "{error}");
    assert!(emls(&output).is_empty());
    assert_eq!(stored(&home).message(&tag).unwrap().sends, history);
    let backup = fs::read_dir(&drafts)
        .unwrap()
        .find_map(|entry| {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let name = name.to_str()?;
            (name.starts_with('.') && name.contains(&tag) && name.ends_with(".tmp"))
                .then(|| entry.path())
        })
        .unwrap_or_else(|| {
            panic!("the earlier letter is recoverable after publication fails: {error}")
        });
    let resolved = fs::canonicalize(&backup).unwrap();
    assert!(error.contains(&resolved.display().to_string()), "{error}");
    assert_eq!(fs::read(backup).unwrap(), earlier_bytes);

    fs::remove_dir(&blocked).unwrap();
    let new = done(&draft_cotality(&home, &output, &["--lever", "app7-source"]));
    assert_ne!(tags(&new).as_slice(), std::slice::from_ref(&tag));
    assert_eq!(stored(&home).message(&tag).unwrap().sends, history);
    assert_eq!(emls(&output).len(), 1);
}

#[test]
fn sent_refuses_a_letter_whose_draft_run_did_not_finish() {
    let (tmp, home, drafts, tag, _file) = start_drafted();
    let (output, blocked) = blocked_drafts(tmp.path());
    // The redraft takes over the tag, then stops before its letter is written; the user may
    // still hold the earlier letter under that tag, which asked for other things.
    refused(&draft_cotality(&home, &output, &["--lever", "app7-source"]));
    // `clean` names the earlier letter's recovery copy and leaves it.
    let [copy] = fs::read_dir(&drafts)
        .unwrap()
        .map(|f| f.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".tmp"))
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let cleaned = done(&ghostsoap(
        &home,
        &["clean", "--out", drafts.to_str().unwrap()],
        "",
    ));
    assert!(
        cleaned.contains(&format!("check   {copy}: left by a write")),
        "{cleaned}"
    );
    assert!(drafts.join(&copy).exists());
    let before = state_bytes(&home);
    let error = refused(&run(&home, &["sent", &tag]));
    assert!(
        error.contains(&format!(
            "error: the draft run that recorded {tag} did not finish, so the letter you hold \
             under that tag may not be the one recorded; draft it again and send that letter"
        )),
        "{error}"
    );
    assert_eq!(state_bytes(&home), before, "nothing is recorded");

    fs::remove_dir(&blocked).unwrap();
    let again = done(&draft_cotality(&home, &output, &["--lever", "app7-source"]));
    assert_eq!(tags(&again), [tag.as_str()], "the retry keeps the tag");
    done(&run(&home, &["sent", &tag]));
    let message = stored(&home).messages.remove(0);
    assert!(message.sent().is_some());
    assert_eq!(message.requests.len(), 1);
}

#[cfg(unix)]
#[test]
fn redrafting_finds_the_old_file_after_a_parent_symlink_moves() {
    use std::os::unix::fs::symlink;

    let (tmp, home, _) = start();
    let first = tmp.path().join("first");
    let second = tmp.path().join("second");
    let link = tmp.path().join("link");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    symlink(&first, &link).unwrap();
    let out = link.join("drafts");
    let original = done(&draft_cotality(&home, &out, &[]));
    let [old_file] = emls(&first.join("drafts")).try_into().unwrap();

    fs::remove_file(&link).unwrap();
    symlink(&second, &link).unwrap();
    let redraft = done(&draft_cotality(&home, &out, &["--lever", "app7-source"]));
    assert_eq!(tags(&redraft), tags(&original));
    assert!(
        !old_file.exists(),
        "the original plaintext draft was removed"
    );
    assert_eq!(emls(&second.join("drafts")).len(), 1);
}

#[test]
fn drafting_again_before_sending_replaces_the_draft_under_its_tag() {
    let (_tmp, home, drafts, tag, file) = start_drafted();
    let first = fs::read_to_string(&file).unwrap();

    let stdout = done(&draft_cotality(&home, &drafts, &["--lever", "app7-source"]));
    assert!(stdout.contains(&format!(
        "note    cotality-au: this draft replaces the earlier one under {tag} and no longer \
         asks for app7-no-marketing, app7-no-facilitation\n"
    )));
    assert_eq!(tags(&stdout), [tag.as_str()]);
    assert_eq!(
        emls(&drafts),
        [file.as_path()],
        "one draft, at the same name"
    );
    let second = fs::read_to_string(&file).unwrap();
    assert_ne!(second, first, "the file holds the new letter");
    let state = stored(&home);
    assert_eq!(state.messages.len(), 1);
    assert_eq!(state.messages[0].tag, tag);
    assert_eq!(state.messages[0].requests.len(), 1);
    assert_eq!(state.messages[0].requests[0].lever, Lever::App7Source);
}

#[test]
fn a_changed_draft_stays_and_a_sent_letter_is_never_replaced() {
    let (_tmp, home, drafts, tag, file) = start_drafted();
    // A draft the user changed is theirs: it stays, and so does the record of it.
    fs::write(&file, "my own words").unwrap();
    let before = state_bytes(&home);
    let error = refused(&draft_cotality(&home, &drafts, &[]));
    assert!(
        error.contains(&format!(
            "has the name of the earlier draft under {tag}, and GhostSoap cannot confirm it \
             still holds the letter it wrote; move or delete it yourself, then draft again"
        )),
        "{error}"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "my own words");
    assert_eq!(state_bytes(&home), before);

    // Once the letter is recorded as sent, drafting again starts another message.
    fs::remove_file(&file).unwrap();
    assert_eq!(
        tags(&done(&draft_cotality(&home, &drafts, &[]))),
        [tag.as_str()]
    );
    done(&run(&home, &["sent", &tag]));
    let stdout = done(&draft_cotality(&home, &drafts, &[]));
    assert!(!stdout.contains("replaces the earlier one"));
    let [another] = tags(&stdout).try_into().unwrap();
    assert_ne!(another, tag);
    assert_eq!(
        emls(&drafts).len(),
        2,
        "the sent letter's draft is left alone"
    );
    let state = stored(&home);
    let recorded: Vec<(&str, usize)> = state
        .messages
        .iter()
        .map(|m| (m.tag.as_str(), m.sends.len()))
        .collect();
    assert_eq!(recorded, [(tag.as_str(), 1), (another.as_str(), 0)]);
}

#[test]
fn a_batch_with_an_edited_draft_leaves_other_drafts_in_place() {
    let (_tmp, home, drafts) = start();
    let args = [
        "draft",
        "--entry",
        "experian-marketing-au",
        "--out",
        drafts.to_str().unwrap(),
    ];
    done(&run(&home, &args));
    let stored_state = stored(&home);
    let [first, second] = stored_state.messages.as_slice() else {
        panic!("the entry has two drafts");
    };
    let first = drafts.join(&first.draft.as_ref().unwrap().name);
    let second = drafts.join(&second.draft.as_ref().unwrap().name);
    let contents = fs::read(&first).unwrap();
    fs::write(&second, "my edits").unwrap();
    let before = state_bytes(&home);

    let failure = refused(&run(&home, &args));
    assert!(failure.contains("cannot confirm"));
    assert_eq!(fs::read(first).unwrap(), contents);
    assert_eq!(fs::read_to_string(second).unwrap(), "my edits");
    assert_eq!(state_bytes(&home), before);
}
