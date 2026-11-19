//! What the voice says about itself for the part that plans what the lamp
//! shows (`docs/expression.md`).
//!
//! Three things on the bus, each on the machine's own clock so that
//! another program can act on them to the hundredth of a second:
//!
//! - `voice/timing`: which turn is in hand and how far it has got
//!   (listening, thinking, speaking), whether the voice can keep what a
//!   person says, when the first sound of an answer is expected, and how
//!   each turn ended: cut off by someone, or over.
//! - `voice/activity`: whether the person being listened to is speaking at
//!   this moment, or has paused.
//! - `voice/playout`: each piece of the lamp's own sound, with when it
//!   reaches the loudspeaker and how loud it is.
//!
//! Nothing here decides anything. The rules for what is said are in
//! [`Timing`], which has no bus and takes its clock from the caller;
//! [`ExpressionOut`] puts what it says on the bus. Off unless the voice's
//! environment says `on`.

use crate::ring_wire::RingPhase;
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{
    ClockStamp, PersonActivity, Voice, VoiceEvent, VoicePlayout, VoiceProgress, VoiceSound,
    VoiceTiming,
};
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};

/// How many times this process has made its publishers. A mute ends the
/// conversation and the next one makes them again, with every sequence
/// counting from 1 once more (#7955).
static STARTS: AtomicU64 = AtomicU64::new(0);

/// The session the `n`th publishers of this process say: the first the
/// service's own `INVOCATION_ID`, each later one that and its number. A
/// publisher whose counts start again under the session of the one before
/// is refused by Expression as "duplicate or out of order" until it passes
/// the old count, which after a software mute was never (2,644 refusals in
/// 3.5 minutes on lamp-06c1, `asleep` gone until the voice restarted). A new
/// session starts Expression's marks again, never its time.
pub fn session_named(base: &str, n: u64) -> String {
    if n == 0 {
        base.to_owned()
    } else {
        format!("{base}-{n}")
    }
}

/// `on` for the three to be published. Anything else: nothing.
pub const ENVIRONMENT: &str = "LAMP_EXPRESSION_OUT";

/// A state that has not changed is said again this often. The topics
/// promise one every 100 ms.
pub const REPEAT_US: u64 = 80_000;
/// The least a foretold first sound moves before it is said again.
pub const RESCHEDULE_US: u64 = 20_000;
/// A first sound nearer than this is not foretold: it would be past by
/// the time anyone had read it.
pub const LEAST_NOTICE_US: u64 = 10_000;
/// The farthest ahead a first sound may be foretold, and a piece of sound
/// said (`lamp_msgs::EXPRESSION_MAX_FUTURE_US`), less a margin for the
/// reader's own clock reading.
pub const FARTHEST_US: u64 = lamp_msgs::EXPRESSION_MAX_FUTURE_US - 50_000;
/// How long the lamp is said to be listening after it was cut off, when
/// nobody then says anything it takes: after this it is at rest.
pub const CUT_OFF_LISTENING_US: u64 = 2_000_000;
/// The person is said to be speaking for this long after the last block
/// the detector called speech: blocks come every 10 ms, and a detector
/// wavers within a word.
pub const SPEECH_LASTS_US: u64 = 120_000;
/// Pieces of the lamp's sound are gathered to about this length.
pub const PIECE_US: u64 = 20_000;
/// How far ahead of now the voice says its own sound that is still held
/// before the speaker. On lamp-4ace on 2026-10-14 each piece said as the
/// speaker took it had at most 18 ms left, and Expression had refused 565
/// as over by the time it read them. Short enough that a pause for
/// someone, which stops the sound, leaves little said that never sounds.
pub const FORETELL_US: u64 = 200_000;
/// A piece that has waited this long for more is said as it is.
pub const PIECE_WAIT_US: u64 = 15_000;
/// A piece is said only if it has this much left to sound: on lamp-4ace
/// what is said reaches Expression some 20 ms later, and a piece over by
/// then is refused (2026-10-14: 565 such). The voice's sound said ahead
/// ([`Timing::foretell`]) has more than this; only the first and last few
/// milliseconds of an answer, taken by the speaker before or after it,
/// have less, and go unsaid.
pub const PIECE_LEAST_US: u64 = 25_000;
/// Pieces this close together are one stretch of sound.
const JOIN_US: u64 = 5_000;

/// One look at the voice, each pass of its loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seen {
    pub now_us: u64,
    /// The turn in hand, if there is one.
    pub turn: Option<u64>,
    /// How far that turn has got, as the ring is told.
    pub phase: Option<RingPhase>,
    /// The voice can keep what a person says now.
    pub accepting_input: bool,
    /// The lamp is holding its own sound back for someone speaking over it.
    pub yielding: bool,
    /// The newest sound was speech, by the voice's own detector.
    pub person_speaking: bool,
    /// When the first sound of the answer in hand is expected at the
    /// loudspeaker, while none of it has been played.
    pub first_sound_us: Option<u64>,
}

/// Something to put on the bus.
#[derive(Clone, Debug, PartialEq)]
pub enum Said {
    Timing(VoiceTiming),
    Activity(PersonActivity),
    Playout(VoicePlayout),
}

/// The turn in hand, as it has been said.
#[derive(Clone, Copy, Debug)]
struct Live {
    turn: u64,
    state: Voice,
    spoke: bool,
    yielding: bool,
    foretold: Option<u64>,
    /// The ring shows an earlier state than the one said.
    behind: bool,
}

/// What is to be said of the voice's timing at one moment.
#[derive(Clone, Copy, Debug)]
struct Saying {
    turn: u64,
    state: Voice,
    accepting_input: bool,
    event: VoiceEvent,
    first_audio_us: Option<u64>,
}

/// A stretch of the lamp's sound being gathered.
#[derive(Clone, Copy, Debug)]
struct Piece {
    turn: u64,
    start_us: u64,
    end_us: u64,
    level: u8,
    since_us: u64,
}

/// What the voice says of its timing, decided here and nowhere else.
#[derive(Debug)]
pub struct Timing {
    clock_id: String,
    session_id: String,
    sequences: [u64; 3],
    live: Option<Live>,
    /// The newest turn that is over, how it ended and when. Turn 0 is no
    /// turn: nothing has been heard since the voice started.
    ended: (u64, VoiceEvent, u64),
    said_timing: Option<(VoiceTiming, u64)>,
    said_activity: Option<(u64, bool, u64)>,
    piece: Option<Piece>,
    /// The end of the voice's own sound said so far, by the speaker's
    /// pieces or ahead of them.
    played_through_us: u64,
    /// States the voice showed that could not be said: a turn's state
    /// never goes back, and these would have.
    pub held_back: u64,
}

fn rank(state: Voice) -> u8 {
    match state {
        Voice::Resting => 0,
        Voice::Listening => 1,
        Voice::Thinking => 2,
        Voice::Speaking => 3,
    }
}

