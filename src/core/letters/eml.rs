// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! RFC 5322 `.eml` draft formatting with header sanitisation to prevent CRLF injection.

use crate::core::letters::Letter;
use crate::core::register::ChannelKind;

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

/// Encode a sanitised header value as pure printable US-ASCII, using RFC 2047 UTF-8
/// Q-encoding only when non-ASCII bytes are present.
pub(crate) fn encode_header_value(value: &str) -> String {
    let clean = sanitise_header_text(value);
    if clean.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
        return clean;
    }
    let mut encoded = String::from("=?UTF-8?Q?");
    for byte in clean.bytes() {
        match byte {
            b' ' => encoded.push('_'),
            0x21..=0x7e if !matches!(byte, b'=' | b'?' | b'_') => {
                encoded.push(char::from(byte));
            }
            b => encoded.push_str(&format!("={b:02X}")),
        }
    }
    encoded.push_str("?=");
    encoded
}

/// Normalise any mix of `\r\n`, `\r` and `\n` to RFC 5322 `\r\n` line endings.
pub(crate) fn normalise_crlf(body: &str) -> String {
    body.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
}

/// An `.eml` draft (RFC 5322). `X-Unsent: 1` makes Thunderbird, Apple Mail and Outlook open
/// it as an unsent draft; there is no `From`, so the mail client fills in the user's own
/// address. Every header value is sanitised against CRLF header injection.
pub fn eml(letter: &Letter) -> String {
    let subject = encode_header_value(&letter.subject);
    let body = normalise_crlf(&letter.body);
    if letter.kind == ChannelKind::Email {
        let to = sanitise_address(&letter.to);
        format!(
            "To: {to}\r\nSubject: {subject}\r\nX-Unsent: 1\r\nMIME-Version: 1.0\r\n\
             Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n{body}\r\n"
        )
    } else {
        let dest = encode_header_value(&letter.to);
        let channel = letter.kind.label();
        format!(
            "Subject: {subject}\r\nX-Unsent: 1\r\nX-GhostSoap-Channel: {channel}\r\n\
             X-GhostSoap-Destination: {dest}\r\nMIME-Version: 1.0\r\n\
             Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n{body}\r\n"
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
    fn encode_header_value_strips_crlf_and_encodes_non_ascii_rfc2047() {
        assert_eq!(
            encode_header_value("Privacy Act\r\nBcc: evil@example.com\t\x00  [GS-TEST1]"),
            "Privacy Act Bcc: evil@example.com [GS-TEST1]"
        );
        let non_ascii = encode_header_value("Request \u{2014} 100% = ?_");
        assert!(non_ascii.starts_with("=?UTF-8?Q?"), "{non_ascii}");
        assert!(non_ascii.ends_with("?="), "{non_ascii}");
        assert!(non_ascii.is_ascii(), "{non_ascii}");
        assert!(non_ascii.contains("=3D"));
        assert!(non_ascii.contains("=3F"));
        assert!(non_ascii.contains("=5F"));
        assert!(non_ascii.contains('_'));
    }

    #[test]
    fn eml_prevents_crlf_header_injection_and_normalises_body() {
        let draft = eml(&email_letter(
            "privacy@example.com\r\nBcc: attacker@evil.example",
            "Privacy Act request\r\nX-Injected: yes",
            "Line 1\r\nLine 2\rLine 3\nLine 4",
        ));
        let (headers, body) = draft.split_once("\r\n\r\n").expect("header separator");
        assert!(
            !headers.contains("Bcc:"),
            "injected header dropped: {headers}"
        );
        assert!(!headers.contains("\r\nX-Injected:"), "{headers}");
        assert!(headers.starts_with("To: privacy@example.com\r\n"));
        assert!(headers.contains("Subject: Privacy Act request X-Injected: yes\r\n"));
        assert!(headers.contains("X-Unsent: 1\r\n"));
        assert_eq!(body, "Line 1\r\nLine 2\r\nLine 3\r\nLine 4\r\n");
    }

    #[test]
    fn eml_for_non_email_channels_omits_to_and_records_destination() {
        let letter = Letter {
            kind: ChannelKind::WebForm,
            to: "https://example.com/privacy\r\nBcc: evil@example.com".to_owned(),
            subject: "Privacy Act request [GS-TEST1]".to_owned(),
            body: "Hello".to_owned(),
        };
        let draft = eml(&letter);
        let (headers, _) = draft.split_once("\r\n\r\n").expect("header separator");
        assert!(!headers.contains("To:"));
        assert!(headers.contains("X-Unsent: 1\r\n"));
        assert!(headers.contains("X-GhostSoap-Channel: web form\r\n"));
        assert!(headers.contains(
            "X-GhostSoap-Destination: https://example.com/privacy Bcc: evil@example.com\r\n"
        ));
    }
}
