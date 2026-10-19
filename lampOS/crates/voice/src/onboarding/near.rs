//! "How you sound" (#1803): the lamp learns how loud its person is when they
//! call it by name from their usual seat, for the busy-near rule
//! (`lamp_near_call`, Fourth's #753).
//!
//! Each prompt is one exact invitation token, as the privacy step's: the core
//! onboarding skill asks the say service for it, and only the whole prompt's
//! playback opens a try. The try's window is measured in Unix milliseconds, as
//! the bus's headers are: the prompt's end is read from the Unix clock at the
//! moment its playback is known to be whole, never derived from the
//! coordinator's monotonic clock. A hearing's start is its header's time less
//! its `ago_ms`.
//!
//! Evidence is only numbers already on the bus: `name/heard` (score),
//! `ears/level` (mic1's level over the room's quiet) and `voice/state` (when
//! the lamp spoke). Nothing of the sound is kept; no voice identity and no
//! mic2 ratio is used. The measurement itself is `lamp_near_call::near_try`.
//!
//! The step is done only when three accepted tries make a bar and the bar's
//! file is written; it fails, and any old bar is removed, when five prompts
//! cannot reach three or the tries spread too far. The model may only skip it.
//! Every file operation runs on the onboarding storage worker; the
//! interaction loop only buffers bounded evidence and submits.
use super::{
    FRESH_MS, GONE_US, Onboarding, Outcome, START_QUIET_US, STEP_GIVE_UP_US, Step,
    TIMED_OUT_QUIET_US, fresh_token, storage,
};
use lamp_near_call::{
    Calibrated, Hearing, Level, MOST_PROMPTS, RISE_MS, SCORE, Spoke, TRIES, Try, WINDOW_MS,
    asks_again, near_try,
};
use lamp_turn_owner::BootId;
use serde_json::json;
use std::time::Duration;

/// Evidence is gathered to the end of the rise window of a hearing that
/// began at the window's very end.
const COLLECT_MS: u64 = WINDOW_MS + RISE_MS;
/// The last level and a final `name/heard` are published a moment after the
/// span they describe; the try is judged this long after it.
const DELIVERY_GRACE_US: u64 = 300_000;
/// The Unix clock may drift from the monotonic one by this much over a try;
/// more is a jump, and the try's window can't be trusted.
const CLOCK_SLACK_MS: u64 = 1_000;
/// Speaking history kept from before the prompt's end: at least the
/// echo guard (`ECHO_BEFORE_MS`, 500 ms) before any hearing in the window.
const HISTORY_MS: u64 = 1_500;
/// A hearing said to have begun longer ago than this is not believed.
const MOST_AGO_MS: u64 = 10_000;
/// Evidence held at most: 8.5 s of levels at ten a second is 85.
const MOST_SEEN: usize = 256;
/// The bus is read at most this often.
const LOOK_EVERY_US: u64 = 20_000;

pub const NEAR_FIRST: &str = "(Nobody has spoken. Tell them in a few words that you would like to learn how they call you from where they usually sit. Ask them to stay in their seat and call your name once, the way they would to get your attention, and then stay quiet yourself. Do not say your own name, and do not claim anything was measured.)";
pub const NEAR_AGAIN: &str = "(Ask them in a few words to call your name once more from their seat, the same way, and then stay quiet yourself. Do not say your own name.)";
pub const NEAR_DONE: &str = "(You have learned how they call you from their seat. Tell them so in a few words. Nothing more.)";
pub const NEAR_FAILED: &str = "(You could not learn how they call you from their seat this time. Tell them in a few light words that it is fine and that they can redo the setup later. Do not claim anything was measured.)";

/// One piece of evidence from the bus, already validated and dated in Unix
/// milliseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Seen {
    Hearing(Hearing),
    Level(Level),
    /// `voice/state` at this time: speaking or not.
    Speaking {
        at_ms: u64,
        speaking: bool,
    },
    /// A source began again, or an event was lost: a try open now is spoiled.
    Restart,
    /// A message that does not parse or is out of range: a try open now is
    /// spoiled rather than judged on part of its evidence.
    Invalid,
}

impl Seen {
    /// A final `name/heard` sent at `at_unix_ms`. Non-finite scores, scores
    /// outside 0..=1 and impossible ages are refused here; a valid score under
    /// [`SCORE`] stays a hearing so that `near_try` says it was unsure.
    pub(crate) fn hearing(at_unix_ms: u64, heard: &lamp_msgs::NameHeard) -> Self {
        let ago = u64::from(heard.ago_ms);
        if heard.check().is_err()
            || !heard.score.is_finite()
            || ago > MOST_AGO_MS
            || ago > at_unix_ms
        {
            return Self::Invalid;
        }
        Self::Hearing(Hearing {
            began_ms: at_unix_ms - ago,
            score: heard.score,
        })
    }

