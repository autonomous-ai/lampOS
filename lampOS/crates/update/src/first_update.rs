//! The first update after a setup (#6284), behind `LAMP_FIRST_UPDATE`
//! (exactly `on`; anything else changes nothing).
//!
//! Setup writes [`lamp_msgs::FIRST_UPDATE_FILE`] with its marker. While it
//! is there and owed ([`lamp_msgs::FirstUpdate::owed`], 20 minutes from its
//! `at_unix_ms`), `serve` starts a round at once instead of after its
//! `--every-s`, and the setup walk does not hold that round: the walk has
//! not begun, as the voice holds its beginning until the file is gone.
//! Every other rule of a round stays: the lock, the signature checks, the
//! quiet moment, the watch and the put-back.
//!
//! One such round per file: any end of it removes the file, but for
//! lamp-update switching itself (`self-switched`, `climbs`), whose new
//! build starts with a round of its own and takes the file then. At most
//! [`MOST_ROUNDS`] rounds per file, counted in the records, so a run of
//! switches of its own never keeps the walk waiting. A file past its 20
//! minutes is removed too. One that can't be read is left as it is: no
//! reader counts it as owed.
use crate::round::{Config, Outcome};
use lamp_msgs::{FIRST_UPDATE_FILE, FirstUpdate};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// The switch, in lamp-update's environment: exactly `on` turns it on.
pub const SWITCH: &str = "LAMP_FIRST_UPDATE";
/// Without the switch set, a product lamp has the first update (#6284, the
/// product's call on 11 Nov): lamp-update's own unit never changes in a
/// home, so the switch can't reach it there.
pub const PRODUCT_DEFAULT: bool = true;
/// Rounds started for one file, in all, across lamp-update's own switches.
pub const MOST_ROUNDS: u32 = 3;
/// In the records folder: `<at_unix_ms> <rounds>` for the last file seen.
const ROUNDS: &str = "first-update-rounds";
/// Whether the first update is owed and the walk has not begun: only then
/// is its round forced and not held by the walk (V2's P2 on #6299: a walk
/// begun after a first round that ended keeps every rule of a round, so
/// no later round switches the lamp mid-walk).
pub fn before_walk(config: &Config, now_unix_ms: u64) -> bool {
    owed(config, now_unix_ms) && !crate::walk::begun(config)
}

/// How long `serve` waits after a round that couldn't reach the channel
/// before setup is done, instead of its `--every-s`.
pub const BEFORE_SETUP_AGAIN: std::time::Duration = std::time::Duration::from_secs(30);

/// Whether setup is not done yet on a lamp with the first update on: no
/// marker. A new lamp is then on its setup hotspot, with no internet.
pub fn before_setup(config: &Config) -> bool {
    config.first_update && !crate::setup_lifecycle::done(&config.root)
}

/// Whether a round's end is kept as the lamp's last (`update/status`, which
/// lamp-cloud sends the app). Not a round that couldn't reach the channel
/// before setup was done: the lamp had no internet yet, and the app would
/// show "Couldn't check" after setup for a round no one could have passed.
/// It is said in the journal, asked again after [`BEFORE_SETUP_AGAIN`],
/// and the first update after setup starts its own round anyway.
pub fn kept(before_setup: bool, outcome: &Outcome) -> bool {
    !(before_setup && matches!(outcome, Outcome::Unreachable(_)))
}

/// What a round says when the walk would have held it but the first
/// update is owed.
pub const NOT_HELD: &str =
    "the setup walk has not begun: the first update after setup is owed, so the round is not held";

/// Whether `value` turns the switch on.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Whether the first update is owed at `now_unix_ms` on the lamp under
/// `config`: the switch on and the file there, read and within its bound.
pub fn owed(config: &Config, now_unix_ms: u64) -> bool {
    config.first_update
        && FirstUpdate::read(&config.on(FIRST_UPDATE_FILE)).is_some_and(|file| {
            file.owed(now_unix_ms)
                && rounds(&config.on(&config.records), file.at_unix_ms) < MOST_ROUNDS
        })
}

/// `serve`'s side of it: which file it started a round for, and whether
/// the round under way is that round.
pub struct First {
    file: PathBuf,
    records: PathBuf,
    /// The file (its `at_unix_ms`) this process started a round for.
    started: Option<u64>,
    /// The round under way is the first update's: the file's
    /// `at_unix_ms` and the round's number for it.
    this_round: Option<(u64, u32)>,
    /// A file that could not be removed, said once.
    unremovable: Option<u64>,
}

