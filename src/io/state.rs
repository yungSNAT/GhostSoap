// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: FSL-1.1-ALv2

//! The encrypted state file (plan section 5, 'Storage and secrets'). Format 1 (M2) held the
//! profile alone; format 2 (M3a) adds the messages, their requests and events. A format 1
//! file is read as a format 2 state with no messages, and is replaced only by the first
//! command that changes something.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use age::secrecy::SecretString;
use serde::Deserialize;

use crate::core::profile::Profile;
use crate::core::tracker::FORMAT;
pub use crate::core::tracker::State;

const STATE_FILE: &str = "state.age";
const LOCK_FILE: &str = "state.lock";

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
    let var = |name: &str| {
        env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(home) = var("GHOSTSOAP_HOME") {
        return Ok(home);
    }
    let dir = if cfg!(windows) {
        var("LOCALAPPDATA").map(|d| d.join("GhostSoap"))
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|d| d.join("Library/Application Support/GhostSoap"))
    } else {
        var("XDG_DATA_HOME")
            .or_else(|| var("HOME").map(|h| h.join(".local/share")))
            .map(|d| d.join("ghostsoap"))
    };
    dir.ok_or_else(|| io::Error::other("cannot find a data directory; set GHOSTSOAP_HOME"))
}

pub fn state_path(dir: &Path) -> PathBuf {
    dir.join(STATE_FILE)
}

