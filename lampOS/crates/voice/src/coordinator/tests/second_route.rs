//! The second voice route through the production coordinator (lampOS#754).
//!
//! Every test runs the real `conversation()` service loop and
//! `finish_active_run` with cooperative child processes and real local IPC,
//! as `pilot_faults` does. The peers (capture, the speaker with the
//! production `BoundaryGuard`, the ring, the Live provider, the second
//! route and privacy) run in the test thread under the stepped scenario
//! clock (`crate::scenario_clock`), as `pilot_faults`' stepped runs do, so
//! a busy host cannot starve a peer past a freshness bound (lampOS#890).
//! The Live peer answers with
//! samples of 1,000 and the second route's with 2,000, so the speaker tells
//! whose answer it played.
//!
//! Nothing here is a device, a cloud service or an acoustic measurement.
use super::*;
use crate::{
    provider_flow::OutputSender,
    provider_link::ProviderLinkState,
    provider_worker::{FailedStage, Fault},
    ring_wire::{RingBlankReason, RingFeedback, RingFrame, RingPhase},
    wire::{CaptureTiming, ReferenceTiming},
};
use lamp_audio::ownership::PlaybackCursor;
use lamp_turn_owner::BoundaryGuard;
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

const PACKET: usize = crate::provider_flow::PACKET_SAMPLES;
const LIVE: i16 = 1_000;
const SECOND: i16 = 2_000;
/// The second route's wait cue, played before its words.
const CUE: i16 = 500;
const LIMIT: Duration = Duration::from_secs(12);
/// The clock the run's `ExpressionOut` stamps with (`Script::expression`).
const EXPRESSION_CLOCK: &str = "second-route-boot";

/// What the Live peer does with one request, by Start order.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Live {
    /// `packets` of audio at End, then completion.
    Answer(usize),
    /// TurnFailed(AwaitingResponse, fault) at End, before any audio.
    Fail(Fault),
    /// As `Fail`, then `packets` of audio for the same request: too late.
    FailThenLate(Fault, usize),
    /// The link becomes unavailable during the line, at capture frame 50.
    DropLink,
    /// As `DropLink` (at `link_drops_at` if set), and the line given to it
    /// again on its new session is answered with `packets`: a reconnect.
    Reconnect(usize),
    /// A skill's tool answered with a look (#941) in this session, then
    /// `packets` of audio.
    Look(usize, u64),
    /// A skill's tool answered with a look, then the answer opens with "...".
    LookSilent,
    /// A skill's tool answered with a look, then the turn is unclear.
    LookUnclear,
    /// The turn is unclear (heard as another language, #1417): no audio,
    /// then completion.
    Unclear,
    /// As `Unclear`, said of a cut-in over the lamp's answer (#828).
    UnclearCutIn,
    /// A skill's tool answered with a look, then as `FailThenLate`: the
    /// second route takes the turn.
    LookFail(Fault, usize),
    /// `packets` of a holding phrase ("One moment."), then `HoldingAnswer`
    /// for session 1 before completion (`LAMP_HOLDING_ANSWER`).
    Holding(usize),
}

/// How the Live route answers a turn the lamp wrote itself (a "Sorry?").
#[derive(Clone, Copy, Debug, PartialEq)]
enum Written {
    /// Not at all, as before #1421.
    Ignore,
    /// Started, then completion with no audio (#1417's empty "Sorry?").
    Silent,
    /// Started, the words "...", then completion: nothing said.
    Dots,
    /// Started, `packets` of audio, then completion: said.
    Said(usize),
}

/// How the second route, alone, answers a look's turn or the look's note
/// (`LAMP_SECOND_LOOK_NOTE`): as its engine tells them.
#[derive(Clone, Copy, Debug, PartialEq)]
enum LookAnswer {
    /// Calls `look_for` for the next place: `LookAsked`, before its audio,
    /// dropped as the engine drops it in a turn that is no search's.
    Next,
    /// Calls nothing: `LookNoCall`, after its audio.
    Nothing,
    /// Calls something else (a found place, another tool): neither.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum When {
    /// This many turns have been settled by the coordinator.
    Settled(usize),
    /// The coordinator has sent this many requests' End.
    Ended(usize),
    /// The speaker has accepted this many chunks.
    Accepted(usize),
    /// Alone, the second route has been sent this many look turns: the
    /// coordinator has committed them, whatever the host's pace.
    LookTurns(usize),
}

#[derive(Clone)]
struct Script {
    second: bool,
    /// `LAMP_VOICE_ROUTE=second`: the second route alone, in `rig.provider`,
    /// with no Gemini worker (`second` is then not read).
    only: bool,
    live: Vec<Live>,
    /// Packets in each of the second route's answers.
    second_packets: usize,
    /// The second route's answers open with these words.
    second_says: Option<&'static str>,
    /// The second route's first answer comes after a skill's look step
    /// (`LAMP_SECOND_SKILL_LOOK`, #941): `LookAsked` for the session that
    /// began it, before its audio.
    second_looks: bool,
    /// `second_looks`' look is the route's own `take_a_look`, no skill's
    /// search.
    own_look: bool,
    /// Alone, how the second route answers each look's turn in order, then
    /// `Other`.
    look_answers: &'static [LookAnswer],
    /// Alone, how the second route answers the look's note.
    note_answer: LookAnswer,
    /// The place the person named for the search's first look, or none
    /// (the desk).
    first_place: Option<&'static str>,
    /// `LAMP_SECOND_LOOK_WALK`: the second route tells a note's turn left
    /// with no call too.
    walk: bool,
    /// Packets of a wait cue the second route plays before its words.
    second_cue: usize,
    /// What the second route heard the person say, as its recogniser
    /// gives it before the answer: a transcript with no request (#1670).
    second_hears: Option<&'static str>,
    /// `second_hears` as the duplex route gives a line: word by word, with
    /// no piece marked finished (Gemini's input transcript, policy.md).
    hears_in_pieces: bool,
    /// With `hears_in_pieces`, the pieces come as the line begins, before
    /// its End, not after it.
    hears_at_start: bool,
    /// Speech starts again (a new turn, or speaking over an answer).
    speak_again: Option<When>,
    /// A murmur of two capture frames, too short to be a turn: the room is
    /// not quiet for a second after it.
    murmur: Option<When>,
    /// A hand Stop (the button's tap) comes through Controls this many
    /// capture frames after the murmur.
    hand_stop: Option<u64>,
    /// The body says it is pushed (`body/hand`, #2904), from then on, on
    /// the bus `TEST_HAND_IN` reads.
    push: Option<When>,
    /// Alone, the first look's turn is answered only this many capture
    /// frames after the push, as a model thinks over a picture: the push
    /// lands while the answer is awaited.
    answer_after_push: Option<u64>,
    /// Privacy reports muted once this many turns are settled.
    turns: usize,
    /// The Live link is Ready again, with a new session, once this many
    /// turns are settled.
    live_back_after: Option<usize>,
    /// Gemini's link is not Ready at startup: it is Ready at this capture
    /// frame (10 ms each), or never.
    live_late: Option<Option<u64>>,
    /// The second route's worker exits as this many of its inputs arrive
    /// (1: on its Start) (#960).
    second_dies: Option<usize>,
    /// The standby's worker exits at this capture frame (#960).
    standby_dies_at: Option<u64>,
    /// Privacy reports muted a second after the second route's worker
    /// exited, instead of after `turns`.
    mute_after_second_dies: bool,
    /// The Live link is Ready again, with a new session, at this capture
    /// frame, whatever turn is in hand.
    live_back_at_frame: Option<u64>,
    /// The Live worker exits at this capture frame, with no report.
    live_dies_at: Option<u64>,
    /// The capture frame a `DropLink` or `Reconnect` link drops at (50).
    link_drops_at: Option<u64>,
    turn_taking: TurnTaking,
    /// How a written turn is answered.
    written: Written,
    /// Privacy reports muted at this capture frame, whatever has settled:
    /// for a run whose turns go on past the person's (the lamp's own).
    mute_at_frame: Option<u64>,
    /// The second route's answer packet by packet: its sample value, so its
    /// level rises and falls as speech does (#933). `None`: `SECOND`.
    second_shape: Option<fn(usize) -> i16>,
    /// Give the voice an `ExpressionOut` on a bus of the run's own, and keep
    /// what it says for Expression (`voice/progress`, `voice/sound`).
    expression: bool,
    /// The second route answers nothing at End: its chat is still
    /// streaming (#1620 on the second route).
    second_holds: bool,
    /// The second route's first request is told at End with its words
    /// (`second_hears`) and nothing else, its answer held, as that route
    /// holds a line that ends before its name (`LAMP_SECOND_OPEN_NAME`);
    /// the requests after it are answered as usual.
    second_waits_first: bool,
    /// `LAMP_SECOND_OPEN_NAME` in the rig.
    open_name: bool,
    /// `LAMP_CALL_TAKES_NEXT_LINE` in the rig.
    call_takes_next: bool,
    /// The second route fails each turn before any audio, as its worker
    /// does when its speech to text or chat cannot be reached.
    second_fails: Option<Fault>,
    /// With `second_fails`, only this many of the route's requests fail;
    /// the ones after are answered: the service is back.
    second_fails_first: Option<usize>,
    /// More lines, of 60 capture frames each, beginning at these frames.
    speak_at: &'static [u64],
    /// The second route's requests, by the count ended, in which speech to
    /// text heard no words: told idle at End, no chat asked.
    second_unheard: &'static [usize],
    /// The second route's requests, by the count ended, that fail only
    /// slow: no answer in time, no refusal of the service's.
    second_slow: &'static [usize],
    /// The recorded failure lines, an offline and a voice service one
    /// among them (`FailureNotices::synthetic_offline`); none without.
    notices: bool,
    /// This peer reads Stop and keeps its process running (#1620).
    ignores_stop: Option<&'static str>,
    /// The scenario's own bound, past which capture reports a fault.
    limit: Duration,
    /// No person's input: only authored skill output is under test.
    silent: bool,
    /// The person's speech is a voice, a 150 Hz tone, not a flat level, so
    /// the voice's tone can read it (`LAMP_VOICE_TONE`).
    voiced: bool,
    /// One parsed Say request, read at this capture frame's scenario time.
    say_at_frame: Option<(u64, crate::say::LineForTest)>,
    /// Onboarding's near-call try window opens this many milliseconds from
    /// the scenario's start, for its 7 s (#1803).
    near_window_ms: Option<u64>,
    /// The lamp's name is heard beginning this many milliseconds from the
    /// scenario's start, as that step reads it (#1803).
    near_name_ms: Option<u64>,
    /// Alone, a line told as the step's call (`StepCall`) has words that
    /// were the lamp's name alone (#3923): the route says so
    /// (`StepCallHeard`) and ends it idle, as the engine does.
    heard_as_call: bool,
    /// With `heard_as_call`, an earlier line's words come back late, just
    /// before the call's own `StepCallHeard` (the Reviewer-lead on #4006).
    late_words_first: bool,
    /// The step's try window opens only this many milliseconds from the
    /// scenario's start, read as it comes (`near_window_ms` is then its
    /// prompt's end): a line can begin before the step waits for a call.
    near_opens_ms: Option<u64>,
    /// With `heard_as_call`, the first call's words come back only once the
    /// next line has begun.
    heard_as_call_late: bool,
    /// The end sent early (`--early-end-ms`), in 10 ms blocks of quiet.
    early_end_blocks: Option<u16>,
    /// Capture frames of speech in the person's first utterance, from frame
    /// 20 (60: 0.6 s, as long as a call).
    speech_frames: u64,
    /// The capture frame the person's first utterance begins at (20).
    speech_from: u64,
    /// Onboarding's sentence window is open over these milliseconds
    /// from the scenario's start (#1803).
    sentence_window_ms: Option<(u64, u64)>,
    /// What Gemini Live heard the person say, in pieces, the last one
    /// finished, then what its answer says, before the answer's audio.
    live_hears: &'static [&'static str],
    live_says: Option<&'static str>,
    /// Each capture frame carries the echo canceller's evidence, as a
    /// duplex lamp's do: the person speaking over the lamp while speech,
    /// else the lamp's own voice.
    echo_evidence: bool,
}

impl Script {
    fn question(live: Live) -> Self {
        Self {
            second: true,
            only: false,
            live: vec![live],
            second_packets: 12,
            second_says: None,
            second_looks: false,
            own_look: false,
            look_answers: &[],
            note_answer: LookAnswer::Other,
            first_place: None,
            walk: false,
            second_cue: 0,
            second_hears: None,
            hears_in_pieces: false,
            hears_at_start: false,
            speak_again: None,
            murmur: None,
            hand_stop: None,
            push: None,
            answer_after_push: None,
            turns: 1,
            live_back_after: None,
            live_late: None,
            second_dies: None,
            standby_dies_at: None,
            mute_after_second_dies: false,
            live_back_at_frame: None,
            live_dies_at: None,
            link_drops_at: None,
            turn_taking: TurnTaking::V1Baseline,
            written: Written::Ignore,
            mute_at_frame: None,
            second_shape: None,
            expression: false,
            second_holds: false,
            second_waits_first: false,
            open_name: false,
            call_takes_next: false,
            second_fails: None,
            second_fails_first: None,
            speak_at: &[],
            second_unheard: &[],
            second_slow: &[],
            notices: false,
            ignores_stop: None,
            limit: LIMIT,
            silent: false,
            say_at_frame: None,
            voiced: false,
            near_window_ms: None,
            sentence_window_ms: None,
            live_hears: &[],
            live_says: None,
            near_name_ms: None,
            heard_as_call: false,
            late_words_first: false,
            near_opens_ms: None,
            heard_as_call_late: false,
            early_end_blocks: None,
            speech_frames: 60,
            speech_from: 20,
            echo_evidence: false,
        }
    }
}

#[derive(Clone, Debug)]
struct Chunk {
    turn: u64,
    value: i16,
    samples: Vec<i16>,
    accepted: bool,
}

#[derive(Default)]
struct Seen {
    live: Vec<ProviderInput>,
    second: Vec<ProviderInput>,
    chunks: Vec<Chunk>,
    /// Owner, microphone and camera of every authority the speaker's guard
    /// took.
    installs: Vec<(Option<u64>, bool, bool)>,
    phases: Vec<RingPhase>,
    stopped: Vec<&'static str>,
    /// Actual body/hand publications, including the held push refreshes.
    hand_publishes: usize,
}

struct Run {
    result: Result<()>,
    trace: Vec<Value>,
    seen: Seen,
    exits: Vec<(&'static str, bool)>,
    /// What the voice said for Expression, as Expression takes it, in the
    /// order it was said (`Script::expression`).
    expression: Vec<lamp_expression::Input>,
}

fn dedup<T: PartialEq + Clone>(items: &[T]) -> Vec<T> {
    let mut out: Vec<T> = Vec::new();
    for item in items {
        if out.last() != Some(item) {
            out.push(item.clone());
        }
    }
    out
}

impl Run {
    fn events(&self, kind: &str) -> Vec<&Value> {
        self.trace.iter().filter(|e| e["kind"] == kind).collect()
    }
    fn finished(&self, turn: u64) -> &Value {
        self.events("turn_finished")
            .into_iter()
            .find(|e| e["turn"] == turn)
            .unwrap_or_else(|| panic!("turn {turn} never finished: {}", self.summary()))
    }
    fn played(&self, turn: u64) -> Vec<i16> {
        self.seen
            .chunks
            .iter()
            .filter(|c| c.turn == turn && c.accepted)
            .map(|c| c.value)
            .collect()
    }
    fn summary(&self) -> String {
        let kinds: Vec<&str> = self
            .trace
            .iter()
            .map(|e| e["kind"].as_str().unwrap_or("?"))
            .collect();
        format!(
            "result {:?}\nkinds {:?}\nphases {:?}\ninstalls {:?}\nhand publications {}",
            self.result.as_ref().map_err(|e| e.to_string()),
            dedup(&kinds),
            dedup(&self.seen.phases),
            dedup(&self.seen.installs),
            self.seen.hand_publishes,
        )
    }
    /// What every run keeps: it ended by the scripted mute, every child saw
    /// Stop and was reaped, and the speaker wrote nothing without authority.
    fn assert_clean(&self) {
        eprintln!("{}", self.summary());
        assert!(
            self.result
                .as_ref()
                .err()
                .is_some_and(|e| e.downcast_ref::<PhysicalMute>().is_some()),
            "{}",
            self.summary()
        );
        for (role, exited) in &self.exits {
            assert!(exited, "{role} child was not reaped");
            assert!(self.seen.stopped.contains(role), "{role} never saw Stop");
        }
        assert!(self.events("worker_forced_shutdown").is_empty());
    }
}

/// Start, Audio 1..=n in order, End: one whole line.
fn whole_line(inputs: &[ProviderInput], request: u64) -> usize {
    let mine: Vec<_> = inputs
        .iter()
        .filter(|input| request_of(input) == request)
        .collect();
    assert!(
        matches!(mine.first(), Some(ProviderInput::Start { .. })),
        "{mine:?}"
    );
    assert!(
        matches!(mine.last(), Some(ProviderInput::End { .. })),
        "{mine:?}"
    );
    for (index, input) in mine[1..mine.len() - 1].iter().enumerate() {
        match input {
            ProviderInput::Audio { sequence, .. } => assert_eq!(*sequence, index as u64 + 1),
            other => panic!("{other:?} inside a line"),
        }
    }
    mine.len() - 2
}

fn request_of(input: &ProviderInput) -> u64 {
    match input {
        ProviderInput::Start { request, .. }
        | ProviderInput::Audio { request, .. }
        | ProviderInput::End { request, .. }
        | ProviderInput::Called { request, .. }
        | ProviderInput::Paused { request, .. }
        | ProviderInput::CountOnly { request, .. }
        | ProviderInput::StepCall { request, .. }
        | ProviderInput::Onboarding { request, .. }
        | ProviderInput::Written { request, .. }
        | ProviderInput::Reminder { request, .. } => *request,
    }
}

fn audio_count(inputs: &[ProviderInput], request: u64) -> usize {
    inputs
        .iter()
        .filter(|input| matches!(input, ProviderInput::Audio { request: r, .. } if *r == request))
        .count()
}

struct Peer {
    role: &'static str,
    channels: WorkerChannels,
    input: Option<std::process::ChildStdin>,
    child_pid: u32,
    stopped: bool,
    output: OutputSender,
    queued: VecDeque<ProviderOutput>,
    snapshot: Option<Snapshot>,
}

impl Peer {
    /// The child exits once its input closes; in a shutdown step it is
    /// waited for, as logical time does not pass there.
    fn close_input(&mut self, deadline: Option<Instant>) -> io::Result<()> {
        if self.input.take().is_some()
            && let Some(deadline) = deadline
        {
            // Bounded by the scope's own watchdog, not the cleanup's real
            // deadline, which a busy host can use up first (#1235's review).
            let bound = crate::scenario_clock::host_deadline().unwrap_or(deadline);
            crate::scenario_clock::wait_child_exited(self.child_pid, bound)?;
        }
        Ok(())
    }
}

thread_local! {
    /// Memory, telling the voice it forgot a person once this many Ends have
    /// been sent (`memory/forgot`), on the bus `TEST_FORGOT_IN` reads.
    static TEST_MEMORY_FORGETS: std::cell::RefCell<Option<(usize, String, lamp_bus::Publisher)>> =
        const { std::cell::RefCell::new(None) };
    /// The body, saying `body/hand` on the bus `TEST_HAND_IN` reads
    /// (`Script::push`), and whether it is pushed yet.
    static TEST_BODY_HAND: std::cell::RefCell<Option<(bool, lamp_bus::Publisher)>> =
        const { std::cell::RefCell::new(None) };
}

struct Peers {
    script: Script,
    peers: Vec<Peer>,
    seen: Arc<Mutex<Seen>>,
    muted: Arc<AtomicBool>,
    guard: BoundaryGuard,
    capture_started: bool,
    frames: u64,
    next_capture: Instant,
    next_write: Instant,
    cursor: u64,
    accepted: usize,
    speech_offset: Option<u64>,
    /// Requests by Live Start order.
    live_requests: Vec<u64>,
    ended: BTreeSet<u64>,
    settled: usize,
    audio_sequence: u64,
    link_dropped: bool,
    link_back: bool,
    live_ready_sent: bool,
    /// The capture frame at which the second route's worker exited.
    second_died_at: Option<u64>,
    /// The session of the second route's last Start.
    second_session: u64,
    /// The second route has asked its look (`Script::second_looks`).
    second_looked: bool,
    /// Look's turns the second route has answered (`Script::look_answers`).
    look_turns: usize,
    /// The capture frame the murmur began at (`Script::murmur`).
    murmur_at: Option<u64>,
    /// The capture frame the push began at (`Script::push`).
    pushed_at: Option<u64>,
    /// Last body/hand publication on this run's scenario clock.
    last_hand_publish_us: Option<u64>,
    /// A look's answer held for `Script::answer_after_push`: its peer and
    /// what it says.
    held_answer: Option<(usize, Vec<ProviderOutput>)>,
    /// Controls' queue into the voice's hand controls (`Script::hand_stop`).
    hand: Option<crate::hand_controls::TestControls>,
    /// The step's try window, opened at this time (`Script::near_opens_ms`).
    near_opens: Option<(u64, crate::onboarding::near::NearFlow)>,
    /// A call's words held back until the next line begins
    /// (`Script::heard_as_call_late`).
    call_words_held: Option<(usize, u64)>,
}

impl Peers {
    fn index(&self, role: &str) -> Option<usize> {
        self.peers.iter().position(|p| p.role == role)
    }

    fn speech(&self, frame: u64) -> bool {
        !self.script.silent
            && ((self.script.speech_from..self.script.speech_from + self.script.speech_frames)
                .contains(&frame)
                || self
                    .speech_offset
                    .is_some_and(|offset| (offset..offset + 60).contains(&frame))
                || self
                    .murmur_at
                    .is_some_and(|at| (at..at + 2).contains(&frame))
                || self
                    .script
                    .speak_at
                    .iter()
                    .any(|at| (*at..at + 60).contains(&frame)))
    }

    fn tick(&mut self, deadline: Option<Instant>) -> io::Result<()> {
        if self
            .near_opens
            .as_ref()
            .is_some_and(|(at, _)| monotonic_us() >= *at)
            && let Some((_, flow)) = self.near_opens.take()
        {
            TEST_NEAR.with(|near| *near.borrow_mut() = Some(flow));
        }
        let ended = self.ended.len();
        TEST_MEMORY_FORGETS.with(|slot| {
            if slot
                .borrow()
                .as_ref()
                .is_some_and(|(after, ..)| ended >= *after)
                && let Some((_, person, mut memory)) = slot.borrow_mut().take()
            {
                memory
                    .publish_json(
                        monotonic_us(),
                        &lamp_msgs::MemoryForgot {
                            person: Some(person),
                        },
                    )
                    .unwrap();
            }
        });
        if self
            .script
            .say_at_frame
            .as_ref()
            .is_some_and(|(frame, _)| self.frames >= *frame)
        {
            self.script
                .say_at_frame
                .take()
                .unwrap()
                .1
                .read_at(monotonic_us());
        }
        if let Some(when) = self.script.speak_again {
            let due = match when {
                When::Settled(count) => self.settled >= count,
                When::Ended(count) => self.ended.len() >= count,
                When::Accepted(count) => self.accepted >= count,
                When::LookTurns(count) => self.look_turns >= count,
            };
            if due {
                self.script.speak_again = None;
                self.speech_offset = Some(self.frames);
            }
        }
        if let Some(when) = self.script.murmur {
            let due = match when {
                When::Settled(count) => self.settled >= count,
                When::Ended(count) => self.ended.len() >= count,
                When::Accepted(count) => self.accepted >= count,
                When::LookTurns(count) => self.look_turns >= count,
            };
            if due {
                self.script.murmur = None;
                self.murmur_at = Some(self.frames);
            }
        }
        if let Some(when) = self.script.push {
            let due = match when {
                When::Settled(count) => self.settled >= count,
                When::Ended(count) => self.ended.len() >= count,
                When::Accepted(count) => self.accepted >= count,
                When::LookTurns(count) => self.look_turns >= count,
            };
            if due {
                self.script.push = None;
                self.pushed_at = Some(self.frames);
                TEST_BODY_HAND.with(|slot| {
                    if let Some((pushed, _)) = slot.borrow_mut().as_mut() {
                        *pushed = true;
                    }
                });
            }
        }
        if let (Some(after), Some(at)) = (self.script.answer_after_push, self.pushed_at)
            && self.frames >= at + after
            && let Some((index, answer)) = self.held_answer.take()
        {
            self.peers[index].queued.extend(answer);
        }
        // Say the first push immediately, then refresh at the topic's
        // promised 100 ms cadence. Every publication writes the latest file;
        // a 2 ms scenario step is not a new body observation.
        let hand_at_us = monotonic_us();
        TEST_BODY_HAND.with(|slot| {
            if let Some((true, body)) = slot.borrow_mut().as_mut()
                && self
                    .last_hand_publish_us
                    .is_none_or(|at| hand_at_us.saturating_sub(at) >= 100_000)
            {
                body.publish_json(
                    hand_at_us,
                    &lamp_msgs::Hand {
                        pushed: true,
                        since_us: 1,
                    },
                )
                .unwrap();
                self.last_hand_publish_us = Some(hand_at_us);
                self.seen.lock().unwrap().hand_publishes += 1;
            }
        });
        if let (Some(after), Some(at)) = (self.script.hand_stop, self.murmur_at)
            && self.frames >= at + after
            && let Some(hand) = self.hand.as_ref()
        {
            self.script.hand_stop = None;
            hand.send(hand_stop_state(monotonic_us()));
        }
        // A voice still serving a second after its second route died can
        // be muted then, which ends the run cleanly.
        if self
            .script
            .mute_at_frame
            .is_some_and(|frame| self.frames >= frame)
        {
            self.muted.store(true, Ordering::SeqCst);
        }
        if self.script.mute_at_frame.is_none() && self.settled >= self.script.turns
            || (self.script.mute_after_second_dies
                && self
                    .second_died_at
                    .is_some_and(|frame| self.frames >= frame + 100))
        {
            self.muted.store(true, Ordering::SeqCst);
        }
        let live_back_due = self
            .script
            .live_back_after
            .is_some_and(|after| self.settled >= after)
            || self
                .script
                .live_back_at_frame
                .is_some_and(|frame| self.frames >= frame);
        if live_back_due
            && self.link_dropped
            && !self.link_back
            && let Some(index) = self.index("provider")
        {
            self.link_back = true;
            let _ = self.peers[index]
                .channels
                .control
                .send(WorkerEvent::ProviderLink {
                    link: ready_link(3, 2),
                });
        }
        if let Some(Some(frame)) = self.script.live_late
            && self.frames >= frame
            && !self.live_ready_sent
            && let Some(index) = self.index("provider")
        {
            self.live_ready_sent = true;
            let _ = self.peers[index]
                .channels
                .control
                .send(WorkerEvent::ProviderLink {
                    link: ready_link(1, 1),
                });
        }
        if self
            .script
            .live_dies_at
            .is_some_and(|frame| self.frames >= frame)
            && let Some(index) = self.index("provider")
            && !self.peers[index].stopped
        {
            let peer = &mut self.peers[index];
            peer.stopped = true;
            peer.input.take();
        }
        if self
            .script
            .standby_dies_at
            .is_some_and(|frame| self.frames >= frame)
            && let Some(index) = self.index("second")
            && !self.peers[index].stopped
        {
            let peer = &mut self.peers[index];
            peer.stopped = true;
            peer.input.take();
        }
        for index in 0..self.peers.len() {
            self.tick_peer(index, deadline)?;
        }
        Ok(())
    }

