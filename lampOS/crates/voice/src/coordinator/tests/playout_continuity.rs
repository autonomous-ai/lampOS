//! Recorded supply through the production reply queue. No audio device.
//!
//! `append_provider_audio`, `may_start` and `next_speaker_chunk` are the
//! coordinator's own, driven at the recorded enqueue times with one block per
//! 10 ms slot. The fixture is the audio crate's: numbers only, frozen from
//! device traces. This is a replay of recorded supply, not a device result.
use super::*;
use crate::options::PlayoutStart;
use crate::provider_worker::{FailedStage, Fault};
use lamp_audio::playout::{Continuity, OpenedBy};

const REPLIES: &str = include_str!("../../../../audio/tests/it/playout_continuity/replies.tsv");
const SLOT_US: u64 = 10_000;

struct Recorded {
    complete_us: u64,
    after_us: u64,
    arrivals: Vec<(u64, usize)>,
}

fn recorded(name: &str) -> Recorded {
    let mut lines = REPLIES
        .lines()
        .skip_while(|line| !line.starts_with(&format!("reply {name} ")));
    let header = lines.next().unwrap();
    let field = |key: &str| {
        header
            .split(' ')
            .find_map(|field| field.strip_prefix(key))
            .map_or(0, |value| value.parse().unwrap())
    };
    Recorded {
        complete_us: field("complete_us="),
        after_us: field("after_us="),
        arrivals: lines
            .take_while(|line| !line.starts_with("reply "))
            .map(|line| line.split_once(' ').unwrap())
            .map(|(at, frames)| (at.parse().unwrap(), frames.parse().unwrap()))
            .collect(),
    }
}

fn reply_with(start: PlayoutStart) -> Reply {
    let (_, mut reply, _) = active_reply();
    reply.pcm.clear();
    reply.had_audio = false;
    reply.start = start.gate();
    reply
}

/// A reply that owns no playback yet, with its controller, for dispatch.
fn fresh(start: PlayoutStart) -> (Controller, Reply) {
    let (mut owner, mut reply, _) = active_reply();
    owner
        .playback_ended(now(), reply.playback.take().unwrap())
        .unwrap();
    reply.pcm.clear();
    reply.had_audio = false;
    reply.start = start.gate();
    (owner, reply)
}

/// The slot of each generation's first block, the runs of slots with nothing
/// to hand over, and the samples handed over.
struct Replayed {
    starts: Vec<u64>,
    starved: Vec<(usize, usize)>,
    samples: Vec<i16>,
}

/// Generations as (arrivals, completion), on one clock. The speaker's
/// retirement of a final block is taken as immediate: in the recorded turn
/// it came 2.2 s before the next generation's audio.
fn replay(start: PlayoutStart, generations: &[(Vec<(u64, usize)>, u64)]) -> Replayed {
    let mut reply = reply_with(start);
    let mut out = Replayed {
        starts: Vec::new(),
        starved: Vec::new(),
        samples: Vec::new(),
    };
    let (mut generation, mut next, mut appended, mut blocks) = (0, 0, 0_usize, 0);
    let (mut speaking, mut dry) = (false, false);
    for slot in 0..6_000 {
        let now = slot * SLOT_US;
        let (arrivals, complete_us) = &generations[generation];
        while let Some(&(at, frames)) = arrivals.get(next).filter(|(at, _)| *at <= now) {
            let samples = (appended..appended + frames)
                .map(|index| (index % 30_000) as i16 + 1)
                .collect();
            reply.append_provider_audio(samples, at);
            appended += frames;
            next += 1;
        }
        if now >= *complete_us && next == arrivals.len() {
            reply.generation_done = true;
        }
        if !reply.may_start(now) {
            if speaking {
                match out.starved.last_mut().filter(|_| dry) {
                    Some(run) => run.1 += 1,
                    None => out.starved.push((blocks, 1)),
                }
                dry = true;
            }
            continue;
        }
        if !speaking {
            out.starts.push(now);
        }
        let (samples, last) = reply.next_speaker_chunk().unwrap();
        out.samples.extend(samples);
        blocks += 1;
        (speaking, dry) = (!last, false);
        if last {
            generation += 1;
            next = 0;
            if generation == generations.len() {
                return out;
            }
        }
    }
    panic!("bounded replay did not finish");
}

fn one(name: &str) -> Vec<(Vec<(u64, usize)>, u64)> {
    let reply = recorded(name);
    vec![(reply.arrivals, reply.complete_us)]
}

