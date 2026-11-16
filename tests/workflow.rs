// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! The M2 commands end to end: init, plan, draft, clean and law.

mod common;

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Output;

use common::{PASSPHRASE, emls, ghostsoap, ghostsoap_with, init, init_answers, text};

#[test]
fn init_draft_and_clean() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let drafts = tmp.path().join("drafts");
    let drafts_arg = drafts.to_str().unwrap();
    init(&home);
    let state = fs::read(home.join("state.age")).unwrap();
    assert!(!text(&state).contains("Croft"), "the profile is encrypted");

    let again = ghostsoap(
        &home,
        &["init", "--passphrase-stdin"],
        &init_answers(PASSPHRASE),
    );
    assert_eq!(again.status.code(), Some(1));
    let refusal = text(&again.stderr);
    assert!(refusal.contains("a profile already exists"));
    assert!(refusal.contains("also deletes the record of your letters"));

    let args = [
        "draft",
        "--entry",
        "cotality-au",
        "--out",
        drafts_arg,
        "--passphrase-stdin",
    ];
    let out = ghostsoap(&home, &args, &format!("{PASSPHRASE}\n"));
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    if cfg!(windows) {
        assert!(text(&out.stderr).contains("does not check who else can open the --out folder"));
    }
    let stdout = text(&out.stdout);
    assert!(stdout.contains("draft   cotality-au  email to AU-Privacy@Cotality.com"));
    let files = emls(&drafts);
    assert_eq!(files.len(), 1);
    let eml = fs::read_to_string(&files[0]).unwrap();
    assert!(eml.starts_with("To: AU-Privacy@Cotality.com\r\nSubject: Privacy Act request [GS-"));
    assert!(eml.contains("Name: Lara Croft"));
    assert!(
        !eml.contains("1968-02-14"),
        "Cotality does not get the date of birth"
    );

    let wrong = ghostsoap(&home, &args, "not the passphrase\n");
    assert_eq!(wrong.status.code(), Some(1));
    assert!(text(&wrong.stderr).contains("error: wrong passphrase"));

    let out = ghostsoap(&home, &["clean", "--out", drafts_arg], "");
    assert_eq!(out.status.code(), Some(0));
    assert!(text(&out.stdout).contains("Deleted 1 drafts"));
    assert!(drafts.exists(), "a folder named with --out stays");
    assert_eq!(fs::read_dir(&drafts).unwrap().count(), 0);
}

/// Draft one letter to `out` with the environment variables `vars`; `extra` adds flags.
fn draft_to(home: &Path, out: &Path, vars: &[(&str, &Path)], extra: &[&str]) -> Output {
    let mut args = vec![
        "draft",
        "--entry",
        "cotality-au",
        "--passphrase-stdin",
        "--out",
    ];
    args.push(out.to_str().unwrap());
    args.extend(extra);
    ghostsoap_with(home, &args, &format!("{PASSPHRASE}\n"), vars)
}

#[test]
fn draft_warns_about_a_repository_or_sync_folder_but_still_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    init(&home);
    // A home folder with a Dropbox in it, a OneDrive folder and a linked Git worktree.
    let [user, onedrive, repo] = ["user", "OneDrive", "repo"].map(|d| tmp.path().join(d));
    for dir in [&user.join("Dropbox"), &onedrive, &repo] {
        fs::create_dir_all(dir).unwrap();
    }
    fs::write(
        repo.join(".git"),
        "gitdir: ../elsewhere/.git/worktrees/repo\n",
    )
    .unwrap();
    let empty = Path::new("");
    let vars = [
        ("HOME", user.as_path()),
        ("USERPROFILE", &user),
        ("OneDrive", &onedrive),
        ("OneDriveCommercial", empty),
        ("OneDriveConsumer", empty),
    ];
    let warning = "warning: the drafts folder is inside the";
    for (out, expected) in [
        (repo.join("drafts"), "Git working tree|be committed"),
        (onedrive.join("drafts"), "OneDrive folder|be uploaded"),
        (user.join("Dropbox/drafts"), "Dropbox folder|be uploaded"),
        (tmp.path().join("plain"), ""),
    ] {
        let run = draft_to(&home, &out, &vars, &[]);
        let stderr = text(&run.stderr);
        assert_eq!(run.status.code(), Some(0), "not a refusal: {stderr}");
        assert_eq!(emls(&out).len(), 1, "the draft is still written");
        assert_eq!(
            stderr.matches(warning).count(),
            usize::from(!expected.is_empty())
        );
        for part in expected.split('|').filter(|p| !p.is_empty()) {
            assert!(stderr.contains(part), "{part:?} in {stderr}");
        }
    }
    let copy = draft_to(&home, &repo, &vars, &["--copy"]);
    assert_eq!(copy.status.code(), Some(0));
    assert!(
        !text(&copy.stderr).contains(warning),
        "--copy writes nothing"
    );
}

