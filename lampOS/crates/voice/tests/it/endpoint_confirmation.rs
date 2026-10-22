//! Model-confirmed endpoint policy with a scripted classifier: deterministic,
//! no model, no audio device. Each pushed block is 10 ms of capture.
use lamp_turn_end_wire::{
    Availability, Classifier, Outcome, Submit, Token, Verdict, WINDOW_SAMPLES,
};
use lamp_voice::activity::{
    Activity, ConfirmPolicy, EndpointDecision, ObservedAudio, TurnDetector,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Script {
    submissions: Vec<(Token, Vec<i16>)>,
    cancels: usize,
    replies: VecDeque<Submit>,
    outcomes: VecDeque<Outcome>,
    availability: Option<Availability>,
}

#[derive(Clone, Default)]
struct Scripted(Arc<Mutex<Script>>);

impl Classifier for Scripted {
    fn submit(&mut self, token: Token, pcm: &[i16]) -> Submit {
        let mut script = self.0.lock().unwrap();
        let reply = script.replies.pop_front().unwrap_or(Submit::Accepted);
        if reply == Submit::Accepted {
            script.submissions.push((token, pcm.to_vec()));
        }
        reply
    }
    fn poll(&mut self) -> Option<Outcome> {
        self.0.lock().unwrap().outcomes.pop_front()
    }
    fn availability(&self) -> Availability {
        self.0
            .lock()
            .unwrap()
            .availability
            .clone()
            .unwrap_or(Availability::Ready)
    }
    fn cancel(&mut self) {
        self.0.lock().unwrap().cancels += 1;
    }
}

impl Scripted {
    fn submissions(&self) -> Vec<Token> {
        let script = self.0.lock().unwrap();
        script.submissions.iter().map(|(token, _)| *token).collect()
    }
    fn last_pcm(&self) -> Vec<i16> {
        self.0.lock().unwrap().submissions.last().unwrap().1.clone()
    }
    fn reply(&self, token: Token, probability: f32) {
        let mut script = self.0.lock().unwrap();
        let request = script.submissions.len() as u64;
        script.outcomes.push_back(Outcome {
            token,
            worker: 1,
            request,
            result: Ok(Verdict {
                probability,
                features_us: 3_000,
                inference_us: 90_000,
                round_trip_us: 95_000,
                ..Verdict::default()
            }),
        });
    }
    fn fail(&self, token: Token) {
        self.0.lock().unwrap().outcomes.push_back(Outcome {
            token,
            worker: 1,
            request: 0,
            result: Err("inference failed".into()),
        });
    }
    fn refuse(&self, replies: &[Submit]) {
        self.0.lock().unwrap().replies.extend(replies);
    }
}

struct Rig {
    detector: TurnDetector,
    script: Scripted,
    sequence: u64,
}

const SPEECH: f32 = 0.95;
const QUIET: f32 = 0.05;

impl Rig {
    /// Mechanics tests use a two-band policy with a 400 ms commit, so each
    /// rule is visible at a short pause; the default profile has its own tests.
    fn new() -> Self {
        Self::with(mechanics())
    }

    fn with(policy: ConfirmPolicy) -> Self {
        Self::with_window(policy, None)
    }

    /// With the detector's own silence window, the policy's fallback, of
    /// `blocks` (the lamp's `--end-silence-ms 600` is 60).
    fn with_window(policy: ConfirmPolicy, blocks: Option<u16>) -> Self {
        let script = Scripted::default();
        let mut detector = TurnDetector::v1_baseline();
        if let Some(blocks) = blocks {
            detector = detector.with_end_silence(blocks).unwrap();
        }
        let detector = detector
            .with_confirmation(Box::new(script.clone()), policy)
            .unwrap();
        Self {
            detector,
            script,
            sequence: 0,
        }
    }

    /// Every sample carries its block's sequence number for retention checks.
    fn push(&mut self, probability: f32) -> Activity {
        self.sequence += 1;
        let audio = ObservedAudio {
            sequence: self.sequence,
            captured_at_us: 1_000_000 + self.sequence * 10_000,
            samples: [(self.sequence % 30_000) as i16; 160],
        };
        self.detector.push(audio, probability)
    }

    /// Six onset blocks (Start when idle, Continue when resuming), then
    /// `blocks` more speech blocks.
    fn speak(&mut self, blocks: usize) {
        for _ in 0..6 + blocks {
            let activity = self.push(SPEECH);
            assert!(
                matches!(
                    activity,
                    Activity::Quiet | Activity::Start(_) | Activity::Continue(_)
                ),
                "{activity:?}"
            );
        }
    }

    /// Pushes quiet blocks until an endpoint or the limit; returns the
    /// 1-based quiet block that ended the turn.
    fn quiet_until_end(&mut self, limit: u16) -> Option<u16> {
        (1..=limit).find(|_| matches!(self.push(QUIET), Activity::End(_)))
    }

    fn quiet(&mut self, blocks: u16) {
        for _ in 0..blocks {
            assert!(
                matches!(self.push(QUIET), Activity::Continue(_)),
                "unexpected endpoint at block {}",
                self.sequence
            );
        }
    }

    fn decision(&self) -> EndpointDecision {
        self.detector.last_endpoint().unwrap().clone()
    }
}

#[test]
fn complete_verdict_ends_at_the_commit_pause() {
    let mut rig = Rig::new();
    rig.speak(50);
    rig.quiet(19);
    assert!(rig.script.submissions().is_empty());
    rig.quiet(1);
    let tokens = rig.script.submissions();
    assert_eq!(tokens.len(), 1);
    rig.quiet(3);
    rig.script.reply(tokens[0], 0.91);
    // Verdict known at 24 blocks; the turn ends at the 400 ms commit pause.
    rig.quiet(16);
    assert!(matches!(rig.push(QUIET), Activity::End(_)));
    let decision = rig.decision();
    assert_eq!(decision.basis, "model_complete");
    assert_eq!(decision.silence_ms, 400);
    assert_eq!(decision.probability, Some(0.91));
    let verdict = decision.verdict.unwrap();
    assert_eq!((verdict.worker, verdict.request), (1, 1));
    assert_eq!((verdict.submitted_at_ms, verdict.heard_at_ms), (200, 230));
    assert_eq!(verdict.inference_us, 90_000);
    // After the endpoint the detector is idle again.
    assert!(matches!(rig.push(QUIET), Activity::Quiet));
}

#[test]
fn verdict_after_the_commit_pause_ends_on_arrival() {
    let mut rig = Rig::new();
    rig.speak(50);
    rig.quiet(45);
    rig.script.reply(rig.script.submissions()[0], 0.85);
    assert!(matches!(rig.push(QUIET), Activity::End(_)));
    assert_eq!(rig.decision().silence_ms, 460);
}

fn mechanics() -> ConfirmPolicy {
    ConfirmPolicy {
        commit_blocks: 40,
        complete_probability: 0.8,
        incomplete_probability: 0.8,
        incomplete_blocks: 120,
        ..ConfirmPolicy::default()
    }
}

#[test]
fn mechanics_threshold_treats_a_moderate_verdict_as_incomplete() {
    let mut rig = Rig::new();
    rig.speak(50);
    rig.quiet(20);
    rig.script.reply(rig.script.submissions()[0], 0.79);
    assert_eq!(rig.quiet_until_end(200), Some(100));
    assert_eq!(rig.decision().basis, "model_incomplete");
}

/// Ends one 0.5 s utterance with a verdict of `probability`; returns the
/// pause length at the endpoint and its basis.
fn default_profile_end(probability: f32) -> (u64, &'static str) {
    let mut rig = Rig::with(ConfirmPolicy::default());
    rig.speak(50);
    rig.quiet(20);
    rig.script.reply(rig.script.submissions()[0], probability);
    rig.quiet_until_end(300).expect("no endpoint");
    let decision = rig.decision();
    (decision.silence_ms, decision.basis)
}

#[test]
fn default_profile_bands_end_complete_uncertain_and_incomplete_pauses() {
    let policy = ConfirmPolicy::default();
    assert_eq!(policy.check_blocks, 20);
    assert_eq!(default_profile_end(0.95), (1_000, "model_complete"));
    assert_eq!(default_profile_end(0.9), (1_000, "model_complete"));
    assert_eq!(default_profile_end(0.899), (1_200, "model_uncertain"));
    assert_eq!(default_profile_end(0.01), (1_200, "model_uncertain"));
    assert_eq!(default_profile_end(0.009), (1_500, "model_incomplete"));
    assert_eq!(default_profile_end(0.0), (1_500, "model_incomplete"));
}

#[test]
fn default_profile_holds_unfinished_speech_but_resumes_on_speech() {
    let mut rig = Rig::with(ConfirmPolicy::default());
    rig.speak(50);
    rig.quiet(20);
    rig.script.reply(rig.script.submissions()[0], 0.005);
    // Beyond the 1.2 s silence window, the clearly unfinished turn is held...
    rig.quiet(125);
    // ...and the person continues it: the same utterance, a new verdict.
    rig.speak(20);
    rig.quiet(20);
    let tokens = rig.script.submissions();
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[1].utterance, tokens[0].utterance);
    rig.script.reply(tokens[1], 0.97);
    assert_eq!(rig.quiet_until_end(200), Some(80));
    assert_eq!(rig.decision().basis, "model_complete");
}

