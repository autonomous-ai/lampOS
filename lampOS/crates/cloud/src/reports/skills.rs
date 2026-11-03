//! Skills, counted, in the opt-in health report (`docs/cloud.md`, "Skills,
//! counted"; the owner's ask on #4710, 3 Nov). An added field under
//! `lamp-report/1`, `skills`, agreed with the backend on #4710: numbers,
//! enums and a Store package's SHA-256 only, never a skill's name, words or
//! files. Read only with the person's yes, at a health report.
//!
//! - **On, and kept but not on**, by [`Source`]: read from the skills folder
//!   with `lamp-skills`' own loader and its off file, as the skills page
//!   reads them. A made skill's maker is the one on its own card
//!   (`made_by`, [`lamp_skills::usage::made_by`]), so nothing else is asked.
//! - **Runs**, by [`Source`], and by Store package: `lamp-skills`' running
//!   totals ([`lamp_skills::usage::RUNS_FILE`] in its `--state-dir`), made
//!   skills' split by the same card.
//! - **Made-skill asks by voice**, by [`Outcome`]: the voice's running totals,
//!   asked on its app-make socket (`{"counts": {}}`,
//!   `lamp_voice::made_counts`).
//!
//! A day's counts are the change in the running totals between two
//! readings, kept in [`Days`] in the reports' own state, and sent only once
//! the UTC day is over, until the report that carries it is acknowledged:
//! never a day still going, so two reports never tell a run apart. The
//! first reading only begins the count, so nothing from before it is put on
//! one day; a total that went down (a file begun again) counts from 0.
use lamp_skills::SkillKind;
use lamp_skills::usage::{Maker, Source as Found};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The most Store packages a day lists by SHA-256; the rest are summed in
/// `store_rest`. A report is cut further to fit [`super::MOST_BYTES`].
pub const STORE_MOST: usize = 50;
/// The most Store packages a kept day remembers apart; past it they are
/// summed in its rest, so the state stays small.
const DAY_STORE_MOST: usize = 64;
/// The most days kept waiting: a week, as the queue keeps a report.
const DAYS_MOST: u64 = 7;
/// How long the voice has to answer `counts`: a health report waits on it.
const VOICE_WITHIN: Duration = Duration::from_secs(1);
const DAY_S: u64 = 86_400;

/// Where a skill came from. Sent as a map's keys, so a source a later build
/// adds (`written`, `uploaded`) is a new key under the same schema, and a
/// key not there is 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Comes with the lamp.
    Core,
    /// From the Skills Store.
    Store,
    /// Made on this lamp by voice.
    MadeVoice,
    /// Made on this lamp from the owner's app.
    MadeApp,
    /// Made on this lamp before the maker was written on its card: by
    /// voice or in the app, not known.
    MadeUnknown,
    /// Pushed by a paired computer.
    Pushed,
    /// Any other installed skill (from a folder or a file).
    Other,
}

impl Source {
    const ALL: [Self; 7] = [
        Self::Core,
        Self::Store,
        Self::MadeVoice,
        Self::MadeApp,
        Self::MadeUnknown,
        Self::Pushed,
        Self::Other,
    ];

    fn key(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Store => "store",
            Self::MadeVoice => "made_voice",
            Self::MadeApp => "made_app",
            Self::MadeUnknown => "made_unknown",
            Self::Pushed => "pushed",
            Self::Other => "other",
        }
    }
}

/// What became of a made-skill ask by voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Kept as a made skill.
    Kept,
    /// Put on by a spoken yes.
    On,
    /// Made nothing.
    Refused,
    /// Asked about, and not put on.
    LeftOff,
}

impl Outcome {
    const ALL: [Self; 4] = [Self::Kept, Self::On, Self::Refused, Self::LeftOff];

    fn key(self) -> &'static str {
        match self {
            Self::Kept => "kept",
            Self::On => "on",
            Self::Refused => "refused",
            Self::LeftOff => "left_off",
        }
    }
}

