// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Flag legal claims the law table does not support, in an edited draft or the user's own
//! words. A statute review on 5 November 2026 settled what is flagged (plan section 14).
//! The rules are word patterns: they read no meaning, so they can miss a claim or flag a
//! harmless one, and an unflagged text is not thereby sound.

use std::collections::BTreeMap;

use crate::core::law::law;
use crate::core::letters;
use crate::core::register::Lever;

/// One claim for the user to recheck.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flag {
    /// The line the claim starts on, from 1.
    pub line: usize,
    /// The words that matched. They come only from the fixed phrases and from numbers of up
    /// to 3 digits, so a flag repeats nothing personal from the text.
    pub found: String,
    /// Why the claim needs rechecking.
    pub why: &'static str,
}

const ERASURE: &str = "if this claims a right or duty to erase, the Privacy Act levers in the \
                       law table give no right to erasure. APP 11.2 makes a business take \
                       reasonable steps to destroy or de-identify what it no longer needs, which \
                       is its duty and not a right you can use. The tranche 2 Exposure Draft \
                       (read 5 November 2026) is a draft, not law, and its erasure right would \
                       reach large digital platforms only.";
const GDPR: &str = "the law table has no row for the GDPR, which reaches a business only as its \
                    Article 3 provides, so it may not apply here.";
const CCPA: &str = "the law table has no row for California's privacy law, which protects \
                    California residents, so it may not apply here.";
const ARTICLE: &str = "this looks like an article of a foreign law. Australian law cites APPs \
                       and sections, and the law table has no such article.";
const DEADLINE: &str = "if this is a legal deadline, the law table does not give it in these \
                        words. The APPs give an organisation 'a reasonable period', and 30 \
                        days is the OAIC's guide for that, not a limit in those APPs. Other rows \
                        set fixed periods, such as 30 days for a credit correction (ss 20T and \
                        21V), so run `ghostsoap law` for the deadlines the law sets.";
const NO_WORDS: &str = "it has no words to check";
const ENCODED: &str = "it holds an encoded mail body, whose words cannot be matched. Paste the \
                       message into a plain text file and check that";
const HTML: &str = "it holds HTML, not the plain text of a letter. Paste the message into a \
                    plain text file and check that";

// A sentence that holds an erasing word and a legal source or a word of duty is flagged. 'Require'
// and 'requires' are neither, so 'I require you to delete' stays out, and 'the Act requires' is
// caught by its source.
#[rustfmt::skip]
const ERASING_WORDS: [&str; 21] = [
    "erase", "erases", "erased", "erasing", "erasure",
    "delete", "deletes", "deleted", "deleting", "deletion",
    "destroy", "destroys", "destroyed", "destroying", "destruction",
    "remove", "removes", "removed", "removing", "removal",
    "forgotten",
];
#[rustfmt::skip]
const LEGAL_SOURCES: [&str; 13] = [
    "act", "acts", "law", "laws", "legal", "legally", "legislation", "statute",
    "section", "sections", "regulation", "regulations", "apps",
];
/// These count as a legal source only before a number, because 'your app' is not 'APP 11'.
const CITING_WORDS: [&str; 3] = ["app", "s", "ss"];
#[rustfmt::skip]
const DUTY_WORDS: [&str; 14] = [
    "must", "shall", "required", "obliged", "obligated", "obligation", "obligations",
    "compelled", "duty", "duties", "entitled", "entitlement", "right", "rights",
];

/// Word patterns and why each needs rechecking.
const PHRASES: [(&[&[&str]], &str); 2] = [
    (
        &[&["gdpr"], &["general", "data", "protection", "regulation"]],
        GDPR,
    ),
    (
        &[
            &["ccpa"],
            &["cpra"],
            &["california", "consumer", "privacy", "act"],
            &["california", "privacy", "rights", "act"],
        ],
        CCPA,
    ),
];

const HTML_SIGNS: [&str; 2] = ["<!doctype", "<!--"];

const NUMBER_WORDS: [&str; 27] = [
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
    "seventy",
    "eighty",
    "ninety",
];
const QUALIFIERS: [&str; 4] = ["calendar", "business", "working", "clear"];
const UNITS: [&str; 12] = [
    "hour",
    "hours",
    "day",
    "days",
    "week",
    "weeks",
    "fortnight",
    "fortnights",
    "month",
    "months",
    "year",
    "years",
];
/// A period after one of these says how long something lasted or how often it happens, not
/// when it is due.
const NOT_A_DEADLINE: [&str; 8] = [
    "for", "last", "past", "previous", "once", "twice", "times", "every",
];

