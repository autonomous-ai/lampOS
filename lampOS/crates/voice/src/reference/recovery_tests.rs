//! Recovery in virtual time, through the production calls: `prime`,
//! `started`, `accept` and `capture_step`, with the real canceller.
use super::*;
use crate::{
    reference::PRIME_FRAMES,
    wire::{ClockPrimed, ReferenceFault, ReferenceTiming},
};
use lamp_audio::ownership::QueuedRange;
use lamp_turn_owner::{
    AdmissionState, AdmittedInput, BootId, CaptureState, Controller, MonoTime, Permission,
    TurnOwner,
};

const START: u64 = 1_000_000;

fn owner() -> TurnOwner {
    let at = MonoTime::from_micros(START);
    let mut controller = Controller::new(BootId::new([1; 16]).unwrap(), at);
    controller
        .set_microphone_permission(at, Permission::Allowed)
        .unwrap();
    let until = MonoTime::from_micros(START + 900_000);
    controller
        .set_capture(at, CaptureState::RetainingUntil(until))
        .unwrap();
    controller
        .set_admission(at, AdmissionState::OpenUntil(until))
        .unwrap();
    controller.admit(at, AdmittedInput::NewTurn).unwrap()
}

/// What becomes of the packet the speaker sent.
#[derive(Clone, Copy, PartialEq)]
enum Sent {
    Arrives,
    Lost,
    Twice,
}

/// One session: a speaker `skew_ppm` fast sending a packet a block, and a
/// capture loop reading a block every 10 ms, half a block out of phase.
struct Session {
    skew_ppm: f64,
    blocks: u64,
    /// The lamp is speaking throughout, not keeping its clock with zeros.
    sound: bool,
    sent: Box<dyn FnMut(u64) -> Sent>,
    /// How long after its turn the speaker accepted packet `n`.
    sender_late: Box<dyn Fn(u64) -> u64>,
    /// How long after its time the capture loop reached block `n`.
    capture_late: Box<dyn Fn(u64) -> u64>,
    /// The capture block from which the worker's privacy generation moved on.
    privacy_at: Option<u64>,
    /// The capture block before which the speaker restarts its clock.
    restart_at: Option<u64>,
    /// How long before the session began the speaker was primed and started
    /// its clock: the worker was held up and took the start notice that
    /// much late.
    start_taken_late_us: u64,
    /// The detector's score for every capture block: 0.9 is someone
    /// speaking throughout, 0.0 an empty room.
    score: f32,
}

#[derive(Debug)]
struct Run {
    blocks: u64,
    fault: Option<Kind>,
    /// Capture block and status, as they were reported.
    statuses: Vec<(u64, ReferenceStatus)>,
    /// Capture blocks that did not pass the canceller.
    bypassed: Vec<u64>,
    reseeds: u32,
    /// Capture blocks whose evidence would have been given out.
    trusted_blocks: u64,
    timing: ReferenceTiming,
}

impl Run {
    fn causes(&self) -> Vec<(Trust, Option<Cause>)> {
        self.statuses
            .iter()
            .map(|(_, status)| (status.trust, status.cause))
            .collect()
    }
}

/// A hold-up of `stall_us` at `at`, worked through without waiting: each
/// thing owed follows 200 us after the one before until it is back on time.
fn stall(at: u64, stall_us: u64) -> Box<dyn Fn(u64) -> u64> {
    Box::new(move |n| match n.checked_sub(at) {
        Some(behind) => (stall_us + behind * 200).saturating_sub(behind * 10_000),
        None => 0,
    })
}

fn both(
    first: Box<dyn Fn(u64) -> u64>,
    second: Box<dyn Fn(u64) -> u64>,
) -> Box<dyn Fn(u64) -> u64> {
    Box::new(move |n| first(n).max(second(n)))
}

impl Session {
    fn new(skew_ppm: f64, blocks: u64) -> Self {
        Self {
            skew_ppm,
            blocks,
            sound: false,
            sent: Box::new(|_| Sent::Arrives),
            sender_late: Box::new(|_| 0),
            capture_late: Box::new(|_| 0),
            privacy_at: None,
            restart_at: None,
            start_taken_late_us: 0,
            score: 0.9,
        }
    }

