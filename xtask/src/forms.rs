// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! `xtask form-check`: does each web form a user is sent to still load, and does it still
//! have the fields the user fills in from what GhostSoap gives them?
//!
//! `xtask/forms.toml` lists every such form once: each web-form channel that carries a
//! request, and each overseas entry's own form. A form is listed with the `name` of each
//! field to find in the page as served, or as `unchecked` with the reason, such as a form
//! drawn by a script, which a plain request cannot see. The check only requests each page,
//! one at a time; it never submits a form.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use ghostsoap::core::register::{ChannelKind, Entry, ExemptionRisk, SourceFile};

use crate::{Check, Dns, MIN_TEXT, dns_query, get_form, record, text};

/// Pause between requests, so no site sees more than one request every few seconds.
pub const PAUSE: Duration = Duration::from_secs(3);

/// The exit status of a run that keeps the open issue open.
pub const HELD: u8 = 3;

/// Characters of each page-served field name a report quotes.
const MAX_NAME: usize = 60;

/// What the check expects of one form.
#[derive(Debug, PartialEq, Eq)]
pub enum Expect {
    /// The `name` of each field the page must serve.
    Fields(Vec<String>),
    /// Why the page's fields cannot be checked; the form is still requested.
    Unchecked(String),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Form {
    pub url: String,
    pub expect: Expect,
}

/// Parse `forms.toml`: an array of `[[form]]` tables, each with `url` and either a
/// non-empty `fields` list or an `unchecked` reason, each URL once.
pub fn parse(source: &str) -> Result<Vec<Form>, String> {
    let table: toml::Table = toml::from_str(source).map_err(|e| e.to_string())?;
    if let Some(key) = table.keys().find(|k| *k != "form") {
        return Err(format!("unknown key `{key}`"));
    }
    let Some(list) = table.get("form").and_then(toml::Value::as_array) else {
        return Err("no [[form]] tables".to_owned());
    };
    let mut forms: Vec<Form> = Vec::new();
    for (i, item) in list.iter().enumerate() {
        let n = i + 1;
        let form = item
            .as_table()
            .ok_or_else(|| format!("form {n} is not a table"))?;
        if let Some(key) = form
            .keys()
            .find(|k| !["url", "fields", "unchecked"].contains(&k.as_str()))
        {
            return Err(format!("form {n}: unknown key `{key}`"));
        }
        let url = form
            .get("url")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("form {n}: no url"))?
            .to_owned();
        let expect = match (form.get("fields"), form.get("unchecked")) {
            (Some(fields), None) => {
                let names: Option<Vec<String>> = fields.as_array().and_then(|a| {
                    a.iter()
                        .map(|v| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned))
                        .collect()
                });
                match names {
                    Some(names) if !names.is_empty() => Expect::Fields(names),
                    _ => return Err(format!("{url}: fields must be a non-empty list of names")),
                }
            }
            (None, Some(reason)) => match reason.as_str().filter(|r| !r.trim().is_empty()) {
                Some(reason) => Expect::Unchecked(reason.to_owned()),
                None => return Err(format!("{url}: unchecked must give a reason")),
            },
            _ => return Err(format!("{url}: give either fields or unchecked")),
        };
        if forms.iter().any(|f| f.url == url) {
            return Err(format!("{url} is listed twice"));
        }
        forms.push(Form { url, expect });
    }
    Ok(forms)
}

/// The forms a user is sent to, each with the entries that send them: each web-form
/// channel of a current entry that carries a request, and each web-form channel of an
/// overseas entry, whose own form is its route.
pub fn sent_to(entries: &[SourceFile]) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut urls: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in entries {
        let entry: Entry = toml::from_str(&file.text).map_err(|e| format!("{}: {e}", file.name))?;
        if entry.obsolete.is_some() {
            continue;
        }
        let overseas = entry.exemption_risk == ExemptionRisk::Overseas;
        for channel in &entry.channels {
            if channel.kind == ChannelKind::WebForm && (overseas || !channel.levers.is_empty()) {
                let ids = urls.entry(channel.address.clone()).or_default();
                if !ids.contains(&entry.id) {
                    ids.push(entry.id.clone());
                }
            }
        }
    }
    Ok(urls)
}

/// Each way `forms` and the register disagree: a form users are sent to that is not
/// listed, or a listed form they are no longer sent to.
pub fn coverage(forms: &[Form], sent_to: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    let listed: BTreeSet<&str> = forms.iter().map(|f| f.url.as_str()).collect();
    let missing = sent_to
        .keys()
        .filter(|u| !listed.contains(u.as_str()))
        .map(|u| format!("not in forms.toml: {u}"));
    let stale = listed
        .iter()
        .filter(|u| !sent_to.contains_key(**u))
        .map(|u| format!("in forms.toml but no entry sends users there: {u}"));
    missing.chain(stale).collect()
}

/// Request `form` and compare the fields the page serves with those expected. A page with
/// no fields at all and little text is a bot check or a script-drawn page, so it says
/// nothing about the form; a page that has text but lacks a field has changed.
pub fn check(
    agent: &ureq::Agent,
    lookup: &dyn Fn(&str, &str) -> Result<Dns, String>,
    form: &Form,
) -> Check {
    let mut response = match get_form(agent, lookup, &form.url) {
        Ok(response) if response.status() == ureq::http::StatusCode::NO_CONTENT => {
            return Check::Dead("the page answers 204 No Content".to_owned());
        }
        Ok(response) => response,
        Err(check) => return check,
    };
    let Expect::Fields(expected) = &form.expect else {
        return Check::Fine;
    };
    // A body that cannot be read counts as an empty page, which is inconclusive below.
    let html = response
        .body_mut()
        .with_config()
        .limit(20 * 1024 * 1024)
        .read_to_vec()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    let served = text::field_names(&html);
    let missing: Vec<&str> = expected
        .iter()
        .filter(|name| !served.contains(name))
        .map(String::as_str)
        .collect();
    if missing.is_empty() {
        Check::Fine
    } else if served.is_empty() && text::main_text(&html).chars().count() < MIN_TEXT {
        Check::Inconclusive("no fields and little text; the page may need a browser".to_owned())
    } else {
        Check::Dead(format!(
            "missing field `{}`; the page serves {}",
            missing.join("`, `"),
            quoted(&served)
        ))
    }
}

