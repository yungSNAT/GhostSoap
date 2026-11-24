// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Export and forget, end to end.

use super::*;

fn count_default_exports(dir: &Path, suffix: &str) -> usize {
    fs::read_dir(dir)
        .unwrap()
        .filter(|e| {
            let n = e
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .into_owned();
            n.starts_with("ghostsoap-export-") && n.ends_with(suffix)
        })
        .count()
}

#[test]
fn export_writes_the_state_as_plaintext_once_and_never_over_a_file() {
    let tmp = tempfile::tempdir().unwrap();
    init(tmp.path());
    let out = tmp.path().join("export.toml");
    let path = out.to_str().unwrap();
    let input = format!("{PASSPHRASE}\n");
    let export = || {
        let args = ["export", "--out", path, "--passphrase-stdin"];
        ghostsoap(tmp.path(), &args, &input)
    };
    let first = export();
    assert_eq!(first.status.code(), Some(0), "{}", text(&first.stderr));
    assert!(text(&first.stderr).contains(&format!("warning: {path} is plaintext")));
    let exported = fs::read_to_string(&out).unwrap();
    assert!(exported.starts_with("# GhostSoap export of "), "{exported}");
    assert!(exported.contains("\nformat = 2\n"), "{exported}");
    assert!(exported.contains("Lara Croft"), "{exported}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&out).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let again = export();
    assert_eq!(again.status.code(), Some(1));
    assert!(
        text(&again.stderr).contains("already exists"),
        "{}",
        text(&again.stderr)
    );
    assert_eq!(fs::read_to_string(&out).unwrap(), exported);
    let nowhere = tmp.path().join("no such folder").join("export.toml");
    let args = [
        "export",
        "--out",
        nowhere.to_str().unwrap(),
        "--passphrase-stdin",
    ];
    let failed = ghostsoap(tmp.path(), &args, &input);
    assert_eq!(failed.status.code(), Some(1), "{}", text(&failed.stderr));
    assert!(!nowhere.exists());
    let default = ghostsoap(tmp.path(), &["export", "--passphrase-stdin"], &input);
    assert_eq!(default.status.code(), Some(0), "{}", text(&default.stderr));
    assert_eq!(
        (
            count_default_exports(tmp.path(), ".toml"),
            count_default_exports(tmp.path(), ".toml.sha256"),
        ),
        (1, 1)
    );
}

#[cfg(unix)]
#[test]
fn export_refuses_a_link_at_its_destination() {
    let tmp = tempfile::tempdir().unwrap();
    init(tmp.path());
    let target = tmp.path().join("elsewhere.toml");
    let link = tmp.path().join("export.toml");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let args = [
        "export",
        "--out",
        link.to_str().unwrap(),
        "--passphrase-stdin",
    ];
    let out = ghostsoap(tmp.path(), &args, &format!("{PASSPHRASE}\n"));
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    assert!(!target.exists());
}

/// `forget` asks first, then deletes the state, exports and the drafts GhostSoap wrote,
/// wherever the state records them, and leaves every other file.
#[test]
fn forget_deletes_what_ghostsoap_wrote_after_confirmation_and_nothing_else() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let out = tmp.path().join("out");
    init(&home);
    let pass = format!("{PASSPHRASE}\n");
    let drafted = draft_to(&home, &out, &[], &[]);
    assert_eq!(drafted.status.code(), Some(0), "{}", text(&drafted.stderr));
    let own = ["draft", "--entry", "hays-au", "--passphrase-stdin"];
    assert_eq!(ghostsoap(&home, &own, &pass).status.code(), Some(0));
    let exported = ghostsoap(&home, &["export", "--passphrase-stdin"], &pass);
    assert_eq!(
        exported.status.code(),
        Some(0),
        "{}",
        text(&exported.stderr)
    );
    fs::write(out.join("notes.txt"), "mine").unwrap();
    assert_eq!(emls(&out).len(), 1);

    let declined = ghostsoap(
        &home,
        &["forget", "--passphrase-stdin"],
        &format!("{pass}no\n"),
    );
    assert_eq!(declined.status.code(), Some(1));
    assert!(text(&declined.stderr).contains("nothing was deleted"));
    let listed = text(&declined.stdout);
    assert!(listed.contains(&home.join("state.age").display().to_string()));
    // Each folder once; the recorded `--out` folder may be shown with links resolved.
    let sep = std::path::MAIN_SEPARATOR;
    for folder in ["drafts", "out"] {
        let line = format!("{sep}{folder} that still hold what it wrote");
        assert_eq!(listed.matches(&line).count(), 1, "{listed}");
    }
    assert!(listed.contains("ghostsoap-export-"), "{listed}");
    assert!(home.join("state.age").exists());

    let forgot = ghostsoap(
        &home,
        &["forget", "--passphrase-stdin"],
        &format!("{pass}forget\n"),
    );
    assert_eq!(forgot.status.code(), Some(0), "{}", text(&forgot.stderr));
    assert!(!home.exists(), "{}", text(&forgot.stdout));
    assert!(emls(&out).is_empty());
    assert_eq!(fs::read_to_string(out.join("notes.txt")).unwrap(), "mine");
}

