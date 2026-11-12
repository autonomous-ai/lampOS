//! The app's Check for update (#4153): lamp-cloud says one `update/check`
//! (`lamp_msgs::UpdateCheck`) when the owner taps it, and `lamp-update
//! serve` starts its next round then instead of after `--every-s`.
//!
//! Only the wait between rounds is cut short. The round is the same round:
//! the same lock, a quiet moment before any switch, `--apply` as set. A
//! check that came while a round ran is dropped, never queued for after it,
//! and at most one a minute starts a round. Only a `serve` with `--apply on`
//! subscribes; with nobody saying a check, it waits as before.
//!
//! With `LAMP_APP_ENGINES=on` (#6240, [`crate::engines`]) the same wait
//! also hears the app's tap to install a missing agent: one taken ends the
//! wait as a check does, and the next round's Hermes step takes it up.
//! One said while a round ran is kept as asked, without ending a wait.
use crate::world::Ears;
use lamp_bus::{Bus, Subscriber};
use lamp_msgs::UpdateCheck;
use std::{
    path::Path,
    time::{Duration, Instant},
};

/// How often the wait between rounds looks for a check: a tap starts a
/// round within about this, plus the bus's own wake.
pub const LOOK_EVERY: Duration = Duration::from_secs(1);

/// What ended [`AppCheck::wait_or`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Woken {
    /// The whole wait.
    Waited,
    /// The app's check.
    Asked,
    /// The first update after setup is owed (#6284).
    Due,
}

/// `serve`'s side of the check: its subscription, and when a check last
/// started a round.
pub struct AppCheck {
    checks: Option<Subscriber>,
    last: Option<Instant>,
    /// [`LOOK_EVERY`] on a lamp; shorter only in tests, so none sleeps
    /// for whole seconds (#1806).
    look: Duration,
    /// The app's taps to install an agent (#6240), with its switch on.
    engines: Option<crate::engines::Asks>,
    /// The sequences of refused builds the owner asked to retry (#6266),
    /// heard with the checks that started the round and not yet taken.
    retries: Vec<u64>,
}

/// Whether the first update is due, saying what it does (#6284).
pub type Due<'a> = dyn FnMut(&mut dyn FnMut(&str)) -> bool + 'a;

impl AppCheck {
    /// Subscribed to `update/check` on the bus at `root`. A bus or topic
    /// that cannot be used is said once, and no check is ever heard: the
    /// wait is then exactly as before.
    pub fn new(root: &Path, say: &mut dyn FnMut(&str)) -> Self {
        let checks = Bus::new(root).and_then(|bus| {
            bus.subscribe_typed(UpdateCheck::TOPIC, crate::world::NAME, UpdateCheck::TYPE)
        });
        let checks = match checks {
            Ok(checks) => Some(checks),
            Err(error) => {
                say(&format!(
                    "{} cannot be subscribed to: {error}",
                    UpdateCheck::TOPIC
                ));
                None
            }
        };
        Self {
            checks,
            last: None,
            look: LOOK_EVERY,
            engines: None,
            retries: Vec::new(),
        }
    }

    /// The same check, hearing the app's taps to install an agent too
    /// (#6240, `LAMP_APP_ENGINES=on`).
    pub fn with_engines(mut self, asks: crate::engines::Asks) -> Self {
        self.engines = Some(asks);
        self
    }

    /// The same check, looking every `look` instead of [`LOOK_EVERY`].
    #[cfg(test)]
    fn looking_every(mut self, look: Duration) -> Self {
        self.look = look;
        self
    }

    /// Drops every check that came while a round ran: a round never starts
    /// another, and the app reads the round it asked for on `update/status`.
    /// A tap to install an agent said meanwhile is kept as asked (#6240):
    /// the install waits for a round anyway.
    pub fn forget(&mut self, say: &mut dyn FnMut(&str)) {
        self.waiting();
        self.retries.clear();
        if let Some(engines) = &mut self.engines {
            engines.take(Instant::now(), say);
        }
    }

