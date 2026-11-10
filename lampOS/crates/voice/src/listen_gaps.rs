//! Listening gaps (#828, `LAMP_LISTEN_GAPS=on`, off unless set): at a
//! sentence end in a reply that has already spoken a while, the lamp holds
//! its next words until it has been quiet [`GAP_US`], so a person's "stop"
//! has a window to be heard in.
//!
//! On lamp-4ace a short word said on top of the lamp's speech is never heard
//! over its echo. Every yield of the first builder's ten cut-ins on 17 Oct
//! came where the lamp's speech left a pause. A person telling a story
//! pauses about this long between sentences too.
//!
//! The gap is the speaker's own provider-pause path, entered on purpose:
//! the next chunk is not handed over, and the speaker writes real zeros
//! through its zero-only API while speech is owed. So the canceller's
//! reference and the render's coverage stay exact. The hearing takes
//! speech in that silence as a person over the lamp
//! (`lamp_audio::duplex::Hearing::silence_heard`): a gap of zeros alone
//! gave the yield nothing to hear.
//!
//! A sentence end is read from the PCM: a run of at least [`LOW_RUN_CHUNKS`]
//! chunks under [`LOW_RMS`] at a chunk boundary, then a chunk over it. The
//! provider's transcript is not timed against its audio, so it is not used.
//! Never before [`AFTER_FRAMES`] of a reply; never while a yield is pending
//! (the mask hands nothing over then anyway).
use lamp_turn_owner::TurnOwner;
use serde_json::{Value, json};

/// `on` turns listening gaps on, and nothing else.
pub const ENVIRONMENT: &str = "LAMP_LISTEN_GAPS";

/// Whether the environment turns them on. In a test, this thread's
/// [`set_for_test`] decides instead, so parallel tests never share the
/// process environment.
pub fn on_from_environment() -> bool {
    #[cfg(test)]
    if let Some(on) = ON_FOR_TEST.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

#[cfg(test)]
thread_local! {
    // Process ownership: this test process writes and reads this fixture state; not IPC.
    static ON_FOR_TEST: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// This thread's `LAMP_LISTEN_GAPS`, for a test; `None` reads the
/// environment again.
#[cfg(test)]
pub(crate) fn set_for_test(on: Option<bool>) {
    ON_FOR_TEST.with(|cell| cell.set(on));
}

/// The speaker's frames a second (`lamp_audio::RENDER_RATE`).
const FRAMES_PER_SECOND: u64 = lamp_audio::RENDER_RATE as u64;

/// A reply speaks this long, about 2 s, before its first gap: never before
/// its first words (the first builder on #828).
pub const AFTER_FRAMES: u64 = 2 * FRAMES_PER_SECOND;

/// A chunk under this RMS, in 16-bit sample units, is a pause in the speech
/// (about 41 dB under full scale).
pub const LOW_RMS: f64 = 300.0;

/// Chunks of 10 ms under [`LOW_RMS`] in a row that end a sentence: 80 ms. A
/// stop consonant inside a word is shorter.
pub const LOW_RUN_CHUNKS: u32 = 8;

/// How long the lamp is quiet at a sentence end, from the first quiet chunk:
/// its own pause, then real zeros until the next words.
pub const GAP_US: u64 = 250_000;

/// How far past its hold's end the speaker's zeros may run and still be
/// the hold's own: the next words go on the coordinator's next pass once it
/// ends, and the speaker writes them next, well inside this. Zeros that run
/// on past it waited for words that were late, a delivery gap begun in the
/// hold (Nine's P3 on #1264).
pub const OVERRUN_US: u64 = 100_000;

/// A sentence end being held: from the first quiet chunk handed over, until
/// the next words may go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Held {
    began_us: u64,
    until_us: u64,
    released: bool,
}

/// The last gap held: whose, from when, until when it was to hold, and when
/// the next words went, once they did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LastGap {
    owner: TurnOwner,
    began_us: u64,
    until_us: u64,
    released_us: Option<u64>,
}

/// One reply's listening gaps, kept by the playback mask.
#[derive(Debug, Default)]
pub struct ListenGaps {
    owner: Option<TurnOwner>,
    sent_frames: u64,
    low_chunks: u32,
    low_since_us: Option<u64>,
    held: Option<Held>,
    /// The last gap held, for the speaker's zeros in it.
    last: Option<LastGap>,
    /// A gap begun since the coordinator last looked, to be recorded.
    news: Option<Value>,
}

fn rms(samples: &[i16]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples
        .iter()
        .map(|&sample| f64::from(sample).powi(2))
        .sum();
    (sum / samples.len() as f64).sqrt()
}

impl ListenGaps {
    fn reset_for(&mut self, owner: TurnOwner) {
        if self.owner != Some(owner) {
            *self = Self {
                owner: Some(owner),
                last: self.last,
                news: self.news.take(),
                ..Self::default()
            };
        }
    }

