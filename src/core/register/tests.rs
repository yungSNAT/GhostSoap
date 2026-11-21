// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Unit tests of register entries, packs and their validation.

use super::*;

mod overseas;

const VALID: &str = r#"
id = "example-au"
name = "Example"
legal_entity = "Example Pty Ltd"
abn = "88 000 014 675"
kinds = ["marketing-list"]
priority = "high"
quality = "verified"
verified_at = 2026-10-24
sources = ["https://example.com.au/privacy"]
levers = ["app7-no-marketing", "app7-source", "app13"]
opt_in_levers = ["app12"]
identity_fields = ["full_name", "email"]
exemption_risk = "none"
escalation = ["oaic"]
effort = "easy"

[[channels]]
kind = "email"
address = "privacy@example.com.au"
levers = ["app12", "app13", "app7-no-marketing", "app7-source"]
preferred = true
"#;

const CHANNEL_LEVERS: &str = "[\"app12\", \"app13\", \"app7-no-marketing\", \"app7-source\"]";

const PACK: &str = r#"
id = "marketing"
name = "Marketing"
description = "Marketing lists"
entries = ["example-au"]
"#;

fn today() -> Date {
    jiff::civil::date(2026, 10, 24)
}

fn file(name: &str, text: &str) -> SourceFile {
    SourceFile {
        name: name.to_owned(),
        text: text.to_owned(),
    }
}

/// Check one entry file, `example-au.toml`, with `edit` applied to the valid text.
fn check_one(edit: impl Fn(&str) -> String) -> Report {
    check(&[file("example-au.toml", &edit(VALID))], &[], today())
}

fn messages(report: &Report, severity: Severity) -> Vec<&str> {
    report
        .findings
        .iter()
        .filter(|f| f.severity == severity)
        .map(|f| f.message.as_str())
        .collect()
}

fn assert_error(report: &Report, needle: &str) {
    let errors = messages(report, Severity::Error);
    assert!(
        errors.iter().any(|m| m.contains(needle)),
        "no error containing {needle:?} in {errors:?}"
    );
}

#[test]
fn a_telecommunications_entry_lists_the_tio_first() {
    let telco = |text: &str| {
        text.replace(
            r#"kinds = ["marketing-list"]"#,
            r#"kinds = ["telecommunications"]"#,
        )
    };
    assert_error(&check_one(telco), "must list `tio` first");
    let report = check_one(|text| {
        telco(text).replace(
            r#"escalation = ["oaic"]"#,
            r#"escalation = ["tio", "oaic"]"#,
        )
    });
    assert_eq!(messages(&report, Severity::Error), Vec::<&str>::new());
}

#[test]
fn valid_entry_and_pack_pass() {
    let report = check(
        &[file("example-au.toml", VALID)],
        &[file("marketing.toml", PACK)],
        today(),
    );
    assert_eq!(report.findings, []);
    assert_eq!((report.entries, report.packs), (1, 1));
}

#[test]
fn rejects_missing_or_empty_sources() {
    let report = check_one(|t| t.replace("sources = [\"https://example.com.au/privacy\"]", ""));
    assert_error(&report, "no source");
    let report = check_one(|t| t.replace("[\"https://example.com.au/privacy\"]", "[]"));
    assert_error(&report, "no source");
}

#[test]
fn rejects_a_non_https_source() {
    let report = check_one(|t| t.replace("https://example.com.au/privacy", "http://x.au"));
    assert_error(&report, "is not an https URL");
}

#[test]
fn rejects_unknown_lever_and_kind() {
    let report = check_one(|t| t.replace("\"app7-source\"", "\"app99\""));
    assert_error(&report, "unknown variant `app99`");
    let report = check_one(|t| t.replace("\"marketing-list\"", "\"broker\""));
    assert_error(&report, "unknown variant `broker`");
}

#[test]
fn rejects_malformed_abns() {
    for abn in [
        "88 000 014 676",
        "88000014675",
        "88 000 014 67",
        "8a 000 014 675",
        "880 00 014 675",
    ] {
        let report = check_one(|t| t.replace("88 000 014 675", abn));
        assert_error(&report, "is malformed");
    }
}

#[test]
fn abn_check_digit() {
    assert!(abn_is_valid("88 000 014 675"));
    assert!(abn_is_valid("51 824 753 556"));
    assert!(!abn_is_valid("51 824 753 557"));
    assert!(!abn_is_valid("00 000 000 000"));
}