#[test]
fn responsive_profile_is_explicit_and_commits_at_400_ms() {
    let policy = ConfirmPolicy::responsive();
    assert_eq!(policy.commit_blocks, 40);
    assert_eq!(policy.least_speech_blocks, 80);
    assert_ne!(policy, ConfirmPolicy::default());
    let mut rig = Rig::with(policy);
    rig.speak(90);
    rig.quiet(20);
    rig.script.reply(rig.script.submissions()[0], 0.95);
    assert_eq!(rig.quiet_until_end(200), Some(20));
    assert_eq!(rig.decision().silence_ms, 400);
}

#[test]
fn snapshot_holds_preroll_opening_and_resumed_speech_in_order() {
    let mut rig = Rig::new();
    for _ in 0..24 {
        assert!(matches!(rig.push(QUIET), Activity::Quiet));
    }
    rig.speak(30);
    rig.quiet(25);
    let first = rig.script.submissions()[0];
    rig.speak(20);
    rig.quiet(20);
    let tokens = rig.script.submissions();
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[1].utterance, first.utterance);
    assert_eq!(tokens[1].generation, first.generation + 1);
    let pcm = rig.script.last_pcm();
    // 30 pre-roll/onset blocks (sequences 1..=30), then every later block once.
    let blocks = pcm.chunks(160).map(|block| block[0]).collect::<Vec<_>>();
    assert!(
        pcm.chunks(160)
            .all(|block| block.iter().all(|&s| s == block[0]))
    );
    assert_eq!(blocks.first(), Some(&1));
    assert_eq!(blocks, (1..=rig.sequence as i16).collect::<Vec<_>>());
}

