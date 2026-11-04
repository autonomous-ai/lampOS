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
