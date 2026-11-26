// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Tamper-evident SHA-256 hash-chained OAIC audit log and export manifest helpers. No I/O.

use std::fmt;

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::core::deadline::{self, Due};
use crate::core::law;
use crate::core::named;
use crate::core::register::{ChannelKind, Entry, Escalation, Lever};
use crate::core::sha256;
use crate::core::tracker::{Message, Reason, Status, Withdrawn};

/// Genesis digest for the first entry in an audit chain (64 zero hex digits).
pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// How a GhostSoap SHA-256 manifest begins, before its date.
pub const MANIFEST_HEADER: &str = "# GhostSoap SHA-256 manifest of ";

const AUDIT_DOMAIN: &str = "ghostsoap-audit-v1";

/// The kind of tracker event recorded in the hash chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuditKind {
    Sent,
    SentWithdrawn,
    Reply,
    Escalation,
    Extension,
    Withdrawal,
}

/// One append-only entry in the tamper-evident OAIC audit chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditEntry {
    pub seq: u64,
    pub recorded_on: Date,
    pub tag: String,
    pub entry: String,
    pub kind: AuditKind,
    pub detail: String,
    pub prev_hash: String,
    pub hash: String,
}

/// Result of verifying a complete audit chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditSummary {
    pub count: usize,
    pub escalations: usize,
    pub head_hash: String,
}

/// One file entry inside a GhostSoap SHA-256 manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub sha256: String,
    pub name: String,
}

