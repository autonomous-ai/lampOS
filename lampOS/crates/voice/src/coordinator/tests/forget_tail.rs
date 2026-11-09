//! Matched host queue occupancy before/after actual Engine forget admission.
//! Speaker drops are modelled only; no acoustic claim.
use super::*;
use crate::{
    provider_flow::OutputSender,
    second_provider::{Engine, Listener, Services, Told, ToolRunner, net},
};
use futures_util::{
    StreamExt,
    future::BoxFuture,
    stream::{self, BoxStream},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;

const OLD: i16 = 111;
const FRESH: i16 = 222;

struct TailServices {
    tool: &'static str,
    round: AtomicUsize,
    admit: Arc<Notify>,
}
struct TailListener(&'static str);
impl Listener for TailListener {
    fn send(&self, _: &[i16]) -> net::Result<()> {
        Ok(())
    }
    fn finish(self: Box<Self>, _: u64) -> BoxFuture<'static, net::Result<String>> {
        let line = if self.0 == "forget_my_face" {
            "Forget my face."
        } else {
            "Forget that preference."
        };
        Box::pin(async move { Ok(line.into()) })
    }
}
impl Services for TailServices {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        let tool = self.tool;
        Box::pin(async move { Ok(Box::new(TailListener(tool)) as Box<dyn Listener>) })
    }
    fn chat(&self, _: Vec<net::Message>) -> BoxStream<'static, net::Result<net::ChatEvent>> {
        use net::ChatEvent;
        if self.round.fetch_add(1, Ordering::SeqCst) == 0 {
            let admit = self.admit.clone();
            let tool = self.tool;
            stream::iter([Ok(ChatEvent::Text("Old speech. ".into()))])
                .chain(stream::once(async move {
                    admit.notified().await;
                    Ok(ChatEvent::ToolCalls(vec![net::ToolCall {
                        id: "forget".into(),
                        name: tool.into(),
                        args: if tool == "forget_my_face" {
                            json!({"name":"A"})
                        } else {
                            json!({"about":"that preference"})
                        },
                        extra: None,
                    }]))
                }))
                .chain(stream::iter([Ok(ChatEvent::Done)]))
                .boxed()
        } else {
            stream::iter([
                Ok(ChatEvent::Text("Fresh reply.".into())),
                Ok(ChatEvent::Done),
            ])
            .boxed()
        }
    }
    fn speak(&self, text: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
        let (value, packets) = if text.contains("Old speech") {
            (OLD, 875)
        } else {
            (FRESH, 1)
        };
        stream::iter((0..packets).map(move |_| Ok(vec![value; PACKET_SAMPLES]))).boxed()
    }
}

fn audio_frames(event: &ProviderOutput) -> usize {
    match event {
        ProviderOutput::Audio { samples, .. } => samples.len(),
        _ => 0,
    }
}

/// A deterministic speaker model. Frames move out of Reply using production
/// chunking, then either retire or remain in exactly one of these two buckets.
/// It does not invoke ALSA, fabricate physical retirement receipts, or grant
/// authority. The extension cases use actual coordinator ownership and IPC,
/// but this model still provides no device or acoustic evidence.
#[derive(Default)]
struct SpeakerModel {
    retired: usize,
    unaccepted: usize,
    accepted: usize,
}
impl SpeakerModel {
    fn drain(&mut self, reply: &mut Reply) {
        for _ in 0..4 {
            if let Some((samples, _)) = reply.next_speaker_chunk() {
                assert!(samples.iter().all(|sample| *sample == OLD));
                self.retired += samples.len();
            }
        }
    }
    fn stage(&mut self, reply: &mut Reply) {
        for _ in 0..2 {
            if let Some((samples, _)) = reply.next_speaker_chunk() {
                self.accepted += samples.len();
            }
        }
        if let Some((samples, _)) = reply.next_speaker_chunk() {
            self.unaccepted = samples.len();
        }
        assert!(self.accepted <= lamp_audio::ownership::MAX_QUEUED_FRAMES);
        assert!(self.unaccepted <= lamp_audio::RENDER_SAMPLES);
    }
}

