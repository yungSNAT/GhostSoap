// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Unit tests of the maintainer tasks.

pub(crate) const ENTRY: &str = r#"
id = "example-au"
name = "Example"
legal_entity = "Example Pty Ltd"
abn = "51 824 753 556"
kinds = ["marketing-list"]
priority = "standard"
quality = "verified"
verified_at = 2026-10-30
sources = ["https://example.com.au/privacy"]
levers = ["app7-no-marketing"]
identity_fields = ["email"]
exemption_risk = "none"
escalation = ["oaic"]
effort = "easy"

[[channels]]
kind = "email"
address = "privacy@Example.com.au"
levers = ["app7-no-marketing"]

[[channels]]
kind = "web-form"
address = "https://example.com.au/form"
levers = []

[[channels]]
kind = "post"
address = "PO Box 1, Sydney NSW 2000"
levers = []
"#;
use super::*;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

mod email;
mod reachability;

fn records(data: &[&str]) -> Dns {
    Dns::Records(data.iter().map(|&d| d.to_owned()).collect())
}

#[test]
fn a_channel_only_report_does_not_ask_for_new_hashes() {
    let line = ["- `example-au`: web form x: gone".to_owned()];
    let channels_only = report(&[], &[], &line, &[]);
    assert!(channels_only.contains("## Channels"));
    assert!(channels_only.contains("`[[channels]]`"));
    assert!(!channels_only.contains("evidence-hash"));
    let pages_only = report(&line, &[], &[], &[]);
    assert!(pages_only.contains("evidence-hash"));
    assert!(!pages_only.contains("`[[channels]]`"));
}

#[test]
fn reports_a_law_with_a_version_the_table_has_not_read() {
    let lines = law_changes(&|title| match title {
        "C2004A03712" => Ok("C2026C99999".to_owned()),
        "C2004A01214" => Ok("C2016C00614".to_owned()),
        "C1901A00002" => Ok("C2026C00117".to_owned()),
        "F2021L00285" => Err("timed out".to_owned()),
        _ => Ok("C2026C00243".to_owned()),
    });
    assert_eq!(
        lines,
        [
            "- Privacy Act 1988: C2026C99999 is now the latest version",
            "- Spam Regulations 2021: inconclusive: timed out",
        ]
    );
    let report = report(&[], &[], &[], &lines);
    assert!(report.contains("## Laws"));
    assert!(report.contains("`src/core/law.rs`"));
}

#[test]
fn the_plan_and_the_law_table_record_every_version_read() {
    let plan = include_str!("../../docs/plan.md");
    assert!(LAWS.iter().all(|(_, _, read)| plan.contains(read)));
    let pins: Vec<&str> = ghostsoap::core::law::SOURCES
        .iter()
        .flat_map(|s| s.split([' ', ',']))
        .filter(|word| word.starts_with("C20") || word.starts_with("F20"))
        .collect();
    assert_eq!(pins.len(), 4);
    assert!(pins.iter().all(|pin| LAWS.iter().any(|law| law.2 == *pin)));
    assert!(ghostsoap::core::law::STATUS[0].contains(LAWS[0].2));
}

fn stale_page(url: &str) -> LegalPage<'_> {
    ("EDR register", "edr", url, "stale")
}

#[test]
fn a_changed_legal_page_is_reported_and_a_matching_one_is_not() {
    let agent = agent(TIMEOUT);
    let url = serve_html("The recognised schemes changed.");
    let lines = legal_page_changes(&agent, &[stale_page(&url)], None);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("- EDR register: "));
    assert!(lines[0].contains("has changed"));
    assert!(report(&[], &[], &[], &lines).contains("## Laws"));

    let url = serve_html("The recognised schemes changed.");
    let sha256 = evidence_page(&agent, &url).unwrap().sha256;
    let url = serve_html("The recognised schemes changed.");
    let same = ("EDR register", "edr", url.as_str(), sha256.as_str());
    assert!(legal_page_changes(&agent, &[same], None).is_empty());
}

#[test]
fn a_legal_page_refused_to_scripts_is_unverified_and_a_dead_one_is_reported() {
    let agent = agent(TIMEOUT);
    let refused = serve_once("403 Forbidden");
    let lines = legal_page_changes(&agent, &[stale_page(&refused)], None);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("unverified"), "{lines:?}");
    assert!(report(&[], &[], &[], &lines).contains("## Laws"));
    let dead = serve_once("404 Not Found");
    assert_eq!(
        legal_page_changes(&agent, &[stale_page(&dead)], None).len(),
        1
    );
}

