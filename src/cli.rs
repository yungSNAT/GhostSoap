// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Command-line interface.

use std::hash::{BuildHasher, RandomState};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::SystemTime;

use clap::{Args, CommandFactory, Parser, Subcommand};
use jiff::civil::Date;
use serde_json::{Value, json};

use crate::core::check;
use crate::core::checklist::{self, MarkKind};
use crate::core::deadline::{self, Due};
use crate::core::embedded;
use crate::core::law::{self, Deadline};
use crate::core::letters::{self, Letter, Request};
use crate::core::named;
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

mod args;
mod auditing;
mod cleanup;
mod demo;
mod drafting;
mod planning;
mod records;
mod scanning;

pub use args::*;
use auditing::*;
use cleanup::*;
use demo::*;
use drafting::*;
use planning::*;
use records::*;
use scanning::*;

/// Write bytes to standard output, ignoring only a reader that has gone.
fn write_stdout(bytes: &[u8]) {
    use std::io::Write;
    if let Err(e) = std::io::stdout().lock().write_all(bytes) {
        assert_eq!(
            e.kind(),
            std::io::ErrorKind::BrokenPipe,
            "failed printing to stdout: {e}"
        );
    }
}

/// Write one line to standard output, ignoring only a reader that has gone.
fn print_line(line: std::fmt::Arguments) {
    write_stdout(format!("{line}\n").as_bytes());
}

/// Write a shell completion script to standard output.
fn print_completions(shell: clap_complete::Shell) {
    let mut script = Vec::new();
    clap_complete::generate(shell, &mut Cli::command(), "ghostsoap", &mut script);
    write_stdout(&script);
}

const LICENCE_NOTICE: &str = "Copyright 2026 Ryan Duguid. No warranty.\n\
    AGPL-3.0-or-later with additional terms (see NOTICE).\n\
    Licence: https://github.com/yungSNAT/GhostSoap/blob/main/LICENSE";
const LEGAL_NOTICE: &str = "GhostSoap provides information and templates, not legal advice.";
const CHECK_LIMITS: &str = "This check matches words. It flags a sentence that holds a word for \
                            erasing and a legal source or a word of duty, so a flag may be \
                            harmless. It also flags the GDPR, California's privacy law, foreign \
                            article numbers and time periods written as a number and a unit, \
                            such as '30 days' or 'a month'. It passes the law table's own \
                            wording wherever it appears, even where that row does not apply to \
                            the business you are writing to. It looks for nothing else, so no \
                            flag does not mean the text is accurate, complete or legally sound.";
const OVERSEAS_NOTE: &str = "run from overseas; Australian privacy law can reach an overseas \
                             organisation that carries on business in Australia (Privacy Act \
                             section 5B(1A) and (3)), but GhostSoap has not established that it \
                             reaches this one, so it drafts no letter.";

/// Prepare Australian Privacy Act requests to data brokers and track the replies.
#[derive(Debug, Parser)]
#[command(name = "ghostsoap", version, about, long_about = None)]
#[command(after_help = LICENCE_NOTICE)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
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
        Some(Command::Scan(args)) => scan(&args),
        Some(Command::Draft {
            selection,
            out,
            format,
            copy,
            mailto,
            passphrase_stdin,
        }) => draft(
            &selection.selection(),
            out,
            Output {
                format,
                copy,
                mailto,
            },
            passphrase_stdin,
        ),
        Some(Command::Export {
            out,
            escalation,
            passphrase_stdin,
        }) => export(out, escalation, passphrase_stdin),
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
        Some(Command::Checklist(args)) => checklist(&args),
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
        Some(Command::Demo(args)) => run_demo(&args),
        Some(Command::Audit(args)) => audit_command(&args),
        Some(Command::Completions { shell }) => {
            print_completions(shell);
            Ok(())
        }
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

fn today() -> Date {
    jiff::ZonedDateTime::now().date()
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
    let report = register::check(&files.entries, &files.packs, today());
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
mod tests;
