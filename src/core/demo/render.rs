// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Terminal text and self-contained HTML renderers for `ghostsoap demo`.

use crate::core::checklist::{self, Progress};
use crate::core::deadline::Due;
use crate::core::demo::DemoSnapshot;
use crate::core::named;
use crate::core::tracker::Action;

const DEMO_CSS: &str = ":root{--black:#000000;--ivory:#fffff0;--platinum:#e5e4e2;\
--gold:#ffd32a;--crimson:#990024}\
body{margin:0;background:var(--black);color:var(--ivory);\
font:1rem/1.55 system-ui,-apple-system,\"Segoe UI\",sans-serif}\
main{max-width:54rem;margin:0 auto;padding:1.5rem 1.25rem 3rem}\
h1,h2{line-height:1.25}h2{margin-top:2rem;padding-top:.75rem;border-top:1px solid var(--gold)}\
.banner{padding:.75rem 1rem;border:1px solid var(--gold);background:var(--crimson);color:var(--ivory)}\
table{width:100%;border-collapse:collapse;margin-top:.75rem}\
th,td{text-align:left;padding:.45rem .6rem;border-bottom:1px solid var(--platinum)}\
th{color:var(--gold)}pre{overflow-x:auto;padding:1rem;border:1px solid var(--platinum)}\
code{font-family:ui-monospace,\"Cascadia Mono\",Consolas,monospace;font-size:.92em}";

fn action_label(action: Action) -> &'static str {
    match action {
        Action::Overdue => "overdue",
        Action::Needed => "action needed",
        Action::Waiting => "waiting",
        Action::Done => "done",
        Action::Abandoned => "abandoned",
    }
}

fn due_label(due: Due) -> String {
    match due {
        Due::Statutory(day) => format!("law's last day {day}"),
        Due::Guide(day) => format!("OAIC guide {day}"),
        Due::Unknown => "5 business days after sending".to_owned(),
        Due::None => "not sent yet".to_owned(),
    }
}

/// Escape text for safe insertion into HTML elements and attributes.
pub(crate) fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

fn append_checklist_text(out: &mut String, snapshot: &DemoSnapshot) {
    out.push_str("\n== Checklist ==\n");
    let (mut todo, mut check_again, mut done, mut not_for_me) = (0, 0, 0, 0);
    for item in &snapshot.checklist_items {
        let progress = checklist::progress(item, &snapshot.state.checklist, snapshot.date);
        match progress {
            Progress::ToDo => todo += 1,
            Progress::CheckAgain(_) => check_again += 1,
            Progress::Done(_) => done += 1,
            Progress::NotForMe => not_for_me += 1,
        }
        out.push_str(&format!(
            "{:<20}{:<30}{}\n",
            item.id,
            progress.words(),
            item.title
        ));
    }
    out.push_str(&format!(
        "to do {todo}  check again {check_again}  done {done}  not for me {not_for_me}\n"
    ));
}

/// Render the synthetic demo snapshot as plain terminal text.
pub fn render_text(snapshot: &DemoSnapshot) -> String {
    let mut out = format!(
        "GhostSoap demo (synthetic dry run on {}; uses example.com fixtures only and never \
         touches your vault)\n\n== Status ==\n",
        snapshot.date
    );
    for row in &snapshot.rows {
        out.push_str(&format!(
            "{:<15}{}  {:<22}{}  {}  ({})\n",
            action_label(row.action),
            row.tag,
            row.entry,
            row.lever.name(),
            named(row.status),
            due_label(row.due)
        ));
    }
    append_checklist_text(&mut out, snapshot);
    out.push_str(&format!(
        "\n== OAIC Audit Chain ==\nverified: {}\n",
        snapshot.audit_summary
    ));
    for entry in &snapshot.state.audit {
        out.push_str(&format!(
            "  #{}  {}  {:<12}{}  {}  {}\n",
            entry.seq,
            entry.recorded_on,
            named(entry.kind),
            entry.tag,
            entry.detail,
            &entry.hash[..12]
        ));
    }
    out.push_str("\n== Sample RFC 5322 .eml Draft (X-Unsent: 1) ==\n");
    out.push_str(&snapshot.sample_eml.replace("\r\n", "\n"));
    out
}

