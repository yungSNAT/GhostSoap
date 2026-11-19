use super::*;
use crate::tests::{loopback_agent, serve_body, serve_once};

fn fields(url: &str, names: &[&str]) -> Form {
    Form {
        url: url.to_owned(),
        expect: Expect::Fields(names.iter().map(|&n| n.to_owned()).collect()),
    }
}

fn there(_: &str, _: &str) -> Result<Dns, String> {
    Ok(Dns::Records(vec!["192.0.2.1".to_owned()]))
}

const FORM: &str = "<html><body><main><h1>Remove my details</h1>\
    <form><input name=\"firstName\"><input name=\"email\">\
    <textarea name=\"message\"></textarea></form></main></body></html>";

#[test]
fn a_form_with_its_fields_passes() {
    let url = serve_body("200 OK", "text/html", FORM);
    let form = fields(&url, &["firstName", "email", "message"]);
    assert_eq!(check(&loopback_agent(), &there, &form), Check::Fine);
}

#[test]
fn names_from_the_page_cannot_break_out_of_the_report() {
    let mut page = String::from(
        "<main><form><input name=\"x` [a link](https://bad.example) @someone\">\
         <input name=\"two&#10;&#10;# lines\">",
    );
    page += &format!("<input name=\"{}\">", "n".repeat(200));
    for i in 0..30 {
        page += &format!("<input name=\"f{i}\">");
    }
    let url = serve_body("200 OK", "text/html", &(page + "</form></main>"));
    // The Debug text shows a newline in the message as `\n`.
    let problem = format!(
        "{:?}",
        check(&loopback_agent(), &there, &fields(&url, &["email"]))
    );
    assert!(
        problem.starts_with("Dead(\"missing field `email`;"),
        "{problem}"
    );
    // Only the backticks the message puts around `email` and the 20 names it shows.
    assert_eq!(problem.matches('`').count(), 42, "{problem}");
    assert!(
        problem.contains("`x' [a link](https://bad.example) @someone`"),
        "{problem}"
    );
    assert!(!problem.contains("\\n"), "{problem}");
    assert!(
        problem.contains(&format!("{}...", "n".repeat(MAX_NAME))),
        "{problem}"
    );
    assert!(!problem.contains(&"n".repeat(MAX_NAME + 1)), "{problem}");
    assert!(problem.ends_with("` and 13 more\")"), "{problem}");
}

#[test]
fn a_form_missing_a_field_has_changed() {
    let url = serve_body("200 OK", "text/html", FORM);
    let form = fields(&url, &["email", "phone"]);
    assert_eq!(
        check(&loopback_agent(), &there, &form),
        Check::Dead(
            "missing field `phone`; the page serves `firstName`, `email`, `message`".to_owned()
        )
    );
}

#[test]
fn a_field_left_only_as_a_hidden_input_has_changed() {
    let page = "<main><form><input name=\"firstName\">\
        <input type=\"hidden\" name=\"email\"></form></main>";
    let url = serve_body("200 OK", "text/html", page);
    let form = fields(&url, &["firstName", "email"]);
    assert_eq!(
        check(&loopback_agent(), &there, &form),
        Check::Dead("missing field `email`; the page serves `firstName`".to_owned())
    );
}

#[test]
fn a_field_left_only_with_the_hidden_attribute_has_changed() {
    let page = "<main><form><input name=\"firstName\">\
        <input type=\"text\" name=\"email\" hidden></form></main>";
    let url = serve_body("200 OK", "text/html", page);
    let form = fields(&url, &["firstName", "email"]);
    assert_eq!(
        check(&loopback_agent(), &there, &form),
        Check::Dead("missing field `email`; the page serves `firstName`".to_owned())
    );
}

#[test]
fn a_page_with_no_content_is_gone() {
    for expect in ["fields = [\"email\"]", "unchecked = \"drawn by a script\""] {
        let url = serve_body("204 No Content", "text/html", "");
        let form = parse(&format!("[[form]]\nurl = \"{url}\"\n{expect}\n")).unwrap();
        assert_eq!(
            check(&loopback_agent(), &there, &form[0]),
            Check::Dead("the page answers 204 No Content".to_owned()),
            "{expect}"
        );
    }
}

#[test]
fn a_page_of_text_with_no_fields_has_changed() {
    let page = format!(
        "<main><p>{}</p></main>",
        "We now take requests by phone. ".repeat(30)
    );
    let url = serve_body("200 OK", "text/html", &page);
    let form = fields(&url, &["email"]);
    assert!(matches!(
        check(&loopback_agent(), &there, &form),
        Check::Dead(p) if p.starts_with("missing field `email`")
    ));
}

