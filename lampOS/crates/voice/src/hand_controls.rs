//! Optional hand controls. Only the coordinator owns the mute latch; a single
//! bounded pump does bus/filesystem I/O for all supervised runtime cycles.
//!
//! A mute request closes live authority before persistence. Before audio opens,
//! `arm_active` stores a conservative muted recovery record without changing the
//! live preference. A crash/fault therefore restores muted; only verified clean
//! cleanup, or a stop the service was asked for (SIGTERM, an update; #960),
//! may disarm that guard, writing the live latch as it was. An unacknowledged request before any durable
//! write cannot be promised to survive arbitrary storage/power failure.
//!
//! With `LAMP_RESTORED_MUTE=on` (#960), arming also leaves a marker beside the
//! record, removed with the guard, by a mute the person chose, or by a clear.
//! A restart that finds the record muted and the marker present knows the mute
//! was restored by the guard after a fault, not chosen.
use lamp_bus::{Bus, Kind, Message, Notice, Publisher, Subscriber};
use lamp_controls::{mute::MuteRecord, receiver::Receiver, touch_receiver::TouchReceiver};
use lamp_msgs::{
    ControlGesture, ControlIntent, ControlsState, MuteReason, TouchEvent, TouchGesture,
    TouchSource, TouchState, VoiceMute,
};
use std::{
    io,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver as ChannelReceiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
// The clock the coordinator's loop polls with: in its stepped tests the
// scenario clock, which is the host's for every unscoped test.
#[cfg(test)]
use crate::scenario_clock::monotonic_us;
#[cfg(not(test))]
use lamp_ipc::monotonic_us;

const QUEUE_CAPACITY: usize = 32;
const POLL_LIMIT: usize = 16;
const PUMP_TICK: Duration = Duration::from_millis(2);
const RECONNECT_AFTER: Duration = Duration::from_millis(250);
const FINAL_JOIN_WAIT: Duration = Duration::from_millis(20);
/// `voice/mute` is said when it changes and at least this often.
const MUTE_SAID_EVERY: Duration = Duration::from_secs(1);
const SUBSCRIBER: &str = "voice_hand";
/// Off unless `on`: keep the restart guard's marker, so a restored mute can be
/// told from a chosen one (#960).
pub const RESTORED_MUTE_ENVIRONMENT: &str = "LAMP_RESTORED_MUTE";
const GUARD_MARKER_SUFFIX: &str = ".restart-guard";

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControlsOptions {
    pub stop: bool,
    /// Existing trusted private directory, absolute filename; enables mute.
    pub mute_record: Option<PathBuf>,
    /// Overrides `LAMP_BUS_ROOT`; neither setting alone enables consumption.
    pub bus_root: Option<PathBuf>,
    /// The head pads' requests on `controls/touch` (#711): a qualified touch
    /// stops the lamp while it speaks. With `mute_record` too, a hand held on
    /// the head puts it to sleep (the software mute, reason `sleep`), a tap
    /// wakes it, and `voice/mute` says which. `LAMP_HEAD_TOUCH=on`; off
    /// unless set.
    pub head_touch: bool,
}

/// `on` for the head pads' requests ([`ControlsOptions::head_touch`]).
pub const HEAD_TOUCH_ENVIRONMENT: &str = "LAMP_HEAD_TOUCH";

/// Whether the environment says `LAMP_HEAD_TOUCH=on`.
pub fn head_touch_from_environment() -> bool {
    std::env::var(HEAD_TOUCH_ENVIRONMENT).ok().as_deref() == Some("on")
}

impl ControlsOptions {
    pub fn enabled(&self) -> bool {
        self.stop || self.mute_record.is_some() || self.head_touch
    }

    /// Sleep from the head needs both its switch and the durable mute.
    pub fn head_sleep(&self) -> bool {
        self.head_touch && self.mute_record.is_some()
    }

    /// Shape checks only: disabled options never open a path or start a thread.
    pub fn validate(&self) -> io::Result<()> {
        if self
            .bus_root
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        {
            return Err(invalid("hand controls bus root must be absolute"));
        }
        if self
            .mute_record
            .as_ref()
            .is_some_and(|path| !path.is_absolute() || path.file_name().is_none())
        {
            return Err(invalid(
                "hand controls mute record needs an absolute filename",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Observation {
    pub stop: bool,
    pub muted: bool,
    pub mute_changed: bool,
    pub pets: u64,
    /// Coordinator poll/acceptance time, on the shared monotonic clock. This
    /// excludes recognizer, publication, bus and pump queue delay.
    pub observed_at_us: Option<u64>,
    /// Earliest original release accepted for Stop/mute in this poll. Neither
    /// timestamp is speaker delivery or evidence of acoustic silence.
    pub acquired_at_us: Option<u64>,
    /// The acquisition of a qualified head touch accepted in this poll
    /// (#711). It asks a speaking lamp to stop and a quiet one nothing; the
    /// coordinator, which knows which it is, decides.
    pub head_touch_at_us: Option<u64>,
    /// The acquisition of a head tap accepted in this poll (#711), and when
    /// its hand arrived.
    pub head_tap_at_us: Option<u64>,
    pub head_tap_began_us: Option<u64>,
    /// The software mute is set because a hand held on the head put the lamp
    /// to sleep (#711); always with `muted`.
    pub asleep: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Health {
    pub received: u64,
    pub rejected: u64,
    pub lost: u64,
    pub reconnects: u64,
    pub discontinuities: u64,
    pub pump_running: bool,
    pub persistence_failed: bool,
}

#[derive(Default)]
struct Shared {
    received: AtomicU64,
    rejected: AtomicU64,
    lost: AtomicU64,
    reconnects: AtomicU64,
    discontinuities: AtomicU64,
    queued: AtomicUsize,
    running: AtomicBool,
    stop: AtomicBool,
    /// The live latch for `voice/mute` (#711): `LATCH_*`, set by the owner
    /// and said by the pump.
    latch: AtomicU8,
}

const LATCH_NONE: u8 = 0;
const LATCH_OPEN: u8 = 1;
const LATCH_MUTED: u8 = 2;
const LATCH_ASLEEP: u8 = 3;

fn latch_message(latch: u8) -> Option<VoiceMute> {
    match latch {
        LATCH_OPEN => Some(VoiceMute {
            muted: false,
            reason: None,
        }),
        LATCH_MUTED => Some(VoiceMute {
            muted: true,
            reason: None,
        }),
        LATCH_ASLEEP => Some(VoiceMute {
            muted: true,
            reason: Some(MuteReason::Sleep),
        }),
        _ => None,
    }
}

fn add(counter: &AtomicU64, amount: u64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(value.saturating_add(amount))
    });
}

impl Shared {
    fn fence(&self) {
        // Saturation remains a permanently unsafe continuity observation.
        let _ = self
            .discontinuities
            .fetch_update(Ordering::Release, Ordering::Relaxed, |value| {
                Some(value.saturating_add(1))
            });
    }

    fn reject(&self) {
        add(&self.rejected, 1);
        self.fence();
    }
}

enum Input {
    State(Box<ControlsState>),
    Pet(ControlGesture),
    TouchState(Box<TouchState>),
    Touch(Box<TouchEvent>),
}

struct Queued {
    input: Input,
    generation: u64,
}

struct Pump {
    thread: Option<JoinHandle<()>>,
    finished: ChannelReceiver<()>,
    shared: Arc<Shared>,
}

struct PumpFinished {
    shared: Arc<Shared>,
    finished: SyncSender<()>,
}

impl Drop for PumpFinished {
    fn drop(&mut self) {
        self.shared.running.store(false, Ordering::Release);
        self.shared.fence();
        let _ = self.finished.try_send(());
    }
}

impl Drop for Pump {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(worker) = self.thread.take() {
            worker.thread().unpark();
            // Never wait indefinitely for a filesystem/syscall inside Bus.
            // This owner is dropped only on final service exit, not per cycle.
            if self.finished.recv_timeout(FINAL_JOIN_WAIT).is_ok() || worker.is_finished() {
                let _ = worker.join();
            }
        }
    }
}

pub struct HandControls {
    options: ControlsOptions,
    receiver: Receiver,
    input: ChannelReceiver<Queued>,
    shared: Arc<Shared>,
    seen_generation: u64,
    dirty: bool,
    armed: bool,
    persistence_failed: bool,
    /// The head pads' requests, checked as Controls' own receiver does:
    /// fresh, from the current Controls and reader, each once.
    touch: Option<TouchReceiver>,
    /// `LAMP_RESTORED_MUTE=on`: arming leaves the guard's marker.
    mark_guard: bool,
    /// The record was muted by the restart guard, not by the person.
    restored_after_fault: bool,
    // One handle for the entire service, not one per supervised audio cycle.
    _pump: Option<Pump>,
}

impl HandControls {
    /// Composed coordinator tests use the real receiver and bounded channel,
    /// without a GPIO, bus thread, persistence path or invented permissions.
    #[cfg(test)]
    pub(crate) fn with_test_state(
        clock_id: &str,
        started_us: u64,
        state: ControlsState,
    ) -> io::Result<Self> {
        let (sender, input) = mpsc::sync_channel(QUEUE_CAPACITY);
        let shared = Arc::new(Shared::default());
        let owner = Self::from_parts(
            ControlsOptions {
                stop: true,
                ..Default::default()
            },
            clock_id,
            MuteRecord::new(clock_id)?,
            input,
            shared,
            started_us,
        )?;
        enqueue(&sender, &owner.shared, Input::State(Box::new(state)));
        Ok(owner)
    }

    /// As [`Self::with_test_state`], for the head pads: their state, then
    /// their events, as the touch runtime publishes them.
    #[cfg(test)]
    pub(crate) fn with_test_touch(
        clock_id: &str,
        started_us: u64,
        state: TouchState,
        events: Vec<TouchEvent>,
    ) -> io::Result<Self> {
        let (sender, input) = mpsc::sync_channel(QUEUE_CAPACITY);
        let shared = Arc::new(Shared::default());
        let owner = Self::from_parts(
            ControlsOptions {
                head_touch: true,
                ..Default::default()
            },
            clock_id,
            MuteRecord::new(clock_id)?,
            input,
            shared,
            started_us,
        )?;
        enqueue(&sender, &owner.shared, Input::TouchState(Box::new(state)));
        for event in events {
            enqueue(&sender, &owner.shared, Input::Touch(Box::new(event)));
        }
        Ok(owner)
    }

    /// As [`Self::with_test_touch`], for a tap on the head (#711): the head
    /// seen settled from `tapped_us - 900 ms`, a hand on it from `tapped_us`
    /// for 200 ms, then off until `tapped_us + 400 ms`. Every step before
    /// the tap is raised is taken now; the rest, the tap with it, wait in
    /// the queue for the first poll. So a tap's hand began after its
    /// reader was seen, as on a lamp, and the tap is fresh at that poll.
    #[cfg(test)]
    pub(crate) fn with_test_tap(clock_id: &str, tapped_us: u64) -> io::Result<Self> {
        let (sender, input) = mpsc::sync_channel(QUEUE_CAPACITY);
        let mut owner = Self::from_parts(
            ControlsOptions {
                head_touch: true,
                ..Default::default()
            },
            clock_id,
            MuteRecord::new(clock_id)?,
            input,
            Arc::new(Shared::default()),
            tapped_us - 1_000_000,
        )?;
        let mut head = TestHead::new(clock_id, tapped_us - 900_000);
        let mut ticks = head.run(tapped_us - 10_000, false);
        ticks.extend(head.run(tapped_us + 190_000, true));
        ticks.extend(head.run(tapped_us + 400_000, false));
        let raised = ticks
            .iter()
            .position(|(_, _, events)| events.iter().any(|e| e.gesture == TouchGesture::Tap))
            .expect("Controls raises a tap");
        let waiting = ticks.split_off(raised);
        for (at, state, events) in ticks {
            enqueue(&sender, &owner.shared, Input::TouchState(Box::new(state)));
            for event in events {
                enqueue(&sender, &owner.shared, Input::Touch(Box::new(event)));
            }
            owner.poll_with_clock(|| at)?;
        }
        for (_, state, events) in waiting {
            enqueue(&sender, &owner.shared, Input::TouchState(Box::new(state)));
            for event in events {
                enqueue(&sender, &owner.shared, Input::Touch(Box::new(event)));
            }
        }
        // The queue's sender stays open: a closed one is input lost.
        std::mem::forget(sender);
        Ok(owner)
    }

    /// As [`Self::with_test_touch`], already put to sleep by a hand held on
    /// the head (#711), its durable mute at `record` (never written here).
    #[cfg(test)]
    pub(crate) fn with_test_asleep(
        clock_id: &str,
        started_us: u64,
        record: &Path,
    ) -> io::Result<Self> {
        let (_sender, input) = mpsc::sync_channel(QUEUE_CAPACITY);
        let mut asleep = MuteRecord::new(clock_id)?;
        assert!(asleep.request_sleep(started_us / 2));
        Self::from_parts(
            ControlsOptions {
                head_touch: true,
                mute_record: Some(record.to_path_buf()),
                ..Default::default()
            },
            clock_id,
            asleep,
            input,
            Arc::new(Shared::default()),
            started_us,
        )
    }

    /// Restore and validate durability before any Voice worker is opened.
    /// The pump alone may open bus paths; an unavailable bus is optional input
    /// loss, not permission to unmute and not a failure of healthy audio.
    pub fn restore(options: &ControlsOptions) -> io::Result<Option<Self>> {
        options.validate()?;
        if !options.enabled() {
            return Ok(None);
        }
        let clock_id = lamp_controls::runtime::clock_id(None)?;
        let now_us = lamp_ipc::monotonic_us();
        // The stored record as it is on disk, before a new boot's rebase:
        // what the guard's marker must still match (the reviewer on #997).
        // Read only once `load_noting` has found a private regular file of
        // at most 4 KiB there (and before `save` writes anything), so a FIFO
        // or a huge file refuses with a line instead of hanging the voice
        // (Jarvis on #1548).
        let mut stored = None;
        let record = match &options.mute_record {
            Some(path) => {
                // A record this build cannot read never stops the voice: it
                // reads as muted, and that is said once (#1505).
                let (record, note) = MuteRecord::load_noting(path, &clock_id, now_us)?;
                if let Some(note) = note {
                    eprintln!("lamp-voice: {note}");
                }
                stored = stored_identity(path);
                // Checks the private parent and first-use/rebased persistence
                // now. A path that cannot store a mute may not open audio.
                record.save(path)?;
                record
            }
            None => MuteRecord::new(&clock_id)?,
        };
        let (sender, input) = mpsc::sync_channel(QUEUE_CAPACITY);
        let shared = Arc::new(Shared::default());
        let mut controls =
            Self::from_parts(options.clone(), &clock_id, record, input, shared, now_us)?;
        controls.mark_guard =
            std::env::var(RESTORED_MUTE_ENVIRONMENT).ok().as_deref() == Some("on");
        controls.restore_guard_marker(stored);
        let root = options
            .bus_root
            .clone()
            .or_else(|| std::env::var_os(lamp_bus::ROOT_VARIABLE).map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(lamp_bus::DEFAULT_ROOT));
        let pump_shared = Arc::clone(&controls.shared);
        let head_touch = options.head_touch;
        let say_mute = options.head_sleep();
        let (finished, completion) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("voice-hand-controls".into())
            .spawn(move || {
                pump_shared.running.store(true, Ordering::Release);
                let _finished = PumpFinished {
                    shared: Arc::clone(&pump_shared),
                    finished,
                };
                pump(root, head_touch, say_mute, sender, &pump_shared);
            })?;
        controls._pump = Some(Pump {
            thread: Some(worker),
            finished: completion,
            shared: Arc::clone(&controls.shared),
        });
        Ok(Some(controls))
    }

    fn from_parts(
        options: ControlsOptions,
        clock_id: &str,
        record: MuteRecord,
        input: ChannelReceiver<Queued>,
        shared: Arc<Shared>,
        now_us: u64,
    ) -> io::Result<Self> {
        let receiver = if options.mute_record.is_some() {
            Receiver::with_hold_clear(record, now_us)?
        } else {
            Receiver::new(record)?
        };
        let touch = options
            .head_touch
            .then(|| TouchReceiver::new(clock_id))
            .transpose()
            .map_err(|_| invalid("head touch needs a valid controls clock"))?;
        let controls = Self {
            options,
            receiver,
            input,
            shared,
            seen_generation: 0,
            dirty: false,
            armed: false,
            persistence_failed: false,
            touch,
            mark_guard: false,
            restored_after_fault: false,
            _pump: None,
        };
        controls.share_latch();
        Ok(controls)
    }

    /// The live latch for the pump to say on `voice/mute`, only where sleep
    /// from the head is on.
    fn share_latch(&self) {
        if !self.options.head_sleep() {
            return;
        }
        let latch = if !self.muted() {
            LATCH_OPEN
        } else if self.asleep() {
            LATCH_ASLEEP
        } else {
            LATCH_MUTED
        };
        self.shared.latch.store(latch, Ordering::Release);
    }

    pub fn health(&self) -> Health {
        Health {
            received: self.shared.received.load(Ordering::Relaxed),
            rejected: self.shared.rejected.load(Ordering::Relaxed),
            lost: self.shared.lost.load(Ordering::Relaxed),
            reconnects: self.shared.reconnects.load(Ordering::Relaxed),
            discontinuities: self.shared.discontinuities.load(Ordering::Acquire),
            pump_running: self.shared.running.load(Ordering::Acquire),
            persistence_failed: self.persistence_failed,
        }
    }

    fn muted(&self) -> bool {
        self.options.mute_record.is_some() && self.receiver.mute_record().muted
    }

    /// The head pads' requests are read (`LAMP_HEAD_TOUCH=on`, #711).
    pub fn head_touch(&self) -> bool {
        self.options.head_touch
    }

    fn asleep(&self) -> bool {
        self.options.mute_record.is_some() && self.receiver.mute_record().asleep()
    }

    /// The live mute was restored by the restart guard after a fault, and the
    /// person has neither chosen a mute since nor cleared it (#960). Always
    /// false without `LAMP_RESTORED_MUTE=on`.
    pub fn restored_after_fault(&self) -> bool {
        self.restored_after_fault && self.muted()
    }

    fn guard_marker(&self) -> Option<PathBuf> {
        let path = self.options.mute_record.as_ref()?;
        let mut name = path.file_name()?.to_owned();
        name.push(GUARD_MARKER_SUFFIX);
        Some(path.with_file_name(name))
    }

    /// At restore: a muted record with the marker beside it was muted by the
    /// guard. An unmuted record's leftover marker (a fault between writing it
    /// and the guard) means nothing and is removed. Without the switch, a
    /// marker left by an earlier run is removed and never read.
    ///
    /// The marker names the very guard record it was written with (its
    /// `clock_id` and `through_us`), and counts only while the record on
    /// disk, as stored before any rebase, still is that one. Every person's
    /// mute moves `through_us` forward, so a person's saved mute never matches
    /// a marker left beside it, even one a failed removal could not take.
    ///
    /// After a reboot, restore saves the record rebased to the new clock, so
    /// a match rewrites the marker to name that saved record: a second
    /// restart in the same boot (an update's clean stop) still knows the
    /// mute is the guard's (the first builder on #997). A rewrite that fails
    /// leaves the old name, which the next restart no longer matches: it
    /// fails closed, as a chosen mute.
    fn restore_guard_marker(&mut self, stored: Option<(String, u64)>) {
        let Some(marker) = self.guard_marker() else {
            return;
        };
        let present = marker.symlink_metadata().is_ok_and(|m| m.is_file());
        let names = std::fs::read_to_string(&marker)
            .ok()
            .and_then(|text| marker_identity(&text));
        if self.mark_guard && present && self.muted() && names.is_some() && names == stored {
            self.restored_after_fault = true;
            let saved = self
                .options
                .mute_record
                .as_deref()
                .and_then(stored_identity);
            if let Some(saved) = saved.filter(|saved| Some(saved) != names.as_ref()) {
                let _ = write_guard_marker(&marker, &saved);
            }
        } else if present {
            let _ = std::fs::remove_file(&marker);
        }
    }

    /// Writing the marker is advice: failing to write it changes no
    /// permission, since a missing marker reads as a chosen mute, which a tap
    /// never opens. Removing it is not: the marker is what lets a tap clear a
    /// mute, so it goes before a person's mute is saved or torn down, and a
    /// removal that fails fails closed for this run; across the restart the
    /// fault causes, the marker no longer matches the record ([`Self::drop_guard_marker`], the
    /// reviewer on #997).
    fn set_guard_marker(&mut self, present: bool) {
        if !present {
            self.restored_after_fault = false;
        }
        let Some(marker) = self.guard_marker() else {
            return;
        };
        if present {
            if self.mark_guard {
                // The guard record's identity, written with it.
                let record = self.receiver.mute_record();
                let identity = (record.clock_id.clone(), record.through_us);
                let _ = write_guard_marker(&marker, &identity);
            }
        } else if marker.symlink_metadata().is_ok() {
            let _ = std::fs::remove_file(&marker);
        }
    }

    /// Removes the guard's marker, its folder synced after, as
    /// `MuteRecord::save` syncs: a power cut must not bring it back beside a
    /// person's mute. A removal or sync that fails is a persistence fault,
    /// as a failed save is: nothing opens until recovery.
    fn drop_guard_marker(&mut self) -> io::Result<()> {
        self.restored_after_fault = false;
        let Some(marker) = self.guard_marker() else {
            return Ok(());
        };
        let removed = match std::fs::remove_file(&marker) {
            Ok(()) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => {
                self.persistence_failed = true;
                return Err(error);
            }
        };
        if removed
            && let Some(parent) = marker.parent()
            && let Err(error) = std::fs::File::open(parent).and_then(|dir| dir.sync_all())
        {
            self.persistence_failed = true;
            return Err(error);
        }
        Ok(())
    }

    /// A person's own mute (the button's or the strip's hold, or sleep) was
    /// accepted: the guard's marker goes at once, after revocation and before
    /// teardown, so a crash in teardown can never leave it beside the mute
    /// for a tap to open (the reviewer's P1 on #997). Not from the urgent
    /// loop: it touches the disk.
    pub fn mute_chosen(&mut self) -> io::Result<()> {
        if self.muted() {
            self.drop_guard_marker()?;
        }
        Ok(())
    }

    fn fence_at(&mut self, now_us: u64) {
        if self.receiver.fence_hold_clear(now_us).is_err() {
            // An exhausted/invalid optional receiver cannot clear its latch,
            // but must not turn bus loss into failure of healthy unmuted Voice.
            add(&self.shared.rejected, 1);
        }
    }

    /// No bus/filesystem I/O, blocking receive, channel send or provider call.
    pub fn poll(&mut self) -> io::Result<Observation> {
        self.poll_with_clock(monotonic_us)
    }

    #[cfg(test)]
    fn poll_at(&mut self, now_us: u64) -> io::Result<Observation> {
        self.poll_with_clock(|| now_us)
    }

    fn poll_with_clock(&mut self, mut now: impl FnMut() -> u64) -> io::Result<Observation> {
        let now_us = now();
        let mut observation = Observation::default();
        let generation = self.shared.discontinuities.load(Ordering::Acquire);
        if generation != self.seen_generation || generation == u64::MAX {
            self.fence_at(now_us);
            self.seen_generation = generation;
        }
        let mut consumed = 0;
        for _ in 0..POLL_LIMIT {
            let queued = match self.input.try_recv() {
                Ok(queued) => queued,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.fence_at(now());
                    break;
                }
            };
            self.shared.queued.fetch_sub(1, Ordering::AcqRel);
            consumed += 1;
            let now_us = now();
            if queued.generation != generation {
                self.fence_at(now_us);
            }
            match queued.input {
                Input::State(state) => match self.receiver.observe(now_us, &state) {
                    Ok(effects) => {
                        if self.options.stop && effects.stop_requested {
                            observation.stop = true;
                            if let Some(stop) = state.latest_stop {
                                earlier(&mut observation.acquired_at_us, stop.released_us);
                            }
                        }
                        if self.options.mute_record.is_some() && effects.mute_changed {
                            self.dirty = true;
                            observation.mute_changed = true;
                            if let Some(mute) = state.latest_mute {
                                earlier(&mut observation.acquired_at_us, mute.released_us);
                            }
                        }
                    }
                    Err(_) => add(&self.shared.rejected, 1),
                },
                Input::Pet(event) => match self.receiver.pet(now_us, &event) {
                    Ok(true) => observation.pets += 1,
                    Ok(false) => {}
                    Err(_) => add(&self.shared.rejected, 1),
                },
                Input::TouchState(state) => {
                    let accepted = self
                        .touch
                        .as_mut()
                        .map(|touch| touch.observe(now_us, &state));
                    match accepted {
                        Some(Ok(events)) => events
                            .iter()
                            .for_each(|event| self.head_event(now_us, event, &mut observation)),
                        Some(Err(_)) => add(&self.shared.rejected, 1),
                        None => {}
                    }
                }
                Input::Touch(event) => {
                    let accepted = self.touch.as_mut().map(|touch| touch.event(now_us, &event));
                    match accepted {
                        Some(Ok(Some(event))) => self.head_event(now_us, &event, &mut observation),
                        Some(Ok(None)) | None => {}
                        Some(Err(_)) => add(&self.shared.rejected, 1),
                    }
                }
            }
        }
        // A full batch may have newer requests still queued. It cannot clear.
        // Older one-way mutes and fresh Stops remain eligible in later batches.
        let now_us = now();
        if consumed == POLL_LIMIT
            || self.shared.discontinuities.load(Ordering::Acquire) != generation
            || self.persistence_failed
        {
            self.fence_at(now_us);
        }
        observation.muted = self.muted();
        observation.asleep = self.asleep();
        self.share_latch();
        if observation.stop
            || observation.mute_changed
            || observation.head_touch_at_us.is_some()
            || observation.head_tap_at_us.is_some()
        {
            observation.observed_at_us = Some(now_us);
        }
        Ok(observation)
    }

    /// One accepted head request (#711): a touch for the coordinator to stop
    /// on or note, a tap, and with the durable mute a hold's sleep and a
    /// tap's wake. Sleep is the software mute with the reason `sleep`: it is
    /// revoked and persisted as any mute. A wake only makes the clear
    /// pending; the quiescent owner saves it before the latch opens.
    fn head_event(&mut self, now_us: u64, event: &TouchEvent, observation: &mut Observation) {
        if event.source != TouchSource::Head {
            return;
        }
        let mute = self.options.mute_record.is_some();
        match event.gesture {
            TouchGesture::Touch => earlier(&mut observation.head_touch_at_us, event.observed_us),
            TouchGesture::Hold if mute => {
                match self
                    .receiver
                    .request_sleep(now_us, event.began_us, event.observed_us)
                {
                    Ok(true) => {
                        self.dirty = true;
                        observation.mute_changed = true;
                        earlier(&mut observation.acquired_at_us, event.observed_us);
                    }
                    Ok(false) => {}
                    Err(_) => add(&self.shared.rejected, 1),
                }
            }
            TouchGesture::Tap => {
                earlier(&mut observation.head_tap_at_us, event.observed_us);
                earlier(&mut observation.head_tap_began_us, event.began_us);
                // A mute left by the restart guard wakes by a tap as sleep
                // does (#960); a person's own mute stays until their hold.
                let after_fault = self.restored_after_fault();
                if mute
                    && let Some(released_us) = event.released_us
                    && if after_fault {
                        self.receiver
                            .request_wake_after_fault(now_us, event.began_us, released_us)
                    } else {
                        self.receiver
                            .request_wake(now_us, event.began_us, released_us)
                    }
                    .is_err()
                {
                    add(&self.shared.rejected, 1);
                }
            }
            _ => {}
        }
    }

    /// Discard any queued/previous clear before an active runtime transition.
    /// It never clears software mute or grants a physical privacy permission.
    pub fn fence_clear(&mut self) -> io::Result<()> {
        self.fence_at(monotonic_us());
        Ok(())
    }

    /// Called only after local authority revocation, before a possibly failing
    /// teardown result is finalized. No acknowledgement precedes this save.
    pub fn persist_muted(&mut self) -> io::Result<()> {
        if self.muted() && self.dirty {
            // The person chose this mute: the guard's marker goes before it
            // is saved, never after. A removal that fails still lets the
            // person's mute be saved (its moved `through_us` no longer matches
            // the marker), and the fault is returned after.
            let dropped = self.drop_guard_marker();
            let result = self
                .receiver
                .mute_record()
                .save(self.options.mute_record.as_ref().unwrap());
            if let Err(error) = result {
                self.persistence_failed = true;
                return Err(error);
            }
            self.dirty = false;
            dropped?;
        }
        Ok(())
    }

    /// No audio workers may exist. Saves a mute before considering a fresh
    /// explicit clear, then saves the proposal before releasing the live latch.
    pub fn quiescent(&mut self) -> io::Result<bool> {
        self.quiescent_observation().map(|(allowed, _)| allowed)
    }

    /// The quiescent transaction with its accepted input report preserved.
    /// Muted supervisors use this to report advisory pets without any audio
    /// action. `muted` reflects the final latch after the persistence/clear
    /// transaction; the other fields describe this bounded input poll.
    pub fn quiescent_observation(&mut self) -> io::Result<(bool, Observation)> {
        self.quiescent_observation_with_clock(monotonic_us)
    }

    #[cfg(test)]
    fn quiescent_at(&mut self, now_us: u64) -> io::Result<bool> {
        self.quiescent_with_clock(|| now_us)
    }

    #[cfg(test)]
    fn quiescent_with_clock(&mut self, now: impl FnMut() -> u64) -> io::Result<bool> {
        self.quiescent_observation_with_clock(now)
            .map(|(allowed, _)| allowed)
    }

    fn quiescent_observation_with_clock(
        &mut self,
        mut now: impl FnMut() -> u64,
    ) -> io::Result<(bool, Observation)> {
        let mut observation = self.poll_with_clock(&mut now)?;
        self.persist_muted()?;
        let allowed = self.after_persistence(now)?;
        observation.muted = self.muted();
        observation.asleep = self.asleep();
        self.share_latch();
        Ok((allowed, observation))
    }

    fn after_persistence(&mut self, mut now: impl FnMut() -> u64) -> io::Result<bool> {
        if self.persistence_failed {
            return Err(io::Error::other(
                "hand mute persistence fault requires recovery",
            ));
        }
        // Disk sync may have blocked while the pump read a newer request or
        // lost continuity. Eligibility is checked after that work, not at the
        // earlier poll. Queued input is consumed on the next bounded tick.
        let generation = self.shared.discontinuities.load(Ordering::Acquire);
        if generation != self.seen_generation || generation == u64::MAX {
            self.fence_at(now());
            self.seen_generation = generation;
            return Ok(!self.muted());
        }
        if self.shared.queued.load(Ordering::Acquire) != 0 {
            return Ok(!self.muted());
        }
        if let Some(path) = &self.options.mute_record
            && self.receiver.clear_pending()
        {
            // Expiration is optional input loss, not a Voice service failure.
            if let Ok(prepared) = self.receiver.prepare_clear(now()) {
                let saved = prepared.record().save(path);
                if let Err(error) = saved {
                    self.persistence_failed = true;
                    return Err(error);
                }
                self.receiver.commit_clear(prepared)?;
                self.armed = false;
                self.set_guard_marker(false);
            }
        }
        Ok(!self.muted())
    }

    /// Before opening any audio worker, store the conservative restart guard.
    /// The live preference is unchanged. Do not call from the urgent audio loop.
    pub fn arm_active(&mut self) -> io::Result<()> {
        let Some(path) = self.options.mute_record.clone() else {
            return Ok(());
        };
        if self.muted() || self.persistence_failed {
            return Err(io::Error::other("hand mute inhibits an active runtime"));
        }
        if !self.armed {
            // Marker first: a fault between the two leaves an unmuted record
            // with a marker, which restore discards.
            self.set_guard_marker(true);
            let mut guard = self.receiver.mute_record().clone();
            guard.muted = true;
            if let Err(error) = guard.save(&path) {
                self.persistence_failed = true;
                return Err(error);
            }
            self.armed = true;
        }
        Ok(())
    }

    /// Only after VERIFIED clean destruction of all audio workers. Fault,
    /// physical/software lifecycle exits and interrupted cleanup must keep the
    /// conservative disk guard until the supervisor chooses its safe next step.
    pub fn disarm_after_cleanup(&mut self) -> io::Result<()> {
        let Some(path) = self.options.mute_record.clone() else {
            return Ok(());
        };
        if self.persistence_failed {
            return Err(io::Error::other(
                "hand mute persistence fault requires recovery",
            ));
        }
        if self.armed || self.dirty {
            // The marker goes before the live latch is written: muted, it is a
            // person's mute, which must never read as the guard's.
            self.drop_guard_marker()?;
            if let Err(error) = self.receiver.mute_record().save(&path) {
                self.persistence_failed = true;
                return Err(error);
            }
            self.armed = false;
            self.dirty = false;
        }
        Ok(())
    }
}

/// The `clock_id` and `through_us` of the mute record stored at `path`, read
/// as it is, before any rebase.
fn stored_identity(path: &Path) -> Option<(String, u64)> {
    let record: MuteRecord = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    Some((record.clock_id, record.through_us))
}

/// Writes the marker naming the guard record `identity`, synced with its
/// folder before the guard is saved: a power cut soon after arming must not
/// leave it empty on ext4, where it would no longer count as the guard's
/// (the first builder on #997).
fn write_guard_marker(marker: &Path, identity: &(String, u64)) -> io::Result<()> {
    let text = format!("{}\n{}\n", identity.0, identity.1);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(marker)?;
    std::io::Write::write_all(&mut file, text.as_bytes())?;
    file.sync_all()?;
    if let Some(parent) = marker.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

/// The guard record's identity a marker names: `clock_id`, then `through_us`.
fn marker_identity(text: &str) -> Option<(String, u64)> {
    let mut lines = text.lines();
    let clock_id = lines.next()?.to_owned();
    let through_us = lines.next()?.parse().ok()?;
    (!clock_id.is_empty() && lines.next().is_none()).then_some((clock_id, through_us))
}

fn earlier(value: &mut Option<u64>, next: u64) {
    *value = Some(value.map_or(next, |previous| previous.min(next)));
}

fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(u128::from(u64::MAX)) as u64
        })
}

/// What one subscription carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Topic {
    State,
    Gesture,
    /// The head pads' (`--touch` Controls; #711).
    TouchState,
    Touch,
}