impl Timing {
    /// `clock_id` names this start of the machine and `session_id` this
    /// start of the voice.
    pub fn new(clock_id: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            clock_id: clock_id.into(),
            session_id: session_id.into(),
            sequences: [0; 3],
            live: None,
            ended: (0, VoiceEvent::State, 0),
            said_timing: None,
            said_activity: None,
            piece: None,
            played_through_us: 0,
            held_back: 0,
        }
    }

    fn stamp(&mut self, stream: usize, now_us: u64) -> ClockStamp {
        self.sequences[stream] += 1;
        ClockStamp {
            clock_id: self.clock_id.clone(),
            at_us: now_us,
            sequence: self.sequences[stream],
        }
    }

    /// Say this if it differs from the last thing said or that was long
    /// enough ago. A foretold first sound is always said, and the state
    /// that follows it is the same state, said again in its own time.
    fn timing(&mut self, now_us: u64, saying: Saying, out: &mut Vec<Said>) {
        let same = self.said_timing.as_ref().is_some_and(|(said, at)| {
            let said_event = match said.event {
                VoiceEvent::Scheduled => VoiceEvent::State,
                event => event,
            };
            said.turn == saying.turn
                && said.state == saying.state
                && said.accepting_input == saying.accepting_input
                && said_event == saying.event
                && now_us.saturating_sub(*at) < REPEAT_US
        });
        if same {
            return;
        }
        let timing = VoiceTiming {
            stamp: self.stamp(0, now_us),
            session_id: self.session_id.clone(),
            turn: saying.turn,
            request: saying.turn,
            state: saying.state,
            accepting_input: saying.accepting_input,
            event: saying.event,
            first_audio_us: saying.first_audio_us,
        };
        self.said_timing = Some((timing.clone(), now_us));
        out.push(Said::Timing(timing));
    }

    /// The turn in hand is over. `followed`: another has begun already.
    fn end(&mut self, now_us: u64, accepting_input: bool, followed: bool, out: &mut Vec<Said>) {
        let Some(live) = self.live.take() else {
            return;
        };
        self.piece = None;
        // Cut off by someone, or over. A turn nothing was said to is over
        // like any other: in an office that is most turns, and the planner
        // takes `Cancelled` for something gone wrong, asks nothing of the
        // body or the light until the next turn and reports a warning.
        let event = if live.spoke && live.yielding {
            VoiceEvent::Interrupted
        } else {
            VoiceEvent::Finished
        };
        let state = if followed || event == VoiceEvent::Interrupted {
            Voice::Listening
        } else {
            Voice::Resting
        };
        self.ended = (live.turn, event, now_us);
        let saying = Saying {
            turn: live.turn,
            state,
            accepting_input,
            event,
            first_audio_us: None,
        };
        self.timing(now_us, saying, out);
    }

    /// One pass of the voice's loop. What to put on the bus, in order.
    pub fn tick(&mut self, seen: Seen) -> Vec<Said> {
        let mut out = Vec::new();
        let now_us = seen.now_us;
        // A turn number that is not newer than one already over is that
        // turn's last traces, not a turn.
        let turn = seen.turn.filter(|turn| {
            *turn > self.ended.0 || self.live.as_ref().is_some_and(|live| live.turn == *turn)
        });
        if let Some(live) = self.live.as_ref()
            && turn != Some(live.turn)
        {
            self.end(now_us, seen.accepting_input, turn.is_some(), &mut out);
        }
        let Some(turn) = turn else {
            let (turn, event, since) = self.ended;
            let (state, event) = match event {
                // Cut off, and nobody has said anything the voice took:
                // it listens for a moment and then rests.
                VoiceEvent::Interrupted if now_us.saturating_sub(since) < CUT_OFF_LISTENING_US => {
                    (Voice::Listening, VoiceEvent::Interrupted)
                }
                VoiceEvent::Interrupted => (Voice::Resting, VoiceEvent::State),
                event => (Voice::Resting, event),
            };
            let saying = Saying {
                turn,
                state,
                accepting_input: seen.accepting_input,
                event,
                first_audio_us: None,
            };
            self.timing(now_us, saying, &mut out);
            if let Some(piece) = self.flush(now_us) {
                out.push(piece);
            }
            return out;
        };
        let shown = match seen.phase {
            Some(RingPhase::Waiting) => Voice::Thinking,
            Some(RingPhase::Speaking) => Voice::Speaking,
            // A turn with no phase yet is being heard.
            Some(RingPhase::Listening) | None => Voice::Listening,
        };
        let live = self.live.get_or_insert(Live {
            turn,
            state: shown,
            spoke: false,
            yielding: false,
            foretold: None,
            behind: false,
        });
        // A turn goes from listening to thinking to speaking and never
        // back: a reader takes a step back as the end of its sound.
        if rank(shown) >= rank(live.state) {
            live.state = shown;
            live.behind = false;
        } else if seen.phase.is_some() && !live.behind {
            live.behind = true;
            self.held_back += 1;
        }
        live.spoke |= live.state == Voice::Speaking;
        live.yielding = seen.yielding;
        let state = live.state;
        let foretell = seen.first_sound_us.filter(|at| {
            state == Voice::Thinking
                && *at >= now_us.saturating_add(LEAST_NOTICE_US)
                && *at - now_us <= FARTHEST_US
                && live
                    .foretold
                    .is_none_or(|said| said.abs_diff(*at) >= RESCHEDULE_US)
        });
        if let Some(at) = foretell {
            live.foretold = Some(at);
        }
        let event = if foretell.is_some() {
            VoiceEvent::Scheduled
        } else {
            VoiceEvent::State
        };
        let saying = Saying {
            turn,
            state,
            accepting_input: seen.accepting_input,
            event,
            first_audio_us: foretell,
        };
        self.timing(now_us, saying, &mut out);
        // Whether the person is speaking is said only of a turn that is
        // being heard by a voice that can keep it.
        if state == Voice::Listening && seen.accepting_input {
            let same = self.said_activity.is_some_and(|(said, speaking, at)| {
                said == turn
                    && speaking == seen.person_speaking
                    && now_us.saturating_sub(at) < REPEAT_US
            });
            if !same {
                self.said_activity = Some((turn, seen.person_speaking, now_us));
                out.push(Said::Activity(PersonActivity {
                    stamp: self.stamp(1, now_us),
                    session_id: self.session_id.clone(),
                    turn,
                    request: turn,
                    speaking: seen.person_speaking,
                }));
            }
        }
        if let Some(piece) = self.flush(now_us) {
            out.push(piece);
        }
        out
    }

    /// The voice's own sound for `turn` that is still held before the
    /// speaker: `ahead`, at the render rate, sounds from `from_us` on,
    /// straight after the speaker's queue. Each whole piece of it that
    /// begins before [`FORETELL_US`] from now is said once, before it
    /// sounds; the speaker's pieces of the same stretch are then not said
    /// again. Nothing while the voice holds back for someone, since what
    /// it holds may never sound. Call after [`Self::played`].
    pub fn foretell(&mut self, turn: u64, from_us: u64, ahead: &[i16], now_us: u64) -> Vec<Said> {
        let mut said = Vec::new();
        if !self.live.as_ref().is_some_and(|live| {
            live.turn == turn && live.state == Voice::Speaking && !live.yielding
        }) {
            return said;
        }
        if let Some(piece) = self.say_piece(now_us) {
            said.push(piece);
        }
        let rate = u64::from(lamp_audio::RENDER_RATE);
        let per = (PIECE_US * rate / 1_000_000) as usize;
        let at = |index: usize| from_us + index as u64 * 1_000_000 / rate;
        // The first sample not yet said, rounded up to a whole sample.
        let mut index =
            (self.played_through_us.saturating_sub(from_us) * rate).div_ceil(1_000_000) as usize;
        let horizon = now_us.saturating_add(FORETELL_US.min(FARTHEST_US));
        while index + per <= ahead.len() && at(index) < horizon {
            let (start_us, end_us) = (at(index), at(index + per));
            let level = crate::ring_face::level_of(&ahead[index..index + per]);
            said.push(Said::Playout(VoicePlayout {
                stamp: self.stamp(2, now_us),
                session_id: self.session_id.clone(),
                turn,
                request: turn,
                start_us,
                end_us,
                level: f32::from(level) / 255.0,
            }));
            self.played_through_us = end_us;
            index += per;
        }
        said
    }

    /// The speaker took a piece of `turn`'s sound that reaches the
    /// loudspeaker from `start_us` to `end_us`, this loud (0 to 255, as
    /// the ring's face has it). Call after [`Self::tick`] in the same
    /// pass. A stretch that is ready to be said, if one is.
    pub fn played(
        &mut self,
        turn: u64,
        start_us: u64,
        end_us: u64,
        level: u8,
        now_us: u64,
    ) -> Option<Said> {
        // Only the turn in hand, once it has been said to be speaking.
        if !self
            .live
            .as_ref()
            .is_some_and(|live| live.turn == turn && live.state == Voice::Speaking)
        {
            return None;
        }
        let floor = self
            .piece
            .map_or(self.played_through_us, |piece| piece.end_us);
        let start_us = start_us.max(floor);
        if end_us <= start_us {
            return None;
        }
        match self.piece.as_mut() {
            Some(piece)
                if piece.turn == turn
                    && start_us - piece.end_us <= JOIN_US
                    && end_us - piece.start_us <= 2 * PIECE_US =>
            {
                piece.end_us = end_us;
                piece.level = piece.level.max(level);
                if piece.end_us - piece.start_us >= PIECE_US {
                    return self.say_piece(now_us);
                }
                None
            }
            _ => {
                let before = self.say_piece(now_us);
                self.piece = Some(Piece {
                    turn,
                    start_us,
                    end_us,
                    level,
                    since_us: now_us,
                });
                before
            }
        }
    }

    /// A stretch that has waited long enough for more.
    fn flush(&mut self, now_us: u64) -> Option<Said> {
        self.piece
            .is_some_and(|piece| now_us.saturating_sub(piece.since_us) >= PIECE_WAIT_US)
            .then(|| self.say_piece(now_us))
            .flatten()
    }

    fn say_piece(&mut self, now_us: u64) -> Option<Said> {
        let piece = self.piece.take()?;
        self.played_through_us = self.played_through_us.max(piece.end_us);
        // Gone already, or dated absurdly far ahead: not worth a reader's
        // refusal.
        if piece.end_us < now_us.saturating_add(PIECE_LEAST_US)
            || piece.start_us > now_us.saturating_add(FARTHEST_US)
        {
            return None;
        }
        Some(Said::Playout(VoicePlayout {
            stamp: self.stamp(2, now_us),
            session_id: self.session_id.clone(),
            turn: piece.turn,
            request: piece.turn,
            start_us: piece.start_us,
            end_us: piece.end_us,
            level: f32::from(piece.level) / 255.0,
        }))
    }
}

