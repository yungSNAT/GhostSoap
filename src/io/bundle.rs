// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! SHA-256 export manifests, OAIC escalation bundles and integrity verification.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::core::audit::{self, AuditEntry, AuditSummary};
use crate::core::register::Entry;
use crate::core::sha256;
use crate::core::tracker::{Message, State};

/// Companion manifest path for an export file (`<path>.sha256`).
pub fn manifest_path(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(".sha256");
    PathBuf::from(name)
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
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options.open(path).map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => io::Error::new(
            e.kind(),
            format!("{} already exists; nothing was written", path.display()),
        ),
        _ => e,
    })?;
    let written = write(&mut file);
    drop(file);
    written.inspect_err(|_| {
        let _ = fs::remove_file(path);
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
    message: &'a Message,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    audit: Vec<&'a AuditEntry>,
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
        message,
        audit: state
            .audit
            .iter()
            .filter(|item| item.tag == message.tag)
            .collect(),
    };
    let record_text = toml::to_string(&record).map_err(io::Error::other)?;
    fs::create_dir(dir).map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => io::Error::new(
            e.kind(),
            format!("{} already exists; nothing was exported", dir.display()),
        ),
        _ => e,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    let record_sha = sha256(record_text.as_bytes());
    let summary_sha = sha256(summary_text.as_bytes());
    let date = today.to_string();
    let manifest_text = audit::format_manifest(
        &date,
        &audit_summary,
        &[("record.toml", &record_sha), ("summary.txt", &summary_sha)],
    );
    let manifest = dir.join("manifest.sha256");
    write_new_file(&dir.join("record.toml"), record_text.as_bytes(), true)?;
    write_new_file(&dir.join("summary.txt"), summary_text.as_bytes(), true)?;
    write_new_file(&manifest, manifest_text.as_bytes(), true)?;
    Ok(BundleReceipt {
        dir: dir.to_owned(),
        manifest,
        files: vec!["record.toml".to_owned(), "summary.txt".to_owned()],
        audit: audit_summary,
    })
}

/// Verified files in a GhostSoap SHA-256 manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestCheck {
    pub manifest: PathBuf,
    pub files: Vec<String>,
}

/// Verify every file listed in a GhostSoap SHA-256 manifest against its SHA-256 digest.
pub fn verify_manifest_file(manifest: &Path) -> Result<ManifestCheck, String> {
    let text = fs::read_to_string(manifest)
        .map_err(|e| format!("cannot read manifest {}: {e}", manifest.display()))?;
    let entries = audit::parse_manifest(&text)?;
    let folder = manifest.parent().unwrap_or_else(|| Path::new("."));
    let mut verified = Vec::with_capacity(entries.len());
    for item in entries {
        let target = folder.join(&item.name);
        let meta = fs::symlink_metadata(&target)
            .map_err(|e| format!("manifest file {} is unreadable: {e}", item.name))?;
        if !meta.is_file() {
            return Err(format!("manifest file {} is not a regular file", item.name));
        }
        let bytes =
            fs::read(&target).map_err(|e| format!("cannot read {}: {e}", target.display()))?;
        let actual = sha256(&bytes);
        if actual != item.sha256 {
            return Err(format!(
                "SHA-256 mismatch for {}: expected {}, got {actual}",
                item.name, item.sha256
            ));
        }
        verified.push(item.name);
    }
    Ok(ManifestCheck {
        manifest: manifest.to_owned(),
        files: verified,
    })
}

#[derive(Deserialize)]
struct ExportAudit {
    #[serde(default)]
    audit: Vec<AuditEntry>,
}

/// Outcome of verifying a plaintext export file and any companion manifest.
#[derive(Debug, Clone)]
pub struct ExportCheck {
    pub summary: AuditSummary,
    pub entries: Vec<AuditEntry>,
    pub manifest: Option<ManifestCheck>,
}

/// Verify the audit hash chain in a plaintext export file, plus its companion `.sha256`
/// manifest when present.
pub fn verify_export_file(export_path: &Path) -> Result<ExportCheck, String> {
    let text = fs::read_to_string(export_path)
        .map_err(|e| format!("cannot read export {}: {e}", export_path.display()))?;
    let parsed: ExportAudit = toml::from_str(&text)
        .map_err(|e| format!("export {} is not valid TOML: {e}", export_path.display()))?;
    let summary = audit::verify_chain(&parsed.audit)?;
    let companion = manifest_path(export_path);
    let manifest = if companion.exists() {
        Some(verify_manifest_file(&companion)?)
    } else {
        None
    };
    Ok(ExportCheck {
        summary,
        entries: parsed.audit,
        manifest,
    })
}

#[cfg(test)]
mod tests;
