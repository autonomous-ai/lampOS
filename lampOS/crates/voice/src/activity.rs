//! Local neural speech activity with finite pre-roll and explicit endpointing.
//! This detector does not identify a speaker or an addressee. Its thresholds
//! are initial experiment settings, not socially qualified release defaults.
use lamp_audio::{CAPTURE_RATE, CAPTURE_SAMPLES, CaptureBlock};
use lamp_turn_end_wire::{
    Availability, Classifier, Outcome, Submit, Token, Verdict, WINDOW_SAMPLES,
};
use serde::Serialize;
use sonora_agc2::vad_wrapper::VoiceActivityDetectorWrapper;
use std::collections::VecDeque;

pub const PRE_ROLL_BLOCKS: usize = 30;
pub const START_BLOCKS: u16 = 6;
pub const END_SILENCE_BLOCKS: u16 = 60;
/// Pinned V1 Lamp profile's 1.2 s silence fallback. This does not reproduce its
/// separate Silero endpoint classifier or optional semantic confirmation.
pub const V1_END_SILENCE_BLOCKS: u16 = 120;
pub const MAX_UTTERANCE_BLOCKS: u32 = 12_000;
pub const MIN_END_SILENCE_BLOCKS: u16 = 30;
pub const MAX_END_SILENCE_BLOCKS: u16 = 200;
/// Scores below this are a low-activity block for every endpoint policy.
pub(crate) const SILENCE_PROBABILITY: f32 = 0.35;
/// [`START_BLOCKS`] blocks in a row scoring this or more begin an utterance.
pub(crate) const START_PROBABILITY: f32 = 0.80;
/// An utterance with no more speech than this when the lamp's name is
/// heard in it is only the call ("Hey Lamp," / "Lamp,"): `--call-wait-ms`
/// holds its end for the rest (#782). "Hey Lamp," played at the office's
/// rate is about 0.7 s.
pub const CALL_SPEECH_BLOCKS: u16 = 120;
/// A name heard this long before the utterance began is its own: the
/// detector takes a moment to be sure someone is speaking.
pub const CALL_BEFORE_US: u64 = 500_000;
/// The longest a call may wait for the rest: 3 s.
pub const MAX_CALL_WAIT_BLOCKS: u16 = 300;
/// Quiet at least this long before speech, while a call waits, makes that
/// speech the rest of what was said, which ends as any other utterance
/// does. Shorter quiet is a gap between the call's own words: its first
/// sign (`name/rising`) comes while the name is still being said, and the
/// call goes on for a moment after it (#891).
pub const CALL_PAUSE_BLOCKS: u16 = 20;

/// Where the coordinator reads whether a person who speaks over the lamp
/// keeps their first words (#828). `kept` only.
///
/// Why: the duplex rule stops the lamp only once a run of blocks is plainly
/// someone over it, and what was said before that, under the lamp's voice,
/// was dropped with the 300 ms before the onset. On lamp-4ace on 16 October
/// "Lamp, stop. What is two plus two?" was sent as its tail and heard as
/// "vestido"; in the first builder's two recordings of 01:51 five requests
/// said over a story began 1.2 to 2.5 s into what was said.
pub const FIRST_WORDS_ENVIRONMENT: &str = "LAMP_FIRST_WORDS";

/// Whether the voice's environment says `kept`.
pub fn first_words_kept() -> bool {
    std::env::var(FIRST_WORDS_ENVIRONMENT).ok().as_deref() == Some("kept")
}

/// With first words kept, how far back a yield's request may begin: 1.6 s,
/// the duplex hold's longest retained age.
pub const REACH_BACK_BLOCKS: usize = 160;
/// And how much before its earliest speech-like block it begins.
pub const REACH_LEAD_BLOCKS: usize = 20;

/// With `linear`, the detector reads the echo canceller's linear output in
/// place of the processed block (#828): the residual suppressor that follows
/// the canceller can take a person's "Stop." said over the lamp's own voice
/// down with the echo. Only the detector: the provider's audio, the residual
/// and the evidence keep the processed block. Off unless set.
pub const INPUT_ENVIRONMENT: &str = "LAMP_VAD_INPUT";

/// Whether this worker's detector reads the linear output, read once.
pub fn reads_linear() -> bool {
    std::env::var_os(INPUT_ENVIRONMENT).as_deref() == Some(std::ffi::OsStr::new("linear"))
}

