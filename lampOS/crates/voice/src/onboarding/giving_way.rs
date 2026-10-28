//! "Giving way" (#1800): the lamp invites its person to move its head gently
//! by hand, gives way, and says so.
//!
//! The invitation is one exact token on `onboarding/pending`, as the privacy
//! and near-call steps' are: the core onboarding skill asks the say service
//! for it, and only the whole invitation's playback opens a try's minute.
//! The body is listened to from the moment the invitation is taken to be
//! said, though: a person pushes as soon as they are asked, while the lamp
//! is still saying it, and does not push again (lamp-29e7, 28 Oct, #3702:
//! the body said the hand 15 s before the try opened). The try's window is
//! in Unix milliseconds, as the bus's headers are; the coordinator's
//! monotonic clock is only its deadline and the clock-jump check. The body's
//! own `since_us` is its monotonic clock and is never read as a time here.
//!
//! The step is done only on the body's evidence (#780, `docs/body.md`, "Giving
//! way to a hand"): in the window, `body/hand` says pushed, and while it does
//! a head joint (`base_yaw`, `head_pitch`, `head_roll` on `body/joint_states`)
//! is more than the body's own `HAND_MOVED` counts from where it was asked to
//! be before (the body's test of "a hand moved it") with its goal within
//! `HAND_MOVED` of where it is (the body holds it where the hand has it, the
//! negation of its "in a hand" test), and then `body/hand` says released.
//! "Before" is the oldest goal of the last second with no hand said
//! ([`BEFORE_MS`]): the body gives way where the hand already has it, its
//! goal there, a step before it says the hand (lamp-52e6, 27 Oct, #3504),
//! so the last goal before the push is already the hand's.
//! The lamp is never silent through the minute (#3702): the first push of a
//! try is answered at once (`give_felt`), and a try with nothing felt
//! [`PROMPT_AFTER_US`] into its minute is prompted once (`give_prompt`),
//! and a push let go before the head moved enough is asked for a little
//! more (`give_more`).
//! The model may only skip it. Nothing here asks the body for anything.
//! What it saw and decided is said on the journal (`lamp-voice: giving_way:`).
#[cfg(test)]
pub use super::legacy_words::{
    GIVE_AGAIN, GIVE_ASK, GIVE_DONE, GIVE_FAILED, GIVE_FELT, GIVE_MORE, GIVE_PROMPT,
};

use super::{
    Delivery, FRESH_MS, GONE_US, GUIDE_EVERY_US, MOST_UNSAID, NOT_SAID, Onboarding, Outcome,
    SIT_GIVE_UP_US, START_QUIET_US, STEP_GIVE_UP_US, Step, TIMED_OUT_QUIET_US, fresh_token,
    near::CLOCK_SLACK_MS, unix_ms,
};
use lamp_turn_owner::BootId;
use serde_json::json;
use std::collections::VecDeque;
use std::time::Duration;

/// The joints that carry the head, as `body/hand` covers them. The base is
/// one: "take its head, move it a little" turned only the base on lamp-52e6.
const HEAD: [&str; 3] = ["base_yaw", "head_pitch", "head_roll"];
/// The body's "a hand moved it" (`lamp_body::HAND_MOVED`, 15 counts), in
/// radians: `body/joint_states` says 4,096 counts a turn.
const MOVED_RADIANS: f64 = lamp_body::HAND_MOVED as f64 * std::f64::consts::TAU / 4_096.0;
/// How far back the head's goals before a push are kept: the body's second
/// in a hand (`HAND_STEPS`), well past its few steps of pushing back before
/// it gives way.
const BEFORE_MS: u64 = 1_000;
/// The body's publisher name (`lamp_body::bus_out::SOURCE`, a Linux-only
/// module): anything else on its topics is not the body.
const BODY: &str = "body";
/// A try is given as long as "where you sit" gives a person to act with
/// their body.
const WINDOW_US: u64 = SIT_GIVE_UP_US;
const WINDOW_MS: u64 = WINDOW_US / 1_000;
/// A push felt before the minute's end is waited for this long past it to
/// be let go (the Reviewer-lead's P3 on #4002).
const LET_GO_US: u64 = 5_000_000;
/// Invitations per setup/redo, the privacy step's budget, kept in the
/// onboarding file (`Progress::giving_way_tries`) across restarts.
pub(super) const MOST_TRIES: u8 = 2;
/// The bus is read at most this often.
const LOOK_EVERY_US: u64 = 20_000;
/// A try with nothing felt this far into its minute is prompted once:
/// touch's guide time, a few seconds, never a silent minute (#3702).
const PROMPT_AFTER_US: u64 = GUIDE_EVERY_US;

