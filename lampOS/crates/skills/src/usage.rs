//! How often skills run, by where each came from, for `lamp-cloud`'s opt-in
//! health report (`docs/cloud.md`, "Skills, counted"; the owner's ask on
//! #4710, 3 Nov). Numbers only: a run is counted against its skill's
//! [`Source`], and a Store or made skill's package by its SHA-256 (the id
//! the Store's catalogue names it by, and lamp-cloud's to tell a made
//! skill's maker), never its name, words or files.
//!
//! A run is counted where it starts (`main.rs`'s `start`), on an atomic in
//! memory: no lock and no file on a run's way. A thread of its own adds what
//! was counted to [`RUNS_FILE`] in the part's `--state-dir` at most every
//! [`FLUSH_EVERY`], so a restart loses at most that much. The file is
//! running totals; `lamp-cloud`, the same user, reads it when it builds a
//! report, and makes a day's counts from two readings.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The totals' file, in the part's `--state-dir`.
pub const RUNS_FILE: &str = "runs.json";
/// The most often what was counted is written.
pub const FLUSH_EVERY: Duration = Duration::from_secs(60);
/// The most packages kept by SHA-256 in each of the file's two maps; past
/// it the smallest totals are let go (a skill long removed, most likely).
pub const MOST_IDS: usize = 256;
/// The most bytes of [`RUNS_FILE`] read.
const MOST_BYTES: u64 = 128 * 1024;
/// The file's own version. Adding a field keeps 1.
pub const VERSION: u64 = 1;

/// Where a skill came from, as far as this part can tell: a core skill, a
/// package from the Skills Store, a skill made on this lamp (`made: here`;
/// by voice or in the app is the voice's to say, by the package's SHA-256),
/// one a paired computer pushed, or any other installable (put on from a
/// folder or a file).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Core,
    Store(String),
    Made(Option<String>),
    Pushed,
    Other,
}

/// `skill`'s source, from its folder in the skills folder: its kind, who
/// pushed it, its install record ([`crate::package::installed_from`]) and
/// its card's `made: here`. A SHA-256 is kept only as 64 lowercase hex.
pub fn source_of(skill: &crate::Skill, folder: &Path) -> Source {
    if skill.kind == crate::SkillKind::Core {
        return Source::Core;
    }
    if skill.pushed_by.is_some() {
        return Source::Pushed;
    }
    let installed = crate::package::installed_from(folder);
    if let Some(installed) = &installed
        && installed.from_store
    {
        return Source::Store(installed.sha256.clone());
    }
    if made_here(folder) {
        return Source::Made(installed.map(|installed| installed.sha256));
    }
    Source::Other
}

/// Whether a skill folder's `SKILL.md` says `made: here` in its front matter.
pub fn made_here(folder: &Path) -> bool {
    let mut card = String::new();
    let read = fs::File::open(folder.join("SKILL.md"))
        .and_then(|file| file.take(64 * 1024).read_to_string(&mut card));
    read.is_ok()
        && card
            .strip_prefix("---\n")
            .and_then(|rest| rest.split_once("\n---"))
            .is_some_and(|(front, _)| front.lines().any(|line| line.trim() == "made: here"))
}

/// [`RUNS_FILE`]: runs since the file began, by source, and by package for
/// the Store's and made skills.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Totals {
    pub version: u64,
    pub core: u64,
    pub store: u64,
    pub made: u64,
    pub pushed: u64,
    pub other: u64,
    /// By the Store package's SHA-256.
    pub store_ids: BTreeMap<String, u64>,
    /// By the made skill's package SHA-256; a made skill with no install
    /// record is counted in `made` only.
    pub made_ids: BTreeMap<String, u64>,
}

impl Totals {
    /// `runs` more for `source`.
    pub fn add(&mut self, source: &Source, runs: u64) {
        let id = |ids: &mut BTreeMap<String, u64>, sha256: &str| {
            if crate::package::sha256_ok(sha256) {
                let total = ids.entry(sha256.to_owned()).or_default();
                *total = total.saturating_add(runs);
            }
        };
        match source {
            Source::Core => self.core = self.core.saturating_add(runs),
            Source::Store(sha256) => {
                self.store = self.store.saturating_add(runs);
                id(&mut self.store_ids, sha256);
            }
            Source::Made(sha256) => {
                self.made = self.made.saturating_add(runs);
                if let Some(sha256) = sha256 {
                    id(&mut self.made_ids, sha256);
                }
            }
            Source::Pushed => self.pushed = self.pushed.saturating_add(runs),
            Source::Other => self.other = self.other.saturating_add(runs),
        }
    }

    /// Each map at most [`MOST_IDS`], the largest totals kept, and any key
    /// that is not a SHA-256 dropped.
    fn bounded(&mut self) {
        for ids in [&mut self.store_ids, &mut self.made_ids] {
            ids.retain(|id, _| crate::package::sha256_ok(id));
            if ids.len() > MOST_IDS {
                let mut by_runs: Vec<(String, u64)> = std::mem::take(ids).into_iter().collect();
                by_runs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                by_runs.truncate(MOST_IDS);
                *ids = by_runs.into_iter().collect();
            }
        }
    }
}

/// The totals kept in `path`; empty when there is none, and none when it
/// can't be read whole.
pub fn read(path: &Path) -> Option<Totals> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Some(Totals::default()),
        Err(_) => return None,
    };
    let mut bytes = Vec::new();
    file.take(MOST_BYTES).read_to_end(&mut bytes).ok()?;
    let mut totals: Totals = serde_json::from_slice(&bytes).ok()?;
    totals.bounded();
    Some(totals)
}

