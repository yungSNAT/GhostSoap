// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! Unit tests of the tracker's records.

use super::*;
use crate::core::{embedded, register};

mod format;

fn day(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, day).unwrap()
}

fn request(lever: Lever, letter_version: u32, deadline: Deadline) -> Request {
    Request {
        lever,
        letter_version,
        deadline,
        events: Vec::new(),
    }
}

/// A letter to one entry drafted on 10 November, carrying 2 requests.
fn message(tag: &str) -> Message {
    let guide = Deadline::Reasonable {
        guide_days: Some(30),
    };
    Message {
        tag: tag.to_owned(),
        entry: "cotality-au".to_owned(),
        channel: ChannelKind::Email,
        address: "privacy@example.com.au".to_owned(),
        drafted: day(10),
        draft: None,
        superseded: false,
        unfinished: false,
        sends: Vec::new(),
        requests: vec![
            request(Lever::App7NoMarketing, 1, guide),
            request(Lever::App7Source, 1, guide),
        ],
    }
}

fn status(on: NaiveDate, status: Status) -> StatusEvent {
    StatusEvent {
        on,
        status,
        reason: None,
        source: None,
        note: None,
    }
}

fn answer(on: NaiveDate, to: Status) -> Event {
    Event::Status(status(on, to))
}

fn withdrawal(of: Withdrawn) -> Event {
    Event::Withdrawal(Withdrawal { of, note: None })
}

fn by(on: NaiveDate, channel: ChannelKind) -> Send {
    Send { on, channel }
}

fn email(on: NaiveDate) -> Send {
    by(on, ChannelKind::Email)
}

/// `message`, recorded as sent by email on 11 November.
fn sent_message(tag: &str) -> Message {
    let mut message = message(tag);
    message.sends = vec![SendRecord::Sent(email(day(11)))];
    message
}

fn state(messages: Vec<Message>) -> State {
    let profile = Profile {
        full_name: "Lara Croft".to_owned(),
        ..Profile::default()
    };
    State {
        messages,
        ..State::new(profile)
    }
}

#[test]
fn a_new_request_keeps_the_law_tables_deadline_and_the_letters_version() {
    for lever in Lever::ALL {
        let request = Request::new(lever);
        let name = lever.name();
        assert_eq!(request.deadline, law::law(lever).deadline, "{name}");
        assert_eq!(request.letter_version, letters::letter_version(lever));
        assert!(request.events.is_empty());
    }
    assert_eq!(Request::new(Lever::S20rAccess).deadline, Deadline::Days(10));
}

#[test]
fn a_withdrawal_voids_the_latest_event_of_its_kind_still_in_effect() {
    let extension = |until| Extension {
        on: day(14),
        until: day(until),
        note: None,
    };
    let mut request = request(Lever::S20tCorrection, 1, Deadline::Days(30));
    assert!(request.in_effect().is_empty());
    request.events = vec![
        answer(day(12), Status::Acknowledged),
        Event::Extension(extension(30)),
        answer(day(13), Status::Refused),
        // A later extension can shorten an earlier one: the latest in effect counts.
        Event::Extension(extension(25)),
        withdrawal(Withdrawn::Status),
    ];
    let kept = request.in_effect();
    let statuses: Vec<Status> = kept.statuses.iter().map(|s| s.status).collect();
    assert_eq!(statuses, [Status::Acknowledged]);
    assert_eq!(kept.extensions.last().unwrap().until, day(25));
    assert_eq!(kept.first_date(), Some(day(12)));

    request.events.extend([
        withdrawal(Withdrawn::Extension),
        withdrawal(Withdrawn::Status),
    ]);
    let kept = request.in_effect();
    assert!(kept.statuses.is_empty(), "the request is back to sent");
    assert_eq!(kept.extensions.last().unwrap().until, day(30));
    assert_eq!(request.events.len(), 7, "nothing is removed");
}

#[test]
fn sent_is_recorded_once() {
    let mut message = message("GS-7K3QX");
    let send = email(day(11));
    assert!(message.is_replaceable());
    assert_eq!(message.sent(), None);
    assert_eq!(message.record_sent(send, false, day(12)), Ok(true));
    assert_eq!(message.sent(), Some(send));
    assert!(!message.is_replaceable());
    assert_eq!(
        message.record_sent(send, false, day(12)),
        Ok(false),
        "the same record again changes nothing"
    );
    assert_eq!(message.record_sent(send, true, day(12)), Ok(false));
    assert_eq!(message.sends, [SendRecord::Sent(send)]);
}

#[test]
fn a_different_record_of_sending_needs_correct_and_keeps_the_first() {
    let mut message = sent_message("GS-7K3QX");
    let second = by(day(10), ChannelKind::Post);
    assert_eq!(
        message.record_sent(second, false, day(12)).unwrap_err(),
        "GS-7K3QX is already recorded as sent on 2026-11-11 by email; add --correct to \
         replace that record"
    );
    assert_eq!(message, sent_message("GS-7K3QX"));
    assert_eq!(message.record_sent(second, true, day(12)), Ok(true));
    let records = [email(day(11)), second].map(SendRecord::Sent);
    assert_eq!(message.sends, records, "the first stays as history");
    assert_eq!(message.sent(), Some(second));
}

