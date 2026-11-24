// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! End-to-end tests for `--format eml`, `demo`, `audit`, SHA-256 export manifests and
//! OAIC escalation bundles.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::{PASSPHRASE, emls, ghostsoap, init, text};

fn extract_tag(stdout: &str) -> String {
    let start = stdout.find("GS-").expect("tag in output");
    stdout[start..start + 8].to_owned()
}

fn setup_escalated_vault(home: &Path) -> (String, String) {
    init(home);
    let pass = format!("{PASSPHRASE}\n");
    let drafted = ghostsoap(
        home,
        &["draft", "--entry", "cotality-au", "--passphrase-stdin"],
        &pass,
    );
    assert_eq!(drafted.status.code(), Some(0), "{}", text(&drafted.stderr));
    let tag = extract_tag(&text(&drafted.stdout));
    for args in [
        &["sent", tag.as_str(), "--passphrase-stdin"][..],
        &["sent", tag.as_str(), "--passphrase-stdin"][..],
        &[
            "reply",
            tag.as_str(),
            "refused-with-reasons",
            "--reason",
            "other",
            "--note",
            "Refused without lawful basis",
            "--passphrase-stdin",
        ][..],
        &["reply", tag.as_str(), "escalated", "--passphrase-stdin"][..],
    ] {
        let out = ghostsoap(home, args, &pass);
        assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    }
    (tag, pass)
}

#[test]
fn draft_format_eml_writes_unsent_eml_and_supports_copy_mode() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, eml_dir) = (tmp.path().join("home"), tmp.path().join("eml"));
    init(&home);
    let pass = format!("{PASSPHRASE}\n");

    let out = ghostsoap(
        &home,
        &[
            "draft",
            "--entry",
            "cotality-au",
            "--format",
            "eml",
            "--out",
            eml_dir.to_str().unwrap(),
            "--passphrase-stdin",
        ],
        &pass,
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let files = emls(&eml_dir);
    assert_eq!(files.len(), 1);
    let eml = fs::read_to_string(&files[0]).unwrap();
    assert!(eml.contains("X-Unsent: 1\r\n") && eml.contains("MIME-Version: 1.0\r\n"));

    let copy_eml = ghostsoap(
        &home,
        &[
            "draft",
            "--entry",
            "experian-marketing-au",
            "--format",
            "eml",
            "--copy",
            "--passphrase-stdin",
        ],
        &pass,
    );
    assert_eq!(
        copy_eml.status.code(),
        Some(0),
        "{}",
        text(&copy_eml.stderr)
    );
    let copy_stdout = text(&copy_eml.stdout);
    assert!(copy_stdout.contains("X-Unsent: 1"));
    assert!(copy_stdout.contains("X-GhostSoap-Channel: web form"));
}

#[test]
fn draft_format_text_writes_plain_text_for_email_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, text_dir) = (tmp.path().join("home"), tmp.path().join("txt"));
    init(&home);
    let pass = format!("{PASSPHRASE}\n");

    let txt_out = ghostsoap(
        &home,
        &[
            "draft",
            "--entry",
            "hays-au",
            "--format",
            "text",
            "--out",
            text_dir.to_str().unwrap(),
            "--passphrase-stdin",
        ],
        &pass,
    );
    assert_eq!(txt_out.status.code(), Some(0), "{}", text(&txt_out.stderr));
    let txt_files: Vec<_> = fs::read_dir(&text_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("txt"))
        .collect();
    assert_eq!(txt_files.len(), 1);
    assert!(emls(&text_dir).is_empty());
}

#[test]
fn demo_renders_text_and_html_without_touching_the_vault() {
    let tmp = tempfile::tempdir().unwrap();
    let unused_home = tmp.path().join("untouched-home");

    let text_run = ghostsoap(&unused_home, &["demo"], "");
    assert_eq!(
        text_run.status.code(),
        Some(0),
        "{}",
        text(&text_run.stderr)
    );
    let stdout = text(&text_run.stdout);
    assert!(stdout.contains("example.com fixtures only") && stdout.contains("GS-DM001"));
    assert!(stdout.contains("X-Unsent: 1") && stdout.contains("OAIC Audit Chain"));
    assert!(
        !unused_home.exists(),
        "demo must never create GHOSTSOAP_HOME"
    );

    let html_stdout = ghostsoap(&unused_home, &["demo", "--format", "html"], "");
    assert_eq!(html_stdout.status.code(), Some(0));
    assert!(text(&html_stdout.stdout).starts_with("<!DOCTYPE html>"));

    let html_file = tmp.path().join("preview.html");
    let write_run = ghostsoap(
        &unused_home,
        &["demo", "--html", html_file.to_str().unwrap()],
        "",
    );
    assert_eq!(
        write_run.status.code(),
        Some(0),
        "{}",
        text(&write_run.stderr)
    );
    assert!(
        fs::read_to_string(&html_file)
            .unwrap()
            .contains("GhostSoap Dry-Run Demo")
    );
    assert!(!unused_home.exists());

    let second = ghostsoap(
        &unused_home,
        &["demo", "--html", html_file.to_str().unwrap()],
        "",
    );
    assert_eq!(second.status.code(), Some(1));
    assert!(text(&second.stderr).contains("already exists"));
}

