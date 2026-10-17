//! Visual context: one bounded image slot beside the audio queue.
use super::*;
use crate::{
    ImageOffer, ImageRefusal, ImageRevocation, MAX_IMAGE_AGE, MAX_IMAGE_BYTES,
    MAX_IMAGES_PER_REQUEST, testing::jpeg,
};

/// A request that is speaking: started, with one block already sent.
async fn speaking(connection: &mut Connection, server: &mut MockSocket, id: u64) -> RequestId {
    let request = request(id);
    let input = connection.input();
    input.try_start(request).unwrap();
    sent(server, "activityStart").await;
    assert!(matches!(
        event(connection).await,
        Event::InputStarted { .. }
    ));
    input
        .try_audio(request, 0, Instant::now(), &[100; 160])
        .unwrap();
    sent(server, "audio").await;
    request
}
fn picture(message: &Value) -> Vec<u8> {
    let video = &message["realtimeInput"]["video"];
    assert_eq!(video["mimeType"], "image/jpeg", "{message}");
    STANDARD.decode(video["data"].as_str().unwrap()).unwrap()
}
/// First sample of an audio message, which the tests use as a block label.
fn label(message: &Value) -> i16 {
    let pcm = STANDARD
        .decode(
            message["realtimeInput"]["audio"]["data"]
                .as_str()
                .unwrap_or_else(|| panic!("expected audio, received {message}")),
        )
        .unwrap();
    i16::from_le_bytes([pcm[0], pcm[1]])
}

#[tokio::test]
async fn image_is_sent_once_inside_its_request_after_audio_already_queued() {
    let (mut connection, mut server) = mock(Timeouts::default()).await;
    let request = speaking(&mut connection, &mut server, 1).await;
    let input = connection.input();
    // The largest image allowed, offered between two audio blocks.
    let image = jpeg(MAX_IMAGE_BYTES, 7);
    input
        .try_audio(request, 1, Instant::now(), &[101; 160])
        .unwrap();
    assert_eq!(
        input.try_image(request, 41, 1, Instant::now(), &image),
        Ok(ImageOffer::Queued)
    );
    input
        .try_audio(request, 2, Instant::now(), &[102; 160])
        .unwrap();
    // Audio that was queued goes first; the image does not jump it.
    assert_eq!(label(&receive(&mut server).await), 101);
    assert_eq!(label(&receive(&mut server).await), 102);
    assert_eq!(picture(&receive(&mut server).await), image);
    input.try_end(request).unwrap();
    sent(&mut server, "activityEnd").await;
    silent(&mut server).await;
    let ledger = input.image_ledger();
    assert_eq!(
        (
            ledger.accepted,
            ledger.sent,
            ledger.replaced,
            ledger.dropped
        ),
        (1, 1, 0, 0)
    );
    let receipt = ledger.last_sent.unwrap();
    assert_eq!(
        (receipt.request, receipt.frame, receipt.bytes),
        (request, 41, MAX_IMAGE_BYTES as u32)
    );
    assert_eq!(connection.state(), State::Ready);
}

