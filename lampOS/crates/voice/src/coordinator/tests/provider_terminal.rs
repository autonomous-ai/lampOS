//! Production coordinator with synthetic owned children and real local IPC.
//! The peers run in the test thread under the stepped scenario clock
//! (`crate::scenario_clock`), as `second_route` does, so a busy host cannot
//! starve the privacy peer past the 50 ms freshness bound (lampOS#975).
//! No hardware, model connection or acoustic timing is involved.
use super::*;
use crate::{
    provider_flow::OutputSender,
    provider_link::LinkFailure,
    provider_worker::{FailedStage, Fault},
    wire::{CaptureTiming, ReferenceTiming},
};
use lamp_audio::ownership::PlaybackCursor;
use std::{cell::RefCell, rc::Rc};

/// A host safety net only: scenario time bounds the run below.
const WATCHDOG: Duration = Duration::from_secs(24);

#[derive(Clone, Copy, Debug)]
enum Case {
    Idle,
    AwaitingAnswer,
    CompleteBuffered,
    FailedBuffered,
    RefusalAfterEnded,
    OpenInput,
    Holding { capture_gap: bool },
}

struct Peer {
    role: &'static str,
    channels: WorkerChannels,
    input: Option<std::process::ChildStdin>,
    child_pid: u32,
    snapshot: Option<Snapshot>,
    stopped: bool,
}
struct Evidence {
    peers: Vec<Peer>,
    played: Vec<i16>,
    provider_starts: usize,
    provider_ends: usize,
    capture_blocks: u64,
    written: Vec<(u64, u64, String)>,
    retired_at_us: Option<u64>,
    capture_gap_at_us: Option<u64>,
    last_capture_us: Option<u64>,
}

fn ended() -> ProviderOutput {
    ended_output()
}

fn run(case: Case) -> (SessionState, Evidence) {
    run_with_notices(case, &FailureNotices::none())
}

fn run_with_notices(case: Case, notices: &FailureNotices) -> (SessionState, Evidence) {
    run_with_notice_policy(case, notices, TurnTaking::VadOnly)
}

