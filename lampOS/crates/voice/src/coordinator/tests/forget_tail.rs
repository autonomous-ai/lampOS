//! Host queue occupancy at actual Engine forget admission. No acoustic claim.
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
    round: AtomicUsize,
    admit: Arc<Notify>,
}
struct TailListener;
impl Listener for TailListener {
    fn send(&self, _: &[i16]) -> net::Result<()> {
        Ok(())
    }
    fn finish(self: Box<Self>, _: u64) -> BoxFuture<'static, net::Result<String>> {
        Box::pin(async { Ok("Forget that preference.".into()) })
    }
}
impl Services for TailServices {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        Box::pin(async { Ok(Box::new(TailListener) as Box<dyn Listener>) })
    }
    fn chat(&self, _: Vec<net::Message>) -> BoxStream<'static, net::Result<net::ChatEvent>> {
        use net::ChatEvent;
        if self.round.fetch_add(1, Ordering::SeqCst) == 0 {
            let admit = self.admit.clone();
            stream::iter([Ok(ChatEvent::Text("Old speech. ".into()))])
                .chain(stream::once(async move {
                    admit.notified().await;
                    Ok(ChatEvent::ToolCalls(vec![net::ToolCall {
                        id: "forget".into(),
                        name: "forget".into(),
                        args: json!({"about":"that preference"}),
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
/// authority. The owner/device path is outside this provider-only regression.
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

async fn measure_forget_tail(draining: bool) {
    let admit = Arc::new(Notify::new());
    let entered = Arc::new(Notify::new());
    let finish = Arc::new(Notify::new());
    let runner: ToolRunner = {
        let entered = entered.clone();
        let finish = finish.clone();
        Arc::new(move |_, _, name, _| {
            assert_eq!(name, "forget");
            // Synchronous runner entry is immediately after Queue::forget.
            entered.notify_one();
            let finish = finish.clone();
            Box::pin(async move {
                finish.notified().await;
                json!({"forgotten":true})
            })
        })
    };
    let mut engine = Engine::new(
        Arc::new(TailServices {
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
    let (_, mut reply, _) = active_reply();
    reply.pcm.clear();
    let mut pending = None;
    let mut handed = 0;
    let mut sent = 0;
    let mut received = 0;
    let mut speaker = SpeakerModel::default();
    // No wall-clock playback or model/network waits. Each yield lets actual
    // Engine producers run; bounded iterations diagnose a stalled fixture.
    for _ in 0..20_000 {
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
        "forget_admission draining={draining} worker_frames={worker_frames} ipc_frames={ipc_frames} coordinator_frames={} speaker_unaccepted_frames={} accepted_unretired_frames={} retired_frames={} emitted_unplayed_ms={:.3}; boundary=actual_tool_runner_entry_after_provider_invalidation; speaker=simulated; acoustic=false",
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
    // Old Shared.out events are now stale. They must never emerge through the
    // actual engine, even though the separately retained worker packet survives.
    while let Some(event) = engine.try_output() {
        assert_eq!(audio_frames(&event), 0);
    }
    finish.notify_one();
    let mut fresh = 0;
    let mut completed = false;
    for _ in 0..20_000 {
        while let Some(event) = engine.try_output() {
            match event {
                ProviderOutput::Audio { samples, .. } => {
                    assert!(
                        samples.iter().all(|sample| *sample == FRESH),
                        "old provider audio escaped reset"
                    );
                    fresh += samples.len();
                }
                ProviderOutput::TurnComplete { .. } => completed = true,
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
    measure_forget_tail(false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn forget_admission_measures_draining_emitted_tail() {
    measure_forget_tail(true).await;
}
