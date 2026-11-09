//! The program's one clock for its own waits (`docs/skills.md`, "A test's
//! clock"): a skill's `for`, a clip's length, a held pose said again, a
//! light asked again, the Policy's 2.5 s, the voice's line and the reload.
//!
//! It is the host's monotonic clock, and stays so in production. With
//! `--clock-control` a test may skip it ahead ([`skip`]), so that it sits
//! through no real second it does not have to: the clock then runs on from
//! there at the host's pace, and every wait below wakes on it within
//! [`SLICE`]. Bus stamps, the freshness of what other parts said, a light's
//! claim and the socket timeouts against other parts stay on the host's
//! clocks, which those parts read too.
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

/// How far a test skipped the clock, in µs.
static SKIPPED_US: AtomicU64 = AtomicU64::new(0);
/// Whether a test drives the clock: only then do waits look at it in slices.
static STEPPED: AtomicBool = AtomicBool::new(false);

/// How soon a wait sees a skip.
pub const SLICE: Duration = Duration::from_millis(5);

/// From now on a test may skip the clock: waits look at it every [`SLICE`].
pub fn stepped() {
    STEPPED.store(true, Ordering::SeqCst);
}

/// Whether a test drives the clock.
pub fn is_stepped() -> bool {
    STEPPED.load(Ordering::SeqCst)
}

/// The most a test may skip in all: a year, so that the clock and any
/// wait counted from it stay far inside what an `Instant` and a µs count
/// hold.
pub const MOST_SKIPPED: Duration = Duration::from_secs(365 * 24 * 3_600);

/// Moves the clock `by` ahead at once, or refuses to, and leaves it, when
/// the total skipped would pass [`MOST_SKIPPED`].
pub fn skip(by: Duration) -> Result<(), String> {
    let most = u64::try_from(MOST_SKIPPED.as_micros()).unwrap_or(u64::MAX);
    let by_us = u64::try_from(by.as_micros()).unwrap_or(u64::MAX);
    SKIPPED_US
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |skipped| {
            skipped
                .checked_add(by_us)
                .filter(|total| *total <= most)
                .filter(|total| {
                    Instant::now()
                        .checked_add(Duration::from_micros(*total))
                        .is_some()
                        && lamp_ipc::monotonic_us().checked_add(*total).is_some()
                })
        })
        .map(|_| ())
        .map_err(|skipped| {
            format!(
                "a skip of {by_us} µs after {skipped} µs passes the most a test may skip, {most} µs"
            )
        })
}

fn skipped() -> Duration {
    Duration::from_micros(SKIPPED_US.load(Ordering::SeqCst))
}

/// Now, on this clock.
pub fn now() -> Instant {
    let host = Instant::now();
    // Never past what an `Instant` holds: [`skip`] keeps the total small.
    host.checked_add(skipped()).unwrap_or(host)
}

/// Now in µs, on the host's monotonic clock and what was skipped.
pub fn now_us() -> u64 {
    lamp_ipc::monotonic_us().saturating_add(SKIPPED_US.load(Ordering::SeqCst))
}

/// The wall clock in ms since 1970, with what a test skipped (#5746: a
/// held run's quiet hours follow a test's skips as its waits do).
pub fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
        .saturating_add(SKIPPED_US.load(Ordering::SeqCst) / 1_000)
}

/// How long a poll for something else waits: no longer than a slice while
/// a test drives the clock, so that a skip is taken in at once.
pub fn poll(wait: Duration) -> Duration {
    if is_stepped() { wait.min(SLICE) } else { wait }
}

/// Sleeps `wait` on this clock.
pub fn sleep(wait: Duration) {
    if !is_stepped() {
        return std::thread::sleep(wait);
    }
    let until = now() + wait;
    loop {
        let left = until.saturating_duration_since(now());
        if left.is_zero() {
            return;
        }
        std::thread::sleep(left.min(SLICE));
    }
}

/// `receiver.recv_timeout(wait)`, the wait on this clock.
pub fn recv_timeout<T>(
    receiver: &mpsc::Receiver<T>,
    wait: Duration,
) -> Result<T, mpsc::RecvTimeoutError> {
    if !is_stepped() {
        return receiver.recv_timeout(wait);
    }
    let until = now() + wait;
    loop {
        let left = until.saturating_duration_since(now());
        if left.is_zero() {
            return Err(mpsc::RecvTimeoutError::Timeout);
        }
        match receiver.recv_timeout(left.min(SLICE)) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            done => return done,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without a test driving it (no `stepped()` in this process), the
    /// clock is the host's: a sleep and a wait take their whole time.
    #[test]
    fn without_a_test_driving_it_a_wait_takes_its_whole_time() {
        assert!(!is_stepped());
        let wait = Duration::from_millis(120);
        let began = Instant::now();
        sleep(wait);
        assert!(began.elapsed() >= wait, "{:?}", began.elapsed());
        let (_keep, nothing) = mpsc::channel::<()>();
        let began = Instant::now();
        assert_eq!(
            recv_timeout(&nothing, wait),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        assert!(began.elapsed() >= wait, "{:?}", began.elapsed());
        assert_eq!(poll(wait), wait);
    }

    /// A skip the total cannot hold is refused and leaves the clock.
    #[test]
    fn a_skip_past_the_most_is_refused() {
        assert!(skip(Duration::MAX).is_err());
        assert!(skip(MOST_SKIPPED + Duration::from_micros(1)).is_err());
        assert_eq!(skipped(), Duration::ZERO);
    }
}
