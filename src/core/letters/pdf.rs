// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! A postal letter as an A4 PDF to print. It uses Courier, one of the standard fonts every PDF
//! reader supplies, so nothing is embedded and no dependency is needed. That font's encoding
//! (WinAnsi) covers English and Western European letters only: a letter holding any other
//! character is refused, never altered.

use super::Letter;

/// The body is wrapped at 76 columns; a longer word is broken here so no character runs off
/// the page.
const COLUMNS: usize = 76;
/// 58 lines of 12 points fill an A4 page between 64-point margins.
const LINES_PER_PAGE: usize = 58;

/// The PDF, all ASCII, or the first character the standard font cannot show.
pub fn pdf(letter: &Letter) -> Result<String, char> {
    let lines = layout(letter)?;
    let pages: Vec<&[Vec<u8>]> = lines.chunks(LINES_PER_PAGE).collect();
    Ok(serialise(&objects(&pages)))
}

/// The body's lines as WinAnsi bytes, each at most `COLUMNS` long.
fn layout(letter: &Letter) -> Result<Vec<Vec<u8>>, char> {
    let mut lines = Vec::new();
    for line in letter.body.trim_end_matches('\n').split('\n') {
        let mut bytes = Vec::new();
        for c in line.chars() {
            bytes.push(win_ansi(c).ok_or(c)?);
        }
        if bytes.is_empty() {
            lines.push(bytes);
        } else {
            lines.extend(bytes.chunks(COLUMNS).map(<[u8]>::to_vec));
        }
    }
    Ok(lines)
}

/// Objects: 1 catalogue, 2 page tree, 3 font, then a page and its content for each page.
fn objects(pages: &[&[Vec<u8>]]) -> Vec<String> {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            (0..pages.len())
                .map(|i| format!("{} 0 R", 4 + 2 * i))
                .collect::<Vec<_>>()
                .join(" "),
            pages.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    for (i, page) in pages.iter().enumerate() {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] \
             /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            5 + 2 * i
        ));
        let mut content = String::from("BT\n/F1 10 Tf\n12 TL\n64 778 Td\n");
        for line in page.iter() {
            content.push('(');
            content.push_str(&escape(line));
            content.push_str(") Tj T*\n");
        }
        content.push_str("ET");
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }
    objects
}

/// The file: header, numbered objects, the cross-reference table at their byte offsets and
/// the trailer.
fn serialise(objects: &[String]) -> String {
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{object}\nendobj\n", i + 1));
    }
    let xref = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        out.push_str(&format!("{offset:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    out
}

/// A PDF string's contents: printable ASCII as itself, with `(`, `)` and `\` escaped, and every
/// other byte as an octal escape, so the file stays ASCII.
fn escape(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(b as char);
            }
            b' '..=b'~' => out.push(b as char),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out
}

