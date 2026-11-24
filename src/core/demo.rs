// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Synthetic, zero-PII dry-run fixtures and previews for `ghostsoap demo`. No I/O.

use jiff::civil::{Date, date};

use crate::core::audit::{self, AuditEntry, AuditKind, AuditSummary};
use crate::core::checklist::{self, Item, Mark, MarkKind};
use crate::core::deadline::{self, Due};
use crate::core::embedded;
use crate::core::letters::{self, Letter};
use crate::core::profile::Profile;
use crate::core::register::{ChannelKind, Lever};
use crate::core::tracker::{
    Action, Event, Message, Reason, Request, Send, SendRecord, State, Status, StatusEvent,
};

mod render;
pub use render::{render_html, render_text};

/// Fixed date for deterministic, reproducible demo previews.
pub const DEMO_DATE: Date = date(2026, 11, 24);

/// One status row in the synthetic demo snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoRow {
    pub action: Action,
    pub tag: String,
    pub entry: String,
    pub lever: Lever,
    pub status: Status,
    pub due: Due,
}

/// Complete in-memory demo state rendered as terminal text or self-contained HTML.
#[derive(Debug, Clone)]
pub struct DemoSnapshot {
    pub date: Date,
    pub state: State,
    pub rows: Vec<DemoRow>,
    pub audit_summary: AuditSummary,
    pub checklist_items: Vec<Item>,
    pub sample_eml: String,
}

fn demo_profile() -> Profile {
    Profile {
        full_name: "Alex Example".to_owned(),
        alias_names: Vec::new(),
        email: Some("alex@example.com".to_owned()),
        phone: Some("0400 000 000".to_owned()),
        date_of_birth: None,
        address: Some("1 Example Street, Sydney NSW 2000".to_owned()),
        previous_addresses: Vec::new(),
        postcode: Some("2000".to_owned()),
    }
}

fn demo_message(
    tag: &str,
    entry: &str,
    address: &str,
    drafted: Date,
    sent_on: Option<Date>,
    levers: &[Lever],
) -> Message {
    let sends = sent_on
        .map(|on| {
            vec![SendRecord::Sent(Send {
                on,
                channel: ChannelKind::Email,
            })]
        })
        .unwrap_or_default();
    Message {
        tag: tag.to_owned(),
        entry: entry.to_owned(),
        channel: ChannelKind::Email,
        address: address.to_owned(),
        drafted,
        draft: None,
        superseded: false,
        unfinished: false,
        sends,
        requests: levers.iter().copied().map(Request::new).collect(),
    }
}

fn status_event(on: Date, status: Status, reason: Option<Reason>) -> Event {
    Event::Status(StatusEvent {
        on,
        status,
        reason,
        source: None,
        note: None,
    })
}

fn demo_escalated_message() -> Message {
    let mut msg = demo_message(
        "GS-DM002",
        "list-beta-example",
        "privacy@list-beta.example.com",
        date(2026, 10, 18),
        Some(date(2026, 10, 18)),
        &[Lever::App7NoMarketing],
    );
    msg.requests[0].events.extend([
        status_event(
            date(2026, 11, 2),
            Status::RefusedWithReasons,
            Some(Reason::SmallBusiness),
        ),
        status_event(date(2026, 11, 20), Status::Escalated, None),
    ]);
    msg
}

fn demo_done_message() -> Message {
    let mut msg = demo_message(
        "GS-DM004",
        "retail-delta-example",
        "privacy@retail-delta.example.com",
        date(2026, 11, 1),
        Some(date(2026, 11, 1)),
        &[Lever::App7NoMarketing],
    );
    msg.requests[0]
        .events
        .push(status_event(date(2026, 11, 8), Status::Suppressed, None));
    msg
}

fn demo_messages() -> Vec<Message> {
    vec![
        demo_message(
            "GS-DM001",
            "broker-alpha-example",
            "privacy@broker-alpha.example.com",
            date(2026, 10, 14),
            Some(date(2026, 10, 15)),
            &[Lever::App7NoMarketing, Lever::App7Source],
        ),
        demo_escalated_message(),
        demo_message(
            "GS-DM003",
            "credit-gamma-example",
            "privacy@credit-gamma.example.com",
            date(2026, 11, 14),
            Some(date(2026, 11, 14)),
            &[Lever::S20rAccess],
        ),
        demo_done_message(),
        demo_message(
            "GS-DM005",
            "broker-alpha-example",
            "privacy@broker-alpha.example.com",
            DEMO_DATE,
            None,
            &[Lever::App7NoFacilitation],
        ),
    ]
}

