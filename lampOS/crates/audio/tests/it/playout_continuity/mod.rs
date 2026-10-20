//! Recorded reply supply replayed through the start gate.
//!
//! `replies.tsv` holds numbers only: when each piece of 23 replies was
//! enqueued on a lamp and how many frames it carried, when the reply was
//! complete, where the speaker started, and the runs of zeros it accepted.
//! No audio and no text.
//!
//! What runs here is the real `StartGate` and a queue that follows the
//! coordinator's rules: one 240-frame block per 10 ms slot, the last block
//! withheld until the generation is complete, the final block padded with
//! zeros. What does not run is ALSA, the speaker worker, IPC, or anything
//! acoustic. Every figure for the continuity start is a replay of recorded
//! supply. None is a measurement of a device. See
//! `docs/playout-continuity.md`.
use lamp_audio::playout::{Continuity, Opened, OpenedBy, START_HORIZON_US, StartGate};
use std::collections::VecDeque;

const BLOCK: usize = 240;
const SLOT_US: u64 = 10_000;
/// Where the 10 ms slots fall relative to the first enqueue is not known in
/// advance, so every figure is taken over twenty phases.
const PHASES: std::ops::Range<u64> = 0..20;
const PHASE_US: u64 = 500;

struct Recorded {
    name: &'static str,
    complete_us: u64,
    first_us: u64,
    /// A later generation of the same turn: its first enqueue, counted from
    /// the first enqueue of the generation before it.
    after_us: Option<u64>,
    /// Blocks accepted before each recorded run of zeros, and its length in blocks.
    gaps: Vec<(usize, usize)>,
    arrivals: Vec<(u64, usize)>,
}

impl Recorded {
    fn frames(&self) -> usize {
        self.arrivals.iter().map(|arrival| arrival.1).sum()
    }
}

fn recorded() -> Vec<Recorded> {
    let mut replies: Vec<Recorded> = Vec::new();
    for line in include_str!("replies.tsv").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut fields = line.split(' ');
        let first = fields.next().unwrap();
        if first != "reply" {
            let frames = fields.next().unwrap().parse().unwrap();
            let reply = replies.last_mut().unwrap();
            reply.arrivals.push((first.parse().unwrap(), frames));
            continue;
        }
        let mut reply = Recorded {
            name: fields.next().unwrap(),
            complete_us: 0,
            first_us: 0,
            after_us: None,
            gaps: Vec::new(),
            arrivals: Vec::new(),
        };
        for field in fields {
            let (key, value) = field.split_once('=').unwrap();
            match key {
                "turn" => {}
                "complete_us" => reply.complete_us = value.parse().unwrap(),
                "first_us" => reply.first_us = value.parse().unwrap(),
                "after_us" => reply.after_us = Some(value.parse().unwrap()),
                "gaps" if value == "-" => {}
                "gaps" => {
                    reply.gaps = value
                        .split(',')
                        .map(|run| run.split_once(':').unwrap())
                        .map(|(block, count)| (block.parse().unwrap(), count.parse().unwrap()))
                        .collect();
                }
                other => panic!("unknown field {other}"),
            }
        }
        replies.push(reply);
    }
    replies
}

fn reply(name: &str) -> Recorded {
    recorded()
        .into_iter()
        .find(|reply| reply.name == name)
        .unwrap()
}

/// The provider's samples are their own index, so a block handed over out of
/// order, twice or altered cannot pass for the right one. None of them is zero.
fn sample(index: usize) -> i16 {
    (index % 30_000) as i16 + 1
}

/// The coordinator's reply queue, as far as playout is concerned.
struct Queue {
    pcm: VecDeque<i16>,
    generation_done: bool,
    start: StartGate,
    appended: usize,
}

impl Queue {
    fn new(start: StartGate) -> Self {
        Self {
            pcm: VecDeque::new(),
            generation_done: false,
            start,
            appended: 0,
        }
    }

    fn append(&mut self, frames: usize, at_us: u64) {
        self.start.arrival(at_us, frames);
        self.generation_done = false;
        self.pcm
            .extend((self.appended..self.appended + frames).map(sample));
        self.appended += frames;
    }

    fn has_block(&self) -> bool {
        !self.pcm.is_empty() && (self.generation_done || self.pcm.len() > BLOCK)
    }

