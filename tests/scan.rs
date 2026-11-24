// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! `scan`: the pages a user checks for themselves, and `--open` handing them to the browser.

#[allow(dead_code)] // the shared helpers include some only other test files use
mod common;

use common::{ghostsoap, text};

#[test]
fn scan_lists_the_search_pages_of_people_search_sites_and_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let out = ghostsoap(tmp.path(), &["scan"], "");
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    let checks: Vec<&str> = stdout.lines().filter(|l| l.starts_with("check ")).collect();
    let ids: Vec<&str> = checks
        .iter()
        .map(|l| l.split_whitespace().nth(1).unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "id4me-au",
            "infotracer-au",
            "locate-family-au",
            "records-finder-au",
            "thryv-au"
        ],
        "{stdout}"
    );
    assert!(
        stdout.contains("check   infotracer-au  InfoTracer"),
        "{stdout}"
    );
    assert!(stdout.contains("https://au.infotracer.com/"), "{stdout}");
    assert!(
        stdout.contains("https://www.whitepages.com.au/"),
        "{stdout}"
    );
    assert!(stdout.contains("records nothing"), "{stdout}");
    assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
}

#[test]
fn scan_takes_packs_and_entries_and_skips_entries_without_a_page() {
    let tmp = tempfile::tempdir().unwrap();
    let out = ghostsoap(
        tmp.path(),
        &["scan", "--pack", "marketing", "--entry", "acceleon-au"],
        "",
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains("https://www.zoominfo.com/trust-center/your-privacy"),
        "{stdout}"
    );
    assert!(
        stdout.contains("skip    acceleon-au: the register has no page of its own to check"),
        "{stdout}"
    );
    let out = ghostsoap(tmp.path(), &["scan", "--entry", "nope-au"], "");
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("unknown entry `nope-au`"));
}

#[cfg(unix)]
mod open {
    use super::common::{ghostsoap_with, text};
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    /// A stand-in for the platform's opener that appends its arguments to `log` and exits
    /// with `code`.
    fn fake_opener(bin: &Path, log: &Path, code: i32) {
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\nexit {code}\n",
            log.display()
        );
        for name in ["xdg-open", "open"] {
            let path = bin.join(name);
            std::fs::write(&path, &script).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn open_hands_each_page_to_the_opener_as_one_argument() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let log = tmp.path().join("opened.txt");
        fake_opener(&bin, &log, 0);
        let out = ghostsoap_with(
            tmp.path(),
            &[
                "scan",
                "--entry",
                "infotracer-au",
                "--entry",
                "peopledatalabs-au",
                "--open",
            ],
            "",
            &[("PATH", &bin)],
        );
        assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "https://au.infotracer.com/\n\
             https://privacy.peopledatalabs.com/policies?name=privacy-policy\n"
        );
        assert!(text(&out.stdout).contains("Handed 2 pages to your browser"));
    }

    #[test]
    fn open_reports_a_page_the_opener_refuses_and_exits_1() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let log = tmp.path().join("opened.txt");
        fake_opener(&bin, &log, 3);
        let out = ghostsoap_with(
            tmp.path(),
            &["scan", "--entry", "locate-family-au", "--open"],
            "",
            &[("PATH", &bin)],
        );
        assert_eq!(out.status.code(), Some(1));
        assert!(
            text(&out.stderr).contains("could not open https://www.locatefamily.com/"),
            "{}",
            text(&out.stderr)
        );
    }

    #[test]
    fn open_reports_a_missing_opener() {
        let tmp = tempfile::tempdir().unwrap();
        let out = ghostsoap_with(
            tmp.path(),
            &["scan", "--entry", "locate-family-au", "--open"],
            "",
            &[("PATH", tmp.path())],
        );
        assert_eq!(out.status.code(), Some(1));
        assert!(text(&out.stderr).contains("could not open https://www.locatefamily.com/"));
    }
}