/// The counts of runs not yet written, by source: one atomic each, found
/// at a skill's load and counted on at each run with no lock.
#[derive(Clone, Default)]
pub struct Counting {
    by_source: Arc<Mutex<HashMap<Source, Arc<AtomicU64>>>>,
}

impl Counting {
    /// The counter for `source`, made the first time; asked at each load,
    /// never at a run.
    pub fn counter(&self, source: Source) -> Arc<AtomicU64> {
        let mut by_source = self.by_source.lock().unwrap_or_else(|e| e.into_inner());
        by_source.entry(source).or_default().clone()
    }

    /// What was counted since the last take, each counter back to 0.
    fn take(&self) -> Vec<(Source, u64)> {
        let by_source = self.by_source.lock().unwrap_or_else(|e| e.into_inner());
        by_source
            .iter()
            .map(|(source, count)| (source.clone(), count.swap(0, Ordering::Relaxed)))
            .filter(|(_, runs)| *runs > 0)
            .collect()
    }

    /// What was counted, added to the totals in `path` and written whole
    /// beside it, then moved over it (0600). Nothing counted, nothing
    /// written; one that can't be written is counted again next time.
    pub fn flush(&self, path: &Path) -> io::Result<()> {
        let taken = self.take();
        if taken.is_empty() {
            return Ok(());
        }
        let written = (|| {
            // A file that can't be read is begun again: lamp-cloud reads a
            // smaller total as a new beginning.
            let mut totals = read(path).unwrap_or_default();
            totals.version = VERSION;
            for (source, runs) in &taken {
                totals.add(source, *runs);
            }
            totals.bounded();
            let new = path.with_extension("json.new");
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&new)?;
            file.write_all(&serde_json::to_vec(&totals).map_err(io::Error::other)?)?;
            file.sync_all()?;
            fs::rename(&new, path)
        })();
        if written.is_err() {
            for (source, runs) in taken {
                self.counter(source).fetch_add(runs, Ordering::Relaxed);
            }
        }
        written
    }

    /// [`Self::flush`] into `state`'s [`RUNS_FILE`] every [`FLUSH_EVERY`],
    /// on a thread of its own, for as long as the part runs.
    pub fn keep_in(&self, state: &Path) -> io::Result<()> {
        let counting = self.clone();
        let path: PathBuf = state.join(RUNS_FILE);
        std::thread::Builder::new()
            .name("lamp-skills-runs".into())
            .spawn(move || {
                let mut said = false;
                loop {
                    std::thread::sleep(FLUSH_EVERY);
                    match counting.flush(&path) {
                        Ok(()) => said = false,
                        Err(error) if !said => {
                            said = true;
                            eprintln!("lamp-skills: the runs counted not kept: {error}");
                        }
                        Err(_) => {}
                    }
                }
            })
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("usage-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn runs_are_counted_by_source_and_added_to_the_file() {
        let dir = place("flush");
        let path = dir.join(RUNS_FILE);
        let counting = Counting::default();
        let store = "a".repeat(64);
        let made = "b".repeat(64);
        let core = counting.counter(Source::Core);
        let from_store = counting.counter(Source::Store(store.clone()));
        let made_here = counting.counter(Source::Made(Some(made.clone())));
        // A key that isn't a SHA-256 counts by source only.
        let odd = counting.counter(Source::Store("Ann's skill".into()));
        for _ in 0..3 {
            core.fetch_add(1, Ordering::Relaxed);
        }
        from_store.fetch_add(2, Ordering::Relaxed);
        made_here.fetch_add(1, Ordering::Relaxed);
        odd.fetch_add(1, Ordering::Relaxed);
        counting.flush(&path).unwrap();
        from_store.fetch_add(1, Ordering::Relaxed);
        counting.flush(&path).unwrap();
        let totals = read(&path).unwrap();
        assert_eq!(
            (totals.core, totals.store, totals.made),
            (3, 4, 1),
            "{totals:?}"
        );
        assert_eq!(totals.store_ids, BTreeMap::from([(store, 3)]));
        assert_eq!(totals.made_ids, BTreeMap::from([(made, 1)]));
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("Ann"), "{text}");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        // Nothing counted: nothing written.
        fs::remove_file(&path).unwrap();
        counting.flush(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn a_file_that_cant_be_read_is_begun_again_and_ids_are_bounded() {
        let dir = place("bounded");
        let path = dir.join(RUNS_FILE);
        fs::write(&path, "not json").unwrap();
        assert_eq!(read(&path), None);
        let counting = Counting::default();
        for n in 0..(MOST_IDS + 10) {
            counting
                .counter(Source::Store(format!("{n:064x}")))
                .fetch_add(n as u64 + 1, Ordering::Relaxed);
        }
        counting.flush(&path).unwrap();
        let totals = read(&path).unwrap();
        assert_eq!(totals.store_ids.len(), MOST_IDS);
        assert!(!totals.store_ids.contains_key(&format!("{:064x}", 0)));
        assert_eq!(read(&dir.join("none.json")), Some(Totals::default()));
    }

    #[test]
    fn made_here_is_read_from_the_front_matter_only() {
        let dir = place("made");
        fs::write(dir.join("SKILL.md"), "---\nname: a\nmade: here\n---\n# A\n").unwrap();
        assert!(made_here(&dir));
        fs::write(dir.join("SKILL.md"), "---\nname: a\n---\nmade: here\n").unwrap();
        assert!(!made_here(&dir));
    }
}