/// Runs since `lamp-skills`' file began.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Runs {
    pub by_source: BTreeMap<Source, u64>,
    /// By Store package SHA-256, 64 lowercase hex only.
    pub store: BTreeMap<String, u64>,
}

/// One reading of the lamp, at a health report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    pub on: BTreeMap<Source, u32>,
    pub kept_off: BTreeMap<Source, u32>,
    /// `None` when `lamp-skills`' totals can't be read.
    pub runs: Option<Runs>,
    /// `None` when the voice did not answer.
    pub asks: Option<BTreeMap<Outcome, u64>>,
}

/// Where the lamp's skills and their counts are read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Places {
    /// `lamp-skills`' folder.
    pub folder: PathBuf,
    /// Its off file, in its `--state-dir`.
    pub off: PathBuf,
    /// Its runs' totals, in the same folder.
    pub runs: PathBuf,
    /// The voice's app-make socket.
    pub voice: PathBuf,
    /// The manage helper's socket, which deletes the counts after a no
    /// (`crate::skills::forget_counts`).
    pub manage: PathBuf,
    /// Whether add-ons run here (`LAMP_ADDONS=on`): an add-on is not on
    /// without it.
    pub addons: bool,
}

impl Places {
    /// The product's, as the skills page reads them.
    pub fn product() -> Self {
        let page = crate::skills::Places::product();
        Self {
            runs: page.off.with_file_name(lamp_skills::usage::RUNS_FILE),
            folder: page.folder,
            off: page.off,
            voice: crate::make_skill::SOCKET.into(),
            manage: page.manage,
            addons: page.addons,
        }
    }
}

/// A reading of `places`; `None` when the skills folder can't be read.
pub fn read(places: &Places) -> Option<Reading> {
    fs::read_dir(&places.folder).ok()?;
    let made = |maker: Maker| match maker {
        Maker::Voice => Source::MadeVoice,
        Maker::App => Source::MadeApp,
        Maker::Unknown => Source::MadeUnknown,
    };
    let of = |found: &Found| match found {
        Found::Core => Source::Core,
        Found::Store(_) => Source::Store,
        Found::Made(maker) => made(*maker),
        Found::Pushed => Source::Pushed,
        Found::Other => Source::Other,
    };
    let off = off_names(&places.off);
    let mut reading = Reading::default();
    for (folder, loaded) in lamp_skills::load_each(&places.folder) {
        let (source, on) = match &loaded {
            Ok(skill) => (
                of(&lamp_skills::usage::source_of(skill, &folder)),
                skill.enabled
                    && !off.contains(&skill.name)
                    && (skill.kind != SkillKind::Addon || places.addons),
            ),
            // One that doesn't load is kept, and not on.
            Err(_) => {
                let installed = lamp_skills::package::installed_from(&folder);
                let source = match (&installed, lamp_skills::usage::made_by(&folder)) {
                    (Some(installed), _) if installed.from_store => Source::Store,
                    (_, Some(maker)) => made(maker),
                    _ => Source::Other,
                };
                (source, false)
            }
        };
        let counts = if on {
            &mut reading.on
        } else {
            &mut reading.kept_off
        };
        *counts.entry(source).or_default() += 1;
    }
    reading.runs = lamp_skills::usage::read(&places.runs).map(|totals| Runs {
        by_source: BTreeMap::from([
            (Source::Core, totals.core),
            (Source::Store, totals.store),
            (Source::MadeVoice, totals.made_voice),
            (Source::MadeApp, totals.made_app),
            (Source::MadeUnknown, totals.made_unknown),
            (Source::Pushed, totals.pushed),
            (Source::Other, totals.other),
        ]),
        store: totals.store_ids,
    });
    reading.asks = ask_voice(&places.voice).and_then(|told| {
        let made = told.get("made_by_voice")?;
        Some(
            Outcome::ALL
                .into_iter()
                .map(|outcome| (outcome, made[outcome.key()].as_u64().unwrap_or_default()))
                .collect(),
        )
    });
    Some(reading)
}

