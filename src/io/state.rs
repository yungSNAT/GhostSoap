// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! The encrypted state file (plan section 5, 'Storage and secrets'). Format 1 (M2) held the
//! profile alone; format 2 (M3a) adds the messages, their requests and events. A format 1
//! file is read as a format 2 state with no messages, and is replaced only by the first
//! command that changes something.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use age::secrecy::SecretString;
use serde::Deserialize;

use crate::core::audit::{self, AuditSummary, MANIFEST_HEADER};
use crate::core::law::{self, Deadline};
use crate::core::profile::Profile;
use crate::core::register::Lever;
use crate::core::sha256;
use crate::core::tracker::FORMAT;
pub use crate::core::tracker::State;
use crate::io::bundle;

const STATE_FILE: &str = "state.age";
const LOCK_FILE: &str = "state.lock";
/// How an export begins, before its date.
const EXPORT_HEADER: &str = "# GhostSoap export of ";
/// The highest scrypt work factor (2^22) a state file may ask for: the reference age
/// implementation's default, about 15 seconds and 4 GiB on a modern machine.
const MAX_WORK_FACTOR: u8 = 22;
/// The scrypt work factor (2^18) a saved state asks for: the reference age implementation's
/// default, about a second on a modern machine. Fixed rather than timed, so a fast machine
/// never writes a state that `MAX_WORK_FACTOR` refuses.
const WORK_FACTOR: u8 = 18;
const _: () = assert!(WORK_FACTOR <= MAX_WORK_FACTOR);

/// State format 1, as M2 wrote it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Format1 {
    #[allow(dead_code)] // read only to refuse an unknown field beside it
    format: u32,
    profile: Profile,
}

/// Where the state lives: `GHOSTSOAP_HOME` if set, else the platform data directory, never
/// a folder a sync service such as OneDrive is likely to copy.
pub fn data_dir() -> io::Result<PathBuf> {
    data_dir_for(env::consts::OS, |name| env::var_os(name))
}

