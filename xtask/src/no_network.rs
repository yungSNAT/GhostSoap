// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! `xtask no-network`: fail if the `ghostsoap` package could open a network connection.
//!
//! The README promises that nothing leaves the machine from GhostSoap itself. CI already runs
//! the tests with only loopback; this check catches the promise breaking at review time
//! instead: a network crate in the package's normal dependency tree, or socket use in its
//! sources. Adding one on purpose (M5's SMTP) means editing this list in the same change.

use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, ExitCode};

/// Crates whose purpose is network I/O: HTTP, mail, sockets, DNS and TLS.
// shortcut: a list catches only the crates it names; add any new network crate here.
const NETWORK_CRATES: &[&str] = &[
    "async-imap",
    "async-net",
    "attohttpc",
    "curl",
    "curl-sys",
    "h2",
    "h3",
    "hickory-resolver",
    "hyper",
    "hyper-util",
    "imap",
    "isahc",
    "lettre",
    "minreq",
    "mio",
    "native-tls",
    "quinn",
    "reqwest",
    "rustls",
    "socket2",
    "surf",
    "tokio-native-tls",
    "tokio-rustls",
    "tokio-tungstenite",
    "trust-dns-resolver",
    "tungstenite",
    "ureq",
];

/// Standard library networking, matched as text.
// shortcut: a textual match misses `use std::{io, net}` and macros that hide the path; the
// dependency check and CI's loopback-only test run are the main guards. Parse with `syn` if
// this ever needs to be exact.
const SOCKET_WORDS: &[&str] = &[
    "std::net",
    "TcpListener",
    "TcpStream",
    "ToSocketAddrs",
    "UdpSocket",
];

/// Network crates named in `cargo tree --prefix none --format {p}` output, sorted and unique.
fn network_crates(tree: &str) -> Vec<String> {
    let mut found: Vec<String> = tree
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| NETWORK_CRATES.contains(name))
        .map(str::to_owned)
        .collect();
    found.sort();
    found.dedup();
    found
}

/// `path:line` for each source line naming standard library networking.
fn socket_uses(files: &[(String, String)]) -> Vec<String> {
    files
        .iter()
        .flat_map(|(path, text)| {
            text.lines()
                .enumerate()
                .filter(|(_, line)| SOCKET_WORDS.iter().any(|word| line.contains(word)))
                .map(move |(i, _)| format!("{path}:{}", i + 1))
        })
        .collect()
}

/// Every `.rs` file under `dir`, with its path relative to the repository root.
fn rust_files(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> io::Result<()> {
    for item in fs::read_dir(root.join(dir))? {
        let item = item?;
        let rel = dir.join(item.file_name());
        if item.file_type()?.is_dir() {
            rust_files(root, &rel, out)?;
        } else if rel.extension().is_some_and(|e| e == "rs") {
            let text = fs::read_to_string(root.join(&rel))?;
            out.push((rel.to_string_lossy().replace('\\', "/"), text));
        }
    }
    Ok(())
}

/// Run the check from the repository root. Exits 1 on a finding, 2 if it cannot run.
pub fn run() -> ExitCode {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["tree", "--locked", "-e", "normal", "-p", "ghostsoap"])
        // Every feature and target, so a dependency for another platform or an optional
        // feature cannot slip past a run on this one.
        .args(["--all-features", "--target", "all"])
        .args(["--prefix", "none", "--format", "{p}"])
        .output();
    let tree = match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
        Ok(out) => {
            eprintln!(
                "error: cargo tree failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            return ExitCode::from(2);
        }
        Err(e) => {
            eprintln!("error: cannot run cargo tree: {e}");
            return ExitCode::from(2);
        }
    };
    let root = Path::new(".");
    let mut files = Vec::new();
    if let Err(e) = rust_files(root, Path::new("src"), &mut files) {
        eprintln!("error: cannot read src: {e}");
        return ExitCode::from(2);
    }
    if let Ok(text) = fs::read_to_string(root.join("build.rs")) {
        files.push(("build.rs".to_owned(), text));
    }
    let crates = network_crates(&tree);
    let uses = socket_uses(&files);
    for name in &crates {
        println!("network crate in ghostsoap's dependencies: {name}");
    }
    for at in &uses {
        println!("standard library networking at {at}");
    }
    if crates.is_empty() && uses.is_empty() {
        println!(
            "ghostsoap has no network crate among its {} dependency lines and no socket use in {} files",
            tree.lines().count(),
            files.len()
        );
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_network_crates_once_each() {
        let tree = "ghostsoap v0.1.0 (C:\\repo)\nage v0.12.1\nureq v3.5.0\nrustls v0.23.31\n\
                    ureq v3.5.0 (*)\n";
        assert_eq!(network_crates(tree), ["rustls", "ureq"]);
    }

    #[test]
    fn passes_a_tree_without_them() {
        let tree = "ghostsoap v0.1.0\nage v0.12.1\nclap v4.6.7\nhyperloglog v0.1.0\n";
        assert!(network_crates(tree).is_empty());
    }

    #[test]
    fn finds_socket_use_by_line() {
        let files = [
            (
                "src/a.rs".to_owned(),
                "fn a() {}\nuse std::net::TcpStream;\n".to_owned(),
            ),
            (
                "src/b.rs".to_owned(),
                "let s = UdpSocket::bind(addr);\n".to_owned(),
            ),
            (
                "src/c.rs".to_owned(),
                "// no sockets here\nfn c() {}\n".to_owned(),
            ),
        ];
        assert_eq!(socket_uses(&files), ["src/a.rs:2", "src/b.rs:1"]);
    }
}
