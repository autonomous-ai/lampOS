//! The model service refusing every turn (#4265, `LAMP_SECOND_REFUSED_HOLD`).
//!
//! On 29 Oct the relay's chat answered every round with 402 in under a
//! second: the lamp said nothing to anyone, and setup failed its steps. With
//! the switch, a turn that fails because a service refused it or could not
//! be reached starts a refusal: the turns after it are listened to and never
//! sent, as while only the voice service is down (#830), so the lamp says
//! the recorded `voice_service_unreachable` line once and breathes its calm
//! offline light; [`TRY_AGAIN_EVERY_US`] after the last refused turn, the
//! lines are sent again to find the service back, until one's chat is
//! refused again (a line heard as no words asks nothing, so it is no try:
//! lamp-52e6's room sounds took each minute's try on 30 Oct), and the
//! first turn the model answers ends the refusal. A turn that is only slow
//! is no refusal: it fails as before. A quota or key refusal starts one at
//! once (a 402 or 401 doesn't clear in a minute); any other only on the second refused turn in a row, so one
//! passing 503 or 429 holds nothing; and a turn cut while its answer was
//! already playing never does: the service was answering (Reviewer-three's
//! P2 on #4273). An onboarding line's own words, said when its chat fails
//! (#4263), are no answer of the model's and end nothing (the
//! Reviewer-lead's P2 on #4273). The fault
//! carries no HTTP status, so a 400 is counted with the other 4xx
//! (`ServerRejected`): a request every round gets wrong leaves the lamp as
//! unable to answer as a refusal does.
use super::*;
use crate::provider_worker::{FailedStage, Fault};

/// The switch, exactly `on`. Off unless set.
pub(super) const ENVIRONMENT: &str = "LAMP_SECOND_REFUSED_HOLD";

