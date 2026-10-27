use lamp_ipc::camera_authority::{HoldWitness, ReceiveGrant, Ticket, Token};
use rustix::io::Errno;
use rustix::net::{
    AddressFamily, RecvFlags, SendAncillaryBuffer, SendAncillaryMessage, SendFlags, SocketType,
    recv, sendmsg, socket,
};
use std::{
    io::{self, IoSlice, Write},
    mem::MaybeUninit,
    net::Shutdown,
    num::NonZeroU64,
    os::fd::{AsFd, BorrowedFd},
    os::unix::net::{UnixDatagram, UnixStream},
    sync::Arc,
    time::{Duration, Instant},
};

fn token() -> Token {
    Token::new([7; 16], 1).unwrap()
}
fn nonce() -> NonZeroU64 {
    NonZeroU64::new(11).unwrap()
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}
fn bytes() -> [u8; 48] {
    let mut bytes = [0; 48];
    bytes[..8].copy_from_slice(b"LMPCTK03");
    bytes[8..24].copy_from_slice(&[7; 16]);
    bytes[24..32].copy_from_slice(&1u64.to_be_bytes());
    bytes[32..40].copy_from_slice(&11u64.to_be_bytes());
    bytes
}
fn finish(receiver: &mut ReceiveGrant, control: &UnixStream) -> io::Result<Ticket> {
    let until = deadline();
    loop {
        if let Some(ticket) = receiver.try_receive(control)? {
            return Ok(ticket);
        }
        let left = until
            .checked_duration_since(Instant::now())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "fixture receiver timeout"))?;
        if !lamp_ipc::readable(control, left)? {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "fixture not readable",
            ));
        }
    }
}
fn ticket(witness: &HoldWitness) -> Ticket {
    let (send, receive) = UnixStream::pair().unwrap();
    let mut grant = witness.grant(nonce(), deadline()).unwrap();
    assert!(grant.try_send(&send).unwrap());
    let mut receiver = ReceiveGrant::new(token(), nonce(), deadline());
    finish(&mut receiver, &receive).unwrap()
}
fn raw_send(control: &UnixStream, bytes: &[u8], rights: &[BorrowedFd<'_>]) {
    let mut storage = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(32))];
    let mut ancillary = SendAncillaryBuffer::new(&mut storage);
    if !rights.is_empty() {
        assert!(ancillary.push(SendAncillaryMessage::ScmRights(rights)));
    }
    let flags = SendFlags::DONTWAIT;
    #[cfg(target_os = "linux")]
    let flags = flags | SendFlags::NOSIGNAL;
    #[cfg(target_os = "macos")]
    rustix::net::sockopt::set_socket_nosigpipe(control, true).unwrap();
    assert_eq!(
        sendmsg(control, &[IoSlice::new(bytes)], &mut ancillary, flags).unwrap(),
        bytes.len()
    );
}
fn peer_eof(peer: &UnixStream) {
    let mut byte = [0u8; 1];
    assert!(
        matches!(recv(peer, &mut byte[..], RecvFlags::DONTWAIT), Ok((_, 0))),
        "rejected transferred FD leaked"
    );
}

#[test]
fn one_revocation_closes_two_independently_transferred_observers() {
    let witness = HoldWitness::new(token()).unwrap();
    let first = ticket(&witness);
    let second = ticket(&witness);
    assert!(first.is_open());
    assert!(second.is_open());
    // A PEEK does not consume or change authority, even across repeated reads.
    for _ in 0..3 {
        assert!(first.is_open() && second.is_open());
    }
    witness.revoke().unwrap();
    assert!(!first.is_open());
    assert!(!second.is_open());
}

#[test]
fn dropping_one_client_does_not_shutdown_the_shared_endpoint() {
    let witness = HoldWitness::new(token()).unwrap();
    let first = ticket(&witness);
    let second = ticket(&witness);
    drop(first);
    assert!(second.is_open());
    let third = ticket(&witness);
    assert!(third.is_open());
    witness.revoke().unwrap();
    assert!(!second.is_open() && !third.is_open());
}