    fn may_start(&mut self, at_us: u64) -> bool {
        self.has_block() && self.start.open(at_us, self.pcm.len(), self.generation_done)
    }

    fn next_block(&mut self) -> (Vec<i16>, bool) {
        let last = self.generation_done && self.pcm.len() <= BLOCK;
        let take = self.pcm.len().min(BLOCK);
        let mut block: Vec<i16> = self.pcm.drain(..take).collect();
        block.resize(BLOCK, 0);
        if last {
            self.start.generation_ended();
        }
        (block, last)
    }
}

#[derive(Debug)]
struct Played {
    /// The slot of the first block, from the first enqueue.
    first_us: u64,
    opened: Opened,
    /// Blocks handed over before each run of empty slots, and its length in slots.
    starved: Vec<(usize, usize)>,
    lead_us: u64,
    /// The largest number of frames the queue held.
    most_queued: usize,
    /// The fewest frames that could still be handed over, seen at any slot
    /// between the start and the completion: what was to spare.
    least_in_hand: usize,
}

impl Played {
    fn starved_us(&self) -> u64 {
        self.starved.iter().map(|run| run.1 as u64).sum::<u64>() * SLOT_US
    }
}

/// One generation through one gate, the first slot at `origin_us`.
fn play(reply: &Recorded, gate: StartGate, origin_us: u64) -> Played {
    let mut queue = Queue::new(gate);
    let (mut next, mut blocks, mut played) = (0, 0, 0);
    let mut first = None;
    let mut starved: Vec<(usize, usize)> = Vec::new();
    let mut dry = false;
    let (mut most_queued, mut least_in_hand) = (0, usize::MAX);
    // Sixty seconds bounds the replay. It is not a runtime deadline.
    for slot in 0..6_000 {
        let now = origin_us + slot * SLOT_US;
        while let Some(&(at, frames)) = reply.arrivals.get(next).filter(|(at, _)| *at <= now) {
            queue.append(frames, at);
            most_queued = most_queued.max(queue.pcm.len());
            next += 1;
        }
        if now >= reply.complete_us && next == reply.arrivals.len() {
            queue.generation_done = true;
        }
        let in_hand = queue.pcm.len().saturating_sub(BLOCK);
        let open = queue.may_start(now);
        if (open || first.is_some()) && !queue.generation_done {
            least_in_hand = least_in_hand.min(in_hand);
        }
        if !open {
            if first.is_some() {
                match starved.last_mut().filter(|_| dry) {
                    Some(run) => run.1 += 1,
                    None => starved.push((blocks, 1)),
                }
                dry = true;
            }
            continue;
        }
        let (block, last) = queue.next_block();
        let real = BLOCK.min(reply.frames() - played);
        assert!((0..real).all(|i| block[i] == sample(played + i)));
        assert!(
            block[real..].iter().all(|&s| s == 0),
            "only padding is zero"
        );
        assert_eq!(real < BLOCK, last && real < BLOCK, "padding is final");
        first.get_or_insert(now);
        dry = false;
        blocks += 1;
        played += real;
        if last {
            assert_eq!(played, reply.frames(), "{}", reply.name);
            return Played {
                first_us: first.unwrap(),
                opened: queue.start.opened().unwrap(),
                starved,
                lead_us: queue.start.lead_us(),
                most_queued,
                least_in_hand,
            };
        }
    }
    panic!("{}: bounded replay did not finish", reply.name);
}

/// The gate the integrated runtime uses today: 200 ms of cover for a reply's
/// first generation, and nothing for a later one, which finds it open.
fn current(reply: &Recorded) -> StartGate {
    StartGate::new(if reply.after_us.is_some() {
        0
    } else {
        START_HORIZON_US
    })
}

fn phases(reply: &Recorded, gate: impl Fn(&Recorded) -> StartGate) -> Vec<Played> {
    PHASES
        .map(|phase| play(reply, gate(reply), phase * PHASE_US))
        .collect()
}

fn continuity(_: &Recorded) -> StartGate {
    StartGate::continuous(Continuity::EVALUATED)
}

