//! Local hand cancellation. The injected clock brackets the actual speaker
//! authority send; there is no provider or persistence dependency on this path.
use super::*;

pub(super) struct Cancellation {
    pub snapshot: Snapshot,
    pub reply: Option<Reply>,
    pub speaker_told_at_us: u64,
    pub cancelled_turn: Option<u64>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn cancel(
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    speaker: &mut Channel,
    pending: &mut Option<PendingSpeaker>,
    provider_queue: &mut VecDeque<ProviderInput>,
    admission: &mut InputAdmission,
    detector: &mut TurnDetector,
    muted: bool,
    observed_at_us: u64,
    mut clock: impl FnMut() -> u64,
) -> Result<Cancellation> {
    let at = MonoTime::from_micros(observed_at_us);
    let active = owner.snapshot(at)?.owner();
    if let Some(active) = active {
        owner.cancel_turn(at, active)?;
    }
    if muted {
        owner.set_microphone_permission(at, Permission::Denied)?;
        owner.set_camera_permission(at, Permission::Denied)?;
    }
    let snapshot = owner.snapshot(at)?;
    // Discard local PCM and commands before telling the speaker to drop its
    // held audio. Retained reply metadata is only for later receipts.
    let mut cancelled = reply.take();
    if let Some(current) = &mut cancelled {
        current.pcm.clear();
    }
    *pending = None;
    provider_queue.clear();
    speaker.send(Control::Authority { snapshot })?;
    let speaker_told_at_us = clock();
    if speaker_told_at_us < observed_at_us {
        return Err(io::Error::other("hand cancellation clock moved backwards").into());
    }
    admission.abandon_until_end(speaker_told_at_us)?;
    detector.abandon_input(speaker_told_at_us);
    let cancelled_turn = active
        .map(|owner| owner.turn())
        .or_else(|| cancelled.as_ref().map(|reply| reply.owner.turn()));
    Ok(Cancellation {
        snapshot,
        reply: cancelled,
        speaker_told_at_us,
        cancelled_turn,
    })
}

/// Output can already be queued on a separate IPC socket when Stop arrives.
/// Consume its flow credit, but never let an old transcript schedule a look or
/// let an old completion schedule another request. Connection events still run.
pub(super) fn cancelled_output(event: &ProviderOutput, through: Option<u64>) -> bool {
    let request = match event {
        ProviderOutput::Started { request, .. }
        | ProviderOutput::Audio { request, .. }
        | ProviderOutput::GenerationComplete { request }
        | ProviderOutput::Interrupted { request }
        | ProviderOutput::TurnComplete { request, .. }
        | ProviderOutput::HoldingAnswer { request, .. }
        | ProviderOutput::Unclear { request, .. }
        | ProviderOutput::LookAsked { request, .. }
        | ProviderOutput::LookNoCall { request, .. }
        | ProviderOutput::RequestedAudio { request, .. }
        | ProviderOutput::NewsAnswered { request, .. }
        | ProviderOutput::SleepAsked { request }
        | ProviderOutput::StepCallHeard { request }
        | ProviderOutput::OwnWordsSaid { request }
        | ProviderOutput::FaceConsentCandidate { request, .. }
        | ProviderOutput::WordsEnd { request, .. }
        | ProviderOutput::TurnFailed { request, .. }
        | ProviderOutput::ReminderDeferred { request, .. }
        | ProviderOutput::InputDiscarded { request, .. } => Some(*request),
        ProviderOutput::Transcript { request, .. }
        | ProviderOutput::OwnershipAssumed { request, .. } => *request,
        // Times only, for the records: never anything to schedule.
        ProviderOutput::Ready { .. }
        | ProviderOutput::Ended { .. }
        | ProviderOutput::Stages { .. } => None,
    };
    through
        .zip(request)
        .is_some_and(|(through, request)| request <= through)
}
