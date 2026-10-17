//! Explicit capture time, through the actual sender, Actor and scripted socket.
//! Tokio stays separate; receipts, never clock samples, establish handled work.
use super::*;
use crate::{
    Connect, Credential, GOOGLE_ENDPOINT,
    testing::{self, CaptureClock, FakeService},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

struct CaptureTime {
    instant: Mutex<Instant>,
    reads: AtomicU64,
}
impl CaptureTime {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            // A host-clock fallback would reject every capture in this domain.
            instant: Mutex::new(
                Instant::now()
                    .checked_add(Duration::from_secs(3_600))
                    .unwrap(),
            ),
            reads: AtomicU64::new(0),
        })
    }
    fn instant(&self) -> Instant {
        *self.instant.lock().unwrap()
    }
    fn advance_to(&self, next: Instant) {
        let mut instant = self.instant.lock().unwrap();
        assert!(next >= *instant);
        *instant = next;
    }
    fn reads(&self) -> u64 {
        self.reads.load(Ordering::SeqCst)
    }
}
impl CaptureClock for CaptureTime {
    fn now(&self) -> Instant {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.instant()
    }
}

fn configuration() -> SessionConfig {
    SessionConfig::new(
        GOOGLE_ENDPOINT,
        Credential::api_key("fixture-capture-clock").unwrap(),
    )
    .unwrap()
}
fn request(id: u64) -> RequestId {
    RequestId::new(id).unwrap()
}

async fn accept(
    attempt: JoinHandle<Result<Connection>>,
    mut service: FakeService,
) -> (Connection, FakeService, Value) {
    let setup = service.receive().await;
    assert_eq!(
        setup["setup"]["realtimeInputConfig"]["automaticActivityDetection"]["disabled"],
        true
    );
    assert!(!setup.to_string().contains("fixture-capture-clock"));
    assert!(!attempt.is_finished(), "setup has not been acknowledged");
    service.send(json!({"setupComplete": {}})).await;
    let connection = attempt.await.unwrap().unwrap();
    assert_eq!(connection.state(), State::Ready);
    (connection, service, setup)
}

async fn open(config: SessionConfig, clock: &Arc<CaptureTime>) -> (Connection, FakeService) {
    let (attempt, service) = testing::pair_with_capture_clock(
        config,
        SessionId::new(17).unwrap(),
        MAX_WIRE_BYTES * 2,
        clock.clone(),
    )
    .await;
    let (connection, service, _) = accept(tokio::spawn(attempt), service).await;
    (connection, service)
}

async fn event(connection: &mut Connection) -> Event {
    timeout(Duration::from_secs(2), connection.next_event())
        .await
        .unwrap()
        .unwrap()
}

async fn started(connection: &mut Connection, service: &mut FakeService, id: u64) -> RequestId {
    let request = request(id);
    connection.input().try_start(request).unwrap();
    assert!(service.receive().await["realtimeInput"]["activityStart"].is_object());
    assert!(matches!(
        event(connection).await,
        Event::InputStarted { lineage, waiting_for_barrier: false } if lineage.request == request
    ));
    request
}

async fn ended(connection: &mut Connection, service: &mut FakeService, error: Error) {
    // A state observation alone does not mean the Actor has finished draining.
    let mut state = connection.subscribe_state();
    timeout(Duration::from_secs(2), async {
        loop {
            if let State::Disconnected(actual) = *state.borrow_and_update() {
                assert_eq!(actual, error);
                break;
            }
            state.changed().await.unwrap();
        }
        assert!(connection.next_event().await.is_none());
        (&mut connection.task).await.unwrap();
    })
    .await
    .unwrap();
    // The real Actor and its socket have ended. Buffered input would still be
    // readable here; a clock sample or an empty live socket was not the fence.
    assert!(service.try_receive(Duration::from_secs(2)).await.is_none());
    assert_eq!(connection.state(), State::Disconnected(error));
}

async fn close(mut connection: Connection) {
    connection.shutdown();
    timeout(Duration::from_secs(2), async {
        while connection.next_event().await.is_some() {}
        (&mut connection.task).await.unwrap();
    })
    .await
    .unwrap();
}

async fn marker(connection: &mut Connection, service: &mut FakeService, text: &str) {
    service
        .send(json!({"serverContent":{"inputTranscription":{"text":text,"finished":true}}}))
        .await;
    assert!(matches!(
        event(connection).await,
        Event::UncorrelatedInputTranscript { text: actual, finished: true, .. } if actual == text
    ));
}

