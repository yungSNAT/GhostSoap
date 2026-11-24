// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

use jiff::civil::date;

use super::*;
use crate::core::register::Entry;
use crate::core::tracker::{Request, Send, SendRecord};

fn two_entry_chain() -> Vec<AuditEntry> {
    let mut chain = Vec::new();
    append(
        &mut chain,
        date(2026, 11, 20),
        "GS-TEST1",
        "broker-au",
        AuditKind::Sent,
        send_detail(date(2026, 11, 20), ChannelKind::Email, false),
    );
    append(
        &mut chain,
        date(2026, 11, 24),
        "GS-TEST1",
        "broker-au",
        AuditKind::Escalation,
        reply_detail(
            date(2026, 11, 24),
            Status::Escalated,
            &[Lever::App7NoMarketing],
            Some(Reason::Other),
            Some("Upstream Co"),
            Some("Filed OAIC complaint"),
        ),
    );
    chain
}

#[test]
fn hash_chain_appends_and_verifies_valid_events() {
    let empty = verify_chain(&[]).expect("empty chain is valid");
    assert_eq!(empty.count, 0);
    assert_eq!(empty.escalations, 0);
    assert_eq!(empty.head_hash, GENESIS_HASH);

    let chain = two_entry_chain();
    let ok = verify_chain(&chain).expect("valid chain");
    assert_eq!(ok.count, 2);
    assert_eq!(ok.escalations, 1);
    assert_eq!(ok.head_hash, chain[1].hash);
    assert!(ok.to_string().contains("2 events (1 escalations)"));
}

#[test]
fn hash_chain_detects_every_tamper_mode() {
    let chain = two_entry_chain();
    let mut bad_detail = chain.clone();
    bad_detail[0].detail.push_str(";extra=1");
    assert!(
        verify_chain(&bad_detail)
            .unwrap_err()
            .contains("content modified")
    );

    let mut bad_link = chain.clone();
    bad_link[1].prev_hash = GENESIS_HASH.to_owned();
    assert!(
        verify_chain(&bad_link)
            .unwrap_err()
            .contains("breaks the hash chain")
    );

    let mut bad_seq = chain.clone();
    bad_seq[1].seq = 3;
    assert!(verify_chain(&bad_seq).unwrap_err().contains("expected 2"));

    let mut bad_hex = chain;
    bad_hex[0].hash = "NOT-HEX".to_owned();
    assert!(
        verify_chain(&bad_hex)
            .unwrap_err()
            .contains("malformed SHA-256")
    );
}

#[test]
fn detail_formatters_commit_notes_by_digest_without_raw_text() {
    assert_eq!(
        send_detail(date(2026, 11, 20), ChannelKind::Post, true),
        "on=2026-11-20;channel=post;mode=corrected"
    );
    assert_eq!(send_withdrawn_detail(None), "withdrawn=sent");
    let withdrawn = send_withdrawn_detail(Some("secret personal reason"));
    assert!(withdrawn.starts_with("withdrawn=sent;note_sha256="));
    assert!(!withdrawn.contains("secret personal reason"));

    let ext = extension_detail(
        date(2026, 11, 20),
        date(2026, 12, 31),
        &[Lever::S20tCorrection],
        Some("private note"),
    );
    assert!(ext.contains("until=2026-12-31") && ext.contains("note_sha256="));
    assert!(!ext.contains("private note"));
    assert!(
        !extension_detail(
            date(2026, 11, 20),
            date(2026, 12, 31),
            &[Lever::S20tCorrection],
            None
        )
        .contains("note_sha256")
    );

    let wd = withdrawal_detail(Withdrawn::Status, &[Lever::App7Source], Some("wrong date"));
    assert!(wd.contains("withdrawn=status") && wd.contains("note_sha256="));
    assert!(
        !withdrawal_detail(Withdrawn::Extension, &[Lever::S20tCorrection], None)
            .contains("note_sha256")
    );
}

