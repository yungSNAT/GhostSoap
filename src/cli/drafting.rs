// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! `draft`: composing letters, writing them to the drafts folder and recording them.

use super::*;

/// How `draft` hands over its letters.
#[derive(Clone, Copy)]
pub(super) struct Output {
    /// Output format (`auto`, `eml`, `text` or `pdf`).
    pub(super) format: DraftFormat,
    /// Print them instead of writing files.
    pub(super) copy: bool,
    /// Also print mailto: links.
    pub(super) mailto: bool,
}

impl Output {
    /// Raw `.eml` to stdout: one message only, with all CLI notes routed to stderr.
    pub(super) fn is_raw_eml(self) -> bool {
        self.copy && self.format == DraftFormat::Eml
    }
}

fn emit_note(raw_eml: bool, line: std::fmt::Arguments) {
    if raw_eml {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
}

fn collect_planned<'a>(steps: &'a [Step<'a>], raw_eml: bool) -> Result<Vec<Planned<'a>>, String> {
    let mut planned = Vec::new();
    for step in steps {
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
            Step::Overseas { entry } if !raw_eml => print_overseas(entry),
            Step::Overseas { entry } => eprintln!("manual  {}: overseas entry", entry.id),
            Step::Skip { entry, reason } => {
                emit_note(raw_eml, format_args!("skip    {}: {reason}", entry.id))
            }
        }
    }
    if raw_eml && planned.len() > 1 {
        return Err(
            "`draft --format eml --copy` outputs one raw .eml message at a time; select a \
             single entry and channel"
                .to_owned(),
        );
    }
    Ok(planned)
}

fn report_draft_notes(drafted: &Drafted, raw_eml: bool) {
    let Drafted { entry, tag, .. } = drafted;
    for &field in &drafted.missing {
        emit_note(
            raw_eml,
            format_args!(
                "note    {}: your profile has no {}, so the letter leaves it out",
                entry.id,
                field_name(field)
            ),
        );
    }
    if let Some(earlier) = &drafted.replaces {
        let dropped: Vec<&str> = drafted.dropped.iter().map(|l| l.name()).collect();
        let dropped = match dropped.as_slice() {
            [] => String::new(),
            names => format!(" and no longer asks for {}", names.join(", ")),
        };
        if earlier == tag {
            emit_note(
                raw_eml,
                format_args!(
                    "note    {}: this draft replaces the earlier one under {tag}{dropped}",
                    entry.id
                ),
            );
        } else {
            emit_note(
                raw_eml,
                format_args!(
                    "note    {}: draft {tag} replaces {earlier}{dropped}; the earlier record stays as history",
                    entry.id
                ),
            );
        }
    }
}

pub(super) fn draft(
    selection: &Selection,
    out: Option<PathBuf>,
    output: Output,
    passphrase_stdin: bool,
) -> Result<(), String> {
    let register = built_in_register()?;
    let steps = plan::plan(&register, selection)?;
    let dir = drafts_dir(out.as_deref())?;
    warn_about_out(&dir, out.is_some(), output);
    let raw_eml = output.is_raw_eml();
    let planned = collect_planned(&steps, raw_eml)?;
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
        let format = output.format;
        let drafted = update_state(
            passphrase_stdin,
            |state| record_drafts(state, &planned, detail, folder.as_deref(), format, today()),
            |(_, redraft)| redraft.commit(),
            |(drafted, redraft), state| {
                publish_drafts(destination, drafted, output, redraft, state)
            },
        )?;
        let tags: Vec<&str> = drafted.iter().map(|d| d.tag.as_str()).collect();
        emit_note(
            raw_eml,
            format_args!(
                "After you send a letter, record it: ghostsoap sent {}",
                tags.join(" ")
            ),
        );
        if !output.copy {
            print_clean_reminder();
        }
    }
    if let Some(text) = access_warning_text(selection) {
        emit_note(raw_eml, format_args!("warning: {text}"));
    }
    Ok(())
}

pub(super) fn publish_drafts<'a>(
    dir: &Path,
    drafted: Vec<Drafted<'a>>,
    output: Output,
    redraft: io::drafts::Redraft,
    state: &mut State,
) -> Result<Vec<Drafted<'a>>, String> {
    let hand = |letter: &Drafted| hand_over(dir, letter, output);
    publish_with(drafted, redraft, output.copy, state, hand)
}