fn horizon() -> PlayoutStart {
    PlayoutStart::new(200).unwrap()
}

#[test]
fn recorded_supply_runs_dry_behind_the_horizon_and_not_behind_continuity() {
    // Coffee and ocean of the six-turn run: blocks handed over before each
    // hole and its length in 10 ms slots, then the two first blocks. With
    // slots counted from the first enqueue the ocean reply starts 7 ms
    // before the lamp did, and loses one more slot at 4.8 s.
    for (name, holes, plain_start, continuity_start) in [
        ("six/4", vec![(672, 7)], 190_000, 290_000),
        ("six/5", vec![(479, 1), (859, 16)], 40_000, 240_000),
    ] {
        let plain = replay(horizon(), &one(name));
        assert_eq!(plain.starts, [plain_start], "{name}");
        assert_eq!(plain.starved, holes, "{name}");
        let held = replay(PlayoutStart::continuity(), &one(name));
        assert_eq!(held.starts, [continuity_start], "{name}");
        assert_eq!(held.starved, [], "{name}");
        // The same samples in the same order, and zeros only as final padding.
        assert_eq!(plain.samples, held.samples, "{name}");
        let frames: usize = recorded(name).arrivals.iter().map(|a| a.1).sum();
        assert!(
            held.samples[..frames]
                .iter()
                .enumerate()
                .all(|(index, &sample)| sample == (index % 30_000) as i16 + 1)
        );
        assert!(held.samples[frames..].iter().all(|&sample| sample == 0));
        assert!(held.samples.len() - frames < 240);
    }
}

#[test]
fn a_second_generation_is_held_again_through_the_production_queue() {
    let (introduction, story) = (recorded("six/6"), recorded("six/7"));
    let after = story.after_us;
    assert_eq!(after, 4_008_721);
    let turn = vec![
        (introduction.arrivals, introduction.complete_us),
        (
            story
                .arrivals
                .iter()
                .map(|&(at, frames)| (at + after, frames))
                .collect(),
            story.complete_us + after,
        ),
    ];
    // Today: the story finds the gate open and runs dry twice.
    let plain = replay(horizon(), &turn);
    assert_eq!(plain.starts, [120_000, 4_010_000]);
    assert_eq!(plain.starved, [(162 + 3, 14), (162 + 1_819, 8)]);
    // Continuity: each generation is held from its own first audio.
    let held = replay(PlayoutStart::continuity(), &turn);
    assert_eq!(held.starts, [290_000, 4_300_000]);
    assert_eq!(held.starved, []);
    assert_eq!(plain.samples, held.samples);
    // The introduction's 38,641 frames end 239 short of a block, and that
    // padding is the only zero before the story's own last block.
    let zeros: Vec<usize> = held
        .samples
        .iter()
        .enumerate()
        .filter_map(|(index, &sample)| (sample == 0).then_some(index))
        .collect();
    assert_eq!(zeros.len(), 239);
    assert_eq!((zeros[0], zeros[238]), (38_641, 38_879));
}

#[test]
fn the_receipt_names_the_policy_the_lead_and_the_reason() {
    let mut reply = reply_with(PlayoutStart::continuity());
    // The ocean reply's opening: 160 ms at once, more at 168 ms and 213 ms.
    for at in [0, 168_000, 213_000] {
        reply.append_provider_audio(vec![7; 3_840], 1_000_000 + at);
    }
    assert!(!reply.may_start(1_232_999));
    let held = reply.start_trace();
    assert_eq!(held["opened_by"], Value::Null);
    assert_eq!(held["measured_lead_us"], 107_000);
    // 233 ms held and 107 ms of lead are the 340 ms margin.
    assert!(reply.may_start(1_233_000));
    let trace = reply.start_trace();
    assert_eq!(trace["opened_by"], "margin");
    assert_eq!(trace["age_since_first_audio_us"], 233_000);
    assert_eq!(trace["horizon_us"], Continuity::EVALUATED.cover_us);
    assert_eq!(trace["continuity"]["margin_us"], 340_000);
    assert_eq!(trace["continuity"]["hold_us"], 290_000);
    assert_eq!(trace["continuity"]["cover_us"], 380_000);
    assert_eq!(reply.start.opened().unwrap().by, OpenedBy::Margin);
    // A plain gate reports the lead it measured and no policy.
    let mut reply = reply_with(horizon());
    for at in [0, 168_000, 213_000] {
        reply.append_provider_audio(vec![7; 3_840], 1_000_000 + at);
    }
    assert!(reply.may_start(1_213_000));
    let trace = reply.start_trace();
    assert_eq!(trace["continuity"], Value::Null);
    assert_eq!(trace["opened_by"], "covered");
    assert_eq!(trace["measured_lead_us"], 107_000);
}

