//! Same-request audio discard. Existing ownership remains the only authority.
use super::*;
use crate::speech_cut::{ACK_TIMEOUT_US, ProviderCut};

#[derive(Clone, Copy)]
struct Fence {
    provider: BootId,
    cut: ProviderCut,
    owner: TurnOwner,
    speaker_through: u64,
}

#[derive(Default)]
pub(super) struct State {
    fence: Option<Fence>,
    waiting_since_us: Option<u64>,
}

impl State {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn admit(
        &mut self,
        provider: BootId,
        cut: ProviderCut,
        reply: &mut Option<Reply>,
        speaker_sequence: u64,
        send: impl FnOnce(Control) -> io::Result<()>,
        trace: &mut SessionState,
    ) -> Result<()> {
        if cut.request == 0 || cut.admitted_at_us > monotonic_us() {
            return Err(io::Error::other("invalid provider speech cut").into());
        }
        let Some(current) = reply.as_mut().filter(|r| {
            r.owner.turn() == cut.request && r.notice.origin() == SpeechOrigin::Provider
        }) else {
            // Retired requests have no playback left to revoke. Do not let their
            // cut touch a new owner; their worker's own retirement releases it.
            return Ok(());
        };
        let newer = self.fence.is_none_or(|f| {
            f.provider != provider || f.cut.request != cut.request || cut.through > f.cut.through
        });
        if newer {
            if self
                .fence
                .is_some_and(|f| f.provider != provider || f.owner != current.owner)
            {
                self.waiting_since_us = None;
            }
            current.pcm.clear();
            current.generation_done = false;
            current.provider_idle = false;
            current.final_chunk = None;
            current.playback_gaps = current.playback_gaps.saturating_add(1);
            // A discarded story must never count as a mostly-heard reminder.
            if let Some(reminder) = &mut current.reminder {
                reminder.invalidate();
            }
            let fence = Fence {
                provider,
                cut,
                owner: current.owner,
                speaker_through: speaker_sequence,
            };
            send(Control::CutSpeech {
                owner: fence.owner,
                through: fence.speaker_through,
            })?;
            self.fence = Some(fence);
            self.waiting_since_us.get_or_insert(monotonic_us());
            record(
                trace,
                json!({"kind":"provider_speech_cut", "cut":cut,
                "owner":fence.owner,"speaker_through":speaker_sequence,"at_us":monotonic_us()}),
            )?;
        }
        Ok(())
    }

    pub(super) fn retire(&mut self, provider: BootId, reply: &Option<Reply>) {
        if self.fence.is_some_and(|f| {
            f.provider != provider
                || reply
                    .as_ref()
                    .is_none_or(|current| current.owner != f.owner)
        }) {
            // Ordinary ownership cancellation already fences these samples.
            // Keep the old receipt floor, but never carry its deadline forward.
            self.waiting_since_us = None;
        }
    }

    pub(super) fn waiting(&self) -> bool {
        self.waiting_since_us.is_some()
    }

    fn check_deadline(&self) -> Result<()> {
        self.check_deadline_at(monotonic_us())
    }

    fn check_deadline_at(&self, at_us: u64) -> Result<()> {
        if self
            .waiting_since_us
            .is_some_and(|at| at_us.saturating_sub(at) >= ACK_TIMEOUT_US)
        {
            return Err(io::Error::other("speaker speech cut acknowledgement expired").into());
        }
        Ok(())
    }

    pub(super) fn discards(&self, provider: BootId, event: &ProviderOutput) -> bool {
        matches!(event, ProviderOutput::Audio { request, sequence, .. }
            if self.fence.is_some_and(|f| f.provider == provider && f.cut.request == *request && *sequence <= f.cut.through))
    }