#[test]
fn the_fixture_is_the_23_recorded_replies() {
    let replies = recorded();
    assert_eq!(replies.len(), 23);
    assert_eq!(
        replies.iter().map(|r| r.arrivals.len()).sum::<usize>(),
        2_888
    );
    // 115 seconds of speech.
    assert_eq!(
        replies.iter().map(Recorded::frames).sum::<usize>(),
        2_755_209
    );
    // 23 recorded runs of zeros in 15 of the replies: 2,340 ms.
    let runs: Vec<usize> = replies
        .iter()
        .flat_map(|reply| reply.gaps.iter().map(|run| run.1))
        .collect();
    assert_eq!((runs.len(), runs.iter().sum::<usize>()), (23, 234));
    assert_eq!(replies.iter().filter(|r| !r.gaps.is_empty()).count(), 15);
    for reply in &replies {
        assert_eq!(reply.arrivals[0].0, 0, "{}", reply.name);
        assert!(reply.arrivals.windows(2).all(|pair| pair[0].0 <= pair[1].0));
    }
}

#[test]
fn the_four_recorded_gaps_come_back_where_the_lamp_recorded_them() {
    // The coffee, ocean and story replies of the six-turn run. Each replay
    // starts where the lamp's speaker started, which the gate had already
    // allowed. Blocks before the gap, then its length in 10 ms blocks.
    for (name, gaps) in [
        ("six/4", vec![(672, 7)]),
        ("six/5", vec![(859, 16)]),
        ("six/7", vec![(3, 14), (1_819, 7)]),
    ] {
        let reply = reply(name);
        assert_eq!(reply.gaps, gaps, "{name}: the fixture");
        let replayed = play(&reply, StartGate::new(0), reply.first_us);
        assert_eq!(replayed.starved, gaps, "{name}: the replay");
    }
}

#[test]
fn every_recorded_gap_is_reproduced_from_the_recorded_start() {
    let mut exact = 0;
    for reply in recorded() {
        let replayed = play(&reply, StartGate::new(0), reply.first_us);
        if reply.name == "coord/1" {
            // One block moves between this reply's two gaps. The total holds.
            assert_eq!(reply.gaps, [(9, 8), (269, 14)]);
            assert_eq!(replayed.starved, [(9, 9), (269, 13)]);
            continue;
        }
        assert_eq!(replayed.starved, reply.gaps, "{}", reply.name);
        exact += 1;
    }
    assert_eq!(exact, 22);
}

/// Per reply: the most the current gate starves it by over the phases, and
/// the least and most the continuity start adds to its first block. All in
/// milliseconds.
const BEFORE_AND_AFTER: &[(&str, u64, u64, u64)] = &[
    ("six/1", 0, 0, 0),
    ("six/2", 0, 160, 160),
    ("six/3", 0, 0, 0),
    ("six/4", 80, 100, 110),
    ("six/5", 170, 190, 200),
    ("six/6", 0, 170, 180),
    ("six/7", 220, 290, 290),
    ("ten/1", 0, 200, 200),
    ("ten/2", 120, 190, 190),
    ("ten/3", 160, 180, 180),
    ("ten/4", 70, 140, 140),
    ("ten/5", 0, 180, 190),
    ("ten/6", 120, 200, 200),
    ("ten/7", 0, 200, 200),
    ("ten/8", 0, 160, 160),
    ("ten/9", 0, 0, 0),
    ("ten/10", 0, 140, 140),
    ("pilot/1", 0, 200, 210),
    ("ns1/1", 0, 0, 0),
    ("ns2/1", 120, 150, 160),
    ("coord/1", 130, 190, 190),
    ("play1/2", 0, 0, 10),
    ("play2/4", 0, 180, 190),
];

