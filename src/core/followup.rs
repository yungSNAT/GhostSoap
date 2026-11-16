// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! Follow-up letters for a sent message (plan section 5, 'M3b, follow-up letters and the
//! complaint record'): a reminder whose wording follows each request's deadline, or the
//! complaint to the business that Privacy Act s 40(1A) generally needs before the OAIC.
//! Pure: no clock, no I/O, and nothing is recorded.

use std::collections::BTreeMap;

use chrono::NaiveDate;

use crate::core::deadline::{self, Due};
use crate::core::law::{Deadline, law};
use crate::core::letters::{self, Letter};
use crate::core::profile::Profile;
use crate::core::register::{Entry, IdentityField, Lever};
use crate::core::tracker::{Message, Request, Status};

const REMINDER: &str = include_str!("../../letters/au/reminder.txt");
const COMPLAINT: &str = include_str!("../../letters/au/complaint.txt");

/// Which follow-up to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Reminder,
    Complaint,
}

impl Kind {
    /// The word in the draft's file name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Reminder => "reminder",
            Self::Complaint => "complaint",
        }
    }
}

/// The follow-up of `kind` to `message`, sent to `entry`, for the requests `levers` names
/// or, with none named, every request it suits. A request it does not suit is refused if
/// named; with none left the letter is refused.
pub fn compose(
    kind: Kind,
    entry: &Entry,
    profile: &Profile,
    message: &Message,
    levers: &[Lever],
    today: NaiveDate,
) -> Result<Letter, String> {
    let tag = &message.tag;
    let sent = message
        .sent()
        .ok_or_else(|| format!("{tag} is not recorded as sent, so there is nothing to follow up"))?
        .on;
    let (paragraphs, passed) = paragraphs(kind, message, levers, sent, today)?;
    let escalation = letters::escalation(entry, false, false);
    let next = match (kind, passed, &escalation) {
        (Kind::Reminder, true, Some(to)) => Some(format!(
            "If I do not have your answer soon, my next step is a complaint to you, and then \
             to {to}."
        )),
        (Kind::Reminder, true, None) => Some(
            "If I do not have your answer soon, my next step is a complaint to you.".to_owned(),
        ),
        _ => None,
    };
    let values: BTreeMap<&str, Option<String>> = BTreeMap::from([
        ("date", Some(long_date(today))),
        ("sent", Some(long_date(sent))),
        ("legal_entity", Some(entry.legal_entity.clone())),
        ("postal_address", postal(message)),
        ("tag", Some(format!("[{tag}]"))),
        ("full_name", profile.value(IdentityField::FullName)),
        ("requests", Some(paragraphs.join("\n\n"))),
        ("next", next),
        ("escalation", escalation),
    ]);
    let template = match kind {
        Kind::Reminder => REMINDER,
        Kind::Complaint => COMPLAINT,
    };
    let body = letters::render(letters::parts(template).1, &values)?;
    let subject = match kind {
        Kind::Reminder => "Follow-up: Privacy Act request",
        Kind::Complaint => "Complaint: Privacy Act request",
    };
    Ok(Letter {
        kind: message.channel,
        to: message.address.clone(),
        subject: format!("{subject} [{tag}]"),
        body: letters::wrap(&letters::tidy(&body), 76),
    })
}

/// Each chosen request's paragraph, and whether any of their dates has passed.
fn paragraphs(
    kind: Kind,
    message: &Message,
    levers: &[Lever],
    sent: NaiveDate,
    today: NaiveDate,
) -> Result<(Vec<String>, bool), String> {
    let tag = &message.tag;
    let mut paragraphs = Vec::new();
    let mut passed = false;
    for request in &message.requests {
        let named = levers.contains(&request.lever);
        if !levers.is_empty() && !named {
            continue;
        }
        let due = deadline::due(message, request);
        let status = request.status(true);
        let paragraph = match kind {
            Kind::Reminder => reminder(request, due, status, sent, today),
            Kind::Complaint => complaint(request, due, status, today),
        };
        match paragraph {
            Ok(text) => {
                passed |= due_date(due).is_some_and(|day| day < today);
                paragraphs.push(text);
            }
            Err(why) if named => return Err(format!("{tag} {} {why}", request.lever.name())),
            Err(_) => {}
        }
    }
    if let Some(lever) = levers
        .iter()
        .find(|l| !message.requests.iter().any(|r| r.lever == **l))
    {
        return Err(format!("{tag} has no {} request", lever.name()));
    }
    if paragraphs.is_empty() {
        return Err(format!("{tag} has no request a {} suits", kind.name()));
    }
    Ok((paragraphs, passed))
}

