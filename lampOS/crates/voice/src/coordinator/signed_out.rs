//! The lamp signed out (`LAMP_SIGNED_OUT`, `docs/failure-notices.md`,
//! "Signed out"). `lamp-cloud` works it out from the ping's answers and says
//! it on `account/state`; a spoken reply needs the same login, so here the
//! turns are listened to and never sent, the kept recording is said once a
//! spell with nothing asked to someone at the lamp, and again when someone
//! talks, at most once in [`HEARD_LINE_EVERY_US`]. Nothing is
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

/// The line is owed unasked this long, and then let go: nobody came, and the
/// line said when someone talks covers them. The same bound as "back online".
pub(super) const UNASKED_OWED_US: u64 = notices::BACK_ONLINE_OWED_US;

/// Heard speech gets the line at most this often, and only the breath in
/// between. A lamp that never signs back in by itself may have a TV or two
/// people talking beside it for days: a line after every sentence reads as a
/// machine, while someone who comes to it later still hears why within minutes.
pub(super) const HEARD_LINE_EVERY_US: u64 = 300_000_000;

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
    /// When the line began to be owed, for [`UNASKED_OWED_US`].
    owed_since_us: Option<u64>,
    /// When the unasked line was last settled: said, or let go. A state read
    /// missing for a while and back in the same spell owes nothing more.
    settled_at_us: Option<u64>,
    /// When a line (or the sigh in its place) last played.
    said_at_us: Option<u64>,
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
            self.take(Some(state(clock.saturating_sub(first))), clock);
            self.let_go_unsaid(clock);
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
            clock,
        );
        self.let_go_unsaid(clock);
    }

    /// Takes what was read: a spell beginning owes the line once, one ending
    /// lets it go. `for_s` tells a spell read again after a gap from a new one:
    /// one that began before the line was settled has had its line.
    pub(super) fn take(&mut self, state: Option<AccountState>, clock: u64) {
        let state = state.unwrap_or_default();
        if state.signed_out && !self.now {
            // A second's slack: `for_s` is whole seconds.
            let began_us_ago = state.for_s.saturating_add(1).saturating_mul(1_000_000);
            let settled = self
                .settled_at_us
                .is_some_and(|at| clock.saturating_sub(at) <= began_us_ago);
            self.owed = state.for_s < SAID_UNASKED_WITHIN_S && !settled;
            self.owed_since_us = None;
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

    /// Whether a line of speech heard now may get the line, not just the
    /// breath.
    pub(super) fn may_say_heard(&self, clock: u64) -> bool {
        self.said_at_us
            .is_none_or(|at| clock.saturating_sub(at) >= HEARD_LINE_EVERY_US)
    }

    /// Lets the unasked line go once it has been owed [`UNASKED_OWED_US`].
    fn let_go_unsaid(&mut self, clock: u64) {
        if !self.owed() {
            self.owed_since_us = None;
            return;
        }
        let since = *self.owed_since_us.get_or_insert(clock);
        if clock.saturating_sub(since) >= UNASKED_OWED_US {
            self.owed = false;
            self.settled_at_us = Some(clock);
            eprintln!("lamp-voice: nobody came: the signed-out line waits for someone to talk");
        }
    }

    /// The line counts as said once the speaker has taken some of it or it
    /// retired, as the offline line does: a Stop before that leaves it owed.
    /// A line with no recording has nothing to say, and is let go.
    /// Called every loop, before the turn can go.
    pub(super) fn note_delivery(&mut self, reply: &Option<Reply>, clock: u64) {
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
                self.settled_at_us = Some(clock);
                self.said_at_us = Some(clock);
            }
            NoticeDelivery::Skipped(_) => {
                self.owed = false;
                self.settled_at_us = Some(clock);
            }
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
        state.take(None, 0);
        assert!(!state.holds() && !state.owed());
        state.take(out(0), 0);
        assert!(state.holds() && state.owed());
        state.owed = false;
        // The same spell read again owes nothing more.
        state.take(out(5), 0);
        assert!(state.holds() && !state.owed());
        // It ended; the next one owes the line again.
        state.take(Some(AccountState::default()), 0);
        assert!(!state.holds() && !state.owed());
        state.take(out(0), 0);
        assert!(state.owed());
    }

    #[test]
    fn a_stale_or_missing_state_is_signed_in_and_lets_the_line_go() {
        let mut state = SignedOut::default();
        state.take(out(0), 0);
        state.take(None, 0);
        assert!(!state.holds() && !state.owed());
    }

    #[test]
    fn a_spell_already_old_when_first_read_holds_but_is_not_said_unasked() {
        let mut state = SignedOut::default();
        state.take(out(SAID_UNASKED_WITHIN_S - 1), 0);
        assert!(state.owed());
        let mut restarted = SignedOut::default();
        restarted.take(out(SAID_UNASKED_WITHIN_S), 0);
        assert!(restarted.holds() && !restarted.owed());
    }

    #[test]
    fn without_the_switch_nothing_is_read() {
        let mut state = SignedOut::default();
        state.read(10_000_000);
        assert!(!state.holds() && state.read_at_us.is_none());
    }

    #[test]
    fn heard_speech_gets_the_line_only_once_in_its_bound() {
        let mut state = SignedOut::default();
        state.take(out(0), 0);
        assert!(state.may_say_heard(0));
        state.said_at_us = Some(1_000);
        assert!(!state.may_say_heard(1_000 + HEARD_LINE_EVERY_US - 1));
        assert!(state.may_say_heard(1_000 + HEARD_LINE_EVERY_US));
    }

    #[test]
    fn the_unasked_line_is_let_go_when_nobody_comes() {
        let mut state = SignedOut::default();
        state.take(out(0), 0);
        state.let_go_unsaid(1_000);
        state.let_go_unsaid(1_000 + UNASKED_OWED_US - 1);
        assert!(state.owed());
        state.let_go_unsaid(1_000 + UNASKED_OWED_US);
        assert!(state.holds() && !state.owed());
    }

    #[test]
    fn a_gap_in_the_state_is_not_a_new_spell_and_a_new_spell_owes_the_line() {
        let second = 1_000_000;
        let mut state = SignedOut::default();
        state.take(out(0), 0);
        // Said at 10 s; the state goes missing, and comes back in the same
        // spell at 40 s, still under the five minutes.
        state.owed = false;
        state.settled_at_us = Some(10 * second);
        state.take(None, 20 * second);
        assert!(!state.holds());
        state.take(out(40), 40 * second);
        assert!(state.holds() && !state.owed());
        // Signed in, then a spell that began after the line was said.
        state.take(Some(AccountState::default()), 50 * second);
        state.take(out(0), 60 * second);
        assert!(state.owed());
    }
}
