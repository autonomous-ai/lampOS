//! Duplex hearing through the production hearing, observation, decision,
//! commit and speaker-dispatch functions, one capture block at a time.
//!
//! Time here is the test's own scenario clock, and the blocks follow one
//! another a step (2 ms) apart, so every assertion is about order and counts
//! of blocks; a paced test runs 40 ms a block
//! ([`Harness::keeping_first_words_paced`]), and one that waits on the host's
//! clock runs on it ([`Harness::on_the_host`]). The bounds that are measured in
//! time are tested where they live: the decision budget in `admission`, the
//! tail and the staleness of hearing in `turn_taking`.
use super::*;
use crate::{
    activity::START_BLOCKS,
    admission::Hold,
    turn_taking::{FORGIVE_FRUITLESS_US, HEARING_STALE_US},
};
use lamp_audio::{
    duplex::Confirmation,
    evidence::{EchoEvidence, EchoMatch, Level},
    ownership::PlaybackCursor,
};

/// Blocks over the lamp that make it yield in the first second it has
/// something to say, which is where these tests take place.
const ONSET: usize = Confirmation::EVALUATED.early_onset_blocks as usize;
/// The same as a capture sequence number.
const ONSET_SEQUENCE: u64 = ONSET as u64;

/// The render explains the microphone: the lamp's own voice.
pub(super) const OWN: Option<(u8, u8)> = Some((97, 10));
/// The render does not: someone is speaking over the lamp.
pub(super) const OVER: Option<(u8, u8)> = Some((40, 0));

pub(super) fn evidence(render_quiet_blocks: u8, shares: Option<(u8, u8)>) -> EchoEvidence {
    let level = Level::from_tenths_of_db(-200);
    EchoEvidence {
        blocks: 1_000,
        render_active_blocks: if render_quiet_blocks == 0 { 12 } else { 0 },
        render_quiet_blocks,
        capture_level: level,
        residual_level: Level::from_tenths_of_db(-400),
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

/// What one capture block led to.
#[derive(Debug, Default)]
pub(super) struct Tick {
    /// Reply samples handed to the speaker after this block was heard.
    pub(super) dispatched: Option<Vec<i16>>,
    pub(super) accepted: Option<Admitted>,
}

/// An input the admission accepted and the coordinator committed.
#[derive(Debug)]
pub(super) struct Admitted {
    pub(super) basis: AcceptanceBasis,
    pub(super) trigger_sequence: u64,
    /// Capture blocks that became the start of the request, in order.
    pub(super) audio: Vec<u64>,
}

pub(super) struct Harness {
    pub(super) owner: Controller,
    pub(super) reply: Option<Reply>,
    mask: PlaybackMask,
    detector: TurnDetector,
    admission: InputAdmission,
    pub(super) trace: SessionState,
    lineage: CaptureLineage,
    heard_reply: Option<TurnOwner>,
    sequence: u64,
    last_read: u64,
    _directory: SessionDirectory,
    parent: WorkerChannels,
    speaker: WorkerChannels,
    pending: Option<PendingSpeaker>,
    chunk: u64,
    pub(super) snapshot: Snapshot,
    pub(super) provider_queue: VecDeque<ProviderInput>,
    hold: Hold,
    /// The least time between two blocks, for a paced run; and when each
    /// block was read, by its sequence.
    gap_us: u64,
    reads: Vec<u64>,
    /// A yield's words, with `LAMP_STOP_WORDS=heard` (#828).
    stop_words: Option<crate::stop_words::StopWords>,
    /// The yield's listener is told each block's score, as the session
    /// loop tells it, so `heard-always` also asks at the detector's own
    /// pause. Off, as the tests that predate it were written.
    scores_speech: bool,
    /// `LAMP_REPLY_END_REACH=on` (#1722), as the session loop reads it.
    pub(super) reach_past_reply: bool,
    /// The test's own scenario clock, unless it waits on the host's.
    _clock: Option<crate::scenario_clock::Scope>,
    /// Every candidate is let go, as the session loop lets go talk over the
    /// lamp's own line in a busy room (`busy_room`, #2602).
    let_go: bool,
    /// Whether each block is heard with the lamp silent is given to the
    /// detector, as the session loop gives it with `LAMP_SOFT_START=on`
    /// ([`lamp_silent`]).
    pub(super) soft_start: bool,
}

impl Harness {
    /// The duplex harness with a yield's first words kept, or not, as the
    /// session loop sets them up with `LAMP_FIRST_WORDS=kept` (#828).
    pub(super) fn keeping_first_words(kept: bool, blocks: usize) -> Self {
        let mut harness = Self::new(TurnTaking::Duplex, blocks);
        if kept {
            harness.detector.keep_first_words(true);
            harness.mask.keep_first_words();
            harness.hold = Hold::DUPLEX.keeping_first_words();
            harness.admission = InputAdmission::with_hold(harness.hold);
        }
        harness
    }
    /// The same on the test's own scenario clock, for a paced run
    /// ([`Self::at`]): the session loop's own reads of the clock, the turn
    /// owner's among them, then follow the paced reads and never pass them.
    /// On the host's clock a host woken late past a paced read had given
    /// the owner a later time already, and the next block's read failed
    /// with `ClockRegression` (a full serial run on 17 October). Keep the
    /// scope until the harness is gone.
    pub(super) fn keeping_first_words_paced(
        kept: bool,
        blocks: usize,
    ) -> (crate::scenario_clock::Scope, Self) {
        let clock = crate::scenario_clock::Scope::enter(Duration::from_secs(30)).unwrap();
        clock.set_pump(|_| Ok(())).unwrap();
        (clock, Self::keeping_first_words(kept, blocks))
    }
    /// A reply of `blocks` speaker blocks whose samples count up from zero,
    /// so that what reaches the speaker can be checked for order and loss.
    /// On the test's own scenario clock, unless one is entered already (a
    /// paced test's): blocks read at once on the host's could be more than
    /// 50 ms apart when the host wakes late (a loaded runner), which the
    /// detector refuses as a capture discontinuity (#1569 row 70).
    pub(super) fn new(policy: TurnTaking, blocks: usize) -> Self {
        let clock = (!crate::scenario_clock::is_active()).then(|| {
            let clock = crate::scenario_clock::Scope::enter(Duration::from_secs(30)).unwrap();
            clock.set_pump(|_| Ok(())).unwrap();
            clock
        });
        Self {
            _clock: clock,
            ..Self::on_the_host(policy, blocks)
        }
    }
    /// [`Self::new`] on the host's clock, for a test that waits on it.
    pub(super) fn on_the_host(policy: TurnTaking, blocks: usize) -> Self {
        let (mut owner, mut reply, _) = active_reply();
        reply.pcm = (0..blocks * 240).map(|n| (n % 30_000) as i16).collect();
        let directory = SessionDirectory::create().unwrap();
        let (parent, speaker) = pair(&directory, "speaker");
        let snapshot = owner.snapshot(now()).unwrap();
        let duplex = policy == TurnTaking::Duplex;
        Self {
            lineage: CaptureLineage {
                worker: new_boot().unwrap(),
                epoch: 1,
                dsp_epoch: 1,
                privacy_generation: snapshot.microphone_generation(),
            },
            owner,
            reply: Some(reply),
            mask: if duplex {
                // The lamp's own sound has been found; see
                // `the_lamps_own_first_word_...` for when it has not.
                let mut mask = PlaybackMask::duplex();
                mask.assume_matched();
                mask
            } else {
                PlaybackMask::new(policy)
            },
            detector: TurnDetector::v1_baseline(),
            admission: InputAdmission::with_hold(if duplex {
                Hold::DUPLEX
            } else {
                Hold::STANDARD
            }),
            trace: SessionState::default(),
            heard_reply: None,
            sequence: 0,
            last_read: monotonic_us(),
            _directory: directory,
            parent,
            speaker,
            pending: None,
            chunk: 0,
            snapshot,
            provider_queue: VecDeque::new(),
            hold: if duplex { Hold::DUPLEX } else { Hold::STANDARD },
            gap_us: 0,
            reads: vec![0],
            stop_words: None,
            scores_speech: false,
            reach_past_reply: false,
            _clock: None,
            let_go: false,
            soft_start: false,
        }
    }
    pub(super) fn old(&self) -> TurnOwner {
        self.reply.as_ref().unwrap().owner
    }
    pub(super) fn events(&self, kind: &str) -> Vec<Value> {
        self.trace
            .iter()
            .filter(|event| event["kind"] == kind)
            .cloned()
            .collect()
    }
    fn at(&self) -> u64 {
        // A paced run: read exactly the paced gap after the previous block,
        // once the host clock has reached it. A host late to wake (a loaded
        // runner) makes no gap longer than the run's, which the detector
        // would refuse past 50 ms: #936's paced test failed so once in a
        // full serial run on 16 October. What reads the block judges it by
        // this time, not the host's.
        // On a scenario clock ([`Self::keeping_first_words_paced`]) time
        // moves only by its own steps, 2 ms each.
        let step = || crate::scenario_clock::step(crate::scenario_clock::Phase::Run).unwrap();
        if self.gap_us > 0 {
            let at = self.last_read + self.gap_us;
            while monotonic_us() < at {
                if !step() {
                    std::thread::sleep(std::time::Duration::from_micros(500));
                }
            }
            return at;
        }
        // Otherwise strictly after the previous block's read.
        loop {
            let at = monotonic_us();
            if at > self.last_read {
                return at;
            }
            if !step() {
                std::hint::spin_loop();
            }
        }
    }
    /// One pass of the session loop for one capture block: hear it, let
    /// admission see it, decide a candidate, commit an accepted one, then
    /// offer the speaker its next chunk and let it accept that chunk.
    pub(super) fn block(&mut self, score: f32, echo: Option<EchoEvidence>) -> Tick {
        self.sequence += 1;
        let at = self.at();
        if self.gap_us > 0 || self._clock.is_some() {
            // A paced run, or one on the scenario clock (2 ms a block),
            // outlasts the half second the reply's capture and admission
            // were opened for: renewed each block, as the session loop
            // renews them each tick.
            let now = MonoTime::from_micros(at);
            let until = MonoTime::from_micros(at + 500_000);
            self.owner
                .set_capture(now, lamp_turn_owner::CaptureState::RetainingUntil(until))
                .unwrap();
            self.owner
                .set_admission(now, lamp_turn_owner::AdmissionState::OpenUntil(until))
                .unwrap();
        }
        let context = AdmissionContext {
            capture: self.lineage,
            // Issued no later than the block it judges.
            authority: self.owner.snapshot(MonoTime::from_micros(at)).unwrap(),
        };
        let (heard, yielded) = hear_capture_block(
            &mut self.mask,
            &self.reply,
            &mut self.heard_reply,
            self.sequence,
            at,
            score,
            echo.as_ref(),
            at,
            &mut self.trace,
        )
        .unwrap();
        let observation = ObservedAudio {
            sequence: self.sequence,
            captured_at_us: at,
            samples: [self.sequence as i16; 160],
        };
        self.reads.push(at);
        if yielded {
            self.detector.reach_back();
        } else if reaches_past_reply(self.reach_past_reply, echo.as_ref()) {
            self.detector.reach_back_to_onset();
        }
        if self.soft_start {
            let sounding = echo.as_ref().is_some_and(|echo| {
                echo.render_quiet_blocks < Hearing::EVALUATED.clear_after_blocks
            });
            self.detector.lamp_silent(lamp_silent(
                sounding,
                false,
                reply_sounding(&self.mask, &self.reply, at),
            ));
        }
        let samples = observation.samples;
        let (update, _) = observe_prospective_heard(
            &mut self.mask,
            &mut self.detector,
            &mut self.admission,
            context,
            observation,
            self.last_read,
            heard,
            echo.is_some().then_some(score),
            at,
        )
        .unwrap();
        self.last_read = at;
        if let Some(rejection) = update.rejection {
            record_input_rejection(&mut self.trace, rejection).unwrap();
        }
        if let AdmissionStep::Candidate(candidate) = &update.step {
            record(
                &mut self.trace,
                json!({"kind":"input_candidate","candidate":candidate,"at_us":at}),
            )
            .unwrap();
        }
        if let Some(words) = self.stop_words.as_mut() {
            if self.scores_speech {
                words.heard_speech(score.is_finite() && score >= 0.5);
            }
            hear_stop_words(
                words,
                &mut self.mask,
                &self.admission,
                &update.step,
                self.reply.is_some(),
                echo.as_ref().is_some_and(|echo| {
                    echo.render_quiet_blocks < Hearing::EVALUATED.clear_after_blocks
                }),
                at,
                &samples,
                &mut self.trace,
            )
            .unwrap();
        }
        let candidate = match update.step {
            AdmissionStep::Candidate(candidate) => Some(candidate),
            AdmissionStep::None => self
                .admission
                .pending_evidence(at, AcceptanceBasis::DirectedSessionVadOnly)
                .map(|(candidate, _)| candidate),
            AdmissionStep::Rejected(rejection) => {
                record_input_rejection(&mut self.trace, rejection).unwrap();
                None
            }
            AdmissionStep::Audio(audio) | AdmissionStep::End(audio) => {
                // The person goes on: more of the request for the provider.
                queue_input(
                    &mut self.provider_queue,
                    self.reply.as_mut().unwrap(),
                    self.snapshot.microphone_generation(),
                    audio,
                    provider_queue_bound(self.hold),
                )
                .unwrap();
                None
            }
        };
        let mut tick = Tick::default();
        let candidate = candidate.filter(|_| {
            if self.let_go {
                self.admission.release_prospective(at).unwrap();
                self.detector.clear_prospective();
            }
            !self.let_go
        });
        if let Some(candidate) = candidate
            && let Some(accepted) = decide_prospective_input(
                &self.mask,
                &mut self.detector,
                &mut self.admission,
                candidate,
                context,
                at,
                &mut self.trace,
            )
            .unwrap()
        {
            tick.accepted = Some(Admitted {
                basis: accepted.basis,
                trigger_sequence: accepted.candidate.trigger_sequence,
                audio: accepted.audio.iter().map(|frame| frame.sequence).collect(),
            });
            self.snapshot = commit_admitted_input(
                &mut self.owner,
                &mut self.reply,
                &mut self.pending,
                &mut self.provider_queue,
                accepted,
                1,
                false,
                false,
                false,
                None,
                None,
                false,
                false,
                false,
                self.detector.began_ordinarily(),
                PlayoutStart::default(),
                &mut self.trace,
                |_| Ok(()),
            )
            .unwrap();
        }
        if self.gap_us > 0 || self._clock.is_some() {
            // A paced run, or one on the scenario clock, lasts longer than
            // the snapshot taken at the start stays fresh: the speaker is
            // offered the one of this block.
            self.snapshot = self.owner.snapshot(MonoTime::from_micros(at)).unwrap();
        }
        tick.dispatched = self.offer_speaker();
        tick
    }
    /// The loop's dispatch call, and a speaker that accepts what it is sent.
    fn offer_speaker(&mut self) -> Option<Vec<i16>> {
        settle_duplex_yield_before_dispatch(
            &self.mask,
            &self.reply,
            monotonic_us(),
            &mut self.trace,
        )
        .unwrap();
        dispatch_speaker_chunk(
            &mut self.owner,
            &mut self.reply,
            &mut self.parent.data,
            &mut self.pending,
            &mut self.chunk,
            &mut self.snapshot,
            &mut self.mask,
        )
        .unwrap();
        let chunk = self.speaker.data.receive::<SpeakerChunk>().unwrap()?;
        let pending = self.pending.take().unwrap();
        assert_eq!(pending.sequence, chunk.chunk_sequence);
        let frame = 480 + 240 * (chunk.chunk_sequence - 1);
        let at = monotonic_us();
        self.mask
            .accepted(
                pending.permit.owner(),
                chunk.chunk_sequence,
                PlaybackCursor { epoch: 1, frame },
                PlaybackCursor {
                    epoch: 1,
                    frame: frame + 240,
                },
                240,
                at,
                at,
                at,
            )
            .unwrap();
        Some(chunk.samples)
    }
    /// `count` blocks alike. Returns what each led to.
    pub(super) fn blocks(
        &mut self,
        count: usize,
        score: f32,
        echo: Option<EchoEvidence>,
    ) -> Vec<Tick> {
        (0..count).map(|_| self.block(score, echo)).collect()
    }
}

fn dispatched(ticks: &[Tick]) -> usize {
    ticks
        .iter()
        .filter(|tick| tick.dispatched.is_some())
        .count()
}

#[test]
fn the_lamps_own_voice_through_a_whole_reply_neither_yields_nor_becomes_a_turn() {
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    let old = h.old();
    // The canceller lets the lamp through and the detector is sure of speech.
    let ticks = h.blocks(300, 0.99, Some(evidence(0, OWN)));
    assert_eq!(dispatched(&ticks), 300, "the speaker was kept waiting");
    assert!(ticks.iter().all(|tick| tick.accepted.is_none()));
    assert_eq!(h.old(), old);
    assert!(h.events("input_candidate").is_empty());
    assert!(h.events("duplex_yield").is_empty());
    assert_eq!(h.mask.duplex_summary(), Some(((0, 0, 0), false)));
    assert_eq!(h.mask.suppressed_blocks(), 0, "heard, not masked");
}

/// `LAMP_OWN_LINE_OVER_TALK` (#2602; lamp-52e6 on 22 Oct): talk over the
/// lamp's own line in a busy room is let go, and the line doesn't pause for
/// it either. There every candidate of the talk was let go, but each yield
/// had withheld the line's audio, the speaker played zeros, and the say
/// ended `dropped` (`audio_written_with_playback_gaps_unscored`). With the
/// mask's `room_talk`, every block of a long run of talk over the line is
/// handed to the speaker, in order, with no yield: no gap, so the say is
/// `spoken`. The talk is still candidates, for the name or a face turned to
/// the lamp to be judged in: one not let go takes the turn as before, and a
/// stop heard by its own listener still yields. Without it, as before.
#[test]
fn talk_over_the_lamps_own_line_in_a_busy_room_leaves_no_playback_gap() {
    for room_talk in [false, true] {
        let mut h = Harness::new(TurnTaking::Duplex, 400);
        h.let_go = true;
        h.mask.room_talk(room_talk);
        let old = h.old();
        assert_eq!(dispatched(&h.blocks(20, 0.99, Some(evidence(0, OWN)))), 20);
        let talk = h.blocks(200, 0.9, Some(evidence(0, OVER)));
        assert!(!h.events("input_candidate").is_empty(), "{room_talk}");
        assert_eq!(h.old(), old, "{room_talk}");
        assert!(h.events("turn_finished").is_empty(), "{room_talk}");
        if room_talk {
            assert_eq!(dispatched(&talk), 200, "the line paused for the talk");
            assert!(h.events("duplex_yield").is_empty());
            let firsts: Vec<i16> = talk
                .iter()
                .map(|tick| tick.dispatched.as_ref().unwrap()[0])
                .collect();
            // The reply's samples count up from zero, modulo 30 000.
            assert!(
                firsts
                    .windows(2)
                    .all(|pair| i32::from(pair[1]) == (i32::from(pair[0]) + 240) % 30_000)
            );
        } else {
            // Today's: each yield withholds the line from the speaker.
            assert!(dispatched(&talk) < 200);
            assert!(!h.events("duplex_yield").is_empty());
        }
    }
    // A candidate not let go (the lamp's name in it) takes the turn.
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    h.mask.room_talk(true);
    let old = h.old();
    h.blocks(20, 0.99, Some(evidence(0, OWN)));
    let talk = h.blocks(40, 0.9, Some(evidence(0, OVER)));
    assert!(talk.iter().any(|tick| tick.accepted.is_some()));
    assert_ne!(h.old(), old);
    assert_eq!(h.events("turn_finished")[0]["outcome"], "user_interrupted");
    // A stop heard by its own listener still yields.
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    h.let_go = true;
    h.mask.room_talk(true);
    h.blocks(20, 0.99, Some(evidence(0, OWN)));
    assert!(h.mask.force_yield());
    assert!(h.block(0.9, Some(evidence(0, OVER))).dispatched.is_none());
    assert_eq!(h.events("duplex_yield").len(), 1);
}

#[test]
fn a_person_over_the_lamp_stops_the_speaker_then_interrupts_with_their_opening_words() {
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    let old = h.old();
    assert_eq!(dispatched(&h.blocks(20, 0.99, Some(evidence(0, OWN)))), 20);
    // From block 21 someone speaks over the lamp.
    let onset = h.blocks(ONSET, 0.9, Some(evidence(0, OVER)));
    assert_eq!(
        dispatched(&onset[..ONSET - 1]),
        ONSET - 1,
        "one block is not a person"
    );
    assert!(
        onset[ONSET - 1].dispatched.is_none(),
        "the block that established the person still fed the speaker"
    );
    let yielded_at = 20 + ONSET_SEQUENCE;
    let yielded = h.events("duplex_yield");
    assert_eq!(yielded.len(), 1);
    assert_eq!(yielded[0]["frame_sequence"], yielded_at);
    assert_eq!(yielded[0]["owner"], serde_json::to_value(old).unwrap());
    assert_eq!(h.events("input_candidate").len(), 1);
    // What the speaker already has plays out; the lamp's sound dies away
    // while the person goes on. Nothing more is handed over, nothing is
    // cancelled yet, and the reply keeps every sample it had.
    let queued = h.reply.as_ref().unwrap().pcm.len();
    let mut held = h.blocks(3, 0.9, Some(evidence(0, OVER)));
    for quiet in 1..=29 {
        held.push(h.block(0.9, Some(evidence(quiet, OVER))));
    }
    assert_eq!(dispatched(&held), 0);
    assert!(held.iter().all(|tick| tick.accepted.is_none()));
    assert_eq!(h.old(), old);
    assert_eq!(h.reply.as_ref().unwrap().pcm.len(), queued);
    assert!(h.events("turn_finished").is_empty());
    // The lamp is silent and the person is still speaking: an interruption.
    let clear = h.blocks(6, 0.9, Some(evidence(30, None)));
    assert!(clear[..5].iter().all(|tick| tick.accepted.is_none()));
    let accepted = clear[5]
        .accepted
        .as_ref()
        .expect("the person was not let in");
    assert_eq!(accepted.basis, AcceptanceBasis::DirectedSessionDuplex);
    let committed = h.events("duplex_commit");
    assert_eq!(committed.len(), 1);
    let committed_at = yielded_at + 38;
    assert_eq!(committed[0]["frame_sequence"], committed_at);
    // The opening words, spoken over the lamp, are the start of the request:
    // one unbroken run from before the first block of the person to now.
    let heard = &accepted.audio;
    assert!(heard[0] <= 21 && *heard.last().unwrap() == committed_at);
    assert!(heard.windows(2).all(|pair| pair[1] == pair[0] + 1));
    assert_eq!(
        accepted.trigger_sequence, yielded_at,
        "the same candidate, not a later one"
    );
    // The old reply went through the ordinary cancellation and a new request
    // began with that audio.
    let finished = h.events("turn_finished");
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0]["outcome"], "user_interrupted");
    assert_eq!(finished[0]["owner"], serde_json::to_value(old).unwrap());
    assert_ne!(h.old(), old);
    assert!(matches!(
        h.provider_queue.front(),
        Some(ProviderInput::Start { .. })
    ));
    // Nothing of the old reply can reach the speaker now.
    assert!(h.offer_speaker().is_none());
    assert_eq!(h.mask.duplex_summary(), Some(((1, 1, 0), false)));
    // The request is the start and every retained block. That is more than
    // the standing queue bound allows, which is why duplex widens it, and the
    // person's next words still fit behind it.
    assert_eq!(h.provider_queue.len(), 1 + heard.len());
    assert!(h.provider_queue.len() > PROVIDER_QUEUE_BOUND);
    let more = h.blocks(8, 0.9, Some(evidence(40, None)));
    assert!(more.iter().all(|tick| tick.accepted.is_none()));
    assert_eq!(h.provider_queue.len(), 1 + heard.len() + 8);
    assert!(h.provider_queue.len() < provider_queue_bound(Hold::DUPLEX));
    assert_eq!(provider_queue_bound(Hold::STANDARD), PROVIDER_QUEUE_BOUND);
    assert_eq!(h.events("turn_finished").len(), 1, "one interruption");
}