#[test]
fn the_current_gate_starves_nine_of_the_replies_and_continuity_none() {
    let replies = recorded();
    assert_eq!(replies.len(), BEFORE_AND_AFTER.len());
    let mut added_most = Vec::new();
    for (reply, &(name, starved_ms, least_ms, most_ms)) in replies.iter().zip(BEFORE_AND_AFTER) {
        assert_eq!(reply.name, name);
        let before = phases(reply, current);
        let after = phases(reply, continuity);
        assert_eq!(
            before.iter().map(Played::starved_us).max().unwrap(),
            starved_ms * 1_000,
            "{name}: starved today"
        );
        for played in &after {
            assert_eq!(played.starved, [], "{name}: starved under continuity");
        }
        let added: Vec<u64> = before
            .iter()
            .zip(&after)
            .map(|(before, after)| after.first_us - before.first_us)
            .collect();
        assert_eq!(
            (*added.iter().min().unwrap(), *added.iter().max().unwrap()),
            (least_ms * 1_000, most_ms * 1_000),
            "{name}: added to the first block"
        );
        if reply.after_us.is_none() {
            added_most.push(most_ms);
        }
    }
    let starved: Vec<u64> = BEFORE_AND_AFTER.iter().map(|row| row.1).collect();
    assert_eq!(starved.iter().filter(|&&ms| ms > 0).count(), 9);
    assert_eq!(starved.iter().sum::<u64>(), 1_190);
    // What it costs a reply's first generation.
    added_most.sort_unstable();
    assert_eq!(added_most.len(), 22);
    assert_eq!(
        (added_most[10], added_most[11]),
        (160, 180),
        "the middle two"
    );
    assert_eq!(added_most.iter().sum::<u64>(), 3_010, "137 ms on average");
    assert_eq!(*added_most.last().unwrap(), 210);
    assert_eq!(added_most.iter().filter(|&&ms| ms > 200).count(), 1);
    assert_eq!(added_most.iter().filter(|&&ms| ms <= 10).count(), 5);
}

#[test]
fn no_recorded_start_is_held_past_the_bound() {
    let bound = Continuity::EVALUATED.longest_hold_us();
    assert_eq!(bound, 380_000);
    let (mut by, mut least) = ([0; 4], usize::MAX);
    for reply in recorded() {
        // The first slot at which a block could be handed over at all.
        let mut frames = 0;
        let could = reply
            .arrivals
            .iter()
            .find(|arrival| {
                frames += arrival.1;
                frames > BLOCK
            })
            .unwrap()
            .0;
        for played in phases(&reply, continuity) {
            assert!(
                played.first_us < could.max(bound) + SLOT_US,
                "{}",
                reply.name
            );
            assert!(played.most_queued <= 24_000, "{}", reply.name);
            least = least.min(played.least_in_hand);
            by[match played.opened.by {
                OpenedBy::Margin => 0,
                OpenedBy::HoldBound => 1,
                OpenedBy::Complete => 2,
                OpenedBy::Covered => 3,
            }] += 1;
            assert!(played.lead_us <= 120_000, "{}", reply.name);
        }
    }
    // 23 replies at 20 phases: 160 starts had the margin in time or lead by
    // the time a block could leave, and 300 were released by the hold bound
    // before their lead had been measured.
    assert_eq!(by, [160, 300, 0, 0]);
    // At the worst slot of the worst reply, 30 ms could still be handed over.
    assert_eq!(least, 720);
}

/// A plain horizon is the same gate with no margin to wait for. Held again
/// for each generation, 600 ms also starves none of the recorded replies, at
/// the same average cost. It holds some starts longer and leaves less to
/// spare, because audio in hand is not how far behind playout runs.
#[test]
fn a_600_ms_horizon_for_each_generation_also_starves_none() {
    let horizon = |_: &Recorded| {
        StartGate::continuous(Continuity {
            margin_us: 0,
            hold_us: 0,
            cover_us: 600_000,
        })
    };
    let (mut added, mut least) = (Vec::new(), usize::MAX);
    for reply in recorded() {
        let before = phases(&reply, current);
        let after = phases(&reply, horizon);
        for played in &after {
            assert_eq!(played.starved, [], "{}", reply.name);
            least = least.min(played.least_in_hand);
        }
        if reply.after_us.is_none() {
            let most = before.iter().zip(&after);
            added.push(most.map(|(b, a)| a.first_us - b.first_us).max().unwrap() / 1_000);
        }
    }
    added.sort_unstable();
    assert_eq!(added.iter().sum::<u64>(), 3_040, "138 ms on average");
    assert_eq!(added[19..], [210, 240, 240]);
    assert_eq!(least, 240, "10 ms to spare at the worst slot");
}

