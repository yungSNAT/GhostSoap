// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! The tracker's records (plan section 5, 'Data model'): messages holding one request per
//! lever, each with its append-only events. State format 2 stores exactly these types, so a
//! change to any of them is a change of format. Nothing that can be worked out is stored: a
//! request's status is its latest status event in effect, and a mistake is put right by a
//! record that withdraws it, never by editing. No I/O.

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::core::checklist::Mark;
use crate::core::law::{self, Deadline};
use crate::core::letters;
use crate::core::profile::Profile;
use crate::core::register::{Channel, ChannelKind, Entry, Lever};

mod reply;

pub use reply::Action;

/// The state format these types are written as. Format 1 held the profile alone.
pub const FORMAT: u32 = 2;

/// Everything GhostSoap keeps: the profile and every message drafted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub format: u32,
    pub profile: Profile,
    /// Oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<Message>,
    /// The user's checklist marks, one per item at most.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checklist: Vec<Mark>,
}

/// One letter to one entry through one channel, carrying one request per lever.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    /// Such as `GS-7K3QX`: no 2 messages share one, and a draft replaced before it is sent
    /// passes its tag to the letter that replaces it.
    pub tag: String,
    /// The register entry's id.
    pub entry: String,
    /// The channel the letter was drafted for.
    pub channel: ChannelKind,
    /// That channel's address as the register gave it when the letter was drafted.
    pub address: String,
    pub drafted: Date,
    /// Where the draft file went; absent for `draft --copy`, which writes none.
    pub draft: Option<DraftFile>,
    /// The draft has been replaced under a new tag and remains as history.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub superseded: bool,
    /// Saved by `draft` before it writes or shows the letter, and cleared once every letter
    /// of that run is handed over. A message still marked was drafted by a run that failed
    /// or stopped, so the letter the user holds under its tag may be an earlier one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unfinished: bool,
    /// What the user recorded about sending, oldest first; empty until they run `sent`. The
    /// last record decides: the message is sent as that record says, or not sent if it
    /// withdraws the one before. Earlier records stay as history.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sends: Vec<SendRecord>,
    /// One per lever, in the letter's order; never empty, and no lever twice.
    pub requests: Vec<Request>,
}

/// A draft file GhostSoap wrote, so a redraft can find it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftFile {
    pub folder: String,
    pub name: String,
}

/// One entry in a message's record of sending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SendRecord {
    /// The user sent the message.
    Sent(Send),
    /// The record before this one was wrong: the message was not sent.
    Withdrawn { note: Option<String> },
}

/// The user's record that they sent a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Send {
    pub on: Date,
    /// The channel actually used, which may differ from the one drafted for.
    pub channel: ChannelKind,
}

/// One lever's request within a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub lever: Lever,
    /// The version of the lever's letter that was drafted, so the record says what was sent.
    pub letter_version: u32,
    /// The lever's deadline kind in the law table when the letter was drafted. The request
    /// keeps it, so a later change to the table does not move this request's dates.
    pub deadline: Deadline,
    /// What happened after sending, in the order recorded; never edited or removed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
}