/// #936, the reviewer at 08:02, paced on the scenario clock, 40 ms a block:
/// first words said 2.5 s before the commit are older than the provider
/// takes (2.5 s). The commit drops the oldest of them from the front, keeps
/// every block it sends within `FIRST_WORDS_AGE_US`, keeps the onset and what
/// followed, and says what it dropped. Without first words kept, as before.
#[test]
fn first_words_older_than_the_provider_takes_are_dropped_before_the_commit() {
    let (_clock, mut h) = Harness::keeping_first_words_paced(true, 400);
    h.blocks(30, 0.1, Some(evidence(0, OWN)));
    // Blocks 31 to 92, 40 ms apart (the detector takes no gap over 50 ms):
    // first words under the lamp, then a breath, 2.5 s in all.
    h.gap_us = 40_000;
    h.blocks(52, 0.9, Some(evidence(0, OWN)));
    h.blocks(10, 0.1, Some(evidence(0, OWN)));
    h.gap_us = 0;
    while h.events("duplex_yield").is_empty() {
        h.block(0.9, Some(evidence(0, OVER)));
    }
    h.blocks(3, 0.9, Some(evidence(0, OVER)));
    for quiet in 1..=29 {
        h.block(0.9, Some(evidence(quiet, OVER)));
    }
    let clear = h.blocks(6, 0.9, Some(evidence(30, None)));
    let committed_at = h.last_read;
    let accepted = clear[5]
        .accepted
        .as_ref()
        .expect("the person was not let in");
    let oldest = h.reads[accepted.audio[0] as usize];
    assert!(
        committed_at - oldest <= crate::admission::FIRST_WORDS_AGE_US,
        "{} us old",
        committed_at - oldest
    );
    // Some of the first words went; the rest, the onset and after did not.
    assert!(accepted.audio[0] > 31 - crate::activity::REACH_LEAD_BLOCKS as u64);
    assert!(accepted.audio[0] <= 92);
    assert!(accepted.audio.windows(2).all(|pair| pair[1] == pair[0] + 1));
    assert_eq!(*accepted.audio.last().unwrap(), committed_sequence(&h));
    let dropped = h.events("input_prefix_sanitized");
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0]["reason"], "first_words_too_old");
    assert_eq!(h.events("turn_finished")[0]["outcome"], "user_interrupted");
}

fn committed_sequence(h: &Harness) -> u64 {
    h.events("duplex_commit")[0]["frame_sequence"]
        .as_u64()
        .unwrap()
}

/// #828, step 2: with `LAMP_FIRST_WORDS=kept`, a person who begins under
/// the lamp's voice keeps their first words. The duplex rule takes them for
/// the lamp's own sound until they speak plainly over it, and without the
/// switch the request begins 300 ms before that onset, as "Lamp, stop.
/// What is two plus two?" was sent as its tail on lamp-4ace (16 October).
/// With it, the request begins 200 ms before the earliest speech-like block
/// of the last 1.6 s, and the decision is as before.
#[test]
fn with_first_words_kept_a_yields_request_begins_at_the_words_said_under_the_lamp() {
    let request = |kept: bool| {
        let mut h = Harness::keeping_first_words(kept, 400);
        // The lamp alone, its own sound scored low by the detector.
        h.blocks(100, 0.1, Some(evidence(0, OWN)));
        // Blocks 101 to 140: the person's first words, under the lamp's
        // voice, which the rule hears as the lamp's own.
        h.blocks(40, 0.9, Some(evidence(0, OWN)));
        // A breath, then plainly over the lamp until it yields.
        h.blocks(10, 0.1, Some(evidence(0, OWN)));
        while h.events("duplex_yield").is_empty() {
            h.block(0.9, Some(evidence(0, OVER)));
        }
        let yielded_at = h.events("duplex_yield")[0]["frame_sequence"]
            .as_u64()
            .unwrap();
        // The lamp falls silent while the person goes on: an interruption.
        h.blocks(3, 0.9, Some(evidence(0, OVER)));
        for quiet in 1..=29 {
            h.block(0.9, Some(evidence(quiet, OVER)));
        }
        let clear = h.blocks(6, 0.9, Some(evidence(30, None)));
        let accepted = clear[5]
            .accepted
            .as_ref()
            .expect("the person was not let in");
        assert_eq!(accepted.trigger_sequence, yielded_at);
        assert!(accepted.audio.windows(2).all(|pair| pair[1] == pair[0] + 1));
        assert_eq!(h.events("turn_finished")[0]["outcome"], "user_interrupted");
        // The whole request is queued for the provider, within its bound.
        assert_eq!(h.provider_queue.len(), 1 + accepted.audio.len());
        assert!(h.provider_queue.len() < provider_queue_bound(h.hold));
        (accepted.audio.clone(), yielded_at)
    };
    let (kept, yielded_at) = request(true);
    assert_eq!(kept[0], 101 - crate::activity::REACH_LEAD_BLOCKS as u64);
    let (usual, usual_yield) = request(false);
    assert_eq!(usual_yield, yielded_at);
    assert_eq!(
        usual[0],
        yielded_at + 1 - crate::activity::PRE_ROLL_BLOCKS as u64
    );
    assert_eq!(usual.last(), kept.last());
}

/// #1722 (the first builder at 06:56): "Hey Lamp, ask Codex to write a haiku
/// about coffee." begun as the lamp's story ends. The duplex rule takes the
/// name, said under the lamp's last words, for the lamp's own sound, and the
/// line starts only once the lamp has been quiet 300 ms, with the 300 ms
/// before that: on lamp-4ace its request heard 8 of the 10 words, held for no
/// name, in 3 of 4 hand-overs. With `LAMP_REPLY_END_REACH=on` the request
/// begins 200 ms before the onset its name made. The lamp's own last words,
/// left by the canceller under the onset's bar, aren't reached back to, even
/// scored where a yield's reach would take them.
#[test]
fn with_the_reply_end_reach_a_line_begun_under_the_lamps_last_words_keeps_its_name() {
    let request = |on: bool, residue: f32| {
        let mut h = Harness::keeping_first_words(true, 400);
        h.reach_past_reply = on;
        // Blocks 1 to 100: the lamp's last words, as the canceller leaves
        // them to the detector.
        h.blocks(100, residue, Some(evidence(0, OWN)));
        // Blocks 101 to 140: "Hey Lamp," under the lamp's last sound.
        h.blocks(40, 0.9, Some(evidence(0, OWN)));
        // The lamp falls quiet while the person goes on, and is let start
        // once it has been quiet 300 ms.
        for quiet in 1..=29 {
            h.block(0.9, Some(evidence(quiet, None)));
        }
        let clear = h.blocks(6, 0.9, Some(evidence(30, None)));
        let accepted = clear[5]
            .accepted
            .as_ref()
            .expect("the person was not let in");
        assert!(accepted.audio.windows(2).all(|pair| pair[1] == pair[0] + 1));
        assert_eq!(h.provider_queue.len(), 1 + accepted.audio.len());
        assert!(h.provider_queue.len() < provider_queue_bound(h.hold));
        assert!(
            h.events("duplex_yield").is_empty(),
            "no yield: the name was taken for the lamp's own sound"
        );
        (accepted.audio.clone(), accepted.trigger_sequence)
    };
    let lead = crate::activity::REACH_LEAD_BLOCKS as u64;
    let (reached, trigger) = request(true, 0.1);
    assert_eq!(reached[0], 101 - lead, "200 ms before the name's onset");
    let (usual, usual_trigger) = request(false, 0.1);
    assert_eq!(usual_trigger, trigger);
    assert_eq!(
        usual[0],
        trigger + 1 - crate::activity::PRE_ROLL_BLOCKS as u64,
        "without the switch, after the lamp's sound, as before"
    );
    assert_eq!(usual.last(), reached.last());
    // The lamp's last words scored 0.6: over a yield's bar (0.35), under an
    // onset's (0.8). The line still begins at the name, not in them.
    let (residue, _) = request(true, 0.6);
    assert_eq!(residue[0], 101 - lead);
}

/// The reviewer's P3 on #2096: the canceller leaves the lamp's own last words
/// scored as an onset now and then (#828's replay). Before a pause as long as
/// the one that ends a line (600 ms, as on lamp-4ace), they aren't this
/// line's, and it begins at its name, not in them.
#[test]
fn with_the_reply_end_reach_the_lamps_words_before_a_pause_are_not_taken() {
    let mut h = Harness::keeping_first_words(true, 400);
    h.reach_past_reply = true;
    let detector = std::mem::take(&mut h.detector);
    h.detector = detector.with_end_silence(60).unwrap();
    // Blocks 1 to 30: the lamp's last words, left scored as an onset.
    h.blocks(30, 0.9, Some(evidence(0, OWN)));
    // Blocks 31 to 91: a pause in them, 610 ms.
    h.blocks(61, 0.1, Some(evidence(0, OWN)));
    // Blocks 92 to 131: "Hey Lamp," under the lamp's last sound.
    h.blocks(40, 0.9, Some(evidence(0, OWN)));
    for quiet in 1..=29 {
        h.block(0.9, Some(evidence(quiet, None)));
    }
    let clear = h.blocks(6, 0.9, Some(evidence(30, None)));
    let accepted = clear[5]
        .accepted
        .as_ref()
        .expect("the person was not let in");
    assert!(
        accepted.audio.first().is_some_and(|first| *first > 30),
        "the lamp's words were taken: {:?}",
        accepted.audio.first()
    );
    assert_eq!(
        accepted.audio[0],
        92 - crate::activity::REACH_LEAD_BLOCKS as u64
    );
}