fn parse(message: Message, topic: Topic, now_unix_ms: u64) -> io::Result<Input> {
    let header = &message.header;
    let state = topic == Topic::State;
    let (name, type_name, kind, ttl) = match topic {
        Topic::State => (ControlsState::TOPIC, ControlsState::TYPE, Kind::Latest, 300),
        Topic::Gesture => (
            ControlGesture::TOPIC,
            ControlGesture::TYPE,
            Kind::Event,
            500,
        ),
        Topic::TouchState => (
            TouchState::TOPIC,
            TouchState::TYPE,
            Kind::Latest,
            lamp_msgs::TOUCH_STATE_TTL_US / 1_000,
        ),
        Topic::Touch => (
            TouchEvent::TOPIC,
            TouchEvent::TYPE,
            Kind::Event,
            lamp_msgs::TOUCH_EVENT_TTL_US / 1_000,
        ),
    };
    let topic_name = name;
    if header.topic != topic_name
        || header.source != "controls"
        || header.type_name.as_deref() != Some(type_name)
        || header.kind != kind
        || header.enc != lamp_bus::JSON
        || header.seq == 0
        || header.boot.len() != 16
        || !header.boot.bytes().all(|byte| byte.is_ascii_hexdigit())
        || header.expires_ms != Some(ttl)
        || message.expired
        || header.expired_at(now_unix_ms)
        || header.at_unix_ms > now_unix_ms.saturating_add(lamp_bus::EXPIRY_GRACE_MS)
    {
        return Err(invalid("invalid or expired hand controls envelope"));
    }
    if topic == Topic::TouchState {
        // The payload names its Controls owner; the receiver fences it.
        let payload: TouchState = message.parse()?;
        payload
            .check()
            .map_err(|_| invalid("invalid touch state"))?;
        return Ok(Input::TouchState(Box::new(payload)));
    }
    if topic == Topic::Touch {
        let payload: TouchEvent = message.parse()?;
        payload
            .check()
            .map_err(|_| invalid("invalid touch event"))?;
        return Ok(Input::Touch(Box::new(payload)));
    }
    if state {
        let payload: ControlsState = message.parse()?;
        payload
            .check()
            .map_err(|_| invalid("invalid controls state"))?;
        if payload.session_id != header.boot {
            return Err(invalid(
                "controls state does not name its publisher instance",
            ));
        }
        Ok(Input::State(Box::new(payload)))
    } else {
        let payload: ControlGesture = message.parse()?;
        payload
            .check()
            .map_err(|_| invalid("invalid controls gesture"))?;
        // Commands are only consumed through the cumulative state. The event
        // publisher's boot differs from the state publisher/session identity.
        Ok(Input::Pet(payload))
    }
}

