// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! `reply` and `status`: answers recorded lever by lever, and requests listed by action.

use super::*;

#[test]
fn reply_records_answers_lever_by_lever_and_status_lists_them() {
    let (_tmp, home, _drafts, tag, _file) = start_drafted();
    let today = today();
    let error = refused(&run(&home, &["reply", &tag, "acknowledged"]));
    assert!(
        error.contains(&format!("{tag} is not recorded as sent")),
        "{error}"
    );
    done(&run(&home, &["sent", &tag]));

    let line = |what: &str| format!("reply   {tag}  cotality-au  {what}\n");
    let stdout = done(&run(&home, &["reply", &tag, "acknowledged"]));
    assert_eq!(stdout, line(&format!("recorded acknowledged on {today}")));
    let stdout = done(&run(&home, &["reply", &tag, "acknowledged"]));
    assert_eq!(
        stdout,
        line(&format!("already recorded acknowledged on {today}"))
    );
    let source = ["reply", &tag, "fulfilled", "--source", "a list broker"];
    assert!(refused(&run(&home, &source)).contains("name the one the source was given for"));
    done(&run(
        &home,
        &[&source[..], &["--lever", "app7-source"]].concat(),
    ));
    let marketing = ["--lever", "app7-no-marketing"];
    let refusal = [
        "reply",
        &tag,
        "refused-with-reasons",
        "--reason",
        "small-business",
    ];
    done(&run(&home, &[&refusal[..], &marketing].concat()));
    let stdout = done(&run(
        &home,
        &[&["reply", &tag, "--withdraw", "status"][..], &marketing].concat(),
    ));
    assert_eq!(stdout, line("the latest answer is withdrawn"));

    let guide = today + Days::new(30);
    let stdout = done(&run(&home, &["status"]));
    assert_eq!(
        stdout,
        format!(
            "overdue 0  action needed 0  waiting 2  done 1  abandoned 0\n\
             waiting        {tag}  cotality-au  app7-no-marketing  acknowledged  OAIC guide {guide}\n\
             waiting        {tag}  cotality-au  app7-no-facilitation  acknowledged  OAIC guide {guide}\n\
             done           {tag}  cotality-au  app7-source  fulfilled  OAIC guide {guide}\n"
        )
    );
    let due = done(&run(&home, &["status", "--due"]));
    assert_eq!(due.lines().count(), 4, "each has a date: {due}");
}

#[test]
fn reply_takes_only_the_answers_reasons_and_withdrawals_it_knows() {
    let (_tmp, home, _drafts, tag, _file) = start_drafted();
    done(&run(&home, &["sent", &tag]));
    for usage in [
        &["reply", &tag, "sent"][..],
        &["reply", &tag],
        &["reply", &tag, "refused", "--reason", "busy"],
        &["reply", &tag, "--withdraw", "everything"],
    ] {
        assert_eq!(run(&home, usage).status.code(), Some(2), "{usage:?}");
    }
    let state = stored(&home);
    let requests = &state.message(&tag).unwrap().requests;
    assert!(requests.iter().all(|r| r.events.is_empty()));
}

#[test]
fn a_credit_correction_shows_the_law_date_and_an_agreed_longer_period_moves_it() {
    use ghostsoap::core::deadline::past_weekend;

    let (tmp, home, drafts) = start();
    let correction = [
        "draft",
        "--entry",
        "equifax-au",
        "--lever",
        "s20t-correction",
        "--detail",
        "The default listed for 2024 is not mine.",
        "--out",
        drafts.to_str().unwrap(),
    ];
    let [tag] = tags(&done(&run(&home, &correction))).try_into().unwrap();
    done(&run(&home, &["sent", &tag]));
    let today = today();
    // 30 days that start on the day it was sent (s 20T(2)(a)).
    let law = past_weekend(today + Days::new(29));
    let stdout = done(&run(&home, &["status", "--due"]));
    assert!(
        stdout.contains(&format!(
            "waiting        {tag}  equifax-au  s20t-correction  sent  law's last day {law}\n"
        )),
        "{stdout}"
    );
    assert!(stdout.contains("(Acts Interpretation Act 1901 s 36(2))"));

    let until = (today + Days::new(29)).to_string();
    let error = refused(&run(&home, &["reply", &tag, "--agreed-until", &until]));
    assert!(
        error.contains("is not later than the law's last day"),
        "{error}"
    );
    let later = today + Days::new(45);
    let agreed = ["reply", &tag, "--agreed-until", &later.to_string()];
    let line =
        |what: &str| format!("reply   {tag}  equifax-au  {what} a longer period to {later}\n");
    assert_eq!(done(&run(&home, &agreed)), line("recorded"));
    assert_eq!(done(&run(&home, &agreed)), line("already recorded"));
    let moved = format!("law's last day {}", past_weekend(later));
    assert!(done(&run(&home, &["status"])).contains(&moved));
    let stdout = done(&run(&home, &["reply", &tag, "--withdraw", "extension"]));
    assert!(stdout.ends_with("the latest longer period is withdrawn\n"));

    // A draft run that stops leaves its letter marked in the list.
    let (output, _blocked) = blocked_drafts(tmp.path());
    refused(&draft_cotality(&home, &output, &[]));
    let stdout = done(&run(&home, &["status"]));
    assert!(
        stdout.contains("cotality-au  app7-no-marketing  drafted  (its draft run did not finish: draft it again)"),
        "{stdout}"
    );
    assert!(stdout.starts_with("overdue 0  action needed 3  waiting 1  done 0  abandoned 0\n"));
}

