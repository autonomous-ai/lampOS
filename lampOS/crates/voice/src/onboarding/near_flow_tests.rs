//! The near-call step (#1803), with a scripted bus and Unix clock.
use super::near::{
    NEAR_DONE, NEAR_FAILED, SENTENCE_AGAIN, SENTENCE_CLOSER, SENTENCE_DONE, SENTENCE_FAILED,
    SENTENCE_FIRST, SENTENCE_LOUDER, Seen,
};
use super::*;
use lamp_msgs::{NameHeard, RoomSound};
use std::cell::Cell;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

const S: u64 = 1_000_000;
/// The room's quiet, in dBFS.
const FLOOR: f32 = -60.0;

thread_local! {
    static NOW_MS: Cell<u64> = const { Cell::new(0) };
}
fn clock() -> u64 {
    NOW_MS.with(Cell::get)
}
fn set_clock(ms: u64) {
    NOW_MS.with(|now| now.set(ms));
}

fn face() -> Option<Framing> {
    Some(Framing {
        size: 0.2,
        x: 0.0,
        y: 0.0,
    })
}

struct Store {
    path: PathBuf,
    owner: storage::ServiceStorage,
}
impl Store {
    /// Setup already past where they sit and who they are.
    fn new(hook: Option<Box<dyn FnMut(storage::Tag) -> io::Result<()> + Send>>) -> Self {
        let path = std::env::temp_dir().join(format!(
            "onboarding-near-{}",
            crate::process::boot_text(crate::process::new_boot().unwrap())
        ));
        std::fs::create_dir(&path).unwrap();
        let mut progress = Progress::default();
        for step in [Step::WhereYouSit, Step::WhoYouAre] {
            progress.steps.insert(
                step.name().into(),
                Kept {
                    outcome: Outcome::Done,
                    at_unix_s: 1,
                    framing: None,
                    offered_at_unix_s: None,
                },
            );
        }
        std::fs::write(
            path.join("progress.json"),
            serde_json::to_vec(&progress).unwrap(),
        )
        .unwrap();
        let paths = storage::Paths {
            progress: path.join("progress.json"),
            checkpoint: path.join("privacy-pending.json"),
            mailbox: path.join("progress.json.told"),
            near_call: path.join("near-call.json"),
        };
        let owner = match hook {
            Some(hook) => storage::ServiceStorage::start_with_hook(paths, hook).unwrap(),
            None => storage::ServiceStorage::start(paths).unwrap(),
        };
        Self { path, owner }
    }
    fn near_file(&self) -> PathBuf {
        self.path.join("near-call.json")
    }
    fn cycle(&self, now: u64) -> Onboarding {
        let mut walk =
            Onboarding::with_storage(self.path.join("progress.json"), self.owner.client());
        walk.unix_clock = clock;
        walk.set_near_available(true);
        walk.drain_storage_for_test(now);
        walk
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        if self.owner.finish(Duration::from_secs(2)).ok()
            == Some(storage::StorageShutdown::Finished)
        {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// One scripted try: the prompt's end on both clocks.
struct Prompt {
    token: u64,
    ended_us: u64,
    ended_ms: u64,
}

/// The step offers a prompt; its budget commits; it becomes ready.
fn offer(walk: &mut Onboarding, now: u64) -> u64 {
    walk.drain_storage_for_test(now);
    assert_eq!(
        walk.tick(now, START_QUIET_US, true, false, face(), true),
        None
    );
    walk.drain_storage_for_test(now);
    assert_eq!(
        walk.tick(now, START_QUIET_US, true, false, face(), true),
        None
    );
    let pending = walk.pending();
    let invitation = &pending.invitations[0];
    assert_eq!(invitation.step, lamp_msgs::OnboardingStep::HowYouSound);
    assert!(invitation.ready, "{pending:?}");
    invitation.invitation
}

/// The prompt is played whole; its end is read from the Unix clock then.
fn play(walk: &mut Onboarding, now: u64, unix_ms: u64, turn: u64) -> Prompt {
    let token = offer(walk, now);
    let words = walk.take_invitation(token, now).unwrap();
    assert!(words.contains("call your name"), "{words}");
    let boot = BootId::new([3; 16]).unwrap();
    assert!(walk.bind_invitation(token, boot, turn));
    // The lamp spoke the prompt.
    walk.near_seen(Seen::Speaking {
        at_ms: unix_ms - 2_000,
        speaking: true,
    });
    set_clock(unix_ms);
    walk.invitation_finished(boot, turn, true, now);
    walk.near_seen(Seen::Speaking {
        at_ms: unix_ms + 20,
        speaking: false,
    });
    Prompt {
        token,
        ended_us: now,
        ended_ms: unix_ms,
    }
}

/// Mic1's level every 100 ms from the prompt's end through 7 s, 2 dB over
/// the quiet, rising `rise_db` at `peak_ms` after the prompt.
fn room(walk: &mut Onboarding, prompt: &Prompt, peak_ms: u64, rise_db: f32) {
    for after in (0..=7_000).step_by(100) {
        let level_db = FLOOR + if after == peak_ms { rise_db } else { 2.0 };
        walk.near_seen(Seen::level(
            prompt.ended_ms + after,
            &RoomSound {
                level_db,
                floor_db: Some(FLOOR),
                own_voice: false,
            },
        ));
    }
}

/// The name, begun `after_ms` after the prompt's end, said final 600 ms on.
fn name(walk: &mut Onboarding, prompt: &Prompt, after_ms: u64, score: f32) {
    walk.near_seen(Seen::hearing(
        prompt.ended_ms + after_ms + 600,
        &NameHeard { score, ago_ms: 600 },
    ));
}

/// The try is judged once 7 s and the delivery grace have passed.
fn judge(walk: &mut Onboarding, prompt: &Prompt) -> Value {
    let now = prompt.ended_us + 7 * S + 300_000;
    set_clock(prompt.ended_ms + 7_300);
    walk.tick(now, START_QUIET_US, true, false, face(), true);
    walk.drain_storage_for_test(now);
    walk.take_records()
        .into_iter()
        .rfind(|row| row["kind"] == "onboarding_near_try")
        .expect("a try was judged")
}

/// One whole try: prompt, call with this rise at 2 s, judged.
fn call(walk: &mut Onboarding, at: u64, rise_db: f32, turn: u64) -> Value {
    let prompt = play(walk, at, 1_760_000_000_000 + at / 1_000, turn);
    room(walk, &prompt, 2_400, rise_db);
    name(walk, &prompt, 2_000, 0.98);
    judge(walk, &prompt)
}

fn outcome(walk: &Onboarding) -> Option<Outcome> {
    walk.kept()
        .steps
        .get(Step::HowYouSound.name())
        .map(|kept| kept.outcome)
}

/// The name part's outcome, with the step not yet decided: its sentence
/// part follows.
fn named(walk: &Onboarding) -> Option<Outcome> {
    assert_eq!(outcome(walk), None, "the step waits for its sentence part");
    walk.near.named()
}

/// Lets the step finish once no prompt is asked again: the notes said.
fn settle(walk: &mut Onboarding, at: u64) -> Vec<&'static str> {
    let mut notes = Vec::new();
    for _ in 0..4 {
        notes.extend(walk.tick(at, START_QUIET_US, true, false, face(), true));
        walk.drain_storage_for_test(at);
    }
    notes
}

/// Nobody in front of the lamp, over two loops (a mailbox read may hold
/// the first).
fn nobody(walk: &mut Onboarding, at: u64) {
    for _ in 0..2 {
        walk.tick(at, START_QUIET_US, true, false, None, true);
        walk.drain_storage_for_test(at);
    }
}

#[test]
fn three_calls_from_the_seat_keep_a_bar_and_only_they_mark_the_step_done() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    // The model cannot mark it done.
    let call_done = ToolCall {
        id: "c".into(),
        name: STEP.into(),
        args: json!({"step":"how_you_sound","outcome":"done"}),
    };
    assert!(run(&walk.file, &call_done).0.get("error").is_some());
    assert_eq!(call(&mut walk, 10 * S, 22.4, 1)["accepted"], true);
    assert_eq!(call(&mut walk, 30 * S, 25.0, 2)["accepted"], true);
    assert_eq!(walk.kept().near_call_tries.len(), 2);
    assert_eq!(walk.kept().near_call_prompts, 2);
    // A new voice cycle goes on from the kept prompts and tries.
    drop(walk);
    let mut walk = store.cycle(40 * S);
    assert_eq!(walk.kept().near_call_tries.len(), 2);
    assert_eq!(call(&mut walk, 50 * S, 23.1, 3)["accepted"], true);
    assert_eq!(walk.pending().invitations.len(), 0);
    assert_eq!(outcome(&walk), None, "nothing done before the bar is kept");
    assert_eq!(settle(&mut walk, 60 * S), [NEAR_DONE]);
    assert_eq!(named(&walk), Some(Outcome::Done));
    assert_eq!(lamp_near_call::read(&store.near_file()), Some(19.4));
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(store.near_file())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    // Only numbers are kept.
    let kept: Value = serde_json::from_slice(&std::fs::read(store.near_file()).unwrap()).unwrap();
    assert_eq!(kept["tries"].as_array().unwrap().len(), 3);
    assert_eq!(walk.kept().near_call_prompts, 3);
}

#[test]
fn a_hearing_begun_at_six_seconds_counts_and_one_a_millisecond_later_does_not() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    let prompt = play(&mut walk, 10 * S, 1_760_000_010_000, 1);
    // Its rise is read through 7 s.
    room(&mut walk, &prompt, 6_900, 21.0);
    name(&mut walk, &prompt, 6_000, 0.97);
    let judged = judge(&mut walk, &prompt);
    assert_eq!(judged["accepted"], true, "{judged}");
    assert_eq!(judged["rise_db"].as_f64().unwrap() as f32, 21.0);
    let prompt = play(&mut walk, 30 * S, 1_760_000_030_000, 2);
    room(&mut walk, &prompt, 6_900, 21.0);
    name(&mut walk, &prompt, 6_001, 0.97);
    let judged = judge(&mut walk, &prompt);
    assert_eq!(judged["accepted"], false);
    assert_eq!(judged["why"], "the name wasn't heard");
    // Evidence past the collection window is not even buffered.
    let prompt = play(&mut walk, 50 * S, 1_760_000_050_000, 3);
    name(&mut walk, &prompt, 7_500, 0.99);
    assert!(
        walk.near
            .seen()
            .iter()
            .all(|seen| !matches!(seen, Seen::Hearing(_)))
    );
}

