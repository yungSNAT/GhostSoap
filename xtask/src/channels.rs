// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Channel checks: whether an email domain takes mail and a web form loads.

use super::*;

/// Group each mail domain and each web form or manual-check page by the entries that use
/// it, so each is checked once.
pub(super) fn collect_channels(
    entry: &Entry,
    mail_domains: &mut BTreeMap<String, Vec<String>>,
    forms: &mut BTreeMap<String, Vec<String>>,
) {
    for channel in &entry.channels {
        let (target, map) = match channel.kind {
            ChannelKind::Email => {
                let domain = channel.address.rsplit('@').next().unwrap_or_default();
                (domain.to_lowercase(), &mut *mail_domains)
            }
            ChannelKind::WebForm => (channel.address.clone(), &mut *forms),
            ChannelKind::Post | ChannelKind::Phone => continue,
        };
        let ids = map.entry(target).or_default();
        if !ids.contains(&entry.id) {
            ids.push(entry.id.clone());
        }
    }
    if let Some(manual) = &entry.manual_check {
        let ids = forms.entry(manual.url.clone()).or_default();
        if !ids.contains(&entry.id) {
            ids.push(entry.id.clone());
        }
    }
}

pub(super) fn channel_problems(
    agent: &ureq::Agent,
    lookup: &dyn Fn(&str, &str) -> Result<Dns, String>,
    mail_domains: &BTreeMap<String, Vec<String>>,
    forms: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    let mut lines = Vec::new();
    for (domain, ids) in mail_domains {
        let what = format!("email domain {domain}");
        record(&mut lines, &what, ids, check_mail(lookup, domain));
    }
    for (url, ids) in forms {
        let what = format!("web form or manual-check page {url}");
        record(&mut lines, &what, ids, check_form(agent, lookup, url));
    }
    lines
}

/// One DNS answer: the name does not exist, or the data of its records of the asked type.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Dns {
    NoSuchName,
    Records(Vec<String>),
}

/// Ask Google Public DNS, which reports MX records that the standard library cannot look up.
pub(super) fn dns_query(
    agent: &ureq::Agent,
    endpoint: &str,
    name: &str,
    record: &str,
) -> Result<Dns, String> {
    let body = agent
        .get(endpoint)
        .query("name", name)
        .query("type", record)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    parse_dns(&body, record)
}

pub(super) fn parse_dns(body: &str, record: &str) -> Result<Dns, String> {
    let number = match record {
        "MX" => 15,
        "A" => 1,
        "AAAA" => 28,
        _ => return Err(format!("unknown record type {record}")),
    };
    let reply: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    match reply["Status"].as_u64() {
        Some(0) => {}
        Some(3) => return Ok(Dns::NoSuchName),
        status => return Err(format!("DNS status {status:?}")),
    }
    let answers = reply["Answer"].as_array().map_or(&[][..], Vec::as_slice);
    Ok(Dns::Records(
        answers
            .iter()
            .filter(|a| a["type"].as_u64() == Some(number))
            .filter_map(|a| a["data"].as_str().map(str::to_owned))
            .collect(),
    ))
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Check {
    Fine,
    Dead(String),
    Inconclusive(String),
}

/// Whether `domain` can receive mail. Mail falls back to the address records of a domain
/// with no MX record (RFC 5321 section 5.1). A failed lookup says nothing about the domain.
pub(super) fn check_mail(
    lookup: &dyn Fn(&str, &str) -> Result<Dns, String>,
    domain: &str,
) -> Check {
    if domain.is_empty() {
        return Check::Dead("no domain in the address".to_owned());
    }
    for record in ["MX", "A", "AAAA"] {
        match lookup(domain, record) {
            Err(e) => return Check::Inconclusive(format!("lookup failed: {e}")),
            Ok(Dns::NoSuchName) => return Check::Dead("domain does not exist".to_owned()),
            Ok(Dns::Records(records)) if records.is_empty() => {}
            // A null MX record (RFC 7505) says the domain accepts no mail.
            Ok(Dns::Records(records)) if record == "MX" && records == ["0 ."] => {
                return Check::Dead(
                    "domain publishes a null MX record: it accepts no mail".to_owned(),
                );
            }
            Ok(Dns::Records(_)) => return Check::Fine,
        }
    }
    Check::Dead("no MX or address record".to_owned())
}

/// Fetch a web form, never submitting it. Sites answer scripts with 401, 403 and 429, or
/// close the connection, even when the form works; a form may accept only POST (405); a
/// 5xx may pass; and this machine's resolver may block a host. So only another HTTP error,
/// or a host public DNS says does not exist, counts as dead.
pub(super) fn check_form(
    agent: &ureq::Agent,
    lookup: &dyn Fn(&str, &str) -> Result<Dns, String>,
    url: &str,
) -> Check {
    get_form(agent, lookup, url).map_or_else(|check| check, |_| Check::Fine)
}

/// `check_form`'s request: the response, or the `Check` its failure counts as.
pub(super) fn get_form(
    agent: &ureq::Agent,
    lookup: &dyn Fn(&str, &str) -> Result<Dns, String>,
    url: &str,
) -> Result<ureq::http::Response<ureq::Body>, Check> {
    agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(401 | 403 | 405 | 429 | 500..=599) => {
                Check::Inconclusive(e.to_string())
            }
            ureq::Error::StatusCode(_) => Check::Dead(e.to_string()),
            _ => {
                let host = url.parse::<ureq::http::Uri>().ok();
                match host.as_ref().and_then(|u| u.host()).map(|h| lookup(h, "A")) {
                    Some(Ok(Dns::NoSuchName)) => Check::Dead("host does not exist".to_owned()),
                    _ => Check::Inconclusive(e.to_string()),
                }
            }
        })
}
