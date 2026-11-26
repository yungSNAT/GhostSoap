// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! `scan`: each selected entity's own page, for the user to search themselves.

use super::*;

const SCAN_NOTE: &str = "Search each page for yourself. GhostSoap itself connects to none of \
                         these sites, fills in no form and records nothing; `--open` hands \
                         each page to your browser, which connects to it. `ghostsoap plan` \
                         shows the letters it would draft.";

pub(super) fn scan(args: &ScanArgs) -> Result<(), String> {
    let register = built_in_register()?;
    let selection = Selection {
        packs: args.pack.clone(),
        entries: args.entry.clone(),
        ..Selection::default()
    };
    let mut pages = Vec::new();
    for entry in plan::scan(&register, &selection)? {
        // A retired entry's page is not offered, as `plan` offers it no step.
        let current = entry.obsolete.is_none();
        match entry.manual_check.as_ref().filter(|_| current) {
            Some(check) => {
                let route = if entry.is_overseas() {
                    "its own form only"
                } else {
                    "GhostSoap drafts letters to it"
                };
                println!(
                    "check   {}  {}: {} ({route})",
                    entry.id, entry.name, check.url
                );
                pages.push(check.url.as_str());
            }
            None => println!(
                "skip    {}: the register has no page of its own to check",
                entry.id
            ),
        }
    }
    if pages.is_empty() {
        println!("Nothing scanned: none of the selected entries has a page of its own to check.");
        return Ok(());
    }
    println!("{SCAN_NOTE}");
    if !args.open {
        return Ok(());
    }
    let failed: Vec<String> = pages
        .iter()
        .filter_map(|url| open_page(url).err())
        .collect();
    // An opener's exit status says it ran, not that a browser showed the page.
    let handed = pages.len() - failed.len();
    let noun = if handed == 1 { "page" } else { "pages" };
    println!("Handed {handed} {noun} to your browser.");
    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join("; "))
    }
}

/// The program and arguments that open `url` in the default browser on `os`
/// (`std::env::consts::OS`). No shell runs, so `?` and `&` in the URL stay as they are.
/// `system_dir` is Windows' System32 directory as the system reports it
/// (`system_directory`). `rundll32.exe` runs from there by absolute path, never by a name
/// that a search could resolve to another file, and a missing or relative directory is an error.
pub(super) fn opener(
    os: &str,
    url: &str,
    system_dir: Option<&Path>,
) -> Result<(PathBuf, Vec<String>), String> {
    let one = vec![url.to_owned()];
    Ok(match os {
        "windows" => {
            let dir = system_dir
                .filter(|dir| dir.is_absolute())
                .ok_or("the system directory is unknown, so rundll32.exe cannot be found safely")?;
            (
                dir.join("rundll32.exe"),
                vec!["url.dll,FileProtocolHandler".to_owned(), url.to_owned()],
            )
        }
        "macos" => (PathBuf::from("open"), one),
        _ => (PathBuf::from("xdg-open"), one),
    })
}

/// The System32 directory from the Windows known-folder API, not from an environment variable.
#[cfg(windows)]
fn system_directory() -> Option<PathBuf> {
    known_folders::get_known_folder_path(known_folders::KnownFolder::System)
}

#[cfg(not(windows))]
fn system_directory() -> Option<PathBuf> {
    None
}

fn open_page(url: &str) -> Result<(), String> {
    // The register check already requires https; this keeps any other scheme, or an
    // argument an opener could read as an option, from reaching the opener.
    if !url.starts_with("https://") || url.contains(char::is_whitespace) {
        return Err(format!("could not open {url}: not an https:// URL"));
    }
    let (program, args) = opener(std::env::consts::OS, url, system_directory().as_deref())
        .map_err(|e| format!("could not open {url}: {e}"))?;
    let status = std::process::Command::new(&program).args(&args).status();
    let program = program.display();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!(
            "could not open {url}: {program} exited with {status}"
        )),
        Err(e) => Err(format!("could not open {url}: {program}: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{open_page, opener};
    use std::path::{Path, PathBuf};

    #[test]
    fn only_an_https_page_reaches_the_opener() {
        for url in [
            "file:///etc/passwd",
            "mailto:privacy@example.com",
            "-a",
            "https://example.com/a b",
        ] {
            assert_eq!(
                open_page(url),
                Err(format!("could not open {url}: not an https:// URL"))
            );
        }
    }

    #[test]
    fn each_platform_gets_the_url_as_one_argument_with_no_shell() {
        let url = "https://example.com/policies?name=privacy&modal=select";
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")); // absolute on every platform
        assert_eq!(
            opener("windows", url, Some(dir)),
            Ok((
                dir.join("rundll32.exe"),
                vec!["url.dll,FileProtocolHandler".to_owned(), url.to_owned()]
            ))
        );
        let one = vec![url.to_owned()];
        for (os, program) in [
            ("macos", "open"),
            ("linux", "xdg-open"),
            ("freebsd", "xdg-open"),
        ] {
            assert_eq!(
                opener(os, url, None),
                Ok((PathBuf::from(program), one.clone()))
            );
        }
    }

    #[test]
    fn windows_runs_rundll32_from_an_absolute_system_directory_or_not_at_all() {
        let url = "https://example.com/";
        for bad in [
            None,
            Some(Path::new("")),
            Some(Path::new("System32")),
            Some(Path::new(".")),
        ] {
            assert!(opener("windows", url, bad).is_err(), "{bad:?}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn the_system_directory_is_absolute_and_holds_rundll32() {
        let dir = super::system_directory().expect("the known-folder API answers");
        assert!(
            dir.is_absolute() && dir.join("rundll32.exe").is_file(),
            "{dir:?}"
        );
    }
}
