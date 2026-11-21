// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Pure logic over values: no files, network or terminal.

pub mod check;
pub mod checklist;
pub mod deadline;
pub mod embedded;
pub mod law;
pub mod letters;
pub mod plan;
pub mod profile;
pub mod register;
pub mod tracker;

/// The name the state file gives `value`, such as `refused-with-reasons`.
pub(crate) fn named(value: impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .expect("a unit variant serialises as its name")
}
