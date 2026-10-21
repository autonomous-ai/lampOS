//! A voice service that refuses the lamp's key turn by turn (#2231).
//!
//! The second route asks its services (speech to text, the chat, speech)
//! once per turn, so a 401/403 from any of them fails the turn and leaves
//! the link up: Health would still say the voice is ready. Which service it
//! was is not told (`second_provider::Shared::fail` gives one fault for all
//! three), so it is said as a voice service. The lead's rule (#1711, 21
//! Oct): Health says a voice service refused this lamp's key after two
//! refusals in a row, or once the first refusal is [`STILL_REFUSED_US`] old
//! with no answered turn since (its age, not a second try: a lamp nobody
//! spoke to again still has the bad key), and stops on the first answered
//! turn. A single refusal stays in the journal only, so one blip
//! does not change what the app shows.
use super::*;
use crate::provider_worker::Fault;

/// One refusal with no answered turn after it for this long is said.
pub(super) const STILL_REFUSED_US: u64 = 120_000_000;

#[derive(Default)]
pub(super) struct KeyRefusals {
    /// Refused turns since the last answered one, and when the first was.
    in_a_row: u32,
    first_at_us: Option<u64>,
}

impl KeyRefusals {
    pub(super) fn observe(&mut self, event: &ProviderOutput, now_us: u64) {
        match event {
            ProviderOutput::TurnFailed {
                request,
                reason: Fault::Authentication,
                ..
            } => {
                self.in_a_row = self.in_a_row.saturating_add(1);
                self.first_at_us.get_or_insert(now_us);
                eprintln!(
                    "lamp-voice: request {request}: a voice service refused this lamp's key ({} in a row)",
                    self.in_a_row
                );
            }
            // Answered: the model's generation finished. An idle
            // `TurnComplete` alone is no answer: a line held as unclear or
            // not for the lamp, onboarding's count, a held or given-up turn
            // never asked the services (the reviewer-lead on #2561).
            ProviderOutput::GenerationComplete { .. } => *self = Self::default(),
            _ => {}
        }
    }

    /// Whether Health says the key is refused now.
    pub(super) fn said(&self, now_us: u64) -> bool {
        self.in_a_row >= 2
            || self
                .first_at_us
                .is_some_and(|at| now_us.saturating_sub(at) >= STILL_REFUSED_US)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_worker::FailedStage;

    fn refused(request: u64) -> ProviderOutput {
        ProviderOutput::TurnFailed {
            request,
            stage: FailedStage::AwaitingResponse,
            reason: Fault::Authentication,
        }
    }

    #[test]
    fn two_refusals_in_a_row_are_said_and_one_answered_turn_clears_them() {
        let mut refusals = KeyRefusals::default();
        refusals.observe(&refused(1), 0);
        assert!(!refusals.said(1_000_000), "one refusal is the journal's");
        refusals.observe(&refused(2), 2_000_000);
        assert!(refusals.said(2_000_000));
        // A turn that ended without asking the services is no answer.
        refusals.observe(
            &ProviderOutput::TurnComplete {
                request: 3,
                idle: true,
            },
            3_000_000,
        );
        assert!(refusals.said(3_000_000));
        refusals.observe(
            &ProviderOutput::GenerationComplete { request: 4 },
            4_000_000,
        );
        assert!(!refusals.said(STILL_REFUSED_US * 2));
    }

    #[test]
    fn one_refusal_still_unanswered_after_120_s_is_said() {
        let mut refusals = KeyRefusals::default();
        refusals.observe(&refused(1), 5_000_000);
        assert!(!refusals.said(5_000_000 + STILL_REFUSED_US - 1));
        assert!(refusals.said(5_000_000 + STILL_REFUSED_US));
    }

    /// The first builder on #2532: speech to text's 401 is a refused key
    /// too, from the stage its turn fails at.
    #[test]
    fn a_speech_to_text_401_counts_as_a_refused_key() {
        let reason =
            crate::second_provider::fault(&crate::second_provider::net::Error::Status(401));
        let mut refusals = KeyRefusals::default();
        for request in [1, 2] {
            refusals.observe(
                &ProviderOutput::TurnFailed {
                    request,
                    stage: FailedStage::InputOpen,
                    reason,
                },
                0,
            );
        }
        assert!(refusals.said(0));
    }

    #[test]
    fn other_failures_are_not_a_refused_key() {
        let mut refusals = KeyRefusals::default();
        for (request, reason) in [(1, Fault::Transport), (2, Fault::QuotaExceeded)] {
            refusals.observe(
                &ProviderOutput::TurnFailed {
                    request,
                    stage: FailedStage::AwaitingResponse,
                    reason,
                },
                0,
            );
        }
        assert!(!refusals.said(STILL_REFUSED_US * 2));
    }
}
