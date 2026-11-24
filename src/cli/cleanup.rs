// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! `export`, `clean` and `forget`: copying out and deleting what GhostSoap keeps.

use super::*;

pub(super) fn export(
    out: Option<PathBuf>,
    escalation: Option<String>,
    passphrase_stdin: bool,
) -> Result<(), String> {
    if let Some(typed) = escalation {
        return export_escalation(out, &typed, passphrase_stdin);
    }
    let dir = state::data_dir().map_err(|e| e.to_string())?;
    let date = today().to_string();
    let path = out.unwrap_or_else(|| dir.join(format!("ghostsoap-export-{date}.toml")));
    let passphrase = Input::new(passphrase_stdin)
        .passphrase("Passphrase: ")
        .map_err(|e| e.to_string())?;
    let receipt = state::export(&dir, passphrase, &path, &date).map_err(|e| e.to_string())?;
    println!("Exported your profile and records to {}.", path.display());
    println!(
        "Manifest: {} (sha256: {}, audit chain: {}).",
        receipt.manifest.display(),
        receipt.sha256,
        receipt.audit
    );
    let folder = path.parent().unwrap_or(Path::new("."));
    warn_exposed(folder, ("the export's folder", "the plaintext export"));
    eprintln!(
        "warning: {} is plaintext: anyone who can open it can read your profile and every \
         letter's record. Keep it private, and delete it when you no longer need it.",
        path.display()
    );
    Ok(())
}

fn export_escalation(
    out: Option<PathBuf>,
    typed_tag: &str,
    passphrase_stdin: bool,
) -> Result<(), String> {
    let dir = state::data_dir().map_err(|e| e.to_string())?;
    let day = today();
    let tag = letters::normalise_tag(typed_tag);
    let bundle_dir = out.unwrap_or_else(|| dir.join(format!("ghostsoap-escalation-{tag}-{day}")));
    let passphrase = Input::new(passphrase_stdin)
        .passphrase("Passphrase: ")
        .map_err(|e| e.to_string())?;
    let state = state::load(&dir, passphrase).map_err(|e| e.to_string())?;
    let register = built_in_register()?;
    let entry = state
        .message(&tag)
        .and_then(|message| register.entries.get(&message.entry));
    let receipt = io::bundle::write_escalation_bundle(&bundle_dir, &state, &tag, entry, day)
        .map_err(|e| e.to_string())?;
    println!(
        "Exported OAIC escalation bundle to {}.",
        receipt.dir.display()
    );
    println!(
        "Manifest: {} ({} files, audit chain: {}).",
        receipt.manifest.display(),
        receipt.files.len(),
        receipt.audit
    );
    warn_exposed(
        &receipt.dir,
        ("the escalation bundle's folder", "the escalation bundle"),
    );
    Ok(())
}

pub(super) fn clean(out: Option<PathBuf>) -> Result<(), String> {
    let own_folder = out.is_none();
    let dir = drafts_dir(out.as_deref())?;
    let cleaned = io::drafts::clean(&dir, own_folder).map_err(|e| e.to_string())?;
    report_cleaned(&dir, &cleaned);
    Ok(())
}

/// Delete the state, exports in the data folder and the drafts GhostSoap wrote, once the
/// user confirms the list. A path that cannot be deleted is reported and the rest go on.
pub(super) fn forget(no_passphrase: bool, yes: bool, passphrase_stdin: bool) -> Result<(), String> {
    let dir = state::data_dir().map_err(|e| e.to_string())?;
    let mut input = Input::new(passphrase_stdin);
    let files = state::own_files(&dir).map_err(|e| e.to_string())?;
    let bundles = io::bundle::own_escalation_bundles(&dir).map_err(|e| e.to_string())?;
    let mut folders = draft_folders(&dir, no_passphrase, &mut input)?;
    folders.retain(|folder| folder.exists());
    if files.is_empty() && bundles.is_empty() && folders.is_empty() {
        println!("GhostSoap has nothing to delete in {}.", dir.display());
        return Ok(());
    }
    let lock = state::lock(&dir).map_err(|e| e.to_string())?;
    list_forgotten(&files, &bundles, &folders, no_passphrase);
    if !yes
        && input
            .line("Type forget to delete them: ")
            .map_err(|e| e.to_string())?
            != "forget"
    {
        return Err("nothing was deleted".to_owned());
    }
    let own = io::drafts::default_dir(&dir);
    let mut failed = 0;
    for file in &files {
        let deleted = std::fs::remove_file(file).map(|()| println!("Deleted {}.", file.display()));
        tally(file, deleted, &mut failed);
    }
    for bundle in &bundles {
        let cleaned = io::bundle::clean_escalation_bundle(bundle);
        tally(
            bundle,
            cleaned.map(|c| report_cleaned_bundle(bundle, &c)),
            &mut failed,
        );
    }
    for folder in &folders {
        let cleaned = refuse_link(folder).and_then(|()| io::drafts::clean(folder, *folder == own));
        tally(
            folder,
            cleaned.map(|c| report_cleaned(folder, &c)),
            &mut failed,
        );
    }
    drop(lock);
    // The data folder goes too unless it holds files GhostSoap did not write.
    let _ = std::fs::remove_dir(&dir);
    match failed {
        0 => Ok(()),
        n => Err(format!("{n} could not be deleted; see the errors above")),
    }
}