/// The block the detector reads: the linear output when asked for and there
/// is one for this block, `clean` otherwise.
pub fn detector_input<'a>(
    linear_on: bool,
    clean: &'a CaptureBlock,
    linear: Option<&'a CaptureBlock>,
) -> &'a CaptureBlock {
    match linear {
        Some(linear) if linear_on => linear,
        _ => clean,
    }
}

pub struct SpeechProbability {
    vad: VoiceActivityDetectorWrapper,
    buffer: [f32; CAPTURE_SAMPLES],
}

impl Default for SpeechProbability {
    fn default() -> Self {
        Self {
            vad: VoiceActivityDetectorWrapper::new(
                sonora_simd::detect_backend(),
                CAPTURE_RATE as i32,
            ),
            buffer: [0.0; CAPTURE_SAMPLES],
        }
    }
}

impl SpeechProbability {
    pub fn analyze(&mut self, block: &CaptureBlock) -> f32 {
        // Sonora's internal WebRTC VAD takes FloatS16 samples, not [-1, 1].
        for (dst, &src) in self.buffer.iter_mut().zip(block) {
            *dst = f32::from(src);
        }
        self.vad.analyze(&self.buffer)
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Debug)]
pub struct ObservedAudio {
    pub sequence: u64,
    pub captured_at_us: u64,
    pub samples: CaptureBlock,
}

#[derive(Debug)]
pub enum Activity {
    Quiet,
    Start(Vec<ObservedAudio>),
    Continue(ObservedAudio),
    End(ObservedAudio),
    Fault(&'static str),
}

pub struct TurnDetector {
    prefix: VecDeque<ObservedAudio>,
    /// With first words kept: each prefix block's own score, before any
    /// mask, or `None` for a block the duplex rule could not hear, which a
    /// request may not reach back past. Empty otherwise.
    scores: VecDeque<Option<f32>>,
    first_words: bool,
    /// The next start is a yield's, and may reach back.
    reach_back: bool,
    start_count: u16,
    silence_count: u16,
    end_silence_blocks: u16,
    active_blocks: u32,
    active: bool,
    last: Option<(u64, u64)>,
    confirmation: Option<Confirmation>,
    decision: Option<EndpointDecision>,
    abandoned: bool,
    ignore_through_us: Option<u64>,
    /// After a capture gap cut someone's utterance (#784): what they still
    /// say of it is dropped until they pause, so it is not a new question.
    await_pause: bool,
    /// How long an utterance that is only the lamp's name waits for the
    /// rest (`--call-wait-ms`, #782); None, the default, waits as any other.
    call_wait_blocks: Option<u16>,
    /// When the utterance in hand began, how much of it was speech, and
    /// whether its end waits for the rest of a call, or already did.
    began_us: u64,
    speech_blocks: u16,
    waiting: bool,
    waited: bool,
}

impl Default for TurnDetector {
    fn default() -> Self {
        Self {
            prefix: VecDeque::with_capacity(PRE_ROLL_BLOCKS),
            scores: VecDeque::new(),
            first_words: false,
            reach_back: false,
            start_count: 0,
            silence_count: 0,
            end_silence_blocks: END_SILENCE_BLOCKS,
            active_blocks: 0,
            active: false,
            last: None,
            confirmation: None,
            decision: None,
            abandoned: false,
            ignore_through_us: None,
            await_pause: false,
            call_wait_blocks: None,
            began_us: 0,
            speech_blocks: 0,
            waiting: false,
            waited: false,
        }
    }
}

impl TurnDetector {
    /// Conservative qualification baseline: retain speech across a short
    /// low-score interval instead of ending and then interrupting that question.
    pub fn v1_baseline() -> Self {
        Self {
            end_silence_blocks: V1_END_SILENCE_BLOCKS,
            ..Self::default()
        }
    }

    /// The same detector with another silence window, in 10 ms blocks. The
    /// 1.2 s of the V1 profile is 0.5 s more than the longest pause inside
    /// thirteen of fourteen utterances of the first person recorded on the
    /// lamp; the caller chooses, and the choice is recorded with the policy.
    pub fn with_end_silence(mut self, blocks: u16) -> Result<Self, &'static str> {
        if !(MIN_END_SILENCE_BLOCKS..=MAX_END_SILENCE_BLOCKS).contains(&blocks) {
            return Err("end-of-speech wait must be 300 to 2000 ms");
        }
        self.end_silence_blocks = blocks;
        Ok(self)
    }