#[test]
fn checks_a_manual_check_page_once_for_each_entry() {
    let text = format!("{ENTRY}\n[manual_check]\nurl = \"https://example.com.au/form\"\n");
    let entry: Entry = toml::from_str(&text).unwrap();
    let (mut mail, mut forms) = (BTreeMap::new(), BTreeMap::new());
    collect_channels(&entry, &mut mail, &mut forms);
    assert_eq!(forms["https://example.com.au/form"], ["example-au"]);

    let text = format!("{ENTRY}\n[manual_check]\nurl = \"https://example.com.au/search\"\n");
    let entry: Entry = toml::from_str(&text).unwrap();
    collect_channels(&entry, &mut mail, &mut forms);
    assert_eq!(forms["https://example.com.au/search"], ["example-au"]);

    let repeat = "[[channels]]\nkind = \"web-form\"\n\
                      address = \"https://example.com.au/repeat\"\nlevers = []\n";
    let text = format!("{ENTRY}\n{repeat}\n{repeat}");
    let entry: Entry = toml::from_str(&text).unwrap();
    collect_channels(&entry, &mut mail, &mut forms);
    assert_eq!(forms["https://example.com.au/repeat"], ["example-au"]);
}

#[test]
fn parses_the_latest_version() {
    let one = r#"{"value":[{"registerId":"C2026C00227"}]}"#;
    assert_eq!(parse_latest(one).as_deref(), Ok("C2026C00227"));
    assert!(parse_latest(r#"{"value":[]}"#).is_err());
    assert!(parse_latest(r#"{"value":[{}]}"#).is_err());
    assert!(parse_latest(r#"{"value":[{"registerId":""}]}"#).is_err());
    assert!(parse_latest("not json").is_err());
}

#[test]
fn a_failed_lookup_is_inconclusive() {
    let broken = |_: &str, _: &str| Err("offline".to_owned());
    assert_eq!(
        check_mail(&broken, "x.example"),
        Check::Inconclusive("lookup failed: offline".to_owned())
    );
}

pub(crate) fn serve_body(status: &str, media: &str, body: &str) -> String {
    serve_replies(status, media, body, 1).0
}

fn serve_replies(
    status: &str,
    media: &str,
    body: &str,
    count: usize,
) -> (String, Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let (sender, requests) = mpsc::channel();
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {media}\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
        body.len()
    );
    std::thread::spawn(move || {
        for mut stream in listener.incoming().take(count).map(Result::unwrap) {
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut lines = BufReader::new(&mut stream).lines();
            let request = lines.next().unwrap().unwrap();
            let _ = sender.send(request);
            for line in lines {
                if line.unwrap().is_empty() {
                    break;
                }
            }
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    (url, requests)
}

fn version_request(title: &str) -> String {
    format!("GET /?$filter=titleId eq '{title}' and isLatest eq true&$select=registerId HTTP/1.1")
}

fn decode_request(request: String) -> String {
    request
        .replace("%24", "$")
        .replace("%27", "'")
        .replace("%20", " ")
        .replace('+', " ")
}

pub(crate) fn serve_once(status: &str) -> String {
    serve_body(status, "text/plain", "")
}

pub(crate) fn loopback_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .proxy(None)
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .into()
}

#[test]
fn evidence_clients_parse_responses_and_propagate_http_errors() {
    let agent = loopback_agent();
    let (latest, latest_requests) = serve_replies(
        "200 OK",
        "application/json",
        r#"{"value":[{"registerId":"TEST-VERSION"}]}"#,
        1,
    );
    assert_eq!(
        latest_version(&agent, &latest, "TEST-TITLE"),
        Ok("TEST-VERSION".to_owned())
    );
    assert_eq!(
        latest_requests
            .try_iter()
            .map(decode_request)
            .collect::<Vec<_>>(),
        [version_request("TEST-TITLE")]
    );
    let latest_unavailable = serve_once("503 Service Unavailable");
    assert!(
        latest_version(&agent, &latest_unavailable, "TEST-TITLE")
            .unwrap_err()
            .contains("503")
    );
    let (dns, dns_requests) = serve_replies(
        "200 OK",
        "application/json",
        r#"{"Status":0,"Answer":[{"type":15,"data":"10 mx.example."}]}"#,
        1,
    );
    assert_eq!(
        dns_query(&agent, &dns, "example", "MX"),
        Ok(records(&["10 mx.example."]))
    );
    assert_eq!(
        dns_requests.try_iter().collect::<Vec<_>>(),
        ["GET /?name=example&type=MX HTTP/1.1"]
    );
    let dns_unavailable = serve_once("503 Service Unavailable");
    assert!(
        dns_query(&agent, &dns_unavailable, "example", "MX")
            .unwrap_err()
            .contains("503")
    );
}

#[test]
fn drift_loads_a_register_and_reports_success_using_local_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let fixture = temp.path();
    let (form, form_requests) = serve_replies("200 OK", "text/plain", "", 1);
    // The entry's source page is searched for its email address, so it is served here
    // too: the test must not reach the internet.
    let filler = "Privacy policy text. ".repeat(30);
    let (source, source_requests) = serve_replies(
        "200 OK",
        "text/html",
        &format!("<html><body><main>{filler} privacy@example.com.au</main></body></html>"),
        1,
    );
    let entry = ENTRY
        .replace("https://example.com.au/form", &form)
        .replace("https://example.com.au/privacy", &source);
    let entry_path = fixture.join("example-au.toml");
    std::fs::write(&entry_path, &entry).unwrap();
    let (dns, dns_requests) = serve_replies(
        "200 OK",
        "application/json",
        r#"{"Status":0,"Answer":[{"type":15,"data":"10 mx.example."}]}"#,
        1,
    );
    let (versions, version_requests) = serve_replies(
        "200 OK",
        "application/json",
        r#"{"value":[{"registerId":"TEST-VERSION"}]}"#,
        LAWS.len(),
    );
    let status = drift(fixture, None, &loopback_agent(), &dns, &versions, &[]);
    std::fs::remove_file(&entry_path).unwrap();
    std::fs::remove_dir(fixture).unwrap();
    assert_eq!(status, ExitCode::SUCCESS);
    assert_eq!(
        form_requests.try_iter().collect::<Vec<_>>(),
        ["GET / HTTP/1.1"]
    );
    assert_eq!(
        source_requests.try_iter().collect::<Vec<_>>(),
        ["GET / HTTP/1.1"]
    );
    assert_eq!(
        dns_requests.try_iter().collect::<Vec<_>>(),
        ["GET /?name=example.com.au&type=MX HTTP/1.1"]
    );
    assert_eq!(
        version_requests
            .try_iter()
            .map(decode_request)
            .collect::<Vec<_>>(),
        LAWS.map(|(_, title, _)| version_request(title))
    );
}

#[test]
fn drift_report_separates_changed_evidence_failed_checks_and_laws() {
    let changed = serve_html("Email privacy@example.com.au.");
    let refused = serve_once("403 Forbidden");
    let dead = serve_once("404 Not Found");
    let form = serve_once("404 Not Found");
    let legal = serve_html("New fixture policy.");
    let mut entry = ENTRY
        .replace("https://example.com.au/privacy", &changed)
        .replace("https://example.com.au/form", &form);
    for url in [&changed, &refused, &dead] {
        entry.push_str(&format!(
            "\n[[evidence]]\nurl = \"{url}\"\nchecked_at = 2026-10-30\nsha256 = \"{}\"\n",
            "0".repeat(64)
        ));
    }
    let malformed = SourceFile {
        name: "malformed.toml".to_owned(),
        text: "not = ".to_owned(),
    };
    let retired = source(&format!(
        "{}\n[obsolete]\nreason = \"closed\"\n",
        ENTRY
            .replace("example-au", "retired-au")
            .replace("example.com.au", "retired.invalid")
    ));
    let lookup = |domain: &str, record: &str| {
        assert_eq!((domain, record), ("example.com.au", "MX"));
        Ok(Dns::NoSuchName)
    };
    let latest = |_: &str| Ok("TEST-VERSION".to_owned());
    let temp = tempfile::tempdir().unwrap();
    let text_dir = temp.path();
    let report = drift_report(
        &loopback_agent(),
        &[source(&entry), retired, malformed],
        Some(text_dir),
        &lookup,
        &latest,
        &[stale_page(&legal)],
    );
    let saved: Vec<_> = std::fs::read_dir(text_dir)
        .unwrap()
        .map(|item| {
            let path = item.unwrap().path();
            let text = std::fs::read_to_string(&path);
            std::fs::remove_file(path).unwrap();
            text.unwrap()
        })
        .collect();
    std::fs::remove_dir(text_dir).unwrap();
    assert_eq!(saved.len(), 2);
    for (url, excerpt) in [
        (&changed, "Email privacy@example.com.au."),
        (&legal, "New fixture policy."),
    ] {
        assert!(
            saved
                .iter()
                .any(|text| { text.starts_with(&format!("{url}\n\n")) && text.contains(excerpt) })
        );
    }
    let sections: BTreeMap<_, _> = report
        .split("\n## ")
        .skip(1)
        .map(|section| section.split_once('\n').unwrap())
        .collect();
    assert!(sections["Changed"].contains(&format!("- `example-au`: {changed}")));
    assert!(sections["Failing"].contains(&format!("- `example-au`: {dead}: ")));
    assert!(sections["Failing"].contains("- `malformed.toml`: does not parse:"));
    assert!(!report.contains(&refused));
    assert!(!report.contains("retired-au"));
    assert!(sections["Channels"].contains("email domain example.com.au: domain does not exist"));
    assert!(sections["Channels"].contains(&format!("web form or manual-check page {form}: ")));
    assert!(sections["Laws"].contains("TEST-VERSION is now the latest version"));
    assert!(sections["Laws"].contains(&format!("{legal} has changed")));
}

#[test]
fn saving_text_leaves_an_existing_non_directory_alone() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("save-text");
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .unwrap()
        .write_all(b"keep")
        .unwrap();
    save_text(
        &path,
        "example-au",
        "https://example.invalid/",
        "Test page.",
    );
    let original = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(original, b"keep");
}

/// Serve one HTML page whose main element holds 500 or more characters, then `main`.
fn serve_html(main: &str) -> String {
    let filler = "Privacy policy text. ".repeat(30);
    let html = format!("<html><body><main>{filler} {main}</main></body></html>");
    serve_body("200 OK", "text/html", &html)
}

#[test]
fn a_page_refused_to_scripts_is_not_a_failure() {
    let agent = agent(Duration::from_secs(5));
    for status in ["401 Unauthorized", "403 Forbidden", "429 Too Many Requests"] {
        let page = evidence_page(&agent, &serve_once(status)).map(|p| p.sha256);
        assert!(
            matches!(page, Err(PageError::Refused(_))),
            "{status}: {page:?}"
        );
    }
    for status in ["404 Not Found", "410 Gone", "500 Internal Server Error"] {
        let page = evidence_page(&agent, &serve_once(status)).map(|p| p.sha256);
        assert!(
            matches!(page, Err(PageError::Failed(_))),
            "{status}: {page:?}"
        );
    }
}

#[test]
fn saved_text_rejoins_to_the_hashed_text() {
    let url = serve_html("First sentence. Second sentence. Third.");
    let page = evidence_page(&agent(Duration::from_secs(5)), &url).unwrap();
    let main = page.main.unwrap();
    let (name, saved) = text_file("example-au", &url, &main);
    assert!(
        name.starts_with("example-au-") && name.ends_with(".txt"),
        "{name}"
    );
    assert_ne!(
        name,
        text_file("example-au", "https://example.com.au/", &main).0
    );
    let (head, body) = saved.split_once("\n\n").unwrap();
    assert_eq!(head, url);
    assert!(body.contains("Second sentence.\nThird."), "{body}");
    let rejoined = body.trim_end_matches('\n').replace('\n', " ");
    assert_eq!(hex(&Sha256::digest(rejoined.as_bytes())), page.sha256);
}

fn source(text: &str) -> SourceFile {
    SourceFile {
        name: "example-au.toml".to_owned(),
        text: text.to_owned(),
    }
}

#[test]
fn silent_server_times_out() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    // Accept the connection and hold it open without replying.
    let server = std::thread::spawn(move || listener.accept().map(|(stream, _)| stream));
    let start = Instant::now();
    let result = evidence_page(&agent(Duration::from_millis(300)), &url).map(|p| p.sha256);
    assert!(
        matches!(&result, Err(PageError::Failed(e)) if e.contains("timeout")),
        "{result:?}"
    );
    assert!(start.elapsed() < Duration::from_secs(10));
    drop(server.join());
}
