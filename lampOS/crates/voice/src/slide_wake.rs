//! The privacy slide as the lamp's sleep and wake (migrate-P4,
//! `LAMP_SLIDE_WAKES`, off unless `on`; the Queue comments
//! <https://github.com/autonomous-ai/lampOS/issues/4710#issuecomment-6568874567>
//! and <https://github.com/autonomous-ai/lampOS/issues/4710#issuecomment-6601369650>).
//! A slide that closes asks the sleep a hand's hold does, and one that has
//! stayed open for [`SETTLE_US`] asks the wake a tap does. It reads only the
//! switch's states: it never grants a microphone or says the switch is open.
use crate::privacy::PrivacyState;

/// Off unless `on`.
pub const SWITCH: &str = "LAMP_SLIDE_WAKES";

/// How long a slide that opened stays open before the lamp wakes: the design
/// owner's "about 1 s". A slide closed again in it wakes nothing.
pub const SETTLE_US: u64 = 1_000_000;

pub fn enabled_from_environment() -> bool {
    std::env::var(SWITCH).as_deref() == Ok("on")
}

/// What the slide asks of the lamp at a look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    Sleep,
    Wake,
}

/// Follows the slide's states through the muted wait. Only a person's own
/// move counts: a slide first read open, or one read again after it could not
/// be read, asks nothing.
#[derive(Debug, Default)]
pub struct SlideWake {
    closed: bool,
    sleep_owed: bool,
    open: bool,
    open_since: Option<u64>,
}

impl SlideWake {
    pub fn said(&mut self, state: PrivacyState) {
        match state {
            PrivacyState::Muted => {
                self.sleep_owed |= !self.closed;
                self.closed = true;
                self.open = false;
                self.open_since = None;
            }
            PrivacyState::Stabilizing => {
                self.open = false;
                self.open_since = None;
            }
            PrivacyState::Allowed => self.open = self.closed,
            PrivacyState::Unavailable => *self = Self::default(),
        }
    }

    /// Whether a slide that opened is waiting out [`SETTLE_US`].
    pub fn settling(&self) -> bool {
        self.open
    }

    pub fn look(&mut self, now_us: u64) -> Option<Asked> {
        if std::mem::take(&mut self.sleep_owed) {
            return Some(Asked::Sleep);
        }
        if !self.open {
            return None;
        }
        let since = *self.open_since.get_or_insert(now_us);
        if now_us.saturating_sub(since) < SETTLE_US {
            return None;
        }
        *self = Self::default();
        Some(Asked::Wake)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: u64 = 10_000_000;

    fn closed() -> SlideWake {
        let mut slide = SlideWake::default();
        slide.said(PrivacyState::Muted);
        assert_eq!(slide.look(T), Some(Asked::Sleep));
        slide
    }

    #[test]
    fn a_slide_closing_asks_sleep_once() {
        let mut slide = closed();
        slide.said(PrivacyState::Muted);
        assert_eq!(slide.look(T + 1), None);
    }

    #[test]
    fn a_slide_opening_asks_the_wake_once_after_it_settles() {
        let mut slide = closed();
        slide.said(PrivacyState::Stabilizing);
        assert_eq!(slide.look(T + 1), None);
        slide.said(PrivacyState::Allowed);
        assert!(slide.settling());
        assert_eq!(slide.look(T + 2), None);
        assert_eq!(slide.look(T + 2 + SETTLE_US - 1), None);
        assert_eq!(slide.look(T + 2 + SETTLE_US), Some(Asked::Wake));
        assert!(!slide.settling());
        assert_eq!(slide.look(T + 3 + SETTLE_US), None);
    }

    #[test]
    fn a_slide_closed_again_before_it_settles_wakes_nothing() {
        let mut slide = closed();
        slide.said(PrivacyState::Allowed);
        assert_eq!(slide.look(T + 1), None);
        slide.said(PrivacyState::Muted);
        assert_eq!(slide.look(T + 1 + SETTLE_US), None);
        slide.said(PrivacyState::Allowed);
        assert_eq!(slide.look(T + 2 + SETTLE_US), None);
        assert_eq!(slide.look(T + 2 + 2 * SETTLE_US), Some(Asked::Wake));
    }

    #[test]
    fn a_slide_first_read_open_asks_nothing() {
        let mut slide = SlideWake::default();
        slide.said(PrivacyState::Allowed);
        assert!(!slide.settling());
        assert_eq!(slide.look(T), None);
        assert_eq!(slide.look(T + SETTLE_US), None);
    }

    #[test]
    fn a_slide_read_again_after_it_could_not_be_read_asks_nothing() {
        let mut slide = closed();
        slide.said(PrivacyState::Unavailable);
        slide.said(PrivacyState::Allowed);
        assert_eq!(slide.look(T + 1), None);
        assert_eq!(slide.look(T + 1 + SETTLE_US), None);
    }
}