    fn run(mut self, state: &mut ReferenceAssembly) -> Run {
        let mut echo = EchoProcessor::new(false);
        let speaker = owner();
        let prime = |epoch, discarded, at| ClockPrimed {
            playback_epoch: epoch,
            privacy_generation: 2,
            accepted_zero_frames: PRIME_FRAMES,
            accepted_at_us: at,
            queue_observed_at_us: at + 1,
            queued_frames: PRIME_FRAMES,
            discarded,
        };
        let primed_at = START - self.start_taken_late_us;
        state
            .prime(prime(1, None, primed_at), primed_at + 2, 2, &mut echo)
            .unwrap();
        let started = state.started(1, primed_at + 10, primed_at + 20, START + 30);
        let period = 10_000.0 / (1.0 + self.skew_ppm * 1e-6);
        let mut run = Run {
            blocks: 0,
            fault: None,
            statuses: Vec::new(),
            bypassed: Vec::new(),
            reseeds: 0,
            trusted_blocks: 0,
            timing: ReferenceTiming::default(),
        };
        if let Err(error) = started {
            run.fault = Some(error.downcast_ref::<ReferenceFault>().unwrap().reason);
            run.timing = state.timing();
            return run;
        }
        // Packets of the current epoch: number, first cursor, when it began.
        let (mut epoch, mut packet, mut epoch_start, mut cursor) = (1, 0_u64, START, 480_u64);
        let mut sent_in_all = 0_u64;
        'session: for block in 0..self.blocks {
            let now = START + 5_000 + block * 10_000 + (self.capture_late)(block);
            let privacy = if self.privacy_at.is_some_and(|at| block >= at) {
                3
            } else {
                2
            };
            if self.restart_at == Some(block) {
                // The speaker dropped its queue and started its clock again.
                let old = QueuedRange {
                    epoch,
                    retired_through: cursor,
                    accepted_through: cursor,
                };
                epoch += 1;
                let at = now - 3_000;
                let result = state
                    .prime(prime(epoch, Some(old), at), at + 100, privacy, &mut echo)
                    .and_then(|_| state.started(epoch, at + 200, at + 300, at + 400));
                if let Err(error) = result {
                    run.fault = Some(error.downcast_ref::<ReferenceFault>().unwrap().reason);
                    break 'session;
                }
                (packet, epoch_start, cursor) = (0, at, 480);
            }
            loop {
                let turn = epoch_start + ((packet + 1) as f64 * period) as u64;
                let accepted_at = turn + (self.sender_late)(sent_in_all + 1);
                if accepted_at > now {
                    break;
                }
                packet += 1;
                sent_in_all += 1;
                let first = cursor;
                cursor += 240;
                let fate = (self.sent)(sent_in_all);
                for _ in 0..match fate {
                    Sent::Lost => 0,
                    Sent::Arrives => 1,
                    Sent::Twice => 2,
                } {
                    let payload = if self.sound {
                        RenderPayload::Speech {
                            owner: speaker,
                            samples: (0..240)
                                .map(|i| (((first + i) % 97) as i16 - 48) * 120)
                                .collect(),
                        }
                    } else {
                        RenderPayload::Silence { frames: 240 }
                    };
                    let reference = RenderReference {
                        playback_epoch: epoch,
                        privacy_generation: 2,
                        sequence: packet,
                        first_sample: first,
                        accepted_at_us: accepted_at,
                        queue_observed_at_us: accepted_at,
                        queued_frames: 456,
                        payload,
                    };
                    if let Err(error) = state.accept(reference, now, privacy, &mut echo) {
                        run.fault = Some(error.downcast_ref::<ReferenceFault>().unwrap().reason);
                        break 'session;
                    }
                }
            }
            // What the microphone has waiting grows with how late the loop is.
            let waiting = 16 + (self.capture_late)(block) as i64 * 16 / 1_000;
            match state.capture_step(waiting, now, now + 100, &mut echo) {
                Ok(Some(_)) => run.trusted_blocks += u64::from(state.evidence_trusted()),
                Ok(None) => run.bypassed.push(block),
                Err(error) => {
                    run.fault = Some(error.downcast_ref::<ReferenceFault>().unwrap().reason);
                    break 'session;
                }
            }
            state.note_score(self.score);
            run.blocks = block + 1;
            run.reseeds += u32::from(state.take_reseed());
            while let Some(status) = state.take_status() {
                run.statuses.push((block, status));
            }
        }
        while let Some(status) = state.take_status() {
            run.statuses.push((run.blocks, status));
        }
        run.timing = state.timing();
        run
    }
}

// ---------------------------------------------------------------------------
// The fixed margin and the run that ended at 378.6 s
// ---------------------------------------------------------------------------

/// Speaker against microphone on the lamp of that run, fitted to its own
/// once-a-second reports.
const RUN03_PPM: f64 = 157.3;

#[test]
fn the_fixed_margin_ends_where_the_native_run_ended() {
    // 37,855 capture blocks on the lamp. Here the packets keep perfect time
    // and the margin has six and a half blocks to cross: 41,322. On the lamp
    // they jitter by up to a block, which brought the bound half a block
    // nearer. The same cause, the same fault, within a tenth of the time.
    let mut state = ReferenceAssembly::default();
    let run = Session::new(RUN03_PPM, 60_000).run(&mut state);
    assert_eq!(run.fault, Some(Kind::ReferenceBacklog));
    assert_eq!(run.blocks, 41_322);
    assert!(run.blocks.abs_diff(37_855) < 37_855 / 10);
    assert!(run.statuses.is_empty() && run.timing.recovery.is_none());
}

