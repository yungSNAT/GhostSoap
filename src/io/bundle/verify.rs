// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! TOCTOU-safe verification of GhostSoap SHA-256 manifests and plaintext export files.

use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::core::audit::{self, AuditEntry, AuditSummary};
use crate::core::checklist::Mark;
use crate::core::profile::Profile;
use crate::core::sha256;
use crate::core::tracker::{FORMAT, Message};
use crate::io::bundle::{manifest_path, read_regular_file};

/// Verified files in a GhostSoap SHA-256 manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestCheck {
    pub manifest: PathBuf,
    pub files: Vec<String>,
}

fn verify_manifest_entries(
    manifest: &Path,
    preloaded: Option<(&str, &[u8])>,
) -> Result<ManifestCheck, String> {
    let raw = read_regular_file(manifest)
        .map_err(|e| format!("cannot read manifest {}: {e}", manifest.display()))?;
    let text = String::from_utf8(raw)
        .map_err(|_| format!("manifest {} is not valid UTF-8", manifest.display()))?;
    let entries = audit::parse_manifest(&text)?;
    let folder = manifest.parent().unwrap_or_else(|| Path::new("."));
    let mut verified = Vec::with_capacity(entries.len());
    for item in entries {
        let actual = if let Some((name, bytes)) = preloaded
            && item.name == name
        {
            sha256(bytes)
        } else {
            let target = folder.join(&item.name);
            let bytes = read_regular_file(&target)
                .map_err(|e| format!("manifest file {} is unreadable: {e}", item.name))?;
            sha256(&bytes)
        };
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

/// Verify every file listed in a GhostSoap SHA-256 manifest against its SHA-256 digest.
pub fn verify_manifest_file(manifest: &Path) -> Result<ManifestCheck, String> {
    verify_manifest_entries(manifest, None)
}

/// Schema mirror of GhostSoap `State` exports (`deny_unknown_fields`) so arbitrary TOML is
/// rejected whilst a tampered `audit` chain still decodes for exact chain diagnosis.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportDocument {
    format: u32,
    profile: Profile,
    #[serde(default)]
    messages: Vec<Message>,
    #[serde(default)]
    checklist: Vec<Mark>,
    #[serde(default)]
    audit: Vec<AuditEntry>,
}

/// Redact TOML parse errors so raw PII lines from an export never appear in stderr/logs.
fn redact_toml_error(path: &Path, text: &str, err: &toml::de::Error) -> String {
    let place = err.span().map_or_else(String::new, |span| {
        let prefix = &text[..span.start.min(text.len())];
        let line = prefix.matches('\n').count() + 1;
        let col = prefix
            .rsplit_once('\n')
            .map_or(prefix.len(), |(_, tail)| tail.len())
            + 1;
        format!(" at line {line}, column {col}")
    });
    format!(
        "export {} is not a valid GhostSoap state export{place}",
        path.display()
    )
}

/// Outcome of verifying a plaintext export file and any companion manifest.
#[derive(Debug, Clone)]
pub struct ExportCheck {
    pub summary: AuditSummary,
    pub entries: Vec<AuditEntry>,
    pub manifest: Option<ManifestCheck>,
}

/// Verify a plaintext GhostSoap export from a single TOCTOU-safe read (no symlink
/// following), checking its schema, audit hash chain and companion `.sha256` manifest.
pub fn verify_export_file(export_path: &Path) -> Result<ExportCheck, String> {
    let raw = read_regular_file(export_path)
        .map_err(|e| format!("cannot read export {}: {e}", export_path.display()))?;
    let text = String::from_utf8(raw)
        .map_err(|_| format!("export {} is not valid UTF-8", export_path.display()))?;
    let doc: ExportDocument =
        toml::from_str(&text).map_err(|e| redact_toml_error(export_path, &text, &e))?;
    let _ = (&doc.profile, &doc.messages, &doc.checklist);
    if doc.format != FORMAT {
        return Err(format!(
            "export {} has unsupported state format {}, expected {FORMAT}",
            export_path.display(),
            doc.format
        ));
    }
    let summary = audit::verify_chain(&doc.audit)?;
    let expected = export_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let companion = manifest_path(export_path);
    let manifest = match std::fs::symlink_metadata(&companion) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        _ => {
            let checked = verify_manifest_entries(&companion, Some((&expected, text.as_bytes())))?;
            if !checked.files.iter().any(|name| name == &expected) {
                return Err(format!(
                    "companion manifest {} does not cover {expected}",
                    companion.display()
                ));
            }
            Some(checked)
        }
    };
    Ok(ExportCheck {
        summary,
        entries: doc.audit,
        manifest,
    })
}
