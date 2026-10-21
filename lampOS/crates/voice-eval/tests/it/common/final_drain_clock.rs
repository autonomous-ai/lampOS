//! One explicit delayed-line fixture; real pipe enqueue and backend drain stay separate.
use super::{Recording, Rig, Then, lines};
use lamp_turn_owner::MonoTime;
use lamp_voice_eval::{
    Result,
    physical::mac::{FixtureDriver, PhysicalBackend},
};
use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use serde_json::Value;
use std::{
    cell::Cell,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

const ORIGIN_US: u64 = 1_000_000;
const DELAY_US: u64 = 300_000;
const HOST_WINDOW: Duration = Duration::from_secs(10);
const INITIAL: &[u8] = b"now-written\n";
const LATER: &[u8] = b"later-written\n";

struct State {
    now: Cell<u64>,
    released_at: Cell<Option<u64>>,
}

struct Driver {
    state: Rc<State>,
    control: File,
    initial_marker: PathBuf,
    later_marker: PathBuf,
    deadline: Instant,
    started: bool,
    released: bool,
}

fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}

fn marker(path: &Path, expected: &[u8]) -> io::Result<bool> {
    match fs::read(path) {
        Ok(bytes) if bytes == expected => Ok(true),
        Ok(bytes) if expected.starts_with(&bytes) => Ok(false),
        Ok(_) => Err(invalid("unexpected delayed-line completion marker")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

impl Driver {
    fn prepare(rig: &Rig, now: &[Value], later: &str) -> (Self, String) {
        let deadline = Instant::now() + HOST_WINDOW;
        fs::write(rig.dir.join("now.jsonl"), lines(now)).unwrap();
        fs::write(rig.dir.join("later.jsonl"), later).unwrap();
        let fifo = rig.dir.join("delayed-release.fifo");
        assert!(
            Command::new("mkfifo")
                .args(["-m", "600"])
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        // Opening both ends avoids a startup open race. Nonblocking writes
        // fail closed instead of turning control or Drop into a host wait.
        let control = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&fifo)
            .unwrap();
        let flags = fcntl_getfl(&control).unwrap();
        fcntl_setfl(&control, flags | OFlags::NONBLOCK).unwrap();
        let initial_marker = rig.dir.join("now-written");
        let later_marker = rig.dir.join("later-written");
        let script = format!(
            "exec 3< '{fifo}'; \
             cat '{now}' || exit 71; \
             printf 'now-written\\n' > '{initial_marker}' || exit 72; \
             IFS= read -r command <&3 || exit 0; \
             case \"$command\" in release) ;; abort) exit 0 ;; *) exit 73 ;; esac; \
             cat '{later}' || exit 74; \
             printf 'later-written\\n' > '{later_marker}' || exit 75; \
             while IFS= read -r runner_line; do :; done",
            fifo = fifo.display(),
            now = rig.dir.join("now.jsonl").display(),
            later = rig.dir.join("later.jsonl").display(),
            initial_marker = initial_marker.display(),
            later_marker = later_marker.display(),
        );
        (
            Self {
                state: Rc::new(State {
                    now: Cell::new(ORIGIN_US),
                    released_at: Cell::new(None),
                }),
                control,
                initial_marker,
                later_marker,
                deadline,
                started: false,
                released: false,
            },
            script,
        )
    }

    fn check_host(&self) -> io::Result<()> {
        if Instant::now() >= self.deadline {
            Err(invalid("delayed-line fixture exceeded its host watchdog"))
        } else {
            Ok(())
        }
    }

    fn written_and_enqueued(
        &self,
        path: &Path,
        expected: &[u8],
        enqueued: &AtomicU64,
        count: u64,
    ) -> io::Result<()> {
        loop {
            if self.written_and_enqueued_once(path, expected, enqueued, count)? {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn written_and_enqueued_once(
        &self,
        path: &Path,
        expected: &[u8],
        enqueued: &AtomicU64,
        count: u64,
    ) -> io::Result<bool> {
        self.check_host()?;
        let written = marker(path, expected)?;
        let actual = enqueued.load(Ordering::Acquire);
        // A filesystem read may return after the watchdog. A late positive
        // observation must not authorize another logical advance.
        self.check_host()?;
        if actual > count {
            return Err(invalid("unexpected delayed-line relay frame count"));
        }
        Ok(written && actual == count)
    }
}

impl FixtureDriver for Driver {
    fn wait(
        &mut self,
        until: MonoTime,
        _sent_frames: u64,
        enqueued_frames: &AtomicU64,
        _queue_drained: bool,
    ) -> Result<()> {
        self.check_host()?;
        let until_us = until.as_micros();
        if until_us < self.state.now.get() {
            return Err(invalid("delayed-line fixture time regressed").into());
        }
        if !self.started {
            // The original sleep follows completion of the first cat. A
            // queued line alone does not prove that external cat has returned.
            self.written_and_enqueued(&self.initial_marker, INITIAL, enqueued_frames, 1)?;
            self.started = true;
        }
        let release_us = ORIGIN_US + DELAY_US;
        if !self.released && until_us >= release_us {
            self.check_host()?;
            self.state.now.set(release_us);
            self.control.write_all(b"release\n")?;
            self.state.released_at.set(Some(release_us));
            // Marker means cat returned. Only the real backend reader's
            // second complete frame proves the line is queued for its drain.
            self.written_and_enqueued(&self.later_marker, LATER, enqueued_frames, 2)?;
            self.released = true;
        }
        self.check_host()?;
        self.state.now.set(until_us);
        Ok(())
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        let _ = self.control.write_all(b"abort\n");
        // Backend Drop still owns actual stdin close, child exit and kill/wait.
        // If the child has not opened the FIFO, its existing fallback remains.
    }
}

pub(super) fn backend<'a>(
    rig: &'a Rig,
    now: &[Value],
    delay: &str,
    later: &str,
    then: Then,
) -> PhysicalBackend<'a> {
    assert_eq!(delay, "0.3");
    assert!(matches!(then, Then::StayOpen));
    assert_eq!(now.len(), 1);
    assert_eq!(later.lines().count(), 1);
    assert!(later.ends_with('\n'));
    let (driver, script) = Driver::prepare(rig, now, later);
    let clock = Rc::clone(&driver.state);
    PhysicalBackend::new_with_clock(
        rig.config(script),
        &rig.assets,
        Box::new(Recording(rig.starts.clone())),
        move || MonoTime::from_micros(clock.now.get()),
    )
    .with_fixture_driver(driver)
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    // `final_dispatch`, which loads this file as its `final_drain_clock`.
    use super::super::{session_end, start_line};
    use lamp_voice_eval::{events::EventKind, runner::Backend};
    use std::process::{Child, ExitStatus, Stdio};

    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn wait_until(deadline: Instant, mut observed: impl FnMut() -> bool) {
        loop {
            assert!(Instant::now() < deadline, "real child progress missing");
            let ready = observed();
            assert!(
                Instant::now() < deadline,
                "real child progress was too late"
            );
            if ready {
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn child(script: String) -> ChildGuard {
        ChildGuard(
            Command::new("/bin/sh")
                .args(["-c", &script, "relay"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }

    fn exited(child: &mut ChildGuard, deadline: Instant) -> ExitStatus {
        let mut status = None;
        wait_until(deadline, || {
            status = child.0.try_wait().unwrap();
            status.is_some()
        });
        status.unwrap()
    }

    #[test]
    fn a_complete_real_line_is_released_only_at_the_full_logical_boundary() {
        let rig = Rig::new("disp-clock-boundary");
        let mut backend = backend(
            &rig,
            &[start_line()],
            "0.3",
            &lines(&[session_end()]),
            Then::StayOpen,
        );
        assert_eq!((backend.now_us(), backend.now_us()), (ORIGIN_US, ORIGIN_US));
        rig.begin(&mut backend);
        assert!(
            backend
                .next_event(ORIGIN_US + DELAY_US - 1)
                .unwrap()
                .is_none()
        );
        assert_eq!(backend.now_us(), ORIGIN_US + DELAY_US - 1);
        assert!(!rig.dir.join("later-written").exists());
        let event = backend.next_event(ORIGIN_US + DELAY_US).unwrap().unwrap();
        assert!(matches!(event.kind, EventKind::RunEnd { .. }));
        assert_eq!(backend.now_us(), ORIGIN_US + DELAY_US);
        assert!(marker(&rig.dir.join("later-written"), LATER).unwrap());
        assert_eq!(rig.starts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn abandoning_the_initial_hold_stops_and_reaps_the_actual_shell() {
        let rig = Rig::new("disp-clock-abort");
        let (driver, script) = Driver::prepare(&rig, &[start_line()], &lines(&[session_end()]));
        let deadline = driver.deadline;
        let mut child = child(script);
        wait_until(deadline, || {
            marker(&driver.initial_marker, INITIAL).unwrap()
        });
        drop(driver);
        assert!(exited(&mut child, deadline).success());
        assert!(!rig.dir.join("later-written").exists());
    }

    #[test]
    fn the_released_shell_exits_and_is_reaped_after_actual_stdin_eof() {
        let rig = Rig::new("disp-clock-eof");
        let (mut driver, script) = Driver::prepare(&rig, &[start_line()], &lines(&[session_end()]));
        let deadline = driver.deadline;
        let mut child = child(script);
        wait_until(deadline, || {
            marker(&driver.initial_marker, INITIAL).unwrap()
        });
        driver.control.write_all(b"release\n").unwrap();
        wait_until(deadline, || marker(&driver.later_marker, LATER).unwrap());
        drop(child.0.stdin.take());
        assert!(exited(&mut child, deadline).success());
    }

    #[test]
    fn an_expired_watchdog_cannot_release_data_or_advance_the_clock() {
        let rig = Rig::new("disp-clock-watchdog");
        let (mut driver, _) = Driver::prepare(&rig, &[start_line()], &lines(&[session_end()]));
        driver.deadline = Instant::now();
        let result = driver.wait(
            MonoTime::from_micros(ORIGIN_US + DELAY_US),
            0,
            &AtomicU64::new(0),
            false,
        );
        assert!(result.is_err());
        assert_eq!(driver.state.now.get(), ORIGIN_US);
        assert!(driver.state.released_at.get().is_none());
        assert!(!rig.dir.join("later-written").exists());
    }

    #[test]
    fn a_marker_or_an_enqueue_alone_never_completes_the_observation() {
        let rig = Rig::new("disp-clock-both");
        let (driver, _) = Driver::prepare(&rig, &[start_line()], &lines(&[session_end()]));
        let enqueued = AtomicU64::new(1);
        let ready = || {
            driver
                .written_and_enqueued_once(&driver.initial_marker, INITIAL, &enqueued, 1)
                .unwrap()
        };
        assert!(!ready());
        enqueued.store(0, Ordering::Release);
        fs::write(&driver.initial_marker, INITIAL).unwrap();
        assert!(!ready());
        enqueued.store(1, Ordering::Release);
        fs::write(&driver.initial_marker, b"now-writ").unwrap();
        assert!(!ready());
        fs::write(&driver.initial_marker, INITIAL).unwrap();
        assert!(ready());
        assert_eq!(driver.state.now.get(), ORIGIN_US);
        assert!(driver.state.released_at.get().is_none());
    }

    #[test]
    fn unexpected_marker_bytes_or_extra_frames_fail_the_observation() {
        let rig = Rig::new("disp-clock-invalid");
        let (driver, _) = Driver::prepare(&rig, &[start_line()], &lines(&[session_end()]));
        fs::write(&driver.initial_marker, b"not-the-marker\n").unwrap();
        let enqueued = AtomicU64::new(1);
        assert!(
            driver
                .written_and_enqueued_once(&driver.initial_marker, INITIAL, &enqueued, 1)
                .is_err()
        );
        fs::write(&driver.initial_marker, INITIAL).unwrap();
        enqueued.store(2, Ordering::Release);
        assert!(
            driver
                .written_and_enqueued_once(&driver.initial_marker, INITIAL, &enqueued, 1)
                .is_err()
        );
        assert_eq!(driver.state.now.get(), ORIGIN_US);
        assert!(driver.state.released_at.get().is_none());
    }
}