/// The re-centred fixed margin over `blocks` of that lamp's drift.
fn recentred(blocks: u64) -> Run {
    let mut state = ReferenceAssembly::recentring(Policy::BOUNDED);
    let run = Session::new(RUN03_PPM, blocks).run(&mut state);
    assert_eq!(
        (run.blocks, run.fault),
        (blocks, None),
        "{:?}",
        run.causes()
    );
    // Every 445 s after the first, seven blocks of drift from nine ahead
    // back to two, and nothing else.
    assert!(
        run.causes()
            .iter()
            .all(|cause| *cause == (Trust::Warming, Some(Cause::MarginExhausted))),
        "{:?}",
        run.causes()
    );
    let between: Vec<u64> = run
        .statuses
        .windows(2)
        .map(|pair| pair[1].0 - pair[0].0)
        .collect();
    assert!(
        between
            .iter()
            .all(|blocks| (44_000..=45_000).contains(blocks)),
        "{between:?}"
    );
    // The canceller was never bypassed and no counter was rewound: every
    // capture block and every accepted frame is still counted.
    assert!(run.bypassed.is_empty());
    assert_eq!(run.timing.capture_blocks_processed, blocks);
    assert_eq!(run.timing.accepted_through % 240, 0);
    assert_eq!(run.timing.analysed_through, run.timing.accepted_through);
    let counts = run.timing.recovery.unwrap();
    assert_eq!(counts.reseeds, run.reseeds);
    assert_eq!(
        (counts.skipped_frames, counts.bypassed_capture_blocks),
        (0, 0)
    );
    // The clock kept itself with zeros throughout: nothing warmed the
    // canceller, so no evidence was given out after the first re-centre.
    assert_eq!(counts.trust, Trust::Warming);
    assert_eq!(run.trusted_blocks, run.statuses[0].0);
    run
}

#[test]
fn recentred_the_fixed_margin_goes_on_past_where_the_native_run_ended() {
    // Sixteen minutes: the run ended after six.
    assert_eq!(recentred(100_000).reseeds, 2);
}

/// An hour. Slow in a debug build: run with `--release -- --ignored`.
#[test]
#[ignore]
fn recentred_the_fixed_margin_carries_an_hour_of_that_lamps_drift() {
    assert_eq!(recentred(360_000).reseeds, 8);
}

#[test]
fn an_idle_room_is_recentred_before_the_bound_and_a_busy_one_at_it() {
    // Nobody speaking and the lamp silent: the re-centre comes two blocks
    // early, where a cold canceller costs nobody anything.
    let mut state = ReferenceAssembly::recentring(Policy::BOUNDED);
    let mut session = Session::new(RUN03_PPM, 50_000);
    session.score = 0.0;
    let idle = session.run(&mut state);
    assert_eq!((idle.blocks, idle.fault), (50_000, None));
    // The lamp speaking all the while: only the bound itself will do.
    let mut state = ReferenceAssembly::recentring(Policy::BOUNDED);
    let mut session = Session::new(RUN03_PPM, 50_000);
    (session.score, session.sound) = (0.0, true);
    let busy = session.run(&mut state);
    assert_eq!((busy.blocks, busy.fault), (50_000, None));
    let first = |run: &Run| run.statuses[0].0;
    // Three and a half blocks of drift against six and a half.
    assert_eq!((first(&idle), first(&busy)), (22_250, 41_322));
    assert!(idle.reseeds > busy.reseeds);
    // Someone speaking in a silent room is not idle either.
    let mut state = ReferenceAssembly::recentring(Policy::BOUNDED);
    let speaking = Session::new(RUN03_PPM, 45_000).run(&mut state);
    assert_eq!(first(&speaking), 41_322);
}

#[test]
fn a_recentred_canceller_warms_on_three_seconds_of_the_lamps_own_sound() {
    let mut state = ReferenceAssembly::recentring(Policy::BOUNDED);
    let mut session = Session::new(RUN03_PPM, 90_000);
    session.sound = true;
    let run = session.run(&mut state);
    assert_eq!((run.blocks, run.fault), (90_000, None));
    let causes = run.causes();
    assert_eq!(causes.len(), 4, "{causes:?}");
    for pair in run.statuses.chunks(2) {
        assert_eq!(pair[0].1.trust, Trust::Warming);
        assert_eq!(pair[1].1.trust, Trust::Trusted);
        // The block of the re-centre is the first of the three hundred.
        assert_eq!(pair[1].0 - pair[0].0, 299, "three seconds of sound");
    }
    assert_eq!(run.trusted_blocks, 90_000 - 2 * 299);
}