fn data_dir_for(os: &str, var: impl Fn(&str) -> Option<std::ffi::OsString>) -> io::Result<PathBuf> {
    let var = |name: &str| var(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(home) = var("GHOSTSOAP_HOME") {
        return Ok(home);
    }
    let dir = match os {
        "windows" => var("LOCALAPPDATA").map(|d| d.join("GhostSoap")),
        "macos" => var("HOME").map(|d| d.join("Library/Application Support/GhostSoap")),
        _ => var("XDG_DATA_HOME")
            .or_else(|| var("HOME").map(|h| h.join(".local/share")))
            .map(|d| d.join("ghostsoap")),
    };
    dir.ok_or_else(|| io::Error::other("cannot find a data directory; set GHOSTSOAP_HOME"))
}

pub fn state_path(dir: &Path) -> PathBuf {
    dir.join(STATE_FILE)
}

/// The state as the text that is encrypted. Refuses text that would not read back as the
/// same state, so a state file is never replaced by one GhostSoap cannot read.
fn encode(state: &State) -> io::Result<String> {
    encode_checked(state, |text| toml::from_str(text).ok())
}

/// `encode`, reading the text back with `read_back`.
fn encode_checked(
    state: &State,
    read_back: impl FnOnce(&str) -> Option<State>,
) -> io::Result<String> {
    let unsaved = |why: &str| io::Error::other(format!("{why}; nothing was saved"));
    if let Some(problem) = state.problems().first() {
        return Err(unsaved(problem));
    }
    let text = toml::to_string(state).map_err(io::Error::other)?;
    match read_back(&text) {
        Some(back) if back == *state => Ok(text),
        _ => Err(unsaved("the state would not read back as it was written")),
    }
}

/// Read decrypted state text of any format this version knows. An error gives the line at
/// fault and never quotes the text, which holds the profile.
fn decode(text: &str) -> io::Result<State> {
    let unreadable = |e: toml::de::Error| {
        let place = e.span().map_or_else(String::new, |span| {
            let line = text[..span.start.min(text.len())].matches('\n').count() + 1;
            format!(" at line {line} of its text")
        });
        io::Error::other(format!(
            "cannot read the state: it is not as expected{place}"
        ))
    };
    let table: toml::Table = toml::from_str(text).map_err(unreadable)?;
    match table.get("format").and_then(toml::Value::as_integer) {
        Some(1) => {
            let old: Format1 = toml::from_str(text).map_err(unreadable)?;
            Ok(State::new(old.profile))
        }
        Some(2) => {
            let mut table = table;
            // Parsing the text, not the table, keeps the line of an error.
            let state: State = if rename_days(&mut table) {
                table.try_into().map_err(unreadable)?
            } else {
                toml::from_str(text).map_err(unreadable)?
            };
            match state.problems().first() {
                None => Ok(state),
                Some(problem) => Err(io::Error::other(format!(
                    "cannot read the state: {problem}"
                ))),
            }
        }
        Some(newer) if newer > i64::from(FORMAT) => Err(io::Error::other(format!(
            "state format {newer} is newer than this version of GhostSoap reads (format \
             {FORMAT}); upgrade GhostSoap"
        ))),
        _ => Err(io::Error::other(
            "cannot read the state: it has no format this version knows",
        )),
    }
}

/// Format 2 as first merged (main at fffb4d1) wrote both fixed periods as `days`. Rename each
/// to the kind its lever's law gives, so the typed read sees only the kinds' own names and
/// refuses a kind written by its name for a lever whose law does not give it. Whether any
/// was renamed.
pub(crate) fn rename_days(table: &mut toml::Table) -> bool {
    let mut renamed = false;
    let messages = table
        .get_mut("messages")
        .and_then(toml::Value::as_array_mut);
    let requests = messages
        .into_iter()
        .flatten()
        .filter_map(|message| message.get_mut("requests")?.as_array_mut())
        .flatten()
        .filter_map(toml::Value::as_table_mut);
    for request in requests {
        let lever: Option<Lever> = request.get("lever").and_then(|v| v.clone().try_into().ok());
        let deadline = request
            .get_mut("deadline")
            .and_then(toml::Value::as_table_mut);
        if let (Some(lever), Some(deadline)) = (lever, deadline)
            && let Some(days) = deadline.remove("days")
        {
            let kind = match law::law(lever).deadline {
                Deadline::DaysAfter(_) => "days-after",
                _ => "days-starting",
            };
            deadline.insert(kind.to_owned(), days);
            renamed = true;
        }
    }
    renamed
}

fn encrypt(state: &State, passphrase: SecretString) -> io::Result<Vec<u8>> {
    let text = encode(state)?;
    let mut recipient = age::scrypt::Recipient::new(passphrase);
    recipient.set_work_factor(WORK_FACTOR);
    age::encrypt(&recipient, text.as_bytes()).map_err(io::Error::other)
}

/// Encrypt and write a new state file. Refuses to replace an existing one.
pub fn create(dir: &Path, state: &State, passphrase: SecretString) -> io::Result<()> {
    let path = state_path(dir);
    let bytes = encrypt(state, passphrase)?;
    create_private_dir(dir)?;
    // Checked under the lock, so 2 `init` runs cannot both see no file and the second replace
    // the first.
    let _lock = Lock::acquire(dir, LOCK_FILE)?;
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", path.display()),
        ));
    }
    write_atomic(dir, &bytes, || {})
}

/// The state as `export` writes it: the text the state file encrypts, after a comment line
/// dated `date` saying it is plaintext. `decode` reads it back as the same state.
fn export_text(state: &State, date: &str) -> io::Result<String> {
    Ok(format!(
        "{EXPORT_HEADER}{date}. This file is plaintext: it holds your profile and every \
         letter's record. Keep it private and delete it when you no longer need it.\n{}",
        encode(state)?
    ))
}

/// Paths and digests written by `export`.
#[derive(Debug, Clone)]
pub struct ExportReceipt {
    pub manifest: PathBuf,
    pub sha256: String,
    pub audit: AuditSummary,
}

/// Write the state, read with `passphrase`, to a new file at `path` as plaintext (mode 600
/// on Unix) alongside a companion SHA-256 manifest `<path>.sha256`. A file or link already
/// at either destination is refused, never replaced.
pub fn export(
    dir: &Path,
    passphrase: SecretString,
    path: &Path,
    date: &str,
) -> io::Result<ExportReceipt> {
    let name = bundle::validate_export_basename(path)?;
    let state = load(dir, passphrase)?;
    let audit = audit::verify_chain(&state.audit).map_err(io::Error::other)?;
    let text = export_text(&state, date)?;
    let digest = sha256(text.as_bytes());
    let manifest = bundle::manifest_path(path);
    if fs::symlink_metadata(&manifest).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "{} already exists; nothing was exported",
                manifest.display()
            ),
        ));
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path).map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => io::Error::new(
            e.kind(),
            format!("{} already exists; nothing was exported", path.display()),
        ),
        _ => e,
    })?;
    let written = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all());
    drop(file);
    written.map_err(|e| discard(path, &e))?;

    let manifest_text = audit::format_manifest(date, &audit, &[(&name, &digest)]);
    bundle::write_new_file(&manifest, manifest_text.as_bytes(), true)
        .map_err(|e| discard(path, &e))?;
    Ok(ExportReceipt {
        manifest,
        sha256: digest,
        audit,
    })
}

