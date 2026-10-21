//! Shared pacing for the touch, privacy-only and strip Controls loops.

/// A gap this long between two steps outlasts the privacy reading.
pub const LONG_GAP_US: u64 = lamp_msgs::PRIVACY_SWITCH_TTL_US;
/// Count steps whose elapsed duration reaches the privacy polling period.
pub const SLOW_STEP_US: u64 = crate::privacy::STEP_US;

/// Elapsed-time diagnostics, independent of the CPU time used for sleeping.
#[derive(Debug, Default)]
pub struct Pace {
    last_us: Option<u64>,
    pub longest_gap_us: u64,
    pub long_gaps: u64,
    pub slow_steps: u64,
    said_us: Option<u64>,
    unsaid: u64,
}

impl Pace {
    /// A step begins at `at_us`: the journal line, if one is due.
    pub fn step(&mut self, at_us: u64) -> Option<String> {
        let gap = at_us.saturating_sub(self.last_us.replace(at_us)?);
        self.longest_gap_us = self.longest_gap_us.max(gap);
        if gap < LONG_GAP_US {
            return None;
        }
        self.long_gaps += 1;
        if self
            .said_us
            .is_some_and(|said| at_us.saturating_sub(said) < 1_000_000)
        {
            self.unsaid += 1;
            return None;
        }
        self.said_us = Some(at_us);
        let more = match std::mem::take(&mut self.unsaid) {
            0 => String::new(),
            more => format!(" ({more} more since the last line)"),
        };
        Some(format!(
            "the loop didn't run for {} ms, past the privacy reading's {} ms{more}",
            gap / 1_000,
            LONG_GAP_US / 1_000
        ))
    }

    /// A service step took `elapsed_us`, including waits and preemption.
    pub fn worked(&mut self, elapsed_us: u64) {
        self.slow_steps += u64::from(elapsed_us >= SLOW_STEP_US);
    }
}

/// Rest for the remaining polling period and at least the step's CPU work.
/// A blocked or descheduled step must not sleep for that wait again: a
/// 30 ms wait plus 30 ms of sleep would lapse the 50 ms privacy reading.
/// Even a sub-microsecond CPU measurement leaves a nonzero sleep.
pub fn rest_us(elapsed_us: u64, cpu_us: u64, poll_us: u64) -> u64 {
    poll_us.saturating_sub(elapsed_us).max(cpu_us).max(1)
}

/// Run and pace one whole iteration, including its hand journal writes.
/// Both service paths use this, including `--touch` with only a slide.
/// The strip child uses the fallible variant below.
pub fn run_step<T>(poll_us: u64, step: impl FnOnce(u64) -> T) -> T {
    try_run_step(poll_us, |began| {
        Ok::<_, std::convert::Infallible>(step(began))
    })
    .unwrap()
}

/// Pace a successful iteration, but return terminal errors without sleeping
/// so the strip worker can publish its fault immediately.
pub(crate) fn try_run_step<T, E>(
    poll_us: u64,
    step: impl FnOnce(u64) -> Result<T, E>,
) -> Result<T, E> {
    run_step_with(
        poll_us,
        lamp_ipc::monotonic_us,
        thread_cpu_us,
        |rest| std::thread::sleep(std::time::Duration::from_micros(rest)),
        step,
    )
}

fn thread_cpu_us() -> u64 {
    let time = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    u64::try_from(time.tv_sec)
        .unwrap_or(0)
        .saturating_mul(1_000_000)
        .saturating_add(u64::try_from(time.tv_nsec).unwrap_or(0) / 1_000)
}

fn run_step_with<T, E>(
    poll_us: u64,
    now: impl Fn() -> u64,
    cpu_now: impl Fn() -> u64,
    sleep: impl FnOnce(u64),
    step: impl FnOnce(u64) -> Result<T, E>,
) -> Result<T, E> {
    let cpu_began = cpu_now();
    let began = now();
    let output = step(began)?;
    let elapsed = now().saturating_sub(began);
    let worked = cpu_now().saturating_sub(cpu_began);
    sleep(rest_us(elapsed, worked, poll_us));
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn waits_do_not_become_another_wait_on_first_or_repeated_steps() {
        let wall = Cell::new(0);
        let cpu = Cell::new(0);
        for (elapsed, worked, expected_rest) in
            [(100, 100, 900), (30_000, 200, 200), (30_000, 200, 200)]
        {
            let began = wall.get();
            let result = run_step_with(
                1_000,
                || wall.get(),
                || cpu.get(),
                |rest| {
                    assert_eq!(rest, expected_rest);
                    wall.set(wall.get() + rest);
                },
                |at| {
                    assert_eq!(at, began);
                    wall.set(wall.get() + elapsed);
                    cpu.set(cpu.get() + worked);
                    Ok::<_, ()>(42)
                },
            );
            assert_eq!(result, Ok(42));
            assert!(wall.get() - began < LONG_GAP_US);
        }
    }

    #[test]
    fn cpu_busy_steps_leave_at_least_as_much_time_for_other_threads() {
        let wall = Cell::new(0);
        let cpu = Cell::new(0);
        for worked in [800, 5_000, 30_000, 5_000] {
            run_step_with(
                1_000,
                || wall.get(),
                || cpu.get(),
                |rest| {
                    assert!(rest >= worked);
                    wall.set(wall.get() + rest);
                },
                |_| {
                    wall.set(wall.get() + worked);
                    cpu.set(cpu.get() + worked);
                    Ok::<_, ()>(())
                },
            )
            .unwrap();
            assert!(cpu.get() * 2 <= wall.get());
        }
        assert_eq!(rest_us(30_000, 0, 1_000), 1);
    }

    #[test]
    fn the_thread_clock_excludes_a_real_wait() {
        for attempt in 0..2 {
            let rest = Cell::new(0);
            let elapsed = Cell::new(0);
            run_step_with(
                1_000,
                lamp_ipc::monotonic_us,
                thread_cpu_us,
                |us| rest.set(us),
                |began| {
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    elapsed.set(lamp_ipc::monotonic_us().saturating_sub(began));
                    Ok::<_, ()>(())
                },
            )
            .unwrap();
            // A wall-clock substitution would charge the entire wait again.
            // This checks clock selection, not a hardware response deadline.
            assert!(rest.get() < elapsed.get() / 2);
            eprintln!(
                "pacing host clock: attempt={attempt}, elapsed_us={}, requested_rest_us={}",
                elapsed.get(),
                rest.get()
            );
        }
    }

    #[test]
    fn a_slow_failed_step_returns_its_fault_without_another_wait() {
        let wall = Cell::new(0);
        let cpu = Cell::new(0);
        let result = run_step_with(
            1_000,
            || wall.get(),
            || cpu.get(),
            |_| panic!("a terminal fault must not wait for the rest floor"),
            |_| {
                wall.set(30_000);
                cpu.set(5_000);
                Err::<(), _>("strip read failed")
            },
        );
        assert_eq!(result, Err("strip read failed"));
    }
}