impl First {
    /// For a `serve` that acts, with the switch on in `config`; none
    /// otherwise, and then nothing here is ever looked at.
    pub fn new(config: &Config) -> Option<Self> {
        config.first_update.then(|| Self {
            file: config.on(FIRST_UPDATE_FILE),
            records: config.on(&config.records),
            started: None,
            this_round: None,
            unremovable: None,
        })
    }

    /// Looked at between rounds: whether a round should start now for the
    /// first update. A file past its bound is removed here.
    pub fn due(&mut self, now_unix_ms: u64, say: &mut dyn FnMut(&str)) -> bool {
        self.look(now_unix_ms, say)
            .is_some_and(|at| self.started != Some(at))
    }

    /// Before each round: whether it is the first update's, counted then.
    pub fn begin(&mut self, now_unix_ms: u64, say: &mut dyn FnMut(&str)) -> bool {
        self.this_round = None;
        let Some(at) = self.look(now_unix_ms, say) else {
            return false;
        };
        if self.started == Some(at) {
            return false;
        }
        self.started = Some(at);
        let round = rounds(&self.records, at) + 1;
        let kept = crate::records::write_whole(
            &self.records.join(ROUNDS),
            format!("{at} {round}\n").as_bytes(),
            0o644,
        );
        if let Err(error) = kept {
            // Uncounted, a run of switches of its own could hold the walk
            // with no end: the file goes now, and this round is as any other.
            say(&format!(
                "the first update after setup: its rounds cannot be counted ({error})"
            ));
            self.remove(at, "its rounds cannot be counted", say);
            return false;
        }
        say(&format!(
            "the first update after setup is owed: a round now, round {round} of {MOST_ROUNDS}"
        ));
        self.this_round = Some((at, round));
        true
    }

    /// After each round: the file removed when it was the first update's,
    /// whatever its end, but for lamp-update switching itself within
    /// [`MOST_ROUNDS`], whose new build's round takes it.
    pub fn ended(&mut self, outcome: &Outcome, say: &mut dyn FnMut(&str)) {
        let Some((at, round)) = self.this_round.take() else {
            return;
        };
        if matches!(
            outcome,
            Outcome::SelfSwitched { .. } | Outcome::Climbs { .. }
        ) && round < MOST_ROUNDS
        {
            say(&format!(
                "the first update after setup: kept for the new lamp-update's round ({} ended round {round} of {MOST_ROUNDS})",
                outcome.kind()
            ));
            return;
        }
        // Kept as spent in lamp-update's own records first: a lamp whose
        // unit (never carried by a round) can't write setup's folder still
        // owes the file no more, and the voice reads the round's end on the
        // bus (#6284).
        self.spend(at, say);
        self.remove(at, &format!("its round ended {}", outcome.kind()), say);
    }

    fn spend(&mut self, at: u64, say: &mut dyn FnMut(&str)) {
        let kept = crate::records::write_whole(
            &self.records.join(ROUNDS),
            format!("{at} {MOST_ROUNDS}\n").as_bytes(),
            0o644,
        );
        if let Err(error) = kept {
            say(&format!(
                "the first update after setup: its end cannot be kept ({error})"
            ));
        }
    }

    /// The file's `at_unix_ms` while it is owed. One past its bound, or
    /// with every round spent, is removed; one that can't be read is left.
    fn look(&mut self, now_unix_ms: u64, say: &mut dyn FnMut(&str)) -> Option<u64> {
        let file = FirstUpdate::read(&self.file)?;
        if !file.owed(now_unix_ms) {
            self.remove(file.at_unix_ms, "past its 20 minutes", say);
            return None;
        }
        if self.started != Some(file.at_unix_ms)
            && rounds(&self.records, file.at_unix_ms) >= MOST_ROUNDS
        {
            self.remove(file.at_unix_ms, "every round of it spent", say);
            return None;
        }
        Some(file.at_unix_ms)
    }

