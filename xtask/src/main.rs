// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Maintainer tasks for the GhostSoap register. Not part of the `ghostsoap` binary, which
//! makes no network requests of its own.
//!
//! ```text
//! cargo run -p xtask -- evidence-hash <url>...        print the evidence hash of each page
//! cargo run -p xtask -- evidence-drift [registry/au [text dir]]
//!                                                     report pages whose hash has changed
//!                                                     and laws or legal pages newer than
//!                                                     the law table, optionally saving
//!                                                     each page's text
//! cargo run -p xtask -- email-changes <base> <head>   warn of email addresses a change added
//! cargo run -p xtask -- form-check [registry/au [xtask/forms.toml [open issue]]]
//!                                                     fail if a form users are sent to is
//!                                                     gone or lacks a listed field; exit 3
//!                                                     if a form the open issue names could
//!                                                     not be checked
//! cargo run -p xtask -- form-fields <url>...          print the fields each page serves
//! cargo run -p xtask -- no-network                    fail if ghostsoap could open a connection
//! ```

mod channels;
mod emails;
mod forms;
mod no_network;
#[cfg(test)]
mod registry_table;
mod text;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use ghostsoap::core::register::{ChannelKind, Entry, SourceFile};
use ghostsoap::core::{checklist, embedded};
use ghostsoap::io::drafts::sha256;

use channels::*;
use emails::*;

/// Pages with less main text than this are bot checks or script-only shells.
const MIN_TEXT: usize = 500;
const USER_AGENT: &str = "GhostSoap-evidence-check/0.1 (+https://github.com/yungSNAT/GhostSoap)";
const LEGISLATION_VERSIONS: &str = "https://api.prod.legislation.gov.au/v1/Versions";
const DNS_RESOLVER: &str = "https://dns.google/resolve";
/// Whole-request limit, so one slow or silent site fails instead of stalling the check.
const TIMEOUT: Duration = Duration::from_secs(30);
/// Each law the plan pins: its name, Federal Register title and the version read.
const LAWS: [(&str, &str, &str); 5] = [
    ("Privacy Act 1988", "C2004A03712", "C2026C00227"),
    ("Acts Interpretation Act 1901", "C1901A00002", "C2026C00117"),
    ("Spam Act 2003", "C2004A01214", "C2016C00614"),
    ("Spam Regulations 2021", "F2021L00285", "F2021L00285"),
    ("Criminal Code", "C2004A04868", "C2026C00243"),
];

/// Pages outside the register that the escalation targets rest on: a name, a file name
/// for the saved text, the URL and the evidence hash of the text read.
const LEGAL_PAGES: [LegalPage; 1] = [(
    "OAIC recognised external dispute resolution schemes register",
    "oaic-edr-register",
    "https://www.oaic.gov.au/privacy/privacy-registers/recognised-external-dispute-resolution-schemes-register",
    "2e23acc8194089cd50cefeb1d29a4d26df08480b6656738214cde865edbb430d",
)];

type LegalPage<'a> = (&'a str, &'a str, &'a str, &'a str);

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

/// A fetched evidence page.
struct Page {
    sha256: String,
    /// The normalised main text the hash covers; `None` for a non-HTML page.
    main: Option<String>,
    /// Lowercase raw HTML, main text and Cloudflare-protected addresses, for finding an
    /// email address in a link, a footer, or entity-encoded or obfuscated text; `None` for
    /// a non-HTML page.
    text: Option<String>,
}

/// Why an evidence page could not be hashed.
#[derive(Debug)]
enum PageError {
    /// 401, 403 or 429: sites answer scripts this way even when the page is up.
    Refused(String),
    Failed(String),
}

impl std::fmt::Display for PageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(e) | Self::Failed(e) => f.write_str(e),
        }
    }
}

