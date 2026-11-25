// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! End-to-end tests for `draft --format pdf`.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{PASSPHRASE, emls, ghostsoap, init, text};

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|f| f.unwrap().path())
        .filter(|p| !p.file_name().unwrap().to_str().unwrap().starts_with('.'))
        .collect();
    files.sort();
    files
}

#[test]
fn a_postal_letter_is_written_as_a_pdf_that_clean_removes() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, drafts) = (tmp.path().join("home"), tmp.path().join("drafts"));
    init(&home);
    let out = ghostsoap(
        &home,
        &[
            "draft",
            "--entry",
            "anz-au",
            "--format",
            "pdf",
            "--out",
            drafts.to_str().unwrap(),
            "--passphrase-stdin",
        ],
        &format!("{PASSPHRASE}\n"),
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("print and post to"));
    let written = files(&drafts);
    assert_eq!(written.len(), 1, "{written:?}");
    assert!(emls(&drafts).is_empty());
    assert_eq!(written[0].extension().unwrap(), "pdf");
    let pdf = fs::read_to_string(&written[0]).unwrap();
    assert!(pdf.starts_with("%PDF-1.4\n") && pdf.contains("(Name: Lara Croft) Tj T*"));

    let cleaned = ghostsoap(&home, &["clean", "--out", drafts.to_str().unwrap()], "");
    assert_eq!(cleaned.status.code(), Some(0), "{}", text(&cleaned.stderr));
    assert!(!written[0].exists());
}

#[test]
fn a_name_the_pdf_font_cannot_show_stops_the_run_before_anything_is_recorded() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, drafts) = (tmp.path().join("home"), tmp.path().join("drafts"));
    let answers = format!(
        "李小龙\n\nlee@example.com\n0400 000 001\n1940-11-27\n\
         1 Example Street, Sydney NSW 2000\n\n2000\n{PASSPHRASE}\n{PASSPHRASE}\n"
    );
    let made = ghostsoap(&home, &["init", "--passphrase-stdin"], &answers);
    assert_eq!(made.status.code(), Some(0), "{}", text(&made.stderr));
    let pass = format!("{PASSPHRASE}\n");
    let draft = |format: &str| {
        ghostsoap(
            &home,
            &[
                "draft",
                "--entry",
                "anz-au",
                "--format",
                format,
                "--out",
                drafts.to_str().unwrap(),
                "--passphrase-stdin",
            ],
            &pass,
        )
    };

    let refused = draft("pdf");
    assert_ne!(refused.status.code(), Some(0));
    let stderr = text(&refused.stderr);
    assert!(
        stderr.contains("U+674E") && stderr.contains("--format text"),
        "{stderr}"
    );
    assert!(!drafts.exists() || files(&drafts).is_empty());
    let status = ghostsoap(&home, &["status", "--passphrase-stdin"], &pass);
    assert!(
        !text(&status.stdout).contains("anz-au"),
        "{}",
        text(&status.stdout)
    );

    let written = draft("text");
    assert_eq!(written.status.code(), Some(0), "{}", text(&written.stderr));
    let files = files(&drafts);
    assert_eq!(files.len(), 1);
    assert!(fs::read_to_string(&files[0]).unwrap().contains("李小龙"));
}
