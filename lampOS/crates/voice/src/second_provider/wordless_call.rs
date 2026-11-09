//! A wordless call only on a sure hearing (`LAMP_NAME_WORDLESS_BAR=on`,
//! #5385). A line that was only the lamp's name (`ProviderInput::Called`)
//! and whose words came back empty is asked as the name ("Lamp"), or it
//! would go unanswered as a sound (#1712). With the switch on, that is done
//! only when lamp-name heard the name in the line at [`BAR`] or more; under
//! it, the line is left as a sound: nothing is asked and nothing answered.
//! `name/heard` itself goes out as ever: this is the voice's reading of it.
//!
//! Why: on lamp-75c9 on 9 Nov a line the body's own move opened was heard as
//! the name at 0.941 with no words, and the lamp answered a call nobody made
//! (Eighth's report on #5385). On lamp-c0a4, 82 real calls scored 0.901 to
//! 1.000 (median 0.964); 4 had no words back, 3 of them under 0.99. Noise
//! wakes scored 0.941 to 0.978. The rule costs about 3 calls in 82, each a
//! lone "Lamp" whose words did not come back; any call with words is
//! answered as before.
//!
//! The hearings are read from `name/heard` on a subscriber of their own, by
//! a reader thread of their own all the time (lines with words never ask,
//! and an unread queue fills and drops the next hearing, Second's P2 on
//! #5845), the last [`KEPT`] kept for [`KEPT_US`], and a line's best is the highest that began from
//! [`crate::attention::NAME_BEFORE_US`] before its first sound to its end.
//! A line with none read is under the bar: its call came from the name's
//! first sign alone (`name/rising`), which lamp-name never said in full.

use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, SystemTime},
};

/// The switch, in the second worker's environment: `on` exactly.
pub const ENVIRONMENT: &str = "LAMP_NAME_WORDLESS_BAR";
/// The least a hearing scores for a line with no words to be a call.
pub const BAR: f32 = 0.99;
/// Hearings older than this are let go.
const KEPT_US: u64 = 30_000_000;
/// The most hearings kept.
const KEPT: usize = 32;
/// The bus's name for this reader.
const SUBSCRIBER: &str = "second_wordless";
/// How long the reader waits for a hearing before it looks whether it is
/// to stop.
const READ_EVERY: Duration = Duration::from_millis(200);

/// The best score lamp-name heard the name at in a line's window (its first
/// and last sound's read times, this process's clock), if it heard it.
pub type BestHeard = Arc<dyn Fn((u64, u64)) -> Option<f32> + Send + Sync>;

/// Whether the switch is on, from its value.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Whether a line with no words, marked as only the name, is asked as the
/// name: lamp-name heard it in the line at [`BAR`] or more.
pub fn answered(best: Option<f32>) -> bool {
    best.is_some_and(|score| score >= BAR)
}

/// What the journal says of the best hearing.
pub fn scored(best: Option<f32>) -> String {
    best.map_or_else(
        || "lamp-name said no hearing of it".to_owned(),
        |score| format!("lamp-name heard it at {score:.3}"),
    )
}