    /// Confirm earlier endpoints with a classifier. Without a usable verdict
    /// the detector still ends after this detector's silence window.
    pub fn with_confirmation(
        mut self,
        classifier: Box<dyn Classifier>,
        mut policy: ConfirmPolicy,
    ) -> Result<Self, &'static str> {
        policy.fallback_blocks = self.end_silence_blocks;
        policy.validate()?;
        self.confirmation = Some(Confirmation::new(classifier, policy));
        Ok(self)
    }

    /// Keep [`REACH_BACK_BLOCKS`] of pre-roll, with each block's own score,
    /// so that a yield's request can begin at the person's first words
    /// (`LAMP_FIRST_WORDS=kept`, #828). Without it, as before, 300 ms.
    pub fn keep_first_words(&mut self, on: bool) {
        self.first_words = on;
    }

    /// The next utterance to start is a yield's, said over the lamp: with
    /// first words kept, it begins at the earliest speech-like block of the
    /// pre-roll. Only for the next block pushed.
    pub fn reach_back(&mut self) {
        self.reach_back = self.first_words;
    }

    /// Configured silence window; with confirmation it is the fallback.
    /// An utterance that is only the lamp's name waits this many blocks of
    /// quiet for the rest (#782): longer than the end-of-speech wait, and
    /// [`MAX_CALL_WAIT_BLOCKS`] at most.
    pub fn with_call_wait(mut self, blocks: u16) -> Result<Self, &'static str> {
        if blocks <= self.end_silence_blocks || blocks > MAX_CALL_WAIT_BLOCKS {
            return Err(
                "a call's wait must be longer than the end-of-speech wait, and 3 s at most",
            );
        }
        self.call_wait_blocks = Some(blocks);
        Ok(self)
    }

    pub fn call_wait_blocks(&self) -> Option<u16> {
        self.call_wait_blocks
    }

    /// The lamp's name was heard, beginning at `began_us` by the capture
    /// clock. If the utterance in hand is only that call so far, its end
    /// now waits for the rest, once, for the call wait: until more speech
    /// comes, which is then ended as any other, or the wait runs out.
    /// Whether it waits.
    pub fn name_heard(&mut self, began_us: u64) -> bool {
        let applies = self.call_wait_blocks.is_some()
            && self.active
            && !self.abandoned
            && !self.waited
            && began_us.saturating_add(CALL_BEFORE_US) >= self.began_us
            && self.speech_blocks <= CALL_SPEECH_BLOCKS;
        if applies {
            self.waiting = true;
            self.waited = true;
        }
        applies
    }

    pub fn endpoint_silence_ms(&self) -> u64 {
        u64::from(self.end_silence_blocks) * 10
    }

    pub fn confirmation_policy(&self) -> Option<ConfirmPolicy> {
        self.confirmation
            .as_ref()
            .map(|confirmation| confirmation.policy)
    }

    pub fn classifier_availability(&self) -> Option<Availability> {
        self.confirmation
            .as_ref()
            .map(|confirmation| confirmation.classifier.availability())
    }

    /// How the most recent `Activity::End` was decided.
    pub fn last_endpoint(&self) -> Option<&EndpointDecision> {
        self.decision.as_ref()
    }

    /// The pause the utterance in hand is in, in blocks, including the last
    /// block pushed: low-score blocks in a row, counted exactly as the
    /// silence window counts them. Zero with no utterance in hand.
    pub fn pause_blocks(&self) -> u16 {
        if self.active { self.silence_count } else { 0 }
    }

    pub fn push(&mut self, frame: ObservedAudio, probability: f32) -> Activity {
        self.push_heard(frame, probability, Some(probability))
    }

    /// [`Self::push`], with the block's own score before any mask: `None`
    /// when it was not heard with echo evidence. Only kept with first words.
    pub fn push_heard(
        &mut self,
        frame: ObservedAudio,
        probability: f32,
        own: Option<f32>,
    ) -> Activity {
        let reach_back = std::mem::take(&mut self.reach_back);
        if let Err(reason) = self.validate(&frame, probability) {
            return Activity::Fault(reason);
        }
        if !self.active
            && self
                .ignore_through_us
                .is_some_and(|at| frame.captured_at_us <= at)
        {
            return Activity::Quiet;
        }
        if self.await_pause {
            self.silence_count = if probability < SILENCE_PROBABILITY {
                self.silence_count + 1
            } else {
                0
            };
            if self.silence_count >= self.end_silence_blocks {
                self.await_pause = false;
                self.silence_count = 0;
            }
            return Activity::Quiet;
        }
        self.ignore_through_us = None;
        if self.active {
            self.active_blocks += 1;
            if self.active_blocks >= MAX_UTTERANCE_BLOCKS {
                self.reset();
                return Activity::Fault("utterance exceeded 120 second bound");
            }
            let speech = probability >= SILENCE_PROBABILITY;
            let quiet_before = self.silence_count;
            self.silence_count = if speech { 0 } else { self.silence_count + 1 };
            if speech {
                self.speech_blocks = self.speech_blocks.saturating_add(1);
            }
            let mut decision = match self.confirmation.as_mut().filter(|_| !self.abandoned) {
                Some(confirmation) => confirmation.observe(&frame.samples, speech),
                None => (self.silence_count >= self.end_silence_blocks)
                    .then(|| EndpointDecision::silence(self.silence_count)),
            };
            // Only the lamp's name so far: nothing ends it until the rest
            // comes or the call's wait runs out (#782).
            if self.waiting
                && let Some(wait) = self.call_wait_blocks
            {
                if speech {
                    // The rest, after the call's pause, or more said than
                    // a call: ended as any other utterance. The call's own
                    // words, still being said, keep the wait.
                    if quiet_before >= CALL_PAUSE_BLOCKS || self.speech_blocks > CALL_SPEECH_BLOCKS
                    {
                        self.waiting = false;
                    }
                } else if self.silence_count < wait {
                    decision = None;
                } else {
                    let mut ran_out =
                        decision.unwrap_or_else(|| EndpointDecision::silence(self.silence_count));
                    ran_out.call_wait_ms = Some(u64::from(wait) * 10);
                    decision = Some(ran_out);
                }
            }
            if let Some(decision) = decision {
                self.active = false;
                self.waiting = false;
                self.start_count = 0;
                self.silence_count = 0;
                self.abandoned = false;
                self.decision = Some(decision);
                return Activity::End(frame);
            }
            return Activity::Continue(frame);
        }
        let kept = if self.first_words {
            REACH_BACK_BLOCKS
        } else {
            PRE_ROLL_BLOCKS
        };
        if self.prefix.len() == kept {
            self.prefix.pop_front();
            self.scores.pop_front();
        }
        self.prefix.push_back(frame);
        if self.first_words {
            self.scores.push_back(own);
        }
        self.start_count = if probability >= START_PROBABILITY {
            self.start_count + 1
        } else {
            0
        };
        if self.start_count >= START_BLOCKS {
            let begin = self.begin(reach_back);
            self.prefix.drain(..begin);
            self.scores.clear();
            self.active = true;
            self.active_blocks = self.prefix.len() as u32;
            self.silence_count = 0;
            self.began_us = self.prefix.front().map_or(0, |first| first.captured_at_us);
            self.speech_blocks = self.start_count;
            self.waiting = false;
            self.waited = false;
            if let Some(confirmation) = self.confirmation.as_mut() {
                confirmation.begin(&self.prefix);
            }
            return Activity::Start(self.prefix.drain(..).collect());
        }
        Activity::Quiet
    }

    fn validate(&mut self, frame: &ObservedAudio, probability: f32) -> Result<(), &'static str> {
        if !probability.is_finite() || !(0.0..=1.0).contains(&probability) {
            self.reset();
            return Err("invalid speech probability");
        }
        if frame.sequence == 0
            || self.last.is_some_and(|(sequence, at)| {
                sequence.checked_add(1) != Some(frame.sequence)
                    || frame.captured_at_us < at
                    || frame.captured_at_us.saturating_sub(at) > 50_000
            })
        {
            self.reset();
            return Err("capture discontinuity");
        }
        self.last = Some((frame.sequence, frame.captured_at_us));
        Ok(())
    }

    /// Where an utterance starting now begins in the pre-roll: the last
    /// [`PRE_ROLL_BLOCKS`], or for a yield with first words kept,
    /// [`REACH_LEAD_BLOCKS`] before the earliest block scored
    /// [`SILENCE_PROBABILITY`] or more, never past a block that was not heard.
    fn begin(&self, reach_back: bool) -> usize {
        let usual = self.prefix.len().saturating_sub(PRE_ROLL_BLOCKS);
        if !reach_back || self.scores.len() != self.prefix.len() {
            return usual;
        }
        // Never past a block heard without evidence, wherever it is: every
        // block of a candidate is checked against what was heard.
        let floor = self
            .scores
            .iter()
            .rposition(Option::is_none)
            .map_or(0, |at| at + 1);
        self.scores
            .iter()
            .enumerate()
            .take(usual)
            .skip(floor)
            .find(|(_, score)| score.is_some_and(|score| score >= SILENCE_PROBABILITY))
            .map_or(usual, |(earliest, _)| {
                earliest
                    .saturating_sub(REACH_LEAD_BLOCKS)
                    .max(floor)
                    .min(usual)
            })
    }

    /// Discard prospective PCM without erasing capture continuity checks.
    pub fn suppress(
        &mut self,
        frame: &ObservedAudio,
        probability: f32,
    ) -> Result<(), &'static str> {
        self.validate(frame, probability)?;
        self.clear_prospective();
        Ok(())
    }
    /// Caller must establish that no admitted input is still streaming.
    pub fn clear_prospective(&mut self) {
        self.prefix.clear();
        self.scores.clear();
        self.start_count = 0;
        self.silence_count = 0;
        self.active_blocks = 0;
        self.active = false;
        self.abandoned = false;
        self.waiting = false;
        self.cancel_confirmation();
    }

    pub fn reset(&mut self) {
        self.prefix.clear();
        self.scores.clear();
        self.start_count = 0;
        self.silence_count = 0;
        self.active_blocks = 0;
        self.active = false;
        self.last = None;
        self.abandoned = false;
        self.ignore_through_us = None;
        self.await_pause = false;
        self.waiting = false;
        self.cancel_confirmation();
    }

    /// Start afresh after a capture gap that cut an utterance: drop what is
    /// still said of it until its speaker pauses as long as an endpoint.
    pub(crate) fn resume_after_cut(&mut self) {
        self.reset();
        self.await_pause = true;
    }

    /// Pending and late verdicts can never end a later utterance.
    fn cancel_confirmation(&mut self) {
        if let Some(confirmation) = self.confirmation.as_mut() {
            confirmation.cancel();
        }
    }

    /// A changed DSP history must not relabel an old inactive prefix. Preserve
    /// sequence validation and an already active utterance across planned AEC resets.
    pub fn discard_inactive_prefix(&mut self) {
        if !self.active {
            self.prefix.clear();
            self.scores.clear();
            self.start_count = 0;
        }
    }

    /// Cancel model work and pre-roll, while retaining an active utterance's
    /// endpoint. Hand Stop must not turn the same person's remaining speech
    /// into a new request. Its next pause uses the bounded silence fallback.
    pub(crate) fn abandon_input(&mut self, at_us: u64) {
        self.discard_inactive_prefix();
        self.cancel_confirmation();
        self.abandoned = self.active;
        self.ignore_through_us = Some(at_us);
        // A call's wait ends with it, and a sign of the name heard later
        // cannot arm one for the same utterance (#891): only a new
        // utterance can wait again.
        self.waiting = false;
        self.waited = true;
    }
}

