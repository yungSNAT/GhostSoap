// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! `sent`, `reply`, `status` and `checklist`: what was sent, what came back and what is due.

use super::*;
use crate::core::audit::{self, AuditKind};

pub(super) fn sent(args: &SentArgs) -> Result<(), String> {
    let today = today();
    let lines = update_state(
        args.passphrase_stdin,
        |state| {
            let mut lines = Vec::new();
            for typed in &args.tags {
                let (tag, entry, recorded, audit_event) = {
                    let message = message_for(state, typed)?;
                    let (recorded, audit_event) = record_sending(message, args, today)?;
                    (
                        message.tag.clone(),
                        message.entry.clone(),
                        recorded,
                        audit_event,
                    )
                };
                if let Some((kind, detail)) = audit_event {
                    audit::append(&mut state.audit, today, &tag, &entry, kind, detail);
                }
                lines.push(format!("sent    {tag}  {entry}  {recorded}"));
            }
            Ok(lines)
        },
        |_| {},
        |lines, _| Ok(lines),
    )?;
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

/// The message under the tag `typed`, as the user typed it.
pub(super) fn message_for<'a>(
    state: &'a mut State,
    typed: &str,
) -> Result<&'a mut Message, String> {
    let tag = letters::normalise_tag(typed);
    state.message_mut(&tag).ok_or_else(|| {
        format!("no letter is recorded under {tag}; the tag is in the letter's subject line")
    })
}

pub(super) fn reply(args: &ReplyArgs) -> Result<(), String> {
    let today = today();
    let on = args.on.unwrap_or(today);
    let line = update_state(
        args.passphrase_stdin,
        |state| {
            let (tag, entry, done, audit_event) = {
                let message = message_for(state, &args.tag)?;
                let (done, audit_event) = apply_reply(message, args, on, today)?;
                (
                    message.tag.clone(),
                    message.entry.clone(),
                    done,
                    audit_event,
                )
            };
            if let Some((kind, detail)) = audit_event {
                audit::append(&mut state.audit, today, &tag, &entry, kind, detail);
            }
            Ok(format!("reply   {tag}  {entry}  {done}"))
        },
        |_| {},
        |line, _| Ok(line),
    )?;
    println!("{line}");
    Ok(())
}

/// A link that opens a register issue about a bounced address, holding only the entry and
/// the channel, never the user's details. Nothing is sent: the user decides whether to file it.
pub(super) fn bounce_report(entry: &str, channel: ChannelKind) -> String {
    let title = format!("Bounced: {entry} by {}", channel.label());
    let body = format!(
        "A letter GhostSoap prepared for `{entry}` by {} came back undelivered, so the \
         register's address may be out of date.",
        channel.label()
    );
    format!(
        "\nThe register's address may be out of date. To tell its maintainers, open this \
         public issue and remove anything about you before submitting:\n\
         https://github.com/yungSNAT/GhostSoap/issues/new?title={}&body={}",
        letters::percent_encode(&title),
        letters::percent_encode(&body)
    )
}

pub(super) fn recorded(new: bool) -> &'static str {
    if new { "recorded" } else { "already recorded" }
}

/// What `status` calls each action state.
pub(super) fn action_name(action: tracker::Action) -> &'static str {
    match action {
        tracker::Action::Overdue => "overdue",
        tracker::Action::Needed => "action needed",
        tracker::Action::Waiting => "waiting",
        tracker::Action::Done => "done",
        tracker::Action::Abandoned => "abandoned",
    }
}

pub(super) const ACTIONS: [tracker::Action; 5] = [
    tracker::Action::Overdue,
    tracker::Action::Needed,
    tracker::Action::Waiting,
    tracker::Action::Done,
    tracker::Action::Abandoned,
];

/// The date a deadline gives, if any.
pub(super) fn due_date(due: Due) -> Option<Date> {
    match due {
        Due::Statutory(day) | Due::Guide(day) => Some(day),
        Due::Unknown | Due::None => None,
    }
}