#[tokio::test]
async fn a_refused_image_changes_nothing_else() {
    let (mut connection, mut server) = mock(Timeouts::default()).await;
    let input = connection.input();
    let image = jpeg(2_000, 3);
    let now = Instant::now;
    assert_eq!(
        input.try_image(request(1), 0, 1, now(), &image),
        Err(ImageRefusal::NotCurrent),
        "nobody is speaking"
    );
    let speaker = speaking(&mut connection, &mut server, 1).await;
    for (wrong, reason) in [
        (jpeg(MAX_IMAGE_BYTES + 1, 3), ImageRefusal::TooLarge),
        (vec![0x89, b'P', b'N', b'G', 0, 0], ImageRefusal::NotJpeg),
        // Cut off before its end marker.
        (image[..image.len() - 2].to_vec(), ImageRefusal::NotJpeg),
    ] {
        assert_eq!(input.try_image(speaker, 0, 1, now(), &wrong), Err(reason));
    }
    assert_eq!(
        input.try_image(speaker, 0, 1, now() - MAX_IMAGE_AGE, &image),
        Err(ImageRefusal::TooOld)
    );
    assert_eq!(
        input.try_image(speaker, 0, 1, now() + Duration::from_secs(1), &image),
        Err(ImageRefusal::TooOld),
        "a capture time in the future is not fresh"
    );
    assert_eq!(
        input.try_image(request(2), 0, 1, now(), &image),
        Err(ImageRefusal::NotCurrent)
    );
    // Up to the limit, then no more for this request.
    for frame in 0..u64::from(MAX_IMAGES_PER_REQUEST) {
        assert_eq!(
            input.try_image(speaker, frame, 1, now(), &image),
            Ok(ImageOffer::Queued)
        );
        assert_eq!(picture(&receive(&mut server).await), image);
    }
    assert_eq!(
        input.try_image(speaker, 9, 1, now(), &image),
        Err(ImageRefusal::LimitReached)
    );
    // None of that disturbed the request's audio or the connection.
    input.try_audio(speaker, 1, now(), &[101; 160]).unwrap();
    assert_eq!(label(&receive(&mut server).await), 101);
    input.try_end(speaker).unwrap();
    sent(&mut server, "activityEnd").await;
    assert_eq!(
        input.try_image(speaker, 9, 1, now(), &image),
        Err(ImageRefusal::NotCurrent),
        "its input has ended"
    );
    silent(&mut server).await;
    assert_eq!(connection.state(), State::Ready);
    let ledger = input.image_ledger();
    assert_eq!((ledger.accepted, ledger.sent), (2, 2));
    input.shutdown();
    assert_eq!(
        input.try_image(speaker, 9, 1, now(), &image),
        Err(ImageRefusal::NotConnected)
    );
}

#[tokio::test]
async fn a_newer_image_replaces_one_not_yet_sent_and_counts_once() {
    let (mut connection, mut server) = mock(Timeouts::default()).await;
    let request = speaking(&mut connection, &mut server, 1).await;
    let input = connection.input();
    let (older, newer, last) = (jpeg(3_000, 1), jpeg(3_000, 2), jpeg(3_000, 3));
    // Both offers land before the connection has had a turn to send.
    assert_eq!(
        input.try_image(request, 1, 1, Instant::now(), &older),
        Ok(ImageOffer::Queued)
    );
    assert_eq!(
        input.try_image(request, 2, 1, Instant::now(), &newer),
        Ok(ImageOffer::ReplacedUnsent)
    );
    assert_eq!(picture(&receive(&mut server).await), newer);
    // The replaced one did not use up the request's allowance.
    assert_eq!(
        input.try_image(request, 3, 1, Instant::now(), &last),
        Ok(ImageOffer::Queued)
    );
    assert_eq!(picture(&receive(&mut server).await), last);
    assert_eq!(
        input.try_image(request, 4, 1, Instant::now(), &last),
        Err(ImageRefusal::LimitReached)
    );
    silent(&mut server).await;
    let ledger = input.image_ledger();
    assert_eq!(
        (
            ledger.accepted,
            ledger.sent,
            ledger.replaced,
            ledger.dropped
        ),
        (3, 2, 1, 0)
    );
    assert_eq!(ledger.last_sent.unwrap().frame, 3);
}

#[tokio::test]
async fn an_image_whose_request_ended_or_was_cancelled_first_is_dropped_unsent() {
    // The person finished speaking before the image could be sent.
    let (mut connection, mut server) = mock(Timeouts::default()).await;
    let request = speaking(&mut connection, &mut server, 1).await;
    let input = connection.input();
    let image = jpeg(3_000, 5);
    assert_eq!(
        input.try_image(request, 1, 1, Instant::now(), &image),
        Ok(ImageOffer::Queued)
    );
    input.try_end(request).unwrap();
    sent(&mut server, "activityEnd").await;
    silent(&mut server).await;
    let ledger = input.image_ledger();
    assert_eq!((ledger.sent, ledger.dropped), (0, 1));

    // The request was cancelled locally with its image still waiting.
    let (mut connection, mut server) = mock(Timeouts::default()).await;
    let request = speaking(&mut connection, &mut server, 1).await;
    let input = connection.input();
    assert_eq!(
        input.try_image(request, 1, 1, Instant::now(), &image),
        Ok(ImageOffer::Queued)
    );
    input.retire(request);
    silent(&mut server).await;
    let ledger = input.image_ledger();
    assert_eq!((ledger.sent, ledger.dropped), (0, 1));
    assert_eq!(connection.state(), State::Ready);
}

