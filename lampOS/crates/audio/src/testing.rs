//! Scripted audio devices for tests of the boundaries and of the worker
//! loops built on them. Nothing here is used by the runtime, and nothing here
//! behaves like a codec, a clock or a room: a test decides what the device
//! holds and when it fails.
use crate::{
    capture::{CaptureConfig, CaptureFormat},
    device::{
        CaptureDevice, CaptureStatus, ConfigureError, DeviceFault, DeviceState, Woken, errno,
    },
};
use std::{
    collections::{BTreeMap, VecDeque},
    os::fd::{BorrowedFd, OwnedFd},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

struct CaptureScript {
    format: CaptureFormat,
    state: DeviceState,
    /// Samples the next reads will return, oldest first.
    queued: VecDeque<i16>,
    /// One-shot failures by call name: open, configure, start, read, status,
    /// discard.
    faults: Vec<(&'static str, i32)>,
    /// How long the next read blocks before it returns.
    stall: Option<Duration>,
    opens: u32,
    starts: u32,
    discards: u32,
    reads: u64,
    /// A pipe readable while `avail_min` samples are queued, as a PCM's
    /// descriptor is: what [`CaptureDevice::wait`] polls.
    readable: (OwnedFd, OwnedFd),
    /// Set by each wait, as `snd_pcm_sw_params_set_avail_min`.
    avail_min: usize,
    signalled: bool,
    waits: u64,
}

impl CaptureScript {
    /// The pipe says whether `avail_min` samples are queued.
    fn signal(&mut self) {
        let queued = !self.queued.is_empty() && self.queued.len() >= self.avail_min;
        if queued && !self.signalled {
            let _ = rustix::io::write(&self.readable.1, &[1]);
        } else if !queued && self.signalled {
            let _ = rustix::io::read(&self.readable.0, &mut [0u8; 8]);
        }
        self.signalled = queued;
    }
}

/// A test's hold on one scripted microphone. The device itself is opened by
/// name through [`CaptureDevice::open`], as the boundary opens any device.
#[derive(Clone)]
pub struct CaptureControl(Arc<Mutex<CaptureScript>>);

static CAPTURES: Mutex<BTreeMap<String, CaptureControl>> = Mutex::new(BTreeMap::new());

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl CaptureControl {
    /// Make `name` openable. Names are process-wide; use one per test.
    pub fn register(name: &str) -> Self {
        let control = Self(Arc::new(Mutex::new(CaptureScript {
            format: CaptureFormat {
                rate: 16_000,
                channels: 1,
                period_frames: 160,
                buffer_frames: 640,
                monotonic_status_timestamps: false,
            },
            state: DeviceState::Setup,
            queued: VecDeque::new(),
            faults: Vec::new(),
            stall: None,
            opens: 0,
            starts: 0,
            discards: 0,
            reads: 0,
            readable: rustix::pipe::pipe().expect("a pipe for the scripted microphone"),
            signalled: false,
            avail_min: 1,
            waits: 0,
        })));
        locked(&CAPTURES).insert(name.to_owned(), control.clone());
        control
    }
    fn script(&self) -> MutexGuard<'_, CaptureScript> {
        locked(&self.0)
    }
    /// What `configure` will report as installed.
    pub fn set_format(&self, format: CaptureFormat) {
        self.script().format = format;
    }
    /// Samples the microphone has "captured" and not yet handed over.
    pub fn push(&self, samples: &[i16]) {
        let mut script = self.script();
        script.queued.extend(samples);
        script.signal();
    }
    /// The next call of this name fails once with `errno`, one of
    /// [`crate::device::errno`].
    pub fn fail_next(&self, call: &'static str, errno: i32) {
        self.script().faults.push((call, errno));
    }
    /// The stream state reported by `status` from now on.
    pub fn set_state(&self, state: DeviceState) {
        self.script().state = state;
    }
    /// The next read blocks for `duration`, as a stuck driver call does.
    pub fn stall_next_read(&self, duration: Duration) {
        self.script().stall = Some(duration);
    }
    pub fn queued(&self) -> usize {
        self.script().queued.len()
    }
    /// How often the device was opened, started and discarded, and how many
    /// reads it has served.
    pub fn counts(&self) -> (u32, u32, u32, u64) {
        let script = self.script();
        (script.opens, script.starts, script.discards, script.reads)
    }
    /// How often the device was waited on ([`CaptureDevice::wait`]).
    pub fn waits(&self) -> u64 {
        self.script().waits
    }
}

pub struct ScriptedCapture(CaptureControl);

impl ScriptedCapture {
    fn failing(&self, call: &'static str) -> Result<(), DeviceFault> {
        let mut script = self.0.script();
        match script.faults.iter().position(|(name, _)| *name == call) {
            Some(index) => {
                let (_, errno) = script.faults.remove(index);
                Err(DeviceFault::new(call, errno))
            }
            None => Ok(()),
        }
    }
}

impl CaptureDevice for ScriptedCapture {
    fn open(name: &str) -> Result<Self, DeviceFault> {
        let control = locked(&CAPTURES)
            .get(name)
            .cloned()
            .ok_or(DeviceFault::new(crate::device::OPEN_CALL, errno::NOENT))?;
        let device = Self(control);
        device.failing(crate::device::OPEN_CALL)?;
        let mut script = device.0.script();
        script.opens += 1;
        script.state = DeviceState::Setup;
        drop(script);
        Ok(device)
    }

    fn configure(&mut self, _config: CaptureConfig) -> Result<CaptureFormat, ConfigureError> {
        self.failing("configure").map_err(ConfigureError::Fault)?;
        let mut script = self.0.script();
        script.state = DeviceState::Prepared;
        script.avail_min = 1;
        script.signal();
        Ok(script.format)
    }

    fn start(&mut self) -> Result<(), DeviceFault> {
        self.failing("start")?;
        let mut script = self.0.script();
        script.starts += 1;
        script.state = DeviceState::Running;
        Ok(())
    }

    fn read(&mut self, into: &mut [i16]) -> Result<usize, DeviceFault> {
        let stall = self.0.script().stall.take();
        if let Some(stall) = stall {
            std::thread::sleep(stall);
        }
        self.failing("read")?;
        let mut script = self.0.script();
        script.reads += 1;
        if script.queued.is_empty() {
            return Err(DeviceFault::new("read", errno::AGAIN));
        }
        let frames = into.len().min(script.queued.len());
        for (target, sample) in into.iter_mut().zip(script.queued.drain(..frames)) {
            *target = sample;
        }
        script.signal();
        Ok(frames)
    }

    fn status(&mut self) -> Result<CaptureStatus, DeviceFault> {
        self.failing("status")?;
        let script = self.0.script();
        Ok(CaptureStatus {
            state: script.state,
            monotonic_us: None,
            available: script.queued.len() as i64,
            delayed: script.queued.len() as i64,
        })
    }

    fn discard(&mut self) -> Result<(), DeviceFault> {
        let mut script = self.0.script();
        script.discards += 1;
        script.queued.clear();
        script.signal();
        script.state = DeviceState::Setup;
        drop(script);
        self.failing("discard")
    }

    fn wait(
        &mut self,
        others: &[BorrowedFd<'_>],
        timeout: std::time::Duration,
        frames: usize,
    ) -> Option<Result<Woken, DeviceFault>> {
        use rustix::event::{PollFd, PollFlags, poll};
        // The pipe is duplicated so the script is not locked while waiting.
        let readable = {
            let mut script = self.0.script();
            script.waits += 1;
            script.avail_min = frames.max(1);
            script.signal();
            rustix::io::dup(&script.readable.0).ok()?
        };
        let mut fds = vec![PollFd::new(&readable, PollFlags::IN)];
        fds.extend(
            others
                .iter()
                .map(|other| PollFd::from_borrowed_fd(*other, PollFlags::IN)),
        );
        let timeout = rustix::time::Timespec {
            tv_sec: timeout.as_secs() as _,
            tv_nsec: timeout.subsec_nanos() as _,
        };
        if let Err(error) = poll(&mut fds, Some(&timeout)) {
            return Some(Err(DeviceFault::new("poll", error.raw_os_error())));
        }
        Some(Ok(Woken {
            samples: !fds[0].revents().is_empty(),
            others: fds[1..].iter().any(|fd| !fd.revents().is_empty()),
        }))
    }
}