/// What `status` says about a deadline.
pub(super) fn due_text(due: Due) -> String {
    match due {
        Due::Statutory(day) => format!("  law's last day {day}"),
        Due::Guide(day) => format!("  OAIC guide {day}"),
        Due::Unknown => {
            "  takes effect 5 business days after sending, counted where it was sent".to_owned()
        }
        Due::None => String::new(),
    }
}

/// List every request that is not history, by action state and then in the order drafted,
/// or with `due_only` only those with a date, soonest first. A state that cannot be read is
/// reported as DEGRADED with unknown counts, never as nothing to do.
pub(super) fn status(due_only: bool, passphrase_stdin: bool) -> Result<(), String> {
    let loaded = state::data_dir().and_then(|dir| {
        let passphrase = Input::new(passphrase_stdin).passphrase("Passphrase: ")?;
        state::load(&dir, passphrase)
    });
    let state = match loaded {
        Ok(state) => state,
        Err(e) => {
            println!("DEGRADED: {e}");
            println!("{}", counts(|_| "?".to_owned()));
            return Err("the state could not be read, so this is not an all-clear".to_owned());
        }
    };
    let today = today();
    let mut rows = Vec::new();
    for message in state.messages.iter().filter(|m| !m.superseded) {
        let sent = message.sent().is_some();
        for request in &message.requests {
            let due = deadline::due(message, request);
            rows.push((request.action(sent, due, today), due, message, request));
        }
    }
    println!(
        "{}",
        counts(|a| rows.iter().filter(|r| r.0 == a).count().to_string())
    );
    if rows.is_empty() {
        println!(
            "Nothing tracked yet: no letter has been drafted, so these counts are not an \
             all-clear. `ghostsoap plan` shows the letters it would draft."
        );
        return Ok(());
    }
    if due_only {
        rows.retain(|row| due_date(row.1).is_some());
        rows.sort_by_key(|row| due_date(row.1));
    } else {
        rows.sort_by_key(|row| row.0);
    }
    for &(action, due, message, request) in &rows {
        println!("{}", status_line(action, due, message, request));
    }
    if rows.iter().any(|row| matches!(row.1, Due::Statutory(_))) {
        println!(
            "A law's last day is moved past a weekend; a public holiday where the business acts \
             moves it to the next working day, which GhostSoap does not know (Acts \
             Interpretation Act 1901 s 36(2))."
        );
    }
    Ok(())
}

pub(super) fn checklist(args: &ChecklistArgs) -> Result<(), String> {
    let items = checklist::parse(&embedded::checklist())?;
    let today = today();
    let (marks, mark) = match &args.command {
        None => return show_checklist(&items, args, today),
        Some(ChecklistCommand::Done { marks, on }) => {
            (marks, Some((MarkKind::Done, on.unwrap_or(today))))
        }
        Some(ChecklistCommand::NotForMe(marks)) => (marks, Some((MarkKind::NotForMe, today))),
        Some(ChecklistCommand::Clear(marks)) => (marks, None),
    };
    if let Some(unknown) = marks
        .items
        .iter()
        .find(|id| !items.iter().any(|i| i.id == **id))
    {
        return Err(format!(
            "no checklist item is called {unknown}; `ghostsoap checklist --no-passphrase` \
             lists them"
        ));
    }
    let lines = update_state(
        marks.passphrase_stdin,
        |state| {
            for id in &marks.items {
                checklist::set_mark(&mut state.checklist, id, mark);
            }
            Ok(())
        },
        |()| {},
        |(), state| {
            Ok(items
                .iter()
                .filter(|item| marks.items.contains(&item.id))
                .map(|item| {
                    let progress = checklist::progress(item, &state.checklist, today);
                    format!("{}  {}", item.id, progress.words())
                })
                .collect::<Vec<_>>())
        },
    )?;
    println!("{}", lines.join("\n"));
    Ok(())
}

