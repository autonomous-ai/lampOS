//! Duplex hearing through the production hearing, observation, decision,
//! commit and speaker-dispatch functions, one capture block at a time.
//!
//! Time here is the host's own and the blocks follow one another at once, so
//! every assertion is about order and counts of blocks; a paced test runs on
//! the scenario clock instead, 40 ms a block
//! ([`Harness::keeping_first_words_paced`]). The bounds that are measured in
//! time are tested where they live: the decision budget in `admission`, the
//! tail and the staleness of hearing in `turn_taking`.
use super::*;
use crate::{activity::START_BLOCKS, admission::Hold, turn_taking::HEARING_STALE_US};
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
    /// `LAMP_REPLY_END_REACH=on` (#1722), as the session loop reads it.
    pub(super) reach_past_reply: bool,
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
    pub(super) fn new(policy: TurnTaking, blocks: usize) -> Self {
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
            reach_past_reply: false,
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
        if self.gap_us > 0 {
            // A paced run outlasts the half second the reply's capture and
            // admission were opened for: renewed each block, as the session
            // loop renews them each tick.
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
                PlayoutStart::default(),
                &mut self.trace,
                |_| Ok(()),
            )
            .unwrap();
        }
        if self.gap_us > 0 {
            // A paced run lasts longer than the snapshot taken at the start
            // stays fresh: the speaker is offered the one of this block.
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
/// asked for. How many blocks they were given.
struct Ears {
    words: Option<&'static str>,
    asked: bool,
    heard: std::sync::Arc<std::sync::atomic::AtomicUsize>,
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
        self.asked
            .then(|| self.words.take())
            .flatten()
            .map(str::to_owned)
    }
    fn forget(&mut self) {}
    fn rest(&mut self) {}
}

/// #828, step 2, through the production hearing, yield and commit, with
/// `LAMP_STOP_WORDS=heard`: a short word over the lamp yields and is over
/// before the lamp is silent, as on lamp-4ace at 05:39. Its words a stop
/// phrase, the yield commits and the reply ends, with the word as the new
/// request. Other words resume it as stillness did before. No words resume
/// it at the deadline. The block it settled at, how many blocks the words
/// were given, and whether it committed.
fn a_short_word_with(words: Option<&'static str>) -> (u64, usize, bool) {
    let heard = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut h = Harness::new(TurnTaking::Duplex, 400);
    h.stop_words = Some(crate::stop_words::StopWords::new(Box::new(Ears {
        words,
        asked: false,
        heard: std::sync::Arc::clone(&heard),
    })));
    let old = h.old();
    h.blocks(20, 0.99, Some(evidence(0, OWN)));
    // "Stop.", 120 ms over the lamp.
    h.blocks(ONSET + 2, 0.9, Some(evidence(0, OVER)));
    let yielded_at = h.events("duplex_yield")[0]["frame_sequence"]
        .as_u64()
        .unwrap();
    for quiet in 1..=29 {
        h.block(0.1, Some(evidence(quiet, None)));
    }
    let mut settled = None;
    for _ in 0..120 {
        h.block(0.1, Some(evidence(40, None)));
        if let Some(at) = h
            .events("duplex_commit")
            .first()
            .or(h.events("duplex_resume").first())
            .and_then(|event| event["frame_sequence"].as_u64())
        {
            settled = Some(at - yielded_at);
            break;
        }
    }
    let committed = !h.events("duplex_commit").is_empty();
    if committed {
        assert_eq!(h.events("turn_finished")[0]["outcome"], "user_interrupted");
        assert_ne!(h.old(), old);
    } else {
        assert_eq!(h.old(), old);
        assert!(h.events("turn_finished").is_empty());
    }
    (
        settled.expect("the yield never settled"),
        heard.load(std::sync::atomic::Ordering::Relaxed),
        committed,
    )
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
    let mut h = Harness::new(TurnTaking::Duplex, 400);
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