/// Whether the switch is on: [`ENVIRONMENT`] exactly `on`, or this
/// thread's test setting.
pub(super) fn switched_on() -> bool {
    #[cfg(test)]
    if let Some(on) = FOR_TEST.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

#[cfg(test)]
thread_local! {
    static FOR_TEST: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// This thread's switch, for a test; `None` reads the environment again.
#[cfg(test)]
pub(super) fn set_for_test(on: Option<bool>) {
    FOR_TEST.with(|cell| cell.set(on));
}

/// While refusing, one turn is sent to find the service back this often.
pub(super) const TRY_AGAIN_EVERY_US: u64 = 60_000_000;

/// The refusal a turn's fault is, as said in the journal and in Health:
/// none for a turn only slow, cut, or failed inside the lamp.
pub(super) fn kind(fault: Fault) -> Option<&'static str> {
    match fault {
        Fault::QuotaExceeded => Some("quota"),
        Fault::Authentication => Some("key"),
        Fault::RateLimited => Some("rate_limited"),
        Fault::ServerUnavailable => Some("server_error"),
        Fault::ServerRejected => Some("rejected"),
        Fault::ConnectTimeout | Fault::Transport => Some("unreachable"),
        _ => None,
    }
}

#[derive(Debug, Default)]
pub(super) struct ModelRefusing {
    /// When the refusal began, and when a turn was last sent during it.
    since_us: Option<u64>,
    tried_at_us: u64,
    /// The kinds already said in the journal this refusal: one line each.
    said: Vec<&'static str>,
    /// Turns refused in a row, with no answer between, before one began.
    in_a_row: u32,
}

impl ModelRefusing {
    /// Takes a turn's outcome: a refusal starts or goes on, a turn the model
    /// answered ends it. `fixed_line`: the turn is an onboarding line, whose
    /// own words are said when its chat fails, so its completion is no
    /// answer. True when this ended one, so its outage starts afresh.
    pub(super) fn observe(
        &mut self,
        event: &ProviderOutput,
        now_us: u64,
        fixed_line: bool,
    ) -> bool {
        match event {
            ProviderOutput::TurnFailed {
                request,
                stage,
                reason,
            } => {
                let Some(kind) = kind(*reason).filter(|_| *stage != FailedStage::Responding) else {
                    return false;
                };
                self.in_a_row = self.in_a_row.saturating_add(1);
                if !self.refusing() && !matches!(kind, "quota" | "key") && self.in_a_row < 2 {
                    return false;
                }
                self.since_us.get_or_insert(now_us);
                self.tried_at_us = now_us;
                if !self.said.contains(&kind) {
                    self.said.push(kind);
                    eprintln!(
                        "lamp-voice: request {request}: the model service refused the turn ({kind}); turns are held and the lamp says so, one sent every {} s (LAMP_SECOND_REFUSED_HOLD)",
                        TRY_AGAIN_EVERY_US / 1_000_000
                    );
                }
                false
            }
            // Answered, as `KeyRefusals` reads it: the model's generation
            // finished, never an idle `TurnComplete` alone.
            ProviderOutput::GenerationComplete { request } if !fixed_line => {
                self.in_a_row = 0;
                let Some(since) = self.since_us else {
                    return false;
                };
                eprintln!(
                    "lamp-voice: request {request}: the model service answers again after {} s of refusals",
                    now_us.saturating_sub(since) / 1_000_000
                );
                *self = Self::default();
                true
            }
            _ => false,
        }
    }

    pub(super) fn refusing(&self) -> bool {
        self.since_us.is_some()
    }

    /// Whether a turn heard now is held, not sent: refusing, and a turn
    /// was refused, or an onboarding line sent, less than
    /// [`TRY_AGAIN_EVERY_US`] ago. A spoken line sent is no try until its
    /// chat is refused: one heard as no words never asks the model.
    pub(super) fn holds(&self, now_us: u64) -> bool {
        self.refusing() && now_us.saturating_sub(self.tried_at_us) < TRY_AGAIN_EVERY_US
    }

    /// An onboarding line sent while refusing, whose chat is always asked:
    /// the next turn waits its time again.
    pub(super) fn tried(&mut self, now_us: u64) {
        if self.refusing() {
            self.tried_at_us = now_us;
        }
    }

    /// For Health: the refusal's kinds so far and how long it has lasted.
    pub(super) fn health(&self, now_us: u64) -> Option<(String, u64)> {
        let since = self.since_us?;
        Some((self.said.join(","), now_us.saturating_sub(since)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(request: u64, reason: Fault) -> ProviderOutput {
        failed_at(request, FailedStage::AwaitingResponse, reason)
    }

    fn failed_at(request: u64, stage: FailedStage, reason: Fault) -> ProviderOutput {
        ProviderOutput::TurnFailed {
            request,
            stage,
            reason,
        }
    }

    fn answered(request: u64) -> ProviderOutput {
        ProviderOutput::GenerationComplete { request }
    }

    /// 29 Oct's 402s start a refusal at once; a 503, a refused connection
    /// and a connect timeout on the second refused turn in a row. A turn
    /// the model answers ends it; an idle TurnComplete does not.
    #[test]
    fn a_402_starts_at_once_and_a_503_or_refused_connection_on_the_second_in_a_row() {
        use crate::second_provider::{fault, net::Error};
        let mut quota = ModelRefusing::default();
        quota.observe(&failed(1, fault(&Error::Status(402))), 10_000_000, false);
        assert!(quota.refusing());
        assert!(quota.holds(10_000_000 + TRY_AGAIN_EVERY_US - 1));
        assert!(!quota.holds(10_000_000 + TRY_AGAIN_EVERY_US));
        for error in [
            Error::Status(503),
            Error::Transport("connect"),
            Error::Timeout("connect"),
        ] {
            let mut refusing = ModelRefusing::default();
            refusing.observe(&failed(1, fault(&error)), 0, false);
            assert!(!refusing.refusing(), "one is no refusal: {error:?}");
            refusing.observe(&failed(2, fault(&error)), 1, false);
            assert!(refusing.refusing(), "{error:?}");
            refusing.observe(
                &ProviderOutput::TurnComplete {
                    request: 3,
                    idle: true,
                },
                2,
                false,
            );
            assert!(refusing.refusing());
            assert!(refusing.observe(&answered(4), 3, false));
            assert!(!refusing.refusing() && !refusing.holds(3));
        }
    }

    /// Reviewer-three's P2 on #4273: one passing 503 followed by an
    /// answered turn holds nothing, and neither does a turn cut while its
    /// answer was already playing.
    #[test]
    fn one_passing_503_or_a_drop_mid_answer_holds_nothing() {
        let mut refusing = ModelRefusing::default();
        refusing.observe(&failed(1, Fault::ServerUnavailable), 0, false);
        refusing.observe(&answered(2), 1, false);
        refusing.observe(&failed(3, Fault::ServerUnavailable), 2, false);
        assert!(!refusing.refusing(), "an answer between: not in a row");
        let mut cut = ModelRefusing::default();
        for request in 1..=3 {
            cut.observe(
                &failed_at(request, FailedStage::Responding, Fault::Transport),
                request,
                false,
            );
        }
        assert!(!cut.refusing() && !cut.holds(4));
    }

    /// The Reviewer-lead's P2 on #4273: an onboarding line whose own words
    /// were said on a failed chat (#4263) completes, but ends nothing.
    #[test]
    fn an_onboarding_lines_own_words_end_no_refusal() {
        let mut refusing = ModelRefusing::default();
        refusing.observe(&failed(1, Fault::QuotaExceeded), 0, false);
        assert!(!refusing.observe(&answered(2), 1, true));
        assert!(refusing.refusing());
        assert!(refusing.observe(&answered(3), 2, false));
    }

    /// Slow is not refused (#4089's): a response or a read that timed out,
    /// a stall, or a fault inside the lamp holds nothing, however often.
    #[test]
    fn a_slow_or_local_failure_is_no_refusal() {
        for reason in [
            Fault::ResponseTimeout,
            Fault::ReadTimeout,
            Fault::StalledResponse,
            Fault::CompletionTimeout,
            Fault::Local,
        ] {
            let mut refusing = ModelRefusing::default();
            for request in 1..=3 {
                refusing.observe(&failed(request, reason), request, false);
            }
            assert!(!refusing.refusing(), "{reason:?}");
        }
    }

    /// One turn sent while refusing, and failing again, waits its time
    /// afresh; the kinds are kept for Health, each said once.
    #[test]
    fn a_turn_tried_while_refusing_waits_again_and_the_kinds_are_kept() {
        let mut refusing = ModelRefusing::default();
        refusing.observe(&failed(1, Fault::QuotaExceeded), 0, false);
        refusing.tried(TRY_AGAIN_EVERY_US);
        assert!(refusing.holds(TRY_AGAIN_EVERY_US + 1));
        refusing.observe(
            &failed(2, Fault::QuotaExceeded),
            TRY_AGAIN_EVERY_US + 2,
            false,
        );
        refusing.observe(
            &failed(3, Fault::ServerUnavailable),
            TRY_AGAIN_EVERY_US + 3,
            false,
        );
        assert_eq!(
            refusing.health(3 * TRY_AGAIN_EVERY_US),
            Some(("quota,server_error".to_owned(), 3 * TRY_AGAIN_EVERY_US))
        );
        // Not refusing: a turn sent changes nothing.
        let mut well = ModelRefusing::default();
        well.tried(5);
        assert!(!well.holds(6) && well.health(6).is_none());
    }
}