fn run_with_notice_policy(
    case: Case,
    notices: &FailureNotices,
    policy: TurnTaking,
) -> (SessionState, Evidence) {
    let holding = matches!(case, Case::Holding { .. });
    // The real loop reads this thread-local test switch, as it does for
    // the misheard hum. Restore it even if a fixture assertion panics.
    struct HoldingSwitch(bool);
    impl Drop for HoldingSwitch {
        fn drop(&mut self) {
            TEST_HOLDING_ANSWER.with(|enabled| enabled.set(self.0));
        }
    }
    let _holding_switch =
        holding.then(|| HoldingSwitch(TEST_HOLDING_ANSWER.with(|enabled| enabled.replace(true))));
    let maximum_played = 480
        + notices.capability()["available"]["frames"]
            .as_u64()
            .unwrap_or(0) as usize;
    let directory = SessionDirectory::create().unwrap();
    let mut peers = Vec::new();
    let mut spawn = |role| {
        let (worker, channels, input) = Worker::cooperative_ready_fixture(&directory.path, role);
        peers.push(Peer {
            role,
            channels,
            input: Some(input),
            child_pid: worker.fixture_pid(),
            snapshot: None,
            stopped: false,
        });
        worker
    };
    let mut rig = Rig {
        expression_out: None,
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        early_finalize: false,
        faint_level: None,
        speaker: spawn("speaker"),
        capture: spawn("capture"),
        provider: spawn("provider"),
        privacy: spawn("privacy"),
        privacy_controls: false,
        ring: None.into(),
        camera: None,
        motor: None,
        ring_channel_ceiling: None,
        diagnostic_directory: None,
        noise_suppression: NoiseSuppression::On,
        turn_taking: policy,
        playout_start: PlayoutStart::default(),
        fixture_at_ms: None,
        endpoint: None,
        call_wait_blocks: None,
        cut_keeps_name: false,
        asker_first: crate::asker_first::Mode::Off,
        follows_on: false,
        paused: Box::new(|| false),
    };
    // The clock starts once every child has crossed its startup barrier;
    // the coordinator and the peers share it from here.
    let clock_scope = crate::scenario_clock::Scope::enter(WATCHDOG).unwrap();
    let (mut owner, mut reply) = if matches!(case, Case::Idle | Case::OpenInput) {
        (Controller::new(new_boot().unwrap(), now()), None)
    } else {
        let (mut owner, mut reply, _) = active_reply();
        if matches!(
            case,
            Case::AwaitingAnswer
                | Case::RefusalAfterEnded
                | Case::CompleteBuffered
                | Case::FailedBuffered
                | Case::Holding { .. }
        ) {
            owner
                .playback_ended(now(), reply.playback.take().unwrap())
                .unwrap();
            reply.pcm.clear();
            reply.had_audio = false;
        }
        if matches!(case, Case::RefusalAfterEnded) {
            reply.cloud = CloudRequest::new(1).unwrap();
            reply
                .cloud
                .submitted(&ProviderInput::Start {
                    request: reply.owner.turn(),
                    session: 1,
                    privacy_generation: 2,
                    continues: false,
                    continues_cut_off: false,
                    over_the_lamp: false,
                    after_stop: false,
                    follows_on: false,
                    over_the_lamps_voice: false,
                })
                .unwrap();
        }
        (owner, Some(reply))
    };
    let request = reply.as_ref().map_or(1, |r| r.owner.turn());
    let mut startup = StartupEvents::default();
    startup
        .cloud
        .observe(ready_link(1, 1), monotonic_us())
        .unwrap();
    let shared = Rc::new(RefCell::new(Evidence {
        peers,
        played: Vec::new(),
        provider_starts: 0,
        provider_ends: 0,
        capture_blocks: 0,
        written: Vec::new(),
        retired_at_us: None,
        capture_gap_at_us: None,
        last_capture_us: None,
    }));
    {
        let shared = shared.clone();
        let mut output = OutputSender::default();
        let mut queued = VecDeque::new();
        match case {
            Case::Idle => queued.push_back(ended()),
            Case::AwaitingAnswer => {
                queued.push_back(ProviderOutput::TurnFailed {
                    request,
                    stage: FailedStage::AwaitingResponse,
                    reason: Fault::Transport,
                });
                queued.push_back(ended());
            }
            Case::CompleteBuffered => {
                queued.push_back(provider_audio(1, vec![123; 480]));
                queued.push_back(ProviderOutput::TurnComplete {
                    request,
                    idle: true,
                });
                queued.push_back(ended());
            }
            Case::FailedBuffered => {
                queued.push_back(provider_audio(1, vec![123; 480]));
                queued.push_back(ProviderOutput::TurnFailed {
                    request,
                    stage: FailedStage::Responding,
                    reason: Fault::Transport,
                });
                queued.push_back(ended());
            }
            Case::RefusalAfterEnded => {
                queued.push_back(ended());
                queued.push_back(ProviderOutput::TurnFailed {
                    request,
                    stage: FailedStage::NotDelivered,
                    reason: Fault::Transport,
                });
            }
            Case::OpenInput => {}
            Case::Holding { .. } => {
                queued.push_back(provider_audio(1, vec![123; 480]));
                queued.push_back(ProviderOutput::HoldingAnswer {
                    request,
                    session: 1,
                });
                queued.push_back(ProviderOutput::TurnComplete {
                    request,
                    idle: true,
                });
            }
        }
        let started = crate::scenario_clock::instant();
        let mut guard_sent = false;
        let mut terminal_sent = false;
        let mut capture_started = false;
        let mut capture_epoch = 1;
        let mut capture_sequence = 0;
        let mut next_capture = started;
        let mut next_privacy = started;
        let mut cursor = 0u64;
        let pump = move |phase| -> io::Result<()> {
            // A real child's exit is not simulated: one deadline bounds the
            // children a step stops, and shutdown brings its own budget.
            let deadline = match phase {
                crate::scenario_clock::Phase::Run => Instant::now() + Duration::from_millis(300),
                crate::scenario_clock::Phase::Shutdown { deadline } => deadline,
            };
            let evidence = &mut *shared.borrow_mut();
            let elapsed = crate::scenario_clock::instant() - started;
            assert!(
                elapsed < Duration::from_secs(4),
                "synthetic terminal test bounded cleanup"
            );
            for peer in &mut evidence.peers {
                if peer.stopped {
                    continue;
                }
                for _ in 0..16 {
                    match peer.channels.control.receive::<Control>().unwrap() {
                        Some(Control::Authority { snapshot }) => peer.snapshot = Some(snapshot),
                        Some(Control::ProviderOutputCapacity { through }) => {
                            output.grant(through).unwrap()
                        }
                        Some(Control::StartCapture) => {
                            assert_eq!(peer.role, "capture");
                            assert!(!capture_started);
                            capture_started = true;
                            let at = monotonic_us();
                            peer.channels
                                .control
                                .send(WorkerEvent::CaptureStarted {
                                    epoch: 1,
                                    dsp_epoch: 1,
                                    noise_suppression: NoiseSuppression::On,
                                    privacy_generation: peer
                                        .snapshot
                                        .unwrap()
                                        .microphone_generation(),
                                    rate: 16000,
                                    channels: 1,
                                    period_frames: 160,
                                    buffer_frames: 640,
                                    monotonic_status_timestamps: false,
                                    open_started_at_us: at,
                                    prepared_at_us: at,
                                    capture_started_at_us: at,
                                    start_completed_at_us: at,
                                })
                                .unwrap();
                        }
                        Some(Control::Stop) => {
                            peer.stopped = true;
                            // The child exits once its input closes; logical
                            // time does not pass in a shutdown step.
                            if peer.input.take().is_some() {
                                // Bounded by the scope's own watchdog, not the
                                // cleanup's real deadline, which a busy host
                                // can use up first (#1235's review).
                                let bound =
                                    crate::scenario_clock::host_deadline().unwrap_or(deadline);
                                crate::scenario_clock::wait_child_exited(peer.child_pid, bound)?;
                            }
                            break;
                        }
                        Some(command) => panic!("unexpected test control {command:?}"),
                        None => break,
                    }
                }
                if peer.stopped {
                    continue;
                }
                match peer.role {
                    "privacy" if crate::scenario_clock::instant() >= next_privacy => {
                        peer.channels
                            .control
                            .send(WorkerEvent::Privacy {
                                receipt: None,
                                acquired_at_us: monotonic_us(),
                                muted: false,
                            })
                            .unwrap();
                        next_privacy = crate::scenario_clock::instant() + Duration::from_millis(5);
                    }
                    "capture"
                        if capture_started && crate::scenario_clock::instant() >= next_capture =>
                    {
                        let at = monotonic_us();
                        if matches!(case, Case::Holding { capture_gap: true })
                            && evidence.capture_gap_at_us.is_none()
                            && evidence
                                .retired_at_us
                                .is_some_and(|retired| at - retired >= 500_000)
                        {
                            // The live capture-control drain, then the next
                            // loop's gap cancellation, must clear the promise.
                            // Do not call Continuation::clear from the fixture.
                            peer.channels
                                .control
                                .send(WorkerEvent::CaptureGap {
                                    epoch: capture_epoch,
                                    reason: "AuthorityExpired".into(),
                                    closed_at_us: at,
                                })
                                .unwrap();
                            capture_epoch += 1;
                            capture_sequence = 0;
                            let mut reopened = super::capture_started(NoiseSuppression::On);
                            if let WorkerEvent::CaptureStarted {
                                epoch,
                                privacy_generation,
                                ..
                            } = &mut reopened
                            {
                                *epoch = capture_epoch;
                                *privacy_generation =
                                    peer.snapshot.unwrap().microphone_generation();
                            }
                            peer.channels.control.send(reopened).unwrap();
                            evidence.capture_gap_at_us = Some(at);
                        }
                        evidence.capture_blocks += 1;
                        capture_sequence += 1;
                        evidence.last_capture_us = Some(at);
                        let probability =
                            if matches!(case, Case::OpenInput) && evidence.capture_blocks <= 16 {
                                0.99
                            } else {
                                0.0
                            };
                        peer.channels
                            .data
                            .send(MicrophoneFrame {
                                raw: None,
                                linear: None,
                                epoch: capture_epoch,
                                dsp_epoch: 1,
                                privacy_generation: peer.snapshot.unwrap().microphone_generation(),
                                frame_sequence: capture_sequence,
                                read_completed_at_us: at,
                                probability,
                                echo: None,
                                reference: Default::default(),
                                samples: vec![7; 160],
                                timing: CaptureTiming {
                                    first_read_started_at_us: at,
                                    last_read_started_at_us: at,
                                    successful_reads: 1,
                                    alsa_status_monotonic_us: None,
                                    status_observed_at_us: at,
                                    available_frames: 0,
                                    delayed_frames: 0,
                                    processing_completed_at_us: at,
                                    aec_queue_delay_ms: 0,
                                    reference: ReferenceTiming {
                                        playback_epoch: 1,
                                        clock_started_at_us: Some(at),
                                        ..ReferenceTiming::default()
                                    },
                                },
                            })
                            .unwrap();
                        next_capture = crate::scenario_clock::instant() + Duration::from_millis(10);
                    }
                    "provider" => {
                        for _ in 0..8 {
                            match peer.channels.data.receive::<ProviderInput>().unwrap() {
                                Some(ProviderInput::Start { request, .. }) => {
                                    evidence.provider_starts += 1;
                                    assert!(matches!(case, Case::OpenInput));
                                    queued.push_back(ended());
                                    queued.push_back(ProviderOutput::TurnFailed {
                                        request,
                                        stage: FailedStage::NotDelivered,
                                        reason: Fault::Transport,
                                    });
                                }
                                Some(ProviderInput::End { .. }) => evidence.provider_ends += 1,
                                Some(ProviderInput::Reminder { .. }) => {
                                    panic!("reminders are disabled in this fixture")
                                }
                                Some(ProviderInput::Written {
                                    request,
                                    session,
                                    text,
                                    ..
                                }) if holding => {
                                    evidence.written.push((request, session, text));
                                    queued.push_back(ProviderOutput::Started {
                                        request,
                                        session,
                                        waiting_for_barrier: false,
                                    });
                                    queued.push_back(ProviderOutput::TurnComplete {
                                        request,
                                        idle: true,
                                    });
                                }
                                Some(
                                    ProviderInput::Audio { .. }
                                    | ProviderInput::Written { .. }
                                    | ProviderInput::Called { .. }
                                    | ProviderInput::Paused { .. },
                                ) => {}
                                None => break,
                            }
                        }
                        // Keep the provider available through the holding
                        // opportunity: 0.5 s to the gap, then over 1 s of
                        // fresh quiet. Only afterwards end this service fixture.
                        let holding_done = holding
                            && evidence
                                .retired_at_us
                                .is_some_and(|retired| monotonic_us() - retired >= 1_600_000);
                        if holding_done && !terminal_sent {
                            queued.push_back(ended());
                        }
                        if !terminal_sent && !queued.is_empty() && (!holding || holding_done) {
                            let mut link = ready_link(2, 1);
                            link.state = ProviderLinkState::Terminal {
                                reason: LinkFailure::NoResumableContext,
                                fault: Fault::Transport,
                                attempts: 1,
                            };
                            peer.channels
                                .control
                                .send(WorkerEvent::ProviderLink { link })
                                .unwrap();
                            terminal_sent = true;
                        }
                        for _ in 0..2 {
                            let Some(event) = queued.front() else {
                                break;
                            };
                            if output.try_send(|| peer.channels.data.send(event)).unwrap() {
                                queued.pop_front();
                            } else {
                                break;
                            }
                        }
                    }
                    "speaker" => {
                        if let Some(chunk) = peer.channels.data.receive::<SpeakerChunk>().unwrap() {
                            let at = monotonic_us();
                            let first = PlaybackCursor {
                                epoch: 1,
                                frame: cursor,
                            };
                            cursor += chunk.samples.len() as u64;
                            let end = PlaybackCursor {
                                epoch: 1,
                                frame: cursor,
                            };
                            evidence.played.extend_from_slice(&chunk.samples);
                            assert!(evidence.played.len() <= maximum_played);
                            peer.channels
                                .control
                                .send(WorkerEvent::Playback {
                                    owner: chunk.permit.owner(),
                                    first_sample: first,
                                    end_sample: end,
                                    queue_observed_at_us: at,
                                    turn: chunk.permit.owner().turn(),
                                    sequence: chunk.chunk_sequence,
                                    queued_frames: 0,
                                    accepted_frames: chunk.samples.len(),
                                    observed_at_us: at,
                                })
                                .unwrap();
                            peer.channels
                                .control
                                .send(WorkerEvent::SpeechDrained {
                                    owner: chunk.permit.owner(),
                                    sequence: chunk.chunk_sequence,
                                    last_sample: end,
                                    observed_at_us: at,
                                })
                                .unwrap();
                            if chunk.end_of_speech {
                                evidence.retired_at_us.get_or_insert(at);
                                peer.channels
                                    .control
                                    .send(WorkerEvent::SpeechRetired {
                                        owner: chunk.permit.owner(),
                                        sequence: chunk.chunk_sequence,
                                        final_sample: end,
                                        observed_at_us: at,
                                    })
                                    .unwrap();
                            }
                        }
                    }
                    _ => {}
                }
                // An unchanged-source red run must finish with an explicit test
                // fault rather than hang indefinitely after the service bug.
                let fault_after = if holding {
                    Duration::from_secs(3)
                } else {
                    Duration::from_millis(1500)
                };
                if !guard_sent && elapsed >= fault_after && peer.role == "privacy" {
                    peer.channels
                        .control
                        .send(WorkerEvent::Fault {
                            code: "synthetic terminal-service timeout".into(),
                        })
                        .unwrap();
                    guard_sent = true;
                }
            }
            Ok(())
        };
        clock_scope.set_pump(pump).unwrap();
    }
    let mut trace = SessionState::default();
    if let Some(reply) = &reply {
        trace
            .receipts
            .admitted(reply.owner, monotonic_us())
            .unwrap();
    }
    let result = conversation(
        &mut rig,
        &mut owner,
        Lifetime::Serve,
        &mut trace,
        startup,
        &mut reply,
        &mut None,
        &Termination::inactive(),
        notices,
    );
    let result = finish_active_run(
        &mut rig, &mut owner, &mut reply, &mut trace, &mut None, result,
    );
    drop(clock_scope);
    let evidence = Rc::try_unwrap(shared)
        .ok()
        .expect("the pump left with its scope")
        .into_inner();
    let error = result.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("terminal provider output settled"),
        "{case:?}: {error}"
    );
    assert!(error.downcast_ref::<PhysicalMute>().is_none());
    assert!(reply.is_none());
    assert!(evidence.peers.iter().all(|p| p.stopped));
    assert!(!trace.iter().any(|e| e["kind"] == "worker_forced_shutdown"));
    (trace, evidence)
}

