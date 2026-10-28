//! The app's Check for update (#4153): lamp-cloud says one `update/check`
//! (`lamp_msgs::UpdateCheck`) when the owner taps it, and `lamp-update
//! serve` starts its next round then instead of after `--every-s`.
//!
//! Only the wait between rounds is cut short. The round is the same round:
//! the same lock, a quiet moment before any switch, `--apply` as set. A
//! check that came while a round ran is dropped, never queued for after it,
//! and at most one a minute starts a round. Only a `serve` with `--apply on`
//! subscribes; with nobody saying a check, it waits as before.
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

/// `serve`'s side of the check: its subscription, and when a check last
/// started a round.
pub struct AppCheck {
    checks: Option<Subscriber>,
    last: Option<Instant>,
    /// [`LOOK_EVERY`] on a lamp; shorter only in tests, so none sleeps
    /// for whole seconds (#1806).
    look: Duration,
}

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
        }
    }

    /// The same check, looking every `look` instead of [`LOOK_EVERY`].
    #[cfg(test)]
    fn looking_every(mut self, look: Duration) -> Self {
        self.look = look;
        self
    }

    /// Drops every check that came while a round ran: a round never starts
    /// another, and the app reads the round it asked for on `update/status`.
    pub fn forget(&mut self) {
        self.waiting();
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
            return false;
        }
        self.last = Some(now);
        true
    }

    /// Takes every check waiting; whether there was one that reads.
    fn waiting(&mut self) -> bool {
        let Some(checks) = &mut self.checks else {
            return false;
        };
        let mut any = false;
        while let Ok(Some(message)) = checks.recv(Duration::ZERO) {
            any |= message.parse::<UpdateCheck>().is_ok();
        }
        any
    }

    /// The wait between rounds: `ears` listen for `every`, as before, and
    /// a check taken ends it early, true. Without a subscription it is one
    /// listen of `every`.
    pub fn wait(&mut self, ears: &mut dyn Ears, every: Duration) -> bool {
        if self.checks.is_none() {
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
            if self.asked(Instant::now()) {
                return true;
            }
        }
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
        assert!(!check.wait(&mut ears, Duration::from_millis(120)));
        assert!(started.elapsed() >= Duration::from_millis(120));
        // How many looks depends on the runner's load, so only their shape
        // is asked: every look but the last is a whole `LOOK`, none longer.
        let (last, whole) = ears.0.split_last().unwrap();
        assert!(!whole.is_empty(), "{:?}", ears.0);
        assert!(whole.iter().all(|look| *look == LOOK), "{:?}", ears.0);
        assert!(*last <= LOOK, "{:?}", ears.0);

        // A check said during the wait ends it within about a look.
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            cloud.publish_json(0, &UpdateCheck {}).unwrap();
            cloud
        });
        let started = Instant::now();
        assert!(check.wait(&mut ears, Duration::from_secs(60)));
        let took = started.elapsed();
        assert!(took < Duration::from_secs(1), "{took:?}");
        let mut cloud = sender.join().unwrap();

        // Asked again within the minute: dropped, and the wait goes on.
        cloud.publish_json(0, &UpdateCheck {}).unwrap();
        assert!(!check.wait(&mut ears, Duration::from_millis(120)));
        // A minute after the last that started a round, it may again.
        cloud.publish_json(0, &UpdateCheck {}).unwrap();
        assert!(check.asked(Instant::now() + UpdateCheck::AGAIN_AFTER));
    }

    #[test]
    fn a_check_said_while_a_round_ran_is_dropped_not_kept_for_after_it() {
        let root = tempfile::tempdir().unwrap();
        let mut check = AppCheck::new(root.path(), &mut |_| {});
        let mut cloud = cloud(&Bus::new(root.path()).unwrap());
        // Two taps while the round ran.
        cloud.publish_json(0, &UpdateCheck {}).unwrap();
        cloud.publish_json(0, &UpdateCheck {}).unwrap();
        check.forget();
        assert!(!check.asked(Instant::now()));
        // The next is taken.
        cloud.publish_json(0, &UpdateCheck {}).unwrap();
        assert!(check.asked(Instant::now()));
    }

    #[test]
    fn a_lamp_update_that_starts_later_reads_no_old_check() {
        let root = tempfile::tempdir().unwrap();
        let bus = Bus::new(root.path()).unwrap();
        let mut cloud = bus
            .publisher_declared(UpdateCheck::TOPIC, &UpdateCheck::declaration(), "cloud")
            .unwrap();
        let said = cloud.publish_json(0, &UpdateCheck {}).unwrap();
        assert_eq!(said.delivered, 0, "nobody reads it yet");
        assert!(bus.latest(UpdateCheck::TOPIC).unwrap().is_none());
        let mut check = AppCheck::new(root.path(), &mut |_| {});
        std::thread::sleep(Duration::from_millis(250));
        assert!(!check.asked(Instant::now()));
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
        assert!(!check.wait(&mut ears, Duration::from_millis(30)));
        assert_eq!(ears.0, [Duration::from_millis(30)]);
        check.forget();
        assert!(!check.asked(Instant::now()));
    }
}
