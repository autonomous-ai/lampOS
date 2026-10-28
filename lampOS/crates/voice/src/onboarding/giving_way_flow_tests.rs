//! The giving-way step (#1800), with a fake body on a scripted Unix clock.
use super::giving_way::{
    Felt, GIVE_AGAIN, GIVE_ASK, GIVE_DONE, GIVE_FAILED, GIVE_FELT, GIVE_MORE, GIVE_PROMPT,
};
use super::*;
use std::cell::Cell;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

const S: u64 = 1_000_000;
/// How long an invitation takes to say, here.
const SAID_MS: u64 = 5_000;

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
    /// Setup already past every step before giving way.
    fn new(hook: Option<Box<dyn FnMut(storage::Tag) -> io::Result<()> + Send>>) -> Self {
        let path = std::env::temp_dir().join(format!(
            "onboarding-giving-{}",
            crate::process::boot_text(crate::process::new_boot().unwrap())
        ));
        std::fs::create_dir(&path).unwrap();
        let mut progress = Progress::default();
        for (step, outcome) in [
            (Step::WhereYouSit, Outcome::Done),
            (Step::WhoYouAre, Outcome::Done),
            (Step::HowYouSound, Outcome::Skipped),
            (Step::Touch, Outcome::NotAvailable),
            (Step::Privacy, Outcome::Skipped),
        ] {
            progress.steps.insert(
                step.name().into(),
                Kept {
                    outcome,
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
            room_sounds: path.join("room-sounds.json"),
            room_sounds_group: None,
        };
        let owner = match hook {
            Some(hook) => storage::ServiceStorage::start_with_hook(paths, hook).unwrap(),
            None => storage::ServiceStorage::start(paths).unwrap(),
        };
        Self { path, owner }
    }
    fn cycle(&self, now: u64) -> Onboarding {
        let mut walk =
            Onboarding::with_storage(self.path.join("progress.json"), self.owner.client());
        walk.unix_clock = clock;
        walk.set_giving_way_available(true);
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

/// The invitation's end on both clocks.
struct Played {
    ended_us: u64,
    ended_ms: u64,
}

impl Played {
    /// Both clocks `after_ms` past the invitation's end.
    fn at(&self, after_ms: u64) -> u64 {
        set_clock(self.ended_ms + after_ms);
        self.ended_us + after_ms * 1_000
    }
}

fn offer(walk: &mut Onboarding, now: u64) -> u64 {
    for _ in 0..2 {
        walk.drain_storage_for_test(now);
        assert_eq!(
            walk.tick(now, START_QUIET_US, true, false, face(), true),
            None
        );
    }
    let pending = walk.pending();
    let invitation = &pending.invitations[0];
    assert_eq!(invitation.step, lamp_msgs::OnboardingStep::GivingWay);
    assert!(invitation.ready, "{pending:?}");
    invitation.invitation
}

/// The invitation is offered, taken, bound and played whole.
fn play(walk: &mut Onboarding, now: u64, turn: u64) -> Played {
    // A second try says what didn't work (#3769).
    play_saying(walk, now, turn, |tries| {
        if tries > 1 { GIVE_AGAIN } else { GIVE_ASK }
    })
}

/// As `play`, the invitation's words being `asked(tries)`.
fn play_saying(
    walk: &mut Onboarding,
    now: u64,
    turn: u64,
    asked: impl Fn(u8) -> &'static str,
) -> Played {
    let token = offer(walk, now);
    let asked = asked(walk.desired.giving_way_tries);
    let ended_ms = 1_760_000_000_000 + now / 1_000;
    // Taken to be said 5 s before its playback ends.
    set_clock(ended_ms - SAID_MS);
    assert_eq!(walk.take_invitation(token, now).unwrap(), asked);
    let boot = BootId::new([4; 16]).unwrap();
    assert!(walk.bind_invitation(token, boot, turn));
    set_clock(ended_ms);
    walk.invitation_finished(boot, turn, Delivery::Whole, now);
    Played {
        ended_us: now,
        ended_ms,
    }
}

/// Counts from the middle of a turn, as `body/joint_states` says them.
fn radians(counts: f64) -> f64 {
    counts * std::f64::consts::TAU / 4_096.0
}

/// The body's joints: the base turned `yaw` counts, its goal `goal`.
fn joints(at_ms: u64, yaw: f64, goal: f64) -> Felt {
    Felt::joints(
        at_ms,
        "body",
        &lamp_msgs::JointState {
            name: ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"]
                .map(str::to_owned)
                .to_vec(),
            position: vec![radians(yaw), 0.3, -0.2, 0.0, 0.1],
            velocity: Vec::new(),
            effort: vec![0.1; 5],
            commanded: vec![radians(goal), 0.3, -0.2, 0.0, 0.1],
        },
    )
}

fn hand(at_ms: u64, pushed: bool) -> Felt {
    Felt::hand(
        at_ms,
        "body",
        &lamp_msgs::Hand {
            pushed,
            since_us: if pushed { 7_000_000 } else { 0 },
        },
    )
}

/// The body's steps from `from_ms` after the invitation: still, then a
/// hand on the base, which goes to `yaw` with its goal at `goal`, then
/// (if `release`) let go.
fn push(walk: &mut Onboarding, played: &Played, from_ms: u64, yaw: f64, goal: f64, release: bool) {
    let felt = [
        (0, joints(played.ended_ms + from_ms, 0.0, 0.0)),
        (80, hand(played.ended_ms + from_ms + 80, true)),
        (160, joints(played.ended_ms + from_ms + 160, yaw, goal)),
        (160, hand(played.ended_ms + from_ms + 160, true)),
    ];
    for (after, felt) in felt {
        let now = played.at(from_ms + after);
        walk.giving_way_felt(felt, now);
    }
    if release {
        let now = played.at(from_ms + 240);
        walk.giving_way_felt(hand(played.ended_ms + from_ms + 240, false), now);
    }
}

fn outcome(walk: &Onboarding) -> Option<Outcome> {
    walk.kept()
        .steps
        .get(Step::GivingWay.name())
        .map(|kept| kept.outcome)
}

fn settle(walk: &mut Onboarding, at: u64) -> Vec<String> {
    // Up to the first note: the walk goes on to its next step after it.
    let mut notes = Vec::new();
    for _ in 0..4 {
        notes.extend(walk.tick(at, START_QUIET_US, true, false, face(), true));
        walk.drain_storage_for_test(at);
        if !notes.is_empty() {
            break;
        }
    }
    notes
}

#[test]
fn a_hand_that_moves_the_head_and_lets_go_is_the_only_done() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    // The model cannot mark it done, by its function or its mailbox.
    let told_done = ToolCall {
        id: "c".into(),
        name: STEP.into(),
        args: json!({"step":"giving_way","outcome":"done"}),
    };
    assert!(run(&walk.file, &told_done).0.get("error").is_some());
    let played = play(&mut walk, 10 * S, 1);
    walk.apply_mailbox("{\"step\":\"giving_way\",\"outcome\":\"done\"}\n", 11 * S);
    assert!(
        walk.take_records()
            .iter()
            .any(|row| row["kind"] == "onboarding_report_refused")
    );
    // Moved 30 counts with its goal following, and let go.
    push(&mut walk, &played, 2_000, 30.0, 30.0, true);
    let now = played.at(3_000);
    assert_eq!(settle(&mut walk, now), [GIVE_DONE]);
    assert_eq!(outcome(&walk), Some(Outcome::Done));
    // The owner's #841 decision: the one move onboarding asks of the body.
    assert_eq!(walk.take_gesture(), Some("nod"));
    // It is kept across a restart.
    drop(walk);
    assert_eq!(outcome(&store.cycle(20 * S)), Some(Outcome::Done));
}

#[test]
fn a_hand_still_on_the_head_is_not_done_and_two_tries_unfelt_fail() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    let played = play(&mut walk, 10 * S, 1);
    push(&mut walk, &played, 2_000, 30.0, 30.0, false);
    let now = played.at(30_000);
    assert_eq!(settle(&mut walk, now), [GIVE_FELT]);
    assert_eq!(outcome(&walk), None);
    // Still held at the minute's end: waited for a little, then asked once
    // more.
    let now = played.at(64_000);
    walk.tick(now, START_QUIET_US, true, false, face(), true);
    assert!(walk.pending().invitations.is_empty());
    let now = played.at(65_000);
    walk.tick(now, START_QUIET_US, true, false, face(), true);
    let played = play(&mut walk, now + 2 * S, 2);
    // The released message of the first push, late, is not this try's.
    walk.giving_way_felt(hand(played.ended_ms - 10, false), played.at(10));
    let now = played.at(60_000);
    assert_eq!(settle(&mut walk, now), [GIVE_FAILED]);
    assert_eq!(outcome(&walk), Some(Outcome::Failed));
    assert_eq!(walk.take_gesture(), None);
}

/// With the onboarding file an older updater's `install` writes (before
/// #3769, no `give_again`), the second try asks as the first did, and two
/// tries unfelt still fail (the Reviewer-lead's P3 on #3781).
#[test]
fn with_the_file_from_before_3769_a_second_try_asks_as_the_first() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    let before = skill::parse(skill::tests::BEFORE_3769).unwrap();
    let first: &'static str = before.note(Step::GivingWay, false).to_owned().leak();
    walk.definition = Arc::new(before);
    let played = play_saying(&mut walk, 10 * S, 1, |_| first);
    let now = played.at(60_000);
    walk.tick(now, START_QUIET_US, true, false, face(), true);
    let played = play_saying(&mut walk, now + 2 * S, 2, |_| first);
    let now = played.at(60_000);
    assert_eq!(settle(&mut walk, now).len(), 1);
    assert_eq!(outcome(&walk), Some(Outcome::Failed));
}