/// The error for an export whose write failed, after deleting the plaintext it left, so
/// none is left unreported.
fn discard(path: &Path, error: &io::Error) -> io::Error {
    let left = match fs::remove_file(path) {
        Ok(()) => "was deleted".to_owned(),
        Err(e) => format!("could not be deleted ({e}); it is plaintext, so delete it yourself"),
    };
    let export = path.display();
    io::Error::new(
        error.kind(),
        format!("{error}; the part-written export at {export} {left}"),
    )
}

/// Whether `path` is an export GhostSoap wrote under its default name: a regular file (never
/// a link, pipe or folder), dated, and starting with the line `export` writes. Only that
/// line's first bytes are read.
fn has_dated_prefix_and_header(path: &Path, suffix: &str, header: &str) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let dated = name
        .strip_prefix("ghostsoap-export-")
        .and_then(|rest| rest.strip_suffix(suffix))
        .is_some_and(|date| jiff::civil::Date::strptime("%Y-%m-%d", date).is_ok());
    if !dated || !fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file()) {
        return false;
    }
    let mut start = Vec::new();
    let bytes = header.as_bytes();
    fs::File::open(path)
        .and_then(|file| file.take(bytes.len() as u64).read_to_end(&mut start))
        .is_ok_and(|_| start == bytes)
}

fn is_export(path: &Path) -> bool {
    has_dated_prefix_and_header(path, ".toml", EXPORT_HEADER)
}

fn is_export_manifest(path: &Path) -> bool {
    has_dated_prefix_and_header(path, ".toml.sha256", MANIFEST_HEADER)
}

/// The files in the data folder `forget` deletes: the state file, a temporary file an
/// interrupted write left, and exports written under their default name. A folder that does
/// not exist holds none.
pub fn own_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let items = match fs::read_dir(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        items => items?,
    };
    let state = state_path(dir);
    let tmp = state.with_extension("age.tmp");
    let mut files = Vec::new();
    for item in items {
        let path = item?.path();
        if path == state || path == tmp || is_export(&path) || is_export_manifest(&path) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Take the state's lock, so no other GhostSoap writes the state whilst it is held.
pub(crate) fn lock(dir: &Path) -> io::Result<Lock> {
    Lock::acquire(dir, LOCK_FILE)
}

/// Read and decrypt the state file. Nothing is written: a format 1 file stays as it is.
pub fn load(dir: &Path, passphrase: SecretString) -> io::Result<State> {
    read(&state_path(dir), passphrase)
}

/// Change the state: one load, change, encrypt and replace under the state lock, so 2
/// GhostSoap processes cannot lose each other's changes. If `change` fails or changes
/// nothing, the file is left exactly as it was, whatever its format; otherwise the new state
/// replaces it whole, in the current format.
pub fn update<T>(
    dir: &Path,
    passphrase: SecretString,
    change: impl FnOnce(&mut State) -> Result<T, String>,
) -> io::Result<T> {
    update_then(dir, passphrase, change, |_| {}, |value, _| Ok(value))
}

/// Commit a state change before handing over its result, whilst still excluding other writers.
/// `committed` runs at the atomic replacement, even if the subsequent directory flush
/// fails. `after_save` may change the state again, such as to record that the handover
/// finished; that change is saved only if `after_save` succeeds. An error in `after_save`
/// leaves the committed state in place.
pub(crate) fn update_then<T, U>(
    dir: &Path,
    passphrase: SecretString,
    change: impl FnOnce(&mut State) -> Result<T, String>,
    committed: impl FnOnce(&mut T),
    after_save: impl FnOnce(T, &mut State) -> Result<U, String>,
) -> io::Result<U> {
    let path = state_path(dir);
    // The lock lives in the data folder; without one there is no state to change.
    if !dir.is_dir() {
        return Err(missing(&path, io::ErrorKind::NotFound));
    }
    let _lock = Lock::acquire(dir, LOCK_FILE)?;
    let before = read(&path, passphrase.clone())?;
    let mut state = before.clone();
    let mut value = change(&mut state).map_err(io::Error::other)?;
    if state != before {
        let bytes = encrypt(&state, passphrase.clone())?;
        write_atomic(dir, &bytes, || committed(&mut value))?;
    } else {
        committed(&mut value);
    }
    let saved = state.clone();
    let result = after_save(value, &mut state).map_err(io::Error::other)?;
    if state != saved {
        let finished =
            encrypt(&state, passphrase).and_then(|bytes| write_atomic(dir, &bytes, || {}));
        finished.map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("could not record that the handover finished: {e}"),
            )
        })?;
    }
    Ok(result)
}