/// The off file's names; none when there is none or it can't be read.
fn off_names(path: &Path) -> BTreeSet<String> {
    let mut text = String::new();
    let _ = fs::File::open(path).and_then(|file| file.take(64 * 1_024).read_to_string(&mut text));
    text.lines().map(|line| line.trim().to_owned()).collect()
}

/// The voice's `counts`, within [`VOICE_WITHIN`]; nothing when it is not
/// there (its app-make socket runs only with `LAMP_APP_MAKE_SKILL=on`) or
/// does not answer. Said nowhere: an absent socket is the usual case.
fn ask_voice(socket: &Path) -> Option<Value> {
    let mut stream = std::os::unix::net::UnixStream::connect(socket).ok()?;
    stream.set_read_timeout(Some(VOICE_WITHIN)).ok()?;
    stream.set_write_timeout(Some(VOICE_WITHIN)).ok()?;
    stream
        .write_all(format!("{}\n", json!({"counts": {}})).as_bytes())
        .ok()?;
    let mut reply = String::new();
    BufReader::new(stream.take(8_192))
        .read_line(&mut reply)
        .ok()?;
    serde_json::from_str(reply.trim()).ok()
}

/// The days' counts kept beside the queue, until acknowledged. Flat, by
/// key: `run/<source>`, `store/<sha256>`, `store_rest`, `ask/<outcome>`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Days {
    /// The running totals last read.
    last: BTreeMap<String, u64>,
    /// The kinds of total read once at least: `run`, `store`, `ask`.
    begun: BTreeSet<String>,
    /// By UTC day (days since 1970), the counts not yet acknowledged.
    days: BTreeMap<u64, BTreeMap<String, u64>>,
}

/// One UTC day as a report carries it: only ever a day that is over, its
/// counts whole.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Day {
    /// `YYYY-MM-DD`, UTC.
    pub date: String,
    /// Made-skill asks by voice; an outcome not there is 0.
    pub made_by_voice: BTreeMap<Outcome, u64>,
    /// Runs by source; a source not there is 0.
    pub runs: BTreeMap<Source, u64>,
    /// Runs by Store package SHA-256, the most run first, at most
    /// [`STORE_MOST`].
    pub store: BTreeMap<String, u64>,
    /// The Store's runs not listed in `store`.
    pub store_rest: u64,
}

/// The `skills` field of a health report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SkillsBody {
    /// Skills on now, by source; a source not there is 0.
    pub on: BTreeMap<Source, u32>,
    /// Skills kept on the lamp and not on now, by source.
    pub kept_off: BTreeMap<Source, u32>,
    /// The oldest day not yet acknowledged, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day: Option<Day>,
}

impl SkillsBody {
    /// One step smaller, for a report over [`super::MOST_BYTES`]: the
    /// least run Store package summed in the rest, then, with none left,
    /// the day left out. False when nothing is left to cut.
    pub fn cut(&mut self) -> bool {
        let Some(day) = &mut self.day else {
            return false;
        };
        let least = day
            .store
            .iter()
            .min_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
            .map(|(sha256, runs)| (sha256.clone(), *runs));
        match least {
            Some((sha256, runs)) => {
                day.store.remove(&sha256);
                day.store_rest = day.store_rest.saturating_add(runs);
            }
            None => self.day = None,
        }
        true
    }
}