#[test]
fn a_bare_page_with_no_fields_is_inconclusive() {
    let url = serve_body("200 OK", "text/html", "<p>Checking your browser</p>");
    let form = fields(&url, &["email"]);
    assert!(matches!(
        check(&loopback_agent(), &there, &form),
        Check::Inconclusive(_)
    ));
}

#[test]
fn a_gone_form_fails_and_a_refused_one_is_inconclusive() {
    let gone = fields(&serve_once("404 Not Found"), &["email"]);
    assert!(matches!(
        check(&loopback_agent(), &there, &gone),
        Check::Dead(p) if p.contains("404")
    ));
    let refused = fields(&serve_once("403 Forbidden"), &["email"]);
    assert!(matches!(
        check(&loopback_agent(), &there, &refused),
        Check::Inconclusive(_)
    ));
}

#[test]
fn a_refused_connection_fails_only_if_the_host_is_gone() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let form = fields(
        &format!("http://{}/", listener.local_addr().unwrap()),
        &["email"],
    );
    drop(listener);
    let gone = |_: &str, _: &str| Ok(Dns::NoSuchName);
    assert_eq!(
        check(&loopback_agent(), &gone, &form),
        Check::Dead("host does not exist".to_owned())
    );
    assert!(matches!(
        check(&loopback_agent(), &there, &form),
        Check::Inconclusive(_)
    ));
}

#[test]
fn an_unchecked_form_must_still_load() {
    let form = |url: String| Form {
        url,
        expect: Expect::Unchecked("drawn by a script".to_owned()),
    };
    let page = serve_body("200 OK", "text/html", "<div id=app></div>");
    assert_eq!(check(&loopback_agent(), &there, &form(page)), Check::Fine);
    let gone = form(serve_once("410 Gone"));
    assert!(matches!(
        check(&loopback_agent(), &there, &gone),
        Check::Dead(_)
    ));
}

#[test]
fn parses_fields_and_unchecked_forms() {
    let forms = parse(
        "[[form]]\nurl = \"https://a.example/f\"\nfields = [\"email\"]\n\n\
         [[form]]\nurl = \"https://b.example/f\"\nunchecked = \"drawn by a script\"\n",
    )
    .unwrap();
    assert_eq!(
        forms,
        [
            fields("https://a.example/f", &["email"]),
            Form {
                url: "https://b.example/f".to_owned(),
                expect: Expect::Unchecked("drawn by a script".to_owned()),
            },
        ]
    );
}

#[test]
fn rejects_a_malformed_list() {
    for (source, error) in [
        ("[[form]]\nurl = \"u\"\n", "give either fields or unchecked"),
        (
            "[[form]]\nurl = \"u\"\nfields = [\"a\"]\nunchecked = \"r\"\n",
            "give either fields or unchecked",
        ),
        ("[[form]]\nurl = \"u\"\nfields = []\n", "non-empty list"),
        ("[[form]]\nurl = \"u\"\nfields = [\"\"]\n", "non-empty list"),
        (
            "[[form]]\nurl = \"u\"\nunchecked = \" \"\n",
            "give a reason",
        ),
        ("[[form]]\nfields = [\"a\"]\n", "no url"),
        ("form = [1]\n", "form 1 is not a table"),
        ("form = 1\n", "no [[form]] tables"),
        (
            "[[form]]\nurl = \"u\"\nfield = [\"a\"]\n",
            "unknown key `field`",
        ),
        ("forms = 1\n", "unknown key `forms`"),
        ("", "no [[form]] tables"),
        (
            "[[form]]\nurl = \"u\"\nfields = [\"a\"]\n[[form]]\nurl = \"u\"\nfields = [\"b\"]\n",
            "listed twice",
        ),
    ] {
        let err = parse(source).unwrap_err();
        assert!(err.contains(error), "{source:?}: {err}");
    }
}

fn entry(id: &str, extra: &str) -> SourceFile {
    SourceFile {
        name: format!("{id}.toml"),
        text: crate::tests::ENTRY.replace("id = \"example-au\"", &format!("id = \"{id}\"")) + extra,
    }
}

#[test]
fn lists_forms_that_carry_a_request_or_are_an_overseas_route() {
    let request = entry(
        "a-au",
        "\n[[channels]]\nkind = \"web-form\"\naddress = \"https://a.example/request\"\n\
         levers = [\"app7-no-marketing\"]\n\
         \n[[channels]]\nkind = \"web-form\"\naddress = \"https://a.example/contact\"\n\
         levers = []\n",
    );
    let overseas = SourceFile {
        text: entry("b-au", "")
            .text
            .replace("exemption_risk = \"none\"", "exemption_risk = \"overseas\""),
        ..entry("b-au", "")
    };
    let retired = entry(
        "c-au",
        "\n[[channels]]\nkind = \"web-form\"\naddress = \"https://c.example/request\"\n\
         levers = [\"app7-no-marketing\"]\n\n[obsolete]\nreason = \"closed\"\n",
    );
    let urls = sent_to(&[request, overseas, retired]).unwrap();
    assert_eq!(urls["https://a.example/request"], ["a-au"]);
    assert_eq!(urls["https://example.com.au/form"], ["b-au"]);
    assert_eq!(urls.len(), 2, "{urls:?}");
}

