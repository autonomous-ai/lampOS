//! How long a write may wait on a link that is behind, and what waits
//! behind it (lampOS#705: on lamp-4ace, 84 lines lost to `write_timeout`).
use super::*;
use crate::{MAX_IMAGE_BYTES, MAX_INPUT_QUEUE, Write, testing::jpeg};

/// A session on a link that holds `capacity` bytes until its peer reads.
async fn behind(capacity: usize, config: SessionConfig) -> (Connection, MockSocket) {
    let (client, mut server) = pair(capacity).await;
    let connect = tokio::spawn(setup_and_spawn(client, config, SessionId::new(17).unwrap()));
    receive(&mut server).await;
    server
        .send(Message::Binary(br#"{"setupComplete":{}}"#.to_vec().into()))
        .await
        .unwrap();
    (connect.await.unwrap().unwrap(), server)
}

/// A person speaking while the link takes nothing for about 0.8 s: a block
/// of 20 ms is offered every 20 ms for 0.9 s, and the peer reads again only
/// then. What happens to the line, and the connection.
async fn a_line_over_a_stall(config: SessionConfig) -> (std::result::Result<usize, Error>, State) {
    let (mut connection, mut server) = behind(4_096, config).await;
    let request = request(1);
    let input = connection.input();
    input.try_start(request).unwrap();
    sent(&mut server, "activityStart").await;
    assert!(matches!(
        event(&mut connection).await,
        Event::InputStarted { .. }
    ));
    let mut offered = 0;
    for sequence in 0..45 {
        if let Err(error) = input.try_audio(request, sequence, Instant::now(), &[100; 320]) {
            return (Err(error), connection.state());
        }
        offered += 1;
        tokio::time::sleep(Duration::from_millis(20)).await;
        if let State::Disconnected(error) = connection.state() {
            return (Err(error), connection.state());
        }
    }
    let mut audio = 0;
    while audio < offered {
        let message = receive(&mut server).await;
        if message["realtimeInput"].get("audio").is_some() {
            audio += 1;
        }
    }
    (Ok(audio), connection.state())
}

/// With today's bounds a stall of 0.8 s while someone speaks ends the
/// connection on that line's audio. A longer write bound alone only moves
/// the end: the 32 blocks queued behind it, 640 ms, overflow first. With
/// the bound and a queue to the audio's own age, every block of the line
/// goes out once the link takes it again, and the connection stays.
#[tokio::test(start_paused = true)]
async fn a_line_over_a_stall_is_kept_only_with_patient_writes_and_their_queue() {
    let (today, _) = a_line_over_a_stall(configuration(Timeouts::default())).await;
    assert!(
        matches!(
            today,
            Err(Error::WriteTimeout {
                write: Write::Audio,
                deadline_ms: 200,
                ..
            })
        ),
        "{today:?}"
    );
    let bound_alone = configuration(Timeouts::default())
        .writes(Duration::from_secs(1), Duration::ZERO, INPUT_QUEUE_CAPACITY)
        .unwrap();
    let (moved, state) = a_line_over_a_stall(bound_alone).await;
    assert_eq!(moved, Err(Error::Backpressure));
    assert_eq!(state, State::Disconnected(Error::Backpressure));
    let patient = configuration(Timeouts::default())
        .writes(
            Duration::from_secs(1),
            Duration::from_millis(8),
            MAX_INPUT_QUEUE,
        )
        .unwrap();
    let (kept, state) = a_line_over_a_stall(patient).await;
    assert_eq!(kept, Ok(45));
    assert_eq!(state, State::Ready);
}

/// A big write is given time for its size: a camera frame the link cannot
/// take at all fails after its 400 ms and 8 ms for each KiB it puts on the
/// wire, and says so. Audio never gets the allowance.
#[tokio::test(start_paused = true)]
async fn a_big_write_is_given_time_for_its_size() {
    let patient = configuration(Timeouts::default())
        .writes(
            Duration::from_secs(1),
            Duration::from_millis(8),
            MAX_INPUT_QUEUE,
        )
        .unwrap();
    let (mut connection, mut server) = behind(4_096, patient).await;
    let request = request(1);
    let input = connection.input();
    input.try_start(request).unwrap();
    sent(&mut server, "activityStart").await;
    assert!(matches!(
        event(&mut connection).await,
        Event::InputStarted { .. }
    ));
    let image = jpeg(MAX_IMAGE_BYTES, 7);
    let kib = u32::try_from(
        crate::wire::encode_image(&image)
            .unwrap()
            .len()
            .div_ceil(1024),
    )
    .unwrap();
    let offered = Clock::now();
    assert_eq!(
        input.try_image(request, 1, 1, Instant::now(), &image),
        Ok(crate::ImageOffer::Queued)
    );
    let bound = IMAGE_WRITE + Duration::from_millis(8) * kib;
    assert_eq!(
        disconnected(&connection).await,
        Error::WriteTimeout {
            write: Write::Image,
            kib,
            deadline_ms: u32::try_from(bound.as_millis()).unwrap(),
        }
    );
    let took = offered.elapsed();
    assert!(
        took >= bound && took <= bound + Duration::from_millis(50),
        "{took:?} for {bound:?}"
    );
    drop(server);
}

/// The bounds a session may be given: an allowance up to 50 ms a KiB, a
/// write bound up to 1 s, and a queue from today's 32 to 128.
#[test]
fn the_write_bounds_are_checked() {
    let config = || configuration(Timeouts::default());
    assert!(
        config()
            .writes(Duration::from_secs(1), Duration::from_millis(50), 128)
            .is_ok()
    );
    for (write, per_kib, queue) in [
        (Duration::from_millis(1_001), Duration::ZERO, 32),
        (Duration::ZERO, Duration::ZERO, 32),
        (Duration::from_secs(1), Duration::from_millis(51), 32),
        (Duration::from_secs(1), Duration::ZERO, 31),
        (Duration::from_secs(1), Duration::ZERO, 129),
    ] {
        assert!(
            config().writes(write, per_kib, queue).is_err(),
            "{write:?} {per_kib:?} {queue}"
        );
    }
}

/// A written turn is named for what it carries (the reviewer's and Ten's
/// P3s on #1393): with the look's picture it is `picture`, without one
/// `written`, each with its size and the bound it had, on a link that
/// takes nothing. With patient writes the picture is given time for its
/// size; today it has the 200 ms every write has.
#[tokio::test(start_paused = true)]
async fn a_written_turn_is_named_for_its_picture_and_given_time_for_it() {
    let look = jpeg(crate::MAX_LOOK_IMAGE_BYTES, 3);
    let text = "What do you see?";
    for (picture, write) in [
        (Some(look.as_slice()), Write::Picture),
        (None, Write::Written),
    ] {
        for patient in [false, true] {
            let (config, bound, per_kib) = if patient {
                let config = configuration(Timeouts::default())
                    .writes(
                        Duration::from_secs(1),
                        Duration::from_millis(8),
                        MAX_INPUT_QUEUE,
                    )
                    .unwrap();
                (config, Duration::from_secs(1), Duration::from_millis(8))
            } else {
                let config = configuration(Timeouts::default());
                (config, Duration::from_millis(200), Duration::ZERO)
            };
            // A link that holds 64 bytes: the setup goes through as the
            // peer reads it, and then the peer reads nothing.
            let (connection, server) = behind(64, config).await;
            let kib = u32::try_from(
                crate::wire::encode_written_turn(text, picture)
                    .unwrap()
                    .len()
                    .div_ceil(1024),
            )
            .unwrap();
            let bound = bound + per_kib * kib;
            let offered = Clock::now();
            connection
                .input()
                .try_written(request(1), text, picture)
                .unwrap();
            assert_eq!(
                disconnected_within(&connection, Duration::from_secs(10)).await,
                Error::WriteTimeout {
                    write,
                    kib,
                    deadline_ms: u32::try_from(bound.as_millis()).unwrap(),
                },
                "{write:?}, patient {patient}"
            );
            let took = offered.elapsed();
            assert!(
                took >= bound && took <= bound + Duration::from_millis(50),
                "{write:?}, patient {patient}: {took:?} for {bound:?}"
            );
            drop(server);
        }
    }
}

/// How the connection ended, waiting up to `bound` (a patient write of the
/// look's picture has about 3 s).
async fn disconnected_within(connection: &Connection, bound: Duration) -> Error {
    let mut state = connection.subscribe_state();
    timeout(bound, async {
        loop {
            if let State::Disconnected(error) = *state.borrow_and_update() {
                return error;
            }
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap()
}

/// Nine's question on #1397: the barrier's quiet counts from the last
/// event read, and nothing is read while a write waits. The earlier answer
/// is interrupted and the person goes on speaking, a block every 20 ms. The
/// link takes their words until `stall_from_ms`, then nothing until 2.0 s.
/// The service's idle completion, which settles the barrier, comes in at
/// `confirm_at_ms`, while a write waits. Kept, the barrier is confirmed and
/// the new request's End goes out; otherwise, how the connection ended.
async fn a_confirmation_during_a_stall(
    config: SessionConfig,
    stall_from_ms: u64,
    confirm_at_ms: u64,
) -> std::result::Result<(), Error> {
    let (mut connection, mut server) = behind(4_096, config).await;
    let old = ended_turn(&mut connection, &mut server, 1).await;
    let new = request(2);
    let input = connection.input();
    input.try_start(new).unwrap();
    sent(&mut server, "activityStart").await;
    assert!(matches!(
        event(&mut connection).await,
        Event::InputStarted {
            waiting_for_barrier: true,
            ..
        }
    ));
    send_json(&mut server, json!({"serverContent":{"interrupted":true}})).await;
    assert!(
        matches!(event(&mut connection).await, Event::Interrupted { lineage } if lineage.request == old)
    );
    let mut audio = 0;
    for sequence in 0..110u64 {
        if input
            .try_audio(new, sequence, Instant::now(), &[100; 320])
            .is_err()
        {
            return Err(disconnected_within(&connection, Duration::from_secs(1)).await);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        let at = (sequence + 1) * 20;
        if let State::Disconnected(error) = connection.state() {
            return Err(error);
        }
        if at <= stall_from_ms {
            sent(&mut server, "audio").await;
            audio += 1;
        } else if at == confirm_at_ms {
            send_json(&mut server, idle()).await;
        } else if at >= 2_000 {
            // The link takes everything again.
            while audio < sequence + 1 {
                match timeout(Duration::from_secs(2), server.next()).await {
                    Ok(Some(Ok(message))) => {
                        let value: Value =
                            serde_json::from_str(message.to_text().unwrap()).unwrap();
                        assert!(value["realtimeInput"].get("audio").is_some(), "{value}");
                    }
                    _ => {
                        return Err(disconnected_within(&connection, Duration::from_secs(1)).await);
                    }
                }
                audio += 1;
            }
        }
    }
    assert!(
        matches!(event(&mut connection).await, Event::TurnComplete { lineage, idle: true } if lineage.request == old)
    );
    input.try_end(new).unwrap();
    sent(&mut server, "activityEnd").await;
    assert_eq!(connection.state(), State::Ready);
    Ok(())
}

/// With patient writes, a confirmation that comes in while a write waits
/// is read before the quiet is judged: over a stall of 0.9 s (inside the
/// patient 1 s), and over a short one of about 140 ms. Judged first, both
/// would end as `BarrierTimeout`.
#[tokio::test(start_paused = true)]
async fn a_confirmation_that_comes_while_a_write_waits_is_read_before_the_quiet_is_judged() {
    let patient = || {
        configuration(Timeouts::default())
            .writes(
                Duration::from_secs(1),
                Duration::from_millis(8),
                MAX_INPUT_QUEUE,
            )
            .unwrap()
    };
    assert_eq!(
        a_confirmation_during_a_stall(patient(), 1_100, 1_500).await,
        Ok(())
    );
    assert_eq!(
        a_confirmation_during_a_stall(patient(), 1_760, 1_900).await,
        Ok(())
    );
}

/// Without patient writes the read loop is as before (the reviewer on
/// #1397): the same short stall, inside today's 200 ms, with the
/// confirmation at 1.9 s, still ends as `BarrierTimeout`.
#[tokio::test(start_paused = true)]
async fn without_patient_writes_the_quiet_is_judged_as_before() {
    assert_eq!(
        a_confirmation_during_a_stall(configuration(Timeouts::default()), 1_760, 1_900).await,
        Err(Error::BarrierTimeout)
    );
}
