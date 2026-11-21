//! A line that ends leaves its trailing quiet as the next line's pre-roll
//! (`LAMP_END_KEEPS_REACH_BACK=on`, #7095): at most 600 ms of it, never a
//! block from its last speech back, so a request said straight after a bare
//! "Hey Lamp," that ended as its own line begins before its first word. The
//! ended line's own audio, and everything with the switch off, is as before.
//!
//! No audio: a constructed run of speech and quiet blocks with the measured
//! timings (the first builder's ruling on #7095).
use lamp_voice::activity::{
    Activity, END_LEAD_BLOCKS, ObservedAudio, PRE_ROLL_BLOCKS, REACH_BACK_BLOCKS,
    SOFT_START_LEAD_BLOCKS, START_BLOCKS, TurnDetector,
};

const BLOCK_US: u64 = 10_000;

fn frame(sequence: u64) -> ObservedAudio {
    ObservedAudio {
        sequence,
        captured_at_us: sequence * BLOCK_US,
        samples: [0; 160],
    }
}

/// "Hey Lamp," at the office's rate: 0.7 s of speech.
const HEY_LAMP: [f32; 70] = [0.95; 70];

/// The product's end rule, without its model: the 600 ms window
/// (`--end-silence-ms 600`) and the call's 2 s wait (`--call-wait-ms 2000`),
/// with first words kept and soft starts as `first_words` and `soft` say.
fn detector(end_lead: bool, first_words: bool, soft: bool, end_silence: u16) -> TurnDetector {
    let mut detector = TurnDetector::v1_baseline()
        .with_end_silence(end_silence)
        .unwrap()
        .with_call_wait(200)
        .unwrap();
    detector.keep_first_words(first_words);
    detector.keep_soft_starts(first_words && soft);
    detector.keep_end_leads(end_lead);
    detector
}

/// One push, by its kind and the sequences of the audio it gave.
type Told = (&'static str, Vec<u64>);

fn told(activity: &Activity) -> Told {
    match activity {
        Activity::Quiet => ("quiet", Vec::new()),
        Activity::Start(audio) => ("start", audio.iter().map(|f| f.sequence).collect()),
        Activity::Continue(audio) => ("continue", vec![audio.sequence]),
        Activity::End(audio) => ("end", vec![audio.sequence]),
        Activity::Fault(reason) => panic!("fault: {reason}"),
    }
}

/// How the scene went.
struct Run {
    /// Every push, in order, the next line's start last.
    pushes: Vec<Told>,
    /// The ended line's audio, by sequence, from its start to its End.
    line: Vec<u64>,
    /// Its last speech block, and the block that ended it.
    last_speech: u64,
    ended: u64,
    /// When the verb's first block was captured.
    onset_us: u64,
    /// The next line's start.
    next: Vec<ObservedAudio>,
}

/// What is said and how the room scores between.
struct Scene<'a> {
    /// The call's line, block by block, then quiet until it ends.
    call: &'a [f32],
    /// The lamp's name heard at the call's start, for its wait.
    named: bool,
    /// Quiet blocks after the End before the verb's onset.
    gap: u64,
    /// Each quiet block's own score before any mask.
    quiet_own: f32,
    /// The next line is a yield's, which reaches back by its own scores.
    yield_start: bool,
}