/// The hearings read from `name/heard`, when each began by this process's
/// clock and its score: read all the time by a thread of their own, which
/// stops when this is dropped.
pub struct Hearings {
    kept: Arc<Mutex<VecDeque<(u64, f32)>>>,
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

impl Hearings {
    pub fn on(root: &Path) -> std::io::Result<Self> {
        let bus = lamp_bus::Bus::new(root)?;
        let heard = bus.subscribe_typed(
            lamp_msgs::NameHeard::TOPIC,
            SUBSCRIBER,
            lamp_msgs::NameHeard::TYPE,
        )?;
        let kept = Arc::new(Mutex::new(VecDeque::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let reader = std::thread::Builder::new()
            .name("lamp-voice-wordless-heard".to_owned())
            .spawn({
                let kept = Arc::clone(&kept);
                let stop = Arc::clone(&stop);
                move || read(heard, &kept, &stop)
            })?;
        Ok(Self {
            kept,
            stop,
            reader: Some(reader),
        })
    }

    /// The best hearing in `window` of those read. Never waits on the bus.
    pub fn best(&self, window: (u64, u64)) -> Option<f32> {
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        prune(&mut kept, lamp_ipc::monotonic_us());
        best_in(&kept, window)
    }
}

impl Drop for Hearings {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

/// The reader: each hearing as it comes, dated and kept, until `stop`. A
/// bus that fails is said once and tried again every [`READ_EVERY`].
fn read(mut heard: lamp_bus::Subscriber, kept: &Mutex<VecDeque<(u64, f32)>>, stop: &AtomicBool) {
    let mut failing = false;
    while !stop.load(Ordering::Relaxed) {
        match heard.recv(READ_EVERY) {
            Ok(Some(message)) => {
                failing = false;
                let Ok(hearing) = message.parse::<lamp_msgs::NameHeard>() else {
                    continue;
                };
                let now_us = lamp_ipc::monotonic_us();
                let now_ms = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map_or(0, |since| since.as_millis().try_into().unwrap_or(u64::MAX));
                // Its publisher's monotonic clock is its own: dated by the
                // wall clock both share.
                let ago_ms =
                    now_ms.saturating_sub(message.header.at_unix_ms) + u64::from(hearing.ago_ms);
                let mut kept = kept.lock().unwrap_or_else(|e| e.into_inner());
                keep(
                    &mut kept,
                    now_us,
                    now_us.saturating_sub(ago_ms * 1_000),
                    hearing.score,
                );
            }
            Ok(None) => failing = false,
            Err(why) => {
                if !failing {
                    eprintln!(
                        "lamp-voice second: lamp-name's hearings cannot be read for a wordless call: {why}; trying again (LAMP_NAME_WORDLESS_BAR)"
                    );
                    failing = true;
                }
                std::thread::sleep(READ_EVERY);
            }
        }
    }
}

/// Keeps a hearing that began at `at_us`, letting go of the oldest past
/// [`KEPT`] and of any older than [`KEPT_US`] at `now_us`.
fn keep(kept: &mut VecDeque<(u64, f32)>, now_us: u64, at_us: u64, score: f32) {
    prune(kept, now_us);
    while kept.len() >= KEPT {
        kept.pop_front();
    }
    kept.push_back((at_us, score));
}

fn prune(kept: &mut VecDeque<(u64, f32)>, now_us: u64) {
    kept.retain(|(at, _)| now_us.saturating_sub(*at) <= KEPT_US);
}

/// The best of `kept` that began from [`crate::attention::NAME_BEFORE_US`]
/// before the window's start to its end.
fn best_in(kept: &VecDeque<(u64, f32)>, (began, ended): (u64, u64)) -> Option<f32> {
    let from = began.saturating_sub(crate::attention::NAME_BEFORE_US);
    kept.iter()
        .filter(|(at, _)| (from..=ended.max(began)).contains(at))
        .map(|(_, score)| *score)
        .reduce(f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_on_exactly_switches_it_on() {
        assert!(on(Some("on")));
        for value in [None, Some(""), Some("off"), Some("ON"), Some("0.99")] {
            assert!(!on(value), "{value:?}");
        }
    }

    /// The first builder's rule (#5385): under 0.99, or no hearing, no call.
    #[test]
    fn a_wordless_line_is_a_call_only_at_the_bar() {
        assert!(!answered(Some(0.941)));
        assert!(!answered(Some(0.989)));
        assert!(!answered(None));
        assert!(answered(Some(0.99)));
        assert!(answered(Some(1.0)));
    }

    #[test]
    fn a_lines_best_hearing_is_from_just_before_its_start_to_its_end() {
        let second = 1_000_000;
        let kept: VecDeque<(u64, f32)> = [
            (2 * second, 0.999),
            (9 * second, 0.95),
            (10 * second + 500_000, 0.94),
            (13 * second, 1.0),
        ]
        .into();
        assert_eq!(best_in(&kept, (10 * second, 12 * second)), Some(0.95));
        assert_eq!(best_in(&kept, (10 * second, 13 * second)), Some(1.0));
        assert_eq!(best_in(&kept, (20 * second, 21 * second)), None);
    }

    fn bus_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "wordless-call-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn says(root: &Path) -> lamp_bus::Publisher {
        lamp_bus::Bus::new(root)
            .unwrap()
            .publisher_declared(
                lamp_msgs::NameHeard::TOPIC,
                &lamp_msgs::NameHeard::declaration(),
                "name",
            )
            .unwrap()
    }

    fn heard(score: f32) -> lamp_msgs::NameHeard {
        lamp_msgs::NameHeard { score, ago_ms: 0 }
    }

    /// Second's P2 on #5845: ordinary calls with words never ask for the
    /// best hearing, so `name/heard` must be read without them. Forty
    /// hearings at 0.95 go by with nobody asking; the sure bare call after
    /// them is still the line's best, not dropped on a full socket queue.
    #[test]
    fn a_sure_hearing_after_many_unasked_ones_is_still_read() {
        let root = bus_root("unasked");
        let hearings = Hearings::on(&root).unwrap();
        let mut says = says(&root);
        for _ in 0..40 {
            says.publish_json(1, &heard(0.95)).unwrap();
            // As lamp-name would, a hearing at a time.
            std::thread::sleep(Duration::from_millis(5));
        }
        // The reader's moment to read the last of them.
        std::thread::sleep(Duration::from_millis(100));
        let delivered = says.publish_json(1, &heard(1.0)).unwrap();
        let now = lamp_ipc::monotonic_us();
        let window = (now.saturating_sub(1_000_000), now + 1_000_000);
        let until = std::time::Instant::now() + Duration::from_secs(2);
        let mut best = hearings.best(window);
        while best != Some(1.0) && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
            best = hearings.best(window);
        }
        assert_eq!(best, Some(1.0), "the sure hearing was {delivered:?}");
        let _ = std::fs::remove_dir_all(root);
    }

    /// The reader stops with its hearings: dropped, its thread has let go
    /// of what it kept.
    #[test]
    fn dropping_the_hearings_stops_their_reader() {
        let root = bus_root("dropped");
        let hearings = Hearings::on(&root).unwrap();
        let kept = Arc::downgrade(&hearings.kept);
        drop(hearings);
        assert!(kept.upgrade().is_none(), "the reader still holds them");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn at_most_the_last_kept_are_kept_and_none_older_than_kept_us() {
        let mut kept = VecDeque::new();
        for at in 0..40 {
            keep(&mut kept, 100, at, 0.95);
        }
        assert_eq!(kept.len(), KEPT);
        assert_eq!(kept.front(), Some(&(40 - KEPT as u64, 0.95)));
        keep(&mut kept, KEPT_US + 50, KEPT_US + 50, 1.0);
        assert_eq!(
            kept.iter().copied().collect::<Vec<_>>(),
            [(KEPT_US + 50, 1.0)]
        );
    }
}