/// The postal address a posted letter repeats; none for other channels.
fn postal(message: &Message) -> Option<String> {
    (message.channel == crate::core::register::ChannelKind::Post).then(|| message.address.clone())
}

fn long_date(day: NaiveDate) -> String {
    day.format("%-d %B %Y").to_string()
}

fn due_date(due: Due) -> Option<NaiveDate> {
    match due {
        Due::Statutory(day) | Due::Guide(day) => Some(day),
        Due::Unknown | Due::None => None,
    }
}

/// `request`'s heading: its title and provision.
fn heading(request: &Request) -> String {
    let row = law(request.lever);
    format!("{} ({}):", row.title, row.provision)
}

/// A reminder's paragraph for `request`, or why it gets none. Its wording follows the
/// deadline: a statutory date that has passed only 'appears' to have passed, because a
/// public holiday where the business acts would move it (plan section 5, 'Deadlines').
fn reminder(
    request: &Request,
    due: Due,
    status: Status,
    sent: NaiveDate,
    today: NaiveDate,
) -> Result<String, &'static str> {
    if matches!(request.deadline, Deadline::BusinessDays(_)) || request.lever == Lever::S20kBan {
        return Err("takes no reply, so it gets no reminder");
    }
    if !matches!(status, Status::Sent | Status::Acknowledged) {
        return Err("is not waiting for an answer, so it gets no reminder");
    }
    let heading = heading(request);
    let days = (today - sent).num_days();
    let made = format!(
        "I made this request on {}, {days} days ago.",
        long_date(sent)
    );
    let text = match due {
        Due::Statutory(day) | Due::Guide(day) if day >= today => {
            format!("{heading} please answer by {}.", long_date(day))
        }
        Due::Statutory(day) => {
            let period = if request.in_effect().extensions.is_empty() {
                "the period the law sets"
            } else {
                "the longer period we agreed in writing"
            };
            format!(
                "{heading} {made} By my count {period} ended on {}, so it appears to have \
                 passed; a public holiday where you must act would move that day later. \
                 Please answer now.",
                long_date(day)
            )
        }
        Due::Guide(day) => format!(
            "{heading} {made} The period the OAIC's APP guidelines give as generally \
             reasonable ended on {}. Please answer as soon as you can.",
            long_date(day)
        ),
        Due::Unknown | Due::None => {
            format!("{heading} {made} I have not yet had your answer. Please answer.")
        }
    };
    Ok(text)
}

/// A complaint's paragraph for `request`, or why it has none: what went wrong, and that a
/// complaint about a credit reporting request need not come to the business first.
fn complaint(
    request: &Request,
    due: Due,
    status: Status,
    today: NaiveDate,
) -> Result<String, &'static str> {
    let what = match (status, due) {
        (Status::Sent | Status::Acknowledged, Due::Statutory(day)) if day < today => format!(
            "I have had no answer, and by my count the period for it appears to have ended on \
             {}; a public holiday where you must act would move that day later.",
            long_date(day)
        ),
        (Status::Sent | Status::Acknowledged, Due::Guide(day)) if day < today => format!(
            "I have had no answer, and the period the OAIC's APP guidelines give as generally \
             reasonable ended on {}.",
            long_date(day)
        ),
        (Status::Refused | Status::RefusedWithReasons, _) => "You refused it.".to_owned(),
        (Status::Partial, _) => "You dealt with only part of it.".to_owned(),
        (Status::ChargeQuoted, _) => "You asked for a charge before acting on it.".to_owned(),
        (Status::IdRequested, _) => {
            "You asked for proof of identity before acting on it.".to_owned()
        }
        (Status::ExemptClaimed, _) => "You said the law does not apply to you.".to_owned(),
        _ => return Err("has nothing recorded to complain about"),
    };
    let direct = matches!(
        request.lever,
        Lever::S20rAccess | Lever::S20tCorrection | Lever::S21tAccess | Lever::S21vCorrection
    )
    .then_some(
        " A complaint about this request may go to the Information Commissioner without \
         first being made to you (section 40(1B)).",
    )
    .unwrap_or_default();
    Ok(format!(
        "{} {what} I still ask you to deal with it as my letter set out.{direct}",
        heading(request)
    ))
}

#[cfg(test)]
mod tests;