/// Whether `text` is 64 lowercase hexadecimal digits.
pub fn is_hex64(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Compute the deterministic, domain-separated SHA-256 digest of an audit entry.
pub fn compute_hash(
    seq: u64,
    recorded_on: Date,
    tag: &str,
    entry: &str,
    kind: AuditKind,
    detail: &str,
    prev_hash: &str,
) -> String {
    let kind_name = named(kind);
    let payload = format!(
        "{AUDIT_DOMAIN}\0{seq}\0{recorded_on}\0{prev_hash}\0{kind_name}\0\
         {}:{tag}\0{}:{entry}\0{}:{detail}",
        tag.len(),
        entry.len(),
        detail.len()
    );
    sha256(payload.as_bytes())
}

/// Append a new hash-chained entry to `chain`.
pub fn append(
    chain: &mut Vec<AuditEntry>,
    recorded_on: Date,
    tag: &str,
    entry: &str,
    kind: AuditKind,
    detail: String,
) {
    let seq = (chain.len() as u64) + 1;
    let prev_hash = chain
        .last()
        .map_or_else(|| GENESIS_HASH.to_owned(), |last| last.hash.clone());
    let hash = compute_hash(seq, recorded_on, tag, entry, kind, &detail, &prev_hash);
    chain.push(AuditEntry {
        seq,
        recorded_on,
        tag: tag.to_owned(),
        entry: entry.to_owned(),
        kind,
        detail,
        prev_hash,
        hash,
    });
}

/// Verify every sequence number, back-link and SHA-256 digest in `entries`.
pub fn verify_chain(entries: &[AuditEntry]) -> Result<AuditSummary, String> {
    let mut expected_prev = GENESIS_HASH;
    let mut escalations = 0usize;
    for (index, item) in entries.iter().enumerate() {
        let expected_seq = (index as u64) + 1;
        if item.seq != expected_seq {
            return Err(format!(
                "entry {} has seq {}, expected {expected_seq}",
                index + 1,
                item.seq
            ));
        }
        if !is_hex64(&item.prev_hash) || !is_hex64(&item.hash) {
            return Err(format!("entry {} has a malformed SHA-256 digest", item.seq));
        }
        if item.prev_hash != expected_prev {
            return Err(format!(
                "entry {} breaks the hash chain (prev_hash does not match entry {})",
                item.seq, index
            ));
        }
        let expected_hash = compute_hash(
            item.seq,
            item.recorded_on,
            &item.tag,
            &item.entry,
            item.kind,
            &item.detail,
            &item.prev_hash,
        );
        if item.hash != expected_hash {
            return Err(format!(
                "entry {} failed SHA-256 verification (content modified)",
                item.seq
            ));
        }
        if item.kind == AuditKind::Escalation {
            escalations += 1;
        }
        expected_prev = &item.hash;
    }
    Ok(AuditSummary {
        count: entries.len(),
        escalations,
        head_hash: expected_prev.to_owned(),
    })
}

/// Commit a free-text note or source by its SHA-256 digest so tampering is detected without
/// copying personal details into the audit detail string.
fn optional_digest(label: &str, value: Option<&str>) -> Option<String> {
    value.map(|text| format!("{label}_sha256={}", sha256(text.as_bytes())))
}

/// Canonical non-PII detail for a recorded send.
pub fn send_detail(on: Date, channel: ChannelKind, corrected: bool) -> String {
    let mode = if corrected { "corrected" } else { "initial" };
    format!("on={on};channel={};mode={mode}", named(channel))
}

/// Canonical non-PII detail for a withdrawn send.
pub fn send_withdrawn_detail(note: Option<&str>) -> String {
    match optional_digest("note", note) {
        Some(digest) => format!("withdrawn=sent;{digest}"),
        None => "withdrawn=sent".to_owned(),
    }
}

/// Canonical non-PII detail for a recorded reply or escalation event.
pub fn reply_detail(
    on: Date,
    status: Status,
    levers: &[Lever],
    reason: Option<Reason>,
    source: Option<&str>,
    note: Option<&str>,
) -> String {
    let lever_names: Vec<&str> = levers.iter().map(|l| l.name()).collect();
    let mut parts = vec![
        format!("on={on}"),
        format!("status={}", named(status)),
        format!("levers={}", lever_names.join(",")),
    ];
    if let Some(reason) = reason {
        parts.push(format!("reason={}", named(reason)));
    }
    if let Some(digest) = optional_digest("source", source) {
        parts.push(digest);
    }
    if let Some(digest) = optional_digest("note", note) {
        parts.push(digest);
    }
    parts.join(";")
}

/// Canonical non-PII detail for an agreed deadline extension.
pub fn extension_detail(on: Date, until: Date, levers: &[Lever], note: Option<&str>) -> String {
    let lever_names: Vec<&str> = levers.iter().map(|l| l.name()).collect();
    let mut parts = vec![
        format!("on={on}"),
        format!("until={until}"),
        format!("levers={}", lever_names.join(",")),
    ];
    if let Some(digest) = optional_digest("note", note) {
        parts.push(digest);
    }
    parts.join(";")
}

/// Canonical non-PII detail for a withdrawn status or extension event.
pub fn withdrawal_detail(of: Withdrawn, levers: &[Lever], note: Option<&str>) -> String {
    let lever_names: Vec<&str> = levers.iter().map(|l| l.name()).collect();
    let mut parts = vec![
        format!("withdrawn={}", named(of)),
        format!("levers={}", lever_names.join(",")),
    ];
    if let Some(digest) = optional_digest("note", note) {
        parts.push(digest);
    }
    parts.join(";")
}

/// Format a GhostSoap SHA-256 manifest file covering `files` (`(filename, sha256)` pairs).
pub fn format_manifest(date: &str, summary: &AuditSummary, files: &[(&str, &str)]) -> String {
    let mut out = format!(
        "{MANIFEST_HEADER}{date} (audit-events={}, escalations={}, chain-head={})\n",
        summary.count, summary.escalations, summary.head_hash
    );
    for &(name, digest) in files {
        out.push_str(&format!("{digest}  {name}\n"));
    }
    out
}

/// Whether `name` is a single safe filename inside the manifest's own folder.
pub fn is_safe_filename(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name
            .chars()
            .any(|c| matches!(c, '/' | '\\' | ':') || c.is_control())
}

/// Parse a GhostSoap SHA-256 manifest, rejecting malformed lines and path traversal.
pub fn parse_manifest(text: &str) -> Result<Vec<ManifestEntry>, String> {
    let mut entries = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((digest, name)) = line.split_once("  ") else {
            return Err(format!(
                "manifest line {} is not `<sha256>  <filename>`",
                index + 1
            ));
        };
        if !is_hex64(digest) {
            return Err(format!(
                "manifest line {} has an invalid SHA-256 digest",
                index + 1
            ));
        }
        if !is_safe_filename(name) {
            return Err(format!(
                "manifest line {} names an unsafe path `{name}`",
                index + 1
            ));
        }
        entries.push(ManifestEntry {
            sha256: digest.to_owned(),
            name: name.to_owned(),
        });
    }
    if entries.is_empty() {
        return Err("manifest lists no files".to_owned());
    }
    Ok(entries)
}

