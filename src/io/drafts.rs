// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! The drafts folder: plaintext letters waiting to be sent (plan section 5).
//!
//! Each folder has a manifest, `.ghostsoap-drafts`, holding the latest record for each name.
//! `<sha256> <name>` is saved before the complete draft is linked into place. Older
//! `pending <name>` records remain readable. The manifest is only ever replaced whole, by renaming
//! a complete new copy over it, so an interrupted update leaves the previous one and a link at
//! its name is replaced rather than written through. `clean` deletes only committed drafts that
//! still hold those bytes; a file put at a draft's name with exactly those bytes counts as that
//! draft. A lock file serialises `write` and `clean` between GhostSoap processes; other
//! programs changing the folder at the same moment are outside this guarantee.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::io::state::{Lock, create_private_dir};

const MANIFEST: &str = ".ghostsoap-drafts";
const LOCK: &str = ".ghostsoap-drafts.lock";
const PENDING: &str = "pending";

/// What `clean` did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Cleaned {
    /// Drafts deleted.
    pub deleted: usize,
    /// Committed names whose file no longer holds what GhostSoap wrote, left alone.
    pub kept: Vec<String>,
    /// Names whose write never committed but whose file exists, left alone.
    pub unconfirmed: Vec<String>,
    /// Temporary files from a write that stopped part-way, left for the user to check.
    pub leftovers: Vec<String>,
    /// Files named like a draft that the manifest does not list, such as a draft whose record
    /// was lost: possibly a plaintext letter, so left for the user to check.
    pub unrecorded: Vec<String>,
}

/// The default drafts folder: `drafts` in the data directory, which is private to the user on
/// every platform, unlike a shared temporary folder such as Linux's `/tmp`.
pub fn default_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("drafts")
}

/// Write `text` to `<dir>/<name>`, creating `dir`. The draft is written to a temporary file of
/// this process's own and linked into place once complete, so a draft name never holds a
/// partial letter and an existing file is never replaced. On Unix, refuses a folder others
/// can open.
pub fn write(dir: &Path, name: &str, text: &str) -> io::Result<PathBuf> {
    if !is_draft(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("`{name}` is not a draft name"),
        ));
    }
    let _lock = lock_for_write(dir, lock_new)?;
    let path = dir.join(name);
    if fs::symlink_metadata(&path).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", path.display()),
        ));
    }
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut file = private_file(OpenOptions::new().write(true).create_new(true)).open(&tmp)?;
    // From here the temporary file is this call's own, so it may remove it.
    let published = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .and_then(|()| add_record(dir, &sha256(text.as_bytes()), name))
        .and_then(|()| fs::hard_link(&tmp, &path).map_err(|e| link_failure(e, &path)));
    drop(file);
    let _ = fs::remove_file(&tmp);
    published?;
    sync_dir(dir)?;
    Ok(path)
}

/// `error` from linking a finished draft to `path`, naming the likely cause: FAT, exFAT and
/// some network shares have no hard links, and the OS error alone does not say so.
fn link_failure(error: io::Error, path: &Path) -> io::Error {
    if error.kind() == io::ErrorKind::AlreadyExists {
        return error;
    }
    io::Error::new(
        error.kind(),
        format!(
            "could not create {}: {error}. Drafts are put in place by a hard link, which FAT, \
             exFAT and some network shares do not support; if the folder is on one, choose a \
             folder on an internal drive with --out",
            path.display()
        ),
    )
}

fn lock_new(dir: &Path) -> io::Result<Lock> {
    create_private_dir(dir)?;
    check_private(dir)?;
    Lock::acquire(dir, LOCK)
}

fn lock_for_write(dir: &Path, acquire: impl Fn(&Path) -> io::Result<Lock>) -> io::Result<Lock> {
    acquire(dir).or_else(|e| match e.kind() {
        // A `clean` removed the emptied folder between creating it and locking it.
        io::ErrorKind::NotFound => acquire(dir),
        _ => Err(e),
    })
}

/// One manifest line: `pending` or a SHA-256, then a draft name.
type Record = (String, String);

