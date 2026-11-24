// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! The command line's arguments and their parsers.

use super::*;

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
    /// List the pages where people-search sites and directories show what they hold, for you
    /// to search yourself; --open opens them in your browser.
    ///
    /// Without --pack or --entry it lists every people-search site and directory in the
    /// register that has such a page. GhostSoap sends nothing to these sites, fills in no form
    /// and records nothing.
    Scan(ScanArgs),
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
        /// Output format: `auto` writes `.eml` for email and `.txt` for post/web form,
        /// `eml` writes RFC 5322 `.eml` drafts (with `X-Unsent: 1`), and `text` writes
        /// plain text.
        #[arg(long, value_enum, default_value_t = DraftFormat::Auto)]
        format: DraftFormat,
        /// Print each letter's To, Subject and body (or raw `.eml` with `--format eml`)
        /// instead of writing files.
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
    /// Write your profile and every letter's record to a new plaintext file alongside a
    /// SHA-256 manifest, or write an OAIC escalation bundle for one tag with `--escalation`.
    Export {
        /// The file (or directory with `--escalation`) to write; it must not exist yet.
        /// Defaults to `ghostsoap-export-<date>.toml` in GhostSoap's data folder.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Export a self-contained OAIC escalation bundle (summary, record and SHA-256
        /// manifest) for one request tag, such as GS-7K3QX.
        #[arg(long)]
        escalation: Option<String>,
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
    /// The data hygiene checklist: steps beyond letters, such as breach checks, credit bans
    /// and the Do Not Call Register, each linked to its official page.
    ///
    /// It lists every item with your progress, kept in your encrypted state. Mark an item
    /// with `done`, `not-for-me` or `clear`. Nothing is submitted for you.
    Checklist(ChecklistArgs),
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
    /// Run a zero-PII synthetic dry-run demo (status, checklist, audit chain and HTML
    /// preview) using example.com fixtures without touching your vault.
    Demo(DemoArgs),
    /// Verify and inspect the tamper-evident SHA-256 OAIC audit chain and export manifests.
    Audit(AuditArgs),
    /// Generate shell completions for bash, elvish, fish, powershell or zsh.
    Completions {
        /// Shell to generate completions for.
        shell: clap_complete::Shell,
    },
}

/// Output format for `ghostsoap draft`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum DraftFormat {
    /// `.eml` for email channels and `.txt` for post or web form.
    #[default]
    Auto,
    /// RFC 5322 `.eml` with `X-Unsent: 1` and header sanitisation.
    Eml,
    /// Plain text (`.txt`).
    Text,
}

/// Output format for `ghostsoap demo`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum DemoFormat {
    #[default]
    Text,
    Html,
}