#[test]
fn the_lamp_speaking_spoils_only_the_half_second_before_to_a_second_after() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    // Speaking until 0.4 s before the name began.
    let prompt = play(&mut walk, 10 * S, 1_760_000_010_000, 1);
    walk.near_seen(Seen::Speaking {
        at_ms: prompt.ended_ms + 1_000,
        speaking: true,
    });
    walk.near_seen(Seen::Speaking {
        at_ms: prompt.ended_ms + 1_600,
        speaking: false,
    });
    room(&mut walk, &prompt, 2_400, 21.0);
    name(&mut walk, &prompt, 2_000, 0.98);
    assert_eq!(judge(&mut walk, &prompt)["why"], "the lamp was speaking");
    // Answering 1.1 s after the name began, outside the span: it counts.
    let prompt = play(&mut walk, 30 * S, 1_760_000_030_000, 2);
    room(&mut walk, &prompt, 2_400, 21.0);
    name(&mut walk, &prompt, 2_000, 0.98);
    walk.near_seen(Seen::Speaking {
        at_ms: prompt.ended_ms + 3_100,
        speaking: true,
    });
    walk.near_seen(Seen::Speaking {
        at_ms: prompt.ended_ms + 4_000,
        speaking: false,
    });
    assert_eq!(judge(&mut walk, &prompt)["accepted"], true);
    // The prompt's own tail: a name heard 0.3 s after it ended.
    let prompt = play(&mut walk, 50 * S, 1_760_000_050_000, 3);
    room(&mut walk, &prompt, 400, 21.0);
    name(&mut walk, &prompt, 300, 0.98);
    assert_eq!(judge(&mut walk, &prompt)["why"], "the lamp was speaking");
}