#[test]
fn sent_refuses_a_record_that_cannot_be_true() {
    let draft = message("GS-7K3QX");
    for (send, correct, today, expected) in [
        (
            email(day(12)),
            false,
            day(11),
            "2026-11-12 is in the future",
        ),
        (
            email(day(9)),
            false,
            day(11),
            "GS-7K3QX was drafted on 2026-11-10, after 2026-11-09; if you sent an earlier \
             draft, its requests may differ from this one's, so check what you sent",
        ),
        (
            email(day(11)),
            true,
            day(11),
            "GS-7K3QX has no record of sending to correct",
        ),
    ] {
        let mut message = draft.clone();
        let error = message.record_sent(send, correct, today).unwrap_err();
        assert_eq!(error, expected);
        assert_eq!(message, draft);
    }
    // A draft the user gave up has an event in effect and no record of sending.
    let mut given_up = draft.clone();
    given_up.requests[0].events = vec![answer(day(10), Status::Abandoned)];
    assert!(!given_up.is_replaceable());
    let error = given_up.record_sent(email(day(11)), false, day(11));
    assert_eq!(
        error.unwrap_err(),
        "GS-7K3QX was given up as a draft; draft the letter again"
    );
}

#[test]
fn a_corrected_day_of_sending_cannot_fall_after_an_answer_in_effect() {
    let mut answered = sent_message("GS-7K3QX");
    answered.requests[1].events = vec![answer(day(13), Status::Fulfilled)];
    let error = answered.record_sent(email(day(14)), true, day(20));
    assert_eq!(
        error.unwrap_err(),
        "GS-7K3QX has an answer recorded on 2026-11-13, before 2026-11-14"
    );
    let on_the_day = answered.record_sent(email(day(13)), true, day(20));
    assert_eq!(on_the_day, Ok(true));

    // An answer that was withdrawn and recorded again for a later day no longer counts.
    let mut corrected = sent_message("GS-7K3QX");
    corrected.requests[1].events = vec![
        answer(day(12), Status::Fulfilled),
        withdrawal(Withdrawn::Status),
        answer(day(14), Status::Fulfilled),
    ];
    let later = corrected.record_sent(email(day(13)), true, day(20));
    assert_eq!(later, Ok(true));
    assert_eq!(corrected.sent(), Some(email(day(13))));
}

#[test]
fn a_record_of_sending_can_be_withdrawn_until_an_answer_is_in_effect() {
    let mut message = message("GS-7K3QX");
    assert_eq!(
        message.withdraw_sent(None).unwrap_err(),
        "GS-7K3QX is not recorded as sent"
    );
    message.record_sent(email(day(11)), false, day(12)).unwrap();
    let note = Some("the other letter".to_owned());
    message.withdraw_sent(note).unwrap();
    assert_eq!(message.sent(), None);
    assert_eq!(message.sends.len(), 2, "the record stays as history");
    assert!(message.is_replaceable(), "it can be drafted again");
    assert!(message.withdraw_sent(None).is_err(), "nothing to withdraw");
    assert_eq!(state(vec![message.clone()]).problems(), [""; 0]);

    // Sending is then recorded afresh, with no correction to make.
    let again = message.record_sent(email(day(12)), false, day(12));
    assert_eq!(again, Ok(true));
    message.requests[0].events = vec![answer(day(13), Status::Acknowledged)];
    assert_eq!(
        message.withdraw_sent(None).unwrap_err(),
        "GS-7K3QX has an answer recorded, so its record of sending cannot be withdrawn"
    );
    message.requests[0]
        .events
        .push(withdrawal(Withdrawn::Status));
    assert_eq!(message.withdraw_sent(None), Ok(()));
    assert_eq!(state(vec![message]).problems(), [""; 0]);
}