#[derive(Debug, Args)]
pub struct DemoArgs {
    /// Print terminal text or self-contained HTML to standard output.
    #[arg(long, value_enum, default_value_t = DemoFormat::Text)]
    pub(super) format: DemoFormat,
    /// Write the self-contained HTML preview to a new file instead of standard output.
    #[arg(long)]
    pub(super) html: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct AuditArgs {
    /// Verify the audit chain in a plaintext export file (and its companion `.sha256`
    /// manifest if present) without unlocking the vault.
    #[arg(long, conflicts_with = "manifest")]
    pub(super) export: Option<PathBuf>,
    /// Verify every SHA-256 digest listed in an export or escalation manifest file.
    #[arg(long, conflicts_with = "export")]
    pub(super) manifest: Option<PathBuf>,
    /// Print every entry in the verified audit chain.
    #[arg(long)]
    pub(super) list: bool,
    /// Print JSON for scripts and AI agents.
    #[arg(long)]
    pub(super) json: bool,
    /// Read the passphrase from standard input instead of the terminal (for scripts).
    #[arg(long, conflicts_with_all = ["export", "manifest"])]
    pub(super) passphrase_stdin: bool,
}

/// Which entities and requests.
#[derive(Debug, Args)]
pub struct SentArgs {
    /// The tag in the letter's subject line, such as GS-7K3QX; repeat for more.
    #[arg(required = true)]
    pub(super) tags: Vec<String>,
    /// The day you sent it, as YYYY-MM-DD; today if left out.
    #[arg(long, value_parser = parse_date)]
    pub(super) on: Option<Date>,
    /// The channel you used, if not the one the letter was drafted for: email, web-form,
    /// post or phone.
    #[arg(long, value_parser = parse_channel)]
    pub(super) channel: Option<ChannelKind>,
    /// Replace an earlier record of sending that was wrong.
    #[arg(long)]
    pub(super) correct: bool,
    /// Take back the record of sending: the letter was not sent. Nothing is deleted, and
    /// the letter counts as a draft again.
    #[arg(long, conflicts_with_all = ["on", "channel", "correct"])]
    pub(super) withdraw: bool,
    /// Why you are withdrawing the record, in your own words, kept with it.
    #[arg(long, requires = "withdraw")]
    pub(super) note: Option<String>,
    /// Read the passphrase from standard input instead of the terminal (for scripts).
    #[arg(long)]
    pub(super) passphrase_stdin: bool,
}

#[derive(Debug, Args)]
pub struct ReplyArgs {
    /// The tag in the letter's subject line, such as GS-7K3QX.
    pub(super) tag: String,
    /// What happened: acknowledged, id-requested, charge-quoted, fulfilled, partial,
    /// suppressed, no-record, refused-with-reasons, refused, exempt-claimed, bounced,
    /// relisted, escalated, closed (finished, with an answer) or abandoned (given up).
    #[arg(value_parser = parse_status, required_unless_present_any = ["agreed_until", "withdraw"])]
    pub(super) status: Option<tracker::Status>,
    /// The request it applies to; repeat for more. All of the letter's requests if left out.
    #[arg(long, value_parser = parse_lever)]
    pub(super) lever: Vec<Lever>,
    /// The day it happened, as YYYY-MM-DD; today if left out.
    #[arg(long, value_parser = parse_date)]
    pub(super) on: Option<Date>,
    /// The ground given for a refusal: not-direct-marketing, small-business,
    /// outside-jurisdiction, insufficient-verification or other.
    #[arg(long, value_parser = parse_reason, requires = "status")]
    pub(super) reason: Option<tracker::Reason>,
    /// A source of your information the answer names, in your own words (APP 7.6(e)).
    #[arg(long, requires = "status")]
    pub(super) source: Option<String>,
    /// Your own words, kept with the record.
    #[arg(long)]
    pub(super) note: Option<String>,
    /// The last day of a longer period you agreed to in writing for a credit correction
    /// (Privacy Act ss 20T(2)(b), 21V(2)(b)).
    #[arg(long, value_parser = parse_date, conflicts_with = "status")]
    pub(super) agreed_until: Option<Date>,
    /// Take back the latest answer (`status`) or longer period (`extension`) recorded: it
    /// was wrong. It stays as history.
    #[arg(long, value_parser = parse_withdrawn, conflicts_with_all = ["status", "agreed_until", "on"])]
    pub(super) withdraw: Option<tracker::Withdrawn>,
    /// Read the passphrase from standard input instead of the terminal (for scripts).
    #[arg(long)]
    pub(super) passphrase_stdin: bool,
}

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct ChecklistArgs {
    #[command(subcommand)]
    pub(super) command: Option<ChecklistCommand>,
    /// Show one item in full: the first to check again, else the first to do.
    #[arg(long)]
    pub(super) next: bool,
    /// Leave out your progress, so no passphrase is asked for.
    #[arg(long, conflicts_with = "passphrase_stdin")]
    pub(super) no_passphrase: bool,
    /// Read the passphrase from standard input instead of the terminal (for scripts).
    #[arg(long)]
    pub(super) passphrase_stdin: bool,
}

#[derive(Debug, Subcommand)]
pub enum ChecklistCommand {
    /// Mark items done.
    Done {
        #[command(flatten)]
        marks: MarkArgs,
        /// The day you did it, as YYYY-MM-DD; today if left out.
        #[arg(long, value_parser = parse_date)]
        on: Option<Date>,
    },
    /// Mark items that do not apply to you.
    NotForMe(MarkArgs),
    /// Remove your mark from items, so they are to do again.
    Clear(MarkArgs),
}

#[derive(Debug, Args)]
pub struct MarkArgs {
    /// The item's id, as `ghostsoap checklist` lists it; repeat for more.
    #[arg(required = true)]
    pub(super) items: Vec<String>,
    /// Read the passphrase from standard input instead of the terminal (for scripts).
    #[arg(long)]
    pub(super) passphrase_stdin: bool,
}

#[derive(Debug, Args)]
pub struct ScanArgs {
    /// A pack of entities, such as `marketing`; repeat for more.
    #[arg(long)]
    pub(super) pack: Vec<String>,
    /// One entity by id, such as `infotracer-au`; repeat for more.
    #[arg(long)]
    pub(super) entry: Vec<String>,
    /// Open each page in your default browser.
    #[arg(long)]
    pub(super) open: bool,
}

#[derive(Debug, Args)]
pub struct SelectionArgs {
    /// A pack of entities, such as `start-here`; repeat for more.
    #[arg(long)]
    pub(super) pack: Vec<String>,
    /// One entity by id, such as `quantium-au`; repeat for more.
    #[arg(long)]
    pub(super) entry: Vec<String>,
    /// A request to make instead of the default APP 7 requests; repeat for more. Access,
    /// correction and ban requests need a single --entry.
    #[arg(long, value_parser = parse_lever)]
    pub(super) lever: Vec<Lever>,
    /// Your own words for levers that need them, such as what is wrong for a correction.
    #[arg(long)]
    pub(super) detail: Option<String>,
}

impl SelectionArgs {
    pub(super) fn selection(&self) -> Selection {
        Selection {
            packs: self.pack.clone(),
            entries: self.entry.clone(),
            levers: self.lever.clone(),
            detail: self.detail.clone(),
        }
    }
}

pub(super) fn parse_lever(name: &str) -> Result<Lever, String> {
    serde_json::from_value(json!(name)).map_err(|_| {
        let names = Lever::ALL.map(Lever::name).join(", ");
        format!("unknown lever; use one of {names}")
    })
}

pub(super) fn parse_date(text: &str) -> Result<Date, String> {
    Date::strptime("%Y-%m-%d", text).map_err(|_| "use a date such as 2026-11-30".to_owned())
}

/// A name as the state file writes it, such as `refused-with-reasons`.
pub(super) fn parse_named<T: serde::de::DeserializeOwned>(
    name: &str,
    names: &str,
) -> Result<T, String> {
    serde_json::from_value(json!(name)).map_err(|_| format!("use {names}"))
}

pub(super) fn parse_status(name: &str) -> Result<tracker::Status, String> {
    parse_named(name, "a status `ghostsoap reply --help` lists").and_then(|status| match status {
        tracker::Status::Drafted | tracker::Status::Sent => {
            Err("`draft` and `sent` record those".to_owned())
        }
        status => Ok(status),
    })
}

pub(super) fn parse_reason(name: &str) -> Result<tracker::Reason, String> {
    parse_named(
        name,
        "not-direct-marketing, small-business, outside-jurisdiction, insufficient-verification or other",
    )
}

pub(super) fn parse_withdrawn(name: &str) -> Result<tracker::Withdrawn, String> {
    parse_named(name, "status or extension")
}

pub(super) fn parse_channel(name: &str) -> Result<ChannelKind, String> {
    parse_named(name, "email, web-form, post or phone")
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
