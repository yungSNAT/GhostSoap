// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

use super::*;
use crate::core::register::{self, ChannelKind};
use crate::core::tracker::{Event, Extension, Send, SendRecord, StatusEvent, Target};
use crate::core::{deadline::past_weekend, embedded};

fn day(month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, month, day).unwrap()
}

fn equifax() -> Entry {
    let mut register = register::parse_register(&embedded::register()).unwrap();
    register.entries.remove("equifax-au").unwrap()
}

fn profile() -> Profile {
    Profile {
        full_name: "Lara Croft".to_owned(),
        ..Profile::default()
    }
}

/// A letter to Equifax with `levers`, sent by email on 1 October.
fn sent(levers: &[Lever]) -> Message {
    Message {
        tag: "GS-7K3QX".to_owned(),
        entry: "equifax-au".to_owned(),
        channel: ChannelKind::Email,
        address: "privacy@example.com.au".to_owned(),
        drafted: day(10, 1),
        draft: None,
        superseded: false,
        unfinished: false,
        sends: vec![SendRecord::Sent(Send {
            on: day(10, 1),
            channel: ChannelKind::Email,
        })],
        requests: levers
            .iter()
            .map(|&lever| Request {
                lever,
                letter_version: 1,
                deadline: law(lever).deadline,
                events: Vec::new(),
            })
            .collect(),
    }
}

fn answer(message: &mut Message, at: usize, status: Status) {
    let event = StatusEvent {
        on: day(10, 2),
        status,
        reason: None,
        to: (status == Status::Escalated).then_some(Target::Business),
        source: None,
        note: None,
    };
    message.requests[at].events.push(Event::Status(event));
}

fn write(kind: Kind, message: &Message, levers: &[Lever], today: NaiveDate) -> String {
    compose(kind, &equifax(), &profile(), message, levers, today)
        .unwrap()
        .body
        .replace('\n', " ")
}

#[test]
fn a_reminder_after_the_dates_follows_each_deadline_kind() {
    let levers = [Lever::S20tCorrection, Lever::App12, Lever::SpamUnsubscribe];
    let message = sent(&levers);
    let body = write(Kind::Reminder, &message, &[], day(11, 17));
    // 30 days starting on 1 October end on 30 October, a Friday (s 20T(2)(a)).
    assert_eq!(past_weekend(day(10, 30)), day(10, 30));
    for expected in [
        "I made this request on 1 October 2026, 47 days ago.",
        "By my count the period the law sets ended on 30 October 2026, so it appears to have passed; a public holiday where you must act would move that day later.",
        "The period the OAIC's APP guidelines give as generally reasonable ended on 31 October 2026.",
        "my next step is a complaint to you, and then to",
        "Reference: [GS-7K3QX]",
    ] {
        assert!(body.contains(expected), "{expected}: {body}");
    }
    assert!(!body.contains("breach"), "{body}");
    assert!(
        !body.contains("Unsubscribe") && !body.contains("Spam"),
        "{body}"
    );
    let letter = compose(
        Kind::Reminder,
        &equifax(),
        &profile(),
        &message,
        &[],
        day(11, 17),
    );
    assert_eq!(
        letter.unwrap().subject,
        "Follow-up: Privacy Act request [GS-7K3QX]"
    );
}

#[test]
fn a_reminder_before_the_date_asks_for_an_answer_by_it_and_names_an_agreed_period() {
    let mut message = sent(&[Lever::S20tCorrection, Lever::App12]);
    let body = write(Kind::Reminder, &message, &[], day(10, 20));
    assert!(body.contains("please answer by 30 October 2026."), "{body}");
    assert!(body.contains("please answer by 31 October 2026."), "{body}");
    assert!(!body.contains("next step"), "{body}");

    let extension = Extension {
        on: day(10, 2),
        until: day(11, 16),
        note: None,
    };
    message.requests[0].events.push(Event::Extension(extension));
    let body = write(
        Kind::Reminder,
        &message,
        &[Lever::S20tCorrection],
        day(11, 17),
    );
    assert!(
        body.contains(
            "By my count the longer period we agreed in writing ended on 16 November 2026"
        ),
        "{body}"
    );
    assert!(!body.contains("Give me access"), "{body}");
}