/// The voice holds its reply to what is said from the prompt's end to 7 s
/// on: with the lamp silent after the name, the call in it is accepted.
/// Before the prompt has played whole, past the window and once the try is
/// judged, nothing is held.
#[test]
fn the_reply_is_held_only_in_the_try_window_and_the_held_call_counts() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    assert!(!walk.near_holds_reply(10 * S));
    let prompt = play(&mut walk, 10 * S, 1_760_000_010_000, 1);
    assert!(!walk.near_holds_reply(prompt.ended_us - 1));
    for after in [0, 2 * S, 7 * S] {
        assert!(walk.near_holds_reply(prompt.ended_us + after), "{after}");
    }
    assert!(!walk.near_holds_reply(prompt.ended_us + 7 * S + 1));
    room(&mut walk, &prompt, 2_400, 21.0);
    name(&mut walk, &prompt, 2_000, 0.98);
    assert_eq!(judge(&mut walk, &prompt)["accepted"], true);
    assert!(!walk.near_holds_reply(prompt.ended_us + 3 * S));
}

/// Without a Controls receipt source privacy is `not_available` (Third on
/// #1802), and this step is offered next rather than waiting on it.
#[test]
fn the_step_follows_privacy_that_is_not_available() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    walk.set_privacy_context(
        privacy::Epoch {
            device_boot: "device-boot".into(),
            clock_id: "clock".into(),
        },
        false,
        None,
        Some("on its base"),
    );
    for _ in 0..3 {
        walk.drain_storage_for_test(S);
        walk.tick(S, START_QUIET_US, true, false, face(), true);
    }
    assert_eq!(walk.kept().steps["privacy"].outcome, Outcome::NotAvailable);
    assert_eq!(walk.led.as_ref().unwrap().step, Step::HowYouSound);
    let pending = walk.pending();
    assert_eq!(
        pending.invitations[0].step,
        lamp_msgs::OnboardingStep::HowYouSound,
        "{pending:?}"
    );
}