/// Model-confirmed endpoint policy in 10 ms capture blocks.
///
/// Smart Turn is calibrated for a short silence after speech, and its verdict
/// on the same speech drifts as more trailing silence is appended. The policy
/// therefore classifies the utterance once per pause with exactly
/// `check_blocks` of trailing pause, then decides when to act on that verdict:
/// a complete verdict ends at `commit_blocks`, a clearly incomplete one holds
/// until the finite `incomplete_blocks`, and anything between, or no usable
/// verdict at all, ends on the detector's own silence window.
///
/// The default is the "parity" profile selected on labelled human clips
/// (docs/endpoint-detection.md): people often pause after a finished sentence
/// and continue, and the model cannot tell that pause from a finished turn,
/// so an earlier commit adds cutoffs. Holding clearly unfinished speech longer
/// offsets the few cutoffs a 1 s commit adds. It is an opt-in experiment, not
/// a default of the interaction core.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ConfirmPolicy {
    /// Pause length classified with the utterance (Pipecat's VAD stop is 200 ms).
    pub check_blocks: u16,
    /// A complete verdict ends the turn once the pause is at least this long.
    pub commit_blocks: u16,
    /// A verdict at or above this probability is complete.
    pub complete_probability: f32,
    /// A verdict below this probability is clearly incomplete.
    pub incomplete_probability: f32,
    /// A clearly incomplete verdict holds the turn open until this pause length.
    pub incomplete_blocks: u16,
    /// A verdict later than this after submission is ignored for this pause.
    pub result_timeout_blocks: u16,
    /// Pause endpoint without a decisive verdict: the detector's own window.
    pub fallback_blocks: u16,
    /// A complete verdict ends the turn at `commit_blocks` only for an
    /// utterance with at least this many speech blocks; a shorter one waits
    /// for `fallback_blocks`, as if the verdict were uncertain. On lamp-4ace
    /// on 2026-10-14 the 400 ms commit took "Hey Lamp," on its own for a
    /// finished turn (p 0.99) and cut the name off its question; in a busy
    /// room the question was then held for want of its name. 0: no floor.
    pub least_speech_blocks: u16,
}