#[test]
fn retained_snapshot_is_the_latest_model_window() {
    let mut rig = Rig::new();
    rig.speak(1_200);
    rig.quiet(20);
    let pcm = rig.script.last_pcm();
    assert_eq!(pcm.len(), WINDOW_SAMPLES);
    assert_eq!(*pcm.last().unwrap(), rig.sequence as i16);
    assert_eq!(pcm[0], (rig.sequence - 799) as i16);
}

/// The lamp's name said alone, then a pause: a confident verdict ends it
/// only at the full silence window, so the question that follows stays in
/// the same turn. A long enough utterance still ends at the early commit.
#[test]
fn a_few_words_are_not_sent_to_the_model_and_end_on_the_silence_window() {
    let floor = ConfirmPolicy {
        least_speech_blocks: 80,
        ..mechanics()
    };
    // Too short to count: not even sent to the model (it took a whole
    // core in a talkative office), and ended on the silence window.
    let mut short = Rig::with(floor);
    short.speak(60);
    short.quiet(20);
    assert!(short.script.submissions().is_empty());
    let window = floor.fallback_blocks;
    assert_eq!(short.quiet_until_end(400), Some(window - 20));
    let decision = short.decision();
    assert_eq!(
        (decision.basis, decision.silence_ms, decision.fallback),
        ("silence_fallback", u64::from(window) * 10, Some("short"))
    );
    let mut long = Rig::with(floor);
    long.speak(120);
    long.quiet(20);
    long.script.reply(long.script.submissions()[0], 0.99);
    assert_eq!(long.quiet_until_end(200), Some(20));
    assert_eq!(long.decision().silence_ms, 400);
}

/// #2135 (`LAMP_ENDPOINT_SHORT_VERDICT`): with the floor's short lines
/// asked, a clearly unfinished one ("Hey Lamp, my…" cut 0.66 s in on
/// lamp-4ace) holds to the incomplete window, and the rest that comes stays
/// in the line; a complete one ("Yes.") and an uncertain one end on the
/// silence window as before, never sooner. Under 20 blocks, or with the
/// switch's floor 0, nothing is asked, and it ends as `short`.
#[test]
fn with_short_verdicts_an_unfinished_short_line_waits_and_a_finished_one_ends_as_before() {
    let asked = ConfirmPolicy {
        least_speech_blocks: 80,
        short_verdict_blocks: 20,
        incomplete_probability: 0.01,
        incomplete_blocks: 150,
        ..mechanics()
    };
    assert!(asked.validate().is_ok());
    let window = asked.fallback_blocks;
    // Unfinished: held past the window, to the incomplete one.
    let mut unfinished = Rig::with(asked);
    unfinished.speak(60);
    unfinished.quiet(20);
    let token = unfinished.script.submissions()[0];
    unfinished.script.reply(token, 0.004);
    // Held past the silence window (`quiet` fails on an End).
    unfinished.quiet(window - 20);
    // The rest comes: the same line goes on.
    unfinished.speak(40);
    unfinished.quiet(20);
    assert_eq!(unfinished.script.submissions().len(), 2);
    assert_eq!(
        unfinished.script.submissions()[1].utterance,
        token.utterance
    );
    // Unfinished and nothing more: ended at the incomplete window.
    let mut alone = Rig::with(asked);
    alone.speak(60);
    alone.quiet(20);
    alone.script.reply(alone.script.submissions()[0], 0.004);
    assert_eq!(alone.quiet_until_end(400), Some(150 - 20));
    assert_eq!(alone.decision().basis, "model_incomplete");
    // Counted: the switch's own submission says so on its End.
    assert_eq!(alone.decision().short_asks, 1);
    // Paused twice before the floor: asked twice, counted twice (the
    // reviewer's P3 on #2380).
    let mut twice = Rig::with(asked);
    twice.speak(30);
    twice.quiet(20);
    twice.script.reply(twice.script.submissions()[0], 0.004);
    twice.quiet(10);
    twice.speak(10);
    twice.quiet(20);
    assert_eq!(twice.script.submissions().len(), 2);
    twice.script.reply(twice.script.submissions()[1], 0.99);
    twice.quiet_until_end(400);
    assert_eq!(twice.decision().short_asks, 2);
    // Finished, or uncertain: the silence window, as before.
    for (probability, basis) in [(0.99, "model_complete"), (0.5, "model_uncertain")] {
        let mut rig = Rig::with(asked);
        rig.speak(60);
        rig.quiet(20);
        rig.script.reply(rig.script.submissions()[0], probability);
        assert_eq!(rig.quiet_until_end(400), Some(window - 20), "{basis}");
        assert_eq!(rig.decision().basis, basis);
        assert_eq!(rig.decision().silence_ms, u64::from(window) * 10);
        assert_eq!(rig.decision().short_asks, 1);
    }
    // A scrap under the switch's floor, or the switch off: not asked.
    for (policy, blocks) in [
        (asked, 5),
        (
            ConfirmPolicy {
                short_verdict_blocks: 0,
                ..asked
            },
            60,
        ),
    ] {
        let mut rig = Rig::with(policy);
        rig.speak(blocks);
        rig.quiet(20);
        assert!(rig.script.submissions().is_empty(), "{blocks}");
        assert_eq!(rig.quiet_until_end(400), Some(window - 20));
        assert_eq!(rig.decision().fallback, Some("short"));
        assert_eq!(rig.decision().short_asks, 0);
    }
    // A line long enough for the floor is asked as before, not counted.
    let mut long = Rig::with(asked);
    long.speak(90);
    long.quiet(20);
    long.script.reply(long.script.submissions()[0], 0.99);
    long.quiet_until_end(400);
    assert_eq!(long.decision().short_asks, 0);
}