/// What the voice says for a planner that understands a turn held back
/// for someone and resumed (`voice/progress`, `voice/sound`;
/// docs/expression.md, "Held turns: the protocol First must publish"),
/// made from what it says on the first pair. Each turn's sound has a
/// generation, from 1; holding back for someone drops what was not yet
/// said and starts the next; while held every heartbeat says so, as
/// Listening; the first sound after a resume follows a heartbeat that
/// says it is no longer held, and none from before the resume is said.
#[derive(Debug, Default)]
pub struct Resumable {
    turn: u64,
    generation: u64,
    held: bool,
    /// The newest timing said on the first pair, for heartbeats of its own.
    last: Option<VoiceTiming>,
    /// What the newest progress said: held or not.
    said_held: Option<bool>,
    /// When the turn was last resumed: no sound from before it is said.
    resumed_at_us: u64,
    sequences: [u64; 2],
}

/// One message of the second pair.
#[derive(Clone, Debug, PartialEq)]
pub enum ResumableSaid {
    Progress(VoiceProgress),
    Sound(VoiceSound),
}

impl Resumable {
    fn stamp(&mut self, stream: usize, clock_id: &str, now_us: u64) -> ClockStamp {
        self.sequences[stream] += 1;
        ClockStamp {
            clock_id: clock_id.to_owned(),
            at_us: now_us,
            sequence: self.sequences[stream],
        }
    }

    fn progress(&mut self, timing: &VoiceTiming, now_us: u64) -> VoiceProgress {
        let terminal = matches!(
            timing.event,
            VoiceEvent::Interrupted | VoiceEvent::Cancelled | VoiceEvent::Finished
        );
        let held = self.held && !terminal;
        let mut timing = timing.clone();
        timing.stamp = self.stamp(0, &timing.stamp.clock_id, now_us);
        if held {
            timing.state = Voice::Listening;
            timing.event = VoiceEvent::State;
            timing.first_audio_us = None;
        }
        self.said_held = Some(held);
        VoiceProgress {
            timing,
            audio_generation: self.generation.max(1),
            held,
        }
    }

    /// Whether the lamp is holding its sound back for someone, each pass.
    /// On the edge into a hold the next generation begins and a held
    /// heartbeat is said at once.
    pub fn holding(&mut self, yielding: bool, now_us: u64) -> Option<ResumableSaid> {
        let speaking = self
            .last
            .as_ref()
            .is_some_and(|t| t.state == Voice::Speaking && t.event == VoiceEvent::State);
        if yielding && !self.held && speaking {
            self.held = true;
            self.generation = self.generation.max(1) + 1;
            let last = self.last.clone()?;
            return Some(ResumableSaid::Progress(self.progress(&last, now_us)));
        }
        if !yielding && self.held {
            self.held = false;
            self.resumed_at_us = now_us;
        }
        None
    }

    /// A timing said on the first pair: its progress on the second.
    pub fn timing(&mut self, timing: &VoiceTiming, now_us: u64) -> ResumableSaid {
        if timing.turn != self.turn {
            self.turn = timing.turn;
            self.generation = 1;
            self.held = false;
            self.resumed_at_us = 0;
        }
        self.last = Some(timing.clone());
        ResumableSaid::Progress(self.progress(timing, now_us))
    }