/// `{prefix}/{key}` for each count of a reading, by the kind of total.
fn totals(reading: &Reading) -> Vec<(&'static str, BTreeMap<String, u64>)> {
    let mut kinds = Vec::new();
    if let Some(runs) = &reading.runs {
        kinds.push((
            "run",
            Source::ALL
                .into_iter()
                .map(|source| {
                    (
                        format!("run/{}", source.key()),
                        runs.by_source.get(&source).copied().unwrap_or_default(),
                    )
                })
                .collect(),
        ));
        kinds.push((
            "store",
            runs.store
                .iter()
                .filter(|(sha256, _)| lamp_skills::package::sha256_ok(sha256))
                .map(|(sha256, runs)| (format!("store/{sha256}"), *runs))
                .collect(),
        ));
    }
    if let Some(asks) = &reading.asks {
        kinds.push((
            "ask",
            Outcome::ALL
                .into_iter()
                .map(|outcome| {
                    (
                        format!("ask/{}", outcome.key()),
                        asks.get(&outcome).copied().unwrap_or_default(),
                    )
                })
                .collect(),
        ));
    }
    kinds
}

impl Days {
    /// The change since the last reading put on `now_s`'s UTC day.
    pub fn add(&mut self, now_s: u64, reading: &Reading) {
        let today = now_s / DAY_S;
        for (kind, now) in totals(reading) {
            let prefix = format!("{kind}/");
            if self.begun.insert(kind.to_owned()) {
                // The first reading begins the count.
                self.last.retain(|key, _| !key.starts_with(&prefix));
                self.last.extend(now);
                continue;
            }
            for (key, total) in &now {
                let was = self.last.get(key).copied().unwrap_or_default();
                // A total that went down was begun again: all of it is new.
                let new = if *total >= was { total - was } else { *total };
                if new == 0 {
                    continue;
                }
                let day = self.days.entry(today).or_default();
                let key = if kind == "store"
                    && !day.contains_key(key)
                    && day.keys().filter(|key| key.starts_with("store/")).count() >= DAY_STORE_MOST
                {
                    "store_rest".to_owned()
                } else {
                    key.clone()
                };
                let count = day.entry(key).or_default();
                *count = count.saturating_add(new);
            }
            self.last.retain(|key, _| !key.starts_with(&prefix));
            self.last.extend(now);
        }
        let oldest = today.saturating_sub(DAYS_MOST);
        self.days.retain(|day, _| *day >= oldest);
    }

    /// The oldest day that is over and not yet acknowledged, as a report
    /// carries it, and its number (so its acknowledgement lets it go).
    /// Today is never one: a day still going is kept, never sent.
    pub fn oldest(&self, now_s: u64) -> Option<(Day, u64)> {
        let (number, counts) = self.days.iter().next()?;
        if *number >= now_s / DAY_S {
            return None;
        }
        let mut store: Vec<(&str, u64)> = counts
            .iter()
            .filter_map(|(key, runs)| Some((key.strip_prefix("store/")?, *runs)))
            .collect();
        store.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        let mut store_rest = counts.get("store_rest").copied().unwrap_or_default();
        for (_, runs) in store.iter().skip(STORE_MOST) {
            store_rest = store_rest.saturating_add(*runs);
        }
        let date = crate::online::rfc3339(
            std::time::UNIX_EPOCH + Duration::from_secs(number.saturating_mul(DAY_S)),
        );
        let day = Day {
            date: date.get(..10).unwrap_or_default().to_owned(),
            made_by_voice: Outcome::ALL
                .into_iter()
                .filter_map(|outcome| {
                    let count = *counts.get(&format!("ask/{}", outcome.key()))?;
                    (count > 0).then_some((outcome, count))
                })
                .collect(),
            runs: Source::ALL
                .into_iter()
                .filter_map(|source| {
                    let count = *counts.get(&format!("run/{}", source.key()))?;
                    (count > 0).then_some((source, count))
                })
                .collect(),
            store: store
                .into_iter()
                .take(STORE_MOST)
                .map(|(sha256, runs)| (sha256.to_owned(), runs))
                .collect(),
            store_rest,
        };
        Some((day, *number))
    }