#[test]
fn a_rebuilt_walk_keeps_its_invitations_and_does_not_ask_a_third_time() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    let played = play(&mut walk, 10 * S, 1);
    // A mute's end or a restart rebuilds the walk mid-try: the next
    // invitation is the second, not the first again.
    drop(walk);
    let mut walk = store.cycle(played.at(1_000));
    assert_eq!(walk.kept().giving_way_tries, 1);
    let now = played.at(2_000);
    offer(&mut walk, now);
    assert_eq!(walk.pending().invitations[0].attempt, 2);
    let played = play(&mut walk, now, 2);
    assert_eq!(walk.kept().giving_way_tries, 2);
    // Rebuilt again with both spent: failed, and never asked a third time.
    drop(walk);
    let now = played.at(1_000);
    let mut walk = store.cycle(now);
    assert_eq!(settle(&mut walk, now), [GIVE_FAILED]);
    assert_eq!(outcome(&walk), Some(Outcome::Failed));
    assert!(walk.pending().invitations.is_empty());
    assert_eq!(walk.kept().giving_way_tries, 2);
}

#[test]
fn an_offer_never_taken_gives_its_try_back() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    offer(&mut walk, 10 * S);
    assert_eq!(walk.kept().giving_way_tries, 1);
    // Nobody there for 30 s before it was taken.
    for _ in 0..2 {
        walk.tick(45 * S, START_QUIET_US, true, false, None, true);
        walk.drain_storage_for_test(45 * S);
    }
    assert!(walk.pending().invitations.is_empty());
    assert_eq!(walk.kept().giving_way_tries, 0);
}

