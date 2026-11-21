// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! GhostSoap: prepare Australian Privacy Act requests to data brokers and track the replies.

pub mod cli;
pub mod core;
pub mod io;

pub use cli::Cli;
