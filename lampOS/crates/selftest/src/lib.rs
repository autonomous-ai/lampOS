//! Whether each part of the lamp works. See `docs/selftest.md`.
//!
//! - [`Check`]: one part's check, asked through the part's own interface
//!   (the bus, its owner process, the system's files). A check never writes
//!   to a device another process owns.
//! - [`run`]: every check in turn, each timed, the whole inside a budget.
//! - [`listen`]: the checks that only listen to a part on the bus.
//! - [`kernel`]: the storage, from what the kernel said since boot.
//! - [`ring`]: the ring's owner, listened to.
//! - [`mic`]: a second of the voice microphone, and its level.
//! - [`hand`]: touch by hand, at the factory.
//! - [`speaker`]: the echo the voice's canceller removed from the lamp's own speech.
//! - [`head`]: the head's small move, at rest, through `expression/body`.
//! - [`report`]: where a run is kept, and what a person reads.
//! - [`quiet`]: a passive run waits for the voice to be out of a turn.

// Nothing in this library prints; the program prints what it is given.
#![deny(clippy::print_stderr, clippy::print_stdout, clippy::dbg_macro)]

use lamp_msgs::{Occasion, PartResult, SelfTest, Verdict};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub mod hand;
pub mod head;
pub mod kernel;
pub mod listen;
pub mod mic;
pub mod quiet;
pub mod report;
pub mod ring;
pub mod speaker;

/// The whole self-test has two minutes (#812).
pub const BUDGET: Duration = Duration::from_secs(120);
/// How long a passive run waits before it checks again the parts it
/// failed (the reviewer on #1245): a part restarting at that moment, or a
/// hand resting on a pad, is over by then.
pub const AGAIN_AFTER: Duration = Duration::from_secs(30);

/// One part's check.
pub trait Check {
    /// The part's name in the report.
    fn part(&self) -> &str;

    /// Checks the part, giving up by `deadline`. The result's `took` is
    /// filled in by [`run`].
    fn run(&mut self, deadline: Instant) -> PartResult;
}

/// The build a self-test program is, from its own path as installed:
/// `/opt/lampOS/selftest/<build>/lamp-selftest`, where `<build>` is the
/// folder `current` links to (`current_exe` gives the link's target). Any
/// other path, a test's or one run by hand, is no build (#1399).
pub fn build_of(program: &std::path::Path) -> Option<String> {
    let folder = program.parent()?;
    let build = folder.file_name()?.to_str()?;
    let part = folder.parent()?.file_name()?.to_str()?;
    (part == "selftest"
        && build != "current"
        && !build.is_empty()
        && build.len() <= 128
        && !build.chars().any(char::is_control))
    .then(|| build.to_owned())
}

/// Where the kernel keeps the lamp's host name, as `lamp-cloud`'s `info`
/// reads it (`crates/cloud/src/lamp.rs`, `Real::hostname`).
pub const HOSTNAME: &str = "/proc/sys/kernel/hostname";

/// The lamp's name for its report, from the kernel's host name at `path`
/// ([`HOSTNAME`]): `lamp-` and the last four of its serial or Ethernet
/// address, which the image's hotspot script gives it at the first boot.
/// None when it cannot be read or is not a host name, so a run is never
/// refused for it.
pub fn lamp_of(path: &std::path::Path) -> Option<String> {
    let name = std::fs::read_to_string(path).ok()?;
    let name = name.trim();
    SelfTest::is_lamp_name(name).then(|| name.to_owned())
}