#[test]
fn a_kept_count_over_the_budget_is_refused_whole() {
    let spent = Progress {
        giving_way_tries: 2,
        ..Progress::default()
    };
    assert!(spent.check().is_ok());
    let spoiled = Progress {
        giving_way_tries: 3,
        ..Progress::default()
    };
    assert!(spoiled.check().is_err());
    // A hand-edited file fails storage's load: nothing runs on it, so it
    // grants no invitation.
    let path = std::env::temp_dir().join(format!(
        "onboarding-giving-load-{}",
        crate::process::boot_text(crate::process::new_boot().unwrap())
    ));
    std::fs::create_dir(&path).unwrap();
    std::fs::write(
        path.join("progress.json"),
        br#"{"steps":{},"giving_way_tries":200}"#,
    )
    .unwrap();
    let mut owner = storage::ServiceStorage::start(storage::Paths {
        progress: path.join("progress.json"),
        checkpoint: path.join("privacy-pending.json"),
        mailbox: path.join("progress.json.told"),
        near_call: path.join("near-call.json"),
        room_sounds: path.join("room-sounds.json"),
        room_sounds_group: None,
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
fn a_head_not_moved_far_or_not_held_where_the_hand_put_it_is_not_done() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    let played = play(&mut walk, 10 * S, 1);
    // Ten counts: under the body's HAND_MOVED, a bump.
    push(&mut walk, &played, 1_000, 10.0, 10.0, true);
    // Thirty counts but its goal left where it was: the body pushed back.
    push(&mut walk, &played, 3_000, 30.0, 0.0, true);
    // A body restarting mid-push spoils that push.
    walk.giving_way_felt(joints(played.ended_ms + 5_000, 0.0, 0.0), played.at(5_000));
    walk.giving_way_felt(hand(played.ended_ms + 5_080, true), played.at(5_080));
    walk.giving_way_felt(Felt::Restart, played.at(5_100));
    walk.giving_way_felt(
        joints(played.ended_ms + 5_160, 30.0, 30.0),
        played.at(5_160),
    );
    walk.giving_way_felt(hand(played.ended_ms + 5_240, false), played.at(5_240));
    // Another publisher's word is not the body's.
    let forged = Felt::hand(
        played.ended_ms + 6_000,
        "someone",
        &lamp_msgs::Hand {
            pushed: false,
            since_us: 0,
        },
    );
    assert_eq!(forged, Felt::Invalid);
    assert_eq!(settle(&mut walk, played.at(7_000)), [GIVE_MORE]);
    assert_eq!(outcome(&walk), None);
    // Evidence from before the invitation was taken to be said is not of
    // this try.
    let early = played.ended_ms - SAID_MS - 1_000;
    for felt in [
        joints(early, 0.0, 0.0),
        hand(early + 80, true),
        joints(early + 160, 30.0, 30.0),
    ] {
        walk.giving_way_felt(felt, played.at(7_000));
    }
    walk.giving_way_felt(hand(played.ended_ms + 7_100, false), played.at(7_100));
    assert_eq!(outcome(&walk), None);
    // The same push the window allows is done.
    push(&mut walk, &played, 8_000, -30.0, -28.0, true);
    assert_eq!(settle(&mut walk, played.at(9_000)), [GIVE_DONE]);
}

#[test]
fn a_clock_that_jumped_does_not_make_a_push_done() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    let played = play(&mut walk, 10 * S, 1);
    for (after, felt) in [
        (2_000, joints(played.ended_ms + 2_000, 0.0, 0.0)),
        (2_080, hand(played.ended_ms + 2_080, true)),
        (2_160, joints(played.ended_ms + 2_160, 30.0, 30.0)),
    ] {
        walk.giving_way_felt(felt, played.at(after));
    }
    let now = played.at(2_240);
    set_clock(played.ended_ms + 2_240 + 5_000);
    walk.giving_way_felt(hand(played.ended_ms + 2_240, false), now);
    assert_eq!(outcome(&walk), None);
    assert!(
        walk.take_records()
            .iter()
            .any(|row| row["why"] == "the clock jumped")
    );
}

#[test]
fn without_a_body_or_under_the_pause_it_is_not_available() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    walk.set_giving_way_available(false);
    settle(&mut walk, 10 * S);
    assert_eq!(outcome(&walk), Some(Outcome::NotAvailable));
    assert!(walk.pending().invitations.is_empty());
    drop(walk);
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    let played = play(&mut walk, 10 * S, 1);
    let now = played.at(1_000);
    assert_eq!(
        walk.tick(now, START_QUIET_US, true, true, face(), true),
        None
    );
    walk.drain_storage_for_test(now);
    assert_eq!(outcome(&walk), Some(Outcome::NotAvailable));
    // The push that follows is felt by nobody.
    push(&mut walk, &played, 2_000, 30.0, 30.0, true);
    settle(&mut walk, played.at(3_000));
    assert_eq!(outcome(&walk), Some(Outcome::NotAvailable));
    assert_eq!(walk.take_gesture(), None);
}

