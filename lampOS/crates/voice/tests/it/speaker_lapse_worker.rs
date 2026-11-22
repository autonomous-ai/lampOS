//! The speaker worker's own loop on ALSA's `null` device, with its
//! authority let lapse (#1178). The controller's side of its channels and
//! the capture worker's side of its reference link are played here, in real
//! time, through the same sockets and messages the lamp uses.
//!
//! At midnight on lamp-4ace the voice ended on "authority snapshot has
//! expired": a worker held up past its lease, with nothing fresher queued,
//! found its installed authority lapsed. It must cost a gap, never the voice.
//!
//! `null` plays every frame the moment it is written, so the speaker's clock
//! never starts there: its prime needs 480 frames queued
//! (`reference InvalidPrime … accepted=480`). What `null` can show is the
//! lapse before a prime, the worker going on after it, and the capture
//! side's late answer to a prime the lapse reset. The lapse with speech or
//! silence queued, and the prime again after it, need a device that holds
//! frames: a lamp. What the coordinator does with the speech a lapse drops
//! is shown in `coordinator::tests::speaker_lapse`.
#![cfg(target_os = "linux")]
use lamp_ipc::monotonic_us;
use lamp_turn_owner::{
    AdmissionState, AdmittedInput, BootId, CaptureState, Controller, MonoTime, Permission,
};
use lamp_voice::{
    audio_workers::{speaker, speaker_on},
    process::{SessionDirectory, new_boot},
    reference::PRIME_FRAMES,
    transport::{ReferenceChannels, WorkerChannels},
    wire::{
        Control, PreparedPrime, PrimeDisposition, ReferenceControl, RenderReference, WorkerEvent,
    },
};
use std::{
    path::PathBuf,
    thread::{JoinHandle, sleep},
    time::{Duration, Instant},
};

/// A turn's lease, as the voice keeps it (`coordinator.rs`'s capture and
/// admission leases).
const TURN_LEASE_US: u64 = 100_000;

/// The longest the reference link is given to connect: a bound, not a wait.
const CONNECT_AT_MOST: Duration = Duration::from_secs(5);

fn now() -> MonoTime {
    MonoTime::from_micros(monotonic_us())
}

struct Rig {
    _directory: SessionDirectory,
    controller: Controller,
    parent: WorkerChannels,
    capture: ReferenceChannels,
    capture_boot: BootId,
    worker: Option<JoinHandle<Result<(), String>>>,
    renewing: bool,
    authority_at: Option<Instant>,
    privacy: u64,
    /// The reference link is connected.
    linked: bool,
    /// The capture side says it is ready, once linked: the speaker then
    /// primes, and every prime needs its authority.
    prepare: bool,
    prepared_sent: bool,
    faults: Vec<String>,
    /// What the worker told the capture side on the reference link, in order.
    told: Vec<ReferenceControl>,
    /// The playback epochs the worker's clock resets moved to.
    clock_resets: Vec<u64>,
}

/// A control message the parent had sent before the worker first looked,
/// stamped this many µs before the worker starts: as when a worker starts
/// slowly and its control messages queue meanwhile (#1569 row 42).
#[derive(Clone, Copy)]
enum Queued {
    Authority(u64),
    ConnectReference(u64),
}

impl Rig {
    fn start() -> Self {
        Self::start_queued(&[])
    }

    /// With `queued`, those are sent before the worker starts, in their
    /// order, and stand for the start's own authority and reference link.
    fn start_queued(queued: &[Queued]) -> Self {
        Self::start_spawning(queued, |channels, parent_boot, worker_boot, path| {
            std::thread::spawn(move || {
                speaker(channels, parent_boot, worker_boot, &path, "null", None)
                    .map_err(|error| error.to_string())
            })
        })
    }