#[test]
fn idle_terminal_provider_ends_service_without_readiness_or_retry() {
    let (trace, evidence) = run(Case::Idle);
    assert!(evidence.played.is_empty());
    assert_eq!(evidence.provider_starts, 0);
    assert!(!trace.iter().any(|e| matches!(
        e["kind"].as_str(),
        Some("turn_finished" | "listening_ready" | "input_admitted")
    )));
}

#[test]
fn failed_awaiting_answer_settles_once_before_service_error() {
    let (trace, evidence) = run(Case::AwaitingAnswer);
    assert!(evidence.played.is_empty());
    assert_outcome(&trace, "provider_failed");
}

#[test]
fn completed_buffered_reply_is_fully_retired_before_service_error() {
    let (trace, evidence) = run(Case::CompleteBuffered);
    assert_eq!(evidence.played, vec![123; 480]);
    assert_outcome(&trace, "audio_written_unscored");
}

#[test]
fn holding_continuation_is_cleared_by_the_live_capture_gap_path_after_retirement() {
    for capture_gap in [false, true] {
        let (trace, evidence) = run(Case::Holding { capture_gap });
        assert_eq!(evidence.played, vec![123; 480]);
        let finished = trace
            .iter()
            .position(|event| event["kind"] == "turn_finished" && event["turn"] == 1)
            .unwrap();
        assert_eq!(trace[finished]["outcome"], "audio_written_unscored");
        let retired = evidence.retired_at_us.unwrap();
        let nudges: Vec<_> = trace
            .iter()
            .filter(|event| event["kind"] == "holding_answer_nudged")
            .collect();
        if capture_gap {
            let sent_gap = evidence.capture_gap_at_us.unwrap();
            assert!((500_000..510_000).contains(&(sent_gap - retired)));
            let closed = trace
                .iter()
                .position(|event| event["kind"] == "capture_gap_closed")
                .unwrap();
            let cancelled = trace
                .iter()
                .position(|event| {
                    event["kind"] == "hand_control_cancel" && event["reason"] == "capture_gap"
                })
                .unwrap();
            assert!(finished < closed && closed < cancelled);
            let cancelled_at = trace[cancelled]["at_us"].as_u64().unwrap();
            assert!(cancelled_at >= sent_gap);
            assert!(evidence.last_capture_us.unwrap() - cancelled_at >= 1_000_000);
            assert!(trace.iter().any(|event| {
                event["kind"] == "capture_started" && event["details"]["epoch"] == 2
            }));
            assert!(
                nudges.is_empty(),
                "the live owner must clear the retired promise"
            );
            assert!(evidence.written.is_empty(), "nothing reaches the provider");
        } else {
            // Without the gap, this same retired reply and fresh silence
            // must really queue the note. Otherwise the negative case could
            // pass without ever having an eligible holding opportunity.
            assert_eq!(nudges.len(), 1);
            assert!(nudges[0]["at_us"].as_u64().unwrap() - retired >= 500_000);
            assert_eq!(evidence.written.len(), 1);
            let (_, session, text) = &evidence.written[0];
            assert_eq!(*session, 1);
            assert_eq!(text, crate::holding_answer::NOTE);
            assert!(evidence.capture_gap_at_us.is_none());
        }
    }
}

