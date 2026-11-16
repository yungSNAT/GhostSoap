// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! State format 2 as text: what is written, what is read and what is refused.

use super::*;

/// Format 2 as it is written, holding every kind of record. A change here is a change
/// to what existing state files mean, so it needs a new format number and a migration.
const FORMAT_2: &str = r#"format = 2

[profile]
full_name = "Lara Croft"
alias_names = []
previous_addresses = []

[[messages]]
tag = "GS-7K3QX"
entry = "cotality-au"
channel = "email"
address = "privacy@example.com.au"
drafted = "2026-11-10"

[messages.draft]
folder = "/home/lara/drafts"
name = "cotality-au-GS-7K3QX.eml"

[[messages.sends]]
kind = "sent"
on = "2026-11-11"
channel = "email"

[[messages.sends]]
kind = "withdrawn"
note = "that was the other letter"

[[messages.sends]]
kind = "sent"
on = "2026-11-12"
channel = "post"

[[messages.requests]]
lever = "s20t-correction"
letter_version = 2

[messages.requests.deadline]
days-starting = 30

[[messages.requests.events]]
kind = "status"
on = "2026-11-13"
status = "acknowledged"

[[messages.requests.events]]
kind = "extension"
on = "2026-11-14"
until = "2026-12-20"
note = "agreed by email"

[[messages.requests.events]]
kind = "withdrawal"
of = "extension"
note = "they never agreed"

[[messages.requests]]
lever = "app7-source"
letter_version = 1

[messages.requests.deadline.reasonable]
guide_days = 30

[[messages.requests.events]]
kind = "status"
on = "2026-11-16"
status = "fulfilled"

[[messages.requests.events]]
kind = "withdrawal"
of = "status"

[[messages.requests.events]]
kind = "status"
on = "2026-11-17"
status = "refused-with-reasons"
reason = "not-direct-marketing"
source = "a list broker"
note = "see their letter"

[[messages.requests]]
lever = "spam-unsubscribe"
letter_version = 1

[messages.requests.deadline]
business-days = 5

[[messages.requests]]
lever = "app11-destroy"
letter_version = 1
deadline = "not-stated"

[[messages.requests]]
lever = "s21t-access"
letter_version = 1

[messages.requests.deadline.reasonable]

[[messages]]
tag = "GS-00AZ9"
entry = "cotality-au"
channel = "web-form"
address = "privacy@example.com.au"
drafted = "2026-11-10"

[[messages.requests]]
lever = "app7-no-marketing"
letter_version = 1

[messages.requests.deadline.reasonable]
guide_days = 30
"#;

/// The first message `FORMAT_2` holds, before its requests are set.
fn sent_twice() -> Message {
    let mut message = sent_message("GS-7K3QX");
    message.draft = Some(DraftFile {
        folder: "/home/lara/drafts".to_owned(),
        name: "cotality-au-GS-7K3QX.eml".to_owned(),
    });
    message.sends.extend([
        SendRecord::Withdrawn {
            note: note("that was the other letter"),
        },
        SendRecord::Sent(by(day(12), ChannelKind::Post)),
    ]);
    message
}

/// The state `FORMAT_2` holds.
fn every_record() -> State {
    let mut full = sent_twice();
    let mut correction = request(Lever::S20tCorrection, 2, Deadline::DaysStarting(30));
    correction.events = vec![
        answer(day(13), Status::Acknowledged),
        Event::Extension(Extension {
            on: day(14),
            until: NaiveDate::from_ymd_opt(2026, 12, 20).unwrap(),
            note: note("agreed by email"),
        }),
        Event::Withdrawal(Withdrawal {
            of: Withdrawn::Extension,
            note: note("they never agreed"),
        }),
    ];
    let guide = Deadline::Reasonable {
        guide_days: Some(30),
    };
    let mut source = request(Lever::App7Source, 1, guide);
    source.events = vec![
        answer(day(16), Status::Fulfilled),
        withdrawal(Withdrawn::Status),
        Event::Status(StatusEvent {
            reason: Some(Reason::NotDirectMarketing),
            source: note("a list broker"),
            note: note("see their letter"),
            ..status(day(17), Status::RefusedWithReasons)
        }),
    ];
    let no_guide = Deadline::Reasonable { guide_days: None };
    full.requests = vec![
        correction,
        source,
        request(Lever::SpamUnsubscribe, 1, Deadline::BusinessDays(5)),
        request(Lever::App11Destroy, 1, Deadline::NotStated),
        request(Lever::S21tAccess, 1, no_guide),
    ];
    let mut copied = message("GS-00AZ9");
    copied.channel = ChannelKind::WebForm;
    copied.requests.truncate(1);
    state(vec![full, copied])
}

