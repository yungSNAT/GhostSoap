// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! `scan`: each selected entity's own page, for the user to search themselves.

use super::*;

const SCAN_NOTE: &str = "Search each page for yourself. GhostSoap sends nothing to these sites, \
                         fills in no form and records nothing; `ghostsoap plan` shows the \
                         letters it would draft.";

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
    let opened = pages.len() - failed.len();
    let noun = if opened == 1 { "page" } else { "pages" };
    println!("Opened {opened} {noun} in your browser.");
    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join("; "))
    }
}

/// The program and arguments that open `url` in the default browser on `os`
/// (`std::env::consts::OS`). No shell runs, so `?` and `&` in the URL stay as they are.
pub(super) fn opener(os: &str, url: &str) -> (&'static str, Vec<String>) {
    match os {
        "windows" => (
            "rundll32.exe",
            vec!["url.dll,FileProtocolHandler".to_owned(), url.to_owned()],
        ),
        "macos" => ("open", vec![url.to_owned()]),
        _ => ("xdg-open", vec![url.to_owned()]),
    }
}

fn open_page(url: &str) -> Result<(), String> {
    let (program, args) = opener(std::env::consts::OS, url);
    match std::process::Command::new(program).args(&args).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!(
            "could not open {url}: {program} exited with {status}"
        )),
        Err(e) => Err(format!("could not open {url}: {program}: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::opener;

    #[test]
    fn each_platform_gets_the_url_as_one_argument_with_no_shell() {
        let url = "https://example.com/policies?name=privacy&modal=select";
        assert_eq!(
            opener("windows", url),
            (
                "rundll32.exe",
                vec!["url.dll,FileProtocolHandler".to_owned(), url.to_owned()]
            )
        );
        assert_eq!(opener("macos", url), ("open", vec![url.to_owned()]));
        assert_eq!(opener("linux", url), ("xdg-open", vec![url.to_owned()]));
        assert_eq!(opener("freebsd", url), ("xdg-open", vec![url.to_owned()]));
    }
}
