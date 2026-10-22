//! A push on the lamp's body during a skill's search (#2904): the voice
//! learns of it as lamp-skills does, from `body/hand`, and the search ends
//! for the voice too, as a spoken stop ends it. On lamp-c0a4 on 22 Oct
//! (#2835, 12:42) a push ended look-for-things' run in lamp-skills within
//! 211 ms, but the voice wasn't told: the pushed look's picture still went
//! to the model, which called `look_for` again 4 s later, and the base
//! turned again.
//!
//! The coordinator drops the held look's picture and the search's note and
//! walk (`coordinator::look_note`). The second route ([`SearchPushed`]) runs
//! no look call of that conversation for [`QUIET_US`], unless the person
//! asks again in words, and tells the next request the person asked
//! ([`NOTE`]), as `LAMP_STOP_TOLD` tells a stop. Only a push while a search
//! goes on does any of it: with no search, a push changes nothing here.

use serde_json::Value;
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// What the next request the person asks is told beside its words, once,
/// in the form of the lamp's other notes (`STOPPED_NOTE`).
pub const NOTE: &str = "(For you to know, not to answer: the person stopped the search with a hand, pushing the lamp as it looked. The search is over: do not look again unless they ask you to in words, and say nothing about the push.)";

/// What a look call held after a push is answered.
pub const HELD: &str = "Not done: the person stopped the search with a hand. Do not look again unless they ask you to in words.";

/// Why a look is dropped after a push, in the journal and the trace.
pub const WHY: &str = "the body was pushed";

/// No look call runs this long after a push, unless the person asks again
/// in words (#2904).
pub const QUIET_US: u64 = 30_000_000;

/// A search goes on this long after its newest look: look-for-things holds
/// each look's pose two minutes, or until someone speaks.
pub const SEARCH_HELD_US: u64 = 120_000_000;

/// The body says `body/hand` at least every 100 ms while it runs: older
/// than this, it is not running, and its last word is no push now.
const FRESH_MS: u64 = 1_000;

/// The bus is looked at this often.
const LOOK_EVERY_US: u64 = 50_000;

/// A skill's search, as the second route asked its looks.
#[derive(Clone, Debug)]
struct Search {
    /// The request whose look was asked last, and when.
    request: u64,
    at_us: u64,
    /// The skill's call, as the model made it: its name, and for
    /// `use_skill` the skill it named.
    name: String,
    skill: Option<String>,
}

/// The push that ended a search.
#[derive(Clone, Debug)]
struct Pushed {
    at_us: u64,
    /// The newest request the person had asked by then: a later one they
    /// ask asks again in words.
    through: u64,
    /// The search's call, held with [`crate::second_provider::LOOK_CALL`].
    name: String,
    skill: Option<String>,
    told: bool,
}

/// The second route's side: the search going on, and the push that ended
/// one.
#[derive(Debug, Default)]
pub struct SearchPushed {
    search: Option<Search>,
    pushed: Option<Pushed>,
    /// The newest request the person asked.
    newest: u64,
}

impl SearchPushed {
    /// A skill's search look was asked in `request`, by the call `name`
    /// with `args`.
    pub fn looked(&mut self, request: u64, name: &str, args: &Value, at_us: u64) {
        self.search = Some(Search {
            request,
            at_us,
            name: name.to_owned(),
            skill: skill_of(name, args),
        });
    }

    /// The body was pushed at `at_us`: whether a search was going on, now
    /// ended.
    pub fn hand(&mut self, at_us: u64) -> bool {
        let Some(search) = self
            .search
            .take()
            .filter(|search| at_us.saturating_sub(search.at_us) < SEARCH_HELD_US)
        else {
            return false;
        };
        self.pushed = Some(Pushed {
            at_us,
            through: self.newest,
            name: search.name,
            skill: search.skill,
            told: false,
        });
        true
    }