/// #1722: a line begun 640 ms or more after the lamp's sound ends starts as
/// before with the switch on, though words said under that sound are still
/// in the pre-roll: only a line begun as the lamp's sound ends reaches back.
#[test]
fn with_the_reply_end_reach_a_line_long_after_the_lamps_sound_starts_as_before() {
    let request = |on: bool| {
        let mut h = Harness::keeping_first_words(true, 100);
        h.reach_past_reply = on;
        h.blocks(50, 0.1, Some(evidence(0, OWN)));
        // Blocks 51 to 80: something said under the lamp's last sound.
        h.blocks(30, 0.9, Some(evidence(0, OWN)));
        // The lamp falls quiet, and the room with it, for 640 ms.
        for quiet in 1..=63 {
            h.block(0.1, Some(evidence(quiet, None)));
        }
        let line = h.blocks(6, 0.9, Some(evidence(64, None)));
        let accepted = line[5].accepted.as_ref().expect("the line was not let in");
        (accepted.audio.clone(), accepted.trigger_sequence)
    };
    let (on, trigger) = request(true);
    let (off, _) = request(false);
    assert_eq!(on, off);
    assert_eq!(on[0], trigger + 1 - crate::activity::PRE_ROLL_BLOCKS as u64);
}

/// The reviewer's P2 on #3392: with `LAMP_SOFT_START=on`, a line begun while a
/// reply's playback is still held but its sound has been quiet 640 ms (not
/// retired) has every block marked as heard with the lamp's reply playing,
/// the onset's own among them. It starts as without the switch, with its
/// audio, and is let in; it never starts with none, which admission refuses
/// as an invalid prefix and which ended the conversation.
#[test]
fn with_soft_starts_a_line_under_a_quiet_unretired_reply_starts_as_before() {
    let request = |soft: bool| {
        let mut h = Harness::keeping_first_words(true, 100);
        h.soft_start = soft;
        h.detector.keep_soft_starts(soft);
        h.blocks(50, 0.1, Some(evidence(0, OWN)));
        // The lamp falls quiet, its playback still held, for 640 ms.
        for quiet in 1..=63 {
            h.block(0.1, Some(evidence(quiet, None)));
        }
        let line = h.blocks(6, 0.95, Some(evidence(64, None)));
        let accepted = line[5].accepted.as_ref().expect("the line was not let in");
        assert!(!accepted.audio.is_empty());
        assert!(accepted.audio.windows(2).all(|pair| pair[1] == pair[0] + 1));
        (accepted.audio.clone(), accepted.trigger_sequence)
    };
    let (on, trigger) = request(true);
    let (off, _) = request(false);
    assert_eq!(on, off);
    assert_eq!(on[0], trigger + 1 - crate::activity::PRE_ROLL_BLOCKS as u64);
}

/// With `LAMP_SOFT_START=on`, a line begun once the lamp's answer is all
/// handed over and its sound quiet a second, though its turn is not retired
/// and its playback still held, keeps 300 ms more than the usual 300 ms (on
/// lamp-4ace on 25 October the playback token alone kept every such line
/// from it). While any of the answer is still to hand over, as above, it
/// starts as before; and never reaches into the 300 ms after the lamp's
/// sound, which the echo evidence still hears as it.
#[test]
fn with_soft_starts_a_line_after_the_lamps_answer_keeps_its_300_ms() {
    let request = |soft: bool| {
        let mut h = Harness::keeping_first_words(true, 100);
        h.soft_start = soft;
        h.detector.keep_soft_starts(soft);
        h.reply.as_mut().unwrap().pcm.clear();
        h.blocks(50, 0.1, Some(evidence(0, OWN)));
        for quiet in 1..=100 {
            h.block(0.1, Some(evidence(quiet, None)));
        }
        let line = h.blocks(6, 0.95, Some(evidence(101, None)));
        let accepted = line[5].accepted.as_ref().expect("the line was not let in");
        assert!(accepted.audio.windows(2).all(|pair| pair[1] == pair[0] + 1));
        (accepted.audio.clone(), accepted.trigger_sequence)
    };
    let (on, trigger) = request(true);
    let (off, _) = request(false);
    let usual = trigger + 1 - crate::activity::PRE_ROLL_BLOCKS as u64;
    assert_eq!(off[0], usual);
    assert_eq!(
        on[0],
        usual - crate::activity::SOFT_START_LEAD_BLOCKS as u64
    );
    assert_eq!(on.last(), off.last());
}

/// #4226: the Start of a line begun with a soft start says how long its
/// sound began before its usual start, by when each block was read, for the
/// second route to date the name at the line's start from; without soft
/// starts, and for a yield's start reaching back, none.
#[test]
fn a_soft_starts_lead_goes_with_its_start() {
    let start = |soft: bool, yielded: bool| {
        let mut h = Harness::keeping_first_words(true, 100);
        h.soft_start = soft;
        h.detector.keep_soft_starts(soft);
        h.reply.as_mut().unwrap().pcm.clear();
        h.blocks(50, 0.1, Some(evidence(0, OWN)));
        for quiet in 1..=100 {
            h.block(0.1, Some(evidence(quiet, None)));
        }
        let mut line = h.blocks(5, 0.95, Some(evidence(101, None)));
        // A yield's start reaches back, taken for the block that starts it.
        if yielded {
            h.detector.reach_back();
        }
        line.push(h.block(0.95, Some(evidence(101, None))));
        assert_eq!(h.detector.began_ordinarily(), !yielded);
        let accepted = line[5].accepted.as_ref().expect("the line was not let in");
        let lead = h
            .provider_queue
            .iter()
            .find_map(|input| match input {
                ProviderInput::Start { soft_lead_us, .. } => Some(*soft_lead_us),
                _ => None,
            })
            .expect("the line's Start is queued");
        let read = |sequence: u64| h.reads[sequence as usize];
        let usual = accepted.trigger_sequence + 1 - crate::activity::PRE_ROLL_BLOCKS as u64;
        (lead, read(usual) - read(accepted.audio[0]))
    };
    let (lead, kept) = start(true, false);
    assert!(kept > 0);
    assert_eq!(lead, kept);
    assert_eq!(start(false, false).0, 0);
    assert_eq!(start(true, true).0, 0);
}

/// [`soft_lead_us`]: from the line's first block to its usual start, at
/// most [`crate::activity::SOFT_START_LEAD_BLOCKS`] blocks' worth; none
/// without the usual start's block, or for a start that was not ordinary.
#[test]
fn a_soft_lead_is_counted_only_for_an_ordinary_start() {
    let frames = |first: u64, count: u64| -> Vec<ObservedAudio> {
        (first..first + count)
            .map(|sequence| ObservedAudio {
                sequence,
                captured_at_us: sequence * 10_000,
                samples: [0; 160],
            })
            .collect()
    };
    let pre_roll = crate::activity::PRE_ROLL_BLOCKS as u64;
    let lead = crate::activity::SOFT_START_LEAD_BLOCKS as u64;
    // An ordinary soft start: 30 blocks more than the usual 30.
    let soft = frames(100, pre_roll + lead);
    let trigger = soft.last().unwrap().sequence;
    assert_eq!(soft_lead_us(&soft, trigger, true), lead * 10_000);
    assert_eq!(soft_lead_us(&soft, trigger, false), 0);
    // The usual start alone: none.
    let usual = frames(130, pre_roll);
    assert_eq!(soft_lead_us(&usual, trigger, true), 0);
    // The usual start's block missing: none.
    assert_eq!(soft_lead_us(&soft[..10], trigger, true), 0);
    // More than a soft start keeps: no more than its lead counted.
    let longer = frames(50, 80 + pre_roll);
    let trigger = longer.last().unwrap().sequence;
    assert_eq!(soft_lead_us(&longer, trigger, true), lead * 10_000);
}

/// #1722: only a start as the lamp's sound ends reaches back: with the
/// switch, quiet under 640 ms, and echo evidence to say so.
#[test]
fn a_start_reaches_past_the_reply_only_as_the_lamps_sound_ends() {
    use lamp_audio::evidence::QUIET_BLOCKS_CEILING;
    assert!(reaches_past_reply(true, Some(&evidence(30, None))));
    assert!(reaches_past_reply(
        true,
        Some(&evidence(QUIET_BLOCKS_CEILING - 1, None))
    ));
    assert!(!reaches_past_reply(
        true,
        Some(&evidence(QUIET_BLOCKS_CEILING, None))
    ));
    assert!(!reaches_past_reply(true, None));
    assert!(!reaches_past_reply(false, Some(&evidence(30, None))));
}

/// The words a yield gives back, as the relay does: only once they are
/// asked for, the same in each yield. How many blocks they were given.
struct Ears {
    words: Option<&'static str>,
    asked: bool,
    heard: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// Blocks after the ask before the words come: the relay's answer.
    wait: usize,
}