    fn tick_peer(&mut self, index: usize, deadline: Option<Instant>) -> io::Result<()> {
        if self.peers[index].stopped {
            return Ok(());
        }
        let role = self.peers[index].role;
        for _ in 0..16 {
            match self.peers[index].channels.control.receive::<Control>() {
                Ok(Some(Control::Authority { snapshot })) => {
                    self.peers[index].snapshot = Some(snapshot);
                    if role == "speaker" {
                        self.install(snapshot);
                    }
                    if snapshot.owner().is_none() {
                        self.settled = self.settled.max(self.ended.len());
                    }
                }
                Ok(Some(Control::ProviderOutputCapacity { through })) => {
                    self.peers[index].output.grant(through).unwrap()
                }
                Ok(Some(Control::StartCapture)) => {
                    self.capture_started = true;
                    let at = monotonic_us();
                    let generation = self.peers[index].snapshot.unwrap().microphone_generation();
                    self.peers[index]
                        .channels
                        .control
                        .send(WorkerEvent::CaptureStarted {
                            epoch: 1,
                            dsp_epoch: 1,
                            noise_suppression: NoiseSuppression::On,
                            privacy_generation: generation,
                            rate: 16_000,
                            channels: 1,
                            period_frames: 160,
                            buffer_frames: 640,
                            monotonic_status_timestamps: false,
                            open_started_at_us: at,
                            prepared_at_us: at,
                            capture_started_at_us: at,
                            start_completed_at_us: at,
                        })
                        .unwrap();
                }
                Ok(Some(Control::Stop)) if self.script.ignores_stop == Some(role) => {
                    // Read, and its process goes on: only a kill ends it.
                    self.peers[index].stopped = true;
                    self.seen.lock().unwrap().stopped.push(role);
                    return Ok(());
                }
                Ok(Some(Control::Stop)) => {
                    if role == "ring" {
                        let at = monotonic_us();
                        let _ = self.peers[index].channels.control.send(WorkerEvent::Ring {
                            report: RingFeedback::Blanked {
                                reason: RingBlankReason::Shutdown,
                                write_started_at_us: at,
                                write_finished_at_us: at,
                            },
                        });
                    }
                    let peer = &mut self.peers[index];
                    peer.stopped = true;
                    peer.close_input(deadline)?;
                    self.seen.lock().unwrap().stopped.push(role);
                    return Ok(());
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => break,
            }
        }
        match role {
            "capture" if self.capture_started => self.tick_capture(index),
            "speaker" => self.tick_speaker(index),
            "provider" | "second" => self.tick_provider(index),
            "ring" => {
                while let Ok(Some(frame)) = self.peers[index].channels.data.receive::<RingFrame>() {
                    self.seen.lock().unwrap().phases.push(frame.phase);
                    let at = monotonic_us();
                    let _ = self.peers[index].channels.control.send(WorkerEvent::Ring {
                        report: RingFeedback::Presented {
                            owner: frame.permit.owner(),
                            phase: frame.phase,
                            requested_at_us: frame.requested_at_us,
                            write_started_at_us: at,
                            write_finished_at_us: at,
                        },
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn tick_capture(&mut self, index: usize) {
        while crate::scenario_clock::instant() >= self.next_capture {
            self.frames += 1;
            let frame = self.frames;
            if frame == self.script.link_drops_at.unwrap_or(50)
                && !self.link_dropped
                && matches!(
                    self.script.live.first(),
                    Some(Live::DropLink | Live::Reconnect(_))
                )
                && let Some(live) = self.index("provider")
            {
                self.link_dropped = true;
                let link = ProviderLink {
                    revision: 2,
                    session: 1,
                    observed_at_us: monotonic_us(),
                    source_clock: None,
                    state: ProviderLinkState::Unavailable {
                        reason: Fault::Transport,
                    },
                };
                let _ = self.peers[live]
                    .channels
                    .control
                    .send(WorkerEvent::ProviderLink { link });
            }
            let at = monotonic_us();
            let speech = self.speech(frame);
            let peer = &mut self.peers[index];
            let sent = peer.channels.data.send(MicrophoneFrame {
                raw: None,
                linear: None,
                epoch: 1,
                dsp_epoch: 1,
                privacy_generation: peer.snapshot.unwrap().microphone_generation(),
                frame_sequence: frame,
                read_completed_at_us: at,
                probability: if speech { 0.99 } else { 0.0 },
                echo: self.script.echo_evidence.then(|| {
                    super::duplex::evidence(
                        0,
                        if speech {
                            super::duplex::OVER
                        } else {
                            super::duplex::OWN
                        },
                    )
                }),
                reference: Default::default(),
                samples: if speech && self.script.voiced {
                    (0..160u64)
                        .map(|at| {
                            let t = (frame * 160 + at) as f32 / 16_000.0;
                            (2_000.0 * (2.0 * std::f32::consts::PI * 150.0 * t).sin()) as i16
                        })
                        .collect()
                } else {
                    vec![if speech { 2_000 } else { 7 }; 160]
                },
                timing: CaptureTiming {
                    first_read_started_at_us: at,
                    last_read_started_at_us: at,
                    successful_reads: 1,
                    alsa_status_monotonic_us: None,
                    status_observed_at_us: at,
                    available_frames: 0,
                    delayed_frames: 0,
                    processing_completed_at_us: at,
                    aec_queue_delay_ms: 0,
                    reference: ReferenceTiming {
                        playback_epoch: 1,
                        clock_started_at_us: Some(at),
                        ..ReferenceTiming::default()
                    },
                },
            });
            self.next_capture += Duration::from_millis(10);
            if sent.is_err() {
                break;
            }
        }
    }

    fn audio(&mut self, index: usize, request: u64, packets: usize, value: i16) {
        for _ in 0..packets {
            self.audio_sequence += 1;
            self.peers[index].queued.push_back(ProviderOutput::Audio {
                request,
                sequence: self.audio_sequence,
                provider_event_at_us: monotonic_us(),
                source_frames: PACKET,
                source_offset: 0,
                samples: vec![value; PACKET],
            });
        }
    }

    fn finish(&mut self, index: usize, request: u64) {
        let queued = &mut self.peers[index].queued;
        queued.push_back(ProviderOutput::GenerationComplete { request });
        queued.push_back(ProviderOutput::TurnComplete {
            request,
            idle: true,
        });
    }

    /// What the person said (`second_hears`): whole and finished, as the
    /// second route's recogniser gives it, or in pieces with none finished.
    fn hears(&mut self, index: usize, text: &'static str) {
        let pieces = self.script.hears_in_pieces;
        let queued = &mut self.peers[index].queued;
        if pieces {
            for piece in text.split_inclusive(' ') {
                queued.push_back(ProviderOutput::Transcript {
                    request: None,
                    text: piece.into(),
                    finished: false,
                });
            }
        } else {
            queued.push_back(ProviderOutput::Transcript {
                request: None,
                text: text.into(),
                finished: true,
            });
        }
    }

    fn tick_provider(&mut self, index: usize) {
        let live = self.peers[index].role == "provider";
        for _ in 0..64 {
            let Ok(Some(input)) = self.peers[index].channels.data.receive::<ProviderInput>() else {
                break;
            };
            if !live
                && self
                    .script
                    .second_dies
                    .is_some_and(|after| self.seen.lock().unwrap().second.len() + 1 >= after)
            {
                // Its process exits; it reads and says nothing more.
                self.second_died_at.get_or_insert(self.frames);
                let peer = &mut self.peers[index];
                peer.stopped = true;
                peer.input.take();
                return;
            }
            {
                let mut seen = self.seen.lock().unwrap();
                let log = if live {
                    &mut seen.live
                } else {
                    &mut seen.second
                };
                log.push(input.clone());
            }
            match input {
                ProviderInput::Start {
                    request, session, ..
                } => {
                    // Alone, the second route's requests are the script's.
                    if live || self.script.only {
                        self.live_requests.push(request);
                    }
                    if !live {
                        self.second_session = session;
                    }
                    self.peers[index].queued.push_back(ProviderOutput::Started {
                        request,
                        session,
                        waiting_for_barrier: false,
                    });
                    // The held call's words, back once this line began.
                    if !live
                        && let Some((at, held)) = self.call_words_held
                        && held < request
                        && at == index
                    {
                        self.call_words_held = Some((at, u64::MAX));
                        let queued = &mut self.peers[index].queued;
                        queued.push_back(ProviderOutput::StepCallHeard { request: held });
                        queued.push_back(ProviderOutput::TurnComplete {
                            request: held,
                            idle: true,
                        });
                    }
                    if !live
                        && self.script.hears_at_start
                        && let Some(text) = self.script.second_hears
                    {
                        self.hears(index, text);
                    }
                }
                // Alone, the second route answers the lamp's own turns as
                // the Live route would, with its own samples.
                ProviderInput::Written {
                    request,
                    session,
                    text,
                    look,
                    search,
                    ..
                } if live || self.script.only => {
                    let queued = &mut self.peers[index].queued;
                    let started = ProviderOutput::Started {
                        request,
                        session,
                        waiting_for_barrier: false,
                    };
                    match self.script.written {
                        Written::Ignore => {}
                        Written::Silent => {
                            queued.push_back(started);
                            self.finish(index, request);
                        }
                        Written::Dots => {
                            queued.push_back(started);
                            queued.push_back(ProviderOutput::Transcript {
                                request: Some(request),
                                text: "...".into(),
                                finished: true,
                            });
                            self.finish(index, request);
                        }
                        Written::Said(packets) => {
                            queued.push_back(started);
                            let before = self.peers[index].queued.len();
                            // Alone, a look's turn or the look's note is
                            // answered as the script says.
                            let answer = if live {
                                None
                            } else if look {
                                self.look_turns += 1;
                                Some(
                                    self.script
                                        .look_answers
                                        .get(self.look_turns - 1)
                                        .copied()
                                        .unwrap_or(LookAnswer::Other),
                                )
                            } else if *text == super::look_note::look_for_things().note {
                                Some(self.script.note_answer)
                            } else {
                                None
                            };
                            // As the engine: the skill's next look is taken
                            // only in a search's own turn, and dropped in any
                            // other the lamp wrote (#1146).
                            if answer == Some(LookAnswer::Next) {
                                self.peers[index]
                                    .queued
                                    .push_back(ProviderOutput::LookAsked {
                                        request,
                                        session,
                                        dropped: (!search).then(|| {
                                            "the turn is one the lamp wrote itself".to_owned()
                                        }),
                                        search: true,
                                        // A place these tests do not read.
                                        call: None,
                                    });
                            }
                            self.audio(index, request, packets, if live { LIVE } else { SECOND });
                            // As the engine: a note's turn left so is
                            // told only with the walk (`walking_looks`).
                            let told = look || self.script.walk;
                            if answer == Some(LookAnswer::Nothing) && told {
                                self.peers[index]
                                    .queued
                                    .push_back(ProviderOutput::LookNoCall { request, session });
                            }
                            self.finish(index, request);
                            // Alone, the first look's answer waits for the
                            // push (`Script::answer_after_push`).
                            if look
                                && self.look_turns == 1
                                && self.script.answer_after_push.is_some()
                            {
                                let answer = self.peers[index].queued.split_off(before);
                                self.held_answer = Some((index, answer.into()));
                            }
                        }
                    }
                }
                ProviderInput::End { request, .. } => {
                    self.ended.insert(request);
                    if !live && self.script.second_holds {
                        continue;
                    }
                    if !live
                        && self.script.heard_as_call
                        && self.seen.lock().unwrap().second.iter().any(|input| {
                            matches!(input, ProviderInput::StepCall { request: called, .. } if *called == request)
                        })
                    {
                        if self.script.heard_as_call_late && self.call_words_held.is_none() {
                            self.call_words_held = Some((index, request));
                            continue;
                        }
                        let queued = &mut self.peers[index].queued;
                        if self.script.late_words_first {
                            queued.push_back(ProviderOutput::Transcript {
                                request: None,
                                text: "an earlier line".into(),
                                finished: true,
                            });
                        }
                        queued.push_back(ProviderOutput::StepCallHeard { request });
                        queued.push_back(ProviderOutput::TurnComplete {
                            request,
                            idle: true,
                        });
                        continue;
                    }
                    if !live && self.script.second_waits_first && self.ended.len() == 1 {
                        if let Some(text) = self.script.second_hears {
                            self.peers[index]
                                .queued
                                .push_back(ProviderOutput::Transcript {
                                    request: None,
                                    text: text.into(),
                                    finished: true,
                                });
                        }
                        continue;
                    }
                    if !live && self.script.second_unheard.contains(&self.ended.len()) {
                        self.peers[index]
                            .queued
                            .push_back(ProviderOutput::TurnComplete {
                                request,
                                idle: true,
                            });
                        continue;
                    }
                    if !live && self.script.second_slow.contains(&self.ended.len()) {
                        self.peers[index]
                            .queued
                            .push_back(ProviderOutput::TurnFailed {
                                request,
                                stage: FailedStage::AwaitingResponse,
                                reason: Fault::ResponseTimeout,
                            });
                        continue;
                    }
                    if !live
                        && let Some(reason) = self.script.second_fails
                        && self
                            .script
                            .second_fails_first
                            .is_none_or(|first| self.ended.len() <= first)
                    {
                        self.peers[index]
                            .queued
                            .push_back(ProviderOutput::TurnFailed {
                                request,
                                stage: FailedStage::AwaitingResponse,
                                reason,
                            });
                        continue;
                    }
                    // Alone, a holding phrase is answered as the second worker
                    // answers one (the holding answers, #1722).
                    if !live
                        && self.script.only
                        && let Some(Live::Holding(packets)) = self
                            .live_requests
                            .iter()
                            .position(|&r| r == request)
                            .and_then(|p| self.script.live.get(p).copied())
                    {
                        self.audio(index, request, packets, SECOND);
                        self.peers[index]
                            .queued
                            .push_back(ProviderOutput::HoldingAnswer {
                                request,
                                session: 1,
                            });
                        self.finish(index, request);
                        continue;
                    }
                    if !live {
                        // Alone, a line the script makes unclear is held
                        // as the second worker holds one (the hums, #828).
                        let unclear = self
                            .live_requests
                            .iter()
                            .position(|&r| r == request)
                            .and_then(|p| self.script.live.get(p).copied())
                            .filter(|answer| {
                                self.script.only
                                    && matches!(answer, Live::Unclear | Live::UnclearCutIn)
                            });
                        if let Some(answer) = unclear {
                            self.peers[index].queued.push_back(ProviderOutput::Unclear {
                                request,
                                why: "fixture".into(),
                                cut_in: answer == Live::UnclearCutIn,
                            });
                            self.finish(index, request);
                            continue;
                        }
                        if self.script.second_looks
                            && !std::mem::replace(&mut self.second_looked, true)
                        {
                            self.peers[index]
                                .queued
                                .push_back(ProviderOutput::LookAsked {
                                    request,
                                    session: self.second_session,
                                    dropped: None,
                                    search: !self.script.own_look,
                                    // The skill's call, at the place the
                                    // person named, or none: the desk.
                                    call: (!self.script.own_look).then(|| {
                                        crate::provider_worker::SearchCall {
                                            name: "look_for".into(),
                                            args: self.script.first_place.map_or_else(
                                                || json!({}),
                                                |place| json!({"place": place}),
                                            ),
                                        }
                                    }),
                                });
                        }
                        // A wait cue first: audio with no words.
                        if self.script.second_cue > 0 {
                            self.audio(index, request, self.script.second_cue, CUE);
                        }
                        // The first request's words only, when it waits.
                        if let Some(text) = self.script.second_hears.filter(|_| {
                            !self.script.second_waits_first && !self.script.hears_at_start
                        }) {
                            self.hears(index, text);
                        }
                        if let Some(text) = self.script.second_says {
                            self.peers[index]
                                .queued
                                .push_back(ProviderOutput::Transcript {
                                    request: Some(request),
                                    text: text.into(),
                                    finished: true,
                                });
                        }
                        match self.script.second_shape {
                            Some(shape) => {
                                for packet in 0..self.script.second_packets {
                                    self.audio(index, request, 1, shape(packet));
                                }
                            }
                            None => self.audio(index, request, self.script.second_packets, SECOND),
                        }
                        self.finish(index, request);
                        continue;
                    }
                    let queued = &mut self.peers[index].queued;
                    for (at, piece) in self.script.live_hears.iter().enumerate() {
                        queued.push_back(ProviderOutput::Transcript {
                            request: None,
                            text: (*piece).into(),
                            finished: at + 1 == self.script.live_hears.len(),
                        });
                    }
                    if let Some(text) = self.script.live_says {
                        queued.push_back(ProviderOutput::Transcript {
                            request: Some(request),
                            text: text.into(),
                            finished: false,
                        });
                    }
                    let position = self.live_requests.iter().position(|&r| r == request);
                    // After the scripted ones, the Live route answers.
                    let answer = position
                        .and_then(|p| self.script.live.get(p).copied())
                        .unwrap_or(Live::Answer(12));
                    let fail = |reason| ProviderOutput::TurnFailed {
                        request,
                        stage: FailedStage::AwaitingResponse,
                        reason,
                    };
                    match answer {
                        Live::Answer(packets) => {
                            self.audio(index, request, packets, LIVE);
                            self.finish(index, request);
                        }
                        Live::Fail(reason) => self.peers[index].queued.push_back(fail(reason)),
                        Live::FailThenLate(reason, packets) => {
                            self.peers[index].queued.push_back(fail(reason));
                            self.audio(index, request, packets, LIVE);
                            self.finish(index, request);
                        }
                        Live::DropLink => {}
                        Live::Reconnect(packets) => {
                            self.audio(index, request, packets, LIVE);
                            self.finish(index, request);
                        }
                        Live::Look(packets, session) => {
                            self.peers[index]
                                .queued
                                .push_back(ProviderOutput::LookAsked {
                                    request,
                                    session,
                                    dropped: None,
                                    search: false,
                                    call: None,
                                });
                            self.audio(index, request, packets, LIVE);
                            self.finish(index, request);
                        }
                        Live::Holding(packets) => {
                            self.audio(index, request, packets, LIVE);
                            self.peers[index]
                                .queued
                                .push_back(ProviderOutput::HoldingAnswer {
                                    request,
                                    session: 1,
                                });
                            self.finish(index, request);
                        }
                        Live::Unclear | Live::UnclearCutIn => {
                            self.peers[index].queued.push_back(ProviderOutput::Unclear {
                                request,
                                why: "fixture".into(),
                                cut_in: answer == Live::UnclearCutIn,
                            });
                            self.finish(index, request);
                        }
                        Live::LookSilent | Live::LookUnclear | Live::LookFail(..) => {
                            let queued = &mut self.peers[index].queued;
                            queued.push_back(ProviderOutput::LookAsked {
                                request,
                                session: 1,
                                dropped: None,
                                search: false,
                                call: None,
                            });
                            match answer {
                                Live::LookSilent => {
                                    queued.push_back(ProviderOutput::Transcript {
                                        request: Some(request),
                                        text: "...".into(),
                                        finished: true,
                                    });
                                    self.finish(index, request);
                                }
                                Live::LookUnclear => {
                                    queued.push_back(ProviderOutput::Unclear {
                                        request,
                                        why: "fixture".into(),
                                        cut_in: false,
                                    });
                                    self.finish(index, request);
                                }
                                Live::LookFail(reason, packets) => {
                                    queued.push_back(fail(reason));
                                    self.audio(index, request, packets, LIVE);
                                    self.finish(index, request);
                                }
                                _ => unreachable!(),
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let peer = &mut self.peers[index];
        while let Some(event) = peer.queued.front() {
            match peer.output.try_send(|| peer.channels.data.send(event)) {
                Ok(true) => {
                    peer.queued.pop_front();
                }
                Ok(false) | Err(_) => break,
            }
        }
    }

    fn tick_speaker(&mut self, index: usize) {
        if crate::scenario_clock::instant() < self.next_write {
            return;
        }
        let Ok(Some(chunk)) = self.peers[index].channels.data.receive::<SpeakerChunk>() else {
            return;
        };
        self.next_write = self
            .next_write
            .max(crate::scenario_clock::instant() - Duration::from_millis(20))
            + Duration::from_micros(chunk.samples.len() as u64 * 1_000 / 24);
        self.install(chunk.snapshot);
        let accepted = self
            .guard
            .check(now(), chunk.permit, OutputKind::Speech)
            .is_ok();
        let owner = chunk.permit.owner();
        self.seen.lock().unwrap().chunks.push(Chunk {
            turn: owner.turn(),
            value: chunk.samples[0],
            samples: chunk.samples.clone(),
            accepted,
        });
        let channels = &mut self.peers[index].channels;
        if !accepted {
            let _ = channels.control.send(WorkerEvent::PlaybackRejected {
                turn: owner.turn(),
                sequence: chunk.chunk_sequence,
                reason: "fixture guard refused the permit".into(),
                lapsed: false,
            });
            return;
        }
        self.accepted += 1;
        let first = PlaybackCursor {
            epoch: 1,
            frame: self.cursor,
        };
        self.cursor += chunk.samples.len() as u64;
        let end = PlaybackCursor {
            epoch: 1,
            frame: self.cursor,
        };
        let at = monotonic_us();
        let _ = channels.control.send(WorkerEvent::Playback {
            owner,
            first_sample: first,
            end_sample: end,
            queue_observed_at_us: at,
            turn: owner.turn(),
            sequence: chunk.chunk_sequence,
            queued_frames: 0,
            accepted_frames: chunk.samples.len(),
            observed_at_us: at,
        });
        let _ = channels.control.send(WorkerEvent::SpeechDrained {
            owner,
            sequence: chunk.chunk_sequence,
            last_sample: end,
            observed_at_us: at,
        });
        if chunk.end_of_speech {
            let _ = channels.control.send(WorkerEvent::SpeechRetired {
                owner,
                sequence: chunk.chunk_sequence,
                final_sample: end,
                observed_at_us: at,
            });
        }
    }

    fn install(&mut self, snapshot: Snapshot) {
        if self.guard.install(now(), snapshot).is_ok() {
            self.seen.lock().unwrap().installs.push((
                snapshot.owner().map(|owner| owner.turn()),
                snapshot.microphone_permission() == Permission::Allowed,
                snapshot.camera_permission() == Permission::Allowed,
            ));
        }
    }
}

struct PrivacyPeer {
    peer: Peer,
    seen: Arc<Mutex<Seen>>,
    muted: Arc<AtomicBool>,
    next: Instant,
}

impl PrivacyPeer {
    /// A fresh observation every 5 ms of scenario time.
    fn tick(&mut self, deadline: Option<Instant>) -> io::Result<()> {
        while !self.peer.stopped
            && let Ok(Some(control)) = self.peer.channels.control.receive::<Control>()
        {
            if matches!(control, Control::Stop) {
                self.peer.stopped = true;
                self.peer.close_input(deadline)?;
                self.seen.lock().unwrap().stopped.push("privacy");
            }
        }
        if !self.peer.stopped && crate::scenario_clock::instant() >= self.next {
            let _ = self.peer.channels.control.send(WorkerEvent::Privacy {
                receipt: None,
                acquired_at_us: monotonic_us(),
                muted: self.muted.load(Ordering::SeqCst),
            });
            self.next = crate::scenario_clock::instant() + Duration::from_millis(5);
        }
        Ok(())
    }
}

/// The clock the run's hand controls are stamped with (`Script::hand_stop`).
const HAND_CLOCK: &str = "second-route-hand";

/// Controls' state with one tap of the button that stops, at `at_us`.
fn hand_stop_state(at_us: u64) -> lamp_msgs::ControlsState {
    use lamp_msgs::{ClockStamp, ControlGesture, ControlIntent, ControlSource, Gesture, HoldTier};
    let stamp = || ClockStamp {
        clock_id: HAND_CLOCK.into(),
        at_us,
        sequence: 1,
    };
    let state = lamp_msgs::ControlsState {
        stamp: stamp(),
        session_id: "second-route-controls".into(),
        enabled: true,
        ready: true,
        fault: None,
        buttons: vec![],
        head_active: false,
        base_ready: false,
        base_held_us: None,
        base_tier: HoldTier::None,
        latest_stop: Some(ControlGesture {
            stamp: stamp(),
            session_id: "second-route-controls".into(),
            source: ControlSource::Button,
            name: "primary".into(),
            gesture: Gesture::Tap,
            intent: ControlIntent::Stop,
            duration_us: 100_000,
            released_us: at_us,
            enabled: true,
        }),
        latest_mute: None,
        event_sequence: 1,
        rejected_inputs: 0,
        privacy: None,
        no_privacy_slide: false,
    };
    state.check().unwrap();
    state
}

fn run(script: Script) -> Run {
    let limit = script.limit;
    let notices = if script.notices {
        FailureNotices::synthetic_offline()
    } else {
        FailureNotices::none()
    };
    let directory = SessionDirectory::create().unwrap();
    let mut peers = Vec::new();
    let mut spawn = |role| {
        let (worker, channels, input) = Worker::cooperative_ready_fixture(&directory.path, role);
        peers.push(Peer {
            role,
            channels,
            input: Some(input),
            child_pid: worker.fixture_pid(),
            stopped: false,
            output: OutputSender::default(),
            queued: VecDeque::new(),
            snapshot: None,
        });
        worker
    };
    // The workers are the ones the service's routes start (`spawn_rig`).
    let routes = if script.only {
        second::Routes::SecondOnly(PathBuf::from("/etc/second.json"))
    } else {
        second::Routes::Live {
            standby: script.second.then(|| PathBuf::from("/etc/second.json")),
        }
    };
    let workers = routes
        .workers(
            vec!["/etc/provider.json".to_owned()],
            second::RouteFlags::default(),
        )
        .unwrap();
    let mut rig = Rig {
        expression_out: None,
        second: None,
        call_wait_blocks: None,
        cut_keeps_name: false,
        asker_first: crate::asker_first::Mode::Off,
        follows_on: false,
        paused: Box::new(|| false),
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: script.early_end_blocks,
        early_finalize: false,
        open_name: script.open_name,
        call_takes_next: script.call_takes_next,
        unfinished_wait: false,
        faint_level: None,
        speaker: spawn("speaker"),
        capture: spawn("capture"),
        provider: spawn(workers.role),
        privacy: spawn("privacy"),
        privacy_controls: false,
        capture_held: false,
        privacy_receipts: false,
        ring: Some(spawn("ring")).into(),
        camera: None,
        motor: None,
        ring_channel_ceiling: Some(24),
        diagnostic_directory: None,
        noise_suppression: NoiseSuppression::On,
        turn_taking: script.turn_taking,
        playout_start: PlayoutStart::default(),
        fixture_at_ms: None,
        endpoint: None,
    };
    if workers.standby.is_some() {
        let worker = spawn("second");
        rig.second = Some(second::Route::standing_by(worker, ready_link(1, 1)));
    }
    // What the voice says for Expression, on a bus of the run's own, kept by
    // a thread as it comes so that no subscriber's queue fills.
    let expression = script.expression.then(|| {
        use lamp_msgs::{VoiceProgress, VoiceSound};
        static RUNS: AtomicU64 = AtomicU64::new(0);
        let root = PathBuf::from(format!(
            "/tmp/lv-route-expr-{}-{}",
            std::process::id(),
            RUNS.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        rig.expression_out = Some(
            crate::expression_out::ExpressionOut::on(&bus, EXPRESSION_CLOCK, "voice-route")
                .unwrap(),
        );
        let progress = bus
            .subscribe_typed(VoiceProgress::TOPIC, "test", VoiceProgress::TYPE)
            .unwrap();
        let sound = bus
            .subscribe_typed(VoiceSound::TOPIC, "test", VoiceSound::TYPE)
            .unwrap();
        let done = Arc::new(AtomicBool::new(false));
        // One thread for each topic, so that neither waits on the other: a
        // Linux socket holds about ten datagrams, and the bus drops one sent
        // to a full queue, so a keeper busy with progress lost sound pieces
        // (Second's read on #1722, 20 Oct 00:01; Eighth's 4 of 6 on Linux).
        let keep = |stop: Arc<AtomicBool>,
                    mut subscriber: lamp_bus::Subscriber,
                    input: fn(lamp_bus::Message) -> lamp_expression::Input| {
            std::thread::spawn(move || {
                let mut kept = Vec::new();
                loop {
                    let stopping = stop.load(Ordering::Acquire);
                    while let Ok(Some(message)) = subscriber.recv(Duration::from_millis(5)) {
                        kept.push(input(message));
                    }
                    if stopping {
                        return kept;
                    }
                }
            })
        };
        let progress = keep(done.clone(), progress, |message| {
            lamp_expression::Input::Progress(message.parse().unwrap())
        });
        let sound = keep(done.clone(), sound, |message| {
            lamp_expression::Input::Sound(message.parse().unwrap())
        });
        let keeper = std::thread::spawn(move || {
            let mut kept = progress.join().unwrap();
            kept.extend(sound.join().unwrap());
            kept
        });
        (root, done, keeper)
    });
    // The clock starts once every child has crossed its startup barrier;
    // the coordinator and the peers share it from here.
    let clock_scope = crate::scenario_clock::Scope::enter(limit * 2).unwrap();
    let origin = monotonic_us();
    // The step's own rule, on the step's own evidence: its Unix clock is
    // the scenario's origin at `UNIX`.
    const UNIX: u64 = 1_760_000_000_000;
    let near_flow = |from: u64| {
        crate::onboarding::near::NearFlow::listening_for_test(
            UNIX + from,
            origin + from * 1_000,
            &script
                .near_name_ms
                .map(|at| UNIX + at)
                .into_iter()
                .collect::<Vec<_>>(),
        )
    };
    let near_opens = script
        .near_opens_ms
        .zip(script.near_window_ms)
        .map(|(opens, from)| (origin + opens * 1_000, near_flow(from)));
    TEST_NEAR.with(|near| {
        *near.borrow_mut() = script
            .near_window_ms
            .filter(|_| near_opens.is_none())
            .map(near_flow)
    });
    TEST_SENTENCE_WINDOW.with(|window| {
        window.set(
            script
                .sentence_window_ms
                .map(|(from, to)| (origin + from * 1_000, origin + to * 1_000)),
        )
    });
    let mut owner = Controller::new(new_boot().unwrap(), now());
    let boot = owner.snapshot(now()).unwrap().boot();
    let mut startup = StartupEvents::default();
    if script.live_late.is_none() {
        startup
            .cloud
            .observe(ready_link(1, 1), monotonic_us())
            .unwrap();
    }
    let seen = Arc::new(Mutex::new(Seen::default()));
    let muted = Arc::new(AtomicBool::new(false));
    let (mut controls, hand) = match script.hand_stop {
        Some(_) => {
            let (controls, hand) =
                HandControls::with_test_sender(HAND_CLOCK, monotonic_us()).unwrap();
            (Some(controls), Some(hand))
        }
        None => (None, None),
    };
    let privacy_at = peers.iter().position(|p| p.role == "privacy").unwrap();
    let mut privacy = PrivacyPeer {
        peer: peers.remove(privacy_at),
        seen: seen.clone(),
        muted: muted.clone(),
        next: crate::scenario_clock::instant(),
    };
    let mut state = Peers {
        script,
        peers,
        seen: seen.clone(),
        muted,
        guard: BoundaryGuard::new(boot, now()),
        capture_started: false,
        frames: 0,
        next_capture: crate::scenario_clock::instant(),
        next_write: crate::scenario_clock::instant(),
        cursor: 0,
        accepted: 0,
        speech_offset: None,
        live_requests: Vec::new(),
        ended: BTreeSet::new(),
        settled: 0,
        audio_sequence: 0,
        link_dropped: false,
        link_back: false,
        live_ready_sent: false,
        second_died_at: None,
        second_session: 0,
        second_looked: false,
        look_turns: 0,
        murmur_at: None,
        pushed_at: None,
        last_hand_publish_us: None,
        held_answer: None,
        hand,
        near_opens,
        call_words_held: None,
    };
    let started = crate::scenario_clock::instant();
    let mut reported = false;
    clock_scope
        .set_pump(move |phase| {
            // A real child's exit is not simulated: one deadline bounds the
            // children a step stops, and shutdown brings its own budget.
            let deadline = match phase {
                crate::scenario_clock::Phase::Run => Instant::now() + Duration::from_millis(300),
                crate::scenario_clock::Phase::Shutdown { deadline } => deadline,
            };
            if crate::scenario_clock::instant() - started > limit && !reported {
                reported = true;
                if let Some(index) = state.index("capture") {
                    let _ = state.peers[index]
                        .channels
                        .control
                        .send(WorkerEvent::Fault {
                            code: "second route scenario limit".into(),
                        });
                }
            }
            privacy.tick(Some(deadline))?;
            state.tick(Some(deadline))
        })
        .unwrap();
    let mut reply = None;
    let mut trace = SessionState::default();
    let result = conversation_with_controls(
        &mut rig,
        &mut owner,
        Lifetime::Serve,
        &mut trace,
        startup,
        &mut reply,
        &mut None,
        &Termination::inactive(),
        &notices,
        &mut controls,
        &mut None,
    );
    let result = finish_active_run(
        &mut rig, &mut owner, &mut reply, &mut trace, &mut None, result,
    );
    TEST_NEAR.with(|near| near.borrow_mut().take());
    TEST_SENTENCE_WINDOW.with(|window| window.set(None));
    let mut exits = vec![
        ("speaker", matches!(rig.speaker.try_exit(), Ok(Some(_)))),
        ("capture", matches!(rig.capture.try_exit(), Ok(Some(_)))),
        ("privacy", matches!(rig.privacy.try_exit(), Ok(Some(_)))),
    ];
    for ring in rig.ring.iter_mut() {
        exits.push(("ring", matches!(ring.try_exit(), Ok(Some(_)))));
    }
    // After a switch either route's worker may be `rig.provider`.
    let mut routes = vec![&mut rig.provider];
    routes.extend(rig.second.as_mut().map(second::Route::worker_mut));
    for worker in routes {
        let role = if worker.role == "second" {
            "second"
        } else {
            "provider"
        };
        exits.push((role, matches!(worker.try_exit(), Ok(Some(_)))));
    }
    drop(clock_scope);
    let expression = expression.map_or_else(Vec::new, |(root, done, keeper)| {
        done.store(true, Ordering::Release);
        let kept = keeper.join().unwrap();
        let _ = std::fs::remove_dir_all(&root);
        kept
    });
    let seen = std::mem::take(&mut *seen.lock().unwrap());
    Run {
        result,
        trace: trace.iter().cloned().collect(),
        seen,
        exits,
        expression,
    }
}

/// The records whose kinds and fields are compared across runs: what a turn
/// leaves, with neither timing nor the route's own records.
fn shape(run: &Run) -> BTreeSet<(String, Vec<String>)> {
    // Nor when an answer began to be heard: on the second route that needs
    // its words, which these peers send only when a script asks
    // (`the_answers_first_audio_is_recorded_after_a_wait_cue_never_at_it`).
    run.trace
        .iter()
        .filter(|e| e["kind"] != "voice_route_switched" && e["kind"] != "answer_first_audio")
        .map(|e| {
            let mut keys: Vec<String> = e.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            (e["kind"].as_str().unwrap().to_owned(), keys)
        })
        .collect()
}

/// The Live packets with their times left out.
fn timeless(inputs: &[ProviderInput]) -> Vec<String> {
    inputs
        .iter()
        .map(|input| match input {
            ProviderInput::Audio {
                request,
                privacy_generation,
                sequence,
                samples,
                ..
            } => format!("audio {request} {privacy_generation} {sequence} {samples:?}"),
            other => format!("{other:?}"),
        })
        .collect()
}

/// (a) Without a second route nothing of the switch is seen, and a route
/// standing by changes nothing while Gemini is well: the Live packets and
/// the kinds and fields of every record are the same.
/// #941, the reviewer's 22:23 P2 and P3 on #1146: a skill call with a look,
/// through the loop, asks the eyes for one still, and only once the turn's
/// first audio is played, as the head lamp-skills sent down is on its way;
/// a plain call asks for none.
#[test]
fn a_tool_s_look_asks_the_eyes_once_its_first_audio_plays_and_a_plain_call_asks_none() {
    let still =
        std::env::temp_dir().join(format!("lamp-voice-loop-look-{}.jpg", std::process::id()));
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still));
    let looked = run(Script {
        second: false,
        ..Script::question(Live::Look(12, 1))
    });
    let plain = run(Script {
        second: false,
        ..Script::question(Live::Answer(12))
    });
    // The first builder's 22:32 P2: a call from a session that is no
    // longer the one ready is dropped, and says so.
    let elsewhere = run(Script {
        second: false,
        ..Script::question(Live::Look(12, 99))
    });
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    let at = |run: &Run, kind: &str| {
        run.trace
            .iter()
            .position(|e| e["kind"] == kind)
            .unwrap_or_else(|| panic!("no {kind}"))
    };
    let asks = |run: &Run| {
        run.trace
            .iter()
            .filter(|e| e["kind"] == "look_full_asked")
            .count()
    };
    assert_eq!(asks(&looked), 1);
    assert!(
        looked
            .trace
            .iter()
            .any(|e| e["kind"] == "look_asked" && e["by"] == "tool")
    );
    assert!(at(&looked, "speaker_first_write") < at(&looked, "look_full_asked"));
    assert_eq!(asks(&plain), 0);
    assert!(!plain.trace.iter().any(|e| e["kind"] == "look_asked"));
    let dropped = |run: &Run| {
        run.trace
            .iter()
            .filter(|e| e["kind"] == "look_dropped")
            .map(|e| e["why"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert!(dropped(&looked).is_empty(), "{:?}", dropped(&looked));
    assert_eq!(
        dropped(&elsewhere),
        ["the session that made the call has gone"]
    );
    assert!(
        !elsewhere
            .trace
            .iter()
            .any(|e| e["kind"] == "look_full_still")
    );
    // The reviewer's 23:47 P2 on #1172: dropped before the eyes are asked.
    assert_eq!(asks(&elsewhere), 0);
}

/// The reviewer's 23:47 P3 on #1172: a tool's look whose turn says nothing,
/// is unclear, or is taken by the second route asks the eyes for nothing,
/// and says why.
#[test]
fn a_tool_s_look_on_an_unsaid_unclear_or_handed_over_turn_asks_the_eyes_nothing() {
    let _one = super::pilot_faults::one_session();
    let still = std::env::temp_dir().join(format!(
        "lamp-voice-loop-look-dropped-{}.jpg",
        std::process::id()
    ));
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still));
    let cases = [
        (Live::LookSilent, false, "its answer said nothing"),
        (Live::LookUnclear, false, "its turn is asked again"),
        (
            Live::LookFail(Fault::RateLimited, 6),
            true,
            "the session that made the call has gone",
        ),
    ];
    let runs: Vec<_> = cases
        .iter()
        .map(|(live, second, _)| {
            run(Script {
                second: *second,
                ..Script::question(*live)
            })
        })
        .collect();
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    for ((live, _, why), run) in cases.iter().zip(&runs) {
        let kinds = |kind: &str| run.trace.iter().filter(|e| e["kind"] == kind).count();
        assert_eq!(kinds("look_asked"), 1, "{live:?}");
        assert_eq!(kinds("look_full_asked"), 0, "{live:?}");
        let dropped: Vec<_> = run
            .trace
            .iter()
            .filter(|e| e["kind"] == "look_dropped")
            .map(|e| e["why"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(dropped, [*why], "{live:?}");
    }
}

/// The reviewer's 01:57 P3 on #1172: once a tool's look is dropped because
/// the second route took its turn, the second route's own "Let me take a
/// look." on that turn asks for its own look.
#[test]
fn the_second_route_s_own_phrase_asks_its_look_after_the_tool_s_is_dropped() {
    let _one = super::pilot_faults::one_session();
    let still = std::env::temp_dir().join(format!(
        "lamp-voice-loop-look-second-{}.jpg",
        std::process::id()
    ));
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still));
    let run = run(Script {
        second_says: Some("Let me take a look."),
        ..Script::question(Live::LookFail(Fault::RateLimited, 6))
    });
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    let asked: Vec<_> = run
        .trace
        .iter()
        .filter(|e| e["kind"] == "look_asked")
        .map(|e| (e["turn"].as_u64(), e["by"].as_str()))
        .collect();
    assert_eq!(asked, [(Some(1), Some("tool")), (Some(1), None)]);
    let dropped: Vec<_> = run
        .trace
        .iter()
        .filter(|e| e["kind"] == "look_dropped")
        .map(|e| e["by"].as_str().unwrap())
        .collect();
    assert_eq!(dropped, ["tool"]);
    assert_eq!(
        run.trace
            .iter()
            .filter(|e| e["kind"] == "look_full_asked")
            .count(),
        1
    );
}

/// A skill's look step on the second route (`LAMP_SECOND_SKILL_LOOK`,
/// #941): the look its worker asks for its own session is taken once the
/// answer's first audio plays, never before it, and its still goes to that
/// session as the `lamp_looks` written turn with the picture. Standing by
/// with Gemini's link gone, and as the only route. Its answer plain, no
/// look is asked.
#[test]
fn a_skills_look_on_the_second_route_takes_the_still_for_its_own_session() {
    let _one = super::pilot_faults::one_session();
    let still = std::env::temp_dir().join(format!(
        "lamp-voice-loop-look-skill-{}.jpg",
        std::process::id()
    ));
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still.clone()));
    TEST_EYES_ANSWER.set(true);
    let script = |only: bool, looks: bool| Script {
        only,
        second: !only,
        second_looks: looks,
        // The look's turn has time to go before the run ends.
        mute_at_frame: Some(500),
        written: Written::Said(6),
        ..Script::question(if only {
            Live::Answer(12)
        } else {
            Live::DropLink
        })
    };
    let standing_by = run(script(false, true));
    let alone = run(script(true, true));
    let plain = run(script(false, false));
    TEST_EYES_ANSWER.set(false);
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    let _ = std::fs::remove_file(&still);
    for (case, run) in [("standing by", &standing_by), ("alone", &alone)] {
        let at = |kind: &str| {
            run.trace
                .iter()
                .position(|e| e["kind"] == kind)
                .unwrap_or_else(|| panic!("{case}: no {kind}: {}", run.summary()))
        };
        assert!(
            run.trace
                .iter()
                .any(|e| e["kind"] == "look_asked" && e["by"] == "tool" && e["turn"] == 1),
            "{case}"
        );
        let dropped: Vec<_> = run.events("look_dropped");
        assert!(dropped.is_empty(), "{case}: {dropped:?}");
        // Asked of the eyes once the answer is heard, not before.
        assert!(at("speaker_first_write") < at("look_full_asked"), "{case}");
        let still: Vec<_> = run.events("look_full_still");
        assert_eq!(still.len(), 1, "{case}");
        assert_eq!(still[0]["still"], true, "{case}");
        // The look's written turn, on the second route's own session.
        let session = run.seen.second.iter().find_map(|input| match input {
            ProviderInput::Start { session, .. } => Some(*session),
            _ => None,
        });
        let looks: Vec<_> = run
            .seen
            .second
            .iter()
            .filter_map(|input| match input {
                ProviderInput::Written {
                    session,
                    text,
                    look,
                    ..
                } => Some((Some(*session), text.as_str(), *look)),
                _ => None,
            })
            .collect();
        assert_eq!(
            looks,
            [(session, crate::speak_up::LOOK_FULL_NOTE, true)],
            "{case}"
        );
        assert!(
            run.trace
                .iter()
                .any(|e| e["kind"] == "input_admitted" && e["admission_basis"] == "lamp_looks"),
            "{case}"
        );
    }
    assert!(plain.events("look_asked").is_empty());
    assert!(plain.events("look_full_asked").is_empty());
    assert!(
        !plain
            .seen
            .second
            .iter()
            .any(|input| matches!(input, ProviderInput::Written { .. }))
    );
}

/// The first builder on this change's plan (20 Oct): a skill's look on
/// the second route is dropped the moment the person speaks, as every look
/// is when someone speaking first takes the turn; no look's turn goes.
#[test]
fn a_skills_look_on_the_second_route_is_dropped_when_the_person_speaks() {
    let _one = super::pilot_faults::one_session();
    let still = std::env::temp_dir().join(format!(
        "lamp-voice-loop-look-skill-cut-{}.jpg",
        std::process::id()
    ));
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still.clone()));
    TEST_EYES_ANSWER.set(true);
    let run = run(Script {
        only: true,
        second: false,
        second_looks: true,
        second_packets: 60,
        speak_again: Some(When::Accepted(8)),
        turns: 2,
        turn_taking: TurnTaking::VadOnly,
        written: Written::Said(6),
        ..Script::question(Live::Answer(60))
    });
    TEST_EYES_ANSWER.set(false);
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    let _ = std::fs::remove_file(&still);
    assert_eq!(run.finished(1)["outcome"], "user_interrupted");
    let dropped: Vec<_> = run
        .events("look_dropped")
        .into_iter()
        .map(|e| (e["turn"].clone(), e["by"].clone()))
        .collect();
    assert_eq!(dropped, [(json!(1), json!("tool"))], "{}", run.summary());
    assert!(run.events("look_full_still").is_empty());
    assert!(
        !run.seen
            .second
            .iter()
            .any(|input| matches!(input, ProviderInput::Written { look: true, .. }))
    );
}

/// A skill's search on the second route alone (`LAMP_SECOND_SKILL_LOOK`),
/// its look's turns answered as `look_answers` says, the note's as `note`,
/// with `LAMP_SECOND_LOOK_NOTE` on or off.
fn search(
    on: bool,
    look_answers: &'static [LookAnswer],
    note: LookAnswer,
    speak_again: Option<When>,
) -> Run {
    search_with(on, look_answers, note, speak_again, None)
}

/// As [`search`], with a murmur and a hand Stop that many capture frames
/// after it (`Script::murmur`, `Script::hand_stop`).
fn search_with(
    on: bool,
    look_answers: &'static [LookAnswer],
    note: LookAnswer,
    speak_again: Option<When>,
    murmur_then_stop: Option<(When, u64)>,
) -> Run {
    let still = std::env::temp_dir().join(format!(
        "lamp-voice-loop-look-note-{}.jpg",
        std::process::id()
    ));
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still.clone()));
    TEST_EYES_ANSWER.set(true);
    TEST_LOOK_NOTE.set(on);
    let run = run(Script {
        only: true,
        second: false,
        second_looks: true,
        look_answers,
        note_answer: note,
        speak_again,
        murmur: murmur_then_stop.map(|(murmur, _)| murmur),
        hand_stop: murmur_then_stop.map(|(_, after)| after),
        turns: if speak_again.is_some() { 3 } else { 1 },
        turn_taking: TurnTaking::VadOnly,
        mute_at_frame: Some(1_100),
        written: Written::Said(6),
        ..Script::question(Live::Answer(12))
    });
    TEST_LOOK_NOTE.set(false);
    TEST_EYES_ANSWER.set(false);
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    let _ = std::fs::remove_file(&still);
    run
}

/// The look's notes the second route was sent, by request.
fn look_notes(run: &Run) -> Vec<u64> {
    run.seen
        .second
        .iter()
        .filter_map(|input| match input {
            ProviderInput::Written { request, text, .. }
                if *text == super::look_note::look_for_things().note =>
            {
                Some(*request)
            }
            _ => None,
        })
        .collect()
}

/// The written turns sent to the second route as a search's own.
fn search_turns(run: &Run) -> Vec<u64> {
    run.seen
        .second
        .iter()
        .filter_map(|input| match input {
            ProviderInput::Written {
                request,
                search: true,
                ..
            } => Some(*request),
            _ => None,
        })
        .collect()
}

/// The look's turns (`lamp_looks`) the second route was sent, by request.
fn look_turns(run: &Run) -> Vec<u64> {
    run.trace
        .iter()
        .filter(|e| e["kind"] == "input_admitted" && e["admission_basis"] == "lamp_looks")
        .map(|e| e["turn"].as_u64().unwrap())
        .collect()
}

/// (a) The first builder's step 2 for look-for-things on the second route
/// (20 Oct): a skill's search whose look got an answer that called nothing
/// is sent one note to go on, once that answer has played and the room has
/// been quiet a second, on the same session; the note's answer is said.
#[test]
fn a_skills_look_answered_with_no_call_gets_one_note_after_it_plays() {
    let _one = super::pilot_faults::one_session();
    let run = search(true, &[LookAnswer::Nothing], LookAnswer::Other, None);
    run.assert_clean();
    let looks = look_turns(&run);
    assert_eq!(looks.len(), 1, "{}", run.summary());
    let notes = look_notes(&run);
    assert_eq!(notes.len(), 1, "{:?}", run.events("look_note_dropped"));
    let sent = run.events("look_note_sent");
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["turn"], looks[0]);
    assert_eq!(sent[0]["next_turn"], notes[0]);
    // After the look's answer played whole, and a second of quiet.
    let at = |kind: &str, turn: u64| {
        run.trace
            .iter()
            .position(|e| e["kind"] == kind && e["turn"] == turn)
            .unwrap_or_else(|| panic!("no {kind} for {turn}: {}", run.summary()))
    };
    assert!(at("turn_finished", looks[0]) < at("look_note_sent", looks[0]));
    assert_eq!(run.played(looks[0]), vec![SECOND; 24]);
    assert_eq!(run.played(notes[0]), vec![SECOND; 24]);
    let session = run.seen.second.iter().find_map(|input| match input {
        ProviderInput::Start { session, .. } => Some(*session),
        _ => None,
    });
    assert!(run.seen.second.iter().any(|input| matches!(input,
        ProviderInput::Written { request, session: of, look: false, .. }
            if *request == notes[0] && Some(*of) == session)));
    assert!(run.events("look_note_dropped").is_empty());
}

/// (b) Once a search: the note's answer calls the next place, its look is
/// taken there (the note's turn goes on with the search), and that look's
/// answer calls nothing again: no second note.
#[test]
fn a_second_look_left_in_the_same_search_gets_no_second_note() {
    let _one = super::pilot_faults::one_session();
    let run = search(
        true,
        &[LookAnswer::Nothing, LookAnswer::Nothing],
        LookAnswer::Next,
        None,
    );
    run.assert_clean();
    let looks = look_turns(&run);
    assert_eq!(looks.len(), 2, "{}", run.summary());
    let notes = look_notes(&run);
    assert_eq!(notes.len(), 1, "{}", run.summary());
    // The search's own turns, in order: look, note, look.
    assert_eq!(search_turns(&run), [looks[0], notes[0], looks[1]]);
    assert!(run.events("look_dropped").is_empty());
    let dropped: Vec<_> = run
        .events("look_note_dropped")
        .into_iter()
        .map(|e| (e["turn"].clone(), e["why"].clone()))
        .collect();
    assert_eq!(
        dropped,
        [(json!(looks[1]), json!("its search had its note"))]
    );
}

/// (c) The chain: the look's answer calls `look_for` for the next place,
/// that look is taken in the look's own turn and the next `lamp_looks` turn
/// goes, also the search's; its answer called something else: no note.
#[test]
fn a_look_answer_that_called_gets_no_note() {
    let _one = super::pilot_faults::one_session();
    // The chain needs only `LAMP_SECOND_SKILL_LOOK`; the note's switch
    // changes nothing of it.
    for on in [true, false] {
        let run = search(
            on,
            &[LookAnswer::Next, LookAnswer::Other],
            LookAnswer::Other,
            None,
        );
        run.assert_clean();
        let looks = look_turns(&run);
        assert_eq!(looks.len(), 2, "note {on}: {}", run.summary());
        assert_eq!(search_turns(&run), looks, "note {on}");
        assert!(run.events("look_dropped").is_empty(), "note {on}");
        assert_eq!(run.events("look_full_still").len(), 2, "note {on}");
        assert!(look_notes(&run).is_empty(), "note {on}");
        assert!(run.events("look_note_sent").is_empty(), "note {on}");
    }
}

/// The voice's own bound on a search (`look_note::SEARCH_LOOKS_AT_MOST`,
/// lamp-skills' 8): every look's answer calls the next place, the search's
/// looks are taken in its own turns up to the 8th, and the 9th, asked in
/// the 8th look's turn, is dropped as in any turn the lamp wrote (#1146),
/// saying the search took its looks. No note follows.
#[test]
fn a_searchs_ninth_chained_look_is_dropped() {
    let _one = super::pilot_faults::one_session();
    let still = std::env::temp_dir().join(format!(
        "lamp-voice-loop-look-cap-{}.jpg",
        std::process::id()
    ));
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still.clone()));
    TEST_EYES_ANSWER.set(true);
    TEST_LOOK_NOTE.set(true);
    let run = run(Script {
        only: true,
        second: false,
        second_looks: true,
        look_answers: &[LookAnswer::Next; 8],
        turn_taking: TurnTaking::VadOnly,
        mute_at_frame: Some(3_000),
        written: Written::Said(6),
        limit: Duration::from_secs(40),
        ..Script::question(Live::Answer(12))
    });
    TEST_LOOK_NOTE.set(false);
    TEST_EYES_ANSWER.set(false);
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    let _ = std::fs::remove_file(&still);
    run.assert_clean();
    let at_most = super::look_note::SEARCH_LOOKS_AT_MOST as usize;
    let looks = look_turns(&run);
    assert_eq!(looks.len(), at_most, "{}", run.summary());
    assert_eq!(run.events("look_full_still").len(), at_most);
    // Every look's turn but the last is the search's own.
    assert_eq!(search_turns(&run), looks[..at_most - 1]);
    let dropped: Vec<_> = run
        .events("look_dropped")
        .into_iter()
        .map(|e| (e["turn"].clone(), e["why"].clone()))
        .collect();
    assert_eq!(
        dropped,
        [(
            json!(looks[at_most - 1]),
            json!("the search took its looks")
        )]
    );
    assert!(look_notes(&run).is_empty());
}

/// #1146 kept: a look's turn that is no skill's search (the route's own
/// `take_a_look`) is a turn the lamp wrote itself like any other: a skill's
/// look its answer asks for is dropped, and no second still goes.
#[test]
fn a_look_asked_in_a_turn_that_is_no_searchs_is_still_dropped() {
    let _one = super::pilot_faults::one_session();
    let still = std::env::temp_dir().join(format!(
        "lamp-voice-loop-look-own-{}.jpg",
        std::process::id()
    ));
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still.clone()));
    TEST_EYES_ANSWER.set(true);
    let run = run(Script {
        only: true,
        second: false,
        second_looks: true,
        own_look: true,
        look_answers: &[LookAnswer::Next],
        turn_taking: TurnTaking::VadOnly,
        mute_at_frame: Some(1_100),
        written: Written::Said(6),
        ..Script::question(Live::Answer(12))
    });
    TEST_EYES_ANSWER.set(false);
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    let _ = std::fs::remove_file(&still);
    run.assert_clean();
    let looks = look_turns(&run);
    assert_eq!(looks.len(), 1, "{}", run.summary());
    assert!(search_turns(&run).is_empty());
    let dropped: Vec<_> = run
        .events("look_dropped")
        .into_iter()
        .map(|e| (e["turn"].clone(), e["why"].clone()))
        .collect();
    assert_eq!(
        dropped,
        [(
            json!(looks[0]),
            json!("the turn is one the lamp wrote itself")
        )]
    );
    assert_eq!(run.events("look_full_still").len(), 1);
}

/// (d) The person speaks while the look's answer plays (the speaker has
/// taken 48 chunks of the question's answer and 12 of the look's 24): the
/// answer is cut, no note goes, and why is recorded.
#[test]
fn the_person_speaking_after_the_look_drops_the_note() {
    let _one = super::pilot_faults::one_session();
    let run = search(
        true,
        &[LookAnswer::Nothing],
        LookAnswer::Other,
        Some(When::Accepted(60)),
    );
    run.assert_clean();
    let looks = look_turns(&run);
    assert_eq!(looks.len(), 1, "{}", run.summary());
    assert!(look_notes(&run).is_empty(), "{}", run.summary());
    let dropped = run.events("look_note_dropped");
    assert_eq!(dropped.len(), 1, "{}", run.summary());
    assert_eq!(dropped[0]["turn"], looks[0]);
    assert!(
        [
            "the person is speaking",
            "another turn began",
            "its answer was not heard whole"
        ]
        .contains(&dropped[0]["why"].as_str().unwrap()),
        "{dropped:?}"
    );
}

/// A hand Stop after the look's answer has played whole ends the search
/// (the reviewer's P3 on #2266): the note owed for it is not sent once the
/// room is quiet, after the person said stop, nor recorded as dropped.
#[test]
fn a_hand_stop_after_a_looks_answer_drops_its_note() {
    let _one = super::pilot_faults::one_session();
    let run = search_with(
        true,
        &[LookAnswer::Nothing],
        LookAnswer::Other,
        None,
        Some((When::Accepted(70), 50)),
    );
    run.assert_clean();
    let looks = look_turns(&run);
    assert_eq!(looks.len(), 1, "{}", run.summary());
    // The look's answer played whole, so the note was owed; the murmur
    // near its end kept it waiting for a quiet second.
    assert_eq!(run.played(looks[0]), vec![SECOND; 24]);
    let at = |kind: &str| {
        run.trace
            .iter()
            .position(|e| {
                e["kind"] == kind
                    && (kind != "turn_finished" || e["turn"] == looks[0])
                    && (kind != "hand_control_cancel" || e["reason"] == "hand_stop")
            })
            .unwrap_or_else(|| panic!("no {kind}: {}", run.summary()))
    };
    // The Stop came by hand once the look's turn had finished.
    assert!(at("turn_finished") < at("hand_control_cancel"));
    // The quiet came after it: no note, and no turn written for one.
    assert!(look_notes(&run).is_empty(), "{}", run.summary());
    assert!(run.events("look_note_sent").is_empty());
    assert!(
        !run.trace
            .iter()
            .any(|e| e["kind"] == "input_admitted" && e["admission_basis"] == "look_note"),
        "{}",
        run.summary()
    );
    // The search ended with the Stop: nothing owed is left to drop.
    assert!(run.events("look_note_dropped").is_empty());
}

/// A skill's search on the second route alone, with the note, and the
/// body's `body/hand` on a bus of the run's own: pushed from `push` on.
fn pushed_search(push: Option<When>) -> Run {
    pushed_search_answering(push, &[LookAnswer::Nothing], None)
}

/// [`pushed_search`], its look's turns answered as `look_answers` says,
/// the first only `answer_after_push` frames after the push if given.
fn pushed_search_answering(
    push: Option<When>,
    look_answers: &'static [LookAnswer],
    answer_after_push: Option<u64>,
) -> Run {
    let root = PathBuf::from(format!("/tmp/lv-hand-loop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let bus = lamp_bus::Bus::new(&root).unwrap();
    let body = bus
        .publisher_declared(
            lamp_msgs::Hand::TOPIC,
            &lamp_msgs::Hand::declaration(),
            "body",
        )
        .unwrap();
    TEST_BODY_HAND.with(|slot| *slot.borrow_mut() = Some((false, body)));
    TEST_HAND_IN.with(|slot| {
        *slot.borrow_mut() = Some(crate::search_pushed::HandIn::on(
            lamp_bus::Bus::new(&root).unwrap(),
        ));
    });
    let still = std::env::temp_dir().join(format!(
        "lamp-voice-loop-look-pushed-{}.jpg",
        std::process::id()
    ));
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still.clone()));
    TEST_EYES_ANSWER.set(true);
    TEST_LOOK_NOTE.set(true);
    let run = run(Script {
        only: true,
        second: false,
        second_looks: true,
        look_answers,
        note_answer: LookAnswer::Other,
        push,
        answer_after_push,
        turn_taking: TurnTaking::VadOnly,
        mute_at_frame: Some(1_100),
        written: Written::Said(6),
        ..Script::question(Live::Answer(12))
    });
    TEST_LOOK_NOTE.set(false);
    TEST_EYES_ANSWER.set(false);
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    TEST_HAND_IN.with(|slot| *slot.borrow_mut() = None);
    TEST_BODY_HAND.with(|slot| *slot.borrow_mut() = None);
    let _ = std::fs::remove_file(&still);
    let _ = std::fs::remove_dir_all(&root);
    run
}

/// #2904: a push on the body while the lamp says it will look ends the
/// search for the voice, as lamp-skills ends it for the body: the held
/// look's picture never goes to the model, and no note follows. With the
/// body read and no push, the search is as before.
#[test]
fn a_push_mid_search_drops_its_looks_picture() {
    let _one = super::pilot_faults::one_session();
    let run = pushed_search(Some(When::Accepted(1)));
    run.assert_clean();
    // Eleven logical seconds allow at most 111 publications, with refreshes.
    assert!(
        (2..=111).contains(&run.seen.hand_publishes),
        "{}",
        run.summary()
    );
    assert!(look_turns(&run).is_empty(), "{}", run.summary());
    assert!(run.events("look_full_still").is_empty());
    let dropped: Vec<_> = run
        .events("look_dropped")
        .into_iter()
        .map(|e| (e["turn"].clone(), e["why"].clone()))
        .collect();
    assert_eq!(
        dropped,
        [(json!(1), json!(crate::search_pushed::WHY))],
        "{}",
        run.summary()
    );
    let pushed = run.events("search_pushed");
    assert_eq!(pushed.len(), 1, "{}", run.summary());
    assert_eq!(pushed[0]["look_dropped"], 1);
    assert!(look_notes(&run).is_empty());

    // The turn saying it will look was the search's: it is cut, as a spoken
    // stop cuts it.
    assert_eq!(pushed[0]["turn_cut"], true, "{}", run.summary());
    assert!(
        run.events("hand_control_cancel")
            .iter()
            .any(|e| e["reason"] == crate::receipt_history::BODY_PUSHED),
        "{}",
        run.summary()
    );

    let unpushed = pushed_search(None);
    unpushed.assert_clean();
    assert_eq!(unpushed.seen.hand_publishes, 0);
    assert_eq!(look_turns(&unpushed).len(), 1, "{}", unpushed.summary());
    assert_eq!(look_notes(&unpushed).len(), 1);
    assert!(unpushed.events("search_pushed").is_empty());
    assert!(unpushed.events("hand_control_cancel").is_empty());
}

/// El's P2 on #2990: a push as the look's picture has gone to the model,
/// while the head holds its pose, cuts that look's turn as a spoken stop
/// cuts it: the model's answer to the pushed look's picture is never
/// played, and its `look_for` for the next place is not taken, so no
/// second round speaks. Unpushed, the same answers look again.
#[test]
fn a_push_once_the_looks_picture_went_cuts_its_answer() {
    let _one = super::pilot_faults::one_session();
    let answers = &[LookAnswer::Next, LookAnswer::Next];
    let run = pushed_search_answering(Some(When::LookTurns(1)), answers, Some(30));
    run.assert_clean();
    // Eleven logical seconds allow at most 111 publications, with refreshes.
    assert!(
        (2..=111).contains(&run.seen.hand_publishes),
        "{}",
        run.summary()
    );
    let looks = look_turns(&run);
    assert_eq!(looks.len(), 1, "{}", run.summary());
    let look = looks[0];
    let pushed = run.events("search_pushed");
    assert_eq!(pushed.len(), 1, "{}", run.summary());
    assert_eq!(pushed[0]["turn_cut"], true, "{}", run.summary());
    assert_eq!(
        run.finished(look)["outcome"],
        crate::receipt_history::BODY_PUSHED,
        "{}",
        run.summary()
    );
    assert!(run.played(look).is_empty(), "{}", run.summary());
    // Nothing after it: no look asked, no turn written, no audio.
    assert!(
        !run.events("look_asked")
            .iter()
            .any(|e| e["turn"].as_u64() >= Some(look)),
        "{}",
        run.summary()
    );
    assert!(
        run.seen.chunks.iter().all(|c| c.turn <= look),
        "{}",
        run.summary()
    );
    assert!(look_notes(&run).is_empty());

    let unpushed = pushed_search_answering(None, answers, None);
    unpushed.assert_clean();
    assert_eq!(unpushed.seen.hand_publishes, 0);
    assert!(look_turns(&unpushed).len() >= 2, "{}", unpushed.summary());
}

/// (e) Without `LAMP_SECOND_LOOK_NOTE`, a look left with no call is as
/// before: no note, nothing recorded of one.
#[test]
fn without_the_switch_a_look_left_gets_no_note() {
    let _one = super::pilot_faults::one_session();
    let run = search(false, &[LookAnswer::Nothing], LookAnswer::Other, None);
    run.assert_clean();
    assert_eq!(look_turns(&run).len(), 1, "{}", run.summary());
    assert!(look_notes(&run).is_empty());
    assert!(run.events("look_note_sent").is_empty());
    assert!(run.events("look_note_dropped").is_empty());
}

/// look-for-things' give-up line (`[search] give_up`), as lamp-skills
/// refuses a look past its five with it.
const GIVE_UP: &str = "That was the last look. Say kindly, in a few words, that you can't see it from here, and do not look again.";
/// The skill's note for each look's picture.
const PICTURE: &str = "say in a few words what you see.";

/// lamp-skills' look-for-things as the walk calls it: each call one look,
/// the search's first (the model's) counted, and past `[search] looks = 5`
/// its give-up line as an error. The places the walk asked, in order.
fn fake_skill() -> (super::look_note::SkillCaller, Arc<Mutex<Vec<String>>>) {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let places = asked.clone();
    let skill: super::look_note::SkillCaller = Arc::new(move |call, _| {
        assert_eq!(call.name, "look_for");
        let mut places = places.lock().unwrap();
        places.push(call.args["place"].as_str().unwrap().to_owned());
        if places.len() + 1 > 5 {
            (json!({"error": GIVE_UP}), true)
        } else {
            (
                json!({"next": format!("Say only \"Let me take a look.\" now. When the picture comes: {PICTURE}"),
                    "look": true}),
                true,
            )
        }
    });
    (skill, asked)
}

/// A skill's search on the second route alone with the note and, `walk`
/// on or off, the walk (`LAMP_SECOND_LOOK_WALK`), its skill `fake_skill`:
/// the run, and the places the walk asked the skill for.
fn walk(
    walk: bool,
    look_answers: &'static [LookAnswer],
    first_place: Option<&'static str>,
    speak_again: Option<When>,
) -> (Run, Vec<String>) {
    let still = std::env::temp_dir().join(format!(
        "lamp-voice-loop-look-walk-{}.jpg",
        std::process::id()
    ));
    let (skill, asked) = fake_skill();
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = Some(still.clone()));
    TEST_EYES_ANSWER.set(true);
    TEST_LOOK_NOTE.set(true);
    TEST_LOOK_WALK.set(walk);
    TEST_WALK_SKILL.with(|walk| *walk.borrow_mut() = Some(skill));
    let run = run(Script {
        only: true,
        second: false,
        second_looks: true,
        look_answers,
        note_answer: LookAnswer::Nothing,
        first_place,
        walk,
        speak_again,
        turn_taking: TurnTaking::VadOnly,
        mute_at_frame: Some(4_000),
        written: Written::Said(6),
        limit: Duration::from_secs(50),
        ..Script::question(Live::Answer(12))
    });
    TEST_WALK_SKILL.with(|walk| *walk.borrow_mut() = None);
    TEST_LOOK_WALK.set(false);
    TEST_LOOK_NOTE.set(false);
    TEST_EYES_ANSWER.set(false);
    TEST_FULL_LOOK.with(|look| *look.borrow_mut() = None);
    let _ = std::fs::remove_file(&still);
    let asked = asked.lock().unwrap().clone();
    (run, asked)
}

/// The places the walk recorded asking for, in order.
fn walked(run: &Run) -> Vec<String> {
    run.events("look_walk_asked")
        .into_iter()
        .map(|e| e["place"].as_str().unwrap().to_owned())
        .collect()
}

/// The give-up lines the second route was sent.
fn give_ups(run: &Run) -> usize {
    run.seen
        .second
        .iter()
        .filter(|input| matches!(input, ProviderInput::Written { text, .. } if text == GIVE_UP))
        .count()
}

/// (a) The first builder's step 4 for look-for-things (20 Oct): the model
/// never calls again. The desk's look, the note, then the lamp walks on by
/// itself, left, right, far_left, far_right, each look's picture sent as
/// the search's next look turn naming its place; the skill refuses the
/// sixth with its give-up line, which goes once. Five looks in all.
#[test]
fn a_search_the_model_never_calls_again_walks_five_looks_and_gives_up_once() {
    let _one = super::pilot_faults::one_session();
    let (run, asked) = walk(true, &[LookAnswer::Nothing; 5], None, None);
    run.assert_clean();
    let looks = look_turns(&run);
    assert_eq!(looks.len(), 5, "{}", run.summary());
    assert_eq!(look_notes(&run).len(), 1);
    let places = ["left", "right", "far_left", "far_right", "far_right"];
    assert_eq!(asked, places, "{}", run.summary());
    assert_eq!(walked(&run), places);
    // Each walk's look turn names its place and carries the skill's note,
    // and is the search's own.
    let texts: Vec<&String> = run
        .seen
        .second
        .iter()
        .filter_map(|input| match input {
            ProviderInput::Written {
                text,
                look: true,
                search: true,
                ..
            } => Some(text),
            _ => None,
        })
        .collect();
    // The first is the desk's, the model's own.
    assert_eq!(texts.len(), 5, "{}", run.summary());
    assert!(!texts[0].contains("You looked"));
    for (text, toward) in texts[1..].iter().zip([
        "to the left",
        "to the right",
        "far to the left",
        "far to the right",
    ]) {
        assert!(
            text.contains(&format!("(You looked {toward}; the picture is that look.)")),
            "{text}"
        );
        assert!(text.contains(PICTURE), "{text}");
    }
    assert_eq!(run.events("look_full_still").len(), 5);
    assert_eq!(give_ups(&run), 1, "{}", run.summary());
    assert_eq!(run.events("look_walk_gave_up").len(), 1);
    assert!(
        run.events("look_walk_dropped").is_empty(),
        "{:?}",
        run.events("look_walk_dropped")
    );
    assert!(run.events("look_dropped").is_empty());
}

/// (b) The model says found (`look_for(found)`, a call) in the answer to
/// the walk's left look: the search is over, nothing more is walked.
#[test]
fn a_search_found_after_the_walks_first_look_walks_no_further() {
    let _one = super::pilot_faults::one_session();
    let (run, asked) = walk(true, &[LookAnswer::Nothing, LookAnswer::Other], None, None);
    run.assert_clean();
    assert_eq!(look_turns(&run).len(), 2, "{}", run.summary());
    assert_eq!(asked, ["left"]);
    assert_eq!(walked(&run), ["left"]);
    assert_eq!(give_ups(&run), 0);
    assert!(run.events("look_walk_gave_up").is_empty());
}

/// (c) The person speaks as the walk's left look's answer plays: the walk
/// stops there, for that reason, and asks the skill for nothing more. The
/// speech is keyed to the left look's turn reaching the second route (its
/// second look turn), not to a chunk count: keyed to the note's last chunk,
/// it raced the walk's skill call on its own thread, and a slow host heard
/// the person before the left look was taken (CI ARM64, PR #2466).
#[test]
fn the_person_speaking_stops_the_walk() {
    let _one = super::pilot_faults::one_session();
    let (run, asked) = walk(
        true,
        &[LookAnswer::Nothing; 5],
        None,
        Some(When::LookTurns(2)),
    );
    run.assert_clean();
    assert_eq!(asked, ["left"], "{}", run.summary());
    assert_eq!(look_turns(&run).len(), 2, "{}", run.summary());
    let dropped: Vec<_> = run
        .events("look_walk_dropped")
        .into_iter()
        .map(|e| e["why"].clone())
        .collect();
    assert_eq!(
        dropped,
        [json!("the person is speaking")],
        "{}",
        run.summary()
    );
    assert_eq!(give_ups(&run), 0);
}

/// (d) Without `LAMP_SECOND_LOOK_WALK`: the note alone, as in step 2. The
/// skill is never called by the lamp.
#[test]
fn without_the_switch_a_search_gets_only_its_note() {
    let _one = super::pilot_faults::one_session();
    let (run, asked) = walk(false, &[LookAnswer::Nothing; 5], None, None);
    run.assert_clean();
    assert_eq!(look_turns(&run).len(), 1, "{}", run.summary());
    assert_eq!(look_notes(&run).len(), 1);
    assert!(asked.is_empty());
    assert!(walked(&run).is_empty());
    assert!(run.events("look_walk_dropped").is_empty());
    assert_eq!(give_ups(&run), 0);
}

/// (e) Places already looked at are skipped: a search the person began at
/// the left walks on right, far_left, far_right, then the desk it has not
/// looked at, and the skill's give-up line ends it after five looks.
#[test]
fn the_walk_skips_the_place_the_person_named() {
    let _one = super::pilot_faults::one_session();
    let (run, asked) = walk(true, &[LookAnswer::Nothing; 5], Some("left"), None);
    run.assert_clean();
    assert_eq!(asked[..3], ["right", "far_left", "far_right"]);
    assert_eq!(asked, ["right", "far_left", "far_right", "desk", "desk"]);
    assert!(!asked.iter().any(|place| place == "left"));
    assert_eq!(look_turns(&run).len(), 5, "{}", run.summary());
    assert_eq!(give_ups(&run), 1);
}

#[test]
fn without_a_second_route_the_packets_and_records_are_as_before() {
    let _one = super::pilot_faults::one_session();
    let plain = run(Script {
        second: false,
        ..Script::question(Live::Answer(12))
    });
    plain.assert_clean();
    assert!(plain.events("voice_route_switched").is_empty());
    assert!(
        plain
            .trace
            .iter()
            .all(|e| e.get("standby").is_none() && e.get("route").is_none())
    );
    let standing_by = run(Script::question(Live::Answer(12)));
    standing_by.assert_clean();
    assert!(standing_by.events("voice_route_switched").is_empty());
    assert!(standing_by.seen.second.is_empty());
    assert_eq!(timeless(&plain.seen.live), timeless(&standing_by.seen.live));
    assert_eq!(shape(&plain), shape(&standing_by));
    assert_eq!(plain.played(1), standing_by.played(1));
    assert!(plain.played(1).iter().all(|&v| v == LIVE));
}

/// (b) A spent quota before any audio: the second route is given the whole
/// line and answers it, and the rest of the lamp sees the turn it would
/// have seen had Gemini answered: the same authority at the speaker, the
/// same ring phases, the same record of the finished turn.
#[test]
fn a_spent_quota_moves_the_line_to_the_second_route_and_nothing_else_changes() {
    let _one = super::pilot_faults::one_session();
    let live = run(Script::question(Live::Answer(12)));
    live.assert_clean();
    let switched = run(Script::question(Live::Fail(Fault::QuotaExceeded)));
    switched.assert_clean();
    let blocks = whole_line(&switched.seen.live, 1);
    assert_eq!(whole_line(&switched.seen.second, 1), blocks);
    // Over, and back to Gemini at rest if the run lasts long enough: its
    // link never went. On the ARM64 runner the run once ended before the
    // idle tick that goes back; going back has its own test below.
    let switch = switched.events("voice_route_switched");
    assert!(matches!(switch.len(), 1 | 2), "{switch:?}");
    assert_eq!(switch[0]["why"], "live_turn_failed");
    if let Some(back) = switch.get(1) {
        assert_eq!(back["why"], "live_link_ready");
    }
    assert_eq!(switch[0]["reason"], json!(Fault::QuotaExceeded));
    assert_eq!(switch[0]["replayed_commands"], blocks + 2);
    assert_eq!(switched.played(1), vec![SECOND; 48]);
    assert!(switched.events("provider_turn_failed").is_empty());
    for run in [&live, &switched] {
        assert_eq!(run.finished(1)["outcome"], "audio_written_unscored");
    }
    let keys = |run: &Run| {
        let mut keys: Vec<_> = run
            .finished(1)
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    };
    assert_eq!(keys(&live), keys(&switched));
    assert_eq!(dedup(&live.seen.installs), dedup(&switched.seen.installs));
    assert_eq!(dedup(&live.seen.phases), dedup(&switched.seen.phases));
    assert!(dedup(&switched.seen.phases).contains(&RingPhase::Speaking));
}

/// (c) The link drops while the person is still speaking: the line is not
/// abandoned but replayed to the second route, which hears all of it.
#[test]
fn a_link_dropped_mid_line_loses_nothing_of_the_question() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script::question(Live::DropLink));
    run.assert_clean();
    let sent_live = audio_count(&run.seen.live, 1);
    assert!(sent_live > 0, "the drop came before any audio");
    assert!(
        !run.seen
            .live
            .iter()
            .any(|i| matches!(i, ProviderInput::End { .. }))
    );
    let heard = whole_line(&run.seen.second, 1);
    assert!(heard > sent_live, "{heard} <= {sent_live}");
    assert!(run.events("provider_input_abandoned").is_empty());
    assert_eq!(
        run.events("voice_route_switched")[0]["why"],
        "live_link_changed"
    );
    assert_eq!(run.finished(1)["outcome"], "audio_written_unscored");
    assert_eq!(run.played(1), vec![SECOND; 48]);
}

/// (d) Gemini's answer that comes after its failure is never heard.
#[test]
fn gemini_audio_after_its_failure_is_dropped() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script::question(Live::FailThenLate(Fault::RateLimited, 6)));
    run.assert_clean();
    assert_eq!(run.played(1), vec![SECOND; 48]);
    assert!(run.seen.chunks.iter().all(|c| c.value != LIVE));
}

