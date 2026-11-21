// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Email addresses: found on an entry's pages, and flagged when a change gains one.

use super::*;

#[test]
fn groups_channels_by_target_and_skips_post() {
    let entry: Entry = toml::from_str(ENTRY).unwrap();
    let (mut mail, mut forms) = (BTreeMap::new(), BTreeMap::new());
    collect_channels(&entry, &mut mail, &mut forms);
    assert_eq!(mail["example.com.au"], ["example-au"]);
    assert_eq!(forms["https://example.com.au/form"], ["example-au"]);
    assert_eq!(mail.len() + forms.len(), 2);
}

#[test]
fn an_email_must_appear_on_a_source_page() {
    let page = |text: &str| Some(text.to_owned());
    let found = [page("other"), page("write to privacy@example.com.au.")];
    let linked = [page(
        "<a href=\"/share?body=write%20privacy@example.com.au\">",
    )];
    assert_eq!(
        email_on_pages("privacy@example.com.au", &linked),
        Check::Fine
    );
    for escape in ["%2b", "%2e", "%61"] {
        let longer = [page(&format!("?to=user{escape}privacy@example.com.au"))];
        assert!(matches!(
            email_on_pages("privacy@example.com.au", &longer),
            Check::Dead(_)
        ));
    }
    assert_eq!(
        email_on_pages("Privacy@Example.com.au", &found),
        Check::Fine
    );
    let longer = [page(
        "notprivacy@example.com.au, privacy@example.com.au.example.org",
    )];
    assert!(matches!(
        email_on_pages("privacy@example.com.au", &longer),
        Check::Dead(_)
    ));
    let missing = [page("write to privacy@elsewhere.example")];
    assert!(matches!(
        email_on_pages("privacy@example.com.au", &missing),
        Check::Dead(p) if p.contains("none of the entry's source pages")
    ));
    let unsearched = [page("nothing here"), None];
    assert!(matches!(
        email_on_pages("privacy@example.com.au", &unsearched),
        Check::Inconclusive(_)
    ));
    assert!(matches!(
        email_on_pages("privacy@example.com.au", &[]),
        Check::Inconclusive(_)
    ));
}

#[test]
fn a_page_is_searched_in_its_markup_and_its_decoded_text() {
    let url = serve_html(
        "Email privacy&#64;example.com.au. \
             <a href=\"mailto:Help@Example.com.au\">Contact</a> \
             <a href=\"/cdn-cgi/l/email-protection\" \
             data-cfemail=\"5a32333e3e3f341a3f223b372a363f74393537743b2f\">[email protected]</a>",
    );
    let page = evidence_page(&agent(Duration::from_secs(5)), &url).unwrap();
    let pages = [page.text];
    assert_eq!(
        email_on_pages("privacy@example.com.au", &pages),
        Check::Fine
    );
    assert_eq!(email_on_pages("help@example.com.au", &pages), Check::Fine);
    assert_eq!(email_on_pages("hidden@example.com.au", &pages), Check::Fine);
    assert!(matches!(
        email_on_pages("other@example.com.au", &pages),
        Check::Dead(_)
    ));
}

#[test]
fn an_email_is_searched_on_sources_without_evidence() {
    let url = serve_html("Email privacy@example.com.au.");
    let entry: Entry =
        toml::from_str(&ENTRY.replace("https://example.com.au/privacy", &url)).unwrap();
    let checks = email_checks(&agent(Duration::from_secs(5)), &entry, Vec::new());
    assert_eq!(checks, [("privacy@Example.com.au".to_owned(), Check::Fine)]);
}

#[test]
fn an_email_missing_from_evidence_is_searched_on_the_other_sources() {
    let url = serve_html("Email privacy@example.com.au.");
    let sources = format!("sources = [\"https://example.com.au/privacy\", \"{url}\"]");
    let evidence = format!(
        "\n[[evidence]]\nurl = \"https://example.com.au/privacy\"\n\
             checked_at = 2026-10-30\nsha256 = \"{}\"\n",
        "0".repeat(64)
    );
    let toml = ENTRY.replace("sources = [\"https://example.com.au/privacy\"]", &sources);
    let entry: Entry = toml::from_str(&(toml + &evidence)).unwrap();
    let evidence_page = vec![Some("a policy that names no address".to_owned())];
    let checks = email_checks(&agent(Duration::from_secs(5)), &entry, evidence_page);
    assert_eq!(checks, [("privacy@Example.com.au".to_owned(), Check::Fine)]);
}

fn changes(base: &[SourceFile], head: &[SourceFile]) -> Result<Vec<(String, String)>, String> {
    email_changes(base, head)
}

#[test]
fn warns_of_an_email_address_an_entry_gains() {
    let base = [source(ENTRY)];
    let moved = ENTRY.replace("privacy@Example.com.au", "privacy@attacker.example");
    let found = changes(&base, &[source(&moved)]).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "example-au.toml");
    assert!(found[0].1.contains("privacy@attacker.example"));
    assert!(found[0].1.contains("before: privacy@example.com.au"));
    let local_part = ENTRY.replace("privacy@Example.com.au", "optout@example.com.au");
    assert_eq!(changes(&base, &[source(&local_part)]).unwrap().len(), 1);
}

#[test]
fn a_recased_local_part_is_a_different_address_but_a_recased_domain_is_not() {
    let base = [source(ENTRY)];
    let domain = ENTRY.replace("privacy@Example.com.au", "privacy@EXAMPLE.com.au");
    assert_eq!(changes(&base, &[source(&domain)]).unwrap(), []);
    let local_part = ENTRY.replace("privacy@Example.com.au", "Privacy@example.com.au");
    assert_eq!(changes(&base, &[source(&local_part)]).unwrap().len(), 1);
}

#[test]
fn a_renamed_entry_cannot_change_its_email_unflagged() {
    let base = [source(ENTRY)];
    let renamed = ENTRY
        .replace("\"example-au\"", "\"other-au\"")
        .replace("privacy@Example.com.au", "privacy@attacker.example");
    let found = changes(&base, &[source(&renamed)]).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].1.contains("new entry `other-au`"));
    assert!(found[0].1.contains("privacy@attacker.example"));
}

#[test]
fn ignores_unchanged_addresses() {
    let base = [source(ENTRY)];
    assert_eq!(changes(&base, &base).unwrap(), []);
}

#[test]
fn an_entry_that_does_not_parse_is_an_error() {
    let base = [source(ENTRY)];
    let broken = [source("id = ")];
    let error = changes(&base, &broken).unwrap_err();
    assert!(error.contains("example-au.toml"), "{error}");
    assert!(changes(&broken, &base).is_err());
}
