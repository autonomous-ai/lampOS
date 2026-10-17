//! The real Coordinated worker and Supervisor use private Unix IPC; the model
//! service is in memory. These are protocol tests, not process/acoustic timing.
use super::*;
use crate::provider_worker::{Continuation, FailedStage, Fault, serve_coordinated_test};
use lamp_gemini::{
    Attempt, BarrierPolicy, Connect, Credential, RecoveryPolicy, Resumption, ResumptionHandle,
    SessionConfig, SessionId, Supervisor,
    testing::{Dial, audio},
};
use tokio::{sync::mpsc::UnboundedReceiver, task::JoinHandle};

// A bounded duplex stream is still the actual Gemini actor/transport. The
// 16-byte case stops the writer before it can finish activityStart, making
// pre-Started InputOpen/AwaitingResponse failures deterministic.
struct ScriptedConnector {
    config: SessionConfig,
    dials: tokio::sync::mpsc::UnboundedSender<Dial>,
    capacity: usize,
}
impl Connect for ScriptedConnector {
    fn connect(&mut self, session: SessionId, resume: Option<ResumptionHandle>) -> Attempt {
        let resumed = resume.is_some();
        let config = self.config.clone().resume(resume);
        let capacity = self.capacity;
        let dials = self.dials.clone();
        Box::pin(async move {
            let (attempt, service) = lamp_gemini::testing::pair(config, session, capacity).await;
            let _ = dials.send(Dial {
                session,
                resumed,
                service,
            });
            attempt.await
        })
    }
}

struct Paired {
    _directory: SessionDirectory,
    channels: WorkerChannels,
    owner: Controller,
    reply: Option<Reply>,
    cloud: CloudAvailability,
    flow: OutputReceiver,
    queue: VecDeque<ProviderInput>,
    admission: InputAdmission,
    draining: bool,
    trace: SessionState,
    outputs: Vec<ProviderOutput>,
    dials: UnboundedReceiver<Dial>,
    worker: Option<JoinHandle<Result<()>>>,
}

impl Drop for Paired {
    fn drop(&mut self) {
        // Also stop a failing test's worker; no detached provider task survives
        // this test's in-memory runtime.
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
    }
}

impl Paired {
    async fn new() -> Self {
        Self::with_capacity(lamp_gemini::MAX_WIRE_BYTES * 2).await
    }

    async fn with_capacity(capacity: usize) -> Self {
        Self::with(capacity, Continuation::Off).await
    }