#[test]
fn a_speaker_that_runs_slow_is_recentred_the_other_way() {
    // Without recovery the two blocks of lead are gone in a quarter of the time.
    let mut state = ReferenceAssembly::default();
    let run = Session::new(-RUN03_PPM, 60_000).run(&mut state);
    assert_eq!(run.fault, Some(Kind::ReferenceLate));
    // A block and a half of drift from a lead of two.
    assert_eq!(run.blocks, 9_537);
    let mut state = ReferenceAssembly::recentring(Policy::BOUNDED);
    let run = Session::new(-RUN03_PPM, 60_000).run(&mut state);
    assert_eq!(
        (run.blocks, run.fault),
        (60_000, None),
        "{:?}",
        run.causes()
    );
    // The first after a block and a half of drift, the next after six.
    assert_eq!(run.reseeds, 2);
    assert!(run.bypassed.is_empty());
    assert_eq!(run.timing.capture_blocks_processed, 60_000);
}

#[test]
fn the_fixed_margin_recovers_drift_and_nothing_else() {
    // A capture loop held up for 60 ms: the packets it finds are older than
    // a reference may be. Still the fault it was.
    let mut state = ReferenceAssembly::recentring(Policy::BOUNDED);
    let mut session = Session::new(RUN03_PPM, 6_000);
    session.capture_late = stall(3_000, 60_000);
    let run = session.run(&mut state);
    assert_eq!((run.blocks, run.fault), (3_000, Some(Kind::QueueTiming)));
    // A lost packet: still a gap.
    let mut state = ReferenceAssembly::recentring(Policy::BOUNDED);
    let mut session = Session::new(RUN03_PPM, 6_000);
    session.sent = Box::new(|packet| {
        if packet == 3_000 {
            Sent::Lost
        } else {
            Sent::Arrives
        }
    });
    let run = session.run(&mut state);
    // Seen first as a queue observation older than the queue it observed.
    assert_eq!((run.blocks, run.fault), (3_000, Some(Kind::ReferenceLate)));
    assert_eq!(run.reseeds, 0);
    // A speaker that stops: still late, and no re-centre is tried.
    let mut state = ReferenceAssembly::recentring(Policy::BOUNDED);
    let mut session = Session::new(RUN03_PPM, 6_000);
    session.sender_late = stall(3_000, 80_000);
    let run = session.run(&mut state);
    assert_eq!(run.fault, Some(Kind::ReferenceLate));
    assert_eq!(run.reseeds, 0);
}

#[test]
fn a_difference_no_clock_has_spends_the_budget_and_says_so() {
    // 2 % is not a crystal: the margin goes every three seconds.
    let mut state = ReferenceAssembly::recentring(Policy::BOUNDED);
    let run = Session::new(20_000.0, 6_000).run(&mut state);
    assert_eq!(run.fault, Some(Kind::RecoveryExhausted));
    assert_eq!(run.reseeds, Policy::BOUNDED.reseeds as u32);
    assert!(run.blocks < 1_500, "{}", run.blocks);
}

// ---------------------------------------------------------------------------
// The capture-clocked reference
// ---------------------------------------------------------------------------

fn recovering() -> ReferenceAssembly {
    ReferenceAssembly::recovering(Policy::BOUNDED)
}

#[test]
fn the_capture_clocked_reference_needs_no_recentre_at_that_lamps_drift() {
    let mut state = recovering();
    let run = Session::new(RUN03_PPM, 60_000).run(&mut state);
    assert_eq!((run.blocks, run.fault), (60_000, None));
    assert!(run.statuses.is_empty() && run.reseeds == 0 && run.bypassed.is_empty());
    assert_eq!(run.trusted_blocks, 60_000);
    let drift = state.drift().unwrap();
    assert!((drift.steady_ppm - RUN03_PPM).abs() < 10.0, "{drift:?}");
}

#[test]
fn a_capture_loop_held_up_with_nothing_missing_loses_nothing() {
    // Up to what a lamp's microphone buffer holds, 40 ms. Every packet is
    // there and in order when the loop returns, and so is every microphone
    // block.
    for stall_us in [15_000, 27_000, 38_000] {
        let mut state = recovering();
        let mut session = Session::new(98.0, 9_000);
        session.sound = true;
        session.capture_late = stall(3_000, stall_us);
        let run = session.run(&mut state);
        assert_eq!((run.blocks, run.fault), (9_000, None), "{stall_us}");
        assert!(run.statuses.is_empty(), "{stall_us}: {:?}", run.causes());
        assert!(run.bypassed.is_empty() && run.reseeds == 0, "{stall_us}");
        assert_eq!(run.trusted_blocks, 9_000, "{stall_us}");
        let counts = run.timing.recovery.unwrap();
        // Late to arrive, and counted so: all but the newest three of them.
        assert_eq!(
            counts.late_packets > 0,
            stall_us >= 38_000,
            "{stall_us}: {counts:?}"
        );
        assert_eq!(counts.skipped_frames, 0);
        // What is accepted and not analysed is what waits, to the frame.
        assert_eq!(
            run.timing.accepted_through - run.timing.analysed_through,
            state.drift().unwrap().unread_frames as u64,
            "{stall_us}"
        );
    }
}

