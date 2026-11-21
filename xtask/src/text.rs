// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Page text normalisation for evidence hashes, as specified in `registry/README.md`.

/// Elements whose content is never page text.
const SKIPPED: [&str; 6] = ["script", "style", "noscript", "template", "svg", "iframe"];
/// Skipped elements whose content is raw text, which ends at the first matching end tag.
const RAW_TEXT: [&str; 4] = ["script", "style", "noscript", "iframe"];
/// `input` types a user does not type into or choose.
const NOT_FILLED: [&str; 5] = ["hidden", "submit", "reset", "button", "image"];
/// Elements without contents or an end tag.
const VOID: [&str; 13] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];
/// `input` types that ignore `readonly`.
const NOT_READONLY: [&str; 5] = ["checkbox", "radio", "file", "range", "color"];

/// Whether a response is HTML: its media type names HTML in any letter case or, with no
/// `Content-Type`, its body starts like an HTML document.
pub fn is_html(content_type: Option<&str>, body: &[u8]) -> bool {
    match content_type {
        Some(media) => media.to_ascii_lowercase().contains("html"),
        None => {
            let body = body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(body);
            let start = body.trim_ascii_start();
            let head = start.get(..14).unwrap_or(start).to_ascii_lowercase();
            head.starts_with(b"<!doctype html") || head.starts_with(b"<html")
        }
    }
}

/// A start or end tag (or `<!` and `<?` markup) in the lowercased document.
struct Tag {
    name: String,
    closing: bool,
    self_closing: bool,
    /// Index just past the tag's `>`.
    end: usize,
}

