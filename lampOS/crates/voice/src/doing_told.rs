//! What the person at the desk was last seen doing, told to the second
//! voice as context only (#6543, `humans/doing`).
//!
//! The cloud labels say when a drink, a meal or a stretch was last seen at
//! the desk (`lamp_msgs::HumanDoing`). The model is told it beside a spoken
//! request, as `humans/seems` is told ([`crate::seems_told`]), so a reply
//! can fit it: no glass of water suggested to someone who just had one.
//! The lamp never volunteers it: it never says "enjoy your lunch", never
//! says it saw anything, and never speaks of the body. Only the last thing
//! seen is told, as a coarse kind with a rounded time, never the raw label,
//! a count or a name, and nothing of it is kept past the note: no history.
//!
//! As for `humans/seems`, a value counts only fresh (within
//! `HumanDoing::LASTS`), sure, with exactly one face in view (with two, it
//! may be the other's), and the note is given only in a room that is not
//! busy, to a known person's spoken request. Each sighting is told once,
//! counted as told only once the answer given it is heard.
//!
//! Off unless the service's environment says `on`.
use lamp_msgs::{Doing, HumanDoing};
use std::{collections::VecDeque, time::Duration};

/// Where the coordinator reads whether what the person was seen doing is
/// told. `on` only.
pub const ENVIRONMENT: &str = "LAMP_SECOND_DOING";
/// The provider worker's flag for it.
pub const FLAG: &str = "--second-doing";

/// How often the worker reads `humans/doing`.
pub const POLL: Duration = Duration::from_secs(2);
/// A read less sure than this is not told: a drink told that wasn't costs
/// more than one missed.
pub const SURE_FROM: f32 = 0.5;
/// The newest read, kept by the worker's poll, says nothing this long after
/// it was taken off the bus (the poll is every 2 s; the bus's own
/// `HumanDoing::LASTS` is checked as it is taken).
const KEPT_FOR: Duration = Duration::from_secs(3);
/// Two reads of one kind whose sighting falls this close are one sighting:
/// each message's timer is whole seconds from its own time.
const SAME_SIGHTING: Duration = Duration::from_secs(60);
/// The most requests remembered as given a note and not yet heard: as many
/// as may be in flight at once (the air's `AIR_GIVEN_MOST`); the oldest is
/// let go past it.
const GIVEN_MOST: usize = 4;

/// One sighting: what, and when on the wall clock (ms since the epoch).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sighting {
    what: Doing,
    at_ms: u64,
}

impl Sighting {
    fn same(&self, other: &Sighting) -> bool {
        self.what == other.what
            && self.at_ms.abs_diff(other.at_ms) < SAME_SIGHTING.as_millis() as u64
    }
}

/// The newest read kept: its sighting, how long ago it was when read, and
/// when (the monotonic clock, µs).
#[derive(Clone, Copy, Debug)]
struct Seen {
    sighting: Sighting,
    ago_s: u64,
    read_at_us: u64,
}

/// What the provider worker knows of what the person was last seen doing,
/// from `humans/doing`.
#[derive(Debug, Default)]
pub struct DoingTold {
    seen: Option<Seen>,
    /// Each request given a note and not yet heard, with the sighting it
    /// was given, the newest last: an answer heard spends only its own
    /// request's sighting (Heron's P3 on #6661). [`GIVEN_MOST`] at most.
    given: VecDeque<(u64, Sighting)>,
    /// The newest sighting told, its answer heard.
    told: Option<Sighting>,
}

impl DoingTold {
    /// The newest fresh read on the bus with the time it was said (ms since
    /// the epoch), or none, at `now_ms` on the wall clock and `now_us` on
    /// the monotonic one. None, nothing seen yet, or an unsure read keeps
    /// nothing.
    pub fn seen(&mut self, read: Option<(HumanDoing, u64)>, now_ms: u64, now_us: u64) {
        self.seen = read.and_then(|(read, said_ms)| {
            let ago_s = match read.last {
                Doing::Drinking => read.drank_s_ago,
                Doing::Eating => read.ate_s_ago,
                Doing::Break => read.break_s_ago,
                Doing::Nothing => return None,
            };
            // A NaN is never sure.
            if read.confidence.is_nan() || read.confidence < SURE_FROM {
                return None;
            }
            let at_ms = said_ms.saturating_sub(u64::from(ago_s) * 1000);
            Some(Seen {
                sighting: Sighting {
                    what: read.last,
                    at_ms,
                },
                ago_s: now_ms.saturating_sub(at_ms) / 1000,
                read_at_us: now_us,
            })
        });
    }