#[test]
fn a_capture_loop_held_up_for_longer_than_a_microphone_holds_reseeds() {
    // 70 ms. On a lamp the microphone's own buffer overruns at 40 ms, and
    // that is the capture device's discontinuity to report. This is the
    // reference's side of it: more waiting than may wait.
    let mut state = recovering();
    let mut session = Session::new(98.0, 9_000);
    session.capture_late = stall(3_000, 70_000);
    let run = session.run(&mut state);
    assert_eq!((run.blocks, run.fault), (9_000, None), "{:?}", run.causes());
    let causes = run.causes();
    assert_eq!(causes.len(), 2, "{causes:?}");
    assert_eq!(causes[0].0, Trust::Untrusted);
    assert!(matches!(
        causes[0].1,
        Some(Cause::CaptureBacklog | Cause::Overflow)
    ));
    assert_eq!(causes[1].0, Trust::Warming);
    assert_eq!(run.reseeds, 1);
    let counts = run.timing.recovery.unwrap();
    assert!(counts.skipped_frames > 0 && counts.bypassed_capture_blocks > 0);
    assert_eq!(counts.bypassed_capture_blocks as usize, run.bypassed.len());
    // Bypassed for less than a quarter of a second, and in one stretch.
    assert!(run.bypassed.len() < 25, "{:?}", run.bypassed);
    assert!(run.bypassed.windows(2).all(|pair| pair[1] == pair[0] + 1));
    // Every capture block is still counted and every accepted frame still
    // has its own cursor.
    assert_eq!(run.timing.capture_blocks_processed, 9_000);
    assert_eq!(run.timing.accepted_through % 240, 0);
}

#[test]
fn a_speaker_held_up_within_its_queue_changes_nothing() {
    for stall_us in [15_000, 30_000] {
        let mut state = recovering();
        let mut session = Session::new(98.0, 9_000);
        session.sender_late = stall(3_000, stall_us);
        let run = session.run(&mut state);
        assert_eq!((run.blocks, run.fault), (9_000, None), "{stall_us}");
        assert!(run.statuses.is_empty(), "{stall_us}: {:?}", run.causes());
        assert_eq!(run.trusted_blocks, 9_000);
    }
}

#[test]
fn a_speaker_held_up_for_longer_than_its_queue_is_untrusted_until_fresh_reference() {
    // Its own timestamps show nothing accepted for longer than its buffer
    // holds: what it played and what was counted have parted.
    for stall_us in [45_000, 70_000, 100_000] {
        let mut state = recovering();
        let mut session = Session::new(98.0, 9_000);
        session.sound = true;
        session.sender_late = stall(3_000, stall_us);
        let run = session.run(&mut state);
        assert_eq!((run.blocks, run.fault), (9_000, None), "{stall_us}");
        let causes = run.causes();
        assert_eq!(causes.len(), 3, "{stall_us}: {causes:?}");
        assert_eq!(causes[0].0, Trust::Untrusted);
        assert!(
            matches!(causes[0].1, Some(Cause::Absent | Cause::SenderPaused)),
            "{stall_us}: {causes:?}"
        );
        assert_eq!(causes[1].0, Trust::Warming);
        assert_eq!(causes[2], (Trust::Trusted, None));
        assert_eq!(run.reseeds, 1);
        // No capture block passed a canceller without its reference, and no
        // evidence was given out until three seconds of sound had warmed it.
        let (lost_at, back_at, warm_at) = (run.statuses[0].0, run.statuses[1].0, run.statuses[2].0);
        assert!(
            run.bypassed
                .iter()
                .all(|block| (lost_at..=back_at).contains(block))
        );
        // A pause the ring outlasts is found by the speaker's own
        // timestamps when its packets return, and they return together:
        // then the reseed is in the same tick and no block is bypassed.
        assert!(run.bypassed.len() < 30, "{stall_us}: {:?}", run.bypassed);
        assert!((299..=306).contains(&(warm_at - back_at)), "{stall_us}");
        assert_eq!(
            run.trusted_blocks,
            9_000 - (warm_at - lost_at),
            "{stall_us}"
        );
        let status = run.statuses[1].1;
        assert!(
            status.untrusted_us.unwrap() < 250_000,
            "{stall_us}: {status:?}"
        );
    }
}

#[test]
fn a_lost_packet_is_a_gap_that_is_reported_and_reseeded_past() {
    let mut state = recovering();
    let mut session = Session::new(98.0, 9_000);
    session.sent = Box::new(|packet| {
        if packet == 3_000 {
            Sent::Lost
        } else {
            Sent::Arrives
        }
    });
    let run = session.run(&mut state);
    assert_eq!((run.blocks, run.fault), (9_000, None), "{:?}", run.causes());
    assert_eq!(
        run.causes(),
        [
            (Trust::Untrusted, Some(Cause::SequenceGap)),
            (Trust::Warming, Some(Cause::SequenceGap))
        ]
    );
    let lost = run.statuses[0].1;
    assert_eq!(
        (lost.expected_sequence, lost.received_sequence),
        (3_000, Some(3_001))
    );
    // The frames of the lost packet, and those waiting when it was missed,
    // are counted as skipped under their own cursors.
    let counts = run.timing.recovery.unwrap();
    assert!(counts.skipped_frames >= 240, "{counts:?}");
    assert_eq!(
        run.timing.accepted_through,
        480 + 240 * (run.timing_packets())
    );
}

