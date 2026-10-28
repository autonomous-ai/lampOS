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
//! The name part ends when three accepted tries make a bar and the bar's
//! file is written, or, with any old bar removed, when five prompts cannot
//! reach three or the tries spread too far. Then the sentence part (the
//! first builder's ruling (a), 20 October): the person says a sentence from
//! where they sit, in their own words. It is done only on the voice's own
//! evidence: a line ended in the window ([`SENTENCE_WINDOW_US`]) with words
//! come back for it, said close (the room microphone rose over its quiet
//! during the line as far as this person's own calls from the seat did, less
//! 3 dB, or 6 dB with no call accepted, never over the near rule's bar nor
//! under 3 dB: `sentence_bar`), and
//! its words covering it (at
//! least one for each second of the line's speech, [`MS_A_WORD`]). Short of
//! that it is coached once, "a little louder" for the level, "a little
//! closer" for the words, then done or failed. Only counts are kept. The
//! step's outcome is the sentence part's; the name part's is its bar file.
//! The model may only skip it. Every file operation runs on the onboarding
//! storage worker; the interaction loop only buffers bounded evidence and
//! submits.
#[cfg(test)]
pub use super::legacy_words::{
    NEAR_DONE, NEAR_FAILED, SENTENCE_AGAIN, SENTENCE_CLOSER, SENTENCE_DONE, SENTENCE_FAILED,
    SENTENCE_FIRST, SENTENCE_LOUDER,
};

use super::{
    Delivery, FRESH_MS, GONE_US, MOST_UNSAID, NOT_SAID, Onboarding, Outcome, START_QUIET_US,
    STEP_GIVE_UP_US, Step, TIMED_OUT_QUIET_US, fresh_token, storage,
};
use lamp_near_call::{
    Calibrated, Hearing, Level, MOST_PROMPTS, RISE_MS, SCORE, Spoke, Try, WINDOW_MS, asks_again,
    near_try,
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
pub(super) const CLOCK_SLACK_MS: u64 = 1_000;
/// Speaking history kept from before the prompt's end: at least the
/// echo guard (`ECHO_BEFORE_MS`, 500 ms) before any hearing in the window.
const HISTORY_MS: u64 = 1_500;
/// A hearing said to have begun longer ago than this is not believed.
const MOST_AGO_MS: u64 = 10_000;
/// Evidence held at most: 8.5 s of levels at ten a second is 85.
const MOST_SEEN: usize = 256;
/// The bus is read at most this often.
const LOOK_EVERY_US: u64 = 20_000;
/// A name begun this long before the prompt's end, over its last word, whose
/// final hearing came after it, is the person answering early: people start
/// answering as the lamp finishes (lamp-52e6, row 10: names begun 237 to
/// 478 ms before the end). Its reply is held as a call's, but it is not a
/// try: its sound sits under the lamp's own voice, and the ears flag
/// `own_voice` for 0.5 s past the end (`lamp-ears`, `own_tail_ms`), so its
/// rise can't be read; nor can it be told from the lamp's own last word
/// heard as the name. The prompt is offered again under [`MOST_REOFFERS`].
/// The detector's own allowance, `crate::activity::CALL_BEFORE_US`.
const OVER_PROMPT_MS: u64 = crate::activity::CALL_BEFORE_US / 1_000;
/// A prompt cut before it played whole (someone spoke over it), or answered
/// over its last word ([`OVER_PROMPT_MS`]), is offered again with its own
/// number, this many times in a step at most.
const MOST_REOFFERS: u8 = 2;
/// The sentence's level bar is never under this rise over the room's quiet
/// (a doubling of its power): in a loud office, where the person's own
/// calls rose only 3.9 dB (lamp-52e6, row 10), a line under it is not told
/// from the room's own swing.
const SENTENCE_LOWEST_DB: f32 = 3.0;
/// The sentence's level bar when no call from the seat was accepted, in place
/// of the near rule's 18 dB, which is a call's bar: a sentence said from the
/// seat rose 9.2 dB on lamp-52e6 (row 10, 20 October) and 10.7 to 11.0 dB on
/// lamp-c0a4 (#2844), so at 18 dB a person whose calls were all refused
/// failed the whole step on their sentence (lamp-52e6, 27 October, #3504).
/// It is 52e6's seat sentence less `UNDER_QUIETEST_DB`, as a bar is made from
/// calls; the same voice from twice as far rises about 4.5 dB, under it.
const SENTENCE_NO_CALL_DB: f32 = 6.0;
/// The sentence part's prompts: the first, and one coached.
pub(crate) const SENTENCE_PROMPTS: u8 = 2;
/// A line said for the sentence ends within this long of the prompt's end:
/// a moment to think of something to say, and a sentence.
const SENTENCE_WINDOW_US: u64 = 15_000_000;
/// The words of a line ended in the window are waited for this long past
/// it: on the second route they came within a second of the line's end.
const WORDS_WAIT_US: u64 = 3_000_000;
/// Words covering a line: at least one for each this many milliseconds of
/// its speech, half the pace of ordinary speech (two to three words a
/// second). A line whose words came back fewer is a fragment.
const MS_A_WORD: u64 = 1_000;

/// What fell short in a sentence try: the coaching it gets.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Short {
    /// Not said close: "a little louder".
    Level,
    /// No words, or too few to cover it: "a little closer".
    Coverage,
}

