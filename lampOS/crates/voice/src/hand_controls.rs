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
//!
//! A guard from an earlier boot is a power cut, not a fault of this run
//! (#4332): pulling the plug is how most people turn a lamp off. That restart
//! starts with the person's own mute only ([`released_after_power_cut`]).
use lamp_bus::{Bus, Kind, Message, Notice, Publisher, Subscriber};
use lamp_controls::{
    mute::MuteRecord,
    receiver::Receiver,
    touch_receiver::{LateSleep, TouchReceiver},
};
use lamp_msgs::{
    ControlGesture, ControlIntent, ControlsState, MuteReason, MutedBy, TouchEvent, TouchGesture,
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
/// With [`ControlsOptions::wait`], the longest the pump waits on its
/// sockets: the default bus rescan (`Bus::rescan_every`, 200 ms, the
/// product's), so the topics' files and publishers are looked at about as
/// often as they would be. A bus set to rescan faster (tests use 20 ms) is
/// still looked at every 200 ms here.
const WAIT_AT_MOST: Duration = Duration::from_millis(200);
const RECONNECT_AFTER: Duration = Duration::from_millis(250);
const FINAL_JOIN_WAIT: Duration = Duration::from_millis(20);
/// `voice/mute` is said when it changes and at least this often.
const MUTE_SAID_EVERY: Duration = Duration::from_secs(1);
const SUBSCRIBER: &str = "voice_hand";
/// Off unless `on`: keep the restart guard's marker, so a restored mute can be
/// told from a chosen one (#960).
pub const RESTORED_MUTE_ENVIRONMENT: &str = "LAMP_RESTORED_MUTE";
const GUARD_MARKER_SUFFIX: &str = ".restart-guard";
/// Off unless `on`: a run that fails before it heard anything, with no
/// control input in flight, does not leave the lamp muted (#1567, B), and a
/// fault raised after the stop's signal is the stop's (A).
pub const NEVER_LISTENED_ENVIRONMENT: &str = "LAMP_GUARD_NEVER_LISTENED";
/// Off unless `on` (#3748): a hand's hold on the head says the recorded
/// line "Going to sleep. Tap my head to wake me." in place of the falling
/// breath, and its sleep waits for that cue: the Hold at 3 s makes the sleep
/// owed, and the mute comes once the cue has played whole
/// ([`Self::sleep_cue_sounding`]), or at [`SLEEP_OWED_CAP_US`]. A hand let go
/// before the Hold cuts the cue ([`Observation::head_lifted`]).
pub const SLEEP_LINE_ENVIRONMENT: &str = "LAMP_SLEEP_LINE";
/// The latest a hold's owed sleep waits for its cue, from the Hold: the
/// line runs about 2.5 s from the hold's 2 s mark.
pub const SLEEP_OWED_CAP_US: u64 = 6_000_000;
/// Beside the mute record: how many runs in a row have failed before they
/// heard anything, for the health to say (#1567).
const NEVER_LISTENED_SUFFIX: &str = ".never-listened";

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
    /// The pump waits on its sockets instead of looking every 2 ms
    /// (`LAMP_CONTROLS_WAIT=on`, #705, the lamp's heat): a request is read
    /// as it arrives, and with nothing coming the pump wakes about five
    /// times a second instead of 500. Off unless set.
    pub wait: bool,
}

/// `on` for the pump to wait on its sockets ([`ControlsOptions::wait`]).
pub const WAIT_ENVIRONMENT: &str = "LAMP_CONTROLS_WAIT";

/// Whether the environment says `LAMP_CONTROLS_WAIT=on`.
pub fn wait_from_environment() -> bool {
    std::env::var(WAIT_ENVIRONMENT).ok().as_deref() == Some("on")
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
    /// A hand on the head now, as the newest touch state taken in this poll
    /// says: its contact and how long it has been there (Controls' own
    /// `held_us`), for the breath in a hold's last second (#1362). None
    /// with no hand, or one Controls gives no time.
    pub head_held: Option<(u64, u64)>,
    /// With `LAMP_SLEEP_LINE=on` and no sleep owed (#3748): the newest touch
    /// state taken in this poll says the hand has left the head (its contact
    /// ended, past the pads' short gaps), so a sleep cue still sounding is
    /// cut. Always false with the switch off.
    pub head_lifted: bool,
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
    /// Held by the pump while it queues an input, and by a release from its
    /// last look for an input in flight through its save (#1607, Jarvis on
    /// #1592): no press can be queued between the look and the write.
    quiet: std::sync::Mutex<()>,
    /// A press found [`Self::quiet`] held, and waited: it came while a
    /// release looked and saved, so the release keeps the guard (the
    /// reviewer and the first builder on #1622). Only an input that could
    /// carry a press ([`carries_press`]).
    pressed_while_held: AtomicBool,
    /// Inputs that could carry a press, queued and not yet taken: what a
    /// release's look counts, not the routine states queued with them.
    pressing: AtomicU64,
    /// The session and `event_sequence` of the last Controls state and touch
    /// state queued: one whose events moved on, or from a new session (whose
    /// count starts again), could carry a press (Fourth on #1632).
    controls_seen: std::sync::Mutex<Option<(String, u64)>>,
    touch_seen: std::sync::Mutex<Option<(String, u64)>>,
    /// Inputs that found [`Self::quiet`] held, whatever they were: a test
    /// waits on it.
    #[cfg(test)]
    waited: AtomicU64,
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
    /// With [`ControlsOptions::wait`], the pipe's end the owner writes a
    /// byte to, to wake the pump from its wait at once: for a stop, and for
    /// a latch to be said.
    wake: std::sync::OnceLock<std::os::fd::OwnedFd>,
    /// The pump's passes: how often it woke.
    passes: AtomicU64,
    /// Completed off-mode park calls, observed only by this pump's tests.
    #[cfg(test)]
    tick_waits: TickWaits,
    /// First subscribed drain completed; only the owning fixture reads this.
    #[cfg(test)]
    pump_ready: AtomicBool,
}

#[cfg(test)]
struct TickWaits {
    count: AtomicU64,
    shortest_ns: AtomicU64,
    longest_ns: AtomicU64,
}

#[cfg(test)]
impl Default for TickWaits {
    fn default() -> Self {
        Self {
            count: AtomicU64::new(0),
            shortest_ns: AtomicU64::new(u64::MAX),
            longest_ns: AtomicU64::new(0),
        }
    }
}

#[cfg(test)]
impl TickWaits {
    fn observe(&self, duration: Duration) {
        let ns = u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);
        self.shortest_ns.fetch_min(ns, Ordering::Relaxed);
        self.longest_ns.fetch_max(ns, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Release);
    }
}

/// How long a wake from a hand's sleep waits for the conversation it opens
/// ([`HandControls::take_woken`]): a lamp opens in well under this.
pub const WOKEN_FRESH_US: u64 = 2_000_000;

