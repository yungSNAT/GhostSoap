// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Unit tests of the claim check.

use super::*;

fn found(text: &str) -> Vec<String> {
    check(text).into_iter().map(|f| f.found).collect()
}

#[test]
fn flags_a_claimed_right_to_erasure_in_its_common_forms() {
    for text in [
        "I exercise my right to erasure.",
        "My Right to be Forgotten applies.",
        "I have a right-to-erasure.",
        "You must honour my right to\ndeletion.",
        "These are my rights to erasure.",
        "I have the right to have my data deleted.",
        "You are legally required to delete my details.",
        "You are required by law to remove them.",
        "My right to be forgotten applies.",
    ] {
        let flags = check(text);
        assert_eq!(flags.len(), 1, "{text}");
        assert_eq!(flags[0].why, ERASURE);
    }
}

#[test]
fn asking_for_destruction_is_not_a_claim_of_right() {
    assert!(found("Please delete my details and destroy your copies.").is_empty());
    assert!(found("I have the right to complain. Please delete my details.").is_empty());
}

#[test]
fn flags_a_stated_legal_duty_to_erase() {
    for text in [
        "The Privacy Act requires you to delete my data.",
        "The Privacy Act requires that you delete my data.",
        "Deletion is required by law.",
        "APP 11.2 requires you to destroy it.",
        "Section 13 obliges the business to erase it.",
        "Regulation 5 compels erasure.",
        "Legislation says to delete it.",
        "See s 20T and remove it.",
        "See s20T and remove it.",
        "See s. 20T and remove it.",
        "The law compels you to remove it.",
        "The Act requires deletion of my data.",
        "The law requires the erasure of it.",
        "You must by law delete it.",
        "You are obliged to erase it.",
        "You are compelled to delete it.",
        "You are required to delete it.",
        "Under the Privacy Act you have to delete it.",
        "You have to delete it.",
        "The business has to erase it.",
        "Under the Act you must delete it.",
        "It must be deleted.",
        "You shall destroy it.",
        "You have a duty to delete it.",
        "You have an obligation to erase it.",
        "I am entitled to have my records erased.",
        "You are required under the Privacy Act to delete it.",
    ] {
        let flags = check(text);
        assert_eq!(flags.len(), 1, "{text}");
        assert_eq!(flags[0].why, ERASURE);
    }
    assert_eq!(
        found("The Privacy Act requires that you delete my data."),
        ["act ... delete"]
    );
}

#[test]
fn an_ordinary_request_to_delete_is_not_flagged() {
    for text in [
        "Please delete my data.",
        "Please delete my account from your app.",
        "I ask you to remove my details.",
        "I would like you to erase my records.",
        "I require you to delete my data.",
        "We require you to delete it.",
        "The Act requires you to reply. Please delete my data.",
        "I am required to give my name. Please delete it.",
        "I have a right (to complain.) Please delete it.",
        "You must reply.\n\nplease delete it",
    ] {
        assert!(found(text).is_empty(), "{text}");
    }
}

#[test]
fn each_sentence_flags_once_and_may_wrap_across_lines() {
    let two = "You must delete it. The Act requires erasure.";
    assert_eq!(found(two), ["must ... delete", "act ... erasure"]);
    assert_eq!(found("You must\ndelete it."), ["must ... delete"]);
}

#[test]
fn the_tables_wording_is_no_claim_but_excuses_nothing_else() {
    let clause = "APP 11.2 requires you to take reasonable steps to destroy it or to ensure \
                      that it is de-identified";
    assert!(found(clause).is_empty());
    assert_eq!(
        found(&format!("{clause}, and you must delete it.")),
        ["app ... delete"]
    );
}

#[test]
fn a_tag_cannot_hide_a_claim() {
    for text in [
        "my right to <b>erasure</b>",
        "my right to <blink>erasure</blink>",
        "my right <o:p></o:p>to erasure",
        "my right to <span style=\"color:red\">erasure</span>",
        "my right to<br/>erasure",
    ] {
        assert_eq!(found(text), ["right ... erasure"], "{text}");
    }
}

#[test]
fn angle_brackets_round_words_do_not_hide_them() {
    assert_eq!(found("<right to erasure>"), ["right ... erasure"]);
    assert_eq!(found("5<6 and right to erasure>2"), ["right ... erasure"]);
    assert_eq!(found("under the <GDPR>"), ["gdpr"]);
}

