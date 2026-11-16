// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! When an answer is due (plan section 5, 'Deadlines'): pure functions of the records and
//! an as-of date. No date is stored.
//!
//! A statutory period is counted as the Acts Interpretation Act 1901 s 36 directs
//! (compilation C2026C00117, in force from 28 March 2026, read 16 November 2026). A period
//! that begins after a day does not include it (s 36(1) item 6); one that starts on a day
//! includes it (item 2). If the last day is a Saturday, a Sunday or a holiday, the thing
//! may be done on the next day that is none of them (s 36(2)). A holiday there is a public
//! holiday in the place the thing is to be done, which GhostSoap does not know, so only
//! weekends move a date here, and every statutory date is shown with that limit.

use chrono::{Datelike, Days, NaiveDate, Weekday};

use crate::core::law::Deadline;
use crate::core::register::Lever;
use crate::core::tracker::{Message, Request, Status};

/// What a request's deadline gives as of now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    /// The last day the law allows, moved past a weekend; a public holiday where the entity
    /// acts can move it further.
    Statutory(NaiveDate),
    /// The OAIC's general guide to a reasonable period. Passing it is not a breach.
    Guide(NaiveDate),
    /// The law sets a period GhostSoap cannot date: the Spam Act's business days depend on
    /// public holidays where the message was sent.
    Unknown,
    /// No date: the law sets none, or the period has not started.
    None,
}

/// The last day of a statutory period of `deadline` that runs from `start`, before s 36(2)
/// moves it past a weekend or holiday; none for a deadline that is not a number of days.
pub fn last_day(deadline: Deadline, start: NaiveDate) -> Option<NaiveDate> {
    match deadline {
        Deadline::DaysAfter(days) => start.checked_add_days(Days::new(days.into())),
        Deadline::DaysStarting(days) => {
            start.checked_add_days(Days::new(u64::from(days).saturating_sub(1)))
        }
        _ => None,
    }
}

/// `day`, or the Monday after it if it is a Saturday or a Sunday (s 36(2)).
pub fn past_weekend(day: NaiveDate) -> NaiveDate {
    let skip = match day.weekday() {
        Weekday::Sat => 2,
        Weekday::Sun => 1,
        _ => 0,
    };
    day + Days::new(skip)
}

/// When `request`, one of `message`'s, is due. Its period starts on the send date in effect,
/// except APP 13.4's statement, whose period starts at the latest refusal in effect on the
/// message's APP 13 correction, unless a later answer there says the correction was made.
/// A statutory date is the latest longer period in effect that the user agreed to, if any.
pub fn due(message: &Message, request: &Request) -> Due {
    let start = if request.lever == Lever::App13Statement {
        correction_refused(message)
    } else {
        message.sent().map(|send| send.on)
    };
    let Some(start) = start else {
        return Due::None;
    };
    match request.deadline {
        Deadline::DaysAfter(_) | Deadline::DaysStarting(_) => {
            let agreed = request.in_effect().extensions.last().map(|e| e.until);
            agreed
                .or_else(|| last_day(request.deadline, start))
                .map_or(Due::None, |day| Due::Statutory(past_weekend(day)))
        }
        Deadline::Reasonable {
            guide_days: Some(days),
        } => Due::Guide(start + Days::new(days.into())),
        Deadline::BusinessDays(_) => Due::Unknown,
        Deadline::Reasonable { guide_days: None } | Deadline::NotStated => Due::None,
    }
}

