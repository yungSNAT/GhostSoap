// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Unit tests of the command line's helpers.

use super::*;

#[test]
fn a_repeated_tag_is_retried() {
    let used = ["GS-ABCDE".to_owned()];
    let tag = unused_tag(&used, |attempt| {
        ["GS-ABCDE", "GS-FGHJK"][attempt as usize].to_owned()
    });
    assert_eq!(tag, "GS-FGHJK");
}

/// `--on` and `--agreed-until` read a calendar day as year, month and day; a month or day
/// may drop its leading zero, and nothing may follow the day.
#[test]
fn a_date_option_reads_year_month_and_day() {
    for (text, read) in [
        ("2026-11-05", "2026-11-05"),
        ("2026-1-5", "2026-01-05"),
        ("2028-02-29", "2028-02-29"),
        ("0099-11-05", "0099-11-05"),
    ] {
        assert_eq!(parse_date(text).unwrap().to_string(), read, "{text}");
    }
    for text in [
        "",
        "2026-02-29",
        "2026-11-31",
        "2026-13-01",
        "2026-11-05x",
        "2026-11-05 ",
        "2026/11/05",
        "20261105",
        "2026-11-05T00:00",
        "10000-01-01",
    ] {
        assert_eq!(
            parse_date(text).unwrap_err(),
            "use a date such as 2026-11-30",
            "{text:?}"
        );
    }
}

/// No register entry offers the Spam Act lever yet, so `status` meets its business days
/// only here.
#[test]
fn status_names_each_kind_of_deadline_and_dates_only_those_with_a_day() {
    let day = jiff::civil::date(2026, 11, 16);
    assert_eq!(due_text(Due::Statutory(day)), "  law's last day 2026-11-16");
    assert_eq!(due_text(Due::Guide(day)), "  OAIC guide 2026-11-16");
    assert!(due_text(Due::Unknown).contains("5 business days after sending"));
    assert_eq!(due_text(Due::None), "");
    assert_eq!(due_date(Due::Statutory(day)), Some(day));
    assert_eq!(due_date(Due::Unknown), None);
    assert_eq!(due_date(Due::None), None);
}

#[test]
fn named_windows_output_calls_for_a_folder_warning() {
    let mut output = Vec::new();
    warn_unchecked_out(false, true, &mut output);
    warn_unchecked_out(true, false, &mut output);
    assert!(output.is_empty());
    warn_unchecked_out(true, true, &mut output);
    let warning = String::from_utf8(output).unwrap();
    assert!(warning.contains("does not check who else can open the --out folder"));
}

#[test]
fn draft_files_match_the_written_channel_and_never_use_phone() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("drafts");
    let register = built_in_register().unwrap();
    let entry = &register.entries["australia-post-au"];
    let phone = Letter {
        kind: ChannelKind::Phone,
        to: "0000000000".to_owned(),
        subject: "Test letter".to_owned(),
        body: "Test body".to_owned(),
    };
    let err = write_draft(&drafts, entry, "GS-01234", &phone, false).unwrap_err();
    assert_eq!(err, "plans never send letters by phone");
    assert!(!drafts.exists());

    for (kind, to, tag) in [
        (ChannelKind::Post, "PO Box 1", "GS-ABCDE"),
        (
            ChannelKind::WebForm,
            "https://example.invalid/form",
            "GS-FGHJK",
        ),
    ] {
        let letter = Letter {
            kind,
            to: to.to_owned(),
            subject: "Test letter".to_owned(),
            body: "Test body".to_owned(),
        };
        write_draft(&drafts, entry, tag, &letter, true).unwrap();
        let path = drafts.join(format!("australia-post-au-{tag}.txt"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "Test body\n");
    }
    let email = Letter {
        kind: ChannelKind::Email,
        to: "privacy@example.invalid".to_owned(),
        ..phone
    };
    write_draft(&drafts, entry, "GS-56789", &email, true).unwrap();
    assert!(drafts.join("australia-post-au-GS-56789.eml").is_file());
}

const RUN: [(&str, &str); 3] = [
    ("australia-post-au", "GS-ABCDE"),
    ("cotality-au", "GS-FGHJK"),
    ("equifax-marketing-au", "GS-MNPQR"),
];