/// Set `name`'s record, replacing any earlier one, so each name has only its latest.
fn set_record(records: &mut Vec<Record>, first: &str, name: &str) {
    records.retain(|(_, n)| n != name);
    records.push((first.to_owned(), name.to_owned()));
}

fn add_record(dir: &Path, first: &str, name: &str) -> io::Result<()> {
    let mut records = read_manifest(dir)?;
    set_record(&mut records, first, name);
    save_manifest(dir, &records)
}

/// The manifest's records, the last for each name winning; none if it is missing. Refuses a
/// symbolic link or anything but a plain file at its name.
fn read_manifest(dir: &Path) -> io::Result<Vec<Record>> {
    let path = dir.join(MANIFEST);
    match fs::symlink_metadata(&path) {
        Ok(meta) if !meta.is_file() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is not a plain file", path.display()),
            ));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
        Ok(_) => {}
    }
    let mut records = Vec::new();
    for (first, name) in fs::read_to_string(&path)?
        .lines()
        .filter_map(|l| l.split_once(' '))
        .filter(|&(_, name)| is_draft(name))
    {
        set_record(&mut records, first, name);
    }
    Ok(records)
}

/// Replace the manifest with `records`, or remove it when none remain. The new copy is
/// written and synced under a new name of this update's own, then renamed over the old one;
/// a copy left by an update that stopped part-way stays and `clean` reports it.
fn save_manifest(dir: &Path, records: &[Record]) -> io::Result<()> {
    let path = dir.join(MANIFEST);
    if records.is_empty() {
        remove_if_present(&path)?;
        return sync_dir(dir);
    }
    // A name of this update's own: a file already at it, whoever left it, is never touched.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let tmp = dir.join(format!("{MANIFEST}.{}-{nanos}.tmp", std::process::id()));
    let mut file = private_file(OpenOptions::new().write(true).create_new(true)).open(&tmp)?;
    let written = records
        .iter()
        .try_for_each(|(first, name)| writeln!(file, "{first} {name}"))
        .and_then(|()| file.sync_all());
    drop(file);
    let replaced = written.and_then(|()| fs::rename(&tmp, &path));
    if replaced.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    replaced?;
    sync_dir(dir)
}

/// A temporary file GhostSoap leaves only if it stops part-way: a draft's or the manifest's.
fn is_leftover(name: &str) -> bool {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let manifest_copy = name
        .strip_prefix(&format!("{MANIFEST}."))
        .and_then(|rest| rest.strip_suffix(".tmp"))
        .and_then(|stamp| stamp.split_once('-'))
        .is_some_and(|(pid, nanos)| digits(pid) && digits(nanos));
    manifest_copy || (name.starts_with('.') && name.ends_with(".tmp") && name.contains("-GS-"))
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// On Unix, flush the folder so a new name survives a power cut.
fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    fs::File::open(dir)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

/// The SHA-256 of `bytes` in lowercase hex.
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Open options for a file only the user can read (mode 600 on Unix).
fn private_file(options: &mut OpenOptions) -> &mut OpenOptions {
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(options, 0o600);
    options
}

/// On Unix, `dir` must be a real folder (not a link) that only its owner can open. Windows
/// folders keep the access list they inherit; the default drafts folder sits in the user's
/// own `%LOCALAPPDATA%`.
#[cfg(unix)]
fn check_private(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::symlink_metadata(dir)?;
    if meta.is_dir() && meta.permissions().mode() & 0o077 == 0 {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "{} must be a folder only you can open (chmod 700), not a link",
            dir.display()
        ),
    ))
}

#[cfg(not(unix))]
fn check_private(_dir: &Path) -> io::Result<()> {
    Ok(())
}

/// A name GhostSoap gives a draft: `<entry-id>-GS-XXXXX.eml` or `.txt`, where the entry id
/// is lowercase ASCII letters, digits and hyphens and `XXXXX` is 5 digits or capitals.
fn is_draft(name: &str) -> bool {
    let Some(stem) = name
        .strip_suffix(".eml")
        .or_else(|| name.strip_suffix(".txt"))
    else {
        return false;
    };
    let Some((id, tag)) = stem.rsplit_once("-GS-") else {
        return false;
    };
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && tag.len() == 5
        && tag
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
}

