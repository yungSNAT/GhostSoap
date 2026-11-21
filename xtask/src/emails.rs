// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Email addresses: whether each is on its entry's pages, and which ones a change to the register adds.

use super::*;

/// Check each email channel of `entry` against its source pages. `pages` holds the
/// searchable text of its evidence pages; its other sources, such as a page behind a bot
/// check that cannot have evidence, are fetched only when an address is not on those.
pub(super) fn email_checks(
    agent: &ureq::Agent,
    entry: &Entry,
    mut pages: Vec<Option<String>>,
) -> Vec<(String, Check)> {
    let addresses: Vec<&str> = entry
        .channels
        .iter()
        .filter(|c| c.kind == ChannelKind::Email)
        .map(|c| c.address.as_str())
        .collect();
    if addresses
        .iter()
        .any(|a| email_on_pages(a, &pages) != Check::Fine)
    {
        let tracked = |url: &String| entry.evidence.iter().any(|e| &e.url == url);
        for url in entry.sources.iter().filter(|url| !tracked(url)) {
            pages.push(evidence_page(agent, url).ok().and_then(|page| page.text));
        }
    }
    addresses
        .into_iter()
        .map(|a| (a.to_owned(), email_on_pages(a, &pages)))
        .collect()
}

/// Whether `address` appears on one of its entry's source pages, given each page's
/// searchable text (`None` for a page that failed or is not HTML). It is missing only when
/// every page could be searched.
pub(super) fn email_on_pages(address: &str, pages: &[Option<String>]) -> Check {
    let address = address.to_lowercase();
    if pages
        .iter()
        .flatten()
        .any(|text| has_address(text, &address))
    {
        Check::Fine
    } else if pages.is_empty() {
        Check::Inconclusive("the entry has no source page".to_owned())
    } else if pages.iter().any(Option::is_none) {
        Check::Inconclusive("not on the source pages that could be searched".to_owned())
    } else {
        Check::Dead("appears on none of the entry's source pages".to_owned())
    }
}

/// Whether `text` contains `address` as a whole address, not inside a longer one such as
/// `not` + address or address + `.au`. A sentence's full stop may follow it, and a
/// percent-escape in a link, such as `%20`, may precede it unless it encodes a character an
/// address can contain, such as `%2B`.
pub(super) fn has_address(text: &str, address: &str) -> bool {
    let local = |c: char| c.is_ascii_alphanumeric() || "._%+-".contains(c);
    let domain = |c: char| c.is_ascii_alphanumeric() || c == '-';
    let escaped_boundary = |before: &[u8]| match before {
        [.., b'%', h, l] if h.is_ascii_hexdigit() && l.is_ascii_hexdigit() => {
            let hex = |d: u8| char::from(d).to_digit(16).unwrap_or(0) as u8;
            !local(char::from(hex(*h) * 16 + hex(*l)))
        }
        _ => false,
    };
    text.match_indices(address).any(|(i, _)| {
        let after = &text[i + address.len()..];
        (!text[..i].ends_with(local) || escaped_boundary(&text.as_bytes()[..i]))
            && !after.starts_with(domain)
            && !(after.starts_with('.') && after[1..].starts_with(domain))
    })
}

/// Email addresses in `head` that the same entry in `base` did not have, as `(file, message)`.
/// A change that repoints a privacy address would send users' requests elsewhere, so a
/// reviewer must see each one. Every address of an entry whose id is not in `base` is listed,
/// so renaming an entry cannot hide a changed address. Fails on a file that does not parse.
pub(super) fn email_changes(
    base: &[SourceFile],
    head: &[SourceFile],
) -> Result<Vec<(String, String)>, String> {
    // Domains are case-insensitive; a local part may not be, so only the domain is folded.
    let emails = |entry: &Entry| -> Vec<String> {
        let fold = |address: &str| match address.rsplit_once('@') {
            Some((local, domain)) => format!("{local}@{}", domain.to_lowercase()),
            None => address.to_owned(),
        };
        entry
            .channels
            .iter()
            .filter(|c| c.kind == ChannelKind::Email)
            .map(|c| fold(&c.address))
            .collect()
    };
    let parsed = |files: &[SourceFile]| -> Result<Vec<(String, Entry)>, String> {
        let parse = |f: &SourceFile| {
            toml::from_str(&f.text)
                .map(|e| (f.name.clone(), e))
                .map_err(|e| format!("{} does not parse: {e}", f.name))
        };
        files.iter().map(parse).collect()
    };
    let before: BTreeMap<String, Vec<String>> = parsed(base)?
        .into_iter()
        .map(|(_, entry)| (entry.id.clone(), emails(&entry)))
        .collect();
    let mut changes = Vec::new();
    for (file, entry) in parsed(head)? {
        let old = before.get(&entry.id);
        for address in emails(&entry)
            .into_iter()
            .filter(|a| old.is_none_or(|old| !old.contains(a)))
        {
            let message = match old {
                None => format!(
                    "new entry `{}` lists email {address}; check it on the entity's own pages",
                    entry.id
                ),
                Some(old) => {
                    let was = if old.is_empty() {
                        "none".to_owned()
                    } else {
                        old.join(", ")
                    };
                    format!(
                        "`{}` gains email {address} (before: {was}); check it on the entity's own pages",
                        entry.id
                    )
                }
            };
            changes.push((file.clone(), message));
        }
    }
    Ok(changes)
}

/// Print a GitHub warning annotation for each email address `email_changes` lists. Exits 0
/// either way, like a stale `verified_at`; an unreadable register or entry exits 1.
pub(super) fn emails_changed(base: &Path, head: &Path) -> ExitCode {
    let load = |dir: &Path| {
        ghostsoap::io::register::load(dir)
            .map_err(|e| eprintln!("error: cannot read {}: {e}", dir.display()))
    };
    let (Ok(base_files), Ok(head_files)) = (load(base), load(head)) else {
        return ExitCode::FAILURE;
    };
    let changes = match email_changes(&base_files.entries, &head_files.entries) {
        Ok(changes) => changes,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    // A workflow command escapes `%`, CR and LF in its message, and `:` and `,` too in a
    // property value.
    let data = |s: &str| {
        s.replace('%', "%25")
            .replace('\r', "%0D")
            .replace('\n', "%0A")
    };
    for (file, message) in &changes {
        let path = head.join(file).to_string_lossy().replace('\\', "/");
        let path = data(&path).replace(':', "%3A").replace(',', "%2C");
        let message = data(message);
        println!("::warning file={path},title=Register email changed::{message}");
    }
    println!("{} email address(es) to check", changes.len());
    ExitCode::SUCCESS
}