/// One reading from the body, validated and dated in Unix milliseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Felt {
    Hand {
        at_ms: u64,
        pushed: bool,
    },
    /// Each head joint's position and goal, in [`HEAD`]'s order.
    Joints {
        at_ms: u64,
        head: [(f64, f64); 3],
    },
    /// The body began again, or a message was lost: a push being watched
    /// is not judged on part of it.
    Restart,
    /// From another publisher, not parsing or out of range.
    Invalid,
}

impl Felt {
    pub(crate) fn hand(at_ms: u64, source: &str, hand: &lamp_msgs::Hand) -> Self {
        // Not pushed, the body says 0 for when the push began.
        if source != BODY || (!hand.pushed && hand.since_us != 0) {
            return Self::Invalid;
        }
        Self::Hand {
            at_ms,
            pushed: hand.pushed,
        }
    }

    pub(crate) fn joints(at_ms: u64, source: &str, joints: &lamp_msgs::JointState) -> Self {
        let count = joints.name.len();
        if source != BODY || joints.position.len() != count || joints.commanded.len() != count {
            return Self::Invalid;
        }
        let mut head = [(0.0, 0.0); 3];
        for (slot, name) in head.iter_mut().zip(HEAD) {
            let Some(at) = joints.name.iter().position(|joint| joint == name) else {
                return Self::Invalid;
            };
            let (position, goal) = (joints.position[at], joints.commanded[at]);
            if !position.is_finite() || !goal.is_finite() {
                return Self::Invalid;
            }
            *slot = (position, goal);
        }
        Self::Joints { at_ms, head }
    }

    fn at_ms(&self) -> Option<u64> {
        match self {
            Self::Hand { at_ms, .. } | Self::Joints { at_ms, .. } => Some(*at_ms),
            Self::Restart | Self::Invalid => None,
        }
    }
}

/// The body's two topics, each drained without waiting.
pub(crate) struct Feel {
    joints: lamp_bus::Subscriber,
    hands: lamp_bus::Subscriber,
    looked_at_us: Option<u64>,
}

impl Feel {
    /// Only with the bus switched on; a bus that cannot be used leaves the
    /// step unavailable.
    pub(crate) fn from_environment() -> Option<Self> {
        if std::env::var(crate::bus_out::ENVIRONMENT).ok().as_deref() != Some("on") {
            return None;
        }
        let bus = lamp_bus::Bus::from_environment().ok()?;
        let subscribe = |topic, kind| bus.subscribe_typed(topic, "onboarding", kind);
        Some(Self {
            joints: subscribe(lamp_msgs::JointState::TOPIC, lamp_msgs::JointState::TYPE).ok()?,
            hands: subscribe(lamp_msgs::Hand::TOPIC, lamp_msgs::Hand::TYPE).ok()?,
            looked_at_us: None,
        })
    }