    /// [`Self::seen`], with how many faces the eyes have in view now
    /// (`humans/faces`): a read counts only with exactly one face in view;
    /// any other count, or none known, keeps nothing, as for
    /// `humans/seems`.
    pub fn seen_in_view(
        &mut self,
        read: Option<(HumanDoing, u64)>,
        faces_in_view: Option<usize>,
        now_ms: u64,
        now_us: u64,
    ) {
        self.seen(read.filter(|_| faces_in_view == Some(1)), now_ms, now_us);
    }

    /// The note for `request`, if one is due now: a fresh sighting not yet
    /// told. It is remembered as given to that request, to be spent by
    /// [`Self::told`] once that request's answer is heard.
    pub fn due(&mut self, request: u64, now_us: u64) -> Option<String> {
        let seen = self.seen?;
        let since_us = now_us.saturating_sub(seen.read_at_us);
        if since_us >= KEPT_FOR.as_micros() as u64 {
            return None;
        }
        if self.told.is_some_and(|told| told.same(&seen.sighting)) {
            return None;
        }
        self.given.retain(|(given_to, _)| *given_to != request);
        if self.given.len() == GIVEN_MOST {
            self.given.pop_front();
        }
        self.given.push_back((request, seen.sighting));
        Some(note(seen.sighting.what, seen.ago_s + since_us / 1_000_000))
    }

    /// `request`'s answer was heard: the sighting it was given, if any, is
    /// told, and so spent for every request given that same sighting. A
    /// request given another (a newer one) keeps it. Whether anything was
    /// told.
    pub fn told(&mut self, request: u64, _now_us: u64) -> bool {
        let Some(at) = self
            .given
            .iter()
            .position(|(given_to, _)| *given_to == request)
        else {
            return false;
        };
        let (_, sighting) = self.given.remove(at).expect("found just now");
        self.given.retain(|(_, given)| !given.same(&sighting));
        // An older sighting's answer heard after a newer one's never
        // makes the newer one due again.
        if self.told.is_none_or(|told| told.at_ms <= sighting.at_ms) {
            self.told = Some(sighting);
        }
        true
    }
}

/// What the model is told: context only, never to be raised by the lamp.
pub fn note(what: Doing, ago_s: u64) -> String {
    let doing = match what {
        Doing::Drinking => "drinking",
        Doing::Eating => "eating",
        Doing::Break => "stretching",
        Doing::Nothing => "nothing",
    };
    format!(
        "(Context only, not to answer: the person you are talking with was seen {doing} {}. Don't mention it unless they bring it up; never say you saw it, never comment on it or on their body. Use it only so a reply fits, such as not suggesting what they just did.)",
        rounded(ago_s)
    )
}

/// A time ago in rounded words: "just now", "about 3 minutes ago",
/// "about 25 minutes ago", "about an hour ago", "about 2 hours ago".
fn rounded(ago_s: u64) -> String {
    let minutes = (ago_s + 30) / 60;
    match minutes {
        0..=1 => "just now".to_owned(),
        2..=10 => format!("about {minutes} minutes ago"),
        11..=52 => format!("about {} minutes ago", (minutes + 2) / 5 * 5),
        53..=89 => "about an hour ago".to_owned(),
        _ => format!("about {} hours ago", (minutes + 30) / 60),
    }
}

