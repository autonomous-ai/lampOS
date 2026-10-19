//! The giving-way step (#1800), with a fake body on a scripted Unix clock.
use super::giving_way::{Felt, GIVE_ASK, GIVE_DONE, GIVE_FAILED};
use super::*;
use std::cell::Cell;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

const S: u64 = 1_000_000;

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
    let token = offer(walk, now);
    assert_eq!(walk.take_invitation(token, now).unwrap(), GIVE_ASK);
    let boot = BootId::new([4; 16]).unwrap();
    assert!(walk.bind_invitation(token, boot, turn));
    let ended_ms = 1_760_000_000_000 + now / 1_000;
    set_clock(ended_ms);
    walk.invitation_finished(boot, turn, true, now);
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

fn settle(walk: &mut Onboarding, at: u64) -> Vec<&'static str> {
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
    assert_eq!(settle(&mut walk, now), Vec::<&str>::new());
    assert_eq!(outcome(&walk), None);
    // The minute ends: asked once more.
    let now = played.at(60_000);
    walk.tick(now, START_QUIET_US, true, false, face(), true);
    let played = play(&mut walk, now + 2 * S, 2);
    // The released message of the first push, late, is not this try's.
    walk.giving_way_felt(hand(played.ended_ms - 10, false), played.at(10));
    let now = played.at(60_000);
    assert_eq!(settle(&mut walk, now), [GIVE_FAILED]);
    assert_eq!(outcome(&walk), Some(Outcome::Failed));
    assert_eq!(walk.take_gesture(), None);
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
    assert_eq!(settle(&mut walk, played.at(7_000)), Vec::<&str>::new());
    assert_eq!(outcome(&walk), None);
    // Evidence from before the invitation ended is not of this try.
    let early = played.ended_ms - 1_000;
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
