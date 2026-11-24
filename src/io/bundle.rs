// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! SHA-256 export manifests, OAIC escalation bundles, rollback and integrity verification.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use jiff::civil::Date;
use serde::Serialize;

use crate::core::audit::{self, AuditEntry, AuditSummary, MANIFEST_HEADER};
use crate::core::letters;
use crate::core::register::Entry;
use crate::core::sha256;
use crate::core::tracker::{Message, State};

mod verify;
pub use verify::{ExportCheck, ManifestCheck, verify_export_file, verify_manifest_file};

const BUNDLE_FILES: [&str; 3] = ["record.toml", "summary.txt", "manifest.sha256"];
const AUDIT_EXCERPT_NOTE: &str = "Tag-filtered excerpt from the global audit chain; integrity \
    is verified via manifest.sha256 rather than as a standalone chain.";

#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "x86")))]
const UNIX_NOFOLLOW_NONBLOCK: i32 = 0x20000 | 0x800; // O_NOFOLLOW | O_NONBLOCK
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const UNIX_NOFOLLOW_NONBLOCK: i32 = 0x8000 | 0x800; // O_NOFOLLOW | O_NONBLOCK
#[cfg(target_os = "macos")]
const UNIX_NOFOLLOW_NONBLOCK: i32 = 0x0100 | 0x0004; // O_NOFOLLOW | O_NONBLOCK

/// Companion manifest path for an export file (`<path>.sha256`).
pub fn manifest_path(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(".sha256");
    PathBuf::from(name)
}

/// Validate that `path`'s basename is valid UTF-8 and safe to record in a SHA-256 manifest.
pub fn validate_export_basename(path: &Path) -> io::Result<String> {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "export filename must be valid UTF-8; nothing was exported",
        ));
    };
    if !audit::is_safe_filename(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "export filename `{name}` cannot be safely recorded in a SHA-256 manifest; \
                 nothing was exported"
            ),
        ));
    }
    Ok(name.to_owned())
}

#[cfg(all(
    unix,
    any(
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "x86", target_arch = "aarch64")
        ),
        target_os = "macos"
    )
))]
fn open_regular_file(path: &Path) -> io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.read(true);
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, UNIX_NOFOLLOW_NONBLOCK);
    let file = options.open(path).map_err(|e| {
        if e.raw_os_error()
            .is_some_and(|code| code == 40 || code == 62)
        {
            io::Error::other("not a regular file (symlink refused)")
        } else {
            e
        }
    })?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    Ok(file)
}

#[cfg(all(
    unix,
    not(any(
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "x86", target_arch = "aarch64")
        ),
        target_os = "macos"
    ))
))]
fn open_regular_file(_path: &Path) -> io::Result<fs::File> {
    Err(io::Error::other(
        "unsupported Unix target for safe O_NOFOLLOW file open",
    ))
}

#[cfg(windows)]
fn open_regular_file(path: &Path) -> io::Result<fs::File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|e| {
            if fs::symlink_metadata(path).is_ok_and(|meta| !meta.is_file()) {
                io::Error::other("not a regular file")
            } else {
                e
            }
        })?;
    let meta = file.metadata()?;
    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !meta.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    Ok(file)
}

