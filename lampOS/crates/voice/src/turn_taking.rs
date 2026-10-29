//! Opt-in, host-read-domain turn taking. This is not acoustic echo detection.
//! Positive speaker acceptance opens a mask; actual cursor retirement closes it.
//! This module never grants authority, completes replies, or changes DSP state.
//!
//! The duplex mask is the same mask with ears. While it is given echo
//! evidence for every capture block it lets blocks through during the lamp's
//! own output, scored so that the detector can start on a person and not on
//! the lamp, and it holds a candidate that began over the lamp until the
//! lamp's own sound has gone and speech is still there. Without evidence for
//! a block it is the conservative mask again, block by block.
use crate::{
    activity::{ObservedAudio, SILENCE_PROBABILITY, START_BLOCKS},
    admission::Candidate,
    options::TurnTaking,
};
use lamp_audio::{
    duplex::{Confirmation, Heard, Hearing, Interruption, Margin, Step},
    evidence::{EchoEvidence, MATCH_PERCENT},
    ownership::{PlaybackCursor, QueuedRange},
};
use lamp_turn_owner::TurnOwner;
use std::{collections::VecDeque, io};

pub const TAIL_US: u64 = 100_000;
/// How long a block's read is kept for a retained prefix's decision: one
/// decided later is refused (`RetainedHistoryLoss::Expired`).
pub(crate) const HISTORY_US: u64 = 600_000;
const MAX_INTERVALS: usize = 16;
const MAX_READS: usize = 80;
/// Blocks whose hearing is remembered: longer than any candidate is held.
const MAX_HEARD: usize = 128;
/// The same with a yield's first words kept (#828): 1.6 s before the
/// onset, the duplex hold's 1.2 s after it, and room to spare.
const FIRST_WORDS_HEARD: usize = 320;
/// The same for a sound that waits while an answer is still to come
/// (#1123): its pre-roll (`activity::PRE_ROLL_BLOCKS`, 30), the duplex
/// hold's 1.2 s (120), and room to spare. At its decision, 1 s after its
/// trigger, it holds about 130 blocks, which [`MAX_HEARD`] would not (the
/// reviewer and the first builder on #1130).
const WAITED_HEARD: usize = 192;
/// Yields in one reply that came to nothing before the mask stops yielding
/// for the rest of that reply and masks as the conservative baseline does,
/// until one is forgiven ([`FORGIVE_FRUITLESS_US`]).
pub const FRUITLESS_YIELDS: u8 = 2;
/// How long a reply goes with no yield before one fruitless yield is
/// forgiven, so at most [`FRUITLESS_YIELDS`] count in any 20 s. A long story
/// that yielded twice to nothing went deaf for its rest: on lamp-4ace on
/// 20 Oct at 03:45 a Hindi line and "What was that?" each yielded and
/// resumed, and "Hey Lamp, what's the weather in Hanoi?" 30 s later made no
/// request (#1712, the first builder at 04:11 and their yes at 04:40). A
/// noise that trips it more often than this still falls back as before.
pub const FORGIVE_FRUITLESS_US: u64 = 20_000_000;
/// With `LAMP_CUT_IN_KEEPS_LISTENING=on`, a reply fallen back after its
/// fruitless yields still yields to a voice over it that keeps going: this
/// many blocks over the lamp in a row (300 ms), then the yield's own onset
/// (about 100 ms more). A person who tries a third time within
/// [`FORGIVE_FRUITLESS_US`] is not talked over for the rest of the story:
/// on the Mac's replay on 22 Oct a line 3 to 19 s after two asides made no
/// yield and no candidate (#2602, "Cutting in"). A cough or a word is
/// shorter than this and is still not let through.
pub const KEEPS_TRYING_BLOCKS: u16 = 30;
/// How many more yields that came to nothing such a voice may cost a reply,
/// before [`FORGIVE_FRUITLESS_US`] gives one back: a television that keeps
/// talking over a story pauses it at most this many times more.
pub const KEEPS_TRYING_YIELDS: u8 = 2;
/// The voice's environment switch for [`KEEPS_TRYING_BLOCKS`], with `on`;
/// off unless set.
pub const CUT_IN_KEEPS_LISTENING_ENVIRONMENT: &str = "LAMP_CUT_IN_KEEPS_LISTENING";

/// Whether the voice's environment says `on` for
/// [`CUT_IN_KEEPS_LISTENING_ENVIRONMENT`].
pub fn cut_in_keeps_listening() -> bool {
    std::env::var(CUT_IN_KEEPS_LISTENING_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on")
}
/// With `LAMP_CUT_IN_COUNTS_REAL=on`, resumes whose window did not point at
/// the lamp's own voice ([`YieldEvidence`]) before the mask stops yielding
/// for the rest of the reply, as [`FRUITLESS_YIELDS`] does, until
/// [`FORGIVE_FRUITLESS_US`] gives one back. Two real asides leave a third
/// line heard; a television, a radio or someone else's short words pause a
/// story at most three times in 20 s (the Reviewer-lead's P2 on #2779).
pub const OTHER_FRUITLESS_YIELDS: u8 = 3;
/// The voice's environment switch, with `on`, for counting a resume toward
/// [`FRUITLESS_YIELDS`] only when its yield's evidence window points at the
/// lamp's own voice ([`YieldEvidence`], #2602 "Cutting in"); off unless set.
pub const CUT_IN_COUNTS_REAL_ENVIRONMENT: &str = "LAMP_CUT_IN_COUNTS_REAL";

/// Whether the voice's environment says `on` for
/// [`CUT_IN_COUNTS_REAL_ENVIRONMENT`].
pub fn cut_in_counts_real() -> bool {
    std::env::var(CUT_IN_COUNTS_REAL_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on")
}

/// With `LAMP_CUT_IN_NEAR_ASKER=on` (#3967), talk heard in the clear in a
/// yield commits only if its middle level is no more than this under the
/// level of the person the lamp is answering (`asker_first`'s middle of
/// their own line): 6 dB, `asker_first::MARGIN`. Quieter, it is the room
/// behind them, and the yield resumes ([`Interruption::quiet_talk`]).
pub const NEAR_ASKER_DB: f32 = 6.0;
/// Blocks of talk in the clear a yield needs before it is judged against
/// the asker: as many as commit it (`Confirmation::speech_blocks`).
pub const NEAR_ASKER_BLOCKS: usize = 6;
/// The voice's environment switch, with `on`, for [`NEAR_ASKER_DB`]; off
/// unless set.
pub const CUT_IN_NEAR_ASKER_ENVIRONMENT: &str = "LAMP_CUT_IN_NEAR_ASKER";

/// Whether the voice's environment says `on` for
/// [`CUT_IN_NEAR_ASKER_ENVIRONMENT`].
pub fn cut_in_near_asker() -> bool {
    std::env::var(CUT_IN_NEAR_ASKER_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// What one yield's evidence window held, with `LAMP_CUT_IN_COUNTS_REAL=on`.
/// The window is bounded: the consecutive `Over` blocks that made the yield
/// (its onset run, 7 or 10 blocks) and every block heard while the yield is
/// in force, through the block that settles it; at most 10 + 110 blocks
/// (the resume deadline), 1.2 s.
///
/// - The lamp's own voice: a block heard `Own`, or `Over` and not plain
///   (the render explains more than `plain_percent` of it, as it did of the
///   lamp's own voice misjudged on lamp-4ace).
/// - A person: a block heard `Over` and plain, or `Clear`.
/// - Nothing either way: `Unknown`, `Under`, `Still` and a block without
///   evidence. `Unknown` says nothing about who spoke, and words that never
///   came are no evidence either (Third on #2602, 08:13).
///
/// A resume counts as fruitless only when the lamp's own voice outnumbers a
/// person in the window. A yield with no evidence either way, or a tie, is
/// not one: nothing in it showed the lamp's own voice made it yield. Such a
/// resume counts on a bound of its own instead ([`OTHER_FRUITLESS_YIELDS`]),
/// so a sound that is never a line still can't pause a reply without limit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct YieldEvidence {
    own: u16,
    person: u16,
}
impl YieldEvidence {
    fn add(&mut self, heard: Heard, plain: bool) {
        match heard {
            Heard::Own => self.own = self.own.saturating_add(1),
            Heard::Over if !plain => self.own = self.own.saturating_add(1),
            Heard::Over | Heard::Clear => self.person = self.person.saturating_add(1),
            Heard::Still | Heard::Under | Heard::Unknown => {}
        }
    }
    fn own_voice(&self) -> bool {
        self.own > self.person
    }
}

/// Blocks of the lamp's own sound that the render must have explained, since
/// the lamp last had nothing to say or last went on after a pause, before a
/// block can be a person over it. The evidence takes the start of every
/// reply to find the lamp's sound again. On lamp-4ace it explained the first
/// two blocks of one, then none of the next twenty, and the lamp took its
/// own "I'm doing great!" for someone cutting in.
pub const MATCHED_BLOCKS: u16 = 15;
/// Hearing older than this withholds nothing. Capture blocks arrive every
/// 10 ms; a muted or stalled microphone must not hold a reply back.
pub const HEARING_STALE_US: u64 = 100_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Disposition {
    Pass,
    Suppress,
    AwaitWriteEvidence,
}

/// Loss of previously observed read history, never permission to use the PCM.
/// Invalid current input, an unknown identity or a cached timestamp mismatch is
/// still a different error. Eviction cannot reconstruct an old exact interval.
#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RetainedHistoryLoss {
    Expired,
    Evicted,
}
impl std::fmt::Display for RetainedHistoryLoss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Expired => "retained prefix original read interval expired",
            Self::Evicted => "retained prefix original read interval evicted",
        })
    }
}
impl std::error::Error for RetainedHistoryLoss {}

/// Constant-size evidence that this lineage actually retired these reads.
/// This is not a second read history and cannot certify an exact evicted stamp.
#[derive(Clone, Copy)]
struct RetiredReads {
    first_sequence: u64,
    first_completed: u64,
    last_sequence: u64,
    last_completed: u64,
}
impl RetiredReads {
    fn covers(&self, frame: &ObservedAudio) -> bool {
        (self.first_sequence..=self.last_sequence).contains(&frame.sequence)
            && (self.first_completed..=self.last_completed).contains(&frame.captured_at_us)
            && (frame.sequence != self.first_sequence
                || frame.captured_at_us == self.first_completed)
            && (frame.sequence != self.last_sequence || frame.captured_at_us == self.last_completed)
    }
}

#[derive(Clone, Copy)]
struct Read {
    sequence: u64,
    first: u64,
    completed: u64,
}
#[derive(Clone, Copy)]
struct Dispatch {
    owner: TurnOwner,
    sequence: u64,
    at_us: u64,
    frames: usize,
    accepted: usize,
    end: Option<PlaybackCursor>,
}
#[derive(Clone, Copy)]
struct Span {
    owner: TurnOwner,
    start: u64,
    end: Option<u64>,
    sequence: u64,
    cursor: PlaybackCursor,
    accepted_at: u64,
}

/// What became of the block that made the lamp yield.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Yielded {
    Undecided,
    Committed,
    Resumed,
}