#[test]
fn a_clock_jump_or_a_source_restart_spoils_the_try() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    let prompt = play(&mut walk, 10 * S, 1_760_000_010_000, 1);
    room(&mut walk, &prompt, 2_400, 21.0);
    name(&mut walk, &prompt, 2_000, 0.98);
    // The Unix clock stepped 5 s while the monotonic one ran 7.3 s.
    let now = prompt.ended_us + 7 * S + 300_000;
    set_clock(prompt.ended_ms + 12_300);
    walk.tick(now, START_QUIET_US, true, false, face(), true);
    let rows = walk.take_records();
    let judged = rows
        .iter()
        .rfind(|row| row["kind"] == "onboarding_near_try")
        .unwrap();
    assert_eq!(judged["why"], "the clock jumped");
    walk.drain_storage_for_test(now);
    let prompt = play(&mut walk, 30 * S, 1_760_000_030_000, 2);
    room(&mut walk, &prompt, 2_400, 21.0);
    walk.near_seen(Seen::Restart);
    name(&mut walk, &prompt, 2_000, 0.98);
    assert_eq!(judge(&mut walk, &prompt)["why"], "a source began again");
    assert!(walk.kept().near_call_tries.is_empty());
}

#[test]
fn invalid_scores_and_levels_are_refused_before_they_reach_a_try() {
    for score in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.01, -0.1] {
        assert_eq!(
            Seen::hearing(10_000, &NameHeard { score, ago_ms: 100 }),
            Seen::Invalid,
            "{score}"
        );
    }
    // An age before the epoch or far longer than a name takes.
    assert_eq!(
        Seen::hearing(
            100,
            &NameHeard {
                score: 0.99,
                ago_ms: 200
            }
        ),
        Seen::Invalid
    );
    assert_eq!(
        Seen::hearing(
            100_000,
            &NameHeard {
                score: 0.99,
                ago_ms: 60_000
            }
        ),
        Seen::Invalid
    );
    for (level_db, floor_db) in [
        (f32::NAN, Some(FLOOR)),
        (f32::INFINITY, Some(FLOOR)),
        (5.0, Some(FLOOR)),
        (-30.0, Some(f32::NAN)),
    ] {
        assert_eq!(
            Seen::level(
                1,
                &RoomSound {
                    level_db,
                    floor_db,
                    own_voice: false
                }
            ),
            Seen::Invalid
        );
    }
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    // A valid score under the bar is heard, and the try says so.
    let prompt = play(&mut walk, 10 * S, 1_760_000_010_000, 1);
    room(&mut walk, &prompt, 2_400, 21.0);
    name(&mut walk, &prompt, 2_000, 0.94);
    let judged = judge(&mut walk, &prompt);
    assert_eq!(judged["accepted"], false);
    assert!(judged["why"].as_str().unwrap().contains("under 0.95"));
    // An invalid message during the try spoils it, whatever else came.
    let prompt = play(&mut walk, 30 * S, 1_760_000_030_000, 2);
    room(&mut walk, &prompt, 2_400, 21.0);
    name(&mut walk, &prompt, 2_000, 0.99);
    walk.near_seen(Seen::hearing(
        prompt.ended_ms + 2_000,
        &NameHeard {
            score: f32::NAN,
            ago_ms: 10,
        },
    ));
    assert_eq!(judge(&mut walk, &prompt)["why"], "invalid evidence");
    assert!(walk.kept().near_call_tries.is_empty());
}

