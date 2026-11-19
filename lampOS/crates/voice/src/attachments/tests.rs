use super::*;
use lamp_msgs::ClockStamp;

/// A made-up photo with no face in it, in a folder of the test's own.
fn photo_file(name: &str) -> (std::path::PathBuf, Vec<u8>) {
    let dir = std::env::temp_dir().join(format!("voice-attach-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
    jpeg.resize(2_048, 9);
    let path = dir.join("0123456789abcdef");
    std::fs::write(&path, &jpeg).unwrap();
    (path, jpeg)
}

fn attach(path: &Path, kind: &str) -> AttachRef {
    AttachRef {
        path: path.to_str().unwrap().to_owned(),
        id: "0123456789abcdef".into(),
        kind: kind.into(),
        mime: if kind == "photo" {
            "image/jpeg".into()
        } else {
            "text/plain".into()
        },
        bytes: 2_048,
        sent_at: 0,
    }
}

/// Made-up people: Bea, the owner, and Ivo, a guest.
fn bea() -> String {
    crate::known_faces::memory_person("Bea")
}

fn ivo() -> String {
    crate::known_faces::memory_person("Ivo")
}

#[test]
fn a_photo_goes_with_the_owners_next_line_within_120_s_and_only_once() {
    let (path, jpeg) = photo_file("owner");
    let t0 = Instant::now();
    let mut inbox = Inbox::default();
    arrive(&mut inbox, attach(&path, "photo"), t0);
    let bea = bea();
    let owners = Line {
        owner: Some("Bea"),
        person: Some(&bea),
        faces: Some(1),
    };
    // A line with no lease, or a model that takes no picture, carries
    // nothing and keeps it for the next.
    assert_eq!(
        take_photo(&mut inbox, None, t0 + Duration::from_secs(5)),
        None
    );
    assert_eq!(
        take_photo(&mut inbox, Some(owners), t0 + Duration::from_secs(10)),
        Some(jpeg)
    );
    // Once: the owner's next line doesn't carry it again.
    assert_eq!(
        take_photo(&mut inbox, Some(owners), t0 + Duration::from_secs(20)),
        None
    );
    // After 120 s, never.
    let mut late = Inbox::default();
    arrive(&mut late, attach(&path, "photo"), t0);
    assert_eq!(
        take_photo(&mut late, Some(owners), t0 + Duration::from_secs(121)),
        None
    );
    // A text is never carried as a photo.
    let mut text = Inbox::default();
    arrive(&mut text, attach(&path, "text"), t0);
    assert_eq!(
        take_photo(&mut text, Some(owners), t0 + Duration::from_secs(1)),
        None
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_guests_line_two_faces_or_an_unknown_face_get_nothing() {
    let (path, jpeg) = photo_file("guest");
    let t0 = Instant::now();
    let mut inbox = Inbox::default();
    arrive(&mut inbox, attach(&path, "photo"), t0);
    let (bea, ivo) = (bea(), ivo());
    for (line, why) in [
        (
            Line {
                owner: Some("Bea"),
                person: Some(&ivo),
                faces: Some(1),
            },
            "a guest's line",
        ),
        (
            Line {
                owner: Some("Bea"),
                person: None,
                faces: Some(1),
            },
            "an unknown face",
        ),
        (
            Line {
                owner: Some("Bea"),
                person: Some(&bea),
                faces: Some(2),
            },
            "the owner with someone else in view",
        ),
        (
            Line {
                owner: Some("Bea"),
                person: Some(&bea),
                faces: None,
            },
            "no look at all",
        ),
    ] {
        assert_eq!(
            take_photo(&mut inbox, Some(line), t0 + Duration::from_secs(5)),
            None,
            "{why}"
        );
    }
    // Still the owner's own next line's.
    let owners = Line {
        owner: Some("Bea"),
        person: Some(&bea),
        faces: Some(1),
    };
    assert_eq!(
        take_photo(&mut inbox, Some(owners), t0 + Duration::from_secs(6)),
        Some(jpeg)
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn an_owner_with_no_kept_face_gets_it_only_in_the_conversation_open_as_it_arrived() {
    let (path, jpeg) = photo_file("no-face");
    let t0 = Instant::now();
    let ivo = ivo();
    let room = Line {
        owner: None,
        person: None,
        faces: Some(1),
    };
    // No conversation open as it arrived: never.
    let mut closed = Inbox::default();
    arrive(&mut closed, attach(&path, "photo"), t0);
    assert_eq!(
        take_photo(&mut closed, Some(room), t0 + Duration::from_secs(5)),
        None
    );
    assert_eq!(
        take_photo(&mut closed, Some(room), t0 + Duration::from_secs(10)),
        None,
        "a conversation begun after it arrived"
    );
    // Open: a line 10 s before it arrived.
    let mut open = Inbox::default();
    assert_eq!(take_photo(&mut open, Some(room), t0), None);
    arrive(
        &mut open,
        attach(&path, "photo"),
        t0 + Duration::from_secs(10),
    );
    for (line, why) in [
        (
            Line {
                person: Some(&ivo),
                ..room
            },
            "a known guest",
        ),
        (
            Line {
                faces: Some(2),
                ..room
            },
            "two faces",
        ),
        (
            Line {
                faces: Some(0),
                ..room
            },
            "nobody in view",
        ),
    ] {
        assert_eq!(
            take_photo(&mut open, Some(line), t0 + Duration::from_secs(20)),
            None,
            "{why}"
        );
    }
    assert_eq!(
        take_photo(&mut open, Some(room), t0 + Duration::from_secs(30)),
        Some(jpeg)
    );
    // The conversation lapsed between: no longer the one open at arrival.
    let mut lapsed = Inbox::default();
    assert_eq!(take_photo(&mut lapsed, Some(room), t0), None);
    arrive(
        &mut lapsed,
        attach(&path, "photo"),
        t0 + Duration::from_secs(10),
    );
    assert_eq!(
        take_photo(&mut lapsed, Some(room), t0 + Duration::from_secs(100)),
        None
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_forget_a_closed_privacy_lease_and_30_min_let_it_go() {
    let (path, _) = photo_file("let-go");
    let t0 = Instant::now();
    // A forget: the file goes at once.
    arrive(&mut inbox(), attach(&path, "photo"), t0);
    forget();
    assert!(inbox().arrived.is_none());
    assert!(!path.exists());
    // 30 min after the conversation's last line, not before.
    let (path, _) = photo_file("let-go");
    let mut kept = Inbox::default();
    arrive(&mut kept, attach(&path, "photo"), t0);
    kept.last_line = Some(t0 + Duration::from_secs(60));
    assert_eq!(
        expire(&mut kept, t0 + Duration::from_secs(60) + KEPT_AFTER / 2),
        None
    );
    assert!(path.exists());
    assert_eq!(
        expire(&mut kept, t0 + Duration::from_secs(61) + KEPT_AFTER),
        Some("0123456789abcdef".to_owned())
    );
    assert!(!path.exists());
    // The privacy lease: closed when not permitted, read muted, or silent.
    let lease = MicrophonePrivacy {
        stamp: ClockStamp {
            clock_id: "c".into(),
            at_us: 1,
            sequence: 1,
        },
        session_id: "0".repeat(16),
        controller_boot: "1".repeat(32),
        microphone_generation: 1,
        observed_us: 1,
        expires_at_us: 2,
        permitted: true,
        physical_muted: Some(false),
        no_slide: false,
    };
    assert!(!closed(&lease, false, 0));
    assert!(!closed(&lease, true, 40), "a renewal a moment late");
    assert!(closed(&lease, true, 5_000), "no longer renewed");
    assert!(closed(
        &MicrophonePrivacy {
            permitted: false,
            ..lease.clone()
        },
        false,
        0
    ));
    assert!(closed(
        &MicrophonePrivacy {
            physical_muted: Some(true),
            ..lease
        },
        false,
        0
    ));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn only_lamp_clouds_message_for_a_file_in_its_folder_is_taken() {
    let root = std::env::temp_dir().join(format!("voice-attach-bus-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bus = lamp_bus::Bus::new(&root).unwrap();
    let mut heard = bus
        .subscribe_typed(AttachRef::TOPIC, SOURCE, AttachRef::TYPE)
        .unwrap();
    let inbox_path = Path::new(AttachRef::DIR).join("0123456789abcdef");
    for (source, path, taken_it) in [
        (CLOUD, inbox_path.as_path(), true),
        ("skills", inbox_path.as_path(), false),
        (CLOUD, Path::new("/etc/0123456789abcdef"), false),
    ] {
        let mut out = bus
            .publisher_declared(AttachRef::TOPIC, &AttachRef::declaration(), source)
            .unwrap();
        out.publish_json(lamp_ipc::monotonic_us(), &attach(path, "photo"))
            .unwrap();
        let message = heard.recv(Duration::from_secs(2)).unwrap().unwrap();
        assert_eq!(taken(&message).is_some(), taken_it, "{source} {path:?}");
    }
    let _ = std::fs::remove_dir_all(root);
}