    /// A piece of sound said on the first pair: nothing while held or from
    /// before the resume; after a resume, first the heartbeat that says so.
    pub fn sound(&mut self, playout: &VoicePlayout, now_us: u64) -> Vec<ResumableSaid> {
        let mut said = Vec::new();
        if self.held || playout.turn != self.turn || playout.start_us < self.resumed_at_us {
            return said;
        }
        if self.said_held == Some(true)
            && let Some(last) = self.last.clone()
        {
            said.push(ResumableSaid::Progress(self.progress(&last, now_us)));
        }
        let mut playout = playout.clone();
        playout.stamp = self.stamp(1, &playout.stamp.clock_id, now_us);
        said.push(ResumableSaid::Sound(VoiceSound {
            playout,
            audio_generation: self.generation.max(1),
        }));
        said
    }
}

/// The three publishers.
pub struct ExpressionOut {
    timing: Timing,
    to_timing: Publisher,
    to_activity: Publisher,
    to_playout: Publisher,
    /// The second pair, for a planner that understands a held turn.
    to_progress: Publisher,
    to_sound: Publisher,
    resumable: Resumable,
    /// How long after the speaker's queue was looked at a piece written
    /// then begins to sound: the newest measured, for foretelling the
    /// first sound of the next answer.
    ahead_us: u64,
    /// The turn whose sound the speaker last took, and when its queue
    /// then ran out: where the voice's held sound begins.
    queue_end: Option<(u64, u64)>,
    refused: u64,
}

/// Until the speaker's queue has been measured in this run: the 30 ms of
/// silence it is kept primed with on lamp-4ace.
const AHEAD_AT_FIRST_US: u64 = 30_000;

impl ExpressionOut {
    /// The publishers, if the environment says `on` and both the bus and
    /// this machine's boot identity can be had. Otherwise it is said once
    /// and the voice goes on without.
    pub fn from_environment() -> Option<Self> {
        if std::env::var(ENVIRONMENT).ok().as_deref() != Some("on") {
            return None;
        }
        let made = Bus::from_environment().and_then(|bus| {
            let clock_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
            let base = std::env::var("INVOCATION_ID").unwrap_or_else(|_| {
                format!("voice-{}-{}", std::process::id(), lamp_ipc::monotonic_us())
            });
            let session_id = session_named(&base, STARTS.fetch_add(1, Ordering::Relaxed));
            Self::on(&bus, clock_id.trim(), &session_id)
        });
        match made {
            Ok(out) => Some(out),
            Err(error) => {
                eprintln!("lamp-voice: its timing is not published for expression: {error}");
                None
            }
        }
    }

    pub fn on(bus: &Bus, clock_id: &str, session_id: &str) -> io::Result<Self> {
        let source = crate::bus_out::SOURCE;
        Ok(Self {
            timing: Timing::new(clock_id, session_id),
            to_timing: bus.publisher_declared(
                VoiceTiming::TOPIC,
                &VoiceTiming::declaration(),
                source,
            )?,
            to_activity: bus.publisher_declared(
                PersonActivity::TOPIC,
                &PersonActivity::declaration(),
                source,
            )?,
            to_playout: bus.publisher_declared(
                VoicePlayout::TOPIC,
                &VoicePlayout::declaration(),
                source,
            )?,
            to_progress: bus.publisher_declared(
                VoiceProgress::TOPIC,
                &VoiceProgress::declaration(),
                source,
            )?,
            to_sound: bus.publisher_declared(
                VoiceSound::TOPIC,
                &VoiceSound::declaration(),
                source,
            )?,
            resumable: Resumable::default(),
            ahead_us: AHEAD_AT_FIRST_US,
            queue_end: None,
            refused: 0,
        })
    }

    /// How long after a look at the speaker's queue a piece written then
    /// begins to sound, as last measured.
    pub fn ahead_us(&self) -> u64 {
        self.ahead_us
    }

    fn put(&mut self, now_us: u64, said: Said) {
        let second = match &said {
            Said::Timing(value) => vec![self.resumable.timing(value, now_us)],
            Said::Playout(value) => self.resumable.sound(value, now_us),
            Said::Activity(_) => Vec::new(),
        };
        let checked = match &said {
            Said::Timing(value) => value.check().map(|()| {
                let _ = self.to_timing.publish_json(now_us, value);
            }),
            Said::Activity(value) => value.check().map(|()| {
                let _ = self.to_activity.publish_json(now_us, value);
            }),
            Said::Playout(value) => value.check().map(|()| {
                let _ = self.to_playout.publish_json(now_us, value);
            }),
        };
        let checked = checked.and_then(|()| {
            second
                .iter()
                .try_for_each(|said| self.put_second(now_us, said))
        });
        // Its own check refused it: a fault here, said once in a while
        // and never sent for a reader to refuse.
        if let Err(why) = checked {
            self.refused += 1;
            if self.refused.is_power_of_two() {
                eprintln!(
                    "lamp-voice: a timing message failed its own check ({} so far): {why:?}",
                    self.refused
                );
            }
        }
    }

    fn put_second(&mut self, now_us: u64, said: &ResumableSaid) -> Result<(), lamp_msgs::Invalid> {
        match said {
            ResumableSaid::Progress(value) => value.check().map(|()| {
                let _ = self.to_progress.publish_json(now_us, value);
            }),
            ResumableSaid::Sound(value) => value.check().map(|()| {
                let _ = self.to_sound.publish_json(now_us, value);
            }),
        }
    }

    /// One pass of the voice's loop.
    pub fn tick(&mut self, seen: Seen) {
        if let Some(held) = self.resumable.holding(seen.yielding, seen.now_us) {
            // What was gathered and not said is not to be said now.
            self.timing.piece = None;
            if let Err(why) = self.put_second(seen.now_us, &held) {
                eprintln!("lamp-voice: a held turn could not be said: {why:?}");
            }
        }
        for said in self.timing.tick(seen) {
            self.put(seen.now_us, said);
        }
        // Between answers these two say nothing for longer than their
        // lease: their readers are told the voice is still there.
        let _ = self.to_activity.alive();
        let _ = self.to_playout.alive();
        let _ = self.to_sound.alive();
    }

    /// The speaker took `frames` of `turn`'s sound; its queue held
    /// `queued_frames` when looked at, at `queue_observed_at_us`, with
    /// those frames in it. Call after [`Self::tick`] in the same pass.
    pub fn played(
        &mut self,
        turn: u64,
        frames: usize,
        queued_frames: usize,
        queue_observed_at_us: u64,
        level: u8,
        now_us: u64,
    ) {
        let (start_us, end_us) = sounds(frames, queued_frames, queue_observed_at_us);
        self.ahead_us = start_us.saturating_sub(queue_observed_at_us);
        self.queue_end = Some((turn, end_us));
        if let Some(said) = self.timing.played(turn, start_us, end_us, level, now_us) {
            self.put(now_us, said);
        }
    }