    /// The person's request `request` is asked at `at_us`: the note, once,
    /// if a push ended a search within `TELL_STOP_WITHIN_US` and it has not
    /// been told. Words of a request later than the push's ask again: the
    /// search they end, and the push holds no look of theirs.
    pub fn asked(&mut self, request: u64, at_us: u64) -> Option<&'static str> {
        self.newest = self.newest.max(request);
        // Words heard end a search, as lamp-skills ends it on them.
        if self
            .search
            .as_ref()
            .is_some_and(|search| request > search.request)
        {
            self.search = None;
        }
        let pushed = self.pushed.as_mut()?;
        let note = (!pushed.told
            && at_us.saturating_sub(pushed.at_us) <= crate::not_speech::TELL_STOP_WITHIN_US)
            .then_some(NOTE);
        pushed.told = true;
        if request > pushed.through {
            self.pushed = None;
        }
        note
    }

    /// Whether a call `name` with `args` in `request` (`heard`: the person
    /// spoke it) is held: a look call within [`QUIET_US`] of a push that
    /// ended a search, in a turn the lamp wrote or one asked before the
    /// push.
    pub fn holds(&self, request: u64, heard: bool, name: &str, args: &Value, at_us: u64) -> bool {
        self.pushed.as_ref().is_some_and(|pushed| {
            at_us.saturating_sub(pushed.at_us) < QUIET_US
                && !(heard && request > pushed.through)
                && (name == crate::second_provider::LOOK_CALL
                    || (name == pushed.name && skill_of(name, args) == pushed.skill))
        })
    }
}

/// The skill a `use_skill` call names; none for a skill's own tool.
fn skill_of(name: &str, args: &Value) -> Option<String> {
    (name == crate::use_skill::NAME)
        .then(|| args.get("skill").and_then(Value::as_str).map(str::to_owned))
        .flatten()
}

/// The body's word on a push, read from `body/hand` as lamp-skills reads
/// it: each push once, by when it began (`since_us`, the body's clock).
#[derive(Default)]
pub struct HandIn {
    bus: Option<lamp_bus::Bus>,
    looked_at_us: Option<u64>,
    taken: Option<u64>,
}

impl HandIn {
    pub fn on(bus: lamp_bus::Bus) -> Self {
        Self {
            bus: Some(bus),
            ..Self::default()
        }
    }

    /// With `LAMP_BUS=on` and a bus that can be used; else nothing.
    pub fn from_environment() -> Option<Self> {
        if std::env::var(crate::bus_out::ENVIRONMENT).ok().as_deref() != Some("on") {
            return None;
        }
        lamp_bus::Bus::from_environment().ok().map(Self::on)
    }