    /// What arrived since the last look, oldest first, at most every
    /// [`LOOK_EVERY_US`]. The body says its joints before its hand in each
    /// step, so of two at the same millisecond the joints come first.
    pub(crate) fn take(&mut self, now_us: u64) -> Vec<Felt> {
        if self
            .looked_at_us
            .is_some_and(|at| now_us.saturating_sub(at) < LOOK_EVERY_US)
        {
            return Vec::new();
        }
        self.looked_at_us = Some(now_us);
        let mut felt = Vec::new();
        let mut drain = |subscriber: &mut lamp_bus::Subscriber,
                         convert: &dyn Fn(&lamp_bus::Message) -> Felt| {
            for _ in 0..32 {
                let Ok(Some(message)) = subscriber.recv(Duration::ZERO) else {
                    break;
                };
                if message.publisher_restarted || message.gap > 0 {
                    felt.push(Felt::Restart);
                }
                felt.push(convert(&message));
            }
        };
        drain(&mut self.joints, &|message| {
            message
                .parse::<lamp_msgs::JointState>()
                .map_or(Felt::Invalid, |joints| {
                    Felt::joints(message.header.at_unix_ms, &message.header.source, &joints)
                })
        });
        drain(&mut self.hands, &|message| {
            message
                .parse::<lamp_msgs::Hand>()
                .map_or(Felt::Invalid, |hand| {
                    Felt::hand(message.header.at_unix_ms, &message.header.source, &hand)
                })
        });
        // A restart or a bad message stays where it was read: it spoils
        // whatever push is being watched then.
        felt.sort_by_key(|felt| felt.at_ms().unwrap_or(0));
        felt
    }
}

struct Offer {
    token: u64,
    attempt: u8,
    ready: bool,
    claimed: bool,
    /// Its attempt is committed to the onboarding file.
    persisted: bool,
    offered_us: u64,
    bound: Option<(BootId, u64)>,
}

struct Window {
    /// Unix milliseconds when the invitation was taken to be said: the body
    /// is read from then (#3702).
    from_ms: u64,
    /// Unix milliseconds, read when the whole invitation's playback was
    /// known; until then, `from_ms`.
    ended_ms: u64,
    /// The coordinator's clock then: the deadline and the clock-jump check only.
    ended_us: u64,
    /// The whole invitation was played: the minute runs.
    open: bool,
}

#[derive(Default)]
pub(super) struct GivingWayFlow {
    /// The coordinator has a body, the bus and the say service.
    available: bool,
    pub(super) generation: u64,
    offer: Option<Offer>,
    window: Option<Window>,
    retry_after_us: u64,
    /// The head's goals, with their Unix milliseconds, at the readings with
    /// no hand on it in the last [`BEFORE_MS`], oldest first.
    before: VecDeque<(u64, [f64; 3])>,
    pushed: bool,
    /// In this push, the head joint farthest from where it was asked to be,
    /// one held where the hand has it first.
    moved: Option<Moved>,
    /// Pushes the body said in this try, for the journal.
    pushes: u32,
    /// This try's `give_felt`, and its `give_prompt`, are owed or said.
    felt_said: bool,
    prompted: bool,
    /// The coordinator's clock when a push last began or ended.
    pushed_us: u64,
    /// Invitations the voice never said, offered again ([`MOST_UNSAID`]).
    unsaid: u8,
}

/// A head joint read during a push, in radians.
#[derive(Clone, Copy)]
struct Moved {
    joint: usize,
    from: f64,
    position: f64,
    goal: f64,
}

impl Moved {
    fn distance(&self) -> f64 {
        (self.position - self.from).abs()
    }

    /// A hand moved it, and the body holds it there.
    fn by_hand(&self) -> bool {
        self.distance() > MOVED_RADIANS && (self.goal - self.position).abs() <= MOVED_RADIANS
    }

    fn told(&self) -> String {
        let counts = |radians: f64| 2_048.0 + radians * 4_096.0 / std::f64::consts::TAU;
        format!(
            "{} at {:.0} counts, {:.0} from its goal before at {:.0}, its goal {:.0}",
            HEAD[self.joint],
            counts(self.position),
            self.distance() * 4_096.0 / std::f64::consts::TAU,
            counts(self.from),
            counts(self.goal)
        )
    }
}

impl GivingWayFlow {
    fn forget_push(&mut self) {
        self.before.clear();
        self.pushed = false;
        self.moved = None;
    }
}

impl Onboarding {
    /// The coordinator says whether it has a body, the bus and the say
    /// service. Without them the step is `not_available`.
    pub(crate) fn set_giving_way_available(&mut self, available: bool) {
        if self.giving.available && !available {
            self.invalidate_giving();
        }
        self.giving.available = available;
    }