const LATCH_NONE: u8 = 0;
const LATCH_OPEN: u8 = 1;
const LATCH_MUTED: u8 = 2;
const LATCH_ASLEEP: u8 = 3;
/// Muted by the restart guard after a fault ([`HandControls::restored_after_fault`]).
const LATCH_GUARD: u8 = 4;
/// Muted because the record could not be trusted ([`HandControls::unreadable`]).
const LATCH_UNREADABLE: u8 = 5;

/// What the pump says on `voice/mute` for a latch, with who or what muted
/// it (`muted_by`, #4332): the mute it always said is the person's hold.
fn latch_message(latch: u8) -> Option<VoiceMute> {
    let muted = |muted_by| {
        Some(VoiceMute {
            muted: true,
            reason: None,
            muted_by: Some(muted_by),
        })
    };
    match latch {
        LATCH_OPEN => Some(VoiceMute {
            muted: false,
            reason: None,
            muted_by: None,
        }),
        LATCH_MUTED => muted(MutedBy::Button),
        LATCH_GUARD => muted(MutedBy::RestartGuard),
        LATCH_UNREADABLE => muted(MutedBy::Unreadable),
        LATCH_ASLEEP => Some(VoiceMute {
            muted: true,
            reason: Some(MuteReason::Sleep),
            muted_by: Some(MutedBy::Sleep),
        }),
        _ => None,
    }
}

/// A hand on the head in a touch state: its contact and Controls' time
/// for it, while it is there (#1362).
fn head_held(state: &TouchState) -> Option<(u64, u64)> {
    let head = state
        .sources
        .iter()
        .find(|source| source.source == TouchSource::Head)?;
    match (head.active, head.contact_id, head.held_us) {
        (Some(true), Some(contact), Some(held)) => Some((contact, held)),
        _ => None,
    }
}