fn enqueue(sender: &SyncSender<Queued>, shared: &Shared, input: Input) -> bool {
    let queued = Queued {
        input,
        generation: shared.discontinuities.load(Ordering::Acquire),
    };
    // Publish the reservation before the item, so a quiescent clear never
    // mistakes a producer in try_send for an empty queue. One producer only.
    shared.queued.fetch_add(1, Ordering::AcqRel);
    match sender.try_send(queued) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) => {
            shared.queued.fetch_sub(1, Ordering::AcqRel);
            add(&shared.lost, 1);
            shared.fence();
            true
        }
        Err(TrySendError::Disconnected(_)) => {
            shared.queued.fetch_sub(1, Ordering::AcqRel);
            false
        }
    }
}

/// What one look at one subscriber found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pumped {
    /// A message came (kept, refused or let go).
    Read,
    Empty,
    /// The coordinator's end of the queue has gone.
    Closed,
}

#[derive(Debug, PartialEq, Eq)]
enum Drained {
    Done,
    Closed,
}

/// One pass of the pump: rounds over every subscriber, up to
/// `POLL_LIMIT / 2` of them so a burst is taken in one pass, but no more
/// after a round that found nothing, since the rest would find nothing too.
/// Idle, that is one look at each subscriber a pass, where it was eight:
/// on lamp-75c9 on 2026-10-17 this thread took 5.4% of a core with nothing
/// happening (#705, 08:37). The bound on a message's wait is unchanged: one
/// that comes just after the round that found nothing is read at the next
/// pass, at most one `PUMP_TICK` (2 ms) later, as before. A head tap's stop
/// comes this way.
fn drain<S, T: Copy>(
    subscribed: &mut [(S, T)],
    mut read: impl FnMut(&mut S, T) -> io::Result<Pumped>,
) -> io::Result<Drained> {
    for _ in 0..POLL_LIMIT / 2 {
        let mut found = false;
        for (subscriber, topic) in subscribed.iter_mut() {
            match read(subscriber, *topic)? {
                Pumped::Read => found = true,
                Pumped::Empty => {}
                Pumped::Closed => return Ok(Drained::Closed),
            }
        }
        if !found {
            break;
        }
    }
    Ok(Drained::Done)
}