#[test]
fn manifest_round_trips_and_rejects_bad_lines_and_traversal() {
    let summary = AuditSummary {
        count: 1,
        escalations: 0,
        head_hash: sha256(b"head"),
    };
    let digest = sha256(b"payload");
    let text = format_manifest("2026-11-24", &summary, &[("export.toml", &digest)]);
    assert!(text.starts_with(MANIFEST_HEADER));
    let parsed = parse_manifest(&format!("\n{text}\r\n")).expect("valid manifest");
    assert_eq!(
        parsed,
        vec![ManifestEntry {
            sha256: digest.clone(),
            name: "export.toml".to_owned(),
        }]
    );

    assert!(
        parse_manifest("# only comments\n")
            .unwrap_err()
            .contains("lists no files")
    );
    assert!(parse_manifest("badline\n").unwrap_err().contains("is not"));
    assert!(
        parse_manifest("zzzz  export.toml\n")
            .unwrap_err()
            .contains("invalid SHA-256")
    );
    for unsafe_name in [
        "",
        ".",
        "..",
        "../evil",
        "dir/file",
        "dir\\file",
        "C:file",
        "a\0b",
    ] {
        let bad = format!("{digest}  {unsafe_name}\n");
        assert!(parse_manifest(&bad).unwrap_err().contains("unsafe path"));
    }
}

fn demo_entry() -> Entry {
    toml::from_str(
        r#"
id = "demo-broker-au"
name = "Demo Broker"
legal_entity = "Demo Broker Pty Ltd"
kinds = ["marketing-list"]
priority = "high"
quality = "verified"
verified_at = 2026-11-24
sources = ["https://example.com/privacy"]
levers = ["app7-no-marketing", "s20r-access", "spam-unsubscribe"]
identity_fields = ["full_name", "email"]
exemption_risk = "none"
escalation = ["oaic", "afca", "tio", "acma"]
effort = "easy"
[[channels]]
kind = "email"
address = "privacy@example.com"
levers = ["app7-no-marketing", "s20r-access", "spam-unsubscribe"]
"#,
    )
    .expect("entry")
}

fn sample_message() -> Message {
    Message {
        tag: "GS-TEST1".to_owned(),
        entry: "demo-broker-au".to_owned(),
        channel: ChannelKind::Email,
        address: "privacy@example.com".to_owned(),
        drafted: date(2026, 10, 1),
        draft: None,
        superseded: false,
        unfinished: false,
        sends: Vec::new(),
        requests: vec![
            Request::new(Lever::App7NoMarketing),
            Request::new(Lever::S20rAccess),
            Request::new(Lever::SpamUnsubscribe),
        ],
    }
}

#[test]
fn escalation_summary_covers_unsent_and_sent_states() {
    let mut message = sample_message();
    let empty_summary = verify_chain(&[]).unwrap();
    let unsent = format_escalation_summary(&message, None, &[], &empty_summary, date(2026, 11, 24));
    assert!(unsent.contains("Sent: not recorded"));
    assert!(unsent.contains("due=none"));
    assert!(unsent.contains("Tag audit entries: none recorded"));

    message.sends.push(SendRecord::Sent(Send {
        on: date(2026, 10, 10),
        channel: ChannelKind::Email,
    }));
    let mut chain = Vec::new();
    append(
        &mut chain,
        date(2026, 10, 10),
        "GS-TEST1",
        "demo-broker-au",
        AuditKind::Sent,
        send_detail(date(2026, 10, 10), ChannelKind::Email, false),
    );
    let summary = verify_chain(&chain).unwrap();
    let meta = demo_entry();
    let sent =
        format_escalation_summary(&message, Some(&meta), &chain, &summary, date(2026, 11, 24));
    for needle in [
        "Demo Broker Pty Ltd",
        "OAIC",
        "AFCA",
        "TIO",
        "ACMA",
        "OAIC guide",
        "statutory deadline",
        "5 business days after sending",
        "#1  2026-10-10  sent",
    ] {
        assert!(sent.contains(needle), "{needle} missing from {sent}");
    }
}