/// (e) Once Gemini's link is Ready again, the next turn at rest is its.
#[test]
fn the_next_turn_goes_back_to_gemini_once_it_is_ready() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        speak_again: Some(When::Settled(1)),
        turns: 2,
        live_back_after: Some(1),
        ..Script::question(Live::DropLink)
    });
    run.assert_clean();
    let whys: Vec<_> = run
        .events("voice_route_switched")
        .iter()
        .map(|e| e["why"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(whys, ["live_link_changed", "live_link_ready"]);
    assert_eq!(run.played(1), vec![SECOND; 48]);
    let second_turn = run.seen.live.iter().rev().find_map(|i| match i {
        ProviderInput::Start {
            request, session, ..
        } => Some((*request, *session)),
        _ => None,
    });
    let (turn, session) = second_turn.unwrap();
    assert!(turn > 1);
    assert_eq!(session, 2);
    whole_line(&run.seen.live, turn);
    assert!(!run.seen.second.iter().any(|i| request_of(i) == turn));
    assert_eq!(run.played(turn), vec![LIVE; 48]);
}

/// (g) Spoken over while the second route answers, the answer stops as
/// one of Gemini's does.
#[test]
fn spoken_over_the_second_routes_answer_stops_as_gemini_does() {
    let _one = super::pilot_faults::one_session();
    let script = |live| Script {
        second_packets: 60,
        speak_again: Some(When::Accepted(8)),
        turns: 2,
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(live)
    };
    let live = run(script(Live::Answer(60)));
    live.assert_clean();
    let switched = run(script(Live::Fail(Fault::ServerRejected)));
    switched.assert_clean();
    for run in [&live, &switched] {
        assert_eq!(run.finished(1)["outcome"], "user_interrupted");
        assert!(run.played(1).len() < 240, "{}", run.played(1).len());
        // The speaker took an authority without turn 1 once it was over.
        let installs = dedup(&run.seen.installs);
        let owned = installs.iter().position(|i| i.0 == Some(1)).unwrap();
        assert!(installs[owned..].iter().any(|i| i.0 != Some(1)));
    }
    assert!(switched.played(1).iter().all(|&v| v == SECOND));
    assert_eq!(dedup(&live.seen.installs), dedup(&switched.seen.installs));
}

/// The reviewer's P2 on #2427: a line spoken over the lamp's answer is read
/// only from what was heard once the lamp fell silent, never the speech
/// heard while its answer still played.
#[test]
fn a_lines_tone_takes_no_speech_heard_while_the_lamp_answers() {
    let _one = super::pilot_faults::one_session();
    TEST_VOICE_TONE.with(|on| on.set(true));
    let run = run(Script {
        only: true,
        second: false,
        voiced: true,
        second_packets: 60,
        speak_again: Some(When::Accepted(8)),
        turns: 2,
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(Live::Answer(60))
    });
    TEST_VOICE_TONE.with(|on| on.set(false));
    run.assert_clean();
    assert_eq!(run.finished(1)["outcome"], "user_interrupted");
    let read: Vec<(u64, u64)> = run
        .events("local_endpoint")
        .into_iter()
        .filter_map(|end| {
            Some((
                end["turn"].as_u64()?,
                end["tone"]["tone"]["blocks"].as_u64()?,
            ))
        })
        .collect();
    // The first line is all its 60 blocks. The second's 60 began while the
    // answer played: here 53 are read, and the 7 heard before it stopped
    // are not; taken while it played, 59 would be (and before the fix,
    // all 60 since the first line's end).
    assert_eq!(read.first(), Some(&(1, 60)), "{}", run.summary());
    let (turn, blocks) = read.get(1).copied().unwrap();
    assert!(turn == 2 && blocks <= 55, "{read:?}");
}

/// Each line's tone is told once its words come, under its own turn and
/// with their valence; the first line says nothing of its arousal, and the
/// second is read against it.
#[test]
fn each_lines_tone_is_told_with_its_own_words() {
    let _one = super::pilot_faults::one_session();
    TEST_VOICE_TONE.with(|on| on.set(true));
    let run = run(Script {
        only: true,
        second: false,
        voiced: true,
        second_hears: Some("I'm so happy today"),
        speak_again: Some(When::Settled(1)),
        turns: 2,
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(Live::Answer(12))
    });
    TEST_VOICE_TONE.with(|on| on.set(false));
    run.assert_clean();
    let told: Vec<_> = run
        .events("voice_tone")
        .into_iter()
        .map(|told| {
            (
                told["turn"].clone(),
                told["words"].clone(),
                told["valence"].clone(),
            )
        })
        .collect();
    assert_eq!(
        told,
        [
            (json!(1), json!(true), json!(1.0)),
            (json!(2), json!(true), json!(1.0))
        ],
        "{}",
        run.summary()
    );
    // No known face here: the room's baseline, the second line its second.
    let whose: Vec<_> = run
        .events("local_endpoint")
        .into_iter()
        .map(|end| {
            (
                end["tone"]["whose"].clone(),
                end["tone"]["baseline_lines"].clone(),
            )
        })
        .collect();
    assert_eq!(
        whose,
        [(json!("room"), json!(1)), (json!("room"), json!(2))]
    );
    let tones = run.events("voice_tone");
    assert_eq!(tones[0]["sure"], 0.0);
    assert!(tones[1]["sure"].as_f64().unwrap() > 0.0, "{}", tones[1]);
    assert!(
        tones[1]["arousal"].as_f64().unwrap().abs() < 0.1,
        "{}",
        tones[1]
    );
}

/// A line that goes on from the one before, begun just after its End with
/// the lamp not yet answering, is read over both and told once, under its
/// own turn, with both lines' words (the first builder on #2602, 21 Oct
/// 21:49): the line before is not told, and the baseline takes it in once.
#[test]
fn a_line_that_goes_on_from_the_one_before_is_read_and_told_once() {
    let _one = super::pilot_faults::one_session();
    TEST_VOICE_TONE.with(|on| on.set(true));
    let run = run(Script {
        only: true,
        second: false,
        voiced: true,
        second_hears: Some("I'm so happy today"),
        // The first line's answer is held, so the second begins with the
        // lamp silent.
        second_waits_first: true,
        // Each line's words as it begins, so the second's come too.
        hears_at_start: true,
        speak_again: Some(When::Ended(1)),
        turns: 2,
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(Live::Answer(12))
    });
    TEST_VOICE_TONE.with(|on| on.set(false));
    run.assert_clean();
    let ends: Vec<_> = run
        .events("local_endpoint")
        .into_iter()
        .map(|end| {
            (
                end["turn"].clone(),
                end["tone"]["tone"]["blocks"].clone(),
                end["tone"]["baseline_lines"].clone(),
                end["tone"]["went_on_from"].clone(),
            )
        })
        .collect();
    assert_eq!(ends.len(), 2, "{}", run.summary());
    assert_eq!(ends[0].2, json!(1));
    assert_eq!(ends[0].3, Value::Null);
    // The second is read over both lines' blocks, and taken in as the
    // baseline's first line, not its second.
    assert_eq!(ends[1].0, json!(2));
    assert!(
        ends[1].1.as_u64().unwrap() >= ends[0].1.as_u64().unwrap() + 30,
        "{ends:?}"
    );
    assert_eq!(ends[1].2, json!(1), "{ends:?}");
    assert_eq!(ends[1].3, json!(1), "{ends:?}");
    let told: Vec<_> = run
        .events("voice_tone")
        .into_iter()
        .map(|told| {
            (
                told["turn"].clone(),
                told["words"].clone(),
                told["valence"].clone(),
            )
        })
        .collect();
    assert_eq!(
        told,
        [(json!(2), json!(true), json!(1.0))],
        "{}",
        run.summary()
    );
}

/// Words of no lamp line, or an older one's, never tell the reading waiting
/// (the reviewer-lead's P3 on #2670): its own words are then taken as not
/// coming, and it is told only as it falls due, never while a line goes on
/// from it. A later line's words are kept for that line, and leave it
/// waiting for its own.
#[test]
fn words_of_no_line_or_an_older_one_leave_the_reading_waiting() {
    let waiting = || ToneHeld {
        reading: Some(TonePending {
            turns: vec![3],
            reading: crate::voice_tone::Reading {
                arousal: -0.5,
                sure: 1.0,
            },
            ended_us: 1_000_000,
            words: None,
            worded: false,
            before: None,
        }),
        later: None,
    };
    // A line that goes on from the one before: the meter has kept it, and
    // the next line's first speech came within the window.
    let mut going_on = crate::voice_tone::ToneMeter::default();
    let block = vec![1_000i16; 160];
    going_on.speech_block(0, &block, true);
    let _ = going_on.finish(10_000);
    going_on.speech_block(500_000, &block, true);
    assert!(going_on.going_on());
    let past = 1_000_000 + crate::voice_tone::GOES_ON_US;
    for heard_in in [None, Some(2)] {
        let mut held = waiting();
        held.heard("someone else", heard_in);
        let pending = held.reading.as_ref().expect("still waiting");
        assert!(pending.worded && pending.words.is_none(), "{heard_in:?}");
        // Not while a line goes on from it, even past the window; then as
        // it falls due, with no line kept to go on.
        assert!(!pending.due(past, Some(&going_on)), "{heard_in:?}");
        assert!(pending.due(past, None), "{heard_in:?}");
    }
    // A later line's words: kept for it, and the reading still waits for
    // its own.
    let mut held = waiting();
    held.heard("the next one", Some(4));
    let pending = held.reading.as_ref().unwrap();
    assert!(!pending.worded && !pending.due(past, None));
    assert_eq!(held.later, Some((4, "the next one".to_owned())));
    // Its own words: kept, and it falls due.
    held.heard("its own", Some(3));
    let pending = held.reading.as_ref().unwrap();
    assert_eq!(pending.words.as_deref(), Some("its own"));
    assert!(pending.due(past, None));
}

/// A known person's lines are read against their own baseline, and their
/// words asking the lamp to forget their face let it go: the line after
/// starts their baseline again.
#[test]
fn a_known_persons_baseline_goes_when_they_ask_to_forget_their_face() {
    let _one = super::pilot_faults::one_session();
    TEST_VOICE_TONE.with(|on| on.set(true));
    TEST_TONE_PERSON.with(|person| *person.borrow_mut() = Some("p-jim".into()));
    let run = run(Script {
        only: true,
        second: false,
        voiced: true,
        second_hears: Some("Please forget my face"),
        speak_again: Some(When::Settled(1)),
        turns: 2,
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(Live::Answer(12))
    });
    TEST_TONE_PERSON.with(|person| *person.borrow_mut() = None);
    TEST_VOICE_TONE.with(|on| on.set(false));
    run.assert_clean();
    let whose: Vec<_> = run
        .events("local_endpoint")
        .into_iter()
        .map(|end| {
            (
                end["tone"]["whose"].clone(),
                end["tone"]["baseline_lines"].clone(),
            )
        })
        .collect();
    assert_eq!(
        whose,
        [(json!("person"), json!(1)), (json!("person"), json!(1))],
        "{}",
        run.summary()
    );
    let forgot: Vec<_> = run
        .events("voice_tone_forgot")
        .into_iter()
        .map(|forgot| (forgot["all"].clone(), forgot["whose"].clone()))
        .collect();
    assert_eq!(
        forgot,
        [
            (json!(false), json!("person")),
            (json!(false), json!("person"))
        ],
        "{}",
        run.summary()
    );
}

/// Thirteen on #1722 (21 Oct): the duplex route hears a line in pieces and
/// marks none finished, so a line's words are read whole once its pieces
/// stop. Asked in pieces to forget their face, a known person's baseline
/// goes, as when the line comes whole.
#[test]
fn a_forget_heard_in_pieces_lets_the_baseline_go() {
    let _one = super::pilot_faults::one_session();
    TEST_VOICE_TONE.with(|on| on.set(true));
    TEST_TONE_PERSON.with(|person| *person.borrow_mut() = Some("p-jim".into()));
    let run = run(Script {
        only: true,
        second: false,
        voiced: true,
        second_hears: Some("Please forget my face"),
        hears_in_pieces: true,
        speak_again: Some(When::Settled(1)),
        turns: 2,
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(Live::Answer(12))
    });
    TEST_TONE_PERSON.with(|person| *person.borrow_mut() = None);
    TEST_VOICE_TONE.with(|on| on.set(false));
    run.assert_clean();
    let lines: Vec<_> = run
        .events("local_endpoint")
        .into_iter()
        .map(|end| end["tone"]["baseline_lines"].clone())
        .collect();
    assert_eq!(lines, [json!(1), json!(1)], "{}", run.summary());
    // Once for each line, never twice (the reviewer-lead's P3 on #2600).
    let forgot: Vec<_> = run
        .events("voice_tone_forgot")
        .into_iter()
        .map(|forgot| (forgot["by"].clone(), forgot["whose"].clone()))
        .collect();
    assert_eq!(
        forgot,
        [
            (json!("words"), json!("person")),
            (json!("words"), json!("person"))
        ],
        "{}",
        run.summary()
    );
}

/// The reviewer-lead's P2 on #2600: a line begun while the one before's
/// pieces are still coming never joins it. Each line's pieces are read as
/// its own, and the first's are read as the second begins, not at its End.
#[test]
fn two_lines_in_a_row_each_keep_their_own_pieces() {
    let _one = super::pilot_faults::one_session();
    TEST_VOICE_TONE.with(|on| on.set(true));
    let run = run(Script {
        only: true,
        second: false,
        voiced: true,
        second_hears: Some("I'm so happy today"),
        hears_in_pieces: true,
        speak_again: Some(When::Ended(1)),
        turns: 2,
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(Live::Answer(12))
    });
    TEST_VOICE_TONE.with(|on| on.set(false));
    run.assert_clean();
    let told: Vec<_> = run
        .events("voice_tone")
        .into_iter()
        .map(|told| (told["turn"].clone(), told["words"].clone()))
        .collect();
    assert_eq!(
        told,
        [(json!(1), json!(true)), (json!(2), json!(true))],
        "{}",
        run.summary()
    );
}

/// A line keeps at most `TONE_LINE_BYTES` of its words: a piece that would
/// pass them is left out (the reviewer-lead's P3 on #2600).
#[test]
fn a_tone_line_keeps_at_most_its_bytes() {
    let mut line = ToneLine::default();
    let piece = "a".repeat(1_000);
    for at in 0..6 {
        assert!(line.heard(&piece, Some(1), at).is_none());
    }
    assert!(line.heard("tail", Some(1), 6).is_none());
    let (words, turn) = line.take();
    assert_eq!(words.len(), 4_004, "four pieces and the tail");
    assert!(words.len() <= TONE_LINE_BYTES);
    assert_eq!(turn, Some(1));
}

/// A piece heard in another turn's line gives the line before back, whole
/// and with its own turn; one of the same turn joins it.
#[test]
fn a_piece_of_another_turns_line_gives_the_line_before_back() {
    let mut line = ToneLine::default();
    assert!(line.heard("Please forget ", Some(1), 0).is_none());
    assert!(line.heard("my face", Some(1), 1).is_none());
    assert_eq!(
        line.heard("Hello", Some(2), 2),
        Some(("Please forget my face".to_owned(), Some(1)))
    );
    // Room talk with no line of the lamp's is a line of its own too.
    assert_eq!(
        line.heard("there", None, 3),
        Some(("Hello".to_owned(), Some(2)))
    );
    assert_eq!(line.take(), ("there".to_owned(), None));
    assert_eq!(line.take(), (String::new(), None), "taken once");
}

/// The reviewer-lead's P3 on #2608: words heard with no lamp line yet, just
/// before one begins, are that line's first, whether the loop's tick or the
/// line's own next piece sees it begin; heard a second or more before, they
/// are a line of their own.
#[test]
fn words_heard_just_before_a_lamp_line_begins_are_its_first() {
    let mut line = ToneLine::default();
    assert!(line.heard("forget my ", None, 0).is_none());
    assert!(!line.over(200_000, Some(1), false), "now line 1's");
    assert!(line.heard("face", Some(1), 300_000).is_none());
    assert_eq!(line.take(), ("forget my face".to_owned(), Some(1)));
    assert!(line.heard("forget my ", None, 0).is_none());
    assert!(line.heard("face", Some(1), 300_000).is_none());
    assert_eq!(line.take(), ("forget my face".to_owned(), Some(1)));
    assert!(line.heard("hello", None, 0).is_none());
    assert!(line.over(TONE_LINE_QUIET_US, Some(1), false));
    assert_eq!(
        line.heard("there", Some(1), TONE_LINE_QUIET_US),
        Some(("hello".to_owned(), None))
    );
}

/// A line is over when the turn it was heard in is no longer the lamp's
/// line, at once, or once no piece has come for `TONE_LINE_QUIET_US` after
/// that line's End; while its line is still being said, never.
#[test]
fn a_tone_line_is_over_at_another_turn_or_a_quiet_after_its_end() {
    let mut line = ToneLine::default();
    assert!(!line.over(0, Some(1), true), "nothing heard");
    let _ = line.heard("forget my", Some(1), 1_000);
    let quiet = 1_000 + TONE_LINE_QUIET_US;
    assert!(!line.over(10 * quiet, Some(1), false), "still being said");
    assert!(!line.over(quiet - 1, Some(1), true));
    assert!(line.over(quiet, Some(1), true));
    assert!(line.over(1_001, Some(2), false), "another line began");
    assert!(line.over(1_001, None, true), "its line let go");
}

/// Pieces heard while the line is still being said are its words: they are
/// kept until its End, and its tone is told with them, never the line
/// before's.
#[test]
fn pieces_heard_before_a_lines_end_are_its_words() {
    let _one = super::pilot_faults::one_session();
    TEST_VOICE_TONE.with(|on| on.set(true));
    let run = run(Script {
        only: true,
        second: false,
        voiced: true,
        second_hears: Some("I'm so happy today"),
        hears_in_pieces: true,
        hears_at_start: true,
        speak_again: Some(When::Settled(1)),
        turns: 2,
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(Live::Answer(12))
    });
    TEST_VOICE_TONE.with(|on| on.set(false));
    run.assert_clean();
    let told: Vec<_> = run
        .events("voice_tone")
        .into_iter()
        .map(|told| {
            (
                told["turn"].clone(),
                told["words"].clone(),
                told["valence"].clone(),
            )
        })
        .collect();
    assert_eq!(
        told,
        [
            (json!(1), json!(true), json!(1.0)),
            (json!(2), json!(true), json!(1.0))
        ],
        "{}",
        run.summary()
    );
}

/// The first builder at 10:5x on #2467: memory has forgotten a known person
/// (`memory/forgot`, from the app or by voice), so their tone baseline goes.
/// Enrolled again under the same name, their next line starts a fresh one.
/// A rules scope memory forgot (`r-`) is no voice, and changes nothing.
#[test]
fn a_name_memory_forgot_starts_a_fresh_tone_baseline_when_enrolled_again() {
    let _one = super::pilot_faults::one_session();
    let person = lamp_msgs::known::person_id("Jim");
    let rules = format!("r-{}", &person[2..]);
    let lines = |forgot: Option<String>| {
        TEST_VOICE_TONE.with(|on| on.set(true));
        TEST_TONE_PERSON.with(|known| *known.borrow_mut() = Some(person.clone()));
        TEST_TONE_FORGOT.with(|told| *told.borrow_mut() = VecDeque::from([None, forgot]));
        let run = run(Script {
            only: true,
            second: false,
            voiced: true,
            second_hears: Some("What a nice day"),
            speak_again: Some(When::Settled(1)),
            turns: 2,
            turn_taking: TurnTaking::VadOnly,
            ..Script::question(Live::Answer(12))
        });
        TEST_TONE_FORGOT.with(|told| told.borrow_mut().clear());
        TEST_TONE_PERSON.with(|known| *known.borrow_mut() = None);
        TEST_VOICE_TONE.with(|on| on.set(false));
        run.assert_clean();
        let lines: Vec<_> = run
            .events("local_endpoint")
            .into_iter()
            .map(|end| end["tone"]["baseline_lines"].clone())
            .collect();
        let forgot: Vec<_> = run
            .events("voice_tone_forgot")
            .into_iter()
            .map(|forgot| (forgot["by"].clone(), forgot["whose"].clone()))
            .collect();
        (lines, forgot, run.summary())
    };
    let (kept, none, summary) = lines(None);
    assert_eq!(kept, [json!(1), json!(2)], "{summary}");
    assert!(none.is_empty(), "{summary}");
    let (fresh, forgot, summary) = lines(Some(person.clone()));
    assert_eq!(fresh, [json!(1), json!(1)], "{summary}");
    assert_eq!(forgot, [(json!("memory"), json!("person"))], "{summary}");
    let (kept, none, summary) = lines(Some(rules));
    assert_eq!(kept, [json!(1), json!(2)], "{summary}");
    assert!(none.is_empty(), "{summary}");
}

/// The reviewer's P3 2 on #2488: the voice's loop reads memory's forgets
/// from the bus itself (`ForgotIn` beside the hello's watch, not a test
/// seam at the End): memory tells it forgot a known person after their first
/// line, and their second line starts a fresh baseline.
#[test]
fn the_loop_reads_memorys_forget_from_the_bus_and_the_next_line_starts_fresh() {
    let _one = super::pilot_faults::one_session();
    let person = lamp_msgs::known::person_id("Jim");
    let root = PathBuf::from(format!("/tmp/lv-forgot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let bus = lamp_bus::Bus::new(&root).unwrap();
    TEST_FORGOT_IN.with(|slot| {
        *slot.borrow_mut() = Some(crate::bus_out::ForgotIn::on(&bus).unwrap());
    });
    let memory = bus
        .publisher_declared(
            lamp_msgs::MemoryForgot::TOPIC,
            &lamp_msgs::MemoryForgot::declaration(),
            "memory",
        )
        .unwrap();
    TEST_MEMORY_FORGETS.with(|slot| *slot.borrow_mut() = Some((1, person.clone(), memory)));
    TEST_VOICE_TONE.with(|on| on.set(true));
    TEST_TONE_PERSON.with(|known| *known.borrow_mut() = Some(person.clone()));
    let run = run(Script {
        only: true,
        second: false,
        voiced: true,
        second_hears: Some("What a nice day"),
        speak_again: Some(When::Settled(1)),
        turns: 2,
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(Live::Answer(12))
    });
    TEST_TONE_PERSON.with(|known| *known.borrow_mut() = None);
    TEST_VOICE_TONE.with(|on| on.set(false));
    let told = TEST_MEMORY_FORGETS
        .with(|slot| slot.borrow_mut().take())
        .is_none();
    TEST_FORGOT_IN.with(|slot| *slot.borrow_mut() = None);
    let _ = std::fs::remove_dir_all(&root);
    run.assert_clean();
    assert!(told, "memory's forget was never told");
    let lines: Vec<_> = run
        .events("local_endpoint")
        .into_iter()
        .map(|end| end["tone"]["baseline_lines"].clone())
        .collect();
    assert_eq!(lines, [json!(1), json!(1)], "{}", run.summary());
    let forgot: Vec<_> = run
        .events("voice_tone_forgot")
        .into_iter()
        .map(|forgot| (forgot["by"].clone(), forgot["whose"].clone()))
        .collect();
    assert_eq!(
        forgot,
        [(json!("memory"), json!("person"))],
        "{}",
        run.summary()
    );
}

/// (h) Gemini is Ready 1 s after the voice starts and the question began
/// before that: it is held, then Gemini's, and the second route hears none
/// of it (lampOS#779).
#[test]
fn a_question_before_geminis_first_ready_waits_for_it() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        live_late: Some(Some(100)),
        ..Script::question(Live::Answer(12))
    });
    run.assert_clean();
    assert!(run.events("voice_route_switched").is_empty());
    let held = run.events("voice_route_held");
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0]["to"], "live");
    assert!(held[0]["held_ms"].as_u64().unwrap() < 3_000);
    assert!(run.seen.second.is_empty());
    whole_line(&run.seen.live, 1);
    assert_eq!(run.played(1), vec![LIVE; 48]);
}