/// The tag starting at `from`, which holds `<`, or `None` if that `<` is text: markup
/// starts with a letter, `/` and a letter, `!` or `?`.
fn tag_at(lower: &str, from: usize) -> Option<Tag> {
    let bytes = lower.as_bytes();
    let closing = bytes.get(from + 1) == Some(&b'/');
    let start = from + 1 + usize::from(closing);
    let first = *bytes.get(start)?;
    if !(first.is_ascii_alphabetic() || !closing && (first == b'!' || first == b'?')) {
        return None;
    }
    let end = tag_end(lower, from);
    let name = lower[start..end]
        .chars()
        .take_while(|&c| !c.is_ascii_whitespace() && c != '/' && c != '>')
        .collect();
    Some(Tag {
        name,
        closing,
        self_closing: lower[..end].ends_with("/>"),
        end,
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Seen {
    No,
    Inside,
    Done,
}

/// The text of the first element called `name`, including any nested ones.
struct Region {
    name: &'static str,
    seen: Seen,
    depth: usize,
    text: String,
}

impl Region {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            seen: Seen::No,
            depth: 0,
            text: String::new(),
        }
    }

    fn tag(&mut self, tag: &Tag) {
        if tag.name != self.name {
            return;
        }
        match (self.seen, tag.closing) {
            (Seen::No, false) => (self.seen, self.depth) = (Seen::Inside, 1),
            (Seen::Inside, false) => self.depth += 1,
            (Seen::Inside, true) => {
                self.depth -= 1;
                if self.depth == 0 {
                    self.seen = Seen::Done;
                }
            }
            _ => {}
        }
    }

    fn push(&mut self, piece: &str) {
        if self.seen == Seen::Inside {
            self.text.push_str(piece);
        }
    }
}

/// The main text of an HTML page: the first `<main>` element, else `<body>`, else the
/// whole document, ignoring comments and skipped elements; tags removed; entities
/// decoded; whitespace collapsed to single spaces.
pub fn main_text(html: &str) -> String {
    // ASCII lowercasing keeps every byte offset, so indexes into `lower` fit `html`.
    let lower = html.to_ascii_lowercase();
    let (mut main, mut body, mut all) = (Region::new("main"), Region::new("body"), String::new());
    let mut i = 0;
    while i < html.len() {
        let piece = if lower[i..].starts_with("<!--") {
            i = after(&lower, i, "-->");
            continue;
        } else if let Some(tag) = (lower.as_bytes()[i] == b'<')
            .then(|| tag_at(&lower, i))
            .flatten()
        {
            i = if tag.closing || !SKIPPED.contains(&tag.name.as_str()) {
                main.tag(&tag);
                body.tag(&tag);
                tag.end
            } else {
                skip_element(&lower, &tag)
            };
            " "
        } else {
            // A `<` that starts no markup is text.
            let from = i + usize::from(lower.as_bytes()[i] == b'<');
            let next = lower[from..].find('<').map_or(html.len(), |j| from + j);
            let text = &html[i..next];
            i = next;
            text
        };
        main.push(piece);
        body.push(piece);
        all.push_str(piece);
    }
    let text = if main.seen != Seen::No {
        main.text
    } else if body.seen != Seen::No {
        body.text
    } else {
        all
    };
    decode_entities(&text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `name` of each `input`, `textarea` and `select` element in an HTML page, in page
/// order with repeats removed, ignoring comments and skipped elements: the fields a page
/// serves without running a script.
pub fn field_names(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut names: Vec<String> = Vec::new();
    // Whether each open `fieldset` is disabled.
    let mut fieldsets: Vec<bool> = Vec::new();
    let mut i = 0;
    while let Some(j) = lower[i..].find('<').map(|j| i + j) {
        if lower[j..].starts_with("<!--") {
            i = after(&lower, j, "-->");
            continue;
        }
        let Some(tag) = tag_at(&lower, j) else {
            i = j + 1;
            continue;
        };
        i = if tag.closing || !SKIPPED.contains(&tag.name.as_str()) {
            tag.end
        } else {
            skip_element(&lower, &tag)
        };
        let source = &html[j..tag.end];
        let element = tag.name.as_str();
        let hidden = !tag.closing
            && element.starts_with(|c: char| c.is_ascii_alphabetic())
            && attribute(source, "hidden").is_some();
        if hidden && !VOID.contains(&element) {
            // Nothing inside an element with the `hidden` attribute is shown; a browser
            // ignores `/>` on an element that is not void.
            // shortcut: an unclosed hidden element (`p`, `li`, `td` may omit their end tags)
            // skips to the end of the document, so later fields read as missing: a false
            // alarm that fails the run, never a field counted as served. Track implied end
            // tags if a listed form ever trips it.
            i = skip_element(&lower, &tag);
            continue;
        }
        if tag.name == "fieldset" {
            // shortcut: a control in a disabled fieldset's first legend counts as disabled,
            // though browsers let users fill it in; the check would then report it missing,
            // loudly. Track the first legend if a listed form ever puts a field there.
            if tag.closing {
                fieldsets.pop();
            } else {
                fieldsets.push(attribute(source, "disabled").is_some());
            }
        }
        if !tag.closing && ["input", "textarea", "select"].contains(&tag.name.as_str()) {
            if tag.name == "textarea" {
                // Its contents are text, even when they look like tags.
                i = raw_text_end(&lower, tag.end, "textarea");
            }
            let kind = attribute(source, "type").map(|kind| kind.to_ascii_lowercase());
            let kind = kind.as_deref().unwrap_or("");
            let readonly = attribute(source, "readonly").is_some()
                && (tag.name == "textarea" || tag.name == "input" && !NOT_READONLY.contains(&kind));
            let fillable = !hidden
                && !fieldsets.contains(&true)
                && attribute(source, "disabled").is_none()
                && !readonly
                && !NOT_FILLED.contains(&kind);
            let name = attribute(source, "name").map(|v| decode_entities(&v));
            if let Some(name) = name.filter(|n| fillable && !n.is_empty() && !names.contains(n)) {
                names.push(name);
            }
        }
    }
    names
}

/// The value of attribute `wanted` (lowercase) in `tag`, the source of one start tag.
fn attribute(tag: &str, wanted: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    // Past `<` and the element name.
    let mut i = 1 + bytes[1..]
        .iter()
        .position(|&b| b.is_ascii_whitespace() || b == b'/' || b == b'>')
        .unwrap_or(bytes.len() - 1);
    loop {
        while bytes
            .get(i)
            .is_some_and(|&b| b.is_ascii_whitespace() || b == b'/')
        {
            i += 1;
        }
        let start = i;
        while bytes
            .get(i)
            .is_some_and(|&b| !b.is_ascii_whitespace() && !b"=/>".contains(&b))
        {
            i += 1;
        }
        if i == start {
            return None;
        }
        let key = tag[start..i].to_ascii_lowercase();
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        let value = if bytes.get(i) == Some(&b'=') {
            i += 1;
            while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                i += 1;
            }
            let (from, to) = match bytes.get(i) {
                Some(&quote @ (b'"' | b'\'')) => {
                    let end = bytes[i + 1..]
                        .iter()
                        .position(|&b| b == quote)
                        .map_or(bytes.len(), |k| i + 1 + k);
                    (i + 1, end)
                }
                _ => {
                    let end = bytes[i..]
                        .iter()
                        .position(|&b| b.is_ascii_whitespace() || b == b'>')
                        .map_or(bytes.len(), |k| i + k);
                    (i, end)
                }
            };
            i = (to + 1).min(bytes.len());
            &tag[from..to]
        } else {
            ""
        };
        if key == wanted {
            return Some(value.to_owned());
        }
    }
}

/// The index just past the end of the skipped element that `start` opens, or the end of
/// the document. Raw-text elements end at their first end tag; `template` and `svg` nest,
/// skipping comments and CDATA sections, and `<svg/>` closes itself.
fn skip_element(lower: &str, start: &Tag) -> usize {
    let name = start.name.as_str();
    if RAW_TEXT.contains(&name) {
        return raw_text_end(lower, start.end, name);
    }
    if name == "svg" && start.self_closing {
        return start.end;
    }
    let mut depth = 1;
    let mut i = start.end;
    while let Some(j) = lower[i..].find('<').map(|j| i + j) {
        if lower[j..].starts_with("<!--") {
            i = after(lower, j, "-->");
            continue;
        }
        if lower[j..].starts_with("<![cdata[") {
            i = after(lower, j, "]]>");
            continue;
        }
        let Some(tag) = tag_at(lower, j) else {
            i = j + 1;
            continue;
        };
        i = tag.end;
        if !tag.closing && RAW_TEXT.contains(&tag.name.as_str()) {
            i = raw_text_end(lower, i, &tag.name);
        } else if tag.name == name {
            if tag.closing {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            } else if !(name == "svg" && tag.self_closing) {
                depth += 1;
            }
        }
    }
    lower.len()
}

/// The index just past the first `</name>` end tag at or after `from`, or the end of the
/// document. `</scripture>` does not end a `script`.
fn raw_text_end(lower: &str, from: usize, name: &str) -> usize {
    let close = format!("</{name}");
    let mut i = from;
    while let Some(j) = lower[i..].find(&close).map(|j| i + j + close.len()) {
        let boundary = lower.as_bytes().get(j);
        if boundary.is_none_or(|&b| b.is_ascii_whitespace() || b == b'/' || b == b'>') {
            return tag_end(lower, j - close.len());
        }
        i = j;
    }
    lower.len()
}

/// The index just past the `>` that ends the tag starting at `from`, skipping quoted
/// attribute values, or the end of the document.
fn tag_end(lower: &str, from: usize) -> usize {
    let bytes = lower.as_bytes();
    let mut i = from + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'>' => return i + 1,
            b'=' => {
                i += 1;
                while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                    i += 1;
                }
                if let Some(&quote @ (b'"' | b'\'')) = bytes.get(i) {
                    i = bytes[i + 1..]
                        .iter()
                        .position(|&b| b == quote)
                        .map_or(bytes.len(), |j| i + 1 + j + 1);
                }
            }
            _ => i += 1,
        }
    }
    bytes.len()
}

