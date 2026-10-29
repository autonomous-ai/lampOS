//! The model service refusing every turn (#4265, `LAMP_SECOND_REFUSED_HOLD`).
//!
//! On 29 Oct the relay's chat answered every round with 402 in under a
//! second: the lamp said nothing to anyone, and setup failed its steps. With
//! the switch, a turn that fails because a service refused it or could not
//! be reached starts a refusal: the turns after it are listened to and never
//! sent, as while only the voice service is down (#830), so the lamp says
//! the recorded `voice_service_unreachable` line once and breathes its calm
//! offline light; one turn is let through every [`TRY_AGAIN_EVERY_US`] to
//! find the service back, and the first answered turn ends the refusal. A
//! turn that is only slow is no refusal: it fails as before. The fault
//! carries no HTTP status, so a 400 is counted with the other 4xx
//! (`ServerRejected`): a request every round gets wrong leaves the lamp as
//! unable to answer as a refusal does.
use super::*;
use crate::provider_worker::Fault;

/// The switch, exactly `on`. Off unless set.
pub(super) const ENVIRONMENT: &str = "LAMP_SECOND_REFUSED_HOLD";

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
}

impl ModelRefusing {
    /// Takes a turn's outcome: a refusal starts or goes on, an answered
    /// turn ends it. True when this ended one, so its outage starts afresh.
    pub(super) fn observe(&mut self, event: &ProviderOutput, now_us: u64) -> bool {
        match event {
            ProviderOutput::TurnFailed {
                request, reason, ..
            } => {
                let Some(kind) = kind(*reason) else {
                    return false;
                };
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
            ProviderOutput::GenerationComplete { request } => {
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
    /// was sent less than [`TRY_AGAIN_EVERY_US`] ago.
    pub(super) fn holds(&self, now_us: u64) -> bool {
        self.refusing() && now_us.saturating_sub(self.tried_at_us) < TRY_AGAIN_EVERY_US
    }

    /// A turn sent while refusing: the next waits its time again.
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
    use crate::provider_worker::FailedStage;

    fn failed(request: u64, reason: Fault) -> ProviderOutput {
        ProviderOutput::TurnFailed {
            request,
            stage: FailedStage::AwaitingResponse,
            reason,
        }
    }

    /// 29 Oct's 402s, a 503, and a refused connection, each a refusal
    /// from the stage the second route fails its turn at.
    #[test]
    fn a_402_a_503_and_a_refused_connection_are_refusals_and_an_answer_ends_them() {
        use crate::second_provider::{fault, net::Error};
        for error in [
            Error::Status(402),
            Error::Status(503),
            Error::Transport("connect"),
            Error::Timeout("connect"),
        ] {
            let mut refusing = ModelRefusing::default();
            assert!(!refusing.observe(&failed(1, fault(&error)), 10_000_000));
            assert!(refusing.refusing(), "{error:?}");
            assert!(refusing.holds(10_000_000 + TRY_AGAIN_EVERY_US - 1));
            assert!(!refusing.holds(10_000_000 + TRY_AGAIN_EVERY_US));
            // An idle TurnComplete is no answer.
            refusing.observe(
                &ProviderOutput::TurnComplete {
                    request: 2,
                    idle: true,
                },
                20_000_000,
            );
            assert!(refusing.refusing());
            assert!(refusing.observe(
                &ProviderOutput::GenerationComplete { request: 3 },
                30_000_000
            ));
            assert!(!refusing.refusing() && !refusing.holds(30_000_000));
        }
    }

    /// Slow is not refused (#4089's): a response or a read that timed out,
    /// a stall, or a fault inside the lamp holds nothing.
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
            refusing.observe(&failed(1, reason), 0);
            assert!(!refusing.refusing(), "{reason:?}");
        }
    }

    /// One turn sent while refusing, and failing again, waits its time
    /// afresh; the kinds are kept for Health, each said once.
    #[test]
    fn a_turn_tried_while_refusing_waits_again_and_the_kinds_are_kept() {
        let mut refusing = ModelRefusing::default();
        refusing.observe(&failed(1, Fault::QuotaExceeded), 0);
        refusing.tried(TRY_AGAIN_EVERY_US);
        assert!(refusing.holds(TRY_AGAIN_EVERY_US + 1));
        refusing.observe(&failed(2, Fault::QuotaExceeded), TRY_AGAIN_EVERY_US + 2);
        refusing.observe(&failed(3, Fault::ServerUnavailable), TRY_AGAIN_EVERY_US + 3);
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
