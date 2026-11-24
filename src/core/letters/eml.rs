// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! RFC 5322 `.eml` draft formatting with CRLF injection sanitisation, RFC 2047 header
//! encoding/folding (<= 76 chars) and RFC 2045 quoted-printable fallback (<= 998 octets).

use crate::core::letters::Letter;
use crate::core::register::ChannelKind;

const MAX_HEADER_LINE: usize = 76;
const MAX_ENCODED_WORD: usize = 75;
const MAX_BODY_OCTETS: usize = 998;
const MAX_QP_LINE: usize = 76;

/// Whether `c` is a line separator that could break an RFC 5322 header line.
fn is_line_break(c: char) -> bool {
    matches!(c, '\r' | '\n' | '\u{2028}' | '\u{2029}')
}

/// Sanitise an email address header value: take the first non-empty line before any CR/LF
/// break so an injected header line after CRLF is dropped, and strip all control characters.
pub(crate) fn sanitise_address(value: &str) -> String {
    let first_line = value
        .split(is_line_break)
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    first_line
        .chars()
        .filter(|&c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Sanitise unstructured header text (`Subject`, `X-GhostSoap-Destination`): replace line
/// breaks with spaces, drop all other control characters and collapse runs of spaces.
pub(crate) fn sanitise_header_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut pending_space = false;
    for c in value.chars() {
        if is_line_break(c) || c == '\t' || c == ' ' {
            if !out.is_empty() {
                pending_space = true;
            }
        } else if !c.is_control() {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(c);
        }
    }
    out
}

/// Try folding pure printable US-ASCII text across `<= 76`-char RFC 5322 lines on spaces.
/// Returns `None` if any word would exceed its line budget (handled by RFC 2047 splitting).
fn fold_ascii_header(name: &str, clean: &str) -> Option<String> {
    let first_budget = MAX_HEADER_LINE.saturating_sub(name.len() + 2);
    let cont_budget = MAX_HEADER_LINE - 1;
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut budget = first_budget;
    for word in clean.split(' ') {
        if word.len() > budget && current.is_empty() {
            return None;
        }
        if !current.is_empty() && current.len() + 1 + word.len() > budget {
            lines.push(std::mem::take(&mut current));
            budget = cont_budget;
            if word.len() > budget {
                return None;
            }
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    lines.push(current);
    Some(lines.join("\r\n "))
}

fn q_encode_char(c: char, out: &mut String) {
    let mut buf = [0u8; 4];
    for &byte in c.encode_utf8(&mut buf).as_bytes() {
        match byte {
            b' ' => out.push('_'),
            0x21..=0x7e if !matches!(byte, b'=' | b'?' | b'_') => out.push(char::from(byte)),
            b => out.push_str(&format!("={b:02X}")),
        }
    }
}

/// Encode `clean` into one or more RFC 2047 UTF-8 Q-encoded words on UTF-8 char boundaries,
/// each `<= 75` chars and folded so every emitted header line is `<= 76` chars.
fn encode_rfc2047_words(name: &str, clean: &str) -> String {
    const PREFIX: &str = "=?UTF-8?Q?";
    const SUFFIX: &str = "?=";
    const OVERHEAD: usize = PREFIX.len() + SUFFIX.len();

    let first_word_max = MAX_ENCODED_WORD.min(MAX_HEADER_LINE.saturating_sub(name.len() + 2));
    let mut payload_budget = first_word_max.saturating_sub(OVERHEAD);
    let cont_payload_budget = MAX_ENCODED_WORD - OVERHEAD;
    let mut words = Vec::new();
    let mut payload = String::new();
    let mut encoded_char = String::new();

    for c in clean.chars() {
        encoded_char.clear();
        q_encode_char(c, &mut encoded_char);
        if !payload.is_empty() && payload.len() + encoded_char.len() > payload_budget {
            words.push(format!("{PREFIX}{payload}{SUFFIX}"));
            payload.clear();
            payload_budget = cont_payload_budget;
        }
        payload.push_str(&encoded_char);
    }
    words.push(format!("{PREFIX}{payload}{SUFFIX}"));
    words.join("\r\n ")
}

/// Format an unstructured header value (`Subject` or `X-GhostSoap-Destination`) as pure
/// US-ASCII folded to `<= 76` chars per line, using RFC 2047 words when needed.
pub(crate) fn format_header_value(name: &str, value: &str) -> String {
    let clean = sanitise_header_text(value);
    if clean.bytes().all(|b| (0x20..=0x7e).contains(&b))
        && let Some(folded) = fold_ascii_header(name, &clean)
    {
        return folded;
    }
    encode_rfc2047_words(name, &clean)
}

/// Normalise any mix of `\r\n`, `\r` and `\n` to RFC 5322 `\r\n` line endings.
pub(crate) fn normalise_crlf(body: &str) -> String {
    body.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
}

/// Encode a `\r\n`-normalised body as RFC 2045 quoted-printable with `<= 76`-char lines.
fn encode_quoted_printable(normalised: &str) -> String {
    let mut lines = Vec::new();
    for line in normalised.split("\r\n") {
        let bytes = line.as_bytes();
        let mut out = String::new();
        let mut col = 0usize;
        for (idx, &byte) in bytes.iter().enumerate() {
            let is_last = idx + 1 == bytes.len();
            let literal = ((0x21..=0x7e).contains(&byte) && byte != b'=')
                || (!is_last && matches!(byte, b' ' | b'\t'));
            let token = if literal {
                String::from(char::from(byte))
            } else {
                format!("={byte:02X}")
            };
            // Leave room for a trailing `=` soft line break when more bytes follow.
            let limit = if is_last {
                MAX_QP_LINE
            } else {
                MAX_QP_LINE - 1
            };
            if col + token.len() > limit {
                out.push_str("=\r\n");
                col = 0;
            }
            out.push_str(&token);
            col += token.len();
        }
        lines.push(out);
    }
    lines.join("\r\n")
}

/// Choose `8bit` when every normalised line is `<= 998` octets, else `quoted-printable`.
fn encode_body(body: &str) -> (&'static str, String) {
    let normalised = normalise_crlf(body);
    if normalised
        .split("\r\n")
        .all(|line| line.len() <= MAX_BODY_OCTETS)
    {
        ("8bit", normalised)
    } else {
        ("quoted-printable", encode_quoted_printable(&normalised))
    }
}

/// An `.eml` draft (RFC 5322). `X-Unsent: 1` makes Thunderbird, Apple Mail and Outlook open
/// it as an unsent draft; there is no `From`, so the mail client fills in the user's own
/// address. Every header value is sanitised against CRLF injection and folded to `<= 76`
/// chars; bodies with any line over 998 octets switch to `quoted-printable`.
pub fn eml(letter: &Letter) -> String {
    let subject = format_header_value("Subject", &letter.subject);
    let (encoding, body) = encode_body(&letter.body);
    if letter.kind == ChannelKind::Email {
        let to = sanitise_address(&letter.to);
        format!(
            "To: {to}\r\nSubject: {subject}\r\nX-Unsent: 1\r\nMIME-Version: 1.0\r\n\
             Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: {encoding}\r\n\r\n{body}\r\n"
        )
    } else {
        let dest = format_header_value("X-GhostSoap-Destination", &letter.to);
        let channel = letter.kind.label();
        format!(
            "Subject: {subject}\r\nX-Unsent: 1\r\nX-GhostSoap-Channel: {channel}\r\n\
             X-GhostSoap-Destination: {dest}\r\nMIME-Version: 1.0\r\n\
             Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: {encoding}\r\n\r\n{body}\r\n"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email_letter(to: &str, subject: &str, body: &str) -> Letter {
        Letter {
            kind: ChannelKind::Email,
            to: to.to_owned(),
            subject: subject.to_owned(),
            body: body.to_owned(),
        }
    }

    #[test]
    fn sanitise_address_drops_crlf_injected_headers_and_controls() {
        assert_eq!(
            sanitise_address("privacy@example.com\r\nBcc: attacker@evil.example"),
            "privacy@example.com"
        );
        assert_eq!(
            sanitise_address("\r\n\n  priv\0acy@example.com\x07\nX-Evil: 1"),
            "privacy@example.com"
        );
        assert_eq!(sanitise_address("\r\n"), "");
    }

    #[test]
    fn rfc2047_and_ascii_folding_respect_75_and_76_char_limits() {
        // 11x 'é' exceeds a single 75-char encoded-word after "Subject: " prefix.
        let accented = "é".repeat(11);
        let draft = eml(&email_letter("privacy@example.com", &accented, "Hi"));
        let (headers, _) = draft.split_once("\r\n\r\n").unwrap();
        for line in headers.split("\r\n") {
            assert!(line.len() <= MAX_HEADER_LINE, "header > 76: {line:?}");
            assert!(line.is_ascii(), "non-ASCII header: {line:?}");
            if let Some(word) = line.trim_start().strip_prefix("=?UTF-8?Q?") {
                assert!(word.ends_with("?="));
                assert!(line.trim_start().len() <= MAX_ENCODED_WORD);
            }
        }
        assert!(headers.matches("=?UTF-8?Q?").count() >= 2, "{headers}");

        // 1,000-char space-separated ASCII subject folds cleanly on spaces <= 76 chars.
        let spaced = vec!["word"; 200].join(" ");
        let spaced_draft = eml(&email_letter("privacy@example.com", &spaced, "Hi"));
        let (spaced_headers, _) = spaced_draft.split_once("\r\n\r\n").unwrap();
        assert!(!spaced_headers.contains("=?UTF-8?Q?"));
        for line in spaced_headers.split("\r\n") {
            assert!(line.len() <= MAX_HEADER_LINE, "line > 76: {line:?}");
        }

        // 1,000-char unbroken ASCII subject splits via RFC 2047 words <= 75/76 chars.
        let unbroken = "a".repeat(1000);
        let unbroken_draft = eml(&email_letter("privacy@example.com", &unbroken, "Hi"));
        let (unbroken_headers, _) = unbroken_draft.split_once("\r\n\r\n").unwrap();
        for line in unbroken_headers.split("\r\n") {
            assert!(line.len() <= MAX_HEADER_LINE, "line > 76: {line:?}");
        }
        // Long word after an earlier short word also triggers RFC 2047 fallback.
        let mixed = format!("ok {}", "b".repeat(80));
        let mixed_val = format_header_value("Subject", &mixed);
        assert!(mixed_val.starts_with("=?UTF-8?Q?"));
    }

    #[test]
    fn eml_prevents_crlf_injection_and_uses_quoted_printable_for_long_body_lines() {
        let draft = eml(&email_letter(
            "privacy@example.com\r\nBcc: attacker@evil.example",
            "Privacy\0 Act\x07 request\r\nX-Injected: yes",
            "Line 1\r\nLine 2\rLine 3\nLine 4",
        ));
        let (headers, body) = draft.split_once("\r\n\r\n").expect("header separator");
        assert!(!headers.contains("Bcc:"));
        assert!(!headers.contains("\r\nX-Injected:"));
        assert!(headers.contains("Content-Transfer-Encoding: 8bit"));
        assert_eq!(body, "Line 1\r\nLine 2\r\nLine 3\r\nLine 4\r\n");

        // 1,000-char unbroken body line triggers quoted-printable <= 76 chars per line.
        let long_body = format!("{} = trailing \nshort", "x".repeat(1000));
        let qp_draft = eml(&email_letter(
            "privacy@example.com",
            "Req [GS-TEST1]",
            &long_body,
        ));
        let (qp_headers, qp_body) = qp_draft.split_once("\r\n\r\n").unwrap();
        assert!(qp_headers.contains("Content-Transfer-Encoding: quoted-printable"));
        assert!(qp_body.contains("=3D"));
        assert!(qp_body.contains("=20"));
        for line in qp_body.split("\r\n") {
            assert!(line.len() <= MAX_QP_LINE, "qp line > 76: {}", line.len());
        }
    }

    #[test]
    fn eml_for_non_email_channels_omits_to_and_records_destination() {
        let letter = Letter {
            kind: ChannelKind::WebForm,
            to: "https://example.com/privacy\r\nBcc: evil@example.com".to_owned(),
            subject: "Privacy Act request = ?_ \u{2014} [GS-TEST1]".to_owned(),
            body: "Hello".to_owned(),
        };
        let draft = eml(&letter);
        let (headers, _) = draft.split_once("\r\n\r\n").expect("header separator");
        assert!(!headers.contains("To:"));
        assert!(headers.contains("X-Unsent: 1\r\n"));
        assert!(headers.contains("X-GhostSoap-Channel: web form\r\n"));
        assert!(headers.contains("=3D") && headers.contains("=3F") && headers.contains("=5F"));
        for line in headers.split("\r\n") {
            assert!(line.len() <= MAX_HEADER_LINE, "line > 76: {line:?}");
        }
    }
}