/// One entry in a request's history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Event {
    /// An answer from the entity or a step by the user that sets the request's status.
    Status(StatusEvent),
    /// A longer period the user agreed to in writing, which moves a statutory due date
    /// (Privacy Act ss 20T(2), 21V(2)). It does not change the status.
    Extension(Extension),
    /// An earlier event was recorded wrongly and no longer counts. It stays in the list.
    Withdrawal(Withdrawal),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusEvent {
    /// The day it happened, as the user gave it.
    pub on: Date,
    /// Never `drafted` or `sent`, which `draft` and `sent` set.
    pub status: Status,
    /// The ground the entity gave, where it is one of the listed ones; only with a refusal
    /// or a claim that the law does not apply.
    pub reason: Option<Reason>,
    /// An upstream source the answer names (APP 7.6(e)), in the user's words.
    pub source: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extension {
    /// The day the user agreed.
    pub on: Date,
    /// The last day of the longer period. The latest extension in effect is the one that
    /// counts, so a later one can shorten an earlier one.
    pub until: Date,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Withdrawal {
    /// Which kind of event it withdraws: the latest of that kind still in effect.
    pub of: Withdrawn,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Withdrawn {
    Status,
    Extension,
}

/// Where a request stands. `overdue` is not a status: it is worked out from the deadline and
/// the date each time, never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Recorded by `draft`; nothing has been sent.
    Drafted,
    /// Recorded by `sent`; no answer yet.
    Sent,
    /// The entity says it received the request and has not yet answered it.
    Acknowledged,
    /// The entity asked for proof of identity before acting.
    IdRequested,
    /// The entity quoted a charge before acting.
    ChargeQuoted,
    /// The entity says it did what the request asked.
    Fulfilled,
    /// The entity did part of what the request asked, such as access with records missing.
    Partial,
    /// The entity says it stopped using or disclosing the information, and has not said it
    /// destroyed or de-identified it. Suppression is not erasure.
    Suppressed,
    /// The entity says it holds no information about the user.
    NoRecord,
    /// The entity refused and gave written reasons.
    RefusedWithReasons,
    /// The entity refused without written reasons.
    Refused,
    /// The entity says the law, or the provision, does not apply to it.
    ExemptClaimed,
    /// The message came back undelivered, so the entity never received the request.
    Bounced,
    /// After the entity acted, the information is in use or on show again.
    Relisted,
    /// The user complained: to the entity, an external dispute resolution scheme or a regulator.
    Escalated,
    /// The user finished with the request and has an answer.
    Closed,
    /// The user gave up on the request without an answer.
    Abandoned,
}

/// The ground an entity gave for refusing or for saying the law does not apply. The last 3
/// follow the Data Rights Protocol's `outside_jurisdiction`, `insuf_verification` and `other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    /// It does not use or disclose the information for direct marketing, so APP 7 gives
    /// nothing to stop (plan section 3).
    NotDirectMarketing,
    /// It is a small business operator (Privacy Act s 6D).
    SmallBusiness,
    /// Australian privacy law does not reach it.
    OutsideJurisdiction,
    /// It could not verify the user's identity.
    InsufficientVerification,
    /// Any other ground; the note says what.
    Other,
}

/// A request's events that still count, oldest first.
#[derive(Debug, PartialEq, Eq)]
pub struct InEffect<'a> {
    pub statuses: Vec<&'a StatusEvent>,
    pub extensions: Vec<&'a Extension>,
}

impl InEffect<'_> {
    pub fn is_empty(&self) -> bool {
        self.statuses.is_empty() && self.extensions.is_empty()
    }

    /// The earliest day any of them is dated.
    fn first_date(&self) -> Option<Date> {
        let statuses = self.statuses.iter().map(|s| s.on);
        statuses.chain(self.extensions.iter().map(|e| e.on)).min()
    }
}

impl Request {
    /// The request `lever` makes in a letter drafted now.
    pub fn new(lever: Lever) -> Self {
        Self {
            lever,
            letter_version: letters::letter_version(lever),
            deadline: law::law(lever).deadline,
            events: Vec::new(),
        }
    }

    /// The events in effect: each withdrawal voids the latest status event, or the latest
    /// extension, still in effect before it.
    pub fn in_effect(&self) -> InEffect<'_> {
        self.replay().0
    }

    /// `in_effect`, and whether every withdrawal had an event to void.
    fn replay(&self) -> (InEffect<'_>, bool) {
        let mut kept = InEffect {
            statuses: Vec::new(),
            extensions: Vec::new(),
        };
        let mut sound = true;
        for event in &self.events {
            match event {
                Event::Status(status) => kept.statuses.push(status),
                Event::Extension(extension) => kept.extensions.push(extension),
                Event::Withdrawal(withdrawal) => {
                    sound &= match withdrawal.of {
                        Withdrawn::Status => kept.statuses.pop().is_some(),
                        Withdrawn::Extension => kept.extensions.pop().is_some(),
                    };
                }
            }
        }
        (kept, sound)
    }
}

impl Message {
    /// The record of sending in effect: none before `sent`, or once that record is withdrawn.
    pub fn sent(&self) -> Option<Send> {
        match self.sends.last() {
            Some(SendRecord::Sent(send)) => Some(*send),
            _ => None,
        }
    }