/// (i) Gemini is never Ready: after the hold the second route is given the
/// whole line and answers it.
#[test]
fn a_question_goes_to_the_second_route_once_the_startup_hold_is_over() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        live_late: Some(None),
        ..Script::question(Live::Answer(12))
    });
    run.assert_clean();
    let held = run.events("voice_route_held");
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0]["to"], "second");
    assert!(
        held[0]["held_ms"].as_u64().unwrap() >= crate::coordinator::second::STARTUP_HOLD_US / 1_000
    );
    let switch = run.events("voice_route_switched");
    assert_eq!(switch.len(), 1, "{switch:?}");
    assert_eq!(switch[0]["why"], "live_link_not_ready");
    assert_eq!(switch[0]["turn"], 1);
    let blocks = whole_line(&run.seen.second, 1);
    assert_eq!(switch[0]["replayed_commands"], blocks + 2);
    assert!(run.seen.live.is_empty());
    assert_eq!(run.played(1), vec![SECOND; 48]);
}

/// lampOS#892: Gemini's worker is told to wait for room in its input queue
/// only when a second route is configured, right after `--keep-link`.
#[test]
fn the_live_worker_waits_for_room_only_with_a_second_route() {
    let options = DirectedOptions::parse(&[]).unwrap();
    let config = std::path::Path::new("/private/provider.json");
    let without = provider_arguments(
        ProviderSource::Gemini(config),
        &options,
        Lifetime::Serve,
        None,
        false,
    )
    .unwrap();
    assert!(
        !without
            .iter()
            .any(|flag| flag == crate::provider_worker::WAIT_FOR_ROOM_FLAG)
    );
    let with = provider_arguments(
        ProviderSource::Gemini(config),
        &options,
        Lifetime::Serve,
        None,
        true,
    )
    .unwrap();
    let keep = with
        .iter()
        .position(|flag| flag == crate::provider_worker::KEEP_LINK_FLAG)
        .unwrap();
    assert_eq!(with[keep + 1], crate::provider_worker::WAIT_FOR_ROOM_FLAG);
    let mut stripped = with.clone();
    stripped.remove(keep + 1);
    assert_eq!(stripped, without);
}