    /// Removes the file, saying why; one already gone is fine.
    fn remove(&mut self, at: u64, why: &str, say: &mut dyn FnMut(&str)) {
        match fs::remove_file(&self.file) {
            Ok(()) => say(&format!(
                "the first update after setup: {why}, its file removed"
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                if self.unremovable != Some(at) {
                    self.unremovable = Some(at);
                    say(&format!(
                        "the first update after setup: {why}, but its file cannot be removed: {error}"
                    ));
                }
            }
        }
    }
}

/// The rounds started for the file of `at` so far, kept in `records`.
fn rounds(records: &Path, at: u64) -> u32 {
    let text = fs::read_to_string(records.join(ROUNDS)).unwrap_or_default();
    let mut words = text.split_whitespace();
    match (words.next(), words.next()) {
        (Some(file), Some(rounds)) if file.parse::<u64>().ok() == Some(at) => {
            rounds.parse().unwrap_or(MOST_ROUNDS)
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::unix_ms;
    use std::time::Duration;

    struct Lamp {
        _folder: tempfile::TempDir,
        config: Config,
    }

    impl Lamp {
        fn new(switch: Option<&str>) -> Self {
            let folder = tempfile::tempdir().unwrap();
            let config = Config {
                root: folder.path().to_owned(),
                units: "/etc/systemd/system/".into(),
                records: "/var/lib/lampos-update/".into(),
                channel: "lamp-voice".into(),
                channel_url: "https://channel.test/".into(),
                key: [0; 32],
                lamp: "lamp-06c1".into(),
                apply: true,
                quiet_wait: Duration::ZERO,
                watch: Duration::ZERO,
                keep_free: 0,
                self_switch: false,
                running: None,
                updater: crate::manifest::UPDATER,
                models: false,
                models_file: false,
                settings: false,
                product_settings: false,
                image: false,
                first_update: on(switch),
                writer_from: None,
            };
            fs::create_dir_all(config.on(&config.records)).unwrap();
            fs::create_dir_all(config.on(FIRST_UPDATE_FILE).parent().unwrap()).unwrap();
            Self {
                _folder: folder,
                config,
            }
        }

        fn owe(&self, at_unix_ms: u64) {
            fs::write(
                self.config.on(FIRST_UPDATE_FILE),
                format!(r#"{{"at_unix_ms":{at_unix_ms}}}"#),
            )
            .unwrap();
        }

        fn there(&self) -> bool {
            self.config.on(FIRST_UPDATE_FILE).exists()
        }

        fn first(&self) -> First {
            First::new(&self.config).unwrap()
        }
    }

    fn self_switched() -> Outcome {
        Outcome::SelfSwitched {
            sequence: 2,
            from: "a".repeat(64),
            to: "b".repeat(64),
        }
    }

    #[test]
    fn only_exactly_on_turns_it_on_and_off_nothing_is_looked_at() {
        for value in [None, Some(""), Some("off"), Some("ON"), Some("on ")] {
            let lamp = Lamp::new(value);
            lamp.owe(unix_ms());
            assert!(First::new(&lamp.config).is_none(), "{value:?}");
            assert!(!owed(&lamp.config, unix_ms()), "{value:?}");
            assert!(lamp.there());
        }
        assert!(on(Some("on")));
    }

    /// Before setup's marker, a round that couldn't reach the channel is
    /// not kept, so the app never shows "Couldn't check" for the hotspot's
    /// round; every other end is, and once setup is done every one is.
    #[test]
    fn an_unreachable_round_before_setup_is_not_kept() {
        let unreachable = Outcome::Unreachable("github.com: failed to lookup".into());
        let off = Lamp::new(None);
        assert!(!before_setup(&off.config));
        let lamp = Lamp::new(Some("on"));
        assert!(before_setup(&lamp.config));
        assert!(!kept(true, &unreachable));
        assert!(kept(true, &Outcome::UpToDate(7)));
        assert!(kept(false, &unreachable));
        let marker = lamp.config.root.join("var/lib/lampos-setup/done");
        fs::create_dir_all(marker.parent().unwrap()).unwrap();
        fs::write(&marker, "").unwrap();
        assert!(!before_setup(&lamp.config));
    }

    #[test]
    fn an_owed_file_starts_a_round_at_once_and_only_one() {
        let lamp = Lamp::new(Some("on"));
        let mut first = lamp.first();
        let said = std::cell::RefCell::new(Vec::<String>::new());
        let mut say = |line: &str| said.borrow_mut().push(line.to_owned());
        let now = unix_ms();
        // No file: nothing to start.
        assert!(!first.due(now, &mut say));
        assert!(!first.begin(now, &mut say));
        lamp.owe(now);
        assert!(owed(&lamp.config, now));
        assert!(first.due(now, &mut say));
        assert!(first.begin(now, &mut say));
        // Its round under way, or one whose removal failed: never another.
        assert!(!first.due(now, &mut say));
        assert!(!first.begin(now, &mut say));
        assert_eq!(
            *said.borrow(),
            ["the first update after setup is owed: a round now, round 1 of 3"]
        );
    }

    #[test]
    fn the_file_is_removed_after_up_to_date_and_after_a_failure() {
        for outcome in [
            Outcome::UpToDate(2),
            Outcome::Failed("the disk".into()),
            Outcome::NotQuiet("someone talks".into()),
            Outcome::Unreachable("timed out".into()),
        ] {
            let lamp = Lamp::new(Some("on"));
            let mut first = lamp.first();
            let now = unix_ms();
            lamp.owe(now);
            let said = std::cell::RefCell::new(Vec::<String>::new());
            let mut say = |line: &str| said.borrow_mut().push(line.to_owned());
            assert!(first.begin(now, &mut say));
            first.ended(&outcome, &mut say);
            assert!(!lamp.there(), "{outcome:?}");
            assert_eq!(
                said.borrow()[1],
                format!(
                    "the first update after setup: its round ended {}, its file removed",
                    outcome.kind()
                )
            );
            // A round that is not the first update's removes nothing.
            lamp.owe(now + 1);
            first.ended(&outcome, &mut say);
            assert!(lamp.there());
        }
    }

    /// A lamp in a home runs lamp-update from the 7071 image's unit, which
    /// no round rewrites and which can't write setup's folder: the end is
    /// kept in lamp-update's records, so the file is owed no more though it
    /// stays, and no later look starts a round for it.
    #[test]
    fn an_end_is_kept_in_the_records_when_the_file_cannot_be_removed() {
        use std::os::unix::fs::PermissionsExt;
        let lamp = Lamp::new(Some("on"));
        let now = unix_ms();
        lamp.owe(now);
        let folder = lamp.config.on(FIRST_UPDATE_FILE);
        let folder = folder.parent().unwrap().to_owned();
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
        let said = std::cell::RefCell::new(Vec::<String>::new());
        let mut say = |line: &str| said.borrow_mut().push(line.to_owned());
        let mut first = lamp.first();
        assert!(first.begin(now, &mut say));
        assert!(owed(&lamp.config, now));
        first.ended(&Outcome::Switched(2), &mut say);
        assert!(lamp.there());
        assert!(!owed(&lamp.config, now + 1_000));
        let mut next = lamp.first();
        assert!(!next.due(now + 2_000, &mut say));
        assert!(!next.begin(now + 2_000, &mut say));
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn the_file_is_kept_after_switching_itself_until_the_third_round() {
        let lamp = Lamp::new(Some("on"));
        let now = unix_ms();
        lamp.owe(now);
        let said = std::cell::RefCell::new(Vec::<String>::new());
        let mut say = |line: &str| said.borrow_mut().push(line.to_owned());
        // Each new lamp-update is a new process: a new `First`.
        for round in 1..MOST_ROUNDS {
            let mut first = lamp.first();
            assert!(first.begin(now, &mut say), "round {round}");
            first.ended(&self_switched(), &mut say);
            assert!(lamp.there(), "round {round}");
        }
        let mut first = lamp.first();
        assert!(first.begin(now, &mut say));
        first.ended(&self_switched(), &mut say);
        assert!(!lamp.there());
        assert_eq!(
            said.borrow()[1],
            "the first update after setup: kept for the new lamp-update's round (self-switched ended round 1 of 3)"
        );
        // A file left once every round is spent is removed, not rounded on.
        lamp.owe(now);
        let mut first = lamp.first();
        assert!(!first.due(now, &mut say));
        assert!(!lamp.there());
        assert_eq!(
            said.borrow().last().unwrap(),
            "the first update after setup: every round of it spent, its file removed"
        );
        // A new setup's file is counted afresh.
        lamp.owe(now + 1);
        assert!(lamp.first().begin(now + 1, &mut say));
    }

    #[test]
    fn a_stale_file_is_removed_and_starts_nothing() {
        let lamp = Lamp::new(Some("on"));
        let now = unix_ms();
        lamp.owe(now - lamp_msgs::FIRST_UPDATE_WITHIN_MS);
        let said = std::cell::RefCell::new(Vec::<String>::new());
        let mut say = |line: &str| said.borrow_mut().push(line.to_owned());
        assert!(!owed(&lamp.config, now));
        let mut first = lamp.first();
        assert!(!first.due(now, &mut say));
        assert!(!lamp.there());
        assert_eq!(
            *said.borrow(),
            ["the first update after setup: past its 20 minutes, its file removed"]
        );
        // One that can't be read is left as it is, and owes nothing.
        fs::write(lamp.config.on(FIRST_UPDATE_FILE), "{").unwrap();
        assert!(!first.due(now, &mut say));
        assert!(lamp.there());
    }
}