#[test]
fn status_ics_writes_each_dated_request_once_without_personal_details_and_never_over_a_file() {
    use ghostsoap::core::deadline::past_weekend;

    let (tmp, home, drafts) = start();
    let detail = "The default listed for 2024 is not mine.";
    let correction = [
        "draft",
        "--entry",
        "equifax-au",
        "--lever",
        "s20t-correction",
        "--detail",
        detail,
        "--out",
        drafts.to_str().unwrap(),
    ];
    let [tag] = tags(&done(&run(&home, &correction))).try_into().unwrap();
    done(&run(&home, &["sent", &tag]));
    // A letter not yet sent has no date, so it is not in the calendar.
    done(&draft_cotality(&home, &drafts, &[]));
    let law = past_weekend(today() + Days::new(29));

    let ics = tmp.path().join("ghostsoap.ics");
    let args = ["status", "--ics", ics.to_str().unwrap()];
    let stdout = done(&run(&home, &args));
    assert!(
        stdout.ends_with(&format!("Wrote 1 date to {}.\n", ics.display())),
        "{stdout}"
    );
    let calendar = fs::read_to_string(&ics).unwrap();
    assert!(calendar.starts_with("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n"));
    assert_eq!(calendar.matches("BEGIN:VEVENT").count(), 1, "{calendar}");
    for line in [
        format!("UID:{tag}-s20t-correction@ghostsoap.invalid"),
        format!("DTSTART;VALUE=DATE:{}", law.format("%Y%m%d")),
    ] {
        assert!(
            calendar.contains(&format!("{line}\r\n")),
            "{line}: {calendar}"
        );
    }
    for personal in [
        "Lara",
        "Croft",
        "croft-manor",
        "0400",
        "1968",
        "3127",
        detail,
    ] {
        assert!(!calendar.contains(personal), "{personal}: {calendar}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&ics).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    let error = refused(&run(&home, &args));
    assert!(
        error.contains("already exists; no calendar file was written"),
        "{error}"
    );
    assert_eq!(fs::read_to_string(&ics).unwrap(), calendar);
}

#[test]
fn a_status_that_cannot_read_the_state_is_degraded_not_empty() {
    let (_tmp, home, _drafts, _tag, _file) = start_drafted();
    let out = ghostsoap(
        &home,
        &["status", "--passphrase-stdin"],
        "not the passphrase\n",
    );
    assert_eq!(
        text(&out.stdout),
        "DEGRADED: wrong passphrase\noverdue ?  action needed ?  waiting ?  done ?  abandoned ?\n"
    );
    let error = refused(&out);
    assert!(error.contains("error: the state could not be read, so this is not an all-clear"));
}

#[test]
fn a_bounced_letter_offers_a_register_report_without_personal_details() {
    let (_tmp, home, _drafts, tag, _file) = start_drafted();
    done(&run(&home, &["sent", &tag]));
    let stdout = done(&run(&home, &["reply", &tag, "bounced"]));
    let link = "https://github.com/yungSNAT/GhostSoap/issues/new?title=Bounced%3A%20cotality-au%20by%20email&body=";
    assert!(stdout.contains(link), "{stdout}");
    let report = stdout.lines().find(|l| l.contains(link)).unwrap();
    for detail in ["Lara", "croft-manor", "0400", "3127"] {
        assert!(!report.contains(detail), "{report}");
    }
    let again = done(&run(&home, &["reply", &tag, "bounced"]));
    assert!(!again.contains("issues/new"), "{again}");
}