fn read_one(
    subscriber: &mut Subscriber,
    topic: Topic,
    sender: &SyncSender<Queued>,
    shared: &Shared,
) -> io::Result<Pumped> {
    let before = (
        subscriber.malformed(),
        subscriber.expired(),
        subscriber.wrong_type(),
    );
    let message = subscriber.recv(Duration::ZERO)?;
    let after = (
        subscriber.malformed(),
        subscriber.expired(),
        subscriber.wrong_type(),
    );
    if before != after {
        shared.reject();
    }
    let found = message.is_some();
    if let Some(message) = message {
        if message.gap != 0 || message.publisher_restarted {
            add(&shared.lost, message.gap);
            shared.fence();
        }
        match parse(message, topic, wall_ms()) {
            Ok(Input::Pet(event)) if event.intent != ControlIntent::Pet => {}
            Ok(input) => {
                add(&shared.received, 1);
                if !enqueue(sender, shared, input) {
                    return Ok(Pumped::Closed);
                }
            }
            Err(_) => shared.reject(),
        }
    }
    for notice in subscriber.notices() {
        if matches!(notice, Notice::SourceGone | Notice::Late) {
            shared.fence();
        }
    }
    Ok(if found { Pumped::Read } else { Pumped::Empty })
}

type Connected = (Vec<(Subscriber, Topic)>, Option<Publisher>);