#[test]
fn old_ticket_never_revives_with_a_new_hold() {
    let old = HoldWitness::new(token()).unwrap();
    let old_ticket = ticket(&old);
    old.revoke().unwrap();
    let new_token = token().next().unwrap();
    let new = HoldWitness::new(new_token).unwrap();
    let (send, receive) = UnixStream::pair().unwrap();
    assert!(
        new.grant(nonce(), deadline())
            .unwrap()
            .try_send(&send)
            .unwrap()
    );
    let new_ticket = finish(
        &mut ReceiveGrant::new(new_token, nonce(), deadline()),
        &receive,
    )
    .unwrap();
    assert!(new_ticket.is_open());
    assert!(!old_ticket.is_open());
}

#[test]
fn revoke_before_grant_refuses_and_pinned_arc_does_not_delay_explicit_revoke() {
    let witness = Arc::new(HoldWitness::new(token()).unwrap());
    let pinned = witness.clone();
    let existing = ticket(&witness);
    witness.revoke().unwrap();
    assert!(pinned.grant(nonce(), deadline()).is_err());
    assert!(!existing.is_open());
}

#[test]
fn revoke_during_prepared_grant_transfers_only_a_terminal_observer() {
    let witness = HoldWitness::new(token()).unwrap();
    let mut grant = witness.grant(nonce(), deadline()).unwrap();
    witness.revoke().unwrap();
    let (send, receive) = UnixStream::pair().unwrap();
    assert!(grant.try_send(&send).unwrap());
    assert!(
        finish(
            &mut ReceiveGrant::new(token(), nonce(), deadline()),
            &receive
        )
        .is_err()
    );
}

#[test]
fn witness_drop_revokes_even_when_a_prepared_grant_keeps_an_observer() {
    let witness = HoldWitness::new(token()).unwrap();
    let existing = ticket(&witness);
    let mut grant = witness.grant(nonce(), deadline()).unwrap();
    drop(witness);
    assert!(!existing.is_open());
    let (send, receive) = UnixStream::pair().unwrap();
    assert!(grant.try_send(&send).unwrap());
    assert!(
        finish(
            &mut ReceiveGrant::new(token(), nonce(), deadline()),
            &receive
        )
        .is_err()
    );
}

#[test]
fn incremental_frame_retains_fd_until_exact_frame_and_control_eof() {
    let (send, receive) = UnixStream::pair().unwrap();
    let (_revoker, observer) = UnixStream::pair().unwrap();
    let mut receiver = ReceiveGrant::new(token(), nonce(), deadline());
    assert!(receiver.try_receive(&receive).unwrap().is_none());
    raw_send(&send, &bytes()[..9], &[observer.as_fd()]);
    assert!(receiver.try_receive(&receive).unwrap().is_none());
    (&send).write_all(&bytes()[9..]).unwrap();
    assert!(receiver.try_receive(&receive).unwrap().is_none());
    // A complete frame without EOF remains Pending: trailing bytes cannot hide.
    assert!(receiver.try_receive(&receive).unwrap().is_none());
    send.shutdown(Shutdown::Write).unwrap();
    assert!(finish(&mut receiver, &receive).unwrap().is_open());
}

#[test]
fn wrong_binding_magic_reserved_size_and_trailing_bytes_refuse_and_close_rights() {
    let mut cases = Vec::new();
    for offset in [0, 7, 8, 24, 32, 40, 47] {
        let mut changed = bytes().to_vec();
        changed[offset] ^= 1;
        cases.push(changed);
    }
    cases.push(bytes()[..47].to_vec());
    let mut oversized = bytes().to_vec();
    oversized.push(1);
    cases.push(oversized);
    for frame in cases {
        let (send, receive) = UnixStream::pair().unwrap();
        let (probe, observer) = UnixStream::pair().unwrap();
        raw_send(&send, &frame, &[observer.as_fd()]);
        drop(observer);
        send.shutdown(Shutdown::Write).unwrap();
        let mut receiver = ReceiveGrant::new(token(), nonce(), deadline());
        assert!(finish(&mut receiver, &receive).is_err());
        peer_eof(&probe);
        assert!(
            receiver.try_receive(&receive).is_err(),
            "errors must remain terminal"
        );
    }
}

