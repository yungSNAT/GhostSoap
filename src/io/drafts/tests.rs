// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Unit tests of the drafts folder.

use super::*;

mod remove;

#[test]
fn drafts_ignore_other_extensions_and_report_filesystem_errors() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    let err = write(&drafts, "example-au-GS-ABCDE.docx", "text").unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(!drafts.exists());

    #[cfg(unix)]
    {
        fs::write(&drafts, "").unwrap();
        let err = read_manifest(&drafts).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotADirectory);
        let err = remove_if_empty(&drafts, |_| panic!("a file is never removed")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotADirectory);
    }
}

#[test]
fn a_failed_manifest_replacement_removes_only_its_own_copy() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path().join(MANIFEST);
    fs::create_dir(&manifest).unwrap();
    let prior = dir.path().join(".ghostsoap-drafts.1-1.tmp");
    fs::write(&prior, "prior copy").unwrap();
    let record = (PENDING.to_owned(), "example-au-GS-ABCDE.eml".to_owned());
    assert!(save_manifest(dir.path(), &[record]).is_err());
    assert!(manifest.is_dir());
    assert_eq!(fs::read_to_string(prior).unwrap(), "prior copy");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn cleanup_forgives_an_absent_committed_draft_but_reports_other_read_errors() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    let name = "example-au-GS-ABCDE.eml";
    let path = write(&drafts, name, "letter").unwrap();
    fs::remove_file(&path).unwrap();
    assert_eq!(clean(&drafts, false).unwrap(), Cleaned::default());
    assert!(!drafts.join(MANIFEST).exists());

    let path = write(&drafts, name, "letter").unwrap();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(clean(&drafts, false).is_err());
    assert!(path.is_dir());
    assert_eq!(
        read_manifest(&drafts).unwrap(),
        [(sha256(b"letter"), name.to_owned())]
    );
}

#[test]
fn a_removed_folder_is_retried_by_write_and_ignored_by_clean() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    let called = std::cell::Cell::new(false);
    let lock = lock_for_write(&drafts, |dir| {
        if !called.replace(true) {
            create_private_dir(dir).unwrap();
            fs::remove_dir(dir).unwrap();
            Lock::acquire(dir, LOCK)
        } else {
            lock_new(dir)
        }
    })
    .unwrap();
    assert!(drafts.join(LOCK).exists());
    drop(lock);
    assert_eq!(
        clean_with_lock(&drafts, true, || {
            fs::remove_dir(&drafts).unwrap();
            Lock::acquire(&drafts, LOCK)
        })
        .unwrap(),
        Cleaned::default()
    );
    remove_if_empty(&drafts, |_| panic!("a missing folder is never removed")).unwrap();
}

#[test]
fn cleanup_leaves_new_arrivals_and_reports_other_removal_errors() {
    let dir = tempfile::tempdir().unwrap();
    let note = dir.path().join("keep.txt");
    remove_if_empty(dir.path(), |dir| {
        fs::write(&note, "keep").unwrap();
        fs::remove_dir(dir)
    })
    .unwrap();
    assert_eq!(fs::read_to_string(&note).unwrap(), "keep");
    fs::remove_file(note).unwrap();
    let err =
        remove_if_empty(dir.path(), |_| Err(io::ErrorKind::PermissionDenied.into())).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert!(dir.path().is_dir());
}