#[test]
fn persisted_samples_are_validated_on_load() {
    let good = || Progress {
        near_call_prompts: 3,
        near_call_tries: vec![
            lamp_near_call::Try {
                rise_db: 22.4,
                score: 0.98,
            };
            3
        ],
        ..Progress::default()
    };
    assert!(good().check().is_ok());
    let bad: [fn(&mut Progress); 7] = [
        |p| p.near_call_tries[0].rise_db = f32::NAN,
        |p| p.near_call_tries[0].rise_db = -1.0,
        |p| p.near_call_tries[0].score = 1.2,
        |p| p.near_call_tries[0].score = 0.9,
        |p| p.near_call_prompts = 6,
        |p| p.near_call_prompts = 2,
        |p| {
            p.near_call_tries.pop();
            p.steps.insert(
                Step::HowYouSound.name().into(),
                Kept {
                    outcome: Outcome::Done,
                    at_unix_s: 1,
                    framing: None,
                    offered_at_unix_s: None,
                },
            );
        },
    ];
    for (index, spoil) in bad.iter().enumerate() {
        let mut progress = good();
        spoil(&mut progress);
        assert!(progress.check().is_err(), "case {index}");
    }
    // A hand-edited file fails storage's load: nothing runs on it.
    let path = std::env::temp_dir().join(format!(
        "onboarding-near-load-{}",
        crate::process::boot_text(crate::process::new_boot().unwrap())
    ));
    std::fs::create_dir(&path).unwrap();
    std::fs::write(
        path.join("progress.json"),
        br#"{"steps":{},"near_call_prompts":3,"near_call_tries":[{"rise_db":22.0,"score":7.0}]}"#,
    )
    .unwrap();
    let mut owner = storage::ServiceStorage::start(storage::Paths {
        progress: path.join("progress.json"),
        checkpoint: path.join("privacy-pending.json"),
        mailbox: path.join("progress.json.told"),
        near_call: path.join("near-call.json"),
    })
    .unwrap();
    let client = owner.client();
    let until = Instant::now() + Duration::from_secs(5);
    let state = loop {
        let state = client.snapshot().unwrap();
        if !state.busy {
            break state;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(state.failed.is_some() && state.progress.is_none());
    owner.finish(Duration::from_secs(2)).unwrap();
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn too_few_calls_or_too_far_apart_keep_no_file_and_the_default() {
    // Nobody answers: three empty prompts can no longer reach three, and an
    // earlier bar is removed.
    let store = Store::new(None);
    std::fs::write(store.near_file(), b"{}").unwrap();
    let mut walk = store.cycle(S);
    for (turn, at) in [(1, 10 * S), (2, 30 * S), (3, 50 * S)] {
        let prompt = play(&mut walk, at, 1_760_000_000_000 + at / 1_000, turn);
        room(&mut walk, &prompt, 2_400, 21.0);
        assert_eq!(judge(&mut walk, &prompt)["why"], "the name wasn't heard");
    }
    assert_eq!(settle(&mut walk, 70 * S), [NEAR_FAILED]);
    assert_eq!(named(&walk), Some(Outcome::Failed));
    assert!(!store.near_file().exists());
    assert_eq!(walk.kept().near_call_prompts, 3);
    assert_eq!(lamp_near_call::read(&store.near_file()), None);
    // Three calls more than 10 dB apart: no bar either.
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    for (turn, rise) in [(1, 18.0), (2, 28.1), (3, 20.0)] {
        assert_eq!(call(&mut walk, turn * 20 * S, rise, turn)["accepted"], true);
    }
    settle(&mut walk, 80 * S);
    assert_eq!(named(&walk), Some(Outcome::Failed));
    assert!(!store.near_file().exists());
}

#[test]
fn five_prompts_at_most() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    for (turn, heard) in [(1, false), (2, true), (3, false), (4, true), (5, false)] {
        let at = turn * 20 * S;
        let prompt = play(&mut walk, at, 1_760_000_000_000 + at / 1_000, turn);
        room(&mut walk, &prompt, 2_400, 21.0);
        if heard {
            name(&mut walk, &prompt, 2_000, 0.98);
        }
        assert_eq!(judge(&mut walk, &prompt)["accepted"], heard);
    }
    settle(&mut walk, 120 * S);
    assert_eq!(walk.kept().near_call_prompts, 5);
    assert_eq!(walk.kept().near_call_tries.len(), 2);
    assert_eq!(named(&walk), Some(Outcome::Failed));
    assert!(
        walk.pending()
            .invitations
            .iter()
            .all(|invitation| invitation.attempt > 5)
    );
    assert!(!store.near_file().exists());
}

#[test]
fn absence_drops_the_prompt_or_try_and_asks_again_when_someone_is_back() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    // Offered, never taken: nobody there for 30 s gives the prompt back.
    offer(&mut walk, 10 * S);
    assert_eq!(walk.desired.near_call_prompts, 1);
    nobody(&mut walk, 41 * S);
    assert!(walk.pending().invitations.is_empty());
    assert_eq!(walk.kept().near_call_prompts, 0);
    // Played, and nobody called: the try is judged without a hearing, and
    // evidence arriving after everyone left is not taken.
    let prompt = play(&mut walk, 50 * S, 1_760_000_050_000, 1);
    set_clock(prompt.ended_ms + 31_000);
    walk.tick(81 * S, START_QUIET_US, true, false, None, true);
    walk.drain_storage_for_test(81 * S);
    assert!(
        walk.take_records().iter().any(
            |row| row["kind"] == "onboarding_near_try" && row["why"] == "the name wasn't heard"
        )
    );
    nobody(&mut walk, 82 * S);
    room(&mut walk, &prompt, 2_400, 21.0);
    name(&mut walk, &prompt, 2_000, 0.98);
    assert!(walk.near.seen().is_empty());
    walk.drain_storage_for_test(90 * S);
    assert!(walk.kept().near_call_tries.is_empty());
    assert_eq!(walk.kept().near_call_prompts, 1);
    assert_eq!(outcome(&walk), None);
    // Back: asked again, the next prompt.
    let token = offer(&mut walk, 100 * S);
    assert_eq!(walk.pending().invitations[0].attempt, 2);
    assert_ne!(token, prompt.token);
}

#[test]
fn skip_and_redo_drop_late_evidence_and_keep_any_earlier_bar_on_skip() {
    let store = Store::new(None);
    std::fs::write(store.near_file(), b"earlier").unwrap();
    let mut walk = store.cycle(S);
    let prompt = play(&mut walk, 10 * S, 1_760_000_010_000, 1);
    report_skip(&walk);
    walk.drain_storage_for_test(11 * S);
    walk.tick(11 * S, START_QUIET_US, true, false, face(), true);
    walk.drain_storage_for_test(12 * S);
    walk.read_us = None;
    walk.drain_storage_for_test(12 * S);
    assert_eq!(outcome(&walk), Some(Outcome::Skipped));
    // Late evidence and a late judgement change nothing.
    room(&mut walk, &prompt, 2_400, 21.0);
    name(&mut walk, &prompt, 2_000, 0.98);
    set_clock(prompt.ended_ms + 7_300);
    walk.tick(
        prompt.ended_us + 8 * S,
        START_QUIET_US,
        true,
        false,
        face(),
        true,
    );
    assert!(walk.kept().near_call_tries.is_empty());
    assert_eq!(std::fs::read(store.near_file()).unwrap(), b"earlier");
    // Redo forgets the walk, its prompts and its generation.
    let generation = walk.near.generation;
    walk.apply_mailbox("{\"redo\":true}\n", 20 * S);
    walk.drain_storage_for_test(20 * S);
    assert!(walk.kept().steps.is_empty());
    assert_eq!(walk.kept().near_call_prompts, 0);
    assert_eq!(walk.near.generation, 0);
    assert_ne!(generation, 0);
}

fn report_skip(walk: &Onboarding) {
    let call = ToolCall {
        id: "s".into(),
        name: STEP.into(),
        args: json!({"step":"how_you_sound","outcome":"skipped"}),
    };
    assert_eq!(run(&walk.file, &call), (json!({"ok": true}), true));
}

struct Release(Option<mpsc::SyncSender<()>>);
impl Drop for Release {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.try_send(());
        }
    }
}