/// The state as the text that is encrypted. Refuses text that would not read back as the
/// same state, so a state file is never replaced by one GhostSoap cannot read.
fn encode(state: &State) -> io::Result<String> {
    let unsaved = |why: &str| io::Error::other(format!("{why}; nothing was saved"));
    if let Some(problem) = state.problems().first() {
        return Err(unsaved(problem));
    }
    let text = toml::to_string(state).map_err(io::Error::other)?;
    match toml::from_str::<State>(&text) {
        Ok(back) if back == *state => Ok(text),
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
            let state: State = toml::from_str(text).map_err(unreadable)?;
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

fn encrypt(state: &State, passphrase: SecretString) -> io::Result<Vec<u8>> {
    let text = encode(state)?;
    let recipient = age::scrypt::Recipient::new(passphrase);
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
    write_atomic(&path, &bytes, || {})
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
    update_then(dir, passphrase, change, |_| {}, Ok)
}

/// Commit a state change before handing over its result, whilst still excluding other writers.
/// `committed` runs at the atomic replacement, even if the subsequent directory flush
/// fails. An error in `after_save` leaves the committed state in place.
pub(crate) fn update_then<T, U>(
    dir: &Path,
    passphrase: SecretString,
    change: impl FnOnce(&mut State) -> Result<T, String>,
    committed: impl FnOnce(&mut T),
    after_save: impl FnOnce(T) -> Result<U, String>,
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
        write_atomic(&path, &encrypt(&state, passphrase)?, || {
            committed(&mut value)
        })?;
    } else {
        committed(&mut value);
    }
    after_save(value).map_err(io::Error::other)
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
    let identity = age::scrypt::Identity::new(passphrase);
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
fn write_atomic(path: &Path, bytes: &[u8], replaced: impl FnOnce()) -> io::Result<()> {
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
    let mut attempt = 0;
    loop {
        match fs::rename(&tmp, path) {
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied && attempt < 10 => {
                attempt += 1;
                thread::sleep(Duration::from_millis(200));
            }
            other => break other?,
        }
    }
    // The new state is in place from here, so a failure must not read as nothing saved.
    replaced();
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        let flushed = fs::File::open(dir).and_then(|dir| dir.sync_all());
        flushed.map_err(|e| {
            let saved = "the state was saved, but its folder could not be flushed to disk";
            io::Error::new(
                e.kind(),
                format!("{saved}, so a power cut now could undo it: {e}"),
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;
    use crate::core::register::{ChannelKind, Lever};
    use crate::core::tracker::{Message, Request};

    fn profile() -> Profile {
        Profile {
            full_name: "Lara Croft".to_owned(),
            email: Some("lara@croft-manor.example".to_owned()),
            ..Profile::default()
        }
    }

    fn secret(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    #[test]
    fn round_trip_and_wrong_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        create(&home, &State::new(profile()), secret("correct horse")).unwrap();
        let raw = fs::read(state_path(&home)).unwrap();
        assert!(
            !String::from_utf8_lossy(&raw).contains("Croft"),
            "state is not plaintext"
        );
        assert!(!home.join(LOCK_FILE).exists());
        assert_eq!(
            load(&home, secret("correct horse")).unwrap().profile,
            profile()
        );
        let err = load(&home, secret("wrong horse")).unwrap_err();
        assert_eq!(err.to_string(), "wrong passphrase");
        let err = create(&home, &State::new(profile()), secret("x")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn a_held_lock_stops_a_write() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(LOCK_FILE), "").unwrap();
        let err = create(dir.path(), &State::new(profile()), secret("pass phrase")).unwrap_err();
        assert!(err.to_string().contains("another GhostSoap is writing"));
        assert!(!state_path(dir.path()).exists());
    }

    /// A state file written by `ghostsoap init` at M2 (main at 2286400, format 1).
    const M2_FILE: &[u8] = include_bytes!("../../tests/fixtures/state/format-1.age");
    const M2_PASSPHRASE: &str = "present day, present time";

    /// A data folder holding the M2 state file.
    fn m2_home() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(state_path(dir.path()), M2_FILE).unwrap();
        dir
    }

    fn m2() -> SecretString {
        secret(M2_PASSPHRASE)
    }

    fn message(tag: &str) -> Message {
        Message {
            tag: tag.to_owned(),
            entry: "cotality-au".to_owned(),
            channel: ChannelKind::Email,
            address: "privacy@example.com.au".to_owned(),
            drafted: NaiveDate::from_ymd_opt(2026, 11, 10).unwrap(),
            draft: None,
            superseded: false,
            sends: Vec::new(),
            requests: vec![Request::new(Lever::App7Source)],
        }
    }

    fn add(tag: &'static str) -> impl FnOnce(&mut State) -> Result<(), String> {
        move |state| {
            state.messages.push(message(tag));
            Ok(())
        }
    }

    /// Encrypt `text` as the state file in `dir`, with a low work factor to keep tests quick.
    fn put(dir: &Path, text: &str, passphrase: &str) {
        let mut recipient = age::scrypt::Recipient::new(secret(passphrase));
        recipient.set_work_factor(10);
        let bytes = age::encrypt(&recipient, text.as_bytes()).unwrap();
        fs::write(state_path(dir), bytes).unwrap();
    }

    #[test]
    fn an_m2_file_reads_as_format_2_and_the_first_change_replaces_it() {
        let dir = m2_home();
        let path = state_path(dir.path());
        let state = load(dir.path(), m2()).unwrap();
        assert_eq!(state.format, 2);
        let some = |text: &str| Some(text.to_owned());
        let profile = Profile {
            full_name: "Lara Croft".to_owned(),
            alias_names: vec!["Lara Croft-Henshingly".to_owned()],
            email: some("lara@croft-manor.example"),
            phone: some("0400 000 001"),
            date_of_birth: some("1968-02-14"),
            address: some("1 Croft Manor Lane, Surrey Hills VIC 3127"),
            previous_addresses: Vec::new(),
            postcode: some("3127"),
        };
        assert_eq!(state.profile, profile, "the profile is as M2 stored it");
        assert!(state.messages.is_empty());
        assert_eq!(fs::read(&path).unwrap(), M2_FILE, "reading writes nothing");

        update(dir.path(), m2(), |_| Ok(())).unwrap();
        assert_eq!(
            fs::read(&path).unwrap(),
            M2_FILE,
            "a change that changes nothing writes nothing"
        );

        update(dir.path(), m2(), add("GS-7K3QX")).unwrap();
        assert_ne!(fs::read(&path).unwrap(), M2_FILE);
        let state = load(dir.path(), m2()).unwrap();
        assert_eq!(state.messages, [message("GS-7K3QX")]);
        assert_eq!(state.profile, profile, "and unchanged by the first write");
        let left: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|f| f.unwrap().file_name())
            .collect();
        assert_eq!(left, [STATE_FILE], "no lock or temporary file is left");
    }

    #[test]
    fn a_draft_is_handed_over_after_save_and_before_the_next_writer() {
        let dir = m2_home();
        update_then(
            dir.path(),
            m2(),
            add("GS-7K3QX"),
            |_| {},
            |()| {
                let state = load(dir.path(), m2()).unwrap();
                assert!(state.message("GS-7K3QX").is_some());
                let competing = update(dir.path(), m2(), add("GS-00AZ9")).unwrap_err();
                assert_eq!(competing.kind(), io::ErrorKind::AlreadyExists);
                Ok(())
            },
        )
        .unwrap();
        update(dir.path(), m2(), add("GS-00AZ9")).unwrap();
    }

    #[test]
    fn a_failed_migration_leaves_the_m2_file_as_it_was() {
        let dir = m2_home();
        let path = state_path(dir.path());
        let unchanged = |why: &str| assert_eq!(fs::read(&path).unwrap(), M2_FILE, "{why}");

        let err = update(dir.path(), m2(), |state| {
            state.messages.push(message("GS-7K3QX"));
            Err::<(), _>("the change failed".to_owned())
        })
        .unwrap_err();
        assert_eq!(err.to_string(), "the change failed");
        unchanged("a change that fails");

        let err = update(dir.path(), secret("not the passphrase"), add("GS-7K3QX")).unwrap_err();
        assert_eq!(err.to_string(), "wrong passphrase");
        unchanged("a wrong passphrase");

        fs::write(dir.path().join(LOCK_FILE), "").unwrap();
        let err = update(dir.path(), m2(), add("GS-7K3QX")).unwrap_err();
        assert!(err.to_string().contains("another GhostSoap is writing"));
        unchanged("a held lock");
        fs::remove_file(dir.path().join(LOCK_FILE)).unwrap();

        // A folder sits where the new file is first written, so the write fails.
        fs::create_dir(path.with_extension("age.tmp")).unwrap();
        update(dir.path(), m2(), add("GS-7K3QX")).unwrap_err();
        unchanged("a write that fails");
        assert!(!dir.path().join(LOCK_FILE).exists(), "the lock is released");
    }

    #[test]
    fn a_link_at_the_temporary_name_is_replaced_not_written_through() {
        let dir = m2_home();
        let path = state_path(dir.path());
        let tmp = path.with_extension("age.tmp");
        let canary = dir.path().join("canary.txt");
        fs::write(&canary, "unchanged").unwrap();
        fs::hard_link(&canary, &tmp).unwrap();
        update(dir.path(), m2(), add("GS-7K3QX")).unwrap();
        assert_eq!(fs::read_to_string(&canary).unwrap(), "unchanged");
        assert!(!tmp.exists());

        // A link to the state itself: the state must hold the old or the new, never neither.
        fs::hard_link(&path, &tmp).unwrap();
        update(dir.path(), m2(), add("GS-00AZ9")).unwrap();
        assert!(!tmp.exists());
        assert_eq!(load(dir.path(), m2()).unwrap().messages.len(), 2);
    }

    /// After the rename the new state is in place. A folder that can be written but not
    /// read lets the rename through and stops the flush that follows it.
    #[cfg(unix)]
    #[test]
    fn a_failure_after_the_rename_says_the_state_was_saved() {
        use std::os::unix::fs::PermissionsExt;
        let dir = m2_home();
        let mode = |mode| fs::set_permissions(dir.path(), fs::Permissions::from_mode(mode));
        mode(0o300).unwrap();
        if fs::read_dir(dir.path()).is_ok() {
            // Root reads any folder, so the flush cannot be made to fail this way.
            mode(0o700).unwrap();
            return;
        }
        let err = update(dir.path(), m2(), add("GS-7K3QX")).unwrap_err();
        mode(0o700).unwrap();
        assert!(
            err.to_string().starts_with("the state was saved, but"),
            "{err}"
        );
        let state = load(dir.path(), m2()).unwrap();
        assert_eq!(state.messages, [message("GS-7K3QX")], "and it reads back");
        assert!(!dir.path().join(LOCK_FILE).exists());
    }

    #[test]
    fn a_state_that_would_not_load_is_never_written() {
        let dir = m2_home();
        let err = update(dir.path(), m2(), |state| {
            state
                .messages
                .extend([message("GS-7K3QX"), message("GS-7K3QX")]);
            Ok(())
        })
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "tag GS-7K3QX is used twice; nothing was saved"
        );
        assert_eq!(fs::read(state_path(dir.path())).unwrap(), M2_FILE);
    }

    #[test]
    fn a_newer_format_is_refused_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let text = "format = 3\nlater = true\n\n[profile]\nfull_name = \"Lara Croft\"\n";
        put(dir.path(), text, "correct horse");
        let before = fs::read(state_path(dir.path())).unwrap();
        let expected = "state format 3 is newer than this version of GhostSoap reads (format 2); \
                        upgrade GhostSoap";
        let err = load(dir.path(), secret("correct horse")).unwrap_err();
        assert_eq!(err.to_string(), expected);
        let err = update(dir.path(), secret("correct horse"), add("GS-7K3QX")).unwrap_err();
        assert_eq!(err.to_string(), expected);
        assert_eq!(fs::read(state_path(dir.path())).unwrap(), before);
    }

    #[test]
    fn an_unreadable_state_is_reported_without_its_text() {
        let dir = tempfile::tempdir().unwrap();
        for (text, expected) in [
            (
                "format = 2\nprofile = \"Lara Croft\"\n",
                "cannot read the state: it is not as expected at line 2 of its text",
            ),
            (
                "format = 1\n\n[profile]\nfull_name = \"Lara Croft\"\nshoe_size = \"Croft\"\n",
                "cannot read the state: it is not as expected at line 5 of its text",
            ),
            (
                "[profile]\nfull_name = \"Lara Croft\"\n",
                "cannot read the state: it has no format this version knows",
            ),
            (
                "format = 0\n\n[profile]\nfull_name = \"Lara Croft\"\n",
                "cannot read the state: it has no format this version knows",
            ),
        ] {
            put(dir.path(), text, "correct horse");
            let err = load(dir.path(), secret("correct horse")).unwrap_err();
            assert_eq!(err.to_string(), expected, "{text}");
        }
    }

    #[test]
    fn a_state_file_holding_a_broken_rule_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = State::new(profile());
        state
            .messages
            .extend([message("GS-7K3QX"), message("GS-7K3QX")]);
        put(
            dir.path(),
            &toml::to_string(&state).unwrap(),
            "correct horse",
        );
        let err = load(dir.path(), secret("correct horse")).unwrap_err();
        assert_eq!(
            err.to_string(),
            "cannot read the state: tag GS-7K3QX is used twice"
        );
    }

    #[test]
    fn update_without_a_state_says_to_run_init() {
        let dir = tempfile::tempdir().unwrap();
        for home in [dir.path().to_owned(), dir.path().join("absent")] {
            let err = update(&home, secret("correct horse"), add("GS-7K3QX")).unwrap_err();
            assert!(err.to_string().contains("run `ghostsoap init` first"));
            assert!(!home.join(LOCK_FILE).exists());
        }
    }
}