#[test]
fn late_evidence_after_a_skip_or_a_redo_is_ignored() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    let played = play(&mut walk, 10 * S, 1);
    walk.apply_mailbox(
        "{\"step\":\"giving_way\",\"outcome\":\"skipped\"}\n",
        played.at(500),
    );
    push(&mut walk, &played, 2_000, 30.0, 30.0, true);
    settle(&mut walk, played.at(3_000));
    assert_eq!(outcome(&walk), Some(Outcome::Skipped));
    assert_eq!(walk.take_gesture(), None);
    drop(walk);
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    let played = play(&mut walk, 10 * S, 1);
    walk.apply_mailbox("{\"redo\":true}\n", played.at(500));
    push(&mut walk, &played, 2_000, 30.0, 30.0, true);
    settle(&mut walk, played.at(3_000));
    assert_eq!(outcome(&walk), None);
    assert!(walk.kept().steps.is_empty());
    assert_eq!(walk.take_gesture(), None);
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
fn a_held_progress_write_never_holds_the_loop_and_nothing_is_done_before_it() {
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
    let played = play(&mut walk, 10 * S, 1);
    walk.read_us = Some(played.at(0));
    push(&mut walk, &played, 1_000, 30.0, 30.0, true);
    // The outcome's commit is the next submission: hold it on the worker.
    armed.store(true, Ordering::SeqCst);
    let now = played.at(1_300);
    walk.tick(now, START_QUIET_US, true, false, face(), true);
    seen.recv_timeout(Duration::from_secs(2)).unwrap();
    let started = Instant::now();
    for offset in 0..1_000 {
        let at = now + offset * 1_000;
        walk.giving_way_felt(joints(played.ended_ms + 1_400, 0.0, 0.0), at);
        assert_eq!(
            walk.tick(at, START_QUIET_US, true, false, face(), true),
            None
        );
    }
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(outcome(&walk), None);
    assert_eq!(walk.take_gesture(), None);
    drop(release);
    assert_eq!(settle(&mut walk, played.at(3_000)), [GIVE_DONE]);
    assert_eq!(outcome(&walk), Some(Outcome::Done));
}

/// lamp-52e6 on 2026-10-27 (#3504): a hand turned the base about 32 counts
/// a hold from 1,903, under its load bound, a hold every 2.6 s. As the body
/// says it with #3676 (Fifth's `a_base_turned_by_hand_in_small_steps_under_its_bound_says_a_hand`),
/// a step at a time (80 ms): each turn is pushed back for a step with its
/// goal where it was, then given way where the hand has it, its goal there;
/// from the second hold, once the base is more than `HAND_MOVED` from where
/// it first gave, `body/hand` says pushed, a step after that hold gave way,
/// and it says released once the hand has gone after the last. `pushed`
/// false is that day's body, which never said it.
fn turn_base_as_on_52e6(walk: &mut Onboarding, played: &Played, holds: &[f64], pushed: bool) {
    const MIDDLE: f64 = 2_048.0;
    let mut at = 1_000;
    let mut felt = |walk: &mut Onboarding, yaw: f64, goal: f64, said: bool| {
        walk.giving_way_felt(
            joints(played.ended_ms + at, yaw - MIDDLE, goal - MIDDLE),
            played.at(at),
        );
        walk.giving_way_felt(hand(played.ended_ms + at, said), played.at(at));
        at += 80;
    };
    let mut from = 1_903.0;
    let mut said = false;
    for (hold, &to) in holds.iter().enumerate() {
        // The hand turns it; the body still asks for where it was.
        felt(walk, to, from, said);
        // It gives way where the hand has it.
        felt(walk, to, to, said);
        // Held there by the hand, under its bound, for the rest of the hold.
        said = pushed && hold >= 1;
        for _ in 0..30 {
            felt(walk, to, to, said);
        }
        from = to;
    }
    // The hand has gone.
    felt(walk, from, from, false);
}

