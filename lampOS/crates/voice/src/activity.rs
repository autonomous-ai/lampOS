//! Local neural speech activity with finite pre-roll and explicit endpointing.
//! This detector does not identify a speaker or an addressee. Its thresholds
//! are initial experiment settings, not socially qualified release defaults.
use crate::unfinished_words::{Trailing, WAIT_BLOCKS as UNFINISHED_WAIT_BLOCKS, Waited};
use lamp_audio::{CAPTURE_RATE, CAPTURE_SAMPLES, CaptureBlock, duplex, evidence::EchoEvidence};
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

/// With first words kept, a line that starts as the lamp's sound ends begins
/// at the onset the lamp's sound hid ([`TurnDetector::reach_back_to_onset`]),
/// with `on`; off unless set (#1722, the first builder at 06:56).
///
/// Why: under the lamp's last words the duplex rule takes a person for the
/// lamp's own sound, and the line starts only once the lamp has been quiet
/// 300 ms, without what was said under it. On lamp-4ace on 20 October "Hey
/// Lamp, ask Codex to write a haiku about coffee." said as a story ended
/// came as its last 8 words, held for no name, in 3 of 4 hand-overs.
pub const REPLY_END_REACH_ENVIRONMENT: &str = "LAMP_REPLY_END_REACH";

/// Whether the voice's environment says `on`.
pub fn reply_end_reach() -> bool {
    std::env::var(REPLY_END_REACH_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// With first words kept, how far back a yield's request may begin: 1.6 s,
/// the duplex hold's longest retained age.
pub const REACH_BACK_BLOCKS: usize = 160;
/// And how much before its earliest speech-like block it begins.
pub const REACH_LEAD_BLOCKS: usize = 20;

/// Where the coordinator reads whether a line's sound begins a moment before
/// its first word (`on`; off unless set). With first words kept, an ordinary
/// start, the lamp silent, keeps [`SOFT_START_LEAD_BLOCKS`] more than the
/// usual 300 ms, never past a block heard while the lamp was sounding, and
/// never later than the usual start.
///
/// Why: a line starts on [`START_BLOCKS`] blocks scored [`START_PROBABILITY`]
/// and over the faint floor, and keeps 300 ms before that. A first word that
/// takes longer to reach both leaves the line's sound beginning at it, or
/// in it, and speech to text then drops the word. On lamp-4ace on 24 October
/// "Lamp, what did I just ask you?" (the name, 1.5 s, the question) was heard
/// as "Did I just ask you?" with the question's sound beginning 12 ms before
/// its "what", and "Lamp, what time is it in London?" as "Time is it in
/// London?" with it beginning about 100 ms into the "what"; with 171 ms
/// before the "what" the same line was heard whole.
pub const SOFT_START_ENVIRONMENT: &str = "LAMP_SOFT_START";

/// Whether the voice's environment says `on`.
pub fn soft_start() -> bool {
    std::env::var(SOFT_START_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// How much more than the usual 300 ms a soft start keeps: 300 ms, so 600
/// ms before the start's sixth block. The detector can take that long to be
/// sure of a word that is not soft at all: in 60 lines recorded on lamp-4ace
/// on 24 and 25 October a start came 58 to 458 ms after its speech began
/// ("what" at -11 dBFS within 50 ms started its line 278 ms in), and every
/// line with 100 ms or more of its sound before the word was heard whole.
/// Its own scores are no guide to where such a word began: they dip inside
/// it, so a reach along them stopped short (the first builder's A/B, 25 Oct).
pub const SOFT_START_LEAD_BLOCKS: usize = 30;

/// With `linear`, the detector reads the echo canceller's linear output in
/// place of the processed block (#828): the residual suppressor that follows
/// the canceller can take a person's "Stop." said over the lamp's own voice
/// down with the echo. Only the detector: the provider's audio, the residual
/// and the evidence keep the processed block. Off unless set.
///
/// With `linear-gated`, it reads the linear output only on a block where
/// speech would plainly be a person: the room clear of the lamp
/// (`Heard::Clear`), or someone over it that the render explains no more
/// than `plain_percent` (20 %) of (`Hearing::plain`), judged at a score of 1.
/// Elsewhere, the lamp's own voice, a block between 20 and 75 % explained, one
/// not yet judged, or no evidence, it reads the processed block, as without
/// the switch. The linear output keeps most of the lamp's voice (3.1–4.1 dB
/// taken out on lamp-4ace's mic2), and the lamp's own voice misjudged as a
/// person was 30 to 75 % explained, so a gate at the duplex rules' 75 % gave
/// `linear`'s false yields; at 20 %, replayed on the #828 tables, it gave 2
/// against `clean`'s 1 and `linear`'s 12, and `clean`'s 10 of 19 yields.
pub const INPUT_ENVIRONMENT: &str = "LAMP_VAD_INPUT";

/// What the detector reads, from `LAMP_VAD_INPUT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectorInput {
    Processed,
    Linear,
    LinearGated,
}

impl DetectorInput {
    /// This worker's, read once.
    pub fn from_environment() -> Self {
        Self::parse(std::env::var(INPUT_ENVIRONMENT).ok().as_deref())
    }

    /// Only the exact values; anything else is the processed block.
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("linear") => Self::Linear,
            Some("linear-gated") => Self::LinearGated,
            _ => Self::Processed,
        }
    }

    /// The switch's value, when it is on.
    pub fn value(self) -> Option<&'static str> {
        match self {
            Self::Processed => None,
            Self::Linear => Some("linear"),
            Self::LinearGated => Some("linear-gated"),
        }
    }

    /// Whether the canceller has to keep its linear output.
    pub fn reads_linear(self) -> bool {
        self != Self::Processed
    }
}