fn push_reply_audit(
    chain: &mut Vec<AuditEntry>,
    on: Date,
    tag: &str,
    entry: &str,
    kind: AuditKind,
    status: Status,
    reason: Option<Reason>,
) {
    let detail = audit::reply_detail(on, status, &[Lever::App7NoMarketing], reason, None, None);
    audit::append(chain, on, tag, entry, kind, detail);
}

fn demo_audit() -> Vec<AuditEntry> {
    let mut chain = Vec::new();
    let d1 = audit::send_detail(date(2026, 10, 15), ChannelKind::Email, false);
    audit::append(
        &mut chain,
        date(2026, 10, 15),
        "GS-DM001",
        "broker-alpha-example",
        AuditKind::Sent,
        d1,
    );
    let d2 = audit::send_detail(date(2026, 10, 18), ChannelKind::Email, false);
    audit::append(
        &mut chain,
        date(2026, 10, 18),
        "GS-DM002",
        "list-beta-example",
        AuditKind::Sent,
        d2,
    );
    push_reply_audit(
        &mut chain,
        date(2026, 11, 2),
        "GS-DM002",
        "list-beta-example",
        AuditKind::Reply,
        Status::RefusedWithReasons,
        Some(Reason::SmallBusiness),
    );
    push_reply_audit(
        &mut chain,
        date(2026, 11, 20),
        "GS-DM002",
        "list-beta-example",
        AuditKind::Escalation,
        Status::Escalated,
        None,
    );
    push_reply_audit(
        &mut chain,
        date(2026, 11, 8),
        "GS-DM004",
        "retail-delta-example",
        AuditKind::Reply,
        Status::Suppressed,
        None,
    );
    chain
}

fn demo_checklist_marks() -> Vec<Mark> {
    vec![
        Mark {
            item: "do-not-call".to_owned(),
            mark: MarkKind::Done,
            on: date(2026, 11, 1),
        },
        Mark {
            item: "breach-check".to_owned(),
            mark: MarkKind::Done,
            on: date(2026, 1, 10),
        },
        Mark {
            item: "silent-elector".to_owned(),
            mark: MarkKind::NotForMe,
            on: date(2026, 11, 1),
        },
    ]
}

fn sample_eml() -> String {
    let letter = Letter {
        kind: ChannelKind::Email,
        to: "privacy@broker-alpha.example.com".to_owned(),
        subject: "Privacy Act request [GS-DM005]".to_owned(),
        body: "24 November 2026\n\n\
               To: Privacy Officer, Broker Alpha Pty Ltd (example.com fixture)\n\n\
               Under Australian Privacy Principle 7.6(d) of the Privacy Act 1988 (Cth),\n\
               please stop using or disclosing my personal information to facilitate\n\
               direct marketing by other organisations.\n\n\
               Details to locate my record:\n\
               Name: Alex Example\n\
               Email: alex@example.com\n\n\
               Reference: [GS-DM005]"
            .to_owned(),
    };
    letters::eml(&letter)
}

/// Build the full zero-PII synthetic demo snapshot purely in memory.
pub fn build() -> DemoSnapshot {
    let mut state = State::new(demo_profile());
    state.messages = demo_messages();
    state.checklist = demo_checklist_marks();
    state.audit = demo_audit();
    let audit_summary = audit::verify_chain(&state.audit).expect("demo audit chain is valid");
    let checklist_items =
        checklist::parse(&embedded::checklist()).expect("embedded checklist is valid");
    let mut rows = Vec::new();
    for message in &state.messages {
        let sent = message.sent().is_some();
        for request in &message.requests {
            let due = deadline::due(message, request);
            rows.push(DemoRow {
                action: request.action(sent, due, DEMO_DATE),
                tag: message.tag.clone(),
                entry: message.entry.clone(),
                lever: request.lever,
                status: request.status(sent),
                due,
            });
        }
    }
    rows.sort_by_key(|row| row.action);
    DemoSnapshot {
        date: DEMO_DATE,
        state,
        rows,
        audit_summary,
        checklist_items,
        sample_eml: sample_eml(),
    }
}