#[test]
fn the_base_turned_by_a_hand_as_on_52e6_is_done_once_the_body_says_the_hand() {
    // Two holds, as in the body's test, and the day's five, to 1,744.
    for holds in [
        &[1_871.0, 1_839.0][..],
        &[1_871.0, 1_839.0, 1_807.0, 1_775.0, 1_744.0],
    ] {
        let store = Store::new(None);
        let mut walk = store.cycle(S);
        let played = play(&mut walk, 10 * S, 1);
        turn_base_as_on_52e6(&mut walk, &played, holds, true);
        assert_eq!(
            settle(&mut walk, played.at(20_000)),
            [GIVE_DONE],
            "{holds:?}"
        );
        assert_eq!(outcome(&walk), Some(Outcome::Done));
    }
}

#[test]
fn the_same_turn_with_the_hand_never_said_is_not_done() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    let played = play(&mut walk, 10 * S, 1);
    let holds = [1_871.0, 1_839.0, 1_807.0, 1_775.0, 1_744.0];
    turn_base_as_on_52e6(&mut walk, &played, &holds, false);
    assert_eq!(settle(&mut walk, played.at(20_000)), [GIVE_PROMPT]);
    assert_eq!(outcome(&walk), None);
    walk.tick(played.at(60_000), START_QUIET_US, true, false, face(), true);
    assert!(
        walk.take_records()
            .iter()
            .any(|row| row["why"] == "no hand moved the head")
    );
}

/// The invitation is offered, taken and bound, and its turn ends with no
/// audio at all (`no_audio_answer`): the voice never said it (#3754).
fn unsaid(walk: &mut Onboarding, now: u64, turn: u64) {
    finished(walk, now, turn, Delivery::Unsaid);
}

/// The invitation is offered, taken and bound, and its turn ends so.
fn finished(walk: &mut Onboarding, now: u64, turn: u64, delivery: Delivery) {
    let token = offer(walk, now);
    assert_eq!(walk.take_invitation(token, now).unwrap(), GIVE_ASK);
    let boot = BootId::new([4; 16]).unwrap();
    assert!(walk.bind_invitation(token, boot, turn));
    walk.invitation_finished(boot, turn, delivery, now);
}

/// lamp-52e6, 28 Oct 12:47:56 and 12:48:09 (#4035): the invitation was
/// said through a yield to the lamp's own voice, or cut, with no push begun,
/// and the step failed with no push asked for. Neither spends a try: it is
/// said again, under the unsaid bound, and done on the person's push.
#[test]
fn an_invitation_cut_with_no_push_is_said_again_and_spends_no_try() {
    for delivery in [Delivery::Gapped, Delivery::Cut] {
        let store = Store::new(None);
        let mut walk = store.cycle(S);
        finished(&mut walk, 10 * S, 1, delivery);
        walk.drain_storage_for_test(10 * S);
        assert_eq!(outcome(&walk), None, "{delivery:?}");
        assert_eq!(walk.kept().giving_way_tries, 0, "{delivery:?}");
        let row = walk
            .take_records()
            .into_iter()
            .rfind(|row| row["kind"] == "onboarding_giving_way_try")
            .unwrap();
        assert_eq!(row["offered_again"], true, "{delivery:?}: {row}");
        finished(&mut walk, 12 * S, 2, delivery);
        walk.drain_storage_for_test(12 * S);
        assert_eq!(walk.kept().giving_way_tries, 0, "{delivery:?}");
        // The third, under the same bound as one never said: the step is
        // left for later, never failed.
        let played = play(&mut walk, 14 * S, 3);
        assert_eq!(walk.kept().giving_way_tries, 1, "{delivery:?}");
        push(&mut walk, &played, 2_000, 30.0, 30.0, true);
        assert_eq!(settle(&mut walk, played.at(3_000)), [GIVE_DONE]);
        assert_eq!(outcome(&walk), Some(Outcome::Done), "{delivery:?}");
    }
    // Past the bound, not available, never failed.
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    for turn in 1..=3 {
        finished(&mut walk, (8 + 2 * turn) * S, turn, Delivery::Gapped);
    }
    walk.drain_storage_for_test(16 * S);
    for _ in 0..2 {
        walk.tick(16 * S, START_QUIET_US, true, false, face(), true);
        walk.drain_storage_for_test(16 * S);
    }
    assert_eq!(outcome(&walk), Some(Outcome::NotAvailable));
    assert_eq!(walk.kept().giving_way_tries, 0);
}

/// Left `not_available` past the unsaid bound and offered again later in
/// the same run, the step starts its count again: its first gap is said
/// again, not straight back to `not_available` (Third's P3 on #4045).
#[test]
fn a_step_left_for_later_by_gaps_counts_its_unsaid_anew_when_offered_again() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    left_for_later_and_offered_again(&mut walk);
    let row = walk
        .take_records()
        .into_iter()
        .rfind(|row| row["kind"] == "onboarding_giving_way_try")
        .unwrap();
    assert_eq!(row["offered_again"], true, "{row}");
}

