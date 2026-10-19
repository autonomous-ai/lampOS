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
    audio_workers::speaker,
    process::{SessionDirectory, new_boot},
    reference::PRIME_FRAMES,
    transport::{ReferenceChannels, WorkerChannels},
    wire::{Control, PreparedPrime, PrimeDisposition, ReferenceControl, WorkerEvent},
};
use std::{
    thread::{JoinHandle, sleep},
    time::{Duration, Instant},
};

/// A turn's lease, as the voice keeps it (`coordinator.rs`'s capture and
/// admission leases).
const TURN_LEASE_US: u64 = 100_000;

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
}

impl Rig {
    fn start() -> Self {
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
        let worker = std::thread::spawn(move || {
            speaker(channels, parent_boot, worker_boot, &path, "null", None)
                .map_err(|error| error.to_string())
        });
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
            worker: Some(worker),
            renewing: true,
            authority_at: None,
            privacy: 0,
            linked: false,
            prepare: false,
            prepared_sent: false,
            faults: Vec::new(),
        };
        // A turn owns the lamp, its leases a turn's.
        rig.renew_leases();
        rig.controller.admit(now(), AdmittedInput::NewTurn).unwrap();
        rig.service();
        rig.parent
            .control
            .send(Control::ConnectReference {
                peer: rig.capture_boot,
            })
            .unwrap();
        rig
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
            self.parent
                .control
                .send(Control::Authority { snapshot })
                .unwrap();
            self.authority_at = Some(Instant::now());
        }
        self.take_events();
        if !self.capture.connect_step().unwrap() {
            return;
        }
        self.linked = true;
        if self.prepare && !self.prepared_sent {
            self.capture
                .control
                .send(ReferenceControl::CapturePrepared {
                    privacy_generation: self.privacy,
                })
                .unwrap();
            self.prepared_sent = true;
        }
    }

    fn take_events(&mut self) {
        while let Ok(Some(event)) = self.parent.control.receive::<WorkerEvent>() {
            if let WorkerEvent::Fault { code } = event {
                self.faults.push(code);
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
    rig.run_for(Duration::from_millis(150), "with authority");
    assert!(rig.linked, "the reference link never connected");
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
    rig.run_for(Duration::from_millis(150), "with authority");
    assert!(rig.linked, "the reference link never connected");
    rig.renewing = false;
    rig.run_for(Duration::from_millis(200), "the lapse");
    rig.renewing = true;
    rig.run_for(Duration::from_millis(150), "authority again");
    rig.answer_prime(1);
    rig.run_for(Duration::from_millis(150), "a late prime");
    assert!(rig.faults.is_empty(), "{:?}", rig.faults);
    assert_eq!(rig.stop(), Ok(()));
}