#[test]
fn a_held_near_call_write_never_holds_the_loop_and_nothing_is_done_before_it() {
    let armed = Arc::new(AtomicBool::new(false));
    let gate = armed.clone();
    let (entered, seen) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    let store = Store::new(Some(Box::new(move |_| {
        if gate.swap(false, Ordering::SeqCst) {
            let _ = entered.try_send(());
            wait.recv().map_err(io::Error::other)?;
        }
        Ok(())
    })));
    let release = Release(Some(release));
    let mut walk = store.cycle(S);
    for (turn, rise) in [(1, 22.4), (2, 25.0), (3, 23.1)] {
        assert_eq!(call(&mut walk, turn * 20 * S, rise, turn)["accepted"], true);
    }
    // The next submission is the bar's write: hold it on the worker.
    armed.store(true, Ordering::SeqCst);
    walk.read_us = Some(90 * S);
    walk.tick(90 * S, START_QUIET_US, true, false, face(), true);
    assert!(walk.near.writing());
    seen.recv_timeout(Duration::from_secs(2)).unwrap();
    let started = Instant::now();
    for offset in 0..1_000 {
        let now = 90 * S + offset * 1_000;
        walk.near_seen(Seen::Speaking {
            at_ms: 1,
            speaking: false,
        });
        assert_eq!(
            walk.tick(now, START_QUIET_US, true, false, face(), true),
            None
        );
        assert!(walk.pending().invitations.is_empty());
    }
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(outcome(&walk), None);
    assert_eq!(walk.owed, None);
    assert!(!store.near_file().exists());
    drop(release);
    assert_eq!(settle(&mut walk, 100 * S), [NEAR_DONE]);
    assert_eq!(named(&walk), Some(Outcome::Done));
    assert_eq!(lamp_near_call::read(&store.near_file()), Some(19.4));
}

// The sentence part (#1803, the first builder's ruling (a)).

