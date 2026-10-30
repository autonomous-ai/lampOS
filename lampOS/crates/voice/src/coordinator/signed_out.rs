//! The lamp signed out (`LAMP_SIGNED_OUT`, `docs/failure-notices.md`,
//! "Signed out"). `lamp-cloud` works it out from the ping's answers and says
//! it on `account/state`; a spoken reply needs the same login, so here the
//! turns are listened to and never sent, the kept recording is said once a
//! spell with nothing asked, and again whenever someone talks. Nothing is
//! tried until `lamp-cloud` says the lamp is signed in again. A value past
//! its lifespan, or none, is read as signed in: a stopped or older
//! `lamp-cloud` never holds a turn.
use super::*;
use lamp_msgs::AccountState;

/// The switch, exactly `on`. Off unless set.
pub(super) const ENVIRONMENT: &str = "LAMP_SIGNED_OUT";

/// A spell older than this when first read is not said unasked: a voice
/// that restarted in it has said it. Longer than any restart of the lamp's
/// parts, and a voice that was down for the spell's start still tells the
/// next person who talks.
pub(super) const SAID_UNASKED_WITHIN_S: u64 = 300;

/// How often the kept state is read, never waiting longer than a file read.
const READ_EVERY_US: u64 = 1_000_000;

pub(super) fn switched_on() -> bool {
    #[cfg(test)]
    if FOR_TEST.with(std::cell::Cell::get).is_some() {
        return true;
    }
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

#[cfg(test)]
type ForTest = Option<fn(u64) -> AccountState>;

#[cfg(test)]
thread_local! {
    static FOR_TEST: std::cell::Cell<ForTest> = const { std::cell::Cell::new(None) };
}

/// This thread's state as a function of the microseconds since the first
/// read, for a test; `None` reads the switch and the bus.
#[cfg(test)]
pub(super) fn set_for_test(state: ForTest) {
    FOR_TEST.with(|cell| cell.set(state));
}

#[derive(Default)]
pub(super) struct SignedOut {
    bus: Option<lamp_bus::Bus>,
    on: bool,
    now: bool,
    /// The line is owed unasked: this spell began, and it is not said yet.
    owed: bool,
    read_at_us: Option<u64>,
    #[cfg(test)]
    first_read_us: Option<u64>,
}

impl SignedOut {
    pub(super) fn from_environment() -> Self {
        let on = switched_on();
        Self {
            on,
            bus: on.then(|| lamp_bus::Bus::from_environment().ok()).flatten(),
            ..Self::default()
        }
    }

    /// Reads the kept state, at most once a second.
    pub(super) fn read(&mut self, clock: u64) {
        if !self.on
            || self
                .read_at_us
                .is_some_and(|at| clock.saturating_sub(at) < READ_EVERY_US)
        {
            return;
        }
        self.read_at_us = Some(clock);
        #[cfg(test)]
        if let Some(state) = FOR_TEST.with(std::cell::Cell::get) {
            let first = *self.first_read_us.get_or_insert(clock);
            self.take(Some(state(clock.saturating_sub(first))));
            return;
        }
        let message = self
            .bus
            .as_ref()
            .and_then(|bus| bus.latest(AccountState::TOPIC).ok().flatten());
        self.take(
            message
                .filter(|message| !message.expired)
                .and_then(|message| message.parse::<AccountState>().ok()),
        );
    }

    /// Takes what was read: a spell beginning owes the line once, one ending
    /// lets it go.
    pub(super) fn take(&mut self, state: Option<AccountState>) {
        let state = state.unwrap_or_default();
        if state.signed_out && !self.now {
            self.owed = state.for_s < SAID_UNASKED_WITHIN_S;
            eprintln!(
                "lamp-voice: this lamp is signed out: turns are listened to and not sent, and the lamp says so (LAMP_SIGNED_OUT)"
            );
        } else if !state.signed_out && self.now {
            self.owed = false;
            eprintln!("lamp-voice: this lamp is signed in again: turns are sent");
        }
        self.now = state.signed_out;
    }

    /// Whether turns are held, not sent.
    pub(super) fn holds(&self) -> bool {
        self.now
    }

    /// Whether the lamp should take a turn for itself to say the line.
    pub(super) fn owed(&self) -> bool {
        self.now && self.owed
    }

    /// The line counts as said once the speaker has taken some of it or it
    /// retired, as the offline line does: a Stop before that leaves it owed.
    /// A line with no recording has nothing to say, and is let go.
    /// Called every loop, before the turn can go.
    pub(super) fn note_delivery(&mut self, reply: &Option<Reply>) {
        let Some(current) = reply.as_ref() else {
            return;
        };
        if !current.local_only.is_some_and(|local| local.signed_out) {
            return;
        }
        match &current.notice {
            NoticeDelivery::Selected { retired_at_us, .. }
                if current.playback.is_some() || retired_at_us.is_some() =>
            {
                self.owed = false;
            }
            NoticeDelivery::Skipped(_) => self.owed = false,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(for_s: u64) -> Option<AccountState> {
        Some(AccountState {
            signed_out: true,
            for_s,
        })
    }

    #[test]
    fn a_spell_owes_the_line_once_and_holds_until_it_ends() {
        let mut state = SignedOut::default();
        state.take(None);
        assert!(!state.holds() && !state.owed());
        state.take(out(0));
        assert!(state.holds() && state.owed());
        state.owed = false;
        // The same spell read again owes nothing more.
        state.take(out(5));
        assert!(state.holds() && !state.owed());
        // It ended; the next one owes the line again.
        state.take(Some(AccountState::default()));
        assert!(!state.holds() && !state.owed());
        state.take(out(0));
        assert!(state.owed());
    }

    #[test]
    fn a_stale_or_missing_state_is_signed_in_and_lets_the_line_go() {
        let mut state = SignedOut::default();
        state.take(out(0));
        state.take(None);
        assert!(!state.holds() && !state.owed());
    }

    #[test]
    fn a_spell_already_old_when_first_read_holds_but_is_not_said_unasked() {
        let mut state = SignedOut::default();
        state.take(out(SAID_UNASKED_WITHIN_S - 1));
        assert!(state.owed());
        let mut restarted = SignedOut::default();
        restarted.take(out(SAID_UNASKED_WITHIN_S));
        assert!(restarted.holds() && !restarted.owed());
    }

    #[test]
    fn without_the_switch_nothing_is_read() {
        let mut state = SignedOut::default();
        state.read(10_000_000);
        assert!(!state.holds() && state.read_at_us.is_none());
    }
}