/// The lamp's responsive profile under its 600 ms window: as it is, or as
/// `LAMP_ENDPOINT_LEAST_SPEECH_MS` sets it (#2602).
fn lamp(policy: ConfirmPolicy) -> Rig {
    Rig::with_window(policy, Some(60))
}

/// Speaks `blocks` after the six start blocks, pauses, answers the verdict
/// complete if one was asked, and returns the End's basis, pause and
/// counted speech in ms.
fn end_after(mut rig: Rig, blocks: usize) -> (&'static str, u64, Option<u64>) {
    rig.speak(blocks);
    rig.quiet(20);
    if let Some(token) = rig.script.submissions().first() {
        rig.script.reply(*token, 0.95);
    }
    rig.quiet_until_end(200).expect("an End");
    let decision = rig.decision();
    (decision.basis, decision.silence_ms, decision.speech_ms)
}

/// The first builder on #2602 (22 Oct 04:28): on lamp-4ace a third of whole
/// played day lines ended as `silence_fallback: short`, never asked, at
/// 600 ms. A 0.6 s line (six start blocks and 54) is one of them as the
/// lamp is; with `LAMP_ENDPOINT_LEAST_SPEECH_MS=500` its complete verdict
/// ends it at the 400 ms commit. Each End says the speech it counted.
#[test]
fn a_lower_floor_ends_a_short_finished_line_at_the_commit() {
    let off = ConfirmPolicy::responsive();
    assert_eq!(
        end_after(lamp(off), 54),
        ("silence_fallback", 600, Some(540))
    );
    assert_eq!(
        end_after(lamp(off.with_floor_ms(500)), 54),
        ("model_complete", 400, Some(600))
    );
    let mut short = lamp(off);
    short.speak(54);
    short.quiet_until_end(200);
    assert_eq!(short.decision().fallback, Some("short"));
}

/// #2602: the line's start counts toward the floor only with the switch.
/// 46 blocks after the start are under a 0.5 s floor, the six start blocks
/// with them (52) over it: asked and ended at the commit with the start
/// counted, `short` at the window without.
#[test]
fn the_start_counts_toward_the_floor_only_with_the_switch() {
    let not_counted = ConfirmPolicy {
        least_speech_blocks: 50,
        ..ConfirmPolicy::responsive()
    };
    assert_eq!(
        end_after(lamp(not_counted), 46),
        ("silence_fallback", 600, Some(460))
    );
    assert_eq!(
        end_after(lamp(ConfirmPolicy::responsive().with_floor_ms(500)), 46),
        ("model_complete", 400, Some(520))
    );
}