/// A state holding `RUN`'s letters as drafted and not yet handed over.
fn unfinished_run() -> State {
    let mut state = State::new(Profile {
        full_name: "Lara Croft".to_owned(),
        ..Profile::default()
    });
    for (id, tag) in RUN {
        state.messages.push(tracker::Message {
            tag: tag.to_owned(),
            entry: id.to_owned(),
            channel: ChannelKind::Email,
            address: "privacy@example.invalid".to_owned(),
            drafted: jiff::civil::date(2026, 11, 17),
            draft: None,
            superseded: false,
            unfinished: true,
            sends: Vec::new(),
            requests: vec![tracker::Request::new(Lever::App7Source)],
        });
    }
    state
}

/// Publish `RUN`'s letters with a handover that fails on the `fails`th (none for 0), and
/// return the result and the tags handed over.
fn publish_failing(
    state: &mut State,
    fails: usize,
    copy: bool,
) -> (Result<(), String>, Vec<String>) {
    let register = built_in_register().unwrap();
    let letters = RUN.map(|(id, tag)| Drafted {
        entry: &register.entries[id],
        tag: tag.to_owned(),
        letter: Letter {
            kind: ChannelKind::Email,
            to: "privacy@example.invalid".to_owned(),
            subject: format!("Test letter {tag}"),
            body: "Test body".to_owned(),
        },
        missing: Vec::new(),
        dropped: Vec::new(),
        replaces: None,
    });
    let mut handed = Vec::new();
    let redraft = io::drafts::Redraft::default();
    let result = publish_with(letters.into(), redraft, copy, state, |letter| {
        handed.push(letter.tag.clone());
        if handed.len() == fails {
            Err(format!("cannot write {}", letter.tag))
        } else {
            Ok(())
        }
    });
    (result.map(|_| ()), handed)
}

/// A run of 3 letters whose third handover fails (R3520), as a full folder or a file in
/// the way would make it, says the letters written before it need `clean` (R1946).
#[test]
fn a_run_that_fails_part_way_keeps_its_letters_unfinished_and_asks_for_clean() {
    let unfinished = |state: &State| state.messages.iter().filter(|m| m.unfinished).count();
    let mut state = unfinished_run();
    let (failed, handed) = publish_failing(&mut state, 3, false);
    assert_eq!(handed, RUN.map(|(_, tag)| tag));
    assert_eq!(
        failed.unwrap_err(),
        "cannot write GS-MNPQR\nThe letters written before it are plaintext drafts. Once \
             you no longer need them, run `ghostsoap clean`."
    );
    assert_eq!(unfinished(&state), 3);
    // Nothing written before the failure, or nothing written at all: no reminder.
    let (failed, _) = publish_failing(&mut state, 1, false);
    assert_eq!(failed.unwrap_err(), "cannot write GS-ABCDE");
    let (failed, _) = publish_failing(&mut state, 2, true);
    assert_eq!(failed.unwrap_err(), "cannot write GS-FGHJK");
    assert_eq!(unfinished(&state), 3);
    let (done, handed) = publish_failing(&mut state, 0, false);
    assert_eq!((done, handed.len()), (Ok(()), 3));
    assert_eq!(unfinished(&state), 0);
}

#[test]
fn channel_and_deadline_labels_preserve_their_distinctions() {
    assert_eq!(ChannelKind::Phone.label(), "phone");
    assert_eq!(
        deadline_summary(Deadline::BusinessDays(5)),
        "5 business days"
    );
    assert_eq!(
        deadline_summary(Deadline::Reasonable { guide_days: None }),
        "a reasonable period"
    );
    assert_eq!(deadline_summary(Deadline::NotStated), "no time set");
}

/// `--lever` and `--channel` take the names the register writes, and list them on a mistake.
#[test]
fn lever_and_channel_names_parse_as_the_register_writes_them() {
    for lever in Lever::ALL {
        assert_eq!(parse_lever(lever.name()), Ok(lever));
    }
    let names = Lever::ALL.map(Lever::name).join(", ");
    assert_eq!(
        parse_lever("App12"),
        Err(format!("unknown lever; use one of {names}"))
    );
    for kind in [
        ChannelKind::Email,
        ChannelKind::WebForm,
        ChannelKind::Post,
        ChannelKind::Phone,
    ] {
        assert_eq!(parse_channel(&named(kind)), Ok(kind));
    }
    assert_eq!(
        parse_channel("Email"),
        Err("use email, web-form, post or phone".to_owned())
    );
}