    /// An `ears/level` reading sent at `at_unix_ms`.
    pub(crate) fn level(at_unix_ms: u64, sound: &lamp_msgs::RoomSound) -> Self {
        if sound.check().is_err() {
            return Self::Invalid;
        }
        Self::Level(Level {
            at_ms: at_unix_ms,
            level_db: sound.level_db,
            floor_db: sound.floor_db,
            own_voice: sound.own_voice,
        })
    }

    fn at_ms(&self) -> Option<u64> {
        match self {
            Self::Hearing(hearing) => Some(hearing.began_ms),
            Self::Level(level) => Some(level.at_ms),
            Self::Speaking { at_ms, .. } => Some(*at_ms),
            Self::Restart | Self::Invalid => None,
        }
    }
}

struct Offer {
    token: u64,
    prompt: u8,
    ready: bool,
    claimed: bool,
    persisted: bool,
    offered_us: u64,
    bound: Option<(BootId, u64)>,
}

struct Listening {
    prompt: u8,
    /// Unix milliseconds, read when the whole prompt's playback was known.
    ended_ms: u64,
    /// The coordinator's clock at the same moment: only for the deadline and
    /// the clock-jump check, never as a Unix time.
    ended_us: u64,
}

#[derive(Default)]
pub(super) struct NearFlow {
    /// The coordinator can deliver an invitation and read the bus.
    available: bool,
    pub(super) generation: u64,
    offer: Option<Offer>,
    listening: Option<Listening>,
    retry_after_us: u64,
    /// Decided: the bar to keep, or none. Waits for idle storage.
    finishing: Option<Option<Calibrated>>,
    flight: Option<(storage::Tag, Outcome)>,
    seen: Vec<Seen>,
    spoiled: Option<&'static str>,
}

impl NearFlow {
    #[cfg(test)]
    pub(super) fn seen(&self) -> &[Seen] {
        &self.seen
    }

    #[cfg(test)]
    pub(super) fn writing(&self) -> bool {
        self.flight.is_some()
    }

    fn take_evidence(&mut self) -> (Vec<Hearing>, Vec<Level>, Vec<Spoke>, Option<&'static str>) {
        let mut hearings = Vec::new();
        let mut levels = Vec::new();
        let mut spoke: Vec<Spoke> = Vec::new();
        let mut open: Option<u64> = None;
        for seen in self.seen.drain(..) {
            match seen {
                Seen::Hearing(hearing) => hearings.push(hearing),
                Seen::Level(level) => levels.push(level),
                Seen::Speaking { at_ms, speaking } => match (speaking, open) {
                    (true, None) => open = Some(at_ms),
                    (false, Some(began_ms)) => {
                        spoke.push(Spoke {
                            began_ms,
                            ended_ms: Some(at_ms),
                        });
                        open = None;
                    }
                    _ => {}
                },
                Seen::Restart | Seen::Invalid => {}
            }
        }
        if let Some(began_ms) = open {
            spoke.push(Spoke {
                began_ms,
                ended_ms: None,
            });
        }
        (hearings, levels, spoke, self.spoiled.take())
    }
}

/// The bus topics the step reads, each drained without waiting.
pub(crate) struct Ears {
    names: lamp_bus::Subscriber,
    levels: lamp_bus::Subscriber,
    states: lamp_bus::Subscriber,
    looked_at_us: Option<u64>,
}

impl Ears {
    /// Only with the bus switched on; a bus that cannot be used leaves the
    /// step unavailable.
    pub(crate) fn from_environment() -> Option<Self> {
        if std::env::var(crate::bus_out::ENVIRONMENT).ok().as_deref() != Some("on") {
            return None;
        }
        let bus = lamp_bus::Bus::from_environment().ok()?;
        let subscribe = |topic, kind| bus.subscribe_typed(topic, "onboarding", kind);
        Some(Self {
            names: subscribe(lamp_msgs::NameHeard::TOPIC, lamp_msgs::NameHeard::TYPE).ok()?,
            levels: subscribe(lamp_msgs::RoomSound::TOPIC, lamp_msgs::RoomSound::TYPE).ok()?,
            states: subscribe(lamp_msgs::VoiceState::TOPIC, lamp_msgs::VoiceState::TYPE).ok()?,
            looked_at_us: None,
        })
    }