    /// Whether a check came since the last look and may start a round at
    /// `now`: every one waiting is taken, and one within
    /// [`UpdateCheck::AGAIN_AFTER`] of the last that started a round is
    /// dropped.
    pub fn asked(&mut self, now: Instant) -> bool {
        if !self.waiting() {
            return false;
        }
        if self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) < UpdateCheck::AGAIN_AFTER)
        {
            // Dropped with the check, as the check is.
            self.retries.clear();
            return false;
        }
        self.last = Some(now);
        true
    }

    /// The refused builds the owner asked to retry (#6266) with the checks
    /// that started this round: taken once, for the caller to drop their
    /// refusals before the round.
    pub fn take_retries(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.retries)
    }

    /// Takes every check waiting; whether there was one that reads.
    fn waiting(&mut self) -> bool {
        let Some(checks) = &mut self.checks else {
            return false;
        };
        let mut any = false;
        while let Ok(Some(message)) = checks.recv(Duration::ZERO) {
            if let Ok(check) = message.parse::<UpdateCheck>() {
                any = true;
                self.retries.extend(check.retry);
            }
        }
        any
    }

    /// The wait between rounds with the first update after setup on
    /// (#6284, crate::first_update): as [`AppCheck::wait`], and `due`
    /// looked at first and then after each look, every [`LOOK_EVERY`] even
    /// without a subscription. A check or a tap to install an agent taken
    /// ends it as [`Woken::Asked`], `due` as [`Woken::Due`].
    pub fn wait_or(
        &mut self,
        ears: &mut dyn Ears,
        every: Duration,
        say: &mut dyn FnMut(&str),
        due: &mut Due<'_>,
    ) -> Woken {
        if due(say) {
            return Woken::Due;
        }
        let until = Instant::now() + every;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Woken::Waited;
            }
            let _ = ears.listen(left.min(self.look));
            let now = Instant::now();
            let installs = self
                .engines
                .as_mut()
                .is_some_and(|engines| engines.take(now, say));
            if self.asked(now) || installs {
                return Woken::Asked;
            }
            if due(say) {
                return Woken::Due;
            }
        }
    }

    /// The wait between rounds: `ears` listen for `every`, as before, and
    /// a check or a tap to install an agent taken ends it early, true.
    /// Without a subscription it is one listen of `every`.
    pub fn wait(
        &mut self,
        ears: &mut dyn Ears,
        every: Duration,
        say: &mut dyn FnMut(&str),
    ) -> bool {
        let engines = self
            .engines
            .as_ref()
            .is_some_and(crate::engines::Asks::listens);
        if self.checks.is_none() && !engines {
            let _ = ears.listen(every);
            return false;
        }
        let until = Instant::now() + every;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            let _ = ears.listen(left.min(self.look));
            let now = Instant::now();
            let installs = self
                .engines
                .as_mut()
                .is_some_and(|engines| engines.take(now, say));
            if self.asked(now) || installs {
                return true;
            }
        }
    }
}

