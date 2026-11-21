// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Whether a channel's domain takes mail and its form loads.

use super::*;

fn fake_dns(mx: Dns, a: Dns) -> impl Fn(&str, &str) -> Result<Dns, String> {
    let (mx, a) = (
        std::cell::RefCell::new(Some(mx)),
        std::cell::RefCell::new(Some(a)),
    );
    move |_, record| match record {
        "MX" => Ok(mx.borrow_mut().take().expect("one MX query")),
        "A" => Ok(a.borrow_mut().take().expect("one A query")),
        _ => Ok(Dns::Records(Vec::new())),
    }
}

#[test]
fn a_domain_with_mx_or_only_an_address_can_take_mail() {
    let mx = fake_dns(records(&["10 mail.example.com."]), records(&[]));
    assert_eq!(check_mail(&mx, "example.com"), Check::Fine);
    let address_only = fake_dns(records(&[]), records(&["192.0.2.1"]));
    assert_eq!(check_mail(&address_only, "example.com"), Check::Fine);
}

#[test]
fn reports_a_domain_that_cannot_take_mail() {
    assert_eq!(
        check_mail(&|_, _| panic!("empty domain must not be looked up"), ""),
        Check::Dead("no domain in the address".to_owned())
    );
    let gone = fake_dns(Dns::NoSuchName, records(&[]));
    assert_eq!(
        check_mail(&gone, "x.example"),
        Check::Dead("domain does not exist".to_owned())
    );
    let empty = fake_dns(records(&[]), records(&[]));
    assert_eq!(
        check_mail(&empty, "x.example"),
        Check::Dead("no MX or address record".to_owned())
    );
    let null = fake_dns(records(&["0 ."]), records(&["192.0.2.1"]));
    assert!(matches!(
        check_mail(&null, "x.example"),
        Check::Dead(p) if p.contains("null MX")
    ));
}

#[test]
fn parses_dns_replies() {
    let mx = r#"{"Status":0,"Answer":[{"type":5,"data":"alias."},{"type":15,"data":"10 m."}]}"#;
    assert_eq!(parse_dns(mx, "MX"), Ok(records(&["10 m."])));
    assert_eq!(parse_dns(r#"{"Status":0}"#, "MX"), Ok(records(&[])));
    assert_eq!(parse_dns(r#"{"Status":3}"#, "A"), Ok(Dns::NoSuchName));
    assert!(parse_dns(r#"{"Status":2}"#, "A").is_err());
    assert!(parse_dns("not json", "A").is_err());
    let ipv6 = r#"{"Status":0,"Answer":[{"type":28,"data":"2001:db8::1"}]}"#;
    assert_eq!(parse_dns(ipv6, "AAAA"), Ok(records(&["2001:db8::1"])));
    assert_eq!(
        parse_dns(ipv6, "TXT"),
        Err("unknown record type TXT".to_owned())
    );
}

#[test]
fn reports_a_form_that_does_not_load() {
    let agent = agent(Duration::from_secs(5));
    let lookup = |_: &str, _: &str| Ok(records(&["192.0.2.1"]));
    assert_eq!(
        check_form(&agent, &lookup, &serve_once("200 OK")),
        Check::Fine
    );
    let form = check_form(&agent, &lookup, &serve_once("404 Not Found"));
    assert!(matches!(form, Check::Dead(p) if p.contains("404")));
}

#[test]
fn a_failed_connection_counts_only_if_public_dns_has_no_host() {
    let agent = agent(Duration::from_secs(5));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    drop(listener);
    let gone = |_: &str, _: &str| Ok(Dns::NoSuchName);
    assert_eq!(
        check_form(&agent, &gone, &url),
        Check::Dead("host does not exist".to_owned())
    );
    let there = |_: &str, _: &str| Ok(records(&["192.0.2.1"]));
    assert!(matches!(
        check_form(&agent, &there, &url),
        Check::Inconclusive(_)
    ));
    let broken = |_: &str, _: &str| Err("offline".to_owned());
    assert!(matches!(
        check_form(&agent, &broken, &url),
        Check::Inconclusive(_)
    ));
}

#[test]
fn a_bot_block_does_not_count_as_a_dead_form() {
    let agent = loopback_agent();
    for (code, status) in [
        (401, "401 Unauthorized"),
        (405, "405 Method Not Allowed"),
        (403, "403 Forbidden"),
        (429, "429 Too Many Requests"),
        (500, "500 Internal Server Error"),
        (502, "502 Bad Gateway"),
        (503, "503 Service Unavailable"),
    ] {
        let lookup = |_: &str, _: &str| Ok(records(&["192.0.2.1"]));
        assert_eq!(
            check_form(&agent, &lookup, &serve_once(status)),
            Check::Inconclusive(ureq::Error::StatusCode(code).to_string()),
            "{status}"
        );
    }
}