/// The checklist with the user's progress, unless `--no-passphrase` is given or there is no
/// state yet; with `--next`, one item in full.
pub(super) fn show_checklist(
    items: &[checklist::Item],
    args: &ChecklistArgs,
    today: Date,
) -> Result<(), String> {
    let dir = state::data_dir().map_err(|e| e.to_string())?;
    let marks = if args.no_passphrase || !state::state_path(&dir).exists() {
        eprintln!(
            "Progress is not shown. It is kept in your encrypted state once `ghostsoap init` \
             has run."
        );
        None
    } else {
        let passphrase = Input::new(args.passphrase_stdin)
            .passphrase("Passphrase: ")
            .map_err(|e| e.to_string())?;
        Some(
            state::load(&dir, passphrase)
                .map_err(|e| e.to_string())?
                .checklist,
        )
    };
    let progress =
        |item: &checklist::Item| marks.as_ref().map(|m| checklist::progress(item, m, today));
    if args.next {
        match checklist::next(items, marks.as_deref().unwrap_or_default(), today) {
            Some(item) => print!("{}", checklist_item(item, progress(item))),
            None => println!("Nothing left: every item is done or not for you."),
        }
        return Ok(());
    }
    let width = items.iter().map(|i| i.id.len()).max().unwrap_or(0) + 2;
    for item in items {
        let status = progress(item).map_or(String::new(), |p| format!("{:<31}", p.words()));
        println!(
            "{:<width$}{status}{}  ({})",
            item.id,
            item.title,
            item.effort.words()
        );
    }
    if let Some(marks) = &marks {
        println!("{}", checklist_counts(items, marks, today));
    }
    Ok(())
}

/// The checklist's count line: how many items stand at each progress.
pub(super) fn checklist_counts(
    items: &[checklist::Item],
    marks: &[checklist::Mark],
    today: Date,
) -> String {
    let count = |wanted: fn(&checklist::Progress) -> bool| {
        items
            .iter()
            .filter(|item| wanted(&checklist::progress(item, marks, today)))
            .count()
    };
    format!(
        "to do {}  check again {}  done {}  not for me {}",
        count(|p| *p == checklist::Progress::ToDo),
        count(|p| matches!(p, checklist::Progress::CheckAgain(_))),
        count(|p| matches!(p, checklist::Progress::Done(_))),
        count(|p| *p == checklist::Progress::NotForMe),
    )
}

/// One checklist item in full, as `checklist --next` shows it.
pub(super) fn checklist_item(
    item: &checklist::Item,
    progress: Option<checklist::Progress>,
) -> String {
    let mut out = format!("{}  ({})\n", item.title, item.id);
    if let Some(progress) = progress {
        out += &format!("Status: {}\n", progress.words());
    }
    let recheck = item
        .recheck_months
        .map_or("once".to_owned(), |m| format!("every {m} months"));
    out += &format!(
        "Applies: {}\nEffort: {}. Do it {recheck}.\n\nWhy: {}\n\nSteps:\n",
        item.applies,
        item.effort.words(),
        item.why
    );
    for (n, step) in item.steps.iter().enumerate() {
        out += &format!("{}. {step}\n", n + 1);
    }
    out += "\nLinks:\n";
    for link in &item.links {
        out += &format!("- {}: {} (checked {})\n", link.name, link.url, link.checked);
    }
    out + &format!(
        "\nWhen it is done: ghostsoap checklist done {id}\n\
         If it is not for you: ghostsoap checklist not-for-me {id}\n",
        id = item.id
    )
}

/// The count line: each action state and what `count` gives for it.
pub(super) fn counts(count: impl Fn(tracker::Action) -> String) -> String {
    let parts: Vec<String> = ACTIONS
        .iter()
        .map(|&a| format!("{} {}", action_name(a), count(a)))
        .collect();
    parts.join("  ")
}

