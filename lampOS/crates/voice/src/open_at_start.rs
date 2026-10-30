//! A microphone or speaker that failed to open at start, and how long it is
//! tried again for. The microphone waits out busy and off-USB answers inside
//! the capture worker's loop (#1178, #1567); the speaker waits out busy only,
//! before it says it is ready (#4332). Off the board, so a test runs here.
use crate::wire::OpenWaited;
use lamp_audio::{
    capture::CaptureError,
    device::{DeviceFault, OPEN_CALL, errno},
};
use std::time::{Duration, Instant};

type Error = dyn std::error::Error + Send + Sync + 'static;
type Result<T> = std::result::Result<T, Box<Error>>;

/// At start the microphone may still be held for a moment by the voice that
/// ran before (lamp-52e6, 2026-10-15: `systemctl restart lamp-voice` found it
/// busy 330 ms after the old process had gone, and after a power cut too).
/// Its open is tried again for this long before the worker gives up. 8 s,
/// not the 3 s it was: on lamp-4ace on 2026-10-18 (17:48) PulseAudio's
/// `module-udev-detect` most likely took the card 60 ms after the old voice
/// let go, and it keeps a card until `module-suspend-on-idle` lets it go,
/// 5 s idle by default; 3 s ran out first (#1567). 8 s covers those 5 s with
/// margin, well inside the coordinator's 25 s for input to be ready.
pub const BUSY_AT_START: Duration = Duration::from_secs(8);
/// How long a busy speaker is tried again at start, every [`BUSY_RETRY`] as
/// the microphone is. Its own bound, not the microphone's 8 s: on lamp-06c1
/// on 2026-10-30 the C-Media speaker answered busy from 10:17:01 and was free
/// again at about 10:17:10, some 9 s, PulseAudio's `module-suspend-on-idle`
/// letting it go 5 s after its last use; two runs ended on it before the
/// third started (#4332). 20 s covers that with margin. A wait inside the
/// run is cheap beside a restart: an update's 60 s watch rides out the one
/// and not the other.
pub const SPEAKER_BUSY_AT_START: Duration = Duration::from_secs(20);
/// How often a busy microphone is tried again: often enough to take it soon
/// after it is free, not on every 2 ms tick (4,000 opens in 8 s).
const BUSY_RETRY: Duration = Duration::from_millis(50);
/// At start the microphone may be off USB for a moment: on lamp-4ace on
/// 2026-10-17 (12:29:20 and 12:49:06) the voice mic fell off for 4-5 s, the
/// open failed with `ENODEV`, and the run ended and muted the lamp (#1178).
/// Its open is tried again for this long, every [`GONE_RETRY`].
const GONE_AT_START: Duration = Duration::from_secs(10);
const GONE_RETRY: Duration = Duration::from_millis(250);

/// Why a microphone that failed to open at start is waited for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AtStart {
    Busy,
    Gone,
}

/// A microphone that failed to open at start: busy, or off the bus. Each
/// reason's wait runs from its own first answer and ends after its bound, so
/// one coming back from off the bus may still answer busy for a moment.
#[derive(Default)]
pub(crate) struct OpenAtStart {
    busy_since: Option<Instant>,
    gone_since: Option<Instant>,
    /// Failed opens so far.
    failed: u32,
    next: Option<Instant>,
    /// The holder of a busy microphone was looked up and said.
    busy_said: bool,
    /// How long busy is waited for, when not [`BUSY_AT_START`].
    busy_bound: Option<Duration>,
}

impl OpenAtStart {
    /// Busy waited for up to `bound`, not [`BUSY_AT_START`].
    fn busy_for(bound: Duration) -> Self {
        Self {
            busy_bound: Some(bound),
            ..Self::default()
        }
    }

    /// True while the open may be tried again after `error`.
    pub(crate) fn waits(&mut self, error: &CaptureError, now: Instant) -> bool {
        self.waits_after(error.device_error, now)
    }