/// A re-offer that ends `not_available` again keeps its date: the step is
/// not offered again before `reoffer_after_s`, so a lamp gapping its own
/// speech does not repeat the invitation without end (the Reviewer-lead's
/// P2 on #4045).
#[test]
fn a_re_offer_left_for_later_again_waits_the_skills_interval() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    left_for_later_and_offered_again(&mut walk);
    let dated = walk.desired.steps[Step::GivingWay.name()].offered_at_unix_s;
    for turn in 11..=10 + u64::from(MOST_UNSAID) {
        finished(&mut walk, (12 + 2 * turn) * S, turn, Delivery::Gapped);
    }
    walk.drain_storage_for_test(40 * S);
    assert_eq!(outcome(&walk), Some(Outcome::NotAvailable));
    assert_eq!(
        walk.desired.steps[Step::GivingWay.name()].offered_at_unix_s,
        dated
    );
    for _ in 0..3 {
        walk.tick(42 * S, START_QUIET_US, true, false, face(), true);
        walk.drain_storage_for_test(42 * S);
    }
    assert!(walk.pending().invitations.is_empty());
    assert_eq!(walk.next_step(), None);
    // Past the interval it is offered again.
    let at = unix_ms() / 1_000 - walk.definition.reoffer_after_s;
    walk.desired
        .steps
        .get_mut(Step::GivingWay.name())
        .unwrap()
        .offered_at_unix_s = Some(at);
    assert_eq!(walk.next_step(), Some(Step::GivingWay));
}

/// Left `not_available` by [`MOST_UNSAID`] + 1 gaps, the walk's other steps
/// ended, and offered again: its first gap is said again and dated.
fn left_for_later_and_offered_again(walk: &mut Onboarding) {
    for turn in 1..=u64::from(MOST_UNSAID) + 1 {
        finished(walk, (8 + 2 * turn) * S, turn, Delivery::Gapped);
    }
    walk.drain_storage_for_test(16 * S);
    assert_eq!(outcome(walk), Some(Outcome::NotAvailable));
    // The walk's other steps end, so it is offered again.
    for step in [Step::RoomSounds, Step::FaceExpressions, Step::Preferences] {
        walk.desired.steps.insert(
            step.name().into(),
            Kept {
                outcome: Outcome::Skipped,
                at_unix_s: 1,
                framing: None,
                offered_at_unix_s: None,
            },
        );
    }
    walk.save();
    walk.drain_storage_for_test(20 * S);
    walk.take_records();
    // Offered again: its first gap is said again, and the offer is dated so
    // the skill's interval holds before the next.
    finished(walk, 30 * S, 10, Delivery::Gapped);
    assert!(
        walk.desired.steps[Step::GivingWay.name()]
            .offered_at_unix_s
            .is_some()
    );
}

#[test]
fn an_invitation_the_voice_never_said_is_no_try_and_is_asked_again() {
    // lamp-29e7, 27 Oct 14:18:52: `say: dropped (no_audio_answer)` and
    // `giving_way: failed` in the same second, the person never asked.
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    unsaid(&mut walk, 10 * S, 1);
    walk.drain_storage_for_test(10 * S);
    assert_eq!(
        outcome(&walk),
        None,
        "a dropped ask is not the person's failure"
    );
    assert_eq!(walk.kept().giving_way_tries, 0, "its try is given back");
    assert!(
        walk.take_records()
            .iter()
            .any(|row| row["why"] == "invitation not said"),
        "the record says why"
    );
    // Asked again once the voice can speak, and done on the person's push.
    let played = play(&mut walk, 12 * S, 2);
    assert_eq!(walk.kept().giving_way_tries, 1);
    push(&mut walk, &played, 2_000, 30.0, 30.0, true);
    let now = played.at(3_000);
    assert_eq!(settle(&mut walk, now), [GIVE_DONE]);
    assert_eq!(outcome(&walk), Some(Outcome::Done));
}

#[test]
fn an_ask_never_said_three_times_is_not_available_never_failed() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    // Offered again twice at most after a dropped ask.
    unsaid(&mut walk, 10 * S, 1);
    unsaid(&mut walk, 12 * S, 2);
    assert_eq!(outcome(&walk), None);
    unsaid(&mut walk, 14 * S, 3);
    walk.drain_storage_for_test(14 * S);
    for _ in 0..2 {
        walk.tick(16 * S, START_QUIET_US, true, false, face(), true);
        walk.drain_storage_for_test(16 * S);
    }
    assert_eq!(outcome(&walk), Some(Outcome::NotAvailable));
    assert_eq!(walk.kept().giving_way_tries, 0);
    assert!(walk.pending().invitations.is_empty());
}

/// Each of the step's lines, as the skill ships them (#3702): the ask names
/// no direction, as any head joint counts (`base_yaw`, `head_pitch`,
/// `head_roll`), and asks for the push now, as the lamp listens from the
/// ask's start.
#[test]
fn the_steps_lines_are_the_skills() {
    let said = |line: &str| line.split('"').nth(1).unwrap().to_owned();
    for (line, words) in [
        (
            GIVE_ASK,
            "Hold my base with one hand. With the other, tilt my head gently to one side. Let go when I say so.",
        ),
        (
            GIVE_AGAIN,
            "I didn't feel my head move. Hold my base with one hand. With the other, tilt my head gently to one side. Let go when I say so.",
        ),
        (GIVE_FELT, "That's it, now let go."),
        (GIVE_MORE, "A little more, then let go."),
        (
            GIVE_PROMPT,
            "Hold my base with one hand. With the other, tilt my head gently to one side, then let go.",
        ),
        (GIVE_DONE, "Got it, my head moved with your hand."),
        (
            GIVE_FAILED,
            "I didn't feel my head move this time. That's fine, you can redo the setup later. Just ask me to redo the setup.",
        ),
    ] {
        assert_eq!(said(line), words);
    }
}

