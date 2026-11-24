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
/// `system_root` is Windows' `SystemRoot`: `rundll32.exe` runs from its `System32`, never
/// from a `PATH` search or the current directory, and a missing or relative root is an error.
pub(super) fn opener(
    os: &str,
    url: &str,
    system_root: Option<&str>,
) -> Result<(String, Vec<String>), String> {
    Ok(match os {
        "windows" => (
            rundll32_path(system_root)?,
            vec!["url.dll,FileProtocolHandler".to_owned(), url.to_owned()],
        ),
        "macos" => ("open".to_owned(), vec![url.to_owned()]),
        _ => ("xdg-open".to_owned(), vec![url.to_owned()]),
    })
}

/// `<SystemRoot>\System32\rundll32.exe` for an absolute drive or UNC `SystemRoot`.
fn rundll32_path(system_root: Option<&str>) -> Result<String, String> {
    let root = system_root.unwrap_or("").trim_end_matches(['\\', '/']);
    let b = root.as_bytes();
    let drive = b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
    if drive || root.starts_with(r"\\") {
        Ok(format!(r"{root}\System32\rundll32.exe"))
    } else {
        Err("SystemRoot is not an absolute path, so rundll32.exe cannot be found safely".to_owned())
    }
}

fn open_page(url: &str) -> Result<(), String> {
    // The register check already requires https; this keeps any other scheme, or an
    // argument an opener could read as an option, from reaching the opener.
    if !url.starts_with("https://") || url.contains(char::is_whitespace) {
        return Err(format!("could not open {url}: not an https:// URL"));
    }
    let system_root = std::env::var("SystemRoot").ok();
    let (program, args) = opener(std::env::consts::OS, url, system_root.as_deref())
        .map_err(|e| format!("could not open {url}: {e}"))?;
    match std::process::Command::new(&program).args(&args).status() {
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
        let args = vec!["url.dll,FileProtocolHandler".to_owned(), url.to_owned()];
        assert_eq!(
            opener("windows", url, Some(r"C:\Windows")),
            Ok((r"C:\Windows\System32\rundll32.exe".to_owned(), args))
        );
        let one = vec![url.to_owned()];
        assert_eq!(
            opener("macos", url, None),
            Ok(("open".to_owned(), one.clone()))
        );
        assert_eq!(
            opener("linux", url, None),
            Ok(("xdg-open".to_owned(), one.clone()))
        );
        assert_eq!(
            opener("freebsd", url, None),
            Ok(("xdg-open".to_owned(), one))
        );
    }

    #[test]
    fn windows_runs_rundll32_from_an_absolute_system_directory_or_not_at_all() {
        let url = "https://example.com/";
        let path = |root| opener("windows", url, root).map(|(program, _)| program);
        assert_eq!(
            path(Some(r"D:\WINNT\")),
            Ok(r"D:\WINNT\System32\rundll32.exe".to_owned())
        );
        assert_eq!(
            path(Some(r"\\server\share\Windows")),
            Ok(r"\\server\share\Windows\System32\rundll32.exe".to_owned())
        );
        for bad in [
            None,
            Some(""),
            Some("."),
            Some("Windows"),
            Some(r"\Windows"),
            Some("C:Windows"),
            Some("/"),
        ] {
            assert!(path(bad).is_err(), "{bad:?}");
        }
    }
}