struct Duplex {
    hearing: Hearing,
    interruption: Interruption,
    margin: Margin,
    heard: VecDeque<(u64, Heard)>,
    /// How many blocks' hearing is kept: [`MAX_HEARD`] or
    /// [`FIRST_WORDS_HEARD`].
    max_heard: usize,
    last_yield: Option<(u64, Yielded)>,
    /// The latest block came without evidence: mask it conservatively.
    blind: bool,
    /// Yields since the reply began that ended in a resume, less those
    /// forgiven ([`FORGIVE_FRUITLESS_US`]).
    fruitless: u8,
    /// With `LAMP_CUT_IN_COUNTS_REAL=on`, resumes that did not count in
    /// [`Self::fruitless`], less those forgiven ([`OTHER_FRUITLESS_YIELDS`]).
    other_fruitless: u8,
    /// When the latest yield began, or the latest fruitless yield was
    /// forgiven: the next is forgiven [`FORGIVE_FRUITLESS_US`] after it.
    last_yield_at_us: Option<u64>,
    /// Fruitless yields forgiven since hearing began, for the record.
    forgiven: u32,
    /// A forced yield (`stop/heard`) committed: the reply is to end
    /// ([`PlaybackMask::take_stop_committed`]).
    stop_committed: bool,
    /// See [`MATCHED_BLOCKS`].
    matched: u16,
    /// When the latest block was heard.
    listened_at_us: u64,
    /// `LAMP_CUT_IN_KEEPS_LISTENING=on`: see [`KEEPS_TRYING_BLOCKS`].
    keeps_listening: bool,
    /// Blocks in a row heard as a person over the lamp while it speaks,
    /// whether or not it may yield.
    over_run: u16,
    /// `LAMP_CUT_IN_COUNTS_REAL=on`: see [`YieldEvidence`].
    counts_real: bool,
    /// The onset run so far, while no yield is in force.
    onset: YieldEvidence,
    /// The window of the yield in force, or of the last one.
    window: YieldEvidence,
    /// See [`PlaybackMask::room_talk`].
    room_talk: bool,
    /// `LAMP_CUT_IN_NEAR_ASKER=on`: see [`NEAR_ASKER_DB`].
    near_asker: bool,
    /// The level of the person the reply answers, in dBFS before
    /// cancellation, when it is known ([`PlaybackMask::asker_level_db`]).
    asker_db: Option<f32>,
    /// The levels of the talk in the clear in the yield in force.
    yield_clear_db: Vec<f32>,
    /// How far under the asker the talk of the yield in force, or of the
    /// last one, was when it was let go as the room's, in dB.
    under_asker_db: Option<f32>,
}

/// The middle of nonempty `levels`.
fn middle_db(levels: &[f32]) -> f32 {
    let mut sorted = levels.to_vec();
    sorted.sort_by(f32::total_cmp);
    sorted[sorted.len() / 2]
}

impl Duplex {
    /// Fallen back for the rest of the reply: no yield, unless one is given
    /// back ([`FORGIVE_FRUITLESS_US`]).
    fn fallen(&self) -> bool {
        self.fruitless >= FRUITLESS_YIELDS || self.other_fruitless >= OTHER_FRUITLESS_YIELDS
    }
}

/// One capture block as the duplex mask heard it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Listened {
    pub heard: Option<Heard>,
    /// The score to give the turn detector for this block.
    pub score: f32,
    pub step: Step,
    /// A fruitless yield was forgiven on this block
    /// ([`FORGIVE_FRUITLESS_US`]).
    pub forgiven: bool,
}

/// A piece of the lamp's own sound that the speaker took: whose turn, how
/// many frames, how deep the speaker's queue was with them in it and when
/// that was looked at, and how loud the piece is on the ring's scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Taken {
    pub turn: u64,
    pub frames: usize,
    pub queued_frames: usize,
    pub queue_observed_at_us: u64,
    pub level: u8,
}

/// The most pieces kept between two looks: a second and more of sound.
const MAX_TAKEN: usize = 128;

