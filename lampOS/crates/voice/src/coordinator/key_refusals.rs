//! A chat service that refuses the lamp's key turn by turn (#2231).
//!
//! The second route asks its chat service once per turn, so a 401/403
//! there fails the turn and leaves the link up: Health would still say the
//! voice is ready. The lead's rule (#1711, 21 Oct): Health says the chat
//! service refused this lamp's key after two refusals in a row, or one
//! still refused [`STILL_REFUSED_US`] later, and stops on the first
//! answered turn. A single refusal stays in the journal only, so one blip
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
                    "lamp-voice: request {request}: the chat service refused this lamp's key ({} in a row)",
                    self.in_a_row
                );
            }
            ProviderOutput::TurnComplete { .. } => *self = Self::default(),
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
        refusals.observe(
            &ProviderOutput::TurnComplete {
                request: 3,
                idle: true,
            },
            3_000_000,
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