impl Short {
    fn name(self) -> &'static str {
        match self {
            Self::Level => "level",
            Self::Coverage => "coverage",
        }
    }
}

/// The last line ended in the sentence window: numbers only.
#[derive(Clone, Copy)]
struct Line {
    speech_ms: u64,
    /// The room microphone's largest rise over its quiet during it, in dB.
    rise_db: Option<f32>,
    words: Option<usize>,
}

/// One piece of evidence from the bus, already validated and dated in Unix
/// milliseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Seen {
    /// A final hearing, and when it was sent: its name had ended by then.
    Hearing(Hearing, u64),
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
        Self::Hearing(
            Hearing {
                began_ms: at_unix_ms - ago,
                score: heard.score,
            },
            at_unix_ms,
        )
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
            Self::Hearing(hearing, _) => Some(hearing.began_ms),
            Self::Level(level) => Some(level.at_ms),
            Self::Speaking { at_ms, .. } => Some(*at_ms),
            Self::Restart | Self::Invalid => None,
        }
    }
}

struct Offer {
    token: u64,
    /// The sentence part's, else the name part's.
    sentence: bool,
    prompt: u8,
    ready: bool,
    claimed: bool,
    persisted: bool,
    offered_us: u64,
    bound: Option<(BootId, u64)>,
}

struct Listening {
    sentence: bool,
    prompt: u8,
    /// Unix milliseconds, read when the whole prompt's playback was known.
    ended_ms: u64,
    /// The coordinator's clock at the same moment: only for the deadline and
    /// the clock-jump check, never as a Unix time.
    ended_us: u64,
}

#[derive(Default)]
pub(crate) struct NearFlow {
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
    /// The name part's outcome once its bar is stored: the sentence part
    /// follows. Memory only: a new voice cycle stores the bar again.
    named: Option<Outcome>,
    line: Option<Line>,
    short: Option<Short>,
    /// Prompts offered again after being cut ([`MOST_REOFFERS`]).
    reoffered: u8,
    /// Prompts the voice never said, offered again ([`MOST_UNSAID`]).
    unsaid: u8,
    /// Whether the last call try judged in this voice cycle was accepted:
    /// the next call prompt says "Got it. Once more." (`near_again`) or "I
    /// didn't catch that." (`near_missed`). None until one is judged, and
    /// again after an absence, a pause or a mute: the step's first line,
    /// which says the phrase and that it asks three times (#3702).
    last: Option<bool>,
}

impl NearFlow {
    /// Whether the name part waits for a call (#3504): from its prompt
    /// handed to the model, through its playing, to its try's judgement. A
    /// line begun in it that is only the lamp's name is the step's, not a
    /// turn (`ProviderInput::StepCall`).
    pub(crate) fn waits_for_call(&self) -> bool {
        self.offer
            .as_ref()
            .is_some_and(|offer| !offer.sentence && offer.claimed)
            || self
                .listening
                .as_ref()
                .is_some_and(|listening| !listening.sentence)
    }