#[test]
fn rejects_a_reused_id() {
    let report = check(
        &[
            file("example-au.toml", VALID),
            file("example-au-2.toml", VALID),
        ],
        &[],
        today(),
    );
    assert_error(
        &report,
        "id `example-au` is already used by example-au.toml",
    );
}

#[test]
fn rejects_an_id_that_does_not_match_the_file() {
    let report = check(&[file("other.toml", VALID)], &[], today());
    assert_error(&report, "does not match the file name");
    let report = check_one(|t| t.replace("\"example-au\"", "\"Example_AU\""));
    assert_error(&report, "must be lowercase");
}

#[test]
fn rejects_access_levers_outside_opt_in_levers() {
    for lever in ["app12", "s20r-access", "s21t-access"] {
        let report = check_one(|t| t.replace("\"app13\"]", &format!("\"app13\", \"{lever}\"]")));
        assert_error(&report, "belong in opt_in_levers only");
    }
}

#[test]
fn accepts_an_entry_with_only_opt_in_levers() {
    let report = check_one(|t| {
        t.replace("[\"app7-no-marketing\", \"app7-source\", \"app13\"]", "[]")
            .replace(CHANNEL_LEVERS, "[\"app12\"]")
    });
    assert_eq!(report.findings, []);
    let report = check_one(|t| {
        t.replace("[\"app7-no-marketing\", \"app7-source\", \"app13\"]", "[]")
            .replace("[\"app12\"]", "[]")
    });
    assert_error(&report, "at least one lever");
}

#[test]
fn rejects_unserved_and_unlisted_channel_levers() {
    let report = check_one(|t| t.replace(CHANNEL_LEVERS, "[\"app12\", \"app7-no-marketing\"]"));
    assert_error(&report, "no channel serves `app13`");
    assert_error(&report, "no channel serves `app7-source`");
    let report = check_one(|t| {
        t.replace(
            "[\"app12\", \"app13\",",
            "[\"app12\", \"app13\", \"s20k-ban\",",
        )
    });
    assert_error(&report, "serves `s20k-ban`, which the entry does not list");
}

#[test]
fn rejects_two_preferred_channels_for_a_lever() {
    let second = "\n[[channels]]\nkind = \"post\"\naddress = \"PO Box 1\"\nlevers = [\"app13\"]\npreferred = true\n";
    let report = check_one(|t| format!("{t}{second}"));
    assert_error(&report, "more than one channel is preferred for `app13`");
    let report = check_one(|t| format!("{t}{}", second.replace("preferred = true\n", "")));
    assert_eq!(report.findings, []);
}

#[test]
fn checks_channel_levers_on_obsolete_entries() {
    let obsolete = "\n[obsolete]\nreason = \"closed\"\n";
    let second = "\n[[channels]]\nkind = \"post\"\naddress = \"PO Box 1\"\nlevers = [\"app13\"]\npreferred = true\n";
    let report = check_one(|t| format!("{t}{second}{obsolete}"));
    assert_error(&report, "more than one channel is preferred for `app13`");
    let report = check_one(|t| {
        let t = t.replace(CHANNEL_LEVERS, "[\"app12\", \"app7-no-marketing\"]");
        format!("{t}{obsolete}")
    });
    assert_error(&report, "no channel serves `app13`");
}

#[test]
fn checks_evidence() {
    let sha = "a".repeat(64);
    let evidence = |url: &str, date: &str, sha: &str| {
        format!("\n[[evidence]]\nurl = \"{url}\"\nchecked_at = {date}\nsha256 = \"{sha}\"\n")
    };
    let good = evidence("https://example.com.au/privacy", "2026-10-24", &sha);
    assert_eq!(check_one(|t| format!("{t}{good}")).findings, []);
    let report = check_one(|t| {
        format!(
            "{t}{}",
            evidence("https://other.com.au/", "2026-10-24", &sha)
        )
    });
    assert_error(&report, "is not one of the sources");
    let report = check_one(|t| {
        format!(
            "{t}{}",
            evidence(
                "https://example.com.au/privacy",
                "2026-10-24",
                &"A".repeat(64)
            )
        )
    });
    assert_error(&report, "64 lowercase hex digits");
    let report = check_one(|t| {
        format!(
            "{t}{}",
            evidence("https://example.com.au/privacy", "2026-10-26", &sha)
        )
    });
    assert_error(&report, "not in the future");
    let report = check_one(|t| format!("{t}{good}{good}"));
    assert_error(&report, "is listed twice");
}