/// `publish_drafts` with the step that hands each letter over passed in, so a test can make
/// one fail part-way through a run.
pub(super) fn publish_with<'a>(
    drafted: Vec<Drafted<'a>>,
    mut redraft: io::drafts::Redraft,
    copy: bool,
    state: &mut State,
    mut hand: impl FnMut(&Drafted) -> Result<(), String>,
) -> Result<Vec<Drafted<'a>>, String> {
    redraft.unlock();
    for (at, letter) in drafted.iter().enumerate() {
        // The letters written before a failure are plaintext on disk (R1946).
        hand(letter).map_err(|e| match at {
            0 => e,
            _ if copy => e,
            _ => format!(
                "{e}\nThe letters written before it are plaintext drafts. Once you no longer \
                 need them, run `ghostsoap clean`."
            ),
        })?;
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
pub(super) struct Planned<'a> {
    pub(super) entry: &'a Entry,
    pub(super) channel: &'a Channel,
    pub(super) levers: &'a [Lever],
}

/// A letter the state has recorded, ready to write or show.
pub(super) struct Drafted<'a> {
    pub(super) entry: &'a Entry,
    pub(super) tag: String,
    pub(super) letter: Letter,
    /// The entry's identity fields the profile has no value for.
    pub(super) missing: Vec<IdentityField>,
    /// The levers an earlier draft held and this one drops.
    pub(super) dropped: Vec<Lever>,
    pub(super) replaces: Option<String>,
}