    /// Whether `owner`'s next chunk, `next`, waits at `now_us`: after
    /// [`AFTER_FRAMES`] of the reply, at the first loud chunk after
    /// [`LOW_RUN_CHUNKS`] quiet ones, until [`GAP_US`] after the first of
    /// them. Each sentence end is held once.
    pub fn holds(&mut self, owner: TurnOwner, next: &[i16], now_us: u64) -> bool {
        self.reset_for(owner);
        if let Some(held) = self.held.as_mut() {
            if held.released {
                return false;
            }
            if now_us < held.until_us {
                return true;
            }
            held.released = true;
            if let Some(last) = self.last.as_mut() {
                last.released_us = Some(now_us);
            }
            return false;
        }
        let Some(began_us) = self.low_since_us else {
            return false;
        };
        if self.sent_frames < AFTER_FRAMES
            || self.low_chunks < LOW_RUN_CHUNKS
            || rms(next) < LOW_RMS
        {
            return false;
        }
        let until_us = began_us.saturating_add(GAP_US);
        if now_us >= until_us {
            // Its own pause was long enough.
            self.held = Some(Held {
                began_us,
                until_us,
                released: true,
            });
            return false;
        }
        self.held = Some(Held {
            began_us,
            until_us,
            released: false,
        });
        self.last = Some(LastGap {
            owner,
            began_us,
            until_us,
            released_us: None,
        });
        self.news = Some(
            json!({"kind":"listen_gap","owner":owner,"turn":owner.turn(),
            "began_us":began_us,"until_us":until_us,"at_us":now_us,
            "sent_ms":self.sent_frames * 1_000 / FRAMES_PER_SECOND,
            "quiet_before_ms":now_us.saturating_sub(began_us) / 1_000,
            "boundary":"from the first quiet chunk handed over; the speaker writes zeros until the next words"}),
        );
        true
    }

    /// `owner`'s chunk `samples` was handed to the speaker at `now_us`.
    pub fn sent(&mut self, owner: TurnOwner, samples: &[i16], now_us: u64) {
        self.reset_for(owner);
        self.sent_frames = self.sent_frames.saturating_add(samples.len() as u64);
        if rms(samples) < LOW_RMS {
            self.low_chunks = self.low_chunks.saturating_add(1);
            self.low_since_us.get_or_insert(now_us);
        } else {
            self.low_chunks = 0;
            self.low_since_us = None;
            self.held = None;
        }
    }

    /// Whether the speaker's zeros for `owner` that began at `started_us`
    /// are a listening gap's, not a delivery gap.
    pub fn covers(&self, owner: TurnOwner, started_us: u64) -> bool {
        self.last.is_some_and(|last| {
            last.owner == owner
                && started_us >= last.began_us
                && last
                    .released_us
                    .is_none_or(|released| started_us <= released)
        })
    }

    /// How far past its hold's end the zeros [`Self::covers`] calls a gap's,
    /// last accepted at `last_zero_us`, ran, when that is more than
    /// [`OVERRUN_US`]: they were then a delivery gap too. The hold's end, not
    /// the next words' going: those go only once there are words, so late
    /// ones are let go late.
    pub fn overran_us(&self, owner: TurnOwner, started_us: u64, last_zero_us: u64) -> Option<u64> {
        let last = self.last.filter(|_| self.covers(owner, started_us))?;
        let past = last_zero_us.saturating_sub(last.until_us);
        (past > OVERRUN_US).then_some(past)
    }

