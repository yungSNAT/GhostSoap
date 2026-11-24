// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

use jiff::civil::date;

use super::*;
use crate::core::audit::AuditKind;
use crate::core::profile::Profile;
use crate::core::register::{ChannelKind, Lever};
use crate::core::tracker::{Request, Send, SendRecord};

fn sample_state() -> State {
    let mut state = State::new(Profile {
        full_name: "Alex Example".to_owned(),
        email: Some("alex@example.com".to_owned()),
        ..Profile::default()
    });
    state.messages.push(Message {
        tag: "GS-TEST1".to_owned(),
        entry: "cotality-au".to_owned(),
        channel: ChannelKind::Email,
        address: "privacy@example.com".to_owned(),
        drafted: date(2026, 11, 1),
        draft: None,
        superseded: false,
        unfinished: false,
        sends: vec![SendRecord::Sent(Send {
            on: date(2026, 11, 2),
            channel: ChannelKind::Email,
        })],
        requests: vec![Request::new(Lever::App7NoMarketing)],
    });
    audit::append(
        &mut state.audit,
        date(2026, 11, 2),
        "GS-TEST1",
        "cotality-au",
        AuditKind::Sent,
        audit::send_detail(date(2026, 11, 2), ChannelKind::Email, false),
    );
    state
}

#[test]
fn write_new_file_creates_private_file_and_refuses_overwrite() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("out.txt");
    write_new_file(&path, b"hello", true).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"hello");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let err = write_new_file(&path, b"second", false).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert!(err.to_string().contains("already exists"));

    let missing_parent = tmp.path().join("no-such-dir/file.txt");
    assert_eq!(
        write_new_file(&missing_parent, b"x", false)
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
}

#[test]
fn escalation_bundle_writes_and_verifies_and_detects_tampering() {
    let tmp = tempfile::tempdir().unwrap();
    let state = sample_state();
    let bundle_dir = tmp.path().join("escalation");

    let missing =
        write_escalation_bundle(&bundle_dir, &state, "GS-NOPE1", None, date(2026, 11, 24))
            .unwrap_err();
    assert_eq!(missing.kind(), io::ErrorKind::NotFound);
    assert!(!bundle_dir.exists());

    let receipt =
        write_escalation_bundle(&bundle_dir, &state, "GS-TEST1", None, date(2026, 11, 24)).unwrap();
    assert_eq!(receipt.files, ["record.toml", "summary.txt"]);
    assert_eq!(receipt.audit.count, 1);

    let dup = write_escalation_bundle(&bundle_dir, &state, "GS-TEST1", None, date(2026, 11, 24))
        .unwrap_err();
    assert_eq!(dup.kind(), io::ErrorKind::AlreadyExists);

    let verified = verify_manifest_file(&receipt.manifest).unwrap();
    assert_eq!(verified.files, ["record.toml", "summary.txt"]);

    // Tamper with summary.txt -> manifest verification must fail.
    fs::write(bundle_dir.join("summary.txt"), "tampered").unwrap();
    let err = verify_manifest_file(&receipt.manifest).unwrap_err();
    assert!(err.contains("SHA-256 mismatch for summary.txt"), "{err}");

    // Delete record.toml -> missing file error.
    fs::remove_file(bundle_dir.join("record.toml")).unwrap();
    let err = verify_manifest_file(&receipt.manifest).unwrap_err();
    assert!(err.contains("is unreadable"), "{err}");

    // Replace record.toml with a directory (not a regular file).
    fs::create_dir(bundle_dir.join("record.toml")).unwrap();
    let err = verify_manifest_file(&receipt.manifest).unwrap_err();
    assert!(err.contains("is not a regular file"), "{err}");
}