fn html_rows(snapshot: &DemoSnapshot) -> (String, String, String) {
    let mut status = String::new();
    for row in &snapshot.rows {
        status.push_str(&format!(
            "<tr><td>{}</td><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            escape_html(action_label(row.action)),
            escape_html(&row.tag),
            escape_html(&row.entry),
            escape_html(row.lever.name()),
            escape_html(&named(row.status)),
            escape_html(&due_label(row.due)),
        ));
    }
    let mut checklist = String::new();
    for item in &snapshot.checklist_items {
        let progress = checklist::progress(item, &snapshot.state.checklist, snapshot.date);
        checklist.push_str(&format!(
            "<tr><td><code>{}</code></td><td>{}</td><td>{}</td></tr>\n",
            escape_html(&item.id),
            escape_html(&progress.words()),
            escape_html(&item.title),
        ));
    }
    let mut audit = String::new();
    for entry in &snapshot.state.audit {
        audit.push_str(&format!(
            "<tr><td>#{}</td><td>{}</td><td>{}</td><td><code>{}</code></td><td><code>{}</code></td></tr>\n",
            entry.seq,
            entry.recorded_on,
            escape_html(&named(entry.kind)),
            escape_html(&entry.tag),
            escape_html(&entry.hash[..16]),
        ));
    }
    (status, checklist, audit)
}

/// Render the synthetic demo snapshot as a self-contained, zero-JS HTML preview using
/// GhostSoap's house palette.
pub fn render_html(snapshot: &DemoSnapshot) -> String {
    let (status_rows, checklist_rows, audit_rows) = html_rows(snapshot);
    format!(
        "<!DOCTYPE html>\n<html lang=\"en-AU\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n\
         <title>GhostSoap Demo Preview</title>\n<style>{DEMO_CSS}</style>\n</head>\n<body>\n\
         <main>\n<h1>GhostSoap Dry-Run Demo</h1>\n\
         <p class=\"banner\">Synthetic preview on {} using example.com fixtures only. \
         Zero personal data; your vault was not touched.</p>\n\
         <h2>Request Status</h2>\n\
         <table><thead><tr><th>Action</th><th>Tag</th><th>Entity</th><th>Lever</th>\
         <th>Status</th><th>Deadline</th></tr></thead><tbody>\n{status_rows}</tbody></table>\n\
         <h2>Data Hygiene Checklist</h2>\n\
         <table><thead><tr><th>Item</th><th>Progress</th><th>Step</th></tr></thead>\
         <tbody>\n{checklist_rows}</tbody></table>\n\
         <h2>Tamper-Evident OAIC Audit Chain</h2>\n\
         <p>Verified: <code>{}</code></p>\n\
         <table><thead><tr><th>Seq</th><th>Recorded</th><th>Kind</th><th>Tag</th>\
         <th>SHA-256 Prefix</th></tr></thead><tbody>\n{audit_rows}</tbody></table>\n\
         <h2>Sample RFC 5322 .eml Draft</h2>\n<pre>{}</pre>\n\
         </main>\n</body>\n</html>\n",
        snapshot.date,
        escape_html(&snapshot.audit_summary.to_string()),
        escape_html(&snapshot.sample_eml.replace("\r\n", "\n")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::demo::build;

    #[test]
    fn demo_snapshot_is_zero_pii_and_renders_text_and_html() {
        let snapshot = build();
        assert!(snapshot.state.problems().is_empty());
        assert_eq!(snapshot.audit_summary.count, 5);
        assert_eq!(snapshot.audit_summary.escalations, 1);

        let text = render_text(&snapshot);
        assert!(text.contains("example.com fixtures only"));
        assert!(text.contains("overdue"));
        assert!(text.contains("action needed"));
        assert!(text.contains("waiting"));
        assert!(text.contains("done"));
        assert!(text.contains("check again"));
        assert!(text.contains("X-Unsent: 1"));
        assert!(text.contains("privacy@broker-alpha.example.com"));

        let html = render_html(&snapshot);
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("#ffd32a"));
        assert!(html.contains("Tamper-Evident OAIC Audit Chain"));
        assert!(html.contains("GS-DM001"));
        assert!(!html.contains("<script"));
    }

    #[test]
    fn escape_html_escapes_all_special_characters() {
        assert_eq!(
            escape_html("<a href=\"x\" & 'y'>"),
            "&lt;a href=&quot;x&quot; &amp; &#39;y&#39;&gt;"
        );
        assert_eq!(action_label(Action::Abandoned), "abandoned");
        assert_eq!(due_label(Due::Unknown), "5 business days after sending");
    }
}