    pub(super) fn giving_way_available(&self) -> bool {
        self.giving.available
    }

    /// Redo: the walk's identity is forgotten too (its kept tries go with
    /// the rest of the onboarding file).
    pub(super) fn redo_giving(&mut self) {
        self.forget_giving();
        self.giving.generation = 0;
        self.giving.unsaid = 0;
    }

    /// Drop everything in flight for this step. An offer never handed to the
    /// model gives its try back; a claimed one counts.
    fn invalidate_giving(&mut self) {
        if let Some(offer) = self.giving.offer.as_ref()
            && !offer.claimed
            && self.desired.giving_way_tries == offer.attempt
        {
            self.desired.giving_way_tries = offer.attempt - 1;
            self.save();
        }
        self.forget_giving();
    }

    /// Memory only: an offer or try in flight is dropped.
    pub(super) fn forget_giving(&mut self) {
        self.giving.offer = None;
        self.giving.window = None;
        self.giving.forget_push();
    }

    /// The keeper's pause while this step is led: it is `not_available`,
    /// offered again later as any step that was.
    pub(super) fn giving_way_paused(&mut self, now_us: u64) {
        let led = self
            .led
            .as_ref()
            .is_some_and(|led| led.step == Step::GivingWay && led.asked_us.is_some());
        if led || self.giving.offer.is_some() || self.giving.window.is_some() {
            self.invalidate_giving();
            self.finish(Step::GivingWay, Outcome::NotAvailable, None, now_us);
        }
    }

    pub(super) fn giving_pending(&self) -> Option<lamp_msgs::OnboardingInvitation> {
        let offer = self.giving.offer.as_ref()?;
        Some(lamp_msgs::OnboardingInvitation {
            invitation: offer.token,
            generation: self.giving.generation,
            attempt: u64::from(offer.attempt),
            step: lamp_msgs::OnboardingStep::GivingWay,
            ready: offer.ready && !offer.claimed && self.storage_idle(),
        })
    }

    pub(super) fn take_giving_invitation(&mut self, token: u64, now_us: u64) -> Option<String> {
        if !self.storage_idle()
            || self
                .seen_us
                .is_none_or(|seen| now_us.saturating_sub(seen) > FRESH_MS * 1_000)
        {
            return None;
        }
        let offer = self.giving.offer.as_mut()?;
        if offer.token != token || !offer.ready || offer.claimed {
            return None;
        }
        offer.claimed = true;
        offer.ready = false;
        let attempt = offer.attempt;
        // The body is read from now: a push while the lamp is still saying
        // it is the try's (#3702).
        let from_ms = (self.unix_clock)();
        self.giving.forget_push();
        self.giving.pushes = 0;
        self.giving.felt_said = false;
        self.giving.pushed_us = 0;
        self.giving.prompted = false;
        self.giving.window = Some(Window {
            from_ms,
            ended_ms: from_ms,
            ended_us: now_us,
            open: false,
        });
        // A second try within the walk says what didn't work and how to do
        // it (#3769); a re-offer later opens as the first did
        // (Reviewer-three's P3 on #3781).
        if attempt > 1
            && !self.desired.steps.contains_key(Step::GivingWay.name())
            && let Some(again) = &self.definition.words.give_again
        {
            return Some(again.clone());
        }
        Some(
            self.definition
                .note(
                    Step::GivingWay,
                    self.desired.steps.contains_key(Step::GivingWay.name()),
                )
                .to_owned(),
        )
    }

    pub(super) fn bind_giving_invitation(
        &mut self,
        token: u64,
        voice_boot: BootId,
        turn: u64,
    ) -> bool {
        let Some(offer) = self.giving.offer.as_mut() else {
            return false;
        };
        if offer.token != token || !offer.claimed || offer.bound.is_some() || turn == 0 {
            return false;
        }
        offer.bound = Some((voice_boot, turn));
        true
    }

    pub(super) fn giving_binding(&self) -> Option<(BootId, u64)> {
        self.giving.offer.as_ref().and_then(|offer| offer.bound)
    }

