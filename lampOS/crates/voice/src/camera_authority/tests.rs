use super::*;
use crate::memory_scope::{RequestKey, RequestScope, ScopeSnapshot};
use crate::second_provider::PermissionState;
use lamp_turn_owner::BootId;
use serde_json::{Value, json};
struct Scratch(crate::process::SessionDirectory);
impl Scratch {
    fn create() -> Self {
        let mut directory = crate::process::SessionDirectory::create().unwrap();
        directory.path = directory.path.canonicalize().unwrap();
        Self(directory)
    }
}
impl std::ops::Deref for Scratch {
    type Target = crate::process::SessionDirectory;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        for name in [
            "known-speaker",
            "known-speaker.new",
            "alias",
            "known.authority.d/hold.sock",
        ] {
            let _ = fs::remove_file(self.path.join(name));
        }
        let _ = fs::remove_dir(self.path.join("known.authority.d"));
    }
}
const PUBLISHER: [u8; 16] = [
    0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
];
const SPEAKER: &str = "0123456789abcdef/f7";
fn source(hold: u64) -> Value {
    let now = lamp_ipc::monotonic_us();
    json!({"version":3,"state":"seen","name":"Fixture owner","id":null,"boot":"0123456789abcdef","face":"f7","publisher":"0123456789abcdef0123456789abcdef","generation":1,"camera_hold":hold,"camera_began_us":1,"observed_us":now,"permission_until_us":now+1_500_000})
}
fn publish(known: &Path, source: &Value) {
    let path = crate::speaking_faces::known_speaker_of(known);
    let temporary = path.with_extension("new");
    fs::write(&temporary, source.to_string()).unwrap();
    fs::rename(temporary, path).unwrap();
}
fn owner(fixture: &fixture::Fixture, request: u64, until: Instant) -> RequestScope {
    let mut owner = RequestScope::looking_later(
        RequestKey {
            worker: BootId::new([61; 16]).unwrap(),
            session: 1,
            request,
        },
        ScopeSnapshot::of("Fixture owner"),
        fixture.cache(),
    );
    let key = owner.lease().key();
    assert!(
        owner
            .speaker_recorder()
            .unwrap()
            .seal(key, &[SPEAKER.into()], until)
    );
    owner
}
#[test]
fn v3_source_rejects_old_mixed_expired_and_pre_hold_observations() {
    let directory = Scratch::create();
    let known = directory.path.join("known");
    publish(&known, &source(1));
    assert!(read(&known).is_some());
    for field in ["state", "name", "generation"] {
        let mut duplicate = source(1).to_string();
        duplicate.pop();
        duplicate.push_str(&format!(",\"{field}\":{}}}", source(1)[field]));
        fs::write(crate::speaking_faces::known_speaker_of(&known), duplicate).unwrap();
        assert!(read(&known).is_none(), "duplicate {field}");
    }
    for case in [
        "v2",
        "unknown",
        "zero-publisher",
        "zero-hold",
        "pre-hold",
        "expired",
        "future",
        "generation",
        "mixed-absence",
    ] {
        let mut value = source(1);
        match case {
            "v2" => {
                value.as_object_mut().unwrap().remove("version");
            }
            "unknown" => value["new_policy"] = true.into(),
            "zero-publisher" => value["publisher"] = "00000000000000000000000000000000".into(),
            "zero-hold" => value["camera_hold"] = 0.into(),
            "pre-hold" => {
                value["camera_began_us"] = value["observed_us"]
                    .as_u64()
                    .unwrap()
                    .saturating_add(1)
                    .into()
            }
            "expired" => value["permission_until_us"] = 1.into(),
            "future" => value["observed_us"] = u64::MAX.into(),
            "generation" => value["generation"] = 0.into(),
            "mixed-absence" => value["state"] = "absent".into(),
            _ => unreachable!(),
        }
        publish(&known, &value);
        assert!(read(&known).is_none(), "{case}");
    }
}
#[test]
fn real_grant_is_request_owned_and_close_loses_every_consumer_without_source_rewrite() {
    let directory = Scratch::create();
    let known = directory.path.join("known");
    publish(&known, &source(1));
    let fixture = fixture::Fixture::start(known.clone(), PUBLISHER, 1);
    let first = owner(&fixture, 1, Instant::now() + Duration::from_secs(3));
    let second = owner(&fixture, 2, Instant::now() + Duration::from_secs(3));
    assert_eq!(fixture.ready(&first.lease()), PermissionState::Ready);
    assert_eq!(fixture.ready(&second.lease()), PermissionState::Ready);
    let before = fs::read(crate::speaking_faces::known_speaker_of(&known)).unwrap();
    first.revoke();
    let evidence = read(&known).unwrap();
    assert_eq!(
        permission(&known, &first.lease(), &evidence),
        PermissionState::Lost
    );
    assert_eq!(
        permission(&known, &second.lease(), &evidence),
        PermissionState::Ready
    );
    fixture.witness.revoke().unwrap();
    assert_eq!(
        permission(&known, &second.lease(), &evidence),
        PermissionState::Lost
    );
    assert_eq!(
        before,
        fs::read(crate::speaking_faces::known_speaker_of(&known)).unwrap()
    );
}
#[test]
fn changed_hold_never_retargets_an_existing_request_and_open_ticket_cannot_extend_source_age() {
    let directory = Scratch::create();
    let known = directory.path.join("known");
    publish(&known, &source(1));
    let fixture = fixture::Fixture::start(known.clone(), PUBLISHER, 1);
    let original = owner(&fixture, 1, Instant::now() + Duration::from_secs(3));
    assert_eq!(fixture.ready(&original.lease()), PermissionState::Ready);
    publish(&known, &source(2));
    let changed = read(&known).unwrap();
    assert_eq!(
        permission(&known, &original.lease(), &changed),
        PermissionState::Lost
    );
    let mut expired = source(1);
    expired["permission_until_us"] = 1.into();
    publish(&known, &expired);
    assert!(read(&known).is_none());
    assert!(
        fixture
            .witness
            .grant(NonZeroU64::new(81).unwrap(), Instant::now() + GRANT_WITHIN)
            .is_ok(),
        "independent witness can still be open; it cannot validate old source"
    );
}
#[test]
fn missing_peer_finishes_original_job_and_never_restarts_its_deadline() {
    let directory = Scratch::create();
    let known = directory.path.join("known");
    publish(&known, &source(1));
    let client = Client::start(known.clone()).unwrap();
    let cache = Arc::new(crate::memory_scope::FaceCache::default());
    assert!(cache.set_camera_client(client.clone()));
    let mut original = RequestScope::looking_later(
        RequestKey {
            worker: BootId::new([62; 16]).unwrap(),
            session: 1,
            request: 1,
        },
        ScopeSnapshot::of("Fixture owner"),
        cache,
    );
    let until = Instant::now() + Duration::from_secs(3);
    let key = original.lease().key();
    assert!(
        original
            .speaker_recorder()
            .unwrap()
            .seal(key, &[SPEAKER.into()], until)
    );
    let evidence = read(&known).unwrap();
    let state = permission(&known, &original.lease(), &evidence);
    assert!(matches!(
        state,
        PermissionState::Pending | PermissionState::Lost
    ));
    let lease = original.lease();
    let binding = lease.camera_binding().unwrap();
    let acquisition_until = binding.job.until;
    while matches!(
        permission(&known, &lease, &evidence),
        PermissionState::Pending
    ) && Instant::now() < acquisition_until + POLL
    {
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(permission(&known, &lease, &evidence), PermissionState::Lost);
    assert_eq!(binding.job.until, acquisition_until);
    original.revoke();
    drop(original);
    drop(client);
}
#[test]
fn configured_path_refuses_foreign_mode_symlink_and_oversize_without_touching_them() {
    let directory = Scratch::create();
    let known = directory.path.join("known");
    let fixture = fixture::Fixture::start(known.clone(), PUBLISHER, 1);
    assert!(trusted_path(&known).is_some());
    use std::os::unix::fs::{PermissionsExt, symlink};
    fs::set_permissions(&fixture.socket, fs::Permissions::from_mode(0o666)).unwrap();
    assert!(trusted_path(&known).is_none());
    fs::set_permissions(&fixture.socket, fs::Permissions::from_mode(0o600)).unwrap();
    let alias = directory.path.join("alias");
    symlink(known.parent().unwrap(), &alias).unwrap();
    assert!(trusted_path(&alias.join("known")).is_none());
    assert!(trusted_path(&directory.path.join("x".repeat(120))).is_none());
    assert!(fixture.socket.exists());
}

#[test]
fn stop_during_partial_grant_closes_retained_or_queued_rights() {
    use rustix::net::{
        RecvFlags, SendAncillaryBuffer, SendAncillaryMessage, SendFlags, recv, sendmsg,
    };
    use std::{
        mem::MaybeUninit,
        os::{
            fd::AsFd,
            unix::{fs::PermissionsExt, net::UnixListener},
        },
    };
    let directory = Scratch::create();
    let known = directory.path.join("known");
    publish(&known, &source(1));
    let private = directory.path.join("known.authority.d");
    fs::create_dir(&private).unwrap();
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
    let socket = private.join("hold.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    listener.set_nonblocking(true).unwrap();
    thread::scope(|scope| {
        let server = scope.spawn(move || {
            assert!(lamp_ipc::readable(&listener, Duration::from_secs(2)).unwrap());
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0; 48];
            stream.read_exact(&mut request).unwrap();
            let (probe, observer) = UnixStream::pair().unwrap();
            {
                let rights = [observer.as_fd()];
                let mut storage = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
                let mut ancillary = SendAncillaryBuffer::new(&mut storage);
                assert!(ancillary.push(SendAncillaryMessage::ScmRights(&rights)));
                stream.set_nonblocking(true).unwrap();
                #[cfg(target_os = "macos")]
                rustix::net::sockopt::set_socket_nosigpipe(&stream, true).unwrap();
                assert_eq!(
                    sendmsg(
                        &stream,
                        &[std::io::IoSlice::new(b"L")],
                        &mut ancillary,
                        SendFlags::DONTWAIT
                    )
                    .unwrap(),
                    1
                );
            }
            drop(observer);
            (stream, probe)
        });
        let client = Client::start(known.clone()).unwrap();
        let evidence = read(&known).unwrap();
        let job = client
            .request(&evidence, Instant::now() + Duration::from_secs(2))
            .unwrap();
        let (control, probe) = server.join().unwrap();
        // The peer remains alive and has NOT completed the frame/EOF. Closing
        // the client must release queued or retained SCM_RIGHTS by RAII.
        drop(client);
        assert!(job.cancelled.load(Ordering::Acquire));
        let mut byte = [0];
        assert!(
            matches!(recv(&probe, &mut byte, RecvFlags::DONTWAIT), Ok((_, 0))),
            "stop leaked its partial grant FD"
        );
        drop(control);
    });
    fs::remove_file(socket).unwrap();
    fs::remove_dir(private).unwrap();
}

#[test]
fn cached_evidence_keeps_its_original_capture_age_deadline() {
    let directory = Scratch::create();
    let known = directory.path.join("known");
    let mut value = source(1);
    let now = lamp_ipc::monotonic_us();
    value["observed_us"] = now.checked_sub(2_000_000).unwrap().into();
    publish(&known, &value);
    let fixture = fixture::Fixture::start(known.clone(), PUBLISHER, 1);
    let original = owner(&fixture, 1, Instant::now() + Duration::from_secs(3));
    assert_eq!(fixture.ready(&original.lease()), PermissionState::Ready);
    let evidence = read(&known).unwrap();
    assert_eq!(
        permission(&known, &original.lease(), &evidence),
        PermissionState::Ready
    );
    thread::sleep(evidence.expires.saturating_duration_since(Instant::now()));
    assert_eq!(
        permission(&known, &original.lease(), &evidence),
        PermissionState::Lost
    );
    assert!(
        fixture
            .witness
            .grant(NonZeroU64::new(83).unwrap(), Instant::now() + GRANT_WITHIN)
            .is_ok(),
        "camera witness alone must not renew capture age"
    );
}

#[test]
fn new_request_recovers_failed_acquisition_without_reviving_the_old_request() {
    let directory = Scratch::create();
    let known = directory.path.join("known");
    publish(&known, &source(1));
    let client = Client::start(known.clone()).unwrap();
    let cache = Arc::new(crate::memory_scope::FaceCache::default());
    assert!(cache.set_camera_client(client));
    let make = |request| {
        let mut owner = RequestScope::looking_later(
            RequestKey {
                worker: BootId::new([63; 16]).unwrap(),
                session: 1,
                request,
            },
            ScopeSnapshot::of("Fixture owner"),
            cache.clone(),
        );
        let key = owner.lease().key();
        assert!(owner.speaker_recorder().unwrap().seal(
            key,
            &[SPEAKER.into()],
            Instant::now() + Duration::from_secs(3)
        ));
        owner
    };
    let first = make(1);
    let evidence = read(&known).unwrap();
    let until = Instant::now() + GRANT_WITHIN;
    loop {
        if permission(&known, &first.lease(), &evidence) == PermissionState::Lost {
            break;
        }
        assert!(
            Instant::now() < until + POLL,
            "failed original acquisition terminates"
        );
        thread::sleep(Duration::from_millis(1));
    }
    let old_until = first.lease().camera_binding().unwrap().job.until;
    // Same configured producer/hold recovers; no new source token or automatic
    // retry is lent to the already bound first request.
    let server = fixture::Fixture::start(known.clone(), PUBLISHER, 1);
    let second = make(2);
    assert_eq!(server.ready(&second.lease()), PermissionState::Ready);
    let evidence = read(&known).unwrap();
    assert_eq!(
        permission(&known, &first.lease(), &evidence),
        PermissionState::Lost
    );
    assert_eq!(first.lease().camera_binding().unwrap().job.until, old_until);
    assert_eq!(
        permission(&known, &second.lease(), &evidence),
        PermissionState::Ready
    );
}

#[test]
fn connected_kernel_peer_credentials_must_match_before_any_grant_bytes() {
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};
    let directory = Scratch::create();
    let known = directory.path.join("known");
    publish(&known, &source(1));
    let private = directory.path.join("known.authority.d");
    fs::create_dir(&private).unwrap();
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
    let path = private.join("hold.sock");
    let listener = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let (checked, expected) = trusted_path(&known).unwrap();
    assert_eq!(checked, path);
    assert!(
        read(&known).is_some(),
        "valid source/path alone is not peer authentication"
    );
    let client = UnixStream::connect(&path).unwrap();
    let (peer, _) = listener.accept().unwrap();
    assert_eq!(
        peer_uid(&client).unwrap(),
        expected,
        "real kernel peer, not a supplied/mock identity"
    );
    assert!(
        authenticate_peer(&client, expected).is_ok(),
        "positive identity prevents all-refusal from passing"
    );
    let different = expected.checked_add(1).unwrap_or_else(|| expected - 1);
    assert_eq!(
        authenticate_peer(&client, different).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    // The production helper itself sends nothing and receives no ancillary
    // rights. Both checks precede exchange's request/nonce/FD handling.
    let mut byte = [0];
    assert!(
        matches!(
            rustix::net::recv(&peer, &mut byte, rustix::net::RecvFlags::DONTWAIT),
            Err(Errno::AGAIN)
        ),
        "credential denial sent grant/challenge bytes"
    );
}