#[test]
fn failed_buffered_reply_keeps_exact_pcm_and_failed_outcome() {
    let (trace, evidence) = run(Case::FailedBuffered);
    assert_eq!(evidence.played, vec![123; 480]);
    assert_outcome(&trace, "provider_failed");
}

#[test]
fn in_flight_start_refusal_after_ended_is_not_skipped() {
    let (trace, evidence) = run(Case::RefusalAfterEnded);
    assert!(evidence.played.is_empty());
    assert_outcome(&trace, "provider_failed");
    assert!(
        trace
            .iter()
            .any(|e| e["kind"] == "provider_turn_failed" && e["stage"] == "not_delivered")
    );
}

#[test]
fn abandoned_open_input_reaches_actual_detector_end_without_tail_replay() {
    let (trace, evidence) = run(Case::OpenInput);
    assert!(evidence.played.is_empty());
    assert_eq!(evidence.provider_starts, 1);
    assert_eq!(evidence.provider_ends, 0);
    assert!(evidence.capture_blocks >= 76);
    assert_outcome(&trace, "provider_failed");
    assert_eq!(
        trace
            .iter()
            .filter(|e| e["kind"] == "input_admitted")
            .count(),
        1
    );
    let endpoint = trace
        .iter()
        .position(|e| e["kind"] == "input_discarded_endpoint")
        .unwrap();
    let finished = trace
        .iter()
        .position(|e| e["kind"] == "turn_finished")
        .unwrap();
    assert!(endpoint < finished);
}

