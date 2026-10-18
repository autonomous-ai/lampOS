//! Lights asked for that lapse (`light/request`, #940, `docs/skills.md`):
//! a skill's glow, or anything else that must not leave the person's own
//! light changed. Each asker (`claim.source`) has one request at a time,
//! its newest; the ring shows the highest priority that holds, the newest
//! claim of equal priority (a renewal keeps its claim's place, so two
//! skills renewing in turn never swap the light), and the person's lasting
//! light once none does.
//!
//! An asker gives its light back by asking again with a time already
//! passed, or by not asking again before its time. A claim holds only on
//! the ring's own clock, and never for more than 30 s from now
//! ([`lamp_msgs::Claim::holds_at`]), so a skill renews what it keeps.
//!
//! The person comes first. When they ask for their own light
//! (`light/command`), every request below a person's whose claim began
//! before then gives way, and stays given way while its asker renews it
//! without a gap; the asker's next claim, after that one has lapsed or been
//! given back, is heard again. A claim given way still has its asker's one
//! place among the [`MOST_SOURCES`], so whatever is held can always be
//! given way: there is no second list that could fill up. A request refused
//! for want of a place is not held, so its asker's next request is a new
//! claim, heard as begun when it comes (Twelve on #993).
//!
//! The loser is told (#940): a request held but not shown is said
//! outranked, naming what is over it, once for each thing over it and
//! again after each renewal while it stays hidden ([`Requests::outranked`]).
use lamp_msgs::{Light, LightOutranked, LightRequest, Priority};

/// At most this many askers are kept at once, given way or not; another is
/// passed over until one's claim lapses or is given back.
pub const MOST_SOURCES: usize = 16;

struct Held {
    request: LightRequest,
    /// The order it was taken in, so the newest of equal priority wins.
    order: u64,
    /// When, on the ring's clock, this claim began: kept while it is
    /// renewed without a gap.
    since_us: u64,
    /// Sent before the person's light that was read in the same look, so
    /// given way as if it had come first.
    before_person: bool,
    /// What it was last said outranked by, since it was taken: none while
    /// it is shown, or before it is first said hidden.
    told: Option<String>,
}

pub struct Requests {
    clock_id: String,
    held: Vec<Held>,
    /// When the person last asked for their own light, on the ring's clock.
    person_at_us: Option<u64>,
    taken: u64,
}

impl Requests {
    /// Requests on the monotonic clock `clock_id` (the host's boot).
    pub fn new(clock_id: impl Into<String>) -> Self {
        Self {
            clock_id: clock_id.into(),
            held: Vec::new(),
            person_at_us: None,
            taken: 0,
        }
    }

    /// Take a request read at `now_us` on the ring's clock. With
    /// `before_person`, it was sent before the person's light read in the
    /// same look, so it gives way as it would have had it come first.
    /// Whether it is now held: a request that is not a light there is, from
    /// another clock, or past [`MOST_SOURCES`] is not, nor is one that
    /// gives the light back. A request held may still be given way.
    pub fn take(&mut self, request: LightRequest, now_us: u64, before_person: bool) -> bool {
        if request.check().is_err() || request.claim.clock_id != self.clock_id {
            return false;
        }
        let clock_id = &self.clock_id;
        let at = self
            .held
            .iter()
            .position(|held| held.request.claim.source == request.claim.source);
        // Renewed before its claim lapsed: the same claim, from when it began.
        let renewed = at
            .map(|at| &self.held[at])
            .filter(|held| held.request.claim.holds_at(clock_id, now_us));
        // A renewal keeps its claim's beginning and its place among equals
        // (the reviewer on #993: two skills renewing in turn swapped the
        // ring's colour at each renewal).
        let (since_us, was_before, order) = renewed.map_or((now_us, before_person, None), |held| {
            (held.since_us, held.before_person, Some(held.order))
        });
        if let Some(at) = at {
            self.held.swap_remove(at);
        }
        if !request.claim.holds_at(&self.clock_id, now_us) {
            return false;
        }
        let clock_id = &self.clock_id;
        self.held
            .retain(|held| held.request.claim.holds_at(clock_id, now_us));
        if self.held.len() >= MOST_SOURCES {
            return false;
        }
        let order = order.unwrap_or_else(|| {
            self.taken += 1;
            self.taken
        });
        self.held.push(Held {
            request,
            order,
            since_us,
            before_person: was_before,
            told: None,
        });
        true
    }