/// Three accepted calls: the bar is kept and the sentence part begins.
fn past_the_name(walk: &mut Onboarding) {
    for (turn, rise) in [(1, 22.4), (2, 25.0), (3, 23.1)] {
        assert_eq!(call(walk, turn * 20 * S, rise, turn)["accepted"], true);
    }
    assert_eq!(settle(walk, 70 * S), [NEAR_DONE]);
    assert_eq!(named(walk), Some(Outcome::Done));
}

/// A sentence prompt asked and played whole at `at`: its words.
fn ask(walk: &mut Onboarding, at: u64, turn: u64) -> String {
    let token = offer(walk, at);
    assert!(walk.pending().invitations[0].attempt > 5);
    let words = walk.take_invitation(token, at).unwrap();
    let boot = BootId::new([3; 16]).unwrap();
    assert!(walk.bind_invitation(token, boot, turn));
    walk.invitation_finished(boot, turn, true, at);
    words
}

/// A line ended 4 s after the prompt, its words back, and the try judged.
fn sentence(
    walk: &mut Onboarding,
    at: u64,
    speech_ms: u64,
    close: bool,
    words: Option<usize>,
) -> Value {
    walk.sentence_line(speech_ms, close, at + 4 * S);
    if let Some(words) = words {
        walk.sentence_words(words);
    }
    for now in [at + 5 * S, at + 18 * S + 1] {
        walk.tick(now, START_QUIET_US, true, false, face(), true);
        walk.drain_storage_for_test(now);
    }
    walk.take_records()
        .into_iter()
        .rfind(|row| row["kind"] == "onboarding_sentence_try")
        .expect("a sentence try was judged")
}

#[test]
fn a_sentence_said_close_with_its_words_covering_it_ends_the_step_done_and_keeps_none_of_it() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    past_the_name(&mut walk);
    assert_eq!(ask(&mut walk, 80 * S, 4), SENTENCE_FIRST);
    // Its window holds no name try: the near-call gate stays shut.
    assert!(walk.sentence_window(80 * S) && walk.sentence_window(95 * S));
    assert!(!walk.sentence_window(95 * S + 1) && !walk.near_holds_reply(81 * S));
    let judged = sentence(&mut walk, 80 * S, 3_000, true, Some(9));
    assert_eq!(judged["accepted"], true, "{judged}");
    assert_eq!(judged["words"], 9);
    assert_eq!(settle(&mut walk, 100 * S), [SENTENCE_DONE]);
    assert_eq!(outcome(&walk), Some(Outcome::Done));
    assert_eq!(walk.kept().sentence_prompts, 1);
    assert_eq!(lamp_near_call::read(&store.near_file()), Some(19.4));
    // Only counts are kept: the file has no field for the sentence.
    let kept: Value =
        serde_json::from_slice(&std::fs::read(store.path.join("progress.json")).unwrap()).unwrap();
    let mut fields: Vec<&String> = kept.as_object().unwrap().keys().collect();
    fields.sort();
    assert_eq!(
        fields,
        [
            "near_call_prompts",
            "near_call_tries",
            "privacy_discard_pending",
            "privacy_invitation_tries",
            "sentence_prompts",
            "steps"
        ]
    );
}

#[test]
fn too_quiet_is_coached_a_little_louder_once_and_a_fragment_a_little_closer() {
    // Not said close: "louder", then a good sentence is done.
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    past_the_name(&mut walk);
    ask(&mut walk, 80 * S, 4);
    let judged = sentence(&mut walk, 80 * S, 3_000, false, Some(9));
    assert_eq!(
        (judged["accepted"].clone(), judged["short"].clone()),
        (json!(false), json!("level"))
    );
    assert_eq!(outcome(&walk), None);
    assert_eq!(ask(&mut walk, 110 * S, 5), SENTENCE_LOUDER);
    assert_eq!(
        sentence(&mut walk, 110 * S, 2_500, true, Some(6))["accepted"],
        true
    );
    assert_eq!(settle(&mut walk, 130 * S), [SENTENCE_DONE]);
    assert_eq!(outcome(&walk), Some(Outcome::Done));
    // A fragment: one word for 4 s of speech, "closer", then failed with
    // the one coaching spent; the name part's bar stands.
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    past_the_name(&mut walk);
    ask(&mut walk, 80 * S, 4);
    assert_eq!(
        sentence(&mut walk, 80 * S, 4_000, true, Some(1))["short"],
        "coverage"
    );
    assert_eq!(ask(&mut walk, 110 * S, 5), SENTENCE_CLOSER);
    // Words that never came back fall short the same way.
    assert_eq!(
        sentence(&mut walk, 110 * S, 4_000, true, None)["short"],
        "coverage"
    );
    assert_eq!(settle(&mut walk, 140 * S), [SENTENCE_FAILED]);
    assert_eq!(outcome(&walk), Some(Outcome::Failed));
    assert!(walk.pending().invitations.is_empty());
    assert_eq!(walk.kept().sentence_prompts, 2);
    assert_eq!(lamp_near_call::read(&store.near_file()), Some(19.4));
}