fn assert_outcome(trace: &SessionState, outcome: &str) {
    let finished: Vec<_> = trace
        .iter()
        .filter(|e| e["kind"] == "turn_finished")
        .collect();
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0]["outcome"], outcome);
    assert!(!trace.iter().any(|e| e["kind"] == "reply_cancelled"));
}

#[test]
fn failure_notice_runs_through_production_conversation_after_awaiting_and_refused_start() {
    for case in [Case::AwaitingAnswer, Case::RefusalAfterEnded] {
        let (trace, evidence) = run_with_notices(case, &FailureNotices::synthetic());
        assert_eq!(evidence.played, vec![321; 7_200]);
        assert_outcome(&trace, "provider_failed");
        assert_eq!(
            trace
                .iter()
                .filter(|e| e["kind"] == "failure_notice_attempted")
                .count(),
            1
        );
        assert_eq!(
            trace
                .iter()
                .filter(|e| e["kind"] == "failure_notice_first_write")
                .count(),
            1
        );
        assert_eq!(
            trace
                .iter()
                .filter(|e| e["kind"] == "failure_notice_final_sample_retired")
                .count(),
            1
        );
        assert!(!trace.iter().any(|e| e["kind"] == "speaker_first_write"));
        let finish = trace.iter().find(|e| e["kind"] == "turn_finished").unwrap();
        assert_eq!(finish["failure_notice"]["state"], "retired");
        assert_eq!(finish["playback_gaps"], 0);
    }
}