/// (j) #960: the second route's worker exits while it holds the turn. The
/// route is dropped and that turn ends, and the voice goes on: the next
/// turn is Gemini's, and the run ends by its scripted mute, not by the
/// worker's exit.
#[test]
fn a_second_worker_that_exits_is_dropped_and_the_voice_goes_on() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        second_dies: Some(1),
        speak_again: Some(When::Settled(1)),
        turns: 2,
        ..Script::question(Live::Fail(Fault::QuotaExceeded))
    });
    run.assert_clean();
    assert_eq!(
        run.events("voice_route_switched")[0]["why"],
        "live_turn_failed"
    );
    let dropped = run.events("voice_route_dropped");
    assert_eq!(dropped.len(), 1, "{}", run.summary());
    assert_eq!(dropped[0]["active"], true);
    assert_eq!(run.finished(1)["outcome"], "second_route_failed");
    assert!(run.played(1).is_empty());
    let turn = run
        .seen
        .live
        .iter()
        .rev()
        .find_map(|i| match i {
            ProviderInput::Start { request, .. } => Some(*request),
            _ => None,
        })
        .unwrap();
    assert!(turn > 1);
    whole_line(&run.seen.live, turn);
    assert_eq!(run.played(turn), vec![LIVE; 48]);
}

/// (k) #960: the standby's worker exits while Gemini is well: it is
/// dropped, and Gemini's answer is untouched.
#[test]
fn a_standby_that_exits_is_dropped_and_gemini_answers() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        standby_dies_at: Some(5),
        ..Script::question(Live::Answer(12))
    });
    run.assert_clean();
    let dropped = run.events("voice_route_dropped");
    assert_eq!(dropped.len(), 1, "{}", run.summary());
    assert_eq!(dropped[0]["active"], false);
    assert!(run.events("voice_route_switched").is_empty());
    whole_line(&run.seen.live, 1);
    assert_eq!(run.played(1), vec![LIVE; 48]);
}

/// (l) #960, the reviewer's P2: the active second worker exits mid-line
/// while the question is still being said, with input queued for it that
/// it never took. That input goes with the turn, and the voice goes on.
#[test]
fn a_second_worker_that_exits_mid_line_leaves_no_queued_input_behind() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        second_dies: Some(40),
        mute_after_second_dies: true,
        ..Script::question(Live::DropLink)
    });
    run.assert_clean();
    let dropped = run.events("voice_route_dropped");
    assert_eq!(dropped.len(), 1, "{}", run.summary());
    assert_eq!(dropped[0]["active"], true);
    assert_eq!(run.finished(1)["outcome"], "second_route_failed");
    assert!(run.played(1).is_empty());
}

/// (m) #960, lamp-4ace 09:03-09:06: on the second route in a busy room the
/// lamp is never between turns. Gemini's link is Ready again while the
/// second route still answers, and someone speaks over it: that new turn
/// is Gemini's, not the second route's.
#[test]
fn a_turn_begun_while_the_second_route_answers_goes_to_gemini_once_ready() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        second_packets: 60,
        speak_again: Some(When::Accepted(8)),
        turns: 2,
        live_back_at_frame: Some(85),
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(Live::DropLink)
    });
    run.assert_clean();
    let switches = run.events("voice_route_switched");
    let whys: Vec<_> = switches
        .iter()
        .map(|e| e["why"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        whys,
        ["live_link_changed", "live_link_ready"],
        "{switches:?}"
    );
    let (turn, session) = run
        .seen
        .live
        .iter()
        .rev()
        .find_map(|i| match i {
            ProviderInput::Start {
                request, session, ..
            } => Some((*request, *session)),
            _ => None,
        })
        .unwrap();
    assert!(turn > 1);
    assert_eq!(session, 2);
    assert_eq!(switches[1]["turn"], turn);
    whole_line(&run.seen.live, turn);
    assert!(!run.seen.second.iter().any(|i| request_of(i) == turn));
    assert!(run.played(1).iter().all(|&v| v == SECOND));
    assert_eq!(run.played(turn), vec![LIVE; 48]);
}

/// (n) The reviewer's P2 on #962: Gemini's worker, waiting while the second
/// route is active, exits with no report. It is still a mandatory worker:
/// the voice ends on it (and is restarted), rather than staying on the
/// second route for good.
#[test]
fn geminis_worker_exiting_while_the_second_route_is_active_still_ends_the_voice() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        live_dies_at: Some(60),
        ..Script::question(Live::DropLink)
    });
    let error = run.result.as_ref().unwrap_err().to_string();
    assert!(
        error.contains("provider worker exited"),
        "{}",
        run.summary()
    );
}

/// `LAMP_VOICE_ROUTE=second` (the owner, 19 Oct: Gemini Live against the
/// cheaper second route, an A/B on lamp-4ace): the second route is the only
/// voice from the first turn. No Gemini worker is started and nothing is
/// sent to one; the second route is sent what Gemini would have been sent,
/// answers, and the rest of the lamp sees the turn as Gemini's: the same
/// authorities at the speaker, the same ring phases, the same kinds and
/// fields of every record, and no switch.
#[test]
fn the_second_route_alone_answers_a_spoken_request_and_no_gemini_worker_starts() {
    let _one = super::pilot_faults::one_session();
    let gemini = run(Script {
        second: false,
        ..Script::question(Live::Answer(12))
    });
    gemini.assert_clean();
    let alone = run(Script {
        second: false,
        only: true,
        ..Script::question(Live::Answer(12))
    });
    alone.assert_clean();
    assert!(alone.seen.live.is_empty(), "{:?}", alone.seen.live);
    assert!(
        alone.exits.iter().all(|(role, _)| *role != "provider"),
        "{:?}",
        alone.exits
    );
    assert!(alone.exits.contains(&("second", true)), "{:?}", alone.exits);
    whole_line(&alone.seen.second, 1);
    assert_eq!(timeless(&gemini.seen.live), timeless(&alone.seen.second));
    assert_eq!(alone.played(1), vec![SECOND; 48]);
    assert_eq!(gemini.played(1), vec![LIVE; 48]);
    for kind in [
        "voice_route_switched",
        "voice_route_held",
        "voice_route_dropped",
    ] {
        assert!(alone.events(kind).is_empty(), "{kind}: {}", alone.summary());
    }
    assert_eq!(shape(&gemini), shape(&alone));
    assert_eq!(gemini.finished(1)["outcome"], alone.finished(1)["outcome"]);
    assert_eq!(dedup(&gemini.seen.installs), dedup(&alone.seen.installs));
    assert_eq!(dedup(&gemini.seen.phases), dedup(&alone.seen.phases));
    assert!(dedup(&alone.seen.phases).contains(&RingPhase::Speaking));
}

/// The first builder, 19 Oct: on the second route the first sound had been
/// its wait cue, not its answer. The session's records say when each turn's
/// answer began to be heard (`answer_first_audio`): on the second route the
/// first audio after its words, a cue before them being no answer; on the
/// Live route its first audio.
#[test]
fn the_answers_first_audio_is_recorded_after_a_wait_cue_never_at_it() {
    let _one = super::pilot_faults::one_session();
    let order = |run: &Run| -> Vec<&'static str> {
        run.trace
            .iter()
            .filter_map(|e| match e["kind"].as_str() {
                Some("provider_first_audio") if e["turn"] == 1 => Some("provider_first_audio"),
                Some("answer_first_audio") if e["turn"] == 1 => Some("answer_first_audio"),
                Some("transcript") if e["turn"] == 1 => Some("words"),
                _ => None,
            })
            .collect()
    };
    let alone = run(Script {
        second: false,
        only: true,
        second_cue: 4,
        second_says: Some("Sure."),
        ..Script::question(Live::Answer(12))
    });
    alone.assert_clean();
    assert_eq!(
        order(&alone),
        ["provider_first_audio", "words", "answer_first_audio"],
        "{}",
        alone.summary()
    );
    assert_eq!(alone.events("answer_first_audio").len(), 1);
    let gemini = run(Script {
        second: false,
        ..Script::question(Live::Answer(12))
    });
    gemini.assert_clean();
    let at = |run: &Run, kind: &str| run.events(kind)[0]["at_us"].as_u64().unwrap();
    assert_eq!(gemini.events("answer_first_audio").len(), 1);
    assert!(at(&gemini, "answer_first_audio") >= at(&gemini, "provider_first_audio"));
    assert_eq!(
        order(&gemini),
        ["provider_first_audio", "answer_first_audio"]
    );
}

/// Alone, the second route's worker is the lamp's only voice: its exit
/// ends the voice (and the service is restarted), as Gemini's does, where a
/// standby's is dropped and the voice goes on with Gemini.
#[test]
fn the_second_route_alone_exiting_ends_the_voice_as_gemini_does() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        second: false,
        only: true,
        second_dies: Some(1),
        ..Script::question(Live::Answer(12))
    });
    let error = run.result.as_ref().unwrap_err().to_string();
    assert!(error.contains("second worker exited"), "{}", run.summary());
    assert!(run.events("voice_route_dropped").is_empty());
    assert!(run.seen.live.is_empty());
}

/// The first builder's 20:15 on #1722: a task's news told unasked on the
/// second route alone is said once, at the next quiet moment, as on Live.
/// After the person's question is answered, the news waiting is begun as
/// the lamp's own turn at the first pause (`LAMP_TASK_NEWS_AT_PAUSE=on`),
/// sent to the voice that answers, said whole, and only then handed on as
/// heard; it is never sent again.
#[test]
fn news_is_told_once_at_the_next_quiet_moment_on_the_second_route_as_on_live() {
    let _one = super::pilot_faults::one_session();
    const NOTE: &str = "Claude Code just finished the story you asked for.";
    let told = |only: bool| {
        let (news, heard) = crate::task_news::TaskNews::waiting_for_test_heard(
            crate::task_news::News {
                key: "s1:2:4".into(),
                note: NOTE.into(),
                until: None,
                progress: None,
                finished: None,
                also: Vec::new(),
            },
            Duration::from_secs(30),
        );
        TEST_TASK_NEWS.with(|given| *given.borrow_mut() = Some(news));
        TEST_NEWS_AT_PAUSE.with(|at_pause| at_pause.set(true));
        let run = run(Script {
            second: false,
            only,
            written: Written::Said(12),
            mute_at_frame: Some(800),
            ..Script::question(Live::Answer(12))
        });
        TEST_NEWS_AT_PAUSE.with(|at_pause| at_pause.set(false));
        TEST_TASK_NEWS.with(|given| given.borrow_mut().take());
        (run, heard.try_iter().collect::<Vec<_>>())
    };
    for (only, voice) in [(false, LIVE), (true, SECOND)] {
        let (run, heard) = told(only);
        run.assert_clean();
        let sent = if only {
            &run.seen.second
        } else {
            &run.seen.live
        };
        let notes: Vec<u64> = sent
            .iter()
            .filter_map(|input| match input {
                ProviderInput::Written { request, text, .. } if text == NOTE => Some(*request),
                _ => None,
            })
            .collect();
        assert_eq!(notes, [2], "only {only}: {}", run.summary());
        assert_eq!(run.events("task_news").len(), 1, "only {only}");
        assert_eq!(run.events("task_news")[0]["key"], "s1:2:4");
        // At the first pause after the answer, not after 15 s with no turn.
        let since = run.events("task_news")[0]["since_turn_ms"]
            .as_u64()
            .unwrap();
        assert!(
            (crate::task_news::PAUSE_US / 1_000..crate::task_news::QUIET_US / 1_000)
                .contains(&since),
            "only {only}: {since} ms"
        );
        assert_eq!(run.played(1), vec![voice; 48], "only {only}");
        assert_eq!(run.played(2), vec![voice; 48], "only {only}");
        // Heard in the turn that told it, the news's.
        assert_eq!(
            heard,
            [("s1:2:4".to_owned(), 2)],
            "only {only}: {}",
            run.summary()
        );
        if only {
            assert!(run.seen.live.is_empty(), "{:?}", run.seen.live);
        }
    }
}

/// #2602, finding 4 (lamp-75c9, 21 October): a sign-in's news waited for
/// a quiet room like a task's, though the person had just asked for it and
/// its question goes unanswered in two minutes. Without
/// `LAMP_TASK_NEWS_AT_PAUSE`, it is now told at the first pause after the
/// answer, while a task's news still waits its 15 s.
#[test]
fn a_sign_ins_news_is_told_at_the_first_pause_without_the_switch() {
    let _one = super::pilot_faults::one_session();
    let told = |key: &str| {
        let (news, _heard) = crate::task_news::TaskNews::waiting_for_test_heard(
            crate::task_news::News {
                key: key.into(),
                note: "Harness asks whether to sign this lamp in.".into(),
                until: None,
                progress: None,
                finished: None,
                also: Vec::new(),
            },
            Duration::from_secs(30),
        );
        TEST_TASK_NEWS.with(|given| *given.borrow_mut() = Some(news));
        let run = run(Script {
            second: false,
            only: true,
            written: Written::Said(12),
            mute_at_frame: Some(800),
            ..Script::question(Live::Answer(12))
        });
        TEST_TASK_NEWS.with(|given| given.borrow_mut().take());
        run
    };
    let run = told("sign-in-1-asking");
    run.assert_clean();
    let news = run.events("task_news");
    assert_eq!(news.len(), 1, "{}", run.summary());
    assert_eq!(news[0]["key"], "sign-in-1-asking");
    assert_eq!(news[0]["at_pause"], true);
    let since = news[0]["since_turn_ms"].as_u64().unwrap();
    assert!(
        (crate::task_news::PAUSE_US / 1_000..crate::task_news::QUIET_US / 1_000).contains(&since),
        "{since} ms"
    );
    // A task's news, the same run: not before its 15 s.
    let run = told("s1:2:4");
    run.assert_clean();
    assert!(
        run.events("task_news")
            .iter()
            .all(|news| news["since_turn_ms"]
                .as_u64()
                .is_some_and(|since| since >= crate::task_news::QUIET_US / 1_000)),
        "{}",
        run.summary()
    );
}

