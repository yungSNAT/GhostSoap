// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! Command-line interface.

use std::hash::{BuildHasher, RandomState};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::SystemTime;

use chrono::NaiveDate;
use clap::{Args, CommandFactory, Parser, Subcommand};
use serde_json::{Value, json};

use crate::core::check;
use crate::core::deadline::{self, Due};
use crate::core::embedded;
use crate::core::law::{self, Deadline};
use crate::core::letters::{self, Letter, Request};
use crate::core::plan::{self, Selection, Step};
use crate::core::profile::Profile;
use crate::core::register::{
    self, Channel, ChannelKind, Entry, IdentityField, Kind, Lever, Register, RegisterFiles,
    Severity,
};
use crate::core::tracker::{self, DraftFile, Message, Send};
use crate::io;
use crate::io::prompt::Input;
use crate::io::state::{self, State};

// Every `println!` in this file writes through `print_line`, so a reader that stops early,
// as `ghostsoap law | head -1` does, ends nothing: the command finishes and exits as it
// would have. A `println!` of the standard library would panic there and exit 101.
macro_rules! println {
    () => {
        print_line(format_args!(""))
    };
    ($($arg:tt)*) => {
        print_line(format_args!($($arg)*))
    };
}

/// Write one line to standard output, ignoring only a reader that has gone.
fn print_line(line: std::fmt::Arguments) {
    use std::io::Write;
    if let Err(e) = writeln!(std::io::stdout().lock(), "{line}") {
        assert_eq!(
            e.kind(),
            std::io::ErrorKind::BrokenPipe,
            "failed printing to stdout: {e}"
        );
    }
}

const LEGAL_NOTICE: &str = "GhostSoap provides information and templates, not legal advice.";
const CHECK_LIMITS: &str = "This check matches words. It flags a sentence that holds a word for \
                            erasing and a legal source or a word of duty, so a flag may be \
                            harmless. It also flags the GDPR, California's privacy law, foreign \
                            article numbers and time periods written as a number and a unit, \
                            such as '30 days' or 'a month'. It passes the law table's own \
                            wording wherever it appears, even where that row does not apply to \
                            the business you are writing to. It looks for nothing else, so no \
                            flag does not mean the text is accurate, complete or legally sound.";
const OVERSEAS_NOTE: &str = "run from overseas; GhostSoap has not established that Australian \
                             privacy law reaches it, so it drafts no letter.";

/// Prepare Australian Privacy Act requests to data brokers and track the replies.
#[derive(Debug, Parser)]
#[command(name = "ghostsoap", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create your encrypted profile: the identity details letters may include.
    Init {
        /// Read the passphrase from standard input instead of the terminal (for scripts).
        #[arg(long)]
        passphrase_stdin: bool,
    },
    /// Show the requests a selection would make, without your details. Nothing is written.
    Plan {
        #[command(flatten)]
        selection: SelectionArgs,
        /// Print JSON for scripts and AI agents. It holds no text given with --detail.
        #[arg(long)]
        json: bool,
    },
    /// Write the planned letters to the drafts folder for you to send yourself.
    ///
    /// Each letter is recorded in your encrypted state under the tag in its subject line.
    /// Drafting again for the same business and channel before you record the letter as sent
    /// replaces the earlier draft under the same tag.
    Draft {
        #[command(flatten)]
        selection: SelectionArgs,
        /// Drafts folder; defaults to `drafts` in GhostSoap's data folder. GhostSoap warns
        /// if it finds the folder inside a Git working tree or a OneDrive, Dropbox or iCloud
        /// Drive folder, but it cannot find every such place, so no warning does not mean
        /// the folder is private.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Print each letter's To, Subject and body instead of writing files (for webmail).
        #[arg(long)]
        copy: bool,
        /// Also print a mailto: link for each email letter short enough for one. The link
        /// holds the whole letter, so it stays in your terminal's history.
        #[arg(long, conflicts_with = "copy")]
        mailto: bool,
        /// Read the passphrase from standard input instead of the terminal (for scripts).
        #[arg(long)]
        passphrase_stdin: bool,
    },
    /// Write your profile and every letter's record to a new plaintext file, so they
    /// survive a lost device. Keep it private and delete it when you no longer need it.
    Export {
        /// The file to write; it must not exist yet. Defaults to
        /// `ghostsoap-export-<date>.toml` in GhostSoap's data folder.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Read the passphrase from standard input instead of the terminal (for scripts).
        #[arg(long)]
        passphrase_stdin: bool,
    },
    /// Delete your profile, the record of your letters, exports in the data folder and the
    /// drafts GhostSoap wrote, after listing them and asking you to confirm.
    Forget {
        /// Skip the passphrase; drafts written to folders named with `draft --out` are then
        /// not looked for.
        #[arg(long)]
        no_passphrase: bool,
        /// Delete without asking (for scripts).
        #[arg(long)]
        yes: bool,
        /// Read the passphrase and the confirmation from standard input instead of the
        /// terminal (for scripts).
        #[arg(long)]
        passphrase_stdin: bool,
    },
    /// Delete the drafts GhostSoap wrote; other files in the folder are left alone.
    Clean {
        /// Drafts folder; defaults to `drafts` in GhostSoap's data folder.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Record that you sent a drafted letter, with the day and the channel you used.
    ///
    /// Repeating the same record changes nothing. To change a recorded day or channel, add
    /// --correct: the earlier record is kept as history and the new one takes effect.
    Sent(SentArgs),
    /// Record an answer to a letter you sent, or a step you took, such as a complaint.
    ///
    /// It applies to every request in the letter unless --lever names some. A mistake is
    /// put right with --withdraw, which keeps the wrong record as history.
    Reply(ReplyArgs),
    /// List your requests by what to do next, overdue first, with their dates.
    ///
    /// A statutory date is moved past a weekend (Acts Interpretation Act 1901 s 36(2)); a
    /// public holiday where the business acts moves it further, which GhostSoap does not know.
    Status {
        /// List only requests with a date, soonest first.
        #[arg(long)]
        due: bool,
        /// Read the passphrase from standard input instead of the terminal (for scripts).
        #[arg(long)]
        passphrase_stdin: bool,
    },
    /// Show the law table: each lever's legal basis, deadline and charge rule.
    Law {
        /// Print JSON for scripts and AI agents.
        #[arg(long)]
        json: bool,
    },
    /// Warn about legal claims the law table does not support, in an edited draft or your
    /// own words. It never blocks, and no flag does not mean the text is sound.
    ///
    /// Exits 0 whether or not it flags anything. Exits 1 if it cannot read the file or
    /// refuses it, because it is empty, HTML or a base64 or quoted-printable mail body, whose
    /// words it cannot match. Exits 2 on a usage error.
    Check {
        /// The text to check, such as an edited draft or `--detail` words saved to a file.
        file: PathBuf,
    },
    /// Work with the register of Australian entities.
    Registry {
        #[command(subcommand)]
        command: RegistryCommand,
    },
}