/// Record each planned letter as drafted and return what to hand over. A plain draft to the
/// same entry and channel is replaced under its tag, after its file is taken out of the
/// folder it was written to. A draft with history keeps its record under the old tag.
pub(super) fn record_drafts<'a>(
    state: &mut State,
    planned: &[Planned<'a>],
    detail: Option<&str>,
    folder: Option<&str>,
    format: DraftFormat,
    today: Date,
) -> Result<(Vec<Drafted<'a>>, io::drafts::Redraft), String> {
    // Every letter is composed first: one that cannot be written changes nothing.
    let drafted = compose_drafts(state, planned, detail, today)?;
    for letter in drafted.iter().filter(|d| uses_pdf(d.letter.kind, format)) {
        letters::pdf(&letter.letter).map_err(|c| {
            format!(
                "the letter to {} holds '{c}' (U+{:04X}), which a PDF's standard font cannot                  show; draft it with --format text instead",
                letter.entry.id, c as u32
            )
        })?;
    }
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
                name: draft_name(plan.entry, &letter.tag, plan.channel.kind, format),
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
pub(super) fn compose_drafts<'a>(
    state: &State,
    planned: &[Planned<'a>],
    detail: Option<&str>,
    today: Date,
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
pub(super) fn stage_earlier_drafts(
    state: &State,
    drafted: &[Drafted],
) -> Result<io::drafts::Redraft, String> {
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
pub(super) fn hand_over(dir: &Path, drafted: &Drafted, output: Output) -> Result<(), String> {
    let Drafted {
        entry, tag, letter, ..
    } = drafted;
    report_draft_notes(drafted, output.is_raw_eml());
    if output.copy {
        if output.format == DraftFormat::Eml {
            write_stdout(letters::eml(letter).as_bytes());
        } else {
            print_copy(entry, letter);
        }
        return Ok(());
    }
    write_draft(dir, entry, tag, letter, output).map_err(|e| {
        format!(
            "{e}; {tag} and the other letters of this run are recorded as unfinished, so `sent` \
             refuses them until you draft them again. If GhostSoap cannot confirm a file it \
             left, move that file yourself first"
        )
    })
}

/// Resolve the destination before saving it, so a later change to an ancestor link cannot hide
/// an earlier draft. On Unix the final component must still pass the private-folder check.
pub(super) fn draft_destination(dir: &Path) -> Result<String, String> {
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

pub(super) fn print_clean_reminder() {
    println!(
        "Drafts are plaintext. After sending them from your own mailbox, run `ghostsoap clean`."
    );
}

/// Warnings about the folder drafts go to, `named` with `--out`; none with `--copy`, which
/// writes nothing.
pub(super) fn warn_about_out(dir: &Path, named: bool, output: Output) {
    if !output.copy {
        warn_unchecked_out(named, cfg!(windows), std::io::stderr());
        warn_exposed(dir, DRAFTS_FOLDER);
    }
}

/// On Windows the drafts folder keeps the access list it inherits, which GhostSoap does not
/// check; say so when the user names the folder (`named`).
pub(super) fn warn_unchecked_out(named: bool, windows: bool, mut output: impl std::io::Write) {
    if windows && named {
        writeln!(
            output,
            "note: on Windows GhostSoap does not check who else can open the --out folder; \
             choose one only you can open"
        )
        .expect("write the drafts folder warning");
    }
}

/// A tag no message in the state and no letter in this run has: `used` holds both, as 5
/// Crockford base32 characters give only 33,554,432 values. `RandomState` is seeded randomly
/// per process, so hashing the time gives unpredictable bits without another dependency;
/// tags only need to be distinct, not secret.
pub(super) fn new_tag(used: &[String]) -> String {
    let state = RandomState::new();
    unused_tag(used, |n| {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        letters::tag(state.hash_one((now, n)))
    })
}

pub(super) fn unused_tag(used: &[String], candidate: impl Fn(u64) -> String) -> String {
    let mut n = 0u64;
    loop {
        let tag = candidate(n);
        if !used.contains(&tag) {
            return tag;
        }
        n += 1;
    }
}

fn uses_eml(kind: ChannelKind, format: DraftFormat) -> bool {
    match format {
        DraftFormat::Auto | DraftFormat::Pdf => kind == ChannelKind::Email,
        DraftFormat::Eml => true,
        DraftFormat::Text => false,
    }
}

fn uses_pdf(kind: ChannelKind, format: DraftFormat) -> bool {
    format == DraftFormat::Pdf && kind == ChannelKind::Post
}

pub(super) fn write_draft(
    dir: &Path,
    entry: &Entry,
    tag: &str,
    letter: &Letter,
    output: Output,
) -> Result<(), String> {
    let how = match letter.kind {
        ChannelKind::Email => format!("email to {}", letter.to),
        ChannelKind::Post => format!("print and post to {}", letter.to),
        ChannelKind::WebForm => format!("paste into the form at {}", letter.to),
        ChannelKind::Phone => return Err("plans never send letters by phone".to_owned()),
    };
    let text = if uses_eml(letter.kind, output.format) {
        letters::eml(letter)
    } else if uses_pdf(letter.kind, output.format) {
        // `record_drafts` refused any letter the font cannot show.
        letters::pdf(letter).map_err(|c| format!("cannot write U+{:04X} to a PDF", c as u32))?
    } else {
        format!("{}\n", letter.body)
    };
    let name = draft_name(entry, tag, letter.kind, output.format);
    let path = io::drafts::write(dir, &name, &text).map_err(|e| e.to_string())?;
    println!(
        "draft   {}  {how}: {}",
        entry.id,
        io::exposure::display(&path)
    );
    if output.mailto && letter.kind == ChannelKind::Email {
        match letters::mailto(letter) {
            Some(link) => println!("        or open: {link}"),
            None => println!("        too long for a mailto: link; open the draft instead"),
        }
    }
    Ok(())
}

/// The file a draft is written to, matching the channel and chosen `--format`.
pub(super) fn draft_name(
    entry: &Entry,
    tag: &str,
    kind: ChannelKind,
    format: DraftFormat,
) -> String {
    let extension = if uses_eml(kind, format) {
        "eml"
    } else if uses_pdf(kind, format) {
        "pdf"
    } else {
        "txt"
    };
    format!("{}-{tag}.{extension}", entry.id)
}

pub(super) fn print_copy(entry: &Entry, letter: &Letter) {
    println!("==== {} ({})", entry.id, letter.kind.label());
    println!("To: {}", letter.to);
    println!("Subject: {}", letter.subject);
    println!();
    println!("{}", letter.body);
    println!();
}

/// The identity fields `entry` takes that `profile` has no value for.
pub(super) fn missing_fields(entry: &Entry, profile: &Profile) -> Vec<IdentityField> {
    let fields = entry.identity_fields.iter().copied();
    fields
        .filter(|&field| profile.value(field).is_none())
        .collect()
}