#[test]
fn rejects_an_empty_register() {
    let report = check(&[], &[], today());
    assert_error(&report, "no entry files");
}

/// 12 months before 29 February is 28 February, so a check made that day is not stale.
#[test]
fn twelve_months_before_a_leap_day_is_the_last_day_of_february() {
    let leap = "2028-02-29".parse().unwrap();
    assert!(!is_stale("2027-02-28".parse().unwrap(), leap));
    assert!(is_stale("2027-02-27".parse().unwrap(), leap));
    assert!(!is_future("2028-03-01".parse().unwrap(), leap));
    assert!(is_future("2028-03-02".parse().unwrap(), leap));
}

#[test]
fn rejects_a_future_verified_at() {
    let report = check_one(|t| t.replace("2026-10-24", "2026-10-25"));
    assert_eq!(report.findings, [], "one day of grace");
    let report = check_one(|t| t.replace("2026-10-24", "2026-10-26"));
    assert_error(&report, "is in the future");
}

#[test]
fn rejects_urls_without_a_host_and_bad_domains() {
    for url in [
        "https:///",
        "https://?",
        "https://example",
        "https://example.com:bad/path",
        "https://-bad.example/path",
        "https://example.com:+443/path",
        "https://example.com:0/path",
        &format!("https://{}.com/", "a".repeat(64)),
    ] {
        let report = check_one(|t| t.replace("https://example.com.au/privacy", url));
        assert_error(&report, "is not an https URL");
    }
    for email in [
        "privacy@example..au",
        "privacy@-bad.example",
        "privacy..team@example.com.au",
        ".privacy@example.com.au",
        "privacy,team@example.com.au",
    ] {
        let report = check_one(|t| t.replace("privacy@example.com.au", email));
        assert_error(&report, "is not an email address");
    }
    let report = check_one(|t| {
        t.replace(
            "https://example.com.au/privacy",
            "https://example.com.au:443/privacy?x=1#y",
        )
        .replace("privacy@example.com.au", "privacy.team+au@example.com.au")
    });
    assert_eq!(report.findings, []);
}

#[test]
fn rejects_unknown_fields_and_bad_channels() {
    let report = check_one(|t| format!("drivers_licence = \"x\"\n{t}"));
    assert_error(&report, "unknown field `drivers_licence`");
    let report = check_one(|t| t.replace("privacy@example.com.au", "privacy"));
    assert_error(&report, "is not an email address");
    let channel = |kind: &str, address: &str| {
        format!("\n[[channels]]\nkind = \"{kind}\"\naddress = \"{address}\"\nlevers = []\n")
    };
    let report = check_one(|t| format!("{t}{}", channel("web-form", "http://x.com.au/")));
    assert_error(&report, "web form `http://x.com.au/` is not an https URL");
    let report = check_one(|t| format!("{t}{}", channel("post", " ")));
    assert_error(&report, "a channel address must not be empty");
}

#[test]
fn rejects_an_empty_name_or_legal_entity() {
    let report = check_one(|t| t.replace("name = \"Example\"", "name = \"\""));
    assert_error(&report, "name and legal_entity must not be empty");
    let report = check_one(|t| t.replace("\"Example Pty Ltd\"", "\" \""));
    assert_error(&report, "name and legal_entity must not be empty");
}

#[test]
fn rejects_an_obsolete_entry_in_a_pack() {
    let obsolete = format!("{VALID}\n[obsolete]\nreason = \"closed\"\n");
    let report = check(
        &[file("example-au.toml", &obsolete)],
        &[file("marketing.toml", PACK)],
        today(),
    );
    assert_error(&report, "pack lists obsolete entry `example-au`");
}

#[test]
fn warns_but_passes_when_verified_at_is_stale() {
    let report = check_one(|t| t.replace("2026-10-24", "2025-10-23"));
    assert_eq!(report.errors(), 0);
    assert_eq!(report.warnings(), 1);
    assert!(messages(&report, Severity::Warning)[0].contains("more than 12 months old"));
    let report = check_one(|t| t.replace("2026-10-24", "2025-10-24"));
    assert_eq!(report.findings, []);
    assert_eq!(report.warnings(), 0);
}