/// The first builder's P2 on #2751 (22 Oct 06:10): at 400 ms the replay
/// committed "Hey Lamp," said alone, the line the 0.8 s floor was set for
/// on 14 Oct, so the switch takes 500 at the least. That floor alone does
/// not hold the name: "Hey Lamp," is about 0.6 s of speech, its start
/// counted, over 500, and a plain 0.6 s line with a complete verdict is
/// committed at 400 ms (the second case). What holds it is the call's wait
/// as the lamp runs it (`--call-wait-ms 2000`, #782; `call_wait_policy`'s
/// most speech 1,200 ms): with the name's sign, the complete verdict ends
/// nothing, a 1 s pause later the question comes, and the line ends on the
/// question's own commit. A line under 500 is still held to the window.
#[test]
fn the_name_alone_still_waits_for_the_rest_with_a_lower_floor() {
    let policy = ConfirmPolicy::responsive().with_floor_ms(500);
    let script = Scripted::default();
    let detector = TurnDetector::v1_baseline()
        .with_end_silence(60)
        .unwrap()
        .with_call_wait(200)
        .unwrap()
        .with_confirmation(Box::new(script.clone()), policy)
        .unwrap();
    let mut called = Rig {
        detector,
        script,
        sequence: 0,
    };
    // "Hey Lamp," (six start blocks and 54), the name's sign as it began.
    called.speak(54);
    assert!(called.detector.name_heard(1_010_000));
    called.quiet(20);
    called.script.reply(called.script.submissions()[0], 0.99);
    // A second's pause: not ended at the 400 ms commit, nor the window.
    called.quiet(80);
    // The question: the same line, ended on its own complete verdict.
    called.speak(100);
    called.quiet(20);
    let asked = called.script.submissions();
    assert_eq!(asked.len(), 2);
    assert_eq!(asked[1].utterance, asked[0].utterance);
    called.script.reply(asked[1], 0.99);
    assert_eq!(called.quiet_until_end(200), Some(20));
    let decision = called.decision();
    assert_eq!(
        (decision.basis, decision.silence_ms, decision.call_wait_ms),
        ("model_complete", 400, None)
    );
    assert_eq!(decision.speech_ms, Some(1_660));
    // No name's sign: the same 0.6 s line is over the floor and committed.
    assert_eq!(
        end_after(lamp(policy), 54),
        ("model_complete", 400, Some(600))
    );
    // Under the floor (0.4 s): held to the window, never asked.
    assert_eq!(
        end_after(lamp(policy), 34),
        ("silence_fallback", 600, Some(400))
    );
}

/// The Reviewer-lead's P3 on #2756: the lamp's-name sign may come late on
/// the bus, after the pause after "Hey Lamp," has begun. `name_heard` takes
/// the name's own start (`began_us`), so a late sign still finds the line
/// it belongs to, sets the wait, and the wait counts the pause from its own
/// start (`silence_count` runs on; `call_wait_blocks` is reached at 2 s of
/// quiet, not 2 s after the sign). A sign at 150 ms (before the verdict is
/// asked at 200) or at 300 ms (after the complete verdict, before the
/// 400 ms commit) holds the line as one heard before the pause does, and
/// the question after a 1 s pause ends at its own commit. The bound: a sign
/// after the commit, at 450 ms, comes after the End, so `name_heard` finds
/// no line and says so; "Hey Lamp," was already committed alone.
#[test]
fn a_late_name_sign_in_the_pause_still_holds_the_name_alone_with_a_lower_floor() {
    fn called() -> Rig {
        let script = Scripted::default();
        let detector = TurnDetector::v1_baseline()
            .with_end_silence(60)
            .unwrap()
            .with_call_wait(200)
            .unwrap()
            .with_confirmation(
                Box::new(script.clone()),
                ConfirmPolicy::responsive().with_floor_ms(500),
            )
            .unwrap();
        Rig {
            detector,
            script,
            sequence: 0,
        }
    }
    for sign_at in [15, 30] {
        let mut rig = called();
        // "Hey Lamp," (six start blocks and 54); its sign not yet in.
        rig.speak(54);
        if sign_at < 20 {
            rig.quiet(sign_at);
            // Late, but carrying when the name began, with the line.
            assert!(rig.detector.name_heard(1_010_000), "sign at {sign_at}0 ms");
            rig.quiet(20 - sign_at);
            rig.script.reply(rig.script.submissions()[0], 0.99);
        } else {
            rig.quiet(20);
            rig.script.reply(rig.script.submissions()[0], 0.99);
            rig.quiet(sign_at - 20);
            assert!(rig.detector.name_heard(1_010_000), "sign at {sign_at}0 ms");
        }
        // The rest of a second's pause: not ended at the 400 ms commit.
        rig.quiet(100 - sign_at.max(20));
        // The question, ended on its own complete verdict at its commit.
        rig.speak(100);
        rig.quiet(20);
        let asked = rig.script.submissions();
        assert_eq!(asked.len(), 2, "sign at {sign_at}0 ms");
        rig.script.reply(asked[1], 0.99);
        assert_eq!(rig.quiet_until_end(200), Some(20), "sign at {sign_at}0 ms");
        let decision = rig.decision();
        assert_eq!(
            (decision.basis, decision.silence_ms, decision.call_wait_ms),
            ("model_complete", 400, None),
            "sign at {sign_at}0 ms"
        );
        assert_eq!(decision.speech_ms, Some(1_660), "sign at {sign_at}0 ms");
    }
    // After the commit (450 ms): "Hey Lamp," already ended alone, and the
    // sign finds no line to hold.
    let mut rig = called();
    rig.speak(54);
    rig.quiet(20);
    rig.script.reply(rig.script.submissions()[0], 0.99);
    assert_eq!(rig.quiet_until_end(200), Some(20));
    assert_eq!(
        (rig.decision().basis, rig.decision().silence_ms),
        ("model_complete", 400)
    );
    for _ in 0..5 {
        assert!(matches!(rig.push(QUIET), Activity::Quiet));
    }
    assert!(!rig.detector.name_heard(1_010_000));
}