#[test]
fn failure_notice_keeps_partial_answer_and_new_playback_occurrence() {
    let (trace, evidence) = run_with_notices(Case::FailedBuffered, &FailureNotices::synthetic());
    assert_eq!(&evidence.played[..480], &vec![123; 480]);
    assert_eq!(&evidence.played[480..], &vec![321; 7_200]);
    let prior = trace
        .iter()
        .find(|e| e["kind"] == "speech_final_sample_retired")
        .unwrap();
    let notice = trace
        .iter()
        .find(|e| e["kind"] == "failure_notice_first_write")
        .unwrap();
    assert!(prior["at_us"].as_u64().unwrap() <= notice["at_us"].as_u64().unwrap());
    assert_ne!(prior["playback_sequence"], notice["playback_sequence"]);
    assert_eq!(
        trace
            .iter()
            .filter(|e| e["kind"] == "provider_first_audio")
            .count(),
        1
    );
    assert_eq!(
        trace
            .iter()
            .filter(|e| e["kind"] == "speaker_first_write")
            .count(),
        1
    );
    assert_outcome(&trace, "provider_failed");
}

#[test]
fn failure_notice_waits_for_actual_abandoned_input_endpoint_without_replay() {
    let (trace, evidence) = run_with_notices(Case::OpenInput, &FailureNotices::synthetic());
    assert_eq!(evidence.played, vec![321; 7_200]);
    assert_eq!((evidence.provider_starts, evidence.provider_ends), (1, 0));
    let endpoint = trace
        .iter()
        .position(|e| e["kind"] == "input_discarded_endpoint")
        .unwrap();
    let notice = trace
        .iter()
        .position(|e| e["kind"] == "failure_notice_attempted")
        .unwrap();
    assert!(endpoint < notice);
    assert_outcome(&trace, "provider_failed");
}