/// The sixth turn: an introduction, a pause of more than two seconds and a
/// story, as two generations through one gate.
fn sixth_turn(gate: StartGate, origin_us: u64) -> (u64, u64, Vec<(usize, usize)>, usize) {
    let (introduction, story) = (reply("six/6"), reply("six/7"));
    let after = story.after_us.unwrap();
    let mut arrivals = introduction.arrivals.clone();
    arrivals.extend(
        story
            .arrivals
            .iter()
            .map(|&(at, frames)| (at + after, frames)),
    );
    let completions = [
        (introduction.complete_us, introduction.arrivals.len()),
        (story.complete_us + after, arrivals.len()),
    ];
    let total = introduction.frames() + story.frames();
    let mut queue = Queue::new(gate);
    let (mut next, mut played, mut blocks, mut generation) = (0, 0, 0, 0);
    let mut starts = Vec::new();
    let mut starved: Vec<(usize, usize)> = Vec::new();
    let (mut speaking, mut dry, mut padding) = (false, false, 0);
    for slot in 0..6_000 {
        let now = origin_us + slot * SLOT_US;
        while let Some(&(at, frames)) = arrivals.get(next).filter(|(at, _)| *at <= now) {
            queue.append(frames, at);
            next += 1;
        }
        if generation < 2 && now >= completions[generation].0 && next == completions[generation].1 {
            queue.generation_done = true;
        }
        if !queue.may_start(now) {
            if speaking {
                match starved.last_mut().filter(|_| dry) {
                    Some(run) => run.1 += 1,
                    None => starved.push((blocks, 1)),
                }
                dry = true;
            }
            continue;
        }
        if !speaking {
            starts.push(now);
        }
        let (block, last) = queue.next_block();
        let real = block.iter().take_while(|&&s| s != 0).count();
        assert!((0..real).all(|i| block[i] == sample(played + i)));
        assert!(block[real..].iter().all(|&s| s == 0));
        assert!(real == BLOCK || last, "only a final block is padded");
        padding += BLOCK - real;
        played += real;
        blocks += 1;
        (speaking, dry) = (!last, false);
        if last {
            generation += 1;
            let expected = if generation == 1 {
                introduction.frames()
            } else {
                total
            };
            assert_eq!(played, expected, "a generation ends on its own last sample");
        }
        if generation == 2 {
            return (starts[0], starts[1] - after, starved, padding);
        }
    }
    panic!("bounded replay did not finish");
}

#[test]
fn a_second_generation_is_held_again_and_every_sample_is_played_once() {
    for phase in PHASES {
        let origin = phase * PHASE_US;
        // Today the story finds the gate open, starts with its first 40 ms
        // and runs dry 30 ms later, and again eighteen seconds in.
        let (introduction, story, starved, padding) =
            sixth_turn(StartGate::new(START_HORIZON_US), origin);
        assert!((113_000..=123_000).contains(&introduction));
        assert!(story < 10_000);
        assert_eq!(starved.len(), 2);
        assert_eq!((starved[0].0, starved[1].0), (162 + 3, 162 + 1_819));
        let zeros: usize = starved.iter().map(|run| run.1).sum();
        assert!((21..=22).contains(&zeros));
        // 38,641 frames of introduction end 239 frames short of a block.
        assert_eq!(padding, 239);

        let (introduction, story, starved, padding) =
            sixth_turn(StartGate::continuous(Continuity::EVALUATED), origin);
        assert!((290_000..300_000).contains(&introduction));
        assert!((290_000..300_000).contains(&story));
        assert_eq!(starved, []);
        assert_eq!(padding, 239);
    }
}

fn synthetic(arrivals: &[(u64, usize)], complete_us: u64) -> Recorded {
    Recorded {
        name: "synthetic",
        complete_us,
        first_us: 0,
        after_us: None,
        gaps: Vec::new(),
        arrivals: arrivals.to_vec(),
    }
}

#[test]
fn a_short_reply_that_is_complete_is_not_held() {
    // 200 ms of audio, declared complete 30 ms after it arrived. It covers
    // the plain horizon by itself. Continuity waits for the flag, not longer.
    let reply = synthetic(&[(0, 4_800)], 30_000);
    let played = play(&reply, StartGate::new(START_HORIZON_US), 0);
    assert_eq!((played.first_us, played.opened.by), (0, OpenedBy::Covered));
    let played = play(&reply, StartGate::continuous(Continuity::EVALUATED), 0);
    assert_eq!(
        (played.first_us, played.opened.by),
        (30_000, OpenedBy::Complete)
    );
    assert_eq!(played.starved, []);
    // 60 ms of audio: both gates hold it until it is complete, and no longer.
    let reply = synthetic(&[(0, 1_440)], 30_000);
    for gate in [
        StartGate::new(START_HORIZON_US),
        StartGate::continuous(Continuity::EVALUATED),
    ] {
        let played = play(&reply, gate, 0);
        assert_eq!(
            (played.first_us, played.opened.by),
            (30_000, OpenedBy::Complete)
        );
    }
    // The same 200 ms without the flag: continuity waits out its hold.
    let reply = synthetic(&[(0, 4_800)], 600_000);
    let played = play(&reply, StartGate::continuous(Continuity::EVALUATED), 0);
    assert_eq!(
        (played.first_us, played.opened.by),
        (290_000, OpenedBy::HoldBound)
    );
}