/// #2135 (`ConfirmPolicy::recheck_uncertain`, off unless set; kept to
/// measure, the first builder on #1722 at 07:48): an uncertain verdict is
/// asked again at once with the pause so far. A complete second ends the
/// line once the commit's pause is reached, said so (`rechecked_from`); an uncertain or failed
/// second, or one still out at the window, leaves the first's, so the line
/// ends at the window as without it, never later. Without the policy no
/// second is asked.
#[test]
fn an_uncertain_verdict_asked_again_ends_the_line_only_on_a_complete_second() {
    // The lamp's window, 600 ms: a second asked at its first's answer is
    // still out when the window comes, before its own result timeout.
    let policy = ConfirmPolicy {
        recheck_uncertain: true,
        complete_probability: 0.9,
        incomplete_probability: 0.01,
        ..mechanics()
    };
    let window = 60;
    // Complete second: ended when it comes, past the commit.
    let mut rig = Rig::with_window(policy, Some(window));
    rig.speak(60);
    rig.quiet(20);
    rig.script.reply(rig.script.submissions()[0], 0.8);
    rig.quiet(1);
    let tokens = rig.script.submissions();
    assert_eq!(tokens.len(), 2, "asked again at once");
    assert_eq!(tokens[1].check, 1);
    assert_eq!(tokens[1].utterance, tokens[0].utterance);
    rig.script.reply(tokens[1], 0.95);
    // At the commit (40 blocks), never sooner, and long before the window.
    assert_eq!(rig.quiet_until_end(400), Some(40 - 21));
    let decision = rig.decision();
    assert_eq!(decision.basis, "model_complete");
    assert_eq!(decision.silence_ms, 400);
    assert_eq!(decision.rechecked_from, Some(0.8));
    // Uncertain, failed, or never back: the first's, at the window.
    for second in ["uncertain", "failed", "never"] {
        let mut rig = Rig::with_window(policy, Some(window));
        rig.speak(60);
        rig.quiet(20);
        rig.script.reply(rig.script.submissions()[0], 0.8);
        rig.quiet(1);
        let token = rig.script.submissions()[1];
        match second {
            "uncertain" => rig.script.reply(token, 0.5),
            "failed" => rig.script.fail(token),
            _ => {}
        }
        assert_eq!(rig.quiet_until_end(400), Some(window - 21), "{second}");
        let decision = rig.decision();
        assert_eq!(decision.basis, "model_uncertain", "{second}");
        assert_eq!(decision.probability, Some(0.8), "{second}");
        assert_eq!(decision.rechecked_from, None, "{second}");
    }
    // Off: no second verdict, the window as before.
    let mut off = Rig::with_window(
        ConfirmPolicy {
            recheck_uncertain: false,
            ..policy
        },
        Some(window),
    );
    off.speak(60);
    off.quiet(20);
    off.script.reply(off.script.submissions()[0], 0.8);
    assert_eq!(off.quiet_until_end(400), Some(window - 20));
    assert_eq!(off.script.submissions().len(), 1);
}

#[test]
fn incomplete_verdict_holds_until_the_incomplete_window() {
    for (window, expected) in [(120u16, 120u16), (200, 200)] {
        let mut rig = Rig::with(ConfirmPolicy {
            incomplete_blocks: window,
            ..mechanics()
        });
        rig.speak(40);
        rig.quiet(20);
        rig.script.reply(rig.script.submissions()[0], 0.12);
        assert_eq!(rig.quiet_until_end(400), Some(expected - 20));
        let decision = rig.decision();
        assert_eq!(decision.basis, "model_incomplete");
        assert_eq!(decision.silence_ms, u64::from(expected) * 10);
        assert_eq!(decision.probability, Some(0.12));
        // One classification per pause: no recheck of the same speech.
        assert_eq!(rig.script.submissions().len(), 1);
    }
}

#[test]
fn resumed_speech_discards_a_pending_complete_verdict() {
    let mut rig = Rig::new();
    rig.speak(40);
    rig.quiet(22);
    let stale = rig.script.submissions()[0];
    rig.speak(10);
    // The verdict about the earlier pause arrives during the new speech and
    // again during the next pause; neither may end the turn.
    rig.script.reply(stale, 0.99);
    assert!(matches!(rig.push(SPEECH), Activity::Continue(_)));
    rig.quiet(19);
    rig.script.reply(stale, 0.99);
    rig.quiet(1);
    let fresh = rig.script.submissions()[1];
    assert_ne!(fresh, stale);
    rig.script.reply(stale, 0.99);
    rig.quiet(5);
    rig.script.reply(fresh, 0.97);
    assert_eq!(rig.quiet_until_end(40), Some(15));
    assert_eq!(rig.decision().basis, "model_complete");
}