#[test]
fn missing_extra_and_truncated_rights_refuse_without_leaking_descriptors() {
    for count in [0, 2, 8] {
        let (send, receive) = UnixStream::pair().unwrap();
        let pairs: Vec<_> = (0..count).map(|_| UnixStream::pair().unwrap()).collect();
        let rights: Vec<_> = pairs.iter().map(|(_, observer)| observer.as_fd()).collect();
        raw_send(&send, &bytes(), &rights);
        drop(rights);
        let probes: Vec<_> = pairs
            .into_iter()
            .map(|(probe, observer)| {
                drop(observer);
                probe
            })
            .collect();
        send.shutdown(Shutdown::Write).unwrap();
        assert!(
            finish(
                &mut ReceiveGrant::new(token(), nonce(), deadline()),
                &receive
            )
            .is_err()
        );
        for probe in probes {
            peer_eof(&probe);
        }
    }
    // Use the exact production buffer. Darwin fully receives and rejects these
    // excess rights; Linux's two-FD buffer actually truncates. One shared
    // observer and peer EOF prove every received/kernel-discarded copy closed,
    // without opening hundreds of files or counting unrelated descriptors.
    for count in [8, 32] {
        use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, ReturnFlags, recvmsg};
        let (send, receive) = UnixStream::pair().unwrap();
        let (probe, observer) = UnixStream::pair().unwrap();
        #[cfg(target_os = "macos")]
        assert!(
            rustix::net::getpeername(&observer).unwrap().is_none(),
            "Darwin connected unnamed peer is a successful absent address"
        );
        let rights = vec![observer.as_fd(); count];
        raw_send(&send, &bytes(), &rights);
        drop(rights);
        drop(observer);
        let mut data = [0u8; 49];
        let mut storage = [MaybeUninit::uninit();
            rustix::cmsg_space!(ScmRights(lamp_ipc::camera_authority::CONTROL_FD_CAPACITY))];
        let mut ancillary = RecvAncillaryBuffer::new(&mut storage);
        let flags = RecvFlags::DONTWAIT;
        #[cfg(target_os = "linux")]
        let flags = flags | RecvFlags::CMSG_CLOEXEC;
        let received = recvmsg(
            &receive,
            &mut [std::io::IoSliceMut::new(&mut data)],
            &mut ancillary,
            flags,
        )
        .unwrap();
        assert_eq!(received.bytes, 48);
        assert_eq!(
            received.flags.contains(ReturnFlags::CTRUNC),
            !cfg!(target_os = "macos")
        );
        let mut received_fds = 0;
        for message in ancillary.drain() {
            match message {
                RecvAncillaryMessage::ScmRights(fds) => {
                    for fd in fds {
                        received_fds += 1;
                        drop(fd);
                    }
                }
                _ => panic!("unexpected ancillary kind"),
            }
        }
        if cfg!(target_os = "macos") {
            assert_eq!(received_fds, count);
        } else {
            assert!(received_fds > 0 && received_fds < count);
        }
        peer_eof(&probe);
    }
}

#[test]
fn wrong_fd_kinds_and_unconnected_stream_refuse() {
    let (pipe, _writer) = rustix::pipe::pipe().unwrap();
    let datagram = UnixDatagram::unbound().unwrap();
    let unconnected = socket(AddressFamily::UNIX, SocketType::STREAM, None).unwrap();
    let internet = socket(AddressFamily::INET, SocketType::STREAM, None).unwrap();
    for fd in [
        pipe.as_fd(),
        datagram.as_fd(),
        unconnected.as_fd(),
        internet.as_fd(),
    ] {
        let (send, receive) = UnixStream::pair().unwrap();
        raw_send(&send, &bytes(), &[fd]);
        send.shutdown(Shutdown::Write).unwrap();
        assert!(
            finish(
                &mut ReceiveGrant::new(token(), nonce(), deadline()),
                &receive
            )
            .is_err()
        );
    }
    // The control argument's Rust type alone cannot establish the native UNIX
    // bound: safe From<OwnedFd> can wrap a different socket family.
    let wrong_control =
        UnixStream::from(socket(AddressFamily::INET, SocketType::STREAM, None).unwrap());
    let mut receiver = ReceiveGrant::new(token(), nonce(), deadline());
    assert!(receiver.try_receive(&wrong_control).is_err());
}

