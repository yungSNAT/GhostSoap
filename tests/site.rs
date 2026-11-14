// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

use std::path::Path;

use ghostsoap::io;

// The site states the register's size, so adding or removing an entry must update it.
#[test]
fn the_site_states_the_register_size() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = io::register::load(&root.join("registry/au")).expect("read register");
    let html = std::fs::read_to_string(root.join("site/index.html")).expect("read site");
    let stated = format!("<strong id=\"register-entries\">{}</strong>", files.entries.len());
    assert!(html.contains(&stated), "site/index.html should contain {stated}");
}