struct Word {
    line: usize,
    sentence: usize,
    text: String,
}

/// The lowercased words of `text`, without surrounding punctuation or a possessive. Hyphens
/// and angle brackets split words, so '30-day' reads as '30 day', and so does a number joined
/// to a unit, so '30days' reads as '30 days'. A sentence ends at a blank line, or at '.', '!' or
/// '?' before a capital, so it can wrap across lines. A missed end joins two sentences, which
/// can only add a flag.
fn words(text: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let (mut sentence, mut ended, mut blank) = (0, false, false);
    for (index, line) in text.lines().enumerate() {
        blank |= line.trim().is_empty();
        for raw in line.split(|c: char| c.is_whitespace() || "-–—/<>".contains(c)) {
            let word = raw.trim_matches(|c: char| !c.is_alphanumeric());
            if word.is_empty() {
                continue;
            }
            if blank || (ended && word.starts_with(char::is_uppercase)) {
                sentence += 1;
            }
            blank = false;
            ended = raw
                .trim_end_matches([')', '"', '\'', '’', '”'])
                .ends_with(['.', '!', '?']);
            let word = word.to_lowercase().replace('’', "'");
            let word = word.strip_suffix("'s").unwrap_or(&word);
            let line = index + 1;
            for part in number_and_unit(word).into_iter().filter(|p| !p.is_empty()) {
                let text = part.to_owned();
                out.push(Word {
                    line,
                    sentence,
                    text,
                });
            }
        }
    }
    out
}

/// A word such as '30days' as its number and unit; any other word whole.
fn number_and_unit(word: &str) -> [&str; 2] {
    let digits = word.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
    let (number, unit) = word.split_at(digits);
    if UNITS.contains(&unit) {
        [number, unit]
    } else {
        [word, ""]
    }
}

fn is_digits(word: &str) -> bool {
    word.chars().all(|c| c.is_ascii_digit())
}

/// How many words the number at `start` spans: a number in digits or one number word, or a
/// tens word and a units word such as 'twenty one'. A phone number before '30 days', in
/// digits or in words, is therefore left out of the period.
fn number_len(words: &[Word], start: usize) -> usize {
    let text = |i: usize| words.get(i).map_or("", |w| w.text.as_str());
    let (units, tens) = (&NUMBER_WORDS[..9], &NUMBER_WORDS[19..]);
    if tens.contains(&text(start)) && units.contains(&text(start + 1)) {
        2
    } else if is_digits(text(start)) || NUMBER_WORDS.contains(&text(start)) {
        1
    } else {
        0
    }
}

/// The index after the period of time that starts at `start`, if one does.
fn period_end(words: &[Word], start: usize) -> Option<usize> {
    let numbers = number_len(words, start);
    let article = ["a", "an"].contains(&words[start].text.as_str());
    if numbers == 0 && !article {
        return None;
    }
    let mut end = start + numbers.max(1);
    let word_at = |i: usize| words.get(i).map(|w| w.text.as_str());
    if word_at(end).is_some_and(|w| QUALIFIERS.contains(&w)) {
        end += 1;
    }
    word_at(end)
        .is_some_and(|w| UNITS.contains(&w))
        .then_some(end + 1)
}

/// Whether a period states a deadline, rather than how long something lasted or how long
/// ago it happened.
fn is_deadline(words: &[Word], start: usize, end: usize) -> bool {
    let after = words.get(end).map(|w| w.text.as_str());
    let before = start.checked_sub(1).map(|i| words[i].text.as_str());
    after != Some("ago")
        && after != Some("old")
        && !before.is_some_and(|w| NOT_A_DEADLINE.contains(&w))
}

