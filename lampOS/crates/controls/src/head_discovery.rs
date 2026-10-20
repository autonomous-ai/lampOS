//! Bounded bench evidence for the four possible head inputs. A driven idle
//! level plus pulses is a candidate, not proof of a physical pad or permission
//! to install a mapping. Nothing here changes live Controls configuration.
//! With a hands-away phase first, each line's changes at rest are counted, so
//! the factory step can refuse a line whose rest does not hold, and each
//! candidate says how a touch shows on it (#711, #1764).
use crate::touch_pins::{HEAD_CANDIDATES, Reads};
use serde::Serialize;
use std::io;

pub const SETTLE_US: u64 = 500_000;
pub const EDGE_US: u64 = 20_000;
pub const FRESH_US: u64 = 100_000;
/// A candidate whose longest pulse is at least this held its level under a
/// hand. The touch's own longest tap. One press this long also finds the
/// pad, as two pulses do: a held hand is the clearest touch (thirteen-P4).
pub const HELD_US: u64 = 500_000;
/// A change of a line this soon after its last one, and no sooner than
/// [`EDGE_US`], is a fast one: a TTP223 pulsing under a hand (50-100 ms) makes
/// them many times a second. As the touch engine's pulse gap.
pub const FAST_US: u64 = 250_000;
/// A candidate pulsed under a hand only after this many fast changes in its
/// touches: two quick taps on a pad that holds its level make none, and with
/// neither these nor a held level it says no `reads`, so the factory step
/// asks again (the reviewer on #1965).
pub const BURST: u64 = 4;

#[derive(Clone, Debug, Serialize)]
pub struct LineEvidence {
    pub line: u32,
    pub high_with_pull_down: bool,
    pub high_with_pull_up: bool,
    pub idle_high: Option<bool>,
    pub completed_pulses: u64,
    pub longest_pulse_us: u64,
    pub active_low_candidate: Option<bool>,
    /// How many times the line changed in the hands-away phase; none
    /// without one.
    pub rest_changes: Option<u64>,
    /// How many of the line's changes in the touches came 20-250 ms after
    /// the one before.
    pub fast_changes: u64,
    /// How a touch showed on a candidate: its level held for 500 ms, or
    /// [`BURST`] fast changes; neither, none.
    pub reads: Option<Reads>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub lines: Vec<LineEvidence>,
    pub observations: u64,
    pub gaps: u64,
    pub started_us: u64,
    pub last_us: Option<u64>,
    pub hardware_mapping_installed: bool,
    /// The hands-away phase, after the settle.
    pub rest_us: u64,
}

#[derive(Default)]
struct Contact {
    armed: bool,
    released_since: Option<u64>,
    pressed_since: Option<u64>,
    release_edge: Option<u64>,
}

pub struct Discovery {
    report: Report,
    contacts: [Contact; 4],
    /// Each line's last level read in the hands-away phase.
    resting: [Option<bool>; 4],
    /// Each line's last level in the touches, and when it last changed.
    touched: [(Option<bool>, Option<u64>); 4],
}

impl Discovery {
    pub fn new(started_us: u64, probes: [(bool, bool); 4]) -> io::Result<Self> {
        Self::with_rest(started_us, probes, 0)
    }

    /// As [`Discovery::new`], with `rest_us` of hands away after the settle.
    pub fn with_rest(started_us: u64, probes: [(bool, bool); 4], rest_us: u64) -> io::Result<Self> {
        if started_us > u64::MAX - 60_000_000 || rest_us > 30_000_000 {
            return Err(io::Error::other("discovery clock out of range"));
        }
        Ok(Self {
            report: Report {
                lines: HEAD_CANDIDATES
                    .into_iter()
                    .zip(probes)
                    .map(|(line, (down, up))| LineEvidence {
                        line,
                        high_with_pull_down: down,
                        high_with_pull_up: up,
                        idle_high: (down == up).then_some(up),
                        completed_pulses: 0,
                        longest_pulse_us: 0,
                        active_low_candidate: None,
                        rest_changes: (rest_us > 0).then_some(0),
                        fast_changes: 0,
                        reads: None,
                    })
                    .collect(),
                observations: 0,
                gaps: 0,
                started_us,
                last_us: None,
                hardware_mapping_installed: false,
                rest_us,
            },
            contacts: std::array::from_fn(|_| Contact::default()),
            resting: [None; 4],
            touched: [(None, None); 4],
        })
    }