    /// Whether a push began since the last look. Never waits, and looks at
    /// the bus at most every 50 ms however often it is asked.
    pub fn pushed(&mut self, now_us: u64) -> bool {
        if self
            .looked_at_us
            .is_some_and(|at| now_us.saturating_sub(at) < LOOK_EVERY_US)
        {
            return false;
        }
        self.looked_at_us = Some(now_us);
        let Some(message) = self
            .bus
            .as_ref()
            .and_then(|bus| bus.latest(lamp_msgs::Hand::TOPIC).ok().flatten())
        else {
            return false;
        };
        let Ok(hand) = message.parse::<lamp_msgs::Hand>() else {
            return false;
        };
        let age_ms = unix_ms().saturating_sub(message.header.at_unix_ms);
        new_push(hand, age_ms, &mut self.taken)
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// Whether `hand`, said `age_ms` ago, is a push not yet taken.
fn new_push(hand: lamp_msgs::Hand, age_ms: u64, taken: &mut Option<u64>) -> bool {
    if !hand.pushed || age_ms > FRESH_MS || *taken == Some(hand.since_us) {
        return false;
    }
    *taken = Some(hand.since_us);
    true
}

/// The second route's reader of `body/hand` on the bus at `root`, telling
/// `pushed` of each push on its own thread; it stops when dropped.
pub struct Reader {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// [`Reader`]; `None` when the bus cannot be opened.
pub fn read_from(pushed: Arc<Mutex<SearchPushed>>, root: &Path) -> Option<Reader> {
    let mut hand = HandIn::on(lamp_bus::Bus::new(root).ok()?);
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let thread = std::thread::Builder::new()
        .name("second-hand".into())
        .spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_micros(LOOK_EVERY_US));
                let now = lamp_ipc::monotonic_us();
                if hand.pushed(now)
                    && pushed
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .hand(now)
                {
                    eprintln!(
                        "lamp-voice second: {WHY}: the search ended; no look for {} s unless the person asks again, and the next request is told",
                        QUIET_US / 1_000_000
                    );
                }
            }
        })
        .ok()?;
    Some(Reader {
        stop,
        thread: Some(thread),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const S: u64 = 1_000_000;

    fn searching() -> SearchPushed {
        let mut pushed = SearchPushed::default();
        assert_eq!(pushed.asked(1, 0), None);
        pushed.looked(1, "look_for", &json!({"place": "desk"}), S);
        pushed
    }

    #[test]
    fn a_push_mid_search_holds_its_looks_in_the_lamps_turns_for_thirty_seconds() {
        let mut pushed = searching();
        assert!(pushed.hand(2 * S));
        let place = json!({"place": "left"});
        // The look's turn the lamp wrote, and the request asked before.
        assert!(pushed.holds(2, false, "look_for", &place, 6 * S));
        assert!(pushed.holds(1, true, "look_for", &place, 6 * S));
        assert!(pushed.holds(
            2,
            false,
            crate::second_provider::LOOK_CALL,
            &Value::Null,
            6 * S
        ));
        // Other calls run.
        assert!(!pushed.holds(2, false, "light_on", &place, 6 * S));
        // Over after 30 s.
        assert!(!pushed.holds(2, false, "look_for", &place, 32 * S));
    }

    #[test]
    fn the_next_request_is_told_once_and_its_words_may_look_again() {
        let mut pushed = searching();
        assert!(pushed.hand(2 * S));
        assert_eq!(pushed.asked(3, 4 * S), Some(NOTE));
        assert!(!pushed.holds(3, true, "look_for", &json!({}), 5 * S));
        assert!(!pushed.holds(4, false, "look_for", &json!({}), 5 * S));
        assert_eq!(pushed.asked(5, 6 * S), None);
    }

    #[test]
    fn a_push_with_no_search_going_on_changes_nothing() {
        let mut pushed = SearchPushed::default();
        assert!(!pushed.hand(S));
        assert!(!pushed.holds(1, false, "look_for", &json!({}), 2 * S));
        assert_eq!(pushed.asked(2, 3 * S), None);
        // Words heard after a search end it: a push then is none of its.
        let mut spoken = searching();
        assert_eq!(spoken.asked(2, 3 * S), None);
        assert!(!spoken.hand(4 * S));
        // Nor a search whose looks are long held out.
        let mut old = searching();
        assert!(!old.hand(S + SEARCH_HELD_US));
    }

    #[test]
    fn use_skill_holds_only_the_searchs_own_skill() {
        let mut pushed = SearchPushed::default();
        pushed.looked(1, "use_skill", &json!({"skill": "look-for-things"}), S);
        assert!(pushed.hand(2 * S));
        let own = json!({"skill": "look-for-things", "choice": "left"});
        assert!(pushed.holds(2, false, "use_skill", &own, 3 * S));
        assert!(!pushed.holds(2, false, "use_skill", &json!({"skill": "dance"}), 3 * S));
    }

    #[test]
    fn each_fresh_push_is_taken_once() {
        let hand = |pushed, since_us| lamp_msgs::Hand { pushed, since_us };
        let mut taken = None;
        assert!(!new_push(hand(false, 0), 10, &mut taken));
        assert!(new_push(hand(true, 500), 10, &mut taken));
        assert!(!new_push(hand(true, 500), 60, &mut taken));
        assert!(new_push(hand(true, 900), 10, &mut taken));
        // The body's last word, from before it stopped: no push now.
        assert!(!new_push(hand(true, 1_300), FRESH_MS + 1, &mut taken));
    }

    #[test]
    fn the_reader_takes_a_push_from_the_bus() {
        let root = std::env::temp_dir().join(format!("lv-hand-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut body = bus
            .publisher_declared(
                lamp_msgs::Hand::TOPIC,
                &lamp_msgs::Hand::declaration(),
                "body",
            )
            .unwrap();
        let mut hand = HandIn::on(bus);
        assert!(!hand.pushed(0));
        body.publish_json(
            1,
            &lamp_msgs::Hand {
                pushed: true,
                since_us: 7,
            },
        )
        .unwrap();
        assert!(hand.pushed(LOOK_EVERY_US));
        assert!(!hand.pushed(2 * LOOK_EVERY_US));
        let _ = std::fs::remove_dir_all(&root);
    }
}