/// Delete the drafts GhostSoap wrote in `dir`: each committed draft that still holds the
/// bytes GhostSoap wrote. Other files, including a listed name now holding something else
/// and a name whose write never committed, are left alone and reported, as are temporary
/// files from an interrupted write. With `remove_dir`, `dir` is also removed once empty;
/// pass it only for GhostSoap's own drafts folder.
// Bravo Six, going dark.
pub fn clean(dir: &Path, remove_dir: bool) -> io::Result<Cleaned> {
    clean_with_lock(dir, remove_dir, || {
        check_private(dir).and_then(|()| Lock::acquire(dir, LOCK))
    })
}

fn clean_with_lock(
    dir: &Path,
    remove_dir: bool,
    acquire: impl FnOnce() -> io::Result<Lock>,
) -> io::Result<Cleaned> {
    if !dir.exists() {
        return Ok(Cleaned::default());
    }
    let lock = match acquire() {
        // Another clean removed the folder since the existence check.
        Err(e) if e.kind() == io::ErrorKind::NotFound && !dir.exists() => {
            return Ok(Cleaned::default());
        }
        lock => lock?,
    };
    let mut cleaned = Cleaned::default();
    // Records for files left in place stay in the manifest, so the next clean reports them.
    let mut remaining = Vec::new();
    let records = read_manifest(dir)?;
    let listed: Vec<String> = records.iter().map(|(_, name)| name.clone()).collect();
    for (first, name) in records {
        let path = dir.join(&name);
        if first == PENDING {
            if path.exists() {
                cleaned.unconfirmed.push(name.clone());
                remaining.push((first, name));
            }
            continue;
        }
        match fs::read(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
            Ok(bytes) if sha256(&bytes) == first => {
                fs::remove_file(&path)?;
                cleaned.deleted += 1;
            }
            Ok(_) => {
                cleaned.kept.push(name.clone());
                remaining.push((first, name));
            }
        }
    }
    save_manifest(dir, &remaining)?;
    report_unlisted(dir, &listed, &mut cleaned)?;
    drop(lock);
    if remove_dir {
        remove_if_empty(dir, |dir| fs::remove_dir(dir))?;
    }
    Ok(cleaned)
}

/// Add to `cleaned` each file in `dir` that only an interrupted write leaves, and each file
/// named like a draft that `listed`, the manifest's names, does not hold.
fn report_unlisted(dir: &Path, listed: &[String], cleaned: &mut Cleaned) -> io::Result<()> {
    for item in fs::read_dir(dir)? {
        let name = item?.file_name().to_string_lossy().into_owned();
        if is_leftover(&name) {
            cleaned.leftovers.push(name);
        } else if is_draft(&name) && !listed.contains(&name) {
            cleaned.unrecorded.push(name);
        }
    }
    cleaned.unrecorded.sort();
    Ok(())
}

/// Check all earlier drafts before the first deletion in a redraft batch.
pub(crate) fn can_remove(dir: &Path, name: &str) -> io::Result<bool> {
    if !draft_present(&dir.join(name))? {
        return Ok(true);
    }
    check_private(dir)?;
    let _lock = Lock::acquire(dir, LOCK)?;
    let records = read_manifest(dir)?;
    Ok(draft_status(dir, name, &records)? != DraftStatus::Changed)
}

#[derive(PartialEq, Eq)]
enum DraftStatus {
    Missing,
    Original,
    Changed,
}

fn draft_present(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(false)
        }
        Err(e) => Err(e),
        Ok(_) => Ok(true),
    }
}

fn draft_status(dir: &Path, name: &str, records: &[Record]) -> io::Result<DraftStatus> {
    let path = dir.join(name);
    match fs::symlink_metadata(&path) {
        Ok(meta) if !meta.is_file() => return Ok(DraftStatus::Changed),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(DraftStatus::Missing),
        Err(e) => return Err(e),
        Ok(_) => {}
    }
    let Some((digest, _)) = records
        .iter()
        .find(|(digest, file)| file == name && digest != PENDING)
    else {
        return Ok(DraftStatus::Changed);
    };
    match fs::read(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(DraftStatus::Missing),
        Err(e) => Err(e),
        Ok(bytes) if sha256(&bytes) == *digest => Ok(DraftStatus::Original),
        Ok(_) => Ok(DraftStatus::Changed),
    }
}

