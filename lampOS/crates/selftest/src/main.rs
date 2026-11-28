//! `lamp-selftest`: checks each part of the lamp once, keeps the report
//! under /var/lib/lampOS/selftest/, publishes it on `lamp/selftest`, and
//! prints one line a part (docs/selftest.md).
use lamp_bus::Bus;
use lamp_msgs::{Occasion, SelfTest, Verdict};
use lamp_selftest::{
    AGAIN_AFTER, BUDGET, hand, head, kernel, listen, mic, quiet, report, ring, speaker, voice_route,
};
use std::{path::PathBuf, process::ExitCode, time::Instant};

const USAGE: &str = "usage: lamp-selftest [--occasion factory|first-boot|on-demand] [--dir ABSOLUTE] [--bus-root ABSOLUTE]";
const HELP: &str = "Checks each part of the lamp within two minutes and says pass, fail,
not fitted or not checked for each, with one reason. The report is kept in
--dir (default /var/lib/lampOS/selftest) and published on lamp/selftest.

Exit status: 0 every part passed or is not fitted, 1 a part failed,
3 a part could not be checked, 2 the arguments were wrong.";

struct Arguments {
    occasion: Occasion,
    dir: PathBuf,
    bus_root: Option<PathBuf>,
}

fn parse(given: &[String]) -> Result<Option<Arguments>, String> {
    let mut arguments = Arguments {
        occasion: Occasion::OnDemand,
        dir: report::DIR.into(),
        bus_root: None,
    };
    let mut given = given.iter();
    while let Some(flag) = given.next() {
        let mut value = || given.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "-h" | "--help" => return Ok(None),
            "--occasion" => {
                arguments.occasion = match value()?.as_str() {
                    "factory" => Occasion::Factory,
                    "first-boot" => Occasion::FirstBoot,
                    "on-demand" => Occasion::OnDemand,
                    other => return Err(format!("no occasion {other:?}")),
                }
            }
            "--dir" => arguments.dir = absolute(flag, value()?)?,
            "--bus-root" => arguments.bus_root = Some(absolute(flag, value()?)?),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(Some(arguments))
}

fn absolute(flag: &str, value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(format!("{flag} must be an absolute path"))
    }
}