impl crate::stop_words::Ears for Ears {
    fn ready(&mut self) {}
    fn hear(&mut self, _: u64, _: &[i16]) {
        self.heard
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn finalize(&mut self) {
        self.asked = true;
    }
    fn words(&mut self) -> Option<String> {
        if self.asked && self.wait > 0 {
            self.wait -= 1;
            return None;
        }
        self.asked
            .then_some(self.words)
            .flatten()
            .map(str::to_owned)
    }
    fn forget(&mut self) {
        self.asked = false;
    }
    fn rest(&mut self) {}
}

/// How a short word over the lamp ends, as far as sound can tell.
#[derive(Clone, Copy, Debug)]
enum Word {
    /// Over the lamp, not plainly, and then quiet: stillness resumes it.
    Over,
    /// Heard plainly, and then quiet: the quiet commits it.
    Plain,
    /// Over the lamp, then 250 ms of speech in the clear once its sound is
    /// gone: the speech commits it at its sixth block.
    Speech,
    /// The same with 500 ms of speech in the clear: a backchannel as played
    /// on lamp-52e6 on 23 Oct (#2939), some 0.5 s of sound.
    Long,
}

/// #828, step 2, through the production hearing, yield and commit, with
/// `LAMP_STOP_WORDS=heard`: a short word over the lamp yields and is over
/// before the lamp is silent, as on lamp-4ace at 05:39. Its words a stop
/// phrase, the yield commits and the reply ends, with the word as the new
/// request. Other words resume it as stillness did before. No words resume
/// it at the deadline. The block it settled at, how many blocks the words
/// were given, and whether it committed.
fn a_short_word_with(words: Option<&'static str>) -> (u64, usize, bool) {
    let (at, heard, committed, _) = a_short_word(words, Word::Over, false, false);
    (at, heard, committed)
}

/// [`a_short_word_with`], ending as `word` says, with
/// `LAMP_BACKCHANNEL_HOLDS=on` as `holds` says (#2939) and the
/// speech-to-text told Vietnamese or not. Also the commit or resume
/// recorded.
fn a_short_word(
    words: Option<&'static str>,
    word: Word,
    holds: bool,
    vietnamese: bool,
) -> (u64, usize, bool, Value) {
    a_short_word_after("", 0, words, word, holds, vietnamese)
}

/// [`a_short_word`] with the lamp having just `said` that, and its words
/// coming `wait` blocks after they are asked for.
fn a_short_word_after(
    said: &str,
    wait: usize,
    words: Option<&'static str>,
    word: Word,
    holds: bool,
    vietnamese: bool,
) -> (u64, usize, bool, Value) {
    let heard = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let stop_words = crate::stop_words::StopWords::new(Box::new(Ears {
        words,
        asked: false,
        heard: std::sync::Arc::clone(&heard),
        wait,
    }))
    .with_language(vietnamese.then_some("vi"));
    let (at, committed, settled) = a_short_word_heard_by(stop_words, said, word, holds);
    (
        at,
        heard.load(std::sync::atomic::Ordering::Relaxed),
        committed,
        settled,
    )
}

/// [`a_short_word_after`] with its words heard by `stop_words`.
fn a_short_word_heard_by(
    stop_words: crate::stop_words::StopWords,
    said: &str,
    word: Word,
    holds: bool,
) -> (u64, bool, Value) {
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    h.stop_words = Some(stop_words);
    if holds {
        h.mask.hold_backchannels();
    }
    let old = h.old();
    if !said.is_empty() {
        let words = h.stop_words.as_mut().unwrap();
        words.lamp_said(said, monotonic_us(), false);
    }
    h.blocks(20, 0.99, Some(evidence(0, OWN)));
    // "Stop.", 120 ms over the lamp.
    let shares = match word {
        Word::Plain => Some((10, 0)),
        Word::Over | Word::Speech | Word::Long => OVER,
    };
    h.blocks(ONSET + 2, 0.9, Some(evidence(0, shares)));
    let yielded_at = h.events("duplex_yield")[0]["frame_sequence"]
        .as_u64()
        .unwrap();
    for quiet in 1..=29 {
        h.block(0.1, Some(evidence(quiet, None)));
    }
    match word {
        Word::Speech => h.blocks(25, 0.9, Some(evidence(40, None))),
        Word::Long => h.blocks(50, 0.9, Some(evidence(40, None))),
        Word::Over | Word::Plain => Vec::new(),
    };
    let mut settled = None;
    for _ in 0..150 {
        h.block(0.1, Some(evidence(40, None)));
        settled = h
            .events("duplex_commit")
            .into_iter()
            .chain(h.events("duplex_resume"))
            .next();
        if settled.is_some() {
            break;
        }
    }
    let settled = settled.expect("the yield never settled");
    let committed = !h.events("duplex_commit").is_empty();
    if committed {
        assert_eq!(h.events("turn_finished")[0]["outcome"], "user_interrupted");
        assert_ne!(h.old(), old);
    } else {
        assert_eq!(h.old(), old);
        assert!(h.events("turn_finished").is_empty());
    }
    (
        settled["frame_sequence"].as_u64().unwrap() - yielded_at,
        committed,
        settled,
    )
}

/// What `LAMP_STOP_WORDS=heard-always` hears (#828): one stream for the
/// whole reply, never forgotten; each pause asks for its words with a
/// flush, and they come once, at the flush.
struct AlwaysEars {
    words: Option<&'static str>,
    flushed: bool,
}

impl crate::stop_words::Ears for AlwaysEars {
    fn ready(&mut self) {}
    fn hear(&mut self, _: u64, _: &[i16]) {}
    fn finalize(&mut self) {}
    fn flush(&mut self) {
        self.flushed = true;
    }
    fn words(&mut self) -> Option<String> {
        std::mem::take(&mut self.flushed)
            .then_some(self.words)
            .flatten()
            .map(str::to_owned)
    }
    fn forget(&mut self) {
        panic!("a stream heard always is never forgotten in a yield");
    }
    fn rest(&mut self) {}
}

/// A short word over the lamp, heard with `heard-always`, and
/// `LAMP_BACKCHANNEL_HOLDS=on` as `holds` says.
fn a_short_word_heard_always(words: &'static str, word: Word, holds: bool) -> (u64, bool, Value) {
    let stop_words = crate::stop_words::StopWords::always_counted(
        Box::new(AlwaysEars {
            words: Some(words),
            flushed: false,
        }),
        None,
    )
    .with_backchannel_holds(holds);
    a_short_word_heard_by(stop_words, "", word, holds)
}

/// `heard-always` ears whose flush gives several results at once, in order,
/// each with whether it may decide the yield ([`crate::stop_words::Said`]).
struct ResultsEars {
    results: Vec<(&'static str, bool)>,
    flushed: bool,
}

impl crate::stop_words::Ears for ResultsEars {
    fn ready(&mut self) {}
    fn hear(&mut self, _: u64, _: &[i16]) {}
    fn finalize(&mut self) {}
    fn flush(&mut self) {
        self.flushed = true;
    }
    fn words(&mut self) -> Option<String> {
        self.result().map(|said| said.words)
    }
    fn result(&mut self) -> Option<crate::stop_words::Said> {
        if !self.flushed || self.results.is_empty() {
            return None;
        }
        let (words, decides) = self.results.remove(0);
        Some(crate::stop_words::Said {
            words: words.into(),
            decides,
        })
    }
    fn forget(&mut self) {
        panic!("a stream heard always is never forgotten in a yield");
    }
    fn rest(&mut self) {}
}

/// A short word over the lamp heard with `heard-always` as `results`, all
/// in one drain, the switch as `holds` says.
fn results_heard_always(
    results: &[(&'static str, bool)],
    word: Word,
    holds: bool,
) -> (u64, bool, Value) {
    let stop_words = crate::stop_words::StopWords::always_counted(
        Box::new(ResultsEars {
            results: results.to_vec(),
            flushed: false,
        }),
        None,
    )
    .with_backchannel_holds(holds);
    a_short_word_heard_by(stop_words, "", word, holds)
}

/// reviewer-two's P2 on #3200: with `heard-always` the first result no
/// longer decides a yield. An interim "Okay." before the final "Okay, what
/// was that?" commits it as a turn; a delayed "Yeah." of a flush before
/// the yield, then this yield's "What was that?", commits it; a final
/// "Okay." alone, after its interim, resumes; a backchannel final followed
/// by a final question commits. The switch off: each commits as before.
#[test]
fn with_backchannel_holds_and_heard_always_only_the_yields_own_final_words_decide_it() {
    let commits: [&[(&str, bool)]; 3] = [
        &[("Okay.", false), ("Okay, what was that?", true)],
        &[("Yeah.", false), ("What was that?", true)],
        &[("Okay.", true), ("What was that?", true)],
    ];
    for results in commits {
        for word in [Word::Plain, Word::Speech] {
            let (_, committed, event) = results_heard_always(results, word, true);
            assert!(committed, "{results:?}");
            assert_eq!(event["backchannel"], false, "{results:?}");
            let (_, committed, _) = results_heard_always(results, word, false);
            assert!(committed, "{results:?} off");
        }
    }
    let held: &[(&str, bool)] = &[("Okay", false), ("Okay.", true)];
    let (at, committed, event) = results_heard_always(held, Word::Plain, true);
    assert_eq!((at, committed), (71, false));
    assert_eq!(event["backchannel"], true);
    let (_, committed, event) = results_heard_always(held, Word::Speech, true);
    assert!(!committed);
    assert_eq!(event["backchannel"], true);
    let (at, committed, _) = results_heard_always(held, Word::Plain, false);
    assert_eq!((at, committed), (71, true), "off");
    // No final of its own: held to the bound, then resumed with no words.
    let (_, committed, event) = results_heard_always(&[("Okay.", false)], Word::Plain, true);
    assert!(!committed);
    assert_eq!(event["kind"], "duplex_resume");
    assert_eq!(event["backchannel"], Value::Null);
}

/// #2939, lamp-52e6 at 09:44 on 23 Oct: with `heard-always` and the switch
/// on, "Yeah", "Okay", "Right", "I see" and "Sure" said over a story are
/// held as yields and the story resumes, as with `heard`, on either rule;
/// the duplex rule takes no face, so one turned to the lamp changes
/// nothing. With the switch off each commits, as at 09:44.
#[test]
fn with_backchannel_holds_and_heard_always_a_backchannel_resumes_the_reply() {
    for words in [
        "Yeah.", "Okay.", "Right.", "I see.", "Sure.", "Mm-hm.", "Uh-huh.",
    ] {
        let (at, committed, event) = a_short_word_heard_always(words, Word::Plain, true);
        assert_eq!((at, committed), (71, false), "{words} plain");
        assert_eq!(event["kind"], "duplex_resume");
        assert_eq!(event["backchannel"], true, "{words} plain");
        let (at, committed, event) = a_short_word_heard_always(words, Word::Speech, true);
        assert_eq!((at, committed), (2 + 29 + 25 + 40, false), "{words} speech");
        assert_eq!(event["held_ms"], 300, "{words} speech");
        assert_eq!(event["backchannel"], true, "{words} speech");
        // The switch off: committed as today.
        let (at, committed, event) = a_short_word_heard_always(words, Word::Plain, false);
        assert_eq!((at, committed), (71, true), "{words} off");
        assert_eq!(event["backchannel"], Value::Null, "{words} off");
        let (at, committed, _) = a_short_word_heard_always(words, Word::Speech, false);
        assert_eq!((at, committed), (37, true), "{words} off");
    }
}

/// lamp-52e6 on 25 Oct (#2939): "Hmm, interesting.", an aside said over a
/// story, committed and stopped it 4 of 4 times. With `LAMP_ASIDES_HOLD=on`
/// as well as backchannel holds it resumes the reply on both rules; without
/// it, it commits as before; a longer line still cuts in.
#[test]
fn with_asides_hold_an_aside_resumes_the_reply() {
    let heard = |words: &'static str, word: Word, asides: bool| {
        let stop_words = crate::stop_words::StopWords::always_counted(
            Box::new(AlwaysEars {
                words: Some(words),
                flushed: false,
            }),
            None,
        )
        .with_backchannel_holds(true)
        .with_asides(asides);
        a_short_word_heard_by(stop_words, "", word, true)
    };
    for words in ["Hmm, interesting.", "Interesting.", "Wow.", "Oh, nice."] {
        let (at, committed, event) = heard(words, Word::Plain, true);
        assert_eq!((at, committed), (71, false), "{words} plain");
        assert_eq!(event["backchannel"], true, "{words} plain");
        let (at, committed, event) = heard(words, Word::Speech, true);
        assert_eq!((at, committed), (2 + 29 + 25 + 40, false), "{words} speech");
        assert_eq!(event["backchannel"], true, "{words} speech");
        // The switch off: committed as on the lamp.
        let (_, committed, event) = heard(words, Word::Plain, false);
        assert!(committed, "{words} off");
        assert_eq!(event["backchannel"], false, "{words} off");
    }
    let (_, committed, _) = heard("Interesting, tell me more.", Word::Plain, true);
    assert!(committed);
}

/// #2939, lamp-52e6 at 09:44 on 23 Oct, with `heard` and the switch on:
/// "Yeah" and "Okay" were held, but "Right" came as two words, and it,
/// "I see" and "Sure" each committed and were let out as "someone is
/// turned to the lamp". That verdict is the busy-room rule's on a turn
/// already committed (`provider_worker`); the duplex rule that holds the
/// yield takes no face. So, as the speech-to-text may write them, each
/// resumes the reply on both rules and no turn starts, and nothing reaches
/// that verdict.
#[test]
fn with_backchannel_holds_the_five_from_lamp_52e6_resume_even_with_a_face_turned() {
    for words in [
        "Yeah.",
        "Okay.",
        "Right.",
        "I see.",
        "Sure.",
        "All right.",
        "Right, right.",
        "Oh, I see.",
        "Okay, sure.",
        "Uh huh.",
        "Mm hm.",
    ] {
        let (at, _, committed, event) = a_short_word(Some(words), Word::Plain, true, false);
        assert_eq!((at, committed), (71, false), "{words} plain");
        assert_eq!(event["backchannel"], true, "{words} plain");
        let (at, _, committed, event) = a_short_word(Some(words), Word::Speech, true, false);
        assert_eq!((at, committed), (2 + 29 + 25 + 40, false), "{words} speech");
        assert_eq!(event["backchannel"], true, "{words} speech");
    }
}

/// #2939: with `heard-always` and the switch on, a call, a question or
/// more than the backchannel words still cut in once their words come; a
/// stop, as with `heard`, at its words, the eleventh still block.
#[test]
fn with_backchannel_holds_and_heard_always_a_request_still_cuts_in() {
    let (at, committed, event) = a_short_word_heard_always("Lamp, wait.", Word::Plain, true);
    assert_eq!((at, committed), (2 + 29 + 11, true));
    assert_eq!(event["backchannel"], false);
    for words in ["What was that?", "Yeah, but why?", "Okay tell me more"] {
        let (at, committed, event) = a_short_word_heard_always(words, Word::Plain, true);
        assert_eq!((at, committed), (71, true), "{words} plain");
        assert_eq!(event["backchannel"], false, "{words} plain");
        let (at, committed, event) = a_short_word_heard_always(words, Word::Speech, true);
        assert_eq!((at, committed), (2 + 29 + 25 + 11, true), "{words} speech");
        assert_eq!(event["held_ms"], 300, "{words} speech");
    }
}

/// lamp-52e6 on 24 Oct (#2939): with the switch on, a stop said over the
/// lamp always cuts in, with `heard` and with `heard-always`, however it
/// ends; "Okay." alone is still held.
#[test]
fn with_backchannel_holds_a_stop_always_cuts_in() {
    for words in ["Stop, stop.", "Stop now.", "Okay, stop.", "Lamp, stop."] {
        for word in [Word::Over, Word::Plain, Word::Speech] {
            let (_, _, committed, event) = a_short_word(Some(words), word, true, false);
            assert!(committed, "{words} {word:?} heard");
            assert_eq!(event["backchannel"], false, "{words} {word:?} heard");
            let (_, committed, _) = a_short_word_heard_always(words, word, true);
            assert!(committed, "{words} {word:?} heard-always");
        }
    }
    let (_, _, committed, event) = a_short_word(Some("Okay."), Word::Plain, true, false);
    assert!(!committed);
    assert_eq!(event["backchannel"], true);
    let (_, committed, _) = a_short_word_heard_always("Okay.", Word::Plain, true);
    assert!(!committed);
    // Reviewer-two's P2 on #3295: words the lamp has just said itself are
    // no stop with `heard-always`, as over the lamp: the yield resumes.
    let stop_words = crate::stop_words::StopWords::always_counted(
        Box::new(AlwaysEars {
            words: Some("the fox had to wait"),
            flushed: false,
        }),
        None,
    )
    .with_backchannel_holds(true);
    let (_, committed, _) =
        a_short_word_heard_by(stop_words, "And so the fox had to wait.", Word::Over, true);
    assert!(!committed);
}

/// Reviewer-two's P2 on #3295: the 03:08 "Stop, stop." with its words
/// coming 300 ms after they were asked for, as the relay's do, and the
/// second word, 250 ms in the clear, said in between. The stream stopped
/// listening at the ask, so those words did not hear it: it commits as any
/// speech does, the switch on or off.
#[test]
fn with_backchannel_holds_speech_after_a_backchannels_words_were_asked_for_commits() {
    for holds in [true, false] {
        let mut h = Harness::new(TurnTaking::Duplex, 400);
        h.stop_words = Some(crate::stop_words::StopWords::new(Box::new(Ears {
            words: Some("Yeah."),
            asked: false,
            heard: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            wait: 30,
        })));
        if holds {
            h.mask.hold_backchannels();
        }
        let old = h.old();
        h.blocks(20, 0.99, Some(evidence(0, OWN)));
        h.blocks(ONSET + 2, 0.9, Some(evidence(0, OVER)));
        for quiet in 1..=29 {
            h.block(0.1, Some(evidence(quiet, None)));
        }
        // Still until its words are asked for, at the tenth still block.
        h.blocks(11, 0.1, Some(evidence(40, None)));
        assert!(h.events("stop_words").is_empty(), "holds {holds}");
        // "Stop.", 250 ms in the clear before the words come.
        h.blocks(25, 0.9, Some(evidence(40, None)));
        h.blocks(40, 0.1, Some(evidence(40, None)));
        assert_eq!(h.events("duplex_commit").len(), 1, "holds {holds}");
        assert!(h.events("duplex_resume").is_empty(), "holds {holds}");
        assert_eq!(h.events("turn_finished")[0]["outcome"], "user_interrupted");
        assert_ne!(h.old(), old);
    }
}

/// `heard-always` ears whose finals come late: each of `finals` comes its
/// count of blocks after the first flush since the yield's mark.
struct LateFinals {
    finals: Vec<(&'static str, usize)>,
    heard: usize,
    marked: bool,
    flushed_at: Option<usize>,
}

impl crate::stop_words::Ears for LateFinals {
    fn ready(&mut self) {}
    fn hear(&mut self, _: u64, _: &[i16]) {
        self.heard += 1;
    }
    fn finalize(&mut self) {}
    fn mark(&mut self) {
        self.marked = true;
    }
    fn flush(&mut self) {
        if self.marked {
            self.flushed_at.get_or_insert(self.heard);
        }
    }
    fn words(&mut self) -> Option<String> {
        let at = self.flushed_at?;
        let (words, wait) = *self.finals.first()?;
        (self.heard >= at + wait).then(|| {
            self.finals.remove(0);
            words.to_owned()
        })
    }
    fn forget(&mut self) {
        panic!("a stream heard always is never forgotten in a yield");
    }
    fn rest(&mut self) {}
}

/// Reviewer-two's HOLD on #3295 at 04:07: with `heard-always` the first
/// word's words are asked for at the detector's own pause, the twentieth
/// quiet block under the lamp's fading sound, before the duplex rule counts
/// a still block. The second word, 250 ms in the clear, comes after that
/// ask; the first final, "Yeah.", comes after it, and the second final long
/// after. The second word is speech those words did not hear: it commits
/// once they come, the switch on or off, before the second final.
#[test]
fn with_backchannel_holds_and_heard_always_speech_after_the_first_flush_commits() {
    for holds in [true, false] {
        let mut h = Harness::new(TurnTaking::Duplex, 400);
        h.stop_words = Some(
            crate::stop_words::StopWords::always_counted(
                Box::new(LateFinals {
                    finals: vec![("Yeah.", 9 + 25 + 10 + 1), ("Stop.", 400)],
                    heard: 0,
                    marked: false,
                    flushed_at: None,
                }),
                None,
            )
            .with_backchannel_holds(holds),
        );
        h.scores_speech = true;
        if holds {
            h.mask.hold_backchannels();
        }
        let old = h.old();
        h.blocks(20, 0.99, Some(evidence(0, OWN)));
        h.blocks(ONSET + 2, 0.9, Some(evidence(0, OVER)));
        // The lamp's sound dies away; the first word's words are asked for
        // at the twentieth of these blocks.
        for quiet in 1..=29 {
            h.block(0.1, Some(evidence(quiet, None)));
        }
        // "Stop.", 250 ms in the clear, then still.
        h.blocks(25, 0.9, Some(evidence(40, None)));
        h.blocks(10, 0.1, Some(evidence(40, None)));
        for _ in 0..150 {
            if !h.events("duplex_commit").is_empty() || !h.events("duplex_resume").is_empty() {
                break;
            }
            h.block(0.1, Some(evidence(40, None)));
        }
        assert_eq!(h.events("duplex_commit").len(), 1, "holds {holds}");
        assert!(h.events("duplex_resume").is_empty(), "holds {holds}");
        assert_eq!(h.events("turn_finished")[0]["outcome"], "user_interrupted");
        assert_ne!(h.old(), old);
    }
}

/// lamp-52e6 at 03:08 on 24 Oct (#2939): "Stop, stop." over a story. Its
/// first word, under the lamp's voice, came as "yeah", asked for at the
/// pause after it; its second, 300 ms in the clear after those words came,
/// was held as that backchannel and the story went on. Speech that goes on
/// after a backchannel's words came is speech they did not hear: it
/// commits as any speech does, at its sixth block. The switch off, the
/// same; a backchannel whose words come after its speech is still held
/// ([`with_backchannel_holds_a_backchannel_resumes_the_reply_on_either_rule`]).
#[test]
fn with_backchannel_holds_speech_after_a_backchannels_words_commits() {
    for holds in [true, false] {
        let mut h = Harness::new(TurnTaking::Duplex, 400);
        h.stop_words = Some(crate::stop_words::StopWords::new(Box::new(Ears {
            words: Some("Yeah."),
            asked: false,
            heard: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            wait: 0,
        })));
        if holds {
            h.mask.hold_backchannels();
        }
        let old = h.old();
        h.blocks(20, 0.99, Some(evidence(0, OWN)));
        h.blocks(ONSET + 2, 0.9, Some(evidence(0, OVER)));
        for quiet in 1..=29 {
            h.block(0.1, Some(evidence(quiet, None)));
        }
        // Still until its words are asked for and come: a backchannel.
        h.blocks(12, 0.1, Some(evidence(40, None)));
        let words = h.events("stop_words");
        assert_eq!(words.len(), 1, "holds {holds}");
        assert_eq!(words[0]["backchannel"], true, "holds {holds}");
        assert!(h.events("duplex_commit").is_empty(), "holds {holds}");
        assert!(h.events("duplex_resume").is_empty(), "holds {holds}");
        // "Stop.", 300 ms in the clear.
        let speech = h.blocks(30, 0.9, Some(evidence(40, None)));
        let commit = h.events("duplex_commit");
        assert_eq!(commit.len(), 1, "holds {holds}");
        assert!(speech[5].accepted.is_some(), "holds {holds}");
        assert_eq!(h.events("turn_finished")[0]["outcome"], "user_interrupted");
        assert_ne!(h.old(), old);
    }
}

#[test]
fn a_stop_word_heard_in_a_yield_commits_it_and_other_words_resume_it() {
    // Asked at the tenth still block, given at once here (the relay takes
    // about 25 blocks), committed at the eleventh: 2 over, 29 dying away,
    // 11 still.
    let (at, heard, committed) = a_short_word_with(Some("Stop."));
    assert_eq!((at, committed), (2 + 29 + 11, true));
    // Held audio (the candidate's 30 blocks) and each block until asked.
    assert_eq!(heard, 30 + 2 + 29 + 10);
    // Other words: resumed at the fortieth still block, as without.
    assert_eq!(a_short_word_with(Some("Mm-hm.")).0, 71);
    assert!(!a_short_word_with(Some("Mm-hm.")).2);
    // No words: the deadline, 110 blocks from the yield.
    let (at, _, committed) = a_short_word_with(None);
    assert_eq!((at, committed), (110, false));
}

/// #2939, `LAMP_BACKCHANNEL_HOLDS=on`: a backchannel that would commit
/// today, on the quiet after a plain word or on speech after the lamp's
/// sound, is held for its words and resumes the reply, with no turn. The
/// words are given as soon as they are asked for, at the tenth still block.
#[test]
fn with_backchannel_holds_a_backchannel_resumes_the_reply_on_either_rule() {
    for (words, vietnamese) in [("Mm-hm.", false), ("Yeah.", false), ("Ừ.", true)] {
        // Plain, then quiet: committed at the fortieth still block today
        // (2 over, 29 dying away, 40 still); resumed there now.
        let (at, _, committed, event) = a_short_word(Some(words), Word::Plain, false, vietnamese);
        assert_eq!((at, committed), (71, true), "{words} off");
        assert_eq!(event["backchannel"], true, "{words} off");
        assert_eq!(event["held_ms"], 0, "{words} off");
        let (at, _, committed, event) = a_short_word(Some(words), Word::Plain, true, vietnamese);
        assert_eq!((at, committed), (71, false), "{words} plain");
        assert_eq!(event["kind"], "duplex_resume");
        assert_eq!(event["backchannel"], true);
        // Its words came before the quiet was due: nothing held.
        assert_eq!(event["held_ms"], 0);
        // Speech once the sound is gone: committed at its sixth block today
        // (2 + 29 + 6); now held to the words, at the tenth still block
        // after it (30 blocks), and resumed at the fortieth.
        let (at, _, committed, _) = a_short_word(Some(words), Word::Speech, false, vietnamese);
        assert_eq!((at, committed), (37, true), "{words} off");
        let (at, _, committed, event) = a_short_word(Some(words), Word::Speech, true, vietnamese);
        assert_eq!((at, committed), (2 + 29 + 25 + 40, false), "{words} speech");
        assert_eq!(event["held_ms"], 300);
        assert_eq!(event["backchannel"], true);
    }
}

/// #2939: a stop phrase, "wait", a question and three words still commit
/// with the switch on, at once once their words come; with no words the
/// hold ends 500 ms after they were asked for, or at the yield's deadline
/// if sooner.
/// The reviewer-lead on #2959: a 250 ms "Yeah." on the speech rule is due
/// 60 ms into it, and its words come after 100 ms of stillness and the
/// relay's 350 ms, some 0.7 s later: held to them, not to a fixed 500 ms
/// from the commit falling due. Resumed at the later of the fortieth still
/// block and the words, at the forty-fifth.
#[test]
fn with_backchannel_holds_a_backchannel_whose_words_come_late_still_resumes() {
    let (at, _, committed, event) =
        a_short_word_after("", 35, Some("Yeah."), Word::Speech, true, false);
    assert_eq!((at, committed), (2 + 29 + 25 + 10 + 35 + 1, false));
    assert_eq!(event["backchannel"], true);
}

/// The reviewer-lead on #2959: the lamp asks "Shall I go on?" and the
/// person says "Yes.": an answer, not a backchannel, so it commits as
/// today. One after a statement still resumes, as does one after a
/// question the lamp has since finished a sentence after.
#[test]
fn with_backchannel_holds_an_answer_to_the_lamps_question_commits() {
    for (said, words, resumed) in [
        ("It rained all day. Shall I go on?", "Yes.", false),
        ("Shall I go on? Well", "Sure.", false),
        ("Do you want more? ", "Mm-hm.", false),
        ("It rained all day.", "Yes.", true),
        ("Shall I go on? Okay. Then", "Yeah.", true),
    ] {
        let (_, _, committed, event) =
            a_short_word_after(said, 0, Some(words), Word::Plain, true, false);
        assert_eq!(committed, !resumed, "{said:?} {words}");
        assert_eq!(event["backchannel"], resumed, "{said:?} {words}");
    }
}

#[test]
fn with_backchannel_holds_anything_else_commits() {
    for words in ["Stop.", "Wait, I", "What was that?", "Why?"] {
        // Plain, then quiet: a stop phrase at the block after its words, as
        // without the switch; others at the quiet, as today.
        let (at, _, committed, event) = a_short_word(Some(words), Word::Plain, true, false);
        let stop = crate::stop_words::stop_phrase(words);
        assert_eq!(
            (at, committed),
            (if stop { 2 + 29 + 11 } else { 71 }, true),
            "{words} plain"
        );
        assert_eq!(event["backchannel"], false);
        // Speech: held to its words, then committed at the next block.
        let (at, _, committed, event) = a_short_word(Some(words), Word::Speech, true, false);
        assert_eq!((at, committed), (2 + 29 + 25 + 11, true), "{words} speech");
        assert_eq!(event["held_ms"], 300);
        assert_eq!(event["backchannel"], false);
    }
    // No words: the hold ends 500 ms after they were asked for, at the
    // tenth still block (the quiet rule: 2 + 29 + 60, held from the
    // fortieth), or at the yield's deadline, 110 blocks from it, if sooner
    // (the speech rule: held from the sixth block of speech, 37), so the
    // request is never kept past it; and the reply resumes, a wordless
    // yield ending no answer.
    let (at, _, committed, event) = a_short_word(None, Word::Plain, true, false);
    assert_eq!((at, committed), (2 + 29 + 60, false));
    assert_eq!(event["held_ms"], 200);
    assert_eq!(event["backchannel"], Value::Null);
    let (at, _, committed, event) = a_short_word(None, Word::Speech, true, false);
    assert_eq!((at, committed), (110, false));
    assert_eq!(event["held_ms"], 730);
    // The switch off: no words commit as today.
    let (at, _, committed, event) = a_short_word(None, Word::Speech, false, false);
    assert_eq!((at, committed), (37, true));
    assert_eq!(event["held_ms"], 0);
}

/// lamp-52e6 on #3200, `heard-always` and the switch on: 2.3 s into a
/// story a yield came with no words, room sound or the lamp's own echo; its
/// commit was held 390 ms for them and then cut the story. A wordless yield
/// now resumes the story, on either rule, with no turn and no cut: when the
/// hold's wait runs out, and when the yield ends before its words came.
/// The switch off: each commits as before.
#[test]
fn with_backchannel_holds_a_wordless_yield_resumes_the_reply() {
    let wordless = |word: Word, holds: bool| {
        let stop_words = crate::stop_words::StopWords::always_counted(
            Box::new(AlwaysEars {
                words: None,
                flushed: false,
            }),
            None,
        )
        .with_backchannel_holds(holds);
        a_short_word_heard_by(stop_words, "", word, holds)
    };
    // The hold's wait runs out: the speech rule, held to the deadline.
    let (at, committed, event) = wordless(Word::Speech, true);
    assert_eq!((at, committed), (110, false));
    assert_eq!(event["kind"], "duplex_resume");
    assert_eq!(event["held_ms"], 730);
    assert_eq!(event["backchannel"], Value::Null);
    // The quiet rule, held 500 ms past the ask.
    let (at, committed, event) = wordless(Word::Plain, true);
    assert_eq!((at, committed), (2 + 29 + 60, false));
    assert_eq!(event["kind"], "duplex_resume");
    assert_eq!(event["held_ms"], 200);
    // The switch off: committed as before.
    let (at, committed, _) = wordless(Word::Speech, false);
    assert_eq!((at, committed), (37, true));
    let (at, committed, _) = wordless(Word::Plain, false);
    assert_eq!((at, committed), (71, true));
}

/// lamp-52e6 on 24 Oct (`heard`, the switch on): a first word over the
/// story ("Wait", "Sorry"), 150 ms in the clear, then a 300 ms pause, at
/// which its words are asked for, then `then` blocks of the question in
/// the clear. The ask hears only the first word, and its words come back
/// as `words`. The block the yield settled at after it, whether it
/// committed, and its record.
fn a_first_word_then_its_question(
    words: Option<&'static str>,
    then: usize,
    holds: bool,
) -> (u64, bool, Value) {
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    h.stop_words = Some(crate::stop_words::StopWords::new(Box::new(Ears {
        words,
        asked: false,
        heard: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        wait: 0,
    })));
    if holds {
        h.mask.hold_backchannels();
    }
    let old = h.old();
    h.blocks(20, 0.99, Some(evidence(0, OWN)));
    h.blocks(ONSET + 2, 0.9, Some(evidence(0, OVER)));
    let yielded_at = h.events("duplex_yield")[0]["frame_sequence"]
        .as_u64()
        .unwrap();
    for quiet in 1..=29 {
        h.block(0.1, Some(evidence(quiet, None)));
    }
    let mut found = None;
    let plan = std::iter::repeat_n(0.9, 15)
        .chain(std::iter::repeat_n(0.1, 30))
        .chain(std::iter::repeat_n(0.9, then))
        .chain(std::iter::repeat_n(0.1, 150));
    for score in plan {
        h.block(score, Some(evidence(40, None)));
        found = h
            .events("duplex_commit")
            .into_iter()
            .chain(h.events("duplex_resume"))
            .next();
        if found.is_some() {
            break;
        }
    }
    let found = found.expect("the yield never settled");
    let committed = !h.events("duplex_commit").is_empty();
    if committed {
        assert_eq!(h.events("turn_finished")[0]["outcome"], "user_interrupted");
        assert_ne!(h.old(), old);
    } else {
        assert_eq!(h.old(), old);
        assert!(h.events("turn_finished").is_empty());
    }
    (
        found["frame_sequence"].as_u64().unwrap() - yielded_at,
        committed,
        found,
    )
}

/// lamp-52e6 on 24 Oct (`heard`, the switch on), 2 of 3 cut-ins: "Wait"
/// or "Sorry", a 300 ms pause, then the question. The words were asked
/// for at the pause and heard only the first word, which came back as no
/// words; the question that went on in the clear was held as a wordless
/// yield until the deadline, and the story resumed. Speech that goes on
/// after the ask now ends the wait for words: the question commits at its
/// sixth block, and the turn's own speech-to-text hears it whole. A lone
/// backchannel whose words come back empty still resumes; "Stop." still
/// commits at its words; the switch off, as before.
#[test]
fn with_backchannel_holds_a_question_after_a_pause_commits_though_its_first_word_came_back_empty() {
    // 2 over, 29 dying away, 15 of "Wait", 30 still (asked at the tenth),
    // then the question's sixth block.
    let (at, committed, event) = a_first_word_then_its_question(Some(""), 60, true);
    assert_eq!((at, committed), (2 + 29 + 15 + 30 + 6, true));
    assert_eq!(event["backchannel"], Value::Null);
    // No question after it: held 500 ms past the ask, then resumed.
    let (at, committed, event) = a_first_word_then_its_question(Some(""), 0, true);
    assert_eq!((at, committed), (2 + 29 + 15 + 60, false));
    assert_eq!(event["backchannel"], Value::Null);
    // A stop commits at its words, the block after the ask.
    let (at, committed, _) = a_first_word_then_its_question(Some("Stop."), 60, true);
    assert_eq!((at, committed), (2 + 29 + 15 + 11, true));
    // The switch off: the first word's speech commits at its sixth block.
    let (at, committed, _) = a_first_word_then_its_question(Some(""), 60, false);
    assert_eq!((at, committed), (2 + 29 + 6, true));
}

/// lamp-52e6 on #3200: "a yield ended before its words came". With the
/// switch on the yield it ends is resumed, not committed, and the words
/// said to be missing are recorded; with `heard` too.
#[test]
fn with_backchannel_holds_a_yield_that_ends_before_its_words_resumes_the_reply() {
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    h.stop_words = Some(
        crate::stop_words::StopWords::always_counted(
            Box::new(AlwaysEars {
                words: None,
                flushed: false,
            }),
            None,
        )
        .with_backchannel_holds(true),
    );
    h.mask.hold_backchannels();
    let old = h.old();
    h.blocks(20, 0.99, Some(evidence(0, OWN)));
    h.blocks(ONSET + 2, 0.9, Some(evidence(0, Some((10, 0)))));
    for quiet in 1..=29 {
        h.block(0.1, Some(evidence(quiet, None)));
    }
    h.blocks(80, 0.1, Some(evidence(40, None)));
    let unheard: Vec<_> = h
        .events("stop_words")
        .into_iter()
        .filter(|event| event["words"].is_null())
        .collect();
    assert_eq!(unheard.len(), 1, "the yield's missing words were not said");
    assert_eq!(h.events("duplex_resume").len(), 1);
    assert!(h.events("duplex_commit").is_empty());
    assert!(h.events("turn_finished").is_empty());
    assert_eq!(h.old(), old);
    assert!(!h.admission.owns_stream());
    assert!(!h.mask.yield_in_force());
    // With `heard`, no words at all: resumed at the hold's end.
    let (at, _, committed, event) = a_short_word(None, Word::Plain, true, false);
    assert_eq!(
        (at, committed, event["kind"].clone()),
        (2 + 29 + 60, false, json!("duplex_resume"))
    );
}

/// `heard-always` ears whose words come 80 result reads (blocks) after the
/// first flush, once: a speech-to-text slower than the hold. `given`: they
/// came.
struct LateEars {
    words: &'static str,
    wait: Option<usize>,
    given: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl crate::stop_words::Ears for LateEars {
    fn ready(&mut self) {}
    fn hear(&mut self, _: u64, _: &[i16]) {}
    fn finalize(&mut self) {}
    fn flush(&mut self) {
        self.wait.get_or_insert(80);
    }
    fn words(&mut self) -> Option<String> {
        let wait = self.wait.as_mut()?;
        if *wait > 0 {
            *wait -= 1;
            return None;
        }
        self.wait = Some(usize::MAX);
        self.given.store(true, std::sync::atomic::Ordering::Relaxed);
        Some(self.words.to_owned())
    }
    fn forget(&mut self) {
        panic!("a stream heard always is never forgotten in a yield");
    }
    fn rest(&mut self) {}
}

/// The lamp-52e6 finding on #3200, a slow speech-to-text: words that come
/// after the hold resumed the reply are not the yield's. They start no
/// turn and cut nothing, and nothing is left yielding; the lamp goes on.
#[test]
fn with_backchannel_holds_words_after_a_wordless_resume_start_nothing() {
    let given = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    h.stop_words = Some(
        crate::stop_words::StopWords::always_counted(
            Box::new(LateEars {
                words: "What was that?",
                wait: None,
                given: std::sync::Arc::clone(&given),
            }),
            None,
        )
        .with_backchannel_holds(true),
    );
    h.mask.hold_backchannels();
    let old = h.old();
    h.blocks(20, 0.99, Some(evidence(0, OWN)));
    h.blocks(ONSET + 2, 0.9, Some(evidence(0, Some((10, 0)))));
    for quiet in 1..=29 {
        h.block(0.1, Some(evidence(quiet, None)));
    }
    while h.events("duplex_resume").is_empty() {
        assert!(h.events("duplex_commit").is_empty());
        h.block(0.1, Some(evidence(40, None)));
    }
    assert!(!given.load(std::sync::atomic::Ordering::Relaxed));
    h.blocks(150, 0.99, Some(evidence(0, OWN)));
    assert!(given.load(std::sync::atomic::Ordering::Relaxed));
    assert!(
        h.events("stop_words")
            .iter()
            .all(|event| event["words"].is_null()),
        "late words were told as the yield's"
    );
    assert_eq!(h.events("duplex_yield").len(), 1);
    assert!(h.events("duplex_commit").is_empty());
    assert!(h.events("turn_finished").is_empty());
    assert_eq!(h.old(), old);
    assert!(!h.admission.owns_stream());
    assert!(!h.mask.yield_in_force());
}

/// lamp-52e6 on #3200: with the switch on, a yield whose words come and
/// are no backchannel still commits as a turn, on either rule.
#[test]
fn with_backchannel_holds_a_yields_real_words_still_commit_a_turn() {
    for word in [Word::Plain, Word::Speech] {
        let (_, committed, event) =
            a_short_word_heard_always("Can you turn the light down?", word, true);
        assert!(committed, "{word:?}");
        assert_eq!(event["kind"], "duplex_commit");
        assert_eq!(event["backchannel"], false);
    }
}

/// #2939: two backchannels the reply went on after are a person, not the
/// lamp's own voice, and cost no fruitless yield: a third line 5 s after
/// the second still makes the lamp yield, and commits once it is a
/// sentence (700 ms of speech) though no words came for it.
#[test]
fn with_backchannel_holds_two_backchannels_leave_the_reply_yielding() {
    let mut h = Harness::new(TurnTaking::Duplex, 40_000);
    h.stop_words = Some(crate::stop_words::StopWords::new(Box::new(Ears {
        words: Some("Mm-hm."),
        asked: false,
        heard: std::sync::Arc::default(),
        wait: 0,
    })));
    h.mask.hold_backchannels();
    yields_over_a_story(&mut h, 2, &[(ONSET + 2, Some((10, 0)))], (0, 0.0, None), 5);
    assert_eq!(h.mask.duplex_summary().unwrap(), ((2, 0, 2), false));
    assert!(h.events("turn_finished").is_empty());
    assert!(
        a_line_over_the_story(&mut h, 200),
        "the third line was not heard"
    );
    assert_eq!(h.events("duplex_yield").len(), 3);
    let commit = &h.events("duplex_commit")[0];
    assert_eq!(commit["backchannel"], Value::Null);
    assert_eq!(h.events("turn_finished")[0]["outcome"], "user_interrupted");
}

/// lamp-52e6 at 12:56 on 23 Oct (#2939), `heard-always` and the switch on:
/// a played "Yeah.", some 0.5 s of sound, reached 400 ms of speech in the
/// clear and committed as a turn though its words were a backchannel, and
/// the story stopped; with `heard`, "Uh-huh." did the same. A backchannel
/// of 500 ms now resumes on both, its words given at once or after the
/// hold's wait; the switch off, each commits as before.
#[test]
fn with_backchannel_holds_a_half_second_backchannel_resumes_heard_or_heard_always() {
    for words in [
        "Yeah.", "Mm-hm.", "Uh-huh.", "Okay.", "Right.", "I see.", "Got it.",
    ] {
        let (_, _, committed, event) = a_short_word(Some(words), Word::Long, true, false);
        assert!(!committed, "{words} heard");
        assert_eq!(event["kind"], "duplex_resume", "{words} heard");
        let (_, committed, event) = a_short_word_heard_always(words, Word::Long, true);
        assert!(!committed, "{words} heard-always");
        assert_eq!(event["kind"], "duplex_resume", "{words} heard-always");
        let (_, _, committed, _) = a_short_word(Some(words), Word::Long, false, false);
        assert!(committed, "{words} off");
        let (_, committed, _) = a_short_word_heard_always(words, Word::Long, false);
        assert!(committed, "{words} heard-always off");
    }
    // Its words after the hold's wait: resumed with no words, no turn.
    let (_, _, committed, event) =
        a_short_word_after("", 60, Some("Yeah."), Word::Long, true, false);
    assert!(!committed);
    assert_eq!(event["kind"], "duplex_resume");
}

/// #2939: a 500 ms yield whose words never come, or end before the yield
/// does, resumes on both modes: "a yield ended before its words came"
/// cuts nothing.
#[test]
fn with_backchannel_holds_a_half_second_wordless_yield_resumes() {
    let (_, _, committed, event) = a_short_word(None, Word::Long, true, false);
    assert!(!committed);
    assert_eq!(event["kind"], "duplex_resume");
    let stop_words = crate::stop_words::StopWords::always_counted(
        Box::new(AlwaysEars {
            words: None,
            flushed: false,
        }),
        None,
    )
    .with_backchannel_holds(true);
    let (_, committed, event) = a_short_word_heard_by(stop_words, "", Word::Long, true);
    assert!(!committed);
    assert_eq!(event["kind"], "duplex_resume");
}

/// lamp-52e6 on 23 Oct (#2939), `heard`: "a yield's words came: 0 words,
/// not a stop phrase" committed a turn. Words that come empty are none,
/// on both modes, with the switch; without it they commit as before. A
/// final of only punctuation, "...", is no words as well.
#[test]
fn with_backchannel_holds_empty_words_resume_the_reply() {
    for words in ["", "...", " - "] {
        for word in [Word::Plain, Word::Long] {
            let (_, _, committed, event) = a_short_word(Some(words), word, true, false);
            assert!(!committed, "{words:?} {word:?} heard");
            assert_eq!(event["kind"], "duplex_resume");
            let (_, committed, event) = a_short_word_heard_always(words, word, true);
            assert!(!committed, "{words:?} {word:?} heard-always");
            assert_eq!(event["kind"], "duplex_resume");
            let (_, _, committed, _) = a_short_word(Some(words), word, false, false);
            assert!(committed, "{words:?} {word:?} off");
        }
    }
}

/// lamp-52e6 on 23 Oct (#2939), `heard`: a played "Uh-huh." came as one
/// word not on the list and committed. The speech-to-text's spellings of
/// the backchannels, any dash among them, resume.
#[test]
fn with_backchannel_holds_the_speech_to_texts_spellings_resume() {
    for words in [
        "Mhmm.",
        "Mmm.",
        "Uh\u{2013}huh.",
        "Uh\u{2011}huh.",
        "Mm\u{2014}hm.",
        "Uh-hum.",
        "Uh hum.",
        "Yea.",
        "Yeh.",
        "Oh, okay.",
        "Hmm, I see.",
    ] {
        let (_, _, committed, event) = a_short_word(Some(words), Word::Plain, true, false);
        assert!(!committed, "{words} heard");
        assert_eq!(event["backchannel"], true, "{words}");
        let (_, committed, _) = a_short_word_heard_always(words, Word::Plain, true);
        assert!(!committed, "{words} heard-always");
    }
}

/// #2939: a turn opener, a stop and a backchannel with a stop still commit
/// with the switch on, on both modes, at their full length too: "So." and
/// "Ahem." were held as backchannels on lamp-52e6 on 23 Oct, each heard as
/// one word; a lone "Oh." or "Hmm." may open a turn as well.
#[test]
fn with_backchannel_holds_openers_and_stops_still_commit() {
    for words in [
        "So.",
        "Uh.",
        "Oh.",
        "Ah.",
        "Hmm.",
        "Hm.",
        "Ahem.",
        "But",
        "Wait.",
        "Uh-uh.",
        "Yeah, stop.",
        "Lamp.",
    ] {
        for word in [Word::Plain, Word::Long] {
            let (_, _, committed, event) = a_short_word(Some(words), word, true, false);
            assert!(committed, "{words} {word:?} heard");
            assert_eq!(event["backchannel"], false, "{words} {word:?}");
            let (_, committed, _) = a_short_word_heard_always(words, word, true);
            assert!(committed, "{words} {word:?} heard-always");
        }
    }
}

#[test]
fn a_brief_acknowledgment_costs_a_pause_and_none_of_the_reply() {
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    let old = h.old();
    let mut ticks = h.blocks(20, 0.99, Some(evidence(0, OWN)));
    // "Mm-hm", 120 ms over the lamp.
    ticks.extend(h.blocks(ONSET + 2, 0.9, Some(evidence(0, OVER))));
    let yielded_at = 20 + ONSET_SEQUENCE;
    assert_eq!(h.events("duplex_yield")[0]["frame_sequence"], yielded_at);
    // Then nobody speaks while the lamp's sound dies away, and after it.
    for quiet in 1..=29 {
        ticks.push(h.block(0.1, Some(evidence(quiet, None))));
    }
    let still = h.blocks(40, 0.1, Some(evidence(40, None)));
    assert!(still[..39].iter().all(|tick| tick.dispatched.is_none()));
    assert!(
        still[39].dispatched.is_some(),
        "the reply did not go on in the block that settled it"
    );
    let resumed = h.events("duplex_resume");
    assert_eq!(resumed.len(), 1);
    assert_eq!(resumed[0]["frame_sequence"], yielded_at + 71);
    ticks.extend(still);
    ticks.extend(h.blocks(40, 0.99, Some(evidence(0, OWN))));
    // The pause, in blocks during which the speaker was offered nothing.
    assert_eq!(ticks.len() - dispatched(&ticks), 71);
    // Same reply, same owner, nothing admitted, nothing cancelled.
    assert_eq!(h.old(), old);
    assert!(ticks.iter().all(|tick| tick.accepted.is_none()));
    assert!(h.events("turn_finished").is_empty());
    assert!(!h.admission.owns_stream());
    assert!(
        h.admission
            .pending_evidence(monotonic_us(), AcceptanceBasis::DirectedSessionDuplex)
            .is_none(),
        "the acknowledgment stayed a candidate"
    );
    // And the speaker got every sample of the reply, in order, none twice.
    let played: Vec<i16> = ticks
        .iter()
        .filter_map(|tick| tick.dispatched.clone())
        .flatten()
        .collect();
    assert!(
        played
            .iter()
            .enumerate()
            .all(|(index, sample)| usize::try_from(*sample).unwrap() == index % 30_000)
    );
    assert_eq!(h.mask.duplex_summary(), Some(((1, 0, 1), false)));
}

#[test]
fn a_person_who_pauses_longer_than_the_endpoint_wait_and_goes_on_still_interrupts() {
    // As on lamp-4ace on 2026-10-13: a word over the lamp, 640 ms of nothing
    // and then speech, with the lamp still waiting. The detector's own wait
    // for the end of speech is 600 ms there and had closed the utterance, so
    // the commit found nothing current to admit and the process ended.
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    h.detector = TurnDetector::v1_baseline().with_end_silence(60).unwrap();
    let old = h.old();
    h.blocks(20, 0.99, Some(evidence(0, OWN)));
    h.blocks(ONSET, 0.9, Some(evidence(0, OVER)));
    assert_eq!(h.events("duplex_yield").len(), 1);
    for quiet in 1..=29 {
        h.block(0.1, Some(evidence(quiet, None)));
    }
    let waiting = h.blocks(35, 0.1, Some(evidence(40, None)));
    assert_eq!(dispatched(&waiting), 0, "still waiting for them");
    assert!(h.events("local_endpoint").is_empty());
    let clear = h.blocks(6, 0.9, Some(evidence(40, None)));
    let accepted = clear[5].accepted.as_ref().expect("the person was let in");
    assert_eq!(accepted.trigger_sequence, 20 + ONSET_SEQUENCE);
    // One request: the word, the pause and what came after it.
    assert!(accepted.audio.windows(2).all(|pair| pair[1] == pair[0] + 1));
    assert_eq!(*accepted.audio.last().unwrap(), 20 + ONSET_SEQUENCE + 70);
    assert_eq!(h.events("duplex_commit").len(), 1);
    let finished = h.events("turn_finished");
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0]["outcome"], "user_interrupted");
    assert_ne!(h.old(), old);
}

#[test]
fn a_short_word_heard_plainly_stops_the_reply_though_nothing_follows_it() {
    // "Stop.", 200 ms, of which the render explains a tenth. Then nobody.
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    let old = h.old();
    h.blocks(20, 0.99, Some(evidence(0, OWN)));
    let word = h.blocks(20, 0.9, Some(evidence(0, Some((10, 0)))));
    assert_eq!(dispatched(&word), ONSET - 1);
    let mut after = Vec::new();
    for quiet in 1..=29 {
        after.push(h.block(0.1, Some(evidence(quiet, None))));
    }
    after.extend(h.blocks(40, 0.1, Some(evidence(40, None))));
    // Nothing more of the reply was handed to the speaker, and at the block
    // that would have resumed it the word was taken as an interruption.
    assert_eq!(dispatched(&after), 0);
    assert!(after[..68].iter().all(|tick| tick.accepted.is_none()));
    let accepted = after[68].accepted.as_ref().expect("the word was let in");
    assert_eq!(accepted.trigger_sequence, 20 + ONSET_SEQUENCE);
    assert!(accepted.audio.windows(2).all(|pair| pair[1] == pair[0] + 1));
    assert!(accepted.audio[0] <= 21);
    assert_eq!(h.events("duplex_commit").len(), 1);
    assert!(h.events("duplex_resume").is_empty());
    let finished = h.events("turn_finished");
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0]["outcome"], "user_interrupted");
    assert_ne!(h.old(), old);
    assert!(h.offer_speaker().is_none());
    // The same word with the lamp's voice half of every block is the
    // acknowledgment it always was.
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    let old = h.old();
    h.blocks(20, 0.99, Some(evidence(0, OWN)));
    h.blocks(20, 0.9, Some(evidence(0, OVER)));
    for quiet in 1..=29 {
        h.block(0.1, Some(evidence(quiet, None)));
    }
    h.blocks(40, 0.1, Some(evidence(40, None)));
    assert_eq!(h.events("duplex_resume").len(), 1);
    assert!(h.events("turn_finished").is_empty());
    assert_eq!(h.old(), old);
}

#[test]
fn a_stop_heard_by_the_listener_ends_the_reply_though_only_quiet_follows() {
    // `lamp-name` hears "Stop" over the lamp and says so on `stop/heard`
    // (#828), on a real bus in a temporary directory. The session loop's
    // own pieces are taken in its order: the hearing forces a yield while
    // the reply sounds, the quiet after it commits the yield, and the
    // committed stop cancels the reply as the hand's Stop does. The loop
    // itself reads `LAMP_BUS` and `LAMP_STOP_HEARD` from the environment,
    // which a test cannot set safely beside others running.
    let root = std::path::PathBuf::from(format!("/tmp/lvc-stop-heard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let bus = lamp_bus::Bus::new(&root).unwrap();
    let mut stop_in = crate::bus_out::NameIn::on_topic(&root, lamp_msgs::StopHeard::TOPIC).unwrap();
    let mut says = bus
        .publisher_declared(
            lamp_msgs::StopHeard::TOPIC,
            &lamp_msgs::StopHeard::declaration(),
            "name",
        )
        .unwrap();
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    let old = h.old();
    h.blocks(20, 0.99, Some(evidence(0, OWN)));
    says.publish_json(
        1,
        &lamp_msgs::StopHeard {
            score: 0.95,
            ago_ms: 0,
        },
    )
    .unwrap();
    // The datagram is in the voice's socket before it looks.
    std::thread::sleep(Duration::from_millis(20));
    let clock = monotonic_us();
    let (began, score) = stop_in.newest_scored(clock).expect("the stop was heard");
    let sounding = reply_sounding(&h.mask, &h.reply, clock);
    let yields = sounding && h.mask.force_yield();
    record(
        &mut h.trace,
        json!({"kind":"stop_heard","at_us":clock,"began_us":began,
            "score":score,"sounding":sounding,"yields":yields}),
    )
    .unwrap();
    // Then nobody speaks while the lamp's sound dies away, and after it.
    let mut after = vec![h.block(0.99, Some(evidence(0, OWN)))];
    let mut committed = false;
    for quiet in 1..=69u8 {
        after.push(h.block(0.1, Some(evidence(quiet.min(40), None))));
        if h.mask.take_stop_committed() {
            committed = true;
            break;
        }
    }
    assert!(committed, "the forced yield never committed");
    // The yield began at the first block after the hearing, and the
    // speaker was offered nothing until the block that committed it.
    let yielded = h.events("duplex_yield");
    assert_eq!(yielded.len(), 1);
    assert_eq!(yielded[0]["frame_sequence"], 21);
    assert_eq!(dispatched(&after[1..after.len() - 1]), 0);
    assert_eq!(h.events("duplex_commit").len(), 1);
    assert!(h.events("duplex_resume").is_empty());
    let heard = h.events("stop_heard");
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0]["yields"], true);
    // The loop's cancellation for a spoken stop.
    let observed_at_us = monotonic_us();
    let cancellation = hand::cancel(
        &mut h.owner,
        &mut h.reply,
        &mut h.parent.control,
        &mut h.pending,
        &mut h.provider_queue,
        &mut h.admission,
        &mut h.detector,
        false,
        observed_at_us,
        monotonic_us,
    )
    .unwrap();
    record_cancelled_reply(
        cancellation.reply.expect("the reply was cut"),
        &mut h.trace,
        crate::receipt_history::SPOKEN_STOP,
        cancellation.speaker_told_at_us,
    )
    .unwrap();
    assert!(h.reply.is_none());
    assert_eq!(cancellation.cancelled_turn, Some(old.turn()));
    let finished = h.events("turn_finished");
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0]["turn"], json!(old.turn()));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_lamps_own_first_word_does_not_stop_it_or_become_a_turn() {
    // The start of a reply as lamp-4ace heard one: two blocks the render
    // explains, then twenty-five it does not, all scored as speech.
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    h.mask = PlaybackMask::duplex();
    let old = h.old();
    let mut ticks = h.blocks(2, 0.99, Some(evidence(0, OWN)));
    ticks.extend(h.blocks(25, 0.95, Some(evidence(0, Some((0, 0))))));
    ticks.extend(h.blocks(40, 0.99, Some(evidence(0, OWN))));
    assert_eq!(
        dispatched(&ticks),
        ticks.len(),
        "the speaker was kept waiting"
    );
    assert!(ticks.iter().all(|tick| tick.accepted.is_none()));
    assert!(h.events("duplex_yield").is_empty());
    assert!(h.events("input_candidate").is_empty());
    assert_eq!(h.old(), old);
    // With its sound found, the same blocks are a person over it.
    h.blocks(ONSET, 0.95, Some(evidence(0, Some((0, 0)))));
    assert_eq!(h.events("duplex_yield").len(), 1);
}

