//! The opt-in PCM authority reaches the actual guarded Bus boundary.
//! Real local subscribers exercise expiry; no capture device is opened.
use lamp_bus::Bus;
use lamp_ears::{
    BLOCK,
    pcm::{Mailbox, Packet, audio_publisher},
    privacy::{Permit, PublicationGate},
};
use lamp_msgs::Audio;
use std::{
    io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ears-pcm-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn packet(gate: &PublicationGate) -> Packet {
    let permit = Permit {
        epoch: 1,
        until_us: 50,
    };
    gate.update(Some(permit), 0);
    let mut mailbox = Mailbox::new(true);
    assert!(mailbox.offer(&[17; BLOCK], gate, permit, 0, 12, 1));
    mailbox
        .take(gate, 2)
        .unwrap()
        .pack(gate, 3)
        .unwrap()
        .unwrap()
}
#[test]
fn revocation_after_borrow_is_checked_by_the_bus_not_only_by_the_buffer() {
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    let mut publisher = audio_publisher(&bus, "ears").unwrap();
    publisher.prepare_guarded_event().unwrap();
    let gate = PublicationGate::default();
    let packet = packet(&gate);
    let bytes = packet.bytes_if_allowed(&gate, 4).unwrap();
    gate.fence();
    let error = publisher
        .publish_bytes_guarded(4, Audio::ENCODING, bytes, || packet.allows(&gate, 4))
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}
#[test]
fn later_lease_does_not_extend_an_encoded_packets_original_deadline() {
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    let mut publisher = audio_publisher(&bus, "ears").unwrap();
    publisher.prepare_guarded_event().unwrap();
    let gate = PublicationGate::default();
    let packet = packet(&gate);
    let bytes = packet.bytes_if_allowed(&gate, 4).unwrap();
    gate.update(
        Some(Permit {
            epoch: 1,
            until_us: 100,
        }),
        0,
    );
    let error = publisher
        .publish_bytes_guarded(50, Audio::ENCODING, bytes, || packet.allows(&gate, 50))
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn queued_mic1_blocks_expire_for_a_stalled_reader_after_mute() {
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    let mut publisher = audio_publisher(&bus, "ears").unwrap();
    let mut subscriber = bus
        .subscribe_typed("mic1/audio", "expiry", Audio::TYPE)
        .unwrap();
    publisher.prepare_guarded_event().unwrap();
    let gate = PublicationGate::default();
    let packet = packet(&gate);
    let bytes = packet.bytes_if_allowed(&gate, 4).unwrap();
    assert_eq!(
        publisher.declaration().unwrap().lifespan(),
        Some(Duration::from_millis(50))
    );
    let sent = publisher
        .publish_bytes_guarded(4, Audio::ENCODING, bytes, || packet.allows(&gate, 4))
        .unwrap();
    assert_eq!(sent.delivered, 1);
    gate.fence();
    // 50ms declaration plus existing 50ms receiver grace. A queued datagram
    // cannot remain usable when this subscriber resumes after a long stall.
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        subscriber
            .recv(Duration::from_millis(30))
            .unwrap()
            .is_none()
    );
    assert_eq!(subscriber.expired(), 1);
    assert_eq!(Audio::declaration().lifespan(), None);
}