/// Which entities and requests.
#[derive(Debug, Args)]
pub struct SentArgs {
    /// The tag in the letter's subject line, such as GS-7K3QX; repeat for more.
    #[arg(required = true)]
    tags: Vec<String>,
    /// The day you sent it, as YYYY-MM-DD; today if left out.
    #[arg(long, value_parser = parse_date)]
    on: Option<NaiveDate>,
    /// The channel you used, if not the one the letter was drafted for: email, web-form,
    /// post or phone.
    #[arg(long, value_parser = parse_channel)]
    channel: Option<ChannelKind>,
    /// Replace an earlier record of sending that was wrong.
    #[arg(long)]
    correct: bool,
    /// Take back the record of sending: the letter was not sent. Nothing is deleted, and
    /// the letter counts as a draft again.
    #[arg(long, conflicts_with_all = ["on", "channel", "correct"])]
    withdraw: bool,
    /// Why you are withdrawing the record, in your own words, kept with it.
    #[arg(long, requires = "withdraw")]
    note: Option<String>,
    /// Read the passphrase from standard input instead of the terminal (for scripts).
    #[arg(long)]
    passphrase_stdin: bool,
}

#[derive(Debug, Args)]
pub struct ReplyArgs {
    /// The tag in the letter's subject line, such as GS-7K3QX.
    tag: String,
    /// What happened: acknowledged, id-requested, charge-quoted, fulfilled, partial,
    /// suppressed, no-record, refused-with-reasons, refused, exempt-claimed, bounced,
    /// relisted, escalated, closed (finished, with an answer) or abandoned (given up).
    #[arg(value_parser = parse_status, required_unless_present_any = ["agreed_until", "withdraw"])]
    status: Option<tracker::Status>,
    /// The request it applies to; repeat for more. All of the letter's requests if left out.
    #[arg(long, value_parser = parse_lever)]
    lever: Vec<Lever>,
    /// The day it happened, as YYYY-MM-DD; today if left out.
    #[arg(long, value_parser = parse_date)]
    on: Option<NaiveDate>,
    /// The ground given for a refusal: not-direct-marketing, small-business,
    /// outside-jurisdiction, insufficient-verification or other.
    #[arg(long, value_parser = parse_reason, requires = "status")]
    reason: Option<tracker::Reason>,
    /// A source of your information the answer names, in your own words (APP 7.6(e)).
    #[arg(long, requires = "status")]
    source: Option<String>,
    /// Your own words, kept with the record.
    #[arg(long)]
    note: Option<String>,
    /// The last day of a longer period you agreed to in writing for a credit correction
    /// (Privacy Act ss 20T(2)(b), 21V(2)(b)).
    #[arg(long, value_parser = parse_date, conflicts_with = "status")]
    agreed_until: Option<NaiveDate>,
    /// Take back the latest answer (`status`) or longer period (`extension`) recorded: it
    /// was wrong. It stays as history.
    #[arg(long, value_parser = parse_withdrawn, conflicts_with_all = ["status", "agreed_until", "on"])]
    withdraw: Option<tracker::Withdrawn>,
    /// Read the passphrase from standard input instead of the terminal (for scripts).
    #[arg(long)]
    passphrase_stdin: bool,
}

#[derive(Debug, Args)]
pub struct SelectionArgs {
    /// A pack of entities, such as `start-here`; repeat for more.
    #[arg(long)]
    pack: Vec<String>,
    /// One entity by id, such as `quantium-au`; repeat for more.
    #[arg(long)]
    entry: Vec<String>,
    /// A request to make instead of the default APP 7 requests; repeat for more. Access,
    /// correction and ban requests need a single --entry.
    #[arg(long, value_parser = parse_lever)]
    lever: Vec<Lever>,
    /// Your own words for levers that need them, such as what is wrong for a correction.
    #[arg(long)]
    detail: Option<String>,
}

impl SelectionArgs {
    fn selection(&self) -> Selection {
        Selection {
            packs: self.pack.clone(),
            entries: self.entry.clone(),
            levers: self.lever.clone(),
            detail: self.detail.clone(),
        }
    }
}

fn parse_lever(name: &str) -> Result<Lever, String> {
    Lever::from_name(name).ok_or_else(|| {
        let names: Vec<&str> = Lever::ALL.iter().map(|l| l.name()).collect();
        format!("unknown lever; use one of {}", names.join(", "))
    })
}

fn parse_date(text: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .map_err(|_| "use a date such as 2026-11-30".to_owned())
}

/// A name as the state file writes it, such as `refused-with-reasons`.
fn parse_named<T: serde::de::DeserializeOwned>(name: &str, names: &str) -> Result<T, String> {
    serde_json::from_value(json!(name)).map_err(|_| format!("use {names}"))
}

fn parse_status(name: &str) -> Result<tracker::Status, String> {
    parse_named(name, "a status `ghostsoap reply --help` lists").and_then(|status| match status {
        tracker::Status::Drafted | tracker::Status::Sent => {
            Err("`draft` and `sent` record those".to_owned())
        }
        status => Ok(status),
    })
}

fn parse_reason(name: &str) -> Result<tracker::Reason, String> {
    parse_named(
        name,
        "not-direct-marketing, small-business, outside-jurisdiction, insufficient-verification or other",
    )
}

fn parse_withdrawn(name: &str) -> Result<tracker::Withdrawn, String> {
    parse_named(name, "status or extension")
}

/// The name the state file gives `value`, such as `refused-with-reasons`.
fn named(value: impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .expect("a unit variant serialises as its name")
}

fn parse_channel(name: &str) -> Result<ChannelKind, String> {
    match name {
        "email" => Ok(ChannelKind::Email),
        "web-form" => Ok(ChannelKind::WebForm),
        "post" => Ok(ChannelKind::Post),
        "phone" => Ok(ChannelKind::Phone),
        _ => Err("use email, web-form, post or phone".to_owned()),
    }
}