/// Open and read a regular file fail-closed without following symlinks or blocking on FIFOs.
pub(crate) fn read_regular_file(path: &Path) -> io::Result<Vec<u8>> {
    let mut file = open_regular_file(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Create a new file (never replacing an existing file or link) and write to it via `write`.
/// On error, returns `(error, created)` where `created` is true iff `create_new` created `path`.
fn create_new_file_with(
    path: &Path,
    private: bool,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> Result<(), (io::Error, bool)> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options.open(path).map_err(|e| {
        let mapped = match e.kind() {
            io::ErrorKind::AlreadyExists => io::Error::new(
                e.kind(),
                format!("{} already exists; nothing was written", path.display()),
            ),
            _ => e,
        };
        (mapped, false)
    })?;
    let written = write(&mut file);
    drop(file);
    written.map_err(|e| (e, true))
}

/// Write bytes to a new file (never replacing an existing file or link), setting mode 600
/// on Unix when `private` is true.
pub fn write_new_file(path: &Path, bytes: &[u8], private: bool) -> io::Result<()> {
    write_new_file_with(path, private, |file| {
        file.write_all(bytes).and_then(|()| file.sync_all())
    })
}

fn write_new_file_with(
    path: &Path,
    private: bool,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> io::Result<()> {
    create_new_file_with(path, private, write).map_err(|(err, created)| {
        if created {
            let _ = fs::remove_file(path);
        }
        err
    })
}

/// Outcome of writing an OAIC escalation bundle directory.
#[derive(Debug, Clone)]
pub struct BundleReceipt {
    pub dir: PathBuf,
    pub manifest: PathBuf,
    pub files: Vec<String>,
    pub audit: AuditSummary,
}

#[derive(Serialize)]
struct EscalationRecord<'a> {
    tag: &'a str,
    exported_on: Date,
    global_audit_count: usize,
    global_audit_head: &'a str,
    audit_excerpt_note: &'static str,
    message: &'a Message,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    audit_excerpt: Vec<&'a AuditEntry>,
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(dir)
    }
    #[cfg(not(unix))]
    {
        fs::DirBuilder::new().create(dir)
    }
}

fn rollback_bundle(dir: &Path, created: &[PathBuf], error: io::Error) -> io::Error {
    let mut leftovers = Vec::new();
    for path in created.iter().rev() {
        if let Err(e) = fs::remove_file(path) {
            leftovers.push(format!("{} ({e})", path.display()));
        }
    }
    if let Err(e) = fs::remove_dir(dir) {
        leftovers.push(format!("{} ({e})", dir.display()));
    }
    if leftovers.is_empty() {
        io::Error::new(
            error.kind(),
            format!(
                "{error}; partial escalation bundle at {} was deleted",
                dir.display()
            ),
        )
    } else {
        io::Error::new(
            error.kind(),
            format!(
                "{error}; could not clean up partial escalation bundle paths: {}",
                leftovers.join(", ")
            ),
        )
    }
}

/// Write a self-contained OAIC escalation bundle for `tag` in a new directory `dir`,
/// including `summary.txt`, `record.toml` and `manifest.sha256`.
pub fn write_escalation_bundle(
    dir: &Path,
    state: &State,
    tag: &str,
    entry: Option<&Entry>,
    today: Date,
) -> io::Result<BundleReceipt> {
    write_escalation_bundle_with(dir, state, tag, entry, today, |path, bytes| {
        create_new_file_with(path, true, |file| {
            file.write_all(bytes).and_then(|()| file.sync_all())
        })
    })
}

fn prepare_bundle_texts(
    state: &State,
    tag: &str,
    entry: Option<&Entry>,
    today: Date,
) -> io::Result<(AuditSummary, String, String, String)> {
    let message = state.message(tag).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("no letter is recorded under {tag}; the tag is in the letter's subject line"),
        )
    })?;
    let audit_summary = audit::verify_chain(&state.audit).map_err(io::Error::other)?;
    let summary_text =
        audit::format_escalation_summary(message, entry, &state.audit, &audit_summary, today);
    let record = EscalationRecord {
        tag: &message.tag,
        exported_on: today,
        global_audit_count: audit_summary.count,
        global_audit_head: &audit_summary.head_hash,
        audit_excerpt_note: AUDIT_EXCERPT_NOTE,
        message,
        audit_excerpt: state
            .audit
            .iter()
            .filter(|item| item.tag == message.tag)
            .collect(),
    };
    let record_text = toml::to_string(&record).map_err(io::Error::other)?;
    let record_sha = sha256(record_text.as_bytes());
    let summary_sha = sha256(summary_text.as_bytes());
    let date = today.to_string();
    let manifest_text = audit::format_manifest(
        &date,
        &audit_summary,
        &[("record.toml", &record_sha), ("summary.txt", &summary_sha)],
    );
    Ok((audit_summary, record_text, summary_text, manifest_text))
}

