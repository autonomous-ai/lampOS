//! Submission availability is independent of capture and output authority.
//! The surrounding Channel authenticates the worker incarnation and envelope;
//! this register validates source ordering, including coalesced observations.
use crate::provider_link::{ProviderLink, ProviderLinkState};
use crate::provider_worker::{FailedStage, Fault, ProviderInput};
use serde::Serialize;
use std::io;

#[derive(Default, Debug)]
pub(super) struct CloudAvailability {
    latest: Option<ProviderLink>,
    output_ended: bool,
    pub(super) conversation: crate::provider_state::conversation::Mirror,
    names_receiver: crate::provider_state::names::NamesReceiver,
    names_revision: Option<u64>,
    names: crate::provider_state::names::ValidatedNames,
}

impl CloudAvailability {
    /// State belongs to this worker incarnation, including while it is standby.
    pub(super) fn observe_state(
        &mut self,
        event: &crate::provider_worker::ProviderOutput,
        now_us: u64,
    ) -> io::Result<()> {
        use crate::provider_worker::ProviderOutput;
        if self.output_ended {
            return Err(invalid("worker state followed ordered Ended"));
        }
        match event {
            ProviderOutput::ConversationState { snapshot } => {
                self.conversation.observe(*snapshot, now_us)?
            }
            ProviderOutput::MachineNames { chunk } => {
                if let Some(names) = self.names_receiver.receive(chunk.clone())? {
                    self.names_revision = Some(chunk.revision);
                    self.names = names;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn observe(&mut self, link: ProviderLink, now_us: u64) -> io::Result<()> {
        link.validate(now_us)?;
        if let Some(previous) = self.latest {
            if link.revision <= previous.revision
                || link.session < previous.session
                || link.observed_at_us < previous.observed_at_us
                || matches!(previous.state, ProviderLinkState::Terminal { .. })
            {
                return Err(invalid("provider link source history regressed or ended"));
            }
            if link.session == previous.session
                && !matches!(previous.state, ProviderLinkState::Ready { .. })
                && matches!(link.state, ProviderLinkState::Ready { .. })
            {
                return Err(invalid("ready resurrected an unavailable provider session"));
            }
        }
        // Control and ordered output are separate sockets. A legitimate Ready
        // sent before terminal shutdown may be observed after Ended. Retain its
        // validated source history, but the independent output_ended latch can
        // never be reopened by it.
        self.latest = Some(link);
        Ok(())
    }

    pub(super) fn ready_session(&self) -> Option<u64> {
        self.latest
            .filter(|link| {
                !self.output_ended && matches!(link.state, ProviderLinkState::Ready { .. })
            })
            .map(|link| link.session)
    }

    pub(super) fn available(&self) -> bool {
        self.ready_session().is_some()
    }

    /// An ordered Ended closes submissions even if a priority terminal status
    /// has not yet fit in its socket. It never retires an admitted request or
    /// discards already-received PCM. Local refusals may still follow.
    pub(super) fn end_output(&mut self) -> io::Result<()> {
        if self.output_ended {
            return Err(invalid("duplicate provider output end"));
        }
        self.output_ended = true;
        Ok(())
    }

    pub(super) fn output_ended(&self) -> bool {
        self.output_ended
    }

    /// The link has ended for good (`second.rs` does not switch away from
    /// a route whose end stops the service).
    pub(super) fn terminal(&self) -> bool {
        matches!(
            self.latest.map(|link| link.state),
            Some(ProviderLinkState::Terminal { .. })
        )
    }

    /// The link ended on a refusal that asking again at once does not
    /// mend: the provider's quota or its rate limit, which pass with time,
    /// or the lamp's key, which needs a person (#2231: a lamp moved from V1
    /// kept V1's key, and systemd restarted the voice every five seconds on
    /// it). Asking again at once only adds load.
    pub(super) fn refused_for_now(&self) -> Option<Fault> {
        match self.latest.map(|link| link.state)? {
            ProviderLinkState::Terminal {
                fault: fault @ (Fault::QuotaExceeded | Fault::RateLimited | Fault::Authentication),
                ..
            } => Some(fault),
            _ => None,
        }
    }

    /// Why the link is down, when the provider refused it rather than
    /// could not be reached: the lamp's key, or its quota or rate limit
    /// (#2231). Said in the voice's health as it is, not as a link down.
    pub(super) fn refusal(&self) -> Option<Fault> {
        match self.latest.map(|link| link.state)? {
            ProviderLinkState::Terminal { fault, .. }
            | ProviderLinkState::Unavailable { reason: fault, .. } => matches!(
                fault,
                Fault::Authentication | Fault::QuotaExceeded | Fault::RateLimited
            )
            .then_some(fault),
            _ => None,
        }
    }
}

/// Only the active worker may publish labels to serve's task-news consumer.
/// The boot key forces reset on replacement and reapplication on route swaps.
#[derive(Default)]
pub(super) struct ActiveNames {
    applied: Option<(crate::coordinator::BootId, Option<u64>)>,
}
impl ActiveNames {
    pub(super) fn sync(&mut self, boot: crate::coordinator::BootId, cloud: &CloudAvailability) {
        let key = (boot, cloud.names_revision);
        if self.applied != Some(key) && crate::agent_tasks::install_parent_names(&cloud.names) {
            self.applied = Some(key);
        }
    }
}

/// A coordinator request is bound to the session available at admission. A
/// source-stamped Started must confirm that exact binding before PCM is usable.
#[derive(Debug)]
pub(super) struct CloudRequest {
    pub(super) session: u64,
    pub(super) start_submitted: bool,
    pub(super) started: bool,
    pub(super) input_end_submitted: bool,
    pub(super) input_abandoned: bool,
    pub(super) outcome: Option<RequestOutcome>,
    /// A second voice route can take this input over (`second.rs`): a link
    /// change leaves it for the switch instead of abandoning it.
    pub(super) movable: bool,
    /// A continuation of this connection's answer cannot move to another
    /// provider route, even before it has been submitted or answered.
    pub(super) same_session_only: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum RequestOutcome {
    Complete,
    Failed {
        stage: FailedStage,
        reason: Fault,
        local_not_submitted: bool,
    },
}

impl CloudRequest {
    pub(super) fn new(session: u64) -> io::Result<Self> {
        if session == 0 {
            return Err(invalid("request has no provider session"));
        }
        Ok(Self {
            session,
            start_submitted: false,
            started: false,
            input_end_submitted: false,
            input_abandoned: false,
            outcome: None,
            movable: false,
            same_session_only: false,
        })
    }

    /// A local-only turn's request (#830): never submitted, failed locally
    /// from the start, so no provider input or output can ever match it.
    pub(super) fn local_only() -> Self {
        Self {
            session: 0,
            start_submitted: false,
            started: false,
            input_end_submitted: false,
            input_abandoned: true,
            outcome: Some(RequestOutcome::Failed {
                stage: FailedStage::NotDelivered,
                reason: Fault::Local,
                local_not_submitted: true,
            }),
            movable: false,
            same_session_only: false,
        }
    }

    /// Only a successful local IPC send advances submission; queued commands
    /// or a WouldBlock attempt do not prove delivery to the worker.
    pub(super) fn validate_submission(&self, command: &ProviderInput) -> io::Result<()> {
        if self.input_abandoned || self.outcome.is_some() {
            return Err(invalid("input submitted after request closed"));
        }
        match command {
            ProviderInput::Start { session, .. }
                if !self.start_submitted && *session == self.session =>
            {
                Ok(())
            }
            ProviderInput::Audio { .. }
            | ProviderInput::Called { .. }
            | ProviderInput::Paused { .. }
            | ProviderInput::PauseJoin { .. }
            | ProviderInput::StopWords { .. }
            | ProviderInput::CountOnly { .. }
            | ProviderInput::StepCall { .. }
            | ProviderInput::Onboarding { .. }
            | ProviderInput::TurnOpened { .. }
            | ProviderInput::WalkUnderWay { .. }
            | ProviderInput::WokenByName { .. }
            | ProviderInput::End { .. }
                if self.start_submitted && !self.input_end_submitted =>
            {
                Ok(())
            }
            // A whole request in writing is its own start and end.
            ProviderInput::Written { session, .. } | ProviderInput::Reminder { session, .. }
                if !self.start_submitted && *session == self.session =>
            {
                Ok(())
            }
            _ => Err(invalid("provider input boundary duplicated or reordered")),
        }
    }

    pub(super) fn submitted(&mut self, command: &ProviderInput) -> io::Result<()> {
        self.validate_submission(command)?;
        // A line handed on stops the memory's nightly pass
        // (`LAMP_MEMORY_NIGHTLY`); off, one load.
        crate::nightly::stir();
        match command {
            ProviderInput::Start { .. } => self.start_submitted = true,
            ProviderInput::End { .. } => self.input_end_submitted = true,
            ProviderInput::Audio { .. }
            | ProviderInput::Called { .. }
            | ProviderInput::Paused { .. }
            | ProviderInput::PauseJoin { .. }
            | ProviderInput::StopWords { .. }
            | ProviderInput::CountOnly { .. }
            | ProviderInput::StepCall { .. }
            | ProviderInput::Onboarding { .. }
            | ProviderInput::TurnOpened { .. }
            | ProviderInput::WalkUnderWay { .. }
            | ProviderInput::WokenByName { .. } => {}
            ProviderInput::Written { .. } | ProviderInput::Reminder { .. } => {
                self.start_submitted = true;
                self.input_end_submitted = true;
            }
        }
        Ok(())
    }

    pub(super) fn confirm_start(&mut self, session: u64) -> io::Result<()> {
        if !self.start_submitted
            || self.started
            || self.outcome.is_some()
            || session != self.session
        {
            return Err(invalid(
                "provider Started has no matching submitted request/session",
            ));
        }
        self.started = true;
        Ok(())
    }

    pub(super) fn require_output(&self) -> io::Result<()> {
        if !self.started || self.outcome.is_some() {
            return Err(invalid("provider output lacks an open matching Started"));
        }
        Ok(())
    }

    /// A link change never cancels owned output. It only makes an unfinished
    /// input non-resumable; the caller discards its queue and drains capture to
    /// the actual endpoint without resetting the detector.
    pub(super) fn abandon_if_changed(&mut self, cloud: &CloudAvailability) -> bool {
        if self.input_end_submitted
            || self.input_abandoned
            || self.outcome.is_some()
            || self.movable
            || cloud.ready_session() == Some(self.session)
        {
            return false;
        }
        self.input_abandoned = true;
        if !self.start_submitted {
            self.outcome = Some(RequestOutcome::Failed {
                stage: FailedStage::NotDelivered,
                reason: Fault::Local,
                local_not_submitted: true,
            });
        }
        true
    }

    pub(super) fn can_submit(&self, cloud: &CloudAvailability) -> bool {
        !self.input_abandoned
            && self.outcome.is_none()
            && !self.input_end_submitted
            && cloud.ready_session() == Some(self.session)
    }

    pub(super) fn complete(&mut self, idle: bool) -> io::Result<()> {
        self.require_output()?;
        if idle {
            if !self.input_end_submitted {
                return Err(invalid("provider completion precedes submitted input end"));
            }
            self.outcome = Some(RequestOutcome::Complete);
        }
        Ok(())
    }

    pub(super) fn failed(&mut self, stage: FailedStage, reason: Fault) -> io::Result<()> {
        if self.outcome.is_some() || !self.start_submitted {
            return Err(invalid("provider failure has no open submitted request"));
        }
        // Start/End can enter the provider queue before activityStart is
        // written and InputStarted is emitted. Their failure acknowledges
        // local submission only; it must never grant Started/PCM authority.
        if (stage == FailedStage::NotDelivered && self.started)
            || (stage == FailedStage::Responding && !self.started)
            || (stage == FailedStage::AwaitingResponse && !self.input_end_submitted)
        {
            return Err(invalid(
                "provider failure contradicts request delivery evidence",
            ));
        }
        self.outcome = Some(RequestOutcome::Failed {
            stage,
            reason,
            local_not_submitted: false,
        });
        Ok(())
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_submission_must_carry_the_selected_session_without_mutating_evidence() {
        let mut request = CloudRequest::new(2).unwrap();
        for session in [0, 1, 3] {
            assert!(
                request
                    .submitted(&ProviderInput::Start {
                        request: 7,
                        session,
                        privacy_generation: 1,
                        continues: false,
                        continues_cut_off: false,
                        over_the_lamp: false,
                        after_stop: false,
                        follows_on: false,
                        over_the_lamps_voice: false,
                        heard_of_cut: None,
                        soft_lead_us: 0,
                    })
                    .is_err()
            );
            assert!(!request.start_submitted);
            assert!(!request.started);
            assert!(request.outcome.is_none());
        }
        request
            .submitted(&ProviderInput::Start {
                request: 7,
                session: 2,
                privacy_generation: 1,
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
                heard_of_cut: None,
                soft_lead_us: 0,
            })
            .unwrap();
        assert!(request.confirm_start(3).is_err());
        assert!(!request.started);
        request.confirm_start(2).unwrap();
        request.require_output().unwrap();
    }

    /// The provider queue can accept Start/End then fail its activityStart write
    /// before emitting Started. Its truthful failure must not require PCM proof.
    #[test]
    fn audit_enqueued_input_failure_without_started_is_a_terminal_failure() {
        for stage in [FailedStage::InputOpen, FailedStage::AwaitingResponse] {
            let mut request = CloudRequest::new(1).unwrap();
            request
                .submitted(&ProviderInput::Start {
                    session: 1,
                    request: 1,
                    privacy_generation: 1,
                    continues: false,
                    continues_cut_off: false,
                    over_the_lamp: false,
                    after_stop: false,
                    follows_on: false,
                    over_the_lamps_voice: false,
                    heard_of_cut: None,
                    soft_lead_us: 0,
                })
                .unwrap();
            if stage == FailedStage::AwaitingResponse {
                request
                    .submitted(&ProviderInput::End {
                        request: 1,
                        privacy_generation: 1,
                        sound: false,
                        cut_in: false,
                    })
                    .unwrap();
            }
            assert!(!request.started);
            assert!(request.require_output().is_err());
            let result = request.failed(stage, Fault::Transport);
            assert!(
                result.is_ok(),
                "legitimate pre-Started {stage:?} failure rejected: {result:?}"
            );
            assert!(matches!(
                request.outcome,
                Some(RequestOutcome::Failed { .. })
            ));
            assert!(
                request.require_output().is_err(),
                "failure must not grant PCM authority"
            );
        }
    }
    use crate::{
        provider_link::{LinkContext, LinkFailure},
        provider_worker::Fault,
    };

    fn submitted_request(ended: bool) -> CloudRequest {
        let mut request = CloudRequest::new(1).unwrap();
        request
            .submitted(&ProviderInput::Start {
                session: 1,
                request: 1,
                privacy_generation: 1,
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
                heard_of_cut: None,
                soft_lead_us: 0,
            })
            .unwrap();
        if ended {
            request
                .submitted(&ProviderInput::End {
                    request: 1,
                    privacy_generation: 1,
                    sound: false,
                    cut_in: false,
                })
                .unwrap();
        }
        request
    }

    #[test]
    fn pre_started_input_open_failure_is_terminal_before_and_after_local_end() {
        for ended in [false, true] {
            let mut request = submitted_request(ended);
            request
                .failed(FailedStage::InputOpen, Fault::Transport)
                .unwrap();
            let expected = RequestOutcome::Failed {
                stage: FailedStage::InputOpen,
                reason: Fault::Transport,
                local_not_submitted: false,
            };
            assert_eq!(request.outcome, Some(expected));
            assert!(!request.started);
            assert!(request.require_output().is_err());
            assert!(request.complete(true).is_err());
            assert!(
                request.confirm_start(1).is_err(),
                "late Started cannot revive a failure"
            );
            assert!(
                request
                    .failed(FailedStage::InputOpen, Fault::Local)
                    .is_err()
            );
            assert_eq!(
                request.outcome,
                Some(expected),
                "duplicate cannot replace the original failure"
            );
        }
    }

    #[test]
    fn pre_started_awaiting_response_failure_requires_submitted_end() {
        let mut open = submitted_request(false);
        assert!(
            open.failed(FailedStage::AwaitingResponse, Fault::Transport)
                .is_err()
        );
        assert!(open.outcome.is_none());
        let mut ended = submitted_request(true);
        ended
            .failed(FailedStage::AwaitingResponse, Fault::Transport)
            .unwrap();
        assert!(matches!(
            ended.outcome,
            Some(RequestOutcome::Failed {
                stage: FailedStage::AwaitingResponse,
                ..
            })
        ));
        assert!(!ended.started);
        assert!(ended.require_output().is_err());
        assert!(ended.complete(true).is_err());
    }

    #[test]
    fn pre_started_failure_preserves_session_and_impossible_stage_checks() {
        let stages = [
            FailedStage::NotDelivered,
            FailedStage::InputOpen,
            FailedStage::AwaitingResponse,
            FailedStage::Responding,
        ];
        for stage in stages {
            let mut unsent = CloudRequest::new(1).unwrap();
            assert!(unsent.failed(stage, Fault::Transport).is_err());
            assert!(unsent.outcome.is_none());
        }
        let mut request = submitted_request(true);
        assert!(request.confirm_start(2).is_err());
        assert!(!request.started);
        assert_eq!(request.session, 1);
        assert!(request.require_output().is_err());
        assert!(
            request
                .failed(FailedStage::Responding, Fault::Transport)
                .is_err()
        );
        assert!(request.outcome.is_none());
        request.confirm_start(1).unwrap();
        assert!(
            request
                .failed(FailedStage::NotDelivered, Fault::Local)
                .is_err()
        );
        assert!(request.outcome.is_none());
        request
            .failed(FailedStage::Responding, Fault::Transport)
            .unwrap();
        assert!(request.require_output().is_err());
    }

    fn current_input() -> (lamp_turn_owner::Controller, Option<super::super::Reply>) {
        use lamp_turn_owner::{
            AdmissionState, AdmittedInput, BootId, CaptureState, Controller, MonoTime, OutputKind,
            Permission,
        };
        let at = MonoTime::from_micros(lamp_ipc::monotonic_us());
        let lease = MonoTime::from_micros(at.as_micros() + 100_000);
        let mut owner = Controller::new(BootId::new([7; 16]).unwrap(), at);
        owner
            .set_microphone_permission(at, Permission::Allowed)
            .unwrap();
        owner
            .set_capture(at, CaptureState::RetainingUntil(lease))
            .unwrap();
        owner
            .set_admission(at, AdmissionState::OpenUntil(lease))
            .unwrap();
        let turn = owner.admit(at, AdmittedInput::NewTurn).unwrap();
        let plan = owner
            .plan_output(at, turn, OutputKind::Speech, None)
            .unwrap();
        let reply = super::super::Reply {
            reminder: None,
            say: None,
            news: Vec::new(),
            down_line_said: false,
            owner: turn,
            cloud: CloudRequest::new(1).unwrap(),
            local_input_ended: false,
            plan,
            playback: None,
            pcm: std::collections::VecDeque::new(),
            generation_done: false,
            provider_idle: false,
            input_sequence: 0,
            had_audio: false,
            voice_level: 0,
            told_stop: None,
            final_chunk: None,
            playback_gaps: 0,
            gap_zeros: super::super::GapZeros::default(),
            notice: Default::default(),
            media: Default::default(),
            local_only: None,
            last_provider_audio_at_us: None,
            start: lamp_audio::playout::StartGate::new(0),
            need: lamp_audio::playout::StartNeed::default(),
            handed: 0,
            sounded: false,
        };
        (owner, Some(reply))
    }

    #[test]
    fn pre_started_failure_wrong_request_does_not_end_current_turn() {
        use crate::provider_worker::ProviderOutput;
        let (_owner, mut reply) = current_input();
        let current = reply.as_mut().unwrap();
        let request = current.owner.turn();
        current.cloud = submitted_request(false);
        let mut cloud = CloudAvailability::default();
        super::super::observe_provider_output(
            &mut cloud,
            &mut reply,
            &ProviderOutput::TurnFailed {
                request: request + 1,
                stage: FailedStage::InputOpen,
                reason: Fault::Transport,
            },
        )
        .unwrap();
        assert!(reply.as_ref().unwrap().cloud.outcome.is_none());
        assert!(!reply.as_ref().unwrap().provider_idle);
        assert!(
            super::super::observe_provider_output(
                &mut cloud,
                &mut reply,
                &ProviderOutput::Started {
                    request,
                    session: 2,
                    waiting_for_barrier: false
                }
            )
            .is_err()
        );
        assert!(!reply.as_ref().unwrap().cloud.started);
        super::super::observe_provider_output(
            &mut cloud,
            &mut reply,
            &ProviderOutput::TurnFailed {
                request,
                stage: FailedStage::InputOpen,
                reason: Fault::Transport,
            },
        )
        .unwrap();
        assert_eq!(
            reply.as_ref().unwrap().delivery_outcome(),
            "provider_failed"
        );
        assert!(reply.as_ref().unwrap().pcm.is_empty());
        assert!(reply.as_ref().unwrap().cloud.require_output().is_err());
    }

    #[test]
    fn pre_started_failure_drains_real_endpoint_and_allows_fresh_next_turn() {
        use crate::{
            activity::{Activity, ObservedAudio, TurnDetector},
            admission::{
                AcceptanceBasis, CaptureLineage, Context, Decision, Evidence, InputAdmission, Step,
                Verdict,
            },
            provider_worker::ProviderOutput,
        };
        use lamp_ipc::monotonic_us;
        use lamp_turn_owner::{AdmittedInput, BootId, MonoTime};
        use std::collections::VecDeque;
        let now = || MonoTime::from_micros(monotonic_us());
        for failure_after_end in [false, true] {
            let (mut owner, mut reply) = current_input();
            let original = reply.as_ref().unwrap().owner;
            let mut cloud = CloudAvailability::default();
            cloud
                .observe(ready(1, 1, monotonic_us()), monotonic_us())
                .unwrap();
            let mut detector = TurnDetector::default();
            let mut admission = InputAdmission::default();
            let capture = CaptureLineage {
                worker: BootId::new([8; 16]).unwrap(),
                epoch: 1,
                dsp_epoch: 1,
                privacy_generation: owner.snapshot(now()).unwrap().microphone_generation(),
            };
            let base = monotonic_us() - 1_000;
            let mut queue = VecDeque::new();
            let mut draining = false;
            let mut trace = crate::session_state::SessionState::default();
            let mut first_candidate = None;
            let mut next_admitted = false;
            for sequence in 1..=86 {
                let context = Context {
                    capture,
                    authority: owner.snapshot(now()).unwrap(),
                };
                let audio = ObservedAudio {
                    sequence,
                    captured_at_us: base + sequence,
                    samples: [1; 160],
                };
                let probability = if sequence <= 20 || sequence > 80 {
                    0.99
                } else {
                    0.0
                };
                let activity = detector.push(audio, probability);
                let is_end = matches!(activity, Activity::End(_));
                let update = admission
                    .observe(activity, context, monotonic_us())
                    .unwrap();
                if sequence == 6 {
                    let Step::Candidate(candidate) = update.step else {
                        panic!("opening candidate missing")
                    };
                    first_candidate = Some(candidate.id);
                    assert!(matches!(
                        admission
                            .decide(
                                Evidence {
                                    candidate: candidate.id,
                                    through_sequence: candidate.trigger_sequence,
                                    produced_at_us: monotonic_us(),
                                    verdict: Verdict::Accept(
                                        AcceptanceBasis::DirectedSessionVadOnly
                                    )
                                },
                                context,
                                monotonic_us()
                            )
                            .unwrap(),
                        Decision::Accepted(_)
                    ));
                    reply.as_mut().unwrap().cloud = submitted_request(false);
                    queue.push_back(ProviderInput::Audio {
                        request: original.turn(),
                        privacy_generation: capture.privacy_generation,
                        sequence: 1,
                        read_completed_at_us: base + sequence,
                        samples: vec![1; 160],
                    });
                    if failure_after_end {
                        super::super::flush_provider_input_with(
                            &cloud,
                            &mut reply,
                            &mut queue,
                            |_| Ok(()),
                        )
                        .unwrap();
                        assert!(queue.is_empty());
                    }
                } else if sequence < 80 && !failure_after_end {
                    assert!(matches!(update.step, Step::None), "abandoned tail escaped");
                } else if sequence == 80 {
                    assert!(is_end);
                    assert!(matches!(&update.step, Step::End(_) | Step::None));
                    assert_eq!(draining, !failure_after_end);
                    draining = false;
                    let current = reply.as_mut().unwrap();
                    current.local_input_ended = true;
                    owner.input_ended(now(), original).unwrap();
                    if failure_after_end {
                        assert!(matches!(update.step, Step::End(_)));
                        current
                            .cloud
                            .submitted(&ProviderInput::End {
                                request: original.turn(),
                                privacy_generation: capture.privacy_generation,
                                sound: false,
                                cut_in: false,
                            })
                            .unwrap();
                    } else {
                        assert!(matches!(update.step, Step::None));
                    }
                } else if sequence == 86 {
                    let Step::Candidate(candidate) = update.step else {
                        panic!("fresh candidate missing")
                    };
                    assert_ne!(first_candidate, Some(candidate.id));
                    let Decision::Accepted(accepted) = admission
                        .decide(
                            Evidence {
                                candidate: candidate.id,
                                through_sequence: candidate.trigger_sequence,
                                produced_at_us: monotonic_us(),
                                verdict: Verdict::Accept(AcceptanceBasis::DirectedSessionVadOnly),
                            },
                            context,
                            monotonic_us(),
                        )
                        .unwrap()
                    else {
                        panic!("fresh input was rejected")
                    };
                    assert_eq!(accepted.audio.first().unwrap().sequence, 81);
                    let next = owner.admit(now(), AdmittedInput::NewTurn).unwrap();
                    assert!(next.turn() > original.turn());
                    assert_eq!(cloud.ready_session(), Some(2));
                    let mut fresh = CloudRequest::new(2).unwrap();
                    fresh
                        .submitted(&ProviderInput::Start {
                            session: 2,
                            request: next.turn(),
                            privacy_generation: capture.privacy_generation,
                            continues: false,
                            continues_cut_off: false,
                            over_the_lamp: false,
                            after_stop: false,
                            follows_on: false,
                            over_the_lamps_voice: false,
                            heard_of_cut: None,
                            soft_lead_us: 0,
                        })
                        .unwrap();
                    fresh.confirm_start(2).unwrap();
                    fresh.require_output().unwrap();
                    next_admitted = true;
                }
                if sequence == if failure_after_end { 80 } else { 6 } {
                    let stage = if failure_after_end {
                        FailedStage::AwaitingResponse
                    } else {
                        FailedStage::InputOpen
                    };
                    super::super::observe_provider_output(
                        &mut cloud,
                        &mut reply,
                        &ProviderOutput::TurnFailed {
                            request: original.turn(),
                            stage,
                            reason: Fault::Transport,
                        },
                    )
                    .unwrap();
                    let current = reply.as_mut().unwrap();
                    super::super::discard_request_input(
                        current,
                        &mut queue,
                        &mut admission,
                        &mut draining,
                        &mut trace,
                        "provider_turn_failed",
                    )
                    .unwrap();
                    assert!(!current.cloud.started);
                    assert!(current.cloud.require_output().is_err());
                    assert!(current.pcm.is_empty());
                    assert!(queue.is_empty());
                    assert_eq!(draining, !failure_after_end);
                }
                if sequence == 10 && !failure_after_end {
                    cloud
                        .observe(ready(3, 2, monotonic_us()), monotonic_us())
                        .unwrap();
                    assert!(!reply.as_ref().unwrap().cloud.can_submit(&cloud));
                }
                if sequence == 80 {
                    let current = reply.as_ref().unwrap();
                    assert!(
                        current.provider_idle
                            && current.generation_done
                            && current.local_input_ended
                    );
                    assert_eq!(current.delivery_outcome(), "provider_failed");
                    assert!(current.pcm.is_empty() && current.playback.is_none());
                    owner.complete_turn(now(), original).unwrap();
                    admission
                        .owner_completed(original, owner.snapshot(now()).unwrap(), monotonic_us())
                        .unwrap();
                    reply = None;
                    assert!(owner.snapshot(now()).unwrap().listening_ready(now()));
                    if failure_after_end {
                        cloud
                            .observe(ready(3, 2, monotonic_us()), monotonic_us())
                            .unwrap();
                    }
                }
            }
            assert!(next_admitted);
            assert!(!draining);
            assert!(queue.is_empty());
        }
    }

    fn ready(revision: u64, session: u64, at: u64) -> ProviderLink {
        ProviderLink {
            revision,
            session,
            observed_at_us: at,
            source_clock: None,
            state: ProviderLinkState::Ready {
                setup_us: 10,
                attempts: 1,
                context: if session == 1 {
                    LinkContext::Initial
                } else {
                    LinkContext::Resumed
                },
                outage_started_at_us: (session > 1).then_some(at - 20),
                after: (session > 1).then_some(Fault::Transport),
            },
        }
    }

    /// #1844: the second route as the lamp's only voice says its link down
    /// and back in an order this register takes, so the offline line, the
    /// breath and "I'm back online" follow it as they follow Gemini's.
    #[test]
    fn the_second_routes_link_down_and_back_is_taken() {
        use crate::second_provider::link::Link;
        let s = 1_000_000u64;
        let mut cloud = CloudAvailability::default();
        let (mut link, ready) = Link::initial(s, s + 10);
        cloud.observe(ready, 2 * s).unwrap();
        assert!(cloud.available());
        assert!(link.failed(10 * s, Fault::Transport));
        let down = link.probed(10 * s, 12 * s, false).unwrap();
        cloud.observe(down, 12 * s).unwrap();
        assert!(!cloud.available());
        let back = link.probed(42 * s, 43 * s, true).unwrap();
        cloud.observe(back, 43 * s).unwrap();
        assert_eq!(cloud.ready_session(), Some(2));
        // And down again, in the new session.
        assert!(link.failed(50 * s, Fault::ConnectTimeout));
        cloud
            .observe(link.probed(50 * s, 52 * s, false).unwrap(), 52 * s)
            .unwrap();
        assert!(!cloud.available());
    }

    #[test]
    fn readiness_requires_a_priority_source_observation() {
        let mut cloud = CloudAvailability::default();
        assert!(!cloud.available());
        cloud.observe(ready(1, 1, 100), 200).unwrap();
        assert_eq!(cloud.ready_session(), Some(1));
    }

    /// What a kept link reports when the lamp starts before its network
    /// (`RecoveryPolicy::endless`): no connection on the first session, then
    /// ready on a later one. Nothing is available until that ready.
    #[test]
    fn a_first_connection_that_failed_and_then_came_up_opens_submissions() {
        let mut cloud = CloudAvailability::default();
        cloud
            .observe(
                ProviderLink {
                    revision: 1,
                    session: 1,
                    observed_at_us: 100,
                    source_clock: None,
                    state: ProviderLinkState::Unavailable {
                        reason: Fault::Transport,
                    },
                },
                150,
            )
            .unwrap();
        assert!(!cloud.available());
        cloud
            .observe(
                ProviderLink {
                    revision: 2,
                    session: 4,
                    observed_at_us: 300,
                    source_clock: None,
                    state: ProviderLinkState::Ready {
                        setup_us: 10,
                        attempts: 4,
                        context: LinkContext::Fresh,
                        outage_started_at_us: Some(100),
                        after: Some(Fault::Transport),
                    },
                },
                300,
            )
            .unwrap();
        assert_eq!(cloud.ready_session(), Some(4));
    }

    #[test]
    fn unavailable_session_cannot_be_resurrected_by_ready() {
        let mut cloud = CloudAvailability::default();
        cloud.observe(ready(1, 1, 100), 200).unwrap();
        cloud
            .observe(
                ProviderLink {
                    revision: 2,
                    session: 1,
                    observed_at_us: 200,
                    source_clock: None,
                    state: ProviderLinkState::Unavailable {
                        reason: Fault::Transport,
                    },
                },
                200,
            )
            .unwrap();
        assert!(!cloud.available());
        assert!(cloud.observe(ready(3, 1, 201), 201).is_err());
        assert!(!cloud.available());
        cloud.observe(ready(4, 2, 300), 301).unwrap();
        assert_eq!(cloud.ready_session(), Some(2));
    }

    #[test]
    fn coalesced_recovery_preserves_the_new_session_and_original_source_time() {
        let mut cloud = CloudAvailability::default();
        cloud.observe(ready(1, 1, 100), 100).unwrap();
        // The outage did not fit in the socket; its recovery replaces it.
        cloud.observe(ready(3, 2, 500), 50_000).unwrap();
        assert_eq!(cloud.ready_session(), Some(2));
        assert_eq!(cloud.latest.unwrap().observed_at_us, 500);
    }

    #[test]
    fn revision_session_source_time_and_terminal_are_independent_bounds() {
        let mut cloud = CloudAvailability::default();
        cloud.observe(ready(3, 2, 300), 300).unwrap();
        for bad in [
            ready(3, 3, 400),
            ready(4, 1, 400),
            ready(4, 3, 299),
            ready(4, 3, 401),
        ] {
            assert!(cloud.observe(bad, 400).is_err());
            assert_eq!(cloud.ready_session(), Some(2));
        }
        cloud
            .observe(
                ProviderLink {
                    revision: 4,
                    session: 2,
                    observed_at_us: 400,
                    source_clock: None,
                    state: ProviderLinkState::Terminal {
                        reason: LinkFailure::NoResumableContext,
                        fault: Fault::Transport,
                        attempts: 0,
                    },
                },
                400,
            )
            .unwrap();
        assert!(!cloud.available());
        assert!(cloud.observe(ready(5, 3, 500), 500).is_err());
        // A lost connection is not a refusal that passes with time.
        assert!(cloud.refused_for_now().is_none());
    }

    /// #2231 (the reviewer on #2459): `refusal()` names a refused key, a
    /// spent quota and a rate limit, from a link unavailable or ended, and
    /// nothing else, so any other fault is said as a link down.
    #[test]
    fn a_refusal_is_named_from_an_unavailable_or_ended_link_and_nothing_else_is() {
        let seen = |state: ProviderLinkState| {
            let mut cloud = CloudAvailability::default();
            cloud
                .observe(
                    ProviderLink {
                        revision: 1,
                        session: 1,
                        observed_at_us: 100,
                        source_clock: None,
                        state,
                    },
                    100,
                )
                .unwrap();
            cloud.refusal()
        };
        for fault in [
            Fault::Authentication,
            Fault::QuotaExceeded,
            Fault::RateLimited,
        ] {
            assert_eq!(
                seen(ProviderLinkState::Unavailable { reason: fault }),
                Some(fault)
            );
            assert_eq!(
                seen(ProviderLinkState::Terminal {
                    reason: LinkFailure::InitialConnect,
                    fault,
                    attempts: 0,
                }),
                Some(fault)
            );
        }
        for fault in [
            Fault::Transport,
            Fault::ConnectTimeout,
            Fault::ServerUnavailable,
        ] {
            assert_eq!(seen(ProviderLinkState::Unavailable { reason: fault }), None);
        }
        assert_eq!(CloudAvailability::default().refusal(), None);
    }

    #[test]
    fn a_spent_quota_or_a_rate_limit_is_a_refusal_for_now_and_nothing_else_is() {
        let ended_on = |fault: Fault| {
            let mut cloud = CloudAvailability::default();
            assert!(cloud.refused_for_now().is_none());
            cloud
                .observe(
                    ProviderLink {
                        revision: 1,
                        session: 1,
                        observed_at_us: 100,
                        source_clock: None,
                        state: ProviderLinkState::Terminal {
                            reason: LinkFailure::InitialConnect,
                            fault,
                            attempts: 0,
                        },
                    },
                    100,
                )
                .unwrap();
            cloud.refused_for_now().is_some()
        };
        assert!(ended_on(Fault::QuotaExceeded));
        assert!(ended_on(Fault::RateLimited));
        // #2231: a refused key is not mended by asking again at once either.
        assert!(ended_on(Fault::Authentication));
        for other in [
            Fault::ServerUnavailable,
            Fault::Transport,
            Fault::ConnectTimeout,
        ] {
            assert!(!ended_on(other), "{other:?}");
        }
    }

    #[test]
    fn ordered_end_closes_admission_even_before_priority_terminal_arrives() {
        let mut cloud = CloudAvailability::default();
        cloud.observe(ready(1, 1, 100), 100).unwrap();
        cloud.end_output().unwrap();
        assert!(!cloud.available());
        assert!(cloud.end_output().is_err());
        cloud.observe(ready(3, 2, 200), 200).unwrap();
        assert!(!cloud.available());
        cloud
            .observe(
                ProviderLink {
                    revision: 4,
                    session: 2,
                    observed_at_us: 200,
                    source_clock: None,
                    state: ProviderLinkState::Terminal {
                        reason: LinkFailure::NoResumableContext,
                        fault: Fault::Transport,
                        attempts: 0,
                    },
                },
                200,
            )
            .unwrap();
        assert!(cloud.output_ended());
    }
}

#[cfg(test)]
mod state_boundary_tests {
    use super::*;
    use crate::provider_state::{conversation::Snapshot, names::NamesChunk};
    use crate::provider_worker::ProviderOutput;

    fn names(revision: u64, index: u8, count: u8, label: &str) -> ProviderOutput {
        ProviderOutput::MachineNames {
            chunk: NamesChunk {
                revision,
                index,
                count,
                enabled: true,
                pairs: vec![(format!("target-{index}"), label.into())],
            },
        }
    }

    #[test]
    fn conversation_metadata_expires_without_packets_and_survives_session_change() {
        let mut cloud = CloudAvailability::default();
        let event = ProviderOutput::ConversationState {
            snapshot: Snapshot {
                observed_at_us: 100,
                until_us: Some(200),
            },
        };
        // The only state on the wire is two clocks, never thread text/person IDs.
        let encoded = serde_json::to_value(&event).unwrap();
        assert_eq!(
            encoded["snapshot"],
            serde_json::json!({"observed_at_us":100,"until_us":200})
        );
        cloud.observe_state(&event, 100).unwrap();
        for session in [1, 2] {
            cloud
                .observe(
                    ProviderLink {
                        revision: session,
                        session,
                        observed_at_us: 100 + session,
                        source_clock: None,
                        state: ProviderLinkState::Ready {
                            setup_us: 0,
                            attempts: 1,
                            context: if session == 1 {
                                crate::provider_link::LinkContext::Initial
                            } else {
                                crate::provider_link::LinkContext::Resumed
                            },
                            outage_started_at_us: (session > 1).then_some(100),
                            after: (session > 1).then_some(Fault::Transport),
                        },
                    },
                    100 + session,
                )
                .unwrap();
            assert!(super::super::conversation_going_on(&cloud, 200));
        }
        assert!(!super::super::conversation_going_on(&cloud, 201));
        assert!(!super::super::conversation_going_on(
            &CloudAvailability::default(),
            150
        ));
        cloud.end_output().unwrap();
        assert!(cloud.observe_state(&event, 150).is_err());
    }

    #[test]
    fn active_names_switch_boots_and_keep_incomplete_standby_private() {
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        struct Restore(crate::agent_tasks::MachineNames);
        impl Drop for Restore {
            fn drop(&mut self) {
                assert!(crate::agent_tasks::apply_parent_snapshot(&self.0));
            }
        }
        let _restore = Restore(crate::agent_tasks::machine_names_after(None).unwrap().1);
        let boot = |n| crate::coordinator::BootId::new([n; 16]).unwrap();
        let mut active = CloudAvailability::default();
        active
            .observe_state(&names(1, 0, 1, "Synthetic A"), 100)
            .unwrap();
        let mut standby = CloudAvailability::default();
        standby
            .observe_state(&names(1, 0, 2, "Synthetic B"), 100)
            .unwrap();
        let mut selected = ActiveNames::default();
        selected.sync(boot(71), &active);
        assert_eq!(
            crate::agent_tasks::machine_name("target-0").as_deref(),
            Some("Synthetic A")
        );
        // Switching to an incomplete new boot clears old labels; its partial
        // labels are not yet usable by task_news's production lookup.
        selected.sync(boot(72), &standby);
        assert_eq!(crate::agent_tasks::machine_name("target-0"), None);
        standby
            .observe_state(&names(1, 1, 2, "Synthetic C"), 101)
            .unwrap();
        selected.sync(boot(72), &standby);
        assert_eq!(
            crate::agent_tasks::machine_name("target-0").as_deref(),
            Some("Synthetic B")
        );
        selected.sync(boot(71), &active);
        assert_eq!(
            crate::agent_tasks::machine_name("target-0").as_deref(),
            Some("Synthetic A")
        );
        selected.sync(boot(73), &CloudAvailability::default());
        assert_eq!(crate::agent_tasks::machine_name("target-0"), None);
        // Equal revision numbers belong to distinct boots, not a global clock.
        selected.sync(boot(72), &standby);
        assert_eq!(
            crate::agent_tasks::machine_name("target-1").as_deref(),
            Some("Synthetic C")
        );
    }
}

#[cfg(test)]
mod state_host_timing {
    use super::*;
    use crate::{
        provider_state::names::NamesChunk, provider_worker::ProviderOutput,
        transport::WorkerChannels,
    };
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "synthetic host timing; run alone with --ignored --nocapture"]
    fn synthetic_state_transport_and_observation_timing() {
        struct Directory(std::path::PathBuf);
        impl Drop for Directory {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let root = Directory(std::env::temp_dir().join(format!(
            "state-timing-{}-{}",
            std::process::id(),
            lamp_ipc::monotonic_us()
        )));
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root.0)
            .unwrap();
        let parent_boot = crate::coordinator::BootId::new([81; 16]).unwrap();
        let worker_boot = crate::coordinator::BootId::new([82; 16]).unwrap();
        let mut parent =
            WorkerChannels::bind(&root.0, "provider", parent_boot, worker_boot, false).unwrap();
        let mut worker =
            WorkerChannels::bind(&root.0, "provider", parent_boot, worker_boot, true).unwrap();
        parent.connect(&root.0, "provider", false).unwrap();
        worker.connect(&root.0, "provider", true).unwrap();
        let mut conversation = CloudAvailability::default();
        let mut local = Vec::new();
        let mut transport = Vec::new();
        for sample in 0..201 {
            let now = lamp_ipc::monotonic_us();
            let event = ProviderOutput::ConversationState {
                snapshot: crate::provider_state::conversation::Snapshot {
                    observed_at_us: now,
                    until_us: Some(now + 45_000_000),
                },
            };
            let start = Instant::now();
            worker.data.send(&event).unwrap();
            let end = Instant::now() + Duration::from_secs(1);
            let received = loop {
                if let Some(event) = parent.data.receive::<ProviderOutput>().unwrap() {
                    break event;
                }
                assert!(Instant::now() < end);
                std::thread::yield_now();
            };
            let handling = Instant::now();
            super::super::observe_provider_output(&mut conversation, &mut None, &received).unwrap();
            let elapsed = handling.elapsed();
            assert!(super::super::conversation_going_on(
                &conversation,
                lamp_ipc::monotonic_us()
            ));
            let total = start.elapsed();
            if sample == 0 {
                eprintln!("conversation first invocation handler={elapsed:?} transport={total:?}");
            } else {
                local.push(elapsed);
                transport.push(total);
            }
        }
        local.sort();
        transport.sort();
        eprintln!(
            "conversation repeated samples=200 handler_p50={:?} handler_p95={:?} handler_max={:?} transport_p50={:?} transport_p95={:?} transport_max={:?}",
            local[100],
            local[190],
            local.last().unwrap(),
            transport[100],
            transport[190],
            transport.last().unwrap()
        );
        assert!(local[190] < Duration::from_millis(1));
        assert!(transport[190] < Duration::from_millis(10));
        for size in [4usize, 800] {
            let pairs: Vec<_> = (0..size)
                .map(|i| {
                    (
                        format!("target-{i}"),
                        format!("Synthetic {}", "x".repeat(40)),
                    )
                })
                .collect();
            let fixture = crate::agent_tasks::MachineNames {
                enabled: true,
                names: pairs.iter().cloned().collect(),
            };
            assert!(fixture.valid());
            let bytes = serde_json::to_vec(&fixture).unwrap().len();
            let mut cloud = CloudAvailability::default();
            let mut handlers = Vec::new();
            let mut transfers = Vec::new();
            for revision in 1..=201 {
                let count = pairs.len().div_ceil(64) as u8;
                let events: Vec<_> = pairs
                    .chunks(64)
                    .enumerate()
                    .map(|(index, pairs)| ProviderOutput::MachineNames {
                        chunk: NamesChunk {
                            revision,
                            index: index as u8,
                            count,
                            enabled: true,
                            pairs: pairs.to_vec(),
                        },
                    })
                    .collect();
                let start = Instant::now();
                let mut largest_handler = Duration::ZERO;
                for event in events {
                    worker.data.send(&event).unwrap();
                    let receive_by = Instant::now() + Duration::from_secs(1);
                    let received = loop {
                        if let Some(event) = parent.data.receive::<ProviderOutput>().unwrap() {
                            break event;
                        }
                        assert!(Instant::now() < receive_by, "local datagram did not arrive");
                        std::thread::yield_now();
                    };
                    let at = Instant::now();
                    // The actual coordinator dispatch, including complete-map
                    // installation in CloudAvailability, but not ActiveNames.
                    super::super::observe_provider_output(&mut cloud, &mut None, &received)
                        .unwrap();
                    largest_handler = largest_handler.max(at.elapsed());
                }
                let elapsed = start.elapsed();
                assert_eq!(cloud.names.snapshot(), &fixture);
                if revision == 1 {
                    eprintln!(
                        "state host first invocation entries={size} bytes={bytes} handler_max={largest_handler:?} full_transfer={elapsed:?}"
                    );
                } else {
                    handlers.push(largest_handler);
                    transfers.push(elapsed);
                }
            }
            handlers.sort();
            transfers.sort();
            eprintln!(
                "state host repeated entries={size} samples={} handler_p50={:?} handler_p95={:?} handler_max={:?} full_transfer_p50={:?} full_transfer_p95={:?} full_transfer_max={:?}",
                handlers.len(),
                handlers[100],
                handlers[190],
                handlers.last().unwrap(),
                transfers[100],
                transfers[190],
                transfers.last().unwrap()
            );
            assert!(
                handlers[190] < Duration::from_millis(1),
                "local handler p95 target"
            );
            assert!(
                transfers[190] < Duration::from_millis(10),
                "unloaded local transport+observation p95 target"
            );
        }
    }
    #[test]
    #[ignore = "synthetic parent installation timing; run alone with --ignored --nocapture"]
    fn synthetic_active_names_installation_timing() {
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        struct Restore(crate::agent_tasks::MachineNames);
        impl Drop for Restore {
            fn drop(&mut self) {
                assert!(crate::agent_tasks::apply_parent_snapshot(&self.0));
            }
        }
        let _restore = Restore(crate::agent_tasks::machine_names_after(None).unwrap().1);
        let boot = crate::coordinator::BootId::new([83; 16]).unwrap();
        let mut failures = Vec::new();
        for size in [4usize, 798] {
            let fixture = crate::agent_tasks::MachineNames {
                enabled: true,
                names: (0..size)
                    .map(|i| (format!("target-{i:05}"), "x".repeat(64)))
                    .collect(),
            };
            assert!(fixture.valid());
            let bytes = serde_json::to_vec(&fixture).unwrap().len();
            if size == 798 {
                assert_eq!(bytes, 65_462);
            }
            let mut cloud = CloudAvailability::default();
            let mut active = ActiveNames::default();
            let mut installs = Vec::new();
            let mut lookups = Vec::new();
            let task = serde_json::json!({"target_alias":"target-00000"});
            for revision in 1..=201 {
                let label = if revision % 2 == 0 { "a" } else { "b" }.repeat(64);
                let pairs: Vec<_> = fixture
                    .names
                    .keys()
                    .map(|key| (key.clone(), label.clone()))
                    .collect();
                let count = pairs.len().div_ceil(64) as u8;
                // Build and receive the completed revision before timing the
                // separate parent install. No fixture validation is timed.
                for (index, pairs) in pairs.chunks(64).enumerate() {
                    let event = ProviderOutput::MachineNames {
                        chunk: NamesChunk {
                            revision,
                            index: index as u8,
                            count,
                            enabled: true,
                            pairs: pairs.to_vec(),
                        },
                    };
                    super::super::observe_provider_output(&mut cloud, &mut None, &event).unwrap();
                }
                assert_eq!(cloud.names_revision, Some(revision));
                let start = Instant::now();
                active.sync(boot, &cloud);
                let install = start.elapsed();
                let start = Instant::now();
                let told = crate::task_news::their_computer(&task);
                let lookup = start.elapsed();
                assert_eq!(told, format!("their {label}"));
                if revision == 1 {
                    eprintln!(
                        "parent active names first invocation entries={size} bytes={bytes} install={install:?} task_news_lookup={lookup:?}"
                    );
                } else {
                    installs.push(install);
                    lookups.push(lookup);
                }
            }
            installs.sort();
            lookups.sort();
            eprintln!(
                "parent active names repeated entries={size} bytes={bytes} samples=200 install_p50={:?} install_p95={:?} install_max={:?} task_news_lookup_p50={:?} task_news_lookup_p95={:?} task_news_lookup_max={:?}",
                installs[100],
                installs[190],
                installs.last().unwrap(),
                lookups[100],
                lookups[190],
                lookups.last().unwrap()
            );
            if installs[190] >= Duration::from_millis(1) {
                failures.push((size, installs[190]));
            }
        }
        assert!(
            failures.is_empty(),
            "parent installation p95 exceeds 1ms: {failures:?}"
        );
    }
}