fn main() -> ExitCode {
    // Its log lines never block it or end it (`lamp_ipc::log_relay`).
    let _log = lamp_ipc::log_relay::guard();
    let given: Vec<String> = std::env::args().skip(1).collect();
    let arguments = match parse(&given) {
        Ok(Some(arguments)) => arguments,
        Ok(None) => {
            println!("{USAGE}\n\n{HELP}");
            return ExitCode::SUCCESS;
        }
        Err(why) => {
            eprintln!("lamp-selftest: {why}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let started = Instant::now();
    let bus = match &arguments.bus_root {
        Some(root) => Bus::new(root),
        None => Bus::from_environment(),
    };
    let bus = match bus {
        Ok(bus) => bus,
        Err(error) => {
            eprintln!("lamp-selftest: the bus cannot be opened: {error}");
            return ExitCode::FAILURE;
        }
    };
    // A passive run never begins inside a turn (#812): it waits for the
    // voice to rest, and leaves the run for the next time if it does not.
    if arguments.occasion == Occasion::FirstBoot
        && !quiet::wait(|| quiet::read(&bus), std::thread::sleep)
    {
        eprintln!(
            "lamp-selftest: the voice was in a turn for {} s; this run is left for the next time",
            quiet::MOST.as_secs()
        );
        return ExitCode::SUCCESS;
    }
    let mut checks = listen::checks(&bus);
    checks.insert(0, Box::new(mic::VoiceMic { bus: &bus }));
    // The colours are asked for at the factory and on demand; at first
    // boot the ring is only listened to.
    checks.push(Box::new(ring::Ring {
        bus: &bus,
        active: arguments.occasion != Occasion::FirstBoot,
    }));
    // The speaker check is asked for only at the factory and on demand; at
    // first boot the part says why it was not.
    checks.push(Box::new(speaker::Speaker {
        bus: &bus,
        occasion: arguments.occasion,
        answer_within: speaker::ANSWER_WITHIN,
    }));
    // The head moves a few counts only from rest, standing, and never at
    // first boot; otherwise the part says why it did not.
    checks.push(Box::new(head::HeadMove {
        bus: &bus,
        occasion: arguments.occasion,
    }));
    checks.push(Box::new(kernel::Kernel {
        path: kernel::KMSG.into(),
    }));
    // The second route's files, looked at and never read for a key (#1712).
    checks.push(Box::new(voice_route::VoiceRoute {
        unit: voice_route::UNIT.into(),
        drop_ins: voice_route::DROP_INS.into(),
    }));
    // Only at the factory is someone there to be asked to touch.
    if arguments.occasion == Occasion::Factory {
        checks.push(Box::new(hand::ByHand {
            bus: &bus,
            given: hand::GIVEN,
            ask: Box::new(|what| eprintln!("lamp-selftest: {what}")),
        }));
    }
    let mut test = lamp_selftest::run(arguments.occasion, &mut checks, BUDGET);
    // A passive run checks a failed part once more, half a minute later,
    // and keeps the fail only if it fails again (the reviewer on #1245): it
    // runs by itself every six hours, and lamp-health says its fails.
    if arguments.occasion == Occasion::FirstBoot && test.verdict == Verdict::Fail {
        eprintln!(
            "lamp-selftest: a part failed; it is checked again in {} s",
            AGAIN_AFTER.as_secs()
        );
        std::thread::sleep(AGAIN_AFTER);
        test = retry_passive(test, &mut checks, || {
            quiet::wait(|| quiet::read(&bus), std::thread::sleep)
        });
    }
    print!("{}", report::text(&test));
    match report::keep(&arguments.dir, &test) {
        Ok(path) => eprintln!("lamp-selftest: kept {}", path.display()),
        Err(error) => eprintln!(
            "lamp-selftest: the report cannot be kept in {}: {error}",
            arguments.dir.display()
        ),
    }
    if let Err(error) = publish(&bus, &test, started) {
        eprintln!("lamp-selftest: {} not published: {error}", SelfTest::TOPIC);
    }
    match test.verdict {
        Verdict::Pass | Verdict::NotFitted => ExitCode::SUCCESS,
        Verdict::Fail => ExitCode::FAILURE,
        Verdict::NotChecked => ExitCode::from(3),
    }
}

/// A failed passive part is only confirmed after another quiet admission.
/// A conversation that outlasts the wait leaves the retry unmeasured; its
/// first failure and readings stay in the report, without running any check.
fn retry_passive(
    mut first: SelfTest,
    checks: &mut [Box<dyn lamp_selftest::Check + '_>],
    wait: impl FnOnce() -> bool,
) -> SelfTest {
    if wait() {
        return lamp_selftest::again(first, checks, BUDGET);
    }
    for part in &mut first.parts {
        if part.verdict == Verdict::Fail {
            part.verdict = Verdict::NotChecked;
            part.reason = format!(
                "retry deferred: the voice stayed in a turn; first check failed: {}",
                part.reason
            );
        }
    }
    first.verdict = SelfTest::verdict_of(&first.parts);
    first
}

fn publish(bus: &Bus, test: &SelfTest, started: Instant) -> std::io::Result<()> {
    let mut out = bus.publisher_declared(SelfTest::TOPIC, &SelfTest::declaration(), "selftest")?;
    out.publish_json(started.elapsed().as_micros() as u64, test)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::{PartResult, Voice};
    use lamp_selftest::Check;
    use std::{cell::Cell, time::Duration};

    struct RetryCheck<'a> {
        part: &'static str,
        calls: &'a Cell<usize>,
        verdict: Verdict,
    }

    impl Check for RetryCheck<'_> {
        fn part(&self) -> &str {
            self.part
        }

        fn run(&mut self, _deadline: Instant) -> PartResult {
            self.calls.set(self.calls.get() + 1);
            PartResult::new(self.part, self.verdict, "retry observation")
        }
    }

    fn failed_passive_run() -> SelfTest {
        SelfTest {
            occasion: Occasion::FirstBoot,
            started: 123.0,
            took: 0.25,
            verdict: Verdict::Fail,
            parts: vec![
                PartResult::new("room_mic", Verdict::Fail, "digital silence").with("dbfs", -90),
                PartResult::new("board", Verdict::Pass, ""),
                PartResult::new("air", Verdict::NotFitted, "not fitted"),
            ],
            build: Some("test-build".into()),
            lamp: None,
        }
    }

    #[test]
    fn a_conversation_that_outlasts_the_retry_wait_never_runs_another_check() {
        let calls = Cell::new(0);
        let mut checks: Vec<Box<dyn Check + '_>> = vec![Box::new(RetryCheck {
            part: "room_mic",
            calls: &calls,
            verdict: Verdict::Pass,
        })];
        let first = failed_passive_run();
        let original = first.clone();
        let mut slept = Duration::ZERO;
        let result = retry_passive(first, &mut checks, || {
            quiet::wait(|| Some((Voice::Thinking, 0)), |pause| slept += pause)
        });
        assert_eq!(slept, quiet::MOST);
        assert_eq!(calls.get(), 0);
        assert_eq!(result.verdict, Verdict::NotChecked);
        assert_eq!(result.parts[0].verdict, Verdict::NotChecked);
        assert!(result.parts[0].reason.contains("retry deferred"));
        assert!(result.parts[0].reason.ends_with("digital silence"));
        assert_eq!(result.parts[0].values, original.parts[0].values);
        assert_eq!(result.parts[0].took, original.parts[0].took);
        assert_eq!(result.parts[1..], original.parts[1..]);
        assert_eq!(result.started, original.started);
        assert_eq!(result.took, original.took);
        assert_eq!(result.build, original.build);
        assert_eq!(result.occasion, Occasion::FirstBoot);
        assert_eq!(result.check(), Ok(()));
    }

    #[test]
    fn a_quiet_retry_only_rechecks_the_failed_parts_and_keeps_its_real_result() {
        for verdict in [Verdict::Pass, Verdict::Fail, Verdict::NotChecked] {
            let failed_calls = Cell::new(0);
            let passed_calls = Cell::new(0);
            let mut checks: Vec<Box<dyn Check + '_>> = vec![
                Box::new(RetryCheck {
                    part: "room_mic",
                    calls: &failed_calls,
                    verdict,
                }),
                Box::new(RetryCheck {
                    part: "board",
                    calls: &passed_calls,
                    verdict: Verdict::Fail,
                }),
            ];
            let first = failed_passive_run();
            let unchanged = first.parts[1..].to_vec();
            let mut slept = Duration::ZERO;
            let result = retry_passive(first, &mut checks, || {
                quiet::wait(|| Some((Voice::Resting, 0)), |pause| slept += pause)
            });
            assert_eq!(slept, quiet::EVERY * (quiet::CALM - 1));
            assert_eq!(failed_calls.get(), 1);
            assert_eq!(passed_calls.get(), 0);
            assert_eq!(result.parts[0].verdict, verdict);
            assert_eq!(result.verdict, verdict);
            assert_eq!(result.parts[1..], unchanged);
            assert_eq!(result.check(), Ok(()));
        }
    }

    fn args(given: &[&str]) -> Result<Option<Arguments>, String> {
        parse(&given.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn arguments_have_defaults_and_refuse_what_they_do_not_know() {
        let plain = args(&[]).unwrap().unwrap();
        assert_eq!(plain.occasion, Occasion::OnDemand);
        assert_eq!(plain.dir, PathBuf::from("/var/lib/lampOS/selftest"));
        let factory = args(&["--occasion", "factory", "--dir", "/tmp/x"])
            .unwrap()
            .unwrap();
        assert_eq!(factory.occasion, Occasion::Factory);
        assert!(args(&["--occasion", "later"]).is_err());
        assert!(args(&["--dir", "relative"]).is_err());
        assert!(args(&["--occasion"]).is_err());
        assert!(args(&["--help"]).unwrap().is_none());
    }
}