#[test]
fn format_2_is_written_and_read_as_this_text() {
    let state = every_record();
    assert_eq!(state.problems(), [""; 0]);
    assert_eq!(toml::to_string(&state).unwrap(), FORMAT_2);
    assert_eq!(toml::from_str::<State>(FORMAT_2).unwrap(), state);
}

#[test]
fn every_status_and_reason_keeps_its_name_in_the_file() {
    let statuses = [
        (Status::Drafted, "drafted"),
        (Status::Sent, "sent"),
        (Status::Acknowledged, "acknowledged"),
        (Status::IdRequested, "id-requested"),
        (Status::ChargeQuoted, "charge-quoted"),
        (Status::Fulfilled, "fulfilled"),
        (Status::Partial, "partial"),
        (Status::Suppressed, "suppressed"),
        (Status::NoRecord, "no-record"),
        (Status::RefusedWithReasons, "refused-with-reasons"),
        (Status::Refused, "refused"),
        (Status::ExemptClaimed, "exempt-claimed"),
        (Status::Bounced, "bounced"),
        (Status::Relisted, "relisted"),
        (Status::Escalated, "escalated"),
        (Status::Closed, "closed"),
        (Status::Abandoned, "abandoned"),
    ];
    let reasons = [
        (Reason::NotDirectMarketing, "not-direct-marketing"),
        (Reason::SmallBusiness, "small-business"),
        (Reason::OutsideJurisdiction, "outside-jurisdiction"),
        (
            Reason::InsufficientVerification,
            "insufficient-verification",
        ),
        (Reason::Other, "other"),
    ];
    for (value, name) in statuses {
        let event = status(day(13), value);
        let text = toml::to_string(&event).unwrap();
        assert_eq!(text, format!("on = \"2026-11-13\"\nstatus = \"{name}\"\n"));
        assert_eq!(toml::from_str::<StatusEvent>(&text).unwrap(), event);
    }
    for (value, name) in reasons {
        let event = StatusEvent {
            reason: Some(value),
            ..status(day(13), Status::Refused)
        };
        let text = toml::to_string(&event).unwrap();
        assert!(text.ends_with(&format!("reason = \"{name}\"\n")), "{text}");
        assert_eq!(toml::from_str::<StatusEvent>(&text).unwrap(), event);
    }
}

/// Where a complaint went is kept by name; an `escalated` without one, as recorded before
/// the field existed, still reads.
#[test]
fn a_complaint_keeps_where_it_went() {
    let targets = [
        (Target::Business, "business"),
        (Target::Afca, "afca"),
        (Target::Tio, "tio"),
        (Target::Acma, "acma"),
        (Target::Oaic, "oaic"),
    ];
    for (value, name) in targets {
        let event = StatusEvent {
            to: Some(value),
            ..status(day(13), Status::Escalated)
        };
        let text = toml::to_string(&event).unwrap();
        assert!(text.ends_with(&format!("to = \"{name}\"\n")), "{text}");
        assert_eq!(toml::from_str::<StatusEvent>(&text).unwrap(), event);
    }
    let legacy = "on = \"2026-11-13\"\nstatus = \"escalated\"\n";
    let event = toml::from_str::<StatusEvent>(legacy).unwrap();
    assert_eq!(event, status(day(13), Status::Escalated));
}

#[test]
fn a_file_with_a_field_or_name_this_version_does_not_know_is_refused() {
    assert!(toml::from_str::<State>(FORMAT_2).is_ok());
    for (known, unknown) in [
        ("drafted = ", "status = \"sent\"\ndrafted = "),
        (
            "letter_version = 2",
            "letter_version = 2\nstatus = \"sent\"",
        ),
        (
            "status = \"acknowledged\"",
            "status = \"acknowledged\"\ncorrects = true",
        ),
        ("until = ", "days = 30\nuntil = "),
        (
            "of = \"extension\"",
            "of = \"extension\"\non = \"2026-11-15\"",
        ),
        (
            "kind = \"withdrawn\"",
            "kind = \"withdrawn\"\non = \"2026-11-12\"",
        ),
        (
            "channel = \"post\"",
            "channel = \"post\"\nstatus = \"sent\"",
        ),
        (
            "days-starting = 30",
            "days-starting = 30\nbusiness-days = 5",
        ),
        ("guide_days = 30", "guide_days = 30\nweeks = 4"),
        ("kind = \"withdrawal\"", "kind = \"retraction\""),
        ("of = \"status\"", "of = \"send\""),
        ("lever = \"app7-source\"", "lever = \"app99\""),
        ("channel = \"post\"", "channel = \"fax\""),
        ("deadline = \"not-stated\"", "deadline = \"none\""),
    ] {
        assert!(FORMAT_2.contains(known), "{known}");
        let changed = FORMAT_2.replacen(known, unknown, 1);
        assert!(toml::from_str::<State>(&changed).is_err(), "{unknown}");
    }
}