/// Whether `linear-gated` reads the linear output for a block with this
/// evidence, judged by `hearing` (the worker's is `Hearing::EVALUATED`):
/// speech in it would be the room clear or plainly a person over the lamp.
pub fn linear_admitted(hearing: &duplex::Hearing, evidence: Option<&EchoEvidence>) -> bool {
    evidence.is_some_and(|evidence| {
        let heard = hearing.heard(1.0, evidence);
        heard == duplex::Heard::Clear || hearing.plain(heard, evidence)
    })
}

/// The block the detector reads: the linear output when asked for, there is
/// one for this block and, with `linear-gated`, the block is `admitted`
/// ([`linear_admitted`]); the processed block otherwise.
pub fn detector_input<'a>(
    input: DetectorInput,
    admitted: bool,
    clean: &'a CaptureBlock,
    linear: Option<&'a CaptureBlock>,
) -> &'a CaptureBlock {
    match (input, linear) {
        (DetectorInput::Linear, Some(linear)) => linear,
        (DetectorInput::LinearGated, Some(linear)) if admitted => linear,
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
    /// The next start comes as the lamp's sound ends, and may reach back
    /// to the onset the lamp's sound hid.
    reach_onset: bool,
    /// With first words kept, `LAMP_SOFT_START=on`: whether each prefix
    /// block was heard with the lamp silent; a soft start never reaches back
    /// past one that was not. Empty otherwise.
    lamp_silent: VecDeque<bool>,
    soft_start: bool,
    /// Whether the last start was an ordinary one, neither a yield's
    /// reach back nor one as the lamp's sound ends ([`Self::began_ordinarily`]).
    began_ordinarily: bool,
    /// Whether the next block pushed is heard with the lamp silent.
    next_lamp_silent: bool,
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
    /// The words so far end unfinished, as the second route last told
    /// (`LAMP_SECOND_UNFINISHED_WAIT`, [`crate::unfinished_words`]).
    unfinished: Option<Trailing>,
    /// An end that was due while they did, held for [`UNFINISHED_WAIT_BLOCKS`]
    /// more of quiet.
    held_end: Option<HeldEnd>,
    /// Quiet the line in hand was already held for, in blocks: its holds
    /// add up to [`UNFINISHED_WAIT_BLOCKS`] at most (El on #3045).
    held_blocks: u16,
    /// The last held end, for the journal, until taken.
    held_told: Option<Waited>,
}

/// An end held for unfinished words: the pause when it was due, how much
/// more it may wait, the trailing word's class, and the decision then.
#[derive(Debug)]
struct HeldEnd {
    from: u16,
    most: u16,
    class: Trailing,
    decision: EndpointDecision,
}