/// The WinAnsiEncoding byte for `c` (PDF 1.7, Annex D), if it has one.
fn win_ansi(c: char) -> Option<u8> {
    let byte = match c {
        // No-break space and soft hyphen are left out: the font draws them as an ordinary
        // space and a visible hyphen.
        ' '..='~' | '\u{A1}'..='\u{AC}' | '\u{AE}'..='\u{FF}' => c as u8,
        '€' => 0x80,
        '‚' => 0x82,
        'ƒ' => 0x83,
        '„' => 0x84,
        '…' => 0x85,
        '†' => 0x86,
        '‡' => 0x87,
        'ˆ' => 0x88,
        '‰' => 0x89,
        'Š' => 0x8A,
        '‹' => 0x8B,
        'Œ' => 0x8C,
        'Ž' => 0x8E,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        '˜' => 0x98,
        '™' => 0x99,
        'š' => 0x9A,
        '›' => 0x9B,
        'œ' => 0x9C,
        'ž' => 0x9E,
        'Ÿ' => 0x9F,
        _ => return None,
    };
    Some(byte)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::register::ChannelKind;

    fn letter(body: &str) -> Letter {
        Letter {
            kind: ChannelKind::Post,
            to: "PO Box 1, Sydney NSW 2000".to_owned(),
            subject: "Privacy request".to_owned(),
            body: body.to_owned(),
        }
    }

    /// Each shown line, decoded back from the PDF's strings.
    fn shown_lines(pdf: &str) -> Vec<Vec<u8>> {
        let mut lines = Vec::new();
        for line in pdf.lines().filter(|l| l.ends_with(") Tj T*")) {
            let inner = &line[1..line.len() - ") Tj T*".len()];
            let bytes = inner.as_bytes();
            let mut out = Vec::new();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    if bytes[i + 1].is_ascii_digit() {
                        let octal = std::str::from_utf8(&bytes[i + 1..i + 4]).unwrap();
                        out.push(u8::from_str_radix(octal, 8).unwrap());
                        i += 4;
                    } else {
                        out.push(bytes[i + 1]);
                        i += 2;
                    }
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            lines.push(out);
        }
        lines
    }

    #[test]
    fn the_xref_offsets_and_stream_lengths_match_the_file() {
        let body = (1..=70).map(|n| format!("Line {n}")).collect::<Vec<_>>();
        let pdf = pdf(&letter(&body.join("\n"))).unwrap();
        assert!(pdf.is_ascii());
        assert!(pdf.starts_with("%PDF-1.4\n") && pdf.ends_with("%%EOF\n"));
        let xref: usize = pdf
            .rsplit("startxref\n")
            .next()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(pdf[xref..].starts_with("xref\n0 8\n0000000000 65535 f \n"));
        let entries: Vec<&str> = pdf[xref..].lines().skip(3).take(7).collect();
        for (i, entry) in entries.iter().enumerate() {
            assert_eq!(entry.len(), 19, "each entry is 20 bytes with its newline");
            let offset: usize = entry[..10].parse().unwrap();
            assert!(pdf[offset..].starts_with(&format!("{} 0 obj\n", i + 1)));
        }
        for part in pdf.split("<< /Length ").skip(1) {
            let (length, rest) = part.split_once(" >>\nstream\n").unwrap();
            let length: usize = length.parse().unwrap();
            assert!(rest[length..].starts_with("\nendstream"));
        }
        assert!(pdf.contains("/Kids [4 0 R 6 0 R] /Count 2"));
        assert!(pdf.contains("/BaseFont /Courier /Encoding /WinAnsiEncoding"));
    }

    #[test]
    fn every_character_reads_back_and_a_page_holds_58_lines() {
        let body = "Name: Zoë O’Brien (née Müller) \\ 50% – €5\n\nYours faithfully";
        let pdf = pdf(&letter(&body.repeat(30))).unwrap();
        let decoded: Vec<u8> = shown_lines(&pdf).join(&b'\n');
        let expected: Vec<u8> = (body.repeat(30).chars())
            .map(|c| {
                if c == '\n' {
                    b'\n'
                } else {
                    win_ansi(c).unwrap()
                }
            })
            .collect();
        assert_eq!(decoded, expected);
        assert!(pdf.contains(r"(Name: Zo\353 O\222Brien \(n\351e M\374ller\) \\ 50% \226 \2005"));
        // 30 copies of 3 lines, less the 29 joins, is 61 lines: 2 pages.
        assert!(pdf.contains("/Count 2"));
        let first_page = pdf.split("endstream").next().unwrap();
        assert_eq!(first_page.matches(") Tj T*").count(), 58);
    }

    #[test]
    fn a_trailing_newline_adds_no_blank_page() {
        let body = vec!["line"; LINES_PER_PAGE].join("\n") + "\n";
        assert!(pdf(&letter(&body)).unwrap().contains("/Count 1"));
    }

    #[test]
    fn a_long_word_is_broken_at_76_columns_without_losing_a_character() {
        let word = "x".repeat(200);
        let pdf = pdf(&letter(&format!("{}\n{word}", "y".repeat(76)))).unwrap();
        let lines = shown_lines(&pdf);
        let lengths: Vec<usize> = lines.iter().map(Vec::len).collect();
        assert_eq!(lengths, [76, 76, 76, 48]);
    }

    #[test]
    fn win_ansi_maps_its_0x80_to_0x9f_slots_and_leaves_c1_controls_out() {
        let specials = "€‚ƒ„…†‡ˆ‰Š‹ŒŽ‘’“”•–—˜™š›œžŸ";
        let bytes: Vec<u8> = specials.chars().map(|c| win_ansi(c).unwrap()).collect();
        let undefined = [0x81, 0x8D, 0x8F, 0x90, 0x9D];
        let expected: Vec<u8> = (0x80..=0x9F).filter(|b| !undefined.contains(b)).collect();
        assert_eq!(bytes, expected);
        for c in '\u{80}'..='\u{9F}' {
            assert_eq!(win_ansi(c), None);
        }
        assert_eq!(win_ansi('\u{A0}'), None);
        assert_eq!(win_ansi('\u{AD}'), None);
        assert_eq!(win_ansi('\u{A1}'), Some(0xA1));
        assert_eq!(win_ansi('ÿ'), Some(0xFF));
    }

    #[test]
    fn a_character_the_font_cannot_show_is_refused() {
        for (body, refused) in [
            ("Name: 李小龙", '李'),
            ("Name: Влад", 'В'),
            ("Name: Zoe\u{301}", '\u{301}'),
            ("Tab\there", '\t'),
        ] {
            assert_eq!(pdf(&letter(body)), Err(refused));
        }
    }
}