/// lamp-29e7, 28 Oct (#3702): the person pushed while the lamp was still
/// saying the invitation, the body said the hand, and the try opened 15 s
/// later with nobody pushing again. A push from the ask's start is the
/// try's: answered once the ask is said, and done on the same judgement.
#[test]
fn a_push_while_the_invitation_is_said_is_the_trys() {
    for release_before_the_end in [true, false] {
        let store = Store::new(None);
        let mut walk = store.cycle(S);
        let token = offer(&mut walk, 10 * S);
        let taken_ms = 1_760_000_000_000;
        set_clock(taken_ms);
        assert_eq!(walk.take_invitation(token, 10 * S).unwrap(), GIVE_ASK);
        let boot = BootId::new([4; 16]).unwrap();
        assert!(walk.bind_invitation(token, boot, 1));
        // Both clocks `after` ms past the ask's start.
        let at = |after: u64| {
            set_clock(taken_ms + after);
            10 * S + after * 1_000
        };
        let felt = [
            (1_000, joints(taken_ms + 1_000, 0.0, 0.0)),
            (1_080, hand(taken_ms + 1_080, true)),
            (1_160, joints(taken_ms + 1_160, 33.0, 0.0)),
            (1_240, joints(taken_ms + 1_240, 33.0, 33.0)),
            (1_240, hand(taken_ms + 1_240, true)),
        ];
        for (after, felt) in felt {
            walk.giving_way_felt(felt, at(after));
        }
        // Nothing is said over the ask.
        assert_eq!(
            walk.tick(at(1_300), START_QUIET_US, false, false, face(), true),
            None
        );
        if release_before_the_end {
            walk.giving_way_felt(hand(taken_ms + 1_320, false), at(1_320));
        }
        walk.invitation_finished(boot, 1, Delivery::Whole, at(15_000));
        if release_before_the_end {
            assert_eq!(settle(&mut walk, at(15_100)), [GIVE_DONE]);
            assert_eq!(outcome(&walk), Some(Outcome::Done));
            continue;
        }
        // Still in the hand: told at once to let go, then done on it.
        assert_eq!(settle(&mut walk, at(15_100)), [GIVE_FELT]);
        walk.giving_way_felt(hand(taken_ms + 16_000, false), at(16_000));
        assert_eq!(settle(&mut walk, at(16_100)), [GIVE_DONE]);
        assert_eq!(outcome(&walk), Some(Outcome::Done));
    }
}

/// Never a silent minute (#3702): with nothing felt, the lamp prompts once
/// a few seconds into the minute, and says nothing more before the minute's
/// end asks again.
#[test]
fn nothing_felt_is_prompted_once_early_never_a_silent_minute() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    let played = play(&mut walk, 10 * S, 1);
    let mut said = Vec::new();
    for tenth in 1..600 {
        for line in settle(&mut walk, played.at(tenth * 100)) {
            said.push((tenth * 100, line));
        }
    }
    assert_eq!(said, [(8_000, GIVE_PROMPT.to_owned())]);
    let now = played.at(60_000);
    assert_eq!(settle(&mut walk, now), Vec::<String>::new());
    offer(&mut walk, now + S);
}

/// A push answered at once, then the hand gone without moving the head
/// far enough: asked for a little more, and nothing else through the minute.
#[test]
fn a_push_that_did_not_move_the_head_is_answered_then_prompted_once() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    let played = play(&mut walk, 10 * S, 1);
    push(&mut walk, &played, 2_000, 10.0, 10.0, false);
    assert_eq!(settle(&mut walk, played.at(2_200)), [GIVE_FELT]);
    // Released 4 counts from where it was: a bump.
    walk.giving_way_felt(joints(played.ended_ms + 3_000, 4.0, 4.0), played.at(3_000));
    walk.giving_way_felt(hand(played.ended_ms + 3_000, false), played.at(3_000));
    let mut said = Vec::new();
    for tenth in 31..600 {
        said.extend(settle(&mut walk, played.at(tenth * 100)));
    }
    assert_eq!(said, [GIVE_MORE, GIVE_PROMPT]);
}

/// The body's joints with only `moved` off the middle: at `position`
/// counts, its goal at `goal`.
fn one_joint(at_ms: u64, moved: &str, position: f64, goal: f64) -> Felt {
    let names = ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"];
    let at = |value: f64| {
        names
            .map(|name| if name == moved { radians(value) } else { 0.0 })
            .to_vec()
    };
    Felt::joints(
        at_ms,
        "body",
        &lamp_msgs::JointState {
            name: names.map(str::to_owned).to_vec(),
            position: at(position),
            velocity: Vec::new(),
            effort: vec![0.1; 5],
            commanded: at(goal),
        },
    )
}