#[test]
fn any_speech_scored_block_restarts_the_pause_like_the_baseline() {
    let mut rig = Rig::new();
    rig.speak(40);
    rig.quiet(20);
    let first = rig.script.submissions()[0];
    // One speech-scored block (a breath or noise) ends this pause: its
    // verdict is obsolete even when it arrives during the next pause.
    assert!(matches!(rig.push(SPEECH), Activity::Continue(_)));
    rig.script.reply(first, 0.99);
    rig.quiet(19);
    assert_eq!(rig.script.submissions().len(), 1);
    rig.quiet(1);
    let second = rig.script.submissions()[1];
    assert_eq!(second.utterance, first.utterance);
    assert_eq!(second.generation, first.generation + 1);
    rig.script.reply(second, 0.95);
    assert_eq!(rig.quiet_until_end(40), Some(20));
    let decision = rig.decision();
    assert_eq!(decision.silence_ms, 400);
    assert_eq!(decision.verdict.unwrap().request, 2);
}

/// A deterministic mix of speech runs, pauses of every length and isolated
/// speech-scored blocks inside pauses.
fn probabilities(seed: u32, blocks: usize) -> Vec<f32> {
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    let mut out = Vec::with_capacity(blocks);
    while out.len() < blocks {
        let speech = 6 + next() % 120;
        out.extend((0..speech).map(|_| SPEECH));
        let pause = next() % 300;
        for _ in 0..pause {
            out.push(if next() % 150 == 0 { 0.5 } else { QUIET });
        }
    }
    out.truncate(blocks);
    out
}

fn ends(detector: &mut TurnDetector, probabilities: &[f32]) -> Vec<usize> {
    probabilities
        .iter()
        .enumerate()
        .filter_map(|(index, &probability)| {
            let audio = ObservedAudio {
                sequence: index as u64 + 1,
                captured_at_us: 1_000_000 + index as u64 * 10_000,
                samples: [7; 160],
            };
            matches!(detector.push(audio, probability), Activity::End(_)).then_some(index)
        })
        .collect()
}

#[test]
fn fallback_ends_every_turn_exactly_where_the_baseline_does() {
    for seed in 1..=40 {
        let sequence = probabilities(seed, 6_000);
        let baseline = ends(&mut TurnDetector::v1_baseline(), &sequence);
        assert!(baseline.len() > 5, "seed {seed}");
        // A missing model, and a model that is never decisive.
        let missing = Scripted::default();
        missing.0.lock().unwrap().availability = Some(Availability::Failed("missing".into()));
        missing.refuse(&[Submit::Unavailable; 400]);
        let mut detector = TurnDetector::v1_baseline()
            .with_confirmation(Box::new(missing), ConfirmPolicy::default())
            .unwrap();
        assert_eq!(ends(&mut detector, &sequence), baseline, "seed {seed}");
        let undecided = Undecided::default();
        let mut detector = TurnDetector::v1_baseline()
            .with_confirmation(Box::new(undecided), ConfirmPolicy::default())
            .unwrap();
        assert_eq!(ends(&mut detector, &sequence), baseline, "seed {seed}");
    }
}

/// Answers every request on the next poll with an uncertain verdict.
#[derive(Default)]
struct Undecided {
    pending: Option<Token>,
    requests: u64,
}

impl Classifier for Undecided {
    fn submit(&mut self, token: Token, _pcm: &[i16]) -> Submit {
        self.requests += 1;
        self.pending = Some(token);
        Submit::Accepted
    }
    fn poll(&mut self) -> Option<Outcome> {
        let token = self.pending.take()?;
        Some(Outcome {
            token,
            worker: 1,
            request: self.requests,
            result: Ok(Verdict {
                probability: 0.5,
                ..Verdict::default()
            }),
        })
    }
    fn availability(&self) -> Availability {
        Availability::Ready
    }
    fn cancel(&mut self) {
        self.pending = None;
    }
}

#[test]
fn late_result_never_closes_a_later_utterance() {
    let mut rig = Rig::new();
    rig.speak(30);
    rig.quiet(20);
    let old = rig.script.submissions()[0];
    // The first utterance ends without that verdict (timeout and fallback).
    assert_eq!(rig.quiet_until_end(200), Some(100));
    assert_eq!(rig.decision().fallback, Some("timeout"));
    rig.speak(30);
    rig.script.reply(old, 0.99);
    rig.quiet(20);
    let new = rig.script.submissions()[1];
    assert_ne!(new.utterance, old.utterance);
    rig.script.reply(old, 0.99);
    rig.quiet(10);
    rig.script.reply(new, 0.9);
    assert_eq!(rig.quiet_until_end(40), Some(10));
}

#[test]
fn cancellation_and_reset_invalidate_pending_verdicts() {
    for cancel in [TurnDetector::clear_prospective, TurnDetector::reset] {
        let mut rig = Rig::new();
        rig.speak(30);
        rig.quiet(20);
        let old = rig.script.submissions()[0];
        cancel(&mut rig.detector);
        rig.speak(30);
        rig.script.reply(old, 0.99);
        rig.quiet(20);
        rig.script.reply(old, 0.99);
        rig.quiet(5);
        let new = rig.script.submissions()[1];
        assert_ne!(new.utterance, old.utterance);
        rig.script.reply(new, 0.1);
        rig.quiet(90);
    }
}

#[test]
fn capture_discontinuity_faults_and_drops_the_pending_verdict() {
    let mut rig = Rig::new();
    rig.speak(30);
    rig.quiet(20);
    let old = rig.script.submissions()[0];
    rig.sequence += 1;
    assert!(matches!(
        rig.push(QUIET),
        Activity::Fault("capture discontinuity")
    ));
    rig.speak(30);
    rig.script.reply(old, 0.99);
    rig.quiet(19);
    assert_eq!(rig.script.submissions().len(), 1);
}