#[derive(Debug, Subcommand)]
pub enum RegistryCommand {
    /// Validate register entries and packs.
    Check {
        /// Check a register directory, such as a working copy's `registry/au`, instead of
        /// the register built into this binary. Packs are read from its `packs` subdirectory.
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// List the packs and entries built into this binary.
    List {
        /// Print JSON for scripts and AI agents.
        #[arg(long)]
        json: bool,
    },
}

/// Run a parsed command line.
#[allow(clippy::too_many_lines)] // one arm per command; splitting it would only scatter them
pub fn run(cli: Cli) -> ExitCode {
    let result = match cli.command {
        None => Cli::command().print_help().map_err(|e| e.to_string()),
        Some(Command::Init { passphrase_stdin }) => init(passphrase_stdin),
        Some(Command::Plan { selection, json }) => {
            if json {
                show_plan_json(&selection.selection())
            } else {
                show_plan(&selection.selection())
            }
        }
        Some(Command::Draft {
            selection,
            out,
            copy,
            mailto,
            passphrase_stdin,
        }) => draft(
            &selection.selection(),
            out,
            Output { copy, mailto },
            passphrase_stdin,
        ),
        Some(Command::Export {
            out,
            passphrase_stdin,
        }) => export(out, passphrase_stdin),
        Some(Command::Forget {
            no_passphrase,
            yes,
            passphrase_stdin,
        }) => forget(no_passphrase, yes, passphrase_stdin),
        Some(Command::Clean { out }) => clean(out),
        Some(Command::Sent(args)) => sent(&args),
        Some(Command::Reply(args)) => reply(&args),
        Some(Command::Status {
            due,
            passphrase_stdin,
        }) => status(due, passphrase_stdin),
        Some(Command::Law { json }) => {
            if json {
                show_law_json();
            } else {
                show_law();
            }
            Ok(())
        }
        Some(Command::Check { file }) => check_file(&file),
        Some(Command::Registry {
            command: RegistryCommand::Check { path },
        }) => return registry_command(path),
        Some(Command::Registry {
            command: RegistryCommand::List { json },
        }) => list_register(json),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn init(passphrase_stdin: bool) -> Result<(), String> {
    let dir = state::data_dir().map_err(|e| e.to_string())?;
    let path = state::state_path(&dir);
    if path.exists() {
        return Err(format!(
            "a profile already exists at {}; delete that file to start again, which also deletes the record of your letters",
            path.display()
        ));
    }
    warn_exposed(&dir, DATA_FOLDER);
    let mut input = Input::new(passphrase_stdin);
    let profile = input.profile().map_err(|e| e.to_string())?;
    let problems = profile.problems();
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    let passphrase = input.new_passphrase().map_err(|e| e.to_string())?;
    state::create(&dir, &State::new(profile), passphrase).map_err(|e| e.to_string())?;
    println!("Saved your encrypted profile to {}.", path.display());
    println!("A lost passphrase cannot be recovered; keep it somewhere safe.");
    Ok(())
}

fn built_in_register() -> Result<Register, String> {
    register::parse_register(&embedded::register())
}

fn list_register(json: bool) -> Result<(), String> {
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

fn show_plan(selection: &Selection) -> Result<(), String> {
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

fn show_plan_json(selection: &Selection) -> Result<(), String> {
    let register = built_in_register()?;
    let steps: Vec<Value> = plan::plan(&register, selection)?
        .iter()
        .map(step_json)
        .collect();
    let warnings: Vec<&str> = access_warning_text(selection).into_iter().collect();
    println!("{}", json!({ "steps": steps, "warnings": warnings }));
    Ok(())
}

fn step_json(step: &Step) -> Value {
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

/// How `draft` hands over its letters.
#[derive(Clone, Copy)]
struct Output {
    /// Print them instead of writing files.
    copy: bool,
    /// Also print mailto: links.
    mailto: bool,
}

fn draft(
    selection: &Selection,
    out: Option<PathBuf>,
    output: Output,
    passphrase_stdin: bool,
) -> Result<(), String> {
    let register = built_in_register()?;
    let steps = plan::plan(&register, selection)?;
    let dir = drafts_dir(out.as_deref())?;
    warn_about_out(&dir, out.is_some(), output);
    let mut planned = Vec::new();
    for step in &steps {
        match step {
            Step::Letter {
                entry,
                channel,
                levers,
            } => planned.push(Planned {
                entry,
                channel,
                levers,
            }),
            Step::Overseas { entry } => print_overseas(entry),
            Step::Skip { entry, reason } => println!("skip    {}: {reason}", entry.id),
        }
    }
    if !planned.is_empty() {
        let folder = if output.copy {
            None
        } else {
            Some(draft_destination(&dir)?)
        };
        let detail = selection.detail.as_deref();
        let destination = folder.as_deref().map_or(dir.as_path(), Path::new);
        // The state records every letter before one is written or shown, so a letter the
        // user can send always has a record for `sent` to find. Each is marked unfinished
        // until all are handed over, so a run that fails or stops leaves `sent` refusing them.
        let drafted = update_state(
            passphrase_stdin,
            |state| record_drafts(state, &planned, detail, folder.as_deref(), today()),
            |(_, redraft)| redraft.commit(),
            |(drafted, redraft), state| {
                publish_drafts(destination, drafted, output, redraft, state)
            },
        )?;
        let tags: Vec<&str> = drafted.iter().map(|d| d.tag.as_str()).collect();
        println!(
            "After you send a letter, record it: ghostsoap sent {}",
            tags.join(" ")
        );
        if !output.copy {
            print_clean_reminder();
        }
    }
    access_warning(selection);
    Ok(())
}

fn publish_drafts<'a>(
    dir: &Path,
    drafted: Vec<Drafted<'a>>,
    output: Output,
    mut redraft: io::drafts::Redraft,
    state: &mut State,
) -> Result<Vec<Drafted<'a>>, String> {
    redraft.unlock();
    for letter in &drafted {
        hand_over(dir, letter, output)?;
    }
    redraft.discard();
    for message in &mut state.messages {
        if drafted.iter().any(|letter| letter.tag == message.tag) {
            message.unfinished = false;
        }
    }
    Ok(drafted)
}

/// A letter the plan calls for.
struct Planned<'a> {
    entry: &'a Entry,
    channel: &'a Channel,
    levers: &'a [Lever],
}

/// A letter the state has recorded, ready to write or show.
struct Drafted<'a> {
    entry: &'a Entry,
    tag: String,
    letter: Letter,
    /// The entry's identity fields the profile has no value for.
    missing: Vec<IdentityField>,
    /// The levers an earlier draft held and this one drops.
    dropped: Vec<Lever>,
    replaces: Option<String>,
}

/// Record each planned letter as drafted and return what to hand over. A plain draft to the
/// same entry and channel is replaced under its tag, after its file is taken out of the
/// folder it was written to. A draft with history keeps its record under the old tag.
fn record_drafts<'a>(
    state: &mut State,
    planned: &[Planned<'a>],
    detail: Option<&str>,
    folder: Option<&str>,
    today: NaiveDate,
) -> Result<(Vec<Drafted<'a>>, io::drafts::Redraft), String> {
    // Every letter is composed first: one that cannot be written changes nothing.
    let drafted = compose_drafts(state, planned, detail, today)?;
    // An earlier draft leaves its folder before the state changes, so the letter on disk is
    // never one the state no longer describes.
    let redraft = stage_earlier_drafts(state, &drafted)?;
    for (plan, letter) in planned.iter().zip(&drafted) {
        if let Some(earlier) = letter.replaces.as_deref()
            && earlier != letter.tag
            && let Some(previous) = state.message_mut(earlier)
        {
            previous.draft = None;
            previous.superseded = true;
        }
        let message = Message {
            tag: letter.tag.clone(),
            entry: plan.entry.id.clone(),
            channel: plan.channel.kind,
            address: plan.channel.address.clone(),
            drafted: today,
            draft: folder.map(|folder| DraftFile {
                folder: folder.to_owned(),
                name: draft_name(plan.entry, &letter.tag, plan.channel.kind),
            }),
            superseded: false,
            unfinished: true,
            sends: Vec::new(),
            requests: plan
                .levers
                .iter()
                .map(|&lever| tracker::Request::new(lever))
                .collect(),
        };
        match state.message_mut(&letter.tag) {
            Some(slot) => *slot = message,
            None => state.messages.push(message),
        }
    }
    Ok((drafted, redraft))
}

/// Compose each planned letter, under the tag of the plain draft it replaces or a new one.
fn compose_drafts<'a>(
    state: &State,
    planned: &[Planned<'a>],
    detail: Option<&str>,
    today: NaiveDate,
) -> Result<Vec<Drafted<'a>>, String> {
    let mut tags: Vec<String> = state.messages.iter().map(|m| m.tag.clone()).collect();
    // The tags this run's letters have taken over from earlier drafts.
    let mut replaced = Vec::new();
    let mut drafted = Vec::new();
    for plan in planned {
        let Planned {
            entry,
            channel,
            levers,
        } = *plan;
        let earlier = state.draft_to_replace(entry, channel, &replaced);
        let previous_tag = earlier.map(|message| message.tag.clone());
        if let Some(tag) = &previous_tag {
            replaced.push(tag.clone());
        }
        let tag = earlier
            .filter(|message| {
                message.sends.is_empty()
                    && message
                        .requests
                        .iter()
                        .all(|request| request.events.is_empty())
            })
            .map_or_else(|| new_tag(&tags), |message| message.tag.clone());
        tags.push(tag.clone());
        let request = Request {
            channel,
            levers,
            detail,
            date: today,
            tag: &tag,
        };
        drafted.push(Drafted {
            entry,
            letter: letters::compose(entry, &state.profile, &request)?,
            missing: missing_fields(entry, &state.profile),
            dropped: earlier.map_or_else(Vec::new, |message| {
                let held = message.requests.iter().map(|r| r.lever);
                held.filter(|lever| !levers.contains(lever)).collect()
            }),
            replaces: previous_tag,
            tag,
        });
    }
    Ok(drafted)
}

/// Delete the draft file of each message `drafted` replaces. A file GhostSoap cannot confirm
/// it wrote stops the redraft, since it may hold the user's own changes.
fn stage_earlier_drafts(state: &State, drafted: &[Drafted]) -> Result<io::drafts::Redraft, String> {
    let earlier: Vec<_> = drafted
        .iter()
        .filter_map(|letter| {
            let message = state.message(letter.replaces.as_deref()?)?;
            Some((&message.tag, message.draft.as_ref()?))
        })
        .collect();
    let changed = |tag: &str, file: &DraftFile| {
        format!(
            "{} in {} has the name of the earlier draft under {tag}, and GhostSoap cannot \
         confirm it still holds the letter it wrote; move or delete it yourself, then draft again",
            file.name, file.folder
        )
    };
    for (tag, file) in &earlier {
        if !io::drafts::can_remove(Path::new(&file.folder), &file.name)
            .map_err(|e| e.to_string())?
        {
            return Err(changed(tag, file));
        }
    }
    let mut redraft = io::drafts::Redraft::default();
    for (tag, file) in earlier {
        redraft
            .stage(Path::new(&file.folder), &file.name)
            .map_err(|e| format!("could not set aside the earlier draft under {tag}: {e}"))?;
    }
    Ok(redraft)
}

/// Write or show one recorded letter.
fn hand_over(dir: &Path, drafted: &Drafted, output: Output) -> Result<(), String> {
    let Drafted {
        entry, tag, letter, ..
    } = drafted;
    for &field in &drafted.missing {
        println!(
            "note    {}: your profile has no {}, so the letter leaves it out",
            entry.id,
            field_name(field)
        );
    }
    if let Some(earlier) = &drafted.replaces {
        let dropped: Vec<&str> = drafted.dropped.iter().map(|l| l.name()).collect();
        let dropped = match dropped.as_slice() {
            [] => String::new(),
            names => format!(" and no longer asks for {}", names.join(", ")),
        };
        if earlier == tag {
            println!(
                "note    {}: this draft replaces the earlier one under {tag}{dropped}",
                entry.id
            );
        } else {
            println!(
                "note    {}: draft {tag} replaces {earlier}{dropped}; the earlier record stays as history",
                entry.id
            );
        }
    }
    if output.copy {
        print_copy(entry, letter);
        return Ok(());
    }
    write_draft(dir, entry, tag, letter, output.mailto).map_err(|e| {
        format!(
            "{e}; {tag} and the other letters of this run are recorded as unfinished, so `sent` \
             refuses them until you draft them again. If GhostSoap cannot confirm a file it \
             left, move that file yourself first"
        )
    })
}

/// Resolve the destination before saving it, so a later change to an ancestor link cannot hide
/// an earlier draft. On Unix the final component must still pass the private-folder check.
fn draft_destination(dir: &Path) -> Result<String, String> {
    let full = std::path::absolute(dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    let resolved = full
        .parent()
        .zip(full.file_name())
        .map(|(parent, name)| io::exposure::resolve(parent).join(name))
        .unwrap_or(full);
    #[cfg(not(unix))]
    let resolved = io::exposure::resolve(&full);
    resolved
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{} is not a Unicode path", dir.display()))
}

fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

fn sent(args: &SentArgs) -> Result<(), String> {
    let today = today();
    let lines = update_state(
        args.passphrase_stdin,
        |state| {
            let mut lines = Vec::new();
            for typed in &args.tags {
                let message = message_for(state, typed)?;
                let recorded = record_sending(message, args, today)?;
                lines.push(format!(
                    "sent    {}  {}  {recorded}",
                    message.tag, message.entry
                ));
            }
            Ok(lines)
        },
        |_| {},
        |lines, _| Ok(lines),
    )?;
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

/// The message under the tag `typed`, as the user typed it.
fn message_for<'a>(state: &'a mut State, typed: &str) -> Result<&'a mut Message, String> {
    let tag = letters::normalise_tag(typed);
    state.message_mut(&tag).ok_or_else(|| {
        format!("no letter is recorded under {tag}; the tag is in the letter's subject line")
    })
}

fn reply(args: &ReplyArgs) -> Result<(), String> {
    let today = today();
    let on = args.on.unwrap_or(today);
    let line = update_state(
        args.passphrase_stdin,
        |state| {
            let message = message_for(state, &args.tag)?;
            let note = args.note.clone();
            let done = if let Some(of) = args.withdraw {
                message.withdraw_event(&args.lever, of, note)?;
                let what = match of {
                    tracker::Withdrawn::Status => "answer",
                    tracker::Withdrawn::Extension => "longer period",
                };
                format!("the latest {what} is withdrawn")
            } else if let Some(until) = args.agreed_until {
                let extension = tracker::Extension { on, until, note };
                let new = message.record_extension(&args.lever, extension, today)?;
                format!("{} a longer period to {until}", recorded(new))
            } else {
                let status = args.status.expect("clap requires a status");
                let event = tracker::StatusEvent {
                    on,
                    status,
                    reason: args.reason,
                    source: args.source.clone(),
                    note,
                };
                let new = message.record_status(&args.lever, event, today)?;
                format!("{} {} on {on}", recorded(new), named(status))
            };
            Ok(format!(
                "reply   {}  {}  {done}",
                message.tag, message.entry
            ))
        },
        |_| {},
        |line, _| Ok(line),
    )?;
    println!("{line}");
    Ok(())
}

fn recorded(new: bool) -> &'static str {
    if new { "recorded" } else { "already recorded" }
}