#[test]
fn a_new_draft_replaces_the_unsent_one_to_the_same_or_a_dropped_address() {
    let register = register::parse_register(&embedded::register()).unwrap();
    // Myer has an email channel for access requests and another for the rest.
    let entry = &register.entries["myer-one-au"];
    let serving = |lever| {
        let serves = |c: &&Channel| c.kind == ChannelKind::Email && c.levers.contains(&lever);
        entry.channels.iter().find(serves).unwrap()
    };
    let (access, marketing) = (serving(Lever::App12), serving(Lever::App7Source));
    assert_ne!(access.address, marketing.address);
    let to = |tag: &str, address: &str| Message {
        entry: entry.id.clone(),
        address: address.to_owned(),
        ..message(tag)
    };
    let mut sent = to("GS-11111", &access.address);
    sent.sends = vec![SendRecord::Sent(email(day(10)))];
    let mut elsewhere = to("GS-22222", &access.address);
    elsewhere.entry = "cotality-au".to_owned();
    let state = state(vec![
        sent,
        elsewhere,
        to("GS-33333", &marketing.address),
        to("GS-44444", "old.address@myer.example"),
        to("GS-55555", &access.address),
    ]);
    let found = |channel, claimed: &[&str]| {
        let claimed: Vec<String> = claimed.iter().map(|&t| t.to_owned()).collect();
        let found = state.draft_to_replace(entry, channel, &claimed);
        found.map(|m| m.tag.as_str())
    };
    assert_eq!(found(access, &[]), Some("GS-55555"), "the same address");
    assert_eq!(found(marketing, &[]), Some("GS-33333"));
    assert_eq!(
        found(access, &["GS-55555"]),
        Some("GS-44444"),
        "then one to an address the entry no longer lists"
    );
    assert_eq!(
        found(access, &["GS-55555", "GS-44444"]),
        None,
        "never a sent letter, another entry's or one to a channel still listed"
    );
}

#[test]
fn problems_names_each_broken_rule() {
    assert_eq!(state(vec![message("GS-7K3QX")]).problems(), [""; 0]);
    let mut newer = state(Vec::new());
    newer.format = 3;
    assert_eq!(newer.problems(), ["format is 3, not 2"]);

    let mut empty = message("GS-11111");
    empty.requests.clear();
    let mut doubled = message("GS-22222");
    doubled.requests[1].lever = Lever::App7NoMarketing;
    let broken = state(vec![
        message("GS-7K3QX"),
        message("gs-7k3qx"),
        message("GS-7K3QX"),
        empty,
        doubled,
    ]);
    assert_eq!(
        broken.problems(),
        [
            "message 2 has no valid tag",
            "tag GS-7K3QX is used twice",
            "GS-11111 holds no request",
            "GS-22222 holds app7-no-marketing twice",
        ]
    );
}

/// `sent_message` with `events` on its second request, and the same never sent.
fn with(events: Vec<Event>) -> Message {
    let mut message = sent_message("GS-7K3QX");
    message.requests[1].events = events;
    message
}

fn unsent(events: Vec<Event>) -> Message {
    Message {
        sends: Vec::new(),
        ..with(events)
    }
}

fn extension(on: u32, until: u32) -> Event {
    Event::Extension(Extension {
        on: day(on),
        until: day(until),
        note: None,
    })
}

fn reasoned(to: Status) -> Event {
    Event::Status(StatusEvent {
        reason: Some(Reason::Other),
        ..status(day(12), to)
    })
}

#[test]
fn problems_names_records_that_contradict_each_other() {
    let mut early = message("GS-7K3QX");
    early.sends = vec![SendRecord::Sent(email(day(9)))];
    let mut unsent_twice = sent_message("GS-7K3QX");
    let withdrawn = SendRecord::Withdrawn { note: None };
    unsent_twice.sends.extend([withdrawn.clone(), withdrawn]);
    let of_message = [
        (early, "is recorded as sent before it was drafted"),
        (unsent_twice, "withdraws a record of sending it lacks"),
    ];
    let no_send = "has an answer, but the message is not recorded as sent";
    let of_request = [
        (
            with(vec![answer(day(12), Status::Sent)]),
            "has an event that sets drafted or sent",
        ),
        (
            with(vec![reasoned(Status::Fulfilled)]),
            "gives a reason for an answer that is not a refusal",
        ),
        (
            with(vec![extension(14, 13)]),
            "has a longer period that ends before it was agreed",
        ),
        (
            with(vec![withdrawal(Withdrawn::Status)]),
            "withdraws an event it lacks",
        ),
        (
            with(vec![extension(12, 30), withdrawal(Withdrawn::Status)]),
            "withdraws an event it lacks",
        ),
        (
            with(vec![answer(day(10), Status::Acknowledged)]),
            "has an event dated before the message was sent",
        ),
        (unsent(vec![answer(day(12), Status::Fulfilled)]), no_send),
        (unsent(vec![extension(12, 30)]), no_send),
    ];
    for (message, problem) in of_message {
        let expected = format!("GS-7K3QX {problem}");
        assert_eq!(state(vec![message]).problems(), [expected]);
    }
    for (message, problem) in of_request {
        let expected = format!("GS-7K3QX app7-source {problem}");
        assert_eq!(state(vec![message]).problems(), [expected]);
    }
}

#[test]
fn problems_allows_what_the_records_can_truthfully_hold() {
    // A refusal with its reason, a draft given up, and an event dated before the
    // message was sent that has since been withdrawn.
    for message in [
        with(vec![reasoned(Status::ExemptClaimed)]),
        unsent(vec![answer(day(10), Status::Abandoned)]),
        with(vec![
            answer(day(10), Status::Acknowledged),
            withdrawal(Withdrawn::Status),
        ]),
    ] {
        assert_eq!(state(vec![message]).problems(), [""; 0]);
    }
}
