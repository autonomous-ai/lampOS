//! What each stage of the capture worker's pass costs its thread, with
//! `LAMP_CAPTURE_STAGES=on` (off unless set, passed to capture only): one
//! journal line a minute.
//!
//! On lamp-4ace on 2026-10-18 `worker capture` took 18 % of a core with
//! nobody there, while the same code benched flat out sums to ~80 µs a block
//! on a Mac, 2–6 % at the lamp's scale. Either the stages cost more on the
//! lamp than the bench says, or the clock the idle worker gets is low. This
//! says which stage, in the thread's own CPU time
//! (`CLOCK_THREAD_CPUTIME_ID`), so the time it waited or was preempted is
//! not counted.
//!
//! The stages: the microphone's read; the reference (packets accepted and
//! each block's step, which render into the canceller and the echo
//! evidence's observer); the canceller's capture (AEC3, and with it the echo
//! evidence's search that `LAMP_QUIET_EVIDENCE` skips and the steady
//! reference); the speech detector; the frame's encode and send; the rest of
//! each pass (control, sockets, authority); and the wait between passes.
//! The line also gives the thread's whole share over the minute against the
//! wall clock, so the stages can be read beside `scaling_cur_freq`.
//!
//! Its own cost, on: four clock reads a pass (the read is timed on every pass
//! while the microphone runs), eight a block and two a reference packet, each
//! a system call on Linux, and each stage's time carries about one of them.
//! Off, a read is one branch and nothing is printed.
use std::time::{Duration, Instant};

pub const ENVIRONMENT: &str = "LAMP_CAPTURE_STAGES";

/// Whether this worker times its stages, read once.
pub fn on() -> bool {
    std::env::var_os(ENVIRONMENT).as_deref() == Some(std::ffi::OsStr::new("on"))
}

/// How often the line is said.
pub const EVERY: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Read,
    Reference,
    Canceller,
    Detector,
    Encode,
    /// What a pass did beyond the stages above.
    Rest,
    /// From the end of one pass's work to the start of the next.
    Wait,
}

const STAGES: [(Stage, &str); 7] = [
    (Stage::Read, "read"),
    (Stage::Reference, "reference"),
    (Stage::Canceller, "canceller"),
    (Stage::Detector, "detector"),
    (Stage::Encode, "encode"),
    (Stage::Rest, "rest of pass"),
    (Stage::Wait, "wait"),
];

/// Thread CPU time in nanoseconds, summed per stage over the minute.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sums {
    pub passes: u64,
    pub blocks: u64,
    /// Each stage's time and calls, in [`STAGES`]' order.
    pub stages: [(u64, u64); 7],
}

impl Sums {
    fn add(&mut self, stage: Stage, ns: u64) {
        let slot = &mut self.stages[stage as usize];
        slot.0 = slot.0.saturating_add(ns);
        slot.1 += 1;
    }
}

/// One loop's sums, on with `LAMP_CAPTURE_STAGES=on`.
pub struct StageCost {
    on: bool,
    since: Instant,
    sums: Sums,
    /// The pass's start and what its named stages took, for its rest.
    pass: Option<(u64, u64)>,
    /// The end of the last pass's work.
    last_end: Option<u64>,
}

impl StageCost {
    pub fn new(on: bool) -> Self {
        Self {
            on,
            since: Instant::now(),
            sums: Sums::default(),
            pass: None,
            last_end: None,
        }
    }

    /// This thread's CPU time now, 0 when off.
    pub fn now(&self) -> u64 {
        if !self.on {
            return 0;
        }
        let time = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
        u64::try_from(time.tv_sec)
            .unwrap_or(0)
            .saturating_mul(1_000_000_000)
            .saturating_add(u64::try_from(time.tv_nsec).unwrap_or(0))
    }

    /// A pass begins: the wait since the last one's work is counted.
    pub fn begin(&mut self) {
        if !self.on {
            return;
        }
        let now = self.now();
        if let Some(end) = self.last_end.take() {
            self.sums.add(Stage::Wait, now.saturating_sub(end));
        }
        self.pass = Some((now, 0));
    }

    /// A stage that began at `from` (a [`Self::now`]) has ended.
    pub fn stage(&mut self, stage: Stage, from: u64) {
        if !self.on {
            return;
        }
        let ns = self.now().saturating_sub(from);
        self.sums.add(stage, ns);
        if let Some((_, named)) = self.pass.as_mut() {
            *named = named.saturating_add(ns);
        }
    }

    /// A block was handed on.
    pub fn block(&mut self) {
        if self.on {
            self.sums.blocks += 1;
        }
    }

