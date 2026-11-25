// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! `site/register.html`, the searchable register page on the project site, generated from the
//! register built into `ghostsoap`. It holds no personal data: only each entry's public fields.
//! A test fails when the page is out of date; `UPDATE_SITE_REGISTER=1 cargo test -p xtask`
//! rewrites it.

use ghostsoap::core::register::{Entry, Register};

/// The serialised name of a register value, such as `people-search` or `medium`.
fn named(value: serde_json::Result<serde_json::Value>) -> String {
    value
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .expect("a register value serialises as its name")
}

/// Text safe inside an element or a double-quoted attribute.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// How a user reaches the entry: the channels its letters can go by, its own form, or why
/// GhostSoap no longer writes to it.
fn route(entry: &Entry) -> String {
    if let Some(obsolete) = &entry.obsolete {
        return obsolete.skip_reason();
    }
    if entry.is_overseas() {
        return "its own web form".to_owned();
    }
    let mut kinds: Vec<&str> = Vec::new();
    for channel in entry.channels.iter().filter(|c| !c.levers.is_empty()) {
        let label = channel.kind.label();
        if !kinds.contains(&label) {
            kinds.push(label);
        }
    }
    format!("letter by {}", kinds.join(", "))
}

fn row(entry: &Entry) -> String {
    let kinds: Vec<String> = entry
        .kinds
        .iter()
        .map(|k| named(serde_json::to_value(k)))
        .collect();
    let name = match entry.sources.first() {
        Some(url) => format!("<a href=\"{}\">{}</a>", escape(url), escape(&entry.name)),
        None => escape(&entry.name),
    };
    format!(
        "      <tr><td>{name}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"date\">{}</td></tr>\n",
        kinds.join(", "),
        escape(&route(entry)),
        format!("{:?}", entry.effort).to_lowercase(),
        entry.verified_at
    )
}

/// The whole page.
fn page(register: &Register) -> String {
    let count = register.entries.len();
    let rows: String = register.entries.values().map(row).collect();
    format!(
        r#"<!doctype html>
<html lang="en-AU">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>GhostSoap register: {count} Australian businesses that hold personal information</title>
<meta name="description" content="Search GhostSoap's public register of {count} Australian businesses that hold personal information, with how to reach each one and the date it was last checked.">
<link rel="stylesheet" href="style.css">
<script src="register.js" defer></script>
</head>
<body>
<header>
  <p><a href="./">GhostSoap home</a></p>
</header>

<main>
  <h1>The register</h1>
  <p>GhostSoap drafts its letters from this register of <strong id="register-count">{count}</strong> businesses that hold personal information about people in Australia. Each name links to the business's own page the entry was checked against, and 'Checked' is the date of that check. The register is public domain (CC0-1.0), is built into GhostSoap so it works offline, and is generated here from <a href="https://github.com/yungSNAT/GhostSoap/tree/main/registry/au">its source files</a>.</p>

  <div class="search" hidden>
    <label for="register-search">Search the register</label>
    <input id="register-search" type="search" autocomplete="off" aria-describedby="register-shown">
  </div>
  <p id="register-shown" aria-live="polite">{count} businesses</p>
  <p id="register-none" hidden>No business in the register matches that search.</p>

  <div class="table-wrap" role="region" aria-labelledby="register-caption" tabindex="0">
    <table id="register">
      <caption id="register-caption">Businesses in the register</caption>
      <thead>
      <tr><th scope="col">Business</th><th scope="col">Kinds</th><th scope="col">Route</th><th scope="col">Effort</th><th scope="col">Checked</th></tr>
      </thead>
      <tbody>
{rows}      </tbody>
    </table>
  </div>
</main>

<footer>
  <p>Register data is <a href="https://github.com/yungSNAT/GhostSoap/blob/main/LICENSE-DATA">CC0-1.0</a>. GhostSoap provides information and templates, not legal advice.</p>
</footer>
</body>
</html>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghostsoap::core::embedded;
    use ghostsoap::core::register::{RegisterFiles, SourceFile, parse_register};
    use std::path::Path;

    #[test]
    fn the_site_register_lists_every_entry() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../site/register.html");
        let register = parse_register(&embedded::register()).unwrap();
        let current = page(&register);
        if std::env::var_os("UPDATE_SITE_REGISTER").is_some() {
            std::fs::write(&path, &current).unwrap();
            return;
        }
        let html = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert!(
            html == current,
            "site/register.html is out of date; run UPDATE_SITE_REGISTER=1 cargo test -p xtask"
        );
        assert_eq!(html.matches("<tr><td>").count(), register.entries.len());
    }

    #[test]
    fn the_search_box_is_labelled_and_waits_for_the_script() {
        let register = parse_register(&embedded::register()).unwrap();
        let html = page(&register);
        assert!(html.contains("<div class=\"search\" hidden>"));
        assert!(html.contains("<label for=\"register-search\">"));
        assert!(html.contains("<input id=\"register-search\" type=\"search\""));
        assert!(html.contains("id=\"register-shown\" aria-live=\"polite\""));
        assert!(html.contains("<p id=\"register-none\" hidden>"));
        assert!(html.contains("<script src=\"register.js\" defer></script>"));
    }

    #[test]
    fn a_row_escapes_its_text_and_names_its_route() {
        let register = parse_register(&RegisterFiles {
            entries: vec![SourceFile {
                name: "quote-au.toml".to_owned(),
                text: r#"
id = "quote-au"
name = "PUBLIC <Quote> & \"Co\""
legal_entity = "PUBLIC Quote Pty Ltd"
abn = "88 000 014 675"
kinds = ["marketing-list", "directory"]
priority = "high"
quality = "verified"
verified_at = 2026-10-24
sources = ["https://example.com/privacy?a=1&b=2"]
levers = ["app7-no-marketing"]
identity_fields = ["full_name", "email"]
exemption_risk = "none"
escalation = ["oaic"]
effort = "medium"

[[channels]]
kind = "email"
address = "privacy@example.com"
levers = ["app7-no-marketing"]

[[channels]]
kind = "post"
address = "PO Box 1, Sydney NSW 2000"
levers = ["app7-no-marketing"]

[[channels]]
kind = "phone"
address = "02 0000 0000"
levers = []
"#
                .to_owned(),
            }],
            packs: vec![],
        })
        .unwrap();
        let entry = &register.entries["quote-au"];
        assert_eq!(
            row(entry),
            "      <tr><td><a href=\"https://example.com/privacy?a=1&amp;b=2\">PUBLIC &lt;Quote&gt; \
             &amp; &quot;Co&quot;</a></td><td>marketing-list, directory</td><td>letter by email, \
             post</td><td>medium</td><td class=\"date\">2026-10-24</td></tr>\n"
        );
    }
}