impl Scene<'_> {
    /// "Hey Lamp," that ends as its own line (no name heard in time), then
    /// the verb 20 ms after its End: ten's measure on relax.wav.
    fn measured() -> Self {
        Scene {
            call: &HEY_LAMP,
            named: false,
            gap: 2,
            quiet_own: 0.0,
            yield_start: false,
        }
    }

    fn run(&self, mut detector: TurnDetector) -> Run {
        let mut sequence = 0;
        let mut pushes = Vec::new();
        let mut push = |detector: &mut TurnDetector, probability: f32, own: f32| {
            sequence += 1;
            detector.lamp_silent(true);
            let activity = detector.push_heard(frame(sequence), probability, Some(own));
            pushes.push(told(&activity));
            (sequence, activity)
        };
        let mut line = Vec::new();
        let keep = |line: &mut Vec<u64>, activity: &Activity| match activity {
            Activity::Start(audio) => line.extend(audio.iter().map(|f| f.sequence)),
            Activity::Continue(audio) | Activity::End(audio) => line.push(audio.sequence),
            _ => {}
        };
        // The room before: quiet.
        for _ in 0..50 {
            let (_, activity) = push(&mut detector, 0.0, self.quiet_own);
            assert!(matches!(activity, Activity::Quiet));
        }
        let mut last_speech = 0;
        for &probability in self.call {
            let own = if probability >= 0.35 {
                probability
            } else {
                self.quiet_own
            };
            let (at, activity) = push(&mut detector, probability, own);
            if probability >= 0.35 {
                last_speech = at;
            }
            keep(&mut line, &activity);
        }
        assert!(!line.is_empty(), "the call never started a line");
        if self.named {
            assert!(detector.name_heard(51 * BLOCK_US), "the call did not wait");
        }
        let mut ended = None;
        for _ in 0..400 {
            let (at, activity) = push(&mut detector, 0.0, self.quiet_own);
            keep(&mut line, &activity);
            if matches!(activity, Activity::End(_)) {
                ended = Some(at);
                break;
            }
            assert!(matches!(activity, Activity::Continue(_)));
        }
        let ended = ended.expect("the call never ended");
        for _ in 0..self.gap {
            let (_, activity) = push(&mut detector, 0.0, self.quiet_own);
            assert!(matches!(activity, Activity::Quiet));
        }
        let onset_us = (ended + self.gap + 1) * BLOCK_US;
        let mut next = None;
        for n in 1..=START_BLOCKS {
            if self.yield_start && n == START_BLOCKS {
                detector.reach_back();
            }
            let (at, activity) = push(&mut detector, 0.95, 0.95);
            if let Activity::Start(audio) = activity {
                assert_eq!(n, START_BLOCKS);
                assert_eq!(audio.last().unwrap().sequence, at);
                next = Some(audio);
                break;
            }
        }
        let next = next.expect("the verb did not start a line");
        assert!(
            next.windows(2).all(|pair| pair[1].sequence == pair[0].sequence + 1),
            "the next line's audio has a gap"
        );
        Run {
            pushes,
            line,
            last_speech,
            ended,
            onset_us,
            next,
        }
    }
}

/// How long before the verb's onset the next line's sound begins.
fn lead_us(run: &Run) -> u64 {
    run.onset_us - run.next[0].captured_at_us
}

/// #7095: "Hey Lamp," ends as its own line at the 600 ms window, no name
/// heard in time, and the verb comes 20 ms after the End. With the switch
/// the next line begins at least 100 ms before the verb, as a word needs
/// (`LAMP_SOFT_START`'s evidence), with the product's first words kept and
/// soft starts on and with neither. Without it, it began 20 ms before.
#[test]
fn a_line_right_after_a_bare_call_begins_100_ms_before_its_verb() {
    for (first_words, soft) in [(true, true), (true, false), (false, false)] {
        let run = Scene::measured().run(detector(true, first_words, soft, 60));
        // The call ended as its own line, on the window, not the call's wait.
        assert_eq!(run.ended - run.last_speech, 60);
        let lead = lead_us(&run);
        assert!(
            lead >= 100_000,
            "first words {first_words} soft {soft}: the next line begins {lead} us before its verb"
        );
        // Never before the End's own trailing quiet: no more than the cap.
        assert!(run.next[0].sequence > run.last_speech);
        assert!(run.next[0].sequence + END_LEAD_BLOCKS as u64 > run.ended);
    }
}