async fn measure_forget_tail(draining: bool, playback_extension: bool, tool: &'static str) {
    let admit = Arc::new(Notify::new());
    let entered = Arc::new(Notify::new());
    let finish = Arc::new(Notify::new());
    let runner: ToolRunner = {
        let entered = entered.clone();
        let finish = finish.clone();
        Arc::new(move |_, _, name, _| {
            assert_eq!(name, tool);
            // Synchronous runner entry is immediately after Queue::forget.
            entered.notify_one();
            let finish = finish.clone();
            Box::pin(async move {
                finish.notified().await;
                if tool == "forget_my_face" {
                    json!({"ok":true})
                } else {
                    json!({"forgotten":true})
                }
            })
        })
    };
    let mut engine = Engine::new(
        Arc::new(TailServices {
            tool,
            round: AtomicUsize::new(0),
            admit: admit.clone(),
        }),
        || Told::from("Answer briefly.".to_owned()),
        runner,
    )
    .announcing(false);
    engine.submit(ProviderInput::Start {
        request: 1,
        session: 1,
        privacy_generation: 1,
        continues: false,
        continues_cut_off: false,
        over_the_lamp: false,
        after_stop: false,
        follows_on: false,
        over_the_lamps_voice: false,
        heard_of_cut: None,
        soft_lead_us: 0,
    });
    engine.submit(ProviderInput::Audio {
        request: 1,
        privacy_generation: 1,
        sequence: 1,
        read_completed_at_us: 1,
        samples: vec![0; lamp_audio::CAPTURE_SAMPLES],
    });
    engine.submit(ProviderInput::End {
        request: 1,
        privacy_generation: 1,
        sound: false,
        cut_in: false,
    });
    let dir = SessionDirectory::create().unwrap();
    let (mut parent, mut provider) = pair(&dir, "provider");
    let mut sender = OutputSender::default();
    let mut receiver = OutputReceiver::default();
    let (mut owner, mut reply, _) = active_reply();
    // Renew the fixture's actual controller while filling the same queues.
    // Its original 500 ms capture/admission grant is not a host speed promise.
    let renew = |owner: &mut Controller| {
        let at = now();
        let until = MonoTime::from_micros(at.as_micros() + 500_000);
        owner
            .set_capture(at, CaptureState::RetainingUntil(until))
            .unwrap();
        owner
            .set_admission(at, AdmissionState::OpenUntil(until))
            .unwrap();
    };
    reply.pcm.clear();
    let mut pending = None;
    let mut handed = 0;
    let mut sent = 0;
    let mut received = 0;
    let mut speaker = SpeakerModel::default();
    // No wall-clock playback or model/network waits. Each yield lets actual
    // Engine producers run; bounded iterations diagnose a stalled fixture.
    for _ in 0..20_000 {
        renew(&mut owner);
        while let Some(Control::ProviderOutputCapacity { through }) =
            provider.control.receive().unwrap()
        {
            sender.grant(through).unwrap();
        }
        if pending.is_none() {
            pending = engine.try_output();
            if let Some(event) = &pending {
                handed += audio_frames(event);
            }
        }
        if let Some(event) = &pending
            && sender.try_send(|| provider.data.send(event)).unwrap()
        {
            sent += audio_frames(event);
            pending = None;
        }
        while let Some(event) = parent.data.receive::<ProviderOutput>().unwrap() {
            receiver.received().unwrap();
            if let ProviderOutput::Audio { samples, .. } = event {
                assert!(samples.iter().all(|sample| *sample == OLD));
                received += samples.len();
                reply.append_provider_audio(samples, monotonic_us());
            }
        }
        if draining {
            speaker.drain(&mut reply);
        }
        assert!(reply.pcm.len() <= MAX_REPLY_SAMPLES);
        receiver
            .replenish(reply.pcm.len(), &mut parent.control)
            .unwrap();
        if received >= MAX_REPLY_SAMPLES {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        received, MAX_REPLY_SAMPLES,
        "old speech must reach the selected admission point"
    );
    // Retain one real worker output before reset; it is explicitly outside the
    // provider fence. Do not manufacture a packet when the engine is blocked.
    for _ in 0..1000 {
        renew(&mut owner);
        if pending.is_none() {
            pending = engine.try_output();
            if let Some(event) = &pending {
                handed += audio_frames(event);
            }
        }
        if pending
            .as_ref()
            .is_some_and(|event| audio_frames(event) > 0)
        {
            break;
        }
        assert!(
            pending.is_none(),
            "unexpected lifecycle output before forget"
        );
        tokio::task::yield_now().await;
    }
    // Spend any remaining real IPC credit without receiving it: the draining
    // control can retain two wire packets, whereas the saturated queue grants
    // none. Then retain exactly one additional worker packet.
    while let Some(Control::ProviderOutputCapacity { through }) =
        provider.control.receive().unwrap()
    {
        sender.grant(through).unwrap();
    }
    for _ in 0..2 {
        let event = pending.as_ref().unwrap();
        if !sender.try_send(|| provider.data.send(event)).unwrap() {
            break;
        }
        sent += audio_frames(event);
        for _ in 0..1000 {
            renew(&mut owner);
            pending = engine.try_output();
            if let Some(event) = &pending {
                handed += audio_frames(event);
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(
            pending
                .as_ref()
                .is_some_and(|event| audio_frames(event) > 0)
        );
    }
    let worker_frames = pending.as_ref().map_or(0, audio_frames);
    assert_eq!(worker_frames, PACKET_SAMPLES);
    speaker.stage(&mut reply);
    // Freeze every downstream bucket before releasing the actual model call.
    // current_thread guarantees no fixture transfers during this await.
    admit.notify_one();
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    let ipc_frames = sent - received;
    let outstanding =
        worker_frames + ipc_frames + reply.pcm.len() + speaker.unaccepted + speaker.accepted;
    assert_eq!(
        handed,
        outstanding + speaker.retired,
        "each emitted frame counted exactly once"
    );
    println!(
        "forget_admission tool={tool} draining={draining} worker_frames={worker_frames} ipc_frames={ipc_frames} coordinator_frames={} speaker_unaccepted_frames={} accepted_unretired_frames={} retired_frames={} emitted_unplayed_ms={:.3}; boundary=actual_tool_runner_entry_after_provider_invalidation; speaker=simulated; acoustic=false",
        reply.pcm.len(),
        speaker.unaccepted,
        speaker.accepted,
        speaker.retired,
        outstanding as f64 / 24.0
    );
    if draining {
        assert!(outstanding <= 4 * PACKET_SAMPLES);
    } else {
        assert!(outstanding >= MAX_REPLY_SAMPLES);
    }
    assert_eq!(
        outstanding,
        if draining { 3_120 } else { 720_960 },
        "before queues must match the original 130 ms / 30,040 ms fixture"
    );
    let provider_boot = new_boot().unwrap();
    let mut extension = None;
    if playback_extension {
        use crate::speech_cut::ProviderCut;
        renew(&mut owner);
        let cut = engine
            .speech_cut()
            .expect("actual admitted forget publishes a cut");
        provider
            .control
            .send(WorkerEvent::ProviderSpeechCut { cut })
            .unwrap();
        let mut found = None;
        service_provider_control_inner(
            &mut parent.control,
            &mut CloudAvailability::default(),
            &mut 8,
            &mut SessionState::default(),
            Some(&mut found),
        )
        .unwrap();
        assert_eq!(found, Some(cut));
        let (mut speaker_parent, mut speaker_peer) = pair(&dir, "speaker");
        let mut state = super::super::speech_cut::State::default();
        let mut trace = SessionState::default();
        let turn = reply.owner;
        let mut snapshot = owner.snapshot(now()).unwrap();
        let coordinator_discarded = reply.pcm.len();
        let speaker_through = ((speaker.retired + speaker.accepted + speaker.unaccepted)
            / lamp_audio::RENDER_SAMPLES) as u64;
        let mut current = Some(reply);
        state
            .admit(
                provider_boot,
                cut,
                &mut current,
                speaker_through,
                |control| {
                    super::super::speech_cut::send_speaker_cut(
                        &mut speaker_parent.control,
                        snapshot,
                        control,
                    )
                },
                &mut trace,
            )
            .unwrap();
        assert!(state.waiting());
        assert!(current.as_ref().unwrap().pcm.is_empty());
        // The separately retained worker output is outside the provider token
        // fence, and must be removed by the inclusive wire watermark.
        assert!(state.discards(provider_boot, pending.as_ref().unwrap()));
        let worker_discarded = audio_frames(&pending.take().unwrap());
        let mut ipc_discarded = 0;
        while let Some(event) = parent.data.receive::<ProviderOutput>().unwrap() {
            receiver.received().unwrap();
            assert!(state.discards(provider_boot, &event));
            ipc_discarded += audio_frames(&event);
        }
        assert_eq!(ipc_discarded, ipc_frames);
        assert!(
            matches!(speaker_peer.control.receive::<Control>().unwrap(), Some(Control::Authority { snapshot }) if snapshot.owner() == Some(turn))
        );
        // The model is cleared ONLY when its production control IPC cut arrives.
        // It does not stand in for an ALSA drop or acoustic observation.
        assert!(
            matches!(speaker_peer.control.receive::<Control>().unwrap(), Some(Control::CutSpeech { owner, through }) if owner == turn && through == speaker_through)
        );
        let model_unaccepted_discarded = std::mem::take(&mut speaker.unaccepted);
        let model_accepted_discarded = std::mem::take(&mut speaker.accepted);
        let model_drop_at_us = monotonic_us();
        speaker_peer
            .control
            .send(WorkerEvent::SpeechCutApplied {
                owner: turn,
                through: speaker_through,
                applied_at_us: model_drop_at_us,
            })
            .unwrap();
        struct MeasuredIo<'a> {
            speaker: &'a mut WorkerChannels,
            provider: &'a mut WorkerChannels,
        }
        impl SpeakerControlIo for MeasuredIo<'_> {
            fn next_speaker_event(&mut self) -> io::Result<Option<WorkerEvent>> {
                self.speaker.control.receive()
            }
            fn publish_speaker_snapshot(&mut self, _: Snapshot) -> io::Result<()> {
                Ok(())
            }
            fn speech_cut_applied(&mut self, _: BootId, cut: ProviderCut) -> io::Result<()> {
                self.provider
                    .control
                    .send(Control::ProviderSpeechCutApplied {
                        request: cut.request,
                        through: cut.through,
                    })
            }
        }
        service_speaker_control_inner(
            &mut MeasuredIo {
                speaker: &mut speaker_parent,
                provider: &mut parent,
            },
            &mut owner,
            &mut current,
            &mut None,
            &mut PlaybackMask::new(TurnTaking::VadOnly),
            &mut snapshot,
            &mut 0,
            &mut 8,
            &mut trace,
            &mut None,
            Some(&mut state),
        )
        .unwrap();
        assert!(!state.waiting());
        assert!(
            matches!(provider.control.receive::<Control>().unwrap(), Some(Control::ProviderSpeechCutApplied { request, through }) if request == cut.request && through == cut.through)
        );
        let model_ack_at_us = monotonic_us();
        reply = current.unwrap();
        assert_eq!(
            worker_discarded
                + ipc_discarded
                + coordinator_discarded
                + model_unaccepted_discarded
                + model_accepted_discarded,
            outstanding,
            "discard buckets must be disjoint and account for every old unplayed frame"
        );
        let remaining = pending.as_ref().map_or(0, audio_frames)
            + reply.pcm.len()
            + speaker.unaccepted
            + speaker.accepted;
        assert_eq!(remaining, 0);
        assert!(parent.data.receive::<ProviderOutput>().unwrap().is_none());
        assert_eq!(handed, outstanding + speaker.retired);
        println!(
            "forget_extension tool={tool} draining={draining} before_worker_frames={worker_frames} before_ipc_frames={ipc_frames} before_coordinator_frames={coordinator_discarded} before_speaker_unaccepted_frames={model_unaccepted_discarded} before_accepted_unretired_frames={model_accepted_discarded} before_emitted_unplayed_ms={:.3} after_worker_frames=0 after_ipc_frames=0 after_coordinator_frames=0 after_speaker_unaccepted_frames=0 after_accepted_unretired_frames=0 after_emitted_unplayed_ms=0.000 admission_to_modelled_drop_us={} admission_to_modelled_ack_us={}; boundary=actual_Engine_admission_to_scripted_speaker_cut_and_ACK; speaker=simulated; acoustic=false",
            outstanding as f64 / 24.0,
            model_drop_at_us.saturating_sub(cut.admitted_at_us),
            model_ack_at_us.saturating_sub(cut.admitted_at_us)
        );
        receiver.replenish(0, &mut parent.control).unwrap();
        while let Some(control) = provider.control.receive::<Control>().unwrap() {
            let Control::ProviderOutputCapacity { through } = control else {
                panic!("only returned capacity remains")
            };
            sender.grant(through).unwrap();
        }
        extension = Some(state);
    }
    // Old Shared.out events are now stale. They must never emerge through the
    // actual engine. The baseline retains its separate worker packet; the
    // extension has discarded that packet using the wire watermark.
    while let Some(event) = engine.try_output() {
        assert_eq!(audio_frames(&event), 0);
    }
    finish.notify_one();
    let mut fresh = 0;
    let mut completed = false;
    for _ in 0..20_000 {
        renew(&mut owner);
        while let Some(mut event) = engine.try_output() {
            if let Some(state) = &extension {
                assert!(!state.waiting());
                assert!(!state.discards(provider_boot, &event));
                assert!(sender.try_send(|| provider.data.send(&event)).unwrap());
                event = parent.data.receive::<ProviderOutput>().unwrap().unwrap();
                receiver.received().unwrap();
                receiver.replenish(0, &mut parent.control).unwrap();
                while let Some(Control::ProviderOutputCapacity { through }) =
                    provider.control.receive().unwrap()
                {
                    sender.grant(through).unwrap();
                }
            }
            match event {
                ProviderOutput::Audio {
                    request, samples, ..
                } => {
                    assert_eq!(request, reply.owner.turn());
                    assert!(
                        samples.iter().all(|sample| *sample == FRESH),
                        "old provider audio escaped reset"
                    );
                    fresh += samples.len();
                }
                ProviderOutput::TurnComplete { request, .. } => {
                    assert_eq!(request, reply.owner.turn());
                    completed = true;
                }
                _ => {}
            }
        }
        if completed {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(completed);
    assert_eq!(fresh, PACKET_SAMPLES);
}

#[tokio::test(flavor = "current_thread")]
async fn forget_admission_measures_saturated_emitted_tail() {
    measure_forget_tail(false, false, "forget").await;
}

#[tokio::test(flavor = "current_thread")]
async fn forget_admission_measures_draining_emitted_tail() {
    measure_forget_tail(true, false, "forget").await;
}

#[tokio::test(flavor = "current_thread")]
async fn forget_extension_clears_the_same_saturated_tail() {
    measure_forget_tail(false, true, "forget").await;
}

#[tokio::test(flavor = "current_thread")]
async fn forget_extension_clears_the_same_draining_tail() {
    measure_forget_tail(true, true, "forget").await;
}

#[tokio::test(flavor = "current_thread")]
async fn face_forget_extension_clears_saturated_emitted_tail() {
    measure_forget_tail(false, true, "forget_my_face").await;
}

#[tokio::test(flavor = "current_thread")]
async fn face_forget_extension_clears_draining_emitted_tail() {
    measure_forget_tail(true, true, "forget_my_face").await;
}

/// Real provider worker/control IPC: withheld audio credits and a refused or
/// missing cut ACK cannot block Stop, nor release fresh output/lifecycle.
#[tokio::test]
async fn provider_cut_without_ack_holds_output_and_stops_or_fails_closed() {
    for stop in [true, false] {
        let (mut owner, current, _) = active_reply();
        let boot = current.owner.boot();
        let request = current.owner.turn();
        let provider_boot = new_boot().unwrap();
        let dir = SessionDirectory::create().unwrap();
        let mut parent =
            WorkerChannels::bind(&dir.path, "provider", boot, provider_boot, false).unwrap();
        let mut worker =
            WorkerChannels::bind(&dir.path, "provider", boot, provider_boot, true).unwrap();
        parent.connect(&dir.path, "provider", false).unwrap();
        worker.connect(&dir.path, "provider", true).unwrap();
        let admit = Arc::new(Notify::new());
        let engine = Engine::new(
            Arc::new(TailServices {
                tool: "forget",
                round: AtomicUsize::new(0),
                admit: admit.clone(),
            }),
            || Told::from("Answer briefly.".to_owned()),
            Arc::new(|_, _, _, _| Box::pin(async { json!({"forgotten":true}) })),
        )
        .announcing(false);
        let renew = |owner: &mut Controller| {
            let at = now();
            let until = MonoTime::from_micros(at.as_micros() + 500_000);
            owner
                .set_capture(at, CaptureState::RetainingUntil(until))
                .unwrap();
            owner
                .set_admission(at, AdmissionState::OpenUntil(until))
                .unwrap();
            owner.snapshot(now()).unwrap()
        };
        let snapshot = renew(&mut owner);
        let privacy_generation = snapshot.microphone_generation();
        parent
            .control
            .send(Control::Authority { snapshot })
            .unwrap();
        for input in [
            ProviderInput::Start {
                request,
                session: 1,
                privacy_generation,
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
                heard_of_cut: None,
                soft_lead_us: 0,
            },
            ProviderInput::Audio {
                request,
                privacy_generation,
                sequence: 1,
                read_completed_at_us: monotonic_us(),
                samples: vec![0; lamp_audio::CAPTURE_SAMPLES],
            },
            ProviderInput::End {
                request,
                privacy_generation,
                sound: false,
                cut_in: false,
            },
        ] {
            parent.data.send(input).unwrap();
        }
        let mut running = tokio::spawn(crate::second_provider::worker::run(
            worker, boot, engine, None, None,
        ));
        let cut = tokio::time::timeout(Duration::from_secs(2), async {
            let mut tick = tokio::time::interval(Duration::from_millis(10));
            let mut received = 0;
            loop {
                tick.tick().await;
                parent
                    .control
                    .send(Control::Authority {
                        snapshot: renew(&mut owner),
                    })
                    .unwrap();
                while let Some(event) = parent.control.receive::<WorkerEvent>().unwrap() {
                    if let WorkerEvent::ProviderSpeechCut { cut } = event {
                        return cut;
                    }
                }
                while parent.data.receive::<ProviderOutput>().unwrap().is_some() {
                    received += 1;
                }
                // Spend the initial real two-packet credit, then never renew it.
                if received == 2 {
                    admit.notify_one();
                }
                assert!(
                    !running.is_finished(),
                    "worker ended before publishing its priority cut"
                );
            }
        })
        .await
        .expect("priority cut bypasses exhausted output credit");
        assert_eq!(cut.request, request);
        parent
            .control
            .send(Control::ProviderSpeechCutApplied {
                request,
                through: cut.through + 1,
            })
            .unwrap();
        if stop {
            // A future, unsent ACK is refused. Stop remains a priority control.
            tokio::time::sleep(Duration::from_millis(10)).await;
            assert!(!running.is_finished());
            assert!(parent.data.receive::<ProviderOutput>().unwrap().is_none());
            parent.control.send(Control::Stop).unwrap();
            tokio::time::timeout(Duration::from_millis(300), &mut running)
                .await
                .expect("Stop must not wait for a cut ACK")
                .unwrap()
                .unwrap();
        } else {
            let result = tokio::time::timeout(Duration::from_millis(700), async {
                let mut tick = tokio::time::interval(Duration::from_millis(20));
                loop {
                    tokio::select! {
                        result = &mut running => break result.unwrap(),
                        _ = tick.tick() => {
                            parent.control.send(Control::Authority { snapshot: renew(&mut owner) }).unwrap();
                            assert!(parent.data.receive::<ProviderOutput>().unwrap().is_none());
                        }
                    }
                }
            }).await.expect("missing cut ACK must have bounded failure");
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("speech cut acknowledgement expired")
            );
        }
    }
}

#[tokio::test]
async fn admitted_engine_cut_crosses_saturated_ipc_and_fences_late_audio_until_ack() {
    use crate::speech_cut::ProviderCut;
    let admit = Arc::new(Notify::new());
    let entered = Arc::new(Notify::new());
    let runner: ToolRunner = {
        let entered = entered.clone();
        Arc::new(move |_, _, name, _| {
            assert_eq!(name, "forget");
            entered.notify_one();
            Box::pin(async { json!({"forgotten":true}) })
        })
    };
    let mut engine = Engine::new(
        Arc::new(TailServices {
            tool: "forget",
            round: AtomicUsize::new(0),
            admit: admit.clone(),
        }),
        || Told::from("Answer briefly.".to_owned()),
        runner,
    )
    .announcing(false);
    for input in [
        ProviderInput::Start {
            request: 1,
            session: 1,
            privacy_generation: 1,
            continues: false,
            continues_cut_off: false,
            over_the_lamp: false,
            after_stop: false,
            follows_on: false,
            over_the_lamps_voice: false,
            heard_of_cut: None,
            soft_lead_us: 0,
        },
        ProviderInput::Audio {
            request: 1,
            privacy_generation: 1,
            sequence: 1,
            read_completed_at_us: monotonic_us(),
            samples: vec![0; lamp_audio::CAPTURE_SAMPLES],
        },
        ProviderInput::End {
            request: 1,
            privacy_generation: 1,
            sound: false,
            cut_in: false,
        },
    ] {
        engine.submit(input);
    }
    let dir = SessionDirectory::create().unwrap();
    let (mut parent, mut provider) = pair(&dir, "provider");
    let (speaker, mut speaker_peer) = pair(&dir, "speaker");
    let mut sender = OutputSender::default();
    let mut receiver = OutputReceiver::default();
    // Fill the actual two-packet IPC credit with old engine-produced audio.
    tokio::time::timeout(Duration::from_secs(2), async {
        while sender.can_send() {
            let event = next_cut_engine_output(&mut engine).await;
            if matches!(event, ProviderOutput::Audio { .. }) {
                assert!(sender.try_send(|| provider.data.send(&event)).unwrap());
            }
        }
    })
    .await
    .unwrap();
    assert!(!sender.try_send(|| panic!("no audio credit")).unwrap());
    admit.notify_one();
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    let cut = engine
        .speech_cut()
        .expect("admitted forget must publish its cut independently of audio");
    provider
        .control
        .send(WorkerEvent::ProviderSpeechCut { cut })
        .unwrap();
    let mut found = None;
    service_provider_control_inner(
        &mut parent.control,
        &mut CloudAvailability::default(),
        &mut 8,
        &mut SessionState::default(),
        Some(&mut found),
    )
    .unwrap();
    assert_eq!(found, Some(cut));
    struct AckIo {
        speaker: WorkerChannels,
        provider: WorkerChannels,
    }
    impl SpeakerControlIo for AckIo {
        fn next_speaker_event(&mut self) -> io::Result<Option<WorkerEvent>> {
            self.speaker.control.receive()
        }
        fn publish_speaker_snapshot(&mut self, _: Snapshot) -> io::Result<()> {
            Ok(())
        }
        fn speech_cut_applied(&mut self, _: BootId, cut: ProviderCut) -> io::Result<()> {
            self.provider
                .control
                .send(Control::ProviderSpeechCutApplied {
                    request: cut.request,
                    through: cut.through,
                })
        }
    }
    let mut io = AckIo {
        speaker,
        provider: parent,
    };
    let (mut owner, current, _) = active_reply();
    let turn = current.owner;
    let mut reply = Some(current);
    let mut state = super::super::speech_cut::State::default();
    let provider_boot = new_boot().unwrap();
    let mut trace = SessionState::default();
    let mut authority = owner.snapshot(now()).unwrap();
    state
        .admit(
            provider_boot,
            cut,
            &mut reply,
            0,
            |message| {
                super::super::speech_cut::send_speaker_cut(
                    &mut io.speaker.control,
                    authority,
                    message,
                )
            },
            &mut trace,
        )
        .unwrap();
    assert!(state.waiting());
    assert!(reply.as_ref().unwrap().pcm.is_empty());
    let mut old_packets = 0;
    while let Some(event) = io.provider.data.receive::<ProviderOutput>().unwrap() {
        receiver.received().unwrap();
        assert!(state.discards(provider_boot, &event));
        old_packets += 1;
    }
    assert_eq!(old_packets, 2);
    receiver.replenish(0, &mut io.provider.control).unwrap();
    let Some(Control::ProviderOutputCapacity { through }) =
        provider.control.receive::<Control>().unwrap()
    else {
        panic!("discarded audio must return capacity")
    };
    sender.grant(through).unwrap();
    assert!(sender.can_send());
    assert!(matches!(
        speaker_peer.control.receive::<Control>().unwrap(),
        Some(Control::Authority { .. })
    ));
    assert!(
        matches!(speaker_peer.control.receive::<Control>().unwrap(), Some(Control::CutSpeech { owner, through: 0 }) if owner == turn)
    );
    assert!(
        provider.control.receive::<Control>().unwrap().is_none(),
        "fresh barrier waits for speaker ACK"
    );
    speaker_peer
        .control
        .send(WorkerEvent::SpeechCutApplied {
            owner: turn,
            through: 0,
            applied_at_us: monotonic_us(),
        })
        .unwrap();
    service_speaker_control_inner(
        &mut io,
        &mut owner,
        &mut reply,
        &mut None,
        &mut PlaybackMask::new(TurnTaking::VadOnly),
        &mut authority,
        &mut 0,
        &mut 8,
        &mut trace,
        &mut None,
        Some(&mut state),
    )
    .unwrap();
    assert!(!state.waiting());
    assert!(
        matches!(provider.control.receive::<Control>().unwrap(), Some(Control::ProviderSpeechCutApplied { request: 1, through }) if through == cut.through)
    );
    let fresh = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = next_cut_engine_output(&mut engine).await;
            if let ProviderOutput::Audio { samples, .. } = &event {
                assert!(samples.iter().all(|sample| *sample == FRESH));
                break event;
            }
        }
    })
    .await
    .unwrap();
    assert!(!state.discards(provider_boot, &fresh));
    assert!(sender.try_send(|| provider.data.send(&fresh)).unwrap());
    let fresh = io
        .provider
        .data
        .receive::<ProviderOutput>()
        .unwrap()
        .unwrap();
    receiver.received().unwrap();
    assert!(!state.discards(provider_boot, &fresh));
    assert_eq!(reply.as_ref().unwrap().owner, turn);
}

async fn next_cut_engine_output(engine: &mut Engine) -> ProviderOutput {
    loop {
        if let Some(event) = engine.try_output() {
            return event;
        }
        tokio::task::yield_now().await;
    }
}