    /// What arrived since the last look, at most every [`LOOK_EVERY_US`].
    pub(crate) fn take(&mut self, now_us: u64) -> Vec<Seen> {
        if self
            .looked_at_us
            .is_some_and(|at| now_us.saturating_sub(at) < LOOK_EVERY_US)
        {
            return Vec::new();
        }
        self.looked_at_us = Some(now_us);
        let mut seen = Vec::new();
        let mut drain = |subscriber: &mut lamp_bus::Subscriber,
                         convert: &dyn Fn(&lamp_bus::Message) -> Seen,
                         events: bool| {
            for _ in 0..32 {
                let Ok(Some(message)) = subscriber.recv(Duration::ZERO) else {
                    break;
                };
                if message.publisher_restarted || (events && message.gap > 0) {
                    seen.push(Seen::Restart);
                }
                seen.push(convert(&message));
            }
        };
        drain(
            &mut self.names,
            &|message| {
                message
                    .parse::<lamp_msgs::NameHeard>()
                    .map_or(Seen::Invalid, |heard| {
                        Seen::hearing(message.header.at_unix_ms, &heard)
                    })
            },
            true,
        );
        // ponytail: a lost level is not a restart; only its peak may be
        // missed. Count gaps if missed peaks show up on a lamp.
        drain(
            &mut self.levels,
            &|message| {
                message
                    .parse::<lamp_msgs::RoomSound>()
                    .map_or(Seen::Invalid, |sound| {
                        Seen::level(message.header.at_unix_ms, &sound)
                    })
            },
            false,
        );
        drain(
            &mut self.states,
            &|message| {
                message
                    .parse::<lamp_msgs::VoiceState>()
                    .map_or(Seen::Invalid, |state| Seen::Speaking {
                        at_ms: message.header.at_unix_ms,
                        speaking: state.state == lamp_msgs::Voice::Speaking,
                    })
            },
            false,
        );
        seen
    }
}

impl Onboarding {
    /// The coordinator says whether it can deliver this step's invitation
    /// (say service and bus) and read its evidence.
    pub(crate) fn set_near_available(&mut self, available: bool) {
        if self.near.available && !available {
            self.invalidate_near();
        }
        self.near.available = available;
    }

    pub(super) fn near_available(&self) -> bool {
        self.near.available
    }

    fn collecting(&self) -> bool {
        self.led
            .as_ref()
            .is_some_and(|led| led.step == Step::HowYouSound)
    }

    /// Bounded evidence, kept only while this step is led. Restart and
    /// invalid evidence spoil a try being listened for.
    pub(crate) fn near_seen(&mut self, seen: Seen) {
        if !self.collecting() {
            return;
        }
        match seen.at_ms() {
            None => {
                if self.near.listening.is_some() {
                    self.near.spoiled = Some(if seen == Seen::Restart {
                        "a source began again"
                    } else {
                        "invalid evidence"
                    });
                }
                return;
            }
            Some(at) => {
                if let Some(listening) = self.near.listening.as_ref()
                    && at > listening.ended_ms.saturating_add(COLLECT_MS)
                {
                    return;
                }
            }
        }
        self.near.seen.push(seen);
        let newest = self.near.seen.iter().filter_map(Seen::at_ms).max();
        let from = match self.near.listening.as_ref() {
            Some(listening) => listening.ended_ms.saturating_sub(HISTORY_MS),
            None => newest.unwrap_or(0).saturating_sub(HISTORY_MS),
        };
        // The last speaking mark before `from` says whether the lamp was
        // speaking then, so it stays too.
        let state_at_from = self
            .near
            .seen
            .iter()
            .rposition(|seen| matches!(seen, Seen::Speaking { at_ms, .. } if *at_ms < from));
        let mut index = 0;
        self.near.seen.retain(|seen| {
            let keep = Some(index) == state_at_from || seen.at_ms().is_some_and(|at| at >= from);
            index += 1;
            keep
        });
        if self.near.seen.len() > MOST_SEEN {
            let extra = self.near.seen.len() - MOST_SEEN;
            self.near.seen.drain(..extra);
        }
    }

    /// Drop everything in flight for this step. An offer never handed to the
    /// model gives its prompt back; a claimed one counts.
    pub(super) fn invalidate_near(&mut self) {
        if let Some(offer) = self.near.offer.as_ref()
            && !offer.claimed
            && self.desired.near_call_prompts == offer.prompt
        {
            self.desired.near_call_prompts = offer.prompt - 1;
            self.save();
        }
        self.forget_near();
    }

    /// Memory only: what a storage fence or failure leaves of this step. The
    /// kept prompts and tries come back from storage.
    pub(super) fn forget_near(&mut self) {
        self.near.offer = None;
        self.near.listening = None;
        self.near.finishing = None;
        self.near.flight = None;
        self.near.seen.clear();
        self.near.spoiled = None;
    }