    /// The person asked for their own light at `now_us`: every request
    /// below a person's whose claim began before gives way.
    pub fn person_asked(&mut self, now_us: u64) {
        self.person_at_us = Some(now_us);
    }

    fn given_way(&self, held: &Held) -> bool {
        held.request.claim.priority < Priority::Person
            && (held.before_person || self.person_at_us.is_some_and(|at| held.since_us < at))
    }

    /// Where the request shown at `now_us` is held, if one holds and has
    /// not given way.
    fn shown_at(&self, now_us: u64) -> Option<usize> {
        self.held
            .iter()
            .enumerate()
            .filter(|(_, held)| {
                held.request.claim.holds_at(&self.clock_id, now_us) && !self.given_way(held)
            })
            .max_by_key(|(_, held)| (held.request.claim.priority, held.order))
            .map(|(at, _)| at)
    }

    /// The request shown at `now_us`, if one holds and has not given way.
    pub fn shown(&self, now_us: u64) -> Option<&LightRequest> {
        self.shown_at(now_us).map(|at| &self.held[at].request)
    }

    /// Tells `tell(source, by)` of each request that holds at `now_us` but
    /// is not shown: `by` is the source of the request shown over it, or
    /// [`LightOutranked::PERSON`] for the person's own light. Each is told
    /// once for what is over it, again when that changes, and again after
    /// each renewal while it stays hidden; the one shown is told nothing.
    pub fn outranked(&mut self, now_us: u64, mut tell: impl FnMut(&str, &str)) {
        let shown = self.shown_at(now_us);
        for at in 0..self.held.len() {
            let held = &self.held[at];
            let by = if Some(at) == shown || !held.request.claim.holds_at(&self.clock_id, now_us) {
                None
            } else if self.given_way(held) {
                Some(LightOutranked::PERSON)
            } else {
                // One holds that has not given way, so one is shown.
                shown.map(|shown| self.held[shown].request.claim.source.as_str())
            };
            if held.told.as_deref() == by {
                continue;
            }
            let by = by.map(str::to_owned);
            if let Some(by) = &by {
                tell(&held.request.claim.source, by);
            }
            self.held[at].told = by;
        }
    }