fn escalation_label(target: Escalation) -> &'static str {
    match target {
        Escalation::Oaic => "OAIC (Office of the Australian Information Commissioner)",
        Escalation::Afca => "AFCA (Australian Financial Complaints Authority)",
        Escalation::Tio => "TIO (Telecommunications Industry Ombudsman)",
        Escalation::Acma => "ACMA (Australian Communications and Media Authority)",
    }
}

fn format_due(due: Due) -> String {
    match due {
        Due::Statutory(day) => format!("statutory deadline {day}"),
        Due::Guide(day) => format!("OAIC guide {day}"),
        Due::Unknown => "5 business days after sending".to_owned(),
        Due::None => "none".to_owned(),
    }
}

fn is_s40_1b(lever: Lever) -> bool {
    matches!(
        lever,
        Lever::S20rAccess | Lever::S20tCorrection | Lever::S21tAccess | Lever::S21vCorrection
    )
}

fn requires_s40_1a(lever: Lever) -> bool {
    !is_s40_1b(lever) && lever != Lever::SpamUnsubscribe
}

/// The comma-joined names of the requests s 40(1A) covers that have (`escalated` true) or
/// lack an `escalated` status in effect. Each request is judged alone: one request's
/// escalation says nothing about a complaint on another.
fn s40_1a_levers(message: &Message, escalated: bool) -> String {
    message
        .requests
        .iter()
        .filter(|r| {
            requires_s40_1a(r.lever)
                && r.in_effect()
                    .statuses
                    .iter()
                    .any(|s| s.status == Status::Escalated)
                    == escalated
        })
        .map(|r| r.lever.name())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The OAIC's complain-first guidance, quoted word for word with its link and dates.
pub(crate) const OAIC_COMPLAIN_FIRST: &str = "the OAIC's page 'Before you lodge a privacy \
     complaint with us' \
     (https://www.oaic.gov.au/privacy/privacy-complaints/before-you-lodge-a-privacy-complaint-with-us, \
     updated 2 March 2026, read 27 November 2026) says: 'If they don't respond to your complaint \
     within 30 days or you're not happy with their response, you can lodge a complaint with us.'";

/// Eligibility notes under Privacy Act ss 40(1A)-(1B) and the Spam Act for `message`, one
/// for each that applies to at least one of its requests, with s 40(1A) split by whether
/// each request has an escalation recorded.
pub fn escalation_eligibility(message: &Message) -> Vec<String> {
    let any = |test: fn(Lever) -> bool| message.requests.iter().any(|r| test(r.lever));
    let mut notes = Vec::new();
    let escalated = s40_1a_levers(message, true);
    if !escalated.is_empty() {
        notes.push(format!(
            "Privacy Act s 40(1A): applies to {escalated} (escalation recorded; confirm you \
             complained to the business first, as otherwise the OAIC may investigate only if it \
             considers complaining to the business was not appropriate; {OAIC_COMPLAIN_FIRST})"
        ));
    }
    let unescalated = s40_1a_levers(message, false);
    if !unescalated.is_empty() {
        notes.push(format!(
            "Privacy Act s 40(1A): applies to {unescalated} (no escalation recorded; complain to \
             the business first unless you already have, as otherwise the OAIC may investigate only if \
             it considers complaining to the business was not appropriate)"
        ));
    }
    if any(is_s40_1b) {
        notes.push(
            "Privacy Act s 40(1B): complain-first precondition does not apply to ss 20R, 20T, 21T \
             or 21V requests"
                .to_owned(),
        );
    }
    if any(|lever| lever == Lever::SpamUnsubscribe) {
        notes.push(
            "Spam Act 2003: the ACMA enforces it and takes spam reports (acma.gov.au/spam), not \
             the OAIC"
                .to_owned(),
        );
    }
    notes
}

/// State and Privacy Act s 40(1A) warnings to surface when exporting an escalation bundle.
pub fn escalation_notices(message: &Message) -> Vec<String> {
    let mut notices = Vec::new();
    if message.unfinished {
        notices.push(
            "draft run did not finish: the letter you hold under this tag may not match the \
             recorded requests; draft it again"
                .to_owned(),
        );
    }
    if message.superseded {
        notices.push(
            "draft was superseded by a later draft under a new tag and remains as history only"
                .to_owned(),
        );
    }
    if message.sent().is_none() {
        notices.push(format!(
            "letter is not recorded as sent; record sending with `ghostsoap sent {}` before \
             escalating",
            message.tag
        ));
    }
    let unescalated = s40_1a_levers(message, false);
    if !unescalated.is_empty() {
        notices.push(format!(
            "Privacy Act s 40(1A): no escalation is recorded for {unescalated}; unless you have \
             already complained to the business, do so first: the OAIC must not investigate \
             unless you complained to the business first or it decides complaining to the \
             business was not appropriate; {OAIC_COMPLAIN_FIRST}"
        ));
    }
    notices
}

fn append_escalation_header(out: &mut String, message: &Message, entry: Option<&Entry>) {
    out.push_str(&format!("Tag: {}\nEntry: {}\n", message.tag, message.entry));
    if let Some(meta) = entry {
        out.push_str(&format!("Legal entity: {}\n", meta.legal_entity));
        let targets: Vec<&str> = meta
            .escalation
            .iter()
            .copied()
            .map(escalation_label)
            .collect();
        if !targets.is_empty() {
            out.push_str(&format!("Escalation bodies: {}\n", targets.join("; ")));
        }
    }
    out.push_str(&format!(
        "Drafted: {} ({} to {})\n",
        message.drafted,
        message.channel.label(),
        message.address
    ));
    match message.sent() {
        Some(sent) => out.push_str(&format!("Sent: {} by {}\n", sent.on, sent.channel.label())),
        None => out.push_str("Sent: not recorded\n"),
    }
    for note in escalation_eligibility(message) {
        out.push_str(&format!("Eligibility: {note}\n"));
    }
    let notices = escalation_notices(message);
    if !notices.is_empty() {
        out.push_str("\nNotices:\n");
        for notice in notices {
            out.push_str(&format!("- {notice}\n"));
        }
    }
}

/// Format a human-readable OAIC escalation summary for one tracked message.
pub fn format_escalation_summary(
    message: &Message,
    entry: Option<&Entry>,
    audit: &[AuditEntry],
    summary: &AuditSummary,
    today: Date,
) -> String {
    let mut out = format!("GhostSoap OAIC escalation summary ({today})\n");
    append_escalation_header(&mut out, message, entry);
    out.push_str("\nRequests:\n");
    let is_sent = message.sent().is_some();
    for request in &message.requests {
        let due = deadline::due(message, request);
        let row = law::law(request.lever);
        out.push_str(&format!(
            "- {} ({}): status={}, due={}\n",
            request.lever.name(),
            row.provision,
            named(request.status(is_sent)),
            format_due(due)
        ));
    }
    out.push_str(&format!("\nAudit chain: {summary}\n"));
    let tagged: Vec<&AuditEntry> = audit.iter().filter(|a| a.tag == message.tag).collect();
    if tagged.is_empty() {
        out.push_str("Tag audit entries: none recorded\n");
    } else {
        out.push_str("Tag audit entries:\n");
        for item in tagged {
            out.push_str(&format!(
                "  #{}  {}  {}  {}  {}\n",
                item.seq,
                item.recorded_on,
                named(item.kind),
                item.detail,
                &item.hash[..12]
            ));
        }
    }
    out
}

impl fmt::Display for AuditSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} events ({} escalations), head {}",
            self.count, self.escalations, self.head_hash
        )
    }
}

#[cfg(test)]
mod tests;