#[test]
fn the_step_is_the_sentences_whatever_the_name_part_came_to() {
    // Nobody called: no bar, and the sentence part still runs and decides.
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    for (turn, at) in [(1, 10 * S), (2, 30 * S), (3, 50 * S)] {
        let prompt = play(&mut walk, at, 1_760_000_000_000 + at / 1_000, turn);
        judge(&mut walk, &prompt);
    }
    assert_eq!(settle(&mut walk, 70 * S), [NEAR_FAILED]);
    assert_eq!(named(&walk), Some(Outcome::Failed));
    ask(&mut walk, 80 * S, 4);
    // Nothing said in the window: "louder".
    let judged = sentence_none(&mut walk, 80 * S);
    assert_eq!(judged["short"], "level");
    ask(&mut walk, 110 * S, 5);
    assert_eq!(
        sentence(&mut walk, 110 * S, 2_000, true, Some(5))["accepted"],
        true
    );
    settle(&mut walk, 130 * S);
    assert_eq!(outcome(&walk), Some(Outcome::Done));
    assert!(!store.near_file().exists());
    // A kept "done" needs its sentence asked.
    let mut progress = walk.kept().clone();
    assert!(progress.check().is_ok());
    progress.sentence_prompts = 0;
    assert!(progress.check().is_err());
    progress.sentence_prompts = 3;
    assert!(progress.check().is_err());
}

/// The window and the words' wait pass with no line.
fn sentence_none(walk: &mut Onboarding, at: u64) -> Value {
    walk.tick(at + 18 * S - 1, START_QUIET_US, true, false, face(), true);
    assert!(
        !walk
            .take_records()
            .iter()
            .any(|row| row["kind"] == "onboarding_sentence_try"),
        "judged before the window and the wait passed"
    );
    walk.tick(at + 18 * S, START_QUIET_US, true, false, face(), true);
    walk.drain_storage_for_test(at + 18 * S);
    walk.take_records()
        .into_iter()
        .rfind(|row| row["kind"] == "onboarding_sentence_try")
        .expect("judged")
}

#[test]
fn late_lines_skip_redo_and_a_new_cycle_in_the_sentence_part() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    past_the_name(&mut walk);
    ask(&mut walk, 80 * S, 4);
    // A line ending past the window is not the sentence.
    walk.sentence_line(3_000, true, 96 * S);
    walk.sentence_words(9);
    assert_eq!(sentence_none(&mut walk, 80 * S)["short"], "level");
    // A new voice cycle keeps the prompt spent and the bar is kept again;
    // what fell short is not known, so the next prompt only asks again.
    drop(walk);
    let mut walk = store.cycle(100 * S);
    assert_eq!(settle(&mut walk, 100 * S), [NEAR_DONE]);
    let token = offer(&mut walk, 110 * S);
    assert_eq!(
        walk.take_invitation(token, 110 * S).unwrap(),
        SENTENCE_AGAIN
    );
    // Claimed and never judged: with both prompts spent the step fails.
    drop(walk);
    let mut walk = store.cycle(120 * S);
    assert_eq!(settle(&mut walk, 120 * S), [NEAR_DONE, SENTENCE_FAILED]);
    assert_eq!(outcome(&walk), Some(Outcome::Failed));
    // Redo forgets the sentence's prompts too.
    walk.apply_mailbox("{\"redo\":true}\n", 130 * S);
    walk.drain_storage_for_test(130 * S);
    assert_eq!(walk.kept().sentence_prompts, 0);
    // A skip in the sentence part ends the step skipped; its late line and
    // words change nothing.
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    past_the_name(&mut walk);
    ask(&mut walk, 80 * S, 4);
    report_skip(&walk);
    walk.read_us = None;
    walk.drain_storage_for_test(81 * S);
    walk.tick(81 * S, START_QUIET_US, true, false, face(), true);
    walk.drain_storage_for_test(81 * S);
    assert_eq!(outcome(&walk), Some(Outcome::Skipped));
    assert!(!walk.sentence_window(82 * S));
    walk.sentence_line(3_000, true, 82 * S);
    walk.sentence_words(9);
    walk.tick(99 * S, START_QUIET_US, true, false, face(), true);
    assert!(
        !walk
            .take_records()
            .iter()
            .any(|row| row["kind"] == "onboarding_sentence_try")
    );
    assert_eq!(outcome(&walk), Some(Outcome::Skipped));
    assert_eq!(lamp_near_call::read(&store.near_file()), Some(19.4));
}