    /// With `spawn`, which runs the worker on its channels.
    fn start_spawning(
        queued: &[Queued],
        spawn: impl FnOnce(WorkerChannels, BootId, BootId, PathBuf) -> JoinHandle<Result<(), String>>,
    ) -> Self {
        let directory = SessionDirectory::create().unwrap();
        let (parent_boot, worker_boot, capture_boot) = (
            new_boot().unwrap(),
            new_boot().unwrap(),
            new_boot().unwrap(),
        );
        let mut parent =
            WorkerChannels::bind(&directory.path, "speaker", parent_boot, worker_boot, false)
                .unwrap();
        let mut channels =
            WorkerChannels::bind(&directory.path, "speaker", parent_boot, worker_boot, true)
                .unwrap();
        parent.connect(&directory.path, "speaker", false).unwrap();
        channels.connect(&directory.path, "speaker", true).unwrap();
        let capture =
            ReferenceChannels::bind(&directory.path, "capture", capture_boot, worker_boot).unwrap();
        let path = directory.path.clone();
        let mut controller = Controller::new(parent_boot, now());
        controller
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        let mut rig = Self {
            _directory: directory,
            controller,
            parent,
            capture,
            capture_boot,
            worker: None,
            renewing: true,
            authority_at: None,
            privacy: 0,
            linked: false,
            prepare: false,
            prepared_sent: false,
            faults: Vec::new(),
            told: Vec::new(),
            clock_resets: Vec::new(),
        };
        // A turn owns the lamp, its leases a turn's.
        rig.renew_leases();
        rig.controller.admit(now(), AdmittedInput::NewTurn).unwrap();
        let at_us = monotonic_us();
        for queued in queued {
            let (control, age_us) = match *queued {
                Queued::Authority(age_us) => (
                    Control::Authority {
                        snapshot: rig.controller.snapshot(now()).unwrap(),
                    },
                    age_us,
                ),
                Queued::ConnectReference(age_us) => (
                    Control::ConnectReference {
                        peer: rig.capture_boot,
                    },
                    age_us,
                ),
            };
            rig.parent
                .control
                .send_with(control, || at_us - age_us)
                .unwrap();
        }
        rig.worker = Some(spawn(channels, parent_boot, worker_boot, path));
        if queued.is_empty() {
            rig.service();
            rig.parent
                .control
                .send(Control::ConnectReference {
                    peer: rig.capture_boot,
                })
                .unwrap();
        } else {
            rig.authority_at = Some(Instant::now());
        }
        rig
    }

    /// Wait until the worker has ended, up to a second, sending nothing:
    /// its error, or `None` if it is still running then.
    fn until_ended(&mut self) -> Option<String> {
        let from = Instant::now();
        while from.elapsed() < Duration::from_secs(1) {
            if self.worker.as_ref().unwrap().is_finished() {
                self.take_events();
                return self.worker.take().unwrap().join().unwrap().err();
            }
            sleep(Duration::from_millis(2));
        }
        None
    }

    fn renew_leases(&mut self) {
        let at = now();
        let until = MonoTime::from_micros(at.as_micros() + TURN_LEASE_US);
        self.controller
            .set_capture(at, CaptureState::RetainingUntil(until))
            .unwrap();
        self.controller
            .set_admission(at, AdmissionState::OpenUntil(until))
            .unwrap();
    }

    /// Renew authority every 20 ms as the coordinator does, unless held, and
    /// take whatever the worker and its reference link have sent.
    fn service(&mut self) {
        if self.renewing
            && self
                .authority_at
                .is_none_or(|at| at.elapsed() >= Duration::from_millis(20))
        {
            self.renew_leases();
            let snapshot = self.controller.snapshot(now()).unwrap();
            self.privacy = snapshot.microphone_generation();
            let sent = self.parent.control.send(Control::Authority { snapshot });
            self.unless_ended(sent, "sending the worker its authority");
            self.authority_at = Some(Instant::now());
        }
        self.take_events();
        let linked = self.capture.connect_step();
        if !self.unless_ended(linked, "connecting the reference link") {
            return;
        }
        self.linked = true;
        while let Ok(Some(told)) = self.capture.control.receive::<ReferenceControl>() {
            self.told.push(told);
        }
        while let Ok(Some(_)) = self.capture.data.receive::<RenderReference>() {}
        if self.prepare && !self.prepared_sent {
            let sent = self
                .capture
                .control
                .send(ReferenceControl::CapturePrepared {
                    privacy_generation: self.privacy,
                });
            self.unless_ended(sent, "telling the worker the capture side is ready");
            self.prepared_sent = true;
        }
    }

