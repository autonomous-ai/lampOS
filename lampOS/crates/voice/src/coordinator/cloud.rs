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
}

impl CloudAvailability {
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

    /// The link ended on a refusal that passes with time: the provider's
    /// quota or its rate limit. Asking again at once only adds load.
    pub(super) fn refused_for_now(&self) -> bool {
        matches!(
            self.latest.map(|link| link.state),
            Some(ProviderLinkState::Terminal {
                fault: Fault::QuotaExceeded | Fault::RateLimited,
                ..
            })
        )
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
        match command {
            ProviderInput::Start { .. } => self.start_submitted = true,
            ProviderInput::End { .. } => self.input_end_submitted = true,
            ProviderInput::Audio { .. } | ProviderInput::Called { .. } => {}
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
            notice: Default::default(),
            local_only: None,
            last_provider_audio_at_us: None,
            start: lamp_audio::playout::StartGate::new(0),
            need: lamp_audio::playout::StartNeed::default(),
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
        assert!(!cloud.refused_for_now());
    }

    #[test]
    fn a_spent_quota_or_a_rate_limit_is_a_refusal_for_now_and_nothing_else_is() {
        let ended_on = |fault: Fault| {
            let mut cloud = CloudAvailability::default();
            assert!(!cloud.refused_for_now());
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
            cloud.refused_for_now()
        };
        assert!(ended_on(Fault::QuotaExceeded));
        assert!(ended_on(Fault::RateLimited));
        for other in [
            Fault::Authentication,
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
