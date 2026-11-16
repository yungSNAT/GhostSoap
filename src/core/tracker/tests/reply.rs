// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! Recording answers, longer periods and withdrawals, and the action each request needs.

use super::*;
use crate::core::deadline::Due;

const BOTH: &[Lever] = &[];
const SOURCE: &[Lever] = &[Lever::App7Source];

fn statuses(message: &Message) -> Vec<Vec<Status>> {
    let request = |r: &Request| r.in_effect().statuses.iter().map(|s| s.status).collect();
    message.requests.iter().map(request).collect()
}

#[test]
fn a_reply_needs_a_letter_sent_and_a_day_that_fits() {
    let today = day(20);
    let mut unsent = message("GS-7K3QX");
    let error = unsent
        .record_status(BOTH, status(day(12), Status::Refused), today)
        .unwrap_err();
    assert!(error.contains("GS-7K3QX is not recorded as sent, so it can only be given up"));
    let early = unsent.record_status(BOTH, status(day(9), Status::Abandoned), today);
    assert_eq!(
        early.unwrap_err(),
        "GS-7K3QX was drafted on 2026-11-10, after 2026-11-09"
    );
    let given_up = status(day(10), Status::Abandoned);
    assert_eq!(unsent.record_status(BOTH, given_up, today), Ok(true));

    let mut sent = sent_message("GS-7K3QX");
    let before = sent.clone();
    let refused = [
        (
            status(day(21), Status::Acknowledged),
            "2026-11-21 is in the future",
        ),
        (
            status(day(10), Status::Acknowledged),
            "GS-7K3QX was sent on 2026-11-11, after 2026-11-10",
        ),
        (
            StatusEvent {
                reason: Some(Reason::Other),
                ..status(day(12), Status::Fulfilled)
            },
            "a reason goes only with refused, refused-with-reasons or exempt-claimed",
        ),
        (
            StatusEvent {
                source: Some("a list broker".to_owned()),
                ..status(day(12), Status::Fulfilled)
            },
            "GS-7K3QX holds more than one request; name the one the source was given for \
             with --lever",
        ),
    ];
    for (event, why) in refused {
        assert_eq!(sent.record_status(BOTH, event, today).unwrap_err(), why);
    }
    let app12 = &[Lever::App12];
    let missing = sent.record_status(app12, status(day(12), Status::Fulfilled), today);
    assert_eq!(missing.unwrap_err(), "GS-7K3QX holds no app12 request");
    assert_eq!(sent, before, "nothing was recorded");
}

#[test]
fn a_reply_is_recorded_once_on_each_request_it_names() {
    let today = day(20);
    let mut sent = sent_message("GS-7K3QX");
    let acknowledged = status(day(12), Status::Acknowledged);
    assert_eq!(
        sent.record_status(BOTH, acknowledged.clone(), today),
        Ok(true)
    );
    assert_eq!(sent.record_status(BOTH, acknowledged, today), Ok(false));
    let twice = [Lever::App7Source, Lever::App7Source];
    let sourced = StatusEvent {
        source: Some("a list broker".to_owned()),
        ..status(day(13), Status::Fulfilled)
    };
    assert_eq!(sent.record_status(&twice, sourced, today), Ok(true));
    assert_eq!(
        statuses(&sent),
        [
            vec![Status::Acknowledged],
            vec![Status::Acknowledged, Status::Fulfilled]
        ]
    );
    let earlier = sent.record_status(SOURCE, status(day(12), Status::Refused), today);
    assert_eq!(
        earlier.unwrap_err(),
        "GS-7K3QX app7-source has an answer recorded on 2026-11-13, after 2026-11-12"
    );
}

#[test]
fn a_reply_must_follow_what_is_already_recorded() {
    let today = day(20);
    let mut sent = sent_message("GS-7K3QX");
    for (to, why) in [
        (
            Status::Closed,
            "has no answer to close it with; record it as abandoned",
        ),
        (
            Status::Relisted,
            "was never recorded as acted on, so it cannot be relisted",
        ),
    ] {
        let error = sent.record_status(SOURCE, status(day(12), to), today);
        assert_eq!(error.unwrap_err(), format!("GS-7K3QX app7-source {why}"));
    }
    let partial = status(day(12), Status::Partial);
    sent.record_status(SOURCE, partial, today).unwrap();
    // All or none: the other request refuses, so this one records nothing either.
    let before = sent.clone();
    let relisted = status(day(13), Status::Relisted);
    assert!(sent.record_status(BOTH, relisted.clone(), today).is_err());
    assert_eq!(sent, before);
    assert_eq!(sent.record_status(SOURCE, relisted, today), Ok(true));
    let given_up = sent.record_status(SOURCE, status(day(14), Status::Abandoned), today);
    assert_eq!(
        given_up.unwrap_err(),
        "GS-7K3QX app7-source has an answer, so record it as closed"
    );
    let closed = status(day(14), Status::Closed);
    assert_eq!(sent.record_status(SOURCE, closed, today), Ok(true));

    let marketing = &[Lever::App7NoMarketing];
    let bounced = status(day(12), Status::Bounced);
    sent.record_status(marketing, bounced, today).unwrap();
    let next = sent.record_status(marketing, status(day(13), Status::Escalated), today);
    assert!(
        next.unwrap_err().ends_with(
            "bounced, so it can only be given up (abandoned); draft it again to send it"
        )
    );
    let given_up = status(day(13), Status::Abandoned);
    assert_eq!(sent.record_status(marketing, given_up, today), Ok(true));
}