/// A person moves the head whichever way they like (#3702): the base
/// turned (lamp-52e6), the head tilted (lamp-29e7's pass) or rolled is
/// done; the arm moved alone is not the head, and the lamp says plainly
/// how to do it.
#[test]
fn any_head_joint_moved_by_hand_is_done_and_the_arm_alone_is_not() {
    for (joint, done) in [
        ("base_yaw", true),
        ("head_pitch", true),
        ("head_roll", true),
        ("shoulder", false),
    ] {
        let store = Store::new(None);
        let mut walk = store.cycle(S);
        let played = play(&mut walk, 10 * S, 1);
        let at = |after: u64| played.ended_ms + after;
        for (after, felt) in [
            (2_000, one_joint(at(2_000), joint, 0.0, 0.0)),
            (2_080, hand(at(2_080), true)),
            (2_160, one_joint(at(2_160), joint, -33.0, -28.0)),
            (2_240, hand(at(2_240), false)),
        ] {
            walk.giving_way_felt(felt, played.at(after));
        }
        let said = settle(&mut walk, played.at(2_300));
        if done {
            assert_eq!(said, [GIVE_DONE], "{joint}");
            assert_eq!(outcome(&walk), Some(Outcome::Done), "{joint}");
        } else {
            assert_eq!(said, [GIVE_MORE], "{joint}");
            assert_eq!(outcome(&walk), None, "{joint}");
        }
    }
}

/// A push late in the minute, a second before its end, still counts.
#[test]
fn a_late_push_near_the_trys_end_is_done() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    let played = play(&mut walk, 10 * S, 1);
    assert_eq!(settle(&mut walk, played.at(8_000)), [GIVE_PROMPT]);
    push(&mut walk, &played, 58_700, 30.0, 30.0, true);
    assert_eq!(settle(&mut walk, played.at(59_000)), [GIVE_DONE]);
    assert_eq!(outcome(&walk), Some(Outcome::Done));
}

/// Nobody pushes at all: each try is prompted once a few seconds in, the
/// second invitation says what didn't work, and the end is the plain fail
/// line, never a silent minute.
#[test]
fn no_push_at_all_is_prompted_each_try_then_failed_plainly() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    let mut played = play(&mut walk, 10 * S, 1);
    let mut said = Vec::new();
    for tries in 1..=2 {
        for tenth in 1..=600 {
            said.extend(settle(&mut walk, played.at(tenth * 100)));
        }
        if tries == 1 {
            let now = played.at(60_000) + S;
            played = play(&mut walk, now, 2);
        }
    }
    assert_eq!(said, [GIVE_PROMPT, GIVE_PROMPT, GIVE_FAILED]);
    assert_eq!(outcome(&walk), Some(Outcome::Failed));
}

/// The person pushes and talks over the invitation ("like this?"), which
/// cuts it: the push is kept and the try goes on to done (the
/// Reviewer-lead's P2 on #4002).
#[test]
fn a_push_while_talking_over_the_invitation_is_kept() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    let token = offer(&mut walk, 10 * S);
    let taken_ms = 1_760_000_000_000;
    set_clock(taken_ms);
    assert_eq!(walk.take_invitation(token, 10 * S).unwrap(), GIVE_ASK);
    let boot = BootId::new([4; 16]).unwrap();
    assert!(walk.bind_invitation(token, boot, 1));
    let at = |after: u64| {
        set_clock(taken_ms + after);
        10 * S + after * 1_000
    };
    walk.giving_way_felt(joints(taken_ms + 1_000, 0.0, 0.0), at(1_000));
    walk.giving_way_felt(hand(taken_ms + 1_080, true), at(1_080));
    walk.giving_way_felt(joints(taken_ms + 1_160, 30.0, 30.0), at(1_160));
    walk.invitation_finished(boot, 1, Delivery::Cut, at(2_000));
    assert_eq!(settle(&mut walk, at(2_100)), [GIVE_FELT]);
    walk.giving_way_felt(hand(taken_ms + 3_000, false), at(3_000));
    assert_eq!(settle(&mut walk, at(3_100)), [GIVE_DONE]);
    assert_eq!(outcome(&walk), Some(Outcome::Done));
}

/// A push begun at 59 s and let go at 61 s is done (the Reviewer-lead's P3
/// on #4002).
#[test]
fn a_push_across_the_minutes_end_is_waited_for() {
    let store = Store::new(None);
    let mut walk = store.cycle(S);
    let played = play(&mut walk, 10 * S, 1);
    push(&mut walk, &played, 59_000, 30.0, 30.0, false);
    assert_eq!(settle(&mut walk, played.at(60_500)), [GIVE_FELT]);
    walk.giving_way_felt(hand(played.ended_ms + 61_000, false), played.at(61_000));
    assert_eq!(settle(&mut walk, played.at(61_100)), [GIVE_DONE]);
    assert_eq!(outcome(&walk), Some(Outcome::Done));
}