#[test]
fn a_yield_that_is_never_settled_is_given_up_at_its_deadline() {
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    let old = h.old();
    h.blocks(ONSET, 0.9, Some(evidence(0, OVER)));
    assert_eq!(h.events("duplex_yield").len(), 1);
    // The evidence goes on saying the lamp is sounding: no silence to confirm
    // a person in, so no commitment can be made.
    let waiting = h.blocks(109, 0.9, Some(evidence(0, OVER)));
    assert_eq!(dispatched(&waiting), 0);
    assert!(h.events("duplex_resume").is_empty());
    let last = h.block(0.9, Some(evidence(0, OVER)));
    assert!(last.dispatched.is_some() && last.accepted.is_none());
    assert_eq!(
        h.events("duplex_resume")[0]["frame_sequence"],
        ONSET_SEQUENCE + 110
    );
    assert_eq!(h.old(), old);
    assert!(h.events("turn_finished").is_empty());
    // The held candidate ended as a policy rejection inside its own budget,
    // not as a missed deadline that would drain the microphone.
    assert!(!h.admission.owns_stream());
    let rejected = h.events("input_candidate_rejected");
    assert!(
        rejected
            .iter()
            .all(|event| event["reason"] != "deadline" && event["reason"] != "capacity"),
        "{rejected:?}"
    );
}

