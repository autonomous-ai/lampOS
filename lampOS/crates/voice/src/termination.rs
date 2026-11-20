//! Cooperative cancellation only. Signal handlers store an atomic value;
//! all control, diagnostics and filesystem work remains on the normal thread.
use signal_hook::{
    SigId,
    consts::signal::{SIGINT, SIGTERM},
};
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

pub(crate) struct Termination {
    signal: Arc<AtomicUsize>,
    registrations: Vec<SigId>,
}
impl Termination {
    #[cfg(test)]
    pub(crate) fn inactive() -> Self {
        Self {
            signal: Arc::new(AtomicUsize::new(0)),
            registrations: Vec::new(),
        }
    }
    /// As if SIGTERM had come.
    #[cfg(test)]
    pub(crate) fn signaled() -> Self {
        Self {
            signal: Arc::new(AtomicUsize::new(SIGTERM as usize)),
            registrations: Vec::new(),
        }
    }
    /// What SIGTERM coming later does, for another thread to call.
    #[cfg(test)]
    pub(crate) fn later(&self) -> impl Fn() + Send + 'static {
        let signal = Arc::clone(&self.signal);
        move || signal.store(SIGTERM as usize, Ordering::SeqCst)
    }
    pub(crate) fn install() -> io::Result<Self> {
        let mut request = Self {
            signal: Arc::new(AtomicUsize::new(0)),
            registrations: Vec::with_capacity(2),
        };
        for signal in [SIGINT, SIGTERM] {
            request
                .registrations
                .push(signal_hook::flag::register_usize(
                    signal,
                    Arc::clone(&request.signal),
                    signal as usize,
                )?);
        }
        Ok(request)
    }
    pub(crate) fn signal_name(&self) -> Option<&'static str> {
        match self.signal.load(Ordering::SeqCst) {
            0 => None,
            value if value == SIGINT as usize => Some("SIGINT"),
            _ => Some("SIGTERM"),
        }
    }
    pub(crate) fn check(&self) -> io::Result<()> {
        match self.signal_name() {
            None => Ok(()),
            Some(signal) => Err(io::Error::new(
                io::ErrorKind::Interrupted,
                format!("directed run interrupted by {signal}"),
            )),
        }
    }
}
/// Whether a run ended because the service was asked to stop (SIGTERM),
/// at whichever [`Termination::check`] the stop met: start-up, a
/// conversation's re-make after a mute, a pass of the loop. The main loop
/// returns `Ok` for a stop it meets; any other check returns this
/// `Interrupted` error, and the process then exited 1 on an ordinary stop
/// (a gate read it as a fault, 19 Nov). A SIGINT, any other `Interrupted`
/// and every other error are not this.
pub fn is_sigterm_stop(error: &(dyn std::error::Error + 'static)) -> bool {
    error.downcast_ref::<io::Error>().is_some_and(|error| {
        error.kind() == io::ErrorKind::Interrupted
            && error.to_string() == "directed run interrupted by SIGTERM"
    })
}

/// Whether the process ends with status 0: the run was `Ok`, or it ended
/// by a stop ([`is_sigterm_stop`]).
pub fn exits_clean(result: &Result<(), Box<dyn std::error::Error + Send + Sync>>) -> bool {
    match result {
        Ok(()) => true,
        Err(error) => is_sigterm_stop(&**error),
    }
}

impl Drop for Termination {
    fn drop(&mut self) {
        // Remove only this run's actions, including partial registration failure.
        // signal-hook does not restore the process's former default disposition.
        // The production CLI exits immediately after this finite run returns.
        for registration in self.registrations.drain(..) {
            signal_hook::low_level::unregister(registration);
        }
    }
}

#[cfg(test)]
mod stop_tests {
    use super::*;

    type Boxed = Box<dyn std::error::Error + Send + Sync>;

    /// What a check that meets a stop gives, carried the way the call sites
    /// carry it (`termination.check()?` into the run's boxed error).
    fn met_by_a_check(termination: &Termination) -> Result<(), Boxed> {
        termination.check()?;
        Ok(())
    }

    #[test]
    fn a_stop_met_at_any_check_exits_clean() {
        // Start-up, a conversation's re-make and the loop's passes all call
        // the same `check`, so one error is what each of them returns.
        let stop = met_by_a_check(&Termination::signaled());
        assert!(stop.is_err());
        assert!(exits_clean(&stop), "a SIGTERM stop is exit 0: {stop:?}");
    }

    #[test]
    fn a_run_that_returned_ok_exits_clean() {
        assert!(exits_clean(&met_by_a_check(&Termination::inactive())));
    }

    #[test]
    fn a_real_error_still_exits_1() {
        for error in [
            io::Error::other("the speaker worker did not answer"),
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "command `x` is not on the voice's list",
            ),
            io::Error::new(
                io::ErrorKind::NotFound,
                "directed run interrupted by SIGTERM",
            ),
            // An interrupted system call, not a stop.
            io::Error::new(io::ErrorKind::Interrupted, "interrupted system call"),
        ] {
            let result: Result<(), Boxed> = Err(error.into());
            assert!(!exits_clean(&result), "{result:?} is a fault");
        }
        let words: Result<(), Boxed> = Err("a plain error".into());
        assert!(!exits_clean(&words));
    }

    #[test]
    fn a_sigint_is_not_the_service_stop() {
        let interrupted = Termination {
            signal: Arc::new(AtomicUsize::new(SIGINT as usize)),
            registrations: Vec::new(),
        };
        let result = met_by_a_check(&interrupted);
        assert!(result.is_err());
        assert!(!exits_clean(&result), "a terminal's Ctrl-C keeps exit 1");
    }
}