/// At most 20 field names from a page, each in backticks, shortened and stripped of
/// backticks and control characters, so that a page cannot write Markdown into the issue.
fn quoted(all: &[String]) -> String {
    const SHOWN: usize = 20;
    let names: Vec<String> = all
        .iter()
        .take(SHOWN)
        .map(|name| {
            let mut safe: String = name
                .chars()
                .map(|c| match c {
                    '`' => '\'',
                    c if c.is_control() => ' ',
                    c => c,
                })
                .take(MAX_NAME)
                .collect();
            if name.chars().count() > MAX_NAME {
                safe += "...";
            }
            safe
        })
        .collect();
    let mut out = format!("`{}`", names.join("`, `"));
    let more = all.len().saturating_sub(SHOWN);
    if more > 0 {
        out += &format!(" and {more} more");
    }
    out
}

/// Check every form in `forms` against the register in `dir`, asking `resolver` (a DNS
/// over HTTPS endpoint) whether the host of a form that does not answer exists. Prints a
/// Markdown report of forms that are gone or changed, or nothing; inconclusive checks go to
/// stderr. Exits 1 if any form is gone or changed, or the list and the register disagree.
/// Otherwise exits `HELD` if a form named in `open`, the text of the open issue, could not
/// be checked, as the issue cannot yet be closed. A report keeps such a form listed.
pub fn run(
    dir: &Path,
    forms_file: &Path,
    open: &str,
    agent: &ureq::Agent,
    resolver: &str,
    pause: Duration,
) -> ExitCode {
    let (report, code) = checked(dir, forms_file, open, agent, resolver, pause);
    if !report.is_empty() {
        println!("{report}");
    }
    code
}

/// `run`'s checks: the report it prints, empty when there is none, and its exit status.
fn checked(
    dir: &Path,
    forms_file: &Path,
    open: &str,
    agent: &ureq::Agent,
    resolver: &str,
    pause: Duration,
) -> (String, ExitCode) {
    let lookup = |name: &str, record: &str| dns_query(agent, resolver, name, record);
    let loaded = std::fs::read_to_string(forms_file)
        .map_err(|e| format!("cannot read {}: {e}", forms_file.display()))
        .and_then(|s| parse(&s).map_err(|e| format!("{}: {e}", forms_file.display())));
    let register = ghostsoap::io::register::load(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))
        .and_then(|files| sent_to(&files.entries));
    let (forms, sent_to) = match (loaded, register) {
        (Ok(forms), Ok(sent_to)) => (forms, sent_to),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("error: {e}");
            return (String::new(), ExitCode::FAILURE);
        }
    };
    let mut lines: Vec<String> = coverage(&forms, &sent_to)
        .into_iter()
        .map(|problem| format!("- {problem}"))
        .collect();
    let mut held = Vec::new();
    for (i, form) in forms.iter().enumerate() {
        if i > 0 {
            std::thread::sleep(pause);
        }
        let what = format!("web form {}", form.url);
        let ids = sent_to.get(&form.url).map_or(&[][..], Vec::as_slice);
        let result = check(agent, &lookup, form);
        // The issue's lines are those `record` writes: "- `<ids>`: <what>: <problem>".
        let named = || {
            open.lines()
                .any(|line| line.starts_with("- `") && line.contains(&format!("`: {what}: ")))
        };
        if matches!(result, Check::Inconclusive(_)) && named() {
            held.push(format!(
                "- `{}`: {what}: could not be checked this week, so it stays listed",
                ids.join("`, `")
            ));
        }
        record(&mut lines, &what, ids, result);
    }
    if lines.is_empty() {
        let code = if held.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(HELD)
        };
        return (String::new(), code);
    }
    lines.extend(held);
    let report = format!(
        "The weekly form check found forms that are gone or no longer have the fields \
         users fill in. Check each against the business's own pages, then correct the \
         register entry's `[[channels]]` and `xtask/forms.toml` \
         (`cargo run -p xtask -- form-fields <url>` lists a page's fields).\n\n{}",
        lines.join("\n")
    );
    (report, ExitCode::FAILURE)
}

/// Print the fields each page serves, for writing `forms.toml`, requesting the pages
/// `pause` apart.
pub fn print_fields(agent: &ureq::Agent, urls: &[String], pause: Duration) -> ExitCode {
    let lookup = |_: &str, _: &str| Err("not looked up".to_owned());
    let mut failed = false;
    for (i, url) in urls.iter().enumerate() {
        if i > 0 {
            std::thread::sleep(pause);
        }
        let html = get_form(agent, &lookup, url)
            .map_err(|check| format!("{check:?}"))
            .and_then(|mut r| r.body_mut().read_to_string().map_err(|e| e.to_string()));
        match html {
            Ok(html) => println!("{url}\n  {}", text::field_names(&html).join("\n  ")),
            Err(e) => {
                failed = true;
                eprintln!("error: {url}: {e}");
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests;