/// With the switch off every push is as before: the next line starts with
/// only what came after the End, 20 ms before its verb. With it on, every
/// push is the same but the next start, which only adds blocks before it:
/// the ended line's own audio is unchanged.
#[test]
fn with_the_switch_off_nothing_changes_and_on_only_the_next_start_does() {
    for (first_words, soft) in [(true, true), (true, false), (false, false)] {
        let before = {
            let mut detector = TurnDetector::v1_baseline()
                .with_end_silence(60)
                .unwrap()
                .with_call_wait(200)
                .unwrap();
            detector.keep_first_words(first_words);
            detector.keep_soft_starts(first_words && soft);
            Scene::measured().run(detector)
        };
        let off = Scene::measured().run(detector(false, first_words, soft, 60));
        assert_eq!(off.pushes, before.pushes, "first words {first_words} soft {soft}");
        let after: Vec<u64> = (off.ended + 1..=off.ended + 2 + u64::from(START_BLOCKS)).collect();
        let next: Vec<u64> = off.next.iter().map(|f| f.sequence).collect();
        assert_eq!(next, after);
        assert_eq!(lead_us(&off), 20_000);

        let on = Scene::measured().run(detector(true, first_words, soft, 60));
        assert_eq!(on.line, off.line, "the ended line's audio");
        let (last, pushes) = on.pushes.split_last().unwrap();
        assert_eq!(pushes, off.pushes.split_last().unwrap().1);
        assert_eq!(last.0, "start");
        assert!(last.1.ends_with(&next), "the next start keeps what it had");
    }
}

/// The carried lead is the ended line's quiet after its last speech, never a
/// speech block of it, even when the line ended on a shorter window than the
/// cap, its call had a gap between words, and the next line is a yield's,
/// which reaches back to the earliest block its own scores call speech.
#[test]
fn the_carried_lead_never_holds_the_ended_lines_speech() {
    let mut call = vec![0.95; 30];
    call.extend([0.0; 10]);
    call.extend([0.95; 30]);
    for yield_start in [false, true] {
        let scene = Scene {
            call: &call,
            quiet_own: if yield_start { 0.5 } else { 0.0 },
            yield_start,
            ..Scene::measured()
        };
        let run = scene.run(detector(true, true, true, 30));
        assert_eq!(run.ended - run.last_speech, 30);
        // All of the 300 ms the line ended on, and nothing before it.
        assert_eq!(run.next[0].sequence, run.last_speech + 1, "yield {yield_start}");
        assert!(run.next.iter().all(|f| f.sequence > run.last_speech));
    }
}

/// The carried lead is 600 ms at most, however long the line's quiet ran
/// (here a call's 2 s wait run out), and a start keeps what the detector
/// keeps: a yield's reach back stops at the cap; an ordinary one is its
/// usual 300 ms, or 600 ms with a soft start; and without first words the
/// pre-roll's 300 ms bounds all of it.
#[test]
fn the_carried_lead_holds_to_its_cap() {
    let waited = |first_words, soft, yield_start, gap| {
        Scene {
            named: true,
            gap,
            quiet_own: 0.5,
            yield_start,
            ..Scene::measured()
        }
        .run(detector(true, first_words, soft, 60))
    };
    // A yield's start, straight after: the whole carried lead, and no more.
    let run = waited(true, false, true, 0);
    assert_eq!(run.ended - run.last_speech, 200, "the call's wait ran out");
    assert_eq!(run.next[0].sequence, run.ended + 1 - END_LEAD_BLOCKS as u64);
    assert_eq!(run.next.len(), END_LEAD_BLOCKS + usize::from(START_BLOCKS));
    assert!(run.next.len() <= REACH_BACK_BLOCKS);
    // An ordinary start with a soft start: 600 ms back from its sixth block.
    let run = waited(true, true, false, 2);
    assert_eq!(run.next.len(), PRE_ROLL_BLOCKS + SOFT_START_LEAD_BLOCKS);
    // Without soft starts, the usual 300 ms.
    let run = waited(true, false, false, 2);
    assert_eq!(run.next.len(), PRE_ROLL_BLOCKS);
    // Without first words: the pre-roll's 300 ms, carried lead and all.
    let run = waited(false, false, false, 2);
    assert_eq!(run.next.len(), PRE_ROLL_BLOCKS);
    assert_eq!(
        run.next[0].sequence,
        run.ended + 2 + u64::from(START_BLOCKS) + 1 - PRE_ROLL_BLOCKS as u64
    );
}