#[test]
fn refuses_a_closing_tag_an_entity_or_other_html_by_its_shape() {
    for text in [
        "<main>My right to &#101;rasure</main>",
        "<main>Hello</main>",
        "<p>I claim my right to erasure</p>",
        "<SPAN style=\"color:red\">Hello</SPAN>",
        "<span\nstyle=\"color:red\">Hello",
        "<a href=\"https://example.com.au\">Hello",
        "<br />Hello",
        "<br/>Hello",
        "Hello </o:p>",
        "<!DOCTYPE html>\nHello",
        "<!-- a comment -->Hello",
        "My right to &#101;rasure",
        "My right to &#x65;rasure",
        "My right&nbsp;to erasure",
        "Terms &amp; conditions",
    ] {
        assert_eq!(refusal(text), Some(HTML), "{text:?}");
    }
}

#[test]
fn refuses_a_numeric_reference_with_or_without_its_semicolon() {
    for text in [
        "My right to &#101rasure",
        "My right to &#x65rasure",
        "My right to &#X65rasure",
        "My right to &#101;rasure",
        "My right to &#x65;rasure",
    ] {
        assert_eq!(refusal(text), Some(HTML), "{text:?}");
    }
    assert_eq!(refusal("Ticket &# and &#x for review"), None);
}

#[test]
fn a_name_of_one_character_is_no_entity_and_a_longer_name_is() {
    for text in ["R&D;", "AT&T;", "Q&A;", "Smith&Co and sons"] {
        assert_eq!(refusal(text), None, "{text:?}");
    }
    for text in ["1 &gt; 0", "a&nbsp;b", "Smith&Co;"] {
        assert_eq!(refusal(text), Some(HTML), "{text:?}");
    }
}

#[test]
fn placeholders_and_addresses_in_angle_brackets_are_not_refused() {
    for text in [
        "Dear <name>,\nHello",
        "Dear <your name>,\nHello",
        "Hello<br>there",
        "To: Jo Example <a@example.com.au>\nHello",
        "See <https://example.com.au/privacy?a=b>",
        "5 < 6 and 7 > 6",
        "AT&T and Q&A",
    ] {
        assert_eq!(refusal(text), None, "{text:?}");
    }
}

#[test]
fn refuses_a_folded_header() {
    for text in [
        "Content-Transfer-Encoding:\r\n base64\r\n\r\nSSBleGVyY2lzZQ==\r\n",
        "Content-Transfer-Encoding:\n\tquoted-printable\n\nHello\n",
    ] {
        assert_eq!(refusal(text), Some(ENCODED), "{text:?}");
    }
    assert_eq!(refusal("Content-Type:\n text/html\n\nHello\n"), Some(HTML));
}

#[test]
fn every_template_is_checked_and_flags_nothing() {
    for lever in Lever::ALL {
        let template = letters::template(lever);
        assert_eq!(refusal(template), None, "{}", lever.name());
        assert_eq!(found(template), Vec::<String>::new(), "{}", lever.name());
    }
}

#[test]
fn a_flag_on_a_claimed_right_leaves_out_the_words_it_spans() {
    assert_eq!(
        found("my right to have Jane Citizen's records deleted"),
        ["right ... deleted"]
    );
}

#[test]
fn flags_foreign_law_and_its_citations() {
    assert_eq!(
        found("Under the GDPR's Article 17:"),
        ["gdpr", "article 17"]
    );
    assert_eq!(found("under art. 15"), ["article 15"]);
    assert_eq!(found("under Article 17(1)"), ["article 17"]);
    assert_eq!(found("under Art.17"), ["article 17"]);
    assert_eq!(found("under Art.17(1)(a)."), ["article 17"]);
    assert_eq!(found("(CCPA) and §1798.105(a)"), ["ccpa", "1798.105"]);
    assert_eq!(
        found("the General Data Protection Regulation"),
        ["general data protection regulation"]
    );
    assert_eq!(check("CCPA")[0].why, CCPA);
}

#[test]
fn flags_a_fixed_deadline_in_any_common_form() {
    for (text, period) in [
        ("Delete it within 14 days.", "14 days"),
        ("You have fourteen days.", "fourteen days"),
        ("a 30-day deadline", "30 day"),
        ("within one month of this letter", "one month"),
        ("in twenty one calendar days", "twenty one calendar days"),
        ("within 72 hours", "72 hours"),
        ("You must reply within 30 days.", "30 days"),
        ("You have a month to reply.", "a month"),
        ("within a fortnight", "a fortnight"),
        ("within an hour", "an hour"),
        ("Keep it no longer than two years.", "two years"),
        ("in twelve days", "twelve days"),
        ("Reply within 30days.", "30 days"),
    ] {
        assert_eq!(found(text), [period], "{text}");
        assert_eq!(check(text)[0].why, DEADLINE);
    }
}