/// The reviewer-lead's P2 on #2642: with `LAMP_TASK_NEWS_AT_PAUSE=on`, as
/// on product lamps, a sign-in's news waiting behind a task's is told
/// first, at the first pause after the answer.
#[test]
fn with_the_switch_on_a_sign_ins_news_goes_ahead_of_a_tasks() {
    let _one = super::pilot_faults::one_session();
    let waiting = |key: &str, note: &str| crate::task_news::News {
        key: key.into(),
        note: note.into(),
        until: None,
        progress: None,
        finished: None,
        also: Vec::new(),
    };
    let (news, _heard) = crate::task_news::TaskNews::all_waiting_for_test(
        vec![
            waiting(
                "s1:2:4",
                "Claude Code just finished the story you asked for.",
            ),
            waiting(
                "sign-in-1-asking",
                "Harness asks whether to sign this lamp in.",
            ),
        ],
        Duration::from_secs(30),
    );
    TEST_TASK_NEWS.with(|given| *given.borrow_mut() = Some(news));
    TEST_NEWS_AT_PAUSE.with(|at_pause| at_pause.set(true));
    let run = run(Script {
        second: false,
        only: true,
        written: Written::Said(12),
        mute_at_frame: Some(800),
        ..Script::question(Live::Answer(12))
    });
    TEST_NEWS_AT_PAUSE.with(|at_pause| at_pause.set(false));
    TEST_TASK_NEWS.with(|given| given.borrow_mut().take());
    run.assert_clean();
    let news = run.events("task_news");
    assert!(!news.is_empty(), "{}", run.summary());
    assert_eq!(news[0]["key"], "sign-in-1-asking", "{}", run.summary());
    let since = news[0]["since_turn_ms"].as_u64().unwrap();
    assert!(
        (crate::task_news::PAUSE_US / 1_000..crate::task_news::QUIET_US / 1_000).contains(&since),
        "{since} ms"
    );
}

/// lamp-52e6 on 21 October: with the person seated quietly, the lamp's fan
/// opened 11 wordless requests in two minutes, each held the quiet the
/// lamp's own turns wait for (`last_turn_us`) until it ended, and the
/// onboarding walk's 3 s never came. Now only talk holds it: the detector's
/// own speech, words or none (a line in a language the route takes no
/// words from), the person's words heard, and the lamp speaking. A request
/// that heard no words and was not answered aloud holds it through its
/// speech and no longer: not through its end's silence or the wait for its
/// answer. Read from the news told at the first pause after it.
#[test]
fn speech_holds_the_quiet_with_or_without_words_and_a_wordless_requests_tail_does_not() {
    let _one = super::pilot_faults::one_session();
    let told = |hears: &'static [&'static str]| {
        let (news, _heard) = crate::task_news::TaskNews::waiting_for_test_heard(
            crate::task_news::News {
                key: "s1:2:4".into(),
                note: "Claude Code just finished the story you asked for.".into(),
                until: None,
                progress: None,
                finished: None,
                also: Vec::new(),
            },
            Duration::from_secs(30),
        );
        TEST_TASK_NEWS.with(|given| *given.borrow_mut() = Some(news));
        TEST_NEWS_AT_PAUSE.with(|at_pause| at_pause.set(true));
        let run = run(Script {
            second: false,
            live_hears: hears,
            written: Written::Said(12),
            mute_at_frame: Some(800),
            ..Script::question(Live::Answer(0))
        });
        TEST_NEWS_AT_PAUSE.with(|at_pause| at_pause.set(false));
        TEST_TASK_NEWS.with(|given| given.borrow_mut().take());
        run
    };
    for (hears, words) in [(&[][..], false), (&["what time is it"][..], true)] {
        let run = told(hears);
        run.assert_clean();
        assert_eq!(run.finished(1)["outcome"], "no_audio_answer", "{words}");
        let news = run.events("task_news");
        assert_eq!(news.len(), 1, "{words}: {}", run.summary());
        let quiet_from =
            news[0]["at_us"].as_u64().unwrap() - news[0]["since_turn_ms"].as_u64().unwrap() * 1_000;
        let started = run
            .events("provider_input_started")
            .into_iter()
            .find(|event| event["turn"] == 1)
            .unwrap()["at_us"]
            .as_u64()
            .unwrap();
        // Speech with no transcript holds it as words do.
        assert!(quiet_from >= started, "{words}: {quiet_from} < {started}");
        if !words {
            // From its last speech, not its end's silence (0.6 s) or after.
            let finished = run.finished(1)["at_us"].as_u64().unwrap();
            assert!(
                quiet_from + 300_000 < finished,
                "{quiet_from} not before {finished}"
            );
        }
        assert!(
            news[0]["since_turn_ms"].as_u64().unwrap() >= crate::task_news::PAUSE_US / 1_000,
            "{words}"
        );
    }
}

/// `LAMP_SAY` (#939) on the second route alone, as on Live: a skill's say
/// is begun as the lamp's own turn once the person's answer is over and the
/// room is quiet, sent to the voice that answers, said whole, and its asker
/// told `begun` and then `spoken`, once.
#[test]
fn a_skills_say_is_said_and_answered_spoken_on_the_second_route_as_on_live() {
    use crate::say::{Answer, Outcome};
    let _one = super::pilot_faults::one_session();
    const NOTE: &str = "Remind the person to stretch.";
    let said = |only: bool| {
        let (says, answered) =
            crate::say::Says::note_for_test("stretch", NOTE, crate::say::URGENT_FROM);
        TEST_SAYS.with(|given| *given.borrow_mut() = Some(says));
        let run = run(Script {
            second: false,
            only,
            written: Written::Said(12),
            mute_at_frame: Some(800),
            ..Script::question(Live::Answer(12))
        });
        TEST_SAYS.with(|given| given.borrow_mut().take());
        (run, answered.try_iter().collect::<Vec<_>>())
    };
    for (only, voice) in [(false, LIVE), (true, SECOND)] {
        let (run, answered) = said(only);
        run.assert_clean();
        let sent = if only {
            &run.seen.second
        } else {
            &run.seen.live
        };
        let notes: Vec<u64> = sent
            .iter()
            .filter_map(|input| match input {
                ProviderInput::Written { request, text, .. } if text == NOTE => Some(*request),
                _ => None,
            })
            .collect();
        assert_eq!(notes.len(), 1, "only {only}: {}", run.summary());
        let turn = notes[0];
        assert_eq!(run.played(turn), vec![voice; 48], "only {only}");
        assert!(
            matches!(
                answered.as_slice(),
                [Answer::Begun(_), Answer::Done(Outcome::Spoken)]
            ),
            "only {only}: {answered:?}"
        );
        if only {
            assert!(run.seen.live.is_empty(), "{:?}", run.seen.live);
        }
    }
}

/// `LAMP_OWN_LINE_OVER_TALK` (#2602's design): a skill's say spoken over by
/// room talk, with no name and no face turned to the lamp, in a room busy as
/// it began, is let go as the room's (`busy_room`) and said whole: `spoken`.
/// Without the switch, or in a room that isn't busy, the talk cuts it as
/// before and it is `dropped`.
#[test]
fn talk_over_a_say_in_a_busy_room_is_let_go_with_the_switch() {
    use crate::say::{Answer, Outcome};
    let _one = super::pilot_faults::one_session();
    let said = |on: bool, busy: bool| {
        let (says, answered) = crate::say::Says::note_for_test(
            "stretch",
            "Remind the person to stretch.",
            crate::say::URGENT_FROM,
        );
        TEST_SAYS.with(|given| *given.borrow_mut() = Some(says));
        TEST_OWN_LINE_OVER_TALK.with(|over| over.set(on));
        TEST_ROOM_BUSY.with(|room| room.set(busy));
        let run = run(Script {
            second: false,
            only: true,
            // The person's answer is 48 chunks, the say 120: the room's
            // talk begins 30 chunks into the say.
            written: Written::Said(30),
            speak_again: Some(When::Accepted(78)),
            turn_taking: TurnTaking::VadOnly,
            mute_at_frame: Some(1_200),
            limit: Duration::from_secs(20),
            ..Script::question(Live::Answer(12))
        });
        TEST_ROOM_BUSY.with(|room| room.set(false));
        TEST_OWN_LINE_OVER_TALK.with(|over| over.set(false));
        TEST_SAYS.with(|given| given.borrow_mut().take());
        (run, answered.try_iter().collect::<Vec<_>>())
    };
    for (on, busy) in [(false, true), (true, false), (true, true)] {
        let (run, answered) = said(on, busy);
        run.assert_clean();
        let let_go = run
            .events("input_let_go_own_line")
            .into_iter()
            .filter(|event| event["why"] == "busy_room")
            .count();
        if on && busy {
            assert!(let_go > 0, "{}", run.summary());
            assert!(
                matches!(
                    answered.as_slice(),
                    [Answer::Begun(_), Answer::Done(Outcome::Spoken)]
                ),
                "{answered:?}: {}",
                run.summary()
            );
        } else {
            assert_eq!(let_go, 0, "on {on}, busy {busy}");
            assert!(
                matches!(
                    answered.as_slice(),
                    [Answer::Begun(_), Answer::Done(Outcome::Dropped(_))]
                ),
                "on {on}, busy {busy}: {answered:?}: {}",
                run.summary()
            );
        }
    }
}

/// #3051's review: the same say under the duplex policy, through the loop.
/// The mask plays on through the talk while it waits for the name
/// (`ForName`): no yield. Talk with no name is let go only once
/// `OWN_LINE_NAME_LAG_US` after its trigger is past, never taken, and the
/// say is `spoken`.
#[test]
fn talk_over_a_say_in_a_busy_room_waits_for_the_name_then_is_let_go_under_duplex() {
    use crate::say::{Answer, Outcome};
    let _one = super::pilot_faults::one_session();
    let (says, answered) = crate::say::Says::note_for_test(
        "stretch",
        "Remind the person to stretch.",
        crate::say::URGENT_FROM,
    );
    TEST_SAYS.with(|given| *given.borrow_mut() = Some(says));
    TEST_OWN_LINE_OVER_TALK.with(|over| over.set(true));
    TEST_ROOM_BUSY.with(|room| room.set(true));
    let run = run(Script {
        second: false,
        only: true,
        written: Written::Said(30),
        speak_again: Some(When::Accepted(78)),
        turn_taking: TurnTaking::Duplex,
        echo_evidence: true,
        mute_at_frame: Some(1_200),
        limit: Duration::from_secs(20),
        ..Script::question(Live::Answer(12))
    });
    TEST_ROOM_BUSY.with(|room| room.set(false));
    TEST_OWN_LINE_OVER_TALK.with(|over| over.set(false));
    TEST_SAYS.with(|given| given.borrow_mut().take());
    let answered: Vec<_> = answered.try_iter().collect();
    run.assert_clean();
    assert!(run.events("duplex_yield").is_empty(), "{}", run.summary());
    let triggers: std::collections::HashMap<String, u64> = run
        .events("input_candidate")
        .into_iter()
        .map(|event| {
            (
                event["candidate"]["id"].to_string(),
                event["candidate"]["trigger_read_at_us"].as_u64().unwrap(),
            )
        })
        .collect();
    let let_go: Vec<_> = run
        .events("input_let_go_own_line")
        .into_iter()
        .filter(|event| event["why"] == "busy_room")
        .collect();
    assert!(!let_go.is_empty(), "{}", run.summary());
    for event in &let_go {
        let trigger = triggers[&event["candidate"].to_string()];
        assert!(
            event["at_us"].as_u64().unwrap() >= trigger + OWN_LINE_NAME_LAG_US,
            "let go before the name could be said: {event}"
        );
    }
    assert!(
        matches!(
            answered.as_slice(),
            [Answer::Begun(_), Answer::Done(Outcome::Spoken)]
        ),
        "{answered:?}: {}",
        run.summary()
    );
}

/// The holding answers on the second route (the first builder's 22:54 on
/// #1722), as on Live: with `LAMP_HOLDING_ANSWER=on`, an answer that only
/// asked the person to wait is followed, once the room is quiet, by one
/// written turn of `holding_answer::NOTE` to the same voice, which says it.
/// Without the switch, nothing follows.
#[test]
fn a_holding_answer_is_asked_to_go_on_once_on_the_second_route_as_on_live() {
    let _one = super::pilot_faults::one_session();
    let with = |on: bool, only: bool| {
        TEST_HOLDING_ANSWER.with(|holding| holding.set(on));
        let run = run(Script {
            second: false,
            only,
            written: Written::Said(12),
            mute_at_frame: Some(900),
            ..Script::question(Live::Holding(12))
        });
        TEST_HOLDING_ANSWER.with(|holding| holding.set(false));
        run
    };
    for (only, voice) in [(false, LIVE), (true, SECOND)] {
        let run = with(true, only);
        run.assert_clean();
        let sent = if only {
            &run.seen.second
        } else {
            &run.seen.live
        };
        let notes: Vec<u64> = sent
            .iter()
            .filter_map(|input| match input {
                ProviderInput::Written { request, text, .. }
                    if text == crate::holding_answer::NOTE =>
                {
                    Some(*request)
                }
                _ => None,
            })
            .collect();
        assert_eq!(notes.len(), 1, "only {only}: {}", run.summary());
        assert_eq!(run.events("holding_answer_nudged").len(), 1, "only {only}");
        assert_eq!(run.played(1), vec![voice; 48], "only {only}");
        assert_eq!(run.played(notes[0]), vec![voice; 48], "only {only}");
        assert_eq!(run.seen.live.is_empty(), only, "only {only}");

        let off = with(false, only);
        off.assert_clean();
        let sent = if only {
            &off.seen.second
        } else {
            &off.seen.live
        };
        assert!(
            !sent
                .iter()
                .any(|input| matches!(input, ProviderInput::Written { .. })),
            "only {only}: nothing follows without the switch"
        );
        assert!(off.events("holding_answer_nudged").is_empty());
    }
}

/// Supply a real Say protocol line to the production loop at a quiet moment.
/// The sender stands in for Skills; no Skills process or provider service runs.
fn second_only_skill_line(
    line: &str,
    at_frame: u64,
    limit: Duration,
) -> (Run, Vec<crate::say::Answer>) {
    let (says, line, answered) = crate::say::Says::line_for_test(line);
    TEST_SAYS.with(|given| *given.borrow_mut() = Some(says));
    let run = run(Script {
        second: false,
        only: true,
        silent: true,
        say_at_frame: Some((at_frame, line)),
        written: Written::Said(12),
        mute_at_frame: Some(at_frame + 200),
        limit,
        ..Script::question(Live::Answer(12))
    });
    TEST_SAYS.with(|given| given.borrow_mut().take());
    (run, answered.try_iter().collect())
}

/// #3697 (lamp-52e6, 27 Oct): the shutdown's line asked while a person is
/// still speaking, the microphone going on: its cached sound (3.5 s at
/// 24 kHz), and the plain text it falls back to. The line cuts their turn and
/// begins; the rest of what they say is drained, never queued for a turn that
/// will not send it, so the voice goes on (it had ended on "microphone to
/// provider queue exceeded bound").
#[test]
fn the_shutdowns_last_words_over_a_person_speaking_play_and_the_voice_goes_on() {
    use crate::say::{Answer, Words};
    const LINE: &str = "Going to sleep now.";
    let _one = super::pilot_faults::one_session();
    for cached in [true, false] {
        let words = if cached {
            Words::Sound(vec![500; 84_000])
        } else {
            Words::Text(LINE.into())
        };
        let (says, line, answered) =
            crate::say::Says::skill_line_for_test(crate::say::SHUTDOWN_SKILL, words);
        TEST_SAYS.with(|given| *given.borrow_mut() = Some(says));
        let run = run(Script {
            second: false,
            only: true,
            speech_frames: 500,
            say_at_frame: Some((100, line)),
            written: Written::Said(12),
            mute_at_frame: Some(900),
            limit: Duration::from_secs(20),
            ..Script::question(Live::Answer(12))
        });
        TEST_SAYS.with(|given| given.borrow_mut().take());
        let answered: Vec<_> = answered.try_iter().collect();
        run.assert_clean();
        assert!(
            matches!(answered.first(), Some(Answer::Begun(_))),
            "cached {cached}: {answered:?}"
        );
        if cached {
            assert!(
                run.events("skill_sound")
                    .iter()
                    .any(|event| event["last_words"] == true && event["plays"] == true),
                "{}",
                run.summary()
            );
            assert!(
                run.seen
                    .chunks
                    .iter()
                    .any(|chunk| chunk.accepted && chunk.value == 500),
                "the last words were not played: {}",
                run.summary()
            );
        } else {
            assert!(
                run.seen.second.iter().any(|input| matches!(
                    input,
                    ProviderInput::Written { text, .. } if text.contains(LINE)
                )),
                "{}",
                run.summary()
            );
        }
    }
}

fn assert_only_second_worker(run: &Run) {
    run.assert_clean();
    assert!(run.seen.live.is_empty(), "{:?}", run.seen.live);
    assert!(run.exits.iter().all(|(role, _)| *role != "provider"));
    assert!(run.exits.contains(&("second", true)));
    assert!(run.events("voice_route_switched").is_empty());
}

/// A custom scheduled action may request exact words. SecondOnly receives
/// the same AS_WRITTEN instruction as Live, and completes the Say ticket only
/// after the guarded speaker has accepted and retired the answer.
#[test]
fn a_scheduled_exact_say_uses_only_the_second_route_and_finishes_spoken() {
    use crate::say::{AS_WRITTEN, Answer, Outcome};
    let _one = super::pilot_faults::one_session();
    const WORDS: &str = "Good morning. The day can start slowly.";
    let (run, answered) = second_only_skill_line(
        &json!({
            "skill":"custom-morning", "text":WORDS, "priority":8,
            "expires_in_ms":60_000,
        })
        .to_string(),
        200,
        LIMIT,
    );
    assert_only_second_worker(&run);
    let [ProviderInput::Written { request, text, .. }] = run.seen.second.as_slice() else {
        panic!(
            "one exact written turn, no captured input: {:?}",
            run.seen.second
        );
    };
    assert_eq!(text, &format!("{AS_WRITTEN} {WORDS}"));
    assert_eq!(run.played(*request), vec![SECOND; 48]);
    assert!(
        run.seen
            .chunks
            .iter()
            .all(|chunk| chunk.turn == *request && chunk.accepted)
    );
    assert!(
        run.seen
            .installs
            .iter()
            .any(|i| i.0 == Some(*request) && i.1)
    );
    assert!(
        matches!(
            answered.as_slice(),
            [Answer::Begun(_), Answer::Done(Outcome::Spoken)]
        ),
        "{answered:?}"
    );
    assert_eq!(run.events("skill_says").len(), 1);
    assert_eq!(run.events("skill_says")[0]["skill"], "custom-morning");
    assert!(run.events("skill_sound").is_empty());
}

/// The authored Bedtime morning action is cue="hum", not a spoken line.
/// After the unchanged 15-second quiet gate, its parsed fixed PCM reaches
/// the guarded speaker without any text, STT input, or chat request.
#[test]
fn the_bedtime_morning_hum_plays_on_second_only_without_provider_input() {
    use crate::say::{Answer, Outcome};
    let _one = super::pilot_faults::one_session();
    let (run, answered) = second_only_skill_line(
        r#"{"skill":"bedtime-morning","cue":"hum","expires_in_ms":60000}"#,
        1_600,
        Duration::from_secs(20),
    );
    assert_only_second_worker(&run);
    assert!(run.seen.second.is_empty(), "{:?}", run.seen.second);
    let turn = run.seen.chunks.first().expect("hum reached speaker").turn;
    assert!(
        run.seen
            .chunks
            .iter()
            .all(|chunk| chunk.turn == turn && chunk.accepted)
    );
    let played: Vec<i16> = run
        .seen
        .chunks
        .iter()
        .flat_map(|chunk| chunk.samples.iter().copied())
        .collect();
    assert_eq!(played, crate::head_cue::hum(crate::head_cue::DBFS));
    assert!(run.seen.installs.iter().any(|i| i.0 == Some(turn) && i.1));
    assert!(
        matches!(
            answered.as_slice(),
            [Answer::Begun(_), Answer::Done(Outcome::Spoken)]
        ),
        "{answered:?}"
    );
    let sounds = run.events("skill_sound");
    assert_eq!(sounds.len(), 1);
    assert_eq!(sounds[0]["skill"], "bedtime-morning");
    assert_eq!(sounds[0]["plays"], true);
    assert!(run.events("skill_says").is_empty());
}

/// Missing the original start deadline never turns either payload into a
/// late morning output. A still-live cue also keeps the normal quiet gate.
#[test]
fn an_expired_scheduled_say_or_an_early_hum_sends_nothing_on_second_only() {
    use crate::say::{Answer, Outcome};
    let _one = super::pilot_faults::one_session();
    for payload in [json!({"text":"Good morning."}), json!({"cue":"hum"})] {
        let mut line = json!({
            "skill":"bedtime-morning", "expires_in_ms":60_000,
            "start_before_us":0,
        });
        line.as_object_mut()
            .unwrap()
            .extend(payload.as_object().unwrap().clone());
        let (run, answered) = second_only_skill_line(&line.to_string(), 200, LIMIT);
        assert_only_second_worker(&run);
        assert_eq!(answered, [Answer::Done(Outcome::Expired)]);
        assert!(run.seen.second.is_empty(), "{:?}", run.seen.second);
        assert!(run.seen.chunks.is_empty());
        assert!(run.events("skill_says").is_empty());
        assert!(run.events("skill_sound").is_empty());
    }
    let (run, answered) = second_only_skill_line(
        r#"{"skill":"bedtime-morning","cue":"hum","expires_in_ms":60000}"#,
        200,
        LIMIT,
    );
    assert_only_second_worker(&run);
    assert_eq!(
        answered,
        [Answer::Done(Outcome::Refused(
            "the lamp spoke a moment ago".into()
        ))]
    );
    assert!(run.seen.second.is_empty());
    assert!(run.seen.chunks.is_empty());
    assert!(run.events("skill_says").is_empty());
    assert_eq!(run.events("skill_sound").len(), 1);
    assert_eq!(run.events("skill_sound")[0]["plays"], false);
}

/// (p) #960, lamp-4ace (`server_go_away`): Gemini's link drops with no turn
/// in hand and is Ready again 500 ms later; the question asked meanwhile
/// waits for it, not for the second route.
#[test]
fn a_question_during_a_reconnect_at_rest_waits_for_gemini() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        link_drops_at: Some(10),
        live_back_at_frame: Some(60),
        ..Script::question(Live::Reconnect(12))
    });
    run.assert_clean();
    assert!(
        run.events("voice_route_switched").is_empty(),
        "{}",
        run.summary()
    );
    let held = run.events("voice_route_held");
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0]["to"], "live");
    assert!(run.seen.second.is_empty());
    assert_eq!(run.played(1), vec![LIVE; 48]);
}

/// #1421, the Seventh's P2: the loop's own wiring, from the asking to the
/// hum. The person's turn is unclear, the lamp asks "Sorry?" in writing, and
/// that turn comes back with nothing said (#1417). With `LAMP_UNCLEAR_HUM=on`
/// the lamp hums its "hm?" as a sound turn of its own, to the speaker;
/// without it, silence, as before. A "Sorry?" whose only words are "..." is
/// as silent and hums too; one the model said doesn't.
#[test]
fn an_empty_sorry_in_the_loop_hums_with_the_switch_and_only_then() {
    let _one = super::pilot_faults::one_session();
    // On Live and on the second route alone (the hums, #1722).
    for only in [false, true] {
        let with = |on: bool, written| {
            TEST_UNCLEAR_HUM.with(|hum| hum.set(on));
            let run = run(Script {
                second: false,
                only,
                written,
                mute_at_frame: Some(900),
                ..Script::question(Live::Unclear)
            });
            TEST_UNCLEAR_HUM.with(|hum| hum.set(false));
            run
        };
        let rows = |run: &Run, kind: &str| -> Vec<Value> {
            run.trace
                .iter()
                .filter(|event| event["kind"] == kind)
                .cloned()
                .collect()
        };
        let asked = |run: &Run| {
            run.trace
                .iter()
                .filter(|event| {
                    event["kind"] == "input_admitted" && event["admission_basis"] == "asked_again"
                })
                .count()
        };

        // The transcript's two ends, then the hum, played to the speaker.
        let silent = with(true, Written::Silent);
        assert_eq!(silent.seen.live.is_empty(), only, "only {only}");
        assert_eq!(rows(&silent, "provider_turn_unclear").len(), 1);
        assert_eq!(asked(&silent), 1, "the lamp asked Sorry?");
        let owed = rows(&silent, "unclear_hum_owed");
        assert_eq!(owed.len(), 1, "{:?}", rows(&silent, "turn_finished"));
        let played = rows(&silent, "unclear_hum");
        assert_eq!(played.len(), 1);
        assert_eq!(played[0]["plays"], true, "{played:?}");
        assert_eq!(rows(&silent, "unclear_hum_attempted").len(), 1);
        assert!(rows(&silent, "head_tap_cue_attempted").is_empty());
        // Its ring frames show the listening face, every one of them while its
        // turn lasts, as the tap's hum's do (the Seventh's P3 on #1421): the
        // first is set in the pass that admits it, the rest by the ring's own
        // reading of the turn (`tell_ring_of_turn`).
        let hum = rows(&silent, "input_admitted")
            .into_iter()
            .find(|row| row["admission_basis"] == "unclear_hum")
            .expect("the hum's own turn");
        let faces: Vec<bool> = rows(&silent, "ring_requested")
            .iter()
            .filter(|row| row["owner"] == hum["owner"])
            .map(|row| row["frame"]["listening"] == true)
            .collect();
        assert!(faces.len() > 1, "{faces:?}");
        assert!(faces.iter().all(|listening| *listening), "{faces:?}");

        // The switch off: asked the same, and silence, as before.
        let off = with(false, Written::Silent);
        assert_eq!(asked(&off), 1);
        assert!(rows(&off, "unclear_hum_owed").is_empty());
        assert!(rows(&off, "unclear_hum").is_empty());
        assert!(rows(&off, "unclear_hum_attempted").is_empty());

        // "..." is nothing said: cut as such, and the hum owed after it.
        let dots = with(true, Written::Dots);
        let owed = rows(&dots, "unclear_hum_owed");
        assert_eq!(owed.len(), 1);
        assert_eq!(owed[0]["after"], "nothing_said");
        assert_eq!(rows(&dots, "unclear_hum")[0]["plays"], true);

        // A "Sorry?" the model said: nothing owed, nothing hummed.
        let said = with(true, Written::Said(6));
        assert_eq!(asked(&said), 1);
        assert!(rows(&said, "unclear_hum_owed").is_empty());
        assert!(rows(&said, "unclear_hum").is_empty());

        // Said again once the first "hm?" plays, heard as unclear again, and
        // the second empty "Sorry?" hums too, well inside the tap's 30 s: it
        // answers the person's words, so it doesn't go blank the second time
        // (the Seventh's P2 on #1421).
        TEST_UNCLEAR_HUM.with(|hum| hum.set(true));
        let again = run(Script {
            second: false,
            only,
            written: Written::Silent,
            live: vec![Live::Unclear, Live::Unclear],
            speak_again: Some(When::Accepted(1)),
            mute_at_frame: Some(1_100),
            ..Script::question(Live::Unclear)
        });
        TEST_UNCLEAR_HUM.with(|hum| hum.set(false));
        assert_eq!(rows(&again, "provider_turn_unclear").len(), 2);
        assert_eq!(asked(&again), 2);
        let hums: Vec<bool> = rows(&again, "unclear_hum")
            .iter()
            .map(|row| row["plays"] == true)
            .collect();
        assert_eq!(hums, [true, true], "{:?}", rows(&again, "unclear_hum"));
        assert_eq!(rows(&again, "unclear_hum_attempted").len(), 2);
    }
}