    /// What `result` holds, or, if it failed because the worker had ended
    /// (its sockets closed: "Connection refused"), the worker's own account,
    /// as `alive` gives it. An unwrap here hid it (#1569 row 42: both
    /// tests failed at 0.25 s on ARM64 with only "speaker.c.p: Connection
    /// refused").
    fn unless_ended<T>(&mut self, result: std::io::Result<T>, what: &str) -> T {
        match result {
            Ok(value) => value,
            Err(error) => {
                let from = Instant::now();
                while from.elapsed() < Duration::from_secs(1) {
                    self.alive(what);
                    sleep(Duration::from_millis(2));
                }
                panic!("{what}: {error}, and the speaker worker is still running");
            }
        }
    }

    fn take_events(&mut self) {
        while let Ok(Some(event)) = self.parent.control.receive::<WorkerEvent>() {
            match event {
                WorkerEvent::Fault { code } => self.faults.push(code),
                WorkerEvent::PlaybackClockReset { next_epoch, .. } => {
                    self.clock_resets.push(next_epoch);
                }
                _ => {}
            }
        }
    }

    /// Service for `long`, failing with the worker's own account if it ends.
    fn run_for(&mut self, long: Duration, what: &str) {
        let from = Instant::now();
        while from.elapsed() < long {
            self.service();
            self.alive(what);
            sleep(Duration::from_millis(2));
        }
    }

    /// Service until the reference link has connected, as long as it takes
    /// up to `CONNECT_AT_MOST`. A fixed 150 ms for it failed on a loaded
    /// ARM64 runner (#1569 row 39): the lapse is what is measured, not how
    /// soon a busy machine connects a socket.
    fn connect(&mut self) {
        let from = Instant::now();
        while !self.linked {
            assert!(
                from.elapsed() < CONNECT_AT_MOST,
                "the reference link never connected in {CONNECT_AT_MOST:?}"
            );
            self.service();
            self.alive("connecting the reference link");
            sleep(Duration::from_millis(2));
        }
    }

    /// Fails with the worker's own account if it has ended: the fault it
    /// sent as it ended, and what it returned.
    fn alive(&mut self, what: &str) {
        if !self.worker.as_ref().unwrap().is_finished() {
            return;
        }
        self.take_events();
        let ended = self.worker.take().unwrap().join().unwrap();
        panic!(
            "{what}: the speaker worker ended: {ended:?}, faults {:?}",
            self.faults
        );
    }

    /// The capture side answers a prime of `epoch`, as it would have.
    fn answer_prime(&mut self, epoch: u64) {
        self.capture
            .control
            .send(ReferenceControl::ReferencePrimed {
                prepared: PreparedPrime {
                    playback_epoch: epoch,
                    privacy_generation: self.privacy,
                    prepared_through: PRIME_FRAMES as u64,
                    analysed_through: PRIME_FRAMES as u64,
                    disposition: PrimeDisposition::Analysed,
                },
            })
            .unwrap();
    }

    fn stop(mut self) -> Result<(), String> {
        self.parent.control.send(Control::Stop).unwrap();
        self.worker.take().unwrap().join().unwrap()
    }
}

/// #1178, the first builder's 03:06: the speaker held past a turn's lease
/// (100 ms) with no fresher authority, and the capture side ready meanwhile,
/// so that its next prime needs the lapsed authority. It stays up and stops
/// cleanly. Before the fix the prime's `zero_authority` returned "authority
/// snapshot has expired" and the worker ended, and the voice with it.
#[test]
fn a_speaker_whose_authority_lapses_before_its_prime_stays_up() {
    let mut rig = Rig::start();
    rig.connect();
    rig.run_for(Duration::from_millis(150), "with authority");
    // Held: no authority for twice a turn's lease, as when the voice is held
    // up by the eMMC at midnight.
    rig.renewing = false;
    rig.run_for(Duration::from_millis(200), "the lapse");
    rig.prepare = true;
    rig.run_for(Duration::from_millis(200), "ready to prime while lapsed");
    assert!(rig.prepared_sent);
    assert!(rig.faults.is_empty(), "{:?}", rig.faults);
    assert_eq!(rig.stop(), Ok(()));
}

