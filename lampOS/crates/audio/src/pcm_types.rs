//! The error, result and state the speaker's [`PlaybackPcm`](crate::pcm::PlaybackPcm) speaks in.
//!
//! On Linux they are ALSA's own types, re-exported under these names, so the board's binary is
//! the same code it was and its journal says what it said ("ALSA function 'snd_pcm_writei'
//! failed with error ..."). Elsewhere (a developer's Mac, where there is no ALSA) they are
//! small copies with the same calls the speaker makes of them, so the speaker's ownership
//! ledger, privacy guard, authority and underrun logic build and run on every unix.

#[cfg(target_os = "linux")]
pub use alsa::{Error as PcmError, Result as PcmResult, pcm::State as PcmState};

#[cfg(not(target_os = "linux"))]
pub use portable::{PcmError, PcmResult, PcmState};

#[cfg(not(target_os = "linux"))]
mod portable {
    use std::fmt;

    /// One failed playback call: which call and the positive `errno` it returned, as
    /// `alsa::Error` carries them.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct PcmError {
        func: &'static str,
        errno: i32,
    }

    impl PcmError {
        pub fn new(func: &'static str, errno: i32) -> Self {
            Self { func, errno }
        }

        pub fn func(&self) -> &'static str {
            self.func
        }

        pub fn errno(&self) -> i32 {
            self.errno
        }
    }

    impl fmt::Display for PcmError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "playback function '{}' failed with errno {}",
                self.func, self.errno
            )
        }
    }

    impl std::error::Error for PcmError {}

    pub type PcmResult<T> = std::result::Result<T, PcmError>;

    /// A playback queue's state, with the names and the meaning of ALSA's `snd_pcm_state_t`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum PcmState {
        Open,
        Setup,
        Prepared,
        Running,
        XRun,
        Draining,
        Paused,
        Suspended,
        Disconnected,
    }
}