fn missing(path: &Path, kind: io::ErrorKind) -> io::Error {
    io::Error::new(
        kind,
        format!(
            "no profile at {}; run `ghostsoap init` first",
            path.display()
        ),
    )
}

fn read(path: &Path, passphrase: SecretString) -> io::Result<State> {
    let bytes = fs::read(path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => missing(path, e.kind()),
        _ => e,
    })?;
    let mut identity = age::scrypt::Identity::new(passphrase);
    // age otherwise accepts 4 doublings above the work factor it times on this machine now,
    // so a busy moment could refuse the state GhostSoap wrote here earlier.
    identity.set_max_work_factor(MAX_WORK_FACTOR);
    let text = age::decrypt(&identity, &bytes).map_err(|e| match e {
        age::DecryptError::DecryptionFailed | age::DecryptError::NoMatchingKeys => {
            io::Error::new(io::ErrorKind::PermissionDenied, "wrong passphrase")
        }
        other => io::Error::other(format!("cannot decrypt {}: {other}", path.display())),
    })?;
    // No detail: a byte that is not UTF-8 would be quoted from the profile's text.
    let text = String::from_utf8(text)
        .map_err(|_| io::Error::other("cannot read the state: it is not text"))?;
    decode(&text)
}

/// Create `dir` and its parents; on Unix, the folders created are private to the user.
#[cfg(unix)]
pub(crate) fn create_private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
pub(crate) fn create_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)
}

/// A lock file created exclusively, so 2 GhostSoap processes cannot interleave changes to
/// the same files, and removed when dropped. Used for the state and for each drafts folder.
pub(crate) struct Lock(PathBuf);

impl Lock {
    pub(crate) fn acquire(dir: &Path, name: &str) -> io::Result<Self> {
        let path = dir.join(name);
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| match e.kind() {
                io::ErrorKind::AlreadyExists => io::Error::new(
                    e.kind(),
                    format!(
                        "{} exists: another GhostSoap is writing, or one stopped mid-write; \
                         delete it if no GhostSoap is running",
                        path.display()
                    ),
                ),
                _ => e,
            })?;
        Ok(Self(path))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // A lock left behind stops the next write with a message saying how to clear it.
        let _ = fs::remove_file(&self.0);
    }
}

/// Write `bytes` to a temporary file beside `path`, flush it to disk and rename it over
/// `path`. The caller holds the [`Lock`]. A rename that fails because antivirus or an indexer
/// briefly holds the file is retried. On Unix the folder is flushed too, so the rename itself
/// survives a power cut. A temporary file left by an interrupted write is removed first, so
/// GhostSoap keeps only what it reported as saved: after an interrupted update `path` still
/// holds the last complete state, and after an interrupted `init` the user runs it again.
fn write_atomic(dir: &Path, bytes: &[u8], replaced: impl FnOnce()) -> io::Result<()> {
    let path = state_path(dir);
    let tmp = path.with_extension("age.tmp");
    // Whatever holds the temporary name is removed and the file made anew: opening it in
    // place would write through a link, and a link to the state would empty the state.
    match fs::remove_file(&tmp) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    rename_with_retry(|| fs::rename(&tmp, &path))?;
    // The new state is in place from here, so a failure must not read as nothing saved.
    replaced();
    #[cfg(unix)]
    fs::File::open(dir)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| {
            let saved = "the state was saved, but its folder could not be flushed to disk";
            io::Error::new(
                e.kind(),
                format!("{saved}, so a power cut now could undo it: {e}"),
            )
        })?;
    Ok(())
}

fn rename_with_retry(mut rename: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    let mut attempt = 0;
    loop {
        match rename() {
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied && attempt < 10 => {
                attempt += 1;
                thread::sleep(Duration::from_millis(200));
            }
            other => return other,
        }
    }
}

#[cfg(test)]
mod tests;