#[test]
fn verify_export_and_manifest_handle_missing_invalid_and_companion() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("missing.toml");
    assert!(
        verify_export_file(&missing)
            .unwrap_err()
            .contains("cannot read export")
    );
    assert!(
        verify_manifest_file(&missing)
            .unwrap_err()
            .contains("cannot read manifest")
    );

    let bad_toml = tmp.path().join("bad.toml");
    fs::write(&bad_toml, "not = [toml").unwrap();
    assert!(
        verify_export_file(&bad_toml)
            .unwrap_err()
            .contains("not valid TOML")
    );

    let state = sample_state();
    let export_path = tmp.path().join("export.toml");
    let text = toml::to_string(&state).unwrap();
    fs::write(&export_path, &text).unwrap();
    let without_manifest = verify_export_file(&export_path).unwrap();
    assert_eq!(without_manifest.summary.count, 1);
    assert!(without_manifest.manifest.is_none());

    let digest = sha256(text.as_bytes());
    let manifest = manifest_path(&export_path);
    let mtext = audit::format_manifest(
        "2026-11-24",
        &without_manifest.summary,
        &[("export.toml", &digest)],
    );
    fs::write(&manifest, mtext).unwrap();
    let with_manifest = verify_export_file(&export_path).unwrap();
    assert!(with_manifest.manifest.is_some());

    // Tamper with an audit entry in export.toml -> chain check catches it.
    let tampered = text.replace("mode=initial", "mode=forged");
    fs::write(&export_path, tampered).unwrap();
    let err = verify_export_file(&export_path).unwrap_err();
    assert!(err.contains("content modified"), "{err}");
}

#[test]
fn write_failure_removes_partial_file_and_missing_parent_bundle_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let partial = tmp.path().join("partial.txt");
    let err =
        write_new_file_with(&partial, true, |_| Err(io::Error::other("disk full"))).unwrap_err();
    assert!(err.to_string().contains("disk full"));
    assert!(!partial.exists());

    let mut state = sample_state();
    let missing_parent = tmp.path().join("no-such-parent/bundle");
    let err = write_escalation_bundle(
        &missing_parent,
        &state,
        "GS-TEST1",
        None,
        date(2026, 11, 24),
    )
    .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);

    state.audit[0].detail.push_str(";tampered=1");
    assert!(
        state
            .problems()
            .iter()
            .any(|problem| problem.contains("audit chain is invalid"))
    );
}

#[test]
fn manifest_checks_reject_non_regular_manifests_and_unbound_companions() {
    let tmp = tempfile::tempdir().unwrap();
    let dir_manifest = tmp.path().join("dir.sha256");
    fs::create_dir(&dir_manifest).unwrap();
    assert!(
        verify_manifest_file(&dir_manifest)
            .unwrap_err()
            .contains("is not a regular file")
    );

    let state = sample_state();
    let export_path = tmp.path().join("export.toml");
    let other_path = tmp.path().join("other.toml");
    fs::write(&export_path, toml::to_string(&state).unwrap()).unwrap();
    fs::write(&other_path, "unrelated").unwrap();

    // Companion manifest that hashes another file instead of export.toml must be rejected.
    let summary = audit::verify_chain(&state.audit).unwrap();
    let other_sha = sha256(b"unrelated");
    let companion = manifest_path(&export_path);
    let substituted = audit::format_manifest("2026-11-24", &summary, &[("other.toml", &other_sha)]);
    fs::write(&companion, substituted).unwrap();
    let err = verify_export_file(&export_path).unwrap_err();
    assert!(err.contains("does not cover export.toml"), "{err}");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Symlinked companion manifest must be rejected rather than followed.
        let real_manifest = tmp.path().join("real.sha256");
        let export_sha = sha256(&fs::read(&export_path).unwrap());
        let valid = audit::format_manifest("2026-11-24", &summary, &[("export.toml", &export_sha)]);
        fs::write(&real_manifest, valid).unwrap();
        fs::remove_file(&companion).unwrap();
        std::os::unix::fs::symlink(&real_manifest, &companion).unwrap();
        let err = verify_export_file(&export_path).unwrap_err();
        assert!(err.contains("is not a regular file"), "{err}");

        // Atomic 0o700 mode on escalation bundle directory.
        let bundle_dir = tmp.path().join("private-bundle");
        write_escalation_bundle(&bundle_dir, &state, "GS-TEST1", None, date(2026, 11, 24)).unwrap();
        assert_eq!(
            fs::metadata(&bundle_dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}