/// The clauses of the law table and the letter templates that state a period or an erasing
/// word. One of these, word for word, is the table's own and is not flagged.
fn table_clauses() -> Vec<Vec<String>> {
    let texts = Lever::ALL.iter().flat_map(|&lever| {
        let row = law(lever);
        [
            row.title,
            row.deadline_text,
            row.charge,
            letters::template(lever),
        ]
    });
    texts
        .flat_map(|text| text.split([',', ';', '\n']))
        .map(words)
        .filter(|clause| {
            clause.iter().any(is_erasing)
                || (0..clause.len()).any(|i| period_end(clause, i).is_some())
        })
        .map(|clause| clause.into_iter().map(|w| w.text).collect())
        .collect()
}

/// For each word of `words`, whether it lies in a clause of the table.
fn in_table(words: &[Word]) -> Vec<bool> {
    let mut covered = vec![false; words.len()];
    for clause in table_clauses() {
        for start in 0..words.len().saturating_sub(clause.len() - 1) {
            let run = &words[start..start + clause.len()];
            if run.iter().zip(&clause).all(|(w, c)| w.text == *c) {
                covered[start..start + clause.len()].fill(true);
            }
        }
    }
    covered
}

fn phrase_at(words: &[Word], start: usize, phrase: &[&str]) -> bool {
    words.len() - start >= phrase.len()
        && words[start..].iter().zip(phrase).all(|(w, p)| w.text == *p)
}

fn is_erasing(word: &Word) -> bool {
    ERASING_WORDS.contains(&word.text.as_str())
}

/// Whether the word at `index` of `sentence` is a legal source or a word of duty.
fn is_source_or_duty(sentence: &[Word], index: usize) -> bool {
    let word = sentence[index].text.as_str();
    let next = sentence.get(index + 1).map_or("", |w| w.text.as_str());
    let number = |s: &str| s.starts_with(|c: char| c.is_ascii_digit());
    LEGAL_SOURCES.contains(&word)
        || DUTY_WORDS.contains(&word)
        || (CITING_WORDS.contains(&word) && number(next))
        || is_short_section(word)
        || (["have", "has"].contains(&word) && next == "to")
}

/// A section cited as 's' and a number, such as 's41' or 's.20t'.
fn is_short_section(word: &str) -> bool {
    word.starts_with('s')
        && word
            .trim_start_matches(['s', '.'])
            .starts_with(|c: char| c.is_ascii_digit())
}

/// How a matched word appears in a flag. A short section citation has the shape of an
/// account or reference number, such as 'S4471923', so it is shown as 'section', and a
/// number of more than 3 digits, such as a phone or customer number, as 'number'. Articles,
/// California's sections after '1798.' and periods of time cited in a letter fit in 3 digits.
fn shown(word: &str) -> &str {
    if is_short_section(word) {
        "section"
    } else if word.len() > 3 && is_digits(word) {
        "number"
    } else {
        word
    }
}

/// The sentences that may claim a right or duty to erase, keyed by the index of the first
/// matched word, as those matched words: the rest of a sentence may be personal.
fn erasure_claims(words: &[Word], covered: &[bool]) -> BTreeMap<usize, String> {
    let mut claims = BTreeMap::new();
    let mut offset = 0;
    for sentence in words.chunk_by(|a, b| a.sentence == b.sentence) {
        let erasing =
            (0..sentence.len()).find(|&i| !covered[offset + i] && is_erasing(&sentence[i]));
        let cited = (0..sentence.len()).find(|&i| is_source_or_duty(sentence, i));
        if let (Some(erasing), Some(cited)) = (erasing, cited) {
            let (first, second) = (erasing.min(cited), erasing.max(cited));
            let found = format!(
                "{} ... {}",
                shown(&sentence[first].text),
                shown(&sentence[second].text)
            );
            claims.insert(offset + first, found);
        }
        offset += sentence.len();
    }
    claims
}

/// A foreign-law citation at `start`: an article number, or a section of California's
/// Civil Code in the CCPA's title (sections 1798.100 onwards), with why it needs rechecking.
fn citation_at(words: &[Word], start: usize) -> Option<(String, &'static str)> {
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    let before_paren = |s: &str| s.split('(').next().unwrap_or_default().to_owned();
    let word = words[start].text.as_str();
    if let Some(section) = word.strip_prefix("1798.") {
        let section = before_paren(section);
        return digits(&section).then(|| (format!("1798.{}", shown(&section)), CCPA));
    }
    let (word, number) = match word.strip_prefix("art.") {
        Some(number) => ("art", number),
        None => (word, words.get(start + 1)?.text.as_str()),
    };
    let number = before_paren(number);
    (["article", "articles", "art"].contains(&word) && digits(&number))
        .then(|| (format!("article {}", shown(&number)), ARTICLE))
}