#[test]
fn clean_deletes_only_drafts_it_wrote() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    write(&drafts, "quantium-au-GS-7K3QX.eml", "x").unwrap();
    write(&drafts, "talefin-au-GS-00AZ9.txt", "y").unwrap();
    write(&drafts, "anz-au-GS-00AZ8.pdf", "z").unwrap();
    fs::write(drafts.join("notes.txt"), "keep").unwrap();
    fs::write(
        drafts.join("notes-GS-ABCDE.txt"),
        "keep: named like a draft",
    )
    .unwrap();
    assert!(
        write(&drafts, "notes.txt", "x").is_err(),
        "not a draft name"
    );
    assert!(
        write(&drafts, "../x-GS-ABCDE.txt", "x").is_err(),
        "no folder part"
    );
    assert!(
        write(&drafts, "a\nnotes-GS-ABCDE.txt", "x").is_err(),
        "no line break"
    );
    let cleaned = clean(&drafts, true).unwrap();
    assert_eq!(cleaned.deleted, 3);
    assert!(drafts.join("notes.txt").exists());
    assert!(drafts.join("notes-GS-ABCDE.txt").exists());
    assert!(!drafts.join(MANIFEST).exists());
    assert!(!drafts.join(LOCK).exists());
    // A file named like a draft that GhostSoap has no record of is reported, never deleted.
    let reported = Cleaned {
        unrecorded: vec!["notes-GS-ABCDE.txt".to_owned()],
        ..Cleaned::default()
    };
    assert_eq!(cleaned.unrecorded, reported.unrecorded);
    assert_eq!(clean(&drafts, true).unwrap(), reported);
}

#[test]
fn a_failed_write_cannot_authorise_deleting_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    write(&drafts, "seed-GS-11111.eml", "seed").unwrap();
    let path = drafts.join("a-GS-22222.eml");
    fs::write(&path, "letter").unwrap();
    let err = write(&drafts, "a-GS-22222.eml", "letter").unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    // A write that published but stopped before committing leaves a pending name only.
    add_record(&drafts, PENDING, "b-GS-33333.eml").unwrap();
    fs::write(drafts.join("b-GS-33333.eml"), "unconfirmed").unwrap();
    let cleaned = clean(&drafts, false).unwrap();
    assert_eq!(cleaned.deleted, 1, "only the seed draft");
    assert_eq!(fs::read_to_string(&path).unwrap(), "letter");
    assert_eq!(cleaned.unconfirmed, ["b-GS-33333.eml"]);
    assert!(drafts.join("b-GS-33333.eml").exists());
    assert_eq!(
        cleaned.unrecorded,
        ["a-GS-22222.eml"],
        "named like a draft, never written"
    );
    let again = clean(&drafts, false).unwrap();
    assert_eq!(
        again.unconfirmed,
        ["b-GS-33333.eml"],
        "reported until dealt with"
    );
    assert_eq!(again.deleted, 0);
}

#[test]
fn clean_keeps_a_committed_file_that_now_holds_other_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    let path = write(&drafts, "a-GS-11111.eml", "letter").unwrap();
    fs::rename(&path, dir.path().join("moved.eml")).unwrap();
    fs::write(&path, "the user's own file").unwrap();
    add_record(&drafts, &"0".repeat(64), "b-GS-22222.eml").unwrap();
    fs::write(drafts.join("b-GS-22222.eml"), "also the user's").unwrap();
    let cleaned = clean(&drafts, true).unwrap();
    assert_eq!(cleaned.deleted, 0);
    assert_eq!(cleaned.kept, ["a-GS-11111.eml", "b-GS-22222.eml"]);
    assert_eq!(fs::read_to_string(&path).unwrap(), "the user's own file");
    assert!(drafts.exists());
    assert_eq!(
        clean(&drafts, true).unwrap().kept,
        cleaned.kept,
        "still reported by the next clean"
    );
}

#[test]
fn a_name_written_again_reports_only_its_latest_draft() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    let path = write(&drafts, "a-GS-11111.eml", "first").unwrap();
    fs::remove_file(&path).unwrap();
    write(&drafts, "a-GS-11111.eml", "second").unwrap();
    let cleaned = clean(&drafts, true).unwrap();
    assert_eq!((cleaned.deleted, cleaned.kept.len()), (1, 0));
    assert!(!drafts.exists());
    // A second write of a name that stops after publishing, with the same or other bytes,
    // leaves its draft unconfirmed: the earlier commit no longer authorises deleting it.
    for second in ["letter", "changed"] {
        let path = write(&drafts, "b-GS-22222.eml", "letter").unwrap();
        fs::remove_file(&path).unwrap();
        add_record(&drafts, PENDING, "b-GS-22222.eml").unwrap();
        fs::write(&path, second).unwrap();
        let cleaned = clean(&drafts, false).unwrap();
        assert_eq!(cleaned.deleted, 0, "{second}");
        assert_eq!(cleaned.unconfirmed, ["b-GS-22222.eml"], "{second}");
        assert!(cleaned.kept.is_empty(), "{second}");
        assert_eq!(fs::read_to_string(&path).unwrap(), second);
        fs::remove_file(&path).unwrap();
    }
}

