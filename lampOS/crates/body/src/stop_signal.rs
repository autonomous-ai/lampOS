//! SIGTERM and SIGINT as a polled "stop" request, without `unsafe`.
//!
//! The caller blocks both signals in its thread and asks `requested` once a
//! step. On Linux this is the signalfd the body has always used. Elsewhere
//! (macOS has no signalfd) one helper thread waits for the blocked signals
//! with `sigwait` and raises a flag. Dropping it puts the old mask back.
//!
//! Build it before any other thread is started: threads inherit the blocked
//! mask, and a thread started earlier would take the default action.
use nix::sys::signal::{SigSet, Signal};
use std::io;

#[cfg(target_os = "linux")]
use nix::sys::signalfd::{SfdFlags, SignalFd};
#[cfg(not(target_os = "linux"))]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub struct StopSignal {
    old: SigSet,
    #[cfg(target_os = "linux")]
    fd: SignalFd,
    #[cfg(not(target_os = "linux"))]
    raised: Arc<AtomicBool>,
}

impl StopSignal {
    pub fn new() -> io::Result<Self> {
        let old = SigSet::thread_get_mask()?;
        let mut mask = SigSet::empty();
        mask.add(Signal::SIGTERM);
        mask.add(Signal::SIGINT);
        mask.thread_block()?;
        match Self::open(&mask) {
            Ok(open) => Ok(Self {
                old,
                #[cfg(target_os = "linux")]
                fd: open,
                #[cfg(not(target_os = "linux"))]
                raised: open,
            }),
            Err(error) => {
                let _ = old.thread_set_mask();
                Err(error)
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn open(mask: &SigSet) -> io::Result<SignalFd> {
        Ok(SignalFd::with_flags(
            mask,
            SfdFlags::SFD_CLOEXEC | SfdFlags::SFD_NONBLOCK,
        )?)
    }

    #[cfg(not(target_os = "linux"))]
    fn open(mask: &SigSet) -> io::Result<Arc<AtomicBool>> {
        let raised = Arc::new(AtomicBool::new(false));
        let waiter = Arc::clone(&raised);
        let mask = *mask;
        std::thread::Builder::new()
            .name("stop-signal".into())
            .spawn(move || {
                // Both signals are blocked here too (inherited), so they are
                // taken by `sigwait`, never by their default action.
                while mask.wait().is_ok() {
                    waiter.store(true, Ordering::SeqCst);
                }
            })?;
        Ok(raised)
    }

    /// Whether a stop was asked for since the last call.
    pub fn requested(&mut self) -> io::Result<bool> {
        #[cfg(target_os = "linux")]
        {
            Ok(self.fd.read_signal()?.is_some())
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(self.raised.swap(false, Ordering::SeqCst))
        }
    }
}

impl Drop for StopSignal {
    fn drop(&mut self) {
        let _ = self.old.thread_set_mask();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_requested_until_a_signal_comes_and_the_old_mask_returns() {
        let before = SigSet::thread_get_mask().unwrap();
        let mut stop = StopSignal::new().unwrap();
        assert!(!stop.requested().unwrap());
        assert!(!stop.requested().unwrap());
        drop(stop);
        let after = SigSet::thread_get_mask().unwrap();
        assert_eq!(
            before.contains(Signal::SIGTERM),
            after.contains(Signal::SIGTERM)
        );
        assert_eq!(
            before.contains(Signal::SIGINT),
            after.contains(Signal::SIGINT)
        );
    }
}
