//! #4332: the workers' leases through the conversation's startup prologue.
//!
//! The real `conversation()` runs on the stepped scenario clock, its setup
//! made slow by the test hook: each stretch between two of the prologue's
//! lease keeps takes 40 ms of logical time, ~0.9 s in all, as a cold lamp's
//! first start after boot did; or 60 ms, ~1.3 s in all and ~1.1 s of it
//! after the prologue's third keep, where the privacy start window's second
//! used to begin: it counts from the loop's start.
//! Each fixture peer is read at every step and notes when each authority
//! reached it. The run is ended by the test once the loop has gone on a
//! while.
//!
//! Synthetic child/socket supervision only: no device, no real privacy
//! worker (its fail-closed exit is not run here, only the leases it reads),
//! and the logical times are not lamp latencies.
use super::*;
use std::{cell::RefCell, rc::Rc};

/// 2 ms steps a prologue stretch takes: 40 ms.
const STRETCH_STEPS: u32 = 20;
/// And in the longer prologue: 60 ms, each still well inside a lease.
const LONG_STRETCH_STEPS: u32 = 30;
/// Steps the whole run, the prologue's included, is let go before the
/// test ends it: 2.4 s.
const RUN_STEPS: u32 = 1_200;
const TEST_OVER: &str = "the prologue lease test is over";

/// One fixture worker's side: each authority it was sent, when it read it
/// and until when that authority holds.
struct LeasePeer {
    role: &'static str,
    channels: WorkerChannels,
    seen: Vec<(u64, u64)>,
}

impl LeasePeer {
    fn read(&mut self) {
        while let Some(message) = self.channels.control.receive::<Control>().unwrap() {
            if let Control::Authority { snapshot } = message {
                self.seen
                    .push((monotonic_us(), snapshot.expires_at().as_micros()));
                // The switch read open, fresh, at each authority, as the
                // privacy worker reads it.
                if self.role == "privacy" {
                    self.channels
                        .control
                        .send(WorkerEvent::Privacy {
                            receipt: None,
                            acquired_at_us: monotonic_us(),
                            muted: false,
                        })
                        .unwrap();
                }
            }
        }
    }
}

/// Restores the hook however the test ends.
struct SlowPrologue;

impl SlowPrologue {
    fn on(stretch_steps: u32) -> Self {
        TEST_PROLOGUE_STEPS.with(|steps| steps.set(stretch_steps));
        TEST_PROLOGUE_STEPPED.with(|stepped| stepped.set(0));
        Self
    }

    /// The logical time the prologue's setup took, in ms.
    fn took_ms() -> u32 {
        TEST_PROLOGUE_STEPPED.with(std::cell::Cell::get) * 2
    }
}

impl Drop for SlowPrologue {
    fn drop(&mut self) {
        TEST_PROLOGUE_STEPS.with(|steps| steps.set(0));
    }
}

#[test]
fn a_slow_startup_prologue_keeps_every_workers_lease_renewed() {
    // The setup did take about the cold lamp's 0.9 s: well past one lease.
    slow_prologue_run(STRETCH_STEPS, 800);
}

/// The privacy worker's observations queue up through the whole prologue,
/// each older than its 50 ms freshness when the loop first reads them. A
/// prologue past the start window's second must not end the voice there:
/// the window counts from the loop's start.
#[test]
fn a_startup_prologue_past_a_second_is_not_ended_by_its_queued_privacy_observations() {
    slow_prologue_run(LONG_STRETCH_STEPS, 1_200);
}

