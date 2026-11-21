// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Validation of register entries and packs: the findings `registry check` reports.

use super::*;

/// What later checks need to know about an entry already seen.
pub(super) struct Known<'a> {
    pub(super) file: &'a str,
    pub(super) quality: Quality,
    pub(super) obsolete: bool,
}

/// Levers that return the user's personal information. An impersonator could use them to
/// collect a victim's details, so they are opt-in per entity (plan section 6a).
pub(super) const ACCESS_LEVERS: [Lever; 3] = [Lever::App12, Lever::S20rAccess, Lever::S21tAccess];

/// Validate register entries and packs. `today` dates the staleness and future-date checks.
/// Findings about packs name the file as `packs/<name>`; a finding with an empty file name
/// is about the register as a whole.
pub fn check(entries: &[SourceFile], packs: &[SourceFile], today: Date) -> Report {
    let mut report = Report::default();
    if entries.is_empty() {
        let message = "the register has no entry files".to_owned();
        report.push(Severity::Error, "", message);
    }
    let mut ids: BTreeMap<String, Known> = BTreeMap::new();
    let mut replacements = Vec::new();
    for file in entries {
        let Some(entry) = parse::<Entry>(&file.text, &file.name, &mut report) else {
            continue;
        };
        report.entries += 1;
        check_entry(&entry, &file.name, today, &mut report);
        if let Some(first) = ids.get(&entry.id) {
            let message = format!("id `{}` is already used by {}", entry.id, first.file);
            report.push(Severity::Error, &file.name, message);
            continue;
        }
        if let Some(target) = entry.obsolete.as_ref().and_then(|o| o.replaced_by.clone()) {
            replacements.push((file.name.as_str(), target));
        }
        let known = Known {
            file: &file.name,
            quality: entry.quality,
            obsolete: entry.obsolete.is_some(),
        };
        ids.insert(entry.id, known);
    }
    for (file, target) in replacements {
        if !ids.contains_key(&target) {
            let message = format!("obsolete.replaced_by names unknown entry `{target}`");
            report.push(Severity::Error, file, message);
        }
    }
    check_packs(packs, &ids, &mut report);
    report
}

pub(super) fn parse<T: DeserializeOwned>(text: &str, name: &str, report: &mut Report) -> Option<T> {
    match toml::from_str(text) {
        Ok(value) => Some(value),
        Err(e) => {
            report.push(Severity::Error, name, e.to_string().trim_end().to_owned());
            None
        }
    }
}

pub(super) fn check_entry(entry: &Entry, file: &str, today: Date, report: &mut Report) {
    let mut errors = Vec::new();
    check_id(&entry.id, file, &mut errors);
    if entry.name.trim().is_empty() || entry.legal_entity.trim().is_empty() {
        errors.push("name and legal_entity must not be empty".to_owned());
    }
    match (&entry.abn, entry.is_overseas()) {
        (None, false) => errors.push("abn is missing".to_owned()),
        (Some(_), true) => errors.push(
            "an overseas entry names an entity with no ABN, so it must not have an abn".to_owned(),
        ),
        (Some(abn), false) if !abn_is_valid(abn) => errors.push(format!(
            "ABN `{abn}` is malformed: use 11 digits as `NN NNN NNN NNN` with a valid check digit"
        )),
        _ => {}
    }
    if entry.kinds.is_empty() {
        errors.push("kinds must not be empty".to_owned());
    }
    if entry.sources.is_empty() {
        errors.push("no source: list at least one URL in `sources`".to_owned());
    }
    for source in entry.sources.iter().filter(|s| !is_https(s)) {
        errors.push(format!("source `{source}` is not an https URL"));
    }
    if entry.kinds.contains(&Kind::Telecommunications)
        && entry.escalation.first() != Some(&Escalation::Tio)
    {
        errors.push("a telecommunications entry must list `tio` first in escalation".to_owned());
    }
    check_levers(entry, &mut errors);
    check_channels(entry, &mut errors);
    check_channel_levers(entry, &mut errors);
    check_obsolete(entry, &mut errors);
    check_evidence(entry, today, &mut errors);
    match date_of(&entry.verified_at) {
        None => errors.push("verified_at must be a date such as 2026-10-24".to_owned()),
        Some(date) if is_future(date, today) => {
            errors.push(format!("verified_at {date} is in the future"));
        }
        Some(date) if is_stale(date, today) => {
            let message =
                format!("verified_at {date} is more than 12 months old; recheck the entry");
            report.push(Severity::Warning, file, message);
        }
        Some(_) => {}
    }
    for message in errors {
        report.push(Severity::Error, file, message);
    }
}