/// The provider worker's argument for the switch in the environment.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading `--second-doing` off a worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC_US: u64 = 1_000_000;
    /// A wall clock well past the epoch.
    const NOW_MS: u64 = 1_800_000_000_000;

    fn drank(ago_s: u32, confidence: f32) -> HumanDoing {
        HumanDoing {
            drank_s_ago: ago_s,
            ate_s_ago: 3600,
            break_s_ago: 3600,
            last: Doing::Drinking,
            confidence,
        }
    }

    #[test]
    fn a_sighting_is_told_once_and_only_once_heard() {
        let mut doing = DoingTold::default();
        doing.seen(Some((drank(120, 0.8), NOW_MS)), NOW_MS, 0);
        let note = doing.due(1, 0).expect("a fresh, sure sighting");
        assert!(note.starts_with("(Context only, not to answer"), "{note}");
        assert!(note.contains("seen drinking about 2 minutes ago"), "{note}");
        assert!(note.contains("Don't mention it unless they bring it up"));
        assert!(note.contains("never comment on it or on their body"));
        // Given, but its answer not heard (cut short): still due.
        assert!(doing.due(2, SEC_US).is_some(), "not heard: due again");
        doing.told(2, SEC_US);
        // The same sighting, read again by the next message 30 s on with
        // its timer 30 s longer: one sighting, not told again.
        doing.seen(
            Some((drank(150, 0.8), NOW_MS + 30_000)),
            NOW_MS + 30_000,
            30 * SEC_US,
        );
        assert_eq!(doing.due(3, 30 * SEC_US), None, "told once");
        // A new drink seen: told.
        doing.seen(
            Some((drank(5, 0.8), NOW_MS + 600_000)),
            NOW_MS + 600_000,
            600 * SEC_US,
        );
        let note = doing.due(4, 600 * SEC_US).expect("a new sighting");
        assert!(note.contains("just now"), "{note}");
        // Something else seen since: told.
        doing.told(4, 600 * SEC_US);
        let ate = HumanDoing {
            last: Doing::Eating,
            ate_s_ago: 60,
            ..drank(65, 0.7)
        };
        doing.seen(
            Some((ate, NOW_MS + 700_000)),
            NOW_MS + 700_000,
            700 * SEC_US,
        );
        assert!(doing.due(5, 700 * SEC_US).unwrap().contains("seen eating"));
    }

    #[test]
    fn a_stale_unsure_or_empty_read_is_not_told() {
        let mut doing = DoingTold::default();
        doing.seen(Some((drank(60, 0.8), NOW_MS)), NOW_MS, 0);
        assert_eq!(doing.due(1, KEPT_FOR.as_micros() as u64), None, "stale");
        for confidence in [0.49, f32::NAN] {
            doing.seen(Some((drank(60, confidence), NOW_MS)), NOW_MS, 0);
            assert_eq!(doing.due(1, 0), None, "{confidence}");
        }
        let nothing = HumanDoing {
            last: Doing::Nothing,
            confidence: 0.0,
            ..drank(600, 0.0)
        };
        doing.seen(Some((nothing, NOW_MS)), NOW_MS, 0);
        assert_eq!(doing.due(1, 0), None, "nothing seen this sitting");
        doing.seen(None, NOW_MS, 0);
        assert_eq!(doing.due(1, 0), None, "none on the bus");
    }

    #[test]
    fn a_read_exactly_half_sure_is_told_and_one_just_under_is_not() {
        let mut doing = DoingTold::default();
        doing.seen(Some((drank(60, SURE_FROM), NOW_MS)), NOW_MS, 0);
        assert!(doing.due(1, 0).is_some(), "0.5 or over is sure");
        let under = f32::from_bits(SURE_FROM.to_bits() - 1);
        assert!(under < SURE_FROM);
        doing.seen(Some((drank(60, under), NOW_MS)), NOW_MS, 0);
        assert_eq!(doing.due(2, 0), None, "just under 0.5 is unsure");
    }

    /// Heron's P3 on #6661: A is given one sighting, B a newer one, both in
    /// flight; A's answer heard spends only A's, and B's is still told.
    #[test]
    fn an_answer_heard_spends_only_the_sighting_its_own_request_was_given() {
        let mut doing = DoingTold::default();
        doing.seen(Some((drank(120, 0.8), NOW_MS)), NOW_MS, 0);
        assert!(doing.due(1, 0).is_some(), "A given the first drink");
        let ate = HumanDoing {
            last: Doing::Eating,
            ate_s_ago: 5,
            ..drank(125, 0.8)
        };
        doing.seen(Some((ate, NOW_MS + 1_000)), NOW_MS + 1_000, SEC_US);
        assert!(doing.due(2, SEC_US).unwrap().contains("seen eating"));
        // A request never given one, or one heard twice, spends nothing.
        assert!(!doing.told(9, SEC_US), "never given a note");
        assert!(doing.told(1, SEC_US), "A's answer heard");
        assert!(!doing.told(1, SEC_US), "A's already spent");
        // B's newer sighting is still due, to the next request too.
        assert!(doing.due(3, SEC_US).unwrap().contains("seen eating"));
        assert!(doing.told(2, SEC_US), "B's answer heard");
        assert_eq!(doing.due(4, SEC_US), None, "B's sighting told once");
        assert!(
            !doing.told(3, SEC_US),
            "3 was given B's sighting: spent with it"
        );
        // A late answer for an older sighting, heard after a newer one's,
        // never makes the newer one due again.
        let mut doing = DoingTold::default();
        doing.seen(Some((drank(120, 0.8), NOW_MS)), NOW_MS, 0);
        assert!(doing.due(1, 0).is_some(), "A given the drink");
        doing.seen(Some((ate, NOW_MS + 1_000)), NOW_MS + 1_000, SEC_US);
        assert!(doing.due(2, SEC_US).is_some(), "B given the meal");
        assert!(doing.told(2, SEC_US), "B's answer heard first");
        assert!(doing.told(1, SEC_US), "then A's");
        assert_eq!(doing.due(3, SEC_US), None, "the meal stays told");
    }

    #[test]
    fn what_was_given_is_kept_for_the_most_in_flight_and_no_more() {
        let mut doing = DoingTold::default();
        doing.seen(Some((drank(60, 0.8), NOW_MS)), NOW_MS, 0);
        for request in 1..=10 {
            assert!(doing.due(request, 0).is_some());
            // The same request asked again is kept once.
            assert!(doing.due(request, 0).is_some());
        }
        assert_eq!(doing.given.len(), GIVEN_MOST);
        assert!(!doing.told(1, 0), "the oldest let go past the most");
        assert!(doing.told(10, 0), "the newest kept");
        assert!(doing.given.is_empty(), "all were given that one sighting");
    }

    #[test]
    fn two_faces_or_none_in_view_tell_nothing_since_it_may_be_theirs() {
        for faces in [Some(0), Some(2), None] {
            let mut doing = DoingTold::default();
            doing.seen_in_view(Some((drank(60, 0.9), NOW_MS)), faces, NOW_MS, 0);
            assert_eq!(doing.due(1, 0), None, "{faces:?}");
        }
        let mut doing = DoingTold::default();
        doing.seen_in_view(Some((drank(60, 0.9), NOW_MS)), Some(1), NOW_MS, 0);
        assert!(doing.due(1, 0).is_some(), "one face");
    }

    #[test]
    fn the_time_is_said_in_rounded_words_and_the_kind_never_as_a_label() {
        assert_eq!(rounded(20), "just now");
        assert_eq!(rounded(170), "about 3 minutes ago");
        assert_eq!(rounded(23 * 60), "about 25 minutes ago");
        assert_eq!(rounded(70 * 60), "about an hour ago");
        assert_eq!(rounded(130 * 60), "about 2 hours ago");
        assert!(note(Doing::Break, 300).contains("seen stretching about 5 minutes ago"));
    }

    #[test]
    fn the_switch_is_on_only_when_set_and_its_flag_is_taken_off() {
        assert_eq!(arguments(Some("on")), [FLAG]);
        assert!(arguments(None).is_empty() && arguments(Some("yes")).is_empty());
        let flags = vec![FLAG.to_owned(), "--other".to_owned()];
        assert_eq!(take(&flags), (true, &flags[1..]));
        assert_eq!(take(&flags[1..]), (false, &flags[1..]));
    }
}