#[test]
fn a_complaint_names_what_went_wrong_and_when_the_commissioner_can_be_asked_first() {
    let mut message = sent(&[Lever::S20tCorrection, Lever::App12, Lever::App13]);
    answer(&mut message, 1, Status::Refused);
    answer(&mut message, 2, Status::Fulfilled);
    let body = write(Kind::Complaint, &message, &[], day(11, 17));
    for expected in [
        "This is a complaint under the Privacy Act 1988 (Cth) about how",
        "my letter of 1 October 2026",
        "appears to have ended on 30 October 2026; a public holiday where you must act would move that day later.",
        "without first being made to you (section 40(1B)).",
        "You refused it.",
        "Please reply in writing within 30 days",
    ] {
        assert!(body.contains(expected), "{expected}: {body}");
    }
    assert_eq!(body.matches("I still ask you").count(), 2, "{body}");
    assert_eq!(body.matches("section 40(1B)").count(), 1, "{body}");
}

#[test]
fn an_undated_request_gets_a_plain_reminder_and_each_answer_its_own_complaint() {
    let mut entry = equifax();
    entry.escalation = vec![register::Escalation::Acma];
    let message = sent(&[Lever::App12, Lever::App13NotifyRecipients]);
    let letter = compose(
        Kind::Reminder,
        &entry,
        &profile(),
        &message,
        &[],
        day(11, 17),
    );
    let body = letter.unwrap().body.replace('\n', " ");
    assert!(
        body.contains("I have not yet had your answer. Please answer."),
        "{body}"
    );
    assert!(
        body.contains("my next step is a complaint to you. "),
        "no Privacy Act place to name: {body}"
    );

    let complaints = [
        (
            None,
            "the period the OAIC's APP guidelines give as generally reasonable ended on 31 October 2026.",
        ),
        (Some(Status::Partial), "You dealt with only part of it."),
        (
            Some(Status::ChargeQuoted),
            "You asked for a charge before acting on it.",
        ),
        (
            Some(Status::IdRequested),
            "You asked for proof of identity before acting on it.",
        ),
        (
            Some(Status::ExemptClaimed),
            "You said the law does not apply to you.",
        ),
    ];
    for (status, expected) in complaints {
        let mut message = sent(&[Lever::App12]);
        if let Some(status) = status {
            answer(&mut message, 0, status);
        }
        let body = write(Kind::Complaint, &message, &[], day(11, 17));
        assert!(body.contains(expected), "{expected}: {body}");
    }
}

#[test]
fn a_follow_up_refuses_what_it_cannot_write() {
    let mut message = sent(&[Lever::App12]);
    let refused = |message: &Message, kind, levers: &[Lever], today| {
        compose(kind, &equifax(), &profile(), message, levers, today).unwrap_err()
    };
    let today = day(11, 17);
    assert_eq!(
        refused(&message, Kind::Complaint, &[], day(10, 20)),
        "GS-7K3QX has no request a complaint suits"
    );
    assert_eq!(
        refused(&message, Kind::Reminder, &[Lever::App13], today),
        "GS-7K3QX has no app13 request"
    );
    answer(&mut message, 0, Status::Escalated);
    assert_eq!(
        refused(&message, Kind::Reminder, &[Lever::App12], today),
        "GS-7K3QX app12 is not waiting for an answer, so it gets no reminder"
    );
    let mut ban = sent(&[Lever::S20kBan]);
    assert_eq!(
        refused(&ban, Kind::Reminder, &[Lever::S20kBan], today),
        "GS-7K3QX s20k-ban takes no reply, so it gets no reminder"
    );
    ban.sends.clear();
    assert_eq!(
        refused(&ban, Kind::Reminder, &[], today),
        "GS-7K3QX is not recorded as sent, so there is nothing to follow up"
    );
    assert_eq!(Kind::Complaint.name(), "complaint");
}
