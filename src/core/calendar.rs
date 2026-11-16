// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! The iCalendar file `status --ics` writes (RFC 5545; plan section 5, 'Calendar file'):
//! one all-day event per request with a date. Pure: no clock, no I/O.

use chrono::{Days, NaiveDate, NaiveDateTime};

use crate::core::deadline::Due;

/// One request's date, as the calendar shows it. It holds no personal details.
#[derive(Debug, Clone, Copy)]
pub struct Item<'a> {
    pub tag: &'a str,
    pub entry: &'a str,
    pub lever: &'a str,
    pub due: Due,
    /// How many records the request's letter holds (records of sending and every request's
    /// events). They are only ever added, so it never falls and it rises with every record
    /// that can move the date: the event's SEQUENCE.
    pub sequence: usize,
}

/// The calendar for `items`, stamped with `stamp` (UTC, the time of this export), with CRLF
/// line ends and lines folded at 75 octets. Items without a date are left out.
pub fn ics(items: &[Item], stamp: NaiveDateTime, version: &str) -> String {
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_owned(),
        "VERSION:2.0".to_owned(),
        format!("PRODID:-//yungSNAT//GhostSoap {version}//EN"),
        "CALSCALE:GREGORIAN".to_owned(),
        "METHOD:PUBLISH".to_owned(),
    ];
    for item in items {
        let (day, what, description) = match item.due {
            Due::Statutory(day) => (
                day,
                "law's last day",
                "The law's last day, moved past a weekend. A public holiday where the business \
                 acts moves it to the next working day, which GhostSoap does not know.",
            ),
            Due::Guide(day) => (
                day,
                "OAIC guide date",
                "The OAIC's guide to a reasonable period. Passing it is not a breach.",
            ),
            Due::Unknown | Due::None => continue,
        };
        let next = day + Days::new(1);
        lines.extend([
            "BEGIN:VEVENT".to_owned(),
            format!("UID:{}-{}@ghostsoap.invalid", item.tag, item.lever),
            format!("DTSTAMP:{}", stamp.format("%Y%m%dT%H%M%SZ")),
            format!("DTSTART;VALUE=DATE:{}", basic(day)),
            format!("DTEND;VALUE=DATE:{}", basic(next)),
            format!(
                "SUMMARY:{}",
                escape(&format!(
                    "GhostSoap {}: {} {}, {what}",
                    item.tag, item.entry, item.lever
                ))
            ),
            format!("DESCRIPTION:{}", escape(description)),
            "TRANSP:TRANSPARENT".to_owned(),
            format!("SEQUENCE:{}", item.sequence),
            "END:VEVENT".to_owned(),
        ]);
    }
    lines.push("END:VCALENDAR".to_owned());
    lines.iter().map(|line| fold(line) + "\r\n").collect()
}

/// A date in the basic format, YYYYMMDD.
fn basic(day: NaiveDate) -> String {
    day.format("%Y%m%d").to_string()
}

/// `text` as a TEXT value: backslash, semicolon, comma and newline escaped (RFC 5545 3.3.11).
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            _ => out.push(c),
        }
    }
    out
}

/// `line` folded so no line is longer than 75 octets, breaking only between characters
/// (RFC 5545 3.1): each continuation starts with a space, which counts towards its 75.
fn fold(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + line.len() / 70 * 3);
    let mut width = 0;
    for c in line.chars() {
        if width + c.len_utf8() > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += c.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, month, day).unwrap()
    }

    fn stamp() -> NaiveDateTime {
        day(11, 17).and_hms_opt(9, 5, 7).unwrap()
    }

    fn item(
        tag: &'static str,
        entry: &'static str,
        lever: &'static str,
        due: Due,
    ) -> Item<'static> {
        Item {
            tag,
            entry,
            lever,
            due,
            sequence: 0,
        }
    }

    #[test]
    fn each_dated_request_is_one_all_day_event_with_a_stable_uid() {
        let items = [
            Item {
                sequence: 3,
                ..item(
                    "GS-7K3QX",
                    "quantium-au",
                    "app12",
                    Due::Statutory(day(12, 31)),
                )
            },
            item("GS-7K3QX", "quantium-au", "app7-no-marketing", Due::None),
            item("GS-AB2CD", "equifax-au", "app13", Due::Guide(day(12, 1))),
            item("GS-EF3GH", "telstra-au", "spam-unsubscribe", Due::Unknown),
        ];
        let text = ics(&items, stamp(), "0.1.0");
        let lines: Vec<&str> = text.split("\r\n").collect();
        assert_eq!(text.matches("BEGIN:VEVENT").count(), 2, "{text}");
        for expected in [
            "UID:GS-7K3QX-app12@ghostsoap.invalid",
            "DTSTAMP:20261117T090507Z",
            "DTSTART;VALUE=DATE:20261231",
            "DTEND;VALUE=DATE:20270101",
            "SEQUENCE:3",
            r"SUMMARY:GhostSoap GS-7K3QX: quantium-au app12\, law's last day",
            "UID:GS-AB2CD-app13@ghostsoap.invalid",
            "DTSTART;VALUE=DATE:20261201",
            "DTEND;VALUE=DATE:20261202",
            r"SUMMARY:GhostSoap GS-AB2CD: equifax-au app13\, OAIC guide date",
        ] {
            assert!(lines.contains(&expected), "{expected} missing from {text}");
        }
        assert_eq!(
            ics(&items, stamp(), "0.1.0"),
            text,
            "the same input, the same file"
        );
        assert!(
            !text.replace("\r\n", "").contains('\n'),
            "every line ends in CRLF"
        );
    }

    #[test]
    fn an_empty_calendar_is_still_a_calendar() {
        let expected = [
            "BEGIN:VCALENDAR",
            "VERSION:2.0",
            "PRODID:-//yungSNAT//GhostSoap 0.1.0//EN",
            "CALSCALE:GREGORIAN",
            "METHOD:PUBLISH",
            "END:VCALENDAR",
            "",
        ];
        assert_eq!(ics(&[], stamp(), "0.1.0"), expected.join("\r\n"));
    }

    #[test]
    fn text_values_escape_what_rfc_5545_reserves() {
        assert_eq!(escape("a\\b;c,d\r\ne"), r"a\\b\;c\,d\ne");
    }

    #[test]
    fn long_lines_fold_at_75_octets_and_never_inside_a_character() {
        let ascii = "x".repeat(160);
        let folded = fold(&ascii);
        let parts: Vec<&str> = folded.split("\r\n").collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].len(), 75);
        assert_eq!(parts[1].len(), 75);
        assert!(parts[1].starts_with(' ') && parts[2].starts_with(' '));
        assert_eq!(folded.replace("\r\n ", ""), ascii);

        // A 3-octet character that would end at octet 77 moves to the next line whole.
        let wide = format!("{}é€", "y".repeat(72));
        let folded = fold(&wide);
        let parts: Vec<&str> = folded.split("\r\n").collect();
        assert_eq!(parts[0], format!("{}é", "y".repeat(72)));
        assert_eq!(parts[0].len(), 74);
        assert_eq!(parts[1], " €");
        assert_eq!(folded.replace("\r\n ", ""), wide);
        assert_eq!(fold("short"), "short");
    }
}