/// Whether `text`, which starts with '<', opens with a tag that only HTML has: `</word>`,
/// `<word/>` or `<word name=value>`. A lone `<word>` may be a placeholder.
fn opens_html(text: &str) -> bool {
    let Some(end) = text[1..].find(['<', '>']).map(|i| i + 1) else {
        return false;
    };
    let inner = &text[1..end];
    let closing = inner.strip_prefix('/');
    let inner = closing.unwrap_or(inner);
    let length = inner
        .find(|c: char| !c.is_ascii_alphanumeric() && !":-".contains(c))
        .unwrap_or(inner.len());
    let (name, rest) = inner.split_at(length);
    let attributes = rest.starts_with(char::is_whitespace) && rest.contains('=');
    let shape = if closing.is_some() {
        rest.trim().is_empty()
    } else {
        attributes || rest.trim() == "/"
    };
    text[end..].starts_with('>') && name.starts_with(|c: char| c.is_ascii_alphabetic()) && shape
}

/// Whether `text`, lowercased and following an '&', starts an entity: a numeric reference
/// such as `#101` or `#x65`, whose semicolon is optional, or a name such as `amp;`.
/// shortcut: any name of 2 or more characters before a semicolon counts, so 'Smith&Co;' is
/// refused. Only the full table of entity names would tell it apart.
fn is_entity(text: &str) -> bool {
    if let Some(number) = text.strip_prefix('#') {
        return match number.strip_prefix('x') {
            Some(hex) => hex.starts_with(|c: char| c.is_ascii_hexdigit()),
            None => number.starts_with(|c: char| c.is_ascii_digit()),
        };
    }
    let end = text
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(text.len());
    end >= 2 && text[end..].starts_with(';')
}

/// Why `text` cannot be checked, if it cannot: its words would not match, so a count of no
/// claims would mislead.
pub fn refusal(text: &str) -> Option<&'static str> {
    if words(text).is_empty() {
        return Some(NO_WORDS);
    }
    let lower = text.to_lowercase();
    // A mail header may fold its value onto a line that starts with a space or tab.
    let unfolded = lower.replace("\n ", " ").replace("\n\t", " ");
    let header = |name: &str, values: &[&str]| {
        unfolded.lines().any(|line| {
            line.strip_prefix(name)
                .is_some_and(|value| values.iter().any(|v| value.contains(v)))
        })
    };
    if header(
        "content-transfer-encoding:",
        &["base64", "quoted-printable"],
    ) {
        return Some(ENCODED);
    }
    let html = header("content-type:", &["text/html"])
        || HTML_SIGNS.iter().any(|sign| lower.contains(sign))
        || lower
            .match_indices('<')
            .any(|(i, _)| opens_html(&lower[i..]))
        || lower
            .match_indices('&')
            .any(|(i, _)| is_entity(&lower[i + 1..]));
    html.then_some(HTML)
}

/// Claims in `text` that the law table does not support, in the order they appear.
pub fn check(text: &str) -> Vec<Flag> {
    let words = words(text);
    let covered = in_table(&words);
    let mut claims = erasure_claims(&words, &covered);
    let mut flags = Vec::new();
    let mut start = 0;
    while start < words.len() {
        let line = words[start].line;
        if let Some(found) = claims.remove(&start) {
            let why = ERASURE;
            flags.push(Flag { line, found, why });
        }
        for (phrases, why) in PHRASES {
            if let Some(phrase) = phrases.iter().find(|p| phrase_at(&words, start, p)) {
                let found = phrase.join(" ");
                flags.push(Flag { line, found, why });
            }
        }
        if let Some((found, why)) = citation_at(&words, start) {
            flags.push(Flag { line, found, why });
        }
        if let Some(end) = period_end(&words, start) {
            if !covered[start] && is_deadline(&words, start, end) {
                let found: Vec<&str> = words[start..end].iter().map(|w| shown(&w.text)).collect();
                let found = found.join(" ");
                flags.push(Flag {
                    line,
                    found,
                    why: DEADLINE,
                });
            }
            start = end;
        } else {
            start += 1;
        }
    }
    flags
}

#[cfg(test)]
mod tests;