/// Runs every check in order. A check that starts after the budget is spent
/// is not run and says so; one that overruns its deadline keeps its result.
pub fn run(occasion: Occasion, checks: &mut [Box<dyn Check + '_>], budget: Duration) -> SelfTest {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let begin = Instant::now();
    let deadline = begin + budget;
    let mut parts = Vec::with_capacity(checks.len());
    for check in checks.iter_mut() {
        let at = Instant::now();
        let mut result = if at >= deadline {
            PartResult::new(
                check.part(),
                Verdict::NotChecked,
                "the self-test ran out of time before this part",
            )
        } else {
            check.run(deadline)
        };
        if result.part != check.part() {
            result.part = check.part().to_owned();
        }
        result.took = at.elapsed().as_secs_f64();
        parts.push(result);
    }
    SelfTest {
        occasion,
        started,
        took: begin.elapsed().as_secs_f64(),
        verdict: SelfTest::verdict_of(&parts),
        parts,
        // The program's own build (#1399).
        build: std::env::current_exe().ok().as_deref().and_then(build_of),
        // The lamp it ran on, so a test station can match it.
        lamp: lamp_of(std::path::Path::new(HOSTNAME)),
    }
}

/// Checks again, inside `budget`, each part `first` failed, and keeps its
/// new result in its place: a part fails only if it failed twice. Every
/// other part keeps its first result; the run keeps its start, and its
/// verdict and length are worked out again.
pub fn again(
    mut first: SelfTest,
    checks: &mut [Box<dyn Check + '_>],
    budget: Duration,
) -> SelfTest {
    let failed: Vec<String> = first
        .parts
        .iter()
        .filter(|part| part.verdict == Verdict::Fail)
        .map(|part| part.part.clone())
        .collect();
    let begin = Instant::now();
    let mut those: Vec<&mut Box<dyn Check + '_>> = checks
        .iter_mut()
        .filter(|check| failed.iter().any(|part| part == check.part()))
        .collect();
    let deadline = begin + budget;
    for check in those.iter_mut() {
        let at = Instant::now();
        let mut result = if at >= deadline {
            PartResult::new(
                check.part(),
                Verdict::NotChecked,
                "the self-test ran out of time before checking this part again",
            )
        } else {
            check.run(deadline)
        };
        result.part = check.part().to_owned();
        result.took = at.elapsed().as_secs_f64();
        if let Some(kept) = first.parts.iter_mut().find(|part| part.part == result.part) {
            *kept = result;
        }
    }
    first.took += begin.elapsed().as_secs_f64();
    first.verdict = SelfTest::verdict_of(&first.parts);
    first
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A part that answers what it is told to, after a while.
    struct Fake {
        part: &'static str,
        verdict: Verdict,
        wait: Duration,
        ran: bool,
    }

    impl Fake {
        fn boxed(part: &'static str, verdict: Verdict, wait: Duration) -> Box<dyn Check> {
            Box::new(Self {
                part,
                verdict,
                wait,
                ran: false,
            })
        }
    }

    impl Check for Fake {
        fn part(&self) -> &str {
            self.part
        }

        fn run(&mut self, _deadline: Instant) -> PartResult {
            self.ran = true;
            std::thread::sleep(self.wait);
            let reason = if self.verdict == Verdict::Pass {
                ""
            } else {
                "told to"
            };
            PartResult::new("someone else", self.verdict, reason)
        }
    }

    #[test]
    fn every_part_is_checked_in_order_and_timed() {
        let mut checks = vec![
            Fake::boxed("camera", Verdict::Pass, Duration::from_millis(20)),
            Fake::boxed("ring", Verdict::Fail, Duration::ZERO),
            Fake::boxed("air", Verdict::NotFitted, Duration::ZERO),
        ];
        let test = run(Occasion::OnDemand, &mut checks, BUDGET);
        let names: Vec<_> = test.parts.iter().map(|p| p.part.as_str()).collect();
        assert_eq!(names, ["camera", "ring", "air"]);
        assert!(test.parts[0].took >= 0.02, "{}", test.parts[0].took);
        assert_eq!(test.verdict, Verdict::Fail);
        assert!(test.took >= test.parts[0].took);
        assert_eq!(test.check(), Ok(()));
    }

    #[test]
    fn a_part_reached_after_the_budget_is_not_checked_and_says_why() {
        let mut checks = vec![
            Fake::boxed("speaker", Verdict::Pass, Duration::from_millis(30)),
            Fake::boxed("ring", Verdict::Pass, Duration::ZERO),
        ];
        let test = run(Occasion::Factory, &mut checks, Duration::from_millis(10));
        assert_eq!(test.parts[0].verdict, Verdict::Pass);
        assert_eq!(test.parts[1].verdict, Verdict::NotChecked);
        assert!(test.parts[1].reason.contains("out of time"));
        assert_eq!(test.verdict, Verdict::NotChecked);
        assert_eq!(test.check(), Ok(()));
    }

    /// The reviewer on #1245: a part that fails once (a part restarting at
    /// that moment, a hand on a pad) is checked again and passes; one that
    /// fails again stays failed; the others are not run again and keep
    /// their first results.
    #[test]
    fn only_a_part_that_fails_twice_is_failed() {
        let mut first = run(
            Occasion::FirstBoot,
            &mut [
                Fake::boxed("ring", Verdict::Fail, Duration::ZERO),
                Fake::boxed("touch", Verdict::Fail, Duration::ZERO),
                Fake::boxed("air", Verdict::Pass, Duration::ZERO),
            ],
            BUDGET,
        );
        assert_eq!(first.verdict, Verdict::Fail);
        first.started = 7.0;
        let mut later: Vec<Box<dyn Check>> = vec![
            Fake::boxed("ring", Verdict::Pass, Duration::ZERO),
            Fake::boxed("touch", Verdict::Fail, Duration::ZERO),
            Fake::boxed("air", Verdict::Fail, Duration::ZERO),
        ];
        let checked = again(first, &mut later, BUDGET);
        let verdicts: Vec<(&str, Verdict)> = checked
            .parts
            .iter()
            .map(|part| (part.part.as_str(), part.verdict))
            .collect();
        // air passed the first time and is not run again.
        assert_eq!(
            verdicts,
            [
                ("ring", Verdict::Pass),
                ("touch", Verdict::Fail),
                ("air", Verdict::Pass)
            ]
        );
        assert_eq!(checked.verdict, Verdict::Fail);
        assert_eq!(checked.started, 7.0);
        assert_eq!(checked.check(), Ok(()));
        let mut once = run(
            Occasion::FirstBoot,
            &mut [Fake::boxed("ring", Verdict::Fail, Duration::ZERO)],
            BUDGET,
        );
        once = again(
            once,
            &mut [Fake::boxed("ring", Verdict::Pass, Duration::ZERO)],
            BUDGET,
        );
        assert_eq!(once.verdict, Verdict::Pass);
    }

    /// #1399: the build is the program's folder under `selftest/`, as
    /// installed; a program anywhere else, or reached through `current`, is
    /// no build.
    #[test]
    fn the_build_is_the_programs_own_folder_as_installed() {
        use std::path::Path;
        assert_eq!(
            build_of(Path::new("/opt/lampOS/selftest/0e8211e769ed/lamp-selftest")).as_deref(),
            Some("0e8211e769ed")
        );
        for other in [
            "/opt/lampOS/selftest/current/lamp-selftest",
            "/home/someone/target/debug/lamp-selftest",
            "/opt/lampOS/health/0e8211e769ed/lamp-selftest",
            "lamp-selftest",
        ] {
            assert_eq!(build_of(Path::new(other)), None, "{other}");
        }
    }

    /// The report names its lamp by the kernel's host name, read as
    /// `lamp-cloud` reads it; one that cannot be read, or is not a host
    /// name, is no name, and a run checked again keeps its first one.
    #[test]
    fn a_run_names_the_lamp_by_its_host_name() {
        let dir = std::env::temp_dir().join(format!("lamp-selftest-lamp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("hostname");
        std::fs::write(&file, "lamp-4ace\n").unwrap();
        assert_eq!(lamp_of(&file).as_deref(), Some("lamp-4ace"));
        for refused in ["", "\n", "lamp 4ace\n", "two\nlines\n"] {
            std::fs::write(&file, refused).unwrap();
            assert_eq!(lamp_of(&file), None, "{refused:?}");
        }
        assert_eq!(lamp_of(&dir.join("missing")), None);
        std::fs::remove_dir_all(&dir).unwrap();

        let mut first = run(
            Occasion::Factory,
            &mut [Fake::boxed("ring", Verdict::Fail, Duration::ZERO)],
            BUDGET,
        );
        assert_eq!(first.lamp, lamp_of(std::path::Path::new(HOSTNAME)));
        assert_eq!(first.check(), Ok(()));
        first.lamp = Some("lamp-4ace".into());
        let checked = again(
            first,
            &mut [Fake::boxed("ring", Verdict::Pass, Duration::ZERO)],
            BUDGET,
        );
        assert_eq!(checked.lamp.as_deref(), Some("lamp-4ace"));
    }
}