#[test]
fn a_longer_period_needs_a_credit_correction_and_a_later_day() {
    let today = day(20);
    let agreed = |on: NaiveDate, until: NaiveDate| Extension {
        on,
        until,
        note: None,
    };
    let december = |day: u32| NaiveDate::from_ymd_opt(2026, 12, day).unwrap();
    let mut unsent = message("GS-7K3QX");
    unsent.requests = vec![Request::new(Lever::S20tCorrection)];
    let error = unsent.record_extension(BOTH, agreed(day(12), december(24)), today);
    assert_eq!(error.unwrap_err(), "GS-7K3QX is not recorded as sent");
    let mut sent = unsent.clone();
    sent.sends = vec![SendRecord::Sent(email(day(11)))];
    let future = sent.record_extension(BOTH, agreed(day(21), december(24)), today);
    assert_eq!(future.unwrap_err(), "2026-11-21 is in the future");
    let early = sent.record_extension(BOTH, agreed(day(10), december(24)), today);
    assert_eq!(
        early.unwrap_err(),
        "GS-7K3QX was sent on 2026-11-11, after 2026-11-10"
    );
    // 30 days starting on 11 November end on 10 December.
    let short = sent.record_extension(BOTH, agreed(day(12), december(10)), today);
    assert_eq!(
        short.unwrap_err(),
        "GS-7K3QX s20t-correction: 2026-12-10 is not later than the law's last day, \
         2026-12-10"
    );
    let longer = agreed(day(12), december(24));
    assert_eq!(sent.record_extension(BOTH, longer.clone(), today), Ok(true));
    assert_eq!(sent.record_extension(BOTH, longer, today), Ok(false));

    let mut marketing = sent_message("GS-7K3QX");
    let error = marketing.record_extension(SOURCE, agreed(day(12), december(24)), today);
    assert_eq!(
        error.unwrap_err(),
        "GS-7K3QX app7-source has no period the law lets you agree to lengthen"
    );
}

#[test]
fn a_withdrawal_needs_something_in_effect_to_withdraw() {
    let mut sent = sent_message("GS-7K3QX");
    for (of, what) in [
        (Withdrawn::Status, "answer"),
        (Withdrawn::Extension, "longer period"),
    ] {
        let error = sent.withdraw_event(SOURCE, of, None).unwrap_err();
        assert_eq!(
            error,
            format!("GS-7K3QX app7-source has no {what} to withdraw")
        );
    }
    let refused = status(day(12), Status::Refused);
    sent.record_status(SOURCE, refused, day(20)).unwrap();
    sent.withdraw_event(SOURCE, Withdrawn::Status, note("misread"))
        .unwrap();
    assert_eq!(statuses(&sent), [Vec::<Status>::new(), Vec::new()]);
    assert_eq!(
        sent.requests[1].events.len(),
        2,
        "the record stays as history"
    );
}

#[test]
fn each_status_falls_in_one_action_state() {
    let today = day(20);
    let passed = Due::Guide(day(19));
    let ahead = Due::Statutory(day(21));
    let request = |lever: Lever, latest: Option<Status>| {
        let mut request = Request::new(lever);
        request.events.extend(latest.map(|s| answer(day(12), s)));
        request
    };
    let action = |lever, latest, due| request(lever, latest).action(true, due, today);
    let unsent = request(Lever::App12, None).action(false, passed, today);
    assert_eq!(unsent, Action::Needed);
    assert_eq!(action(Lever::App12, None, passed), Action::Overdue);
    let late = Due::Statutory(day(19));
    let acknowledged = Some(Status::Acknowledged);
    assert_eq!(action(Lever::App12, acknowledged, late), Action::Overdue);
    for waiting in [ahead, Due::Unknown, Due::None] {
        assert_eq!(action(Lever::App12, None, waiting), Action::Waiting);
    }
    let escalated = Some(Status::Escalated);
    assert_eq!(action(Lever::App12, escalated, passed), Action::Waiting);
    for done in [Status::Fulfilled, Status::NoRecord, Status::Closed] {
        assert_eq!(action(Lever::App12, Some(done), passed), Action::Done);
    }
    let abandoned = Some(Status::Abandoned);
    assert_eq!(action(Lever::App12, abandoned, passed), Action::Abandoned);
    for needed in [
        Status::IdRequested,
        Status::Refused,
        Status::Bounced,
        Status::Relisted,
    ] {
        assert_eq!(action(Lever::App12, Some(needed), passed), Action::Needed);
    }
    let suppressed = Some(Status::Suppressed);
    assert_eq!(
        action(Lever::App11Destroy, suppressed, ahead),
        Action::Needed
    );
    for stop in [
        Lever::App7NoMarketing,
        Lever::App7NoFacilitation,
        Lever::SpamUnsubscribe,
    ] {
        assert_eq!(action(stop, suppressed, ahead), Action::Done);
    }
}