/// Ten on #1214: the capture side's answer to a prime the speaker reset
/// before it came. A lapse resets the speaker from epoch 1 to 2; a fresh
/// authority comes and the worker goes on; then the answer to epoch 1
/// arrives. It is dropped, and the worker stays up. Taken, it ended the
/// worker (`InvalidStart`), so a second stall close after a first, during
/// the prime between them, would have ended the voice.
#[test]
fn a_late_prime_from_before_a_lapse_is_dropped() {
    let mut rig = Rig::start();
    rig.connect();
    rig.run_for(Duration::from_millis(150), "with authority");
    rig.renewing = false;
    rig.run_for(Duration::from_millis(200), "the lapse");
    rig.renewing = true;
    rig.run_for(Duration::from_millis(150), "authority again");
    rig.answer_prime(1);
    rig.run_for(Duration::from_millis(150), "a late prime");
    assert!(rig.faults.is_empty(), "{:?}", rig.faults);
    assert_eq!(rig.stop(), Ok(()));
}

/// #1569 row 42 (the first builder on #1722, 20 Oct 17:46): a worker that
/// starts slowly finds its control messages queued, the third a renewal
/// more than 100 ms old, past the first message's grace and the one late
/// message a channel takes. With the next renewal behind it, that one is
/// passed over and the worker goes on: it links its reference and plays.
/// Before, this ended it ("sequence 3 after 2 … stale"), three times on
/// ARM64 after #2118 gave the test every core.
#[test]
fn a_stale_renewal_with_a_fresh_one_behind_it_keeps_the_speaker_going() {
    let mut rig = Rig::start_queued(&[
        Queued::Authority(300_000),
        Queued::ConnectReference(290_000),
        Queued::Authority(200_000),
        Queued::Authority(10_000),
    ]);
    rig.connect();
    rig.run_for(Duration::from_millis(150), "after the stale renewal");
    assert!(rig.faults.is_empty(), "{:?}", rig.faults);
    assert_eq!(rig.stop(), Ok(()));
}

/// The rule's other side: a stale renewal with nothing behind it, and a
/// stale message of another kind with a fresh renewal behind it, still end
/// the worker, as before.
#[test]
fn a_stale_renewal_alone_or_a_stale_connect_reference_still_ends_the_speaker() {
    let mut alone = Rig::start_queued(&[
        Queued::Authority(300_000),
        Queued::ConnectReference(290_000),
        Queued::Authority(200_000),
    ]);
    alone.renewing = false;
    let ended = alone
        .until_ended()
        .expect("a stale renewal alone ends the worker");
    assert!(ended.contains("stale"), "{ended}");

    let mut connect = Rig::start_queued(&[
        Queued::Authority(300_000),
        Queued::Authority(290_000),
        Queued::ConnectReference(200_000),
        Queued::Authority(10_000),
    ]);
    connect.renewing = false;
    let ended = connect
        .until_ended()
        .expect("a stale ConnectReference ends the worker");
    assert!(ended.contains("stale"), "{ended}");
}

