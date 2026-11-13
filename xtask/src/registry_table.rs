// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! The entries table in `registry/README.md`, generated from the register built into
//! `ghostsoap`. A test fails when the README's table is out of date, so CI catches an entry
//! added without it; `UPDATE_REGISTRY_TABLE=1 cargo test -p xtask` rewrites the table.

use ghostsoap::core::register::Register;

const START: &str = "<!-- entries:start -->";
const END: &str = "<!-- entries:end -->";

/// One row per entry, in id order, with the packs that name it and how a user reaches it.
fn table(register: &Register) -> String {
    let mut out = String::from("| Entry | Name | Kinds | Packs | Route |\n|---|---|---|---|---|\n");
    for entry in register.entries.values() {
        let kinds: Vec<String> = entry
            .kinds
            .iter()
            .map(|kind| {
                serde_json::to_value(kind)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .expect("a kind serialises as its name")
            })
            .collect();
        let packs: Vec<String> = register
            .packs
            .values()
            .filter(|pack| pack.entries.contains(&entry.id))
            .map(|pack| format!("`{}`", pack.id))
            .collect();
        let route = match &entry.obsolete {
            Some(obsolete) => obsolete.skip_reason(),
            None if entry.is_overseas() => "its own web form".to_owned(),
            None => "letter".to_owned(),
        };
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            entry.id,
            cell(&entry.name),
            kinds.join(", "),
            packs.join(", "),
            route
        ));
    }
    out
}

/// Text that cannot end a table cell early.
fn cell(text: &str) -> String {
    text.replace('|', "\\|")
}

/// `readme` with the text between the markers replaced by `table`.
fn with_table(readme: &str, table: &str) -> Option<String> {
    let (before, rest) = readme.split_once(START)?;
    let (_, after) = rest.split_once(END)?;
    Some(format!("{before}{START}\n{table}{END}{after}"))
}

mod tests {
    use super::*;
    use ghostsoap::core::embedded;
    use ghostsoap::core::register::parse_register;
    use std::path::Path;

    #[test]
    fn the_register_readme_lists_every_entry() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../registry/README.md");
        let readme = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        let register = parse_register(&embedded::register()).unwrap();
        let current = with_table(&readme, &table(&register))
            .expect("registry/README.md has the entries table markers");
        if std::env::var_os("UPDATE_REGISTRY_TABLE").is_some() {
            std::fs::write(&path, &current).unwrap();
            return;
        }
        assert!(
            readme == current,
            "the entries table in registry/README.md is out of date; \
             run UPDATE_REGISTRY_TABLE=1 cargo test -p xtask"
        );
    }

    #[test]
    fn a_name_cannot_break_the_table() {
        assert_eq!(cell("Smith | Jones"), "Smith \\| Jones");
        assert_eq!(with_table("a b", "x"), None);
        assert_eq!(
            with_table(&format!("a\n{START}\nold\n{END}\nb"), "new\n").as_deref(),
            Some(format!("a\n{START}\nnew\n{END}\nb").as_str())
        );
    }
}