/// #828, the first builder at 05:07 on 19 October: with
/// `LAMP_MISHEARD_HUM=on`, a cut-in the worker held as misheard gets the
/// lamp's "hm?" as a sound turn of its own, with the listening face, and no
/// written "Sorry?" turn goes to the model; within 100 ms of the hold. A
/// line held as unclear that was no cut-in is asked again as before, and so
/// is a misheard cut-in without the switch.
#[test]
fn with_the_switch_a_misheard_cut_in_hums_and_sends_no_model_turn() {
    let _one = super::pilot_faults::one_session();
    // On Live and on the second route alone (the hums, #1722).
    for only in [false, true] {
        let with = |on: bool, live: Live| {
            TEST_MISHEARD_HUM.with(|hum| hum.set(on));
            let run = run(Script {
                second: false,
                only,
                written: Written::Said(6),
                mute_at_frame: Some(900),
                ..Script::question(live)
            });
            TEST_MISHEARD_HUM.with(|hum| hum.set(false));
            run
        };
        let rows = |run: &Run, kind: &str| -> Vec<Value> {
            run.trace
                .iter()
                .filter(|event| event["kind"] == kind)
                .cloned()
                .collect()
        };
        let asked = |run: &Run| {
            run.trace
                .iter()
                .filter(|event| {
                    event["kind"] == "input_admitted" && event["admission_basis"] == "asked_again"
                })
                .count()
        };

        let hummed = with(true, Live::UnclearCutIn);
        assert_eq!(hummed.seen.live.is_empty(), only, "only {only}");
        let unclear = rows(&hummed, "provider_turn_unclear");
        assert_eq!(unclear.len(), 1);
        assert_eq!(unclear[0]["hum"], true, "{unclear:?}");
        assert_eq!(asked(&hummed), 0, "no model turn for the misheard line");
        let owed = rows(&hummed, "unclear_hum_owed");
        assert_eq!(owed.len(), 1, "{owed:?}");
        assert_eq!(owed[0]["after"], "misheard_cut_in");
        let played = rows(&hummed, "unclear_hum");
        assert_eq!(played.len(), 1, "{played:?}");
        assert_eq!(played[0]["plays"], true, "{played:?}");
        assert_eq!(played[0]["misheard_turn"], unclear[0]["turn"]);
        let held_us = unclear[0]["at_us"].as_u64().unwrap();
        let hum_us = played[0]["at_us"].as_u64().unwrap();
        assert!(
            hum_us.saturating_sub(held_us) < 100_000,
            "the hum {} us after the hold",
            hum_us.saturating_sub(held_us)
        );
        // Its ring frames show the listening face, as the empty Sorry?'s hum's do.
        let hum = rows(&hummed, "input_admitted")
            .into_iter()
            .find(|row| row["admission_basis"] == "unclear_hum")
            .expect("the hum's own turn");
        let faces: Vec<bool> = rows(&hummed, "ring_requested")
            .iter()
            .filter(|row| row["owner"] == hum["owner"])
            .map(|row| row["frame"]["listening"] == true)
            .collect();
        assert!(faces.len() > 1, "{faces:?}");
        assert!(faces.iter().all(|listening| *listening), "{faces:?}");

        // A line held as unclear that was no cut-in: asked again, as before.
        let other = with(true, Live::Unclear);
        assert_eq!(asked(&other), 1);
        assert!(rows(&other, "unclear_hum_owed").is_empty());
        assert!(rows(&other, "unclear_hum").is_empty());

        // The switch off: the misheard cut-in is asked again, as before.
        let off = with(false, Live::UnclearCutIn);
        assert_eq!(asked(&off), 1);
        assert!(rows(&off, "unclear_hum_owed").is_empty());
        assert!(rows(&off, "unclear_hum").is_empty());
    }
}

/// #933 on the second route (the first builder, 19:48 on #1722): an answer
/// spoken by the second route alone, its level rising on stressed words and
/// falling between sentences, makes the head nod and sway. What the voice
/// says for Expression (`voice/progress`, `voice/sound`) comes from the loop,
/// whichever route answers, so this is the whole path: the second route's
/// audio, the speaker, `expression_out`, and Expression's planner with the
/// product's `speaking` section.
#[test]
fn the_second_routes_answer_nods_and_sways_the_head_as_it_is_spoken() {
    let _one = super::pilot_faults::one_session();
    // 40 ms packets: a stressed word about every 1.1 s, syllables between,
    // and a half-second pause every 3 s. On the meter's scale (-48 to -14
    // dBFS, `ring_face::level_of`) the syllables read about 0.3 and 0.45 and a
    // stressed word 1.0, so each stressed word stands out, as in speech. (At
    // 3,000 and 11,000 the syllables filled the meter too, so only a
    // sentence's onset could nod, and a slow start that lost one failed the
    // count: Seventh and Eighth on #1722, 19 Oct.)
    fn shape(packet: usize) -> i16 {
        let into = (packet * 40) % 3_000;
        if into >= 2_500 {
            0
        } else if into % 1_100 < 120 {
            8_000
        } else if packet.is_multiple_of(3) {
            422
        } else {
            760
        }
    }
    let run = run(Script {
        second: false,
        only: true,
        second_packets: 200,
        second_shape: Some(shape),
        expression: true,
        ..Script::question(Live::Answer(12))
    });
    run.assert_clean();
    let sounds = run
        .expression
        .iter()
        .filter(|input| matches!(input, lamp_expression::Input::Sound(_)))
        .count();
    assert!(sounds > 100, "{sounds} pieces of sound said for Expression");
    // Pieces lost between the voice and this test, by their stamps'
    // sequence: said with the counts below, so a loss is told from the
    // planner.
    let mut sequences: Vec<u64> = run
        .expression
        .iter()
        .filter_map(|input| match input {
            lamp_expression::Input::Sound(sound) => Some(sound.playout.stamp.sequence),
            _ => None,
        })
        .collect();
    sequences.sort_unstable();
    let lost = sequences.windows(2).map(|w| w[1] - w[0] - 1).sum::<u64>();
    // Through Expression's planner, with the product's speaking section.
    let mut catalogue = lamp_expression::Catalogue::from_json(include_str!(
        "../../../../../deploy/product/settings/expression.json"
    ))
    .unwrap();
    // The catalogue's own body lag (300 ms), past the 200 ms the voice
    // says its sound ahead: the speaking motion takes the farthest piece
    // said (#1759, 20 Oct). At 0 here, it once hid that no lamp nodded.
    if catalogue.speaking.is_none() {
        catalogue.speaking = lamp_expression::Catalogue::from_json(include_str!(
            "../../../../expression/alive-behaviours.json"
        ))
        .unwrap()
        .speaking;
    }
    let mut engine = lamp_expression::Engine::new(catalogue, true, EXPRESSION_CLOCK).unwrap();
    let at_of = |input: &lamp_expression::Input| match input {
        lamp_expression::Input::Progress(p) => p.timing.stamp.at_us,
        lamp_expression::Input::Sound(s) => s.playout.stamp.at_us,
        _ => 0,
    };
    let mut inputs = run.expression.clone();
    inputs.sort_by_key(at_of);
    let (first, last) = (at_of(&inputs[0]), at_of(inputs.last().unwrap()));
    let mut behaviours: Vec<Vec<String>> = Vec::new();
    let mut next = inputs.into_iter().peekable();
    let mut sequence = 0;
    let mut at = first;
    while at <= last + 1_500_000 {
        if at % 100_000 < 20_000 {
            sequence += 1;
            for context in [
                serde_json::json!({"kind":"room","stamp":{"clock_id":EXPRESSION_CLOCK,"at_us":at,"sequence":sequence},"value":{"state":"quiet"}}),
                serde_json::json!({"kind":"light","stamp":{"clock_id":EXPRESSION_CLOCK,"at_us":at,"sequence":sequence},"value":{"on":true,"colour":"warm","level":3}}),
                serde_json::json!({"kind":"joints","stamp":{"clock_id":EXPRESSION_CLOCK,"at_us":at,"sequence":sequence},
                    "value":{"name":["base_yaw","shoulder","elbow","head_roll","head_pitch"],"position":[0,0,0,0,0],"velocity":[],"effort":[],"commanded":[]}}),
            ] {
                engine
                    .ingest(at, serde_json::from_value(context).unwrap())
                    .unwrap();
            }
        }
        while next.peek().is_some_and(|input| at_of(input) <= at) {
            let input = next.next().unwrap();
            // A piece the planner refuses (out of its turn) is not a fault here.
            let _ = engine.ingest(at, input);
        }
        behaviours.push(engine.tick(at).state.behaviours);
        at += 20_000;
    }
    let starts = |name: &str| {
        behaviours
            .windows(2)
            .filter(|w| !w[0].iter().any(|b| b == name) && w[1].iter().any(|b| b == name))
            .count()
    };
    let nods = starts("nodding on a stressed word");
    let sways = starts("swaying between sentences");
    // A stressed word about every 1.1 s over 8 s in three sentences: a nod
    // on one in each, never two within 4 s (the first builder on #2030),
    // so two; a flat answer (`second_shape: None`) never sways.
    assert!(nods >= 2, "{nods} nods ({sounds} pieces, {lost} lost)");
    assert!(sways >= 1, "{sways} sways ({sounds} pieces, {lost} lost)");
}

/// #1620 on the second route (`LAMP_VOICE_ROUTE=second`, the owner, 19 Oct:
/// every voice feature on it). The voice is muted while the second route's
/// worker holds the request, its chat still streaming: the worker is sent
/// Stop with the others, reads it and is reaped inside the shared deadline,
/// and nothing is forced, failed or left behind, as on Live.
#[test]
fn a_stop_while_the_second_route_alone_is_mid_request_is_clean() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        second: false,
        only: true,
        second_holds: true,
        mute_at_frame: Some(300),
        ..Script::question(Live::Answer(12))
    });
    run.assert_clean();
    assert!(run.exits.contains(&("second", true)), "{:?}", run.exits);
    assert!(run.seen.stopped.contains(&"second"));
    // The whole line reached it and nothing came back before the stop.
    whole_line(&run.seen.second, 1);
    assert!(run.played(1).is_empty(), "{}", run.summary());
    assert!(run.events("worker_exit_failed").is_empty());
    assert!(run.events("stop_delivery_error").is_empty());
}

/// #1620 on the second route: a worker that reads Stop and goes on running
/// is killed at the shared deadline, reaped, and named as the cleanup's
/// cause (`ForcedStop`, its own role), exactly as Gemini's worker is. The
/// rest of the rig still stops cleanly, and the conversation's own end
/// (the mute) is kept beside the cleanup failure.
#[test]
fn a_second_worker_that_ignores_stop_is_killed_and_named_as_geminis_is() {
    let _one = super::pilot_faults::one_session();
    for (only, role) in [(false, "provider"), (true, "second")] {
        let run = run(Script {
            second: false,
            only,
            ignores_stop: Some(role),
            ..Script::question(Live::Answer(12))
        });
        eprintln!("{}", run.summary());
        let error = run.result.as_ref().unwrap_err().to_string();
        let forced = run.events("worker_forced_shutdown");
        assert_eq!(forced.len(), 1, "{role}: {}", run.summary());
        assert_eq!(forced[0]["worker"], role);
        let cause = forced[0]["error"].as_str().unwrap();
        for said in [
            format!("{role} worker did not stop within shared 300 ms deadline"),
            "forced termination required; reaped with".to_owned(),
        ] {
            assert!(cause.contains(&said), "{role}: {cause}");
            assert!(error.contains(&said), "{role}: {error}");
        }
        assert!(
            error.contains("PhysicalMute") || error.contains("mute"),
            "{role}: {error}"
        );
        for (exited_role, exited) in &run.exits {
            assert!(exited, "{exited_role} child was not reaped ({role})");
            assert!(
                run.seen.stopped.contains(exited_role),
                "{exited_role} never saw Stop"
            );
        }
        assert!(run.exits.contains(&(role, true)), "{:?}", run.exits);
    }
}

/// #1567/#1864 on the second route: its worker says its link is Ready as
/// it starts, before any service is asked
/// (`the_second_workers_link_is_ready_before_any_service_is_asked`), so a
/// speech to text, chat or voice that refuses or does not answer can never
/// hold the voice's readiness. Each line then fails on its own
/// (`TurnFailed`, before any audio), and a service runs past the 25 s
/// startup budget to its mute, with `LAMP_START_OFFLINE` on or off; nothing
/// waits for a link.
#[test]
fn the_second_route_alone_with_its_services_refused_stays_up_past_the_startup_budget() {
    let _one = super::pilot_faults::one_session();
    let past = INPUT_READINESS_TIMEOUT + Duration::from_secs(3);
    for start_offline in [true, false] {
        crate::session_lifetime::set_start_offline_for_test(Some(start_offline));
        let run = run(Script {
            second: false,
            only: true,
            second_fails: Some(Fault::Transport),
            mute_at_frame: Some(past.as_millis() as u64 / 10),
            limit: past + Duration::from_secs(5),
            ..Script::question(Live::Answer(12))
        });
        crate::session_lifetime::set_start_offline_for_test(None);
        run.assert_clean();
        let error = run.result.as_ref().unwrap_err().to_string();
        assert!(!error.contains("never became ready"), "{error}");
        assert!(run.events("waiting_for_link").is_empty());
        assert_eq!(
            run.finished(1)["outcome"],
            "provider_failed",
            "{}",
            run.summary()
        );
        // Muted past the budget, not ended at it.
        let at = |kind: &str| run.events(kind)[0]["at_us"].as_u64().unwrap();
        assert!(
            at("physical_mute") - at("capture_started")
                >= INPUT_READINESS_TIMEOUT.as_micros() as u64,
            "{}",
            run.summary()
        );
        whole_line(&run.seen.second, 1);
        assert!(run.played(1).is_empty());
    }
}

/// Services that refuse every call, and count each.
#[derive(Default)]
struct Refusing {
    asked: std::sync::atomic::AtomicUsize,
}

impl crate::second_provider::Services for Refusing {
    fn listen(
        &self,
    ) -> futures_util::future::BoxFuture<
        'static,
        crate::second_provider::net::Result<Box<dyn crate::second_provider::Listener>>,
    > {
        self.asked.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Err(crate::second_provider::net::Error::Transport(
                "speech to text",
            ))
        })
    }
    fn chat(
        &self,
        _: Vec<crate::second_provider::net::Message>,
    ) -> futures_util::stream::BoxStream<
        'static,
        crate::second_provider::net::Result<crate::second_provider::net::ChatEvent>,
    > {
        use futures_util::StreamExt;
        self.asked.fetch_add(1, Ordering::SeqCst);
        futures_util::stream::once(async {
            Err(crate::second_provider::net::Error::Transport("chat"))
        })
        .boxed()
    }
    fn speak(
        &self,
        _: String,
    ) -> futures_util::stream::BoxStream<'static, crate::second_provider::net::Result<Vec<i16>>>
    {
        use futures_util::StreamExt;
        self.asked.fetch_add(1, Ordering::SeqCst);
        futures_util::stream::once(async {
            Err(crate::second_provider::net::Error::Transport("speech"))
        })
        .boxed()
    }
}

/// The worker side of
/// `the_second_route_alone_with_its_services_refused_stays_up_past_the_startup_budget`,
/// through the second route's own `run`: it says Ready and its link Ready
/// (session 1, `Initial`) before any service is asked, and returns at once,
/// cleanly, on Stop.
#[test]
fn the_second_workers_link_is_ready_before_any_service_is_asked() {
    let directory = SessionDirectory::create().unwrap();
    let (mut parent, worker) = super::pair(&directory, "second");
    let services = Arc::new(Refusing::default());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    parent.control.send(Control::Stop).unwrap();
    let started = Instant::now();
    let engine_services = services.clone();
    runtime
        .block_on(async move {
            let engine = crate::second_provider::Engine::new(
                engine_services,
                || crate::second_provider::Told::from(String::new()),
                Arc::new(|_, _, _, _| Box::pin(async { serde_json::json!({}) })),
            );
            crate::second_provider::worker::run(worker, new_boot().unwrap(), engine, None, None)
                .await
        })
        .unwrap();
    assert!(started.elapsed() < Duration::from_millis(300));
    assert!(matches!(
        parent.control.receive::<WorkerEvent>().unwrap(),
        Some(WorkerEvent::Ready)
    ));
    match parent.control.receive::<WorkerEvent>().unwrap() {
        Some(WorkerEvent::ProviderLink { link }) => {
            assert_eq!(link.session, 1);
            assert!(matches!(
                link.state,
                ProviderLinkState::Ready {
                    context: crate::provider_link::LinkContext::Initial,
                    ..
                }
            ));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(services.asked.load(Ordering::SeqCst), 0);
}

/// #3755 part 3, the Reviewer-lead's P3 on #3790: with the wait sounds and
/// the down line on, as the product runs, the worker still says Ready and
/// its link Ready before any service is asked; then it renders those lines
/// at once, before any turn, and a Stop while they're in flight still ends
/// it inside the budget.
#[test]
fn the_second_worker_renders_its_lines_at_the_start_after_its_link_is_ready() {
    let directory = SessionDirectory::create().unwrap();
    let (mut parent, worker) = super::pair(&directory, "second");
    let services = Arc::new(Refusing::default());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let engine_services = services.clone();
    let worker_thread = std::thread::spawn(move || {
        runtime.block_on(async move {
            let engine = crate::second_provider::Engine::new(
                engine_services,
                || crate::second_provider::Told::from(String::new()),
                Arc::new(|_, _, _, _| Box::pin(async { serde_json::json!({}) })),
            )
            .waiting_with_a_sound(true)
            .saying_when_down(true);
            crate::second_provider::worker::run(worker, new_boot().unwrap(), engine, None, None)
                .await
        })
    });
    // `receive` doesn't wait: poll for each of the worker's first events.
    let mut first = || {
        let asked = Instant::now();
        loop {
            if let Some(event) = parent.control.receive::<WorkerEvent>().unwrap() {
                return event;
            }
            assert!(asked.elapsed() < Duration::from_secs(2), "no event");
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    assert!(matches!(first(), WorkerEvent::Ready));
    assert!(matches!(first(), WorkerEvent::ProviderLink { .. }));
    // No turn: only the start's renders ask the voice, once for each clip,
    // the wait sounds and the three down lines both (the Reviewer-lead's P3
    // on #3814: one render dropped must fail this).
    let clips = crate::second_provider::CUE_WORDS.len() + 3;
    let started = Instant::now();
    while services.asked.load(Ordering::SeqCst) < clips
        && started.elapsed() < Duration::from_secs(2)
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    // A moment more: nothing beyond them.
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        services.asked.load(Ordering::SeqCst),
        clips,
        "the start's renders"
    );
    parent.control.send(Control::Stop).unwrap();
    let stopping = Instant::now();
    worker_thread.join().unwrap().unwrap();
    assert!(
        stopping.elapsed() < Duration::from_millis(300),
        "{:?}",
        stopping.elapsed()
    );
}

/// #1620 on the second route, the gap this closes: the second worker's
/// process (`worker::serve`) runs on a current-thread tokio runtime, ended
/// when `run` returns on Stop. Its async network I/O (the
/// chat's stream, the voice's download, speech to text's socket) is
/// dropped with it at once, but tokio's drop waits, with no bound, for
/// every `spawn_blocking` still running (a plain drop): each tool call (`run_call`, up to
/// `tools::ASK_DEADLINE`, 1.5 s, for the executive and the skills, longer
/// for the tasks part), a text's tool call (`texts_ends`), and a DNS lookup
/// (hyper-util's and tokio's resolvers both use the blocking pool). A stop
/// during one holds the worker past the shared 300 ms (750 ms with
/// diagnostics), so it is killed and the run's cleanup fails, which keeps
/// the restart guard's mute, #1620's symptom. The Live worker runs its
/// calls on detached threads, which never hold its exit.
///
/// Built as `serve` builds its runtime and ended by `serve`'s own
/// `worker::finish`, with the real `tool_runner` asking an executive that
/// takes the call and does not answer. With a plain drop it took 1.50 s.
#[test]
fn a_stop_during_a_slow_tool_call_lets_the_second_worker_exit_within_its_deadline() {
    use std::os::unix::net::UnixListener;
    let directory = SessionDirectory::create().unwrap();
    let executive = UnixListener::bind(directory.path.join(crate::tools::CALL_SOCKET)).unwrap();
    // A chat stream that has said nothing yet, as DeepSeek thinking.
    let chat = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let chat_at = chat.local_addr().unwrap();
    let settings = crate::tools::Settings {
        exec: Some(directory.path.clone()),
        ..Default::default()
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (taken, held) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        let (call, _) = executive.accept().unwrap();
        let (stream, _) = chat.accept().unwrap();
        taken.send(()).unwrap();
        // Held open, unanswered, until the test has timed the stop, not a
        // fixed 3 s the test then waited out (#1806).
        let _ = released.recv_timeout(Duration::from_secs(10));
        drop((call, stream));
    });
    runtime.block_on(async move {
        let runner = crate::second_provider::worker::tool_runner(settings, None, None, false);
        tokio::spawn(runner(1, false, "light_on".into(), serde_json::json!({})));
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut stream = tokio::net::TcpStream::connect(chat_at).await.unwrap();
            let _ = stream.read(&mut [0; 16]).await;
        });
        // Both are in flight: the call in the blocking pool, the stream
        // waiting for its first byte. Then `run` returns on Stop.
        tokio::task::spawn_blocking(move || held.recv_timeout(Duration::from_secs(5)))
            .await
            .unwrap()
            .unwrap();
    });
    let stopped = Instant::now();
    crate::second_provider::worker::finish(runtime);
    let took = stopped.elapsed();
    release.send(()).unwrap();
    holder.join().unwrap();
    assert!(
        took < Duration::from_millis(300),
        "the second worker's runtime took {took:?} to end after Stop"
    );
}

/// #1670, the owner's rule of 19 Oct (a voice feature works on the second
/// route): the inner state reads two of the voice's topics, `voice/state`
/// and `voice/heard`. With the second route alone, what it heard the person
/// say reaches the coordinator's heard path (`turn` null, the branch that
/// says `voice/heard`), and the ring phases that `voice/state` is made from
/// pass through listening, waiting and speaking, as with Gemini.
#[test]
fn the_second_route_alone_gives_mood_what_was_heard_and_the_voices_phases() {
    let _one = super::pilot_faults::one_session();
    let alone = run(Script {
        second: false,
        only: true,
        second_hears: Some("what time is it"),
        ..Script::question(Live::Answer(12))
    });
    alone.assert_clean();
    let heard: Vec<_> = alone
        .events("transcript")
        .into_iter()
        .filter(|event| event["turn"].is_null())
        .collect();
    assert_eq!(heard.len(), 1, "{}", alone.summary());
    assert_eq!(heard[0]["text"], "what time is it");
    assert_eq!(heard[0]["finished"], true);
    let phases = dedup(&alone.seen.phases);
    for phase in [
        RingPhase::Listening,
        RingPhase::Waiting,
        RingPhase::Speaking,
    ] {
        assert!(phases.contains(&phase), "{phase:?}: {phases:?}");
    }
}

/// #2233 through the session (the reviewer's P3, 16:46): with
/// `LAMP_CAMERA_FOLLOWS_EYES=on`, a voice whose camera is the eyes' own (an
/// eyes link and no camera of its own) publishes the camera permitted in the
/// authority with the microphone, and denied with it after the mute. Without
/// the switch, the camera is never permitted.
#[test]
fn with_the_switch_the_eyes_camera_is_permitted_with_the_microphone_through_the_session() {
    let _one = super::pilot_faults::one_session();
    for on in [false, true] {
        let directory = std::env::temp_dir().join(format!(
            "lamp-voice-eyes-camera-{on}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let permit = directory.join("permit.sock");
        let _eyes = std::os::unix::net::UnixDatagram::bind(&permit).unwrap();
        TEST_EYES.with(|eyes| {
            *eyes.borrow_mut() = Some(crate::body_link::EyesLink::to(permit).unwrap())
        });
        TEST_CAMERA_FOLLOWS_EYES.with(|follows| follows.set(on));
        let run = run(Script::question(Live::Answer(12)));
        TEST_CAMERA_FOLLOWS_EYES.with(|follows| follows.set(false));
        TEST_EYES.with(|eyes| eyes.borrow_mut().take());
        let _ = std::fs::remove_dir_all(&directory);
        run.assert_clean();
        let installs = dedup(&run.seen.installs);
        assert!(
            installs.iter().any(|install| install.1),
            "on {on}: {installs:?}"
        );
        assert!(
            installs.last().is_some_and(|install| !install.1),
            "on {on}: {installs:?}"
        );
        for install in &installs {
            assert_eq!(install.2, on && install.1, "on {on}: {installs:?}");
        }
    }
}

/// #1803: in onboarding's near-call try window the person calls the lamp's
/// name for the measurement, and an answer within a second of it spoils the
/// try. The voice holds its reply to that call, on Gemini Live and on the
/// second route alone: the turn ends unanswered, nothing is played, and it is
/// not counted as a sound. The hold is the step's own rule
/// (`Onboarding::near_holds_reply`), read on its evidence.
#[test]
fn a_call_in_the_near_call_window_is_not_answered_on_either_route() {
    let _one = super::pilot_faults::one_session();
    for only in [false, true] {
        let held = run(Script {
            second: false,
            only,
            near_window_ms: Some(0),
            near_name_ms: Some(200),
            mute_at_frame: Some(300),
            second_hears: Some("Lamp."),
            heard_as_call: true,
            ..Script::question(Live::Answer(12))
        });
        held.assert_clean();
        assert!(held.played(1).is_empty(), "{only}: {}", held.summary());
        let endpoint = held.events("local_endpoint");
        assert_eq!(endpoint.len(), 1, "{endpoint:?}");
        assert!(endpoint[0].get("not_speech").is_none());
        if only {
            // The second route is told it is the step's call and never
            // answers the name alone, so its End goes on for its words
            // (#3923): they reach the step as its call by text.
            assert!(endpoint[0].get("onboarding_near_call").is_none());
            let calls = held.events("onboarding_text_call");
            assert_eq!(calls.len(), 1, "{}", held.summary());
            assert_eq!(calls[0]["taken"], true);
        } else {
            assert_eq!(held.finished(1)["outcome"], "onboarding_near_call");
            assert!(endpoint[0]["onboarding_near_call"].is_string());
        }
    }
}

/// #1803, the reviewer on #1964: in the same window only the name is held.
/// A line with no name heard ("skip that") and one with more said than a
/// call ("Lamp, can we skip this?", 1.8 s) are answered as any other.
#[test]
fn a_line_that_is_more_than_the_name_is_answered_in_the_near_call_window() {
    let _one = super::pilot_faults::one_session();
    for only in [false, true] {
        for (name, speech_frames) in [(None, 60), (Some(200), 180)] {
            let answered = run(Script {
                second: false,
                only,
                near_window_ms: Some(0),
                near_name_ms: name,
                speech_frames,
                ..Script::question(Live::Answer(12))
            });
            answered.assert_clean();
            assert_eq!(
                answered.played(1),
                vec![if only { SECOND } else { LIVE }; 48],
                "{only} {name:?} {speech_frames}"
            );
            assert!(
                answered.events("local_endpoint")[0]
                    .get("onboarding_near_call")
                    .is_none()
            );
        }
    }
}

/// lamp-52e6, 27 October (#3504): a line begun while how_you_sound waits for
/// a call is told the second route as `StepCall`, right after its Start,
/// whether or not lamp-name's hearing has come yet (on 52e6 a bare "Lamp"
/// went on as a turn and its answer cut the next prompt). Outside the step
/// nothing is told.
#[test]
fn a_line_begun_while_the_step_waits_for_a_call_is_told_the_second_route() {
    let _one = super::pilot_faults::one_session();
    for window in [Some(0), None] {
        let run = run(Script {
            second: false,
            only: true,
            near_window_ms: window,
            second_hears: Some("Lamp."),
            ..Script::question(Live::Answer(12))
        });
        run.assert_clean();
        let told: Vec<u64> = run
            .seen
            .second
            .iter()
            .filter_map(|input| match input {
                ProviderInput::StepCall { request, .. } => Some(*request),
                _ => None,
            })
            .collect();
        assert_eq!(
            told,
            if window.is_some() { vec![1] } else { vec![] },
            "{window:?}: {}",
            run.summary()
        );
        assert_eq!(
            run.events("onboarding_step_call").len(),
            usize::from(window.is_some())
        );
    }
}

/// lamp-29e7, build 8001 (#3923): a bare "Lamp" the name detector missed
/// but speech to text heard is told back by the second route
/// (`StepCallHeard`), and the coordinator gives the step the line's speech
/// start, mapped to the Unix clock by the try's prompt end. The step takes
/// it only while its name part listens for a call; outside the step the
/// line is never told as a call.
#[test]
fn a_bare_name_heard_by_text_reaches_the_step_as_its_call() {
    let _one = super::pilot_faults::one_session();
    // An earlier line's words coming back late, after this line became the
    // call's, leave its place to it (the Reviewer-lead on #4006).
    for (window, late_words_first) in [(Some(0), false), (Some(0), true), (None, false)] {
        let run = run(Script {
            second: false,
            only: true,
            near_window_ms: window,
            second_hears: Some("Lamp."),
            heard_as_call: true,
            late_words_first,
            ..Script::question(Live::Answer(12))
        });
        run.assert_clean();
        let calls = run.events("onboarding_text_call");
        if window.is_some() {
            assert_eq!(calls.len(), 1, "{}", run.summary());
            assert_eq!(calls[0]["turn"], 1);
            assert_eq!(calls[0]["taken"], true, "{calls:?}");
            assert!(run.played(1).is_empty(), "{}", run.summary());
        } else {
            assert!(calls.is_empty(), "{calls:?}");
        }
    }
}

/// c0a4, head 9d4d3e0d9 (#3923): each call began about 0.8 s after its
/// prompt ended, and lamp-name heard something of it. The line was held as
/// a near call and dropped at its End, so on the second route its words
/// never came back (no "at the line's end") and nothing reached the step.
/// A line begun 0.8 s after the prompt's end, heard by the detector, is
/// told the second route as the step's call, its End goes on, and its words
/// reach the step as a call by text; nothing is played.
#[test]
fn a_call_begun_after_the_prompt_and_heard_by_the_detector_still_reaches_the_step_by_text() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        second: false,
        only: true,
        near_window_ms: Some(0),
        near_name_ms: Some(800),
        speech_from: 80,
        second_hears: Some("Hey Lamp"),
        heard_as_call: true,
        ..Script::question(Live::Answer(12))
    });
    run.assert_clean();
    assert_eq!(
        run.events("onboarding_step_call").len(),
        1,
        "{}",
        run.summary()
    );
    assert!(run.played(1).is_empty(), "{}", run.summary());
    let ended = run
        .seen
        .second
        .iter()
        .any(|input| matches!(input, ProviderInput::End { request: 1, .. }));
    assert!(ended, "{}", run.summary());
    let calls = run.events("onboarding_text_call");
    assert_eq!(calls.len(), 1, "{}", run.summary());
    assert_eq!(calls[0]["taken"], true, "{calls:?}");
}

