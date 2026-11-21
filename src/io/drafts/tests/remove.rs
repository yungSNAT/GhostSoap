// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Removing one draft, and staging several for removal.

use super::*;

#[test]
fn remove_deletes_one_draft_it_wrote_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    let gone = write(&drafts, "a-GS-11111.eml", "first").unwrap();
    let stays = write(&drafts, "b-GS-22222.eml", "second").unwrap();
    let mut batch = Redraft::default();
    batch.stage(&drafts, "a-GS-11111.eml").unwrap();
    batch.commit();
    batch.discard();
    assert!(!gone.exists());
    assert_eq!(fs::read_to_string(&stays).unwrap(), "second");
    assert_eq!(
        read_manifest(&drafts).unwrap().len(),
        2,
        "the stale record is safe to leave until the next write or clean"
    );
    assert!(stage_remove(&drafts, "a-GS-11111.eml").unwrap().is_none());
    assert!(!drafts.join(LOCK).exists());
    // The name can be written again, and `clean` still deletes what is left.
    write(&drafts, "a-GS-11111.eml", "third").unwrap();
    assert_eq!(clean(&drafts, true).unwrap().deleted, 2);
}

#[test]
fn remove_leaves_a_file_it_cannot_confirm_it_wrote() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    let changed = write(&drafts, "a-GS-11111.eml", "letter").unwrap();
    fs::write(&changed, "the user's edit").unwrap();
    // A write that published but stopped before committing.
    add_record(&drafts, PENDING, "b-GS-22222.eml").unwrap();
    fs::write(drafts.join("b-GS-22222.eml"), "unconfirmed").unwrap();
    // A file GhostSoap never recorded.
    fs::write(drafts.join("c-GS-33333.eml"), "the user's own").unwrap();
    for name in ["a-GS-11111.eml", "b-GS-22222.eml", "c-GS-33333.eml"] {
        assert_eq!(
            stage_remove(&drafts, name).unwrap_err().kind(),
            io::ErrorKind::InvalidData,
            "{name}"
        );
        assert!(drafts.join(name).exists(), "{name}");
    }
    let cleaned = clean(&drafts, false).unwrap();
    assert_eq!(cleaned.kept, ["a-GS-11111.eml"], "clean still reports it");
    assert_eq!(cleaned.unconfirmed, ["b-GS-22222.eml"]);
}

#[test]
fn remove_finds_nothing_where_no_draft_could_be() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("not-a-folder");
    fs::write(&file, "the user's file").unwrap();
    for folder in [dir.path().join("absent"), file.clone()] {
        Redraft::default().stage(&folder, "a-GS-11111.eml").unwrap();
    }
    assert_eq!(fs::read_to_string(&file).unwrap(), "the user's file");
    assert!(!dir.path().join("absent").exists(), "nothing is created");
}

#[test]
fn a_held_lock_stops_remove() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    let path = write(&drafts, "a-GS-11111.eml", "x").unwrap();
    fs::write(drafts.join(LOCK), "").unwrap();
    let err = Redraft::default()
        .stage(&drafts, "a-GS-11111.eml")
        .unwrap_err();
    assert!(err.to_string().contains("another GhostSoap"), "{err}");
    assert!(path.exists());
}

#[test]
fn a_batch_stages_several_drafts_in_one_folder_under_one_lock() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    let a = write(&drafts, "a-GS-11111.eml", "first").unwrap();
    let b = write(&drafts, "b-GS-22222.eml", "second").unwrap();
    let mut batch = Redraft::default();
    batch.stage(&drafts, "a-GS-11111.eml").unwrap();
    batch.stage(&drafts, "b-GS-22222.eml").unwrap();
    assert_eq!(batch.locks.len(), 1);
    batch.commit();
    batch.unlock();
    batch.discard();
    assert!(!a.exists() && !b.exists());
    assert!(!drafts.join(LOCK).exists());
}

#[test]
fn a_draft_that_cannot_be_removed_stays_where_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    let path = write(&drafts, "a-GS-11111.eml", "letter").unwrap();
    let err = stage_remove_with(&drafts, "a-GS-11111.eml", |_| {
        Err(io::ErrorKind::PermissionDenied.into())
    })
    .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(fs::read_to_string(&path).unwrap(), "letter");
    let left: Vec<_> = fs::read_dir(&drafts)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|n| n.to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(left.is_empty(), "the temporary copy is removed: {left:?}");
}

#[test]
fn staged_removals_report_what_they_could_not_finish() {
    let dir = tempfile::tempdir().unwrap();
    // A folder where the superseded copy should be: removing it fails.
    let backup = dir.path().join("in-the-way");
    fs::create_dir(&backup).unwrap();
    fs::write(backup.join("keep"), "").unwrap();
    let removal = StagedRemoval {
        original: dir.path().join("a-GS-11111.eml"),
        backup: Some(backup.clone()),
        committed: true,
    };
    let err = removal.discard().unwrap_err();
    assert!(
        err.to_string()
            .contains("could not remove the superseded draft"),
        "{err}"
    );
    let batch = Redraft {
        removals: vec![StagedRemoval {
            original: dir.path().join("a-GS-11111.eml"),
            backup: Some(backup.clone()),
            committed: true,
        }],
        locks: BTreeMap::new(),
    };
    batch.discard();
    assert!(backup.join("keep").exists());

    // A restore whose copy has gone warns and leaves the original name free.
    let original = dir.path().join("b-GS-22222.eml");
    drop(StagedRemoval {
        original: original.clone(),
        backup: Some(dir.path().join("gone")),
        committed: false,
    });
    assert!(!original.exists());
}