    /// True while the open may be tried again after it failed with `fault`:
    /// busy or off the bus, by ALSA's own name for the open.
    fn waits_after(&mut self, fault: Option<DeviceFault>, now: Instant) -> bool {
        let Some(why) = fault.and_then(|fault| {
            (fault.call == OPEN_CALL)
                .then_some(match fault.errno {
                    errno::BUSY => Some(AtStart::Busy),
                    errno::NODEV => Some(AtStart::Gone),
                    _ => None,
                })
                .flatten()
        }) else {
            return false;
        };
        let (since, bound, retry) = match why {
            AtStart::Busy => (
                &mut self.busy_since,
                self.busy_bound.unwrap_or(BUSY_AT_START),
                BUSY_RETRY,
            ),
            AtStart::Gone => (&mut self.gone_since, GONE_AT_START, GONE_RETRY),
        };
        let since = *since.get_or_insert(now);
        self.failed = self.failed.saturating_add(1);
        self.next = Some(now + retry);
        now.duration_since(since) < bound
    }

    /// True once, after the first busy answer: who holds the microphone is
    /// looked up and said then, not on every try.
    pub(crate) fn busy_first(&mut self) -> bool {
        self.busy_since.is_some() && !std::mem::replace(&mut self.busy_said, true)
    }

    /// Whether to try the open on this tick.
    pub(crate) fn due(&self, now: Instant) -> bool {
        self.next.is_none_or(|next| now >= next)
    }

    /// The microphone opened: whether it was off USB at all (else only
    /// busy), how long since the first failed open, and on which try.
    pub(crate) fn opened(&mut self, now: Instant) -> Option<OpenWaited> {
        let gone = self.gone_since.take();
        let first = match (self.busy_since.take(), gone) {
            (Some(busy), Some(gone)) => Some(busy.min(gone)),
            (busy, gone) => busy.or(gone),
        };
        let failed = std::mem::take(&mut self.failed);
        self.next = None;
        Some(OpenWaited {
            gone: gone.is_some(),
            waited_ms: u64::try_from(now.duration_since(first?).as_millis()).unwrap_or(u64::MAX),
            opened_on_try: failed.saturating_add(1),
        })
    }
}

/// Opens the speaker, and while `open` fails busy (`EBUSY` from
/// `snd_pcm_open`, as `fault` reads it) tries again every [`BUSY_RETRY`] for
/// up to [`SPEAKER_BUSY_AT_START`], as the microphone is for its own bound
/// (#4332). Off the bus, or any other answer, still ends the start at once.
/// `first_busy` runs once, at the first busy answer; `wait_until` waits
/// until the next try. With what was waited, if anything.
///
/// The speaker worker runs this before it says it is ready. No authority
/// has been sent to it yet, so no lease can lapse while it waits; the
/// coordinator is told by `first_busy` and waits for it that much longer,
/// and still ends it on a stop (`process::Worker::wait_until_ready`).
pub(crate) fn open_waiting_out_busy<S>(
    mut open: impl FnMut() -> Result<S>,
    fault: impl Fn(&Error) -> Option<DeviceFault>,
    mut first_busy: impl FnMut() -> Result<()>,
    mut wait_until: impl FnMut(Instant) -> Result<()>,
    mut now: impl FnMut() -> Instant,
) -> Result<(S, Option<OpenWaited>)> {
    let mut at_start = OpenAtStart::busy_for(SPEAKER_BUSY_AT_START);
    loop {
        let error = match open() {
            Ok(opened) => return Ok((opened, at_start.opened(now()))),
            Err(error) => error,
        };
        let busy = fault(error.as_ref()).filter(|fault| fault.errno == errno::BUSY);
        if !at_start.waits_after(busy, now()) {
            return Err(error);
        }
        if at_start.busy_first() {
            first_busy()?;
        }
        if let Some(next) = at_start.next {
            wait_until(next)?;
        }
    }
}