    /// A pass's work has ended, before its wait. Each minute, the line.
    pub fn done(&mut self) -> Option<String> {
        if !self.on {
            return None;
        }
        let now = self.now();
        if let Some((began, named)) = self.pass.take() {
            self.sums.passes += 1;
            self.sums
                .add(Stage::Rest, now.saturating_sub(began).saturating_sub(named));
        }
        self.last_end = Some(now);
        let over = self.since.elapsed();
        if over < EVERY {
            return None;
        }
        let line = line(&self.sums, over, cpu());
        self.sums = Sums::default();
        self.since = Instant::now();
        Some(line)
    }
}

/// The CPU the thread is on as the line is said: the lamp's small and big
/// cores differ about 2.7 times (lamp-4ace, 18 October), so a minute's
/// numbers are read with it. One system call a minute.
#[cfg(target_os = "linux")]
fn cpu() -> Option<usize> {
    Some(rustix::thread::sched_getcpu())
}

#[cfg(not(target_os = "linux"))]
fn cpu() -> Option<usize> {
    None
}

/// The journal's line for one minute's sums.
pub fn line(sums: &Sums, over: Duration, cpu: Option<usize>) -> String {
    let total: u64 = sums.stages.iter().map(|(ns, _)| ns).sum();
    let wall = over.as_nanos().max(1) as f64;
    let mut out = format!(
        "lamp-voice: capture: thread CPU {:.1} % on CPU {} over the last {} s, {} passes, {} blocks:",
        total as f64 * 100.0 / wall,
        cpu.map_or_else(|| "-".to_owned(), |cpu| cpu.to_string()),
        over.as_secs(),
        sums.passes,
        sums.blocks,
    );
    for (index, (_, name)) in STAGES.iter().enumerate() {
        let (ns, calls) = sums.stages[index];
        out.push_str(&format!(
            " {name} {} ms ({:.1} µs each of {calls}),",
            ns / 1_000_000,
            ns.checked_div(calls).unwrap_or(0) as f64 / 1_000.0,
        ));
    }
    out.pop();
    out.push_str(" (LAMP_CAPTURE_STAGES=on)");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_reads_nothing_and_says_nothing() {
        let mut cost = StageCost::new(false);
        cost.begin();
        assert_eq!(cost.now(), 0);
        cost.stage(Stage::Canceller, 0);
        assert_eq!(cost.done(), None);
        assert_eq!(cost.sums, Sums::default());
    }

    #[test]
    fn a_pass_counts_its_stages_its_rest_and_the_wait_before_the_next() {
        let mut cost = StageCost::new(true);
        cost.begin();
        let from = cost.now();
        // Spun on the thread's own clock, so a preempted runner can't leave
        // it short.
        while cost.now() - from < 2_000_000 {}
        cost.stage(Stage::Canceller, from);
        cost.block();
        assert_eq!(cost.done(), None);
        cost.begin();
        let sums = cost.sums;
        assert_eq!((sums.passes, sums.blocks), (1, 1));
        let canceller = sums.stages[Stage::Canceller as usize];
        assert_eq!(canceller.1, 1);
        assert!(canceller.0 >= 2_000_000, "{canceller:?}");
        // The rest is the pass without its named stages, so far under the
        // canceller's 2 ms spin.
        let rest = sums.stages[Stage::Rest as usize];
        assert_eq!(rest.1, 1);
        assert!(rest.0 < 1_000_000, "{rest:?} against {canceller:?}");
        assert_eq!(sums.stages[Stage::Wait as usize].1, 1);
        assert_eq!(sums.stages[Stage::Detector as usize], (0, 0));
    }

    #[test]
    fn the_line_gives_the_share_and_each_stage_per_call() {
        let mut sums = Sums {
            passes: 30_000,
            blocks: 6_000,
            ..Sums::default()
        };
        sums.stages[Stage::Canceller as usize] = (3_000_000_000, 6_000);
        sums.stages[Stage::Detector as usize] = (900_000_000, 6_000);
        let line = line(&sums, Duration::from_secs(60), Some(6));
        assert!(
            line.starts_with(
                "lamp-voice: capture: thread CPU 6.5 % on CPU 6 over the last 60 s, 30000 passes, 6000 blocks: read 0 ms (0.0 µs each of 0),"
            ),
            "{line}"
        );
        assert!(
            line.contains(
                " canceller 3000 ms (500.0 µs each of 6000), detector 900 ms (150.0 µs each of 6000),"
            ),
            "{line}"
        );
        assert!(
            line.ends_with(" wait 0 ms (0.0 µs each of 0) (LAMP_CAPTURE_STAGES=on)"),
            "{line}"
        );
    }
}