    /// [`Onboarding::near_holds_reply`]'s rule, on this step's own evidence.
    pub(crate) fn holds_reply(&self, began_us: u64, at_us: u64) -> bool {
        let Some(listening) = self.listening.as_ref() else {
            return false;
        };
        // The sentence part holds nothing here: its line is held by its own
        // window, which lets the words come back.
        !listening.sentence
            && at_us >= listening.ended_us
            && at_us - listening.ended_us <= COLLECT_MS * 1_000
            && self.seen.iter().any(|seen| match seen {
                // The call's Unix start on the coordinator's clock, by the
                // prompt's end read on both.
                Seen::Hearing(hearing, heard_ms) => {
                    call_began_ms(hearing, *heard_ms, listening.ended_ms).is_some_and(|began| {
                        let heard_us =
                            listening.ended_us + (began - listening.ended_ms).saturating_mul(1_000);
                        heard_us.saturating_add(crate::activity::CALL_BEFORE_US) >= began_us
                    })
                }
                _ => false,
            })
    }

    /// A loop test's try window, open since `ended_us` (`ended_ms` on the
    /// Unix clock), with the name heard beginning at these Unix times.
    #[cfg(test)]
    pub(crate) fn listening_for_test(ended_ms: u64, ended_us: u64, names_ms: &[u64]) -> Self {
        Self {
            listening: Some(Listening {
                sentence: false,
                prompt: 1,
                ended_ms,
                ended_us,
            }),
            seen: names_ms
                .iter()
                .map(|&began_ms| {
                    Seen::Hearing(
                        Hearing {
                            began_ms,
                            score: 0.98,
                        },
                        began_ms + 600,
                    )
                })
                .collect(),
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(super) fn seen(&self) -> &[Seen] {
        &self.seen
    }

    #[cfg(test)]
    pub(super) fn writing(&self) -> bool {
        self.flight.is_some()
    }

    #[cfg(test)]
    pub(super) fn named(&self) -> Option<Outcome> {
        self.named
    }

    fn take_evidence(&mut self) -> (Vec<Hearing>, Vec<Level>, Vec<Spoke>, Option<&'static str>) {
        let mut hearings = Vec::new();
        let mut levels = Vec::new();
        let mut spoke: Vec<Spoke> = Vec::new();
        let mut open: Option<u64> = None;
        for seen in self.seen.drain(..) {
            match seen {
                Seen::Hearing(hearing, _) => hearings.push(hearing),
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
        if let Some((sentence, prompt)) = self
            .near
            .offer
            .as_ref()
            .filter(|offer| !offer.claimed)
            .map(|offer| (offer.sentence, offer.prompt))
            && *self.prompts(sentence) == prompt
        {
            *self.prompts(sentence) = prompt - 1;
            self.save();
        }
        // The name part's stored bar stands: the sentence part goes on. The
        // prompts offered again stay counted, so absence can't renew them.
        // The last try is forgotten: after an absence, a pause or a mute the
        // next prompt names the phrase again, never "Got it" (#3702).
        let (named, reoffered, unsaid) = (self.near.named, self.near.reoffered, self.near.unsaid);
        self.forget_near();
        self.near.named = named;
        self.near.reoffered = reoffered;
        self.near.unsaid = unsaid;
    }

    /// The committed-to-be prompt count of a part.
    fn prompts(&mut self, sentence: bool) -> &mut u8 {
        if sentence {
            &mut self.desired.sentence_prompts
        } else {
            &mut self.desired.near_call_prompts
        }
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
        self.near.named = None;
        self.near.line = None;
        self.near.short = None;
        self.near.reoffered = 0;
        self.near.unsaid = 0;
        self.near.last = None;
    }

    pub(super) fn near_pending(&self) -> Option<lamp_msgs::OnboardingInvitation> {
        let offer = self.near.offer.as_ref()?;
        Some(lamp_msgs::OnboardingInvitation {
            invitation: offer.token,
            generation: self.near.generation,
            // The sentence's prompts follow the name's five.
            attempt: u64::from(offer.prompt)
                + if offer.sentence {
                    MOST_PROMPTS as u64
                } else {
                    0
                },
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
            match (offer.sentence, offer.prompt, self.near.short) {
                // Each call prompt answers the last try (#3702).
                (false, _, _) => match self.near.last {
                    Some(true) => self.definition.words.near_again.as_str(),
                    // An older file without `near_missed` says the first line,
                    // never "Got it" for a miss.
                    Some(false) if self.definition.words.near_missed.is_some() => {
                        self.definition.words.near_missed.as_deref().unwrap()
                    }
                    _ => self.definition.note(
                        Step::HowYouSound,
                        self.desired.steps.contains_key(Step::HowYouSound.name()),
                    ),
                },
                (true, 1, _) => self.definition.words.sentence_first.as_str(),
                (true, _, Some(Short::Level)) => self.definition.words.sentence_louder.as_str(),
                (true, _, Some(Short::Coverage)) => self.definition.words.sentence_closer.as_str(),
                // A new voice cycle: what fell short is not known.
                (true, _, None) => self.definition.words.sentence_again.as_str(),
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

    /// A prompt that made no try is offered again with its own number and
    /// spends none of the budget, [`MOST_REOFFERS`] times in a step at most.
    fn offer_again(&mut self, sentence: bool, prompt: u8) -> bool {
        let again = self.near.reoffered < MOST_REOFFERS && *self.prompts(sentence) == prompt;
        if again {
            self.near.reoffered += 1;
            *self.prompts(sentence) = prompt - 1;
            self.save();
        }
        again
    }

    /// The whole prompt's playback is the only start of a try. Its end is
    /// read from the Unix clock now, at that boundary.
    pub(super) fn near_invitation_finished(
        &mut self,
        voice_boot: BootId,
        turn: u64,
        delivery: Delivery,
        now_us: u64,
    ) {
        if self.near_binding() != Some((voice_boot, turn)) {
            return;
        }
        let offer = self.near.offer.take().expect("a bound offer");
        // The cue a test driven from outside times its answer by (#3504).
        eprintln!(
            "lamp-voice onboarding: how_you_sound prompt {} played {} ({})",
            offer.prompt,
            match delivery {
                Delivery::Whole => "whole",
                Delivery::Cut => "cut",
                Delivery::Gapped => "with a gap",
                Delivery::Unsaid => "not said",
            },
            if offer.sentence { "sentence" } else { "call" }
        );
        if delivery == Delivery::Unsaid || delivery == Delivery::Gapped {
            // Never said, never asked: its prompt is given back, and none of
            // the cut prompts' budget is spent (#3754). So is one said
            // through with a gap and nobody's turn taken over it (#4035).
            if *self.prompts(offer.sentence) == offer.prompt {
                *self.prompts(offer.sentence) = offer.prompt - 1;
                self.save();
            }
            self.near.unsaid = self.near.unsaid.saturating_add(1);
            let again = self.near.unsaid <= MOST_UNSAID;
            let kind = if offer.sentence {
                "onboarding_sentence_try"
            } else {
                "onboarding_near_try"
            };
            let why = if delivery == Delivery::Unsaid {
                NOT_SAID
            } else {
                "the prompt was played with a gap"
            };
            self.records.push(json!({"kind":kind,"prompt":offer.prompt,
                "accepted":false,"why":why,"offered_again":again,"at_us":now_us}));
            if again {
                self.near.retry_after_us = now_us.saturating_add(1_000_000);
            } else {
                self.invalidate_near();
                self.finish(Step::HowYouSound, Outcome::NotAvailable, None, now_us);
            }
            return;
        }
        if delivery != Delivery::Whole {
            self.near.retry_after_us = now_us.saturating_add(1_000_000);
            let kind = if offer.sentence {
                "onboarding_sentence_try"
            } else {
                "onboarding_near_try"
            };
            // Someone spoke over it (an office's voices, or the person
            // answering early).
            let again = self.offer_again(offer.sentence, offer.prompt);
            self.note_try(json!({"kind":kind,"prompt":offer.prompt,
                "accepted":false,"why":"the prompt was not played whole",
                "offered_again":again,"at_us":now_us}));
            return;
        }
        // History from before the prompt's end is kept (the echo guard); a
        // spoil from before this try is not.
        self.near.spoiled = None;
        self.near.line = None;
        self.near.listening = Some(Listening {
            sentence: offer.sentence,
            prompt: offer.prompt,
            ended_ms: (self.unix_clock)(),
            ended_us: now_us,
        });
    }

    /// Whether the voice holds its reply to an utterance whose speech began
    /// at `began_us`, at `at_us` (#1803): a try's window is open, from the
    /// prompt's end to the end of its collection (7 s), and the lamp's name
    /// was heard in the window as the utterance began, since an answer within
    /// a second of the name spoils the try. Something said without the name
    /// ("skip that") is not held; the coordinator lets one with more said
    /// than a call through too.
    pub(crate) fn near_holds_reply(&self, began_us: u64, at_us: u64) -> bool {
        self.near.holds_reply(began_us, at_us)
    }

    /// [`NearFlow::waits_for_call`], while this step is led.
    pub(crate) fn near_waits_for_call(&self) -> bool {
        self.collecting() && self.near.waits_for_call()
    }

    /// Whether the sentence part's window is open at `at_us`, from the
    /// prompt's end to [`SENTENCE_WINDOW_US`] on: a line that ends in it is
    /// the sentence. The voice lets it reach the words, and holds the
    /// model's answer to it (`coordinator.rs`, `sentence_window`).
    pub(crate) fn sentence_window(&self, at_us: u64) -> bool {
        self.near.listening.as_ref().is_some_and(|listening| {
            listening.sentence
                && at_us >= listening.ended_us
                && at_us - listening.ended_us <= SENTENCE_WINDOW_US
        })
    }

    /// A line ended at `at_us`: its speech in milliseconds, and the room
    /// microphone's largest rise over its quiet during it (none without the
    /// bus). Taken only in the sentence window; a later line replaces it.
    pub(crate) fn sentence_line(&mut self, speech_ms: u64, rise_db: Option<f32>, at_us: u64) {
        if self.sentence_window(at_us) {
            self.near.line = Some(Line {
                speech_ms,
                rise_db,
                words: None,
            });
        }
    }

    /// The rise a sentence said close reaches: this person's quietest
    /// accepted call from the seat less `UNDER_QUIETEST_DB`, as their bar is
    /// made, so it is relative to this room's quiet at this hour; with no
    /// call accepted, [`SENTENCE_NO_CALL_DB`]; never over the near rule's bar
    /// (`crate::attention::near_rise_db`, 18 dB unless calibrated), and never
    /// under [`SENTENCE_LOWEST_DB`]. People farther off rose 9.8 to 15.1 dB
    /// in a quiet room, under a quiet room's bar of 18 dB and under the
    /// calls' own less 3 dB.
    fn sentence_bar(&self) -> f32 {
        let bar = crate::attention::near_rise_db();
        self.desired
            .near_call_tries
            .iter()
            .map(|done| done.rise_db)
            .reduce(f32::min)
            .map_or(SENTENCE_NO_CALL_DB.min(bar), |quietest| {
                (quietest - lamp_near_call::UNDER_QUIETEST_DB)
                    .min(bar)
                    .max(SENTENCE_LOWEST_DB)
            })
    }

    /// How many words came back for the line, never the words. Taken once,
    /// while its try is open.
    pub(crate) fn sentence_words(&mut self, words: usize) {
        if self
            .near
            .listening
            .as_ref()
            .is_some_and(|listening| listening.sentence)
            && let Some(line) = self.near.line.as_mut()
            && line.words.is_none()
        {
            line.words = Some(words);
        }
    }

    /// Judge the sentence try once a line's words are in, or once the
    /// window and the words' wait have passed.
    fn sentence_judge(&mut self, now_us: u64) {
        let Some(listening) = self.near.listening.as_ref() else {
            return;
        };
        // A line whose words came back none ("okay", a hum, the room) is not
        // the sentence: the window stays open for one with words.
        let worded = self
            .near
            .line
            .is_some_and(|line| line.words.is_some_and(|words| words > 0));
        if !worded && now_us < listening.ended_us + SENTENCE_WINDOW_US + WORDS_WAIT_US {
            return;
        }
        let listening = self.near.listening.take().expect("open");
        let line = self.near.line.take();
        self.near.seen.clear();
        let bar = self.sentence_bar();
        let close = |line: &Line| line.rise_db.is_some_and(|rise| rise >= bar);
        let short = match line {
            None => Some(Short::Level),
            Some(line) => {
                let words = line.words.unwrap_or(0) as u64;
                if words == 0 {
                    Some(Short::Coverage)
                } else if !close(&line) {
                    Some(Short::Level)
                } else if words * MS_A_WORD < line.speech_ms {
                    Some(Short::Coverage)
                } else {
                    None
                }
            }
        };
        self.note_try(json!({"kind":"onboarding_sentence_try",
            "prompt":listening.prompt,"accepted":short.is_none(),
            "short":short.map(Short::name),"words":line.and_then(|line| line.words),
            "speech_ms":line.map(|line| line.speech_ms),
            "rise_db":line.and_then(|line| line.rise_db),"bar_db":bar,
            "said_close":line.as_ref().map(close),"at_us":now_us}));
        let outcome = match short {
            None => Outcome::Done,
            Some(short) if self.desired.sentence_prompts < SENTENCE_PROMPTS => {
                self.near.short = Some(short);
                self.near.retry_after_us = now_us.saturating_add(1_000_000);
                return;
            }
            Some(_) => Outcome::Failed,
        };
        self.sentence_finished(outcome, now_us);
    }

    fn sentence_finished(&mut self, outcome: Outcome, now_us: u64) {
        self.finish(Step::HowYouSound, outcome, None, now_us);
        self.owed = Some(
            if outcome == Outcome::Done {
                self.definition.words.sentence_done.as_str()
            } else {
                self.definition.words.sentence_failed.as_str()
            }
            .to_owned(),
        );
    }

    /// Judge the try being listened for, once its evidence is all in. Pure
    /// computation on bounded buffers; never waits for storage.
    pub(super) fn near_judge(&mut self, now_us: u64) {
        if self
            .near
            .listening
            .as_ref()
            .is_some_and(|listening| listening.sentence)
        {
            self.sentence_judge(now_us);
            return;
        }
        let due = self.near.listening.as_ref().is_some_and(|listening| {
            now_us >= listening.ended_us + COLLECT_MS * 1_000 + DELIVERY_GRACE_US
        });
        if !due {
            return;
        }
        let listening = self.near.listening.take().expect("due");
        let from_end = |ms: u64| ms as i64 - listening.ended_ms as i64;
        // Every name heard, as (ms from the prompt's end, score), for the
        // try's record and journal line.
        let heard: Vec<(i64, f32)> = self
            .near
            .seen
            .iter()
            .filter_map(|seen| match seen {
                Seen::Hearing(hearing, _) => Some((from_end(hearing.began_ms), hearing.score)),
                _ => None,
            })
            .collect();
        // A name begun over the prompt's last word ([`OVER_PROMPT_MS`]).
        let early = self.near.seen.iter().any(|seen| match seen {
            Seen::Hearing(hearing, heard_ms) => {
                hearing.began_ms < listening.ended_ms
                    && hearing.score >= SCORE
                    && call_began_ms(hearing, *heard_ms, listening.ended_ms).is_some()
            }
            _ => false,
        });
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
        } else if early {
            self.near.last = Some(false);
            let again = self.offer_again(false, listening.prompt);
            self.note_try(json!({"kind":"onboarding_near_try",
                "prompt":listening.prompt,"accepted":false,"why":"answered_early",
                "offered_again":again,"heard":heard,"at_us":now_us}));
            return;
        } else {
            near_try(listening.ended_ms, &hearings, &levels, &spoke).map_err(|why| why.to_string())
        };
        // When the lamp spoke, as ms from the prompt's end (none: still).
        let spoke: Vec<(i64, Option<i64>)> = spoke
            .iter()
            .map(|span| (from_end(span.began_ms), span.ended_ms.map(from_end)))
            .collect();
        self.near.last = Some(judged.is_ok());
        match judged {
            Ok(done) => {
                self.desired.near_call_tries.push(done);
                self.save();
                self.note_try(
                    json!({"kind":"onboarding_near_try","prompt":listening.prompt,
                    "accepted":true,"rise_db":done.rise_db,"score":done.score,
                    "heard":heard,"at_us":now_us}),
                );
            }
            Err(why) => self.note_try(json!({"kind":"onboarding_near_try",
                "prompt":listening.prompt,"accepted":false,"why":why,"heard":heard,
                "spoke":spoke,"at_us":now_us})),
        }
    }

    /// One journal line for each try, accepted or refused (#3504): the
    /// session's record of it, with the reason and the numbers that decided
    /// it. Numbers only: nothing of the sound or the words.
    fn note_try(&mut self, record: serde_json::Value) {
        eprintln!("lamp-voice onboarding: how_you_sound try: {record}");
        self.records.push(record);
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
        // The name part is over; the step goes on to its sentence part.
        self.near.named = Some(outcome);
        self.records.push(json!({"kind":"onboarding_near_call",
            "outcome":outcome,"at_us":now_us}));
        self.owed = Some(
            if outcome == Outcome::Done {
                self.definition.words.near_done.as_str()
            } else {
                self.definition.words.near_failed.as_str()
            }
            .to_owned(),
        );
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
        let sentence = self.near.named.is_some();
        let prompts = *self.prompts(sentence);
        if sentence {
            // Every sentence prompt was used, its try never judged (a new
            // voice cycle): failed.
            if self.near.offer.is_none() && prompts >= SENTENCE_PROMPTS {
                self.sentence_finished(Outcome::Failed, now_us);
                return;
            }
        } else if self.near.offer.is_none()
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
                let committed = if offer.sentence {
                    self.committed.sentence_prompts
                } else {
                    self.committed.near_call_prompts
                };
                if committed >= offer.prompt {
                    offer.persisted = true;
                } else {
                    let (part, prompt) = (offer.sentence, offer.prompt);
                    *self.prompts(part) = prompt;
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
        debug_assert!(sentence || usize::from(prompt) <= MOST_PROMPTS);
        self.near.offer = Some(Offer {
            token,
            sentence,
            prompt,
            ready: true,
            claimed: false,
            persisted: false,
            offered_us: now_us,
            bound: None,
        });
        // Its budget is committed before the invitation is published.
        *self.prompts(sentence) = prompt;
        self.save();
        if let Some(led) = self.led.as_mut() {
            led.asked_us = Some(now_us);
        }
        self.records.push(
            json!({"kind":"onboarding_step","step":Step::HowYouSound.name(),
            "state":"offered","part":if sentence { "sentence" } else { "name" },
            "prompt":prompt,"at_us":now_us}),
        );
    }
}

/// When a hearing's call began, for holding the reply in a try whose prompt
/// ended at `ended_ms`: its own start, or the prompt's end for a name begun
/// at most [`OVER_PROMPT_MS`] before it that ended after it (its final
/// hearing, sent at `heard_ms`, came after the end). None for one begun
/// earlier. A name begun before the end is never measured: the prompt is
/// offered again (`near_judge`).
fn call_began_ms(hearing: &Hearing, heard_ms: u64, ended_ms: u64) -> Option<u64> {
    if hearing.began_ms >= ended_ms {
        Some(hearing.began_ms)
    } else if hearing.began_ms + OVER_PROMPT_MS >= ended_ms && heard_ms > ended_ms {
        Some(ended_ms)
    } else {
        None
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
