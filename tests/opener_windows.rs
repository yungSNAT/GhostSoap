// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! On Windows, `scan --open` runs `rundll32.exe url.dll,FileProtocolHandler <url>` (`opener`
//! in src/cli/scanning.rs). This runs that command with a page on 127.0.0.1 and waits for the
//! default browser to connect. It opens a browser, so it is ignored by default; CI runs it on
//! its Windows runner.
#![cfg(windows)]

use std::net::TcpListener;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[test]
#[ignore = "opens the default browser; CI runs it on Windows"]
fn the_windows_opener_hands_a_page_to_the_default_browser() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
    let port = listener.local_addr().expect("local address").port();
    let url = format!("https://127.0.0.1:{port}/ghostsoap-opener-test?a=1&b=2");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(listener.accept().is_ok());
    });
    let system_root = std::env::var("SystemRoot").expect("SystemRoot is set on Windows");
    let status = Command::new(format!(r"{system_root}\System32\rundll32.exe"))
        .args(["url.dll,FileProtocolHandler", &url])
        .status()
        .expect("run rundll32.exe");
    assert!(status.success(), "rundll32.exe exited with {status}");
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(120)),
        Ok(true),
        "no browser connected to {url}"
    );
}
