//! "Giving way" (#1800): the lamp invites its person to move its head gently
//! by hand, gives way, and says so.
//!
//! The invitation is one exact token on `onboarding/pending`, as the privacy
//! and near-call steps' are: the core onboarding skill asks the say service
//! for it, and only the whole invitation's playback opens a try. The try's
//! window is in Unix milliseconds, as the bus's headers are; the coordinator's
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
//! The model may only skip it. Nothing here asks the body for anything.
use super::{
    FRESH_MS, GONE_US, Onboarding, Outcome, SIT_GIVE_UP_US, START_QUIET_US, STEP_GIVE_UP_US, Step,
    TIMED_OUT_QUIET_US, fresh_token, near::CLOCK_SLACK_MS,
};
use lamp_turn_owner::BootId;
use serde_json::json;
use std::time::Duration;

/// The joints that carry the head, as `body/hand` covers them.
const HEAD: [&str; 3] = ["base_yaw", "head_pitch", "head_roll"];
/// The body's "a hand moved it" (`lamp_body::HAND_MOVED`, 15 counts), in
/// radians: `body/joint_states` says 4,096 counts a turn.
const MOVED_RADIANS: f64 = lamp_body::HAND_MOVED as f64 * std::f64::consts::TAU / 4_096.0;
/// The body's publisher name (`lamp_body::bus_out::SOURCE`, a Linux-only
/// module): anything else on its topics is not the body.
const BODY: &str = "body";
/// A try is given as long as "where you sit" gives a person to act with
/// their body.
const WINDOW_US: u64 = SIT_GIVE_UP_US;
const WINDOW_MS: u64 = WINDOW_US / 1_000;
/// Invitations per setup/redo, the privacy step's budget, kept in the
/// onboarding file (`Progress::giving_way_tries`) across restarts.
pub(super) const MOST_TRIES: u8 = 2;
/// The bus is read at most this often.
const LOOK_EVERY_US: u64 = 20_000;

pub const GIVE_ASK: &str = "(Nobody has spoken. Tell them in a few words that your head gives way to a hand: ask them to take it gently and move it a little, then let go. Do not claim anything has been felt yet.)";
pub const GIVE_DONE: &str = "(You felt their hand move your head, gave way, and they let go. Tell them so in a few warm words. Nothing more.)";
pub const GIVE_FAILED: &str = "(You did not feel your head moved by a hand this time. Tell them in a few light words that it is fine and that they can redo the setup later. Do not claim anything was felt.)";

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
    /// Unix milliseconds, read when the whole invitation's playback was known.
    ended_ms: u64,
    /// The coordinator's clock then: the deadline and the clock-jump check only.
    ended_us: u64,
}

#[derive(Default)]
pub(super) struct GivingWayFlow {
    /// The coordinator has a body, the bus and the say service.
    available: bool,
    pub(super) generation: u64,
    offer: Option<Offer>,
    window: Option<Window>,
    retry_after_us: u64,
    /// The head's goals at the last reading with no hand on it.
    before: Option<[f64; 3]>,
    pushed: bool,
    /// In this push, a head joint moved by the hand and held there.
    moved: bool,
}

impl GivingWayFlow {
    fn forget_push(&mut self) {
        self.before = None;
        self.pushed = false;
        self.moved = false;
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
        Some(GIVE_ASK.to_owned())
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
            self.giving.offer = None;
            self.giving.retry_after_us = now_us.saturating_add(1_000_000);
        }
    }

    /// Only the whole invitation's playback opens a try. Its start is read
    /// from the Unix clock now, at that boundary.
    pub(super) fn giving_invitation_finished(
        &mut self,
        voice_boot: BootId,
        turn: u64,
        delivered_whole: bool,
        now_us: u64,
    ) {
        if self.giving_binding() != Some((voice_boot, turn)) {
            return;
        }
        self.giving.offer = None;
        if !delivered_whole {
            self.records.push(json!({"kind":"onboarding_giving_way_try",
                "felt":false,"why":"the invitation was not played whole","at_us":now_us}));
            flow_end(self, now_us);
            return;
        }
        self.giving.forget_push();
        self.giving.window = Some(Window {
            ended_ms: (self.unix_clock)(),
            ended_us: now_us,
        });
    }

    /// One reading from the body. Judged as it comes, on the loop, without
    /// waiting for storage: the outcome is kept by the storage worker after.
    pub(crate) fn giving_way_felt(&mut self, felt: Felt, now_us: u64) {
        let Some(window) = self.giving.window.as_ref() else {
            return;
        };
        let (ended_ms, ended_us) = (window.ended_ms, window.ended_us);
        if let Some(at_ms) = felt.at_ms()
            && (at_ms < ended_ms || at_ms > ended_ms.saturating_add(WINDOW_MS))
        {
            return;
        }
        let flow = &mut self.giving;
        match felt {
            Felt::Restart | Felt::Invalid => flow.forget_push(),
            Felt::Joints { head, .. } if !flow.pushed => {
                flow.before = Some(head.map(|(_, goal)| goal));
            }
            Felt::Joints { head, .. } => {
                if let Some(before) = flow.before
                    && head.iter().zip(before).any(|(&(position, goal), was)| {
                        (position - was).abs() > MOVED_RADIANS
                            && (goal - position).abs() <= MOVED_RADIANS
                    })
                {
                    flow.moved = true;
                }
            }
            Felt::Hand { pushed: true, .. } => flow.pushed = true,
            Felt::Hand { pushed: false, .. } => {
                let given_way = flow.pushed && flow.moved;
                flow.pushed = false;
                flow.moved = false;
                if !given_way {
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
                self.owed = Some(GIVE_DONE);
            }
        }
    }

    /// A try's minute ended without the evidence. Run every loop, whatever
    /// storage is doing, as the evidence is judged.
    pub(super) fn giving_judge(&mut self, now_us: u64) {
        if self
            .giving
            .window
            .as_ref()
            .is_some_and(|window| now_us.saturating_sub(window.ended_us) >= WINDOW_US)
        {
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
        if self.giving.window.is_some() {
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
        // Its budget is committed before the invitation is published.
        self.desired.giving_way_tries = attempt;
        self.save();
        if let Some(led) = self.led.as_mut() {
            led.asked_us = Some(now_us);
        }
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
    walk.finish(Step::GivingWay, Outcome::Failed, None, now_us);
    walk.owed = Some(GIVE_FAILED);
}