/// What `status` calls each action state.
fn action_name(action: tracker::Action) -> &'static str {
    match action {
        tracker::Action::Overdue => "overdue",
        tracker::Action::Needed => "action needed",
        tracker::Action::Waiting => "waiting",
        tracker::Action::Done => "done",
        tracker::Action::Abandoned => "abandoned",
    }
}

const ACTIONS: [tracker::Action; 5] = [
    tracker::Action::Overdue,
    tracker::Action::Needed,
    tracker::Action::Waiting,
    tracker::Action::Done,
    tracker::Action::Abandoned,
];

/// The date a deadline gives, if any.
fn due_date(due: Due) -> Option<NaiveDate> {
    match due {
        Due::Statutory(day) | Due::Guide(day) => Some(day),
        Due::Unknown | Due::None => None,
    }
}

/// What `status` says about a deadline.
fn due_text(due: Due) -> String {
    match due {
        Due::Statutory(day) => format!("  law's last day {day}"),
        Due::Guide(day) => format!("  OAIC guide {day}"),
        Due::Unknown => {
            "  takes effect 5 business days after sending, counted where it was sent".to_owned()
        }
        Due::None => String::new(),
    }
}

/// List every request that is not history, by action state and then in the order drafted,
/// or with `due_only` only those with a date, soonest first. A state that cannot be read is
/// reported as DEGRADED with unknown counts, never as nothing to do.
fn status(due_only: bool, passphrase_stdin: bool) -> Result<(), String> {
    let loaded = state::data_dir().and_then(|dir| {
        let passphrase = Input::new(passphrase_stdin).passphrase("Passphrase: ")?;
        state::load(&dir, passphrase)
    });
    let state = match loaded {
        Ok(state) => state,
        Err(e) => {
            println!("DEGRADED: {e}");
            println!("{}", counts(|_| "?".to_owned()));
            return Err("the state could not be read, so this is not an all-clear".to_owned());
        }
    };
    let today = today();
    let mut rows = Vec::new();
    for message in state.messages.iter().filter(|m| !m.superseded) {
        let sent = message.sent().is_some();
        for request in &message.requests {
            let due = deadline::due(message, request);
            rows.push((request.action(sent, due, today), due, message, request));
        }
    }
    println!(
        "{}",
        counts(|a| rows.iter().filter(|r| r.0 == a).count().to_string())
    );
    if due_only {
        rows.retain(|row| due_date(row.1).is_some());
        rows.sort_by_key(|row| due_date(row.1));
    } else {
        rows.sort_by_key(|row| row.0);
    }
    for &(action, due, message, request) in &rows {
        println!("{}", status_line(action, due, message, request));
    }
    if rows.iter().any(|row| matches!(row.1, Due::Statutory(_))) {
        println!(
            "A law's last day is moved past a weekend; a public holiday where the business acts \
             moves it to the next working day, which GhostSoap does not know (Acts \
             Interpretation Act 1901 s 36(2))."
        );
    }
    Ok(())
}