/// The owner's retry of refused builds (#6266): each of `sequences` that is
/// refused is dropped from the state, under the records' lock, so the round
/// that follows takes that build again. A sequence that is not refused is
/// ignored, and the round is a plain check. A build that is really unwell is
/// refused again by the round itself, so a retry never loops. Said, one line
/// each.
pub fn drop_refusals(
    records: &crate::records::Records,
    sequences: &[u64],
    say: &mut dyn FnMut(&str),
) {
    if sequences.is_empty() {
        return;
    }
    // Not while another lamp-update holds the records: the retry is then a
    // plain check, and the app asks again (the round is not going to start).
    let Ok(_lock) = records.lock() else {
        say("a retry of a refused build: another lamp-update is at work, so it is a plain check");
        return;
    };
    let Ok(mut state) = records.state(say) else {
        return;
    };
    let mut dropped = false;
    for sequence in sequences {
        if state.forget_refusal(*sequence) {
            say(&format!(
                "the owner asked to retry build {sequence}: its refusal is dropped"
            ));
            dropped = true;
        } else {
            say(&format!(
                "the owner asked to retry build {sequence}: it is not refused, so a plain check"
            ));
        }
    }
    if dropped && let Err(error) = records.save_state(&state) {
        say(&format!("the refusals could not be saved: {error}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quiet::Heard;
    use lamp_bus::Publisher;
    use lamp_msgs::Health;

    /// Ears that hear nothing and only take the time, counting the listens.
    #[derive(Default)]
    struct Waits(Vec<Duration>);

    impl Ears for Waits {
        fn listen(&mut self, wait: Duration) -> Vec<Heard> {
            std::thread::sleep(wait);
            self.0.push(wait);
            Vec::new()
        }
        fn health(&mut self) -> Option<Health> {
            None
        }
    }

    fn cloud(bus: &Bus) -> Publisher {
        let publisher = bus
            .publisher_declared(UpdateCheck::TOPIC, &UpdateCheck::declaration(), "cloud")
            .unwrap();
        // A publisher finds a new subscriber within 200 ms.
        std::thread::sleep(Duration::from_millis(250));
        publisher
    }

    /// How often the tests' waits look: short, so a test takes well under
    /// #1806's 3 s, with the same shape as [`LOOK_EVERY`] on a lamp.
    const LOOK: Duration = Duration::from_millis(50);

    #[test]
    fn a_check_between_rounds_starts_the_next_at_once_and_at_most_once_a_minute() {
        let root = tempfile::tempdir().unwrap();
        let mut said = Vec::new();
        let mut check =
            AppCheck::new(root.path(), &mut |line| said.push(line.to_owned())).looking_every(LOOK);
        assert!(said.is_empty(), "{said:?}");
        let mut cloud = cloud(&Bus::new(root.path()).unwrap());
        let mut ears = Waits::default();

        // Nothing asked: the whole wait, in looks of `LOOK`.
        let started = Instant::now();
        assert!(!check.wait(&mut ears, Duration::from_millis(120), &mut |_| {}));
        assert!(started.elapsed() >= Duration::from_millis(120));
        // How many looks depends on the runner's load (one late wake can
        // outlast the whole wait), so only their shape is asked: every look
        // but the last is a whole `LOOK`, none longer.
        let (last, whole) = ears.0.split_last().unwrap();
        assert!(whole.iter().all(|look| *look == LOOK), "{:?}", ears.0);
        assert!(*last <= LOOK, "{:?}", ears.0);

        // A check said during the wait ends it within about a look.
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            cloud.publish_json(0, &UpdateCheck::default()).unwrap();
            cloud
        });
        let started = Instant::now();
        assert!(check.wait(&mut ears, Duration::from_secs(60), &mut |_| {}));
        let took = started.elapsed();
        assert!(took < Duration::from_secs(1), "{took:?}");
        let mut cloud = sender.join().unwrap();

        // Asked again within the minute: dropped, and the wait goes on.
        cloud.publish_json(0, &UpdateCheck::default()).unwrap();
        assert!(!check.wait(&mut ears, Duration::from_millis(120), &mut |_| {}));
        // A minute after the last that started a round, it may again.
        cloud.publish_json(0, &UpdateCheck::default()).unwrap();
        assert!(check.asked(Instant::now() + UpdateCheck::AGAIN_AFTER));
    }

    #[test]
    fn a_check_said_while_a_round_ran_is_dropped_not_kept_for_after_it() {
        let root = tempfile::tempdir().unwrap();
        let mut check = AppCheck::new(root.path(), &mut |_| {});
        let mut cloud = cloud(&Bus::new(root.path()).unwrap());
        // Two taps while the round ran.
        cloud.publish_json(0, &UpdateCheck::default()).unwrap();
        cloud.publish_json(0, &UpdateCheck::default()).unwrap();
        check.forget(&mut |_| {});
        assert!(!check.asked(Instant::now()));
        // The next is taken.
        cloud.publish_json(0, &UpdateCheck::default()).unwrap();
        assert!(check.asked(Instant::now()));
    }

    #[test]
    fn a_lamp_update_that_starts_later_reads_no_old_check() {
        let root = tempfile::tempdir().unwrap();
        let bus = Bus::new(root.path()).unwrap();
        let mut cloud = bus
            .publisher_declared(UpdateCheck::TOPIC, &UpdateCheck::declaration(), "cloud")
            .unwrap();
        let said = cloud.publish_json(0, &UpdateCheck::default()).unwrap();
        assert_eq!(said.delivered, 0, "nobody reads it yet");
        assert!(bus.latest(UpdateCheck::TOPIC).unwrap().is_none());
        let mut check = AppCheck::new(root.path(), &mut |_| {});
        std::thread::sleep(Duration::from_millis(250));
        assert!(!check.asked(Instant::now()));
    }

    /// #6284: an owed first update ends the wait at the next look, a bus
    /// or not; nothing owed, the whole wait.
    #[test]
    fn the_first_update_owed_ends_the_wait_at_its_next_look() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("not-a-folder");
        std::fs::write(&file, "").unwrap();
        let mut check = AppCheck::new(&file, &mut |_| {}).looking_every(LOOK);
        let mut ears = Waits::default();
        // Owed already: no listen at all.
        assert_eq!(
            check.wait_or(&mut ears, Duration::from_secs(60), &mut |_| {}, &mut |_| {
                true
            }),
            Woken::Due
        );
        assert!(ears.0.is_empty());
        // Owed from the third look on.
        let mut looks = 0;
        let started = Instant::now();
        let woken = check.wait_or(&mut ears, Duration::from_secs(60), &mut |_| {}, &mut |_| {
            looks += 1;
            looks > 3
        });
        assert_eq!(woken, Woken::Due);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(ears.0.len(), 3);
        // Never owed: the whole wait.
        assert_eq!(
            check.wait_or(
                &mut ears,
                Duration::from_millis(120),
                &mut |_| {},
                &mut |_| false
            ),
            Woken::Waited
        );
    }

    #[test]
    fn without_a_bus_the_wait_is_one_listen_as_before() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("not-a-folder");
        std::fs::write(&file, "").unwrap();
        let mut said = Vec::new();
        let mut check = AppCheck::new(&file, &mut |line| said.push(line.to_owned()));
        assert_eq!(said.len(), 1, "{said:?}");
        let mut ears = Waits::default();
        assert!(!check.wait(&mut ears, Duration::from_millis(30), &mut |_| {}));
        assert_eq!(ears.0, [Duration::from_millis(30)]);
        check.forget(&mut |_| {});
        assert!(!check.asked(Instant::now()));
    }

    /// #6240: a tap to install a missing Hermes ends the wait as a check
    /// does and marks it asked; one said during a round is kept as asked.
    #[test]
    fn a_tap_to_install_an_agent_ends_the_wait_and_is_kept_from_a_round() {
        use crate::engines::{Asks, Engines};
        use lamp_msgs::EngineInstallAsk;
        let tap = EngineInstallAsk {
            engine: "hermes".into(),
        };
        let lamp = || {
            let root = tempfile::tempdir().unwrap();
            let records = tempfile::tempdir().unwrap();
            let engines = Engines {
                hermes_on: true,
                ..Engines::default()
            };
            let asks = Asks::new(root.path(), records.path(), engines.clone(), &mut |_| {});
            let check = AppCheck::new(root.path(), &mut |_| {})
                .looking_every(LOOK)
                .with_engines(asks);
            let cloud = Bus::new(root.path())
                .unwrap()
                .publisher_declared(
                    EngineInstallAsk::TOPIC,
                    &EngineInstallAsk::declaration(),
                    "cloud",
                )
                .unwrap();
            std::thread::sleep(Duration::from_millis(250));
            (root, records, engines, check, cloud)
        };
        let mut ears = Waits::default();

        // Said during the wait: ends it within about a look.
        let (_root, _records, engines, mut check, mut cloud) = lamp();
        let said = tap.clone();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            cloud.publish_json(0, &said).unwrap();
            cloud
        });
        let started = Instant::now();
        assert!(check.wait(&mut ears, Duration::from_secs(60), &mut |_| {}));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(engines.pending());
        drop(sender.join().unwrap());

        // Said during a round: kept as asked, and no wait ended by it.
        let (_root, _records, engines, mut check, mut cloud) = lamp();
        cloud.publish_json(0, &tap).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        check.forget(&mut |_| {});
        assert!(engines.pending());
        assert!(!check.wait(&mut ears, Duration::from_millis(120), &mut |_| {}));
    }

    /// The owner's retry of a refused build (#6266) comes with the check that
    /// starts the round, once; one dropped with a too-soon check is dropped
    /// with it, and a plain check carries none.
    #[test]
    fn a_retry_in_the_check_is_heard_once_with_the_round_it_starts() {
        let root = tempfile::tempdir().unwrap();
        let mut check = AppCheck::new(root.path(), &mut |_| {}).looking_every(LOOK);
        let mut cloud = cloud(&Bus::new(root.path()).unwrap());
        let mut ears = Waits::default();
        cloud
            .publish_json(0, &UpdateCheck { retry: Some(7) })
            .unwrap();
        assert!(check.wait(&mut ears, Duration::from_secs(2), &mut |_| {}));
        assert_eq!(check.take_retries(), vec![7]);
        assert!(check.take_retries().is_empty(), "taken once");
        // Within the minute: the check is dropped, and the retry with it.
        cloud
            .publish_json(0, &UpdateCheck { retry: Some(8) })
            .unwrap();
        assert!(!check.asked(Instant::now()));
        assert!(check.take_retries().is_empty());
        // A plain check carries none.
        let later = Instant::now() + UpdateCheck::AGAIN_AFTER;
        cloud.publish_json(0, &UpdateCheck::default()).unwrap();
        assert!(check.asked(later));
        assert!(check.take_retries().is_empty());
    }
}