#[test]
fn a_continuity_hold_mints_no_permit_and_leaves_the_pcm_alone() {
    let dir = SessionDirectory::create().unwrap();
    let (mut parent, mut speaker) = pair(&dir, "speaker");
    let (mut owner, mut current) = fresh(PlayoutStart::continuity());
    let original: Vec<i16> = (0..4_800).map(|i| (i % 100) as i16).collect();
    // 200 ms in hand opens the 200 ms horizon at once. Continuity holds it.
    current.append_provider_audio(original.clone(), monotonic_us());
    let mut reply = Some(current);
    let mut pending = None;
    let mut sequence = 0;
    let mut snapshot = owner.snapshot(now()).unwrap();
    let revision = snapshot.revision();
    dispatch_speaker_chunk(
        &mut owner,
        &mut reply,
        &mut parent.data,
        &mut pending,
        &mut sequence,
        &mut snapshot,
        &mut PlaybackMask::new(TurnTaking::V1Baseline),
    )
    .unwrap();
    assert!(speaker.data.receive::<SpeakerChunk>().unwrap().is_none());
    assert!(pending.is_none());
    assert_eq!((sequence, snapshot.revision()), (0, revision));
    let current = reply.as_mut().unwrap();
    assert_eq!(current.pcm, original);
    assert!(current.start.opened().is_none());
    // Declared complete, the same reply leaves at once.
    current.generation_done = true;
    dispatch_speaker_chunk(
        &mut owner,
        &mut reply,
        &mut parent.data,
        &mut pending,
        &mut sequence,
        &mut snapshot,
        &mut PlaybackMask::new(TurnTaking::V1Baseline),
    )
    .unwrap();
    let chunk = speaker.data.receive::<SpeakerChunk>().unwrap().unwrap();
    assert_eq!(chunk.samples, original[..240]);
    assert_eq!(
        reply.unwrap().start.opened().unwrap().by,
        OpenedBy::Complete
    );
}

#[test]
fn audio_before_the_final_block_leaves_joins_the_generation_being_played() {
    let mut reply = reply_with(PlayoutStart::continuity());
    reply.append_provider_audio(vec![7; 480], 1_000_000);
    reply.generation_done = true;
    assert!(reply.may_start(1_000_000));
    let opened = reply.start.opened();
    assert_eq!(reply.next_speaker_chunk(), Some((vec![7; 240], false)));
    // The last block of the first generation is still in the queue when the
    // next generation's audio arrives: no final block, no hold, no break.
    reply.append_provider_audio(vec![9; 480], 1_015_000);
    assert!(!reply.generation_done);
    assert!(reply.may_start(1_015_000));
    assert_eq!(reply.next_speaker_chunk(), Some((vec![7; 240], false)));
    assert!(reply.may_start(1_025_000));
    assert_eq!(reply.next_speaker_chunk(), Some((vec![9; 240], false)));
    assert_eq!(reply.start.opened(), opened, "one start for both");
    // The last block of the second waits for its own completion.
    assert!(!reply.may_start(1_035_000));
    reply.generation_done = true;
    assert_eq!(reply.next_speaker_chunk(), Some((vec![9; 240], true)));
}

#[test]
fn audio_after_the_final_block_left_is_held_from_its_own_arrival() {
    for retired_after_us in [50_000_u64, 400_000] {
        let mut reply = reply_with(PlayoutStart::continuity());
        reply.append_provider_audio(vec![7; 480], 1_000_000);
        reply.generation_done = true;
        assert!(reply.may_start(1_000_000));
        assert_eq!(reply.next_speaker_chunk(), Some((vec![7; 240], false)));
        assert_eq!(reply.next_speaker_chunk(), Some((vec![7; 240], true)));
        // As the coordinator does when the speaker accepts a final chunk.
        reply.final_chunk = Some(44);
        // The next generation arrives while that block is still playing.
        let arrived = 1_100_000;
        reply.append_provider_audio(vec![9; 4_800], arrived);
        assert_eq!(reply.start.opened(), None, "a new start");
        // Nothing leaves before the retirement receipt, however long ago
        // the audio arrived.
        assert!(!reply.may_start(arrived + 600_000));
        assert!(reply.next_speaker_chunk().is_none());
        assert_eq!(reply.start.opened(), None);
        reply.final_chunk = None;
        let retired = arrived + retired_after_us;
        if retired_after_us < 290_000 {
            // Retired early: the hold still runs from the arrival.
            assert!(!reply.may_start(retired));
            assert!(!reply.may_start(arrived + 289_999));
            assert!(reply.may_start(arrived + 290_000));
            let opened = reply.start.opened().unwrap();
            assert_eq!((opened.held_us, opened.by), (290_000, OpenedBy::HoldBound));
        } else {
            // Retired late: the wait already was the margin.
            assert!(reply.may_start(retired));
            let opened = reply.start.opened().unwrap();
            assert_eq!((opened.held_us, opened.by), (400_000, OpenedBy::Margin));
        }
        assert_eq!(reply.next_speaker_chunk(), Some((vec![9; 240], false)));
        assert_eq!(reply.pcm.len(), 4_560);
    }
}