#[test]
fn rejects_a_verified_at_with_a_time() {
    let report = check_one(|t| t.replace("2026-10-24", "2026-10-24T10:00:00"));
    assert_error(&report, "verified_at must be a date");
}

#[test]
fn rejects_bad_pack_entries() {
    let lead = VALID.replace("\"verified\"", "\"lead\"");
    let pack = PACK.replace("[\"example-au\"]", "[\"example-au\", \"nobody\"]");
    let report = check(
        &[file("example-au.toml", &lead)],
        &[file("marketing.toml", &pack)],
        today(),
    );
    assert_error(&report, "below verified quality");
    assert_error(&report, "unknown entry `nobody`");
}

#[test]
fn rejects_levers_in_a_pack() {
    let pack = format!("{PACK}levers = [\"app12\"]\n");
    let report = check(
        &[file("example-au.toml", VALID)],
        &[file("marketing.toml", &pack)],
        today(),
    );
    assert_error(&report, "unknown field `levers`");
}

#[test]
fn rejects_self_obsolete_redirect() {
    let report = check_one(|t| {
        format!(
            "{t}
[obsolete]
reason = \"merged\"
replaced_by = \"example-au\"
"
        )
    });
    assert_error(&report, "must name another entry");
}

#[test]
fn rejects_an_empty_obsolete_redirect() {
    let report =
        check_one(|t| format!("{t}\n[obsolete]\nreason = \"merged\"\nreplaced_by = \"\"\n"));
    assert_error(&report, "names unknown entry");
}

#[test]
fn rejects_an_unknown_obsolete_reason() {
    let report = check_one(|t| format!("{t}\n[obsolete]\nreason = \"gone\"\n"));
    assert_error(&report, "unknown variant");
}

#[test]
fn requires_a_redirect_only_where_requests_moved() {
    for reason in ["acquired", "merged", "duplicate"] {
        let report = check_one(|t| format!("{t}\n[obsolete]\nreason = \"{reason}\"\n"));
        assert_error(&report, "needs replaced_by");
    }
    for reason in ["closed", "out-of-scope"] {
        let report = check_one(|t| format!("{t}\n[obsolete]\nreason = \"{reason}\"\n"));
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.message.contains("needs replaced_by")),
            "{reason}"
        );
    }
}

#[test]
fn skip_reason_names_the_redirect() {
    let moved = Obsolete {
        reason: ObsoleteReason::Acquired,
        replaced_by: Some("other-au".to_owned()),
    };
    assert_eq!(
        moved.skip_reason(),
        "obsolete (acquired); use other-au instead"
    );
    let closed = Obsolete {
        reason: ObsoleteReason::Closed,
        replaced_by: None,
    };
    assert_eq!(closed.skip_reason(), "obsolete (closed)");
}

#[test]
fn names_pack_findings_by_their_folder() {
    let pack = PACK.replace("[\"example-au\"]", "[\"nobody\"]");
    let report = check(
        &[file("example-au.toml", VALID)],
        &[file(
            "example-au.toml",
            &pack.replace("\"marketing\"", "\"example-au\""),
        )],
        today(),
    );
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].file, "packs/example-au.toml");
}

#[test]
fn rejects_an_unknown_replacement() {
    let report =
        check_one(|t| format!("{t}\n[obsolete]\nreason = \"merged\"\nreplaced_by = \"gone-au\"\n"));
    assert_error(&report, "unknown entry `gone-au`");
}

#[test]
fn only_an_email_channel_may_serve_spam_unsubscribe() {
    let with_spam = |kind: &str, address: &str| {
        check_one(|t| {
            t.replace(
                    "levers = [\"app7-no-marketing\", \"app7-source\", \"app13\"]",
                    "levers = [\"app7-no-marketing\", \"app7-source\", \"app13\", \"spam-unsubscribe\"]",
                )
                .replace(
                    CHANNEL_LEVERS,
                    "[\"app12\", \"app13\", \"app7-no-marketing\", \"app7-source\", \"spam-unsubscribe\"]",
                )
                .replace("kind = \"email\"", &format!("kind = \"{kind}\""))
                .replace("privacy@example.com.au", address)
        })
    };
    assert_eq!(with_spam("email", "privacy@example.com.au").findings, []);
    let report = with_spam("post", "PO Box 1, Sydney NSW 2001");
    assert_error(&report, "only an email channel may serve");
}
