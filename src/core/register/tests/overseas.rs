// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Overseas entries: no ABN or levers, a form and a manual check.

use super::*;

const OVERSEAS: &str = r#"
id = "example-au"
name = "Example People"
legal_entity = "Example People LLC"
kinds = ["people-search"]
priority = "standard"
quality = "verified"
verified_at = 2026-10-24
sources = ["https://example.com/privacy"]
identity_fields = ["full_name"]
exemption_risk = "overseas"
escalation = []
effort = "medium"

[[channels]]
kind = "web-form"
address = "https://example.com/opt-out"
levers = []

[manual_check]
url = "https://example.com/au"
"#;

#[test]
fn overseas_entries_have_no_abn_or_levers_and_need_a_form_and_manual_check() {
    let check_overseas = |text: &str| check(&[file("example-au.toml", text)], &[], today());
    assert_eq!(check_overseas(OVERSEAS).findings, []);
    let report = check_overseas(&format!("abn = \"88 000 014 675\"\n{OVERSEAS}"));
    assert_error(&report, "must not have an abn");
    let report = check_overseas(&OVERSEAS.replace("levers = []", "levers = [\"app7-source\"]"));
    assert_error(
        &report,
        "serves `app7-source`, which the entry does not list",
    );
    let report = check_overseas(&format!("levers = [\"app7-source\"]\n{OVERSEAS}"));
    assert_error(&report, "an overseas entry has no levers");
    let report =
        check_overseas(&OVERSEAS.replace("[manual_check]\nurl = \"https://example.com/au\"\n", ""));
    assert_error(&report, "needs a manual_check page");
    let report = check_overseas(&OVERSEAS.replace("kind = \"web-form\"", "kind = \"post\""));
    assert_error(&report, "needs its own web-form opt-out channel");
}

#[test]
fn only_overseas_entries_may_omit_the_abn() {
    let report = check_one(|t| t.replace("abn = \"88 000 014 675\"\n", ""));
    assert_error(&report, "abn is missing");
}