/// The day of the latest refusal in effect on `message`'s APP 13 correction, unless a later
/// status in effect there says it was made. A later step such as a complaint leaves the
/// refusal standing.
fn correction_refused(message: &Message) -> Option<NaiveDate> {
    let correction = message.requests.iter().find(|r| r.lever == Lever::App13)?;
    let mut refused = None;
    for event in correction.in_effect().statuses {
        match event.status {
            Status::Refused | Status::RefusedWithReasons => refused = Some(event.on),
            Status::Fulfilled => refused = None,
            _ => {}
        }
    }
    refused
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::register::ChannelKind;
    use crate::core::tracker::{Event, Extension, Send, SendRecord, StatusEvent};

    fn day(month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, month, day).unwrap()
    }

    fn answer(on: NaiveDate, status: Status) -> Event {
        Event::Status(StatusEvent {
            on,
            status,
            reason: None,
            source: None,
            note: None,
        })
    }

    /// A letter drafted on 31 October carrying `levers`, sent on `sent` if given.
    fn letter(levers: &[Lever], sent: Option<NaiveDate>) -> Message {
        Message {
            tag: "GS-7K3QX".to_owned(),
            entry: "equifax-au".to_owned(),
            channel: ChannelKind::Email,
            address: "privacy@example.com.au".to_owned(),
            drafted: day(10, 31),
            draft: None,
            superseded: false,
            unfinished: false,
            sends: sent
                .map(|on| {
                    SendRecord::Sent(Send {
                        on,
                        channel: ChannelKind::Email,
                    })
                })
                .into_iter()
                .collect(),
            requests: levers.iter().map(|&lever| Request::new(lever)).collect(),
        }
    }

    fn due_of(message: &Message, lever: Lever) -> Due {
        let request = message.requests.iter().find(|r| r.lever == lever).unwrap();
        due(message, request)
    }

    #[test]
    fn statutory_periods_count_as_section_36_directs() {
        // Monday 2 November: s 20R(3)'s 10 days begin after it (item 6) and end on
        // Thursday 12 November; s 20T(2)'s 30 days start on it (item 2) and end on Tuesday
        // 1 December.
        assert_eq!(day(11, 2).weekday(), Weekday::Mon);
        assert_eq!(
            last_day(Deadline::DaysAfter(10), day(11, 2)),
            Some(day(11, 12))
        );
        assert_eq!(
            last_day(Deadline::DaysStarting(30), day(11, 2)),
            Some(day(12, 1))
        );
        for none in [
            Deadline::NotStated,
            Deadline::BusinessDays(5),
            Deadline::Reasonable {
                guide_days: Some(30),
            },
        ] {
            assert_eq!(last_day(none, day(11, 2)), None, "{none:?}");
        }
        // A last day on a weekend moves to the Monday (s 36(2)); a weekday stays.
        assert_eq!(day(11, 14).weekday(), Weekday::Sat);
        assert_eq!(past_weekend(day(11, 14)), day(11, 16));
        assert_eq!(past_weekend(day(11, 15)), day(11, 16));
        assert_eq!(past_weekend(day(11, 16)), day(11, 16));
        assert_eq!(past_weekend(day(11, 20)), day(11, 20));
    }

    #[test]
    fn each_kind_of_deadline_gives_its_date_from_the_send_date() {
        let levers = [
            Lever::S20rAccess,
            Lever::S21vCorrection,
            Lever::App12,
            Lever::S21tAccess,
            Lever::App11Destroy,
            Lever::SpamUnsubscribe,
        ];
        let unsent = letter(&levers, None);
        for lever in levers {
            assert_eq!(
                due_of(&unsent, lever),
                Due::None,
                "{lever:?} before sending"
            );
        }
        // Sent Wednesday 4 November: 20R's last day is Saturday 14 November.
        let mut sent = letter(&levers, Some(day(11, 4)));
        assert_eq!(
            due_of(&sent, Lever::S20rAccess),
            Due::Statutory(day(11, 16))
        );
        assert_eq!(
            due_of(&sent, Lever::S21vCorrection),
            Due::Statutory(day(12, 3))
        );
        assert_eq!(due_of(&sent, Lever::App12), Due::Guide(day(12, 4)));
        assert_eq!(due_of(&sent, Lever::S21tAccess), Due::None);
        assert_eq!(due_of(&sent, Lever::App11Destroy), Due::None);
        assert_eq!(due_of(&sent, Lever::SpamUnsubscribe), Due::Unknown);

        // An agreed longer period replaces the law's last day, moved past a weekend too.
        let correction = sent
            .requests
            .iter_mut()
            .find(|r| r.lever == Lever::S21vCorrection)
            .unwrap();
        correction.events.push(Event::Extension(Extension {
            on: day(11, 20),
            until: day(12, 19),
            note: None,
        }));
        assert_eq!(day(12, 19).weekday(), Weekday::Sat);
        assert_eq!(
            due_of(&sent, Lever::S21vCorrection),
            Due::Statutory(day(12, 21))
        );
    }

    #[test]
    fn the_app13_statement_period_starts_at_a_refusal_of_the_correction() {
        let mut message = letter(&[Lever::App13, Lever::App13Statement], Some(day(11, 2)));
        assert_eq!(due_of(&message, Lever::App13Statement), Due::None);
        message.requests[0]
            .events
            .push(answer(day(11, 9), Status::Acknowledged));
        assert_eq!(due_of(&message, Lever::App13Statement), Due::None);
        message.requests[0]
            .events
            .push(answer(day(11, 10), Status::Refused));
        assert_eq!(
            due_of(&message, Lever::App13Statement),
            Due::Guide(day(12, 10))
        );
        message.requests[0]
            .events
            .push(answer(day(11, 11), Status::Escalated));
        assert_eq!(
            due_of(&message, Lever::App13Statement),
            Due::Guide(day(12, 10)),
            "a complaint about the refusal leaves it in effect"
        );
        message.requests[0]
            .events
            .push(answer(day(11, 12), Status::Fulfilled));
        assert_eq!(
            due_of(&message, Lever::App13Statement),
            Due::None,
            "the correction was made"
        );
        let alone = letter(&[Lever::App13Statement], Some(day(11, 2)));
        assert_eq!(due_of(&alone, Lever::App13Statement), Due::None);
    }
}