/// The index just past the next `needle` at or after `from`, or the end of the document.
fn after(lower: &str, from: usize, needle: &str) -> usize {
    lower[from..]
        .find(needle)
        .map_or(lower.len(), |j| from + j + needle.len())
}

/// Email addresses hidden by Cloudflare's email obfuscation, which replaces each address in
/// a page with `data-cfemail="<hex>"` or a `/cdn-cgi/l/email-protection#<hex>` link. The
/// first byte of the hex is a key XORed with each following byte. Cloudflare writes both
/// forms in double-quoted attributes, so the hex must end at a double quote; any other
/// character there makes the payload malformed.
pub fn cloudflare_emails(html: &str) -> Vec<String> {
    let mut emails = Vec::new();
    for marker in ["data-cfemail=\"", "/cdn-cgi/l/email-protection#"] {
        for (i, _) in html.match_indices(marker) {
            let rest = &html[i + marker.len()..];
            let end = rest
                .find(|c: char| !c.is_ascii_hexdigit())
                .unwrap_or(rest.len());
            if rest[end..].starts_with('"') {
                emails.extend(decode_cloudflare(&rest[..end]));
            }
        }
    }
    emails
}

fn decode_cloudflare(hex: &str) -> Option<String> {
    if hex.len() < 4 || !hex.len().is_multiple_of(2) {
        return None;
    }
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .ok()?;
    let (key, rest) = bytes.split_first()?;
    String::from_utf8(rest.iter().map(|b| b ^ key).collect()).ok()
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        // An entity is at most 10 bytes before its `;`, so look no further.
        let decoded = rest
            .bytes()
            .take(11)
            .position(|b| b == b';')
            .and_then(|semi| entity(&rest[1..semi]).map(|c| (c, semi + 1)));
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out + rest
}

fn entity(name: &str) -> Option<char> {
    let numeric = |digits: &str, radix| {
        u32::from_str_radix(digits, radix)
            .ok()
            .and_then(char::from_u32)
    };
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => match name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
            Some(hex) => numeric(hex, 16),
            None => name.strip_prefix('#').and_then(|dec| numeric(dec, 10)),
        },
    }
}

#[cfg(test)]
mod tests;
