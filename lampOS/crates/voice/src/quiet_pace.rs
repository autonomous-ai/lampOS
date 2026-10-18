//! An idle pace for the voice's loops, with `LAMP_QUIET_PACE=on` (#705,
//! 08:37: the quiet voice took half of lamp-4ace's core, which ran past the
//! 90 °C where it slows itself).
//!
//! Each loop passes every 2 ms whatever is happening, mostly finding nothing
//! to receive. While nothing is going on (no reply, nothing queued for the
//! provider, no admitted speech, nothing draining, the room not busy, and no
//! speech heard for [`SPEECH_KEEPS_US`]), a loop with this on passes every
//! [`QUIET`] instead. What it costs is up to 3 ms more before a pass notices
//! a sound that starts the quiet's end; the pass after it is at [`BUSY`]
//! again. The 20 ms authority renewals and every lease (250 ms idle) are far
//! from it. Off unless set.
use std::time::Duration;

pub const ENVIRONMENT: &str = "LAMP_QUIET_PACE";
/// With `on`, the capture worker skips the echo evidence's search while the
/// speaker has been given only silence for its whole reach (520 ms): a
/// search that then cannot find anything, which a room loud enough to search
/// (a fan, an office) otherwise runs every 10 ms. The evidence is the same
/// (`lamp_audio::evidence::EchoObserver::skip_while_render_silent`).
pub const EVIDENCE_ENVIRONMENT: &str = "LAMP_QUIET_EVIDENCE";

/// `on` for the capture worker to wait in `poll` between blocks instead of
/// passing every 2 ms (`audio_workers.rs`, #705). Passed on to the capture
/// worker only.
pub const CAPTURE_WAIT_ENVIRONMENT: &str = "LAMP_CAPTURE_WAIT";

/// Whether this worker skips the evidence's search, read once.
pub fn evidence_skipped() -> bool {
    std::env::var_os(EVIDENCE_ENVIRONMENT).as_deref() == Some(std::ffi::OsStr::new("on"))
}
/// The pace whenever something is going on, and always without the switch.
pub const BUSY: Duration = Duration::from_millis(2);
/// The pace while nothing is.
pub const QUIET: Duration = Duration::from_millis(5);
/// Speech heard this recently keeps the busy pace.
pub const SPEECH_KEEPS_US: u64 = 2_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pace {
    on: bool,
}

impl Pace {
    /// Read once, when the loop starts.
    pub fn from_environment() -> Self {
        Self::from_value(std::env::var(ENVIRONMENT).ok().as_deref())
    }

    /// On only for `Some("on")`.
    pub fn from_value(value: Option<&str>) -> Self {
        Self {
            on: value == Some("on"),
        }
    }

    /// How long this pass takes, its work and its wait together.
    pub fn pass(self, quiet: bool) -> Duration {
        if self.on && quiet { QUIET } else { BUSY }
    }
}

/// What a pass found going on, which keeps the busy pace.
#[derive(Clone, Copy, Debug, Default)]
pub struct Going {
    pub reply: bool,
    pub provider_queue: bool,
    pub stream: bool,
    pub draining: bool,
    pub room_busy: bool,
    pub since_speech_us: u64,
}

impl Going {
    /// Nothing going on, and no speech for [`SPEECH_KEEPS_US`].
    pub fn quiet(self) -> bool {
        !(self.reply || self.provider_queue || self.stream || self.draining || self.room_busy)
            && self.since_speech_us >= SPEECH_KEEPS_US
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_on_and_quiet_take_the_slower_pace() {
        for value in [None, Some("1"), Some("off"), Some("ON"), Some("")] {
            let pace = Pace::from_value(value);
            assert_eq!(pace.pass(true), BUSY, "{value:?}");
            assert_eq!(pace.pass(false), BUSY, "{value:?}");
        }
        let pace = Pace::from_value(Some("on"));
        assert_eq!(pace.pass(true), QUIET);
        assert_eq!(pace.pass(false), BUSY);
        // Far inside the 20 ms renewal and the 100 ms lease of a turn.
        assert!(QUIET * 4 <= Duration::from_millis(20));
    }

    #[test]
    fn anything_going_on_or_recent_speech_keeps_the_busy_pace() {
        let idle = Going {
            since_speech_us: SPEECH_KEEPS_US,
            ..Going::default()
        };
        assert!(idle.quiet());
        assert!(
            !Going {
                since_speech_us: SPEECH_KEEPS_US - 1,
                ..idle
            }
            .quiet()
        );
        for going in [
            Going {
                reply: true,
                ..idle
            },
            Going {
                provider_queue: true,
                ..idle
            },
            Going {
                stream: true,
                ..idle
            },
            Going {
                draining: true,
                ..idle
            },
            Going {
                room_busy: true,
                ..idle
            },
        ] {
            assert!(!going.quiet(), "{going:?}");
        }
    }
}