pub(super) fn check_id(id: &str, file: &str, errors: &mut Vec<String>) {
    if !is_slug(id) {
        errors.push(format!(
            "id `{id}` must be lowercase letters, digits and single hyphens"
        ));
    } else if file.strip_suffix(".toml") != Some(id) {
        errors.push(format!("id `{id}` does not match the file name"));
    }
}

pub(super) fn check_levers(entry: &Entry, errors: &mut Vec<String>) {
    let no_levers = entry.levers.is_empty() && entry.opt_in_levers.is_empty();
    if entry.is_overseas() {
        if !no_levers {
            errors
                .push("an overseas entry has no levers: GhostSoap drafts it no letter".to_owned());
        }
        if entry.manual_check.is_none() {
            errors.push("an overseas entry needs a manual_check page".to_owned());
        }
        if !entry
            .channels
            .iter()
            .any(|c| c.kind == ChannelKind::WebForm)
        {
            errors.push("an overseas entry needs its own web-form opt-out channel".to_owned());
        }
    } else if entry.obsolete.is_none() && no_levers {
        errors.push("an entry must list at least one lever".to_owned());
    }
    if entry.levers.iter().any(|l| ACCESS_LEVERS.contains(l)) {
        errors.push(
            "access levers (app12, s20r-access, s21t-access) belong in opt_in_levers only"
                .to_owned(),
        );
    }
    if entry.opt_in_levers.iter().any(|l| entry.levers.contains(l)) {
        errors.push("a lever is listed in both levers and opt_in_levers".to_owned());
    }
}

pub(super) fn check_channels(entry: &Entry, errors: &mut Vec<String>) {
    if entry.obsolete.is_none() && entry.channels.is_empty() {
        errors.push("channels must list at least one way to send a request".to_owned());
    }
    for channel in &entry.channels {
        let address = &channel.address;
        match channel.kind {
            ChannelKind::Email if !is_email(address) => {
                errors.push(format!("`{address}` is not an email address"));
            }
            ChannelKind::WebForm if !is_https(address) => {
                errors.push(format!("web form `{address}` is not an https URL"));
            }
            _ if address.trim().is_empty() => {
                errors.push("a channel address must not be empty".to_owned());
            }
            _ => {}
        }
    }
    if let Some(manual) = &entry.manual_check
        && !is_https(&manual.url)
    {
        errors.push(format!(
            "manual_check.url `{}` is not an https URL",
            manual.url
        ));
    }
}

/// Every lever has a channel, every channel lever belongs to the entry, and no lever has
/// two preferred channels.
pub(super) fn check_channel_levers(entry: &Entry, errors: &mut Vec<String>) {
    let listed = || entry.levers.iter().chain(&entry.opt_in_levers);
    for channel in &entry.channels {
        for lever in channel.levers.iter().filter(|l| !listed().any(|x| x == *l)) {
            errors.push(format!(
                "channel `{}` serves `{}`, which the entry does not list",
                channel.address,
                lever.name()
            ));
        }
        // The letter states the Spam Act's 5-business-day period from the day an electronic
        // message is sent; post and other methods start it on a different day (Schedule 2
        // clause 6(1)).
        if channel.kind != ChannelKind::Email && channel.levers.contains(&Lever::SpamUnsubscribe) {
            errors.push(format!(
                "channel `{}` serves `spam-unsubscribe`, which only an email channel may serve",
                channel.address
            ));
        }
    }
    for lever in listed() {
        let serving: Vec<&Channel> = entry
            .channels
            .iter()
            .filter(|c| c.levers.contains(lever))
            .collect();
        if serving.is_empty() {
            errors.push(format!("no channel serves `{}`", lever.name()));
        }
        if serving.iter().filter(|c| c.preferred).count() > 1 {
            errors.push(format!(
                "more than one channel is preferred for `{}`",
                lever.name()
            ));
        }
    }
}

pub(super) fn check_evidence(entry: &Entry, today: Date, errors: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    for evidence in &entry.evidence {
        let url = &evidence.url;
        if !seen.insert(url) {
            errors.push(format!("evidence url `{url}` is listed twice"));
        }
        if !entry.sources.contains(url) {
            errors.push(format!("evidence url `{url}` is not one of the sources"));
        }
        let hex = |c: char| c.is_ascii_digit() || ('a'..='f').contains(&c);
        if evidence.sha256.len() != 64 || !evidence.sha256.chars().all(hex) {
            errors.push(format!(
                "evidence sha256 for `{url}` must be 64 lowercase hex digits"
            ));
        }
        if date_of(&evidence.checked_at).is_none_or(|date| is_future(date, today)) {
            errors.push(format!(
                "evidence checked_at for `{url}` must be a date, not in the future"
            ));
        }
    }
}