#[test]
fn copy_prints_instead_of_writing() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let drafts = tmp.path().join("drafts");
    init(&home);
    let args = [
        "draft",
        "--entry",
        "experian-marketing-au",
        "--copy",
        "--out",
        drafts.to_str().unwrap(),
        "--passphrase-stdin",
    ];
    let out = ghostsoap(&home, &args, &format!("{PASSPHRASE}\n"));
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.contains("==== experian-marketing-au (web form)"));
    assert!(stdout.contains("==== experian-marketing-au (post)"));
    assert!(stdout.contains("PO Box 7405, St Kilda Road VIC 3004"));
    assert!(!drafts.exists());
}

#[test]
fn drafts_default_to_the_data_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    init(&home);
    let args = ["draft", "--entry", "cotality-au", "--passphrase-stdin"];
    let out = ghostsoap(&home, &args, &format!("{PASSPHRASE}\n"));
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(!text(&out.stdout).contains("mailto:"), "links are opt-in");
    let with_link = [&args[..], &["--mailto"]].concat();
    let out = ghostsoap(&home, &with_link, &format!("{PASSPHRASE}\n"));
    assert!(
        text(&out.stdout).contains("too long for a mailto: link"),
        "a 3-request letter is too long for a link"
    );
    assert_eq!(
        emls(&home.join("drafts")).len(),
        1,
        "the second draft replaces the first, which was never recorded as sent"
    );
    let out = ghostsoap(&home, &["clean"], "");
    assert_eq!(out.status.code(), Some(0));
    assert!(!home.join("drafts").exists());
    assert!(home.join("state.age").exists(), "clean keeps the profile");
}

#[test]
fn init_rejects_a_short_passphrase_and_bad_details() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let out = ghostsoap(
        &home,
        &["init", "--passphrase-stdin"],
        &init_answers("short"),
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("at least 12 characters"));
    let different = format!(
        "{}another long passphrase\n",
        init_answers(PASSPHRASE)
            .strip_suffix(&format!("{PASSPHRASE}\n"))
            .unwrap()
    );
    let out = ghostsoap(&home, &["init", "--passphrase-stdin"], &different);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("the passphrases do not match"));
    let answers = init_answers(PASSPHRASE).replace("1968-02-14", "14/02/1968");
    let out = ghostsoap(&home, &["init", "--passphrase-stdin"], &answers);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("is not a date"));
    assert!(
        !text(&out.stderr).contains("14/02/1968"),
        "errors do not echo profile values"
    );
    assert!(!home.join("state.age").exists());
}

#[test]
fn drafting_with_an_incomplete_profile_reports_omitted_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let input = format!("Lara Croft\n\n\n\n\n\n\n\n{PASSPHRASE}\n{PASSPHRASE}\n");
    let out = ghostsoap(&home, &["init", "--passphrase-stdin"], &input);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let out = ghostsoap(
        &home,
        &[
            "draft",
            "--entry",
            "cotality-au",
            "--copy",
            "--passphrase-stdin",
        ],
        &format!("{PASSPHRASE}\n"),
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.contains("so the letter leaves it out"), "{stdout}");
    assert!(stdout.contains("Name: Lara Croft"), "{stdout}");
    assert!(!home.join("drafts").exists());
}