impl Default for ConfirmPolicy {
    fn default() -> Self {
        Self {
            check_blocks: 20,
            commit_blocks: 100,
            complete_probability: 0.9,
            incomplete_probability: 0.01,
            incomplete_blocks: 150,
            result_timeout_blocks: 50,
            fallback_blocks: V1_END_SILENCE_BLOCKS,
            least_speech_blocks: 0,
        }
    }
}

impl ConfirmPolicy {
    /// Ends a confident verdict after 400 ms. On labelled human speech this
    /// adds cutoffs at pauses between sentences; it is for evaluation with a
    /// path that lets the person continue, never a default.
    pub fn responsive() -> Self {
        Self {
            commit_blocks: 40,
            // Longer than the lamp's name said alone ("Hey Lamp," is about
            // 0.6 s), shorter than most questions.
            least_speech_blocks: 80,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        let valid = self.check_blocks > 0
            && self.check_blocks <= self.commit_blocks
            && self.commit_blocks < self.fallback_blocks
            && self.fallback_blocks <= self.incomplete_blocks
            && self.result_timeout_blocks > 0
            && self.complete_probability > 0.0
            && self.complete_probability < 1.0
            && self.incomplete_probability >= 0.0
            && self.incomplete_probability <= self.complete_probability;
        if valid {
            Ok(())
        } else {
            Err("invalid endpoint confirmation policy")
        }
    }
}

/// Recorded with each local endpoint; not an acoustic speech-end measurement.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EndpointDecision {
    /// `silence`, `model_complete`, `model_incomplete`, `model_uncertain`
    /// (a verdict between the thresholds, ended on the silence window) or
    /// `silence_fallback` (no usable verdict).
    pub basis: &'static str,
    /// Pause length at the decision, including this block. Without
    /// confirmation it is the consecutive low-score run.
    pub silence_ms: u64,
    pub probability: Option<f32>,
    /// Why no verdict could decide this pause, for `silence_fallback`.
    pub fallback: Option<&'static str>,
    /// The verdict this decision used.
    pub verdict: Option<VerdictTrace>,
    /// The call's wait that ran out first, when the utterance was only the
    /// lamp's name (`--call-wait-ms`, #782). Absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_wait_ms: Option<u64>,
}