    pub(super) fn giving_invitation_refused(&mut self, token: u64, now_us: u64) {
        if self
            .giving
            .offer
            .as_ref()
            .is_some_and(|offer| offer.token == token && offer.bound.is_none())
        {
            self.forget_giving();
            self.giving.retry_after_us = now_us.saturating_add(1_000_000);
        }
    }

    /// Only the whole invitation's playback opens a try. Its start is read
    /// from the Unix clock now, at that boundary.
    pub(super) fn giving_invitation_finished(
        &mut self,
        voice_boot: BootId,
        turn: u64,
        delivery: Delivery,
        now_us: u64,
    ) {
        if self.giving_binding() != Some((voice_boot, turn)) {
            return;
        }
        let Some(offer) = self.giving.offer.take() else {
            return;
        };
        // Never said, or cut or gapped before any push began (on lamp-52e6
        // the lamp yielded to its own voice, #4035): the person was never
        // asked, so its try is given back and it is said again,
        // [`MOST_UNSAID`] times in all (#3754).
        if delivery == Delivery::Unsaid || (delivery != Delivery::Whole && self.giving.pushes == 0)
        {
            if self.desired.giving_way_tries == offer.attempt {
                self.desired.giving_way_tries = offer.attempt - 1;
                self.save();
            }
            self.giving.unsaid = self.giving.unsaid.saturating_add(1);
            let again = self.giving.unsaid <= MOST_UNSAID;
            let why = match delivery {
                Delivery::Unsaid => NOT_SAID,
                Delivery::Gapped => "the invitation was played with a gap",
                _ => "the invitation was not played whole",
            };
            eprintln!(
                "lamp-voice: giving_way: {why}; {}",
                if again {
                    "asked again"
                } else {
                    "left for later"
                }
            );
            self.records.push(json!({"kind":"onboarding_giving_way_try",
                "felt":false,"why":why,"offered_again":again,"at_us":now_us}));
            self.forget_giving();
            if again {
                self.giving.retry_after_us = now_us.saturating_add(1_000_000);
            } else {
                self.finish(Step::GivingWay, Outcome::NotAvailable, None, now_us);
            }
            return;
        }
        // Cut by the person talking while they push ("like this?"): the
        // push is kept and the try goes on (the Reviewer-lead's P2 on #4002).
        // A push felt while it was said is kept: the minute runs from here.
        let ended_ms = (self.unix_clock)();
        let window = self.giving.window.get_or_insert(Window {
            from_ms: ended_ms,
            ended_ms,
            ended_us: now_us,
            open: true,
        });
        (window.ended_ms, window.ended_us, window.open) = (ended_ms, now_us, true);
        eprintln!(
            "lamp-voice: giving_way: the invitation was played whole; the try is open, {} push(es) felt while it was said",
            self.giving.pushes
        );
    }