#[test]
fn clean_reports_edited_pending_and_interrupted_drafts() {
    use ghostsoap::io::drafts;

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("drafts");
    let edited = "example-au-GS-ABCDE.eml";
    let pending = "example-au-GS-FGHJK.eml";
    let leftover = ".example-au-GS-01234.eml.42.tmp";
    let path = drafts::write(&dir, edited, "original letter").unwrap();
    fs::write(path, "edited letter").unwrap();
    fs::write(dir.join(pending), "unconfirmed letter").unwrap();
    fs::write(dir.join(leftover), "interrupted letter").unwrap();
    writeln!(
        fs::OpenOptions::new()
            .append(true)
            .open(dir.join(".ghostsoap-drafts"))
            .unwrap(),
        "pending {pending}"
    )
    .unwrap();

    let out = ghostsoap(tmp.path(), &["clean", "--out", dir.to_str().unwrap()], "");
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.contains("Deleted 0 drafts"), "{stdout}");
    assert!(stdout.contains(&format!("kept    {edited}:")), "{stdout}");
    for name in [pending, leftover] {
        assert!(stdout.contains(&format!("check   {name}:")), "{stdout}");
    }
    for (name, body) in [
        (edited, "edited letter"),
        (pending, "unconfirmed letter"),
        (leftover, "interrupted letter"),
    ] {
        assert_eq!(fs::read_to_string(dir.join(name)).unwrap(), body);
    }
}

#[test]
fn clean_reports_a_draft_it_has_no_record_of_and_leaves_it() {
    use ghostsoap::io::drafts;

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("drafts");
    drafts::write(&dir, "example-au-GS-ABCDE.eml", "recorded letter").unwrap();
    // As a draft whose manifest record was lost: the letter is there, its record is not.
    let unrecorded = "example-au-GS-KLMNP.eml";
    fs::write(dir.join(unrecorded), "a letter").unwrap();

    let out = ghostsoap(tmp.path(), &["clean", "--out", dir.to_str().unwrap()], "");
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.contains("Deleted 1 drafts"), "{stdout}");
    let check = format!("check   {unrecorded}: GhostSoap has no record of writing it");
    assert!(stdout.contains(&check), "{stdout}");
    assert_eq!(
        fs::read_to_string(dir.join(unrecorded)).unwrap(),
        "a letter"
    );
}

#[test]
fn draft_without_a_profile_says_to_run_init() {
    let tmp = tempfile::tempdir().unwrap();
    let args = [
        "draft",
        "--entry",
        "cotality-au",
        "--out",
        tmp.path().to_str().unwrap(),
        "--passphrase-stdin",
    ];
    let out = ghostsoap(&tmp.path().join("home"), &args, &format!("{PASSPHRASE}\n"));
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("run `ghostsoap init` first"));
}

#[test]
fn plan_needs_no_profile_and_shows_overseas_sites() {
    let tmp = tempfile::tempdir().unwrap();
    let out = ghostsoap(tmp.path(), &["plan", "--pack", "people-search"], "");
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.contains("letter  acceleon-au  by email to privacy@acceleon.com.au"));
    assert!(stdout.contains(
        "    app7-no-marketing (APP 7.6(c), 7.7(a); a reasonable period, OAIC guide 30 days)"
    ));
    assert!(stdout.contains(
        "manual  locate-family-au: run from overseas; GhostSoap has not established that \
         Australian privacy law reaches it, so it drafts no letter. Search for yourself at"
    ));
    assert!(!tmp.path().join("state.age").exists());
}

#[test]
fn overseas_services_get_a_manual_step_without_a_search() {
    let tmp = tempfile::tempdir().unwrap();
    let out = ghostsoap(tmp.path(), &["plan", "--pack", "recruitment"], "");
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains(
        "manual  linkedin-au: run from overseas; GhostSoap has not established that Australian \
         privacy law reaches it, so it drafts no letter. See \
         https://www.linkedin.com/help/linkedin/answer/63, then use its own opt-out or privacy \
         request form at https://www.linkedin.com/help/linkedin/ask/TSO-DPO"
    ));
}