    /// A gap begun since the last look, to be recorded.
    pub fn take_news(&mut self) -> Option<Value> {
        self.news.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_turn_owner::{
        AdmissionState, AdmittedInput, BootId, CaptureState, Controller, MonoTime, Permission,
    };

    fn owner() -> TurnOwner {
        let at = MonoTime::from_micros(1);
        let mut controller = Controller::new(BootId::try_from([3; 16]).unwrap(), at);
        controller
            .set_microphone_permission(at, Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(1_000_000);
        controller
            .set_capture(at, CaptureState::RetainingUntil(until))
            .unwrap();
        controller
            .set_admission(at, AdmissionState::OpenUntil(until))
            .unwrap();
        controller.admit(at, AdmittedInput::NewTurn).unwrap()
    }

    const LOUD: [i16; 240] = [6_000; 240];
    const QUIET: [i16; 240] = [40; 240];

    /// Hands over `chunks` from `at_us`, one each 10 ms, as long as nothing
    /// holds; then the time reached.
    fn speak(gaps: &mut ListenGaps, owner: TurnOwner, chunks: &[[i16; 240]], at_us: u64) -> u64 {
        let mut now = at_us;
        for chunk in chunks {
            assert!(!gaps.holds(owner, chunk, now), "held at {now}");
            gaps.sent(owner, chunk, now);
            now += 10_000;
        }
        now
    }

    /// Two sentences: 2.5 s, then 100 ms quiet. The second sentence's first
    /// chunk waits until 250 ms after the first quiet one, once, and the
    /// zeros in that time are the gap's.
    #[test]
    fn a_sentence_end_after_two_seconds_is_held_to_a_quarter_second_once() {
        let mut gaps = ListenGaps::default();
        let owner = owner();
        let mut now = speak(&mut gaps, owner, &[LOUD; 250], 0);
        let quiet_from = now;
        now = speak(&mut gaps, owner, &[QUIET; 10], now);
        assert!(gaps.holds(owner, &LOUD, now));
        let news = gaps.take_news().unwrap();
        assert_eq!(news["began_us"], quiet_from);
        assert_eq!(news["until_us"], quiet_from + GAP_US);
        assert!(gaps.holds(owner, &LOUD, quiet_from + GAP_US - 1));
        assert!(gaps.covers(owner, quiet_from + 120_000));
        assert!(!gaps.holds(owner, &LOUD, quiet_from + GAP_US));
        gaps.sent(owner, &LOUD, quiet_from + GAP_US);
        assert!(gaps.covers(owner, quiet_from + GAP_US));
        assert!(!gaps.covers(owner, quiet_from + GAP_US + 20_000));
        // On, with no second gap at the same sentence end.
        speak(&mut gaps, owner, &[LOUD; 20], quiet_from + GAP_US + 10_000);
        assert!(gaps.take_news().is_none());
    }

    /// Nine's P3s on #1264: zeros the speaker began before the hold, or
    /// after the next words went, are no listening gap's. Zeros begun in it
    /// that ran on past its end by more than 100 ms waited for late words:
    /// a delivery gap too. Late words are let go late, so the hold's end,
    /// not their going, is what counts.
    #[test]
    fn zeros_outside_a_hold_or_running_past_it_are_a_delivery_gap() {
        let mut gaps = ListenGaps::default();
        let owner = owner();
        let now = speak(&mut gaps, owner, &[LOUD; 250], 0);
        let quiet_from = now;
        let now = speak(&mut gaps, owner, &[QUIET; 10], now);
        assert!(gaps.holds(owner, &LOUD, now));
        let until = quiet_from + GAP_US;
        assert!(!gaps.covers(owner, quiet_from - 1));
        assert_eq!(
            gaps.overran_us(owner, quiet_from - 1, until + 500_000),
            None
        );
        let started = quiet_from + 120_000;
        assert!(gaps.covers(owner, started));
        assert_eq!(gaps.overran_us(owner, started, until), None);
        assert_eq!(gaps.overran_us(owner, started, until + OVERRUN_US), None);
        // The next words come 300 ms late: let go when they come.
        let late = until + 300_000;
        assert!(!gaps.holds(owner, &LOUD, late));
        gaps.sent(owner, &LOUD, late);
        assert!(gaps.covers(owner, started));
        assert_eq!(gaps.overran_us(owner, started, late), Some(300_000));
        assert!(!gaps.covers(owner, late + 10_000));
    }

    /// None in a reply's first two seconds, none for a pause under 80 ms,
    /// and none where the reply's own pause was long enough already.
    #[test]
    fn no_gap_before_two_seconds_for_a_short_pause_or_after_a_long_one() {
        let owner = owner();
        let mut early = ListenGaps::default();
        let now = speak(&mut early, owner, &[LOUD; 150], 0);
        let now = speak(&mut early, owner, &[QUIET; 10], now);
        assert!(!early.holds(owner, &LOUD, now));
        let mut short = ListenGaps::default();
        let now = speak(&mut short, owner, &[LOUD; 250], 0);
        let now = speak(&mut short, owner, &[QUIET; 7], now);
        assert!(!short.holds(owner, &LOUD, now));
        let mut long = ListenGaps::default();
        let now = speak(&mut long, owner, &[LOUD; 250], 0);
        let now = speak(&mut long, owner, &[QUIET; 30], now);
        assert!(!long.holds(owner, &LOUD, now));
        assert!(long.take_news().is_none());
    }

    /// Each sentence end of a long reply gets its own gap, and a new reply
    /// starts its count again.
    #[test]
    fn each_sentence_end_gets_its_own_gap_and_a_new_reply_counts_again() {
        let mut gaps = ListenGaps::default();
        let owner = owner();
        let mut now = speak(&mut gaps, owner, &[LOUD; 250], 0);
        for _ in 0..2 {
            now = speak(&mut gaps, owner, &[QUIET; 10], now);
            assert!(gaps.holds(owner, &LOUD, now));
            assert!(gaps.take_news().is_some());
            now += GAP_US;
            now = speak(&mut gaps, owner, &[LOUD; 100], now);
        }
        let other = {
            let at = MonoTime::from_micros(1);
            let mut controller = Controller::new(BootId::try_from([4; 16]).unwrap(), at);
            controller
                .set_microphone_permission(at, Permission::Allowed)
                .unwrap();
            let until = MonoTime::from_micros(1_000_000);
            controller
                .set_capture(at, CaptureState::RetainingUntil(until))
                .unwrap();
            controller
                .set_admission(at, AdmissionState::OpenUntil(until))
                .unwrap();
            controller.admit(at, AdmittedInput::NewTurn).unwrap()
        };
        let now = speak(&mut gaps, other, &[LOUD; 100], now);
        let now = speak(&mut gaps, other, &[QUIET; 10], now);
        assert!(!gaps.holds(other, &LOUD, now), "a new reply's first second");
    }
}