/// Fetch `url`. Its evidence hash is SHA-256 of its normalised main text, or of the raw
/// bytes for a non-HTML response.
fn evidence_page(agent: &ureq::Agent, url: &str) -> Result<Page, PageError> {
    let mut response = agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(401 | 403 | 429) => PageError::Refused(e.to_string()),
            _ => PageError::Failed(e.to_string()),
        })?;
    let content_type = response
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap_or_default().to_owned());
    let bytes = response
        .body_mut()
        .with_config()
        .limit(20 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| PageError::Failed(e.to_string()))?;
    let (digest, main, text) = if text::is_html(content_type.as_deref(), &bytes) {
        let html = String::from_utf8_lossy(&bytes);
        let main = text::main_text(&html);
        if main.chars().count() < MIN_TEXT {
            return Err(PageError::Failed(format!(
                "only {} characters of main text; the page may need a browser",
                main.chars().count()
            )));
        }
        let hidden = text::cloudflare_emails(&html).join("\n");
        let searchable = format!("{html}\n{main}\n{hidden}").to_lowercase();
        (sha256(main.as_bytes()), Some(main), Some(searchable))
    } else {
        (sha256(&bytes), None, None)
    };
    Ok(Page {
        sha256: digest,
        main,
        text,
    })
}

/// Save a page's main text as `<entry id>-<URL hash>.txt` under `dir`, so a later run's
/// copy can be diffed against it. The URL is the first line; after a blank line each
/// sentence starts a line, and joining those lines with spaces gives the hashed text.
fn save_text(dir: &Path, id: &str, url: &str, main: &str) {
    let (name, body) = text_file(id, url, main);
    if let Err(e) =
        std::fs::create_dir_all(dir).and_then(|()| std::fs::write(dir.join(&name), body))
    {
        eprintln!("warning: cannot save the text of {url}: {e}");
    }
}

/// The file name and contents `save_text` writes.
fn text_file(id: &str, url: &str, main: &str) -> (String, String) {
    let name = format!("{id}-{}.txt", &sha256(url.as_bytes())[..8]);
    (name, format!("{url}\n\n{}\n", main.replace(". ", ".\n")))
}