    /// The worker's session configured as `--provider-continuation` does it.
    async fn with(capacity: usize, continuation: Continuation) -> Self {
        let directory = SessionDirectory::create().unwrap();
        let (channels, child) = pair(&directory, "provider");
        let (mut owner, reply) = current_input();
        let boot = owner.snapshot(now()).unwrap().boot();
        let config = SessionConfig::new(
            lamp_gemini::GOOGLE_ENDPOINT,
            Credential::api_key("test-only").unwrap(),
        )
        .unwrap()
        .resumption(Resumption::Retain)
        .barrier_policy(BarrierPolicy::Require);
        let config = continuation.apply(config).unwrap();
        let (sender, dials) = tokio::sync::mpsc::unbounded_channel();
        let provider = Supervisor::new(
            ScriptedConnector {
                config,
                dials: sender,
                capacity,
            },
            RecoveryPolicy {
                first_backoff: Duration::from_millis(20),
                max_backoff: Duration::from_millis(40),
                ..RecoveryPolicy::default()
            },
        )
        .unwrap();
        let mut rig = Self {
            _directory: directory,
            channels,
            owner,
            reply,
            cloud: CloudAvailability::default(),
            flow: OutputReceiver::default(),
            queue: VecDeque::new(),
            admission: InputAdmission::default(),
            draining: false,
            trace: SessionState::default(),
            outputs: Vec::new(),
            dials,
            worker: Some(tokio::spawn(serve_coordinated_test(child, boot, provider))),
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            rig.publish();
            if let Some(event) = rig.channels.control.receive::<WorkerEvent>().unwrap() {
                assert!(matches!(event, WorkerEvent::Ready));
                break;
            }
            rig.assert_running(deadline).await;
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        rig
    }

    async fn assert_running(&mut self, deadline: Instant) {
        assert!(Instant::now() < deadline, "paired provider test deadline");
        if self.worker.as_ref().unwrap().is_finished() {
            let result = self.worker.take().unwrap().await;
            panic!("paired provider worker ended unexpectedly: {result:?}");
        }
    }

    fn publish(&mut self) {
        let at = now();
        let until = at.checked_add(INPUT_LEASE_US).unwrap();
        self.owner
            .set_capture(at, CaptureState::RetainingUntil(until))
            .unwrap();
        self.owner
            .set_admission(at, AdmissionState::OpenUntil(until))
            .unwrap();
        let snapshot = self.owner.snapshot(at).unwrap();
        self.channels
            .control
            .send(Control::Authority { snapshot })
            .unwrap();
    }

    async fn tick(&mut self, return_credit: bool) {
        self.publish();
        assert!(
            service_provider_control(
                &mut self.channels.control,
                &mut self.cloud,
                &mut 16,
                &mut self.trace,
            )
            .unwrap()
        );
        let mut slice = ProviderSlice::new(monotonic_us());
        while let Some(event) = slice
            .receive(&mut self.channels.data, monotonic_us())
            .unwrap()
        {
            self.flow.received().unwrap();
            observe_provider_output(&mut self.cloud, &mut self.reply, &event).unwrap();
            match &event {
                ProviderOutput::Audio {
                    request, samples, ..
                } => {
                    let current = self.reply.as_mut().unwrap();
                    assert_eq!(current.owner.turn(), *request);
                    current.append_provider_audio(samples.clone(), monotonic_us());
                }
                ProviderOutput::TurnFailed { request, .. } => {
                    let current = self.reply.as_mut().unwrap();
                    assert_eq!(current.owner.turn(), *request);
                    discard_request_input(
                        current,
                        &mut self.queue,
                        &mut self.admission,
                        &mut self.draining,
                        &mut self.trace,
                        "provider_turn_failed",
                    )
                    .unwrap();
                }
                _ => {}
            }
            self.outputs.push(event);
        }
        if return_credit {
            self.flow
                .replenish(
                    self.reply.as_ref().map_or(0, |reply| reply.pcm.len()),
                    &mut self.channels.control,
                )
                .unwrap();
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }

    async fn until(&mut self, return_credit: bool, done: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !done(self) {
            self.assert_running(deadline).await;
            self.tick(return_credit).await;
        }
    }

    async fn dial(&mut self, return_credit: bool) -> Dial {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(dial) = self.dials.try_recv() {
                return dial;
            }
            self.assert_running(deadline).await;
            self.tick(return_credit).await;
        }
    }

    async fn ready(&mut self) -> Dial {
        let mut dial = self.dial(false).await;
        dial.service.accept().await;
        self.until(false, |rig| rig.cloud.available()).await;
        assert_eq!(self.cloud.ready_session(), Some(dial.session.get()));
        assert!(self.outputs.is_empty(), "Ready is priority-only");
        dial
    }

    fn submit(&mut self, selected: &CloudAvailability, end: bool) {
        let current = self.reply.as_ref().unwrap();
        let request = current.owner.turn();
        let privacy_generation = self.owner.snapshot(now()).unwrap().microphone_generation();
        self.queue.push_back(ProviderInput::Start {
            request,
            session: current.cloud.session,
            privacy_generation,
            continues: false,
            continues_cut_off: false,
            over_the_lamp: false,
            after_stop: false,
            follows_on: false,
            over_the_lamps_voice: false,
        });
        self.queue.push_back(ProviderInput::Audio {
            request,
            privacy_generation,
            sequence: 1,
            read_completed_at_us: monotonic_us(),
            samples: vec![73; 160],
        });
        if end {
            self.queue.push_back(ProviderInput::End {
                request,
                privacy_generation,
                sound: false,
                cut_in: false,
            });
            self.owner.input_ended(now(), current.owner).unwrap();
            self.reply.as_mut().unwrap().local_input_ended = true;
        }
        self.publish();
        flush_provider_input(
            selected,
            &mut self.reply,
            &mut self.queue,
            &mut self.channels.data,
        )
        .unwrap();
        assert!(self.queue.is_empty());
    }

    async fn stop(mut self) {
        self.channels.control.send(Control::Stop).unwrap();
        tokio::time::timeout(Duration::from_secs(1), self.worker.take().unwrap())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}

#[tokio::test]
async fn paired_outage_bypasses_zero_pcm_credit_and_retains_failed_answer_in_order() {
    let mut rig = Paired::new().await;
    let mut first = rig.ready().await;
    let mut selected = CloudAvailability::default();
    selected
        .observe(ready_link(1, first.session.get()), monotonic_us())
        .unwrap();
    rig.submit(&selected, true);
    rig.until(false, |rig| rig.outputs.len() == 1).await;
    assert!(matches!(rig.outputs[0], ProviderOutput::Started { .. }));
    for field in ["activityStart", "audio", "activityEnd"] {
        assert!(
            first.service.receive().await["realtimeInput"]
                .get(field)
                .is_some()
        );
    }
    first.service.send(audio(31, 960)).await;
    rig.until(false, |rig| rig.outputs.len() == 2).await;
    assert_eq!(rig.reply.as_ref().unwrap().pcm, vec![31; 960]);
    let original_owner = rig.reply.as_ref().unwrap().owner;
    first
        .service
        .send(json!({"sessionResumptionUpdate":{
            "newHandle":"paired-test-handle","resumable":true
        }}))
        .await;
    first.service.send(audio(41, 960)).await;
    first.service.close(1012, "paired outage").await;
    rig.until(false, |rig| !rig.cloud.available()).await;
    assert_eq!(
        rig.outputs.len(),
        2,
        "no credit for the queued audio or failure"
    );
    assert_eq!(rig.reply.as_ref().unwrap().pcm.len(), 960);
    apply_submission_gate(
        &rig.cloud,
        &mut rig.reply,
        &mut rig.queue,
        &mut rig.admission,
        &mut rig.draining,
        &mut rig.trace,
    )
    .unwrap();
    assert!(!rig.draining, "the original End was already submitted");
    assert_eq!(
        rig.owner.snapshot(now()).unwrap().owner(),
        Some(original_owner)
    );

    let mut second = rig.dial(false).await;
    assert!(second.session > first.session);
    second.service.accept().await;
    rig.until(false, |rig| rig.cloud.available()).await;
    assert_eq!(rig.outputs.len(), 2, "recovery also bypasses PCM credit");
    assert_eq!(rig.cloud.ready_session(), Some(second.session.get()));
    assert!(
        second
            .service
            .try_receive(Duration::from_millis(5))
            .await
            .is_none(),
        "old input was replayed on the replacement"
    );

    rig.until(true, |rig| {
        rig.reply.as_ref().unwrap().cloud.outcome.is_some()
    })
    .await;
    assert_eq!(rig.outputs.len(), 4);
    assert!(matches!(rig.outputs[2], ProviderOutput::Audio { .. }));
    assert!(matches!(
        rig.outputs[3],
        ProviderOutput::TurnFailed {
            stage: FailedStage::Responding,
            ..
        }
    ));
    let current = rig.reply.as_mut().unwrap();
    assert_eq!(current.pcm, [vec![31; 960], vec![41; 960]].concat());
    assert!(matches!(
        current.cloud.outcome,
        Some(RequestOutcome::Failed { .. })
    ));
    assert_ne!(current.delivery_outcome(), "audio_written_unscored");
    let mut played = Vec::new();
    while let Some((samples, _)) = current.next_speaker_chunk() {
        played.extend(samples);
    }
    assert_eq!(played, [vec![31; 960], vec![41; 960]].concat());
    assert_eq!(
        rig.owner.snapshot(now()).unwrap().owner(),
        Some(original_owner)
    );
    rig.stop().await;
}

#[tokio::test]
async fn paired_checked_start_refusal_is_not_replayed_and_consumer_accepts_not_delivered() {
    let mut rig = Paired::new().await;
    let mut first = rig.ready().await;
    // Keep the coordinator view that selected A. The command crosses the
    // control/data race after the worker has learned B; it must retain A.
    let mut selected = CloudAvailability::default();
    selected
        .observe(ready_link(1, first.session.get()), monotonic_us())
        .unwrap();
    first
        .service
        .send(json!({"sessionResumptionUpdate":{
            "newHandle":"paired-test-handle","resumable":true
        }}))
        .await;
    first
        .service
        .close(1012, "paired selected-session race")
        .await;
    rig.until(false, |rig| !rig.cloud.available()).await;
    let mut second = rig.dial(false).await;
    second.service.accept().await;
    rig.until(false, |rig| rig.cloud.available()).await;
    assert_eq!(rig.cloud.ready_session(), Some(second.session.get()));

    // Retain the admission state of the original open utterance. A refusal
    // drains it through an actual endpoint; recovery cannot rearm its tail.
    let capture = CaptureLineage {
        worker: BootId::new([77; 16]).unwrap(),
        epoch: 1,
        dsp_epoch: 1,
        privacy_generation: rig.owner.snapshot(now()).unwrap().microphone_generation(),
    };
    let at = monotonic_us();
    let context = AdmissionContext {
        capture,
        authority: rig.owner.snapshot(MonoTime::from_micros(at)).unwrap(),
    };
    let update = rig
        .admission
        .observe(
            Activity::Start(
                (1..=6)
                    .map(|sequence| ObservedAudio {
                        sequence,
                        captured_at_us: at - 6 + sequence,
                        samples: [73; 160],
                    })
                    .collect(),
            ),
            context,
            monotonic_us(),
        )
        .unwrap();
    let AdmissionStep::Candidate(candidate) = update.step else {
        panic!("missing original candidate")
    };
    assert!(matches!(
        rig.admission
            .decide(
                Evidence {
                    candidate: candidate.id,
                    through_sequence: candidate.trigger_sequence,
                    produced_at_us: monotonic_us(),
                    verdict: Verdict::Accept(AcceptanceBasis::DirectedSessionVadOnly),
                },
                context,
                monotonic_us()
            )
            .unwrap(),
        Decision::Accepted(_)
    ));
    rig.submit(&selected, false);
    rig.until(true, |rig| {
        rig.reply.as_ref().unwrap().cloud.outcome.is_some()
    })
    .await;
    assert_eq!(rig.outputs.len(), 1);
    assert!(matches!(
        rig.outputs[0],
        ProviderOutput::TurnFailed {
            stage: FailedStage::NotDelivered,
            reason: Fault::Local,
            ..
        }
    ));
    assert!(!rig.reply.as_ref().unwrap().cloud.started);
    assert!(rig.reply.as_ref().unwrap().pcm.is_empty());
    assert!(rig.draining);
    assert!(
        second
            .service
            .try_receive(Duration::from_millis(5))
            .await
            .is_none()
    );

    for (sequence, ended) in [(7, false), (8, true)] {
        let at = monotonic_us();
        let frame = ObservedAudio {
            sequence,
            captured_at_us: at,
            samples: [91; 160],
        };
        let context = AdmissionContext {
            capture,
            authority: rig.owner.snapshot(MonoTime::from_micros(at)).unwrap(),
        };
        let update = rig
            .admission
            .observe(
                if ended {
                    Activity::End(frame)
                } else {
                    Activity::Continue(frame)
                },
                context,
                at,
            )
            .unwrap();
        assert!(
            matches!(update.step, AdmissionStep::None),
            "discarded tail was re-admitted"
        );
    }
    assert!(rig.queue.is_empty());
    rig.draining = false;
    let at = monotonic_us();
    let context = AdmissionContext {
        capture,
        authority: rig.owner.snapshot(MonoTime::from_micros(at)).unwrap(),
    };
    let update = rig
        .admission
        .observe(
            Activity::Start(vec![ObservedAudio {
                sequence: 9,
                captured_at_us: at,
                samples: [17; 160],
            }]),
            context,
            at,
        )
        .unwrap();
    let AdmissionStep::Candidate(candidate) = update.step else {
        panic!("endpoint did not rearm")
    };
    assert!(matches!(
        rig.admission
            .decide(
                Evidence {
                    candidate: candidate.id,
                    through_sequence: candidate.trigger_sequence,
                    produced_at_us: at,
                    verdict: Verdict::Accept(AcceptanceBasis::DirectedSessionVadOnly),
                },
                context,
                at
            )
            .unwrap(),
        Decision::Accepted(_)
    ));
    // Only that new utterance may use B. The failed A command and tail never
    // create a provider output owner or spend B's input state.
    let old = rig.reply.as_ref().unwrap().owner;
    rig.owner.cancel_turn(now(), old).unwrap();
    let successor = rig.owner.admit(now(), AdmittedInput::NewTurn).unwrap();
    let current = rig.reply.as_mut().unwrap();
    current.owner = successor;
    current.plan = rig
        .owner
        .plan_output(now(), successor, OutputKind::Speech, None)
        .unwrap();
    current.cloud = CloudRequest::new(second.session.get()).unwrap();
    current.generation_done = false;
    current.provider_idle = false;
    current.local_input_ended = false;
    let mut recovered = CloudAvailability::default();
    recovered
        .observe(ready_link(3, second.session.get()), monotonic_us())
        .unwrap();
    rig.submit(&recovered, true);
    rig.until(true, |rig| rig.reply.as_ref().unwrap().cloud.started)
        .await;
    assert_eq!(
        rig.outputs.len(),
        2,
        "one refusal and one successor Started"
    );
    assert!(
        matches!(rig.outputs[1], ProviderOutput::Started { request, session, .. }
        if request == successor.turn() && session == second.session.get())
    );
    for field in ["activityStart", "audio", "activityEnd"] {
        assert!(
            second.service.receive().await["realtimeInput"]
                .get(field)
                .is_some()
        );
    }
    rig.stop().await;
}

#[tokio::test]
async fn paired_normal_start_and_answer_require_matching_started_and_finish_once() {
    let mut rig = Paired::new().await;
    let mut dial = rig.ready().await;
    let mut selected = CloudAvailability::default();
    selected
        .observe(ready_link(1, dial.session.get()), monotonic_us())
        .unwrap();
    rig.submit(&selected, true);
    rig.until(true, |rig| rig.reply.as_ref().unwrap().cloud.started)
        .await;
    for field in ["activityStart", "audio", "activityEnd"] {
        assert!(
            dial.service.receive().await["realtimeInput"]
                .get(field)
                .is_some()
        );
    }
    dial.service
        .burst([
            audio(97, 960),
            json!({"serverContent":{"generationComplete":true}}),
            json!({"serverContent":{"turnComplete":true,"interactionStatus":"IDLE"}}),
        ])
        .await;
    rig.until(true, |rig| {
        rig.reply.as_ref().unwrap().cloud.outcome.is_some()
    })
    .await;
    assert!(
        matches!(rig.outputs.first(), Some(ProviderOutput::Started { session, .. })
        if *session == dial.session.get())
    );
    assert_eq!(
        rig.outputs
            .iter()
            .filter(|event| matches!(event, ProviderOutput::TurnComplete { idle: true, .. }))
            .count(),
        1
    );
    assert!(
        !rig.outputs
            .iter()
            .any(|event| matches!(event, ProviderOutput::TurnFailed { .. }))
    );
    let current = rig.reply.as_mut().unwrap();
    assert_eq!(current.cloud.outcome, Some(RequestOutcome::Complete));
    assert!(current.cloud.input_end_submitted && current.cloud.started);
    let mut played = Vec::new();
    let mut final_chunks = 0;
    while let Some((samples, final_chunk)) = current.next_speaker_chunk() {
        played.extend(samples);
        final_chunks += usize::from(final_chunk);
    }
    assert_eq!(played, vec![97; 960]);
    assert_eq!(final_chunks, 1);
    assert_eq!(rig.cloud.ready_session(), Some(dial.session.get()));
    rig.stop().await;
}

#[tokio::test]
async fn paired_write_failure_before_started_preserves_both_input_stages_without_pcm() {
    for ended in [false, true] {
        let mut rig = Paired::with_capacity(16).await;
        let dial = rig.ready().await;
        let mut selected = CloudAvailability::default();
        selected
            .observe(ready_link(1, dial.session.get()), monotonic_us())
            .unwrap();
        rig.submit(&selected, ended);
        // Intentionally read nothing at the server: activityStart cannot fit
        // in 16 bytes. The real actor's finite write timeout produces failure.
        rig.until(true, |rig| {
            rig.reply.as_ref().unwrap().cloud.outcome.is_some()
        })
        .await;
        let expected = if ended {
            FailedStage::AwaitingResponse
        } else {
            FailedStage::InputOpen
        };
        assert!(
            matches!(rig.outputs.first(), Some(ProviderOutput::TurnFailed { stage, reason:Fault::WriteTimeout { .. }, .. }) if *stage == expected),
            "ended={ended} actual={:?}",
            rig.outputs
        );
        assert_eq!(
            rig.outputs
                .iter()
                .filter(|event| matches!(event, ProviderOutput::TurnFailed { .. }))
                .count(),
            1
        );
        assert!(!rig.outputs.iter().any(|event| matches!(
            event,
            ProviderOutput::Started { .. } | ProviderOutput::Audio { .. }
        )));
        let current = rig.reply.as_ref().unwrap();
        assert!(!current.cloud.started);
        assert!(current.cloud.require_output().is_err());
        assert!(current.pcm.is_empty());
        assert_eq!(current.delivery_outcome(), "provider_failed");
        assert_eq!(rig.draining, !ended);
        assert!(!rig.cloud.available());
        rig.stop().await;
        drop(dial);
    }
}

#[test]
fn correlated_link_trace_retains_oldest_and_upper_bounds_without_exact_latency_claim() {
    let mut link = ready_link(1, 1);
    link.observed_at_us = 100;
    link.source_clock = Some(crate::provider_link::SourceClockBounds {
        observed_upper_us: 150,
        outage_started_upper_us: None,
    });
    let trace = link_trace(link, 200);
    assert_eq!(trace["at_us"], 100);
    assert_eq!(trace["at_us_upper"], 150);
    assert_eq!(trace["at_us_role"], "source_interval_lower");
    assert_eq!(trace["coordinator_observed_at_us"], 200);
    assert_eq!(trace["link"]["source_clock"]["observed_upper_us"], 150);
}

/// Continuation and abandonment through the real hand-off: the coordinator's
/// own `cancel_reply`, authority and `flush_provider_input`, the Coordinated
/// worker and Supervisor over private IPC, and the real session against the
/// in-memory service. The worker is started as `--provider-continuation
/// kept-start` starts it.
mod continuation_or_discard {
    use super::*;
    use lamp_gemini::testing::sent_audio;

    const QUIET: Duration = Duration::from_millis(60);

    impl Paired {
        async fn kept() -> (Self, Dial, CloudAvailability) {
            let mut rig =
                Self::with(lamp_gemini::MAX_WIRE_BYTES * 2, Continuation::KeptStart).await;
            let dial = rig.ready().await;
            let mut selected = CloudAvailability::default();
            selected
                .observe(ready_link(1, dial.session.get()), monotonic_us())
                .unwrap();
            (rig, dial, selected)
        }
        /// A reply for a newly admitted owner, as the coordinator makes one.
        fn admit(&mut self, input: AdmittedInput, session: u64) -> u64 {
            let (_, mut reply, _) = active_reply();
            let turn = self.owner.admit(now(), input).unwrap();
            reply.owner = turn;
            reply.plan = self
                .owner
                .plan_output(now(), turn, OutputKind::Speech, None)
                .unwrap();
            reply.cloud = CloudRequest::new(session).unwrap();
            reply.local_input_ended = false;
            reply.playback = None;
            reply.pcm.clear();
            reply.had_audio = false;
            self.reply = Some(reply);
            self.publish();
            turn.turn()
        }
        /// The person speaks again: the coordinator cancels the current
        /// reply as a user interruption and admits a newer owner at once,
        /// with no authority published between the two.
        fn continued(&mut self, session: u64) -> u64 {
            cancel_reply(
                &mut self.owner,
                &mut self.reply,
                &mut self.trace,
                "user_interrupted",
            )
            .unwrap();
            self.queue.clear();
            self.admit(AdmittedInput::Interruption, session)
        }
        /// The coordinator gives the current request up: no owner follows.
        fn abandoned(&mut self, reason: &str) {
            cancel_reply(&mut self.owner, &mut self.reply, &mut self.trace, reason).unwrap();
            self.queue.clear();
            self.publish();
        }
        fn flush(&mut self, selected: &CloudAvailability) {
            flush_provider_input(
                selected,
                &mut self.reply,
                &mut self.queue,
                &mut self.channels.data,
            )
            .unwrap();
            assert!(self.queue.is_empty());
        }
        /// The current request's Start.
        fn start(&mut self, selected: &CloudAvailability) {
            let current = self.reply.as_ref().unwrap();
            let privacy_generation = self.owner.snapshot(now()).unwrap().microphone_generation();
            self.queue.push_back(ProviderInput::Start {
                request: current.owner.turn(),
                session: current.cloud.session,
                privacy_generation,
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
            });
            self.publish();
            self.flush(selected);
        }
        /// Blocks of 10 ms at about the rate they are spoken, each read now.
        async fn say(
            &mut self,
            selected: &CloudAvailability,
            blocks: std::ops::Range<u64>,
            value: i16,
        ) {
            for sequence in blocks {
                let request = self.reply.as_ref().unwrap().owner.turn();
                let privacy_generation =
                    self.owner.snapshot(now()).unwrap().microphone_generation();
                self.queue.push_back(ProviderInput::Audio {
                    request,
                    privacy_generation,
                    sequence,
                    read_completed_at_us: monotonic_us(),
                    samples: vec![value; 160],
                });
                self.flush(selected);
                for _ in 0..4 {
                    self.tick(true).await;
                }
            }
        }
        /// The current request's input ends.
        fn end(&mut self, selected: &CloudAvailability) {
            let current = self.reply.as_ref().unwrap();
            let privacy_generation = self.owner.snapshot(now()).unwrap().microphone_generation();
            self.queue.push_back(ProviderInput::End {
                request: current.owner.turn(),
                privacy_generation,
                sound: false,
                cut_in: false,
            });
            self.owner.input_ended(now(), current.owner).unwrap();
            self.reply.as_mut().unwrap().local_input_ended = true;
            self.publish();
            self.flush(selected);
        }
        /// What the service receives until it has received nothing for
        /// `quiet`: "start", "end", or an audio block's first sample.
        async fn heard(&mut self, dial: &mut Dial, quiet: Duration) -> Vec<String> {
            let mut all = Vec::new();
            let mut last = Instant::now();
            while last.elapsed() < quiet {
                self.tick(true).await;
                while let Some(message) = dial.service.try_receive(Duration::from_millis(1)).await {
                    last = Instant::now();
                    let input = &message["realtimeInput"];
                    all.push(if input.get("activityStart").is_some() {
                        "start".to_owned()
                    } else if input.get("activityEnd").is_some() {
                        "end".to_owned()
                    } else {
                        let samples = sent_audio(&message)
                            .unwrap_or_else(|| panic!("unexpected client message {message}"));
                        assert!(samples.iter().all(|sample| *sample == samples[0]));
                        samples[0].to_string()
                    });
                }
            }
            all
        }
        fn discards(&self) -> Vec<(u64, u64, u64)> {
            self.outputs
                .iter()
                .filter_map(|event| match event {
                    ProviderOutput::InputDiscarded {
                        request,
                        dropped_samples,
                        sent_samples,
                    } => Some((*request, *dropped_samples, *sent_samples)),
                    _ => None,
                })
                .collect()
        }
        /// The first request: committed, and the service has all of it.
        async fn first_committed(&mut self, dial: &mut Dial, selected: &CloudAvailability) -> u64 {
            let first = self.reply.as_ref().unwrap().owner.turn();
            self.submit(selected, true);
            self.until(true, |rig| rig.reply.as_ref().unwrap().cloud.started)
                .await;
            assert_eq!(self.heard(dial, QUIET).await, ["start", "73", "end"]);
            first
        }
    }
    fn of(value: i16, count: usize) -> Vec<String> {
        vec![value.to_string(); count]
    }
    fn interrupted() -> serde_json::Value {
        json!({"serverContent":{"interrupted":true}})
    }
    fn idle() -> serde_json::Value {
        json!({"serverContent":{"turnComplete":true,"interactionStatus":"IDLE"}})
    }

    #[tokio::test]
    async fn continued_twice_the_words_are_one_question_in_order_and_nothing_is_discarded() {
        let (mut rig, mut dial, selected) = Paired::kept().await;
        rig.first_committed(&mut dial, &selected).await;
        let session = dial.session.get();
        // The person goes on, and its input ends.
        rig.continued(session);
        rig.start(&selected);
        rig.say(&selected, 1..6, 10).await;
        rig.end(&selected);
        // And goes on again before anything was answered.
        let last = rig.continued(session);
        rig.start(&selected);
        rig.say(&selected, 1..6, 20).await;
        assert!(rig.heard(&mut dial, QUIET).await.is_empty());
        // The first answer begins.
        dial.service.send(audio(97, 240)).await;
        let mut expected = vec!["start".to_owned()];
        expected.extend(of(10, 5));
        expected.extend(of(20, 5));
        assert_eq!(rig.heard(&mut dial, QUIET).await, expected);
        dial.service.burst([interrupted(), idle()]).await;
        rig.end(&selected);
        assert_eq!(rig.heard(&mut dial, QUIET).await, ["end"]);
        dial.service.send(audio(98, 240)).await;
        rig.until(true, |rig| {
            rig.outputs.iter().any(
                |event| matches!(event, ProviderOutput::Audio { request, .. } if *request == last),
            )
        })
        .await;
        // Continued, never abandoned: nothing was discarded.
        assert!(rig.discards().is_empty());
        rig.stop().await;
    }

    #[tokio::test]
    async fn abandoned_while_kept_none_of_it_is_sent_and_the_next_question_is_heard_alone() {
        let (mut rig, mut dial, selected) = Paired::kept().await;
        rig.first_committed(&mut dial, &selected).await;
        let session = dial.session.get();
        let given_up = rig.continued(session);
        rig.start(&selected);
        rig.say(&selected, 1..6, 10).await;
        // The coordinator's own cancellation with no successor.
        rig.abandoned("deadline_cancelled");
        rig.until(true, |rig| !rig.discards().is_empty()).await;
        assert_eq!(rig.discards(), [(given_up, 5 * 160, 0)]);
        assert!(rig.heard(&mut dial, QUIET).await.is_empty());
        // The first answer begins late: interrupted, with nothing behind
        // the start. Its audio is nobody's.
        dial.service.send(audio(97, 240)).await;
        assert_eq!(rig.heard(&mut dial, QUIET).await, ["start"]);
        dial.service.burst([interrupted(), idle()]).await;
        assert!(
            rig.heard(&mut dial, Duration::from_millis(120))
                .await
                .is_empty()
        );
        let next = rig.admit(AdmittedInput::NewTurn, session);
        rig.start(&selected);
        rig.say(&selected, 1..4, 30).await;
        rig.end(&selected);
        let mut expected = of(30, 3);
        expected.push("end".to_owned());
        assert_eq!(rig.heard(&mut dial, QUIET).await, expected);
        dial.service.send(audio(98, 240)).await;
        rig.until(true, |rig| {
            rig.outputs.iter().any(
                |event| matches!(event, ProviderOutput::Audio { request, .. } if *request == next),
            )
        })
        .await;
        assert!(!rig.outputs.iter().any(
            |event| matches!(event, ProviderOutput::Audio { request, .. } if *request != next)
        ));
        rig.stop().await;
    }

    #[tokio::test]
    async fn continued_then_abandoned_then_a_new_question_only_the_new_one_is_sent() {
        let (mut rig, mut dial, selected) = Paired::kept().await;
        rig.first_committed(&mut dial, &selected).await;
        let session = dial.session.get();
        rig.continued(session);
        rig.start(&selected);
        rig.say(&selected, 1..4, 10).await;
        rig.end(&selected);
        let second = rig.continued(session);
        rig.start(&selected);
        rig.say(&selected, 1..4, 20).await;
        // The whole uncommitted thought is given up: both parts.
        rig.abandoned("playback_discarded");
        rig.until(true, |rig| !rig.discards().is_empty()).await;
        assert_eq!(rig.discards(), [(second, 6 * 160, 0)]);
        // A new question while the first answer still has not begun: kept.
        let next = rig.admit(AdmittedInput::NewTurn, session);
        rig.start(&selected);
        rig.say(&selected, 1..4, 30).await;
        assert!(rig.heard(&mut dial, QUIET).await.is_empty());
        dial.service.send(audio(97, 240)).await;
        let mut expected = vec!["start".to_owned()];
        expected.extend(of(30, 3));
        assert_eq!(rig.heard(&mut dial, QUIET).await, expected);
        dial.service.burst([interrupted(), idle()]).await;
        rig.end(&selected);
        assert_eq!(rig.heard(&mut dial, QUIET).await, ["end"]);
        dial.service.send(audio(98, 240)).await;
        rig.until(true, |rig| {
            rig.outputs.iter().any(
                |event| matches!(event, ProviderOutput::Audio { request, .. } if *request == next),
            )
        })
        .await;
        rig.stop().await;
    }

    /// The transport boundary through the whole hand-off. Abandoned after
    /// the kept words went out, nothing is left to drop and the outcome
    /// says sent, not discarded: the next question is heard after them.
    #[tokio::test]
    async fn abandoned_after_the_words_were_written_the_outcome_says_sent_and_not_discarded() {
        let (mut rig, mut dial, selected) = Paired::kept().await;
        rig.first_committed(&mut dial, &selected).await;
        let session = dial.session.get();
        let given_up = rig.continued(session);
        rig.start(&selected);
        rig.say(&selected, 1..6, 10).await;
        dial.service.send(audio(97, 240)).await;
        let mut expected = vec!["start".to_owned()];
        expected.extend(of(10, 5));
        assert_eq!(rig.heard(&mut dial, QUIET).await, expected);
        rig.abandoned("deadline_cancelled");
        rig.until(true, |rig| !rig.discards().is_empty()).await;
        assert_eq!(rig.discards(), [(given_up, 0, 5 * 160)]);
        dial.service.burst([interrupted(), idle()]).await;
        // Never committed by itself, and the connection is not abandoned.
        assert!(
            rig.heard(&mut dial, Duration::from_millis(120))
                .await
                .is_empty()
        );
        rig.admit(AdmittedInput::NewTurn, session);
        rig.start(&selected);
        rig.say(&selected, 1..3, 30).await;
        rig.end(&selected);
        let mut expected = of(30, 2);
        expected.push("end".to_owned());
        assert_eq!(rig.heard(&mut dial, QUIET).await, expected);
        assert_eq!(rig.cloud.ready_session(), Some(session));
        rig.stop().await;
    }

    #[tokio::test]
    async fn abandoned_with_no_first_answer_only_the_start_is_written_and_late_output_is_nobodys() {
        let (mut rig, mut dial, selected) = Paired::kept().await;
        rig.first_committed(&mut dial, &selected).await;
        let session = dial.session.get();
        let given_up = rig.continued(session);
        rig.start(&selected);
        rig.say(&selected, 1..6, 10).await;
        rig.abandoned("provider_interrupted");
        rig.until(true, |rig| !rig.discards().is_empty()).await;
        assert_eq!(rig.discards(), [(given_up, 5 * 160, 0)]);
        // No first answer and no terminal. After the patience the start is
        // written by itself, to stop an answer nobody wants.
        let kept_since = Instant::now();
        let mut wire = Vec::new();
        while wire.is_empty() {
            assert!(kept_since.elapsed() < Duration::from_secs(4));
            wire = rig.heard(&mut dial, QUIET).await;
        }
        assert_eq!(wire, ["start"]);
        // Output that comes after all that is nobody's.
        dial.service.send(audio(97, 240)).await;
        dial.service.burst([interrupted(), idle()]).await;
        assert!(
            rig.heard(&mut dial, Duration::from_millis(120))
                .await
                .is_empty()
        );
        assert!(
            !rig.outputs
                .iter()
                .any(|event| matches!(event, ProviderOutput::Audio { .. }))
        );
        rig.stop().await;
    }

    #[tokio::test]
    async fn abandoned_with_no_output_credit_it_is_dropped_at_once_and_reported_when_credit_returns()
     {
        let (mut rig, mut dial, selected) = Paired::kept().await;
        rig.first_committed(&mut dial, &selected).await;
        let session = dial.session.get();
        let given_up = rig.continued(session);
        rig.start(&selected);
        rig.say(&selected, 1..6, 10).await;
        // The coordinator takes outputs and returns no credit: the worker's
        // output is under pressure.
        for _ in 0..20 {
            rig.tick(false).await;
        }
        rig.abandoned("deadline_cancelled");
        for _ in 0..20 {
            rig.tick(false).await;
        }
        // Dropped in the transport whatever the worker can report: the
        // first answer finds only the start.
        dial.service.send(audio(97, 240)).await;
        let mut wire = Vec::new();
        for _ in 0..40 {
            rig.tick(false).await;
            while let Some(message) = dial.service.try_receive(Duration::from_millis(1)).await {
                wire.push(message);
            }
        }
        assert_eq!(wire.len(), 1);
        assert!(wire[0]["realtimeInput"].get("activityStart").is_some());
        // The report needs capacity like every packet, and is not lost.
        rig.until(true, |rig| !rig.discards().is_empty()).await;
        assert_eq!(rig.discards(), [(given_up, 5 * 160, 0)]);
        rig.stop().await;
    }

    /// A breath. The person stops for half a second inside the resumed
    /// question and goes on: one request, nothing lost, nothing timed out.
    #[tokio::test]
    async fn a_pause_for_breath_inside_a_kept_question_loses_nothing_and_times_nothing_out() {
        let (mut rig, mut dial, selected) = Paired::kept().await;
        rig.first_committed(&mut dial, &selected).await;
        let session = dial.session.get();
        let resumed = rig.continued(session);
        rig.start(&selected);
        rig.say(&selected, 1..4, 10).await;
        for _ in 0..200 {
            rig.tick(true).await;
        }
        rig.say(&selected, 4..7, 10).await;
        assert!(rig.heard(&mut dial, QUIET).await.is_empty());
        dial.service.send(audio(97, 240)).await;
        let mut expected = vec!["start".to_owned()];
        expected.extend(of(10, 6));
        assert_eq!(rig.heard(&mut dial, QUIET).await, expected);
        dial.service.burst([interrupted(), idle()]).await;
        rig.end(&selected);
        assert_eq!(rig.heard(&mut dial, QUIET).await, ["end"]);
        dial.service.send(audio(98, 240)).await;
        rig.until(true, |rig| {
            rig.outputs.iter().any(
                |event| matches!(event, ProviderOutput::Audio { request, .. } if *request == resumed),
            )
        })
        .await;
        assert!(rig.discards().is_empty());
        rig.stop().await;
    }

    /// A change of subject while the first answer is already being spoken:
    /// the ordinary interruption. With the option on nothing is kept,
    /// because there is something to interrupt, and it behaves as before.
    #[tokio::test]
    async fn interrupting_an_answer_that_has_begun_keeps_nothing_and_is_the_ordinary_interruption()
    {
        let (mut rig, mut dial, selected) = Paired::kept().await;
        let first = rig.first_committed(&mut dial, &selected).await;
        let session = dial.session.get();
        dial.service.send(audio(97, 240)).await;
        rig.until(true, |rig| {
            rig.outputs.iter().any(
                |event| matches!(event, ProviderOutput::Audio { request, .. } if *request == first),
            )
        })
        .await;
        let changed = rig.continued(session);
        rig.start(&selected);
        rig.say(&selected, 1..4, 10).await;
        // Written at once: the start and the words as they are spoken.
        let mut expected = vec!["start".to_owned()];
        expected.extend(of(10, 3));
        assert_eq!(rig.heard(&mut dial, QUIET).await, expected);
        // More of the old answer, late: nobody's.
        dial.service.send(audio(97, 240)).await;
        dial.service.burst([interrupted(), idle()]).await;
        rig.end(&selected);
        assert_eq!(rig.heard(&mut dial, QUIET).await, ["end"]);
        dial.service.send(audio(98, 240)).await;
        rig.until(true, |rig| {
            rig.outputs.iter().any(
                |event| matches!(event, ProviderOutput::Audio { request, .. } if *request == changed),
            )
        })
        .await;
        // One block of the first answer reached the coordinator, before it
        // was interrupted, and none after.
        assert_eq!(
            rig.outputs
                .iter()
                .filter(
                    |event| matches!(event, ProviderOutput::Audio { request, .. } if *request == first)
                )
                .count(),
            1
        );
        assert!(rig.discards().is_empty());
        rig.stop().await;
    }

    /// Authority changes that the worker reads in one turn of its loop. An
    /// abandonment followed at once by a new question and its continuation
    /// must still be an abandonment of what came before.
    #[tokio::test]
    async fn an_abandonment_is_not_lost_when_a_new_question_and_its_continuation_follow_at_once() {
        let (mut rig, mut dial, selected) = Paired::kept().await;
        rig.first_committed(&mut dial, &selected).await;
        let session = dial.session.get();
        let given_up = rig.continued(session);
        rig.start(&selected);
        rig.say(&selected, 1..6, 10).await;
        assert!(rig.heard(&mut dial, QUIET).await.is_empty());
        // Three authority packets with no turn of the worker between them:
        // no owner, a new question, and that question continued.
        rig.abandoned("deadline_cancelled");
        rig.admit(AdmittedInput::NewTurn, session);
        let last = rig.continued(session);
        rig.start(&selected);
        rig.say(&selected, 1..4, 30).await;
        rig.until(true, |rig| !rig.discards().is_empty()).await;
        assert_eq!(rig.discards(), [(given_up, 5 * 160, 0)]);
        // The first answer begins: the start, and the new question alone.
        dial.service.send(audio(97, 240)).await;
        let mut expected = vec!["start".to_owned()];
        expected.extend(of(30, 3));
        assert_eq!(rig.heard(&mut dial, QUIET).await, expected);
        dial.service.burst([interrupted(), idle()]).await;
        rig.end(&selected);
        assert_eq!(rig.heard(&mut dial, QUIET).await, ["end"]);
        dial.service.send(audio(98, 240)).await;
        rig.until(true, |rig| {
            rig.outputs.iter().any(
                |event| matches!(event, ProviderOutput::Audio { request, .. } if *request == last),
            )
        })
        .await;
        rig.stop().await;
    }

    #[tokio::test]
    async fn a_mute_while_the_question_is_kept_writes_none_of_it_and_ends_cleanly() {
        let (mut rig, mut dial, selected) = Paired::kept().await;
        rig.first_committed(&mut dial, &selected).await;
        let session = dial.session.get();
        rig.continued(session);
        rig.start(&selected);
        rig.say(&selected, 1..6, 10).await;
        assert!(rig.heard(&mut dial, QUIET).await.is_empty());
        // The microphone is closed: authority says so once, and there is no
        // lease to renew afterwards.
        rig.owner
            .set_microphone_permission(now(), Permission::Denied)
            .unwrap();
        let snapshot = rig.owner.snapshot(now()).unwrap();
        rig.channels
            .control
            .send(Control::Authority { snapshot })
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), rig.worker.take().unwrap())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        // Kept, then dropped with the session: not a start, not a block.
        assert!(
            dial.service
                .try_receive(Duration::from_millis(50))
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_stop_after_an_abandonment_writes_nothing_and_ends_cleanly() {
        let (mut rig, mut dial, selected) = Paired::kept().await;
        rig.first_committed(&mut dial, &selected).await;
        let session = dial.session.get();
        rig.continued(session);
        rig.start(&selected);
        rig.say(&selected, 1..6, 10).await;
        rig.abandoned("service_stopped");
        rig.until(true, |rig| !rig.discards().is_empty()).await;
        rig.stop().await;
        assert!(
            dial.service
                .try_receive(Duration::from_millis(50))
                .await
                .is_none()
        );
    }

    /// The transport remembers what each request sent into an activity that
    /// has no End, so that a late abandonment reports its own audio, and
    /// that memory is bounded. As many continuations in a row as it holds,
    /// none committed and none given up, are all heard in order. One more
    /// ends the provider with its local fault, and none of that one's words
    /// are written.
    #[tokio::test]
    async fn one_continuation_more_than_the_transport_remembers_ends_the_provider_with_its_local_fault()
     {
        let (mut rig, mut dial, selected) = Paired::kept().await;
        let first = rig.first_committed(&mut dial, &selected).await;
        let session = dial.session.get();
        // The first answer has begun, so nothing is kept from here on: each
        // continuation is written as it is spoken, into one open activity.
        dial.service.send(audio(97, 240)).await;
        rig.until(true, |rig| {
            rig.outputs.iter().any(
                |event| matches!(event, ProviderOutput::Audio { request, .. } if *request == first),
            )
        })
        .await;
        let bound = lamp_gemini::MAX_OPEN_ACTIVITY_REQUESTS as i16;
        let mut expected = vec!["start".to_owned()];
        let mut heard = Vec::new();
        for value in 1..=bound {
            rig.continued(session);
            rig.start(&selected);
            rig.say(&selected, 1..2, value).await;
            expected.push(value.to_string());
            if value == 1 {
                heard.extend(rig.heard(&mut dial, QUIET).await);
                dial.service.burst([interrupted(), idle()]).await;
            }
        }
        heard.extend(rig.heard(&mut dial, QUIET).await);
        assert_eq!(heard, expected);
        assert!(rig.discards().is_empty());
        assert!(
            !rig.outputs
                .iter()
                .any(|event| matches!(event, ProviderOutput::Ended { .. }))
        );
        // One more than is remembered.
        rig.continued(session);
        rig.start(&selected);
        rig.say(&selected, 1..2, -1).await;
        rig.until(true, |rig| {
            rig.outputs
                .iter()
                .any(|event| matches!(event, ProviderOutput::Ended { .. }))
        })
        .await;
        let ended: Vec<_> = rig
            .outputs
            .iter()
            .filter(|event| matches!(event, ProviderOutput::Ended { .. }))
            .collect();
        assert!(
            matches!(
                ended[..],
                [ProviderOutput::Ended {
                    fault: Fault::Local,
                    attempts: 0,
                    ..
                }]
            ),
            "{ended:?}"
        );
        assert!(rig.heard(&mut dial, QUIET).await.is_empty());
        assert!(rig.discards().is_empty());
    }
}

/// A duplex interruption end to end: the coordinator's own hearing,
/// admission and `commit_admitted_input` for a person speaking over the
/// lamp, its provider queue flushed by `flush_provider_input` to the real
/// Coordinated worker and Supervisor, and the real session against the
/// in-memory service. The capture blocks and their evidence are scripted;
/// nothing here is a microphone or a speaker.
mod duplex_through_the_worker {
    use super::super::duplex::{Harness, OVER, OWN, evidence};
    use super::*;
    use lamp_gemini::testing::sent_audio;

    struct Link {
        _directory: SessionDirectory,
        channels: WorkerChannels,
        cloud: CloudAvailability,
        flow: OutputReceiver,
        trace: SessionState,
        outputs: Vec<ProviderOutput>,
        dials: UnboundedReceiver<Dial>,
        worker: Option<JoinHandle<Result<()>>>,
    }
    impl Drop for Link {
        fn drop(&mut self) {
            if let Some(worker) = self.worker.take() {
                worker.abort();
            }
        }
    }
    impl Link {
        /// The worker for the harness's own controller, started as
        /// `--provider-continuation` starts it.
        async fn start(
            h: &mut Harness,
            continuation: Continuation,
        ) -> (Self, Dial, CloudAvailability) {
            let directory = SessionDirectory::create().unwrap();
            let (channels, child) = pair(&directory, "provider");
            let boot = h.owner.snapshot(now()).unwrap().boot();
            let config = continuation
                .apply(
                    SessionConfig::new(
                        lamp_gemini::GOOGLE_ENDPOINT,
                        Credential::api_key("test-only").unwrap(),
                    )
                    .unwrap()
                    .resumption(Resumption::Retain)
                    .barrier_policy(BarrierPolicy::Require),
                )
                .unwrap();
            let (sender, dials) = tokio::sync::mpsc::unbounded_channel();
            let provider = Supervisor::new(
                ScriptedConnector {
                    config,
                    dials: sender,
                    capacity: lamp_gemini::MAX_WIRE_BYTES * 2,
                },
                RecoveryPolicy {
                    first_backoff: Duration::from_millis(20),
                    max_backoff: Duration::from_millis(40),
                    ..RecoveryPolicy::default()
                },
            )
            .unwrap();
            let mut link = Self {
                _directory: directory,
                channels,
                cloud: CloudAvailability::default(),
                flow: OutputReceiver::default(),
                trace: SessionState::default(),
                outputs: Vec::new(),
                dials,
                worker: Some(tokio::spawn(serve_coordinated_test(child, boot, provider))),
            };
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                link.publish(h);
                if let Some(event) = link.channels.control.receive::<WorkerEvent>().unwrap() {
                    assert!(matches!(event, WorkerEvent::Ready));
                    break;
                }
                assert!(Instant::now() < deadline, "worker never became ready");
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            let mut dial = loop {
                if let Ok(dial) = link.dials.try_recv() {
                    break dial;
                }
                assert!(Instant::now() < deadline, "worker never dialled");
                link.tick(h).await;
            };
            dial.service.accept().await;
            while !link.cloud.available() {
                assert!(Instant::now() < deadline, "link never became available");
                link.tick(h).await;
            }
            let mut selected = CloudAvailability::default();
            selected
                .observe(ready_link(1, dial.session.get()), monotonic_us())
                .unwrap();
            (link, dial, selected)
        }
        /// Fresh leases and authority from the harness's controller.
        fn publish(&mut self, h: &mut Harness) {
            let at = now();
            let until = at.checked_add(INPUT_LEASE_US).unwrap();
            h.owner
                .set_capture(at, CaptureState::RetainingUntil(until))
                .unwrap();
            h.owner
                .set_admission(at, AdmissionState::OpenUntil(until))
                .unwrap();
            let snapshot = h.owner.snapshot(at).unwrap();
            h.snapshot = snapshot;
            self.channels
                .control
                .send(Control::Authority { snapshot })
                .unwrap();
        }
        async fn tick(&mut self, h: &mut Harness) {
            self.publish(h);
            assert!(
                service_provider_control(
                    &mut self.channels.control,
                    &mut self.cloud,
                    &mut 16,
                    &mut self.trace,
                )
                .unwrap()
            );
            let mut slice = ProviderSlice::new(monotonic_us());
            while let Some(event) = slice
                .receive(&mut self.channels.data, monotonic_us())
                .unwrap()
            {
                self.flow.received().unwrap();
                self.outputs.push(event);
            }
            self.flow.replenish(0, &mut self.channels.control).unwrap();
            assert!(!self.worker.as_ref().unwrap().is_finished(), "worker ended");
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        /// The coordinator's own flush of the harness's provider queue.
        async fn flush(&mut self, h: &mut Harness, selected: &CloudAvailability) {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !h.provider_queue.is_empty() {
                assert!(Instant::now() < deadline, "provider queue never drained");
                self.publish(h);
                flush_provider_input(
                    selected,
                    &mut h.reply,
                    &mut h.provider_queue,
                    &mut self.channels.data,
                )
                .unwrap();
                self.tick(h).await;
            }
        }
        async fn heard(
            &mut self,
            h: &mut Harness,
            dial: &mut Dial,
            quiet: Duration,
        ) -> Vec<String> {
            let mut all = Vec::new();
            let mut last = Instant::now();
            while last.elapsed() < quiet {
                self.tick(h).await;
                while let Some(message) = dial.service.try_receive(Duration::from_millis(1)).await {
                    last = Instant::now();
                    let input = &message["realtimeInput"];
                    all.push(if input.get("activityStart").is_some() {
                        "start".to_owned()
                    } else if input.get("activityEnd").is_some() {
                        "end".to_owned()
                    } else {
                        let samples = sent_audio(&message)
                            .unwrap_or_else(|| panic!("unexpected client message {message}"));
                        samples[0].to_string()
                    });
                }
            }
            all
        }
        /// The same, but not before `count` messages have arrived: a host
        /// that stalls for longer than `quiet` in the middle of a burst
        /// must not be mistaken for the end of it.
        async fn heard_all(
            &mut self,
            h: &mut Harness,
            dial: &mut Dial,
            count: usize,
            quiet: Duration,
        ) -> Vec<String> {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut all = Vec::new();
            while all.len() < count && Instant::now() < deadline {
                all.extend(self.heard(h, dial, quiet).await);
            }
            all
        }
        async fn until_output(
            &mut self,
            h: &mut Harness,
            wanted: impl Fn(&ProviderOutput) -> bool,
        ) {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !self.outputs.iter().any(&wanted) {
                assert!(
                    Instant::now() < deadline,
                    "expected provider output never came"
                );
                self.tick(h).await;
            }
        }
    }
    const QUIET: Duration = Duration::from_millis(60);

    #[tokio::test]
    async fn a_person_speaking_over_the_lamp_reaches_the_service_as_one_question_with_their_opening_words()
     {
        let mut h = Harness::new(TurnTaking::Duplex, 400);
        let (mut link, mut dial, selected) = Link::start(&mut h, Continuation::KeptStart).await;
        let old = h.old().turn();
        let session = dial.session.get();
        // The reply the lamp is speaking, as the worker knows it: its
        // request committed, and its answer begun.
        let privacy_generation = h.snapshot.microphone_generation();
        link.publish(&mut h);
        for command in [
            ProviderInput::Start {
                request: old,
                session,
                privacy_generation,
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
            },
            ProviderInput::Audio {
                request: old,
                privacy_generation,
                sequence: 1,
                read_completed_at_us: monotonic_us(),
                samples: vec![-5; 160],
            },
            ProviderInput::End {
                request: old,
                privacy_generation,
                sound: false,
                cut_in: false,
            },
        ] {
            link.channels.data.send(command).unwrap();
        }
        assert_eq!(
            link.heard(&mut h, &mut dial, QUIET).await,
            ["start", "-5", "end"]
        );
        dial.service.send(audio(97, 240)).await;
        link.until_output(
            &mut h,
            |event| matches!(event, ProviderOutput::Audio { request, .. } if *request == old),
        )
        .await;
        // The lamp speaks; a person speaks over it; the lamp's sound dies
        // away while they go on; then they are speaking in the clear. All of
        // it through the coordinator's own hearing and admission.
        h.blocks(20, 0.99, Some(evidence(0, OWN)));
        h.blocks(9, 0.9, Some(evidence(0, OVER)));
        for quiet in 1..=29 {
            h.block(0.9, Some(evidence(quiet, OVER)));
        }
        let clear = h.blocks(6, 0.9, Some(evidence(30, None)));
        let accepted = clear[5]
            .accepted
            .as_ref()
            .expect("the person was not let in");
        assert_eq!(accepted.basis, AcceptanceBasis::DirectedSessionDuplex);
        assert_eq!(h.events("duplex_commit").len(), 1);
        let finished = h.events("turn_finished");
        assert_eq!(finished[0]["outcome"], "user_interrupted");
        let question = h.old().turn();
        assert!(question > old);
        let opening: Vec<String> = accepted.audio.iter().map(u64::to_string).collect();
        assert!(opening.len() > 6, "{opening:?}");
        // They go on without a pause: capture is continuous, and each
        // further block goes straight into the request.
        let last = *accepted.audio.last().unwrap();
        h.blocks(5, 0.9, Some(evidence(40, None)));
        // The coordinator's own queue for the new request, flushed to the
        // worker: the answer has begun, so nothing is kept and the old
        // answer is interrupted at once. The opening words, spoken over the
        // lamp, and what followed arrive whole and in order.
        link.flush(&mut h, &selected).await;
        let mut expected = vec!["start".to_owned()];
        expected.extend(opening.iter().cloned());
        expected.extend((last + 1..=last + 5).map(|sequence| sequence.to_string()));
        let heard = link
            .heard_all(&mut h, &mut dial, expected.len(), QUIET)
            .await;
        assert_eq!(heard, expected, "provider outputs: {:?}", link.outputs);
        dial.service
            .burst([
                json!({"serverContent":{"interrupted":true}}),
                json!({"serverContent":{"turnComplete":true,"interactionStatus":"IDLE"}}),
            ])
            .await;
        // And stop.
        let owner = h.old();
        h.owner.input_ended(now(), owner).unwrap();
        h.reply.as_mut().unwrap().local_input_ended = true;
        h.provider_queue.push_back(ProviderInput::End {
            request: question,
            privacy_generation: h.snapshot.microphone_generation(),
            sound: false,
            cut_in: false,
        });
        link.flush(&mut h, &selected).await;
        assert_eq!(link.heard(&mut h, &mut dial, QUIET).await, ["end"]);
        // The answer that follows is the new question's, and nothing of the
        // old answer reached the coordinator after the interruption.
        dial.service.send(audio(98, 240)).await;
        link.until_output(
            &mut h,
            |event| matches!(event, ProviderOutput::Audio { request, .. } if *request == question),
        )
        .await;
        assert_eq!(
            link.outputs
                .iter()
                .filter(|event| matches!(event, ProviderOutput::Audio { request, .. } if *request == old))
                .count(),
            1
        );
        assert!(
            !link
                .outputs
                .iter()
                .any(|event| matches!(event, ProviderOutput::InputDiscarded { .. }))
        );
    }
}