/// An earlier draft held under a private temporary name until handover succeeds.
#[derive(Debug)]
struct StagedRemoval {
    original: PathBuf,
    backup: Option<PathBuf>,
    committed: bool,
}

impl StagedRemoval {
    fn commit(&mut self) {
        self.committed = true;
    }

    fn discard(mut self) -> io::Result<()> {
        debug_assert!(self.committed);
        let backup = self.backup.take().expect("staged backup");
        remove_if_present(&backup).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!(
                    "could not remove the superseded draft at {}: {e}",
                    backup.display()
                ),
            )
        })
    }
}

impl Drop for StagedRemoval {
    fn drop(&mut self) {
        let Some(backup) = &self.backup else { return };
        if self.committed {
            eprintln!(
                "warning: earlier draft kept at {}; check this copy before removing it",
                backup.display()
            );
            return;
        }
        // Restore without overwriting a file put at the original name meanwhile.
        let restored = fs::hard_link(backup, &self.original).and_then(|()| fs::remove_file(backup));
        if let Err(e) = restored {
            eprintln!(
                "warning: could not finish restoring {}; the earlier draft is at {}: {e}",
                self.original.display(),
                backup.display()
            );
        }
    }
}

/// Holds the folder locks until the staged removals have committed or rolled back. In
/// particular, `clean` cannot discard a temporarily missing draft's manifest record.
#[derive(Default)]
pub(crate) struct Redraft {
    removals: Vec<StagedRemoval>,
    locks: BTreeMap<PathBuf, Lock>,
}

impl Redraft {
    pub(crate) fn stage(&mut self, dir: &Path, name: &str) -> io::Result<()> {
        if !draft_present(&dir.join(name))? {
            return Ok(());
        }
        if !self.locks.contains_key(dir) {
            check_private(dir)?;
            self.locks.insert(dir.to_owned(), Lock::acquire(dir, LOCK)?);
        }
        self.removals.extend(stage_remove(dir, name)?);
        Ok(())
    }

    pub(crate) fn commit(&mut self) {
        for removal in &mut self.removals {
            removal.commit();
        }
    }

    pub(crate) fn unlock(&mut self) {
        self.locks.clear();
    }

    pub(crate) fn discard(self) {
        for removal in self.removals {
            if let Err(error) = removal.discard() {
                eprintln!("warning: {error}");
            }
        }
    }
}

/// Take a verified draft out of circulation before a redraft. Keep its manifest record
/// until either the replacement write takes over the name or `clean` drops the missing entry.
/// The caller holds the drafts lock.
fn stage_remove(dir: &Path, name: &str) -> io::Result<Option<StagedRemoval>> {
    stage_remove_with(dir, name, |path| fs::remove_file(path))
}

fn stage_remove_with(
    dir: &Path,
    name: &str,
    remove: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<Option<StagedRemoval>> {
    let path = dir.join(name);
    let records = read_manifest(dir)?;
    match draft_status(dir, name, &records)? {
        DraftStatus::Missing => return Ok(None),
        DraftStatus::Changed => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the earlier draft changed during redraft",
            ));
        }
        DraftStatus::Original => {}
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let backup = dir.join(format!(".{name}.{}-{nanos}.tmp", std::process::id()));
    fs::hard_link(&path, &backup)?;
    if let Err(error) = remove(&path) {
        let _ = fs::remove_file(&backup);
        return Err(error);
    }
    let removal = StagedRemoval {
        original: path,
        backup: Some(backup),
        committed: false,
    };
    sync_dir(dir)?;
    Ok(Some(removal))
}

fn remove_if_empty(dir: &Path, remove: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<()> {
    let empty = match fs::read_dir(dir) {
        Ok(mut items) => items.next().is_none(),
        // Another clean removed it after this one released the lock.
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if !empty {
        return Ok(());
    }
    match remove(dir) {
        // A `write` started in the folder after the lock was released; it stays.
        Err(e)
            if !matches!(
                e.kind(),
                io::ErrorKind::DirectoryNotEmpty | io::ErrorKind::NotFound
            ) =>
        {
            Err(e)
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