#[test]
fn audit_verifies_encrypted_vault_text_and_json() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let (_tag, pass) = setup_escalated_vault(&home);

    let audit_list = ghostsoap(&home, &["audit", "--list", "--passphrase-stdin"], &pass);
    assert_eq!(
        audit_list.status.code(),
        Some(0),
        "{}",
        text(&audit_list.stderr)
    );
    let list_out = text(&audit_list.stdout);
    assert!(list_out.contains("Audit chain OK: 3 events (1 escalations)"));
    assert!(list_out.contains("escalation"));

    let audit_json = ghostsoap(
        &home,
        &["audit", "--list", "--json", "--passphrase-stdin"],
        &pass,
    );
    assert_eq!(audit_json.status.code(), Some(0));
    let parsed: serde_json::Value = serde_json::from_slice(&audit_json.stdout).unwrap();
    assert_eq!(parsed["verified"], true);
    assert_eq!(parsed["count"], 3);
    assert_eq!(parsed["escalations"], 1);
}

#[test]
fn audit_verifies_plaintext_export_and_companion_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let (_tag, pass) = setup_escalated_vault(&home);

    let export_path = tmp.path().join("backup.toml");
    let manifest_path = tmp.path().join("backup.toml.sha256");
    let exp = ghostsoap(
        &home,
        &[
            "export",
            "--out",
            export_path.to_str().unwrap(),
            "--passphrase-stdin",
        ],
        &pass,
    );
    assert_eq!(exp.status.code(), Some(0), "{}", text(&exp.stderr));
    assert!(manifest_path.is_file());

    let verify_exp = ghostsoap(
        &home,
        &["audit", "--export", export_path.to_str().unwrap(), "--json"],
        "",
    );
    assert_eq!(
        verify_exp.status.code(),
        Some(0),
        "{}",
        text(&verify_exp.stderr)
    );
    let exp_json: serde_json::Value = serde_json::from_slice(&verify_exp.stdout).unwrap();
    assert_eq!(exp_json["manifest"]["files"][0], "backup.toml");

    let verify_both = ghostsoap(
        &home,
        &[
            "audit",
            "--export",
            export_path.to_str().unwrap(),
            "--manifest",
            manifest_path.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(
        verify_both.status.code(),
        Some(0),
        "{}",
        text(&verify_both.stderr)
    );
    assert!(text(&verify_both.stdout).contains("Manifest OK:"));
}

#[test]
fn escalation_bundle_manifest_verifies_and_detects_tampering() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let (tag, pass) = setup_escalated_vault(&home);

    let bundle_dir = tmp.path().join("oaic-pack");
    let bundle = ghostsoap(
        &home,
        &[
            "export",
            "--escalation",
            &tag,
            "--out",
            bundle_dir.to_str().unwrap(),
            "--passphrase-stdin",
        ],
        &pass,
    );
    assert_eq!(bundle.status.code(), Some(0), "{}", text(&bundle.stderr));
    let bundle_manifest = bundle_dir.join("manifest.sha256");
    assert!(bundle_dir.join("summary.txt").is_file() && bundle_dir.join("record.toml").is_file());

    let ok = ghostsoap(
        &home,
        &["audit", "--manifest", bundle_manifest.to_str().unwrap()],
        "",
    );
    assert_eq!(ok.status.code(), Some(0), "{}", text(&ok.stderr));
    assert!(text(&ok.stdout).contains("2 file(s) verified"));

    let ok_json = ghostsoap(
        &home,
        &[
            "audit",
            "--manifest",
            bundle_manifest.to_str().unwrap(),
            "--json",
        ],
        "",
    );
    assert_eq!(ok_json.status.code(), Some(0));

    fs::write(bundle_dir.join("summary.txt"), "altered summary").unwrap();
    let tampered = ghostsoap(
        &home,
        &["audit", "--manifest", bundle_manifest.to_str().unwrap()],
        "",
    );
    assert_eq!(tampered.status.code(), Some(1));
    assert!(text(&tampered.stderr).contains("SHA-256 mismatch"));
}

#[test]
fn shell_completions_include_new_subcommands_and_flags() {
    for shell in ["bash", "elvish", "fish", "powershell", "zsh"] {
        let out = Command::new(env!("CARGO_BIN_EXE_ghostsoap"))
            .args(["completions", shell])
            .output()
            .expect("run ghostsoap completions");
        assert_eq!(out.status.code(), Some(0), "{shell}: {}", text(&out.stderr));
        let script = text(&out.stdout);
        for sub in ["demo", "audit"] {
            assert!(script.contains(sub), "{shell} completion missing {sub:?}");
        }
        let prefix = if shell == "fish" { "-l " } else { "--" };
        for flag in ["format", "escalation", "html", "manifest"] {
            let needle = format!("{prefix}{flag}");
            assert!(
                script.contains(&needle),
                "{shell} completion missing {needle:?}"
            );
        }
    }
}