/// The first direct prepare in #3530 was 143402 us old. Reaching the
/// speaker's own prime/privacy validation proves that the real loop took
/// it, rather than failing transport freshness or silently dropping it.
/// ALSA null cannot retain the prime's 480 frames, so InvalidPrime is the
/// expected downstream witness, not a claim of playback or startup success.
#[test]
fn an_aged_first_reference_prepare_reaches_the_real_speakers_validation() {
    for (wrong_privacy, expected) in [
        (false, "reference InvalidPrime:"),
        (true, "reference PrivacyMismatch:"),
    ] {
        // Queue these before the thread starts, so every later assertion is
        // inside the cleanup boundary below, including connection failures.
        let mut rig = Rig::start_queued(&[Queued::Authority(0), Queued::ConnectReference(0)]);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rig.connect();
            rig.renew_leases();
            let snapshot = rig.controller.snapshot(now()).unwrap();
            rig.privacy = snapshot.microphone_generation();
            rig.parent
                .control
                .send(Control::Authority { snapshot })
                .unwrap();
            rig.authority_at = Some(Instant::now());
            let privacy_generation = if wrong_privacy { 0 } else { rig.privacy };
            assert_ne!(rig.privacy, 0);
            let sent_at = monotonic_us().checked_sub(143_402).unwrap();
            rig.capture
                .control
                .send_with(
                    ReferenceControl::CapturePrepared { privacy_generation },
                    || sent_at,
                )
                .unwrap();
            let deadline = Instant::now() + CONNECT_AT_MOST;
            let mut send_error = None;
            let ended = loop {
                if rig.worker.as_ref().unwrap().is_finished() {
                    break rig.worker.take().unwrap().join().unwrap();
                }
                assert!(
                    Instant::now() < deadline,
                    "the prepare was not consumed; authority send error: {send_error:?}"
                );
                // Preserve the ordinary 100 ms lease and 20 ms renewal.
                // A send can race the expected worker exit: keep its error
                // for diagnosis and wait for the worker's own final result.
                if send_error.is_none()
                    && rig
                        .authority_at
                        .is_none_or(|at| at.elapsed() >= Duration::from_millis(20))
                {
                    rig.renew_leases();
                    let snapshot = rig.controller.snapshot(now()).unwrap();
                    if let Err(error) = rig.parent.control.send(Control::Authority { snapshot }) {
                        send_error = Some(error.to_string());
                    }
                    rig.authority_at = Some(Instant::now());
                }
                rig.take_events();
                sleep(Duration::from_millis(2));
            };
            let error = ended.expect_err("null must not claim a completed prime");
            assert!(
                error.starts_with(expected),
                "expected {expected}, got {error}"
            );
            assert!(!error.contains("stale"), "{error}");
        }));
        if let Some(worker) = rig.worker.as_ref() {
            let _ = rig.parent.control.send(Control::Stop);
            let deadline = Instant::now() + CONNECT_AT_MOST;
            while !worker.is_finished() && Instant::now() < deadline {
                sleep(Duration::from_millis(2));
            }
            if !worker.is_finished() {
                // A Rust thread cannot be killed separately. Do not detach
                // an unconfirmed audio worker after a fixture failure.
                eprintln!("speaker reference fixture could not confirm worker shutdown");
                std::process::abort();
            }
            let _ = rig.worker.take().unwrap().join();
        }
        if let Err(failure) = result {
            std::panic::resume_unwind(failure);
        }
    }
}

/// A speaker over a software queue whose accepted sound is kept in `sound`,
/// in the clock mode the worker uses, and the queue's failures.
fn soft_speaker(
    boot: BootId,
    sound: std::sync::Arc<std::sync::Mutex<Vec<i16>>>,
) -> (lamp_audio::pcm::Speaker, lamp_audio::soft_pcm::SoftFaults) {
    let queue = lamp_audio::soft_pcm::SoftPcm::real_time(move |frames| {
        sound.lock().unwrap().extend_from_slice(frames);
    });
    let faults = queue.faults();
    let speaker = lamp_audio::pcm::Speaker::with_pcm(
        Box::new(queue),
        lamp_audio::soft_pcm::FORMAT,
        boot,
        true,
    )
    .unwrap();
    (speaker, faults)
}

impl Rig {
    /// Service until the worker has told the capture side something `wanted`
    /// picks, up to two seconds; its index in `told`.
    fn until_told(&mut self, what: &str, wanted: impl Fn(&ReferenceControl) -> bool) -> usize {
        let from = Instant::now();
        loop {
            if let Some(index) = self.told.iter().position(&wanted) {
                return index;
            }
            assert!(
                from.elapsed() < Duration::from_secs(2),
                "never told {what}: {:?}, faults {:?}",
                self.told,
                self.faults
            );
            self.service();
            self.alive(what);
            sleep(Duration::from_millis(2));
        }
    }