    /// One reading from the body. Judged as it comes, on the loop, without
    /// waiting for storage: the outcome is kept by the storage worker after.
    pub(crate) fn giving_way_felt(&mut self, felt: Felt, now_us: u64) {
        // Any push while the step is led is an attempt, inside a try's
        // window or not: one with none is `later`, never `failed` (#3996).
        if matches!(felt, Felt::Hand { pushed: true, .. })
            && self
                .led
                .as_ref()
                .is_some_and(|led| led.step == Step::GivingWay)
        {
            self.mark_tried(Step::GivingWay);
        }
        let Some(window) = self.giving.window.as_ref() else {
            return;
        };
        let (from_ms, ended_ms, ended_us) = (window.from_ms, window.ended_ms, window.ended_us);
        if let Some(at_ms) = felt.at_ms()
            && (at_ms < from_ms || at_ms > ended_ms + WINDOW_MS + LET_GO_US / 1_000)
        {
            return;
        }
        let flow = &mut self.giving;
        match felt {
            Felt::Restart | Felt::Invalid => flow.forget_push(),
            Felt::Joints { at_ms, head } if !flow.pushed => {
                flow.before.push_back((at_ms, head.map(|(_, goal)| goal)));
                while flow
                    .before
                    .front()
                    .is_some_and(|&(was, _)| was.saturating_add(BEFORE_MS) < at_ms)
                {
                    flow.before.pop_front();
                }
            }
            Felt::Joints { head, .. } => {
                let Some(&(_, before)) = flow.before.front() else {
                    return;
                };
                for (joint, (&(position, goal), from)) in head.iter().zip(before).enumerate() {
                    let read = Moved {
                        joint,
                        from,
                        position,
                        goal,
                    };
                    if flow.moved.is_none_or(|was| {
                        (read.by_hand(), read.distance()) > (was.by_hand(), was.distance())
                    }) {
                        flow.moved = Some(read);
                    }
                }
            }
            Felt::Hand { pushed: true, .. } => {
                if !flow.pushed {
                    flow.pushes += 1;
                    eprintln!(
                        "lamp-voice: giving_way: the body says a hand is on it, push {} of this try",
                        flow.pushes
                    );
                }
                flow.pushed = true;
                flow.pushed_us = now_us;
                // Answered at once, on the first push of a try (#3702).
                if !std::mem::replace(&mut flow.felt_said, true)
                    && let Some(line) = &self.definition.words.give_felt
                {
                    self.owed = Some(line.clone());
                }
            }
            Felt::Hand { pushed: false, .. } => {
                if !flow.pushed {
                    return;
                }
                flow.pushed_us = now_us;
                let moved = flow.moved.take();
                flow.pushed = false;
                let given_way = moved.is_some_and(|moved| moved.by_hand());
                eprintln!(
                    "lamp-voice: giving_way: the body says the hand let go; {}: {}",
                    moved.map_or("no head reading during the push".into(), |moved| moved
                        .told()),
                    if given_way {
                        "given way"
                    } else {
                        "not given way (wanted more than HAND_MOVED counts from its goal before, its goal within HAND_MOVED of it)"
                    }
                );
                if !given_way {
                    // Let go before the head moved enough: a little more,
                    // each time (#3702). Too hard is never a failure: the
                    // body gives way.
                    if let Some(line) = &self.definition.words.give_more {
                        self.owed = Some(line.clone());
                    }
                    return;
                }
                let expected_ms = ended_ms + now_us.saturating_sub(ended_us) / 1_000;
                if (self.unix_clock)().abs_diff(expected_ms) > CLOCK_SLACK_MS {
                    self.records.push(json!({"kind":"onboarding_giving_way_try",
                        "felt":false,"why":"the clock jumped","at_us":now_us}));
                    flow_end(self, now_us);
                    return;
                }
                self.forget_giving();
                self.records
                    .push(json!({"kind":"onboarding_giving_way_try","felt":true,"at_us":now_us}));
                self.finish(Step::GivingWay, Outcome::Done, None, now_us);
                self.owed = Some(self.definition.words.give_done.clone());
            }
        }
    }

    /// A try's minute ended without the evidence. Run every loop, whatever
    /// storage is doing, as the evidence is judged.
    pub(super) fn giving_judge(&mut self, now_us: u64) {
        let Some(ended_us) = self
            .giving
            .window
            .as_ref()
            .filter(|window| window.open)
            .map(|window| window.ended_us)
        else {
            return;
        };
        let open_us = now_us.saturating_sub(ended_us);
        // Nothing felt a few seconds into the minute, or since a push that
        // was not enough: prompted once, never a silent minute (#3702).
        let flow = &mut self.giving;
        if !flow.pushed
            && now_us.saturating_sub(ended_us.max(flow.pushed_us)) >= PROMPT_AFTER_US
            && !std::mem::replace(&mut flow.prompted, true)
            && let Some(line) = &self.definition.words.give_prompt
        {
            eprintln!("lamp-voice: giving_way: nothing felt yet; prompted once");
            self.owed = Some(line.clone());
        }
        // A push still on at the minute's end is waited for a little.
        if open_us >= WINDOW_US && (!self.giving.pushed || open_us >= WINDOW_US + LET_GO_US) {
            eprintln!(
                "lamp-voice: giving_way: the try's minute ended, not given way; the body said a hand {} times",
                self.giving.pushes
            );
            self.records.push(json!({"kind":"onboarding_giving_way_try",
                "felt":false,"why":"no hand moved the head","at_us":now_us}));
            flow_end(self, now_us);
        }
    }

