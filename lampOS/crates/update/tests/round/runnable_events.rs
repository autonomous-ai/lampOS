//! Test-only ownership of a builtin stub's output and exit. The caller retains
//! the Child guard: this helper never starts a thread or transfers reaping.
use std::{
    io,
    os::fd::AsFd,
    process::{Child, ExitStatus},
    time::{Duration, Instant},
};

const STARTUP: Duration = Duration::from_secs(5);
const FINISH: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(5);
const OUTPUT_LIMIT: usize = 128;

/// No blocking read, including after leader exit: EOF is its own event. One
/// extra byte detects output beyond the old 128-byte limit instead of silently
/// discarding it. All bytes read, including invalid UTF-8, remain in diagnostics.
pub(super) fn observe(
    child: &mut Child,
    stdout: &impl AsFd,
    started: Instant,
) -> Result<(ExitStatus, Vec<u8>), String> {
    let flags = rustix::fs::fcntl_getfl(stdout).map_err(|error| error.to_string())?;
    let observed = (|| {
        rustix::fs::fcntl_setfl(stdout, flags | rustix::fs::OFlags::NONBLOCK)
            .map_err(|error| error.to_string())?;
        let actual = rustix::fs::fcntl_getfl(stdout).map_err(|error| error.to_string())?;
        if !actual.contains(rustix::fs::OFlags::NONBLOCK) {
            return Err("stdout remained blocking".to_owned());
        }
        collect(child, stdout, started)
    })();
    // Also restore on a failed read or watchdog. The caller owns this descriptor
    // throughout; controls can verify that neither path leaves shared flags set.
    let restored = (|| -> io::Result<()> {
        rustix::fs::fcntl_setfl(stdout, flags)?;
        if rustix::fs::fcntl_getfl(stdout)? != flags {
            return Err(io::Error::other("stdout flags did not restore exactly"));
        }
        Ok(())
    })();
    match (observed, restored) {
        (result, Ok(())) => result,
        (result, Err(error)) => Err(format!(
            "stdout restoration failed: {error}; observation={result:?}"
        )),
    }
}

fn collect(
    child: &mut Child,
    stdout: &impl AsFd,
    started: Instant,
) -> Result<(ExitStatus, Vec<u8>), String> {
    let startup_deadline = started + STARTUP;
    let mut first_byte: Option<Instant> = None;
    let mut output = Vec::new();
    let mut eof = false;
    let mut status = None;
    let mut polls = 0;
    let mut last_poll = (Duration::ZERO, Duration::ZERO);
    loop {
        let deadline = first_byte.map_or(startup_deadline, |at| at + FINISH);
        if Instant::now() >= deadline {
            // Evidence only. A late exit cannot turn a missed deadline green.
            let final_status = child.try_wait();
            return Err(format!(
                "{} watchdog: pid={} elapsed={:?} first_byte={:?} eof={eof} \
                 status={status:?} output={output:?} polls={polls} \
                 last_poll={last_poll:?} final_status={final_status:?} \
                 (diagnostic only; timeout still fails)",
                if first_byte.is_some() {
                    "exit/EOF"
                } else {
                    "startup"
                },
                child.id(),
                started.elapsed(),
                first_byte.map(|at| at.duration_since(started)),
            ));
        }
        if !eof {
            let mut buffer = [0_u8; OUTPUT_LIMIT + 1];
            match rustix::io::read(stdout, &mut buffer[..OUTPUT_LIMIT + 1 - output.len()]) {
                Ok(0) => eof = true,
                Ok(count) => {
                    let at = Instant::now();
                    output.extend_from_slice(&buffer[..count]);
                    // Do not move startup's boundary retroactively if this read
                    // was observed late. Subsequent bytes never renew FINISH.
                    if first_byte.is_none() {
                        if at >= startup_deadline {
                            return Err(format!(
                                "startup watchdog: first byte observed late; elapsed={:?} output={output:?}",
                                started.elapsed()
                            ));
                        }
                        first_byte = Some(at);
                    }
                    if output.len() > OUTPUT_LIMIT {
                        return Err(format!(
                            "stdout exceeded {OUTPUT_LIMIT} bytes; output={output:?}"
                        ));
                    }
                }
                Err(rustix::io::Errno::AGAIN | rustix::io::Errno::INTR) => {}
                Err(error) => {
                    return Err(format!("stdout read failed: {error}; output={output:?}"));
                }
            }
        }
        if status.is_none() {
            last_poll.0 = started.elapsed();
            status = child
                .try_wait()
                .map_err(|error| format!("status read failed: {error}; output={output:?}"))?;
            last_poll.1 = started.elapsed();
            polls += 1;
        }
        // Recheck time before accepting completion, even if the process exited
        // while this test thread was descheduled. The next iteration diagnoses it.
        let deadline = first_byte.map_or(startup_deadline, |at| at + FINISH);
        if Instant::now() < deadline
            && eof
            && let Some(status) = status
        {
            if first_byte.is_none() {
                return Err(format!(
                    "updater exited without output: {status}; output={output:?}"
                ));
            }
            return Ok((status, output));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if !remaining.is_zero() {
            // Only a pending event chooses this poll wait, never test completion.
            std::thread::sleep(POLL.min(remaining));
        }
    }
}

#[cfg(test)]
#[path = "runnable_event_controls.rs"]
mod controls;