#[test]
fn marketing_pack_shows_the_contact_data_brokers_own_forms() {
    let tmp = tempfile::tempdir().unwrap();
    let out = ghostsoap(tmp.path(), &["plan", "--pack", "marketing"], "");
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    for (id, page, form) in [
        (
            "contactout-au",
            "https://contactout.com/privacy_data_subjects",
            "https://contactout.com/optout",
        ),
        (
            "peopledatalabs-au",
            "https://privacy.peopledatalabs.com/policies?name=privacy-policy",
            "https://privacy.peopledatalabs.com/policies?modal=select-requester",
        ),
        (
            "rocketreach-au",
            "https://rocketreach.co/privacy",
            "https://rocketreach.co/remove-profile",
        ),
        (
            "snovio-au",
            "https://snov.io/privacy-policy",
            "https://app.snov.io/clear-email",
        ),
        (
            "uplead-au",
            "https://support.uplead.com/en/articles/10836637-data-removal-requests-handled-correctly",
            "https://www.uplead.com/opt-out/",
        ),
    ] {
        let line = stdout
            .lines()
            .find(|l| l.starts_with(&format!("manual  {id}: run from overseas;")))
            .unwrap_or_else(|| panic!("no manual step for {id}"));
        assert!(
            line.contains(&format!(
                "See {page}, then use its own opt-out or privacy request form at {form} (effort:"
            )),
            "{line}"
        );
    }
}

#[test]
fn plan_refuses_access_levers_in_bulk() {
    let tmp = tempfile::tempdir().unwrap();
    let out = ghostsoap(
        tmp.path(),
        &["plan", "--pack", "credit", "--lever", "s20r-access"],
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("must be chosen for one entity at a time"));
    let out = ghostsoap(
        tmp.path(),
        &["plan", "--entry", "talefin-au", "--lever", "s20r-access"],
        "",
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(text(&out.stdout).contains("warning: access requests return personal information"));
    let out = ghostsoap(
        tmp.path(),
        &["plan", "--entry", "talefin-au", "--lever", "app99"],
        "",
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("unknown lever"));
}

#[test]
fn law_lists_every_lever_with_its_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let out = ghostsoap(tmp.path(), &["law"], "");
    assert_eq!(out.status.code(), Some(0));
    let stdout = text(&out.stdout);
    for line in [
        "s20r-access: Give me access to my credit reporting information",
        "    deadline: within a reasonable period, but not longer than 10 days, after the request is made (section 20R(3))",
        "spam-unsubscribe: Stop sending me commercial electronic messages",
        "    Privacy Act 1988 (Cth), compilation C2026C00227, in force from 4 June 2026",
        "Law status, read 5 November 2026 unless a line says otherwise:",
        "    Children's Online Privacy Code (read 6 November 2026): s 26GC(10) requires",
        "    From 1 January 2027: the Interactive Gambling Amendment (Gambling Reform) Act 2026, sch 1 items 46 and 47, adds s 33C(1)(h) and inserts 'and subsection 62P(2)' in APP 7.8(aa).",
        "APP 7.8 limits APP 7, which the app7 levers rest on, so reread it then.",
        "    Tranche 2: the Privacy Amendment (Personal Data Protection) Bill 2026 is an Exposure Draft, not law",
        "    OAIC complaints: as at February 2026",
        "more than 12 months after you became aware of the act or practice (s 41(1)(c)).",
    ] {
        assert!(stdout.contains(line), "missing {line:?}");
    }
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
    let named = |e: &fs::DirEntry| {
        e.file_name()
            .to_string_lossy()
            .starts_with("ghostsoap-export-")
    };
    let entries = fs::read_dir(tmp.path()).unwrap().map(Result::unwrap);
    assert_eq!(entries.filter(named).count(), 1);
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
