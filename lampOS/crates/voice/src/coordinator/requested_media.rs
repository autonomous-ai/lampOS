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

    pub(super) fn accept_requested_audio(
        &mut self,
        service: &crate::requested_audio::Service,
        worker: BootId,
        snapshot: Snapshot,
        offer: RequestedAudioOffer,
    ) -> bool {
        if !self.media.spoken
            || self.media.offered
            || !self.cloud.started
            || self.provider_idle
            || self.notice.origin() != SpeechOrigin::Provider
            || self.local_only.is_some()
            || self.reminder.is_some()
            || self.say.is_some()
            || snapshot.owner() != Some(self.owner)
            || snapshot.microphone_permission() != Permission::Allowed
            || offer.check().is_err()
            || offer.context.worker_boot != boot_text(worker)
            || offer.context.request != self.owner.turn()
            || offer.context.provider_session != self.cloud.session
            || offer.context.privacy_generation != snapshot.microphone_generation()
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

    pub(super) fn requested_audio_invalid(
        &self,
        worker: BootId,
        snapshot: Snapshot,
        at_us: u64,
    ) -> bool {
        self.media.ticket.as_ref().is_some_and(|ticket| {
            !ticket.live(at_us)
                || self.media.binding.as_ref().is_none_or(|binding| {
                    binding.worker_boot != boot_text(worker)
                        || binding.provider_session != self.cloud.session
                        || binding.request != self.owner.turn()
                        || snapshot.owner() != Some(self.owner)
                        || snapshot.microphone_permission() != Permission::Allowed
                        || binding.privacy_generation != snapshot.microphone_generation()
                })
        })
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
        if self.notice.origin() == SpeechOrigin::RequestedAudio
            && self.notice.retired()
            && self.playback_gaps == 0
            && let Some(ticket) = self.media.ticket.as_mut()
            && ticket.may_play(at_us)
        {
            ticket.completed();
        }
    }
}