pub(super) fn check_obsolete(entry: &Entry, errors: &mut Vec<String>) {
    let Some(obsolete) = &entry.obsolete else {
        return;
    };
    if obsolete.reason.needs_redirect() && obsolete.replaced_by.is_none() {
        errors.push(format!(
            "obsolete.reason `{}` needs replaced_by",
            obsolete.reason.name()
        ));
    }
    if obsolete.replaced_by.as_deref() == Some(entry.id.as_str()) {
        errors.push("obsolete.replaced_by must name another entry".to_owned());
    }
}

pub(super) fn check_packs(
    packs: &[SourceFile],
    ids: &BTreeMap<String, Known>,
    report: &mut Report,
) {
    for file in packs {
        let name = format!("packs/{}", file.name);
        let Some(pack) = parse::<Pack>(&file.text, &name, report) else {
            continue;
        };
        report.packs += 1;
        let mut errors = Vec::new();
        check_id(&pack.id, &file.name, &mut errors);
        if pack.entries.is_empty() {
            errors.push("pack lists no entries".to_owned());
        }
        let mut listed = BTreeSet::new();
        for id in &pack.entries {
            if !listed.insert(id) {
                errors.push(format!("pack lists `{id}` twice"));
                continue;
            }
            match ids.get(id) {
                None => errors.push(format!("pack lists unknown entry `{id}`")),
                Some(known) if known.obsolete => {
                    errors.push(format!("pack lists obsolete entry `{id}`"));
                }
                Some(known) if !known.quality.is_packable() => {
                    errors.push(format!(
                        "pack lists `{id}`, which is below verified quality"
                    ));
                }
                Some(_) => {}
            }
        }
        for message in errors {
            report.push(Severity::Error, &name, message);
        }
    }
}

/// An ABN in the form `NN NNN NNN NNN` whose digits pass the ATO check-digit test.
pub fn abn_is_valid(abn: &str) -> bool {
    const WEIGHTS: [u32; 11] = [10, 1, 3, 5, 7, 9, 11, 13, 15, 17, 19];
    let shape = abn.len() == 14 && [2, 6, 10].iter().all(|&i| abn.as_bytes()[i] == b' ');
    let digits: Option<Vec<u32>> = abn
        .chars()
        .filter(|&c| c != ' ')
        .map(|c| c.to_digit(10))
        .collect();
    match digits {
        Some(mut d) if shape && d.len() == 11 && d[0] > 0 => {
            d[0] -= 1;
            d.iter().zip(WEIGHTS).map(|(x, w)| x * w).sum::<u32>() % 89 == 0
        }
        _ => false,
    }
}

pub(super) fn is_slug(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && !id.ends_with('-')
        && !id.contains("--")
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// An https URL with a dotted host name and, optionally, a numeric port.
pub(super) fn is_https(url: &str) -> bool {
    url.strip_prefix("https://").is_some_and(|rest| {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let (host, port) = authority.split_once(':').unwrap_or((authority, "1"));
        let port_ok =
            port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok_and(|p| p > 0);
        is_domain(host) && port_ok && !url.contains(char::is_whitespace)
    })
}

/// Two or more DNS labels of 1 to 63 letters, digits and inner hyphens, at most 253
/// bytes in all.
pub(super) fn is_domain(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    labels.len() >= 2
        && host.len() <= 253
        && labels.iter().all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

pub(crate) fn is_email(address: &str) -> bool {
    address.split_once('@').is_some_and(|(local, domain)| {
        !local.is_empty()
            && local
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-/=?^_`{|}~.".contains(c))
            && !local.starts_with('.')
            && !local.ends_with('.')
            && !local.contains("..")
            && is_domain(domain)
            && !address.contains(char::is_whitespace)
    })
}

/// A plain TOML date (no time or offset) as a calendar date.
pub(crate) fn date_of(value: &Datetime) -> Option<Date> {
    match (value.date, value.time, value.offset) {
        (Some(d), None, None) => Date::new(
            i16::try_from(d.year).ok()?,
            i8::try_from(d.month).ok()?,
            i8::try_from(d.day).ok()?,
        )
        .ok(),
        _ => None,
    }
}

/// More than a day ahead. The day of grace covers entries dated in Australian time
/// whilst CI runs in UTC.
pub(super) fn is_future(date: Date, today: Date) -> bool {
    today.tomorrow().is_ok_and(|tomorrow| date > tomorrow)
}

pub(super) fn is_stale(verified: Date, today: Date) -> bool {
    today
        .checked_sub(12.months())
        .is_ok_and(|cutoff| verified < cutoff)
}
