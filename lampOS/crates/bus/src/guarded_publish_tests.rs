use super::*;
use std::{
    cell::Cell,
    sync::{Arc, atomic::AtomicBool},
};

fn publisher(kind: wire::Kind) -> Publisher {
    Publisher::new(
        &Bus::new(std::env::temp_dir().join(format!("lamp-guarded-unused-{}", process::id())))
            .unwrap(),
        "mic1/audio",
        kind,
        "test",
    )
    .unwrap()
}

#[test]
fn revoked_before_or_after_encoding_never_admits_an_event_or_retains_raw() {
    for revoke_on in [1, 2] {
        let mut publisher = publisher(wire::Kind::Event);
        publisher.datagram = b"ordinary-buffer".to_vec();
        let mut calls = 0;
        let error = publisher
            .publish_bytes_guarded(1, "json+pcm", b"sensitive", || {
                calls += 1;
                calls < revoke_on
            })
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(calls, revoke_on);
        assert_eq!(publisher.seq, 0);
        assert_eq!(publisher.totals().published, 0);
        assert_eq!(publisher.datagram, b"ordinary-buffer");
    }
}

#[test]
fn latest_is_refused_before_authority_or_payload_processing() {
    let mut publisher = publisher(wire::Kind::Latest);
    assert_eq!(
        publisher.prepare_guarded_event().unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        publisher
            .publish_bytes_guarded(1, "json+pcm", b"private", || panic!(
                "must not check authority"
            ))
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(publisher.seq, 0);
    assert!(publisher.datagram.is_empty());
}

#[test]
fn filesystem_preparation_does_not_preserve_a_previous_authorization() {
    let directory = std::env::temp_dir().join(format!("lamp-guarded-prep-{}", process::id()));
    let _ = fs::remove_dir_all(&directory);
    let authority = Arc::new(AtomicBool::new(true));
    let revoke = authority.clone();
    let mut publisher = Publisher::new_with_cadence_clock(
        &Bus::new(&directory).unwrap(),
        "mic1/audio",
        wire::Kind::Event,
        "test",
        move || {
            revoke.store(false, Ordering::SeqCst);
            Instant::now()
        },
    )
    .unwrap();
    // The cadence callback models a revocation during filesystem preparation.
    publisher.prepare_guarded_event().unwrap();
    assert_eq!(
        publisher
            .publish_bytes_guarded(1, "json+pcm", b"private", || authority
                .load(Ordering::SeqCst))
            .unwrap_err()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    assert!(publisher.datagram.is_empty());
    drop(publisher);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn each_subscriber_is_checked_and_a_revocation_stops_fanout() {
    let allowed = Cell::new(true);
    let sent = Cell::new(0);
    let (outcome, error) = guarded_fanout(3, &mut || allowed.get(), |index| {
        assert_eq!(index, 0);
        sent.set(sent.get() + 1);
        allowed.set(false);
        Ok(5)
    });
    assert_eq!(sent.get(), 1);
    assert_eq!(outcome.delivered, 1);
    assert_eq!(error.unwrap().kind(), io::ErrorKind::PermissionDenied);
    let (outcome, error) = guarded_fanout(3, &mut || false, |_| panic!("revoked bytes were sent"));
    assert_eq!(outcome, Published::default());
    assert_eq!(error.unwrap().kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn interruption_rechecks_authority_and_cannot_spin_unboundedly() {
    let allowed = Cell::new(true);
    let attempts = Cell::new(0);
    let (outcome, error) = guarded_fanout(1, &mut || allowed.get(), |_| {
        attempts.set(attempts.get() + 1);
        allowed.set(false);
        Err(io::ErrorKind::Interrupted.into())
    });
    assert_eq!(attempts.get(), 1);
    assert_eq!(outcome.delivered, 0);
    assert_eq!(error.unwrap().kind(), io::ErrorKind::PermissionDenied);
    attempts.set(0);
    let (outcome, error) = guarded_fanout(1, &mut || true, |_| {
        attempts.set(attempts.get() + 1);
        Err(io::ErrorKind::Interrupted.into())
    });
    assert_eq!(attempts.get(), 2);
    assert_eq!(outcome.dropped, 1);
    assert!(error.is_none());
}

#[test]
fn dead_full_and_oversized_destinations_have_bounded_truthful_outcomes() {
    let (outcome, error) = guarded_fanout(4, &mut || true, |index| match index {
        0 => Ok(1),
        1 => Err(io::ErrorKind::WouldBlock.into()),
        2 => Err(io::ErrorKind::NotFound.into()),
        3 => Err(io::Error::from_raw_os_error(MESSAGE_TOO_LONG)),
        _ => unreachable!(),
    });
    assert_eq!(
        outcome,
        Published {
            delivered: 1,
            dropped: 1,
            removed: 1
        }
    );
    assert_eq!(error.unwrap().kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn partial_encoding_errors_are_scrubbed_and_never_touch_reusable_storage() {
    let publisher = publisher(wire::Kind::Event);
    let mut buffer = SensitiveDatagram::new();
    assert!(
        buffer
            .encode(&publisher.header, &vec![b'X'; wire::MOST_BYTES])
            .is_err()
    );
    assert!(buffer.bytes.contains(&b'X'));
    buffer.scrub();
    assert!(buffer.bytes.iter().all(|byte| *byte == 0));
    assert_eq!(buffer.len, 0);
    let mut publisher = publisher;
    assert!(
        publisher
            .publish_bytes_guarded(1, "json+pcm", &vec![b'X'; wire::MOST_BYTES], || true)
            .is_err()
    );
    assert!(publisher.datagram.is_empty());
    assert_eq!(publisher.seq, 0);
    // Same Drop path runs on success, early return, error and unwinding.
    assert!(
        std::panic::catch_unwind(|| {
            let mut buffer = SensitiveDatagram::new();
            buffer.encode(&publisher.header, b"private").unwrap();
            panic!("fixture unwind");
        })
        .is_err()
    );
}

#[test]
fn owned_encoded_buffer_is_scrubbed_on_error_and_unwind() {
    let header = publisher(wire::Kind::Event).header.clone();
    let observed = Arc::new(AtomicBool::new(false));
    let fail = |observed: Arc<AtomicBool>| -> io::Result<()> {
        let mut buffer = SensitiveDatagram::new();
        buffer.scrubbed = Some(observed);
        buffer.encode(&header, &vec![b'X'; wire::MOST_BYTES])?;
        Ok(())
    };
    assert!(fail(observed.clone()).is_err());
    assert!(observed.load(Ordering::SeqCst));
    observed.store(false, Ordering::SeqCst);
    assert!(
        std::panic::catch_unwind(|| {
            let mut buffer = SensitiveDatagram::new();
            buffer.scrubbed = Some(observed.clone());
            buffer.encode(&header, b"private").unwrap();
            panic!("fixture unwind");
        })
        .is_err()
    );
    assert!(observed.load(Ordering::SeqCst));
}

#[test]
fn successful_guarded_encoding_leaves_ordinary_buffer_and_sequence_interoperable() {
    let mut publisher = publisher(wire::Kind::Event);
    publisher.datagram = b"ordinary-buffer".to_vec();
    assert_eq!(
        publisher
            .publish_bytes_guarded(1, "json+pcm", b"private", || true)
            .unwrap(),
        Published::default()
    );
    assert_eq!(publisher.seq, 1);
    assert_eq!(publisher.totals().published, 1);
    assert_eq!(publisher.datagram, b"ordinary-buffer");
    // The ordinary API retains its original reusable-buffer behavior.
    publisher.scanned = Some(Instant::now());
    publisher.shown = Some(Instant::now());
    assert_eq!(
        publisher.publish_bytes(2, "json+pcm", b"ordinary").unwrap(),
        Published::default()
    );
    assert_eq!(publisher.seq, 2);
    assert!(publisher.datagram.ends_with(b"ordinary"));
}
