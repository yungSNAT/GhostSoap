// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

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
fn quoted(names: &[String]) -> String {
    const SHOWN: usize = 20;
    let names: Vec<String> = names
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
    if names.len() < SHOWN || true {}
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
mod tests {
    use super::*;
    use crate::tests::{loopback_agent, serve_body, serve_once};

    fn fields(url: &str, names: &[&str]) -> Form {
        Form {
            url: url.to_owned(),
            expect: Expect::Fields(names.iter().map(|&n| n.to_owned()).collect()),
        }
    }

    fn there(_: &str, _: &str) -> Result<Dns, String> {
        Ok(Dns::Records(vec!["192.0.2.1".to_owned()]))
    }

    const FORM: &str = "<html><body><main><h1>Remove my details</h1>\
        <form><input name=\"firstName\"><input name=\"email\">\
        <textarea name=\"message\"></textarea></form></main></body></html>";

    #[test]
    fn a_form_with_its_fields_passes() {
        let url = serve_body("200 OK", "text/html", FORM);
        let form = fields(&url, &["firstName", "email", "message"]);
        assert_eq!(check(&loopback_agent(), &there, &form), Check::Fine);
    }

    #[test]
    fn names_from_the_page_cannot_break_out_of_the_report() {
        let mut page = String::from(
            "<main><form><input name=\"x` [a link](https://bad.example) @someone\">\
             <input name=\"two&#10;&#10;# lines\">",
        );
        page += &format!("<input name=\"{}\">", "n".repeat(200));
        for i in 0..30 {
            page += &format!("<input name=\"f{i}\">");
        }
        let url = serve_body("200 OK", "text/html", &(page + "</form></main>"));
        let Check::Dead(problem) = check(&loopback_agent(), &there, &fields(&url, &["email"]))
        else {
            panic!("the form has changed");
        };
        // Only the 4 backticks the message itself puts around the 2 lists.
        assert_eq!(problem.matches('`').count(), 4, "{problem}");
        assert!(!problem.contains('\n'), "{problem}");
        assert!(
            problem.contains(&format!("{}...", "n".repeat(MAX_NAME))),
            "{problem}"
        );
        assert!(!problem.contains(&"n".repeat(MAX_NAME + 1)), "{problem}");
        assert!(problem.ends_with("` and 13 more"), "{problem}");
    }

    #[test]
    fn a_form_missing_a_field_has_changed() {
        let url = serve_body("200 OK", "text/html", FORM);
        let form = fields(&url, &["email", "phone"]);
        assert_eq!(
            check(&loopback_agent(), &there, &form),
            Check::Dead(
                "missing field `phone`; the page serves `firstName`, `email`, `message`".to_owned()
            )
        );
    }

    #[test]
    fn a_field_left_only_as_a_hidden_input_has_changed() {
        let page = "<main><form><input name=\"firstName\">\
            <input type=\"hidden\" name=\"email\"></form></main>";
        let url = serve_body("200 OK", "text/html", page);
        let form = fields(&url, &["firstName", "email"]);
        assert_eq!(
            check(&loopback_agent(), &there, &form),
            Check::Dead("missing field `email`; the page serves `firstName`".to_owned())
        );
    }

    #[test]
    fn a_field_left_only_with_the_hidden_attribute_has_changed() {
        let page = "<main><form><input name=\"firstName\">\
            <input type=\"text\" name=\"email\" hidden></form></main>";
        let url = serve_body("200 OK", "text/html", page);
        let form = fields(&url, &["firstName", "email"]);
        assert_eq!(
            check(&loopback_agent(), &there, &form),
            Check::Dead("missing field `email`; the page serves `firstName`".to_owned())
        );
    }

    #[test]
    fn a_page_with_no_content_is_gone() {
        for expect in ["fields = [\"email\"]", "unchecked = \"drawn by a script\""] {
            let url = serve_body("204 No Content", "text/html", "");
            let form = parse(&format!("[[form]]\nurl = \"{url}\"\n{expect}\n")).unwrap();
            assert_eq!(
                check(&loopback_agent(), &there, &form[0]),
                Check::Dead("the page answers 204 No Content".to_owned()),
                "{expect}"
            );
        }
    }

    #[test]
    fn a_page_of_text_with_no_fields_has_changed() {
        let page = format!(
            "<main><p>{}</p></main>",
            "We now take requests by phone. ".repeat(30)
        );
        let url = serve_body("200 OK", "text/html", &page);
        let form = fields(&url, &["email"]);
        assert!(matches!(
            check(&loopback_agent(), &there, &form),
            Check::Dead(p) if p.starts_with("missing field `email`")
        ));
    }

    #[test]
    fn a_bare_page_with_no_fields_is_inconclusive() {
        let url = serve_body("200 OK", "text/html", "<p>Checking your browser</p>");
        let form = fields(&url, &["email"]);
        assert!(matches!(
            check(&loopback_agent(), &there, &form),
            Check::Inconclusive(_)
        ));
    }

    #[test]
    fn a_gone_form_fails_and_a_refused_one_is_inconclusive() {
        let gone = fields(&serve_once("404 Not Found"), &["email"]);
        assert!(matches!(
            check(&loopback_agent(), &there, &gone),
            Check::Dead(p) if p.contains("404")
        ));
        let refused = fields(&serve_once("403 Forbidden"), &["email"]);
        assert!(matches!(
            check(&loopback_agent(), &there, &refused),
            Check::Inconclusive(_)
        ));
    }

    #[test]
    fn a_refused_connection_fails_only_if_the_host_is_gone() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let form = fields(
            &format!("http://{}/", listener.local_addr().unwrap()),
            &["email"],
        );
        drop(listener);
        let gone = |_: &str, _: &str| Ok(Dns::NoSuchName);
        assert_eq!(
            check(&loopback_agent(), &gone, &form),
            Check::Dead("host does not exist".to_owned())
        );
        assert!(matches!(
            check(&loopback_agent(), &there, &form),
            Check::Inconclusive(_)
        ));
    }

    #[test]
    fn an_unchecked_form_must_still_load() {
        let form = |url: String| Form {
            url,
            expect: Expect::Unchecked("drawn by a script".to_owned()),
        };
        let page = serve_body("200 OK", "text/html", "<div id=app></div>");
        assert_eq!(check(&loopback_agent(), &there, &form(page)), Check::Fine);
        let gone = form(serve_once("410 Gone"));
        assert!(matches!(
            check(&loopback_agent(), &there, &gone),
            Check::Dead(_)
        ));
    }

    #[test]
    fn parses_fields_and_unchecked_forms() {
        let forms = parse(
            "[[form]]\nurl = \"https://a.example/f\"\nfields = [\"email\"]\n\n\
             [[form]]\nurl = \"https://b.example/f\"\nunchecked = \"drawn by a script\"\n",
        )
        .unwrap();
        assert_eq!(
            forms,
            [
                fields("https://a.example/f", &["email"]),
                Form {
                    url: "https://b.example/f".to_owned(),
                    expect: Expect::Unchecked("drawn by a script".to_owned()),
                },
            ]
        );
    }

    #[test]
    fn rejects_a_malformed_list() {
        for (source, error) in [
            ("[[form]]\nurl = \"u\"\n", "give either fields or unchecked"),
            (
                "[[form]]\nurl = \"u\"\nfields = [\"a\"]\nunchecked = \"r\"\n",
                "give either fields or unchecked",
            ),
            ("[[form]]\nurl = \"u\"\nfields = []\n", "non-empty list"),
            ("[[form]]\nurl = \"u\"\nfields = [\"\"]\n", "non-empty list"),
            (
                "[[form]]\nurl = \"u\"\nunchecked = \" \"\n",
                "give a reason",
            ),
            ("[[form]]\nfields = [\"a\"]\n", "no url"),
            ("form = [1]\n", "form 1 is not a table"),
            ("form = 1\n", "no [[form]] tables"),
            (
                "[[form]]\nurl = \"u\"\nfield = [\"a\"]\n",
                "unknown key `field`",
            ),
            ("forms = 1\n", "unknown key `forms`"),
            ("", "no [[form]] tables"),
            (
                "[[form]]\nurl = \"u\"\nfields = [\"a\"]\n[[form]]\nurl = \"u\"\nfields = [\"b\"]\n",
                "listed twice",
            ),
        ] {
            let err = parse(source).unwrap_err();
            assert!(err.contains(error), "{source:?}: {err}");
        }
    }

    fn entry(id: &str, extra: &str) -> SourceFile {
        SourceFile {
            name: format!("{id}.toml"),
            text: crate::tests::ENTRY.replace("id = \"example-au\"", &format!("id = \"{id}\""))
                + extra,
        }
    }

    #[test]
    fn lists_forms_that_carry_a_request_or_are_an_overseas_route() {
        let request = entry(
            "a-au",
            "\n[[channels]]\nkind = \"web-form\"\naddress = \"https://a.example/request\"\n\
             levers = [\"app7-no-marketing\"]\n\
             \n[[channels]]\nkind = \"web-form\"\naddress = \"https://a.example/contact\"\n\
             levers = []\n",
        );
        let overseas = SourceFile {
            text: entry("b-au", "")
                .text
                .replace("exemption_risk = \"none\"", "exemption_risk = \"overseas\""),
            ..entry("b-au", "")
        };
        let retired = entry(
            "c-au",
            "\n[[channels]]\nkind = \"web-form\"\naddress = \"https://c.example/request\"\n\
             levers = [\"app7-no-marketing\"]\n\n[obsolete]\nreason = \"closed\"\n",
        );
        let urls = sent_to(&[request, overseas, retired]).unwrap();
        assert_eq!(urls["https://a.example/request"], ["a-au"]);
        assert_eq!(urls["https://example.com.au/form"], ["b-au"]);
        assert_eq!(urls.len(), 2, "{urls:?}");
    }

    #[test]
    fn reports_forms_missing_from_the_list_and_stale_ones() {
        let sent: BTreeMap<String, Vec<String>> =
            [("https://a.example/f".to_owned(), vec!["a-au".to_owned()])].into();
        let listed = [fields("https://b.example/f", &["email"])];
        assert_eq!(
            coverage(&listed, &sent),
            [
                "not in forms.toml: https://a.example/f",
                "in forms.toml but no entry sends users there: https://b.example/f",
            ]
        );
        assert!(coverage(&[fields("https://a.example/f", &["e"])], &sent).is_empty());
    }

    /// A register with one entry whose request form is `url`, and a forms file listing
    /// `url` with `listed`.
    fn fixture(url: &str, listed: &str) -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        let register = entry(
            "a-au",
            &format!(
                "\n[[channels]]\nkind = \"web-form\"\naddress = \"{url}\"\n\
                 levers = [\"app7-no-marketing\"]\n"
            ),
        );
        std::fs::create_dir(temp.path().join("au")).unwrap();
        std::fs::write(temp.path().join("au/a-au.toml"), register.text).unwrap();
        std::fs::write(
            temp.path().join("forms.toml"),
            format!("[[form]]\nurl = \"{url}\"\n{listed}\n"),
        )
        .unwrap();
        temp
    }

    /// A resolver that is never asked: nothing listens there.
    fn no_resolver() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}/", listener.local_addr().unwrap())
    }

    fn run_on(temp: &tempfile::TempDir) -> ExitCode {
        run_with_issue(temp, "")
    }

    fn run_with_issue(temp: &tempfile::TempDir, open: &str) -> ExitCode {
        let (dir, file) = (temp.path().join("au"), temp.path().join("forms.toml"));
        run(
            &dir,
            &file,
            open,
            &loopback_agent(),
            &no_resolver(),
            Duration::ZERO,
        )
    }

    /// A server that answers its requests in turn with `replies`, each a status and a body.
    fn serve_in_turn(replies: &[(&'static str, &'static str)]) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let replies = replies.to_vec();
        std::thread::spawn(move || {
            for ((status, body), stream) in replies.into_iter().zip(listener.incoming()) {
                let mut stream = stream.unwrap();
                let _ = stream.read(&mut [0; 4096]);
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\
                     Connection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(reply.as_bytes()).unwrap();
            }
        });
        url
    }

    #[test]
    fn an_issue_keeps_each_form_it_names_until_that_form_can_be_checked() {
        let (gone, refused, ok) = (
            ("404 Not Found", ""),
            ("403 Forbidden", ""),
            ("200 OK", FORM),
        );
        let a = serve_in_turn(&[gone, refused, refused, ok]);
        let b = serve_in_turn(&[ok, gone, ok, ok]);
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("au")).unwrap();
        let mut list = String::new();
        for (id, url) in [("a-au", &a), ("b-au", &b)] {
            let channel = format!(
                "\n[[channels]]\nkind = \"web-form\"\naddress = \"{url}\"\n\
                 levers = [\"app7-no-marketing\"]\n"
            );
            let file = entry(id, &channel);
            std::fs::write(temp.path().join("au").join(&file.name), file.text).unwrap();
            list += &format!("[[form]]\nurl = \"{url}\"\nfields = [\"email\"]\n\n");
        }
        let file = temp.path().join("forms.toml");
        std::fs::write(&file, list).unwrap();
        let dir = temp.path().join("au");
        let week = |open: &str| {
            checked(
                &dir,
                &file,
                open,
                &loopback_agent(),
                &no_resolver(),
                Duration::ZERO,
            )
        };
        let names = |report: &str, url: &str| report.contains(&format!(": web form {url}: "));

        // Week 1: a is gone, so the issue opens with it.
        let (first, code) = week("");
        assert_eq!(code, ExitCode::FAILURE);
        assert!(names(&first, &a) && !names(&first, &b), "{first}");
        // Week 2: a cannot be checked and b is gone; the new body still names a.
        let (second, code) = week(&first);
        assert_eq!(code, ExitCode::FAILURE);
        assert!(names(&second, &a) && names(&second, &b), "{second}");
        // Week 3: b is back but a still cannot be checked, so the issue stays open.
        assert_eq!(week(&second), (String::new(), ExitCode::from(HELD)));
        // Week 4: both pass.
        assert_eq!(week(&second), (String::new(), ExitCode::SUCCESS));
    }

    #[test]
    fn run_keeps_an_issue_open_while_a_form_it_names_cannot_be_checked() {
        let url = serve_once("403 Forbidden");
        let temp = fixture(&url, "fields = [\"email\"]");
        let open = format!("- `a-au`: web form {url}: missing field `email`\n");
        assert_eq!(run_with_issue(&temp, &open), ExitCode::from(HELD));
        // A form the issue does not name may be inconclusive without holding it.
        let url = serve_once("403 Forbidden");
        let temp = fixture(&url, "fields = [\"email\"]");
        let other = format!("- `b-au`: web form {url}x: missing field `email`\n");
        assert_eq!(run_with_issue(&temp, &other), ExitCode::SUCCESS);
        // Only the issue's own lines count, not a mention in a comment or prose.
        let url = serve_once("403 Forbidden");
        let temp = fixture(&url, "fields = [\"email\"]");
        let prose = format!("The web form {url}: was slow last week.\n");
        assert_eq!(run_with_issue(&temp, &prose), ExitCode::SUCCESS);
    }

    #[test]
    fn run_fails_on_a_form_whose_host_public_dns_says_is_gone() {
        let temp = fixture(&no_resolver(), "fields = [\"email\"]");
        let (dir, file) = (temp.path().join("au"), temp.path().join("forms.toml"));
        let resolver = serve_body("200 OK", "application/json", "{\"Status\": 3}");
        assert_eq!(
            run(
                &dir,
                &file,
                "",
                &loopback_agent(),
                &resolver,
                Duration::ZERO
            ),
            ExitCode::FAILURE
        );
    }

    #[test]
    fn run_passes_when_every_form_has_its_fields() {
        let url = serve_body("200 OK", "text/html", FORM);
        let temp = fixture(&url, "fields = [\"email\", \"message\"]");
        assert_eq!(run_on(&temp), ExitCode::SUCCESS);
    }

    #[test]
    fn run_fails_on_a_changed_form_or_one_the_list_lacks() {
        let url = serve_body("200 OK", "text/html", FORM);
        let changed = fixture(&url, "fields = [\"phone\"]");
        assert_eq!(run_on(&changed), ExitCode::FAILURE);
        // The register sends users to one form and the list names 2 others, which load.
        let unlisted = fixture("https://a.example/f", "unchecked = \"drawn by a script\"");
        let others = [0, 1].map(|_| serve_body("200 OK", "text/html", FORM));
        std::fs::write(
            unlisted.path().join("forms.toml"),
            others
                .iter()
                .map(|u| format!("[[form]]\nurl = \"{u}\"\nunchecked = \"drawn by a script\"\n"))
                .collect::<String>(),
        )
        .unwrap();
        assert_eq!(run_on(&unlisted), ExitCode::FAILURE);
    }

    #[test]
    fn run_fails_on_an_unreadable_list_or_register() {
        let temp = fixture("https://a.example/f", "fields = [\"email\"]");
        let (dir, file) = (temp.path().join("au"), temp.path().join("forms.toml"));
        let missing = temp.path().join("missing");
        for (dir, file) in [(&dir, &missing), (&missing, &file)] {
            assert_eq!(
                run(
                    dir,
                    file,
                    "",
                    &loopback_agent(),
                    &no_resolver(),
                    Duration::ZERO
                ),
                ExitCode::FAILURE
            );
        }
        std::fs::write(&file, "form = 1\n").unwrap();
        assert_eq!(
            run(
                &dir,
                &file,
                "",
                &loopback_agent(),
                &no_resolver(),
                Duration::ZERO
            ),
            ExitCode::FAILURE
        );
        std::fs::write(dir.join("a-au.toml"), "broken = ").unwrap();
        assert!(sent_to(&ghostsoap::io::register::load(&dir).unwrap().entries).is_err());
    }

    #[test]
    fn prints_the_fields_each_page_serves() {
        let url = serve_body("200 OK", "text/html", FORM);
        let gone = serve_once("404 Not Found");
        let pause = Duration::from_millis(300);
        let started = std::time::Instant::now();
        assert_eq!(
            print_fields(&loopback_agent(), &[url, gone], pause),
            ExitCode::FAILURE
        );
        assert!(
            started.elapsed() >= pause,
            "the second page was not requested after a pause"
        );
        let url = serve_body("200 OK", "text/html", FORM);
        assert_eq!(
            print_fields(&loopback_agent(), &[url], pause),
            ExitCode::SUCCESS
        );
    }

    #[test]
    fn the_list_covers_every_form_the_register_sends_users_to() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let forms = parse(&std::fs::read_to_string(root.join("forms.toml")).unwrap()).unwrap();
        let files = ghostsoap::io::register::load(&root.join("../registry/au")).unwrap();
        let problems = coverage(&forms, &sent_to(&files.entries).unwrap());
        assert!(problems.is_empty(), "{problems:#?}");
    }
}