#[cfg(test)]
mod open_at_start_tests {
    use super::*;
    use lamp_audio::capture::ErrorKind as CaptureErrorKind;

    fn failed(call: &'static str, errno: i32) -> CaptureError {
        CaptureError {
            kind: CaptureErrorKind::NotPermitted,
            device_error: Some(DeviceFault::new(call, errno)),
            close_error: None,
        }
    }

    /// A microphone off the bus at start is tried every 250 ms for 10 s, one
    /// busy at start every 50 ms for 8 s, and only an open that failed so:
    /// by ALSA's own name for it, which the lamp's errors carry.
    #[test]
    fn a_microphone_gone_or_busy_at_start_is_waited_for_within_its_bound() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let gone = failed(OPEN_CALL, errno::NODEV);
        let mut wait = OpenAtStart::default();
        assert!(wait.due(at(0)));
        assert!(wait.waits(&gone, at(0)));
        assert!(!wait.due(at(249)));
        assert!(wait.due(at(250)));
        assert!(wait.waits(&gone, at(9_999)));
        assert!(!wait.waits(&gone, at(10_000)));
        let mut back = OpenAtStart::default();
        assert!(back.waits(&gone, at(0)));
        assert!(!back.busy_first(), "off USB is not busy");
        assert!(back.waits(&gone, at(250)));
        let waited = back.opened(at(4_500)).unwrap();
        assert_eq!(
            waited,
            crate::wire::OpenWaited {
                gone: true,
                waited_ms: 4_500,
                opened_on_try: 3
            }
        );
        assert_eq!(
            waited.plain(),
            "the voice mic was off USB for 4.5 s at start; opened on try 3"
        );
        assert!(back.due(at(4_500)));
        assert_eq!(back.opened(at(4_600)), None);

        let busy = failed(OPEN_CALL, errno::BUSY);
        let mut wait = OpenAtStart::default();
        assert!(wait.waits(&busy, at(0)));
        assert!(wait.busy_first(), "its holder is said on the first busy");
        assert!(wait.waits(&busy, at(50)));
        assert!(!wait.busy_first(), "and only once");
        assert!(!wait.due(at(99)));
        assert!(wait.due(at(100)), "busy is tried again every 50 ms");
        assert!(wait.waits(&busy, at(7_999)));
        assert!(!wait.waits(&busy, at(8_000)));

        // Gone for 4.5 s, then busy for a moment as it comes back: busy's
        // 8 s run from its own first answer, and gone's 10 s from its own.
        let mut back = OpenAtStart::default();
        assert!(back.waits(&gone, at(0)));
        assert!(back.waits(&busy, at(4_500)));
        assert!(back.waits(&busy, at(7_499)));
        // Off USB first, so it says so.
        assert_eq!(
            back.opened(at(7_500)),
            Some(crate::wire::OpenWaited {
                gone: true,
                waited_ms: 7_500,
                opened_on_try: 4
            })
        );
        let mut back = OpenAtStart::default();
        assert!(back.waits(&busy, at(0)));
        assert!(back.waits(&gone, at(2_000)));
        assert!(back.waits(&gone, at(11_999)));
        assert!(!back.waits(&gone, at(12_000)));
        assert!(!back.waits(&busy, at(8_000)), "busy's own 8 s are over");