    pub fn ingest(&mut self, at_us: u64, high_mask: u8) -> io::Result<()> {
        if high_mask & !15 != 0
            || at_us < self.report.started_us
            || self.report.last_us.is_some_and(|last| at_us <= last)
        {
            self.contacts = std::array::from_fn(|_| Contact::default());
            return Err(io::Error::other(
                "invalid or unordered head discovery acquisition",
            ));
        }
        if self
            .report
            .last_us
            .is_some_and(|last| at_us - last >= FRESH_US)
        {
            self.report.gaps += 1;
            self.contacts = std::array::from_fn(|_| Contact::default());
            self.touched = [(None, None); 4];
        }
        self.report.last_us = Some(at_us);
        self.report.observations += 1;
        let rest_us = self.report.rest_us;
        for (index, (((line, contact), resting), touched)) in self
            .report
            .lines
            .iter_mut()
            .zip(&mut self.contacts)
            .zip(&mut self.resting)
            .zip(&mut self.touched)
            .enumerate()
        {
            let Some(idle) = line.idle_high else {
                continue;
            };
            let high = high_mask & (1 << index) != 0;
            let active = high != idle;
            let since = at_us - self.report.started_us;
            if since < SETTLE_US {
                *contact = Contact::default();
                continue;
            }
            if since < SETTLE_US + rest_us {
                // Hands away: any change from its probed rest, or since, is
                // a rest that does not hold.
                if resting.replace(high).unwrap_or(idle) != high {
                    line.rest_changes = line.rest_changes.map(|n| n + 1);
                }
                *contact = Contact::default();
                continue;
            }
            if touched.0.replace(high).is_some_and(|was| was != high) {
                if touched
                    .1
                    .is_some_and(|last| (EDGE_US..=FAST_US).contains(&(at_us - last)))
                {
                    line.fast_changes += 1;
                }
                touched.1 = Some(at_us);
            }
            if line.active_low_candidate.is_some() {
                line.reads = if line.longest_pulse_us >= HELD_US {
                    Some(Reads::Level)
                } else if line.fast_changes >= BURST {
                    Some(Reads::Pulses)
                } else {
                    None
                };
            }
            if !contact.armed {
                if active {
                    contact.released_since = None;
                } else {
                    let since = *contact.released_since.get_or_insert(at_us);
                    if at_us - since >= EDGE_US {
                        contact.armed = true;
                    }
                }
                continue;
            }
            if active {
                // A bounce interrupts release qualification; its elapsed gap
                // cannot increase the duration of the preceding active pulse.
                if contact.release_edge.take().is_some() {
                    contact.pressed_since = Some(at_us);
                } else {
                    contact.pressed_since.get_or_insert(at_us);
                }
                continue;
            }
            let Some(pressed) = contact.pressed_since else {
                continue;
            };
            let released = *contact.release_edge.get_or_insert(at_us);
            if at_us - released >= EDGE_US {
                let duration = released - pressed;
                if duration >= EDGE_US {
                    line.completed_pulses += 1;
                    line.longest_pulse_us = line.longest_pulse_us.max(duration);
                    // Two presses, or one held: a person's two touches on
                    // a pad merge into one long one more often than not
                    // (lamp-c0a4, three runs, #1965; thirteen-P4).
                    if line.completed_pulses >= 2 || duration >= HELD_US {
                        line.active_low_candidate = Some(idle);
                        line.reads = if line.longest_pulse_us >= HELD_US {
                            Some(Reads::Level)
                        } else if line.fast_changes >= BURST {
                            Some(Reads::Pulses)
                        } else {
                            None
                        };
                    }
                }
                contact.pressed_since = None;
                contact.release_edge = None;
            }
        }
        Ok(())
    }

    pub fn report(&self) -> &Report {
        &self.report
    }
}

#[cfg(target_os = "linux")]
pub fn run(seconds: u64, rest_seconds: u64) -> io::Result<Report> {
    use crate::pins::{GpioLines, Lines, Pin, PinsConfig, Pull};
    use std::{thread, time::Duration};
    if !(2..=60).contains(&seconds) || rest_seconds >= seconds {
        return Err(io::Error::other(
            "head discovery lasts 2..60 seconds, its rest less",
        ));
    }
    let chip = "/dev/gpiochip0";
    let mut probes = [(false, false); 4];
    for (index, line) in HEAD_CANDIDATES.iter().enumerate() {
        probes[index] = GpioLines::probe(chip, *line)?;
    }
    let wiring = PinsConfig {
        privacy: None,
        chip: chip.into(),
        buttons: vec![],
        every_ms: 5,
        head_pads: HEAD_CANDIDATES
            .into_iter()
            .map(|line| Pin {
                name: format!("candidate-{line}"),
                line,
                active_low: false,
                pull: Pull::Up,
            })
            .collect(),
    };
    let mut lines = GpioLines::open(&wiring)?;
    let started = lamp_ipc::monotonic_us();
    let mut discovery = Discovery::with_rest(started, probes, rest_seconds * 1_000_000)?;
    while lamp_ipc::monotonic_us() - started < seconds * 1_000_000 {
        let before = lamp_ipc::monotonic_us();
        let mut mask = 0;
        for (index, line) in HEAD_CANDIDATES.iter().enumerate() {
            if lines.high(*line)? {
                mask |= 1 << index;
            }
        }
        if lamp_ipc::monotonic_us() - before >= FRESH_US {
            return Err(io::Error::other(
                "head discovery GPIO read took at least 100 ms",
            ));
        }
        discovery.ingest(before, mask)?;
        thread::sleep(Duration::from_millis(5));
    }
    Ok(discovery.report)
}

#[cfg(not(target_os = "linux"))]
pub fn run(_: u64, _: u64) -> io::Result<Report> {
    Err(io::Error::other("head discovery requires Linux GPIO"))
}