/// Show what `forget` will delete, and what it cannot look for.
pub(super) fn list_forgotten(
    files: &[PathBuf],
    bundles: &[PathBuf],
    folders: &[PathBuf],
    no_passphrase: bool,
) {
    println!("GhostSoap will delete:");
    for file in files {
        println!("    {}", file.display());
    }
    for bundle in bundles {
        println!(
            "    the escalation bundle in {} (known bundle files only; other files stay)",
            bundle.display()
        );
    }
    for folder in folders {
        println!(
            "    the drafts it wrote in {} that still hold what it wrote; other files stay",
            io::exposure::display(folder)
        );
    }
    if no_passphrase {
        println!("Without the passphrase, folders named with `draft --out` are not looked in.");
    }
    println!(
        "GhostSoap keeps nothing in the system keyring. Deleting does not erase: the disk may \
         still hold what was deleted."
    );
}

/// GhostSoap's own drafts folder, then each other folder the state records a draft in.
pub(super) fn draft_folders(
    dir: &Path,
    no_passphrase: bool,
    input: &mut Input,
) -> Result<Vec<PathBuf>, String> {
    let own = io::drafts::default_dir(dir);
    let mut seen = vec![io::exposure::resolve(&own)];
    let mut folders = vec![own];
    if no_passphrase || !state::state_path(dir).exists() {
        return Ok(folders);
    }
    let passphrase = input
        .passphrase("Passphrase: ")
        .map_err(|e| e.to_string())?;
    let state = state::load(dir, passphrase).map_err(|e| e.to_string())?;
    for draft in state.messages.iter().filter_map(|m| m.draft.as_ref()) {
        // A folder reached 2 ways, such as the drafts folder recorded with links resolved,
        // is cleaned once.
        let folder = PathBuf::from(&draft.folder);
        let real = io::exposure::resolve(&folder);
        if !seen.contains(&real) {
            seen.push(real);
            folders.push(folder);
        }
    }
    Ok(folders)
}

/// A folder `forget` cleans must still be a folder: one replaced by a link (or, on Windows, a
/// junction) since the state recorded it may lead anywhere, so it is left alone.
pub(super) fn refuse_link(folder: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(folder)?.file_type().is_symlink() {
        return Err(std::io::Error::other(
            "it is now a link, so GhostSoap leaves it alone",
        ));
    }
    Ok(())
}

/// Report a path `forget` could not delete, and count it.
pub(super) fn tally(path: &Path, result: std::io::Result<()>, failed: &mut usize) {
    if let Err(e) = result {
        *failed += 1;
        eprintln!("error: cannot delete {}: {e}", path.display());
    }
}

fn report_cleaned_bundle(dir: &Path, cleaned: &io::bundle::CleanedBundle) {
    println!(
        "Deleted {} escalation bundle files from {}.",
        cleaned.deleted,
        dir.display()
    );
    for name in &cleaned.kept {
        println!("kept    {name}: unrelated file left in {}", dir.display());
    }
}

/// Print what `clean` did in `dir`.
pub(super) fn report_cleaned(dir: &Path, cleaned: &io::drafts::Cleaned) {
    println!("Deleted {} drafts from {}.", cleaned.deleted, dir.display());
    for name in &cleaned.kept {
        println!("kept    {name}: it no longer holds the letter GhostSoap wrote");
    }
    for name in &cleaned.unconfirmed {
        println!("check   {name}: its write never finished recording; delete it if unwanted");
    }
    for name in &cleaned.leftovers {
        println!("check   {name}: left by a write that stopped part-way; delete it if unwanted");
    }
    for name in &cleaned.unrecorded {
        println!("check   {name}: GhostSoap has no record of writing it; delete it if unwanted");
    }
}

/// The folder `--out` names, else the default drafts folder in the data directory.
pub(super) fn drafts_dir(out: Option<&Path>) -> Result<PathBuf, String> {
    match out {
        Some(dir) => Ok(dir.to_owned()),
        None => state::data_dir()
            .map(|d| io::drafts::default_dir(&d))
            .map_err(|e| e.to_string()),
    }
}