impl Run {
    /// Packets the speaker sent, by the cursor it reached.
    fn timing_packets(&self) -> u64 {
        (self.timing.accepted_through - 480) / 240
    }
}

#[test]
fn a_packet_seen_before_is_counted_and_never_used() {
    let mut state = recovering();
    let mut session = Session::new(98.0, 6_000);
    session.sent = Box::new(|packet| {
        if packet % 1_000 == 0 {
            Sent::Twice
        } else {
            Sent::Arrives
        }
    });
    let run = session.run(&mut state);
    assert_eq!((run.blocks, run.fault), (6_000, None));
    assert!(run.statuses.is_empty());
    assert_eq!(run.timing.recovery.unwrap().ignored_packets, 6);
    assert_eq!(run.trusted_blocks, 6_000);
}

#[test]
fn a_cursor_or_a_clock_that_runs_backwards_is_never_recovered() {
    let mut echo = EchoProcessor::new(false);
    for regress in [true, false] {
        let mut state = recovering();
        let run = Session::new(0.0, 100).run(&mut state);
        assert_eq!(run.fault, None);
        let now = START + 5_000 + 100 * 10_000;
        let timing = state.timing();
        let mut bad = RenderReference {
            playback_epoch: 1,
            privacy_generation: 2,
            sequence: 1_000,
            first_sample: timing.accepted_through,
            accepted_at_us: now - 1_000,
            queue_observed_at_us: now - 1_000,
            queued_frames: 456,
            payload: RenderPayload::Silence { frames: 240 },
        };
        let expected = if regress {
            bad.first_sample -= 240;
            Kind::CursorGap
        } else {
            bad.accepted_at_us = timing.accepted_at_us - 1;
            bad.queue_observed_at_us = bad.accepted_at_us;
            Kind::QueueTiming
        };
        let error = state.accept(bad, now, 2, &mut echo).unwrap_err();
        assert_eq!(
            error.downcast_ref::<ReferenceFault>().unwrap().reason,
            expected
        );
        assert!(state.take_status().is_none(), "a fault, not a status");
    }
}

#[test]
fn two_recoveries_are_two_reseeds_and_a_fourth_in_a_minute_is_the_end() {
    let lose = |every: u64| -> Box<dyn FnMut(u64) -> Sent> {
        Box::new(move |packet| {
            if packet % every == 0 {
                Sent::Lost
            } else {
                Sent::Arrives
            }
        })
    };
    let mut state = recovering();
    let mut session = Session::new(98.0, 5_000);
    session.sent = lose(2_000);
    let run = session.run(&mut state);
    assert_eq!((run.blocks, run.fault, run.reseeds), (5_000, None, 2));
    // A packet lost every ten seconds: three reseeds, then no more.
    let mut state = recovering();
    let mut session = Session::new(98.0, 6_000);
    session.sent = lose(1_000);
    let run = session.run(&mut state);
    assert_eq!(run.fault, Some(Kind::RecoveryExhausted));
    assert_eq!(run.reseeds, 3);
    assert!((4_000..4_100).contains(&run.blocks), "{}", run.blocks);
    // The same loss spread over more than a minute is within the budget.
    let mut state = recovering();
    let mut session = Session::new(98.0, 14_000);
    session.sent = lose(2_500);
    let run = session.run(&mut state);
    assert_eq!((run.blocks, run.fault, run.reseeds), (14_000, None, 5));
}

#[test]
fn a_reference_that_does_not_return_ends_the_worker_within_its_bound() {
    let mut state = recovering();
    let mut session = Session::new(98.0, 6_000);
    session.sent = Box::new(|packet| {
        if packet >= 3_000 {
            Sent::Lost
        } else {
            Sent::Arrives
        }
    });
    let run = session.run(&mut state);
    assert_eq!(run.fault, Some(Kind::RecoveryTimedOut));
    assert_eq!(run.causes(), [(Trust::Untrusted, Some(Cause::Absent))]);
    // Dry about 50 ms after the last packet, then a second and no longer.
    let lost_at = run.statuses[0].0;
    assert!((3_002..=3_008).contains(&lost_at), "{lost_at}");
    assert!(
        (99..=101).contains(&(run.blocks - lost_at)),
        "{}",
        run.blocks - lost_at
    );
    assert_eq!(run.bypassed.len() as u64, run.blocks - lost_at);
}

#[test]
fn privacy_is_never_recovered_from() {
    // The worker's privacy generation moves on while the reference is
    // untrusted: the next packet is a fault, not an anchor.
    let mut state = recovering();
    let mut session = Session::new(98.0, 6_000);
    session.sent = Box::new(|packet| {
        if packet == 3_000 {
            Sent::Lost
        } else {
            Sent::Arrives
        }
    });
    session.privacy_at = Some(3_002);
    let run = session.run(&mut state);
    assert_eq!(run.fault, Some(Kind::PrivacyMismatch));
    assert_eq!(run.reseeds, 0);
    assert_eq!(run.causes(), [(Trust::Untrusted, Some(Cause::SequenceGap))]);
}