        for other in [
            failed(OPEN_CALL, errno::IO),
            failed(OPEN_CALL, errno::NOENT),
            failed("open", errno::NODEV),
            failed("snd_pcm_hw_params", errno::NODEV),
            CaptureError {
                kind: CaptureErrorKind::NotPermitted,
                device_error: None,
                close_error: None,
            },
        ] {
            assert!(!OpenAtStart::default().waits(&other, at(0)), "{other}");
        }
    }

    /// The speaker's open, answered busy until `free` tries, then opened;
    /// or answered `errno` from `call` on every try.
    struct Opens {
        tries: u32,
        free: u32,
        call: &'static str,
        errno: i32,
    }

    impl Opens {
        fn open(&mut self) -> Result<&'static str> {
            self.tries += 1;
            if self.tries > self.free {
                return Ok("speaker");
            }
            Err(Box::new(DeviceFault::new(self.call, self.errno)))
        }
    }

    fn fault(error: &Error) -> Option<DeviceFault> {
        error.downcast_ref::<DeviceFault>().copied()
    }

    /// What the speaker's open came to: the speaker, or the error.
    type Opened = Result<(&'static str, Option<OpenWaited>)>;

    /// The speaker's open on a clock that moves only when it waits: the
    /// result, the opens tried, the first-busy calls and when it ended, ms.
    fn run(mut opens: Opens) -> (Opened, u32, u32, u64) {
        let start = Instant::now();
        let clock = std::cell::Cell::new(start);
        let mut said = 0;
        let result = open_waiting_out_busy(
            || opens.open(),
            fault,
            || {
                said += 1;
                Ok(())
            },
            |until| {
                assert!(until > clock.get(), "each wait moves on");
                clock.set(until);
                Ok(())
            },
            || clock.get(),
        );
        let ended = clock.get().duration_since(start).as_millis() as u64;
        (result, opens.tries, said, ended)
    }

    /// A speaker busy at start, as PulseAudio held lamp-06c1's for about 9 s
    /// (#4332), is tried every 50 ms and opens once it is free, past the
    /// microphone's 8 s, saying so once; one still busy after 20 s ends the
    /// start; off the bus, or any other answer, ends it at the first try, as
    /// before. On a clock that moves only when it waits.
    #[test]
    fn a_speaker_busy_at_start_is_waited_for_and_nothing_else_is() {
        let busy = |free| Opens {
            tries: 0,
            free,
            call: OPEN_CALL,
            errno: errno::BUSY,
        };
        let (opened, tries, said, ended) = run(busy(0));
        assert_eq!(opened.unwrap(), ("speaker", None), "free at once");
        assert_eq!((tries, said, ended), (1, 0, 0));

        let (opened, tries, said, ended) = run(busy(20));
        let waited = OpenWaited {
            gone: false,
            waited_ms: 1_000,
            opened_on_try: 21,
        };
        assert_eq!(opened.unwrap(), ("speaker", Some(waited)));
        assert_eq!((tries, said, ended), (21, 1, 1_000));

        // lamp-06c1's 9 s: free on the try at 9.2 s, past the mic's 8 s.
        let (opened, tries, said, ended) = run(busy(184));
        let waited = OpenWaited {
            gone: false,
            waited_ms: 9_200,
            opened_on_try: 185,
        };
        assert_eq!(opened.unwrap(), ("speaker", Some(waited)));
        assert_eq!((tries, said, ended), (185, 1, 9_200));

        let (opened, tries, said, ended) = run(busy(u32::MAX));
        let error = opened.unwrap_err();
        assert_eq!(fault(error.as_ref()).unwrap().errno, errno::BUSY);
        assert_eq!((tries, said), (401, 1), "every 50 ms for 20 s");
        assert_eq!(ended, 20_000);

        for (call, errno) in [
            (OPEN_CALL, errno::NODEV),
            (OPEN_CALL, errno::IO),
            (OPEN_CALL, errno::NOENT),
            ("snd_pcm_hw_params", errno::BUSY),
        ] {
            let (opened, tries, said, ended) = run(Opens {
                tries: 0,
                free: u32::MAX,
                call,
                errno,
            });
            let error = opened.unwrap_err();
            assert_eq!(fault(error.as_ref()), Some(DeviceFault::new(call, errno)));
            assert_eq!((tries, said, ended), (1, 0, 0), "{call} {errno}");
        }
    }
}
