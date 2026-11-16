// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{self, Command, Output};
use std::thread::{self, JoinHandle};
use std::time::Duration;

struct Fixture {
    root: PathBuf,
    path: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let path = root.join(format!("xtask-{}-{name}", process::id()));
        fs::create_dir(&path).unwrap();
        Self { root, path }
    }

    fn write(&self, name: &str, text: &str) {
        let path = self.path.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn cargo_tree_fixture(&self, dependencies: &str) {
        self.write(
            "Cargo.toml",
            &format!(
                "[package]\nname = \"ghostsoap\"\nversion = \"0.1.0\"\n\
                 edition = \"2024\"\n[workspace]\n[lib]\npath = \"fixture.rs\"\n\
                 [dependencies]\n{dependencies}"
            ),
        );
        self.write("fixture.rs", "");
        let output = Command::new(env!("CARGO"))
            .args(["generate-lockfile", "--offline"])
            .current_dir(&self.path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn check(&self, cargo: impl AsRef<std::ffi::OsStr>) -> Output {
        self.command()
            .arg("no-network")
            .env("CARGO", cargo)
            .output()
            .unwrap()
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
        command
            .current_dir(&self.path)
            .env("CARGO_NET_OFFLINE", "true");
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Ok(path) = self.path.canonicalize()
            && path.parent() == Some(self.root.as_path())
        {
            let _ = fs::remove_dir_all(path);
        }
    }
}

#[test]
fn no_network_checks_dependencies_nested_rust_sources_and_the_build_script() {
    let fixture = Fixture::new("checks");
    fixture.write(
        "ureq/Cargo.toml",
        "[package]\nname = \"ureq\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
    );
    fixture.write("ureq/src/lib.rs", "");
    fixture.write("src/nested/plain.rs", "fn plain() {}\n");
    fixture.write(
        "src/notes.txt",
        "This file mentions std::net and is ignored.",
    );
    fixture.cargo_tree_fixture("");
    let clean = fixture.check(env!("CARGO"));
    assert_eq!(clean.status.code(), Some(0), "{clean:?}");
    assert!(String::from_utf8_lossy(&clean.stdout).contains("no socket use in 1 files"));

    fixture.cargo_tree_fixture("ureq = { path = \"ureq\" }\n");
    fixture.write(
        "src/nested/plain.rs",
        "fn plain() {}\nuse std::net::TcpStream;\n",
    );
    fixture.write("build.rs", "use std::net::UdpSocket;\n");
    let flagged = fixture.check(env!("CARGO"));
    assert_eq!(flagged.status.code(), Some(1), "{flagged:?}");
    let stdout = String::from_utf8_lossy(&flagged.stdout);
    assert!(stdout.contains("network crate in ghostsoap's dependencies: ureq"));
    assert!(stdout.contains("standard library networking at src/nested/plain.rs:2"));
    assert!(stdout.contains("standard library networking at build.rs:1"));
}

#[test]
fn no_network_fails_closed_when_cargo_or_a_source_cannot_be_read() {
    let fixture = Fixture::new("fail-closed");
    let missing = fixture.check(fixture.path.join("missing-cargo"));
    assert_eq!(missing.status.code(), Some(2), "{missing:?}");
    assert!(String::from_utf8_lossy(&missing.stderr).contains("cannot run cargo tree"));

    fixture.write("Cargo.toml", "invalid = ");
    let failed = fixture.check(env!("CARGO"));
    assert_eq!(failed.status.code(), Some(2), "{failed:?}");
    assert!(String::from_utf8_lossy(&failed.stderr).contains("cargo tree failed"));

    fixture.cargo_tree_fixture("");
    let no_src = fixture.check(env!("CARGO"));
    assert_eq!(no_src.status.code(), Some(2), "{no_src:?}");
    assert!(String::from_utf8_lossy(&no_src.stderr).contains("cannot read src"));

    fixture.write("src/plain.rs", "");
    fs::create_dir(fixture.path.join("build.rs")).unwrap();
    let unreadable = fixture.check(env!("CARGO"));
    assert_eq!(unreadable.status.code(), Some(2), "{unreadable:?}");
    assert!(String::from_utf8_lossy(&unreadable.stderr).contains("cannot read build.rs"));
}

#[test]
fn evidence_commands_report_usage_and_unreadable_registers() {
    let fixture = Fixture::new("bad-register");
    for args in [vec![], vec!["evidence-hash"], vec!["email-changes", "base"]] {
        let out = fixture.run(&args);
        assert_eq!(out.status.code(), Some(2), "{out:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("usage: xtask"));
    }
    for args in [
        &["evidence-drift", "missing"][..],
        &["email-changes", "missing", "missing"][..],
    ] {
        let out = fixture.run(args);
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("error: cannot read missing"));
    }
}

#[test]
fn email_changes_escapes_annotation_data_and_reports_unparseable_entries() {
    let fixture = Fixture::new("email-changes");
    let entry = include_str!("../../tests/fixtures/registry/no-source/no-source-au.toml");
    fixture.write("base/no-source-au.toml", entry);
    fixture.write(
        "head,%/no-source-au.toml",
        &entry.replace("privacy@example.com.au", "privacy%tag@example.com.au"),
    );
    let out = fixture.run(&["email-changes", "base", "head,%"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("::warning file=head%2C%25/no-source-au.toml,title="));
    assert!(stdout.contains("gains email privacy%25tag@example.com.au"));
    assert!(stdout.contains("1 email address(es) to check"));

    fixture.write("head,%/no-source-au.toml", "broken = ");
    let out = fixture.run(&["email-changes", "base", "head,%"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no-source-au.toml does not parse"));
}

#[test]
fn email_changes_reports_a_first_address_even_when_it_is_malformed() {
    let fixture = Fixture::new("first-email");
    let entry = include_str!("../../tests/fixtures/registry/no-source/no-source-au.toml");
    let (without_email, _) = entry.split_once("[[channels]]").unwrap();
    fixture.write("base/no-source-au.toml", without_email);
    fixture.write(
        "head/no-source-au.toml",
        &entry.replace("privacy@example.com.au", "missing-at-sign"),
    );
    let out = fixture.run(&["email-changes", "base", "head"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("gains email missing-at-sign (before: none)")
    );
}

fn serve(status: &str, media: &str, body: &str) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {media}\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    let thread = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        for line in BufReader::new(&mut stream).lines() {
            if line.unwrap().is_empty() {
                break;
            }
        }
        stream.write_all(reply.as_bytes()).unwrap();
    });
    (url, thread)
}

#[test]
fn evidence_hash_keeps_successful_results_when_another_url_fails() {
    let fixture = Fixture::new("evidence-hash");
    let (binary, binary_server) = serve("200 OK", "application/octet-stream", "abc");
    let (short, short_server) = serve("200 OK", "text/html", "<main>Too short</main>");
    let out = fixture
        .command()
        .args(["evidence-hash", &binary, &short])
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout.trim_end(),
        format!("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  {binary}")
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains(&format!("error: {short}: only ")));
    assert!(stderr.contains("the page may need a browser"));
    binary_server.join().unwrap();
    short_server.join().unwrap();
}

#[test]
fn evidence_hash_succeeds_when_every_url_hashes() {
    let fixture = Fixture::new("evidence-hash-success");
    let (url, server) = serve("200 OK", "application/octet-stream", "abc");
    let out = fixture
        .command()
        .args(["evidence-hash", &url])
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim_end(),
        format!("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  {url}")
    );
    assert!(out.stderr.is_empty());
    server.join().unwrap();
}