    fn old_sequence(&self, turn: u64, sequence: u64) -> bool {
        self.fence
            .is_some_and(|f| f.owner.turn() == turn && sequence <= f.speaker_through)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn speaker_event(
        &mut self,
        event: &WorkerEvent,
        io: &mut impl SpeakerControlIo,
        owner: &mut Controller,
        reply: &mut Option<Reply>,
        pending: &mut Option<PendingSpeaker>,
        mask: &mut PlaybackMask,
        snapshot: &mut Snapshot,
        last_publish: &mut u64,
        trace: &mut SessionState,
    ) -> Result<bool> {
        if self.fence.is_some_and(|f| {
            reply
                .as_ref()
                .is_none_or(|current| current.owner != f.owner)
        }) {
            self.waiting_since_us = None;
        }
        match event {
            WorkerEvent::SpeechCutApplied {
                owner: cut_owner,
                through,
                applied_at_us,
            } => {
                let Some(fence) = self
                    .fence
                    .filter(|f| f.owner == *cut_owner && *through == f.speaker_through)
                else {
                    return Ok(true);
                };
                if !self.waiting() {
                    return Ok(true);
                }
                self.check_deadline()?;
                if *applied_at_us == 0 || *applied_at_us > monotonic_us() {
                    return Err(io::Error::other("invalid speech cut acknowledgement time").into());
                }
                if pending
                    .as_ref()
                    .is_some_and(|p| p.permit.owner() == *cut_owner && p.sequence <= *through)
                {
                    let old = pending.take().expect("checked pending chunk");
                    mask.rejected(old.sequence, monotonic_us())?;
                    trace.receipts.rejected(old.sequence);
                }
                if let Some(current) = reply.as_mut().filter(|r| r.owner == *cut_owner) {
                    if let Some(token) = current.playback.take() {
                        owner.playback_ended(now(), token)?;
                    }
                    current.final_chunk = None;
                    current.generation_done = false;
                    current.last_provider_audio_at_us = None;
                    if let Some(reminder) = &mut current.reminder {
                        reminder.invalidate();
                    }
                    *snapshot = owner.snapshot(now())?;
                    io.publish_speaker_snapshot(*snapshot)?;
                    *last_publish = monotonic_us();
                }
                mask.discarded(*cut_owner, *applied_at_us, monotonic_us())?;
                io.speech_cut_applied(fence.provider, fence.cut)?;
                self.waiting_since_us = None;
                record(
                    trace,
                    json!({"kind":"speech_cut_applied", "owner":cut_owner,
                    "provider_through":fence.cut.through,"speaker_through":through,
                    "admitted_at_us":fence.cut.admitted_at_us,"at_us":applied_at_us,
                    "already_applied":*applied_at_us < fence.cut.admitted_at_us,
                    "admission_to_physical_ack_us":applied_at_us.saturating_sub(fence.cut.admitted_at_us),
                    "boundary":"speaker physical discard acknowledgement; acoustic stop unmeasured"}),
                )?;
                Ok(true)
            }
            // Pre-ACK accepted writes still have their original pending permit.
            // After ACK an old receipt cannot attach to fresh same-request PCM.
            WorkerEvent::Playback { turn, sequence, .. }
                if !self.waiting() && self.old_sequence(*turn, *sequence) =>
            {
                Ok(true)
            }
            WorkerEvent::SpeechRetired {
                owner,
                sequence,
                final_sample,
                ..
            } if self.old_sequence(owner.turn(), *sequence) => {
                trace.receipts.drained(*owner, *final_sample);
                Ok(true)
            }
            WorkerEvent::SpeechDrained {
                owner, sequence, ..
            } if !self.waiting() && self.old_sequence(owner.turn(), *sequence) => Ok(true),
            WorkerEvent::PlaybackRejected { turn, sequence, .. }
                if self.old_sequence(*turn, *sequence) =>
            {
                if pending.as_ref().is_some_and(|p| p.sequence == *sequence) {
                    mask.rejected(*sequence, monotonic_us())?;
                    trace.receipts.rejected(*sequence);
                    *pending = None;
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

pub(super) fn send_speaker_cut(
    channel: &mut Channel,
    authority: Snapshot,
    cut: Control,
) -> io::Result<()> {
    channel.send(Control::Authority {
        snapshot: authority,
    })?;
    channel.send(cut)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn provider_control(
    rig: &mut Rig,
    cloud: &mut CloudAvailability,
    remaining: &mut usize,
    trace: &mut SessionState,
    state: &mut State,
    reply: &mut Option<Reply>,
    speaker_sequence: u64,
    owner: &mut Controller,
) -> Result<bool> {
    state.retire(rig.provider.boot, reply);
    state.check_deadline()?;
    let mut cut = None;
    let empty = service_provider_control_inner(
        &mut rig.provider.channels.control,
        cloud,
        remaining,
        trace,
        Some(&mut cut),
    )?;
    if let Some(cut) = cut {
        let authority = owner.snapshot(now())?;
        if authority.owner().map(|owner| owner.turn()) != Some(cut.request) {
            return Ok(empty);
        }
        // A first forget can precede both audio and the periodic renewal.
        // Install real current authority on the same ordered control channel;
        // the cut never creates authority or depends on a data-plane snapshot.
        state.admit(
            rig.provider.boot,
            cut,
            reply,
            speaker_sequence,
            |control| send_speaker_cut(&mut rig.speaker.channels.control, authority, control),
            trace,
        )?;
    }
    Ok(empty)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_physical_ack_fails_closed_at_the_roundtrip_deadline() {
        let state = State {
            fence: None,
            waiting_since_us: Some(1_000_000),
        };
        assert!(
            state
                .check_deadline_at(1_000_000 + ACK_TIMEOUT_US - 1)
                .is_ok()
        );
        assert!(state.check_deadline_at(1_000_000 + ACK_TIMEOUT_US).is_err());
        assert!(state.waiting());
    }
}