/// The count line: each action state and what `count` gives for it.
fn counts(count: impl Fn(tracker::Action) -> String) -> String {
    let parts: Vec<String> = ACTIONS
        .iter()
        .map(|&a| format!("{} {}", action_name(a), count(a)))
        .collect();
    parts.join("  ")
}

/// One request as `status` lists it.
fn status_line(
    action: tracker::Action,
    due: Due,
    message: &Message,
    request: &tracker::Request,
) -> String {
    let unfinished = if message.unfinished {
        "  (its draft run did not finish: draft it again)"
    } else {
        ""
    };
    format!(
        "{:<15}{}  {}  {}  {}{}{unfinished}",
        action_name(action),
        message.tag,
        message.entry,
        request.lever.name(),
        named(request.status(message.sent().is_some())),
        due_text(due),
    )
}

/// Apply `sent`'s flags to one message and say what is now recorded.
fn record_sending(
    message: &mut Message,
    args: &SentArgs,
    today: NaiveDate,
) -> Result<String, String> {
    if args.withdraw {
        message.withdraw_sent(args.note.clone())?;
        return Ok("no longer recorded as sent".to_owned());
    }
    let send = Send {
        on: args.on.unwrap_or(today),
        channel: args.channel.unwrap_or(message.channel),
    };
    let recorded = if message.record_sent(send, args.correct, today)? {
        "recorded as sent"
    } else {
        "already recorded as sent"
    };
    Ok(format!(
        "{recorded} on {} by {}",
        send.on,
        send.channel.label()
    ))
}