impl EndpointDecision {
    fn silence(blocks: u16) -> Self {
        Self {
            basis: "silence",
            silence_ms: u64::from(blocks) * 10,
            probability: None,
            fallback: None,
            verdict: None,
            call_wait_ms: None,
        }
    }
}

/// Provenance and stage costs of one verdict. Pause lengths are in this
/// detector's 10 ms blocks; the microsecond costs come from the classifier.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct VerdictTrace {
    pub worker: u64,
    pub request: u64,
    /// Pause length when the request was submitted, and before the block in
    /// which the detector first polled its verdict.
    pub submitted_at_ms: u64,
    pub heard_at_ms: u64,
    pub snapshot_us: u64,
    pub delivery_us: u64,
    pub features_us: u64,
    pub inference_us: u64,
    pub return_us: u64,
    pub round_trip_us: u64,
}

#[derive(Clone, Copy, Debug)]
struct Heard {
    probability: f32,
    trace: VerdictTrace,
}

#[derive(Clone, Copy, Debug)]
enum Pause {
    Listening,
    /// The classifier slot is occupied; retry each block until the fallback.
    Busy,
    Waiting {
        token: Token,
        submitted: u16,
    },
    Heard(Heard),
    Unavailable(&'static str),
}

struct Confirmation {
    classifier: Box<dyn Classifier>,
    policy: ConfirmPolicy,
    /// The latest model window of this utterance: pre-roll and resumed speech.
    audio: VecDeque<i16>,
    utterance: u64,
    generation: u64,
    pause: Pause,
    pause_blocks: u16,
    /// Speech blocks heard in this utterance.
    speech_blocks: u16,
}

impl Confirmation {
    fn new(classifier: Box<dyn Classifier>, policy: ConfirmPolicy) -> Self {
        Self {
            classifier,
            policy,
            audio: VecDeque::with_capacity(WINDOW_SAMPLES),
            utterance: 0,
            generation: 0,
            pause: Pause::Listening,
            pause_blocks: 0,
            speech_blocks: 0,
        }
    }

