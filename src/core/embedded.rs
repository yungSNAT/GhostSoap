// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! The shipped register and checklist, embedded at compile time by `build.rs`, so an
//! installed binary (or a later WebAssembly build) needs no data files on disk.

use crate::core::register::{RegisterFiles, SourceFile};

mod data {
    include!(concat!(env!("OUT_DIR"), "/register.rs"));
}

fn files(table: &[(&str, &str)]) -> Vec<SourceFile> {
    table
        .iter()
        .map(|(name, text)| SourceFile {
            name: (*name).to_owned(),
            text: (*text).to_owned(),
        })
        .collect()
}

/// The register built into this binary.
pub fn register() -> RegisterFiles {
    RegisterFiles {
        entries: files(data::ENTRIES),
        packs: files(data::PACKS),
    }
}

/// The checklist items built into this binary.
pub fn checklist() -> Vec<SourceFile> {
    files(data::CHECKLIST)
}