impl Default for TurnDetector {
    fn default() -> Self {
        Self {
            prefix: VecDeque::with_capacity(PRE_ROLL_BLOCKS),
            scores: VecDeque::new(),
            first_words: false,
            reach_back: false,
            reach_onset: false,
            lamp_silent: VecDeque::new(),
            soft_start: false,
            began_ordinarily: true,
            next_lamp_silent: false,
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
            unfinished: None,
            held_end: None,
            held_blocks: 0,
            held_told: None,
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

    /// The next utterance to start comes as the lamp's sound ends
    /// (`LAMP_REPLY_END_REACH=on`, #1722): with first words kept, it begins
    /// [`REACH_LEAD_BLOCKS`] before the earliest onset of the pre-roll, a run
    /// of [`START_BLOCKS`] blocks each scored [`START_PROBABILITY`] or more by
    /// its own score: where it would have started had the lamp not been
    /// sounding. Never past a block that was not heard. Only for the next
    /// block pushed.
    pub fn reach_back_to_onset(&mut self) {
        self.reach_onset = self.first_words;
    }

    /// `LAMP_SOFT_START=on`: with first words kept, an ordinary start keeps
    /// [`SOFT_START_LEAD_BLOCKS`] more ([`SOFT_START_ENVIRONMENT`]).
    pub fn keep_soft_starts(&mut self, on: bool) {
        self.soft_start = on;
    }

    /// Whether the last utterance started was an ordinary start: neither a
    /// yield's reach back ([`Self::reach_back`]) nor one as the lamp's sound
    /// ends ([`Self::reach_back_to_onset`]). Only such a start keeps a soft
    /// start's lead (#4226).
    pub fn began_ordinarily(&self) -> bool {
        self.began_ordinarily
    }

    /// Whether the next block is heard with the lamp silent. Only for the
    /// next block pushed; unsaid, it was not.
    pub fn lamp_silent(&mut self, silent: bool) {
        self.next_lamp_silent = silent;
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

    /// The words so far of the utterance in hand end on an unfinished word
    /// of `class`, or on none (`LAMP_SECOND_UNFINISHED_WAIT`,
    /// [`crate::unfinished_words`]): while they do, an end that comes due is
    /// held for [`UNFINISHED_WAIT_BLOCKS`] more of quiet. Nothing with no
    /// utterance in hand.
    pub fn words_end(&mut self, class: Option<Trailing>) {
        if self.active {
            self.unfinished = class;
        }
    }

    /// The last end held for unfinished words, once, for the journal.
    pub fn take_unfinished_wait(&mut self) -> Option<Waited> {
        self.held_told.take()
    }

    /// `decision`, held while the words so far end unfinished: at most
    /// [`UNFINISHED_WAIT_BLOCKS`] of quiet past when it was due, and let go
    /// at once when speech comes (the line goes on) or the words are told
    /// finished after all. The bound is the line's: holds on one line add
    /// up to no more, and none takes the pause past the silence window plus
    /// it. An end the model held as incomplete has waited already (El on
    /// #3045: Smart Turn's 1.5 s plus 1.3 s). `quiet_before` is the pause
    /// before this block.
    fn hold_unfinished(
        &mut self,
        decision: Option<EndpointDecision>,
        speech: bool,
        quiet_before: u16,
    ) -> Option<EndpointDecision> {
        if let Some(held) = self.held_end.take() {
            let pause = if speech {
                quiet_before
            } else {
                self.silence_count
            };
            let waited = pause.saturating_sub(held.from);
            if !speech && waited < held.most && self.unfinished.is_some() {
                self.held_end = Some(held);
                return None;
            }
            self.held_blocks = self.held_blocks.saturating_add(waited);
            self.held_told = Some(Waited {
                class: held.class,
                waited_ms: u64::from(waited) * 10,
                went_on: speech,
            });
            if speech {
                return decision;
            }
            let mut ended = held.decision;
            ended.unfinished_wait_ms = Some(u64::from(waited) * 10);
            return Some(ended);
        }
        // What the line has left of its hold, and no quiet past the window
        // plus the hold in all.
        let most = UNFINISHED_WAIT_BLOCKS.saturating_sub(self.held_blocks).min(
            self.end_silence_blocks
                .saturating_add(UNFINISHED_WAIT_BLOCKS)
                .saturating_sub(self.silence_count),
        );
        match (decision, self.unfinished) {
            // A call's wait that ran out, or the model's incomplete wait,
            // has waited already.
            (Some(due), Some(class))
                if due.call_wait_ms.is_none() && due.basis != "model_incomplete" && most > 0 =>
            {
                self.held_end = Some(HeldEnd {
                    from: self.silence_count,
                    most,
                    class,
                    decision: due,
                });
                None
            }
            (decision, _) => decision,
        }
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
        let reach_onset = std::mem::take(&mut self.reach_onset);
        let lamp_silent = std::mem::take(&mut self.next_lamp_silent);
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
            // Words so far that can't end a request: the end waits a little
            // longer (`LAMP_SECOND_UNFINISHED_WAIT`).
            decision = self.hold_unfinished(decision, speech, quiet_before);
            if let Some(decision) = decision {
                self.active = false;
                self.waiting = false;
                self.unfinished = None;
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
            self.lamp_silent.pop_front();
        }
        self.prefix.push_back(frame);
        if self.first_words {
            self.scores.push_back(own);
            if self.soft_start {
                self.lamp_silent.push_back(lamp_silent);
            }
        }
        self.start_count = if probability >= START_PROBABILITY {
            self.start_count + 1
        } else {
            0
        };
        if self.start_count >= START_BLOCKS {
            let begin = self.begin(reach_back, reach_onset);
            self.began_ordinarily = !(reach_back || reach_onset);
            self.prefix.drain(..begin);
            self.scores.clear();
            self.lamp_silent.clear();
            self.active = true;
            self.active_blocks = self.prefix.len() as u32;
            self.silence_count = 0;
            self.began_us = self.prefix.front().map_or(0, |first| first.captured_at_us);
            self.speech_blocks = self.start_count;
            self.waiting = false;
            self.unfinished = None;
            self.held_end = None;
            self.held_blocks = 0;
            self.waited = false;
            if let Some(confirmation) = self.confirmation.as_mut() {
                confirmation.begin(&self.prefix, self.speech_blocks);
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
    /// As the lamp's sound ends (`reach_onset`), [`REACH_LEAD_BLOCKS`] before
    /// the earliest onset the same way, so the lamp's own last words, which
    /// the canceller leaves scored under [`START_PROBABILITY`], aren't taken;
    /// and only in the line's own stretch, since a pause.
    fn begin(&self, reach_back: bool, reach_onset: bool) -> usize {
        let usual = self.prefix.len().saturating_sub(PRE_ROLL_BLOCKS);
        if !(reach_back || reach_onset) {
            return self.soft_begin(usual);
        }
        if self.scores.len() != self.prefix.len() {
            return usual;
        }
        // Never past a block heard without evidence, wherever it is: every
        // block of a candidate is checked against what was heard.
        let floor = self
            .scores
            .iter()
            .rposition(Option::is_none)
            .map_or(0, |at| at + 1);
        if reach_onset && !reach_back {
            // The line's own stretch: back from the usual start to a pause
            // as long as the one that ends a line. The canceller leaves the
            // lamp's own last words scored as an onset now and then (#828's
            // replay), and words before such a pause aren't this line's
            // (the reviewer's P3 on #2096).
            let pause = usize::from(self.end_silence_blocks);
            let mut quiet = 0;
            let mut stretch = floor;
            for at in (floor..usual).rev() {
                if self.scores[at].is_some_and(|score| score >= SILENCE_PROBABILITY) {
                    quiet = 0;
                } else {
                    quiet += 1;
                    if quiet >= pause {
                        stretch = at + quiet;
                        break;
                    }
                }
            }
            let onset = usize::from(START_BLOCKS);
            let mut run = 0;
            for (at, score) in self.scores.iter().enumerate().take(usual).skip(stretch) {
                if score.is_some_and(|score| score >= START_PROBABILITY) {
                    run += 1;
                    if run == onset {
                        return (at + 1 - onset)
                            .saturating_sub(REACH_LEAD_BLOCKS)
                            .max(floor)
                            .min(usual);
                    }
                } else {
                    run = 0;
                }
            }
            return usual;
        }
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

    /// An ordinary start with soft starts kept: [`SOFT_START_LEAD_BLOCKS`]
    /// before the usual start, never past a block heard while the lamp was
    /// sounding, and never later than the usual start: a block so heard at
    /// or after it (the onset's own, under a reply not yet over) leaves the
    /// start as it was, never with less (the reviewer's P2 on #3392: `Start`
    /// with no audio). Without the switch, the usual start.
    fn soft_begin(&self, usual: usize) -> usize {
        if !self.soft_start || self.lamp_silent.len() != self.prefix.len() {
            return usual;
        }
        let floor = self
            .lamp_silent
            .iter()
            .rposition(|silent| !silent)
            .map_or(0, |at| at + 1)
            .min(usual);
        usual.saturating_sub(SOFT_START_LEAD_BLOCKS).max(floor)
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
        self.lamp_silent.clear();
        self.start_count = 0;
        self.silence_count = 0;
        self.active_blocks = 0;
        self.active = false;
        self.abandoned = false;
        self.waiting = false;
        self.unfinished = None;
        self.held_end = None;
        self.cancel_confirmation();
    }

    pub fn reset(&mut self) {
        self.prefix.clear();
        self.scores.clear();
        self.lamp_silent.clear();
        self.start_count = 0;
        self.silence_count = 0;
        self.active_blocks = 0;
        self.active = false;
        self.last = None;
        self.abandoned = false;
        self.ignore_through_us = None;
        self.await_pause = false;
        self.waiting = false;
        self.unfinished = None;
        self.held_end = None;
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
            self.lamp_silent.clear();
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
        self.unfinished = None;
        self.held_end = None;
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
    /// A line under `least_speech_blocks` but with at least this many
    /// speech blocks is sent to the model as well (`LAMP_ENDPOINT_SHORT_VERDICT`,
    /// #2135). Its verdict ends it no sooner: a complete one waits for
    /// `fallback_blocks` as above, an uncertain one ends there too, and only
    /// a clearly incomplete one holds it to `incomplete_blocks`. On lamp-4ace
    /// on 21 Oct 71 of 635 Ends (11%) were such lines, never asked, ended on
    /// the silence window, among them "Hey Lamp, my name is" cut 0.66 s in
    /// (the first builder on #1722 at 04:56). 0: they are not sent, as on
    /// 14 Oct, when in a talkative office the model took a whole core.
    pub short_verdict_blocks: u16,
    /// An uncertain verdict is asked again at once, with the pause so far,
    /// and a complete second ends the line when it comes, no sooner than
    /// the commit; any other answer, or none, leaves the first's, so the
    /// line ends no later than without it (#2135, the first builder on #1722
    /// at 07:16: of 23 uncertain Ends on lamp-4ace, 12 were 0.745 to 0.888,
    /// and none was followed by speech within 1 s).
    pub recheck_uncertain: bool,
    /// The utterance's start counts toward `least_speech_blocks`: the
    /// confirmation's count begins at the speech blocks the detector already
    /// heard to start the line (its [`START_BLOCKS`] run), not at 0
    /// (`LAMP_ENDPOINT_LEAST_SPEECH_MS`, the first builder on #2602, 22 Oct
    /// 04:28). Without it the six start blocks never count, so a line needs
    /// 0.86 s of speech, not the floor's 0.8 s, to be asked. Absent from the
    /// policy's record when off, so that record is as before.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub count_start: bool,
    /// An uncertain verdict ends the line at this pause, when it is longer
    /// than the silence window (`fallback_blocks`), in place of the window
    /// (`LAMP_ENDPOINT_UNSURE_MS`, the first builder's night A/B on
    /// lamp-4ace, 22 Oct: of 110 Ends, 36 were uncertain verdicts ended at
    /// the 600 ms window, 17 of them at p 0.2 or under, while people's lines
    /// split at natural pauses of 0.6 to 1.4 s). 0: at the window, as before.
    /// Absent from the policy's record when 0, so that record is as before.
    #[serde(skip_serializing_if = "is_zero")]
    pub uncertain_blocks: u16,
}

/// The probabilities `LAMP_ENDPOINT_UNFINISHED_BELOW` takes for
/// [`ConfirmPolicy::incomplete_probability`] (the first builder's night A/B
/// on lamp-4ace, 22 Oct), and `turn_end_replay --unfinished-below` with it.
pub const UNFINISHED_BELOW_RANGE: std::ops::RangeInclusive<f32> = 0.01..=0.6;

/// The waits `LAMP_ENDPOINT_UNSURE_MS` takes for
/// [`ConfirmPolicy::uncertain_blocks`], in ms: from the lamp's window to the
/// unfinished window. `turn_end_replay --unsure-ms` takes the same.
pub const UNSURE_RANGE_MS: std::ops::RangeInclusive<u16> = 600..=1500;

/// The probability `value` asks for, if it is one in
/// [`UNFINISHED_BELOW_RANGE`]: the voice's read of
/// `LAMP_ENDPOINT_UNFINISHED_BELOW`, after trimming, shared with the replay.
pub fn unfinished_below(value: &str) -> Option<f32> {
    value
        .parse::<f32>()
        .ok()
        .filter(|probability| UNFINISHED_BELOW_RANGE.contains(probability))
}

/// The wait in ms `value` asks for, if it is a whole number in
/// [`UNSURE_RANGE_MS`] and a multiple of 10: the voice's read of
/// `LAMP_ENDPOINT_UNSURE_MS`, after trimming, shared with the replay.
pub fn unsure_ms(value: &str) -> Option<u16> {
    value
        .parse::<u16>()
        .ok()
        .filter(|ms| UNSURE_RANGE_MS.contains(ms) && ms % 10 == 0)
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
            short_verdict_blocks: 0,
            recheck_uncertain: false,
            count_start: false,
            uncertain_blocks: 0,
        }
    }
}

impl ConfirmPolicy {
    /// This policy with the night's dials, as the voice's switches and
    /// `turn_end_replay` both set them: a probability from
    /// [`unfinished_below`] for `incomplete_probability`, and a wait from
    /// [`unsure_ms`] for `uncertain_blocks`. `None` keeps each as it is.
    pub fn with_dials(mut self, unfinished_below: Option<f32>, unsure_ms: Option<u16>) -> Self {
        if let Some(probability) = unfinished_below {
            self.incomplete_probability = probability;
        }
        if let Some(ms) = unsure_ms {
            self.uncertain_blocks = ms / 10;
        }
        self
    }

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

    /// The commit's floor at `ms` of speech, the line's start counted toward
    /// it (`LAMP_ENDPOINT_LEAST_SPEECH_MS`, the first builder on #2602, 22 Oct
    /// 04:28), and the short lines' floor kept under it. The switch takes
    /// only 200 ms up to the profile's own floor; this is what it then sets.
    pub fn with_floor_ms(self, ms: u16) -> Self {
        let least = ms / 10;
        Self {
            least_speech_blocks: least,
            short_verdict_blocks: self.short_verdict_blocks.min(least),
            count_start: true,
            ..self
        }
    }

    /// The silence an uncertain verdict waits before the line ends:
    /// [`Self::uncertain_blocks`] when longer than the window, else the
    /// window, as before.
    pub fn uncertain_wait_blocks(&self) -> u16 {
        self.uncertain_blocks.max(self.fallback_blocks)
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
            && self.incomplete_probability <= self.complete_probability
            && self.short_verdict_blocks <= self.least_speech_blocks
            && self.uncertain_blocks <= self.incomplete_blocks;
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
    /// How many times the line was sent to the model under the commit's
    /// floor (`ConfirmPolicy::short_verdict_blocks`): the inferences the
    /// switch adds, one a pause, so a line that pauses twice before the
    /// floor counts two (the reviewer's P3 on #2380). Absent when none.
    #[serde(skip_serializing_if = "is_zero")]
    pub short_asks: u16,
    /// The utterance's speech as the confirmation counted it at the
    /// decision (its speech blocks, 10 ms each; with
    /// [`ConfirmPolicy::count_start`] its start's too): what the commit's
    /// floor (`least_speech_blocks`) was held against, so each End says how
    /// far under it a `silence_fallback: short` line was (the first builder
    /// on #2602, 22 Oct 04:28: on lamp-4ace a third of whole played day lines
    /// ended that way, at 600 ms, not the 400 ms commit). Absent for a
    /// decision the confirmation didn't make.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speech_ms: Option<u64>,
    /// The first verdict's probability, when a second asked of an uncertain
    /// one ended the line (`ConfirmPolicy::recheck_uncertain`). Absent
    /// otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rechecked_from: Option<f32>,
    /// The quiet waited past the end that was due, while the words so far
    /// ended unfinished (`LAMP_SECOND_UNFINISHED_WAIT`). Absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unfinished_wait_ms: Option<u64>,
}

fn is_zero(count: &u16) -> bool {
    *count == 0
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
            short_asks: 0,
            speech_ms: None,
            rechecked_from: None,
            unfinished_wait_ms: None,
        }
    }

    /// The quiet waited before this end, in ms: the pause when it was due,
    /// plus any hold for unfinished words after it (the Reviewer-lead on
    /// #3045: a held end keeps its `silence_ms` from when it was due).
    pub fn quiet_ms(&self) -> u64 {
        self.silence_ms + self.unfinished_wait_ms.unwrap_or(0)
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
    /// How many times this utterance was sent under the commit's floor
    /// ([`ConfirmPolicy::short_verdict_blocks`]).
    short_asks: u16,
    /// This pause's first verdict while a second is asked of it
    /// ([`ConfirmPolicy::recheck_uncertain`]); whether the pause has had its
    /// second; and the first's probability when the second's complete stood.
    first: Option<Heard>,
    rechecked: bool,
    rechecked_from: Option<f32>,
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
            short_asks: 0,
            first: None,
            rechecked: false,
            rechecked_from: None,
        }
    }

    /// A new utterance, begun with `prefix`; `start_speech` is the speech
    /// blocks the detector counted to start it, which count toward the
    /// floor only with [`ConfirmPolicy::count_start`].
    fn begin(&mut self, prefix: &VecDeque<ObservedAudio>, start_speech: u16) {
        self.cancel();
        if self.policy.count_start {
            self.speech_blocks = start_speech;
        }
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
        self.short_asks = 0;
        self.forget_recheck();
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

    /// No second verdict for this pause, nor any held for it.
    fn forget_recheck(&mut self) {
        self.first = None;
        self.rechecked = false;
        self.rechecked_from = None;
    }

    /// Submits the utterance with exactly `check_blocks` of trailing pause,
    /// even when a busy slot delayed the submission.
    fn submit(&mut self) {
        self.submit_as(0, self.policy.check_blocks);
    }

    /// Submits it as check `check` with `trailing` blocks of the pause: a
    /// second verdict sends the whole pause so far.
    fn submit_as(&mut self, check: u8, trailing: u16) {
        let token = Token {
            utterance: self.utterance,
            generation: self.generation,
            check,
        };
        let late =
            usize::from(self.pause_blocks - trailing.min(self.pause_blocks)) * CAPTURE_SAMPLES;
        let audio = self.audio.make_contiguous();
        let snapshot = &audio[..audio.len().saturating_sub(late)];
        self.pause = match self.classifier.submit(token, snapshot) {
            Submit::Accepted => {
                // An inference the switch adds: taken by the worker, under
                // the floor (a busy slot's retry is counted once, here).
                if self.speech_blocks < self.policy.least_speech_blocks {
                    self.short_asks = self.short_asks.saturating_add(1);
                }
                Pause::Waiting {
                    token,
                    submitted: self.pause_blocks,
                }
            }
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
        // A second verdict's answer: a complete one stands; anything else
        // gives the first back, so the line ends no later than it would have.
        if let Some(first) = self.first {
            match self.pause {
                Pause::Heard(heard) if heard.probability >= self.policy.complete_probability => {
                    self.first = None;
                    self.rechecked_from = Some(first.probability);
                }
                Pause::Heard(_) | Pause::Unavailable(_) | Pause::Busy => {
                    self.pause = Pause::Heard(first);
                    self.first = None;
                }
                Pause::Listening | Pause::Waiting { .. } => {}
            }
        }
        if speech {
            self.speech_blocks = self.speech_blocks.saturating_add(1);
            if !matches!(self.pause, Pause::Listening) {
                self.generation = self.generation.wrapping_add(1);
                self.pause = Pause::Listening;
            }
            self.forget_recheck();
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
                    && (self.speech_blocks >= policy.least_speech_blocks
                        || (policy.short_verdict_blocks > 0
                            && self.speech_blocks >= policy.short_verdict_blocks)) =>
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
        // An uncertain verdict, asked again at once with the pause so far
        // (`ConfirmPolicy::recheck_uncertain`). Not taken by the worker: the
        // first stands.
        if policy.recheck_uncertain
            && !self.rechecked
            && let Pause::Heard(heard) = self.pause
            && heard.probability >= policy.incomplete_probability
            && heard.probability < policy.complete_probability
        {
            self.rechecked = true;
            self.first = Some(heard);
            self.submit_as(1, pause);
            if !matches!(self.pause, Pause::Waiting { .. }) {
                self.pause = Pause::Heard(heard);
                self.first = None;
            }
        }
        let short_asks = self.short_asks;
        let speech_ms = Some(u64::from(self.speech_blocks) * 10);
        let (first, rechecked_from) = (self.first, self.rechecked_from);
        let decide = |basis, heard: Option<Heard>, fallback| EndpointDecision {
            basis,
            silence_ms: u64::from(pause) * 10,
            probability: heard.map(|heard| heard.probability),
            fallback,
            verdict: heard.map(|heard| heard.trace),
            call_wait_ms: None,
            short_asks,
            speech_ms,
            rechecked_from,
            unfinished_wait_ms: None,
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
            // Uncertain: the silence window, or the longer wait asked for an
            // uncertain verdict (`ConfirmPolicy::uncertain_blocks`).
            Pause::Heard(heard)
                if heard.probability >= policy.incomplete_probability
                    && heard.probability < policy.complete_probability
                    && pause >= policy.uncertain_wait_blocks() =>
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
            // A second verdict still out at the window: the first's, as without it.
            Pause::Waiting { .. } if pause >= policy.uncertain_wait_blocks() && first.is_some() => {
                Some(decide("model_uncertain", first, None))
            }
            Pause::Waiting { .. } if pause >= policy.fallback_blocks && first.is_none() => {
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

    /// #3126 (`LAMP_SECOND_UNFINISHED_WAIT`): words so far ending on a
    /// filler ("…eggs, um") hold the end that came due, and speech in the
    /// hold ("and some bread") goes on with the same line: one Start, one
    /// End, after the rest. The journal tells the filler class.
    #[test]
    fn a_filler_ended_line_is_held_and_joined_with_what_follows() {
        let mut detector = TurnDetector::default();
        let mut sequence = 0;
        let mut push = |detector: &mut TurnDetector, probability: f32| {
            sequence += 1;
            detector.push(
                ObservedAudio {
                    sequence,
                    captured_at_us: sequence * 10_000,
                    samples: [0; CAPTURE_SAMPLES],
                },
                probability,
            )
        };
        // How many Starts and Ends so far.
        let mut seen = (0, 0);
        let count = |activity: Activity, seen: &mut (u32, u32)| match activity {
            Activity::Start(_) => seen.0 += 1,
            Activity::End(_) => seen.1 += 1,
            _ => {}
        };
        // "I need eggs, um", then 1 s of quiet: the 0.6 s window is past.
        for _ in 0..50 {
            count(push(&mut detector, 1.0), &mut seen);
        }
        detector.words_end(Some(Trailing::Filler));
        for _ in 0..100 {
            count(push(&mut detector, 0.0), &mut seen);
        }
        // "and some bread": the same line, the hold told.
        for _ in 0..50 {
            count(push(&mut detector, 1.0), &mut seen);
        }
        let waited = detector.take_unfinished_wait().expect("the end was held");
        assert_eq!(
            waited.told(Some(1)),
            "lamp-voice: request 1: unfinished words (filler) held its end 400 ms, more speech came: yes"
        );
        assert_eq!(seen, (1, 0));
        detector.words_end(None);
        for _ in 0..END_SILENCE_BLOCKS {
            count(push(&mut detector, 0.0), &mut seen);
        }
        assert_eq!(seen, (1, 1));
    }

    /// The first builder's test for #828: the processed block suppressed to
    /// silence while the linear output still holds the speech scores speech
    /// with the switch and not without it.
    #[test]
    fn the_detector_hears_the_linear_output_only_with_the_switch() {
        let speech = blocks();
        let suppressed: CaptureBlock = [0; CAPTURE_SAMPLES];
        let loudest = |input| {
            let mut detector = SpeechProbability::default();
            speech
                .iter()
                .map(|linear| {
                    detector.analyze(detector_input(input, false, &suppressed, Some(linear)))
                })
                .fold(0.0_f32, f32::max)
        };
        let linear = loudest(DetectorInput::Linear);
        let processed = loudest(DetectorInput::Processed);
        assert!(linear >= START_PROBABILITY, "{linear}");
        assert!(processed < SILENCE_PROBABILITY, "{processed}");
        // No linear output for this block (no reference yet, or none kept):
        // the processed block, switch or not.
        for input in [
            DetectorInput::Processed,
            DetectorInput::Linear,
            DetectorInput::LinearGated,
        ] {
            assert!(std::ptr::eq(
                detector_input(input, false, &suppressed, None),
                &suppressed
            ));
        }
    }

    /// Evidence for a block while the lamp speaks: `explained` percent of the
    /// capture is the render's, with a fresh alignment; or the room clear of
    /// the lamp.
    fn evidence(explained: Option<u8>) -> EchoEvidence {
        use lamp_audio::evidence::{EchoMatch, Level};
        EchoEvidence {
            blocks: 1_000,
            render_active_blocks: if explained.is_some() { 50 } else { 0 },
            render_quiet_blocks: if explained.is_some() { 0 } else { 50 },
            capture_level: Level::from_tenths_of_db(-130),
            residual_level: Level::from_tenths_of_db(-570),
            echo: explained.map(|capture_explained| EchoMatch {
                render_level: Level::from_tenths_of_db(-180),
                capture_explained,
                residual_explained: 10,
                residual_playback_floor: 0,
                alignment_age_blocks: 0,
                render_block_level: Level::FLOOR,
                render_drop: 0,
            }),
            silent_at_echo: None,
        }
    }

    /// `linear-gated` (#828, the stop rule's false side): the linear output
    /// is read where speech would plainly be a person (the room clear, or at
    /// most 20 % explained by the render), and the processed block on the
    /// lamp's own voice, a block over the lamp but not plainly a person, one
    /// not yet judged or one with no evidence. Values other than the exact
    /// two are off.
    #[test]
    fn linear_gated_reads_the_linear_output_only_where_speech_would_be_a_person() {
        let admitted = |evidence: Option<EchoEvidence>| {
            linear_admitted(&duplex::Hearing::EVALUATED, evidence.as_ref())
        };
        assert!(admitted(Some(evidence(None))));
        assert!(admitted(Some(evidence(Some(10)))));
        assert!(admitted(Some(evidence(Some(20)))));
        // Over the lamp but not plainly: the lamp's own voice misjudged sat
        // at 30 to 75 % on lamp-4ace.
        assert!(!admitted(Some(evidence(Some(21)))));
        assert!(!admitted(Some(evidence(Some(75)))));
        assert!(!admitted(Some(evidence(Some(95)))));
        assert!(!admitted(None));
        // A replay's own judge moves the gate with it.
        let wider = duplex::Hearing {
            plain_percent: 40,
            ..duplex::Hearing::EVALUATED
        };
        assert!(linear_admitted(&wider, Some(&evidence(Some(40)))));
        let no_match = EchoEvidence {
            echo: None,
            ..evidence(Some(95))
        };
        assert!(!admitted(Some(no_match)), "not yet judged");
        let (clean, linear) = ([0; CAPTURE_SAMPLES], [1; CAPTURE_SAMPLES]);
        let over = evidence(Some(10));
        let own = evidence(Some(95));
        let read = |input, evidence: &EchoEvidence| {
            std::ptr::eq(
                detector_input(
                    input,
                    linear_admitted(&duplex::Hearing::EVALUATED, Some(evidence)),
                    &clean,
                    Some(&linear),
                ),
                &linear,
            )
        };
        assert!(read(DetectorInput::LinearGated, &over));
        assert!(!read(DetectorInput::LinearGated, &own));
        assert!(read(DetectorInput::Linear, &own));
        assert!(!read(DetectorInput::Processed, &over));
        for (value, input) in [
            (Some("linear"), DetectorInput::Linear),
            (Some("linear-gated"), DetectorInput::LinearGated),
            (Some("Linear"), DetectorInput::Processed),
            (Some("on"), DetectorInput::Processed),
            (Some(""), DetectorInput::Processed),
            (None, DetectorInput::Processed),
        ] {
            assert_eq!(DetectorInput::parse(value), input, "{value:?}");
        }
        assert!(!DetectorInput::Processed.reads_linear());
        assert!(DetectorInput::LinearGated.reads_linear());
    }

    /// The lamp's own voice kept in the linear output while the suppressor
    /// takes it out of the processed block: `linear` scores it as speech,
    /// `linear-gated` as the processed block does, also where the render
    /// explains only half of it. A stop said plainly over the lamp (the
    /// render explains 10 %) is scored on the linear output, the same as
    /// `linear`.
    #[test]
    fn linear_gated_keeps_the_lamps_own_voice_out_and_a_stop_over_it_in() {
        let speech = blocks();
        let suppressed: CaptureBlock = [0; CAPTURE_SAMPLES];
        let scores = |input, evidence: &EchoEvidence| {
            let mut detector = SpeechProbability::default();
            speech
                .iter()
                .map(|linear| {
                    detector.analyze(detector_input(
                        input,
                        linear_admitted(&duplex::Hearing::EVALUATED, Some(evidence)),
                        &suppressed,
                        Some(linear),
                    ))
                })
                .collect::<Vec<f32>>()
        };
        let loudest = |scores: Vec<f32>| scores.into_iter().fold(0.0_f32, f32::max);
        // The lamp alone, and the lamp's own voice misjudged as a person
        // (between 20 and 75 % explained): as the processed block.
        for explained in [95, 50] {
            let own = evidence(Some(explained));
            let linear = loudest(scores(DetectorInput::Linear, &own));
            let gated = loudest(scores(DetectorInput::LinearGated, &own));
            assert!(linear >= START_PROBABILITY, "{explained}: {linear}");
            assert!(gated < SILENCE_PROBABILITY, "{explained}: {gated}");
        }
        // Plainly a person over the lamp, and the room clear: block for
        // block the same scores as `linear`.
        for evidence in [evidence(Some(10)), evidence(None)] {
            assert_eq!(
                scores(DetectorInput::LinearGated, &evidence),
                scores(DetectorInput::Linear, &evidence)
            );
        }
    }

    /// The night's dials (`LAMP_ENDPOINT_UNFINISHED_BELOW`,
    /// `LAMP_ENDPOINT_UNSURE_MS`, `turn_end_replay`'s two flags) take only
    /// their ranges, and `with_dials` sets exactly the two fields: none
    /// leaves the policy as it was, as the replay's "before" is.
    #[test]
    fn the_night_dials_take_only_their_ranges_and_set_two_fields() {
        for (value, taken) in [("0.01", 0.01), ("0.3", 0.3), ("0.6", 0.6)] {
            assert_eq!(unfinished_below(value), Some(taken), "{value}");
        }
        for value in [
            "0", "0.009", "0.61", "0.7", "1", "-0.2", "NaN", "on", " 0.3",
        ] {
            assert_eq!(unfinished_below(value), None, "{value}");
        }
        for (value, taken) in [("600", 600), ("900", 900), ("1500", 1500)] {
            assert_eq!(unsure_ms(value), Some(taken), "{value}");
        }
        for value in ["599", "605", "1510", "1.2", "900ms", "-600", "on", " 900"] {
            assert_eq!(unsure_ms(value), None, "{value}");
        }
        let policy = ConfirmPolicy::default();
        assert_eq!(policy.with_dials(None, None), policy);
        let dialed = policy.with_dials(Some(0.3), Some(900));
        assert_eq!(
            dialed,
            ConfirmPolicy {
                incomplete_probability: 0.3,
                uncertain_blocks: 90,
                ..policy
            }
        );
        assert!(dialed.validate().is_ok());
    }
}