#[test]
fn cancellation_drops_a_retained_partial_frame_fd() {
    let (send, receive) = UnixStream::pair().unwrap();
    let (probe, observer) = UnixStream::pair().unwrap();
    raw_send(&send, &bytes()[..1], &[observer.as_fd()]);
    drop(observer);
    let mut receiver = ReceiveGrant::new(token(), nonce(), deadline());
    assert!(receiver.try_receive(&receive).unwrap().is_none());
    drop(receiver);
    peer_eof(&probe);
}

#[test]
fn deadline_does_not_renew_while_waiting_for_reply() {
    let (send, receive) = UnixStream::pair().unwrap();
    let expired = Instant::now() - Duration::from_millis(1);
    let witness = HoldWitness::new(token()).unwrap();
    assert!(witness.grant(nonce(), expired).is_err());
    let mut receiver = ReceiveGrant::new(token(), nonce(), expired);
    assert_eq!(
        receiver.try_receive(&receive).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    drop(send);
}

#[test]
fn unexpected_data_never_disappears_between_observers() {
    let (send, receive) = UnixStream::pair().unwrap();
    let (mut revoker, observer) = UnixStream::pair().unwrap();
    raw_send(&send, &bytes(), &[observer.as_fd()]);
    send.shutdown(Shutdown::Write).unwrap();
    let ticket = finish(
        &mut ReceiveGrant::new(token(), nonce(), deadline()),
        &receive,
    )
    .unwrap();
    assert!(ticket.is_open());
    revoker.write_all(b"x").unwrap();
    assert!(!ticket.is_open());
    let mut byte = [0u8; 1];
    assert!(matches!(
        recv(&observer, &mut byte[..], RecvFlags::DONTWAIT),
        Ok((_, 1))
    ));
    assert!(
        !ticket.is_open(),
        "loss is sticky even after byte is consumed elsewhere"
    );
}

#[test]
fn token_refuses_empty_incarnation_zero_hold_and_wraparound() {
    assert!(Token::new([0; 16], 1).is_none());
    assert!(Token::new([1; 16], 0).is_none());
    assert!(Token::new([1; 16], u64::MAX).unwrap().next().is_none());
    // This local witness has no camera/source-age inputs: openness alone must
    // never be advertised as proving expiry, a person or captured-frame origin.
    let witness = HoldWitness::new(token()).unwrap();
    assert!(ticket(&witness).is_open());
}

#[test]
fn completed_frame_with_closed_or_nonsilent_observer_never_exposes_ready() {
    for payload in [false, true] {
        let (send, receive) = UnixStream::pair().unwrap();
        let (mut revoker, observer) = UnixStream::pair().unwrap();
        raw_send(&send, &bytes(), &[observer.as_fd()]);
        let mut receiver = ReceiveGrant::new(token(), nonce(), deadline());
        assert!(receiver.try_receive(&receive).unwrap().is_none());
        if payload {
            revoker.write_all(b"x").unwrap();
        } else {
            revoker.shutdown(Shutdown::Both).unwrap();
        }
        send.shutdown(Shutdown::Write).unwrap();
        assert!(finish(&mut receiver, &receive).is_err());
    }
}

// Fixture-only rescue: cancel/join on success and unwinding; never touches a
// witness observer. An old blocking send is released by its control peer EOF.
struct ControlRescue {
    cancel: std::sync::mpsc::Sender<()>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl ControlRescue {
    fn new(peer: &UnixStream) -> Self {
        let peer = peer.try_clone().unwrap();
        let (cancel, receive) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            if matches!(
                receive.recv_timeout(Duration::from_secs(3)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ) {
                let _ = peer.shutdown(Shutdown::Both);
            }
        });
        Self {
            cancel,
            worker: Some(worker),
        }
    }
}
impl Drop for ControlRescue {
    fn drop(&mut self) {
        let _ = self.cancel.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[test]
fn full_control_socket_keeps_grant_pending_until_its_original_deadline() {
    let witness = HoldWitness::new(token()).unwrap();
    let existing = ticket(&witness);
    let (send, receive) = UnixStream::pair().unwrap();
    let _rescue = ControlRescue::new(&receive);
    send.set_nonblocking(true).unwrap();
    rustix::net::sockopt::set_socket_send_buffer_size(&send, 4096).unwrap();
    let flags = SendFlags::DONTWAIT;
    #[cfg(target_os = "linux")]
    let flags = flags | SendFlags::NOSIGNAL;
    #[cfg(target_os = "macos")]
    rustix::net::sockopt::set_socket_nosigpipe(&send, true).unwrap();
    let filler = [0u8; 1];
    let mut full = false;
    // Fill a real control socket without reading its peer. Both each syscall
    // and this fixture's admission work are bounded; no forced short-write shim.
    for _ in 0..65_536 {
        let mut ancillary = SendAncillaryBuffer::default();
        match sendmsg(&send, &[IoSlice::new(&filler)], &mut ancillary, flags) {
            Ok(count) => assert!(count > 0),
            Err(Errno::AGAIN) => {
                full = true;
                break;
            }
            Err(Errno::INTR) => {}
            Err(error) => panic!("fill control socket: {error}"),
        }
    }
    assert!(full, "bounded socket fill never reached backpressure");
    // Undo fixture setup: the API must establish control nonblocking mode,
    // rather than passing only because its caller happened to configure it.
    send.set_nonblocking(false).unwrap();
    let until = Instant::now() + Duration::from_secs(1);
    let mut grant = witness.grant(nonce(), until).unwrap();
    assert!(
        !grant.try_send(&send).unwrap(),
        "full control socket must stay Pending"
    );
    assert!(existing.is_open(), "backpressure grants no global revoke");
    // The production method never waits. Only this test crosses the fixed
    // original deadline, with no short first-poll timing assumption or retry.
    std::thread::sleep(until.saturating_duration_since(Instant::now()));
    assert_eq!(
        grant.try_send(&send).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    assert!(
        grant.try_send(&send).is_err(),
        "expired transfer is terminal"
    );
    drop(grant);
    drop(send);
    drop(receive);
    drop(witness);
    assert!(!existing.is_open());
    // Sender cleanup closes its observer duplicate by RAII. Its private FD is
    // intentionally unavailable to the fixture; no global FD count is used.
}

#[test]
fn partial_receive_deadline_immediately_closes_retained_rights() {
    let (send, receive) = UnixStream::pair().unwrap();
    let (probe, observer) = UnixStream::pair().unwrap();
    let until = Instant::now() + Duration::from_secs(1);
    let mut receiver = ReceiveGrant::new(token(), nonce(), until);
    raw_send(&send, &bytes()[..1], &[observer.as_fd()]);
    drop(observer);
    assert!(receiver.try_receive(&receive).unwrap().is_none());
    let mut byte = [0u8; 1];
    assert!(
        matches!(
            recv(&probe, &mut byte[..], RecvFlags::DONTWAIT),
            Err(Errno::AGAIN)
        ),
        "partial receiver must still own its received descriptor"
    );
    std::thread::sleep(until.saturating_duration_since(Instant::now()));
    assert_eq!(
        receiver.try_receive(&receive).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    // The receiver remains alive here: EOF proves error cleanup immediately
    // closed the retained duplicate, rather than waiting for Receiver Drop.
    peer_eof(&probe);
    (&send).write_all(&bytes()[1..]).unwrap();
    send.shutdown(Shutdown::Write).unwrap();
    assert!(
        receiver.try_receive(&receive).is_err(),
        "late completion cannot renew expiry"
    );
    peer_eof(&probe);
}

#[test]
fn full_control_recovers_one_grant_after_exact_drain_without_renewing_deadline() {
    use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, recvmsg};
    let witness = HoldWitness::new(token()).unwrap();
    // Obtain a separate SCM_RIGHTS duplicate of the SAME observer solely to
    // inspect shared open-file flags; no test-only production accessor needed.
    let (flag_send, flag_receive) = UnixStream::pair().unwrap();
    assert!(
        witness
            .grant(nonce(), deadline())
            .unwrap()
            .try_send(&flag_send)
            .unwrap()
    );
    let mut flag_bytes = [0; 48];
    let mut flag_storage = [MaybeUninit::uninit();
        rustix::cmsg_space!(ScmRights(lamp_ipc::camera_authority::CONTROL_FD_CAPACITY))];
    let mut ancillary = RecvAncillaryBuffer::new(&mut flag_storage);
    assert_eq!(
        recvmsg(
            &flag_receive,
            &mut [std::io::IoSliceMut::new(&mut flag_bytes)],
            &mut ancillary,
            RecvFlags::DONTWAIT
        )
        .unwrap()
        .bytes,
        48
    );
    assert_eq!(flag_bytes, bytes());
    let mut observer = None;
    for message in ancillary.drain() {
        match message {
            RecvAncillaryMessage::ScmRights(fds) => {
                for fd in fds {
                    assert!(observer.replace(fd).is_none());
                }
            }
            _ => panic!("unexpected flag-probe ancillary"),
        }
    }
    let observer = observer.unwrap();
    rustix::io::fcntl_setfd(&observer, rustix::io::FdFlags::CLOEXEC).unwrap();
    let original_flags = rustix::fs::fcntl_getfl(&observer).unwrap();
    let existing = ticket(&witness);
    let (send, receive) = UnixStream::pair().unwrap();
    let _rescue = ControlRescue::new(&receive);
    send.set_nonblocking(true).unwrap();
    rustix::net::sockopt::set_socket_send_buffer_size(&send, 4096).unwrap();
    let flags = SendFlags::DONTWAIT;
    #[cfg(target_os = "linux")]
    let flags = flags | SendFlags::NOSIGNAL;
    #[cfg(target_os = "macos")]
    rustix::net::sockopt::set_socket_nosigpipe(&send, true).unwrap();
    let mut filled = 0;
    let mut full = false;
    for _ in 0..65_536 {
        let mut ancillary = SendAncillaryBuffer::default();
        match sendmsg(&send, &[IoSlice::new(&[0])], &mut ancillary, flags) {
            Ok(n) => {
                assert_eq!(n, 1);
                filled += n;
            }
            Err(Errno::AGAIN) => {
                full = true;
                break;
            }
            Err(Errno::INTR) => {}
            Err(error) => panic!("bounded fill: {error}"),
        }
    }
    assert!(full && filled > 0);
    send.set_nonblocking(false).unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    let mut grant = witness.grant(nonce(), until).unwrap();
    assert!(
        !grant.try_send(&send).unwrap(),
        "initial full control must stay Pending"
    );
    let mut drained = 0;
    let mut data = [0; 1024];
    while drained < filled {
        assert!(
            Instant::now() < until,
            "exact filler drain keeps original grant deadline"
        );
        let count = (filled - drained).min(data.len());
        match recv(&receive, &mut data[..count], RecvFlags::DONTWAIT) {
            Ok((_, n)) => {
                assert!(n > 0);
                assert!(data[..n].iter().all(|byte| *byte == 0));
                drained += n;
            }
            Err(Errno::INTR) => {}
            Err(error) => panic!("filler was sent but unreadable: {error}"),
        }
    }
    assert_eq!(drained, filled);
    // Rejected firstsend must not have smuggled frame bytes or any FD after
    // the exact filler. Safe ancillary ownership also cleans unexpected rights
    // if this assertion fails; no raw/global descriptor inspection.
    let mut empty_storage = [MaybeUninit::uninit();
        rustix::cmsg_space!(ScmRights(lamp_ipc::camera_authority::CONTROL_FD_CAPACITY))];
    let mut empty = RecvAncillaryBuffer::new(&mut empty_storage);
    assert!(matches!(
        recvmsg(
            &receive,
            &mut [std::io::IoSliceMut::new(&mut data)],
            &mut empty,
            RecvFlags::DONTWAIT
        ),
        Err(Errno::AGAIN)
    ));
    assert!(
        grant.try_send(&send).unwrap(),
        "same grant must recover after peer drain"
    );
    assert!(
        grant.try_send(&send).is_err(),
        "rights/frame complete exactly once"
    );
    let mut receiver = ReceiveGrant::new(token(), nonce(), until);
    let received = finish(&mut receiver, &receive).unwrap();
    assert!(Instant::now() < until);
    assert!(received.is_open() && existing.is_open());
    assert!(receiver.try_receive(&receive).is_err());
    assert_eq!(
        rustix::fs::fcntl_getfl(&observer)
            .unwrap()
            .contains(rustix::fs::OFlags::NONBLOCK),
        original_flags.contains(rustix::fs::OFlags::NONBLOCK),
        "control-mode setup never changes shared observer NONBLOCK"
    );
    drop(observer);
    assert!(received.is_open());
    drop(witness);
    assert!(!received.is_open() && !existing.is_open());
}

#[test]
fn named_control_eof_after_both_write_halves_close_delivers_live_observers() {
    use std::{
        fs::{self, DirBuilder, Permissions},
        io::Read,
        os::unix::{
            fs::{DirBuilderExt, PermissionsExt},
            net::UnixListener,
        },
        path::PathBuf,
    };
    struct Directory(PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_file(self.0.join("grant.sock"));
            let _ = fs::remove_dir(&self.0);
        }
    }
    let root = PathBuf::from("/tmp").join(format!(
        "ct-{}-{}",
        std::process::id(),
        lamp_ipc::monotonic_ns()
    ));
    DirBuilder::new().mode(0o700).create(&root).unwrap();
    let directory = Directory(root);
    let path = directory.0.canonicalize().unwrap().join("grant.sock");
    let listener = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let witness = HoldWitness::new(token()).unwrap();
    let until = deadline();
    let remaining = || {
        until
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .expect("original fixture deadline expired")
    };
    let mut tickets = Vec::new();
    for value in [17, 18] {
        let mut client = UnixStream::connect(&path).unwrap();
        assert!(client.local_addr().unwrap().as_pathname().is_none());
        let (mut server, _) = listener.accept().unwrap();
        server.set_nonblocking(false).unwrap();
        client.set_write_timeout(Some(remaining())).unwrap();
        client.write_all(b"Q").unwrap();
        client.shutdown(Shutdown::Write).unwrap();
        server.set_read_timeout(Some(remaining())).unwrap();
        let mut request = [0];
        server.read_exact(&mut request).unwrap();
        assert_eq!(request, [b'Q']);
        server.set_read_timeout(Some(remaining())).unwrap();
        assert_eq!(server.read(&mut request).unwrap(), 0);
        let nonce = NonZeroU64::new(value).unwrap();
        let mut grant = witness.grant(nonce, until).unwrap();
        while !grant.try_send(&server).unwrap() {
            // A legitimate Pending step never starts a new transfer budget.
            std::thread::sleep(Duration::from_millis(1).min(remaining()));
        }
        // Grant already half-closed CONTROL write. Both halves are closed,
        // then the server disappears BEFORE the receiver's very first poll.
        drop(server);
        let mut receiver = ReceiveGrant::new(token(), nonce, until);
        let ticket = loop {
            if let Some(ticket) = receiver.try_receive(&client).unwrap() {
                break ticket;
            }
            assert!(lamp_ipc::readable(&client, remaining()).unwrap());
        };
        assert!(ticket.is_open(), "control EOF is not observer revocation");
        tickets.push(ticket);
    }
    assert_eq!(tickets.len(), 2);
    assert!(tickets.iter().all(Ticket::is_open));
    for wrong in [
        socket(AddressFamily::UNIX, SocketType::STREAM, None).unwrap(),
        socket(AddressFamily::INET, SocketType::STREAM, None).unwrap(),
        socket(AddressFamily::UNIX, SocketType::DGRAM, None).unwrap(),
    ] {
        let control = UnixStream::from(wrong);
        let mut receiver = ReceiveGrant::new(token(), nonce(), until);
        let error = receiver.try_receive(&control).unwrap_err();
        assert_ne!(error.kind(), io::ErrorKind::TimedOut);
        let mut grant = witness.grant(nonce(), until).unwrap();
        let error = grant.try_send(&control).unwrap_err();
        assert_ne!(error.kind(), io::ErrorKind::TimedOut);
        assert!(tickets.iter().all(Ticket::is_open));
    }
    witness.revoke().unwrap();
    assert!(tickets.iter().all(|ticket| !ticket.is_open()));
    assert!(
        Instant::now() < until,
        "the original two-second bound stays"
    );
}