fn print_clean_reminder() {
    println!(
        "Drafts are plaintext. After sending them from your own mailbox, run `ghostsoap clean`."
    );
}

/// Warnings about the folder drafts go to, `named` with `--out`; none with `--copy`, which
/// writes nothing.
fn warn_about_out(dir: &Path, named: bool, output: Output) {
    if !output.copy {
        warn_unchecked_out(named, cfg!(windows), std::io::stderr());
        warn_exposed(dir, DRAFTS_FOLDER);
    }
}

/// On Windows the drafts folder keeps the access list it inherits, which GhostSoap does not
/// check; say so when the user names the folder (`named`).
fn warn_unchecked_out(named: bool, windows: bool, mut output: impl std::io::Write) {
    if windows && named {
        writeln!(
            output,
            "note: on Windows GhostSoap does not check who else can open the --out folder; \
             choose one only you can open"
        )
        .expect("write the drafts folder warning");
    }
}

/// A folder `warn_exposed` checks: what to call it and what it holds.
type Folder = (&'static str, &'static str);
const DRAFTS_FOLDER: Folder = ("the drafts folder", "plaintext letters");
const DATA_FOLDER: Folder = ("GhostSoap's data folder", "your encrypted state");

/// Warn when `dir` is inside a Git working tree or a known sync folder: the drafts folder,
/// and the data folder at `init` and before each change to the state. The checks are
/// incomplete (see `io::exposure`), so no warning says nothing about safety.
fn warn_exposed(dir: &Path, (folder, holds): Folder) {
    let roots = io::exposure::sync_roots(|name| std::env::var_os(name));
    for exposure in io::exposure::exposures(dir, &roots) {
        let (place, consequence) = match exposure {
            io::exposure::Exposure::Git { root } => (
                format!("the Git working tree at {}", io::exposure::display(&root)),
                "committed",
            ),
            io::exposure::Exposure::Sync { service, root } => (
                format!("the {service} folder at {}", io::exposure::display(&root)),
                "uploaded",
            ),
        };
        eprintln!("warning: {folder} is inside {place}; {holds} there could be {consequence}");
    }
}

/// Change the state under its lock (see `state::update`), asking for the passphrase first.
fn update_state<T, U>(
    passphrase_stdin: bool,
    change: impl FnOnce(&mut State) -> Result<T, String>,
    committed: impl FnOnce(&mut T),
    after_save: impl FnOnce(T, &mut State) -> Result<U, String>,
) -> Result<U, String> {
    let dir = state::data_dir().map_err(|e| e.to_string())?;
    warn_exposed(&dir, DATA_FOLDER);
    let passphrase = Input::new(passphrase_stdin)
        .passphrase("Passphrase: ")
        .map_err(|e| e.to_string())?;
    state::update_then(&dir, passphrase, change, committed, after_save).map_err(|e| e.to_string())
}

/// A tag no message in the state and no letter in this run has: `used` holds both, as 5
/// Crockford base32 characters give only 33,554,432 values. `RandomState` is seeded randomly
/// per process, so hashing the time gives unpredictable bits without another dependency;
/// tags only need to be distinct, not secret.
fn new_tag(used: &[String]) -> String {
    let state = RandomState::new();
    unused_tag(used, |n| {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        letters::tag(state.hash_one((now, n)))
    })
}

fn unused_tag(used: &[String], candidate: impl Fn(u64) -> String) -> String {
    let mut n = 0u64;
    loop {
        let tag = candidate(n);
        if !used.contains(&tag) {
            return tag;
        }
        n += 1;
    }
}

fn write_draft(
    dir: &Path,
    entry: &Entry,
    tag: &str,
    letter: &Letter,
    mailto: bool,
) -> Result<(), String> {
    let how = match letter.kind {
        ChannelKind::Email => format!("email to {}", letter.to),
        ChannelKind::Post => format!("print and post to {}", letter.to),
        ChannelKind::WebForm => format!("paste into the form at {}", letter.to),
        ChannelKind::Phone => return Err("plans never send letters by phone".to_owned()),
    };
    let text = match letter.kind {
        ChannelKind::Email => letters::eml(letter),
        _ => format!("{}\n", letter.body),
    };
    let name = draft_name(entry, tag, letter.kind);
    let path = io::drafts::write(dir, &name, &text).map_err(|e| e.to_string())?;
    println!(
        "draft   {}  {how}: {}",
        entry.id,
        io::exposure::display(&path)
    );
    if mailto && letter.kind == ChannelKind::Email {
        match letters::mailto(letter) {
            Some(link) => println!("        or open: {link}"),
            None => println!("        too long for a mailto: link; open the draft instead"),
        }
    }
    Ok(())
}

/// The file a draft is written to: an email as `.eml`, anything else as text.
fn draft_name(entry: &Entry, tag: &str, kind: ChannelKind) -> String {
    let extension = if kind == ChannelKind::Email {
        "eml"
    } else {
        "txt"
    };
    format!("{}-{tag}.{extension}", entry.id)
}

fn print_copy(entry: &Entry, letter: &Letter) {
    println!("==== {} ({})", entry.id, letter.kind.label());
    println!("To: {}", letter.to);
    println!("Subject: {}", letter.subject);
    println!();
    println!("{}", letter.body);
    println!();
}

fn overseas_check(entry: &Entry) -> &str {
    entry.manual_check.as_ref().map_or("", |m| m.url.as_str())
}

fn overseas_forms(entry: &Entry) -> Vec<&str> {
    entry
        .channels
        .iter()
        .filter(|c| c.kind == ChannelKind::WebForm)
        .map(|c| c.address.as_str())
        .collect()
}

fn effort_name(entry: &Entry) -> String {
    format!("{:?}", entry.effort).to_lowercase()
}

fn overseas_look(entry: &Entry) -> &'static str {
    if entry.kinds.contains(&Kind::PeopleSearch) {
        "Search for yourself at"
    } else {
        "See"
    }
}

