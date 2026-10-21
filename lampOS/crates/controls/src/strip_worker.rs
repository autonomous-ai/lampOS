//! One disposable MPR121 process. I2C, including initialization, never runs in
//! the GPIO/gesture owner. There is no replacement until this owner restarts.
//! Datagram loss remains sequence loss; cached levels are never re-stamped.
use crate::{input::InputError, mpr121, touch::StripSample};
use lamp_msgs::{ClockStamp, ControlFault};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        net::UnixDatagram,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub const STARTUP_US: u64 = 7_000_000;
pub const SILENCE_US: u64 = 150_000;
pub const MAX_PACKET_BYTES: usize = 1024;

/// Held by the I2C child, not its parent, until the device is closed. The fixed
/// per-device path excludes owners using different bus roots too. Never unlink
/// this inode: an old child may still hold it while blocked in the kernel.
pub struct DeviceClaim {
    _file: fs::File,
}

impl DeviceClaim {
    pub fn at(path: &Path) -> io::Result<Self> {
        use rustix::fs::{Mode, OFlags};
        let fd = rustix::fs::open(
            path,
            OFlags::CREATE | OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR,
        )?;
        let file = fs::File::from(fd);
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 || metadata.mode() & 0o077 != 0 {
            return Err(io::Error::other("unsafe MPR121 device claim file"));
        }
        file.try_lock().map_err(|error| {
            io::Error::other(format!(
                "MPR121 already owned or claim unavailable: {error}"
            ))
        })?;
        Ok(Self { _file: file })
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn acquire(config: &mpr121::Config) -> io::Result<Self> {
        Self::at(Path::new(&format!(
            "/run/lock/lamp-controls-mpr121-{}-{:02x}.lock",
            config.bus, config.address
        )))
    }
}

/// Same nonblocking contract as [`crate::input::PinReader`], for an independently acquired
/// strip. GPIO freshness is unaffected by any error returned here.
pub trait StripReader {
    fn read(&mut self, now_us: u64) -> Result<Option<StripSample>, InputError>;
}

impl<R: StripReader + ?Sized> StripReader for Box<R> {
    fn read(&mut self, now_us: u64) -> Result<Option<StripSample>, InputError> {
        (**self).read(now_us)
    }
}

impl<R: StripReader> StripReader for Option<R> {
    fn read(&mut self, now_us: u64) -> Result<Option<StripSample>, InputError> {
        match self {
            Some(reader) => reader.read(now_us),
            None => Ok(None),
        }
    }
}

pub struct NoStrip;
impl StripReader for NoStrip {
    fn read(&mut self, _: u64) -> Result<Option<StripSample>, InputError> {
        Ok(None)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Packet {
    Sample {
        stamp: ClockStamp,
        session_id: String,
        mask: u16,
    },
    Fault {
        stamp: ClockStamp,
        session_id: String,
        detail: String,
    },
}

impl Packet {
    fn identity(&self) -> (&ClockStamp, &str) {
        match self {
            Self::Sample {
                stamp, session_id, ..
            }
            | Self::Fault {
                stamp, session_id, ..
            } => (stamp, session_id),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Cleanup {
    pub kill_requested: bool,
    pub exit_observed: bool,
    pub success: Option<bool>,
    pub error: Option<String>,
}

pub struct Worker {
    child: Child,
    socket: UnixDatagram,
    path: PathBuf,
    clock: String,
    session: String,
    started_us: u64,
    last_packet_us: Option<u64>,
    last_now_us: u64,
    ended: bool,
    cleanup: Cleanup,
}

impl Worker {
    /// No sensor is opened by the parent. The bounded serialized profile is
    /// passed directly as an argument, so the child uses this exact validated
    /// snapshot rather than reopening a configuration file that may change.
    pub fn start(
        config: &mpr121::Config,
        root: &Path,
        clock: &str,
        session: &str,
    ) -> io::Result<Self> {
        config.check().map_err(io::Error::other)?;
        let profile = serde_json::to_string(config)?;
        if profile.len() > 16_384 {
            return Err(io::Error::other("MPR121 profile exceeds 16 KiB"));
        }
        let mut command = Command::new(std::env::current_exe()?);
        command.arg("mpr-worker").arg("--profile-json").arg(profile);
        Self::spawn(command, root, clock, session, lamp_ipc::monotonic_us())
    }

    /// The command injection is for deterministic worker lifecycle tests.
    /// Production always calls `start`, which runs this executable only.
    pub fn spawn(
        mut command: Command,
        root: &Path,
        clock: &str,
        session: &str,
        now_us: u64,
    ) -> io::Result<Self> {
        crate::input::check_name(clock).map_err(io::Error::other)?;
        crate::input::check_name(session).map_err(io::Error::other)?;
        let path = root.join(format!("controls-mpr-{}-{now_us}.sock", std::process::id()));
        // Never unlink an existing pathname: it may belong to a living reader.
        let socket = UnixDatagram::bind(&path)?;
        let prepare = (|| {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            socket.set_nonblocking(true)?;
            command
                .arg("--sink")
                .arg(&path)
                .arg("--clock-id")
                .arg(clock)
                .arg("--session")
                .arg(session)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
        })();
        let child = match prepare {
            Ok(child) => child,
            Err(error) => {
                let _ = fs::remove_file(&path);
                return Err(error);
            }
        };
        Ok(Self {
            child,
            socket,
            path,
            clock: clock.into(),
            session: session.into(),
            started_us: now_us,
            last_packet_us: None,
            last_now_us: now_us,
            ended: false,
            cleanup: Cleanup::default(),
        })
    }

    fn observe_exit(&mut self) -> io::Result<bool> {
        if self.cleanup.exit_observed {
            return Ok(true);
        }
        if let Some(status) = self.child.try_wait()? {
            self.cleanup.exit_observed = true;
            self.cleanup.success = Some(status.success());
        }
        Ok(self.cleanup.exit_observed)
    }

    fn inhibit(&mut self) {
        self.ended = true;
        match self.observe_exit() {
            Ok(true) => return,
            Ok(false) => {}
            Err(error) => self.cleanup.error = Some(error.to_string()),
        }
        if !self.cleanup.kill_requested {
            self.cleanup.kill_requested = true;
            if let Err(error) = self.child.kill() {
                self.cleanup.error = Some(error.to_string());
            }
        }
    }

    fn fail(&mut self, fault: ControlFault, message: impl Into<String>) -> InputError {
        self.inhibit();
        InputError::new(fault, message)
    }

    /// At most 100 ms of bounded polling. A kernel-blocked child may not exit
    /// in that time; report that honestly and never start a second worker.
    pub fn shutdown(&mut self) -> Cleanup {
        self.inhibit();
        let deadline = Instant::now() + Duration::from_millis(100);
        loop {
            match self.observe_exit() {
                Ok(true) => break,
                Ok(false) => {}
                Err(error) => {
                    self.cleanup.error = Some(error.to_string());
                    break;
                }
            }
            if Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        self.cleanup.clone()
    }
}

impl StripReader for Worker {
    fn read(&mut self, now_us: u64) -> Result<Option<StripSample>, InputError> {
        if self.ended {
            let _ = self.observe_exit();
            return Ok(None);
        }
        if now_us < self.last_now_us {
            return Err(self.fail(
                ControlFault::ClockReset,
                "strip owner clock moved backwards",
            ));
        }
        self.last_now_us = now_us;
        let timeout = self.last_packet_us.map_or(STARTUP_US, |_| SILENCE_US);
        let last = self.last_packet_us.unwrap_or(self.started_us);
        if now_us.saturating_sub(last) >= timeout {
            return Err(self.fail(
                ControlFault::Stale,
                "strip worker exceeded its response deadline",
            ));
        }
        let mut bytes = [0u8; MAX_PACKET_BYTES + 1];
        match self.socket.recv(&mut bytes) {
            Ok(length) => {
                if length > MAX_PACKET_BYTES {
                    return Err(
                        self.fail(ControlFault::Overflow, "strip worker packet exceeds 1 KiB")
                    );
                }
                let packet: Packet = match serde_json::from_slice(&bytes[..length]) {
                    Ok(packet) => packet,
                    Err(_) => {
                        return Err(self.fail(ControlFault::Invalid, "invalid strip worker packet"));
                    }
                };
                let (stamp, session) = packet.identity();
                if stamp.check().is_err() || stamp.sequence == 0 || session != self.session {
                    return Err(
                        self.fail(ControlFault::Invalid, "strip worker identity is invalid")
                    );
                }
                if stamp.clock_id != self.clock {
                    return Err(
                        self.fail(ControlFault::WrongClock, "strip worker boot does not match")
                    );
                }
                if stamp.at_us > now_us {
                    return Err(
                        self.fail(ControlFault::Future, "strip acquisition is in the future")
                    );
                }
                if now_us - stamp.at_us >= crate::input::INPUT_FRESH_US {
                    return Err(
                        self.fail(ControlFault::Stale, "strip acquisition is stale on arrival")
                    );
                }
                self.last_packet_us = Some(now_us);
                match packet {
                    Packet::Sample {
                        stamp,
                        session_id,
                        mask,
                    } => {
                        if mask & !crate::base::ALL_PADS != 0 {
                            return Err(self.fail(
                                ControlFault::Invalid,
                                "strip worker returned invalid electrodes",
                            ));
                        }
                        Ok(Some(StripSample {
                            stamp,
                            session_id,
                            mask,
                        }))
                    }
                    Packet::Fault { detail, .. } => Err(self.fail(
                        ControlFault::Missing,
                        format!("MPR121 unavailable: {}", bounded_detail(&detail)),
                    )),
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                match self.observe_exit() {
                    Ok(true) => return Err(self.fail(ControlFault::Missing, "strip worker exited")),
                    Ok(false) => {}
                    Err(error) => {
                        return Err(
                            self.fail(ControlFault::Missing, format!("strip worker exit: {error}"))
                        );
                    }
                }
                Ok(None)
            }
            Err(error) => Err(self.fail(
                ControlFault::Missing,
                format!("strip worker receive: {error}"),
            )),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.inhibit();
        let _ = self.observe_exit();
        let _ = fs::remove_file(&self.path);
    }
}

fn bounded_detail(text: &str) -> String {
    let mut output = String::new();
    for character in text.chars().filter(|c| !c.is_control()) {
        if output.len() + character.len_utf8() > 256 {
            break;
        }
        output.push(character);
    }
    output
}

/// Send at most one datagram. A full receiver queue drops this acquisition;
/// the next acquisition keeps increasing sequence, exposing the loss.
pub fn send(socket: &UnixDatagram, packet: &Packet) -> io::Result<()> {
    let bytes = serde_json::to_vec(packet)?;
    if bytes.len() > MAX_PACKET_BYTES {
        return Err(io::Error::other("strip worker packet too large"));
    }
    match socket.send(&bytes) {
        Ok(sent) if sent == bytes.len() => Ok(()),
        Ok(_) => Err(io::Error::other("partial strip datagram")),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(()),
        Err(error) => Err(error),
    }
}

/// Pace the whole acquisition and send on the strip's own thread. A read
/// that exceeds the polling period still leaves CPU time for other work;
/// time blocked in I2C is not charged again as sleep.
#[cfg(any(target_os = "linux", test))]
fn read_loop(poll_us: u64, mut read: impl FnMut(u64) -> io::Result<()>) -> io::Result<()> {
    loop {
        crate::pace::try_run_step(poll_us, &mut read)?;
    }
}

/// Internal child entry point. It opens no GPIO, bus topic or output owner.
#[cfg(target_os = "linux")]
pub fn run(config: mpr121::Config, sink: &Path, clock: &str, session: &str) -> io::Result<()> {
    config.check().map_err(io::Error::other)?;
    crate::input::check_name(clock).map_err(io::Error::other)?;
    crate::input::check_name(session).map_err(io::Error::other)?;
    let socket = UnixDatagram::unbound()?;
    socket.connect(sink)?;
    socket.set_nonblocking(true)?;
    let mut sequence = 0u64;
    let stamp = |at_us, sequence| ClockStamp {
        clock_id: clock.into(),
        at_us,
        sequence,
    };
    let poll_us = config.every_ms.saturating_mul(1_000);
    let result: io::Result<()> = (|| {
        let _claim = DeviceClaim::acquire(&config)?;
        let io = mpr121::LinuxIo::open(&config).map_err(io::Error::other)?;
        let mut delay = mpr121::ThreadDelay;
        let mut device =
            mpr121::Device::initialize(io, config, &mut delay).map_err(io::Error::other)?;
        read_loop(poll_us, |began| {
            sequence = sequence
                .checked_add(1)
                .ok_or_else(|| io::Error::other("strip sequence exhausted"))?;
            let mask = device.read().map_err(io::Error::other)?;
            send(
                &socket,
                &Packet::Sample {
                    stamp: stamp(began, sequence),
                    session_id: session.into(),
                    mask,
                },
            )
        })
    })();
    if let Err(error) = &result {
        let _ = send(
            &socket,
            &Packet::Fault {
                stamp: stamp(lamp_ipc::monotonic_us(), sequence.saturating_add(1)),
                session_id: session.into(),
                detail: bounded_detail(&error.to_string()),
            },
        );
    }
    result
}

#[cfg(not(target_os = "linux"))]
pub fn run(_: mpr121::Config, _: &Path, _: &str, _: &str) -> io::Result<()> {
    Err(io::Error::other("MPR121 hardware requires Linux"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu_us() -> u64 {
        let time = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
        u64::try_from(time.tv_sec).unwrap() * 1_000_000
            + u64::try_from(time.tv_nsec).unwrap() / 1_000
    }

    #[test]
    fn first_and_repeated_slow_strip_reads_leave_at_least_their_cpu_work_to_rest() {
        let mut previous: Option<(u64, u64)> = None;
        let mut reads = 0;
        let error = read_loop(1_000, |began| {
            if let Some((finished, worked)) = previous {
                let rested = began.checked_sub(finished).unwrap();
                eprintln!("strip pacing host: read={reads}, cpu_us={worked}, rest_us={rested}");
                assert!(rested >= worked, "a slow strip read skipped its rest");
            }
            if reads == 3 {
                return Err(io::Error::other("end of strip fixture"));
            }
            // Exercise the production loop with a read that outlasts its
            // period through CPU work, rather than a sleeping I2C stand-in.
            let began_cpu = cpu_us();
            let deadline = Instant::now() + Duration::from_secs(2);
            while cpu_us().saturating_sub(began_cpu) < 5_000 {
                assert!(Instant::now() < deadline, "fixture did not get CPU time");
                std::hint::spin_loop();
            }
            let worked = cpu_us() - began_cpu;
            previous = Some((lamp_ipc::monotonic_us(), worked));
            reads += 1;
            Ok(())
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "end of strip fixture");
        assert_eq!(reads, 3);
    }
}