    /// The light shown at `now_us`, if a request holds.
    pub fn light(&self, now_us: u64) -> Option<&Light> {
        self.shown(now_us).map(|request| &request.light)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::Claim;

    const BOOT: &str = "boot-one";
    const S: u64 = 1_000_000;

    fn request(source: &str, priority: Priority, until_us: u64, colour: &str) -> LightRequest {
        LightRequest {
            claim: Claim {
                source: source.into(),
                priority,
                clock_id: BOOT.into(),
                until_us,
            },
            light: Light::new(true, colour, 3).unwrap(),
        }
    }

    fn colour(requests: &Requests, now_us: u64) -> Option<&str> {
        requests.light(now_us).map(|light| light.colour.as_str())
    }

    #[test]
    fn the_highest_priority_that_holds_is_shown_the_newest_of_equal_and_none_once_lapsed() {
        let mut requests = Requests::new(BOOT);
        assert_eq!(colour(&requests, 0), None);
        assert!(requests.take(
            request("skill:a", Priority::Skill, 10 * S, "blue"),
            0,
            false
        ));
        assert!(requests.take(request("idle", Priority::Idle, 20 * S, "red"), 0, false));
        assert_eq!(colour(&requests, S), Some("blue"));
        assert!(requests.take(
            request("skill:b", Priority::Skill, 5 * S, "green"),
            S,
            false
        ));
        assert_eq!(colour(&requests, 2 * S), Some("green"), "the newest skill");
        assert_eq!(colour(&requests, 5 * S), Some("blue"), "b has lapsed");
        assert_eq!(colour(&requests, 10 * S), Some("red"), "a has lapsed");
        assert_eq!(colour(&requests, 20 * S), None, "the person's light again");
    }

    /// The reviewer on #993: two skills at one priority, each renewing in
    /// turn, keep the light of the newest claim; renewals never swap it.
    #[test]
    fn renewals_keep_the_newest_claim_and_never_swap_the_light() {
        let mut requests = Requests::new(BOOT);
        // Each renews every 10 s, 5 s apart, each claim 12 s long: always
        // before its last one runs out.
        let a = |at: u64| request("skill:a", Priority::Skill, at + 12 * S, "blue");
        let b = |at: u64| request("skill:b", Priority::Skill, at + 12 * S, "green");
        requests.take(a(0), 0, false);
        requests.take(b(5 * S), 5 * S, false);
        assert_eq!(
            colour(&requests, 5 * S),
            Some("green"),
            "b is the newer claim"
        );
        for step in 1..=5 {
            let at = step * 10 * S;
            assert!(requests.take(a(at), at, false));
            assert_eq!(
                colour(&requests, at),
                Some("green"),
                "after a's renewal at {at}"
            );
            assert!(requests.take(b(at + 5 * S), at + 5 * S, false));
            assert_eq!(colour(&requests, at + 5 * S), Some("green"));
        }
        // b gives its light back: a's, still held, is shown.
        requests.take(
            request("skill:b", Priority::Skill, 0, "green"),
            56 * S,
            false,
        );
        assert_eq!(colour(&requests, 56 * S), Some("blue"));
    }

    #[test]
    fn an_asker_has_one_request_and_gives_its_light_back_by_a_time_passed() {
        let mut requests = Requests::new(BOOT);
        requests.take(
            request("skill:a", Priority::Skill, 10 * S, "blue"),
            0,
            false,
        );
        requests.take(
            request("skill:a", Priority::Skill, 10 * S, "pink"),
            S,
            false,
        );
        assert_eq!(colour(&requests, 2 * S), Some("pink"));
        assert!(!requests.take(
            request("skill:a", Priority::Skill, 2 * S, "pink"),
            2 * S,
            false
        ));
        assert_eq!(colour(&requests, 2 * S), None);
    }

    #[test]
    fn another_clock_too_far_ahead_or_too_many_askers_are_passed_over() {
        let mut requests = Requests::new(BOOT);
        let mut other = request("skill:a", Priority::Skill, 10 * S, "blue");
        other.claim.clock_id = "boot-two".into();
        assert!(!requests.take(other, 0, false));
        assert!(!requests.take(
            request("skill:a", Priority::Skill, 31 * S, "blue"),
            0,
            false
        ));
        for n in 0..MOST_SOURCES {
            assert!(requests.take(
                request(&format!("skill:{n}"), Priority::Skill, 10 * S, "blue"),
                0,
                false
            ));
        }
        assert!(!requests.take(
            request("one-more", Priority::Safety, 10 * S, "red"),
            0,
            false
        ));
        // Once some have lapsed, there is room again.
        requests.take(request("skill:0", Priority::Skill, S, "blue"), 0, false);
        assert!(requests.take(
            request("one-more", Priority::Safety, 10 * S, "red"),
            2 * S,
            false
        ));
        assert_eq!(colour(&requests, 2 * S), Some("red"));
    }

    #[test]
    fn the_persons_light_overrules_lower_requests_while_they_are_renewed() {
        let mut requests = Requests::new(BOOT);
        let blue = |until_us| request("skill:a", Priority::Skill, until_us, "blue");
        requests.take(blue(10 * S), 0, false);
        requests.take(request("hand", Priority::Person, 3 * S, "white"), 0, false);
        requests.person_asked(S);
        // A person's own request is not overruled by their light.
        assert_eq!(colour(&requests, S), Some("white"));
        assert_eq!(colour(&requests, 3 * S), None, "the skill gave way");
        // Renewed before its claim lapsed: the same claim, held and still
        // given way.
        assert!(requests.take(blue(14 * S), 5 * S, false));
        assert!(requests.take(blue(18 * S), 13 * S, false));
        assert_eq!(colour(&requests, 13 * S), None);
        // A new claim after that one lapsed is heard again.
        assert!(requests.take(blue(25 * S), 19 * S, false));
        assert_eq!(colour(&requests, 19 * S), Some("blue"));
        // So is one after the asker gave its light back.
        requests.person_asked(20 * S);
        assert_eq!(colour(&requests, 20 * S), None);
        assert!(!requests.take(blue(0), 21 * S, false));
        assert!(requests.take(blue(30 * S), 21 * S, false));
        assert_eq!(colour(&requests, 22 * S), Some("blue"));
    }

    #[test]
    fn a_request_sent_before_the_persons_light_is_overruled_as_if_it_had_come_first() {
        let mut requests = Requests::new(BOOT);
        requests.person_asked(0);
        assert!(requests.take(request("skill:a", Priority::Skill, 10 * S, "blue"), 0, true));
        assert_eq!(colour(&requests, 0), None);
        // Its renewal is the same claim, still given way.
        requests.take(
            request("skill:a", Priority::Skill, 12 * S, "blue"),
            S,
            false,
        );
        assert_eq!(colour(&requests, S), None);
        assert!(requests.take(request("hand", Priority::Person, 3 * S, "white"), 0, true));
        assert_eq!(colour(&requests, S), Some("white"));
    }

    /// #940: the loser is told what is over it, once, again when that
    /// changes or it renews, and never while it is shown or once lapsed.
    #[test]
    fn a_request_held_but_not_shown_is_told_what_is_over_it() {
        let told = |requests: &mut Requests, now_us| {
            let mut said = Vec::new();
            requests.outranked(now_us, |source, by| said.push(format!("{source} by {by}")));
            said.sort();
            said
        };
        let mut requests = Requests::new(BOOT);
        let a = |until_us| request("skill:a", Priority::Skill, until_us, "blue");
        let b = |until_us| request("skill:b", Priority::Skill, until_us, "red");
        requests.take(a(15 * S), 0, false);
        assert!(told(&mut requests, 0).is_empty(), "alone, it is shown");
        // The newest claim of equal priority wins: a loses its place.
        requests.take(b(15 * S), S, false);
        assert_eq!(told(&mut requests, S), ["skill:a by skill:b"]);
        assert!(told(&mut requests, 2 * S).is_empty(), "said once");
        // Its renewal keeps b over it, and it is told again; b's is shown
        // and told nothing.
        requests.take(a(20 * S), 5 * S, false);
        assert_eq!(told(&mut requests, 5 * S), ["skill:a by skill:b"]);
        requests.take(b(20 * S), 6 * S, false);
        // Taken below the light shown: told at once.
        requests.take(
            request("idle", Priority::Idle, 20 * S, "warm"),
            6 * S,
            false,
        );
        assert_eq!(told(&mut requests, 6 * S), ["idle by skill:b"]);
        // The conversation over all three, and b over the others again once
        // it lapses.
        let voice = request("voice", Priority::Conversation, 10 * S, "white");
        requests.take(voice, 7 * S, false);
        assert_eq!(
            told(&mut requests, 7 * S),
            ["idle by voice", "skill:a by voice", "skill:b by voice"]
        );
        assert_eq!(
            told(&mut requests, 10 * S),
            ["idle by skill:b", "skill:a by skill:b"]
        );
        // The person's own light over every one.
        requests.person_asked(11 * S);
        assert_eq!(
            told(&mut requests, 11 * S),
            ["idle by person", "skill:a by person", "skill:b by person"]
        );
        // One given back, or lapsed, is told nothing more.
        assert!(!requests.take(b(0), 12 * S, false));
        assert!(told(&mut requests, 12 * S).is_empty());
        assert!(told(&mut requests, 20 * S).is_empty());
    }

    /// The reviewer on #993 (11:35): sixteen skills, the person's light, a
    /// seventeenth skill, the person's light again. A claim given way keeps
    /// its place, so the seventeenth is not held and cannot show over the
    /// person's light; it is heard only as a new claim once a place is free.
    #[test]
    fn whatever_is_held_can_always_be_given_way_however_many_ask() {
        let mut requests = Requests::new(BOOT);
        let skill =
            |n: usize, until_us| request(&format!("skill:{n}"), Priority::Skill, until_us, "blue");
        for n in 0..MOST_SOURCES {
            assert!(requests.take(skill(n, 10 * S), 0, false));
        }
        requests.person_asked(S);
        assert_eq!(colour(&requests, S), None);
        // The sixteen renew and keep their places, given way.
        for n in 0..MOST_SOURCES {
            assert!(requests.take(skill(n, 12 * S), 2 * S, false));
        }
        let seventeenth = |until_us| request("skill:17", Priority::Skill, until_us, "red");
        assert!(
            !requests.take(seventeenth(12 * S), 2 * S, false),
            "no place"
        );
        requests.person_asked(3 * S);
        assert!(!requests.take(seventeenth(14 * S), 4 * S, false));
        assert_eq!(colour(&requests, 4 * S), None, "the person's light");
        // The sixteen let go: the seventeenth's next request is a claim
        // begun after the person's light, and is shown.
        assert!(requests.take(seventeenth(20 * S), 12 * S, false));
        assert_eq!(colour(&requests, 12 * S), Some("red"));
        // And a later person's light gives it way in its turn.
        requests.person_asked(13 * S);
        assert!(requests.take(seventeenth(24 * S), 14 * S, false));
        assert_eq!(colour(&requests, 14 * S), None);
    }
}