    /// The step being led, with storage idle.
    pub(super) fn giving_tick(
        &mut self,
        now_us: u64,
        quiet_us: u64,
        can_speak: bool,
        present: bool,
    ) {
        if self
            .seen_us
            .is_some_and(|seen| now_us.saturating_sub(seen) >= GONE_US)
        {
            if self.giving.offer.is_some() || self.giving.window.is_some() {
                self.records
                    .push(json!({"kind":"onboarding_stopped","why":"nobody_there",
                    "step":Step::GivingWay.name(),"at_us":now_us}));
            }
            self.invalidate_giving();
            self.led = None;
            return;
        }
        // Listening for a hand, from the invitation's start.
        if self
            .giving
            .window
            .as_ref()
            .is_some_and(|window| window.open)
        {
            return;
        }
        if self.giving.generation == 0 {
            let Ok(generation) = fresh_token() else {
                return;
            };
            self.giving.generation = generation;
        }
        let ready = can_speak && present && quiet_us >= START_QUIET_US;
        if let Some(offer) = self.giving.offer.as_mut() {
            if !offer.persisted {
                if self.committed.giving_way_tries >= offer.attempt {
                    offer.persisted = true;
                } else {
                    self.desired.giving_way_tries = offer.attempt;
                    self.save();
                    return;
                }
            }
            offer.ready = ready && !offer.claimed;
            if offer.bound.is_none()
                && now_us.saturating_sub(offer.offered_us) >= STEP_GIVE_UP_US
                && quiet_us >= TIMED_OUT_QUIET_US
            {
                let token = offer.token;
                self.giving_invitation_refused(token, now_us);
            }
            return;
        }
        // A walk rebuilt (a restart, a mute's end) with its budget spent
        // does not ask again.
        if self.desired.giving_way_tries >= MOST_TRIES {
            flow_end(self, now_us);
            return;
        }
        if !ready || now_us < self.giving.retry_after_us {
            return;
        }
        let Ok(token) = fresh_token() else {
            return;
        };
        let attempt = self.desired.giving_way_tries + 1;
        self.giving.offer = Some(Offer {
            token,
            attempt,
            ready: false,
            claimed: false,
            persisted: false,
            offered_us: now_us,
            bound: None,
        });
        // Led anew (offered again later, or asked once the person is back):
        // its unsaid count starts again, as the step's own, and a re-offer is
        // dated so the skill's interval holds before the next (Third's P3 on
        // #4045).
        if let Some(led) = self.led.as_mut()
            && led.asked_us.replace(now_us).is_none()
        {
            self.giving.unsaid = 0;
            if let Some(kept) = self.desired.steps.get_mut(Step::GivingWay.name()) {
                kept.offered_at_unix_s = Some(unix_ms() / 1_000);
            }
        }
        // Its budget is committed before the invitation is published.
        self.desired.giving_way_tries = attempt;
        self.save();
        self.records.push(
            json!({"kind":"onboarding_step","step":Step::GivingWay.name(),
            "state":"offered","try":attempt,"at_us":now_us}),
        );
    }
}

/// A try ended without the evidence: asked once more within the budget,
/// else failed and said so.
fn flow_end(walk: &mut Onboarding, now_us: u64) {
    walk.forget_giving();
    if walk.desired.giving_way_tries < MOST_TRIES {
        walk.giving.retry_after_us = now_us.saturating_add(1_000_000);
        return;
    }
    if walk.was_tried(Step::GivingWay) {
        walk.finish(Step::GivingWay, Outcome::Failed, None, now_us);
        walk.owed = Some(walk.definition.words.give_failed.clone());
    } else {
        // No push felt in either try: later, said so (#3996).
        walk.finish(Step::GivingWay, Outcome::Later, None, now_us);
        walk.owed = Some(walk.definition.later_line());
    }
}
