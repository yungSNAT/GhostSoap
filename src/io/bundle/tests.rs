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
fn write_new_file_and_basename_validation_enforce_safety() {
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
    assert_eq!(
        write_new_file(&path, b"second", false).unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
    let missing_parent = tmp.path().join("no-such-dir/file.txt");
    assert_eq!(
        write_new_file(&missing_parent, b"x", false)
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );

    assert_eq!(
        validate_export_basename(Path::new("backup.toml")).unwrap(),
        "backup.toml"
    );
    assert_eq!(
        validate_export_basename(Path::new("/")).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    for bad in ["backup:1.toml", "bad\nname.toml"] {
        assert_eq!(
            validate_export_basename(Path::new(bad)).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

fn interleaved_state() -> State {
    let mut state = sample_state();
    audit::append(
        &mut state.audit,
        date(2026, 11, 3),
        "GS-OTHR2",
        "hays-au",
        AuditKind::Sent,
        audit::send_detail(date(2026, 11, 3), ChannelKind::Email, false),
    );
    audit::append(
        &mut state.audit,
        date(2026, 11, 4),
        "GS-TEST1",
        "cotality-au",
        AuditKind::Escalation,
        audit::reply_detail(
            date(2026, 11, 4),
            crate::core::tracker::Status::Escalated,
            &[Lever::App7NoMarketing],
            None,
            None,
            None,
        ),
    );
    state
}

#[test]
fn escalation_bundle_emits_audit_excerpt_for_interleaved_tags() {
    let tmp = tempfile::tempdir().unwrap();
    let state = interleaved_state();
    let bundle_dir = tmp.path().join("escalation");
    let receipt =
        write_escalation_bundle(&bundle_dir, &state, "GS-TEST1", None, date(2026, 11, 24)).unwrap();
    let record_toml = fs::read_to_string(bundle_dir.join("record.toml")).unwrap();
    assert!(record_toml.contains("[[audit_excerpt]]"));
    assert!(record_toml.contains("audit_excerpt_note ="));
    assert!(record_toml.contains("global_audit_count = 3"));
    assert!(!record_toml.contains("[[audit]]"));
    assert_eq!(
        verify_manifest_file(&receipt.manifest).unwrap().files,
        ["record.toml", "summary.txt"]
    );
}

#[test]
fn escalation_bundle_rolls_back_cleanly_and_reports_leftovers() {
    let tmp = tempfile::tempdir().unwrap();
    let state = sample_state();
    let fail_clean = tmp.path().join("fail-clean");
    let mut calls = 0usize;
    let err = write_escalation_bundle_with(
        &fail_clean,
        &state,
        "GS-TEST1",
        None,
        date(2026, 11, 24),
        |p, b| {
            calls += 1;
            if calls == 2 {
                Err(io::Error::other("simulated disk full"))
            } else {
                write_new_file(p, b, true)
            }
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("was deleted"), "{err}");
    assert!(!fail_clean.exists());

    let fail_leftover = tmp.path().join("fail-leftover");
    let mut step = 0usize;
    let err = write_escalation_bundle_with(
        &fail_leftover,
        &state,
        "GS-TEST1",
        None,
        date(2026, 11, 24),
        |p, b| {
            step += 1;
            if step == 1 {
                write_new_file(p, b, true)?;
                fs::remove_file(p)?;
                fs::write(fail_leftover.join("stray.txt"), "x")?;
                Ok(())
            } else {
                Err(io::Error::other("disk full"))
            }
        },
    )
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("could not clean up partial escalation bundle"),
        "{err}"
    );
    assert!(err.to_string().contains("record.toml"), "{err}");
}

#[test]
fn escalation_bundle_errors_and_manifest_tampering_are_detected() {
    let tmp = tempfile::tempdir().unwrap();
    let state = sample_state();
    let bundle_dir = tmp.path().join("escalation");

    assert_eq!(
        write_escalation_bundle(&bundle_dir, &state, "GS-NOPE1", None, date(2026, 11, 24))
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
    let receipt =
        write_escalation_bundle(&bundle_dir, &state, "GS-TEST1", None, date(2026, 11, 24)).unwrap();
    assert_eq!(
        write_escalation_bundle(&bundle_dir, &state, "GS-TEST1", None, date(2026, 11, 24))
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );

    fs::write(bundle_dir.join("summary.txt"), "tampered").unwrap();
    assert!(
        verify_manifest_file(&receipt.manifest)
            .unwrap_err()
            .contains("SHA-256 mismatch")
    );

    fs::remove_file(bundle_dir.join("record.toml")).unwrap();
    assert!(
        verify_manifest_file(&receipt.manifest)
            .unwrap_err()
            .contains("is unreadable")
    );

    fs::create_dir(bundle_dir.join("record.toml")).unwrap();
    assert!(
        verify_manifest_file(&receipt.manifest)
            .unwrap_err()
            .contains("not a regular file")
    );

    let bad_utf8_manifest = tmp.path().join("bad-utf8.sha256");
    fs::write(&bad_utf8_manifest, [0xff, 0xfe]).unwrap();
    assert!(
        verify_manifest_file(&bad_utf8_manifest)
            .unwrap_err()
            .contains("not valid UTF-8")
    );
}

#[test]
fn verify_export_validates_schema_and_redacts_pii() {
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

    let bad_utf8 = tmp.path().join("bad-utf8.toml");
    fs::write(&bad_utf8, [0xff, 0xfe]).unwrap();
    assert!(
        verify_export_file(&bad_utf8)
            .unwrap_err()
            .contains("not valid UTF-8")
    );

    let unrelated = tmp.path().join("unrelated.toml");
    fs::write(&unrelated, "unrelated = \"SENTINEL-PII-99887766\"\n").unwrap();
    let err = verify_export_file(&unrelated).unwrap_err();
    assert!(err.contains("not a valid GhostSoap state export"), "{err}");
    assert!(!err.contains("SENTINEL-PII-99887766"), "PII leaked: {err}");

    let bad_fmt = tmp.path().join("bad-format.toml");
    fs::write(&bad_fmt, "format = 99\n[profile]\nfull_name = \"Alex\"\n").unwrap();
    assert!(
        verify_export_file(&bad_fmt)
            .unwrap_err()
            .contains("unsupported state format 99")
    );
}

#[test]
fn verify_export_binds_companion_manifest_and_detects_tampering() {
    let tmp = tempfile::tempdir().unwrap();
    let state = sample_state();
    let export_path = tmp.path().join("export.toml");
    let text = toml::to_string(&state).unwrap();
    fs::write(&export_path, &text).unwrap();
    assert!(verify_export_file(&export_path).unwrap().manifest.is_none());

    let summary = audit::verify_chain(&state.audit).unwrap();
    let digest = sha256(text.as_bytes());
    let companion = manifest_path(&export_path);
    fs::write(
        &companion,
        audit::format_manifest("2026-11-24", &summary, &[("export.toml", &digest)]),
    )
    .unwrap();
    assert!(verify_export_file(&export_path).unwrap().manifest.is_some());

    fs::write(&export_path, text.replace("mode=initial", "mode=forged")).unwrap();
    assert!(
        verify_export_file(&export_path)
            .unwrap_err()
            .contains("content modified")
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
            .contains("not a regular file")
    );

    let state = sample_state();
    let export_path = tmp.path().join("export.toml");
    let other_path = tmp.path().join("other.toml");
    fs::write(&export_path, toml::to_string(&state).unwrap()).unwrap();
    fs::write(&other_path, "unrelated").unwrap();

    let summary = audit::verify_chain(&state.audit).unwrap();
    let other_sha = sha256(b"unrelated");
    let companion = manifest_path(&export_path);
    fs::write(
        &companion,
        audit::format_manifest("2026-11-24", &summary, &[("other.toml", &other_sha)]),
    )
    .unwrap();
    assert!(
        verify_export_file(&export_path)
            .unwrap_err()
            .contains("does not cover export.toml")
    );
}

#[cfg(unix)]
#[test]
fn unix_symlink_rejections_and_private_bundle_mode() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let state = sample_state();
    let export_path = tmp.path().join("export.toml");
    let text = toml::to_string(&state).unwrap();
    fs::write(&export_path, &text).unwrap();
    let summary = audit::verify_chain(&state.audit).unwrap();
    let real_manifest = tmp.path().join("real.sha256");
    let export_sha = sha256(text.as_bytes());
    fs::write(
        &real_manifest,
        audit::format_manifest("2026-11-24", &summary, &[("export.toml", &export_sha)]),
    )
    .unwrap();
    std::os::unix::fs::symlink(&real_manifest, manifest_path(&export_path)).unwrap();
    assert!(
        verify_export_file(&export_path)
            .unwrap_err()
            .contains("not a regular file")
    );

    let link_export = tmp.path().join("link-export.toml");
    std::os::unix::fs::symlink(&export_path, &link_export).unwrap();
    assert!(
        verify_export_file(&link_export)
            .unwrap_err()
            .contains("not a regular file")
    );

    let bundle_dir = tmp.path().join("private-bundle");
    write_escalation_bundle(&bundle_dir, &state, "GS-TEST1", None, date(2026, 11, 24)).unwrap();
    assert_eq!(
        fs::metadata(&bundle_dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[test]
fn own_escalation_bundles_and_clean_preserve_unrelated_files() {
    let tmp = tempfile::tempdir().unwrap();
    let missing_dir = tmp.path().join("missing");
    assert!(own_escalation_bundles(&missing_dir).unwrap().is_empty());

    let state = sample_state();
    let valid = tmp.path().join("ghostsoap-escalation-GS-TEST1-2026-11-24");
    write_escalation_bundle(&valid, &state, "GS-TEST1", None, date(2026, 11, 24)).unwrap();

    // Non-matching names or files are ignored.
    let bad_tag = tmp.path().join("ghostsoap-escalation-NOPE-2026-11-24");
    fs::create_dir(&bad_tag).unwrap();
    let bad_prefix = tmp.path().join("ghostsoap-escalation-no-date");
    fs::create_dir(&bad_prefix).unwrap();
    let fake_file = tmp.path().join("ghostsoap-escalation-GS-TEST1-2026-11-25");
    fs::write(&fake_file, "not a dir").unwrap();

    assert_eq!(
        own_escalation_bundles(tmp.path()).unwrap(),
        vec![valid.clone()]
    );
    assert!(
        clean_escalation_bundle(&fake_file)
            .unwrap_err()
            .to_string()
            .contains("no longer a folder")
    );

    // Add unrelated file: known 3 bundle files are deleted, unrelated file and folder stay.
    fs::write(valid.join("my-notes.txt"), "keep me").unwrap();
    let partial = clean_escalation_bundle(&valid).unwrap();
    assert_eq!(partial.deleted, 3);
    assert_eq!(partial.kept, vec!["my-notes.txt"]);
    assert!(valid.join("my-notes.txt").is_file());

    // Remove unrelated file and clean again: empty bundle dir is removed.
    fs::remove_file(valid.join("my-notes.txt")).unwrap();
    let empty = clean_escalation_bundle(&valid).unwrap();
    assert_eq!(empty.deleted, 0);
    assert!(empty.kept.is_empty());
    assert!(!valid.exists());
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