#[test]
fn failure_notice_is_absent_for_idle_or_successful_terminal_and_default_stays_silent() {
    for case in [Case::Idle, Case::CompleteBuffered] {
        let (trace, evidence) = run_with_notices(case, &FailureNotices::synthetic());
        assert!(
            !trace
                .iter()
                .any(|e| e["kind"] == "failure_notice_attempted")
        );
        assert_eq!(
            evidence.played.len(),
            if matches!(case, Case::Idle) { 0 } else { 480 }
        );
    }
    let (trace, evidence) = run(Case::AwaitingAnswer);
    assert!(evidence.played.is_empty());
    assert!(
        trace
            .iter()
            .any(|e| e["kind"] == "failure_notice_selected" && e["notice"]["state"] == "skipped")
    );
    assert_outcome(&trace, "provider_failed");
}

#[test]
fn independent_notice_v1_baseline_uses_the_real_conversation_path() {
    for case in [
        Case::AwaitingAnswer,
        Case::RefusalAfterEnded,
        Case::FailedBuffered,
    ] {
        let (trace, evidence) =
            run_with_notice_policy(case, &FailureNotices::synthetic(), TurnTaking::V1Baseline);
        let prefix = if matches!(case, Case::FailedBuffered) {
            480
        } else {
            0
        };
        assert_eq!(&evidence.played[prefix..], &vec![321; 7200]);
        assert_eq!(
            trace
                .iter()
                .filter(|e| e["kind"] == "failure_notice_attempted")
                .count(),
            1
        );
        assert_eq!(
            trace
                .iter()
                .filter(|e| e["kind"] == "failure_notice_final_sample_retired")
                .count(),
            1
        );
        assert_outcome(&trace, "provider_failed");
    }
}