#[test]
fn the_standard_hold_cannot_keep_a_candidate_through_a_yield() {
    // Why duplex widens the decision budget: with the standard one the same
    // person is turned away for capacity before the lamp has fallen silent.
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    h.admission = InputAdmission::default();
    let old = h.old();
    h.blocks(20, 0.99, Some(evidence(0, OWN)));
    h.blocks(9, 0.9, Some(evidence(0, OVER)));
    let mut ticks = Vec::new();
    for quiet in 1..=29 {
        ticks.push(h.block(0.9, Some(evidence(quiet, OVER))));
    }
    ticks.extend(h.blocks(6, 0.9, Some(evidence(30, None))));
    assert!(ticks.iter().all(|tick| tick.accepted.is_none()));
    assert_eq!(h.old(), old);
    assert!(
        h.events("input_candidate_rejected")
            .iter()
            .any(|event| event["reason"] == "capacity")
    );
}

#[test]
fn a_reply_that_twice_yields_to_nothing_masks_like_the_baseline_until_it_ends() {
    let mut h = Harness::new(TurnTaking::Duplex, 600);
    let old = h.old();
    for _ in 0..2 {
        h.blocks(ONSET + 2, 0.9, Some(evidence(0, OVER)));
        for quiet in 1..=29 {
            h.block(0.1, Some(evidence(quiet, None)));
        }
        h.blocks(40, 0.1, Some(evidence(40, None)));
        h.blocks(20, 0.99, Some(evidence(0, OWN)));
    }
    assert_eq!(h.mask.duplex_summary(), Some(((2, 0, 2), true)));
    // The same evidence a third time: the reply plays on and no one is let in.
    let candidates = h.events("input_candidate").len();
    let ticks = h.blocks(40, 0.9, Some(evidence(0, OVER)));
    assert_eq!(dispatched(&ticks), 40);
    assert!(ticks.iter().all(|tick| tick.accepted.is_none()));
    assert_eq!(h.events("input_candidate").len(), candidates);
    assert_eq!(h.events("duplex_yield").len(), 2);
    assert_eq!(h.old(), old);
}