#[test]
fn unavailable_model_falls_back_to_the_baseline_window() {
    for (availability, reason) in [
        (Availability::Loading, "loading"),
        (Availability::Failed("missing".into()), "failed"),
        (Availability::Closed, "closed"),
    ] {
        let mut rig = Rig::new();
        rig.script.0.lock().unwrap().availability = Some(availability);
        rig.script.refuse(&[Submit::Unavailable]);
        rig.speak(30);
        assert_eq!(rig.quiet_until_end(300), Some(120));
        let decision = rig.decision();
        assert_eq!(decision.basis, "silence_fallback");
        assert_eq!(decision.fallback, Some(reason));
        assert_eq!(decision.probability, None);
    }
}

#[test]
fn failed_inference_falls_back_without_a_verdict() {
    let mut rig = Rig::new();
    rig.speak(30);
    rig.quiet(20);
    rig.script.fail(rig.script.submissions()[0]);
    assert_eq!(rig.quiet_until_end(300), Some(100));
    let decision = rig.decision();
    assert_eq!(
        (decision.basis, decision.fallback),
        ("silence_fallback", Some("failed"))
    );
}

#[test]
fn slow_result_times_out_and_is_ignored_afterwards() {
    let mut rig = Rig::new();
    rig.speak(30);
    rig.quiet(20);
    let token = rig.script.submissions()[0];
    rig.quiet(60);
    // 80 blocks of pause: 600 ms after submission. Too late for this pause.
    rig.script.reply(token, 0.99);
    assert_eq!(rig.quiet_until_end(300), Some(40));
    assert_eq!(rig.decision().fallback, Some("timeout"));
}

#[test]
fn busy_classifier_is_retried_with_the_same_trailing_context() {
    let mut rig = Rig::new();
    rig.script.refuse(&[Submit::Busy; 5]);
    rig.speak(30);
    rig.quiet(25);
    let tokens = rig.script.submissions();
    assert_eq!(tokens.len(), 1);
    // Submitted at 25 pause blocks, but it still ends 20 blocks into the pause.
    let pcm = rig.script.last_pcm();
    assert_eq!(*pcm.last().unwrap(), (rig.sequence - 5) as i16);
    rig.script.reply(tokens[0], 0.95);
    assert_eq!(rig.quiet_until_end(40), Some(15));

    let mut rig = Rig::new();
    rig.script.refuse(&[Submit::Busy; 500]);
    rig.speak(30);
    assert_eq!(rig.quiet_until_end(300), Some(120));
    assert_eq!(rig.decision().fallback, Some("busy"));
}

#[test]
fn confirmation_never_runs_before_the_check_window() {
    let mut rig = Rig::new();
    rig.speak(30);
    // Short pauses inside speech never reach the classifier.
    for _ in 0..20 {
        rig.quiet(19);
        for _ in 0..3 {
            assert!(matches!(rig.push(SPEECH), Activity::Continue(_)));
        }
    }
    assert!(rig.script.submissions().is_empty());
}

#[test]
fn without_confirmation_the_baseline_records_its_silence_endpoint() {
    let mut detector = TurnDetector::v1_baseline();
    let mut sequence = 0;
    let mut push = |probability| {
        sequence += 1;
        detector.push(
            ObservedAudio {
                sequence,
                captured_at_us: sequence * 10_000,
                samples: [0; 160],
            },
            probability,
        )
    };
    for _ in 0..40 {
        push(SPEECH);
    }
    let end = (1..=200).find(|_| matches!(push(QUIET), Activity::End(_)));
    assert_eq!(end, Some(120));
    let decision = detector.last_endpoint().unwrap();
    assert_eq!((decision.basis, decision.silence_ms), ("silence", 1_200));
}

#[test]
fn invalid_policies_are_rejected() {
    let base = ConfirmPolicy::default();
    for policy in [
        ConfirmPolicy {
            check_blocks: 0,
            ..base
        },
        ConfirmPolicy {
            check_blocks: 101,
            ..base
        },
        ConfirmPolicy {
            commit_blocks: 120,
            ..base
        },
        ConfirmPolicy {
            incomplete_blocks: 100,
            ..base
        },
        ConfirmPolicy {
            complete_probability: 1.0,
            ..base
        },
        ConfirmPolicy {
            complete_probability: f32::NAN,
            ..base
        },
        ConfirmPolicy {
            result_timeout_blocks: 0,
            ..base
        },
        ConfirmPolicy {
            incomplete_probability: 0.95,
            ..base
        },
        ConfirmPolicy {
            incomplete_probability: -0.1,
            ..base
        },
        ConfirmPolicy {
            incomplete_probability: f32::NAN,
            ..base
        },
    ] {
        assert!(
            TurnDetector::v1_baseline()
                .with_confirmation(Box::new(Scripted::default()), policy)
                .is_err(),
            "{policy:?}"
        );
    }
}