    /// The report carrying `day` was acknowledged.
    pub fn acked(&mut self, day: u64) {
        self.days.remove(&day);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_792_296_000; // 2026-10-18T04:00:00Z

    fn reading(core: u64, store: &[(&str, u64)], refused: u64) -> Reading {
        Reading {
            on: BTreeMap::from([(Source::Core, 3)]),
            kept_off: BTreeMap::new(),
            runs: Some(Runs {
                by_source: BTreeMap::from([(Source::Core, core)]),
                store: store
                    .iter()
                    .map(|(sha256, runs)| (sha256.to_string(), *runs))
                    .collect(),
            }),
            asks: Some(BTreeMap::from([(Outcome::Refused, refused)])),
        }
    }

    #[test]
    fn a_day_is_the_change_between_readings_and_goes_on_its_acknowledgement() {
        let a = "a".repeat(64);
        let mut days = Days::default();
        // The first reading begins the count: nothing from before it.
        days.add(NOW, &reading(100, &[(&a, 40)], 7));
        assert_eq!(days.oldest(NOW), None);
        days.add(NOW + 60, &reading(103, &[(&a, 42)], 8));
        // Today is never sent, however far it has come.
        assert_eq!(days.oldest(NOW + 60), None);
        // A total begun again counts from 0; the next day is its own.
        days.add(NOW + DAY_S, &reading(2, &[(&a, 42)], 8));
        let (day, over) = days.oldest(NOW + DAY_S).unwrap();
        assert_eq!(day.date, "2026-10-18");
        assert_eq!(day.runs, BTreeMap::from([(Source::Core, 3)]));
        assert_eq!(day.store, BTreeMap::from([(a.clone(), 2)]));
        assert_eq!(day.made_by_voice, BTreeMap::from([(Outcome::Refused, 1)]));
        days.acked(over);
        assert_eq!(days.oldest(NOW + DAY_S), None);
        // A part not read keeps its count where it was.
        let mut quiet = reading(2, &[], 8);
        quiet.runs = None;
        days.add(NOW + DAY_S + 1, &quiet);
        days.add(NOW + DAY_S + 2, &reading(2, &[(&a, 43)], 8));
        let (next, _) = days.oldest(NOW + 2 * DAY_S).unwrap();
        assert_eq!(next.date, "2026-10-19");
        assert_eq!(next.runs, BTreeMap::from([(Source::Core, 2)]));
        assert_eq!(next.store[&a], 1);
    }

    /// Thirteen's HOLD on #5096: six skills made, the first in the app,
    /// read before and after, with the voice answering and not. Each made
    /// skill's maker is on its own card, so `run/made_app` and
    /// `run/made_voice` change only by the runs between readings, whatever
    /// the voice keeps or says; a card from before the marker is
    /// `made_unknown`.
    #[test]
    fn made_runs_split_by_the_card_whoever_answers() {
        use std::sync::atomic::Ordering;
        let page = crate::skills::tests::places("made-split");
        let runs = page.off.with_file_name(lamp_skills::usage::RUNS_FILE);
        let silent = Places {
            folder: page.folder.clone(),
            off: page.off.clone(),
            runs: runs.clone(),
            // Nothing listens: the voice doesn't answer.
            voice: page.off.with_file_name("none.sock"),
            manage: page.manage.clone(),
            addons: false,
        };
        let answering = Places {
            voice: page.off.with_file_name("v.sock"),
            ..silent.clone()
        };
        let listener = std::os::unix::net::UnixListener::bind(&answering.voice).unwrap();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut line = String::new();
                let _ = BufReader::new(stream.try_clone().unwrap()).read_line(&mut line);
                let reply =
                    json!({"made_by_voice": {"kept": 5, "on": 5, "refused": 0, "left_off": 0}});
                let _ = writeln!(stream, "{reply}");
            }
        });
        let make = |name: &str, front: &str| {
            crate::skills::tests::skill(&page, name, name, front, "[permissions]\nbody = true");
        };
        let counting = lamp_skills::usage::Counting::default();
        let run = |name: &str, times: u64| {
            for (folder, loaded) in lamp_skills::load_each(&page.folder) {
                if let Ok(skill) = loaded
                    && skill.name == name
                {
                    let source = lamp_skills::usage::source_of(&skill, &folder);
                    counting.counter(source).fetch_add(times, Ordering::Relaxed);
                }
            }
            counting.flush(&runs).unwrap();
        };
        let made = |reading: &Reading| {
            let runs = reading.runs.as_ref().unwrap();
            [Source::MadeApp, Source::MadeVoice, Source::MadeUnknown]
                .map(|source| runs.by_source.get(&source).copied().unwrap_or_default())
        };
        make("first-app", "made: here\nmade_by: app\n");
        make("from-before", "made: here\n");
        run("first-app", 3);
        run("from-before", 1);
        let mut days = Days::default();
        let before = read(&answering).unwrap();
        assert_eq!(made(&before), [3, 0, 1]);
        days.add(NOW, &before);
        // Five more by voice: six made, the app's the oldest.
        for n in 1..=5 {
            let name = format!("voice-{n}");
            make(&name, "made: here\nmade_by: voice\n");
            run(&name, 1);
        }
        run("first-app", 2);
        let after = read(&answering).unwrap();
        assert_eq!(made(&after), [5, 5, 1]);
        assert_eq!(
            after.on,
            BTreeMap::from([
                (Source::MadeVoice, 5),
                (Source::MadeApp, 1),
                (Source::MadeUnknown, 1)
            ])
        );
        days.add(NOW + 60, &after);
        // The voice doesn't answer: the split is the same, only its asks
        // unread.
        run("first-app", 1);
        let unanswered = read(&silent).unwrap();
        assert_eq!(unanswered.asks, None);
        assert_eq!(made(&unanswered), [6, 5, 1]);
        assert_eq!(unanswered.on, after.on);
        days.add(NOW + 120, &unanswered);
        // And answering again: nothing moves back.
        run("voice-5", 1);
        let again = read(&answering).unwrap();
        assert_eq!(made(&again), [6, 6, 1]);
        days.add(NOW + 180, &again);
        let (day, _) = days.oldest(NOW + DAY_S).unwrap();
        assert_eq!(
            day.runs,
            BTreeMap::from([(Source::MadeVoice, 6), (Source::MadeApp, 3)])
        );
    }

    #[test]
    fn the_store_list_is_the_top_fifty_and_the_rest_summed() {
        let mut days = Days::default();
        let shas: Vec<String> = (0..60).map(|n| format!("{n:064x}")).collect();
        let zero: Vec<(&str, u64)> = shas.iter().map(|sha| (sha.as_str(), 0)).collect();
        let runs: Vec<(&str, u64)> = shas
            .iter()
            .enumerate()
            .map(|(n, sha)| (sha.as_str(), n as u64 + 1))
            .collect();
        days.add(NOW, &reading(0, &zero, 0));
        days.add(NOW + 1, &reading(0, &runs, 0));
        let (mut day, _) = days.oldest(NOW + DAY_S).unwrap();
        assert_eq!(day.store.len(), STORE_MOST);
        // The ten least run, 1 to 10, summed.
        assert_eq!(day.store_rest, 55);
        assert!(!day.store.contains_key(&shas[0]));
        assert!(day.store.contains_key(&shas[59]));
        let total: u64 = day.store.values().sum::<u64>() + day.store_rest;
        // Cut one step: the least listed goes to the rest, the total kept.
        let mut body = SkillsBody {
            on: BTreeMap::new(),
            kept_off: BTreeMap::new(),
            day: Some(day.clone()),
        };
        assert!(body.cut());
        day = body.day.clone().unwrap();
        assert_eq!(day.store.len(), STORE_MOST - 1);
        assert_eq!(day.store.values().sum::<u64>() + day.store_rest, total);
        // With the list empty, the day goes; then nothing is left to cut.
        while body.day.as_ref().is_some_and(|day| !day.store.is_empty()) {
            assert!(body.cut());
        }
        assert!(body.cut());
        assert_eq!(body.day, None);
        assert!(!body.cut());
    }
}