fn subscriptions(root: &Path, head_touch: bool, say_mute: bool) -> io::Result<Connected> {
    let bus = Bus::new(root)?;
    let mut subscribed = vec![
        (
            bus.subscribe_typed(ControlsState::TOPIC, SUBSCRIBER, ControlsState::TYPE)?,
            Topic::State,
        ),
        (
            bus.subscribe_typed(ControlGesture::TOPIC, SUBSCRIBER, ControlGesture::TYPE)?,
            Topic::Gesture,
        ),
    ];
    if head_touch {
        // State before events, as the touch runtime publishes them.
        subscribed.push((
            bus.subscribe_typed(TouchState::TOPIC, SUBSCRIBER, TouchState::TYPE)?,
            Topic::TouchState,
        ));
        subscribed.push((
            bus.subscribe_typed(TouchEvent::TOPIC, SUBSCRIBER, TouchEvent::TYPE)?,
            Topic::Touch,
        ));
    }
    let mute = say_mute
        .then(|| bus.publisher_declared(VoiceMute::TOPIC, &VoiceMute::declaration(), "voice"))
        .transpose()?;
    Ok((subscribed, mute))
}

/// Says the owner's live latch on `voice/mute` when it changes and every
/// second, so a part started later learns it within one. A failed send
/// reconnects, as a failed read does.
struct SaidMute {
    latch: u8,
    at: Option<std::time::Instant>,
}

