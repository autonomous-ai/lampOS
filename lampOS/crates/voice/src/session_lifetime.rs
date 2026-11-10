//! Session duration is independent of turn, provider and hardware authority.
use std::{
    env, io,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Lifetime {
    Finite(u64),
    Serve,
}

impl Lifetime {
    pub(crate) fn validate(self) -> io::Result<()> {
        match self {
            Self::Finite(1..=600) | Self::Serve => Ok(()),
            _ => Err(io::Error::other("duration must be 1..600 seconds")),
        }
    }

    pub(crate) fn seconds(self) -> Option<u64> {
        match self {
            Self::Finite(seconds) => Some(seconds),
            Self::Serve => None,
        }
    }

    pub(crate) fn continuous(self) -> bool {
        self == Self::Serve
    }
}

/// `on` for a service to start with no link to the speech service
/// (`docs/offline.md`, #830): once the microphone is being retained it
/// keeps listening past the startup budget and becomes ready when the link
/// comes up. Anything else: the budget covers the link too, as before.
pub const START_OFFLINE_ENVIRONMENT: &str = "LAMP_START_OFFLINE";

/// Whether the environment switches it on. In a test, this thread's
/// [`set_start_offline_for_test`] decides instead, so parallel tests never
/// share the process environment.
pub(crate) fn start_offline() -> bool {
    #[cfg(test)]
    if let Some(on) = START_OFFLINE_FOR_TEST.with(std::cell::Cell::get) {
        return on;
    }
    env::var(START_OFFLINE_ENVIRONMENT).ok().as_deref() == Some("on")
}

#[cfg(test)]
thread_local! {
    // Process ownership: this test process writes and reads this fixture state; not IPC.
    static START_OFFLINE_FOR_TEST: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
}

/// This thread's `LAMP_START_OFFLINE`, for a test; `None` reads the
/// environment again.
#[cfg(test)]
pub(crate) fn set_start_offline_for_test(on: Option<bool>) {
    START_OFFLINE_FOR_TEST.with(|cell| cell.set(on));
}

pub(crate) struct SessionClock {
    lifetime: Lifetime,
    startup_deadline: Instant,
    ready: bool,
    deadline: Option<Instant>,
    /// A service may wait for its link past the startup budget.
    start_offline: bool,
    /// Input has been retained: the budget's other half is met.
    capturing: bool,
}

impl SessionClock {
    pub(crate) fn new(
        lifetime: Lifetime,
        started: Instant,
        startup_budget: Duration,
    ) -> io::Result<Self> {
        lifetime.validate()?;
        Ok(Self {
            lifetime,
            startup_deadline: started + startup_budget,
            ready: false,
            deadline: None,
            start_offline: false,
            capturing: false,
        })
    }

    /// Lets a service wait for its link once input is retained. A finite
    /// session keeps its whole budget.
    pub(crate) fn with_start_offline(mut self, on: bool) -> Self {
        self.start_offline = on && self.lifetime.continuous();
        self
    }

    /// Input is being retained: microphone, privacy and capture are up.
    pub(crate) fn capture_retained(&mut self) {
        self.capturing = true;
    }

    /// Waiting past the budget for the link only, with input retained.
    pub(crate) fn waiting_for_link(&self, at: Instant) -> bool {
        !self.ready && self.start_offline && self.capturing && at >= self.startup_deadline
    }

    pub(crate) fn ready(&self) -> bool {
        self.ready
    }

    pub(crate) fn activate(&mut self, at: Instant) -> io::Result<()> {
        if self.ready || (at >= self.startup_deadline && !self.waiting_for_link(at)) {
            return Err(io::Error::other(
                "duplicate or late initial input readiness",
            ));
        }
        self.deadline = self
            .lifetime
            .seconds()
            .map(|seconds| at + Duration::from_secs(seconds));
        self.ready = true;
        Ok(())
    }

    pub(crate) fn startup_expired(&self, at: Instant) -> bool {
        !self.ready && at >= self.startup_deadline && !self.waiting_for_link(at)
    }

    pub(crate) fn expired(&self, at: Instant) -> bool {
        self.deadline.is_some_and(|deadline| at >= deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_passes_six_hundred_seconds_without_expiring_or_rearming_readiness() {
        let started = Instant::now();
        let mut clock =
            SessionClock::new(Lifetime::Serve, started, Duration::from_secs(25)).unwrap();
        clock.activate(started + Duration::from_secs(1)).unwrap();
        for seconds in [25, 599, 600, 601, 3_600, 86_400, 604_800] {
            let at = started + Duration::from_secs(seconds);
            assert!(!clock.startup_expired(at));
            assert!(!clock.expired(at));
            assert!(clock.ready());
            assert!(clock.activate(at).is_err());
        }
    }

    #[test]
    fn finite_qualification_keeps_its_original_deadline_from_readiness() {
        let started = Instant::now();
        let mut clock =
            SessionClock::new(Lifetime::Finite(600), started, Duration::from_secs(25)).unwrap();
        clock.activate(started + Duration::from_secs(2)).unwrap();
        assert!(!clock.expired(started + Duration::from_secs(601)));
        assert!(clock.expired(started + Duration::from_secs(602)));
        assert!(Lifetime::Finite(601).validate().is_err());
    }

    #[test]
    fn serve_does_not_invent_initial_readiness_or_remove_startup_failure() {
        let started = Instant::now();
        let mut clock =
            SessionClock::new(Lifetime::Serve, started, Duration::from_secs(25)).unwrap();
        assert!(!clock.ready());
        assert!(clock.startup_expired(started + Duration::from_secs(25)));
        assert!(clock.activate(started + Duration::from_secs(25)).is_err());
    }

    #[test]
    fn serve_started_offline_waits_for_its_link_once_input_is_retained() {
        let started = Instant::now();
        let budget = Duration::from_secs(25);
        let mut clock = SessionClock::new(Lifetime::Serve, started, budget)
            .unwrap()
            .with_start_offline(true);
        clock.capture_retained();
        // Not before the budget: within it a link is only coming up.
        assert!(!clock.waiting_for_link(started));
        assert!(!clock.waiting_for_link(started + budget - Duration::from_millis(1)));
        assert!(clock.waiting_for_link(started + budget));
        let late = started + Duration::from_secs(600);
        assert!(clock.waiting_for_link(late));
        assert!(!clock.startup_expired(late));
        clock.activate(late).unwrap();
        assert!(clock.ready());
        assert!(!clock.waiting_for_link(late));
        assert!(!clock.expired(started + Duration::from_secs(86_400)));
        assert!(clock.activate(late).is_err());
    }

    #[test]
    fn started_offline_without_input_or_switch_or_as_a_finite_session_still_fails() {
        let started = Instant::now();
        let budget = Duration::from_secs(25);
        let at = started + budget;
        // No input retained: the microphone is the part that failed.
        let clock = SessionClock::new(Lifetime::Serve, started, budget)
            .unwrap()
            .with_start_offline(true);
        assert!(clock.startup_expired(at));
        // Switch off: exactly as before.
        let mut clock = SessionClock::new(Lifetime::Serve, started, budget).unwrap();
        clock.capture_retained();
        assert!(clock.startup_expired(at));
        assert!(clock.activate(at).is_err());
        // A finite session never waits.
        let mut clock = SessionClock::new(Lifetime::Finite(60), started, budget)
            .unwrap()
            .with_start_offline(true);
        clock.capture_retained();
        assert!(clock.startup_expired(at));
        assert!(clock.activate(at).is_err());
    }
}