    /// Whether a new draft may take this message's place.
    pub fn is_replaceable(&self) -> bool {
        !self.superseded
            && self.sent().is_none()
            && self.requests.iter().all(|r| r.in_effect().is_empty())
    }

    /// Record that the message was sent. `Ok(false)` means this record was already the one
    /// in effect, so nothing changed. A different record needs `correct`, and is then kept
    /// after the earlier one. `today` stops a date in the future.
    pub fn record_sent(&mut self, send: Send, correct: bool, today: Date) -> Result<bool, String> {
        let tag = &self.tag;
        if self.unfinished {
            return Err(format!(
                "the draft run that recorded {tag} did not finish, so the letter you hold under \
                 that tag may not be the one recorded; draft it again and send that letter"
            ));
        }
        if send.on > today {
            return Err(format!("{} is in the future", send.on));
        }
        if send.on < self.drafted {
            return Err(format!(
                "{tag} was drafted on {}, after {}; if you sent an earlier draft, its requests \
                 may differ from this one's, so check what you sent",
                self.drafted, send.on
            ));
        }
        match self.sent() {
            Some(current) if current == send => return Ok(false),
            Some(current) if !correct => {
                return Err(format!(
                    "{tag} is already recorded as sent on {} by {}; add --correct to replace \
                     that record",
                    current.on,
                    current.channel.label()
                ));
            }
            Some(_) => {
                if let Some(first) = self.first_date_in_effect()
                    && send.on > first
                {
                    return Err(format!(
                        "{tag} has an answer recorded on {first}, before {}",
                        send.on
                    ));
                }
            }
            None if correct => return Err(format!("{tag} has no record of sending to correct")),
            // Only giving a draft up leaves an event in effect on a message never sent.
            None if self.requests.iter().any(|r| !r.in_effect().is_empty()) => {
                return Err(format!(
                    "{tag} was given up as a draft; draft the letter again"
                ));
            }
            None => {}
        }
        self.sends.push(SendRecord::Sent(send));
        Ok(true)
    }

    /// Record that the message was not sent after all: the record of sending in effect was
    /// wrong. It stays as history. Refused once an answer is in effect, since an answer
    /// means the message arrived.
    pub fn withdraw_sent(&mut self, note: Option<String>) -> Result<(), String> {
        let tag = &self.tag;
        if self.sent().is_none() {
            return Err(format!("{tag} is not recorded as sent"));
        }
        if self.requests.iter().any(|r| !r.in_effect().is_empty()) {
            return Err(format!(
                "{tag} has an answer recorded, so its record of sending cannot be withdrawn"
            ));
        }
        self.sends.push(SendRecord::Withdrawn { note });
        Ok(())
    }

    /// The earliest day an event in effect on any request is dated.
    fn first_date_in_effect(&self) -> Option<Date> {
        let dates = self.requests.iter().map(|r| r.in_effect().first_date());
        dates.flatten().min()
    }

    /// What is wrong with this message's records, for `State::problems`.
    fn problems(&self, problems: &mut Vec<String>) {
        let tag = &self.tag;
        if self.requests.is_empty() {
            problems.push(format!("{tag} holds no request"));
        }
        let mut sent = false;
        for record in &self.sends {
            match record {
                SendRecord::Sent(send) if send.on < self.drafted => {
                    problems.push(format!("{tag} is recorded as sent before it was drafted"));
                }
                SendRecord::Withdrawn { .. } if !sent => {
                    problems.push(format!("{tag} withdraws a record of sending it lacks"));
                }
                _ => {}
            }
            sent = matches!(record, SendRecord::Sent(_));
        }
        for (at, request) in self.requests.iter().enumerate() {
            let lever = request.lever.name();
            if self.requests[..at].iter().any(|r| r.lever == request.lever) {
                problems.push(format!("{tag} holds {lever} twice"));
            }
            for problem in request.problems(self.sent()) {
                problems.push(format!("{tag} {lever} {problem}"));
            }
        }
    }
}