/// One request as `status` lists it.
pub(super) fn status_line(
    action: tracker::Action,
    due: Due,
    message: &Message,
    request: &tracker::Request,
) -> String {
    let unfinished = if message.unfinished {
        "  (its draft run did not finish: draft it again)"
    } else {
        ""
    };
    format!(
        "{:<15}{}  {}  {}  {}{}{unfinished}",
        action_name(action),
        message.tag,
        message.entry,
        request.lever.name(),
        named(request.status(message.sent().is_some())),
        due_text(due),
    )
}

/// Apply `reply`'s flags to one message, returning the summary and any new audit event.
fn apply_reply(
    message: &mut Message,
    args: &ReplyArgs,
    on: Date,
    today: Date,
) -> Result<(String, Option<(AuditKind, String)>), String> {
    if let Some(of) = args.withdraw {
        let applied = message.withdraw_event(&args.lever, of, args.note.clone())?;
        let what = match of {
            tracker::Withdrawn::Status => "answer",
            tracker::Withdrawn::Extension => "longer period",
        };
        let detail = audit::withdrawal_detail(of, &applied, args.note.as_deref());
        return Ok((
            format!("the latest {what} is withdrawn"),
            Some((AuditKind::Withdrawal, detail)),
        ));
    }
    if let Some(until) = args.agreed_until {
        let extension = tracker::Extension {
            on,
            until,
            note: args.note.clone(),
        };
        let applied = message.record_extension(&args.lever, extension, today)?;
        let new = !applied.is_empty();
        let event = new.then(|| {
            let detail = audit::extension_detail(on, until, &applied, args.note.as_deref());
            (AuditKind::Extension, detail)
        });
        return Ok((
            format!("{} a longer period to {until}", recorded(new)),
            event,
        ));
    }
    apply_status_reply(message, args, on, today)
}

fn apply_status_reply(
    message: &mut Message,
    args: &ReplyArgs,
    on: Date,
    today: Date,
) -> Result<(String, Option<(AuditKind, String)>), String> {
    let status = args.status.expect("clap requires a status");
    let event = tracker::StatusEvent {
        on,
        status,
        reason: args.reason,
        source: args.source.clone(),
        note: args.note.clone(),
    };
    let applied = message.record_status(&args.lever, event, today)?;
    let new = !applied.is_empty();
    let report = if new && status == tracker::Status::Bounced {
        bounce_report(&message.entry, message.channel)
    } else {
        String::new()
    };
    let audit_event = new.then(|| {
        let kind = if status == tracker::Status::Escalated {
            AuditKind::Escalation
        } else {
            AuditKind::Reply
        };
        let detail = audit::reply_detail(
            on,
            status,
            &applied,
            args.reason,
            args.source.as_deref(),
            args.note.as_deref(),
        );
        (kind, detail)
    });
    Ok((
        format!("{} {} on {on}{report}", recorded(new), named(status)),
        audit_event,
    ))
}

/// Apply `sent`'s flags to one message and say what is now recorded, plus any new audit event.
pub(super) fn record_sending(
    message: &mut Message,
    args: &SentArgs,
    today: Date,
) -> Result<(String, Option<(AuditKind, String)>), String> {
    if args.withdraw {
        message.withdraw_sent(args.note.clone())?;
        let detail = audit::send_withdrawn_detail(args.note.as_deref());
        return Ok((
            "no longer recorded as sent".to_owned(),
            Some((AuditKind::SentWithdrawn, detail)),
        ));
    }
    let send = Send {
        on: args.on.unwrap_or(today),
        channel: args.channel.unwrap_or(message.channel),
    };
    let new = message.record_sent(send, args.correct, today)?;
    let recorded = if new {
        "recorded as sent"
    } else {
        "already recorded as sent"
    };
    let event = new.then(|| {
        (
            AuditKind::Sent,
            audit::send_detail(send.on, send.channel, args.correct),
        )
    });
    Ok((
        format!("{recorded} on {} by {}", send.on, send.channel.label()),
        event,
    ))
}
