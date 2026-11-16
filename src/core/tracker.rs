// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! The tracker's records (plan section 5, 'Data model'): messages holding one request per
//! lever, each with its append-only events. State format 2 stores exactly these types, so a
//! change to any of them is a change of format. Nothing that can be worked out is stored: a
//! request's status is its latest status event in effect, and a mistake is put right by a
//! record that withdraws it, never by editing. No I/O.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::core::deadline::{self, Due};
use crate::core::law::{self, Deadline};
use crate::core::letters;
use crate::core::profile::Profile;
use crate::core::register::{Channel, ChannelKind, Entry, Lever};

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
    pub drafted: NaiveDate,
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
    pub on: NaiveDate,
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
    pub on: NaiveDate,
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
    pub on: NaiveDate,
    /// The last day of the longer period. The latest extension in effect is the one that
    /// counts, so a later one can shorten an earlier one.
    pub until: NaiveDate,
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
    fn first_date(&self) -> Option<NaiveDate> {
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
    pub fn record_sent(
        &mut self,
        send: Send,
        correct: bool,
        today: NaiveDate,
    ) -> Result<bool, String> {
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

    /// The requests `levers` names, each once, or every request when it names none.
    fn targets(&self, levers: &[Lever]) -> Result<Vec<usize>, String> {
        if levers.is_empty() {
            return Ok((0..self.requests.len()).collect());
        }
        let mut targets = Vec::new();
        for lever in levers {
            let at = self.requests.iter().position(|r| r.lever == *lever);
            let at = at.ok_or_else(|| format!("{} holds no {} request", self.tag, lever.name()))?;
            if !targets.contains(&at) {
                targets.push(at);
            }
        }
        Ok(targets)
    }

    /// Record an answer, or a step the user took, on the requests `levers` names, or on
    /// every request: all of them or none. `Ok(false)` means it was already the latest
    /// status in effect on each, so nothing changed. `today` stops a date in the future.
    pub fn record_status(
        &mut self,
        levers: &[Lever],
        event: StatusEvent,
        today: NaiveDate,
    ) -> Result<bool, String> {
        let tag = &self.tag;
        if event.on > today {
            return Err(format!("{} is in the future", event.on));
        }
        match self.sent() {
            None if event.status != Status::Abandoned => {
                return Err(format!(
                    "{tag} is not recorded as sent, so it can only be given up (abandoned); \
                     record sending it first with `ghostsoap sent {tag}`"
                ));
            }
            Some(send) if event.on < send.on => {
                return Err(format!("{tag} was sent on {}, after {}", send.on, event.on));
            }
            None if event.on < self.drafted => {
                return Err(format!(
                    "{tag} was drafted on {}, after {}",
                    self.drafted, event.on
                ));
            }
            _ => {}
        }
        if event.reason.is_some() && !is_refusal(event.status) {
            return Err(
                "a reason goes only with refused, refused-with-reasons or exempt-claimed"
                    .to_owned(),
            );
        }
        let targets = self.targets(levers)?;
        if event.source.is_some() && targets.len() != 1 {
            return Err(format!(
                "{tag} holds more than one request; name the one the source was given for with --lever"
            ));
        }
        let mut new = Vec::new();
        for at in targets {
            let request = &self.requests[at];
            let lever = request.lever.name();
            if request
                .allows(&event)
                .map_err(|why| format!("{tag} {lever} {why}"))?
            {
                new.push(at);
            }
        }
        for &at in &new {
            self.requests[at].events.push(Event::Status(event.clone()));
        }
        Ok(!new.is_empty())
    }

    /// Record a longer period the user agreed to in writing, which moves the statutory date
    /// of the requests `levers` names, or of every request (ss 20T(2)(b), 21V(2)(b)).
    /// `Ok(false)` means it was already the longer period in effect on each.
    pub fn record_extension(
        &mut self,
        levers: &[Lever],
        extension: Extension,
        today: NaiveDate,
    ) -> Result<bool, String> {
        let tag = &self.tag;
        if extension.on > today {
            return Err(format!("{} is in the future", extension.on));
        }
        let send = self
            .sent()
            .ok_or_else(|| format!("{tag} is not recorded as sent"))?;
        if extension.on < send.on {
            return Err(format!(
                "{tag} was sent on {}, after {}",
                send.on, extension.on
            ));
        }
        let mut new = Vec::new();
        for at in self.targets(levers)? {
            let request = &self.requests[at];
            let lever = request.lever.name();
            // Only ss 20T(2)(b) and 21V(2)(b) allow a longer period, and both count from
            // the day the request is made.
            let last = match request.deadline {
                Deadline::DaysStarting(_) => deadline::last_day(request.deadline, send.on),
                _ => None,
            };
            let Some(last) = last else {
                return Err(format!(
                    "{tag} {lever} has no period the law lets you agree to lengthen"
                ));
            };
            if extension.until <= last {
                return Err(format!(
                    "{tag} {lever}: {} is not later than the law's last day, {last}",
                    extension.until
                ));
            }
            if request.in_effect().extensions.last() != Some(&&extension) {
                new.push(at);
            }
        }
        for &at in &new {
            let event = Event::Extension(extension.clone());
            self.requests[at].events.push(event);
        }
        Ok(!new.is_empty())
    }

    /// Record that the latest status, or the latest longer period, in effect on the requests
    /// `levers` names, or on every request, was recorded wrongly. It stays as history.
    pub fn withdraw_event(
        &mut self,
        levers: &[Lever],
        of: Withdrawn,
        note: Option<String>,
    ) -> Result<(), String> {
        let targets = self.targets(levers)?;
        for &at in &targets {
            let request = &self.requests[at];
            let kept = request.in_effect();
            let (none, what) = match of {
                Withdrawn::Status => (kept.statuses.is_empty(), "answer"),
                Withdrawn::Extension => (kept.extensions.is_empty(), "longer period"),
            };
            if none {
                let lever = request.lever.name();
                return Err(format!("{} {lever} has no {what} to withdraw", self.tag));
            }
        }
        for at in targets {
            let withdrawal = Withdrawal {
                of,
                note: note.clone(),
            };
            self.requests[at].events.push(Event::Withdrawal(withdrawal));
        }
        Ok(())
    }

    /// The earliest day an event in effect on any request is dated.
    fn first_date_in_effect(&self) -> Option<NaiveDate> {
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

/// An answer from the entity, as `closed` needs and `abandoned` must not have.
fn is_answer(status: Status) -> bool {
    !matches!(
        status,
        Status::Drafted
            | Status::Sent
            | Status::Bounced
            | Status::Escalated
            | Status::Closed
            | Status::Abandoned
    )
}

/// What the user should do about a request now, in the order `status` lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    /// Sent or acknowledged, and its statutory date or OAIC guide date has passed.
    Overdue,
    /// The user must send, answer, complain or close it.
    Needed,
    Waiting,
    Done,
    Abandoned,
}

impl Request {
    /// The request's status: the latest status in effect, else `sent` or `drafted` from
    /// whether its message is recorded as sent.
    pub fn status(&self, sent: bool) -> Status {
        match self.in_effect().statuses.last() {
            Some(latest) => latest.status,
            None if sent => Status::Sent,
            None => Status::Drafted,
        }
    }

    /// Whether `event` may be recorded next. `Ok(false)` means it is already the latest
    /// status in effect, so it is recorded once. The reason reads after the tag and lever.
    fn allows(&self, event: &StatusEvent) -> Result<bool, String> {
        let kept = self.in_effect();
        let latest = kept.statuses.last().copied();
        if latest == Some(event) {
            return Ok(false);
        }
        if let Some(latest) = latest
            && event.on < latest.on
        {
            return Err(format!(
                "has an answer recorded on {}, after {}",
                latest.on, event.on
            ));
        }
        let had = |statuses: &[Status]| kept.statuses.iter().any(|s| statuses.contains(&s.status));
        let answered = kept.statuses.iter().any(|s| is_answer(s.status));
        let bounced = latest.is_some_and(|l| l.status == Status::Bounced);
        let why = match event.status {
            _ if bounced && event.status != Status::Abandoned => {
                "bounced, so it can only be given up (abandoned); draft it again to send it"
            }
            Status::Closed if !answered => "has no answer to close it with; record it as abandoned",
            Status::Abandoned if answered => "has an answer, so record it as closed",
            Status::Relisted if !had(&[Status::Fulfilled, Status::Suppressed, Status::Partial]) => {
                "was never recorded as acted on, so it cannot be relisted"
            }
            _ => return Ok(true),
        };
        Err(why.to_owned())
    }

    /// Which action state the request is in on `today`, with `due` its deadline.
    pub fn action(&self, sent: bool, due: Due, today: NaiveDate) -> Action {
        let status = self.status(sent);
        let passed = match due {
            Due::Statutory(day) | Due::Guide(day) => day < today,
            Due::Unknown | Due::None => false,
        };
        // Only the 3 stop-using requests ask for exactly what `suppressed` reports.
        let stop_using = matches!(
            self.lever,
            Lever::App7NoMarketing | Lever::App7NoFacilitation | Lever::SpamUnsubscribe
        );
        match status {
            Status::Sent | Status::Acknowledged if passed => Action::Overdue,
            Status::Sent | Status::Acknowledged | Status::Escalated => Action::Waiting,
            Status::Fulfilled | Status::NoRecord | Status::Closed => Action::Done,
            Status::Suppressed if stop_using => Action::Done,
            Status::Abandoned => Action::Abandoned,
            _ => Action::Needed,
        }
    }

    /// What is wrong with this request's events, given the message's record of sending in
    /// effect. Each reads after the tag and lever.
    fn problems(&self, sent: Option<Send>) -> Vec<&'static str> {
        let mut problems = Vec::new();
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
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{embedded, register};

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

    fn note(text: &str) -> Option<String> {
        Some(text.to_owned())
    }

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

    #[test]
    fn a_new_request_keeps_the_law_tables_deadline_and_the_letters_version() {
        for lever in Lever::ALL {
            let request = Request::new(lever);
            let name = lever.name();
            assert_eq!(request.deadline, law::law(lever).deadline, "{name}");
            assert_eq!(request.letter_version, letters::letter_version(lever));
            assert!(request.events.is_empty());
        }
        assert_eq!(
            Request::new(Lever::S20rAccess).deadline,
            Deadline::DaysAfter(10)
        );
    }

    #[test]
    fn a_withdrawal_voids_the_latest_event_of_its_kind_still_in_effect() {
        let extension = |until| Extension {
            on: day(14),
            until: day(until),
            note: None,
        };
        let mut request = request(Lever::S20tCorrection, 1, Deadline::DaysStarting(30));
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
        assert!(next.unwrap_err().ends_with(
            "bounced, so it can only be given up (abandoned); draft it again to send it"
        ));
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
}