    pub(super) fn near_pending(&self) -> Option<lamp_msgs::OnboardingInvitation> {
        let offer = self.near.offer.as_ref()?;
        Some(lamp_msgs::OnboardingInvitation {
            invitation: offer.token,
            generation: self.near.generation,
            attempt: u64::from(offer.prompt),
            step: lamp_msgs::OnboardingStep::HowYouSound,
            ready: offer.ready && offer.persisted && !offer.claimed && self.storage_idle(),
        })
    }

    pub(super) fn take_near_invitation(&mut self, token: u64, now_us: u64) -> Option<String> {
        if !self.storage_idle()
            || self
                .seen_us
                .is_none_or(|seen| now_us.saturating_sub(seen) > FRESH_MS * 1_000)
        {
            return None;
        }
        let offer = self.near.offer.as_mut()?;
        if offer.token != token || !offer.ready || !offer.persisted || offer.claimed {
            return None;
        }
        offer.claimed = true;
        offer.ready = false;
        Some(
            if offer.prompt == 1 {
                NEAR_FIRST
            } else {
                NEAR_AGAIN
            }
            .to_owned(),
        )
    }

    pub(super) fn bind_near_invitation(
        &mut self,
        token: u64,
        voice_boot: BootId,
        turn: u64,
    ) -> bool {
        let Some(offer) = self.near.offer.as_mut() else {
            return false;
        };
        if offer.token != token || !offer.claimed || offer.bound.is_some() || turn == 0 {
            return false;
        }
        offer.bound = Some((voice_boot, turn));
        true
    }

    pub(super) fn near_binding(&self) -> Option<(BootId, u64)> {
        self.near.offer.as_ref().and_then(|offer| offer.bound)
    }

    pub(super) fn near_invitation_refused(&mut self, token: u64, now_us: u64) {
        if self
            .near
            .offer
            .as_ref()
            .is_some_and(|offer| offer.token == token && offer.bound.is_none())
        {
            // The prompt was committed to the budget: it counts.
            self.near.offer = None;
            self.near.retry_after_us = now_us.saturating_add(1_000_000);
        }
    }

    /// The whole prompt's playback is the only start of a try. Its end is
    /// read from the Unix clock now, at that boundary.
    pub(super) fn near_invitation_finished(
        &mut self,
        voice_boot: BootId,
        turn: u64,
        delivered_whole: bool,
        now_us: u64,
    ) {
        if self.near_binding() != Some((voice_boot, turn)) {
            return;
        }
        let offer = self.near.offer.take().expect("a bound offer");
        if !delivered_whole {
            self.near.retry_after_us = now_us.saturating_add(1_000_000);
            self.records
                .push(json!({"kind":"onboarding_near_try","prompt":offer.prompt,
                "accepted":false,"why":"the prompt was not played whole","at_us":now_us}));
            return;
        }
        // History from before the prompt's end is kept (the echo guard); a
        // spoil from before this try is not.
        self.near.spoiled = None;
        self.near.listening = Some(Listening {
            prompt: offer.prompt,
            ended_ms: (self.unix_clock)(),
            ended_us: now_us,
        });
    }

    /// Judge the try being listened for, once its evidence is all in. Pure
    /// computation on bounded buffers; never waits for storage.
    pub(super) fn near_judge(&mut self, now_us: u64) {
        let due = self.near.listening.as_ref().is_some_and(|listening| {
            now_us >= listening.ended_us + COLLECT_MS * 1_000 + DELIVERY_GRACE_US
        });
        if !due {
            return;
        }
        let listening = self.near.listening.take().expect("due");
        let (hearings, levels, mut spoke, spoiled) = self.near.take_evidence();
        // The prompt itself: the lamp spoke until its end.
        spoke.push(Spoke {
            began_ms: listening.ended_ms.saturating_sub(1),
            ended_ms: Some(listening.ended_ms),
        });
        let expected_ms = listening.ended_ms + now_us.saturating_sub(listening.ended_us) / 1_000;
        let judged = if (self.unix_clock)().abs_diff(expected_ms) > CLOCK_SLACK_MS {
            Err("the clock jumped".to_owned())
        } else if let Some(why) = spoiled {
            Err(why.to_owned())
        } else {
            near_try(listening.ended_ms, &hearings, &levels, &spoke).map_err(|why| why.to_string())
        };
        match judged {
            Ok(done) => {
                self.desired.near_call_tries.push(done);
                self.save();
                self.records.push(
                    json!({"kind":"onboarding_near_try","prompt":listening.prompt,
                    "accepted":true,"rise_db":done.rise_db,"score":done.score,"at_us":now_us}),
                );
            }
            Err(why) => self.records.push(json!({"kind":"onboarding_near_try",
                "prompt":listening.prompt,"accepted":false,"why":why,"at_us":now_us})),
        }
    }

