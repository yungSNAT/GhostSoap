// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! The hygiene checklist (plan section 7): steps a person takes themselves, each with the
//! official page it links to, and the marks that record their progress in the state.

use std::collections::BTreeSet;

use jiff::ToSpan;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};
use toml::value::Datetime;

use crate::core::register::{self, SourceFile};

/// One checklist item, read from `checklist/au/<id>.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    /// The file name without `.toml`.
    #[serde(skip)]
    pub id: String,
    /// Its place in the list and for `--next`, lowest first.
    pub rank: u32,
    pub title: String,
    /// Why it matters.
    pub why: String,
    pub steps: Vec<String>,
    /// When it applies, in words.
    pub applies: String,
    pub effort: Effort,
    /// How long a done item stays done; none means once is enough.
    pub recheck_months: Option<u32>,
    /// The official page first.
    pub links: Vec<Link>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Effort {
    Minutes,
    AnHour,
    Longer,
}

impl Effort {
    pub fn words(self) -> &'static str {
        match self {
            Self::Minutes => "minutes",
            Self::AnHour => "about an hour",
            Self::Longer => "longer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub name: String,
    pub url: String,
    /// The day the page was last read to confirm the item, a plain TOML date as the
    /// register writes its dates.
    #[serde(deserialize_with = "plain_date")]
    pub checked: Date,
}

fn plain_date<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Date, D::Error> {
    let value = Datetime::deserialize(deserializer)?;
    register::date_of(&value)
        .ok_or_else(|| serde::de::Error::custom("use a plain date such as 2026-11-17"))
}

/// The user's mark on one item, kept in the encrypted state. A mark for an item this
/// version does not ship is kept and not shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mark {
    pub item: String,
    pub on: Date,
    pub mark: MarkKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MarkKind {
    Done,
    NotForMe,
}

/// Where an item stands for the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    ToDo,
    Done(Date),
    /// Done, but its re-check interval has passed.
    CheckAgain(Date),
    NotForMe,
}

impl Progress {
    pub fn words(self) -> String {
        match self {
            Self::ToDo => "to do".to_owned(),
            Self::Done(on) => format!("done {on}"),
            Self::CheckAgain(on) => format!("check again (done {on})"),
            Self::NotForMe => "not for me".to_owned(),
        }
    }
}

/// Read and validate the checklist files, sorted by rank. Every problem is reported, by
/// file, before any item is returned.
pub fn parse(files: &[SourceFile]) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    let mut problems = Vec::new();
    for file in files {
        let Some(id) = file.name.strip_suffix(".toml") else {
            problems.push(format!("{}: not a .toml file", file.name));
            continue;
        };
        match toml::from_str::<Item>(&file.text) {
            Ok(item) => items.push(Item {
                id: id.to_owned(),
                ..item
            }),
            Err(e) => problems.push(format!("{}: {}", file.name, e.message())),
        }
    }
    problems.extend(check(&items));
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    items.sort_by_key(|item| item.rank);
    Ok(items)
}

fn check(items: &[Item]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut ranks = BTreeSet::new();
    for item in items {
        let mut problem = |what: &str| problems.push(format!("{}.toml: {what}", item.id));
        let id_ok = !item.id.is_empty()
            && item
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !id_ok {
            problem("the file name must be lowercase letters, digits and hyphens");
        }
        if !ranks.insert(item.rank) {
            problem("its rank is used by another item");
        }
        let blank = [&item.title, &item.why, &item.applies]
            .iter()
            .any(|text| text.trim().is_empty());
        if blank || item.steps.is_empty() || item.steps.iter().any(|s| s.trim().is_empty()) {
            problem("title, why, applies and each step must have text, with at least one step");
        }
        if item.recheck_months == Some(0) {
            problem("recheck_months must be at least 1");
        }
        if item.links.is_empty() {
            problem("it needs at least one link, the official page first");
        }
        for link in &item.links {
            if link.name.trim().is_empty() || !link.url.starts_with("https://") {
                problem("each link needs a name and an https:// URL");
            }
        }
    }
    problems
}

/// Each link whose `checked` day is before the first day of `month` (`YYYY-MM`), as
/// `<id>: <url> (checked <day>)`. M4's release gate: every link checked within the release
/// month.
pub fn unchecked_since(items: &[Item], month: &str) -> Result<Vec<String>, String> {
    let first = Date::strptime("%Y-%m-%d", format!("{month}-01"))
        .map_err(|_| format!("{month} is not a month such as 2026-12"))?;
    Ok(items
        .iter()
        .flat_map(|item| item.links.iter().map(move |link| (item, link)))
        .filter(|(_, link)| link.checked < first)
        .map(|(item, link)| format!("{}: {} (checked {})", item.id, link.url, link.checked))
        .collect())
}

/// Where `item` stands, given the user's marks, on `today`.
pub fn progress(item: &Item, marks: &[Mark], today: Date) -> Progress {
    match marks.iter().find(|m| m.item == item.id) {
        None => Progress::ToDo,
        Some(Mark {
            mark: MarkKind::NotForMe,
            ..
        }) => Progress::NotForMe,
        Some(Mark { on, .. }) => {
            let due = item
                .recheck_months
                .and_then(|months| on.checked_add(i64::from(months).months()).ok());
            match due {
                Some(due) if today >= due => Progress::CheckAgain(*on),
                _ => Progress::Done(*on),
            }
        }
    }
}

/// The item to do next: the first by rank to check again, else the first to do.
pub fn next<'a>(items: &'a [Item], marks: &[Mark], today: Date) -> Option<&'a Item> {
    let first = |wanted: fn(Progress) -> bool| {
        items
            .iter()
            .find(|item| wanted(progress(item, marks, today)))
    };
    first(|p| matches!(p, Progress::CheckAgain(_))).or_else(|| first(|p| p == Progress::ToDo))
}

/// Set the user's mark on `item`, replacing any earlier one; `None` clears it.
pub fn set_mark(marks: &mut Vec<Mark>, item: &str, mark: Option<(MarkKind, Date)>) {
    marks.retain(|m| m.item != item);
    if let Some((mark, on)) = mark {
        marks.push(Mark {
            item: item.to_owned(),
            on,
            mark,
        });
    }
}

#[cfg(test)]
mod tests;
