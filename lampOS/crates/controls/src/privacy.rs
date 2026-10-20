//! Independent, explicitly selected privacy-slide acquisition. LOW is muted.
//! Nothing here opens audio, clears software mute, or recognizes hand gestures.
use crate::{
    input::InputError,
    pins::{Lines, Pull},
};
use lamp_msgs::{ClockStamp, ControlFault, Diagnostic, PRIVACY_SWITCH_TTL_US, PrivacySwitch};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    ffi::OsStr,
    io,
    time::{Duration, Instant},
};

pub const ENV: &str = "LAMP_PRIVACY_CONTROLS";
pub const POLL_US: u64 = 5_000;
pub const STATE_EVERY_US: u64 = 20_000;
pub const STEP_US: u64 = 1_000;
const MAX_RETIRED: usize = 32;

pub fn selected(value: Option<&OsStr>) -> bool {
    value == Some(OsStr::new("on"))
}

/// `no-slide`: the privacy route on a lamp with no slide. No line is read and
/// a pins file's privacy section is refused; each `controls/state` says so
/// (`no_privacy_slide`), at the route's cadence. Builds before it read this
/// value as off.
pub fn no_slide(value: Option<&OsStr>) -> bool {
    value == Some(OsStr::new("no-slide"))
}

/// Apply only to an explicitly selected privacy route, with a slide or with
/// none. The normal Bus discovery period can exceed the route's freshness
/// budget when a subscriber joins late.
pub fn selected_bus(bus: lamp_bus::Bus) -> lamp_bus::Bus {
    bus.rescan_every(std::time::Duration::from_millis(5))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pins {
    pub chip: String,
    pub line: u32,
    pub active_low: bool,
    pub pull: Pull,
}

impl Pins {
    pub fn check(&self) -> Result<(), String> {
        if self.chip != "/dev/gpiochip1"
            || self.line != 9
            || !self.active_low
            || self.pull != Pull::Up
        {
            return Err("privacy is active-low chip 1 line 9 with pull-up".into());
        }
        Ok(())
    }
}

/// At most one actual acquisition per call, without waiting for a transition.
/// None is no acquisition; it never refreshes the last level.
pub trait Reader {
    fn read(&mut self, now_us: u64) -> Option<PrivacySwitch>;
}

pub struct LineReader<L: Lines> {
    lines: L,
    clock: String,
    session: String,
    sequence: u64,
    next_us: u64,
}

impl<L: Lines> LineReader<L> {
    pub fn new(lines: L, clock: &str, session: &str) -> Self {
        Self {
            lines,
            clock: clock.into(),
            session: session.into(),
            sequence: 0,
            next_us: 0,
        }
    }
}

impl<L: Lines> Reader for LineReader<L> {
    fn read(&mut self, now_us: u64) -> Option<PrivacySwitch> {
        if now_us < self.next_us {
            return None;
        }
        self.next_us = now_us.saturating_add(POLL_US);
        let sequence = self.sequence.checked_add(1)?;
        self.sequence = sequence;
        let (muted, fault) = match self.lines.high(9) {
            Ok(high) => (Some(!high), None),
            Err(_) => (None, Some(ControlFault::Missing)),
        };
        Some(PrivacySwitch {
            stamp: ClockStamp {
                clock_id: self.clock.clone(),
                at_us: now_us,
                sequence,
            },
            reader_session_id: self.session.clone(),
            muted,
            fault,
        })
    }
}

/// An unavailable selected chip remains unknown, never an implicit HIGH.
pub struct MissingLine;
impl Lines for MissingLine {
    fn high(&mut self, _: u32) -> std::io::Result<bool> {
        Err(std::io::Error::other(
            "selected privacy line is unavailable",
        ))
    }
}

/// A requested line's level.
pub trait Level {
    fn high(&mut self) -> io::Result<bool>;
}

/// How often a line that cannot be had is asked for again.
pub const REOPEN_EVERY: Duration = Duration::from_secs(1);

/// The selected line, asked for again at most every [`REOPEN_EVERY`] while
/// it cannot be had: one another process still holds when Controls starts
/// (the voice's direct path, on a lamp moving to this route), or one this
/// user may not open. Until it is had the switch is unknown, never an
/// implicit HIGH. Each new reason it cannot be had is said once, and so is
/// the moment it is taken, so a lamp's log says why the voice waits.
pub struct Reopening<O, L> {
    open: O,
    line: Option<L>,
    tried: Option<Instant>,
    every: Duration,
    said: Option<String>,
    name: String,
}

impl<O: FnMut() -> io::Result<L>, L: Level> Reopening<O, L> {
    pub fn new(name: impl Into<String>, every: Duration, open: O) -> Self {
        Self {
            open,
            line: None,
            tried: None,
            every,
            said: None,
            name: name.into(),
        }
    }

    fn say(&mut self, why: String) {
        if self.said.as_deref() != Some(why.as_str()) {
            eprintln!(
                "lamp-controls: privacy {} cannot be taken, asked again each {} ms: {why}",
                self.name,
                self.every.as_millis()
            );
            self.said = Some(why);
        }
    }
}

impl<O: FnMut() -> io::Result<L>, L: Level> Lines for Reopening<O, L> {
    fn high(&mut self, _: u32) -> io::Result<bool> {
        if self.line.is_none() && self.tried.is_none_or(|at| at.elapsed() >= self.every) {
            self.tried = Some(Instant::now());
            match (self.open)() {
                Ok(line) => {
                    eprintln!("lamp-controls: privacy {} taken", self.name);
                    self.line = Some(line);
                    self.said = None;
                }
                Err(error) => self.say(error.to_string()),
            }
        }
        let Some(line) = &mut self.line else {
            return Err(io::Error::other("selected privacy line is unavailable"));
        };
        // A line that stops answering is let go and asked for again.
        line.high().inspect_err(|error| {
            self.line = None;
            self.say(error.to_string());
        })
    }
}

#[cfg(target_os = "linux")]
pub fn open(pins: &Pins, clock: &str, session: &str) -> Result<Box<dyn Reader>, String> {
    pins.check()?;
    struct PrivacyLine(gpiocdev::Request, u32);
    impl Level for PrivacyLine {
        fn high(&mut self) -> io::Result<bool> {
            self.0
                .value(self.1)
                .map(|v| v == gpiocdev::line::Value::Active)
                .map_err(io::Error::other)
        }
    }
    let (chip, line) = (pins.chip.clone(), pins.line);
    let lines = Reopening::new(format!("line {chip} {line}"), REOPEN_EVERY, move || {
        gpiocdev::Request::builder()
            .on_chip(&chip)
            .with_consumer("lampOS-controls-privacy")
            .with_line(line)
            .as_input()
            .with_bias(gpiocdev::line::Bias::PullUp)
            .request()
            .map(|request| PrivacyLine(request, line))
            .map_err(io::Error::other)
    });
    Ok(Box::new(LineReader::new(lines, clock, session)))
}

#[cfg(not(target_os = "linux"))]
pub fn open(pins: &Pins, clock: &str, session: &str) -> Result<Box<dyn Reader>, String> {
    pins.check()?;
    Ok(Box::new(LineReader::new(MissingLine, clock, session)))
}

/// The small independent acquisition fence is also used by hardware-free replay.
pub struct Monitor {
    clock: String,
    current: PrivacySwitch,
    reader: Option<String>,
    retired: BTreeSet<String>,
    last: Option<(u64, u64)>,
    now: Option<u64>,
    terminal: Option<ControlFault>,
}

impl Monitor {
    pub fn new(clock: &str) -> Self {
        Self {
            clock: clock.into(),
            current: PrivacySwitch {
                stamp: ClockStamp {
                    clock_id: clock.into(),
                    at_us: 0,
                    sequence: 0,
                },
                reader_session_id: "privacy-unobserved".into(),
                muted: None,
                fault: Some(ControlFault::Missing),
            },
            reader: None,
            retired: BTreeSet::new(),
            last: None,
            now: None,
            terminal: None,
        }
    }

    pub fn fault(&mut self, fault: ControlFault) {
        self.current.muted = None;
        self.current.fault = Some(self.terminal.unwrap_or(fault));
    }

    pub fn clock(&self) -> &str {
        &self.clock
    }

    fn advance(&mut self, now: u64) {
        if self.now.is_some_and(|last| now < last) {
            self.terminal = Some(ControlFault::ClockReset);
        }
        self.now = Some(now);
        if let Some(fault) = self.terminal {
            self.fault(fault);
        } else if self.current.muted.is_some()
            && now.saturating_sub(self.current.stamp.at_us) >= PRIVACY_SWITCH_TTL_US
        {
            self.fault(ControlFault::Stale);
        }
    }

    pub fn observe(&mut self, now: u64, sample: PrivacySwitch) -> Result<(), InputError> {
        self.advance(now);
        let result = self.accept(now, sample);
        if let Err(error) = &result {
            self.fault(error.fault);
        }
        result
    }

    fn accept(&mut self, now: u64, sample: PrivacySwitch) -> Result<(), InputError> {
        let fail = |fault, message| InputError::new(fault, message);
        if let Some(fault) = self.terminal {
            return Err(fail(fault, "privacy fence is terminal"));
        }
        sample
            .check()
            .map_err(|_| fail(ControlFault::Invalid, "invalid privacy acquisition"))?;
        if sample.stamp.sequence == 0 {
            return Err(fail(
                ControlFault::Invalid,
                "privacy acquisition sequence is zero",
            ));
        }
        if sample.stamp.clock_id != self.clock {
            return Err(fail(ControlFault::WrongClock, "privacy boot clock differs"));
        }
        let at = sample.stamp.at_us;
        if at > now {
            return Err(fail(
                ControlFault::Future,
                "privacy acquisition is in the future",
            ));
        }
        if now - at >= PRIVACY_SWITCH_TTL_US {
            return Err(fail(ControlFault::Stale, "privacy acquisition is stale"));
        }
        let changed = self
            .reader
            .as_ref()
            .is_some_and(|old| old != &sample.reader_session_id);
        if self.retired.contains(&sample.reader_session_id) {
            return Err(fail(
                ControlFault::ReaderChanged,
                "privacy reader is retired",
            ));
        }
        if let Some((last_at, sequence)) = self.last {
            if at <= last_at || (!changed && sample.stamp.sequence <= sequence) {
                return Err(fail(
                    ControlFault::OutOfOrder,
                    "privacy acquisition did not advance",
                ));
            }
            if !changed && sequence.checked_add(1) != Some(sample.stamp.sequence) {
                self.last = Some((at, sample.stamp.sequence));
                return Err(fail(
                    ControlFault::Overflow,
                    "privacy acquisition sequence gap",
                ));
            }
            if !changed && at - last_at >= PRIVACY_SWITCH_TTL_US {
                self.last = Some((at, sample.stamp.sequence));
                return Err(fail(
                    ControlFault::Stale,
                    "privacy acquisition continuity was lost",
                ));
            }
        }
        if changed {
            if self.retired.len() == MAX_RETIRED {
                self.terminal = Some(ControlFault::Overflow);
                return Err(fail(
                    ControlFault::Overflow,
                    "privacy reader fence exhausted",
                ));
            }
            self.retired
                .insert(self.reader.take().expect("changed reader"));
        }
        self.reader = Some(sample.reader_session_id.clone());
        self.last = Some((at, sample.stamp.sequence));
        self.current = sample;
        if changed {
            return Err(fail(ControlFault::ReaderChanged, "privacy reader changed"));
        }
        Ok(())
    }

    pub fn snapshot(&mut self, now: u64) -> PrivacySwitch {
        self.advance(now);
        self.current.clone()
    }
}

/// The item health shows for the switch itself (the first builder on #2143).
pub const SWITCH_ITEM: &str = "privacy switch";

pub struct Source {
    reader: Box<dyn Reader>,
    pub monitor: Monitor,
    /// The switch's last level read, and how often it has changed since
    /// Controls started, for [`Self::health`].
    moved: (Option<bool>, u64),
}

impl Source {
    pub fn new(reader: Box<dyn Reader>, clock: &str) -> Self {
        Self {
            reader,
            monitor: Monitor::new(clock),
            moved: (None, 0),
        }
    }

    /// Preserve the original acquisition in records, including rejected input.
    pub fn poll(&mut self, now: impl Fn() -> u64) -> Option<(u64, PrivacySwitch)> {
        let sample = self.reader.read(now())?;
        let received = now();
        if let Some(muted) = sample.muted {
            let (last, changes) = &mut self.moved;
            if last.is_some_and(|last| last != muted) {
                *changes += 1;
            }
            *last = Some(muted);
        }
        let _ = self.monitor.observe(received, sample.clone());
        Some((received, sample))
    }

    /// What health says of the switch itself ([`SWITCH_ITEM`]): open or
    /// closed, from its own fresh reading at `now`, and whether it has moved
    /// since Controls started, so a lamp whose switch never changes is
    /// noticed (the first builder on #2143). Either is well; a reading that
    /// can't be had is a warning, with its fault, as Controls' own report
    /// already says the route is closed then.
    pub fn health(&mut self, now: u64) -> Diagnostic {
        let sample = self.monitor.snapshot(now);
        let changes = self.moved.1;
        let report = match sample.muted {
            Some(muted) => {
                let level = if muted { "closed" } else { "open" };
                let message = if changes == 0 {
                    format!("{level}, not moved since Controls started")
                } else {
                    level.to_owned()
                };
                Diagnostic::new(lamp_msgs::Level::Ok, SWITCH_ITEM, &message)
            }
            None => Diagnostic::new(
                lamp_msgs::Level::Warn,
                SWITCH_ITEM,
                &format!("unknown: {:?}", sample.fault),
            ),
        };
        report.with("changes", changes)
    }
}

#[cfg(test)]
mod reopening_tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};

    struct Fixed(io::Result<bool>);
    impl Level for Fixed {
        fn high(&mut self) -> io::Result<bool> {
            match &self.0 {
                Ok(high) => Ok(*high),
                Err(error) => Err(io::Error::new(error.kind(), error.to_string())),
            }
        }
    }

    /// The voice's old direct path still holds the line when Controls
    /// starts (lamp-4ace, 2026-10-16 10:01): it is unknown, asked again
    /// at most each interval, and read as soon as it is free.
    #[test]
    fn a_line_held_at_start_is_asked_again_and_read_once_free() {
        let tries = Rc::new(Cell::new(0));
        let free = Rc::new(Cell::new(false));
        let (counting, freed) = (Rc::clone(&tries), Rc::clone(&free));
        let mut lines = Reopening::new("line test 9", Duration::from_millis(20), move || {
            counting.set(counting.get() + 1);
            if freed.get() {
                Ok(Fixed(Ok(true)))
            } else {
                Err(io::Error::from_raw_os_error(16))
            }
        });
        for _ in 0..5 {
            assert!(lines.high(9).is_err());
        }
        assert_eq!(tries.get(), 1, "not asked again within the interval");
        free.set(true);
        std::thread::sleep(Duration::from_millis(25));
        assert!(lines.high(9).unwrap());
        assert_eq!(tries.get(), 2);
        assert!(lines.high(9).unwrap());
        assert_eq!(tries.get(), 2, "kept once taken");
    }

    #[test]
    fn a_line_that_stops_answering_is_let_go_and_asked_for_again() {
        let tries = Rc::new(Cell::new(0));
        let counting = Rc::clone(&tries);
        let mut lines = Reopening::new("line test 9", Duration::from_millis(1), move || {
            counting.set(counting.get() + 1);
            Ok(Fixed(if counting.get() == 1 {
                Err(io::Error::other("gone"))
            } else {
                Ok(false)
            }))
        });
        assert!(lines.high(9).is_err());
        std::thread::sleep(Duration::from_millis(2));
        assert!(!lines.high(9).unwrap());
        assert_eq!(tries.get(), 2);
    }

    #[test]
    fn until_the_line_is_had_each_acquisition_is_unknown_with_a_fault() {
        let lines = Reopening::new("line test 9", Duration::from_secs(60), || {
            Err::<Fixed, _>(io::Error::from_raw_os_error(13))
        });
        let mut reader = LineReader::new(lines, "boot", "session");
        let sample = reader.read(1).unwrap();
        assert_eq!(
            (sample.muted, sample.fault),
            (None, Some(ControlFault::Missing))
        );
    }
}