/// c0a4, 28 October (#3923, c2): "Hey Lamp." begun as the step was coming
/// to wait for a call was answered as a turn, never told as the step's
/// call. A line whose Start came before the step waited is told once it
/// does, before its End, so the route leaves the name alone to the step.
#[test]
fn a_line_begun_before_the_step_waits_is_told_as_its_call_once_it_does() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        second: false,
        only: true,
        near_window_ms: Some(500),
        near_opens_ms: Some(500),
        second_hears: Some("Hey Lamp."),
        heard_as_call: true,
        ..Script::question(Live::Answer(12))
    });
    run.assert_clean();
    let told = run.events("onboarding_step_call");
    assert_eq!(told.len(), 1, "{}", run.summary());
    assert_eq!(told[0]["late"], true, "{told:?}");
    assert!(run.played(1).is_empty(), "{}", run.summary());
    assert_eq!(run.events("onboarding_text_call").len(), 1);
}

/// c0a4, 28 October (#3923, c3): "Hey, Lamp." was left to the step, but the
/// try's `said` stayed empty: a newer line told as the step's call took the
/// one slot before the first call's words came back. Each call line keeps
/// its own place, so the first one's words still reach the step.
#[test]
fn a_call_whose_words_come_back_after_the_next_line_began_still_reaches_the_step() {
    let _one = super::pilot_faults::one_session();
    let run = run(Script {
        second: false,
        only: true,
        near_window_ms: Some(0),
        second_hears: Some("Hey, Lamp."),
        heard_as_call: true,
        heard_as_call_late: true,
        speak_again: Some(When::Ended(1)),
        turns: 2,
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(Live::Answer(12))
    });
    let calls = run.events("onboarding_text_call");
    let first: Vec<_> = calls.iter().filter(|call| call["turn"] == 1).collect();
    assert_eq!(first.len(), 1, "{}", run.summary());
    assert_eq!(first[0]["taken"], true, "{calls:?}");
}

/// c0a4, 28 October (#3923): a call's text start mapped 2.5 s before the
/// detector's hearing of the same word, from speech blocks kept from before
/// the line, and later calls were refused for beginning well before their
/// prompt ended. A sound before the line (a murmur, too short to be a turn)
/// leaves the line's own start, its trigger, as the call's.
#[test]
fn a_calls_start_is_its_own_lines_not_a_sound_before_it() {
    let _one = super::pilot_faults::one_session();
    const UNIX: u64 = 1_760_000_000_000;
    let run = run(Script {
        second: false,
        only: true,
        near_window_ms: Some(0),
        murmur: Some(When::Settled(0)),
        speech_from: 150,
        second_hears: Some("Hey Lamp"),
        heard_as_call: true,
        ..Script::question(Live::Answer(12))
    });
    run.assert_clean();
    let calls = run.events("onboarding_text_call");
    assert_eq!(calls.len(), 1, "{}", run.summary());
    assert_eq!(calls[0]["taken"], true, "{calls:?}");
    let began = calls[0]["began_unix_ms"].as_u64().unwrap() - UNIX;
    assert!((1_400..=2_000).contains(&began), "{began} ms: {calls:?}");
}

/// The Reviewer-lead on #4006: a slow "Hey, … Lamp" whose pause ends early
/// (`--early-end-ms`) and then goes on is a new turn, given up and continued.
/// It keeps the line's own start, so its call is still taken from when the
/// person began, not refused for a start not known.
#[test]
fn a_slow_call_whose_pause_ended_early_keeps_its_start() {
    let _one = super::pilot_faults::one_session();
    const UNIX: u64 = 1_760_000_000_000;
    let run = run(Script {
        second: false,
        only: true,
        near_window_ms: Some(0),
        early_end_blocks: Some(20),
        speak_again: Some(When::Ended(1)),
        second_hears: Some("Hey, Lamp."),
        heard_as_call: true,
        // The piece given up has its words back only once the line goes on.
        heard_as_call_late: true,
        ..Script::question(Live::Answer(12))
    });
    let continued = run
        .events("input_admitted")
        .into_iter()
        .any(|row| row["admission_basis"] == "continued_after_early_end");
    assert!(continued, "{}", run.summary());
    let taken: Vec<_> = run
        .events("onboarding_text_call")
        .into_iter()
        .filter(|call| call["taken"] == true)
        .collect();
    // The piece and the line going on are one line, from its first start.
    assert!(!taken.is_empty(), "{}", run.summary());
    for call in &taken {
        let began = call["began_unix_ms"].as_u64().unwrap() - UNIX;
        assert!(began <= 700, "{began} ms: {taken:?}");
    }
    assert!(
        taken
            .iter()
            .all(|call| call["began_unix_ms"] == taken[0]["began_unix_ms"]),
        "{taken:?}"
    );
}

/// The Reviewer-lead on #4006 (P3): a call's line whose end went early
/// (`--early-end-ms`) and was not given up told the step it ended twice,
/// at the early end and again at its End. Told once, at the early end, a
/// try never waits again for words already on their way.
#[test]
fn a_call_line_ended_early_tells_the_step_its_end_once() {
    let _one = super::pilot_faults::one_session();
    TEST_LINE_ENDS.with(|ends| ends.set(0));
    let run = run(Script {
        second: false,
        only: true,
        near_window_ms: Some(0),
        early_end_blocks: Some(20),
        second_hears: Some("Hey Lamp"),
        heard_as_call: true,
        // Its words never come back, so its End finds its place still
        // held; the scenario ends at its limit.
        heard_as_call_late: true,
        ..Script::question(Live::Answer(12))
    });
    let early = run.events("early_end");
    assert_eq!(early.len(), 1, "{}", run.summary());
    let continued = run
        .events("input_admitted")
        .into_iter()
        .any(|row| row["admission_basis"] == "continued_after_early_end");
    assert!(!continued, "{}", run.summary());
    assert_eq!(
        TEST_LINE_ENDS.with(|ends| ends.get()),
        1,
        "{}",
        run.summary()
    );
}

/// #2135 (`LAMP_SECOND_OPEN_NAME`, the first builder on #1722 at 02:16):
/// with the second route alone, a line whose words end before a name ("Hey
/// Lamp, my name is"), its answer held, and speech again as soon as its End
/// is sent. With the switch the new line goes on from it: its Start says so
/// (`continues_cut_off`), so the route asks the two as one, and its record
/// says why. Without it, the new line is one of its own, as before. With
/// `LAMP_WAIT_KEEPS_ANSWER=on` too, as on lamp-4ace, the name first waits as
/// a sound while the answer is awaited (`answer_waits`; there it was let
/// go), and with the switch it goes on at once, never waiting.
#[test]
fn a_name_said_just_after_a_line_that_ended_before_it_goes_on_from_it() {
    let _one = super::pilot_faults::one_session();
    for (on, waits) in [(false, false), (true, false), (false, true), (true, true)] {
        TEST_WAIT_KEEPS_ANSWER.with(|wait| wait.set(waits));
        let run = run(Script {
            second: false,
            only: true,
            open_name: on,
            second_hears: Some("Hey Lamp, my name is"),
            second_waits_first: true,
            speak_again: Some(When::Ended(1)),
            turns: 2,
            turn_taking: TurnTaking::VadOnly,
            ..Script::question(Live::Answer(12))
        });
        TEST_WAIT_KEEPS_ANSWER.with(|wait| wait.set(false));
        run.assert_clean();
        assert_eq!(
            run.events("answer_waits").is_empty(),
            !waits || on,
            "on {on}, waits {waits}: {}",
            run.summary()
        );
        let continues: Vec<(u64, bool)> = run
            .seen
            .second
            .iter()
            .filter_map(|input| match input {
                ProviderInput::Start {
                    request,
                    continues_cut_off,
                    ..
                } => Some((*request, *continues_cut_off)),
                _ => None,
            })
            .collect();
        assert_eq!(
            continues,
            [(1, false), (2, on)],
            "on {on}: {}",
            run.summary()
        );
        assert_eq!(run.events("open_name").len(), usize::from(on), "on {on}");
        let went_on = run
            .events("input_admitted")
            .into_iter()
            .filter(|admitted| admitted["goes_on_from_its_name"] == true)
            .count();
        assert_eq!(went_on, usize::from(on), "on {on}: {}", run.summary());
        assert_eq!(run.finished(1)["outcome"], "user_interrupted", "on {on}");
    }
}

/// `LAMP_CALL_TAKES_NEXT_LINE` (24 Oct): with the second route alone, a line
/// that is only the lamp's name ("Lamp."), its answer held as the name's
/// carry holds it, and speech again as soon as its End is sent. With
/// `LAMP_WAIT_KEEPS_ANSWER=on`, as in the product, that speech waits as a
/// sound while the answer is awaited (`answer_waits`; on lamp-4ace a short
/// "tell me a joke" was let go so). With the switch it takes the turn at
/// once, never waiting, and its Start is not marked as going on from the
/// name's line: the second route joins the two itself. Without the answer
/// wait it is decided at once either way.
#[test]
fn the_line_just_after_the_bare_name_takes_the_turn_at_once() {
    let _one = super::pilot_faults::one_session();
    for (on, waits) in [(false, true), (true, true), (false, false), (true, false)] {
        TEST_WAIT_KEEPS_ANSWER.with(|wait| wait.set(waits));
        let run = run(Script {
            second: false,
            only: true,
            call_takes_next: on,
            second_hears: Some("Lamp."),
            second_waits_first: true,
            speak_again: Some(When::Ended(1)),
            turns: 2,
            turn_taking: TurnTaking::VadOnly,
            ..Script::question(Live::Answer(12))
        });
        TEST_WAIT_KEEPS_ANSWER.with(|wait| wait.set(false));
        run.assert_clean();
        assert_eq!(
            run.events("answer_waits").is_empty(),
            !waits || on,
            "on {on}, waits {waits}: {}",
            run.summary()
        );
        assert_eq!(
            run.events("call_takes_next").len(),
            usize::from(on),
            "on {on}: {}",
            run.summary()
        );
        let continues: Vec<(u64, bool)> = run
            .seen
            .second
            .iter()
            .filter_map(|input| match input {
                ProviderInput::Start {
                    request,
                    continues_cut_off,
                    ..
                } => Some((*request, *continues_cut_off)),
                _ => None,
            })
            .collect();
        assert_eq!(
            continues,
            [(1, false), (2, false)],
            "on {on}, waits {waits}: {}",
            run.summary()
        );
        assert_eq!(run.finished(1)["outcome"], "user_interrupted", "on {on}");
    }
}

/// The reviewer's P3s on #2355: in a room busy with other people's talk, a
/// sound begun just after a line that ended before its name is still held
/// by the busy-room floor, as the room's next voice is, never joined to the
/// line as its name. The colleague's "…see you at lunch" never becomes the
/// person's name. With the switch, as without it.
#[test]
fn in_a_busy_room_a_sound_after_a_line_that_ends_before_its_name_is_held_as_before() {
    let _one = super::pilot_faults::one_session();
    TEST_ROOM_BUSY.with(|busy| busy.set(true));
    let run = run(Script {
        second: false,
        only: true,
        open_name: true,
        second_hears: Some("Hey Lamp, my name is"),
        second_waits_first: true,
        speak_again: Some(When::Ended(1)),
        mute_at_frame: Some(300),
        turn_taking: TurnTaking::VadOnly,
        ..Script::question(Live::Answer(12))
    });
    TEST_ROOM_BUSY.with(|busy| busy.set(false));
    run.assert_clean();
    assert_eq!(run.events("open_name").len(), 1, "{}", run.summary());
    assert!(!run.events("floor_held").is_empty(), "{}", run.summary());
    assert!(
        !run.events("input_admitted")
            .iter()
            .any(|admitted| admitted["goes_on_from_its_name"] == true),
        "{}",
        run.summary()
    );
    assert!(
        !run.seen.second.iter().any(|input| matches!(
            input,
            ProviderInput::Start {
                continues_cut_off: true,
                ..
            }
        )),
        "{}",
        run.summary()
    );
}

/// #1803, the sentence part: a line ending in onboarding's sentence window
/// goes on to the words, which on the second route come back before any
/// answer. Only their count is recorded, nothing of them is published or
/// kept, and the turn ends unanswered as they arrive; on Gemini Live, whose
/// words may come later, it ends at the answer's first audio. Past the
/// window the same line is answered.
#[test]
fn a_sentence_in_its_window_is_counted_and_not_answered_on_either_route() {
    let _one = super::pilot_faults::one_session();
    for only in [false, true] {
        let held = run(Script {
            second: false,
            only,
            sentence_window_ms: Some((0, 15_000)),
            second_hears: Some("I usually sit at the desk by the window"),
            mute_at_frame: Some(300),
            ..Script::question(Live::Answer(12))
        });
        held.assert_clean();
        assert!(held.played(1).is_empty(), "{only}: {}", held.summary());
        assert_eq!(held.finished(1)["outcome"], "onboarding_sentence");
        let line = held.events("onboarding_sentence_line");
        assert_eq!(line.len(), 1, "{line:?}");
        assert!(line[0]["speech_ms"].as_u64().unwrap() > 0, "{line:?}");
        let words = held.events("onboarding_sentence_words");
        if only {
            assert_eq!(words.len(), 1, "{words:?}");
            assert_eq!(words[0]["words"], 9);
        } else {
            assert!(words.is_empty(), "{words:?}");
        }
        // Nothing of the sentence is in the record.
        assert!(
            !held
                .trace
                .iter()
                .any(|event| event.to_string().contains("desk")),
            "{only}"
        );
        let after = run(Script {
            second: false,
            only,
            sentence_window_ms: Some((0, 1)),
            second_hears: Some("I usually sit at the desk by the window"),
            ..Script::question(Live::Answer(12))
        });
        after.assert_clean();
        assert_eq!(
            after.played(1),
            vec![if only { SECOND } else { LIVE }; 48],
            "{only}"
        );
        assert!(after.events("onboarding_sentence_line").is_empty());
    }
}

/// Jarvis on #2031 (#1803): a planted marker in the sentence never leaves
/// the coordinator on either route. The provider is told the line is
/// count-only right after its Start, before any of its sound; on Live every
/// piece of its words is counted (ten words in three pieces, not the last
/// piece's three), and neither those pieces nor the model's answer, which
/// repeats the marker, is put in the trace (the journal), said on
/// `voice/heard` or `voice/said` (the `transcript` record is that branch),
/// or played.
#[test]
fn the_sentences_words_and_its_answer_are_counted_only_on_either_route() {
    let _one = super::pilot_faults::one_session();
    for only in [false, true] {
        let held = run(Script {
            second: false,
            only,
            sentence_window_ms: Some((0, 15_000)),
            second_hears: Some("I usually sit at the zebrafinch desk by the window"),
            second_says: Some("A zebrafinch desk, lovely."),
            live_hears: &["I usually sit ", "at the zebrafinch desk ", "by the window"],
            live_says: Some("A zebrafinch desk, lovely."),
            mute_at_frame: Some(300),
            ..Script::question(Live::Answer(12))
        });
        held.assert_clean();
        assert!(held.played(1).is_empty(), "{only}: {}", held.summary());
        assert_eq!(held.finished(1)["outcome"], "onboarding_sentence");
        let words = held.events("onboarding_sentence_words");
        assert_eq!(words.len(), 1, "{only}: {words:?}");
        assert_eq!(words[0]["words"], 10, "{only}");
        assert!(held.events("transcript").is_empty(), "{only}");
        assert!(
            !held
                .trace
                .iter()
                .any(|event| event.to_string().contains("zebrafinch")),
            "{only}"
        );
        // The provider was told right after the Start.
        let inputs = if only {
            &held.seen.second
        } else {
            &held.seen.live
        };
        let kinds: Vec<_> = inputs
            .iter()
            .map(|input| serde_json::to_value(input).unwrap()["kind"].clone())
            .collect();
        assert_eq!(
            kinds[..2],
            [json!("start"), json!("count_only")],
            "{kinds:?}"
        );
    }
}

/// The production coordinator generated these records from actual provider
/// IPC. Storage is exercised separately through the shared service writer;
/// this is a metadata-disclosure control, not a hardware/acoustic claim.
#[cfg(target_os = "linux")]
#[test]
fn a_real_coordinator_transcript_is_projected_before_shared_storage() {
    use std::os::unix::fs::PermissionsExt;
    let _one = super::pilot_faults::one_session();
    const SECRET: &str = "COORDINATOR_PRIVATE_CANARY";
    let run = run(Script {
        second: false,
        only: true,
        second_hears: Some(SECRET),
        ..Script::question(Live::Answer(12))
    });
    run.assert_clean();
    let heard = run.events("transcript");
    assert!(heard.iter().any(|event| event["text"] == SECRET));
    assert!(!run.events("input_admitted").is_empty());
    assert!(!run.events("turn_finished").is_empty());
    let directory = SessionDirectory::create().unwrap();
    let voice = directory.path.join("voice");
    let sessions = voice.join("sessions");
    let output = sessions.join("0123456789abcdef0123456789abcdef");
    std::fs::create_dir(&voice).unwrap();
    std::fs::create_dir(&sessions).unwrap();
    std::fs::set_permissions(&voice, std::fs::Permissions::from_mode(0o710)).unwrap();
    std::fs::set_permissions(&sessions, std::fs::Permissions::from_mode(0o750)).unwrap();
    let reader =
        crate::session_reader::Reader::prepare(&output, rustix::process::getegid().as_raw())
            .unwrap();
    std::fs::create_dir(&output).unwrap();
    std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut trace = SessionState::service_with_reader(&output, Ok(reader));
    let generated: Vec<_> = run
        .trace
        .iter()
        .filter(|row| {
            matches!(
                row["kind"].as_str(),
                Some(
                    "transcript"
                        | "input_admitted"
                        | "turn_finished"
                        | "answer_stages"
                        | "answer_first_audio"
                )
            )
        })
        .cloned()
        .collect();
    assert!(!generated.is_empty() && generated.len() < 64);
    for event in &generated {
        record(&mut trace, event.clone()).unwrap();
    }
    trace.flush(&mut None, 1);
    let bounded = trace.finish_logging().unwrap();
    assert!(!bounded.writer_failed, "bounded finish: {bounded:?}");
    assert!(bounded.finish_attempted);
    assert_eq!(bounded.degraded, !bounded.acknowledged);
    let report = trace
        .writer_for_test()
        .observe_completion_for_test(&bounded, std::time::Duration::from_secs(5));
    assert!(report.acknowledged && !report.degraded);
    assert_eq!(report.written_events, generated.len() as u64);
    let mut saved = Vec::new();
    for n in 0..4 {
        let bytes = std::fs::read(output.join(format!("metadata/events-{n}.jsonl"))).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(SECRET));
        for line in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let row: Value = serde_json::from_slice(line).unwrap();
            assert!(
                row.get("text").is_none()
                    && row.get("owner").is_none()
                    && row.get("provider_session").is_none()
            );
            if row["kind"] != "service_segment" {
                saved.push(row);
            }
        }
    }
    assert_eq!(saved.len(), generated.len());
    assert!(saved.iter().any(|row| row["kind"] == "service_event"));
    for kind in ["input_admitted", "turn_finished"] {
        let original = generated.iter().find(|row| row["kind"] == kind).unwrap();
        let shared = saved.iter().find(|row| row["kind"] == kind).unwrap();
        assert_eq!(shared["turn"], original["turn"]);
        assert_eq!(shared["at_us"], original["at_us"]);
    }
}

/// #4265 (`LAMP_SECOND_REFUSED_HOLD=on`), as lamp-52e6 ran it on 30 Oct:
/// the second route alone, its first turn refused for its quota, so the
/// refusal starts at once and the lines heard within the minute are held,
/// never sent. A line heard past the minute is sent, the service is back
/// and answers it, and that answer ends the refusal: the line after it,
/// seconds later, is sent and answered too, not held.
#[test]
fn a_refusal_holds_lines_for_its_minute_then_a_line_is_sent_and_its_answer_ends_the_refusal() {
    let _one = super::pilot_faults::one_session();
    super::super::model_refusing::set_for_test(Some(true));
    let run = run(Script {
        second: false,
        only: true,
        second_fails: Some(Fault::QuotaExceeded),
        second_fails_first: Some(1),
        // Held (20 s), sent past the minute (75 s), then sent again (85 s).
        speak_at: &[2_000, 7_500, 8_500],
        mute_at_frame: Some(9_500),
        limit: Duration::from_secs(100),
        ..Script::question(Live::Answer(12))
    });
    super::super::model_refusing::set_for_test(None);
    run.assert_clean();
    let started: Vec<u64> = dedup(
        &run.seen
            .second
            .iter()
            .filter(|input| matches!(input, ProviderInput::Start { .. }))
            .map(request_of)
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        started.len(),
        3,
        "the refused line, the try past the minute and the line after it: {started:?}\n{}",
        run.summary()
    );
    let answered = run
        .seen
        .chunks
        .iter()
        .filter(|chunk| chunk.accepted && chunk.value == SECOND)
        .map(|chunk| chunk.turn)
        .collect::<BTreeSet<_>>();
    assert_eq!(answered.len(), 2, "{answered:?}\n{}", run.summary());
    let held = run
        .events("input_admitted")
        .into_iter()
        .filter(|event| event["local_only"] == true)
        .count();
    assert_eq!(held, 1, "the line within the minute: {}", run.summary());
}

/// #4265 as lamp-52e6 ran it on 30 Oct: connections refused, so a refusal
/// starts on the second refused turn in a row; the recorded voice service
/// line plays for the line held after it, and the person speaks over it.
/// A line heard past the minute is still taken and sent, the service is
/// back and answers it, and the line after it is sent too.
#[test]
fn a_refusal_on_refused_connections_whose_line_is_spoken_over_still_sends_the_line_past_its_minute()
{
    let _one = super::pilot_faults::one_session();
    for (turn_taking, echo_evidence) in [
        (TurnTaking::V1Baseline, false),
        (TurnTaking::VadOnly, false),
        (TurnTaking::Duplex, true),
    ] {
        super::super::model_refusing::set_for_test(Some(true));
        let run = run(Script {
            turn_taking,
            echo_evidence,
            second: false,
            only: true,
            second_fails: Some(Fault::Transport),
            second_fails_first: Some(2),
            notices: true,
            // Refused again (10 s); held, its line spoken over (20 s, 20.7 s);
            // sent past the minute (80 s), then sent again (90 s).
            speak_at: &[1_000, 2_000, 2_070, 8_000, 9_000],
            mute_at_frame: Some(10_000),
            limit: Duration::from_secs(110),
            ..Script::question(Live::Answer(12))
        });
        super::super::model_refusing::set_for_test(None);
        run.assert_clean();
        let started: Vec<u64> = dedup(
            &run.seen
                .second
                .iter()
                .filter(|input| matches!(input, ProviderInput::Start { .. }))
                .map(request_of)
                .collect::<Vec<_>>(),
        );
        let finished: Vec<_> = run
            .events("turn_finished")
            .into_iter()
            .map(|event| {
                (
                    event["turn"].clone(),
                    event["outcome"].clone(),
                    event["failure_notice"].clone(),
                )
            })
            .collect();
        assert_eq!(
            started.len(),
            4,
            "{turn_taking:?} {started:?}\n{finished:?}\n{}",
            run.summary()
        );
        let answered = run
            .seen
            .chunks
            .iter()
            .filter(|chunk| chunk.accepted && chunk.value == SECOND)
            .map(|chunk| chunk.turn)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            answered.len(),
            2,
            "{turn_taking:?} {answered:?}\n{finished:?}\n{}",
            run.summary()
        );
        assert!(
            run.seen
                .chunks
                .iter()
                .any(|chunk| chunk.accepted && chunk.value == 55),
            "the voice service line played: {finished:?}"
        );
    }
}

/// #4265 as lamp-52e6 ran it on 30 Oct (turns 13 and 17 of its trace): the
/// minute's try went to a short sound of the room, sent, its speech to
/// text hearing no words, so no chat asked the model; the person's ask a
/// few seconds after it was held for another minute, and so each ask was.
/// A line in which no words were heard tells nothing of the service: the
/// ask after it is still sent, the service answers it, and the refusal
/// ends.
#[test]
fn a_refusals_try_spent_on_a_line_with_no_words_still_sends_the_ask_after_it() {
    let _one = super::pilot_faults::one_session();
    super::super::model_refusing::set_for_test(Some(true));
    let run = run(Script {
        second: false,
        only: true,
        second_fails: Some(Fault::QuotaExceeded),
        second_fails_first: Some(1),
        // Past the minute: the room's sound, sent and heard as no words
        // (75 s), then the person's ask (80 s), then another (90 s).
        speak_at: &[7_500, 8_000, 9_000],
        second_unheard: &[2],
        mute_at_frame: Some(10_000),
        limit: Duration::from_secs(110),
        ..Script::question(Live::Answer(12))
    });
    super::super::model_refusing::set_for_test(None);
    run.assert_clean();
    let started: Vec<u64> = dedup(
        &run.seen
            .second
            .iter()
            .filter(|input| matches!(input, ProviderInput::Start { .. }))
            .map(request_of)
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        started.len(),
        4,
        "the refused line, the unheard one and both asks: {started:?}\n{}",
        run.summary()
    );
    let answered = run
        .seen
        .chunks
        .iter()
        .filter(|chunk| chunk.accepted && chunk.value == SECOND)
        .map(|chunk| chunk.turn)
        .collect::<BTreeSet<_>>();
    assert_eq!(answered.len(), 2, "{answered:?}\n{}", run.summary());
}

/// #4265 (the Reviewer-lead's P3 on #4483): past the minute, a line is
/// sent and its turn fails only slow, no answer in time. A slow turn is no
/// refusal: it tries nothing and holds nothing, so the ask a few seconds
/// after it is sent too, the service answers it, and the refusal ends.
#[test]
fn a_refusals_line_past_its_minute_that_fails_only_slow_holds_nothing_and_the_next_is_sent() {
    let _one = super::pilot_faults::one_session();
    super::super::model_refusing::set_for_test(Some(true));
    let run = run(Script {
        second: false,
        only: true,
        second_fails: Some(Fault::QuotaExceeded),
        second_fails_first: Some(1),
        // Past the minute: a line sent that fails slow (75 s), then the
        // person's ask (80 s), then another (90 s).
        speak_at: &[7_500, 8_000, 9_000],
        second_slow: &[2],
        mute_at_frame: Some(10_000),
        limit: Duration::from_secs(110),
        ..Script::question(Live::Answer(12))
    });
    super::super::model_refusing::set_for_test(None);
    run.assert_clean();
    let started: Vec<u64> = dedup(
        &run.seen
            .second
            .iter()
            .filter(|input| matches!(input, ProviderInput::Start { .. }))
            .map(request_of)
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        started.len(),
        4,
        "the refused line, the slow one and both asks: {started:?}\n{}",
        run.summary()
    );
    let answered = run
        .seen
        .chunks
        .iter()
        .filter(|chunk| chunk.accepted && chunk.value == SECOND)
        .map(|chunk| chunk.turn)
        .collect::<BTreeSet<_>>();
    assert_eq!(answered.len(), 2, "{answered:?}\n{}", run.summary());
}
