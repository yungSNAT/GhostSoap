// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! Answers, longer periods and withdrawals recorded on a sent message, and what each
//! request asks of the user now.

use chrono::NaiveDate;

use crate::core::deadline::{self, Due};
use crate::core::law::Deadline;
use crate::core::register::Lever;

use super::{
    Event, Extension, Message, Request, Status, StatusEvent, Withdrawal, Withdrawn, is_refusal,
};

impl Message {
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
        fits(&event)?;
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
            // A period ending at a weekend already runs to the Monday (s 36(2)).
            let last = deadline::past_weekend(last);
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
        // A finished request takes no new status, except that information on show again
        // reopens a closed one.
        if let Some(finished) = latest.and_then(|l| match (l.status, event.status) {
            (Status::Closed, Status::Relisted) => None,
            (Status::Closed, _) => Some("closed"),
            (Status::Abandoned, _) => Some("abandoned"),
            _ => None,
        }) {
            return Err(format!(
                "is {finished}; to record another status, withdraw that one first \
                 (--withdraw status)"
            ));
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
}

/// Whether the fields `event` carries go with its status: a reason only with a refusal, and
/// where a complaint went with `escalated`, which needs it.
fn fits(event: &StatusEvent) -> Result<(), String> {
    if event.reason.is_some() && !is_refusal(event.status) {
        return Err(
            "a reason goes only with refused, refused-with-reasons or exempt-claimed".to_owned(),
        );
    }
    match (event.status, event.to) {
        (Status::Escalated, None) => {
            return Err(
                "say where you complained with --to business, afca, tio, acma or oaic".to_owned(),
            );
        }
        (Status::Escalated, Some(_)) | (_, None) => {}
        (_, Some(_)) => return Err("--to goes only with escalated".to_owned()),
    }
    Ok(())
}
