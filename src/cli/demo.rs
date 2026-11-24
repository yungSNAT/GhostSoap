// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! `demo`: zero-PII synthetic dry-run status, checklist, audit chain and HTML preview.

use super::*;
use crate::core::demo as core_demo;
use crate::io::bundle;

pub(super) fn run_demo(args: &DemoArgs) -> Result<(), String> {
    let snapshot = core_demo::build();
    if let Some(path) = &args.html {
        let html = core_demo::render_html(&snapshot);
        bundle::write_new_file(path, html.as_bytes(), false).map_err(|e| e.to_string())?;
        println!(
            "Wrote synthetic HTML demo preview (example.com fixtures only) to {}.",
            path.display()
        );
        return Ok(());
    }
    match args.format {
        DemoFormat::Text => write_stdout(core_demo::render_text(&snapshot).as_bytes()),
        DemoFormat::Html => write_stdout(core_demo::render_html(&snapshot).as_bytes()),
    }
    Ok(())
}