#[tokio::test(start_paused = true)]
async fn custom_setup_and_sender_clones_share_one_pure_capture_clock() {
    let clock = CaptureTime::new();
    let (mut connection, mut service) = open(configuration(), &clock).await;
    let request = started(&mut connection, &mut service, 1).await;
    assert_eq!(clock.reads(), 0, "setup and Start do not read capture time");
    let now = clock.instant();
    let protocol_now = Clock::now();
    assert_eq!(clock.now(), now);
    assert_eq!(clock.now(), now);
    assert_eq!(Clock::now(), protocol_now);
    let input = connection.input();
    let other = input.clone();
    input.try_audio(request, 0, now, &[41; 160]).unwrap();
    other.try_audio(request, 1, now, &[42; 160]).unwrap();
    assert_eq!(
        testing::sent_audio(&service.receive().await),
        Some(vec![41; 160])
    );
    assert_eq!(
        testing::sent_audio(&service.receive().await),
        Some(vec![42; 160])
    );
    assert_eq!(
        clock.reads(),
        8,
        "two reads plus three per actual audio block"
    );
    assert_eq!(clock.instant(), now);
    close(connection).await;
}

#[tokio::test(start_paused = true)]
async fn every_existing_factory_keeps_system_capture_time_with_fixture_support() {
    let clock = CaptureTime::new();
    let (mut fixture, mut fixture_service) = open(configuration(), &clock).await;
    let fixture_request = started(&mut fixture, &mut fixture_service, 1).await;
    let future_capture = clock.instant();
    fixture
        .input()
        .try_audio(fixture_request, 0, future_capture, &[7; 160])
        .unwrap();
    assert_eq!(
        testing::sent_audio(&fixture_service.receive().await),
        Some(vec![7; 160])
    );

    // Keep the fixture alive while each old factory creates a separate session.
    for factory in 0..3 {
        let (mut connection, mut service, _) = if factory == 0 {
            let (attempt, service) = testing::pair(
                configuration(),
                SessionId::new(18).unwrap(),
                MAX_WIRE_BYTES * 2,
            )
            .await;
            accept(tokio::spawn(attempt), service).await
        } else {
            let (mut connector, mut dials) = if factory == 1 {
                testing::connector(configuration())
            } else {
                testing::connector_with_capacity(configuration(), MAX_WIRE_BYTES * 2)
            };
            let attempt = tokio::spawn(connector.connect(SessionId::new(18).unwrap(), None));
            let dial = testing::next_dial(&mut dials).await;
            assert!(!dial.resumed);
            accept(attempt, dial.service).await
        };
        let request = started(&mut connection, &mut service, 1).await;
        assert_eq!(
            connection
                .input()
                .try_audio(request, 0, future_capture, &[8; 160]),
            Err(Error::StaleInput)
        );
        connection
            .input()
            .try_audio(request, 0, Instant::now(), &[9; 160])
            .unwrap();
        assert_eq!(
            testing::sent_audio(&service.receive().await),
            Some(vec![9; 160])
        );
        close(connection).await;
    }
    assert_eq!(
        clock.reads(),
        3,
        "default sessions never sample the fixture"
    );
    close(fixture).await;
}

#[tokio::test(start_paused = true)]
async fn audio_offer_keeps_the_exact_full_bound_and_refusal_order() {
    let clock = CaptureTime::new();
    let (mut connection, mut service) = open(configuration(), &clock).await;
    let request = started(&mut connection, &mut service, 1).await;
    let input = connection.input();
    let now = clock.instant();
    input
        .try_audio(
            request,
            0,
            now - MAX_INPUT_AGE + Duration::from_nanos(1),
            &[1; 160],
        )
        .unwrap();
    assert_eq!(
        input.try_audio(request, 1, now - MAX_INPUT_AGE, &[2]),
        Err(Error::StaleInput)
    );
    assert_eq!(
        input.try_audio(request, 1, now + Duration::from_nanos(1), &[2]),
        Err(Error::StaleInput)
    );
    assert_eq!(
        input.try_audio(request, 1, now, &[]),
        Err(Error::InvalidAudio)
    );
    assert_eq!(
        input.try_audio(request, 1, now + Duration::from_nanos(1), &[]),
        Err(Error::StaleInput)
    );
    assert_eq!(clock.reads(), 5);
    assert_eq!(
        testing::sent_audio(&service.receive().await),
        Some(vec![1; 160])
    );
    assert_eq!(clock.reads(), 7);
    input.retire(request);
    assert_eq!(
        input.try_audio(request, 1, now, &[]),
        Err(Error::StaleRequest)
    );
    input.shutdown();
    assert_eq!(input.try_audio(request, 1, now, &[]), Err(Error::Closed));
    assert_eq!(
        clock.reads(),
        7,
        "ready and retirement guards precede the clock"
    );
    close(connection).await;
}