#[test]
fn without_echo_evidence_duplex_decides_as_the_conservative_baseline_does() {
    // The same blocks to both policies, from a worker that reports nothing.
    let mut outcomes = Vec::new();
    for policy in [TurnTaking::Duplex, TurnTaking::V1Baseline] {
        let mut h = Harness::new(policy, 400);
        let old = h.old();
        let mut ticks = h.blocks(20, 0.2, None);
        ticks.extend(h.blocks(40, 0.99, None));
        assert!(ticks.iter().all(|tick| tick.accepted.is_none()));
        assert_eq!(dispatched(&ticks), 60);
        assert_eq!(h.old(), old);
        assert!(h.events("duplex_yield").is_empty());
        outcomes.push((
            h.mask.suppressed_blocks(),
            h.events("input_candidate").len(),
            h.events("input_candidate_rejected").len(),
        ));
    }
    assert_eq!(outcomes[0], outcomes[1]);
    assert!(outcomes[0].0 > 0, "nothing was masked: {outcomes:?}");
}

#[test]
fn a_microphone_that_stops_arriving_does_not_hold_the_reply_back() {
    // Stale on the host's clock: the wait below is the host's.
    let mut h = Harness::on_the_host(TurnTaking::Duplex, 400);
    h.blocks(ONSET, 0.9, Some(evidence(0, OVER)));
    assert!(h.offer_speaker().is_none(), "yielded");
    // Muted or stalled: no further block is heard.
    std::thread::sleep(Duration::from_micros(HEARING_STALE_US + 20_000));
    assert!(h.offer_speaker().is_some());
}

#[test]
fn a_person_who_speaks_once_the_lamp_is_silent_is_an_ordinary_turn() {
    let mut h = Harness::new(TurnTaking::Duplex, 20);
    // The reply plays out and its sound dies away.
    h.blocks(20, 0.99, Some(evidence(0, OWN)));
    for quiet in 1..=29 {
        h.block(0.1, Some(evidence(quiet, None)));
    }
    h.blocks(10, 0.1, Some(evidence(40, None)));
    // Then speech in the clear: six blocks and it is admitted, as ever.
    let ticks = h.blocks(usize::from(START_BLOCKS), 0.95, Some(evidence(50, None)));
    assert!(ticks[..5].iter().all(|tick| tick.accepted.is_none()));
    let accepted = ticks[5].accepted.as_ref().unwrap();
    assert_eq!(accepted.basis, AcceptanceBasis::DirectedSessionDuplex);
    assert!(h.events("duplex_yield").is_empty());
}

// Independent review probes. Production source is unchanged; this module
// calls the existing hearing, retention and admission methods directly.
fn corrected_prefix_after_duplex_hold(
    missing_prefix_evidence: bool,
    last_sequence: u64,
) -> (Result<Option<Accepted>>, SessionState) {
    const BASE: u64 = 1_000_000;
    /// The reply starts at block 31 and the person with it.
    const YIELDED: u64 = 30 + ONSET_SEQUENCE;
    let instant = |micros| MonoTime::from_micros(micros);
    let mut owner = Controller::new(new_boot().unwrap(), instant(BASE));
    owner
        .set_microphone_permission(instant(BASE), Permission::Allowed)
        .unwrap();
    owner
        .set_capture(
            instant(BASE),
            CaptureState::RetainingUntil(instant(BASE + 100_000)),
        )
        .unwrap();
    owner
        .set_admission(
            instant(BASE),
            AdmissionState::OpenUntil(instant(BASE + 100_000)),
        )
        .unwrap();
    let old = owner.admit(instant(BASE), AdmittedInput::NewTurn).unwrap();
    let snapshot = owner.snapshot(instant(BASE)).unwrap();
    let lineage = CaptureLineage {
        worker: new_boot().unwrap(),
        epoch: 1,
        dsp_epoch: 1,
        privacy_generation: snapshot.microphone_generation(),
    };
    let mut mask = PlaybackMask::duplex();
    let mut detector = TurnDetector::v1_baseline();
    let mut admission = InputAdmission::with_hold(Hold::DUPLEX);
    let mut trace = SessionState::default();
    let mut last = BASE;
    for sequence in 1..=last_sequence {
        let at = BASE + sequence * 10_000;
        if sequence == 31 {
            mask.dispatched(old, 1, at, 240).unwrap();
            mask.accepted(
                old,
                1,
                PlaybackCursor {
                    epoch: 1,
                    frame: 480,
                },
                PlaybackCursor {
                    epoch: 1,
                    frame: 720,
                },
                240,
                at,
                at,
                at,
            )
            .unwrap();
        }
        owner
            .set_capture(
                instant(at),
                CaptureState::RetainingUntil(instant(at + 100_000)),
            )
            .unwrap();
        owner
            .set_admission(
                instant(at),
                AdmissionState::OpenUntil(instant(at + 100_000)),
            )
            .unwrap();
        let context = AdmissionContext {
            capture: lineage,
            authority: owner.snapshot(instant(at)).unwrap(),
        };
        let (probability, echo) = if sequence <= 30 {
            (0.1, (!missing_prefix_evidence).then(|| evidence(64, None)))
        } else if sequence <= YIELDED {
            (0.9, Some(evidence(0, OVER)))
        } else if sequence <= last_sequence - 6 {
            (
                0.9,
                Some(evidence((sequence - YIELDED).min(29) as u8, None)),
            )
        } else {
            (0.9, Some(evidence(30, None)))
        };
        if sequence == 31 {
            mask.assume_matched();
        }
        let heard = mask
            .listen(sequence, probability, echo.as_ref(), sequence >= 31, at)
            .unwrap();
        let observation = ObservedAudio {
            sequence,
            captured_at_us: at,
            samples: [sequence as i16; 160],
        };
        let (update, _) = observe_prospective_input(
            &mut mask,
            &mut detector,
            &mut admission,
            context,
            observation,
            last,
            heard.score,
            at,
        )
        .unwrap();
        assert!(update.rejection.is_none());
        last = at;
        if sequence == last_sequence {
            assert_eq!(heard.step, DuplexStep::Commit);
            let (candidate, _) = admission
                .pending_evidence(at, AcceptanceBasis::DirectedSessionDuplex)
                .unwrap();
            assert_eq!(candidate.trigger_sequence, YIELDED);
            assert_eq!(
                candidate.decision_deadline_us,
                BASE + YIELDED * 10_000 + Hold::DUPLEX.decision_budget_us
            );
            let result = decide_prospective_input(
                &mask,
                &mut detector,
                &mut admission,
                candidate,
                context,
                at,
                &mut trace,
            );
            assert_eq!(owner.snapshot(instant(at)).unwrap().owner(), Some(old));
            assert!(
                !mask.yielded(at),
                "retired candidate must not keep the reply withheld"
            );
            if missing_prefix_evidence && result.as_ref().is_ok_and(Option::is_none) {
                assert!(admission.pending_audio(candidate.id).is_none());
                assert!(!admission.owns_stream());
                let update = admission
                    .observe(
                        Activity::Continue(ObservedAudio {
                            sequence: last_sequence + 1,
                            captured_at_us: at + 1,
                            samples: [0; 160],
                        }),
                        AdmissionContext {
                            capture: lineage,
                            authority: owner.snapshot(instant(at + 1)).unwrap(),
                        },
                        at + 1,
                    )
                    .unwrap();
                assert!(
                    matches!(update.step, AdmissionStep::None),
                    "reject must drain the same utterance instead of admitting its tail"
                );
            }
            return (result, trace);
        }
    }
    unreachable!()
}

#[test]
fn expired_fallback_prefix_rejects_only_the_candidate_and_preserves_the_reply() {
    let (result, trace) = corrected_prefix_after_duplex_hold(true, 65 + ONSET_SEQUENCE);
    assert!(result.unwrap().is_none());
    let rejected: Vec<_> = trace
        .iter()
        .filter(|e| e["kind"] == "input_candidate_rejected")
        .collect();
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0]["reason"], "retained_history_unavailable");
    assert_eq!(rejected[0]["history_loss"], "expired");
    assert!(
        !trace
            .iter()
            .any(|e| e["kind"] == "input_admitted" || e["kind"] == "turn_finished")
    );
}

#[test]
fn complete_evidence_keeps_one_contiguous_opening_within_the_same_budget() {
    for last in [65 + ONSET_SEQUENCE, 90 + ONSET_SEQUENCE] {
        let (result, trace) = corrected_prefix_after_duplex_hold(false, last);
        let accepted = result.unwrap().unwrap();
        assert_eq!(
            accepted
                .audio
                .iter()
                .map(|f| f.sequence)
                .collect::<Vec<_>>(),
            (1 + ONSET_SEQUENCE..=last).collect::<Vec<_>>()
        );
        assert_eq!(accepted.basis, AcceptanceBasis::DirectedSessionDuplex);
        assert!(
            !trace
                .iter()
                .any(|e| e["kind"] == "input_candidate_rejected")
        );
    }
}

#[test]
fn missing_evidence_closes_the_yield_without_reclassifying_raw_gaps() {
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    let old = h.old();
    h.blocks(ONSET, 0.9, Some(evidence(0, OVER)));
    let start = h.events("duplex_yield")[0].clone();
    assert!(h.block(0.99, None).dispatched.is_none());
    assert!(h.events("duplex_yield_end").is_empty());
    assert!(h.block(0.99, Some(evidence(0, OWN))).dispatched.is_some());
    assert_eq!(h.old(), old);
    let end = h.events("duplex_yield_end");
    assert_eq!(end.len(), 1);
    assert_eq!(end[0]["owner"], serde_json::to_value(old).unwrap());
    assert_eq!(end[0]["started_at_us"], start["at_us"]);
    assert_eq!(end[0]["first_frame_sequence"], ONSET_SEQUENCE);
    assert_eq!(end[0]["ending_frame_sequence"], ONSET_SEQUENCE + 2);
    assert_eq!(
        end[0]["missing_evidence_since_sequence"],
        ONSET_SEQUENCE + 1
    );
    assert_eq!(end[0]["outcome"], "evidence_discontinuity");
    assert_eq!(end[0]["starvation_excluded"], false);
    assert!(end[0]["at_us"].as_u64().unwrap() >= start["at_us"].as_u64().unwrap());
    assert!(h.trace.yield_interval.is_none());
    h.blocks(3, 0.99, Some(evidence(0, OWN)));
    assert_eq!(h.events("duplex_yield_end").len(), 1);
}

#[test]
fn cancellation_closes_only_one_interval_without_resuming_the_old_reply() {
    for reason in [
        "privacy_muted",
        "provider_interrupted",
        "service_stopped",
        "runtime_failed",
    ] {
        let mut h = Harness::new(TurnTaking::Duplex, 400);
        let old = h.old();
        h.blocks(ONSET, 0.9, Some(evidence(0, OVER)));
        cancel_reply(&mut h.owner, &mut h.reply, &mut h.trace, reason).unwrap();
        assert!(h.reply.is_none());
        assert!(h.offer_speaker().is_none());
        let end = h.events("duplex_yield_end");
        assert_eq!(end.len(), 1);
        assert_eq!(end[0]["owner"], serde_json::to_value(old).unwrap());
        assert_eq!(end[0]["outcome"], reason);
        cancel_reply(&mut h.owner, &mut h.reply, &mut h.trace, reason).unwrap();
        finish_duplex_yield(&mut h.trace, monotonic_us(), "session_failed", None, None).unwrap();
        assert_eq!(h.events("duplex_yield_end").len(), 1);
    }
}

#[test]
fn resume_and_commit_decisions_close_intervals_but_only_admission_replaces_ownership() {
    for commit in [false, true] {
        let mut h = Harness::new(TurnTaking::Duplex, 400);
        let old = h.old();
        h.blocks(ONSET, 0.9, Some(evidence(0, OVER)));
        for quiet in 1..=29 {
            h.block(if commit { 0.9 } else { 0.1 }, Some(evidence(quiet, None)));
        }
        let ticks = h.blocks(
            if commit { 6 } else { 40 },
            if commit { 0.9 } else { 0.1 },
            Some(evidence(40, None)),
        );
        let ends = h.events("duplex_yield_end");
        assert_eq!(ends.len(), 1);
        assert_eq!(
            ends[0]["outcome"],
            if commit { "commit_decision" } else { "resumed" }
        );
        assert_eq!(ends[0]["owner"], serde_json::to_value(old).unwrap());
        assert_eq!(h.events("input_admitted").len(), usize::from(commit));
        assert_eq!(
            ticks.iter().filter(|t| t.accepted.is_some()).count(),
            usize::from(commit)
        );
        if commit {
            assert_ne!(h.old(), old);
        } else {
            assert_eq!(h.old(), old);
        }
    }
}

#[test]
fn stale_hearing_closes_observation_without_granting_new_input() {
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    let old = h.old();
    h.blocks(ONSET, 0.9, Some(evidence(0, OVER)));
    let last = h.last_read;
    settle_duplex_yield_before_dispatch(
        &h.mask,
        &h.reply,
        last + crate::turn_taking::HEARING_STALE_US + 1,
        &mut h.trace,
    )
    .unwrap();
    assert_eq!(
        h.events("duplex_yield_end")[0]["outcome"],
        "hearing_expired"
    );
    assert_eq!(h.old(), old);
    assert!(h.events("input_admitted").is_empty());
    // This is metadata for the mask boundary; live capture heartbeat loss still
    // ends the real conversation and is not a successful microphone recovery.
}

#[test]
fn regressing_hearing_time_is_not_a_successful_resume() {
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    h.blocks(ONSET, 0.9, Some(evidence(0, OVER)));
    let at = h.last_read - 1;
    assert!(
        hear_capture_block(
            &mut h.mask,
            &h.reply,
            &mut h.heard_reply,
            ONSET_SEQUENCE + 1,
            at,
            0.1,
            Some(&evidence(40, None)),
            at,
            &mut h.trace
        )
        .is_err()
    );
    assert!(h.events("duplex_yield_end").is_empty());
    assert!(h.trace.yield_interval.is_some());
    // The normal error-exit cleanup supplies the final session failure outcome.
    finish_duplex_yield(&mut h.trace, monotonic_us(), "session_failed", None, None).unwrap();
    assert_eq!(h.events("duplex_yield_end")[0]["outcome"], "session_failed");
}

/// #828, the reviewer on #1291: a line is over the lamp's voice only when
/// the reply in hand had sound or is playing. One only in hand, silent while
/// it waits for its answer, isn't, and nor is no reply.
#[test]
fn the_lamps_voice_is_heard_only_from_a_reply_with_sound() {
    assert!(!lamps_voice_heard(None));
    let (_, mut current, _) = active_reply();
    current.had_audio = false;
    current.playback = None;
    assert!(!lamps_voice_heard(Some(&current)));
    current.had_audio = true;
    assert!(lamps_voice_heard(Some(&current)));
}