#[test]
fn a_hard_linked_manifest_is_replaced_not_written_through() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    write(&drafts, "a-GS-11111.eml", "x").unwrap();
    fs::remove_file(drafts.join(MANIFEST)).unwrap();
    let canary = dir.path().join("canary.txt");
    fs::write(&canary, "unchanged").unwrap();
    fs::hard_link(&canary, drafts.join(MANIFEST)).unwrap();
    write(&drafts, "b-GS-22222.eml", "y").unwrap();
    assert_eq!(fs::read_to_string(&canary).unwrap(), "unchanged");
    assert_eq!(clean(&drafts, false).unwrap().deleted, 1);
    assert_eq!(fs::read_to_string(&canary).unwrap(), "unchanged");
}

#[test]
fn an_interrupted_manifest_update_keeps_the_previous_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    write(&drafts, "a-GS-11111.eml", "x").unwrap();
    // A copy left half-written by an update that stopped before its rename.
    let stale = format!("{MANIFEST}.1-1.tmp");
    fs::write(drafts.join(&stale), "pending").unwrap();
    write(&drafts, "b-GS-22222.eml", "y").unwrap();
    let cleaned = clean(&drafts, true).unwrap();
    assert_eq!(cleaned.deleted, 2);
    assert_eq!(
        cleaned.leftovers,
        std::slice::from_ref(&stale),
        "reported, not deleted"
    );
    assert_eq!(fs::read_to_string(drafts.join(&stale)).unwrap(), "pending");
}

#[test]
fn a_file_at_an_old_manifest_temp_name_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("mine");
    fs::create_dir(&out).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&out, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let other = out.join(format!("{MANIFEST}.tmp"));
    fs::write(&other, "keep this").unwrap();
    write(&out, "a-GS-11111.eml", "x").unwrap();
    let cleaned = clean(&out, false).unwrap();
    assert_eq!(cleaned.deleted, 1);
    assert!(
        cleaned.leftovers.is_empty(),
        "not GhostSoap's, so not reported"
    );
    assert_eq!(fs::read_to_string(&other).unwrap(), "keep this");
}

#[test]
fn clean_refuses_a_path_that_is_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("not-a-folder");
    fs::write(&file, "x").unwrap();
    assert!(clean(&file, true).is_err());
    assert!(file.exists());
}

#[test]
fn clean_removes_the_folder_only_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("mine");
    write(&out, "a-GS-11111.eml", "x").unwrap();
    assert_eq!(clean(&out, false).unwrap().deleted, 1);
    assert!(out.exists(), "a folder the user named stays");
    write(&out, "a-GS-11111.eml", "x").unwrap();
    assert_eq!(clean(&out, true).unwrap().deleted, 1);
    assert!(!out.exists(), "GhostSoap's own emptied folder is removed");
}

/// FAT, exFAT and some network shares refuse hard links, as EPERM on Linux and as an
/// unsupported function on Windows; CI has no such volume, so the errors are made here.
#[test]
fn a_folder_without_hard_links_is_named_as_the_likely_cause() {
    let path = Path::new("/media/usb/drafts/a-GS-11111.eml");
    for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::Unsupported] {
        let error = link_failure(io::Error::from(kind), path);
        assert_eq!(error.kind(), kind);
        let message = error.to_string();
        assert!(message.starts_with("could not create /media/usb/drafts/a-GS-11111.eml: "));
        assert!(
            message.contains("FAT, exFAT and some network shares"),
            "{message}"
        );
        assert!(message.ends_with("choose a folder on an internal drive with --out"));
    }
    let exists = link_failure(io::Error::from(io::ErrorKind::AlreadyExists), path);
    assert_eq!(exists.to_string(), "entity already exists");
}