fn print_overseas(entry: &Entry) {
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

fn access_warning_text(selection: &Selection) -> Option<&'static str> {
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

fn access_warning(selection: &Selection) {
    if let Some(text) = access_warning_text(selection) {
        println!("warning: {text}");
    }
}

/// The identity fields `entry` takes that `profile` has no value for.
fn missing_fields(entry: &Entry, profile: &Profile) -> Vec<IdentityField> {
    let fields = entry.identity_fields.iter().copied();
    fields
        .filter(|&field| profile.value(field).is_none())
        .collect()
}

fn field_names(entry: &Entry) -> String {
    let names: Vec<&str> = entry
        .identity_fields
        .iter()
        .map(|&f| field_name(f))
        .collect();
    names.join(", ")
}

fn field_name(field: IdentityField) -> &'static str {
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

fn deadline_summary(deadline: Deadline) -> String {
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

fn export(out: Option<PathBuf>, passphrase_stdin: bool) -> Result<(), String> {
    let dir = state::data_dir().map_err(|e| e.to_string())?;
    let date = today().to_string();
    let path = out.unwrap_or_else(|| dir.join(format!("ghostsoap-export-{date}.toml")));
    let passphrase = Input::new(passphrase_stdin)
        .passphrase("Passphrase: ")
        .map_err(|e| e.to_string())?;
    state::export(&dir, passphrase, &path, &date).map_err(|e| e.to_string())?;
    println!("Exported your profile and records to {}.", path.display());
    let folder = path.parent().unwrap_or(Path::new("."));
    warn_exposed(folder, ("the export's folder", "the plaintext export"));
    eprintln!(
        "warning: {} is plaintext: anyone who can open it can read your profile and every \
         letter's record. Keep it private, and delete it when you no longer need it.",
        path.display()
    );
    Ok(())
}

fn clean(out: Option<PathBuf>) -> Result<(), String> {
    let own_folder = out.is_none();
    let dir = drafts_dir(out.as_deref())?;
    let cleaned = io::drafts::clean(&dir, own_folder).map_err(|e| e.to_string())?;
    report_cleaned(&dir, &cleaned);
    Ok(())
}

/// Delete the state, exports in the data folder and the drafts GhostSoap wrote, once the
/// user confirms the list. A path that cannot be deleted is reported and the rest go on.
fn forget(no_passphrase: bool, yes: bool, passphrase_stdin: bool) -> Result<(), String> {
    let dir = state::data_dir().map_err(|e| e.to_string())?;
    let mut input = Input::new(passphrase_stdin);
    let files = state::own_files(&dir).map_err(|e| e.to_string())?;
    let mut folders = draft_folders(&dir, no_passphrase, &mut input)?;
    folders.retain(|folder| folder.exists());
    if files.is_empty() && folders.is_empty() {
        println!("GhostSoap has nothing to delete in {}.", dir.display());
        return Ok(());
    }
    let lock = state::lock(&dir).map_err(|e| e.to_string())?;
    list_forgotten(&files, &folders, no_passphrase);
    if !yes
        && input
            .line("Type forget to delete them: ")
            .map_err(|e| e.to_string())?
            != "forget"
    {
        return Err("nothing was deleted".to_owned());
    }
    let own = io::drafts::default_dir(&dir);
    let mut failed = 0;
    for file in &files {
        let deleted = std::fs::remove_file(file).map(|()| println!("Deleted {}.", file.display()));
        tally(file, deleted, &mut failed);
    }
    for folder in &folders {
        let cleaned = io::drafts::clean(folder, *folder == own);
        tally(
            folder,
            cleaned.map(|c| report_cleaned(folder, &c)),
            &mut failed,
        );
    }
    drop(lock);
    // The data folder goes too unless it holds files GhostSoap did not write.
    let _ = std::fs::remove_dir(&dir);
    match failed {
        0 => Ok(()),
        n => Err(format!("{n} could not be deleted; see the errors above")),
    }
}

/// Show what `forget` will delete, and what it cannot look for.
fn list_forgotten(files: &[PathBuf], folders: &[PathBuf], no_passphrase: bool) {
    println!("GhostSoap will delete:");
    for file in files {
        println!("    {}", file.display());
    }
    for folder in folders {
        println!(
            "    the drafts it wrote in {} that still hold what it wrote; other files stay",
            folder.display()
        );
    }
    if no_passphrase {
        println!("Without the passphrase, folders named with `draft --out` are not looked in.");
    }
    println!(
        "GhostSoap keeps nothing in the system keyring. Deleting does not erase: the disk may \
         still hold what was deleted."
    );
}

/// GhostSoap's own drafts folder, then each other folder the state records a draft in.
fn draft_folders(
    dir: &Path,
    no_passphrase: bool,
    input: &mut Input,
) -> Result<Vec<PathBuf>, String> {
    let mut folders = vec![io::drafts::default_dir(dir)];
    if no_passphrase || !state::state_path(dir).exists() {
        return Ok(folders);
    }
    let passphrase = input
        .passphrase("Passphrase: ")
        .map_err(|e| e.to_string())?;
    let state = state::load(dir, passphrase).map_err(|e| e.to_string())?;
    for draft in state.messages.iter().filter_map(|m| m.draft.as_ref()) {
        let folder = PathBuf::from(&draft.folder);
        if !folders.contains(&folder) {
            folders.push(folder);
        }
    }
    Ok(folders)
}

/// Report a path `forget` could not delete, and count it.
fn tally(path: &Path, result: std::io::Result<()>, failed: &mut usize) {
    if let Err(e) = result {
        *failed += 1;
        eprintln!("error: cannot delete {}: {e}", path.display());
    }
}

/// Print what `clean` did in `dir`.
fn report_cleaned(dir: &Path, cleaned: &io::drafts::Cleaned) {
    println!("Deleted {} drafts from {}.", cleaned.deleted, dir.display());
    for name in &cleaned.kept {
        println!("kept    {name}: it no longer holds the letter GhostSoap wrote");
    }
    for name in &cleaned.unconfirmed {
        println!("check   {name}: its write never finished recording; delete it if unwanted");
    }
    for name in &cleaned.leftovers {
        println!("check   {name}: left by a write that stopped part-way; delete it if unwanted");
    }
    for name in &cleaned.unrecorded {
        println!("check   {name}: GhostSoap has no record of writing it; delete it if unwanted");
    }
}

/// The folder `--out` names, else the default drafts folder in the data directory.
fn drafts_dir(out: Option<&Path>) -> Result<PathBuf, String> {
    match out {
        Some(dir) => Ok(dir.to_owned()),
        None => state::data_dir()
            .map(|d| io::drafts::default_dir(&d))
            .map_err(|e| e.to_string()),
    }
}

fn show_law() {
    for lever in Lever::ALL {
        let row = law::law(lever);
        println!("{}: {}", lever.name(), row.title);
        println!("    basis: {}", row.provision);
        println!("    deadline: {}", row.deadline_text);
        println!("    charge: {}", row.charge);
    }
    println!();
    println!("Sources:");
    for source in law::SOURCES {
        println!("    {source}");
    }
    println!();
    println!(
        "Law status, read {} unless a line says otherwise:",
        law::STATUS_READ
    );
    for line in law::STATUS {
        println!("    {line}");
    }
    println!("{LEGAL_NOTICE}");
}

fn show_law_json() {
    let levers: Vec<Value> = Lever::ALL
        .iter()
        .map(|&lever| {
            let row = law::law(lever);
            json!({
                "lever": lever.name(),
                "title": row.title,
                "basis": row.provision,
                "deadline": row.deadline_text,
                "charge": row.charge,
            })
        })
        .collect();
    println!(
        "{}",
        json!({
            "levers": levers,
            "sources": law::SOURCES,
            "status": { "read": law::STATUS_READ, "lines": law::STATUS },
            "notice": LEGAL_NOTICE,
        })
    );
}

/// Print the claims to recheck in `file`, repeating only the words of each pattern: the
/// text may hold personal details. Flags never change the exit status; text the check
/// cannot match is an error, so that it never reports no claims for it.
fn check_file(file: &Path) -> Result<(), String> {
    let failed = |why: String| format!("{why}\n{CHECK_LIMITS}\n{LEGAL_NOTICE}");
    let text = std::fs::read_to_string(file)
        .map_err(|e| failed(format!("cannot read {}: {e}", file.display())))?;
    if let Some(why) = check::refusal(&text) {
        return Err(failed(format!("cannot check {}: {why}", file.display())));
    }
    let flags = check::check(&text);
    for flag in &flags {
        println!("line {}: '{}': {}", flag.line, flag.found, flag.why);
    }
    println!("Claims to recheck: {}.", flags.len());
    println!("{CHECK_LIMITS}");
    println!("{LEGAL_NOTICE}");
    Ok(())
}

fn registry_command(path: Option<PathBuf>) -> ExitCode {
    match path {
        None => registry_check(&embedded::register(), "built-in register"),
        Some(dir) => match io::register::load(&dir) {
            Ok(files) => registry_check(&files, &dir.display().to_string()),
            Err(e) => {
                eprintln!("error: cannot read {}: {e}", dir.display());
                ExitCode::FAILURE
            }
        },
    }
}

fn registry_check(files: &RegisterFiles, place_name: &str) -> ExitCode {
    let today = chrono::Local::now().date_naive();
    let report = register::check(&files.entries, &files.packs, today);
    for finding in &report.findings {
        let label = match finding.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let place = if finding.file.is_empty() {
            place_name.to_owned()
        } else {
            format!("{place_name}/{}", finding.file)
        };
        eprintln!("{label}: {place}: {}", finding.message);
    }
    println!(
        "Checked {} entries and {} packs; errors: {}, warnings: {}.",
        report.entries,
        report.packs,
        report.errors(),
        report.warnings()
    );
    if report.errors() == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repeated_tag_is_retried() {
        let used = ["GS-ABCDE".to_owned()];
        let tag = unused_tag(&used, |attempt| {
            ["GS-ABCDE", "GS-FGHJK"][attempt as usize].to_owned()
        });
        assert_eq!(tag, "GS-FGHJK");
    }

    /// No register entry offers the Spam Act lever yet, so `status` meets its business days
    /// only here.
    #[test]
    fn status_names_each_kind_of_deadline_and_dates_only_those_with_a_day() {
        let day = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        assert_eq!(due_text(Due::Statutory(day)), "  law's last day 2026-11-16");
        assert_eq!(due_text(Due::Guide(day)), "  OAIC guide 2026-11-16");
        assert!(due_text(Due::Unknown).contains("5 business days after sending"));
        assert_eq!(due_text(Due::None), "");
        assert_eq!(due_date(Due::Statutory(day)), Some(day));
        assert_eq!(due_date(Due::Unknown), None);
        assert_eq!(due_date(Due::None), None);
    }

    #[test]
    fn named_windows_output_calls_for_a_folder_warning() {
        let mut output = Vec::new();
        warn_unchecked_out(false, true, &mut output);
        warn_unchecked_out(true, false, &mut output);
        assert!(output.is_empty());
        warn_unchecked_out(true, true, &mut output);
        let warning = String::from_utf8(output).unwrap();
        assert!(warning.contains("does not check who else can open the --out folder"));
    }

    #[test]
    fn draft_files_match_the_written_channel_and_never_use_phone() {
        let dir = tempfile::tempdir().unwrap();
        let drafts = dir.path().join("drafts");
        let register = built_in_register().unwrap();
        let entry = &register.entries["australia-post-au"];
        let phone = Letter {
            kind: ChannelKind::Phone,
            to: "0000000000".to_owned(),
            subject: "Test letter".to_owned(),
            body: "Test body".to_owned(),
        };
        let err = write_draft(&drafts, entry, "GS-01234", &phone, false).unwrap_err();
        assert_eq!(err, "plans never send letters by phone");
        assert!(!drafts.exists());

        for (kind, to, tag) in [
            (ChannelKind::Post, "PO Box 1", "GS-ABCDE"),
            (
                ChannelKind::WebForm,
                "https://example.invalid/form",
                "GS-FGHJK",
            ),
        ] {
            let letter = Letter {
                kind,
                to: to.to_owned(),
                subject: "Test letter".to_owned(),
                body: "Test body".to_owned(),
            };
            write_draft(&drafts, entry, tag, &letter, true).unwrap();
            let path = drafts.join(format!("australia-post-au-{tag}.txt"));
            assert_eq!(std::fs::read_to_string(path).unwrap(), "Test body\n");
        }
        let email = Letter {
            kind: ChannelKind::Email,
            to: "privacy@example.invalid".to_owned(),
            ..phone
        };
        write_draft(&drafts, entry, "GS-56789", &email, true).unwrap();
        assert!(drafts.join("australia-post-au-GS-56789.eml").is_file());
    }

    #[test]
    fn channel_and_deadline_labels_preserve_their_distinctions() {
        assert_eq!(ChannelKind::Phone.label(), "phone");
        assert_eq!(
            deadline_summary(Deadline::BusinessDays(5)),
            "5 business days"
        );
        assert_eq!(
            deadline_summary(Deadline::Reasonable { guide_days: None }),
            "a reasonable period"
        );
        assert_eq!(deadline_summary(Deadline::NotStated), "no time set");
    }
}