#[test]
fn provider_pcm_threshold_keeps_exactly_one_terminal_allowance_in_both_modes() {
    for hold in [Hold::STANDARD, Hold::DUPLEX] {
        let (mut owner, mut current, _) = active_reply();
        let privacy = owner.snapshot(now()).unwrap().microphone_generation();
        let bound = provider_queue_bound(hold);
        assert_eq!(bound, if hold == Hold::STANDARD { 64 } else { 164 });
        let mut queue = VecDeque::new();
        queue.push_back(ProviderInput::Start {
            session: 1,
            request: current.owner.turn(),
            privacy_generation: privacy,
            continues: false,
            continues_cut_off: false,
            over_the_lamp: false,
            after_stop: false,
            follows_on: false,
            over_the_lamps_voice: false,
            heard_of_cut: None,
            soft_lead_us: 0,
        });
        for n in 1..bound {
            queue_input(
                &mut queue,
                &mut current,
                privacy,
                ObservedAudio {
                    sequence: n as u64,
                    captured_at_us: n as u64,
                    samples: [0; 160],
                },
                bound,
            )
            .unwrap();
        }
        assert_eq!(queue.len(), bound);
        let input_sequence = current.input_sequence;
        assert!(
            queue_input(
                &mut queue,
                &mut current,
                privacy,
                ObservedAudio {
                    sequence: bound as u64,
                    captured_at_us: bound as u64,
                    samples: [0; 160]
                },
                bound
            )
            .is_err()
        );
        assert_eq!(current.input_sequence, input_sequence);
        assert_eq!(queue.len(), bound);
        queue_input_end(&mut queue, &current, privacy, bound, false, false).unwrap();
        assert_eq!(queue.len(), bound + 1);
        assert!(
            matches!(queue.back(),Some(ProviderInput::End {request,..}) if *request==current.owner.turn())
        );
        assert!(queue_input_end(&mut queue, &current, privacy, bound, false, false).is_err());
        assert_eq!(queue.len(), bound + 1);
        // A cut-in's End says so (#926); an ordinary one never does.
        let mut cut = VecDeque::new();
        queue_input_end(&mut cut, &current, privacy, bound, false, true).unwrap();
        assert!(matches!(
            cut.back(),
            Some(ProviderInput::End {
                cut_in: true,
                sound: false,
                ..
            })
        ));
        assert!(matches!(
            queue.back(),
            Some(ProviderInput::End { cut_in: false, .. })
        ));
    }
}

#[test]
fn evicted_fallback_prefix_rejects_within_the_original_duplex_deadline() {
    let (result, trace) = corrected_prefix_after_duplex_hold(true, 90 + ONSET_SEQUENCE);
    assert!(result.unwrap().is_none());
    let rejected: Vec<_> = trace
        .iter()
        .filter(|e| e["kind"] == "input_candidate_rejected")
        .collect();
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0]["reason"], "retained_history_unavailable");
    assert_eq!(rejected[0]["history_loss"], "evicted");
}

#[test]
fn invalid_capture_ingress_cannot_take_the_history_loss_recovery_path() {
    for invalid in 0..3 {
        let mut h = Harness::new(TurnTaking::Duplex, 400);
        h.blocks(ONSET, 0.9, Some(evidence(0, OVER)));
        let at = h.at();
        let context = AdmissionContext {
            capture: h.lineage,
            authority: h.owner.snapshot(MonoTime::from_micros(at)).unwrap(),
        };
        let sequence = ONSET_SEQUENCE + if invalid == 0 { 3 } else { 1 };
        let completed = if invalid == 1 { at + 1 } else { at };
        let first = if invalid == 2 {
            h.last_read - 1
        } else {
            h.last_read
        };
        assert!(
            observe_prospective_input(
                &mut h.mask,
                &mut h.detector,
                &mut h.admission,
                context,
                ObservedAudio {
                    sequence,
                    captured_at_us: completed,
                    samples: [0; 160]
                },
                first,
                0.9,
                at
            )
            .is_err()
        );
        assert!(h.events("input_admitted").is_empty());
        assert!(
            !h.events("input_candidate_rejected")
                .iter()
                .any(|e| e["reason"] == "retained_history_unavailable")
        );
    }
}

#[test]
fn lineage_reset_closes_a_yield_and_does_not_lend_it_to_new_capture() {
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    let old = h.old();
    h.blocks(ONSET, 0.9, Some(evidence(0, OVER)));
    let at = h.at();
    clear_capture_lineage(
        &mut h.detector,
        &mut h.mask,
        &mut h.trace,
        at,
        ONSET_SEQUENCE + 1,
        at,
    )
    .unwrap();
    assert!(!h.mask.yielded(at));
    assert!(h.trace.yield_interval.is_none());
    let ends = h.events("duplex_yield_end");
    assert_eq!(ends.len(), 1);
    assert_eq!(ends[0]["outcome"], "capture_lineage_changed");
    assert_eq!(ends[0]["owner"], serde_json::to_value(old).unwrap());
    assert!(h.events("input_admitted").is_empty());
}

#[test]
fn clearing_observation_storage_does_not_erase_the_open_yield_identity() {
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    let old = h.old();
    h.blocks(ONSET, 0.9, Some(evidence(0, OVER)));
    let start = h.trace.yield_interval.unwrap();
    // The real service periodically flushes/removes observation rows. This
    // injection also deletes the start row; the bounded slot is independent.
    h.trace.clear();
    h.block(0.99, None);
    h.block(0.99, Some(evidence(0, OWN)));
    let ends = h.events("duplex_yield_end");
    assert_eq!(ends.len(), 1);
    assert_eq!(ends[0]["started_at_us"], start.started_at_us);
    assert_eq!(ends[0]["owner"], serde_json::to_value(old).unwrap());
    assert_eq!(ends[0]["starvation_excluded"], false);
    assert_eq!(h.old(), old);
}

/// Capture blocks a second on the scenario clock (2 ms a block), against
/// which the fruitless yields are forgiven.
const BLOCKS_A_SECOND: usize = 500;

/// A long story with two asides over it early on, each a yield the lamp
/// went on after ("What was that?"), as on lamp-4ace on 20 October; then
/// the story plays on to `gap_s` after the second aside began.
fn story_after_two_asides(keeps_listening: bool, gap_s: usize) -> Harness {
    let mut h = Harness::new(TurnTaking::Duplex, 40_000);
    if keeps_listening {
        h.mask.keep_listening();
    }
    yields_over_a_story(&mut h, 2, &[(ONSET + 2, OVER)], (0, 0.0, None), gap_s);
    let (counts, fallen_back) = h.mask.duplex_summary().unwrap();
    assert_eq!(counts, (2, 0, 2));
    assert_eq!(
        fallen_back,
        gap_s * 1_000_000 < FORGIVE_FRUITLESS_US as usize
    );
    h
}

/// `count` yields over a long story that the lamp went on after: each `over`'s
/// runs of blocks heard over the lamp with their shares, then `after` (that
/// many blocks at that score and shares, the lamp still sounding), then
/// its sound dies away and stillness resumes it. They are 2 s apart, and
/// the last is `gap_s` before the story's end here.
fn yields_over_a_story(
    h: &mut Harness,
    count: usize,
    over: &[(usize, Option<(u8, u8)>)],
    after: (usize, f32, Option<(u8, u8)>),
    gap_s: usize,
) {
    h.blocks(200, 0.99, Some(evidence(0, OWN)));
    for aside in 0..count {
        for &(blocks, shares) in over {
            h.blocks(blocks, 0.9, Some(evidence(0, shares)));
        }
        h.blocks(after.0, after.1, Some(evidence(0, after.2)));
        for quiet in 1..=29 {
            h.block(0.1, Some(evidence(quiet, None)));
        }
        h.blocks(40, 0.1, Some(evidence(40, None)));
        let spent = over.iter().map(|run| run.0).sum::<usize>() + after.0 + 29 + 40;
        let story = if aside + 1 < count {
            2 * BLOCKS_A_SECOND
        } else {
            gap_s * BLOCKS_A_SECOND
        };
        h.blocks(story - spent, 0.99, Some(evidence(0, OWN)));
    }
}

/// Someone speaks `blocks` blocks over the story at `score`. Its sound dies
/// away only once the lamp has stopped for them. Then, if they stopped it,
/// they go quiet until it goes on; whether it was let in is returned.
fn a_line_over_the_story(h: &mut Harness, blocks: usize) -> bool {
    let mut quiet: u8 = 0;
    let mut accepted = false;
    for _ in 0..blocks {
        let shares = if quiet >= 30 { None } else { OVER };
        let tick = h.block(0.9, Some(evidence(quiet, shares)));
        accepted |= tick.accepted.is_some();
        if accepted {
            return true;
        }
        if tick.dispatched.is_none() || quiet > 0 {
            quiet = quiet.saturating_add(1);
        }
    }
    if quiet > 0 {
        for _ in 0..80 {
            let shares = if quiet >= 30 { None } else { Some((10, 0)) };
            h.block(0.1, Some(evidence(quiet, shares)));
            quiet = quiet.saturating_add(1).min(40);
        }
    }
    false
}

/// #2602, "Cutting in" (the first builder at 04:13, 22 Oct): with
/// `LAMP_CUT_IN_KEEPS_LISTENING=on` a person who tries a third time, soon
/// after two asides the lamp went on after, still stops the story, as a
/// person stops for someone who keeps trying; and from 20 s on as before.
#[test]
fn with_keeps_listening_a_third_line_soon_after_two_asides_stops_the_story() {
    for gap_s in [5, 10, 30] {
        let mut h = story_after_two_asides(true, gap_s);
        let old = h.old();
        assert!(a_line_over_the_story(&mut h, 200), "{gap_s} s after");
        assert_eq!(h.events("duplex_yield").len(), 3, "{gap_s} s after");
        assert_eq!(h.events("duplex_commit").len(), 1, "{gap_s} s after");
        assert_ne!(h.old(), old, "{gap_s} s after: the story went on");
        assert_eq!(h.events("turn_finished")[0]["outcome"], "user_interrupted");
    }
}

/// The switch off: the old rule. Within 20 s of the second aside the third
/// line makes no yield and no candidate, and the story plays on to the end
/// of it; 21 s on, the forgiven yield lets it in.
#[test]
fn without_keeps_listening_a_third_line_within_twenty_seconds_is_not_heard() {
    for (gap_s, heard) in [(5, false), (19, false), (21, true)] {
        let mut h = story_after_two_asides(false, gap_s);
        let old = h.old();
        let candidates = h.events("input_candidate").len();
        assert_eq!(a_line_over_the_story(&mut h, 200), heard, "{gap_s} s after");
        assert_eq!(h.old() != old, heard, "{gap_s} s after");
        if !heard {
            assert_eq!(h.events("duplex_yield").len(), 2, "{gap_s} s after");
            assert_eq!(h.events("input_candidate").len(), candidates);
        }
    }
}

/// With the switch, the lamp's own sound is never a cut-in: its voice the
/// render explains, and a voice it can't explain yet because the lamp's
/// sound has not been found again since it went on, however long either
/// lasts. A word or a cough over the story is not one either.
#[test]
fn with_keeps_listening_the_lamps_own_voice_is_never_a_cut_in() {
    let mut h = story_after_two_asides(true, 5);
    let old = h.old();
    let candidates = h.events("input_candidate").len();
    let ticks = h.blocks(3_000, 0.99, Some(evidence(0, OWN)));
    assert_eq!(dispatched(&ticks), 3_000);
    // A word over the story, 250 ms: shorter than one who keeps going.
    let mut ticks = h.blocks(25, 0.9, Some(evidence(0, OVER)));
    ticks.extend(h.blocks(500, 0.99, Some(evidence(0, OWN))));
    assert_eq!(dispatched(&ticks), 525);
    assert_eq!(h.events("duplex_yield").len(), 2);
    assert!(ticks.iter().all(|tick| tick.accepted.is_none()));
    assert_eq!(h.old(), old);
    // A third aside the lamp goes on after: its own sound starts unmatched.
    // One harness at a time: each holds the scenario clock.
    drop(h);
    let mut h = story_after_two_asides(true, 5);
    let old = h.old();
    assert!(!a_line_over_the_story(&mut h, 60));
    assert_eq!(h.events("duplex_resume").len(), 3);
    let ticks = h.blocks(1_000, 0.95, Some(evidence(0, Some((0, 0)))));
    assert_eq!(dispatched(&ticks), 1_000, "the lamp yielded to itself");
    assert_eq!(h.events("duplex_yield").len(), 3);
    assert!(ticks.iter().all(|tick| tick.accepted.is_none()));
    assert_eq!(h.events("input_candidate").len(), candidates + 1);
    assert_eq!(h.old(), old);
}

/// With the switch, a voice over the story that keeps going and is never
/// a line (a television) pauses it at most twice more before 20 s forgives
/// one: after that the story falls back as it did without the switch.
#[test]
fn with_keeps_listening_a_voice_that_is_never_a_line_pauses_the_story_twice_more() {
    let mut h = story_after_two_asides(true, 3);
    let old = h.old();
    for _ in 0..4 {
        assert!(!a_line_over_the_story(&mut h, 60));
        h.blocks(BLOCKS_A_SECOND, 0.99, Some(evidence(0, OWN)));
    }
    assert_eq!(h.events("duplex_yield").len(), 2 + 2);
    assert_eq!(h.events("duplex_resume").len(), 2 + 2);
    assert_eq!(h.old(), old);
    assert!(h.events("turn_finished").is_empty());
}

/// A person's short aside, heard mostly plainly: eight blocks the render
/// explains a tenth of, then four it explains 40% of as the lamp's sound
/// dies away. Fewer than ten plain blocks, so stillness resumes it
/// (`Confirmation::plain_blocks`); a person outnumbers the lamp's own voice.
const PLAIN_ASIDE: &[(usize, Option<(u8, u8)>)] = &[(8, Some((10, 0))), (4, OVER)];

/// A story with two yields over it, with `LAMP_CUT_IN_COUNTS_REAL` as
/// `counts_real` says; then a third line 5 s after the second, and whether
/// it was let in.
fn a_third_line_after(
    counts_real: bool,
    over: &[(usize, Option<(u8, u8)>)],
    after: (usize, f32, Option<(u8, u8)>),
) -> (bool, Harness) {
    let mut h = Harness::new(TurnTaking::Duplex, 40_000);
    if counts_real {
        h.mask.count_real_cut_ins();
    }
    yields_over_a_story(&mut h, 2, over, after, 5);
    assert_eq!(h.mask.duplex_summary().unwrap().0, (2, 0, 2));
    let heard = a_line_over_the_story(&mut h, 200);
    (heard, h)
}

/// #2602, "Cutting in" (Seventh's design, Third's 08:13): with
/// `LAMP_CUT_IN_COUNTS_REAL=on`, two asides heard plainly over the story
/// are a person, not the lamp's own voice, and cost no fruitless yield: a
/// third line 5 s after the second commits and stops the story. Off, as
/// today, it is not heard at all.
#[test]
fn with_counts_real_two_plain_asides_leave_a_third_line_heard() {
    let (heard, h) = a_third_line_after(true, PLAIN_ASIDE, (0, 0.0, None));
    assert!(heard, "on: the third line was not heard");
    assert_eq!(h.events("duplex_yield").len(), 3);
    assert_eq!(h.events("duplex_commit").len(), 1);
    assert_eq!(h.events("turn_finished")[0]["outcome"], "user_interrupted");
    drop(h);
    let (heard, h) = a_third_line_after(false, PLAIN_ASIDE, (0, 0.0, None));
    assert!(!heard, "off: the third line was heard");
    assert_eq!(h.events("duplex_yield").len(), 2);
    assert!(h.events("turn_finished").is_empty());
}

/// With the switch, the lamp's own voice twice still trips the cap: two
/// yields whose windows the render mostly explains (an onset run 40%
/// explained, then the lamp's own voice let through) count, and a third
/// line within 20 s makes no yield and no candidate.
#[test]
fn with_counts_real_the_lamps_own_voice_twice_still_falls_back() {
    let (heard, h) = a_third_line_after(true, &[(ONSET + 2, OVER)], (20, 0.99, OWN));
    assert!(!heard);
    assert_eq!(h.events("duplex_yield").len(), 2);
    assert!(h.mask.duplex_summary().unwrap().1, "not fallen back");
    assert!(h.events("turn_finished").is_empty());
}

/// With the switch, `Unknown` is no evidence of the lamp's own voice
/// (Third on #2602): two yields whose onset runs were heard mostly plainly, and
/// whose windows then held 60 blocks of speech with no alignment to judge
/// by, more than all the rest, do not count, and a third line commits. The
/// design note's first rule ("Own or Unknown dominated") would have counted
/// them and left it unheard.
#[test]
fn with_counts_real_unknown_alone_is_not_the_lamps_own_voice() {
    let (heard, h) = a_third_line_after(true, PLAIN_ASIDE, (60, 0.9, None));
    assert!(heard, "the third line was not heard");
    assert_eq!(h.events("duplex_commit").len(), 1);
}

/// With the switch, a sound that is never a line (a television's short
/// bursts, heard mostly plainly, then voice with nothing to judge it by)
/// is no fruitless yield of the lamp's own voice, but it has a bound of its
/// own (`OTHER_FRUITLESS_YIELDS`, the Reviewer-lead's P2 on #2779): after
/// three that resume, a fourth within 20 s makes no yield and no candidate.
#[test]
fn with_counts_real_a_sound_that_is_never_a_line_stops_pausing_after_three() {
    let mut h = Harness::new(TurnTaking::Duplex, 40_000);
    h.mask.count_real_cut_ins();
    yields_over_a_story(&mut h, 3, PLAIN_ASIDE, (20, 0.9, None), 3);
    assert_eq!(h.mask.duplex_summary().unwrap(), ((3, 0, 3), true));
    let old = h.old();
    let candidates = h.events("input_candidate").len();
    assert!(!a_line_over_the_story(&mut h, 200));
    assert_eq!(h.events("duplex_yield").len(), 3);
    assert_eq!(h.events("input_candidate").len(), candidates);
    assert_eq!(h.old(), old);
}