#[test]
fn a_draft_never_replaces_a_file_and_leftovers_stay_reported() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    let path = write(&drafts, "a-GS-11111.eml", "first").unwrap();
    let err = write(&drafts, "a-GS-11111.eml", "second").unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(&path).unwrap(), "first");
    // Another writer's temporary file with this process's name is neither used nor
    // removed.
    let tmp_name = format!(".b-GS-22222.eml.{}.tmp", std::process::id());
    let tmp = drafts.join(&tmp_name);
    fs::write(&tmp, "canary").unwrap();
    assert!(write(&drafts, "b-GS-22222.eml", "x").is_err());
    assert_eq!(fs::read_to_string(&tmp).unwrap(), "canary");
    assert!(!drafts.join("b-GS-22222.eml").exists());
    let cleaned = clean(&drafts, true).unwrap();
    assert_eq!(cleaned.deleted, 1);
    assert_eq!(cleaned.leftovers, std::slice::from_ref(&tmp_name));
    assert_eq!(
        clean(&drafts, true).unwrap().leftovers,
        [tmp_name],
        "still reported without a manifest"
    );
    assert!(tmp.exists(), "leftovers are reported, not deleted");
}

#[test]
fn a_held_lock_stops_write_and_clean() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    write(&drafts, "a-GS-11111.eml", "x").unwrap();
    fs::write(drafts.join(LOCK), "").unwrap();
    let err = write(&drafts, "b-GS-22222.eml", "y").unwrap_err();
    assert!(err.to_string().contains("another GhostSoap"), "{err}");
    assert!(clean(&drafts, true).is_err());
    assert!(drafts.join("a-GS-11111.eml").exists());
    assert!(
        drafts.join(LOCK).exists(),
        "someone else's lock is left alone"
    );
}

#[cfg(unix)]
#[test]
fn drafts_are_private_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    let path = write(&drafts, "a-GS-11111.eml", "x").unwrap();
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&drafts), 0o700);
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(&drafts.join(MANIFEST)), 0o600);
    let open = dir.path().join("open");
    fs::create_dir(&open).unwrap();
    fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).unwrap();
    let err = write(&open, "a-GS-11111.eml", "x").unwrap_err();
    assert!(err.to_string().contains("chmod 700"), "{err}");
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&drafts, &link).unwrap();
    assert!(
        write(&link, "b-GS-22222.eml", "x").is_err(),
        "a link is refused"
    );
    assert!(clean(&link, true).is_err(), "clean refuses a link too");
}

#[cfg(unix)]
#[test]
fn a_linked_manifest_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    write(&drafts, "a-GS-11111.eml", "x").unwrap();
    fs::remove_file(drafts.join(MANIFEST)).unwrap();
    let canary = dir.path().join("canary.txt");
    fs::write(&canary, "unchanged").unwrap();
    std::os::unix::fs::symlink(&canary, drafts.join(MANIFEST)).unwrap();
    assert!(write(&drafts, "b-GS-22222.eml", "y").is_err());
    assert!(clean(&drafts, true).is_err());
    assert_eq!(fs::read_to_string(&canary).unwrap(), "unchanged");
}

#[cfg(unix)]
#[test]
fn draft_checks_report_errors_other_than_a_missing_draft() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    let name = "a-GS-11111.eml";
    let path = write(&drafts, name, "letter").unwrap();
    // A name longer than the file system allows cannot be looked up at all.
    let long = format!("{}-GS-11111.eml", "a".repeat(300));
    let err = draft_present(&drafts.join(&long)).unwrap_err();
    assert_ne!(err.kind(), io::ErrorKind::NotFound);
    assert!(draft_status(&drafts, &long, &[]).is_err());

    let records = read_manifest(&drafts).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    let readable = fs::read(&path).is_ok();
    let status = (!readable).then(|| draft_status(&drafts, name, &records));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    // Root reads any file, so the read cannot be made to fail this way.
    let Some(status) = status else { return };
    let kind = status.err().map(|e| e.kind());
    assert_eq!(kind, Some(io::ErrorKind::PermissionDenied));
}