    fn begin(&mut self, prefix: &VecDeque<ObservedAudio>) {
        self.cancel();
        for frame in prefix {
            self.retain(&frame.samples);
        }
    }

    fn cancel(&mut self) {
        // Drop queued audio and unread results; a verdict already being
        // computed is discarded when it arrives.
        self.classifier.cancel();
        self.utterance = self.utterance.wrapping_add(1);
        self.generation = 0;
        self.pause = Pause::Listening;
        self.pause_blocks = 0;
        self.speech_blocks = 0;
        self.audio.clear();
    }

    fn retain(&mut self, samples: &CaptureBlock) {
        let excess = (self.audio.len() + samples.len()).saturating_sub(WINDOW_SAMPLES);
        self.audio.drain(..excess.min(self.audio.len()));
        self.audio.extend(samples.iter().copied());
    }

    fn accept(&mut self, outcome: Outcome) {
        let Pause::Waiting { token, submitted } = self.pause else {
            return;
        };
        if outcome.token != token {
            return;
        }
        let ms = |blocks: u16| u64::from(blocks) * 10;
        self.pause = match outcome.result {
            Ok(verdict) => {
                let Verdict {
                    probability,
                    snapshot_us,
                    delivery_us,
                    features_us,
                    inference_us,
                    return_us,
                    round_trip_us,
                } = verdict;
                Pause::Heard(Heard {
                    probability,
                    trace: VerdictTrace {
                        worker: outcome.worker,
                        request: outcome.request,
                        submitted_at_ms: ms(submitted),
                        heard_at_ms: ms(self.pause_blocks),
                        snapshot_us,
                        delivery_us,
                        features_us,
                        inference_us,
                        return_us,
                        round_trip_us,
                    },
                })
            }
            Err(_) => Pause::Unavailable("failed"),
        };
    }

    /// Submits the utterance with exactly `check_blocks` of trailing pause,
    /// even when a busy slot delayed the submission.
    fn submit(&mut self) {
        let token = Token {
            utterance: self.utterance,
            generation: self.generation,
            check: 0,
        };
        let late = usize::from(self.pause_blocks - self.policy.check_blocks) * CAPTURE_SAMPLES;
        let audio = self.audio.make_contiguous();
        let snapshot = &audio[..audio.len().saturating_sub(late)];
        self.pause = match self.classifier.submit(token, snapshot) {
            Submit::Accepted => Pause::Waiting {
                token,
                submitted: self.pause_blocks,
            },
            Submit::Busy => Pause::Busy,
            Submit::Unavailable => Pause::Unavailable(self.classifier.availability().reason()),
        };
    }

