// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! `audit`: verify and inspect the tamper-evident SHA-256 OAIC audit chain and manifests.

use super::*;
use crate::core::audit::{self, AuditEntry, AuditSummary};
use crate::io::bundle;

pub(super) fn audit_command(args: &AuditArgs) -> Result<(), String> {
    if args.export.is_some() && args.manifest.is_some() {
        return Err(
            "--export and --manifest cannot be used together; --export checks its own \
             companion .sha256 manifest"
                .to_owned(),
        );
    }
    if let Some(manifest) = &args.manifest {
        let check = bundle::verify_manifest_file(manifest)?;
        return print_manifest_result(&check, args.json);
    }
    if let Some(export_path) = &args.export {
        let check = bundle::verify_export_file(export_path)?;
        return print_chain_result(
            &check.summary,
            &check.entries,
            check.manifest.as_ref(),
            true,
            args.list,
            args.json,
        );
    }
    let dir = state::data_dir().map_err(|e| e.to_string())?;
    let passphrase = Input::new(args.passphrase_stdin)
        .passphrase("Passphrase: ")
        .map_err(|e| e.to_string())?;
    let state = state::load(&dir, passphrase).map_err(|e| e.to_string())?;
    let summary = audit::verify_chain(&state.audit)?;
    print_chain_result(&summary, &state.audit, None, false, args.list, args.json)
}

fn print_manifest_result(manifest: &bundle::ManifestCheck, as_json: bool) -> Result<(), String> {
    if as_json {
        let out = json!({
            "manifest_verified": true,
            "manifest": manifest.manifest.display().to_string(),
            "files": manifest.files,
        });
        println!("{}", serde_json::to_string_pretty(&out).expect("json"));
    } else {
        println!(
            "Manifest OK: {} ({} file(s) verified).",
            manifest.manifest.display(),
            manifest.files.len()
        );
        for file in &manifest.files {
            println!("  ok  {file}");
        }
    }
    Ok(())
}

fn print_chain_json(
    summary: &AuditSummary,
    entries: &[AuditEntry],
    manifest: Option<&bundle::ManifestCheck>,
    list: bool,
) {
    let mut out = json!({
        "chain_verified": true,
        "audit_entries_present": summary.count > 0,
        "count": summary.count,
        "escalations": summary.escalations,
        "head_hash": summary.head_hash,
        "manifest_present": manifest.is_some(),
        "manifest_verified": manifest.is_some(),
    });
    if let Some(manifest) = manifest {
        out["manifest"] = json!({
            "path": manifest.manifest.display().to_string(),
            "files": manifest.files,
        });
    }
    if list {
        out["entries"] = json!(entries);
    }
    println!("{}", serde_json::to_string_pretty(&out).expect("json"));
}

fn print_chain_result(
    summary: &AuditSummary,
    entries: &[AuditEntry],
    manifest: Option<&bundle::ManifestCheck>,
    is_export: bool,
    list: bool,
    as_json: bool,
) -> Result<(), String> {
    if as_json {
        print_chain_json(summary, entries, manifest, list);
        return Ok(());
    }
    if summary.count > 0 {
        println!("Audit chain OK: {summary}.");
    } else {
        println!(
            "Audit chain: empty (0 recorded events; head {}).",
            summary.head_hash
        );
    }
    if let Some(manifest) = manifest {
        println!(
            "Companion manifest OK: {} ({} file(s) verified).",
            manifest.manifest.display(),
            manifest.files.len()
        );
    } else if is_export {
        println!("Companion manifest: absent (export file digest not verified).");
    }
    if list {
        for item in entries {
            println!(
                "  #{}  {}  {:<14}{}  {}  {}  {}",
                item.seq,
                item.recorded_on,
                named(item.kind),
                item.tag,
                item.entry,
                item.detail,
                &item.hash[..12]
            );
        }
    }
    Ok(())
}
