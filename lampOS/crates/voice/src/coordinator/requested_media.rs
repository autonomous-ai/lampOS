//! A requested recording continues its spoken owner after real confirmation
//! retirement. File and socket work belongs to `requested_audio::Service`.
use super::*;
use lamp_msgs::{RequestedAudioContext, RequestedAudioOffer};

#[derive(Default)]
pub(super) struct RequestedMedia {
    /// Set only by microphone admission, never Written/Reminder/local turns.
    spoken: bool,
    offered: bool,
    binding: Option<RequestedAudioContext>,
    pub(super) confirmation_retired_at_us: Option<u64>,
    ticket: Option<crate::requested_audio::Ticket>,
    /// Recorded only after final retirement while the ticket still permits
    /// playback. Later lease expiry cannot erase an already delivered turn.
    completed: bool,
}

impl RequestedMedia {
    pub(super) fn spoken() -> Self {
        Self {
            spoken: true,
            ..Self::default()
        }
    }
}

impl Reply {
    #[cfg(test)]
    pub(super) fn wait_requested_audio_ready(&self, timeout: Duration) -> bool {
        self.media
            .ticket
            .as_ref()
            .is_some_and(|ticket| ticket.wait_ready(timeout))
    }

    /// Why this turn would refuse `offer`, the first of its conditions that
    /// fails, or `None` when it would take it: what the trace and the
    /// journal say for a refused offer. On lamp-4ace on 24 Oct a first
    /// "play me something to relax" was refused with nothing to say which
    /// (#2793).
    pub(super) fn requested_audio_refusal(
        &self,
        worker: BootId,
        snapshot: Snapshot,
        offer: &RequestedAudioOffer,
    ) -> Option<&'static str> {
        let refusals = [
            (!self.media.spoken, "not a spoken turn"),
            (self.media.offered, "this turn already had its one offer"),
            (
                !self.cloud.started,
                "the turn's provider speech has not started",
            ),
            (self.provider_idle, "the provider is idle"),
            (
                self.notice.origin() != SpeechOrigin::Provider,
                "the turn's speech is not the provider's",
            ),
            (self.local_only.is_some(), "a local-only turn"),
            (self.reminder.is_some(), "a reminder turn"),
            (self.say.is_some(), "a Say cue holds the speaker"),
            (
                snapshot.owner() != Some(self.owner),
                "the speaker belongs to another owner",
            ),
            (
                snapshot.microphone_permission() != Permission::Allowed,
                "the microphone is not allowed",
            ),
            (offer.check().is_err(), "the offer does not check"),
            (
                offer.context.worker_boot != boot_text(worker),
                "the offer is from another worker boot",
            ),
            (
                offer.context.request != self.owner.turn(),
                "the offer is for another request",
            ),
            (
                offer.context.provider_session != self.cloud.session,
                "the offer is for another provider session",
            ),
            (
                offer.context.privacy_generation != snapshot.microphone_generation(),
                "the offer is from another privacy generation",
            ),
        ];
        refusals
            .into_iter()
            .find_map(|(refused, why)| refused.then_some(why))
    }

    pub(super) fn accept_requested_audio(
        &mut self,
        service: &crate::requested_audio::Service,
        worker: BootId,
        snapshot: Snapshot,
        offer: RequestedAudioOffer,
    ) -> bool {
        if self
            .requested_audio_refusal(worker, snapshot, &offer)
            .is_some()
        {
            return false;
        }
        // Even unavailable preparation consumes this turn's single attempt.
        self.media.offered = true;
        self.media.binding = Some(offer.context.clone());
        self.media.confirmation_retired_at_us = None;
        self.media.ticket = service.offer(offer);
        if self.media.ticket.is_some() {
            self.cloud.movable = false;
        }
        self.media.ticket.is_some()
    }

    pub(super) fn requested_audio_bound(&self) -> bool {
        self.media.ticket.is_some()
    }

    pub(super) fn requested_audio_waiting(&self) -> bool {
        self.media.ticket.is_some() && self.notice.origin() == SpeechOrigin::Provider
    }

    #[cfg(test)]
    pub(super) fn requested_audio_invalid(
        &self,
        worker: BootId,
        snapshot: Snapshot,
        at_us: u64,
    ) -> bool {
        self.requested_audio_invalidity(worker, snapshot, at_us)
            .is_some()
    }

    /// Why a bound recording is no longer valid, or `None` while it is: what
    /// the journal says before the turn is revoked (#2793).
    pub(super) fn requested_audio_invalidity(
        &self,
        worker: BootId,
        snapshot: Snapshot,
        at_us: u64,
    ) -> Option<&'static str> {
        let ticket = self.media.ticket.as_ref()?;
        if !ticket.live(at_us) {
            return Some("its recording is no longer live");
        }
        let Some(binding) = self.media.binding.as_ref() else {
            return Some("it has no binding");
        };
        [
            (
                binding.worker_boot != boot_text(worker),
                "another worker boot",
            ),
            (
                binding.provider_session != self.cloud.session,
                "another provider session",
            ),
            (binding.request != self.owner.turn(), "another request"),
            (
                snapshot.owner() != Some(self.owner),
                "the speaker belongs to another owner",
            ),
            (
                snapshot.microphone_permission() != Permission::Allowed,
                "the microphone is not allowed",
            ),
            (
                binding.privacy_generation != snapshot.microphone_generation(),
                "another privacy generation",
            ),
        ]
        .into_iter()
        .find_map(|(invalid, why)| invalid.then_some(why))
    }

    /// Return true only when a new recording is selected. A completed model
    /// generation or accepted final chunk cannot substitute for retirement.
    pub(super) fn prepare_requested_audio(
        &mut self,
        pending: &Option<PendingSpeaker>,
        at_us: u64,
    ) -> bool {
        if !self.requested_audio_waiting() || !self.output_settled(pending) {
            return false;
        }
        if !matches!(self.cloud.outcome, Some(RequestOutcome::Complete))
            || !self.had_audio
            || self.playback_gaps != 0
            || self.media.confirmation_retired_at_us.is_none()
        {
            // A failed, empty or gapped confirmation ends the offer. Dropping
            // the ticket releases the grant without I/O on this loop.
            self.media.ticket = None;
            return false;
        }
        let Some(samples) = self
            .media
            .ticket
            .as_mut()
            .and_then(|ticket| ticket.take_ready(at_us))
        else {
            return false;
        };
        self.notice = NoticeDelivery::sound(SpeechOrigin::RequestedAudio, samples);
        self.voice_level = 0;
        true
    }

    pub(super) fn requested_audio_permit_us(&mut self, at_us: u64) -> Option<u64> {
        let ticket = self.media.ticket.as_mut()?;
        if !ticket.started(at_us) {
            return None;
        }
        ticket
            .output_lease_us(at_us)
            .map(|lease| lease.min(SPEAKER_PERMIT_LEASE_US))
    }

    pub(super) fn complete_requested_audio(&mut self, at_us: u64) {
        if self.requested_audio_retired_whole()
            && self.media.confirmation_retired_at_us.is_some()
            && matches!(self.cloud.outcome, Some(RequestOutcome::Complete))
            && self.had_audio
            && self.playback_gaps == 0
            && let Some(ticket) = self.media.ticket.as_mut()
            && ticket.may_play(at_us)
        {
            self.media.completed = ticket.completed();
        }
    }

    /// Provider gaps remain on Reply; a local recording's gaps are retained
    /// by NoticeDelivery. Both must be clean for the combined delivery.
    fn requested_audio_retired_whole(&self) -> bool {
        matches!(
            self.notice,
            NoticeDelivery::Selected {
                origin: SpeechOrigin::RequestedAudio,
                retired_at_us: Some(_),
                gaps: 0,
                ..
            }
        )
    }

    /// News spoken in the confirmation is acknowledged only after the entire
    /// handoff completes. A retired confirmation alone cannot acknowledge a
    /// cancelled, gapped or expired recording. This does not widen Say or
    /// reminder delivery, which still require the provider's own origin.
    pub(super) fn requested_audio_heard_whole(
        &self,
        pending: &Option<PendingSpeaker>,
        snapshot: Snapshot,
    ) -> bool {
        self.media.completed
            && self.requested_audio_retired_whole()
            && self.media.confirmation_retired_at_us.is_some()
            && self.settled(pending)
            && snapshot.owner().is_none()
            && snapshot.boot() == self.owner.boot()
            && Some(snapshot.generation()) == self.owner.generation().checked_add(1)
            && snapshot.microphone_permission() == Permission::Allowed
            && self.media.binding.as_ref().is_some_and(|binding| {
                binding.request == self.owner.turn()
                    && binding.provider_session == self.cloud.session
                    && binding.privacy_generation == snapshot.microphone_generation()
            })
            && matches!(self.cloud.outcome, Some(RequestOutcome::Complete))
            && self.had_audio
            && self.playback_gaps == 0
    }
}