#[test]
fn forget_without_the_passphrase_deletes_the_state_and_says_what_it_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    init(&home);
    let forgot = ghostsoap(&home, &["forget", "--no-passphrase", "--yes"], "");
    assert_eq!(forgot.status.code(), Some(0), "{}", text(&forgot.stderr));
    assert!(text(&forgot.stdout).contains("--out"));
    assert!(!home.exists());

    // Files named like an export that GhostSoap did not write stay.
    init(&home);
    let yours = [
        (
            "ghostsoap-export-notes.toml",
            "# GhostSoap export of 2026-11-17. mine",
        ),
        ("ghostsoap-export-2026-11-17.toml", "my own notes"),
    ];
    for (name, body) in yours {
        fs::write(home.join(name), body).unwrap();
    }
    let forgot = ghostsoap(&home, &["forget", "--no-passphrase", "--yes"], "");
    assert_eq!(forgot.status.code(), Some(0), "{}", text(&forgot.stderr));
    assert!(!home.join("state.age").exists());
    for (name, body) in yours {
        assert_eq!(fs::read_to_string(home.join(name)).unwrap(), body);
        fs::remove_file(home.join(name)).unwrap();
    }
    fs::remove_dir(&home).unwrap();
    let nothing = ghostsoap(&home, &["forget", "--no-passphrase", "--yes"], "");
    assert_eq!(nothing.status.code(), Some(0));
    assert!(text(&nothing.stdout).contains("nothing to delete"));
}

#[cfg(unix)]
#[test]
fn forget_reports_a_folder_it_cannot_clean_and_deletes_the_rest() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let out = tmp.path().join("out");
    init(&home);
    assert_eq!(draft_to(&home, &out, &[], &[]).status.code(), Some(0));
    fs::set_permissions(&out, fs::Permissions::from_mode(0o500)).unwrap();
    let pass = format!("{PASSPHRASE}\n");
    let forgot = ghostsoap(&home, &["forget", "--passphrase-stdin", "--yes"], &pass);
    fs::set_permissions(&out, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(forgot.status.code(), Some(1));
    let errors = text(&forgot.stderr);
    assert!(
        errors.contains(&format!("error: cannot delete {}", out.display())),
        "{errors}"
    );
    assert!(errors.contains("1 could not be deleted"), "{errors}");
    assert!(!home.join("state.age").exists());
    assert_eq!(emls(&out).len(), 1);
}

/// A recorded `--out` folder replaced by a link since the draft is left alone.
#[test]
fn forget_leaves_a_recorded_folder_that_is_now_a_link() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let out = tmp.path().join("out");
    let moved = tmp.path().join("moved");
    init(&home);
    assert_eq!(draft_to(&home, &out, &[], &[]).status.code(), Some(0));
    fs::rename(&out, &moved).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&moved, &out).unwrap();
    #[cfg(windows)]
    if let Err(e) = std::os::windows::fs::symlink_dir(&moved, &out) {
        // Windows needs Developer Mode or an administrator to make links.
        eprintln!("skipped: cannot create a directory link here: {e}");
        return;
    }
    let pass = format!("{PASSPHRASE}\n");
    let forgot = ghostsoap(&home, &["forget", "--passphrase-stdin", "--yes"], &pass);
    assert_eq!(forgot.status.code(), Some(1));
    assert!(
        text(&forgot.stderr).contains("is now a link"),
        "{}",
        text(&forgot.stderr)
    );
    assert_eq!(emls(&moved).len(), 1);
    assert!(!home.join("state.age").exists());
}