fn hash(urls: &[String]) -> ExitCode {
    let agent = agent(TIMEOUT);
    let mut failed = false;
    for url in urls {
        match evidence_page(&agent, url) {
            Ok(page) => println!("{}  {url}", page.sha256),
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

/// Prints a Markdown report of changed and failing evidence pages, or nothing if every
/// page still matches, and saves each HTML page's text under `text_dir` if given. Exits 0
/// either way; a register that cannot be read exits 1.
fn drift(
    dir: &Path,
    text_dir: Option<&Path>,
    agent: &ureq::Agent,
    dns_resolver: &str,
    versions_url: &str,
    legal_pages: &[LegalPage],
) -> ExitCode {
    let files = match ghostsoap::io::register::load(dir) {
        Ok(files) => files,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", dir.display());
            return ExitCode::FAILURE;
        }
    };
    let report = drift_report(
        agent,
        &files.entries,
        text_dir,
        &|name, record| dns_query(agent, dns_resolver, name, record),
        &|title| latest_version(agent, versions_url, title),
        legal_pages,
    );
    print!("{report}");
    ExitCode::SUCCESS
}

fn drift_report(
    agent: &ureq::Agent,
    entries: &[SourceFile],
    text_dir: Option<&Path>,
    lookup: &dyn Fn(&str, &str) -> Result<Dns, String>,
    latest: &dyn Fn(&str) -> Result<String, String>,
    legal_pages: &[LegalPage],
) -> String {
    let mut changed = Vec::new();
    let mut failing = Vec::new();
    let mut mail_domains = BTreeMap::new();
    let mut forms = BTreeMap::new();
    let mut emails = Vec::new();
    for file in entries {
        let entry: Entry = match toml::from_str(&file.text) {
            Ok(entry) => entry,
            Err(e) => {
                failing.push(format!("- `{}`: does not parse: {e}", file.name));
                continue;
            }
        };
        let ids = std::slice::from_ref(&entry.id);
        let mut pages = Vec::new();
        for evidence in &entry.evidence {
            match evidence_page(agent, &evidence.url) {
                Ok(page) => {
                    if page.sha256 != evidence.sha256 {
                        changed.push(format!(
                            "- `{}`: {} (checked {})",
                            entry.id, evidence.url, evidence.checked_at
                        ));
                    }
                    if let (Some(dir), Some(main)) = (text_dir, &page.main) {
                        save_text(dir, &entry.id, &evidence.url, main);
                    }
                    pages.push(page.text);
                }
                Err(PageError::Refused(e)) => {
                    record(&mut failing, &evidence.url, ids, Check::Inconclusive(e));
                    pages.push(None);
                }
                Err(PageError::Failed(e)) => {
                    record(&mut failing, &evidence.url, ids, Check::Dead(e));
                    pages.push(None);
                }
            }
        }
        if entry.obsolete.is_none() {
            collect_channels(&entry, &mut mail_domains, &mut forms);
            for (address, check) in email_checks(agent, &entry, pages) {
                record(&mut emails, &format!("email {address}"), ids, check);
            }
        }
    }
    let mut channels = channel_problems(agent, lookup, &mail_domains, &forms);
    channels.append(&mut emails);
    let mut laws = law_changes(latest);
    laws.append(&mut legal_page_changes(agent, legal_pages, text_dir));
    report(&changed, &failing, &channels, &laws)
}

/// The Federal Register's latest version of a title.
fn latest_version(agent: &ureq::Agent, endpoint: &str, title: &str) -> Result<String, String> {
    let body = agent
        .get(endpoint)
        .query(
            "$filter",
            format!("titleId eq '{title}' and isLatest eq true"),
        )
        .query("$select", "registerId")
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    parse_latest(&body)
}

fn parse_latest(body: &str) -> Result<String, String> {
    let reply: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    match reply["value"].as_array().map(Vec::as_slice) {
        Some([version]) => version["registerId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| "a version has no register ID".to_owned()),
        versions => Err(format!("expected one latest version, got {versions:?}")),
    }
}

/// Laws whose latest version is not the one read, and laws that could not be checked.
fn law_changes(latest: &dyn Fn(&str) -> Result<String, String>) -> Vec<String> {
    LAWS.iter()
        .filter_map(|(name, title, read)| match latest(title) {
            Ok(id) if id == *read => None,
            Ok(id) => Some(format!("- {name}: {id} is now the latest version")),
            Err(e) => Some(format!("- {name}: inconclusive: {e}")),
        })
        .collect()
}

/// Legal pages whose text has changed, that no longer load or that refused the check. A
/// refused page proves nothing, so it stays unverified and keeps the issue open.
fn legal_page_changes(
    agent: &ureq::Agent,
    pages: &[LegalPage],
    text_dir: Option<&Path>,
) -> Vec<String> {
    let mut lines = Vec::new();
    for &(name, id, url, sha256) in pages {
        match evidence_page(agent, url) {
            Ok(page) => {
                if page.sha256 != sha256 {
                    lines.push(format!(
                        "- {name}: {url} has changed. Check the `escalation` values and the \
                         plan's law status against it, then update `LEGAL_PAGES` in xtask"
                    ));
                }
                if let (Some(dir), Some(main)) = (text_dir, &page.main) {
                    save_text(dir, id, url, main);
                }
            }
            Err(PageError::Refused(e)) => lines.push(format!(
                "- {name}: {url}: unverified, because it refused the check ({e}). Compare it \
                 by hand with the `escalation` values and the plan's law status"
            )),
            Err(PageError::Failed(e)) => lines.push(format!("- {name}: {url}: {e}")),
        }
    }
    lines
}

/// Add a failed check to the issue's lines.
fn record(lines: &mut Vec<String>, what: &str, ids: &[String], check: Check) {
    match check {
        Check::Fine => {}
        Check::Dead(problem) => {
            lines.push(format!("- `{}`: {what}: {problem}", ids.join("`, `")));
        }
        // The workflow log keeps these; they stay out of the issue so that it can close.
        // evidence-drift.yml counts lines starting `inconclusive: ` for its closing comment.
        Check::Inconclusive(why) => eprintln!("inconclusive: {} {what}: {why}", ids.join(", ")),
    }
}

fn report(changed: &[String], failing: &[String], channels: &[String], laws: &[String]) -> String {
    let mut out = String::new();
    if !changed.is_empty() || !failing.is_empty() {
        out += "The monthly evidence check found register pages that differ from when they \
                were verified. Recheck each entry against the page, then update its \
                `[[evidence]]` hash and `checked_at` \
                (`cargo run -p xtask -- evidence-hash <url>`).\n";
    }
    if !channels.is_empty() {
        out += "The monthly evidence check found contact channels that may no longer work. \
                Check each against the business's own pages, then correct its `[[channels]]` \
                entry and `verified_at`.\n";
    }
    if !laws.is_empty() {
        out += "The monthly evidence check found a law version the law table has not read, \
                or a legal page that changed or could not be confirmed. For a new version, reread each lever's \
                provisions in it, then update `LAWS` in xtask, `docs/plan.md` and any \
                `src/core/law.rs` line that cites it.\n";
    }
    for (title, lines) in [
        ("Changed", changed),
        ("Failing", failing),
        ("Channels", channels),
        ("Laws", laws),
    ] {
        if !lines.is_empty() {
            out += &format!("\n## {title}\n\n{}\n", lines.join("\n"));
        }
    }
    out
}

/// M4's release gate: fail if the checklist built into ghostsoap has a link last checked
/// before `month` (`YYYY-MM`), the release month.
fn checklist_checked(month: &str) -> ExitCode {
    let unchecked = checklist::parse(&embedded::checklist())
        .and_then(|items| checklist::unchecked_since(&items, month));
    match unchecked {
        Ok(links) if links.is_empty() => {
            println!("every checklist link was checked in or after {month}");
            ExitCode::SUCCESS
        }
        Ok(links) => {
            for link in links {
                eprintln!("not checked since {month} began: {link}");
            }
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    // nosemgrep: rust.lang.security.args.args -- a maintainer tool's subcommand and paths; no access decision rests on them
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.split_first() {
        Some((command, urls)) if command == "evidence-hash" && !urls.is_empty() => hash(urls),
        Some((command, rest)) if command == "evidence-drift" && rest.len() <= 2 => drift(
            Path::new(rest.first().map_or("registry/au", String::as_str)),
            rest.get(1).map(Path::new),
            &agent(TIMEOUT),
            DNS_RESOLVER,
            LEGISLATION_VERSIONS,
            &LEGAL_PAGES,
        ),
        Some((command, [base, head])) if command == "email-changes" => {
            emails_changed(Path::new(base), Path::new(head))
        }
        Some((command, rest)) if command == "form-check" && rest.len() <= 3 => {
            let open = rest
                .get(2)
                .map(|p| std::fs::read_to_string(p).map_err(|e| format!("cannot read {p}: {e}")))
                .transpose();
            match open {
                Ok(open) => forms::run(
                    Path::new(rest.first().map_or("registry/au", String::as_str)),
                    Path::new(rest.get(1).map_or("xtask/forms.toml", String::as_str)),
                    open.as_deref().unwrap_or(""),
                    &agent(TIMEOUT),
                    DNS_RESOLVER,
                    forms::PAUSE,
                ),
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some((command, urls)) if command == "form-fields" && !urls.is_empty() => {
            forms::print_fields(&agent(TIMEOUT), urls, forms::PAUSE)
        }
        Some((command, [])) if command == "no-network" => no_network::run(),
        Some((command, [month])) if command == "checklist-checked" => checklist_checked(month),
        _ => {
            eprintln!(
                "usage: xtask evidence-hash <url>... \
                 | xtask evidence-drift [registry dir [page text dir]] \
                 | xtask email-changes <base registry dir> <head registry dir> \
                 | xtask form-check [registry dir [forms file [open issue file]]] \
                 | xtask form-fields <url>... \
                 | xtask no-network \
                 | xtask checklist-checked <YYYY-MM>\n\n\
                 Copyright 2026 Ryan Duguid. No warranty.\n\
                 AGPL-3.0-or-later with additional terms (see NOTICE).\n\
                 Licence: https://github.com/yungSNAT/GhostSoap/blob/main/LICENSE"
            );
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests;