/// Runs the conversation with each prologue stretch taking `stretch_steps`
/// 2 ms steps, and checks the whole prologue took at least `least_ms`, that
/// the test, not anything else, ended the run, and that no worker's lease
/// lapsed.
fn slow_prologue_run(stretch_steps: u32, least_ms: u32) {
    let directory = SessionDirectory::create().unwrap();
    let mut workers = Vec::new();
    let mut peers = Vec::new();
    for role in ["speaker", "capture", "provider", "privacy"] {
        let (worker, channels) = Worker::sleeping_fixture(&directory.path, role);
        workers.push(worker);
        peers.push(LeasePeer {
            role,
            channels,
            seen: Vec::new(),
        });
    }
    let privacy = workers.pop().unwrap();
    let provider = workers.pop().unwrap();
    let capture = workers.pop().unwrap();
    let speaker = workers.pop().unwrap();
    let clock = crate::scenario_clock::Scope::enter(Duration::from_secs(30)).unwrap();
    let peers = Rc::new(RefCell::new(peers));
    let pumped = Rc::clone(&peers);
    let mut steps = 0u32;
    clock
        .set_pump(move |_| {
            for peer in pumped.borrow_mut().iter_mut() {
                peer.read();
            }
            steps += 1;
            if steps > RUN_STEPS {
                return Err(io::Error::other(TEST_OVER));
            }
            Ok(())
        })
        .unwrap();
    let mut rig = Rig {
        expression_out: None,
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        early_finalize: false,
        open_name: false,
        call_takes_next: false,
        unfinished_wait: false,
        faint_level: None,
        speaker,
        capture,
        provider,
        privacy,
        privacy_controls: false,
        capture_held: false,
        privacy_receipts: false,
        ring: None.into(),
        camera: None,
        motor: None,
        ring_channel_ceiling: None,
        diagnostic_directory: None,
        noise_suppression: NoiseSuppression::On,
        turn_taking: TurnTaking::VadOnly,
        playout_start: PlayoutStart::default(),
        fixture_at_ms: None,
        endpoint: None,
        call_wait_blocks: None,
        cut_keeps_name: false,
        asker_first: crate::asker_first::Mode::Off,
        follows_on: false,
        paused: Box::new(|| false),
    };
    let mut owner = Controller::new(new_boot().unwrap(), now());
    // Each worker's start was read, as `spawn_rig` reads its Ready: what the
    // privacy worker says while the setup runs is then a backlog the loop
    // drains as held up, not stale.
    for worker in [
        &mut rig.speaker,
        &mut rig.capture,
        &mut rig.provider,
        &mut rig.privacy,
    ] {
        assert!(
            worker
                .channels
                .control
                .receive::<WorkerEvent>()
                .unwrap()
                .is_none()
        );
    }
    // `spawn_rig`'s last pulse, the authority every worker holds as the
    // conversation begins.
    let began = monotonic_us();
    rig.publish(owner.snapshot(now()).unwrap()).unwrap();
    let mut trace = SessionState::default();
    let mut reply = None;
    let slow = SlowPrologue::on(stretch_steps);
    let result = conversation(
        &mut rig,
        &mut owner,
        Lifetime::Serve,
        &mut trace,
        StartupEvents::default(),
        &mut reply,
        &mut None,
        &Termination::inactive(),
        &FailureNotices::none(),
    );
    let prologue_ms = SlowPrologue::took_ms();
    drop(slow);
    let ended = monotonic_us();
    // Ended by the test, not by a lapsed lease or anything else.
    let error = result.unwrap_err().to_string();
    assert!(error.contains(TEST_OVER), "{error}");
    assert!(
        prologue_ms >= least_ms,
        "the prologue took only {prologue_ms} ms"
    );
    for peer in peers.borrow().iter() {
        let seen = &peer.seen;
        assert!(
            seen.first().is_some_and(|(at, _)| *at <= began + 2_000),
            "{}: never got the starting authority",
            peer.role
        );
        // Each authority came before the one before it had expired, and the
        // last still holds at the end: the lease never lapsed.
        for pair in seen.windows(2) {
            let ((_, held_until), (at, _)) = (pair[0], pair[1]);
            assert!(
                at < held_until,
                "{}: its lease lapsed at {} ms, renewed only at {} ms",
                peer.role,
                (held_until - began) / 1_000,
                (at - began) / 1_000
            );
        }
        let (_, held_until) = *seen.last().unwrap();
        assert!(
            held_until > ended,
            "{}: its lease lapsed at {} ms, before the run ended at {} ms",
            peer.role,
            (held_until - began) / 1_000,
            (ended - began) / 1_000
        );
    }
    drop(rig);
    drop(clock);
    let _ = fs::remove_dir(&directory.path);
}
