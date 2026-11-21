// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! `registry list` and `plan`: the register's entries and what a selection would send.

use super::*;

pub(super) fn list_register(json: bool) -> Result<(), String> {
    let register = built_in_register()?;
    if json {
        let packs: Vec<Value> = register
            .packs
            .values()
            .map(|p| json!({ "id": p.id, "name": p.name, "description": p.description, "entries": p.entries }))
            .collect();
        let entries: Vec<Value> = register
            .entries
            .values()
            .map(|e| {
                json!({
                    "id": e.id,
                    "name": e.name,
                    "kinds": e.kinds,
                    "overseas": e.is_overseas(),
                    "obsolete": e.obsolete.is_some(),
                })
            })
            .collect();
        println!("{}", json!({ "packs": packs, "entries": entries }));
        return Ok(());
    }
    for pack in register.packs.values() {
        println!(
            "pack    {}: {} ({} entries)",
            pack.id,
            pack.description,
            pack.entries.len()
        );
    }
    for entry in register.entries.values() {
        println!("entry   {}: {}", entry.id, entry.name);
    }
    Ok(())
}

pub(super) fn show_plan(selection: &Selection) -> Result<(), String> {
    let register = built_in_register()?;
    let steps = plan::plan(&register, selection)?;
    for step in &steps {
        match step {
            Step::Letter {
                entry,
                channel,
                levers,
            } => {
                println!(
                    "letter  {}  by {} to {}",
                    entry.id,
                    channel.kind.label(),
                    channel.address
                );
                for &lever in levers {
                    let row = law::law(lever);
                    println!(
                        "    {} ({}; {})",
                        lever.name(),
                        row.provision,
                        deadline_summary(row.deadline)
                    );
                }
                println!("    details used: {}", field_names(entry));
            }
            Step::Overseas { entry } => print_overseas(entry),
            Step::Skip { entry, reason } => println!("skip    {}: {reason}", entry.id),
        }
    }
    access_warning(selection);
    Ok(())
}

pub(super) fn show_plan_json(selection: &Selection) -> Result<(), String> {
    let register = built_in_register()?;
    let steps: Vec<Value> = plan::plan(&register, selection)?
        .iter()
        .map(step_json)
        .collect();
    let warnings: Vec<&str> = access_warning_text(selection).into_iter().collect();
    println!("{}", json!({ "steps": steps, "warnings": warnings }));
    Ok(())
}

pub(super) fn step_json(step: &Step) -> Value {
    match step {
        Step::Letter {
            entry,
            channel,
            levers,
        } => {
            let levers: Vec<Value> = levers
                .iter()
                .map(|&lever| {
                    let row = law::law(lever);
                    json!({
                        "lever": lever.name(),
                        "provision": row.provision,
                        "deadline": deadline_summary(row.deadline),
                    })
                })
                .collect();
            let details: Vec<&str> = entry
                .identity_fields
                .iter()
                .map(|&f| field_name(f))
                .collect();
            json!({
                "action": "letter",
                "entry": entry.id,
                "channel": channel.kind.label(),
                "address": channel.address,
                "levers": levers,
                "details_used": details,
            })
        }
        Step::Overseas { entry } => json!({
            "action": "manual",
            "entry": entry.id,
            "note": OVERSEAS_NOTE,
            "page_step": overseas_look(entry),
            "page": overseas_check(entry),
            "forms": overseas_forms(entry),
            "effort": effort_name(entry),
        }),
        Step::Skip { entry, reason } => json!({
            "action": "skip",
            "entry": entry.id,
            "reason": reason,
        }),
    }
}

pub(super) fn overseas_check(entry: &Entry) -> &str {
    entry.manual_check.as_ref().map_or("", |m| m.url.as_str())
}

pub(super) fn overseas_forms(entry: &Entry) -> Vec<&str> {
    entry
        .channels
        .iter()
        .filter(|c| c.kind == ChannelKind::WebForm)
        .map(|c| c.address.as_str())
        .collect()
}

pub(super) fn effort_name(entry: &Entry) -> String {
    format!("{:?}", entry.effort).to_lowercase()
}

pub(super) fn overseas_look(entry: &Entry) -> &'static str {
    if entry.kinds.contains(&Kind::PeopleSearch) {
        "Search for yourself at"
    } else {
        "See"
    }
}

pub(super) fn print_overseas(entry: &Entry) {
    println!(
        "manual  {}: {OVERSEAS_NOTE} {} {}, then use its own opt-out or \
         privacy request form at {} (effort: {}).",
        entry.id,
        overseas_look(entry),
        overseas_check(entry),
        overseas_forms(entry).join(" or "),
        effort_name(entry)
    );
}

pub(super) fn access_warning_text(selection: &Selection) -> Option<&'static str> {
    let access = [Lever::App12, Lever::S20rAccess, Lever::S21tAccess];
    selection
        .levers
        .iter()
        .any(|l| access.contains(l))
        .then_some(
            "access requests return personal information. Make them only about yourself; \
             requesting someone else's information may be an offence.",
        )
}

pub(super) fn access_warning(selection: &Selection) {
    if let Some(text) = access_warning_text(selection) {
        println!("warning: {text}");
    }
}

pub(super) fn field_names(entry: &Entry) -> String {
    let names: Vec<&str> = entry
        .identity_fields
        .iter()
        .map(|&f| field_name(f))
        .collect();
    names.join(", ")
}

pub(super) fn field_name(field: IdentityField) -> &'static str {
    match field {
        IdentityField::FullName => "full name",
        IdentityField::AliasNames => "other names",
        IdentityField::Email => "email",
        IdentityField::Phone => "phone",
        IdentityField::DateOfBirth => "date of birth",
        IdentityField::Address => "address",
        IdentityField::PreviousAddresses => "previous addresses",
        IdentityField::Postcode => "postcode",
    }
}

pub(super) fn deadline_summary(deadline: Deadline) -> String {
    match deadline {
        Deadline::DaysAfter(n) | Deadline::DaysStarting(n) => format!("{n} days"),
        Deadline::BusinessDays(n) => format!("{n} business days"),
        Deadline::Reasonable {
            guide_days: Some(n),
        } => format!("a reasonable period, OAIC guide {n} days"),
        Deadline::Reasonable { guide_days: None } => "a reasonable period".to_owned(),
        Deadline::NotStated => "no time set".to_owned(),
    }
}