    /// Whether the sound said so far ends less than [`FORETELL_US`] and a
    /// piece ahead of now, so that foretelling has something to add: the
    /// loop asks this before it copies what is held.
    pub fn foretell_due(&self, now_us: u64) -> bool {
        self.timing.played_through_us < now_us.saturating_add(FORETELL_US)
    }

    /// The voice's sound for `turn` still held before the speaker, at the
    /// render rate, as it will be handed over: said ahead of time
    /// ([`Timing::foretell`]). Call after [`Self::played`] in the same
    /// pass.
    pub fn foretell(&mut self, turn: u64, ahead: &[i16], now_us: u64) {
        if !self.foretell_due(now_us) {
            return;
        }
        let Some(from_us) = self
            .queue_end
            .filter(|(queued, _)| *queued == turn)
            .map(|(_, end)| end.max(now_us))
        else {
            return;
        };
        for said in self.timing.foretell(turn, from_us, ahead, now_us) {
            self.put(now_us, said);
        }
    }
}

/// How many of the voice's held samples can be foretold at once: the
/// horizon and one piece more, at the render rate.
pub fn foretell_samples() -> usize {
    ((FORETELL_US + PIECE_US) * u64::from(lamp_audio::RENDER_RATE) / 1_000_000) as usize
}

/// When a piece of `frames` just written sounds, given the speaker's
/// queue after the write: the queue's last frame sounds when the whole
/// queue has played, and the piece is the last `frames` of it. By the
/// device's own count of what is queued, not by a microphone.
pub fn sounds(frames: usize, queued_frames: usize, queue_observed_at_us: u64) -> (u64, u64) {
    let us = |frames: usize| frames as u64 * 1_000_000 / u64::from(lamp_audio::RENDER_RATE);
    let end_us = queue_observed_at_us.saturating_add(us(queued_frames));
    (end_us.saturating_sub(us(frames)), end_us)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_publisher_of_a_process_says_a_session_of_its_own() {
        // After a software mute the conversation is made again, and its
        // sequences count from 1 under the session of the one before: Expression
        // refused every timing until the voice restarted (#7955).
        let ids: Vec<String> = (0..4).map(|n| session_named("abc123", n)).collect();
        assert_eq!(ids[0], "abc123", "the first is the service's invocation");
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "no two are the same: {ids:?}");
        assert!(ids.iter().all(|id| id.len() <= 128));
    }

    const MS: u64 = 1_000;
    const SECOND: u64 = 1_000_000;

    fn seen(now_us: u64, turn: Option<u64>, phase: Option<RingPhase>) -> Seen {
        Seen {
            now_us,
            turn,
            phase,
            accepting_input: true,
            yielding: false,
            person_speaking: false,
            first_sound_us: None,
        }
    }

    fn timings(said: &[Said]) -> Vec<(u64, Voice, VoiceEvent, Option<u64>)> {
        said.iter()
            .filter_map(|said| match said {
                Said::Timing(t) => Some((t.turn, t.state, t.event, t.first_audio_us)),
                _ => None,
            })
            .collect()
    }

    /// Everything said passes its own check, and each stream's sequence
    /// only rises.
    fn sound(all: &[Said]) {
        let mut last = [0u64; 3];
        for said in all {
            let (stream, sequence) = match said {
                Said::Timing(t) => {
                    t.check().unwrap();
                    (0, t.stamp.sequence)
                }
                Said::Activity(a) => {
                    a.check().unwrap();
                    (1, a.stamp.sequence)
                }
                Said::Playout(p) => {
                    p.check().unwrap();
                    (2, p.stamp.sequence)
                }
            };
            assert!(sequence > last[stream]);
            last[stream] = sequence;
        }
    }

    /// A question and its answer: heard, thought about with the first
    /// sound foretold, spoken, finished, and at rest after.
    #[test]
    fn a_turn_is_said_from_listening_to_its_end_with_its_first_sound_foretold() {
        let mut timing = Timing::new("boot", "voice-1");
        let mut all = Vec::new();
        // At rest, before anything was heard: turn 0.
        all.extend(timing.tick(seen(0, None, None)));
        assert_eq!(
            timings(&all),
            [(0, Voice::Resting, VoiceEvent::State, None)]
        );
        // Heard.
        all.extend(timing.tick(seen(SECOND, Some(7), Some(RingPhase::Listening))));
        // The person finished; the answer is awaited.
        all.extend(timing.tick(seen(3 * SECOND, Some(7), Some(RingPhase::Waiting))));
        // Its first sound has arrived and will play 300 ms from now.
        let mut about = seen(4 * SECOND, Some(7), Some(RingPhase::Waiting));
        about.first_sound_us = Some(4 * SECOND + 300 * MS);
        all.extend(timing.tick(about));
        // A moment later the same forecast: not said again.
        about.now_us += 10 * MS;
        let again = timing.tick(about);
        assert!(timings(&again).is_empty(), "{again:?}");
        // More arrived and it will be 60 ms sooner: said again.
        about.now_us += 10 * MS;
        about.first_sound_us = Some(4 * SECOND + 240 * MS);
        all.extend(timing.tick(about));
        // Speaking, then over.
        all.extend(timing.tick(seen(
            4 * SECOND + 250 * MS,
            Some(7),
            Some(RingPhase::Speaking),
        )));
        all.extend(timing.tick(seen(7 * SECOND, None, None)));
        assert_eq!(
            timings(&all)[1..],
            [
                (7, Voice::Listening, VoiceEvent::State, None),
                (7, Voice::Thinking, VoiceEvent::State, None),
                (
                    7,
                    Voice::Thinking,
                    VoiceEvent::Scheduled,
                    Some(4 * SECOND + 300 * MS)
                ),
                (
                    7,
                    Voice::Thinking,
                    VoiceEvent::Scheduled,
                    Some(4 * SECOND + 240 * MS)
                ),
                (7, Voice::Speaking, VoiceEvent::State, None),
                (7, Voice::Resting, VoiceEvent::Finished, None),
            ]
        );
        // The end stays in what is said until another turn takes its
        // place, however long that is.
        let later = timing.tick(seen(60 * SECOND, None, None));
        assert_eq!(
            timings(&later),
            [(7, Voice::Resting, VoiceEvent::Finished, None)]
        );
        all.extend(later);
        sound(&all);
    }

    #[test]
    fn an_unchanged_state_is_said_again_every_80_ms_and_a_change_at_once() {
        let mut timing = Timing::new("boot", "voice-1");
        let mut said = 0;
        for step in 0..100u64 {
            said += timings(&timing.tick(seen(step * 10 * MS, None, None))).len();
        }
        // One second in 10 ms passes: at 0, 80, 160, ... 960 ms.
        assert_eq!(said, 13);
        let change = timing.tick(seen(SECOND + MS, Some(1), Some(RingPhase::Listening)));
        assert_eq!(timings(&change).len(), 1);
        // Whether it can keep what is said changing is a change too.
        let mut deaf = seen(SECOND + 2 * MS, Some(1), Some(RingPhase::Listening));
        deaf.accepting_input = false;
        assert_eq!(timings(&timing.tick(deaf)).len(), 1);
    }

    /// A turn nothing was said to is over like one that was answered; one
    /// cut off by a person is interrupted, and the turn that cut it off
    /// follows at once.
    #[test]
    fn how_a_turn_ended_is_told_by_whether_it_spoke_and_whether_it_was_yielding() {
        let mut timing = Timing::new("boot", "voice-1");
        let mut all = Vec::new();
        // Heard, sent, and nothing said to it.
        all.extend(timing.tick(seen(SECOND, Some(1), Some(RingPhase::Listening))));
        all.extend(timing.tick(seen(2 * SECOND, Some(1), Some(RingPhase::Waiting))));
        all.extend(timing.tick(seen(6 * SECOND, None, None)));
        // Answered, and cut off in the middle by someone who is heard.
        all.extend(timing.tick(seen(10 * SECOND, Some(2), Some(RingPhase::Listening))));
        all.extend(timing.tick(seen(11 * SECOND, Some(2), Some(RingPhase::Waiting))));
        all.extend(timing.tick(seen(12 * SECOND, Some(2), Some(RingPhase::Speaking))));
        let mut yielding = seen(13 * SECOND, Some(2), Some(RingPhase::Speaking));
        yielding.yielding = true;
        // Holding its sound back is still the same turn speaking: a
        // reader takes a step back as the end of the turn's sound.
        assert_eq!(
            timings(&timing.tick(yielding)),
            [(2, Voice::Speaking, VoiceEvent::State, None)]
        );
        all.extend(timing.tick(seen(
            13 * SECOND + 300 * MS,
            Some(3),
            Some(RingPhase::Listening),
        )));
        assert_eq!(
            timings(&all),
            [
                (1, Voice::Listening, VoiceEvent::State, None),
                (1, Voice::Thinking, VoiceEvent::State, None),
                (1, Voice::Resting, VoiceEvent::Finished, None),
                (2, Voice::Listening, VoiceEvent::State, None),
                (2, Voice::Thinking, VoiceEvent::State, None),
                (2, Voice::Speaking, VoiceEvent::State, None),
                (2, Voice::Listening, VoiceEvent::Interrupted, None),
                (3, Voice::Listening, VoiceEvent::State, None),
            ]
        );
        sound(&all);
        // A pause it came back from is not an interruption.
        let mut timing = Timing::new("boot", "voice-1");
        timing.tick(seen(SECOND, Some(1), Some(RingPhase::Speaking)));
        let mut yielding = seen(2 * SECOND, Some(1), Some(RingPhase::Speaking));
        yielding.yielding = true;
        timing.tick(yielding);
        timing.tick(seen(3 * SECOND, Some(1), Some(RingPhase::Speaking)));
        let end = timing.tick(seen(5 * SECOND, None, None));
        assert_eq!(
            timings(&end),
            [(1, Voice::Resting, VoiceEvent::Finished, None)]
        );
    }

    /// Cut off, and what cut it off was not taken as a turn: the lamp is
    /// said to listen for two seconds and then to rest.
    #[test]
    fn cut_off_with_nobody_then_heard_it_listens_for_a_moment_and_rests() {
        let mut timing = Timing::new("boot", "voice-1");
        timing.tick(seen(SECOND, Some(4), Some(RingPhase::Speaking)));
        let mut yielding = seen(2 * SECOND, Some(4), Some(RingPhase::Speaking));
        yielding.yielding = true;
        timing.tick(yielding);
        let cut = timing.tick(seen(2 * SECOND + 100 * MS, None, None));
        assert_eq!(
            timings(&cut),
            [(4, Voice::Listening, VoiceEvent::Interrupted, None)]
        );
        let still = timing.tick(seen(4 * SECOND, None, None));
        assert_eq!(
            timings(&still),
            [(4, Voice::Listening, VoiceEvent::Interrupted, None)]
        );
        let rest = timing.tick(seen(4 * SECOND + 100 * MS + 1, None, None));
        assert_eq!(
            timings(&rest),
            [(4, Voice::Resting, VoiceEvent::State, None)]
        );
        let all: Vec<Said> = [cut, still, rest].concat();
        sound(&all);
    }

    /// A turn's state never goes back, a turn number that is over is not
    /// a turn again, and a first sound too near or too far is not
    /// foretold.
    #[test]
    fn nothing_is_said_that_a_reader_would_have_to_refuse() {
        let mut timing = Timing::new("boot", "voice-1");
        timing.tick(seen(SECOND, Some(5), Some(RingPhase::Waiting)));
        timing.tick(seen(2 * SECOND, Some(5), Some(RingPhase::Speaking)));
        // The ring shows listening for a moment in the middle of it.
        let back = timing.tick(seen(
            2 * SECOND + 10 * MS,
            Some(5),
            Some(RingPhase::Listening),
        ));
        assert!(timings(&back).is_empty());
        assert_eq!(timing.held_back, 1);
        let on = timing.tick(seen(2 * SECOND + 100 * MS, Some(5), None));
        assert_eq!(
            timings(&on),
            [(5, Voice::Speaking, VoiceEvent::State, None)]
        );
        timing.tick(seen(3 * SECOND, None, None));
        // Turn 5 again, and an older one: the end of turn 5 goes on
        // being said.
        for stale in [5, 3] {
            let said = timing.tick(seen(
                3 * SECOND + (8 - stale) * 100 * MS,
                Some(stale),
                Some(RingPhase::Speaking),
            ));
            assert_eq!(
                timings(&said),
                [(5, Voice::Resting, VoiceEvent::Finished, None)],
                "turn {stale}"
            );
        }
        // Forecasts: 9 ms ahead is too near, over two seconds too far,
        // and one is made only while thinking.
        for (ahead, phase, foretold) in [
            (9 * MS, RingPhase::Waiting, false),
            (2 * SECOND, RingPhase::Waiting, false),
            (300 * MS, RingPhase::Listening, false),
            (300 * MS, RingPhase::Speaking, false),
            (300 * MS, RingPhase::Waiting, true),
        ] {
            let mut timing = Timing::new("boot", "voice-1");
            let mut about = seen(10 * SECOND, Some(9), Some(phase));
            about.first_sound_us = Some(10 * SECOND + ahead);
            let said = timing.tick(about);
            assert_eq!(
                timings(&said)
                    .iter()
                    .any(|(_, _, event, _)| *event == VoiceEvent::Scheduled),
                foretold,
                "{ahead} us ahead while {phase:?}"
            );
            for said in &said {
                if let Said::Timing(t) = said {
                    t.check().unwrap();
                }
            }
        }
    }

    /// Whether the person is speaking is said while they are being heard
    /// by a voice that can keep it, on each change and every 80 ms.
    #[test]
    fn the_persons_speech_and_pauses_are_said_only_while_they_are_being_heard() {
        let mut timing = Timing::new("boot", "voice-1");
        let activity = |said: &[Said]| -> Vec<(u64, bool)> {
            said.iter()
                .filter_map(|said| match said {
                    Said::Activity(a) => Some((a.turn, a.speaking)),
                    _ => None,
                })
                .collect()
        };
        // At rest: nothing.
        assert!(activity(&timing.tick(seen(0, None, None))).is_empty());
        let mut hearing = seen(SECOND, Some(1), Some(RingPhase::Listening));
        hearing.person_speaking = true;
        let said = timing.tick(hearing);
        // The state first, then the activity: a reader takes activity
        // only for a turn it knows is being heard.
        assert!(matches!(said[0], Said::Timing(_)) && matches!(said[1], Said::Activity(_)));
        assert_eq!(activity(&said), [(1, true)]);
        hearing.now_us += 10 * MS;
        assert!(activity(&timing.tick(hearing)).is_empty());
        // A pause, at once.
        hearing.now_us += 10 * MS;
        hearing.person_speaking = false;
        assert_eq!(activity(&timing.tick(hearing)), [(1, false)]);
        // And again 80 ms on while nothing changes.
        hearing.now_us += 79 * MS;
        assert!(activity(&timing.tick(hearing)).is_empty());
        hearing.now_us += MS;
        assert_eq!(activity(&timing.tick(hearing)), [(1, false)]);
        // A voice that cannot keep what is said says nothing of it.
        hearing.now_us += 100 * MS;
        hearing.accepting_input = false;
        assert!(activity(&timing.tick(hearing)).is_empty());
        // Nor once the answer is awaited.
        let mut waiting = seen(3 * SECOND, Some(1), Some(RingPhase::Waiting));
        waiting.person_speaking = true;
        assert!(activity(&timing.tick(waiting)).is_empty());
    }

    /// The publishing object itself, on a real bus, as the voice's loop
    /// drives it while speaking: the speaker takes a piece, then what is
    /// held is foretold. Pieces arrive on both sound topics.
    #[test]
    fn while_speaking_the_held_sound_reaches_both_sound_topics() {
        use std::time::Duration;
        let root = std::path::PathBuf::from(format!("/tmp/lve-sound-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut out = ExpressionOut::on(&bus, "boot", "voice-1").unwrap();
        let mut playout = bus
            .subscribe_typed(VoicePlayout::TOPIC, "watch", VoicePlayout::TYPE)
            .unwrap();
        let mut sound = bus
            .subscribe_typed(VoiceSound::TOPIC, "watch", VoiceSound::TYPE)
            .unwrap();
        std::thread::sleep(Duration::from_millis(250));
        let now = || lamp_ipc::monotonic_us();
        for phase in [
            RingPhase::Listening,
            RingPhase::Waiting,
            RingPhase::Speaking,
        ] {
            let mut look = seen(now(), Some(4), Some(phase));
            look.first_sound_us = None;
            out.tick(look);
        }
        let at = now();
        out.played(4, 240, 480, at, 120, at);
        out.foretell(4, &vec![5_000i16; foretell_samples()], at);
        let count = |sub: &mut lamp_bus::Subscriber| {
            let mut n = 0;
            while let Ok(Some(_)) = sub.recv(Duration::from_millis(40)) {
                n += 1;
            }
            n
        };
        let (on_playout, on_sound) = (count(&mut playout), count(&mut sound));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(on_playout >= 9, "{on_playout} on voice/playout");
        assert!(on_sound >= 9, "{on_sound} on voice/sound");
    }

    /// A held turn on the second pair: the next generation and a held
    /// heartbeat at once; nothing sounds while held; after the resume, a
    /// heartbeat that says so before the first sound, and nothing from
    /// before it; the turn's end said not held.
    #[test]
    fn a_turn_held_for_someone_is_said_with_its_generation() {
        let timing = |state: Voice, event: VoiceEvent, at: u64| VoiceTiming {
            stamp: ClockStamp {
                clock_id: "boot".into(),
                at_us: at,
                sequence: at,
            },
            session_id: "voice-1".into(),
            turn: 3,
            request: 3,
            state,
            accepting_input: true,
            event,
            first_audio_us: None,
        };
        let piece = |start: u64| VoicePlayout {
            stamp: ClockStamp {
                clock_id: "boot".into(),
                at_us: start,
                sequence: start,
            },
            session_id: "voice-1".into(),
            turn: 3,
            request: 3,
            start_us: start,
            end_us: start + 20 * MS,
            level: 0.5,
        };
        let progress = |said: &ResumableSaid| match said {
            ResumableSaid::Progress(p) => Some((p.audio_generation, p.held, p.timing.state)),
            ResumableSaid::Sound(_) => None,
        };
        let mut second = Resumable::default();
        let said = second.timing(&timing(Voice::Speaking, VoiceEvent::State, SECOND), SECOND);
        assert_eq!(progress(&said), Some((1, false, Voice::Speaking)));
        assert!(matches!(
            second.sound(&piece(SECOND + 30 * MS), SECOND)[..],
            [ResumableSaid::Sound(ref s)] if s.audio_generation == 1
        ));
        let held = second.holding(true, SECOND + 50 * MS).unwrap();
        assert_eq!(progress(&held), Some((2, true, Voice::Listening)));
        assert!(
            second
                .sound(&piece(SECOND + 60 * MS), SECOND + 55 * MS)
                .is_empty()
        );
        let heartbeat = second.timing(
            &timing(Voice::Speaking, VoiceEvent::State, SECOND + 90 * MS),
            SECOND + 90 * MS,
        );
        assert_eq!(progress(&heartbeat), Some((2, true, Voice::Listening)));
        assert!(second.holding(false, SECOND + 200 * MS).is_none());
        assert!(
            second
                .sound(&piece(SECOND + 190 * MS), SECOND + 200 * MS)
                .is_empty()
        );
        let after = second.sound(&piece(SECOND + 230 * MS), SECOND + 210 * MS);
        assert_eq!(progress(&after[0]), Some((2, false, Voice::Speaking)));
        assert!(matches!(after[1], ResumableSaid::Sound(ref s) if s.audio_generation == 2));
        second.holding(true, SECOND + 300 * MS);
        let end = second.timing(
            &timing(Voice::Listening, VoiceEvent::Interrupted, SECOND + 400 * MS),
            SECOND + 400 * MS,
        );
        assert_eq!(progress(&end), Some((3, false, Voice::Listening)));
        for said in [held, heartbeat, end] {
            if let ResumableSaid::Progress(p) = said {
                assert!(p.check().is_ok());
            }
        }
    }

    /// What is held before the speaker is said ahead, in whole 20 ms
    /// pieces with their own loudness, up to 200 ms from now and no
    /// further; the speaker's pieces of the same stretch are not said
    /// again; nothing is foretold while the voice holds back for someone.
    #[test]
    fn the_sound_held_before_the_speaker_is_said_ahead_once() {
        let mut timing = Timing::new("boot", "voice-1");
        let pieces = |said: &[Said]| -> Vec<(u64, u64, u8)> {
            said.iter()
                .filter_map(|said| match said {
                    Said::Playout(p) => Some((
                        p.start_us - SECOND,
                        p.end_us - SECOND,
                        (p.level * 255.0).round() as u8,
                    )),
                    _ => None,
                })
                .collect()
        };
        // Half a second held at the render rate: loud, then silent.
        let per = (PIECE_US * u64::from(lamp_audio::RENDER_RATE) / 1_000_000) as usize;
        let mut held = vec![8_000i16; 2 * per];
        held.extend(vec![0i16; 23 * per]);
        // Not speaking: nothing.
        timing.tick(seen(SECOND, Some(2), Some(RingPhase::Waiting)));
        assert!(
            timing
                .foretell(2, SECOND + 20 * MS, &held, SECOND)
                .is_empty()
        );
        timing.tick(seen(SECOND + MS, Some(2), Some(RingPhase::Speaking)));
        let said = pieces(&timing.foretell(2, SECOND + 20 * MS, &held, SECOND + MS));
        // From the end of the speaker's queue, 20 ms each, until the first
        // that would begin 200 ms or more from now.
        assert_eq!(said.len(), 10);
        assert_eq!(said[0].0, 20 * MS);
        assert!(
            said.windows(2)
                .all(|w| w[0].1 == w[1].0 && w[1].1 - w[1].0 == PIECE_US)
        );
        assert!(said.last().unwrap().0 < MS + FORETELL_US);
        assert!(said[0].2 > 0 && said[1].2 > 0 && said[2].2 == 0);
        // Asked again at once: nothing new. Later: only what was not said.
        assert!(
            timing
                .foretell(2, SECOND + 20 * MS, &held, SECOND + MS)
                .is_empty()
        );
        let later = pieces(&timing.foretell(2, SECOND + 20 * MS, &held, SECOND + 41 * MS));
        assert_eq!(later.first().map(|p| p.0), Some(220 * MS));
        assert_eq!(later.len(), 2);
        // The speaker then takes a stretch already said: not said again.
        assert_eq!(
            timing.played(2, SECOND + 20 * MS, SECOND + 30 * MS, 200, SECOND + 45 * MS),
            None
        );
        // Holding back for someone: nothing foretold.
        let mut holding = seen(SECOND + 50 * MS, Some(2), Some(RingPhase::Speaking));
        holding.yielding = true;
        timing.tick(holding);
        assert!(
            timing
                .foretell(2, SECOND + 20 * MS, &held, SECOND + 300 * MS)
                .is_empty()
        );
    }

    /// The speaker takes 10 ms at a time. They are said as stretches of
    /// 20 ms, in order, never overlapping, never for a turn that is not
    /// speaking, and a last odd piece is said when nothing follows it.
    #[test]
    fn the_lamps_sound_is_said_in_stretches_that_never_overlap() {
        let mut timing = Timing::new("boot", "voice-1");
        let mut all = Vec::new();
        // Not speaking yet: nothing.
        timing.tick(seen(SECOND, Some(2), Some(RingPhase::Waiting)));
        assert_eq!(
            timing.played(2, SECOND + 30 * MS, SECOND + 40 * MS, 100, SECOND),
            None
        );
        timing.tick(seen(SECOND + 5 * MS, Some(2), Some(RingPhase::Speaking)));
        // Five pieces of 10 ms, each sounding 30 ms after it is written.
        for piece in 0..5u64 {
            let now = SECOND + 10 * MS + piece * 10 * MS;
            let level = [10, 200, 30, 40, 50][piece as usize];
            all.extend(timing.played(2, now + 30 * MS, now + 40 * MS, level, now));
            all.extend(timing.tick(seen(now + MS, Some(2), Some(RingPhase::Speaking))));
        }
        // Nothing more comes: the fifth is said once it has waited, while
        // it still has the least a reader needs left to sound.
        all.extend(timing.tick(seen(SECOND + 65 * MS, Some(2), Some(RingPhase::Speaking))));
        let stretches: Vec<(u64, u64, u8)> = all
            .iter()
            .filter_map(|said| match said {
                Said::Playout(p) => Some((
                    p.start_us - SECOND,
                    p.end_us - SECOND,
                    (p.level * 255.0).round() as u8,
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            stretches,
            [
                (40 * MS, 60 * MS, 200),
                (60 * MS, 80 * MS, 40),
                (80 * MS, 90 * MS, 50)
            ]
        );
        sound(&all);
        // A piece dated before what was already said is cut to what is
        // new, one wholly inside it is dropped, another turn's is dropped,
        // and one that has already sounded is not said.
        let now = SECOND + 100 * MS;
        assert_eq!(
            timing.played(2, SECOND + 70 * MS, SECOND + 85 * MS, 9, now),
            None
        );
        assert_eq!(timing.played(9, now + 30 * MS, now + 40 * MS, 9, now), None);
        let cut = timing.played(2, SECOND + 85 * MS, SECOND + 160 * MS, 9, now);
        assert_eq!(cut, None, "a new stretch waits for more or for its time");
        let Some(Said::Playout(said)) = timing
            .tick(seen(now + 20 * MS, Some(2), Some(RingPhase::Speaking)))
            .into_iter()
            .find(|said| matches!(said, Said::Playout(_)))
        else {
            panic!("the stretch was not said");
        };
        assert_eq!(
            (said.start_us, said.end_us),
            (SECOND + 90 * MS, SECOND + 160 * MS)
        );
        timing.played(2, now + 30 * MS, now + 40 * MS, 9, now + 20 * MS);
        let late = timing.tick(seen(now + 60 * MS, Some(2), Some(RingPhase::Speaking)));
        assert!(!late.iter().any(|said| matches!(said, Said::Playout(_))));
        // The turn ends with a piece in hand: it is dropped with it.
        timing.played(2, now + 100 * MS, now + 110 * MS, 9, now + 61 * MS);
        let end = timing.tick(seen(now + 62 * MS, None, None));
        assert!(!end.iter().any(|said| matches!(said, Said::Playout(_))));
        let rest = timing.tick(seen(now + 200 * MS, None, None));
        assert!(!rest.iter().any(|said| matches!(said, Said::Playout(_))));
    }

    #[test]
    fn a_written_piece_sounds_when_the_queue_before_it_has_played() {
        // 240 frames written; the queue then holds 960 (40 ms), looked at
        // at 5 s: the piece is its last 10 ms.
        assert_eq!(
            sounds(240, 960, 5 * SECOND),
            (5 * SECOND + 30 * MS, 5 * SECOND + 40 * MS)
        );
        // A queue that reports less than was written (a device that had
        // run dry): the piece is dated by the queue's end all the same.
        assert_eq!(
            sounds(240, 120, 5 * SECOND),
            (5 * SECOND - 5 * MS, 5 * SECOND + 5 * MS)
        );
    }
}