/// A connection whose peer has room for only `capacity` unread bytes, so a
/// large write does not complete until the peer reads.
async fn narrow(capacity: usize) -> (Connection, MockSocket) {
    let (client, mut server) = pair(capacity).await;
    let connect = tokio::spawn(setup_and_spawn(
        client,
        configuration(Timeouts::default()),
        SessionId::new(17).unwrap(),
    ));
    receive(&mut server).await;
    server
        .send(Message::Binary(br#"{"setupComplete":{}}"#.to_vec().into()))
        .await
        .unwrap();
    (connect.await.unwrap().unwrap(), server)
}

#[tokio::test]
async fn audio_offered_during_a_slow_image_write_is_held_and_sent_in_order() {
    let (mut connection, mut server) = narrow(4_096).await;
    let request = speaking(&mut connection, &mut server, 1).await;
    let input = connection.input();
    let image = jpeg(MAX_IMAGE_BYTES, 7);
    assert_eq!(
        input.try_image(request, 1, 1, Instant::now(), &image),
        Ok(ImageOffer::Queued)
    );
    // The image is far larger than the peer has room for, so its write is
    // still in progress. More audio arrives than the command queue alone
    // could hold; none of it may be refused or lost.
    const BLOCKS: i16 = 60;
    assert!(BLOCKS as usize > crate::INPUT_QUEUE_CAPACITY);
    for block in 1..=BLOCKS {
        tokio::task::yield_now().await;
        input
            .try_audio(request, block as u64, Instant::now(), &[100 + block; 160])
            .unwrap_or_else(|error| panic!("block {block} was refused: {error}"));
    }
    assert_eq!(picture(&receive(&mut server).await), image);
    for block in 1..=BLOCKS {
        assert_eq!(label(&receive(&mut server).await), 100 + block);
    }
    input.try_end(request).unwrap();
    sent(&mut server, "activityEnd").await;
    assert_eq!(connection.state(), State::Ready);
    let receipt = input.image_ledger().last_sent.unwrap();
    assert!(receipt.written_in <= IMAGE_WRITE, "{receipt:?}");
    println!(
        "MEASURED image_write bytes={} held_audio_blocks={BLOCKS} written_in_ms={:.1} waited_ms={:.1} (in-memory peer with 4 KiB of room, host)",
        receipt.bytes,
        receipt.written_in.as_secs_f64() * 1e3,
        receipt.waited.as_secs_f64() * 1e3
    );
}

#[tokio::test(start_paused = true)]
async fn an_image_the_link_cannot_take_fails_the_connection_within_its_write_bound() {
    let (mut connection, mut server) = narrow(4_096).await;
    let request = speaking(&mut connection, &mut server, 1).await;
    let input = connection.input();
    let offered = Clock::now();
    assert_eq!(
        input.try_image(request, 1, 1, Instant::now(), &jpeg(MAX_IMAGE_BYTES, 7)),
        Ok(ImageOffer::Queued)
    );
    // The peer never reads. The drop names the image, its size on the wire
    // and its bound (lampOS#705).
    let kib = u32::try_from(
        crate::wire::encode_image(&jpeg(MAX_IMAGE_BYTES, 7))
            .unwrap()
            .len()
            .div_ceil(1024),
    )
    .unwrap();
    assert_eq!(
        disconnected(&connection).await,
        Error::WriteTimeout {
            write: crate::Write::Image,
            kib,
            deadline_ms: 400
        }
    );
    let took = offered.elapsed();
    assert!(
        took >= IMAGE_WRITE && took <= IMAGE_WRITE + Duration::from_millis(50),
        "{took:?}"
    );
    // A write timeout is a link failure a supervisor reconnects from.
    assert_eq!(
        Error::WriteTimeout {
            write: crate::Write::Image,
            kib,
            deadline_ms: 400
        }
        .recovery(),
        crate::Recovery::Reconnect
    );
    drop(server);
}

/// A 64 KiB image stuck in its write behind a peer with 4 KiB of room, and
/// one audio block and the request's End held locally behind it.
async fn blocked_image_with_audio_held_behind_it() -> (Connection, MockSocket, InputSender) {
    let (mut connection, mut server) = narrow(4_096).await;
    let request = speaking(&mut connection, &mut server, 1).await;
    let input = connection.input();
    input
        .try_image(request, 91, 1, Instant::now(), &jpeg(MAX_IMAGE_BYTES, 7))
        .unwrap();
    timeout(Duration::from_secs(1), async {
        while input.shared.images().in_flight.is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the image entered its blocked write");
    input
        .try_audio(request, 1, Instant::now(), &[1234; 160])
        .unwrap();
    input.try_end(request).unwrap();
    timeout(Duration::from_secs(1), async {
        while input.commands.capacity() != crate::INPUT_QUEUE_CAPACITY {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("audio and End are held locally while the image cannot finish");
    assert_eq!(input.image_ledger().sent, 0);
    (connection, server, input)
}

/// Everything the peer still receives, by kind, until the connection closes.
async fn received_until_closed(server: &mut MockSocket) -> Vec<&'static str> {
    let mut kinds = Vec::new();
    loop {
        match timeout(Duration::from_secs(1), server.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let message: Value = serde_json::from_str(&text).unwrap();
                for kind in ["audio", "activityEnd", "video"] {
                    if message["realtimeInput"].get(kind).is_some() {
                        kinds.push(kind);
                    }
                }
            }
            Ok(Some(Ok(_))) => {}
            Ok(None | Some(Err(_))) => return kinds,
            Err(_) => panic!("the connection was not closed"),
        }
    }
}

// Found in review by the integrator, whose regression this is.
#[tokio::test]
async fn shutdown_during_a_blocked_image_write_sends_nothing_that_was_held_behind_it() {
    let (connection, mut server, input) = blocked_image_with_audio_held_behind_it().await;
    input.shutdown();
    assert_eq!(connection.state(), State::Closed);
    // Part of the image may already be on the wire. The peer now reads; no
    // microphone audio and no request for a response may follow a shutdown,
    // and the image itself is never completed.
    let after = received_until_closed(&mut server).await;
    assert!(after.is_empty(), "sent after shutdown: {after:?}");
    assert_eq!(input.image_ledger().sent, 0);
}

#[tokio::test]
async fn a_revoked_camera_grant_withdraws_a_waiting_image_and_leaves_the_request_alone() {
    let (mut connection, mut server) = mock(Timeouts::default()).await;
    let request = speaking(&mut connection, &mut server, 1).await;
    let input = connection.input();
    let image = jpeg(3_000, 5);
    assert_eq!(input.revoke_images(0), ImageRevocation::Nothing);
    // Offered under grant 7 and revoked before the connection has had a turn.
    assert_eq!(
        input.try_image(request, 1, 7, Instant::now(), &image),
        Ok(ImageOffer::Queued)
    );
    assert_eq!(input.revoke_images(7), ImageRevocation::DroppedUnsent);
    // Nothing more under that grant or an older one, however fresh the frame.
    for grant in [7, 3] {
        assert_eq!(
            input.try_image(request, 2, grant, Instant::now(), &image),
            Err(ImageRefusal::Revoked)
        );
    }
    // The person's audio never noticed.
    input
        .try_audio(request, 1, Instant::now(), &[101; 160])
        .unwrap();
    assert_eq!(label(&receive(&mut server).await), 101);
    silent(&mut server).await;
    // A new permission is a new grant, and its frames are accepted again.
    assert_eq!(
        input.try_image(request, 3, 8, Instant::now(), &image),
        Ok(ImageOffer::Queued)
    );
    assert_eq!(picture(&receive(&mut server).await), image);
    input.try_end(request).unwrap();
    sent(&mut server, "activityEnd").await;
    let ledger = input.image_ledger();
    assert_eq!((ledger.sent, ledger.revoked, ledger.dropped), (1, 1, 0));
    assert_eq!(ledger.last_sent.unwrap().frame, 3);
    assert_eq!(connection.state(), State::Ready);
}

#[tokio::test]
async fn revoking_the_grant_of_an_image_being_written_abandons_its_connection() {
    let (connection, mut server, input) = blocked_image_with_audio_held_behind_it().await;
    // A grant the image was not offered under changes nothing.
    assert_eq!(input.revoke_images(0), ImageRevocation::Nothing);
    assert_eq!(connection.state(), State::Ready);
    assert_eq!(input.revoke_images(1), ImageRevocation::AbandonedInFlight);
    assert_eq!(disconnected(&connection).await, Error::InputCancelled);
    // The image is never completed and nothing held behind it is sent.
    let after = received_until_closed(&mut server).await;
    assert!(after.is_empty(), "sent after revocation: {after:?}");
    let ledger = input.image_ledger();
    assert_eq!((ledger.sent, ledger.revoked), (0, 1));
    // The connection is replaced like any lost one; the request is lost once.
    assert_eq!(Error::InputCancelled.recovery(), crate::Recovery::Reconnect);
}

/// A transport that revokes the camera grant from inside the last flush of
/// the image write: after the connection has looked for a revocation in that
/// poll, and before it learns the write succeeded. The integrator's audit
/// built this to show the write winning; it is kept as the regression.
struct RevokeAtImageFlush {
    inner: DuplexStream,
    input: Arc<Mutex<Option<InputSender>>>,
    outcome: Arc<Mutex<Option<ImageRevocation>>>,
    fired: bool,
}
impl AsyncRead for RevokeAtImageFlush {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_read(cx, buffer)
    }
}
impl AsyncWrite for RevokeAtImageFlush {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.inner).poll_write(cx, bytes)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let result = std::pin::Pin::new(&mut self.inner).poll_flush(cx);
        if !self.fired && matches!(result, std::task::Poll::Ready(Ok(()))) {
            let input = self.input.lock().unwrap().clone();
            if let Some(input) = input
                && input.shared.images().in_flight.is_some()
            {
                self.fired = true;
                *self.outcome.lock().unwrap() = Some(input.revoke_images(1));
            }
        }
        result
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[tokio::test]
async fn a_revocation_that_lands_as_the_image_write_completes_still_abandons_the_connection() {
    let (client, server) = tokio::io::duplex(4_096);
    let slot = Arc::new(Mutex::new(None));
    let outcome = Arc::new(Mutex::new(None));
    let client = WebSocketStream::from_raw_socket(
        RevokeAtImageFlush {
            inner: client,
            input: slot.clone(),
            outcome: outcome.clone(),
            fired: false,
        },
        Role::Client,
        Some(socket_config()),
    )
    .await;
    let mut server =
        WebSocketStream::from_raw_socket(server, Role::Server, Some(socket_config())).await;
    let connect = tokio::spawn(setup_and_spawn(
        client,
        configuration(Timeouts::default()),
        SessionId::new(17).unwrap(),
    ));
    receive(&mut server).await;
    send_json(&mut server, json!({"setupComplete":{}})).await;
    let mut connection = connect.await.unwrap().unwrap();
    let request = speaking(&mut connection, &mut server, 1).await;
    let input = connection.input();
    *slot.lock().unwrap() = Some(input.clone());
    input
        .try_image(request, 901, 1, Instant::now(), &jpeg(MAX_IMAGE_BYTES, 7))
        .unwrap();
    timeout(Duration::from_secs(1), async {
        while input.shared.images().in_flight.is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    input
        .try_audio(request, 1, Instant::now(), &[1234; 160])
        .unwrap();
    input.try_end(request).unwrap();
    timeout(Duration::from_secs(1), async {
        while input.commands.capacity() != crate::INPUT_QUEUE_CAPACITY {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    // The peer reads, so the image write runs to its final flush, where the
    // revocation lands.
    let after = received_until_closed(&mut server).await;
    assert_eq!(
        *outcome.lock().unwrap(),
        Some(ImageRevocation::AbandonedInFlight)
    );
    // The image itself may be complete on the wire by then; that cannot be
    // undone. Nothing behind it may follow, and it is not counted as sent.
    assert!(
        !after.iter().any(|kind| *kind != "video"),
        "sent after revocation: {after:?}"
    );
    assert_eq!(
        connection.state(),
        State::Disconnected(Error::InputCancelled)
    );
    let ledger = input.image_ledger();
    assert_eq!((ledger.sent, ledger.revoked), (0, 1));
}