#[test]
fn a_speaker_that_restarts_its_clock_is_a_cold_canceller_too() {
    let mut state = recovering();
    let mut session = Session::new(98.0, 6_000);
    session.sound = true;
    session.restart_at = Some(3_000);
    let run = session.run(&mut state);
    assert_eq!((run.blocks, run.fault), (6_000, None), "{:?}", run.causes());
    assert_eq!(
        run.causes(),
        [
            (Trust::Warming, Some(Cause::ClockRestart)),
            (Trust::Trusted, None)
        ]
    );
    // Three seconds of sound, after the silence the new stream leads with.
    assert!((299..=306).contains(&(run.statuses[1].0 - run.statuses[0].0)));
    // The speaker's restart is its own; it spends none of the reseed budget.
    assert_eq!(run.reseeds, 0);
    assert_eq!(run.timing.playback_epoch, 2);
}

/// A load stall of the issue's length between the prime and the start
/// notice being taken.
const LATE_START_US: u64 = 67_000;

#[test]
fn a_start_notice_taken_late_under_a_stall_is_taken_and_counted() {
    // Capture-clocked, the reference is paired by count through the ring
    // the prime filled; the start's stamps only feed the delay hint. Taken
    // late, it pairs exactly as an on-time one, and the stream runs.
    let mut state = recovering();
    let mut session = Session::new(98.0, 3_000);
    session.sound = true;
    session.start_taken_late_us = LATE_START_US;
    let run = session.run(&mut state);
    assert_eq!((run.blocks, run.fault), (3_000, None), "{:?}", run.causes());
    assert!(run.statuses.is_empty(), "{:?}", run.causes());
    assert!(run.bypassed.is_empty() && run.reseeds == 0);
    assert_eq!(run.trusted_blocks, 3_000);
    assert_eq!(run.timing.capture_blocks_processed, 3_000);
    let counts = run.timing.recovery.unwrap();
    assert_eq!(counts.late_starts, 1, "{counts:?}");
    assert_eq!((counts.degradations, counts.skipped_frames), (0, 0));
}

#[test]
fn a_late_start_notice_is_still_a_fault_where_its_stamps_align_the_stream() {
    // The fixed margin measures its origin from the start, so a late one is
    // what it always was even with recovery. Without recovery nothing is.
    for mut state in [
        ReferenceAssembly::recentring(Policy::BOUNDED),
        ReferenceAssembly::capture_clocked(),
    ] {
        let mut session = Session::new(98.0, 100);
        session.start_taken_late_us = LATE_START_US;
        let run = session.run(&mut state);
        assert_eq!((run.blocks, run.fault), (0, Some(Kind::InvalidStart)));
        assert!(run.timing.clock_started_at_us.is_none());
    }
}

#[test]
fn a_start_notice_older_than_recovery_allows_is_still_a_fault() {
    let mut state = recovering();
    let mut session = Session::new(98.0, 100);
    session.start_taken_late_us = Policy::BOUNDED.untrusted_deadline_us;
    let run = session.run(&mut state);
    assert_eq!((run.blocks, run.fault), (0, Some(Kind::InvalidStart)));
    assert_eq!(run.timing.recovery.unwrap().late_starts, 0);
}

#[test]
fn a_start_notice_completed_after_it_was_taken_is_a_fault() {
    let mut state = recovering();
    let mut echo = EchoProcessor::new(false);
    let prime = ClockPrimed {
        playback_epoch: 1,
        privacy_generation: 2,
        accepted_zero_frames: PRIME_FRAMES,
        accepted_at_us: START,
        queue_observed_at_us: START + 1,
        queued_frames: PRIME_FRAMES,
        discarded: None,
    };
    state.prime(prime, START + 2, 2, &mut echo).unwrap();
    let error = state
        .started(1, START + 10, START + 50, START + 30)
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<ReferenceFault>().unwrap().reason,
        Kind::InvalidStart
    );
    assert!(state.timing().clock_started_at_us.is_none());
}

#[test]
fn the_rate_learned_survives_a_reseed() {
    // Five minutes to learn the rate, a lost packet, and one second more.
    let learned = |lose: bool| {
        let mut state = recovering();
        let mut session = Session::new(RUN03_PPM, 30_100);
        session.sent = Box::new(move |packet| {
            if lose && packet == 30_000 {
                Sent::Lost
            } else {
                Sent::Arrives
            }
        });
        let run = session.run(&mut state);
        assert_eq!((run.blocks, run.fault), (30_100, None));
        (run.reseeds, state.drift().unwrap().steady_ppm)
    };
    let ((none, before), (one, after)) = (learned(false), learned(true));
    assert_eq!((none, one), (0, 1));
    // Kept, not learned again: the crystals are the ones they were.
    assert!(before > 120.0, "{before}");
    assert!((after - before).abs() < 5.0, "{before} then {after}");
}