#[test]
fn cancellation_during_a_continuity_hold_sends_nothing_and_frees_the_next_reply() {
    let dir = SessionDirectory::create().unwrap();
    let (mut parent, mut speaker) = pair(&dir, "speaker");
    let (mut owner, mut current) = fresh(PlayoutStart::continuity());
    current.append_provider_audio(vec![7; 4_800], monotonic_us());
    let old_owner = current.owner;
    let mut reply = Some(current);
    let mut pending = None;
    let mut sequence = 0;
    let mut snapshot = owner.snapshot(now()).unwrap();
    for cancel in [false, true] {
        if cancel {
            cancel_reply(
                &mut owner,
                &mut reply,
                &mut SessionState::default(),
                "user_interrupted",
            )
            .unwrap();
        }
        dispatch_speaker_chunk(
            &mut owner,
            &mut reply,
            &mut parent.data,
            &mut pending,
            &mut sequence,
            &mut snapshot,
            &mut PlaybackMask::new(TurnTaking::V1Baseline),
        )
        .unwrap();
        assert!(speaker.data.receive::<SpeakerChunk>().unwrap().is_none());
        assert!(pending.is_none());
        assert_eq!(sequence, 0);
    }
    assert!(reply.is_none());
    assert_ne!(owner.snapshot(now()).unwrap().owner(), Some(old_owner));
    // The next reply is held from its own first audio and no earlier.
    let mut next = reply_with(PlayoutStart::continuity());
    next.append_provider_audio(vec![9; 4_800], 5_000_000);
    assert!(!next.may_start(5_289_999));
    assert!(next.may_start(5_290_000));
}

#[test]
fn a_complete_or_failed_short_answer_leaves_at_once_under_continuity() {
    for failed in [false, true] {
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut speaker) = pair(&dir, "speaker");
        let (mut owner, mut current) = fresh(PlayoutStart::continuity());
        let request = current.owner.turn();
        current.append_provider_audio(vec![7; 73], monotonic_us());
        current.had_audio = true;
        let mut reply = Some(current);
        let mut pending = None;
        let mut sequence = 0;
        let mut snapshot = owner.snapshot(now()).unwrap();
        // Three milliseconds of audio and no completion: held.
        dispatch_speaker_chunk(
            &mut owner,
            &mut reply,
            &mut parent.data,
            &mut pending,
            &mut sequence,
            &mut snapshot,
            &mut PlaybackMask::new(TurnTaking::V1Baseline),
        )
        .unwrap();
        assert!(speaker.data.receive::<SpeakerChunk>().unwrap().is_none());
        let terminal = if failed {
            ProviderOutput::TurnFailed {
                request,
                stage: FailedStage::Responding,
                reason: Fault::Transport,
            }
        } else {
            ProviderOutput::TurnComplete {
                request,
                idle: true,
            }
        };
        observe_provider_output(&mut CloudAvailability::default(), &mut reply, &terminal).unwrap();
        dispatch_speaker_chunk(
            &mut owner,
            &mut reply,
            &mut parent.data,
            &mut pending,
            &mut sequence,
            &mut snapshot,
            &mut PlaybackMask::new(TurnTaking::V1Baseline),
        )
        .unwrap();
        let chunk = speaker.data.receive::<SpeakerChunk>().unwrap().unwrap();
        assert!(chunk.end_of_speech);
        assert_eq!(&chunk.samples[..73], &[7; 73]);
        assert_eq!(&chunk.samples[73..], &[0; 167]);
        let current = reply.as_ref().unwrap();
        assert_eq!(current.start.opened().unwrap().by, OpenedBy::Complete);
        assert!(current.pcm.is_empty());
    }
}