pub(crate) struct PlaybackMask {
    /// What the speaker took since this was last asked, oldest first, for
    /// whoever follows the lamp's own sound (`crate::expression_out`).
    taken: Vec<Taken>,
    policy: TurnTaking,
    duplex: Option<Duplex>,
    pending: VecDeque<Dispatch>,
    spans: VecDeque<Span>,
    reads: VecDeque<Read>,
    retired_reads: Option<RetiredReads>,
    epoch: u64,
    last_reset_at: u64,
    last_dispatch: u64,
    last_now: u64,
    suppressed_blocks: u64,
    /// Listening gaps (#828, [`crate::listen_gaps`]): with the duplex mask
    /// only, off unless set.
    listen_gaps: Option<crate::listen_gaps::ListenGaps>,
}
impl PlaybackMask {
    pub fn new(policy: TurnTaking) -> Self {
        Self {
            taken: Vec::new(),
            policy,
            duplex: None,
            pending: VecDeque::with_capacity(8),
            spans: VecDeque::with_capacity(MAX_INTERVALS),
            reads: VecDeque::with_capacity(MAX_READS),
            retired_reads: None,
            epoch: 1,
            last_reset_at: 0,
            last_dispatch: 0,
            last_now: 0,
            suppressed_blocks: 0,
            listen_gaps: None,
        }
    }
    /// The conservative mask with ears. See the module documentation.
    pub fn duplex() -> Self {
        Self {
            duplex: Some(Duplex {
                hearing: Hearing::EVALUATED,
                interruption: Interruption::new(Confirmation::EVALUATED),
                margin: Margin::default(),
                heard: VecDeque::with_capacity(MAX_HEARD),
                max_heard: MAX_HEARD,
                last_yield: None,
                blind: true,
                fruitless: 0,
                other_fruitless: 0,
                last_yield_at_us: None,
                forgiven: 0,
                stop_committed: false,
                matched: 0,
                listened_at_us: 0,
                keeps_listening: false,
                room_talk: false,
                near_asker: false,
                asker_db: None,
                yield_clear_db: Vec::new(),
                under_asker_db: None,
                over_run: 0,
                counts_real: false,
                onset: YieldEvidence::default(),
                window: YieldEvidence::default(),
            }),
            ..Self::new(TurnTaking::V1Baseline)
        }
    }
    pub fn is_duplex(&self) -> bool {
        self.duplex.is_some()
    }
    /// Listening gaps (#828, `LAMP_LISTEN_GAPS=on`): a sentence end of a
    /// long reply is held quiet ([`crate::listen_gaps`]), and speech in the
    /// lamp's own silence is heard as a person over it
    /// (`Hearing::silence_heard`). Only with ears.
    pub fn listen_in_gaps(&mut self) {
        if let Some(duplex) = self.duplex.as_mut() {
            duplex.hearing.silence_heard = true;
            self.listen_gaps = Some(crate::listen_gaps::ListenGaps::default());
        }
    }
    /// The listening gaps, when on.
    pub fn listen_gaps(&mut self) -> Option<&mut crate::listen_gaps::ListenGaps> {
        self.listen_gaps.as_mut()
    }
    /// Whether the speaker's zeros for `owner` from `started_us` are a
    /// listening gap's, not a delivery gap.
    pub fn listen_gap_covers(&self, owner: TurnOwner, started_us: u64) -> bool {
        self.listen_gaps
            .as_ref()
            .is_some_and(|gaps| gaps.covers(owner, started_us))
    }
    /// How far a listening gap's zeros ran past its hold, last accepted at
    /// `last_zero_us`, if far enough to be a delivery gap too
    /// ([`crate::listen_gaps::ListenGaps::overran_us`]).
    pub fn listen_gap_overran_us(
        &self,
        owner: TurnOwner,
        started_us: u64,
        last_zero_us: u64,
    ) -> Option<u64> {
        self.listen_gaps
            .as_ref()
            .and_then(|gaps| gaps.overran_us(owner, started_us, last_zero_us))
    }
    /// `LAMP_CUT_IN_KEEPS_LISTENING=on`: a reply fallen back after its
    /// fruitless yields still yields to a voice that keeps going over it
    /// ([`KEEPS_TRYING_BLOCKS`]). Only with ears.
    pub fn keep_listening(&mut self) {
        if let Some(duplex) = self.duplex.as_mut() {
            duplex.keeps_listening = true;
        }
    }
    /// `LAMP_CUT_IN_COUNTS_REAL=on`: a resume counts toward
    /// [`FRUITLESS_YIELDS`] only when its yield's window points at the
    /// lamp's own voice ([`YieldEvidence`]). Only with ears.
    pub fn count_real_cut_ins(&mut self) {
        if let Some(duplex) = self.duplex.as_mut() {
            duplex.counts_real = true;
        }
    }
    /// `LAMP_CUT_IN_NEAR_ASKER=on` (#3967): talk in the clear in a yield
    /// commits only when near the level of the person being answered
    /// ([`NEAR_ASKER_DB`]). Only with ears.
    pub fn cut_in_near_asker(&mut self) {
        if let Some(duplex) = self.duplex.as_mut() {
            duplex.near_asker = true;
        }
    }
    /// The level of the person the reply in hand answers, in dBFS before
    /// cancellation, or `None` when none is known: a line the lamp began
    /// itself, one too short to measure, or another turn's. Without it the
    /// cut-in is judged as without the switch. Set for every block.
    pub fn asker_level_db(&mut self, level: Option<f32>) {
        if let Some(duplex) = self.duplex.as_mut() {
            duplex.asker_db = level;
        }
    }
    /// How far under the asker the talk of the latest yield let go as the
    /// room's was, in dB, for the record.
    pub fn under_asker_db(&self) -> Option<f32> {
        self.duplex.as_ref()?.under_asker_db
    }
    /// `LAMP_OWN_LINE_OVER_TALK=on` (#2602): the lamp's own line is sounding
    /// in a room busy as it began, so talk over it is the room's and makes it
    /// yield no more: withheld, its audio left the speaker playing zeros, a
    /// playback gap. The talk is still heard as a person's and judged as a
    /// candidate (the lamp's name, a face turned to it), and a stop heard by
    /// its own listener still yields. Set for every block; a yield already
    /// in force runs on.
    pub fn room_talk(&mut self, on: bool) {
        if let Some(duplex) = self.duplex.as_mut() {
            duplex.room_talk = on;
        }
    }
    /// Remember enough hearing for a yield's first words (#828): every
    /// block of a candidate that reaches back is checked against it before
    /// the candidate is let through.
    pub fn keep_first_words(&mut self) {
        if let Some(duplex) = self.duplex.as_mut() {
            duplex.max_heard = duplex.max_heard.max(FIRST_WORDS_HEARD);
        }
    }
    /// Remember enough hearing for a sound that waits while an answer is
    /// still to come (#1123, [`WAITED_HEARD`]); never less than is kept
    /// already.
    pub fn keep_waited_sound(&mut self) {
        if let Some(duplex) = self.duplex.as_mut() {
            duplex.max_heard = duplex.max_heard.max(WAITED_HEARD);
        }
    }
    /// The yield in force waits for the words said in it, with
    /// `LAMP_STOP_WORDS=heard` (#828): see `Interruption::await_words`.
    pub fn await_words(&mut self) {
        if let Some(duplex) = self.duplex.as_mut() {
            duplex.interruption.await_words();
        }
    }
    /// The yield's listener asked for its words: see
    /// `Interruption::words_asked`.
    pub fn words_asked(&mut self) {
        if let Some(duplex) = self.duplex.as_mut() {
            duplex.interruption.words_asked();
        }
    }
    /// A forced yield committed since the last look: the stop was a stop,
    /// and the reply ends, whether or not anything said in it is admitted
    /// (a commit alone only lets a waiting request through).
    pub fn take_stop_committed(&mut self) -> bool {
        self.duplex
            .as_mut()
            .is_some_and(|duplex| std::mem::take(&mut duplex.stop_committed))
    }
    /// A stop word heard by its own listener (`stop/heard`, #828): the next
    /// block with something to yield begins a yield. See
    /// `Interruption::force_yield`. Whether it was taken: a duplex mask with
    /// no yield in force.
    pub fn force_yield(&mut self) -> bool {
        self.duplex
            .as_mut()
            .is_some_and(|duplex| duplex.interruption.force_yield())
    }
    /// Its words came: whether they told the lamp to stop, and whether they
    /// were a backchannel (#2939).
    pub fn words(&mut self, stop: bool, backchannel: bool) {
        if let Some(duplex) = self.duplex.as_mut() {
            duplex.interruption.words(stop, backchannel);
        }
    }
    /// `LAMP_BACKCHANNEL_HOLDS=on` (#2939): a yield's commit is held for
    /// its words, and a backchannel resumes the reply. See
    /// `Interruption::hold_backchannels`. Only with ears. A commit held to
    /// the yield's deadline (1.1 s) keeps its pre-roll and onset with it,
    /// more than [`MAX_HEARD`] remembers: as much is kept as for a waited
    /// sound ([`WAITED_HEARD`]).
    /// The words are asked for after `crate::stop_words::FINALIZE_AFTER_STILL_BLOCKS`.
    pub fn hold_backchannels(&mut self) {
        if let Some(duplex) = self.duplex.as_mut() {
            duplex
                .interruption
                .hold_backchannels(crate::stop_words::FINALIZE_AFTER_STILL_BLOCKS);
            duplex.max_heard = duplex.max_heard.max(WAITED_HEARD);
        }
    }
    /// Whether backchannels are held ([`Self::hold_backchannels`]).
    pub fn holds_backchannels(&self) -> bool {
        self.duplex
            .as_ref()
            .is_some_and(|duplex| duplex.interruption.holds_backchannels())
    }
    /// For the record of the latest commit or resume: how long its commit
    /// was held for its words, in ms, and whether they were a backchannel
    /// (`None` until they came).
    pub fn yield_words(&self) -> (u64, Option<bool>) {
        self.duplex.as_ref().map_or((0, None), |duplex| {
            (
                u64::from(duplex.interruption.held_blocks()) * 10,
                duplex.interruption.backchannel(),
            )
        })
    }
    /// Blocks of stillness in a row in the yield in force, while one is.
    pub fn still_blocks(&self) -> Option<u8> {
        self.duplex.as_ref()?.interruption.still_blocks()
    }
    /// Whether a yield is in force, by what was heard alone.
    pub fn yield_in_force(&self) -> bool {
        self.duplex
            .as_ref()
            .is_some_and(|duplex| duplex.interruption.yielded())
    }
    /// Hear one capture block. Call for every block, in order, before asking
    /// for its disposition. `speaking` is whether a reply has audio that
    /// would otherwise be going to the speaker, or sound still in the air.
    /// Without evidence the block is not judged and keeps its own score; the
    /// conservative mask covers it.
    pub fn listen(
        &mut self,
        sequence: u64,
        probability: f32,
        evidence: Option<&EchoEvidence>,
        speaking: bool,
        at_us: u64,
    ) -> io::Result<Listened> {
        let unjudged = Listened {
            heard: None,
            score: probability,
            step: Step::Carry,
            forgiven: false,
        };
        let Some(duplex) = self.duplex.as_mut() else {
            return Ok(unjudged);
        };
        if at_us < duplex.listened_at_us {
            return Err(invalid("hearing time moved backwards"));
        }
        duplex.listened_at_us = at_us;
        // A fruitless yield is forgiven after a spell of the reply with no
        // yield, so a long story is never deaf for its rest.
        let forgiven = (duplex.fruitless > 0 || duplex.other_fruitless > 0)
            && duplex
                .last_yield_at_us
                .is_some_and(|at| at_us.saturating_sub(at) >= FORGIVE_FRUITLESS_US);
        if forgiven {
            duplex.fruitless = duplex.fruitless.saturating_sub(1);
            duplex.other_fruitless = duplex.other_fruitless.saturating_sub(1);
            duplex.forgiven = duplex.forgiven.saturating_add(1);
            duplex.last_yield_at_us = Some(at_us);
        }
        let unjudged = Listened {
            forgiven,
            ..unjudged
        };
        if duplex
            .heard
            .back()
            .is_some_and(|(last, _)| last.checked_add(1) != Some(sequence))
        {
            // A gap in capture: what was heard before it says nothing now.
            duplex.heard.clear();
            duplex.interruption.clear();
            duplex.last_yield = None;
            duplex.onset = YieldEvidence::default();
        }
        let Some(evidence) = evidence else {
            duplex.blind = true;
            // A yield in force still runs to its deadline.
            let step = duplex.interruption.block(Heard::Unknown, speaking);
            // No evidence either way, and the end of any onset run.
            duplex.onset = YieldEvidence::default();
            if step == Step::Yield {
                duplex.window = YieldEvidence::default();
            }
            Self::settle(duplex, sequence, step);
            return Ok(Listened {
                step,
                score: Self::held_open(duplex, probability),
                ..unjudged
            });
        };
        duplex.blind = false;
        if !speaking {
            duplex.matched = 0;
        } else if evidence
            .echo
            .is_some_and(|echo| echo.capture_explained >= MATCH_PERCENT)
        {
            duplex.matched = duplex.matched.saturating_add(1);
        }
        let heard = match duplex.hearing.heard(probability, evidence) {
            // The lamp's own sound has not been found in this reply yet.
            Heard::Over if speaking && duplex.matched < MATCHED_BLOCKS => Heard::Unknown,
            heard => heard,
        };
        duplex.margin.observe(heard, evidence);
        duplex.over_run = if speaking && heard == Heard::Over {
            duplex.over_run.saturating_add(1)
        } else {
            0
        };
        let fallen = duplex.fallen();
        // With the switch, a voice that keeps going over a reply fallen
        // back is still a person trying to cut in, a few times more.
        let keeps_trying = duplex.keeps_listening
            && fallen
            && duplex.fruitless < FRUITLESS_YIELDS.saturating_add(KEEPS_TRYING_YIELDS)
            && duplex.other_fruitless < OTHER_FRUITLESS_YIELDS.saturating_add(KEEPS_TRYING_YIELDS)
            && duplex.over_run > KEEPS_TRYING_BLOCKS;
        // A reply that keeps yielding to nothing stops yielding.
        // A stop heard by its own listener yields however many did not.
        // Nor does the lamp's own line yield to a busy room's talk.
        let may_yield = speaking
            && (duplex.interruption.force_pending()
                || !duplex.room_talk && (!fallen || keeps_trying));
        let plain = duplex.hearing.plain(heard, evidence);
        let was_yielded = duplex.interruption.yielded();
        // The talk in the clear in a yield against the asker (#3967).
        if !was_yielded {
            duplex.yield_clear_db.clear();
        } else if duplex.near_asker && heard == Heard::Clear {
            duplex
                .yield_clear_db
                .push(f32::from(evidence.capture_level.tenths_of_db()) / 10.0);
        }
        let under = duplex
            .asker_db
            .filter(|_| duplex.near_asker && duplex.yield_clear_db.len() >= NEAR_ASKER_BLOCKS)
            .map(|asker| asker - middle_db(&duplex.yield_clear_db))
            .filter(|under| *under > NEAR_ASKER_DB);
        duplex.interruption.quiet_talk(under.is_some());
        if under.is_some() {
            duplex.under_asker_db = under;
        }
        let step = duplex.interruption.observe(heard, plain, may_yield);
        if step == Step::Yield {
            duplex.under_asker_db = None;
        }
        // The yield's evidence window: its onset run, then every block
        // heard while it is in force, through the one that settles it.
        if was_yielded {
            duplex.window.add(heard, plain);
        } else if may_yield && heard == Heard::Over {
            duplex.onset.add(heard, plain);
        } else {
            duplex.onset = YieldEvidence::default();
        }
        if step == Step::Yield {
            duplex.window = std::mem::take(&mut duplex.onset);
        }
        if duplex.heard.len() >= duplex.max_heard {
            duplex.heard.pop_front();
        }
        duplex.heard.push_back((sequence, heard));
        Self::settle(duplex, sequence, step);
        // The person the lamp stopped for with the switch is heard on.
        let heard_on =
            keeps_trying || (duplex.keeps_listening && fallen && duplex.interruption.yielded());
        let score = if speaking && fallen && !heard_on && heard == Heard::Over {
            // Fallen back for this reply: a person over the lamp is not let
            // through either, as in the conservative baseline.
            0.0
        } else if (1..=duplex
            .interruption
            .onset_needed()
            .saturating_sub(START_BLOCKS as u8))
            .contains(&duplex.interruption.onset_run())
        {
            // Over the lamp, and not yet for long enough to stop it. The
            // detector hears nothing of the first blocks of the run, as many
            // as the yield takes more than its own six, so that its start
            // ends on the block that yields and never before it.
            0.0
        } else {
            Hearing::detector_score(heard, probability)
        };
        Ok(Listened {
            heard: Some(heard),
            score: Self::held_open(duplex, score),
            step,
            ..unjudged
        })
    }
    /// While the lamp has stopped for someone, what it hears is one
    /// utterance until the yield is settled: the detector is never given a
    /// score it would count towards the end of speech. A yield can stay open
    /// for longer than the detector waits for that end. On lamp-4ace it
    /// closed the utterance 600 ms into a yield, the person went on 40 ms
    /// later, and the commit that followed had nothing current to admit.
    fn held_open(duplex: &Duplex, score: f32) -> f32 {
        if duplex.interruption.yielded() {
            score.max(SILENCE_PROBABILITY)
        } else {
            score
        }
    }
    fn settle(duplex: &mut Duplex, sequence: u64, step: Step) {
        match step {
            Step::Carry => {}
            Step::Yield => {
                duplex.last_yield = Some((sequence, Yielded::Undecided));
                duplex.last_yield_at_us = Some(duplex.listened_at_us);
            }
            Step::Commit => {
                if let Some((_, state)) = duplex.last_yield.as_mut() {
                    *state = Yielded::Committed;
                }
                duplex.fruitless = 0;
                duplex.other_fruitless = 0;
                duplex.stop_committed |= duplex.interruption.yield_was_forced();
            }
            Step::Resume => {
                if let Some((_, state)) = duplex.last_yield.as_mut() {
                    *state = Yielded::Resumed;
                }
                // A forced yield that resumes is a false hearing of the
                // stop listener, not the lamp yielding to its own voice.
                // With `LAMP_CUT_IN_COUNTS_REAL=on`, nor is one whose window
                // did not point at the lamp's own voice ([`YieldEvidence`]):
                // that counts on its own bound ([`OTHER_FRUITLESS_YIELDS`]).
                // Nor is a backchannel held as a yield (#2939): a person
                // saying "mm-hm" twice is not the lamp's own voice.
                if !duplex.interruption.yield_was_forced()
                    && !duplex.interruption.yield_was_backchannel()
                {
                    if !duplex.counts_real || duplex.window.own_voice() {
                        duplex.fruitless = duplex.fruitless.saturating_add(1);
                    } else {
                        duplex.other_fruitless = duplex.other_fruitless.saturating_add(1);
                    }
                }
                // What plays next starts like a reply does.
                duplex.matched = 0;
            }
        }
    }
    /// For tests of what follows: the lamp's own sound has been found.
    #[cfg(test)]
    pub(crate) fn assume_matched(&mut self) {
        if let Some(duplex) = self.duplex.as_mut() {
            duplex.matched = MATCHED_BLOCKS;
        }
    }
    /// The speaker took a piece of sound. Kept until asked for; if nobody
    /// asks, the oldest are let go.
    pub fn speaker_took(&mut self, piece: Taken) {
        if self.taken.len() == MAX_TAKEN {
            self.taken.remove(0);
        }
        self.taken.push(piece);
    }
    /// What the speaker took since the last call, oldest first.
    pub fn take_taken(&mut self) -> Vec<Taken> {
        std::mem::take(&mut self.taken)
    }
    /// Whether reply audio is being withheld from the speaker for a person.
    /// Only while hearing is current: see [`HEARING_STALE_US`].
    pub fn yielded(&self, now: u64) -> bool {
        self.duplex.as_ref().is_some_and(|duplex| {
            // A committed stop holds the speaker until the coordinator has
            // ended the reply on its next pass (the reviewer on #1555: one
            // chunk was handed on between the commit and the cancel).
            (duplex.interruption.yielded() || duplex.stop_committed)
                && now.saturating_sub(duplex.listened_at_us) <= HEARING_STALE_US
        })
    }
    /// A reply ended or was replaced: the next one starts with a clean
    /// record. A yield in force is left to be settled by what is heard: a
    /// candidate may be waiting on it, and with no reply it withholds nothing.
    pub fn reply_closed(&mut self) {
        if let Some(duplex) = self.duplex.as_mut() {
            duplex.fruitless = 0;
            duplex.other_fruitless = 0;
            duplex.last_yield_at_us = None;
            duplex.stop_committed = false;
        }
    }
    /// The person's level against the lamp's own at the microphone, once
    /// both have been heard. See `lamp_audio::duplex::Margin`.
    pub fn hearing_margin_db(&self) -> Option<f32> {
        self.duplex.as_ref().and_then(|duplex| duplex.margin.db())
    }
    /// The two levels the margin is the difference of, before cancellation,
    /// in dBFS: a person heard with the lamp silent, and the lamp's own voice
    /// where the canceller let it through. Slow averages of each.
    pub fn hearing_levels_db(&self) -> (Option<f32>, Option<f32>) {
        self.duplex.as_ref().map_or((None, None), |duplex| {
            (duplex.margin.person_db(), duplex.margin.own_db())
        })
    }
    /// Yields, commits and resumes so far, and whether this reply has fallen
    /// back to the conservative mask.
    pub fn duplex_summary(&self) -> Option<((u32, u32, u32), bool)> {
        self.duplex
            .as_ref()
            .map(|duplex| (duplex.interruption.counts(), duplex.fallen()))
    }
    /// Fruitless yields forgiven since hearing began
    /// ([`FORGIVE_FRUITLESS_US`]): `fallen_back_at_end` says only whether the
    /// last reply had fallen back when the session ended (Eighth on #2048).
    pub fn fruitless_forgiven(&self) -> u32 {
        self.duplex.as_ref().map_or(0, |duplex| duplex.forgiven)
    }
    /// Hearing, when the latest block was judged.
    fn hearing(&self) -> Option<&Duplex> {
        self.duplex.as_ref().filter(|duplex| !duplex.blind)
    }
    /// A candidate whose six onset blocks end at `trigger`: one that began
    /// over the lamp waits for its yield to be settled. `None` when not all
    /// of the onset was heard; the conservative rules then decide it.
    fn heard_disposition(duplex: &Duplex, first: u64, trigger: u64) -> Option<Disposition> {
        let mut over = false;
        let mut found = 0;
        for (_, heard) in duplex
            .heard
            .iter()
            .filter(|(sequence, _)| (first..=trigger).contains(sequence))
        {
            found += 1;
            over |= *heard == Heard::Over;
        }
        if trigger < first || found != trigger - first + 1 {
            return None;
        }
        if !over {
            return Some(Disposition::Pass);
        }
        Some(match duplex.last_yield {
            Some((at, Yielded::Undecided)) if at >= first => Disposition::AwaitWriteEvidence,
            Some((at, Yielded::Resumed)) if at >= first => Disposition::Suppress,
            // Committed, or a person over a sound that was no reply.
            _ => Disposition::Pass,
        })
    }
    /// Whether this capture block was judged with evidence.
    fn was_heard(&self, sequence: u64) -> bool {
        self.hearing()
            .is_some_and(|duplex| duplex.heard.iter().any(|(heard, _)| *heard == sequence))
    }
    pub fn suppressed(&mut self) -> io::Result<()> {
        self.suppressed_blocks = self
            .suppressed_blocks
            .checked_add(1)
            .ok_or_else(|| invalid("suppression counter overflow"))?;
        Ok(())
    }
    pub fn suppressed_blocks(&self) -> u64 {
        self.suppressed_blocks
    }
    pub fn discarded(&mut self, owner: TurnOwner, observed: u64, now: u64) -> io::Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        self.advance(now)?;
        if observed == 0 || observed > now {
            return Err(invalid("invalid playback discard time"));
        }
        for span in &mut self.spans {
            if span.owner == owner && span.end.is_none() {
                if observed < span.accepted_at {
                    return Err(invalid("discard predates accepted speech"));
                }
                span.end = Some(
                    observed
                        .checked_add(TAIL_US)
                        .ok_or_else(|| invalid("tail overflow"))?,
                );
            }
        }
        Ok(())
    }
    /// A successful physical drop/reset is not playback retirement. Preserve
    /// the closed old interval for delayed capture and accept only its exact
    /// next clock epoch. Pending command rejection obligations remain intact.
    pub fn clock_reset(
        &mut self,
        previous: QueuedRange,
        next_epoch: u64,
        observed: u64,
        now: u64,
    ) -> io::Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        self.advance(now)?;
        if previous.epoch == 0
            || previous.epoch.checked_add(1) != Some(next_epoch)
            || previous.retired_through > previous.accepted_through
            || previous.accepted_through - previous.retired_through
                > lamp_audio::ownership::MAX_QUEUED_FRAMES as u64
            || self.epoch != previous.epoch
            || observed == 0
            || observed > now
            || observed < self.last_reset_at
        {
            return Err(invalid("invalid or duplicate physical playback reset"));
        }
        for span in self
            .spans
            .iter_mut()
            .filter(|span| span.cursor.epoch == previous.epoch)
        {
            if span.cursor.frame > previous.accepted_through || observed < span.accepted_at {
                return Err(invalid("playback reset does not cover accepted speech"));
            }
            if span.end.is_none() {
                span.end = Some(
                    observed
                        .checked_add(TAIL_US)
                        .ok_or_else(|| invalid("tail overflow"))?,
                );
            }
        }
        self.epoch = next_epoch;
        self.last_reset_at = observed;
        Ok(())
    }
    pub fn enabled(&self) -> bool {
        self.policy == TurnTaking::V1Baseline
    }
    fn advance(&mut self, now: u64) -> io::Result<()> {
        if now < self.last_now {
            return Err(invalid("turn-taking clock regressed"));
        }
        self.last_now = now;
        if self
            .pending
            .iter()
            .any(|p| now.saturating_sub(p.at_us) >= crate::admission::DECISION_BUDGET_US)
        {
            return Err(invalid("speaker write evidence expired"));
        }
        while self.spans.front().is_some_and(|s| {
            s.end
                .is_some_and(|end| now.saturating_sub(end) > HISTORY_US)
        }) {
            self.spans.pop_front();
        }
        Ok(())
    }
    pub fn dispatched(
        &mut self,
        owner: TurnOwner,
        sequence: u64,
        at_us: u64,
        frames: usize,
    ) -> io::Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        self.advance(at_us)?;
        if (self.pending.len() == 8 || self.pending.iter().any(|p| p.owner == owner))
            || sequence <= self.last_dispatch
            || at_us == 0
            || frames == 0
            || frames > 240
        {
            return Err(invalid("invalid or unresolved speaker dispatch"));
        }
        self.last_dispatch = sequence;
        self.pending.push_back(Dispatch {
            owner,
            sequence,
            at_us,
            frames,
            accepted: 0,
            end: None,
        });
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub fn accepted(
        &mut self,
        owner: TurnOwner,
        sequence: u64,
        first: PlaybackCursor,
        end: PlaybackCursor,
        frames: usize,
        completed: u64,
        queue_observed: u64,
        now: u64,
    ) -> io::Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        self.advance(now)?;
        if self.epoch != first.epoch {
            return Err(invalid(
                "accepted speech changed epoch without physical reset",
            ));
        }
        let index = self
            .pending
            .iter()
            .position(|p| p.sequence == sequence)
            .ok_or_else(|| invalid("write has no original dispatch"))?;
        let pending = &mut self.pending[index];
        if owner != pending.owner
            || sequence != pending.sequence
            || first.epoch == 0
            || first.epoch != end.epoch
            || end.frame.checked_sub(first.frame) != Some(frames as u64)
            || frames == 0
            || completed < pending.at_us
            || completed > queue_observed
            || queue_observed > now
            || pending.end.is_some_and(|previous| previous != first)
            || pending
                .accepted
                .checked_add(frames)
                .is_none_or(|n| n > pending.frames)
        {
            return Err(invalid("invalid accepted speech receipt"));
        }
        let start = pending.at_us;
        pending.accepted += frames;
        pending.end = Some(end);
        let done = pending.accepted == pending.frames;
        if let Some(last) = self.spans.back_mut() {
            if (first.epoch == last.cursor.epoch && first.frame < last.cursor.frame)
                || completed < last.accepted_at
            {
                return Err(invalid("accepted speech cursor regressed"));
            }
            if first.epoch == last.cursor.epoch
                && last.owner == owner
                && last.end.is_none_or(|until| start <= until)
            {
                last.end = None;
                last.sequence = sequence;
                last.cursor = end;
                last.accepted_at = completed;
            } else {
                if last.end.is_none() {
                    return Err(invalid("new speech owner before old speech drained"));
                }
                self.push_span(Span {
                    owner,
                    start,
                    end: None,
                    sequence,
                    cursor: end,
                    accepted_at: completed,
                })?;
            }
        } else {
            self.push_span(Span {
                owner,
                start,
                end: None,
                sequence,
                cursor: end,
                accepted_at: completed,
            })?;
        }
        if done {
            self.pending.remove(index);
        }
        Ok(())
    }
    fn push_span(&mut self, span: Span) -> io::Result<()> {
        if self.spans.len() == MAX_INTERVALS {
            return Err(invalid("turn-taking interval history full"));
        }
        self.spans.push_back(span);
        Ok(())
    }
    /// A nonfinal gap receipt is local queue evidence, never conversation completion.
    pub fn drained(
        &mut self,
        owner: TurnOwner,
        sequence: u64,
        cursor: PlaybackCursor,
        observed: u64,
        now: u64,
    ) -> io::Result<bool> {
        if !self.enabled() {
            return Ok(false);
        }
        self.advance(now)?;
        if cursor.epoch == 0 || cursor.frame == 0 || observed == 0 || observed > now {
            return Err(invalid("invalid speech-drained time/cursor"));
        }
        let known_owner = self.spans.iter().any(|s| s.owner == owner);
        let last = self
            .spans
            .back_mut()
            .ok_or_else(|| invalid("drain without accepted speech"))?;
        if owner != last.owner || sequence != last.sequence || cursor != last.cursor {
            // An older receipt cannot close a resumed write. It must still name
            // a known original owner and a cursor strictly behind the latest one.
            if known_owner
                && cursor.epoch == last.cursor.epoch
                && cursor.frame < last.cursor.frame
                && sequence <= last.sequence
            {
                return Ok(false);
            }
            return Err(invalid(
                "speech-drained receipt mismatched latest accepted cursor",
            ));
        }
        if last.end.is_some() || observed < last.accepted_at {
            return Err(invalid("duplicate or regressing speech drain"));
        }
        last.end = Some(
            observed
                .checked_add(TAIL_US)
                .ok_or_else(|| invalid("tail overflow"))?,
        );
        Ok(true)
    }
    /// Existing immutable cancelled-tail evidence closes only its original mask.
    pub fn cancelled_tail(
        &mut self,
        owner: TurnOwner,
        through: PlaybackCursor,
        observed: u64,
        now: u64,
    ) -> io::Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        self.advance(now)?;
        let Some(last) = self.spans.back_mut().filter(|s| s.owner == owner) else {
            return Ok(());
        };
        if through.epoch != last.cursor.epoch
            || through.frame < last.cursor.frame
            || observed < last.accepted_at
            || observed > now
        {
            return Err(invalid("cancelled tail does not cover accepted speech"));
        }
        if last.end.is_none() {
            last.end = Some(
                observed
                    .checked_add(TAIL_US)
                    .ok_or_else(|| invalid("tail overflow"))?,
            );
        }
        Ok(())
    }
    pub fn rejected(&mut self, sequence: u64, now: u64) -> io::Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        self.advance(now)?;
        if let Some(index) = self.pending.iter().position(|p| p.sequence == sequence) {
            self.pending.remove(index);
            Ok(())
        } else {
            Err(invalid("speaker rejection has no matching dispatch"))
        }
    }
    pub fn remember_read(
        &mut self,
        sequence: u64,
        first: u64,
        completed: u64,
        now: u64,
    ) -> io::Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        self.advance(now)?;
        if sequence == 0
            || first == 0
            || first > completed
            || completed > now
            || now.saturating_sub(first) > HISTORY_US
            || self
                .reads
                .back()
                .is_some_and(|r| r.sequence.checked_add(1) != Some(sequence) || first < r.completed)
        {
            return Err(invalid("invalid original microphone read interval"));
        }
        if self.reads.len() == MAX_READS {
            let read = self.reads.pop_front().expect("full read history");
            if let Some(retired) = self.retired_reads.as_mut() {
                retired.last_sequence = read.sequence;
                retired.last_completed = read.completed;
            } else {
                self.retired_reads = Some(RetiredReads {
                    first_sequence: read.sequence,
                    first_completed: read.completed,
                    last_sequence: read.sequence,
                    last_completed: read.completed,
                });
            }
        }
        self.reads.push_back(Read {
            sequence,
            first,
            completed,
        });
        Ok(())
    }
    pub fn clear_read_lineage(&mut self) {
        self.reads.clear();
        self.retired_reads = None;
        if let Some(duplex) = self.duplex.as_mut() {
            // Another capture stream: what was heard on the old one is gone.
            duplex.heard.clear();
            duplex.interruption.clear();
            duplex.last_yield = None;
            duplex.blind = true;
        }
    }
    fn read_disposition(&self, read: Read) -> Disposition {
        if self
            .spans
            .iter()
            .any(|s| read.completed >= s.start && s.end.is_none_or(|end| read.first < end))
        {
            return Disposition::Suppress;
        }
        if self.pending.iter().any(|p| read.completed >= p.at_us) {
            Disposition::AwaitWriteEvidence
        } else {
            Disposition::Pass
        }
    }
    pub fn frame(&self) -> Disposition {
        if self.hearing().is_some() {
            // Heard blocks reach the detector with the score `listen` gave.
            return Disposition::Pass;
        }
        self.reads
            .back()
            .map_or(Disposition::Pass, |r| self.read_disposition(*r))
    }
    /// Recheck retained pre-roll after delayed output evidence. Use the exact
    /// original read interval, never infer its beginning from receipt time.
    pub fn retained_frame(&self, frame: &ObservedAudio) -> io::Result<Disposition> {
        if !self.enabled() {
            return Ok(Disposition::Pass);
        }
        if frame.sequence == 0 || frame.captured_at_us == 0 || frame.captured_at_us > self.last_now
        {
            return Err(invalid("invalid retained prefix observation"));
        }
        let cached = self
            .reads
            .iter()
            .find(|read| read.sequence == frame.sequence);
        if cached.is_some_and(|read| read.completed != frame.captured_at_us) {
            return Err(invalid("retained prefix original read timestamp mismatch"));
        }
        if self.was_heard(frame.sequence) {
            // Duplex keeps the original validated observation heard with echo
            // evidence; a cached conflicting timestamp is never evidence.
            return Ok(Disposition::Pass);
        }
        let read = match cached {
            Some(read) if read.completed == frame.captured_at_us => read,
            Some(_) => return Err(invalid("retained prefix original read timestamp mismatch")),
            None if self
                .retired_reads
                .is_some_and(|retired| retired.covers(frame)) =>
            {
                return Err(io::Error::other(RetainedHistoryLoss::Evicted));
            }
            None => {
                return Err(invalid(
                    "retained prefix original read interval unavailable",
                ));
            }
        };
        if self.last_now.saturating_sub(read.first) > HISTORY_US {
            return Err(io::Error::other(RetainedHistoryLoss::Expired));
        }
        Ok(self.read_disposition(*read))
    }
    pub fn candidate(&self, candidate: Candidate) -> io::Result<Disposition> {
        if !self.enabled() {
            return Ok(Disposition::Pass);
        }
        let first_sequence = candidate
            .trigger_sequence
            .saturating_sub(u64::from(START_BLOCKS) - 1)
            .max(1);
        if let Some(disposition) = self.hearing().and_then(|duplex| {
            Self::heard_disposition(duplex, first_sequence, candidate.trigger_sequence)
        }) {
            return Ok(disposition);
        }
        let mut disposition = Disposition::Pass;
        let mut found = false;
        for read in self
            .reads
            .iter()
            .filter(|r| (first_sequence..=candidate.trigger_sequence).contains(&r.sequence))
        {
            found |= read.sequence == candidate.trigger_sequence;
            match self.read_disposition(*read) {
                Disposition::Suppress => return Ok(Disposition::Suppress),
                Disposition::AwaitWriteEvidence => disposition = Disposition::AwaitWriteEvidence,
                Disposition::Pass => {}
            }
        }
        if !found {
            return Err(invalid("candidate original read interval unavailable"));
        }
        Ok(disposition)
    }
    pub fn available(&self, now: u64) -> bool {
        !self.enabled()
            || (self.pending.is_empty()
                && self
                    .spans
                    .back()
                    .is_none_or(|s| s.end.is_some_and(|end| now >= end)))
    }
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::other(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A tie is not the lamp's own voice, and `Clear` is a person
    /// (the Reviewer-lead's minor on #2779).
    #[test]
    fn a_yield_window_tie_or_a_clear_block_is_not_the_lamps_own_voice() {
        let mut window = YieldEvidence::default();
        window.add(Heard::Own, false);
        window.add(Heard::Clear, false);
        assert!(!window.own_voice(), "a tie");
        window.add(Heard::Over, false);
        assert!(window.own_voice(), "not-plain over is the lamp's own voice");
        window.add(Heard::Over, true);
        window.add(Heard::Unknown, false);
        assert!(
            !window.own_voice(),
            "plain over is a person; unknown is neither"
        );
    }
    use lamp_turn_owner::{
        AdmissionState, AdmittedInput, BootId, CaptureState, Controller, MonoTime, Permission,
    };
    fn owners() -> (TurnOwner, TurnOwner) {
        let t = MonoTime::from_micros(1_000_000);
        let mut c = Controller::new(BootId::new([8; 16]).unwrap(), t);
        c.set_microphone_permission(t, Permission::Allowed).unwrap();
        c.set_capture(
            t,
            CaptureState::RetainingUntil(MonoTime::from_micros(1_900_000)),
        )
        .unwrap();
        c.set_admission(
            t,
            AdmissionState::OpenUntil(MonoTime::from_micros(1_900_000)),
        )
        .unwrap();
        let a = c.admit(t, AdmittedInput::NewTurn).unwrap();
        let b = c.admit(t, AdmittedInput::Interruption).unwrap();
        (a, b)
    }
    fn cursor(frame: u64) -> PlaybackCursor {
        PlaybackCursor { epoch: 1, frame }
    }
    fn mask() -> PlaybackMask {
        PlaybackMask::new(TurnTaking::V1Baseline)
    }
    #[test]
    fn partial_writes_and_nonfinal_gap_preserve_exact_cursors_and_tail() {
        let (owner, _) = owners();
        let mut m = mask();
        m.dispatched(owner, 1, 1_000_000, 240).unwrap();
        m.accepted(
            owner,
            1,
            cursor(480),
            cursor(600),
            120,
            1_001_000,
            1_001_000,
            1_001_000,
        )
        .unwrap();
        m.accepted(
            owner,
            1,
            cursor(600),
            cursor(720),
            120,
            1_002_000,
            1_002_000,
            1_002_000,
        )
        .unwrap();
        assert!(
            !m.drained(owner, 1, cursor(600), 1_003_000, 1_003_000)
                .unwrap()
        );
        assert!(!m.available(1_090_000));
        m.drained(owner, 1, cursor(720), 1_020_000, 1_020_000)
            .unwrap();
        assert!(!m.available(1_119_999));
        assert!(m.available(1_120_000));
        // Provider owes more speech; intervening real zeros advance cursor.
        m.dispatched(owner, 2, 1_200_000, 240).unwrap();
        m.accepted(
            owner,
            2,
            cursor(5040),
            cursor(5280),
            240,
            1_201_000,
            1_201_000,
            1_201_000,
        )
        .unwrap();
        assert!(
            !m.drained(owner, 1, cursor(720), 1_021_000, 1_202_000)
                .unwrap()
        );
        assert!(!m.available(1_300_000));
    }
    #[test]
    fn delayed_retirement_uses_source_time_and_preserves_closed_history() {
        let (owner, _) = owners();
        let mut m = mask();
        m.dispatched(owner, 1, 1_000_000, 240).unwrap();
        m.accepted(
            owner,
            1,
            cursor(480),
            cursor(720),
            240,
            1_001_000,
            1_001_000,
            1_001_000,
        )
        .unwrap();
        m.drained(owner, 1, cursor(720), 1_020_000, 1_200_000)
            .unwrap();
        assert!(m.available(1_200_000));
        m.remember_read(1, 1_050_000, 1_060_000, 1_200_001).unwrap();
        assert_eq!(m.frame(), Disposition::Suppress);
        m.remember_read(2, 1_120_000, 1_130_000, 1_200_002).unwrap();
        assert_eq!(m.frame(), Disposition::Pass);
        assert!(
            m.drained(owner, 1, cursor(720), 1_020_000, 1_200_003)
                .is_err()
        );
    }
    #[test]
    fn late_original_write_settles_without_borrowing_successor_identity() {
        let (old, new) = owners();
        let mut m = mask();
        m.dispatched(old, 1, 1_000_000, 240).unwrap();
        m.dispatched(new, 2, 1_010_000, 240).unwrap();
        m.accepted(
            old,
            1,
            cursor(480),
            cursor(720),
            240,
            1_001_000,
            1_001_000,
            1_020_000,
        )
        .unwrap();
        m.cancelled_tail(old, cursor(720), 1_015_000, 1_021_000)
            .unwrap();
        m.accepted(
            new,
            2,
            cursor(720),
            cursor(960),
            240,
            1_022_000,
            1_022_000,
            1_022_000,
        )
        .unwrap();
        assert!(!m.available(1_030_000));
        assert!(
            !m.drained(old, 1, cursor(720), 1_015_000, 1_030_000)
                .unwrap()
        );
    }
    #[test]
    fn invalid_receipts_do_not_certify_mask_or_acceptance() {
        let (owner, wrong) = owners();
        for bad in 0..5 {
            let mut m = mask();
            m.dispatched(owner, 1, 1_000_000, 240).unwrap();
            let result = m.accepted(
                if bad == 0 { wrong } else { owner },
                1,
                cursor(480),
                cursor(if bad == 1 { 721 } else { 720 }),
                240,
                1_001_000,
                if bad == 2 {
                    1_000_000
                } else if bad == 3 {
                    1_020_000
                } else {
                    1_001_000
                },
                if bad == 4 { 999_999 } else { 1_002_000 },
            );
            assert!(result.is_err(), "case {bad}");
        }
        let mut m = mask();
        m.dispatched(owner, 1, 1_000_000, 240).unwrap();
        assert!(m.remember_read(1, 1_190_000, 1_200_000, 1_200_000).is_err());
    }
    #[test]
    fn original_read_start_is_preserved_and_clock_regression_is_rejected() {
        let (owner, _) = owners();
        let mut m = mask();
        m.dispatched(owner, 1, 1_000_000, 240).unwrap();
        m.accepted(
            owner,
            1,
            cursor(480),
            cursor(720),
            240,
            1_001_000,
            1_001_000,
            1_001_000,
        )
        .unwrap();
        m.drained(owner, 1, cursor(720), 1_020_000, 1_020_000)
            .unwrap();
        m.remember_read(1, 1_119_999, 1_130_000, 1_130_000).unwrap();
        assert_eq!(
            m.frame(),
            Disposition::Suppress,
            "completion alone would falsely pass this block"
        );
        assert!(m.remember_read(2, 1_120_000, 1_140_000, 1_129_999).is_err());
    }
    #[test]
    fn overlapping_read_intervals_and_sequence_overflow_are_invalid() {
        let mut m = mask();
        m.remember_read(1, 1_000_000, 1_010_000, 1_010_000).unwrap();
        assert!(m.remember_read(2, 1_009_999, 1_020_000, 1_020_000).is_err());
        let mut m = mask();
        m.remember_read(u64::MAX, 1_000_000, 1_010_000, 1_010_000)
            .unwrap();
        assert!(m.remember_read(1, 1_010_000, 1_020_000, 1_020_000).is_err());
    }
    #[test]
    fn explicit_reset_allows_only_exact_next_epoch_and_keeps_pending_rejection() {
        let (owner, _) = owners();
        let mut m = mask();
        m.dispatched(owner, 1, 1_000_000, 240).unwrap();
        m.accepted(
            owner,
            1,
            cursor(480),
            cursor(600),
            120,
            1_001_000,
            1_001_000,
            1_001_000,
        )
        .unwrap();
        let previous = QueuedRange {
            epoch: 1,
            retired_through: 480,
            accepted_through: 600,
        };
        m.clock_reset(previous, 2, 1_010_000, 1_010_000).unwrap();
        m.rejected(1, 1_011_000).unwrap(); // A partial command retains its explicit obligation.
        m.remember_read(1, 1_020_000, 1_030_000, 1_030_000).unwrap();
        assert_eq!(m.frame(), Disposition::Suppress);
        assert!(m.clock_reset(previous, 2, 1_010_000, 1_040_000).is_err());
        m.dispatched(owner, 2, 1_130_000, 240).unwrap();
        m.accepted(
            owner,
            2,
            PlaybackCursor {
                epoch: 2,
                frame: 480,
            },
            PlaybackCursor {
                epoch: 2,
                frame: 720,
            },
            240,
            1_131_000,
            1_131_000,
            1_131_000,
        )
        .unwrap();
        assert!(!m.available(1_140_000));
    }
    #[test]
    fn arbitrary_or_future_reset_cannot_authorize_new_epoch() {
        let (owner, _) = owners();
        for case in 0..3 {
            let mut m = mask();
            m.dispatched(owner, 1, 1_000_000, 240).unwrap();
            m.accepted(
                owner,
                1,
                cursor(480),
                cursor(720),
                240,
                1_001_000,
                1_001_000,
                1_001_000,
            )
            .unwrap();
            let previous = QueuedRange {
                epoch: if case == 0 { 2 } else { 1 },
                retired_through: 480,
                accepted_through: 720,
            };
            assert!(
                m.clock_reset(
                    previous,
                    if case == 0 {
                        3
                    } else if case == 1 {
                        4
                    } else {
                        2
                    },
                    if case == 2 { 1_020_000 } else { 1_010_000 },
                    1_011_000
                )
                .is_err()
            );
        }
        let mut m = mask();
        m.dispatched(owner, 1, 1_000_000, 240).unwrap();
        m.accepted(
            owner,
            1,
            cursor(480),
            cursor(720),
            240,
            1_001_000,
            1_001_000,
            1_001_000,
        )
        .unwrap();
        m.drained(owner, 1, cursor(720), 1_002_000, 1_002_000)
            .unwrap();
        m.dispatched(owner, 2, 1_150_000, 240).unwrap();
        assert!(
            m.accepted(
                owner,
                2,
                PlaybackCursor {
                    epoch: 2,
                    frame: 480
                },
                PlaybackCursor {
                    epoch: 2,
                    frame: 720
                },
                240,
                1_151_000,
                1_151_000,
                1_151_000
            )
            .is_err()
        );
    }
    #[test]
    fn late_rejection_cannot_erase_the_original_dispatch_deadline() {
        let (owner, _) = owners();
        let mut m = mask();
        m.dispatched(owner, 1, 1_000_000, 240).unwrap();
        assert!(m.rejected(1, 1_200_000).is_err());
    }
    #[test]
    fn retained_prefix_requires_matching_unexpired_original_read_evidence() {
        let mut m = mask();
        m.remember_read(1, 1_000_000, 1_010_000, 1_010_000).unwrap();
        let mut frame = ObservedAudio {
            sequence: 1,
            captured_at_us: 1_010_000,
            samples: [0; 160],
        };
        assert_eq!(m.retained_frame(&frame).unwrap(), Disposition::Pass);
        frame.captured_at_us += 1;
        assert!(m.retained_frame(&frame).is_err());
        frame.captured_at_us -= 1;
        frame.sequence = 2;
        assert!(m.retained_frame(&frame).is_err());
        frame.sequence = 1;
        m.advance(1_600_001).unwrap();
        assert!(m.retained_frame(&frame).is_err());
    }

    use lamp_audio::evidence::{EchoMatch, Level};

    /// Evidence for a block: render blocks since the lamp last sounded, and
    /// the shares the observer reports when it has an alignment.
    fn evidence(quiet: u8, shares: Option<(u8, u8)>) -> EchoEvidence {
        let level = Level::from_tenths_of_db(-200);
        EchoEvidence {
            blocks: 1_000,
            render_active_blocks: if quiet == 0 { 50 } else { 0 },
            render_quiet_blocks: quiet,
            capture_level: level,
            residual_level: level,
            echo: shares.map(|(capture_explained, residual_playback_floor)| EchoMatch {
                render_level: level,
                capture_explained,
                residual_explained: 10,
                residual_playback_floor,
                alignment_age_blocks: 0,
                render_block_level: Level::FLOOR,
                render_drop: 0,
            }),
            silent_at_echo: None,
        }
    }
    const OWN: Option<(u8, u8)> = Some((97, 10));
    const OVER: Option<(u8, u8)> = Some((40, 0));

    /// Hear `count` blocks alike, from `*sequence` on. Returns the steps that
    /// were not `Carry`, with the sequence each came at.
    fn hear(
        mask: &mut PlaybackMask,
        sequence: &mut u64,
        count: u64,
        probability: f32,
        evidence: &EchoEvidence,
        speaking: bool,
    ) -> Vec<(u64, Step)> {
        let mut steps = Vec::new();
        for _ in 0..count {
            let listened = mask
                .listen(
                    *sequence,
                    probability,
                    Some(evidence),
                    speaking,
                    *sequence * 10_000,
                )
                .unwrap();
            if listened.step != Step::Carry {
                steps.push((*sequence, listened.step));
            }
            *sequence += 1;
        }
        steps
    }

    /// Blocks over the lamp that make it yield in the first second it has
    /// something to say, which is where these tests take place.
    const ONSET: u64 = Confirmation::EVALUATED.early_onset_blocks as u64;
    /// The first blocks of such a run that the detector is not told about.
    const ONSET_LEAD: u8 = Confirmation::EVALUATED.early_onset_blocks - START_BLOCKS as u8;

    fn disposition(mask: &PlaybackMask, trigger: u64) -> Option<Disposition> {
        PlaybackMask::heard_disposition(mask.duplex.as_ref().unwrap(), trigger - 5, trigger)
    }

    #[test]
    fn the_duplex_mask_scores_the_lamp_as_silence_and_a_person_as_speech() {
        let mut mask = PlaybackMask::duplex();
        mask.assume_matched();
        assert!(mask.is_duplex() && mask.enabled());
        let own = mask
            .listen(1, 0.99, Some(&evidence(0, OWN)), true, 10_000)
            .unwrap();
        assert_eq!((own.heard, own.score), (Some(Heard::Own), 0.0));
        // A person over a sound that is no reply is speech at once.
        let over = mask
            .listen(2, 0.55, Some(&evidence(0, OVER)), false, 2 * 10_000)
            .unwrap();
        assert_eq!((over.heard, over.score), (Some(Heard::Over), 1.0));
        let clear = mask
            .listen(3, 0.62, Some(&evidence(40, None)), false, 3 * 10_000)
            .unwrap();
        assert_eq!((clear.heard, clear.score), (Some(Heard::Clear), 0.62));
        // Heard blocks are never masked: the score is the protection.
        assert_eq!(mask.frame(), Disposition::Pass);
        // The plain mask hears nothing and changes no score.
        let mut plain = PlaybackMask::new(TurnTaking::V1Baseline);
        let unjudged = plain
            .listen(1, 0.99, Some(&evidence(0, OVER)), true, 10_000)
            .unwrap();
        assert_eq!(
            (unjudged.heard, unjudged.score, unjudged.step),
            (None, 0.99, Step::Carry)
        );
        assert!(!plain.yielded(10_000) && plain.duplex_summary().is_none());
    }

    /// #828, listening gaps: speech in the lamp's own silence (the render at
    /// the echo's alignment silent, a fresh alignment) makes the lamp yield
    /// with the switch, as speech over it does; without it, it says nothing
    /// and nothing yields.
    #[test]
    fn speech_in_a_listening_gap_yields_only_with_the_switch() {
        let silent = EchoEvidence {
            silent_at_echo: Some(5),
            ..evidence(5, None)
        };
        for on in [true, false] {
            let mut mask = PlaybackMask::duplex();
            mask.assume_matched();
            if on {
                mask.listen_in_gaps();
            }
            let mut yielded = false;
            for sequence in 1..=ONSET + 2 {
                let listened = mask
                    .listen(sequence, 0.9, Some(&silent), true, sequence * 10_000)
                    .unwrap();
                yielded |= listened.step == Step::Yield;
            }
            assert_eq!(yielded, on, "switch {on}");
        }
        // Only with ears: the plain mask has no gaps to listen in.
        let mut plain = PlaybackMask::new(TurnTaking::V1Baseline);
        plain.listen_in_gaps();
        assert!(plain.listen_gaps().is_none());
    }

    #[test]
    fn the_detector_hears_a_person_over_a_reply_so_that_its_start_ends_on_the_yield() {
        let mut mask = PlaybackMask::duplex();
        mask.assume_matched();
        let mut heard = Vec::new();
        for sequence in 1..=ONSET + 2 {
            let listened = mask
                .listen(
                    sequence,
                    0.55,
                    Some(&evidence(0, OVER)),
                    true,
                    sequence * 10_000,
                )
                .unwrap();
            heard.push((listened.score, listened.step));
        }
        // Nothing for the first four blocks, then the detector's own six:
        // the sixth is the block that yields. After it, speech as before.
        let (lead, start) = heard.split_at(usize::from(ONSET_LEAD));
        assert!(lead.iter().all(|heard| *heard == (0.0, Step::Carry)));
        assert_eq!(start.len(), usize::from(START_BLOCKS) + 2);
        assert!(start[..5].iter().all(|heard| *heard == (1.0, Step::Carry)));
        assert_eq!(start[5], (1.0, Step::Yield));
        assert!(start[6..].iter().all(|heard| *heard == (1.0, Step::Carry)));
        // A run that stops short was never speech to the detector for six
        // blocks in a row.
        let mut mask = PlaybackMask::duplex();
        mask.assume_matched();
        let mut sequence = 1;
        let mut longest = 0;
        let mut run = 0;
        for round in 0..3 {
            for block in 0..ONSET {
                let own = block == ONSET - 1;
                let shares = if own { OWN } else { OVER };
                let listened = mask
                    .listen(
                        sequence,
                        0.9,
                        Some(&evidence(0, shares)),
                        true,
                        sequence * 10_000,
                    )
                    .unwrap();
                assert_eq!(listened.step, Step::Carry, "round {round}");
                run = if listened.score >= 0.8 { run + 1 } else { 0 };
                longest = longest.max(run);
                sequence += 1;
            }
        }
        assert_eq!(longest, u64::from(START_BLOCKS) - 1);
    }

    #[test]
    fn a_second_into_a_reply_the_shorter_run_stops_it_and_the_detector_starts_on_the_yield() {
        let rule = Confirmation::EVALUATED;
        let mut mask = PlaybackMask::duplex();
        mask.assume_matched();
        let mut sequence = 1;
        assert_eq!(
            hear(
                &mut mask,
                &mut sequence,
                u64::from(rule.early_blocks),
                0.9,
                &evidence(0, OWN),
                true
            ),
            []
        );
        let late = usize::from(rule.onset_blocks);
        let mut heard = Vec::new();
        for _ in 0..late {
            let listened = mask
                .listen(
                    sequence,
                    0.9,
                    Some(&evidence(0, OVER)),
                    true,
                    sequence * 10_000,
                )
                .unwrap();
            heard.push((listened.score, listened.step));
            sequence += 1;
        }
        let lead = late - usize::from(START_BLOCKS);
        assert!(
            heard[..lead]
                .iter()
                .all(|heard| *heard == (0.0, Step::Carry))
        );
        assert!(
            heard[lead..late - 1]
                .iter()
                .all(|heard| *heard == (1.0, Step::Carry))
        );
        assert_eq!(heard[late - 1], (1.0, Step::Yield));
    }

    #[test]
    fn nothing_is_a_person_over_the_lamp_until_its_own_sound_has_been_found_in_this_reply() {
        // As on lamp-4ace: the render explains the first two blocks of a
        // reply and then none of the next twenty, through which the lamp's
        // own first word is scored as speech.
        let mut mask = PlaybackMask::duplex();
        let mut sequence = 1;
        let unexplained = evidence(0, Some((0, 0)));
        assert_eq!(
            hear(&mut mask, &mut sequence, 2, 0.9, &evidence(0, OWN), true),
            []
        );
        for _ in 0..40 {
            let listened = mask
                .listen(sequence, 0.9, Some(&unexplained), true, sequence * 10_000)
                .unwrap();
            assert_eq!(
                (listened.heard, listened.score, listened.step),
                (Some(Heard::Unknown), 0.0, Step::Carry)
            );
            sequence += 1;
        }
        assert_eq!(mask.duplex_summary(), Some(((0, 0, 0), false)));
        // Once the render has explained enough of the reply, a person is
        // heard over it as always.
        hear(
            &mut mask,
            &mut sequence,
            u64::from(MATCHED_BLOCKS) - 3,
            0.9,
            &evidence(0, OWN),
            true,
        );
        assert_eq!(
            hear(&mut mask, &mut sequence, ONSET, 0.9, &unexplained, true),
            []
        );
        hear(&mut mask, &mut sequence, 1, 0.9, &evidence(0, OWN), true);
        let steps = hear(
            &mut mask,
            &mut sequence,
            ONSET,
            0.9,
            &evidence(0, OVER),
            true,
        );
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].1, Step::Yield);
        // A reply that goes on after a pause has to be found again.
        hear(&mut mask, &mut sequence, 30, 0.1, &evidence(10, None), true);
        let steps = hear(&mut mask, &mut sequence, 40, 0.1, &evidence(40, None), true);
        assert_eq!(steps.last().unwrap().1, Step::Resume);
        assert_eq!(
            hear(&mut mask, &mut sequence, 30, 0.9, &unexplained, true),
            []
        );
        // And so has the next reply, after there was nothing to yield.
        let mut mask = PlaybackMask::duplex();
        mask.assume_matched();
        let mut sequence = 1;
        hear(&mut mask, &mut sequence, 1, 0.1, &evidence(40, None), false);
        assert_eq!(
            hear(&mut mask, &mut sequence, 30, 0.9, &unexplained, true),
            []
        );
    }

    #[test]
    fn without_evidence_the_duplex_mask_is_the_conservative_mask() {
        let mut mask = PlaybackMask::duplex();
        mask.assume_matched();
        let blind = mask.listen(1, 0.99, None, true, 10_000).unwrap();
        assert_eq!(
            (blind.heard, blind.score, blind.step),
            (None, 0.99, Step::Carry)
        );
        assert!(mask.hearing().is_none(), "the baseline rules decide");
        // Evidence for the next block and the mask hears again.
        mask.listen(2, 0.99, Some(&evidence(0, OWN)), true, 2 * 10_000)
            .unwrap();
        assert!(mask.hearing().is_some());
        mask.listen(3, 0.99, None, true, 3 * 10_000).unwrap();
        assert!(mask.hearing().is_none());
    }

    #[test]
    fn a_candidate_that_began_over_the_lamp_waits_and_passes_when_the_person_goes_on() {
        let mut mask = PlaybackMask::duplex();
        mask.assume_matched();
        let mut sequence = 1;
        assert_eq!(
            hear(&mut mask, &mut sequence, 10, 0.9, &evidence(0, OWN), true),
            []
        );
        let steps = hear(
            &mut mask,
            &mut sequence,
            ONSET,
            0.9,
            &evidence(0, OVER),
            true,
        );
        let yielded = 10 + ONSET;
        assert_eq!(steps, [(yielded, Step::Yield)]);
        assert!(mask.yielded((sequence - 1) * 10_000));
        assert_eq!(
            disposition(&mask, yielded).unwrap(),
            Disposition::AwaitWriteEvidence
        );
        // The lamp's sound dies away with the person still speaking.
        assert_eq!(
            hear(&mut mask, &mut sequence, 30, 0.9, &evidence(10, None), true),
            []
        );
        assert_eq!(
            disposition(&mask, yielded).unwrap(),
            Disposition::AwaitWriteEvidence
        );
        let steps = hear(&mut mask, &mut sequence, 6, 0.9, &evidence(40, None), true);
        assert_eq!(steps, [(yielded + 36, Step::Commit)]);
        assert!(!mask.yielded((sequence - 1) * 10_000));
        assert_eq!(disposition(&mask, yielded).unwrap(), Disposition::Pass);
        assert_eq!(mask.duplex_summary(), Some(((1, 1, 0), false)));
    }

    #[test]
    fn a_short_word_heard_plainly_passes_though_nobody_goes_on() {
        // The render explains a tenth of the block: not the lamp.
        const PLAIN: Option<(u8, u8)> = Some((10, 0));
        let mut mask = PlaybackMask::duplex();
        mask.assume_matched();
        let mut sequence = 1;
        let word = u64::from(Confirmation::EVALUATED.plain_blocks);
        let steps = hear(
            &mut mask,
            &mut sequence,
            word,
            0.9,
            &evidence(0, PLAIN),
            true,
        );
        assert_eq!(steps, [(ONSET, Step::Yield)]);
        hear(&mut mask, &mut sequence, 30, 0.1, &evidence(10, None), true);
        assert_eq!(
            disposition(&mask, ONSET).unwrap(),
            Disposition::AwaitWriteEvidence
        );
        let steps = hear(&mut mask, &mut sequence, 40, 0.1, &evidence(40, None), true);
        assert_eq!(steps, [(word + 70, Step::Commit)]);
        assert_eq!(disposition(&mask, ONSET).unwrap(), Disposition::Pass);
        assert_eq!(mask.duplex_summary(), Some(((1, 1, 0), false)));
        // One plain block fewer and it is an acknowledgment: a pause.
        let mut mask = PlaybackMask::duplex();
        mask.assume_matched();
        let mut sequence = 1;
        hear(
            &mut mask,
            &mut sequence,
            ONSET - (word - 1),
            0.9,
            &evidence(0, OVER),
            true,
        );
        hear(
            &mut mask,
            &mut sequence,
            word - 1,
            0.9,
            &evidence(0, PLAIN),
            true,
        );
        hear(&mut mask, &mut sequence, 30, 0.1, &evidence(10, None), true);
        let steps = hear(&mut mask, &mut sequence, 40, 0.1, &evidence(40, None), true);
        assert_eq!(steps, [(ONSET + 70, Step::Resume)]);
    }

    #[test]
    fn a_candidate_over_the_lamp_is_dropped_when_nobody_goes_on() {
        let mut mask = PlaybackMask::duplex();
        mask.assume_matched();
        let mut sequence = 1;
        hear(
            &mut mask,
            &mut sequence,
            ONSET,
            0.9,
            &evidence(0, OVER),
            true,
        );
        assert_eq!(
            disposition(&mask, ONSET).unwrap(),
            Disposition::AwaitWriteEvidence
        );
        hear(&mut mask, &mut sequence, 30, 0.1, &evidence(10, None), true);
        let steps = hear(&mut mask, &mut sequence, 40, 0.1, &evidence(40, None), true);
        assert_eq!(steps, [(ONSET + 70, Step::Resume)]);
        assert_eq!(disposition(&mask, ONSET).unwrap(), Disposition::Suppress);
        assert_eq!(mask.duplex_summary(), Some(((1, 0, 1), false)));
    }

    /// #3967 (`LAMP_CUT_IN_NEAR_ASKER=on`): talk in the clear in a yield
    /// more than 6 dB under the asker (52e6's office bed, 29 Oct) resumes
    /// the answer; about as loud as them, or with no asker's level, it
    /// commits as before; and without the switch nothing changes.
    #[test]
    fn talk_well_under_the_asker_is_the_rooms_and_resumes_the_answer() {
        // The talk at -20 dBFS (`evidence`'s level).
        let cut_in = |switch: bool, asker: Option<f32>| {
            let mut mask = PlaybackMask::duplex();
            if switch {
                mask.cut_in_near_asker();
            }
            mask.assume_matched();
            mask.asker_level_db(asker);
            let mut sequence = 1;
            let mut steps = hear(
                &mut mask,
                &mut sequence,
                ONSET,
                0.9,
                &evidence(0, OVER),
                true,
            );
            hear(&mut mask, &mut sequence, 30, 0.1, &evidence(10, None), true);
            steps.extend(hear(
                &mut mask,
                &mut sequence,
                120,
                0.9,
                &evidence(40, None),
                true,
            ));
            (
                steps.into_iter().map(|(_, step)| step).collect::<Vec<_>>(),
                mask.under_asker_db(),
            )
        };
        // 8 dB under the asker: the room's, resumed at the yield's deadline.
        let (steps, under) = cut_in(true, Some(-12.0));
        assert_eq!(steps, [Step::Yield, Step::Resume]);
        assert!(
            under.is_some_and(|under| (under - 8.0).abs() < 0.01),
            "{under:?}"
        );
        // 2 dB under: someone as near as the asker, a cut-in.
        assert_eq!(cut_in(true, Some(-18.0)).0, [Step::Yield, Step::Commit]);
        // No asker's level known, or the switch off: as before.
        assert_eq!(cut_in(true, None).0, [Step::Yield, Step::Commit]);
        assert_eq!(cut_in(false, Some(-12.0)).0, [Step::Yield, Step::Commit]);
    }

    #[test]
    fn a_reply_that_twice_yields_to_nothing_stops_yielding_until_it_closes() {
        let mut mask = PlaybackMask::duplex();
        mask.assume_matched();
        let mut sequence = 1;
        for _ in 0..FRUITLESS_YIELDS {
            mask.assume_matched();
            let steps = hear(
                &mut mask,
                &mut sequence,
                ONSET,
                0.9,
                &evidence(0, OVER),
                true,
            );
            assert_eq!(steps.len(), 1);
            hear(&mut mask, &mut sequence, 30, 0.1, &evidence(10, None), true);
            let steps = hear(&mut mask, &mut sequence, 40, 0.1, &evidence(40, None), true);
            assert_eq!(steps.last().unwrap().1, Step::Resume);
        }
        assert!(mask.duplex_summary().unwrap().1);
        // The same evidence again: no yield, and the detector is told nothing.
        mask.assume_matched();
        let before = sequence;
        assert_eq!(
            hear(
                &mut mask,
                &mut sequence,
                ONSET + 6,
                0.9,
                &evidence(0, OVER),
                true
            ),
            []
        );
        let listened = mask
            .listen(
                sequence,
                0.9,
                Some(&evidence(0, OVER)),
                true,
                sequence * 10_000,
            )
            .unwrap();
        assert_eq!((listened.heard, listened.score), (Some(Heard::Over), 0.0));
        assert!(sequence > before && !mask.yielded((sequence - 1) * 10_000));
        // The next reply starts clean.
        mask.reply_closed();
        mask.assume_matched();
        sequence += 1;
        let steps = hear(
            &mut mask,
            &mut sequence,
            ONSET,
            0.9,
            &evidence(0, OVER),
            true,
        );
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].1, Step::Yield);
    }

    /// #1712 (the first builder's yes at 04:40): two fruitless yields, then
    /// 20 s of the reply with no yield forgives one, so a line over it
    /// yields again, and a person who goes on stops the story: the third
    /// cut-in of lamp-4ace's 20 Oct 03:45 story (the first builder on #1722,
    /// 21 Oct 16:2x). 10 s isn't enough.
    #[test]
    fn a_fruitless_yield_is_forgiven_after_twenty_seconds_with_no_yield() {
        let quiet_blocks = |seconds: u64| seconds * 100;
        for (after, yields) in [(20, true), (10, false)] {
            let mut mask = PlaybackMask::duplex();
            let mut sequence = 1;
            for _ in 0..FRUITLESS_YIELDS {
                mask.assume_matched();
                let steps = hear(
                    &mut mask,
                    &mut sequence,
                    ONSET,
                    0.9,
                    &evidence(0, OVER),
                    true,
                );
                assert_eq!(steps.len(), 1);
                hear(&mut mask, &mut sequence, 30, 0.1, &evidence(10, None), true);
                let steps = hear(&mut mask, &mut sequence, 40, 0.1, &evidence(40, None), true);
                assert_eq!(steps.last().unwrap().1, Step::Resume);
            }
            assert!(mask.duplex_summary().unwrap().1, "fallen back");
            // The lamp goes on speaking, nobody over it. The block that
            // forgives one says so, and the count keeps it (Eighth on #2048).
            let mut forgiven = 0;
            for _ in 0..quiet_blocks(after) {
                let listened = mask
                    .listen(
                        sequence,
                        0.1,
                        Some(&evidence(40, None)),
                        true,
                        sequence * 10_000,
                    )
                    .unwrap();
                forgiven += u32::from(listened.forgiven);
                sequence += 1;
            }
            assert_eq!(forgiven, u32::from(yields), "{after} s after");
            assert_eq!(mask.fruitless_forgiven(), forgiven);
            mask.assume_matched();
            let steps = hear(
                &mut mask,
                &mut sequence,
                ONSET + 6,
                0.9,
                &evidence(0, OVER),
                true,
            );
            assert_eq!(
                steps.iter().any(|(_, step)| *step == Step::Yield),
                yields,
                "{after} s after: {steps:?}"
            );
            if yields {
                // The person goes on as the lamp's sound dies away: the
                // third line stops the story.
                assert_eq!(
                    hear(&mut mask, &mut sequence, 30, 0.9, &evidence(10, None), true),
                    []
                );
                let steps = hear(&mut mask, &mut sequence, 6, 0.9, &evidence(40, None), true);
                assert_eq!(
                    steps.last().map(|(_, step)| *step),
                    Some(Step::Commit),
                    "{steps:?}"
                );
            }
        }
    }

    #[test]
    fn a_person_over_a_sound_that_is_no_reply_is_an_ordinary_candidate() {
        let mut mask = PlaybackMask::duplex();
        mask.assume_matched();
        let mut sequence = 1;
        // Nothing to yield: no reply has audio waiting.
        assert_eq!(
            hear(&mut mask, &mut sequence, 6, 0.9, &evidence(0, OVER), false),
            []
        );
        assert_eq!(disposition(&mask, 6).unwrap(), Disposition::Pass);
        // And in the clear a candidate has nothing to wait for.
        hear(&mut mask, &mut sequence, 6, 0.9, &evidence(40, None), false);
        assert_eq!(disposition(&mask, 12).unwrap(), Disposition::Pass);
    }

    #[test]
    fn a_gap_in_capture_forgets_what_was_heard_and_any_yield() {
        let mut mask = PlaybackMask::duplex();
        mask.assume_matched();
        let mut sequence = 1;
        hear(
            &mut mask,
            &mut sequence,
            ONSET,
            0.9,
            &evidence(0, OVER),
            true,
        );
        assert!(mask.yielded((sequence - 1) * 10_000));
        // The next block is missing.
        let after = ONSET + 2;
        mask.listen(after, 0.9, Some(&evidence(0, OWN)), true, after * 10_000)
            .unwrap();
        assert!(!mask.yielded(after * 10_000));
        assert_eq!(disposition(&mask, ONSET), None, "the baseline rules decide");
    }

    #[test]
    fn a_yield_whose_hearing_has_stopped_withholds_nothing() {
        let mut mask = PlaybackMask::duplex();
        mask.assume_matched();
        let mut sequence = 1;
        hear(
            &mut mask,
            &mut sequence,
            ONSET,
            0.9,
            &evidence(0, OVER),
            true,
        );
        let heard_at = ONSET * 10_000;
        assert!(mask.yielded(heard_at));
        assert!(mask.yielded(heard_at + HEARING_STALE_US));
        // A muted or stalled microphone: the reply is not held back.
        assert!(!mask.yielded(heard_at + HEARING_STALE_US + 1));
        // Hearing returns in order and the yield is still to be settled.
        mask.listen(
            ONSET + 1,
            0.9,
            Some(&evidence(1, None)),
            true,
            heard_at + 300_000,
        )
        .unwrap();
        assert!(mask.yielded(heard_at + 300_000));
        assert!(mask.listen(ONSET + 2, 0.9, None, true, heard_at).is_err());
    }

    #[test]
    fn a_reply_that_closes_during_a_yield_leaves_its_candidate_to_be_settled() {
        let mut mask = PlaybackMask::duplex();
        mask.assume_matched();
        let mut sequence = 1;
        hear(
            &mut mask,
            &mut sequence,
            ONSET,
            0.9,
            &evidence(0, OVER),
            true,
        );
        // The reply had nothing more to say and ends while the lamp waits.
        mask.reply_closed();
        assert_eq!(
            disposition(&mask, ONSET).unwrap(),
            Disposition::AwaitWriteEvidence
        );
        hear(
            &mut mask,
            &mut sequence,
            30,
            0.9,
            &evidence(10, None),
            false,
        );
        let steps = hear(&mut mask, &mut sequence, 6, 0.9, &evidence(40, None), false);
        assert_eq!(steps, [(ONSET + 36, Step::Commit)]);
        assert_eq!(disposition(&mask, ONSET).unwrap(), Disposition::Pass);
    }

    #[test]
    fn another_capture_stream_forgets_what_was_heard_and_any_yield() {
        let mut mask = PlaybackMask::duplex();
        mask.assume_matched();
        let mut sequence = 1;
        hear(
            &mut mask,
            &mut sequence,
            ONSET,
            0.9,
            &evidence(0, OVER),
            true,
        );
        assert!(mask.yielded(ONSET * 10_000));
        mask.clear_read_lineage();
        assert!(!mask.yielded(ONSET * 10_000));
        assert!(mask.hearing().is_none(), "the baseline rules decide");
        // The new stream may number its blocks from anywhere.
        let first = mask
            .listen(1, 0.9, Some(&evidence(0, OWN)), true, (ONSET + 1) * 10_000)
            .unwrap();
        assert_eq!(first.heard, Some(Heard::Own));
    }
    #[test]
    fn only_internally_aged_matching_retained_reads_have_the_expiry_type() {
        let mut m = mask();
        m.remember_read(1, 1_000_000, 1_010_000, 1_010_000).unwrap();
        m.remember_read(2, 1_590_000, 1_600_000, 1_610_001).unwrap();
        let exact = ObservedAudio {
            sequence: 1,
            captured_at_us: 1_010_000,
            samples: [0; 160],
        };
        let expired = m.retained_frame(&exact).unwrap_err();
        assert!(
            expired
                .get_ref()
                .is_some_and(|e| e.is::<RetainedHistoryLoss>())
        );
        for wrong in [
            ObservedAudio {
                captured_at_us: 1_010_001,
                ..exact
            },
            ObservedAudio {
                sequence: 3,
                ..exact
            },
        ] {
            let error = m.retained_frame(&wrong).unwrap_err();
            assert!(
                !error
                    .get_ref()
                    .is_some_and(|e| e.is::<RetainedHistoryLoss>())
            );
        }
        let invalid = m
            .remember_read(3, 1_020_000, 1_620_000, 1_610_001)
            .unwrap_err();
        assert!(
            !invalid
                .get_ref()
                .is_some_and(|e| e.is::<RetainedHistoryLoss>())
        );
    }
    #[test]
    fn cache_retirement_does_not_label_unknown_or_new_lineage_reads_as_expired() {
        let mut m = mask();
        for sequence in 100..=181 {
            let first = 1_000_000 + (sequence - 100) * 10_000;
            m.remember_read(sequence, first, first + 10_000, first + 10_000)
                .unwrap();
        }
        let evicted = ObservedAudio {
            sequence: 100,
            captured_at_us: 1_010_000,
            samples: [0; 160],
        };
        assert!(
            m.retained_frame(&evicted)
                .unwrap_err()
                .get_ref()
                .is_some_and(|e| matches!(
                    e.downcast_ref::<RetainedHistoryLoss>(),
                    Some(RetainedHistoryLoss::Evicted)
                ))
        );
        for unknown in [
            ObservedAudio {
                sequence: 99,
                ..evicted
            },
            ObservedAudio {
                captured_at_us: 1_010_001,
                ..evicted
            },
            ObservedAudio {
                captured_at_us: 2_000_000,
                ..evicted
            },
        ] {
            let e = m.retained_frame(&unknown).unwrap_err();
            assert!(!e.get_ref().is_some_and(|e| e.is::<RetainedHistoryLoss>()));
        }
        m.clear_read_lineage();
        let e = m.retained_frame(&evicted).unwrap_err();
        assert!(!e.get_ref().is_some_and(|e| e.is::<RetainedHistoryLoss>()));
    }

    #[test]
    fn heard_evidence_cannot_hide_a_cached_retained_timestamp_mismatch() {
        let mut m = PlaybackMask::duplex();
        m.listen(1, 0.1, Some(&evidence(40, None)), false, 1_020_000)
            .unwrap();
        m.remember_read(1, 1_000_000, 1_010_000, 1_020_000).unwrap();
        let original = ObservedAudio {
            sequence: 1,
            captured_at_us: 1_010_000,
            samples: [0; 160],
        };
        assert_eq!(m.retained_frame(&original).unwrap(), Disposition::Pass);
        let mismatched = ObservedAudio {
            captured_at_us: 1_009_999,
            ..original
        };
        let e = m.retained_frame(&mismatched).unwrap_err();
        assert!(!e.get_ref().is_some_and(|e| e.is::<RetainedHistoryLoss>()));
    }
}
