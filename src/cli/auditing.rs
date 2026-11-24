// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! `audit`: verify and inspect the tamper-evident SHA-256 OAIC audit chain and manifests.

use super::*;
use crate::core::audit::{self, AuditEntry, AuditSummary};
use crate::io::bundle;

pub(super) fn audit_command(args: &AuditArgs) -> Result<(), String> {
    if let Some(manifest) = &args.manifest {
        let check = bundle::verify_manifest_file(manifest)?;
        let export_check = args
            .export
            .as_deref()
            .map(bundle::verify_export_file)
            .transpose()?;
        return print_manifest_result(&check, export_check.as_ref(), args.list, args.json);
    }
    if let Some(export_path) = &args.export {
        let check = bundle::verify_export_file(export_path)?;
        return print_chain_result(
            &check.summary,
            &check.entries,
            check.manifest.as_ref(),
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
    print_chain_result(&summary, &state.audit, None, args.list, args.json)
}

fn print_manifest_result(
    manifest: &bundle::ManifestCheck,
    export: Option<&bundle::ExportCheck>,
    list: bool,
    as_json: bool,
) -> Result<(), String> {
    if let Some(export) = export {
        return print_chain_result(
            &export.summary,
            &export.entries,
            Some(manifest),
            list,
            as_json,
        );
    }
    if as_json {
        let out = json!({
            "verified": true,
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

fn print_chain_result(
    summary: &AuditSummary,
    entries: &[AuditEntry],
    manifest: Option<&bundle::ManifestCheck>,
    list: bool,
    as_json: bool,
) -> Result<(), String> {
    if as_json {
        let mut out = json!({
            "verified": true,
            "count": summary.count,
            "escalations": summary.escalations,
            "head_hash": summary.head_hash,
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
        return Ok(());
    }
    println!("Audit chain OK: {summary}.");
    if let Some(manifest) = manifest {
        println!(
            "Manifest OK: {} ({} file(s) verified).",
            manifest.manifest.display(),
            manifest.files.len()
        );
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