/// A service that sends 200 ms every 200 ms and leads by 100 ms, with one
/// burst late by `late_us`.
fn steady(late_us: u64) -> Recorded {
    let mut arrivals = vec![(0, 4_800)];
    for burst in 1..20 {
        let at = burst * 200_000 - 100_000 + if burst == 10 { late_us } else { 0 };
        arrivals.push((at, 4_800));
    }
    synthetic(&arrivals, 3_900_000)
}

#[test]
fn a_burst_later_than_the_margin_still_starves_and_playout_resumes() {
    // 300 ms late: ridden out. The start saw the 100 ms lead at the second
    // burst and opened when the hold made up the rest of the margin.
    let played = play(
        &steady(300_000),
        StartGate::continuous(Continuity::EVALUATED),
        0,
    );
    assert_eq!(
        (played.first_us, played.opened.by),
        (240_000, OpenedBy::Margin)
    );
    assert_eq!(played.lead_us, 100_000);
    assert_eq!(played.starved, []);
    // 700 ms late: nothing bounded covers it, and nothing here pretends to.
    // The 340 ms margin, less the withheld block, shortens the hole to 370 ms.
    let played = play(
        &steady(700_000),
        StartGate::continuous(Continuity::EVALUATED),
        0,
    );
    assert_eq!(played.starved, [(199, 37)]);
    // The current gate started 240 ms earlier and loses that much more.
    let played = play(&steady(700_000), StartGate::new(START_HORIZON_US), 0);
    assert_eq!(played.first_us, 0);
    assert_eq!(played.starved, [(199, 61)]);
    // And the 300 ms it does not ride out.
    let played = play(&steady(300_000), StartGate::new(START_HORIZON_US), 0);
    assert_eq!(played.starved, [(199, 21)]);
}

#[test]
fn a_reply_cancelled_while_held_has_handed_nothing_over() {
    let reply = reply("six/5");
    let mut queue = Queue::new(StartGate::continuous(Continuity::EVALUATED));
    let mut next = 0;
    for now in (0..200_000).step_by(SLOT_US as usize) {
        while let Some(&(at, frames)) = reply.arrivals.get(next).filter(|(at, _)| *at <= now) {
            queue.append(frames, at);
            next += 1;
        }
        assert!(!queue.may_start(now), "held at {now}");
    }
    // 320 ms had arrived and all of it is still the caller's to drop.
    assert_eq!(queue.pcm.len(), 7_680);
    assert_eq!(queue.start.opened(), None);
    assert!(queue.pcm.iter().copied().eq((0..7_680).map(sample)));
    queue.pcm.clear();
    queue.start.reset();
    // The next reply is held from its own first audio.
    queue.append(4_800, 5_000_000);
    assert!(!queue.may_start(5_289_999));
    assert!(queue.may_start(5_290_000));
}

/// Not a pass or fail: prints what one decision costs. Run with
/// `--ignored --nocapture`, in a release build for a figure worth quoting.
#[test]
#[ignore = "prints a timing"]
fn cost_of_the_gate() {
    let reply = reply("six/7");
    let rounds = 200;
    let started = std::time::Instant::now();
    let mut calls = 0_u64;
    for round in 0..rounds {
        let mut gate = StartGate::continuous(Continuity::EVALUATED);
        let mut queued = 0;
        for &(at, frames) in &reply.arrivals {
            gate.arrival(at, frames);
            queued += frames;
            // One decision per 10 ms slot is what the coordinator asks for.
            std::hint::black_box(gate.open(at + round, queued, false));
            calls += 2;
        }
        std::hint::black_box(gate.lead_us());
    }
    let each = started.elapsed().as_nanos() as f64 / calls as f64;
    println!(
        "{calls} calls, {each:.1} ns each; the gate is {} bytes and allocates nothing",
        std::mem::size_of::<StartGate>()
    );
}