    /// Never blocks: one non-blocking poll and at most one non-blocking submit.
    ///
    /// A pause is a run of consecutive low-score blocks, counted exactly like
    /// the baseline silence window, so the fallback without a usable verdict is
    /// the baseline itself. Any speech-scored block ends the pause and makes
    /// every verdict about it obsolete; the turn never ends on such a block.
    fn observe(&mut self, samples: &CaptureBlock, speech: bool) -> Option<EndpointDecision> {
        self.retain(samples);
        if let Some(outcome) = self.classifier.poll() {
            self.accept(outcome);
        }
        if speech {
            self.speech_blocks = self.speech_blocks.saturating_add(1);
            if !matches!(self.pause, Pause::Listening) {
                self.generation = self.generation.wrapping_add(1);
                self.pause = Pause::Listening;
            }
            self.pause_blocks = 0;
            return None;
        }
        self.pause_blocks = self.pause_blocks.saturating_add(1);
        let policy = self.policy;
        let pause = self.pause_blocks;
        match self.pause {
            // An utterance too short for an early verdict to count is not
            // sent to the model at all: it ends on the silence window, as
            // it would anyway. On lamp-4ace on 2026-10-14, in a talkative
            // office, the model took a whole core and the board 76 °C.
            Pause::Listening
                if pause >= policy.check_blocks
                    && self.speech_blocks >= policy.least_speech_blocks =>
            {
                self.submit()
            }
            Pause::Busy => self.submit(),
            Pause::Waiting { submitted, .. }
                if pause.saturating_sub(submitted) >= policy.result_timeout_blocks =>
            {
                self.pause = Pause::Unavailable("timeout");
            }
            _ => {}
        }
        let decide = |basis, heard: Option<Heard>, fallback| EndpointDecision {
            basis,
            silence_ms: u64::from(pause) * 10,
            probability: heard.map(|heard| heard.probability),
            fallback,
            verdict: heard.map(|heard| heard.trace),
            call_wait_ms: None,
        };
        match self.pause {
            Pause::Heard(heard)
                if heard.probability >= policy.complete_probability
                    && pause
                        >= if self.speech_blocks >= policy.least_speech_blocks {
                            policy.commit_blocks
                        } else {
                            policy.fallback_blocks
                        } =>
            {
                Some(decide("model_complete", Some(heard), None))
            }
            Pause::Heard(heard)
                if heard.probability < policy.incomplete_probability
                    && pause >= policy.incomplete_blocks =>
            {
                Some(decide("model_incomplete", Some(heard), None))
            }
            Pause::Heard(heard)
                if heard.probability >= policy.incomplete_probability
                    && heard.probability < policy.complete_probability
                    && pause >= policy.fallback_blocks =>
            {
                Some(decide("model_uncertain", Some(heard), None))
            }
            Pause::Unavailable(reason) if pause >= policy.fallback_blocks => {
                Some(decide("silence_fallback", None, Some(reason)))
            }
            Pause::Busy if pause >= policy.fallback_blocks => {
                Some(decide("silence_fallback", None, Some("busy")))
            }
            // Too short to be sent to the model: the silence window.
            Pause::Listening if pause >= policy.fallback_blocks => {
                Some(decide("silence_fallback", None, Some("short")))
            }
            Pause::Waiting { .. } if pause >= policy.fallback_blocks => {
                Some(decide("silence_fallback", None, Some("timeout")))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Stop. Please stop talking." generated with Piper's
    /// `en_US-libritts-high` voice, speaker 0 (trained on LibriTTS, CC BY 4.0,
    /// openslr.org/60; voices repository MIT), as `docs/stop-word.md` chose
    /// its generated speech. 22.05 kHz resampled to 16 kHz mono s16le.
    const SPEECH: &[u8] = include_bytes!("../fixtures/piper-stop-16k.s16");

    fn blocks() -> Vec<CaptureBlock> {
        let samples: Vec<i16> = SPEECH
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        samples
            .chunks_exact(CAPTURE_SAMPLES)
            .map(|chunk| chunk.try_into().unwrap())
            .collect()
    }

    /// The first builder's test for #828: the processed block suppressed to
    /// silence while the linear output still holds the speech scores speech
    /// with the switch and not without it.
    #[test]
    fn the_detector_hears_the_linear_output_only_with_the_switch() {
        let speech = blocks();
        let suppressed: CaptureBlock = [0; CAPTURE_SAMPLES];
        let loudest = |linear_on| {
            let mut detector = SpeechProbability::default();
            speech
                .iter()
                .map(|linear| {
                    detector.analyze(detector_input(linear_on, &suppressed, Some(linear)))
                })
                .fold(0.0_f32, f32::max)
        };
        assert!(loudest(true) >= START_PROBABILITY, "{}", loudest(true));
        assert!(loudest(false) < SILENCE_PROBABILITY, "{}", loudest(false));
        // No linear output for this block (no reference yet, or none kept):
        // the processed block, switch or not.
        for linear_on in [false, true] {
            assert!(std::ptr::eq(
                detector_input(linear_on, &suppressed, None),
                &suppressed
            ));
        }
    }
}
