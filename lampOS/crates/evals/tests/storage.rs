//! Filesystem failure and recovery boundaries, using only private host fixtures.
use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use lamp_evals::{Contract, Ingest, Observation, Origin, Recorder, SourceTime, Terminal, store};

static NEXT: AtomicU64 = AtomicU64::new(0);
const END_HOUR: u64 = 500_000;

struct Folder(PathBuf);
impl Folder {
    fn new() -> Self {
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "lamp-evals-storage-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => {
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
                    return Self(path);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("fixture directory: {error}"),
            }
        }
        panic!("no unused fixture directory");
    }

    fn child(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
}
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn private_write(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn absent_state_is_explicit_and_saving_round_trips_the_whole_checkpoint() {
    let folder = Folder::new();
    let mut disk = store::Store::open(&folder.0).unwrap();
    assert!(disk.load().unwrap().is_none());
    let mut first = Recorder::new(END_HOUR).unwrap();
    let observation = Observation {
        epoch: 8,
        turn: 9,
        time: SourceTime::UtcMinute((END_HOUR - 1) * 60),
        build: None,
        origin: Origin::Microphone,
        terminal: Terminal::Completed,
        audio_started: Some(true),
        endpoint_to_first_write_ms: Some(700),
        contract: Contract::VoiceDeliveryV1,
    };
    assert_eq!(first.observe(observation.clone()), Ingest::Accepted);
    disk.save(&first).unwrap();
    assert_eq!(
        disk.load().unwrap().unwrap().checkpoint().unwrap(),
        first.checkpoint().unwrap()
    );
    let mut later = first;
    later.advance_window(END_HOUR + 1).unwrap();
    disk.save(&later).unwrap();
    assert_eq!(
        disk.load().unwrap().unwrap().checkpoint().unwrap(),
        later.checkpoint().unwrap()
    );
    assert_eq!(
        fs::metadata(folder.0.join(store::FILE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(fs::read_dir(&folder.0).unwrap().count(), 1);
    drop(disk);
    let reopened = store::Store::open(&folder.0).unwrap();
    assert_eq!(
        reopened.load().unwrap().unwrap().checkpoint().unwrap(),
        later.checkpoint().unwrap()
    );
    assert_eq!(
        reopened.load().unwrap().unwrap().observe(observation),
        Ingest::Duplicate
    );
}

#[test]
fn a_second_writer_is_refused_and_dropping_the_holder_releases_it() {
    let folder = Folder::new();
    let first = store::Store::open(&folder.0).unwrap();
    assert_eq!(
        store::Store::open(&folder.0).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    drop(first);
    assert!(store::Store::open(&folder.0).is_ok());
}

#[test]
fn shared_directory_is_not_silently_repaired() {
    let folder = Folder::new();
    fs::set_permissions(&folder.0, fs::Permissions::from_mode(0o750)).unwrap();
    assert_eq!(
        store::Store::open(&folder.0).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        fs::metadata(&folder.0).unwrap().permissions().mode() & 0o777,
        0o750
    );
}

#[test]
fn directory_and_checkpoint_links_are_refused_without_reading_the_target() {
    let folder = Folder::new();
    let target = folder.child("target");
    let link = folder.0.join("directory-link");
    symlink(&target, &link).unwrap();
    assert!(store::Store::open(&link).is_err());
    let trailing = PathBuf::from(format!("{}/", link.display()));
    assert!(store::Store::open(&trailing).is_err());
    assert!(store::Store::open(&link.join(".")).is_err());
    let disk = store::Store::open(&target).unwrap();
    let other = folder.0.join("private-canary");
    private_write(&other, b"private words must never be an error message");
    symlink(&other, target.join(store::FILE)).unwrap();
    let error = disk.load().err().unwrap();
    assert!(!error.to_string().contains("private words"));
    assert_eq!(
        fs::read(&other).unwrap(),
        b"private words must never be an error message"
    );
}

#[test]
fn hardlinked_or_shared_checkpoint_is_refused() {
    let folder = Folder::new();
    let mut disk = store::Store::open(&folder.0).unwrap();
    disk.save(&Recorder::new(END_HOUR).unwrap()).unwrap();
    let state = folder.0.join(store::FILE);
    fs::set_permissions(&state, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(disk.load().is_err());
    fs::set_permissions(&state, fs::Permissions::from_mode(0o600)).unwrap();
    fs::hard_link(&state, folder.0.join("alias")).unwrap();
    assert!(disk.load().is_err());
}

#[test]
fn corruption_and_truncation_are_errors_and_never_replaced_with_empty_history() {
    let folder = Folder::new();
    let mut disk = store::Store::open(&folder.0).unwrap();
    disk.save(&Recorder::new(END_HOUR).unwrap()).unwrap();
    let state = folder.0.join(store::FILE);
    let original = fs::read(&state).unwrap();
    let mut changed = original.clone();
    *changed.last_mut().unwrap() ^= 1;
    private_write(&state, &changed);
    assert_eq!(
        disk.load().err().unwrap().kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(fs::read(&state).unwrap(), changed);
    for cut in [0, 5, original.len() - 1] {
        private_write(&state, &original[..cut]);
        assert_eq!(
            disk.load().err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(fs::read(&state).unwrap(), &original[..cut]);
    }
}

#[test]
fn oversized_files_and_directories_are_refused_before_an_unbounded_read() {
    let folder = Folder::new();
    let disk = store::Store::open(&folder.0).unwrap();
    let state = folder.0.join(store::FILE);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&state)
        .unwrap();
    file.set_len((store::MAX_CHECKPOINT_BYTES + 1024) as u64)
        .unwrap();
    assert!(disk.load().is_err());
    drop(file);
    fs::remove_file(&state).unwrap();
    fs::create_dir(&state).unwrap();
    assert!(disk.load().is_err());
}

// rustix does not expose mkfifoat on Darwin. Exercise the production Linux
// FIFO path there; the directory fixture above covers non-regular files on
// both supported host test platforms.
#[cfg(target_os = "linux")]
#[test]
fn a_fifo_with_no_writer_is_refused_without_waiting_for_input() {
    let folder = Folder::new();
    let disk = store::Store::open(&folder.0).unwrap();
    let state = folder.0.join(store::FILE);
    rustix::fs::mkfifoat(rustix::fs::CWD, &state, rustix::fs::Mode::RUSR).unwrap();
    assert!(disk.load().is_err());
}

#[test]
fn failed_replace_preserves_the_existing_entry_and_removes_its_candidate() {
    let folder = Folder::new();
    let mut disk = store::Store::open(&folder.0).unwrap();
    fs::create_dir(folder.0.join(store::FILE)).unwrap();
    assert!(disk.save(&Recorder::new(END_HOUR).unwrap()).is_err());
    assert!(folder.0.join(store::FILE).is_dir());
    assert_eq!(fs::read_dir(&folder.0).unwrap().count(), 1);
}

#[test]
fn replacement_uses_the_locked_directory_even_if_its_path_is_replaced() {
    let folder = Folder::new();
    let held = folder.child("held");
    let mut disk = store::Store::open(&held).unwrap();
    let moved = folder.0.join("moved");
    fs::rename(&held, &moved).unwrap();
    fs::create_dir(&held).unwrap();
    let candidate = ".state.bin.0011223344556677.new";
    private_write(&moved.join(candidate), b"incomplete checkpoint");
    private_write(&held.join(candidate), b"different directory");
    disk.save(&Recorder::new(END_HOUR).unwrap()).unwrap();
    assert!(moved.join(store::FILE).is_file());
    assert!(!held.join(store::FILE).exists());
    assert!(!moved.join(candidate).exists());
    assert_eq!(
        fs::read(held.join(candidate)).unwrap(),
        b"different directory"
    );
    assert!(disk.load().unwrap().is_some());
}

#[test]
fn recovery_removes_crash_candidates_but_preserves_committed_and_unrelated_files() {
    let folder = Folder::new();
    let mut disk = store::Store::open(&folder.0).unwrap();
    disk.save(&Recorder::new(END_HOUR).unwrap()).unwrap();
    let committed = fs::read(folder.0.join(store::FILE)).unwrap();
    drop(disk);
    for (nonce, bytes) in [
        (1, b"".as_slice()),
        (2, b"incomplete"),
        (3, committed.as_slice()),
    ] {
        private_write(
            &folder.0.join(format!(".state.bin.{nonce:016x}.new")),
            bytes,
        );
    }
    let unrelated = folder.0.join(".state.bin.not-a-nonce.new");
    private_write(&unrelated, b"leave this alone");
    let mut reopened = store::Store::open(&folder.0).unwrap();
    assert_eq!(fs::read_dir(&folder.0).unwrap().count(), 2);
    assert_eq!(fs::read(folder.0.join(store::FILE)).unwrap(), committed);
    assert_eq!(fs::read(&unrelated).unwrap(), b"leave this alone");
    assert!(reopened.load().unwrap().is_some());
    private_write(
        &folder.0.join(".state.bin.0000000000000004.new"),
        b"partial",
    );
    reopened
        .save(&Recorder::new(END_HOUR + 1).unwrap())
        .unwrap();
    assert_eq!(fs::read_dir(&folder.0).unwrap().count(), 2);
}

#[test]
fn unsafe_candidates_are_refused_without_removing_any_files() {
    for kind in ["symlink", "directory", "hardlink", "shared", "oversized"] {
        let folder = Folder::new();
        let candidate = folder.0.join(".state.bin.0000000000000001.new");
        let other = folder.0.join("keep");
        private_write(&other, b"private canary");
        match kind {
            "symlink" => symlink(&other, &candidate).unwrap(),
            "directory" => fs::create_dir(&candidate).unwrap(),
            "hardlink" => fs::hard_link(&other, &candidate).unwrap(),
            "shared" => {
                private_write(&candidate, b"partial");
                fs::set_permissions(&candidate, fs::Permissions::from_mode(0o640)).unwrap();
            }
            "oversized" => {
                let file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&candidate)
                    .unwrap();
                file.set_len((store::MAX_CHECKPOINT_BYTES + 1024) as u64)
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(store::Store::open(&folder.0).is_err(), "{kind}");
        assert!(fs::symlink_metadata(&candidate).is_ok(), "{kind}");
        assert_eq!(fs::read(&other).unwrap(), b"private canary");
        assert_eq!(fs::read_dir(&folder.0).unwrap().count(), 2);
    }
}

#[test]
fn recovery_scan_is_bounded_and_refusal_does_not_add_or_delete_entries() {
    let folder = Folder::new();
    for index in 0..=store::MAX_DIRECTORY_ENTRIES {
        private_write(
            &folder.0.join(format!(".state.bin.{index:016x}.new")),
            b"partial",
        );
    }
    assert!(store::Store::open(&folder.0).is_err());
    assert_eq!(
        fs::read_dir(&folder.0).unwrap().count(),
        store::MAX_DIRECTORY_ENTRIES + 1
    );
    let full = Folder::new();
    let mut disk = store::Store::open(&full.0).unwrap();
    for index in 0..store::MAX_DIRECTORY_ENTRIES - 1 {
        private_write(&full.0.join(format!("unrelated-{index}")), b"leave");
    }
    assert!(disk.save(&Recorder::new(END_HOUR).unwrap()).is_err());
    assert_eq!(
        fs::read_dir(&full.0).unwrap().count(),
        store::MAX_DIRECTORY_ENTRIES - 1
    );
    assert!(!full.0.join(store::FILE).exists());
}

#[test]
fn first_save_at_the_directory_limit_still_allows_reopen_and_replacement() {
    let folder = Folder::new();
    for index in 0..store::MAX_DIRECTORY_ENTRIES - 2 {
        private_write(&folder.0.join(format!("unrelated-{index}")), b"leave");
    }
    let mut disk = store::Store::open(&folder.0).unwrap();
    disk.save(&Recorder::new(END_HOUR).unwrap()).unwrap();
    drop(disk);
    let mut reopened = store::Store::open(&folder.0).unwrap();
    reopened
        .save(&Recorder::new(END_HOUR + 1).unwrap())
        .unwrap();
    assert_eq!(
        fs::read_dir(&folder.0).unwrap().count(),
        store::MAX_DIRECTORY_ENTRIES - 1
    );
    assert_eq!(
        reopened.load().unwrap().unwrap().snapshot().window_end_hour,
        END_HOUR + 1
    );
}