#[test]
fn reports_forms_missing_from_the_list_and_stale_ones() {
    let sent: BTreeMap<String, Vec<String>> =
        [("https://a.example/f".to_owned(), vec!["a-au".to_owned()])].into();
    let listed = [fields("https://b.example/f", &["email"])];
    assert_eq!(
        coverage(&listed, &sent),
        [
            "not in forms.toml: https://a.example/f",
            "in forms.toml but no entry sends users there: https://b.example/f",
        ]
    );
    assert!(coverage(&[fields("https://a.example/f", &["e"])], &sent).is_empty());
}

/// A register with one entry whose request form is `url`, and a forms file listing
/// `url` with `listed`.
fn fixture(url: &str, listed: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let register = entry(
        "a-au",
        &format!(
            "\n[[channels]]\nkind = \"web-form\"\naddress = \"{url}\"\n\
             levers = [\"app7-no-marketing\"]\n"
        ),
    );
    std::fs::create_dir(temp.path().join("au")).unwrap();
    std::fs::write(temp.path().join("au/a-au.toml"), register.text).unwrap();
    std::fs::write(
        temp.path().join("forms.toml"),
        format!("[[form]]\nurl = \"{url}\"\n{listed}\n"),
    )
    .unwrap();
    temp
}

/// A resolver that is never asked: nothing listens there.
fn no_resolver() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    format!("http://{}/", listener.local_addr().unwrap())
}

fn run_on(temp: &tempfile::TempDir) -> ExitCode {
    run_with_issue(temp, "")
}

fn run_with_issue(temp: &tempfile::TempDir, open: &str) -> ExitCode {
    let (dir, file) = (temp.path().join("au"), temp.path().join("forms.toml"));
    run(
        &dir,
        &file,
        open,
        &loopback_agent(),
        &no_resolver(),
        Duration::ZERO,
    )
}

/// A server that answers its requests in turn with `replies`, each a status and a body.
fn serve_in_turn(replies: &[(&'static str, &'static str)]) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let replies = replies.to_vec();
    std::thread::spawn(move || {
        for ((status, body), stream) in replies.into_iter().zip(listener.incoming()) {
            let mut stream = stream.unwrap();
            let _ = stream.read(&mut [0; 4096]);
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
        }
    });
    url
}

#[test]
fn an_issue_keeps_each_form_it_names_until_that_form_can_be_checked() {
    let (gone, refused, ok) = (
        ("404 Not Found", ""),
        ("403 Forbidden", ""),
        ("200 OK", FORM),
    );
    let a = serve_in_turn(&[gone, refused, refused, ok]);
    let b = serve_in_turn(&[ok, gone, ok, ok]);
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("au")).unwrap();
    let mut list = String::new();
    for (id, url) in [("a-au", &a), ("b-au", &b)] {
        let channel = format!(
            "\n[[channels]]\nkind = \"web-form\"\naddress = \"{url}\"\n\
             levers = [\"app7-no-marketing\"]\n"
        );
        let file = entry(id, &channel);
        std::fs::write(temp.path().join("au").join(&file.name), file.text).unwrap();
        list += &format!("[[form]]\nurl = \"{url}\"\nfields = [\"email\"]\n\n");
    }
    let file = temp.path().join("forms.toml");
    std::fs::write(&file, list).unwrap();
    let dir = temp.path().join("au");
    let week = |open: &str| {
        checked(
            &dir,
            &file,
            open,
            &loopback_agent(),
            &no_resolver(),
            Duration::ZERO,
        )
    };
    let names = |report: &str, url: &str| report.contains(&format!(": web form {url}: "));

    // Week 1: a is gone, so the issue opens with it.
    let (first, code) = week("");
    assert_eq!(code, ExitCode::FAILURE);
    assert!(names(&first, &a) && !names(&first, &b), "{first}");
    // Week 2: a cannot be checked and b is gone; the new body still names a.
    let (second, code) = week(&first);
    assert_eq!(code, ExitCode::FAILURE);
    assert!(names(&second, &a) && names(&second, &b), "{second}");
    // Week 3: b is back but a still cannot be checked, so the issue stays open.
    assert_eq!(week(&second), (String::new(), ExitCode::from(HELD)));
    // Week 4: both pass.
    assert_eq!(week(&second), (String::new(), ExitCode::SUCCESS));
}