fn write_escalation_bundle_with(
    dir: &Path,
    state: &State,
    tag: &str,
    entry: Option<&Entry>,
    today: Date,
    mut write_file: impl FnMut(&Path, &[u8]) -> Result<(), (io::Error, bool)>,
) -> io::Result<BundleReceipt> {
    let (audit_summary, record_text, summary_text, manifest_text) =
        prepare_bundle_texts(state, tag, entry, today)?;
    create_private_dir(dir).map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => io::Error::new(
            e.kind(),
            format!("{} already exists; nothing was exported", dir.display()),
        ),
        _ => e,
    })?;
    let items = [
        (dir.join("record.toml"), record_text.as_bytes()),
        (dir.join("summary.txt"), summary_text.as_bytes()),
        (dir.join("manifest.sha256"), manifest_text.as_bytes()),
    ];
    let mut created = Vec::new();
    for (path, bytes) in items {
        match write_file(&path, bytes) {
            Ok(()) => created.push(path),
            Err((err, was_created)) => {
                if was_created {
                    created.push(path);
                }
                return Err(rollback_bundle(dir, &created, err));
            }
        }
    }
    Ok(BundleReceipt {
        dir: dir.to_owned(),
        manifest: dir.join("manifest.sha256"),
        files: vec!["record.toml".to_owned(), "summary.txt".to_owned()],
        audit: audit_summary,
    })
}

/// Split `ghostsoap-escalation-<TAG>-YYYY-MM-DD` on its fixed 11-byte `-YYYY-MM-DD` suffix.
fn split_escalation_dir_name(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix("ghostsoap-escalation-")?;
    if rest.len() <= 11 {
        return None;
    }
    let split_at = rest.len() - 11;
    if !rest.is_char_boundary(split_at) || rest.as_bytes()[split_at] != b'-' {
        return None;
    }
    let tag = &rest[..split_at];
    let date = &rest[split_at + 1..];
    (letters::is_tag(tag) && Date::strptime("%Y-%m-%d", date).is_ok()).then_some((tag, date))
}

/// Whether `path` is a default-named GhostSoap escalation bundle folder inside the data dir.
pub fn is_escalation_bundle_dir(path: &Path) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    if split_escalation_dir_name(&name).is_none() {
        return false;
    }
    if !fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir()) {
        return false;
    }
    let manifest = path.join("manifest.sha256");
    read_regular_file(&manifest).is_ok_and(|bytes| bytes.starts_with(MANIFEST_HEADER.as_bytes()))
}

/// Default-named GhostSoap escalation bundle folders in `dir`.
pub fn own_escalation_bundles(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let items = match fs::read_dir(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        items => items?,
    };
    let mut bundles = Vec::new();
    for item in items {
        let path = item?.path();
        if is_escalation_bundle_dir(&path) {
            bundles.push(path);
        }
    }
    bundles.sort();
    Ok(bundles)
}

/// Outcome of cleaning one GhostSoap escalation bundle directory during `forget`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanedBundle {
    pub deleted: usize,
    pub kept: Vec<String>,
}

/// Delete only GhostSoap's 3 known bundle files from `dir` (never following symlinks) and
/// remove `dir` only when empty. Any unrelated files are left untouched and reported.
pub fn clean_escalation_bundle(dir: &Path) -> io::Result<CleanedBundle> {
    if !fs::symlink_metadata(dir)?.is_dir() {
        return Err(io::Error::other(
            "it is no longer a folder, so GhostSoap leaves it alone",
        ));
    }
    let mut deleted = 0usize;
    for name in BUNDLE_FILES {
        let file = dir.join(name);
        if let Ok(meta) = fs::symlink_metadata(&file)
            && meta.is_file()
        {
            fs::remove_file(&file)?;
            deleted += 1;
        }
    }
    let mut kept = Vec::new();
    for entry in fs::read_dir(dir)? {
        kept.push(entry?.file_name().to_string_lossy().into_owned());
    }
    kept.sort();
    if kept.is_empty() {
        fs::remove_dir(dir)?;
    }
    Ok(CleanedBundle { deleted, kept })
}

#[cfg(test)]
mod tests;