/// The hand has left the head in a touch state: the head read, and no
/// contact on it, past the pads' short gaps (#3748).
fn head_lifted(state: &TouchState) -> bool {
    state.sources.iter().any(|source| {
        source.source == TouchSource::Head
            && source.ready
            && source.active == Some(false)
            && source.contact_id.is_none()
    })
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

    /// Wake a waiting pump at once; nothing without the switch. A full
    /// pipe already wakes it.
    fn wake(&self) {
        if let Some(wake) = self.wake.get() {
            let _ = rustix::io::write(wake, &[1]);
        }
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
    /// Could carry a press ([`carries_press`]), counted in
    /// [`Shared::pressing`] until taken.
    press: bool,
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
        self.shared.wake();
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

/// The sending end of [`HandControls::with_test_sender`].
#[cfg(test)]
pub(crate) struct TestControls {
    sender: SyncSender<Queued>,
    shared: Arc<Shared>,
}

#[cfg(test)]
impl TestControls {
    /// Queues one Controls state, as the bus pump does.
    pub(crate) fn send(&self, state: ControlsState) {
        assert!(enqueue(
            &self.sender,
            &self.shared,
            Input::State(Box::new(state))
        ));
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
    /// Whether this run's capture has delivered a block since it was armed
    /// ([`Self::heard_audio`], #1567).
    heard: bool,
    persistence_failed: bool,
    /// The head pads' requests, checked as Controls' own receiver does:
    /// fresh, from the current Controls and reader, each once.
    touch: Option<TouchReceiver>,
    /// `LAMP_RESTORED_MUTE=on`: arming leaves the guard's marker.
    mark_guard: bool,
    /// `LAMP_GUARD_NEVER_LISTENED=on` (#1567).
    never_listened: bool,
    /// Runs in a row that failed before they heard anything, since the last
    /// that did ([`Self::could_not_start`]).
    could_not_start: u32,
    /// The record was muted by the restart guard, not by the person.
    restored_after_fault: bool,
    /// The record could not be trusted at the start and was read as muted
    /// (#1505's newer build, #4378's unfinished write): `load_noting`'s
    /// note, kept to be said as `muted_by` (#4332).
    restored_unreadable: bool,
    /// The last run closed, its cleanup verified, on a privacy switch that
    /// could not be read ([`Self::closed_on_unreadable_switch`]).
    closed_unreadable: bool,
    /// The late sleep window ([`Self::allow_late_sleep`]), given to the head
    /// pads' receiver once the first Controls state is applied.
    late_sleep: Option<LateSleep>,
    /// A sleep asked aloud, for the next poll ([`Self::ask_sleep`]).
    sleep_asked: bool,
    /// `LAMP_SLEEP_LINE=on` ([`SLEEP_LINE_ENVIRONMENT`], #3748).
    sleep_line: bool,
    /// A hold's sleep owed until its cue has played: when the Hold was
    /// taken, on the poll's clock. Only with [`Self::sleep_line`].
    hold_owed: Option<u64>,
    /// The coordinator said, before this poll, that the hold's cue is
    /// sounding ([`Self::sleep_cue_sounding`]); each poll takes it.
    cue_sounding: bool,
    /// When the cue was last said to be sounding: its end, for the journal.
    cue_seen_us: Option<u64>,
    /// When the conversation last read the press queue ([`Self::poll`]), on
    /// the wall clock: a clean stop's floor for the next start's late sleep
    /// ([`Self::disarm_after_clean_stop`]).
    polled_at: Option<SystemTime>,
    /// Run while a release holds the press queue, after its look and before
    /// its save (a test presses then).
    #[cfg(test)]
    while_held: Option<Box<dyn FnOnce() + Send>>,
    /// The person woke a hand's sleep (#1362): the acquisition of the input
    /// that did, until the next conversation takes it for its "hm?"
    /// ([`Self::take_woken`]).
    woken: Option<u64>,
    /// The onboarding walk is under way ([`Self::set_onboarding`]): a hold
    /// on the head asks no sleep.
    onboarding: bool,
    /// Inputs a test has queue later, each when the poll's clock reaches
    /// its time, through the sender kept here.
    #[cfg(test)]
    scheduled: Vec<(u64, Input)>,
    #[cfg(test)]
    scheduler: Option<SyncSender<Queued>>,
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

    /// As [`Self::with_test_state`], with nothing queued yet: the test
    /// sends Controls' states through the returned end when it chooses,
    /// as the bus pump queues them.
    #[cfg(test)]
    pub(crate) fn with_test_sender(
        clock_id: &str,
        started_us: u64,
    ) -> io::Result<(Self, TestControls)> {
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
            Arc::clone(&shared),
            started_us,
        )?;
        Ok((owner, TestControls { sender, shared }))
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

    /// As [`Self::with_test_asleep`], muted by the person, not asleep: a
    /// plain mute, with head touch and its durable mute at `record`.
    #[cfg(test)]
    pub(crate) fn with_test_muted(
        clock_id: &str,
        started_us: u64,
        record: &Path,
    ) -> io::Result<Self> {
        let (_sender, input) = mpsc::sync_channel(QUEUE_CAPACITY);
        let mut muted = MuteRecord::new(clock_id)?;
        assert!(muted.request_mute(started_us / 2));
        Self::from_parts(
            ControlsOptions {
                head_touch: true,
                mute_record: Some(record.to_path_buf()),
                ..Default::default()
            },
            clock_id,
            muted,
            input,
            Arc::new(Shared::default()),
            started_us,
        )
    }

    /// A hand on the head since `now_us - held_us` (#1362), lifted when it
    /// has been there `lifted_us` (3.2 s or more: kept past the sleep), with
    /// head sleep and its durable mute at `record` (in a private
    /// directory): every step before `now_us` taken now but the last, which
    /// waits for the first poll, and every later one, to 0.4 s after the
    /// lift, queued when the poll's clock reaches it. So the first poll sees
    /// the hand `held_us` in, and Controls' sleep, if the hand stays, comes
    /// on the poll that reaches its own time.
    #[cfg(test)]
    pub(crate) fn with_test_hold(
        clock_id: &str,
        now_us: u64,
        held_us: u64,
        lifted_us: u64,
        record: &Path,
    ) -> io::Result<Self> {
        let (sender, input) = mpsc::sync_channel(QUEUE_CAPACITY);
        let began_us = now_us - held_us;
        let mut owner = Self::from_parts(
            ControlsOptions {
                head_touch: true,
                mute_record: Some(record.to_path_buf()),
                ..Default::default()
            },
            clock_id,
            MuteRecord::new(clock_id)?,
            input,
            Arc::new(Shared::default()),
            began_us - 1_000_000,
        )?;
        let mut head = TestHead::new(clock_id, began_us - 900_000);
        let mut ticks = head.run(began_us - 10_000, false);
        ticks.extend(head.run(began_us + lifted_us, true));
        ticks.extend(head.run(began_us + lifted_us + 400_000, false));
        let later = ticks.split_off(ticks.partition_point(|(at, _, _)| *at <= now_us));
        let (first, earlier) = ticks.split_last().expect("steps before now");
        for (at, state, events) in earlier {
            enqueue(
                &sender,
                &owner.shared,
                Input::TouchState(Box::new(state.clone())),
            );
            for event in events {
                enqueue(
                    &sender,
                    &owner.shared,
                    Input::Touch(Box::new(event.clone())),
                );
            }
            owner.poll_with_clock(|| *at)?;
        }
        let (_, state, events) = first;
        enqueue(
            &sender,
            &owner.shared,
            Input::TouchState(Box::new(state.clone())),
        );
        for event in events {
            enqueue(
                &sender,
                &owner.shared,
                Input::Touch(Box::new(event.clone())),
            );
        }
        for (at, state, events) in later {
            owner
                .scheduled
                .push((at, Input::TouchState(Box::new(state))));
            for event in events {
                owner.scheduled.push((at, Input::Touch(Box::new(event))));
            }
        }
        owner.scheduler = Some(sender);
        Ok(owner)
    }

    /// A lamp the person woke from a hand's sleep at `woken_us` (#1362),
    /// as the muted wait leaves it for the conversation the wake opens.
    #[cfg(test)]
    pub(crate) fn with_test_woken(clock_id: &str, woken_us: u64) -> io::Result<Self> {
        let (_sender, input) = mpsc::sync_channel(QUEUE_CAPACITY);
        let mut owner = Self::from_parts(
            ControlsOptions {
                head_touch: true,
                ..Default::default()
            },
            clock_id,
            MuteRecord::new(clock_id)?,
            input,
            Arc::new(Shared::default()),
            woken_us,
        )?;
        owner.woken = Some(woken_us);
        std::mem::forget(_sender);
        Ok(owner)
    }

    /// Retain a pending wake alongside another fixture's scheduled inputs.
    #[cfg(test)]
    pub(crate) fn set_woken_for_test(&mut self, woken_us: u64) {
        self.woken = Some(woken_us);
    }

    /// Queues each scheduled input whose time the poll's clock has reached.
    #[cfg(test)]
    fn release_scheduled(&mut self, now_us: u64) {
        let Some(sender) = self.scheduler.as_ref() else {
            return;
        };
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.scheduled)
            .into_iter()
            .partition(|(at, _)| *at <= now_us);
        self.scheduled = later;
        for (_, input) in due {
            enqueue(sender, &self.shared, input);
        }
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

    /// A listening lamp with its mute record at `record` (in a private
    /// directory) and `LAMP_GUARD_NEVER_LISTENED=on` (#1567).
    #[cfg(test)]
    pub(crate) fn with_test_guard(
        clock_id: &str,
        started_us: u64,
        record: &Path,
    ) -> io::Result<Self> {
        let (_sender, input) = mpsc::sync_channel(QUEUE_CAPACITY);
        let mut controls = Self::from_parts(
            ControlsOptions {
                mute_record: Some(record.to_path_buf()),
                ..Default::default()
            },
            clock_id,
            MuteRecord::new(clock_id)?,
            input,
            Arc::new(Shared::default()),
            started_us,
        )?;
        controls.never_listened = true;
        Ok(controls)
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
        let restored = match &options.mute_record {
            Some(path) => restored_record(path, &clock_id, now_us)?,
            None => Restored {
                record: MuteRecord::new(&clock_id)?,
                stored: None,
                late_sleep: None,
                unreadable: false,
            },
        };
        let (sender, input) = mpsc::sync_channel(QUEUE_CAPACITY);
        let shared = Arc::new(Shared::default());
        let mut controls =
            Self::with_restored(options, &clock_id, restored, input, shared, now_us)?;
        controls.start_pump(sender)?;
        Ok(Some(controls))
    }

    /// The one pump, on its own thread: the bus's subscriptions into
    /// `sender`, and with [`ControlsOptions::wait`] the pipe it is woken by.
    fn start_pump(&mut self, sender: SyncSender<Queued>) -> io::Result<()> {
        let options = &self.options;
        let root = options
            .bus_root
            .clone()
            .or_else(|| std::env::var_os(lamp_bus::ROOT_VARIABLE).map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(lamp_bus::DEFAULT_ROOT));
        let pump_shared = Arc::clone(&self.shared);
        let woken = if options.wait {
            let (read, write) = wake_pipe()?;
            let _ = self.shared.wake.set(write);
            Some(read)
        } else {
            None
        };
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
                pump(root, head_touch, say_mute, woken, sender, &pump_shared);
            })?;
        self._pump = Some(Pump {
            thread: Some(worker),
            finished: completion,
            shared: Arc::clone(&self.shared),
        });
        Ok(())
    }

    /// The controls from what [`restored_record`] read: the switches, the
    /// count of runs that could not start, the late sleep window and the
    /// guard's marker. The pump is [`Self::start_pump`]'s.
    fn with_restored(
        options: &ControlsOptions,
        clock_id: &str,
        restored: Restored,
        input: ChannelReceiver<Queued>,
        shared: Arc<Shared>,
        now_us: u64,
    ) -> io::Result<Self> {
        let mut restored = restored;
        let mark_guard = std::env::var(RESTORED_MUTE_ENVIRONMENT).ok().as_deref() == Some("on");
        if let Some(path) = &options.mute_record
            && released_after_power_cut(path, mark_guard, &mut restored)?
        {
            eprintln!(
                "lamp-voice: the restart guard was left by an earlier boot (a power cut), so the lamp starts with its person's own mute: listening"
            );
        }
        let mut controls = Self::from_parts(
            options.clone(),
            clock_id,
            restored.record,
            input,
            shared,
            now_us,
        )?;
        controls.mark_guard = mark_guard;
        controls.never_listened =
            std::env::var(NEVER_LISTENED_ENVIRONMENT).ok().as_deref() == Some("on");
        controls.sleep_line = std::env::var(SLEEP_LINE_ENVIRONMENT).ok().as_deref() == Some("on");
        controls.load_could_not_start();
        // A hand held on the head while no voice ran is still taken (#1607).
        if let Some(window) = restored.late_sleep {
            controls.allow_late_sleep(window);
        }
        controls.restore_guard_marker(restored.stored);
        controls.restored_unreadable = restored.unreadable;
        controls.share_latch();
        Ok(controls)
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
            heard: false,
            persistence_failed: false,
            touch,
            mark_guard: false,
            never_listened: false,
            could_not_start: 0,
            restored_after_fault: false,
            restored_unreadable: false,
            closed_unreadable: false,
            late_sleep: None,
            sleep_asked: false,
            sleep_line: false,
            hold_owed: None,
            cue_sounding: false,
            cue_seen_us: None,
            polled_at: None,
            #[cfg(test)]
            while_held: None,
            woken: None,
            onboarding: false,
            #[cfg(test)]
            scheduled: Vec::new(),
            #[cfg(test)]
            scheduler: None,
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
        } else if self.restored_after_fault() {
            LATCH_GUARD
        } else if self.unreadable() {
            LATCH_UNREADABLE
        } else {
            LATCH_MUTED
        };
        // Woken only for a latch that changed: the owner shares it on every
        // poll, and the pump says an unchanged one every second by itself
        // (the reviewer on #1456: a wake each poll kept a waiting pump at
        // the coordinator's pace with head sleep on).
        if self.shared.latch.swap(latch, Ordering::AcqRel) != latch {
            self.shared.wake();
        }
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

    /// A hand held on the head may put the lamp to sleep: head touch with
    /// the durable mute (#711).
    pub fn head_sleep(&self) -> bool {
        self.options.head_sleep()
    }

    /// Whether the onboarding walk is under way, from its start until every
    /// step has ended, as the coordinator last said before a poll (#3747,
    /// the lead on #1711). Kept until said again: between conversations
    /// there is no walk to ask. False until first said.
    pub fn set_onboarding(&mut self, under_way: bool) {
        self.onboarding = under_way;
    }

    /// The person asked aloud for the lamp to sleep (`go_to_sleep`,
    /// #3748): the next [`Self::poll`] asks the same sleep a hand's hold
    /// does, unless onboarding is under way.
    pub fn ask_sleep(&mut self) {
        self.sleep_asked = true;
    }

    /// `LAMP_SLEEP_LINE=on` (#3748): a hand's hold says the recorded sleep
    /// line, and its sleep waits for the cue.
    pub fn sleep_line(&self) -> bool {
        self.sleep_line
    }

    /// A loop test's `LAMP_SLEEP_LINE`, without the process environment.
    #[cfg(test)]
    pub(crate) fn set_sleep_line(&mut self, on: bool) {
        self.sleep_line = on;
    }

    /// The hold's sleep cue (the line or the breath) is sounding now, at
    /// `now_us` (#3748): said by the coordinator before each poll while it
    /// is, so an owed sleep waits for it. A poll it was not said before
    /// takes the cue as over.
    pub fn sleep_cue_sounding(&mut self, now_us: u64) {
        self.cue_sounding = true;
        self.cue_seen_us = Some(now_us);
    }

    /// What [`Self::set_onboarding`] last said.
    pub fn onboarding(&self) -> bool {
        self.onboarding
    }

    /// When the person woke a hand's sleep, if they did within
    /// [`WOKEN_FRESH_US`] of `now_us`: once, for the conversation the wake
    /// opened to say its "hm?" (#1362). A wake older than that, held back
    /// by a closed slide, is let go: the hum belongs to the wake.
    pub fn take_woken(&mut self, now_us: u64) -> Option<u64> {
        self.woken
            .take()
            .filter(|at| now_us.saturating_sub(*at) < WOKEN_FRESH_US)
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

    /// The live mute is the one read from a record the voice could not
    /// trust at its start (#1505, #4378), and the person has neither chosen
    /// a mute since nor cleared it: cleared where the guard's is.
    pub fn unreadable(&self) -> bool {
        self.restored_unreadable && self.muted()
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
    /// Only a guard of this boot gets here matched: one from an earlier boot
    /// was released before ([`released_after_power_cut`], #4332), so the
    /// record is not rebased and the marker still names it as saved.
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
            self.restored_unreadable = false;
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
        self.restored_unreadable = false;
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
        #[cfg(test)]
        self.release_scheduled(now_us);
        self.polled_at = Some(SystemTime::now());
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
            if queued.press {
                self.shared.pressing.fetch_sub(1, Ordering::AcqRel);
            }
            consumed += 1;
            let now_us = now();
            if queued.generation != generation {
                self.fence_at(now_us);
            }
            match queued.input {
                Input::State(state) => match self.receiver.observe(now_us, &state) {
                    Ok(effects) => {
                        // The late sleep only once Controls' own state is
                        // in, so a button's mute released in the same gap
                        // is taken first and never refused behind the
                        // hold's watermark (the reviewer on #1622).
                        if let (Some(window), Some(touch)) =
                            (self.late_sleep.take(), self.touch.as_mut())
                        {
                            touch.allow_late_sleep(window);
                        }
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
                        Some(Ok(events)) => {
                            observation.head_held = head_held(&state);
                            observation.head_lifted =
                                self.sleep_line && self.hold_owed.is_none() && head_lifted(&state);
                            events
                                .iter()
                                .for_each(|event| self.head_event(now_us, event, &mut observation));
                        }
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
        // A sleep asked aloud (#3748): the same request a hand's hold makes,
        // begun and asked now, and never while the walk is under way. Only
        // with head sleep: a tap on the head is how it is woken.
        if std::mem::take(&mut self.sleep_asked) && self.options.head_sleep() && !self.onboarding {
            match self.receiver.request_sleep(now_us, now_us, now_us) {
                Ok(true) => {
                    self.dirty = true;
                    observation.mute_changed = true;
                    earlier(&mut observation.acquired_at_us, now_us);
                }
                Ok(false) => {}
                Err(_) => add(&self.shared.rejected, 1),
            }
        }
        self.land_owed_sleep(now_us, &mut observation);
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

    /// A hold's owed sleep (`LAMP_SLEEP_LINE`, #3748), asked once its cue
    /// is no longer sounding, or at [`SLEEP_OWED_CAP_US`] from the Hold. A
    /// tap or new speech meanwhile does not cancel it: the person held the
    /// head past three seconds. Never while the walk is under way.
    fn land_owed_sleep(&mut self, now_us: u64, observation: &mut Observation) {
        let sounding = std::mem::take(&mut self.cue_sounding);
        let Some(held_at) = self.hold_owed else {
            return;
        };
        let capped = now_us >= held_at.saturating_add(SLEEP_OWED_CAP_US);
        if sounding && !capped {
            return;
        }
        self.hold_owed = None;
        let cue_seen = self.cue_seen_us.take();
        if self.onboarding {
            eprintln!(
                "lamp-voice: a hand's hold: its owed sleep is dropped: onboarding is under way"
            );
            return;
        }
        match self.receiver.request_sleep(now_us, now_us, now_us) {
            Ok(true) => {
                self.dirty = true;
                observation.mute_changed = true;
                earlier(&mut observation.acquired_at_us, now_us);
                let after_hold = (now_us - held_at) / 1_000;
                if capped {
                    eprintln!(
                        "lamp-voice: a hand's hold: its owed sleep lands at the cap, {after_hold} ms after the hold, its cue still sounding"
                    );
                } else if let Some(seen) = cue_seen.filter(|seen| *seen >= held_at) {
                    eprintln!(
                        "lamp-voice: a hand's hold: its owed sleep lands {} ms after its cue ended ({after_hold} ms after the hold)",
                        now_us.saturating_sub(seen) / 1_000
                    );
                } else {
                    eprintln!(
                        "lamp-voice: a hand's hold: its owed sleep lands {after_hold} ms after the hold, no cue sounding then"
                    );
                }
            }
            Ok(false) => {}
            Err(_) => add(&self.shared.rejected, 1),
        }
    }

    /// One accepted head request (#711): a touch for the coordinator to stop
    /// on or note, a tap, and with the durable mute a hold's sleep and the
    /// wake of a tap or a hand kept a second (#3747). Sleep is the software
    /// mute with the reason `sleep`: it is revoked and persisted as any mute.
    /// A wake only makes the clear pending; the quiescent owner saves it
    /// before the latch opens.
    fn head_event(&mut self, now_us: u64, event: &TouchEvent, observation: &mut Observation) {
        if event.source != TouchSource::Head {
            return;
        }
        let mute = self.options.mute_record.is_some();
        match event.gesture {
            TouchGesture::Touch => earlier(&mut observation.head_touch_at_us, event.observed_us),
            // Not while the walk is under way (#3747, the lead on #1711):
            // giving way tells the person to hold the head, and the hold
            // already counted as the touch at 20 ms. The latch stays open,
            // and is said so, so Expression doesn't fold either.
            // With `LAMP_SLEEP_LINE=on` (#3748) the hold's sleep is owed
            // until its cue has played ([`Self::land_owed_sleep`]).
            TouchGesture::Hold if mute && !self.onboarding && self.sleep_line => {
                if self.hold_owed.is_none() {
                    self.hold_owed = Some(now_us);
                }
            }
            TouchGesture::Hold if mute && !self.onboarding => {
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
                if let Some(released_us) = event.released_us {
                    self.head_wake(now_us, event.began_us, released_us);
                }
            }
            // A hand kept a second wakes a sleeping lamp at the stroke, as a
            // tap does at its letting go (#3747): no meaning by how long a
            // hand stays, so a person who holds it to bring it back does.
            // Its time is the wake's, for the "hm?" (#1362).
            // Controls' Stroke keeps the form older readers take (Third's P2
            // on #3767), so the wake is this reader's own rule: the receiver
            // wakes only a lamp asleep when the contact began.
            TouchGesture::Stroke if self.head_wake(now_us, event.began_us, event.observed_us) => {
                earlier(&mut observation.acquired_at_us, event.observed_us);
            }
            _ => {}
        }
    }

    /// A head tap's or kept hand's wake, asked at `at_us`: of a hand's sleep,
    /// and of a mute left by the restart guard as sleep is (#960); a
    /// person's own mute stays until their hold. Whether it is pending.
    fn head_wake(&mut self, now_us: u64, began_us: u64, at_us: u64) -> bool {
        if self.options.mute_record.is_none() {
            return false;
        }
        let asked = if self.restored_after_fault() {
            self.receiver
                .request_wake_after_fault(now_us, began_us, at_us)
        } else {
            self.receiver.request_wake(now_us, began_us, at_us)
        };
        asked.unwrap_or_else(|_| {
            add(&self.shared.rejected, 1);
            false
        })
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
        let was_asleep = self.asleep();
        let mut observation = self.poll_with_clock(&mut now)?;
        self.persist_muted()?;
        let allowed = self.after_persistence(now)?;
        observation.muted = self.muted();
        observation.asleep = self.asleep();
        // Open again after a hand's sleep, by the person's tap or button:
        // the next conversation says its "hm?" (#1362).
        if was_asleep && allowed && !observation.muted {
            self.woken = observation
                .head_tap_at_us
                .or(observation.acquired_at_us)
                .or(observation.observed_at_us);
        }
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
        // A new run: nothing heard in it yet (#1567).
        self.heard = false;
        if !self.armed {
            self.arm_guard_at(&path)?;
        }
        Ok(())
    }

    /// The restart guard stored again (a press came while a release saved).
    fn arm_guard(&mut self) -> io::Result<()> {
        match self.options.mute_record.clone() {
            Some(path) => self.arm_guard_at(&path),
            None => Ok(()),
        }
    }

    fn arm_guard_at(&mut self, path: &Path) -> io::Result<()> {
        // Marker first: a fault between the two leaves an unmuted record
        // with a marker, which restore discards.
        self.set_guard_marker(true);
        let mut guard = self.receiver.mute_record().clone();
        guard.muted = true;
        if let Err(error) = guard.save(path) {
            self.persistence_failed = true;
            return Err(error);
        }
        self.armed = true;
        Ok(())
    }

    /// Whether the last run closed, its cleanup verified, on a privacy switch
    /// that could not be read, set for each run as it ends: a run that ends
    /// any other way, or a cycle refused before its run, says false.
    pub fn closed_on_unreadable_switch(&mut self, closed: bool) {
        self.closed_unreadable = closed;
    }

    /// The guard released when the service stops while it waits after a run
    /// that closed on an unreadable switch, as the latch was, only with
    /// nothing in flight (as [`Self::release_if_still_quiet`]). An update
    /// restarts lamp-controls before the voice, so the old voice reads the
    /// switch as gone half a second before its own stop (lamp-06c1, 29 Oct,
    /// 08:33:16): that is the stop's, not a crash. The switch is read again
    /// at every start. Whether it released.
    pub fn release_after_unreadable_stop(&mut self) -> io::Result<bool> {
        if !self.closed_unreadable || !self.armed || self.persistence_failed {
            return Ok(false);
        }
        self.release_if_still_quiet()
    }

    /// This run's capture has delivered audio: from now on a fault keeps the
    /// guard (#1567), and the count of runs that could not start is over.
    pub fn heard_audio(&mut self) {
        if self.heard {
            return;
        }
        self.heard = true;
        if self.could_not_start != 0 {
            self.could_not_start = 0;
            if let Some(path) = self
                .never_listened_path()
                .filter(|path| regular_or_absent(path))
            {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    /// No control input sent and not yet read (#1567): a press still in the
    /// queue could be a mute the latch has not taken.
    /// No input that could carry a press is queued and not yet taken
    /// ([`carries_press`]): the routine Controls and touch states, each
    /// republished every 100 ms with nothing new, and pets do not count, or
    /// any teardown longer than a republish would keep the guard with
    /// nobody having pressed (Fourth and the reviewer on #1632).
    pub fn nothing_in_flight(&self) -> bool {
        self.shared.pressing.load(Ordering::Acquire) == 0
    }

    /// The guard released, as the latch was, only if no control is in flight
    /// or taken unsaved; the press queue is held from that look through the
    /// save, so none can be queued between them and written over (#1607,
    /// Jarvis on #1592). A press that comes meanwhile is queued after the
    /// save, and the next start takes it: a button's mute by its watermark,
    /// a head's sleep by [`Self::allow_late_sleep`]. Whether it released.
    ///
    /// A press that comes while it looks and saves waits on the queue, and
    /// says it waited: the guard is then put back, so the next start is
    /// muted whatever the press was (the reviewer and the first builder on
    /// #1622). The record's time is set to the look, the next voice's floor
    /// for a hold made after it ([`restored_record`]).
    pub fn release_if_still_quiet(&mut self) -> io::Result<bool> {
        let shared = Arc::clone(&self.shared);
        let _quiet = shared
            .quiet
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        shared.pressed_while_held.store(false, Ordering::Release);
        if !self.still_quiet() {
            return Ok(false);
        }
        let looked = SystemTime::now();
        #[cfg(test)]
        if let Some(press) = self.while_held.take() {
            press();
        }
        self.disarm_after_cleanup()?;
        let pressed = shared.pressed_while_held.load(Ordering::Acquire);
        if pressed {
            self.arm_guard()?;
        }
        // Best effort: without it the floor is the save's own write, a
        // moment after the look.
        self.set_record_time(looked);
        Ok(!pressed)
    }

    /// A mute the restart guard restored, ended by a stop the service was
    /// asked for (#4332, lamp-0c89, 29 Oct 15:22): the app's Restart
    /// stopped the voice clean, and the next start read the guard's mute
    /// again, so until a tap it was the saved state. The guard's mute is
    /// not the person's ([`Self::restored_after_fault`]), and a clean stop is
    /// no fault, so it ends here, saved listening, the marker removed after.
    /// A mute the person chose since keeps, as does one a press may be
    /// making: only with nothing in flight or taken unsaved
    /// ([`Self::still_quiet`]), from the look through the save, and a press
    /// that came meanwhile puts the guard back, as
    /// [`Self::release_if_still_quiet`] does. Whether it ended.
    pub fn release_restored_on_stop(&mut self) -> io::Result<bool> {
        if !self.restored_after_fault() || self.persistence_failed {
            return Ok(false);
        }
        let Some(path) = self.options.mute_record.clone() else {
            return Ok(false);
        };
        let shared = Arc::clone(&self.shared);
        let _quiet = shared
            .quiet
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        shared.pressed_while_held.store(false, Ordering::Release);
        if !self.still_quiet() {
            return Ok(false);
        }
        #[cfg(test)]
        if let Some(press) = self.while_held.take() {
            press();
        }
        if let Err(error) = self
            .receiver
            .clear_after_explicit_user_request(monotonic_us(), &path)
        {
            self.persistence_failed = true;
            return Err(error);
        }
        // Saved listening first: a marker left beside an unmuted record is
        // removed at the next start.
        self.drop_guard_marker()?;
        if shared.pressed_while_held.load(Ordering::Acquire) {
            self.arm_guard()?;
            return Ok(false);
        }
        Ok(true)
    }

    /// The guard released after a clean stop's cleanup (#960), as
    /// [`Self::disarm_after_cleanup`], with the record's time set to the
    /// conversation's last read of the press queue ([`Self::poll`]): the
    /// next start's floor for a hold made after it ([`restored_record`]). A
    /// head hold reached during the teardown, which no voice read, is then
    /// taken by the next start; at the save's own time it was lost (the
    /// reviewer on #1632). Best effort, as at a release's look; a hold
    /// whose state was still on its way at that last read, tens of ms, is
    /// the gap left. One the conversation took is refused by the record's
    /// watermark, however early the floor.
    pub fn disarm_after_clean_stop(&mut self) -> io::Result<()> {
        self.disarm_after_cleanup()?;
        if let Some(polled) = self.polled_at {
            self.set_record_time(polled);
        }
        Ok(())
    }

    /// The mute record's file time set to `at`, best effort: the next
    /// start's floor for a late sleep ([`restored_record`]).
    fn set_record_time(&self, at: SystemTime) {
        if let Some(path) = &self.options.mute_record {
            let _ = std::fs::File::options()
                .write(true)
                .open(path)
                .and_then(|file| file.set_modified(at));
        }
    }

    /// For a new owner (#1607): a hand held on the head to put the lamp to
    /// sleep within `window`, while no voice ran, is taken on the first
    /// touch state after the first Controls state is applied
    /// ([`TouchReceiver::allow_late_sleep`]).
    pub fn allow_late_sleep(&mut self, window: LateSleep) {
        self.late_sleep = Some(window);
    }

    /// The press queue's lock, held by a test.
    #[cfg(test)]
    pub(crate) fn quiet_for_test(&self) -> std::sync::MutexGuard<'_, ()> {
        self.shared.quiet.lock().unwrap()
    }

    /// Nothing in flight and nothing taken unsaved (#1567): no control the
    /// release could write over.
    pub fn still_quiet(&self) -> bool {
        self.nothing_in_flight() && !self.dirty
    }

    /// `LAMP_GUARD_NEVER_LISTENED`, for a test.
    #[cfg(test)]
    pub(crate) fn set_never_listened_for_test(&mut self, on: bool) {
        self.never_listened = on;
    }

    /// A control taken and not yet saved, for a test.
    #[cfg(test)]
    pub(crate) fn set_dirty_for_test(&mut self) {
        self.dirty = true;
    }

    /// The count of control inputs that could carry a press, sent and not
    /// yet read (what a release's look counts, #1632), for a test.
    #[cfg(test)]
    pub(crate) fn queued_for_test(&self) -> &AtomicU64 {
        &self.shared.pressing
    }

    /// Whether `LAMP_GUARD_NEVER_LISTENED=on` (#1567).
    pub fn never_listened_on(&self) -> bool {
        self.never_listened
    }

    /// Runs in a row that failed before they heard anything, since the last
    /// that heard something; 0 once this run has (#1567).
    pub fn could_not_start(&self) -> u32 {
        self.could_not_start
    }

    fn never_listened_path(&self) -> Option<PathBuf> {
        let record = self.options.mute_record.as_ref()?;
        let mut name = record.file_name()?.to_os_string();
        name.push(NEVER_LISTENED_SUFFIX);
        Some(record.with_file_name(name))
    }

    /// The count beside the record, read only with the switch on, so a
    /// count left from when it was on says nothing with it off (#1567).
    fn load_could_not_start(&mut self) {
        self.could_not_start = if self.never_listened {
            self.read_could_not_start()
        } else {
            0
        };
    }

    /// The count kept beside the record: at most a few digits; anything
    /// else reads as none.
    fn read_could_not_start(&self) -> u32 {
        // Only a regular file of a few bytes is read, so a FIFO or a large
        // file there never holds the restore up (as #1583 for the marker).
        self.never_listened_path()
            .filter(|path| {
                path.symlink_metadata()
                    .is_ok_and(|meta| meta.is_file() && meta.len() <= 16)
            })
            .and_then(|path| std::fs::read(path).ok())
            .filter(|bytes| bytes.len() <= 10)
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(0)
    }

    /// With `LAMP_GUARD_NEVER_LISTENED=on` (#1567, B): a run that ended in a
    /// fault before its capture delivered any audio, with no control input
    /// unread (`queued`) or unsaved (`dirty`), releases the guard as a clean
    /// stop does, the latch written as it was. It heard nothing, so the guard
    /// has nothing to protect, and muting it would leave a lamp muted after
    /// an update with nobody told (lamp-4ace, 18 Oct, 15:32 and 17:48).
    /// Whether it released.
    pub fn release_never_listened(&mut self) -> io::Result<bool> {
        if !self.never_listened || !self.armed || self.heard || self.persistence_failed {
            return Ok(false);
        }
        if !self.release_if_still_quiet()? {
            return Ok(false);
        }
        // Advice for the health only: a count that cannot be kept changes no
        // permission.
        self.could_not_start = self.could_not_start.saturating_add(1);
        if let Some(path) = self.never_listened_path() {
            let _ = write_count(&path, self.could_not_start);
        }
        Ok(true)
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

/// The record at a voice's start (#1505): read, its stored identity for the
/// guard's marker, saved again (checking the private parent and
/// first-use/rebased persistence now: a path that cannot store a mute may
/// not open audio), and the late sleep window it opens (#1607). A hold made
/// while no voice ran is taken if it began after the record's watermark
/// and was reached after the voice before last looked for input (the
/// record's time, set to that look by [`HandControls::release_if_still_quiet`]),
/// and before this start. No window without a time for the record.
fn restored_record(path: &Path, clock_id: &str, now_us: u64) -> io::Result<Restored> {
    // A record this build cannot read never stops the voice: it reads as
    // muted, and that is said once (#1505).
    let (record, note) = MuteRecord::load_noting(path, clock_id, now_us)?;
    // Kept as a flag, for `voice/mute` to say why it is muted (#4332).
    let unreadable = note.is_some();
    if let Some(note) = note {
        eprintln!("lamp-voice: {note}");
    }
    // The stored record as it is on disk, before a new boot's rebase: what
    // the guard's marker must still match (the reviewer on #997). Read only
    // once `load_noting` has found a private regular file of at most 4 KiB
    // there (and before `save` writes anything), so a FIFO or a huge file
    // refuses with a line instead of hanging the voice (Jarvis on #1548).
    let stored = stored_identity(path);
    // Before this save writes the record again.
    let looked_us = last_saved_us(path, now_us);
    record.save(path)?;
    let late_sleep = looked_us.map(|looked| LateSleep {
        began_after_us: record.through_us,
        reached_after_us: record.through_us.max(looked),
        reached_before_us: now_us,
    });
    Ok(Restored {
        record,
        stored,
        late_sleep,
        unreadable,
    })
}

/// At a start, before the controls are built (#4332): a muted record that is
/// the restart guard of an earlier boot, its marker naming it as stored, is
/// a power cut, not a fault of this run. It is saved again as the person
/// left it, listening, and the marker removed. Whether it was.
///
/// The person's own mute is what the guard stood over: [`HandControls::arm_active`]
/// arms only an unmuted latch, and every mute a person makes after it drops
/// the marker before it is saved ([`HandControls::mute_chosen`]) and moves
/// `through_us` on, so a marker that still names the record means no mute
/// was chosen since. The record's `clock_id` is the boot id
/// (`lamp_controls::runtime::clock_id`), so another clock is another boot; a
/// marker from a build before this one names its clock the same way. A
/// guard of this boot (a crash, or a voice that ended before its stop) keeps
/// its mute: a mute or a privacy change may have been in flight. A press
/// accepted in the last moment before the power went, before it was saved,
/// is lost, as any unsaved request is (the module's note).
///
/// The record is saved first: a fault between the two leaves an unmuted
/// record beside the marker, which [`HandControls::restore_guard_marker`]
/// removes. Without `LAMP_RESTORED_MUTE=on` there is no marker to trust, and
/// the mute stays.
fn released_after_power_cut(
    path: &Path,
    mark_guard: bool,
    restored: &mut Restored,
) -> io::Result<bool> {
    let record = &mut restored.record;
    let Some(stored) = &restored.stored else {
        return Ok(false);
    };
    if !mark_guard || !record.muted || record.reason.is_some() || stored.0 == record.clock_id {
        return Ok(false);
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(GUARD_MARKER_SUFFIX);
    let marker = path.with_file_name(name);
    if !marker.symlink_metadata().is_ok_and(|meta| meta.is_file()) {
        return Ok(false);
    }
    let names = std::fs::read_to_string(&marker)
        .ok()
        .and_then(|text| marker_identity(&text));
    if names.as_ref() != Some(stored) {
        return Ok(false);
    }
    let mut listening = record.clone();
    listening.muted = false;
    listening.save(path)?;
    *record = listening;
    let _ = std::fs::remove_file(&marker);
    Ok(true)
}

/// What [`restored_record`] reads at a start.
struct Restored {
    record: MuteRecord,
    /// The record's `clock_id` and `through_us` as stored, before a rebase.
    stored: Option<(String, u64)>,
    late_sleep: Option<LateSleep>,
    /// `load_noting` could not trust the record and read it as muted.
    unreadable: bool,
}

/// When `path` was last written, on the monotonic clock of `now_us`: its
/// age by the wall clock taken from now. None when it is not there or its
/// time is ahead of the wall clock (#1607), so no late sleep is taken.
fn last_saved_us(path: &Path, now_us: u64) -> Option<u64> {
    let modified = path.symlink_metadata().ok()?.modified().ok()?;
    let age = std::time::SystemTime::now().duration_since(modified).ok()?;
    Some(now_us.saturating_sub(u64::try_from(age.as_micros()).unwrap_or(u64::MAX)))
}

/// Nothing there, or a regular file: never a link, a FIFO or a folder,
/// whose target a write or a removal would touch (#1567, Jarvis on #1592).
fn regular_or_absent(path: &Path) -> bool {
    match path.symlink_metadata() {
        Ok(meta) => meta.is_file(),
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

/// The count, written whole beside a regular file or nothing: to a new
/// private file first, then renamed over, so a link there is replaced, not
/// followed.
fn write_count(path: &Path, count: u32) -> io::Result<()> {
    if !regular_or_absent(path) {
        return Err(io::Error::other("the count's path is not a regular file"));
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".new");
    let new = path.with_file_name(name);
    let _ = std::fs::remove_file(&new);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&new)?;
    std::io::Write::write_all(&mut file, format!("{count}\n").as_bytes())?;
    drop(file);
    std::fs::rename(&new, path)
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

/// Whether `input` could carry a press (#1607, Fourth and the reviewer on
/// #1632): a head touch event, or a Controls or touch state whose events
/// moved on since the last one queued (a new `latest_mute`, `latest_stop` or
/// head event). Not the routine republish of the same state every 100 ms,
/// nor a pet, which never changes the mute. One producer only.
fn carries_press(input: &Input, shared: &Shared) -> bool {
    match input {
        Input::Touch(_) => true,
        Input::State(state) => moved(
            &shared.controls_seen,
            &state.session_id,
            state.event_sequence,
        ),
        Input::TouchState(state) => {
            moved(&shared.touch_seen, &state.session_id, state.event_sequence)
        }
        Input::Pet(_) => false,
    }
}

/// Whether `sequence` in `session` is past the last one seen there, which
/// it then is. A new session's events count from nothing again, so after
/// Controls restarts its first mute is a press, not one taken for an old
/// number (Fourth on #1632).
fn moved(seen: &std::sync::Mutex<Option<(String, u64)>>, session: &str, sequence: u64) -> bool {
    let mut seen = seen
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let last = match seen.as_ref() {
        Some((was, last)) if was == session => *last,
        _ => 0,
    };
    *seen = Some((session.to_owned(), sequence.max(last)));
    sequence > last
}

fn enqueue(sender: &SyncSender<Queued>, shared: &Shared, input: Input) -> bool {
    let press = carries_press(&input, shared);
    let queued = Queued {
        input,
        generation: shared.discontinuities.load(Ordering::Acquire),
        press,
    };
    // Not while a release looks for inputs in flight and saves (#1607); a
    // press that has to wait says so, and the release keeps the guard.
    let _quiet = match shared.quiet.try_lock() {
        Ok(quiet) => quiet,
        Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => {
            if press {
                shared.pressed_while_held.store(true, Ordering::Release);
            }
            #[cfg(test)]
            shared.waited.fetch_add(1, Ordering::AcqRel);
            shared
                .quiet
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }
    };
    // Publish the reservation before the item, so a quiescent clear never
    // mistakes a producer in try_send for an empty queue. One producer only.
    shared.queued.fetch_add(1, Ordering::AcqRel);
    if press {
        shared.pressing.fetch_add(1, Ordering::AcqRel);
    }
    let unqueued = || {
        shared.queued.fetch_sub(1, Ordering::AcqRel);
        if press {
            shared.pressing.fetch_sub(1, Ordering::AcqRel);
        }
    };
    // A state the queue could not take never reached the owner: its number
    // is forgotten, so the next state from its topic counts as a press, not
    // one taken for a number that was lost (the reviewer on #1632).
    let forget = |dropped: &Queued| {
        let seen = match dropped.input {
            Input::State(_) => &shared.controls_seen,
            Input::TouchState(_) => &shared.touch_seen,
            Input::Touch(_) | Input::Pet(_) => return,
        };
        *seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    };
    match sender.try_send(queued) {
        Ok(()) => true,
        Err(TrySendError::Full(dropped)) => {
            forget(&dropped);
            unqueued();
            add(&shared.lost, 1);
            shared.fence();
            true
        }
        Err(TrySendError::Disconnected(dropped)) => {
            forget(&dropped);
            unqueued();
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

/// The pipe the owner wakes a waiting pump with: both ends close on exec and
/// never block. Made with `pipe` and `fcntl`, as `pipe2` is not on every
/// system the voice builds on (macOS).
fn wake_pipe() -> io::Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)> {
    let (read, write) = rustix::pipe::pipe()?;
    for end in [&read, &write] {
        rustix::io::fcntl_setfd(end, rustix::io::FdFlags::CLOEXEC)?;
        rustix::fs::fcntl_setfl(end, rustix::fs::OFlags::NONBLOCK)?;
    }
    Ok((read, write))
}

/// With [`ControlsOptions::wait`]: until a subscriber's socket is readable,
/// the owner writes to the pipe, or [`WAIT_AT_MOST`] passes, and no longer
/// than none if a subscriber already holds a datagram read ahead. The pipe
/// is emptied.
fn wait_on(subscribed: &[(Subscriber, Topic)], woken: &std::os::fd::OwnedFd) {
    use rustix::event::{PollFd, PollFlags, poll};
    if subscribed.iter().any(|(subscriber, _)| subscriber.holds()) {
        return;
    }
    let mut fds: Vec<PollFd<'_>> = subscribed
        .iter()
        .map(|(subscriber, _)| PollFd::new(subscriber, PollFlags::IN))
        .collect();
    fds.push(PollFd::new(woken, PollFlags::IN));
    let timeout = rustix::time::Timespec {
        tv_sec: 0,
        tv_nsec: WAIT_AT_MOST.as_nanos() as _,
    };
    if poll(&mut fds, Some(&timeout)).is_err() {
        // A failed wait is never a busy loop: it waits as the tick does.
        thread::park_timeout(PUMP_TICK);
    }
    let mut drained = [0u8; 64];
    while matches!(rustix::io::read(woken, &mut drained), Ok(read) if read > 0) {}
}

fn pump(
    root: PathBuf,
    head_touch: bool,
    say_mute: bool,
    woken: Option<std::os::fd::OwnedFd>,
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
                add(&shared.passes, 1);
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
                #[cfg(test)]
                shared.pump_ready.store(true, Ordering::Release);
                match &woken {
                    Some(woken) => wait_on(&subscribed, woken),
                    None => {
                        let duration = PUMP_TICK;
                        thread::park_timeout(duration);
                        #[cfg(test)]
                        shared.tick_waits.observe(duration);
                    }
                }
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