impl SaidMute {
    fn say(&mut self, publisher: &mut Publisher, shared: &Shared) -> io::Result<()> {
        let latch = shared.latch.load(Ordering::Acquire);
        let Some(message) = latch_message(latch) else {
            return Ok(());
        };
        if latch == self.latch && self.at.is_some_and(|at| at.elapsed() < MUTE_SAID_EVERY) {
            return Ok(());
        }
        publisher.publish_json(lamp_ipc::monotonic_us(), &message)?;
        self.latch = latch;
        self.at = Some(std::time::Instant::now());
        Ok(())
    }
}

fn pump(
    root: PathBuf,
    head_touch: bool,
    say_mute: bool,
    sender: SyncSender<Queued>,
    shared: &Shared,
) {
    while !shared.stop.load(Ordering::Acquire) {
        shared.fence();
        if let Ok((mut subscribed, mut mute)) = subscriptions(&root, head_touch, say_mute) {
            let mut said = SaidMute {
                latch: LATCH_NONE,
                at: None,
            };
            'connected: while !shared.stop.load(Ordering::Acquire) {
                if let Some(publisher) = &mut mute
                    && said.say(publisher, shared).is_err()
                {
                    break 'connected;
                }
                match drain(&mut subscribed, |subscriber, topic| {
                    read_one(subscriber, topic, &sender, shared)
                }) {
                    Ok(Drained::Done) => {}
                    Ok(Drained::Closed) => return,
                    Err(_) => break 'connected,
                }
                thread::park_timeout(PUMP_TICK);
            }
        }
        if !shared.stop.load(Ordering::Acquire) {
            add(&shared.reconnects, 1);
            shared.fence();
            thread::park_timeout(RECONNECT_AFTER);
        }
    }
}