    /// The worker primed `epoch`; the capture side answers it, and the
    /// worker starts its clock.
    fn prime_and_start(&mut self, epoch: u64) {
        self.until_told("the prime", |told| {
            matches!(told, ReferenceControl::ClockPrimed { prime } if prime.playback_epoch == epoch)
        });
        self.answer_prime(epoch);
        self.until_told("the clock's start", |told| {
            matches!(told, ReferenceControl::ClockStarted { playback_epoch, .. } if *playback_epoch == epoch)
        });
    }
}

/// #8269 (d): the speaker's device goes mid-run (a write `ENODEV`, and its
/// discard too, as one USB card going). The old speaker's guard stays
/// latched and it is dropped; the worker tells the capture side the speaker
/// went, tries the device again every 250 ms, and opens a fresh speaker with
/// a new guard. That one waits for the coordinator's next snapshot, primes a
/// new epoch, and plays on. What was queued is dropped and reported, never
/// written again: the old queue takes nothing after the fault, and the new
/// one only silence. Before, the worker ended and the voice with it.
#[test]
fn a_speaker_gone_mid_run_is_opened_again_with_nothing_replayed() {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    };
    let old_sound = Arc::new(Mutex::new(Vec::new()));
    let new_sound = Arc::new(Mutex::new(Vec::new()));
    let tries = Arc::new(AtomicU32::new(0));
    let faults = Arc::new(Mutex::new(None));
    let (old, new, counted, kept) = (
        old_sound.clone(),
        new_sound.clone(),
        tries.clone(),
        faults.clone(),
    );
    let mut rig = Rig::start_spawning(&[], move |channels, parent_boot, worker_boot, path| {
        std::thread::spawn(move || {
            speaker_on(
                channels,
                parent_boot,
                worker_boot,
                &path,
                None,
                |_| {
                    let (speaker, faults) = soft_speaker(parent_boot, old);
                    *kept.lock().unwrap() = Some(faults);
                    Ok(speaker)
                },
                move || {
                    // Off USB for two tries, then back.
                    if counted.fetch_add(1, Ordering::SeqCst) < 2 {
                        return Err(lamp_audio::pcm::PcmError::new(
                            "snd_pcm_open",
                            rustix::io::Errno::NODEV.raw_os_error(),
                        )
                        .into());
                    }
                    Ok(soft_speaker(parent_boot, new.clone()).0)
                },
            )
            .map_err(|error| error.to_string())
        })
    });
    rig.connect();
    rig.prepare = true;
    rig.prime_and_start(1);
    rig.run_for(Duration::from_millis(200), "playing");
    let nodev = rustix::io::Errno::NODEV.raw_os_error();
    {
        let faults = faults.lock().unwrap();
        let faults = faults.as_ref().expect("the first speaker's queue");
        faults.fail_next("snd_pcm_writei", nodev);
        faults.fail_next("snd_pcm_drop", nodev);
    }
    let gone = rig.until_told("the speaker went", |told| {
        matches!(
            told,
            ReferenceControl::SpeakerGone {
                playback_epoch: 1,
                ..
            }
        )
    });
    let old_taken = old_sound.lock().unwrap().len();
    assert!(old_taken > 0, "the first speaker played nothing");
    // The new epoch's prime comes only after the speaker went.
    rig.prime_and_start(2);
    assert!(
        rig.told
            .iter()
            .position(|told| matches!(told, ReferenceControl::ClockPrimed { prime } if prime.playback_epoch == 2))
            > Some(gone)
    );
    rig.run_for(Duration::from_millis(200), "playing again");
    assert_eq!(tries.load(Ordering::SeqCst), 3, "opened on the third try");
    assert_eq!(
        old_sound.lock().unwrap().len(),
        old_taken,
        "the gone speaker was written after it went"
    );
    let played = new_sound.lock().unwrap().clone();
    assert!(!played.is_empty(), "the fresh speaker played nothing");
    assert!(played.iter().all(|&s| s == 0), "something was replayed");
    assert_eq!(rig.clock_resets, [2], "the reset to the new epoch");
    assert!(rig.faults.is_empty(), "{:?}", rig.faults);
    assert_eq!(rig.stop(), Ok(()));
}