/// A refusal, or a claim that the law does not apply: the statuses a reason goes with.
fn is_refusal(status: Status) -> bool {
    matches!(
        status,
        Status::RefusedWithReasons | Status::Refused | Status::ExemptClaimed
    )
}

impl Request {
    /// What is wrong with this request's deadline and events, given the message's record of
    /// sending in effect. Each reads after the tag and lever.
    fn problems(&self, sent: Option<Send>) -> Vec<&'static str> {
        let mut problems = Vec::new();
        let kind = std::mem::discriminant(&law::law(self.lever).deadline);
        if std::mem::discriminant(&self.deadline) != kind {
            problems.push("has a deadline kind its lever's law does not give");
        }
        for event in &self.events {
            match event {
                Event::Status(status) => {
                    if matches!(status.status, Status::Drafted | Status::Sent) {
                        problems.push("has an event that sets drafted or sent");
                    }
                    if status.reason.is_some() && !is_refusal(status.status) {
                        problems.push("gives a reason for an answer that is not a refusal");
                    }
                }
                Event::Extension(extension) if extension.until < extension.on => {
                    problems.push("has a longer period that ends before it was agreed");
                }
                Event::Extension(_) | Event::Withdrawal(_) => {}
            }
        }
        let (kept, sound) = self.replay();
        if !sound {
            problems.push("withdraws an event it lacks");
        }
        match sent {
            Some(sent) if kept.first_date().is_some_and(|first| first < sent.on) => {
                problems.push("has an event dated before the message was sent");
            }
            Some(_) => {}
            // A draft can be given up; nothing else can happen to a message never sent.
            None => {
                let given_up = |s: &&StatusEvent| s.status == Status::Abandoned;
                if !kept.extensions.is_empty() || !kept.statuses.iter().all(given_up) {
                    problems.push("has an answer, but the message is not recorded as sent");
                }
            }
        }
        problems
    }
}

impl State {
    pub fn new(profile: Profile) -> Self {
        Self {
            format: FORMAT,
            profile,
            messages: Vec::new(),
            checklist: Vec::new(),
        }
    }

    pub fn message(&self, tag: &str) -> Option<&Message> {
        self.messages.iter().find(|m| m.tag == tag)
    }

    pub fn message_mut(&mut self, tag: &str) -> Option<&mut Message> {
        self.messages.iter_mut().find(|m| m.tag == tag)
    }

    /// The draft a new letter to `entry` through `channel` takes the place of, so 2 drafts
    /// never leave 2 messages to send: the replaceable message to that address, else one to
    /// an address `entry` no longer lists for that kind of channel. `claimed` holds the tags
    /// other letters in the same run have taken.
    pub fn draft_to_replace(
        &self,
        entry: &Entry,
        channel: &Channel,
        claimed: &[String],
    ) -> Option<&Message> {
        let listed = |m: &Message| {
            let same = |c: &Channel| c.kind == m.channel && c.address == m.address;
            entry.channels.iter().any(same)
        };
        let open = || {
            self.messages.iter().rev().filter(|m| {
                m.entry == entry.id
                    && m.channel == channel.kind
                    && m.is_replaceable()
                    && !claimed.contains(&m.tag)
            })
        };
        open()
            .find(|m| m.address == channel.address)
            .or_else(|| open().find(|m| !listed(m)))
    }

    /// What is wrong with a state, or nothing. A state with a problem is neither loaded nor
    /// saved. It checks what the records mean and that none contradicts another, not which
    /// changes of status are allowed: a history an earlier version allowed still loads.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.format != FORMAT {
            problems.push(format!("format is {}, not {FORMAT}", self.format));
        }
        for (index, message) in self.messages.iter().enumerate() {
            let tag = &message.tag;
            if !letters::is_tag(tag) {
                problems.push(format!("message {} has no valid tag", index + 1));
            }
            if self.messages[..index].iter().any(|m| m.tag == *tag) {
                problems.push(format!("tag {tag} is used twice"));
            }
            message.problems(&mut problems);
        }
        for (index, mark) in self.checklist.iter().enumerate() {
            if self.checklist[..index].iter().any(|m| m.item == mark.item) {
                problems.push(format!("checklist item {} is marked twice", mark.item));
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests;