#[test]
fn stalls_on_both_sides_at_once_are_one_recovery() {
    // The whole host stops for 70 ms: the speaker's queue ran out and the
    // capture loop found a backlog.
    let mut state = recovering();
    let mut session = Session::new(98.0, 9_000);
    session.sender_late = stall(3_000, 70_000);
    session.capture_late = both(stall(3_000, 70_000), stall(6_000, 20_000));
    let run = session.run(&mut state);
    assert_eq!(
        (run.blocks, run.fault, run.reseeds),
        (9_000, None, 1),
        "{:?} bypassed {:?}",
        run.statuses
            .iter()
            .map(|(block, status)| (
                *block,
                status.trust,
                status.cause,
                status.accepted_through,
                status.analysed_through
            ))
            .collect::<Vec<_>>(),
        run.bypassed
    );
    assert_eq!(run.timing.capture_blocks_processed, 9_000);
}

// Independent review regressions: production ReferenceAssembly calls only.
#[test]
fn audit_fixed_current_capture_stall_must_not_be_labelled_clock_drift() {
    let mut state = ReferenceAssembly::recentring(Policy::BOUNDED);
    let mut session = Session::new(RUN03_PPM, 30_010);
    // A 20ms hold-up at a pre-existing drift margin: the preceding capture
    // had no backlog, but the capture being serviced has 336 frames waiting.
    session.capture_late = stall(30_000, 20_000);
    let run = session.run(&mut state);
    eprintln!(
        "audit short stall blocks={} fault={:?} statuses={:?}",
        run.blocks, run.fault, run.statuses
    );
    assert!(run.fault.is_some(), "current backlog was relabelled drift");
    assert_eq!(run.reseeds, 0, "stall cannot certify drift-only recovery");
}

#[test]
fn audit_expired_untrusted_deadline_cannot_be_healed_by_the_last_anchor_block() {
    let mut state = recovering();
    let run = Session::new(0.0, 100).run(&mut state);
    assert_eq!(run.fault, None);
    let mut echo = EchoProcessor::new(false);
    let lost_at = START + 5_000 + 100 * 10_000;
    let mut sequence = state.sequence + 2;
    let mut cursor = state.timing().accepted_through + 240;
    let mut send = |state: &mut ReferenceAssembly, at: u64, echo: &mut EchoProcessor| {
        state
            .accept(
                RenderReference {
                    playback_epoch: 1,
                    privacy_generation: 2,
                    sequence,
                    first_sample: cursor,
                    accepted_at_us: at,
                    queue_observed_at_us: at,
                    queued_frames: 456,
                    payload: RenderPayload::Silence { frames: 240 },
                },
                at + 10,
                2,
                echo,
            )
            .unwrap();
        sequence += 1;
        cursor += 240;
    };
    send(&mut state, lost_at, &mut echo);
    assert_eq!(state.trust(), Trust::Untrusted);
    for block in 0..100 {
        let at = lost_at + block * 10_000;
        if block >= 96 {
            send(&mut state, at, &mut echo);
        }
        assert_eq!(
            state
                .capture_step(16, at + 100, at + 100, &mut echo)
                .unwrap(),
            None
        );
    }
    let deadline = lost_at + Policy::BOUNDED.untrusted_deadline_us;
    send(&mut state, deadline, &mut echo);
    let result = state.capture_step(16, deadline + 100, deadline + 100, &mut echo);
    eprintln!(
        "audit expired deadline result={:?} trust={:?} recovery={:?}",
        result,
        state.trust(),
        state.recovery_counts()
    );
    assert!(result.is_err(), "expired untrusted interval was reseeded");
    assert_eq!(
        result
            .unwrap_err()
            .downcast_ref::<ReferenceFault>()
            .unwrap()
            .reason,
        Kind::RecoveryTimedOut
    );
}

#[test]
fn audit_future_queue_observation_is_not_just_a_late_packet() {
    let mut state = recovering();
    let run = Session::new(0.0, 100).run(&mut state);
    assert_eq!(run.fault, None);
    let now = START + 5_000 + 100 * 10_000;
    let mut echo = EchoProcessor::new(false);
    let result = state.accept(
        RenderReference {
            playback_epoch: 1,
            privacy_generation: 2,
            sequence: state.sequence + 1,
            first_sample: state.timing().accepted_through,
            accepted_at_us: now - 100,
            queue_observed_at_us: now + 100_000,
            queued_frames: 456,
            payload: RenderPayload::Silence { frames: 240 },
        },
        now,
        2,
        &mut echo,
    );
    eprintln!(
        "audit future queue result={:?} counts={:?}",
        result,
        state.recovery_counts()
    );
    assert!(
        result.is_err(),
        "future queue time accepted and counted as merely late"
    );
}