/// Real head output from Controls' own recognizer (#711), for tests: the
/// head settled, then a hand on it from `touched_us` for 30 ms; the state
/// and events the touch runtime would publish at the end.
#[cfg(test)]
pub(crate) fn test_head_touch(clock_id: &str, touched_us: u64) -> (TouchState, Vec<TouchEvent>) {
    let mut head = TestHead::new(clock_id, touched_us - 600_000);
    let mut ticks = head.run(touched_us - 10_000, false);
    ticks.extend(head.run(touched_us + 30_000, true));
    let events = ticks
        .iter()
        .flat_map(|(_, _, events)| events.clone())
        .collect();
    (ticks.pop().unwrap().1, events)
}

/// Controls' own recognizer for the head, stepped every 10 ms with a hand on
/// or off its pad, for tests (#711).
#[cfg(test)]
pub(crate) struct TestHead {
    engine: lamp_controls::touch::TouchEngine,
    clock_id: String,
    next_us: u64,
    sequence: u64,
}

#[cfg(test)]
impl TestHead {
    pub(crate) fn new(clock_id: &str, from_us: u64) -> Self {
        let config = lamp_controls::touch::TouchConfig {
            enabled: true,
            head_pads: vec!["head-1".into()],
            ..lamp_controls::touch::TouchConfig::default()
        };
        Self {
            engine: lamp_controls::touch::TouchEngine::new(config, clock_id, "controls").unwrap(),
            clock_id: clock_id.into(),
            next_us: from_us,
            sequence: 0,
        }
    }

    /// Each 10 ms step through `until_us`: its time, the state the touch
    /// runtime would publish then and the events it raised.
    pub(crate) fn run(
        &mut self,
        until_us: u64,
        on: bool,
    ) -> Vec<(u64, TouchState, Vec<TouchEvent>)> {
        let mut ticks = Vec::new();
        while self.next_us <= until_us {
            let at = self.next_us;
            self.sequence += 1;
            let sample = lamp_controls::touch::GpioSample {
                stamp: lamp_msgs::ClockStamp {
                    clock_id: self.clock_id.clone(),
                    at_us: at,
                    sequence: self.sequence,
                },
                session_id: "gpio".into(),
                primary: None,
                reset: None,
                head_pads: vec![lamp_controls::input::Level {
                    name: "head-1".into(),
                    active: on,
                }],
            };
            self.engine.ingest_gpio(at, sample).unwrap();
            let output = self.engine.tick(at);
            ticks.push((at, output.state, output.events));
            self.next_us += 10_000;
        }
        ticks
    }
}

#[cfg(test)]
mod tests;