    /// The bar's write or removal completed on the storage worker in this
    /// epoch: only now is the step's outcome recorded.
    pub(super) fn near_stored(&mut self, epoch: u64, revision: u64, now_us: u64) {
        let Some((tag, outcome)) = self.near.flight else {
            return;
        };
        if tag.epoch != epoch || tag.revision > revision {
            return;
        }
        self.near.flight = None;
        self.near.finishing = None;
        self.finish(Step::HowYouSound, outcome, None, now_us);
        self.owed = Some(if outcome == Outcome::Done {
            NEAR_DONE
        } else {
            NEAR_FAILED
        });
    }

    /// The step being led, with storage idle.
    pub(super) fn near_tick(&mut self, now_us: u64, quiet_us: u64, can_speak: bool, present: bool) {
        if self
            .seen_us
            .is_some_and(|seen| now_us.saturating_sub(seen) >= GONE_US)
        {
            if self.near.offer.is_some() || self.near.listening.is_some() {
                self.records
                    .push(json!({"kind":"onboarding_stopped","why":"nobody_there",
                    "step":Step::HowYouSound.name(),"at_us":now_us}));
            }
            self.invalidate_near();
            self.led = None;
            return;
        }
        if self.near.flight.is_some() || self.near.listening.is_some() {
            return;
        }
        let prompts = self.desired.near_call_prompts;
        if self.near.offer.is_none()
            && self.near.finishing.is_none()
            && !asks_again(usize::from(prompts), self.desired.near_call_tries.len())
        {
            let at_unix_s = (self.unix_clock)() / 1_000;
            self.near.finishing = Some(Calibrated::of(
                self.desired.near_call_tries.clone(),
                at_unix_s,
            ));
        }
        if let Some(calibrated) = self.near.finishing.clone() {
            let outcome = if calibrated.is_some() {
                Outcome::Done
            } else {
                Outcome::Failed
            };
            match self.storage.near_call(calibrated) {
                Ok(storage::Submit::Queued(tag)) => {
                    self.near.flight = Some((tag, outcome));
                    self.storage_busy = true;
                }
                Ok(storage::Submit::Busy) => self.storage_busy = true,
                Ok(storage::Submit::Closed) => self.storage_failed("storage is closed"),
                Err(error) => self.storage_failed(error),
            }
            return;
        }
        if self.near.generation == 0 {
            let Ok(generation) = fresh_token() else {
                return;
            };
            self.near.generation = generation;
        }
        let ready = can_speak && present && quiet_us >= START_QUIET_US;
        if let Some(offer) = self.near.offer.as_mut() {
            if !offer.persisted {
                if self.committed.near_call_prompts >= offer.prompt {
                    offer.persisted = true;
                } else {
                    self.desired.near_call_prompts = offer.prompt;
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
                self.near_invitation_refused(token, now_us);
            }
            return;
        }
        if !ready || now_us < self.near.retry_after_us {
            return;
        }
        let Ok(token) = fresh_token() else {
            return;
        };
        let prompt = prompts.saturating_add(1);
        debug_assert!(usize::from(prompt) <= MOST_PROMPTS);
        self.near.offer = Some(Offer {
            token,
            prompt,
            ready: true,
            claimed: false,
            persisted: false,
            offered_us: now_us,
            bound: None,
        });
        // Its budget is committed before the invitation is published.
        self.desired.near_call_prompts = prompt;
        self.save();
        if let Some(led) = self.led.as_mut() {
            led.asked_us = Some(now_us);
        }
        self.records.push(
            json!({"kind":"onboarding_step","step":Step::HowYouSound.name(),
            "state":"offered","prompt":prompt,"at_us":now_us}),
        );
    }
}

/// Persisted tries are numbers someone could have edited: each must be one
/// `near_try` could have accepted.
pub(super) fn valid_try(done: &Try) -> bool {
    done.rise_db.is_finite()
        && (0.0..=-lamp_msgs::EARS_SILENCE_DB).contains(&done.rise_db)
        && done.score.is_finite()
        && (SCORE..=1.0).contains(&done.score)
}

/// Whether a kept "done" is backed by a bar its tries make.
pub(super) fn done_fits(tries: &[Try]) -> bool {
    tries.len() == TRIES && lamp_near_call::near_rise(tries).is_some()
}