#[test]
fn a_period_that_is_no_deadline_is_left_alone() {
    for text in [
        "I moved 3 months ago.",
        "I was a customer for 6 months.",
        "It has been wrong for the last 2 weeks and the past 3 days.",
        "Born 12 November 1960. 2 children.",
        "You mail me twice a month.",
        "My son is 9 years old.",
        "It came 3 times a week.",
    ] {
        assert!(found(text).is_empty(), "{text}");
    }
}

#[test]
fn the_deadline_warning_keeps_the_fixed_periods_apart_from_the_apps() {
    let why = check("within 30 days")[0].why;
    assert!(
        why.contains("an organisation 'a reasonable period'"),
        "{why}"
    );
    assert!(why.contains("ss 20T and 21V"), "{why}");
    assert!(!why.contains("limit in the Act"), "{why}");
}

#[test]
fn refuses_text_whose_words_cannot_be_matched() {
    for text in ["", " \n\n", "\u{feff}", "---\n"] {
        assert_eq!(refusal(text), Some(NO_WORDS), "{text:?}");
    }
    for text in [
        "Content-Transfer-Encoding: base64\r\n\r\nSSBleGVyY2lzZSBteQ==\r\n",
        "CONTENT-TRANSFER-ENCODING: Quoted-Printable\n\nmy right to=\n erasure\n",
    ] {
        assert_eq!(refusal(text), Some(ENCODED), "{text:?}");
    }
    for text in [
        "Content-Type: text/html; charset=utf-8\n\nHello\n",
        "<HTML><body>Hello</body></HTML>\n",
    ] {
        assert_eq!(refusal(text), Some(HTML), "{text:?}");
    }
}

#[test]
fn plain_text_and_an_eight_bit_mail_are_not_refused() {
    assert_eq!(refusal("Dear Privacy Officer\n"), None);
    let mail = "Content-Type: text/plain; charset=utf-8\r\n\
                    Content-Transfer-Encoding: 8bit\r\n\r\nDear Privacy Officer\r\n";
    assert_eq!(refusal(mail), None);
}

#[test]
fn the_law_tables_own_wording_is_not_flagged() {
    for lever in Lever::ALL {
        let row = law(lever);
        assert!(found(row.deadline_text).is_empty(), "{}", lever.name());
        assert!(found(row.charge).is_empty(), "{}", lever.name());
    }
}

#[test]
fn a_table_clause_does_not_excuse_a_period_elsewhere_in_the_text() {
    let table = law(Lever::S20tCorrection).deadline_text;
    assert_eq!(
        found(&format!("{table}. Delete it in 30 days.")),
        ["30 days"]
    );
}

#[test]
fn reports_the_line_a_claim_starts_on() {
    let flags = check("Dear Sir\r\n\r\nI claim my right to\r\nerasure.\r\n");
    assert_eq!(flags[0].line, 3);
}

#[test]
fn a_flag_repeats_only_its_pattern() {
    let flags = check("Jane Citizen of 1 Test Street claims the GDPR");
    assert_eq!(flags[0].found, "gdpr");
}

#[test]
fn a_short_section_citation_is_shown_without_its_number() {
    assert_eq!(
        found("Please delete my account S4471923."),
        ["delete ... section"]
    );
    assert_eq!(found("See s20T and remove it."), ["section ... remove"]);
}

#[test]
fn a_long_number_in_a_citation_or_period_is_shown_as_number() {
    for (text, shown, number) in [
        ("Regards,\nArt\n0412345678", "article number", "0412345678"),
        ("under art.0412345678", "article number", "0412345678"),
        ("see 1798.0412345678", "1798.number", "0412345678"),
    ] {
        let found = found(text);
        assert_eq!(found, [shown], "{text:?}");
        assert!(found.iter().all(|f| !f.contains(number)), "{text:?}");
    }
}

#[test]
fn a_period_holds_only_the_number_next_to_its_unit() {
    for text in [
        "My customer number is 4471923. 30 days have passed.",
        "Call me on +61 412 345 678. 30 days have passed.",
        "Call me on 0412 345 678. 30 days have passed.",
        "Call me on four one two three four five six seven eight, 30 days on.",
    ] {
        assert_eq!(found(text), ["30 days"], "{text:?}");
    }
    assert_eq!(
        found("Call four one two three nine days on."),
        ["nine days"]
    );
    assert_eq!(found("within twenty one days"), ["twenty one days"]);
}

#[test]
fn a_citation_or_period_keeps_its_own_number() {
    assert_eq!(found("under Article 99"), ["article 99"]);
    assert_eq!(found("§1798.100 and §1798.199"), ["1798.100", "1798.199"]);
    assert_eq!(found("within 365 days"), ["365 days"]);
}