#[tokio::test(start_paused = true)]
async fn queued_audio_is_checked_again_by_the_actor_at_the_exact_bound() {
    let clock = CaptureTime::new();
    let (mut connection, mut service) = open(configuration(), &clock).await;
    let request = started(&mut connection, &mut service, 1).await;
    let captured = clock.instant();
    connection
        .input()
        .try_audio(request, 0, captured, &[5; 160])
        .unwrap();
    // No await: the current-thread Actor cannot consume the queued audio yet.
    clock.advance_to(captured + MAX_INPUT_AGE);
    ended(&mut connection, &mut service, Error::StaleInput).await;
    assert_eq!(
        clock.reads(),
        2,
        "offer and failing accept; no post-accept read"
    );
}

#[tokio::test(start_paused = true)]
async fn sequence_and_capture_order_fail_after_one_actor_read_not_two() {
    for capture_regresses in [false, true] {
        let clock = CaptureTime::new();
        let (mut connection, mut service) = open(configuration(), &clock).await;
        let request = started(&mut connection, &mut service, 1).await;
        let first = clock.instant() - Duration::from_millis(1);
        connection
            .input()
            .try_audio(request, 10, first, &[10; 160])
            .unwrap();
        assert_eq!(
            testing::sent_audio(&service.receive().await),
            Some(vec![10; 160])
        );
        let (sequence, captured) = if capture_regresses {
            (11, first - Duration::from_nanos(1))
        } else {
            (12, first)
        };
        connection
            .input()
            .try_audio(request, sequence, captured, &[11; 160])
            .unwrap();
        ended(&mut connection, &mut service, Error::InputSequence).await;
        assert_eq!(
            clock.reads(),
            5,
            "three for sent audio, two for refused audio"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn image_offer_and_actual_send_keep_the_full_bound_and_refusal_order() {
    let clock = CaptureTime::new();
    let (mut connection, mut service) = open(configuration(), &clock).await;
    let speaker = started(&mut connection, &mut service, 1).await;
    let input = connection.input();
    let now = clock.instant();
    let image = testing::jpeg(100, 3);
    assert_eq!(
        input.try_image(
            speaker,
            41,
            1,
            now - MAX_IMAGE_AGE + Duration::from_nanos(1),
            &image
        ),
        Ok(ImageOffer::Queued)
    );
    assert_eq!(
        input.try_image(speaker, 42, 1, now - MAX_IMAGE_AGE, &image),
        Err(ImageRefusal::TooOld)
    );
    assert_eq!(
        input.try_image(speaker, 42, 1, now + Duration::from_nanos(1), &image),
        Err(ImageRefusal::TooOld)
    );
    assert_eq!(
        input.try_image(request(2), 42, 1, now + Duration::from_nanos(1), &image),
        Err(ImageRefusal::TooOld)
    );
    assert_eq!(
        input.try_image(request(2), 42, 1, now, &image),
        Err(ImageRefusal::NotCurrent)
    );
    assert_eq!(
        input.try_image(speaker, 42, 1, now, &testing::jpeg(MAX_IMAGE_BYTES + 1, 3)),
        Err(ImageRefusal::TooLarge)
    );
    assert_eq!(
        input.try_image(speaker, 42, 1, now, b"not a JPEG"),
        Err(ImageRefusal::NotJpeg)
    );
    assert_eq!(clock.reads(), 5);
    let message = service.receive().await;
    let video = &message["realtimeInput"]["video"];
    assert_eq!(video["mimeType"], "image/jpeg");
    assert_eq!(
        STANDARD.decode(video["data"].as_str().unwrap()).unwrap(),
        image
    );
    let ledger = input.image_ledger();
    assert_eq!((ledger.accepted, ledger.sent, ledger.dropped), (1, 1, 0));
    let receipt = ledger.last_sent.unwrap();
    assert_eq!(
        (receipt.request, receipt.frame, receipt.bytes),
        (speaker, 41, 100)
    );
    assert_eq!(clock.reads(), 6);
    input.shutdown();
    assert_eq!(
        input.try_image(speaker, 42, 1, now, &image),
        Err(ImageRefusal::NotConnected)
    );
    assert_eq!(clock.reads(), 6);
    close(connection).await;
}

#[tokio::test(start_paused = true)]
async fn pending_image_expires_on_an_actual_actor_turn_and_leaves_audio_live() {
    let clock = CaptureTime::new();
    let (mut connection, mut service) = open(configuration(), &clock).await;
    let speaker = started(&mut connection, &mut service, 1).await;
    let input = connection.input();
    let captured = clock.instant();
    assert_eq!(
        input.try_image(speaker, 1, 1, captured, &testing::jpeg(100, 3)),
        Ok(ImageOffer::Queued)
    );
    clock.advance_to(captured + MAX_IMAGE_AGE);
    // Two actually handled server messages force an intervening Actor loop
    // top, where send_image decides the pending frame. Neither marker changes
    // the current request, grant or command queue.
    marker(&mut connection, &mut service, "first image fence").await;
    marker(&mut connection, &mut service, "second image fence").await;
    let ledger = input.image_ledger();
    assert_eq!((ledger.accepted, ledger.sent, ledger.dropped), (1, 0, 1));
    assert!(ledger.last_sent.is_none());
    assert_eq!(
        clock.reads(),
        2,
        "offer and the actual pending-image decision"
    );
    input
        .try_audio(speaker, 0, clock.instant(), &[19; 160])
        .unwrap();
    assert_eq!(
        testing::sent_audio(&service.receive().await),
        Some(vec![19; 160])
    );
    assert_eq!(connection.state(), State::Ready);
    close(connection).await;
}

async fn kept_audio(clock: &Arc<CaptureTime>, patience: Duration) -> (Connection, FakeService) {
    let config = configuration()
        .successor_start(SuccessorStart::AfterFirstOutput { patience })
        .unwrap();
    let (mut connection, mut service) = open(config, clock).await;
    let first = started(&mut connection, &mut service, 1).await;
    let input = connection.input();
    input
        .try_audio(first, 0, clock.instant(), &[1; 160])
        .unwrap();
    assert_eq!(
        testing::sent_audio(&service.receive().await),
        Some(vec![1; 160])
    );
    input.try_end(first).unwrap();
    assert!(service.receive().await["realtimeInput"]["activityEnd"].is_object());
    let next = request(2);
    input.try_start(next).unwrap();
    assert!(matches!(
        event(&mut connection).await,
        Event::InputStarted { lineage, waiting_for_barrier: true } if lineage.request == next
    ));
    input
        .try_audio(next, 0, clock.instant(), &[23; 160])
        .unwrap();
    input
        .try_tool_response("capture-fence", "fixture", &json!({}))
        .unwrap();
    // This actual FIFO command receipt follows completion of the held Audio
    // command. It neither releases the start nor retires the successor.
    assert!(matches!(
        event(&mut connection).await,
        Event::Discarded {
            reason: Discard::StaleToolResponse,
            ..
        }
    ));
    assert_eq!(clock.reads(), 6);
    (connection, service)
}

#[tokio::test(start_paused = true)]
async fn held_audio_is_written_one_nanosecond_inside_its_full_extended_bound() {
    let clock = CaptureTime::new();
    let captured = clock.instant();
    let patience = Duration::from_secs(2);
    let (connection, mut service) = kept_audio(&clock, patience).await;
    clock.advance_to(captured + MAX_INPUT_AGE + patience - Duration::from_nanos(1));
    service.send(testing::audio(7, 1)).await;
    assert!(service.receive().await["realtimeInput"]["activityStart"].is_object());
    // The small in-memory write is immediately writable; paused Tokio does
    // not consume its original one-nanosecond remaining write budget.
    assert_eq!(
        testing::sent_audio(&service.receive().await),
        Some(vec![23; 160])
    );
    assert_eq!(
        clock.reads(),
        7,
        "the retained block has its own fresh sample"
    );
    assert_eq!(connection.state(), State::Ready);
    close(connection).await;
}

#[tokio::test(start_paused = true)]
async fn held_audio_at_its_full_extended_bound_fails_before_any_pcm_write() {
    let clock = CaptureTime::new();
    let captured = clock.instant();
    let patience = Duration::from_secs(2);
    let (mut connection, mut service) = kept_audio(&clock, patience).await;
    clock.advance_to(captured + MAX_INPUT_AGE + patience);
    service.send(testing::audio(7, 1)).await;
    // Original ordering releases Start before checking the first held block.
    assert!(service.receive().await["realtimeInput"]["activityStart"].is_object());
    ended(&mut connection, &mut service, Error::StaleInput).await;
    assert_eq!(clock.reads(), 7);
}

#[tokio::test(start_paused = true)]
async fn capture_updates_do_not_move_tokio_or_prevent_its_read_deadline() {
    let clock = CaptureTime::new();
    let timeouts = Timeouts {
        read: Duration::from_millis(50),
        keepalive: Duration::from_millis(10),
        ..Timeouts::default()
    };
    let (mut connection, mut service) =
        open(configuration().timeouts(timeouts).unwrap(), &clock).await;
    let protocol_now = Clock::now();
    clock.advance_to(clock.instant() + Duration::from_secs(3_600));
    assert_eq!(Clock::now(), protocol_now);
    assert_eq!(connection.state(), State::Ready);
    tokio::time::advance(timeouts.read).await;
    ended(&mut connection, &mut service, Error::ReadTimeout).await;
    assert_eq!(clock.reads(), 0, "protocol timing never reads capture time");
}

#[tokio::test(start_paused = true)]
async fn connector_attempts_share_the_explicit_clock_and_keep_setup_metadata() {
    let clock = CaptureTime::new();
    let (mut connector, mut dials) = testing::connector_with_capture_clock(
        configuration().resumption(Resumption::Retain),
        MAX_WIRE_BYTES * 2,
        clock.clone(),
    );
    for id in 1..=2 {
        let resume = (id == 2).then(|| ResumptionHandle::new("fixture-resume").unwrap());
        let session = SessionId::new(id).unwrap();
        let attempt = tokio::spawn(connector.connect(session, resume));
        let dial = testing::next_dial(&mut dials).await;
        assert_eq!(dial.session, session);
        assert_eq!(dial.resumed, id == 2);
        let (mut connection, mut service, setup) = accept(attempt, dial.service).await;
        if id == 2 {
            assert_eq!(
                setup["setup"]["sessionResumption"]["handle"],
                "fixture-resume"
            );
        } else {
            assert!(setup["setup"].get("sessionResumption").is_none());
        }
        let request = started(&mut connection, &mut service, 1).await;
        connection
            .input()
            .try_audio(request, 0, clock.instant(), &[13; 160])
            .unwrap();
        assert_eq!(
            testing::sent_audio(&service.receive().await),
            Some(vec![13; 160])
        );
        assert_eq!(clock.reads(), id * 3);
        close(connection).await;
        clock.advance_to(clock.instant() + Duration::from_secs(1));
    }
}

#[tokio::test(start_paused = true)]
async fn connector_keeps_the_requested_duplex_capacity_and_original_write_timeout() {
    let clock = CaptureTime::new();
    let config = configuration()
        .timeouts(Timeouts {
            write: Duration::from_millis(50),
            ..Timeouts::default()
        })
        .unwrap();
    let (mut connector, mut dials) =
        testing::connector_with_capture_clock(config, 64, clock.clone());
    let attempt = tokio::spawn(connector.connect(SessionId::new(1).unwrap(), None));
    let dial = testing::next_dial(&mut dials).await;
    let (mut connection, mut service, _) = accept(attempt, dial.service).await;
    let request = started(&mut connection, &mut service, 1).await;
    connection
        .input()
        .try_audio(request, 0, clock.instant(), &[5; MAX_INPUT_SAMPLES])
        .unwrap();
    // Do not drain the 64-byte peer. Its capacity cannot hold this real frame.
    // A silently substituted default capacity would complete the write instead.
    // The drop names the audio, the message's size and the requested bound.
    let kib = u32::try_from(
        crate::wire::encode_audio(&[5; MAX_INPUT_SAMPLES])
            .unwrap()
            .len()
            .div_ceil(1024),
    )
    .unwrap();
    let write = Error::WriteTimeout {
        write: crate::Write::Audio,
        kib,
        deadline_ms: 50,
    };
    ended(&mut connection, &mut service, write).await;
    assert_eq!(clock.reads(), 3);
}