#[test]
fn run_keeps_an_issue_open_while_a_form_it_names_cannot_be_checked() {
    let url = serve_once("403 Forbidden");
    let temp = fixture(&url, "fields = [\"email\"]");
    let open = format!("- `a-au`: web form {url}: missing field `email`\n");
    assert_eq!(run_with_issue(&temp, &open), ExitCode::from(HELD));
    // A form the issue does not name may be inconclusive without holding it.
    let url = serve_once("403 Forbidden");
    let temp = fixture(&url, "fields = [\"email\"]");
    let other = format!("- `b-au`: web form {url}x: missing field `email`\n");
    assert_eq!(run_with_issue(&temp, &other), ExitCode::SUCCESS);
    // Only the issue's own lines count, not a mention in a comment or prose.
    let url = serve_once("403 Forbidden");
    let temp = fixture(&url, "fields = [\"email\"]");
    let prose = format!("The web form {url}: was slow last week.\n");
    assert_eq!(run_with_issue(&temp, &prose), ExitCode::SUCCESS);
}

#[test]
fn run_fails_on_a_form_whose_host_public_dns_says_is_gone() {
    let temp = fixture(&no_resolver(), "fields = [\"email\"]");
    let (dir, file) = (temp.path().join("au"), temp.path().join("forms.toml"));
    let resolver = serve_body("200 OK", "application/json", "{\"Status\": 3}");
    assert_eq!(
        run(
            &dir,
            &file,
            "",
            &loopback_agent(),
            &resolver,
            Duration::ZERO
        ),
        ExitCode::FAILURE
    );
}

#[test]
fn run_passes_when_every_form_has_its_fields() {
    let url = serve_body("200 OK", "text/html", FORM);
    let temp = fixture(&url, "fields = [\"email\", \"message\"]");
    assert_eq!(run_on(&temp), ExitCode::SUCCESS);
}

#[test]
fn run_fails_on_a_changed_form_or_one_the_list_lacks() {
    let url = serve_body("200 OK", "text/html", FORM);
    let changed = fixture(&url, "fields = [\"phone\"]");
    assert_eq!(run_on(&changed), ExitCode::FAILURE);
    // The register sends users to one form and the list names 2 others, which load.
    let unlisted = fixture("https://a.example/f", "unchecked = \"drawn by a script\"");
    let others = [0, 1].map(|_| serve_body("200 OK", "text/html", FORM));
    std::fs::write(
        unlisted.path().join("forms.toml"),
        others
            .iter()
            .map(|u| format!("[[form]]\nurl = \"{u}\"\nunchecked = \"drawn by a script\"\n"))
            .collect::<String>(),
    )
    .unwrap();
    assert_eq!(run_on(&unlisted), ExitCode::FAILURE);
}

#[test]
fn run_fails_on_an_unreadable_list_or_register() {
    let temp = fixture("https://a.example/f", "fields = [\"email\"]");
    let (dir, file) = (temp.path().join("au"), temp.path().join("forms.toml"));
    let missing = temp.path().join("missing");
    for (dir, file) in [(&dir, &missing), (&missing, &file)] {
        assert_eq!(
            run(
                dir,
                file,
                "",
                &loopback_agent(),
                &no_resolver(),
                Duration::ZERO
            ),
            ExitCode::FAILURE
        );
    }
    std::fs::write(&file, "form = 1\n").unwrap();
    assert_eq!(
        run(
            &dir,
            &file,
            "",
            &loopback_agent(),
            &no_resolver(),
            Duration::ZERO
        ),
        ExitCode::FAILURE
    );
    std::fs::write(dir.join("a-au.toml"), "broken = ").unwrap();
    assert!(sent_to(&ghostsoap::io::register::load(&dir).unwrap().entries).is_err());
}

#[test]
fn prints_the_fields_each_page_serves() {
    let url = serve_body("200 OK", "text/html", FORM);
    let gone = serve_once("404 Not Found");
    let pause = Duration::from_millis(300);
    let started = std::time::Instant::now();
    assert_eq!(
        print_fields(&loopback_agent(), &[url, gone], pause),
        ExitCode::FAILURE
    );
    assert!(
        started.elapsed() >= pause,
        "the second page was not requested after a pause"
    );
    let url = serve_body("200 OK", "text/html", FORM);
    assert_eq!(
        print_fields(&loopback_agent(), &[url], pause),
        ExitCode::SUCCESS
    );
}

#[test]
fn the_list_covers_every_form_the_register_sends_users_to() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let forms = parse(&std::fs::read_to_string(root.join("forms.toml")).unwrap()).unwrap();
    let files = ghostsoap::io::register::load(&root.join("../registry/au")).unwrap();
    let problems = coverage(&forms, &sent_to(&files.entries).unwrap());
    assert!(problems.is_empty(), "{problems:#?}");
}
